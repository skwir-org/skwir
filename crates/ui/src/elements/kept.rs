//! What the keys are on keeps its place as a view changes size
//! (`docs/invariants/views.md`, "Scrolling"): a split opened or closed, a
//! panel beside or under the files, the window resized. The cursor, or,
//! where it is not in view, the first entry selected that is, stays as far
//! down the view as it was, wholly in view; a view scrolled away from both
//! keeps what was at its top, as before.
//!
//! Kept so, a view comes back as it was once its size does: a split opened
//! and closed again gives back the view it had, as dragging a window's edge
//! out and back does. Where the item had to be moved to be wholly in view
//! (near an edge), how far into the view it was meant to be is remembered
//! (`Memory`), for as long as the view stays where it was put.
//!
//! The arithmetic is along one axis, the same for every view: an item's
//! extent in the content, where the view starts in it, how long the view
//! is, and what a heading pinned at its start covers. The grid decides in
//! its element's prepaint (`uniform_grid`), where it lays its rows out
//! anew; a `uniform_list` is told by `rows`, drawn before it, and a row
//! scrolled sideways by `across`. A scroll asked in the same frame (a key's)
//! runs after it, and wins.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    AnyElement, App, IntoElement, ScrollHandle, Styled, UniformListScrollHandle, Window, point, px,
};

/// What the layout may round away between two frames of one size.
const SAME: f32 = 0.5;

/// Whether a view `before` long the last frame (none drawn: 0) is another
/// length now.
pub fn resized(before: f32, now: f32) -> bool {
    before > 0. && now > 0. && (before - now).abs() > SAME
}

/// How far into the view the item `at..at + len` is: its middle, as a
/// share of the room the view gives it (`view` long from `from`, its first
/// `pinned` under a heading). `None` unless it is wholly in view.
pub fn seen(at: f32, len: f32, from: f32, view: f32, pinned: f32) -> Option<f32> {
    let room = view - pinned;
    let into = at - from - pinned;
    (room > 0. && into >= -SAME && into + len <= room + SAME).then(|| (into + len / 2.) / room)
}

/// Where a view `view` long starts for the item `at..at + len` to be as far
/// into it as `share` says (`seen`), wholly in it where it fits, else its
/// start shown. The caller clamps it to the content.
pub fn start(at: f32, len: f32, share: f32, view: f32, pinned: f32) -> f32 {
    let room = view - pinned;
    let into = (share * room - len / 2.).min(room - len).max(0.);
    at - pinned - into
}

/// Where a view put what it kept the last time it changed size: the item,
/// how far into the view it was meant to be (`seen`), and where the view
/// started once put.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Placed {
    item: usize,
    share: f32,
    from: f32,
}

/// What a view placed last, held from frame to frame where its scroll is.
#[derive(Clone, Debug, Default)]
pub struct Memory(Rc<Cell<Option<Placed>>>);

impl Memory {
    /// The first of `items` wholly in view (`seen`), each laid out where
    /// `extent` says, with how far into the view it keeps: as far as this
    /// last meant it to be while the view starts where that left it, else
    /// as far as it is.
    pub fn keep(
        &self,
        items: &[usize],
        extent: impl Fn(usize) -> Option<(f32, f32)>,
        from: f32,
        view: f32,
        pinned: f32,
    ) -> Option<(usize, f32)> {
        let (item, share) = items.iter().find_map(|&item| {
            let (at, len) = extent(item)?;
            seen(at, len, from, view, pinned).map(|share| (item, share))
        })?;
        Some(match self.0.get() {
            Some(placed) if placed.item == item && (placed.from - from).abs() <= SAME => {
                (item, placed.share)
            }
            _ => (item, share),
        })
    }

    /// `item` put `share` into a view that now starts at `from`.
    pub fn put(&self, item: usize, share: f32, from: f32) {
        self.0.set(Some(Placed { item, share, from }));
    }
}

