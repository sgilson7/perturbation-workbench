//! Table 1, transition by transition, and every rule that refuses.
//!
//! The transitions are checked against the prototype's `versionStatus` so the
//! two agree; the refusals are checked because they are the difference between
//! a workbench and a record.

mod common;

use common::{at, digest, pct, simple};
use workbench_core::hash::Sha256Hex;
use workbench_core::protocol::{
    status_from, ProtocolError, Question, Status, Strategy, MAX_ATTEMPTS,
};
use workbench_core::readability::Limits;
use workbench_core::rubric::{AttemptRef, Percent, RubricError, Scores};

const THR: f64 = 60.0;

fn thr() -> Percent {
    pct(THR)
}

/// Mark the current revision's chips, in order.
fn marks(q: &Question, vals: &[f64]) -> Scores {
    q.rubric()
        .current()
        .chips
        .iter()
        .zip(vals)
        .map(|(c, v)| (c.id.clone(), pct(*v)))
        .collect()
}

/// `simple()` is worth 2 + 8 points, so these land on round percentages.
const FAIL: &[f64] = &[100.0, 0.0]; // 20%
const EDGE: &[f64] = &[100.0, 50.0]; // exactly 60%
const PASS: &[f64] = &[100.0, 100.0]; // 100%

/// Stamp the latest version, marking the current revision's chips in order.
fn stamp(q: &mut Question, minute: u32, vals: &[f64]) -> Result<usize, ProtocolError> {
    let scores = marks(q, vals);
    let seed = format!("response-{}", minute);
    q.stamp(q.latest_ordinal(), thr(), at(minute), digest(&seed), scores)
}

// ------------------------------------------------------------ the transitions

/// The table itself, over percentages alone, transcribed from the prototype.
#[test]
fn the_transition_table_agrees_with_the_prototype() {
    let cases: &[(&[f64], Status)] = &[
        (&[], Status::Untested),
        (&[80.0], Status::NotResistant { pct: 80.0 }),
        // `>=` is the prototype's comparison: exactly at the line is not
        // resistant. The paper's rule is "< 60% is resistant".
        (&[60.0], Status::NotResistant { pct: 60.0 }),
        (&[59.9], Status::Testing { attempts: 1 }),
        (&[40.0, 40.0], Status::Testing { attempts: 2 }),
        (&[40.0, 40.0, 40.0], Status::Resistant),
        (&[40.0, 80.0], Status::Inconsistent { at: 2 }),
        (&[40.0, 40.0, 80.0], Status::Inconsistent { at: 3 }),
        // A first-attempt pass is decided by the first attempt, whatever
        // followed it.
        (&[80.0, 40.0, 40.0], Status::NotResistant { pct: 80.0 }),
    ];
    for (pcts, expect) in cases {
        assert_eq!(status_from(pcts, THR), *expect, "{:?}", pcts);
    }
}

#[test]
fn an_untested_version_is_untested() {
    let q = simple();
    assert_eq!(q.status(thr()), Ok(Status::Untested));
    assert!(!q.base().locked());
    assert!(!q.frozen());
}

#[test]
fn a_first_attempt_at_the_threshold_is_not_resistant() {
    let mut q = simple();
    stamp(&mut q, 0, EDGE).unwrap();
    assert_eq!(q.status(thr()), Ok(Status::NotResistant { pct: 60.0 }));
    assert!(!q.status(thr()).unwrap().is_resistant());
}

#[test]
fn three_failures_are_resistant_and_two_are_not() {
    let mut q = simple();
    assert_eq!(stamp(&mut q, 0, FAIL), Ok(1));
    assert_eq!(q.status(thr()), Ok(Status::Testing { attempts: 1 }));
    assert_eq!(stamp(&mut q, 5, FAIL), Ok(2));
    assert_eq!(q.status(thr()), Ok(Status::Testing { attempts: 2 }));
    assert_eq!(stamp(&mut q, 9, FAIL), Ok(3));
    assert_eq!(q.status(thr()), Ok(Status::Resistant));
    assert_eq!(q.pcts(0), Ok(vec![20.0, 20.0, 20.0]));
}

/// Step 8's whole purpose: catching the run where the first prompt happened to
/// fail. A version that passes on any attempt is not resistant.
#[test]
fn a_later_pass_is_a_false_negative_caught() {
    let mut q = simple();
    stamp(&mut q, 0, FAIL).unwrap();
    stamp(&mut q, 5, FAIL).unwrap();
    stamp(&mut q, 9, PASS).unwrap();
    assert_eq!(q.status(thr()), Ok(Status::Inconsistent { at: 3 }));
    assert!(!q.status(thr()).unwrap().is_resistant());
}

