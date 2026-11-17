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
//! (`Folds`), each then saying its gist on its heading (`Group::gist`),
//! and a row's links show under the pointer, so it says much in little
//! room.
//!
//! Facts come from three places, none of them read while painting: the entry's
//! metadata, with its provenance, collected when the entry or its listing
//! changes (`entry_groups`); what the preview already holds in memory (a
//! picture's size, a document's pages, a film's length); and what the format
//! itself told us (`Inspection::details`). Properties never starts a scan
//! by itself: what a folder holds in all is counted when asked
//! (`ui.properties_count`, `Show::Count`).

use crate::sizes::TextSizes;
use crate::sizes::sized;
use gpui::{AnyElement, Context, SharedString, div, prelude::*};
use skwir_core::command::Invocation;
use skwir_core::entry::{Entry, Provenance, Value, keys};
use skwir_core::op::EntryRef;
use skwir_core::words;
use skwir_core::words::{Name, Size, Words};

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
    /// What it says in short on its group's heading while the group is
    /// folded (`Group::gist`); most rows say nothing there.
    pub gist: Option<Gist>,
    /// What its gist follows where the heading has room for all of it
    /// (`Group::gist_led`): what the gist is, where its words alone do not
    /// say ("Modified" before a date, "Comment:" before a comment).
    pub lead: Option<Lead>,
    /// What its label means, in a tooltip while the pointer rests on the
    /// row: which of a file's times it is.
    pub tip: Option<SharedString>,
}

/// What a row's gist is, said before it on a folded heading with room
/// (`Row::lead`): each a whole message placing the gist.
#[derive(Clone, Debug, PartialEq)]
pub enum Lead {
    /// The drive it is on: `On Data`.
    On,
    OwnedBy,
    Lasts,
    DeletedFrom,
    Modified,
    Comment,
    DownloadedFrom,
    FoundOn,
    /// A category holding one tag at a time, by its name: `Status: Draft`.
    Field(String),
}

impl Lead {
    /// It, before `gist`.
    fn said(&self, gist: &str) -> Words {
        let gist = gist.to_owned();
        match self {
            Lead::On => words!(
                properties_lead_on = "On { $gist }",
                gist = gist,
                note = "A folded heading of Properties: the drive a file is on (On Data)."
            ),
            Lead::OwnedBy => words!(
                properties_lead_owned_by = "Owned by { $gist }",
                gist = gist,
                note = "A folded heading of Properties: who owns the file (a user's name)."
            ),
            Lead::Lasts => words!(
                properties_lead_lasts = "Lasts { $gist }",
                gist = gist,
                note = "A folded heading of Properties: how long a sound or a film lasts (3:45)."
            ),
            Lead::DeletedFrom => words!(
                properties_lead_deleted_from = "Deleted from { $gist }",
                gist = gist,
                note = "A folded heading of Properties: the folder a file of the trash was deleted from (a path)."
            ),
            Lead::Modified => words!(
                properties_lead_modified = "Modified { $gist }",
                gist = gist,
                note = "A folded heading of Properties: when the file last changed (2024-03-01 14:02)."
            ),
            Lead::Comment => words!(
                properties_lead_comment = "Comment: { $gist }",
                gist = gist,
                note = "A folded heading of Properties: the first line of the comment on the file."
            ),
            Lead::DownloadedFrom => words!(
                properties_lead_downloaded_from = "Downloaded from { $gist }",
                gist = gist,
                note = "A folded heading of Properties: the site a file was downloaded from (example.org)."
            ),
            Lead::FoundOn => words!(
                properties_lead_found_on = "Found on { $gist }",
                gist = gist,
                note = "A folded heading of Properties: the page that led to the download (example.org)."
            ),
            Lead::Field(field) => words!(
                properties_lead_field = "{ $field }: { $gist }",
                field = field.clone(),
                gist = gist,
                note = "A folded heading of Properties: a category holding one tag at a time, by its name as the user wrote it, then the file's tag in it (Status: Draft)."
            ),
        }
    }
}

/// A row's part of its folded group's heading.
#[derive(Clone, Debug, PartialEq)]
pub enum Gist {
    /// Words, cut at their end where the heading is short of room.
    Said(SharedString),
    /// A path, cut at its start: its end says which file.
    Path(SharedString),
}

