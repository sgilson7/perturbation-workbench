//! The browser's own extraction, split by the core.
//!
//! `tests/ingest.rs` runs against a poppler extraction, which is a proxy.
//! This runs against what pdf.js actually produced, written out by
//! `make test-ui`. Different tool, different line grouping, same nine
//! questions - or the tool and its tests are reading different documents.

use workbench_core::ingest::{ingest, HeadingStyle, Line};

#[test]
fn the_browsers_extraction_splits_the_same_way() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../testing/out/browser.lines.json");
    let Ok(raw) = std::fs::read_to_string(path) else {
        eprintln!("skipped: no browser extraction (run `make test-ui`)");
        return;
    };
    let lines: Vec<Line> = serde_json::from_str(&raw).expect("the extraction should parse");
    let out = ingest(&lines);

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
