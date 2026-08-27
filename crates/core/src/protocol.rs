//! Table 1 of the paper, made unforgeable.
//!
//! The prototype had this state machine as a twelve-line function over a plain
//! object, which is the right shape for finding out whether the method works
//! and the wrong shape for evidence. Anything a UI can write, a UI can write
//! wrongly, and the claim this tool exists to support — *this question was
//! prompted with these exact bytes three times and failed every time* — is
//! worth precisely as much as the difficulty of writing it down without it
//! being true.
//!
//! So the rule here is: **nothing derivable is stored.** There is no `status`
//! field, no `locked` field, no `pct` field, no cached text hash. Each is a
//! method over the attempts. A hand-edited session file has no field in which
//! to lie about them, which is a stronger guarantee than validating a field
//! would be, and it is the same move `pdfwrite` makes by not having a code path
//! that writes an `/Info` dictionary.
//!
//! What remains storable — the text, the response digests, the marks, the
//! timestamps — is checked twice: refused here when it contradicts the
//! protocol, and re-derived from scratch by `verify` before anything can be
//! exported. Belt and braces, because the two catch different things. This
//! module catches an honest mistake at the moment it is made; `verify` catches
//! a session file that was edited afterwards.

use crate::hash::{canonical, Sha256Hex, Timestamp};
use crate::readability::{analyze, guard, GuardReport, Limits, Metrics};
use crate::rubric::{AttemptRef, Percent, Rubric, RubricError, Scores};

/// The paper's Three-Attempt Consistency Protocol, in one number.
pub const MAX_ATTEMPTS: usize = 3;

/// The three perturbation strategies of Table 1, Step 5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Strategy {
    /// Adding reasoning that requires visual or geometric interpretation.
    Spatial,
    /// Altering fundamental premises to prevent rote retrieval of a standard
    /// result.
    Axiomatic,
    /// Embedding the question in novel, class-specific context the rubric
    /// requires.
    Contextual,
}

impl Strategy {
    pub fn name(self) -> &'static str {
        match self {
            Strategy::Spatial => "Spatial Injection",
            Strategy::Axiomatic => "Axiomatic Replacement",
            Strategy::Contextual => "Contextual Embedding",
        }
    }

    pub fn all() -> [Strategy; 3] {
        [Strategy::Spatial, Strategy::Axiomatic, Strategy::Contextual]
    }
}

/// Where one version of a question stands. Always derived, never stored.
///
/// It deserialises as well as serialises, which is not a contradiction: a
/// `Status` is never a field of a run, but it *is* a value a verification
/// finding carries into a manifest, and a manifest has to be readable back to
/// be checkable.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", tag = "status")]
pub enum Status {
    /// Step 3. No attempt has been graded.
    Untested,
    /// Step 4. The first attempt met the threshold, so the question is not
    /// resistant and wants a perturbation.
    NotResistant { pct: f64 },
    /// Step 8a. Below threshold so far, with fewer than three attempts. The
    /// next move is to re-prompt the *exact* same text in a fresh instance.
    Testing { attempts: usize },
    /// Step 8b. Three attempts, all below threshold.
    Resistant,
    /// Step 8 did its job: a later attempt met the threshold after an earlier
    /// one did not. The first result was a false negative, and this version is
    /// not resistant however the first attempt looked.
    Inconsistent { at: usize },
}

impl Status {
    pub fn is_resistant(self) -> bool {
        matches!(self, Status::Resistant)
    }

    /// Can this version take another attempt? Untested and Testing only: once
    /// a version is decided, further prompting is not part of the protocol and
    /// would be shopping for a result.
    pub fn accepts_attempt(self) -> bool {
        matches!(self, Status::Untested | Status::Testing { .. })
    }

