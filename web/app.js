// Perturbation Workbench — browser front end.
//
// Division of labour: pdf.js reads, Rust decides, this file draws. Nothing
// here makes a protocol, grading, verification or manifest decision. Whether
// the stamp panel is live, what the banner says, which strategy to suggest,
// whether the rubric is still editable, whether an export is allowed — each of
// those is a field of the `View` that `crates/core/src/view.rs` computes and
// `cargo test` covers. This file renders that document and sends events back.
//
// The run itself lives in the wasm module, not here. Holding it in JavaScript
// would mean the one structure the whole tool's claim rests on spends its life
// as a mutable object in a language that cannot refuse an invalid change.

import { read } from './read.js';
import init, { Workbench } from './pkg/workbench_wasm.js';

const $ = (id) => document.getElementById(id);
const KEY = 'perturbation-workbench-v1';

const ui = {
  wb: null,
  view: null,
  q: 0,          // selected question
  v: 0,          // selected version
  draft: '',
  strategy: 'spatial',
  marks: {},     // chip id -> percent, for the attempt being graded
  note: '',
  response: '',
};

// ----------------------------------------------------------------- helpers

const esc = (s) =>
  String(s).replace(/[&<>"']/g, (c) =>
    ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));

/// `Timestamp` is UTC at seconds resolution and refuses anything else, so that
/// two stamps can be compared as strings. Trim the milliseconds toISOString
/// insists on adding.
const now = () => new Date().toISOString().replace(/\.\d+Z$/, 'Z');

async function sha256Text(text) {
  const bytes = new TextEncoder().encode(text);
  const out = await crypto.subtle.digest('SHA-256', bytes);
  return [...new Uint8Array(out)].map((b) => b.toString(16).padStart(2, '0')).join('');
}

/// The build stamped by `package-web.sh`. An unbuilt page has none, and says
/// so rather than inventing one that looks published.
function buildId() {
  const shown = $('build').textContent.trim();
  return /^[0-9a-f]{6,16}$/.test(shown) ? shown : '00000000';
}

/// Replace a panel's contents only when they would actually differ.
///
/// The page renders by assigning innerHTML, which replaces every node in the
/// panel. That is fine when something changed and actively harmful when
/// nothing did: a click is a mousedown and a mouseup on the *same* node, and
/// blurring a toolbar field fires `change` in between. Re-rendering there
/// swaps the button out from under the click, which then never fires at all —
/// the user's first click after typing a model name silently does nothing.
///
/// Comparing the markup first costs a string compare and removes the whole
/// class of problem. Handlers are only re-wired when the nodes they are wired
/// to are new.
function paint(el, html, wire) {
  if (el.dataset.html === html) return;
  el.dataset.html = html;
  el.innerHTML = html;
  if (wire) wire();
}

function busy(text) { $('busytext').textContent = text; $('busy').hidden = false; }
function idle() { $('busy').hidden = true; }

function sheet(title, html, download) {
  $('mtitle').textContent = title;
  $('mbody').innerHTML = html;
  const a = $('mdownload');
  if (download) {
    a.hidden = false;
    a.href = download.href;
    a.download = download.name;
    a.textContent = `Download ${download.name}`;
  } else {
    a.hidden = true;
  }
  $('modal').hidden = false;
}

function save(name, text, type = 'text/plain') {
  return { name, href: URL.createObjectURL(new Blob([text], { type })) };
}

/// Every call that can be refused comes through here, so a refusal reaches the
/// user as a message rather than as nothing happening.
function attempt(fn) {
  try {
    fn();
    persist();
    render();
    return true;
  } catch (e) {
    sheet('That is not allowed', `<p class="finding">${esc(String(e))}</p>`);
    return false;
  }
}

function persist() {
  try {
    localStorage.setItem(KEY, ui.wb.save());
  } catch (e) {
    // A full or disabled store loses the run on reload and nothing else.
    console.warn('session not saved:', e);
  }
}

// -------------------------------------------------------------------- boot

