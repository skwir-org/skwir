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
//! - **Case** is ignored unless asked to count: both sides folded alike
//!   ([`fold`]): lowercased with `str::to_lowercase` (`ß` is not `ss`), a
//!   final sigma `ς` taken as `σ`, since `to_lowercase` writes `Σ` by its
//!   context, and a word typed alone ends where the same word in a name
//!   goes on to its extension (`ΟΔΟΣ`, `ΟΔΟΣ.txt`). A glob of ASCII folds
//!   case as globset does, in ASCII alone, on the name as it is (the fast
//!   path, as before). One holding more, with case ignored, is matched on
//!   the name folded, itself folded first: its literal parts, what its
//!   escapes escape, its classes' single members. A range's ends stay as
//!   typed: globset reads a class's members beyond ASCII as bytes (`[É]`
//!   is the bytes C3 and 89), so such a class finds a character's first
//!   byte, not the character, and folding its ends would mend nothing.
//! - **Its forms** (the user's choice of 2026-11-24, with skwird, as
//!   fsearch does): a term is matched as typed, composed and decomposed
//!   (NFC and NFD), each once ([`Forms`]), folded first where case is
//!   ignored, then normalized; a glob in its literal parts alone, its `*`,
//!   `?`, classes and escapes left as typed. macOS's programs often write
//!   names decomposed, and keyboards type them composed; and as typed, for
//!   a character neither form keeps (a CJK compatibility ideograph, the
//!   Ångström sign, Devanagari QA), which a name holds as typed. No name is
//!   rewritten: a name is matched as it is (folded) against the three, so
//!   one whose own parts mix the forms may not match.
//! - **One pattern where the forms are one**: every ASCII term, and most
//!   others (Cyrillic, Han). Nothing is normalized for an ASCII term, and
//!   an ASCII name is compared byte by byte with an ASCII form, never
//!   lowercased into a string of its own: a folder of a hundred thousand
//!   names filtered at each key costs what it did.

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
    /// the term itself (lowercased as `to_lowercase` lowers it where case
    /// is ignored), and a glob on the name as it is
    /// ([`NameMatch::as_written`]).
    as_written: bool,
}

#[derive(Clone, Debug)]
enum How {
    /// Found anywhere in the name: the term in its forms, folded unless
    /// case counts.
    Substring {
        forms: Forms,
        /// Its one ASCII form, if any (`Forms::ascii`), what an ASCII name
        /// is compared with.
        ascii: Option<String>,
        case_sensitive: bool,
    },
    /// The glob in its forms, matched on the name folded where `lowered`,
    /// else on the name as it is.
    Glob { globs: Globs, lowered: bool },
}

#[derive(Clone, Debug)]
enum Globs {
    One(GlobMatcher),
    /// Its forms, two or three, matched in one pass.
    Many(GlobSet),
}

impl Globs {
    fn is_match(&self, text: &str) -> bool {
        match self {
            Globs::One(glob) => glob.is_match(text),
            Globs::Many(set) => set.is_match(text),
        }
    }

    fn count(&self) -> usize {
        match self {
            Globs::One(_) => 1,
            Globs::Many(set) => set.len(),
        }
    }
}

/// A text as it is, composed and decomposed (NFC, NFD), each once: one
/// where they are the same, every ASCII text, given back as it is. As it
/// is too, since neither form keeps every character (U+F900 is U+8C48 in
/// both, the Ångström sign U+212B is U+00C5 or an `A` and its ring), and
/// a name holding one as typed is never normalized.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Forms {
    /// The text as it is.
    first: String,
    /// Its composed and decomposed forms, where they differ from it and
    /// from each other.
    more: Vec<String>,
}

impl Forms {
    /// `text`'s forms, as it is: neither folded nor told apart from a glob
    /// (what a program asked in each form takes, `plocate`).
    pub fn of(text: &str) -> Forms {
        let mut more: Vec<String> = Vec::new();
        if !text.is_ascii() {
            for form in [text.nfc().collect::<String>(), text.nfd().collect()] {
                if form != text && !more.contains(&form) {
                    more.push(form);
                }
            }
        }
        Forms {
            first: text.to_owned(),
            more,
        }
    }

    /// Each form: as it is, then composed, then decomposed.
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.first.as_str()).chain(self.more.iter().map(String::as_str))
    }

    /// How many forms it has, from one to three.
    pub fn count(&self) -> usize {
        1 + self.more.len()
    }

    /// Whether its forms are one.
    pub fn is_one(&self) -> bool {
        self.more.is_empty()
    }

    /// Its one form that is ASCII, if any: the text where it is ASCII, its
    /// forms normalized where they are (the Kelvin sign's `K`). Never two:
    /// an ASCII text is its own NFC and NFD, and a text's NFD is its NFC's.
    pub fn ascii(&self) -> Option<&str> {
        self.iter().find(|form| form.is_ascii())
    }
}

