//! The languages Skwir says its words in (ADR-139, 6): the system's, as
//! `skwir_platform::locale` reads them (handed in as Skwir starts,
//! [`set_system`]), after the one the user chose (`core.language`), if
//! they chose one. English is always the last.
//!
//! Process-wide, as sizes' units are (`fs::size_units`): every reader of a
//! language (a desktop entry's name, later every message) asks here.

use parking_lot::RwLock;

/// The setting of the language Skwir speaks: `automatic` (the system's)
/// or a language's tag.
pub const LANGUAGE_KEY: &str = "core.language";

/// The setting's value for the system's languages.
pub const AUTOMATIC: &str = "automatic";

/// English, which every language falls back to, and which Skwir's words
/// are written in.
pub const ENGLISH: &str = "en";

/// What decides them: the system's languages, and the user's choice.
struct Languages {
    system: Vec<String>,
    chosen: Option<String>,
    /// The locales numbers and dates are written in, the system's
    /// (`LC_NUMERIC`, `LC_TIME`), apart from the messages' languages.
    numbers: Option<String>,
    dates: Option<String>,
    /// Moves whenever what is said would change, for what keeps what it
    /// read of them (a cache of names by language).
    revision: u64,
}

static LANGUAGES: RwLock<Languages> = RwLock::new(Languages {
    system: Vec::new(),
    chosen: None,
    numbers: None,
    dates: None,
    revision: 0,
});

/// The system's locales for numbers and for dates (`skwir_platform::locale`,
/// `LC_NUMERIC` and `LC_TIME` on Linux), `None` where it says none: the
/// window writes them as the rest of the desktop does, whatever language
/// its words are in.
pub fn set_formats(numbers: Option<String>, dates: Option<String>) {
    let mut kept = LANGUAGES.write();
    if kept.numbers != numbers || kept.dates != dates {
        kept.numbers = numbers;
        kept.dates = dates;
        kept.revision += 1;
    }
}

/// The locale numbers are written in for the window: the system's, else
/// the first language words are said in.
pub fn numbers() -> String {
    let kept = LANGUAGES.read();
    kept.numbers.clone().unwrap_or_else(|| first(&kept))
}

/// The locale dates and times are written in for the window: the system's,
/// else the first language words are said in.
pub fn dates() -> String {
    let kept = LANGUAGES.read();
    kept.dates.clone().unwrap_or_else(|| first(&kept))
}

fn first(kept: &Languages) -> String {
    kept.chosen
        .iter()
        .chain(&kept.system)
        .next()
        .cloned()
        .unwrap_or_else(|| ENGLISH.to_owned())
}

/// The system's languages, most wanted first (`skwir_platform::locale`):
/// none for its C locale.
pub fn set_system(languages: Vec<String>) {
    let mut kept = LANGUAGES.write();
    if kept.system != languages {
        kept.system = languages;
        kept.revision += 1;
    }
}

/// The language the user chose, `None` for the system's.
pub fn set_chosen(language: Option<String>) {
    let mut kept = LANGUAGES.write();
    if kept.chosen != language {
        kept.chosen = language;
        kept.revision += 1;
    }
}

/// The languages to say words in, most wanted first: the one chosen, then
/// the system's, then English, each once.
pub fn languages() -> Vec<String> {
    let kept = LANGUAGES.read();
    let mut languages: Vec<String> = Vec::new();
    for language in kept
        .chosen
        .iter()
        .chain(&kept.system)
        .map(String::as_str)
        .chain([ENGLISH])
    {
        if !languages.iter().any(|kept| kept == language) {
            languages.push(language.to_owned());
        }
    }
    languages
}

/// What moves whenever [`languages`] would answer otherwise.
pub fn revision() -> u64 {
    LANGUAGES.read().revision
}

/// The language the setting's value `value` chooses: `None` for
/// Automatic, or a value that names no language.
pub fn chosen_of(value: &str) -> Option<String> {
    (value != AUTOMATIC && !value.is_empty()).then(|| value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::{AUTOMATIC, chosen_of};

    /// The languages, as each reader asks for them: the one chosen first,
    /// the system's, English last, each once. One test, as they are kept
    /// for the whole process.
    #[test]
    fn the_one_chosen_comes_first_and_english_last() {
        assert_eq!(chosen_of(AUTOMATIC), None);
        assert_eq!(chosen_of("fr").as_deref(), Some("fr"));
        let before = super::revision();
        super::set_system(vec!["fr-FR".into(), "en-GB".into()]);
        assert_eq!(super::languages(), ["fr-FR", "en-GB", "en"]);
        super::set_chosen(Some("de".into()));
        assert_eq!(super::languages(), ["de", "fr-FR", "en-GB", "en"]);
        super::set_chosen(Some("en".into()));
        assert_eq!(super::languages(), ["en", "fr-FR", "en-GB"]);
        assert!(super::revision() > before);
        let now = super::revision();
        super::set_chosen(Some("en".into()));
        assert_eq!(super::revision(), now, "nothing changed");
        super::set_system(Vec::new());
        super::set_chosen(None);
        assert_eq!(super::languages(), ["en"]);
    }
}
