//! A scrollable grid of fixed-size tiles, virtualized by row. Modeled on
//! gpui's `uniform_list`: the column count is computed in `prepaint` from the
//! element's real width, so a reflow never lags a frame behind a resize,
//! split, or panel toggle.
//!
//! Tiles sit in slots. A slot is at least a tile plus the gutter, and the
//! width no whole column could use is shared between the slots, so the grid
//! fills its pane and the gutters only ever grow. `math::Slots` is that
//! arithmetic, shared with whatever hit-tests the grid (the rubber band).
//!
//! What `uniform_list` uses that is `pub(crate)` in gpui, and the public
//! substitute here: the shared scroll offset (`Interactivity::scroll_offset`)
//! is reached through `InteractiveElement::track_scroll`, which makes the
//! handle's offset the element's offset; the wheel listeners and offset
//! clamping come with `overflow.y = Scroll` through `Interactivity::paint`.

use std::cell::{Cell, RefCell};
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

    /// Where the tiles are, in content coordinates (origin at the top left
    /// of the scrolled content, inside the padding). The grid places tiles
    /// with it and the rubber band hit-tests with it, so they cannot disagree.
    #[derive(Clone, Copy, Debug, Default, PartialEq)]
    pub struct Slots {
        pub cols: usize,
        /// Slot size: a tile, its gutter, and its share of the spare width.
        pub pitch: (f32, f32),
        pub tile: (f32, f32),
    }

    impl Slots {
        /// Whole pixels throughout, so no tile is drawn between two.
        pub fn fit(width: f32, tile: (f32, f32), gap: (f32, f32)) -> Self {
            let cols = cols_for(width, tile.0 + gap.0);
            Slots {
                cols,
                pitch: (
                    (width / cols as f32).floor().max(tile.0 + gap.0),
                    tile.1 + gap.1,
                ),
                tile,
            }
        }

        /// A tile is centred in its slot.
        fn inset(&self) -> (f32, f32) {
            (
                ((self.pitch.0 - self.tile.0) / 2.).floor(),
                ((self.pitch.1 - self.tile.1) / 2.).floor(),
            )
        }

        /// Top left corner of tile `ix`.
        pub fn origin(&self, ix: usize) -> (f32, f32) {
            let cols = self.cols.max(1);
            let inset = self.inset();
            (
                (ix % cols) as f32 * self.pitch.0 + inset.0,
                (ix / cols) as f32 * self.pitch.1 + inset.1,
            )
        }

        /// The tile under a point. The gutters belong to no tile.
        pub fn tile_at(&self, x: f32, y: f32) -> Option<usize> {
            if self.pitch.0 <= 0. || self.pitch.1 <= 0. || x < 0. || y < 0. {
                return None;
            }
            let (col, row) = ((x / self.pitch.0) as usize, (y / self.pitch.1) as usize);
            let inset = self.inset();
            let (dx, dy) = (
                x - col as f32 * self.pitch.0 - inset.0,
                y - row as f32 * self.pitch.1 - inset.1,
            );
            let inside = (0. ..self.tile.0).contains(&dx) && (0. ..self.tile.1).contains(&dy);
            (col < self.cols && inside).then(|| row * self.cols + col)
        }

        /// Columns (or rows) whose tiles a span touches, on one axis.
        fn touched(lo: f32, hi: f32, pitch: f32, inset: f32, tile: f32) -> Option<(usize, usize)> {
            if pitch <= 0. || hi < inset {
                return None;
            }
            // The first slot whose tile ends after `lo`, the last whose tile
            // starts before `hi`.
            let first = ((lo - inset - tile) / pitch).floor() + 1.;
            let last = ((hi - inset) / pitch).floor();
            let first = first.max(0.);
            (first <= last).then_some((first as usize, last as usize))
        }

        /// Tiles a rectangle touches, given by its two extremes on each axis.
        pub fn covered(&self, x: (f32, f32), y: (f32, f32)) -> Vec<usize> {
            let inset = self.inset();
            let (Some((c0, c1)), Some((r0, r1))) = (
                Self::touched(x.0, x.1, self.pitch.0, inset.0, self.tile.0),
                Self::touched(y.0, y.1, self.pitch.1, inset.1, self.tile.1),
            ) else {
                return Vec::new();
            };
            let cols = self.cols.max(1);
            let c1 = c1.min(cols - 1);
            (r0..=r1)
                .flat_map(|row| (c0..=c1).map(move |col| row * cols + col))
                .collect()
        }
    }

    /// Columns that fit in `width`; at least one.
    pub fn cols_for(width: f32, cell_w: f32) -> usize {
        if cell_w <= 0.0 {
            return 1;
        }
        ((width / cell_w).floor() as usize).max(1)
    }

    /// Rows needed for `n` items at `cols` per row.
    pub fn rows_for(n: usize, cols: usize) -> usize {
        n.div_ceil(cols.max(1))
    }

    /// Rows intersecting the viewport. `offset_y` is the (negative) scroll offset.
    pub fn visible_rows(offset_y: f32, viewport_h: f32, cell_h: f32, rows: usize) -> Range<usize> {
        if cell_h <= 0.0 || rows == 0 {
            return 0..0;
        }
        let first = ((-offset_y) / cell_h).floor().max(0.0) as usize;
        let last = ((-offset_y + viewport_h) / cell_h).ceil().max(0.0) as usize;
        first.min(rows)..last.min(rows)
    }

    /// The item at the top-left of the viewport for a given offset and column count.
    pub fn top_item(offset_y: f32, cell_h: f32, cols: usize) -> usize {
        if cell_h <= 0.0 {
            return 0;
        }
        (((-offset_y) / cell_h).floor().max(0.0) as usize) * cols.max(1)
    }

    /// New scroll offset that keeps the same first visible item after the
    /// rows changed from `old` to `new`, each a column count and a row height:
    /// a resize changes the first, a zoom both.
    pub fn preserve_top_item(offset_y: f32, old: (usize, f32), new: (usize, f32)) -> f32 {
        let item = top_item(offset_y, old.1, old.0);
        -((item / new.0.max(1)) as f32 * new.1)
    }

    /// Offset that shows item `ix` (row at the top), or the current offset
    /// when the item is already fully visible and `strict` is false. Clamped
    /// to the content.
    pub fn offset_for_item(
        ix: usize,
        cols: usize,
        cell_h: f32,
        viewport_h: f32,
        content_h: f32,
        current: f32,
        strict: bool,
    ) -> f32 {
        let row = ix / cols.max(1);
        let top = row as f32 * cell_h;
        let bottom = top + cell_h;
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

/// Create a grid of `item_count` tiles of size `cell`. `f` renders the items
/// in a range given the column count.
#[track_caller]
pub fn uniform_grid<R>(
    id: impl Into<ElementId>,
    item_count: usize,
    cell: Size<Pixels>,
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
        cell,
        gap: Size::default(),
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
    cell: Size<Pixels>,
    gap: Size<Pixels>,
    render_items: RenderItems,
    interactivity: Interactivity,
    scroll_handle: Option<UniformGridScrollHandle>,
}

pub struct UniformGridFrameState {
    items: SmallVec<[AnyElement; 32]>,
}

/// What the last prepaint decided; keyboard navigation reads this.
#[derive(Clone, Copy, Debug, Default)]
pub struct GridLayout {
    pub slots: math::Slots,
    pub rows_visible: usize,
    pub top_item: usize,
}

#[derive(Debug, Default)]
pub struct GridScrollState {
    pub base_handle: ScrollHandle,
    deferred: Option<(usize, bool)>,
    layout: Cell<GridLayout>,
    /// Column count and row height of the last prepaint.
    last_rows: Option<(usize, f32)>,
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
        self.0.borrow_mut().deferred = Some((ix, false));
    }

    /// Put item `ix`'s row at the top.
    pub fn scroll_to_item_strict(&self, ix: usize) {
        self.0.borrow_mut().deferred = Some((ix, true));
    }

    /// Column count of the last prepaint (1 before the first).
    pub fn cols(&self) -> usize {
        self.0.borrow().layout.get().slots.cols.max(1)
    }

    /// Where the last prepaint put the tiles.
    pub fn slots(&self) -> math::Slots {
        self.0.borrow().layout.get().slots
    }

    /// Fully or partly visible rows in the last prepaint.
    pub fn rows_visible(&self) -> usize {
        self.0.borrow().layout.get().rows_visible.max(1)
    }

    /// Index of the item at the top-left of the viewport in the last prepaint.
    pub fn top_item(&self) -> usize {
        self.0.borrow().layout.get().top_item
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
        let tile = (f32::from(self.cell.width), f32::from(self.cell.height));
        let gap = (f32::from(self.gap.width), f32::from(self.gap.height));
        let viewport_w = f32::from(padded_bounds.size.width);
        let viewport_h = f32::from(padded_bounds.size.height);

        // Everything size-dependent is decided here, from the real bounds.
        let slots = math::Slots::fit(viewport_w, tile, gap);
        let (cols, cell_h) = (slots.cols, slots.pitch.1);
        let rows = math::rows_for(self.item_count, cols);
        let content_h = rows as f32 * cell_h + f32::from(padding.top + padding.bottom);
        let content_size = size(padded_bounds.size.width, px(content_h));

        let (deferred, last_rows) = match &self.scroll_handle {
            Some(h) => {
                let mut s = h.0.borrow_mut();
                let last = s.last_rows;
                s.last_rows = Some((cols, cell_h));
                (s.deferred.take(), last)
            }
            None => (None, None),
        };
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
                if let Some(old) = last_rows
                    && old != (cols, cell_h)
                {
                    y = math::preserve_top_item(y, old, (cols, cell_h));
                }
                if let Some((ix, strict)) = deferred {
                    let ix = ix.min(item_count.saturating_sub(1));
                    y = math::offset_for_item(ix, cols, cell_h, viewport_h, content_h, y, strict);
                }
                let max_scroll = (content_h - viewport_h).max(0.0);
                y = y.clamp(-max_scroll, 0.0);
                if (y - f32::from(scroll_offset.y)).abs() > 0.01 {
                    scroll_offset.y = px(y);
                    if let Some(b) = &base {
                        b.set_offset(point(scroll_offset.x, scroll_offset.y));
                    }
                }

                let visible = math::visible_rows(y, viewport_h, cell_h, rows);
                if let Some(h) = &handle {
                    h.0.borrow().layout.set(GridLayout {
                        slots,
                        rows_visible: visible.len(),
                        top_item: math::top_item(y, cell_h, cols),
                    });
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
                            AvailableSpace::Definite(px(tile.0)),
                            AvailableSpace::Definite(px(tile.1)),
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

    #[test]
    fn columns_and_rows() {
        assert_eq!(cols_for(800.0, 152.0), 5);
        assert_eq!(cols_for(100.0, 152.0), 1, "never zero columns");
        assert_eq!(cols_for(0.0, 152.0), 1);
        assert_eq!(rows_for(0, 5), 0);
        assert_eq!(rows_for(10, 5), 2);
        assert_eq!(rows_for(11, 5), 3);
    }

    #[test]
    fn spare_width_goes_to_the_gutters() {
        // Five 100 px tiles with 12 px gutters need 560; the other 40 are shared.
        let slots = Slots::fit(600., (100., 140.), (12., 8.));
        assert_eq!((slots.cols, slots.pitch), (5, (120., 148.)));
        assert_eq!(slots.origin(0), (10., 4.));
        assert_eq!(slots.origin(6), (130., 152.));
        // A pane narrower than one slot still holds a column.
        let narrow = Slots::fit(50., (100., 140.), (12., 8.));
        assert_eq!((narrow.cols, narrow.pitch), (1, (112., 148.)));
    }

    #[test]
    fn gutters_belong_to_no_tile() {
        let slots = Slots::fit(600., (100., 140.), (12., 8.));
        assert_eq!(slots.tile_at(10., 4.), Some(0));
        assert_eq!(slots.tile_at(109., 143.), Some(0));
        assert_eq!(slots.tile_at(9., 50.), None, "left of the tile");
        assert_eq!(slots.tile_at(115., 50.), None, "between two tiles");
        assert_eq!(slots.tile_at(50., 146.), None, "between two rows");
        assert_eq!(slots.tile_at(130., 152.), Some(6));
        assert_eq!(slots.tile_at(601., 50.), None, "beside the last column");
        assert_eq!(Slots::default().tile_at(5., 5.), None, "before any layout");
    }

    #[test]
    fn a_band_selects_the_tiles_it_touches() {
        let slots = Slots::fit(600., (100., 140.), (12., 8.));
        assert!(
            slots.covered((111., 128.), (0., 300.)).is_empty(),
            "in a gutter"
        );
        assert_eq!(slots.covered((105., 135.), (100., 146.)), vec![0, 1]);
        assert_eq!(slots.covered((105., 135.), (100., 152.)), vec![0, 1, 5, 6]);
        assert_eq!(slots.covered((-50., 5000.), (0., 10.)), vec![0, 1, 2, 3, 4]);
        assert!(
            slots.covered((0., 9.), (0., 300.)).is_empty(),
            "the left margin"
        );
        assert!(Slots::default().covered((0., 9.), (0., 9.)).is_empty());
    }

    #[test]
    fn visible_rows_follow_the_offset() {
        assert_eq!(visible_rows(0.0, 400.0, 100.0, 50), 0..4);
        assert_eq!(visible_rows(-250.0, 400.0, 100.0, 50), 2..7);
        assert_eq!(
            visible_rows(-4900.0, 400.0, 100.0, 50),
            49..50,
            "clamped to rows"
        );
        assert_eq!(visible_rows(0.0, 400.0, 100.0, 0), 0..0);
    }

    #[test]
    fn reflow_keeps_the_first_visible_item() {
        // 6 columns, scrolled to row 4: item 24 is at the top.
        assert_eq!(top_item(-400.0, 100.0, 6), 24);
        // Now 4 columns: item 24 is row 6.
        let y = preserve_top_item(-400.0, (6, 100.0), (4, 100.0));
        assert!((y - -600.0).abs() < 1e-6);
        assert_eq!(top_item(y, 100.0, 4), 24);
        // And 8 columns: item 24 is row 3.
        assert!((preserve_top_item(-400.0, (6, 100.0), (8, 100.0)) - -300.0).abs() < 1e-6);
        // Zoomed in: three taller columns, and item 24 is still at the top.
        assert!((preserve_top_item(-400.0, (6, 100.0), (3, 180.0)) - -1440.0).abs() < 1e-6);
        // Partial rows floor to the row start.
        assert_eq!(top_item(-450.0, 100.0, 6), 24);
    }

    #[test]
    fn scroll_to_item_is_minimal_unless_strict() {
        // viewport 400, cell 100, 4 cols, 100 items -> 25 rows, content 2500
        let cur = -200.0; // rows 2..6 visible
        assert!(
            (offset_for_item(12, 4, 100.0, 400.0, 2500.0, cur, false) - cur).abs() < 1e-6,
            "visible: unchanged"
        );
        assert!(
            (offset_for_item(12, 4, 100.0, 400.0, 2500.0, cur, true) - -300.0).abs() < 1e-6,
            "strict: row 3 at top"
        );
        assert!(
            (offset_for_item(28, 4, 100.0, 400.0, 2500.0, cur, false) - -400.0).abs() < 1e-6,
            "below: row 7 at bottom"
        );
        assert!(
            (offset_for_item(0, 4, 100.0, 400.0, 2500.0, cur, false) - 0.0).abs() < 1e-6,
            "above: row at top"
        );
        assert!(
            (offset_for_item(99, 4, 100.0, 400.0, 2500.0, cur, true) - -2100.0).abs() < 1e-6,
            "clamped to content"
        );
    }
}
