//! Properties: everything known about what the panel shows (ADR-058).
//!
//! A section of the preview frame, not a kind of preview and not a dialog
//! of its own: what other file managers open as a Properties window
//! (Alt+Enter, Ctrl+I, the context menu's last item) is this, following
//! the selection. With nothing to preview (a folder, an unknown file,
//! several entries) it is always open; under a preview it opens on demand
//! in the same place, under the header's card and above the toolbar
//! (`preview::under_card`, the one place that draws it), as tall as what
//! it says, up to the whole panel.
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
    pub actions: Vec<(SharedString, Invocation)>,
}

impl Row {
    pub fn new(label: impl Into<SharedString>, value: impl Into<SharedString>) -> Self {
        Row {
            label: label.into(),
            value: value.into(),
            show: Show::Text,
            source: None,
            actions: Vec::new(),
        }
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
}

fn group(title: &'static str, rows: Vec<Row>) -> Option<Group> {
    (!rows.is_empty()).then(|| Group {
        title: title.into(),
        rows,
    })
}

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
pub fn entry_groups(entry: &Entry, path: &std::path::Path, notes: Option<&[String]>) -> Vec<Group> {
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
    if let Some(parent) = path.parent() {
        location.push(Row::new("Folder", parent.to_string_lossy().into_owned()));
    }
    if let Some(target) = meta.value(keys::LINK_TARGET).and_then(Value::as_str) {
        location.push(Row::new("Links to", target.to_owned()));
    }

    let mut permissions = Vec::new();
    if let Some(uid) = uint(keys::UID) {
        permissions.push(Row::new("Owner", owner_named(uid)));
    }
    if let Some(gid) = uint(keys::GID) {
        permissions.push(Row::new("Group", group_named(gid)));
    }
    if let Some(mode) = uint(keys::MODE) {
        permissions.push(Row::new("Access", skwir_platform::users::mode(mode)));
    }
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
/// a count where folders are among them), and the owner, group and access
/// they share, "Mixed" where they differ. A fact some entry does not say
/// yet is left out. Pure, so it is tested without a window.
pub fn selection_group(entries: &[&Entry], selected: usize, bytes: Option<u64>) -> Group {
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
    // Shared, mixed, or unknown (None) while some entry does not say it.
    let shared = |key: &str, bits: u64| -> Option<Option<u64>> {
        let mut values = entries.iter().map(|e| {
            e.metadata
                .value(key)
                .and_then(Value::as_u64)
                .map(|v| v & bits)
        });
        let first = values.next()??;
        let mut mixed = false;
        for value in values {
            mixed |= value? != first;
        }
        Some((!mixed).then_some(first))
    };
    let mixed = || "Mixed".to_owned();
    if let Some(uid) = shared(keys::UID, u64::MAX) {
        rows.push(Row::new("Owner", uid.map_or_else(mixed, owner_named)));
    }
    if let Some(gid) = shared(keys::GID, u64::MAX) {
        rows.push(Row::new("Group", gid.map_or_else(mixed, group_named)));
    }
    if let Some(mode) = shared(keys::MODE, 0o7777) {
        rows.push(Row::new(
            "Access",
            mode.map_or_else(mixed, skwir_platform::users::mode),
        ));
    }
    Group {
        title: "Selection".into(),
        rows,
    }
}

/// The rows of a count of what folders hold (`Show::Count`), labelled
/// `label`: the link that counts, what is counted so far with the link
/// that stops, or the total, the room it takes and how many files and
/// folders, and what it could not count.
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
        Row::new(label.to_owned(), format!("At least {size}"))
    } else {
        Row::new(
            label.to_owned(),
            format!("{size} ({} bytes)", format::thousands(now.size as usize)),
        )
    };
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
                "{} could not be read",
                format::count(now.unreadable as usize, "folder")
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
        }) if document.is_none() && preview.media.session.is_none() => {
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
    if preview.media.session.is_some() {
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
    shell.preview.media.session.is_none()
        && shell.preview.parked().is_none()
        && match &shell.preview.content {
            None => shell.preview.phase == crate::preview::PreviewPhase::Idle,
            Some(
                PreviewContent::Generic | PreviewContent::Dir { .. } | PreviewContent::Error(_),
            ) => true,
            Some(_) => false,
        }
}

/// The section: right under the header's card, above the toolbar
/// (`preview::under_card` puts it there, and nothing else draws it). Opened
/// on demand under a preview or above a container's kept rows; always
/// open when `alone`. As tall as what it says, up to the whole panel (what
/// is under it takes the rest), scrolling only when the panel is too
/// short. The header's card (or its Properties button) is what opens it,
/// and holds the name, type, size and date, so the groups here repeat none
/// of those.
pub fn render(shell: &Shell, theme: &Theme, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let preview = &shell.preview;
    // Nothing to say (a search named with nothing selected): no section,
    // not an empty one with its scrollbar.
    let nothing = preview.properties.is_empty()
        && preview.selection_summary.is_none()
        && preview.fields.is_empty();
    if preview.info.name.is_empty() || nothing {
        return None;
    }
    let alone = alone(shell);
    if !alone && !preview.properties_open {
        return None;
    }
    let section = div()
        .flex()
        .flex_col()
        .min_h_0()
        .bg(theme.bg)
        .border_b_1()
        .border_color(theme.border)
        .flex_initial();
    let several = preview.selection_summary.is_some();
    let count = preview.count.as_ref().map(|c| (c.now, c.running));
    let groups = preview
        .selection_summary
        .iter()
        .chain(preview_group(shell).iter())
        .chain(preview.properties.iter())
        .chain(preview.volume.iter())
        .cloned()
        .map(|mut group| {
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
    let mut list = div()
        .id("details-list")
        .track_scroll(&preview.properties_scroll)
        .overflow_y_scroll()
        .flex_1()
        .min_h_0()
        .px_3()
        .pb_3()
        .text_xs();
    // A group's title is a heading, not one more label: in the text's own
    // color and weight where the labels are muted, and ruled off from the
    // group above it.
    for (g, group) in groups.into_iter().enumerate() {
        list = list.child(
            div()
                .when(g > 0, |d| d.mt_2().border_t_1().border_color(theme.border))
                .pt_3()
                .pb_1()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(theme.text)
                .child(group.title),
        );
        for (r, row) in group.rows.into_iter().enumerate() {
            let (label, value) = (row.label.clone(), row.value.clone());
            let id = g * 1000 + r;
            let shown = match &row.show {
                Show::Text => (!row.value.is_empty())
                    .then(|| div().child(row.value.clone()).into_any_element()),
                Show::Tags { tags, given } => (!tags.is_empty())
                    .then(|| tag_chips(shell, tags, given, id, theme, cx).into_any_element()),
                Show::Rating { rating, target } => {
                    Some(star_picker(*rating, target, id, theme, cx).into_any_element())
                }
                Show::Count { .. } => None,
            };
            list = list.child(
                div()
                    .id(("detail", g * 1000 + r))
                    .flex()
                    .items_start()
                    .gap_2()
                    .py(px(3.))
                    .rounded_sm()
                    .cursor_pointer()
                    .hover(|d| d.bg(theme.hover))
                    .on_click(cx.listener(move |this, _, _, cx| {
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
                            .children(shown)
                            .when_some(row.source, |d, source| {
                                d.child(div().text_color(theme.text_muted).child(source))
                            })
                            .when(!row.actions.is_empty(), |d| {
                                d.child(div().flex().flex_wrap().gap_x_3().children(
                                    row.actions.into_iter().enumerate().map(
                                        |(a, (words, action))| {
                                            div()
                                                .id(("detail-action", id * 8 + a))
                                                .text_color(theme.accent)
                                                .hover(gpui::Styled::underline)
                                                .child(words)
                                                .on_click(cx.listener(
                                                    move |this, _, window, cx| {
                                                        cx.stop_propagation();
                                                        this.run(
                                                            &action.name,
                                                            action.args.clone(),
                                                            window,
                                                            cx,
                                                        );
                                                    },
                                                ))
                                        },
                                    ),
                                ))
                            }),
                    ),
            );
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

#[cfg(test)]
mod tests {
    use super::*;
    use skwir_core::entry::{EntryId, EntryKind};

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
        let groups = entry_groups(&entry, path, Some(&[]));
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
        let with_field = entry_groups(&tagged, path, Some(&fields));
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
        let without = entry_groups(&entry, path, None);
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
        let general = entry_groups(&folder, path, None);
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
        assert_eq!(
            said(&count_rows("Size", Some((done, false)))[..1]),
            [row("Size", "1.2 KB (1,234 bytes)", &[])]
        );
        let partial = Count {
            unreadable: 2,
            other_drives: 1,
            ..done
        };
        assert_eq!(
            said(&count_rows("Size", Some((partial, false)))),
            [
                row("Size", "At least 1.2 KB", &[]),
                row("On disk", "4.0 MB", &[]),
                row("In all", "1,500 files, 1 folder", &[]),
                row("Unreadable", "2 folders could not be read", &[]),
                row("Not counted", "1 drive mounted inside", &[]),
            ]
        );
    }

    #[test]
    fn several_entries_say_what_they_share() {
        let a = entry(10, "a's file.txt", EntryKind::File, 1000, Some(0o100_644));
        let b = entry(11, "b café", EntryKind::File, 1000, Some(0o100_644));
        let group = selection_group(&[&a, &b], 2, Some(2048));
        assert_eq!(
            said(&group.rows)
                .into_iter()
                .map(|(l, v, _)| (l, v))
                .filter(|(l, _)| l != "Owner")
                .collect::<Vec<_>>(),
            [
                ("Selected".to_owned(), "2 items".to_owned()),
                ("Total size".to_owned(), "2.0 KB".to_owned()),
                ("Access".to_owned(), "rw-r--r-- (644)".to_owned()),
            ]
        );
        let dir = entry(12, "a folder", EntryKind::Directory, 0, Some(0o040_755));
        let group = selection_group(&[&a, &b, &dir], 3, Some(2048));
        let find = |label: &str| group.rows.iter().find(|r| r.label == label).cloned();
        assert_eq!(find("Of which").unwrap().value, "1 folder, 2 files");
        assert_eq!(
            find("Total size").unwrap().show,
            Show::Count { selection: true },
            "a folder's files are counted, not left out"
        );
        assert_eq!(find("Owner").unwrap().value, "Mixed");
        assert_eq!(find("Access").unwrap().value, "Mixed");
        let unknown = entry(13, "not stat-ed yet", EntryKind::File, 1000, None);
        let group = selection_group(&[&a, &unknown], 2, None);
        assert!(
            group
                .rows
                .iter()
                .all(|r| r.label != "Access" && r.label != "Total size"),
            "nothing said before every entry says it"
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
