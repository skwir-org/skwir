//! A drag that moves what a view shows, as a hand moves a sheet (ADR-143).
//!
//! A view that scrolls both ways (a picture zoomed in, a document's pages)
//! lays a [`layer`] under what it shows, as wide as its room. A press of the
//! left button that nothing above it took (the picture, the room around and
//! between pages), or of the middle button anywhere, holds what is shown:
//! each move of the pointer then moves it as far, the point grabbed staying
//! under the pointer, until the button is let go. A page takes its left
//! presses itself: one on a character of its text selects ([`Grab::select`]),
//! unless the user chose the drag for it too; anywhere else on it (a margin,
//! a picture, a page with no text) it holds the [`Grab`] itself
//! ([`Grab::hold`]) and tells a click from a drag by it ([`Grab::dragged`]).
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

/// Whether the pointer at `at` has gone past a click's reach of a press at
/// `from`: the one definition of a drag, for what moves, a selection's
/// release and a double click alike.
fn past_click(from: Point<Pixels>, at: Point<Pixels>) -> bool {
    let gone = at - from;
    f32::from(gone.x).hypot(f32::from(gone.y)) > CLICK_SLOP
}

/// Whether `event` is a double click, not the release of a drag: gpui
/// tells a click at any release over what was pressed, however far the
/// pointer went, and counts a second press near the first as a double one,
/// whatever the first did, so two quick drags from about the same place,
/// one that wandered and came back, or a flick and a click where it began
/// would be one. Both presses must have been clicks, as `grab` heard them
/// (a press the layer did not hear is none of its). A key's or a tap's
/// click has no drag to tell.
pub fn double_click(event: &gpui::ClickEvent, grab: &Grab) -> bool {
    event.click_count() == 2
        && match event {
            gpui::ClickEvent::Mouse(click) => grab.clicked_twice(click.down.position),
            gpui::ClickEvent::Keyboard(_) | gpui::ClickEvent::Touch(_) => true,
        }
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
    /// The last two presses heard, the newest last, held or not (one where
    /// nothing moves is heard too): what a double click asks.
    heard: Cell<[Option<Heard>; 2]>,
}

/// A press heard: where it was made, with which button, and whether its
/// pointer then went past a click's reach while it was held.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Heard {
    button: MouseButton,
    from: Point<Pixels>,
    dragged: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Held {
    button: MouseButton,
    by: By,
    /// Where the press was.
    from: Point<Pixels>,
    /// Where the pointer was when what is shown last moved.
    last: Point<Pixels>,
    /// Gone past `CLICK_SLOP` from the press: a drag, never a click.
    dragged: bool,
}

/// Who took the press the grab holds.
#[derive(Clone, Copy, Debug, PartialEq)]
enum By {
    /// The layer: a press around and between what it moves, or the middle
    /// button's anywhere.
    Layer,
    /// What is drawn over the layer and takes its left press itself, where
    /// its drag moves what is shown (a page, [`Grab::hold`]).
    Over,
    /// What is drawn over the layer selecting its text with the press
    /// ([`Grab::select`]): nothing moves.
    Selecting,
}

impl Grab {
    fn press(&self, at: Point<Pixels>, button: MouseButton, by: By) {
        self.0.held.set(Some(Held {
            button,
            by,
            from: at,
            last: at,
            dragged: false,
        }));
        self.heard(at, button);
    }

    /// A press at `at` heard, whoever takes it.
    fn heard(&self, at: Point<Pixels>, button: MouseButton) {
        let [_, last] = self.0.heard.get();
        self.0.heard.set([
            last,
            Some(Heard {
                button,
                from: at,
                dragged: false,
            }),
        ]);
    }

    /// The pointer at `at`, `pressed` held: the press heard last is a drag
    /// once it goes past a click's reach.
    fn went(&self, at: Point<Pixels>, pressed: Option<MouseButton>) {
        let [before, last] = self.0.heard.get();
        if let Some(mut last) = last
            && !last.dragged
            && pressed == Some(last.button)
            && past_click(last.from, at)
        {
            last.dragged = true;
            self.0.heard.set([before, Some(last)]);
        }
    }

    /// Whether the press at `at` and the one heard before it were both
    /// clicks: the press at `at` must be the one heard last.
    fn clicked_twice(&self, at: Point<Pixels>) -> bool {
        let [before, last] = self.0.heard.get();
        last.is_some_and(|last| last.from == at && !last.dragged)
            && before.is_none_or(|before| !before.dragged)
    }

    /// Hold what is shown from `at` with the left button: what is drawn
    /// over the layer and takes the press itself, where its drag moves the
    /// view (a page). It holds the grab even where nothing can move, to tell
    /// its click by it ([`Grab::dragged`]).
    pub fn hold(&self, at: Point<Pixels>) {
        self.press(at, MouseButton::Left, By::Over);
    }

