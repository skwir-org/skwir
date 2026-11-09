//! The Exclusions page (ADR-125), as data: the exclusions' catalog
//! (`skwir_core::exclusions::Catalog`) as the page lists it for a role
//! (headings, sub-headings, rows, each with the words it says), the edits
//! the page makes, each from one catalog to the next, what a change takes
//! from agents (asked first), and which folders are on this computer. No
//! gpui: drawn by `settings_exclusions_view.rs`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use skwir_core::exclusions::{
    Catalog, Entry, Matcher, Names, Role, Switches, Written, YOURS, unique_id,
};

/// The roles the page offers: the index is not built.
pub(crate) const ROLES: [Role; 2] = [Role::Searches, Role::Agents];

/// What a role is called where the page names it.
pub(crate) fn role_name(role: Role) -> &'static str {
    match role {
        Role::Searches => "Searches and tags",
        Role::Agents => "Agents",
        Role::Index => "The index",
    }
}

// What the page calls what it lists, in one place each: the page's own
// name is its title (`settings::BUILTIN`).

/// One of the page's entries, and several.
pub(crate) const ENTRY: &str = "exclusion";
pub(crate) const ENTRIES: &str = "exclusions";
/// What the role picker says before the roles.
pub(crate) const EXCLUDED_FROM: &str = "Excluded from";

/// "excluded from searches and tags", as a count and a heading say it.
pub(crate) fn excluded_from(role: Role) -> String {
    format!(
        "{} {}",
        EXCLUDED_FROM.to_lowercase(),
        role_name(role).to_lowercase()
    )
}

/// The folder as a row writes it: its text, `~` for home; a holding rule
/// by its file.
pub(crate) fn pattern_of(matcher: &Matcher) -> String {
    match matcher {
        Matcher::Pattern { text, .. } => text.trim().to_owned(),
        Matcher::Holding(file) => format!("holding {file}"),
    }
}

/// What a matcher means, where that says more than the folder written.
pub(crate) fn meaning(matcher: &Matcher) -> Option<String> {
    let said = matcher.said();
    (said != pattern_of(matcher)).then_some(said)
}

/// Which of `texts` (paths and path globs as written) are on this
/// computer, each looked up once (`exclusions::here`: a glob a level at
/// a time, a remote's taken as there, never read); those no lookup tells
/// are left out, and show. Run on a worker.
pub(crate) fn presence(texts: &[String], home: Option<&Path>) -> Vec<(String, bool)> {
    texts
        .iter()
        .filter_map(|text| {
            skwir_core::exclusions::here(text, home).map(|here| (text.clone(), here))
        })
        .collect()
}

/// What the settings hold that cannot be read, in a line above the list:
/// the first few, and how many more. `None` where all can be.
pub(crate) fn unreadable(catalog: &Catalog) -> Option<String> {
    const SHOWN: usize = 3;
    if catalog.problems.is_empty() {
        return None;
    }
    let shown = catalog.problems[..catalog.problems.len().min(SHOWN)].join("; ");
    let more = catalog.problems.len().saturating_sub(SHOWN);
    let tail = if more > 0 {
        format!(" (and {more} more)")
    } else {
        String::new()
    };
    Some(format!("Not applied, as it cannot be read: {shown}{tail}"))
}

/// The folders whose presence the page needs: every path's and path
/// glob's.
pub(crate) fn asked_presence(catalog: &Catalog) -> HashSet<String> {
    catalog
        .entries
        .iter()
        .filter_map(|e| e.matcher.looked_up().map(str::to_owned))
        .collect()
}

/// What the page lists, as asked.
pub(crate) struct Ask<'a> {
    pub role: Role,
    /// Words that must all be in what a row says (its folder, what it is,
    /// what it means, its group or category), case ignored.
    pub filter: &'a str,
    /// Leave out the folders not on this computer: those `here` knows
    /// are not. A folder not looked up yet shows.
    pub only_here: bool,
    pub here: &'a HashMap<String, bool>,
    /// Only what differs from skwir's.
    pub changed_only: bool,
    /// Categories unfolded, by id: every other is folded, the page
    /// reading as headings, unless words are typed or Changed is on, which
    /// show what they keep unfolded.
    pub unfolded: &'a HashSet<String>,
    /// Entries shown whatever Only on this computer and Changed say, by
    /// id: what was just added, edited, moved or shown, which they would
    /// hide the moment it is made (a folder added that is not on this
    /// computer). The words typed still narrow them.
    pub revealed: &'a HashSet<String>,
    /// What each entry means, by its place in the catalog (`meanings`),
    /// worked out once a catalog: a glob's or a regex's words are read by
    /// compiling it. Without, worked out here for each entry shown.
    pub meanings: Option<&'a [Option<String>]>,
}

/// What each of `catalog`'s entries means (`meaning`), by its place.
pub(crate) fn meanings(catalog: &Catalog) -> Vec<Option<String>> {
    catalog
        .entries
        .iter()
        .map(|e| meaning(&e.matcher))
        .collect()
}

/// A row of the page's list.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Row {
    Heading(Heading),
    /// A sub-heading within a category (a language).
    Group(String),
    /// An entry, by its index in the catalog.
    Entry(Line),
    /// A category unfolded with nothing shown in it, by its id, and why:
    /// nothing in it, nothing here, nothing the filters keep.
    Empty(String, Narrowed),
}

impl Row {
    /// Where the keys stop on it: a heading or an entry.
    pub(crate) fn cursor(&self) -> Option<Cursor> {
        match self {
            Row::Heading(h) => Some(Cursor::Category(h.id.clone())),
            Row::Entry(line) => Some(Cursor::Entry(line.id.clone())),
            _ => None,
        }
    }
}

/// Where the keyboard is in the list: a category's heading or an entry.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Cursor {
    Category(String),
    Entry(String),
}

/// A category's heading.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Heading {
    pub id: String,
    pub name: String,
    pub builtin: bool,
    /// Its switch for the role.
    pub on: bool,
    pub folded: bool,
    /// Its entries, all of them.
    pub total: usize,
    /// Those the filters show.
    pub shown: usize,
    /// Those kept from the role (its switch and their tick).
    pub kept: usize,
    /// Why fewer are shown: only the folders here, or a filter too.
    pub narrowed: Narrowed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Narrowed {
    No,
    /// Only the folders on this computer.
    Here,
    /// The text or Changed.
    Found,
}

impl Heading {
    /// Its count, as the heading writes it after its name: "23 here of 92
    /// · 22 excluded", "7 of 7 excluded"; the picker above it says from
    /// what, and a screen reader hears it whole (`said`).
    pub(crate) fn count(&self) -> String {
        if self.shown == self.total {
            return format!("{} of {} excluded", self.kept, self.total);
        }
        let kept = format!("{} excluded", self.kept);
        match self.narrowed {
            Narrowed::Here => format!("{} here of {} · {kept}", self.shown, self.total),
            Narrowed::Found | Narrowed::No => {
                format!("{} found of {} · {kept}", self.shown, self.total)
            }
        }
    }

    /// What a screen reader hears of it, a button that folds and unfolds
    /// it: "Secrets, 22 of 92 excluded from searches and tags".
    pub(crate) fn said(&self, role: Role) -> String {
        let mut said = format!(
            "{}, {} of {} {}",
            self.name,
            self.kept,
            self.total,
            excluded_from(role)
        );
        if self.shown != self.total {
            let how = if self.narrowed == Narrowed::Here {
                "here"
            } else {
                "found"
            };
            said = format!("{said}, {} {how}", self.shown);
        }
        if self.folded {
            said.push_str(", folded");
        }
        said
    }
}

