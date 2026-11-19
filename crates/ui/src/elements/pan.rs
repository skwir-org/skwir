//! A drag that moves what a view shows, as a hand moves a sheet (ADR-143).
//!
//! A view that scrolls both ways (a picture zoomed in, a document's pages)
//! lays a [`layer`] under what it shows, as wide as its room. A press of the
//! left button that nothing above it took (the picture, the room around and
//! between pages, a page with no text), or of the middle button anywhere,
//! holds what is shown: each move of the pointer then moves it as far, the
//! point grabbed staying under the pointer, until the button is let go.
//! What has something to select (a page's text) takes the press itself and
//! selects, unless the user chose the drag for it too: then it holds the
//! [`Grab`] itself ([`Grab::press`]) and tells a click from a drag by it
//! ([`Grab::dragged`]).
//!
//! It moves the handles the arrows and the scrollbars move, so a drag goes
//! no further than they do. It never throws what it moves on after the
//! pointer stops: that would draw a frame after frame (ADR-000).

use gpui::{
    CursorStyle, HitboxBehavior, ListState, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, ScrollHandle, point, prelude::*, px,
};
use std::{cell::Cell, rc::Rc};

/// A press that moved less than this is a click, not a drag.
pub const CLICK_SLOP: f32 = 4.;

/// Whether `event` is a double click, not the release of a drag: gpui
/// tells a click at any release over what was pressed, however far the
/// pointer went, and counts a second press near the first as a double one,
/// so two quick drags from about the same place would be one. A key's or a
/// tap's click has no drag to tell.
pub fn double_click(event: &gpui::ClickEvent) -> bool {
    let stayed = match event {
        gpui::ClickEvent::Mouse(click) => {
            let gone = click.up.position - click.down.position;
            f32::from(gone.x).hypot(f32::from(gone.y)) <= CLICK_SLOP
        }
        gpui::ClickEvent::Keyboard(_) | gpui::ClickEvent::Touch(_) => true,
    };
    stayed && event.click_count() == 2
}

/// A press holding what a view shows, kept between frames by whoever shows
/// it, shared with what is drawn over the layer.
#[derive(Clone, Default)]
pub struct Grab(Rc<Shared>);

#[derive(Default)]
struct Shared {
    held: Cell<Option<Held>>,
    /// What the layer moves is larger than its room, as last painted: a
    /// hand shows only where it can move something.
    movable: Cell<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Held {
    button: MouseButton,
    /// Where the press was.
    from: Point<Pixels>,
    /// Where the pointer was when what is shown last moved.
    last: Point<Pixels>,
    /// Gone past `CLICK_SLOP` from the press: a drag, never a click.
    dragged: bool,
}

impl Grab {
    /// Hold what is shown from `at`, with `button`: what is drawn over the
    /// layer and takes the press itself (a page whose drag moves it).
    pub fn press(&self, at: Point<Pixels>, button: MouseButton) {
        self.0.held.set(Some(Held {
            button,
            from: at,
            last: at,
            dragged: false,
        }));
    }

    /// Whether the press held has gone far enough to be a drag: its release
    /// is no click (a link under it is not followed).
    pub fn dragged(&self) -> bool {
        self.0.held.get().is_some_and(|held| held.dragged)
    }

    /// Whether a press holds something that can move: its hand is closed,
    /// and what is drawn over the layer leaves the pointer to it.
    pub fn grabbing(&self) -> bool {
        self.0.held.get().is_some() && self.movable()
    }

    /// Whether what the layer moves is larger than its room, as last
    /// painted: what is drawn over it (painted after it) shows the open
    /// hand where its drag moves the view.
    pub fn movable(&self) -> bool {
        self.0.movable.get()
    }

    fn let_go(&self) {
        self.0.held.set(None);
    }

    /// The pointer at `at` with the press held: how far what is shown moves.
    /// Nothing until the pointer leaves a click's reach, then the whole way
    /// from the press, so the point grabbed lands under the pointer. Taken
    /// once: a second layer over the same grab (a picture drawn in the panel
    /// and in peek) finds nothing left to move.
    fn follow(&self, at: Point<Pixels>) -> Option<Point<Pixels>> {
        let mut held = self.0.held.get()?;
        if !held.dragged {
            let gone = at - held.from;
            if f32::from(gone.x).hypot(f32::from(gone.y)) <= CLICK_SLOP {
                return None;
            }
            held.dragged = true;
        }
        let delta = at - held.last;
        held.last = at;
        self.0.held.set(Some(held));
        (delta != Point::default()).then_some(delta)
    }
}

/// What a drag moves, by the handles that already scroll it.
#[derive(Clone)]
pub enum Moves {
    /// An `overflow_scroll` box, both ways (a picture, a page at a time).
    Offset(ScrollHandle),
    /// Pages read continuously: the list up and down, the column it lies in
    /// sideways.
    Column {
        list: ListState,
        across: ScrollHandle,
    },
}

impl Moves {
    /// Move what is shown `delta` along with the pointer. gpui holds each
    /// offset within its bounds as it lays the box out, so a drag past an
    /// edge goes no further.
    fn by(&self, delta: Point<Pixels>) {
        match self {
            Moves::Offset(handle) => handle.set_offset(handle.offset() + delta),
            Moves::Column { list, across } => {
                list.scroll_by(-delta.y);
                across.set_offset(point(across.offset().x + delta.x, px(0.)));
            }
        }
    }

