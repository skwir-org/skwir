//! The list view: one row per entry with icon, name, size, modified;
//! clickable, sortable headers. The tree view is the same list whose
//! folders open in place (`Pane::expanded`, the core `expand` command):
//! their entries follow them, indented, and are the pane's like any other,
//! so selection, commands and drags need nothing of their own.

use std::ops::Range;
use std::path::PathBuf;

use bibli_core::entry::keys;
use bibli_core::{EntryId, SortKey};
use gpui::{
    AnyElement, Context, Div, MouseButton, MouseDownEvent, ScrollStrategy, Stateful,
    UniformListScrollHandle, Window, div, prelude::*, px, uniform_list,
};

use crate::dnd;
use crate::format;
use crate::icons;
use crate::list::Step;
use crate::shell::Shell;
use crate::theme::Theme;
use crate::view::{Nav, PaneView};

/// Height of a row at the default size: the archive preview's rows too.
pub const ROW_HEIGHT: f32 = 26.0;
/// The icon sizes the list and the columns zoom through (`ui.zoom`); a
/// row is its icon and a margin, so Ctrl+wheel makes rows taller.
pub const ROW_ZOOMS: [u32; 5] = [16, 18, 24, 32, 48];
pub const DEFAULT_ROW_ZOOM: u32 = 18;

/// Height of a row whose icon is `icon` px.
pub fn row_height(icon: u32) -> f32 {
    icon_px(icon) + ROW_HEIGHT - icon_px(DEFAULT_ROW_ZOOM)
}

/// A row zoom as the icon's side in pixels.
pub fn icon_px(zoom: u32) -> f32 {
    f32::from(u16::try_from(zoom).unwrap_or(u16::MAX))
}
/// The name never gets narrower than this; detail columns give way first.
const NAME_MIN: f32 = 150.0;
/// The tree's disclosure triangle, and how far each level is indented: a
/// child's triangle stands under its parent's icon.
const DISCLOSURE: f32 = 12.0;
const INDENT: f32 = DISCLOSURE + 4.0;
/// How much of a cut entry remains visible until it is pasted.
pub const CUT_OPACITY: f32 = 0.45;

/// Size, Modified, Type, Rating, Folder.
pub const DEFAULT_WIDTHS: [f32; 5] = [80.0, 136.0, 150.0, 76.0, 180.0];
const DETAIL_GAP: f32 = 12.0;

pub fn clamp_width(width: f32) -> f32 {
    width.clamp(56.0, 360.0)
}

/// Payload of a header edge drag.
#[derive(Clone)]
pub struct ColumnResize(pub usize);

/// Nothing follows the pointer while an edge is dragged.
struct NoDragPreview;
impl gpui::Render for NoDragPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

impl Shell {
    /// The columns are right-aligned, so a column grows as its left edge
    /// moves left; the name takes what is left.
    pub fn on_column_drag(
        &mut self,
        ev: &gpui::DragMoveEvent<ColumnResize>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let x = f32::from(ev.event.position.x);
        let column = ev.drag(cx).0;
        // A drag starts clean (`resize_edge` clears the state), so a release
        // missed outside the window cannot leave another edge's start here.
        let (_, start_x, start_width) =
            *self
                .column_drag
                .get_or_insert((column, x, self.column_widths[column]));
        self.column_widths[column] = clamp_width(start_width + (start_x - x));
        cx.notify();
    }

    /// The drag ended: remember the widths. Saved on a worker.
    pub fn finish_column_drag(&mut self) {
        if self.column_drag.take().is_none() {
            return;
        }
        self.store_column_widths();
    }

    fn store_column_widths(&self) {
        self.app.settings.set(
            crate::settings::COLUMN_WIDTHS_KEY,
            toml::Value::Array(
                self.column_widths
                    .iter()
                    .map(|width| toml::Value::Integer(width.round() as i64))
                    .collect(),
            ),
        );
        self.save_settings_later();
    }

