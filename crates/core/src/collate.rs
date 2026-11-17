//! The order people expect file names in: "f2" before "f10", "é" beside
//! "e", case after the letters. Every list that sorts names by hand uses
//! [`sort_key`] (or [`compare`]) so the panes, archives, comic pages,
//! searches' trees, tags and groups agree; a name typed to reach an entry
//! is matched by the same collation ([`Prefix`]), and a group by name is
//! the letter it puts a name under ([`initial`]).
//!
//! **The locale's collation** (ADR-139, 15), set once as Skwir starts
//! ([`set_locale`]): the formats' locale, not the window's language
//! (`LC_COLLATE` on Linux, the region on macOS and Windows,
//! `skwir_platform::locale`). ICU4X's collator, with numbers by value and
//! at its third strength: the base letters decide, then the accents, then
//! the case, as Finder, Explorer and Dolphin order names, each blind to
//! case and seeing accents, a tie then broken. Spaces and punctuation
//! count, before digits and letters (a locale's default that ignores them,
//! Thai's, is not taken: "a b" and "ab" are two names, and a space typed
//! reaches only names that go on with one). Its keys are held in memory,
//! never written: a new ICU4X may change their bytes.
//!
//! **Folded**, for the C and POSIX locales, where none is said, and until
//! one is set: case folded with Unicode lowercase and the accents of Latin
//! letters dropped (written with their letter or apart from it, as a
//! combining mark), which covers the names a file manager mostly shows
//! without a collation database; numbers by value.
//!
//! Either way, names equal under it fall back to their raw bytes, so the
//! order is total. A name that is not UTF-8 is ordered by what it reads as,
//! each byte that is no character read as U+FFFD (which both put after
//! every letter), then by its bytes: among the others, beside the names it
//! begins as.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::sync::{Arc, OnceLock};

use icu_collator::options::{AlternateHandling, CollatorOptions, Strength};
use icu_collator::preferences::CollationNumericOrdering;
use icu_collator::{CollationKeySink, Collator, CollatorBorrowed, CollatorPreferences};
use parking_lot::RwLock;

/// Base letters of U+00C0..=U+017F; `.` keeps the lowercase character.
const LATIN: &str = concat!(
    "aaaaaa.ceeeeiiiidnooooo.ouuuuy..",
    "aaaaaa.ceeeeiiiidnooooo.ouuuuy.y",
    "aaaaaaccccccccddddeeeeeeeeeegggggggghhhhiiiiiiiiii..jjkkkllllllllllnnnnnnnnnoooooo..",
    "rrrrrrssssssssttttttuuuuuuuuuuuuwwyyyzzzzzzs",
);

/// `c` case-folded, a Latin letter's accent dropped, handed to `push` a
/// part at a time. An accent written apart from its letter, as a name in
/// decomposed form has it ("e" then U+0301, as macOS's HFS+ writes names),
/// is a combining mark and folds to nothing, so "école" folds alike
/// written either way.
fn fold(c: char, push: &mut impl FnMut(&[u8])) {
    match c {
        // Combining Diacritical Marks.
        '\u{300}'..='\u{36f}' => {}
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

/// How this process orders names, set once ([`set_locale`]).
static COLLATION: OnceLock<Collation> = OnceLock::new();

/// Order names by `locale`'s collation from now on: a BCP 47 tag, as
/// `skwir_platform::locale::of(Category::Collate)` reads it; `None` (the C
/// and POSIX locales, or none said) keeps them folded. Once, as Skwir
/// starts, before any name is keyed: a listing compares the keys it holds,
/// and keys of two collations do not compare. False where the order was
/// set already (asked twice, or a name keyed first), which keeps it.
pub fn set_locale(locale: Option<&str>) -> bool {
    let set = COLLATION.set(Collation::of(locale)).is_ok();
    if !set {
        tracing::warn!(?locale, "names' order already set, kept");
    }
    set
}

/// How this process orders names: the locale's, once set; folded before.
pub fn current() -> &'static Collation {
    COLLATION.get_or_init(Collation::folded)
}

/// Read what a beginning typed is first sought with, by this process's
/// order (its ASCII's weights, a few milliseconds by a locale), on a
/// worker as Skwir starts, so that the first name typed to reach an entry
/// does not wait for it on the window's thread. Nothing for folded names.
pub fn prewarm() {
    if let Some(collators) = &current().0 {
        collators
            .ascii
            .get_or_init(|| Ascii::of(&collators.primary));
    }
}

/// A byte string whose plain byte order is the order of `name` (this
/// process's collation, [`current`]). Compute it once per name and compare
/// keys, not names; make it off the window's thread where there are many.
pub fn sort_key(name: &OsStr) -> Box<[u8]> {
    current().sort_key(name)
}

/// Compare two names as [`sort_key`] orders them.
pub fn compare(a: &OsStr, b: &OsStr) -> Ordering {
    current().compare(a, b)
}

/// The letter a name is under where names are grouped by their first, as
/// [`current`] orders them ([`Collation::initial`]).
pub fn initial(name: &OsStr) -> Initial {
    current().initial(name)
}

/// Where a name is grouped by its first character (`Group by › Name`):
/// before the digits what begins with neither a digit nor a letter, then
/// the digits, then each letter, the letters as the order puts them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Initial {
    /// Punctuation, a symbol, a space, a mark: what begins with neither.
    Other,
    /// A digit.
    Digit,
    /// A letter, uppercase as a heading says it: the one the order puts it
    /// under ("E" for "é"; "Ä" in Swedish, a letter after "Z", "A" in
    /// German). Groups of letters are ordered by the letter's
    /// [`sort_key`].
    Letter(char),
}

