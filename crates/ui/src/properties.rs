//! Properties: everything known about what the panel shows (ADR-058).
//!
//! A section of the preview frame, not a kind of preview and not a dialog
//! of its own: what other file managers open as a Properties window
//! (Alt+Enter, Ctrl+I, the context menu's last item) is this, following
//! the selection. With nothing to preview (a folder, an unknown file) it
//! is always open; over a preview it opens on demand
//! under the header's card (`preview::under_card`, the one place that
//! draws it), with at most its share of the room, above the preview or
//! beside it (`Room`), the edge between them dragged. Its groups fold
//! (`Folds`), and a row's links show under the pointer, so it says much
//! in little room.
//!
//! Facts come from three places, none of them read while painting: the entry's
//! metadata, with its provenance, collected when the entry or its listing
//! changes (`entry_groups`); what the preview already holds in memory (a
//! picture's size, a document's pages, a film's length); and what the format
//! itself told us (`Inspection::details`). Properties never starts a scan
//! by itself: what a folder holds in all is counted when asked
//! (`ui.properties_count`, `Show::Count`).

use gpui::{AnyElement, Context, SharedString, div, prelude::*, px};
use skwir_core::command::Invocation;
use skwir_core::entry::{Entry, Provenance, Value, keys};
use skwir_core::op::EntryRef;

use crate::format;
use crate::preview::PreviewContent;
use crate::shell::Shell;
use crate::theme::Theme;

/// How a row shows its value.
#[derive(Clone, Debug, PartialEq)]
pub enum Show {
    Text,
    /// Tags, as chips in their colors (`given`: those the file gives
    /// some, a Finder tag's); each searches for its tag.
    Tags {
        tags: Vec<String>,
        given: Vec<(String, usize)>,
    },
    /// Stars to click (half stars, from 1 to 10, or unrated), rating
    /// `target`, the entry shown.
    Rating {
        rating: Option<u64>,
        target: EntryRef,
    },
    /// What folders hold, as counted so far (`PreviewState::count`), or
    /// the link that counts it: drawn as the rows of `count_rows`. Of the
    /// selection, or of the entry shown, which only says it alone selected.
    Count {
        selection: bool,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub label: SharedString,
    /// What a click on the row copies, and what `Show::Text` shows.
    pub value: SharedString,
    pub show: Show,
    /// Who says so, when it is not the filesystem: the user, a plugin, a model.
    pub source: Option<SharedString>,
    /// Links after the value: their words and the commands they run.
    /// They show while the pointer is on the row, unless `actions_shown`
    /// or the row says nothing else.
    pub actions: Vec<(SharedString, Invocation)>,
    /// Its links show without the pointer on it: they stop what runs, or
    /// start it again.
    pub actions_shown: bool,
    /// Its links change who may use the file: a read-only drive takes
    /// them away (`render`).
    pub edits_access: bool,
    /// A click copies its value; not one that says a path lossily (a
    /// name that is not text), which would name another file.
    pub copies: bool,
}

impl Row {
    pub fn new(label: impl Into<SharedString>, value: impl Into<SharedString>) -> Self {
        Row {
            label: label.into(),
            value: value.into(),
            show: Show::Text,
            source: None,
            actions: Vec::new(),
            actions_shown: false,
            edits_access: false,
            copies: true,
        }
    }

    /// A path, said as text: one that is not text is said lossily, and
    /// a click does not copy it.
    fn path(label: impl Into<SharedString>, path: &std::path::Path) -> Self {
        Row {
            copies: path.to_str().is_some(),
            ..Row::new(label, path.to_string_lossy().into_owned())
        }
    }

    /// Its links show whether the pointer is on it or not.
    fn shown(mut self) -> Self {
        self.actions_shown = true;
        self
    }

    /// Whether its links show without the pointer on it: they stop or
    /// start what runs, or they are all it says (Count, Compute, the Add…
    /// of tags and of a comment not given yet).
    pub fn links_shown(&self) -> bool {
        self.actions_shown || self.value.is_empty()
    }

    /// A link that changes who may use the file.
    fn edit(mut self, words: &'static str, command: Invocation) -> Self {
        self.actions.push((words.into(), command));
        self.edits_access = true;
        self
    }

    fn action(mut self, words: &'static str, command: Invocation) -> Self {
        self.actions.push((words.into(), command));
        self
    }

