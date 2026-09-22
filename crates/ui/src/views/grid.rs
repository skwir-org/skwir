//! The grid (gallery) view: thumbnail tiles in a `UniformGrid`, whose
//! column count and gutters are decided in prepaint from the element's real
//! width.
//!
//! A tile is what the pointer can hit: the picture, standing on the bottom of
//! its box so the name is always right under it, and up to three lines of
//! name. An icon fills the box like a square thumbnail does; smaller, it
//! would float above its name with a blank on top. The gutters between tiles
//! belong to the rubber band.
//!
//! A row is as tall as its longest name needs, so a row of short names gives
//! its space back; a tile beside a longer name keeps its own height, hanging
//! from the top of the row, so its highlight hugs its own picture and name
//! (as Dolphin's does). That takes knowing how many lines each name wraps to
//! before anything is drawn: `GridView::measure` counts them with the text
//! system's line wrapper, once per change of the listing, the order or the
//! zoom, never per frame. The label is then clamped to that count, so a
//! wrapper that disagrees with the shaper by a word costs an ellipsis, not a
//! name drawn over the next row.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::rc::Rc;

use bibli_core::EntryId;
use bibli_core::thumbs::ThumbSize;
use gpui::{AnyElement, Context, ScrollWheelEvent, Window, div, img, prelude::*, px, size};

use crate::dnd;
use crate::elements::uniform_grid::{TileHeights, UniformGridScrollHandle, uniform_grid};
use crate::icons;
use crate::shell::Shell;
use crate::theme::Theme;
use crate::thumbs::ThumbView;
use crate::view::{Nav, PaneView};

/// Zoom steps for `ui.zoom`: the side of a tile's box.
pub const ZOOMS: [u32; 5] = [64, 96, 128, 192, 256];
/// The step a new window opens at, and Ctrl+0 returns to.
pub const DEFAULT_ZOOM: u32 = 128;

#[derive(Default)]
pub struct GridView {
    scroll: UniformGridScrollHandle,
    /// Lines each name needs, in sorted order.
    lines: Rc<[u8]>,
    /// Lines per entry, with a hash of the name they were counted for, at
    /// `wrapped_at` (zoom and font). A listing streaming in, or re-sorted,
    /// wraps only the names it has not seen.
    wrapped: HashMap<EntryId, (u64, u8)>,
    wrapped_at: Option<(u32, gpui::SharedString)>,
    /// What `lines` was counted for: the names, their order and the zoom.
    /// Not the listing's version, which hydration advances with every batch.
    lines_for: Option<(u64, std::sync::Arc<[EntryId]>, u32)>,
    /// Advances with `lines`, so the grid sums its rows again.
    revision: u64,
}

/// Border and padding around a tile's content, on each side.
const FRAME: f32 = 5.;
/// Between the picture and the name.
const LABEL_GAP: f32 = 2.;
/// A tile's height is counted in lines of name, so the line height is fixed.
const LABEL_LINE: f32 = 15.;
/// A name takes what it needs up to this, then ends with an ellipsis.
const MAX_LINES: u8 = 3;
/// The least space between tiles. Spare width widens the gutters between
/// columns up to `MAX_GUTTER`; the rest becomes the margins.
const GUTTER: (f32, f32) = (12., 8.);
const MAX_GUTTER: f32 = 24.;

/// A label is as wide as the box, but never narrower than this: at the small
/// steps a box-wide label would break most names in the middle of a word.
const MIN_LABEL: f32 = 90.;

impl GridView {
    fn label_width(zoom: u32) -> f32 {
        (zoom as f32 + 2.).max(MIN_LABEL)
    }

    fn tile_width(zoom: u32) -> f32 {
        Self::label_width(zoom) + 2. * FRAME
    }

    fn tile_height(zoom: u32, lines: u8) -> f32 {
        zoom as f32 + 2. * FRAME + LABEL_GAP + f32::from(lines) * LABEL_LINE
    }

