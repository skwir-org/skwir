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
        hang(
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

    /// `popup_for`, hanging above the anchor and reaching left from its
    /// right edge: for a button at the window's foot (the dock's More),
    /// where there is no room below it.
    pub fn popup_above_for(
        &self,
        chosen: &crate::elements::adaptive::Chosen,
        mut panels: Vec<AnyElement>,
    ) -> AnyElement {
        let chosen = chosen.clone();
        hang(
            std::slice::from_ref(self),
            3,
            gpui::Corner::BottomRight,
            move || {
                let last = panels.len().checked_sub(1)?;
                Some(panels.swap_remove(chosen.get().unwrap_or(last).min(last)))
            },
        )
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

    /// `popup_first`, standing above the anchor: for a menu at the
    /// window's foot (the terminal's, above its cursor).
    pub fn popup_first_above(
        anchors: &[PopoverAnchor],
        priority: usize,
        panel: AnyElement,
    ) -> AnyElement {
        let mut panel = Some(panel);
        hang(anchors, priority, gpui::Corner::BottomLeft, move || {
            panel.take()
        })
    }

    /// Mark where the anchor is, for an element that draws what it is
    /// over itself, from its prepaint (the terminal's cursor cell).
    pub fn mark(&self, bounds: Bounds<Pixels>) {
        self.0.set(Some(bounds));
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
    hang(anchors, priority, gpui::Corner::TopLeft, panel)
}

/// `below`, the panel's `corner` at the anchor's opposite edge: a top
/// corner hangs under the anchor, a bottom one stands above it, with the
/// height left on that side.
fn hang(
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
                let (at, room, offset) = match corner {
                    gpui::Corner::TopLeft => (
                        bounds.bottom_left(),
                        window.viewport_size().height - bounds.bottom() - gap - margin,
                        gap,
                    ),
                    gpui::Corner::TopRight => (
                        bounds.bottom_right(),
                        window.viewport_size().height - bounds.bottom() - gap - margin,
                        gap,
                    ),
                    gpui::Corner::BottomLeft => (bounds.origin, bounds.top() - gap - margin, -gap),
                    gpui::Corner::BottomRight => {
                        (bounds.top_right(), bounds.top() - gap - margin, -gap)
                    }
                };
                let mut popup = gpui::anchored()
                    .anchor(corner)
                    .position(at)
                    .offset(gpui::point(px(0.), offset))
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