    /// A provider's fact, with its link.
    fn of(detail: &skwir_core::inspection::Detail) -> Self {
        Row {
            actions: detail
                .action
                .iter()
                .map(|(words, action)| (words.clone().into(), action.clone()))
                .collect(),
            ..Row::new(detail.label.clone(), detail.value.clone())
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub title: SharedString,
    pub rows: Vec<Row>,
    /// What its heading says at its right while it is folded: the gist of
    /// its rows (the drive's name and its free room).
    pub gist: Option<SharedString>,
    /// It ships folded, until the user unfolds it: what is seldom looked
    /// at (the drive's, the shell's own; a source's whose rows say so,
    /// `Property::folded`).
    pub folded: bool,
}

impl Group {
    pub fn new(title: impl Into<SharedString>, rows: Vec<Row>) -> Self {
        Group {
            title: title.into(),
            rows,
            gist: None,
            folded: false,
        }
    }
}

fn group(title: &'static str, rows: Vec<Row>) -> Option<Group> {
    (!rows.is_empty()).then(|| Group::new(title, rows))
}

/// The title of the Permissions group of several entries: what they
/// share, apart from the Permissions of the entry the cursor is on.
pub const SHARED_PERMISSIONS: &str = "Shared permissions";

/// The groups the user folded or unfolded, by title; one it does not name
/// is as Properties ships it (`Group::folded`). Part of the window's shape
/// (`session.rs`), so a group folded for one file stays folded for every
/// other.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Folds(std::collections::BTreeMap<String, bool>);

impl Folds {
    /// Whether the group `title` is folded: as the user left it, else as
    /// it ships (`shipped`).
    pub fn folded(&self, title: &str, shipped: bool) -> bool {
        self.0.get(title).copied().unwrap_or(shipped)
    }

    /// Fold the group `title` or unfold it; as it ships (`shipped`), it is
    /// forgotten, so a later change of how it ships reaches it.
    pub fn set(&mut self, title: &str, folded: bool, shipped: bool) {
        if folded == shipped {
            self.0.remove(title);
        } else {
            self.0.insert(title.to_owned(), folded);
        }
    }
}

/// A row a property source answered (`plugin_rows`): the group it joins,
/// whether that group ships folded, and the row.
#[derive(Clone, Debug, PartialEq)]
pub struct SourceRow {
    pub group: SharedString,
    pub folded: bool,
    pub row: Row,
}

/// Properties' share of the room over a preview, until its edge is
/// dragged, and again after a double click on it: at most half above the
/// preview (less when it says less), half beside it.
pub const SHARE: f32 = 0.5;
/// What each side keeps, whatever the drag: a heading and a few rows of
/// Properties, the preview's toolbar and some of it; a column wide enough
/// for a label and its value.
const KEEP_HEIGHT: f32 = crate::preview_bar::HEIGHT + crate::views::list::ROW_HEIGHT * 3.;
const KEEP_WIDTH: f32 = 200.;

fn text_of(value: &Value) -> String {
    match value {
        Value::Bool(v) => if *v { "Yes" } else { "No" }.into(),
        Value::Int(v) => v.to_string(),
        Value::UInt(v) => v.to_string(),
        Value::Float(v) => format!("{v:.3}"),
        Value::Text(v) => v.to_string(),
        Value::Time(v) => crate::format::exact_time(*v),
        Value::List(items) => items.iter().map(text_of).collect::<Vec<_>>().join(", "),
    }
}

fn source_of(provenance: &Provenance) -> Option<SharedString> {
    match provenance {
        Provenance::Filesystem => None,
        Provenance::User => Some("set by you".into()),
        Provenance::Plugin { name } => Some(format!("from {name}").into()),
        Provenance::Model { name, confidence } => {
            Some(format!("suggested by {name} · {:.0}%", confidence * 100.).into())
        }
    }
}

/// `tags` reads better than `tags.color`: the last part, with a capital.
fn label_of(key: &str) -> String {
    let word = key.rsplit('.').next().unwrap_or(key).replace('_', " ");
    let mut chars = word.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().collect::<String>() + chars.as_str()
    })
}

/// The fields Properties names itself; any other is shown under More.
const CORE: [&str; 19] = [
    keys::SIZE,
    keys::MODIFIED,
    keys::CREATED,
    keys::ACCESSED,
    keys::MODE,
    keys::NLINK,
    keys::UID,
    keys::GID,
    keys::LINK_TARGET,
    keys::TAGS,
    keys::TAG_COLORS,
    keys::RATING,
    keys::COMMENT,
    keys::ORIGIN,
    keys::REFERRER,
    keys::NOTES_KEPT,
    keys::NOTES_DIFFER,
    keys::TRASHED,
    keys::TRASHED_FROM,
];

/// Who took a note, when it was not the user: notes are theirs, so only a
/// suggestion (a model's, a plugin's) says so.
fn note_source(provenance: &Provenance) -> Option<SharedString> {
    match provenance {
        Provenance::User => None,
        other => source_of(other),
    }
}

/// The user's notes on an entry (tags, rating, comment), each with the way
/// to change it, and where a download came from, with the way back there.
/// Only while the tags plugin, which reads and writes them, is on.
/// A category holding one tag at a time (`fields`) is a field of its own,
/// whose value is the tag within it (`Status: Draft`).
fn notes_groups(entry: &Entry, path: &std::path::Path, fields: &[String]) -> [Option<Group>; 2] {
    let meta = &entry.metadata;
    let text = |key| meta.value(key).and_then(Value::as_str).map(str::to_owned);
    let target = serde_json::json!([EntryRef::new(entry.file(), path)]);
    let (valued, tags): (Vec<String>, Vec<String>) = crate::tags::tags_of(entry)
        .into_iter()
        .partition(|t| skwir_core::metadata::exclusive_category(t, fields).is_some());
    let field_rows: Vec<Row> = valued
        .iter()
        .filter_map(|t| {
            let field = skwir_core::metadata::exclusive_category(t, fields)?;
            let value = skwir_core::metadata::tag_under(t, field)?.trim_start_matches('/');
            Some(
                Row::new(
                    skwir_core::metadata::tag_leaf(field).to_owned(),
                    value.to_owned(),
                )
                .action(
                    "Change…",
                    Invocation::with_args(
                        "ui.tags_prompt",
                        &serde_json::json!({ "entries": target, "text": format!("{field}/") }),
                    ),
                ),
            )
        })
        .collect();
    let mut tags_row = Row {
        show: Show::Tags {
            tags: tags.clone(),
            given: crate::tags::given_colors(entry),
        },
        source: meta
            .get(keys::TAGS)
            .and_then(|f| note_source(&f.provenance)),
        ..Row::new("Tags", tags.join(", "))
    };
    tags_row = tags_row.action(
        if tags.is_empty() { "Add…" } else { "Edit…" },
        Invocation::with_args("ui.tags_prompt", &serde_json::json!({ "entries": target })),
    );
    let rating = meta.value(keys::RATING).and_then(Value::as_u64);
    let rating_row = Row {
        show: Show::Rating {
            rating,
            target: EntryRef::new(entry.file(), path),
        },
        ..Row::new(
            "Rating",
            rating.map_or_else(String::new, crate::tags::stars_words),
        )
    };
    let comment = text(keys::COMMENT);
    let mut ask = serde_json::json!({
        "title": "Comment",
        "accept": "Save",
        "placeholder": "A comment on this item (empty removes it)",
        "command": "notes.comment",
        "field": "text",
        "args": { "entries": target },
    });
    if let Some(comment) = &comment {
        ask["text"] = comment.as_str().into();
    }
    let comment_row = Row {
        source: comment
            .as_ref()
            .and(meta.get(keys::COMMENT))
            .and_then(|f| note_source(&f.provenance)),
        ..Row::new("Comment", comment.clone().unwrap_or_default())
    }
    .action(
        if comment.is_some() {
            "Edit…"
        } else {
            "Add…"
        },
        Invocation::with_args("ui.ask", &ask),
    );
    // A URL is opened only when asked, in the browser: skwir fetches
    // nothing a file names (ADR-020).
    let link = |label: &'static str, url: String| {
        let row = Row::new(label, url.clone());
        let row = if url.starts_with("https://") || url.starts_with("http://") {
            row.action(
                "Open",
                Invocation::with_args("ui.open_url", &serde_json::json!({ "url": url })),
            )
        } else {
            row
        };
        row.action(
            "Copy",
            Invocation::with_args(
                "ui.copy_text",
                &serde_json::json!({ "text": url, "what": label.to_lowercase() }),
            ),
        )
    };
    let source = [
        text(keys::ORIGIN).map(|url| link("Downloaded from", url)),
        text(keys::REFERRER).map(|url| link("Found on", url)),
    ]
    .into_iter()
    .flatten()
    .collect();
    let titles = |key| match meta.value(key) {
        Some(Value::List(items)) => items
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    let mut rows = vec![tags_row];
    rows.extend(field_rows);
    rows.extend([rating_row, comment_row]);
    // Where the notes are, and the stores that say otherwise (ADR-027).
    let kept = titles(keys::NOTES_KEPT);
    if !kept.is_empty() {
        rows.push(Row::new("Kept in", kept.join("; ")));
    }
    let differ = titles(keys::NOTES_DIFFER);
    if !differ.is_empty() {
        rows.push(Row::new("Differs in", differ.join("; ")).action(
            "Make them agree",
            Invocation::with_args("notes.agree", &serde_json::json!({ "entries": target })),
        ));
    }
    [group("Notes", rows), group("Source", source)]
}

/// What the entry itself says; with `notes`, what the user noted on it
/// (the tags plugin is on), the categories it holds one tag at a time of
/// as fields. Pure, so it is tested without a window.
pub fn entry_groups(
    entry: &Entry,
    path: &std::path::Path,
    notes: Option<&[String]>,
    user: &User,
) -> Vec<Group> {
    let meta = &entry.metadata;
    let uint = |key| meta.value(key).and_then(Value::as_u64);
    let time = |key| meta.value(key).and_then(Value::as_time);

    // The header already shows the name and the type; the exact byte count
    // is the one general fact it rounds. A folder's is what it holds,
    // counted when asked.
    let general = if entry.is_dir() {
        vec![Row {
            show: Show::Count { selection: false },
            ..Row::new("Size", "")
        }]
    } else {
        uint(keys::SIZE)
            .map(|size| {
                Row::new(
                    "Size",
                    format!(
                        "{} ({} bytes)",
                        format::size(size),
                        format::thousands(size as usize)
                    ),
                )
            })
            .into_iter()
            .collect()
    };

    let mut location = Vec::new();
    // A file of the trash: where restoring it puts it back.
    if let Some(from) = meta.value(keys::TRASHED_FROM).and_then(Value::as_str) {
        location.push(Row::new("Deleted from", from.to_owned()).action(
            "Show",
            Invocation::with_args("navigate", &serde_json::json!({ "location": from })),
        ));
    }
    // The whole path, to copy with a click; a place (a tag's) has none.
    if !entry.is_place() && path.is_absolute() {
        location.push(Row::path("Path", path));
    }
    if let Some(parent) = path.parent() {
        location.push(Row::path("Folder", parent));
    }
    if let Some(target) = meta.value(keys::LINK_TARGET).and_then(Value::as_str) {
        location.push(Row::new("Links to", target.to_owned()));
    }

    let mut permissions = if let Some(subject) = Subject::of(entry) {
        access_rows(
            &[subject],
            &Targets::Entry(EntryRef::new(entry.file(), path)),
            meta.value(keys::TRASHED).is_some(),
            user,
        )
    } else {
        // A mode alone, or an owner alone: what is known, read-only.
        let mut rows = Vec::new();
        if let Some(uid) = uint(keys::UID) {
            rows.push(Row::new("Owner", owner_named(uid)));
        }
        if let Some(gid) = uint(keys::GID) {
            rows.push(Row::new("Group", group_named(gid)));
        }
        if let Some(mode) = uint(keys::MODE) {
            rows.push(Row::new("Access", skwir_platform::users::mode(mode)));
        }
        rows
    };
    if let Some(links) = uint(keys::NLINK).filter(|links| *links > 1 && !entry.is_dir()) {
        permissions.push(Row::new("Hard links", links.to_string()));
    }

    let times = [
        ("Deleted", keys::TRASHED),
        ("Modified", keys::MODIFIED),
        ("Created", keys::CREATED),
        ("Accessed", keys::ACCESSED),
    ]
    .into_iter()
    .filter_map(|(label, key)| Some(Row::new(label, crate::format::exact_time(time(key)?))))
    .collect();

    // Whatever else a plugin or the user attached: tags, ratings, suggestions.
    let more = meta
        .iter()
        .filter(|(key, _)| !CORE.contains(key))
        .map(|(key, field)| Row {
            source: source_of(&field.provenance),
            ..Row::new(label_of(key), text_of(&field.value))
        })
        .filter(|row| !row.value.is_empty())
        .collect();

    let [notes, source] = match notes {
        Some(fields) => notes_groups(entry, path, fields),
        None => [None, None],
    };
    [
        group("General", general),
        notes,
        source,
        group("Location", location),
        group("Permissions", permissions),
        group("Times", times),
        group("More", more),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// An owner by name and number, or its number alone where no name is known.
fn owner_named(uid: u64) -> String {
    named(uid, skwir_platform::users::user(uid))
}

/// A group by name and number, or its number alone.
fn group_named(gid: u64) -> String {
    named(gid, skwir_platform::users::group(gid))
}

fn named(id: u64, name: Option<String>) -> String {
    match name {
        Some(name) => format!("{name} ({id})"),
        None => id.to_string(),
    }
}

/// The Selection group, for several entries selected: how many and of
/// which kinds, what they weigh (the files' sizes once all are known, or
/// a count where folders are among them); then the owner, group and
/// access they share, "Mixed" where they differ, in a group of its own
/// (`SHARED_PERMISSIONS`: the entry under the cursor keeps its
/// Permissions, folded apart). A fact some entry does not say yet is left
/// out. Pure, so it is tested without a window.
pub fn selection_groups(
    entries: &[&Entry],
    selected: usize,
    bytes: Option<u64>,
    pane: skwir_core::workspace::PaneId,
    user: &User,
) -> Vec<Group> {
    let folders = entries.iter().filter(|e| e.is_dir()).count();
    let mut rows = vec![Row::new("Selected", format::count(selected, "item"))];
    if folders > 0 && folders < entries.len() {
        rows.push(Row::new(
            "Of which",
            format!(
                "{}, {}",
                format::count(folders, "folder"),
                format::count(entries.len() - folders, "file")
            ),
        ));
    }
    if folders > 0 {
        rows.push(Row {
            show: Show::Count { selection: true },
            ..Row::new("Total size", "")
        });
    } else if let Some(bytes) = bytes {
        rows.push(Row::new("Total size", format::size(bytes)));
    }
    let mut groups = vec![Group::new("Selection", rows)];
    // Who may use them, once every entry says it.
    let subjects: Option<Vec<Subject>> = entries.iter().map(|e| Subject::of(e)).collect();
    if let Some(subjects) = subjects.filter(|s| !s.is_empty()) {
        let trashed = entries
            .iter()
            .any(|e| e.metadata.value(keys::TRASHED).is_some());
        groups.push(Group::new(
            SHARED_PERMISSIONS,
            access_rows(&subjects, &Targets::Selection(pane), trashed, user),
        ));
    }
    groups
}

/// What `sources` say of the entry `target`, each row with the group it
/// joins and whether that group ships folded: their commands dispatched
/// in turn, a source that fails or answers what is no rows saying
/// nothing. On a worker.
pub fn plugin_rows(
    app: &skwir_core::App,
    sources: &[skwir_core::registry::PropertySource],
    target: &EntryRef,
) -> Vec<SourceRow> {
    #[derive(serde::Deserialize)]
    struct Answer {
        rows: Vec<skwir_core::registry::Property>,
    }
    let mut rows = Vec::new();
    for source in sources {
        let command = source
            .command
            .clone()
            .with("entries", serde_json::json!([target]));
        let answer = match app.dispatch(&command) {
            Ok(skwir_core::command::Outcome::Value(value)) => {
                serde_json::from_value::<Answer>(value)
            }
            Ok(_) => continue,
            Err(e) => {
                tracing::debug!(command = %command.name, "property source: {e:#}");
                continue;
            }
        };
        match answer {
            Ok(answer) => rows.extend(answer.rows.into_iter().map(|property| {
                SourceRow {
                    group: SharedString::from(property.group),
                    folded: property.folded,
                    row: Row {
                        actions: property
                            .actions
                            .into_iter()
                            .map(|a| (a.words.into(), a.command))
                            .collect(),
                        actions_shown: property.actions_shown,
                        ..Row::new(property.label, property.value)
                    },
                }
            })),
            Err(e) => tracing::debug!(command = %command.name, "property source answer: {e}"),
        }
    }
    rows
}

/// The user Properties offers changes to: who they are and their groups
/// (`skwir_platform::users`), asked when Properties is collected.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct User {
    /// `None` where users are not numbers (Windows): nothing is offered.
    pub id: Option<u64>,
    pub groups: Vec<u64>,
}

impl User {
    pub fn current() -> Self {
        User {
            id: skwir_platform::users::me(),
            groups: skwir_platform::users::my_groups(),
        }
    }
}

/// What a change of access acts on: the entry shown, or a pane's
/// selection.
#[derive(Clone, Debug)]
pub enum Targets {
    Entry(EntryRef),
    Selection(skwir_core::workspace::PaneId),
}

impl Targets {
    /// `set_access`'s arguments for them, with `more`.
    fn args(&self, more: &serde_json::Value) -> serde_json::Value {
        let mut args = match self {
            Targets::Entry(entry) => serde_json::json!({ "entries": [entry] }),
            Targets::Selection(pane) => serde_json::json!({ "pane": pane }),
        };
        if let (Some(args), Some(more)) = (args.as_object_mut(), more.as_object()) {
            args.extend(more.clone());
        }
        args
    }

    /// How a picker's title names them.
    fn named(&self, count: usize) -> String {
        match self {
            Targets::Entry(entry) => format!(
                "“{}”",
                entry
                    .path
                    .file_name()
                    .unwrap_or(entry.path.as_os_str())
                    .to_string_lossy()
            ),
            Targets::Selection(_) => format::count(count, "item"),
        }
    }
}

/// What a change of access needs to know of an entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Subject {
    /// Its permission bits.
    pub mode: u64,
    pub uid: u64,
    pub gid: u64,
    pub folder: bool,
    /// A file that could be run: not a folder, a pipe or a device.
    pub regular: bool,
}

impl Subject {
    /// What `entry` says of its access, once it says all of it.
    pub fn of(entry: &Entry) -> Option<Subject> {
        let meta = &entry.metadata;
        let uint = |key| meta.value(key).and_then(Value::as_u64);
        Some(Subject {
            mode: uint(keys::MODE)? & 0o7777,
            uid: uint(keys::UID)?,
            gid: uint(keys::GID)?,
            folder: entry.is_dir(),
            regular: !entry.is_dir() && entry.inode_type().is_none(),
        })
    }
}

/// The three classes a mode speaks to: as `chmod` names each, the row's
/// label, how a picker's title says it, and where its bits are.
const CLASSES: [(char, &str, &str, u32); 3] = [
    ('u', "Owner can", "its owner", 6),
    ('g', "Group can", "its group", 3),
    ('o', "Others can", "others", 0),
];

/// A file's levels (its run bit is "Can run"'s), in the order offered:
/// each in words, its read and write bits, and the clause that sets them
/// and keeps the run bit.
fn file_levels(c: char) -> Vec<(&'static str, u64, String)> {
    vec![
        ("None", 0, format!("{c}-rw")),
        ("Read", 4, format!("{c}+r,{c}-w")),
        ("Read and write", 6, format!("{c}+rw")),
    ]
}

/// A folder's levels, with the clause that sets each: `X` gives search to
/// the folders inside and leaves files that no one runs as they were.
fn folder_levels(c: char) -> Vec<(&'static str, u64, String)> {
    vec![
        ("None", 0, format!("{c}=")),
        ("List items only", 4, format!("{c}=r")),
        ("Open items", 5, format!("{c}=rX")),
        ("Create and delete items", 7, format!("{c}=rwX")),
    ]
}

/// A class's level in words: of a file its read and write bits, of a
/// folder all three.
fn level_said(bits: u64, folder: bool) -> String {
    if !folder {
        return match bits & 6 {
            0 => "None",
            4 => "Read",
            6 => "Read and write",
            _ => "Write only",
        }
        .to_owned();
    }
    folder_levels('u')
        .into_iter()
        .find(|(_, at, _)| *at == bits)
        .map_or_else(
            || {
                // A combination no one picks (-wx): its letters.
                let letter = |bit, c| if bits & bit == 0 { '-' } else { c };
                [letter(4, 'r'), letter(2, 'w'), letter(1, 'x')]
                    .iter()
                    .collect()
            },
            |(words, ..)| words.to_owned(),
        )
}

/// Who may run a file, from its run bits.
fn runners(mode: u64) -> &'static str {
    match (mode & 0o100 != 0, mode & 0o010 != 0, mode & 0o001 != 0) {
        (false, false, false) => "No one",
        (true, false, false) => "Its owner",
        (true, true, false) => "Its owner and group",
        (true, true, true) => "Everyone",
        (true, false, true) => "Its owner and others",
        (false, true, false) => "Its group",
        (false, true, true) => "Its group and others",
        (false, false, true) => "Others",
    }
}

