//! Exclusions (ADR-125): the files and folders skwir treats apart, each kept from
//! roles. A role is a kind of reader: [`Role::Searches`] (a search's walk,
//! a tag's count, the timeline: never below the folder looked in),
//! [`Role::Agents`] (an agent, a plugin from outside, a model: never named,
//! listed or read for them; what was `permissions::Privacy`'s list), and
//! [`Role::Index`] (the harvest ADR-092 designs, not built).
//!
//! - **One list:** skwir's own rules (`builtin.toml`, ids `skwir.…`), in
//!   categories, and the user's, in skwir's categories or their own. A
//!   category's switch for a role (the heading's) and an entry's tick for
//!   it both on: the entry's folders are kept from it.
//! - **Stored as what changed:** `exclusions.categories.<id>` and
//!   `exclusions.entries.<id>`, tables keyed by id (Home Manager's merge and
//!   an agent's dotted key change one), a built-in's override or the
//!   user's own; never a copy of skwir's list, so what a release adds
//!   reaches everyone. A user's id holds no dot; an override of a built-in
//!   a release dropped does nothing.
//! - **An entry** is a name (any folder so named), a path, a glob, a
//!   `regex:` over the folder's path (`~` for home), with `beside` (only
//!   where the folder has that sibling), or `holding` (a folder holding a
//!   marker file: a signed `CACHEDIR.TAG`, `.private`).
//!
//! What each reader makes of its rule is its own: a walk leaves out what
//! is below its root (`walks::Below`), privacy refuses the path, what is
//! under it and its target, fail closed (`permissions::Checker`). The
//! trash and network mounts are left out of walks by code, whatever the
//! list says.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, LazyLock};

use globset::{GlobBuilder, GlobMatcher, GlobSet, GlobSetBuilder};
use regex::bytes::{RegexSet, RegexSetBuilder};

use crate::settings::{Beside, SettingSpec, Settings};

#[cfg(test)]
mod tests;

/// The settings' namespace: the Exclusions page.
pub const OWNER: &str = "exclusions";
/// The categories' overrides and the user's own, by id.
pub const CATEGORIES_KEY: &str = "exclusions.categories";
/// Whether the login shell is asked for the variables paths start from
/// (`environment`, `skwir_platform::shell_environment`).
pub const SHELL_VARIABLES_KEY: &str = "exclusions.shell_variables";
/// The entries' overrides and the user's own, by id.
pub const ENTRIES_KEY: &str = "exclusions.entries";
/// What a built-in's id starts with: skwir's.
pub const BUILTIN: &str = "skwir.";
/// The category the user's entries go to when they name none.
pub const YOURS: &str = "skwir.yours";
/// Before exclusions (0.3.1): the private folders, a list of records.
pub const LEGACY_FOLDERS_KEY: &str = "privacy.folders";
/// Before exclusions: whether `.private` makes a folder private.
pub const LEGACY_MARKER_KEY: &str = "privacy.marker";

/// The file that says its folder is a cache, and how it begins.
pub const CACHEDIR_TAG: &str = "CACHEDIR.TAG";
const SIGNATURE: &[u8] = b"Signature: 8a477f597d28d172789f06886806bc55";

/// Changed when what a rule means changes: what was made under another
/// is made again (`Rule::fingerprint`).
const RULE_VERSION: u64 = 2;

/// A kind of reader a folder may be kept from.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// A search's walk, a tag's count, the timeline.
    Searches,
    /// Agents, plugins from outside, models: privacy.
    Agents,
    /// The index of what files hold (ADR-092): not built.
    Index,
}

impl Role {
    pub const ALL: [Role; 3] = [Role::Searches, Role::Agents, Role::Index];

    /// Its key in a category's or an entry's table.
    pub fn key(self) -> &'static str {
        match self {
            Role::Searches => "searches",
            Role::Agents => "agents",
            Role::Index => "index",
        }
    }

    fn at(self) -> usize {
        match self {
            Role::Searches => 0,
            Role::Agents => 1,
            Role::Index => 2,
        }
    }
}

/// On or off for each role.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Switches([bool; 3]);

impl Switches {
    pub const ALL: Switches = Switches([true; 3]);
    pub const NONE: Switches = Switches([false; 3]);

    pub fn get(self, role: Role) -> bool {
        self.0[role.at()]
    }

    #[must_use]
    pub fn with(mut self, role: Role, on: bool) -> Switches {
        self.0[role.at()] = on;
        self
    }

    /// These, each role a table says changed (`searches = false`).
    fn over(mut self, table: &toml::Table) -> Switches {
        for role in Role::ALL {
            if let Some(on) = table.get(role.key()).and_then(toml::Value::as_bool) {
                self = self.with(role, on);
            }
        }
        self
    }
}

/// What an exclusion's pattern names: `*.kdbx` files, `node_modules`
/// folders.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Names {
    Files,
    Folders,
    #[default]
    Both,
}

impl Names {
    /// As the settings and `builtin.toml` write it.
    pub fn key(self) -> &'static str {
        match self {
            Names::Files => "files",
            Names::Folders => "folders",
            Names::Both => "both",
        }
    }

    fn of_key(key: &str) -> Option<Names> {
        [Names::Files, Names::Folders, Names::Both]
            .into_iter()
            .find(|names| names.key() == key)
    }

    fn folders(self) -> bool {
        self != Names::Files
    }

    fn files(self) -> bool {
        self != Names::Folders
    }

    /// "any file named x", "any file or folder named x".
    fn any(self) -> &'static str {
        match self {
            Names::Files => "any file",
            Names::Folders => "any folder",
            Names::Both => "any file or folder",
        }
    }

    /// "the files like x", "the files and folders like x".
    fn the(self) -> &'static str {
        match self {
            Names::Files => "the files",
            Names::Folders => "the folders",
            Names::Both => "the files and folders",
        }
    }
}

/// What an exclusion names.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Matcher {
    /// A name, a path, a glob or a `regex:`, as written (a built-in's
    /// under a base written from the home folder, `~/.local/share/x`);
    /// with `beside`, a folder only where it has that sibling (a name or
    /// a glob).
    Pattern {
        text: String,
        beside: Option<String>,
        names: Names,
    },
    /// A folder holding this file.
    Holding(String),
}

impl Matcher {
    /// Whether the rules can read it, or what stops them: a glob or a
    /// regex that does not parse, a path that starts nowhere, a sibling
    /// written as a path, a folder beside a file written as a path (a
    /// name is beside its tool's file, not a path).
    ///
    /// # Errors
    /// What is wrong, in words for the user.
    pub fn check(&self) -> Result<(), String> {
        match self {
            Matcher::Pattern {
                text,
                beside,
                names,
            } => {
                let pattern = Pattern::of(text)?;
                if let Some(sibling) = beside {
                    if !names.folders() {
                        return Err(format!(
                            "{}: beside a file goes with folders, not files",
                            text.trim()
                        ));
                    }
                    if !matches!(pattern, Pattern::Name(_) | Pattern::NameGlob(_)) {
                        return Err(format!(
                            "{}: beside a file, the folder is a name (target), not a path",
                            text.trim()
                        ));
                    }
                    NameMatch::of(sibling)?;
                }
                Ok(())
            }
            Matcher::Holding(file) => {
                if file.trim().is_empty() || file.contains(['/', '\\']) {
                    Err("holding is the name of a file a folder holds".into())
                } else {
                    Ok(())
                }
            }
        }
    }

    /// The one folder it names, as written: a path, never a name, a glob,
    /// a regex, a beside or a holding rule (those name folders wherever
    /// they are).
    pub fn one_path(&self) -> Option<&str> {
        match self {
            Matcher::Pattern {
                text, beside: None, ..
            } if Written::of(text) == Written::Path => Some(text.trim()),
            _ => None,
        }
    }

    /// The path or the path glob it names, as written, whose folders can
    /// be looked up on this computer ([`here`]): never a name, a regex, a
    /// beside or a holding rule, which name folders wherever they are.
    pub fn looked_up(&self) -> Option<&str> {
        match self {
            Matcher::Pattern {
                text, beside: None, ..
            } if matches!(Written::of(text), Written::Path | Written::PathGlob) => {
                Some(text.trim())
            }
            _ => None,
        }
    }