impl Gist {
    /// What it says, however it is cut.
    pub fn words(&self) -> &SharedString {
        match self {
            Gist::Said(words) | Gist::Path(words) => words,
        }
    }
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
            gist: None,
            lead: None,
            tip: None,
        }
    }

    /// A row known to come before its value has (what a source said it
    /// answers, what the shell reads on a worker, what a preview on its way
    /// will say): its label in place and its value empty, so the answer
    /// fills it in and moves nothing below it (ADR-058).
    pub fn reserved(label: impl Into<SharedString>) -> Self {
        Row {
            copies: false,
            ..Row::new(label, "")
        }
    }

    /// It says `words` on its folded group's heading.
    pub(crate) fn gist(mut self, words: impl Into<SharedString>) -> Self {
        self.gist = Some(Gist::Said(words.into()));
        self
    }

    /// It says its value on its folded group's heading.
    pub(crate) fn in_gist(self) -> Self {
        let value = self.value.clone();
        self.gist(value)
    }

    /// Its gist follows `lead` where the folded heading has room for it
    /// all (`Row::lead`).
    pub(crate) fn led(mut self, lead: Lead) -> Self {
        self.lead = Some(lead);
        self
    }

    /// Its label explained in a tooltip (`Row::tip`).
    fn tip(mut self, words: &Words) -> Self {
        self.tip = Some(words.say().into());
        self
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
    fn edit(mut self, words: &Words, command: Invocation) -> Self {
        self.actions.push((words.say().into(), command));
        self.edits_access = true;
        self
    }

    fn action(mut self, words: &Words, command: Invocation) -> Self {
        self.actions.push((words.say().into(), command));
        self
    }

    /// A provider's fact, with its link, said where the row is made.
    fn of(detail: &skwir_core::inspection::Detail) -> Self {
        Row {
            actions: detail
                .action
                .iter()
                .map(|(words, action)| (words.say().into(), action.clone()))
                .collect(),
            ..Row::new(detail.label.say(), detail.value.clone())
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    /// What it is known by, the same in every language: what is folded
    /// (`Folds`), and the group a source's rows join (`Property::group`).
    /// Properties' own are `group_keys`; a source's own group's is its name.
    pub key: SharedString,
    /// Its heading, said where it is made.
    pub title: SharedString,
    pub rows: Vec<Row>,
    /// It ships folded, until the user unfolds it: what is seldom looked
    /// at (the drive's, the shell's own; a source's whose rows say so,
    /// `Property::folded`).
    pub folded: bool,
}

impl Group {
    /// The group known by `key` (`group_keys`), headed by core's words for
    /// it; a source's own group, by its key, headed by the source's words
    /// (`headed`).
    pub fn new(key: impl Into<SharedString>, rows: Vec<Row>) -> Self {
        Group::headed(key, None, rows)
    }

    /// The group known by `key`, headed by `heading` where it is a source's
    /// own, else by core's words for one of Properties' own.
    pub fn headed(key: impl Into<SharedString>, heading: Option<&Words>, rows: Vec<Row>) -> Self {
        let key = key.into();
        let title = heading
            .cloned()
            .or_else(|| group_keys::words_of(&key))
            .map_or_else(|| key.clone(), |title| title.say().into());
        Group {
            title,
            key,
            rows,
            folded: false,
        }
    }

    /// What its heading says at its right while it is folded: what its
    /// rows say in short (`Row::gist`: the size, the path, the drive and
    /// its free room), in their order. Taken once its rows are all there
    /// (`render` adds some), so a count says how far it went. Cut at its
    /// start when a path is part of it.
    pub fn gist(&self) -> Option<Gist> {
        self.said(false)
    }

    /// Its tags, and the colors their file gives them: a folded heading
    /// draws them as chips, as every view does, before its gist's words
    /// (`heading`). `None` without tags.
    pub fn tags(&self) -> Option<(&[String], &[(String, usize)])> {
        self.rows.iter().find_map(|row| match &row.show {
            Show::Tags { tags, given } if !tags.is_empty() => Some((&tags[..], &given[..])),
            _ => None,
        })
    }

    /// Its gist with each part after its lead (`Row::lead`), said instead
    /// where the heading has room for all of it; `None` when no part has
    /// one, so it would say no more.
    pub fn gist_led(&self) -> Option<Gist> {
        self.rows
            .iter()
            .any(|row| row.gist.is_some() && row.lead.is_some())
            .then(|| self.said(true))
            .flatten()
    }

    /// Its rows' gists in their order, each after its lead when `leads`.
    fn said(&self, leads: bool) -> Option<Gist> {
        let parts: Vec<(Option<&Lead>, &Gist)> = self
            .rows
            .iter()
            .filter_map(|row| Some((row.lead.as_ref().filter(|_| leads), row.gist.as_ref()?)))
            .collect();
        if parts.is_empty() {
            return None;
        }
        let words: SharedString = parts
            .iter()
            .map(|(lead, part)| match lead {
                Some(lead) => lead.said(part.words()).say(),
                None => part.words().to_string(),
            })
            .collect::<Vec<_>>()
            .join(" · ")
            .into();
        Some(
            if parts.iter().any(|(_, part)| matches!(part, Gist::Path(_))) {
                Gist::Path(words)
            } else {
                Gist::Said(words)
            },
        )
    }
}

fn group(key: &'static str, rows: Vec<Row>) -> Option<Group> {
    (!rows.is_empty()).then(|| Group::new(key, rows))
}

/// What Properties' own groups are known by (`Group::key`), the same in
/// every language, as the sources name the groups they join and as the
/// window remembers what is folded: core's, beside their headings'
/// words (`words_of`).
pub use skwir_core::registry::property_groups as group_keys;

/// The title of the Permissions group of several entries: what they
/// share, apart from the Permissions of the entry the cursor is on.
pub const SHARED_PERMISSIONS: &str = group_keys::SHARED_PERMISSIONS;

/// The groups the user folded or unfolded, by key (`Group::key`); one it does not name
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

/// A row a property source answered (`plugin_rows`): the group it joins
/// (by key), the heading of a group of the source's own, whether that
/// group ships folded, and the row.
#[derive(Clone, Debug, PartialEq)]
pub struct SourceRow {
    pub group: SharedString,
    pub heading: Option<Words>,
    pub folded: bool,
    pub row: Row,
}

/// Properties' share of the room over a preview, until its edge is
/// dragged, and again after a double click on it: at most half above the
/// preview (less when it says less), half beside it.
pub const SHARE: f32 = 0.5;
/// What each side keeps, whatever the drag: a heading and a few rows of
/// Properties, the preview's toolbar and some of it; a column wide enough
/// for a label and its value. At 100 %, drawn at the interface's size
/// (`keep`).
const KEEP_HEIGHT: f32 = crate::preview_bar::HEIGHT + crate::views::list::ROW_HEIGHT * 3.;
const KEEP_WIDTH: f32 = 200.;

/// What each side keeps, in pixels: Properties' rows and the preview's
/// bar grow with the interface's text (ADR-117).
fn keep(theme: &Theme, beside: bool) -> f32 {
    let scale = theme.sizes.scale(crate::sizes::Part::Interface);
    scale.f(if beside { KEEP_WIDTH } else { KEEP_HEIGHT })
}

fn text_of(value: &Value) -> String {
    match value {
        Value::Bool(v) => if *v {
            words!(
                properties_yes = "Yes",
                note = "The value of a field of a file that is yes or no, in Properties' More."
            )
        } else {
            words!(
                properties_no = "No",
                note = "The value of a field of a file that is yes or no, in Properties' More."
            )
        }
        .say(),
        Value::Int(v) => v.to_string(),
        Value::UInt(v) => v.to_string(),
        Value::Float(v) => format!("{v:.3}"),
        Value::Text(v) => v.to_string(),
        Value::Time(v) => crate::format::exact_time(*v),
        // Transitional: a list argument (ADR-139).
        Value::List(items) => items.iter().map(text_of).collect::<Vec<_>>().join(", "),
    }
}

fn source_of(provenance: &Provenance) -> Option<SharedString> {
    match provenance {
        Provenance::Filesystem => None,
        Provenance::User => Some(
            words!(
                properties_set_by_you = "set by you",
                note = "Under a value in Properties: the user gave it."
            )
            .say()
            .into(),
        ),
        Provenance::Plugin { name } => Some(
            words!(
                properties_from_plugin = "from { $plugin }",
                plugin = name.to_string(),
                note = "Under a value in Properties: which plugin gave it, by its name."
            )
            .say()
            .into(),
        ),
        Provenance::Model { name, confidence } => Some(
            words!(
                properties_suggested_by = "suggested by { $model } · { $confidence }",
                model = name.to_string(),
                confidence = format!("{:.0}%", confidence * 100.),
                note = "Under a value in Properties: a model suggested it, by its name, and how sure it is (80%)."
            )
            .say()
            .into(),
        ),
    }
}

/// What a field a plugin, an agent or the user attached is called: its key
/// as written, since nothing gives it words (ADR-139, 8).
fn label_of(key: &str) -> String {
    key.to_owned()
}

/// The fields Properties names itself; any other is shown under More.
const CORE: [&str; 23] = [
    keys::SIZE,
    keys::MODIFIED,
    keys::CHANGED,
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
    keys::COMPANIONS,
    keys::COMPANION_OF,
    keys::COMPANION_OF_GONE,
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
fn notes_groups(entry: &Entry, path: &std::path::Path, noting: Noting<'_>) -> [Option<Group>; 2] {
    let fields = noting.fields;
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
            let name = skwir_core::metadata::tag_leaf(field);
            Some(
                Row::new(name.to_owned(), value.to_owned())
                    .in_gist()
                    .led(Lead::Field(name.to_owned()))
                    .action(
                        &label::change(),
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
        // No gist: a folded heading draws them as chips (`Group::tags`).
        // Transitional: a list argument (ADR-139).
        ..Row::new(
            words!(
                properties_tags = "Tags",
                note = "A row's label in Properties: the file's tags."
            )
            .say(),
            tags.join(", "),
        )
    };
    tags_row = tags_row.action(
        &if tags.is_empty() {
            label::add()
        } else {
            label::edit()
        },
        Invocation::with_args("ui.tags_prompt", &serde_json::json!({ "entries": target })),
    );
    let rating = meta.value(keys::RATING).and_then(Value::as_u64);
    let rating_row = Row {
        show: Show::Rating {
            rating,
            target: EntryRef::new(entry.file(), path),
        },
        gist: rating.map(|r| Gist::Said(crate::tags::stars_said(r).say().into())),
        ..Row::new(
            words!(
                properties_rating = "Rating",
                note = "A row's label in Properties: the stars the file was given."
            )
            .say(),
            rating.map_or_else(String::new, |r| crate::tags::stars_said(r).say()),
        )
    };
    let comment = text(keys::COMMENT);
    let comment_words = words!(
        properties_comment = "Comment",
        note = "A row's label in Properties, and the title of where it is written: the comment on the file."
    );
    let comment_label = comment_words.say();
    // What asks for the comment takes its words as their data (`ui.ask`'s
    // arguments, said in the window's language).
    let mut ask = serde_json::json!({
        "title": comment_words.to_data(),
        "accept": words!(
            properties_comment_save = "Save",
            note = "The button that keeps the comment written for a file."
        )
        .to_data(),
        "placeholder": words!(
            properties_comment_placeholder = "A comment on this item (empty removes it)",
            note = "The placeholder of the field a file's comment is written in."
        )
        .to_data(),
        "command": "notes.comment",
        "field": "text",
        "args": { "entries": target },
    });
    if let Some(comment) = &comment {
        ask["text"] = comment.as_str().into();
    }
    // Its first line, which the heading cuts where room is short.
    let comment_row = Row {
        gist: comment
            .as_deref()
            .and_then(|c| c.lines().map(str::trim).find(|line| !line.is_empty()))
            .map(|line| Gist::Said(line.to_owned().into())),
        lead: Some(Lead::Comment),
        source: comment
            .as_ref()
            .and(meta.get(keys::COMMENT))
            .and_then(|f| note_source(&f.provenance)),
        ..Row::new(comment_label, comment.clone().unwrap_or_default())
    }
    .action(
        &if comment.is_some() {
            label::edit()
        } else {
            label::add()
        },
        Invocation::with_args("ui.ask", &ask),
    );
    // A URL is opened only when asked, in the browser: skwir fetches
    // nothing a file names (ADR-020).
    let link = |(label, lead, what): (Words, Lead, Words), url: String| {
        let row = Row::new(label.say(), url.clone())
            .gist(host_of(&url).to_owned())
            .led(lead);
        let row = if url.starts_with("https://") || url.starts_with("http://") {
            row.action(
                &words!(
                    properties_open_link = "Open",
                    note = "A link after a web address in Properties: open it in the browser."
                ),
                Invocation::with_args("ui.open_url", &serde_json::json!({ "url": url })),
            )
        } else {
            row
        };
        // What it copied as words' data, said where the toast is drawn.
        row.action(
            &words!(
                properties_copy_link = "Copy",
                note = "A link after a web address in Properties: copy it."
            ),
            Invocation::with_args(
                "ui.copy_text",
                &serde_json::json!({ "text": url, "what": what.to_data() }),
            ),
        )
    };
    let source = [
        text(keys::ORIGIN).map(|url| {
            link(
                (
                    words!(
                        properties_downloaded_from = "Downloaded from",
                        note = "A row's label in Properties: the web address a file was downloaded from."
                    ),
                    Lead::DownloadedFrom,
                    words!(
                        properties_downloaded_from_what = "downloaded from",
                        note = "What was copied, inside a sentence (Copied downloaded from): the web address a file was downloaded from."
                    ),
                ),
                url,
            )
        }),
        text(keys::REFERRER).map(|url| {
            link(
                (
                    words!(
                        properties_found_on = "Found on",
                        note = "A row's label in Properties: the web page that led to a file's download."
                    ),
                    Lead::FoundOn,
                    words!(
                        properties_found_on_what = "found on",
                        note = "What was copied, inside a sentence (Copied found on): the web page that led to a file's download."
                    ),
                ),
                url,
            )
        }),
    ]
    .into_iter()
    .flatten()
    .collect();
    // The stores the field names by id, said by their titles.
    let titles = |key| match meta.value(key) {
        Some(Value::List(items)) => {
            let ids: Vec<&str> = items.iter().filter_map(Value::as_str).collect();
            noting
                .stores
                .titles(&ids)
                .iter()
                .map(Words::say)
                .collect::<Vec<_>>()
        }
        _ => Vec::new(),
    };
    let mut rows = vec![tags_row];
    rows.extend(field_rows);
    rows.extend([rating_row, comment_row]);
    // Where the notes are, and the stores that say otherwise (ADR-027).
    let kept = titles(keys::NOTES_KEPT);
    // Transitional: a list argument (ADR-139).
    if !kept.is_empty() {
        rows.push(Row::new(
            words!(
                properties_kept_in = "Kept in",
                note = "A row's label in Properties: the stores the file's notes are kept in (its attributes, a sidecar)."
            )
            .say(),
            kept.join("; "),
        ));
    }
    let differ = titles(keys::NOTES_DIFFER);
    if !differ.is_empty() {
        rows.push(Row::new(
            words!(
                properties_differs_in = "Differs in",
                note = "A row's label in Properties: the stores whose notes on the file say otherwise."
            )
            .say(),
            differ.join("; "),
        )
        .action(
            &words!(
                properties_agree = "Make them agree",
                note = "A link after the stores whose notes say otherwise: write the same notes in all."
            ),
            Invocation::with_args("notes.agree", &serde_json::json!({ "entries": target })),
        ));
    }
    [
        group(group_keys::NOTES, rows),
        group(group_keys::SOURCE, source),
    ]
}

/// What Properties says of a file's notes, while a store keeps some: the
/// categories it holds one tag at a time of (as fields), and the stores,
/// which say their titles by id where the notes are kept
/// (`keys::NOTES_KEPT`).
#[derive(Clone, Copy)]
pub struct Noting<'a> {
    pub fields: &'a [String],
    pub stores: &'a skwir_core::notes::NoteStores,
}

/// What the entry itself says; with `notes`, what the user noted on it
/// (the tags plugin is on). Pure, so it is tested without a window.
pub fn entry_groups(
    entry: &Entry,
    path: &std::path::Path,
    notes: Option<Noting<'_>>,
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
            ..Row::new(label::size().say(), "")
        }]
    } else {
        match uint(keys::SIZE) {
            Some(size) => {
                vec![Row::new(label::size().say(), exact_size(size)).gist(format::size(size))]
            }
            // Not stat-ed yet: it comes, in its place.
            None if !meta.is_hydrated() => vec![Row::reserved(label::size().say())],
            None => Vec::new(),
        }
    };

    let mut location = Vec::new();
    // A file of the trash: where restoring it puts it back.
    if let Some(from) = meta.value(keys::TRASHED_FROM).and_then(Value::as_str) {
        location.push(
            Row::new(
                words!(
                    properties_deleted_from = "Deleted from",
                    note = "A row's label in Properties, for a file of the trash: the folder it was deleted from, where restoring it puts it back."
                )
                .say(),
                from.to_owned(),
            )
            .led(Lead::DeletedFrom)
            .action(
                &words!(
                    properties_show_folder = "Show",
                    note = "A link after the folder a file of the trash was deleted from: go there."
                ),
                Invocation::with_args("navigate", &serde_json::json!({ "location": from })),
            ),
        );
    }
    // The whole path, to copy with a click; a place (a tag's) has none.
    if !entry.is_place() && path.is_absolute() {
        location.push(Row::path(
            words!(
                properties_path = "Path",
                note = "A row's label in Properties: the file's whole path."
            )
            .say(),
            path,
        ));
    }
    if let Some(parent) = path.parent() {
        location.push(Row::path(
            words!(
                properties_folder = "Folder",
                note = "A row's label in Properties: the folder the file is in, its path."
            )
            .say(),
            parent,
        ));
    }
    if let Some(target) = meta.value(keys::LINK_TARGET).and_then(Value::as_str) {
        location.push(Row::new(
            words!(
                properties_links_to = "Links to",
                note = "A row's label in Properties, for a link: the path it leads to."
            )
            .say(),
            target.to_owned(),
        ));
    }
    // What is beside it (ADR-028), read with its metadata wherever it is
    // shown (`companions::BesideProvider`): its companions, or for one,
    // the file it belongs to.
    if let Some(Value::List(beside)) = meta.value(keys::COMPANIONS) {
        let names: Vec<&str> = beside.iter().filter_map(Value::as_str).collect();
        if !names.is_empty() {
            // Transitional: a list argument (ADR-139).
            location.push(Row::new(
                words!(
                    properties_beside_it = "Beside it",
                    note = "A row's label in Properties: the files beside it that keep its notes (a sidecar, a Mac's ._ file)."
                )
                .say(),
                names.join(", "),
            ));
        }
    }
    if let Some(owner) = meta.value(keys::COMPANION_OF).and_then(Value::as_str) {
        let gone = meta.value(keys::COMPANION_OF_GONE).is_some();
        let owner = Name(owner.to_owned());
        location.push(Row::new(
            words!(
                properties_beside = "Beside",
                note = "A row's label in Properties, for a file that keeps another's notes: the file it belongs to."
            )
            .say(),
            if gone {
                words!(
                    properties_companion_of_gone = "{ $name }, which is gone",
                    name = owner,
                    note = "In Properties, for a file that kept another's notes: the name of that file, which is no longer there."
                )
            } else {
                words!(
                    properties_companion_of = "{ $name }",
                    name = owner,
                    note = "In Properties, for a file that keeps another's notes: the name of that file, quoted."
                )
            }
            .say(),
        ));
    }
    // Folded, it says the first: where the file was, or where it is.
    if let Some(first) = location.first_mut() {
        first.gist = Some(Gist::Path(first.value.clone()));
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
            rows.push(
                Row::new(label::owner().say(), owner_named(uid))
                    .gist(owner_gist(uid))
                    .led(Lead::OwnedBy),
            );
        }
        if let Some(gid) = uint(keys::GID) {
            rows.push(Row::new(label::group().say(), group_named(gid)));
        }
        if let Some(mode) = uint(keys::MODE) {
            rows.push(Row::new(label::access().say(), skwir_platform::users::mode(mode)).in_gist());
        }
        rows
    };
    if let Some(links) = uint(keys::NLINK).filter(|links| *links > 1 && !entry.is_dir()) {
        permissions.push(Row::new(
            words!(
                properties_hard_links = "Hard links",
                note = "A row's label in Properties: how many names (hard links) the file has."
            )
            .say(),
            links.to_string(),
        ));
    }

    // Each says which time it is over its row: they are easily confused,
    // a folder's most of all.
    let (modified, accessed) = if entry.is_dir() {
        (
            words!(
                properties_modified_folder_tip = "When a name in it was last added, removed or renamed; not when a file in it changed.",
                note = "Pointing at a folder's Modified in Properties."
            ),
            words!(
                properties_accessed_folder_tip = "When it was last listed, as far as the system notes it: most note it seldom or never, to spare the drive.",
                note = "Pointing at a folder's Accessed in Properties."
            ),
        )
    } else {
        (
            words!(
                properties_modified_tip = "When its contents last changed. A copy, or a file unpacked, may keep the original's.",
                note = "Pointing at a file's Modified in Properties."
            ),
            words!(
                properties_accessed_tip = "When it was last read, as far as the system notes it: most note it seldom or never, to spare the drive.",
                note = "Pointing at a file's Accessed in Properties."
            ),
        )
    };
    let times: Vec<Row> = [
        (
            words!(
                properties_deleted = "Deleted",
                note = "A row's label in Properties: when the file was put in the trash."
            ),
            keys::TRASHED,
            words!(
                properties_deleted_tip = "When it was put in the trash, as the trash noted it.",
                note = "Pointing at Deleted in Properties."
            ),
        ),
        (label::modified(), keys::MODIFIED, modified),
        (
            words!(
                properties_changed = "Changed",
                note = "A row's label in Properties: when anything about the file last changed (its ctime)."
            ),
            keys::CHANGED,
            words!(
                properties_changed_tip = "When anything about it last changed: its contents, or what is said of it (its name, permissions, owner, tags).",
                note = "Pointing at Changed in Properties."
            ),
        ),
        (
            words!(
                properties_created = "Created",
                note = "A row's label in Properties: when the file was made on this drive."
            ),
            keys::CREATED,
            words!(
                properties_created_tip = "When it was made on this drive: a copy's is when it was copied, which may be later than Modified.",
                note = "Pointing at Created in Properties. Modified is the name of the row above it."
            ),
        ),
        (
            words!(
                properties_accessed = "Accessed",
                note = "A row's label in Properties: when the file was last read."
            ),
            keys::ACCESSED,
            accessed,
        ),
    ]
    .into_iter()
    .filter_map(|(label, key, tip)| {
        let at = time(key)?;
        let row = Row::new(label.say(), crate::format::exact_time(at)).tip(&tip);
        // Folded, it says when the contents last changed, to the minute,
        // and which time that is where there is room.
        Some(if key == keys::MODIFIED {
            row.gist(crate::format::time(at)).led(Lead::Modified)
        } else {
            row
        })
    })
    .collect();

    // Whatever else a plugin or the user attached: tags, ratings,
    // suggestions. Folded, it says what they are.
    let more = meta
        .iter()
        .filter(|(key, _)| !CORE.contains(key))
        .map(|(key, field)| Row {
            source: source_of(&field.provenance),
            gist: Some(Gist::Said(label_of(key).into())),
            ..Row::new(label_of(key), text_of(&field.value))
        })
        .filter(|row| !row.value.is_empty())
        .collect();

    let [notes, source] = match notes {
        Some(noting) => notes_groups(entry, path, noting),
        None => [None, None],
    };
    // Not read yet: what reading the entry always says keeps its place,
    // its group's heading there from the start, its first row labelled and
    // empty, so the rest fills in under it (both groups ship folded).
    let unread = !meta.is_hydrated();
    if unread && permissions.is_empty() {
        permissions.push(Row::reserved(label::owner().say()));
    }
    let times = if unread && times.is_empty() {
        vec![Row::reserved(label::modified().say())]
    } else {
        times
    };
    [
        group(group_keys::GENERAL, general),
        notes,
        source,
        group(group_keys::LOCATION, location),
        group(group_keys::PERMISSIONS, permissions),
        group(group_keys::TIMES, times),
        group(group_keys::MORE, more),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// The words Properties says in more than one place.
pub(crate) mod label {
    use skwir_core::words;
    use skwir_core::words::Words;

    pub fn size() -> Words {
        words!(
            properties_size = "Size",
            note = "A row's label in Properties: how much the file holds."
        )
    }
    pub fn owner() -> Words {
        words!(
            properties_owner = "Owner",
            note = "A row's label in Properties: the user who owns the file."
        )
    }
    pub fn group() -> Words {
        words!(
            properties_group = "Group",
            note = "A row's label in Properties: the group of users the file belongs to (Unix permissions)."
        )
    }
    pub fn access() -> Words {
        words!(
            properties_access = "Access",
            note = "A row's label in Properties: the file's mode, as ls writes it (rw-r--r--)."
        )
    }
    pub fn modified() -> Words {
        words!(
            properties_modified = "Modified",
            note = "A row's label in Properties: when the file's contents last changed."
        )
    }
    pub fn duration() -> Words {
        words!(
            properties_duration = "Duration",
            note = "A row's label in Properties: how long a sound or a film lasts."
        )
    }
    pub fn pages() -> Words {
        words!(
            properties_pages = "Pages",
            note = "A row's label in Properties: how many pages a document has."
        )
    }
    pub fn dimensions() -> Words {
        words!(
            properties_dimensions = "Dimensions",
            note = "A row's label in Properties: a picture's size in pixels."
        )
    }
    pub fn contains() -> Words {
        words!(
            properties_contains = "Contains",
            note = "A row's label in Properties: what a folder holds."
        )
    }
    pub fn changes() -> Words {
        words!(
            properties_changes = "Changes",
            note = "A row's label in Properties: who may change who may use the file, or why no one here can."
        )
    }
    pub fn change() -> Words {
        words!(
            properties_change = "Change…",
            note = "A link after a value in Properties: change it."
        )
    }
    pub fn add() -> Words {
        words!(
            properties_add = "Add…",
            note = "A link after the file's tags or comment in Properties, where it has none: give some."
        )
    }
    pub fn edit() -> Words {
        words!(
            properties_edit = "Edit…",
            note = "A link after the file's tags or comment in Properties: change them."
        )
    }
    pub fn count_again() -> Words {
        words!(
            properties_count_again = "Count again",
            note = "A link after what a folder holds in Properties: count it again, as it may have changed."
        )
    }
    pub fn mixed() -> Words {
        words!(
            properties_mixed = "Mixed",
            note = "A value in Properties, with several items selected, where they differ."
        )
    }
    pub fn none() -> Words {
        words!(
            properties_level_none = "None",
            note = "What a class of users (its owner, its group, others) can do with a file or a folder: nothing."
        )
    }
    pub fn read() -> Words {
        words!(
            properties_level_read = "Read",
            note = "What a class of users can do with a file: read it, not change it."
        )
    }
    pub fn read_write() -> Words {
        words!(
            properties_level_read_write = "Read and write",
            note = "What a class of users can do with a file: read it and change it."
        )
    }
    pub fn on_disk() -> Words {
        words!(
            properties_on_disk = "On disk",
            note = "A row's label in Properties: the room it takes on the drive."
        )
    }
    pub fn read_only() -> Words {
        words!(
            properties_read_only = "Read-only",
            note = "A row's label in Properties: the file, or the drive, cannot be changed."
        )
    }
}

/// `3 folders, 12 files`.
fn folders_and_files(folders: usize, files: usize) -> String {
    words!(
        properties_folders_and_files = "{ $folders ->
            [one] 1 folder
           *[other] { $folders } folders
        }, { $files ->
            [one] 1 file
           *[other] { $files } files
        }",
        folders = folders,
        files = files,
        note = "In Properties: how many folders, then how many files."
    )
    .say()
}

