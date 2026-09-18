//! A scrollable grid of fixed-size cells, virtualized by row. Modeled on
//! gpui's `uniform_list`: the column count is computed in `prepaint` from the
//! element's real width, so a reflow never lags a frame behind a resize,
//! split, or panel toggle.
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
    /// column count changed from `old_cols` to `new_cols`.
    pub fn preserve_top_item(offset_y: f32, cell_h: f32, old_cols: usize, new_cols: usize) -> f32 {
        let item = top_item(offset_y, cell_h, old_cols);
        -((item / new_cols.max(1)) as f32 * cell_h)
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

/// Create a grid of `item_count` cells of size `cell`. `f` renders the items
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
    pub cols: usize,
    pub rows_visible: usize,
    pub top_item: usize,
}

#[derive(Debug, Default)]
pub struct GridScrollState {
    pub base_handle: ScrollHandle,
    deferred: Option<(usize, bool)>,
    layout: Cell<GridLayout>,
    last_cols: Option<usize>,
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
        self.0.borrow().layout.get().cols.max(1)
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
        let (cell_w, cell_h) = (f32::from(self.cell.width), f32::from(self.cell.height));
        let viewport_w = f32::from(padded_bounds.size.width);
        let viewport_h = f32::from(padded_bounds.size.height);

        // Everything size-dependent is decided here, from the real bounds.
        let cols = math::cols_for(viewport_w, cell_w);
        let rows = math::rows_for(self.item_count, cols);
        let content_h = rows as f32 * cell_h + f32::from(padding.top + padding.bottom);
        let content_size = size(padded_bounds.size.width, px(content_h));

        let (deferred, last_cols) = match &self.scroll_handle {
            Some(h) => {
                let mut s = h.0.borrow_mut();
                let last = s.last_cols;
                s.last_cols = Some(cols);
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
                if let Some(old) = last_cols
                    && old != cols
                {
                    y = math::preserve_top_item(y, cell_h, old, cols);
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
                        cols,
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
                        let (row, col) = (ix / cols, ix % cols);
                        let origin = padded_bounds.origin
                            + point(
                                px(col as f32 * cell_w) + scroll_offset.x,
                                px(row as f32 * cell_h) + scroll_offset.y + padding.top,
                            );
                        let available = size(
                            AvailableSpace::Definite(px(cell_w)),
                            AvailableSpace::Definite(px(cell_h)),
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
        let y = preserve_top_item(-400.0, 100.0, 6, 4);
        assert!((y - -600.0).abs() < 1e-6);
        assert_eq!(top_item(y, 100.0, 4), 24);
        // And 8 columns: item 24 is row 3.
        assert!((preserve_top_item(-400.0, 100.0, 6, 8) - -300.0).abs() < 1e-6);
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
