//! The languages Skwir says its words in (ADR-139, 6): the system's, as
//! `skwir_platform::locale` reads them (handed in as Skwir starts,
//! [`set_system`]), after the one the user chose (`core.language`), if
//! they chose one. English is always the last. Of the system's, only
//! those Skwir offers (ADR-139, 13: `words::Share::offered`), reviewed or
//! as a draft, which the window says ([`draft_spoken`]); one listed as
//! incomplete is said only to whoever chooses it.
//!
//! Process-wide, as sizes' units are (`fs::size_units`): every reader of a
//! language (a desktop entry's name, later every message) asks here.
//!
//! **A reader's tag finds a catalog by what CLDR says of both**
//! ([`matching`]): its language and its script, the one it names or the
//! one the language is most likely written in (ICU4X's likely subtags), so
//! `zh-TW` and `zh-HK` find a `zh-Hant` catalog and `sr-Latn` never a
//! Cyrillic `sr`; then its region.

use std::cell::RefCell;
use std::collections::HashMap;

use icu_decimal::CompactDecimalFormatter;
use icu_decimal::options::CompactDecimalFormatterOptions;
use icu_locale::LocaleExpander;
use icu_locale_core::subtags::{Language, Region, Script, Variants};
use icu_locale_core::{LanguageIdentifier, Locale};
use parking_lot::RwLock;

use crate::words::Offer;

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
    /// How the desktop writes the hours of a time, where it says so apart
    /// from its locale.
    hours: Option<Hours>,
    /// Dates written as ISO 8601 writes them, whatever the locale (the
    /// window's dates setting, `ui.date_format`).
    iso_dates: bool,
}

/// Moves whenever what is said would change, for what keeps what it read
/// of them (a cache of names by language): moved under `LANGUAGES`' lock
/// once what it holds changed, read without one, as every word said
/// reads it.
static REVISION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// What is said moved: `REVISION` moves, after what changed is written.
fn moved() {
    REVISION.fetch_add(1, std::sync::atomic::Ordering::Release);
}

static LANGUAGES: RwLock<Languages> = RwLock::new(Languages {
    system: Vec::new(),
    chosen: None,
    numbers: None,
    dates: None,
    hours: None,
    iso_dates: false,
});

/// How a time's hours are written, where the desktop says so apart from
/// its locale: GNOME's clock format, macOS's and Windows' own switch
/// (`skwir_platform::desktop_style`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Hours {
    /// 2:02 PM.
    Twelve,
    /// 14:02.
    TwentyFour,
}

/// How the desktop writes the hours, `None` where it says nothing but its
/// locale: the dates' locale then writes them its way.
pub fn set_hours(hours: Option<Hours>) {
    let mut kept = LANGUAGES.write();
    if kept.hours != hours {
        kept.hours = hours;
        moved();
    }
}

/// Whether the window writes dates as ISO 8601 writes them (`2026-03-12
/// 14:02`), every reader of a date in it alike, the plugins' included.
pub fn set_iso_dates(iso: bool) {
    let mut kept = LANGUAGES.write();
    if kept.iso_dates != iso {
        kept.iso_dates = iso;
        moved();
    }
}

/// What words say moved, beside the languages and their formats (the size
/// setting's units, `fs::set_size_units`): what keeps words it said keyed
/// by the revision says them again.
pub(crate) fn said_moved() {
    moved();
}

/// Whether the window writes dates as ISO 8601 writes them.
pub fn iso_dates() -> bool {
    LANGUAGES.read().iso_dates
}

/// The system's locales for numbers and for dates (`skwir_platform::locale`,
/// `LC_NUMERIC` and `LC_TIME` on Linux), `None` where it says none: the
/// window writes them as the rest of the desktop does, whatever language
/// its words are in.
pub fn set_formats(numbers: Option<String>, dates: Option<String>) {
    let mut kept = LANGUAGES.write();
    if kept.numbers != numbers || kept.dates != dates {
        kept.numbers = numbers;
        kept.dates = dates;
        moved();
    }
}

/// The locale numbers are written in for the window: the system's, else
/// the first language words are said in.
pub fn numbers() -> String {
    let kept = LANGUAGES.read();
    kept.numbers.clone().unwrap_or_else(|| first(&kept))
}

/// The locale dates and times are written in for the window: the system's,
/// else the first language words are said in; its hours as the desktop
/// writes them where it says (`-u-hc-h23`, BCP 47's hour cycle).
pub fn dates() -> String {
    let kept = LANGUAGES.read();
    let locale = kept.dates.clone().unwrap_or_else(|| first(&kept));
    match kept.hours {
        Some(hours) => with_hours(&locale, hours),
        None => locale,
    }
}

