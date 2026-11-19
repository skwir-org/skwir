//! A crate's messages in one language (`l10n/<language>.ftl`), and the
//! registry of every crate's (ADR-139, 4). A catalog is indexed the first
//! time a message of it is asked; a message without arguments is its own
//! text, as it lies there; any other is parsed with `fluent-syntax` when
//! first said, and kept. Measured on 8,000 messages, the index is 190 KiB
//! and 0.8 ms, a parsed message 1.1 KB (`docs/STATUS-LOG.md`, 2026-11-16).
//!
//! What describes a plugin that is off (its title, what it does) is said
//! from catalogs kept apart from the registry, never registered: those
//! shipped for its crate, or those its host keeps (`find_describing`), each
//! read a message at a time, no index made.

use std::borrow::Cow;
use std::collections::HashMap;
use std::ops::Range;
use std::sync::{Arc, OnceLock};

use parking_lot::{Mutex, RwLock};

use super::pattern::{Element, from_fluent};

/// What is said of a message: its parts.
pub type Said = Arc<[Element]>;

/// One language's messages of one crate.
pub struct Catalog {
    language: String,
    source: Cow<'static, str>,
    index: OnceLock<Vec<Entry>>,
    said: Mutex<HashMap<Box<str>, Option<Said>>>,
}

impl std::fmt::Debug for Catalog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Catalog")
            .field("language", &self.language)
            .field("bytes", &self.source.len())
            .finish_non_exhaustive()
    }
}

/// Where a message lies in its catalog's source.
struct Entry {
    id: Range<usize>,
    whole: Range<usize>,
    /// Its value, where it is plain text on one line.
    plain: Option<Range<usize>>,
}

impl Catalog {
    /// The messages `source` holds, in `language` (a BCP 47 tag).
    pub fn new(language: &str, source: impl Into<Cow<'static, str>>) -> Self {
        Catalog {
            language: language.to_owned(),
            source: source.into(),
            index: OnceLock::new(),
            said: Mutex::new(HashMap::new()),
        }
    }

    pub fn language(&self) -> &str {
        &self.language
    }

    /// The parts of message `id`, `None` where this catalog has none, or
    /// one Skwir does not read (a term, a function: the checks refuse
    /// them).
    pub fn message(&self, id: &str) -> Option<Said> {
        if let Some(said) = self.said.lock().get(id) {
            return said.clone();
        }
        let index = self.index.get_or_init(|| index(&self.source));
        let said = self.said_of(first(index, &self.source, id)?);
        self.said.lock().insert(id.into(), said.clone());
        said
    }

    /// The parts of message `id`, as [`Catalog::message`] says them, but
    /// found by reading the source up to it, no index made where none is:
    /// for a catalog asked for a message or two (what describes a plugin
    /// that is off, `find_describing`), whose index would be kept for them.
    fn message_alone(&self, id: &str) -> Option<Said> {
        if let Some(said) = self.said.lock().get(id) {
            return said.clone();
        }
        if self.index.get().is_some() {
            return self.message(id);
        }
        let said = entry_of(&self.source, id).and_then(|entry| self.said_of(&entry));
        self.said.lock().insert(id.into(), said.clone());
        said
    }

    /// What the message at `entry` says: its own text where it is plain,
    /// else its value parsed.
    fn said_of(&self, entry: &Entry) -> Option<Said> {
        match &entry.plain {
            Some(plain) => Some(Arc::from([Element::Text(self.text(plain.clone()))])),
            None => parse(&self.source[entry.whole.clone()]).map(Arc::from),
        }
    }

    /// The parts of attribute `name` of message `id` (a noun's `.gender`,
    /// one of its forms), `None` where the message has none, or is not in
    /// this catalog. Parsed when first asked, and kept as a message is.
    pub fn attribute(&self, id: &str, name: &str) -> Option<Said> {
        // An id holds no dot: "id.name" is no message's.
        let key = format!("{id}.{name}");
        if let Some(said) = self.said.lock().get(key.as_str()) {
            return said.clone();
        }
        let index = self.index.get_or_init(|| index(&self.source));
        let entry = first(index, &self.source, id)?;
        let said: Option<Said> =
            parse_attribute(&self.source[entry.whole.clone()], name).map(Arc::from);
        self.said.lock().insert(key.into(), said.clone());
        said
    }

    /// A slice of the source: borrowed where the source is the program's
    /// own (embedded), else copied.
    fn text(&self, range: Range<usize>) -> Cow<'static, str> {
        match &self.source {
            Cow::Borrowed(source) => Cow::Borrowed(&source[range]),
            Cow::Owned(source) => Cow::Owned(source[range].to_owned()),
        }
    }
}