/// What a row says of an entry beside its folder, in words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Badge {
    /// One of skwir's, switched or moved: Reset takes it back.
    Changed,
    /// The user's own.
    Yours,
    /// One of skwir's outside the category skwir ships it in.
    Moved,
    /// A path not on this computer, shown with Only on this computer off.
    Absent,
}

impl Badge {
    pub(crate) fn words(self) -> &'static str {
        match self {
            Badge::Changed => "Changed",
            Badge::Yours => "Added by you",
            Badge::Moved => "Moved here",
            Badge::Absent => "Not on this computer",
        }
    }
}

/// An entry's row.
// What a row says of itself, each a fact of its own, not one state.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Line {
    pub id: String,
    /// Its index in the catalog's entries.
    pub at: usize,
    pub category: String,
    pub builtin: bool,
    /// Its tick for the role.
    pub tick: bool,
    /// Kept from the role: its tick and its category's switch.
    pub kept: bool,
    /// Its category is off for the role: the row keeps its tick, dimmed.
    pub dimmed: bool,
    /// The folder as written.
    pub pattern: String,
    pub what: String,
    /// What it means, where that says more than the folder.
    pub meaning: Option<String>,
    pub badges: Vec<Badge>,
    /// Where the row sits among the rows of its card.
    pub first: bool,
    pub last: bool,
}

impl Line {
    /// What a dimmed row says of itself: it keeps its tick, its category
    /// decides.
    pub(crate) fn off_words(role: Role) -> String {
        format!("Not {}: its category is off", excluded_from(role))
    }

    /// What is said under the folder: what it is, what it means.
    pub(crate) fn under(&self) -> String {
        match (&self.meaning, self.what.is_empty()) {
            (Some(meaning), false) => format!("{} · {meaning}", self.what),
            (Some(meaning), true) => meaning.clone(),
            (None, _) => self.what.clone(),
        }
    }

    /// What a screen reader hears as its description: what it is, what
    /// it means, its state words.
    pub(crate) fn description(&self, role: Role) -> String {
        let mut parts: Vec<String> = Vec::new();
        let under = self.under();
        if !under.is_empty() {
            parts.push(under);
        }
        if self.dimmed {
            parts.push(Line::off_words(role));
        }
        parts.extend(self.badges.iter().map(|b| b.words().to_owned()));
        parts.join("; ")
    }
}

/// Whether every word of `words` (lowercase) is in one of `texts`.
fn holds_all(words: &[String], texts: &[&str]) -> bool {
    words.iter().all(|word| {
        texts
            .iter()
            .any(|text| text.to_lowercase().contains(word.as_str()))
    })
}

/// Whether a category differs from skwir's: switched, or the user's.
pub(crate) fn category_changed(catalog: &Catalog, id: &str) -> bool {
    catalog
        .category(id)
        .is_some_and(|c| !c.builtin || c.shipped != Some(c.switches))
}

/// The rows for `ask`: each category in the catalog's order (skwir's,
/// then the user's), its heading, then its entries: those of no group
/// first, then each group under its sub-heading, in the order each group
/// first comes. Linear in the
/// catalog (a sort within each category).
pub(crate) fn rows(catalog: &Catalog, ask: &Ask<'_>) -> Vec<Row> {
    let words: Vec<String> = ask
        .filter
        .split_whitespace()
        .map(str::to_lowercase)
        .collect();
    let filtering = !words.is_empty();
    let mut by_category: HashMap<&str, Vec<usize>> = HashMap::new();
    for (at, entry) in catalog.entries.iter().enumerate() {
        by_category
            .entry(entry.category.as_str())
            .or_default()
            .push(at);
    }
    let mut rows = Vec::new();
    for category in &catalog.categories {
        let on = category.switches.get(ask.role);
        let members = by_category
            .get(category.id.as_str())
            .map_or(&[][..], Vec::as_slice);
        // The name typed finds the whole category.
        let named = filtering && holds_all(&words, &[&category.name]);
        // Folded, its entries are counted, not made into rows.
        let folded = !filtering && !ask.changed_only && !ask.unfolded.contains(&category.id);
        let mut counted = 0;
        let mut lines: Vec<(usize, Line)> = Vec::new();
        let mut here_narrowed = false;
        let mut found_narrowed = false;
        let mut kept = 0;
        let mut groups: HashMap<Option<&str>, usize> = HashMap::new();
        for &at in members {
            let entry = &catalog.entries[at];
            let keeps = catalog.keeps(entry, ask.role);
            kept += usize::from(keeps);
            let absent = entry
                .matcher
                .looked_up()
                .and_then(|text| ask.here.get(text))
                .is_some_and(|here| !here);
            let revealed = ask.revealed.contains(&entry.id);
            if ask.only_here && absent && !revealed {
                here_narrowed = true;
                continue;
            }
            if ask.changed_only && !entry.changed() && !revealed {
                found_narrowed = true;
                continue;
            }
            if folded {
                counted += 1;
                continue;
            }
            let shown_as = pattern_of(&entry.matcher);
            let meaning = match ask.meanings {
                Some(meanings) => meanings.get(at).cloned().flatten(),
                None => meaning(&entry.matcher),
            };
            if filtering && !named {
                let mut texts = vec![shown_as.as_str(), entry.what.as_str()];
                texts.extend(meaning.as_deref());
                texts.extend(entry.group.as_deref());
                if let Matcher::Pattern {
                    beside: Some(sibling),
                    ..
                } = &entry.matcher
                {
                    texts.push(sibling);
                }
                if !holds_all(&words, &texts) {
                    found_narrowed = true;
                    continue;
                }
            }
            let mut badges = Vec::new();
            if !entry.builtin {
                badges.push(Badge::Yours);
            } else if entry.changed() {
                badges.push(Badge::Changed);
            }
            if entry.builtin && entry.shipped_category.as_deref() != Some(entry.category.as_str()) {
                badges.push(Badge::Moved);
            }
            if absent {
                badges.push(Badge::Absent);
            }
            // The ungrouped first, then each group in the order it first
            // comes, however the list interleaves them.
            let next = groups.len() + 1;
            let group = match entry.group.as_deref() {
                None => 0,
                Some(name) => *groups.entry(Some(name)).or_insert(next),
            };
            lines.push((
                group,
                Line {
                    id: entry.id.clone(),
                    at,
                    category: category.id.clone(),
                    builtin: entry.builtin,
                    tick: entry.ticks.get(ask.role),
                    kept: keeps,
                    dimmed: !on,
                    pattern: shown_as,
                    what: entry.what.clone(),
                    meaning,
                    badges,
                    first: false,
                    last: false,
                },
            ));
        }
        let shown = lines.len() + counted;
        let changed = category_changed(catalog, &category.id);
        // A category the filters find nothing in is left out; with no
        // filter, every one shows, an empty one too.
        let listed = shown > 0
            || if filtering {
                named
            } else {
                !ask.changed_only || changed
            };
        if !listed {
            continue;
        }
        rows.push(Row::Heading(Heading {
            id: category.id.clone(),
            name: category.name.clone(),
            builtin: category.builtin,
            on,
            folded,
            total: members.len(),
            shown,
            kept,
            narrowed: if found_narrowed {
                Narrowed::Found
            } else if here_narrowed {
                Narrowed::Here
            } else {
                Narrowed::No
            },
        }));
        if folded {
            continue;
        }
        if lines.is_empty() {
            let why = if found_narrowed {
                Narrowed::Found
            } else if here_narrowed {
                Narrowed::Here
            } else {
                Narrowed::No
            };
            rows.push(Row::Empty(category.id.clone(), why));
            continue;
        }
        // The ungrouped, then each group together: stable.
        lines.sort_by_key(|(group, _)| *group);
        let their_groups: Vec<Option<String>> = lines
            .iter()
            .map(|(_, line)| catalog.entries[line.at].group.clone())
            .collect();
        let mut previous: Option<&Option<String>> = None;
        let start = rows.len();
        for ((_, line), group) in lines.into_iter().zip(&their_groups) {
            if previous != Some(group) {
                if let Some(name) = group {
                    rows.push(Row::Group(name.clone()));
                }
                previous = Some(group);
            }
            rows.push(Row::Entry(line));
        }
        mark_edges(&mut rows[start..]);
    }
    rows
}

