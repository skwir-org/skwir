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
//! system's line wrapper, once per change of the order drawn (arrivals
//! count once placed in it), the zoom or the font, never per frame. The
//! label is then clamped to that count, so a wrapper that disagrees with
//! the shaper by a word costs an ellipsis, not a name drawn over the next
//! row. A name cut keeps its first lines whole and its end on the last
//! (vendored patch 0037: Zed's gpui cuts a clamped text in the middle to a
//! single line).
//!
//! Under the name, a line for each caption the user chose (`CAPTIONS`, off
//! until chosen): its tags, or one of the list's details. Every tile has
//! the same lines, said or blank, so its height is still its name's and
//! the captions', never what its metadata holds.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::rc::Rc;

use gpui::{AnyElement, Context, Window, div, prelude::*, px, size};
use skwir_core::EntryId;
use skwir_core::thumbs::ThumbSize;

use crate::elements::uniform_grid::{TileHeights, UniformGridScrollHandle, uniform_grid};
use crate::elements::zoom_wheel::{self, Zoomed};
use crate::icons;
use crate::shell::Shell;
use crate::theme::Theme;
use crate::thumbs::ThumbView;
use crate::view::{Nav, PaneView};
use crate::views::list::{COLUMNS, Detail, PerColumn, RowData};

/// Zoom steps for `ui.zoom`: the side of a tile's box.
pub const ZOOMS: [u32; 5] = [64, 96, 128, 192, 256];
/// The step a new window opens at, and Ctrl+0 returns to.
pub const DEFAULT_ZOOM: u32 = 128;

/// What a tile can say under its name, a line each: its tags, or one of
/// the list's details (`views::list::COLUMNS`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Caption {
    Tags,
    Detail(Detail),
}

/// A caption: everything about it, once. Settings, the View menu and the
/// tiles read `CAPTIONS`.
pub struct CaptionSpec {
    pub caption: Caption,
    /// Whether tiles say it (`ui.grid_caption_*`), off until chosen.
    pub setting: &'static str,
    /// What the View menu calls it.
    pub label: &'static str,
    /// What Settings calls it.
    pub title: &'static str,
    pub description: &'static str,
}

/// The captions in the order they stand under a name: the tags, then the
/// details as the list's columns stand.
pub const CAPTIONS: [CaptionSpec; 7] = [
    CaptionSpec {
        caption: Caption::Tags,
        setting: "ui.grid_caption_tags",
        label: "Tags",
        title: "Tags under names",
        description: "Under each name in the grid, a line of its tags, as many as fit, then dots. The picture is then left to itself.",
    },
    CaptionSpec {
        caption: Caption::Detail(Detail::Folder),
        setting: "ui.grid_caption_folder",
        label: "Folder",
        title: "Folder under names",
        description: "Under each name in the grid, the folder it is in, where files from many folders are listed: a search, a tag, the shelf, Recent.",
    },
    CaptionSpec {
        caption: Caption::Detail(Detail::Deleted),
        setting: "ui.grid_caption_deleted",
        label: "Deleted",
        title: "Deletion under names",
        description: "Under each name in the grid, when it was put in the trash, where the trash's files are listed.",
    },
    CaptionSpec {
        caption: Caption::Detail(Detail::Size),
        setting: "ui.grid_caption_size",
        label: "Size",
        title: "Size under names",
        description: "Under each name in the grid, the size of the file, or how many files a place holds.",
    },
    CaptionSpec {
        caption: Caption::Detail(Detail::Modified),
        setting: "ui.grid_caption_modified",
        label: "Modified",
        title: "Modification under names",
        description: "Under each name in the grid, when it last changed.",
    },
    CaptionSpec {
        caption: Caption::Detail(Detail::Type),
        setting: "ui.grid_caption_type",
        label: "Type",
        title: "Type under names",
        description: "Under each name in the grid, the kind of file, such as PDF document.",
    },
    CaptionSpec {
        caption: Caption::Detail(Detail::Rating),
        setting: "ui.grid_caption_rating",
        label: "Rating",
        title: "Rating under names",
        description: "Under each name in the grid, the stars it was given, where its notes are kept.",
    },
];

impl Shell {
    /// Whether `caption` can be said here: the tags while a store keeps
    /// them, a detail where its column can be shown
    /// (`Shell::column_applies`).
    pub fn caption_applies(&self, caption: Caption) -> bool {
        match caption {
            Caption::Tags => self.app.notes.held().tags,
            Caption::Detail(detail) => self.column_applies(detail),
        }
    }

    /// The captions the grid's tiles say here: those chosen that apply.
    pub fn grid_captions(&self) -> Vec<Caption> {
        CAPTIONS
            .iter()
            .zip(&self.captions)
            .filter(|(spec, chosen)| **chosen && self.caption_applies(spec.caption))
            .map(|(spec, _)| spec.caption)
            .collect()
    }

