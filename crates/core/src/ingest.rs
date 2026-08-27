//! Turning somebody else's assignment PDF into a starting point.
//!
//! pdf.js extracts the text layer in the browser; everything that decides what
//! is a question, what is a heading, and what is page furniture happens here,
//! under `cargo test`, because getting it wrong silently is easy and the
//! instructor would have no way to tell.
//!
//! The tool has to work on any CS assignment, so nothing here is specific to
//! the discrete-mathematics labs the paper used. Assignments number their
//! questions in a handful of ways and each document picks one and sticks to
//! it, which is the fact this module leans on: rather than accepting anything
//! that looks like a heading, it works out which single convention *this*
//! document uses and then applies only that one. A `1.` inside a list of parts
//! cannot be promoted to a question in a document whose questions say
//! `Question 4`.
//!
//! Everything it produces is a draft. Text extraction loses indentation on
//! some producers, glues superscripts onto words on others, and cannot tell a
//! monospaced heading from a line of code. The instructor edits what comes
//! out; the tool's job is to save the typing, not to be right.

use crate::hash::canonical;
use crate::markup::fence_monospace;

/// One line of extracted text, as the browser found it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Line {
    pub text: String,
    /// 1-based, so furniture can be recognised by repeating across pages.
    pub page: usize,
    /// Every glyph on the line came from a monospaced font. pdf.js knows this
    /// and a PDF does not, which is as close to "this is code" as extraction
    /// gets.
    #[serde(default)]
    pub mono: bool,
}

/// How this document numbers its questions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HeadingStyle {
    /// `Question 4`, `Question 4:`, `Question 4.`
    Question,
    /// `Problem 4`
    Problem,
    /// `Exercise 4`
    Exercise,
    /// `Task 4`
    Task,
    /// `Q4`, `Q4:`
    Abbreviated,
    /// `4.` or `4)` followed by text. Last resort, and only when nothing else
    /// matches, because it is the shape a list of parts also has.
    Numbered,
}

impl HeadingStyle {
    /// In priority order. A document that says `Question 4` is read that way
    /// even though its parts are also numbered.
    pub fn all() -> [HeadingStyle; 6] {
        use HeadingStyle::*;
        [Question, Problem, Exercise, Task, Abbreviated, Numbered]
    }

    fn keyword(self) -> Option<&'static str> {
        use HeadingStyle::*;
        match self {
            Question => Some("question"),
            Problem => Some("problem"),
            Exercise => Some("exercise"),
            Task => Some("task"),
            Abbreviated | Numbered => None,
        }
    }
}

/// One question as extracted, before it becomes a `protocol::Question`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Draft {
    pub ordinal: usize,
    pub title: String,
    pub text: String,
}

/// What the split found, and what it threw away doing it.
///
/// The counts are here so the UI can say what happened rather than presenting
/// the result as if it were obvious. An ingest that silently dropped nine
/// lines it took for footers should be able to say so.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Ingested {
    pub drafts: Vec<Draft>,
    /// `None` when no convention was recognised and everything came back as
    /// one draft for the instructor to split by hand.
    pub style: Option<HeadingStyle>,
    pub pages: usize,
    pub furniture_dropped: usize,
    pub checkboxes_stripped: usize,
    pub code_lines: usize,
}

/// Glyphs used to mark "I used AI for this part" and their neighbours.
///
/// Stripped rather than kept because they are an artefact of the answer sheet,
/// not of the question: leaving them in puts a checkbox in the middle of every
/// query the model is asked, and in the exported assignment.
const CHECKBOXES: &[char] = &[
    '\u{25A1}', '\u{25A0}', '\u{25AA}', '\u{25AB}', '\u{25FB}', '\u{25FC}', '\u{2610}',
    '\u{2611}', '\u{2612}', '\u{274F}', '\u{2750}', '\u{2751}', '\u{2752}', '\u{2B1B}',
    '\u{2B1C}', '\u{FFFD}',
];

