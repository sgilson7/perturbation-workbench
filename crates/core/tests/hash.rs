//! The digest, the canonical bytes, and the two validated strings.
//!
//! The whole tool rests on a collaborator being able to run `shasum -a 256` on
//! a file and get the number in the manifest. These tests are that claim.

mod common;

use workbench_core::hash::{canonical, sha256_hex, InvalidId, Sha256Hex, Timestamp};

/// The NIST vectors. A hand-written compression function earns its place by
/// being pinned to these; without them "we implemented SHA-256" is an
/// assertion rather than a fact.
#[test]
fn the_digest_matches_the_published_vectors() {
    for (input, expect) in [
        ("", "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"),
        ("abc", "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"),
        (
            "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
        ),
        (
            "abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu",
            "cf5b16a778af8380036ce59e7b0492370b249b11e8f07a51afac45037afee9d1",
        ),
    ] {
        assert_eq!(sha256_hex(input.as_bytes()), expect, "{:?}", input);
    }
    // The multi-block case, where the padding and length encoding matter.
    assert_eq!(
        sha256_hex(&vec![b'a'; 1_000_000]),
        "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
    );
}

/// The number in the manifest has to be the number `shasum` prints. This
/// digest was taken with `shasum -a 256` on the command line, not by this
/// module, so agreement here is agreement with the recipe in the README.
#[test]
fn a_digest_agrees_with_the_command_line_recipe() {
    let text = "Question 4 - The Dessert Menu.\nLet the universe be tray positions.";
    assert_eq!(
        Sha256Hex::of(text.as_bytes()).as_str(),
        "4e5166600a9cc66713e771355d86fa0768207cc3eb8d5850f3024e59a48369d5"
    );
}

/// The round trip a collaborator actually performs: copy the exact query out
/// of the tool, paste it into a file, hash the file. That path rewrites line
/// endings on Windows, and the answer must not change.
#[test]
fn canonicalisation_survives_a_clipboard_round_trip() {
    let unix = "Part 1. Count the sundaes.\nPart 2. Justify the bijection.";
    let windows = "Part 1. Count the sundaes.\r\nPart 2. Justify the bijection.";
    let old_mac = "Part 1. Count the sundaes.\rPart 2. Justify the bijection.";
    let padded = "\n\nPart 1. Count the sundaes.   \nPart 2. Justify the bijection.\t\n\n\n";
    // A PDF text layer routinely emits U+00A0 where a keyboard emits a space.
    let from_pdf = "Part 1. Count\u{00A0}the sundaes.\nPart 2. Justify the bijection.";

    let want = Sha256Hex::of(canonical(unix).as_bytes());
    for other in [windows, old_mac, padded, from_pdf] {
        assert_eq!(Sha256Hex::of(canonical(other).as_bytes()), want, "{:?}", other);
    }
}

/// Indentation inside a question is structure, not noise.
#[test]
fn canonicalisation_leaves_the_text_itself_alone() {
    let text = "Part 1.\n    (a) the first case\n    (b) the second case";
    assert_eq!(canonical(text), text);
    // Blank lines *inside* a question are kept; only the ends are trimmed.
    assert_eq!(canonical("a b\n\nc d\n"), "a b\n\nc d");
}

/// The hash fields are the only free-form strings the manifest tolerates, so
/// they are the only route by which question text could reach it.
#[test]
fn a_question_cannot_be_smuggled_through_a_hash_field() {
    for bad in [
        "How many sundaes can you make?",
        "",
        "4e5166600a9cc66713e771355d86fa0768207cc3eb8d5850f3024e59a48369d",  // 63
        "4e5166600a9cc66713e771355d86fa0768207cc3eb8d5850f3024e59a48369d55", // 65
        "4E5166600A9CC66713E771355D86FA0768207CC3EB8D5850F3024E59A48369D5", // upper case
    ] {
        assert_eq!(Sha256Hex::parse(bad), Err(InvalidId::NotAHash), "{:?}", bad);
    }
    // Right length, wrong alphabet - still refused.
    assert_eq!(Sha256Hex::parse(&"z".repeat(64)), Err(InvalidId::NotAHash));
    assert!(Sha256Hex::parse(&"a".repeat(64)).is_ok());
}

#[test]
fn a_timestamp_is_a_utc_instant_and_nothing_else() {
    assert!(Timestamp::parse("2026-08-27T14:02:11Z").is_ok());
    for bad in [
        "Gemini said the answer was 256",
        "2026-08-27T14:02:11.500Z",   // fractional seconds break string ordering
        "2026-08-27T14:02:11+05:00",  // an offset is a different instant per reader
        "2026-08-27 14:02:11Z",
        "2026-13-27T14:02:11Z",
        "2026-08-32T14:02:11Z",
        "2026-08-27T24:02:11Z",
        "2026-08-27T14:60:11Z",
        "",
    ] {
        assert_eq!(Timestamp::parse(bad), Err(InvalidId::NotAnInstant), "{:?}", bad);
    }
}

/// The reason the format is that narrow: comparing two stamps as strings has
/// to be comparing two instants, because that is how the protocol refuses an
/// attempt graded against a rubric revision that did not exist yet.
#[test]
fn timestamps_sort_chronologically_as_strings() {
    let mut stamps: Vec<Timestamp> = [
        "2026-08-27T14:02:11Z",
        "2025-12-31T23:59:59Z",
        "2026-08-27T09:00:00Z",
        "2026-01-01T00:00:00Z",
    ]
    .iter()
    .map(|s| Timestamp::parse(s).unwrap())
    .collect();
    stamps.sort();
    let order: Vec<&str> = stamps.iter().map(|t| t.as_str()).collect();
    assert_eq!(
        order,
        [
            "2025-12-31T23:59:59Z",
            "2026-01-01T00:00:00Z",
            "2026-08-27T09:00:00Z",
            "2026-08-27T14:02:11Z"
        ]
    );
}

/// Exported query files are named by hash prefix, so the prefix has to be
/// stable and long enough that a nine-question set cannot collide.
#[test]
fn a_digest_offers_a_short_form_for_file_names() {
    let h = Sha256Hex::of(b"abc");
    assert_eq!(h.short(), "ba7816bf");
    assert_eq!(h.short().len(), 8);
}
