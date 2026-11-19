//! Which way a language is written (ADR-139, 12): from its script, the
//! one its tag names (`az-Arab`) or the one CLDR says it is most likely
//! written in (`ar`, `he`, `fa`: ICU4X's `LocaleDirectionality`), never
//! from a list of languages kept here. The window is laid out in its first
//! language's direction (`window`): mirrored while it speaks one written
//! right to left, which no shipped language does yet and the right-to-left
//! pseudo-language does (`PSEUDO_RTL`), to try the mirrored window with.

use std::cell::Cell;

use icu_locale::LocaleDirectionality;
use icu_locale_core::LanguageIdentifier;

/// The right-to-left pseudo-language (CLDR's `ar-XB`, "pseudo-bidi"):
/// English written as a right-to-left language is, each word's letters as
/// they are, the words of a sentence in the order a right-to-left
/// sentence draws them (`pseudo_rtl`). Its tag is Arabic's, so it is
/// right to left as Arabic is (`of`), and the window it is spoken in is
/// mirrored; its words are English's, said as English says them.
pub const PSEUDO_RTL: &str = "ar-XB";

/// RIGHT-TO-LEFT MARK (U+200F): a strong right-to-left character that
/// draws nothing.
const RLM: char = '\u{200F}';

/// Which way a language is written, and a window laid out for it; which
/// way a document reads (its pages turned, its text), where it says so.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// Left to right: Latin, Cyrillic, Greek, CJK, Indic scripts…
    #[default]
    LeftToRight,
    /// Right to left: Arabic, Hebrew, Syriac, Thaana, N'Ko, Adlam…
    RightToLeft,
}

impl Direction {
    /// Whether it reads right to left.
    pub fn is_right_to_left(self) -> bool {
        self == Direction::RightToLeft
    }
}

/// The direction `language` (a BCP 47 tag: `fr`, `ar-EG`, `sr-Latn`,
/// `ks-Deva`) is written in: its script's. Left to right for a tag that
/// does not parse or whose script CLDR does not know.
pub fn of(language: &str) -> Direction {
    let Ok(id) = language.parse::<LanguageIdentifier>() else {
        return Direction::LeftToRight;
    };
    // CLDR's likely scripts and each script's direction, compiled in: of
    // every language CLDR knows, as the common ones leave Yiddish and
    // Divehi out. Made in place, pointers to static data.
    match LocaleDirectionality::new_extended().get(&id) {
        Some(icu_locale::Direction::RightToLeft) => Direction::RightToLeft,
        _ => Direction::LeftToRight,
    }
}

/// The direction the window is laid out in: that of the first language it
/// speaks its words in (`locale::languages`, the first Skwir has words in:
/// English, a language it ships, a pseudo-language), so a system's Arabic
/// that Skwir does not offer, or a Hebrew written into the settings file
/// that it has no words in, leaves it left to right, in the English it
/// speaks. Kept on each thread until what is said moves (`words::revision`:
/// the languages, and the catalogs and shares that decide which are
/// spoken): a frame asks it for every side it lays out.
pub fn window() -> Direction {
    thread_local! {
        static KEPT: Cell<(u64, Direction)> = const { Cell::new((u64::MAX, Direction::LeftToRight)) };
    }
    let now = crate::words::revision();
    KEPT.with(|kept| {
        let (at, direction) = kept.get();
        if at == now {
            return direction;
        }
        let shipped = crate::words::shipped_languages();
        let direction = crate::locale::languages()
            .iter()
            .find(|language| has_words(language, &shipped))
            .map_or(Direction::LeftToRight, |language| of(language));
        kept.set((now, direction));
        direction
    })
}

/// Whether Skwir says its words in `language`: English, a pseudo-language
/// (English said so), or one of the languages its catalogs are in, found as
/// its words are (`locale::matching`: by script, then region), so a
/// language written in another script than a catalog's (`az-Arab` beside
/// `az`, `sr-Latn` beside `sr`) has none, and is not laid out as though it
/// had.
fn has_words(language: &str, shipped: &[&str]) -> bool {
    !crate::locale::matching(language, shipped.iter().copied()).is_empty()
}

/// English said in the right-to-left pseudo-language: a right-to-left mark
/// first, so the sentence's direction (its first strong character's, UAX #9
/// P2) is right to left, and one on each side of every space between its
/// words, so each word is a run of its own, set from the right: "Copy to
/// Docs" draws as "Docs to Copy", each word's letters as they are, the
/// sentence's punctuation at its left end. Arguments (between the marks
/// `‹` and `›` the words put around each, nested ones included) are left
/// as they are: a name's words keep their order, and the name is isolated
/// from the sentence where the window draws it.
pub fn pseudo_rtl(english: &str) -> String {
    let mut out = String::with_capacity(english.len() + english.len() / 2 + 3);
    out.push(RLM);
    let mut depth = 0_usize;
    for c in english.chars() {
        match c {
            '‹' => depth += 1,
            '›' if depth > 0 => depth -= 1,
            c if depth == 0 && c.is_whitespace() => {
                out.push(RLM);
                out.push(c);
                out.push(RLM);
                continue;
            }
            _ => {}
        }
        out.push(c);
    }
    out
}

