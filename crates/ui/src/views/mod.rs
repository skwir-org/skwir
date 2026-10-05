//! Built-in pane views.

pub mod board;
pub mod columns;
pub mod entry;
pub mod grid;
pub mod list;

use bibli_core::registry::ViewSpec;
use gpui::{Context, ScrollWheelEvent, prelude::*};

use crate::shell::Shell;
use crate::view::{self, spec};

/// Ctrl and the wheel over a view zoom it (`Shell::zoom_wheel`). An overlay
/// over the view's scroll area, taking the wheel in the capture phase so the
/// view under it does not also scroll.
pub fn zoom_wheel(cx: &mut Context<Shell>) -> impl IntoElement {
    let shell = cx.weak_entity();
    gpui::canvas(
        |bounds, window, _| window.insert_hitbox(bounds, gpui::HitboxBehavior::Normal),
        move |_, hitbox, window, _| {
            window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, cx| {
                if !phase.capture()
                    || !event.modifiers.control
                    || !hitbox.should_handle_scroll(window)
                {
                    return;
                }
                let delta = event.delta.pixel_delta(window.line_height()).y;
                let lines = matches!(event.delta, gpui::ScrollDelta::Lines(_));
                shell
                    .update(cx, |shell, cx| {
                        shell.zoom_wheel(f32::from(delta), lines, cx);
                    })
                    .ok();
                cx.stop_propagation();
            });
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

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
        crate::view::USAGE => Some(Box::new(crate::usage::UsageView)),
        crate::map_view::ID => Some(Box::new(crate::map_view::MapView::default())),
        _ => None,
    }
}