    /// Whether what is shown is larger than its room, either way.
    fn movable(&self) -> bool {
        // gpui rounds the most an offset goes to two decimals.
        let some = |most: Pixels| f32::from(most) > 0.5;
        match self {
            Moves::Offset(handle) => {
                let most = handle.max_offset();
                some(most.x) || some(most.y)
            }
            Moves::Column { list, across } => {
                some(list.max_offset_for_scrollbar().y) || some(across.max_offset().x)
            }
        }
    }
}

/// The layer under what a view shows that moves it when dragged, as wide as
/// the view's room: drawn first in it, so what is drawn over it (a page, a
/// scrollbar, a drawer) hears a press before it does and keeps the presses
/// it takes. `left` says whether the left button drags here too (not on a
/// film, whose click is the player's); the middle button always does.
pub fn layer(grab: &Grab, moves: Moves, left: bool) -> impl IntoElement {
    let grab = grab.clone();
    gpui::canvas(
        |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
        move |_, hitbox, window, _| {
            let movable = moves.movable();
            grab.0.movable.set(movable);
            if grab.grabbing() {
                window.set_cursor_style(CursorStyle::ClosedHand, &hitbox);
            } else if left && movable {
                window.set_cursor_style(CursorStyle::OpenHand, &hitbox);
            }
            let (pressed, hit) = (grab.clone(), hitbox.clone());
            window.on_mouse_event(move |event: &MouseDownEvent, phase, window, _| {
                let takes = match event.button {
                    MouseButton::Left => left,
                    MouseButton::Middle => true,
                    _ => false,
                };
                // What is under the press (a double click, the focus) hears
                // it too: nothing here stops it.
                if phase.bubble() && takes && hit.is_hovered(window) {
                    pressed.press(event.position, event.button);
                    window.refresh();
                }
            });
            let followed = grab.clone();
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, _| {
                if !phase.bubble() {
                    return;
                }
                let Some(held) = followed.0.held.get() else {
                    return;
                };
                // A release outside the window is never told here.
                if event.pressed_button != Some(held.button) {
                    followed.let_go();
                    window.refresh();
                    return;
                }
                if let Some(delta) = followed.follow(event.position) {
                    moves.by(delta);
                    window.refresh();
                }
            });
            let released = grab.clone();
            window.on_mouse_event(move |event: &MouseUpEvent, phase, window, _| {
                if phase.bubble()
                    && released
                        .0
                        .held
                        .get()
                        .is_some_and(|held| held.button == event.button)
                {
                    released.let_go();
                    window.refresh();
                }
            });
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f32, y: f32) -> Point<Pixels> {
        point(px(x), px(y))
    }

    #[test]
    fn a_press_moves_nothing_within_a_click() {
        let grab = Grab::default();
        grab.press(at(100., 100.), MouseButton::Left);
        assert_eq!(grab.follow(at(102., 103.)), None);
        assert!(!grab.dragged(), "a click's tremble is no drag");
    }

    #[test]
    fn past_a_click_the_point_grabbed_follows_the_pointer() {
        let grab = Grab::default();
        grab.press(at(100., 100.), MouseButton::Left);
        // The first move past the slop goes the whole way from the press.
        assert_eq!(grab.follow(at(110., 100.)), Some(at(10., 0.)));
        assert!(grab.dragged());
        // Then each move as far as the pointer, back within the slop too.
        assert_eq!(grab.follow(at(108., 95.)), Some(at(-2., -5.)));
        assert_eq!(grab.follow(at(101., 101.)), Some(at(-7., 6.)));
        assert!(grab.dragged(), "a drag back to the press is still a drag");
    }

    #[test]
    fn a_move_is_taken_once_by_every_layer_over_the_grab() {
        let grab = Grab::default();
        grab.press(at(0., 0.), MouseButton::Middle);
        assert_eq!(grab.follow(at(0., 20.)), Some(at(0., 20.)));
        assert_eq!(grab.follow(at(0., 20.)), None, "the second layer's turn");
    }

    #[test]
    fn a_drag_released_is_no_double_click() {
        let click = |from: Point<Pixels>, to: Point<Pixels>| {
            gpui::ClickEvent::Mouse(gpui::MouseClickEvent {
                down: MouseDownEvent {
                    button: MouseButton::Left,
                    position: from,
                    click_count: 2,
                    ..MouseDownEvent::default()
                },
                up: MouseUpEvent {
                    button: MouseButton::Left,
                    position: to,
                    click_count: 2,
                    ..MouseUpEvent::default()
                },
            })
        };
        assert!(double_click(&click(at(10., 10.), at(12., 13.))));
        assert!(
            !double_click(&click(at(10., 10.), at(40., 10.))),
            "the second of two quick drags"
        );
        let once = gpui::ClickEvent::Mouse(gpui::MouseClickEvent {
            down: MouseDownEvent::default(),
            up: MouseUpEvent {
                click_count: 1,
                ..MouseUpEvent::default()
            },
        });
        assert!(!double_click(&once));
    }

    #[test]
    fn nothing_held_moves_nothing() {
        let grab = Grab::default();
        assert_eq!(grab.follow(at(50., 50.)), None);
        grab.press(at(0., 0.), MouseButton::Left);
        grab.let_go();
        assert_eq!(grab.follow(at(50., 50.)), None);
        assert!(!grab.dragged() && !grab.grabbing());
    }
}
