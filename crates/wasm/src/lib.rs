//! Thin bridge between the browser and `workbench-core`.
//!
//! Deliberately contains no logic worth testing: everything that decides what
//! the protocol allows, what a percentage is, what the page should render, and
//! what may be exported lives in `core`, where it runs under `cargo test`
//! without a browser. This file moves JSON across the boundary and nothing
//! else.
//!
//! The run itself lives here rather than in JavaScript. Handing the session to
//! the front end and taking it back on every edit would mean the one structure
//! whose integrity the whole tool rests on spends most of its life as a mutable
//! object in a language that cannot refuse an invalid change. Holding it in
//! Rust means every transition goes through a method that can say no.

use workbench_core::assignment::{self, Options};
use workbench_core::hash::{canonical, BuildId, ModelId, Sha256Hex, Timestamp};
use workbench_core::ingest::{ingest, Line};
use workbench_core::manifest::{self, Outputs};
use workbench_core::pdfwrite;
use workbench_core::protocol::{Question, Settings, Strategy};
use workbench_core::rubric::{AttemptRef, ChipId, Percent, RubricDoc, Scale, Scores};
use workbench_core::session::{Access, Session};
use workbench_core::{verify, view};

use wasm_bindgen::prelude::*;

/// Panics inside wasm otherwise surface as an opaque "unreachable executed".
#[wasm_bindgen(start)]
pub fn start() {
    std::panic::set_hook(Box::new(|info| {
        log(&format!("workbench panic: {}", info));
    }));
}

#[wasm_bindgen(inline_js = "export function log(s) { console.error(s); }")]
extern "C" {
    fn log(s: &str);
}

fn err<E: std::fmt::Debug>(e: E) -> JsValue {
    JsValue::from_str(&format!("{:?}", e))
}

fn json<T: serde::Serialize>(v: &T) -> Result<String, JsValue> {
    serde_json::to_string(v).map_err(err)
}

/// A run, held across calls.
#[wasm_bindgen]
pub struct Workbench {
    session: Session,
}

#[wasm_bindgen]
impl Workbench {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Workbench {
        Workbench { session: Session::default() }
    }

    /// Restore a run from `localStorage` or an imported file.
    ///
    /// Returns an error rather than an empty run when the JSON is not a
    /// session this build understands, so a schema mismatch surfaces as a
    /// message instead of as silently losing somebody's afternoon.
    pub fn load(json: &str) -> Result<Workbench, JsValue> {
        let session: Session = serde_json::from_str(json).map_err(err)?;
        Ok(Workbench { session })
    }

    /// The run, for `localStorage` and for export.
    pub fn save(&self) -> Result<String, JsValue> {
        serde_json::to_string(&self.session).map_err(err)
    }

    /// Everything the page renders.
    pub fn view(&self) -> Result<String, JsValue> {
        json(&view::view(&self.session))
    }

    // ---------------------------------------------------------------- setup

    #[wasm_bindgen(js_name = setSettings)]
    pub fn set_settings(&mut self, settings_json: &str) -> Result<(), JsValue> {
        let settings: Settings = serde_json::from_str(settings_json).map_err(err)?;
        self.session.set_settings(settings);
        Ok(())
    }

    #[wasm_bindgen(js_name = setTarget)]
    pub fn set_target(
        &mut self,
        model: &str,
        access: &str,
        fresh_instance_per_attempt: bool,
        at: &str,
    ) -> Result<(), JsValue> {
        let access: Access = serde_json::from_str(&format!("\"{}\"", access)).map_err(err)?;
        let at = Timestamp::parse(at).map_err(err)?;
        self.session
            .set_target(model, access, fresh_instance_per_attempt, at)
            .map_err(err)
    }

    /// Split an extracted PDF into drafts and load them as questions.
    ///
    /// Returns the ingest summary so the UI can say what it did — how many
    /// footers it dropped, which numbering convention it recognised — rather
    /// than presenting the result as if it were obvious.
    pub fn ingest(
        &mut self,
        lines_json: &str,
        sha256: &str,
        pages: usize,
    ) -> Result<String, JsValue> {
        let lines: Vec<Line> = serde_json::from_str(lines_json).map_err(err)?;
        let out = ingest(&lines);
        self.session.set_input(Sha256Hex::parse(sha256).map_err(err)?, pages);
        for d in &out.drafts {
            let q = Question::new(d.ordinal, &d.title, &d.text).map_err(err)?;
            self.session.add_question(q);
        }
        json(&out)
    }

    /// Start a question from nothing, which is how the tool opens.
    #[wasm_bindgen(js_name = addQuestion)]
    pub fn add_question(
        &mut self,
        ordinal: usize,
        title: &str,
        text: &str,
    ) -> Result<usize, JsValue> {
        let q = Question::new(ordinal, title, text).map_err(err)?;
        Ok(self.session.add_question(q))
    }

    // ---------------------------------------------------------------- editing

