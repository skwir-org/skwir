//! A child drawn in a rem of its own: a part of the window at its text
//! size (ADR-117, `sizes.rs`). Every text size and length written in rems
//! under it follows, and `sizes::Scale::of` reads it in an element's
//! layout. It takes no room of its own: its child is laid out where it
//! stands.

use gpui::{
    AnyElement, App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement,
    LayoutId, Pixels, Window,
};

pub struct Rem {
    rem: Pixels,
    child: AnyElement,
}

/// `child`, drawn with `rem` as its rem.
pub fn rem(rem: Pixels, child: impl IntoElement) -> Rem {
    Rem {
        rem,
        child: child.into_any_element(),
    }
}

impl IntoElement for Rem {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Rem {
    type RequestLayoutState = ();
    type PrepaintState = ();

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
        let child = &mut self.child;
        let layout =
            window.with_rem_size(Some(self.rem), |window| child.request_layout(window, cx));
        (layout, ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let child = &mut self.child;
        window.with_rem_size(Some(self.rem), |window| child.prepaint(window, cx));
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let child = &mut self.child;
        window.with_rem_size(Some(self.rem), |window| child.paint(window, cx));
    }
}