    /// Count the lines every name wraps to, when what they depend on changed.
    fn measure(&mut self, shell: &Shell, window: &Window, cx: &gpui::App) {
        let list = &shell.ui().list;
        let ws = shell.app.workspace.read();
        let listing = ws.pane(shell.pane).map(|pane| &pane.listing);
        let names = listing.map_or(0, bibli_core::Listing::names_version);
        if let Some((counted, order, zoom)) = &self.lines_for
            && (*counted, *zoom) == (names, shell.zoom)
            && (std::sync::Arc::ptr_eq(order, &list.sorted) || **order == *list.sorted)
        {
            return;
        }
        let started = std::time::Instant::now();
        let at = (shell.zoom, shell.font.clone());
        if self.wrapped_at.as_ref() != Some(&at) {
            self.wrapped.clear();
            self.wrapped_at = Some(at);
        }
        let font_size = window.rem_size() * 0.75; // text_xs
        let mut wrapper = cx
            .text_system()
            .line_wrapper(gpui::font(shell.font.clone()), font_size);
        let width = px(Self::label_width(shell.zoom));
        let mut wrapped_now = 0usize;
        let counted = &mut self.wrapped;
        self.lines = list
            .sorted
            .iter()
            .map(|id| {
                let Some(entry) = listing.and_then(|listing| listing.get(*id)) else {
                    return 1;
                };
                let hash = {
                    let mut hasher = std::hash::DefaultHasher::new();
                    entry.name.hash(&mut hasher);
                    hasher.finish()
                };
                if let Some((seen, lines)) = counted.get(id)
                    && *seen == hash
                {
                    return *lines;
                }
                wrapped_now += 1;
                let name = entry.name_str();
                let breaks = wrapper
                    .wrap_line(&[gpui::LineFragment::text(&name)], width)
                    .take(usize::from(MAX_LINES) - 1)
                    .count();
                let lines = breaks as u8 + 1;
                counted.insert(*id, (hash, lines));
                lines
            })
            .collect();
        // Entries that left keep no memory: bounded by the listing.
        if self.wrapped.len() > 2 * self.lines.len() + 1024 {
            let ids: HashSet<EntryId> = list.sorted.iter().copied().collect();
            self.wrapped.retain(|id, _| ids.contains(id));
        }
        self.lines_for = Some((names, list.sorted.clone(), shell.zoom));
        self.revision += 1;
        tracing::debug!(
            names = self.lines.len(),
            wrapped = wrapped_now,
            took_us = started.elapsed().as_micros() as u64,
            "grid names measured"
        );
    }

    /// An icon fills its box, like a square thumbnail does: a theme's glyph
    /// already keeps a margin inside its canvas, so an icon drawn smaller
    /// than the box floats above its name with a blank on top. The theme
    /// size asked for is never smaller than the box, so a raster theme is
    /// scaled down, not up.
    fn icon(zoom: u32) -> (f32, u16) {
        let theme = match zoom {
            0..=64 => 64,
            65..=96 => 96,
            97..=128 => 128,
            _ => 256,
        };
        (zoom as f32, theme)
    }
}