/// `1.2 MB (1,234,567 bytes)`: a size, and exactly.
fn exact_size(bytes: u64) -> String {
    words!(
        properties_exact_size = "{ $size } ({ $bytes ->
           *[other] { $bytes } bytes
        })",
        size = Size(bytes),
        bytes = bytes,
        note = "A size in Properties: as every size is said (1.2 MB), then exactly, in bytes."
    )
    .say()
}

/// An owner by name and number, or its number alone where no name is known.
fn owner_named(uid: u64) -> String {
    named(uid, skwir_platform::users::user(uid))
}

/// An owner on a folded heading: by name, or by number where no name is
/// known.
fn owner_gist(uid: u64) -> String {
    skwir_platform::users::user(uid).unwrap_or_else(|| uid.to_string())
}

/// Where a URL leads, said short: its host, or the URL when it names none.
fn host_of(url: &str) -> &str {
    url.split_once("://")
        .and_then(|(_, rest)| rest.split(['/', '?', '#']).next())
        .map(|host| host.rsplit('@').next().unwrap_or(host))
        .filter(|host| !host.is_empty())
        .unwrap_or(url)
}

/// A group by name and number, or its number alone.
fn group_named(gid: u64) -> String {
    named(gid, skwir_platform::users::group(gid))
}

fn named(id: u64, name: Option<String>) -> String {
    match name {
        Some(name) => words!(
            properties_named_id = "{ $name } ({ $id })",
            name = name,
            id = id.to_string(),
            note = "A user or a group in Properties: its name, then its number (alice (1000))."
        )
        .say(),
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
    let mut rows = vec![
        Row::new(
            words!(
                properties_selected = "Selected",
                note = "A row's label in Properties, with several items selected: how many."
            )
            .say(),
            format::items(selected),
        )
        .in_gist(),
    ];
    if folders > 0 && folders < entries.len() {
        rows.push(Row::new(
            words!(
                properties_of_which = "Of which",
                note = "A row's label in Properties, with several items selected: how many folders and files they are."
            )
            .say(),
            folders_and_files(folders, entries.len() - folders),
        ));
    }
    let total = || {
        words!(
            properties_total_size = "Total size",
            note = "A row's label in Properties, with several items selected: what they weigh together."
        )
        .say()
    };
    if folders > 0 {
        rows.push(Row {
            show: Show::Count { selection: true },
            ..Row::new(total(), "")
        });
    } else if let Some(bytes) = bytes {
        rows.push(Row::new(total(), format::size(bytes)).in_gist());
    }
    let mut groups = vec![Group::new(group_keys::SELECTION, rows)];
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
    mime: Option<&str>,
) -> Vec<SourceRow> {
    #[derive(serde::Deserialize)]
    struct Answer {
        rows: Vec<skwir_core::registry::Property>,
    }
    let mut rows = Vec::new();
    for source in sources {
        let mut command = source
            .command
            .clone()
            .with("entries", serde_json::json!([target]));
        if let (true, Some(mime)) = (source.typed, mime) {
            command = command.with("mime", serde_json::json!(mime));
        }
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
            // Said here, as every row Properties collects is.
            Ok(answer) => rows.extend(answer.rows.into_iter().map(|property| {
                SourceRow {
                    group: SharedString::from(property.group),
                    heading: property.heading,
                    folded: property.folded,
                    row: Row {
                        actions: property
                            .actions
                            .into_iter()
                            .map(|a| (a.words.say().into(), a.command))
                            .collect(),
                        actions_shown: property.actions_shown,
                        gist: property.gist.map(|gist| Gist::Said(gist.say().into())),
                        ..Row::new(property.label.say(), property.value.say())
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

    /// How a picker's title names them: the entry by its name, a
    /// selection by how many.
    fn named(&self, count: usize) -> Words {
        match self {
            Targets::Entry(entry) => words!(
                properties_target_named = "{ $name }",
                name = Name(
                    entry
                        .path
                        .file_name()
                        .unwrap_or(entry.path.as_os_str())
                        .to_string_lossy()
                        .into_owned()
                ),
                note = "Inside the title of what changes who may use a file: the file, by its name, quoted."
            ),
            Targets::Selection(_) => words!(
                properties_target_items = "{ $count ->
                    [one] 1 item
                   *[other] { $count } items
                }",
                count = count,
                note = "Inside the title of what changes who may use what is selected: how many items."
            ),
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

/// The three classes a mode speaks to: as `chmod` names each, and where
/// its bits are; `class_label` and `class_title` say each.
const CLASSES: [(char, u32); 3] = [('u', 6), ('g', 3), ('o', 0)];

/// A class's row's label.
fn class_label(c: char) -> Words {
    match c {
        'u' => words!(
            properties_owner_can = "Owner can",
            note = "A row's label in Properties: what the file's owner may do with it (Read and write)."
        ),
        'g' => words!(
            properties_group_can = "Group can",
            note =
                "A row's label in Properties: what the users of the file's group may do with it."
        ),
        _ => words!(
            properties_others_can = "Others can",
            note = "A row's label in Properties: what every other user may do with the file."
        ),
    }
}

/// The title of what picks a class's level for `what` (`Targets::named`).
fn class_title(c: char, what: Words) -> Words {
    match c {
        'u' => words!(
            properties_owner_may = "What its owner may do with { $what }",
            what = what,
            note = "The title of the choices of what a file's owner may do with it. what: the file, by its name, or how many items."
        ),
        'g' => words!(
            properties_group_may = "What its group may do with { $what }",
            what = what,
            note = "The title of the choices of what the users of a file's group may do with it. what: the file, by its name, or how many items."
        ),
        _ => words!(
            properties_others_may = "What others may do with { $what }",
            what = what,
            note = "The title of the choices of what every other user may do with a file. what: the file, by its name, or how many items."
        ),
    }
}

/// A file's levels (its run bit is "Can run"'s), in the order offered:
/// each in words, its read and write bits, and the clause that sets them
/// and keeps the run bit.
fn file_levels(c: char) -> Vec<(Words, u64, String)> {
    vec![
        (label::none(), 0, format!("{c}-rw")),
        (label::read(), 4, format!("{c}+r,{c}-w")),
        (label::read_write(), 6, format!("{c}+rw")),
    ]
}

/// A folder's levels, with the clause that sets each: `X` gives search to
/// the folders inside and leaves files that no one runs as they were.
fn folder_levels(c: char) -> Vec<(Words, u64, String)> {
    vec![
        (label::none(), 0, format!("{c}=")),
        (
            words!(
                properties_level_list = "List items only",
                note = "What a class of users can do with a folder: see the names in it, nothing more."
            ),
            4,
            format!("{c}=r"),
        ),
        (
            words!(
                properties_level_open = "Open items",
                note = "What a class of users can do with a folder: see the names in it and open them."
            ),
            5,
            format!("{c}=rX"),
        ),
        (
            words!(
                properties_level_create = "Create and delete items",
                note = "What a class of users can do with a folder: also make, rename and delete what is in it."
            ),
            7,
            format!("{c}=rwX"),
        ),
    ]
}

/// A class's level in words: of a file its read and write bits, of a
/// folder all three.
fn level_said(bits: u64, folder: bool) -> String {
    if !folder {
        return match bits & 6 {
            0 => label::none(),
            4 => label::read(),
            6 => label::read_write(),
            _ => words!(
                properties_level_write_only = "Write only",
                note = "What a class of users can do with a file: change it, not read it."
            ),
        }
        .say();
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
            |(words, ..)| words.say(),
        )
}

/// Who may run a file, from its run bits.
fn runners(mode: u64) -> Words {
    match (mode & 0o100 != 0, mode & 0o010 != 0, mode & 0o001 != 0) {
        (false, false, false) => words!(
            properties_runs_no_one = "No one",
            note = "Who may run a file as a program, in Properties' Can run."
        ),
        (true, false, false) => words!(
            properties_runs_owner = "Its owner",
            note = "Who may run a file as a program, in Properties' Can run."
        ),
        (true, true, false) => words!(
            properties_runs_owner_group = "Its owner and group",
            note = "Who may run a file as a program, in Properties' Can run: its owner and the users of its group."
        ),
        (true, true, true) => words!(
            properties_runs_everyone = "Everyone",
            note = "Who may run a file as a program, in Properties' Can run."
        ),
        (true, false, true) => words!(
            properties_runs_owner_others = "Its owner and others",
            note = "Who may run a file as a program, in Properties' Can run: its owner and the users not of its group."
        ),
        (false, true, false) => words!(
            properties_runs_group = "Its group",
            note =
                "Who may run a file as a program, in Properties' Can run: the users of its group."
        ),
        (false, true, true) => words!(
            properties_runs_group_others = "Its group and others",
            note = "Who may run a file as a program, in Properties' Can run: every user but its owner."
        ),
        (false, false, true) => words!(
            properties_runs_others = "Others",
            note = "Who may run a file as a program, in Properties' Can run: the users neither its owner nor of its group."
        ),
    }
}

/// What its set-ID and sticky bits do, in words.
fn specials(mode: u64, folder: bool) -> Vec<Words> {
    let mut said = Vec::new();
    if mode & 0o4000 != 0 {
        said.push(if folder {
            words!(
                properties_setuid_folder = "Set-user-ID",
                note = "What a folder's set-user-ID bit is called, in Properties' Special: it does nothing on most systems."
            )
        } else {
            words!(
                properties_setuid = "Runs as its owner, whoever starts it",
                note = "What a program's set-user-ID bit does, in Properties' Special."
            )
        });
    }
    if mode & 0o2000 != 0 {
        said.push(if folder {
            words!(
                properties_setgid_folder = "New items take the folder's group",
                note = "What a folder's set-group-ID bit does, in Properties' Special."
            )
        } else {
            words!(
                properties_setgid = "Runs as its group",
                note = "What a program's set-group-ID bit does, in Properties' Special."
            )
        });
    }
    if mode & 0o1000 != 0 {
        said.push(words!(
            properties_sticky = "Only an item's owner may rename or delete it",
            note = "What a folder's sticky bit does, in Properties' Special."
        ));
    }
    said
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
    let mixed = || label::mixed().say();
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
    // What picks takes its words as their data (`ui.choose`'s arguments,
    // said in the window's language).
    let choose = |title: Words, choices: Vec<serde_json::Value>| {
        let mut args = serde_json::json!({ "title": title.to_data(), "choices": choices });
        if subjects.iter().any(|s| s.folder) {
            args["option"] = serde_json::json!({
                "title": words!(
                    properties_also_inside = "Also everything inside",
                    note = "A switch under the choices of who may use folders: change what they hold too."
                )
                .to_data(),
                "field": "inside",
            });
        }
        Invocation::with_args("ui.choose", &args)
    };
    let choice = |title: Words, now: bool, command: serde_json::Value| {
        let mut choice = serde_json::json!({ "title": title.to_data(), "command": command });
        if now {
            choice["detail"] = words!(
                properties_now = "Now",
                note = "Beside the choice that is the file's now, among the choices of who may use it."
            )
            .to_data();
        }
        choice
    };

    // Folded, the group says the owner and the mode they share.
    let owner = shared(&|s| s.uid);
    let mut rows = vec![Row {
        gist: owner.map(|uid| Gist::Said(owner_gist(uid).into())),
        lead: Some(Lead::OwnedBy),
        ..Row::new(label::owner().say(), owner.map_or_else(mixed, owner_named))
    }];
    let mut group = Row::new(
        label::group().say(),
        shared(&|s| s.gid).map_or_else(mixed, group_named),
    );
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
                    Words::as_written(name),
                    now == Some(gid),
                    serde_json::json!({
                        "name": "set_access",
                        "args": targets.args(&serde_json::json!({ "group": gid })),
                    }),
                )
            })
            .collect();
        group = group.edit(
            &label::change(),
            choose(
                words!(
                    properties_give_to_group = "Give { $what } to the group",
                    what = targets.named(count),
                    note = "The title of the choices of the group a file belongs to. what: the file, by its name, or how many items."
                ),
                choices,
            ),
        );
    }
    rows.push(group);

    // A file's level leaves its run bit to "Can run".
    let level_bits = if folders { 0o7 } else { 0o6 };
    for (c, shift) in CLASSES {
        let bits = shared(&|s| (s.mode >> shift) & level_bits);
        let mut row = Row::new(
            class_label(c).say(),
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
                &label::change(),
                choose(class_title(c, targets.named(count)), choices),
            );
        }
        rows.push(row);
    }

    if subjects.iter().all(|s| s.regular) {
        let runs = shared(&|s| s.mode & 0o111);
        let mut row = Row::new(
            words!(
                properties_can_run = "Can run",
                note = "A row's label in Properties: who may run the file as a program."
            )
            .say(),
            runs.map_or_else(mixed, |m| runners(m).say()),
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
                    .filter(|(c, shift)| {
                        *c == 'u' || subjects.iter().all(|s| (s.mode >> shift) & 4 != 0)
                    })
                    .map(|(c, _)| c)
                    .collect();
                row.edit(
                    &words!(
                        properties_allow_running = "Allow running",
                        note =
                            "A link after who may run a file: let everyone who can read it run it."
                    ),
                    to(format!("{readers}+x")),
                )
            } else {
                row.edit(
                    &words!(
                        properties_stop_running = "Stop running",
                        note = "A link after who may run a file: let no one run it."
                    ),
                    to("a-x".into()),
                )
            };
        }
        rows.push(row);
    }

    let mode = shared(&|s| s.mode);
    let mut access = Row::new(
        label::access().say(),
        mode.map_or_else(mixed, skwir_platform::users::mode),
    );
    if mode.is_some() {
        access = access.in_gist();
    }
    if editable {
        // What a mode is written as, and what the one typed comes to for
        // them, said as it is typed (ADR-137).
        let mut describe = serde_json::json!({ "folders": folders, "files": files });
        if let Some(mode) = mode {
            describe["current"] = (mode & 0o7777).into();
        }
        // What asks takes its words as their data (`ui.ask`'s arguments,
        // said in the window's language).
        let mut ask = serde_json::json!({
            "title": words!(
                properties_access_of = "Access of { $what }",
                what = targets.named(count),
                note = "The title of where a file's mode is typed. what: the file, by its name, or how many items."
            )
            .to_data(),
            "accept": words!(
                properties_access_change = "Change",
                note = "The button that changes a file's mode to the one typed."
            )
            .to_data(),
            "placeholder": words!(
                properties_access_placeholder = "As chmod takes it: 644, u+x, go-w, a=rX",
                note = "The placeholder of the field a file's mode is typed in. chmod is a program's name; 644, u+x, go-w and a=rX are modes as it writes them."
            )
            .to_data(),
            "command": "set_access",
            "field": "mode",
            "args": targets.args(&serde_json::json!({})),
            "reference": skwir_core::op::access::reference(),
            "describe": { "command": "set_access.describe", "args": describe },
        });
        if let Some(mode) = mode {
            ask["text"] = format!("{mode:o}").into();
        }
        access = access.edit(&label::change(), Invocation::with_args("ui.ask", &ask));
    }
    rows.push(access);
    if let Some(mode) = mode {
        rows.extend(specials(mode, first.folder).into_iter().map(|said| {
            Row::new(
                words!(
                    properties_special = "Special",
                    note = "A row's label in Properties: what a special bit of the file's mode does (set-user-ID, sticky)."
                )
                .say(),
                said.say(),
            )
        }));
    }
    if !editable && user.id.is_some() {
        let why = if trashed {
            words!(
                properties_restore_to_change = "Restore it to change who may use it",
                note =
                    "In Properties, for a file of the trash: why who may use it cannot be changed."
            )
        } else if count > 1 {
            words!(
                properties_owner_changes_them = "Only their owner can change who may use them",
                note = "In Properties, with several items selected someone else owns: why who may use them cannot be changed."
            )
        } else {
            words!(
                properties_owner_changes = "Only its owner can change who may use it",
                note = "In Properties, for a file someone else owns: why who may use it cannot be changed."
            )
        };
        rows.push(Row::new(label::changes().say(), why.say()));
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
        return vec![Row::new(label.to_owned(), "").action(
            &words!(
                properties_count = "Count",
                note = "A link in Properties: count what the folder holds, inside its folders too."
            ),
            again(),
        )];
    };
    let at_least = || {
        words!(
            properties_at_least = "At least { $size }",
            size = Size(now.size),
            note = "What a folder holds in Properties, where some of it could not be counted."
        )
        .say()
    };
    // Folded, its group says the size as it stands, without the bytes.
    let first = if running {
        Row::new(
            label.to_owned(),
            words!(
                properties_so_far = "{ $size } so far…",
                size = Size(now.size),
                note = "What a folder holds in Properties, while it is counted."
            )
            .say(),
        )
        .in_gist()
        .action(
            &words!(
                properties_count_stop = "Stop",
                note = "A link in Properties, while what a folder holds is counted: stop counting."
            ),
            Invocation::with_args("ui.properties_count", &serde_json::json!({ "stop": true })),
        )
    } else if !now.done {
        Row::new(
            label.to_owned(),
            words!(
                properties_at_least_stopped = "At least { $size }, stopped",
                size = Size(now.size),
                note = "What a folder holds in Properties, its count stopped before its end."
            )
            .say(),
        )
        .gist(at_least())
        .action(&label::count_again(), again())
    } else if now.lower_bound() {
        Row::new(label.to_owned(), at_least())
            .in_gist()
            .action(&label::count_again(), again())
    } else {
        Row::new(label.to_owned(), exact_size(now.size))
            .gist(format::size(now.size))
            .action(&label::count_again(), again())
    }
    .shown();
    let mut rows = vec![
        first,
        Row::new(
            label::on_disk().say(),
            format::size(now.allocated),
        ),
        Row::new(
            words!(
                properties_in_all = "In all",
                note = "A row's label in Properties: how many files and folders a folder holds, inside its folders too."
            )
            .say(),
            words!(
                properties_files_and_folders = "{ $files ->
                    [one] 1 file
                   *[other] { $files } files
                }, { $folders ->
                    [one] 1 folder
                   *[other] { $folders } folders
                }",
                files = now.files,
                folders = now.folders,
                note = "In Properties: how many files, then how many folders."
            )
            .say(),
        ),
    ];
    if now.unreadable > 0 {
        rows.push(Row::new(
            words!(
                properties_unreadable = "Unreadable",
                note = "A row's label in Properties: what of a folder could not be counted."
            )
            .say(),
            words!(
                properties_unlisted = "{ $count ->
                    [one] 1 folder
                   *[other] { $count } folders
                } could not be listed",
                count = now.unreadable,
                note = "In Properties: how many folders inside could not be read, while what a folder holds was counted."
            )
            .say(),
        ));
    }
    // Neither files nor folders: what they are could not be read.
    if now.unknown > 0 {
        rows.push(Row::new(
            words!(
                properties_unknown = "Unknown",
                note = "A row's label in Properties: what of a folder could not be told a file or a folder."
            )
            .say(),
            words!(
                properties_unread = "{ $count ->
                    [one] 1 item
                   *[other] { $count } items
                } could not be read",
                count = now.unknown,
                note = "In Properties: how many items inside could not be read, while what a folder holds was counted."
            )
            .say(),
        ));
    }
    if now.other_drives > 0 {
        rows.push(Row::new(
            words!(
                properties_not_counted = "Not counted",
                note = "A row's label in Properties: what was left out of what a folder holds."
            )
            .say(),
            words!(
                properties_drives_inside = "{ $count ->
                    [one] 1 drive
                   *[other] { $count } drives
                } mounted inside",
                count = now.other_drives,
                note = "In Properties: how many drives are mounted inside the folder, left out of what it holds."
            )
            .say(),
        ));
    }
    rows
}

