//! The scripts text is written in, and those the reader's languages are
//! written in (ADR-139): what tells a letter no font draws in a language
//! the user reads from one of a stray name (the window's missing-glyph
//! warning), and which forms of the Han characters a reader of Japanese,
//! Korean or Chinese expects (`han_language`: 直 and 骨 are one character
//! each, drawn apart in Japan, Korea, China, Taiwan and Hong Kong).
//!
//! A text's script is Unicode's Script property (ICU4X's compiled data,
//! which core holds already for collation); a language's is the one
//! `locale::script_of` reads from CLDR's likely subtags. Nothing is listed
//! here but how ISO 15924's scripts of Japanese, Korean and the two
//! Chinese writings break into Unicode's.

use icu_locale_core::LanguageIdentifier;
use icu_locale_core::subtags;
use icu_properties::props::Script as Property;
use icu_properties::{CodePointMapData, PropertyNamesShort, PropertyParser};

/// A script, as ISO 15924 names it (`Latn`, `Deva`, `Hani`): Unicode's
/// Script property's short name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Script(subtags::Script);

impl Script {
    /// The script ISO 15924 writes `code` (`Deva`), when Unicode knows it.
    pub fn named(code: &str) -> Option<Script> {
        let property = PropertyParser::<Property>::new().get_strict(code)?;
        unicode_script(property)
    }

    /// Its ISO 15924 code (`Deva`): for a log, a table's key, a tag; never a
    /// label.
    pub fn code(&self) -> &str {
        self.0.as_str()
    }
}

/// What a Unicode Script value is, as ISO 15924 writes it; none for those
/// that are no script of their own (Common: digits, punctuation, most
/// symbols and emoji; Inherited: combining marks; Unknown).
fn unicode_script(property: Property) -> Option<Script> {
    if matches!(
        property,
        Property::Common | Property::Inherited | Property::Unknown
    ) {
        return None;
    }
    PropertyNamesShort::<Property>::new()
        .get_locale_script(property)
        .map(Script)
}

/// The script `text` is written in: its first letter's that is written in
/// one, so "2026年" is Han and "(a)" Latin. None for text of digits,
/// punctuation, symbols and emoji alone.
pub fn of(text: &str) -> Option<Script> {
    let map = CodePointMapData::<Property>::new();
    text.chars().find_map(|c| unicode_script(map.get(c)))
}

/// The scripts a reader of `language` (a BCP 47 tag) reads: the one its
/// tag names, else the one CLDR says it is most likely written in. Japanese
/// reads Han and both kanas, Korean Hangul and Han, Chinese Han (and
/// Bopomofo where it is written in Traditional Han). None for a tag that
/// does not parse, or whose script is not known.
pub fn of_language(language: &str) -> Vec<Script> {
    let Some(written) = crate::locale::script_of(language) else {
        return Vec::new();
    };
    // CLDR writes Japanese, Korean and Chinese in scripts that are several
    // of Unicode's at once (ISO 15924's Jpan, Kore, Hans, Hant).
    let codes: &[&str] = match written.as_str() {
        "Jpan" => &["Hani", "Hira", "Kana"],
        "Kore" => &["Hang", "Hani"],
        "Hans" => &["Hani"],
        "Hant" => &["Hani", "Bopo"],
        code => return Script::named(code).into_iter().collect(),
    };
    codes
        .iter()
        .filter_map(|code| Script::named(code))
        .collect()
}

/// The scripts the user reads: those of each of their languages
/// (`locale::preferred`: the one chosen, the system's, English), each once.
pub fn read() -> Vec<Script> {
    read_in(&crate::locale::preferred())
}

/// The scripts read in `languages`, each once, in their order.
fn read_in(languages: &[String]) -> Vec<Script> {
    let mut read: Vec<Script> = Vec::new();
    for script in languages.iter().flat_map(|language| of_language(language)) {
        if !read.contains(&script) {
            read.push(script);
        }
    }
    read
}

/// Which forms of the Han characters a reader expects: one character,
/// drawn as each place writes it (a font's face for each, as Noto CJK's
/// JP, KR, SC, TC and HK).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HanForms {
    Japanese,
    Korean,
    /// Simplified Chinese, as the mainland and Singapore write it.
    Simplified,
    /// Traditional Chinese, as Taiwan writes it.
    Traditional,
    /// Traditional Chinese, as Hong Kong and Macao write it.
    HongKong,
}

/// The first of the reader's languages written in Han characters, and the
/// forms it reads them in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HanLanguage {
    /// Its tag, as the reader's languages write it (`ja`, `zh-TW`).
    pub tag: String,
    pub forms: HanForms,
}

/// The first of the user's languages (`locale::preferred`) written in Han
/// characters: what forms of them to draw, whatever language the window
/// speaks. None when they read none.
pub fn han_language() -> Option<HanLanguage> {
    han_in(&crate::locale::preferred())
}