    /// `ui.grid_caption`: say one caption under the names or no longer,
    /// a preference saved as the dialog's switch is.
    pub fn toggle_grid_caption(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(at) = CAPTIONS.iter().position(|spec| spec.setting == key) else {
            return;
        };
        let shown = !self.captions[at];
        self.captions[at] = shown;
        cx.notify();
        crate::settings::remember(self, CAPTIONS[at].setting, shown.to_string(), cx);
    }
}

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
    /// What `lines` was counted for: the order drawn and the zoom. Not the
    /// listing's versions: hydration advances one with every batch, and
    /// arrivals the names' with every batch while a folder streams in,
    /// though they are drawn only once placed in the order (`stale`).
    lines_for: Option<(std::sync::Arc<[EntryId]>, u32)>,
    /// Advances with `lines` and `captions`, so the grid sums its rows
    /// again.
    revision: u64,
    /// How many captions the tiles were counted with.
    captions: usize,
}

/// Whether the lines counted for `last` (an order and a zoom) are not
/// those of `order` at `zoom`. The order is the listing's sort as placed
/// (`Listing::sorted`): a name changed in place sorts again, a new order;
/// arrivals change nothing drawn until placed, which they are by quarters
/// while a folder streams in. So a frame that brings only arrivals or
/// metadata counts nothing.
fn stale(
    last: Option<&(std::sync::Arc<[EntryId]>, u32)>,
    order: &std::sync::Arc<[EntryId]>,
    zoom: u32,
) -> bool {
    !last.is_some_and(|(counted, at)| {
        *at == zoom && (std::sync::Arc::ptr_eq(counted, order) || **counted == **order)
    })
}

/// Border and padding around a tile's content, on each side.
const FRAME: f32 = 5.;
/// Between the picture and the name.
const LABEL_GAP: f32 = 2.;
/// A tile's height is counted in lines of name, so the line height is fixed.
const LABEL_LINE: f32 = 15.;
/// A name takes what it needs up to this, then ends with an ellipsis.
const MAX_LINES: u8 = 3;
/// A caption's line, as tall as a tag's chip.
const CAPTION_LINE: f32 = crate::tags::CHIP_HEIGHT;
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

    fn tile_height(zoom: u32, lines: u8, captions: usize) -> f32 {
        let captions = if captions == 0 {
            0.
        } else {
            LABEL_GAP + captions as f32 * CAPTION_LINE
        };
        zoom as f32 + 2. * FRAME + LABEL_GAP + f32::from(lines) * LABEL_LINE + captions
    }

    /// Count the lines every name wraps to, when what they depend on changed.
    fn measure(&mut self, shell: &Shell, window: &Window, cx: &gpui::App) {
        let list = &shell.ui().list;
        // Names wrap in the interface font, which may change while shown
        // (ADR-073).
        let font = cx.global::<crate::theme::Theme>().font.clone();
        let at = (shell.zoom, font.clone());
        if self.wrapped_at.as_ref() == Some(&at)
            && !stale(self.lines_for.as_ref(), &list.sorted, shell.zoom)
        {
            return;
        }
        let ws = shell.app.workspace.read();
        let listing = ws.pane(shell.pane).map(|pane| &pane.listing);
        let started = std::time::Instant::now();
        if self.wrapped_at.as_ref() != Some(&at) {
            self.wrapped.clear();
            self.wrapped_at = Some(at);
        }
        let font_size = window.rem_size() * 0.75; // text_xs
        let mut wrapper = cx.text_system().line_wrapper(gpui::font(font), font_size);
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
                // As the label wraps: a newline ends a line, and no line that
                // goes on is indented.
                let lines = entry
                    .name_str()
                    .split('\n')
                    .map(|line| {
                        wrapper
                            .wrap_line(
                                &[gpui::LineFragment::text(line)],
                                width,
                                gpui::IndentAdjustment::NoIndent,
                            )
                            .take(usize::from(MAX_LINES) - 1)
                            .count()
                            + 1
                    })
                    .sum::<usize>()
                    .min(usize::from(MAX_LINES)) as u8;
                counted.insert(*id, (hash, lines));
                lines
            })
            .collect();
        // Entries that left keep no memory: bounded by the listing.
        if self.wrapped.len() > 2 * self.lines.len() + 1024 {
            let ids: HashSet<EntryId> = list.sorted.iter().copied().collect();
            self.wrapped.retain(|id, _| ids.contains(id));
        }
        self.lines_for = Some((list.sorted.clone(), shell.zoom));
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
    row: &RowData,
    lines: u8,
    captions: &[Caption],
    cx: &mut Context<Shell>,
) -> AnyElement {
    let zoom = shell.zoom;
    let (ix, look, id) = (row.ix, &row.look, row.look.id);
    let act = Shell::tag_act(shell.pane, Some(id), false, cx);
    let pictures = shell
        .app
        .workspace
        .read()
        .pane(shell.pane)
        .and_then(|pane| pane.entry(id))
        .map(mosaic_of)
        .unwrap_or_default();
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
        ThumbView::Ready(thumb) if fill => div()
            .size(px(zoom as f32))
            .overflow_hidden()
            .child(
                thumb
                    .img(theme)
                    .size_full()
                    .object_fit(gpui::ObjectFit::Cover),
            )
            .into_any_element(),
        ThumbView::Ready(thumb) => {
            let own = thumb.image.size(0);
            let (width, height) = fitted(
                (own.width.0.max(1) as f32, own.height.0.max(1) as f32),
                zoom as f32,
            );
            thumb
                .img(theme)
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
    // Known by its entry: what is inside it, by its place in it.
    let el = crate::views::entry::highlight(
        div()
            .id(crate::elements::known_by("tile", look.id))
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .p(px(FRAME - 1.))
            .gap(px(LABEL_GAP))
            .rounded_md(),
        look,
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
                    // Over the picture's top edge, as wide as the picture:
                    // the tile keeps the height its name gives it. Said
                    // under the name instead when that is chosen.
                    .when(
                        !look.tags.is_empty() && !captions.contains(&Caption::Tags),
                        |d| {
                            d.child(
                                div()
                                    .absolute()
                                    .top(px(-3.))
                                    .left(px(-3.))
                                    .right(px(-3.))
                                    .flex()
                                    .child(crate::views::entry::tags(
                                        look,
                                        crate::tags::StripOn::Picture { zoom: zoom as f32 },
                                        theme,
                                        Some(act.clone()),
                                    )),
                            )
                        },
                    ),
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
    ))
    .when(!captions.is_empty(), |d| {
        d.child(caption_lines(row, captions, theme, act.clone()))
    });
    let seat = crate::views::entry::Seat {
        index: ix,
        tree: None,
    };
    crate::views::entry::behave(el, look, seat, shell, cx).into_any_element()
}

