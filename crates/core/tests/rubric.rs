//! Chips, the mastery scale they are graded on, and the file they travel in.

mod common;

use common::{base, pct, BANK};
use workbench_core::rubric::{
    AttemptRef, ChipId, DocChip, Rubric, RubricDoc, RubricError, Scale, Scores, RUBRIC_SCHEMA,
};

fn scored(marks: &[(&str, f64)]) -> Scores {
    marks.iter().map(|(id, v)| (ChipId::parse(id).unwrap(), pct(*v))).collect()
}

fn grade(r: &Rubric, marks: &[(&str, f64)]) -> Result<f64, RubricError> {
    r.current().grade(r.scale(), &scored(marks))
}

// ------------------------------------------------------------------ scoring

#[test]
fn a_percentage_is_the_weighted_sum_over_the_total() {
    let mut r = Rubric::blank();
    r.add_chip("States the claim", 2).unwrap();
    r.add_chip("Proves the claim", 8).unwrap();
    assert_eq!(r.current().total_points(), 10);

    assert_eq!(grade(&r, &[("states-the-claim", 100.0), ("proves-the-claim", 100.0)]), Ok(100.0));
    assert_eq!(grade(&r, &[("states-the-claim", 100.0), ("proves-the-claim", 0.0)]), Ok(20.0));
    assert_eq!(grade(&r, &[("states-the-claim", 0.0), ("proves-the-claim", 0.0)]), Ok(0.0));
    // Half credit on the eight-point chip is four points of ten.
    assert_eq!(grade(&r, &[("states-the-claim", 0.0), ("proves-the-claim", 50.0)]), Ok(40.0));
}

/// The nine rubrics of the study, as the prototype weighted them.
#[test]
fn the_study_rubrics_total_what_the_prototype_totalled() {
    for b in BANK {
        let mut r = Rubric::blank();
        for (label, points) in b.chips {
            r.add_chip(label, *points).unwrap();
        }
        let expect: i32 = b.chips.iter().map(|(_, p)| p).sum();
        assert_eq!(r.current().total_points(), expect, "Q{}", b.ordinal);
        assert_eq!(r.current().chips.len(), b.chips.len(), "Q{}", b.ordinal);
    }
    assert_eq!(base(4).chips.len(), 8);
    assert_eq!(base(12).chips.iter().map(|(_, p)| p).sum::<i32>(), 28);
}

/// Halves round up, not to even. A library that rounded to even would report
/// 6.2 here and quietly disagree with every figure in the study's record.
#[test]
fn percentages_round_the_way_the_prototype_rounded() {
    let mut r = Rubric::blank();
    r.add_chip("One", 1).unwrap();
    r.add_chip("Fifteen", 15).unwrap();
    // 1/16 is exactly 6.25%, so the tenths digit lands exactly on a half.
    assert_eq!(grade(&r, &[("one", 100.0), ("fifteen", 0.0)]), Ok(6.3));
}

/// Where JavaScript and Rust actually disagree: a negative half. JavaScript
/// rounds it up toward zero, Rust's `f64::round` away from zero. A rubric with
/// penalty chips can land here, so the port follows JavaScript.
#[test]
fn a_negative_half_rounds_the_javascript_way() {
    let mut r = Rubric::blank();
    r.add_chip("Everything", 16).unwrap();
    r.add_penalty("Invented a standard identity", -1, AttemptRef { version: 0, attempt: 1 }, common::at(0))
        .unwrap();
    // -1 of 16 is exactly -6.25%.
    let marks = scored(&[("everything", 0.0), ("invented-a-standard-identity", 100.0)]);
    assert_eq!(r.current().grade(r.scale(), &marks), Ok(-6.2));
}

