//! The manifest exists to be shared. These tests are what make that safe.

mod common;

use common::{at, build_id, digest, question, resistant_run, run};
use serde_json::Value;
use workbench_core::hash::{InvalidId, ModelId, Sha256Hex, Timestamp};
use workbench_core::manifest::{build, Manifest, ManifestError, Outputs, SCHEMA};
use workbench_core::protocol::{Status, Strategy};
use workbench_core::rubric::AttemptRef;
use workbench_core::session::{Access, Session};
use workbench_core::verify::Finding;

fn created() -> Timestamp {
    Timestamp::parse("2026-08-27T14:02:11Z").unwrap()
}

/// A run dense with exactly the things that must never escape: a title, nine
/// rubric chip labels naming the marking scheme, two perturbed question texts,
/// and a ledger note describing what the model got wrong.
fn study_run() -> Session {
    let mut s = run();
    s.add_question(question(5));

    // Step 3-4: the baseline passes, so it is perturbed.
    let marks: Vec<f64> = vec![100.0; 8];
    common::stamp(&mut s, 0, 1, &marks);
    s.question_mut(0)
        .unwrap()
        .add_version(
            Strategy::Spatial,
            "Question 5 - Probability at the Salad Bar. Items i0 through i9 sit in a grid of \
             three rows. Read the row of each slot off the diagram before computing anything.",
        )
        .unwrap();

    // Step 8: three failures on the perturbed version.
    let zero: Vec<f64> = vec![0.0; 8];
    for m in [10u32, 15, 20] {
        common::stamp(&mut s, 0, m, &zero);
    }

    // Step 6: a hallucination logged and promoted to a penalty chip.
    let from = AttemptRef { version: 1, attempt: 1 };
    s.question_mut(0)
        .unwrap()
        .note(from, "Invented P(7,6) = 7^6 and never used the Bijection Rule.")
        .unwrap();
    s.question_mut(0)
        .unwrap()
        .add_penalty("Hallucinated a permutation formula", -4, from, at(22))
        .unwrap();
    s
}

fn manifest_of(s: &Session) -> Manifest {
    build(build_id(), created(), s, None).expect("the run should verify")
}

/// Every string *value* in a document, paired with the field it sat under.
///
/// Values rather than raw JSON: field names are fixed by the schema and cannot
/// leak, and searching the raw text finds "row" inside "growthCap" and reports
/// a leak in a file that has none — the same false positive pdf-redactor's
/// verifier had to be taught to avoid.
fn string_values(v: &Value, key: Option<&str>, out: &mut Vec<(String, String)>) {
    match v {
        Value::String(s) => out.push((key.unwrap_or("").to_string(), s.clone())),
        Value::Array(a) => a.iter().for_each(|x| string_values(x, key, out)),
        Value::Object(o) => o.iter().for_each(|(k, x)| string_values(x, Some(k), out)),
        _ => {}
    }
}

fn values_of(m: &Manifest) -> Vec<(String, String)> {
    let mut out = Vec::new();
    string_values(&serde_json::to_value(m).unwrap(), None, &mut out);
    out
}

/// The central assertion. Everything else in this file supports it.
#[test]
fn a_manifest_cannot_contain_question_text() {
    let m = manifest_of(&study_run());
    let carried = values_of(&m)
        .into_iter()
        .filter(|(k, _)| k != "note" && k != "howToCheck")
        .map(|(_, v)| v)
        .collect::<Vec<_>>()
        .join("\n");
    for forbidden in [
        "Salad", "salad", "Probability", "probability", "sundae", "Bijection", "bijection",
        "Bayes", "i9", "Items", "grid", "diagram", "Invented", "P(7,6)", "permutation",
        "Hallucinated", "Additivity", "Conditional", "Independence", "row", "slot",
    ] {
        assert!(!carried.contains(forbidden), "the manifest leaked {:?}\n{}", forbidden, carried);
    }
}

/// Rule: no field carries free text other than a validated hash, an instant, a
/// build id, or the model identifier. This walks the whole serialised document
/// and classifies every string in it, so the rule is checked rather than
/// asserted.
#[test]
fn no_field_carries_free_text() {
    // The document's own fixed prose: not caller-supplied, so not a route in.
    let constants = [
        SCHEMA,
        "perturbation-workbench",
        "Gilson, Tabarsi & Barnes, AIED 2026, Table 1 (doi:10.1007/978-3-032-29770-9_48)",
    ];
    // Every enum name the schema can emit.
    let enum_names = [
        "spatial", "axiomatic", "contextual",
        "untested", "notResistant", "testing", "resistant", "inconsistent",
        "institutional", "consumer", "api", "unspecified",
        "textChangedAfterPrompting", "queryChangedBetweenAttempts", "tooFewAttempts",
        "tooManyAttempts", "attemptMetThreshold", "rubricRevisionDidNotExist",
        "rubricRevisionMissing", "gradeCannotBeRederived", "penaltyWithoutProvenance",
        "noTargetRecorded", "guardTripped", "inProgress", "targetChanged", "notesNotPromoted",
    ];

    let m = manifest_of(&study_run());
    let found = values_of(&m);
    assert!(found.len() > 10, "the walk found almost nothing: {:?}", found);

    for (key, s) in &found {
        // The two fields that are prose on purpose, and say so.
        if key == "note" || key == "howToCheck" {
            continue;
        }
        let ok = constants.contains(&s.as_str())
            || enum_names.contains(&s.as_str())
            || Sha256Hex::parse(s).is_ok()
            || Timestamp::parse(s).is_ok()
            || s == m.build.as_str()
            || m.target.as_ref().is_some_and(|t| t.model.as_str() == s);
        assert!(ok, "field {:?} carries the free string {:?}", key, s);
    }
}

