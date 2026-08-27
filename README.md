# Perturbation Workbench

Make an assignment question resistant to a chatbot, and produce a record that
proves you did.

### ▶ [Open the tool](https://sgilson7.github.io/perturbation-workbench/)

Nothing is uploaded. There is no server, no account, and no network request
after the page loads — the assignment is opened, perturbed, graded and exported
entirely inside your browser tab. **No model is called either**: the tool has
no API key and no code that could use one. That is not a policy promise; it is a
property of how the tool is built, and you can confirm it by watching the
Network tab while you use it.

---

## What it is

A companion to *Adversarial Assignment Perturbation: Effects on Help-seeking
Behaviors in Student Generative AI Chatbot Use* (Gilson, Tabarsi & Barnes,
AIED 2026). It runs the iterative perturbation process of the paper's Table 1
and produces two artefacts a collaborator at another institution can check:
the **assignment**, and a **run manifest** that is a proof of execution.

The paper's process is instructor-led, and so is this. You are the transport
between the workbench and the chatbot: the tool gives you the exact query, you
paste it into a fresh instance, you paste the response back, you grade it. What
the tool guarantees is that the protocol was followed — the same bytes were
prompted three times, the rubric was fixed before grading, and nothing gets
stamped "resistant" that did not earn it.

## How it works

Start by typing a question, or drop an assignment PDF and have its questions
split out. Either way you get a **protocol sheet** per question:

1. **Baseline.** Copy the exact query. Paste it into a fresh instance of the
   model you are testing. Paste the response back — it is hashed and discarded —
   and grade it against your rubric.
2. **Evaluate.** At or above the threshold (60% by default, the paper's failing
   line), the question is not resistant and wants a perturbation.
3. **Perturb.** Apply one of the paper's three strategies — Spatial Injection,
   Axiomatic Replacement, Contextual Embedding — and save it as a new version.
   The old version keeps its attempts; they are evidence about *those* bytes.
4. **Repeat, three times.** Below the threshold on three separate attempts with
   the same query, the version is classified One-Shot GenAI Resistant. A later
   attempt that passes is a false negative caught, and says so.
5. **Update the rubric.** What the model invented goes in the ledger, and the
   ledger becomes **penalty chips** that carry a pointer back to the attempt
   that motivated them.

Rubrics are **chips**: one chip is one atomistic thing the answer either shows
or does not. Chips carry point weights, and what fractions of those weights are
attainable is fixed by a **mastery scale** you define. Rubrics import and
export as their own file, so one can be reused across questions or runs.

Everything that has to be correct is decided in `crates/core` and runs under
`cargo test` without a browser. Nothing in `web/app.js` makes a protocol,
grading, verification or manifest decision; it renders a view the core computed
and does no arithmetic of its own.

## The run manifest

Doing the protocol is half the obligation; being able to show a collaborator —
months later, with no access to the run — that you followed it is the other
half. The manifest is that evidence, and it is **deliberately not the
assignment**:

```json
{
  "schema": "perturbation-workbench-manifest/1",
  "containsQuestionText": false,
  "input": { "sha256": "969548…", "pages": 22, "questionsIngested": 9 },
  "settings": { "threshold": 60.0, "fkDrift": 1.5, "fkCap": 14.0, "growthCap": 35.0 },
  "target": { "model": "gemini-2.5-flash", "access": "institutional",
              "freshInstancePerAttempt": true },
  "questions": [{
    "ordinal": 5,
    "rubric": { "revisions": 2, "chips": 8, "penaltyChips": 1, "totalPoints": 16 },
    "versions": [{
      "ordinal": 1, "strategy": "spatial", "textSha256": "55709665…",
      "fkGrade": 4.5, "words": 216, "guardTripped": false, "codeBlocks": 0,
      "attempts": [{ "ordinal": 1, "at": "2026-08-27T09:10:00Z",
                     "querySha256": "55709665…", "responseSha256": "58aac4e9…",
                     "rubricRevision": 1, "pct": 0.0, "ledgerEntries": 1 }]
    }],
    "status": { "status": "resistant" }
  }],
  "verification": { "blocking": [], "advisories": [] },
  "outputs": { "assignmentSha256": "0dda657c…", "includesHistory": true }
}
```

The obvious log is worse than useless: "Question 5 failed three times" plus the
text of question 5 is a file that gets emailed around and read by the students
sitting the exam. A record that leaks the questions it certifies destroys the
thing it was made to protect.

