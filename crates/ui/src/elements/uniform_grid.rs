//! A scrollable grid of fixed-size tiles, virtualized by row. Modeled on
//! gpui's `uniform_list`: the column count is computed in `prepaint` from the
//! element's real width, so a reflow never lags a frame behind a resize,
//! split, or panel toggle.
//!
//! Tiles sit in slots. A slot is at least a tile plus the gutter; the width
//! no whole column could use widens the gutters up to a cap, and the rest
//! becomes the margins. Columns are uniform, rows are not: a row is as tall
//! as its tallest tile (a name on three lines beside names on one), so rows
//! of short names give their space back. `math::Slots` is that arithmetic,
//! shared with whatever hit-tests the grid (the rubber band).
//!
//! What `uniform_list` uses that is `pub(crate)` in gpui, and the public
//! substitute here: the shared scroll offset (`Interactivity::scroll_offset`)
//! is reached through `InteractiveElement::track_scroll`, which makes the
//! handle's offset the element's offset; the wheel listeners and offset
//! clamping come with `overflow.y = Scroll` through `Interactivity::paint`.

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

use gpui::{
    AnyElement, App, AvailableSpace, Bounds, ContentMask, Element, ElementId, GlobalElementId,
    Hitbox, InspectorElementId, InteractiveElement, Interactivity, IntoElement, LayoutId, Overflow,
    Pixels, ScrollHandle, Size, StatefulInteractiveElement, StyleRefinement, Styled, Window, point,
    px, size,
};
use smallvec::SmallVec;

/// Pure layout math, testable without a window.
pub mod math {
    use std::ops::Range;
    use std::rc::Rc;

    /// How the columns share a width.
    #[derive(Clone, Copy, Debug, Default, PartialEq)]
    pub struct Columns {
        pub cols: usize,
        /// Before the first slot: what the capped gutters could not absorb,
        /// halved, so the block of tiles sits in the middle.
        pub left: f32,
        /// Slot width: a tile, its gutter, and its share of the spare width.
        pub pitch: f32,
        pub tile: f32,
    }

    impl Columns {
        /// As many columns as fit with at least `gap` between tiles. Spare
        /// width widens the gutters up to `max_gap`; beyond that it becomes
        /// the margins. Whole pixels throughout, so no tile is drawn between
        /// two.
        pub fn fit(width: f32, tile: f32, gap: f32, max_gap: f32) -> Self {
            let least = tile + gap;
            let cols = if least > 0. {
                ((width / least).floor() as usize).max(1)
            } else {
                1
            };
            let pitch = (width / cols as f32)
                .floor()
                .clamp(least, tile + max_gap.max(gap));
            Columns {
                cols,
                left: ((width - cols as f32 * pitch) / 2.).floor().max(0.),
                pitch,
                tile,
            }
        }

        /// A tile is centred in its slot.
        fn inset(&self) -> f32 {
            ((self.pitch - self.tile) / 2.).floor()
        }

        /// The column whose tile holds `x`. The gutters belong to no tile.
        fn at(&self, x: f32) -> Option<usize> {
            let x = x - self.left;
            if self.pitch <= 0. || x < 0. {
                return None;
            }
            let col = (x / self.pitch) as usize;
            let dx = x - col as f32 * self.pitch - self.inset();
            (col < self.cols && (0. ..self.tile).contains(&dx)).then_some(col)
        }

        /// Columns whose tiles a span touches.
        fn touched(&self, lo: f32, hi: f32) -> Option<(usize, usize)> {
            let (lo, hi) = (lo - self.left, hi - self.left);
            if self.pitch <= 0. || self.cols == 0 || hi < self.inset() {
                return None;
            }
            // The first slot whose tile ends after `lo`, the last whose tile
            // starts before `hi`.
            let first = (((lo - self.inset() - self.tile) / self.pitch).floor() + 1.).max(0.);
            let last = ((hi - self.inset()) / self.pitch).floor();
            let (first, last) = (first as usize, (last as usize).min(self.cols - 1));
            (first <= last).then_some((first, last))
        }
    }