/// The ICU4X collators of a locale: its order, and its first strength for
/// what is typed.
struct Collators {
    locale: Box<str>,
    /// Names' order: numbers by value, third strength.
    order: CollatorBorrowed<'static>,
    /// What a typed beginning is matched by: the base letters alone,
    /// numbers as they are written ("f2" begins "f20", not "f02").
    primary: CollatorBorrowed<'static>,
    /// The letter a name beginning with each character is under, as read
    /// ([`Collation::initial`]): a few searches the first time, a look-up
    /// then.
    initials: RwLock<HashMap<char, Initial>>,
    /// "A" to "Z" in the locale's order ("Y" after "I" in Lithuanian, "Z"
    /// after "S" in Estonian), one of each base letter: what a Latin
    /// letter is grouped under is the last of them at or before it.
    latin: Vec<char>,
    /// Printable ASCII's weights at the first strength, for the walk that
    /// turns a name away before ICU4X reads it ([`Prefix::begins`]):
    /// made the first time a beginning is sought.
    ascii: OnceLock<Ascii>,
}

/// Printable ASCII characters as classes of their weight at the first
/// strength: two of one class are one base letter ("a" and "A"); none (0)
/// for a character outside it, or one that begins a contraction in the
/// locale ("c" of Czech's "ch", "a" of Danish's "aa"), whose weight
/// depends on what follows. Each ASCII character has a weight of its own
/// in every locale CLDR has (none expands into two).
struct Ascii([u16; 128]);

impl Ascii {
    /// Read from `primary` (about 18,000 keys of two characters: a few
    /// milliseconds, once).
    fn of(primary: &CollatorBorrowed<'static>) -> Ascii {
        let key = |text: &str| {
            let mut key = Vec::new();
            let Ok(()) = primary.write_sort_key_to(text, &mut key);
            key
        };
        let printable = || (b' '..=b'~').map(char::from);
        let mut classes: Vec<Vec<u8>> = Vec::new();
        let mut table = [0; 128];
        for x in printable() {
            let own = key(x.encode_utf8(&mut [0; 4]));
            // Followed by each, then by each apart (a combining grapheme
            // joiner, which no contraction spans): a difference is a
            // contraction it begins.
            let begins_one =
                printable().any(|y| key(&format!("{x}{y}")) != key(&format!("{x}\u{34f}{y}")));
            if own.is_empty() || begins_one {
                continue;
            }
            let class = classes.iter().position(|c| *c == own).unwrap_or_else(|| {
                classes.push(own);
                classes.len() - 1
            });
            table[usize::from(u8::try_from(x).unwrap_or_default())] =
                u16::try_from(class + 1).unwrap_or_default();
        }
        Ascii(table)
    }

    fn class(&self, b: u8) -> u16 {
        self.0.get(usize::from(b)).copied().unwrap_or(0)
    }
}

/// How names are ordered: a locale's collation, or folded. One is this
/// process's ([`current`]); others are made for a test or a measure.
#[derive(Clone)]
pub struct Collation(Option<Arc<Collators>>);

impl std::fmt::Debug for Collation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            Some(collators) => write!(f, "Collation({})", collators.locale),
            None => f.write_str("Collation(folded)"),
        }
    }
}

impl Collation {
    /// Names folded: the C locale's order.
    pub fn folded() -> Self {
        Collation(None)
    }