    /// The word the status rail shows.
    pub fn label(self) -> &'static str {
        match self {
            Status::Untested => "UNTESTED",
            Status::NotResistant { .. } => "NOT RESISTANT",
            Status::Testing { .. } => "TESTING",
            Status::Resistant => "RESISTANT",
            Status::Inconsistent { .. } => "FALSE NEGATIVE",
        }
    }

    /// How the status should read at a glance.
    ///
    /// In the core rather than in CSS because a status and its colour have to
    /// agree, and two places that each decide independently eventually will
    /// not. The stylesheet maps the name to a hue and nothing else.
    pub fn tone(self) -> Tone {
        match self {
            Status::Untested => Tone::Neutral,
            Status::NotResistant { .. } | Status::Inconsistent { .. } => Tone::Bad,
            Status::Testing { .. } => Tone::Working,
            Status::Resistant => Tone::Good,
        }
    }

    /// The Table 1 step this state is waiting on, for the UI banner.
    pub fn banner(self) -> &'static str {
        match self {
            Status::Untested => "Step 3 — Baseline. Copy this exact text into a fresh instance of \
                                 the target model, then grade the response as attempt 1.",
            Status::NotResistant { .. } => "Step 4 — Attempt 1 met the threshold. This version is \
                                            not resistant; apply a perturbation.",
            Status::Testing { .. } => "Step 8a — Below threshold so far. Re-prompt the exact same \
                                       text in a fresh instance.",
            Status::Resistant => "Step 8b — Failure across all three trials. Classified One-Shot \
                                  GenAI Resistant. Log hallucinations and update the rubric.",
            Status::Inconsistent { .. } => "Step 8 caught an inconsistency: a later attempt met \
                                            the threshold. Not resistant; perturb again.",
        }
    }
}

/// How a status reads at a glance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tone {
    Neutral,
    Working,
    Good,
    Bad,
}

/// The transition table, over percentages alone.
///
/// Split out from the state it is usually asked about so the table itself can
/// be tested directly against the prototype's `versionStatus`, which is the
/// only way to know the two agree rather than merely look similar.
pub fn status_from(pcts: &[f64], threshold: f64) -> Status {
    match pcts.first() {
        None => Status::Untested,
        Some(&first) if first >= threshold => Status::NotResistant { pct: first },
        Some(_) => match pcts.iter().position(|&p| p >= threshold) {
            Some(i) => Status::Inconsistent { at: i + 1 },
            None if pcts.len() >= MAX_ATTEMPTS => Status::Resistant,
            None => Status::Testing { attempts: pcts.len() },
        },
    }
}

/// Everything about a run that is a dial rather than an observation.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    /// The paper's failing line. Recorded in the manifest because "resistant"
    /// means nothing without it.
    pub threshold: Percent,
    pub fk_drift: f64,
    pub fk_cap: f64,
    pub growth_cap: f64,
}

impl Settings {
    pub fn limits(&self) -> Limits {
        Limits { fk_drift: self.fk_drift, fk_cap: self.fk_cap, growth_cap: self.growth_cap }
    }
}

impl Default for Settings {
    fn default() -> Self {
        let d = Limits::default();
        Settings {
            threshold: Percent::new(60.0).expect("60 is a percentage"),
            fk_drift: d.fk_drift,
            fk_cap: d.fk_cap,
            growth_cap: d.growth_cap,
        }
    }
}

/// What the protocol refused, and why.
#[derive(Debug, Clone, PartialEq)]
pub enum ProtocolError {
    NoSuchVersion(usize),
    NoSuchAttempt(AttemptRef),
    /// The text of a version that has been prompted cannot change. Perturbing
    /// means making a new version; the old one keeps its attempts, because
    /// those attempts are evidence about *those bytes*.
    VersionLocked,
    /// Attempts may only be stamped on the latest version. Grading an old
    /// version after moving on would produce a run whose history says the
    /// instructor went back and tried again until something failed.
    NotLatestVersion,
    /// A fourth attempt on one version. Three is the protocol.
    AttemptLimit,
    /// The version is already decided; more prompting is result-shopping.
    AlreadyDecided(Status),
    /// Graded against a rubric revision that did not exist at that time.
    TimestampBeforeRubric,
    /// The base version is the question; there is nothing behind it to fall
    /// back to.
    BaseVersionIsPermanent,
    EmptyText,
    Rubric(RubricError),
}

