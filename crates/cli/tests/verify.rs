//! The audit, run the way a recipient would run it: as a command, on a file.

use std::process::Command;

use workbench_core::hash::{BuildId, Sha256Hex, Timestamp};
use workbench_core::manifest;
use workbench_core::protocol::{Question, Settings, Strategy};
use workbench_core::rubric::{Percent, Scores};
use workbench_core::session::{Access, Session};

fn at(minute: u32) -> Timestamp {
    Timestamp::parse(&format!("2026-08-27T09:{:02}:00Z", minute)).unwrap()
}

fn marks(s: &Session, vals: &[f64]) -> Scores {
    s.question(0)
        .unwrap()
        .rubric()
        .current()
        .chips
        .iter()
        .zip(vals)
        .map(|(c, v)| (c.id.clone(), Percent::new(*v).unwrap()))
        .collect()
}

/// One question taken to Step 8b, exactly as the protocol prescribes.
fn resistant_run() -> Session {
    let mut s = Session::new(Settings::default());
    s.set_input(Sha256Hex::of(b"lab3-part2.pdf"), 22);
    s.set_target("gemini-2.5-flash", Access::Institutional, true, at(0)).unwrap();

    let mut q = Question::new(5, "A question", "Count the arrangements of ten items.").unwrap();
    q.rubric_mut().add_chip("States the count", 4).unwrap();
    q.rubric_mut().add_chip("Justifies the count", 6).unwrap();
    s.add_question(q);

    s.stamp(0, 0, at(1), Sha256Hex::of(b"r1"), marks(&s, &[100.0, 100.0])).unwrap();
    s.question_mut(0)
        .unwrap()
        .add_version(Strategy::Spatial, "Count the arrangements shown in the grid above.")
        .unwrap();
    for (i, m) in [10u32, 15, 20].iter().enumerate() {
        let scores = marks(&s, &[100.0, 0.0]);
        s.stamp(0, 1, at(*m), Sha256Hex::of(format!("r{}", i).as_bytes()), scores).unwrap();
    }
    s
}

fn write(name: &str, body: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("pw-test-{}-{}", std::process::id(), name));
    std::fs::write(&p, body).unwrap();
    p
}

fn pw(path: &std::path::Path) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_pw")).arg("verify").arg(path).output().unwrap();
    let mut text = String::from_utf8_lossy(&out.stdout).to_string();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    (out.status.success(), text)
}

#[test]
fn a_clean_session_passes_from_the_command_line() {
    let f = write("clean-session.json", &serde_json::to_string(&resistant_run()).unwrap());
    let (ok, out) = pw(&f);
    assert!(ok, "{}", out);
    assert!(out.contains("(session)"), "{}", out);
    assert!(out.contains("1 question(s), 2 version(s), 4 attempt(s), 1 resistant"), "{}", out);
    assert!(out.contains("ok: nothing blocking"), "{}", out);
    std::fs::remove_file(f).ok();
}

/// The check a recipient can actually run: they have the manifest and nothing
/// else.
#[test]
fn a_manifest_is_audited_against_itself() {
    let m = manifest::build(
        BuildId::parse("c8878585").unwrap(),
        Timestamp::parse("2026-08-27T14:02:11Z").unwrap(),
        &resistant_run(),
        None,
    )
    .unwrap();
    let f = write("manifest.json", &serde_json::to_string_pretty(&m).unwrap());
    let (ok, out) = pw(&f);
    assert!(ok, "{}", out);
    assert!(out.contains("(manifest)"), "{}", out);
    assert!(out.contains("1 resistant"), "{}", out);

    // Re-label the question resistant when its attempts say otherwise.
    let mut v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
    v["questions"][0]["versions"][1]["attempts"][0]["pct"] = serde_json::json!(95.0);
    let g = write("manifest-doctored.json", &serde_json::to_string(&v).unwrap());
    let (ok, out) = pw(&g);
    assert!(!ok, "a doctored manifest passed:\n{}", out);
    assert!(out.contains("the recorded status is not the one its attempts produce"), "{}", out);
    assert!(out.contains("met the threshold on a version claimed resistant"), "{}", out);
    assert!(out.contains("FAILED"), "{}", out);

    std::fs::remove_file(f).ok();
    std::fs::remove_file(g).ok();
}

#[test]
fn a_broken_session_fails_and_says_which_attempt() {
    let mut v = serde_json::to_value(resistant_run()).unwrap();
    v["questions"][0]["versions"][1]["text"] = serde_json::json!("Count something else.");
    let f = write("broken-session.json", &serde_json::to_string(&v).unwrap());

    let (ok, out) = pw(&f);
    assert!(!ok, "{}", out);
    assert!(out.contains("blocking"), "{}", out);
    assert!(out.contains("not the text its attempts recorded prompting"), "{}", out);
    std::fs::remove_file(f).ok();
}

/// A file that will not parse is not a file that passed.
#[test]
fn an_unreadable_file_fails_rather_than_being_skipped() {
    let f = write("junk.json", "{\"schema\":\"something-else/1\"}");
    let (ok, out) = pw(&f);
    assert!(!ok);
    assert!(out.contains("not a readable session"), "{}", out);
    std::fs::remove_file(f).ok();

    let missing = std::env::temp_dir().join("pw-test-nothing-here.json");
    let (ok, out) = pw(&missing);
    assert!(!ok);
    assert!(out.contains("cannot read"), "{}", out);
}

#[test]
fn the_advisories_are_reported_without_failing() {
    let mut s = resistant_run();
    s.set_target("gemini-2.5-pro", Access::Institutional, true, at(40)).unwrap();
    let f = write("advisory-session.json", &serde_json::to_string(&s).unwrap());

    let (ok, out) = pw(&f);
    assert!(ok, "an advisory must not fail the audit:\n{}", out);
    assert!(out.contains("reported - the instructor's calls"), "{}", out);
    assert!(out.contains("named 2 different targets"), "{}", out);
    std::fs::remove_file(f).ok();
}

/// The manifest from a real run of the study's own lab, committed as a
/// regression fixture.
///
/// It is the one artefact of a real run that is safe to publish — the manifest
/// carries no question text by construction — and it is worth more than any
/// synthetic case: it is the shape a manifest actually takes when a human runs
/// the protocol for an afternoon, with the mistakes and the changed minds in
/// it. If a later change to `verify` or `manifest` starts rejecting it,
/// something that used to be provable no longer is.
#[test]
fn the_recorded_run_still_verifies() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/example_run.json");
    let Ok(raw) = std::fs::read_to_string(path) else {
        eprintln!("skipped: no recorded run at {} yet", path);
        return;
    };
    let m: workbench_core::manifest::Manifest =
        serde_json::from_str(&raw).expect("the recorded run should still parse");
    let report = workbench_core::manifest::audit(&m);
    assert!(report.passed(), "the recorded run no longer verifies: {:?}", report.blocking());
    assert!(report.questions > 0);
    assert!(m.target.is_some(), "a run with no named target is not evidence");

    // And it is still safe to have committed.
    assert!(!m.contains_question_text);
}