#[test]
fn a_manifest_says_what_it_is() {
    let json = serde_json::to_string(&manifest_of(&study_run())).unwrap();
    assert!(json.contains("\"containsQuestionText\":false"));
    assert!(json.contains(SCHEMA));
    assert!(json.contains("10.1007/978-3-032-29770-9_48"));
    // It carries its own recipe for checking the files it accompanies.
    assert!(json.contains("shasum -a 256"));
}

/// The manifest's whole job is to let someone else re-run the check. These are
/// the fields they check against.
#[test]
fn a_manifest_records_what_a_collaborator_needs_to_repeat_the_run() {
    let s = study_run();
    let m = manifest_of(&s);

    assert_eq!(m.questions.len(), 1);
    let q = &m.questions[0];
    assert_eq!(q.ordinal, 5);
    assert_eq!(q.status, Status::Resistant);
    assert_eq!(q.versions.len(), 2);
    assert_eq!(q.rubric.chips, 8);
    assert_eq!(q.rubric.penalty_chips, 1);
    assert_eq!(q.rubric.revisions, 2);
    assert_eq!(q.rubric.total_points, 16);
    assert_eq!(q.rubric.scale_levels, 3);

    // The exact bytes that were prompted, three times, all the same.
    let v1 = &q.versions[1];
    assert_eq!(v1.strategy, Some(Strategy::Spatial));
    assert_eq!(v1.attempts.len(), 3);
    assert!(v1.locked);
    assert!(v1.attempts.iter().all(|a| a.query_sha256 == v1.text_sha256));
    assert!(v1.attempts.iter().all(|a| a.pct == 0.0));
    assert_eq!(v1.attempts.iter().map(|a| a.ordinal).collect::<Vec<_>>(), [1, 2, 3]);
    // A prose question carries no code; a CS1 one would say so here.
    assert_eq!(v1.code_blocks, 0);

    // ...and the digest a reader can reproduce from the exported query file.
    let text = s.question(0).unwrap().version(1).unwrap().text();
    assert_eq!(v1.text_sha256, Sha256Hex::of(text.as_bytes()));

    // Step 6's provenance survives as counts.
    assert_eq!(v1.attempts[0].ledger_entries, 1);
    assert_eq!(v1.attempts[0].penalties_derived, 1);
    assert_eq!(v1.attempts[0].rubric_revision, 1);
    assert_eq!(v1.attempts[2].rubric_revision, 1);

    assert_eq!(m.input.as_ref().unwrap().pages, 22);
    assert_eq!(m.input.as_ref().unwrap().questions_ingested, 1);
    assert_eq!(m.target.as_ref().unwrap().model.as_str(), "gemini-2.5-flash");
    assert_eq!(m.target.as_ref().unwrap().access, Access::Institutional);
    assert!(m.target.as_ref().unwrap().fresh_instance_per_attempt);
}

/// "Resistant" means nothing without the line it was judged against, so the
/// line travels with it.
/// The tool is for any CS assignment, so a question can be mostly code. The
/// manifest says that it was, and says nothing about what the code did.
#[test]
fn a_manifest_records_that_a_question_carried_code_but_not_the_code() {
    let mut s = run();
    let mut q = question(5);
    q.rubric_mut().add_chip("Loop terminates", 4).unwrap();
    s.add_question(q);
    s.question_mut(0)
        .unwrap()
        .add_version(
            Strategy::Contextual,
            "Complete the method.\n```java\nint mysteryAccumulator(int[] xs) { return 0; }\n```",
        )
        .unwrap();
    let none: Vec<f64> = vec![0.0; 9];
    for m in [10u32, 15, 20] {
        common::stamp(&mut s, 0, m, &none);
    }

    let m = manifest_of(&s);
    assert_eq!(m.questions[0].versions[1].code_blocks, 1);
    assert_eq!(m.questions[0].versions[0].code_blocks, 0);
    let carried = values_of(&m).into_iter().map(|(_, v)| v).collect::<Vec<_>>().join("\n");
    for leak in ["mysteryAccumulator", "java", "int[]", "return"] {
        assert!(!carried.contains(leak), "the manifest leaked {:?}", leak);
    }
}

#[test]
fn a_manifest_records_the_line_it_judged_against() {
    let m = manifest_of(&study_run());
    assert_eq!(m.settings.threshold.get(), 60.0);
    assert_eq!(m.settings.fk_drift, 1.5);
    assert_eq!(m.settings.fk_cap, 14.0);
    assert_eq!(m.settings.growth_cap, 35.0);
}