impl From<RubricError> for ProtocolError {
    fn from(e: RubricError) -> Self {
        ProtocolError::Rubric(e)
    }
}

/// One graded prompt of one version.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Attempt {
    at: Timestamp,
    /// SHA-256 of the version text as it stood when this attempt was prompted.
    ///
    /// Not a duplicate of the version's own hash: that one says what the query
    /// *is*, this one says what was actually sent. They agree in every run this
    /// module can produce, because a prompted version is locked — and the whole
    /// point of recording both is that a session file edited outside it cannot
    /// make them agree without editing all three attempts to match. "The same
    /// bytes were prompted three times" is not checkable without it.
    query: Sha256Hex,
    /// SHA-256 of the model's response. There is no field for the response
    /// itself: the text is hashed in the browser and dropped, so a run log
    /// cannot become a transcript of somebody's chatbot session.
    response: Sha256Hex,
    /// Which rubric revision graded this. Not a convenience — without it, a
    /// rubric that gained a penalty chip afterwards would silently re-grade
    /// every attempt that came before it.
    rubric_revision: usize,
    scores: Scores,
    /// The hallucination ledger. Session and assignment appendix only; these
    /// never reach the manifest, which is why the manifest can be shared.
    #[serde(default)]
    notes: Vec<String>,
}

impl Attempt {
    pub fn at(&self) -> &Timestamp {
        &self.at
    }
    /// The bytes that were sent, as they stood at the time.
    pub fn query(&self) -> &Sha256Hex {
        &self.query
    }
    pub fn response(&self) -> &Sha256Hex {
        &self.response
    }
    pub fn rubric_revision(&self) -> usize {
        self.rubric_revision
    }
    pub fn scores(&self) -> &Scores {
        &self.scores
    }
    pub fn notes(&self) -> &[String] {
        &self.notes
    }
}

/// One text of one question, and every attempt made against it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Version {
    /// `None` for the base version, which is the question as it arrived.
    strategy: Option<Strategy>,
    text: String,
    #[serde(default)]
    attempts: Vec<Attempt>,
}

impl Version {
    pub fn strategy(&self) -> Option<Strategy> {
        self.strategy
    }

    /// The canonical bytes of the query, which are the bytes that get copied,
    /// exported, and hashed.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Derived, so there is no stored hash to disagree with the text.
    pub fn text_sha256(&self) -> Sha256Hex {
        Sha256Hex::of(self.text.as_bytes())
    }

    /// Derived: a version is locked exactly when it has been prompted.
    pub fn locked(&self) -> bool {
        !self.attempts.is_empty()
    }

    pub fn attempts(&self) -> &[Attempt] {
        &self.attempts
    }

    pub fn metrics(&self) -> Metrics {
        analyze(&self.text)
    }
}

/// A question, its perturbation history, and its rubric.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Question {
    /// The number the question carries on the assignment. The only thing about
    /// a question the manifest is allowed to know — a title is question text.
    ordinal: usize,
    title: String,
    /// Index 0 is the base version; the rest are perturbations in order.
    versions: Vec<Version>,
    rubric: Rubric,
}

impl Question {
    pub fn new(ordinal: usize, title: &str, base: &str) -> Result<Question, ProtocolError> {
        let text = canonical(base);
        if text.trim().is_empty() {
            return Err(ProtocolError::EmptyText);
        }
        Ok(Question {
            ordinal,
            title: title.trim().to_string(),
            versions: vec![Version { strategy: None, text, attempts: Vec::new() }],
            rubric: Rubric::blank(),
        })
    }

    pub fn ordinal(&self) -> usize {
        self.ordinal
    }
    pub fn title(&self) -> &str {
        &self.title
    }
    pub fn versions(&self) -> &[Version] {
        &self.versions
    }
    pub fn rubric(&self) -> &Rubric {
        &self.rubric
    }