/// `locale` with its hour cycle `hours`, in place of any it named.
fn with_hours(locale: &str, hours: Hours) -> String {
    let cycle = match hours {
        Hours::Twelve => "h12",
        Hours::TwentyFour => "h23",
    };
    // The Unicode extension's keywords, kept but `hc`; the private use
    // (`-x-`) after it, if any, kept too.
    let (main, private) = match locale.find("-x-") {
        Some(at) => locale.split_at(at),
        None => (locale, ""),
    };
    let (base, keywords) = match main.find("-u-") {
        Some(at) => (&main[..at], &main[at + 3..]),
        None => (main, ""),
    };
    let mut kept: Vec<&str> = Vec::new();
    let mut parts = keywords
        .split('-')
        .filter(|part| !part.is_empty())
        .peekable();
    while let Some(part) = parts.next() {
        if part == "hc" {
            // Its value follows, one subtag.
            parts.next();
        } else {
            kept.push(part);
        }
    }
    let keywords = kept.join("-");
    let joined = if keywords.is_empty() {
        String::new()
    } else {
        format!("-{keywords}")
    };
    format!("{base}-u{joined}-hc-{cycle}{private}")
}

/// The first language words are said in, as numbers and dates are written
/// where the system says no locale for them: a pseudo-language passed by
/// (its words are English's, and its tag no locale's: `ar-XB` would write
/// Arabic digits), English where none is left.
fn first(kept: &Languages) -> String {
    kept.chosen
        .iter()
        .chain(&kept.system)
        .find(|language| !is_pseudo(language))
        .cloned()
        .unwrap_or_else(|| ENGLISH.to_owned())
}

/// Whether `tag` is one of Skwir's pseudo-languages (`words::PSEUDO`,
/// `direction::PSEUDO_RTL`): English said so that what is not a message,
/// or is cut, or is laid out wrong, shows. Their words are English's, so
/// is everything a locale decides of them (their script, their catalogs,
/// their formats, their order), and nothing outside Skwir is written in
/// them.
pub fn is_pseudo(tag: &str) -> bool {
    [crate::words::PSEUDO, crate::direction::PSEUDO_RTL]
        .iter()
        .any(|pseudo| tag.eq_ignore_ascii_case(pseudo))
}

/// The system's languages, most wanted first (`skwir_platform::locale`):
/// none for its C locale.
pub fn set_system(languages: Vec<String>) {
    let mut kept = LANGUAGES.write();
    if kept.system != languages {
        kept.system = languages;
        moved();
    }
}

/// The language the user chose, `None` for the system's.
pub fn set_chosen(language: Option<String>) {
    let mut kept = LANGUAGES.write();
    if kept.chosen != language {
        kept.chosen = language;
        moved();
    }
}

/// The languages Skwir says its words in, most wanted first: the one
/// chosen, then those of the system's Skwir offers its words in, reviewed
/// or as a draft (ADR-139, 13: Automatic never picks one listed as
/// incomplete), then English, each once.
pub fn languages() -> Vec<String> {
    let (chosen, system) = {
        let kept = LANGUAGES.read();
        (kept.chosen.clone(), kept.system.clone())
    };
    let system = system
        .into_iter()
        .filter(|language| matches!(has(language), Has::Offered { .. }));
    once_each(chosen.into_iter().chain(system))
}

/// A translation the window speaks that is still a draft (ADR-139, 13).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraftSpoken {
    /// The tag its catalogs are named by (`fr`, `zh-Hans`).
    pub language: String,
    /// Whether Automatic chose it: the user did not.
    pub automatic: bool,
}

/// The translation Skwir's words are said in, where it is still a draft:
/// one Automatic speaks before it is reviewed, or one listed as incomplete
/// that the user chose. `None` where they are said in English, or in a
/// translation a person reviewed.
pub fn draft_spoken() -> Option<DraftSpoken> {
    let automatic = LANGUAGES.read().chosen.is_none();
    let first = languages().into_iter().next()?;
    match has(&first) {
        Has::Offered { tag, draft: true } | Has::Incomplete(tag) => Some(DraftSpoken {
            language: tag,
            automatic,
        }),
        Has::Offered { draft: false, .. } | Has::Nothing => None,
    }
}

/// The languages the user reads, most wanted first: the one chosen, the
/// system's, English, each once, whether Skwir offers its own words in
/// them or not. What the system translates itself, whole where it ships
/// (desktop entries' names, the type database's comments), is read in
/// these, as other programs on the desktop read it. A pseudo-language is
/// none of them ([`is_pseudo`]): nothing outside Skwir is written in it,
/// and `ar-XB` would find Arabic.
pub fn preferred() -> Vec<String> {
    let kept = LANGUAGES.read();
    once_each(
        kept.chosen
            .iter()
            .chain(&kept.system)
            .filter(|language| !is_pseudo(language))
            .cloned(),
    )
}

/// `languages`, then English, each once.
fn once_each(languages: impl Iterator<Item = String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for language in languages.chain([ENGLISH.to_owned()]) {
        if !out.contains(&language) {
            out.push(language);
        }
    }
    out
}