So the safety property is structural rather than a filter: **there is no
parameter through which question text could arrive.** Every field is a count,
an ordinal, a setting, a hash, an instant, or the name of an enum variant. The
four exceptions are validated to be what they claim — a SHA-256 is sixty-four
hex digits, a timestamp is a UTC instant, a build id is a short hex hash, and a
model identifier is bounded to sixty-four characters of the alphabet model
names are made of. A question is an **ordinal, never a title**: "Probability at
the Salad Bar" is question text by any honest reading. Ledger notes are
**counted, not carried**: a description of what the model got wrong is the
answer turned inside out.

Everything is identified by SHA-256 of its bytes, and the file carries its own
recipe:

```sh
shasum -a 256 q5-55709665.txt     # compare against questions[].versions[].textSha256
shasum -a 256 assignment.pdf      # compare against outputs.assignmentSha256
```

Those numbers are checkable because the tool exports the **exact query files**
alongside the assignment — one `.txt` per resistant question, named by its own
digest prefix. That is the file whose bytes were prompted.

The **session** file is a different thing entirely and the tool says so on the
way out: it holds the question text, the ledger and every score, because it is
the run, paused. It is for moving a run between machines, not for sharing.

## Verification

Every export re-derives the whole run from its attempts first, and deliberately
does not ask the protocol module for the answer it is checking. The report
distinguishes two things that are easy to conflate:

**Blocking — the run contradicts itself. The download is refused.**

- the text on file is not the text an attempt recorded prompting;
- two attempts on one version prompted different bytes;
- a version claimed resistant with fewer than three attempts, or with one that
  met the threshold;
- an attempt graded against a rubric revision that did not exist at its
  timestamp, or that is not in the file at all;
- a penalty chip pointing at an attempt that does not exist;
- attempts stamped without naming the model they were tested against.

**Advisory — your call. Reported into the manifest, not blocked.**

- a version saved with the complexity guard tripped;
- a question still in progress at export;
- the run changed target models partway through;
- ledger notes that never became penalty chips.

Most of the blocking cases cannot be reached through the tool at all — a
prompted version is locked, a fourth attempt is refused. They are checked
anyway, because a session is a JSON file on somebody's disk and the tool is not
the only way to write one.

You can run the same audit without a browser:

```sh
make verify RUN=run-manifest.json    # or a session.json
```

A manifest is audited against itself: do the recorded attempts actually produce
the recorded status, and are the query digests on a resistant version all the
same digest as its text. That is narrower than the full audit on purpose — it
is what a recipient who has only the manifest can do, so it has to be enough.

## The complexity guard

A perturbation that defeats the model by being *harder to read* has not made
the question resistant; it has made it a worse question, and the students it
costs most are the ones the paper is about. Every draft is measured against its
own base text — Flesch-Kincaid drift, an absolute grade cap, word growth — and
drifting is called out.

The guard **reports and does not refuse**. The instructor is the one who knows
whether a longer question is a worse question, and a tool that blocked there
would be overruling the person with the expertise. What it must not do is let
the drift pass unrecorded, so it travels into the manifest as an advisory.

Code is excluded before anything is counted. Flesch-Kincaid on a forty-line
Java method returns a grade level in the twenties, and a guard that fires on
every code-bearing question is a guard people learn to ignore.

## What it does not do

Being clear about this matters more than the feature list.

- **It does not test the model.** It never calls one and has no code that
  could. Every number in a manifest is a grade *you* assigned to a response
  *you* pasted in. The tool proves the protocol was followed; it cannot prove
  the grading was fair, and it does not try to.
- **"Resistant" is relative to a named model on a date.** It means: this model,
  reached this way, failed these bytes three times, on that day. A model
  updated next month may pass the same question. This is why the manifest
  refuses to be built until the run names its target, and why it records the
  access route — an institutional licence and a consumer free tier are not the
  same model in practice.
- **Three attempts lower the false-negative rate; they do not bound it.** A
  question that fails three times may still be answered on the fourth. The
  protocol is a sampling procedure with n = 3, and the paper says so.
- **The reading level is approximate on mathematics and code**, and the
  formula is a heuristic even on prose. It is ported line-for-line from the
  study's own implementation so a figure recorded then and one shown here mean
  the same thing — not because that implementation is authoritative. Use it to
  see drift, not as a number about a question.
- **PDF extraction produces drafts, not questions.** Layout is lost on some
  producers, superscripts fuse into words on others, and a monospaced heading
  is indistinguishable from a line of code. Everything that comes out of an
  ingest is yours to read and fix before you test it.
- **The splitter works out one numbering convention and applies it.** Pointed
  at something that is not an assignment — a paper, say — it will find the
  reference list and tell you it numbered the document that way. It says which
  convention it used and how many lines it dropped, so a wrong answer is
  visible rather than silent.
