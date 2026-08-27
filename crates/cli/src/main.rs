//! Check a run without a browser.
//!
//! The workbench's claim is that a collaborator at another institution can
//! verify what was done. A claim that can only be checked by opening the tool
//! that made it is not much of one, so the same audit `verify.rs` runs before
//! an export is reachable here from a shell script, a CI job, or somebody
//! else's laptop with no idea what a wasm module is.
//!
//!     pw verify run-manifest.json    # the check a recipient can run
//!     pw verify session.json         # the full audit, over the whole run
//!
//! Two files, two different checks. A session holds the run, so it is audited
//! against §9 in full. A manifest holds only what the file says, so it is
//! checked against itself: do the recorded attempts produce the recorded
//! status, and are the query digests on a resistant version all the same
//! digest as its text. The second is narrower on purpose — it is what a
//! recipient can actually do, and it is what has to be enough.

use std::process::ExitCode;

use workbench_core::manifest::{self, Manifest};
use workbench_core::session::Session;
use workbench_core::verify::{self, Finding, Report};

const USAGE: &str = "\
pw - check a perturbation run

usage:
  pw verify <file>   audit a session.json or a run-manifest.json
  pw help            this text

exit status is 0 when nothing blocking was found, 1 otherwise.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        [] | ["help"] | ["-h"] | ["--help"] => {
            println!("{}", USAGE);
            ExitCode::SUCCESS
        }
        ["verify", path] => run(path),
        other => {
            eprintln!("pw: unknown command {:?}\n\n{}", other.join(" "), USAGE);
            ExitCode::FAILURE
        }
    }
}

fn run(path: &str) -> ExitCode {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("pw: cannot read {}: {}", path, e);
            return ExitCode::FAILURE;
        }
    };

    // Which file this is, decided by what it says it is rather than by its
    // name: both are JSON and people rename things.
    let (kind, report) = if text.contains(manifest::SCHEMA) {
        match serde_json::from_str::<Manifest>(&text) {
            Ok(m) => ("manifest", manifest::audit(&m)),
            Err(e) => return refuse(path, "manifest", &e),
        }
    } else {
        match serde_json::from_str::<Session>(&text) {
            Ok(s) => ("session", verify::verify(&s)),
            Err(e) => return refuse(path, "session", &e),
        }
    };

    report_on(path, kind, &report);
    if report.passed() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// A file that will not parse is not a file that passed.
fn refuse(path: &str, kind: &str, e: &serde_json::Error) -> ExitCode {
    eprintln!("pw: {} is not a readable {}: {}", path, kind, e);
    ExitCode::FAILURE
}

fn report_on(path: &str, kind: &str, r: &Report) {
    println!("{}  ({})", path, kind);
    println!(
        "  {} question(s), {} version(s), {} attempt(s), {} resistant",
        r.questions, r.versions, r.attempts, r.resistant
    );

    let blocking = r.blocking();
    let advisories = r.advisories();
    if !blocking.is_empty() {
        println!("\n  blocking - the run contradicts itself:");
        for f in &blocking {
            println!("    x {}", describe(f));
        }
    }
    if !advisories.is_empty() {
        println!("\n  reported - the instructor's calls:");
        for f in &advisories {
            println!("    - {}", describe(f));
        }
    }
    println!();
    if blocking.is_empty() {
        println!("  ok: nothing blocking. {} advisory finding(s).", advisories.len());
    } else {
        println!("  FAILED: {} blocking finding(s).", blocking.len());
    }
}

/// Findings name ordinals and enum values, so rendering them is safe: there is
/// nothing in one that could put a question on somebody's terminal.
fn describe(f: &Finding) -> String {
    let q = |n: &usize| n + 1;
    match f {
        Finding::TextChangedAfterPrompting { question, version } => format!(
            "question {}, v{}: the text on file is not the text its attempts recorded prompting",
            q(question), version
        ),
        Finding::QueryChangedBetweenAttempts { question, version, attempt } => format!(
            "question {}, v{}: attempt {} prompted different bytes from attempt 1",
            q(question), version, attempt
        ),
        Finding::TooFewAttempts { question, version, attempts } => format!(
            "question {}, v{}: claimed resistant on {} attempt(s), not 3",
            q(question), version, attempts
        ),
        Finding::TooManyAttempts { question, version, attempts } => format!(
            "question {}, v{}: {} attempts, more than the protocol allows",
            q(question), version, attempts
        ),
        Finding::AttemptMetThreshold { question, version, attempt } => format!(
            "question {}, v{}: attempt {} met the threshold on a version claimed resistant",
            q(question), version, attempt
        ),
        Finding::RubricRevisionDidNotExist { question, version, attempt } => format!(
            "question {}, v{}: attempt {} is graded against a rubric revision that post-dates it",
            q(question), version, attempt
        ),
        Finding::RubricRevisionMissing { question, version, attempt } => format!(
            "question {}, v{}: attempt {} names a rubric revision that is not in the file",
            q(question), version, attempt
        ),
        Finding::GradeCannotBeRederived { question, version, attempt } => format!(
            "question {}, v{}: attempt {}'s marks do not fit the revision that graded it",
            q(question), version, attempt
        ),
        Finding::StatusDoesNotFollow { question } => format!(
            "question {}: the recorded status is not the one its attempts produce",
            q(question)
        ),
        Finding::PenaltyWithoutProvenance { question } => format!(
            "question {}: a penalty chip points at an attempt that is not in the file",
            q(question)
        ),
        Finding::NoTargetRecorded => {
            "attempts were stamped without naming the model they were tested against".into()
        }
        Finding::GuardTripped { question, version } => format!(
            "question {}, v{}: saved with the complexity guard tripped",
            q(question), version
        ),
        Finding::InProgress { question, state } => {
            format!("question {}: still in progress ({})", q(question), state_name(state))
        }
        Finding::TargetChanged { targets } => {
            format!("the run named {} different targets; the manifest records the last", targets)
        }
        Finding::NotesNotPromoted { question, version, attempt, notes } => format!(
            "question {}, v{}: {} ledger note(s) on attempt {} never became penalty chips",
            q(question), version, notes, attempt
        ),
    }
}

fn state_name(s: &workbench_core::protocol::Status) -> &'static str {
    use workbench_core::protocol::Status::*;
    match s {
        Untested => "untested",
        NotResistant { .. } => "not resistant - needs a perturbation",
        Testing { .. } => "testing - more attempts to go",
        Resistant => "resistant",
        Inconsistent { .. } => "inconsistent - a later attempt passed",
    }
}