async function boot() {
  await init();
  ui.wb = new Workbench();
  try {
    const stored = localStorage.getItem(KEY);
    if (stored) ui.wb = Workbench.load(stored);
  } catch (e) {
    console.warn('stored run could not be read:', e);
  }

  wireStart();
  wireToolbar();
  wireKeys();
  $('mclose').onclick = () => ($('modal').hidden = true);
  $('addq').onclick = addQuestion;
  $('reset').onclick = reset;
  $('exportmanifest').onclick = exportManifest;
  $('exportqueries').onclick = exportQueries;
  $('exportsession').onclick = exportSession;
  render();
}

function wireStart() {
  $('wayblank').onsubmit = (e) => {
    e.preventDefault();
    const text = $('firstq').value;
    if (!text.trim()) return;
    attempt(() => ui.wb.addQuestion(1, titleFrom(text), text));
    $('firstq').value = '';
  };
  for (const id of ['file', 'file2']) {
    $(id).onchange = (e) => {
      const f = e.target.files[0];
      e.target.value = '';   // so re-picking the same file fires again
      if (f) openPdf(f);
    };
  }
  const drop = $('drop');
  for (const ev of ['dragenter', 'dragover']) {
    drop.addEventListener(ev, (e) => { e.preventDefault(); drop.classList.add('over'); });
  }
  for (const ev of ['dragleave', 'drop']) {
    drop.addEventListener(ev, (e) => { e.preventDefault(); drop.classList.remove('over'); });
  }
  drop.addEventListener('drop', (e) => {
    const f = [...e.dataTransfer.files].find((f) => f.type === 'application/pdf');
    if (f) openPdf(f);
  });
}

/// A title is how the instructor finds a question in the rail. It never
/// reaches the manifest, so guessing one from the first line is free.
function titleFrom(text) {
  const first = text.split('\n').find((l) => l.trim()) || 'Question';
  return first.trim().slice(0, 60);
}

async function openPdf(file) {
  busy('Reading the document…');
  try {
    const got = await read(new Uint8Array(await file.arrayBuffer()));
    const summary = JSON.parse(
      ui.wb.ingest(JSON.stringify(got.lines), got.sha256, got.pages));
    persist();
    render();
    const style = summary.style
      ? `numbered as <b>${esc(summary.style)}</b>`
      : 'no numbering convention recognised, so it came in as one question you can split';
    sheet('Opened', `
      <p>${summary.drafts.length} question(s) from ${got.pages} page(s), ${style}.</p>
      <p class="sidenote">
        ${summary.furnitureDropped} repeated header/footer line(s) dropped,
        ${summary.checkboxesStripped} checkbox glyph(s) stripped,
        ${summary.codeLines} monospaced line(s) kept as code.
      </p>
      <p class="sidenote">Everything here is a draft — text extraction is imperfect and the
      questions are yours to edit before you test them.</p>`);
  } catch (e) {
    sheet('Could not open that PDF', `<p class="finding">${esc(String(e))}</p>`);
  } finally {
    idle();
  }
}

function wireToolbar() {
  // Committing an unchanged value would re-render for nothing, and a render
  // for nothing is the one that lands mid-click. `change` fires on blur
  // whenever the value differs from what it was at focus, so this fires far
  // more often than the value actually moves.
  const commitTarget = () => {
    const model = $('model').value.trim();
    const access = $('access').value;
    const fresh = $('fresh').checked;
    $('model').classList.toggle('bad', model !== '' && !Workbench.isModelId(model));
    if (!model || !Workbench.isModelId(model)) return;
    const t = ui.view?.target;
    if (t && t.model === model && t.access === access && t.freshInstancePerAttempt === fresh) return;
    attempt(() => ui.wb.setTarget(model, access, fresh, now()));
  };
  $('model').onchange = commitTarget;
  $('model').oninput = () =>
    $('model').classList.toggle('bad',
      $('model').value.trim() !== '' && !Workbench.isModelId($('model').value.trim()));
  $('access').onchange = commitTarget;
  $('fresh').onchange = commitTarget;

  for (const id of ['threshold', 'fkdrift', 'fkcap', 'growthcap']) {
    $(id).onchange = () => {
      const next = {
        threshold: +$('threshold').value,
        fkDrift: +$('fkdrift').value,
        fkCap: +$('fkcap').value,
        growthCap: +$('growthcap').value,
      };
      const s = ui.view?.settings;
      if (s && Object.keys(next).every((k) => s[k] === next[k])) return;
      attempt(() => ui.wb.setSettings(JSON.stringify(next)));
    };
  }
}

