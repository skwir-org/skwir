//! The list view: one row per entry with icon, name, size, modified;
//! clickable, sortable headers. The tree view is the same list whose
//! folders open in place (`Pane::expanded`, the core `expand` command):
//! their entries follow them, indented, and are the pane's like any other,
//! so selection, commands and drags need nothing of their own.

use crate::sizes::TextSizes;
use std::ops::Range;

use gpui::{
    AnyElement, Context, Div, MouseButton, MouseDownEvent, ScrollStrategy, Stateful,
    UniformListScrollHandle, Window, div, prelude::*, px, uniform_list,
};
use skwir_core::SortKey;
use skwir_core::entry::keys;

use crate::elements::zoom_wheel::{self, Zoomed};
use crate::format;
use crate::list::Step;
use crate::shell::Shell;
use crate::sizes::{Part, Scale};
use crate::theme::Theme;
use crate::view::{Nav, PaneView};
use crate::views::groups::{self, Row, RowMap};

/// Height of a row at the default size and 100 %: the archive preview's
/// rows too.
pub const ROW_HEIGHT: f32 = 26.0;
/// The icon sizes the list and the columns zoom through (`ui.zoom`); a
/// row is its icon and a margin, so Ctrl+wheel makes rows taller.
pub const ROW_ZOOMS: [u32; 5] = [16, 18, 24, 32, 48];
pub const DEFAULT_ROW_ZOOM: u32 = 18;

/// Height of a row whose icon is `icon` px, its text at `scale`: the
/// text's row, or taller where the icon needs it (ADR-117). The icon
/// keeps the zoom's size whatever the text's.
pub fn row_height(icon: u32, scale: Scale) -> f32 {
    (icon_px(icon) + ROW_HEIGHT - icon_px(DEFAULT_ROW_ZOOM)).max(scale.f(ROW_HEIGHT))
}

/// The scale the file views draw at: the files' text size (ADR-117).
pub fn files_scale(theme: &Theme) -> Scale {
    theme.sizes.scale(Part::Files)
}

/// A row zoom as the icon's side in pixels.
pub fn icon_px(zoom: u32) -> f32 {
    f32::from(u16::try_from(zoom).unwrap_or(u16::MAX))
}
/// The name never gets narrower than this, at 100 %; detail columns give
/// way first.
const NAME_MIN: f32 = 150.0;
/// The tree's disclosure triangle, and how far each level is indented: a
/// child's triangle stands under its parent's icon.
const DISCLOSURE: f32 = 12.0;
const INDENT: f32 = DISCLOSURE + 4.0;
/// How much of a cut entry remains visible until it is pasted.
pub const CUT_OPACITY: f32 = 0.45;
/// A folder shown only as the way to what a filter keeps: readable, as it
/// is still there to be gone into, yet behind what the filter keeps.
pub const ONLY_LEADS_OPACITY: f32 = 0.6;

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
    /// Its width at 100 % (ADR-117): drawn at the files' scale, as a
    /// width dragged is remembered.
    pub width: f32,
    /// A number, set against the right.
    pub right: bool,
}

/// The detail columns, by their place in `COLUMNS`: every criterion the
/// pane sorts by but its name and extension (what shows, sorts; what
/// sorts, shows), and the tags, which a row shows by its name.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Detail {
    Folder,
    Deleted,
    Size,
    Dimensions,
    Duration,
    Modified,
    Created,
    Accessed,
    Taken,
    Type,
    Kind,
    Owner,
    /// What its repository says of it (ADR-134).
    Vcs,
    Rating,
}

/// How many detail columns there are.
pub const DETAILS: usize = 14;

impl Detail {
    pub fn column(self) -> &'static Column {
        &COLUMNS[self as usize]
    }
}

