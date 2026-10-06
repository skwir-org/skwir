//! Rubber-band selection: press on the empty part of a view and drag a
//! rectangle over the entries to select.
//!
//! The element sits over a view's scroll area and owns no layout. It knows
//! its bounds where they are real (prepaint), reads the view's scroll offset
//! and tile slots when it paints (after the view's own prepaint, so they are
//! this frame's), turns pointer positions into content coordinates, and asks
//! pure geometry which entries a rectangle covers. The drag itself lives in the shell,
//! anchored in content coordinates, so it survives re-renders and scrolling.

use gpui::{
    App, Bounds, Element, ElementId, GlobalElementId, Hitbox, HitboxBehavior, InspectorElementId,
    IntoElement, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    Point, ScrollHandle, Style, WeakEntity, Window, point, px, relative,
};

use bibli_core::workspace::PaneId;

use crate::elements::uniform_grid::UniformGridScrollHandle;
use crate::elements::uniform_grid::math::Slots;
use crate::shell::Shell;
use crate::theme::Theme;

/// Where a view puts its entries, in content coordinates (origin at the top
/// left of the scrolled content).
#[derive(Clone, Debug, PartialEq)]
pub enum Geometry {
    /// Full-width rows of one height.
    Rows { height: f32 },
    /// A grid's tiles, placed from `origin` (the grid's border and
    /// padding). The gutters between them are empty: a band can begin there.
    Cells { slots: Slots, origin: (f32, f32) },
}

impl Geometry {
    /// The entry under a point, if any. Rows span the width; the space after
    /// the last entry, and beside the last column, is empty.
    pub fn item_at(&self, x: f32, y: f32, count: usize) -> Option<usize> {
        if x < 0. || y < 0. {
            return None;
        }
        let index = match self {
            Geometry::Rows { height } => (y / *height) as usize,
            Geometry::Cells { slots, origin } => slots.tile_at(x - origin.0, y - origin.1)?,
        };
        (index < count).then_some(index)
    }

    /// Entries a rectangle touches, given by two opposite corners.
    pub fn covered(&self, a: (f32, f32), b: (f32, f32), count: usize) -> Vec<usize> {
        let (x0, x1) = (a.0.min(b.0), a.0.max(b.0));
        let (y0, y1) = (a.1.min(b.1).max(0.), a.1.max(b.1));
        if count == 0 || y1 < 0. {
            return Vec::new();
        }
        match self {
            Geometry::Rows { height } => {
                let first = (y0 / height) as usize;
                let last = ((y1 / height) as usize).min(count - 1);
                (first..=last).filter(|index| *index < count).collect()
            }
            Geometry::Cells { slots, origin } => slots
                .covered(
                    (x0 - origin.0, x1 - origin.0),
                    (y0 - origin.1, y1 - origin.1),
                )
                .into_iter()
                .filter(|index| *index < count)
                .collect(),
        }
    }
}

/// A drag in progress, kept by the shell.
#[derive(Clone, Debug)]
pub struct Drag {
    pub pane: PaneId,
    /// Where it began, in content coordinates: scrolling extends the band.
    pub start: (f32, f32),
    /// The pointer now, in window coordinates.
    pub current: Point<Pixels>,
    /// What the last move selected, to skip moves that change nothing.
    pub covered: Vec<usize>,
    /// Entries selected before the drag, kept when it began with Ctrl/Shift.
    pub kept: Vec<bibli_core::EntryId>,
}

/// Where a view's entries are, read when the marquee paints: after the
/// view's own prepaint, so the scroll offset and the grid's slots are this
/// frame's, never the last one's.
pub enum Source {
    Rows { height: f32 },
    Grid(UniformGridScrollHandle),
}

impl Source {
    fn geometry(&self) -> Geometry {
        match self {
            Source::Rows { height } => Geometry::Rows { height: *height },
            Source::Grid(grid) => Geometry::Cells {
                slots: grid.slots(),
                origin: grid.origin(),
            },
        }
    }
}

pub struct Marquee {
    pub pane: PaneId,
    pub shell: WeakEntity<Shell>,
    pub source: Source,
    pub count: usize,
    /// The handle the view scrolls with.
    pub scroll: ScrollHandle,
    pub drag: Option<Drag>,
}