    /// Where the tiles are, in content coordinates (origin at the top left
    /// of the scrolled content, inside the padding). The grid places tiles
    /// with it and the rubber band hit-tests with it, so they cannot disagree.
    ///
    /// A row is as tall as its tallest tile, so rows differ: `tops` holds
    /// where each slot row begins, and one more entry for the end. A tile
    /// keeps its own height inside the row, hanging from the row's top: the
    /// pictures of a row line up, and a tile beside a long name is not
    /// stretched to it, so its highlight hugs its own picture and name.
    #[derive(Clone, Debug, Default, PartialEq)]
    pub struct Slots {
        pub columns: Columns,
        /// Between the tiles of two rows.
        pub gap: f32,
        tops: Rc<[f32]>,
        heights: Rc<[f32]>,
    }

    impl Slots {
        /// `height_of` is the height tile `ix` needs; its row takes the most
        /// any of its tiles needs.
        pub fn new(
            columns: Columns,
            gap: f32,
            count: usize,
            height_of: impl Fn(usize) -> f32,
        ) -> Self {
            let cols = columns.cols.max(1);
            let heights: Rc<[f32]> = (0..count).map(height_of).collect();
            let mut tops = Vec::with_capacity(count.div_ceil(cols) + 1);
            let mut top = 0.;
            tops.push(top);
            for row in heights.chunks(cols) {
                let tallest = row.iter().copied().fold(0., f32::max);
                top += tallest + gap;
                tops.push(top);
            }
            Slots {
                columns,
                gap,
                tops: tops.into(),
                heights,
            }
        }

        pub fn rows(&self) -> usize {
            self.tops.len().saturating_sub(1)
        }

        pub fn height(&self) -> f32 {
            self.tops.last().copied().unwrap_or(0.)
        }

        fn inset(&self) -> f32 {
            (self.gap / 2.).floor()
        }

        /// The slot row holding `y`; the last one for anything below.
        fn row_at(&self, y: f32) -> usize {
            self.tops
                .partition_point(|top| *top <= y)
                .saturating_sub(1)
                .min(self.rows().saturating_sub(1))
        }

        /// Height of `row`: what its tallest tile needs.
        pub fn row_height(&self, row: usize) -> f32 {
            match (self.tops.get(row), self.tops.get(row + 1)) {
                (Some(top), Some(next)) => next - top - self.gap,
                _ => 0.,
            }
        }

        /// Height of tile `ix`, its own, not its row's.
        pub fn tile_height(&self, ix: usize) -> f32 {
            self.heights.get(ix).copied().unwrap_or(0.)
        }

        /// Top left corner of tile `ix`.
        pub fn origin(&self, ix: usize) -> (f32, f32) {
            let cols = self.columns.cols.max(1);
            (
                self.columns.left + (ix % cols) as f32 * self.columns.pitch + self.columns.inset(),
                self.tops.get(ix / cols).copied().unwrap_or(0.) + self.inset(),
            )
        }

        /// The tile under a point. The gutters belong to no tile.
        pub fn tile_at(&self, x: f32, y: f32) -> Option<usize> {
            if y < 0. || y >= self.height() {
                return None;
            }
            let row = self.row_at(y);
            let dy = y - self.tops[row] - self.inset();
            let col = self.columns.at(x)?;
            let ix = row * self.columns.cols + col;
            (0. ..self.tile_height(ix)).contains(&dy).then_some(ix)
        }

        /// Tiles a rectangle touches, given by its two extremes on each axis.
        pub fn covered(&self, x: (f32, f32), y: (f32, f32)) -> Vec<usize> {
            let Some((c0, c1)) = self.columns.touched(x.0, x.1) else {
                return Vec::new();
            };
            if self.rows() == 0 || y.1 < 0. || y.0 >= self.height() {
                return Vec::new();
            }
            let cols = self.columns.cols;
            let touches = |ix: usize| {
                let top = self.tops[ix / cols] + self.inset();
                top <= y.1 && top + self.tile_height(ix) > y.0
            };
            (self.row_at(y.0.max(0.))..=self.row_at(y.1))
                .flat_map(|row| (c0..=c1).map(move |col| row * cols + col))
                .filter(|ix| touches(*ix))
                .collect()
        }

        /// Rows intersecting the viewport. `offset_y` is the (negative)
        /// scroll offset.
        pub fn visible_rows(&self, offset_y: f32, viewport_h: f32) -> Range<usize> {
            if self.rows() == 0 {
                return 0..0;
            }
            let top = (-offset_y).max(0.);
            let last = self.tops.partition_point(|t| *t < top + viewport_h);
            self.row_at(top)..last.min(self.rows())
        }