/// A tile's captions under its name, a line each, said or blank.
fn caption_lines(
    row: &RowData,
    captions: &[Caption],
    theme: &Theme,
    act: crate::tags::TagAct,
) -> gpui::Div {
    let look = &row.look;
    let muted = if look.selected {
        theme.selection_text
    } else {
        theme.text_muted
    };
    div()
        .w_full()
        .flex()
        .flex_col()
        .children(captions.iter().enumerate().map(|(k, caption)| {
            let line = div()
                .h(px(CAPTION_LINE))
                .w_full()
                .min_w_0()
                .flex()
                .items_center()
                .justify_center()
                .overflow_hidden();
            let text = |text: &str, whole: &str| {
                crate::elements::clip::clipped_with(
                    div()
                        .id(("caption", row.ix * CAPTIONS.len() + k))
                        .min_w_0()
                        .truncate()
                        .text_xs()
                        .text_color(muted),
                    text.to_owned(),
                    (!whole.is_empty()).then(|| whole.to_owned().into()),
                )
            };
            match caption {
                Caption::Tags => line.child(
                    crate::views::entry::tags(
                        look,
                        crate::tags::StripOn::Row {
                            selected: look.selected,
                        },
                        theme,
                        Some(act.clone()),
                    )
                    .centered(),
                ),
                Caption::Detail(Detail::Rating) => line.children(
                    row.rating
                        .map(|stars| crate::tags::stars(stars, 12., theme)),
                ),
                Caption::Detail(Detail::Folder) => {
                    let (name, whole) = row.folder.clone().unwrap_or_default();
                    line.child(text(&name, &whole))
                }
                Caption::Detail(Detail::Size) => line.child(text(&row.size, "")),
                Caption::Detail(Detail::Modified) => {
                    line.child(text(&row.modified, &row.modified_exact))
                }
                Caption::Detail(Detail::Deleted) => {
                    line.child(text(&row.deleted, &row.deleted_exact))
                }
                Caption::Detail(Detail::Type) => line.child(text(&row.kind, "")),
            }
        }))
}

