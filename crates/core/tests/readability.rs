//! The readability port, held to the prototype's numbers.

mod common;

use common::BANK;
use workbench_core::readability::{analyze, guard, syllables, Limits};

/// The heuristic is idiosyncratic - "comes" is one syllable to it and two to a
/// dictionary. That is fine and it is the point: this table pins *the
/// prototype's* idiosyncrasies, because a drift measurement is only meaningful
/// against a fixed ruler.
#[test]
fn syllable_counts_match_the_prototype() {
    for (word, expect) in [
        ("a", 1), ("the", 1), ("cat", 1), ("cats", 1), ("comes", 1), ("hoped", 1),
        ("named", 1), ("queue", 2), ("idea", 2), ("bijection", 3), ("probability", 5),
        ("axiomatic", 4), ("yellow", 2), ("yes", 1), ("rhythm", 1), ("strengths", 1),
        ("equation", 3), ("sundaes", 2), ("modulo", 3), ("pigeonhole", 4),
        ("Euclidean", 3), ("congruent", 2), ("independent", 4), ("ceil", 1),
    ] {
        assert_eq!(syllables(word), expect, "{:?}", word);
    }
}

/// The load-bearing test of this module. If these nine numbers move, a figure
/// in the study's record and the same figure in the tool have stopped meaning
/// the same thing.
#[test]
fn the_study_base_texts_score_what_the_prototype_scored() {
    for b in BANK {
        let m = analyze(b.text);
        assert_eq!(m.words, b.words, "Q{} words", b.ordinal);
        assert_eq!(m.sentences, b.sentences, "Q{} sentences", b.ordinal);
        assert_eq!(m.grade, b.grade, "Q{} grade", b.ordinal);
        assert_eq!(m.ease, b.ease, "Q{} ease", b.ordinal);
    }
}

#[test]
fn an_empty_text_is_no_words_and_one_sentence() {
    for empty in ["", "   ", "\n\n\t"] {
        let m = analyze(empty);
        assert_eq!((m.words, m.sentences, m.grade, m.ease), (0, 1, 0.0, 0.0));
    }
}

/// A heading or a bare list marker is not a sentence. Counting them would
/// shrink the words-per-sentence figure and flatter every question that is
/// laid out in parts - which is all of them.
#[test]
fn a_one_word_line_is_not_a_sentence() {
    let m = analyze("Overview\nThe tray holds eight desserts in total.\n1.\n");
    assert_eq!(m.sentences, 1);
}

#[test]
fn math_is_stripped_before_anything_is_counted() {
    // `$...$` collapses to one word however long it was.
    let with = analyze("Compute $\\sum_{i=0}^{n} \\binom{n}{i} x^i$ and explain the result.");
    let without = analyze("Compute equation and explain the result.");
    assert_eq!(with.words, without.words);
    assert_eq!(with.grade, without.grade);

    // Arrows become spaces rather than leaving a hyphen glued to a word.
    assert_eq!(analyze("Map a -> b now.").words, analyze("Map a b now.").words);
    for op in ["=>", "<=", ">=", "!="] {
        assert_eq!(analyze(&format!("Let x {} y here.", op)).words, 4, "{}", op);
    }

    // A bare number is one syllable however many digits it carries.
    assert_eq!(analyze("The count is 3628800 exactly.").grade, analyze("The count is one exactly.").grade);
}

#[test]
fn an_unclosed_dollar_is_left_alone() {
    // The prototype's regex needs a closing delimiter, so a stray `$` is just
    // a character. Diverging here would silently change a question's metrics.
    let m = analyze("The price is $5 for the tray.");
    assert_eq!(m.words, 7);
}

#[test]
fn the_guard_notices_drift_a_cap_and_growth_separately() {
    let limits = Limits::default();
    let base = analyze(BANK[0].text);

    let clean = guard(&base, &base, &limits);
    assert!(!clean.tripped(), "a text against itself cannot drift: {:?}", clean);
    assert_eq!(clean.growth, 0.0);

    // Long sentences and long words: grade climbs.
    let dense = analyze(
        "Notwithstanding the aforementioned considerations regarding the intrinsically \
         multidimensional characterisation of combinatorial enumeration, demonstrate \
         comprehensively the corresponding equivalences whilst simultaneously \
         substantiating every intermediary justification exhaustively and unambiguously.",
    );
    let drifted = guard(&base, &dense, &limits);
    assert!(drifted.drifted, "{:?}", drifted);
    assert!(drifted.over_cap, "{:?}", drifted);

    // Growth is measured in words against the base, and is reported even when
    // the reading level is unchanged.
    let padded = format!("{}\n{}", BANK[0].text, BANK[0].text);
    let grown = guard(&base, &analyze(&padded), &limits);
    assert!(grown.overgrown, "{:?}", grown);
    assert_eq!(grown.growth, 100.0);

    // The limits are the caller's, not the module's.
    let generous = Limits { fk_drift: 40.0, fk_cap: 99.0, growth_cap: 400.0 };
    assert!(!guard(&base, &dense, &generous).tripped());
}

/// Every field of the report is advisory. Nothing in the guard can refuse a
/// save, because the instructor is the one who knows whether a longer question
/// is a worse question.
/// The page reads this report as JSON, and a method serialises to nothing —
/// which reads as `false` in JavaScript and silently hides the advisory.
#[test]
fn the_guard_report_carries_its_verdict_into_json() {
    let base = analyze("Add two numbers.");
    let long = analyze(&format!("Add two numbers. {}", "Explain every step fully. ".repeat(30)));
    let r = guard(&base, &long, &Limits::default());
    assert!(r.tripped());

    let json = serde_json::to_string(&r).unwrap();
    assert!(json.contains("\"tripped\":true"), "{}", json);
    assert!(json.contains("\"overgrown\":true"), "{}", json);
    assert!(serde_json::to_string(&guard(&base, &base, &Limits::default()))
        .unwrap()
        .contains("\"tripped\":false"));
}

#[test]
fn the_guard_reports_the_numbers_it_judged_on() {
    let base = analyze("Add two numbers.");
    let other = analyze("Add two numbers and then explain what you did in one sentence.");
    let r = guard(&base, &other, &Limits::default());
    assert_eq!(r.base_grade, base.grade);
    assert_eq!(r.grade, other.grade);
}
