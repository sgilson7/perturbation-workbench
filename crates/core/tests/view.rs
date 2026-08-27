//! What the page renders, decided here so the page cannot decide it.

mod common;

use common::{at, digest, marks, question, resistant_run, run, simple, stamp};
use workbench_core::protocol::{Status, Strategy, Tone};
use workbench_core::rubric::AttemptRef;
use workbench_core::session::Session;
use workbench_core::view::view;

#[test]
fn a_fresh_run_offers_the_three_strategies_and_nothing_to_export() {
    let v = view(&Session::default());
    assert!(v.questions.is_empty());
    assert!(!v.started);
    assert!(!v.can_export, "there is nothing to export yet");
    assert_eq!(v.strategies.len(), 3);
    assert_eq!(v.strategies[0].name, "Spatial Injection");
    assert!(!v.strategies[0].description.is_empty());
    assert!(v.target.is_none());
}

#[test]
fn a_question_carries_its_own_label_banner_and_tone() {
    let mut s = run();
    s.add_question(simple());

    let q = &view(&s).questions[0];
    assert_eq!(q.status, Status::Untested);
    assert_eq!(q.label, "UNTESTED");
    assert_eq!(q.tone, Tone::Neutral);
    assert!(q.banner.starts_with("Step 3"));

    stamp(&mut s, 0, 10, &[100.0, 100.0]);
    let q = &view(&s).questions[0];
    assert_eq!(q.label, "NOT RESISTANT");
    assert_eq!(q.tone, Tone::Bad);
    assert!(q.banner.starts_with("Step 4"));

    let v = view(&resistant_run());
    assert_eq!(v.questions[0].label, "RESISTANT");
    assert_eq!(v.questions[0].tone, Tone::Good);
    assert_eq!(v.resistant, 1);
}

/// The grading panel is live or it is not, and that is a protocol question.
/// A front end that worked it out for itself would eventually work it out
/// differently.
#[test]
fn whether_a_version_can_take_an_attempt_is_decided_here() {
    let mut s = run();
    s.add_question(simple());

    assert!(view(&s).questions[0].versions[0].can_stamp);

    // An older version cannot, once a perturbation exists.
    stamp(&mut s, 0, 10, &[100.0, 100.0]);
    s.question_mut(0).unwrap().add_version(Strategy::Spatial, "Now with a grid.").unwrap();
    let q = &view(&s).questions[0];
    assert!(!q.versions[0].can_stamp, "the base is history now");
    assert!(q.versions[1].can_stamp);

    // Nor can a decided one, nor one at the cap.
    for m in [20u32, 25, 30] {
        stamp(&mut s, 0, m, &[100.0, 0.0]);
    }
    let q = &view(&s).questions[0];
    assert!(!q.versions[1].can_stamp);
    assert_eq!(q.versions[1].attempts_left, 0);
    assert_eq!(q.versions[1].status, Status::Resistant);
}

/// A rubric with no chips cannot grade anything, so the panel stays shut
/// rather than offering a form that will be refused.
#[test]
fn a_question_with_no_rubric_yet_cannot_be_stamped() {
    let mut s = run();
    s.add_question(
        workbench_core::protocol::Question::new(1, "Blank", "Describe a linked list.").unwrap(),
    );
    let q = &view(&s).questions[0];
    assert!(q.rubric.empty);
    assert!(!q.versions[0].can_stamp);
    assert_eq!(q.rubric.total_points, 0);

    s.question_mut(0).unwrap().rubric_mut().add_chip("Names the nodes", 5).unwrap();
    assert!(view(&s).questions[0].versions[0].can_stamp);
}

/// Reaching for something you have not tried is the whole idea of Step 5.
#[test]
fn the_suggested_strategy_is_one_that_has_not_been_tried() {
    let mut s = run();
    s.add_question(simple());
    assert_eq!(view(&s).questions[0].suggested, Some(Strategy::Spatial));

    s.question_mut(0).unwrap().add_version(Strategy::Spatial, "With a grid.").unwrap();
    assert_eq!(view(&s).questions[0].suggested, Some(Strategy::Axiomatic));

    s.question_mut(0).unwrap().add_version(Strategy::Axiomatic, "With new axioms.").unwrap();
    assert_eq!(view(&s).questions[0].suggested, Some(Strategy::Contextual));

    // All three tried: the tool has nothing useful left to suggest, and says
    // so rather than repeating itself.
    s.question_mut(0).unwrap().add_version(Strategy::Contextual, "In a kitchen.").unwrap();
    assert_eq!(view(&s).questions[0].suggested, None);
}

