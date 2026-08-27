//! The audit, and the forgeries it is there to catch.
//!
//! Most of these sessions cannot be produced through `protocol`'s API — a
//! prompted version is locked, a fourth attempt is refused. They are written
//! here as JSON and read back, because a session is a file on somebody's disk
//! and the API is not the only way to write one.

mod common;

use common::{at, digest, resistant_run, run, simple, stamp};
use workbench_core::protocol::{Status, Strategy};
use workbench_core::rubric::AttemptRef;
use workbench_core::session::{Access, Session};
use serde_json::Value;
use workbench_core::verify::{verify, Finding};

/// Round-trip a run through JSON with one edit applied along the way, which is
/// what someone with a text editor and a session file can do.
fn tamper(s: &Session, edit: impl FnOnce(&mut Value)) -> Session {
    let mut v = serde_json::to_value(s).unwrap();
    edit(&mut v);
    serde_json::from_value(v).expect("the forged session should still parse")
}

/// The attempts array of one version.
fn attempts_of<'a>(v: &'a mut Value, q: usize, ver: usize) -> &'a mut Vec<Value> {
    v["questions"][q]["versions"][ver]["attempts"].as_array_mut().expect("an attempts array")
}

fn has(s: &Session, f: &Finding) -> bool {
    verify(s).findings.contains(f)
}

#[test]
fn a_clean_run_verifies_and_counts_itself() {
    let s = resistant_run();
    let r = verify(&s);
    assert!(r.passed(), "{:?}", r.blocking());
    assert!(r.blocking().is_empty());
    assert_eq!(r.questions, 1);
    assert_eq!(r.versions, 2);
    assert_eq!(r.attempts, 4);
    assert_eq!(r.resistant, 1);
}

// ------------------------------------------------------------------ blocking

/// The first rule of §9. Whatever was tested, it was not the text now on file.
#[test]
fn text_edited_after_prompting_blocks_the_export() {
    let s = tamper(&resistant_run(), |v| {
        v["questions"][0]["versions"][1]["text"] =
            Value::from("Prove that two plus two is four, using only the graph drawn above.")
    });
    let r = verify(&s);
    assert!(!r.passed());
    assert!(r.findings.contains(&Finding::TextChangedAfterPrompting { question: 0, version: 1 }));
}

/// "The same bytes, three times" is the claim. If the digests differ between
/// attempts, it is not the claim that was tested.
#[test]
fn attempts_on_different_bytes_block_the_export() {
    let s = tamper(&resistant_run(), |v| {
        attempts_of(v, 0, 1)[0]["query"] = Value::from(digest("something else").to_string())
    });
    let r = verify(&s);
    assert!(!r.passed());
    assert!(r.findings.iter().any(|f| matches!(f, Finding::QueryChangedBetweenAttempts { .. })));
    assert!(r.findings.contains(&Finding::TextChangedAfterPrompting { question: 0, version: 1 }));
}

#[test]
fn a_fourth_attempt_in_a_file_blocks_the_export() {
    let s = tamper(&resistant_run(), |v| {
        let a = attempts_of(v, 0, 1);
        a.push(a[2].clone());
    });
    let r = verify(&s);
    assert!(!r.passed());
    assert!(r.findings.contains(&Finding::TooManyAttempts {
        question: 0,
        version: 1,
        attempts: 4
    }));
}

#[test]
fn a_grade_that_cannot_be_rederived_blocks_the_export() {
    // A revision that is not in the file at all.
    let s = tamper(&resistant_run(), |v| {
        attempts_of(v, 0, 1)[0]["rubricRevision"] = Value::from(9)
    });
    assert!(verify(&s).findings.iter().any(|f| matches!(f, Finding::RubricRevisionMissing { .. })));

    // Marks that do not fit the revision that is supposed to have produced them.
    let s = tamper(&resistant_run(), |v| {
        attempts_of(v, 0, 1)[0]["scores"] = serde_json::json!({ "proves-the-claim": 50.0 })
    });
    let r = verify(&s);
    assert!(!r.passed());
    assert!(r.findings.contains(&Finding::GradeCannotBeRederived {
        question: 0,
        version: 1,
        attempt: 1
    }));
}

#[test]
fn an_attempt_graded_before_its_rubric_blocks_the_export() {
    let mut s = run();
    s.add_question(simple());
    stamp(&mut s, 0, 10, &[100.0, 0.0]);
    // Move the attempt earlier than the revision it is graded against.
    let s = tamper(&s, |v| {
        v["questions"][0]["rubric"]["revisions"][0]["at"] = Value::from("2026-08-27T09:20:00Z")
    });
    let r = verify(&s);
    assert!(!r.passed());
    assert!(r.findings.contains(&Finding::RubricRevisionDidNotExist {
        question: 0,
        version: 0,
        attempt: 1
    }));
}

/// Provenance is what separates a rubric tuned to the evidence from a rubric
/// tuned until the model fails.
#[test]
fn a_penalty_without_provenance_blocks_the_export() {
    let mut s = run();
    s.add_question(simple());
    stamp(&mut s, 0, 10, &[100.0, 0.0]);
    let from = AttemptRef { version: 0, attempt: 1 };
    s.question_mut(0).unwrap().note(from, "Asserted P(7,6) = 7^6.").unwrap();
    s.question_mut(0).unwrap().add_penalty("Invented an identity", -4, from, at(12)).unwrap();
    assert!(verify(&s).passed());

    let s = tamper(&s, |v| {
        let chips = v["questions"][0]["rubric"]["revisions"][1]["chips"].as_array_mut().unwrap();
        let last = chips.len() - 1;
        chips[last]["kind"]["penalty"]["from"]["attempt"] = Value::from(3);
    });
    assert!(has(&s, &Finding::PenaltyWithoutProvenance { question: 0 }));
}

