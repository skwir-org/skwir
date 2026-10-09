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
use gpui::{AnyElement, Context, Window, div, img, prelude::*, px, size};

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
    cx: &mut Context<Shell>,
) -> AnyElement {
    let zoom = shell.zoom;
    let (look, pictures) = {
        let app = std::sync::Arc::clone(&shell.app);
        let ws = app.workspace.read();
        let Some(pane) = ws.pane(shell.pane) else {
            return div().into_any_element();
        };
        let (Some(e), Some(look)) = (pane.entry(id), shell.look(pane, id, GridView::icon(zoom).1))
        else {
            return div().into_any_element();
        };
        (look, mosaic_of(e))
    };
    // A filled box shows the short side of the thumbnail at full size.
    let fill = shell.prefs.grid_fill;
    let size = if zoom > 128 || (fill && zoom > 96) {
        ThumbSize::Large
    } else {
        ThumbSize::Normal
    };
    let thumb = if look.is_dir {
        ThumbView::Unavailable
    } else {
        shell.thumb(id, size)
    };
    let icon_px = GridView::icon(zoom).0;
    let picture: AnyElement = match thumb {
        // Cover scales the picture past its box and leaves the clipping to us.
        ThumbView::Ready(ri) if fill => div()
            .size(px(zoom as f32))
            .overflow_hidden()
            .child(img(ri).size_full().object_fit(gpui::ObjectFit::Cover))
            .into_any_element(),
        ThumbView::Ready(ri) => {
            let own = ri.size(0);
            let (width, height) = fitted(
                (own.width.0.max(1) as f32, own.height.0.max(1) as f32),
                zoom as f32,
            );
            img(ri)
                .w(px(width))
                .h(px(height))
                .object_fit(gpui::ObjectFit::Contain)
                .rounded_sm()
                .into_any_element()
        }
        ThumbView::Pending => div()
            .size(px(zoom as f32 * 0.6))
            .rounded_md()
            .bg(theme.placeholder)
            .into_any_element(),
        // Marked below, with the thumbnail's mark.
        ThumbView::Unavailable => icons::entry_picture(
            look.icon.as_ref(),
            look.is_dir,
            icon_px,
            icons::Mark::None,
            theme,
        ),
    };
    // A place with pictures (a tag's) shows a few of them.
    let picture = match &look.place {
        Some((_, Some(color))) if !pictures.is_empty() => {
            mosaic(shell, pictures, zoom as f32, size, *color, theme)
        }
        _ => picture,
    };
    let el = crate::views::entry::highlight(
        div()
            .id(ix)
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .p(px(FRAME - 1.))
            .gap(px(LABEL_GAP))
            .rounded_md(),
        &look,
        theme,
    )
    .child(
        div()
            .size(px(zoom as f32))
            .flex_none()
            .flex()
            .items_end()
            .justify_center()
            .child(
                div()
                    .relative()
                    .flex_none()
                    .child(icons::marked(picture, icon_px, look.mark(), theme))
                    .children(crate::tags::tile_dots(
                        ("tile-tags", ix),
                        &look.tags,
                        zoom as f32,
                        theme,
                        shell.font.clone(),
                    )),
            ),
    )
    .child(crate::elements::clip::clipped(
        div()
            .id(("label", ix))
            .w_full()
            .text_xs()
            .line_height(px(LABEL_LINE))
            .text_center()
            .overflow_hidden()
            .text_ellipsis_middle()
            .line_clamp(usize::from(lines)),
        look.name.clone(),
        shell.font.clone(),
    ));
    crate::views::entry::behave(el, &look, shell, cx).into_any_element()
}

/// A thumbnail's size on its tile: its own, shrunk until its longer side
/// is the box's, never enlarged. Given whole, not left to the image's
/// `max_w` and `max_h`: an image keeps its own aspect ratio over its
/// `max_h`, so a portrait taller than its box was clamped in width only
/// and stood over the name above it, out of its own highlight.
fn fitted((width, height): (f32, f32), side: f32) -> (f32, f32) {
    let scale = (side / width).min(side / height).min(1.);
    (width * scale, height * scale)
}

/// The pictures a place's tile shows (`keys::PLACE_PICTURES`): each one's
/// identity and path, and when it was modified.
fn mosaic_of(
    e: &bibli_core::Entry,
) -> Vec<(bibli_core::op::EntryRef, Option<std::time::SystemTime>)> {
    use bibli_core::Value;
    let Some(Value::List(items)) = e.metadata.value(bibli_core::entry::keys::PLACE_PICTURES) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let Value::List(parts) = item else {
                return None;
            };
            let [path, dev, ino, modified] = parts.as_slice() else {
                return None;
            };
            let id = EntryId {
                dev: dev.as_u64()?,
                ino: ino.as_u64()?,
            };
            let path = std::path::PathBuf::from(path.as_str()?);
            Some((bibli_core::op::EntryRef::new(id, path), modified.as_time()))
        })
        .collect()
}

