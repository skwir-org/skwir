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

use bibli_core::entry::{Entry, Provenance, Value, keys};
use gpui::{AnyElement, Context, SharedString, div, prelude::*, px};

use crate::format;
use crate::preview::PreviewContent;
use crate::shell::Shell;
use crate::theme::Theme;

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub label: SharedString,
    pub value: SharedString,
    /// Who says so, when it is not the filesystem: the user, a plugin, a model.
    pub source: Option<SharedString>,
}

impl Row {
    pub fn new(label: impl Into<SharedString>, value: impl Into<SharedString>) -> Self {
        Row {
            label: label.into(),
            value: value.into(),
            source: None,
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
const CORE: [&str; 9] = [
    keys::SIZE,
    keys::MODIFIED,
    keys::CREATED,
    keys::ACCESSED,
    keys::MODE,
    keys::NLINK,
    keys::UID,
    keys::GID,
    keys::LINK_TARGET,
];

/// What the entry itself says. Pure, so it is tested without a window.
pub fn entry_groups(entry: &Entry, path: &std::path::Path, kind: Option<String>) -> Vec<Group> {
    let meta = &entry.metadata;
    let uint = |key| meta.value(key).and_then(Value::as_u64);
    let time = |key| meta.value(key).and_then(Value::as_time);

    let mut general = vec![Row::new("Name", entry.name_str().into_owned())];
    if let Some(kind) = kind {
        general.push(Row::new("Type", kind));
    }
    if let Some(size) = uint(keys::SIZE).filter(|_| !entry.is_dir()) {
        general.push(Row::new(
            "Size",
            format!(
                "{} ({} bytes)",
                format::size(size),
                format::thousands(size as usize)
            ),
        ));
    }

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
            label: label_of(key).into(),
            value: text_of(&field.value).into(),
            source: source_of(&field.provenance),
        })
        .filter(|row| !row.value.is_empty())
        .collect();

    [
        group("General", general),
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
            rows.extend(
                facts
                    .iter()
                    .map(|fact| Row::new(fact.label.clone(), fact.value.clone())),
            );
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
        Some(PreviewContent::Inspection(display)) => rows.extend(
            display
                .data
                .details
                .iter()
                .map(|detail| Row::new(detail.label.clone(), detail.value.clone())),
        ),
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
        if !state.subtitle_tracks.is_empty() {
            rows.push(Row::new("Subtitles", state.subtitle_tracks.join(", ")));
        }
    }
    group("Content", rows)
}

/// Whether there is nothing to show but the details.
pub fn alone(shell: &Shell) -> bool {
    shell.preview.media.session.is_none()
        && matches!(
            shell.preview.content,
            None | Some(
                PreviewContent::Generic | PreviewContent::Dir { .. } | PreviewContent::Error(_)
            )
        )
}

/// The section: a bar that opens under a preview, or the whole of the panel's
/// body when `alone`.
pub fn render(shell: &Shell, theme: &Theme, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let preview = &shell.preview;
    if preview.info.name.is_empty() {
        return None;
    }
    let alone = alone(shell);
    let open = alone || preview.details_open;
    let mut section = div()
        .flex()
        .flex_col()
        .min_h_0()
        .bg(theme.bg)
        .when(alone, gpui::Styled::flex_1)
        .when(!alone, |d| {
            d.flex_none()
                .max_h(gpui::relative(0.5))
                .border_t_1()
                .border_color(theme.border)
        });
    if !alone {
        let summary = [preview.info.size.clone(), preview.info.modified.clone()]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · ");
        section = section.child(
            div()
                .id("details-bar")
                .flex_none()
                .h(px(28.))
                .px_2()
                .flex()
                .items_center()
                .gap_2()
                .text_xs()
                .bg(theme.surface)
                .cursor_pointer()
                .hover(|d| d.bg(theme.hover))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.run("ui.preview_details", serde_json::json!({}), window, cx);
                }))
                .child(
                    gpui::svg()
                        .path(if open {
                            "toolbar/chevron.svg"
                        } else {
                            "toolbar/chevron-right.svg"
                        })
                        .size(px(12.))
                        .text_color(theme.text_muted),
                )
                .child("Details")
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(theme.text_muted)
                        .child(summary),
                ),
        );
    }
    if !open {
        return Some(section.into_any_element());
    }
    let groups = preview
        .selection_summary
        .iter()
        .chain(preview_group(shell).iter())
        .chain(preview.details.iter())
        .cloned()
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
    for (g, group) in groups.into_iter().enumerate() {
        list = list.child(
            div()
                .pt_3()
                .pb_1()
                .text_color(theme.text_muted)
                .child(group.title),
        );
        for (r, row) in group.rows.into_iter().enumerate() {
            let (label, value) = (row.label.clone(), row.value.clone());
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
                            .child(row.value)
                            .when_some(row.source, |d, source| {
                                d.child(div().text_color(theme.text_muted).child(source))
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
        meta.set(
            "vision.subject",
            Value::Text("a harbour".into()),
            Provenance::Model {
                name: "vision".into(),
                confidence: 0.82,
            },
        );
        let groups = entry_groups(
            &entry,
            std::path::Path::new("/data/notes/it's a café.txt"),
            Some("Plain text document".into()),
        );
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
        let tags = find("More", "Tags").unwrap();
        assert_eq!(
            (tags.value.as_ref(), tags.source.as_ref().map(AsRef::as_ref)),
            ("work, draft", Some("set by you"))
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
