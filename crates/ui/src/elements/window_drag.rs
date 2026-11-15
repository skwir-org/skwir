//! Where a press moves the window, while skwir draws its title bar
//! (`title_bar.rs`, ADR-138): the empty parts of the rows that hold the
//! window's buttons, the frame's gutter above the cards, the bar of its
//! own. One element for all, laid under what the row holds: it acts only
//! where nothing else is under the pointer (`Hitbox::is_frontmost`, gpui
//! patch 0063), so a button, the address field or a tab keeps its press.
//!
//! As a title bar does on every desktop: pressed and moved, the window
//! moves; pressed twice, it is maximized or restored (`zooms`); the right
//! button opens the system's menu for the window, where it has one.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    HitboxBehavior, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Styled,
    canvas,
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
            let armed = press.clone();
            window.on_mouse_event(move |event: &MouseDownEvent, phase, window, _| {
                if !phase.bubble() || !hitbox.is_frontmost(window) {
                    return;
                }
                match event.button {
                    MouseButton::Left if zooms && event.click_count == 2 => {
                        armed.set(false);
                        window.zoom_window();
                    }
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