/// The first message `id` of `source` written, in its `index`: an id
/// written twice (the catalogs' test refuses it in Skwir's own) is said as
/// its first copy, as a catalog read alone says it (`entry_of`) and as
/// Fluent's bundles keep it.
fn first<'a>(index: &'a [Entry], source: &str, id: &str) -> Option<&'a Entry> {
    let at = index.partition_point(|entry| source[entry.id.clone()] < *id);
    index
        .get(at)
        .filter(|entry| source[entry.id.clone()] == *id)
}

/// Where each message of `source` lies, sorted by id, those of one id in
/// the order written.
fn index(source: &str) -> Vec<Entry> {
    let mut entries: Vec<Entry> = Vec::new();
    walk(source, |entry| {
        entries.push(entry);
        false
    });
    entries.sort_by(|a, b| source[a.id.clone()].cmp(&source[b.id.clone()]));
    entries
}

/// Where message `id` lies in `source`, read up to it.
fn entry_of(source: &str, id: &str) -> Option<Entry> {
    let mut found = None;
    walk(source, |entry| {
        let this = source[entry.id.clone()] == *id;
        if this {
            found = Some(entry);
        }
        this
    });
    found
}

/// Each message of `source` where it lies, in the order written, until
/// `each` has what it wants (answers `true`). A message begins at the start
/// of a line with its id and `=`; it goes on over the indented and blank
/// lines after it, those a variant, an attribute or a selector's end begins
/// (`[`, `*`, `.`, `}`: Fluent reads them at a line's start too), and over
/// every line while a placeable it opened is open. It ends, open or not, at
/// a line where Fluent's parser begins anew whatever came before: one that
/// begins a message or a term (an id and `=`), or a comment (`#`). So one
/// message Fluent cannot read is said in English, never the rest of its
/// catalog with it.
fn walk(source: &str, mut each: impl FnMut(Entry) -> bool) {
    let mut at = 0;
    let mut open: Option<Entry> = None;
    let mut braces = Braces::default();
    for line in source.split_inclusive('\n') {
        let start = at;
        at += line.len();
        let continues = open.is_some()
            && !begins_anew(line)
            && (braces.depth > 0
                || line.starts_with([' ', '\t', '[', '*', '.', '}'])
                || line.trim().is_empty());
        if continues {
            braces.read(line);
            if let Some(entry) = open.as_mut() {
                entry.whole.end = at;
                // A value on more lines, or attributes: parsed.
                if !line.trim().is_empty() {
                    entry.plain = None;
                }
            }
            continue;
        }
        if let Some(entry) = open.take()
            && each(entry)
        {
            return;
        }
        braces = Braces::default();
        let Some((id, rest)) = line.split_once('=') else {
            continue;
        };
        let id = id.trim_end();
        let valid = id.starts_with(|c: char| c.is_ascii_alphabetic())
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        if !valid {
            continue;
        }
        let value = rest.trim();
        let plain = (!value.is_empty() && !value.contains(['{', '}'])).then(|| {
            let begins = start + line.len() - rest.len() + (rest.len() - rest.trim_start().len());
            begins..begins + value.len()
        });
        braces.read(rest);
        open = Some(Entry {
            id: start..start + id.len(),
            whole: start..at,
            plain,
        });
    }
    if let Some(entry) = open {
        each(entry);
    }
}

