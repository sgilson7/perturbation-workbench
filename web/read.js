// Extracting an assignment PDF into the lines `core` splits.
//
// Division of labour, the same as pdf-redactor's: pdf.js parses, Rust decides.
// Nothing here works out what a question is, what a heading is, or what is page
// furniture — those are decisions that have to be correct, so they live in
// `crates/core/src/ingest.rs` under `cargo test`. This file turns a PDF into a
// list of `{ text, page, mono }` and stops.
//
// pdf.js is vendored, not loaded from a CDN. The tool's claim is that no
// network request happens after the page loads, and a CDN would make that claim
// false in the most literal way possible.

import * as pdfjs from './vendor/pdfjs/pdf.mjs';

// Resolved against this module rather than against the document. A bare
// './vendor/...' only works when the page sits at the web root, and pdf.js
// resolves the worker path against its own URL anyway, so the two disagree the
// moment anything loads the module from elsewhere — a test harness, say. The
// failure is a doubled path in a fetch nobody was watching.
const VENDOR = new URL('./vendor/pdfjs/', import.meta.url);
pdfjs.GlobalWorkerOptions.workerSrc = new URL('pdf.worker.mjs', VENDOR).href;
const ASSETS = {
  cMapUrl: new URL('cmaps/', VENDOR).href,
  cMapPacked: true,
  standardFontDataUrl: new URL('standard_fonts/', VENDOR).href,
};

// Two text items belong to the same line when their baselines are within this
// fraction of the line height. A fixed pixel tolerance breaks on a document
// typeset at a different size; superscripts and subscripts sit within it, which
// is what we want, since a footnote marker is part of the line it marks.
const SAME_LINE = 0.5;

/// Read a document: its digest, its page count, and its lines.
///
/// One entry point rather than three, because the order matters and getting it
/// wrong is silent. `getDocument` takes ownership of the array it is handed and
/// detaches it, so hashing the same array afterwards hashes a zero-length
/// buffer — and every run would record the SHA-256 of nothing as its input
/// document. The digest is taken first, and pdf.js gets a copy.
export async function read(bytes) {
  const sha256 = await digest(bytes);
  const doc = await pdfjs.getDocument({ data: bytes.slice(), ...ASSETS }).promise;
  try {
    return { sha256, pages: doc.numPages, lines: await lines(doc) };
  } finally {
    doc.destroy();
  }
}

/// Read one page into lines, in reading order.
///
/// pdf.js emits text in the order the content stream paints it, which is
/// usually reading order and is not guaranteed to be. Sorting by baseline and
/// then by x costs nothing and makes the splitter's job deterministic across
/// producers.
async function readPage(page, pageNumber) {
  const tc = await page.getTextContent();
  const vp = page.getViewport({ scale: 1 });

  const items = [];
  for (const it of tc.items) {
    if (!it.str) continue;
    const tx = pdfjs.Util.transform(vp.transform, it.transform);
    const h = Math.hypot(tx[2], tx[3]) || 1;
    items.push({
      text: it.str,
      x: tx[4],
      y: tx[5],
      h,
      // pdf.js classifies each font into a CSS family. A PDF has no idea it
      // contains code; this is the closest thing to a signal that it does, and
      // in a CS assignment it is very nearly the same question.
      mono: tc.styles?.[it.fontName]?.fontFamily === 'monospace',
      eol: !!it.hasEOL,
    });
  }

  items.sort((a, b) => (Math.abs(a.y - b.y) > a.h * SAME_LINE ? a.y - b.y : a.x - b.x));

  const lines = [];
  let cur = null;
  for (const it of items) {
    if (cur && Math.abs(it.y - cur.y) <= cur.h * SAME_LINE) {
      // pdf.js splits a line at arbitrary points, and whether a gap is a space
      // is a question about geometry, not about the strings. It is only a
      // question at all when neither side already has one: pdf.js hands back
      // the space as leading whitespace on the next item about as often as it
      // drops it, and adding a second turns "public static" into
      // "public  static" throughout every code block.
      const spaced = cur.text.endsWith(' ') || it.text.startsWith(' ');
      const gap = it.x - cur.right;
      cur.text += gap > it.h * 0.2 && !spaced ? ' ' + it.text : it.text;
      cur.right = it.x + (it.width || 0);
      cur.mono = cur.mono && it.mono;
    } else {
      if (cur) lines.push(cur);
      cur = { text: it.text, page: pageNumber, mono: it.mono, y: it.y, h: it.h, x: it.x,
              right: it.x + (it.width || 0) };
    }
    if (it.eol) {
      lines.push(cur);
      cur = null;
    }
  }
  if (cur) lines.push(cur);

  // Restore the indentation of code, geometrically.
  //
  // Leading whitespace does not survive extraction — pdf.js expresses it as a
  // starting x rather than as characters, and on some producers drops it
  // outright. For prose that costs nothing. For a code block it is the
  // meaning: a method body flush against the margin is not the question that
  // was asked. So the indent is measured against the leftmost text on the page
  // and converted back into spaces at Courier's fixed pitch, which is exact
  // for the fixed-pitch fonts this applies to.
  const left = Math.min(...lines.map((l) => l.x), Infinity);
  for (const l of lines) {
    if (!l.mono || !Number.isFinite(left)) continue;
    const indent = Math.round((l.x - left) / (l.h * 0.6));
    if (indent > 0) l.text = ' '.repeat(indent) + l.text;
  }

  return lines
    .map((l) => ({ text: l.text.trimEnd(), page: l.page, mono: l.mono }))
    .filter((l) => l.text.trim() !== '');
}

/// Every line of a document, ready for `ingest`.
async function lines(doc) {
  const out = [];
  for (let p = 1; p <= doc.numPages; p++) {
    const page = await doc.getPage(p);
    out.push(...(await readPage(page, p)));
    page.cleanup();
  }
  return out;
}

/// SHA-256 of the bytes, which is how a document is identified everywhere else.
///
/// In the browser rather than in Rust only because the response digests are
/// too: SubtleCrypto is already here, and moving a 5 MB PDF across the wasm
/// boundary to hash it would cost a copy for nothing. Query text is hashed in
/// `core`, where it matters that the canonicalisation rule is under test.
async function digest(bytes) {
  const out = await crypto.subtle.digest('SHA-256', bytes);
  return [...new Uint8Array(out)].map((b) => b.toString(16).padStart(2, '0')).join('');
}