    /// `locale`'s order (a BCP 47 tag); folded for `None`. A tag ICU4X
    /// cannot read, or a language it has no order of its own for, orders
    /// as its root does (CLDR's, which English uses).
    pub fn of(locale: Option<&str>) -> Self {
        let Some(tag) = locale else {
            return Collation::folded();
        };
        let parsed: icu_locale_core::Locale = tag.parse().unwrap_or_else(|_| {
            tracing::warn!(tag, "a locale ICU4X cannot read: names in its root's order");
            icu_locale_core::Locale::UNKNOWN
        });
        let made = |numeric: bool, strength: Strength| {
            let mut prefs: CollatorPreferences = (&parsed).into();
            prefs.numeric_ordering = Some(if numeric {
                CollationNumericOrdering::True
            } else {
                CollationNumericOrdering::False
            });
            let mut options = CollatorOptions::default();
            options.strength = Some(strength);
            options.alternate_handling = Some(AlternateHandling::NonIgnorable);
            Collator::try_new(prefs, options)
        };
        match (
            made(true, Strength::Tertiary),
            made(false, Strength::Primary),
        ) {
            (Ok(order), Ok(primary)) => {
                let mut latin: Vec<char> = ('A'..='Z').collect();
                let mut buf = ([0; 4], [0; 4]);
                let mut by_base = |a: &char, b: &char| {
                    primary.compare(a.encode_utf8(&mut buf.0), b.encode_utf8(&mut buf.1))
                };
                latin.sort_by(&mut by_base);
                latin.dedup_by(|b, a| by_base(a, b).is_eq());
                Collation(Some(Arc::new(Collators {
                    locale: tag.into(),
                    order,
                    primary,
                    initials: RwLock::default(),
                    latin,
                    ascii: OnceLock::new(),
                })))
            }
            (Err(error), _) | (_, Err(error)) => {
                tracing::warn!(tag, %error, "no collation: names folded");
                Collation::folded()
            }
        }
    }

    /// Whether it folds names (the C locale's order).
    pub fn is_folded(&self) -> bool {
        self.0.is_none()
    }

    /// A byte string whose plain byte order is the order of `name`: its
    /// collation's key, then a zero, then the name's bytes, which break
    /// ties. Neither key holds a zero, so one that is the start of another
    /// still sorts first.
    ///
    /// Folded, each run of digits becomes a marker that sorts where a digit
    /// would, the count of its significant digits, then those digits: a
    /// longer number is a larger one, and equal lengths compare digit by
    /// digit.
    pub fn sort_key(&self, name: &OsStr) -> Box<[u8]> {
        let raw = name.as_encoded_bytes();
        let Some(collators) = &self.0 else {
            return folded_key(name);
        };
        // ICU4X's keys weigh 1.2 times the names' bytes on `flat100k`, 1.4
        // on the research's names (ADR-139): the name's bytes follow.
        let mut key = Vec::with_capacity(raw.len() * 3 + 2);
        let Ok(()) = collators.order.write_sort_key_utf8_to(raw, &mut key);
        key.push(0);
        key.extend_from_slice(raw);
        key.into_boxed_slice()
    }

    /// Compare two names as [`Collation::sort_key`] orders them: without
    /// keys, so as cheap for one comparison as ICU4X's own.
    pub fn compare(&self, a: &OsStr, b: &OsStr) -> Ordering {
        let (ra, rb) = (a.as_encoded_bytes(), b.as_encoded_bytes());
        match &self.0 {
            None => self.sort_key(a).cmp(&self.sort_key(b)),
            Some(collators) => collators
                .order
                .compare_utf8(ra, rb)
                .then_with(|| ra.cmp(rb)),
        }
    }

    /// The beginning `text` of a name, as this order matches it.
    pub fn prefix(&self, text: &str) -> Prefix {
        let Some(collators) = &self.0 else {
            let mut folded = Vec::with_capacity(text.len());
            for c in text.chars() {
                fold(c, &mut |part| folded.extend_from_slice(part));
            }
            return Prefix {
                sought: folded,
                typed: Box::default(),
                collators: None,
            };
        };
        collators
            .ascii
            .get_or_init(|| Ascii::of(&collators.primary));
        let mut sought = Vec::with_capacity(text.len() * 2);
        let Ok(()) = collators.primary.write_sort_key_to(text, &mut sought);
        Prefix {
            sought,
            typed: text.as_bytes().into(),
            collators: Some(Arc::clone(collators)),
        }
    }

    /// The letter `name` is grouped under, by its first character alone.
    ///
    /// Folded: the first character folded ("é" under "E", "æ" under "A"),
    /// a digit, or neither. By a locale: a letter the order puts among "A"
    /// to "Z" under the last of them at or before it ("é" and "ß" under
    /// "E" and "S"; "Ä" under "A" in German, while Swedish orders it after
    /// "Z", a letter of its own), as Dolphin groups them; a letter of
    /// another script under itself, uppercase.
    pub fn initial(&self, name: &OsStr) -> Initial {
        let Some(c) = first_char(name) else {
            return Initial::Other;
        };
        let Some(collators) = &self.0 else {
            if c.is_ascii_digit() {
                return Initial::Digit;
            }
            let mut first = None;
            fold(c, &mut |part| {
                if first.is_none() {
                    first = String::from_utf8_lossy(part).chars().next();
                }
            });
            return match first {
                Some(f) if f.is_alphabetic() => Initial::Letter(upper(f)),
                _ => Initial::Other,
            };
        };
        if let Some(&known) = collators.initials.read().get(&c) {
            return known;
        }
        let initial = collators.initial_of(c);
        collators.initials.write().insert(c, initial);
        initial
    }
}