// ------------------------------------------------------------ the refusals

#[test]
fn a_fourth_attempt_is_refused() {
    let mut q = simple();
    for m in 0..MAX_ATTEMPTS as u32 {
        stamp(&mut q, m * 5, FAIL).unwrap();
    }
    assert_eq!(stamp(&mut q, 30, FAIL), Err(ProtocolError::AttemptLimit));
    assert_eq!(q.base().attempts().len(), MAX_ATTEMPTS);
}

/// Once a version is decided, prompting it again is shopping for a result.
#[test]
fn a_decided_version_accepts_no_further_attempts() {
    let mut q = simple();
    stamp(&mut q, 0, PASS).unwrap();
    assert_eq!(
        stamp(&mut q, 5, FAIL),
        Err(ProtocolError::AlreadyDecided(Status::NotResistant { pct: 100.0 }))
    );
}

/// Grading an old version after moving on would produce a history saying the
/// instructor went back and tried again until something failed.
#[test]
fn an_attempt_on_an_older_version_is_refused() {
    let mut q = simple();
    stamp(&mut q, 0, PASS).unwrap();
    q.add_version(Strategy::Spatial, "A perturbed question about a grid.").unwrap();
    let scores = marks(&q, FAIL);
    assert_eq!(
        q.stamp(0, thr(), at(10), digest("late"), scores.clone()),
        Err(ProtocolError::NotLatestVersion)
    );
    assert_eq!(
        q.stamp(7, thr(), at(10), digest("late"), scores),
        Err(ProtocolError::NoSuchVersion(7))
    );
}

/// The attempts on a version are evidence about *those bytes*. Editing the
/// text afterwards would leave the evidence pointing at something that no
/// longer exists.
#[test]
fn a_version_that_has_been_prompted_cannot_be_edited() {
    let mut q = simple();
    let before = q.base().text_sha256();
    stamp(&mut q, 0, FAIL).unwrap();
    assert!(q.base().locked());
    assert_eq!(q.edit(0, "Something else entirely."), Err(ProtocolError::VersionLocked));
    assert_eq!(q.base().text_sha256(), before);

    // An unprompted version is still the instructor's to change.
    let v1 = q.add_version(Strategy::Axiomatic, "First draft.").unwrap();
    q.edit(v1, "Second draft.").unwrap();
    assert_eq!(q.version(v1).unwrap().text(), "Second draft.");
    assert_eq!(q.edit(v1, "   "), Err(ProtocolError::EmptyText));
}

#[test]
fn a_perturbation_leaves_the_earlier_attempts_intact() {
    let mut q = simple();
    stamp(&mut q, 0, PASS).unwrap();
    let v1 = q.add_version(Strategy::Contextual, "The same question, set in a kitchen.").unwrap();

    assert_eq!(v1, 1);
    assert_eq!(q.base().attempts().len(), 1);
    assert_eq!(q.version(1).unwrap().attempts().len(), 0);
    assert_eq!(q.version(1).unwrap().strategy(), Some(Strategy::Contextual));
    assert_eq!(q.base().strategy(), None);
    // The question is whatever it currently says; the history is still there.
    assert_eq!(q.status(thr()), Ok(Status::Untested));
    assert_eq!(q.status_of(0, thr()), Ok(Status::NotResistant { pct: 100.0 }));
}

#[test]
fn a_prompted_version_cannot_be_discarded_and_neither_can_the_base() {
    let mut q = simple();
    assert_eq!(q.discard_latest(), Err(ProtocolError::BaseVersionIsPermanent));

    q.add_version(Strategy::Spatial, "A draft nobody liked.").unwrap();
    q.discard_latest().unwrap();
    assert_eq!(q.versions().len(), 1);

    q.add_version(Strategy::Spatial, "A draft that got prompted.").unwrap();
    stamp(&mut q, 0, FAIL).unwrap();
    assert_eq!(q.discard_latest(), Err(ProtocolError::VersionLocked));
}

#[test]
fn a_question_needs_text() {
    assert_eq!(Question::new(4, "Empty", "   \n\n"), Err(ProtocolError::EmptyText));
    let mut q = simple();
    assert_eq!(q.add_version(Strategy::Spatial, ""), Err(ProtocolError::EmptyText));
}

// ------------------------------------------------------------ the rubric