impl IntoElement for Marquee {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for Marquee {
    type RequestLayoutState = ();
    type PrepaintState = Hitbox;

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
        _: &mut App,
    ) -> Hitbox {
        // A plain hitbox: it blocks nothing below, and tells us whether a
        // menu or dialog above covers the pointer.
        window.insert_hitbox(bounds, HitboxBehavior::Normal)
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        (): &mut (),
        hitbox: &mut Hitbox,
        window: &mut Window,
        cx: &mut App,
    ) {
        // Painted after the view laid itself out and scrolled: current.
        let (geometry, count, offset, pane) = (
            self.source.geometry(),
            self.count,
            self.scroll.offset(),
            self.pane,
        );
        let to_content = move |position: Point<Pixels>| {
            (
                f32::from(position.x - bounds.origin.x - offset.x),
                f32::from(position.y - bounds.origin.y - offset.y),
            )
        };
        if let Some(drag) = self.drag.as_ref().filter(|drag| drag.pane == pane) {
            let start = point(
                bounds.origin.x + offset.x + px(drag.start.0),
                bounds.origin.y + offset.y + px(drag.start.1),
            );
            let band = Bounds::from_corners(
                point(start.x.min(drag.current.x), start.y.min(drag.current.y)),
                point(start.x.max(drag.current.x), start.y.max(drag.current.y)),
            )
            .intersect(&bounds);
            if band.size.width > px(3.) || band.size.height > px(3.) {
                let theme = cx.global::<Theme>();
                window.paint_quad(gpui::quad(
                    band,
                    px(2.),
                    theme.band,
                    px(1.),
                    theme.band_edge,
                    gpui::BorderStyle::default(),
                ));
            }
        }

        let shell = self.shell.clone();
        let hitbox = hitbox.clone();
        let pressed = geometry.clone();
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
            if !phase.bubble() || event.button != MouseButton::Left || !hitbox.is_hovered(window) {
                return;
            }
            let (x, y) = to_content(event.position);
            if pressed.item_at(x, y, count).is_some() {
                return;
            }
            let keep = event.modifiers.secondary() || event.modifiers.shift;
            shell
                .update(cx, |shell, cx| {
                    shell.marquee_begin(pane, (x, y), event.position, keep, window, cx);
                })
                .ok();
        });
        // Registered every frame, not only while a drag is known: a quick
        // click presses and releases before the repaint that would register
        // them, and a band must never outlive its button.
        let shell = self.shell.clone();
        window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
            if phase.bubble() && event.button == MouseButton::Left {
                shell.update(cx, Shell::marquee_end).ok();
            }
        });
        let Some(start) = self
            .drag
            .as_ref()
            .filter(|drag| drag.pane == pane)
            .map(|drag| drag.start)
        else {
            return;
        };
        let shell = self.shell.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
            if !phase.bubble() {
                return;
            }
            // The release can be missed (it may land outside the window).
            if event.pressed_button != Some(MouseButton::Left) {
                shell.update(cx, Shell::marquee_end).ok();
                return;
            }
            let covered = geometry.covered(start, to_content(event.position), count);
            shell
                .update(cx, |shell, cx| {
                    shell.marquee_move(event.position, covered, window, cx);
                })
                .ok();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{Geometry, Slots};
    use crate::elements::uniform_grid::math::Columns;

    #[test]
    fn rows_are_hit_across_their_width_and_the_space_below_is_empty() {
        let rows = Geometry::Rows { height: 26. };
        assert_eq!(rows.item_at(900., 30., 3), Some(1));
        assert_eq!(rows.item_at(10., 26. * 3. + 1., 3), None);
        assert_eq!(rows.covered((5., 100.), (400., 20.), 10), [0, 1, 2, 3]);
        assert_eq!(
            rows.covered((5., 60.), (9., 9000.), 4),
            [2, 3],
            "clamped to the entries"
        );
        assert!(rows.covered((0., 0.), (10., 10.), 0).is_empty());
    }

    #[test]
    fn tiles_are_hit_inside_their_gutters_and_the_band_selects_a_block() {
        // Four 100 by 120 tiles a row, each 8 px from the edge of its slot.
        let cells = Geometry::Cells {
            slots: Slots::new(Columns::fit(464., 100., 16., 40.), 16., 10, |_| 120.),
            origin: (4., 0.),
        };
        assert_eq!(cells.item_at(130., 150., 10), Some(5));
        assert_eq!(cells.item_at(124., 150., 10), None, "in the gutter");
        assert_eq!(cells.item_at(370., 290., 10), None, "past the last entry");
        assert_eq!(cells.item_at(480., 10., 10), None, "beside the last column");
        assert_eq!(cells.covered((130., 10.), (300., 150.), 10), [1, 2, 5, 6]);
        assert_eq!(
            cells.covered((360., 150.), (900., 400.), 10),
            [7],
            "the rest are past the end"
        );
    }
}
