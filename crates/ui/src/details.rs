//! Details: everything known about what the panel shows.
//!
//! A section of the preview frame, not a kind of preview. With nothing to
//! preview (a folder, an unknown file, several entries) it fills the panel;
//! under a preview it is one line at the bottom that opens on demand.
//!
//! Facts come from three places, none of them read while painting: the entry's
//! metadata, with its provenance, collected when the entry or its listing
//! changes (`entry_groups`); what the preview already holds in memory (a
//! picture's size, a document's pages, a film's length); and what the format
//! itself told us (`Inspection::details`). Details never starts a scan.
use std::time::SystemTime;

use bibli_core::command::Invocation;
use bibli_core::entry::{Entry, Provenance, Value, keys};
use bibli_core::op::EntryRef;
use gpui::{AnyElement, Context, SharedString, div, prelude::*, px};

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
    fn of(detail: &bibli_core::inspection::Detail) -> Self {
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

/// Seconds too: Details is where the exact value is.
fn exact_time(t: SystemTime) -> String {
    jiff::Timestamp::try_from(t).map_or_else(
        |_| "—".into(),
        |ts| {
            ts.to_zoned(jiff::tz::TimeZone::system())
                .strftime("%Y-%m-%d %H:%M:%S")
                .to_string()
        },
    )
}

fn text_of(value: &Value) -> String {
    match value {
        Value::Bool(v) => if *v { "Yes" } else { "No" }.into(),
        Value::Int(v) => v.to_string(),
        Value::UInt(v) => v.to_string(),
        Value::Float(v) => format!("{v:.3}"),
        Value::Text(v) => v.to_string(),
        Value::Time(v) => exact_time(*v),
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

/// The fields Details names itself; any other is shown under More.
const CORE: [&str; 17] = [
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
    let target = serde_json::json!([EntryRef::new(entry.id, path)]);
    let (valued, tags): (Vec<String>, Vec<String>) = crate::views::list::tags_of(entry)
        .into_iter()
        .partition(|t| bibli_core::metadata::exclusive_category(t, fields).is_some());
    let field_rows: Vec<Row> = valued
        .iter()
        .filter_map(|t| {
            let field = bibli_core::metadata::exclusive_category(t, fields)?;
            let value = bibli_core::metadata::tag_under(t, field)?.trim_start_matches('/');
            Some(
                Row::new(
                    bibli_core::metadata::tag_leaf(field).to_owned(),
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
            target: EntryRef::new(entry.id, path),
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
        "command": "tags.comment",
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
    // A URL is opened only when asked, in the browser: bibli fetches
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
            Invocation::with_args("tags.agree", &serde_json::json!({ "entries": target })),
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
    // is the one general fact it rounds.
    let general = uint(keys::SIZE)
        .filter(|_| !entry.is_dir())
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
        .collect();

    let mut location = Vec::new();
    if let Some(parent) = path.parent() {
        location.push(Row::new("Folder", parent.to_string_lossy().into_owned()));
    }
    if let Some(target) = meta.value(keys::LINK_TARGET).and_then(Value::as_str) {
        location.push(Row::new("Links to", target.to_owned()));
    }

    let named = |id: u64, name: Option<String>| match name {
        Some(name) => format!("{name} ({id})"),
        None => id.to_string(),
    };
    let mut permissions = Vec::new();
    if let Some(uid) = uint(keys::UID) {
        permissions.push(Row::new(
            "Owner",
            named(uid, bibli_platform::users::user(uid)),
        ));
    }
    if let Some(gid) = uint(keys::GID) {
        permissions.push(Row::new(
            "Group",
            named(gid, bibli_platform::users::group(gid)),
        ));
    }
    if let Some(mode) = uint(keys::MODE) {
        permissions.push(Row::new("Access", bibli_platform::users::mode(mode)));
    }
    if let Some(links) = uint(keys::NLINK).filter(|links| *links > 1 && !entry.is_dir()) {
        permissions.push(Row::new("Hard links", links.to_string()));
    }

    let times = [
        ("Modified", keys::MODIFIED),
        ("Created", keys::CREATED),
        ("Accessed", keys::ACCESSED),
    ]
    .into_iter()
    .filter_map(|(label, key)| Some(Row::new(label, exact_time(time(key)?))))
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
                        "tags.rate",
                        serde_json::json!({ "stars": stars, "entries": [target] }),
                        window,
                        cx,
                    );
                }))
        }))
}

/// Whether there is nothing to show but the details. A preview still on
/// its way is not nothing: Details waits for the answer rather than
/// filling the body for the instant before a picture replaces it.
pub fn alone(shell: &Shell) -> bool {
    shell.preview.media.session.is_none()
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
/// on demand under a preview, taking up to half the panel; always open when
/// `alone`, as tall as what it says, scrolling only when the panel is too
/// short. Above a container's kept rows (`kept`) it takes up to half the
/// panel either way: the rows are content too. The header's card is what
/// opens it, and holds the name, type, size and date, so the groups here
/// repeat none of those.
pub fn render(
    shell: &Shell,
    theme: &Theme,
    kept: bool,
    cx: &mut Context<Shell>,
) -> Option<AnyElement> {
    let preview = &shell.preview;
    // Nothing to say (a search named with nothing selected): no section,
    // not an empty one with its scrollbar.
    let nothing = preview.details.is_empty()
        && preview.selection_summary.is_none()
        && preview.fields.is_empty();
    if preview.info.name.is_empty() || nothing {
        return None;
    }
    let alone = alone(shell);
    if !alone && !preview.details_open {
        return None;
    }
    let section = div()
        .flex()
        .flex_col()
        .min_h_0()
        .bg(theme.bg)
        .border_b_1()
        .border_color(theme.border)
        .when(alone && !kept, gpui::Styled::flex_initial)
        .when(!alone || kept, |d| d.flex_none().max_h(gpui::relative(0.5)));
    let groups = preview
        .selection_summary
        .iter()
        .chain(preview_group(shell).iter())
        .chain(preview.details.iter())
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
            group
        })
        .collect::<Vec<_>>();
    let mut list = div()
        .id("details-list")
        .track_scroll(&preview.details_scroll)
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
                        crate::elements::scrollbar::Scrolls::Offset(preview.details_scroll.clone()),
                    )),
            )
            .into_any_element(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use bibli_core::entry::{EntryId, EntryKind};

    #[test]
    fn an_entry_says_what_it_knows_and_who_said_it() {
        let mut entry = Entry::new(
            EntryId { dev: 1, ino: 2 },
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
    }
}
