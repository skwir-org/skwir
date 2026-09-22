//! The order people expect file names in: "f2" before "f10", "é" beside
//! "e", case ignored. Every list that sorts names by hand uses [`sort_key`]
//! (or [`compare`]) so the panes, archives and comic pages agree.
//!
//! This is not a locale's collation: case is folded with Unicode lowercase
//! and the accents of Latin letters are dropped, which covers the names a
//! file manager mostly shows without a collation database. Names equal
//! under those rules fall back to their raw bytes, so the order is total.

use std::cmp::Ordering;
use std::ffi::OsStr;

/// Base letters of U+00C0..=U+017F; `.` keeps the lowercase character.
const LATIN: &str = concat!(
    "aaaaaa.ceeeeiiiidnooooo.ouuuuy..",
    "aaaaaa.ceeeeiiiidnooooo.ouuuuy.y",
    "aaaaaaccccccccddddeeeeeeeeeegggggggghhhhiiiiiiiiii..jjkkkllllllllllnnnnnnnnnoooooo..",
    "rrrrrrssssssssttttttuuuuuuuuuuuuwwyyyzzzzzzs",
);

fn fold(c: char, out: &mut Vec<u8>) {
    let mut push = |s: &str| out.extend_from_slice(s.as_bytes());
    match c {
        'Æ' | 'æ' => push("ae"),
        'Œ' | 'œ' => push("oe"),
        'ß' => push("ss"),
        'Ĳ' | 'ĳ' => push("ij"),
        '\u{c0}'..='\u{17f}' => {
            let base = LATIN.as_bytes()[c as usize - 0xc0];
            if base == b'.' {
                for l in c.to_lowercase() {
                    push(l.encode_utf8(&mut [0; 4]));
                }
            } else {
                out.push(base);
            }
        }
        _ => {
            for l in c.to_lowercase() {
                push(l.encode_utf8(&mut [0; 4]));
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
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() {
            let mut digits = String::from(c);
            while let Some(d) = chars.next_if(char::is_ascii_digit) {
                digits.push(d);
            }
            let significant = digits.trim_start_matches('0');
            key.push(b'0');
            key.push(u8::try_from(significant.len()).unwrap_or(u8::MAX));
            key.extend_from_slice(significant.as_bytes());
        } else {
            fold(c, &mut key);
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

    #[cfg(unix)]
    #[test]
    fn names_that_are_not_utf8_still_order_totally() {
        use std::os::unix::ffi::OsStrExt;
        let a = OsStr::from_bytes(b"caf\xe9");
        let b = OsStr::from_bytes(b"caf\xe8");
        assert_ne!(compare(a, b), Ordering::Equal);
    }
}
