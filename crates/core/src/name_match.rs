//! How words typed find a file by its name: the one matcher of every
//! place Skwir matches a name with them (a search's name terms and a
//! filter's words, the query plugin's `NamePat`; the name filter and `/`,
//! [`crate::workspace::NameFilter`]), and so what a names index must match
//! to be taken as covering a search ([`crate::names::NameTerm`],
//! [`crate::names::NameIndex::both_forms`]).
//!
//! - **A substring or a glob** ([`NameMatch::term`]): a glob where the
//!   text holds `*`, `?` or `[` and compiles as one (globset, no separator
//!   rule), else a substring, searched for as written (`a[b`);
//!   [`NameMatch::words`] never globs.
//! - **Case** is ignored unless asked to count: both sides lowercased with
//!   `str::to_lowercase` (`ß` is not `ss`). A glob whose literal parts are
//!   ASCII folds case as globset does, in ASCII alone, on the name as it
//!   is (the fast path, as before); one whose literal parts hold more, with
//!   case ignored, is matched on the name lowercased, those parts
//!   lowercased first, as a substring is.
//! - **Both forms** (the user's choice of 2026-11-24, with skwird, as
//!   fsearch does): a term is matched in its composed and its decomposed
//!   form (NFC and NFD), lowercased first where case is ignored, then
//!   normalized; a glob in its literal parts alone, its `*`, `?`, classes
//!   and escapes left as typed. macOS's programs often write names
//!   decomposed, and keyboards type them composed. No name is rewritten: a
//!   name is matched as it is (lowercased) against the two, so one whose
//!   own parts mix the forms may not match.
//! - **One pattern where both forms are the same**: every ASCII term, and
//!   most others (Cyrillic, Han). Nothing is normalized for an ASCII term,
//!   and an ASCII name is compared byte by byte with an ASCII substring,
//!   never lowercased into a string of its own: a folder of a hundred
//!   thousand names filtered at each key costs what it did.

use std::borrow::Cow;
use std::cell::OnceCell;
use std::ffi::OsStr;

use globset::{GlobBuilder, GlobMatcher, GlobSet, GlobSetBuilder};
use unicode_normalization::UnicodeNormalization;

/// A term made once, matched against many names.
#[derive(Clone, Debug)]
pub struct NameMatch {
    how: How,
    /// Whether it finds what the term matched as written finds: one form,
    /// the term itself (lowercased where case is ignored), and a glob on
    /// the name as it is ([`NameMatch::as_written`]).
    as_written: bool,
}

#[derive(Clone, Debug)]
enum How {
    /// Found anywhere in the name: the term in its forms, lowercased
    /// unless case counts.
    Substring { forms: Forms, case_sensitive: bool },
    /// The glob in its forms, matched on the name lowercased where
    /// `lowered`, else on the name as it is.
    Glob { globs: Globs, lowered: bool },
}

#[derive(Clone, Debug)]
enum Globs {
    One(GlobMatcher),
    /// Its two forms, matched in one pass.
    Two(GlobSet),
}

impl Globs {
    fn is_match(&self, text: &str) -> bool {
        match self {
            Globs::One(glob) => glob.is_match(text),
            Globs::Two(set) => set.is_match(text),
        }
    }

    fn count(&self) -> usize {
        match self {
            Globs::One(_) => 1,
            Globs::Two(set) => set.len(),
        }
    }
}

/// A text in its composed and decomposed forms (NFC, then NFD), one where
/// they are the same: every ASCII text, given back as it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Forms {
    first: String,
    second: Option<String>,
}

impl Forms {
    /// `text`'s forms, as it is: neither lowercased nor told apart from
    /// a glob (what a program asked in both forms takes, `plocate`).
    pub fn of(text: &str) -> Forms {
        if text.is_ascii() {
            return Forms {
                first: text.to_owned(),
                second: None,
            };
        }
        let composed: String = text.nfc().collect();
        let decomposed: String = text.nfd().collect();
        Forms {
            second: (decomposed != composed).then_some(decomposed),
            first: composed,
        }
    }

    /// Each form, the composed first.
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.first.as_str()).chain(self.second.as_deref())
    }

    /// Whether both forms are one.
    pub fn is_one(&self) -> bool {
        self.second.is_none()
    }
}

