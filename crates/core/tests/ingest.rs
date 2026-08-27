//! Turning somebody else's assignment into a starting point.
//!
//! The fixtures here are synthetic and deliberately not all discrete
//! mathematics: the tool has to work on a CS1 lab, an algorithms problem set,
//! and a proof-based worksheet, and each numbers its questions differently.

mod common;

use workbench_core::hash::{canonical, Sha256Hex};
use workbench_core::ingest::{ingest, split_at, HeadingStyle, Line};
use workbench_core::markup::code_blocks;
use workbench_core::protocol::Question;

/// Build a page of lines, marking none of them monospaced.
fn page(n: usize, text: &[&str]) -> Vec<Line> {
    text.iter().map(|t| Line { text: (*t).to_string(), page: n, mono: false }).collect()
}

/// A lab in the shape the study's was: `Question N:` headings, `Part N`
/// markers, checkbox glyphs, and a two-line running footer whose second line
/// carries the page number.
fn lab() -> Vec<Line> {
    let mut v = Vec::new();
    v.extend(page(1, &[
        "CSC226: Discrete Mathematics",
        "Question 4:",
        "Part 1 The tray has 8 positions. Compute A union B. \u{25A1}",
        "Part 2 Compute the size of the power set of A. \u{25A1}",
        "\u{00A9} NC State University Computer Science Faculty",
        "Question 4:-1",
    ]));
    v.extend(page(2, &[
        "CSC226: Discrete Mathematics",
        "Part 3 Classify each mapping as injective or surjective. \u{25A1}",
        "\u{00A9} NC State University Computer Science Faculty",
        "Question 4:-2",
    ]));
    v.extend(page(3, &[
        "CSC226: Discrete Mathematics",
        "Question 5:",
        "Part 1 What is the probability that i9 is in the top row? \u{25A1}",
        "\u{00A9} NC State University Computer Science Faculty",
        "Question 5:-1",
    ]));
    v.extend(page(4, &[
        "CSC226: Discrete Mathematics",
        "Question 6:",
        "Part 1 Build the truth table for p and q implies r. \u{25A1}",
        "\u{00A9} NC State University Computer Science Faculty",
        "Question 6:-1",
    ]));
    v
}

#[test]
fn a_lab_splits_into_its_questions() {
    let out = ingest(&lab());
    assert_eq!(out.style, Some(HeadingStyle::Question));
    assert_eq!(out.pages, 4);
    assert_eq!(out.drafts.len(), 3);
    assert_eq!(out.drafts.iter().map(|d| d.ordinal).collect::<Vec<_>>(), [4, 5, 6]);

    // Question 4 runs across a page break and keeps both of its pages.
    let q4 = &out.drafts[0];
    assert!(q4.text.contains("Part 1 The tray has 8 positions"));
    assert!(q4.text.contains("Part 2"));
    assert!(q4.text.contains("Part 3 Classify each mapping"));
    assert!(!q4.text.contains("Question 5"));
}

/// A running footer repeats with its page number changing, so comparing raw
/// text finds nothing and comparing shapes finds it on the first try.
#[test]
fn running_page_furniture_is_dropped() {
    let out = ingest(&lab());
    for d in &out.drafts {
        assert!(!d.text.contains("NC State University"), "footer survived:\n{}", d.text);
        assert!(!d.text.contains("CSC226"), "header survived:\n{}", d.text);
        assert!(!d.text.contains(":-1"), "page marker survived:\n{}", d.text);
    }
    assert!(out.furniture_dropped >= 10, "{} lines dropped", out.furniture_dropped);
}

/// Plenty of assignments are two pages long, and a rule that needed three
/// pages to establish a repeat left the footer on every one of them.
#[test]
fn a_footer_on_a_two_page_assignment_is_still_a_footer() {
    let mut v = page(1, &[
        "CSC 999: Programming Fundamentals",
        "Question 1. Compute the sum.",
        "Part 1 Show your working.",
        "\u{00A9} CSC 999 Faculty",
        "Question 1:-1",
    ]);
    v.extend(page(2, &[
        "CSC 999: Programming Fundamentals",
        "Question 2. Prove it.",
        "Part 1 State the definition.",
        "\u{00A9} CSC 999 Faculty",
        "Question 2:-1",
    ]));
    let out = ingest(&v);
    assert_eq!(out.drafts.len(), 2);
    for d in &out.drafts {
        assert!(!d.text.contains("CSC 999 Faculty"), "Q{} kept the footer:\n{}", d.ordinal, d.text);
        assert!(!d.text.contains("Programming Fundamentals"), "Q{} kept the header", d.ordinal);
        assert!(d.text.contains("Part 1"));
    }

    // A single page has nothing to repeat against, and nothing is swept.
    let one = ingest(&page(1, &["Question 1. Compute.", "Part 1 Show it.", "Footer line"]));
    assert_eq!(one.furniture_dropped, 0);
    assert!(one.drafts[0].text.contains("Footer line"));
}