    /// The left press at `at` selects what is drawn over the layer (a
    /// page's text): nothing moves, and the layer shows no hand, until it is
    /// let go.
    pub fn select(&self, at: Point<Pixels>) {
        self.press(at, MouseButton::Left, By::Selecting);
    }

    /// Whether a press that what is drawn over the layer took ([`hold`],
    /// [`select`]) is still held. The layer, never virtual, hears every
    /// release; a page in a virtual list does not once it has left it (the
    /// pages moved under the press, the wheel took them away): what such a
    /// page hears without its press held is left over, never its click.
    ///
    /// [`hold`]: Grab::hold
    /// [`select`]: Grab::select
    pub fn held_over(&self) -> bool {
        self.0.held.get().is_some_and(|held| held.by != By::Layer)
    }

    /// Whether the press held has gone far enough to be a drag, moving what
    /// is shown or selecting: its release is no click (a link under it is
    /// not followed).
    pub fn dragged(&self) -> bool {
        self.0.held.get().is_some_and(|held| held.dragged)
    }

    /// Whether a press holds something that can move: its hand is closed,
    /// and what is drawn over the layer leaves the pointer to it.
    pub fn grabbing(&self) -> bool {
        self.0
            .held
            .get()
            .is_some_and(|held| held.by != By::Selecting)
            && self.movable()
    }

    /// Whether what is drawn over the layer selects with the press held: no
    /// hand shows, as the drag moves nothing.
    pub fn selecting(&self) -> bool {
        self.0
            .held
            .get()
            .is_some_and(|held| held.by == By::Selecting)
    }

    /// Whether what the layer moves is larger than its room, as last
    /// painted: what is drawn over it (painted after it) shows the open
    /// hand where its drag moves the view.
    pub fn movable(&self) -> bool {
        self.0.movable.get()
    }

    /// Let the press go: whether the cursor it showed (a hand closed, a
    /// selection's) must be drawn again.
    fn let_go(&self) -> bool {
        let showed = self.grabbing() || self.selecting();
        self.0.held.set(None);
        showed
    }