/// Arrow keys move through the queue; C copies the exact query. Both are off
/// while a field has focus, or typing a question would page away from it.
function wireKeys() {
  document.addEventListener('keydown', (e) => {
    if (!$('modal').hidden) {
      if (e.key === 'Escape') $('modal').hidden = true;
      return;
    }
    if (/^(INPUT|SELECT|TEXTAREA)$/.test(document.activeElement?.tagName)) return;
    const n = ui.view?.questions.length || 0;
    if (!n) return;
    if (e.key === 'ArrowDown') { e.preventDefault(); select(Math.min(ui.q + 1, n - 1)); }
    if (e.key === 'ArrowUp') { e.preventDefault(); select(Math.max(ui.q - 1, 0)); }
    if (e.key === 'c' || e.key === 'C') { e.preventDefault(); copyQuery(); }
  });
}

function select(q) {
  ui.q = q;
  ui.v = ui.view.questions[q].versions.length - 1;
  resetPanels();
  render();
}

function resetPanels() {
  ui.marks = {};
  ui.note = '';
  ui.response = '';
  ui.draft = '';
}

// ------------------------------------------------------------------ render

function render() {
  ui.view = JSON.parse(ui.wb.view());
  const v = ui.view;
  const has = v.questions.length > 0;

  $('start').hidden = has;
  $('app').hidden = !has;
  $('runbar').hidden = !has;
  if (!has) return;

  ui.q = Math.min(ui.q, v.questions.length - 1);
  ui.v = Math.min(ui.v, v.questions[ui.q].versions.length - 1);

  $('runcount').textContent =
    `${v.resistant}/${v.questions.length} resistant` +
    (v.advisories ? ` · ${v.advisories} reported` : '') +
    (v.blocking ? ` · ${v.blocking} blocking` : '');
  $('exportmanifest').disabled = !v.canExport;
  $('exportqueries').disabled = !v.canExport;

  // Settings and target are owned by the run, so the fields follow it rather
  // than the other way round.
  if (document.activeElement !== $('model')) $('model').value = v.target?.model ?? '';
  if (document.activeElement !== $('access')) $('access').value = v.target?.access ?? 'institutional';
  $('fresh').checked = v.target?.freshInstancePerAttempt ?? true;
  for (const [id, val] of [['threshold', v.settings.threshold], ['fkdrift', v.settings.fkDrift],
                           ['fkcap', v.settings.fkCap], ['growthcap', v.settings.growthCap]]) {
    if (document.activeElement !== $(id)) $(id).value = val;
  }

  renderQueue();
  renderStage();
  renderSide();
}

function renderQueue() {
  const html = ui.view.questions.map((q, i) => `
    <button class="qcard bar-${q.tone} ${i === ui.q ? 'sel' : ''}" data-q="${i}">
      <span class="qtop">
        <span class="qnum">Q${q.ordinal}</span>
        <span class="tag tone-${q.tone}">${esc(q.label)}</span>
      </span>
      <span class="qtitle">${esc(q.title)}</span>
      <span class="qmeta">
        <span>${q.versions.length} version${q.versions.length === 1 ? '' : 's'}</span>
        ${q.suggested ? `<span>try ${esc(q.suggested)}</span>` : ''}
      </span>
    </button>`).join('');
  paint($('queue'), html, () => {
    for (const el of $('queue').querySelectorAll('.qcard')) {
      el.onclick = () => select(+el.dataset.q);
    }
  });
}

