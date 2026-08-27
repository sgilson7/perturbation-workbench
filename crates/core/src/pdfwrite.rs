//! Minimal PDF writer.
//!
//! Ported from pdf-redactor and reduced: no images, no invisible text layer,
//! just typeset pages. It exists instead of a PDF library for the reason the
//! redactor gives — the claim being made is "the output contains nothing but
//! the bytes we chose to write", and a general-purpose library gives you no way
//! to *prove* that, because any version bump can start emitting a new key. The
//! entire output is visible in one screenful of `write_*` calls.
//!
//! The subset emitted is deliberately tiny:
//!   catalog -> page tree -> N pages
//!   each page: one uncompressed content stream of positioned text and rules,
//!              drawn in four base-14 fonts.
//!
//! Content streams are left uncompressed, as they are in the redactor. It costs
//! a few KB and buys the ability to check the tool's work without the tool:
//!
//! ```sh
//! strings assignment.pdf | grep -i "bijection"
//! ```
//!
//! No `/Info`, no XMP, no `/ID`. An assignment carries the instructor's name in
//! its metadata by default in every word processor, and this one is going to be
//! handed to a class.
//!
//! ## Why four fonts and no embedding
//!
//! Base-14 fonts are the ones every reader already has, so nothing is embedded
//! and no font program can ride along into the output. Helvetica and
//! Helvetica-Bold set the prose, Courier sets code — a code block reflowed into
//! a proportional face with its indentation collapsed is not the question that
//! was asked — and **Symbol** carries the mathematics.
//!
//! That last one matters more than it sounds. WinAnsiEncoding cannot represent
//! `∪`, `∈`, `∀` or `Θ`, and a discrete-mathematics assignment is made of them.
//! The plan says to emit maths as plain Unicode; that is not possible in a
//! base-14 font, so the writer switches into Symbol mid-line for the characters
//! Symbol has. It is the one way to typeset `A ∪ B` correctly without embedding
//! a font program, and it is why the alternative — transliterating to
//! "A union B" — was not taken.
//!
//! Anything Symbol does not have either is transliterated to readable ASCII and
//! **counted**, so the caller can say how much was approximated rather than
//! letting it pass silently. The exact query files carry the original UTF-8; the
//! PDF is the readable rendering, and the `.txt` is the evidence.

use crate::metrics::Font;

/// One run of text in a single font.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub text: String,
    pub font: Font,
}

impl Run {
    pub fn new(text: impl Into<String>, font: Font) -> Run {
        Run { text: text.into(), font }
    }
}

/// One typeset line, positioned in PDF user space (origin bottom-left, y up).
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub runs: Vec<Run>,
    pub x: f32,
    pub y: f32,
    pub size: f32,
    /// 0.0 is black. Used for the muted grey of metadata lines.
    pub grey: f32,
}

/// A horizontal rule, for separating one question from the next.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rule {
    pub x0: f32,
    pub x1: f32,
    pub y: f32,
    pub grey: f32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Page {
    pub lines: Vec<Line>,
    pub rules: Vec<Rule>,
}

/// US Letter, in points.
pub const PAGE_W: f32 = 612.0;
pub const PAGE_H: f32 = 792.0;

