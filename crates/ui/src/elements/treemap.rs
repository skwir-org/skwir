//! A treemap element: squarify runs in `prepaint` on the element's real
//! bounds; `paint` draws tiles and labels and installs mouse handlers on
//! the element's hitbox for hover and click.
//!
//! Each block is a tile with rounded corners, a gap of the ground between
//! it and the next. An item with children is drawn open while its tile
//! has room for them: a tray, its name and size in a header on top, its
//! children laid out below. Whether it has room is decided here, from the
//! bounds prepaint was given, never from a size measured in `render`.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    App, BorderStyle, Bounds, ContentMask, Corners, DispatchPhase, Edges, Element, ElementId, Font,
    FontWeight, GlobalElementId, Hitbox, HitboxBehavior, Hsla, InspectorElementId, IntoElement,
    LayoutId, MouseButton, MouseDownEvent, MouseExitEvent, MouseMoveEvent, Pixels, Point,
    ShapedLine, SharedString, Style, TextRun, Window, pattern_slash, point, px, quad, relative,
    size,
};

use crate::treemap::{Rect, squarify};

/// One block to lay out. `children` are laid out inside it, as deep as
/// they go and the blocks have room.
pub struct TreemapItem {
    /// Identifies the block in hover/click callbacks.
    pub key: usize,
    pub weight: f64,
    pub label: SharedString,
    /// What it weighs, said after its name where there is room: beside it
    /// in an open block's header, under it in a closed one.
    pub detail: SharedString,
    pub color: Hsla,
    /// Its fill drawn open, its children laid on it.
    pub tray: Hsla,
    /// Its label's color, on `color` and on `tray`.
    pub text: Hsla,
    /// Striped: its size is a lower bound, still being counted.
    pub hatched: bool,
    /// A dashed line inside its edge: its room can be had back.
    pub dashed: bool,
    /// Ringed: chosen.
    pub selected: bool,
    pub children: Vec<TreemapItem>,
}

/// The state colors a treemap draws with (`theme.rs`).
#[derive(Clone, Copy)]
pub struct TreemapStyle {
    /// The ground, between blocks and around a chosen one's ring.
    pub gap: Hsla,
    /// Laid over the block under the pointer, and the line inside its edge.
    pub hover: Hsla,
    pub hover_edge: Hsla,
    /// The ring around a chosen block.
    pub selected: Hsla,
    /// The dashed line inside a block whose room can be had back.
    pub dashed: Hsla,
}

type HoverFn = Rc<dyn Fn(Option<usize>, &mut Window, &mut App)>;
type ClickFn = Rc<dyn Fn(usize, &MouseDownEvent, &mut Window, &mut App)>;

/// Where the top level's blocks were last laid out, by key: what a view
/// reads for keyboard navigation, the one later read of prepaint allowed.
pub type Laidout = Rc<RefCell<Vec<(usize, Rect)>>>;

pub struct Treemap {
    id: ElementId,
    /// Shared: a view keeps what it built while nothing it was built from
    /// changed, and hands it again at each frame.
    items: Rc<Vec<TreemapItem>>,
    style: TreemapStyle,
    /// The key currently reported as hovered, so the callback fires on change only.
    hovered: Option<usize>,
    record: Option<Laidout>,
    on_hover: HoverFn,
    on_click: ClickFn,
}

pub fn treemap(
    id: impl Into<ElementId>,
    items: Rc<Vec<TreemapItem>>,
    style: TreemapStyle,
    on_hover: impl Fn(Option<usize>, &mut Window, &mut App) + 'static,
    on_click: impl Fn(usize, &MouseDownEvent, &mut Window, &mut App) + 'static,
) -> Treemap {
    Treemap {
        id: id.into(),
        items,
        style,
        hovered: None,
        record: None,
        on_hover: Rc::new(on_hover),
        on_click: Rc::new(on_click),
    }
}

impl Treemap {
    /// The block under the pointer, as the last hover callback said.
    pub fn hovered(mut self, key: Option<usize>) -> Self {
        self.hovered = key;
        self
    }

