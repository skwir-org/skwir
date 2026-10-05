//! What answers on a picture drawn smaller than its box, as a film is in
//! its letterbox: laid over the rectangle the picture takes, found from the
//! box's real bounds in prepaint with the arithmetic `img` paints with
//! (`ObjectFit::get_bounds`), so the room round the picture stays the box's.
use gpui::{
    AnyElement, App, Bounds, DevicePixels, Element, ElementId, GlobalElementId, InspectorElementId,
    IntoElement, LayoutId, ObjectFit, Pixels, Size, Style, Window, px, relative, size,
};

/// `child` on the rectangle a picture of `image` pixels takes in this box
/// when fitted by `fit`: the box is the whole of the parent, which must be
/// `relative`. The child draws the picture itself, at the size it is given.
pub struct OnPicture {
    pub child: AnyElement,
    pub image: Size<DevicePixels>,
    pub fit: ObjectFit,
}

impl IntoElement for OnPicture {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for OnPicture {
    type RequestLayoutState = ();
    type PrepaintState = bool;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style {
            position: gpui::Position::Absolute,
            ..Style::default()
        };
        style.inset.top = px(0.).into();
        style.inset.left = px(0.).into();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        (): &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        if bounds.size.width <= px(0.)
            || bounds.size.height <= px(0.)
            || self.image.width.0 <= 0
            || self.image.height.0 <= 0
        {
            return false;
        }
        let picture = self.fit.get_bounds(bounds, self.image);
        self.child.layout_as_root(
            size(
                gpui::AvailableSpace::Definite(picture.size.width),
                gpui::AvailableSpace::Definite(picture.size.height),
            ),
            window,
            cx,
        );
        self.child.prepaint_at(picture.origin, window, cx);
        true
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        (): &mut (),
        placed: &mut bool,
        window: &mut Window,
        cx: &mut App,
    ) {
        if *placed {
            self.child.paint(window, cx);
        }
    }
}