    /// The pointer at `at` with the press held: how far what is shown moves.
    /// Nothing until the pointer leaves a click's reach, then the whole way
    /// from the press, so the point grabbed lands under the pointer; never
    /// anything while it selects, which is told a drag all the same, so
    /// that every press over the layer has one click (`dragged`). Each move
    /// is taken once: a guard, as no two layers share a grab (the panel
    /// behind peek or the whole screen has none).
    fn follow(&self, at: Point<Pixels>) -> Option<Point<Pixels>> {
        let mut held = self.0.held.get()?;
        self.went(at, Some(held.button));
        if !held.dragged {
            if !past_click(held.from, at) {
                return None;
            }
            held.dragged = true;
            self.0.held.set(Some(held));
        }
        if held.by == By::Selecting {
            return None;
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

/// `offset` moved `delta`, held within `most` as gpui holds a box's offset
/// as it lays it out (`Interactivity::clamp_scroll_position`): from 0 down
/// to `-most`, but sideways from 0 up to `most` where the box is laid out
/// right to left.
fn within(
    offset: Point<Pixels>,
    delta: Point<Pixels>,
    most: Point<Pixels>,
    rtl: bool,
) -> Point<Pixels> {
    let (x, y) = (f32::from(offset.x + delta.x), f32::from(offset.y + delta.y));
    let (across, down) = (f32::from(most.x).max(0.), f32::from(most.y).max(0.));
    let x = if rtl {
        x.clamp(0., across)
    } else {
        x.clamp(-across, 0.)
    };
    point(px(x), px(y.clamp(-down, 0.)))
}

/// How far down a list is scrolled once moved `distance` from `top`, held
/// within `most`, as the list holds it at its next layout.
fn down_within(top: Pixels, distance: Pixels, most: Pixels) -> Pixels {
    px(f32::from(top + distance).clamp(0., f32::from(most).max(0.)))
}

impl Moves {
    /// Move what is shown `delta` along with the pointer, held within its
    /// bounds as gpui will hold it when it lays it out: whether it moved,
    /// so a drag against an edge, or over what fits, draws nothing. (Two
    /// offsets are told apart in numbers: `Pixels`' `==` is its `f32`'s,
    /// -0 and 0 one, though it orders -0 below 0.) Sideways, in the
    /// direction the box was last laid out in, as its handle says: its own,
    /// whatever the layer's.
    fn by(&self, delta: Point<Pixels>) -> bool {
        match self {
            Moves::Offset(handle) => {
                let (was, most) = (handle.offset(), handle.max_offset());
                let rtl = handle.direction().is_rtl();
                let to = within(was, delta, most, rtl);
                let moved = to != was;
                if moved {
                    handle.set_offset(to);
                }
                moved
            }
            Moves::Column { list, across } => {
                let top = -list.scroll_px_offset_for_scrollbar().y;
                let to = down_within(top, -delta.y, list.max_offset_for_scrollbar().y);
                let down = to != top;
                if down {
                    list.scroll_by(to - top);
                }
                let (was, most) = (across.offset(), across.max_offset());
                let rtl = across.direction().is_rtl();
                let sideways = within(was, point(delta.x, px(0.)), point(most.x, px(0.)), rtl);
                let aside = sideways != was;
                if aside {
                    across.set_offset(sideways);
                }
                down || aside
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
/// film, whose click is the player's); the middle button always does. It
/// takes a press only where what it moves can move, and draws again only
/// for a hand to show or what moved.
pub fn layer(grab: &Grab, moves: Moves, left: bool) -> impl IntoElement {
    let grab = grab.clone();
    gpui::canvas(
        |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
        move |_, hitbox, window, _| {
            let movable = moves.movable();
            grab.0.movable.set(movable);
            // A selection's I-beam goes with its drag past the pages' edges,
            // around and between them.
            if grab.grabbing() {
                window.set_cursor_style(CursorStyle::ClosedHand, &hitbox);
            } else if grab.selecting() {
                window.set_cursor_style(CursorStyle::IBeam, &hitbox);
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
                // it too: nothing here stops it. One it does not take is
                // heard all the same, for the double click it may be half
                // of.
                if phase.bubble() && hit.is_hovered(window) {
                    if takes && pressed.movable() {
                        pressed.press(event.position, event.button, By::Layer);
                        // The hand closes.
                        window.refresh();
                    } else {
                        pressed.heard(event.position, event.button);
                    }
                }
            });
            let followed = grab.clone();
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, _| {
                if !phase.bubble() {
                    return;
                }
                let Some(held) = followed.0.held.get() else {
                    // A press the layer did not take still tells a drag.
                    followed.went(event.position, event.pressed_button);
                    return;
                };
                // A release outside the window is never told here.
                if event.pressed_button != Some(held.button) {
                    if followed.let_go() {
                        window.refresh();
                    }
                    return;
                }
                if let Some(delta) = followed.follow(event.position)
                    && moves.by(delta)
                {
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
                    && released.let_go()
                {
                    // The hand opens, or the selection's cursor goes.
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
        grab.press(at(100., 100.), MouseButton::Left, By::Layer);
        assert_eq!(grab.follow(at(102., 103.)), None);
        assert!(!grab.dragged(), "a click's tremble is no drag");
    }

    #[test]
    fn past_a_click_the_point_grabbed_follows_the_pointer() {
        let grab = Grab::default();
        grab.press(at(100., 100.), MouseButton::Left, By::Layer);
        // The first move past the slop goes the whole way from the press.
        assert_eq!(grab.follow(at(110., 100.)), Some(at(10., 0.)));
        assert!(grab.dragged());
        // Then each move as far as the pointer, back within the slop too.
        assert_eq!(grab.follow(at(108., 95.)), Some(at(-2., -5.)));
        assert_eq!(grab.follow(at(101., 101.)), Some(at(-7., 6.)));
        assert!(grab.dragged(), "a drag back to the press is still a drag");
    }

    #[test]
    fn a_move_is_taken_once() {
        let grab = Grab::default();
        grab.press(at(0., 0.), MouseButton::Middle, By::Layer);
        assert_eq!(grab.follow(at(0., 20.)), Some(at(0., 20.)));
        // A guard: no two layers share a grab.
        assert_eq!(grab.follow(at(0., 20.)), None);
    }

    #[test]
    fn a_selection_moves_nothing_and_shows_no_hand() {
        let grab = Grab::default();
        grab.0.movable.set(true);
        grab.select(at(0., 0.));
        assert_eq!(grab.follow(at(2., 2.)), None, "a click's tremble");
        assert!(!grab.dragged());
        assert_eq!(grab.follow(at(40., 40.)), None, "a selection's drag");
        assert!(grab.selecting() && !grab.grabbing());
        assert!(grab.dragged(), "its release is no click");
        assert!(grab.let_go(), "its cursor goes with it");
        assert!(!grab.selecting());
    }

    #[test]
    fn a_page_press_is_its_own_while_held() {
        let grab = Grab::default();
        grab.hold(at(0., 0.));
        assert!(grab.held_over());
        // Nothing to move: the page still tells its click by the grab, and
        // nothing is drawn again for it.
        assert_eq!(grab.follow(at(30., 0.)), Some(at(30., 0.)));
        assert!(grab.dragged() && !grab.grabbing());
        assert!(!grab.let_go());
        // Let go where the page did not hear it: what it hears next is
        // another press's, the layer's or none.
        assert!(!grab.held_over());
        grab.press(at(0., 0.), MouseButton::Left, By::Layer);
        assert!(!grab.held_over());
        grab.select(at(0., 0.));
        assert!(grab.held_over());
    }

    /// The holds gpui's own tests find for a box laid out either way
    /// (vendored gpui's `scroll_axis_tests::a_strip_left_to_right_scrolls_as_it_did`
    /// and `a_strip_right_to_left_starts_at_its_right`: 0 to -300, 0 to
    /// 300), as `within` holds a move, in the direction the handle says
    /// (`ScrollHandle::direction`, patch 0088).
    #[test]
    fn a_move_is_held_where_gpui_holds_the_offset() {
        let most = at(100., 50.);
        // Within: as far as the pointer.
        assert_eq!(
            within(at(-10., -10.), at(-5., 5.), most, false),
            at(-15., -5.)
        );
        // Past an edge: no further, both ways.
        assert_eq!(
            within(at(-95., -2.), at(-20., 10.), most, false),
            at(-100., 0.)
        );
        assert_eq!(within(at(0., 0.), at(30., 30.), most, false), at(0., 0.));
        // Right to left, sideways runs from 0 up.
        assert_eq!(within(at(10., 0.), at(20., 0.), most, true), at(30., 0.));
        let strip = at(300., 0.);
        assert_eq!(
            within(at(0., 0.), at(-400., 0.), strip, false),
            at(-300., 0.)
        );
        assert_eq!(within(at(0., 0.), at(400., 0.), strip, true), strip);
        assert_eq!(within(at(40., 0.), at(-60., 0.), strip, true), at(0., 0.));
        assert_eq!(within(at(10., 0.), at(-20., 0.), most, true), at(0., 0.));
        // What fits moves nowhere.
        assert_eq!(
            within(at(0., 0.), at(7., -7.), at(0., 0.), false),
            at(0., 0.)
        );
        assert_eq!(at(-0., 0.), at(0., -0.), "-0 is 0 to `==`");
        // A list: from its top down to the most it goes.
        assert_eq!(down_within(px(10.), px(-30.), px(500.)), px(0.));
        assert_eq!(down_within(px(490.), px(30.), px(500.)), px(500.));
        assert_eq!(down_within(px(100.), px(30.), px(500.)), px(130.));
    }

    /// A double click is two presses the grab heard as clicks, by the
    /// path their pointer took, not by where they were let go: a drag that
    /// came back, a flick before a click, a press nothing moved (heard, not
    /// taken) dragged away are none.
    #[test]
    fn a_double_click_is_two_clicks_heard() {
        let twice = |at: Point<Pixels>, count: usize| {
            gpui::ClickEvent::Mouse(gpui::MouseClickEvent {
                down: MouseDownEvent {
                    button: MouseButton::Left,
                    position: at,
                    click_count: count,
                    ..MouseDownEvent::default()
                },
                up: MouseUpEvent {
                    button: MouseButton::Left,
                    position: at,
                    click_count: count,
                    ..MouseUpEvent::default()
                },
            })
        };
        let left = Some(MouseButton::Left);
        let grab = Grab::default();
        grab.heard(at(10., 10.), MouseButton::Left);
        grab.went(at(12., 11.), left);
        grab.heard(at(11., 12.), MouseButton::Left);
        assert!(double_click(&twice(at(11., 12.), 2), &grab));
        assert!(!double_click(&twice(at(11., 12.), 1), &grab), "one click");
        assert!(
            !double_click(&twice(at(50., 50.), 2), &grab),
            "a press the grab did not hear"
        );
        // The second press went away and came back before its release.
        grab.heard(at(10., 10.), MouseButton::Left);
        grab.went(at(40., 10.), left);
        grab.went(at(11., 10.), left);
        assert!(!double_click(&twice(at(10., 10.), 2), &grab));
        // A flick, then a still click where it began.
        grab.press(at(10., 10.), MouseButton::Left, By::Layer);
        assert_eq!(grab.follow(at(60., 10.)), Some(at(50., 0.)));
        grab.let_go();
        grab.heard(at(10., 10.), MouseButton::Left);
        assert!(!double_click(&twice(at(10., 10.), 2), &grab));
        // Another button held moves no press of the left one.
        grab.heard(at(10., 10.), MouseButton::Left);
        grab.went(at(60., 10.), Some(MouseButton::Middle));
        grab.heard(at(10., 10.), MouseButton::Left);
        assert!(double_click(&twice(at(10., 10.), 2), &grab));
    }

    #[test]
    fn nothing_held_moves_nothing() {
        let grab = Grab::default();
        assert_eq!(grab.follow(at(50., 50.)), None);
        grab.press(at(0., 0.), MouseButton::Left, By::Layer);
        grab.let_go();
        assert_eq!(grab.follow(at(50., 50.)), None);
        assert!(!grab.dragged() && !grab.grabbing());
    }
}