/// A name as terms find it: read as text and lowercased each once, and
/// only once a term asks, for every term tested of it.
pub struct Name<'a> {
    raw: &'a OsStr,
    text: OnceCell<Cow<'a, str>>,
    lowered: OnceCell<String>,
}

impl<'a> Name<'a> {
    pub fn new(raw: &'a OsStr) -> Self {
        Name {
            raw,
            text: OnceCell::new(),
            lowered: OnceCell::new(),
        }
    }

    /// What it reads as: each byte that is no character U+FFFD.
    pub fn text(&self) -> &str {
        self.text.get_or_init(|| self.raw.to_string_lossy())
    }

    /// What it reads as, lowercased (`str::to_lowercase`).
    pub fn lowered(&self) -> &str {
        self.lowered.get_or_init(|| self.text().to_lowercase())
    }
}

impl NameMatch {
    /// A name term as a search reads it: a glob where `text` holds `*`,
    /// `?` or `[` and compiles as one, else a substring.
    pub fn term(text: &str, case_sensitive: bool) -> NameMatch {
        if text.contains(['*', '?', '['])
            && let Some(glob) = Self::glob(text, case_sensitive)
        {
            return glob;
        }
        Self::words(text, case_sensitive)
    }

    /// `text` anywhere in a name, never a glob.
    pub fn words(text: &str, case_sensitive: bool) -> NameMatch {
        let base = if case_sensitive {
            Cow::Borrowed(text)
        } else {
            Cow::Owned(text.to_lowercase())
        };
        let forms = Forms::of(&base);
        NameMatch {
            as_written: forms.is_one() && forms.first == *base,
            how: How::Substring {
                forms,
                case_sensitive,
            },
        }
    }

    /// `text` as a glob in its forms, or `None` where it does not compile
    /// as typed.
    fn glob(text: &str, case_sensitive: bool) -> Option<NameMatch> {
        let compiled = |text: &str| {
            GlobBuilder::new(text)
                .case_insensitive(!case_sensitive)
                .literal_separator(false)
                .build()
                .ok()
        };
        let typed = compiled(text)?;
        let parts = glob_parts(text);
        if parts
            .iter()
            .all(|(literal, part)| !literal || part.is_ascii())
        {
            // Nothing to lowercase or normalize: the glob as typed.
            return Some(NameMatch {
                how: How::Glob {
                    globs: Globs::One(typed.compile_matcher()),
                    lowered: false,
                },
                as_written: true,
            });
        }
        let lowered = !case_sensitive;
        let form = |normalize: fn(&str) -> String| -> String {
            parts
                .iter()
                .map(|&(literal, part)| match (literal, lowered) {
                    (false, _) => Cow::Borrowed(part),
                    (true, false) => Cow::Owned(normalize(part)),
                    (true, true) => Cow::Owned(normalize(&part.to_lowercase())),
                })
                .collect()
        };
        let composed = form(|part| part.nfc().collect());
        let decomposed = form(|part| part.nfd().collect());
        let as_written = !lowered && composed == decomposed && composed == text;
        let globs = if composed == decomposed {
            Globs::One(compiled(&composed)?.compile_matcher())
        } else {
            let mut set = GlobSetBuilder::new();
            set.add(compiled(&composed)?).add(compiled(&decomposed)?);
            Globs::Two(set.build().ok()?)
        };
        Some(NameMatch {
            how: How::Glob { globs, lowered },
            as_written,
        })
    }

    /// Whether it is a glob.
    pub fn is_glob(&self) -> bool {
        matches!(self.how, How::Glob { .. })
    }

    /// Whether the term matched as written, in one form, finds what this
    /// finds: a glob on the name as it is. What an index that matches as
    /// skwird's protocol 0 does answers alike only then
    /// ([`crate::names::NameTerm::as_written`]): every ASCII term, any of
    /// one form but a glob holding more than ASCII where case is ignored.
    pub fn as_written(&self) -> bool {
        self.as_written
    }

    /// How many patterns it matches a name against: two only where the
    /// term's forms differ.
    pub fn patterns(&self) -> usize {
        match &self.how {
            How::Substring { forms, .. } => forms.iter().count(),
            How::Glob { globs, .. } => globs.count(),
        }
    }