    /// What it means, in a few words: "any folder named target beside a
    /// Cargo.toml", "~/.cargo/registry".
    pub fn said(&self) -> String {
        match self {
            Matcher::Holding(file) => format!("any folder holding a {file}"),
            Matcher::Pattern {
                text,
                beside,
                names,
            } => {
                // Told by how it is written, nothing compiled: the page
                // says it of every row.
                let what = match Written::of(text) {
                    Written::Name => format!("{} named {text}", names.any()),
                    Written::NameGlob => format!("{} whose name is like {text}", names.any()),
                    Written::PathGlob => format!("{} like {text}", names.the()),
                    Written::Regex => {
                        format!("{} matching {}", names.the(), &text.trim()[REGEX.len()..])
                    }
                    // Where a variable puts it now.
                    Written::Path if !variables_in(text).is_empty() => {
                        match expand(text, crate::os_path::home().as_deref()) {
                            Some(path) => format!("{text}, now {}", tilde(&path)),
                            None => format!(
                                "{text}, while {} is not set: none",
                                variables_in(text).join(", ")
                            ),
                        }
                    }
                    Written::Path => text.clone(),
                };
                match beside {
                    Some(sibling) => format!("{what} beside a {sibling}"),
                    None => what,
                }
            }
        }
    }
}

/// A category of the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Category {
    pub id: String,
    pub name: String,
    pub builtin: bool,
    /// Its switches now.
    pub switches: Switches,
    /// A built-in's switches as skwir ships them.
    pub shipped: Option<Switches>,
}

/// An entry of the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub builtin: bool,
    pub category: String,
    /// A built-in's category as skwir ships it.
    pub shipped_category: Option<String>,
    pub what: String,
    /// A sub-heading within its category (a language).
    pub group: Option<String>,
    pub matcher: Matcher,
    /// Its ticks now.
    pub ticks: Switches,
}

impl Entry {
    /// Whether it differs from what skwir ships: switched or moved. The
    /// user's own entries always do.
    pub fn changed(&self) -> bool {
        !self.builtin
            || self.ticks != Switches::ALL
            || self.shipped_category.as_deref() != Some(self.category.as_str())
    }
}

/// The list as the settings say it now: skwir's, with what the user
/// changed of it and added.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Catalog {
    pub categories: Vec<Category>,
    pub entries: Vec<Entry>,
    /// What the settings said that could not be taken, said as the
    /// settings' check says a problem.
    pub problems: Vec<String>,
    /// What they hold that this list has no place for.
    aside: Aside,
}

/// What the settings hold that a list read here could not take in, written
/// back as it stood ([`Catalog::values`]), so a tick on the page never
/// erases it: an override of one of skwir's for another system, for a base
/// unset on this launch or dropped by a release, a category or an entry
/// that cannot be read (said in [`Catalog::problems`]).
#[derive(Clone, Debug, Default, PartialEq)]
struct Aside {
    categories: toml::Table,
    entries: toml::Table,
    /// The categories asked for, by entry, that are not here: the entry
    /// stands in another meanwhile, which is not its choice.
    wanted: HashMap<String, String>,
}

// What `builtin.toml` holds.
#[derive(serde::Deserialize)]
struct Shipped {
    base: Vec<ShippedBase>,
    category: Vec<ShippedCategory>,
    entry: Vec<ShippedEntry>,
}

/// A folder entries are written under (`under`): a path that starts at a
/// variable, with what stands for it unset.
#[derive(serde::Deserialize)]
struct ShippedBase {
    name: String,
    path: String,
    systems: Option<Vec<String>>,
    /// What separates the list its variable holds, where it is neither
    /// `:` nor Windows' `;` (conda's `,`).
    separator: Option<char>,
}

#[derive(serde::Deserialize)]
struct ShippedCategory {
    id: String,
    name: String,
    searches: bool,
    agents: bool,
    index: bool,
}

#[derive(serde::Deserialize)]
struct ShippedEntry {
    id: String,
    category: String,
    what: String,
    pattern: Option<String>,
    /// `files` or `both`; a shipped pattern names folders otherwise.
    names: Option<String>,
    holding: Option<String>,
    beside: Option<String>,
    group: Option<String>,
    systems: Option<Vec<String>>,
    under: Option<String>,
}

/// `builtin.toml`, read once.
fn shipped_table() -> &'static Shipped {
    static SHIPPED: LazyLock<Shipped> = LazyLock::new(|| {
        toml::from_str(include_str!("builtin.toml")).expect("builtin.toml is skwir's own")
    });
    &SHIPPED
}

/// skwir's list for this system, its bases where the environment puts
/// them: made again only once what the login shell said changed
/// (`environment::generation`).
fn builtin() -> Arc<Catalog> {
    static BUILTIN: parking_lot::Mutex<Option<(u64, Arc<Catalog>)>> = parking_lot::Mutex::new(None);
    let generation = crate::environment::generation();
    let mut kept = BUILTIN.lock();
    if let Some((at, catalog)) = kept.as_ref()
        && *at == generation
    {
        return Arc::clone(catalog);
    }
    let catalog = Arc::new(shipped_catalog(shipped_table()));
    *kept = Some((generation, Arc::clone(&catalog)));
    catalog
}

/// Whether what `systems` says holds on this system.
fn for_this_system(systems: Option<&Vec<String>>) -> bool {
    systems.is_none_or(|systems| systems.iter().any(|s| s == SYSTEM))
}

/// The system this build is for, as `systems` says it.
const SYSTEM: &str = if cfg!(target_os = "macos") {
    "macos"
} else if cfg!(windows) {
    "windows"
} else {
    "linux"
};

fn shipped_catalog(shipped: &Shipped) -> Catalog {
    let categories = shipped
        .category
        .iter()
        .map(|c| {
            let switches = Switches([c.searches, c.agents, c.index]);
            Category {
                id: format!("{BUILTIN}{}", c.id),
                name: c.name.clone(),
                builtin: true,
                switches,
                shipped: Some(switches),
            }
        })
        .collect();
    let home = crate::os_path::home();
    // Each base where the environment puts it now; one unset with no
    // default (the runtime folder, elsewhere than Linux) holds nothing.
    let home = home.as_deref();
    let bases: HashMap<&str, Base> = shipped
        .base
        .iter()
        .filter(|b| for_this_system(b.systems.as_ref()))
        .map(|b| {
            let now = expand_by(&b.path, home, &|name| {
                variable_path(name, home, b.separator)
            });
            let unmoved = expand_by(&b.path, home, &|_| None);
            (b.name.as_str(), Base { now, unmoved })
        })
        .collect();
    let private: HashSet<&str> = shipped
        .category
        .iter()
        .filter(|c| c.agents)
        .map(|c| c.id.as_str())
        .collect();
    let mut entries: Vec<Entry> = Vec::new();
    for e in &shipped.entry {
        if !for_this_system(e.systems.as_ref()) {
            continue;
        }
        // Where a variable moved it, what was kept from agents where it
        // was stays so: a credential left behind is one still.
        let mut left_behind = None;
        let names = e
            .names
            .as_deref()
            .and_then(Names::of_key)
            .unwrap_or(Names::Folders);
        let matcher = match (&e.pattern, &e.holding) {
            (Some(folder), None) => {
                let text = match e.under.as_deref() {
                    None => folder.clone(),
                    Some(base) => {
                        let Some(Base {
                            now: Some(now),
                            unmoved,
                        }) = bases.get(base)
                        else {
                            continue;
                        };
                        if private.contains(e.category.as_str())
                            && let Some(unmoved) = unmoved
                            && unmoved != now
                        {
                            left_behind = Some(tilde(&joined(unmoved, folder)));
                        }
                        tilde(&joined(now, folder))
                    }
                };
                Matcher::Pattern {
                    text,
                    beside: e.beside.clone(),
                    names,
                }
            }
            (None, Some(file)) => Matcher::Holding(file.clone()),
            _ => continue,
        };
        let category = format!("{BUILTIN}{}", e.category);
        let entry = |id: String, what: String, matcher: Matcher| Entry {
            id,
            builtin: true,
            shipped_category: Some(category.clone()),
            category: category.clone(),
            what,
            group: e.group.clone(),
            matcher,
            ticks: Switches::ALL,
        };
        entries.push(entry(format!("{BUILTIN}{}", e.id), e.what.clone(), matcher));
        if let Some(text) = left_behind {
            entries.push(entry(
                format!("{BUILTIN}{}.before-it-moved", e.id),
                format!("{} (where it was before it moved)", e.what),
                Matcher::Pattern {
                    text,
                    beside: None,
                    names,
                },
            ));
        }
    }
    // The cache home, where it was moved from ~/.cache, which the `.cache`
    // name leaves out already.
    #[cfg(not(any(target_os = "macos", windows)))]
    if let Some(Base {
        now: Some(cache), ..
    }) = bases.get("cache")
        && cache.file_name() != Some(OsStr::new(".cache"))
    {
        entries.push(Entry {
            id: format!("{BUILTIN}caches.home"),
            builtin: true,
            category: format!("{BUILTIN}caches"),
            shipped_category: Some(format!("{BUILTIN}caches")),
            what: "Your cache folder (XDG_CACHE_HOME)".into(),
            group: None,
            matcher: Matcher::Pattern {
                text: tilde(cache),
                beside: None,
                names: Names::Folders,
            },
            ticks: Switches::ALL,
        });
    }
    Catalog {
        categories,
        entries,
        problems: Vec::new(),
        aside: Aside::default(),
    }
}