function renderStage() {
  const q = ui.view.questions[ui.q];
  const ver = q.versions[ui.v];
  const g = ver.guard;
  const base = q.versions[0];

  const warn = (on, text) => `<span class="${on ? 'warn' : ''}">${text}</span>`;

  const html = `
    <div class="banner bar-${q.tone}">
      <span class="tag tone-${q.tone}">${esc(q.label)}</span>
      ${esc(q.banner)}
    </div>

    <div class="card">
      <div class="cardhead">
        <div class="tabs">
          ${q.versions.map((v, i) => `
            <button class="tab ${i === ui.v ? 'sel' : ''}" data-v="${i}">
              ${esc(v.label)}${v.locked ? ' <span class="lock">🔒</span>' : ''}
            </button>`).join('')}
        </div>
        <span class="digest">sha256 <b>${esc(ver.short)}</b>…</span>
      </div>

      <textarea class="query" id="query" spellcheck="false"
        ${ver.editable ? '' : 'readonly'}>${esc(ver.text)}</textarea>

      <div class="cardhead">
        <div class="tabs">
          <button class="btn tiny" id="copy" title="Copy the exact query (C)">Copy exact query</button>
          ${ver.editable ? '<button class="btn tiny" id="savetext">Save edit</button>' : ''}
          ${ver.editable && ui.v > 0 ? '<button class="btn tiny danger" id="discard">Discard this version</button>' : ''}
        </div>
        <div class="meters">
          <span>FK grade <b>${ver.metrics.grade}</b></span>
          ${warn(g.drifted || g.overCap, `vs base <b>${base.metrics.grade}</b>`)}
          <span>words <b>${ver.metrics.words}</b></span>
          ${warn(g.overgrown, `growth <b>${g.growth}%</b>`)}
          ${ver.codeBlocks ? `<span>code blocks <b>${ver.codeBlocks}</b></span>` : ''}
        </div>
      </div>
      ${g.tripped ? `<p class="sidenote warn">Complexity guard tripped — allowed, and recorded
        in the manifest as an advisory. A question that got harder to read may still be the
        right question; that is your call, not the tool's.</p>` : ''}
    </div>

    <div class="card">
      <div class="cardhead">
        <h2>Attempts</h2>
        <span class="total">${ver.attempts.length}/3 · ${ver.attemptsLeft} left</span>
      </div>
      <div class="stamps">
        ${[0, 1, 2].map((i) => {
          const a = ver.attempts[i];
          if (!a) return `<div class="stamp blank"><span class="n">attempt ${i + 1}</span>
            <span class="pct">—</span><span class="when">not run</span></div>`;
          return `<div class="stamp bar-${a.metThreshold ? 'bad' : 'good'}">
            <span class="n">attempt ${a.ordinal} · rubric r${a.rubricRevision}</span>
            <span class="pct tone-${a.metThreshold ? 'bad' : 'good'}">${a.pct}%</span>
            <span class="when">${esc(a.at.slice(11, 16))} · resp ${esc(a.responseSha256.slice(0, 8))}</span>
          </div>`;
        }).join('')}
      </div>
    </div>

    ${ver.canStamp ? gradingPanel(q, ver) : ''}
    ${benchPanel(q)}`;

  paint($('stage'), html, () => {
    for (const el of $('stage').querySelectorAll('.tab[data-v]')) {
      el.onclick = () => { ui.v = +el.dataset.v; ui.marks = {}; render(); };
    }
    $('copy').onclick = copyQuery;
    if ($('savetext')) {
      $('savetext').onclick = () =>
        attempt(() => ui.wb.editVersion(ui.q, ui.v, $('query').value));
    }
    if ($('discard')) {
      $('discard').onclick = () =>
        attempt(() => { ui.wb.discardLatest(ui.q); ui.v = Math.max(0, ui.v - 1); });
    }
    wireGrading();
    wireBench();
  });
  drawDraftMeters();
  for (const t of $('stage').querySelectorAll('textarea')) grow(t);
}

/// Size a textarea to its contents.
///
/// The exact query is the thing the instructor is meant to read carefully
/// before pasting it into a chatbot, and a box that clips it at eight lines
/// invites not reading it. Re-applied on input so a draft grows as it is
/// written.
function grow(el) {
  const fit = () => {
    el.style.height = 'auto';
    el.style.height = `${Math.min(el.scrollHeight + 2, 760)}px`;
  };
  el.addEventListener('input', fit);
  fit();
}

function gradingPanel(q, ver) {
  const needTarget = !ui.view.target;
  return `
  <div class="card">
    <div class="cardhead">
      <h2>Grade attempt ${ver.attempts.length + 1}</h2>
      <span class="total">rubric revision <b>${q.rubric.revisions}</b> ·
        <b>${q.rubric.totalPoints}</b> points · threshold <b>${ui.view.settings.threshold}%</b></span>
    </div>
    ${needTarget ? `<p class="finding">Name the target model above first. "Resistant" is a claim
      about a specific model on a specific day, and a run that never wrote it down cannot
      support one.</p>` : ''}
    <div class="chips">
      ${q.rubric.chips.map((c) => `
        <div class="chiprow">
          <span class="chiplabel ${c.penalty ? 'penalty' : ''}">
            ${esc(c.label)} <small>${c.points > 0 ? '+' : ''}${c.points}</small>
          </span>
          <span class="levels">
            ${q.rubric.scale.levels.map((l) => `
              <button class="level ${ui.marks[c.id] === l.credit ? 'sel' : ''}"
                data-chip="${esc(c.id)}" data-credit="${l.credit}"
                title="${l.credit}% of ${c.points}">${esc(l.label)}</button>`).join('')}
          </span>
        </div>`).join('')}
    </div>
    <div class="grid2">
      <label class="stacked">Paste the model's response
        <textarea id="response" rows="5" spellcheck="false"
          placeholder="Pasted here only to be hashed. The text is discarded; only the digest is kept."></textarea>
      </label>
      <label class="stacked">Hallucination ledger (optional)
        <textarea id="notetext" rows="5"
          placeholder="What did it get wrong, specifically? This is what Step 6 turns into a penalty chip.">${esc(ui.note)}</textarea>
      </label>
    </div>
    <div class="cardhead">
      <span class="sidenote">Every chip must be marked. The response digest is what proves an
        attempt happened.</span>
      <button class="btn primary" id="stamp" ${needTarget ? 'disabled' : ''}>Stamp attempt ${ver.attempts.length + 1}</button>
    </div>
  </div>`;
}

function benchPanel(q) {
  return `
  <div class="card">
    <div class="cardhead">
      <h2>Perturbation bench</h2>
      ${q.suggested ? `<span class="sidenote">Not yet tried: <b>${esc(q.suggested)}</b></span>`
                    : '<span class="sidenote">All three strategies have been tried on this question.</span>'}
    </div>
    <div class="tabs">
      ${ui.view.strategies.map((s) => `
        <button class="tab ${ui.strategy === s.id ? 'sel' : ''}" data-s="${esc(s.id)}"
          title="${esc(s.description)}">${esc(s.name)}</button>`).join('')}
    </div>
    <textarea class="draft" id="draft" spellcheck="false"
      placeholder="Write the perturbed question here. Start from the exact query above and change
as little as possible — the point is to break one thing, not to rewrite.">${esc(ui.draft)}</textarea>
    <div class="cardhead">
      <span class="meters" id="draftmeters"></span>
      <button class="btn primary" id="saveversion">Save as v${q.versions.length}</button>
    </div>
  </div>`;
}

function renderSide() {
  const q = ui.view.questions[ui.q];
  const r = q.rubric;

  const html = `
    <div class="card">
      <div class="cardhead">
        <h2>Rubric</h2>
        <span class="total"><b>${r.totalPoints}</b> pts · r${r.revisions}</span>
      </div>
      ${r.frozen
        ? `<p class="sidenote">Frozen — the first attempt was graded against it. Penalty chips
           can still be added from the ledger below; nothing else can change, or the attempts
           already recorded would be graded by a rubric that did not exist when they ran.</p>`
        : `<p class="sidenote">Editable until the first attempt is stamped. One chip is one
           atomistic thing the answer either shows or does not.</p>`}

      <label class="stacked">Mastery scale
        <select id="scale" ${r.frozen ? 'disabled' : ''}>
          <option value="">${esc(r.scale.name)} (${r.scale.levels.length} levels)</option>
        </select>
      </label>

      <div class="chips">
        ${r.chips.map((c) => `
          <div class="editrow">
            <input value="${esc(c.label)}" data-chip="${esc(c.id)}" data-f="label"
              ${r.frozen ? 'readonly' : ''} class="${c.penalty ? 'penalty' : ''}">
            <input type="number" value="${c.points}" data-chip="${esc(c.id)}" data-f="points"
              ${r.frozen ? 'readonly' : ''}>
            ${r.frozen ? `<span class="digest" title="${c.from ? `from v${c.from.version} attempt ${c.from.attempt}` : ''}">${c.penalty ? '⊖' : ''}</span>`
                       : `<button class="btn tiny danger" data-drop="${esc(c.id)}">×</button>`}
          </div>`).join('') || '<p class="sidenote">No chips yet. A question with no rubric cannot be graded.</p>'}
      </div>

      ${r.frozen ? '' : `
      <form class="editrow" id="addchip">
        <input id="chiplabel" placeholder="What the answer must show" required>
        <input id="chippoints" type="number" value="2" min="1" required>
        <button class="btn tiny primary" type="submit">+</button>
      </form>`}

      <div class="cardhead">
        <button class="btn tiny" id="rubricout">Export rubric</button>
        <label class="btn tiny ${r.frozen ? '' : ''}">Import rubric
          <input type="file" id="rubricin" accept="application/json" hidden ${r.frozen ? 'disabled' : ''}>
        </label>
      </div>
    </div>

    <div class="card">
      <div class="cardhead">
        <h2>Ledger</h2>
        ${q.unpromotedNotes ? `<span class="tag tone-working">${q.unpromotedNotes} unpromoted</span>` : ''}
      </div>
      <p class="sidenote">Step 6. An observation becomes a penalty chip, or it is just a note.
        Notes stay in the session and the assignment appendix; they never reach the manifest.</p>
      <div class="ledger">
        ${ledgerRows(q)}
      </div>
    </div>`;

  paint($('side'), html, () => wireSide(q));
}

function ledgerRows(q) {
  const rows = [];
  q.versions.forEach((v) => v.attempts.forEach((a) => {
    a.notes.forEach((n, i) => {
      const un = a.penaltiesDerived === 0;
      rows.push(`<div class="note ${un ? 'unpromoted' : ''}">
        <span class="src">v${v.ordinal} · attempt ${a.ordinal} · ${a.pct}%${un ? '' : ' · penalised'}</span>
        ${esc(n)}
        ${un ? `<button class="btn tiny" data-promote="${v.ordinal}:${a.ordinal}:${i}">Make a penalty chip</button>` : ''}
      </div>`);
    });
  }));
  return rows.join('') || '<p class="sidenote">Nothing logged yet.</p>';
}

// ----------------------------------------------------------------- actions

async function copyQuery() {
  const text = ui.view.questions[ui.q].versions[ui.v].text;
  try {
    await navigator.clipboard.writeText(text);
    const b = $('copy');
    if (b) { b.textContent = 'Copied — paste into a fresh instance'; setTimeout(render, 1400); }
  } catch (e) {
    sheet('Clipboard blocked', '<p>Select the query text and copy it by hand.</p>');
  }
}

function wireGrading() {
  for (const el of $('stage').querySelectorAll('.level')) {
    el.onclick = () => {
      ui.marks[el.dataset.chip] = +el.dataset.credit;
      render();
    };
  }
  if ($('notetext')) $('notetext').oninput = (e) => (ui.note = e.target.value);
  if ($('stamp')) {
    $('stamp').onclick = async () => {
      const q = ui.view.questions[ui.q];
      const missing = q.rubric.chips.filter((c) => !(c.id in ui.marks));
      if (missing.length) {
        sheet('Every chip must be marked', `
          <p>A forgotten chip and a chip marked absent are different claims, and only one of
          them is evidence.</p>
          <p class="finding">Unmarked: ${missing.map((c) => esc(c.label)).join(', ')}</p>`);
        return;
      }
      const response = $('response').value;
      if (!response.trim()) {
        sheet('Paste the response first', `
          <p>The response digest is what proves an attempt happened. The text is hashed here and
          discarded — it never enters the run.</p>`);
        return;
      }
      const digest = await sha256Text(response);
      const note = $('notetext').value.trim();
      const v = ui.v;
      const ok = attempt(() => {
        const ordinal = ui.wb.stamp(ui.q, v, now(), digest, JSON.stringify(ui.marks));
        if (note) ui.wb.note(ui.q, v, ordinal, note);
      });
      if (ok) { resetPanels(); render(); }
    };
  }
}

function wireBench() {
  for (const el of $('stage').querySelectorAll('.tab[data-s]')) {
    el.onclick = () => { ui.strategy = el.dataset.s; render(); };
  }
  const draft = $('draft');
  draft.oninput = () => { ui.draft = draft.value; drawDraftMeters(); };
  drawDraftMeters();
  $('saveversion').onclick = () => {
    if (!ui.draft.trim()) return;
    const ok = attempt(() => ui.wb.addVersion(ui.q, ui.strategy, ui.draft));
    if (ok) {
      ui.draft = '';
      ui.v = ui.view.questions[ui.q].versions.length - 1;
      render();
    }
  };
}

/// The draft's readings come from the core, live, so the number shown while
/// typing is the number the manifest will record.
function drawDraftMeters() {
  const el = $('draftmeters');
  if (!el) return;
  if (!ui.draft.trim()) { el.innerHTML = '<span>Nothing drafted yet.</span>'; return; }
  const g = JSON.parse(ui.wb.guardDraft(ui.q, ui.draft));
  const digest = Workbench.digestOf(ui.draft);
  el.innerHTML = `
    <span class="${g.drifted || g.overCap ? 'warn' : ''}">FK <b>${g.grade}</b> vs <b>${g.baseGrade}</b></span>
    <span class="${g.overgrown ? 'warn' : ''}">growth <b>${g.growth}%</b></span>
    <span class="digest">sha256 <b>${esc(digest.slice(0, 8))}</b>…</span>`;
}

function wireSide(q) {
  for (const el of $('side').querySelectorAll('input[data-chip]')) {
    el.onchange = () => {
      const id = el.dataset.chip;
      const row = $('side').querySelector(`input[data-chip="${CSS.escape(id)}"][data-f="label"]`);
      const pts = $('side').querySelector(`input[data-chip="${CSS.escape(id)}"][data-f="points"]`);
      attempt(() => ui.wb.editChip(ui.q, id, row.value, +pts.value));
    };
  }
  for (const el of $('side').querySelectorAll('button[data-drop]')) {
    el.onclick = () => attempt(() => ui.wb.removeChip(ui.q, el.dataset.drop));
  }
  if ($('addchip')) {
    $('addchip').onsubmit = (e) => {
      e.preventDefault();
      const ok = attempt(() => ui.wb.addChip(ui.q, $('chiplabel').value, +$('chippoints').value));
      if (ok) $('chiplabel').focus();
    };
  }
  fillScales(q);
  $('rubricout').onclick = () => {
    const doc = ui.wb.exportRubric(ui.q);
    sheet('Rubric', `<pre>${esc(doc)}</pre>`, save(`rubric-q${q.ordinal}.json`, doc, 'application/json'));
  };
  $('rubricin').onchange = async (e) => {
    const f = e.target.files[0];
    e.target.value = '';
    if (!f) return;
    const text = await f.text();
    attempt(() => ui.wb.importRubric(ui.q, text));
  };
  for (const el of $('side').querySelectorAll('button[data-promote]')) {
    el.onclick = () => {
      const [v, a] = el.dataset.promote.split(':').map(Number);
      const label = prompt('What does this penalise?');
      if (!label) return;
      const points = prompt('How many points does it cost? (a positive number)', '4');
      if (!points) return;
      attempt(() => ui.wb.addPenalty(ui.q, label, -Math.abs(+points), v, a, now()));
    };
  }
}

/// The scale picker offers what `core` ships and what the run already uses; it
/// does not invent a scale of its own.
function fillScales(q) {
  const sel = $('scale');
  const built = JSON.parse(Workbench.builtInScales());
  const all = [q.rubric.scale, ...built.filter((s) => s.name !== q.rubric.scale.name)];
  sel.innerHTML = all.map((s, i) =>
    `<option value="${i}">${esc(s.name)} (${s.levels.map((l) => l.credit + '%').join(' / ')})</option>`).join('');
  sel.onchange = () => attempt(() => ui.wb.setScale(ui.q, JSON.stringify(all[+sel.value])));
}

function addQuestion() {
  const text = prompt('Paste or type the question. Code goes in a ``` fenced block.');
  if (!text || !text.trim()) return;
  const next = Math.max(0, ...ui.view.questions.map((q) => q.ordinal)) + 1;
  attempt(() => { ui.wb.addQuestion(next, titleFrom(text), text); ui.q = ui.view.questions.length; });
}