/// "Resistant" is a claim about a specific model on a specific day, so a run
/// with attempts and no target is not evidence of anything.
#[test]
fn attempts_with_no_named_target_block_the_export() {
    let s = tamper(&resistant_run(), |v| v["targets"] = Value::Array(vec![]));
    assert!(s.target().is_none());
    let r = verify(&s);
    assert!(!r.passed());
    assert!(r.findings.contains(&Finding::NoTargetRecorded));

    // A run with no attempts and no target is merely unstarted, not broken.
    let mut fresh = Session::default();
    fresh.add_question(simple());
    assert!(!verify(&fresh).findings.contains(&Finding::NoTargetRecorded));
}

/// The type does the work here: there is no way to write an attempt without a
/// response digest, so §9's last blocking case is unreachable rather than
/// checked. A file that tries is refused before `verify` ever sees it.
#[test]
fn an_attempt_without_a_response_digest_cannot_be_read_at_all() {
    for forgery in [Value::from(""), Value::from("The answer is 256 sundaes."), Value::Null] {
        let mut v = serde_json::to_value(resistant_run()).unwrap();
        v["questions"][0]["versions"][1]["attempts"][0]["response"] = forgery.clone();
        assert!(
            serde_json::from_value::<Session>(v).is_err(),
            "a response of {:?} was accepted",
            forgery
        );
    }
    // Removing the field outright is refused too, not defaulted.
    let mut v = serde_json::to_value(resistant_run()).unwrap();
    v["questions"][0]["versions"][1]["attempts"][0]
        .as_object_mut()
        .unwrap()
        .remove("response");
    assert!(serde_json::from_value::<Session>(v).is_err());
}

// ------------------------------------------------------------------ advisory

/// The instructor knows whether a longer question is a worse question. What
/// the tool must not do is let the drift pass unrecorded.
#[test]
fn a_tripped_guard_is_reported_and_does_not_stop_the_export() {
    let mut s = run();
    s.add_question(simple());
    s.question_mut(0)
        .unwrap()
        .add_version(
            Strategy::Contextual,
            &format!("Prove that two plus two is four. {}", "Justify every step fully. ".repeat(40)),
        )
        .unwrap();
    for m in [10u32, 15, 20] {
        stamp(&mut s, 0, m, &[100.0, 0.0]);
    }

    let r = verify(&s);
    assert!(r.passed(), "an advisory must not block: {:?}", r.blocking());
    assert!(r.findings.contains(&Finding::GuardTripped { question: 0, version: 1 }));
    assert_eq!(r.advisories().len(), 1);
}

#[test]
fn a_question_still_in_progress_is_reported_with_the_state_it_is_in() {
    let mut s = run();
    s.add_question(simple());
    assert!(has(&s, &Finding::InProgress { question: 0, state: Status::Untested }));

    stamp(&mut s, 0, 10, &[100.0, 0.0]);
    assert!(has(&s, &Finding::InProgress { question: 0, state: Status::Testing { attempts: 1 } }));
    assert!(verify(&s).passed());

    stamp(&mut s, 0, 15, &[100.0, 100.0]);
    assert!(has(&s, &Finding::InProgress { question: 0, state: Status::Inconsistent { at: 2 } }));
}

#[test]
fn a_changed_target_is_reported() {
    let mut s = resistant_run();
    assert!(!verify(&s).findings.iter().any(|f| matches!(f, Finding::TargetChanged { .. })));

    s.set_target("gemini-2.5-pro", Access::Institutional, true, at(40)).unwrap();
    let r = verify(&s);
    assert!(r.passed());
    assert!(r.findings.contains(&Finding::TargetChanged { targets: 2 }));
}

/// Step 6 is where an observed hallucination becomes something the rubric
/// penalises. A note that never got there is an observation that did not.
#[test]
fn ledger_notes_that_never_became_penalties_are_reported() {
    let mut s = resistant_run();
    let from = AttemptRef { version: 1, attempt: 1 };
    s.question_mut(0).unwrap().note(from, "Claimed 2 + 2 = 5 and did not derive it.").unwrap();

    let r = verify(&s);
    assert!(r.passed());
    assert!(r.findings.contains(&Finding::NotesNotPromoted {
        question: 0,
        version: 1,
        attempt: 1,
        notes: 1
    }));

    // Promote it and the advisory goes away.
    s.question_mut(0).unwrap().add_penalty("Asserted 2 + 2 = 5", -4, from, at(25)).unwrap();
    assert!(!verify(&s).findings.iter().any(|f| matches!(f, Finding::NotesNotPromoted { .. })));
}

/// A finding is pasted into a manifest, so it must name ordinals and enum
/// values and nothing else.
#[test]
fn a_finding_names_nothing_it_could_leak() {
    let mut s = resistant_run();
    s.question_mut(0)
        .unwrap()
        .note(AttemptRef { version: 1, attempt: 1 }, "Invented P(7,6) = 7^6 for the salad bar.")
        .unwrap();
    let json = serde_json::to_string(&verify(&s)).unwrap();
    for leak in ["Invented", "salad", "grid", "Prove", "two plus two", "Simple", "claim"] {
        assert!(!json.contains(leak), "a finding leaked {:?}: {}", leak, json);
    }
}
