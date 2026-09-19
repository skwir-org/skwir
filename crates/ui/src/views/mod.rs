//! Built-in pane views.

pub mod columns;
pub mod grid;
pub mod list;

use bibli_core::registry::ViewSpec;

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
        spec(
            view::GRID,
            "Grid",
            1,
            || Box::new(grid::GridView::default()),
        ),
        spec(view::COLUMNS, "Columns", 2, || {
            Box::new(columns::ColumnsView::default())
        }),
    ]
}

/// Bundled renderers for toolkit-independent plugin descriptors.
/// Add a renderer here; registration, toolbar and palette are owned by the plugin.
pub fn named_renderer(id: &str) -> Option<Box<dyn crate::view::PaneView>> {
    match id {
        "usage" => Some(Box::new(crate::usage::UsageView)),
        _ => None,
    }
}
