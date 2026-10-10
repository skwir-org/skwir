//! The list view: one row per entry with icon, name, size, modified;
//! clickable, sortable headers. The tree view is the same list whose
//! folders open in place (`Pane::expanded`, the core `expand` command):
//! their entries follow them, indented, and are the pane's like any other,
//! so selection, commands and drags need nothing of their own.

use std::ops::Range;

use gpui::{
    AnyElement, Context, Div, MouseButton, MouseDownEvent, ScrollStrategy, Stateful,
    UniformListScrollHandle, Window, div, prelude::*, px, uniform_list,
};
use skwir_core::SortKey;
use skwir_core::entry::keys;

use crate::format;
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

/// A detail column of the list: everything about it, once. The header,
/// the rows, the header's menu, the View menu's sorts and Settings all
/// read `COLUMNS`.
pub struct Column {
    pub detail: Detail,
    /// Its name where its dragged width is remembered
    /// (`Session::column_widths`).
    pub key: &'static str,
    /// Whether it is shown (`ui.column_*`).
    pub setting: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    /// Shown until the user says otherwise.
    pub shown: bool,
    /// What a click on its header sorts by, if anything.
    pub sort: Option<SortKey>,
    pub width: f32,
    /// A number, set against the right.
    pub right: bool,
}

/// The detail columns, by their place in `COLUMNS`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Detail {
    Folder,
    Deleted,
    Size,
    Modified,
    Type,
    Rating,
}

impl Detail {
    pub fn column(self) -> &'static Column {
        &COLUMNS[self as usize]
    }
}

/// The detail columns in the order they stand after the name. A narrow
/// pane drops them whole from the last (`line`): Folder, first of them
/// where it is shown at all, is the last to give way, then in the trash
/// when each file was put there.
pub const COLUMNS: [Column; 6] = [
    Column {
        detail: Detail::Folder,
        key: "folder",
        setting: "ui.column_folder",
        label: "Folder",
        description: "Show the folder each file is in, where files from many folders are listed: a search, a tag, the shelf, Recent. It is the last column to give way.",
        shown: true,
        sort: None,
        width: 180.,
        right: false,
    },
    Column {
        detail: Detail::Deleted,
        key: "deleted",
        setting: "ui.column_deleted",
        label: "Deleted",
        description: "Show when each file was put in the trash, where the trash's files are listed.",
        shown: true,
        sort: Some(SortKey::Deleted),
        width: 136.,
        right: false,
    },
    Column {
        detail: Detail::Size,
        key: "size",
        setting: "ui.column_size",
        label: "Size",
        description: "Show the size of files.",
        shown: true,
        sort: Some(SortKey::Size),
        width: 80.,
        right: true,
    },
    Column {
        detail: Detail::Modified,
        key: "modified",
        setting: "ui.column_modified",
        label: "Modified",
        description: "Show when each item last changed.",
        shown: true,
        sort: Some(SortKey::Modified),
        width: 136.,
        right: false,
    },
    Column {
        detail: Detail::Type,
        key: "type",
        setting: "ui.column_type",
        label: "Type",
        description: "Show the kind of file, such as PDF document.",
        shown: true,
        sort: None,
        width: 150.,
        right: false,
    },
    Column {
        detail: Detail::Rating,
        key: "rating",
        setting: "ui.column_rating",
        label: "Rating",
        description: "Show the stars each file was given, where its notes are kept. It is the first column to give way in a narrow pane.",
        shown: false,
        sort: Some(SortKey::Rating),
        width: 76.,
        right: false,
    },
];

/// One value per detail column, read by `Detail`.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct PerColumn<T>(pub [T; 6]);

impl<T> std::ops::Index<Detail> for PerColumn<T> {
    type Output = T;
    fn index(&self, detail: Detail) -> &T {
        &self.0[detail as usize]
    }
}

impl<T> std::ops::IndexMut<Detail> for PerColumn<T> {
    fn index_mut(&mut self, detail: Detail) -> &mut T {
        &mut self.0[detail as usize]
    }
}