#[test]
fn penalty_chips_subtract_without_inflating_the_denominator() {
    let mut r = Rubric::blank();
    r.add_chip("Correct count", 10).unwrap();
    let before = r.current().total_points();
    r.add_penalty("Hallucinated a formula", -5, AttemptRef { version: 1, attempt: 2 }, common::at(30))
        .unwrap();

    // The denominator is assessed points only: carrying more penalties must
    // not dilute the ones already there.
    assert_eq!(r.current().total_points(), before);
    assert_eq!(r.current().total_points(), 10);

    let all_right = scored(&[("correct-count", 100.0), ("hallucinated-a-formula", 0.0)]);
    assert_eq!(r.current().grade(r.scale(), &all_right), Ok(100.0));

    let caught = scored(&[("correct-count", 100.0), ("hallucinated-a-formula", 100.0)]);
    assert_eq!(r.current().grade(r.scale(), &caught), Ok(50.0));

    // Half the penalty is half the deduction.
    let partly = scored(&[("correct-count", 100.0), ("hallucinated-a-formula", 50.0)]);
    assert_eq!(r.current().grade(r.scale(), &partly), Ok(75.0));
}

/// A forgotten chip and a chip marked absent are different claims, and only
/// one of them is evidence. Defaulting the first to the second would let an
/// incomplete grading pass as a complete one.
#[test]
fn every_chip_must_be_scored() {
    let mut r = Rubric::blank();
    r.add_chip("First", 5).unwrap();
    r.add_chip("Second", 5).unwrap();

    assert_eq!(
        grade(&r, &[("first", 100.0)]),
        Err(RubricError::ScoreMissing(ChipId::parse("second").unwrap()))
    );
    assert_eq!(
        grade(&r, &[("first", 100.0), ("second", 0.0), ("third", 0.0)]),
        Err(RubricError::ScoreUnknownChip(ChipId::parse("third").unwrap()))
    );
}

// ------------------------------------------------------------------ the scale

/// The constraint the redirect asked for: possible points are set by the
/// scale, and that is enforced here rather than by whatever buttons the UI
/// happens to draw.
#[test]
fn a_score_must_be_a_level_of_the_scale() {
    let mut r = Rubric::blank();
    r.add_chip("Only chip", 4).unwrap();

    assert!(grade(&r, &[("only-chip", 50.0)]).is_ok());
    assert_eq!(
        grade(&r, &[("only-chip", 37.0)]),
        Err(RubricError::ScoreOffScale { chip: ChipId::parse("only-chip").unwrap(), given: 37.0 })
    );

    // Change the scale and the same mark becomes legal.
    r.set_scale(Scale::new("Thirds", vec![("None", 0.0), ("Some", 37.0), ("All", 100.0)]).unwrap())
        .unwrap();
    assert!(grade(&r, &[("only-chip", 37.0)]).is_ok());
    assert_eq!(
        grade(&r, &[("only-chip", 50.0)]),
        Err(RubricError::ScoreOffScale { chip: ChipId::parse("only-chip").unwrap(), given: 50.0 })
    );
}

#[test]
fn the_built_in_scales_are_valid_and_the_default_is_the_prototypes() {
    Scale::partial_credit().validate().unwrap();
    Scale::mastery().validate().unwrap();
    assert_eq!(Scale::default(), Scale::partial_credit());
    assert_eq!(Scale::mastery().levels.len(), 4);
    assert!(Scale::partial_credit().admits(pct(50.0)));
    assert!(!Scale::partial_credit().admits(pct(51.0)));
}

#[test]
fn a_scale_must_run_from_nothing_to_everything() {
    assert_eq!(Scale::new("One rung", vec![("All", 100.0)]), Err(RubricError::ScaleTooSmall));
    assert_eq!(
        Scale::new("Backwards", vec![("All", 100.0), ("None", 0.0)]),
        Err(RubricError::ScaleNotAscending)
    );
    assert_eq!(
        Scale::new("Tied", vec![("None", 0.0), ("Same", 50.0), ("Also", 50.0), ("All", 100.0)]),
        Err(RubricError::ScaleNotAscending)
    );
    // No zero: nothing could ever be marked absent.
    assert_eq!(
        Scale::new("Generous", vec![("Some", 25.0), ("All", 100.0)]),
        Err(RubricError::ScaleMissingZero)
    );
    // No full: a chip's stated points are unreachable and a 60% threshold
    // silently becomes a 75% one.
    assert_eq!(
        Scale::new("Capped", vec![("None", 0.0), ("Most", 80.0)]),
        Err(RubricError::ScaleMissingFull)
    );
    assert_eq!(Scale::new("Blank", vec![("", 0.0), ("All", 100.0)]), Err(RubricError::EmptyLabel));
}

