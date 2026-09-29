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

    /// `panel` under the anchor's right edge, reaching left: what it
    /// covers is at the right of what the anchor is over (the address
    /// field's dropdown leaves the names of the list below it in sight).
    pub fn popup_right(&self, panel: impl IntoElement) -> AnyElement {
        let mut panel = Some(panel.into_any_element());
        below_at(
            std::slice::from_ref(self),
            3,
            gpui::Corner::TopRight,
            move || panel.take(),
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
        let chosen = chosen.clone();
        below(std::slice::from_ref(self), 3, move || {
            let last = panels.len().checked_sub(1)?;
            Some(panels.swap_remove(chosen.get().unwrap_or(last).min(last)))
        })
    }

    /// `panel` under the first of `anchors` drawn this frame: a menu opened
    /// from the keyboard hangs from the row it is about, or from the pane
    /// when no row is. Drawn at `priority` among deferred draws.
    pub fn popup_first(
        anchors: &[PopoverAnchor],
        priority: usize,
        panel: AnyElement,
    ) -> AnyElement {
        let mut panel = Some(panel);
        below(anchors, priority, move || panel.take())
    }
}

/// Defer until the anchors have prepainted in this same frame, then lay out
/// what `panel` gives under the first that has bounds. No coordinates
/// survive this render or feed back into view state. The panel gets the
/// height left under the anchor: one that scrolls (`toolbar::popup_panel`)
/// shrinks to it instead of running past the window's foot.
fn below(
    anchors: &[PopoverAnchor],
    priority: usize,
    panel: impl FnMut() -> Option<AnyElement> + 'static,
) -> AnyElement {
    below_at(anchors, priority, gpui::Corner::TopLeft, panel)
}

/// `below`, the panel's `corner` at the anchor's matching bottom corner.
fn below_at(
    anchors: &[PopoverAnchor],
    priority: usize,
    corner: gpui::Corner,
    mut panel: impl FnMut() -> Option<AnyElement> + 'static,
) -> AnyElement {
    let anchors: Vec<_> = anchors.iter().map(|a| a.0.clone()).collect();
    gpui::deferred(
        canvas(
            move |_, window, cx| {
                let bounds = anchors.iter().find_map(|a| a.get())?;
                let (gap, margin) = (px(6.), px(8.));
                let room = window.viewport_size().height - bounds.bottom() - gap - margin;
                let at = if corner == gpui::Corner::TopRight {
                    bounds.bottom_right()
                } else {
                    bounds.bottom_left()
                };
                let mut popup = gpui::anchored()
                    .anchor(corner)
                    .position(at)
                    .offset(gpui::point(px(0.), gap))
                    .snap_to_window_with_margin(margin)
                    .child(
                        gpui::div()
                            .flex()
                            .flex_col()
                            .max_h(room.max(px(120.)))
                            .child(panel()?),
                    )
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
    .with_priority(priority)
    .into_any_element()
}