/// A base as the environment puts it now, and where it is unmoved.
struct Base {
    now: Option<PathBuf>,
    unmoved: Option<PathBuf>,
}

/// `folder` under `base`; empty, the base itself (a variable that names
/// a whole folder or a file).
fn joined(base: &Path, folder: &str) -> PathBuf {
    if folder.is_empty() {
        base.to_path_buf()
    } else {
        base.join(folder)
    }
}

/// `path` as the list says it: from the home folder with `~`, `/` between
/// its names on every system.
pub fn tilde(path: &Path) -> String {
    let home = crate::os_path::home();
    let (start, rest) = match home
        .as_deref()
        .and_then(|home| path.strip_prefix(home).ok())
    {
        Some(rest) => ("~".to_owned(), rest.to_owned()),
        None => return path.display().to_string(),
    };
    let parts: Vec<String> = rest
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    if parts.is_empty() {
        start
    } else {
        format!("{start}/{}", parts.join("/"))
    }
}

impl Catalog {
    /// What the settings say now: skwir's list, the 0.3.1 private folders
    /// a file still holds, and what `exclusions.*` changes and adds.
    pub fn of(settings: &Settings) -> Catalog {
        Catalog::from_values(
            settings.get::<toml::Value>(CATEGORIES_KEY).as_ref(),
            settings.get::<toml::Value>(ENTRIES_KEY).as_ref(),
            &Legacy::of(settings),
        )
    }

    /// skwir's list alone.
    pub fn shipped() -> Catalog {
        (*builtin()).clone()
    }

    /// The variables its paths start from, skwir's bases' among them:
    /// those the login shell is asked for (`environment::want`).
    pub fn variables(&self) -> BTreeSet<String> {
        let mut names: BTreeSet<String> = shipped_table()
            .base
            .iter()
            .filter(|b| for_this_system(b.systems.as_ref()))
            .flat_map(|b| variables_in(&b.path))
            .collect();
        for entry in &self.entries {
            if let Matcher::Pattern { text, .. } = &entry.matcher {
                names.extend(variables_in(text));
            }
        }
        names
    }