/// Drawn before a `uniform_list` of `count` rows `height` tall, over the
/// list's bounds: as the list's height changes, the first of `rows` (the
/// cursor's, then those selected, in the list's rows) wholly in view stays
/// as far down it as it was, a scroll the list carries out in this same
/// frame. The list's handle holds what it was the last frame: its bounds
/// and offset. `rows` is asked only then, from what the list drew the last
/// frame; `memory` is the list's, kept by its view.
pub fn rows(
    scroll: UniformListScrollHandle,
    memory: Memory,
    (count, height, pinned): (usize, f32, f32),
    rows: impl FnOnce(&mut Window, &mut App) -> Vec<usize> + 'static,
) -> AnyElement {
    gpui::canvas(
        move |bounds, window, cx| {
            let base = scroll.0.borrow().base_handle.clone();
            let before = f32::from(base.bounds().size.height);
            let now = f32::from(bounds.size.height);
            if height <= 0. || !resized(before, now) {
                return;
            }
            let rows = rows(window, cx);
            let offset = base.offset();
            let from = -f32::from(offset.y);
            let extent = |row: usize| Some((row as f32 * height, height));
            let Some((row, share)) = memory.keep(&rows, extent, from, before, pinned) else {
                return;
            };
            let most = (count as f32 * height - now).max(0.);
            let start = start(row as f32 * height, height, share, now, pinned).clamp(0., most);
            base.set_offset(point(offset.x, px(-start)));
            memory.put(row, share, start);
        },
        |_, (), _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
    .into_any_element()
}

/// Drawn before a row of children side by side that scrolls sideways
/// (`row`: a board's columns), over its bounds: as the row changes width,
/// child `child` (the cursor's column), wholly in view, stays as far
/// along it as it was. Its children keep their widths: where the last frame
/// laid them out is where they are. `child` is asked only then.
pub fn across(
    row: ScrollHandle,
    memory: Memory,
    child: impl FnOnce(&mut Window, &mut App) -> Option<usize> + 'static,
) -> AnyElement {
    gpui::canvas(
        move |bounds, window, cx| {
            let before = row.bounds();
            let now = f32::from(bounds.size.width);
            if !resized(f32::from(before.size.width), now) {
                return;
            }
            let Some(child) = child(window, cx) else {
                return;
            };
            let laid = |k: usize| {
                let laid = row.bounds_for_item(k)?;
                Some((
                    f32::from(laid.left() - before.left()),
                    f32::from(laid.size.width),
                ))
            };
            let offset = row.offset();
            let from = -f32::from(offset.x);
            let view = f32::from(before.size.width);
            let Some((child, share)) = memory.keep(&[child], laid, from, view, 0.) else {
                return;
            };
            let Some((at, len)) = laid(child) else {
                return;
            };
            // As wide as its children: a dozen columns at most.
            let wide = (0..)
                .map_while(laid)
                .last()
                .map_or(0., |(at, len)| at + len);
            let start = start(at, len, share, now, 0.).clamp(0., (wide - now).max(0.));
            row.set_offset(point(px(-start), offset.y));
            memory.put(child, share, start);
        },
        |_, (), _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
    .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{Memory, resized, seen, start};

    #[test]
    fn what_is_in_view_stays_as_far_down_it() {
        // A row 20 tall, its middle 610 down a view 800 tall from 0:
        // halved, its middle is 305 down, and 610 again once the view is
        // 800 again.
        let share = seen(600., 20., 0., 800., 0.).unwrap();
        let from = start(600., 20., share, 400., 0.);
        assert!((610. - from - 305.).abs() < 1e-3, "{from}");
        let share = seen(600., 20., from, 400., 0.).unwrap();
        assert!(start(600., 20., share, 800., 0.).abs() < 1e-3);
    }

    #[test]
    fn only_what_is_wholly_in_view_is_kept() {
        // Cut by the bottom, cut by the top, under the pinned heading.
        assert!(seen(790., 20., 0., 800., 0.).is_none());
        assert!(seen(90., 20., 100., 800., 0.).is_none());
        assert!(seen(110., 20., 100., 800., 24.).is_none());
        assert!(seen(130., 20., 100., 800., 24.).is_some());
    }

    #[test]
    fn what_is_kept_stays_wholly_in_view() {
        // At the bottom of a view that shrinks, it stays its whole height
        // above the bottom; a view shorter than it shows its start.
        let share = seen(780., 20., 0., 800., 0.).unwrap();
        let from = start(780., 20., share, 300., 0.);
        assert!(780. + 20. - from <= 300. + 1e-3, "{from}");
        assert!((start(780., 20., share, 10., 0.) - 780.).abs() < 1e-3);
        // Under a heading 24 tall, never under it.
        let share = seen(124., 20., 100., 800., 24.).unwrap();
        assert!(start(124., 20., share, 400., 24.) <= 124. - 24. + 1e-3);
    }

    #[test]
    fn a_size_is_changed_only_once_drawn_and_beyond_rounding() {
        assert!(!resized(0., 400.));
        assert!(!resized(400., 400.3));
        assert!(resized(800., 400.));
    }

    #[test]
    fn what_was_moved_to_be_wholly_in_view_goes_back_where_it_was() {
        // A row 20 tall at the bottom of a view 800 tall, halved: moved up
        // to be wholly in view, its middle no longer where it was.
        let memory = Memory::default();
        let extent = |_| Some((780., 20.));
        let (_, share) = memory.keep(&[0], extent, 0., 800., 0.).unwrap();
        let from = start(780., 20., share, 400., 0.);
        assert!((780. + 20. - from - 400.).abs() < 1e-3, "{from}");
        // As far as it is now, back to 800 it would not be where it was.
        let (_, there) = Memory::default()
            .keep(&[0], extent, from, 400., 0.)
            .unwrap();
        assert!(start(780., 20., there, 800., 0.) > 1.);
        // Remembered, while the view stayed: where it was.
        memory.put(0, share, from);
        let (_, again) = memory.keep(&[0], extent, from, 400., 0.).unwrap();
        assert!(start(780., 20., again, 800., 0.).abs() < 1e-3);
        // Scrolled meanwhile, it is as far as it is.
        let (_, moved) = memory.keep(&[0], extent, from + 10., 400., 0.).unwrap();
        assert!((moved - again).abs() > 0.01);
    }
}
