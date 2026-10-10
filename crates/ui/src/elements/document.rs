//! Document pixels and hit testing share the transform calculated in prepaint.
//! No measured bounds escape into render; normalized text/link boxes remain
//! aligned when the panel is resized or the image is zoomed.

use std::{rc::Rc, sync::Arc};

use gpui::{
    App, Bounds, ContentMask, Corners, DispatchPhase, Element, ElementId, GlobalElementId, Hitbox,
    HitboxBehavior, Hsla, InspectorElementId, IntoElement, LayoutId, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, RenderImage, Style, Window, fill, point, px, relative,
    size,
};
use skwir_core::document::{PageContent, Rect};

type Pointer = Rc<dyn Fn(&str, f32, f32, &mut Window, &mut App)>;

/// What pages cover on screen, reported by their elements in prepaint: the
/// most device pixels any covers on its longer side, and the scale it is
/// shown at (its size on screen over its own). The reader renders pages at
/// that side (`Shell::document_shown`) and zooms from that scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shown {
    pub covered: f32,
    pub scale: f32,
}
pub type Report = Rc<std::cell::Cell<Option<Shown>>>;
pub struct DocumentElement {
    pub image: Arc<RenderImage>,
    pub page: Arc<PageContent>,
    pub highlights: Vec<Rect>,
    pub selection: Vec<Rect>,
    /// The theme's `page_match` and `page_selection`, drawn over the page.
    pub colors: (Hsla, Hsla),
    pub zoom: Option<f32>,
    pub fit_width: bool,
    pub on_pointer: Pointer,
    pub report: Option<Report>,
}
impl IntoElement for DocumentElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

pub struct Prepaint {
    image: Bounds<Pixels>,
    hitbox: Hitbox,
}

pub fn fitted_rect(
    width: f32,
    height: f32,
    page_width: f32,
    page_height: f32,
) -> (f32, f32, f32, f32) {
    let scale = (width / page_width.max(1.))
        .min(height / page_height.max(1.))
        .max(0.);
    let (w, h) = (page_width * scale, page_height * scale);
    ((width - w) / 2., (height - h) / 2., w, h)
}