/// `c` uppercase, as one character: its first where it has several.
fn upper(c: char) -> char {
    c.to_uppercase().next().unwrap_or(c)
}

/// The first character of `name`, U+FFFD where it begins with bytes that
/// are no character.
fn first_char(name: &OsStr) -> Option<char> {
    let chunk = name.as_encoded_bytes().utf8_chunks().next()?;
    Some(
        chunk
            .valid()
            .chars()
            .next()
            .unwrap_or(char::REPLACEMENT_CHARACTER),
    )
}

impl Collators {
    fn initial_of(&self, c: char) -> Initial {
        if c.is_alphabetic() {
            let mut buf = [0; 4];
            let text: &str = c.encode_utf8(&mut buf);
            let primary = |letter: char| {
                let mut buf = [0; 4];
                self.primary.compare(letter.encode_utf8(&mut buf), text)
            };
            // Among "A" to "Z" when the first of them is at or before it
            // and the last at or after: then under the last letter at or
            // before it.
            let (Some(&first), Some(&last)) = (self.latin.first(), self.latin.last()) else {
                return Initial::Letter(upper(c));
            };
            if primary(first).is_le() && primary(last).is_ge() {
                let under = self.latin.partition_point(|&l| primary(l).is_le());
                return Initial::Letter(self.latin[under.saturating_sub(1)]);
            }
            return Initial::Letter(upper(c));
        }
        if c.is_numeric() {
            return Initial::Digit;
        }
        Initial::Other
    }
}

/// The folded key ([`Collation::folded`]).
fn folded_key(name: &OsStr) -> Box<[u8]> {
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

/// The beginning of a name, as the order matches it: what a name typed to
/// reach an entry matches, so "ecole" reaches "École", which the order
/// puts beside "ecole". Made once ([`Prefix::new`]); a name is then
/// compared in place, only as far as the text goes.
///
/// By a locale, a name begins with it when the order's base letters of
/// the name begin with the text's (ICU4X's first strength: case and
/// accents aside, numbers as they are written). So it reaches exactly the
/// names the order puts beside the text: in Swedish "a" does not reach
/// "Ärlig", which comes after "z"; in German it does. Folded, the text and
/// the name are folded alike.
#[derive(Clone, Default)]
pub struct Prefix {
    /// The text folded, or its key at the first strength.
    sought: Vec<u8>,
    /// The text as typed, by a locale: walked beside a name's beginning
    /// while both are ASCII ([`Prefix::turned_away`]).
    typed: Box<[u8]>,
    /// The locale's collators, or none: folded.
    collators: Option<Arc<Collators>>,
}

impl PartialEq for Prefix {
    fn eq(&self, other: &Self) -> bool {
        self.sought == other.sought
            && self.typed == other.typed
            && match (&self.collators, &other.collators) {
                (None, None) => true,
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                _ => false,
            }
    }
}

impl Eq for Prefix {}

impl std::fmt::Debug for Prefix {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Prefix")
            .field("sought", &self.sought)
            .field("typed", &String::from_utf8_lossy(&self.typed))
            .field(
                "collators",
                &self.collators.as_ref().map(|c| c.locale.as_ref()),
            )
            .finish()
    }
}

/// Why writing a name's key stopped: what it began with was known.
enum Stop {
    Begins,
    Differs,
}

/// A sink for a name's key at the first strength that stops it as soon as
/// it begins with what is sought, or cannot.
struct Begins<'a> {
    sought: &'a [u8],
}

impl CollationKeySink for Begins<'_> {
    type Error = Stop;
    /// How much of what is sought was written.
    type State = usize;
    type Output = usize;

    fn write(&mut self, at: &mut usize, buf: &[u8]) -> Result<(), Stop> {
        for &b in buf {
            match self.sought.get(*at) {
                None => return Err(Stop::Begins),
                Some(&s) if s == b => *at += 1,
                Some(_) => return Err(Stop::Differs),
            }
        }
        Ok(())
    }

    fn finish(&mut self, at: usize) -> Result<usize, Stop> {
        Ok(at)
    }
}

impl Prefix {
    /// `text`, as this process's order matches it ([`current`]).
    pub fn new(text: &str) -> Self {
        current().prefix(text)
    }

