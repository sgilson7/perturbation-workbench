//! A proof that the protocol was followed, that is not itself the assignment.
//!
//! Running the protocol is half the obligation; being able to show a
//! collaborator at another institution — months later, with no access to the
//! run — that you followed it is the other half. The evidence is: these exact
//! bytes were prompted, this many times, against this named model, graded by
//! this rubric revision, and nothing was stamped resistant that did not earn it.
//!
//! The difficulty is that the obvious log is the assignment. "Question 5 failed
//! three times" plus the text of question 5 is a file that gets emailed around,
//! posted to a repository, and read by the students sitting the exam. A record
//! that leaks the questions it certifies is worse than no record, because it
//! destroys the thing it was made to protect.
//!
//! So the safety property here is structural, exactly as it is in
//! pdf-redactor's `manifest.rs`: not "we strip the question text" but **there
//! is no parameter through which it could arrive.** Every field below is a
//! count, an ordinal, a setting, a hash, an instant, or the name of an enum
//! variant. The four exceptions are validated to be what they claim:
//! `Sha256Hex` is sixty-four hex digits, `Timestamp` is a UTC instant,
//! `BuildId` is a short hex hash, and `ModelId` is bounded to sixty-four
//! characters of the alphabet model names are made of. A caller cannot smuggle
//! a question through a field typed as one of those, and there is no fifth.
//!
//! Two consequences are worth stating because they are easy to want to undo:
//!
//! * **A question is an ordinal, never a title.** "Probability at the Salad
//!   Bar" is question text by any honest reading, and a manifest naming nine
//!   of them tells a reader most of the assignment.
//! * **Ledger notes are counted, not carried.** The observation that a model
//!   invented `P(7,6) = 7^6` is a description of the answer, and the answer is
//!   the question turned inside out. Notes live in the session and in the
//!   instructor appendix of the assignment PDF; the manifest records how many
//!   there were and whether they became penalty chips.
//!
//! ```compile_fail
//! // There is no field a question could be assigned to.
//! use workbench_core::manifest::Input;
//! let _ = Input {
//!     sha256: "Question 4 - The Dessert Menu.".to_string(),
//!     pages: 22,
//!     questions_ingested: 9,
//! };
//! ```

use crate::hash::{BuildId, ModelId, Sha256Hex, Timestamp};
use crate::protocol::{status_from, Settings, Status, Strategy, MAX_ATTEMPTS};
use crate::rubric::AttemptRef;
use crate::session::{Access, Session};
use crate::verify::{verify, Finding, Report};

/// Bumped, not extended, when the shape changes.
pub const SCHEMA: &str = "perturbation-workbench-manifest/1";

const TOOL: &str = "perturbation-workbench";
const PROTOCOL: &str = "Gilson, Tabarsi & Barnes, AIED 2026, Table 1 \
(doi:10.1007/978-3-032-29770-9_48)";
const NOTE: &str = "Question text, model responses and ledger notes do not appear in this \
file by design. Questions are identified by ordinal and by the SHA-256 of their exact query \
bytes; nothing here can be read back into an assignment.";
const HOW_TO_CHECK: &str = "shasum -a 256 question_set/*.txt   # compare against \
questions[].versions[].textSha256\nshasum -a 256 assignment.pdf      # compare against \
outputs.assignmentSha256";

/// Why no manifest was produced.
#[derive(Debug, Clone, PartialEq)]
pub enum ManifestError {
    /// The run contradicts itself. These are §9's blocking findings, and they
    /// refuse the export rather than being written into it: a manifest whose
    /// own audit says the evidence is broken is not evidence.
    Blocked(Vec<Finding>),
    /// A percentage could not be re-derived even though the audit passed,
    /// which would mean these two modules disagree about the same run.
    Inconsistent(AttemptRef),
}

