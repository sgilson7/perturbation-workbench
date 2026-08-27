//! Everything the page renders, decided here.
//!
//! Ground rule for this repository: nothing in `web/app.js` may make a
//! protocol, grading, verification, or manifest decision. That rule is easy to
//! state and easy to erode — the UI needs to know whether to grey out the
//! stamp button, what the banner says, which strategy to suggest next, whether
//! the rubric is still editable — and each of those is a protocol question
//! that a front end will answer for itself if nobody answers it first.
//!
//! So the answer is a single serialisable document. `app.js` renders a `View`
//! and does no arithmetic: every label, every colour name, every enabled or
//! disabled control is a field computed here, under `cargo test`. If the UI
//! shows something wrong, the bug has a test that can be written for it.
//!
//! It is a projection, not state. Nothing here is stored, and it is rebuilt
//! after every change — cheap, because a run is nine questions and not nine
//! thousand, and worth far more than the alternative, which is two
//! representations of the same run drifting apart.

use crate::hash::{Sha256Hex, Timestamp};
use crate::markup::code_blocks;
use crate::protocol::{Question, Settings, Status, Strategy, Tone, MAX_ATTEMPTS};
use crate::readability::{GuardReport, Metrics};
use crate::rubric::{AttemptRef, Chip, ChipKind, Scale};
use crate::session::{Input, Session, Target};
use crate::verify::verify;

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttemptView {
    pub ordinal: usize,
    pub at: Timestamp,
    pub query_sha256: Sha256Hex,
    pub response_sha256: Sha256Hex,
    pub rubric_revision: usize,
    pub pct: f64,
    /// Whether this attempt met the threshold — the single fact the stamp rail
    /// colours itself by.
    pub met_threshold: bool,
    pub notes: Vec<String>,
    /// Whether an observation here has become a penalty chip yet (Step 6).
    pub penalties_derived: usize,
}

/// Why this version cannot take an attempt right now.
///
/// The absence of a grading panel is not an explanation. A question ingested
/// from a PDF starts with no rubric, which is deliberate — nobody's marking
/// scheme can be guessed — but it means the panel is missing on the very first
/// screen a new user sees, with nothing to say why or what to do about it.
///
/// So the reason is a value rather than an inference. `can_stamp` says whether;
/// this says why not, and the page prints it where the panel would have been.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", tag = "blocked")]
pub enum StampBlocked {
    /// No chips yet. A question with no rubric cannot be graded, and a
    /// percentage over nothing is not a number.
    NoRubric,
    /// An older version is on screen. Attempts land on the latest one.
    NotLatest { latest: usize },
    /// Resistant, not resistant, or inconsistent — the protocol's next move is
    /// a perturbation, not another prompt.
    Decided,
    /// Three attempts used.
    AttemptLimit,
}