/// What case ignored makes of a text, a term's and a name's alike:
/// lowercased as `str::to_lowercase` lowercases it (`ß` is not `ss`), and a
/// final sigma `ς` taken as `σ`. `to_lowercase` writes `Σ` as `ς` only at a
/// word's end, which a word typed alone (`ΟΔΟΣ`) has where the same word
/// in a name goes on to its extension (`ΟΔΟΣ.txt`), and a Greek keyboard
/// types `ς` wherever a word ends (`οδος`). The same as lowercasing each
/// character alone (`char::to_lowercase`), then taking `ς` as `σ`.
pub fn fold(text: &str) -> String {
    let lowered = text.to_lowercase();
    if lowered.contains('ς') {
        lowered.replace('ς', "σ")
    } else {
        lowered
    }
}

/// A name as terms find it: read as text and folded each once, and only
/// once a term asks, for every term tested of it.
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

    /// What it reads as, folded as case ignored folds it ([`fold`]).
    pub fn lowered(&self) -> &str {
        self.lowered.get_or_init(|| fold(self.text()))
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
            Cow::Owned(fold(text))
        };
        let forms = Forms::of(&base);
        NameMatch {
            // Of one form, and folded as `to_lowercase` lowers it: not where
            // it holds a sigma, which `to_lowercase` writes by its context.
            as_written: forms.is_one() && (case_sensitive || !base.contains('σ')),
            how: How::Substring {
                ascii: forms.ascii().map(str::to_owned),
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
        if text.is_ascii() {
            // Nothing to fold or normalize: the glob as typed.
            return Some(NameMatch {
                how: How::Glob {
                    globs: Globs::One(typed.compile_matcher()),
                    lowered: false,
                },
                as_written: true,
            });
        }
        let lowered = !case_sensitive;
        // Each part as it is matched: folded where case is ignored, a
        // literal one then normalized in each form.
        let parts: Vec<(bool, Cow<'_, str>)> = glob_parts(text)
            .into_iter()
            .map(|(literal, part)| {
                let part = if lowered {
                    fold_part(literal, part)
                } else {
                    Cow::Borrowed(part)
                };
                (literal, part)
            })
            .collect();
        let form = |normalize: Option<fn(&str) -> String>| -> String {
            parts
                .iter()
                .map(|(literal, part)| match normalize {
                    Some(normalize) if *literal => Cow::Owned(normalize(part)),
                    _ => Cow::Borrowed(&**part),
                })
                .collect()
        };
        let mut forms: Vec<String> = Vec::with_capacity(3);
        for each in [form(None), form(Some(nfc)), form(Some(nfd))] {
            if !forms.contains(&each) {
                forms.push(each);
            }
        }
        // Case counting, the form as typed is the glob as typed.
        let as_written = !lowered && forms.len() == 1;
        let globs = if let [one] = forms.as_slice() {
            Globs::One(if as_written { typed } else { compiled(one)? }.compile_matcher())
        } else {
            let mut set = GlobSetBuilder::new();
            for form in &forms {
                set.add(compiled(form)?);
            }
            Globs::Many(set.build().ok()?)
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
    /// finds: a glob on the name as it is, a word lowercased as
    /// `to_lowercase` lowers it. What an index that matches as skwird's
    /// protocol 0 does answers alike only then
    /// ([`crate::names::NameTerm::as_written`]): every ASCII term, any of
    /// one form but, where case is ignored, a glob holding more than ASCII
    /// and a word holding a sigma.
    pub fn as_written(&self) -> bool {
        self.as_written
    }

    /// How many patterns it matches a name against: more than one only
    /// where the term's forms differ.
    pub fn patterns(&self) -> usize {
        match &self.how {
            How::Substring { forms, .. } => forms.count(),
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
                ascii,
                case_sensitive,
            } => {
                let bytes = name.raw.as_encoded_bytes();
                if bytes.is_ascii() {
                    // An ASCII name folds to ASCII alone: only an ASCII
                    // form can be in it (most terms' one form; the Kelvin
                    // sign's, `K`, composed), compared byte by byte, case
                    // folded as it goes where it is ignored.
                    return ascii.as_ref().is_some_and(|needle| {
                        holds_ascii(bytes, needle.as_bytes(), *case_sensitive)
                    });
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

/// Whether the ASCII `name` holds the ASCII `needle`, case folded in ASCII
/// unless `case_sensitive`.
fn holds_ascii(name: &[u8], needle: &[u8], case_sensitive: bool) -> bool {
    needle.is_empty()
        || name.windows(needle.len()).any(|window| {
            if case_sensitive {
                window == needle
            } else {
                window.eq_ignore_ascii_case(needle)
            }
        })
}

fn nfc(text: &str) -> String {
    text.nfc().collect()
}

fn nfd(text: &str) -> String {
    text.nfd().collect()
}

/// A part of a glob ([`glob_parts`]) as case ignored folds it: a literal
/// run whole ([`fold`]), what an escape escapes (each character it folds
/// to escaped again), a class's single members ([`fold_class`]); what is
/// ASCII as it is, which globset folds.
fn fold_part(literal: bool, part: &str) -> Cow<'_, str> {
    if part.is_ascii() {
        Cow::Borrowed(part)
    } else if literal {
        Cow::Owned(fold(part))
    } else if let Some(escaped) = part.strip_prefix('\\') {
        Cow::Owned(fold(escaped).chars().flat_map(|c| ['\\', c]).collect())
    } else if part.starts_with('[') {
        fold_class(part)
    } else {
        Cow::Borrowed(part)
    }
}

/// `class`, a class as [`glob_parts`] parts it (its brackets, closed), with
/// each single member beyond ASCII folded where it folds to one character
/// (a class matches one: `İ`, which folds to two, stays). A range's ends
/// stay as typed: globset reads a range beyond ASCII as one of bytes,
/// which folding its ends would not mend, and folded they may cross (`Ω`
/// folds past `ΰ`), which globset refuses. Read as globset reads a class
/// ([`class_end`]).
fn fold_class(class: &str) -> Cow<'_, str> {
    let mut at = 1;
    if class[at..].starts_with(['!', '^']) {
        at += 1;
    }
    let body = at;
    // Where each range's ends are, by their first byte.
    let mut ends = vec![false; class.len()];
    // The last member, where a range may yet extend it: none after a
    // POSIX class, whose ends are ASCII.
    let mut last: Option<usize> = None;
    let (mut first, mut in_range) = (true, false);
    let mut close = class.len();
    while let Some(c) = class[at..].chars().next() {
        let here = at;
        at += c.len_utf8();
        match c {
            ']' if !first => {
                close = here;
                break;
            }
            '-' if !first && !in_range => in_range = true,
            '[' if !in_range && posix_class(&class[at..]).is_some() => {
                at += posix_class(&class[at..]).unwrap_or(0);
                last = None;
            }
            _ if in_range => {
                if let Some(start) = last {
                    ends[start] = true;
                }
                ends[here] = true;
                in_range = false;
            }
            _ => last = Some(here),
        }
        first = false;
    }
    let mut folded = String::with_capacity(class.len());
    folded.push_str(&class[..body]);
    let mut changed = false;
    for (offset, c) in class[body..close].char_indices() {
        if !c.is_ascii() && !ends[body + offset] {
            // As `fold` folds it, a character alone.
            let mut lower = c
                .to_lowercase()
                .map(|lower| if lower == 'ς' { 'σ' } else { lower });
            if let (Some(one), None) = (lower.next(), lower.next())
                && one != c
            {
                folded.push(one);
                changed = true;
                continue;
            }
        }
        folded.push(c);
    }
    folded.push_str(&class[close..]);
    if changed {
        Cow::Owned(folded)
    } else {
        Cow::Borrowed(class)
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
        // Case folds as `to_lowercase` lowers it, `ς` as `σ` ([`fold`]).
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

    /// Where the forms are one (every ASCII term, Cyrillic and Han ones),
    /// a name is matched against one pattern, and nothing is normalized
    /// for an ASCII term; a term matched as written stays so.
    #[test]
    fn one_pattern_where_the_forms_are_one() {
        for (term, glob) in [
            ("report", false),
            ("Report*.PDF", true),
            ("[!a-z]?x", true),
            ("\u{434}\u{43e}\u{43c}", false),
            ("\u{65e5}\u{672c}", false),
        ] {
            let matcher = NameMatch::term(term, false);
            assert_eq!(matcher.is_glob(), glob, "{term:?}");
            assert_eq!(matcher.patterns(), 1, "{term:?}");
            assert!(matcher.as_written(), "{term:?}");
        }
        // A class beyond ASCII is folded where case is ignored, on the name
        // folded: one pattern, not as written.
        let class = NameMatch::term("caf[\u{e9}]*", false);
        assert!(class.is_glob());
        assert_eq!(class.patterns(), 1);
        assert!(!class.as_written());
        assert!(NameMatch::term("caf[\u{e9}]*", true).as_written());
        assert_eq!(
            Forms::of("report"),
            Forms {
                first: "report".into(),
                more: Vec::new()
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

    /// A term finds its own text where neither normalization keeps it: a
    /// name holding it as typed, never normalized (a CJK compatibility
    /// ideograph, a composition exclusion, Greek oxia, the Ångström and Ohm
    /// signs, a Hebrew presentation form); and an ASCII name its form that
    /// normalization makes ASCII.
    #[test]
    fn a_term_finds_itself_where_neither_form_keeps_it() {
        for (term, name, case_sensitive) in [
            ("\u{f900}", "\u{f900}.txt", false),
            ("\u{f900}", "\u{f900}.txt", true),
            ("\u{958}", "\u{958}.txt", false),
            ("\u{1f71}", "\u{1f71}.txt", false),
            ("\u{212b}", "\u{212b}.txt", true),
            ("\u{2126}", "\u{2126}.txt", true),
            ("\u{fb2a}", "\u{fb2a}.txt", false),
            ("\u{212a}", "\u{212a}.txt", true),
            ("\u{f900}*", "\u{f900}.txt", false),
            ("\u{f900}*", "\u{f900}.txt", true),
            ("*\u{958}.txt", "\u{958}.txt", false),
        ] {
            assert!(
                finds(term, case_sensitive, name),
                "{term:?} in {name:?}, case_sensitive {case_sensitive}"
            );
        }
        // Its normalized forms still find what they did.
        assert!(finds("\u{f900}", false, "\u{8c48}.txt"));
        assert!(finds("\u{212b}", true, "\u{c5}.txt"));
        assert!(finds("\u{212b}", true, "A\u{30a}.txt"));
        assert!(finds("\u{212a}", true, "OK.txt"), "its form `K`, ASCII");
        assert!(finds("\u{212a}", true, "OK \u{e9}.txt"));
        // Its forms, each once: as typed, composed, decomposed.
        for (term, patterns) in [
            ("\u{f900}", 2),
            ("\u{1f71}", 3),
            ("\u{1f71}*", 3),
            ("caf\u{e9}", 2),
            ("cafe\u{301}", 2),
        ] {
            let matcher = NameMatch::term(term, false);
            assert_eq!(matcher.patterns(), patterns, "{term:?}");
            assert!(!matcher.as_written(), "{term:?}");
        }
        let forms = Forms::of("\u{1f71}");
        assert_eq!(
            forms.iter().collect::<Vec<_>>(),
            ["\u{1f71}", "\u{3ac}", "\u{3b1}\u{301}"]
        );
        assert_eq!(forms.count(), 3);
        assert_eq!(
            Forms::of("cafe\u{301}").iter().collect::<Vec<_>>(),
            ["cafe\u{301}", "caf\u{e9}"],
            "as typed first, the same one once"
        );
    }

    /// Case ignored, a final sigma is a sigma, on the term and on the name:
    /// a word in capitals before an extension finds itself, and a word typed
    /// in small letters, ending in `ς`, finds it in capitals.
    #[test]
    fn a_final_sigma_is_a_sigma_where_case_is_ignored() {
        for (term, name) in [
            (
                "\u{39f}\u{394}\u{39f}\u{3a3}",
                "\u{39f}\u{394}\u{39f}\u{3a3}.txt",
            ),
            (
                "\u{3bf}\u{3b4}\u{3bf}\u{3c2}",
                "\u{39f}\u{394}\u{39f}\u{3a3}.txt",
            ),
            (
                "\u{3bb}\u{3bf}\u{3b3}\u{3b1}\u{3c1}\u{3b9}\u{3b1}\u{3c3}\u{3bc}\u{3bf}\u{3c2}",
                "\u{39b}\u{39f}\u{393}\u{391}\u{3a1}\u{399}\u{391}\u{3a3}\u{39c}\u{39f}\u{3a3}.pdf",
            ),
            (
                "\u{39f}\u{394}\u{39f}\u{3a3}",
                "\u{3bf}\u{3b4}\u{3bf}\u{3c2}.txt",
            ),
            (
                "\u{3bf}\u{3b4}\u{3bf}\u{3c2}",
                "\u{39f}\u{394}\u{39f}\u{3a3} \u{39c}\u{39f}\u{3a5}.txt",
            ),
            (
                "\u{3bf}\u{3b4}\u{3bf}\u{3c3}",
                "\u{3bf}\u{3b4}\u{3bf}\u{3c2}.txt",
            ),
            (
                "\u{39f}\u{394}\u{39f}\u{3a3}*",
                "\u{39f}\u{394}\u{39f}\u{3a3}.txt",
            ),
            (
                "*\u{3bf}\u{3b4}\u{3bf}\u{3c2}.txt",
                "\u{39f}\u{394}\u{39f}\u{3a3}.txt",
            ),
        ] {
            assert!(finds(term, false, name), "{term:?} in {name:?}");
        }
        // The name filter, small letters typed.
        assert!(
            NameMatch::words("\u{3bf}\u{3b4}\u{3bf}\u{3c2}", false)
                .matches(OsStr::new("\u{39f}\u{394}\u{39f}\u{3a3}.txt"))
        );
        // As typed where case counts.
        assert!(!finds(
            "\u{3bf}\u{3b4}\u{3bf}\u{3c2}",
            true,
            "\u{3bf}\u{3b4}\u{3bf}\u{3c3}.txt"
        ));
        assert_eq!(
            fold("\u{39f}\u{394}\u{39f}\u{3a3}"),
            "\u{3bf}\u{3b4}\u{3bf}\u{3c3}"
        );
        assert_eq!(fold("\u{3a3}\u{391}\u{3a3}"), "\u{3c3}\u{3b1}\u{3c3}");
        assert_eq!(fold("Stra\u{df}e"), "stra\u{df}e");
        // Lowercased otherwise than `to_lowercase` lowers it: not as written.
        assert!(!NameMatch::words("\u{39f}\u{394}\u{39f}\u{3a3}", false).as_written());
        assert!(!NameMatch::words("\u{3bf}\u{3b4}\u{3bf}\u{3c3}", false).as_written());
        assert!(NameMatch::words("\u{39f}\u{394}\u{39f}\u{3a3}", true).as_written());
        assert!(NameMatch::words("\u{3bf}\u{3b4}\u{3bf}", false).as_written());
    }

    /// Case ignored, a glob beyond ASCII is folded whole, as the name is:
    /// what an escape escapes, a class's single members; a range's ends as
    /// typed.
    #[test]
    fn a_glob_folds_its_escapes_and_classes_where_case_is_ignored() {
        if !std::path::is_separator('\\') {
            for (term, name, case_sensitive, found) in [
                ("\u{e9}*\\\u{c9}*", "\u{e9}t\u{c9}.txt", false, true),
                ("\u{e9}*\\\u{c9}*", "\u{c9}T\u{c9}.txt", false, true),
                ("\\\u{c9}*", "\u{e9}t\u{e9}.txt", false, true),
                ("\\\u{c9}*", "\u{e9}t\u{e9}.txt", true, false),
                ("\\\u{c9}*", "\u{c9}t\u{e9}.txt", true, true),
                ("\\\u{3a3}*", "\u{3c2}.txt", false, true),
            ] {
                assert_eq!(
                    finds(term, case_sensitive, name),
                    found,
                    "{term:?} in {name:?}, case_sensitive {case_sensitive}"
                );
            }
        }
        // globset reads a class's members beyond ASCII as bytes: one
        // folded is the byte a name folded starts with.
        assert!(finds("[\u{3a9}]*", false, "\u{3c9}mega.txt"));
        assert!(finds("[\u{3a9}]*", false, "\u{3a9}mega.txt"));
        // A range's ends as typed: folded, these would cross.
        assert!(NameMatch::term("[\u{3a9}-\u{3b0}]*", false).is_glob());
        for (class, folded) in [
            ("[\u{c9}]", "[\u{e9}]"),
            ("[!\u{c9}\u{3a9}]", "[!\u{e9}\u{3c9}]"),
            ("[^\u{3a3}]", "[^\u{3c3}]"),
            ("[]\u{c9}]", "[]\u{e9}]"),
            ("[\u{c9}-]", "[\u{e9}-]"),
            ("[-\u{c9}]", "[-\u{e9}]"),
            ("[a-c\u{c9}]", "[a-c\u{e9}]"),
            ("[[:alpha:]\u{c9}]", "[[:alpha:]\u{e9}]"),
            ("[\u{c0}-\u{de}\u{c9}]", "[\u{c0}-\u{de}\u{e9}]"),
            ("[\u{c9}-\u{3a9}]", "[\u{c9}-\u{3a9}]"),
            ("[a-\u{c9}]", "[a-\u{c9}]"),
            ("[a-c-\u{c9}]", "[a-c-\u{c9}]"),
            ("[\u{130}]", "[\u{130}]"),
            ("[\u{e9}]", "[\u{e9}]"),
        ] {
            assert_eq!(fold_class(class), folded, "{class:?}");
        }
    }
}
