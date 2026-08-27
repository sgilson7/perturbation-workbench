//! Flesch–Kincaid, ported from the prototype, and the complexity guard.
//!
//! A perturbation that defeats the model by being *harder to read* has not
//! made the question resistant; it has made it a worse question, and the
//! students it costs most are the ones the paper is about. So every draft is
//! measured against its own base text before it can be saved, and drifting
//! upward is called out.
//!
//! The arithmetic is a deliberate, line-for-line port of `analyze` and
//! `countSyl` from the React prototype rather than a fresh implementation.
//! Readability formulas disagree with each other by a whole grade level on the
//! same paragraph, so "correct" is not available; what *is* available is that
//! numbers recorded during the study and numbers shown by this tool mean the
//! same thing. `tests/readability.rs` pins that agreement on the nine base
//! texts the study actually used.
//!
//! It is approximate on mathematics and on code, and says so in the README. `f1 : {1,2,3}
//! -> {a,b,c}` is not prose and has no syllables; the stripping below removes
//! the notation that would otherwise be counted as very long words, which is
//! enough to make a drift measurement meaningful and is not enough to make the
//! absolute number mean anything on its own.

/// JavaScript's `Math.round`: halves go up, including negative halves, where
/// Rust's `f64::round` goes away from zero instead. Ported rather than
/// approximated because a rubric with penalty chips can produce a negative
/// percentage, and that is exactly where the two disagree.
pub(crate) fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// What one text measures.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Metrics {
    pub words: usize,
    pub sentences: usize,
    /// Flesch–Kincaid grade level, one decimal.
    pub grade: f64,
    /// Flesch reading ease, whole number.
    pub ease: f64,
}

/// Syllables in one word, by the prototype's heuristic.
pub fn syllables(word: &str) -> usize {
    let w: String = word.to_lowercase().chars().filter(|c| c.is_ascii_lowercase()).collect();
    if w.is_empty() || w.len() <= 3 {
        return 1;
    }
    let b = w.as_bytes();
    let n = b.len();
    let consonantish = |c: u8| !matches!(c, b'l' | b'a' | b'e' | b'i' | b'o' | b'u' | b'y');

    // `/(?:[^laeiouy]es|ed|[^laeiouy]e)$/` — anchored at the end, so only two
    // start positions can match, and the leftmost one wins as it does in JS.
    let cut = if n >= 3 && consonantish(b[n - 3]) && &b[n - 2..] == b"es" {
        n - 3
    } else if n >= 2 && &b[n - 2..] == b"ed" {
        n - 2
    } else if n >= 2 && consonantish(b[n - 2]) && b[n - 1] == b'e' {
        n - 2
    } else {
        n
    };
    let trimmed = &w[..cut];
    let trimmed = trimmed.strip_prefix('y').unwrap_or(trimmed);

    // `/[aeiouy]{1,2}/g` — greedy, so a run of three vowels counts as two.
    let mut groups = 0;
    let mut run = 0;
    for c in trimmed.bytes() {
        if matches!(c, b'a' | b'e' | b'i' | b'o' | b'u' | b'y') {
            run += 1;
            if run == 1 {
                groups += 1;
            } else if run == 2 {
                run = 0;
            }
        } else {
            run = 0;
        }
    }
    groups.max(1)
}

/// Remove the notation that is not prose.
///
/// Mirrors the prototype's three passes in order: inline `$…$` becomes the
/// single word "equation", two-character operators become spaces, and the
/// remaining operator characters become spaces. Arrows go first because
/// `->` would otherwise leave a stray `-` glued to the next word.
fn strip_math(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '$' {
            if let Some(close) = chars[i + 1..].iter().position(|&c| c == '$') {
                out.push_str(" equation ");
                i += close + 2;
                continue;
            }
        }
        let two = if i + 1 < chars.len() {
            [chars[i], chars[i + 1]]
        } else {
            [chars[i], '\0']
        };
        match (two[0], two[1]) {
            ('-', '>') | ('=', '>') | ('<', '=') | ('>', '=') | ('!', '=') => {
                out.push(' ');
                i += 2;
                continue;
            }
            _ => {}
        }
        match chars[i] {
            '\\' | '{' | '}' | '_' | '^' | '*' | '=' | '+' | '~' | '|' => out.push(' '),
            c => out.push(c),
        }
        i += 1;
    }
    out
}