    /// Whether `name` begins with it. A letter folded into two ("æ" into
    /// "ae") is begun by its first.
    pub fn begins(&self, name: &OsStr) -> bool {
        match &self.collators {
            Some(collators) => {
                let name = name.as_encoded_bytes();
                if collators
                    .ascii
                    .get()
                    .is_some_and(|ascii| self.turned_away(ascii, name))
                {
                    return false;
                }
                let mut sink = Begins {
                    sought: &self.sought,
                };
                match collators.primary.write_sort_key_utf8_to(name, &mut sink) {
                    Err(Stop::Begins) => true,
                    Err(Stop::Differs) => false,
                    Ok(written) => written == self.sought.len(),
                }
            }
            None => self.begins_folded(name),
        }
    }

    /// Whether `name` cannot begin with it, told without ICU4X where the
    /// text and the name begin alike in printable ASCII that begins no
    /// contraction (`Ascii`), and then differ by a character's weight: about
    /// a twentieth of what ICU4X takes (`collate_prefix_us`), as a name
    /// typed is sought in every name of a folder at each key. Not told (false) where either goes on with
    /// what may change the weight it differs by (a combining mark: "a" then
    /// U+030A is Swedish's "å"), nor where it would begin: ICU4X then says.
    fn turned_away(&self, ascii: &Ascii, name: &[u8]) -> bool {
        let typed = &self.typed;
        for (at, (&t, &n)) in typed.iter().zip(name).enumerate() {
            let (of_typed, of_name) = (ascii.class(t), ascii.class(n));
            if of_typed == 0 || of_name == 0 {
                return false;
            }
            if of_typed != of_name {
                let settled = |text: &[u8]| text.get(at + 1).is_none_or(u8::is_ascii);
                return settled(typed) && settled(name);
            }
        }
        false
    }