    /// skwir's list, these values over it (the two tables), and what a
    /// 0.3.1 file said under them.
    pub fn from_values(
        categories: Option<&toml::Value>,
        entries: Option<&toml::Value>,
        legacy: &Legacy,
    ) -> Catalog {
        let mut catalog = (*builtin()).clone();
        let empty = toml::Table::new();
        let categories = categories.and_then(toml::Value::as_table).unwrap_or(&empty);
        let entries = entries.and_then(toml::Value::as_table).unwrap_or(&empty);
        legacy.apply(&mut catalog, entries);
        let category_at: HashMap<String, usize> = catalog
            .categories
            .iter()
            .enumerate()
            .map(|(at, c)| (c.id.clone(), at))
            .collect();
        for (id, value) in categories {
            let aside = |problem: Option<String>, catalog: &mut Catalog| {
                catalog.aside.categories.insert(id.clone(), value.clone());
                catalog
                    .problems
                    .extend(problem.map(|p| format!("{CATEGORIES_KEY}.{id}: {p}")));
            };
            let Some(table) = value.as_table() else {
                aside(Some("a category is a table".into()), &mut catalog);
                continue;
            };
            if let Err(problem) = category_problem(id, table) {
                aside(Some(problem), &mut catalog);
                continue;
            }
            if let Some(&at) = category_at.get(id) {
                let category = &mut catalog.categories[at];
                category.switches = category.switches.over(table);
                continue;
            }
            if id.starts_with(BUILTIN) {
                // A category a later release dropped, or another system's:
                // nothing to change here.
                aside(None, &mut catalog);
                continue;
            }
            catalog.categories.push(Category {
                id: id.clone(),
                name: table
                    .get("name")
                    .and_then(toml::Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                builtin: false,
                switches: Switches::ALL.over(table),
                shipped: None,
            });
        }
        let known: HashSet<String> = catalog.categories.iter().map(|c| c.id.clone()).collect();
        let category_of =
            |table: &toml::Table, fallback: &str, catalog: &mut Catalog, id: &str| match table
                .get("category")
                .and_then(toml::Value::as_str)
            {
                Some(category) if known.contains(category) => category.to_owned(),
                Some(category) => {
                    catalog
                        .problems
                        .push(format!("{ENTRIES_KEY}.{id}: no category {category}"));
                    catalog
                        .aside
                        .wanted
                        .insert(id.to_owned(), category.to_owned());
                    fallback.to_owned()
                }
                None => fallback.to_owned(),
            };
        let entry_at: HashMap<String, usize> = catalog
            .entries
            .iter()
            .enumerate()
            .map(|(at, e)| (e.id.clone(), at))
            .collect();
        for (id, value) in entries {
            let aside = |problem: Option<String>, catalog: &mut Catalog| {
                catalog.aside.entries.insert(id.clone(), value.clone());
                catalog
                    .problems
                    .extend(problem.map(|p| format!("{ENTRIES_KEY}.{id}: {p}")));
            };
            let Some(table) = value.as_table() else {
                aside(Some("an entry is a table".into()), &mut catalog);
                continue;
            };
            if id.starts_with(BUILTIN) {
                if let Err(problem) = override_problem(table) {
                    aside(Some(problem), &mut catalog);
                    continue;
                }
                let Some(&at) = entry_at.get(id) else {
                    // Another system's, a base unset here, a built-in a
                    // later release dropped: it does nothing here.
                    aside(None, &mut catalog);
                    continue;
                };
                let fallback = catalog.entries[at].category.clone();
                let category = category_of(table, &fallback, &mut catalog, id);
                let entry = &mut catalog.entries[at];
                entry.ticks = entry.ticks.over(table);
                entry.category = category;
                continue;
            }
            match user_entry(id, table) {
                Ok(mut entry) => {
                    entry.category = category_of(table, YOURS, &mut catalog, id);
                    catalog.entries.push(entry);
                }
                Err(problem) => aside(Some(problem), &mut catalog),
            }
        }
        catalog
    }

    /// The ids of entries taken, what is set aside included: a new entry
    /// gets none of them.
    pub fn entry_ids(&self) -> HashSet<String> {
        let held = self.entries.iter().map(|e| e.id.clone());
        held.chain(self.aside.entries.keys().cloned()).collect()
    }

    /// The ids of categories taken, what is set aside included.
    pub fn category_ids(&self) -> HashSet<String> {
        let held = self.categories.iter().map(|c| c.id.clone());
        held.chain(self.aside.categories.keys().cloned()).collect()
    }

    pub fn category(&self, id: &str) -> Option<&Category> {
        self.categories.iter().find(|c| c.id == id)
    }

    /// Whether `entry` is kept from `role`: its category's switch and its
    /// tick.
    pub fn keeps(&self, entry: &Entry, role: Role) -> bool {
        entry.ticks.get(role)
            && self
                .category(&entry.category)
                .is_some_and(|c| c.switches.get(role))
    }

    /// The entries kept from `role`.
    pub fn kept_from(&self, role: Role) -> impl Iterator<Item = &Entry> {
        self.entries.iter().filter(move |e| self.keeps(e, role))
    }

    /// What `role`'s readers leave out, made once to be asked many times.
    pub fn rule(&self, role: Role) -> Rule {
        Rule::of(self.kept_from(role).map(|e| &e.matcher))
    }

    /// The overrides and the user's own entries and categories that make
    /// this list from skwir's: what the two settings would hold. A 0.3.1
    /// file's private folders, read under them, are written out with them.
    pub fn values(&self) -> (toml::Value, toml::Value) {
        let builtin = builtin();
        let shipped: std::collections::HashMap<&str, &Entry> =
            builtin.entries.iter().map(|e| (e.id.as_str(), e)).collect();
        let mut categories = self.aside.categories.clone();
        for category in &self.categories {
            let mut table = toml::Table::new();
            let base = category.shipped.unwrap_or(Switches::ALL);
            for role in Role::ALL {
                if category.switches.get(role) != base.get(role) {
                    table.insert(
                        role.key().into(),
                        toml::Value::Boolean(category.switches.get(role)),
                    );
                }
            }
            if !category.builtin {
                table.insert("name".into(), toml::Value::String(category.name.clone()));
            }
            if !table.is_empty() {
                categories.insert(category.id.clone(), toml::Value::Table(table));
            }
        }
        let mut entries = self.aside.entries.clone();
        for entry in &self.entries {
            let mut table = toml::Table::new();
            let wanted = self.aside.wanted.get(&entry.id);
            if entry.builtin {
                let Some(was) = shipped.get(entry.id.as_str()) else {
                    continue;
                };
                for role in Role::ALL {
                    if entry.ticks.get(role) != was.ticks.get(role) {
                        table.insert(
                            role.key().into(),
                            toml::Value::Boolean(entry.ticks.get(role)),
                        );
                    }
                }
                if entry.category != was.category {
                    table.insert(
                        "category".into(),
                        toml::Value::String(entry.category.clone()),
                    );
                } else if let Some(wanted) = wanted {
                    table.insert("category".into(), toml::Value::String(wanted.clone()));
                }
            } else {
                match &entry.matcher {
                    Matcher::Pattern {
                        text,
                        beside,
                        names,
                    } => {
                        table.insert("pattern".into(), toml::Value::String(text.clone()));
                        table.insert("names".into(), toml::Value::String(names.key().to_owned()));
                        if let Some(beside) = beside {
                            table.insert("beside".into(), toml::Value::String(beside.clone()));
                        }
                    }
                    Matcher::Holding(file) => {
                        table.insert("holding".into(), toml::Value::String(file.clone()));
                    }
                }
                if !entry.what.is_empty() {
                    table.insert("what".into(), toml::Value::String(entry.what.clone()));
                }
                if entry.category != YOURS {
                    table.insert(
                        "category".into(),
                        toml::Value::String(entry.category.clone()),
                    );
                } else if let Some(wanted) = wanted {
                    table.insert("category".into(), toml::Value::String(wanted.clone()));
                }
                for role in Role::ALL {
                    table.insert(
                        role.key().into(),
                        toml::Value::Boolean(entry.ticks.get(role)),
                    );
                }
            }
            if !table.is_empty() {
                entries.insert(entry.id.clone(), toml::Value::Table(table));
            }
        }
        (toml::Value::Table(categories), toml::Value::Table(entries))
    }
}

/// Whether `id` may be the user's: no dot (an owner's), not empty.
fn user_id(id: &str) -> Result<(), String> {
    if id.is_empty() {
        return Err("an id is needed".into());
    }
    if id.contains('.') {
        return Err("an id of yours has no dot: those are skwir's and its plugins'".into());
    }
    Ok(())
}

/// A user's entry from its table.
fn user_entry(id: &str, table: &toml::Table) -> Result<Entry, String> {
    user_id(id)?;
    let text = |key: &str| table.get(key).and_then(toml::Value::as_str).map(str::trim);
    let names = match text("names") {
        None => Names::Both,
        Some(names) => Names::of_key(names)
            .ok_or_else(|| format!("names is files, folders or both, not {names}"))?,
    };
    let matcher = match (text("pattern"), text("holding")) {
        (Some(pattern), None) => Matcher::Pattern {
            text: pattern.to_owned(),
            beside: text("beside").filter(|b| !b.is_empty()).map(str::to_owned),
            names,
        },
        (None, Some(_)) if table.contains_key("beside") || table.contains_key("names") => {
            return Err("beside and names go with a pattern, not with holding".into());
        }
        (None, Some(file)) => Matcher::Holding(file.to_owned()),
        (Some(_), Some(_)) => return Err("a pattern or a file it holds, not both".into()),
        (None, None) => return Err("pattern or holding says what it names".into()),
    };
    matcher.check()?;
    for (key, field) in table {
        match key.as_str() {
            "searches" | "agents" | "index" if !field.is_bool() => {
                return Err(format!("{key}: true or false"));
            }
            "pattern" | "names" | "holding" | "beside" | "what" | "category" | "searches"
            | "agents" | "index" => {}
            _ => return Err(format!("no field {key}")),
        }
    }
    Ok(Entry {
        id: id.to_owned(),
        builtin: false,
        category: YOURS.to_owned(),
        shipped_category: None,
        what: text("what").unwrap_or_default().to_owned(),
        group: None,
        matcher,
        // A hand-written entry naming no role is kept from every role its
        // category is: as skwir's are.
        ticks: Switches::ALL.over(table),
    })
}

/// The fields a built-in's override may hold.
const OVERRIDES: &[&str] = &["searches", "agents", "index", "category"];

/// What stops a category's table being read: a built-in's holds switches,
/// the user's a name, under an id of no dot.
fn category_problem(id: &str, category: &toml::Table) -> Result<(), String> {
    let builtin = id.starts_with(BUILTIN);
    if !builtin {
        user_id(id)?;
        if category
            .get("name")
            .and_then(toml::Value::as_str)
            .is_none_or(|n| n.trim().is_empty())
        {
            return Err("a category of yours needs a name".into());
        }
    }
    for (key, field) in category {
        match key.as_str() {
            "name" if !builtin => {}
            "searches" | "agents" | "index" if field.is_bool() => {}
            "searches" | "agents" | "index" => return Err(format!("{key}: true or false")),
            _ => return Err(format!("no field {key}")),
        }
    }
    Ok(())
}

/// What stops an override of one of skwir's being read: it holds switches
/// and a category, never a folder.
fn override_problem(entry: &toml::Table) -> Result<(), String> {
    for (key, field) in entry {
        if key == "folder" || key == "holding" {
            return Err("skwir. names skwir's own folders; give yours a name without a dot".into());
        }
        if !OVERRIDES.contains(&key.as_str()) {
            return Err(format!("no field {key}"));
        }
        if key != "category" && !field.is_bool() {
            return Err(format!("{key}: true or false"));
        }
    }
    Ok(())
}

/// The table stored beside `beside`, where a value replaces one: a key it
/// holds as it stands is spared, as a shortcut waiting for its plugin is.
/// A file's value is judged whole (`settings.check`).
fn stored_table(beside: Beside<'_>) -> Option<&toml::Table> {
    match beside {
        Beside::Stored(stored) => stored?.as_table(),
        Beside::File => None,
    }
}

/// The settings' own check of the categories. What was stored as it is
/// passes, so one a hand wrote wrong stops no other change: it is said on
/// the page, and kept.
fn check_categories(value: &toml::Value, beside: Beside<'_>) -> anyhow::Result<()> {
    let Some(table) = value.as_table() else {
        anyhow::bail!("folder categories are a table, by id");
    };
    let stored = stored_table(beside);
    for (id, category) in table {
        if stored.is_some_and(|stored| stored.get(id) == Some(category)) {
            continue;
        }
        let Some(category) = category.as_table() else {
            anyhow::bail!("{id}: a category is a table");
        };
        category_problem(id, category).map_err(|e| anyhow::anyhow!("{id}: {e}"))?;
    }
    Ok(())
}

/// The settings' own check of the entries, what was stored as it is
/// spared as [`check_categories`] spares it.
fn check_entries(value: &toml::Value, beside: Beside<'_>) -> anyhow::Result<()> {
    let Some(table) = value.as_table() else {
        anyhow::bail!("exclusions are a table, by id");
    };
    let stored = stored_table(beside);
    for (id, entry) in table {
        if stored.is_some_and(|stored| stored.get(id) == Some(entry)) {
            continue;
        }
        let Some(entry) = entry.as_table() else {
            anyhow::bail!("{id}: an entry is a table");
        };
        if id.starts_with(BUILTIN) {
            override_problem(entry).map_err(|e| anyhow::anyhow!("{id}: {e}"))?;
        } else {
            user_entry(id, entry).map_err(|e| anyhow::anyhow!("{id}: {e}"))?;
        }
    }
    Ok(())
}

/// What the settings hold of the exclusions that cannot be read, told
/// once, as a notice leading to the page: what a hand or a file wrote
/// wrong keeps nothing private, and says so at the start and when the file
/// changes. On a worker: it reads the catalog.
pub(crate) fn tell_problems(app: &crate::App) {
    static TOLD: parking_lot::Mutex<BTreeSet<String>> = parking_lot::Mutex::new(BTreeSet::new());
    let problems: BTreeSet<String> = Catalog::of(&app.settings).problems.into_iter().collect();
    let mut told = TOLD.lock();
    let new = problems.difference(&told).count();
    if let Some(first) = problems.difference(&told).next() {
        let more = if new > 1 {
            format!(" (and {} more)", new - 1)
        } else {
            String::new()
        };
        app.notify_about_setting(
            crate::app::NotifyLevel::Warning,
            format!("An exclusion cannot be read, so it does not apply{more}: {first}"),
            ENTRIES_KEY,
        );
    }
    *told = problems;
}

/// Whether the settings hold any exclusions of the user's: what there is to
/// read wrong. Asked at start, where reading the list would cost for none.
pub(crate) fn holds_changes(settings: &Settings) -> bool {
    [ENTRIES_KEY, CATEGORIES_KEY].into_iter().any(|key| {
        settings
            .get::<toml::Value>(key)
            .and_then(|value| value.as_table().map(|table| !table.is_empty()))
            .unwrap_or(false)
    })
}

/// Declare the two settings (the Exclusions page draws them), and the 0.3.1
/// ones still read.
pub fn declare(settings: &Settings) {
    let table = || toml::Value::Table(toml::Table::new());
    settings.declare(
        SettingSpec::drawn_by_page(
            CATEGORIES_KEY,
            "Exclusion categories",
            "What changed of skwir's categories of exclusions (a role switched: searches, agents, index), and yours (a name)",
            table(),
        )
        .page(crate::settings::page::EXCLUSIONS)
        .validate_with(check_categories),
    );
    settings.declare(
        SettingSpec::drawn_by_page(
            ENTRIES_KEY,
            "Exclusions",
            "What changed of skwir's exclusions (a role unticked, a category), and yours: a pattern (a name, a path, a glob or regex:…, of files, folders or both, with beside), or holding (a file a folder holds), what it is, its category, and the roles it is excluded from",
            table(),
        )
        .page(crate::settings::page::EXCLUSIONS)
        .validate_with(check_entries),
    );
    settings.declare(
        SettingSpec::toggle(
            SHELL_VARIABLES_KEY,
            "Read your shell's variables",
            "Read variables such as CARGO_HOME from your login shell too, where a path starts at one. A Skwir started from your desktop does not see what .zshrc or config.fish export. The shell is asked in the background once its files change, and only the variables these paths name are kept.",
            true,
        )
        .page(crate::settings::page::PRIVACY),
    );
    // Read under the exclusions, written out when the Exclusions page saves.
    settings.declare(
        SettingSpec::drawn_by_page(
            LEGACY_FOLDERS_KEY,
            "Private folders (before exclusions)",
            "0.3.1's private folders, read as exclusions kept from agents",
            toml::Value::Array(Vec::new()),
        )
        .page(crate::settings::page::EXCLUSIONS),
    );
    settings.declare(
        SettingSpec::drawn_by_page(
            LEGACY_MARKER_KEY,
            "Folders marked private (before exclusions)",
            "0.3.1's switch for .private, read as the exclusion holding a .private",
            toml::Value::Boolean(true),
        )
        .page(crate::settings::page::EXCLUSIONS),
    );
}

/// What a 0.3.1 settings file said of private folders, read under the
/// exclusions: the folders it listed that skwir does not are the user's,
/// the folders 0.3.1 shipped it no longer lists are unticked for agents,
/// and `.private` off unticks its rule. 0.3.1's list is kept here, as it
/// was: a folder skwir lists since is never taken for one the user removed.
#[derive(Clone, Debug, Default)]
pub struct Legacy {
    folders: Option<Vec<String>>,
    marker: Option<bool>,
}

/// 0.3.1's private folders, as `permissions::DEFAULT_FOLDERS` had them.
const PRIVATE_0_3_1: &[&str] = &[
    "~/.ssh",
    "~/.gnupg",
    "~/.password-store",
    #[cfg(not(any(target_os = "macos", windows)))]
    "~/.local/share/keyrings",
    #[cfg(not(any(target_os = "macos", windows)))]
    "~/.local/share/kwalletd",
    #[cfg(target_os = "macos")]
    "~/Library/Keychains",
    #[cfg(windows)]
    "~/AppData/Roaming/Microsoft/Credentials",
    #[cfg(windows)]
    "~/AppData/Local/Microsoft/Credentials",
    #[cfg(windows)]
    "~/AppData/Local/Microsoft/Vault",
    #[cfg(windows)]
    "~/AppData/Roaming/Microsoft/Protect",
    "~/.pki",
    "~/.aws",
    "~/.kube",
    "~/.docker",
    "~/.netrc",
];

impl Legacy {
    pub fn of(settings: &Settings) -> Legacy {
        Legacy::read(
            settings.get::<toml::Value>(LEGACY_FOLDERS_KEY).as_ref(),
            settings.get::<toml::Value>(LEGACY_MARKER_KEY).as_ref(),
        )
    }

