//! Inputs no file should hold, for tests of what reads files skwir did
//! not write: arbitrary bytes, and a sample file mutated (bytes changed,
//! characters of several bytes put where a parser expects ASCII, cut
//! short). A mutated sample stays close to a real file, so it reaches
//! past a parser's first checks. Behind the `testing` feature: plugins
//! ask for it in their dev-dependencies alone.

use proptest::prelude::*;

/// What is put into a sample: characters of two, three and four bytes,
/// and the punctuation file formats are cut at.
const PUT: &[&str] = &[
    "é", "€", "𝄞", "\u{0}", "\n", "\r", "\t", " ", "\"", "'", "<", ">", "&", ":", ";", ",", "=",
    "[", "]", "{", "}", "(", ")", "-", ".", "/", "\\", "#", "*", "%", "0", "9",
];

/// Arbitrary bytes, at most `max`.
pub fn bytes(max: usize) -> impl Strategy<Value = Vec<u8>> {
    proptest::collection::vec(any::<u8>(), 0..=max)
}

/// `sample` with a few bytes changed, a few pieces of [`PUT`] put in,
/// and, one time in four, cut short.
pub fn mutated(sample: impl Into<Vec<u8>>) -> impl Strategy<Value = Vec<u8>> {
    let sample: std::sync::Arc<[u8]> = sample.into().into();
    let len = sample.len();
    (
        proptest::collection::vec((0..len.max(1), any::<u8>()), 0..8),
        proptest::collection::vec((0..=len, proptest::sample::select(PUT)), 0..6),
        prop_oneof![3 => Just(None), 1 => (0..=len).prop_map(Some)],
    )
        .prop_map(move |(changes, puts, cut)| {
            let mut out = sample.to_vec();
            for (at, byte) in changes {
                if let Some(b) = out.get_mut(at) {
                    *b = byte;
                }
            }
            for (at, piece) in puts {
                let at = at.min(out.len());
                out.splice(at..at, piece.bytes());
            }
            if let Some(cut) = cut {
                out.truncate(cut);
            }
            out
        })
}

/// [`mutated`] for a parser of text: invalid UTF-8 made into U+FFFD.
pub fn mutated_text(sample: impl Into<String>) -> impl Strategy<Value = String> {
    mutated(sample.into().into_bytes()).prop_map(|b| String::from_utf8_lossy(&b).into_owned())
}
