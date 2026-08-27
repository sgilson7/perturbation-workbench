//! Questions that contain code, which most CS questions do.

use workbench_core::markup::{blocks, code_blocks, fence_monospace, prose_only, Block};
use workbench_core::readability::analyze;

fn code_of<'a>(bs: &'a [Block<'a>], n: usize) -> (&'a Option<&'a str>, &'a str) {
    match &bs[n] {
        Block::Code { language, body } => (language, body),
        other => panic!("block {} is {:?}, not code", n, other),
    }
}

#[test]
fn a_question_splits_into_prose_and_code() {
    let q = "Complete the method below.\n```java\npublic int sum(int[] a) {\n    return 0;\n}\n```\nState its asymptotic running time.";
    let bs = blocks(q);
    assert_eq!(bs.len(), 3);
    assert!(matches!(bs[0], Block::Prose(_)));
    assert!(bs[1].is_code());
    assert!(matches!(bs[2], Block::Prose(_)));

    let (lang, body) = code_of(&bs, 1);
    assert_eq!(*lang, Some("java"));
    // Indentation is exactly what was typed. A question whose code has been
    // re-indented is a different question.
    assert_eq!(body, "public int sum(int[] a) {\n    return 0;\n}\n");
    assert_eq!(code_blocks(q), 1);
}

#[test]
fn tildes_and_longer_fences_work_and_a_fence_can_hold_backticks() {
    let q = "Here:\n~~~python\ndef f():\n    return '```'\n~~~\nDone.";
    let bs = blocks(q);
    assert_eq!(code_blocks(q), 1);
    let (lang, body) = code_of(&bs, 1);
    assert_eq!(*lang, Some("python"));
    assert!(body.contains("```"));

    // A longer fence closes with one at least as long, not a shorter one.
    let q = "a\n````\ncode ``` still code\n````\nb";
    assert_eq!(code_blocks(q), 1);
}

/// A question being typed is still the question. Reclassifying the rest of it
/// as prose the moment a fence is opened would move the readability numbers
/// under the author mid-keystroke.
#[test]
fn an_unclosed_fence_runs_to_the_end() {
    let q = "Finish this:\n```c\nint main(void) {";
    let bs = blocks(q);
    assert_eq!(bs.len(), 2);
    let (lang, body) = code_of(&bs, 1);
    assert_eq!(*lang, Some("c"));
    assert_eq!(body, "int main(void) {");
}

#[test]
fn inline_code_stays_prose_and_loses_its_backticks() {
    let q = "Show that `mergeSort` runs in `n log n` time.";
    assert_eq!(code_blocks(q), 0);
    let p = prose_only(q);
    assert!(!p.contains('`'), "{}", p);
    assert!(p.contains("mergeSort"), "{}", p);
}

/// The reason this module exists: Flesch-Kincaid on a Java method returns a
/// grade level in the twenties, the complexity guard fires on every
/// code-bearing question, and the instructor learns to ignore the guard.
#[test]
fn code_does_not_drag_the_reading_level_up() {
    let prose = "Complete the method below. State its asymptotic running time.";
    let with_code = format!(
        "Complete the method below.\n```java\n{}\n```\nState its asymptotic running time.",
        "public static int accumulateWeightedSubtotal(int[] xs) { return 0; }\n".repeat(12)
    );

    let bare = analyze(prose);
    let coded = analyze(&with_code);
    // One word ("code") stands in for the whole block, so the sentence
    // structure survives and the identifiers do not count.
    assert_eq!(coded.words, bare.words + 1);
    assert!(
        (coded.grade - bare.grade).abs() < 2.0,
        "code moved the grade from {} to {}",
        bare.grade,
        coded.grade
    );

    // A question that is only code still measures as something.
    let only = analyze("```\nwhile (true) { tick(); }\n```");
    assert_eq!(only.words, 1);
}

/// Deleting the block instead would leave "Complete the method" followed by
/// nothing, and a question that is entirely code would measure as empty.
#[test]
fn code_is_replaced_rather_than_deleted() {
    let p = prose_only("Before.\n```\nx = 1\n```\nAfter.");
    assert!(p.contains("Before."), "{}", p);
    assert!(p.contains("code"), "{}", p);
    assert!(p.contains("After."), "{}", p);
    assert!(!p.contains("x = 1"), "{}", p);
}

/// On ingest, a PDF has no idea it contains code — but it does know which
/// glyphs came from a monospaced font, and in a CS assignment that is very
/// nearly the same question.
#[test]
fn monospaced_runs_become_fenced_code() {
    let lines = [
        ("Complete the method:".to_string(), false),
        ("public int sum(int[] a) {".to_string(), true),
        ("".to_string(), true),
        ("    return 0;".to_string(), true),
        ("}".to_string(), true),
        ("State its running time.".to_string(), false),
    ];
    let out = fence_monospace(&lines);
    assert_eq!(code_blocks(&out), 1);
    let bs = blocks(&out);
    let (_, body) = code_of(&bs, 1);
    assert!(body.contains("    return 0;"), "{:?}", body);
    assert!(body.contains("public int sum"), "{:?}", body);
}

/// Indentation arrives measured against the page, not against the block, so a
/// method set right of the prose margin comes back with the same two spaces in
/// front of every line. The block's own left edge is its zero.
#[test]
fn a_code_run_is_dedented_to_its_own_left_edge() {
    let lines = [
        ("Complete the method:".to_string(), false),
        ("  public static int sum(int[] xs) {".to_string(), true),
        ("      int total = 0;".to_string(), true),
        ("          total += x;".to_string(), true),
        ("  }".to_string(), true),
        ("State its running time.".to_string(), false),
    ];
    let out = fence_monospace(&lines);
    let bs = blocks(&out);
    let (_, body) = code_of(&bs, 1);
    assert_eq!(body, "public static int sum(int[] xs) {\n    int total = 0;\n        total += x;\n}\n");
    // The prose around it keeps whatever it had.
    assert!(out.starts_with("Complete the method:"));
}

/// A blank line inside a function is not the end of the function. Closing the
/// fence on one shreds every method with a paragraph break into pieces.
#[test]
fn a_blank_line_does_not_end_a_code_run() {
    let lines = [
        ("def a():".to_string(), true),
        ("".to_string(), false),
        ("def b():".to_string(), true),
    ];
    assert_eq!(code_blocks(&fence_monospace(&lines)), 1);
}

#[test]
fn a_question_with_no_code_is_left_exactly_alone() {
    let plain = "Prove that the sum of two even integers is even. Use the definition.";
    assert_eq!(prose_only(plain), plain);
    assert_eq!(blocks(plain).len(), 1);
    assert_eq!(code_blocks(plain), 0);
}
