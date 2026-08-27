//! The browser's own extraction, split by the core.
//!
//! `tests/ingest.rs` runs against a poppler extraction, which is a proxy.
//! This runs against what pdf.js actually produced, written out by
//! `make test-ui`. Different tool, different line grouping, same nine
//! questions - or the tool and its tests are reading different documents.

use workbench_core::ingest::{ingest, HeadingStyle, Ingested, Line};
use workbench_core::markup::code_blocks;

/// What `make test-ui` wrote out for one document, if it ran.
fn extracted(stem: &str) -> Option<Ingested> {
    let path = format!(
        "{}/../../testing/out/{}.lines.json",
        env!("CARGO_MANIFEST_DIR"),
        stem
    );
    let raw = std::fs::read_to_string(&path).ok()?;
    let lines: Vec<Line> = serde_json::from_str(&raw).expect("the extraction should parse");
    Some(ingest(&lines))
}

/// The synthetic assignment, which CI always has. It carries a Courier block,
/// which is the only exercise the monospaced-run detection gets in a browser.
#[test]
fn the_browser_finds_the_code_in_a_code_bearing_assignment() {
    let Some(out) = extracted("sample-assignment") else {
        eprintln!("skipped: no browser extraction (run `make test-ui`)");
        return;
    };
    assert_eq!(out.style, Some(HeadingStyle::Question));
    assert_eq!(out.drafts.iter().map(|d| d.ordinal).collect::<Vec<_>>(), [1, 2, 3]);
    assert_eq!(out.pages, 2);

    // The running footer changes page number, so it is only found by shape.
    for d in &out.drafts {
        assert!(!d.text.contains("CSC 999 Faculty"), "Q{} kept the footer", d.ordinal);
        assert!(d.text.contains("Part 1"), "Q{} lost its parts", d.ordinal);
    }
    assert!(out.furniture_dropped >= 4, "{} furniture lines", out.furniture_dropped);

    // pdf.js said those lines were Courier, and the splitter fenced them.
    assert!(out.code_lines >= 5, "{} code lines", out.code_lines);
    let q2 = out.drafts.iter().find(|d| d.ordinal == 2).unwrap();
    assert_eq!(code_blocks(&q2.text), 1, "{}", q2.text);
    assert!(q2.text.contains("\npublic static int sum"), "the block kept a page margin:\n{}", q2.text);
    assert!(q2.text.contains("        total += x;"), "indentation lost:\n{}", q2.text);
    assert_eq!(code_blocks(&out.drafts[0].text), 0, "prose was fenced as code");
}

/// The real lab, when it is on this machine. `tests/ingest.rs` runs against a
/// poppler extraction of the same file; if these two ever disagree, the tool
/// and its tests are reading different documents.
#[test]
fn the_browsers_extraction_of_the_real_lab_splits_the_same_way() {
    let Some(out) = extracted("Lab3-Part2 (3)") else {
        eprintln!("skipped: the lab PDF is not on this machine");
        return;
    };
    assert_eq!(out.style, Some(HeadingStyle::Question));
    assert_eq!(
        out.drafts.iter().map(|d| d.ordinal).collect::<Vec<_>>(),
        [4, 5, 6, 7, 8, 9, 10, 11, 12]
    );
    for d in &out.drafts {
        assert!(!d.text.contains('\u{25A1}'), "Q{} kept a checkbox", d.ordinal);
        assert!(!d.text.contains("NC State University"), "Q{} kept the footer", d.ordinal);
        assert!(d.text.contains("Part 1"), "Q{} lost its parts", d.ordinal);
    }
}
