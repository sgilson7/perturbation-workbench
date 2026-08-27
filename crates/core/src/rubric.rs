//! Rubrics as chips, scored on a mastery scale.
//!
//! A rubric here is a flat set of **chips**: one chip is one atomistic thing
//! the answer either demonstrates or does not. Chips carry point weights, and
//! what fractions of those weights are attainable is fixed by a **scale** —
//! the mastery levels the whole rubric is graded on. Both are authored by the
//! instructor; nothing in this module invents a chip.
//!
//! Two structural choices exist to keep this replaceable. A later version of
//! this tool is meant to derive chips from a knowledge-component graph rather
//! than from typing, and that change should not invalidate a single recorded
//! attempt:
//!
//! * **Chips are identified, not numbered.** A recorded score is a map from
//!   `ChipId` to a percentage, never a positional array, so chips can be added,
//!   retired, or reordered without silently re-pointing history. The id is the
//!   join key a knowledge-component graph will attach to.
//! * **The import format carries its schema.** `perturbation-workbench-rubric/1`
//!   is checked on the way in and unknown fields are refused, so a rubric file
//!   written for a later, graph-shaped format fails loudly here instead of
//!   being read halfway and graded against.
//!
//! Scores are percentages rather than an enum of named levels because the
//! scale is the instructor's to define: a three-level rubric and a five-level
//! one have to be the same type. The scale is still enforced — a percentage
//! that is not one of its levels is refused — so "possible points are set by
//! the scale" is a property of the core rather than a habit of the UI.

use std::collections::BTreeMap;

use crate::hash::Timestamp;
use crate::readability::js_round;

/// Where a rubric edit or a grade went wrong.
#[derive(Debug, Clone, PartialEq)]
pub enum RubricError {
    /// A scale needs at least two levels to be a scale.
    ScaleTooSmall,
    /// Levels must be ordered, lowest credit first, with no ties.
    ScaleNotAscending,
    /// No level scores zero, so nothing could ever be marked absent and every
    /// answer would collect points for turning up.
    ScaleMissingZero,
    /// No level scores full, so a chip's stated points are unreachable and the
    /// threshold quietly means something other than what it says.
    ScaleMissingFull,
    /// A label the reader has to distinguish from another identical one.
    DuplicateLabel,
    EmptyLabel,
    /// The rubric was frozen by the first attempt stamped against it.
    Frozen,
    /// A rubric with no chips cannot grade anything.
    Empty,
    DuplicateChip(ChipId),
    NoSuchChip(ChipId),
    NoSuchRevision(usize),
    /// An assessed chip must be worth something.
    AssessedMustBePositive,
    /// A penalty chip must subtract.
    PenaltyMustBeNegative,
    /// A chip in this revision was not scored.
    ScoreMissing(ChipId),
    /// A score was given for a chip this revision does not contain.
    ScoreUnknownChip(ChipId),
    /// A score that is not one of the scale's levels.
    ScoreOffScale { chip: ChipId, given: f64 },
    /// Not a percentage: negative, above 100, or not a number.
    NotAPercentage(f64),
    /// A rubric file written to a format this build does not know.
    UnknownSchema(String),
}

// ---------------------------------------------------------------- percentage

/// A percentage between 0 and 100 that has been checked to be one.
///
/// The type exists so that "score" cannot arrive as `-3`, `1.0` meaning full
/// credit, or a NaN that would make every later comparison quietly false.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, serde::Serialize)]
#[serde(transparent)]
pub struct Percent(f64);

impl Percent {
    pub fn new(v: f64) -> Result<Percent, RubricError> {
        if v.is_finite() && (0.0..=100.0).contains(&v) {
            Ok(Percent(v))
        } else {
            Err(RubricError::NotAPercentage(v))
        }
    }

    pub const ZERO: Percent = Percent(0.0);
    pub const FULL: Percent = Percent(100.0);

    pub fn get(self) -> f64 {
        self.0
    }

    /// The fraction this percentage represents, for arithmetic.
    pub fn fraction(self) -> f64 {
        self.0 / 100.0
    }
}

impl<'de> serde::Deserialize<'de> for Percent {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = f64::deserialize(d)?;
        Percent::new(v).map_err(|_| serde::de::Error::custom("not a percentage between 0 and 100"))
    }
}

// ---------------------------------------------------------------- the scale