/// The details `captions` say, as the list's columns would.
fn caption_columns(captions: &[Caption]) -> PerColumn<bool> {
    let mut columns = PerColumn([false; COLUMNS.len()]);
    for caption in captions {
        if let Caption::Detail(detail) = caption {
            columns[*detail] = true;
        }
    }
    columns
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
    e: &skwir_core::Entry,
) -> Vec<(skwir_core::op::EntryRef, Option<std::time::SystemTime>)> {
    use skwir_core::Value;
    let Some(Value::List(items)) = e.metadata.value(skwir_core::entry::keys::PLACE_PICTURES) else {
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
            let id = skwir_core::FileId::new(dev.as_u64()?, ino.as_u64()?);
            let path = std::path::PathBuf::from(path.as_str()?);
            Some((skwir_core::op::EntryRef::new(id, path), modified.as_time()))
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
    pictures: Vec<(skwir_core::op::EntryRef, Option<std::time::SystemTime>)>,
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
                ThumbView::Ready(thumb) => frame
                    .child(
                        thumb
                            .img(theme)
                            .size_full()
                            .object_fit(gpui::ObjectFit::Cover),
                    )
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

    fn role(&self) -> gpui::Role {
        gpui::Role::ListBox
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
        let captions: Rc<[Caption]> = shell.grid_captions().into();
        if captions.len() != self.captions {
            self.captions = captions.len();
            self.revision += 1;
        }
        let lines = self.lines.clone();
        let count = shell.ui().list.sorted.len();
        let zoom = shell.zoom;
        let pane = shell.pane;
        let said = Rc::clone(&captions);
        let render = cx.processor(
            move |this: &mut Shell, (range, cols): (Range<usize>, usize), _, cx| {
                this.with_pane(pane, |this| {
                    let theme = cx.global::<Theme>().clone();
                    let list = &mut this.ui_mut().list;
                    list.visible_rows = range.len().div_ceil(cols.max(1)).max(1);
                    list.visible_items = range.len();
                    let ids: Vec<EntryId> = range
                        .clone()
                        .filter_map(|ix| this.ui().list.sorted.get(ix).copied())
                        .collect();
                    // Metadata for the whole visible range in one request.
                    this.request_hydrate(ids);
                    // What every tile shows, and the details its captions
                    // say, read as the list reads its rows.
                    let rows = this.rows_showing(
                        range.clone(),
                        GridView::icon(zoom).1,
                        caption_columns(&said),
                    );
                    // An entry gone between the order's reading and its
                    // own is a blank: the tiles after it keep their places.
                    let mut rows = rows.iter().peekable();
                    let cells = range
                        .map(|ix| match rows.next_if(|row| row.ix == ix) {
                            Some(row) => {
                                let lines = lines.get(ix).copied().unwrap_or(1);
                                cell(this, &theme, row, lines, &said, cx)
                            }
                            None => div().into_any_element(),
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
                    let captions = captions.len();
                    Rc::new(move |ix| {
                        GridView::tile_height(zoom, lines.get(ix).copied().unwrap_or(1), captions)
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
        let wheel = zoom_wheel::overlay(Zoomed::View(pane), cx);
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
    use std::sync::Arc;

    use skwir_core::{EntryId, FileId};

    use super::{CAPTIONS, COLUMNS, Caption, Detail, fitted, stale};

    /// Names are counted again for another order or zoom, never for a
    /// frame that brought only arrivals not yet placed, or metadata.
    #[test]
    fn names_are_counted_for_the_order_drawn_and_its_zoom() {
        let id = |ino| EntryId {
            file: FileId::new(1, ino),
            at: ino,
        };
        let order: Arc<[EntryId]> = Arc::from([id(1), id(2), id(3)]);
        assert!(stale(None, &order, 96), "nothing counted yet");
        let counted = (Arc::clone(&order), 96);
        assert!(!stale(Some(&counted), &order, 96), "the same order");
        let again: Arc<[EntryId]> = Arc::from([id(1), id(2), id(3)]);
        assert!(!stale(Some(&counted), &again, 96), "sorted alike again");
        let placed: Arc<[EntryId]> = Arc::from([id(1), id(4), id(2), id(3)]);
        assert!(stale(Some(&counted), &placed, 96), "arrivals placed");
        let resorted: Arc<[EntryId]> = Arc::from([id(3), id(2), id(1)]);
        assert!(stale(Some(&counted), &resorted, 96), "another order");
        assert!(stale(Some(&counted), &order, 128), "another zoom");
    }

    /// The captions are the tags, then the list's columns as they stand,
    /// each called and kept as its column is: a column added to the list
    /// is a caption the grid offers.
    #[test]
    fn the_captions_are_the_tags_then_the_lists_columns() {
        assert_eq!(CAPTIONS[0].caption, Caption::Tags);
        let details: Vec<Detail> = CAPTIONS[1..]
            .iter()
            .map(|spec| match spec.caption {
                Caption::Detail(detail) => detail,
                Caption::Tags => panic!("the tags once, first"),
            })
            .collect();
        let columns: Vec<Detail> = COLUMNS.iter().map(|column| column.detail).collect();
        assert_eq!(details, columns);
        for (spec, column) in CAPTIONS[1..].iter().zip(&COLUMNS) {
            assert_eq!(spec.label, column.label);
            assert_eq!(spec.setting, format!("ui.grid_caption_{}", column.key));
        }
    }

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
