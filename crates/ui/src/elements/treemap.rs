//! A treemap element: squarify runs in `prepaint` on the element's real
//! bounds; `paint` draws rectangles and labels and installs mouse handlers on
//! the element's hitbox for hover and click.

use std::rc::Rc;

use gpui::{
    App, BorderStyle, Bounds, ContentMask, Corners, DispatchPhase, Edges, Element, ElementId,
    GlobalElementId, Hitbox, HitboxBehavior, Hsla, InspectorElementId, IntoElement, LayoutId,
    MouseButton, MouseDownEvent, MouseMoveEvent, Pixels, ShapedLine, SharedString, Style, TextRun,
    Window, pattern_slash, point, px, quad, relative, size,
};

use crate::treemap::{Rect, squarify};

/// One block to lay out. `children` are laid out inside it (one level).
pub struct TreemapItem {
    /// Identifies the block in hover/click callbacks.
    pub key: usize,
    pub weight: f64,
    pub label: SharedString,
    pub color: Hsla,
    pub hatched: bool,
    pub children: Vec<TreemapItem>,
}

type HoverFn = Rc<dyn Fn(Option<usize>, &mut Window, &mut App)>;
type ClickFn = Rc<dyn Fn(usize, &MouseDownEvent, &mut Window, &mut App)>;

pub struct Treemap {
    id: ElementId,
    items: Vec<TreemapItem>,
    border: Hsla,
    label_color: Hsla,
    /// The key currently reported as hovered, so the callback fires on change only.
    hovered: Option<usize>,
    on_hover: HoverFn,
    on_click: ClickFn,
}

pub fn treemap(
    id: impl Into<ElementId>,
    items: Vec<TreemapItem>,
    border: Hsla,
    label_color: Hsla,
    hovered: Option<usize>,
    on_hover: impl Fn(Option<usize>, &mut Window, &mut App) + 'static,
    on_click: impl Fn(usize, &MouseDownEvent, &mut Window, &mut App) + 'static,
) -> Treemap {
    Treemap {
        id: id.into(),
        items,
        border,
        label_color,
        hovered,
        on_hover: Rc::new(on_hover),
        on_click: Rc::new(on_click),
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
    label: Option<ShapedLine>,
}

pub struct TreemapPrepaint {
    blocks: Vec<Laid>,
    hitbox: Hitbox,
}

const PAD: f64 = 3.0;
const LABEL_H: f64 = 16.0;

fn layout_level(
    items: &[TreemapItem],
    area: Rect,
    depth: usize,
    label_color: Hsla,
    out: &mut Vec<Laid>,
    window: &mut Window,
) {
    let weights: Vec<f64> = items.iter().map(|i| i.weight).collect();
    for (item, r) in items.iter().zip(squarify(&weights, area)) {
        if r.w <= 0.0 || r.h <= 0.0 {
            continue;
        }
        let bounds = Bounds::new(
            point(px(r.x as f32), px(r.y as f32)),
            size(px(r.w as f32), px(r.h as f32)),
        );
        let label = if r.w > 48.0 && r.h > LABEL_H {
            let style = window.text_style();
            let font_size = px(11.0);
            let run = TextRun {
                len: item.label.len(),
                font: style.font(),
                color: label_color,
                background_color: None,
                underline: None,
                strikethrough: None,
                position: gpui::RunPosition::Baseline,
            };
            Some(
                window
                    .text_system()
                    .shape_line(item.label.clone(), font_size, &[run], None),
            )
        } else {
            None
        };
        out.push(Laid {
            block: Block {
                key: item.key,
                bounds,
                depth,
            },
            color: item.color,
            hatched: item.hatched,
            label,
        });
        if !item.children.is_empty() && r.w > 40.0 && r.h > 40.0 {
            let label_h = if r.h > 40.0 { LABEL_H } else { 0.0 };
            let inner = Rect::new(
                r.x + PAD,
                r.y + PAD + label_h,
                (r.w - 2.0 * PAD).max(0.0),
                (r.h - 2.0 * PAD - label_h).max(0.0),
            );
            layout_level(&item.children, inner, depth + 1, label_color, out, window);
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
        _: &mut App,
    ) -> Self::PrepaintState {
        // Squarify against the real bounds: nothing here was guessed in render.
        let area = Rect::new(
            f64::from(f32::from(bounds.origin.x)),
            f64::from(f32::from(bounds.origin.y)),
            f64::from(f32::from(bounds.size.width)),
            f64::from(f32::from(bounds.size.height)),
        );
        let mut blocks = Vec::new();
        layout_level(&self.items, area, 0, self.label_color, &mut blocks, window);
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
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            for b in &prepaint.blocks {
                let bounds = b.block.bounds;
                window.paint_quad(quad(
                    bounds,
                    Corners::default(),
                    b.color,
                    Edges::all(px(1.)),
                    self.border,
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
                        self.border,
                        BorderStyle::Solid,
                    ));
                }
                if let Some(line) = &b.label {
                    let origin = bounds.origin + point(px(4.), px(2.));
                    let mask = ContentMask {
                        bounds: Bounds::new(
                            bounds.origin + point(px(2.), px(0.)),
                            size(bounds.size.width - px(4.), px(LABEL_H as f32)),
                        ),
                    };
                    window.with_content_mask(Some(mask), |window| {
                        if let Err(error) = line.paint(origin, px(LABEL_H as f32), window, cx) {
                            tracing::debug!(%error, "treemap label");
                        }
                    });
                }
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
        let on_click = Rc::clone(&self.on_click);
        window.on_mouse_event(
            move |ev: &MouseDownEvent, phase: DispatchPhase, window, cx| {
                if phase.bubble()
                    && matches!(ev.button, MouseButton::Left | MouseButton::Right)
                    && hitbox.is_hovered(window)
                    && let Some(key) = hit_key(ev.position)
                {
                    on_click(key, ev, window, cx);
                }
            },
        );
    }
}