#[test]
fn the_first_attempt_freezes_the_rubric() {
    let mut q = simple();
    q.rubric_mut().add_chip("Third chip", 5).unwrap();
    assert!(!q.frozen());

    stamp(&mut q, 0, &[0.0, 0.0, 0.0]).unwrap();

    assert!(q.frozen());
    let first = common::chip(&q, 0);
    assert_eq!(q.rubric_mut().add_chip("Too late", 1), Err(RubricError::Frozen));
    assert_eq!(q.rubric_mut().remove_chip(&first), Err(RubricError::Frozen));
    assert_eq!(q.rubric_mut().edit_chip(&first, "Renamed", 3), Err(RubricError::Frozen));
    assert_eq!(
        q.rubric_mut().set_scale(workbench_core::rubric::Scale::mastery()),
        Err(RubricError::Frozen)
    );
    // The initial revision takes the timestamp of the attempt that froze it.
    assert_eq!(q.rubric().revisions()[0].at.as_ref(), Some(&at(0)));
}

/// Step 6 is the one edit a frozen rubric accepts, because it is the one the
/// protocol asks for: the ledger says what the model invented, and the rubric
/// grows a chip that penalises it.
#[test]
fn a_frozen_rubric_still_accepts_a_penalty_chip() {
    let mut q = simple();
    stamp(&mut q, 0, FAIL).unwrap();
    let from = AttemptRef { version: 0, attempt: 1 };
    q.note(from, "Asserted P(7,6) = 7^6 without deriving it.").unwrap();

    let rev = q.add_penalty("Invented a standard identity", -4, from, at(2)).unwrap();
    assert_eq!(rev, 2);
    assert_eq!(q.rubric().revisions().len(), 2);
    assert_eq!(q.rubric().penalties_from(from), 1);
    assert_eq!(q.attempt(from).unwrap().notes(), ["Asserted P(7,6) = 7^6 without deriving it."]);
}

/// Provenance is the difference between a rubric tuned to the evidence and a
/// rubric tuned until the model fails.
#[test]
fn a_penalty_must_point_at_an_attempt_that_happened() {
    let mut q = simple();
    stamp(&mut q, 0, FAIL).unwrap();
    let nothing = AttemptRef { version: 0, attempt: 3 };
    assert_eq!(
        q.add_penalty("Made it up", -2, nothing, at(2)),
        Err(ProtocolError::NoSuchAttempt(nothing))
    );
    let no_version = AttemptRef { version: 4, attempt: 1 };
    assert_eq!(
        q.add_penalty("Made it up", -2, no_version, at(2)),
        Err(ProtocolError::NoSuchVersion(4))
    );
    assert_eq!(q.rubric().revisions().len(), 1);
}

/// Without this, a rubric that grew a penalty chip would silently re-grade
/// every attempt that came before it — and every one of them would look worse.
#[test]
fn a_penalty_chip_does_not_regrade_what_came_before_it() {
    let mut q = simple();
    stamp(&mut q, 0, FAIL).unwrap();
    let first = AttemptRef { version: 0, attempt: 1 };
    assert_eq!(q.pct(first), Ok(20.0));

    q.add_penalty("Invented an identity", -5, first, at(2)).unwrap();

    // Attempt 1 keeps the grade revision 1 gave it.
    assert_eq!(q.attempt(first).unwrap().rubric_revision(), 1);
    assert_eq!(q.pct(first), Ok(20.0));

    // Attempt 2 is graded by revision 2, penalty and all.
    stamp(&mut q, 5, &[100.0, 0.0, 100.0]).unwrap();
    let second = AttemptRef { version: 0, attempt: 2 };
    assert_eq!(q.attempt(second).unwrap().rubric_revision(), 2);
    assert_eq!(q.pct(second), Ok(-30.0)); // (2 - 5) of 10
    assert_eq!(q.pcts(0), Ok(vec![20.0, -30.0]));
}

/// §9 blocks on this at export; the protocol also refuses it at the source, so
/// an honest mistake is caught at the moment it is made.
#[test]
fn an_attempt_graded_before_its_rubric_existed_is_refused() {
    let mut q = simple();
    stamp(&mut q, 10, FAIL).unwrap();
    q.add_penalty("Invented an identity", -5, AttemptRef { version: 0, attempt: 1 }, at(20))
        .unwrap();

    let scores = marks(&q, &[0.0, 0.0, 0.0]);
    assert_eq!(
        q.stamp(0, thr(), at(15), digest("early"), scores),
        Err(ProtocolError::TimestampBeforeRubric)
    );
    assert_eq!(q.base().attempts().len(), 1);
}