    /// Record the top level's blocks in `laid` at each prepaint.
    pub fn record(mut self, laid: Laidout) -> Self {
        self.record = Some(laid);
        self
    }
}

/// Where a block is, for hit-testing: its tile and half the gaps around
/// it, so the pointer between two tiles is on one of them.
#[derive(Clone, Copy)]
struct Block {
    key: usize,
    bounds: Bounds<Pixels>,
    depth: usize,
}

struct Laid {
    block: Block,
    /// What is drawn, and its corners.
    tile: Bounds<Pixels>,
    radius: Pixels,
    color: Hsla,
    hatched: bool,
    dashed: bool,
    selected: bool,
    /// Its name, and what it weighs: beside the name in a header, or
    /// under it; each where it is drawn.
    name: Option<(ShapedLine, Point<Pixels>)>,
    detail: Option<(ShapedLine, Point<Pixels>)>,
}

pub struct TreemapPrepaint {
    blocks: Rc<Vec<Laid>>,
    /// Where each block is, for the pointer.
    hits: Rc<Vec<Block>>,
    hitbox: Hitbox,
}

/// What a treemap laid out, kept between frames by what it was laid out
/// from (its items, as shared, its bounds, the font and the scale): a
/// frame drawn as the pointer moves over the map (its hover) squarifies
/// nothing, and cuts no label to its block, again.
struct Kept {
    from: (usize, Bounds<Pixels>, SharedString, u32),
    blocks: Rc<Vec<Laid>>,
    hits: Rc<Vec<Block>>,
}

/// Between two tiles, and between a tray's edge and the tiles on it.
const GAP: f64 = 3.0;
/// A tray's header, above its children, holding its name and size.
const HEADER: f64 = 22.0;
/// A tile's corners, by depth: smaller inside.
const RADII: [f32; 3] = [6., 5., 4.];
/// A name, and what it weighs.
const FONT: f32 = 11.5;
const SMALL: f32 = 11.0;
/// A label's line, and its inset from its tile's sides.
const LINE: f64 = 15.0;
const INSET: f64 = 7.0;
/// Where a name's line begins below a tile's top, and a header's.
const TOP: f64 = 5.0;
const HEADER_TOP: f64 = 4.0;
/// Narrower or lower than this, a tile says nothing.
const LABEL_MIN_W: f64 = 40.0;
const LABEL_MIN_H: f64 = 17.0;
/// Lower than this, a tile says no size under its name.
const DETAIL_MIN_H: f64 = 36.0;
/// A name cut to fewer characters than this says nothing ("gar…g"), and
/// is not drawn; a header's, beside its size, may be shorter.
const LEAST: usize = 6;
const LEAST_HEADER: usize = 4;
/// A tray's children need at least this much room.
const OPEN_MIN_W: f64 = 40.0;
const OPEN_MIN_H: f64 = 28.0;
/// Between a header's name and the size beside it.
const BESIDE: f32 = 8.0;
/// The ring around a chosen tile, outside it, a band of the ground
/// between them as wide.
const RING: f32 = 2.0;

/// A name's weight: a closed tile's, or a header's, heavier.
struct Weight {
    font: Font,
    wrapper: gpui::LineWrapperHandle,
    least: usize,
}

/// Shapes labels, names cut in the middle to the width they get: the end
/// of a name tells files apart.
struct Labels<'a> {
    window: &'a Window,
    tile: Weight,
    header: Weight,
    /// What a block weighs.
    font: Font,
}

fn run(font: &Font, len: usize, color: Hsla) -> TextRun {
    TextRun {
        len,
        font: font.clone(),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
        position: gpui::RunPosition::Baseline,
    }
}