/// One rung of a mastery scale: what it is called, and what it earns.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Level {
    pub label: String,
    pub credit: Percent,
}

/// The levels every chip in a rubric is scored on.
///
/// Endpoints are required to be 0 and 100. Without a zero there is no way to
/// record that something was absent; without a full there is no way to earn a
/// chip's stated points, and a 60% threshold applied to a rubric whose ceiling
/// is 80% is a 75% threshold wearing a disguise. Both would be silent.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scale {
    pub name: String,
    pub levels: Vec<Level>,
}

impl Scale {
    pub fn new(name: &str, levels: Vec<(&str, f64)>) -> Result<Scale, RubricError> {
        let mut built = Vec::with_capacity(levels.len());
        for (label, credit) in levels {
            if label.trim().is_empty() {
                return Err(RubricError::EmptyLabel);
            }
            built.push(Level { label: label.to_string(), credit: Percent::new(credit)? });
        }
        let scale = Scale { name: name.to_string(), levels: built };
        scale.validate()?;
        Ok(scale)
    }

    pub fn validate(&self) -> Result<(), RubricError> {
        if self.levels.len() < 2 {
            return Err(RubricError::ScaleTooSmall);
        }
        for pair in self.levels.windows(2) {
            if pair[1].credit.get() <= pair[0].credit.get() {
                return Err(RubricError::ScaleNotAscending);
            }
            if pair[0].label.trim().eq_ignore_ascii_case(pair[1].label.trim()) {
                return Err(RubricError::DuplicateLabel);
            }
        }
        if self.levels.iter().any(|l| l.label.trim().is_empty()) {
            return Err(RubricError::EmptyLabel);
        }
        if self.levels.first().map(|l| l.credit.get()) != Some(0.0) {
            return Err(RubricError::ScaleMissingZero);
        }
        if self.levels.last().map(|l| l.credit.get()) != Some(100.0) {
            return Err(RubricError::ScaleMissingFull);
        }
        Ok(())
    }

    /// Is this percentage one of the scale's levels?
    pub fn admits(&self, p: Percent) -> bool {
        self.levels.iter().any(|l| l.credit == p)
    }

    /// The prototype's scale, and the default: nothing, half, all. Keeping it
    /// as the default is what makes this tool's percentages comparable with the
    /// numbers recorded during the study.
    pub fn partial_credit() -> Scale {
        Scale::new("Partial credit", vec![("Not shown", 0.0), ("Partial", 50.0), ("Complete", 100.0)])
            .expect("built-in scale is valid")
    }

    /// A four-rung mastery scale, for rubrics that want more resolution than
    /// half credit gives.
    pub fn mastery() -> Scale {
        Scale::new(
            "Mastery",
            vec![
                ("No evidence", 0.0),
                ("Emerging", 33.0),
                ("Developing", 67.0),
                ("Mastered", 100.0),
            ],
        )
        .expect("built-in scale is valid")
    }
}

impl Default for Scale {
    fn default() -> Self {
        Scale::partial_credit()
    }
}

// ---------------------------------------------------------------- chips

/// A chip's stable name.
///
/// Derived from the label once, then never again: renaming a chip must not
/// re-point the attempts already graded against it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(transparent)]
pub struct ChipId(String);

impl ChipId {
    /// A slug for `label` that does not collide with `taken`.
    ///
    /// Generated in the core rather than the UI so that two chips typed in two
    /// browser tabs cannot end up sharing an id and merging their scores.
    pub fn derive(label: &str, taken: &[ChipId]) -> ChipId {
        let mut slug = String::new();
        let mut gap = false;
        for c in label.chars() {
            if c.is_ascii_alphanumeric() {
                if gap && !slug.is_empty() {
                    slug.push('-');
                }
                gap = false;
                slug.push(c.to_ascii_lowercase());
            } else {
                gap = true;
            }
            if slug.len() >= 48 {
                break;
            }
        }
        if slug.is_empty() {
            slug.push_str("chip");
        }
        let mut candidate = ChipId(slug.clone());
        let mut n = 2;
        while taken.contains(&candidate) {
            candidate = ChipId(format!("{}-{}", slug, n));
            n += 1;
        }
        candidate
    }