fn cell(
    shell: &mut Shell,
    theme: &Theme,
    ix: usize,
    id: EntryId,
    lines: u8,
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let zoom = shell.zoom;
    let pane = shell.pane;
    let (name, is_dir, selected, is_cursor, icon, path) = {
        let ws = shell.app.workspace.read();
        let Some(pane) = ws.pane(shell.pane) else {
            return div().into_any_element();
        };
        let Some(e) = pane.listing.get(id) else {
            return div().into_any_element();
        };
        (
            e.name_str().into_owned(),
            e.is_dir(),
            pane.selection.contains(id),
            pane.selection.cursor == Some(id),
            shell.icons.for_entry(e, GridView::icon(zoom).1),
            if e.is_dir() {
                pane.open.as_ref().and_then(|o| o.path_of(id))
            } else {
                None
            },
        )
    };
    // A filled box shows the short side of the thumbnail at full size.
    let fill = shell.grid_fill;
    let size = if zoom > 128 || (fill && zoom > 96) {
        ThumbSize::Large
    } else {
        ThumbSize::Normal
    };
    let thumb = if is_dir {
        ThumbView::Unavailable
    } else {
        shell.thumb(id, size, window, cx)
    };
    let icon_px = GridView::icon(zoom).0;
    let picture: AnyElement = match thumb {
        // Cover scales the picture past its box and leaves the clipping to us.
        ThumbView::Ready(ri) if fill => div()
            .size(px(zoom as f32))
            .overflow_hidden()
            .child(img(ri).size_full().object_fit(gpui::ObjectFit::Cover))
            .into_any_element(),
        ThumbView::Ready(ri) => img(ri)
            .max_w(px(zoom as f32))
            .max_h(px(zoom as f32))
            .object_fit(gpui::ObjectFit::Contain)
            .rounded_sm()
            .into_any_element(),
        ThumbView::Pending => div()
            .size(px(zoom as f32 * 0.6))
            .rounded_md()
            .bg(theme.placeholder)
            .into_any_element(),
        ThumbView::Unavailable => match &icon {
            Some(p) => icons::element(p, icon_px),
            None => div()
                .size(px(icon_px))
                .rounded_md()
                .bg(if is_dir {
                    theme.accent
                } else {
                    theme.text_muted
                })
                .into_any_element(),
        },
    };
    let mut el = div()
        .id(ix)
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .p(px(FRAME - 1.))
        .gap(px(LABEL_GAP))
        .rounded_md()
        .border_1()
        .border_color(if is_cursor {
            theme.accent
        } else {
            gpui::transparent_black()
        })
        .when(selected, |d| {
            d.bg(theme.selection_bg).text_color(theme.selection_text)
        })
        .when(!selected, |d| d.hover(|s| s.bg(theme.hover)))
        .when(
            shell.pending.cut && shell.pending.entries.contains(&id),
            |d| d.opacity(crate::views::list::CUT_OPACITY),
        )
        .child(
            div()
                .size(px(zoom as f32))
                .flex_none()
                .flex()
                .items_end()
                .justify_center()
                .child(picture),
        )
        .child(
            div()
                .w_full()
                .text_xs()
                .line_height(px(LABEL_LINE))
                .text_center()
                .overflow_hidden()
                .text_ellipsis_middle()
                .line_clamp(usize::from(lines))
                .child(name),
        );
    el = crate::list::entry_mouse(el, pane, ix, cx);
    el = dnd::draggable(el, pane, cx);
    if let Some(p) = path {
        el = dnd::droppable_dir(el, p, cx);
    }
    el.into_any_element()
}