        /// The first item of the row at the top of the viewport, and how far
        /// into that row the viewport begins: what a reflow must keep.
        pub fn anchor(&self, offset_y: f32) -> (usize, f32) {
            if self.rows() == 0 {
                return (0, 0.);
            }
            let top = (-offset_y).max(0.);
            let row = self.row_at(top);
            (row * self.columns.cols.max(1), top - self.tops[row])
        }

        /// Scroll offset that shows `anchor` again after the rows changed: a
        /// resize regroups them, a zoom or a rename changes their heights.
        pub fn offset_for_anchor(&self, (item, within): (usize, f32)) -> f32 {
            if self.rows() == 0 {
                return 0.;
            }
            let row = (item / self.columns.cols.max(1)).min(self.rows() - 1);
            let room = (self.tops[row + 1] - self.tops[row] - 1.).max(0.);
            -(self.tops[row] + within.clamp(0., room))
        }

        /// Offset that shows item `ix` (row at the top), or the current
        /// offset when the item is already fully visible and `strict` is
        /// false. Clamped to the content.
        pub fn offset_for_item(
            &self,
            ix: usize,
            viewport_h: f32,
            content_h: f32,
            current: f32,
            strict: bool,
        ) -> f32 {
            if self.rows() == 0 {
                return current;
            }
            let row = (ix / self.columns.cols.max(1)).min(self.rows() - 1);
            let (top, bottom) = (self.tops[row], self.tops[row + 1]);
            let scroll_top = -current;
            let visible = top >= scroll_top && bottom <= scroll_top + viewport_h;
            let max_scroll = (content_h - viewport_h).max(0.0);
            if !strict && visible {
                return current;
            }
            let target = if !strict && bottom > scroll_top + viewport_h && top >= scroll_top {
                // Scroll the minimum: put the row at the bottom.
                bottom - viewport_h
            } else {
                top
            };
            -target.clamp(0.0, max_scroll)
        }
    }
}

/// The height each tile needs. Row tops are summed once per `revision`, not
/// per frame, so it must change whenever an answer of `of` does.
#[derive(Clone)]
pub struct TileHeights {
    pub revision: u64,
    pub of: Rc<dyn Fn(usize) -> f32>,
}

/// Create a grid of `item_count` tiles `tile_width` wide. `f` renders the
/// items in a range given the column count; each is laid out at its own
/// height, at the top of its row.
#[track_caller]
pub fn uniform_grid<R>(
    id: impl Into<ElementId>,
    item_count: usize,
    tile_width: Pixels,
    heights: TileHeights,
    f: impl 'static + Fn(Range<usize>, usize, &mut Window, &mut App) -> Vec<R>,
) -> UniformGrid
where
    R: IntoElement,
{
    let mut base_style = StyleRefinement::default();
    base_style.overflow.y = Some(Overflow::Scroll);
    let render_items =
        move |range: Range<usize>, cols: usize, window: &mut Window, cx: &mut App| {
            f(range, cols, window, cx)
                .into_iter()
                .map(IntoElement::into_any_element)
                .collect()
        };
    // `Interactivity`'s other fields are `pub(crate)`; the two public ones
    // (id and base style) are all a scrollable element needs.
    let mut interactivity = Interactivity::new();
    interactivity.element_id = Some(id.into());
    interactivity.base_style = Box::new(base_style);
    UniformGrid {
        item_count,
        tile_width,
        heights,
        gap: Size::default(),
        max_gap: None,
        render_items: Box::new(render_items),
        interactivity,
        scroll_handle: None,
    }
}

type RenderItems = Box<
    dyn for<'a> Fn(Range<usize>, usize, &'a mut Window, &'a mut App) -> SmallVec<[AnyElement; 64]>,
>;

pub struct UniformGrid {
    item_count: usize,
    tile_width: Pixels,
    heights: TileHeights,
    gap: Size<Pixels>,
    max_gap: Option<Pixels>,
    render_items: RenderItems,
    interactivity: Interactivity,
    scroll_handle: Option<UniformGridScrollHandle>,
}