    /// `ui.columns`: show or hide one detail column, from the header's menu.
    /// It goes through `settings.apply` like any other preference, so the
    /// dialog and the header always agree and the choice outlives the window.
    pub fn toggle_column(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(index) = crate::settings::COLUMN_KEYS.iter().position(|k| *k == key) else {
            return;
        };
        let shown = !self.columns[index];
        self.columns[index] = shown;
        cx.notify();
        if let Err(error) = self
            .app
            .dispatch(&bibli_core::command::Invocation::with_args(
                "settings.apply",
                &serde_json::json!({ "changes": { key: shown.to_string() } }),
            ))
        {
            self.set_status(format!("{error:#}"), true, cx);
        }
    }

    /// The detail columns drawn: those chosen, but Rating only while the
    /// tags plugin, whose ratings they are, is on.
    pub fn shown_columns(&self) -> [bool; 5] {
        let [size, modified, kind, rating, folder] = self.columns;
        let rated = rating && self.app.commands.get("tags.rate").is_some();
        // Where files from many folders are listed: a folder's are in it.
        let elsewhere = folder && !self.ui().place.folder;
        [size, modified, kind, rated, elsewhere]
    }

    /// `ui.columns_reset`: back to the widths the view ships with.
    pub fn reset_column_widths(&mut self, cx: &mut Context<Self>) {
        self.column_widths = DEFAULT_WIDTHS;
        self.store_column_widths();
        cx.notify();
    }
}

/// The draggable left edge of a header cell.
fn resize_edge(column: usize, cx: &mut Context<Shell>) -> Stateful<Div> {
    let shell = cx.weak_entity();
    div()
        .id(("column-edge", column))
        .absolute()
        .top_0()
        // Inside the cell: the columns' container clips what leaves it.
        .left_0()
        .w(px(7.))
        .h_full()
        .cursor(gpui::CursorStyle::ResizeLeftRight)
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_drag(ColumnResize(column), move |_, _, _, cx| {
            // The first move of this drag records where it starts.
            shell.update(cx, |shell, _| shell.column_drag = None).ok();
            cx.new(|_| NoDragPreview)
        })
}

/// The row at the top of rows `height` tall scrolled to `offset_y`, and how
/// many pixels into it the view begins.
fn row_anchor(offset_y: f32, height: f32) -> (usize, f32) {
    let top = (-offset_y).max(0.);
    let row = (top / height).floor();
    (row as usize, top - row * height)
}

/// The offset that shows `anchor` again (the list clamps it to its rows).
fn row_offset((row, within): (usize, f32), height: f32) -> f32 {
    -(row as f32 * height + within.clamp(0., height - 1.))
}

pub struct ListView {
    scroll: UniformListScrollHandle,
    /// The rows' height when last drawn, for the scroll anchor of a tab
    /// put away and shown again.
    height: f32,
    /// The tree view: folders open in place.
    tree: bool,
}

impl Default for ListView {
    fn default() -> Self {
        ListView {
            scroll: UniformListScrollHandle::default(),
            height: ROW_HEIGHT,
            tree: false,
        }
    }
}

impl ListView {
    pub fn tree() -> Self {
        ListView {
            tree: true,
            ..ListView::default()
        }
    }
}

/// Everything a row needs, collected under the workspace lock.
#[allow(clippy::struct_excessive_bools)]
pub struct RowData {
    pub ix: usize,
    pub id: EntryId,
    pub name: String,
    pub is_dir: bool,
    pub path: Option<PathBuf>,
    pub size: String,
    pub modified: String,
    /// Exact timestamp, shown as the date's tooltip.
    pub modified_exact: String,
    pub kind: std::sync::Arc<str>,
    pub selected: bool,
    /// Cut and waiting for a paste: drawn dimmed, as the file is on its way out.
    pub cut: bool,
    pub is_cursor: bool,
    pub icon: Option<std::sync::Arc<std::path::Path>>,
    pub hydrated: bool,
    /// Each tag with its color (`tags::TagColors`).
    pub tags: Vec<(String, usize)>,
    /// Half stars, from 1 to 10.
    pub rating: Option<u64>,
    pub link: icons::Link,
    /// How many opened folders deep it is (`Pane::depth`), and for a
    /// folder whether it is open in place.
    pub depth: usize,
    pub expanded: bool,
    /// The folder it is in, and its whole path, where files from many
    /// folders are listed.
    pub folder: Option<(String, String)>,
    /// A place (a tag's): where it leads, and its tag's color.
    pub place: Option<(bibli_core::Location, usize)>,
}

