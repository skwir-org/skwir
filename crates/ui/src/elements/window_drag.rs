//! Where a press moves the window, while skwir draws its title bar
//! (`title_bar.rs`, ADR-138): the empty parts of the rows that hold the
//! window's buttons, the frame's gutter above the cards, the bar of its
//! own. One element for all, laid under what the row holds: it acts only
//! where nothing else is under the pointer (`Hitbox::is_frontmost`, gpui
//! patch 0063), so a button, the address field or a tab keeps its press.
//!
//! As a title bar does on every desktop: pressed and moved, the window
//! moves; pressed twice, it is maximized or restored (`zooms`); the right
//! button opens the system's menu for the window, where it has one. On
//! Windows the system does all three: the region is told to it as a
//! caption (`WindowControlArea::Drag`, where it is frontmost: gpui patch
//! 0066), so it also snaps the window to the screen's edges; beside the
//! tabs, one whose double click is the row's (`DragOnly`, gpui patch 0068). On macOS the
//! press itself starts the move (`AppKit` drags from a press), and a double
//! click does what the system's settings say (`titlebar_double_click`).
//! Everywhere, the second press of a double click moves nothing: a move
//! takes the release, and a row whose double click is its own (beside the
//! tabs) needs it for its click.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    HitboxBehavior, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Styled,
    WindowControlArea, canvas,
};

/// The press that moves the window once the pointer moves, one a window:
/// a click that stays where it is moves nothing (a double click maximizes,
/// which a move begun at its first press would swallow).
#[derive(Clone, Default)]
pub struct Press(Rc<Cell<bool>>);

/// A region over its parent (`relative`), its first child, under what the
/// parent holds. `zooms`: a double click maximizes or restores the window;
/// not where a double click means something of the row's own (beside the
/// tabs, which opens one).
pub fn area(press: &Press, zooms: bool) -> impl IntoElement + use<> {
    let press = press.0.clone();
    canvas(
        |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
        move |_, hitbox, window, _| {
            if cfg!(target_os = "windows") {
                // Beside the tabs a double click opens one: there Windows
                // moves the window but leaves its double click to the row
                // (gpui patch 0068).
                let area = if zooms {
                    WindowControlArea::Drag
                } else {
                    WindowControlArea::DragOnly
                };
                window.insert_window_control_hitbox(area, hitbox);
                return;
            }
            let armed = press.clone();
            window.on_mouse_event(move |event: &MouseDownEvent, phase, window, _| {
                if !phase.bubble() || !hitbox.is_frontmost(window) {
                    return;
                }
                match event.button {
                    // The second press of a double click never moves the
                    // window: it maximizes or restores it, or, where a
                    // double click is the row's own, the row has it (a move
                    // begun takes the release its click needs: on macOS,
                    // from a press).
                    MouseButton::Left if event.click_count >= 2 => {
                        armed.set(false);
                        if zooms && event.click_count == 2 {
                            if cfg!(target_os = "macos") {
                                window.titlebar_double_click();
                            } else {
                                window.zoom_window();
                            }
                        }
                    }
                    MouseButton::Left if cfg!(target_os = "macos") => window.start_window_move(),
                    MouseButton::Left => armed.set(true),
                    MouseButton::Right if window.window_controls().window_menu => {
                        window.show_window_menu(event.position);
                    }
                    _ => {}
                }
            });
            let armed = press.clone();
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, _| {
                if !phase.bubble() || !armed.replace(false) {
                    return;
                }
                if event.pressed_button == Some(MouseButton::Left) {
                    window.start_window_move();
                }
            });
            let armed = press.clone();
            window.on_mouse_event(move |_: &MouseUpEvent, _, _, _| armed.set(false));
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}