/// What the system keeps of a file beside its mode, in words, for the
/// Permissions group: why it cannot be deleted, why it takes less room.
pub fn flag_rows(flags: skwir_core::fs::stat::Flags) -> Vec<Row> {
    let by_the_drive = || {
        words!(
            properties_by_the_drive = "By the drive",
            note = "In Properties: the file is compressed, or encrypted, by the drive it is on."
        )
    };
    [
        (
            flags.immutable,
            words!(
                properties_immutable = "Immutable",
                note = "A row's label in Properties: the system keeps the file as it is."
            ),
            words!(
                properties_immutable_value = "No one can change, rename or delete it",
                note = "In Properties, after Immutable."
            ),
        ),
        (
            flags.append_only,
            words!(
                properties_append_only = "Append only",
                note = "A row's label in Properties: the system lets the file only grow."
            ),
            words!(
                properties_append_only_value = "It can only grow",
                note = "In Properties, after Append only."
            ),
        ),
        (
            flags.compressed,
            words!(
                properties_compressed = "Compressed",
                note = "A row's label in Properties: the drive keeps the file compressed."
            ),
            by_the_drive(),
        ),
        (
            flags.encrypted,
            words!(
                properties_encrypted = "Encrypted",
                note = "A row's label in Properties: the drive keeps the file encrypted."
            ),
            by_the_drive(),
        ),
        (
            flags.read_only,
            label::read_only(),
            words!(
                properties_read_only_value = "It cannot be changed",
                note = "In Properties, after Read-only: the file's own read-only mark."
            ),
        ),
    ]
    .into_iter()
    .filter(|(set, ..)| *set)
    .map(|(_, label, value)| Row::new(label.say(), value.say()))
    .collect()
}