/// What Skwir has of a language a system names.
enum Has {
    /// Its words, offered: under the tag its catalogs are named by (`fr`
    /// for `fr-FR`), English the code's own; `draft` while they are not
    /// yet reviewed.
    Offered { tag: String, draft: bool },
    /// Words that are not offered, under the tag their catalogs are named
    /// by: listed as incomplete, said only to whoever chooses them.
    Incomplete(String),
    /// No words of its own.
    Nothing,
}

/// What Skwir has of `language`: the catalog closest to it ([`matching`]:
/// `fr-FR` finding `fr`, `zh-TW` finding `zh-Hant`), English for any
/// English with no catalog of its own closer.
fn has(language: &str) -> Has {
    let shipped = crate::words::shipped_languages();
    match matching(language, shipped.iter().copied()).first() {
        None => Has::Nothing,
        Some(&ENGLISH) => Has::Offered {
            tag: ENGLISH.to_owned(),
            draft: false,
        },
        Some(&found) => match crate::words::share_of(found).map(|share| share.offered) {
            Some(Offer::Reviewed) => Has::Offered {
                tag: found.to_owned(),
                draft: false,
            },
            Some(Offer::Draft) => Has::Offered {
                tag: found.to_owned(),
                draft: true,
            },
            // Shipped uncounted (no share), it is offered no more than one
            // counted short.
            Some(Offer::Incomplete) | None => Has::Incomplete(found.to_owned()),
        },
    }
}

thread_local! {
    /// CLDR's likely subtags of the languages it covers well (`zh-TW` is
    /// written in Traditional Han, `sr` in Cyrillic, `pt` is Brazil's):
    /// compiled in, beside the fallbacker's that ICU4X's formatters hold.
    /// On each thread, as ICU4X's data handles are not shared.
    static LIKELY: LocaleExpander = const { LocaleExpander::new_common() };

    /// The short (compact) decimal formatters read on this thread, by
    /// locale: made the first time a locale's is asked.
    static COMPACT: RefCell<HashMap<Box<str>, CompactDecimalFormatter>> =
        RefCell::new(HashMap::new());
}

/// What a tag says, as a reader's and a catalog's are matched: its
/// language and its script (the one it names, else the one CLDR says the
/// language is most likely written in), its region as written and as
/// likely, its variants.
#[derive(Debug)]
struct Named {
    language: Language,
    script: Option<Script>,
    region: Option<Region>,
    likely_region: Option<Region>,
    variants: Variants,
}

impl Named {
    /// `None` for a tag that is no BCP 47 tag. A pseudo-language is read as
    /// English, which its words are ([`is_pseudo`]).
    fn of(tag: &str) -> Option<Named> {
        let tag = if is_pseudo(tag) { ENGLISH } else { tag };
        let written = tag.parse::<Locale>().ok()?.id;
        let mut likely = written.clone();
        LIKELY.with(|expander| expander.maximize(&mut likely));
        Some(Named {
            language: likely.language,
            script: likely.script,
            region: written.region,
            likely_region: likely.region,
            variants: written.variants,
        })
    }

    /// How close a catalog named `self` is to a `reader`, the closest 0:
    /// `None` where it says none of their words (another language, another
    /// script, another variant).
    fn closeness(&self, reader: &Named) -> Option<u8> {
        if self.language != reader.language || self.script != reader.script {
            return None;
        }
        // A catalog of a variant (Valencian) speaks to its readers alone;
        // one of none to all, after those of theirs.
        let variant = if self.variants == reader.variants {
            0
        } else if self.variants.is_empty() {
            1
        } else {
            return None;
        };
        let region = match self.region {
            Some(region) if Some(region) == reader.likely_region => 0,
            None => 1,
            Some(_) => 2,
        };
        Some(variant * 3 + region)
    }
}

/// Whether two tags name one language written in one script, as CLDR's
/// likely subtags say (`fr` and `fr-CA`, `zh-Hant` and `zh-HK`; not `sr`
/// and `sr-Latn`, nor `zh` and `zh-TW`): one's grammar (a noun's gender)
/// holds for the other's words. Tags that are no BCP 47 tags, alike as
/// written.
pub fn same_writing(a: &str, b: &str) -> bool {
    match (Named::of(a), Named::of(b)) {
        (Some(a), Some(b)) => a.language == b.language && a.script == b.script,
        _ => a.eq_ignore_ascii_case(b),
    }
}

/// The script `language` (a BCP 47 tag) is written in: the one it names
/// (`sr-Latn`, `az-Arab`), else the one CLDR says the language is most
/// likely written in (ICU4X's likely subtags: `ja` Jpan, `zh-TW` Hant,
/// `zh-CN` Hans, `sr` Cyrl, `ar` Arab). A pseudo-language's is English's,
/// Latin ([`is_pseudo`]). `None` for a tag that is no BCP 47 tag, or one
/// CLDR knows no script of.
pub fn script_of(language: &str) -> Option<Script> {
    Named::of(language)?.script
}