/// Measure a text.
///
/// Code is removed before anything is counted — see `markup::prose_only` for
/// why. A question with no fenced blocks is unaffected, which is what keeps
/// the study's nine base texts scoring exactly what they scored.
pub fn analyze(text: &str) -> Metrics {
    if text.trim().is_empty() {
        return Metrics { words: 0, sentences: 1, grade: 0.0, ease: 0.0 };
    }
    let clean = strip_math(&crate::markup::prose_only(text));

    // A "sentence" needs more than one word, so a heading or a bare `1.` on its
    // own line does not divide the text and inflate the sentence count.
    let sentences = clean
        .split(|c| matches!(c, '.' | '!' | '?' | ';' | '\n'))
        .filter(|s| s.trim().split_whitespace().count() > 1)
        .count();

    let words: Vec<&str> = clean
        .split_whitespace()
        .filter(|w| w.chars().any(|c| c.is_ascii_alphanumeric()))
        .collect();

    let syl: usize = words
        .iter()
        .map(|w| {
            // A bare number is one syllable however many digits it has, so
            // "3628800" does not read as an eleven-syllable word.
            if w.chars().all(|c| c.is_ascii_digit() || matches!(c, '(' | ')' | '.' | ',')) {
                1
            } else {
                syllables(w)
            }
        })
        .sum();

    let s = sentences.max(1) as f64;
    let w = words.len().max(1) as f64;
    let per_sentence = w / s;
    let per_word = syl as f64 / w;

    Metrics {
        words: words.len(),
        sentences: sentences.max(1),
        grade: js_round((0.39 * per_sentence + 11.8 * per_word - 15.59) * 10.0) / 10.0,
        ease: js_round(206.835 - 1.015 * per_sentence - 84.6 * per_word),
    }
}

// ---------------------------------------------------------------- the guard

/// How far a perturbation may drift before it is worth saying so out loud.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Limits {
    /// Grade levels a version may rise above its own base text.
    pub fk_drift: f64,
    /// Absolute grade ceiling, whatever the base was.
    pub fk_cap: f64,
    /// Percentage the word count may grow by.
    pub growth_cap: f64,
}

impl Default for Limits {
    fn default() -> Self {
        // The plan's defaults. 14 is roughly a second-year undergraduate
        // reading level, which a discrete mathematics lab is already at.
        Limits { fk_drift: 1.5, fk_cap: 14.0, growth_cap: 35.0 }
    }
}

/// What the guard found. Advisory by construction: there is no `blocked` field
/// and nothing consumes this as a veto.
///
/// The guard reports rather than refuses because the instructor is the one who
/// knows whether a longer question is a worse question. A tool that blocked
/// here would be overruling the person with the expertise, and the paper's
/// method is explicitly instructor-led. What it must not do is let the drift
/// pass unrecorded, so the finding travels into the manifest as an advisory.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuardReport {
    pub grade: f64,
    pub base_grade: f64,
    pub growth: f64,
    /// Rose more than `fk_drift` grades above the base text.
    pub drifted: bool,
    /// Above `fk_cap` in absolute terms.
    pub over_cap: bool,
    /// Grew more than `growth_cap` percent in words.
    pub overgrown: bool,
}

impl GuardReport {
    pub fn tripped(&self) -> bool {
        self.drifted || self.over_cap || self.overgrown
    }
}

/// Compare a candidate against the base text it descends from.
pub fn guard(base: &Metrics, candidate: &Metrics, limits: &Limits) -> GuardReport {
    let growth = js_round(
        (candidate.words as f64 - base.words as f64) / (base.words.max(1) as f64) * 100.0,
    );
    GuardReport {
        grade: candidate.grade,
        base_grade: base.grade,
        growth,
        drifted: candidate.grade > base.grade + limits.fk_drift,
        over_cap: candidate.grade > limits.fk_cap,
        overgrown: growth > limits.growth_cap,
    }
}
