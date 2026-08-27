//! Proving the run is what it says it is, by re-deriving it from the attempts.
//!
//! Every other module asserts what it *intended*. This one starts from the
//! recorded attempts and works forward, and it deliberately does not ask
//! `protocol` for the answer it is checking. `Question::status` says a version
//! is resistant; this module independently asserts the three things that must
//! be true if it is — three attempts, every one below the line, and all three
//! carrying the same query digest as the text they are attached to. A check
//! that delegated to the function it is checking would prove nothing about it.
//!
//! The two tiers are pdf-redactor's, and the distinction is the same one:
//!
//! * **Blocking** — the run contradicts itself. Something is wrong with the
//!   evidence, not with the judgement, and no export happens.
//! * **Advisory** — the instructor made a call this module is not entitled to
//!   overrule. A question saved with the complexity guard tripped, a question
//!   still in progress, a target changed halfway through. Reported into the
//!   manifest so the choice stays deliberate rather than forgotten.
//!
//! Most of the blocking cases cannot be reached through `protocol`'s API at
//! all: a prompted version is locked, a fourth attempt is refused, an attempt
//! graded before its rubric is refused. They are checked anyway, because a
//! session is a JSON file on somebody's disk and the API is not the only way
//! to write one. Findings name nothing but ordinals and enum values, so a
//! report can be pasted into a manifest without carrying a question with it.

use crate::protocol::{Status, MAX_ATTEMPTS};
use crate::rubric::{AttemptRef, ChipKind};
use crate::session::Session;

/// Something the run says that the run does not support.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", tag = "finding")]
pub enum Finding {
    // ---------------------------------------------------------- blocking
    /// The text of a version differs from the bytes its attempts recorded
    /// prompting. Whatever was tested, it was not this.
    TextChangedAfterPrompting { question: usize, version: usize },
    /// Two attempts on one version prompted different bytes, so "the same
    /// query, three times" is not what happened.
    QueryChangedBetweenAttempts { question: usize, version: usize, attempt: usize },
    /// Claimed resistant on fewer than three attempts.
    TooFewAttempts { question: usize, version: usize, attempts: usize },
    /// More attempts than the protocol allows.
    TooManyAttempts { question: usize, version: usize, attempts: usize },
    /// Claimed resistant with an attempt that met the threshold.
    AttemptMetThreshold { question: usize, version: usize, attempt: usize },
    /// Graded against a rubric revision that did not exist at that time.
    RubricRevisionDidNotExist { question: usize, version: usize, attempt: usize },
    /// Graded against a rubric revision that is not in the file.
    RubricRevisionMissing { question: usize, version: usize, attempt: usize },
    /// The recorded marks do not fit the revision that is supposed to have
    /// produced them, so the percentage cannot be re-derived at all.
    GradeCannotBeRederived { question: usize, version: usize, attempt: usize },
    /// A penalty chip pointing at an attempt that is not in the file. Without
    /// provenance it is a rubric tuned until the model failed.
    PenaltyWithoutProvenance { question: usize },
    /// Attempts exist but the run never named what it was testing against.
    NoTargetRecorded,
    /// A manifest's recorded status is not the one its own attempts produce.
    /// Only reachable from `manifest::audit`, where the attempts and the claim
    /// arrive as separate fields of a file somebody else wrote.
    StatusDoesNotFollow { question: usize },

    // ---------------------------------------------------------- advisory
    /// Saved with the complexity guard tripped. The instructor's call: a
    /// question that got harder to read may still be the right question.
    GuardTripped { question: usize, version: usize },
    /// Not finished. Exporting mid-run is legitimate and worth saying.
    InProgress { question: usize, state: Status },
    /// The run changed target models. Both are in the session; the manifest
    /// names the last.
    TargetChanged { targets: usize },
    /// Ledger notes that never became penalty chips. Step 6 is where an
    /// observed hallucination turns into something the rubric penalises, and
    /// notes left un-promoted are the observations that did not.
    NotesNotPromoted { question: usize, version: usize, attempt: usize, notes: usize },
}

impl Finding {
    /// Blocking findings refuse the export; advisory ones are written into it.
    pub fn is_blocking(&self) -> bool {
        !matches!(
            self,
            Finding::GuardTripped { .. }
                | Finding::InProgress { .. }
                | Finding::TargetChanged { .. }
                | Finding::NotesNotPromoted { .. }
        )
    }
}

/// What the audit found, and what it counted on the way.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Report {
    pub findings: Vec<Finding>,
    pub questions: usize,
    pub versions: usize,
    pub attempts: usize,
    pub resistant: usize,
}