    pub fn parse(s: &str) -> Result<ChipId, RubricError> {
        let ok = !s.is_empty()
            && s.len() <= 64
            && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if ok {
            Ok(ChipId(s.to_string()))
        } else {
            Err(RubricError::NoSuchChip(ChipId(s.to_string())))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> serde::Deserialize<'de> for ChipId {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        ChipId::parse(&s).map_err(|_| serde::de::Error::custom("not a chip id"))
    }
}

/// Which attempt a penalty chip was written in response to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AttemptRef {
    pub version: usize,
    pub attempt: usize,
}

/// What a chip is for.
/// Serialised as `"assessed"` or `{"penalty": {...}}` rather than flattened
/// into the chip: flattening is silently incompatible with
/// `deny_unknown_fields`, and losing that check on the one structure a
/// hand-edited session file would most want to add a field to is not a trade
/// worth making for tidier JSON.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChipKind {
    /// Something the answer is meant to demonstrate.
    Assessed,
    /// Something the answer is penalised for doing — a hallucination the run
    /// actually observed. It carries a pointer to the attempt that motivated
    /// it, so a reader can ask *why is this here* and get an answer, which is
    /// the whole difference between a rubric tuned to the evidence and a rubric
    /// tuned until the model fails.
    Penalty { from: AttemptRef },
}

/// One atomistic rubric criterion.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Chip {
    pub id: ChipId,
    pub label: String,
    /// Positive for an assessed chip, negative for a penalty.
    pub points: i32,
    pub kind: ChipKind,
}

impl Chip {
    /// What this chip is worth at a given level, for display.
    pub fn points_at(&self, level: &Level) -> f64 {
        self.points as f64 * level.credit.fraction()
    }

    pub fn is_penalty(&self) -> bool {
        matches!(self.kind, ChipKind::Penalty { .. })
    }
}

// ---------------------------------------------------------------- scoring

/// One attempt's marks: a percentage per chip, by id.
pub type Scores = BTreeMap<ChipId, Percent>;

// ---------------------------------------------------------------- revisions

/// The rubric as it stood when some set of attempts was graded.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Revision {
    pub ordinal: usize,
    /// When this revision came into force.
    ///
    /// `None` only before the rubric is frozen, i.e. while it is still being
    /// authored and no attempt has been graded against it. The moment the first
    /// attempt is stamped the initial revision takes that attempt's timestamp,
    /// so from then on every revision has one and "was this attempt graded
    /// against a rubric that existed yet" is answerable.
    pub at: Option<Timestamp>,
    pub chips: Vec<Chip>,
}

impl Revision {
    /// The denominator: assessed points only.
    ///
    /// Penalty chips are excluded deliberately. Adding them would let a rubric
    /// dilute its own penalties by carrying more of them, and would make the
    /// same answer score differently for a reason unrelated to the answer.
    pub fn total_points(&self) -> i32 {
        self.chips.iter().filter(|c| !c.is_penalty()).map(|c| c.points).sum()
    }

    pub fn chip(&self, id: &ChipId) -> Option<&Chip> {
        self.chips.iter().find(|c| &c.id == id)
    }

    pub fn assessed(&self) -> impl Iterator<Item = &Chip> {
        self.chips.iter().filter(|c| !c.is_penalty())
    }

    pub fn penalties(&self) -> impl Iterator<Item = &Chip> {
        self.chips.iter().filter(|c| c.is_penalty())
    }

    /// Grade one attempt against this revision.
    ///
    /// Every chip must be scored and every score must be a level of `scale`:
    /// a missing chip is refused rather than defaulted to zero, because a
    /// forgotten chip and a chip marked absent are different claims and only
    /// one of them is evidence.
    pub fn grade(&self, scale: &Scale, scores: &Scores) -> Result<f64, RubricError> {
        for id in scores.keys() {
            if self.chip(id).is_none() {
                return Err(RubricError::ScoreUnknownChip(id.clone()));
            }
        }
        let mut earned = 0.0;
        for chip in &self.chips {
            let p = *scores.get(&chip.id).ok_or_else(|| RubricError::ScoreMissing(chip.id.clone()))?;
            if !scale.admits(p) {
                return Err(RubricError::ScoreOffScale { chip: chip.id.clone(), given: p.get() });
            }
            earned += chip.points as f64 * p.fraction();
        }
        let total = self.total_points();
        if total <= 0 {
            return Err(RubricError::Empty);
        }
        // One decimal, the way the prototype rounded, so a percentage recorded
        // during the study and one recorded here are the same number.
        Ok(js_round(earned / total as f64 * 1000.0) / 10.0)
    }
}