/// What its set-ID and sticky bits do, in words.
fn specials(mode: u64, folder: bool) -> Vec<&'static str> {
    let setuid = if folder {
        "Set-user-ID"
    } else {
        "Runs as its owner, whoever starts it"
    };
    let setgid = if folder {
        "New items take the folder's group"
    } else {
        "Runs as its group"
    };
    [
        (0o4000, setuid),
        (0o2000, setgid),
        (0o1000, "Only an item's owner may rename or delete it"),
    ]
    .into_iter()
    .filter(|(bit, _)| mode & bit != 0)
    .map(|(_, words)| words)
    .collect()
}

/// Who may use `subjects` (one entry, or a selection) and the ways to
/// change it: the owner, the group (to one of the user's), what each class
/// can do in words, who can run a file, the mode as `chmod` writes it, and
/// what its special bits do. A value they do not share says Mixed; levels
/// are offered where all are files or all folders. Nothing is offered to a
/// user who does not own them all (root aside), nor for the trash's files:
/// a row says why. Pure, so it is tested without a window.
pub fn access_rows(
    subjects: &[Subject],
    targets: &Targets,
    trashed: bool,
    user: &User,
) -> Vec<Row> {
    let Some(first) = subjects.first().copied() else {
        return Vec::new();
    };
    let shared = |of: &dyn Fn(&Subject) -> u64| {
        let value = of(&first);
        subjects.iter().all(|s| of(s) == value).then_some(value)
    };
    let mixed = || "Mixed".to_owned();
    let owns = user
        .id
        .is_some_and(|me| me == 0 || subjects.iter().all(|s| s.uid == me));
    let editable = owns && !trashed;
    let folders = subjects.iter().all(|s| s.folder);
    let files = subjects.iter().all(|s| !s.folder);
    let count = subjects.len();
    let set = |mode: String| {
        serde_json::json!({
            "name": "set_access",
            "args": targets.args(&serde_json::json!({ "mode": mode })),
        })
    };
    // A picker whose choices each run a `set_access`; for folders, with a
    // switch that reaches what they hold.
    let choose = |title: String, choices: Vec<serde_json::Value>| {
        let mut args = serde_json::json!({ "title": title, "choices": choices });
        if subjects.iter().any(|s| s.folder) {
            args["option"] = serde_json::json!({
                "title": "Also everything inside",
                "field": "inside",
            });
        }
        Invocation::with_args("ui.choose", &args)
    };
    let choice = |title: &str, now: bool, command: serde_json::Value| {
        let mut choice = serde_json::json!({ "title": title, "command": command });
        if now {
            choice["detail"] = "Now".into();
        }
        choice
    };

    let mut rows = vec![Row::new(
        "Owner",
        shared(&|s| s.uid).map_or_else(mixed, owner_named),
    )];
    let mut group = Row::new("Group", shared(&|s| s.gid).map_or_else(mixed, group_named));
    if editable && !user.groups.is_empty() {
        let now = shared(&|s| s.gid);
        let mut groups: Vec<(String, u64)> = user
            .groups
            .iter()
            .map(|&gid| {
                (
                    skwir_platform::users::group(gid).unwrap_or_else(|| gid.to_string()),
                    gid,
                )
            })
            .collect();
        groups.sort();
        let choices = groups
            .into_iter()
            .map(|(name, gid)| {
                choice(
                    &name,
                    now == Some(gid),
                    serde_json::json!({
                        "name": "set_access",
                        "args": targets.args(&serde_json::json!({ "group": gid })),
                    }),
                )
            })
            .collect();
        group = group.edit(
            "Change…",
            choose(
                format!("Give {} to the group", targets.named(count)),
                choices,
            ),
        );
    }
    rows.push(group);

    // A file's level leaves its run bit to "Can run".
    let level_bits = if folders { 0o7 } else { 0o6 };
    for (c, label, whose, shift) in CLASSES {
        let bits = shared(&|s| (s.mode >> shift) & level_bits);
        let mut row = Row::new(
            label,
            bits.map_or_else(mixed, |bits| level_said(bits, folders)),
        );
        if editable && (folders || files) {
            let levels = if folders {
                folder_levels(c)
            } else {
                file_levels(c)
            };
            let choices = levels
                .into_iter()
                .map(|(words, at, clause)| choice(words, bits == Some(at), set(clause)))
                .collect();
            row = row.edit(
                "Change…",
                choose(
                    format!("What {whose} may do with {}", targets.named(count)),
                    choices,
                ),
            );
        }
        rows.push(row);
    }

    if subjects.iter().all(|s| s.regular) {
        let runs = shared(&|s| s.mode & 0o111);
        let mut row = Row::new(
            "Can run",
            runs.map_or_else(mixed, |m| runners(m).to_owned()),
        );
        if editable {
            let to = |mode: String| {
                Invocation::with_args(
                    "set_access",
                    &targets.args(&serde_json::json!({ "mode": mode })),
                )
            };
            row = if runs == Some(0) || runs.is_none() {
                // Everyone who can read it may run it, its owner at least.
                let readers: String = CLASSES
                    .iter()
                    .filter(|(c, _, _, shift)| {
                        *c == 'u' || subjects.iter().all(|s| (s.mode >> shift) & 4 != 0)
                    })
                    .map(|(c, ..)| c)
                    .collect();
                row.edit("Allow running", to(format!("{readers}+x")))
            } else {
                row.edit("Stop running", to("a-x".into()))
            };
        }
        rows.push(row);
    }

    let mode = shared(&|s| s.mode);
    let mut access = Row::new(
        "Access",
        mode.map_or_else(mixed, skwir_platform::users::mode),
    );
    if editable {
        let mut ask = serde_json::json!({
            "title": format!("Access of {}", targets.named(count)),
            "accept": "Change",
            "placeholder": "As chmod takes it: 644, u+x, go-w, a=rX",
            "command": "set_access",
            "field": "mode",
            "args": targets.args(&serde_json::json!({})),
        });
        if let Some(mode) = mode {
            ask["text"] = format!("{mode:o}").into();
        }
        access = access.edit("Change…", Invocation::with_args("ui.ask", &ask));
    }
    rows.push(access);
    if let Some(mode) = mode {
        rows.extend(
            specials(mode, first.folder)
                .into_iter()
                .map(|words| Row::new("Special", words)),
        );
    }
    if !editable && user.id.is_some() {
        let why = if trashed {
            "Restore it to change who may use it"
        } else if count > 1 {
            "Only their owner can change who may use them"
        } else {
            "Only its owner can change who may use it"
        };
        rows.push(Row::new("Changes", why));
    }
    rows
}