/// Where a character can be drawn.
enum Slot {
    /// A byte in WinAnsiEncoding, in the run's own font.
    Win(u8),
    /// A byte in the Symbol font.
    Sym(u8),
    /// Not available in any base-14 font; this is the readable substitute.
    Fallback(&'static str),
    /// Draws nothing and is not an approximation: a line break, a zero-width
    /// mark, a soft hyphen.
    Skip,
}

/// Symbol's encoding for the mathematics a CS assignment actually uses.
///
/// Not a complete table on purpose: every entry here is a character that has
/// turned up in a discrete-mathematics, logic, or algorithms question. An
/// unlisted character falls through to a transliteration and is counted, which
/// is a better failure than a silently blank glyph.
fn symbol(c: char) -> Option<u8> {
    Some(match c {
        '∀' => 0x22, '∃' => 0x24, '∍' => 0x27, '≅' => 0x40, '∴' => 0x5C,
        '≤' => 0xA3, '∞' => 0xA5, '↔' => 0xAB, '←' => 0xAC, '↑' => 0xAD,
        '→' => 0xAE, '↓' => 0xAF, '°' => 0xB0, '±' => 0xB1, '≥' => 0xB3,
        '×' => 0xB4, '∝' => 0xB5, '∂' => 0xB6, '÷' => 0xB8, '≠' => 0xB9,
        '≡' => 0xBA, '≈' => 0xBB, 'ℵ' => 0xC0, '⊗' => 0xC4, '⊕' => 0xC5,
        '∅' => 0xC6, '∩' => 0xC7, '∪' => 0xC8, '⊃' => 0xC9, '⊇' => 0xCA,
        '⊄' => 0xCB, '⊂' => 0xCC, '⊆' => 0xCD, '∈' => 0xCE, '∉' => 0xCF,
        '∠' => 0xD0, '∇' => 0xD1, '∏' => 0xD5, '√' => 0xD6, '⋅' => 0xD7,
        '¬' => 0xD8, '∧' => 0xD9, '∨' => 0xDA, '⇔' => 0xDB, '⇐' => 0xDC,
        '⇑' => 0xDD, '⇒' => 0xDE, '⇓' => 0xDF, '∑' => 0xE5, '∫' => 0xF2,
        // Greek, lower case then upper. Symbol orders these by Latin
        // transliteration rather than by the Greek alphabet, which is why the
        // codes look arbitrary.
        'α' => 0x61, 'β' => 0x62, 'χ' => 0x63, 'δ' => 0x64, 'ε' => 0x65,
        'φ' => 0x66, 'γ' => 0x67, 'η' => 0x68, 'ι' => 0x69, 'ϕ' => 0x6A,
        'κ' => 0x6B, 'λ' => 0x6C, 'μ' => 0x6D, 'ν' => 0x6E, 'ο' => 0x6F,
        'π' => 0x70, 'θ' => 0x71, 'ρ' => 0x72, 'σ' => 0x73, 'τ' => 0x74,
        'υ' => 0x75, 'ω' => 0x77, 'ξ' => 0x78, 'ψ' => 0x79, 'ζ' => 0x7A,
        'Α' => 0x41, 'Β' => 0x42, 'Χ' => 0x43, 'Δ' => 0x44, 'Ε' => 0x45,
        'Φ' => 0x46, 'Γ' => 0x47, 'Η' => 0x48, 'Ι' => 0x49, 'Κ' => 0x4B,
        'Λ' => 0x4C, 'Μ' => 0x4D, 'Ν' => 0x4E, 'Ο' => 0x4F, 'Π' => 0x50,
        'Θ' => 0x51, 'Ρ' => 0x52, 'Σ' => 0x53, 'Τ' => 0x54, 'Υ' => 0x55,
        'Ω' => 0x57, 'Ξ' => 0x58, 'Ψ' => 0x59, 'Ζ' => 0x5A,
        _ => return None,
    })
}

/// Where one character goes.
fn slot(c: char) -> Slot {
    match c {
        // Printable ASCII is identical in WinAnsi.
        ' '..='~' => Slot::Win(c as u8),
        // Typographic punctuation WinAnsi has, at its own code points. Without
        // these "didn't" reads as "didn t".
        '\u{2018}' => Slot::Win(0x91),
        '\u{2019}' => Slot::Win(0x92),
        '\u{201C}' => Slot::Win(0x93),
        '\u{201D}' => Slot::Win(0x94),
        '\u{2013}' => Slot::Win(0x96),
        '\u{2014}' => Slot::Win(0x97),
        '\u{2022}' => Slot::Win(0x95),
        '\u{2026}' => Slot::Win(0x85),
        '\u{00A0}' | '\u{2007}' | '\u{2009}' | '\u{202F}' => Slot::Win(b' '),
        // Line and tab breaks are the layout's business, not the encoder's.
        // Counting them as characters that could not be drawn would report an
        // approximation on every line of every question.
        '\n' | '\r' => Slot::Skip,
        // A combining mark with nowhere to combine. Dropped rather than
        // substituted: `=` followed by U+0338 is `≠` in a font that can compose
        // it and a stray `?` in one that cannot.
        '\u{0300}'..='\u{036F}' | '\u{200B}'..='\u{200F}' | '\u{FEFF}' => Slot::Skip,
        // The Unicode minus, which is not the ASCII hyphen and turns up in
        // every set-difference in a discrete-maths paper.
        '\u{2212}' => Slot::Win(b'-'),
        '\u{2032}' => Slot::Win(b'\''),
        '\u{2033}' => Slot::Win(b'"'),
        '\u{00AD}' => Slot::Skip,
        // Latin-1 accented letters, which WinAnsi shares with Latin-1.
        '\u{00A1}'..='\u{00FF}' => Slot::Win(c as u8),
        _ => match symbol(c) {
            Some(b) => Slot::Sym(b),
            None => Slot::Fallback(match c {
                '□' | '☐' | '⬜' => "[ ]",
                '⌈' => "ceil(", '⌉' => ")",
                '∼' | '∽' => "~",
                '⌊' => "floor(", '⌋' => ")",
                '≺' => " < ", '≻' => " > ",
                '↦' => " -> ",
                '∖' => " \\ ",
                '⊤' => "T", '⊥' => "F",
                '⊢' => " |- ", '⊨' => " |= ",
                '∎' => "QED",
                '\t' => "    ",
                _ => "?",
            }),
        },
    }
}

/// Split a run into the byte strings each font can actually draw.
///
/// Returns the pieces and how many characters had to be approximated, so the
/// caller can report it rather than shipping a page with silent gaps in the
/// mathematics.
pub fn encode(text: &str, font: Font) -> (Vec<(Font, Vec<u8>)>, usize) {
    let mut out: Vec<(Font, Vec<u8>)> = Vec::new();
    let mut approximated = 0;
    for c in text.chars() {
        let (f, bytes): (Font, Vec<u8>) = match slot(c) {
            Slot::Win(b) => (font, vec![b]),
            Slot::Sym(b) => (Font::Symbol, vec![b]),
            Slot::Fallback(s) => {
                approximated += 1;
                (font, s.bytes().collect())
            }
            Slot::Skip => continue,
        };
        match out.last_mut() {
            Some((last, buf)) if *last == f => buf.extend_from_slice(&bytes),
            _ => out.push((f, bytes)),
        }
    }
    (out, approximated)
}

/// How many characters in this text cannot be drawn exactly.
pub fn approximations(text: &str) -> usize {
    encode(text, Font::Body).1
}

// ---------------------------------------------------------------- the writer

struct Writer {
    buf: Vec<u8>,
    offsets: Vec<usize>,
}

impl Writer {
    fn new() -> Self {
        // PDF 1.7. The binary comment line on the second row tells transfer
        // agents this is not a text file and stops them mangling line endings.
        let mut buf = Vec::new();
        buf.extend_from_slice(b"%PDF-1.7\n");
        buf.extend_from_slice(&[b'%', 0xE2, 0xE3, 0xCF, 0xD3, b'\n']);
        Writer { buf, offsets: Vec::new() }
    }