#[test]
fn a_chip_can_say_what_it_is_worth_at_each_level() {
    let mut r = Rubric::blank();
    let id = r.add_chip("Worth four", 4).unwrap();
    let scale = Scale::mastery();
    let chip = r.current().chip(&id).unwrap();
    let worth: Vec<f64> = scale.levels.iter().map(|l| chip.points_at(l)).collect();
    assert_eq!(worth[0], 0.0);
    assert_eq!(worth[3], 4.0);
    assert!(worth[1] > 0.0 && worth[1] < worth[2]);
}

// ------------------------------------------------------------------ chips

#[test]
fn a_chip_must_be_worth_something_and_a_penalty_must_cost_something() {
    let mut r = Rubric::blank();
    assert_eq!(r.add_chip("Free", 0), Err(RubricError::AssessedMustBePositive));
    assert_eq!(r.add_chip("Negative", -2), Err(RubricError::AssessedMustBePositive));
    assert_eq!(r.add_chip("  ", 3), Err(RubricError::EmptyLabel));

    let from = AttemptRef { version: 0, attempt: 1 };
    assert_eq!(
        r.add_penalty("Bonus", 3, from, common::at(0)),
        Err(RubricError::PenaltyMustBeNegative)
    );
    assert_eq!(
        r.add_penalty("Neutral", 0, from, common::at(0)),
        Err(RubricError::PenaltyMustBeNegative)
    );
}

/// Ids are the join key a knowledge-component graph will attach to, so two
/// chips typed with the same words must not become one chip with two scores.
#[test]
fn identical_labels_get_distinct_ids() {
    let mut r = Rubric::blank();
    let a = r.add_chip("Cites the rule", 2).unwrap();
    let b = r.add_chip("Cites the rule", 2).unwrap();
    let c = r.add_chip("Cites the rule", 2).unwrap();
    assert_eq!(a.as_str(), "cites-the-rule");
    assert_eq!(b.as_str(), "cites-the-rule-2");
    assert_eq!(c.as_str(), "cites-the-rule-3");

    // A label with nothing sluggable in it still gets an id.
    let odd = r.add_chip("≤ ≥ ≠", 1).unwrap();
    assert_eq!(odd.as_str(), "chip");
}

/// Renaming a chip must not re-point the attempts already graded against it.
#[test]
fn editing_a_chip_does_not_move_its_id() {
    let mut r = Rubric::blank();
    let id = r.add_chip("Bijection stated", 2).unwrap();
    r.edit_chip(&id, "Bijection stated and justified", 4).unwrap();
    let chip = r.current().chip(&id).unwrap();
    assert_eq!(chip.id, id);
    assert_eq!(chip.label, "Bijection stated and justified");
    assert_eq!(chip.points, 4);

    r.remove_chip(&id).unwrap();
    assert_eq!(r.remove_chip(&id), Err(RubricError::NoSuchChip(id)));
}

#[test]
fn chips_can_be_reordered_without_losing_their_scores() {
    let mut r = Rubric::blank();
    let a = r.add_chip("First", 1).unwrap();
    let b = r.add_chip("Second", 2).unwrap();
    let c = r.add_chip("Third", 3).unwrap();
    r.reorder(&[c.clone(), a.clone(), b.clone()]).unwrap();
    let order: Vec<&str> = r.current().chips.iter().map(|x| x.id.as_str()).collect();
    assert_eq!(order, ["third", "first", "second"]);

    // A score is a map from id, so it survives the move untouched.
    // 1 + 0 + 1.5 of 6 points.
    let marks = scored(&[("first", 100.0), ("second", 0.0), ("third", 50.0)]);
    assert_eq!(r.current().grade(r.scale(), &marks), Ok(41.7));
}