pub struct UniformGridFrameState {
    items: SmallVec<[AnyElement; 32]>,
}

/// What the last prepaint decided; keyboard navigation reads this.
#[derive(Clone, Debug, Default)]
pub struct GridLayout {
    pub slots: math::Slots,
    pub rows_visible: usize,
    /// Where tile coordinates start, from the element's top left (its
    /// border and padding).
    pub origin: (f32, f32),
}

/// A scroll asked for before the next prepaint, which has the real rows.
#[derive(Clone, Copy, Debug)]
enum Deferred {
    /// Bring an item into view, its row at the top when `strict`.
    Item { ix: usize, strict: bool },
    /// Put an item's row at the top, `within` pixels scrolled into it.
    Anchor { ix: usize, within: f32 },
}

#[derive(Debug, Default)]
pub struct GridScrollState {
    pub base_handle: ScrollHandle,
    deferred: Option<Deferred>,
    layout: GridLayout,
    /// What `layout.slots` was built from: rebuilt only when this changes.
    built_from: Option<(usize, math::Columns, f32, u64)>,
}

/// Scroll handle for a [`UniformGrid`]; keep it in the view and pass it each frame.
#[derive(Clone, Debug, Default)]
pub struct UniformGridScrollHandle(pub Rc<RefCell<GridScrollState>>);

impl UniformGridScrollHandle {
    pub fn new() -> Self {
        Self::default()
    }

    /// Bring item `ix` into view with the minimum scroll.
    pub fn scroll_to_item(&self, ix: usize) {
        self.0.borrow_mut().deferred = Some(Deferred::Item { ix, strict: false });
    }

    /// Put item `ix`'s row at the top.
    pub fn scroll_to_item_strict(&self, ix: usize) {
        self.0.borrow_mut().deferred = Some(Deferred::Item { ix, strict: true });
    }

    /// The first item of the row at the top of the last prepaint, and how
    /// many pixels into that row the view began: what `scroll_to_anchor`
    /// puts back, exactly.
    pub fn anchor(&self) -> (usize, f32) {
        let state = self.0.borrow();
        state
            .layout
            .slots
            .anchor(f32::from(state.base_handle.offset().y))
    }

    /// Scroll back to an `anchor`: its item's row at the top, as far into
    /// it as it was, from the rows as they are then (the window may have
    /// been resized, the zoom changed, entries added before it).
    pub fn scroll_to_anchor(&self, (ix, within): (usize, f32)) {
        self.0.borrow_mut().deferred = Some(Deferred::Anchor { ix, within });
    }

    /// Column count of the last prepaint (1 before the first).
    pub fn cols(&self) -> usize {
        self.0.borrow().layout.slots.columns.cols.max(1)
    }

    /// Where the last prepaint put the tiles.
    pub fn slots(&self) -> math::Slots {
        self.0.borrow().layout.slots.clone()
    }

    /// Fully or partly visible rows in the last prepaint.
    pub fn rows_visible(&self) -> usize {
        self.0.borrow().layout.rows_visible.max(1)
    }

    /// Where tile coordinates start in the last prepaint, from the
    /// element's top left.
    pub fn origin(&self) -> (f32, f32) {
        self.0.borrow().layout.origin
    }
}

impl Styled for UniformGrid {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.interactivity.base_style
    }
}

impl InteractiveElement for UniformGrid {
    fn interactivity(&mut self) -> &mut Interactivity {
        &mut self.interactivity
    }
}

impl StatefulInteractiveElement for UniformGrid {}

impl UniformGrid {
    /// The least space between two tiles; spare width widens it.
    pub fn gap(mut self, gap: Size<Pixels>) -> Self {
        self.gap = gap;
        self
    }

    /// How wide spare width may make a gutter; the rest becomes margins.
    pub fn max_gap(mut self, max_gap: Pixels) -> Self {
        self.max_gap = Some(max_gap);
        self
    }

    /// Track scroll state with `handle` (also enables wheel scrolling on it).
    pub fn track_scroll(mut self, handle: UniformGridScrollHandle) -> Self {
        let base = handle.0.borrow().base_handle.clone();
        self.scroll_handle = Some(handle);
        <Self as StatefulInteractiveElement>::track_scroll(self, &base)
    }
}