impl Labels<'_> {
    /// `text` shaped to fit `width`, in a header's weight or a tile's,
    /// if enough of it does to say something.
    fn name(
        &mut self,
        text: &SharedString,
        color: Hsla,
        width: f32,
        header: bool,
    ) -> Option<ShapedLine> {
        if text.is_empty() || width < FONT {
            return None;
        }
        let weight = if header {
            &mut self.header
        } else {
            &mut self.tile
        };
        let runs = vec![run(&weight.font, text.len(), color)];
        let (cut, runs) = weight.wrapper.truncate_line(
            text.clone(),
            px(width),
            "…",
            &runs,
            gpui::TruncateFrom::Middle,
        );
        // What is left of a name cut is its characters but the ellipsis.
        if cut != *text && cut.chars().count() <= weight.least {
            return None;
        }
        let runs = runs.into_owned();
        Some(
            self.window
                .text_system()
                .shape_line(cut, px(FONT), &runs, None),
        )
    }

    /// `text` whole, when it fits `width`.
    fn whole(&mut self, text: &SharedString, color: Hsla, width: f32) -> Option<ShapedLine> {
        if text.is_empty() {
            return None;
        }
        let line = self.window.text_system().shape_line(
            text.clone(),
            px(SMALL),
            &[run(&self.font, text.len(), color)],
            None,
        );
        (f32::from(line.width) <= width).then_some(line)
    }
}

fn bounds(r: Rect) -> Bounds<Pixels> {
    Bounds::new(
        point(px(r.x as f32), px(r.y as f32)),
        size(px(r.w as f32), px(r.h as f32)),
    )
}

/// `b` moved in by `by` on every side, out where `by` is less than none.
fn inset(b: Bounds<Pixels>, by: Pixels) -> Bounds<Pixels> {
    Bounds::new(
        b.origin + point(by, by),
        size(
            (b.size.width - by * 2.).max(px(0.)),
            (b.size.height - by * 2.).max(px(0.)),
        ),
    )
}

fn layout_level(
    items: &[TreemapItem],
    area: Rect,
    depth: usize,
    labels: &mut Labels,
    out: &mut Vec<Laid>,
) {
    let weights: Vec<f64> = items.iter().map(|i| i.weight).collect();
    for (item, cell) in items.iter().zip(squarify(&weights, area)) {
        // The tile: its cell, half a gap in on every side.
        let r = Rect::new(
            cell.x + GAP / 2.,
            cell.y + GAP / 2.,
            cell.w - GAP,
            cell.h - GAP,
        );
        if r.w <= 0.0 || r.h <= 0.0 {
            continue;
        }
        let inner = Rect::new(r.x + GAP, r.y + HEADER, r.w - 2.0 * GAP, r.h - HEADER - GAP);
        let open = !item.children.is_empty() && inner.w >= OPEN_MIN_W && inner.h >= OPEN_MIN_H;
        let room = (r.w - 2.0 * INSET) as f32;
        let left = px((r.x + INSET) as f32);
        let (mut name, mut detail) = (None, None);
        if open && r.w >= LABEL_MIN_W {
            // The header: the name, and its size at the right where the
            // whole name leaves room for it: a name is worth more than its
            // size, which the line under the map says too.
            let top = px((r.y + HEADER_TOP) as f32);
            name = labels
                .name(&item.label, item.text, room, true)
                .map(|line| (line, point(left, top)));
            if let Some((line, _)) = &name {
                let rest = room - f32::from(line.width) - BESIDE;
                detail = labels.whole(&item.detail, item.text, rest).map(|line| {
                    let at = point(px((r.x + r.w - INSET) as f32) - line.width, top);
                    (line, at)
                });
            }
        } else if !open && r.w >= LABEL_MIN_W && r.h >= LABEL_MIN_H {
            let top = px((r.y + TOP) as f32);
            name = labels
                .name(&item.label, item.text, room, false)
                .map(|line| (line, point(left, top)));
            if name.is_some() && r.h >= DETAIL_MIN_H {
                // A size is said whole or not at all.
                detail = labels
                    .whole(&item.detail, item.text, room)
                    .map(|line| (line, point(left, top + px(LINE as f32))));
            }
        }
        let radius = RADII[depth.min(RADII.len() - 1)]
            .min((r.w / 2.) as f32)
            .min((r.h / 2.) as f32);
        out.push(Laid {
            block: Block {
                key: item.key,
                bounds: bounds(cell),
                depth,
            },
            tile: bounds(r),
            radius: px(radius),
            color: if open { item.tray } else { item.color },
            hatched: item.hatched,
            dashed: item.dashed,
            selected: item.selected,
            name,
            detail,
        });
        if open {
            layout_level(&item.children, inner, depth + 1, labels, out);
        }
    }
}