/// Whether Fluent's parser begins anew at `line`, whatever came before: a
/// line that begins a message or a term (an id, `-` before a term's, then
/// `=`), which no placeable can hold, or a comment.
fn begins_anew(line: &str) -> bool {
    if line.starts_with('#') {
        return true;
    }
    let named = line.strip_prefix('-').unwrap_or(line);
    named.starts_with(|c: char| c.is_ascii_alphabetic())
        && named
            .trim_start_matches(|c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            .trim_start_matches([' ', '\t'])
            .starts_with('=')
}

/// How deep in placeables a message's text stands, as its lines are read:
/// braces counted outside a placeable's string literals (`{"{"}`), which
/// end at their line's end, as Fluent's do.
#[derive(Default)]
struct Braces {
    depth: usize,
    in_string: bool,
    escaped: bool,
}

impl Braces {
    fn read(&mut self, text: &str) {
        for c in text.chars() {
            if c == '\n' {
                self.in_string = false;
                self.escaped = false;
                continue;
            }
            if self.in_string {
                match (self.escaped, c) {
                    (true, _) => self.escaped = false,
                    (false, '\\') => self.escaped = true,
                    (false, '"') => self.in_string = false,
                    _ => {}
                }
                continue;
            }
            match c {
                '{' => self.depth += 1,
                '}' => self.depth = self.depth.saturating_sub(1),
                '"' if self.depth > 0 => self.in_string = true,
                _ => {}
            }
        }
    }
}

/// The value of the one message `ftl` holds.
fn parse(ftl: &str) -> Option<Vec<Element>> {
    let resource = fluent_syntax::parser::parse_runtime(ftl).ok()?;
    let fluent_syntax::ast::Entry::Message(message) = resource.body.into_iter().next()? else {
        return None;
    };
    from_fluent(message.value.as_ref()?).ok()
}

/// The value of attribute `name` of the one message `ftl` holds.
fn parse_attribute(ftl: &str, name: &str) -> Option<Vec<Element>> {
    let resource = fluent_syntax::parser::parse_runtime(ftl).ok()?;
    let fluent_syntax::ast::Entry::Message(message) = resource.body.into_iter().next()? else {
        return None;
    };
    let attribute = message
        .attributes
        .into_iter()
        .find(|attribute| attribute.id.name == name)?;
    from_fluent(&attribute.value).ok()
}

/// A catalog, as a plugin or the core registered it.
struct Registered {
    owner: String,
    domain: String,
    catalog: Arc<Catalog>,
}

/// Every crate's catalogs, and what moves as they come and go.
static CATALOGS: RwLock<Registry> = RwLock::new((Vec::new(), 0));

/// Say `domain`'s messages in `catalog`'s language, as `owner`'s: a
/// contribution like its commands, withdrawn on disable (`withdraw`).
pub fn register(owner: &str, domain: &str, catalog: Catalog) {
    let mut catalogs = CATALOGS.write();
    catalogs
        .0
        .retain(|kept| !(kept.domain == domain && kept.catalog.language == catalog.language));
    catalogs.0.push(Registered {
        owner: owner.to_owned(),
        domain: domain.to_owned(),
        catalog: Arc::new(catalog),
    });
    catalogs.1 += 1;
}

/// A catalog Skwir ships: a crate's (domain's) messages in one language,
/// its source compiled into the program (`crates/skwir/src/catalogs.rs`,
/// made by `just words`).
#[derive(Clone, Copy, Debug)]
pub struct Shipped {
    pub domain: &'static str,
    pub language: &'static str,
    pub source: &'static str,
}

/// The catalogs the program ships, handed in as it starts (`ship`).
static SHIPPED: RwLock<&'static [Shipped]> = RwLock::new(&[]);

/// The catalogs the program ships, each registered by its crate's owner:
/// the program's own at once (`register_shipped_of_program`), a plugin's
/// when it is enabled (`register_shipped`).
pub fn ship(catalogs: &'static [Shipped]) {
    *SHIPPED.write() = catalogs;
    // What describes a plugin is read from the catalogs shipped now.
    if let Some(described) = DESCRIBED.lock().as_mut() {
        described.retain(|_, one| !one.shipped);
    }
}

/// The languages the program ships catalogs in, once each, in the order
/// first shipped.
pub fn shipped_languages() -> Vec<&'static str> {
    let mut languages: Vec<&'static str> = Vec::new();
    for one in *SHIPPED.read() {
        if !languages.contains(&one.language) {
            languages.push(one.language);
        }
    }
    languages
}

/// How much of one language Skwir's catalogs say, and whether Skwir offers
/// it (ADR-139, 13). Counted at build time by `just words`
/// (`tools/words/extract.py`, beside the catalogs in
/// `crates/skwir/src/catalogs.rs`), so that nothing of the catalogs is
/// read, nor made resident, to know it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Share {
    /// Its tag, as its catalogs are named (`fr`).
    pub language: &'static str,
    /// All of Skwir's messages: every crate's English.
    pub messages: u32,
    /// How many of them its catalogs translate.
    pub translated: u32,
    /// How many of those a person reviewed: no longer drafts.
    pub reviewed: u32,
    /// Whether Automatic speaks it, and as what (`tools/words/offered`).
    pub offered: Offer,
    /// Its name in itself (Français, 简体中文), as its catalog of core says
    /// it (`settings_language_self`): what the Language setting and About
    /// name it by, read from no catalog, as a start would read every
    /// language's to list them.
    pub name: &'static str,
}