/// What is shown measured in its own unit, for General under its sizes:
/// a folder's items, a picture's pixels, a document's pages, a track's or
/// a film's length. Which one comes is known by its kind before the
/// preview says it, so while that preview is on its way the row is there,
/// labelled and empty (`Row::reserved`), and its value moves nothing.
fn measure(shell: &Shell) -> Option<Row> {
    let preview = &shell.preview;
    if let Some(PreviewContent::Dir { items, folders }) = &preview.content {
        return Some(contains(*items, *folders));
    }
    // A folder shown by the file standing for it is measured by what it
    // holds, never by that file's pages, length or pixels.
    if preview.info.is_dir
        && let Some(holds) = preview.stands_for()
    {
        return Some(contains(holds.items, holds.folders));
    }
    if shell.shows_player() {
        return Some(match preview.media.state.duration {
            Some(duration) => Row::new(label::duration().say(), crate::media::time(duration))
                .in_gist()
                .led(Lead::Lasts),
            None => Row::reserved(label::duration().say()),
        });
    }
    if let Some(pages) = crate::reader::pages(shell) {
        return Some(
            Row::new(label::pages().say(), pages.to_string()).gist(
                words!(
                    properties_pages_gist = "{ $count ->
                        [one] 1 page
                       *[other] { $count } pages
                    }",
                    count = pages,
                    note = "A folded heading of Properties: how many pages a document has."
                )
                .say(),
            ),
        );
    }
    if let Some(PreviewContent::Image {
        width,
        height,
        document: None,
        ..
    }) = &preview.content
    {
        return Some(
            Row::new(
                label::dimensions().say(),
                words!(
                    properties_dimensions_value = "{ $size } px",
                    size = format!("{width} × {height}"),
                    note = "A picture's size in Properties, in pixels (4032 × 3024 px)."
                )
                .say(),
            )
            .in_gist(),
        );
    }
    // On its way: asked of the worker, not answered yet. A folder is read
    // whenever it is shown, the Preview off too; a file only while a
    // preview is (`PreviewState::previews`).
    let coming = preview.content.is_none() && preview.phase != crate::preview::PreviewPhase::Idle;
    if !coming {
        return None;
    }
    if preview.info.is_dir {
        Some(Row::reserved(label::contains().say()))
    } else if !preview.previews() {
        None
    } else if shell.media_kind().is_some() {
        Some(Row::reserved(label::duration().say()))
    } else if crate::preview::opens_as_pages(shell) {
        Some(Row::reserved(label::pages().say()))
    } else if preview.info.picture {
        Some(Row::reserved(label::dimensions().say()))
    } else {
        None
    }
}