/// Tags from the entry's `tags` metadata field (a list of text values).
pub fn tags_of(e: &bibli_core::Entry) -> Vec<String> {
    match e.metadata.value(keys::TAGS) {
        Some(bibli_core::Value::List(items)) => items
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect(),
        Some(bibli_core::Value::Text(t)) => vec![t.to_string()],
        _ => Vec::new(),
    }
}

impl Shell {
    /// Collect row data for `range` of the sorted order. `compact` rows
    /// (the Columns view) show a name only: their details are not computed.
    pub fn rows(&mut self, range: Range<usize>, icon_px: u16, compact: bool) -> Vec<RowData> {
        let mut out = Vec::with_capacity(range.len());
        let friendly = self.prefs.friendly_dates;
        let columns = self.shown_columns();
        let now = (!compact && friendly && columns[1]).then(jiff::Zoned::now);
        let app = std::sync::Arc::clone(&self.app);
        let ws = app.workspace.read();
        let Some(pane) = ws.pane(self.pane) else {
            return out;
        };
        let cursor = pane.selection.cursor;
        for ix in range {
            let Some(&id) = self.ui().list.sorted.get(ix) else {
                break;
            };
            let Some(e) = pane.entry(id) else {
                continue;
            };
            let mime = (!e.is_dir())
                .then(|| bibli_platform::mime::of_entry(e))
                .flatten();
            let icon = if e.is_dir() {
                self.icons.folder(icon_px)
            } else {
                self.icons.for_mime(mime.as_ref(), icon_px)
            };
            let path = if e.is_dir() { pane.path_of(id) } else { None };
            let place = pane.location_of(id).map(|place| {
                let tag = e
                    .metadata
                    .value(keys::PLACE_TAG)
                    .and_then(bibli_core::Value::as_str)
                    .unwrap_or_default();
                (place, self.tag_colors.of(tag))
            });
            let mut row = RowData {
                ix,
                id,
                name: e.name_str().into_owned(),
                is_dir: e.is_dir(),
                expanded: e.is_dir() && pane.is_open_at(id),
                depth: pane.depth(id),
                path,
                size: String::new(),
                modified: String::new(),
                modified_exact: String::new(),
                kind: std::sync::Arc::from(""),
                selected: pane.selection.contains(id),
                cut: self.pending.cut && self.pending.entries.contains(&id),
                is_cursor: cursor == Some(id),
                icon,
                hydrated: e.metadata.is_hydrated(),
                tags: crate::tags::entry_tags(e, &self.tag_colors),
                rating: None,
                link: icons::Link::of(e),
                place,
                folder: None,
            };
            if !compact {
                let [size, modified, kind, rating, folder] = columns;
                if folder && !e.is_place() {
                    row.folder = pane.path_of(id).and_then(|path| {
                        let dir = path.parent()?;
                        let name = dir.file_name().map_or_else(
                            || dir.display().to_string(),
                            |n| n.to_string_lossy().into_owned(),
                        );
                        Some((name, dir.display().to_string()))
                    });
                }
                if rating {
                    row.rating = e
                        .metadata
                        .value(keys::RATING)
                        .and_then(bibli_core::Value::as_u64);
                }
                if size {
                    row.size = match e
                        .metadata
                        .value(keys::SIZE)
                        .and_then(bibli_core::Value::as_u64)
                    {
                        Some(s) if !e.is_dir() => format::size(s),
                        // A place says how many files it holds.
                        _ => match e
                            .metadata
                            .value(keys::FILES)
                            .and_then(bibli_core::Value::as_u64)
                        {
                            Some(1) => "1 file".to_owned(),
                            Some(n) => format!("{n} files"),
                            None => "—".to_owned(),
                        },
                    };
                }
                if modified {
                    let time = e
                        .metadata
                        .value(keys::MODIFIED)
                        .and_then(bibli_core::Value::as_time);
                    row.modified_exact = time.map_or_else(String::new, format::time);
                    row.modified = match (time, &now) {
                        (None, _) => "—".to_owned(),
                        (Some(time), Some(now)) => format::friendly_time(time, now),
                        (Some(_), None) => row.modified_exact.clone(),
                    };
                }
                if kind {
                    row.kind = if e.is_place() {
                        place_kind(e, &self.tag_fields).into()
                    } else if e.is_dir() {
                        "Folder".into()
                    } else if e.is_broken_link() {
                        crate::typenames::BROKEN_LINK.into()
                    } else {
                        mime.as_ref()
                            .and_then(|mime| self.type_names.get(mime))
                            .unwrap_or_else(|| crate::typenames::fallback_of(e).into())
                    };
                }
            }
            out.push(row);
        }
        drop(ws);
        self.type_names.request(&app);
        self.icons.request(&app);
        out
    }
}

