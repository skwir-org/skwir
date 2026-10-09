//! A treemap element: squarify runs in `prepaint` on the element's real
//! bounds; `paint` draws rectangles and labels and installs mouse handlers on
//! the element's hitbox for hover and click.
//!
//! An item with children is drawn open while its block has room for them:
//! a band with its name and size on top, its children laid out below.
//! Whether it has room is decided here, from the bounds prepaint was
//! given, never from a size measured in `render`.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    App, BorderStyle, Bounds, ContentMask, Corners, DispatchPhase, Edges, Element, ElementId, Font,
    GlobalElementId, Hitbox, HitboxBehavior, Hsla, InspectorElementId, IntoElement, LayoutId,
    MouseButton, MouseDownEvent, MouseExitEvent, MouseMoveEvent, Pixels, ShapedLine, SharedString,
    Style, TextRun, Window, pattern_slash, point, px, quad, relative, size,
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
    /// in an open block's band, under it in a closed one.
    pub detail: SharedString,
    pub color: Hsla,
    /// Its label's color, on `color`.
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
    /// Between blocks.
    pub gap: Hsla,
    /// Laid over the block under the pointer.
    pub hover: Hsla,
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
    items: Vec<TreemapItem>,
    style: TreemapStyle,
    /// The key currently reported as hovered, so the callback fires on change only.
    hovered: Option<usize>,
    record: Option<Laidout>,
    on_hover: HoverFn,
    on_click: ClickFn,
}

pub fn treemap(
    id: impl Into<ElementId>,
    items: Vec<TreemapItem>,
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

/// Where a block is, for hit-testing.
#[derive(Clone, Copy)]
struct Block {
    key: usize,
    bounds: Bounds<Pixels>,
    depth: usize,
}

struct Laid {
    block: Block,
    color: Hsla,
    hatched: bool,
    dashed: bool,
    selected: bool,
    /// Its name, and what it weighs: beside the name in a band, or under it.
    name: Option<ShapedLine>,
    detail: Option<(ShapedLine, gpui::Point<Pixels>)>,
}

pub struct TreemapPrepaint {
    blocks: Vec<Laid>,
    hitbox: Hitbox,
}

/// Inside an open block: the margin around its children, and the band
/// above them holding its name.
const PAD: f64 = 3.0;
const BAND: f64 = 16.0;
const FONT: f32 = 11.0;
/// A label's line, and its inset from its block's corner.
const LINE: f64 = 14.0;
const INSET: f64 = 4.0;
/// Narrower than this, a block says nothing.
const LABEL_MIN_W: f64 = 44.0;
/// An open block's children need at least this much room.
const OPEN_MIN_W: f64 = 32.0;
const OPEN_MIN_H: f64 = 24.0;
/// Room kept for a name before a size is said beside it in a band.
const NAME_MIN_W: f32 = 24.0;
/// The ring around a chosen block: over the gap around it (each block's
/// edge is a line of the gap's color, so two blocks are two lines apart),
/// then a line of the gap's color inside, clear of its labels (`INSET`).
const RING: f32 = 2.0;

/// Shapes labels, cut in the middle to the width they get: the end of a
/// name tells files apart.
struct Labels<'a> {
    window: &'a Window,
    wrapper: gpui::LineWrapperHandle,
    font: Font,
}