    #[wasm_bindgen(js_name = editVersion)]
    pub fn edit_version(&mut self, q: usize, v: usize, text: &str) -> Result<(), JsValue> {
        self.session.question_mut(q).map_err(err)?.edit(v, text).map_err(err)
    }

    #[wasm_bindgen(js_name = retitle)]
    pub fn retitle(&mut self, q: usize, title: &str) -> Result<(), JsValue> {
        self.session.question_mut(q).map_err(err)?.retitle(title);
        Ok(())
    }

    #[wasm_bindgen(js_name = addVersion)]
    pub fn add_version(&mut self, q: usize, strategy: &str, text: &str) -> Result<usize, JsValue> {
        let s: Strategy = serde_json::from_str(&format!("\"{}\"", strategy)).map_err(err)?;
        self.session.question_mut(q).map_err(err)?.add_version(s, text).map_err(err)
    }

    #[wasm_bindgen(js_name = discardLatest)]
    pub fn discard_latest(&mut self, q: usize) -> Result<(), JsValue> {
        self.session.question_mut(q).map_err(err)?.discard_latest().map_err(err)
    }

    // ---------------------------------------------------------------- rubric

    #[wasm_bindgen(js_name = addChip)]
    pub fn add_chip(&mut self, q: usize, label: &str, points: i32) -> Result<String, JsValue> {
        let id =
            self.session.question_mut(q).map_err(err)?.rubric_mut().add_chip(label, points).map_err(err)?;
        Ok(id.as_str().to_string())
    }

    #[wasm_bindgen(js_name = editChip)]
    pub fn edit_chip(
        &mut self,
        q: usize,
        id: &str,
        label: &str,
        points: i32,
    ) -> Result<(), JsValue> {
        let id = ChipId::parse(id).map_err(err)?;
        self.session
            .question_mut(q)
            .map_err(err)?
            .rubric_mut()
            .edit_chip(&id, label, points)
            .map_err(err)
    }

    #[wasm_bindgen(js_name = removeChip)]
    pub fn remove_chip(&mut self, q: usize, id: &str) -> Result<(), JsValue> {
        let id = ChipId::parse(id).map_err(err)?;
        self.session.question_mut(q).map_err(err)?.rubric_mut().remove_chip(&id).map_err(err)
    }

    #[wasm_bindgen(js_name = setScale)]
    pub fn set_scale(&mut self, q: usize, scale_json: &str) -> Result<(), JsValue> {
        let scale: Scale = serde_json::from_str(scale_json).map_err(err)?;
        self.session.question_mut(q).map_err(err)?.rubric_mut().set_scale(scale).map_err(err)
    }

    #[wasm_bindgen(js_name = importRubric)]
    pub fn import_rubric(&mut self, q: usize, doc_json: &str) -> Result<(), JsValue> {
        let doc: RubricDoc = serde_json::from_str(doc_json).map_err(err)?;
        self.session.question_mut(q).map_err(err)?.rubric_mut().import(&doc).map_err(err)
    }

    #[wasm_bindgen(js_name = exportRubric)]
    pub fn export_rubric(&self, q: usize) -> Result<String, JsValue> {
        let doc = self.session.question(q).map_err(err)?.rubric().export();
        serde_json::to_string_pretty(&doc).map_err(err)
    }

    /// The built-in scales, so the scale picker offers them without inventing
    /// its own.
    #[wasm_bindgen(js_name = builtInScales)]
    pub fn built_in_scales() -> Result<String, JsValue> {
        json(&[Scale::partial_credit(), Scale::mastery()])
    }

    // ---------------------------------------------------------------- stamping

    /// Record one graded attempt.
    ///
    /// `scores_json` is `{ "chip-id": 50.0 }`. The response digest is computed
    /// in the browser and the response text is dropped there; there is no
    /// parameter here through which it could arrive.
    pub fn stamp(
        &mut self,
        q: usize,
        v: usize,
        at: &str,
        response_sha256: &str,
        scores_json: &str,
    ) -> Result<usize, JsValue> {
        let raw: std::collections::BTreeMap<String, f64> =
            serde_json::from_str(scores_json).map_err(err)?;
        let mut scores: Scores = Scores::new();
        for (id, pct) in raw {
            scores.insert(ChipId::parse(&id).map_err(err)?, Percent::new(pct).map_err(err)?);
        }
        self.session
            .stamp(
                q,
                v,
                Timestamp::parse(at).map_err(err)?,
                Sha256Hex::parse(response_sha256).map_err(err)?,
                scores,
            )
            .map_err(err)
    }

    pub fn note(&mut self, q: usize, v: usize, attempt: usize, text: &str) -> Result<(), JsValue> {
        self.session
            .question_mut(q)
            .map_err(err)?
            .note(AttemptRef { version: v, attempt }, text)
            .map_err(err)
    }

    #[wasm_bindgen(js_name = addPenalty)]
    pub fn add_penalty(
        &mut self,
        q: usize,
        label: &str,
        points: i32,
        v: usize,
        attempt: usize,
        at: &str,
    ) -> Result<usize, JsValue> {
        self.session
            .question_mut(q)
            .map_err(err)?
            .add_penalty(
                label,
                points,
                AttemptRef { version: v, attempt },
                Timestamp::parse(at).map_err(err)?,
            )
            .map_err(err)
    }

