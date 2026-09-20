//! Same-frame button anchors, shared across responsive toolbar variants.
use std::{cell::Cell, rc::Rc};

use gpui::{AnyElement, AvailableSpace, Bounds, IntoElement, Pixels, canvas, prelude::*, px, size};

#[derive(Clone, Default)]
pub struct PopoverAnchor(Rc<Cell<Option<Bounds<Pixels>>>>);

impl PopoverAnchor {
    pub fn marker(&self) -> AnyElement {
        let bounds = self.0.clone();
        canvas(move |value, _, _| bounds.set(Some(value)), |_, (), _, _| {})
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .into_any_element()
    }

    pub fn popup(&self, panel: impl IntoElement) -> AnyElement {
        self.popup_for(
            &crate::elements::adaptive::Chosen::default(),
            vec![panel.into_any_element()],
        )
    }

    /// A popup whose content depends on the variant an adaptive row chose:
    /// `panels[variant]`, or the last one. The choice is read in the deferred
    /// prepaint, after the row made it in this same frame, so a menu can hold
    /// exactly what its toolbar dropped without any size reaching `render`.
    pub fn popup_for(
        &self,
        chosen: &crate::elements::adaptive::Chosen,
        mut panels: Vec<AnyElement>,
    ) -> AnyElement {
        let anchor = self.0.clone();
        let chosen = chosen.clone();
        // Defer until the selected toolbar variant has prepainted its button.
        // No coordinates survive this render or feed back into view state.
        gpui::deferred(
            canvas(
                move |_, window, cx| {
                    let bounds = anchor.get()?;
                    let last = panels.len().checked_sub(1)?;
                    let panel = panels.swap_remove(chosen.get().unwrap_or(last).min(last));
                    let mut popup = gpui::anchored()
                        .position(bounds.bottom_left())
                        .offset(gpui::point(px(0.), px(6.)))
                        .snap_to_window_with_margin(px(8.))
                        .child(panel)
                        .into_any_element();
                    popup.layout_as_root(
                        size(AvailableSpace::MaxContent, AvailableSpace::MaxContent),
                        window,
                        cx,
                    );
                    popup.prepaint_at(gpui::Point::default(), window, cx);
                    Some(popup)
                },
                |_, popup, window, cx| {
                    if let Some(mut popup) = popup {
                        popup.paint(window, cx);
                    }
                },
            )
            .absolute()
            .size(px(0.)),
        )
        .with_priority(3)
        .into_any_element()
    }
}