/// Each run of entries between sub-headings is a card: its first and
/// last rows say so.
fn mark_edges(rows: &mut [Row]) {
    let len = rows.len();
    for at in 0..len {
        let before = at.checked_sub(1).map(|b| &rows[b]);
        let first = !matches!(before, Some(Row::Entry(_)));
        let last = !matches!(rows.get(at + 1), Some(Row::Entry(_)));
        if let Row::Entry(line) = &mut rows[at] {
            line.first = first;
            line.last = last;
        }
    }
}

// ------------------------------------------------------------ edits

/// What the Add and Edit dialog holds.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Draft {
    pub kind: Kind,
    /// A name, a path, a glob or `regex:`; a name for `Beside`.
    pub pattern: String,
    /// What a `Pattern` names; a `Beside` and a `Holding` name folders.
    pub names: Names,
    /// The sibling a `Beside` asks for.
    pub beside: String,
    /// The file a `Holding` asks for.
    pub holding: String,
    pub what: String,
    pub category: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A name, a path, a glob or a regex.
    #[default]
    Pattern,
    /// A name, only where its folder holds a sibling.
    Beside,
    /// Any folder holding a file.
    Holding,
}

impl Draft {
    /// The draft of an entry as it stands, to edit it.
    pub(crate) fn of(entry: &Entry) -> Draft {
        let none = String::new;
        let (kind, pattern, beside, holding, names) = match &entry.matcher {
            Matcher::Pattern {
                text,
                beside: None,
                names,
            } => (Kind::Pattern, text.clone(), none(), none(), *names),
            Matcher::Pattern {
                text,
                beside: Some(sibling),
                ..
            } => (
                Kind::Beside,
                text.clone(),
                sibling.clone(),
                none(),
                Names::Folders,
            ),
            Matcher::Holding(file) => (Kind::Holding, none(), none(), file.clone(), Names::Folders),
        };
        Draft {
            kind,
            pattern,
            names,
            beside,
            holding,
            what: entry.what.clone(),
            category: entry.category.clone(),
        }
    }

    /// Its matcher, or what is missing, said under the field that lacks
    /// it. What the rules cannot read is said by their check
    /// (`problem`).
    pub(crate) fn matcher(&self) -> Result<Matcher, (Field, String)> {
        let folder = self.pattern.trim();
        match self.kind {
            Kind::Pattern | Kind::Beside if folder.is_empty() => Err((
                Field::Pattern,
                "Say what it names: a name, a path from ~ or /, a glob or regex:…".to_owned(),
            )),
            Kind::Pattern => Ok(Matcher::Pattern {
                text: folder.to_owned(),
                beside: None,
                names: self.names,
            }),
            Kind::Beside => {
                let beside = self.beside.trim();
                if beside.is_empty() {
                    return Err((
                        Field::Beside,
                        "Say which file or folder it is beside: Cargo.toml, *.csproj".to_owned(),
                    ));
                }
                if !matches!(Written::of(folder), Written::Name | Written::NameGlob) {
                    return Err((
                        Field::Pattern,
                        "Beside a file, give a name (target), not a path".to_owned(),
                    ));
                }
                Ok(Matcher::Pattern {
                    text: folder.to_owned(),
                    beside: Some(beside.to_owned()),
                    names: Names::Folders,
                })
            }
            Kind::Holding => {
                let file = self.holding.trim();
                if file.is_empty() {
                    return Err((
                        Field::Holding,
                        "Say which file a folder holds: .nosearch, CACHEDIR.TAG".to_owned(),
                    ));
                }
                Ok(Matcher::Holding(file.to_owned()))
            }
        }
    }
}

/// A field of the Add and Edit dialog, where its problem is said.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Field {
    Pattern,
    Beside,
    Holding,
    What,
}

/// The same folders, however written: the same text, or for one folder
/// the same path (`~/x` and its whole path).
pub(crate) fn same_pattern(a: &Matcher, b: &Matcher, home: Option<&Path>) -> bool {
    if a == b {
        return true;
    }
    match (a.one_path(), b.one_path()) {
        (Some(a), Some(b)) => {
            let path = |text: &str| -> Option<PathBuf> {
                let path = skwir_core::exclusions::expand(text, home)?;
                // `~/x/` is `~/x`.
                Some(path.components().collect())
            };
            path(a).is_some_and(|a| Some(a) == path(b))
        }
        _ => false,
    }
}

/// The entry already naming these folders, other than `except` (the one
/// being edited): Add offers to show it instead of a second.
pub(crate) fn already<'a>(
    catalog: &'a Catalog,
    matcher: &Matcher,
    except: Option<&str>,
    home: Option<&Path>,
) -> Option<&'a Entry> {
    catalog
        .entries
        .iter()
        .find(|e| Some(e.id.as_str()) != except && same_pattern(&e.matcher, matcher, home))
}

/// What the rules' own check says of the folders of the entry `id` in
/// `catalog`, if anything (`Matcher::check`).
pub(crate) fn problem(catalog: &Catalog, id: &str) -> Option<String> {
    catalog
        .entries
        .iter()
        .find(|e| e.id == id)?
        .matcher
        .check()
        .err()
}

fn taken(catalog: &Catalog) -> HashSet<String> {
    catalog.entry_ids()
}

fn entry_mut<'a>(catalog: &'a mut Catalog, id: &str) -> Option<&'a mut Entry> {
    catalog.entries.iter_mut().find(|e| e.id == id)
}

/// The entry's tick for `role`, `on`.
pub(crate) fn ticked(catalog: &Catalog, id: &str, role: Role, on: bool) -> Catalog {
    let mut next = catalog.clone();
    if let Some(entry) = entry_mut(&mut next, id) {
        entry.ticks = entry.ticks.with(role, on);
    }
    next
}

/// The category's switch for `role`, `on`.
pub(crate) fn switched(catalog: &Catalog, category: &str, role: Role, on: bool) -> Catalog {
    let mut next = catalog.clone();
    if let Some(c) = next.categories.iter_mut().find(|c| c.id == category) {
        c.switches = c.switches.with(role, on);
    }
    next
}

/// The entry in another category, skwir's or the user's.
pub(crate) fn moved(catalog: &Catalog, id: &str, category: &str) -> Catalog {
    let mut next = catalog.clone();
    if catalog.category(category).is_some()
        && let Some(entry) = entry_mut(&mut next, id)
    {
        category.clone_into(&mut entry.category);
    }
    next
}

