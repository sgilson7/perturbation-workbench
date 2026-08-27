//! The working state of a run: what `localStorage` holds, and what moves
//! between machines.
//!
//! This file is **not** the manifest, and the difference is the point. The
//! manifest is built to be shared — it carries hashes, counts and settings and
//! is structurally incapable of carrying anything else. A session carries the
//! question text, the ledger notes, and every score, because it has to: it is
//! the run, paused. Sending someone a session file is sending them your
//! assignment before it is finished, which is occasionally what you want and
//! never what you want by accident. The README says so and so does the file.
//!
//! Two things are enforced here rather than left to the UI. A run has to name
//! the model it is testing against before it can stamp anything, because
//! "resistant" is a claim about a specific model on a specific day and a run
//! that never wrote it down cannot support one. And the target is *appended*
//! rather than overwritten, so switching models halfway through leaves a trace
//! instead of leaving a tidy file that quietly means two different things.

use crate::hash::{Sha256Hex, Timestamp};
use crate::protocol::{ProtocolError, Question, Settings, Status};
use crate::rubric::Scores;

/// Bumped, not extended, when the shape changes. A file naming a schema this
/// build does not know is refused on the way in rather than read partially.
pub const SESSION_SCHEMA: &str = "perturbation-workbench-session/1";

const NOTE: &str = "Working state of a perturbation run: question text, ledger notes and \
scores. This is NOT the run manifest -- the manifest carries only hashes, counts and \
settings and is the file meant for sharing.";

/// A schema tag that only deserialises from its own value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Schema;

impl serde::Serialize for Schema {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(SESSION_SCHEMA)
    }
}

impl<'de> serde::Deserialize<'de> for Schema {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        if s == SESSION_SCHEMA {
            Ok(Schema)
        } else {
            Err(serde::de::Error::custom(format!(
                "session schema {:?}, expected {:?}",
                s, SESSION_SCHEMA
            )))
        }
    }
}

/// A constant the file carries about itself. Written on the way out, ignored
/// on the way in, so editing it changes nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Note;

impl serde::Serialize for Note {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(NOTE)
    }
}

impl<'de> serde::Deserialize<'de> for Note {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d).map(|_| Note)
    }
}

/// What went wrong at the run level.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionError {
    /// Nothing can be stamped until the run says what it is testing against.
    NoTarget,
    EmptyModel,
    NoSuchQuestion(usize),
    Protocol(ProtocolError),
}

impl From<ProtocolError> for SessionError {
    fn from(e: ProtocolError) -> Self {
        SessionError::Protocol(e)
    }
}

/// The document the questions came out of.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Input {
    pub sha256: Sha256Hex,
    pub pages: usize,
}

/// The model a run is being tested against.
///
/// `model` is the one piece of free text the manifest keeps, because a run
/// that does not name its target is not evidence of anything. `access`
/// describes how the instructor reached it — a licensed workspace account
/// behaves differently from a public free tier, and a collaborator repeating
/// the run needs to know which.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Target {
    pub model: String,
    #[serde(default)]
    pub access: String,
    /// Step 7: each attempt goes to a distinct instance, so context cannot
    /// carry over between prompts.
    pub fresh_instance_per_attempt: bool,
    pub at: Timestamp,
}

/// A run.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Session {
    schema: Schema,
    note: Note,
    settings: Settings,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    input: Option<Input>,
    /// Append-only. The last entry is in force; more than one means the run
    /// changed targets, which `verify` reports.
    #[serde(default)]
    targets: Vec<Target>,
    #[serde(default)]
    questions: Vec<Question>,
}

impl Session {
    pub fn new(settings: Settings) -> Session {
        Session {
            schema: Schema,
            note: Note,
            settings,
            input: None,
            targets: Vec::new(),
            questions: Vec::new(),
        }
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Settings are a dial, and dials move. What they cannot do is move
    /// silently: the manifest records the values in force at export, and
    /// `verify` re-derives every status against them.
    pub fn set_settings(&mut self, settings: Settings) {
        self.settings = settings;
    }

    pub fn input(&self) -> Option<&Input> {
        self.input.as_ref()
    }

    pub fn set_input(&mut self, sha256: Sha256Hex, pages: usize) {
        self.input = Some(Input { sha256, pages });
    }

    pub fn target(&self) -> Option<&Target> {
        self.targets.last()
    }

    pub fn targets(&self) -> &[Target] {
        &self.targets
    }

    pub fn set_target(
        &mut self,
        model: &str,
        access: &str,
        fresh_instance_per_attempt: bool,
        at: Timestamp,
    ) -> Result<(), SessionError> {
        if model.trim().is_empty() {
            return Err(SessionError::EmptyModel);
        }
        let next = Target {
            model: model.trim().to_string(),
            access: access.trim().to_string(),
            fresh_instance_per_attempt,
            at,
        };
        // Re-stating the same target is not a change, and recording it as one
        // would produce an advisory about something that did not happen.
        if let Some(current) = self.targets.last() {
            if current.model == next.model
                && current.access == next.access
                && current.fresh_instance_per_attempt == next.fresh_instance_per_attempt
            {
                return Ok(());
            }
        }
        self.targets.push(next);
        Ok(())
    }

    pub fn questions(&self) -> &[Question] {
        &self.questions
    }

    pub fn question(&self, at: usize) -> Result<&Question, SessionError> {
        self.questions.get(at).ok_or(SessionError::NoSuchQuestion(at))
    }

    pub fn question_mut(&mut self, at: usize) -> Result<&mut Question, SessionError> {
        self.questions.get_mut(at).ok_or(SessionError::NoSuchQuestion(at))
    }

    pub fn add_question(&mut self, q: Question) -> usize {
        self.questions.push(q);
        self.questions.len() - 1
    }

    /// True once any attempt anywhere has been stamped.
    pub fn started(&self) -> bool {
        self.questions.iter().any(|q| q.versions().iter().any(|v| v.locked()))
    }

    /// Record an attempt. The entry point the bridge uses, because it is the
    /// one that refuses a run with no named target.
    ///
    /// `Question::stamp` stays reachable so the protocol can be tested without
    /// a session around it; a run that reached this state some other way is
    /// caught again by `verify`, which blocks on attempts with no target.
    pub fn stamp(
        &mut self,
        question: usize,
        version: usize,
        at: Timestamp,
        response: Sha256Hex,
        scores: Scores,
    ) -> Result<usize, SessionError> {
        if self.targets.is_empty() {
            return Err(SessionError::NoTarget);
        }
        let threshold = self.settings.threshold;
        Ok(self.question_mut(question)?.stamp(version, threshold, at, response, scores)?)
    }

    pub fn status(&self, question: usize) -> Result<Status, SessionError> {
        Ok(self.question(question)?.status(self.settings.threshold))
    }

    /// How many questions have reached Step 8b.
    pub fn resistant(&self) -> usize {
        self.questions
            .iter()
            .filter(|q| q.status(self.settings.threshold).is_resistant())
            .count()
    }
}

impl Default for Session {
    fn default() -> Self {
        Session::new(Settings::default())
    }
}