// ------------------------------------------------------------------ portability

#[test]
fn a_rubric_survives_a_round_trip_through_its_file() {
    let mut r = Rubric::blank();
    r.set_scale(Scale::mastery()).unwrap();
    for (label, points) in base(6).chips {
        r.add_chip(label, *points).unwrap();
    }

    let doc = r.export();
    let json = serde_json::to_string_pretty(&doc).unwrap();
    let back: RubricDoc = serde_json::from_str(&json).unwrap();

    let mut fresh = Rubric::blank();
    fresh.import(&back).unwrap();
    assert_eq!(fresh.scale(), r.scale());
    assert_eq!(fresh.current().chips, r.current().chips);
    assert_eq!(fresh.current().total_points(), 16);
}

/// A penalty points at an attempt in the run that produced it, and that
/// pointer means nothing anywhere else. It is left behind structurally:
/// `DocChip` has no field that could carry one.
#[test]
fn a_penalty_cannot_travel_between_runs() {
    let mut r = Rubric::blank();
    r.add_chip("Correct count", 10).unwrap();
    r.add_penalty("Invented P(7,6)", -4, AttemptRef { version: 1, attempt: 3 }, common::at(60))
        .unwrap();

    let doc = r.export();
    assert_eq!(doc.chips.len(), 1);
    assert_eq!(doc.chips[0].label, "Correct count");

    let json = serde_json::to_string(&doc).unwrap();
    assert!(!json.contains("Invented"), "{}", json);
    assert!(!json.contains("kind"), "{}", json);
    assert!(!json.contains("from"), "{}", json);
}

/// A rubric file written to a later, graph-shaped format must fail loudly
/// rather than be read as much of as this build understands.
#[test]
fn a_rubric_file_from_another_format_is_refused() {
    let mut r = Rubric::blank();
    let doc = RubricDoc {
        schema: "perturbation-workbench-rubric/2".into(),
        scale: Scale::partial_credit(),
        chips: vec![DocChip { id: None, label: "Anything".into(), points: 1 }],
    };
    assert_eq!(
        r.import(&doc),
        Err(RubricError::UnknownSchema("perturbation-workbench-rubric/2".into()))
    );

    // An unknown field is a different format wearing this one's name.
    let sneaky = format!(
        r#"{{"schema":"{}","scale":{},"chips":[{{"label":"A","points":1}}],"knowledgeComponent":"counting"}}"#,
        RUBRIC_SCHEMA,
        serde_json::to_string(&Scale::partial_credit()).unwrap()
    );
    assert!(serde_json::from_str::<RubricDoc>(&sneaky).is_err());
}

#[test]
fn an_imported_rubric_is_validated_not_trusted() {
    let mut r = Rubric::blank();
    let doc = |chips: Vec<DocChip>| RubricDoc {
        schema: RUBRIC_SCHEMA.into(),
        scale: Scale::partial_credit(),
        chips,
    };
    assert_eq!(r.import(&doc(vec![])), Err(RubricError::Empty));
    assert_eq!(
        r.import(&doc(vec![DocChip { id: None, label: "Free".into(), points: 0 }])),
        Err(RubricError::AssessedMustBePositive)
    );
    assert_eq!(
        r.import(&doc(vec![DocChip { id: None, label: " ".into(), points: 1 }])),
        Err(RubricError::EmptyLabel)
    );
    let dup = ChipId::parse("same").unwrap();
    assert_eq!(
        r.import(&doc(vec![
            DocChip { id: Some(dup.clone()), label: "A".into(), points: 1 },
            DocChip { id: Some(dup.clone()), label: "B".into(), points: 1 },
        ])),
        Err(RubricError::DuplicateChip(dup))
    );
    // A hand-written file may omit ids; they are derived from the labels.
    r.import(&doc(vec![
        DocChip { id: None, label: "Cites the law".into(), points: 2 },
        DocChip { id: None, label: "Cites the law".into(), points: 2 },
    ]))
    .unwrap();
    let ids: Vec<&str> = r.current().chips.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, ["cites-the-law", "cites-the-law-2"]);
}
