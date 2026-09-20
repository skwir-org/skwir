//! A scrollbar for anything that scrolls.
//!
//! It overlays the right edge of a scroll area rather than taking a column of
//! its own, so adding one never reflows the view under it. Like every element
//! here it reads sizes where they are real: the thumb is placed in `prepaint`
//! from the scroll handle's own numbers, never from a size measured while
//! rendering.
//!
//! A view supplies whichever handle it already scrolls with. There are two
//! kinds in this codebase — a pixel offset (`overflow_*_scroll` divs, uniform
//! lists, our uniform grid) and `gpui::list`, which scrolls by item and knows
//! only what it has measured — so the bar asks each in its own terms and
//! speaks pixels to the rest of the code.

use gpui::{
    App, Bounds, Element, ElementId, GlobalElementId, Hitbox, HitboxBehavior, InspectorElementId,
    IntoElement, LayoutId, ListState, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, ScrollHandle, Style, Window, point, px, relative, size,
};
use std::cell::Cell;
use std::rc::Rc;

use crate::theme::Theme;

/// Width of the gutter the bar lives in.
const GUTTER: f32 = 12.;
/// The bar at rest and while the pointer is over it.
const BAR: f32 = 5.;
const BAR_HOVER: f32 = 9.;
/// However long the content, the thumb stays grabbable.
const MIN_THUMB: f32 = 24.;

/// The scroll position of a view, in pixels from the top.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    offset: f32,
    /// How far it can scroll; zero when everything fits.
    max: f32,
    viewport: f32,
}

/// What the bar drives.
#[derive(Clone)]
pub enum Scrolls {
    /// A pixel offset: `overflow_*_scroll`, `uniform_list`, `uniform_grid`.
    Offset(ScrollHandle),
    /// `gpui::list`: scrolls by item, measures as it goes.
    Items(ListState),
}

impl Scrolls {
    fn metrics(&self) -> Metrics {
        match self {
            Scrolls::Offset(handle) => Metrics {
                offset: -f32::from(handle.offset().y),
                max: f32::from(handle.max_offset().height),
                viewport: f32::from(handle.bounds().size.height),
            },
            Scrolls::Items(list) => Metrics {
                offset: -f32::from(list.scroll_px_offset_for_scrollbar().y),
                max: f32::from(list.max_offset_for_scrollbar().height),
                viewport: f32::from(list.viewport_bounds().size.height),
            },
        }
    }

    fn scroll_to(&self, y: f32) {
        match self {
            Scrolls::Offset(handle) => {
                let offset = handle.offset();
                handle.set_offset(point(offset.x, px(-y)));
            }
            Scrolls::Items(list) => list.set_offset_from_scrollbar(point(px(0.), px(-y))),
        }
    }

    /// A list measures more of itself as it scrolls, which would move the
    /// thumb out from under the pointer; it freezes its height while dragged.
    fn dragging(&self, started: bool) {
        if let Scrolls::Items(list) = self {
            if started {
                list.scrollbar_drag_started();
            } else {
                list.scrollbar_drag_ended();
            }
        }
    }
}

/// Where the pointer grabbed the thumb, kept between frames.
#[derive(Clone, Default)]
struct Grab(Rc<Cell<Option<f32>>>);

pub struct Scrollbar {
    id: ElementId,
    scrolls: Scrolls,
}

/// A vertical scrollbar over `scrolls`. The id must be stable across frames,
/// or a drag ends the moment the view repaints.
pub fn vertical(id: impl Into<ElementId>, scrolls: Scrolls) -> Scrollbar {
    Scrollbar {
        id: id.into(),
        scrolls,
    }
}