    /// The two settings' values, as a file holds them.
    pub fn read(folders: Option<&toml::Value>, marker: Option<&toml::Value>) -> Legacy {
        let folders = folders
            .and_then(|value| value.as_array().cloned())
            .filter(|records| !records.is_empty())
            .map(|records| {
                records
                    .iter()
                    .filter_map(|record| match record {
                        toml::Value::String(path) => Some(path.clone()),
                        record => record
                            .get("path")
                            .and_then(toml::Value::as_str)
                            .map(str::to_owned),
                    })
                    .collect()
            });
        let marker = marker.and_then(toml::Value::as_bool).filter(|on| !on);
        Legacy { folders, marker }
    }

    /// For tests: a 0.3.1 file's list and switch.
    pub fn said(folders: Option<Vec<String>>, marker: Option<bool>) -> Legacy {
        Legacy { folders, marker }
    }

    pub fn is_empty(&self) -> bool {
        self.folders.is_none() && self.marker.is_none()
    }

    /// Into `catalog`, under what `entries` (the new table) says: a key it
    /// holds wins.
    fn apply(&self, catalog: &mut Catalog, entries: &toml::Table) {
        if self.marker == Some(false) {
            for entry in &mut catalog.entries {
                if entry.matcher == Matcher::Holding(".private".into())
                    && !entries.contains_key(&entry.id)
                {
                    entry.ticks = entry.ticks.with(Role::Agents, false);
                }
            }
        }
        let Some(listed) = &self.folders else { return };
        let home = crate::os_path::home();
        let normal = |text: &str| expand(text, home.as_deref());
        // Each of skwir's that is one path, by that path: asked once per
        // folder either list holds, never per pair.
        let mut at_path: HashMap<PathBuf, Vec<usize>> = HashMap::new();
        for (at, entry) in catalog.entries.iter().enumerate() {
            if let Matcher::Pattern {
                text, beside: None, ..
            } = &entry.matcher
                && let Some(path) = normal(text)
            {
                at_path.entry(path).or_default().push(at);
            }
        }
        let listed_paths: HashSet<PathBuf> = listed.iter().filter_map(|t| normal(t)).collect();
        // 0.3.1's own, absent from the list: unticked for agents.
        for was in PRIVATE_0_3_1 {
            let Some(path) = normal(was) else { continue };
            if listed_paths.contains(&path) {
                continue;
            }
            for &at in at_path.get(&path).map_or(&[][..], Vec::as_slice) {
                let entry = &mut catalog.entries[at];
                if entry.builtin && !entries.contains_key(&entry.id) {
                    entry.ticks = entry.ticks.with(Role::Agents, false);
                }
            }
        }
        // The list's own that no built-in is: the user's, in Secrets.
        let mut taken: HashSet<String> = catalog.entries.iter().map(|e| e.id.clone()).collect();
        for text in listed {
            let Some(path) = normal(text) else { continue };
            if at_path.contains_key(&path) {
                continue;
            }
            let id = unique_id(&format!("private {text}"), &taken);
            if entries.contains_key(&id) {
                continue;
            }
            taken.insert(id.clone());
            catalog.entries.push(Entry {
                id,
                builtin: false,
                category: format!("{BUILTIN}secrets"),
                shipped_category: None,
                what: "Private (from 0.3.1's list)".into(),
                group: None,
                matcher: Matcher::Pattern {
                    text: text.trim().to_owned(),
                    beside: None,
                    // 0.3.1's list said "folders" of what was only ever a
                    // path: a file listed stayed private.
                    names: Names::Both,
                },
                ticks: Switches::NONE
                    .with(Role::Agents, true)
                    .with(Role::Index, true),
            });
        }
    }
}

/// An id of the user's made from `what` (`vm-images`), another than
/// `taken` (`vm-images-2`).
pub fn unique_id<S: std::hash::BuildHasher>(what: &str, taken: &HashSet<String, S>) -> String {
    let mut slug = String::new();
    for c in what.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            slug.push(c);
        } else if !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
    }
    let slug = slug.trim_end_matches('-');
    let base = if slug.is_empty() { "folder" } else { slug };
    if !taken.contains(base) {
        return base.to_owned();
    }
    // One more than were taken always leaves one free.
    (2..=taken.len() + 2)
        .map(|n| format!("{base}-{n}"))
        .find(|id| !taken.contains(id))
        .unwrap_or_else(|| base.to_owned())
}

/// What a folder's text is, told by its syntax.
enum Pattern {
    /// Any folder so named.
    Name(OsString),
    /// Any folder whose name is like this.
    NameGlob(GlobMatcher),
    /// This folder.
    Path(PathBuf),
    /// The folders whose path is like this.
    PathGlob(GlobMatcher),
    /// The folders whose path, written with `~` for home, it matches.
    Regex(String),
    /// A path from a variable that is unset here, with no default: no
    /// folder, until it is set.
    Unset,
}

/// The prefix that says the rest is a regular expression.
const REGEX: &str = "regex:";

