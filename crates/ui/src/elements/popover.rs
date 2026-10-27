//! Same-frame button anchors, shared across responsive toolbar variants.
use std::{cell::Cell, rc::Rc};

use gpui::{
    AnyElement, AvailableSpace, Bounds, IntoElement, Pixels, Point, Window, canvas, prelude::*, px,
    size,
};

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

    /// `panel` under the anchor's right edge, reaching left: it stays
    /// where it is as what is before the anchor comes and goes (the view
    /// buttons' menu).
    pub fn popup_right(&self, panel: impl IntoElement) -> AnyElement {
        let mut panel = Some(panel.into_any_element());
        hang(
            std::slice::from_ref(self),
            3,
            gpui::Anchor::TopRight,
            Room::Side,
            move || panel.take(),
        )
    }

    /// `panel` around what the anchor is over: `pad` before, above and
    /// after it, at least `least` wide, moved left to stay in the window
    /// but never short of the anchor's right edge. The address field's
    /// card (ADR-098): its top is the field, which draws itself above the
    /// card (deferred at a higher priority), so the text stays where it is
    /// typed.
    pub fn popup_around(
        &self,
        priority: usize,
        pad: Pixels,
        least: Pixels,
        panel: impl IntoElement,
    ) -> AnyElement {
        let anchor = self.0.clone();
        let panel = panel.into_any_element();
        gpui::deferred(
            canvas(
                move |_, window, cx| {
                    let bounds = anchor.get()?;
                    let viewport = window.viewport_size();
                    let width = (bounds.size.width + pad * 2.).max(least);
                    let right = (viewport.width - MARGIN).max(bounds.right() + pad);
                    let left = (bounds.left() - pad).min(right - width).max(px(0.));
                    let top = bounds.top() - pad;
                    let room = (viewport.height - top - MARGIN).max(px(120.));
                    let mut popup = gpui::div()
                        .w(width)
                        .max_h(room)
                        .flex()
                        .flex_col()
                        .child(panel)
                        .into_any_element();
                    popup.layout_as_root(
                        size(AvailableSpace::Definite(width), AvailableSpace::MaxContent),
                        window,
                        cx,
                    );
                    popup.prepaint_at(gpui::point(left, top), window, cx);
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
            gpui::Anchor::BottomRight,
            Room::Side,
            move || {
                let last = panels.len().checked_sub(1)?;
                Some(panels.swap_remove(chosen.get().unwrap_or(last).min(last)))
            },
        )
    }

    /// A menu under the first of `anchors` drawn this frame: one opened
    /// from the keyboard hangs from the row it is about, or from the pane
    /// when no row is. Drawn at `priority` among deferred draws. It takes
    /// the window's height (`Room::Window`).
    pub fn popup_first(
        anchors: &[PopoverAnchor],
        priority: usize,
        panel: AnyElement,
    ) -> AnyElement {
        let mut panel = Some(panel);
        hang(
            anchors,
            priority,
            gpui::Anchor::TopLeft,
            Room::Window,
            move || panel.take(),
        )
    }

    /// `popup_first`, standing above the anchor: for a menu at the
    /// window's foot (the terminal's, above its cursor).
    pub fn popup_first_above(
        anchors: &[PopoverAnchor],
        priority: usize,
        panel: AnyElement,
    ) -> AnyElement {
        let mut panel = Some(panel);
        hang(
            anchors,
            priority,
            gpui::Anchor::BottomLeft,
            Room::Window,
            move || panel.take(),
        )
    }

    /// A menu whose corner is at `position`, where the pointer pressed,
    /// taking the window's height (`Room::Window`).
    pub fn popup_at(position: Point<Pixels>, priority: usize, panel: AnyElement) -> AnyElement {
        let mut panel = Some(panel);
        place(
            priority,
            move |window| {
                let room = window.viewport_size().height - MARGIN * 2.;
                Some((
                    gpui::anchored()
                        .position(position)
                        .snap_to_window_with_margin(MARGIN),
                    room,
                ))
            },
            move || panel.take(),
        )
    }

    /// A submenu beside the row the anchor is over: its first row level
    /// with that row, `inset` being the room between its panel's edge and
    /// its rows, the panel meeting the menu's. On the row's right, or its
    /// left where the window has no room for it there; moved up to stay in
    /// the window, and as tall as the window at most. Measured, then
    /// placed, in the deferred prepaint, after the row's (`priority`
    /// above the menu's).
    pub fn popup_beside(&self, priority: usize, inset: Pixels, panel: AnyElement) -> AnyElement {
        let anchor = self.0.clone();
        gpui::deferred(
            canvas(
                move |_, window, cx| {
                    let row = anchor.get()?;
                    let view = window.viewport_size();
                    let mut popup = gpui::div()
                        .flex()
                        .flex_col()
                        .max_h((view.height - MARGIN * 2.).max(px(120.)))
                        .child(panel)
                        .into_any_element();
                    let size = popup.layout_as_root(
                        size(AvailableSpace::MaxContent, AvailableSpace::MaxContent),
                        window,
                        cx,
                    );
                    // Its panel's edge meets the menu's: the row's edge
                    // plus the menu's own inset, less the two borders.
                    let reach = inset - px(2.);
                    let x = if row.right() + reach + size.width <= view.width - MARGIN {
                        row.right() + reach
                    } else {
                        (row.left() - reach - size.width).max(MARGIN)
                    };
                    let y = (row.top() - inset)
                        .min(view.height - MARGIN - size.height)
                        .max(MARGIN);
                    popup.prepaint_at(gpui::point(x, y), window, cx);
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

    /// Where the anchor was drawn this frame, if it was.
    pub fn bounds(&self) -> Option<Bounds<Pixels>> {
        self.0.get()
    }

    /// Mark where the anchor is, for an element that draws what it is
    /// over itself, from its prepaint (the terminal's cursor cell).
    pub fn mark(&self, bounds: Bounds<Pixels>) {
        self.0.set(Some(bounds));
    }
}

/// How tall a popup may be.
#[derive(Clone, Copy)]
enum Room {
    /// What is left on the anchor's side: a toolbar's popup hangs from
    /// its button and shrinks to that.
    Side,
    /// The window's: a menu is moved to stay in the window, and shrinks
    /// only when taller than it.
    Window,
}

/// Room kept between a popup and the window's edge.
const MARGIN: Pixels = px(8.);

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
    hang(anchors, priority, gpui::Anchor::TopLeft, Room::Side, panel)
}

/// `below`, the panel's `corner` at the anchor's opposite edge: a top
/// corner hangs under the anchor, a bottom one stands above it, with the
/// height `room` gives.
fn hang(
    anchors: &[PopoverAnchor],
    priority: usize,
    corner: gpui::Anchor,
    room: Room,
    panel: impl FnMut() -> Option<AnyElement> + 'static,
) -> AnyElement {
    let anchors: Vec<_> = anchors.iter().map(|a| a.0.clone()).collect();
    place(
        priority,
        move |window| {
            let bounds = anchors.iter().find_map(|a| a.get())?;
            let gap = px(6.);
            let height = window.viewport_size().height;
            let below = height - bounds.bottom() - gap - MARGIN;
            let above = bounds.top() - gap - MARGIN;
            let center = bounds.center().x;
            // Popups hang by a corner or a side's middle; one anchored at a
            // side's middle (never asked here) hangs below it.
            let (at, side, offset) = match corner {
                gpui::Anchor::TopLeft | gpui::Anchor::LeftCenter => {
                    (bounds.bottom_left(), below, gap)
                }
                gpui::Anchor::TopRight | gpui::Anchor::RightCenter => {
                    (bounds.bottom_right(), below, gap)
                }
                gpui::Anchor::TopCenter => (gpui::point(center, bounds.bottom()), below, gap),
                gpui::Anchor::BottomLeft => (bounds.origin, above, -gap),
                gpui::Anchor::BottomRight => (bounds.top_right(), above, -gap),
                gpui::Anchor::BottomCenter => (gpui::point(center, bounds.top()), above, -gap),
            };
            let room = match room {
                Room::Side => side,
                Room::Window => height - MARGIN * 2.,
            };
            Some((
                gpui::anchored()
                    .anchor(corner)
                    .position(at)
                    .offset(gpui::point(px(0.), offset))
                    .snap_to_window_with_margin(MARGIN),
                room,
            ))
        },
        panel,
    )
}

/// Draw `panel` deferred, at `priority`: in prepaint, where the window
/// and the anchors are measured, `at` says where it stands and how tall
/// it may be, and it is laid out there.
fn place(
    priority: usize,
    at: impl Fn(&Window) -> Option<(gpui::Anchored, Pixels)> + 'static,
    mut panel: impl FnMut() -> Option<AnyElement> + 'static,
) -> AnyElement {
    gpui::deferred(
        canvas(
            move |_, window, cx| {
                let (popup, room) = at(window)?;
                let mut popup = popup
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