/// Whether Automatic speaks a language (ADR-139, 13), as the record of
/// languages offered says (`tools/words/offered`, which `just words`
/// writes a language into as it reaches each bar).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Offer {
    /// Not spoken by Automatic: listed as incomplete, said only to whoever
    /// chooses it.
    Incomplete,
    /// Spoken by Automatic as a draft: 90 % of all messages and every one
    /// of what is on by default translated, not yet reviewed as much. The
    /// window says so once, and About while it speaks it.
    Draft,
    /// Spoken by Automatic: 90 % of all messages and every one of what is
    /// on by default reviewed by a person who speaks it.
    Reviewed,
}

impl Offer {
    /// Whether Automatic speaks it, as a draft or reviewed.
    pub fn spoken(self) -> bool {
        self != Offer::Incomplete
    }
}

impl Share {
    /// The messages translated, in whole percents of all, rounded down: 100
    /// only once every one is.
    pub fn translated_percent(&self) -> u32 {
        percent(self.translated, self.messages)
    }

    /// The messages reviewed, in whole percents of all, rounded down.
    pub fn reviewed_percent(&self) -> u32 {
        percent(self.reviewed, self.messages)
    }
}

fn percent(part: u32, whole: u32) -> u32 {
    if whole == 0 {
        return 0;
    }
    u32::try_from(u64::from(part.min(whole)) * 100 / u64::from(whole)).unwrap_or(100)
}

/// Each language's share, handed in as the program starts (`ship_shares`).
static SHARES: RwLock<&'static [Share]> = RwLock::new(&[]);

/// How much of each language the catalogs shipped say (`ship`), as
/// `just words` counted it: what the Language setting lists, and which
/// languages Automatic may speak (`locale::automatic`).
pub fn ship_shares(shares: &'static [Share]) {
    *SHARES.write() = shares;
    // What is said in the system's languages may change with it.
    CATALOGS.write().1 += 1;
}

/// The share of `language` (a tag as a catalog is named, any case): `None`
/// for one no catalog shipped is in, English included, the code's own.
pub fn share_of(language: &str) -> Option<Share> {
    SHARES
        .read()
        .iter()
        .find(|share| share.language.eq_ignore_ascii_case(language))
        .copied()
}

/// Whether two names are one crate's: its package name (`skwir-plugin-tags`)
/// and Rust's (`skwir_plugin_tags`).
fn same_crate(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .all(|(x, y)| x == y || (x == b'-' && y == b'_') || (x == b'_' && y == b'-'))
}

/// Register, as the plugin `owner`'s, the catalogs shipped for its crate
/// `domain` (either spelling of its name); a crate that is no plugin's (a
/// plugin from outside answers its host's) keeps the program's.
pub fn register_shipped(owner: &str, domain: &str) {
    let shipped = *SHIPPED.read();
    for one in shipped
        .iter()
        .filter(|one| one.domain.starts_with("skwir-plugin-") && same_crate(one.domain, domain))
    {
        register(owner, one.domain, Catalog::new(one.language, one.source));
    }
}

/// Register, as `owner`'s, the shipped catalogs of every crate that is no
/// plugin's (core, the platform, the window, the host of plugins from
/// outside): a plugin's come and go with it.
pub fn register_shipped_of_program(owner: &str) {
    let shipped = *SHIPPED.read();
    for one in shipped
        .iter()
        .filter(|one| !one.domain.starts_with("skwir-plugin-"))
    {
        register(owner, one.domain, Catalog::new(one.language, one.source));
    }
}

/// Take back every catalog `owner` registered.
pub fn withdraw(owner: &str) {
    let mut catalogs = CATALOGS.write();
    let before = catalogs.0.len();
    catalogs.0.retain(|kept| kept.owner != owner);
    if catalogs.0.len() != before {
        catalogs.1 += 1;
    }
}

/// The catalogs that say what describes a plugin (its title, what it does,
/// what it needs a program for) while none of its domain is registered, the
/// plugin being off: data about the plugin, never a contribution of it, so
/// nothing is registered for a plugin that is off (`plugins_toggle.rs`).
/// Only words describing a plugin are said from them
/// (`Words::describing`); its commands and answers never are.
struct Described {
    catalogs: Vec<Catalog>,
    /// Whether they are the program's, shipped (`ship`), rather than kept
    /// by a host (`describe_with`).
    shipped: bool,
    /// What each message was found as, and for which languages.
    found: HashMap<Box<str>, (Vec<String>, Option<(Said, String)>)>,
}

