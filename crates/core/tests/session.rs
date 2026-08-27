//! The run as it is paused, moved between machines, and picked back up.

mod common;

use common::{at, digest, pct, simple};
use workbench_core::protocol::{Settings, Status, Strategy};
use workbench_core::rubric::Scores;
use workbench_core::session::{Session, SessionError, SESSION_SCHEMA};

fn marks(s: &Session, q: usize, vals: &[f64]) -> Scores {
    s.question(q)
        .unwrap()
        .rubric()
        .current()
        .chips
        .iter()
        .zip(vals)
        .map(|(c, v)| (c.id.clone(), pct(*v)))
        .collect()
}

fn started_run() -> Session {
    let mut s = Session::new(Settings::default());
    s.set_input(digest("the lab pdf"), 22);
    s.set_target("gemini-2.5-flash", "NCSU Google Workspace licence", true, at(0)).unwrap();
    s.add_question(simple());
    s
}

#[test]
fn a_run_survives_being_paused_and_picked_up_elsewhere() {
    let mut s = started_run();
    for m in [1u32, 6, 11] {
        let scores = marks(&s, 0, &[100.0, 0.0]);
        s.stamp(0, 0, at(m), digest(&format!("r{}", m)), scores).unwrap();
    }
    s.question_mut(0).unwrap().add_version(Strategy::Spatial, "Now with a grid.").unwrap();

    let json = serde_json::to_string_pretty(&s).unwrap();
    let back: Session = serde_json::from_str(&json).unwrap();

    assert_eq!(back, s);
    assert_eq!(back.status(0), Ok(Status::Untested));
    assert_eq!(back.question(0).unwrap().status_of(0, pct(60.0)), Ok(Status::Resistant));
    assert_eq!(back.input().unwrap().pages, 22);
    assert_eq!(back.target().unwrap().model, "gemini-2.5-flash");
}

/// The file says what it is, so nobody has to guess whether it is the one that
/// is safe to email.
#[test]
fn a_session_says_that_it_is_not_the_manifest() {
    let json = serde_json::to_string(&started_run()).unwrap();
    assert!(json.contains(SESSION_SCHEMA));
    assert!(json.contains("NOT the run manifest"));
}

#[test]
fn a_session_from_another_schema_is_refused() {
    let mut json = serde_json::to_string(&started_run()).unwrap();
    json = json.replace(SESSION_SCHEMA, "perturbation-workbench-session/2");
    let err = serde_json::from_str::<Session>(&json).unwrap_err().to_string();
    assert!(err.contains("session schema"), "{}", err);
}

