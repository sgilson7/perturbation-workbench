//! The assignment, laid out as a document.
//!
//! Two modes, and they are for two different readers.
//!
//! **Final** is the assignment: each question's resistant text, its rubric, and
//! optionally the observed-hallucination ledger as an instructor appendix. Turn
//! the ledger off and it is the copy the class gets.
//!
//! **Full history** is the show-your-work document — every version in order
//! with the strategy that produced it, its readability figures, its digest, and
//! the three attempt stamps beneath it. It exists because "we perturbed this
//! question until the model failed" is a claim a reviewer or a collaborator at
//! another institution should be able to read rather than take on trust. The
//! manifest proves the same thing in hashes; this says it in sentences.
//!
//! Layout is done here rather than in the writer because it is a decision — how
//! much of a question fits on a page, whether a heading is allowed to sit alone
//! at the bottom of one, how a code block is broken — and decisions belong
//! under `cargo test`. `pdfwrite` places glyphs where it is told.

use crate::markup::{blocks, Block};
use crate::metrics::{points, Font};
use crate::pdfwrite::{approximations, Line, Page, Rule, Run, PAGE_H, PAGE_W};
use crate::protocol::{Question, Status};
use crate::session::Session;

/// Which document to produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    /// The assignment: final questions and rubrics.
    Final,
    /// Every version, every attempt, every strategy.
    FullHistory,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Options {
    pub mode: Mode,
    /// Include the observed-hallucination ledger. Off is the student-facing
    /// copy: the ledger names the model's specific failures, which is a map of
    /// where to push it.
    pub ledger: bool,
    /// The instructor's own title for the document.
    pub title: String,
}

impl Default for Options {
    fn default() -> Self {
        Options { mode: Mode::Final, ledger: true, title: "Assignment".to_string() }
    }
}

/// A finished document and what had to be approximated to make it.
#[derive(Debug, Clone, PartialEq)]
pub struct Rendered {
    pub pages: Vec<Page>,
    /// Characters no base-14 font could draw exactly. Reported rather than
    /// hidden: the `.txt` query files carry the original UTF-8, and a reader
    /// should know when the PDF is the approximation.
    pub approximations: usize,
}

const MARGIN: f32 = 60.0;
const TOP: f32 = PAGE_H - 62.0;
const BOTTOM: f32 = 62.0;
const WIDTH: f32 = PAGE_W - 2.0 * MARGIN;

const BODY: f32 = 10.0;
const CODE: f32 = 8.5;
const META: f32 = 8.0;

/// A cursor down a stack of pages.
struct Flow {
    pages: Vec<Page>,
    y: f32,
    approximations: usize,
}

impl Flow {
    fn new() -> Flow {
        Flow { pages: vec![Page::default()], y: TOP, approximations: 0 }
    }

    fn page(&mut self) -> &mut Page {
        self.pages.last_mut().expect("a flow always has a page")
    }

    fn newpage(&mut self) {
        self.pages.push(Page::default());
        self.y = TOP;
    }

    /// Break unless `height` still fits. Used before a heading so one never
    /// sits alone at the foot of a page with its question overleaf.
    fn keep(&mut self, height: f32) {
        if self.y - height < BOTTOM {
            self.newpage();
        }
    }

    fn gap(&mut self, h: f32) {
        self.y -= h;
    }

    fn rule(&mut self) {
        self.keep(12.0);
        self.gap(6.0);
        let y = self.y;
        self.page().rules.push(Rule { x0: MARGIN, x1: PAGE_W - MARGIN, y, grey: 0.75 });
        self.gap(10.0);
    }

    fn line(&mut self, runs: Vec<Run>, size: f32, grey: f32, indent: f32) {
        self.keep(size * 1.4);
        self.y -= size * 1.15;
        let (x, y) = (MARGIN + indent, self.y);
        self.page().lines.push(Line { runs, x, y, size, grey });
        self.y -= size * 0.25;
    }