impl IntoElement for Scrollbar {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

/// The thumb inside `track`, or `None` when everything fits.
fn thumb(track: Bounds<Pixels>, metrics: Metrics) -> Option<Bounds<Pixels>> {
    let height = f32::from(track.size.height);
    if metrics.max <= 1. || metrics.viewport <= 0. || height <= MIN_THUMB {
        return None;
    }
    let content = metrics.viewport + metrics.max;
    let thumb = (height * metrics.viewport / content).clamp(MIN_THUMB, height);
    let travel = height - thumb;
    let progress = (metrics.offset / metrics.max).clamp(0., 1.);
    Some(Bounds::new(
        point(track.origin.x, track.origin.y + px(travel * progress)),
        size(track.size.width, px(thumb)),
    ))
}

impl Element for Scrollbar {
    type RequestLayoutState = ();
    type PrepaintState = (Hitbox, Option<Bounds<Pixels>>, Metrics);

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
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
        style.inset.right = px(0.).into();
        style.size.width = px(GUTTER).into();
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
        _: &mut App,
    ) -> Self::PrepaintState {
        let metrics = self.scrolls.metrics();
        let thumb = thumb(bounds, metrics);
        // The gutter takes the pointer only while there is a bar in it: a
        // press here must scroll, not select the row underneath. With
        // nothing to scroll it is not there at all.
        let behavior = if thumb.is_some() {
            HitboxBehavior::BlockMouse
        } else {
            HitboxBehavior::Normal
        };
        (window.insert_hitbox(bounds, behavior), thumb, metrics)
    }

    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        (): &mut (),
        (hitbox, thumb, metrics): &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let (Some(thumb), Some(global_id)) = (*thumb, global_id) else {
            return;
        };
        let metrics = *metrics;
        let scrolls = self.scrolls.clone();
        window.with_element_state::<Grab, _>(global_id, |grab, window| {
            let grab: Grab = grab.unwrap_or_default();
            let held = grab.0.get().is_some();
            let hovered = hitbox.is_hovered(window);
            let theme = cx.global::<Theme>();
            let width = px(if held || hovered { BAR_HOVER } else { BAR });
            // Right-aligned inside the gutter, so the bar hugs the edge.
            let bar = Bounds::new(
                point(bounds.origin.x + bounds.size.width - width, thumb.origin.y),
                size(width, thumb.size.height),
            );
            if held || hovered {
                window.paint_quad(gpui::fill(
                    Bounds::new(
                        point(bounds.origin.x + bounds.size.width - width, bounds.origin.y),
                        size(width, bounds.size.height),
                    ),
                    theme.hover,
                ));
            }
            window.paint_quad(gpui::quad(
                bar,
                px(f32::from(width) / 2.),
                theme
                    .text_muted
                    .opacity(if held || hovered { 0.8 } else { 0.45 }),
                px(0.),
                gpui::transparent_black(),
                gpui::BorderStyle::default(),
            ));

            let travel = f32::from(bounds.size.height - thumb.size.height);
            let top = bounds.origin.y;
            let to_offset = move |y: Pixels, grabbed: f32| {
                if travel <= 0. {
                    return 0.;
                }
                let position = f32::from(y - top) - grabbed;
                (position / travel).clamp(0., 1.) * metrics.max
            };

            let down = grab.clone();
            let hit = hitbox.clone();
            let handle = scrolls.clone();
            window.on_mouse_event(move |event: &MouseDownEvent, phase, window, _| {
                if !phase.bubble() || event.button != MouseButton::Left || !hit.is_hovered(window) {
                    return;
                }
                // On the thumb: drag from where it was grabbed. Beside it:
                // jump there, then keep dragging from its middle.
                let grabbed = if thumb.contains(&event.position) {
                    f32::from(event.position.y - thumb.origin.y)
                } else {
                    let middle = f32::from(thumb.size.height) / 2.;
                    handle.scroll_to(to_offset(event.position.y, middle));
                    middle
                };
                down.0.set(Some(grabbed));
                handle.dragging(true);
                window.refresh();
            });

            let moved = grab.clone();
            let handle = scrolls.clone();
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, _| {
                if !phase.bubble() {
                    return;
                }
                let Some(grabbed) = moved.0.get() else {
                    return;
                };
                // A release outside the window is never delivered here.
                if event.pressed_button != Some(MouseButton::Left) {
                    moved.0.set(None);
                    handle.dragging(false);
                    return;
                }
                handle.scroll_to(to_offset(event.position.y, grabbed));
                window.refresh();
            });

            let up = grab.clone();
            let handle = scrolls.clone();
            window.on_mouse_event(move |event: &MouseUpEvent, phase, window, _| {
                if phase.bubble() && event.button == MouseButton::Left && up.0.take().is_some() {
                    handle.dragging(false);
                    window.refresh();
                }
            });
            ((), grab)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{MIN_THUMB, Metrics, thumb};
    use gpui::{Bounds, point, px, size};

    fn track() -> Bounds<gpui::Pixels> {
        Bounds::new(point(px(0.), px(0.)), size(px(10.), px(200.)))
    }

    #[test]
    fn the_thumb_is_as_long_as_the_view_is_of_the_content() {
        // Half the content is visible: half the track, at the top.
        let half = thumb(
            track(),
            Metrics {
                offset: 0.,
                max: 200.,
                viewport: 200.,
            },
        )
        .expect("a thumb");
        assert_eq!(half.size.height, px(100.));
        assert_eq!(half.origin.y, px(0.));
        // Scrolled to the end, it sits at the bottom of the track.
        let end = thumb(
            track(),
            Metrics {
                offset: 200.,
                max: 200.,
                viewport: 200.,
            },
        )
        .expect("a thumb");
        assert_eq!(end.origin.y + end.size.height, px(200.));
    }

    #[test]
    fn nothing_to_scroll_means_no_thumb() {
        assert!(
            thumb(
                track(),
                Metrics {
                    offset: 0.,
                    max: 0.,
                    viewport: 500.,
                }
            )
            .is_none()
        );
    }

    #[test]
    fn a_very_long_document_keeps_a_grabbable_thumb() {
        let long = thumb(
            track(),
            Metrics {
                offset: 500_000.,
                max: 1_000_000.,
                viewport: 200.,
            },
        )
        .expect("a thumb");
        assert_eq!(long.size.height, px(MIN_THUMB));
        // Half way down the content, half way down the travel.
        assert_eq!(long.origin.y, px((200. - MIN_THUMB) / 2.));
    }
}