/// The first of `languages` written in Han characters.
fn han_in(languages: &[String]) -> Option<HanLanguage> {
    languages.iter().find_map(|language| {
        let script = crate::locale::script_of(language)?;
        // Traditional Han as Hong Kong and Macao write it, where the tag
        // names either (`zh-HK`); else as Taiwan does (`zh-Hant`, `yue`).
        let id = language.parse::<LanguageIdentifier>().ok()?;
        let hong_kong = matches!(
            id.region.as_ref().map(subtags::Region::as_str),
            Some("HK" | "MO")
        );
        let forms = match script.as_str() {
            "Jpan" => HanForms::Japanese,
            "Kore" => HanForms::Korean,
            "Hans" => HanForms::Simplified,
            "Hant" if hong_kong => HanForms::HongKong,
            "Hant" => HanForms::Traditional,
            _ => return None,
        };
        Some(HanLanguage {
            tag: language.clone(),
            forms,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codes(scripts: &[Script]) -> Vec<&str> {
        scripts.iter().map(Script::code).collect()
    }

    fn tags(languages: &[&str]) -> Vec<String> {
        languages.iter().map(|tag| (*tag).to_owned()).collect()
    }

    #[test]
    fn a_text_is_written_in_its_first_letter_s_script() {
        assert_eq!(
            of("直 骨 誤.txt").map(|s| s.code().to_owned()),
            Some("Hani".into())
        );
        assert_eq!(
            of("2026年").map(|s| s.code().to_owned()),
            Some("Hani".into())
        );
        assert_eq!(of("かな").map(|s| s.code().to_owned()), Some("Hira".into()));
        assert_eq!(
            of("(नमस्ते)").map(|s| s.code().to_owned()),
            Some("Deva".into())
        );
        assert_eq!(
            of("Ελληνικά").map(|s| s.code().to_owned()),
            Some("Grek".into())
        );
        assert_eq!(of("(a)").map(|s| s.code().to_owned()), Some("Latn".into()));
        // Digits, punctuation, emoji and marks alone are no script's.
        assert_eq!(of("12 - 3!"), None);
        assert_eq!(of("😀"), None);
        assert_eq!(of("\u{301}"), None);
        assert_eq!(of(""), None);
    }

    #[test]
    fn a_language_reads_the_scripts_its_tag_names() {
        assert_eq!(codes(&of_language("ja-Jpan")), ["Hani", "Hira", "Kana"]);
        assert_eq!(codes(&of_language("ko-Kore-KR")), ["Hang", "Hani"]);
        assert_eq!(codes(&of_language("zh-Hans")), ["Hani"]);
        assert_eq!(codes(&of_language("zh-Hant-TW")), ["Hani", "Bopo"]);
        assert_eq!(codes(&of_language("hi-Deva")), ["Deva"]);
        assert_eq!(codes(&of_language("sr-Latn")), ["Latn"]);
        assert_eq!(codes(&of_language("sr-Cyrl")), ["Cyrl"]);
        assert!(of_language("not a tag").is_empty());
    }

    #[test]
    fn a_language_reads_the_scripts_cldr_writes_it_in() {
        assert_eq!(codes(&of_language("ja")), ["Hani", "Hira", "Kana"]);
        assert_eq!(codes(&of_language("ja-JP")), ["Hani", "Hira", "Kana"]);
        assert_eq!(codes(&of_language("ko-KR")), ["Hang", "Hani"]);
        assert_eq!(codes(&of_language("zh")), ["Hani"]);
        assert_eq!(codes(&of_language("zh-TW")), ["Hani", "Bopo"]);
        assert_eq!(codes(&of_language("hi")), ["Deva"]);
        assert_eq!(codes(&of_language("ru")), ["Cyrl"]);
        assert_eq!(codes(&of_language("ar-EG")), ["Arab"]);
        assert_eq!(codes(&of_language("fr")), ["Latn"]);
        assert_eq!(
            codes(&read_in(&tags(&["ja-JP", "zh-CN", "en"]))),
            ["Hani", "Hira", "Kana", "Latn"]
        );
    }

    #[test]
    fn the_reader_reads_each_script_once() {
        assert_eq!(
            codes(&read_in(&tags(&["ja-Jpan-JP", "zh-Hans-CN", "en-Latn"]))),
            ["Hani", "Hira", "Kana", "Latn"]
        );
        assert_eq!(codes(&read_in(&tags(&["fr-Latn", "en-Latn"]))), ["Latn"]);
    }

    #[test]
    fn the_han_forms_are_the_first_han_language_s() {
        let forms = |languages: &[&str]| han_in(&tags(languages)).map(|han| han.forms);
        assert_eq!(forms(&["ja-Jpan-JP"]), Some(HanForms::Japanese));
        assert_eq!(forms(&["fr-Latn", "ja-Jpan"]), Some(HanForms::Japanese));
        assert_eq!(forms(&["ko-Kore"]), Some(HanForms::Korean));
        assert_eq!(forms(&["zh-Hans-SG"]), Some(HanForms::Simplified));
        assert_eq!(forms(&["zh-Hant"]), Some(HanForms::Traditional));
        assert_eq!(forms(&["zh-Hant-HK"]), Some(HanForms::HongKong));
        assert_eq!(forms(&["zh-Hant-MO"]), Some(HanForms::HongKong));
        assert_eq!(forms(&["en-Latn", "de-Latn"]), None);
        assert_eq!(
            han_in(&tags(&["de-Latn", "zh-Hant-TW", "ja-Jpan"])).map(|han| han.tag),
            Some("zh-Hant-TW".to_owned())
        );
    }

    #[test]
    fn the_han_forms_of_bare_tags() {
        let forms = |languages: &[&str]| han_in(&tags(languages)).map(|han| han.forms);
        assert_eq!(forms(&["ja-JP"]), Some(HanForms::Japanese));
        assert_eq!(forms(&["fr", "ja", "en"]), Some(HanForms::Japanese));
        assert_eq!(forms(&["ko"]), Some(HanForms::Korean));
        assert_eq!(forms(&["zh"]), Some(HanForms::Simplified));
        assert_eq!(forms(&["zh-SG"]), Some(HanForms::Simplified));
        assert_eq!(forms(&["zh-TW"]), Some(HanForms::Traditional));
        assert_eq!(forms(&["zh-HK"]), Some(HanForms::HongKong));
        assert_eq!(forms(&["en", "de"]), None);
    }
}
