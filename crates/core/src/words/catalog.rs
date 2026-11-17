//! A crate's messages in one language (`l10n/<language>.ftl`), and the
//! registry of every crate's (ADR-139, 4). A catalog is indexed the first
//! time a message of it is asked; a message without arguments is its own
//! text, as it lies there; any other is parsed with `fluent-syntax` when
//! first said, and kept. Measured on 8,000 messages, the index is 190 KiB
//! and 0.8 ms, a parsed message 1.1 KB (`docs/STATUS-LOG.md`, 2026-11-16).

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
        let at = index
            .binary_search_by(|entry| self.source[entry.id.clone()].cmp(id))
            .ok()?;
        let entry = &index[at];
        let said: Option<Said> = match &entry.plain {
            Some(plain) => Some(Arc::from([Element::Text(self.text(plain.clone()))])),
            None => parse(&self.source[entry.whole.clone()]).map(Arc::from),
        };
        self.said.lock().insert(id.into(), said.clone());
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

/// Where each message of `source` lies, sorted by id. A message begins at
/// the start of a line with its id and `=`; it goes on over the indented
/// and blank lines after it, and over every line while a placeable it
/// opened is open (a selector's `}` may stand at a line's start).
fn index(source: &str) -> Vec<Entry> {
    let mut entries: Vec<Entry> = Vec::new();
    let mut at = 0;
    let mut open: Option<Entry> = None;
    let mut braces = Braces::default();
    for line in source.split_inclusive('\n') {
        let start = at;
        at += line.len();
        let continues = open.is_some()
            && (braces.depth > 0 || line.starts_with([' ', '\t']) || line.trim().is_empty());
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
        entries.extend(open.take());
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
    entries.extend(open);
    entries.sort_by(|a, b| source[a.id.clone()].cmp(&source[b.id.clone()]));
    entries
}

/// How deep in placeables a message's text stands, as its lines are read:
/// braces counted outside a placeable's string literals (`{"{"}`).
#[derive(Default)]
struct Braces {
    depth: usize,
    in_string: bool,
    escaped: bool,
}

impl Braces {
    fn read(&mut self, text: &str) {
        for c in text.chars() {
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

/// A catalog, as a plugin or the core registered it.
struct Registered {
    owner: String,
    domain: String,
    catalog: Arc<Catalog>,
}

/// Every crate's catalogs, and what moves as they come and go.
static CATALOGS: RwLock<(Vec<Registered>, u64)> = RwLock::new((Vec::new(), 0));

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
/// that language; `None` where English, the code's, comes first. A
/// language is looked for as it is, then by fewer subtags (`fr-CA`, then
/// `fr`).
pub fn find(domain: &str, id: &str, languages: &[String]) -> Option<(Said, String)> {
    let catalogs = CATALOGS.read();
    for language in languages {
        for candidate in fallbacks(language) {
            if candidate == super::ENGLISH {
                return None;
            }
            let found = catalogs
                .0
                .iter()
                .find(|kept| kept.domain == domain && kept.catalog.language == candidate);
            if let Some(said) = found.and_then(|kept| kept.catalog.message(id)) {
                return Some((said, candidate.to_owned()));
            }
        }
    }
    None
}

/// `fr-CA`, then `fr`: a tag, then shorter by a subtag at a time.
fn fallbacks(tag: &str) -> impl Iterator<Item = &str> {
    let mut next = Some(tag);
    std::iter::from_fn(move || {
        let tag = next?;
        next = tag.rsplit_once('-').map(|(shorter, _)| shorter);
        Some(tag)
    })
}

#[cfg(test)]
mod tests {
    use super::{Catalog, Element, fallbacks};

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

    #[test]
    fn a_tag_falls_back_a_subtag_at_a_time() {
        assert_eq!(
            fallbacks("sr-Latn-RS").collect::<Vec<_>>(),
            ["sr-Latn-RS", "sr-Latn", "sr"]
        );
        assert_eq!(fallbacks("fr").collect::<Vec<_>>(), ["fr"]);
    }
}
