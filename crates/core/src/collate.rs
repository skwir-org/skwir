//! The order people expect file names in: "f2" before "f10", "é" beside
//! "e", case ignored. Every list that sorts names by hand uses [`sort_key`]
//! (or [`compare`]) so the panes, archives and comic pages agree.
//!
//! This is not a locale's collation: case is folded with Unicode lowercase
//! and the accents of Latin letters are dropped, which covers the names a
//! file manager mostly shows without a collation database. Names equal
//! under those rules fall back to their raw bytes, so the order is total.
//! A name typed to reach an entry is matched by the same folding
//! ([`Prefix`]), so it reaches the names the order puts beside it.

use std::cmp::Ordering;
use std::ffi::OsStr;

/// Base letters of U+00C0..=U+017F; `.` keeps the lowercase character.
const LATIN: &str = concat!(
    "aaaaaa.ceeeeiiiidnooooo.ouuuuy..",
    "aaaaaa.ceeeeiiiidnooooo.ouuuuy.y",
    "aaaaaaccccccccddddeeeeeeeeeegggggggghhhhiiiiiiiiii..jjkkkllllllllllnnnnnnnnnoooooo..",
    "rrrrrrssssssssttttttuuuuuuuuuuuuwwyyyzzzzzzs",
);

/// `c` case-folded, a Latin letter's accent dropped, handed to `push` a
/// part at a time.
fn fold(c: char, push: &mut impl FnMut(&[u8])) {
    match c {
        'Æ' | 'æ' => push(b"ae"),
        'Œ' | 'œ' => push(b"oe"),
        'ß' => push(b"ss"),
        'Ĳ' | 'ĳ' => push(b"ij"),
        '\u{c0}'..='\u{17f}' => {
            let base = LATIN.as_bytes()[c as usize - 0xc0];
            if base == b'.' {
                for l in c.to_lowercase() {
                    push(l.encode_utf8(&mut [0; 4]).as_bytes());
                }
            } else {
                push(&[base]);
            }
        }
        _ => {
            for l in c.to_lowercase() {
                push(l.encode_utf8(&mut [0; 4]).as_bytes());
            }
        }
    }
}

/// A byte string whose plain byte order is the natural, case-folded order
/// of `name`. Compute it once per name and compare keys, not names.
///
/// Each run of digits becomes a marker that sorts where a digit would, the
/// count of its significant digits, then those digits: a longer number is a
/// larger one, and equal lengths compare digit by digit.
pub fn sort_key(name: &OsStr) -> Box<[u8]> {
    let text = name.to_string_lossy();
    let raw = name.as_encoded_bytes();
    let mut key = Vec::with_capacity(raw.len() * 2 + 1);
    // Digits are ASCII: a run of them is sliced from the text as it is,
    // never copied into a string of its own.
    let bytes = text.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        let b = bytes[at];
        if b.is_ascii_digit() {
            let start = at;
            while bytes.get(at).is_some_and(u8::is_ascii_digit) {
                at += 1;
            }
            let digits = &bytes[start..at];
            let zeros = digits.iter().take_while(|&&d| d == b'0').count();
            let significant = &digits[zeros..];
            key.push(b'0');
            key.push(u8::try_from(significant.len()).unwrap_or(u8::MAX));
            key.extend_from_slice(significant);
        } else if b.is_ascii() {
            key.push(b.to_ascii_lowercase());
            at += 1;
        } else {
            let c = text[at..]
                .chars()
                .next()
                .unwrap_or(char::REPLACEMENT_CHARACTER);
            fold(c, &mut |part| key.extend_from_slice(part));
            at += c.len_utf8();
        }
    }
    // Ties (case, accents, leading zeros) are broken by the name itself.
    key.push(0);
    key.extend_from_slice(raw);
    key.into_boxed_slice()
}

/// Compare two names as [`sort_key`] orders them.
pub fn compare(a: &OsStr, b: &OsStr) -> Ordering {
    sort_key(a).cmp(&sort_key(b))
}

/// The beginning of a name, folded as [`sort_key`] folds names (case, the
/// accents of Latin letters; numbers as they are written): what a name
/// typed to reach an entry matches, so "ecole" reaches "École", which the
/// order puts beside "ecole". Folded once; a name is then compared in
/// place, only as far as the text goes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Prefix {
    folded: Vec<u8>,
}

