//! A question, segmented into prose and code.
//!
//! The workbench is for any CS assignment, not only the discrete-mathematics
//! labs the paper used, and most CS questions contain code. That breaks two
//! things if it is ignored.
//!
//! The first is readability. A forty-line Java method is not prose: it has no
//! sentences, its identifiers are unpronounceable, and Flesch-Kincaid on it
//! returns a grade level in the twenties. Left in, the complexity guard fires
//! on every code-bearing question and the instructor learns to ignore it,
//! which is worse than not having a guard. Code is replaced by a single word
//! before counting, exactly as `$...$` already is.
//!
//! The second is the assignment PDF, where a code block set in Helvetica with
//! its indentation collapsed is not the question that was asked.
//!
//! **Fenced markdown, not a structured document.** The query is a string and
//! stays a string: what the instructor types is byte-for-byte what gets
//! hashed, copied and pasted into the chatbot. That matters more here than
//! anywhere else, because a structured editor would have to *serialise* itself
//! back to text before prompting, and the bytes that were tested would then be
//! the bytes some serialiser chose rather than the bytes anyone saw. Fences
//! are also what people already paste into a chatbot, so nothing is lost in
//! translation at the one moment the tool is not in the loop.
//!
//! This module is therefore a *view*, never a representation. Blocks borrow
//! from the text and are recomputed on demand; there is nothing stored to fall
//! out of step with the query it describes.

/// One run of a question.
#[derive(Debug, Clone, PartialEq)]
pub enum Block<'a> {
    Prose(&'a str),
    Code {
        /// The fence's info string, when there was one: `rust`, `java`, `py`.
        /// Advisory — nothing here interprets it, and the PDF writer sets every
        /// language the same way.
        language: Option<&'a str>,
        /// The lines between the fences, verbatim and un-reindented.
        body: &'a str,
    },
}

impl Block<'_> {
    pub fn is_code(&self) -> bool {
        matches!(self, Block::Code { .. })
    }
}

/// Is this line a fence, and if so how long is it and what follows it?
///
/// Deliberately a small subset of CommonMark: three or more backticks or
/// tildes, indented no more than three spaces. The full grammar buys nothing
/// here and a surprising corner of it would change what gets hashed.
fn fence(line: &str) -> Option<(char, usize, &str)> {
    let trimmed = line.trim_start();
    if line.len() - trimmed.len() > 3 {
        return None;
    }
    let marker = trimmed.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let run = trimmed.chars().take_while(|&c| c == marker).count();
    if run < 3 {
        return None;
    }
    let info = trimmed[run..].trim();
    // An info string cannot contain a backtick, or `a ``b`` c` opens a fence.
    if marker == '`' && info.contains('`') {
        return None;
    }
    Some((marker, run, info))
}

/// Split a question into prose and code.
///
/// An unclosed fence runs to the end of the text rather than being discarded:
/// a half-typed question is still the question, and silently reclassifying the
/// rest of it as prose would move the readability numbers under the author
/// mid-keystroke.
pub fn blocks(text: &str) -> Vec<Block<'_>> {
    let mut out = Vec::new();
    let mut prose_from: Option<usize> = None;
    let mut offset = 0;

    let mut lines = text.split_inclusive('\n').peekable();
    let mut pending: Option<(char, usize, Option<&str>, usize)> = None; // marker, len, lang, body start

    while let Some(line) = lines.next() {
        let start = offset;
        offset += line.len();
        let bare = line.strip_suffix('\n').unwrap_or(line);

        match &pending {
            None => {
                if let Some((marker, run, info)) = fence(bare) {
                    if let Some(from) = prose_from.take() {
                        out.push(Block::Prose(&text[from..start]));
                    }
                    let language = if info.is_empty() { None } else { Some(info) };
                    pending = Some((marker, run, language, offset));
                } else if prose_from.is_none() {
                    prose_from = Some(start);
                }
            }
            Some((marker, run, language, body_from)) => {
                // A closing fence is the same marker, at least as long, and
                // carries no info string.
                let closes = fence(bare)
                    .is_some_and(|(m, r, info)| m == *marker && r >= *run && info.is_empty());
                if closes {
                    out.push(Block::Code { language: *language, body: &text[*body_from..start] });
                    pending = None;
                    prose_from = None;
                } else if lines.peek().is_none() {
                    out.push(Block::Code { language: *language, body: &text[*body_from..offset] });
                    pending = None;
                }
            }
        }
    }
    if let Some(from) = prose_from {
        if from < text.len() {
            out.push(Block::Prose(&text[from..]));
        }
    }
    out
}

/// The text with every code block replaced by the single word `code`.
///
/// The replacement rather than a deletion is deliberate, and mirrors what
/// `readability` already does with `$...$`. Deleting a block would leave the
/// sentence around it truncated — "Complete the method" followed by nothing —
/// and a question that is *entirely* code would measure as empty rather than
/// as one short instruction.
pub fn prose_only(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in blocks(text) {
        match b {
            // Inline code is short and reads as a noun, so it stays; only the
            // backticks go, or "`n log n`" counts as a word with no vowels.
            Block::Prose(p) => out.push_str(&p.replace('`', " ")),
            Block::Code { .. } => out.push_str(" code "),
        }
    }
    out
}

/// How many fenced blocks a question carries. A count, so the manifest can
/// record that a question contained code without recording the code.
pub fn code_blocks(text: &str) -> usize {
    blocks(text).iter().filter(|b| b.is_code()).count()
}

/// Wrap runs of monospaced lines in a fence.
///
/// Used on ingest. A PDF has no idea it contains code, but it does know which
/// glyphs came from a monospaced font, and in a CS assignment that is very
/// nearly the same question. The result is a starting point the instructor
/// edits, not an answer: text extraction loses indentation on some producers
/// and mistakes a monospaced heading for a code line on others.
pub fn fence_monospace(lines: &[(String, bool)]) -> String {
    let mut out = String::new();
    let mut in_code = false;
    for (text, mono) in lines {
        // A blank line inside a run does not end it; a blank line is not
        // monospaced in any producer, and closing on one shreds every
        // function with a paragraph break in it into separate blocks.
        let blank = text.trim().is_empty();
        if *mono && !in_code {
            out.push_str("```\n");
            in_code = true;
        } else if !*mono && !blank && in_code {
            out.push_str("```\n");
            in_code = false;
        }
        out.push_str(text);
        out.push('\n');
    }
    if in_code {
        out.push_str("```\n");
    }
    out
}
