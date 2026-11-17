//! The user's locale as the system says it (ADR-139, 6): the languages
//! messages are wanted in, most wanted first, and the locale each kind of
//! format is read in (dates, numbers, the order of names, paper), apart
//! from messages. Every name is a BCP 47 tag (`fr-FR`, `sr-Latn-RS`).
//!
//! On Linux, gettext's rules: messages from `LC_ALL`, else `LC_MESSAGES`,
//! else `LANG`, where `LANGUAGE`, a list, comes first, unless the locale is
//! C (`C`, `C.UTF-8`, `POSIX`), which means English and ignores it; each
//! other kind from `LC_ALL`, else its own variable, else `LANG`. A variable
//! set to nothing is not set. On macOS, the preferred languages and the
//! region's locale; on Windows, the display languages and the user's
//! locale.
//!
//! What Skwir says is in `skwir_core::locale`, which is handed the system's
//! languages as Skwir starts and adds the user's choice
//! (`core.language`).

/// A kind of what a locale decides, beside messages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    /// Dates and times (`LC_TIME`).
    Time,
    /// Numbers (`LC_NUMERIC`).
    Numeric,
    /// The order of names (`LC_COLLATE`).
    Collate,
    /// The paper printed on (`LC_PAPER`).
    Paper,
}

impl Category {
    /// Its variable on Linux.
    #[cfg_attr(not(any(all(unix, not(target_os = "macos")), test)), allow(dead_code))]
    fn variable(self) -> &'static str {
        match self {
            Category::Time => "LC_TIME",
            Category::Numeric => "LC_NUMERIC",
            Category::Collate => "LC_COLLATE",
            Category::Paper => "LC_PAPER",
        }
    }
}

/// The languages the user wants messages in, most wanted first, as the
/// system says them; none for the C locale or a system that says none,
/// which is English. Read each time: cheap, and never on a hot path.
pub fn languages() -> Vec<String> {
    system::languages()
}

/// The locale `category` is read in, as the system says it; `None` for
/// the C locale or none said.
pub fn of(category: Category) -> Option<String> {
    system::of(category)
}

/// Linux's rules, read from `var`: the languages messages are wanted in.
#[cfg_attr(not(any(all(unix, not(target_os = "macos")), test)), allow(dead_code))]
fn languages_from(var: &dyn Fn(&str) -> Option<String>) -> Vec<String> {
    let set = |name: &str| var(name).filter(|value| !value.is_empty());
    let Some(messages) = set("LC_ALL")
        .or_else(|| set("LC_MESSAGES"))
        .or_else(|| set("LANG"))
    else {
        return Vec::new();
    };
    let Some(locale) = bcp47(&messages) else {
        // The C locale: English, whatever LANGUAGE says.
        return Vec::new();
    };
    let mut languages: Vec<String> = Vec::new();
    for listed in set("LANGUAGE").iter().flat_map(|list| list.split(':')) {
        if let Some(language) = bcp47(listed)
            && !languages.contains(&language)
        {
            languages.push(language);
        }
    }
    if languages.is_empty() {
        languages.push(locale);
    }
    languages
}

/// Linux's rules, read from `var`: the locale `category` is read in.
#[cfg_attr(not(any(all(unix, not(target_os = "macos")), test)), allow(dead_code))]
fn of_from(var: &dyn Fn(&str) -> Option<String>, category: Category) -> Option<String> {
    let set = |name: &str| var(name).filter(|value| !value.is_empty());
    let locale = set("LC_ALL")
        .or_else(|| set(category.variable()))
        .or_else(|| set("LANG"))?;
    bcp47(&locale)
}