impl PaneView for GridView {
    fn id(&self) -> &'static str {
        crate::view::GRID
    }

    fn render(
        &mut self,
        shell: &mut Shell,
        window: &mut Window,
        cx: &mut Context<Shell>,
    ) -> AnyElement {
        self.measure(shell, window, cx);
        let lines = self.lines.clone();
        let count = shell.ui().list.sorted.len();
        let zoom = shell.zoom;
        let pane = shell.pane;
        let render = cx.processor(
            move |this: &mut Shell, (range, cols): (Range<usize>, usize), window, cx| {
                this.with_pane(pane, |this| {
                    let theme = cx.global::<Theme>().clone();
                    this.ui_mut().list.visible_rows = range.len().div_ceil(cols.max(1)).max(1);
                    let ids: Vec<(usize, EntryId)> = range
                        .filter_map(|ix| this.ui().list.sorted.get(ix).map(|id| (ix, *id)))
                        .collect();
                    // Metadata for the whole visible range in one request.
                    let need: Vec<EntryId> = {
                        let ws = this.app.workspace.read();
                        ws.pane(pane).map_or_else(Vec::new, |p| {
                            ids.iter()
                                .map(|(_, id)| *id)
                                .filter(|id| {
                                    p.listing
                                        .get(*id)
                                        .is_some_and(|e| !e.metadata.is_hydrated())
                                })
                                .collect()
                        })
                    };
                    this.request_hydrate(need);
                    let cells = ids
                        .into_iter()
                        .map(|(ix, id)| {
                            let lines = lines.get(ix).copied().unwrap_or(1);
                            cell(this, &theme, ix, id, lines, window, cx)
                        })
                        .collect::<Vec<_>>();
                    let app = std::sync::Arc::clone(&this.app);
                    this.icons.request(&app);
                    cells
                })
            },
        );
        let grid = uniform_grid(
            ("grid", pane.0 as usize),
            count,
            px(GridView::tile_width(zoom)),
            TileHeights {
                revision: self.revision,
                of: {
                    let lines = self.lines.clone();
                    Rc::new(move |ix| {
                        GridView::tile_height(zoom, lines.get(ix).copied().unwrap_or(1))
                    })
                },
            },
            move |range, cols, window, cx| render((range, cols), window, cx),
        )
        .gap(size(px(GUTTER.0), px(GUTTER.1)))
        .max_gap(px(MAX_GUTTER))
        .track_scroll(self.scroll.clone())
        .size_full()
        .px_1();
        let marquee = crate::elements::marquee::Marquee {
            pane,
            shell: cx.weak_entity(),
            source: crate::elements::marquee::Source::Grid(self.scroll.clone()),
            count,
            scroll: self.scroll.0.borrow().base_handle.clone(),
            drag: shell.marquee.clone(),
        };
        // Ctrl and the wheel resize the tiles. Taken in the capture phase, so
        // the grid under it does not also scroll.
        let shell_handle = cx.weak_entity();
        let wheel = gpui::canvas(
            |bounds, window, _| window.insert_hitbox(bounds, gpui::HitboxBehavior::Normal),
            move |_, hitbox, window, _| {
                window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, cx| {
                    if !phase.capture()
                        || !event.modifiers.control
                        || !hitbox.should_handle_scroll(window)
                    {
                        return;
                    }
                    let delta = event.delta.pixel_delta(window.line_height()).y;
                    let lines = matches!(event.delta, gpui::ScrollDelta::Lines(_));
                    shell_handle
                        .update(cx, |shell, cx| {
                            shell.zoom_wheel(f32::from(delta), lines, cx);
                        })
                        .ok();
                    cx.stop_propagation();
                });
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();
        div()
            .relative()
            .size_full()
            .child(grid)
            .child(marquee)
            .child(wheel)
            .child(crate::elements::scrollbar::vertical(
                ("grid-scrollbar", pane.0 as usize),
                crate::elements::scrollbar::Scrolls::Offset(
                    self.scroll.0.borrow().base_handle.clone(),
                ),
            ))
            .into_any_element()
    }

    fn scroll_to(&mut self, ix: usize) {
        self.scroll.scroll_to_item(ix);
    }

    fn nav_delta(&self, nav: Nav, _: &Shell) -> Option<isize> {
        // Geometry from the last prepaint, never from a measured width.
        let cols = self.scroll.cols() as isize;
        let page = (self.scroll.rows_visible().saturating_sub(1).max(1) as isize) * cols;
        match nav {
            Nav::Up => Some(-cols),
            Nav::Down => Some(cols),
            Nav::Left => Some(-1),
            Nav::Right => Some(1),
            Nav::PageUp => Some(-page),
            Nav::PageDown => Some(page),
            Nav::Home | Nav::End => None,
        }
    }

    fn reset(&mut self) {
        self.scroll.scroll_to_item_strict(0);
    }
}