impl StampBlocked {
    /// What to do about it, in the words the page shows.
    pub fn remedy(self) -> &'static str {
        match self {
            StampBlocked::NoRubric => {
                "Add at least one rubric chip in the panel on the right. One chip is one \
                 atomistic thing the answer either shows or does not; grading needs at least one."
            }
            StampBlocked::NotLatest { .. } => {
                "You are looking at an earlier version. Its attempts are history — select the \
                 latest version above to grade a new one."
            }
            StampBlocked::Decided => {
                "This version is decided. The protocol's next move is a perturbation, on the \
                 bench below."
            }
            StampBlocked::AttemptLimit => {
                "Three attempts is the protocol. Perturb the question on the bench below."
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionView {
    pub ordinal: usize,
    pub strategy: Option<Strategy>,
    /// `v0 · base`, `v1 · Spatial Injection`.
    pub label: String,
    pub text: String,
    pub text_sha256: Sha256Hex,
    /// The eight characters the exported query file is named by, and the ones
    /// worth showing next to a Copy button.
    pub short: String,
    pub locked: bool,
    pub metrics: Metrics,
    pub guard: GuardReport,
    pub code_blocks: usize,
    pub status: Status,
    pub attempts: Vec<AttemptView>,
    /// Whether the grading panel should be live.
    pub can_stamp: bool,
    /// If not, why not. `None` exactly when `can_stamp` is true.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stamp_blocked: Option<StampBlocked>,
    /// What to do about it, in words. Here rather than in the stylesheet's
    /// neighbouring script because the remedy is protocol advice.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stamp_blocked_why: Option<&'static str>,
    pub attempts_left: usize,
    /// Whether the version can still be edited in place, or needs a new one.
    pub editable: bool,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChipView {
    pub id: String,
    pub label: String,
    pub points: i32,
    pub penalty: bool,
    /// Which attempt a penalty chip came from, so the side panel can say why
    /// it is there.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<AttemptRef>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RubricView {
    pub scale: Scale,
    pub chips: Vec<ChipView>,
    pub revisions: usize,
    pub total_points: i32,
    /// Once true the chip editor is read-only and only penalties may be added.
    pub frozen: bool,
    pub empty: bool,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionView {
    pub ordinal: usize,
    pub title: String,
    pub status: Status,
    pub label: &'static str,
    pub tone: Tone,
    pub banner: &'static str,
    /// The strategy to reach for next: the first of the paper's three that has
    /// not been tried on this question yet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested: Option<Strategy>,
    pub versions: Vec<VersionView>,
    pub rubric: RubricView,
    /// How many ledger notes exist that have not become penalty chips. The
    /// number the side panel nudges with, and an advisory at export.
    pub unpromoted_notes: usize,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub settings: Settings,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<Target>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<Input>,
    /// True once anything has been stamped. The target field locks after this,
    /// and the run stops being a draft.
    pub started: bool,
    pub questions: Vec<QuestionView>,
    pub resistant: usize,
    /// Live verification, so the export buttons can say why they are disabled
    /// before they are pressed.
    pub blocking: usize,
    pub advisories: usize,
    pub can_export: bool,
    /// What is blocking, so the export dialog can say which question and why.
    pub blocking_findings: Vec<crate::verify::Finding>,
    /// The paper's three, in order, for the perturbation bench's chips.
    pub strategies: Vec<StrategyView>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StrategyView {
    pub id: Strategy,
    pub name: &'static str,
    pub description: &'static str,
}

/// Table 1, Step 5, in the words the paper uses.
fn describe(s: Strategy) -> &'static str {
    match s {
        Strategy::Spatial => {
            "Add reasoning that requires visual or geometric interpretation of the layout."
        }
        Strategy::Axiomatic => {
            "Alter the premises or axioms so a standard result cannot be recited."
        }
        Strategy::Contextual => {
            "Embed the question in novel, class-specific context the rubric requires."
        }
    }
}

/// The strategy to try next: the first of the three not yet used on this
/// question.
///
/// A suggestion, not a rule — the paper's process is instructor-led and the
/// instructor may have a reason to apply the same strategy twice. What it is
/// not is a random pick or a fixed rotation: reaching for something you have
/// not tried is the whole idea of Step 5, and a question that has had all
/// three is a question where the tool has nothing useful left to say.
fn suggest(q: &Question) -> Option<Strategy> {
    let used: Vec<Strategy> = q.versions().iter().filter_map(|v| v.strategy()).collect();
    Strategy::all().into_iter().find(|s| !used.contains(s))
}

fn chip_view(c: &Chip) -> ChipView {
    ChipView {
        id: c.id.as_str().to_string(),
        label: c.label.clone(),
        points: c.points,
        penalty: c.is_penalty(),
        from: match c.kind {
            ChipKind::Penalty { from } => Some(from),
            ChipKind::Assessed => None,
        },
    }
}

fn question_view(q: &Question, settings: &Settings) -> QuestionView {
    let threshold = settings.threshold;
    let limits = settings.limits();
    let rubric = q.rubric();
    let current = rubric.current();
    let latest = q.latest_ordinal();

    let mut unpromoted = 0;
    let versions: Vec<VersionView> = q
        .versions()
        .iter()
        .enumerate()
        .map(|(vi, v)| {
            let status = q.status_of(vi, threshold).unwrap_or(Status::Untested);

            // In the order a user would hit them: the rubric is the first
            // thing missing on a fresh question, and being on an old version
            // explains everything else that would otherwise look wrong.
            let blocked = if current.chips.is_empty() {
                Some(StampBlocked::NoRubric)
            } else if vi != latest {
                Some(StampBlocked::NotLatest { latest })
            } else if v.attempts().len() >= MAX_ATTEMPTS {
                Some(StampBlocked::AttemptLimit)
            } else if !status.accepts_attempt() {
                Some(StampBlocked::Decided)
            } else {
                None
            };

            let attempts: Vec<AttemptView> = v
                .attempts()
                .iter()
                .enumerate()
                .map(|(i, a)| {
                    let r = AttemptRef { version: vi, attempt: i + 1 };
                    let pct = q.pct(r).unwrap_or(f64::NAN);
                    let derived = rubric.penalties_from(r);
                    if !a.notes().is_empty() && derived == 0 {
                        unpromoted += a.notes().len();
                    }
                    AttemptView {
                        ordinal: i + 1,
                        at: a.at().clone(),
                        query_sha256: a.query().clone(),
                        response_sha256: a.response().clone(),
                        rubric_revision: a.rubric_revision(),
                        pct,
                        met_threshold: pct >= threshold.get(),
                        notes: a.notes().to_vec(),
                        penalties_derived: derived,
                    }
                })
                .collect();

            VersionView {
                ordinal: vi,
                strategy: v.strategy(),
                label: match v.strategy() {
                    None => format!("v{} · base", vi),
                    Some(s) => format!("v{} · {}", vi, s.name()),
                },
                text: v.text().to_string(),
                text_sha256: v.text_sha256(),
                short: v.text_sha256().short().to_string(),
                locked: v.locked(),
                metrics: v.metrics(),
                guard: q.guard_of(vi, &limits).unwrap_or_else(|_| {
                    crate::readability::guard(&v.metrics(), &v.metrics(), &limits)
                }),
                code_blocks: code_blocks(v.text()),
                status,
                // Only the latest version can take an attempt, and only while
                // it is undecided and under the cap. Decided here, not in the
                // front end — and the reason is decided here too.
                can_stamp: blocked.is_none(),
                stamp_blocked: blocked,
                stamp_blocked_why: blocked.map(StampBlocked::remedy),
                attempts_left: MAX_ATTEMPTS.saturating_sub(v.attempts().len()),
                editable: !v.locked(),
                attempts,
            }
        })
        .collect();

    let status = q.status(threshold).unwrap_or(Status::Untested);
    QuestionView {
        ordinal: q.ordinal(),
        title: q.title().to_string(),
        status,
        label: status.label(),
        tone: status.tone(),
        banner: status.banner(),
        suggested: suggest(q),
        versions,
        rubric: RubricView {
            scale: rubric.scale().clone(),
            chips: current.chips.iter().map(chip_view).collect(),
            revisions: rubric.revisions().len(),
            total_points: current.total_points(),
            frozen: rubric.frozen(),
            empty: current.chips.is_empty(),
        },
        unpromoted_notes: unpromoted,
    }
}

/// Project a run into everything the page needs to draw it.
pub fn view(session: &Session) -> View {
    let settings = *session.settings();
    let report = verify(session);
    View {
        settings,
        target: session.target().cloned(),
        input: session.input().cloned(),
        started: session.started(),
        questions: session.questions().iter().map(|q| question_view(q, &settings)).collect(),
        resistant: session.resistant(),
        blocking: report.blocking().len(),
        advisories: report.advisories().len(),
        // Blocking findings refuse the download, and the button says so before
        // it is pressed rather than after.
        can_export: report.passed() && !session.questions().is_empty(),
        strategies: Strategy::all()
            .into_iter()
            .map(|s| StrategyView { id: s, name: s.name(), description: describe(s) })
            .collect(),
        blocking_findings: report.blocking().into_iter().cloned().collect(),
    }
}