/// The deepest block containing `p`.
fn hit(blocks: &[Block], p: Point<Pixels>) -> Option<usize> {
    blocks
        .iter()
        .filter(|b| b.bounds.contains(&p))
        .max_by_key(|b| b.depth)
        .map(|b| b.key)
}

impl IntoElement for Treemap {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Treemap {
    type RequestLayoutState = ();
    type PrepaintState = TreemapPrepaint;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let font = window.text_style().font();
        let from = (
            Rc::as_ptr(&self.items) as usize,
            bounds,
            font.family.clone(),
            window.scale_factor().to_bits(),
        );
        let lay_out = |window: &mut Window, cx: &mut App| {
            // Squarify against the real bounds: nothing here was guessed
            // in render. A gap inside them, so a tile at the edge has
            // room for the ring around it.
            let area = Rect::new(
                f64::from(f32::from(bounds.origin.x)) + GAP,
                f64::from(f32::from(bounds.origin.y)) + GAP,
                f64::from(f32::from(bounds.size.width)) - 2. * GAP,
                f64::from(f32::from(bounds.size.height)) - 2. * GAP,
            );
            let mut blocks = Vec::new();
            let weight = |weight, least| {
                let font = Font {
                    weight,
                    ..font.clone()
                };
                Weight {
                    wrapper: cx.text_system().line_wrapper(font.clone(), px(FONT)),
                    font,
                    least,
                }
            };
            let mut labels = Labels {
                window,
                tile: weight(FontWeight::MEDIUM, LEAST),
                header: weight(FontWeight::SEMIBOLD, LEAST_HEADER),
                font: font.clone(),
            };
            layout_level(&self.items, area, 0, &mut labels, &mut blocks);
            let hits = Rc::new(blocks.iter().map(|b| b.block).collect());
            (Rc::new(blocks), hits)
        };
        let (blocks, hits) = match id {
            Some(id) => window.with_element_state(id, |kept: Option<Kept>, window| {
                let (blocks, hits) = match kept.filter(|kept| kept.from == from) {
                    Some(kept) => (kept.blocks, kept.hits),
                    None => lay_out(window, cx),
                };
                let kept = Kept {
                    from,
                    blocks: Rc::clone(&blocks),
                    hits: Rc::clone(&hits),
                };
                ((blocks, hits), kept)
            }),
            None => lay_out(window, cx),
        };
        if let Some(record) = &self.record {
            *record.borrow_mut() = blocks
                .iter()
                .filter(|b| b.block.depth == 0)
                .map(|b| {
                    let o = b.block.bounds.origin;
                    let s = b.block.bounds.size;
                    let r = Rect::new(
                        f64::from(f32::from(o.x)),
                        f64::from(f32::from(o.y)),
                        f64::from(f32::from(s.width)),
                        f64::from(f32::from(s.height)),
                    );
                    (b.block.key, r)
                })
                .collect();
        }
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        TreemapPrepaint {
            blocks,
            hits,
            hitbox,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let style = self.style;
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            for b in prepaint.blocks.iter() {
                let (tile, corners) = (b.tile, Corners::all(b.radius));
                let none = Edges::default();
                let clear = gpui::transparent_black();
                window.paint_quad(quad(
                    tile,
                    corners,
                    b.color,
                    none,
                    clear,
                    BorderStyle::Solid,
                ));
                if b.hatched {
                    // Patterns have transparent gaps. Keep the solid base so
                    // labels retain their contrast in both light and dark themes.
                    window.paint_quad(quad(
                        tile,
                        corners,
                        pattern_slash(gpui::black().opacity(0.2), 2.0, 8.0),
                        none,
                        clear,
                        BorderStyle::Solid,
                    ));
                }
                if b.dashed {
                    // Just inside the edge, its corners following the
                    // tile's, clear of the labels (`INSET`) and of what
                    // lies inside (`GAP`).
                    let two = px(2.);
                    window.paint_quad(quad(
                        inset(tile, two),
                        Corners::all((b.radius - two).max(px(0.))),
                        clear,
                        Edges::all(px(1.)),
                        style.dashed,
                        BorderStyle::Dashed,
                    ));
                }
                // Labels stay inside their tile.
                let mask = ContentMask {
                    bounds: inset(tile, px(2.)),
                };
                window.with_content_mask(Some(mask), |window| {
                    let line_height = px(LINE as f32);
                    for (line, origin) in [&b.name, &b.detail].into_iter().flatten() {
                        if let Err(error) = line.paint(
                            *origin,
                            line_height,
                            gpui::TextAlign::Left,
                            None,
                            window,
                            cx,
                        ) {
                            tracing::debug!(%error, "treemap label");
                        }
                    }
                });
            }
            // What the pointer is on, then what is chosen, over everything
            // drawn inside them.
            if let Some(b) = prepaint
                .blocks
                .iter()
                .find(|b| Some(b.block.key) == self.hovered)
            {
                window.paint_quad(quad(
                    b.tile,
                    Corners::all(b.radius),
                    style.hover,
                    Edges::all(px(1.)),
                    style.hover_edge,
                    BorderStyle::Solid,
                ));
            }
            for b in prepaint.blocks.iter().filter(|b| b.selected) {
                // A band of the ground around the tile, then the ring.
                let ring = px(RING);
                for (out, color) in [(ring, style.gap), (ring * 2., style.selected)] {
                    window.paint_quad(quad(
                        inset(b.tile, -out),
                        Corners::all(b.radius + out),
                        gpui::transparent_black(),
                        Edges::all(ring),
                        color,
                        BorderStyle::Solid,
                    ));
                }
            }
        });