/// What a folder's text is, by how it is written: the one reading of the
/// syntax, for the rules and for what shows them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Written {
    /// Any folder so named (`node_modules`).
    Name,
    /// Any folder whose name is like it (`*.egg-info`).
    NameGlob,
    /// One folder (`~/.cargo/registry`, `/srv/cache`).
    Path,
    /// The folders whose path is like it (`~/.var/app/*/cache`).
    PathGlob,
    /// `regex:` over the folder's path, written with `~` for home.
    Regex,
}

impl Written {
    pub fn of(text: &str) -> Written {
        let text = text.trim();
        if text.starts_with(REGEX) {
            return Written::Regex;
        }
        // A path may start at a variable (`${CARGO_HOME:-~/.cargo}/x`),
        // whose braces are no glob's.
        let (start, rest) = Start::of(text);
        let path = !matches!(start, Start::Nothing) || text.contains(['/', '\\']);
        match (path, is_glob(rest)) {
            (false, false) => Written::Name,
            (false, true) => Written::NameGlob,
            (true, false) => Written::Path,
            (true, true) => Written::PathGlob,
        }
    }
}

/// Whether a name or a path is written as a glob.
fn is_glob(text: &str) -> bool {
    text.contains(['*', '?', '[', '{'])
}

impl Pattern {
    fn of(text: &str) -> Result<Pattern, String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("a folder needs a name or a path".into());
        }
        let written = Written::of(text);
        match written {
            Written::Regex => {
                let re = &text[REGEX.len()..];
                regex::bytes::Regex::new(re).map_err(|e| format!("regex: {e}"))?;
                return Ok(Pattern::Regex(re.to_owned()));
            }
            Written::NameGlob => return Ok(Pattern::NameGlob(glob_of(text)?)),
            Written::Name => return Ok(Pattern::Name(folded(OsStr::new(text)))),
            Written::Path | Written::PathGlob => {}
        }
        let home = crate::os_path::home();
        let Some(full) = expand(text, home.as_deref()) else {
            return match Start::of(text).0 {
                Start::Unreadable => Err(format!(
                    "{text}: a variable is $NAME, ${{NAME}} or ${{NAME:-what stands for it unset}}"
                )),
                // Set on another computer, or later.
                Start::Variable { .. } => Ok(Pattern::Unset),
                Start::Home | Start::Nothing => Err(format!(
                    "{text}: a path starts with ~, a variable ($CARGO_HOME) or is whole (/srv/cache)"
                )),
            };
        };
        Ok(if written == Written::PathGlob {
            // Matched as the system writes the path: on Windows, where a
            // glob has no escapes, globset reads `\` as `/` in the glob as
            // in the path.
            Pattern::PathGlob(glob_of(&full.to_string_lossy())?)
        } else {
            Pattern::Path(full)
        })
    }
}

/// A glob as names and paths are compared here: `*` within one name,
/// `**` across them; case folded on a Mac and on Windows.
fn glob_of(text: &str) -> Result<GlobMatcher, String> {
    GlobBuilder::new(text)
        .literal_separator(true)
        .case_insensitive(cfg!(any(target_os = "macos", windows)))
        .backslash_escape(!cfg!(windows))
        .build()
        .map(|glob| glob.compile_matcher())
        .map_err(|e| format!("{text}: {e}"))
}

/// A sibling a beside rule asks for: a name, or a glob of names.
#[derive(Clone, Debug)]
pub enum NameMatch {
    Exact(OsString),
    Glob(GlobMatcher),
}

impl NameMatch {
    fn of(text: &str) -> Result<NameMatch, String> {
        if text.contains(['/', '\\']) {
            return Err(format!(
                "{text}: beside names a file or a folder, not a path"
            ));
        }
        Ok(if is_glob(text) {
            NameMatch::Glob(glob_of(text)?)
        } else {
            NameMatch::Exact(folded(OsStr::new(text)))
        })
    }

    pub fn matches(&self, name: &OsStr) -> bool {
        match self {
            NameMatch::Exact(exact) => folded(name) == *exact,
            NameMatch::Glob(glob) => glob.is_match(name),
        }
    }

    /// The name itself, where it is one: asked by a lookup, no listing.
    pub fn exact(&self) -> Option<&OsStr> {
        match self {
            NameMatch::Exact(name) => Some(name),
            NameMatch::Glob(_) => None,
        }
    }
}

/// A folder named so (`folder`), left out where its parent holds
/// `sibling`.
#[derive(Clone, Debug)]
pub struct BesideRule {
    pub folder: NameMatch,
    pub sibling: NameMatch,
}

/// What one role's readers leave out, compiled: names and paths in sets,
/// globs and regexes each in one set, beside rules and markers to probe.
#[derive(Clone, Debug)]
pub struct Rule {
    /// What names folders, what names files.
    folders: Side,
    files: Side,
    /// The exact paths as written, for those who resolve or quote them.
    listed: Vec<PathBuf>,
    beside: Vec<BesideRule>,
    holding: Vec<OsString>,
    fingerprint: u64,
}

impl Default for Rule {
    fn default() -> Self {
        Rule::of(std::iter::empty())
    }
}

/// What names one kind of thing, folders or files.
#[derive(Clone, Debug, Default)]
struct Side {
    /// Exact paths, as the system compares them.
    paths: HashSet<OsString>,
    names: HashSet<OsString>,
    name_globs: GlobSet,
    path_globs: GlobSet,
    regexes: RegexSet,
}

impl Side {
    fn is_empty(&self) -> bool {
        self.paths.is_empty()
            && self.names.is_empty()
            && self.name_globs.is_empty()
            && self.path_globs.is_empty()
            && self.regexes.is_empty()
    }

    fn names(&self, path: &Path) -> bool {
        if self.is_empty() {
            return false;
        }
        if let Some(name) = path.file_name()
            && self.names_anywhere(name)
        {
            return true;
        }
        self.paths.contains(&folded(path.as_os_str()))
            || self.path_globs.is_match(path)
            || (!self.regexes.is_empty() && self.regexes.is_match(tilde(path).as_bytes()))
    }

    fn names_anywhere(&self, name: &OsStr) -> bool {
        self.names.contains(&folded(name)) || self.name_globs.is_match(name)
    }
}

/// A [`Side`] being made.
struct SideMade {
    paths: HashSet<OsString>,
    names: HashSet<OsString>,
    name_globs: GlobSetBuilder,
    path_globs: GlobSetBuilder,
    regexes: Vec<String>,
}

impl SideMade {
    fn new() -> SideMade {
        SideMade {
            paths: HashSet::new(),
            names: HashSet::new(),
            name_globs: GlobSetBuilder::new(),
            path_globs: GlobSetBuilder::new(),
            regexes: Vec::new(),
        }
    }

    fn add(&mut self, pattern: &Pattern) {
        match pattern {
            Pattern::Name(name) => {
                self.names.insert(name.clone());
            }
            Pattern::NameGlob(glob) => {
                self.name_globs.add(glob.glob().clone());
            }
            Pattern::PathGlob(glob) => {
                self.path_globs.add(glob.glob().clone());
            }
            Pattern::Regex(re) => self.regexes.push(re.clone()),
            Pattern::Path(path) => {
                self.paths.insert(folded(path.as_os_str()));
            }
            Pattern::Unset => {}
        }
    }

    fn made(self) -> Side {
        Side {
            paths: self.paths,
            names: self.names,
            name_globs: self.name_globs.build().unwrap_or_else(|_| GlobSet::empty()),
            path_globs: self.path_globs.build().unwrap_or_else(|_| GlobSet::empty()),
            regexes: RegexSetBuilder::new(&self.regexes)
                .case_insensitive(cfg!(any(target_os = "macos", windows)))
                .build()
                .unwrap_or_else(|_| RegexSet::empty()),
        }
    }
}