/// What a folder holds, one row whatever it holds: its folders and its
/// files (anything else it holds counted with them).
fn contains(items: usize, folders: usize) -> Row {
    let files = items.saturating_sub(folders);
    let value = match (folders, files) {
        (0, 0) => words!(
            properties_contains_nothing = "Nothing",
            note = "What a folder holds, in Properties' Contains: it is empty."
        )
        .say(),
        (0, files) => format::files(files),
        (folders, 0) => format::folders(folders),
        (folders, files) => folders_and_files(folders, files),
    };
    Row::new(label::contains().say(), value).gist(format::items(items))
}

/// What the file says of itself, beyond its measure (`measure`): a
/// picture's facts, a format's details, a track's title and tracks, and
/// what plugins read of it on demand (a picture's place). Last of the
/// groups: how many rows it has is known only once read.
fn content_group(shell: &Shell) -> Option<Group> {
    let preview = &shell.preview;
    let mut rows = Vec::new();
    // A folder's are its own: what the file standing for it says of itself
    // is that file's.
    let content = preview.content.as_ref().filter(|_| !preview.info.is_dir);
    match content {
        Some(PreviewContent::Image {
            document: None,
            facts,
            ..
        }) if !shell.shows_player() => {
            rows.extend(facts.iter().map(Row::of));
        }
        Some(PreviewContent::Inspection(display)) => {
            // A fact of the format's is said with its label: its value
            // alone may be a bare number.
            rows.extend(display.data.details.iter().enumerate().map(|(d, detail)| {
                let row = Row::of(detail);
                if d == 0 {
                    row.gist(
                        words!(
                            properties_detail_gist = "{ $label }: { $value }",
                            label = detail.label.clone(),
                            value = detail.value.as_str(),
                            note = "A folded heading of Properties: the first fact the file's format says of it, its label then its value, as the preview said them (Author: Ann)."
                        )
                        .say(),
                    )
                } else {
                    row
                }
            }));
        }
        _ => {}
    }
    if shell.shows_player() && !preview.info.is_dir {
        let state = &preview.media.state;
        for (label, value) in [
            (
                words!(
                    properties_track_title = "Title",
                    note = "A row's label in Properties: a track's title, as its tags say it."
                ),
                &state.title,
            ),
            (
                words!(
                    properties_artist = "Artist",
                    note = "A row's label in Properties: who plays a track, as its tags say it."
                ),
                &state.artist,
            ),
            (
                words!(
                    properties_album = "Album",
                    note =
                        "A row's label in Properties: the album a track is of, as its tags say it."
                ),
                &state.album,
            ),
        ] {
            if !value.is_empty() {
                rows.push(Row::new(label.say(), value.clone()));
            }
        }
        rows.extend(
            state
                .details
                .iter()
                .map(|(label, value)| Row::new(label.clone(), value.clone())),
        );
        // Transitional: a list argument (ADR-139).
        if state.audio_tracks.len() > 1 {
            rows.push(Row::new(
                words!(
                    properties_audio_tracks = "Audio tracks",
                    note = "A row's label in Properties: a film's sound tracks (their languages)."
                )
                .say(),
                state.audio_tracks.join(", "),
            ));
        }
        // What the file says of itself, read with its poster: where a
        // video was recorded.
        if let Some(PreviewContent::Image { facts, .. }) = &preview.content {
            rows.extend(facts.iter().map(Row::of));
        }
        if !state.subtitle_tracks.is_empty() {
            rows.push(Row::new(
                words!(
                    properties_subtitles = "Subtitles",
                    note = "A row's label in Properties: a film's subtitles (their languages)."
                )
                .say(),
                state.subtitle_tracks.join(", "),
            ));
        }
    }
    // What plugins read of the file on demand (a picture's place).
    rows.extend(preview.fields.iter().cloned());
    group(group_keys::CONTENT, rows)
}

/// The rows the sources said they answer for `entry` (`PropertyPlace`),
/// labelled and empty: what Properties shows of them while they are asked.
pub fn places(
    sources: &[skwir_core::registry::PropertySource],
    entry: &Entry,
    path: &std::path::Path,
) -> Vec<SourceRow> {
    use skwir_core::registry::PlaceOf;
    let of = if entry.is_dir() {
        PlaceOf::Folders
    } else if entry.content(path).is_some() {
        PlaceOf::Files
    } else {
        return Vec::new();
    };
    sources
        .iter()
        .flat_map(|source| &source.places)
        .filter(|place| place.of == of)
        .map(|place| SourceRow {
            group: SharedString::from(place.group.clone()),
            heading: place.heading.clone(),
            folded: place.folded,
            row: Row::reserved(place.label.say()),
        })
        .collect()
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
                            this.open_tag_menu(&tag, true, event.position, window, cx);
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
        .gap(sized(2.))
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

/// Whether there is nothing to preview, so Properties, while on, takes the
/// room as tall as it says (`Room::Whole`), and while off the type's icon
/// stands for the preview. A preview still on its way is not nothing:
/// Properties waits for the answer rather than filling the body for the
/// instant before a picture replaces it. Nor is a member with no preview
/// of its own while its container's view is kept above it
/// (`PreviewState::parked`): that view is content.
pub fn alone(shell: &Shell) -> bool {
    !shell.shows_player()
        && shell.preview.parked().is_none()
        && match &shell.preview.content {
            None => shell.preview.phase == crate::preview::PreviewPhase::Idle,
            Some(
                PreviewContent::Generic
                | PreviewContent::Dir { .. }
                | PreviewContent::Error(_)
                | PreviewContent::TooLarge(_),
            ) => true,
            Some(_) => false,
        }
}

/// Where the section is in the room under the header's card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Room {
    /// The panel's Preview switch off: Properties is all the panel shows,
    /// and fills it.
    Panel,
    /// Nothing to preview (`alone`): as tall as what it says, up to the
    /// whole panel, the toolbar and why there is no preview under it.
    Whole,
    /// Over a preview, above it: at most its share of the room, less when
    /// it says less.
    Above,
    /// Over a preview in a room wider than tall (the panel at the bottom,
    /// peek): a column of its share of the room at the preview's left.
    Beside,
}

/// Where the section goes: `previews` is whether the preview is drawn
/// beside it (the panel's Preview switch, or peek), `wide` whether the
/// room is wider than tall.
pub fn room(shell: &Shell, previews: bool, wide: bool) -> Room {
    if !previews {
        Room::Panel
    } else if alone(shell) {
        Room::Whole
    } else if wide {
        Room::Beside
    } else {
        Room::Above
    }
}

/// What a row's links are a group of: the pointer on the row shows them.
const ROW: &str = "properties-row";

/// The line height of Properties' text, a paragraph's rather than gpui's
/// default 1.618: a section's rows read as the lines of one paragraph, a
/// value that wraps at the same pitch as the rows, while the sections stand
/// apart under their headings.
const LINE: f32 = 1.4;

/// What a row adds above and below its line, at 100 %: enough for its
/// hover to frame it.
const ROW_PAD: f32 = 1.;