impl Prefix {
    pub fn new(text: &str) -> Self {
        let mut folded = Vec::with_capacity(text.len());
        for c in text.chars() {
            fold(c, &mut |part| folded.extend_from_slice(part));
        }
        Prefix { folded }
    }

    /// Whether `name` begins with it. A letter folded into two ("æ" into
    /// "ae") is begun by its first.
    pub fn begins(&self, name: &OsStr) -> bool {
        let mut rest = self.folded.as_slice();
        for chunk in name.as_encoded_bytes().utf8_chunks() {
            let invalid = (!chunk.invalid().is_empty()).then_some(char::REPLACEMENT_CHARACTER);
            for c in chunk.valid().chars().chain(invalid) {
                let Some(&next) = rest.first() else {
                    return true;
                };
                if c.is_ascii() {
                    if c.to_ascii_lowercase() as u8 != next {
                        return false;
                    }
                    rest = &rest[1..];
                    continue;
                }
                let mut agrees = true;
                fold(c, &mut |part| {
                    let n = part.len().min(rest.len());
                    if agrees && part[..n] == rest[..n] {
                        rest = &rest[n..];
                    } else {
                        agrees = false;
                    }
                });
                if !agrees {
                    return false;
                }
            }
        }
        rest.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(names: &[&str]) -> Vec<String> {
        let mut v: Vec<&str> = names.to_vec();
        v.sort_by_cached_key(|n| sort_key(OsStr::new(n)));
        v.into_iter().map(str::to_owned).collect()
    }

    #[test]
    fn the_table_covers_its_range() {
        assert_eq!(LATIN.len(), 0x180 - 0xc0);
    }

    #[test]
    fn numbers_sort_by_value() {
        assert_eq!(
            sorted(&["f10", "f2", "f1", "f02", "g", "f"]),
            ["f", "f1", "f02", "f2", "f10", "g"]
        );
        assert_eq!(sorted(&["page 100", "page 9"]), ["page 9", "page 100"]);
    }

    #[test]
    fn case_and_accents_are_folded() {
        assert_eq!(sorted(&["z", "École", "b", "a"]), ["a", "b", "École", "z"]);
        assert_eq!(sorted(&["b", "B", "A", "a"]), ["A", "a", "B", "b"]);
        assert_eq!(
            sorted(&["Straße", "strasse", "strass"]),
            ["strass", "Straße", "strasse"]
        );
    }

    #[test]
    fn a_prefix_begins_names_the_order_puts_beside_it() {
        let begins = |text: &str, name: &str| Prefix::new(text).begins(OsStr::new(name));
        assert!(begins("ecole", "École normale"));
        assert!(begins("ÉCO", "ecole"));
        assert!(begins("strass", "Straße"));
        assert!(begins("a", "Æble"));
        assert!(begins("ae", "Æble"));
        assert!(begins("oeuvre", "Œuvres"));
        assert!(begins("my ", "My file.txt"));
        assert!(begins("", "anything"));
        assert!(begins("été", "Été 2026"));
        assert!(begins("f2", "f2.txt"));
        assert!(!begins("f2", "f02.txt"));
        assert!(!begins("ecoles", "École"));
        assert!(!begins("b", "abc"));
        assert!(begins("aeb", "Æble"));
        assert!(!begins("af", "Æble"));
        assert!(begins("пр", "Привет"));
    }

    #[cfg(unix)]
    #[test]
    fn a_prefix_reads_a_name_that_is_not_utf8() {
        use std::os::unix::ffi::OsStrExt;
        let name = OsStr::from_bytes(b"caf\xe9 cr\xe8me");
        assert!(Prefix::new("caf").begins(name));
        assert!(Prefix::new("caf\u{fffd} c").begins(name));
        assert!(!Prefix::new("café").begins(name));
    }

    #[cfg(unix)]
    #[test]
    fn names_that_are_not_utf8_still_order_totally() {
        use std::os::unix::ffi::OsStrExt;
        let a = OsStr::from_bytes(b"caf\xe9");
        let b = OsStr::from_bytes(b"caf\xe8");
        assert_ne!(compare(a, b), Ordering::Equal);
    }
}