// ---------------------------------------------------------------- the rubric

/// A question's rubric and everything it has been.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Rubric {
    scale: Scale,
    revisions: Vec<Revision>,
}

impl Rubric {
    /// An empty rubric on the default scale, ready to be built up chip by chip.
    pub fn blank() -> Rubric {
        Rubric {
            scale: Scale::partial_credit(),
            revisions: vec![Revision { ordinal: 1, at: None, chips: Vec::new() }],
        }
    }

    pub fn scale(&self) -> &Scale {
        &self.scale
    }

    pub fn revisions(&self) -> &[Revision] {
        &self.revisions
    }

    pub fn current(&self) -> &Revision {
        self.revisions.last().expect("a rubric always has at least one revision")
    }

    pub fn revision(&self, ordinal: usize) -> Result<&Revision, RubricError> {
        self.revisions
            .iter()
            .find(|r| r.ordinal == ordinal)
            .ok_or(RubricError::NoSuchRevision(ordinal))
    }

    /// True once an attempt has been graded against this rubric.
    pub fn frozen(&self) -> bool {
        self.revisions[0].at.is_some()
    }

    /// Record that grading has begun. Called by the protocol when the first
    /// attempt on a question is stamped; the initial revision takes that
    /// attempt's timestamp, and from then on the rubric may only be extended.
    pub(crate) fn freeze(&mut self, at: Timestamp) {
        if self.revisions[0].at.is_none() {
            self.revisions[0].at = Some(at);
        }
    }

    fn editable(&mut self) -> Result<&mut Revision, RubricError> {
        if self.frozen() {
            return Err(RubricError::Frozen);
        }
        Ok(self.revisions.last_mut().expect("at least one revision"))
    }

    pub fn set_scale(&mut self, scale: Scale) -> Result<(), RubricError> {
        scale.validate()?;
        if self.frozen() {
            // Changing the scale after grading would re-interpret scores that
            // were recorded against the old one, and might leave them off the
            // new scale entirely.
            return Err(RubricError::Frozen);
        }
        self.scale = scale;
        Ok(())
    }

    /// Add a chip, returning the id it was given.
    pub fn add_chip(&mut self, label: &str, points: i32) -> Result<ChipId, RubricError> {
        if label.trim().is_empty() {
            return Err(RubricError::EmptyLabel);
        }
        if points <= 0 {
            return Err(RubricError::AssessedMustBePositive);
        }
        let rev = self.editable()?;
        let taken: Vec<ChipId> = rev.chips.iter().map(|c| c.id.clone()).collect();
        let id = ChipId::derive(label, &taken);
        rev.chips.push(Chip {
            id: id.clone(),
            label: label.trim().to_string(),
            points,
            kind: ChipKind::Assessed,
        });
        Ok(id)
    }

    pub fn edit_chip(&mut self, id: &ChipId, label: &str, points: i32) -> Result<(), RubricError> {
        if label.trim().is_empty() {
            return Err(RubricError::EmptyLabel);
        }
        if points <= 0 {
            return Err(RubricError::AssessedMustBePositive);
        }
        let rev = self.editable()?;
        let chip = rev
            .chips
            .iter_mut()
            .find(|c| &c.id == id)
            .ok_or_else(|| RubricError::NoSuchChip(id.clone()))?;
        chip.label = label.trim().to_string();
        chip.points = points;
        Ok(())
    }

    pub fn remove_chip(&mut self, id: &ChipId) -> Result<(), RubricError> {
        let rev = self.editable()?;
        let before = rev.chips.len();
        rev.chips.retain(|c| &c.id != id);
        if rev.chips.len() == before {
            return Err(RubricError::NoSuchChip(id.clone()));
        }
        Ok(())
    }

    pub fn reorder(&mut self, order: &[ChipId]) -> Result<(), RubricError> {
        let rev = self.editable()?;
        if order.len() != rev.chips.len() {
            return Err(RubricError::Empty);
        }
        let mut moved = Vec::with_capacity(order.len());
        for id in order {
            let at = rev
                .chips
                .iter()
                .position(|c| &c.id == id)
                .ok_or_else(|| RubricError::NoSuchChip(id.clone()))?;
            moved.push(rev.chips.remove(at));
        }
        rev.chips = moved;
        Ok(())
    }