/// The language CLDR says most likely writes `script` (Russian Cyrillic,
/// English Latin, Japanese Hiragana), with the script: `None` where it
/// says none.
pub(crate) fn writer_of(script: Script) -> Option<LanguageIdentifier> {
    let mut id = LanguageIdentifier::UNKNOWN;
    id.script = Some(script);
    LIKELY.with(|expander| expander.maximize(&mut id));
    (!id.language.is_unknown()).then_some(id)
}

/// Of the catalogs' tags `among`, those whose words a reader of `reader`
/// reads, the closest first, each once:
/// - a tag as written (any case);
/// - then those of its language written in its script, as CLDR's likely
///   subtags say of both, a catalog named by its language alone being
///   written in the script the language most likely is (`zh` is
///   Simplified Chinese, `sr` Cyrillic Serbian): `zh-TW`, `zh-HK` and
///   `zh-MO` find `zh-Hant`, `zh-CN` and `zh-SG` find `zh-Hans` or `zh`,
///   `sr-Latn` never `sr`;
/// - among those, one of the reader's region first (`zh-Hant` finds
///   `zh-TW`), then one that names none (`fr-CA` finds `fr`), then one of
///   another region (`pt-BR` finds `pt-PT` only after `pt-BR` and `pt`);
///   ties in the order of `among`.
///
/// English, the code's own words (named `en`, by its language alone), is
/// among them for a reader of English, where its closeness puts it: a
/// catalog of `en-GB` comes first for a British reader. A pseudo-language
/// finds what English finds (`ar-XB` never an `ar` catalog). A tag that is
/// no BCP 47 tag is matched as written, then by fewer subtags.
pub fn matching<'a>(reader: &str, among: impl IntoIterator<Item = &'a str>) -> Vec<&'a str> {
    let wanted = Named::of(reader);
    let mut ranked: Vec<(u8, &'a str)> = among
        .into_iter()
        .chain([ENGLISH])
        .filter_map(|tag| Some((rank(reader, wanted.as_ref(), tag)?, tag)))
        .collect();
    // Stable: ties keep `among`'s order.
    ranked.sort_by_key(|(closeness, _)| *closeness);
    let mut seen = std::collections::HashSet::new();
    ranked
        .into_iter()
        .filter(|(_, tag)| seen.insert(*tag))
        .map(|(_, tag)| tag)
        .collect()
}

/// How close the catalog `tag` is to a reader of `reader` (`wanted`, as
/// read), 0 the closest, as [`matching`] ranks them: `None` where it says
/// none of their words.
fn rank(reader: &str, wanted: Option<&Named>, tag: &str) -> Option<u8> {
    match wanted {
        Some(wanted) => {
            if tag.eq_ignore_ascii_case(reader) {
                return Some(0);
            }
            Named::of(tag)?
                .closeness(wanted)
                .map(|closeness| closeness + 1)
        }
        // No BCP 47 tag: as written, then by fewer subtags.
        None => std::iter::successors(Some(reader), |shorter| {
            shorter.rsplit_once('-').map(|(shorter, _)| shorter)
        })
        .position(|shorter| shorter.eq_ignore_ascii_case(tag))
        .map(|at| u8::try_from(at).unwrap_or(u8::MAX)),
    }
}

/// A reader's languages, most wanted first, as the translations a file
/// holds are chosen for them: a desktop entry's `Name[…]`, the type
/// database's comments, which name the same few dozen languages over and
/// over. Each language is read once ([`Ranker::rank`]); what is chosen is
/// what [`matching`] would find for the first of the languages that has one
/// (`zh-HK` finding `zh-TW`, `sr-Latn` never `sr`, a pseudo-language
/// nothing but English).
#[derive(Debug, Default)]
pub struct Ranker {
    readers: Vec<(String, Option<Named>)>,
    read: parking_lot::Mutex<HashMap<Box<str>, Option<(usize, u8)>>>,
}

impl Ranker {
    /// For `languages` (as [`preferred`] lists them).
    pub fn new(languages: &[String]) -> Ranker {
        Ranker {
            readers: languages
                .iter()
                .map(|language| (language.clone(), Named::of(language)))
                .collect(),
            read: parking_lot::Mutex::default(),
        }
    }

    /// How close the translation in `tag` (a BCP 47 tag) is: the first of
    /// the languages it serves, and how closely ([`matching`]'s rank), the
    /// lower the closer; `None` where it serves none of them.
    pub fn rank(&self, tag: &str) -> Option<(usize, u8)> {
        if let Some(known) = self.read.lock().get(tag) {
            return *known;
        }
        let ranked = self
            .readers
            .iter()
            .enumerate()
            .find_map(|(at, (reader, wanted))| Some((at, rank(reader, wanted.as_ref(), tag)?)));
        self.read.lock().insert(tag.into(), ranked);
        ranked
    }