/// What a place is, for its Type column: a tag's is a Tag, or a Field
/// when its category holds one tag at a time (`tags.exclusive`).
fn place_kind(e: &bibli_core::Entry, fields: &[String]) -> &'static str {
    let tag = e
        .metadata
        .value(keys::PLACE_TAG)
        .and_then(bibli_core::Value::as_str);
    match tag {
        Some(tag)
            if fields
                .iter()
                .any(|f| bibli_core::metadata::same_tag(f, tag)) =>
        {
            "Field"
        }
        Some(_) => "Tag",
        None => "Place",
    }
}

/// The triangle that opens or closes folder `r` in place, pointing right
/// while it is closed. Pressing it does only that: the row is not selected.
fn disclosure(r: &RowData, color: gpui::Hsla, cx: &mut Context<Shell>) -> Div {
    let slot = div().w(px(DISCLOSURE)).flex_none().flex().items_center();
    if !r.is_dir {
        return slot;
    }
    let id = r.id;
    slot.child(
        div()
            .id(("disclosure", r.ix))
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    let pane = this.pane;
                    this.run(
                        "expand",
                        serde_json::json!({ "ids": [id], "mode": "toggle", "pane": pane }),
                        window,
                        cx,
                    );
                }),
            )
            .child(
                gpui::svg()
                    .path(if r.expanded {
                        "toolbar/chevron.svg"
                    } else {
                        "toolbar/right.svg"
                    })
                    .size(px(DISCLOSURE))
                    .text_color(color),
            ),
    )
}