impl IntoElement for UniformGrid {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for UniformGrid {
    type RequestLayoutState = UniformGridFrameState;
    type PrepaintState = Option<Hitbox>;

    fn id(&self) -> Option<ElementId> {
        self.interactivity.element_id.clone()
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let layout_id = self.interactivity.request_layout(
            global_id,
            inspector_id,
            window,
            cx,
            |style, window, cx| window.request_layout(style, None, cx),
        );
        (
            layout_id,
            UniformGridFrameState {
                items: SmallVec::new(),
            },
        )
    }

    fn prepaint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        frame_state: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Hitbox> {
        let style = self
            .interactivity
            .compute_style(global_id, None, window, cx);
        let border = style.border_widths.to_pixels(window.rem_size());
        let padding = style
            .padding
            .to_pixels(bounds.size.into(), window.rem_size());
        let padded_bounds = Bounds::from_corners(
            bounds.origin + point(border.left + padding.left, border.top + padding.top),
            bounds.bottom_right()
                - point(border.right + padding.right, border.bottom + padding.bottom),
        );
        let gap = (f32::from(self.gap.width), f32::from(self.gap.height));
        let viewport_w = f32::from(padded_bounds.size.width);
        let viewport_h = f32::from(padded_bounds.size.height);

        // Everything size-dependent is decided here, from the real bounds.
        let columns = math::Columns::fit(
            viewport_w,
            f32::from(self.tile_width),
            gap.0,
            self.max_gap.map_or(f32::INFINITY, f32::from),
        );
        let built_from = (self.item_count, columns, gap.1, self.heights.revision);
        // The rows before this frame, to find again what was at the top.
        let (deferred, before) = match &self.scroll_handle {
            Some(h) => {
                let mut s = h.0.borrow_mut();
                let changed = s.built_from.is_some() && s.built_from != Some(built_from);
                (s.deferred.take(), changed.then(|| s.layout.slots.clone()))
            }
            None => (None, None),
        };
        let reuse = self
            .scroll_handle
            .as_ref()
            .filter(|h| h.0.borrow().built_from == Some(built_from))
            .map(|h| h.0.borrow().layout.slots.clone());
        let slots = reuse.unwrap_or_else(|| {
            math::Slots::new(columns, gap.1, self.item_count, &*self.heights.of)
        });
        let cols = columns.cols;
        let content_h = slots.height() + f32::from(padding.top + padding.bottom);
        let content_size = size(padded_bounds.size.width, px(content_h));

        let base = self
            .scroll_handle
            .as_ref()
            .map(|h| h.0.borrow().base_handle.clone());
        let item_count = self.item_count;
        let render_items = &self.render_items;
        let handle = self.scroll_handle.clone();

        self.interactivity.prepaint(
            global_id,
            inspector_id,
            bounds,
            content_size,
            window,
            cx,
            |_style, mut scroll_offset, hitbox, window, cx| {
                let mut y = f32::from(scroll_offset.y);
                if let Some(before) = &before {
                    y = slots.offset_for_anchor(before.anchor(y));
                }
                match deferred {
                    Some(Deferred::Item { ix, strict }) => {
                        let ix = ix.min(item_count.saturating_sub(1));
                        y = slots.offset_for_item(ix, viewport_h, content_h, y, strict);
                    }
                    Some(Deferred::Anchor { ix, within }) => {
                        y = slots.offset_for_anchor((ix.min(item_count.saturating_sub(1)), within));
                    }
                    None => {}
                }
                let max_scroll = (content_h - viewport_h).max(0.0);
                y = y.clamp(-max_scroll, 0.0);
                if (y - f32::from(scroll_offset.y)).abs() > 0.01 {
                    scroll_offset.y = px(y);
                    if let Some(b) = &base {
                        b.set_offset(point(scroll_offset.x, scroll_offset.y));
                    }
                }

                let visible = slots.visible_rows(y, viewport_h);
                if let Some(h) = &handle {
                    let mut state = h.0.borrow_mut();
                    state.built_from = Some(built_from);
                    state.layout = GridLayout {
                        slots: slots.clone(),
                        rows_visible: visible.len(),
                        origin: (
                            f32::from(border.left + padding.left),
                            f32::from(border.top + padding.top * 2.),
                        ),
                    };
                }
                if item_count == 0 || visible.is_empty() {
                    return hitbox;
                }
                let items_range = (visible.start * cols)..((visible.end * cols).min(item_count));
                let items = render_items(items_range.clone(), cols, window, cx);
                let content_mask = ContentMask { bounds };
                window.with_content_mask(Some(content_mask), |window| {
                    for (mut item, ix) in items.into_iter().zip(items_range) {
                        let (x, y) = slots.origin(ix);
                        let origin = padded_bounds.origin
                            + point(
                                px(x) + scroll_offset.x,
                                px(y) + scroll_offset.y + padding.top,
                            );
                        let available = size(
                            AvailableSpace::Definite(px(columns.tile)),
                            AvailableSpace::Definite(px(slots.tile_height(ix))),
                        );
                        item.layout_as_root(available, window, cx);
                        item.prepaint_at(origin, window, cx);
                        frame_state.items.push(item);
                    }
                });
                hitbox
            },
        )
    }

    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        frame_state: &mut Self::RequestLayoutState,
        hitbox: &mut Option<Hitbox>,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.interactivity.paint(
            global_id,
            inspector_id,
            bounds,
            hitbox.as_ref(),
            window,
            cx,
            |_, window, cx| {
                for item in &mut frame_state.items {
                    item.paint(window, cx);
                }
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::math::*;

    /// Five 100 px tiles a row; every fourth name needs a taller tile.
    fn slots(count: usize) -> Slots {
        let columns = Columns::fit(600., 100., 12., 40.);
        Slots::new(
            columns,
            8.,
            count,
            |ix| if ix % 4 == 0 { 170. } else { 140. },
        )
    }

    #[test]
    fn spare_width_goes_to_the_gutters_up_to_a_cap() {
        // Five tiles with 12 px gutters need 560; the other 40 are shared.
        let columns = Columns::fit(600., 100., 12., 40.);
        assert_eq!((columns.cols, columns.pitch, columns.left), (5, 120., 0.));
        // Capped at 16 px, the slots take 580 and the margins the rest.
        let capped = Columns::fit(600., 100., 12., 16.);
        assert_eq!((capped.cols, capped.pitch, capped.left), (5, 116., 10.));
        // A pane narrower than one slot still holds a column.
        let narrow = Columns::fit(50., 100., 12., 40.);
        assert_eq!((narrow.cols, narrow.pitch, narrow.left), (1, 112., 0.));
    }

    #[test]
    fn a_row_is_as_tall_as_its_tallest_tile() {
        // Items 0 and 4 are tall: both are in the first row. The second row
        // (5..10) holds item 8, the third (10..12) only short ones.
        let slots = slots(12);
        assert_eq!(
            (
                slots.row_height(0),
                slots.row_height(1),
                slots.row_height(2)
            ),
            (170., 170., 140.)
        );
        // A tile keeps its own height inside a taller row.
        assert_eq!((slots.tile_height(0), slots.tile_height(1)), (170., 140.));
        assert!(slots.tile_height(12) < 1., "past the last tile");
        assert_eq!((slots.rows(), slots.height()), (3, 178. + 178. + 148.));
        assert_eq!(slots.origin(0), (10., 4.));
        assert_eq!(slots.origin(6), (130., 182.));
        assert_eq!(slots.origin(10), (10., 360.));
        let empty = Slots::new(Columns::fit(600., 100., 12., 40.), 8., 0, |_| 140.);
        assert_eq!((empty.rows(), empty.height()), (0, 0.));
    }

    #[test]
    fn gutters_belong_to_no_tile() {
        let slots = slots(12);
        assert_eq!(slots.tile_at(10., 4.), Some(0));
        assert_eq!(slots.tile_at(109., 173.), Some(0));
        assert_eq!(slots.tile_at(9., 50.), None, "left of the tile");
        assert_eq!(slots.tile_at(115., 50.), None, "between two tiles");
        assert_eq!(slots.tile_at(50., 176.), None, "between two rows");
        assert_eq!(slots.tile_at(130., 143.), Some(1));
        assert_eq!(slots.tile_at(130., 150.), None, "below a short tile");
        assert_eq!(slots.tile_at(130., 182.), Some(6));
        assert_eq!(slots.tile_at(50., 499.), Some(10), "the short last row");
        assert_eq!(slots.tile_at(50., 501.), None);
        assert_eq!(slots.tile_at(50., 5000.), None, "below the content");
        assert_eq!(slots.tile_at(601., 50.), None, "beside the last column");
        assert_eq!(Slots::default().tile_at(5., 5.), None, "before any layout");
        // Capped gutters leave a margin, which is no tile either.
        let capped = Slots::new(Columns::fit(600., 100., 12., 16.), 8., 12, |_| 140.);
        assert_eq!(capped.tile_at(12., 50.), None);
        assert_eq!(capped.tile_at(18., 50.), Some(0));
    }

    #[test]
    fn a_band_selects_the_tiles_it_touches() {
        let slots = slots(12);
        assert!(
            slots.covered((111., 128.), (0., 300.)).is_empty(),
            "in a gutter"
        );
        assert_eq!(slots.covered((105., 135.), (100., 176.)), vec![0, 1]);
        assert_eq!(
            slots.covered((105., 135.), (150., 176.)),
            vec![0],
            "under the short tile, beside the tall one"
        );
        assert_eq!(slots.covered((105., 135.), (100., 182.)), vec![0, 1, 5, 6]);
        assert_eq!(slots.covered((-50., 5000.), (0., 10.)), vec![0, 1, 2, 3, 4]);
        assert_eq!(slots.covered((0., 50.), (175., 9000.)), vec![5, 10]);
        assert!(
            slots.covered((0., 9.), (0., 300.)).is_empty(),
            "the left margin"
        );
        assert!(slots.covered((0., 50.), (9000., 9900.)).is_empty(), "below");
        assert!(Slots::default().covered((0., 9.), (0., 9.)).is_empty());
    }

    #[test]
    fn visible_rows_follow_the_offset() {
        let slots = slots(50); // ten rows of 178
        assert_eq!(slots.visible_rows(0.0, 400.0), 0..3);
        assert_eq!(slots.visible_rows(-178.0, 356.0), 1..3);
        assert_eq!(slots.visible_rows(-400.0, 400.0), 2..5);
        assert_eq!(slots.visible_rows(-1700.0, 400.0), 9..10, "clamped to rows");
        assert_eq!(Slots::default().visible_rows(0.0, 400.0), 0..0);
    }

    #[test]
    fn reflow_keeps_the_first_visible_item() {
        let wide = slots(50);
        // Scrolled 30 px into the third row: item 10 is at the top.
        let anchor = wide.anchor(-(2. * 178. + 30.));
        assert_eq!(anchor, (10, 30.));
        // Three columns now: item 10 is in the fourth row (9..12).
        let narrow = Slots::new(Columns::fit(340., 100., 12., 40.), 8., 50, |_| 140.);
        assert_eq!(narrow.columns.cols, 3);
        let y = narrow.offset_for_anchor(anchor);
        assert!((y - -(3. * 148. + 30.)).abs() < 1e-3);
        assert_eq!(narrow.anchor(y).0, 9);
        // Nothing changed: the offset is what it was.
        assert!((wide.offset_for_anchor(anchor) - -(2. * 178. + 30.)).abs() < 1e-3);
        // The list shrank under the view: the last row, not past it.
        let short = slots(7);
        assert_eq!(short.anchor(short.offset_for_anchor((40, 0.))).0, 5);
    }

    #[test]
    fn scroll_to_item_is_minimal_unless_strict() {
        // viewport 400, 4 columns, 100 items -> 25 rows of 100
        let slots = Slots::new(Columns::fit(448., 100., 12., 40.), 0., 100, |_| 100.);
        assert_eq!(slots.columns.cols, 4);
        let at = |ix, current, strict| slots.offset_for_item(ix, 400., 2500., current, strict);
        // Already visible: unchanged.
        assert!((at(5, 0., false) - 0.).abs() < 1e-6);
        // Below the viewport: the row lands at the bottom.
        assert!((at(20, 0., false) - -200.).abs() < 1e-6);
        // Above the viewport: the row lands at the top.
        assert!((at(4, -500., false) - -100.).abs() < 1e-6);
        // Strict: always the top.
        assert!((at(20, 0., true) - -500.).abs() < 1e-6);
        // Clamped to the content.
        assert!((at(99, 0., true) - -2100.).abs() < 1e-6);
    }
}