/// A manifest whose own audit says the evidence is broken is not evidence.
#[test]
fn a_blocked_run_produces_no_manifest_at_all() {
    let mut v = serde_json::to_value(resistant_run()).unwrap();
    v["questions"][0]["versions"][1]["text"] = Value::from("Something else entirely.");
    let forged: Session = serde_json::from_value(v).unwrap();

    match build(build_id(), created(), &forged, None) {
        Err(ManifestError::Blocked(findings)) => {
            assert!(findings
                .contains(&Finding::TextChangedAfterPrompting { question: 0, version: 1 }));
            assert!(findings.iter().all(Finding::is_blocking));
        }
        other => panic!("a broken run produced {:?}", other.map(|_| "a manifest")),
    }
}

/// Advisories are the instructor's calls, and they travel into the file rather
/// than stopping it.
#[test]
fn advisories_are_written_in_and_blocking_is_empty_by_construction() {
    let mut s = study_run();
    s.set_target("gemini-2.5-pro", Access::Institutional, true, at(40)).unwrap();
    s.add_question(question(6)); // never tested

    let m = manifest_of(&s);
    assert!(m.verification.blocking.is_empty());
    assert!(m.verification.advisories.contains(&Finding::TargetChanged { targets: 2 }));
    assert!(m
        .verification
        .advisories
        .contains(&Finding::InProgress { question: 1, state: Status::Untested }));
    assert_eq!(m.target.unwrap().model.as_str(), "gemini-2.5-pro");
}

#[test]
fn the_outputs_block_records_the_files_the_manifest_accompanies() {
    let s = study_run();
    let assignment = Sha256Hex::of(b"%PDF-1.7 ...");
    let outputs = Outputs {
        assignment_sha256: assignment.clone(),
        includes_history: true,
        includes_ledger: false,
        query_files: 1,
    };
    let m = build(build_id(), created(), &s, Some(outputs)).unwrap();
    assert_eq!(m.outputs.as_ref().unwrap().assignment_sha256, assignment);
    assert!(m.outputs.as_ref().unwrap().includes_history);

    // Taken mid-run, before anything has been written, the block is absent
    // rather than filled with zeros.
    let m = manifest_of(&s);
    assert!(m.outputs.is_none());
    let doc: Value = serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
    assert!(doc.as_object().unwrap().get("outputs").is_none());
}

/// Manifests get diffed across tool versions to catch a silent regression, so
/// the same run must always produce the same bytes.
#[test]
fn serialisation_is_stable_for_diffing() {
    let s = study_run();
    let a = serde_json::to_string_pretty(&manifest_of(&s)).unwrap();
    let b = serde_json::to_string_pretty(&manifest_of(&s)).unwrap();
    assert_eq!(a, b);

    // ...and reads back to the same document.
    let back: Manifest = serde_json::from_str(&a).unwrap();
    assert_eq!(back, manifest_of(&s));
}

/// The model identifier is the one piece of prose the file keeps, so it is the
/// one route a question could take. It is bounded and filtered, not trusted.
#[test]
fn a_question_cannot_be_smuggled_through_the_model_name() {
    for bad in [
        "How many different sundaes can you make, assuming each topping is on or off?",
        "",
        "   ",
        "gemini-2.5-flash\nQuestion 5 - Probability at the Salad Bar.",
        "-leading-punctuation",
        "trailing-punctuation-",
        &"a".repeat(65),
    ] {
        assert_eq!(ModelId::parse(bad), Err(InvalidId::NotAModel), "{:?}", bad);
    }
    for good in ["gemini-2.5-flash", "gpt-5.1", "claude-opus-5", "Gemini 2.5 Flash", "llama3.1:8b"] {
        assert!(ModelId::parse(good).is_ok(), "{:?}", good);
    }

    // And a session refuses it at the source, not only at export.
    let mut s = Session::default();
    assert!(s.set_target("Question 5 - Probability at the Salad Bar.", Access::Api, true, at(0)).is_err());
}

/// A doctored manifest is refused rather than read partially, the same way a
/// doctored session is.
#[test]
fn a_manifest_from_another_format_is_refused() {
    let json = serde_json::to_string(&manifest_of(&study_run())).unwrap();
    for edit in [
        (r#""ordinal":5"#, r#""ordinal":5,"title":"Probability at the Salad Bar""#),
        (r#""pages":22"#, r#""pages":22,"filename":"Lab3-Part2.pdf""#),
    ] {
        let doctored = json.replacen(edit.0, edit.1, 1);
        assert_ne!(doctored, json, "the edit did not apply: {:?}", edit.0);
        assert!(
            serde_json::from_str::<Manifest>(&doctored).is_err(),
            "a manifest with {:?} was accepted",
            edit.1
        );
    }
    // A build id is a short hex hash, not somewhere to put a sentence.
    let doctored = json.replacen(r#""build":"c8878585""#, r#""build":"the salad bar one""#, 1);
    assert!(serde_json::from_str::<Manifest>(&doctored).is_err());
    let _ = digest("unused");
}