/// A full-width row for the list view, the tree view (`tree`: indented
/// by depth after a disclosure triangle) and the last column of the
/// columns view.
pub fn row(
    shell: &Shell,
    theme: &Theme,
    r: &RowData,
    compact: bool,
    tree: bool,
    cx: &mut Context<Shell>,
) -> Stateful<Div> {
    let ix = r.ix;
    let pane = shell.pane;
    let muted = if r.selected {
        theme.selection_text
    } else {
        theme.text_muted
    };
    let height = row_height(shell.row_zoom);
    let icon_px = icon_px(shell.row_zoom);
    let icon: AnyElement = match &r.icon {
        Some(p) => icons::element(p, icon_px),
        None => div()
            .size(px(icon_px))
            .rounded_sm()
            .bg(if r.is_dir {
                theme.accent
            } else {
                theme.text_muted
            })
            .flex_none()
            .into_any_element(),
    };
    let icon = match &r.place {
        Some((_, color)) => icons::with_tag(icon, icon_px, *color, theme),
        None => icons::with_link(icon, icon_px, r.link, theme),
    };
    let el = div()
        .id(ix)
        .relative()
        .h(px(height))
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .px_2()
        .border_1()
        .border_color(if r.is_cursor {
            theme.accent
        } else {
            gpui::transparent_black()
        })
        .when(r.selected, |d| {
            d.bg(theme.selection_bg).text_color(theme.selection_text)
        })
        .when(!r.selected, |d| d.hover(|s| s.bg(theme.hover)))
        .when(r.cut, |d| d.opacity(CUT_OPACITY))
        .children(shell.menu_marker(pane, r.is_cursor));
    // In the tree the icon is part of the name, indented with it: the
    // detail columns stay aligned at every depth and give way together.
    let (el, icon) = if tree {
        (el, Some(icon))
    } else {
        (el.child(icon), None)
    };
    let el = crate::list::entry_mouse(el, pane, ix, cx);
    let name = div()
        .flex()
        .items_center()
        .gap_1()
        .when_some(icon, |d, icon| {
            d.pl(px(r.depth as f32 * INDENT))
                .child(disclosure(r, muted, cx))
                .child(div().flex_none().child(icon))
        })
        .child(
            // A name keeps its end: the extension tells files apart.
            crate::elements::clip::clipped(
                div()
                    .id(("name", ix))
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis_middle(),
                r.name.clone(),
                shell.font.clone(),
            ),
        )
        // The tags take only what the name leaves (no basis of their own):
        // the chips that do not fit drop whole, last first, "+N" saying how
        // many, and the name never gives way to them.
        .when(!r.tags.is_empty(), |d| {
            let text = if r.selected {
                theme.selection_text
            } else {
                theme.text
            };
            let muted = if r.selected {
                theme.selection_text
            } else {
                theme.text_muted
            };
            d.child(crate::elements::chips::ChipLine {
                chips: r
                    .tags
                    .iter()
                    .map(|(tag, color)| {
                        crate::tags::chip(tag, *color, theme)
                            .text_color(text)
                            .into_any_element()
                    })
                    .collect(),
                more: Box::new(move |n| crate::tags::more_chip(n, muted).into_any_element()),
                height: crate::tags::CHIP_HEIGHT,
                gap: 4.,
            })
        });
    let mut line = line(name, if compact { 0. } else { NAME_MIN }, height);
    if !compact {
        let font = shell.font.clone();
        let exact: gpui::SharedString = r.modified_exact.clone().into();
        let [size, modified, kind, rating, folder] = shell.shown_columns();
        let place = r.folder.clone();
        line = line
            // First of the details: the last to give way.
            .when(folder, |d| {
                let (name, whole) = place.unwrap_or_default();
                d.child(
                    column(shell.column_widths[4], muted, height)
                        .id(("folder", ix))
                        .truncate()
                        .when(!whole.is_empty(), |d| {
                            let font = shell.font.clone();
                            let whole: gpui::SharedString = whole.into();
                            d.tooltip(move |_, cx| {
                                crate::toolbar::tooltip(whole.clone(), font.clone(), cx)
                            })
                        })
                        .child(name),
                )
            })
            .when(size, |d| {
                d.child(
                    column(shell.column_widths[0], muted, height)
                        .text_right()
                        .child(r.size.clone()),
                )
            })
            .when(modified, |d| {
                d.child(
                    column(shell.column_widths[1], muted, height)
                        .id(("modified", ix))
                        .truncate()
                        .when(!exact.is_empty(), |d| {
                            d.tooltip(move |_, cx| {
                                crate::toolbar::tooltip(exact.clone(), font.clone(), cx)
                            })
                        })
                        .child(r.modified.clone()),
                )
            })
            .when(kind, |d| {
                d.child(
                    column(shell.column_widths[2], muted, height)
                        .truncate()
                        .child(gpui::SharedString::from(r.kind.clone())),
                )
            })
            .when(rating, |d| {
                d.child(
                    column(shell.column_widths[3], muted, height)
                        .flex()
                        .items_center()
                        .children(r.rating.map(|stars| crate::tags::stars(stars, 12., theme))),
                )
            });
    }
    let mut el = el.child(line);
    el = dnd::draggable(el, pane, cx);
    el = dnd::takes_gifts(el, pane, ix, cx);
    if let Some(p) = &r.path {
        el = dnd::droppable_dir(el, p.clone(), cx);
    } else if let Some((place, _)) = &r.place {
        el = dnd::droppable_place(el, place.clone(), cx);
    }
    el
}