impl Described {
    /// Message `id` in the first of `languages` its catalogs have, as
    /// `saying` finds one among those registered.
    fn find(&self, id: &str, languages: &[String]) -> Option<(Said, String)> {
        let mut tags: Vec<&str> = self.catalogs.iter().map(Catalog::language).collect();
        tags.sort_unstable();
        tags.dedup();
        for language in languages {
            for candidate in crate::locale::matching(language, tags.iter().copied()) {
                if candidate == super::ENGLISH {
                    return None;
                }
                let said = self
                    .catalogs
                    .iter()
                    .find(|catalog| catalog.language == candidate)
                    .and_then(|catalog| catalog.message_alone(id));
                if let Some(said) = said {
                    return Some((said, candidate.to_owned()));
                }
            }
        }
        None
    }
}

/// What describes each plugin while it is off, by its domain: made from the
/// catalogs shipped for its crate the first time it is asked, or kept by its
/// host (`describe_with`). A catalog there is read a message at a time, no
/// index made (`Catalog::message_alone`): a plugin is described by two or
/// three.
static DESCRIBED: Mutex<Option<HashMap<Box<str>, Described>>> = Mutex::new(None);

/// Keep `catalogs` as what says the words describing the plugin whose
/// messages are `domain`'s (`outside:<id>`) while it is off: what the host
/// of plugins from outside reads of one when it finds it, kept with its list
/// of plugins for as long as it knows the plugin. Never registered: only
/// words describing the plugin are said from them (`Words::describing`),
/// never its commands or answers, which are said only while it is on, from
/// the catalogs registered then (`register`). A plugin built into Skwir
/// needs none: its own are the catalogs shipped for its crate.
pub fn describe_with(domain: &str, catalogs: Vec<Catalog>) {
    DESCRIBED.lock().get_or_insert_with(HashMap::new).insert(
        domain.into(),
        Described {
            catalogs,
            shipped: false,
            found: HashMap::new(),
        },
    );
}

/// Message `id` of `domain` as words describing a plugin find it
/// (`Words::describing`), in the first of `languages` that has it, and the
/// tag of the catalog it is said from; `None` where English comes first.
/// From the catalogs registered for `domain` while any is, the plugin on
/// ([`find`]); else, the plugin off, from those the program ships for its
/// crate, read only for this and registering nothing, else from those its
/// host keeps for it (`describe_with`).
pub(crate) fn find_describing(
    domain: &str,
    id: &str,
    languages: &[String],
) -> Option<(Said, String)> {
    if languages.is_empty() {
        return None;
    }
    {
        let catalogs = CATALOGS.read();
        if catalogs.0.iter().any(|kept| kept.domain == domain) {
            let (_, said, tag) = saying(&catalogs, domain, id, languages)?;
            return Some((said, tag));
        }
    }
    let mut described = DESCRIBED.lock();
    let described = described.get_or_insert_with(HashMap::new);
    if !described.contains_key(domain) {
        let catalogs = SHIPPED
            .read()
            .iter()
            .filter(|one| one.domain == domain)
            .map(|one| Catalog::new(one.language, one.source))
            .collect();
        described.insert(
            domain.into(),
            Described {
                catalogs,
                shipped: true,
                found: HashMap::new(),
            },
        );
    }
    let one = described.get_mut(domain)?;
    if let Some((asked, found)) = one.found.get(id)
        && asked.as_slice() == languages
    {
        return found.clone();
    }
    let found = one.find(id, languages);
    one.found
        .insert(id.into(), (languages.to_vec(), found.clone()));
    found
}

/// Whether a catalog kept to describe the plugins of `domain` while they are
/// off was indexed: never, as each is read a message at a time.
#[cfg(test)]
pub(crate) fn described_indexed(domain: &str) -> Option<bool> {
    let described = DESCRIBED.lock();
    let one = described.as_ref()?.get(domain)?;
    Some(
        one.catalogs
            .iter()
            .any(|catalog| catalog.index.get().is_some()),
    )
}

/// Whether the catalog of `language` registered for `domain` was read:
/// indexed, as it is when its first message is asked.
#[cfg(test)]
pub(crate) fn was_read(domain: &str, language: &str) -> bool {
    CATALOGS.read().0.iter().any(|kept| {
        kept.domain == domain
            && kept.catalog.language == language
            && kept.catalog.index.get().is_some()
    })
}

/// What moves as catalogs come and go.
pub fn revision() -> u64 {
    CATALOGS.read().1
}

