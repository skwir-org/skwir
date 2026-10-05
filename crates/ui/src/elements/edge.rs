//! The edge the user drags to size an area: the sidebar's, the preview's,
//! the split's, the archive's view's, the dock's, the terminal's, a list
//! column's. One element for all: a strip over the area's border, the
//! resize cursor, the handle's color under the pointer, a double click
//! back to the size the area ships with, and nothing following the
//! pointer while dragged (the edge itself follows). What a drag means is
//! the payload's (`T`): the area's owner listens for its `DragMoveEvent`
//! where the room it shares is (the panes' container for the split, the
//! window's root for the rest), and the root's mouse-up remembers where
//! the edge was left (`Shell::resize_hook`).

use gpui::{Context, Div, ElementId, Stateful, div, prelude::*, px};

use crate::shell::Shell;
use crate::theme::Theme;

/// How thick an edge is to the pointer.
pub const THICKNESS: f32 = 6.;

/// Which edge of its area it is, the area being its parent (`relative`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
    Top,
}

/// The edge on `side` of its parent, dragged as `drag`; a double click
/// runs `reset`, then the window's shape is remembered.
pub fn edge<T: 'static>(
    id: impl Into<ElementId>,
    side: Side,
    drag: T,
    reset: impl Fn(&mut Shell) + 'static,
    cx: &mut Context<Shell>,
) -> Stateful<Div> {
    let hover = cx.global::<Theme>().handle;
    let shell = cx.weak_entity();
    let el = div()
        .id(id)
        .absolute()
        .top_0()
        .hover(move |style| style.bg(hover))
        // The press is the edge's: nothing under it starts a band or
        // selects.
        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
            if event.click_count() == 2 {
                reset(this);
                this.remember_session();
                cx.notify();
            }
        }))
        .on_drag(drag, move |_, _, _, cx| {
            // A drag starts clean: a release missed outside the window
            // cannot leave another drag's start behind.
            shell.update(cx, |shell, _| shell.edge_start = None).ok();
            cx.new(|_| Nothing)
        });
    match side {
        Side::Left => el.left_0().h_full().w(px(THICKNESS)),
        Side::Right => el.right_0().h_full().w(px(THICKNESS)),
        Side::Top => el.left_0().w_full().h(px(THICKNESS)),
    }
    .cursor(if side == Side::Top {
        gpui::CursorStyle::ResizeUpDown
    } else {
        gpui::CursorStyle::ResizeLeftRight
    })
}

/// The first side's share when the edge between two sides is `along` px
/// into `size` px of room, each side keeping `keep` px; `None` in a room
/// too small to share, where the edge stays where it was.
pub fn share(along: f32, size: f32, keep: f32) -> Option<f32> {
    if !size.is_finite() || size <= 2. * keep || !along.is_finite() {
        return None;
    }
    Some(along.clamp(keep, size - keep) / size)
}

/// What an edge being dragged shows under the pointer: nothing.
struct Nothing;

impl gpui::Render for Nothing {
    fn render(&mut self, _: &mut gpui::Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

#[cfg(test)]
mod tests {
    use super::share;

    #[test]
    fn the_edge_follows_the_pointer_and_leaves_each_side_its_room() {
        assert_eq!(share(300., 1000., 160.), Some(0.3));
        assert_eq!(share(50., 1000., 160.), Some(0.16));
        assert_eq!(share(990., 1000., 160.), Some(0.84));
        assert_eq!(share(-50., 1000., 240.), Some(0.24));
        // Too small to share: the edge stays where it was.
        assert_eq!(share(100., 300., 160.), None);
        assert_eq!(share(f32::NAN, 1000., 160.), None);
    }
}