/// `Question 4:-1` is a page footer. Reading it as question 4 would split the
/// lab into a dozen fragments; dropping question 5's heading as furniture
/// because it looks like question 4's would lose a question entirely.
#[test]
fn a_footer_shaped_like_a_heading_is_neither_a_heading_nor_furniture() {
    let out = ingest(&lab());
    assert_eq!(out.drafts.len(), 3);
    assert!(out.drafts.iter().all(|d| !d.text.is_empty()));
}

/// The glyph is an artefact of the answer sheet, not of the question. Left in,
/// it goes into every query the model is asked and into the exported PDF.
#[test]
fn checkbox_glyphs_do_not_reach_the_query() {
    let out = ingest(&lab());
    assert_eq!(out.checkboxes_stripped, 5);
    for d in &out.drafts {
        assert!(!d.text.contains('\u{25A1}'), "{}", d.text);
    }
}

/// Assignments number their questions in a handful of ways. Each of these is
/// somebody's real convention.
#[test]
fn every_common_numbering_convention_is_recognised() {
    let cases: &[(&str, HeadingStyle)] = &[
        ("Question", HeadingStyle::Question),
        ("Problem", HeadingStyle::Problem),
        ("Exercise", HeadingStyle::Exercise),
        ("Task", HeadingStyle::Task),
    ];
    for (word, style) in cases {
        let lines = page(1, &[
            &format!("{} 1. Write a loop that prints the first ten squares.", word),
            "Use a for loop.",
            &format!("{} 2. Explain why your loop terminates.", word),
        ]);
        let out = ingest(&lines);
        assert_eq!(out.style, Some(*style), "{}", word);
        assert_eq!(out.drafts.len(), 2, "{}", word);
        assert_eq!(out.drafts[0].title, "Write a loop that prints the first ten squares.");
    }

    // `Q3` / `Q4`, the shorthand half of every problem set.
    let out = ingest(&page(1, &["Q3: Sort the array.", "In place.", "Q4: Prove it terminates."]));
    assert_eq!(out.style, Some(HeadingStyle::Abbreviated));
    assert_eq!(out.drafts.iter().map(|d| d.ordinal).collect::<Vec<_>>(), [3, 4]);

    // A bare numbered list, when that is all there is.
    let out = ingest(&page(1, &["1. Implement push.", "Use an array.", "2. Implement pop."]));
    assert_eq!(out.style, Some(HeadingStyle::Numbered));
    assert_eq!(out.drafts.len(), 2);
}

/// The fact this module leans on: a document picks one convention and sticks
/// to it. So a `1.` inside a list of parts cannot be promoted to a question in
/// a document whose questions say `Question 4`.
#[test]
fn the_documents_own_convention_wins_over_a_numbered_sublist() {
    let lines = page(1, &[
        "Question 1. Implement a stack.",
        "1. push",
        "2. pop",
        "3. peek",
        "Question 2. Analyse its complexity.",
        "1. time",
        "2. space",
    ]);
    let out = ingest(&lines);
    assert_eq!(out.style, Some(HeadingStyle::Question));
    assert_eq!(out.drafts.len(), 2);
    assert!(out.drafts[0].text.contains("1. push"));
    assert!(out.drafts[0].text.contains("3. peek"));
}