    /// Whether `name` matches.
    pub fn matches(&self, name: &OsStr) -> bool {
        self.finds(&Name::new(name))
    }

    /// Whether `name` matches, as read for every term asked of it.
    pub fn finds(&self, name: &Name<'_>) -> bool {
        match &self.how {
            How::Substring {
                forms,
                case_sensitive,
            } => {
                let bytes = name.raw.as_encoded_bytes();
                if bytes.is_ascii() {
                    // An ASCII name lowercases to ASCII alone, and a term
                    // of two forms has no ASCII one: only an ASCII form
                    // can be in it, compared byte by byte, case folded as
                    // it goes where it is ignored.
                    let needle = forms.first.as_bytes();
                    return forms.is_one()
                        && needle.is_ascii()
                        && (needle.is_empty()
                            || bytes.windows(needle.len()).any(|window| {
                                if *case_sensitive {
                                    window == needle
                                } else {
                                    window.eq_ignore_ascii_case(needle)
                                }
                            }));
                }
                let hay = if *case_sensitive {
                    name.text()
                } else {
                    name.lowered()
                };
                forms.iter().any(|form| hay.contains(form))
            }
            How::Glob { globs, lowered } => globs.is_match(if *lowered {
                name.lowered()
            } else {
                name.text()
            }),
        }
    }
}

/// `glob` in its parts, as globset reads it: runs of literal characters
/// (`true`), and what is no literal (`false`): a wildcard, a class with
/// its brackets, an escape with what it escapes, the braces and commas of
/// alternatives. Every one of those is ASCII, and none composes with a
/// mark after it, so normalizing the runs apart is normalizing the
/// literal text.
fn glob_parts(glob: &str) -> Vec<(bool, &str)> {
    // globset's default: a backslash escapes, unless it separates paths.
    let escapes = !std::path::is_separator('\\');
    let mut parts = Vec::new();
    let mut literal_from: Option<usize> = None;
    let mut at = 0;
    while let Some(c) = glob[at..].chars().next() {
        let end = match c {
            '*' | '?' | '{' | '}' | ',' => Some(at + 1),
            '\\' if escapes => Some(
                glob[at + 1..]
                    .chars()
                    .next()
                    .map_or(glob.len(), |escaped| at + 1 + escaped.len_utf8()),
            ),
            '[' => Some(class_end(glob, at)),
            _ => None,
        };
        if let Some(end) = end {
            if let Some(from) = literal_from.take() {
                parts.push((true, &glob[from..at]));
            }
            parts.push((false, &glob[at..end]));
            at = end;
        } else {
            literal_from.get_or_insert(at);
            at += c.len_utf8();
        }
    }
    if let Some(from) = literal_from {
        parts.push((true, &glob[from..]));
    }
    parts
}

/// Where the class `glob` opens at `open` (its `[`) ends, past its `]`, as
/// globset parses one: a `!` or `^` first negates it, a `]` first is one of
/// its characters, a `[:name:]` (eight small letters at most) is read
/// whole; the end of the text where it is not closed, which globset
/// refuses.
fn class_end(glob: &str, open: usize) -> usize {
    let mut at = open + 1;
    if glob[at..].starts_with(['!', '^']) {
        at += 1;
    }
    let (mut first, mut in_range) = (true, false);
    while let Some(c) = glob[at..].chars().next() {
        at += c.len_utf8();
        match c {
            ']' if !first => return at,
            ']' => {}
            // A range's dash, or its end where one is open.
            '-' => {
                if !first {
                    in_range = !in_range;
                }
            }
            '[' if !in_range && glob[at..].starts_with(':') => {
                if let Some(len) = posix_class(&glob[at..]) {
                    at += len;
                }
            }
            _ => in_range = false,
        }
        first = false;
    }
    glob.len()
}