/// One of skwir's as it ships: its ticks and its category.
pub(crate) fn reset(catalog: &Catalog, id: &str) -> Catalog {
    let mut next = catalog.clone();
    if let Some(entry) = entry_mut(&mut next, id).filter(|e| e.builtin) {
        entry.ticks = Switches::ALL;
        if let Some(shipped) = entry.shipped_category.clone() {
            entry.category = shipped;
        }
    }
    next
}

/// A copy of the entry, the user's, in its category, with its ticks; one of
/// skwir's is unticked, so the copy, which can be edited, stands for it.
/// The copy's id.
pub(crate) fn duplicated(catalog: &Catalog, id: &str) -> (Catalog, Option<String>) {
    let mut next = catalog.clone();
    let Some(at) = next.entries.iter().position(|e| e.id == id) else {
        return (next, None);
    };
    let original = next.entries[at].clone();
    let new_id = unique_id(
        if original.what.is_empty() {
            &original.id
        } else {
            &original.what
        },
        &taken(catalog),
    );
    let copy = Entry {
        id: new_id.clone(),
        builtin: false,
        category: original.category.clone(),
        shipped_category: None,
        what: original.what.clone(),
        group: None,
        matcher: original.matcher.clone(),
        ticks: original.ticks,
    };
    if original.builtin {
        next.entries[at].ticks = Switches::NONE;
    }
    // At the end, where the settings, read again, put the user's entries.
    next.entries.push(copy);
    (next, Some(new_id))
}

/// The ticks a new entry gets from the page's role: that role on, the
/// others off; one kept from agents is kept from the index too, as
/// skwir's secrets are.
pub(crate) fn ticks_for(role: Role) -> Switches {
    match role {
        Role::Agents => Switches::NONE
            .with(Role::Agents, true)
            .with(Role::Index, true),
        role => Switches::NONE.with(role, true),
    }
}

/// The user's entry `draft` makes, added at the end of its category, its
/// ticks the role's (`ticks_for`). Its id is made from what it is, else
/// from its folder.
pub(crate) fn added(
    catalog: &Catalog,
    matcher: Matcher,
    draft: &Draft,
    role: Role,
) -> (Catalog, String) {
    let mut next = catalog.clone();
    let what = draft.what.trim();
    let folder = pattern_of(&matcher);
    let id = unique_id(
        if what.is_empty() { &folder } else { what },
        &taken(catalog),
    );
    let category = if catalog.category(&draft.category).is_some() {
        draft.category.clone()
    } else {
        YOURS.to_owned()
    };
    // At the end of its category's entries, before the next category's.
    let at = next
        .entries
        .iter()
        .rposition(|e| e.category == category)
        .map_or(next.entries.len(), |at| at + 1);
    next.entries.insert(
        at,
        Entry {
            id: id.clone(),
            builtin: false,
            category,
            shipped_category: None,
            what: what.to_owned(),
            group: None,
            matcher,
            ticks: ticks_for(role),
        },
    );
    (next, id)
}

/// The user's entry `id` as `draft` says: its folders, what it is, its
/// category; its ticks stay.
pub(crate) fn edited(catalog: &Catalog, id: &str, matcher: Matcher, draft: &Draft) -> Catalog {
    let mut next = catalog.clone();
    let category = catalog.category(&draft.category).map(|c| c.id.clone());
    if let Some(entry) = entry_mut(&mut next, id).filter(|e| !e.builtin) {
        entry.matcher = matcher;
        draft.what.trim().clone_into(&mut entry.what);
        if let Some(category) = category {
            entry.category = category;
        }
    }
    next
}

/// Without the user's entry `id`; skwir's are unticked, never removed.
pub(crate) fn removed(catalog: &Catalog, id: &str) -> Catalog {
    let mut next = catalog.clone();
    next.entries.retain(|e| e.builtin || e.id != id);
    next
}

/// A category of the user's, kept from every role its entries are ticked
/// for. Its id.
pub(crate) fn new_category(catalog: &Catalog, name: &str) -> (Catalog, String) {
    let mut next = catalog.clone();
    let id = unique_id(name, &catalog.category_ids());
    next.categories.push(skwir_core::exclusions::Category {
        id: id.clone(),
        name: name.trim().to_owned(),
        builtin: false,
        switches: Switches::ALL,
        shipped: None,
    });
    (next, id)
}

/// The user's category `id`, named `name`; skwir's keep theirs.
pub(crate) fn renamed(catalog: &Catalog, id: &str, name: &str) -> Catalog {
    let mut next = catalog.clone();
    if let Some(c) = next
        .categories
        .iter_mut()
        .find(|c| c.id == id && !c.builtin)
    {
        name.trim().clone_into(&mut c.name);
    }
    next
}

/// Without the user's category `id`: its entries go back, skwir's to the
/// category it ships them in, the user's to Yours.
pub(crate) fn deleted_category(catalog: &Catalog, id: &str) -> Catalog {
    let mut next = catalog.clone();
    if !next.categories.iter().any(|c| c.id == id && !c.builtin) {
        return next;
    }
    next.categories.retain(|c| c.id != id);
    for entry in next.entries.iter_mut().filter(|e| e.category == id) {
        entry.category = match &entry.shipped_category {
            Some(shipped) if entry.builtin => shipped.clone(),
            _ => YOURS.to_owned(),
        };
    }
    next
}

/// The folders a change takes from agents, asked before saving: those
/// no entry keeps from them any more (`exclusions::exposed`, which an
/// agent's `settings.apply` is refused by). Making something private
/// never asks.
pub(crate) use skwir_core::exclusions::exposed as exposure;

/// "Agents will be able to read ~/.ssh": the question a change asks,
/// naming a few of what it lets them read (files or folders).
pub(crate) fn exposure_question(named: &[String]) -> String {
    match named {
        [] => String::new(),
        [one] => format!("Agents will be able to read {one}."),
        [a, b] => format!("Agents will be able to read {a} and {b}."),
        [a, b, rest @ ..] => format!(
            "Agents will be able to read {a}, {b} and {} more.",
            rest.len()
        ),
    }
}

// ------------------------------------------------------------ words

/// What a tick changed, said after the change: what it does, or, while
/// its category is off for the role, that the tick waits for it.
pub(crate) fn said_tick(folder: &str, role: Role, on: bool, category_on: bool) -> String {
    if !category_on {
        let ticked = if on { "Ticked" } else { "Unticked" };
        return format!(
            "{ticked} {folder}; its category is off for {}",
            role_name(role).to_lowercase()
        );
    }
    if on {
        format!("{folder} is {}", excluded_from(role))
    } else {
        format!("{folder} is no longer {}", excluded_from(role))
    }
}