// ----------------------------------------------------------------- exports

function exportManifest() {
  try {
    const json = ui.wb.manifest(buildId(), now(), '');
    sheet('Run manifest', `
      <p>Hashes, counts and settings. No question text, no responses, no ledger notes —
      that is a property of the schema, not a filter.</p>
      <pre>${esc(json.slice(0, 1400))}${json.length > 1400 ? '\n…' : ''}</pre>`,
      save('run-manifest.json', json, 'application/json'));
  } catch (e) {
    showBlocked(String(e));
  }
}

function exportQueries() {
  const files = JSON.parse(ui.wb.queryFiles(true));
  if (!files.length) {
    sheet('Nothing resistant yet', '<p>No question has reached Step 8b.</p>');
    return;
  }
  // One file per question rather than an archive: a zip would need a library,
  // and the point of these files is that a collaborator can `shasum` them.
  const body = files.map((f) => `
    <p class="finding advisory">${esc(f.name)} — question ${f.question}, v${f.version}
      <a class="btn tiny" href="${save(f.name, f.text).href}" download="${esc(f.name)}">Download</a></p>`).join('');
  sheet('Exact query files', `
    <p>One file per resistant question, named by the digest in the manifest. Check them with:</p>
    <pre>shasum -a 256 q*.txt</pre>${body}`);
}