/// The detail columns in the order they stand after the name. A narrow
/// pane drops them whole from the last (`line`): Folder, first of them
/// where it is shown at all, is the last to give way, then in the trash
/// when each file was put there.
pub const COLUMNS: [Column; DETAILS] = [
    Column {
        detail: Detail::Folder,
        key: "folder",
        setting: "ui.column_folder",
        label: "Folder",
        description: "Show the folder each file is in, where files from many folders are listed: a search, a tag, the shelf, Recent. It is the last column to give way.",
        shown: true,
        sort: Some(SortKey::Folder),
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
        detail: Detail::Dimensions,
        key: "dimensions",
        setting: "ui.column_dimensions",
        label: "Dimensions",
        description: "Show the size of pictures in pixels, where a plugin reads it of them.",
        shown: false,
        sort: Some(SortKey::Dimensions),
        width: 100.,
        right: true,
    },
    Column {
        detail: Detail::Duration,
        key: "duration",
        setting: "ui.column_duration",
        label: "Duration",
        description: "Show how long sounds and films last, where a plugin reads it of them.",
        shown: false,
        sort: Some(SortKey::Duration),
        width: 72.,
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
        detail: Detail::Created,
        key: "created",
        setting: "ui.column_created",
        label: "Created",
        description: "Show when each item was made, where the system keeps it.",
        shown: false,
        sort: Some(SortKey::Created),
        width: 136.,
        right: false,
    },
    Column {
        detail: Detail::Accessed,
        key: "accessed",
        setting: "ui.column_accessed",
        label: "Accessed",
        description: "Show when each item was last read, as the system keeps it.",
        shown: false,
        sort: Some(SortKey::Accessed),
        width: 136.,
        right: false,
    },
    Column {
        detail: Detail::Taken,
        key: "taken",
        setting: "ui.column_taken",
        label: "Taken",
        description: "Show when pictures and films were taken, by their camera's clock, where a plugin reads it of them.",
        shown: false,
        sort: Some(SortKey::Taken),
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
        sort: Some(SortKey::Type),
        width: 150.,
        right: false,
    },
    Column {
        detail: Detail::Kind,
        key: "kind",
        setting: "ui.column_kind",
        label: "Kind",
        description: "Show what kind of file each is, such as Image or Document.",
        shown: false,
        sort: Some(SortKey::Kind),
        width: 110.,
        right: false,
    },
    Column {
        detail: Detail::Owner,
        key: "owner",
        setting: "ui.column_owner",
        label: "Owner",
        description: "Show who owns each item.",
        shown: false,
        sort: Some(SortKey::Owner),
        width: 100.,
        right: false,
    },
    Column {
        detail: Detail::Vcs,
        key: "vcs",
        setting: "ui.column_vcs",
        label: "Git",
        description: "Show what Git says of each file in a repository: changed, staged, new, in conflict or ignored, and of a folder what it holds.",
        shown: false,
        sort: Some(SortKey::Vcs),
        // A folder's says what it holds: "Holds staged changes".
        width: 160.,
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
pub struct PerColumn<T>(pub [T; DETAILS]);

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
        let scale = files_scale(cx.global::<Theme>());
        let (start_x, start_width) = *self
            .edge_start
            .get_or_insert((self.edge_pressed, self.column_widths[detail]));
        // Widths are kept at 100 %, whatever the size they are dragged at.
        self.column_widths[detail] = clamp_width(start_width + scale.at_100(start_x - x));
        cx.notify();
    }

    /// `ui.columns`: show or hide one detail column, from the header's menu.
    /// It is saved as any other preference is (`settings::remember`: one
    /// save at a time, the next waiting), so the dialog and the header
    /// always agree and the choice outlives the window.
    pub fn toggle_column(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(column) = COLUMNS.iter().find(|column| column.setting == key) else {
            return;
        };
        let shown = !self.columns[column.detail];
        self.columns[column.detail] = shown;
        cx.notify();
        crate::settings::remember(self, column.setting, shown.to_string(), cx);
    }

    /// What the pane sorts by here, each with what menus call it: every
    /// criterion (ADR-130) that applies here, as its column would (Rating
    /// while ratings are kept, Deleted in the trash, Owner where the system
    /// keeps one by number), those only the sorter reads (a kind, a type's
    /// name, an owner) while there is one.
    /// The View menu offers these, and the palette only their sorts.
    pub fn sort_keys(&self) -> Vec<(&'static str, SortKey)> {
        let sorter = !self.app.registry.sorters.all().is_empty();
        SortKey::ALL
            .into_iter()
            .filter(|key| match key {
                SortKey::Rating => self.column_applies(Detail::Rating),
                SortKey::Vcs => self.column_applies(Detail::Vcs),
                SortKey::Deleted => self.column_applies(Detail::Deleted),
                SortKey::Folder => self.column_applies(Detail::Folder),
                // Where the system keeps an owner by number, as its column.
                SortKey::Owner => sorter && self.column_applies(Detail::Owner),
                // Read on demand (ADR-131): while something reads it.
                key if key.field().is_some() => key
                    .field()
                    .is_some_and(|field| self.app.fields.knows(field)),
                key => sorter || !key.keyed(),
            })
            .map(|key| (key.title(), key))
            .collect()
    }

    /// What the pane can be grouped by here: what it sorts by, while the
    /// sorter, which makes every group, is there; nothing else.
    pub fn group_keys(&self) -> Vec<SortKey> {
        if self.app.registry.sorters.all().is_empty() {
            return Vec::new();
        }
        self.sort_keys().into_iter().map(|(_, key)| key).collect()
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
            // Wherever a version control may say it: a folder's files (a
            // folder in no repository says nothing).
            Detail::Vcs => place.folder && !self.app.vcs.is_empty(),
            Detail::Folder => !place.folder && !one_folder,
            Detail::Deleted => place.verbs.restore.is_some() && !one_folder,
            Detail::Size
            | Detail::Modified
            | Detail::Created
            | Detail::Accessed
            | Detail::Type
            | Detail::Kind => true,
            // Read on demand (ADR-131): while something reads it.
            Detail::Dimensions | Detail::Duration | Detail::Taken => detail
                .column()
                .sort
                .and_then(SortKey::field)
                .is_some_and(|field| self.app.fields.knows(field)),
            // A file says its owner where the system keeps one by number.
            Detail::Owner => cfg!(unix),
        }
    }

    /// What the pane's view shows that is read on demand (ADR-131): what
    /// the list's columns or the grid's captions read of each file, as its
    /// sort reads them (`App::show_fields`).
    pub fn fields_shown(&self) -> Vec<&'static str> {
        let view = self.ui().view_id;
        let shown = if view == crate::view::LIST || view == crate::view::TREE {
            self.shown_columns()
        } else if view == crate::view::GRID {
            crate::views::grid::caption_columns(&self.grid_captions())
        } else {
            return Vec::new();
        };
        COLUMNS
            .iter()
            .filter(|column| shown[column.detail])
            .filter_map(|column| column.sort.and_then(SortKey::field))
            .collect()
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
fn resize_edge(detail: Detail, scale: Scale, cx: &mut Context<Shell>) -> Stateful<Div> {
    crate::elements::edge::edge(
        ("column-edge", detail as usize),
        crate::elements::edge::Side::Left,
        ColumnResize(detail),
        move |shell| shell.column_widths[detail] = detail.column().width,
        cx,
    )
    .left(px(-(scale.f(DETAIL_GAP)
        + crate::elements::edge::THICKNESS)
        / 2.))
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
    /// The rows as last drawn, the groups' headings among the entries:
    /// where the view is scrolled is told by them.
    drawn: RowMap,
    /// The order those rows were of, and the pane's folds then
    /// (`Pane::folds_version`): a fold keeps the view on what was at its
    /// top (`RowMap::kept_row`).
    drawn_order: std::sync::Arc<[skwir_core::EntryId]>,
    folds_at: Option<u64>,
    /// A scroll asked for by an entry's position, made a row's when the
    /// view draws, with the rows as they are then: the order and its
    /// headings can change between the asking and the drawing.
    asked: Option<Asked>,
}

/// A scroll asked of the list by position (`ListView::asked`).
#[derive(Clone, Copy, Debug)]
enum Asked {
    /// Into view, moving the least.
    Near(usize),
    /// Into the middle.
    Middle(usize),
    /// Back where a tab was (`RowMap::anchor`).
    Anchor((usize, f32)),
    /// A row into view, a heading the keys went to.
    Row(usize),
}

