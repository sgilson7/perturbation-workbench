//! The exported assignment: what is in the file, and what is not.

mod common;

use common::{at, question, resistant_run, run, simple, stamp};
use workbench_core::assignment::{render, Mode, Options};
use workbench_core::metrics::{points, Font};
use workbench_core::pdfwrite::{build, encode, Line, Page, Rule, Run, PAGE_W};
use workbench_core::protocol::Strategy;
use workbench_core::rubric::AttemptRef;
use workbench_core::session::Session;

/// Every literal string in the file, as bytes. Works precisely because content
/// streams are left uncompressed — if that ever changes, this goes quiet and
/// the structural checks below go with it.
fn literals(pdf: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < pdf.len() {
        if pdf[i] == b'(' {
            let mut j = i + 1;
            while j < pdf.len() && pdf[j] != b')' {
                if pdf[j] == b'\\' {
                    j += 1;
                }
                out.push(pdf[j]);
                j += 1;
            }
            out.push(b' ');
            i = j + 1;
        } else {
            i += 1;
        }
    }
    out
}

fn text_of(pdf: &[u8]) -> String {
    String::from_utf8_lossy(&literals(pdf)).to_string()
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

fn pdf_of(s: &Session, opts: &Options) -> Vec<u8> {
    build(&render(s, opts).pages)
}

// ------------------------------------------------------------------ structure

/// An assignment carries its author's name in its metadata by default in every
/// word processor, and this one is going to be handed to a class.
#[test]
fn the_output_carries_no_metadata_at_all() {
    let pdf = pdf_of(&resistant_run(), &Options::default());
    for key in [
        "/Info", "/Metadata", "/Author", "/Producer", "/Creator", "/CreationDate", "/ModDate",
        "/Annots", "/AcroForm", "/EmbeddedFile", "/JavaScript", "/OpenAction", "/PieceInfo",
        "/Names", "/Thumb", "/ID",
    ] {
        assert!(!contains(&pdf, key.as_bytes()), "the output emitted {}", key);
    }
    // Exactly one revision: no incremental-update history to recover an
    // earlier draft of a question from.
    assert_eq!(pdf.windows(5).filter(|w| *w == b"%%EOF").count(), 1);
    assert!(pdf.starts_with(b"%PDF-1.7"));
}

/// The claim is that the file contains only what was written. Uncompressed
/// streams are what let a reader check it without the tool.
#[test]
fn the_output_can_be_checked_with_strings() {
    let mut s = run();
    s.add_question(simple());
    let pdf = pdf_of(&s, &Options::default());
    assert!(text_of(&pdf).contains("Prove that two plus two is four"));
    assert!(!contains(&pdf, b"/FlateDecode"));
}

/// Base-14 only: no font program from anywhere else can ride along.
#[test]
fn only_the_standard_fonts_are_referenced_and_none_embedded() {
    let pdf = pdf_of(&resistant_run(), &Options::default());
    for face in ["Helvetica", "Helvetica-Bold", "Courier", "Symbol"] {
        assert!(contains(&pdf, face.as_bytes()), "missing {}", face);
    }
    for embedded in ["/FontFile", "/FontFile2", "/FontFile3", "/TrueType", "/Type0"] {
        assert!(!contains(&pdf, embedded.as_bytes()), "the output embedded {}", embedded);
    }
}

// ------------------------------------------------------------------ glyphs

/// WinAnsiEncoding has none of this, and a discrete-mathematics assignment is
/// made of it. Symbol is a base-14 font, so it costs nothing to switch into.
#[test]
fn mathematics_is_typeset_rather_than_dropped() {
    let (runs, approx) = encode("A ∪ B and A ∩ B, ∀x ∈ S, Θ(n log n)", Font::Body);
    assert_eq!(approx, 0, "no character here should need approximating");

    let symbol: Vec<u8> = runs
        .iter()
        .filter(|(f, _)| *f == Font::Symbol)
        .flat_map(|(_, b)| b.clone())
        .collect();
    assert!(symbol.contains(&0xC8), "union missing");
    assert!(symbol.contains(&0xC7), "intersection missing");
    assert!(symbol.contains(&0x22), "for-all missing");
    assert!(symbol.contains(&0xCE), "element-of missing");
    assert!(symbol.contains(&0x51), "capital theta missing");

    // ...and it reaches the file.
    let mut s = run();
    s.add_question(
        workbench_core::protocol::Question::new(4, "Sets", "Compute A ∪ B and A ∩ B.").unwrap(),
    );
    let pdf = pdf_of(&s, &Options::default());
    assert!(contains(&pdf, &[0xC8]), "the union glyph did not reach the file");
    assert!(contains(&pdf, b"/Symbol"));
}

/// The alternative to Symbol was transliterating to "A union B", which is a
/// worse assignment. What is left over is substituted and counted, so a reader
/// can be told the PDF approximated something rather than finding a gap.
#[test]
fn an_unrepresentable_character_is_substituted_and_counted() {
    let (runs, approx) = encode("Tick the box □ and compute ⌈25/7⌉", Font::Body);
    assert_eq!(approx, 3);
    let drawn: String = runs
        .iter()
        .flat_map(|(_, b)| b.iter().map(|&c| c as char))
        .collect();
    assert!(drawn.contains("[ ]"), "{}", drawn);
    assert!(drawn.contains("ceil("), "{}", drawn);

    let mut s = run();
    s.add_question(
        workbench_core::protocol::Question::new(1, "Boxes", "Tick the box □ here.").unwrap(),
    );
    assert_eq!(render(&s, &Options::default()).approximations, 1);
}

/// Indentation is the meaning of a code block. A fixed-pitch face is the only
/// way a column of aligned characters stays a column.
/// Found by rendering the real lab: the Unicode minus is not the ASCII hyphen,
/// and every set difference in the paper came out as "A ? B". Line breaks were
/// also being counted as characters that could not be drawn, which reported an
/// approximation on every line of every question.
#[test]
fn the_characters_a_maths_paper_actually_uses_are_drawn() {
    assert_eq!(workbench_core::pdfwrite::approximations("A − B"), 0);
    let drawn: String = encode("A − B", Font::Body)
        .0
        .iter()
        .flat_map(|(_, b)| b.iter().map(|&c| c as char))
        .collect();
    assert_eq!(drawn, "A - B");

    // Line breaks, zero-width marks and soft hyphens draw nothing and are not
    // approximations.
    for invisible in ["a\nb", "a\u{0338}b", "a\u{00AD}b", "a\u{200B}b"] {
        assert_eq!(workbench_core::pdfwrite::approximations(invisible), 0, "{:?}", invisible);
    }
    assert_eq!(
        encode("a\nb", Font::Body).0.iter().map(|(_, b)| b.len()).sum::<usize>(),
        2
    );
}

/// An ingested heading with no title of its own already says everything the
/// line can say.
#[test]
fn a_question_with_no_title_is_not_headed_twice() {
    let mut s = run();
    s.add_question(
        workbench_core::protocol::Question::new(4, "Question 4", "Compute the thing.").unwrap(),
    );
    // Checked on the runs rather than the extracted text: the em dash is a
    // WinAnsi byte and does not survive a UTF-8 reading of the literals.
    let headings: Vec<String> = render(&s, &Options::default())
        .pages
        .iter()
        .flat_map(|p| p.lines.iter().filter(|l| l.size > 12.0))
        .flat_map(|l| l.runs.iter().map(|r| r.text.clone()))
        .collect();
    assert!(headings.contains(&"Question 4".to_string()), "{:?}", headings);
    assert!(!headings.iter().any(|h| h.contains("— Question 4")), "{:?}", headings);

    // A question that does have a title of its own keeps it.
    let mut s = run();
    s.add_question(
        workbench_core::protocol::Question::new(5, "Salad bar", "Compute p(T9).").unwrap(),
    );
    assert!(text_of(&pdf_of(&s, &Options::default())).contains("Salad bar"));
}

#[test]
fn code_is_set_in_courier_with_its_indentation_intact() {
    let mut s = run();
    s.add_question(
        workbench_core::protocol::Question::new(
            1,
            "CS1",
            "Complete it.\n```java\npublic int sum(int[] a) {\n    return 0;\n}\n```",
        )
        .unwrap(),
    );
    let out = render(&s, &Options::default());
    let mono: Vec<&Run> = out
        .pages
        .iter()
        .flat_map(|p| p.lines.iter().flat_map(|l| l.runs.iter()))
        .filter(|r| r.font == Font::Mono)
        .collect();
    assert!(mono.iter().any(|r| r.text == "    return 0;"), "{:?}", mono);
    assert!(mono.iter().any(|r| r.text.starts_with("public int sum")));
    // ...and the prose around it is not in Courier.
    assert!(!mono.iter().any(|r| r.text.contains("Complete it")));
}

// ------------------------------------------------------------------ layout

#[test]
fn no_line_runs_past_the_margin() {
    let mut s = resistant_run();
    s.add_question(question(4));
    s.add_question(question(12));
    for opts in [Options::default(), Options { mode: Mode::FullHistory, ..Options::default() }] {
        let out = render(&s, &opts);
        for page in &out.pages {
            for line in &page.lines {
                let w: f32 = line.runs.iter().map(|r| points(&r.text, r.font, line.size)).sum();
                assert!(
                    line.x + w <= PAGE_W - 40.0,
                    "a line reached {:.1} of {:.1}: {:?}",
                    line.x + w,
                    PAGE_W,
                    line.runs
                );
            }
        }
    }
}

#[test]
fn a_long_run_breaks_across_pages_and_numbers_them() {
    let mut s = run();
    for n in 4..=12 {
        s.add_question(question(n));
    }
    let out = render(&s, &Options::default());
    assert!(out.pages.len() > 1, "nine questions fitted on one page");
    let last = out.pages.last().unwrap();
    let numbered = last
        .lines
        .iter()
        .any(|l| l.runs.iter().any(|r| r.text == format!("{} of {}", out.pages.len(), out.pages.len())));
    assert!(numbered, "the last page is not numbered");
}

// ------------------------------------------------------------------ the modes

#[test]
fn final_mode_carries_the_final_question_and_not_the_history() {
    let s = resistant_run();
    let pdf = pdf_of(&s, &Options { mode: Mode::Final, ..Options::default() });
    let text = text_of(&pdf);

    assert!(text.contains("using only the grid drawn above"), "the resistant version is missing");
    assert!(!text.contains("v0"), "Final mode showed the version history");
    assert!(!text.contains("attempt 1"), "Final mode showed the attempt stamps");
    // It does say what the claim rests on.
    assert!(text.contains("One-Shot GenAI Resistant"));
    assert!(text.contains("gemini-2.5-flash"));
    assert!(text.contains("three attempts"));
    // And the rubric, which is the marking scheme the class is entitled to.
    assert!(text.contains("Proves the claim"));
}

/// "We perturbed this until the model failed" is a claim a reviewer should be
/// able to read rather than take on trust.
#[test]
fn full_history_carries_every_version_and_every_attempt() {
    let s = resistant_run();
    let pdf = pdf_of(&s, &Options { mode: Mode::FullHistory, ..Options::default() });
    let text = text_of(&pdf);

    assert!(text.contains("v0"));
    assert!(text.contains("v1"));
    assert!(text.contains("Spatial Injection"));
    assert!(text.contains("Prove that two plus two is four"), "the base version is missing");
    assert!(text.contains("using only the grid drawn above"));
    assert!(text.contains("attempt 1"));
    assert!(text.contains("attempt 3"));
    assert!(text.contains("Flesch-Kincaid grade"));
    // The digest a reader checks the query file against.
    let short = s.question(0).unwrap().latest().text_sha256().short().to_string();
    assert!(text.contains(&short), "the version digest is missing");
}

/// The ledger names the model's specific failures, which is a map of where to
/// push it. Off is the copy the class gets.
#[test]
fn the_ledger_can_be_left_out_of_the_student_copy() {
    let mut s = resistant_run();
    s.question_mut(0)
        .unwrap()
        .note(AttemptRef { version: 1, attempt: 1 }, "Invented a lemma about grids.")
        .unwrap();

    let with = text_of(&pdf_of(&s, &Options { ledger: true, ..Options::default() }));
    assert!(with.contains("Invented a lemma about grids"));
    assert!(with.contains("Observed hallucinations"));

    let without = text_of(&pdf_of(&s, &Options { ledger: false, ..Options::default() }));
    assert!(!without.contains("Invented a lemma"));
    assert!(!without.contains("Observed hallucinations"));
    // ...and the question itself is still there.
    assert!(without.contains("using only the grid drawn above"));
}

#[test]
fn a_penalty_chip_appears_in_the_rubric_with_its_sign() {
    let mut s = resistant_run();
    s.question_mut(0)
        .unwrap()
        .add_penalty("Asserted 2 + 2 = 5", -4, AttemptRef { version: 1, attempt: 1 }, at(40))
        .unwrap();
    let text = text_of(&pdf_of(&s, &Options::default()));
    assert!(text.contains("-4 Asserted 2 + 2 = 5"), "{}", text);
    assert!(text.contains("+8 Proves the claim"));
}

/// A run that has not finished still exports; the document says so rather than
/// implying a resistance nobody earned.
#[test]
fn an_unfinished_question_says_so() {
    let mut s = run();
    s.add_question(simple());
    stamp(&mut s, 0, 10, &[100.0, 0.0]);
    let text = text_of(&pdf_of(&s, &Options::default()));
    assert!(text.contains("In progress"));
    assert!(!text.contains("One-Shot GenAI Resistant \u{2014} failed"));
}

#[test]
fn the_title_is_the_instructors_own() {
    let s = resistant_run();
    let opts = Options { title: "CSC116 Lab 3 — Part 2".into(), ..Options::default() };
    assert!(text_of(&pdf_of(&s, &opts)).contains("CSC116 Lab 3"));
}

/// The writer places glyphs where it is told and nothing else.
#[test]
fn an_empty_document_is_still_a_valid_pdf() {
    let pdf = build(&[Page { lines: vec![], rules: vec![Rule { x0: 10.0, x1: 20.0, y: 30.0, grey: 0.5 }] }]);
    assert!(pdf.starts_with(b"%PDF-1.7"));
    assert!(contains(&pdf, b"/Type/Page"));
    assert!(contains(&pdf, b"trailer"));

    let pdf = build(&[]);
    assert!(contains(&pdf, b"/Count 0"));
    let _ = Line { runs: vec![Run::new("x", Font::Body)], x: 0.0, y: 0.0, size: 10.0, grey: 0.0 };
    let _ = Strategy::Spatial;
}