/// The languages a catalog is registered in for `domain`, as their tags.
pub fn languages_of(domain: &str) -> Vec<String> {
    CATALOGS
        .read()
        .0
        .iter()
        .filter(|kept| kept.domain == domain)
        .map(|kept| kept.catalog.language.clone())
        .collect()
}

/// Message `id` of `domain` in the first of `languages` that has it, and
/// the tag of the catalog it is said from; `None` where English, the
/// code's, comes first. A reader's language finds the catalogs closest to
/// it (`locale::matching`: `fr-CA` finds `fr`, `zh-TW` finds `zh-Hant`,
/// `sr-Latn` never `sr`), the closest first.
pub fn find(domain: &str, id: &str, languages: &[String]) -> Option<(Said, String)> {
    let catalogs = CATALOGS.read();
    let (_, said, tag) = saying(&catalogs, domain, id, languages)?;
    Some((said, tag))
}

/// Attribute `attribute` of message `id` of `domain` (a noun's `.gender`),
/// as the catalog [`find`] says the message from has it, and that
/// catalog's tag: `Some((None, tag))` where that message has no such
/// attribute, `None` where English says the message.
pub fn find_attribute(
    domain: &str,
    id: &str,
    attribute: &str,
    languages: &[String],
) -> Option<(Option<Said>, String)> {
    let catalogs = CATALOGS.read();
    let (kept, _, tag) = saying(&catalogs, domain, id, languages)?;
    Some((kept.catalog.attribute(id, attribute), tag))
}

/// The catalog message `id` of `domain` is said from in the first of
/// `languages` that has it, the message's parts, and the catalog's tag;
/// `None` where English comes first.
fn saying<'a>(
    catalogs: &'a Registry,
    domain: &str,
    id: &str,
    languages: &[String],
) -> Option<(&'a Registered, Said, String)> {
    for language in languages {
        for candidate in matched(catalogs, language).iter() {
            if candidate.as_ref() == super::ENGLISH {
                return None;
            }
            let found = catalogs
                .0
                .iter()
                .find(|kept| kept.domain == domain && *kept.catalog.language == **candidate);
            if let Some((kept, said)) =
                found.and_then(|kept| Some((kept, kept.catalog.message(id)?)))
            {
                return Some((kept, said, candidate.to_string()));
            }
        }
    }
    None
}

/// The catalogs registered, and what moves as they come and go.
type Registry = (Vec<Registered>, u64);

/// The tags of the catalogs each reader's tag finds, the closest first
/// (`locale::matching`), as read for the catalogs registered at a
/// revision: kept until they change, as every message said asks.
type Matched = (u64, HashMap<Box<str>, Arc<[Box<str>]>>);

static MATCHED: RwLock<Option<Matched>> = RwLock::new(None);

/// The tags of the catalogs `reader` finds among those registered, the
/// closest first, English (`en`) where its closeness puts it for a reader
/// of English.
fn matched(catalogs: &Registry, reader: &str) -> Arc<[Box<str>]> {
    if let Some((revision, by_reader)) = MATCHED.read().as_ref()
        && *revision == catalogs.1
        && let Some(found) = by_reader.get(reader)
    {
        return Arc::clone(found);
    }
    let mut languages: Vec<&str> = catalogs
        .0
        .iter()
        .map(|kept| kept.catalog.language.as_str())
        .collect();
    languages.sort_unstable();
    languages.dedup();
    let found: Arc<[Box<str>]> = crate::locale::matching(reader, languages)
        .into_iter()
        .map(Box::from)
        .collect();
    let mut kept = MATCHED.write();
    let (revision, by_reader) = kept.get_or_insert_with(|| (catalogs.1, HashMap::new()));
    if *revision != catalogs.1 {
        *revision = catalogs.1;
        by_reader.clear();
    }
    by_reader.insert(reader.into(), Arc::clone(&found));
    found
}

#[cfg(test)]
mod tests {
    use super::{Catalog, Element, find, find_attribute, register, withdraw};

    #[test]
    fn a_catalog_is_indexed_once_and_each_message_read_as_asked() {
        let catalog = Catalog::new(
            "fr",
            "# A comment.\nplain = Déplacer dans la corbeille\nmoved = { $count ->\n    [one] Un élément déplacé\n   *[other] { $count } éléments déplacés\n}\n\nbraced = Le { $name } est là\nlast = Fin",
        );
        assert_eq!(
            catalog.message("plain").as_deref(),
            Some(&[Element::Text("Déplacer dans la corbeille".into())][..])
        );
        assert!(matches!(
            catalog.message("moved").as_deref(),
            Some([Element::Select { .. }])
        ));
        assert_eq!(catalog.message("braced").map(|said| said.len()), Some(3));
        assert_eq!(
            catalog.message("last").as_deref(),
            Some(&[Element::Text("Fin".into())][..])
        );
        assert!(catalog.message("missing").is_none());
        // Embedded, a plain message is the source's own text.
        let embedded = Catalog::new("fr", "plain = Un mot\n");
        let said = embedded.message("plain").unwrap();
        assert!(matches!(
            &said[0],
            Element::Text(std::borrow::Cow::Borrowed("Un mot"))
        ));
    }