/// The direction of `text`'s first letter that has one (UAX #9's P2 and
/// P3): right to left for a Hebrew or an Arabic one (classes R and AL),
/// left to right for a Latin, Greek or Chinese one (L), what an isolate
/// holds (from LRI, RLI or FSI to its PDI) passed over, a paragraph's end
/// not stopping it, as HTML's `dir="auto"` reads an element's text.
/// `None` for none: digits, punctuation, nothing.
pub fn of_text(text: &str) -> Option<Direction> {
    use icu_properties::props::BidiClass;
    let classes = icu_properties::CodePointMapData::<BidiClass>::new();
    let mut isolated = 0usize;
    for c in text.chars() {
        match classes.get(c) {
            BidiClass::LeftToRightIsolate
            | BidiClass::RightToLeftIsolate
            | BidiClass::FirstStrongIsolate => isolated += 1,
            BidiClass::PopDirectionalIsolate => isolated = isolated.saturating_sub(1),
            _ if isolated > 0 => {}
            BidiClass::LeftToRight => return Some(Direction::LeftToRight),
            BidiClass::RightToLeft | BidiClass::ArabicLetter => {
                return Some(Direction::RightToLeft);
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    /// A text's direction is its first letter's that has one: a Hebrew or
    /// an Arabic letter's after digits and punctuation, what an isolate
    /// holds passed over; none for digits alone.
    #[test]
    fn a_text_reads_as_its_first_letter_with_a_direction() {
        assert_eq!(of_text("שלום, עולם"), Some(Direction::RightToLeft));
        assert_eq!(of_text("2026 — مرحبا"), Some(Direction::RightToLeft));
        assert_eq!(of_text("Hello שלום"), Some(Direction::LeftToRight));
        assert_eq!(of_text("« Été »"), Some(Direction::LeftToRight));
        assert_eq!(
            of_text("\u{2068}Docs\u{2069} שלום"),
            Some(Direction::RightToLeft),
            "an isolate passed over"
        );
        assert_eq!(of_text("\u{200F}2026"), Some(Direction::RightToLeft));
        assert_eq!(of_text("2026 (3)"), None);
        assert_eq!(of_text(""), None);
    }

    use super::{Direction, PSEUDO_RTL, of, of_text, pseudo_rtl};

    #[test]
    fn a_language_is_written_as_its_script_is() {
        for rtl in [
            "ar", "ar-EG", "he", "fa", "ur", "ps", "yi", "ckb", "sd", "ug", "dv", "syr", "az-Arab",
            "pa-Arab", "ks-Arab", "en-Arab", "nqo", "ff-Adlm", PSEUDO_RTL,
        ] {
            assert_eq!(of(rtl), Direction::RightToLeft, "{rtl}");
        }
        for ltr in [
            "en", "en-XA", "fr", "fr-FR", "de", "ru", "el", "ja", "zh-Hant", "hi", "th", "tr",
            "az", "pa", "sr-Latn", "ks-Deva", "ff",
        ] {
            assert_eq!(of(ltr), Direction::LeftToRight, "{ltr}");
        }
        // What names no script known, or no tag, is left to right.
        for unknown in ["", "not a tag", "zxx", "und"] {
            assert_eq!(of(unknown), Direction::LeftToRight, "{unknown}");
        }
    }

    /// The window is mirrored only while it speaks a right-to-left
    /// language: one chosen that Skwir has no words in leaves it as the
    /// English it speaks. One test: the languages are the process's.
    #[test]
    fn the_window_is_laid_out_as_the_language_it_speaks() {
        use crate::locale::{set_chosen, set_system};
        assert_eq!(super::window(), Direction::LeftToRight);
        set_chosen(Some(PSEUDO_RTL.into()));
        assert_eq!(super::window(), Direction::RightToLeft);
        set_chosen(Some(crate::words::PSEUDO.into()));
        assert_eq!(super::window(), Direction::LeftToRight);
        // Hebrew, chosen or the system's, with no words in it.
        set_chosen(Some("he".into()));
        assert_eq!(super::window(), Direction::LeftToRight);
        set_chosen(None);
        set_system(vec!["ar-EG".into(), "fr-FR".into()]);
        assert_eq!(super::window(), Direction::LeftToRight);
        set_system(Vec::new());
        let shipped = ["fr", "he", "az", "sr"];
        assert!(super::has_words("he-IL", &shipped));
        assert!(super::has_words("FR", &shipped));
        assert!(super::has_words("en-GB", &shipped));
        assert!(super::has_words("AR-xb", &shipped));
        assert!(super::has_words("en-XA", &shipped));
        assert!(!super::has_words("ar", &shipped));
        // Another script than the catalog's has none of its words: English,
        // laid out left to right.
        assert!(super::has_words("az-Latn", &shipped));
        assert!(!super::has_words("az-Arab", &shipped));
        assert!(!super::has_words("sr-Latn", &shipped));
    }

    #[test]
    fn the_pseudo_language_sets_the_words_from_the_right() {
        const R: char = '\u{200F}';
        assert_eq!(
            pseudo_rtl("Copy to Docs"),
            format!("{R}Copy{R} {R}to{R} {R}Docs")
        );
        // A word's letters and its punctuation stay with it; the sentence
        // starts right to left even where it starts with a digit.
        assert_eq!(pseudo_rtl("3 files?"), format!("{R}3{R} {R}files?"));
        // An argument is as it is, its spaces included, nested words too.
        assert_eq!(
            pseudo_rtl("Moved ‹“my file.txt”› to ‹in ‹a b››."),
            format!("{R}Moved{R} {R}‹“my file.txt”›{R} {R}to{R} {R}‹in ‹a b››."),
        );
        // A number and its unit: the no-break space is a space between
        // words, the marks beside it glue (gpui wraps at a plain space only).
        assert_eq!(pseudo_rtl("‹3›\u{a0}MB"), format!("{R}‹3›{R}\u{a0}{R}MB"));
        // Each line starts right to left.
        assert_eq!(pseudo_rtl("a\nb"), format!("{R}a{R}\n{R}b"));
        // A closing mark with none open is a character of the sentence.
        assert_eq!(pseudo_rtl("a › b"), format!("{R}a{R} {R}›{R} {R}b"));
        assert_eq!(pseudo_rtl(""), R.to_string());
    }
}