impl Rule {
    /// The rule these matchers make; one that cannot be read is left out
    /// (the settings' check refused it already).
    pub fn of<'a>(matchers: impl IntoIterator<Item = &'a Matcher>) -> Rule {
        let (mut folders, mut files) = (SideMade::new(), SideMade::new());
        let mut listed = Vec::new();
        let mut beside = Vec::new();
        let mut holding = Vec::new();
        let mut said: Vec<String> = Vec::new();
        for matcher in matchers {
            match matcher {
                Matcher::Holding(file) => {
                    holding.push(OsString::from(file));
                    said.push(format!("holding:{file}"));
                }
                Matcher::Pattern {
                    text,
                    beside: Some(sibling),
                    ..
                } => {
                    let (Ok(folder), Ok(sibling_match)) =
                        (NameMatch::of(text), NameMatch::of(sibling))
                    else {
                        continue;
                    };
                    beside.push(BesideRule {
                        folder,
                        sibling: sibling_match,
                    });
                    said.push(format!("beside:{text}:{sibling}"));
                }
                Matcher::Pattern {
                    text,
                    beside: None,
                    names,
                } => {
                    let Ok(pattern) = Pattern::of(text) else {
                        continue;
                    };
                    // What a variable puts it at is part of the rule: the
                    // rule changes as the variable does.
                    let at = match &pattern {
                        Pattern::Path(path) => path.display().to_string(),
                        Pattern::PathGlob(glob) => glob.glob().glob().to_owned(),
                        Pattern::Unset => "unset".to_owned(),
                        _ => String::new(),
                    };
                    if names.folders() {
                        folders.add(&pattern);
                    }
                    if names.files() {
                        files.add(&pattern);
                    }
                    if let Pattern::Path(path) = pattern {
                        listed.push(path);
                    }
                    let kind = names.key();
                    if variables_in(text).is_empty() {
                        said.push(format!("pattern:{kind}:{}", text.trim()));
                    } else {
                        said.push(format!("pattern:{kind}:{}={at}", text.trim()));
                    }
                }
            }
        }
        said.sort();
        said.dedup();
        let mut parts: Vec<&[u8]> = vec![b"exclusions"];
        parts.extend(said.iter().map(String::as_bytes));
        let fingerprint = stable_hash(&parts) ^ RULE_VERSION;
        Rule {
            folders: folders.made(),
            files: files.made(),
            listed,
            beside,
            holding,
            fingerprint,
        }
    }

    /// Only these folders, by their path: whatever their names hold, never
    /// read as a pattern (a test's, Privacy's of old).
    pub fn paths(paths: impl IntoIterator<Item = PathBuf>) -> Rule {
        let mut rule = Rule::default();
        let mut said: Vec<String> = Vec::new();
        for path in paths {
            said.push(format!("path:{}", path.display()));
            rule.folders.paths.insert(folded(path.as_os_str()));
            rule.files.paths.insert(folded(path.as_os_str()));
            rule.listed.push(path);
        }
        said.sort();
        let mut parts: Vec<&[u8]> = vec![b"exclusions"];
        parts.extend(said.iter().map(String::as_bytes));
        rule.fingerprint = stable_hash(&parts) ^ RULE_VERSION;
        rule
    }

    /// This, and the folders holding `file` (a test's).
    #[must_use]
    pub fn holding(mut self, file: &str) -> Rule {
        self.holding.push(OsString::from(file));
        self.fingerprint ^= stable_hash(&[b"holding", file.as_bytes()]);
        self
    }

    /// Which rule this is: what was made under another is made again.
    pub fn fingerprint(&self) -> u64 {
        self.fingerprint
    }

    /// Whether it leaves nothing out.
    pub fn is_empty(&self) -> bool {
        self.folders.is_empty()
            && self.files.is_empty()
            && self.beside.is_empty()
            && self.holding.is_empty()
    }

    /// Whether the folder `dir` is one of it by its name or its path:
    /// nothing is read.
    pub fn names(&self, dir: &Path) -> bool {
        self.folders.names(dir)
    }

    /// Whether the file `path` is one of it by its name or its path:
    /// nothing is read.
    pub fn names_file(&self, path: &Path) -> bool {
        self.files.names(path)
    }

    /// Whether it names any file: when not, a walk asks nothing of the
    /// files it finds.
    pub fn names_files(&self) -> bool {
        !self.files.is_empty()
    }

    /// Whether a name alone is a folder's of it, wherever the folder is.
    pub fn names_anywhere(&self, name: &OsStr) -> bool {
        self.folders.names_anywhere(name)
    }

    /// The beside rules a folder named `name` falls under: those asking for
    /// a sibling, which the caller looks for.
    pub fn beside(&self, name: &OsStr) -> impl Iterator<Item = &NameMatch> {
        self.beside
            .iter()
            .filter(move |b| b.folder.matches(name))
            .map(|b| &b.sibling)
    }

    /// Whether it has beside rules: a folder's name is then asked.
    pub fn has_beside(&self) -> bool {
        !self.beside.is_empty()
    }

    /// The files whose presence in a folder leaves it out.
    pub fn markers(&self) -> &[OsString] {
        &self.holding
    }

    /// The exact paths it lists, as written.
    pub fn listed(&self) -> &[PathBuf] {
        &self.listed
    }

    /// Whether `dir`, whose names are `names` (read already: a count
    /// walk's), holds one of its markers; a `CACHEDIR.TAG` read for its
    /// signature.
    pub fn marked_among(&self, dir: &Path, names: impl Fn(&OsStr) -> bool) -> bool {
        self.holding.iter().any(|marker| {
            names(marker) && (marker != CACHEDIR_TAG || tags_a_cache(&dir.join(marker)))
        })
    }

    /// Whether `dir` holds one of its markers, looked up: a lookup per
    /// marker, a `CACHEDIR.TAG` read for its signature.
    pub fn marked(&self, dir: &Path) -> bool {
        self.holding.iter().any(|marker| {
            let file = dir.join(marker);
            if marker == CACHEDIR_TAG {
                tags_a_cache(&file)
            } else {
                std::fs::symlink_metadata(&file).is_ok()
            }
        })
    }

    /// Whether a beside rule leaves out `dir`: its name falls under one,
    /// and its parent holds the sibling, looked up (a listing of the
    /// parent for a glob).
    pub fn beside_found(&self, dir: &Path) -> bool {
        let (Some(name), Some(parent)) = (dir.file_name(), dir.parent()) else {
            return false;
        };
        self.beside(name).any(|sibling| match sibling.exact() {
            Some(exact) => std::fs::symlink_metadata(parent.join(exact)).is_ok(),
            None => std::fs::read_dir(parent)
                .is_ok_and(|names| names.flatten().any(|e| sibling.matches(&e.file_name()))),
        })
    }
}

/// Where a path written in the list starts: the home folder (`~`), a
/// variable (`$NAME`, `${NAME}`, `${NAME:-what stands for it unset}`, the
/// shell's syntax, a default nesting another), or neither (a whole path,
/// a name).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Start<'a> {
    Home,
    Variable {
        name: &'a str,
        unset: Option<&'a str>,
    },
    /// `$` or `${` with no name a variable may have, or no closing brace.
    Unreadable,
    Nothing,
}

impl<'a> Start<'a> {
    /// The start of `text`, and the rest after it.
    fn of(text: &'a str) -> (Start<'a>, &'a str) {
        let named = |name: &str| {
            name.chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        };
        if let Some(rest) = text.strip_prefix('~')
            && (rest.is_empty() || rest.starts_with(['/', '\\']))
        {
            return (Start::Home, rest);
        }
        if let Some(braced) = text.strip_prefix("${") {
            // Its closing brace, past those a default nests.
            let bytes = braced.as_bytes();
            let (mut depth, mut at, mut end) = (1usize, 0, None);
            while at < bytes.len() {
                match bytes[at] {
                    b'$' if bytes.get(at + 1) == Some(&b'{') => {
                        depth += 1;
                        at += 1;
                    }
                    b'}' => {
                        depth -= 1;
                        if depth == 0 {
                            end = Some(at);
                            break;
                        }
                    }
                    _ => {}
                }
                at += 1;
            }
            let Some(end) = end else {
                return (Start::Unreadable, text);
            };
            let (name, unset) = match braced[..end].split_once(":-") {
                Some((name, unset)) => (name, Some(unset)),
                None => (&braced[..end], None),
            };
            return if named(name) {
                (Start::Variable { name, unset }, &braced[end + 1..])
            } else {
                (Start::Unreadable, text)
            };
        }
        if let Some(after) = text.strip_prefix('$') {
            let end = after
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(after.len());
            let name = &after[..end];
            return if named(name) {
                (Start::Variable { name, unset: None }, &after[end..])
            } else {
                (Start::Unreadable, text)
            };
        }
        (Start::Nothing, text)
    }
}

/// The variables `text` starts from, those its defaults name included.
pub fn variables_in(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut text = text.trim();
    while let (Start::Variable { name, unset }, _) = Start::of(text) {
        names.push(name.to_owned());
        match unset {
            Some(unset) => text = unset.trim(),
            None => break,
        }
    }
    names
}

/// A variable's value as a folder: the first of a list (`:`, `;` on
/// Windows, or `separator`), where tools write. An empty first element
/// is unset (Julia's way of asking for the defaults); `~` is the home
/// folder, as a tool that expands it reads it; anything else that is not
/// a whole path is no value, as the XDG specification says of its own.
fn variable_path(name: &str, home: Option<&Path>, separator: Option<char>) -> Option<PathBuf> {
    let value = crate::environment::var(name)?;
    let first = match separator {
        Some(separator) => PathBuf::from(
            value
                .to_string_lossy()
                .split(separator)
                .next()
                .unwrap_or_default(),
        ),
        None => std::env::split_paths(&value).next()?,
    };
    if first.as_os_str().is_empty() {
        return None;
    }
    let first = match (first.strip_prefix("~"), home) {
        (Ok(rest), Some(home)) => home.join(rest),
        _ => first,
    };
    first.is_absolute().then_some(first)
}

