//! Character advance widths, so lines can be wrapped before they are written.
//!
//! Ported from pdf-redactor, where the same table was used to find a glyph
//! inside a text fragment. Here it does the other job: a PDF has no line
//! breaking of its own, so every line in the output is broken here, and a
//! wrapper that guesses widths produces a page whose right margin wanders.
//!
//! Widths are exact for the three text fonts and approximate for Symbol. The
//! consequence of a Symbol approximation is a line that ends a few points early
//! or late, which is invisible; getting it exact would mean shipping the whole
//! AFM table for a font used a handful of times per page.

/// The four base-14 faces the writer uses. Nothing is embedded, so no font
/// program from anywhere else can ride along into the output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Font {
    /// Helvetica. Prose.
    Body,
    /// Helvetica-Bold. Headings and labels.
    Bold,
    /// Courier. Code, where indentation is the meaning.
    Mono,
    /// Symbol. Mathematics, because WinAnsiEncoding has none of it.
    Symbol,
}

impl Font {
    pub fn all() -> [Font; 4] {
        [Font::Body, Font::Bold, Font::Mono, Font::Symbol]
    }

    /// The resource name in the page's font dictionary.
    pub fn tag(self) -> &'static str {
        match self {
            Font::Body => "F1",
            Font::Bold => "F2",
            Font::Mono => "F3",
            Font::Symbol => "F4",
        }
    }

    pub fn base_font(self) -> &'static str {
        match self {
            Font::Body => "Helvetica",
            Font::Bold => "Helvetica-Bold",
            Font::Mono => "Courier",
            Font::Symbol => "Symbol",
        }
    }

    /// Advance of one encoded byte, in ems.
    pub fn advance(self, b: u8) -> f32 {
        match self {
            // Courier is fixed-pitch, which is the entire reason code is set
            // in it: a column of aligned characters stays a column.
            Font::Mono => 0.6,
            Font::Symbol => symbol_advance(b),
            Font::Body | Font::Bold => {
                let table = if self == Font::Bold { &BOLD } else { &REGULAR };
                match b {
                    0x20..=0x7E => table[(b - 0x20) as usize] as f32 / 1000.0,
                    // The Latin-1 range is close enough to the average that a
                    // per-character table earns nothing.
                    _ => 0.556,
                }
            }
        }
    }
}

/// Helvetica advance widths for printable ASCII, in units of 1/1000 em.
const REGULAR: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556,
    556, 556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, 1015, 667, 667, 722,
    722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722,
    667, 944, 667, 667, 611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556, 556, 278, 556,
    556, 222, 222, 500, 222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500,
    500, 334, 260, 334, 584,
];

/// Helvetica-Bold, same range.
const BOLD: [u16; 95] = [
    278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556,
    556, 556, 556, 556, 556, 556, 556, 556, 333, 333, 584, 584, 584, 611, 975, 722, 722, 722,
    722, 667, 611, 778, 722, 278, 556, 722, 611, 833, 722, 778, 667, 778, 722, 667, 611, 722,
    667, 944, 667, 667, 611, 333, 278, 333, 584, 556, 333, 556, 611, 556, 611, 556, 333, 611,
    611, 278, 278, 556, 278, 889, 611, 611, 611, 611, 389, 556, 333, 611, 556, 778, 556, 556,
    500, 389, 280, 389, 584,
];

/// Symbol, for the codes the writer actually emits. Arrows are wide, set
/// operators are wide-ish, everything else is close enough to 600.
fn symbol_advance(b: u8) -> f32 {
    match b {
        0xAB..=0xAF | 0xDB..=0xDF => 0.987, // arrows, single and double
        0xC7 | 0xC8 | 0xC5 | 0xC4 => 0.768, // intersection, union, circled ops
        0xCE | 0xCF | 0xC9..=0xCD => 0.713, // element, subset, superset
        0x22 | 0x24 | 0xD8 | 0xE5 => 0.713, // quantifiers, not, summation
        0xA3 | 0xB3 | 0xB9 | 0xBA | 0xBB => 0.549, // relations
        _ => 0.6,
    }
}

/// Width of a string set in `font`, in ems.
///
/// Measured through the writer's own encoder rather than over the characters,
/// so that a maths symbol is measured in Symbol and a substituted character is
/// measured as the string it will actually become. Two implementations of
/// "which font draws this" would eventually disagree, and the visible result
/// would be a line that overruns the margin.
pub fn width(text: &str, font: Font) -> f32 {
    crate::pdfwrite::encode(text, font)
        .0
        .iter()
        .map(|(f, bytes)| bytes.iter().map(|&b| f.advance(b)).sum::<f32>())
        .sum()
}

/// Width in points at a given size.
pub fn points(text: &str, font: Font, size: f32) -> f32 {
    width(text, font) * size
}