    /// A reader's tag finds a catalog by its script and region, as CLDR's
    /// likely subtags say them: each message from the closest catalog that
    /// has it, English once the closest is the code's own.
    #[test]
    fn a_reader_finds_the_catalog_of_its_script() {
        const DOMAIN: &str = "skwir-test-catalog-scripts";
        let said = |id: &str, languages: &[&str]| {
            let languages: Vec<String> = languages.iter().map(|l| (*l).to_owned()).collect();
            find(DOMAIN, id, &languages).map(|(said, tag)| {
                let Some(Element::Text(text)) = said.first() else {
                    panic!("a plain message");
                };
                (text.to_string(), tag)
            })
        };
        for (tag, source) in [
            ("zh-Hans", "word = 简\n"),
            ("zh-Hant", "word = 繁\nonly = 只\n"),
            ("zh-HK", "word = 港\n"),
            ("sr", "word = ћ\n"),
            ("pt-BR", "word = br\n"),
            ("pt-PT", "word = pt\nonly = só\n"),
            ("fr", "word = fr\nnoun = dossier\n    .gender = masculine\n"),
            ("en-GB", "word = colour\n"),
        ] {
            register(DOMAIN, DOMAIN, Catalog::new(tag, source));
        }
        let of = |text: &str, tag: &str| Some((text.to_owned(), tag.to_owned()));
        assert_eq!(said("word", &["zh-CN"]), of("简", "zh-Hans"));
        assert_eq!(said("word", &["zh-SG"]), of("简", "zh-Hans"));
        assert_eq!(said("word", &["zh"]), of("简", "zh-Hans"));
        assert_eq!(said("word", &["zh-TW"]), of("繁", "zh-Hant"));
        assert_eq!(said("word", &["zh-MO"]), of("繁", "zh-Hant"));
        assert_eq!(
            said("word", &["zh-HK"]),
            of("港", "zh-HK"),
            "its own region first"
        );
        assert_eq!(
            said("only", &["zh-HK"]),
            of("只", "zh-Hant"),
            "then its script's"
        );
        assert_eq!(
            said("only", &["zh-CN", "en"]),
            None,
            "never another script's"
        );
        // Serbian: Cyrillic is its likely script, Latin another.
        assert_eq!(said("word", &["sr-RS"]), of("ћ", "sr"));
        assert_eq!(said("word", &["sr-Cyrl"]), of("ћ", "sr"));
        assert_eq!(said("word", &["sr-Latn", "en"]), None);
        // Portuguese, as written; another region's only for what its own
        // lacks.
        assert_eq!(said("word", &["pt-BR"]), of("br", "pt-BR"));
        assert_eq!(said("word", &["pt-PT"]), of("pt", "pt-PT"));
        assert_eq!(said("word", &["pt"]), of("br", "pt-BR"), "pt is Brazil's");
        assert_eq!(said("only", &["pt-BR"]), of("só", "pt-PT"));
        // A catalog named by its language alone, for each of its regions.
        assert_eq!(said("word", &["fr-CA"]), of("fr", "fr"));
        assert_eq!(said("word", &["fr-Latn-BE"]), of("fr", "fr"));
        assert_eq!(
            find_attribute(DOMAIN, "noun", "gender", &["fr-CA".to_owned()])
                .map(|(gender, tag)| (gender.is_some(), tag)),
            Some((true, "fr".to_owned()))
        );
        assert_eq!(
            find_attribute(DOMAIN, "word", "gender", &["fr".to_owned()])
                .map(|(gender, tag)| (gender.is_some(), tag)),
            Some((false, "fr".to_owned()))
        );
        // English: a British reader's own catalog first, the code's before
        // another region's.
        assert_eq!(said("word", &["en-GB"]), of("colour", "en-GB"));
        assert_eq!(said("word", &["en-US", "fr"]), None);
        assert_eq!(said("word", &["en", "fr"]), None);
        assert_eq!(said("word", &["de", "fr"]), of("fr", "fr"));
        withdraw(DOMAIN);
        assert_eq!(said("word", &["fr"]), None, "withdrawn");
    }

