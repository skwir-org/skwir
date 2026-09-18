//! Built-in pane views.

pub mod columns;
pub mod grid;
pub mod list;

use bibli_core::registry::ViewSpec;

use crate::view::{self, spec};

/// The specs registered under owner `ui`.
pub fn specs() -> Vec<ViewSpec> {
    vec![
        spec(view::LIST, "List", || Box::new(list::ListView::default())),
        spec(view::GRID, "Grid", || Box::new(grid::GridView::default())),
        spec(view::COLUMNS, "Columns", || {
            Box::new(columns::ColumnsView::default())
        }),
    ]
}