function exportSession() {
  const json = ui.wb.save();
  sheet('Session', `
    <p>The whole run, including question text and ledger notes, so it can be paused and moved
    between machines.</p>
    <p class="finding">This is <b>not</b> the manifest. It contains your assignment. The manifest
    is the file meant for sharing.</p>`,
    save('perturbation-session.json', json, 'application/json'));
}

function showBlocked(message) {
  const findings = ui.view.blockingFindings || [];
  sheet('Export refused', `
    <p>Verification runs before any export, and something in this run contradicts itself.
    A manifest whose own audit says the evidence is broken is not evidence.</p>
    ${findings.map((f) => `<p class="finding">${esc(JSON.stringify(f))}</p>`).join('')}
    ${findings.length ? '' : `<p class="finding">${esc(message)}</p>`}`);
}

function reset() {
  sheet('Discard this run?', `
    <p>Everything goes: questions, versions, attempts, rubrics and ledger. Export the session
    first if you might want it back.</p>
    <p><button class="btn danger" id="reallyreset">Yes, discard it</button></p>`);
  $('reallyreset').onclick = () => {
    localStorage.removeItem(KEY);
    ui.wb = new Workbench();
    ui.q = 0; ui.v = 0;
    resetPanels();
    $('modal').hidden = true;
    render();
  };
}

boot();