/// The rows of a count of what folders hold (`Show::Count`), labelled
/// `label`: the link that counts, what is counted so far with the link
/// that stops, or the total with the link that counts again (what a folder
/// holds changes), the room it takes and how many files and folders, and
/// what it could not count.
pub fn count_rows(label: &str, count: Option<(skwir_core::fs::count::Count, bool)>) -> Vec<Row> {
    let again = || Invocation::new("ui.properties_count");
    let Some((now, running)) = count else {
        return vec![Row::new(label.to_owned(), "").action("Count", again())];
    };
    let size = format::size(now.size);
    let first = if running {
        Row::new(label.to_owned(), format!("{size} so far…")).action(
            "Stop",
            Invocation::with_args("ui.properties_count", &serde_json::json!({ "stop": true })),
        )
    } else if !now.done {
        Row::new(label.to_owned(), format!("At least {size}, stopped"))
            .action("Count again", again())
    } else if now.lower_bound() {
        Row::new(label.to_owned(), format!("At least {size}")).action("Count again", again())
    } else {
        Row::new(
            label.to_owned(),
            format!("{size} ({} bytes)", format::thousands(now.size as usize)),
        )
        .action("Count again", again())
    }
    .shown();
    let mut rows = vec![
        first,
        Row::new("On disk", format::size(now.allocated)),
        Row::new(
            "In all",
            format!(
                "{}, {}",
                format::count(now.files as usize, "file"),
                format::count(now.folders as usize, "folder")
            ),
        ),
    ];
    if now.unreadable > 0 {
        rows.push(Row::new(
            "Unreadable",
            format!(
                "{} could not be listed",
                format::count(now.unreadable as usize, "folder")
            ),
        ));
    }
    // Neither files nor folders: what they are could not be read.
    if now.unknown > 0 {
        rows.push(Row::new(
            "Unknown",
            format!(
                "{} could not be read",
                format::count(now.unknown as usize, "item")
            ),
        ));
    }
    if now.other_drives > 0 {
        rows.push(Row::new(
            "Not counted",
            format!(
                "{} mounted inside",
                format::count(now.other_drives as usize, "drive")
            ),
        ));
    }
    rows
}

/// What the system keeps of a file beside its mode, in words, for the
/// Permissions group: why it cannot be deleted, why it takes less room.
pub fn flag_rows(flags: skwir_core::fs::stat::Flags) -> Vec<Row> {
    [
        (
            flags.immutable,
            "Immutable",
            "No one can change, rename or delete it",
        ),
        (flags.append_only, "Append only", "It can only grow"),
        (flags.compressed, "Compressed", "By the drive"),
        (flags.encrypted, "Encrypted", "By the drive"),
        (flags.read_only, "Read-only", "It cannot be changed"),
    ]
    .into_iter()
    .filter(|(set, ..)| *set)
    .map(|(_, label, value)| Row::new(label, value))
    .collect()
}

/// What the preview in memory knows, without touching the file again.
fn preview_group(shell: &Shell) -> Option<Group> {
    let preview = &shell.preview;
    let mut rows = Vec::new();
    match &preview.content {
        Some(PreviewContent::Image {
            width,
            height,
            document,
            facts,
            ..
        }) if document.is_none() && !shell.shows_player() => {
            rows.push(Row::new("Dimensions", format!("{width} × {height} px")));
            rows.extend(facts.iter().map(Row::of));
        }
        Some(PreviewContent::Dir { items, folders }) => {
            rows.push(Row::new("Contains", format::count(*items, "item")));
            if *folders > 0 && folders < items {
                rows.push(Row::new(
                    "Of which",
                    format!(
                        "{}, {}",
                        format::count(*folders, "folder"),
                        format::count(items - folders, "file")
                    ),
                ));
            }
        }
        Some(PreviewContent::Inspection(display)) => {
            rows.extend(display.data.details.iter().map(Row::of));
        }
        _ => {}
    }
    if let Some(pages) = crate::reader::pages(shell) {
        rows.push(Row::new("Pages", pages.to_string()));
    }
    if shell.shows_player() {
        let state = &preview.media.state;
        for (label, value) in [
            ("Title", &state.title),
            ("Artist", &state.artist),
            ("Album", &state.album),
        ] {
            if !value.is_empty() {
                rows.push(Row::new(label, value.clone()));
            }
        }
        if let Some(duration) = state.duration {
            rows.push(Row::new("Duration", crate::media::time(duration)));
        }
        rows.extend(
            state
                .details
                .iter()
                .map(|(label, value)| Row::new(label.clone(), value.clone())),
        );
        if state.audio_tracks.len() > 1 {
            rows.push(Row::new("Audio tracks", state.audio_tracks.join(", ")));
        }
        // What the file says of itself, read with its poster: where a
        // video was recorded.
        if let Some(PreviewContent::Image { facts, .. }) = &preview.content {
            rows.extend(facts.iter().map(Row::of));
        }
        if !state.subtitle_tracks.is_empty() {
            rows.push(Row::new("Subtitles", state.subtitle_tracks.join(", ")));
        }
    }
    // What plugins read of the file on demand (a picture's place).
    rows.extend(preview.fields.iter().cloned());
    group("Content", rows)
}

