//! Built-in pane views.

pub mod arriving;
pub mod board;
pub mod columns;
pub mod entry;
pub mod grid;
pub mod groups;
pub mod list;

use skwir_core::registry::ViewSpec;
use skwir_core::words;

use crate::view::{self, spec};

/// The specs registered under owner `ui`.
pub fn specs() -> Vec<ViewSpec> {
    vec![
        // The two a folder may remember as its view (ADR-132).
        spec(
            view::LIST,
            words!(
                views_list = "List",
                note = "A view's name: the files of a folder in rows, one under the other."
            ),
            0,
            || Box::new(list::ListView::default()),
        )
        .remembered(),
        // Beside the list it extends, in the View menu: a button only
        // while shown. No Ctrl+digit: the others keep theirs, and Usage
        // and the map have the next ones.
        spec(
            view::TREE,
            words!(
                views_tree = "Tree",
                note = "A view's name: the list with folders opened in place below their rows."
            ),
            1,
            || Box::new(list::ListView::tree()),
        )
        .alternate(),
        spec(
            view::GRID,
            words!(
                views_grid = "Grid",
                note = "A view's name: the files of a folder as tiles with their pictures."
            ),
            2,
            || Box::new(grid::GridView::default()),
        )
        .remembered(),
        spec(
            view::COLUMNS,
            words!(
                views_columns = "Columns",
                note = "A view's name: the folders above shown side by side, each a column."
            ),
            3,
            || Box::new(columns::ColumnsView::default()),
        ),
        // The folders, or a tag's tags, side by side: a shape chosen now
        // and then, with a button only while shown.
        spec(
            view::BOARD,
            words!(
                views_board = "Board",
                note = "A view's name: the folders, or a tag's tags, side by side as columns of cards."
            ),
            4,
            || Box::new(board::BoardView::default()),
        )
        .alternate(),
    ]
}

/// Bundled renderers for toolkit-independent plugin descriptors.
/// Add a renderer here; registration, toolbar and palette are owned by the plugin.
pub fn named_renderer(id: &str) -> Option<Box<dyn crate::view::PaneView>> {
    match id {
        crate::view::USAGE => Some(Box::<crate::usage::UsageView>::default()),
        crate::map_view::ID => Some(Box::new(crate::map_view::MapView::default())),
        _ => None,
    }
}

/// What keeps its place in a view changing size (`elements::kept`), among
/// `shown`, what the view drew the last frame, in its order: where the
/// cursor is there, then where what is selected is. Only one wholly in view
/// is kept, so only what was drawn is asked of.
pub fn kept(shell: &crate::shell::Shell, shown: &[skwir_core::EntryId]) -> Vec<usize> {
    let ws = shell.app.workspace.read();
    let Some(pane) = ws.pane(shell.pane) else {
        return Vec::new();
    };
    let selection = &pane.selection;
    let cursor = selection
        .cursor
        .and_then(|cursor| shown.iter().position(|id| *id == cursor));
    let mut kept: Vec<usize> = cursor.into_iter().collect();
    if !selection.is_empty() {
        kept.extend(
            (0..shown.len()).filter(|at| Some(*at) != cursor && selection.contains(shown[*at])),
        );
    }
    kept
}

/// The rows of a `uniform_list` that keep their place as it changes size
/// (`elements::kept::rows`): what `kept` says of the rows it drew the last
/// frame (`drawn` over `order`, `height` tall then, where its handle
/// `scroll` left them), each with its row then and its row now in `now`,
/// found by its place in its order now (`position`, given its place in
/// `order` and its entry).
pub fn kept_rows(
    shell: &crate::shell::Shell,
    (drawn, order): (&groups::RowMap, &[skwir_core::EntryId]),
    scroll: &gpui::UniformListScrollHandle,
    height: f32,
    now: &groups::RowMap,
    position: impl Fn(usize, skwir_core::EntryId) -> Option<usize>,
) -> Vec<(usize, usize)> {
    if height <= 0. {
        return Vec::new();
    }
    let base = scroll.0.borrow().base_handle.clone();
    let top = (-f32::from(base.offset().y)).max(0.);
    let view = f32::from(base.bounds().size.height);
    // Every row partly in view: only one wholly in it is kept (`seen`).
    let first = (top / height).floor() as usize;
    let end = ((top + view) / height).ceil() as usize;
    let shown = drawn.entries_in(first..end.max(first));
    let Some(ids) = order.get(shown.clone()) else {
        return Vec::new();
    };
    kept(shell, ids)
        .into_iter()
        .filter_map(|at| {
            let was = drawn.row_of(shown.start + at);
            Some((was, now.row_of(position(shown.start + at, ids[at])?)))
        })
        .collect()
}