impl Element for DocumentElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepaint;
    fn id(&self) -> Option<ElementId> {
        Some(("document-page", self.page.page as usize).into())
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
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        if let Some(zoom) = self.zoom {
            style.size.width = px(self.page.width * zoom).into();
            style.size.height = px(self.page.height * zoom).into();
            style.flex_shrink = 0.;
        } else if self.fit_width {
            style.size.width = relative(1.).into();
            style.aspect_ratio = Some(self.page.width / self.page.height.max(1.));
            style.flex_shrink = 0.;
        } else {
            style.size.width = relative(1.).into();
            style.size.height = relative(1.).into();
        }
        (window.request_layout(style, [], cx), ())
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        _: &mut App,
    ) -> Prepaint {
        let (x, y, w, h) = fitted_rect(
            bounds.size.width.into(),
            bounds.size.height.into(),
            self.page.width,
            self.page.height,
        );
        let image = Bounds::new(bounds.origin + point(px(x), px(y)), size(px(w), px(h)));
        if let Some(report) = &self.report {
            let shown = Shown {
                covered: w.max(h) * window.scale_factor(),
                scale: w / self.page.width.max(1.),
            };
            if report.get().is_none_or(|seen| seen.covered < shown.covered) {
                report.set(Some(shown));
            }
        }
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        Prepaint { image, hitbox }
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        prepaint: &mut Prepaint,
        window: &mut Window,
        _: &mut App,
    ) {
        let image_bounds = prepaint.image;
        let mouse = window.mouse_position();
        let x =
            f32::from(mouse.x - image_bounds.origin.x) / f32::from(image_bounds.size.width).max(1.);
        let y = f32::from(mouse.y - image_bounds.origin.y)
            / f32::from(image_bounds.size.height).max(1.);
        let cursor = if self
            .page
            .links
            .iter()
            .any(|link| link.bounds.contains(x, y))
        {
            gpui::CursorStyle::PointingHand
        } else {
            gpui::CursorStyle::IBeam
        };
        window.set_cursor_style(cursor, &prepaint.hitbox);
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            let _ = window.paint_image(
                image_bounds,
                Corners::default(),
                self.image.clone(),
                0,
                false,
            );
            for (rects, color) in [
                (&self.highlights, self.colors.0),
                (&self.selection, self.colors.1),
            ] {
                for rect in rects {
                    let bounds = Bounds::new(
                        image_bounds.origin
                            + point(
                                image_bounds.size.width * rect.x,
                                image_bounds.size.height * rect.y,
                            ),
                        size(
                            image_bounds.size.width * rect.width,
                            image_bounds.size.height * rect.height,
                        ),
                    );
                    window.paint_quad(fill(bounds, color));
                }
            }
        });
        let coordinates = move |p: gpui::Point<Pixels>| {
            let x =
                f32::from(p.x - image_bounds.origin.x) / f32::from(image_bounds.size.width).max(1.);
            let y = f32::from(p.y - image_bounds.origin.y)
                / f32::from(image_bounds.size.height).max(1.);
            (x, y)
        };
        let callback = self.on_pointer.clone();
        let hitbox = prepaint.hitbox.clone();
        window.on_mouse_event(
            move |ev: &MouseDownEvent, phase: DispatchPhase, window, cx| {
                if phase.bubble()
                    && ev.button == MouseButton::Left
                    && hitbox.is_hovered(window)
                    && image_bounds.contains(&ev.position)
                {
                    let (x, y) = coordinates(ev.position);
                    callback("down", x, y, window, cx);
                    cx.stop_propagation();
                }
            },
        );
        let callback = self.on_pointer.clone();
        window.on_mouse_event(
            move |ev: &MouseMoveEvent, phase: DispatchPhase, window, cx| {
                if phase.bubble() && ev.pressed_button == Some(MouseButton::Left) {
                    let (x, y) = coordinates(ev.position);
                    callback("move", x, y, window, cx);
                }
            },
        );
        let callback = self.on_pointer.clone();
        window.on_mouse_event(move |ev: &MouseUpEvent, phase: DispatchPhase, window, cx| {
            if phase.bubble() && ev.button == MouseButton::Left {
                let (x, y) = coordinates(ev.position);
                callback("up", x, y, window, cx);
            }
        });
    }
}

/// Build a virtual document list with the current viewport dimensions. Layout
/// inputs never make a round trip through `Shell::render`.
pub struct DocumentViewport {
    pub build: Box<dyn FnOnce(gpui::Size<Pixels>, &mut Window, &mut App) -> gpui::AnyElement>,
    pub laid_out: Box<dyn FnOnce(&mut Window, &mut App)>,
}
impl IntoElement for DocumentViewport {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for DocumentViewport {
    type RequestLayoutState = ();
    type PrepaintState = gpui::AnyElement;
    fn id(&self) -> Option<ElementId> {
        None
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
    ) -> (LayoutId, ()) {
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
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> gpui::AnyElement {
        let build = std::mem::replace(&mut self.build, Box::new(|_, _, _| unreachable!()));
        let mut child = build(bounds.size, window, cx);
        child.layout_as_root(bounds.size.map(gpui::AvailableSpace::Definite), window, cx);
        child.prepaint_at(bounds.origin, window, cx);
        let laid_out = std::mem::replace(&mut self.laid_out, Box::new(|_, _| {}));
        laid_out(window, cx);
        child
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        child: &mut gpui::AnyElement,
        window: &mut Window,
        cx: &mut App,
    ) {
        child.paint(window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fit_keeps_selection_and_pixels_in_the_same_letterboxed_area() {
        assert_eq!(fitted_rect(400., 600., 200., 200.), (0., 100., 400., 400.));
        assert_eq!(fitted_rect(800., 200., 400., 200.), (200., 0., 400., 200.));
    }
}