    /// The rubric, for authoring. Safe to hand out: `Rubric` refuses every
    /// edit once it is frozen, and the one edit it accepts afterwards —
    /// appending a penalty chip — is re-checked by `verify` for a provenance
    /// pointer that resolves.
    pub fn rubric_mut(&mut self) -> &mut Rubric {
        &mut self.rubric
    }

    pub fn base(&self) -> &Version {
        &self.versions[0]
    }

    pub fn latest(&self) -> &Version {
        self.versions.last().expect("a question always has a base version")
    }

    pub fn latest_ordinal(&self) -> usize {
        self.versions.len() - 1
    }

    pub fn version(&self, at: usize) -> Result<&Version, ProtocolError> {
        self.versions.get(at).ok_or(ProtocolError::NoSuchVersion(at))
    }

    pub fn frozen(&self) -> bool {
        self.rubric.frozen()
    }

    // ------------------------------------------------------------ editing

    /// Rename a question.
    ///
    /// Always allowed, and it changes nothing that has been recorded: a title
    /// is how the instructor finds the question in the rail, and it never
    /// reaches the manifest. An ingested title is a guess made from a heading
    /// line, so being able to fix it matters more than freezing it.
    pub fn retitle(&mut self, title: &str) {
        self.title = title.trim().to_string();
    }

    /// Rewrite a version that has not been prompted yet.
    pub fn edit(&mut self, at: usize, text: &str) -> Result<(), ProtocolError> {
        let clean = canonical(text);
        if clean.trim().is_empty() {
            return Err(ProtocolError::EmptyText);
        }
        let v = self.versions.get_mut(at).ok_or(ProtocolError::NoSuchVersion(at))?;
        if v.locked() {
            return Err(ProtocolError::VersionLocked);
        }
        v.text = clean;
        Ok(())
    }

    /// Step 5. Save a perturbation as the next version, returning its ordinal.
    pub fn add_version(&mut self, strategy: Strategy, text: &str) -> Result<usize, ProtocolError> {
        let clean = canonical(text);
        if clean.trim().is_empty() {
            return Err(ProtocolError::EmptyText);
        }
        self.versions.push(Version { strategy: Some(strategy), text: clean, attempts: Vec::new() });
        Ok(self.versions.len() - 1)
    }

    /// Undo the latest perturbation, if it has not been prompted.
    pub fn discard_latest(&mut self) -> Result<(), ProtocolError> {
        if self.versions.len() == 1 {
            return Err(ProtocolError::BaseVersionIsPermanent);
        }
        if self.latest().locked() {
            return Err(ProtocolError::VersionLocked);
        }
        self.versions.pop();
        Ok(())
    }

    // ------------------------------------------------------------ stamping

    /// Record one graded attempt, returning its 1-based ordinal.
    ///
    /// Everything is checked before anything is written, so a refused stamp
    /// leaves the run exactly as it was — including the rubric, which must not
    /// be frozen by a stamp that did not happen.
    ///
    /// The threshold is a parameter rather than a field because whether a
    /// version is already decided is a protocol question, and the answer
    /// depends on the line the run is being judged against.
    pub fn stamp(
        &mut self,
        version: usize,
        threshold: Percent,
        at: Timestamp,
        response: Sha256Hex,
        scores: Scores,
    ) -> Result<usize, ProtocolError> {
        if version >= self.versions.len() {
            return Err(ProtocolError::NoSuchVersion(version));
        }
        if version != self.latest_ordinal() {
            return Err(ProtocolError::NotLatestVersion);
        }
        // The attempt cap is checked before the status so that a fourth
        // attempt reports the rule it broke rather than the state that rule
        // put the version in.
        if self.versions[version].attempts.len() >= MAX_ATTEMPTS {
            return Err(ProtocolError::AttemptLimit);
        }
        let status = self.status_of(version, threshold)?;
        if !status.accepts_attempt() {
            return Err(ProtocolError::AlreadyDecided(status));
        }

        // Grading first: it is the check most likely to fail, and it must not
        // be able to freeze the rubric on its way to failing.
        let (rubric_revision, since) = {
            let revision = self.rubric.current();
            revision.grade(self.rubric.scale(), &scores)?;
            (revision.ordinal, revision.at.clone())
        };
        if let Some(since) = since {
            if at < since {
                return Err(ProtocolError::TimestampBeforeRubric);
            }
        }

        self.rubric.freeze(at.clone());
        let v = &mut self.versions[version];
        let query = Sha256Hex::of(v.text.as_bytes());
        v.attempts.push(Attempt {
            at,
            query,
            response,
            rubric_revision,
            scores,
            notes: Vec::new(),
        });
        Ok(v.attempts.len())
    }