/// The widths the columns ship with.
pub fn default_widths() -> PerColumn<f32> {
    PerColumn(COLUMNS.each_ref().map(|column| column.width))
}

const DETAIL_GAP: f32 = 12.0;

pub fn clamp_width(width: f32) -> f32 {
    width.clamp(56.0, 360.0)
}

/// Payload of a header edge drag.
#[derive(Clone)]
pub struct ColumnResize(pub Detail);

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
        let detail = ev.drag(cx).0;
        let (start_x, start_width) = *self
            .edge_start
            .get_or_insert((x, self.column_widths[detail]));
        self.column_widths[detail] = clamp_width(start_width + (start_x - x));
        cx.notify();
    }

    /// `ui.columns`: show or hide one detail column, from the header's menu.
    /// It goes through `settings.apply` like any other preference, so the
    /// dialog and the header always agree and the choice outlives the window.
    pub fn toggle_column(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(column) = COLUMNS.iter().find(|column| column.setting == key) else {
            return;
        };
        let shown = !self.columns[column.detail];
        self.columns[column.detail] = shown;
        cx.notify();
        if let Err(error) = self
            .app
            .dispatch(&skwir_core::command::Invocation::with_args(
                "settings.apply",
                &serde_json::json!({ "changes": { key: shown.to_string() } }),
            ))
        {
            self.set_status(format!("{error:#}"), true, cx);
        }
    }

    /// Whether `detail` can be shown here: Rating only while ratings are
    /// kept, Folder only where files from many folders are listed (a
    /// folder's are in it, as a trashed folder's are), Deleted where the
    /// trash's files are (`Verbs::restore`), not those of a trashed folder,
    /// put there with it.
    pub fn column_applies(&self, detail: Detail) -> bool {
        let place = &self.ui().place;
        let one_folder = place.trail.as_ref().is_some_and(|trail| trail.one_folder);
        match detail {
            Detail::Rating => self.rated(),
            Detail::Folder => !place.folder && !one_folder,
            Detail::Deleted => place.verbs.restore.is_some() && !one_folder,
            Detail::Size | Detail::Modified | Detail::Type => true,
        }
    }

    /// Whether the header's menu offers `detail`: where it applies, and
    /// Folder everywhere, for where files from many folders are listed.
    pub fn column_offered(&self, detail: Detail) -> bool {
        detail == Detail::Folder || self.column_applies(detail)
    }

    /// Whether a store keeps ratings (`NoteStores::held`): no column,
    /// sort or star while none does.
    pub fn rated(&self) -> bool {
        self.app.notes.held().rating
    }

    /// The detail columns drawn: those chosen that apply here.
    pub fn shown_columns(&self) -> PerColumn<bool> {
        PerColumn(
            COLUMNS
                .each_ref()
                .map(|column| self.columns[column.detail] && self.column_applies(column.detail)),
        )
    }

    /// `ui.columns_reset`: back to the widths the view ships with.
    pub fn reset_column_widths(&mut self, cx: &mut Context<Self>) {
        self.column_widths = default_widths();
        self.remember_session();
        cx.notify();
    }
}