        // Hover and click: hit-test the prepaint's rectangles.
        let hitbox = prepaint.hitbox.clone();
        let blocks = Rc::clone(&prepaint.hits);
        let hit_blocks = Rc::clone(&blocks);
        let hit_key = move |p: Point<Pixels>| hit(&hit_blocks, p);
        let hit_move = move |p: Point<Pixels>| hit(&blocks, p);
        let current = self.hovered;
        let on_hover = Rc::clone(&self.on_hover);
        let hb = hitbox.clone();
        window.on_mouse_event(
            move |ev: &MouseMoveEvent, phase: DispatchPhase, window, cx| {
                if !phase.bubble() {
                    return;
                }
                let key = if hb.is_hovered(window) {
                    hit_move(ev.position)
                } else {
                    None
                };
                if key != current {
                    on_hover(key, window, cx);
                }
            },
        );
        // The pointer gone from the window is on no block: GPUI keeps
        // the position it left at.
        let on_hover = Rc::clone(&self.on_hover);
        window.on_mouse_event(
            move |_: &MouseExitEvent, phase: DispatchPhase, window, cx| {
                if phase.bubble() && current.is_some() {
                    on_hover(None, window, cx);
                }
            },
        );
        let on_click = Rc::clone(&self.on_click);
        window.on_mouse_event(
            move |ev: &MouseDownEvent, phase: DispatchPhase, window, cx| {
                if phase.bubble()
                    && matches!(
                        ev.button,
                        MouseButton::Left | MouseButton::Right | MouseButton::Middle
                    )
                    && hitbox.is_hovered(window)
                    && let Some(key) = hit_key(ev.position)
                {
                    on_click(key, ev, window, cx);
                }
            },
        );
    }
}