/// A line reduced to the shape it would have on any page: case folded,
/// whitespace collapsed, every run of digits replaced by `#`.
///
/// Page furniture repeats with its page number changing — `Question 4:-1`,
/// `Question 4:-2` — so comparing raw text finds nothing. Comparing shapes
/// finds it on the first try.
fn shape(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_digits = false;
    for c in line.trim().chars() {
        if c.is_ascii_digit() {
            if !in_digits {
                out.push('#');
                in_digits = true;
            }
            continue;
        }
        in_digits = false;
        if c.is_whitespace() {
            if !out.ends_with(' ') {
                out.push(' ');
            }
        } else {
            out.extend(c.to_lowercase());
        }
    }
    out.trim().to_string()
}

/// Read a heading of one style off a line: its number, and whatever title
/// follows it.
fn heading(line: &str, style: HeadingStyle) -> Option<(usize, String)> {
    let t = line.trim();
    let rest = match style.keyword() {
        Some(word) => {
            let head = t.get(..word.len())?;
            if !head.eq_ignore_ascii_case(word) {
                return None;
            }
            t[word.len()..].trim_start()
        }
        None if style == HeadingStyle::Abbreviated => {
            if !t.starts_with('Q') && !t.starts_with('q') {
                return None;
            }
            &t[1..]
        }
        None => t,
    };

    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() || digits.len() > 3 {
        return None;
    }
    let ordinal: usize = digits.parse().ok()?;
    let mut tail = rest[digits.len()..].trim_start();

    // A bare number needs its punctuation, or every year and quantity in the
    // document becomes a question.
    if style == HeadingStyle::Numbered && !(tail.starts_with('.') || tail.starts_with(')')) {
        return None;
    }
    tail = tail.trim_start_matches([':', '.', ')', '-', '\u{2013}']).trim();

    // `Question 4:-1` is a page footer, not question 4. A title starts with a
    // word; anything else means this line is furniture wearing a heading's
    // clothes.
    if !tail.is_empty() && !tail.starts_with(|c: char| c.is_alphanumeric()) {
        return None;
    }
    if style == HeadingStyle::Numbered && tail.is_empty() {
        return None;
    }
    Some((ordinal, tail.to_string()))
}

/// Which convention this document uses, and where its headings are.
///
/// A style qualifies only if it yields at least two headings whose numbers
/// strictly increase, which is what stops a repeated footer and a numbered
/// sub-list from being read as questions.
fn detect(lines: &[Line]) -> Option<(HeadingStyle, Vec<(usize, usize, String)>)> {
    for style in HeadingStyle::all() {
        let mut found: Vec<(usize, usize, String)> = Vec::new();
        for (i, l) in lines.iter().enumerate() {
            if let Some((ordinal, title)) = heading(&l.text, style) {
                if found.last().is_none_or(|(_, last, _)| ordinal > *last) {
                    found.push((i, ordinal, title));
                }
            }
        }
        if found.len() >= 2 {
            return Some((style, found));
        }
    }
    None
}

/// Lines that repeat at the top or bottom of page after page.
///
/// Only page edges are considered. A running footer is always at an edge, and
/// restricting the search there is what keeps a legitimately repeated
/// instruction — "Show your work." under every part — out of the furniture
/// pile.
///
/// Two pages is enough to establish a repeat, and it has to be: plenty of
/// assignments are two pages long, and a rule that needed three left the footer
/// on every one of them. The other half of the test — that it appears on at
/// least half the pages — is what stops a line that happens to fall at an edge
/// twice in a long document from being swept away with the furniture.
fn furniture(lines: &[Line], headings: &[usize]) -> Vec<bool> {
    use std::collections::{BTreeMap, BTreeSet};

    let pages = lines.iter().map(|l| l.page).max().unwrap_or(0);
    let mut at_edge: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();

    let mut page_start = 0;
    for i in 0..lines.len() {
        let last_of_page = i + 1 == lines.len() || lines[i + 1].page != lines[i].page;
        if last_of_page {
            let page = &lines[page_start..=i];
            let n = page.len();
            for (j, l) in page.iter().enumerate() {
                if j < 2 || j + 3 >= n {
                    let s = shape(&l.text);
                    if !s.is_empty() {
                        at_edge.entry(s).or_default().insert(l.page);
                    }
                }
            }
            page_start = i + 1;
        }
    }

    let repeated: BTreeSet<&String> = at_edge
        .iter()
        .filter(|(_, seen)| seen.len() >= 2 && seen.len() * 2 >= pages)
        .map(|(s, _)| s)
        .collect();

    lines
        .iter()
        .enumerate()
        .map(|(i, l)| !headings.contains(&i) && repeated.contains(&shape(&l.text)))
        .collect()
}