    /// Add a hallucination-ledger note to an attempt.
    pub fn note(&mut self, r: AttemptRef, text: &str) -> Result<(), ProtocolError> {
        if text.trim().is_empty() {
            return Err(ProtocolError::EmptyText);
        }
        let v = self.versions.get_mut(r.version).ok_or(ProtocolError::NoSuchVersion(r.version))?;
        let a = v
            .attempts
            .get_mut(r.attempt.wrapping_sub(1))
            .ok_or(ProtocolError::NoSuchAttempt(r))?;
        a.notes.push(text.trim().to_string());
        Ok(())
    }

    /// Step 6. Append a penalty chip derived from what an attempt showed.
    pub fn add_penalty(
        &mut self,
        label: &str,
        points: i32,
        from: AttemptRef,
        at: Timestamp,
    ) -> Result<usize, ProtocolError> {
        self.attempt(from)?;
        Ok(self.rubric.add_penalty(label, points, from, at)?)
    }

    pub fn attempt(&self, r: AttemptRef) -> Result<&Attempt, ProtocolError> {
        self.version(r.version)?
            .attempts
            .get(r.attempt.wrapping_sub(1))
            .ok_or(ProtocolError::NoSuchAttempt(r))
    }

    // ------------------------------------------------------------ derived

    /// Re-grade one attempt against the revision that actually graded it.
    pub fn pct(&self, r: AttemptRef) -> Result<f64, ProtocolError> {
        let a = self.attempt(r)?;
        let revision = self.rubric.revision(a.rubric_revision)?;
        Ok(revision.grade(self.rubric.scale(), &a.scores)?)
    }

    /// Every attempt's percentage on one version, in order.
    pub fn pcts(&self, version: usize) -> Result<Vec<f64>, ProtocolError> {
        let v = self.version(version)?;
        (1..=v.attempts.len())
            .map(|attempt| self.pct(AttemptRef { version, attempt }))
            .collect()
    }

    pub fn status_of(&self, version: usize, threshold: Percent) -> Result<Status, ProtocolError> {
        Ok(status_from(&self.pcts(version)?, threshold.get()))
    }

    /// The question's status is its latest version's status. Earlier versions
    /// are history; the question is whatever it currently says.
    ///
    /// Fallible, which looks like pedantry and is not: re-deriving a status
    /// means re-grading every attempt against the revision it names, and a
    /// session file edited outside this module can name a revision that is not
    /// in it. An infallible signature here would have to panic on that input,
    /// and a panic inside wasm reaches the user as "unreachable executed" with
    /// no indication of what broke. `verify` turns the same condition into a
    /// blocking finding that says exactly which attempt is unreadable.
    pub fn status(&self, threshold: Percent) -> Result<Status, ProtocolError> {
        self.status_of(self.latest_ordinal(), threshold)
    }

    /// How this version reads against the base text it descends from.
    pub fn guard_of(&self, version: usize, limits: &Limits) -> Result<GuardReport, ProtocolError> {
        let v = self.version(version)?;
        Ok(guard(&self.base().metrics(), &v.metrics(), limits))
    }

    /// The same measurement for a draft that has not been saved, so the bench
    /// can show it live without the UI computing anything.
    pub fn guard_draft(&self, draft: &str, limits: &Limits) -> GuardReport {
        guard(&self.base().metrics(), &analyze(&canonical(draft)), limits)
    }
}