/// The draggable left edge of a header cell, in the gap before it (inside
/// the line, which clips what leaves it), so it never covers the label. A
/// double click gives the column back its own width.
fn resize_edge(detail: Detail, cx: &mut Context<Shell>) -> Stateful<Div> {
    crate::elements::edge::edge(
        ("column-edge", detail as usize),
        crate::elements::edge::Side::Left,
        ColumnResize(detail),
        move |shell| shell.column_widths[detail] = detail.column().width,
        cx,
    )
    .left(px(-(DETAIL_GAP + crate::elements::edge::THICKNESS) / 2.))
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

/// Everything a row needs, collected under the workspace lock: what every
/// view draws of the entry (`Look`), and what the list shows besides.
pub struct RowData {
    pub ix: usize,
    pub look: crate::views::entry::Look,
    pub hydrated: bool,
    pub size: String,
    pub modified: String,
    /// Exact timestamp, shown as the date's tooltip.
    pub modified_exact: String,
    /// When it was put in the trash, and exactly.
    pub deleted: String,
    pub deleted_exact: String,
    pub kind: std::sync::Arc<str>,
    /// Half stars, from 1 to 10.
    pub rating: Option<u64>,
    /// How many opened folders deep it is (`Pane::depth`), and for a
    /// folder whether it is open in place.
    pub depth: usize,
    pub expanded: bool,
    /// The folder it is in, and its whole path, where files from many
    /// folders are listed.
    pub folder: Option<(String, String)>,
}

impl Shell {
    /// Collect row data for `range` of the sorted order. `compact` rows
    /// (the Columns view) show a name only: their details are not computed.
    pub fn rows(&mut self, range: Range<usize>, icon_px: u16, compact: bool) -> Vec<RowData> {
        let mut out = Vec::with_capacity(range.len());
        let friendly = self.prefs.friendly_dates;
        let columns = self.shown_columns();
        let now = (!compact && friendly && (columns[Detail::Modified] || columns[Detail::Deleted]))
            .then(jiff::Zoned::now);
        // A date as the column says it, and exactly for its tooltip.
        let date = |time: Option<std::time::SystemTime>| -> (String, String) {
            let exact = time.map_or_else(String::new, format::time);
            let shown = match (time, &now) {
                (None, _) => "—".to_owned(),
                (Some(time), Some(now)) => format::friendly_time(time, now),
                (Some(_), None) => exact.clone(),
            };
            (shown, exact)
        };
        let app = std::sync::Arc::clone(&self.app);
        let ws = app.workspace.read();
        let Some(pane) = ws.pane(self.pane) else {
            return out;
        };
        for ix in range {
            let Some(&id) = self.ui().list.sorted.get(ix) else {
                break;
            };
            let (Some(e), Some(look)) = (pane.entry(id), self.look(pane, id, icon_px)) else {
                continue;
            };
            let mut row = RowData {
                ix,
                look,
                expanded: e.is_dir() && pane.is_open_at(id),
                depth: pane.depth(id),
                size: String::new(),
                modified: String::new(),
                modified_exact: String::new(),
                deleted: String::new(),
                deleted_exact: String::new(),
                kind: std::sync::Arc::from(""),
                hydrated: e.metadata.is_hydrated(),
                rating: None,
                folder: None,
            };
            if !compact {
                if columns[Detail::Folder] && !e.is_place() {
                    // A file of the trash is from the folder it was put
                    // there from, which restoring it puts it back in.
                    let from = e
                        .metadata
                        .value(keys::TRASHED_FROM)
                        .and_then(skwir_core::Value::as_str)
                        .map(std::path::PathBuf::from);
                    row.folder = from
                        .or_else(|| pane.path_of(id)?.parent().map(ToOwned::to_owned))
                        .map(|dir| {
                            let name = dir.file_name().map_or_else(
                                || dir.display().to_string(),
                                |n| n.to_string_lossy().into_owned(),
                            );
                            (name, dir.display().to_string())
                        });
                }
                if columns[Detail::Rating] {
                    row.rating = e
                        .metadata
                        .value(keys::RATING)
                        .and_then(skwir_core::Value::as_u64);
                }
                if columns[Detail::Size] {
                    row.size = match e
                        .metadata
                        .value(keys::SIZE)
                        .and_then(skwir_core::Value::as_u64)
                    {
                        Some(s) if !e.is_dir() => format::size(s),
                        // A place says how many files it holds.
                        _ => match e
                            .metadata
                            .value(keys::FILES)
                            .and_then(skwir_core::Value::as_u64)
                        {
                            Some(n) => {
                                format::count(usize::try_from(n).unwrap_or(usize::MAX), "file")
                            }
                            None => "—".to_owned(),
                        },
                    };
                }
                if columns[Detail::Modified] {
                    (row.modified, row.modified_exact) = date(
                        e.metadata
                            .value(keys::MODIFIED)
                            .and_then(skwir_core::Value::as_time),
                    );
                }
                if columns[Detail::Deleted] {
                    (row.deleted, row.deleted_exact) = date(
                        e.metadata
                            .value(keys::TRASHED)
                            .and_then(skwir_core::Value::as_time),
                    );
                }
                if columns[Detail::Type] {
                    row.kind = if e.is_place() {
                        e.metadata
                            .value(keys::PLACE_KIND)
                            .and_then(skwir_core::Value::as_str)
                            .unwrap_or("Place")
                            .into()
                    } else if e.is_dir() {
                        "Folder".into()
                    } else if e.is_broken_link() {
                        crate::typenames::BROKEN_LINK.into()
                    } else {
                        // Typed only where the column shows it.
                        skwir_platform::mime::of_entry(e)
                            .as_ref()
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

/// The triangle that opens or closes folder `r` in place, pointing right
/// while it is closed. Pressing it does only that: the row is not selected.
fn disclosure(r: &RowData, color: gpui::Hsla, cx: &mut Context<Shell>) -> Div {
    let slot = div().w(px(DISCLOSURE)).flex_none().flex().items_center();
    if !r.look.is_dir {
        return slot;
    }
    let id = r.look.id;
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
/// An element for each row of `range`: an entry gone between the frame's
/// reading of the order and its own (an agent's command meanwhile) is a
/// blank of a row's height, so the rows after it stay in their places
/// rather than each taking the one before's.
pub(crate) fn row_elements(
    shell: &Shell,
    theme: &Theme,
    range: Range<usize>,
    rows: &[RowData],
    (compact, tree): (bool, bool),
    cx: &mut Context<Shell>,
) -> Vec<Stateful<Div>> {
    let height = row_height(shell.row_zoom);
    let mut rows = rows.iter().peekable();
    range
        .map(|ix| match rows.next_if(|r| r.ix == ix) {
            Some(r) => row(shell, theme, r, compact, tree, cx),
            None => div().id(("row-gone", ix)).h(px(height)),
        })
        .collect()
}

pub fn row(
    shell: &Shell,
    theme: &Theme,
    r: &RowData,
    compact: bool,
    tree: bool,
    cx: &mut Context<Shell>,
) -> Stateful<Div> {
    let ix = r.ix;
    let look = &r.look;
    let muted = if look.selected {
        theme.selection_text
    } else {
        theme.text_muted
    };
    let height = row_height(shell.row_zoom);
    let icon = look.picture(icon_px(shell.row_zoom), theme);
    let el = crate::views::entry::highlight(
        div()
            .id(ix)
            .relative()
            .h(px(height))
            .w_full()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .px_2(),
        look,
        theme,
    );
    // In the tree the icon is part of the name, indented with it: the
    // detail columns stay aligned at every depth and give way together.
    let (el, icon) = if tree {
        (el, Some(icon))
    } else {
        (el.child(icon), None)
    };
    let name = div()
        .flex()
        .items_center()
        .gap_1()
        .when_some(icon, |d, icon| {
            d.pl(px(r.depth as f32 * INDENT))
                .child(disclosure(r, muted, cx))
                .child(div().flex_none().child(icon))
        })
        // A name keeps its end: the extension tells files apart.
        .child(crate::views::entry::name(
            div().id(("name", ix)),
            look,
            shell,
        ))
        // The tags take only what the name leaves (no basis of their own):
        // the chips that do not fit drop whole, last first, "+N" saying how
        // many, and the name never gives way to them.
        .when(!look.tags.is_empty(), |d| {
            d.child(crate::views::entry::tags(look, theme))
        });
    let mut line = line(name, if compact { 0. } else { NAME_MIN }, height);
    if !compact {
        let shown = shell.shown_columns();
        for column in COLUMNS.iter().filter(|column| shown[column.detail]) {
            let cell = cell(shell, column.detail, muted, height);
            line = line.child(detail(shell, r, column.detail, cell, theme));
        }
    }
    crate::views::entry::behave(el.child(line), look, shell, cx)
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

/// A detail column's cell, of a row or the header: its width, and set
/// against the right when it holds a number.
fn cell(shell: &Shell, detail: Detail, color: gpui::Hsla, height: f32) -> Div {
    column(shell.column_widths[detail], color, height)
        .when(detail.column().right, gpui::Styled::text_right)
}

/// What row `r` shows in its `detail` column. A text cut to fit says itself
/// whole when pointed at, before what the column adds (a folder's path, the
/// exact time).
fn detail(shell: &Shell, r: &RowData, detail: Detail, cell: Div, theme: &Theme) -> AnyElement {
    let (name, text, also) = match detail {
        Detail::Rating => {
            return cell
                .flex()
                .items_center()
                .children(r.rating.map(|stars| crate::tags::stars(stars, 12., theme)))
                .into_any_element();
        }
        Detail::Folder => {
            let (name, whole) = r.folder.clone().unwrap_or_default();
            ("folder", name, whole)
        }
        Detail::Size => ("size", r.size.clone(), String::new()),
        Detail::Modified => ("modified", r.modified.clone(), r.modified_exact.clone()),
        Detail::Deleted => ("deleted", r.deleted.clone(), r.deleted_exact.clone()),
        Detail::Type => ("type", r.kind.to_string(), String::new()),
    };
    crate::elements::clip::clipped_with(
        cell.id((name, r.ix)).min_w_0().truncate(),
        text,
        (!also.is_empty()).then(|| also.into()),
        shell.font.clone(),
    )
    .into_any_element()
}

/// A header's label, sorting by `key` when clicked, with the order's
/// arrow while it is the key.
fn header_cell(
    id: impl Into<gpui::ElementId>,
    label: &'static str,
    key: Option<SortKey>,
    shell: &Shell,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> Stateful<Div> {
    let spec = shell.sort_spec();
    let indicator = match key {
        Some(key) if spec.key == key && spec.ascending => " ▲",
        Some(key) if spec.key == key => " ▼",
        _ => "",
    };
    let el = div().id(id).child(format!("{label}{indicator}"));
    match key {
        Some(key) => el
            .cursor_pointer()
            .hover(|s| s.text_color(theme.text))
            .on_click(cx.listener(move |this, _, _, cx| this.sort_by(key, cx))),
        None => el,
    }
}

fn header(shell: &Shell, theme: &Theme, tree: bool, cx: &mut Context<Shell>) -> Div {
    let icon = icon_px(shell.row_zoom);
    let shown = shell.shown_columns();
    let mut line = line(
        // In the tree, Name stands over the names, past the triangle and
        // the icon a row begins with.
        div()
            .when(tree, |d| d.pl(px(DISCLOSURE + icon + 8.)))
            .child(header_cell(
                "h-name",
                "Name",
                Some(SortKey::Name),
                shell,
                theme,
                cx,
            )),
        NAME_MIN,
        ROW_HEIGHT,
    );
    for column in COLUMNS.iter().filter(|column| shown[column.detail]) {
        line = line.child(
            cell(shell, column.detail, theme.text_muted, ROW_HEIGHT)
                .relative()
                .child(header_cell(
                    ("h", column.detail as usize),
                    column.label,
                    column.sort,
                    shell,
                    theme,
                    cx,
                ))
                .child(resize_edge(column.detail, cx)),
        );
    }
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
        .child(line)
}

/// The keys of a view of rows (the list, the tree, the columns' last):
/// up and down a row, a page of the rows drawn, half of one.
pub(crate) fn rows_nav(nav: Nav, shell: &Shell) -> Option<isize> {
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

impl PaneView for ListView {
    fn zooms(&self) -> crate::view::Zooms {
        crate::view::Zooms::Rows
    }

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
                    let rows = this.rows(range.clone(), icon_px, false);
                    this.request_hydrate(rows.iter().map(|r| r.look.id).collect::<Vec<_>>());
                    row_elements(this, &theme, range, &rows, (false, tree), cx)
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
        rows_nav(nav, shell)
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
            let dir = p.entry(id).is_some_and(skwir_core::Entry::is_dir);
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
