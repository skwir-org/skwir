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
    /// The press holding the pages, shared with the layer under them that
    /// moves them (`elements::pan`, ADR-143).
    pub grab: crate::elements::pan::Grab,
    /// A left drag across its text moves the pages rather than selecting it
    /// (`reader::drags`). Wherever a press selects nothing (`selects_at`),
    /// the page holds the grab itself, and a release that did not move the
    /// pages is a click.
    pub drags: bool,
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

/// The character a left press at `(x, y)` on `page` (its share of the
/// page's width and height) begins a selection on: the one under it,
/// unless a drag across the text moves the pages (`drags`). Anywhere else
/// (a margin, between paragraphs, a picture, a page with no text) the
/// press selects nothing, so it holds the pages, as one beside the page
/// does (ADR-143). The page's element and `Shell::reader_pointer` both ask
/// it, at the press and for the cursor over the page.
pub fn selects_at(drags: bool, page: &PageContent, x: f32, y: f32) -> Option<usize> {
    if drags {
        return None;
    }
    page.chars
        .iter()
        .position(|ch| ch.bounds.height > 0. && ch.bounds.contains(x, y))
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
        // The page alone, not the room around it a page at a time: a press
        // there is the layer's under it, and its hand shows there.
        let hitbox = window.insert_hitbox(image, HitboxBehavior::Normal);
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
        // While a press holds the pages, the hand closed on them is the
        // layer's, over a link or text too; while one selects, the I-beam
        // shows wherever it goes. Else a link's pointing hand, the I-beam
        // over a character a press selects from, and elsewhere the open
        // hand, where the pages can move. Only the page under the pointer
        // looks for what is under it.
        let cursor = if self.grab.grabbing() {
            None
        } else if self.grab.selecting() {
            Some(gpui::CursorStyle::IBeam)
        } else if !image_bounds.contains(&mouse) {
            None
        } else if self
            .page
            .links
            .iter()
            .any(|link| link.bounds.contains(x, y))
        {
            Some(gpui::CursorStyle::PointingHand)
        } else if selects_at(self.drags, &self.page, x, y).is_some() {
            Some(gpui::CursorStyle::IBeam)
        } else {
            self.grab.movable().then_some(gpui::CursorStyle::OpenHand)
        };
        if let Some(cursor) = cursor {
            window.set_cursor_style(cursor, &prepaint.hitbox);
        }
        window.with_content_mask(Some(ContentMask::new(bounds)), |window| {
            let _ = window.paint_image(
                image_bounds,
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
        let (grab, drags, page) = (self.grab.clone(), self.drags, self.page.clone());
        window.on_mouse_event(
            move |ev: &MouseDownEvent, phase: DispatchPhase, window, cx| {
                if phase.bubble()
                    && ev.button == MouseButton::Left
                    && hitbox.is_hovered(window)
                    && image_bounds.contains(&ev.position)
                {
                    let (x, y) = coordinates(ev.position);
                    // On a character it selects; anywhere else it holds the
                    // pages, and the page keeps it to tell its click.
                    if selects_at(drags, &page, x, y).is_some() {
                        grab.select(ev.position);
                    } else {
                        grab.hold(ev.position);
                    }
                    callback("down", x, y, window, cx);
                    cx.stop_propagation();
                }
            },
        );
        let callback = self.on_pointer.clone();
        let selecting = self.grab.clone();
        // A drag that moves the pages is the layer's: the page has nothing
        // to select as it goes.
        window.on_mouse_event(
            move |ev: &MouseMoveEvent, phase: DispatchPhase, window, cx| {
                if phase.bubble()
                    && ev.pressed_button == Some(MouseButton::Left)
                    && selecting.selecting()
                {
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
    fn a_press_selects_only_on_a_character() {
        use skwir_core::document::TextChar;
        let char_at = |x, width, height| TextChar {
            text: "a".into(),
            bounds: Rect {
                x,
                y: 0.1,
                width,
                height,
            },
        };
        let page = PageContent {
            page: 1,
            width: 600.,
            height: 800.,
            chars: vec![char_at(0.1, 0.02, 0.02), char_at(0.5, 0.02, 0.)],
            links: Vec::new(),
        };
        assert_eq!(selects_at(false, &page, 0.11, 0.11), Some(0));
        // A margin, and a character with no height, select nothing: the
        // press holds the pages.
        assert_eq!(selects_at(false, &page, 0.9, 0.5), None);
        assert_eq!(selects_at(false, &page, 0.51, 0.1), None);
        // Where the drag across text moves the pages, nothing selects.
        assert_eq!(selects_at(true, &page, 0.11, 0.11), None);
    }

    #[test]
    fn fit_keeps_selection_and_pixels_in_the_same_letterboxed_area() {
        assert_eq!(fitted_rect(400., 600., 200., 200.), (0., 100., 400., 400.));
        assert_eq!(fitted_rect(800., 200., 400., 200.), (200., 0., 400., 200.));
    }
}