/// What a category's switch changed, said after the change.
pub(crate) fn said_switch(name: &str, role: Role, on: bool) -> String {
    if on {
        format!("{name}: {}", excluded_from(role))
    } else {
        format!("{name}: no longer {}", excluded_from(role))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skwir_core::exclusions::{Category, Legacy};

    fn shipped() -> Catalog {
        Catalog::shipped()
    }

    fn entry<'a>(catalog: &'a Catalog, text: &str) -> &'a Entry {
        catalog
            .entries
            .iter()
            .find(|e| matches!(&e.matcher, Matcher::Pattern { text: t, beside: None, .. } if t == text))
            .unwrap_or_else(|| panic!("{text} listed"))
    }

    /// Every category of `catalog` unfolded.
    fn open(catalog: &Catalog) -> HashSet<String> {
        catalog.categories.iter().map(|c| c.id.clone()).collect()
    }

    fn ask<'a>(
        role: Role,
        filter: &'a str,
        here: &'a HashMap<String, bool>,
        unfolded: &'a HashSet<String>,
    ) -> Ask<'a> {
        static NONE: std::sync::LazyLock<HashSet<String>> = std::sync::LazyLock::new(HashSet::new);
        Ask {
            role,
            filter,
            only_here: false,
            here,
            changed_only: false,
            unfolded,
            revealed: &NONE,
            meanings: None,
        }
    }

    fn line<'a>(rows: &'a [Row], id: &str) -> Option<&'a Line> {
        rows.iter().find_map(|row| match row {
            Row::Entry(line) if line.id == id => Some(line),
            _ => None,
        })
    }

    fn heading<'a>(rows: &'a [Row], id: &str) -> Option<&'a Heading> {
        rows.iter().find_map(|row| match row {
            Row::Heading(h) if h.id == id => Some(h),
            _ => None,
        })
    }

    /// Every category heads its entries, in the catalog's order, and
    /// every entry is listed once with nothing filtering.
    #[test]
    fn every_category_heads_its_entries() {
        let catalog = shipped();
        let (here, unfolded) = (HashMap::new(), open(&catalog));
        let rows = rows(&catalog, &ask(Role::Searches, "", &here, &unfolded));
        let headings: Vec<&str> = rows
            .iter()
            .filter_map(|r| match r {
                Row::Heading(h) => Some(h.id.as_str()),
                _ => None,
            })
            .collect();
        let order: Vec<&str> = catalog.categories.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(headings, order);
        let listed = rows.iter().filter(|r| matches!(r, Row::Entry(_))).count();
        assert_eq!(listed, catalog.entries.len());
        // An entry stands under its own category's heading.
        let mut under = "";
        for row in &rows {
            match row {
                Row::Heading(h) => under = &h.id,
                Row::Entry(line) => assert_eq!(line.category, under),
                _ => {}
            }
        }
        // Toolchains by language: each group once.
        let mut groups: Vec<&str> = rows
            .iter()
            .filter_map(|r| match r {
                Row::Group(g) => Some(g.as_str()),
                _ => None,
            })
            .collect();
        let count = groups.len();
        groups.sort_unstable();
        groups.dedup();
        assert_eq!(groups.len(), count, "a group's entries together");
        assert!(groups.contains(&"Rust"));
        // In a category, the entries of no group come before its first
        // sub-heading: none follows one.
        let mut grouped = false;
        for row in &rows {
            match row {
                Row::Heading(_) => grouped = false,
                Row::Group(_) => grouped = true,
                Row::Entry(line) => assert!(
                    !grouped || catalog.entries[line.at].group.is_some(),
                    "{} after a sub-heading",
                    line.pattern
                ),
                Row::Empty(..) => {}
            }
        }
    }

    /// What each entry means, worked out once for the catalog, lists the
    /// same rows as worked out for each.
    #[test]
    fn what_cannot_be_read_is_said_and_stays_through_a_tick() {
        let entries = toml::Value::Table(
            toml::from_str(r#"broken = { pattern = "~/x", colour = "red" }"#).unwrap(),
        );
        let catalog = Catalog::from_values(None, Some(&entries), &Legacy::default());
        let said = unreadable(&catalog).expect("said");
        assert!(said.contains("broken"), "{said}");
        let ssh = catalog
            .entries
            .iter()
            .find(|e| matches!(&e.matcher, Matcher::Pattern { text, .. } if text == "~/.ssh"))
            .unwrap();
        let after = ticked(&catalog, &ssh.id, Role::Searches, false);
        let written = after.values().1;
        assert_eq!(written.get("broken"), entries.get("broken"));
        assert!(taken(&after).contains("broken"), "its id stays taken");
        assert!(unreadable(&Catalog::shipped()).is_none());
    }

    #[test]
    fn meanings_worked_out_once_say_the_same() {
        let catalog = shipped();
        let (here, unfolded) = (HashMap::new(), open(&catalog));
        let once = meanings(&catalog);
        for filter in ["", "any folder named", "cargo"] {
            let mut asked = ask(Role::Searches, filter, &here, &unfolded);
            let each = rows(&catalog, &asked);
            asked.meanings = Some(&once);
            assert_eq!(rows(&catalog, &asked), each, "{filter}");
        }
    }

    #[test]
    fn a_folded_category_shows_its_heading_alone() {
        let catalog = shipped();
        let here = HashMap::new();
        let mut unfolded = open(&catalog);
        unfolded.remove("skwir.secrets");
        let rows = rows(&catalog, &ask(Role::Agents, "", &here, &unfolded));
        let ssh = &entry(&catalog, "~/.ssh").id;
        assert!(line(&rows, ssh).is_none());
        let secrets = heading(&rows, "skwir.secrets").unwrap();
        assert!(secrets.folded);
        assert!(secrets.kept > 0, "counted folded or not");
        // A filter typed shows what it finds, folded or not.
        let rows = super::rows(&catalog, &ask(Role::Agents, ".ssh", &here, &unfolded));
        assert!(line(&rows, ssh).is_some());
    }

    /// The page opens as headings: every category folded but those
    /// unfolded, all of them while words are typed or Changed is on.
    #[test]
    fn categories_are_folded_unless_unfolded_found_or_changed() {
        let catalog = shipped();
        let here = HashMap::new();
        let none = HashSet::new();
        let rows = rows(&catalog, &ask(Role::Searches, "", &here, &none));
        assert!(
            rows.iter()
                .all(|r| matches!(r, Row::Heading(h) if h.folded))
        );
        assert_eq!(rows.len(), catalog.categories.len());
        let secrets = HashSet::from(["skwir.secrets".to_owned()]);
        let rows = super::rows(&catalog, &ask(Role::Agents, "", &here, &secrets));
        let ssh = &entry(&catalog, "~/.ssh").id;
        assert!(line(&rows, ssh).is_some());
        assert!(heading(&rows, "skwir.toolchains").unwrap().folded);
        // Changed on: what holds changes shows unfolded.
        let changed = ticked(&catalog, ssh, Role::Agents, false);
        let mut asked = ask(Role::Agents, "", &here, &none);
        asked.changed_only = true;
        let rows = super::rows(&changed, &asked);
        assert!(!heading(&rows, "skwir.secrets").unwrap().folded);
        assert!(line(&rows, ssh).is_some());
    }

    #[test]
    fn the_filter_finds_folders_and_what_they_are() {
        let catalog = shipped();
        let (here, unfolded) = (HashMap::new(), open(&catalog));
        let rows = rows(
            &catalog,
            &ask(Role::Searches, "cargo REGISTRY", &here, &unfolded),
        );
        let found: Vec<&Line> = rows
            .iter()
            .filter_map(|r| match r {
                Row::Entry(l) => Some(l),
                _ => None,
            })
            .collect();
        assert!(found.iter().any(|l| l.pattern == "~/.cargo/registry"));
        assert!(found.iter().all(|l| {
            let all = format!("{} {}", l.pattern, l.under()).to_lowercase();
            all.contains("cargo") && all.contains("registry")
        }));
        // Only the categories it finds something in.
        let toolchains = heading(&rows, "skwir.toolchains").unwrap();
        assert_eq!(toolchains.narrowed, Narrowed::Found);
        assert!(heading(&rows, "skwir.games").is_none());
        assert!(toolchains.count().contains("found of"));
        // A category's name finds it whole.
        let rows = super::rows(&catalog, &ask(Role::Searches, "secrets", &here, &unfolded));
        let secrets = heading(&rows, "skwir.secrets").unwrap();
        assert_eq!(secrets.shown, secrets.total);
        // What it means: a name is any folder so named.
        let rows = super::rows(
            &catalog,
            &ask(Role::Searches, "any folder named", &here, &unfolded),
        );
        assert!(
            rows.iter()
                .any(|r| matches!(r, Row::Entry(l) if l.pattern == "node_modules"))
        );
    }

    #[test]
    fn only_on_this_computer_hides_paths_not_here_and_never_names() {
        let catalog = shipped();
        let ssh = entry(&catalog, "~/.ssh").id.clone();
        let mut here: HashMap<String, bool> = asked_presence(&catalog)
            .into_iter()
            .map(|t| (t, false))
            .collect();
        here.insert("~/.ssh".into(), true);
        let unfolded = open(&catalog);
        let mut asked = ask(Role::Agents, "", &here, &unfolded);
        asked.only_here = true;
        let rows = rows(&catalog, &asked);
        assert!(line(&rows, &ssh).is_some());
        assert!(
            !rows
                .iter()
                .any(|r| matches!(r, Row::Entry(l) if l.pattern == "~/.gnupg")),
            "not here"
        );
        // Names, beside and holding rules always show.
        assert!(
            rows.iter()
                .any(|r| matches!(r, Row::Entry(l) if l.pattern == "node_modules"))
        );
        assert!(
            rows.iter()
                .any(|r| matches!(r, Row::Entry(l) if l.pattern == "holding .private"))
        );
        let secrets = heading(&rows, "skwir.secrets").unwrap();
        assert_eq!(secrets.narrowed, Narrowed::Here);
        assert!(secrets.count().contains(" here of "), "{}", secrets.count());
        // Off, they show, saying so.
        asked.only_here = false;
        let rows = super::rows(&catalog, &asked);
        let gnupg = rows
            .iter()
            .find_map(|r| match r {
                Row::Entry(l) if l.pattern == "~/.gnupg" => Some(l),
                _ => None,
            })
            .unwrap();
        assert!(gnupg.badges.contains(&Badge::Absent));
        assert!(
            gnupg
                .description(Role::Agents)
                .contains("Not on this computer")
        );
        // Not looked up yet: shown.
        let unknown = HashMap::new();
        let mut asked = ask(Role::Agents, "", &unknown, &unfolded);
        asked.only_here = true;
        let rows = super::rows(&catalog, &asked);
        assert!(
            rows.iter()
                .any(|r| matches!(r, Row::Entry(l) if l.pattern == "~/.gnupg"))
        );
    }

    /// A folder just added that is not here stays in view, its category
    /// saying why it would be empty otherwise.
    #[test]
    fn what_was_just_added_shows_whatever_the_filters_say() {
        let draft = Draft {
            pattern: "~/Gone".into(),
            ..Draft::default()
        };
        let (catalog, id) = added(&shipped(), draft.matcher().unwrap(), &draft, Role::Agents);
        let here = HashMap::from([("~/Gone".to_owned(), false)]);
        let unfolded = open(&catalog);
        let mut asked = ask(Role::Agents, "", &here, &unfolded);
        asked.only_here = true;
        let rows = rows(&catalog, &asked);
        assert!(line(&rows, &id).is_none());
        assert!(rows.contains(&Row::Empty(YOURS.to_owned(), Narrowed::Here)));
        let revealed = HashSet::from([id.clone()]);
        asked.revealed = &revealed;
        asked.changed_only = true;
        let rows = super::rows(&catalog, &asked);
        let shown = line(&rows, &id).expect("shown");
        assert!(shown.badges.contains(&Badge::Absent));
        // What is typed still narrows it.
        asked.filter = "nothing like it";
        assert!(line(&super::rows(&catalog, &asked), &id).is_none());
    }

    #[test]
    fn changed_shows_only_what_differs_from_skwirs() {
        let catalog = shipped();
        let ssh = entry(&catalog, "~/.ssh").id.clone();
        let (here, unfolded) = (HashMap::new(), open(&catalog));
        let mut asked = ask(Role::Agents, "", &here, &unfolded);
        asked.changed_only = true;
        assert!(rows(&catalog, &asked).is_empty(), "nothing changed yet");
        let catalog = ticked(&catalog, &ssh, Role::Agents, false);
        let (catalog, mine) = added(
            &catalog,
            Matcher::Pattern {
                text: "~/VMs".into(),
                beside: None,
                names: Names::Folders,
            },
            &Draft {
                what: "Virtual machines".into(),
                ..Draft::default()
            },
            Role::Searches,
        );
        let rows = rows(&catalog, &asked);
        let entries: Vec<&Line> = rows
            .iter()
            .filter_map(|r| match r {
                Row::Entry(l) => Some(l),
                _ => None,
            })
            .collect();
        assert_eq!(entries.len(), 2);
        let ssh = line(&rows, &ssh).unwrap();
        assert_eq!(ssh.badges, [Badge::Changed]);
        assert!(!ssh.tick);
        let mine = line(&rows, &mine).unwrap();
        assert_eq!(mine.badges, [Badge::Yours]);
        assert_eq!(mine.category, YOURS);
    }

    /// A row whose category is off for the role keeps its tick, dimmed,
    /// and says why in words.
    #[test]
    fn a_row_under_a_switch_that_is_off_keeps_its_tick_and_says_so() {
        let catalog = shipped();
        let (here, unfolded) = (HashMap::new(), open(&catalog));
        // Toolchains are not kept from agents: their switch is off there.
        let rows = rows(&catalog, &ask(Role::Agents, "", &here, &unfolded));
        let rustup = entry(&catalog, "~/.rustup").id.clone();
        let rustup = line(&rows, &rustup).unwrap();
        assert!(rustup.tick && rustup.dimmed && !rustup.kept);
        assert!(
            rustup
                .description(Role::Agents)
                .contains("Not excluded from agents: its category is off")
        );
        let toolchains = heading(&rows, "skwir.toolchains").unwrap();
        assert!(!toolchains.on);
        assert_eq!(toolchains.kept, 0);
        assert!(
            toolchains
                .said(Role::Agents)
                .starts_with("Toolchains and packages, 0 of ")
        );
        // Secrets are not kept from searches.
        let rows = super::rows(&catalog, &ask(Role::Searches, "", &here, &unfolded));
        let ssh = entry(&catalog, "~/.ssh").id.clone();
        let ssh = line(&rows, &ssh).unwrap();
        assert!(ssh.dimmed);
        assert!(
            ssh.description(Role::Searches)
                .contains("Not excluded from searches and tags: its category is off")
        );
    }

    #[test]
    fn a_heading_counts_what_its_role_keeps() {
        let catalog = shipped();
        let (here, unfolded) = (HashMap::new(), open(&catalog));
        let rows = rows(&catalog, &ask(Role::Agents, "", &here, &unfolded));
        let secrets = heading(&rows, "skwir.secrets").unwrap();
        assert!(secrets.on);
        assert_eq!(secrets.kept, secrets.total);
        assert_eq!(
            secrets.count(),
            format!("{0} of {0} excluded", secrets.total)
        );
        let ssh = entry(&catalog, "~/.ssh").id.clone();
        let fewer = ticked(&catalog, &ssh, Role::Agents, false);
        let rows = super::rows(&fewer, &ask(Role::Agents, "", &here, &unfolded));
        assert_eq!(
            heading(&rows, "skwir.secrets").unwrap().kept,
            secrets.total - 1
        );
    }

    #[test]
    fn ticking_and_switching_change_one_role() {
        let catalog = shipped();
        let ssh = entry(&catalog, "~/.ssh").id.clone();
        let off = ticked(&catalog, &ssh, Role::Agents, false);
        let e = entry(&off, "~/.ssh");
        assert!(!e.ticks.get(Role::Agents) && e.ticks.get(Role::Searches));
        assert!(!off.keeps(e, Role::Agents));
        let on = switched(&catalog, "skwir.secrets", Role::Searches, true);
        assert!(on.keeps(entry(&on, "~/.ssh"), Role::Searches));
        assert!(on.keeps(entry(&on, "~/.ssh"), Role::Agents));
        // Stored as what changed.
        let (categories, entries) = on.values();
        assert!(categories.get("skwir.secrets").is_some());
        assert!(entries.as_table().unwrap().is_empty());
    }

    #[test]
    fn a_built_in_moves_and_resets_and_never_is_removed() {
        let catalog = shipped();
        let ssh = entry(&catalog, "~/.ssh").id.clone();
        let (catalog, work) = new_category(&catalog, "Work things");
        assert_eq!(work, "work-things");
        let moved = moved(&catalog, &ssh, &work);
        let e = entry(&moved, "~/.ssh");
        assert_eq!(e.category, work);
        assert!(e.changed());
        let (here, unfolded) = (HashMap::new(), open(&catalog));
        let rows = rows(&moved, &ask(Role::Agents, "", &here, &unfolded));
        let l = line(&rows, &ssh).unwrap();
        assert_eq!(l.badges, [Badge::Changed, Badge::Moved]);
        // Moved nowhere: a category that is not there.
        assert_eq!(
            entry(&super::moved(&catalog, &ssh, "nowhere"), "~/.ssh").category,
            "skwir.secrets"
        );
        let unticked = ticked(&moved, &ssh, Role::Searches, false);
        let back = reset(&unticked, &ssh);
        let e = entry(&back, "~/.ssh");
        assert_eq!(e.category, "skwir.secrets");
        assert_eq!(e.ticks, Switches::ALL);
        assert!(!e.changed());
        let kept = removed(&back, &ssh);
        assert!(kept.entries.iter().any(|e| e.id == ssh), "skwir's stay");
    }

    #[test]
    fn a_duplicate_is_yours_and_stands_for_skwirs() {
        let catalog = shipped();
        let ssh = entry(&catalog, "~/.ssh").id.clone();
        let (copied, id) = duplicated(&catalog, &ssh);
        let id = id.unwrap();
        assert!(!id.contains('.'), "{id}");
        let copy = copied.entries.iter().find(|e| e.id == id).unwrap();
        assert!(!copy.builtin);
        assert_eq!(copy.category, "skwir.secrets");
        assert!(copied.keeps(copy, Role::Agents));
        let original = copied.entries.iter().find(|e| e.id == ssh).unwrap();
        assert_eq!(original.ticks, Switches::NONE, "skwir's unticked");
        // Still kept from agents under the copy: nothing asked.
        assert!(exposure(&catalog, &copied).is_empty());
        // The user's own, copied, stays as it was.
        let (copied_again, again) = duplicated(&copied, &id);
        let again = again.unwrap();
        assert_ne!(again, id);
        assert!(copied_again.keeps(
            copied_again.entries.iter().find(|e| e.id == id).unwrap(),
            Role::Agents
        ));
    }

    #[test]
    fn yours_are_added_edited_and_removed_with_ids_without_dots() {
        let catalog = shipped();
        let draft = Draft {
            pattern: "~/My VMs.d".into(),
            what: "Virtual machines (big)".into(),
            category: "skwir.secrets".into(),
            ..Draft::default()
        };
        let matcher = draft.matcher().unwrap();
        let (catalog, id) = added(&catalog, matcher, &draft, Role::Agents);
        assert_eq!(id, "virtual-machines-big");
        let e = catalog.entries.iter().find(|e| e.id == id).unwrap();
        assert_eq!(e.category, "skwir.secrets");
        assert!(catalog.keeps(e, Role::Agents) && !e.ticks.get(Role::Searches));
        assert!(
            e.ticks.get(Role::Index),
            "a private folder is kept from the index too"
        );
        // Placed after its category's others.
        let at = catalog.entries.iter().position(|e| e.id == id).unwrap();
        assert_eq!(catalog.entries[at - 1].category, "skwir.secrets");
        // Another with the same words gets an id of its own.
        let (catalog, second) = added(
            &catalog,
            Matcher::Pattern {
                text: "~/Other".into(),
                beside: None,
                names: Names::Folders,
            },
            &draft,
            Role::Searches,
        );
        assert_eq!(second, "virtual-machines-big-2");
        let e = catalog.entries.iter().find(|e| e.id == second).unwrap();
        assert!(e.ticks.get(Role::Searches) && !e.ticks.get(Role::Agents));
        // Without words, the id comes from the folder.
        let bare = Draft {
            pattern: "~/.local/share/Steam".into(),
            ..Draft::default()
        };
        let (catalog, bare_id) = added(&catalog, bare.matcher().unwrap(), &bare, Role::Searches);
        assert_eq!(bare_id, "local-share-steam");
        assert_eq!(
            catalog
                .entries
                .iter()
                .find(|e| e.id == bare_id)
                .unwrap()
                .category,
            YOURS
        );
        // Read back as written.
        let (categories, entries) = catalog.values();
        let read = Catalog::from_values(Some(&categories), Some(&entries), &Legacy::default());
        assert!(read.problems.is_empty(), "{:?}", read.problems);
        assert!(
            read.entries
                .iter()
                .any(|e| e.id == id && e.what == "Virtual machines (big)")
        );
        // Edited: its folders, words and category; its ticks stay.
        let edit = Draft {
            names: Names::Both,
            kind: Kind::Beside,
            pattern: "out".into(),
            beside: "*.vbox".into(),
            what: "VM output".into(),
            category: YOURS.into(),
            holding: String::new(),
        };
        let edited = edited(&catalog, &id, edit.matcher().unwrap(), &edit);
        let e = edited.entries.iter().find(|e| e.id == id).unwrap();
        assert_eq!(e.what, "VM output");
        assert_eq!(e.category, YOURS);
        assert!(e.ticks.get(Role::Agents));
        assert_eq!(e.matcher.said(), "any folder named out beside a *.vbox");
        let gone = removed(&edited, &id);
        assert!(!gone.entries.iter().any(|e| e.id == id));
    }

    #[test]
    fn a_draft_says_what_it_lacks_and_the_rules_say_what_they_cannot_read() {
        let empty = Draft::default();
        assert_eq!(empty.matcher().unwrap_err().0, Field::Pattern);
        let beside = Draft {
            kind: Kind::Beside,
            pattern: "target".into(),
            ..Draft::default()
        };
        assert_eq!(beside.matcher().unwrap_err().0, Field::Beside);
        let pathed = Draft {
            kind: Kind::Beside,
            pattern: "~/x/target".into(),
            beside: "Cargo.toml".into(),
            ..Draft::default()
        };
        assert_eq!(pathed.matcher().unwrap_err().0, Field::Pattern);
        let holding = Draft {
            kind: Kind::Holding,
            ..Draft::default()
        };
        assert_eq!(holding.matcher().unwrap_err().0, Field::Holding);
        // What only the rules know: a relative path, a regex that is none.
        for bad in ["rel/x", "regex:(["] {
            let draft = Draft {
                pattern: bad.into(),
                ..Draft::default()
            };
            let (catalog, id) = added(&shipped(), draft.matcher().unwrap(), &draft, Role::Searches);
            let problem = problem(&catalog, &id).unwrap_or_else(|| panic!("{bad} refused"));
            assert!(!problem.starts_with("exclusions."), "{problem}");
        }
        let fine = Draft {
            pattern: "cmake-build-*".into(),
            ..Draft::default()
        };
        let (catalog, id) = added(&shipped(), fine.matcher().unwrap(), &fine, Role::Searches);
        assert_eq!(problem(&catalog, &id), None);
    }

    #[test]
    fn the_same_folders_are_found_however_written() {
        let catalog = shipped();
        let home = skwir_core::os_path::home();
        let whole = home.as_ref().unwrap().join(".ssh");
        let written = Matcher::Pattern {
            text: format!("{}/", whole.display()),
            beside: None,
            names: Names::Folders,
        };
        let found = already(&catalog, &written, None, home.as_deref()).unwrap();
        assert_eq!(pattern_of(&found.matcher), "~/.ssh");
        // Not the one being edited.
        assert!(
            already(
                &catalog,
                &found.matcher.clone(),
                Some(&found.id.clone()),
                home.as_deref()
            )
            .is_none()
        );
        let name = Matcher::Pattern {
            text: "node_modules".into(),
            beside: None,
            names: Names::Folders,
        };
        assert!(already(&catalog, &name, None, home.as_deref()).is_some());
        let other = Matcher::Pattern {
            text: "node_modules".into(),
            beside: Some("package.json".into()),
            names: Names::Folders,
        };
        assert!(already(&catalog, &other, None, home.as_deref()).is_none());
    }

    #[test]
    fn a_users_category_is_made_renamed_and_deleted_its_folders_going_back() {
        let catalog = shipped();
        let ssh = entry(&catalog, "~/.ssh").id.clone();
        let (catalog, work) = new_category(&catalog, "Work");
        let (catalog, again) = new_category(&catalog, "Work");
        assert_eq!((work.as_str(), again.as_str()), ("work", "work-2"));
        let renamed = renamed(&catalog, &work, " Job ");
        assert_eq!(renamed.category(&work).unwrap().name, "Job");
        let skwirs = super::renamed(&renamed, "skwir.secrets", "Mine");
        assert_eq!(skwirs.category("skwir.secrets").unwrap().name, "Secrets");
        let moved = moved(&renamed, &ssh, &work);
        let draft = Draft {
            pattern: "~/Job".into(),
            category: work.clone(),
            ..Draft::default()
        };
        let (moved, mine) = added(&moved, draft.matcher().unwrap(), &draft, Role::Agents);
        assert_eq!(
            moved
                .entries
                .iter()
                .find(|e| e.id == mine)
                .unwrap()
                .category,
            work
        );
        let deleted = deleted_category(&moved, &work);
        assert!(deleted.category(&work).is_none());
        assert_eq!(entry(&deleted, "~/.ssh").category, "skwir.secrets");
        assert_eq!(
            deleted
                .entries
                .iter()
                .find(|e| e.id == mine)
                .unwrap()
                .category,
            YOURS
        );
        // skwir's are never deleted.
        let kept = deleted_category(&deleted, "skwir.secrets");
        assert!(kept.category("skwir.secrets").is_some());
        // Stored and read back, the user's category is named.
        let (categories, entries) = renamed.values();
        let read = Catalog::from_values(Some(&categories), Some(&entries), &Legacy::default());
        assert_eq!(read.category(&work).unwrap().name, "Job");
        let category: &Category = read.category(&work).unwrap();
        assert!(!category.builtin);
    }

    #[test]
    fn lowering_privacy_is_asked_and_raising_it_is_not() {
        let catalog = shipped();
        let ssh = entry(&catalog, "~/.ssh").id.clone();
        // Unticked in the Agents view: asked, naming it.
        let unticked = ticked(&catalog, &ssh, Role::Agents, false);
        assert_eq!(exposure(&catalog, &unticked), ["~/.ssh"]);
        assert_eq!(
            exposure_question(&exposure(&catalog, &unticked)),
            "Agents will be able to read ~/.ssh."
        );
        // Ticked again: nothing asked.
        assert!(exposure(&unticked, &catalog).is_empty());
        // Searches never ask.
        let searched = ticked(&catalog, &ssh, Role::Searches, false);
        assert!(exposure(&catalog, &searched).is_empty());
        // The whole category switched off: every secret.
        let off = switched(&catalog, "skwir.secrets", Role::Agents, false);
        let all = exposure(&catalog, &off);
        assert!(all.len() > 3);
        assert!(exposure_question(&all).contains(" more."));
        // Moved to a category not kept from agents, a category deleted.
        let moved = super::moved(&catalog, &ssh, "skwir.games");
        assert_eq!(exposure(&catalog, &moved), ["~/.ssh"]);
        let draft = Draft {
            pattern: "~/Taxes".into(),
            ..Draft::default()
        };
        let (mine, id) = added(&catalog, draft.matcher().unwrap(), &draft, Role::Agents);
        assert!(exposure(&catalog, &mine).is_empty(), "made private");
        assert_eq!(exposure(&mine, &removed(&mine, &id)), ["~/Taxes"]);
        let (with_category, work) = new_category(&catalog, "Work");
        let with_category = super::switched(&with_category, &work, Role::Agents, true);
        let into = super::moved(&with_category, &ssh, &work);
        assert!(exposure(&with_category, &into).is_empty());
        let deleted = deleted_category(&into, &work);
        assert!(exposure(&into, &deleted).is_empty(), "back in Secrets");
    }

    #[test]
    fn a_folder_is_looked_up_once_and_a_name_never() {
        let tmp = tempfile::tempdir().unwrap();
        let there = tmp.path().join("there");
        std::fs::create_dir(&there).unwrap();
        let texts = vec![
            there.display().to_string(),
            tmp.path().join("gone").display().to_string(),
        ];
        let found = presence(&texts, None);
        assert_eq!(found, [(texts[0].clone(), true), (texts[1].clone(), false)]);
        let asked = asked_presence(&shipped());
        assert!(asked.contains("~/.ssh") && !asked.contains("node_modules"));
    }

    #[test]
    fn a_tick_says_what_it_does_or_that_it_waits_for_its_category() {
        assert_eq!(
            said_tick("~/.ssh", Role::Agents, false, true),
            "~/.ssh is no longer excluded from agents"
        );
        assert_eq!(
            said_tick("~/.ssh", Role::Searches, false, false),
            "Unticked ~/.ssh; its category is off for searches and tags"
        );
    }

    #[test]
    fn a_row_says_what_it_means_only_where_that_adds_something() {
        assert_eq!(
            meaning(&Matcher::Pattern {
                text: "~/.ssh".into(),
                beside: None,
                names: Names::Folders,
            }),
            None
        );
        assert_eq!(
            meaning(&Matcher::Pattern {
                text: "target".into(),
                beside: Some("Cargo.toml".into()),
                names: Names::Folders,
            })
            .as_deref(),
            Some("any folder named target beside a Cargo.toml")
        );
    }
}