    /// Of the translations `tags` (each with its BCP 47 tag), the closest,
    /// the first of those as close; `None` where the untranslated text
    /// (English, `en`) is closer, or nothing serves the languages. As
    /// [`matching`] ranks them, a translation as close as English comes
    /// first (`Name[en]` for a reader of `en`).
    pub fn closest<T>(&self, tags: impl IntoIterator<Item = (T, impl AsRef<str>)>) -> Option<T> {
        let english = self.rank(ENGLISH);
        let (best, item) = tags
            .into_iter()
            .filter_map(|(item, tag)| Some((self.rank(tag.as_ref())?, item)))
            .min_by_key(|(rank, _)| *rank)?;
        match english {
            Some(english) if english < best => None,
            _ => Some(item),
        }
    }
}

/// The tag of the catalog `words` are said from for the window
/// (`Words::say`): the first of [`languages`] whose catalog has their
/// message, English (`en`) where the code's English says them, or where
/// they are no message. What a language decides beside the words (the
/// powers of ten a short count is in, [`compact_power`]) is asked of it,
/// so that it is the language of the words that say it, whole.
pub fn said_in(words: &crate::words::Words) -> String {
    words
        .message_of()
        .and_then(|message| {
            crate::words::find(
                message.domain,
                message.id,
                &crate::words::window_languages(),
            )
        })
        .map_or_else(|| ENGLISH.to_owned(), |(_, tag)| tag)
}

/// The power of ten `locale` counts `n` in where a count is said short
/// (CLDR's compact decimal patterns, short): 3 for 12,345 in English
/// (12K) and in French (12 k), 4 in Chinese and Japanese (1.2万), 5 for
/// lakhs in Hindi, 0 where the language writes the count whole (12.345 in
/// German, which shortens only millions). A tag that is none, or a
/// language CLDR has no compact decimals for, counts as CLDR's root does
/// (thousands, millions); a pseudo-language as English.
pub fn compact_power(locale: &str, n: u64) -> u8 {
    let Some(magnitude) = n.checked_ilog10() else {
        return 0;
    };
    let magnitude = i16::try_from(magnitude).unwrap_or(i16::MAX);
    let locale = if is_pseudo(locale) { ENGLISH } else { locale };
    COMPACT.with_borrow_mut(|made| {
        if let Some(formatter) = made.get(locale) {
            return formatter.compact_exponent_for_magnitude(magnitude);
        }
        let parsed: Locale = locale.parse().unwrap_or(Locale::UNKNOWN);
        let Ok(formatter) = CompactDecimalFormatter::try_new_short(
            (&parsed).into(),
            CompactDecimalFormatterOptions::default(),
        ) else {
            // No data, not even the root's (compiled in): the count whole.
            return 0;
        };
        let power = formatter.compact_exponent_for_magnitude(magnitude);
        made.insert(locale.into(), formatter);
        power
    })
}

/// The language Automatic speaks: the first of the system's languages
/// Skwir offers, reviewed or as a draft (`fr-FR` finding `fr`, `en-GB`
/// English), else English. A language listed as incomplete is never it
/// (ADR-139, 13).
pub fn automatic() -> String {
    let system = LANGUAGES.read().system.clone();
    system
        .iter()
        .find_map(|language| match has(language) {
            Has::Offered { tag, .. } => Some(tag),
            Has::Incomplete(_) | Has::Nothing => None,
        })
        .unwrap_or_else(|| ENGLISH.to_owned())
}

/// What moves whenever [`languages`] would answer otherwise.
pub fn revision() -> u64 {
    REVISION.load(std::sync::atomic::Ordering::Acquire)
}