    fn reserve(&mut self) -> u32 {
        self.offsets.push(0);
        self.offsets.len() as u32
    }

    fn begin(&mut self, id: u32) {
        self.offsets[(id - 1) as usize] = self.buf.len();
        self.buf.extend_from_slice(format!("{} 0 obj\n", id).as_bytes());
    }

    fn end(&mut self) {
        self.buf.extend_from_slice(b"\nendobj\n");
    }

    fn put(&mut self, s: &str) {
        self.buf.extend_from_slice(s.as_bytes());
    }

    fn stream(&mut self, id: u32, dict_body: &str, data: &[u8]) {
        self.begin(id);
        self.put(&format!("<<{}/Length {}>>\nstream\n", dict_body, data.len()));
        self.buf.extend_from_slice(data);
        self.buf.extend_from_slice(b"\nendstream");
        self.end();
    }
}

/// Escape a byte string for a PDF literal.
fn escape(bytes: &[u8], out: &mut Vec<u8>) {
    for &b in bytes {
        match b {
            b'(' => out.extend_from_slice(b"\\("),
            b')' => out.extend_from_slice(b"\\)"),
            b'\\' => out.extend_from_slice(b"\\\\"),
            _ => out.push(b),
        }
    }
}

/// Build the finished document.
///
/// The trailer carries `/Size` and `/Root` and nothing else. There is
/// deliberately no `/Info` and no `/ID`: an `/ID` is derived from file content
/// and timestamps in most producers, and is a fingerprinting vector with no use
/// here.
pub fn build(pages: &[Page]) -> Vec<u8> {
    let mut w = Writer::new();

    let catalog_id = w.reserve();
    let pages_id = w.reserve();
    let fonts: Vec<(Font, u32)> =
        Font::all().into_iter().map(|f| (f, w.reserve())).collect();

    let mut page_ids = Vec::with_capacity(pages.len());
    for _ in pages {
        page_ids.push((w.reserve(), w.reserve())); // page, content
    }

    w.begin(catalog_id);
    w.put(&format!("<</Type/Catalog/Pages {} 0 R>>", pages_id));
    w.end();

    w.begin(pages_id);
    let kids: Vec<String> = page_ids.iter().map(|(p, _)| format!("{} 0 R", p)).collect();
    w.put(&format!("<</Type/Pages/Count {}/Kids[{}]>>", pages.len(), kids.join(" ")));
    w.end();

    for (font, id) in &fonts {
        w.begin(*id);
        // Base-14, so nothing is embedded. Symbol carries its own encoding and
        // must not be given WinAnsi, or every glyph comes out wrong.
        let encoding = if *font == Font::Symbol { "" } else { "/Encoding/WinAnsiEncoding" };
        w.put(&format!(
            "<</Type/Font/Subtype/Type1/BaseFont/{}{}>>",
            font.base_font(),
            encoding
        ));
        w.end();
    }

    let resources = format!(
        "/Font<<{}>>",
        fonts
            .iter()
            .map(|(f, id)| format!("/{} {} 0 R", f.tag(), id))
            .collect::<Vec<_>>()
            .join("")
    );

    for (page, &(pid, content_id)) in pages.iter().zip(page_ids.iter()) {
        // No /Annots, no /Rotate, no /Thumb, no /PieceInfo, no /Group.
        w.begin(pid);
        w.put(&format!(
            "<</Type/Page/Parent {} 0 R/MediaBox[0 0 {:.2} {:.2}]/Resources<<{}>>/Contents {} 0 R>>",
            pages_id, PAGE_W, PAGE_H, resources, content_id
        ));
        w.end();

        let mut c: Vec<u8> = Vec::new();
        for r in &page.rules {
            c.extend_from_slice(
                format!(
                    "{:.3} G\n0.6 w\n{:.2} {:.2} m\n{:.2} {:.2} l\nS\n",
                    r.grey, r.x0, r.y, r.x1, r.y
                )
                .as_bytes(),
            );
        }
        if !page.lines.is_empty() {
            c.extend_from_slice(b"BT\n");
            let mut grey = f32::NAN;
            for line in &page.lines {
                if line.grey != grey {
                    grey = line.grey;
                    c.extend_from_slice(format!("{:.3} g\n", grey).as_bytes());
                }
                c.extend_from_slice(
                    format!("1 0 0 1 {:.2} {:.2} Tm\n", line.x, line.y).as_bytes(),
                );
                for run in &line.runs {
                    for (font, bytes) in encode(&run.text, run.font).0 {
                        c.extend_from_slice(
                            format!("/{} {:.2} Tf\n(", font.tag(), line.size).as_bytes(),
                        );
                        escape(&bytes, &mut c);
                        c.extend_from_slice(b") Tj\n");
                    }
                }
            }
            c.extend_from_slice(b"ET\n");
        }
        w.stream(content_id, "", &c);
    }

    let xref_pos = w.buf.len();
    let count = w.offsets.len() + 1;
    w.put(&format!("xref\n0 {}\n", count));
    w.put("0000000000 65535 f \n");
    for i in 0..w.offsets.len() {
        w.put(&format!("{:010} 00000 n \n", w.offsets[i]));
    }
    w.put(&format!(
        "trailer\n<</Size {}/Root {} 0 R>>\nstartxref\n{}\n%%EOF\n",
        count, catalog_id, xref_pos
    ));

    w.buf
}