/// `text` as an absolute path, `~` read as `home` and a variable as the
/// environment says (`environment::var`; where it is unset, its default),
/// its parts joined as this system writes a path (`~/.cargo/registry` on
/// Windows is `C:\Users\x\.cargo\registry`, as a walk finds it). None
/// where it starts nowhere: a relative path, a variable unset with no
/// default.
pub fn expand(text: &str, home: Option<&Path>) -> Option<PathBuf> {
    expand_by(text, home, &|name| variable_path(name, home, None))
}

/// [`expand`], `variable` saying what each variable holds.
fn expand_by(
    text: &str,
    home: Option<&Path>,
    variable: &dyn Fn(&str) -> Option<PathBuf>,
) -> Option<PathBuf> {
    let text = text.trim();
    let (mut path, rest) = match Start::of(text) {
        (Start::Home, rest) => (home?.to_path_buf(), rest),
        (Start::Variable { name, unset }, rest) => {
            let base = match variable(name) {
                Some(base) => base,
                None => expand_by(unset?, home, variable)?,
            };
            (base, rest)
        }
        (Start::Unreadable, _) => return None,
        (Start::Nothing, _) => {
            // Its root, `/` or `C:\` (`C:/` as a hand writes it): one byte
            // a separator, whichever.
            let roots: Vec<Component<'_>> = Path::new(text)
                .components()
                .take_while(|c| matches!(c, Component::Prefix(_) | Component::RootDir))
                .collect();
            let written: usize = roots.iter().map(|c| c.as_os_str().len()).sum();
            (roots.iter().collect(), text.get(written..)?)
        }
    };
    for part in rest.split(['/', '\\']).filter(|part| !part.is_empty()) {
        path.push(part);
    }
    path.is_absolute().then_some(path)
}

/// The most folders a glob's lookup follows at one level ([`here`]):
/// beyond, it cannot tell cheaply.
const LOOKED_UP: usize = 256;

/// Whether the path or path glob `text` names anything on this computer:
/// a path is looked up, a glob a level at a time (one listing of each
/// folder its parts reach, never below what it names), and what is on a
/// remote is taken as there and never read (it may not answer). `None`
/// where no lookup tells: a name, a regex, a `**`, a glob that reaches
/// more than [`LOOKED_UP`] folders at a level. Run on a worker.
pub fn here(text: &str, home: Option<&Path>) -> Option<bool> {
    let written = Written::of(text);
    if !matches!(written, Written::Path | Written::PathGlob) {
        return None;
    }
    let Some(full) = expand(text, home) else {
        // A variable unset here names nothing here.
        return matches!(Start::of(text.trim()).0, Start::Variable { .. }).then_some(false);
    };
    let found = |path: &Path| {
        crate::fs::reach::of(path).is_some() || std::fs::symlink_metadata(path).is_ok()
    };
    if written == Written::Path {
        return Some(found(&full));
    }
    let mut reached: Vec<PathBuf> = vec![PathBuf::new()];
    for part in full.components() {
        let name = part.as_os_str();
        let glob = match part {
            Component::Normal(name) if is_glob(&name.to_string_lossy()) => {
                if name == "**" {
                    return None;
                }
                glob_of(&name.to_string_lossy()).ok()?
            }
            _ => {
                for path in &mut reached {
                    path.push(name);
                }
                continue;
            }
        };
        let mut next = Vec::new();
        for folder in &reached {
            if crate::fs::reach::of(folder).is_some() {
                return Some(true);
            }
            let Ok(listing) = std::fs::read_dir(folder) else {
                continue;
            };
            for entry in listing.flatten() {
                if glob.is_match(entry.file_name()) {
                    next.push(folder.join(entry.file_name()));
                }
            }
            if next.len() > LOOKED_UP {
                return None;
            }
        }
        if next.is_empty() {
            return Some(false);
        }
        reached = next;
    }
    Some(reached.iter().any(|path| found(path)))
}

/// A name or a path as the system compares them: regardless of case on a
/// Mac and on Windows, whose volumes mostly fold it.
fn folded(text: &OsStr) -> OsString {
    if cfg!(any(target_os = "macos", windows)) {
        OsString::from(text.to_string_lossy().to_lowercase())
    } else {
        text.to_owned()
    }
}

/// Whether the `CACHEDIR.TAG` at `path` begins as the convention says: a
/// file of that name and nothing more is not one.
pub fn tags_a_cache(path: &Path) -> bool {
    use std::io::Read;
    let mut head = [0; SIGNATURE.len()];
    crate::fs::file::open_regular(path)
        .and_then(|mut file| file.read_exact(&mut head))
        .is_ok_and(|()| head == SIGNATURE)
}

/// Whether `dir` holds a signed [`CACHEDIR_TAG`]: reads it.
pub fn is_cache(dir: &Path) -> bool {
    tags_a_cache(&dir.join(CACHEDIR_TAG))
}

/// 64-bit FNV-1a: the same in every build, so a fingerprint kept by one
/// is read by the next.
fn stable_hash(parts: &[&[u8]]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for part in parts {
        for byte in *part {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        hash ^= 0xff;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// Refuse to an agent's call (`permissions::agent_call`) a change of the
/// settings (`changes`, read) that would make a folder kept from agents
/// readable: only the user lowers privacy.
///
/// # Errors
/// Naming the folders, as the agent is told.
pub fn refuse_exposure(
    settings: &Settings,
    changes: &std::collections::BTreeMap<String, toml::Value>,
) -> anyhow::Result<()> {
    if crate::permissions::agent_call().is_none()
        || !changes
            .keys()
            .any(|key| key.starts_with("exclusions.") || key.starts_with("privacy."))
    {
        return Ok(());
    }
    // Off, a path only the shell's files move goes back to its default: the
    // folder it names now is named no more. Whether it lowers privacy
    // depends on what those files say, so it is the user's alone either way.
    if let Some(value) = changes.get(SHELL_VARIABLES_KEY)
        && settings.get::<toml::Value>(SHELL_VARIABLES_KEY).as_ref() != Some(value)
    {
        anyhow::bail!(
            "an agent may not change whether the login shell's variables are read: a private folder only they name would stop being private. Only you can, in Settings, Privacy"
        );
    }
    let before = Catalog::of(settings);
    let now = |key: &str| {
        changes
            .get(key)
            .cloned()
            .or_else(|| settings.get::<toml::Value>(key))
    };
    let legacy = Legacy::read(
        now(LEGACY_FOLDERS_KEY).as_ref(),
        now(LEGACY_MARKER_KEY).as_ref(),
    );
    let after = Catalog::from_values(
        now(CATEGORIES_KEY).as_ref(),
        now(ENTRIES_KEY).as_ref(),
        &legacy,
    );
    let exposed = exposed(&before, &after);
    anyhow::ensure!(
        exposed.is_empty(),
        "an agent may not make private folders readable ({}): only you can, in Settings, Exclusions",
        exposed.join(", ")
    );
    Ok(())
}

/// What a change of the two settings does to agents: the folders kept
/// from them before that no entry keeps from them now (unticked, their
/// category switched off, moved, removed, or their pattern changed), each
/// said once. A folder another entry still keeps (a copy of one of
/// skwir's, made the user's) is not one. An agent may not ask for any
/// (`settings.apply`): only the user lowers privacy.
pub fn exposed(before: &Catalog, after: &Catalog) -> Vec<String> {
    let now: Vec<&Matcher> = after.kept_from(Role::Agents).map(|e| &e.matcher).collect();
    let mut kept: HashMap<Same<'_>, Vec<Names>> = HashMap::new();
    for matcher in &now {
        let (same, names) = same_of(matcher);
        kept.entry(same).or_default().push(names);
    }
    let mut said = HashSet::new();
    before
        .kept_from(Role::Agents)
        .filter(|e| {
            let (same, names) = same_of(&e.matcher);
            !kept
                .get(&same)
                .is_some_and(|all| all.iter().any(|now| *now == names || *now == Names::Both))
        })
        .map(|e| e.matcher.said())
        .filter(|words| said.insert(words.clone()))
        .collect()
}

/// What two entries must share to exclude the same things, what each
/// names of them aside.
type Same<'a> = (&'a str, Option<&'a str>, bool);

fn same_of(matcher: &Matcher) -> (Same<'_>, Names) {
    match matcher {
        Matcher::Pattern {
            text,
            beside,
            names,
        } => ((text.trim(), beside.as_deref(), false), *names),
        Matcher::Holding(file) => ((file.trim(), None, true), Names::Folders),
    }
}
