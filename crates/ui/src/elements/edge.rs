//! The edge the user drags to size an area: the sidebar's, the preview's,
//! the split's, the archive's view's, the dock's, the terminal's, a list
//! column's, a table preview's column's. One element for all: a strip over the area's border, the
//! resize cursor, the handle's color under the pointer, a double click
//! back to the size the area ships with, and nothing following the
//! pointer while dragged (the edge itself follows); an edge between two
//! areas that share a room goes where they ship once the pointer brings it
//! near (`share`, `SNAP`). What a drag means is
//! the payload's (`T`): the area's owner listens for its `DragMoveEvent`
//! where the room it shares is (the panes' container for the split, the
//! window's root for the rest), and the root's mouse-up remembers where
//! the edge was left (`Shell::resize_hook`).

use gpui::{Context, Div, ElementId, Stateful, div, prelude::*, px};

use crate::shell::Shell;
use crate::theme::Theme;

/// How thick an edge is to the pointer.
pub const THICKNESS: f32 = 6.;

/// How near, in px, the pointer brings an edge between two areas to where
/// they ship before the edge goes there: a magnet, so the even split is
/// found again by hand. Pointer geometry, as `THICKNESS`, not text: it
/// keeps its size whatever the text's.
pub const SNAP: f32 = 12.;

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
    let el = dragged(div().id(id).absolute().top_0(), drag, reset, cx);
    match side {
        Side::Left => el.left_0().h_full().w(px(THICKNESS)),
        Side::Right => el.right_0().h_full().w(px(THICKNESS)),
        Side::Top => el.left_0().w_full().h(px(THICKNESS)),
    }
    .cursor(cursor(side))
}

/// The edge between two cards (`Theme::cards`, ADR-123): the gutter
/// itself, a room of its own between them (`frame::GUTTER` across, as long
/// as they are) rather than a strip over a border, which their rounded
/// corners would cut; the card on `side` of it is the one it sizes.
pub fn gutter<T: 'static>(
    id: impl Into<ElementId>,
    side: Side,
    drag: T,
    reset: impl Fn(&mut Shell) + 'static,
    cx: &mut Context<Shell>,
) -> Stateful<Div> {
    let across = px(crate::elements::frame::GUTTER);
    let el = dragged(div().id(id).flex_none().rounded_full(), drag, reset, cx);
    match side {
        Side::Left | Side::Right => el.w(across).h_full(),
        Side::Top => el.h(across).w_full(),
    }
    .cursor(cursor(side))
}

fn cursor(side: Side) -> gpui::CursorStyle {
    if side == Side::Top {
        gpui::CursorStyle::ResizeUpDown
    } else {
        gpui::CursorStyle::ResizeLeftRight
    }
}

/// `el` as an edge: the handle's color under the pointer, the press its
/// own, `drag` while dragged, `reset` on a double click.
fn dragged<T: 'static>(
    el: Stateful<Div>,
    drag: T,
    reset: impl Fn(&mut Shell) + 'static,
    cx: &mut Context<Shell>,
) -> Stateful<Div> {
    let hover = cx.global::<Theme>().handle;
    el.hover(move |style| style.bg(hover))
        // The press is the edge's: nothing under it starts a band or
        // selects. A drag starts clean, measured from where it was pressed
        // (`Shell::edge_pressed`): a release missed outside the window
        // cannot leave another drag's start behind.
        .on_mouse_down(
            gpui::MouseButton::Left,
            cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                this.edge_start = None;
                this.edge_pressed = f32::from(event.position.x);
                cx.stop_propagation();
            }),
        )
        .on_click(cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
            if event.click_count() == 2 {
                reset(this);
                this.remember_session();
                cx.notify();
            }
        }))
        .on_drag(drag, |_, _, _, cx| cx.new(|_| Nothing))
}

/// An area's share of the room it splits with another, one for each way
/// their edge runs: dragged one way, it says nothing of the other.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Share {
    /// Above the other.
    pub above: f32,
    /// At its left.
    pub beside: f32,
}

impl Share {
    /// The same share both ways: as the area ships, and where a double
    /// click on its edge puts it back.
    pub const fn even(share: f32) -> Self {
        Self {
            above: share,
            beside: share,
        }
    }

    pub fn get(self, beside: bool) -> f32 {
        if beside { self.beside } else { self.above }
    }

    pub fn set(&mut self, beside: bool, share: f32) {
        *if beside {
            &mut self.beside
        } else {
            &mut self.above
        } = share;
    }
}

/// The first side's share when the edge between two sides is `along` px
/// into `size` px of room, each side keeping `keep` px: `ships`, the share
/// they ship with (a double click's), while the pointer is within `SNAP`
/// px of it and it leaves each side its room. `None` in a room too small
/// to share, where the edge stays where it was.
pub fn share(along: f32, size: f32, keep: f32, ships: f32) -> Option<f32> {
    if !size.is_finite() || size <= 2. * keep || !along.is_finite() {
        return None;
    }
    let shipped = ships * size;
    if (along - shipped).abs() <= SNAP && (keep..=size - keep).contains(&shipped) {
        return Some(ships);
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
        assert_eq!(share(300., 1000., 160., 0.5), Some(0.3));
        assert_eq!(share(50., 1000., 160., 0.5), Some(0.16));
        assert_eq!(share(990., 1000., 160., 0.5), Some(0.84));
        assert_eq!(share(-50., 1000., 240., 0.5), Some(0.24));
        // Too small to share: the edge stays where it was.
        assert_eq!(share(100., 300., 160., 0.5), None);
        assert_eq!(share(f32::NAN, 1000., 160., 0.5), None);
    }

    #[test]
    fn the_edge_goes_where_it_ships_once_brought_near() {
        // Within SNAP px of the even split, either way, it is even.
        assert_eq!(share(500., 1000., 160., 0.5), Some(0.5));
        assert_eq!(share(509., 1000., 160., 0.5), Some(0.5));
        assert_eq!(share(488., 1000., 160., 0.5), Some(0.5));
        // Past it, the edge follows the pointer again.
        assert_eq!(share(487., 1000., 160., 0.5), Some(0.487));
        assert_eq!(share(513., 1000., 160., 0.5), Some(0.513));
        // Where it ships need not be the middle.
        assert_eq!(share(405., 1000., 160., 0.4), Some(0.4));
        // Where it ships would leave a side less than its room: no magnet
        // there, the edge kept at the side's room.
        assert_eq!(share(95., 300., 100., 0.3), Some(100. / 300.));
    }
}