    fn begins_folded(&self, name: &OsStr) -> bool {
        let mut rest = self.sought.as_slice();
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

/// `text` folded as folded names are (case, the accents of Latin letters),
/// numbers as they are written: what a text is compared as where words are
/// typed to find it ([`Words`]). Not the names' order: words are found
/// among texts in any language alike, whatever the locale.
pub fn folded(text: &str) -> String {
    let mut folded = String::with_capacity(text.len());
    for c in text.chars() {
        // Each part is whole characters: a letter's base, or its lowercase.
        fold(c, &mut |part| {
            folded.push_str(&String::from_utf8_lossy(part));
        });
    }
    folded
}

/// Words typed to find something among texts (the settings' search and
/// each of its pages' filters): each found in one of a thing's texts,
/// folded as names are ([`folded`]), so "ecole" finds "École", which the
/// order puts beside "ecole". Folded once.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Words(Vec<String>);

impl Words {
    pub fn new(typed: &str) -> Self {
        Words(
            folded(typed)
                .split_whitespace()
                .map(str::to_owned)
                .collect(),
        )
    }

    /// Nothing typed: everything is found.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Whether every word is in one of `texts`.
    pub fn found_in(&self, texts: &[&str]) -> bool {
        if self.0.is_empty() {
            return true;
        }
        let texts: Vec<String> = texts.iter().map(|text| folded(text)).collect();
        self.0
            .iter()
            .all(|word| texts.iter().any(|text| text.contains(word.as_str())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Words are found as names are folded: case and Latin accents folded,
    /// written with their letter or apart, each word in any of the texts.
    #[test]
    fn words_are_found_folded_each_in_a_text() {
        let words = Words::new("ECOLE  café");
        assert!(words.found_in(&["l'École des arts", "un cafe\u{301}"]));
        assert!(words.found_in(&["Café et école"]), "in one text, any order");
        assert!(!words.found_in(&["école"]), "every word");
        assert!(Words::new("  ").found_in(&[]), "nothing typed finds all");
        assert!(Words::new("strasse").found_in(&["Straße"]));
        assert!(Words::new("Ĳs").found_in(&["ijsselmeer"]));
        assert!(
            Words::new("ДОМ").found_in(&["мой дом"]),
            "case beyond Latin"
        );
        assert_eq!(folded("Æsir 10"), "aesir 10");
    }

    fn sorted_by(collation: &Collation, names: &[&str]) -> Vec<String> {
        let mut v: Vec<&str> = names.to_vec();
        v.sort_by_cached_key(|n| collation.sort_key(OsStr::new(n)));
        v.into_iter().map(str::to_owned).collect()
    }

    /// Folded: the C locale's order, which this process's is until one is
    /// set.
    fn sorted(names: &[&str]) -> Vec<String> {
        sorted_by(&Collation::folded(), names)
    }

    #[test]
    fn the_table_covers_its_range() {
        assert_eq!(LATIN.len(), 0x180 - 0xc0);
    }

    /// The C and POSIX locales, and none said, keep the folded order, the
    /// one names had before collations: this process's until one is set.
    #[test]
    fn the_c_locale_keeps_names_folded() {
        assert!(Collation::of(None).is_folded());
        assert!(current().is_folded(), "no locale set in a test");
        for name in ["École 10", "b", "Straße", "f02"] {
            let name = OsStr::new(name);
            assert_eq!(Collation::of(None).sort_key(name), folded_key(name));
            assert_eq!(sort_key(name), folded_key(name));
        }
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

    /// A name in decomposed form, its accent a mark after its letter, sorts
    /// where the same name composed does: beside its letter, not after "z".
    #[test]
    fn an_accent_written_apart_folds_as_one_written_with_its_letter() {
        let decomposed = "e\u{301}cole";
        for collation in [Collation::folded(), Collation::of(Some("en-US"))] {
            assert_eq!(
                sorted_by(&collation, &["ezra", decomposed, "eclair", "ecume"]),
                ["eclair", decomposed, "ecume", "ezra"],
                "{collation:?}"
            );
            // Ties fall back to the bytes: the order stays total.
            assert_ne!(
                collation.compare(OsStr::new(decomposed), OsStr::new("école")),
                Ordering::Equal
            );
        }
    }

    /// Swedish puts å, ä and ö after z, letters of their own.
    #[test]
    fn swedish_orders_its_letters_after_z() {
        let sv = Collation::of(Some("sv-SE"));
        assert_eq!(
            sorted_by(&sv, &["ö", "z", "å", "ä", "a", "o", "Ärlig", "zebra"]),
            ["a", "o", "z", "zebra", "å", "ä", "Ärlig", "ö"]
        );
    }

    /// German puts ä beside a: the base letter first, the accent a tie.
    #[test]
    fn german_orders_a_with_its_accent_beside_a() {
        let de = Collation::of(Some("de-DE"));
        assert_eq!(
            sorted_by(&de, &["Bach", "Äpfel", "Zebra", "Apfel", "acht", "Ärger"]),
            ["acht", "Apfel", "Äpfel", "Ärger", "Bach", "Zebra"]
        );
    }

    /// By a locale, numbers are by value, the base letters decide, then
    /// the accents, then the case, then the bytes.
    #[test]
    fn a_locale_orders_numbers_by_value_and_case_after_letters() {
        let en = Collation::of(Some("en-US"));
        assert_eq!(
            sorted_by(&en, &["f10", "f2", "f1", "f02", "g", "f"]),
            ["f", "f1", "f02", "f2", "f10", "g"]
        );
        assert_eq!(
            sorted_by(&en, &["page 100", "page 9", "page 10b", "page 10a"]),
            ["page 9", "page 10a", "page 10b", "page 100"]
        );
        assert_eq!(sorted_by(&en, &["b", "B", "A", "a"]), ["a", "A", "b", "B"]);
        assert_eq!(
            sorted_by(&en, &["z", "École", "ecole", "b", "a", "Ecole"]),
            ["a", "b", "ecole", "Ecole", "École", "z"]
        );
        // Punctuation and spaces count, before digits and letters.
        assert_eq!(
            sorted_by(&en, &["ab", "a b", "_a", "1a", "a-b"]),
            ["_a", "1a", "a b", "a-b", "ab"]
        );
        // Other scripts in their letters' order, not their code points'.
        assert_eq!(
            sorted_by(&en, &["γάμμα", "Άλφα", "βήτα"]),
            ["Άλφα", "βήτα", "γάμμα"]
        );
    }

    /// What a key says is what a comparison says, for every pair.
    #[test]
    fn keys_and_comparisons_agree() {
        let names = [
            "f10",
            "f2",
            "F2",
            "f02",
            "École",
            "ecole",
            "e\u{301}cole",
            "Straße",
            "strasse",
            "a b",
            "ab",
            "_x",
            "Ärger",
            "Zoë",
            "zoe",
            "日本",
            "Привет",
            "",
        ];
        for collation in [
            Collation::folded(),
            Collation::of(Some("en-US")),
            Collation::of(Some("sv-SE")),
        ] {
            for a in names {
                for b in names {
                    let (a, b) = (OsStr::new(a), OsStr::new(b));
                    assert_eq!(
                        collation.compare(a, b),
                        collation.sort_key(a).cmp(&collation.sort_key(b)),
                        "{collation:?}: {a:?} against {b:?}"
                    );
                }
            }
        }
    }

    /// ICU4X's keys hold no zero, which separates a key from the name's
    /// bytes after it: a key that begins another still sorts first.
    #[test]
    fn a_locale_s_keys_hold_no_zero() {
        let en = Collation::of(Some("en-US"));
        let Some(collators) = &en.0 else {
            panic!("a collation for English");
        };
        let mut texts: Vec<String> = (1..0x3400u32)
            .chain(0x4e00..0x4f00)
            .chain(0x1_f600..0x1_f650)
            .filter_map(char::from_u32)
            .map(|c| format!("{c}a{c}"))
            .collect();
        texts.extend(
            [
                "0",
                "00",
                "007",
                "123456789012345678901234567890",
                "a\u{301}",
            ]
            .map(String::from),
        );
        for text in &texts {
            let mut key = Vec::new();
            let Ok(()) = collators.order.write_sort_key_to(text, &mut key);
            assert!(!key.contains(&0), "{text:?}: {key:?}");
        }
    }

    /// A name that is not UTF-8 is ordered by what it reads as, its bytes
    /// that are no character after every letter, then by its bytes.
    #[cfg(unix)]
    #[test]
    fn names_that_are_not_utf8_still_order_totally() {
        use std::os::unix::ffi::OsStrExt;
        let a = OsStr::from_bytes(b"caf\xe9");
        let b = OsStr::from_bytes(b"caf\xe8");
        for collation in [Collation::folded(), Collation::of(Some("en-US"))] {
            assert_ne!(collation.compare(a, b), Ordering::Equal);
            let mut names = vec![
                OsStr::new("cag"),
                a,
                OsStr::new("cafz"),
                OsStr::new("café"),
                OsStr::new("cafe"),
            ];
            names.sort_by_cached_key(|n| collation.sort_key(n));
            assert_eq!(
                names,
                [
                    OsStr::new("cafe"),
                    OsStr::new("café"),
                    OsStr::new("cafz"),
                    a,
                    OsStr::new("cag")
                ],
                "{collation:?}"
            );
        }
    }

    #[test]
    fn a_prefix_begins_names_the_order_puts_beside_it() {
        for collation in [Collation::folded(), Collation::of(Some("en-US"))] {
            let begins = |text: &str, name: &str| collation.prefix(text).begins(OsStr::new(name));
            assert!(begins("ecole", "École normale"));
            assert!(begins("ÉCO", "ecole"));
            assert!(begins("strass", "Straße"));
            assert!(begins("a", "Æble"));
            assert!(begins("ae", "Æble"));
            assert!(begins("oeuvre", "Œuvres"));
            assert!(begins("my ", "My file.txt"));
            assert!(!begins("my ", "Myfile.txt"));
            assert!(begins("", "anything"));
            assert!(begins("", ""));
            assert!(begins("été", "Été 2026"));
            assert!(begins("f2", "f2.txt"));
            assert!(begins("f2", "f20.txt"), "numbers as they are written");
            assert!(!begins("f2", "f02.txt"));
            assert!(!begins("ecoles", "École"));
            assert!(begins("École", "École"));
            assert!(!begins("b", "abc"));
            assert!(begins("aeb", "Æble"));
            assert!(!begins("af", "Æble"));
            assert!(begins("пр", "Привет"));
            // A name in decomposed form (macOS's HFS+), reached typed either way.
            assert!(begins("ecole", "E\u{301}cole normale"));
            assert!(begins("école", "e\u{301}cole"));
            assert!(begins("e\u{301}t", "été"));
            assert!(!begins("ecoles", "e\u{301}cole"));
        }
    }

    /// By a locale, a beginning reaches what its order puts beside it: in
    /// Swedish "a" does not reach "Ärlig", after "z"; in German it does.
    #[test]
    fn a_prefix_follows_its_locale() {
        let begins = |locale: &str, text: &str, name: &str| {
            Collation::of(Some(locale))
                .prefix(text)
                .begins(OsStr::new(name))
        };
        assert!(!begins("sv-SE", "a", "Ärlig"));
        assert!(!begins("sv-SE", "ar", "Ärlig"));
        assert!(begins("sv-SE", "ä", "Ärlig"));
        assert!(begins("sv-SE", "ÄR", "ärlig"));
        assert!(begins("de-DE", "a", "Ärger"));
        assert!(begins("de-DE", "arg", "Ärger"));
        // Danish reads "aa" as "å", a letter after "z".
        assert!(!begins("da-DK", "a", "Aalborg"));
        assert!(begins("da-DK", "å", "Aalborg"));
    }

    /// The walk in ASCII turns away only names ICU4X would: in locales
    /// whose letters contract ("ch" in Czech, "cs" in Hungarian, "aa" in
    /// Danish), with accents written apart, against every beginning.
    #[test]
    fn the_walk_in_ascii_agrees_with_icu4x() {
        let names = [
            "file-000123.txt",
            "chata",
            "Chata",
            "csak",
            "cukor",
            "Aalborg",
            "abe",
            "a\u{30a}lborg",
            "ab\u{30a}",
            "hrad",
            "My file.txt",
            "Myfile",
            "École",
            "y2k",
            "i\u{307}",
            "",
        ];
        let typed = [
            "f", "file-1", "file-0", "c", "ch", "cs", "h", "a", "aa", "ab", "å", "my ", "my", "e",
            "y", "i", "",
        ];
        for locale in ["en-US", "cs-CZ", "hu-HU", "da-DK", "sv-SE", "lt-LT"] {
            let collation = Collation::of(Some(locale));
            let Some(collators) = &collation.0 else {
                panic!("a collation for {locale}");
            };
            for text in typed {
                let prefix = collation.prefix(text);
                for name in names {
                    let mut sink = Begins {
                        sought: &prefix.sought,
                    };
                    let icu4x = match collators.primary.write_sort_key_to(name, &mut sink) {
                        Err(Stop::Begins) => true,
                        Err(Stop::Differs) => false,
                        Ok(written) => written == prefix.sought.len(),
                    };
                    assert_eq!(
                        prefix.begins(OsStr::new(name)),
                        icu4x,
                        "{locale}: {text:?} beginning {name:?}"
                    );
                }
            }
        }
        // What contracts takes ICU4X's road.
        let czech = Collation::of(Some("cs-CZ"));
        assert!(!czech.prefix("c").begins(OsStr::new("chata")));
        assert!(czech.prefix("ch").begins(OsStr::new("Chata")));
        assert!(!czech.prefix("h").begins(OsStr::new("chata")));
    }

    #[cfg(unix)]
    #[test]
    fn a_prefix_reads_a_name_that_is_not_utf8() {
        use std::os::unix::ffi::OsStrExt;
        let name = OsStr::from_bytes(b"caf\xe9 cr\xe8me");
        for collation in [Collation::folded(), Collation::of(Some("en-US"))] {
            assert!(collation.prefix("caf").begins(name));
            assert!(collation.prefix("caf\u{fffd} c").begins(name));
            assert!(!collation.prefix("café").begins(name));
        }
    }

    /// The letter a name is grouped under: its first, its accent aside
    /// where the order sees the same letter, a letter of its own where the
    /// order puts it after "z".
    #[test]
    fn names_are_grouped_under_the_letter_the_order_puts_them_under() {
        let of = |collation: &Collation, name: &str| collation.initial(OsStr::new(name));
        let folded = Collation::folded();
        assert_eq!(of(&folded, "École"), Initial::Letter('E'));
        assert_eq!(of(&folded, "Æble"), Initial::Letter('A'));
        assert_eq!(of(&folded, "ßa"), Initial::Letter('S'));
        assert_eq!(of(&folded, "10 things"), Initial::Digit);
        assert_eq!(of(&folded, "_x"), Initial::Other);
        assert_eq!(of(&folded, ""), Initial::Other);
        let en = Collation::of(Some("en-US"));
        assert_eq!(of(&en, "école"), Initial::Letter('E'));
        assert_eq!(of(&en, "e\u{301}cole"), Initial::Letter('E'));
        assert_eq!(of(&en, "Æble"), Initial::Letter('A'));
        assert_eq!(of(&en, "ßa"), Initial::Letter('S'));
        assert_eq!(of(&en, "zoë"), Initial::Letter('Z'));
        assert_eq!(of(&en, "Привет"), Initial::Letter('П'));
        assert_eq!(of(&en, "привет"), Initial::Letter('П'));
        assert_eq!(of(&en, "10 things"), Initial::Digit);
        assert_eq!(of(&en, "١٠"), Initial::Digit, "a digit of another script");
        assert_eq!(of(&en, "_x"), Initial::Other);
        assert_eq!(of(&en, " x"), Initial::Other);
        let sv = Collation::of(Some("sv-SE"));
        assert_eq!(of(&sv, "ärlig"), Initial::Letter('Ä'));
        assert_eq!(of(&sv, "Öl"), Initial::Letter('Ö'));
        assert_eq!(of(&sv, "zebra"), Initial::Letter('Z'));
        let de = Collation::of(Some("de-DE"));
        assert_eq!(of(&de, "Ärger"), Initial::Letter('A'));
        // Groups of letters stand in the order's: Swedish's after "Z".
        let key = |c: &str| sv.sort_key(OsStr::new(c));
        assert!(key("Z") < key("Å") && key("Å") < key("Ä") && key("Ä") < key("Ö"));
    }

    #[cfg(unix)]
    #[test]
    fn a_name_that_is_not_utf8_is_grouped_under_none() {
        use std::os::unix::ffi::OsStrExt;
        let name = OsStr::from_bytes(b"\xe9t\xe9");
        assert_eq!(Collation::folded().initial(name), Initial::Other);
        assert_eq!(Collation::of(Some("en-US")).initial(name), Initial::Other);
        let name = OsStr::from_bytes(b"caf\xe9");
        assert_eq!(
            Collation::of(Some("en-US")).initial(name),
            Initial::Letter('C')
        );
    }
}