/// A place's pictures on its tile, as a thumbnail fills a box: one alone,
/// else four quarters, those it lacks left blank; framed in its tag's
/// color, so a place is never taken for one of its files.
#[allow(
    clippy::too_many_arguments,
    reason = "what one tile draws, read once by its caller"
)]
fn mosaic(
    shell: &mut Shell,
    pictures: Vec<(bibli_core::op::EntryRef, Option<std::time::SystemTime>)>,
    side: f32,
    quality: ThumbSize,
    color: usize,
    theme: &Theme,
) -> AnyElement {
    const GAP: f32 = 2.;
    const FRAME: f32 = 3.;
    let one = pictures.len() == 1;
    let inner = side - 2. * FRAME;
    let cell = if one { inner } else { (inner - GAP) / 2. };
    let mut cells: Vec<AnyElement> = pictures
        .into_iter()
        .take(4)
        .map(|(entry, mtime)| {
            let frame = div()
                .size(px(cell))
                .flex_none()
                .overflow_hidden()
                .rounded_sm()
                .bg(theme.placeholder);
            match shell.thumb_of(entry, mtime, quality) {
                ThumbView::Ready(ri) => frame
                    .child(img(ri).size_full().object_fit(gpui::ObjectFit::Cover))
                    .into_any_element(),
                _ => frame.into_any_element(),
            }
        })
        .collect();
    while !one && cells.len() < 4 {
        cells.push(
            div()
                .size(px(cell))
                .flex_none()
                .rounded_sm()
                .bg(theme.hover)
                .into_any_element(),
        );
    }
    div()
        .size(px(side))
        .p(px(FRAME - 1.))
        .rounded_md()
        .border_1()
        .border_color(theme.tag(color))
        .bg(theme.tag_fill(color))
        .child(
            div()
                .size(px(inner))
                .flex()
                .flex_wrap()
                .gap(px(GAP))
                .children(cells),
        )
        .into_any_element()
}

impl PaneView for GridView {
    fn zooms(&self) -> crate::view::Zooms {
        crate::view::Zooms::Tiles
    }

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
            move |this: &mut Shell, (range, cols): (Range<usize>, usize), _, cx| {
                this.with_pane(pane, |this| {
                    let theme = cx.global::<Theme>().clone();
                    let list = &mut this.ui_mut().list;
                    list.visible_rows = range.len().div_ceil(cols.max(1)).max(1);
                    list.visible_items = range.len();
                    let ids: Vec<(usize, EntryId)> = range
                        .filter_map(|ix| this.ui().list.sorted.get(ix).map(|id| (ix, *id)))
                        .collect();
                    // Metadata for the whole visible range in one request.
                    this.request_hydrate(ids.iter().map(|(_, id)| *id).collect::<Vec<_>>());
                    let cells = ids
                        .into_iter()
                        .map(|(ix, id)| {
                            let lines = lines.get(ix).copied().unwrap_or(1);
                            cell(this, &theme, ix, id, lines, cx)
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
        // Ctrl and the wheel resize the tiles.
        let wheel = crate::views::zoom_wheel(cx);
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

    fn scroll_anchor(&self) -> Option<(usize, f32)> {
        Some(self.scroll.anchor())
    }

    fn restore_anchor(&mut self, anchor: (usize, f32)) {
        self.scroll.scroll_to_anchor(anchor);
    }

    fn nav_delta(&self, nav: Nav, _: &Shell) -> Option<isize> {
        // Geometry from the last prepaint, never from a measured width.
        let cols = self.scroll.cols() as isize;
        let rows = self.scroll.rows_visible();
        let page = (rows.saturating_sub(1).max(1) as isize) * cols;
        // Whole rows, so the cursor stays in its column.
        let half = ((rows / 2).max(1) as isize) * cols;
        match nav {
            Nav::Up => Some(-cols),
            Nav::Down => Some(cols),
            Nav::Left => Some(-1),
            Nav::Right => Some(1),
            Nav::PageUp => Some(-page),
            Nav::PageDown => Some(page),
            Nav::HalfPageUp => Some(-half),
            Nav::HalfPageDown => Some(half),
            Nav::Home | Nav::End => None,
        }
    }

    fn reset(&mut self) {
        self.scroll.scroll_to_item_strict(0);
    }
}

#[cfg(test)]
mod tests {
    use super::fitted;

    #[test]
    fn a_thumbnail_fits_its_box_whatever_its_shape() {
        // A normal thumbnail (128 px) at the 96 px step: a portrait is as
        // tall as the box, never taller, a landscape as wide.
        assert_eq!(fitted((85., 128.), 96.), (63.75, 96.));
        assert_eq!(fitted((128., 85.), 96.), (96., 63.75));
        // A large one (256 px) at the 192 px step, a strip at 64 px.
        assert_eq!(fitted((192., 256.), 192.), (144., 192.));
        assert_eq!(fitted((43., 128.), 64.), (21.5, 64.));
        // A picture smaller than the box stays its size.
        assert_eq!(fitted((40., 60.), 128.), (40., 60.));
    }
}