    /// Append a penalty chip as a new revision (Step 6).
    ///
    /// This is the one edit a frozen rubric accepts, because it is the one the
    /// protocol asks for: the ledger records what the model hallucinated, and
    /// the rubric grows a chip that penalises it. Appending as a *new revision*
    /// rather than editing the current one is what keeps the earlier attempts
    /// graded by the rubric they were actually graded by.
    pub fn add_penalty(
        &mut self,
        label: &str,
        points: i32,
        from: AttemptRef,
        at: Timestamp,
    ) -> Result<usize, RubricError> {
        if label.trim().is_empty() {
            return Err(RubricError::EmptyLabel);
        }
        if points >= 0 {
            return Err(RubricError::PenaltyMustBeNegative);
        }
        let taken: Vec<ChipId> = self.current().chips.iter().map(|c| c.id.clone()).collect();
        let id = ChipId::derive(label, &taken);
        let mut chips = self.current().chips.clone();
        chips.push(Chip {
            id,
            label: label.trim().to_string(),
            points,
            kind: ChipKind::Penalty { from },
        });
        let ordinal = self.revisions.len() + 1;
        self.revisions.push(Revision { ordinal, at: Some(at), chips });
        Ok(ordinal)
    }

    /// Do any of this rubric's penalty chips descend from this attempt?
    pub fn penalties_from(&self, at: AttemptRef) -> usize {
        self.current()
            .penalties()
            .filter(|c| matches!(c.kind, ChipKind::Penalty { from } if from == at))
            .count()
    }

    // ------------------------------------------------------------ portability

    /// Take a rubric out to reuse on another question or in another run.
    ///
    /// Penalty chips are left behind, and not by filtering: `RubricDoc` has no
    /// way to represent one. A penalty points at an attempt in the run that
    /// produced it, and that pointer means nothing anywhere else — an imported
    /// penalty would be a claim about evidence that does not exist.
    pub fn export(&self) -> RubricDoc {
        RubricDoc {
            schema: RUBRIC_SCHEMA.to_string(),
            scale: self.scale.clone(),
            chips: self
                .current()
                .assessed()
                .map(|c| DocChip { id: Some(c.id.clone()), label: c.label.clone(), points: c.points })
                .collect(),
        }
    }

    /// Replace this rubric's scale and chips with an imported set.
    pub fn import(&mut self, doc: &RubricDoc) -> Result<(), RubricError> {
        if doc.schema != RUBRIC_SCHEMA {
            return Err(RubricError::UnknownSchema(doc.schema.clone()));
        }
        doc.scale.validate()?;
        if self.frozen() {
            return Err(RubricError::Frozen);
        }
        if doc.chips.is_empty() {
            return Err(RubricError::Empty);
        }
        let mut chips: Vec<Chip> = Vec::with_capacity(doc.chips.len());
        for dc in &doc.chips {
            if dc.label.trim().is_empty() {
                return Err(RubricError::EmptyLabel);
            }
            if dc.points <= 0 {
                return Err(RubricError::AssessedMustBePositive);
            }
            let taken: Vec<ChipId> = chips.iter().map(|c| c.id.clone()).collect();
            let id = match &dc.id {
                Some(id) if taken.contains(id) => return Err(RubricError::DuplicateChip(id.clone())),
                Some(id) => id.clone(),
                None => ChipId::derive(&dc.label, &taken),
            };
            chips.push(Chip {
                id,
                label: dc.label.trim().to_string(),
                points: dc.points,
                kind: ChipKind::Assessed,
            });
        }
        self.scale = doc.scale.clone();
        self.revisions = vec![Revision { ordinal: 1, at: None, chips }];
        Ok(())
    }
}

impl Default for Rubric {
    fn default() -> Self {
        Rubric::blank()
    }
}

/// The identifier at the top of every exported rubric file.
///
/// Bumped, not extended, when the chip model changes. A file that names a
/// schema this build does not know is refused rather than read as much of as
/// possible, so a rubric authored against a knowledge-component graph cannot be
/// half-understood by a build that predates it.
pub const RUBRIC_SCHEMA: &str = "perturbation-workbench-rubric/1";

/// A chip as it travels between runs: no provenance, no kind, no history.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DocChip {
    /// Kept if present so a round trip preserves ids; derived from the label
    /// when a rubric was written by hand.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<ChipId>,
    pub label: String,
    pub points: i32,
}

/// A portable rubric.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RubricDoc {
    pub schema: String,
    pub scale: Scale,
    pub chips: Vec<DocChip>,
}