impl Default for ListView {
    fn default() -> Self {
        ListView {
            scroll: UniformListScrollHandle::default(),
            height: ROW_HEIGHT,
            tree: false,
            drawn: RowMap::default(),
            drawn_order: std::sync::Arc::from([]),
            folds_at: None,
            asked: None,
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

    /// Put the row `anchor` names at the top, as far into it, at the rows'
    /// height as it is now.
    fn place_anchor(&mut self, anchor: (usize, f32)) {
        self.place_offset(row_offset(anchor, self.height));
    }

    /// Scroll to `y` (0 or less: the rows' top is `-y` above the view's).
    fn place_offset(&mut self, y: f32) {
        let state = self.scroll.0.borrow();
        let offset = state.base_handle.offset();
        state.base_handle.set_offset(gpui::point(offset.x, px(y)));
    }

    /// Carry out the scroll asked by position, in the rows of `rows`. A
    /// row under the pinned heading is hidden: where groups are drawn, one
    /// brought into view from above stops a row below the top.
    fn scroll_as_asked(&mut self, rows: &RowMap) {
        match self.asked.take() {
            Some(Asked::Near(ix)) if rows.grouped() => {
                self.scroll
                    .scroll_to_item_with_offset(rows.row_of(ix), ScrollStrategy::Nearest, 1);
            }
            Some(Asked::Near(ix)) => self
                .scroll
                .scroll_to_item(rows.row_of(ix), ScrollStrategy::Nearest),
            Some(Asked::Middle(ix)) => {
                self.scroll
                    .scroll_to_item(rows.row_of(ix), ScrollStrategy::Center);
            }
            Some(Asked::Row(row)) => self.scroll.scroll_to_item(row, ScrollStrategy::Nearest),
            Some(Asked::Anchor(anchor)) => {
                // A pending scroll (the reset of a new listing) would move
                // it again.
                self.scroll.0.borrow_mut().deferred_scroll_to_item = None;
                self.place_offset(-rows.offset_of(anchor, self.height));
            }
            None => {}
        }
    }
}

/// Everything a row needs, collected under the workspace lock: what every
/// view draws of the entry (`Look`), and what the list shows besides.
pub struct RowData {
    pub ix: usize,
    pub look: crate::views::entry::Look,
    pub hydrated: bool,
    /// What it says in each detail asked for, read by `Detail`: blank
    /// where none was (`rows_showing`), and in Rating, whose stars are
    /// `rating`.
    pub said: PerColumn<Said>,
    /// Half stars, from 1 to 10.
    pub rating: Option<u64>,
    /// How many opened folders deep it is (`Pane::depth`), and for a
    /// folder whether it is open in place.
    pub depth: usize,
    pub expanded: bool,
    /// Whether it can be opened in place: under the tree of what a search
    /// found, a folder with something found in it.
    pub opens: bool,
}

/// What a row says in a detail: the words drawn, and what pointing at
/// them adds, if anything (a folder's whole path, a date's exact time).
#[derive(Clone, Default, Debug, PartialEq)]
pub struct Said {
    pub text: String,
    pub whole: String,
}

impl Said {
    fn text(text: impl Into<String>) -> Self {
        Said {
            text: text.into(),
            whole: String::new(),
        }
    }
}

impl Shell {
    /// Collect row data for `range` of the sorted order. `compact` rows
    /// (the Columns view) show a name only: their details are not computed.
    pub fn rows(&mut self, range: Range<usize>, icon_px: u16, compact: bool) -> Vec<RowData> {
        let columns = if compact {
            PerColumn([false; COLUMNS.len()])
        } else {
            self.shown_columns()
        };
        self.rows_showing(range, icon_px, columns)
    }

    /// `rows`, with the details of `columns` computed, whatever the list
    /// shows: what the grid's captions say (`views::grid::CAPTIONS`).
    pub fn rows_showing(
        &mut self,
        range: Range<usize>,
        icon_px: u16,
        columns: PerColumn<bool>,
    ) -> Vec<RowData> {
        let mut out = Vec::with_capacity(range.len());
        let date = self.dates_said(columns);
        let app = std::sync::Arc::clone(&self.app);
        let ws = app.workspace.read();
        let Some(pane) = ws.pane(self.pane) else {
            return out;
        };
        for ix in range {
            let Some(&id) = self.ui().list.sorted.get(ix) else {
                break;
            };
            let (Some(e), Some(look)) = (pane.drawn_entry(id), self.look(pane, id, icon_px)) else {
                continue;
            };
            let folder = || pane.path_of(id)?.parent().map(ToOwned::to_owned);
            let mut row = self.row_data(e, look, ix, columns, &date, &folder);
            row.expanded = e.is_dir() && pane.is_open_at(id);
            row.opens = e.is_dir() && pane.opens_here(id);
            row.depth = pane.depth(id);
            out.push(row);
        }
        drop(ws);
        self.type_names.request(&app);
        self.icons.request(&app);
        out
    }

    /// How the date columns among `columns` say a date: as the column
    /// says it, and exactly for its tooltip.
    pub(crate) fn dates_said(
        &self,
        columns: PerColumn<bool>,
    ) -> impl Fn(Option<std::time::SystemTime>) -> Said + use<> {
        let friendly = self.prefs.friendly_dates;
        let dated = [
            Detail::Modified,
            Detail::Created,
            Detail::Accessed,
            Detail::Taken,
            Detail::Deleted,
        ];
        let now = (friendly && dated.iter().any(|detail| columns[*detail])).then(jiff::Zoned::now);
        move |time: Option<std::time::SystemTime>| -> Said {
            let exact = time.map_or_else(String::new, format::time);
            let shown = match (time, &now) {
                (None, _) => "—".to_owned(),
                (Some(time), Some(now)) => format::friendly_time(time, now),
                (Some(_), None) => exact.clone(),
            };
            Said {
                text: shown,
                whole: exact,
            }
        }
    }

    /// The rows of what arrives among `rows` (ADR-135), by which of what
    /// arrives each is, as `rows` (the list's columns) shows them.
    pub(crate) fn arriving_rows(
        &mut self,
        rows: &RowMap,
        range: Range<usize>,
        icon_px: u16,
    ) -> Vec<(usize, RowData)> {
        let columns = self.shown_columns();
        self.arriving_data(
            range.filter_map(|at| match rows.at(at)? {
                Row::Arriving(i) => Some(i),
                _ => None,
            }),
            icon_px,
            columns,
        )
    }

    /// The rows of what arrives `which` (ADR-135), with the details of
    /// `columns`: each as its file will be, at the top of a tree.
    pub(crate) fn arriving_data(
        &mut self,
        which: impl Iterator<Item = usize>,
        icon_px: u16,
        columns: PerColumn<bool>,
    ) -> Vec<(usize, RowData)> {
        let date = self.dates_said(columns);
        let which: Vec<(usize, skwir_core::op::arriving::Arriving)> = which
            .filter_map(|i| Some((i, self.ui().list.arriving.get(i)?.clone())))
            .collect();
        let rows = which
            .into_iter()
            .map(|(i, arriving)| {
                let look = self.arriving_look(&arriving, icon_px);
                let folder = arriving.arrival.folder.clone();
                let folder = move || Some(folder.clone());
                let mut row =
                    self.row_data(&arriving.arrival.like, look, i, columns, &date, &folder);
                row.depth = self.ui().list.arriving_depth.get(i).copied().unwrap_or(0);
                (i, row)
            })
            .collect();
        let app = std::sync::Arc::clone(&self.app);
        self.type_names.request(&app);
        self.icons.request(&app);
        rows
    }

    /// What row `ix` says of entry `e`, drawn as `look`: the details of
    /// `columns`, dates as `date` says them, its folder by `folder` where
    /// the Folder column asks. What a pane says of where it is in a tree
    /// is the caller's.
    pub(crate) fn row_data(
        &mut self,
        e: &skwir_core::Entry,
        look: crate::views::entry::Look,
        ix: usize,
        columns: PerColumn<bool>,
        date: &dyn Fn(Option<std::time::SystemTime>) -> Said,
        folder: &dyn Fn() -> Option<std::path::PathBuf>,
    ) -> RowData {
        let mut row = RowData {
            ix,
            look,
            expanded: false,
            opens: false,
            depth: 0,
            said: PerColumn(std::array::from_fn(|_| Said::default())),
            hydrated: e.metadata.is_hydrated(),
            rating: None,
        };
        let value = |key: &str| e.metadata.value(key);
        let time = |key: &str| value(key).and_then(skwir_core::Value::as_time);
        // What its type's database says of it: a place by what it is,
        // a folder as one, a link that leads nowhere as such.
        let typed = |words: &str| -> Option<std::sync::Arc<str>> {
            if e.is_place() {
                Some(
                    value(keys::PLACE_KIND)
                        .and_then(skwir_core::Value::as_str)
                        .unwrap_or("Place")
                        .into(),
                )
            } else if e.is_dir() {
                Some(words.into())
            } else if e.is_broken_link() {
                Some(crate::typenames::BROKEN_LINK.into())
            } else {
                None
            }
        };
        for column in COLUMNS.iter().filter(|column| columns[column.detail]) {
            let said = match column.detail {
                Detail::Rating => {
                    row.rating = value(keys::RATING).and_then(skwir_core::Value::as_u64);
                    continue;
                }
                Detail::Folder if e.is_place() => continue,
                Detail::Folder => {
                    // A file of the trash is from the folder it was put
                    // there from, which restoring it puts it back in.
                    let from = value(keys::TRASHED_FROM)
                        .and_then(skwir_core::Value::as_str)
                        .map(std::path::PathBuf::from);
                    let Some(dir) = from.or_else(folder) else {
                        continue;
                    };
                    let name = dir.file_name().map_or_else(
                        || dir.display().to_string(),
                        |n| n.to_string_lossy().into_owned(),
                    );
                    Said {
                        text: name,
                        whole: dir.display().to_string(),
                    }
                }
                Detail::Size => Said::text(
                    match value(keys::SIZE).and_then(skwir_core::Value::as_u64) {
                        Some(s) if !e.is_dir() => format::size(s),
                        // A place says how many files it holds.
                        _ => match value(keys::FILES).and_then(skwir_core::Value::as_u64) {
                            Some(n) => {
                                format::count(usize::try_from(n).unwrap_or(usize::MAX), "file")
                            }
                            None => "—".to_owned(),
                        },
                    },
                ),
                Detail::Dimensions => Said::text(dimensions(value(keys::DIMENSIONS))),
                Detail::Duration => Said::text(
                    value(keys::DURATION)
                        .and_then(skwir_core::Value::as_u64)
                        .map_or_else(
                            || "—".to_owned(),
                            // Milliseconds, said as a player says them.
                            |ms| crate::media::time(ms as f64 / 1000.),
                        ),
                ),
                Detail::Modified => date(time(keys::MODIFIED)),
                Detail::Created => date(time(keys::CREATED)),
                Detail::Accessed => date(time(keys::ACCESSED)),
                Detail::Deleted => date(time(keys::TRASHED)),
                Detail::Taken => date(
                    value(keys::TAKEN)
                        .and_then(skwir_core::Value::as_u64)
                        .and_then(on_the_clock),
                ),
                Detail::Type => Said::text(&*typed("Folder").unwrap_or_else(|| {
                    // Typed only where the column shows it.
                    skwir_platform::mime::of_entry(e)
                        .as_ref()
                        .and_then(|mime| self.type_names.get(mime))
                        .unwrap_or_else(|| crate::typenames::fallback_of(e).into())
                })),
                Detail::Kind => Said::text(&*typed("Folder").unwrap_or_else(|| {
                    skwir_platform::mime::of_entry(e)
                        .as_ref()
                        .and_then(|mime| self.type_names.kind(mime))
                        .unwrap_or_else(|| "".into())
                })),
                // A folder's is what it holds, as a screen reader hears it
                // (`entry::vcs_said`): "Holds changes".
                Detail::Vcs => Said::text(skwir_core::vcs::State::of(e).map_or("", |state| {
                    if e.is_dir() {
                        state.below()
                    } else {
                        state.title()
                    }
                })),
                Detail::Owner => Said::text(
                    value(keys::UID)
                        .and_then(skwir_core::Value::as_u64)
                        .map_or_else(
                            || "—".to_owned(),
                            // By its name, else its number.
                            |uid| {
                                skwir_platform::users::user(uid).unwrap_or_else(|| uid.to_string())
                            },
                        ),
                ),
            };
            row.said[column.detail] = said;
        }
        row
    }
}

/// The triangle that opens or closes folder `r` in place, pointing right
/// while it is closed. Pressing it does only that: the row is not selected.
fn disclosure(r: &RowData, color: gpui::Hsla, scale: Scale, cx: &mut Context<Shell>) -> Div {
    let slot = div()
        .w(scale.px(DISCLOSURE))
        .flex_none()
        .flex()
        .items_center();
    if !r.opens {
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
                    .size(scale.px(DISCLOSURE))
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
    let height = row_height(shell.row_zoom, files_scale(theme));
    let mut rows = rows.iter().peekable();
    range
        .map(|ix| match rows.next_if(|r| r.ix == ix) {
            Some(r) => row(shell, theme, r, compact, tree, cx),
            None => div().id(("row-gone", ix)).h(px(height)),
        })
        .collect()
}

/// An element for each row of `range` of a list whose rows are not all
/// entries (`rows`): a group's heading, what arrives (ADR-135), or an
/// entry's row as `row_elements` makes it. `data` holds the entries' and
/// what arrives's among them.
fn grouped_row_elements(
    shell: &Shell,
    theme: &Theme,
    range: Range<usize>,
    rows: &RowMap,
    (data, arriving): (&[RowData], &[(usize, RowData)]),
    tree: bool,
    cx: &mut Context<Shell>,
) -> Vec<Stateful<Div>> {
    let scale = files_scale(theme);
    let height = row_height(shell.row_zoom, scale);
    let mut data = data.iter().peekable();
    let mut arriving = arriving.iter().peekable();
    range
        .map(|at| match rows.at(at) {
            Some(Row::Heading(n)) => {
                let span = &rows.spans()[n];
                let chevron = groups::chevron(
                    n,
                    rows.folded(n),
                    shell.pane,
                    cx.weak_entity(),
                    scale,
                    theme,
                );
                let stopped = shell.heading_stop() == Some(n);
                let heading =
                    groups::heading(span, at == 0, height, None, scale, theme, Some(chevron))
                        .id(("heading", n))
                        .children(shell.menu_marker(shell.pane, stopped));
                let heading = groups::ring(heading, stopped, theme);
                // Each group's first entry says its group as well.
                groups::expose(
                    groups::acting(heading, n, shell.pane, cx.weak_entity(), theme),
                    span,
                    stopped,
                    shell.a11y,
                )
            }
            Some(Row::Entry(ix)) => match data.next_if(|r| r.ix == ix) {
                Some(r) => {
                    let first = rows
                        .group_of(ix)
                        .filter(|n| rows.spans()[*n].start == ix)
                        .map(|n| groups::said(&rows.spans()[n]));
                    row_in(shell, theme, r, false, tree, first, cx)
                }
                None => div().id(("row-gone", ix)).h(px(height)),
            },
            // A group no file is in yet (ADR-135): no stop, nothing to fold.
            Some(Row::Fresh(k)) => match rows.fresh().get(k) {
                Some(fresh) => {
                    let heading = groups::fresh_heading(fresh, at == 0, height, None, scale, theme)
                        .id(("fresh-heading", k));
                    if shell.a11y {
                        heading
                            .role(gpui::Role::Heading)
                            .aria_level(2)
                            .aria_label(groups::fresh_said(fresh))
                    } else {
                        heading
                    }
                }
                None => div().id(("fresh-gone", k)).h(px(height)),
            },
            Some(Row::Arriving(i)) => match (
                arriving.next_if(|(n, _)| *n == i),
                shell.ui().list.arriving.get(i),
            ) {
                (Some((_, r)), Some(it)) => arriving_row(shell, theme, r, it, (i, tree), cx),
                _ => div().id(("arriving-gone", i)).h(px(height)),
            },
            None => div().id(("row-none", at)).h(px(height)),
        })
        .collect()
}

/// What arrives's row (ADR-135): drawn as its file's will be, at the top
/// of a tree, dimmed, its tint behind it, its pill after its name.
fn arriving_row(
    shell: &Shell,
    theme: &Theme,
    r: &RowData,
    arriving: &skwir_core::op::arriving::Arriving,
    (i, tree): (usize, bool),
    cx: &mut Context<Shell>,
) -> Stateful<Div> {
    let look = &r.look;
    let muted = theme.text_muted;
    let scale = files_scale(theme);
    let height = row_height(shell.row_zoom, scale);
    let icon = div()
        .flex_none()
        .opacity(crate::views::arriving::OPACITY)
        .child(look.picture(icon_px(shell.row_zoom), theme));
    // In the tree, indented with its name under the folder it arrives in.
    let (in_row, in_name) = if tree {
        (None, Some(icon))
    } else {
        (Some(icon), None)
    };
    let name = div()
        .flex()
        .items_center()
        .gap_1()
        .when_some(in_name, |d, icon| {
            d.pl(px(r.depth as f32 * scale.f(INDENT)))
                .child(disclosure(r, muted, scale, cx))
                .child(icon)
        })
        .child(crate::views::entry::name(
            div()
                .id(("arriving-name", i))
                .opacity(crate::views::arriving::OPACITY),
            look,
        ))
        .child(crate::views::arriving::pill(arriving, theme));
    let mut line = line(name, NAME_MIN, height, scale);
    let shown = shell.shown_columns();
    for column in COLUMNS.iter().filter(|column| shown[column.detail]) {
        let cell = cell(shell, column.detail, muted, height, scale)
            .opacity(crate::views::arriving::OPACITY);
        line = line.child(detail(r, column.detail, cell, scale, theme));
    }
    let el = div()
        .id(("arriving", i))
        .relative()
        .h(px(height))
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .px_2()
        // The ring an entry's highlight draws, unseen: what arrives lines
        // up with the rows around it.
        .border_1()
        .border_color(gpui::transparent_black())
        .child(crate::views::arriving::tint(arriving, theme))
        .child(
            div()
                .relative()
                .flex()
                .items_center()
                .gap_2()
                .flex_1()
                .min_w_0()
                .children(in_row)
                .child(line),
        );
    let el = crate::views::arriving::acting(el, arriving, shell, cx);
    crate::views::arriving::expose(el, &look.name, arriving, shell.a11y)
}

pub fn row(
    shell: &Shell,
    theme: &Theme,
    r: &RowData,
    compact: bool,
    tree: bool,
    cx: &mut Context<Shell>,
) -> Stateful<Div> {
    row_in(shell, theme, r, compact, tree, None, cx)
}

/// `row`, the first of its group's entries where `group` says what a
/// screen reader hears of the group (`groups::said`).
fn row_in(
    shell: &Shell,
    theme: &Theme,
    r: &RowData,
    compact: bool,
    tree: bool,
    group: Option<String>,
    cx: &mut Context<Shell>,
) -> Stateful<Div> {
    let ix = r.ix;
    let look = &r.look;
    // Drawn in the selection's ink where chosen (`Chosen`).
    let muted = theme.text_muted;
    let scale = files_scale(theme);
    let height = row_height(shell.row_zoom, scale);
    let icon = look.picture(icon_px(shell.row_zoom), theme);
    // Known by its entry: what is inside it, by its place in it.
    let el = crate::views::entry::highlight(
        div()
            .id(crate::elements::known_by("row", look.id))
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
    // Something landing on it (a folder merged into): its tint under it,
    // its pill after its name (ADR-135).
    let on = shell.ui().list.arriving_on.get(&look.id);
    let el = match on {
        Some(arriving) => el.child(crate::views::arriving::tint(arriving, theme)),
        None => el,
    };
    let pill = on
        .map(|arriving| crate::views::arriving::pill(arriving, theme))
        .or_else(|| {
            look.writing
                .then(|| crate::views::arriving::writing_pill(theme))
        });
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
            d.pl(px(r.depth as f32 * scale.f(INDENT)))
                .child(disclosure(r, muted, scale, cx))
                .child(div().flex_none().child(icon))
        })
        // A name keeps its end: the extension tells files apart.
        .child(crate::views::entry::name(div().id(("name", ix)), look))
        .children(pill)
        // The tags take only what the name leaves (no basis of their own):
        // where chips do not fit they give way to dots, then to "+N", and
        // the name never gives way to them.
        .when(!look.tags.is_empty(), |d| {
            d.child(crate::views::entry::tags(
                look,
                crate::tags::StripOn::Row {
                    selected: look.selected,
                },
                theme,
                Some(Shell::tag_act(shell.pane, Some(look.id), false, cx)),
            ))
        });
    let name_min = if compact { 0. } else { NAME_MIN };
    let mut line = line(name, name_min, height, scale);
    if !compact {
        let shown = shell.shown_columns();
        for column in COLUMNS.iter().filter(|column| shown[column.detail]) {
            let cell = cell(shell, column.detail, muted, height, scale);
            line = line.child(detail(r, column.detail, cell, scale, theme));
        }
    }
    let seat = crate::views::entry::Seat {
        index: ix,
        tree: tree.then_some((r.depth, look.is_dir.then_some(r.expanded))),
        group,
    };
    crate::views::entry::behave(el.child(line), look, seat, shell, cx)
}

/// The name and the detail columns of a row, or of the header. A wrapping
/// flex line one row tall: a column that no longer fits wraps below and is
/// clipped, so columns drop whole, last first (Type, then Modified, then
/// Size), and the name, the only child that grows, takes what they leave: it
/// never sits at `NAME_MIN` beside the blank of a column that went. Pure
/// layout: nothing is measured, the header and every row agree by
/// construction, and a pane of any width is laid out correctly in the frame
/// it gets that width. `height` is the row's, drawn; `name_min` is at
/// 100 %, drawn at `scale`.
pub(crate) fn line(name: Div, name_min: f32, height: f32, scale: Scale) -> Div {
    div()
        .h(px(height - 2.))
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .flex()
        .flex_row()
        .flex_wrap()
        .content_start()
        .gap_x(scale.px(DETAIL_GAP))
        .child(
            name.h(px(height - 2.))
                .flex()
                .items_center()
                .flex_1()
                .min_w(scale.px(name_min)),
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
fn cell(shell: &Shell, detail: Detail, color: gpui::Hsla, height: f32, scale: Scale) -> Div {
    column(scale.f(shell.column_widths[detail]), color, height)
        .when(detail.column().right, gpui::Styled::text_right)
}

/// What row `r` shows in its `detail` column. A text cut to fit says itself
/// whole when pointed at, before what the column adds (a folder's path, the
/// exact time).
fn detail(r: &RowData, detail: Detail, cell: Div, scale: Scale, theme: &Theme) -> AnyElement {
    if detail == Detail::Rating {
        return cell
            .flex()
            .items_center()
            .children(
                r.rating
                    .map(|stars| crate::tags::stars(stars, scale.f(12.), theme)),
            )
            .into_any_element();
    }
    let said = &r.said[detail];
    crate::elements::clip::clipped_with(
        cell.id((detail.column().key, r.ix)).min_w_0().truncate(),
        said.text.clone(),
        (!said.whole.is_empty()).then(|| said.whole.clone().into()),
    )
    .into_any_element()
}

/// A picture's size as a column says it: `4032 × 3024`, a dash while
/// unknown or where it has none.
fn dimensions(value: Option<&skwir_core::Value>) -> String {
    let Some(skwir_core::Value::List(sides)) = value else {
        return "—".to_owned();
    };
    match (
        sides.first().and_then(skwir_core::Value::as_u64),
        sides.get(1).and_then(skwir_core::Value::as_u64),
    ) {
        (Some(width), Some(height)) => format!("{width} × {height}"),
        _ => "—".to_owned(),
    }
}

/// When a camera's clock (`keys::TAKEN`: `20260701083000`, in no zone)
/// says a picture was taken, as the time it is here at that hour: said as
/// every date is, it reads as the clock read.
fn on_the_clock(clock: u64) -> Option<std::time::SystemTime> {
    let part = |by: u64| i8::try_from(clock / by % 100).ok();
    let civil = jiff::civil::DateTime::new(
        i16::try_from(clock / 10_000_000_000).ok()?,
        part(100_000_000)?,
        part(1_000_000)?,
        part(10_000)?,
        part(100)?,
        part(1)?,
        0,
    )
    .ok()?;
    let zoned = civil.to_zoned(jiff::tz::TimeZone::system()).ok()?;
    Some(std::time::SystemTime::from(zoned.timestamp()))
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
    let scale = files_scale(theme);
    let height = scale.f(ROW_HEIGHT);
    let shown = shell.shown_columns();
    let mut line = line(
        // In the tree, Name stands over the names, past the triangle and
        // the icon a row begins with.
        div()
            .when(tree, |d| d.pl(px(scale.f(DISCLOSURE + 8.) + icon)))
            .child(header_cell(
                "h-name",
                "Name",
                Some(SortKey::Name),
                shell,
                theme,
                cx,
            )),
        NAME_MIN,
        height,
        scale,
    );
    for column in COLUMNS.iter().filter(|column| shown[column.detail]) {
        line = line.child(
            cell(shell, column.detail, theme.text_muted, height, scale)
                .relative()
                .child(header_cell(
                    ("h", column.detail as usize),
                    column.label,
                    column.sort,
                    shell,
                    theme,
                    cx,
                ))
                .child(resize_edge(column.detail, scale, cx)),
        );
    }
    div()
        .h(scale.px(24.))
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .px_2()
        .text_small()
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

    fn role(&self) -> gpui::Role {
        if self.tree {
            gpui::Role::Tree
        } else {
            gpui::Role::ListBox
        }
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

    fn lays_found_out_as_tree(&self) -> bool {
        self.tree
    }

    fn render(&mut self, shell: &mut Shell, _: &mut Window, cx: &mut Context<Shell>) -> AnyElement {
        let theme = cx.global::<Theme>().clone();
        // Rows: the entries, and the groups' headings among them.
        let rows = shell.ui().list.rows.clone();
        let count = rows.len();
        let entries = shell.ui().list.sorted.len();
        let pane = shell.pane;
        let tree = self.tree;
        let height = row_height(shell.row_zoom, files_scale(&theme));
        let icon_px = u16::try_from(shell.row_zoom).unwrap_or(u16::MAX);
        if (height - self.height).abs() > f32::EPSILON {
            // Zoomed, or the text resized: the row at the top stays at the
            // top, as far into it.
            let before = self.height;
            let (row, within) = row_anchor(
                f32::from(self.scroll.0.borrow().base_handle.offset().y),
                before,
            );
            self.height = height;
            // A scroll asked meanwhile still runs after it: the zoom keys
            // keep the cursor in view (`Shell::zoom_step`).
            self.place_anchor((row, within * height / before));
        }
        // A fold (or an unfold), from a heading, its menu or the palette,
        // keeps the view on what was at its top: the same entry, or, one
        // folded away, its group's heading, never the rows that slid
        // under the same pixels (ADR-131, 2).
        let folds = shell
            .app
            .workspace
            .read()
            .pane(pane)
            .map(skwir_core::workspace::Pane::folds_version);
        if folds != self.folds_at {
            if self.folds_at.is_some() && self.asked.is_none() {
                let offset = f32::from(self.scroll.0.borrow().base_handle.offset().y);
                let (row, within) = row_anchor(offset, self.height);
                let list = &shell.ui().list;
                if let Some(kept) =
                    rows.kept_row(&self.drawn, &self.drawn_order, row, |id| list.index_of(id))
                {
                    // A heading kept is put at the top, where it is drawn.
                    let within = if matches!(rows.at(kept), Some(Row::Heading(_))) {
                        0.
                    } else {
                        within
                    };
                    self.place_anchor((kept, within));
                }
            }
            self.folds_at = folds;
        }
        self.scroll_as_asked(&rows);
        self.drawn = rows.clone();
        self.drawn_order = std::sync::Arc::clone(&shell.ui().list.sorted);
        let drawn = rows.clone();
        let list = uniform_list(
            "rows",
            count,
            cx.processor(move |this: &mut Shell, range: Range<usize>, _window, cx| {
                this.with_pane(pane, |this| {
                    let theme = cx.global::<Theme>().clone();
                    let shown = drawn.entries_in(range.clone());
                    let list = &mut this.ui_mut().list;
                    list.visible_rows = range.len().max(1);
                    list.visible_items = shown.len();
                    let data = this.rows(shown, icon_px, false);
                    this.request_hydrate(data.iter().map(|r| r.look.id).collect::<Vec<_>>());
                    if drawn.plain() {
                        row_elements(this, &theme, range, &data, (false, tree), cx)
                    } else {
                        let arriving = this.arriving_rows(&drawn, range.clone(), icon_px);
                        let given = (&data[..], &arriving[..]);
                        grouped_row_elements(this, &theme, range, &drawn, given, tree, cx)
                    }
                })
            }),
        )
        .track_scroll(&self.scroll)
        .when(rows.grouped(), |list| {
            list.with_decoration(groups::Pinned {
                rows: rows.clone(),
                scale: files_scale(&theme),
                theme: theme.clone(),
                ground: theme.files_ground(shell.lowered()),
                stopped: shell.heading_stop(),
                anchor: shell.host_row_anchor(crate::menu::MenuHost::Pane(pane)),
                a11y: shell.a11y,
                pane,
                shell: cx.weak_entity(),
            })
        })
        .size_full();
        let marquee = crate::elements::marquee::Marquee {
            pane,
            shell: cx.weak_entity(),
            source: crate::elements::marquee::Source::Rows { height, rows },
            count: entries,
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
                    .child(zoom_wheel::overlay(Zoomed::View(pane), cx))
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
        self.asked = Some(Asked::Near(ix));
    }

    fn scroll_to_middle(&mut self, ix: usize) {
        self.asked = Some(Asked::Middle(ix));
    }

    /// The entry at the top, a heading there told by a distance above it.
    fn scroll_anchor(&self) -> Option<(usize, f32)> {
        let offset = -f32::from(self.scroll.0.borrow().base_handle.offset().y);
        self.drawn.anchor(offset, self.height).or(Some((0, 0.)))
    }

    fn restore_anchor(&mut self, anchor: (usize, f32)) {
        self.asked = Some(Asked::Anchor(anchor));
    }

    /// A page goes a page of rows, headings among them, to the entry there
    /// (a heading's, the first of its group).
    fn nav_delta(&self, nav: Nav, shell: &Shell) -> Option<isize> {
        let delta = rows_nav(nav, shell)?;
        let rows = &shell.ui().list.rows;
        let Some(cursor) = shell
            .cursor_index()
            .filter(|_| !rows.plain() && delta.abs() > 1)
        else {
            return Some(delta);
        };
        let row = (rows.row_of(cursor) as isize + delta).clamp(0, rows.len() as isize - 1);
        let to = rows.entry_near(row as usize).unwrap_or(cursor);
        Some(to as isize - cursor as isize)
    }

    /// Grouped, the plain arrows stop on headings (ADR-131, 1): on one,
    /// Left folds it and Right opens it, the pages go a page of rows. In the
    /// tree, Right opens the folder under the cursor, or goes into it when
    /// it is open; Left closes it, or goes to the folder it is in.
    fn nav(
        &mut self,
        nav: Nav,
        step: crate::list::Step,
        shell: &mut Shell,
        window: &mut Window,
        cx: &mut Context<Shell>,
    ) -> bool {
        if step == Step::Select
            && let Some(handled) = self.heading_nav(nav, shell, window, cx)
        {
            return handled;
        }
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
        self.asked = None;
        self.folds_at = None;
        self.scroll.scroll_to_item_strict(0, ScrollStrategy::Top);
    }
}

impl ListView {
    /// A plain arrow over grouped rows, headings among them (ADR-131, 1):
    /// `Some` when it is the headings' to answer, with whether it did.
    fn heading_nav(
        &mut self,
        nav: Nav,
        shell: &mut Shell,
        window: &mut Window,
        cx: &mut Context<Shell>,
    ) -> Option<bool> {
        let rows = shell.ui().list.rows.clone();
        if !rows.grouped() {
            return None;
        }
        let stop = shell.heading_stop();
        let row = match (stop, shell.cursor_index()) {
            (Some(n), _) => rows.heading_row(n),
            (None, Some(cursor)) => rows.row_of(cursor),
            // Every group folded: no entry to be on, the keys go to the
            // first heading or the last.
            (None, None)
                if shell.ui().list.sorted.is_empty() && matches!(nav, Nav::Up | Nav::Down) =>
            {
                let n = if nav == Nav::Down {
                    0
                } else {
                    rows.spans().len() - 1
                };
                shell.stop_on_heading(n, cx);
                self.asked = Some(Asked::Row(rows.heading_row(n)));
                return Some(true);
            }
            (None, None) => return None,
        };
        match (nav, stop) {
            (Nav::Up | Nav::Down, _) => {
                let next_row = |from: usize| {
                    if nav == Nav::Up {
                        from.checked_sub(1)
                    } else {
                        Some(from + 1).filter(|to| *to < rows.len())
                    }
                };
                // What arrives is no stop: the keys go past it.
                let mut to = next_row(row);
                while let Some(at) = to
                    && matches!(rows.at(at), Some(Row::Arriving(_) | Row::Fresh(_)))
                {
                    to = next_row(at);
                }
                match to.and_then(|to| Some((to, rows.at(to)?))) {
                    Some((to, Row::Heading(n))) => {
                        shell.stop_on_heading(n, cx);
                        self.asked = Some(Asked::Row(to));
                        Some(true)
                    }
                    Some((_, Row::Entry(ix))) if stop.is_some() => {
                        shell.cursor_to(ix as isize, Step::Select, window, cx);
                        Some(true)
                    }
                    // At an end, the keys stay on the heading.
                    None if stop.is_some() => Some(true),
                    _ => None,
                }
            }
            (Nav::Left, Some(n)) => {
                shell.fold_from_heading(n, "fold", window, cx);
                Some(true)
            }
            (Nav::Right, Some(n)) => {
                shell.fold_from_heading(n, "unfold", window, cx);
                Some(true)
            }
            (Nav::PageUp | Nav::PageDown | Nav::HalfPageUp | Nav::HalfPageDown, Some(_)) => {
                let delta = rows_nav(nav, shell)?;
                let to = (row as isize + delta).clamp(0, rows.len() as isize - 1) as usize;
                // No entry at all (every group folded): the keys stay.
                if let Some(ix) = rows.entry_near(to) {
                    shell.cursor_to(ix as isize, Step::Select, window, cx);
                }
                Some(true)
            }
            // Home and End go to the order's ends, from wherever the keys are.
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_ROW_ZOOM, ROW_HEIGHT, ROW_ZOOMS, row_anchor, row_height, row_offset};
    use crate::sizes::Scale;

    #[test]
    fn a_scroll_position_comes_back_to_the_pixel() {
        for h in ROW_ZOOMS.map(|zoom| row_height(zoom, Scale::ONE)) {
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
        assert!((row_height(DEFAULT_ROW_ZOOM, Scale::ONE) - ROW_HEIGHT).abs() < f32::EPSILON);
        assert!(ROW_ZOOMS.contains(&DEFAULT_ROW_ZOOM));
    }

    #[test]
    fn rows_grow_with_their_text_and_icons_keep_their_zoom() {
        // Text at 150 %: the row holds it, the icon as it was.
        assert!((row_height(DEFAULT_ROW_ZOOM, Scale(1.5)) - 39.).abs() < f32::EPSILON);
        // The largest icon still needs more than the text.
        assert!((row_height(48, Scale(1.5)) - 56.).abs() < f32::EPSILON);
        // Smaller text never makes a row shorter than its icon needs.
        assert!((row_height(DEFAULT_ROW_ZOOM, Scale(0.8)) - ROW_HEIGHT).abs() < f32::EPSILON);
    }
}