/// The name and the detail columns of a row, or of the header. A wrapping
/// flex line one row tall: a column that no longer fits wraps below and is
/// clipped, so columns drop whole, last first (Type, then Modified, then
/// Size), and the name, the only child that grows, takes what they leave: it
/// never sits at `NAME_MIN` beside the blank of a column that went. Pure
/// layout: nothing is measured, the header and every row agree by
/// construction, and a pane of any width is laid out correctly in the frame
/// it gets that width.
pub(crate) fn line(name: Div, name_min: f32, height: f32) -> Div {
    div()
        .h(px(height - 2.))
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .flex()
        .flex_row()
        .flex_wrap()
        .content_start()
        .gap_x(px(DETAIL_GAP))
        .child(
            name.h(px(height - 2.))
                .flex()
                .items_center()
                .flex_1()
                .min_w(px(name_min)),
        )
}

/// One detail column of `line`.
pub(crate) fn column(width: f32, color: gpui::Hsla, height: f32) -> Div {
    div()
        .w(px(width))
        .flex_none()
        .line_height(px(height - 2.))
        .text_color(color)
}

fn header_cell(
    id: &'static str,
    label: &'static str,
    key: SortKey,
    shell: &Shell,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> Stateful<Div> {
    let spec = shell.sort_spec();
    let indicator = if spec.key == key {
        if spec.ascending { " ▲" } else { " ▼" }
    } else {
        ""
    };
    div()
        .id(id)
        .cursor_pointer()
        .hover(|s| s.text_color(theme.text))
        .on_click(cx.listener(move |this, _, _, cx| this.sort_by(key, cx)))
        .child(format!("{label}{indicator}"))
}

fn header(shell: &Shell, theme: &Theme, tree: bool, cx: &mut Context<Shell>) -> Div {
    let icon = icon_px(shell.row_zoom);
    let columns = shell.shown_columns();
    div()
        .h(px(24.))
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .px_2()
        .text_xs()
        .text_color(theme.text_muted)
        .border_b_1()
        .border_color(theme.border)
        // Right-click the header to choose columns, where they are.
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                this.open_column_menu(ev.position, window, cx);
            }),
        )
        .when(!tree, |d| d.child(div().w(px(icon)).flex_none()))
        .child(
            line(
                // In the tree, Name stands over the names, past the
                // triangle and the icon a row begins with.
                div()
                    .when(tree, |d| d.pl(px(DISCLOSURE + icon + 8.)))
                    .child(header_cell(
                        "h-name",
                        "Name",
                        SortKey::Name,
                        shell,
                        theme,
                        cx,
                    )),
                NAME_MIN,
                ROW_HEIGHT,
            )
            .when(columns[4], |d| {
                d.child(
                    column(shell.column_widths[4], theme.text_muted, ROW_HEIGHT)
                        .relative()
                        .child("Folder")
                        .child(resize_edge(4, cx)),
                )
            })
            .when(columns[0], |d| {
                d.child(
                    column(shell.column_widths[0], theme.text_muted, ROW_HEIGHT)
                        .relative()
                        .flex()
                        .justify_end()
                        .child(header_cell(
                            "h-size",
                            "Size",
                            SortKey::Size,
                            shell,
                            theme,
                            cx,
                        ))
                        .child(resize_edge(0, cx)),
                )
            })
            .when(columns[1], |d| {
                d.child(
                    column(shell.column_widths[1], theme.text_muted, ROW_HEIGHT)
                        .relative()
                        .child(header_cell(
                            "h-mod",
                            "Modified",
                            SortKey::Modified,
                            shell,
                            theme,
                            cx,
                        ))
                        .child(resize_edge(1, cx)),
                )
            })
            .when(columns[2], |d| {
                d.child(
                    column(shell.column_widths[2], theme.text_muted, ROW_HEIGHT)
                        .relative()
                        .child("Type")
                        .child(resize_edge(2, cx)),
                )
            })
            .when(columns[3], |d| {
                d.child(
                    column(shell.column_widths[3], theme.text_muted, ROW_HEIGHT)
                        .relative()
                        .child(header_cell(
                            "h-rating",
                            "Rating",
                            SortKey::Rating,
                            shell,
                            theme,
                            cx,
                        ))
                        .child(resize_edge(3, cx)),
                )
            }),
        )
}