/// A hand-edited session cannot grow a field. `status`, `locked` and `pct` are
/// derived, so adding one is not "overriding" anything — it is a key nothing
/// reads, and refusing the file outright is the honest response.
#[test]
fn a_forged_session_is_refused_rather_than_partly_believed() {
    let json = serde_json::to_string(&started_run()).unwrap();
    for forgery in [
        (r#""versions":["#, r#""status":"resistant","versions":["#),
        (r#""versions":["#, r#""locked":true,"versions":["#),
        (r#""title":"Simple""#, r#""title":"Simple","pct":100"#),
    ] {
        let edited = json.replacen(forgery.0, forgery.1, 1);
        assert_ne!(edited, json, "the forgery did not apply: {:?}", forgery.0);
        assert!(
            serde_json::from_str::<Session>(&edited).is_err(),
            "a session with {:?} was accepted",
            forgery.1
        );
    }
}

/// "Resistant" is a claim about a specific model on a specific day. A run that
/// never wrote down which model cannot support one, so it cannot start.
#[test]
fn nothing_can_be_stamped_before_the_target_is_named() {
    let mut s = Session::new(Settings::default());
    s.add_question(simple());
    let scores = marks(&s, 0, &[100.0, 0.0]);
    assert_eq!(s.stamp(0, 0, at(1), digest("r"), scores), Err(SessionError::NoTarget));
    assert!(!s.started());

    assert_eq!(s.set_target("  ", "", true, at(0)), Err(SessionError::EmptyModel));
    s.set_target("gemini-2.5-flash", "", true, at(0)).unwrap();
    let scores = marks(&s, 0, &[100.0, 0.0]);
    assert_eq!(s.stamp(0, 0, at(1), digest("r"), scores), Ok(1));
    assert!(s.started());
}

/// Switching models halfway through is allowed and is reported. What it must
/// not do is leave a tidy file that quietly means two different things.
#[test]
fn changing_the_target_leaves_a_trace() {
    let mut s = started_run();
    assert_eq!(s.targets().len(), 1);

    // Re-stating the same target is not a change; recording it as one would
    // produce an advisory about something that did not happen.
    s.set_target("gemini-2.5-flash", "NCSU Google Workspace licence", true, at(5)).unwrap();
    assert_eq!(s.targets().len(), 1);

    s.set_target("gemini-2.5-pro", "NCSU Google Workspace licence", true, at(30)).unwrap();
    assert_eq!(s.targets().len(), 2);
    assert_eq!(s.target().unwrap().model, "gemini-2.5-pro");
    assert_eq!(s.targets()[0].model, "gemini-2.5-flash");
}

#[test]
fn the_run_counts_only_the_questions_that_earned_it() {
    let mut s = started_run();
    s.add_question(simple());
    assert_eq!(s.resistant(), 0);

    for m in [1u32, 6, 11] {
        let scores = marks(&s, 0, &[100.0, 0.0]);
        s.stamp(0, 0, at(m), digest(&format!("a{}", m)), scores).unwrap();
    }
    // Two failures and a pass on the second question: not resistant.
    for (m, vals) in [(2u32, &[100.0, 0.0]), (7, &[100.0, 0.0]), (12, &[100.0, 100.0])] {
        let scores = marks(&s, 1, vals);
        s.stamp(1, 0, at(m), digest(&format!("b{}", m)), scores).unwrap();
    }

    assert_eq!(s.status(0), Ok(Status::Resistant));
    assert_eq!(s.status(1), Ok(Status::Inconsistent { at: 3 }));
    assert_eq!(s.resistant(), 1);
    assert_eq!(s.status(9), Err(SessionError::NoSuchQuestion(9)));
}

/// The threshold is a dial, and moving it re-decides every question. That is
/// correct — "resistant" means nothing without the line it was judged against —
/// and it is exactly why the manifest records the value in force at export
/// rather than leaving a reader to assume the default.
#[test]
fn the_threshold_travels_with_the_run_and_re_decides_it() {
    let mut s = started_run();
    s.set_settings(Settings { threshold: pct(75.0), ..Settings::default() });
    for m in [1u32, 6, 11] {
        let scores = marks(&s, 0, &[100.0, 50.0]); // 60% each time
        s.stamp(0, 0, at(m), digest(&format!("r{}", m)), scores).unwrap();
    }
    assert_eq!(s.status(0), Ok(Status::Resistant));

    // Judged against the paper's line instead, the very first attempt passed.
    s.set_settings(Settings::default());
    assert_eq!(s.status(0), Ok(Status::NotResistant { pct: 60.0 }));
    assert_eq!(s.settings().threshold, pct(60.0));
    assert_eq!(s.settings().fk_cap, 14.0);
}

#[test]
fn a_fresh_run_carries_the_plans_defaults() {
    let s = Session::default();
    assert_eq!(s.settings().threshold, pct(60.0));
    assert_eq!(s.settings().fk_drift, 1.5);
    assert_eq!(s.settings().fk_cap, 14.0);
    assert_eq!(s.settings().growth_cap, 35.0);
    assert!(s.input().is_none());
    assert!(s.target().is_none());
    assert_eq!(s.questions().len(), 0);
}