/// Tags as chips in their colors: a click searches for the tag where the
/// pane is (`query.refine`, which Back undoes); a right click opens the
/// tag's menu (`Shell::open_tag_menu`).
fn tag_chips(
    shell: &Shell,
    tags: &[String],
    given: &[(String, usize)],
    row: usize,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> gpui::Div {
    let pane = shell.pane;
    div()
        .flex()
        .flex_wrap()
        .gap_1()
        .children(tags.iter().enumerate().map(|(i, tag)| {
            let search = tag.clone();
            crate::tags::chip_in_full(tag, shell.tag_colors.of_given(tag, given), theme)
                .id(("detail-tag", row * 64 + i))
                .cursor_pointer()
                .text_color(theme.text)
                .hover(gpui::Styled::underline)
                .on_mouse_down(
                    gpui::MouseButton::Right,
                    cx.listener({
                        let tag = tag.clone();
                        move |this, event: &gpui::MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            this.open_tag_menu(&tag, event.position, window, cx);
                        }
                    }),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.run(
                        "query.refine",
                        serde_json::json!({ "facet": "tag", "value": search, "pane": pane }),
                        window,
                        cx,
                    );
                }))
        }))
}

/// Five stars to click: the nth rates `target` n stars, and the one it has
/// already takes its rating away.
fn star_picker(
    rating: Option<u64>,
    target: &EntryRef,
    row: usize,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> gpui::Div {
    let current = rating.unwrap_or(0);
    div()
        .flex()
        .items_center()
        .gap(px(2.))
        .children((1..=5u64).map(|n| {
            let stars = if current == n * 2 { 0 } else { n };
            let target = target.clone();
            div()
                .id(("detail-star", row * 8 + n as usize))
                .cursor_pointer()
                .child(crate::tags::star(n, current, 14., theme))
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.run(
                        "notes.rate",
                        serde_json::json!({ "stars": stars, "entries": [target] }),
                        window,
                        cx,
                    );
                }))
        }))
}

/// Whether there is nothing to show but the properties. A preview still on
/// its way is not nothing: Properties waits for the answer rather than
/// filling the body for the instant before a picture replaces it. Nor is
/// a member with no preview of its own while its container's view is kept
/// above it (`PreviewState::parked`): that view is content, so Properties
/// stays as the card left it, opened and closed on demand.
pub fn alone(shell: &Shell) -> bool {
    !shell.shows_player()
        && shell.preview.parked().is_none()
        && match &shell.preview.content {
            None => shell.preview.phase == crate::preview::PreviewPhase::Idle,
            Some(
                PreviewContent::Generic | PreviewContent::Dir { .. } | PreviewContent::Error(_),
            ) => true,
            Some(_) => false,
        }
}

/// Where the section is in the room under the header's card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Room {
    /// Nothing to preview (`alone`): the section is open, as tall as what
    /// it says, up to the whole panel, the toolbar under it.
    Whole,
    /// Over a preview, above it: at most its share of the room, less when
    /// it says less.
    Above,
    /// Over a preview in a room wider than tall (the panel at the bottom,
    /// peek): a column of its share of the room at the preview's left.
    Beside,
}

/// Where the section goes: `wide` is whether the room is wider than tall.
pub fn room(shell: &Shell, wide: bool) -> Room {
    if alone(shell) {
        Room::Whole
    } else if wide {
        Room::Beside
    } else {
        Room::Above
    }
}

/// What a row's links are a group of: the pointer on the row shows them.
const ROW: &str = "properties-row";

/// The section: right under the header's card (`preview::under_card` puts
/// it there, and nothing else draws it), in the `room` it is given. Opened
/// on demand over a preview or above a container's kept rows; always open
/// with nothing to preview. It scrolls past its room. The header's card
/// (or its Properties button) is what opens it, and holds the name, type,
/// size and date, so the groups here repeat none of those.
///
/// Each group's heading folds it (`ui.properties_fold`), as the user left
/// it or as it ships (`Group::folded`), and while folded says at its right
/// what its rows say in short (`Group::gist`: the drive and its free
/// room). A row's links show while the pointer is on it, unless they are
/// all it says or stop or start what runs (`Row::links_shown`); where they
/// go is `row_element`'s.
pub fn render(
    shell: &Shell,
    theme: &Theme,
    room: Room,
    cx: &mut Context<Shell>,
) -> Option<AnyElement> {
    let preview = &shell.preview;
    // Nothing to say (a search named with nothing selected): no section,
    // not an empty one with its scrollbar.
    let nothing = preview.properties.is_empty()
        && preview.selection_summary.is_empty()
        && preview.fields.is_empty();
    if preview.info.name.is_empty() || nothing {
        return None;
    }
    if room != Room::Whole && !preview.properties_open {
        return None;
    }
    let share = preview.properties_share.get(room == Room::Beside);
    let section = div()
        .flex()
        .flex_col()
        .min_h_0()
        .bg(theme.bg)
        .border_color(theme.border);
    let section = match room {
        Room::Whole => section.flex_initial().border_b_1(),
        Room::Above => section
            .flex_initial()
            .max_h(gpui::relative(share))
            .border_b_1(),
        Room::Beside => section
            .flex_none()
            .h_full()
            .w(gpui::relative(share))
            .border_r_1(),
    };
    let several = !preview.selection_summary.is_empty();
    let count = preview.count.as_ref().map(|c| (c.now, c.running));
    // The selection's groups, then the entry's under the cursor: each says
    // what its own drives allow.
    let groups = preview
        .selection_summary
        .iter()
        .map(|group| (true, group))
        .chain(
            preview_group(shell)
                .iter()
                .chain(preview.properties.iter())
                .chain(preview.volume.iter())
                .map(|group| (false, group)),
        )
        .map(|(of_selection, group)| {
            let mut group = group.clone();
            // What the system adds to the mode, read after the rest.
            if group.title.as_ref() == "Permissions" {
                group.rows.extend(preview.access.iter().cloned());
            }
            // The files beside it, or what a companion belongs to.
            if group.title.as_ref() == "Location" {
                group.rows.extend(preview.beside.iter().cloned());
            }
            // What the file takes on its drive, after its size.
            if group.title.as_ref() == "General" {
                group.rows.extend(preview.general.iter().cloned());
            }
            // Nothing on a read-only drive changes who may use it: the
            // entry's drive, or for the selection's, any of theirs.
            let (read_only, why) = if of_selection {
                (
                    preview.selection_read_only,
                    "A drive they are on is read-only",
                )
            } else {
                (preview.read_only, "The drive is read-only")
            };
            if read_only && group.rows.iter().any(|row| row.edits_access) {
                for row in group.rows.iter_mut().filter(|row| row.edits_access) {
                    row.actions.clear();
                }
                group.rows.push(Row::new("Changes", why));
            }
            // A count as it stands. Of the entry shown only when it is
            // all that is selected: with several, the selection's.
            group.rows = std::mem::take(&mut group.rows)
                .into_iter()
                .flat_map(|row| match row.show {
                    Show::Count { selection } if selection || !several => {
                        count_rows(&row.label, count)
                    }
                    Show::Count { .. } => Vec::new(),
                    _ => vec![row],
                })
                .collect();
            group
        })
        .filter(|group| !group.rows.is_empty())
        .collect::<Vec<_>>();
    // What plugins say: in the group each names, or one of its own before
    // the drive's, folded as the source ships it.
    let mut groups = groups;
    for source in &preview.plugin_rows {
        if let Some(group) = groups.iter_mut().find(|g| g.title == source.group) {
            group.rows.push(source.row.clone());
        } else {
            let at = groups
                .iter()
                .position(|g| g.title.as_ref() == "Volume")
                .unwrap_or(groups.len());
            groups.insert(
                at,
                Group {
                    folded: source.folded,
                    ..Group::new(source.group.clone(), vec![source.row.clone()])
                },
            );
        }
    }
    let mut list = div()
        .id("details-list")
        .track_scroll(&preview.properties_scroll)
        .overflow_y_scroll()
        .flex_1()
        .min_h_0()
        .px_3()
        .pb_3()
        .text_xs();
    for (g, group) in groups.into_iter().enumerate() {
        let folded = preview.properties_folds.folded(&group.title, group.folded);
        list = list.child(heading(
            g,
            &group.title,
            folded,
            group.gist.filter(|_| folded),
            theme,
            cx,
        ));
        if folded {
            continue;
        }
        for (r, row) in group.rows.into_iter().enumerate() {
            list = list.child(row_element(shell, row, g * 1000 + r, theme, cx));
        }
    }
    Some(
        section
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(list)
                    .child(crate::elements::scrollbar::vertical(
                        "details-scrollbar",
                        crate::elements::scrollbar::Scrolls::Offset(
                            preview.properties_scroll.clone(),
                        ),
                    )),
            )
            .into_any_element(),
    )
}