impl Labels<'_> {
    fn run(&self, len: usize, color: Hsla) -> TextRun {
        TextRun {
            len,
            font: self.font.clone(),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
            position: gpui::RunPosition::Baseline,
        }
    }

    /// `text` shaped to fit `width`, if anything of it does.
    fn line(&mut self, text: &SharedString, color: Hsla, width: f32) -> Option<ShapedLine> {
        if text.is_empty() || width < FONT {
            return None;
        }
        let mut runs = vec![self.run(text.len(), color)];
        let cut = self
            .wrapper
            .truncate_line_middle(text.clone(), px(width), "…", &mut runs);
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
            px(FONT),
            &[self.run(text.len(), color)],
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

fn layout_level(
    items: &[TreemapItem],
    area: Rect,
    depth: usize,
    labels: &mut Labels,
    out: &mut Vec<Laid>,
) {
    let weights: Vec<f64> = items.iter().map(|i| i.weight).collect();
    for (item, r) in items.iter().zip(squarify(&weights, area)) {
        if r.w <= 0.0 || r.h <= 0.0 {
            continue;
        }
        let inner = Rect::new(r.x + PAD, r.y + BAND, r.w - 2.0 * PAD, r.h - BAND - PAD);
        let open = !item.children.is_empty() && inner.w >= OPEN_MIN_W && inner.h >= OPEN_MIN_H;
        let room = (r.w - 2.0 * INSET) as f32;
        let (mut name, mut detail) = (None, None);
        if r.w >= LABEL_MIN_W && r.h >= LINE {
            if open {
                // The band: the name, and its size at the right while the
                // name keeps some room.
                let size = labels
                    .whole(&item.detail, item.text.opacity(0.8), room - NAME_MIN_W - 6.)
                    .map(|line| {
                        let at = point(
                            px((r.x + r.w - INSET) as f32) - line.width,
                            px((r.y + 1.0) as f32),
                        );
                        (line, at)
                    });
                let name_room = size
                    .as_ref()
                    .map_or(room, |(line, _)| room - f32::from(line.width) - 6.);
                name = labels.line(&item.label, item.text, name_room);
                detail = size;
            } else {
                name = labels.line(&item.label, item.text, room);
                if r.h >= 2.0 * LINE + 4.0 {
                    // A size is said whole or not at all.
                    detail = labels
                        .whole(&item.detail, item.text.opacity(0.8), room)
                        .map(|line| {
                            let at = point(px((r.x + INSET) as f32), px((r.y + 2.0 + LINE) as f32));
                            (line, at)
                        });
                }
            }
        }
        out.push(Laid {
            block: Block {
                key: item.key,
                bounds: bounds(r),
                depth,
            },
            color: item.color,
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
fn hit(blocks: &[Block], p: gpui::Point<Pixels>) -> Option<usize> {
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
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        // Squarify against the real bounds: nothing here was guessed in
        // render. A line inside them, so a block at the edge has the gap
        // a ring is drawn over.
        let area = Rect::new(
            f64::from(f32::from(bounds.origin.x)) + 1.,
            f64::from(f32::from(bounds.origin.y)) + 1.,
            f64::from(f32::from(bounds.size.width)) - 2.,
            f64::from(f32::from(bounds.size.height)) - 2.,
        );
        let font = window.text_style().font();
        let mut blocks = Vec::new();
        {
            let mut labels = Labels {
                window,
                wrapper: cx.text_system().line_wrapper(font.clone(), px(FONT)),
                font,
            };
            layout_level(&self.items, area, 0, &mut labels, &mut blocks);
        }
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
        TreemapPrepaint { blocks, hitbox }
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
            for b in &prepaint.blocks {
                let bounds = b.block.bounds;
                window.paint_quad(quad(
                    bounds,
                    Corners::default(),
                    b.color,
                    Edges::all(px(1.)),
                    style.gap,
                    BorderStyle::Solid,
                ));
                if b.hatched {
                    // Patterns have transparent gaps. Keep the solid base so
                    // labels retain their contrast in both light and dark themes.
                    window.paint_quad(quad(
                        bounds,
                        Corners::default(),
                        pattern_slash(gpui::black().opacity(0.2), 2.0, 8.0),
                        Edges::all(px(1.)),
                        style.gap,
                        BorderStyle::Solid,
                    ));
                }
                if b.dashed {
                    // Just inside the edge, clear of the labels (`INSET`)
                    // and of what lies inside (`PAD`).
                    let one = px(1.);
                    window.paint_quad(quad(
                        Bounds::new(
                            bounds.origin + point(one, one),
                            size(
                                (bounds.size.width - one * 2.).max(px(0.)),
                                (bounds.size.height - one * 2.).max(px(0.)),
                            ),
                        ),
                        Corners::default(),
                        gpui::transparent_black(),
                        Edges::all(px(1.)),
                        style.dashed,
                        BorderStyle::Dashed,
                    ));
                }
                // Labels stay inside their block, and a band's inside its band.
                let mask = ContentMask {
                    bounds: Bounds::new(
                        bounds.origin + point(px(2.), px(0.)),
                        size(
                            (bounds.size.width - px(4.)).max(px(0.)),
                            bounds.size.height.min(px((BAND + LINE) as f32)),
                        ),
                    ),
                };
                window.with_content_mask(Some(mask), |window| {
                    let line_height = px(LINE as f32);
                    if let Some(name) = &b.name {
                        let origin = bounds.origin + point(px(INSET as f32), px(1.));
                        if let Err(error) = name.paint(origin, line_height, window, cx) {
                            tracing::debug!(%error, "treemap label");
                        }
                    }
                    if let Some((detail, origin)) = &b.detail
                        && let Err(error) = detail.paint(*origin, line_height, window, cx)
                    {
                        tracing::debug!(%error, "treemap label");
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
                    b.block.bounds,
                    Corners::default(),
                    style.hover,
                    Edges::all(px(1.)),
                    style.gap,
                    BorderStyle::Solid,
                ));
            }
            for b in prepaint.blocks.iter().filter(|b| b.selected) {
                let (o, s) = (b.block.bounds.origin, b.block.bounds.size);
                let one = px(1.);
                let ring = Bounds::new(o - point(one, one), s + size(one * 2., one * 2.));
                window.paint_quad(quad(
                    ring,
                    Corners::default(),
                    gpui::transparent_black(),
                    Edges::all(px(RING)),
                    style.selected,
                    BorderStyle::Solid,
                ));
                let inside = Bounds::new(
                    o + point(one, one),
                    size(
                        (s.width - one * 2.).max(px(0.)),
                        (s.height - one * 2.).max(px(0.)),
                    ),
                );
                window.paint_quad(quad(
                    inside,
                    Corners::default(),
                    gpui::transparent_black(),
                    Edges::all(one),
                    style.gap,
                    BorderStyle::Solid,
                ));
            }
        });

        // Hover and click: hit-test the prepaint's rectangles.
        let hitbox = prepaint.hitbox.clone();
        let blocks: Rc<Vec<Block>> = Rc::new(prepaint.blocks.iter().map(|b| b.block).collect());
        let hit_blocks = Rc::clone(&blocks);
        let hit_key = move |p: gpui::Point<Pixels>| hit(&hit_blocks, p);
        let hit_move = move |p: gpui::Point<Pixels>| hit(&blocks, p);
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