    /// One line of prose, wrapped to the margin.
    ///
    /// Runs of whitespace collapse to a single space, which is what a text
    /// layer usually wants — PDF extraction produces ragged spacing and a
    /// question typed in a browser has whatever the keyboard gave it. Anything
    /// where the spacing is the meaning belongs in a code block, where it is
    /// preserved exactly.
    fn wrap(&mut self, text: &str, font: Font, size: f32, grey: f32, indent: f32) {
        self.approximations += approximations(text);
        let max = WIDTH - indent;
        let mut current = String::new();
        for word in text.split_whitespace() {
            let candidate =
                if current.is_empty() { word.to_string() } else { format!("{} {}", current, word) };
            if points(&candidate, font, size) > max && !current.is_empty() {
                self.line(vec![Run::new(current.clone(), font)], size, grey, indent);
                current = word.to_string();
            } else {
                current = candidate;
            }
        }
        if !current.is_empty() {
            self.line(vec![Run::new(current, font)], size, grey, indent);
        }
    }

    /// A code block, set in Courier with its indentation intact.
    ///
    /// Hard-wrapped by character rather than by word: a line of code broken at
    /// a space is not more readable than one broken at column 96, and the
    /// fixed pitch makes the column exact.
    fn code(&mut self, body: &str) {
        let per = points("M", Font::Mono, CODE);
        let cols = ((WIDTH - 10.0) / per).floor().max(20.0) as usize;
        for raw in body.lines() {
            self.approximations += approximations(raw);
            let chars: Vec<char> = raw.chars().collect();
            if chars.is_empty() {
                self.gap(CODE * 0.9);
                continue;
            }
            for chunk in chars.chunks(cols) {
                let text: String = chunk.iter().collect();
                self.line(vec![Run::new(text, Font::Mono)], CODE, 0.15, 10.0);
            }
        }
    }

    /// A question's text: prose wrapped, code left alone.
    ///
    /// Each line of prose is wrapped on its own rather than joined into a
    /// paragraph. The line breaks in a question are structure — `Part 1`,
    /// `Part 2`, a list of cases — and reflowing them into prose loses the
    /// shape the student is meant to answer against.
    fn question_text(&mut self, text: &str) {
        for block in blocks(text) {
            match block {
                Block::Code { body, .. } => {
                    self.gap(3.0);
                    self.code(body);
                    self.gap(4.0);
                }
                Block::Prose(p) => {
                    for line in p.lines() {
                        if line.trim().is_empty() {
                            self.gap(BODY * 0.6);
                        } else {
                            self.wrap(line, Font::Body, BODY, 0.0, 0.0);
                        }
                    }
                }
            }
        }
    }
}

fn status_line(q: &Question, s: &Session) -> String {
    let model = s.target().map(|t| t.model.to_string()).unwrap_or_else(|| "no model named".into());
    match q.status(s.settings().threshold) {
        Ok(Status::Resistant) => format!(
            "One-Shot GenAI Resistant — failed all three attempts against {} at the {}% threshold",
            model,
            s.settings().threshold.get()
        ),
        Ok(other) => format!("In progress — {} against {}", other.label().to_lowercase(), model),
        Err(_) => "In progress — this run could not be re-derived".to_string(),
    }
}