impl PaneView for ListView {
    fn id(&self) -> &'static str {
        if self.tree {
            crate::view::TREE
        } else {
            crate::view::LIST
        }
    }

    fn opens_in_place(&self) -> bool {
        self.tree
    }

    fn render(&mut self, shell: &mut Shell, _: &mut Window, cx: &mut Context<Shell>) -> AnyElement {
        let theme = cx.global::<Theme>().clone();
        let count = shell.ui().list.sorted.len();
        let pane = shell.pane;
        let tree = self.tree;
        let height = row_height(shell.row_zoom);
        let icon_px = u16::try_from(shell.row_zoom).unwrap_or(u16::MAX);
        if (height - self.height).abs() > f32::EPSILON {
            // Zoomed: the row at the top stays at the top, as far into it.
            let before = self.height;
            let (row, within) = row_anchor(
                f32::from(self.scroll.0.borrow().base_handle.offset().y),
                before,
            );
            self.height = height;
            self.restore_anchor((row, within * height / before));
        }
        let list = uniform_list(
            "rows",
            count,
            cx.processor(move |this: &mut Shell, range: Range<usize>, _window, cx| {
                this.with_pane(pane, |this| {
                    let theme = cx.global::<Theme>().clone();
                    let list = &mut this.ui_mut().list;
                    list.visible_rows = range.len().max(1);
                    list.visible_items = range.len();
                    let rows = this.rows(range, icon_px, false);
                    let need: Vec<EntryId> =
                        rows.iter().filter(|r| !r.hydrated).map(|r| r.id).collect();
                    this.request_hydrate(need);
                    rows.iter()
                        .map(|r| row(this, &theme, r, false, tree, cx))
                        .collect::<Vec<_>>()
                })
            }),
        )
        .track_scroll(self.scroll.clone())
        .size_full();
        let marquee = crate::elements::marquee::Marquee {
            pane,
            shell: cx.weak_entity(),
            source: crate::elements::marquee::Source::Rows { height },
            count,
            scroll: self.scroll.0.borrow().base_handle.clone(),
            drag: shell.marquee.clone(),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(header(shell, &theme, tree, cx))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(list)
                    .child(marquee)
                    .child(crate::views::zoom_wheel(cx))
                    .child(crate::elements::scrollbar::vertical(
                        ("list-scrollbar", pane.0 as usize),
                        crate::elements::scrollbar::Scrolls::Offset(
                            self.scroll.0.borrow().base_handle.clone(),
                        ),
                    )),
            )
            .into_any_element()
    }

    fn scroll_to(&mut self, ix: usize) {
        self.scroll.scroll_to_item(ix, ScrollStrategy::Top);
    }

    fn scroll_anchor(&self) -> Option<(usize, f32)> {
        Some(row_anchor(
            f32::from(self.scroll.0.borrow().base_handle.offset().y),
            self.height,
        ))
    }

    fn restore_anchor(&mut self, anchor: (usize, f32)) {
        let mut state = self.scroll.0.borrow_mut();
        // A pending scroll (the reset of a new listing) would move it again.
        state.deferred_scroll_to_item = None;
        let offset = state.base_handle.offset();
        state
            .base_handle
            .set_offset(gpui::point(offset.x, px(row_offset(anchor, self.height))));
    }

    fn nav_delta(&self, nav: Nav, shell: &Shell) -> Option<isize> {
        let rows = shell.ui().list.visible_rows;
        let page = rows.saturating_sub(1).max(1) as isize;
        let half = (rows / 2).max(1) as isize;
        match nav {
            Nav::Up => Some(-1),
            Nav::Down => Some(1),
            Nav::PageUp => Some(-page),
            Nav::PageDown => Some(page),
            Nav::HalfPageUp => Some(-half),
            Nav::HalfPageDown => Some(half),
            Nav::Left | Nav::Right | Nav::Home | Nav::End => None,
        }
    }

    /// In the tree, Right opens the folder under the cursor, or goes into
    /// it when it is open; Left closes it, or goes to the folder it is in.
    fn nav(
        &mut self,
        nav: Nav,
        shell: &mut Shell,
        window: &mut Window,
        cx: &mut Context<Shell>,
    ) -> bool {
        if !self.tree || !matches!(nav, Nav::Left | Nav::Right) {
            return false;
        }
        let Some(ix) = shell.cursor_index() else {
            return false;
        };
        let sorted = shell.ui().list.sorted.clone();
        let id = sorted[ix];
        let (dir, open, parent) = {
            let ws = shell.app.workspace.read();
            let Some(p) = ws.pane(shell.pane) else {
                return false;
            };
            let dir = p.entry(id).is_some_and(bibli_core::Entry::is_dir);
            let open = dir && p.is_open_at(id);
            let depth = p.depth(id);
            let parent = (depth > 0)
                .then(|| (0..ix).rev().find(|&j| p.depth(sorted[j]) < depth))
                .flatten();
            (dir, open, parent)
        };
        let expand =
            |mode: &str, shell: &mut Shell, window: &mut Window, cx: &mut Context<Shell>| {
                let pane = shell.pane;
                shell.run(
                    "expand",
                    serde_json::json!({ "ids": [id], "mode": mode, "pane": pane }),
                    window,
                    cx,
                );
            };
        match nav {
            Nav::Right if open => shell.cursor_move(1, Step::Select, window, cx),
            Nav::Right if dir => expand("expand", shell, window, cx),
            Nav::Left if open => expand("collapse", shell, window, cx),
            Nav::Left => match parent {
                Some(parent) => shell.cursor_to(parent as isize, Step::Select, window, cx),
                None => return false,
            },
            _ => return false,
        }
        true
    }

    fn reset(&mut self) {
        self.scroll.scroll_to_item_strict(0, ScrollStrategy::Top);
    }
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_ROW_ZOOM, ROW_HEIGHT, ROW_ZOOMS, row_anchor, row_height, row_offset};

    #[test]
    fn a_scroll_position_comes_back_to_the_pixel() {
        for h in ROW_ZOOMS.map(row_height) {
            for offset in [0., -1., -13., -h, -(40. * h + 11.5)] {
                let anchor = row_anchor(offset, h);
                assert!((row_offset(anchor, h) - offset).abs() < 1e-3, "{offset}");
            }
        }
        let h = ROW_HEIGHT;
        // Half a row down: the row at the top and half of it scrolled.
        let (row, within) = row_anchor(-(3.5 * h), h);
        assert_eq!(row, 3);
        assert!((within - h / 2.).abs() < 1e-3);
        // An entry that moved keeps how far into its row the view began.
        assert!((row_offset((5, 7.), h) - -(5. * h + 7.)).abs() < 1e-3);
        // What no row can hold is clamped into the row.
        assert!((row_offset((2, 99.), h) - -(3. * h - 1.)).abs() < 1e-3);
    }

    #[test]
    fn the_default_size_is_the_row_the_list_always_had() {
        assert!((row_height(DEFAULT_ROW_ZOOM) - ROW_HEIGHT).abs() < f32::EPSILON);
        assert!(ROW_ZOOMS.contains(&DEFAULT_ROW_ZOOM));
    }
}