/// A POSIX locale's name (`fr_FR.UTF-8@euro`, `sr_RS@latin`) as a BCP 47
/// tag (`fr-FR`, `sr-Latn-RS`): its codeset left out, a modifier that names
/// a script or a variant kept as one, any other dropped. `None` for the C
/// locale (`C`, `C.UTF-8`, `POSIX`) and for what names no language.
pub fn bcp47(posix: &str) -> Option<String> {
    let (rest, modifier) = posix
        .split_once('@')
        .map_or((posix, None), |(rest, modifier)| (rest, Some(modifier)));
    let rest = rest.split('.').next().unwrap_or_default();
    let (language, region) = rest
        .split_once(['_', '-'])
        .map_or((rest, None), |(language, region)| (language, Some(region)));
    let lower = language.to_ascii_lowercase();
    if lower.is_empty() || lower == "c" || lower == "posix" {
        return None;
    }
    if !(2..=3).contains(&lower.len()) || !lower.bytes().all(|b| b.is_ascii_lowercase()) {
        return None;
    }
    let (script, variant) = match modifier.map(str::to_ascii_lowercase).as_deref() {
        Some("latin") => (Some("Latn"), None),
        Some("cyrillic") => (Some("Cyrl"), None),
        Some("devanagari") => (Some("Deva"), None),
        Some("valencia") => (None, Some("valencia")),
        // `@euro` and the rest say nothing a tag holds.
        _ => (None, None),
    };
    let mut tag = lower;
    if let Some(script) = script {
        tag.push('-');
        tag.push_str(script);
    }
    if let Some(region) = region.filter(|region| {
        (region.len() == 2 && region.bytes().all(|b| b.is_ascii_alphabetic()))
            || (region.len() == 3 && region.bytes().all(|b| b.is_ascii_digit()))
    }) {
        tag.push('-');
        tag.push_str(&region.to_ascii_uppercase());
    }
    if let Some(variant) = variant {
        tag.push('-');
        tag.push_str(variant);
    }
    Some(tag)
}

#[cfg(all(unix, not(target_os = "macos")))]
mod system {
    use super::Category;

    fn var(name: &str) -> Option<String> {
        std::env::var(name).ok()
    }

    pub fn languages() -> Vec<String> {
        super::languages_from(&var)
    }

    pub fn of(category: Category) -> Option<String> {
        super::of_from(&var, category)
    }
}

#[cfg(target_os = "macos")]
mod system {
    use super::Category;
    use objc2_foundation::NSLocale;

    /// The preferred languages (System Settings › Language & Region), the
    /// app's own first where the user chose one for it.
    pub fn languages() -> Vec<String> {
        let mut languages = Vec::new();
        for language in &NSLocale::preferredLanguages() {
            if let Some(tag) = super::bcp47(&language.to_string())
                && !languages.contains(&tag)
            {
                languages.push(tag);
            }
        }
        languages
    }

    /// Every kind from the region's locale, its formats as the user set
    /// them.
    pub fn of(_category: Category) -> Option<String> {
        let identifier = NSLocale::currentLocale().localeIdentifier().to_string();
        super::bcp47(&identifier)
    }
}

#[cfg(windows)]
mod system {
    use super::Category;

    /// The display languages, most wanted first: only those installed.
    pub fn languages() -> Vec<String> {
        let mut languages = Vec::new();
        for name in crate::windows::preferred_ui_languages() {
            if let Some(tag) = super::bcp47(&name)
                && !languages.contains(&tag)
            {
                languages.push(tag);
            }
        }
        languages
    }

    /// Every kind from the user's locale (Settings › Time & language ›
    /// Region), its formats as the user set them.
    pub fn of(_category: Category) -> Option<String> {
        super::bcp47(&crate::windows::user_locale_name()?)
    }
}

