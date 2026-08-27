//! The two validated identifiers, and the bytes they are taken over.
//!
//! Everything this tool claims rests on one sentence: *the same bytes were
//! prompted three times*. That sentence is only checkable if a collaborator at
//! another institution can run `shasum -a 256` on a file and get the number in
//! the manifest. So the digest is not an implementation detail — it is the
//! evidence, and the rules for producing it belong here, under `cargo test`,
//! rather than in JavaScript where they would be unverifiable.
//!
//! SHA-256 is implemented here rather than pulled in, for the same reason
//! `pdfwrite` does not use a PDF library: the claim being made is about exactly
//! these bytes, and a claim you can check by reading one file is worth more
//! than one that depends on a version bump elsewhere. The compression function
//! is sixty lines and is pinned by the NIST vectors in `tests/hash.rs`. Nothing
//! secret is ever hashed here, so there is no timing surface to get wrong.
//!
//! `Sha256Hex` and `Timestamp` live together because they are the same kind of
//! thing: the only free-form strings the manifest tolerates, and therefore the
//! only route by which question text could get into it. Both are validated on
//! construction, so a caller cannot smuggle prose through a field typed as one.

use std::fmt;

// ---------------------------------------------------------------- digest

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// SHA-256 of `bytes`, lowercase hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];

    // Message padding: a 0x80 byte, zeros, then the bit length big-endian.
    let mut msg = bytes.to_vec();
    let bit_len = (bytes.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for block in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                block[i * 4], block[i * 4 + 1], block[i * 4 + 2], block[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let (mut a, mut b, mut c, mut d) = (h[0], h[1], h[2], h[3]);
        let (mut e, mut f, mut g, mut hh) = (h[4], h[5], h[6], h[7]);
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g; g = f; f = e;
            e = d.wrapping_add(t1);
            d = c; c = b; b = a;
            a = t1.wrapping_add(t2);
        }
        for (slot, v) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *slot = slot.wrapping_add(v);
        }
    }

    let mut out = String::with_capacity(64);
    for word in h {
        out.push_str(&format!("{:08x}", word));
    }
    out
}

// ---------------------------------------------------------------- canonical bytes

/// The bytes a query is identified by.
///
/// Not the raw text. A collaborator's check is: copy the exact query out of the
/// tool, paste it into a file, `shasum` it, compare. That round trip passes
/// through a clipboard and a text editor, either of which will rewrite line
/// endings — so hashing raw bytes would tell an honest collaborator the query
/// had been altered when nothing had changed but CRLF. The differences folded
/// here are exactly the ones no reader can see and no model can act on:
///
/// * `\r\n` and lone `\r` become `\n`;
/// * a non-breaking space becomes a space, because PDF text layers emit U+00A0
///   for ordinary spaces routinely and the query would otherwise change
///   identity depending on whether it came from a PDF or a keyboard;
/// * trailing whitespace is dropped from every line, and blank lines from
///   both ends.
///
/// Indentation is left alone: leading spaces inside a question are structure.
///
/// The canonical form is what gets stored, shown, copied, exported, and hashed,
/// so there is no second form anywhere for the two to disagree about.
pub fn canonical(text: &str) -> String {
    let unified = text.replace("\r\n", "\n").replace('\r', "\n").replace('\u{00A0}', " ");
    let mut lines: Vec<&str> = unified.lines().map(|l| l.trim_end()).collect();
    while lines.first().is_some_and(|l| l.is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines.join("\n")
}

// ---------------------------------------------------------------- validated strings

/// Something that failed to be the identifier it was typed as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvalidId {
    /// Not 64 lowercase hex digits.
    NotAHash,
    /// Not `YYYY-MM-DDTHH:MM:SSZ`, or a field out of range.
    NotAnInstant,
}

/// A SHA-256 digest, in hex, that has been checked to be one.
///
/// The point is not tidiness. This is the type on every field where a hash is
/// expected, so "the response was recorded" cannot be satisfied by a caller
/// passing the response.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
pub struct Sha256Hex(String);

impl Sha256Hex {
    pub fn parse(s: &str) -> Result<Sha256Hex, InvalidId> {
        let ok = s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if ok {
            Ok(Sha256Hex(s.to_string()))
        } else {
            Err(InvalidId::NotAHash)
        }
    }

    /// Hash `bytes` and wrap the result. The only constructor that cannot fail,
    /// because it is the only one that does not take the caller's word for it.
    pub fn of(bytes: &[u8]) -> Sha256Hex {
        Sha256Hex(sha256_hex(bytes))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The first eight characters, used to name exported query files. Short
    /// enough to type, long enough that a nine-question set will not collide.
    pub fn short(&self) -> &str {
        &self.0[..8]
    }
}

impl fmt::Display for Sha256Hex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for Sha256Hex {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Sha256Hex::parse(&s).map_err(|_| serde::de::Error::custom("not a sha-256 hex digest"))
    }
}

/// A UTC instant at seconds resolution: `2026-08-27T14:02:11Z`.
///
/// Deliberately narrower than RFC 3339 allows. An offset such as `+05:00` and a
/// fractional second are both legal there and both break the property this type
/// exists for: every timestamp in a run has the same length and the same
/// alphabet, so **comparing two of them as strings is comparing two instants**.
/// That is what lets `protocol` refuse an attempt graded against a rubric
/// revision that did not exist yet, and `verify` re-check it later, without a
/// date library in a crate that has to compile to wasm.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
pub struct Timestamp(String);

impl Timestamp {
    pub fn parse(s: &str) -> Result<Timestamp, InvalidId> {
        let b = s.as_bytes();
        if b.len() != 20 {
            return Err(InvalidId::NotAnInstant);
        }
        let digits_at = |i: usize| b[i].is_ascii_digit();
        let shape = (0..4).all(digits_at)
            && b[4] == b'-' && digits_at(5) && digits_at(6)
            && b[7] == b'-' && digits_at(8) && digits_at(9)
            && b[10] == b'T' && digits_at(11) && digits_at(12)
            && b[13] == b':' && digits_at(14) && digits_at(15)
            && b[16] == b':' && digits_at(17) && digits_at(18)
            && b[19] == b'Z';
        if !shape {
            return Err(InvalidId::NotAnInstant);
        }
        let field = |from: usize, to: usize| s[from..to].parse::<u32>().unwrap_or(u32::MAX);
        let (month, day) = (field(5, 7), field(8, 10));
        let (hour, min, sec) = (field(11, 13), field(14, 16), field(17, 19));
        // Range-checked so a wrong-but-well-shaped stamp cannot order strangely.
        // Leap seconds are allowed through at :60 rather than argued with.
        let sane = (1..=12).contains(&month)
            && (1..=31).contains(&day)
            && hour <= 23
            && min <= 59
            && sec <= 60;
        if sane {
            Ok(Timestamp(s.to_string()))
        } else {
            Err(InvalidId::NotAnInstant)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for Timestamp {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Timestamp::parse(&s).map_err(|_| serde::de::Error::custom("not a UTC instant"))
    }
}