#[test]
fn scores_must_cover_the_revision_that_is_grading() {
    let mut q = simple();
    let partial: Scores = [(common::chip(&q, 0), pct(100.0))].into_iter().collect();
    assert!(matches!(
        q.stamp(0, thr(), at(0), digest("r"), partial),
        Err(ProtocolError::Rubric(RubricError::ScoreMissing(_)))
    ));
    // A mark that is not a level of the scale is refused too.
    let mut off_scale = marks(&q, FAIL);
    off_scale.insert(common::chip(&q, 1), pct(33.0));
    assert!(matches!(
        q.stamp(0, thr(), at(0), digest("r"), off_scale),
        Err(ProtocolError::Rubric(RubricError::ScoreOffScale { .. }))
    ));
}

/// A refused stamp must leave the run exactly as it was — in particular it
/// must not freeze the rubric on its way to failing.
#[test]
fn a_refused_stamp_changes_nothing() {
    let mut q = simple();
    let before = q.clone();
    let partial: Scores = [(common::chip(&q, 0), pct(100.0))].into_iter().collect();
    assert!(q.stamp(0, thr(), at(0), digest("r"), partial).is_err());
    assert_eq!(q, before);
    assert!(!q.frozen());
    q.rubric_mut().add_chip("Still editable", 1).unwrap();
}

// ------------------------------------------------------------ derived, not stored

/// The central claim of this module. A hand-edited session file has no field
/// in which to lie about a status, a lock, a percentage, or a hash, because
/// none of them are fields.
#[test]
fn nothing_derivable_is_stored() {
    let mut q = simple();
    stamp(&mut q, 0, FAIL).unwrap();
    stamp(&mut q, 5, FAIL).unwrap();
    q.add_version(Strategy::Spatial, "A version with a grid in it.").unwrap();

    let json = serde_json::to_string_pretty(&q).unwrap();
    for forged in ["\"status\"", "\"locked\"", "\"pct\"", "\"textSha256\"", "\"resistant\""] {
        assert!(!json.contains(forged), "{} appears as a field:\n{}", forged, json);
    }
    // ...and the derivations still work over what *is* stored.
    let back: Question = serde_json::from_str(&json).unwrap();
    assert_eq!(back.status(thr()), Ok(Status::Untested));
    assert_eq!(back.status_of(0, thr()), Ok(Status::Testing { attempts: 2 }));
    assert_eq!(back.version(0).unwrap().text_sha256(), q.base().text_sha256());
    assert!(back.version(0).unwrap().locked());
}

#[test]
fn a_versions_text_is_canonical_and_its_hash_is_taken_from_it() {
    let q = Question::new(5, "Salad bar", "  Part 1. Count the slots.  \r\nPart 2. Explain.\n\n")
        .unwrap();
    assert_eq!(q.base().text(), "  Part 1. Count the slots.\nPart 2. Explain.");
    assert_eq!(q.base().text_sha256(), Sha256Hex::of(q.base().text().as_bytes()));
}

/// The readability readout the bench shows for an unsaved draft comes from
/// here, so the UI is not computing a number the manifest will later record.
#[test]
fn a_draft_is_measured_against_the_base_before_it_is_saved() {
    let q = simple();
    let clean = q.guard_draft(q.base().text(), &Limits::default());
    assert!(!clean.tripped());
    assert_eq!(clean.growth, 0.0);

    let long = format!("{} {}", q.base().text(), "And also explain your reasoning fully. ".repeat(20));
    assert!(q.guard_draft(&long, &Limits::default()).overgrown);

    assert_eq!(q.guard_of(0, &Limits::default()), Ok(clean));
    assert_eq!(q.guard_of(9, &Limits::default()), Err(ProtocolError::NoSuchVersion(9)));
}

#[test]
fn an_attempt_records_a_response_digest_it_cannot_do_without() {
    let mut q = simple();
    stamp(&mut q, 0, FAIL).unwrap();
    let a = q.attempt(AttemptRef { version: 0, attempt: 1 }).unwrap();
    assert_eq!(a.response(), &digest("response-0"));
    assert_eq!(a.at(), &at(0));
    // There is no field for the response text, so a run log cannot become a
    // transcript of somebody's chatbot session.
    let json = serde_json::to_string(&q).unwrap();
    assert!(!json.contains("\"response\":\"") || json.contains(&digest("response-0").to_string()));
    assert!(!json.contains("responseText"));
}

#[test]
fn the_strategies_are_the_papers_three() {
    assert_eq!(Strategy::all().len(), 3);
    assert_eq!(Strategy::Spatial.name(), "Spatial Injection");
    assert_eq!(Strategy::Axiomatic.name(), "Axiomatic Replacement");
    assert_eq!(Strategy::Contextual.name(), "Contextual Embedding");
    assert_eq!(serde_json::to_string(&Strategy::Spatial).unwrap(), "\"spatial\"");
}