- **It is not tamper-proof against its own author.** Verification catches a run
  that contradicts itself, which is what an honest mistake and a careless edit
  both look like. Someone determined to forge a session consistently can. The
  guarantee is that you cannot do it by accident, and that a reader can check.
- **Mathematics in the PDF is set in Symbol**, which covers what a discrete
  maths or algorithms question actually uses. Anything outside it is
  transliterated to readable ASCII and *counted*, and the export tells you how
  many. The exact query `.txt` files carry the original UTF-8; the PDF is the
  readable rendering.
- **The run lives in `localStorage`.** Clearing site data loses it. Export the
  session if it matters.

## Building it

Requires Rust and the wasm target. No node, npm, or bundler.

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version "$(awk '/^name = "wasm-bindgen"$/{f=1} f&&/^version = /{gsub(/"/,"");print $3;exit}' Cargo.lock)"

make test           # the protocol suite (153 tests, native, no browser)
make test-ui-setup  # one-time: headless Chromium for browser tests
make test-ui        # drive the real UI through a whole run and check its exports
make serve          # build and open at localhost:8080
make deploy         # push; Actions tests, builds, and publishes to Pages
```

| command | what it does |
|---|---|
| `make test` | the protocol suite, no browser needed |
| `make test-ui` | a whole run driven in headless Chromium, ending in an exported assignment |
| `make verify RUN=…` | audit a session or a manifest from the command line |
| `make web` | build the browser app into `dist/web/` |
| `make serve` | build it and open it locally |
| `make fixtures` | extract a local assignment PDF into an ingest fixture |
| `make deploy` | push; Actions publishes to Pages |

## Layout

```
crates/core/    everything that has to be correct — no browser dependencies, so
                all of it runs under `cargo test`
  protocol.rs   Table 1 as a state machine; nothing derivable is stored
  rubric.rs     chips, the mastery scale they are graded on, import and export
  readability.rs Flesch-Kincaid and the complexity guard
  markup.rs     prose and fenced code, so code is not counted as prose
  ingest.rs     splitting an extracted PDF into questions
  hash.rs       SHA-256, the canonical query bytes, the validated identifiers
  view.rs       everything the page renders, so the page decides nothing
  verify.rs     re-derives a run from its attempts; blocking vs advisory
  manifest.rs   the record — structurally incapable of carrying question text
  assignment.rs the document, in Final and Full-history modes
  pdfwrite.rs   the PDF writer — every byte of the output is chosen here
  session.rs    the run as it is paused and moved between machines
crates/wasm/    a thin wasm-bindgen bridge with no logic of its own
crates/cli/     `pw verify`, so the audit does not need the tool that made it
web/            the protocol sheet; pdf.js is vendored, not loaded from a CDN
```

Reading is done by [pdf.js](https://mozilla.github.io/pdf.js/), the engine
Firefox ships. Writing is hand-rolled rather than delegated to a PDF library,
so that "the output contains only what we wrote" is something you can check by
reading one file. Content streams are left uncompressed for the same reason:

```sh
strings assignment.pdf | grep -i bijection
```

## Build version

The build id in the top-right corner identifies exactly which version is
running. Quote it when reporting a bug.

Every internal asset URL carries that hash, because GitHub Pages serves with
`Cache-Control: max-age=600` and a reload inside that window would otherwise
keep using the previous `app.js` and `.wasm` from disk cache — which reads as a
fix that failed to deploy, and can leave a browser mixing a fresh script with a
stale module. A changed build is a different URL, so a stale copy can never be
served for it. `index.html` itself can still lag by up to ten minutes; a hard
reload (Cmd-Shift-R) skips the wait.

## Citation

If this tool is useful in your work, please cite the paper it implements:

```bibtex
@inproceedings{gilson2026perturbation,
  author    = {Gilson, Sam and Tabarsi, Benyamin and Barnes, Tiffany},
  title     = {Adversarial Assignment Perturbation: Effects on Help-seeking
               Behaviors in Student Generative {AI} Chatbot Use},
  booktitle = {Artificial Intelligence in Education (AIED 2026)},
  publisher = {Springer},
  year      = {2026},
  doi       = {10.1007/978-3-032-29770-9_48}
}
```

Gilson, S., Tabarsi, B., & Barnes, T. (2026). *Adversarial Assignment
Perturbation: Effects on Help-seeking Behaviors in Student Generative AI
Chatbot Use.* Artificial Intelligence in Education (AIED 2026). Springer.
<https://doi.org/10.1007/978-3-032-29770-9_48>

## Licence

MIT.
