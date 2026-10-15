//! Built-in pane views.

pub mod board;
pub mod columns;
pub mod entry;
pub mod grid;
pub mod list;

use skwir_core::registry::ViewSpec;

use crate::view::{self, spec};

/// The specs registered under owner `ui`.
pub fn specs() -> Vec<ViewSpec> {
    vec![
        spec(
            view::LIST,
            "List",
            0,
            || Box::new(list::ListView::default()),
        ),
        // Beside the list it extends, in the View menu: a button only
        // while shown. No Ctrl+digit: the others keep theirs, and Usage
        // and the map have the next ones.
        spec(view::TREE, "Tree", 1, || Box::new(list::ListView::tree())).alternate(),
        spec(
            view::GRID,
            "Grid",
            2,
            || Box::new(grid::GridView::default()),
        ),
        spec(view::COLUMNS, "Columns", 3, || {
            Box::new(columns::ColumnsView::default())
        }),
        // The folders, or a tag's tags, side by side: a shape chosen now
        // and then, with a button only while shown.
        spec(view::BOARD, "Board", 4, || {
            Box::new(board::BoardView::default())
        })
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