    /// A message read alone, no index made, is what the index finds: on one
    /// line or several, the last, none.
    #[test]
    fn a_message_read_alone_is_the_one_indexed() {
        let source = "# A comment.\nplain = Déplacer\nmoved = { $count ->\n    [one] Un élément\n   *[other] { $count } éléments\n}\n\nbraced = Le { $name }\n  suite\nlast = Fin";
        let indexed = Catalog::new("fr", source);
        for id in ["plain", "moved", "braced", "last", "missing", "suite"] {
            let alone = Catalog::new("fr", source);
            assert_eq!(alone.message_alone(id), indexed.message(id), "{id}");
            assert!(alone.index.get().is_none(), "{id}: no index made");
            // Asked again, as it was kept.
            assert_eq!(alone.message_alone(id), indexed.message(id), "{id}");
        }
    }

    /// A message Fluent cannot read ends where Fluent begins anew (the next
    /// message, a comment): only it is missing, never the rest.
    #[test]
    fn a_message_left_open_takes_no_other_with_it() {
        let source = "a = { $n\nb = ok\nc = { $n ->\n   *[other] say \"x\n}\nd = { \"{\" } fine\n# draft\ne = { $n\n# A comment.\nf = last\n";
        let catalog = Catalog::new("fr", source);
        for (id, text) in [("b", "ok"), ("f", "last")] {
            assert_eq!(
                catalog.message(id).as_deref(),
                Some(&[Element::Text(text.into())][..]),
                "{id}"
            );
            assert_eq!(
                Catalog::new("fr", source).message_alone(id),
                catalog.message(id),
                "{id} read alone"
            );
        }
        assert_eq!(
            catalog.message("d").as_deref(),
            Some(&[Element::Text("{".into()), Element::Text(" fine".into())][..])
        );
        // What Fluent cannot read is missing, said in English.
        assert!(catalog.message("a").is_none());
        assert!(catalog.message("e").is_none());
        // Variants, attributes and a selector's end at a line's start are the
        // message's.
        let column = Catalog::new(
            "fr",
            "g = { $n ->\n[one] un\n*[other] plus\n}\nh = mot\n.gender = masculine\n",
        );
        assert!(matches!(
            column.message("g").as_deref(),
            Some([Element::Select { .. }])
        ));
        assert_eq!(
            column.attribute("h", "gender").as_deref(),
            Some(&[Element::Text("masculine".into())][..])
        );
    }

    /// An id written twice is said as its first copy, indexed or read
    /// alone, whether the plugin it is of is on or off.
    #[test]
    fn an_id_written_twice_is_its_first() {
        let source = "a = un\nb = deux\na = autre\n    .gender = feminine\nc = { $n ->\n   *[other] trois\n}\nc = quatre\n";
        let indexed = Catalog::new("fr", source);
        for (id, text) in [("a", "un"), ("b", "deux")] {
            let first = Some(vec![Element::Text(text.into())]);
            assert_eq!(indexed.message(id).map(|said| said.to_vec()), first, "{id}");
            assert_eq!(
                Catalog::new("fr", source)
                    .message_alone(id)
                    .map(|said| said.to_vec()),
                first,
                "{id} read alone"
            );
        }
        assert!(indexed.attribute("a", "gender").is_none(), "the first's");
        assert!(matches!(
            indexed.message("c").as_deref(),
            Some([Element::Select { .. }])
        ));
        assert_eq!(
            Catalog::new("fr", source).message_alone("c"),
            indexed.message("c")
        );
    }

    #[test]
    fn an_attribute_is_read_as_asked() {
        let catalog = Catalog::new(
            "fr",
            "folder = dossier\n    .gender = masculine\n    .plural = dossiers\nplain = mot\n",
        );
        assert_eq!(
            catalog.attribute("folder", "gender").as_deref(),
            Some(&[Element::Text("masculine".into())][..])
        );
        assert_eq!(
            catalog.attribute("folder", "plural").as_deref(),
            Some(&[Element::Text("dossiers".into())][..])
        );
        assert!(catalog.attribute("folder", "case").is_none());
        assert!(catalog.attribute("plain", "gender").is_none());
        assert!(catalog.attribute("missing", "gender").is_none());
        assert_eq!(
            catalog.message("folder").as_deref(),
            Some(&[Element::Text("dossier".into())][..])
        );
    }
}