/// The section: right under the header's card (`preview::under_card` puts
/// it there, and nothing else draws it), in the `room` it is given, while
/// its switch is on (`PreviewState::properties_on`), whatever is shown. It
/// scrolls past its room. The card holds the name, type, size and date, so
/// the groups here repeat none of those.
///
/// Each group's heading folds it (`ui.properties_fold`), as the user left
/// it or as it ships (`Group::folded`), and while folded says at its right
/// what its rows say in short (`Group::gist`: the size, the path, the
/// drive and its free room). A row's links show while the pointer is on
/// it, unless they are all it says or stop or start what runs
/// (`Row::links_shown`); where they go is `row_element`'s.
pub fn render(
    shell: &Shell,
    theme: &Theme,
    room: Room,
    grounds: crate::theme::Grounds,
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
    // Its switch alone says whether it shows, whatever is shown.
    if !preview.properties_on {
        return None;
    }
    let share = preview.properties_share.get(room == Room::Beside);
    let section = div()
        .flex()
        .flex_col()
        .min_h_0()
        .bg(grounds.read)
        .border_color(theme.border);
    // As cards, the card around it takes the room's size
    // (`preview::cards_under`), and a gutter sets it apart.
    let lines = theme.lines();
    let section = match room {
        Room::Panel => section.flex_1(),
        Room::Whole => section.flex_initial().when(lines, gpui::Styled::border_b_1),
        Room::Above => section
            .flex_initial()
            .max_h(gpui::relative(share))
            .when(lines, gpui::Styled::border_b_1),
        Room::Beside => section
            .flex_none()
            .h_full()
            .w(gpui::relative(share))
            .when(lines, gpui::Styled::border_r_1),
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
            preview
                .properties
                .iter()
                .chain(preview.volume.iter())
                .map(|group| (false, group)),
        )
        .map(|(of_selection, group)| {
            let mut group = group.clone();
            // What the system adds to the mode, read after the rest.
            if group.key.as_ref() == group_keys::PERMISSIONS {
                group.rows.extend(preview.access.iter().cloned());
            }
            // What the file takes on its drive, after its size, then its
            // own measure (`measure`).
            if group.key.as_ref() == group_keys::GENERAL {
                group.rows.extend(preview.general.iter().cloned());
                if !of_selection {
                    group.rows.extend(measure(shell));
                }
            }
            // Nothing on a read-only drive changes who may use it: the
            // entry's drive, or for the selection's, any of theirs.
            let (read_only, why) = if of_selection {
                (
                    preview.selection_read_only,
                    words!(
                        properties_drives_read_only = "A drive they are on is read-only",
                        note = "In Properties, with several items selected: why who may use them cannot be changed."
                    ),
                )
            } else {
                (
                    preview.read_only,
                    words!(
                        properties_drive_read_only = "The drive is read-only",
                        note = "In Properties: why who may use the file cannot be changed."
                    ),
                )
            };
            if read_only && group.rows.iter().any(|row| row.edits_access) {
                for row in group.rows.iter_mut().filter(|row| row.edits_access) {
                    row.actions.clear();
                }
                group.rows.push(Row::new(label::changes().say(), why.say()));
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
    // What plugins say: in the group each names (by its key, `Group::key`),
    // or one of its own before the drive's, folded as the source ships it.
    let mut groups = groups;
    for source in &preview.plugin_rows {
        if let Some(group) = groups.iter_mut().find(|g| g.key == source.group) {
            group.rows.push(source.row.clone());
        } else {
            let at = groups
                .iter()
                .position(|g| g.key.as_ref() == group_keys::VOLUME)
                .unwrap_or(groups.len());
            groups.insert(
                at,
                Group {
                    folded: source.folded,
                    ..Group::headed(
                        source.group.clone(),
                        source.heading.as_ref(),
                        vec![source.row.clone()],
                    )
                },
            );
        }
    }
    // What else the file says of itself, last: how many rows it has is
    // known only once read, and below it nothing moves.
    groups.extend(content_group(shell));
    let mut list = div()
        .id("details-list")
        .track_scroll(&preview.properties_scroll)
        .overflow_y_scroll()
        .flex_1()
        .min_h_0()
        .px_3()
        .pb_3()
        // What is read here is about the file: its values are the body
        // text, their labels muted (ADR-117), in the lines of a paragraph.
        .text_body()
        .line_height(gpui::relative(LINE));
    for (g, group) in groups.into_iter().enumerate() {
        let folded = preview.properties_folds.folded(&group.key, group.folded);
        let chips = folded.then(|| group.tags()).flatten().map(|(tags, given)| {
            let tags =
                crate::tags::ordered(tags.to_vec(), given, &shell.tag_colors, &shell.tag_pins);
            crate::tags::strip(&tags, crate::tags::StripOn::Row { selected: false }, theme)
                .natural()
        });
        let gists = if folded {
            [group.gist_led(), group.gist()]
                .into_iter()
                .flatten()
                .collect()
        } else {
            Vec::new()
        };
        list = list.child(heading(
            g,
            (&group.key, &group.title),
            folded,
            chips,
            gists,
            theme,
            cx,
        ));
        if folded {
            continue;
        }
        for (r, row) in group.rows.into_iter().enumerate() {
            list = list.child(row_element(shell, row, (g, r), theme, cx));
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
/// heading shows it). While folded, its gist at its right: the first of
/// `gists` that fits (`Group::gist_led`, then `Group::gist`), measured
/// as it is laid out (`adaptive::Fit`); where none does, the last, cut at
/// its end, or at its start when it holds a path. Its tags come first, as
/// `chips` (`Group::tags`), named as room allows: the words give way
/// before them.
fn heading(
    g: usize,
    (key, title): (&SharedString, &SharedString),
    folded: bool,
    chips: Option<crate::elements::chips::ChipStrip>,
    gists: Vec<Gist>,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> gpui::Div {
    let gist = (!gists.is_empty()).then(|| crate::elements::adaptive::Fit {
        variants: gists
            .into_iter()
            .map(|gist| {
                // As wide as it is given, so the last is cut there.
                let cut = div()
                    .w_full()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_color(theme.text_muted);
                match gist {
                    Gist::Said(words) => cut.text_ellipsis().child(words),
                    Gist::Path(words) => cut.text_ellipsis_start().child(words),
                }
                .into_any_element()
            })
            .collect(),
        cuts: true,
        start: false,
    });
    let fold = key.clone();
    div()
        .when(g == 0, gpui::Styled::pt_2)
        // Sections stand apart, their rows the lines of a paragraph
        // (`LINE`).
        .when(g > 0, |d| {
            d.mt_2().pt_1().border_t_1().border_color(theme.border)
        })
        .child(
            div()
                .id(crate::elements::known_by("properties-heading", key))
                .flex()
                .items_center()
                .justify_between()
                .gap_1()
                .py(sized(3.))
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
                .map(|d| match chips {
                    // The words are cut to nothing before a chip steps
                    // down: they give way a hundred times as readily.
                    Some(chips) => {
                        d.child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .items_center()
                                .justify_end()
                                .gap_2()
                                .child(chips)
                                .children(gist.map(|gist| {
                                    div().flex().min_w_0().flex_shrink(100.).child(gist)
                                })),
                        )
                    }
                    None => d.children(gist),
                }),
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
                    .text_color(theme.accent_text)
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
    (g, r): (usize, usize),
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> gpui::Stateful<gpui::Div> {
    // What is inside the row is known within it.
    let id = g * 1000 + r;
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
    // Known by what it says: another file's row where it stands is not
    // given its tooltip, nor its links' (inside it).
    div()
        .id(crate::elements::known_by("detail", (g, r, &label, &value)))
        .group(ROW)
        .relative()
        .flex()
        .items_start()
        .gap_2()
        .py(sized(ROW_PAD))
        .rounded_sm()
        .cursor_pointer()
        .hover(|d| d.bg(theme.hover))
        .on_click(cx.listener(move |this, _, _, cx| {
            if !copies {
                this.set_status(not_text(), true, cx);
                return;
            }
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(value.to_string()));
            // Transitional: the row's label lowercased in code (ADR-139):
            // a row's words for what it copies go with its label.
            this.set_status(
                words!(
                    properties_copied = "Copied { $what }",
                    what = label.to_lowercase(),
                    note = "Said as a row of Properties is clicked: what it said was copied. what: the row's label (path, size)."
                ),
                false,
                cx,
            );
        }))
        .when_some(row.tip, |d, tip| {
            d.tooltip(move |_, cx| crate::toolbar::tooltip(tip.clone(), cx))
        })
        .child(
            div()
                .w(sized(84.))
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
                .py(sized(ROW_PAD))
                .pl_3()
                .rounded_sm()
                // Over the row's words: opaque, as the row it covers is.
                .bg(theme.bg.blend(theme.hover))
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

/// The same edge as cards (`Theme::cards`): the gutter between Properties'
/// card and the preview's.
pub fn gutter(room: Room, cx: &mut Context<Shell>) -> AnyElement {
    use crate::elements::edge::{Side, gutter};
    let beside = room == Room::Beside;
    gutter(
        "properties-resize",
        if beside { Side::Left } else { Side::Top },
        PropertiesResize { beside },
        move |this| this.preview.properties_share.set(beside, SHARE),
        cx,
    )
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
        let keep = keep(cx.global::<Theme>(), beside);
        let (along, size) = if beside {
            (ev.event.position.x - ev.bounds.left(), ev.bounds.size.width)
        } else {
            (ev.event.position.y - ev.bounds.top(), ev.bounds.size.height)
        };
        // As cards, the pointer at the middle of the gutter between them.
        let half = gpui::px(crate::terminal::half_gutter(cx));
        let (along, size) = (along - half, size - half * 2.);
        if let Some(share) =
            crate::elements::edge::share(f32::from(along), f32::from(size), keep, SHARE)
        {
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

/// What a path no text can hold says as it is clicked to be copied.
pub(crate) fn not_text() -> Words {
    words!(
        properties_path_not_text = "The path is not text the clipboard can hold",
        note = "Said where a path to copy is not text (a name in no encoding), which the clipboard cannot hold."
    )
}

/// Whether the group known by `key` ships folded: as the shell's own group
/// of that key says (`Group::folded`), else as the sources whose rows make
/// it say (`Property::folded`). A group not shown ships unfolded.
pub fn ships_folded(preview: &crate::preview::PreviewState, title: &str) -> bool {
    let own = preview
        .selection_summary
        .iter()
        .chain(preview.properties.iter())
        .chain(preview.volume.iter())
        .find(|group| group.key.as_ref() == title);
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

    /// Notes said with the categories `fields`, no store registered.
    fn noting(fields: &[String]) -> Noting<'_> {
        static STORES: std::sync::LazyLock<skwir_core::notes::NoteStores> =
            std::sync::LazyLock::new(skwir_core::notes::NoteStores::default);
        Noting {
            fields,
            stores: &STORES,
        }
    }

    /// A file's whole path on this system (a drive's on Windows), and its
    /// folder.
    const FILE: &str = if cfg!(windows) {
        r"C:\data\notes\it's a café.txt"
    } else {
        "/data/notes/it's a café.txt"
    };
    const FOLDER: &str = if cfg!(windows) {
        r"C:\data\notes"
    } else {
        "/data/notes"
    };

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

    /// A folder's contents are one row, whatever it holds, so a folder of
    /// files alone has the rows a folder of both has (ADR-058).
    #[test]
    fn a_folder_s_contents_are_one_row_whatever_it_holds() {
        let said = |items, folders| {
            let row = contains(items, folders);
            (row.label.to_string(), row.value.to_string())
        };
        assert_eq!(said(0, 0), ("Contains".into(), "Nothing".into()));
        assert_eq!(said(3, 0).1, "3 files");
        assert_eq!(said(2, 2).1, "2 folders");
        assert_eq!(said(3, 1).1, "1 folder, 2 files");
        assert_eq!(
            contains(3, 1)
                .gist
                .as_ref()
                .map(Gist::words)
                .map(ToString::to_string),
            Some("3 items".into()),
            "folded, how many in all"
        );
    }

    /// What a source said it answers keeps its place, labelled and empty,
    /// for the kind of entry it answers for, and only for that kind.
    #[test]
    fn a_source_s_rows_keep_their_place_where_it_answers() {
        let source = skwir_core::registry::PropertySource {
            places: vec![skwir_core::registry::PropertyPlace {
                group: group_keys::GENERAL.into(),
                heading: None,
                label: Words::as_written("Opens with"),
                folded: false,
                of: skwir_core::registry::PlaceOf::Files,
            }],
            ..skwir_core::registry::PropertySource::new(Invocation::new("test.properties"))
        };
        let entry = |kind| {
            Entry::new(
                EntryId {
                    file: skwir_core::FileId::new(1, 2),
                    at: 0,
                },
                "it's a café.txt",
                kind,
            )
        };
        let path = std::path::Path::new(FILE);
        let kept = places(std::slice::from_ref(&source), &entry(EntryKind::File), path);
        assert_eq!(kept.len(), 1);
        assert_eq!(
            (kept[0].group.as_ref(), kept[0].row.label.as_ref()),
            ("General", "Opens with")
        );
        assert!(kept[0].row.value.is_empty() && !kept[0].row.copies);
        for kind in [EntryKind::Directory, EntryKind::Fifo] {
            assert!(
                places(std::slice::from_ref(&source), &entry(kind), path).is_empty(),
                "{kind:?} is not a file read"
            );
        }
    }

    #[test]
    fn an_entry_says_what_it_knows_and_who_said_it() {
        let mut entry = Entry::new(
            EntryId {
                file: skwir_core::FileId::new(1, 2),
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
        let path = std::path::Path::new(FILE);
        let groups = entry_groups(&entry, path, Some(noting(&[])), &User::default());
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
        assert_eq!(whole.value, FILE, "copied as it is");
        assert!(whole.copies);
        assert_eq!(find("Location", "Folder").unwrap().value, FOLDER);
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
        let with_field = entry_groups(&tagged, path, Some(noting(&fields)), &User::default());
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
            find("More", "vision.subject")
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
                file: skwir_core::FileId::new(1, 3),
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
                file: skwir_core::FileId::new(1, ino),
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
                row("Size", "3.1 MB so far…", &["Stop"]),
                row("On disk", "4.2 MB", &[]),
                row("In all", "1,500 files, 1 folder", &[]),
            ]
        );
        assert_eq!(running[0].actions[0].1.args["stop"], true);
        let stopped = count_rows("Total size", Some((so_far, false)));
        assert_eq!(
            said(&stopped[..1]),
            [row(
                "Total size",
                "At least 3.1 MB, stopped",
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
            [row("Size", "1.2 kB (1,234 bytes)", &["Count again"])]
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
                row("Size", "At least 1.2 kB", &["Count again"]),
                row("On disk", "4.2 MB", &[]),
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
            "At least 1.2 kB"
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
            entry_groups(entry, path, Some(noting(&[])), &me)
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
            heading: None,
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
    fn a_folded_group_says_its_gist() {
        let mut file = entry(20, "it's a café.txt", EntryKind::File, 4_000_000, None);
        let meta = &mut file.metadata;
        meta.set(keys::SIZE, Value::UInt(1_234_567), Provenance::Filesystem);
        meta.set(keys::MODE, Value::UInt(0o100_640), Provenance::Filesystem);
        let modified = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        meta.set(
            keys::MODIFIED,
            Value::Time(modified),
            Provenance::Filesystem,
        );
        meta.set(
            keys::ACCESSED,
            Value::Time(modified),
            Provenance::Filesystem,
        );
        meta.set(
            "tags",
            Value::List(vec![Value::Text("work".into())]),
            Provenance::User,
        );
        meta.set(keys::RATING, Value::UInt(7), Provenance::User);
        meta.set(
            keys::COMMENT,
            Value::Text("\n  First line\nsecond".into()),
            Provenance::User,
        );
        meta.set(
            keys::ORIGIN,
            Value::Text("https://someone@example.org:8080/it's a café.txt?x".into()),
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
        let path = std::path::Path::new(FILE);
        let groups = entry_groups(&file, path, Some(noting(&[])), &User::default());
        let gist = |groups: &[Group], title: &str| {
            groups
                .iter()
                .find(|group| group.title == title)
                .and_then(Group::gist)
        };
        let said = |words: &str| Some(Gist::Said(words.to_owned().into()));
        assert_eq!(
            gist(&groups, "General"),
            said("1.2 MB"),
            "without the bytes"
        );
        assert_eq!(
            gist(&groups, "Notes"),
            said("3½ stars · First line"),
            "the tags are chips beside it"
        );
        let notes = groups.iter().find(|group| group.title == "Notes").unwrap();
        assert_eq!(
            notes.tags().map(|(tags, _)| tags),
            Some(&["work".to_owned()][..])
        );
        assert_eq!(gist(&groups, "Source"), said("example.org:8080"));
        assert_eq!(
            gist(&groups, "Location"),
            Some(Gist::Path(FILE.into())),
            "the path, cut at its start: its end names the file"
        );
        assert_eq!(
            gist(&groups, "Permissions"),
            said(&format!("{} · rw-r----- (640)", owner_gist(4_000_000)))
        );
        assert_eq!(
            gist(&groups, "Times"),
            said(&format::time(modified)),
            "when it changed, to the minute"
        );
        assert_eq!(gist(&groups, "More"), said(&label_of("vision.subject")));
        // With room, each part says what it is where its words do not.
        let led = |groups: &[Group], title: &str| {
            groups
                .iter()
                .find(|group| group.title == title)
                .and_then(Group::gist_led)
        };
        assert_eq!(
            led(&groups, "Times"),
            said(&format!("Modified {}", format::time(modified)))
        );
        assert_eq!(
            led(&groups, "Notes"),
            said("3½ stars · Comment: First line")
        );
        assert_eq!(
            led(&groups, "Source"),
            said("Downloaded from example.org:8080")
        );
        assert_eq!(
            led(&groups, "Permissions"),
            said(&format!(
                "Owned by {} · rw-r----- (640)",
                owner_gist(4_000_000)
            ))
        );
        for title in ["General", "Location", "More"] {
            assert_eq!(led(&groups, title), None, "{title} would say no more");
        }

        // A file of the trash says where it was.
        file.metadata.set(
            keys::TRASHED_FROM,
            Value::Text(FOLDER.into()),
            Provenance::Filesystem,
        );
        let trashed = entry_groups(&file, path, None, &User::default());
        assert_eq!(gist(&trashed, "Location"), Some(Gist::Path(FOLDER.into())));
        assert_eq!(
            led(&trashed, "Location"),
            Some(Gist::Path(format!("Deleted from {FOLDER}").into()))
        );

        // A folder says its size once counted; the selection, how many
        // and what they weigh; what they share, unless it is mixed.
        let folder = entry(21, "a folder", EntryKind::Directory, 1000, Some(0o040_755));
        let groups = entry_groups(&folder, path, None, &User::default());
        let size = groups.iter().find(|g| g.title == "General").unwrap();
        assert!(size.gist().is_none(), "not counted, it says nothing");
        let done = skwir_core::fs::count::Count {
            done: true,
            size: 1234,
            ..Default::default()
        };
        let counted = Group::new("General", count_rows("Size", Some((done, false))));
        assert_eq!(counted.gist(), said("1.2 kB"));
        let so_far = skwir_core::fs::count::Count {
            size: 3 << 20,
            ..Default::default()
        };
        let stopped = Group::new("General", count_rows("Size", Some((so_far, false))));
        assert_eq!(
            stopped.gist(),
            said("At least 3.1 MB"),
            "the bound, not why"
        );
        let me = User {
            id: Some(1000),
            groups: vec![100],
        };
        let a = entry(22, "a's file.txt", EntryKind::File, 1000, Some(0o100_644));
        let b = entry(23, "b café", EntryKind::File, 1000, Some(0o100_755));
        let pane = skwir_core::workspace::PaneId(3);
        let groups = selection_groups(&[&a, &b], 2, Some(2048), pane, &me);
        assert_eq!(gist(&groups, "Selection"), said("2 items · 2.0 kB"));
        assert_eq!(
            gist(&groups, SHARED_PERMISSIONS),
            said(&owner_gist(1000)),
            "their modes differ"
        );

        // Volume's gist is its rows', in their order; a path in it cuts
        // it at its start.
        let volume = Group::new(
            "Volume",
            vec![
                Row::new("Volume", "Data").in_gist().led(Lead::On),
                Row::new("Device", "/dev/sdz1").led(Lead::OwnedBy),
                Row::new("Free", "1 GB of 2 GB").gist("1 GB free"),
            ],
        );
        assert_eq!(volume.gist(), said("Data · 1 GB free"));
        assert_eq!(volume.gist_led(), said("On Data · 1 GB free"));
        let mixed = Group::new(
            "Location",
            vec![
                Row::new("Size", "1 B").in_gist(),
                Row {
                    gist: Some(Gist::Path(path.to_string_lossy().into_owned().into())),
                    ..Row::path("Path", path)
                },
            ],
        );
        assert_eq!(
            mixed.gist(),
            Some(Gist::Path(format!("1 B · {FILE}").into()))
        );
    }

    #[test]
    fn times_say_which_is_which() {
        let at = |secs| std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs);
        let mut file = entry(30, "it's a café.txt", EntryKind::File, 1000, None);
        for (key, secs) in [
            (keys::ACCESSED, 1_700_000_400),
            (keys::CREATED, 1_700_000_100),
            (keys::CHANGED, 1_700_000_300),
            (keys::MODIFIED, 1_700_000_200),
            (keys::TRASHED, 1_700_000_500),
        ] {
            file.metadata
                .set(key, Value::Time(at(secs)), Provenance::Filesystem);
        }
        let path = std::path::Path::new(FILE);
        let times = |entry: &Entry| {
            entry_groups(entry, path, None, &User::default())
                .into_iter()
                .find(|group| group.title == "Times")
                .unwrap()
                .rows
        };
        let rows = times(&file);
        let labels: Vec<&str> = rows.iter().map(|row| row.label.as_ref()).collect();
        assert_eq!(
            labels,
            ["Deleted", "Modified", "Changed", "Created", "Accessed"],
            "the change time beside the modification time"
        );
        assert_eq!(rows[2].value, format::exact_time(at(1_700_000_300)));
        assert!(
            rows.iter().all(|row| row.tip.is_some()),
            "each says which time it is"
        );
        assert!(
            entry_groups(&file, path, None, &User::default())
                .iter()
                .all(|group| group.title != "More"),
            "the change time is not a field of its own"
        );
        // A folder's modification time is when a name in it changed.
        let mut folder = entry(31, "a folder", EntryKind::Directory, 1000, None);
        folder
            .metadata
            .set(keys::MODIFIED, Value::Time(at(1)), Provenance::Filesystem);
        let modified = |rows: &[Row]| {
            rows.iter()
                .find(|r| r.label == "Modified")
                .unwrap()
                .tip
                .clone()
        };
        assert_ne!(modified(&times(&folder)), modified(&rows));
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
        assert_eq!(find(&groups, "Total size").value, "2.0 kB");
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
            skwir_core::FileId::new(1, 2),
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
        // Words as their data, read back as the window reads them.
        let english = |said: &serde_json::Value| {
            serde_json::from_value::<Words>(said.clone())
                .unwrap()
                .english()
        };
        assert_eq!(english(&choices[2]["detail"]), "Now");
        assert!(owner.args.get("option").is_none(), "a file holds nothing");
        // The group goes to one of the user's.
        let groups = rows[1].actions[0].1.args["choices"].as_array().unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0]["command"]["args"]["group"], 10);
        // The mode as chmod writes it, ready to edit.
        assert_eq!(rows[6].actions[0].1.args["text"], "644");
        // The question passes the window's check of `ui.ask`, says what a
        // mode is written as, and what it asks to say what is typed comes
        // to is answered (ADR-137).
        let ask = &rows[6].actions[0].1;
        let tmp = tempfile::tempdir().unwrap();
        let app = skwir_core::App::new(skwir_core::app::AppConfig::ephemeral(tmp.path())).unwrap();
        crate::commands::declare_all(&app);
        app.commands.check(ask).unwrap();
        assert_eq!(english(&ask.args["reference"]["title"]), "Access");
        let describe = &ask.args["describe"];
        let mut args = describe["args"].clone();
        args["mode"] = "u+x".into();
        let Ok(skwir_core::command::Outcome::Value(said)) = app.dispatch(&Invocation::with_args(
            describe["command"].as_str().unwrap(),
            &args,
        )) else {
            panic!("no words for u+x")
        };
        if !cfg!(windows) {
            assert!(
                english(&said["says"]).starts_with("rwxr--r-- (744)"),
                "{said}"
            );
        }
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