/// One draft with everything in it beats nine wrong ones.
#[test]
fn a_document_with_no_convention_comes_back_as_one_draft() {
    let lines = page(1, &[
        "Describe the difference between a stack and a queue.",
        "Give an example of each from a program you have written.",
    ]);
    let out = ingest(&lines);
    assert_eq!(out.style, None);
    assert_eq!(out.drafts.len(), 1);
    assert!(out.drafts[0].text.contains("stack and a queue"));

    // ...and then the instructor cuts it by hand.
    let (a, b) = split_at(&out.drafts[0].text, 1).unwrap();
    assert!(a.contains("stack and a queue"));
    assert!(b.contains("Give an example"));
    assert_eq!(split_at(&out.drafts[0].text, 0), None);
    assert_eq!(split_at(&out.drafts[0].text, 99), None);
}

/// A CS1 lab, which is what most of this tool's users will actually open.
#[test]
fn a_cs1_lab_keeps_its_code_as_code() {
    let mut lines = page(1, &["Question 1. Complete the method below."]);
    for (t, mono) in [
        ("public static int sum(int[] xs) {", true),
        ("    int total = 0;", true),
        ("    for (int x : xs) total += x;", true),
        ("    return total;", true),
        ("}", true),
    ] {
        lines.push(Line { text: t.to_string(), page: 1, mono });
    }
    lines.extend(page(1, &["Question 2. State its running time in big-O."]));

    let out = ingest(&lines);
    assert_eq!(out.drafts.len(), 2);
    assert_eq!(out.code_lines, 5);
    let q1 = &out.drafts[0];
    assert_eq!(code_blocks(&q1.text), 1);
    // Indentation is the code. Losing it would change the question.
    assert!(q1.text.contains("    int total = 0;"), "{}", q1.text);
    assert_eq!(code_blocks(&out.drafts[1].text), 0);
}

/// A draft goes straight into a question, so it has to already be the bytes
/// that will be hashed — otherwise the digest shown at ingest and the digest
/// recorded at the first attempt would differ.
#[test]
fn an_ingested_draft_is_already_canonical() {
    let lines = page(1, &[
        "Question 1. Trace the loop.   ",
        "Show every iteration.\u{00A0}",
        "Question 2. Explain.",
    ]);
    let out = ingest(&lines);
    let d = &out.drafts[0];
    assert_eq!(d.text, canonical(&d.text));
    assert!(!d.text.contains('\u{00A0}'));
    assert!(!d.text.ends_with(' '));

    let q = Question::new(d.ordinal, &d.title, &d.text).unwrap();
    assert_eq!(q.base().text(), d.text);
    assert_eq!(q.base().text_sha256(), Sha256Hex::of(d.text.as_bytes()));
}

#[test]
fn a_question_with_no_title_on_its_heading_line_still_gets_one() {
    let out = ingest(&page(1, &["Question 4:", "Part 1 Compute.", "Question 5:", "Part 1 Prove."]));
    assert_eq!(out.drafts[0].title, "Question 4");
    assert_eq!(out.drafts[1].title, "Question 5");
}

#[test]
fn an_empty_document_ingests_to_nothing_rather_than_to_a_blank_question() {
    let out = ingest(&[]);
    assert!(out.drafts.is_empty());
    assert_eq!(out.pages, 0);
    let out = ingest(&page(1, &["", "   ", ""]));
    assert!(out.drafts.is_empty());
}

/// The real lab, when it is on this machine. It is gitignored, so CI runs
/// everything above and skips this one; the synthetic fixtures are shaped
/// after it precisely so that they can stand in.
#[test]
fn the_real_lab_splits_into_its_nine_questions() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/lab3_part2.lines.json");
    let Ok(raw) = std::fs::read_to_string(path) else {
        eprintln!("skipped: no fixture at {} (run `make fixtures`)", path);
        return;
    };
    let lines: Vec<Line> = serde_json::from_str(&raw).expect("the fixture should parse");
    let out = ingest(&lines);

    assert_eq!(out.style, Some(HeadingStyle::Question));
    assert_eq!(out.drafts.len(), 9);
    assert_eq!(
        out.drafts.iter().map(|d| d.ordinal).collect::<Vec<_>>(),
        [4, 5, 6, 7, 8, 9, 10, 11, 12]
    );
    for d in &out.drafts {
        assert!(!d.text.contains('\u{25A1}'), "Q{} kept a checkbox", d.ordinal);
        assert!(!d.text.contains("NC State University"), "Q{} kept the footer", d.ordinal);
        assert!(d.text.contains("Part 1"), "Q{} lost its parts", d.ordinal);
    }
    assert!(out.checkboxes_stripped > 40, "{}", out.checkboxes_stripped);
}