#[cfg(test)]
mod tests {
    use super::{Category, bcp47, languages_from, of_from};
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |name| map.get(name).cloned()
    }

    #[test]
    fn posix_names_are_read_as_tags() {
        assert_eq!(bcp47("fr_FR.UTF-8").as_deref(), Some("fr-FR"));
        assert_eq!(bcp47("fr_FR.UTF-8@euro").as_deref(), Some("fr-FR"));
        assert_eq!(bcp47("sr_RS@latin").as_deref(), Some("sr-Latn-RS"));
        assert_eq!(bcp47("ca_ES@valencia").as_deref(), Some("ca-ES-valencia"));
        assert_eq!(bcp47("de").as_deref(), Some("de"));
        assert_eq!(bcp47("pt_BR").as_deref(), Some("pt-BR"));
        assert_eq!(bcp47("es_419").as_deref(), Some("es-419"));
        // Already a tag (macOS, Windows).
        assert_eq!(bcp47("zh-TW").as_deref(), Some("zh-TW"));
        for c in ["C", "C.UTF-8", "POSIX", "", "c.utf8"] {
            assert_eq!(bcp47(c), None, "{c}");
        }
        assert_eq!(bcp47("not a locale"), None);
    }

    #[test]
    fn messages_follow_gettext_s_rules() {
        let of = |pairs: &[(&str, &str)]| languages_from(&env(pairs));
        assert_eq!(of(&[("LANG", "fr_FR.UTF-8")]), ["fr-FR"]);
        // LANGUAGE first, as a list; a locale named twice once.
        assert_eq!(
            of(&[("LANG", "fr_FR.UTF-8"), ("LANGUAGE", "fr:en_GB:fr")]),
            ["fr", "en-GB"]
        );
        // Under the C locale, English: LANGUAGE is not read.
        assert!(of(&[("LANG", "C.UTF-8"), ("LANGUAGE", "fr")]).is_empty());
        assert!(of(&[("LANGUAGE", "fr")]).is_empty(), "no locale is C");
        // LC_ALL over LC_MESSAGES over LANG; one set to nothing is not set.
        assert_eq!(
            of(&[("LANG", "de_DE.UTF-8"), ("LC_MESSAGES", "it_IT.UTF-8")]),
            ["it-IT"]
        );
        assert_eq!(
            of(&[("LC_ALL", "es_ES.UTF-8"), ("LC_MESSAGES", "it_IT.UTF-8")]),
            ["es-ES"]
        );
        assert_eq!(of(&[("LC_ALL", ""), ("LANG", "nb_NO.UTF-8")]), ["nb-NO"]);
        assert_eq!(of(&[("LANG", "fr_FR.UTF-8"), ("LANGUAGE", "")]), ["fr-FR"]);
    }

    /// What Windows says, as tags: its display languages and its user's
    /// locale (under Wine, from the Unix locale it runs in).
    #[cfg(windows)]
    #[test]
    fn the_system_s_languages_are_tags() {
        let languages = super::languages();
        eprintln!(
            "languages {languages:?}, formats {:?}",
            super::of(Category::Time)
        );
        for language in &languages {
            assert_eq!(bcp47(language).as_deref(), Some(language.as_str()));
        }
        if let Some(locale) = super::of(Category::Time) {
            assert_eq!(bcp47(&locale).as_deref(), Some(locale.as_str()));
        }
    }

    #[test]
    fn each_kind_reads_its_own_variable() {
        let pairs = [("LANG", "en_US.UTF-8"), ("LC_TIME", "en_DK.UTF-8")];
        let var = env(&pairs);
        assert_eq!(of_from(&var, Category::Time).as_deref(), Some("en-DK"));
        assert_eq!(of_from(&var, Category::Numeric).as_deref(), Some("en-US"));
        // LC_ALL over every kind's own.
        let all = env(&[("LC_ALL", "fr_FR.UTF-8"), ("LC_PAPER", "en_US.UTF-8")]);
        assert_eq!(of_from(&all, Category::Paper).as_deref(), Some("fr-FR"));
        assert_eq!(of_from(&env(&[("LANG", "C")]), Category::Time), None);
    }

    /// The order of names is `LC_COLLATE`'s (ADR-139, 15), apart from the
    /// messages' language; C or POSIX there keeps names folded, whatever
    /// the rest says.
    #[test]
    fn names_are_ordered_by_lc_collate() {
        let of = |pairs: &[(&str, &str)]| of_from(&env(pairs), Category::Collate);
        let swedish = [("LANG", "en_US.UTF-8"), ("LC_COLLATE", "sv_SE.UTF-8")];
        assert_eq!(of(&swedish).as_deref(), Some("sv-SE"));
        assert_eq!(of(&[("LANG", "de_DE.UTF-8")]).as_deref(), Some("de-DE"));
        assert_eq!(of(&[("LANG", "fr_FR.UTF-8"), ("LC_COLLATE", "C")]), None);
        assert_eq!(
            of(&[("LANG", "fr_FR.UTF-8"), ("LC_COLLATE", "POSIX")]),
            None
        );
        assert_eq!(of(&[("LC_ALL", "C.UTF-8"), ("LC_COLLATE", "sv_SE")]), None);
        assert_eq!(
            of(&[("LANG", "C.UTF-8"), ("LC_COLLATE", "")]),
            None,
            "one set to nothing is not set"
        );
        assert_eq!(of(&[]), None, "none said");
    }
}