/// Lay out the whole document.
pub fn render(session: &Session, opts: &Options) -> Rendered {
    let mut f = Flow::new();
    let threshold = session.settings().threshold;

    // --- title block ---------------------------------------------------
    f.wrap(&opts.title, Font::Bold, 17.0, 0.0, 0.0);
    f.gap(2.0);
    let what = match opts.mode {
        Mode::Final => "One-Shot GenAI Resistant question set.",
        Mode::FullHistory => {
            "One-Shot GenAI Resistant question set, with the full perturbation history."
        }
    };
    f.wrap(what, Font::Body, META + 1.0, 0.35, 0.0);
    f.wrap(
        "Produced with the Perturbation Workbench, following the iterative perturbation \
         process of Gilson, Tabarsi & Barnes, AIED 2026, Table 1.",
        Font::Body,
        META,
        0.45,
        0.0,
    );
    if let Some(t) = session.target() {
        f.wrap(
            &format!(
                "Target model {} ({}){}. Threshold {}%.",
                t.model,
                t.access.label(),
                if t.fresh_instance_per_attempt { ", a fresh instance per attempt" } else { "" },
                threshold.get()
            ),
            Font::Body,
            META,
            0.45,
            0.0,
        );
    }

    // --- questions -----------------------------------------------------
    for q in session.questions() {
        f.rule();
        f.keep(70.0);
        // A question whose heading line carried no title of its own is titled
        // "Question 4" by the ingester, and "Question 4 — Question 4" reads
        // like a bug even though nothing is wrong.
        let plain = format!("Question {}", q.ordinal());
        let heading = if q.title() == plain || q.title().is_empty() {
            plain
        } else {
            format!("{} — {}", plain, q.title())
        };
        f.wrap(&heading, Font::Bold, 12.5, 0.0, 0.0);
        f.wrap(&status_line(q, session), Font::Body, META, 0.45, 0.0);
        f.gap(5.0);

        match opts.mode {
            Mode::Final => {
                f.question_text(q.latest().text());
            }
            Mode::FullHistory => {
                for (vi, v) in q.versions().iter().enumerate() {
                    f.gap(6.0);
                    f.keep(50.0);
                    let head = match v.strategy() {
                        None => format!("v{} · base", vi),
                        Some(s) => format!("v{} · {}", vi, s.name()),
                    };
                    f.wrap(&head, Font::Bold, 10.5, 0.1, 0.0);
                    let m = v.metrics();
                    f.wrap(
                        &format!(
                            "sha256 {} · Flesch-Kincaid grade {} · {} words",
                            v.text_sha256().short(),
                            m.grade,
                            m.words
                        ),
                        Font::Body,
                        META,
                        0.45,
                        0.0,
                    );
                    f.gap(3.0);
                    f.question_text(v.text());

                    for (i, a) in v.attempts().iter().enumerate() {
                        let pct = q
                            .pct(crate::rubric::AttemptRef { version: vi, attempt: i + 1 })
                            .map(|p| format!("{}%", p))
                            .unwrap_or_else(|_| "unreadable".into());
                        f.wrap(
                            &format!(
                                "attempt {} · {} · response {} · rubric r{} · {}",
                                i + 1,
                                a.at(),
                                a.response().short(),
                                a.rubric_revision(),
                                pct
                            ),
                            Font::Body,
                            META,
                            0.4,
                            10.0,
                        );
                        if opts.ledger {
                            for n in a.notes() {
                                f.wrap(&format!("— {}", n), Font::Body, META, 0.45, 20.0);
                            }
                        }
                    }
                }
            }
        }

        // --- rubric ------------------------------------------------------
        let current = q.rubric().current();
        if !current.chips.is_empty() {
            f.gap(9.0);
            f.keep(40.0);
            f.wrap(
                &format!(
                    "Rubric — {} points, {} scale",
                    current.total_points(),
                    q.rubric().scale().name.to_lowercase()
                ),
                Font::Bold,
                10.0,
                0.0,
                0.0,
            );
            for c in &current.chips {
                let sign = if c.points > 0 { "+" } else { "" };
                f.wrap(&format!("{}{}  {}", sign, c.points, c.label), Font::Body, BODY - 0.5,
                       if c.is_penalty() { 0.25 } else { 0.1 }, 10.0);
            }
        }

        // --- instructor appendix -----------------------------------------
        if opts.ledger && opts.mode == Mode::Final {
            let notes: Vec<&String> = q
                .versions()
                .iter()
                .flat_map(|v| v.attempts().iter().flat_map(|a| a.notes().iter()))
                .collect();
            if !notes.is_empty() {
                f.gap(8.0);
                f.keep(34.0);
                f.wrap("Observed hallucinations (instructor copy)", Font::Bold, 10.0, 0.0, 0.0);
                for n in notes {
                    f.wrap(&format!("— {}", n), Font::Body, BODY - 0.5, 0.35, 10.0);
                }
            }
        }
        f.gap(6.0);
    }

    // --- page numbers ----------------------------------------------------
    let total = f.pages.len();
    for (i, page) in f.pages.iter_mut().enumerate() {
        page.lines.push(Line {
            runs: vec![Run::new(format!("{} of {}", i + 1, total), Font::Body)],
            x: PAGE_W - MARGIN - points(&format!("{} of {}", i + 1, total), Font::Body, META),
            y: 38.0,
            size: META,
            grey: 0.55,
        });
    }

    Rendered { pages: f.pages, approximations: f.approximations }
}