    // ---------------------------------------------------------------- readouts

    /// Measure a draft against the question's base text, live.
    #[wasm_bindgen(js_name = guardDraft)]
    pub fn guard_draft(&self, q: usize, draft: &str) -> Result<String, JsValue> {
        let question = self.session.question(q).map_err(err)?;
        json(&question.guard_draft(draft, &self.session.settings().limits()))
    }

    /// The digest of a draft, so the bench can show what it would be hashed as
    /// before it is saved. Canonicalised here, because the digest has to be of
    /// the bytes that would actually be stored.
    #[wasm_bindgen(js_name = digestOf)]
    pub fn digest_of(text: &str) -> String {
        Sha256Hex::of(canonical(text).as_bytes()).to_string()
    }

    /// The canonical form of a draft, which is what Copy puts on the clipboard
    /// and what the exported query file contains.
    #[wasm_bindgen(js_name = canonicalise)]
    pub fn canonicalise(text: &str) -> String {
        canonical(text)
    }

    // ---------------------------------------------------------------- export

    pub fn verify(&self) -> Result<String, JsValue> {
        json(&verify::verify(&self.session))
    }

    /// Build the run manifest, or refuse.
    #[wasm_bindgen(js_name = manifest)]
    pub fn manifest_json(
        &self,
        build: &str,
        created: &str,
        outputs_json: &str,
    ) -> Result<String, JsValue> {
        let outputs: Option<Outputs> = if outputs_json.is_empty() {
            None
        } else {
            Some(serde_json::from_str(outputs_json).map_err(err)?)
        };
        let m = manifest::build(
            BuildId::parse(build).map_err(err)?,
            Timestamp::parse(created).map_err(err)?,
            &self.session,
            outputs,
        )
        .map_err(err)?;
        serde_json::to_string_pretty(&m).map_err(err)
    }

    /// The exact-query files: one per version, named by digest prefix.
    ///
    /// Named here rather than in the front end because the name is part of the
    /// evidence — a collaborator runs `shasum` on the file and compares it with
    /// the manifest, and the two have to agree about which digest that is.
    #[wasm_bindgen(js_name = queryFiles)]
    pub fn query_files(&self, resistant_only: bool) -> Result<String, JsValue> {
        #[derive(serde::Serialize)]
        #[serde(rename_all = "camelCase")]
        struct File {
            name: String,
            text: String,
            question: usize,
            version: usize,
        }
        let threshold = self.session.settings().threshold;
        let mut out = Vec::new();
        for q in self.session.questions() {
            let resistant = q.status(threshold).is_ok_and(|s| s.is_resistant());
            if resistant_only && !resistant {
                continue;
            }
            let v = q.latest();
            out.push(File {
                name: format!("q{}-{}.txt", q.ordinal(), v.text_sha256().short()),
                text: v.text().to_string(),
                question: q.ordinal(),
                version: q.latest_ordinal(),
            });
        }
        json(&out)
    }

    /// Typeset the assignment.
    ///
    /// Returns the bytes and what had to be approximated to produce them. The
    /// caller re-opens the result with pdf.js before recording its digest,
    /// which is the same move the redactor makes: a writer that never re-reads
    /// its own output is asking to be trusted rather than demonstrating it
    /// deserves to be.
    pub fn assignment(&self, options_json: &str) -> Result<Assignment, JsValue> {
        let opts: Options = serde_json::from_str(options_json).map_err(err)?;
        let out = assignment::render(&self.session, &opts);
        Ok(Assignment {
            pages: out.pages.len(),
            approximations: out.approximations,
            bytes: pdfwrite::build(&out.pages),
        })
    }

    /// A model identifier the manifest will accept, checked before the run
    /// starts rather than at export.
    #[wasm_bindgen(js_name = isModelId)]
    pub fn is_model_id(s: &str) -> bool {
        ModelId::parse(s).is_ok()
    }
}

/// A typeset assignment, on its way out of the module.
#[wasm_bindgen]
pub struct Assignment {
    bytes: Vec<u8>,
    pages: usize,
    approximations: usize,
}

#[wasm_bindgen]
impl Assignment {
    /// Takes the bytes rather than borrowing them, so the document is released
    /// as soon as the caller holds it.
    pub fn take(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.bytes)
    }

    #[wasm_bindgen(getter)]
    pub fn pages(&self) -> usize {
        self.pages
    }

    /// Characters no base-14 font could draw exactly. Shown to the user rather
    /// than hidden: the `.txt` query files carry the original UTF-8, and a
    /// reader should know when the PDF is the approximation.
    #[wasm_bindgen(getter)]
    pub fn approximations(&self) -> usize {
        self.approximations
    }
}

impl Default for Workbench {
    fn default() -> Self {
        Self::new()
    }
}