/// A group's heading: a heading, not one more label (in the text's own
/// color and weight where the labels are muted), ruled off from the group
/// above it; a click folds the group or unfolds it (saying which, as the
/// heading shows it). While folded, its gist at its right.
fn heading(
    g: usize,
    title: &SharedString,
    folded: bool,
    gist: Option<SharedString>,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> gpui::Div {
    let gist = gist.map(|gist| {
        div()
            .ml_auto()
            .min_w_0()
            .truncate()
            .text_color(theme.text_muted)
            .child(gist)
    });
    let fold = title.clone();
    div()
        .when(g == 0, gpui::Styled::pt_2)
        .when(g > 0, |d| {
            d.mt_1().pt_1().border_t_1().border_color(theme.border)
        })
        .child(
            div()
                .id(("properties-heading", g))
                .flex()
                .items_center()
                .gap_1()
                .py(px(3.))
                .rounded_sm()
                .cursor_pointer()
                .hover(|d| d.bg(theme.hover))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.run(
                        "ui.properties_fold",
                        serde_json::json!({ "group": fold, "fold": !folded }),
                        window,
                        cx,
                    );
                }))
                .child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(crate::controls::glyph(
                            if folded { "chevron-right" } else { "chevron" },
                            theme.text_muted,
                            12.,
                        ))
                        .child(
                            div()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(theme.text)
                                .child(title.clone()),
                        ),
                )
                .children(gist),
        )
}

/// A row's links, each running its command.
fn links(
    actions: Vec<(SharedString, Invocation)>,
    id: usize,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> Option<gpui::Div> {
    (!actions.is_empty()).then(|| {
        div()
            .flex_none()
            .flex()
            .gap_x_3()
            .children(actions.into_iter().enumerate().map(|(a, (words, action))| {
                div()
                    .id(("detail-action", id * 8 + a))
                    .text_color(theme.accent)
                    .hover(gpui::Styled::underline)
                    .child(words)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.run(&action.name, action.args.clone(), window, cx);
                    }))
            }))
    })
}

/// A row: its label, then its value. Links shown without the pointer
/// follow the value on its line, or go under it when the value takes the
/// line (the line wraps, so nothing is measured); a row that says nothing
/// but its links has them where its value would be. Links that show with
/// the pointer come over the end of the row's last line, taking no room
/// while hidden and moving nothing when they show. A click copies the
/// value, unless it says a path lossily (`Row::copies`).
fn row_element(
    shell: &Shell,
    row: Row,
    id: usize,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> gpui::Stateful<gpui::Div> {
    let (label, value, copies) = (row.label.clone(), row.value.clone(), row.copies);
    let shown = row.links_shown();
    let said = match &row.show {
        Show::Text => {
            (!row.value.is_empty()).then(|| div().child(row.value.clone()).into_any_element())
        }
        Show::Tags { tags, given } => (!tags.is_empty())
            .then(|| tag_chips(shell, tags, given, id, theme, cx).into_any_element()),
        Show::Rating { rating, target } => {
            Some(star_picker(*rating, target, id, theme, cx).into_any_element())
        }
        Show::Count { .. } => None,
    };
    let bare = said.is_none() && row.source.is_none();
    let links = links(row.actions, id, theme, cx);
    let (inline, over) = if shown { (links, None) } else { (None, links) };
    div()
        .id(("detail", id))
        .group(ROW)
        .relative()
        .flex()
        .items_start()
        .gap_2()
        .py(px(3.))
        .rounded_sm()
        .cursor_pointer()
        .hover(|d| d.bg(theme.hover))
        .on_click(cx.listener(move |this, _, _, cx| {
            if !copies {
                this.set_status("The path is not text the clipboard can hold", true, cx);
                return;
            }
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(value.to_string()));
            this.set_status(format!("Copied {}", label.to_lowercase()), false, cx);
        }))
        .child(
            div()
                .w(px(84.))
                .flex_none()
                .text_color(theme.text_muted)
                .child(row.label),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_wrap()
                .items_start()
                .gap_x_3()
                .when(!bare, |d| {
                    // It grows into the line, so its text is laid out at
                    // the width it is given, links or not.
                    d.child(
                        div()
                            .flex_grow_1()
                            .min_w_0()
                            .children(said)
                            .when_some(row.source, |d, source| {
                                d.child(div().text_color(theme.text_muted).child(source))
                            }),
                    )
                })
                .children(inline.map(|links| links.when(!bare, gpui::Styled::ml_auto))),
        )
        .children(over.map(|links| {
            links
                .absolute()
                .bottom_0()
                .right_0()
                .py(px(3.))
                .pl_3()
                .rounded_sm()
                .bg(theme.hover)
                .invisible()
                .group_hover(ROW, gpui::Styled::visible)
        }))
}

/// The edge between Properties and the preview, at the start of the
/// preview (`body`, which it wraps): dragged, it sets Properties' share of
/// the room, each way apart; a double click puts it back to half.
pub fn edged(body: AnyElement, room: Room, cx: &mut Context<Shell>) -> AnyElement {
    use crate::elements::edge::{Side, edge};
    let beside = room == Room::Beside;
    div()
        .relative()
        .flex_1()
        .min_h_0()
        .min_w_0()
        .flex()
        .flex_col()
        .child(body)
        .child(edge(
            "properties-resize",
            if beside { Side::Left } else { Side::Top },
            PropertiesResize { beside },
            move |this| this.preview.properties_share.set(beside, SHARE),
            cx,
        ))
        .into_any_element()
}

/// Marker for dragging the edge between Properties and the preview; it
/// knows which way the edge runs.
#[derive(Clone)]
pub struct PropertiesResize {
    beside: bool,
}

impl Shell {
    /// The edge between Properties and the preview follows the pointer
    /// across the room under the card (the bounds are its). The root's
    /// mouse-up remembers where it was left (`resize_hook`).
    pub(crate) fn on_properties_drag(
        &mut self,
        ev: &gpui::DragMoveEvent<PropertiesResize>,
        _: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let beside = ev.drag(cx).beside;
        let (along, size, keep) = if beside {
            (
                ev.event.position.x - ev.bounds.left(),
                ev.bounds.size.width,
                KEEP_WIDTH,
            )
        } else {
            (
                ev.event.position.y - ev.bounds.top(),
                ev.bounds.size.height,
                KEEP_HEIGHT,
            )
        };
        if let Some(share) = crate::elements::edge::share(f32::from(along), f32::from(size), keep) {
            self.preview.properties_share.set(beside, share);
            cx.notify();
        }
    }

    /// Fold a group of Properties, or unfold it (`fold`; else the other
    /// way), for every file from now on.
    pub(crate) fn properties_fold(&mut self, args: &serde_json::Value, cx: &mut Context<Self>) {
        let Some(group) = args.get("group").and_then(serde_json::Value::as_str) else {
            return;
        };
        let shipped = ships_folded(&self.preview, group);
        let fold = args
            .get("fold")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or_else(|| !self.preview.properties_folds.folded(group, shipped));
        self.preview.properties_folds.set(group, fold, shipped);
        self.remember_session();
        cx.notify();
    }
}