/// The language the setting's value `value` chooses: `None` for
/// Automatic, or a value that names no language.
pub fn chosen_of(value: &str) -> Option<String> {
    (value != AUTOMATIC && !value.is_empty()).then(|| value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::{
        AUTOMATIC, DraftSpoken, automatic, chosen_of, compact_power, draft_spoken, languages,
        matching, preferred, set_chosen, set_system,
    };
    use crate::words::{Offer, Share, Shipped};

    // French drafted and not offered, German reviewed and offered, Spanish
    // shipped uncounted, Traditional Chinese offered.
    static SHIPPED: [Shipped; 4] = [
        Shipped {
            domain: "skwir-sample",
            language: "fr",
            source: "",
        },
        Shipped {
            domain: "skwir-sample",
            language: "de",
            source: "",
        },
        Shipped {
            domain: "skwir-sample",
            language: "es",
            source: "",
        },
        Shipped {
            domain: "skwir-sample",
            language: "zh-Hant",
            source: "",
        },
    ];
    static SHARES: [Share; 3] = [
        Share {
            language: "fr",
            messages: 100,
            translated: 100,
            reviewed: 0,
            offered: Offer::Incomplete,
            name: "Français",
        },
        Share {
            language: "de",
            messages: 100,
            translated: 95,
            reviewed: 91,
            offered: Offer::Reviewed,
            name: "Deutsch",
        },
        Share {
            language: "zh-Hant",
            messages: 100,
            translated: 100,
            reviewed: 100,
            offered: Offer::Reviewed,
            name: "繁體中文",
        },
    ];
    /// French once translated: a draft; German as before.
    static DRAFTED: [Share; 2] = [
        Share {
            language: "fr",
            messages: 100,
            translated: 100,
            reviewed: 10,
            offered: Offer::Draft,
            name: "Français",
        },
        Share {
            language: "de",
            messages: 100,
            translated: 95,
            reviewed: 91,
            offered: Offer::Reviewed,
            name: "Deutsch",
        },
    ];
    /// French once reviewed.
    static OFFERED: [Share; 1] = [Share {
        language: "fr",
        messages: 100,
        translated: 100,
        reviewed: 90,
        offered: Offer::Reviewed,
        name: "Français",
    }];

    /// The languages, as each reader asks for them: the one chosen first,
    /// the system's, English last, each once; Skwir's words only in those
    /// it offers, and Automatic picking only one of them (ADR-139, 13). One
    /// test, as they are kept for the whole process.
    #[test]
    fn the_one_chosen_comes_first_then_what_skwir_offers_and_english_last() {
        assert_eq!(chosen_of(AUTOMATIC), None);
        assert_eq!(chosen_of("fr").as_deref(), Some("fr"));
        let before = super::revision();
        // Nothing shipped: the system's languages are the user's, Skwir's
        // words English.
        set_system(vec!["fr-FR".into(), "en-GB".into()]);
        assert_eq!(preferred(), ["fr-FR", "en-GB", "en"]);
        assert_eq!(languages(), ["en-GB", "en"]);
        assert_eq!(automatic(), "en");
        // One chosen comes first, whatever Skwir has of it.
        set_chosen(Some("de".into()));
        assert_eq!(preferred(), ["de", "fr-FR", "en-GB", "en"]);
        assert_eq!(languages(), ["de", "en-GB", "en"]);
        set_chosen(Some("en".into()));
        assert_eq!(preferred(), ["en", "fr-FR", "en-GB"]);
        assert_eq!(languages(), ["en", "en-GB"]);
        assert!(super::revision() > before);
        let now = super::revision();
        set_chosen(Some("en".into()));
        assert_eq!(super::revision(), now, "nothing changed");
        set_chosen(None);

        crate::words::ship(&SHIPPED);
        crate::words::ship_shares(&SHARES);
        // Automatic passes French by: German, by a shorter tag.
        set_system(vec!["fr-FR".into(), "it".into(), "de-AT".into()]);
        assert_eq!(automatic(), "de");
        assert_eq!(languages(), ["de-AT", "en"]);
        assert_eq!(preferred(), ["fr-FR", "it", "de-AT", "en"]);
        // English is always offered, any English.
        set_system(vec!["en-GB".into(), "de".into()]);
        assert_eq!(automatic(), "en");
        // Chinese by its script: Hong Kong's finds Traditional, China's
        // none.
        set_system(vec!["zh-CN".into(), "zh-HK".into()]);
        assert_eq!(automatic(), "zh-Hant");
        assert_eq!(languages(), ["zh-HK", "en"]);
        // Nothing offered: English.
        set_system(vec!["fr-FR".into(), "es-ES".into(), "it".into()]);
        assert_eq!(automatic(), "en");
        assert_eq!(languages(), ["en"]);
        // French is said to whoever chooses it; the system's language it
        // stands for stays out.
        set_system(vec!["fr-FR".into(), "de-AT".into()]);
        set_chosen(Some("fr".into()));
        assert_eq!(languages(), ["fr", "de-AT", "en"]);
        assert_eq!(automatic(), "de", "Automatic is not what is chosen");
        // Chosen while incomplete, it is a draft the user chose.
        let drafted = |automatic| {
            Some(DraftSpoken {
                language: "fr".into(),
                automatic,
            })
        };
        assert_eq!(draft_spoken(), drafted(false));
        // Translated, French is Automatic's as a draft, and says so.
        crate::words::ship_shares(&DRAFTED);
        set_chosen(None);
        assert_eq!(automatic(), "fr");
        assert_eq!(languages(), ["fr-FR", "de-AT", "en"]);
        assert_eq!(draft_spoken(), drafted(true));
        // German, reviewed, is no draft; nor is English.
        set_system(vec!["de-AT".into(), "fr-FR".into()]);
        assert_eq!(draft_spoken(), None);
        set_system(vec!["en-GB".into(), "fr-FR".into()]);
        assert_eq!(draft_spoken(), None);
        // Once offered, French is Automatic's, a draft no more.
        set_system(vec!["fr-FR".into(), "de-AT".into()]);
        crate::words::ship_shares(&OFFERED);
        assert_eq!(automatic(), "fr");
        assert_eq!(languages(), ["fr-FR", "en"]);
        assert_eq!(draft_spoken(), None);

        // A pseudo-language chosen: its words first, but nothing the
        // system translates, and no locale of numbers or dates.
        set_system(vec!["fr-FR".into()]);
        for pseudo in [crate::direction::PSEUDO_RTL, crate::words::PSEUDO] {
            set_chosen(Some(pseudo.into()));
            assert_eq!(languages(), [pseudo, "fr-FR", "en"]);
            assert_eq!(preferred(), ["fr-FR", "en"], "{pseudo}");
            assert_eq!(super::numbers(), "fr-FR");
            assert_eq!(super::dates(), "fr-FR");
        }
        set_system(Vec::new());
        assert_eq!(preferred(), ["en"]);
        assert_eq!(super::numbers(), "en", "English, not ar-XB's Arabic digits");
        set_chosen(None);

        crate::words::ship_shares(&[]);
        crate::words::ship(&[]);
        set_system(Vec::new());
        assert_eq!(languages(), ["en"]);
        assert_eq!(automatic(), "en");
    }

    /// A pseudo-language is English to all a locale decides: it finds no
    /// catalog English would not (`ar-XB` no Arabic one), and is written
    /// in Latin.
    #[test]
    fn a_pseudo_language_is_english_to_the_locale() {
        use super::{is_pseudo, same_writing, script_of};
        use crate::direction::PSEUDO_RTL;
        use crate::words::PSEUDO;
        assert!(is_pseudo(PSEUDO_RTL) && is_pseudo("AR-xb") && is_pseudo(PSEUDO));
        assert!(!is_pseudo("ar") && !is_pseudo("en"));
        for pseudo in [PSEUDO_RTL, PSEUDO] {
            assert_eq!(matching(pseudo, ["ar", "ar-EG", "en-GB"]), ["en", "en-GB"]);
            assert_eq!(
                script_of(pseudo).map(|s| s.to_string()).as_deref(),
                Some("Latn")
            );
            assert!(!same_writing(pseudo, "ar"));
        }
    }

    /// A file's translations ranked for the reader's languages, each
    /// language read once: what `matching` finds for the first that has
    /// one, the untranslated English otherwise.
    #[test]
    fn a_ranker_chooses_as_matching_finds() {
        use super::Ranker;
        let langs = |l: &[&str]| l.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        let tags = [
            "zh-CN", "zh-TW", "sr", "sr-Latn", "pt-BR", "fr", "en-GB", "ar",
        ];
        let chosen = |languages: &[&str]| {
            Ranker::new(&langs(languages)).closest(tags.iter().map(|tag| (*tag, *tag)))
        };
        assert_eq!(chosen(&["zh-HK", "en"]), Some("zh-TW"));
        assert_eq!(chosen(&["zh-SG", "en"]), Some("zh-CN"));
        assert_eq!(chosen(&["sr-Latn-RS", "en"]), Some("sr-Latn"));
        assert_eq!(chosen(&["sr-RS", "en"]), Some("sr"));
        assert_eq!(
            chosen(&["pt-PT", "en"]),
            Some("pt-BR"),
            "another region's, before English"
        );
        assert_eq!(chosen(&["de", "fr-CA", "en"]), Some("fr"));
        assert_eq!(chosen(&["en-GB", "fr"]), Some("en-GB"));
        assert_eq!(chosen(&["en", "fr"]), None, "English first: untranslated");
        assert_eq!(chosen(&["de", "en", "fr"]), None, "nothing before English");
        assert_eq!(
            chosen(&["ar-XB", "en"]),
            None,
            "a pseudo-language is English"
        );
        assert_eq!(chosen(&["de"]), None);
        assert_eq!(Ranker::new(&[]).closest([("fr", "fr")]), None);
        // Each language read once, then remembered.
        let ranker = Ranker::new(&langs(&["fr-CA", "en"]));
        assert_eq!(ranker.rank("fr"), Some((0, 2)));
        assert_eq!(ranker.rank("fr"), Some((0, 2)));
        assert_eq!(ranker.rank("fr-CA"), Some((0, 0)));
        assert_eq!(ranker.rank("en"), Some((1, 0)));
        assert_eq!(ranker.rank("de"), None);
    }

    /// The script a tag names, else the one CLDR says its language is most
    /// likely written in.
    #[test]
    fn a_tag_s_script_is_named_or_likely() {
        use super::script_of;
        let of = |tag: &str| script_of(tag).map(|script| script.to_string());
        for (tag, script) in [
            ("ja", "Jpan"),
            ("ja-JP", "Jpan"),
            ("zh-TW", "Hant"),
            ("zh-HK", "Hant"),
            ("zh-CN", "Hans"),
            ("zh", "Hans"),
            ("sr", "Cyrl"),
            ("sr-Latn", "Latn"),
            ("sr-ME", "Latn"),
            ("ar", "Arab"),
            ("fa-IR", "Arab"),
            ("he", "Hebr"),
            ("ko", "Kore"),
            ("hi", "Deva"),
            ("fr", "Latn"),
            ("az-Arab", "Arab"),
        ] {
            assert_eq!(of(tag).as_deref(), Some(script), "{tag}");
        }
        assert_eq!(of("not a tag"), None);
    }

    /// A reader's tag finds the catalogs of its language written in its
    /// script, as CLDR's likely subtags say both are written, its region's
    /// first, then those naming none, then another region's.
    #[test]
    fn a_tag_finds_the_catalogs_of_its_script() {
        let among = [
            "zh",
            "zh-Hans",
            "zh-Hant",
            "zh-TW",
            "sr",
            "sr-Latn",
            "pt",
            "pt-PT",
            "fr",
            "fr-CA",
            "en-GB",
            "ca-valencia",
        ];
        let of = |reader: &str| matching(reader, among);
        assert_eq!(of("zh-CN"), ["zh", "zh-Hans"]);
        assert_eq!(of("zh-SG"), ["zh", "zh-Hans"]);
        assert_eq!(of("zh-Hans-CN"), ["zh", "zh-Hans"]);
        assert_eq!(of("zh-TW"), ["zh-TW", "zh-Hant"]);
        assert_eq!(of("zh-HK"), ["zh-Hant", "zh-TW"]);
        assert_eq!(of("zh-MO"), ["zh-Hant", "zh-TW"]);
        assert_eq!(
            of("zh-Hant"),
            ["zh-Hant", "zh-TW"],
            "as written, then TW, Hant's own"
        );
        assert_eq!(of("sr"), ["sr"]);
        assert_eq!(of("sr-Cyrl-RS"), ["sr"]);
        assert_eq!(of("sr-Latn"), ["sr-Latn"]);
        assert_eq!(of("sr-Latn-ME"), ["sr-Latn"]);
        assert_eq!(of("sr-ME"), ["sr-Latn"], "Montenegro's Serbian is Latin");
        assert_eq!(of("pt-BR"), ["pt", "pt-PT"], "pt is Brazil's");
        assert_eq!(of("pt-PT"), ["pt-PT", "pt"]);
        assert_eq!(of("pt-AO"), ["pt", "pt-PT"]);
        assert_eq!(of("fr-CA"), ["fr-CA", "fr"]);
        assert_eq!(of("FR_be".replace('_', "-").as_str()), ["fr", "fr-CA"]);
        assert_eq!(of("en-GB"), ["en-GB", "en"]);
        assert_eq!(
            of("en-US"),
            ["en", "en-GB"],
            "the code's English before Britain's"
        );
        assert_eq!(of("en"), ["en", "en-GB"]);
        assert_eq!(of("ca-ES-valencia"), ["ca-valencia"]);
        assert_eq!(
            of("ca-ES"),
            Vec::<&str>::new(),
            "a variant speaks to its own"
        );
        assert_eq!(of("de-DE"), Vec::<&str>::new());
        // No BCP 47 tag: as written, then by fewer subtags.
        assert_eq!(matching("x_y-fr", ["x_y-fr", "x_y"]), ["x_y-fr", "x_y"]);
    }

    #[test]
    fn one_language_is_written_one_way() {
        use super::same_writing;
        assert!(same_writing("fr", "fr-CA"));
        assert!(same_writing("zh-Hant", "zh-HK"));
        assert!(same_writing("zh-TW", "zh-MO"));
        assert!(same_writing("zh", "zh-SG"));
        assert!(!same_writing("zh", "zh-TW"));
        assert!(!same_writing("sr", "sr-Latn"));
        assert!(!same_writing("fr", "de"));
        assert!(same_writing("x_y", "X_Y"));
    }

    /// The power of ten a count is said short in, as CLDR's compact
    /// decimals say: thousands and millions, myriads in Chinese and
    /// Japanese, lakhs and crores in Hindi, nothing below millions in
    /// German.
    #[test]
    fn a_short_count_is_counted_in_the_language_s_powers() {
        let powers = |locale: &str| {
            [
                999,
                1_234,
                12_345,
                123_456,
                1_234_567,
                123_456_789,
                2_345_678_901,
            ]
            .map(|n| compact_power(locale, n))
        };
        assert_eq!(powers("en"), [0, 3, 3, 3, 6, 6, 9]);
        assert_eq!(powers("fr-FR"), [0, 3, 3, 3, 6, 6, 9]);
        assert_eq!(powers("ja"), [0, 0, 4, 4, 4, 8, 8]);
        assert_eq!(powers("zh-Hant-TW"), [0, 0, 4, 4, 4, 8, 8]);
        assert_eq!(powers("ko"), [0, 3, 4, 4, 4, 8, 8]);
        assert_eq!(powers("hi"), [0, 3, 3, 5, 5, 7, 9]);
        assert_eq!(powers("de"), [0, 0, 0, 0, 6, 6, 9]);
        assert_eq!(compact_power("en", 0), 0);
        assert_eq!(compact_power("not a tag", 12_345), 3, "the root's");
    }
}