#[test]
fn a_version_is_labelled_by_its_number_and_its_strategy() {
    let mut s = run();
    s.add_question(simple());
    s.question_mut(0).unwrap().add_version(Strategy::Axiomatic, "New axioms.").unwrap();
    let q = &view(&s).questions[0];
    assert_eq!(q.versions[0].label, "v0 · base");
    assert_eq!(q.versions[1].label, "v1 · Axiomatic Replacement");
    // The digest prefix the exported query file is named by.
    assert_eq!(q.versions[0].short.len(), 8);
    assert!(q.versions[0].text_sha256.as_str().starts_with(&q.versions[0].short));
}

#[test]
fn the_rubric_panel_goes_read_only_when_the_rubric_freezes() {
    let mut s = run();
    s.add_question(simple());
    assert!(!view(&s).questions[0].rubric.frozen);
    assert!(view(&s).questions[0].versions[0].editable);

    stamp(&mut s, 0, 10, &[100.0, 0.0]);
    let q = &view(&s).questions[0];
    assert!(q.rubric.frozen);
    assert!(!q.versions[0].editable, "a prompted version needs a new one, not an edit");
    assert_eq!(q.rubric.revisions, 1);
    assert_eq!(q.rubric.chips.len(), 2);
    assert!(q.rubric.chips.iter().all(|c| !c.penalty));
}

/// Step 6 is where an observation becomes something the rubric penalises. The
/// side panel needs to know how many have not got there yet.
#[test]
fn ledger_notes_awaiting_promotion_are_counted() {
    let mut s = resistant_run();
    let from = AttemptRef { version: 1, attempt: 1 };
    s.question_mut(0).unwrap().note(from, "Asserted 2 + 2 = 5.").unwrap();
    s.question_mut(0).unwrap().note(from, "Cited a law that does not exist.").unwrap();
    assert_eq!(view(&s).questions[0].unpromoted_notes, 2);

    s.question_mut(0).unwrap().add_penalty("Invented a law", -4, from, at(30)).unwrap();
    let q = &view(&s).questions[0];
    assert_eq!(q.unpromoted_notes, 0);
    assert_eq!(q.versions[1].attempts[0].penalties_derived, 1);
    assert_eq!(q.versions[1].attempts[0].notes.len(), 2);
    // The penalty chip says which attempt it came from, so the panel can
    // answer "why is this here".
    let penalty = q.rubric.chips.iter().find(|c| c.penalty).unwrap();
    assert_eq!(penalty.from, Some(from));
    assert_eq!(penalty.points, -4);
}

/// The export button says why it is disabled before it is pressed, rather than
/// after.
#[test]
fn the_view_carries_what_blocks_an_export() {
    let s = resistant_run();
    let v = view(&s);
    assert!(v.can_export);
    assert_eq!(v.blocking, 0);

    let mut doctored = serde_json::to_value(&s).unwrap();
    doctored["questions"][0]["versions"][1]["text"] = serde_json::json!("Something else.");
    let forged: Session = serde_json::from_value(doctored).unwrap();

    let v = view(&forged);
    assert!(!v.can_export);
    assert_eq!(v.blocking, 1);
    assert_eq!(v.blocking_findings.len(), 1);
}

/// Every number the stamp rail shows is computed here. The front end formats
/// them and does no arithmetic.
#[test]
fn the_attempt_rail_arrives_already_decided() {
    let mut s = run();
    s.add_question(question(5));
    let full: Vec<f64> = vec![100.0; 8];
    let half: Vec<f64> = vec![50.0; 8];
    let none: Vec<f64> = vec![0.0; 8];
    stamp(&mut s, 0, 10, &none);
    stamp(&mut s, 0, 15, &half);
    stamp(&mut s, 0, 20, &full);

    let a = &view(&s).questions[0].versions[0].attempts;
    assert_eq!(a.iter().map(|x| x.pct).collect::<Vec<_>>(), [0.0, 50.0, 100.0]);
    assert_eq!(a.iter().map(|x| x.met_threshold).collect::<Vec<_>>(), [false, false, true]);
    assert_eq!(a[0].ordinal, 1);
    assert_eq!(a[2].rubric_revision, 1);
    // The bytes that were sent, next to the bytes on file.
    assert!(a.iter().all(|x| x.query_sha256 == view(&s).questions[0].versions[0].text_sha256));
    let _ = marks(&s, 0, &none);
    let _ = digest("unused");
}

#[test]
fn a_code_bearing_question_says_so() {
    let mut s = run();
    s.add_question(
        workbench_core::protocol::Question::new(
            1,
            "CS1",
            "Complete the method.\n```java\nint sum(int[] a) { return 0; }\n```",
        )
        .unwrap(),
    );
    let v = &view(&s).questions[0].versions[0];
    assert_eq!(v.code_blocks, 1);
    // ...and the code did not drag the reading level into the twenties.
    assert!(v.metrics.grade < 12.0, "grade was {}", v.metrics.grade);
    assert!(!v.guard.tripped());
}