/// The length of `rest`, read after a `[` in a class, that globset reads
/// as a POSIX class: `:`, up to eight small ASCII letters, `:]`; whatever
/// the name, as globset consumes it.
fn posix_class(rest: &str) -> Option<usize> {
    let name = rest.strip_prefix(':')?;
    let letters = name
        .bytes()
        .take_while(u8::is_ascii_lowercase)
        .count()
        .min(8);
    name[letters..].starts_with(":]").then_some(1 + letters + 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMPOSED: &str = "caf\u{e9}.txt";
    const DECOMPOSED: &str = "cafe\u{301}.txt";

    fn finds(term: &str, case_sensitive: bool, name: &str) -> bool {
        NameMatch::term(term, case_sensitive).matches(OsStr::new(name))
    }

    /// A word typed composed finds a name written decomposed, and back;
    /// lowercased first where case is ignored, as typed where it counts.
    #[test]
    fn a_word_finds_a_name_in_either_form() {
        for term in ["caf\u{e9}", "cafe\u{301}", "CAF\u{c9}", "CAFE\u{301}"] {
            for name in [COMPOSED, DECOMPOSED, "CAF\u{c9}.TXT", "CAFE\u{301} 2.txt"] {
                assert!(finds(term, false, name), "{term:?} in {name:?}");
            }
        }
        assert!(finds("CAF\u{c9}", true, "CAFE\u{301}.txt"));
        assert!(finds("CAF\u{c9}", true, "le CAF\u{c9}"));
        assert!(!finds("CAF\u{c9}", true, COMPOSED), "its case counts");
        assert!(
            !finds("caf\u{e9}", false, "cafe.txt"),
            "an accent is no letter"
        );
        // A word never globs.
        assert!(!NameMatch::words("caf\u{e9}*", false).matches(OsStr::new(COMPOSED)));
        // Case folds as `to_lowercase` folds it.
        assert!(!finds("strasse", false, "Stra\u{df}e"));
        assert!(finds("STRA\u{df}E", false, "stra\u{df}e"));
    }

    /// A glob is normalized in its literal parts alone: `caf\u{e9}*` finds
    /// the name written decomposed; its classes are left as typed.
    #[test]
    fn a_glob_finds_a_name_in_either_form_its_classes_as_typed() {
        for term in ["caf\u{e9}*", "CAF\u{c9}*", "*\u{e9}.txt", "cafe\u{301}.*"] {
            for name in [COMPOSED, DECOMPOSED] {
                assert!(finds(term, false, name), "{term:?} for {name:?}");
            }
        }
        assert!(finds("CAF\u{c9}*", false, "CAFE\u{301}.TXT"));
        assert!(finds("CAF\u{c9}*", true, "CAFE\u{301}.txt"));
        assert!(!finds("CAF\u{c9}*", true, COMPOSED), "its case counts");
        assert!(finds("{caf\u{e9},th\u{e9}}*.txt", false, DECOMPOSED));
        // A class is left as typed: decomposed, it would hold an `e`.
        assert!(finds("caf[\u{e9}]*", false, COMPOSED));
        assert!(!finds("caf[\u{e9}]*", false, DECOMPOSED));
        // A glob of ASCII folds case in ASCII alone on the name as it is,
        // as it always did.
        assert!(finds("CAF*.txt", false, DECOMPOSED));
        assert!(!finds("k*", false, "\u{212a}elvin"));
        assert!(finds("k", false, "\u{212a}elvin"), "a word lowercases it");
        // One that does not compile is a word, as typed.
        assert!(finds("caf\u{e9}[", false, "cafe\u{301}[1].txt"));
    }

    /// Where both forms are one (every ASCII term, Cyrillic and Han ones),
    /// a name is matched against one pattern, and nothing is normalized
    /// for an ASCII term; a term matched as written stays so.
    #[test]
    fn one_pattern_where_both_forms_are_one() {
        for (term, glob) in [
            ("report", false),
            ("Report*.PDF", true),
            ("[!a-z]?x", true),
            ("\u{434}\u{43e}\u{43c}", false),
            ("\u{65e5}\u{672c}", false),
            ("caf[\u{e9}]*", true),
        ] {
            let matcher = NameMatch::term(term, false);
            assert_eq!(matcher.is_glob(), glob, "{term:?}");
            assert_eq!(matcher.patterns(), 1, "{term:?}");
            assert!(matcher.as_written(), "{term:?}");
        }
        assert_eq!(
            Forms::of("report"),
            Forms {
                first: "report".into(),
                second: None
            }
        );
        for term in ["caf\u{e9}", "caf\u{e9}*", "cafe\u{301}"] {
            let matcher = NameMatch::term(term, false);
            assert_eq!(matcher.patterns(), 2, "{term:?}");
            assert!(!matcher.as_written(), "{term:?}");
        }
        // Of one form, yet matched otherwise than as written: a glob of
        // Cyrillic letters, with case ignored, lowercases the name.
        let cyrillic = NameMatch::term("\u{434}\u{43e}\u{43c}*", false);
        assert_eq!(cyrillic.patterns(), 1);
        assert!(!cyrillic.as_written());
        assert!(cyrillic.matches(OsStr::new("\u{414}\u{41e}\u{41c}.txt")));
        assert!(NameMatch::term("\u{434}\u{43e}\u{43c}*", true).as_written());
        // A word typed with a capital, case ignored, is of one form, but
        // lowercased: as written is what an index does with it too.
        assert!(NameMatch::words("Report", false).as_written());
    }

    /// An ASCII name is never lowercased into a string of its own.
    #[test]
    fn an_ascii_name_is_compared_in_place() {
        let matcher = NameMatch::words("F007", false);
        let name = Name::new(OsStr::new("dir/f007.txt"));
        assert!(matcher.finds(&name));
        assert!(name.text.get().is_none() && name.lowered.get().is_none());
        assert!(!NameMatch::words("caf\u{e9}", false).matches(OsStr::new("cafe.txt")));
        assert!(NameMatch::words("", false).matches(OsStr::new("anything")));
        assert!(NameMatch::words("", true).matches(OsStr::new("")));
    }

    /// A name that is not UTF-8 is matched as it reads.
    #[cfg(unix)]
    #[test]
    fn a_name_that_is_not_utf8_is_matched_as_it_reads() {
        use std::os::unix::ffi::OsStrExt;
        let name = OsStr::from_bytes(b"caf\xe9 cafe\xcc\x81.txt");
        assert!(NameMatch::words("caf\u{e9}", false).matches(name));
        assert!(NameMatch::term("*\u{fffd}*", false).matches(name));
    }

    /// The parts of a glob are those globset reads: literal runs apart
    /// from wildcards, classes (a `]` first, a POSIX class within),
    /// escapes and alternatives.
    #[test]
    fn a_glob_is_parted_as_globset_reads_it() {
        let parts = glob_parts;
        assert_eq!(
            parts("caf\u{e9}*x?"),
            [(true, "caf\u{e9}"), (false, "*"), (true, "x"), (false, "?")]
        );
        assert_eq!(
            parts("a[]\u{e9}]b[!\u{e9}-\u{ea}]c"),
            [
                (true, "a"),
                (false, "[]\u{e9}]"),
                (true, "b"),
                (false, "[!\u{e9}-\u{ea}]"),
                (true, "c")
            ]
        );
        assert_eq!(
            parts("[[:alpha:]\u{e9}]\u{e9}"),
            [(false, "[[:alpha:]\u{e9}]"), (true, "\u{e9}")]
        );
        assert_eq!(
            parts("{\u{e9},b}"),
            [
                (false, "{"),
                (true, "\u{e9}"),
                (false, ","),
                (true, "b"),
                (false, "}")
            ]
        );
        if !std::path::is_separator('\\') {
            assert_eq!(
                parts("\\*\u{e9}\\\u{e9}"),
                [(false, "\\*"), (true, "\u{e9}"), (false, "\\\u{e9}")]
            );
        }
        assert_eq!(parts("[\u{e9}"), [(false, "[\u{e9}")], "not closed");
        // What globset reads as one class, read as one: each of these
        // compiles, and a name holding the class's own characters matches.
        for (glob, name) in [
            ("[]]x", "]x"),
            ("[!]]x", "ax"),
            ("[[:alpha:]]x", "ax"),
            ("[a-]x", "-x"),
            // globset reads `[:nope:]` whole, then takes its `[` alone.
            ("[[:nope:]]x", "[x"),
        ] {
            let matcher = NameMatch::term(glob, true);
            assert!(matcher.is_glob(), "{glob}");
            assert!(matcher.matches(OsStr::new(name)), "{glob} for {name}");
        }
    }
}