impl Report {
    /// True when nothing blocking was found. Advisories do not fail.
    pub fn passed(&self) -> bool {
        !self.findings.iter().any(Finding::is_blocking)
    }

    pub fn blocking(&self) -> Vec<&Finding> {
        self.findings.iter().filter(|f| f.is_blocking()).collect()
    }

    pub fn advisories(&self) -> Vec<&Finding> {
        self.findings.iter().filter(|f| !f.is_blocking()).collect()
    }
}

/// Re-derive a whole run and report where it disagrees with itself.
pub fn verify(session: &Session) -> Report {
    let mut findings = Vec::new();
    let threshold = session.settings().threshold;
    let limits = session.settings().limits();
    let (mut versions, mut attempts, mut resistant) = (0, 0, 0);

    for (qi, q) in session.questions().iter().enumerate() {
        // Every penalty chip must point at an attempt that is in this file.
        for chip in q.rubric().current().penalties() {
            if let ChipKind::Penalty { from } = chip.kind {
                if q.attempt(from).is_err() {
                    findings.push(Finding::PenaltyWithoutProvenance { question: qi });
                }
            }
        }

        for (vi, v) in q.versions().iter().enumerate() {
            versions += 1;
            attempts += v.attempts().len();

            if v.attempts().len() > MAX_ATTEMPTS {
                findings.push(Finding::TooManyAttempts {
                    question: qi,
                    version: vi,
                    attempts: v.attempts().len(),
                });
            }

            // The bytes on file must be the bytes that were sent.
            let now = v.text_sha256();
            if v.attempts().iter().any(|a| a.query() != &now) {
                findings.push(Finding::TextChangedAfterPrompting { question: qi, version: vi });
            }
            // ...and they must not have moved between attempts.
            if let Some(first) = v.attempts().first().map(|a| a.query()) {
                for (i, a) in v.attempts().iter().enumerate().skip(1) {
                    if a.query() != first {
                        findings.push(Finding::QueryChangedBetweenAttempts {
                            question: qi,
                            version: vi,
                            attempt: i + 1,
                        });
                    }
                }
            }

            for (i, a) in v.attempts().iter().enumerate() {
                let r = AttemptRef { version: vi, attempt: i + 1 };
                match q.rubric().revision(a.rubric_revision()) {
                    Err(_) => findings.push(Finding::RubricRevisionMissing {
                        question: qi,
                        version: vi,
                        attempt: i + 1,
                    }),
                    Ok(rev) => {
                        if let Some(since) = &rev.at {
                            if a.at() < since {
                                findings.push(Finding::RubricRevisionDidNotExist {
                                    question: qi,
                                    version: vi,
                                    attempt: i + 1,
                                });
                            }
                        }
                        if rev.grade(q.rubric().scale(), a.scores()).is_err() {
                            findings.push(Finding::GradeCannotBeRederived {
                                question: qi,
                                version: vi,
                                attempt: i + 1,
                            });
                        }
                    }
                }

                // Step 6: an observation that never became a penalty chip.
                if !a.notes().is_empty() && q.rubric().penalties_from(r) == 0 {
                    findings.push(Finding::NotesNotPromoted {
                        question: qi,
                        version: vi,
                        attempt: i + 1,
                        notes: a.notes().len(),
                    });
                }
            }

            if q.guard_of(vi, &limits).is_ok_and(|g| g.tripped()) {
                findings.push(Finding::GuardTripped { question: qi, version: vi });
            }
        }

        // The claim, checked against its own evidence rather than against the
        // function that made it.
        let vi = q.latest_ordinal();
        // Unreadable is not a state a run can be in; it is a state a *file* can
        // be in, and the per-attempt checks above have already said which
        // attempt made it so.
        let Ok(state) = q.status(threshold) else { continue };
        if state.is_resistant() {
            resistant += 1;
            let latest = q.latest();
            if latest.attempts().len() != MAX_ATTEMPTS {
                findings.push(Finding::TooFewAttempts {
                    question: qi,
                    version: vi,
                    attempts: latest.attempts().len(),
                });
            }
            if let Ok(pcts) = q.pcts(vi) {
                for (i, p) in pcts.iter().enumerate() {
                    if *p >= threshold.get() {
                        findings.push(Finding::AttemptMetThreshold {
                            question: qi,
                            version: vi,
                            attempt: i + 1,
                        });
                    }
                }
            }
        } else {
            findings.push(Finding::InProgress { question: qi, state });
        }
    }

    if attempts > 0 && session.target().is_none() {
        findings.push(Finding::NoTargetRecorded);
    }
    if session.targets().len() > 1 {
        findings.push(Finding::TargetChanged { targets: session.targets().len() });
    }

    Report { findings, questions: session.questions().len(), versions, attempts, resistant }
}