// ---------------------------------------------------------------- the record

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Input {
    pub sha256: Sha256Hex,
    pub pages: usize,
    pub questions_ingested: usize,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Target {
    pub model: ModelId,
    pub access: Access,
    pub fresh_instance_per_attempt: bool,
}

/// The rubric as a shape, not as a rubric. Chip labels are the marking scheme,
/// which is most of the answer key.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RubricSummary {
    pub revisions: usize,
    pub chips: usize,
    pub penalty_chips: usize,
    pub total_points: i32,
    pub scale_levels: usize,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AttemptRecord {
    pub ordinal: usize,
    pub at: Timestamp,
    /// The bytes that were sent. This is the field a collaborator checks.
    pub query_sha256: Sha256Hex,
    pub response_sha256: Sha256Hex,
    pub rubric_revision: usize,
    pub pct: f64,
    /// How many hallucinations were logged against this attempt, not what they
    /// were.
    pub ledger_entries: usize,
    /// Whether those observations became penalty chips (Step 6).
    pub penalties_derived: usize,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VersionRecord {
    pub ordinal: usize,
    pub strategy: Option<Strategy>,
    pub text_sha256: Sha256Hex,
    pub fk_grade: f64,
    pub words: usize,
    pub guard_tripped: bool,
    pub locked: bool,
    /// How many fenced code blocks the question carried, not what was in them.
    /// A reader comparing a CS1 lab against a proof worksheet needs to know
    /// the difference, and a count is the most that can be said safely.
    pub code_blocks: usize,
    pub attempts: Vec<AttemptRecord>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuestionRecord {
    pub ordinal: usize,
    pub rubric: RubricSummary,
    pub versions: Vec<VersionRecord>,
    /// Written from the attempts by `verify`, never copied from UI state.
    pub status: Status,
}

/// What the audit found.
///
/// `blocking` is always empty in a manifest that exists, because a blocking
/// finding refuses the export. It is present rather than omitted so that the
/// file states the check ran and passed, instead of leaving a reader to infer
/// it from an absence.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Verification {
    pub blocking: Vec<Finding>,
    pub advisories: Vec<Finding>,
}

/// The files this manifest accompanies. Filled in once they have been written
/// and re-read, which is why it is optional: a manifest can be taken mid-run.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Outputs {
    pub assignment_sha256: Sha256Hex,
    pub includes_history: bool,
    pub includes_ledger: bool,
    pub query_files: usize,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    pub schema: String,
    pub tool: String,
    pub build: BuildId,
    pub protocol: String,
    pub created: Timestamp,
    /// Stated in the file so a reader can see the intent rather than infer it.
    pub contains_question_text: bool,
    pub note: String,
    pub how_to_check: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<Input>,
    pub settings: Settings,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<Target>,
    pub questions: Vec<QuestionRecord>,
    pub verification: Verification,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outputs: Option<Outputs>,
}

/// Build the run's manifest, or refuse.
///
/// Verification is re-run here from the session rather than accepting a report
/// from the caller, mirroring the redactor's habit of re-reading its own output
/// instead of trusting what it meant to write. A caller cannot hand in a clean
/// report for a dirty run, because it cannot hand in a report at all.
pub fn build(
    build: BuildId,
    created: Timestamp,
    session: &Session,
    outputs: Option<Outputs>,
) -> Result<Manifest, ManifestError> {
    let report = verify(session);
    if !report.passed() {
        return Err(ManifestError::Blocked(
            report.findings.into_iter().filter(Finding::is_blocking).collect(),
        ));
    }

    let settings = *session.settings();
    let limits = settings.limits();
    let mut questions = Vec::with_capacity(session.questions().len());

    for q in session.questions() {
        let rubric = q.rubric();
        let current = rubric.current();
        let mut versions = Vec::with_capacity(q.versions().len());

        for (vi, v) in q.versions().iter().enumerate() {
            let metrics = v.metrics();
            let mut attempts = Vec::with_capacity(v.attempts().len());
            for (i, a) in v.attempts().iter().enumerate() {
                let r = AttemptRef { version: vi, attempt: i + 1 };
                let pct = q.pct(r).map_err(|_| ManifestError::Inconsistent(r))?;
                attempts.push(AttemptRecord {
                    ordinal: i + 1,
                    at: a.at().clone(),
                    query_sha256: a.query().clone(),
                    response_sha256: a.response().clone(),
                    rubric_revision: a.rubric_revision(),
                    pct,
                    ledger_entries: a.notes().len(),
                    penalties_derived: rubric.penalties_from(r),
                });
            }
            versions.push(VersionRecord {
                ordinal: vi,
                strategy: v.strategy(),
                text_sha256: v.text_sha256(),
                fk_grade: metrics.grade,
                words: metrics.words,
                guard_tripped: q.guard_of(vi, &limits).is_ok_and(|g| g.tripped()),
                locked: v.locked(),
                code_blocks: crate::markup::code_blocks(v.text()),
                attempts,
            });
        }

        questions.push(QuestionRecord {
            ordinal: q.ordinal(),
            rubric: RubricSummary {
                revisions: rubric.revisions().len(),
                chips: current.assessed().count(),
                penalty_chips: current.penalties().count(),
                total_points: current.total_points(),
                scale_levels: rubric.scale().levels.len(),
            },
            versions,
            status: q
                .status(settings.threshold)
                .map_err(|_| ManifestError::Inconsistent(AttemptRef { version: 0, attempt: 0 }))?,
        });
    }

    Ok(Manifest {
        schema: SCHEMA.to_string(),
        tool: TOOL.to_string(),
        build,
        protocol: PROTOCOL.to_string(),
        created,
        contains_question_text: false,
        note: NOTE.to_string(),
        how_to_check: HOW_TO_CHECK.to_string(),
        input: session.input().map(|i| Input {
            sha256: i.sha256.clone(),
            pages: i.pages,
            questions_ingested: session.questions().len(),
        }),
        settings,
        target: session.target().map(|t| Target {
            model: t.model.clone(),
            access: t.access,
            fresh_instance_per_attempt: t.fresh_instance_per_attempt,
        }),
        questions,
        verification: Verification {
            blocking: Vec::new(),
            advisories: report.advisories().into_iter().cloned().collect(),
        },
        outputs,
    })
}

/// Re-check a manifest against itself.
///
/// This is the check a collaborator can run, and it is deliberately not the
/// same check as `verify`. They have the run; a reader of the manifest has
/// only what the file says, so the question is narrower: do the recorded
/// attempts actually produce the recorded status, are the query digests on a
/// resistant version all the same digest as its text, and did the tool's own
/// audit pass. Everything it needs is in the file, which is the point — a
/// proof nobody can check without the original is not a proof.
///
/// The status is re-derived rather than read. `questions[].status` is a claim
/// like any other, and a manifest is a JSON file somebody could edit before
/// forwarding it.
pub fn audit(m: &Manifest) -> Report {
    let mut findings = m.verification.blocking.clone();
    let threshold = m.settings.threshold.get();
    let (mut versions, mut attempts, mut resistant) = (0, 0, 0);

    for (qi, q) in m.questions.iter().enumerate() {
        for v in &q.versions {
            versions += 1;
            attempts += v.attempts.len();

            if v.attempts.len() > MAX_ATTEMPTS {
                findings.push(Finding::TooManyAttempts {
                    question: qi,
                    version: v.ordinal,
                    attempts: v.attempts.len(),
                });
            }
            if v.attempts.iter().any(|a| a.query_sha256 != v.text_sha256) {
                findings.push(Finding::TextChangedAfterPrompting {
                    question: qi,
                    version: v.ordinal,
                });
            }
            if let Some(first) = v.attempts.first().map(|a| &a.query_sha256) {
                for a in v.attempts.iter().skip(1) {
                    if &a.query_sha256 != first {
                        findings.push(Finding::QueryChangedBetweenAttempts {
                            question: qi,
                            version: v.ordinal,
                            attempt: a.ordinal,
                        });
                    }
                }
            }
        }

        let Some(latest) = q.versions.last() else { continue };
        let pcts: Vec<f64> = latest.attempts.iter().map(|a| a.pct).collect();
        let derived = status_from(&pcts, threshold);
        if derived != q.status {
            findings.push(Finding::StatusDoesNotFollow { question: qi });
        }
        if q.status.is_resistant() {
            resistant += 1;
            if latest.attempts.len() != MAX_ATTEMPTS {
                findings.push(Finding::TooFewAttempts {
                    question: qi,
                    version: latest.ordinal,
                    attempts: latest.attempts.len(),
                });
            }
            for a in &latest.attempts {
                if a.pct >= threshold {
                    findings.push(Finding::AttemptMetThreshold {
                        question: qi,
                        version: latest.ordinal,
                        attempt: a.ordinal,
                    });
                }
            }
        } else {
            findings.push(Finding::InProgress { question: qi, state: q.status });
        }
    }

    if m.target.is_none() && attempts > 0 {
        findings.push(Finding::NoTargetRecorded);
    }

    Report { findings, questions: m.questions.len(), versions, attempts, resistant }
}