/// Whether the group `title` ships folded: as the shell's own group of
/// that title says (`Group::folded`), else as the sources whose rows make
/// it say (`Property::folded`). A group not shown ships unfolded.
pub fn ships_folded(preview: &crate::preview::PreviewState, title: &str) -> bool {
    let own = preview
        .selection_summary
        .iter()
        .chain(preview.properties.iter())
        .chain(preview.volume.iter())
        .find(|group| group.title.as_ref() == title);
    match own {
        Some(group) => group.folded,
        None => preview
            .plugin_rows
            .iter()
            .any(|source| source.group.as_ref() == title && source.folded),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skwir_core::entry::{EntryId, EntryKind};

    /// A name that is not UTF-8 is said lossily, which names another
    /// file: such a row is not copied.
    #[cfg(unix)]
    #[test]
    fn a_path_that_is_not_text_is_said_but_not_copied() {
        let name = skwir_core::os_path::from_bytes(b"caf\xe9.txt".to_vec());
        let path = std::path::Path::new("/data").join(name);
        let row = Row::path("Path", &path);
        assert!(!row.copies && row.value.contains('\u{FFFD}'));
        let folder = Row::path("Folder", path.parent().unwrap());
        assert!(folder.copies, "its folder is text");
    }

    #[test]
    fn an_entry_says_what_it_knows_and_who_said_it() {
        let mut entry = Entry::new(
            EntryId {
                file: skwir_core::FileId { dev: 1, ino: 2 },
                at: 0,
            },
            "it's a café.txt",
            EntryKind::File,
        );
        let meta = &mut entry.metadata;
        meta.set(keys::SIZE, Value::UInt(1_234_567), Provenance::Filesystem);
        meta.set(keys::MODE, Value::UInt(0o100_640), Provenance::Filesystem);
        meta.set(keys::UID, Value::UInt(4_000_000), Provenance::Filesystem);
        meta.set(keys::NLINK, Value::UInt(1), Provenance::Filesystem);
        meta.set(
            "tags",
            Value::List(vec![
                Value::Text("work".into()),
                Value::Text("draft".into()),
            ]),
            Provenance::User,
        );
        meta.set(keys::RATING, Value::UInt(7), Provenance::User);
        meta.set(
            keys::ORIGIN,
            Value::Text("https://example.org/it's a café.txt".into()),
            Provenance::Filesystem,
        );
        meta.set(
            "vision.subject",
            Value::Text("a harbour".into()),
            Provenance::Model {
                name: "vision".into(),
                confidence: 0.82,
            },
        );
        let path = std::path::Path::new("/data/notes/it's a café.txt");
        let groups = entry_groups(&entry, path, Some(&[]), &User::default());
        let find = |title: &str, label: &str| {
            groups
                .iter()
                .find(|group| group.title == title)
                .and_then(|group| group.rows.iter().find(|row| row.label == label))
                .cloned()
        };
        assert_eq!(
            find("General", "Size").unwrap().value,
            "1.2 MB (1,234,567 bytes)"
        );
        assert!(
            find("General", "Name").is_none() && find("General", "Type").is_none(),
            "the header shows them"
        );
        let whole = find("Location", "Path").unwrap();
        assert_eq!(
            whole.value, "/data/notes/it's a café.txt",
            "copied as it is"
        );
        assert!(whole.copies);
        assert_eq!(find("Location", "Folder").unwrap().value, "/data/notes");
        assert_eq!(
            find("Permissions", "Access").unwrap().value,
            "rw-r----- (640)"
        );
        assert_eq!(
            find("Permissions", "Owner").unwrap().value,
            "4000000",
            "no name: the id"
        );
        assert!(
            find("Permissions", "Hard links").is_none(),
            "one link is not news"
        );
        let tags = find("Notes", "Tags").unwrap();
        assert_eq!(
            (tags.value.as_ref(), tags.source.as_ref().map(AsRef::as_ref)),
            ("work, draft", None),
            "notes are the user's: no need to say so"
        );
        assert_eq!(
            tags.show,
            Show::Tags {
                tags: vec!["work".into(), "draft".into()],
                given: Vec::new()
            }
        );
        let rating = find("Notes", "Rating").unwrap();
        assert_eq!(rating.value.as_ref(), "3½ stars");
        assert!(matches!(
            rating.show,
            Show::Rating {
                rating: Some(7),
                ..
            }
        ));
        let comment = find("Notes", "Comment").unwrap();
        assert!(comment.value.is_empty(), "none yet, but a way to add one");
        assert_eq!(comment.actions[0].0.as_ref(), "Add…");
        assert_eq!(
            comment.actions[0].1.args["args"]["entries"][0]["id"]["ino"],
            2
        );
        let origin = find("Source", "Downloaded from").unwrap();
        let links: Vec<&str> = origin.actions.iter().map(|(w, _)| w.as_ref()).collect();
        assert_eq!(links, ["Open", "Copy"]);
        assert!(find("More", "Tags").is_none() && find("More", "Rating").is_none());
        // A category of one tag at a time is a field of its own.
        let fields = ["Status".to_owned()];
        let mut tagged = entry.clone();
        tagged.metadata.set(
            keys::TAGS,
            Value::List(vec![
                Value::Text("work".into()),
                Value::Text("Status/In review".into()),
            ]),
            Provenance::User,
        );
        let with_field = entry_groups(&tagged, path, Some(&fields), &User::default());
        let notes = with_field.iter().find(|g| g.title == "Notes").unwrap();
        let status = notes.rows.iter().find(|r| r.label == "Status").unwrap();
        assert_eq!(status.value.as_ref(), "In review");
        assert_eq!(status.actions[0].1.args["text"], "Status/");
        let tags = notes.rows.iter().find(|r| r.label == "Tags").unwrap();
        assert_eq!(
            tags.value.as_ref(),
            "work",
            "the field's value is not a tag twice"
        );
        // With the tags plugin off, no notes to take and none to show.
        let without = entry_groups(&entry, path, None, &User::default());
        assert!(
            without
                .iter()
                .all(|g| g.title != "Notes" && g.title != "Source")
        );
        assert_eq!(
            find("More", "Subject")
                .unwrap()
                .source
                .as_ref()
                .map(AsRef::as_ref),
            Some("suggested by vision · 82%")
        );
        assert!(
            groups.iter().all(|group| group.title != "Times"),
            "nothing to say"
        );
        // A folder's size is what it holds, counted when asked.
        let folder = Entry::new(
            EntryId {
                file: skwir_core::FileId { dev: 1, ino: 3 },
                at: 0,
            },
            "Ünïcode folder",
            EntryKind::Directory,
        );
        let general = entry_groups(&folder, path, None, &User::default());
        let size = &general.iter().find(|g| g.title == "General").unwrap().rows[0];
        assert_eq!(size.show, Show::Count { selection: false });
    }

    fn entry(ino: u64, name: &str, kind: EntryKind, uid: u64, mode: Option<u64>) -> Entry {
        let mut entry = Entry::new(
            EntryId {
                file: skwir_core::FileId { dev: 1, ino },
                at: 0,
            },
            name,
            kind,
        );
        let meta = &mut entry.metadata;
        meta.set(keys::UID, Value::UInt(uid), Provenance::Filesystem);
        meta.set(keys::GID, Value::UInt(100), Provenance::Filesystem);
        if let Some(mode) = mode {
            meta.set(keys::MODE, Value::UInt(mode), Provenance::Filesystem);
        }
        entry
    }

    fn said(rows: &[Row]) -> Vec<(String, String, Vec<String>)> {
        rows.iter()
            .map(|r| {
                (
                    r.label.to_string(),
                    r.value.to_string(),
                    r.actions.iter().map(|(w, _)| w.to_string()).collect(),
                )
            })
            .collect()
    }

    #[test]
    fn a_count_says_how_far_it_went() {
        use skwir_core::fs::count::Count;
        let row = |label: &str, value: &str, actions: &[&str]| {
            (
                label.to_owned(),
                value.to_owned(),
                actions.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>(),
            )
        };
        assert_eq!(
            said(&count_rows("Size", None)),
            [row("Size", "", &["Count"])]
        );
        let so_far = Count {
            size: 3 << 20,
            allocated: 4 << 20,
            files: 1500,
            folders: 1,
            ..Count::default()
        };
        let running = count_rows("Size", Some((so_far, true)));
        assert_eq!(
            said(&running),
            [
                row("Size", "3.0 MB so far…", &["Stop"]),
                row("On disk", "4.0 MB", &[]),
                row("In all", "1,500 files, 1 folder", &[]),
            ]
        );
        assert_eq!(running[0].actions[0].1.args["stop"], true);
        let stopped = count_rows("Total size", Some((so_far, false)));
        assert_eq!(
            said(&stopped[..1]),
            [row(
                "Total size",
                "At least 3.0 MB, stopped",
                &["Count again"]
            )]
        );
        let done = Count {
            done: true,
            size: 1234,
            ..so_far
        };
        // Done, it counts again when asked: what a folder holds changes.
        let exact = count_rows("Size", Some((done, false)));
        assert_eq!(
            said(&exact[..1]),
            [row("Size", "1.2 KB (1,234 bytes)", &["Count again"])]
        );
        assert!(exact[0].links_shown(), "it starts what runs");
        assert_eq!(exact[0].actions[0].1.args, serde_json::json!({}));
        let partial = Count {
            unreadable: 2,
            unknown: 3,
            other_drives: 1,
            ..done
        };
        assert_eq!(
            said(&count_rows("Size", Some((partial, false)))),
            [
                row("Size", "At least 1.2 KB", &["Count again"]),
                row("On disk", "4.0 MB", &[]),
                row("In all", "1,500 files, 1 folder", &[]),
                row("Unreadable", "2 folders could not be listed", &[]),
                row("Unknown", "3 items could not be read", &[]),
                row("Not counted", "1 drive mounted inside", &[]),
            ]
        );
        // An entry it could not look at alone makes the total a bound.
        let unknown = Count { unknown: 1, ..done };
        assert_eq!(
            count_rows("Size", Some((unknown, false)))[0].value.as_ref(),
            "At least 1.2 KB"
        );
    }

    #[test]
    fn links_show_with_the_pointer_unless_they_are_all_a_row_says_or_stop_what_runs() {
        use skwir_core::fs::count::Count;
        let shown = |rows: &[Row], label: &str| {
            rows.iter()
                .find(|r| r.label == label)
                .map(Row::links_shown)
                .unwrap()
        };
        assert!(
            shown(&count_rows("Size", None), "Size"),
            "Count is all it says"
        );
        let counting = Count {
            size: 1 << 20,
            ..Count::default()
        };
        assert!(
            shown(&count_rows("Size", Some((counting, true))), "Size"),
            "Stop stops what runs"
        );
        assert!(
            shown(&count_rows("Size", Some((counting, false))), "Size"),
            "Count again starts it again"
        );
        let me = User {
            id: Some(1000),
            groups: vec![100],
        };
        let mut file = entry(
            10,
            "it's a café.txt",
            EntryKind::File,
            1000,
            Some(0o100_644),
        );
        let path = std::path::Path::new("/data/it's a café.txt");
        let rows = |entry: &Entry| -> Vec<Row> {
            entry_groups(entry, path, Some(&[]), &me)
                .into_iter()
                .flat_map(|g| g.rows)
                .collect()
        };
        let none = rows(&file);
        assert!(shown(&none, "Tags"), "Add… to no tags");
        assert!(shown(&none, "Comment"), "Add… to no comment");
        assert!(!shown(&none, "Owner can"), "Change… shows with the pointer");
        assert!(!shown(&none, "Can run"), "Allow running too");
        assert!(!shown(&none, "Access"), "nor the mode's");
        file.metadata.set(
            keys::COMMENT,
            Value::Text("a draft".into()),
            Provenance::User,
        );
        file.metadata.set(
            keys::TAGS,
            Value::List(vec![Value::Text("work".into())]),
            Provenance::User,
        );
        let noted = rows(&file);
        assert!(!shown(&noted, "Tags"), "Edit… shows with the pointer");
        assert!(!shown(&noted, "Comment"));
    }

    #[test]
    fn groups_fold_as_shipped_until_the_user_folds_them() {
        // How each ships is the group's to say: the shell names no
        // plugin's (a source's rows say theirs, `Property::folded`).
        let mut folds = Folds::default();
        assert!(folds.folded("Volume", true) && folds.folded("A source's", true));
        assert!(!folds.folded("Permissions", false));
        folds.set("A source's", false, true);
        folds.set("Times", true, false);
        assert!(!folds.folded("A source's", true) && folds.folded("Times", false));
        folds.set("A source's", true, true);
        assert_eq!(
            serde_json::to_value(&folds).unwrap(),
            serde_json::json!({ "Times": true }),
            "only what differs from how it ships is kept"
        );
        let back: Folds = serde_json::from_value(serde_json::json!({ "Volume": false })).unwrap();
        assert!(!back.folded("Volume", true) && back.folded("A source's", true));
    }

    #[test]
    fn a_source_says_its_group_ships_folded() {
        let mut preview = crate::preview::PreviewState::default();
        assert!(
            !ships_folded(&preview, "Checksums"),
            "not shown, not folded"
        );
        let row = |group: &str, folded: bool| SourceRow {
            group: group.to_owned().into(),
            folded,
            row: Row::new("SHA-256", ""),
        };
        preview.plugin_rows = vec![row("Checksums", true), row("General", true)];
        preview.properties = vec![Group::new("General", vec![Row::new("Size", "1 B")])];
        assert!(ships_folded(&preview, "Checksums"), "as its rows say");
        assert!(
            !ships_folded(&preview, "General"),
            "a source joining the shell's group does not fold it"
        );
        preview.volume = Some(Group {
            folded: true,
            ..Group::new("Volume", vec![Row::new("Free", "1 GB")])
        });
        assert!(
            ships_folded(&preview, "Volume"),
            "the shell's own, said by its group"
        );
    }

    #[test]
    fn several_entries_say_what_they_share() {
        let pane = skwir_core::workspace::PaneId(3);
        let me = User {
            id: Some(1000),
            groups: vec![100],
        };
        let a = entry(10, "a's file.txt", EntryKind::File, 1000, Some(0o100_644));
        let b = entry(11, "b café", EntryKind::File, 1000, Some(0o100_755));
        let groups = selection_groups(&[&a, &b], 2, Some(2048), pane, &me);
        let titles: Vec<&str> = groups.iter().map(|g| g.title.as_ref()).collect();
        assert_eq!(
            titles,
            ["Selection", SHARED_PERMISSIONS],
            "not the Permissions of the entry under the cursor, folded apart"
        );
        let find = |groups: &[Group], label: &str| {
            groups
                .iter()
                .flat_map(|g| &g.rows)
                .find(|r| r.label == label)
                .cloned()
                .unwrap()
        };
        assert_eq!(find(&groups, "Selected").value, "2 items");
        assert_eq!(find(&groups, "Total size").value, "2.0 KB");
        assert_eq!(find(&groups, "Owner can").value, "Read and write");
        assert_eq!(find(&groups, "Access").value, "Mixed");
        let run = find(&groups, "Can run");
        assert_eq!(run.value, "Mixed");
        assert_eq!(run.actions[0].0.as_ref(), "Allow running");
        assert_eq!(
            run.actions[0].1.args,
            serde_json::json!({ "pane": pane, "mode": "ugo+x" }),
            "the selection, by its pane"
        );
        let dir = entry(12, "a folder", EntryKind::Directory, 0, Some(0o040_755));
        let groups = selection_groups(&[&a, &b, &dir], 3, Some(2048), pane, &me);
        assert_eq!(find(&groups, "Of which").value, "1 folder, 2 files");
        assert_eq!(
            find(&groups, "Total size").show,
            Show::Count { selection: true },
            "a folder's files are counted, not left out"
        );
        assert_eq!(find(&groups, "Owner").value, "Mixed");
        assert_eq!(
            find(&groups, "Changes").value,
            "Only their owner can change who may use them"
        );
        assert!(
            groups
                .iter()
                .flat_map(|g| &g.rows)
                .all(|r| r.actions.is_empty() || !r.edits_access)
        );
        let unknown = entry(13, "not stat-ed yet", EntryKind::File, 1000, None);
        let groups = selection_groups(&[&a, &unknown], 2, None, pane, &me);
        assert_eq!(groups.len(), 1, "no Permissions before every entry says it");
        assert!(
            groups[0].rows.iter().all(|r| r.label != "Total size"),
            "nothing said before every entry says it"
        );
    }

    fn subject(mode: u64, uid: u64, folder: bool) -> Subject {
        Subject {
            mode,
            uid,
            gid: 100,
            folder,
            regular: !folder,
        }
    }

    #[test]
    fn access_is_said_in_words_with_the_ways_to_change_it() {
        let me = User {
            id: Some(1000),
            groups: vec![100, 10],
        };
        let target = Targets::Entry(EntryRef::new(
            skwir_core::FileId { dev: 1, ino: 2 },
            "/data/it's a script.sh",
        ));
        let rows = access_rows(&[subject(0o644, 1000, false)], &target, false, &me);
        assert_eq!(
            said(&rows)
                .into_iter()
                .map(|(l, v, a)| format!("{l}: {v} {a:?}"))
                .collect::<Vec<_>>(),
            [
                "Owner: 1000 []",
                "Group: 100 [\"Change…\"]",
                "Owner can: Read and write [\"Change…\"]",
                "Group can: Read [\"Change…\"]",
                "Others can: Read [\"Change…\"]",
                "Can run: No one [\"Allow running\"]",
                "Access: rw-r--r-- (644) [\"Change…\"]",
            ]
        );
        let allow = &rows[5].actions[0].1;
        assert_eq!(allow.name.as_ref(), "set_access");
        assert_eq!(allow.args["mode"], "ugo+x", "everyone who reads it");
        assert_eq!(allow.args["entries"][0]["id"]["ino"], 2);
        // A level's choices keep the run bit, the current one said.
        let owner = &rows[2].actions[0].1;
        assert_eq!(owner.name.as_ref(), "ui.choose");
        let choices = owner.args["choices"].as_array().unwrap();
        assert_eq!(choices[1]["command"]["args"]["mode"], "u+r,u-w");
        assert_eq!(choices[2]["detail"], "Now");
        assert!(owner.args.get("option").is_none(), "a file holds nothing");
        // The group goes to one of the user's.
        let groups = rows[1].actions[0].1.args["choices"].as_array().unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0]["command"]["args"]["group"], 10);
        // The mode as chmod writes it, ready to edit.
        assert_eq!(rows[6].actions[0].1.args["text"], "644");
        assert!(
            rows.iter()
                .filter(|r| !r.actions.is_empty())
                .all(|r| r.edits_access)
        );

        // A folder: its levels in a folder's words, a switch that reaches
        // inside, what its set-group-ID bit does, no running.
        let rows = access_rows(&[subject(0o2775, 1000, true)], &target, false, &me);
        let row = |label: &str| rows.iter().find(|r| r.label == label).unwrap();
        assert_eq!(row("Group can").value, "Create and delete items");
        assert_eq!(row("Others can").value, "Open items");
        assert_eq!(row("Special").value, "New items take the folder's group");
        assert!(rows.iter().all(|r| r.label != "Can run"));
        let others = &row("Others can").actions[0].1.args;
        assert_eq!(others["option"]["field"], "inside");
        assert_eq!(others["choices"][2]["command"]["args"]["mode"], "o=rX");

        // Someone else's, the trash's, or where users are no numbers.
        let theirs = access_rows(&[subject(0o644, 0, false)], &target, false, &me);
        assert!(theirs.iter().all(|r| r.actions.is_empty()));
        assert_eq!(
            theirs.last().unwrap().value,
            "Only its owner can change who may use it"
        );
        let trashed = access_rows(&[subject(0o644, 1000, false)], &target, true, &me);
        assert_eq!(
            trashed.last().unwrap().value,
            "Restore it to change who may use it"
        );
        let nobody = access_rows(
            &[subject(0o644, 1000, false)],
            &target,
            false,
            &User::default(),
        );
        assert!(
            nobody
                .iter()
                .all(|r| r.actions.is_empty() && r.label != "Changes")
        );
        // Root may change anyone's.
        let root = User {
            id: Some(0),
            groups: vec![0],
        };
        assert!(
            !access_rows(&[subject(0o644, 1000, false)], &target, false, &root)[2]
                .actions
                .is_empty()
        );
    }

    #[test]
    fn flags_say_why_in_words() {
        use skwir_core::fs::stat::Flags;
        assert!(flag_rows(Flags::default()).is_empty());
        let rows = flag_rows(Flags {
            immutable: true,
            compressed: true,
            ..Flags::default()
        });
        assert_eq!(
            said(&rows)
                .into_iter()
                .map(|(l, _, _)| l)
                .collect::<Vec<_>>(),
            ["Immutable", "Compressed"]
        );
    }
}