/// Split an extracted document into question drafts.
pub fn ingest(lines: &[Line]) -> Ingested {
    let pages = lines.iter().map(|l| l.page).max().unwrap_or(0);

    // Checkboxes first: a heading is not a heading if a glyph is stuck to it.
    let mut checkboxes_stripped = 0;
    let cleaned: Vec<Line> = lines
        .iter()
        .map(|l| {
            let before = l.text.chars().count();
            let text: String = l.text.chars().filter(|c| !CHECKBOXES.contains(c)).collect();
            checkboxes_stripped += before - text.chars().count();
            Line { text: text.trim_end().to_string(), page: l.page, mono: l.mono }
        })
        .collect();

    let detected = detect(&cleaned);
    let heading_at: Vec<usize> =
        detected.as_ref().map(|(_, h)| h.iter().map(|(i, _, _)| *i).collect()).unwrap_or_default();
    let drop = furniture(&cleaned, &heading_at);
    let furniture_dropped = drop.iter().filter(|d| **d).count();

    // The remainder of a heading line is the first line of its question, not
    // decoration. "Question 2. Explain why your loop terminates." carries the
    // whole question there and has no body at all; dropping the tail loses it.
    let body = |lead: &str, from: usize, to: usize| -> (String, usize) {
        let mut kept: Vec<(String, bool)> = Vec::new();
        if !lead.trim().is_empty() {
            kept.push((lead.to_string(), false));
        }
        kept.extend(
            (from..to)
                .filter(|i| !drop[*i])
                .map(|i| (cleaned[i].text.clone(), cleaned[i].mono)),
        );
        let code_lines = kept.iter().filter(|(_, m)| *m).count();
        let text = if code_lines > 0 {
            fence_monospace(&kept)
        } else {
            kept.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>().join("\n")
        };
        (canonical(&text), code_lines)
    };

    let mut code_lines = 0;
    let drafts = match &detected {
        Some((_, found)) => found
            .iter()
            .enumerate()
            .map(|(n, (i, ordinal, title))| {
                let to = found.get(n + 1).map(|(j, _, _)| *j).unwrap_or(cleaned.len());
                let (text, code) = body(title, *i + 1, to);
                code_lines += code;
                Draft {
                    ordinal: *ordinal,
                    // A short remainder doubles as the title. A long one is the
                    // question itself, and a title that is the whole question
                    // is not a title.
                    title: if title.is_empty() || title.chars().count() > 80 {
                        format!("Question {}", ordinal)
                    } else {
                        title.clone()
                    },
                    text,
                }
            })
            .filter(|d| !d.text.trim().is_empty())
            .collect(),
        // Nothing recognisable. One draft with everything in it beats nine
        // wrong ones: the instructor splits it by hand, which is the
        // affordance the plan asks for.
        None => {
            let (text, code) = body("", 0, cleaned.len());
            code_lines += code;
            if text.trim().is_empty() {
                Vec::new()
            } else {
                vec![Draft { ordinal: 1, title: "Question 1".to_string(), text }]
            }
        }
    };

    Ingested {
        drafts,
        style: detected.map(|(s, _)| s),
        pages,
        furniture_dropped,
        checkboxes_stripped,
        code_lines,
    }
}

/// Cut one draft in two at a line boundary — the manual "split here" the UI
/// offers when no convention was recognised, or when one was recognised wrongly.
///
/// In the core rather than the UI so that the halves are canonicalised the
/// same way an ingested draft is, and therefore hash the same way.
pub fn split_at(text: &str, line: usize) -> Option<(String, String)> {
    let lines: Vec<&str> = text.lines().collect();
    if line == 0 || line >= lines.len() {
        return None;
    }
    let head = canonical(&lines[..line].join("\n"));
    let tail = canonical(&lines[line..].join("\n"));
    if head.trim().is_empty() || tail.trim().is_empty() {
        return None;
    }
    Some((head, tail))
}
