//! A line's elevation under its map: altitude over distance, a track's as
//! it was recorded, a plan's as it was planned (its altitudes from its
//! datum: the sea, the ground, the takeoff). The pointer over it reads the
//! distance and the altitude there and marks the place on the map
//! (`Handles::mark`), so a climb is found on the ground.
//!
//! The curve is placed in `prepaint`, from the bounds it really has; the
//! profile itself is built once per file, on the worker, with the map's
//! shapes (`inspector::Display`).
use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    App, Bounds, DispatchPhase, Element, ElementId, GlobalElementId, Hitbox, HitboxBehavior, Hsla,
    InspectorElementId, IntoElement, LayoutId, MouseMoveEvent, PathBuilder, Pixels, ShapedLine,
    Style, Window, point, px, relative, size,
};
use skwir_core::geo::{Datum, GeoDoc, LatLon, LineKind};

/// Points drawn at most: more than a profile's width has pixels.
const MOST_POINTS: usize = 2000;
/// Room left inside the element: labels on the left, a margin elsewhere.
const LEFT: f32 = 44.;
const MARGIN: f32 = 6.;

/// Altitude over distance along a file's lines.
#[derive(Debug, PartialEq)]
pub struct Profile {
    /// Metres along the lines, and the altitude there.
    pub points: Vec<(f64, f32)>,
    /// Where each point is.
    pub places: Vec<LatLon>,
    pub datum: Datum,
    /// A plan's altitudes, not a recording's.
    pub planned: bool,
    pub low: f32,
    pub high: f32,
    pub length: f64,
}

impl Profile {
    /// The profile of `doc`'s tracks, or of its routes when it has no
    /// track with altitudes; `None` when fewer than two points have one.
    /// Segments follow one another: a gap in a recording adds no
    /// distance.
    pub fn of(doc: &GeoDoc) -> Option<Profile> {
        let with = |kind: LineKind| {
            doc.lines
                .iter()
                .filter(move |l| l.kind == kind)
                .filter(|l| l.segments.iter().flatten().any(|p| p.altitude.is_some()))
        };
        let (kind, lines): (LineKind, Vec<_>) = if with(LineKind::Track).next().is_some() {
            (LineKind::Track, with(LineKind::Track).collect())
        } else {
            (LineKind::Route, with(LineKind::Route).collect())
        };
        let datum = lines.first()?.datum;
        let mut points = Vec::new();
        let mut places = Vec::new();
        let mut along = 0.;
        for segment in lines.iter().flat_map(|l| &l.segments) {
            let mut last: Option<LatLon> = None;
            for point in segment {
                if let Some(before) = last {
                    along += before.distance(point.at);
                }
                last = Some(point.at);
                if let Some(altitude) = point.altitude {
                    points.push((along, altitude));
                    places.push(point.at);
                }
            }
        }
        if points.len() < 2 {
            return None;
        }
        // Evenly fewer, the ends kept.
        if points.len() > MOST_POINTS {
            let step = points.len().div_ceil(MOST_POINTS);
            let last = points.len() - 1;
            let keep = |i: usize| i.is_multiple_of(step) || i == last;
            points = points
                .into_iter()
                .enumerate()
                .filter(|(i, _)| keep(*i))
                .map(|(_, p)| p)
                .collect();
            places = places
                .into_iter()
                .enumerate()
                .filter(|(i, _)| keep(*i))
                .map(|(_, p)| p)
                .collect();
        }
        let (low, high) = points
            .iter()
            .fold((f32::MAX, f32::MIN), |(l, h), &(_, a)| (l.min(a), h.max(a)));
        let length = points.last().map_or(0., |p| p.0);
        Some(Profile {
            points,
            places,
            datum,
            planned: kind == LineKind::Route,
            low,
            high,
            length,
        })
    }

    /// Where the point at `along` metres is, in a chart of `width` by
    /// `height` pixels: distance across, altitude up, a flat line in the
    /// middle.
    pub fn place(&self, along: f64, altitude: f32, width: f32, height: f32) -> (f32, f32) {
        let x = if self.length > 0. {
            (along / self.length) as f32 * width
        } else {
            0.
        };
        let span = self.high - self.low;
        let y = if span > 0. {
            height - (altitude - self.low) / span * height
        } else {
            height / 2.
        };
        (x, y)
    }

    /// The point nearest `x` of `width` pixels across.
    pub fn nearest(&self, x: f32, width: f32) -> usize {
        let along = if width > 0. {
            f64::from(x / width) * self.length
        } else {
            0.
        };
        let i = self.points.partition_point(|p| p.0 < along);
        match (i.checked_sub(1), self.points.get(i)) {
            (Some(before), Some(after)) if along - self.points[before].0 < after.0 - along => {
                before
            }
            (Some(before), None) => before,
            _ => i.min(self.points.len() - 1),
        }
    }

    /// What an altitude is measured from, in the readout.
    pub fn from_words(&self) -> &'static str {
        match self.datum {
            Datum::Sea => "",
            Datum::Ground => " above ground",
            Datum::Takeoff => " above takeoff",
        }
    }
}

pub struct Colors {
    pub line: Hsla,
    pub text: Hsla,
    pub muted: Hsla,
    pub grid: Hsla,
    pub paper: Hsla,
}

pub struct Chart {
    id: ElementId,
    profile: std::sync::Arc<Profile>,
    colors: Colors,
    /// The point the pointer is on, shared with the map that marks it.
    mark: Rc<Cell<Option<LatLon>>>,
    hover: Rc<Cell<Option<usize>>>,
}

pub fn chart(
    id: impl Into<ElementId>,
    profile: std::sync::Arc<Profile>,
    colors: Colors,
    mark: Rc<Cell<Option<LatLon>>>,
    hover: Rc<Cell<Option<usize>>>,
) -> Chart {
    Chart {
        id: id.into(),
        profile,
        colors,
        mark,
        hover,
    }
}

impl IntoElement for Chart {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

pub struct Prepaint {
    hitbox: Hitbox,
    plot: Bounds<Pixels>,
    curve: Vec<gpui::Point<Pixels>>,
    labels: Vec<(gpui::Point<Pixels>, ShapedLine)>,
    readout: Option<(gpui::Point<Pixels>, gpui::Point<Pixels>, ShapedLine)>,
}

fn metres(value: f32) -> String {
    format!("{value:.0} m")
}

impl Element for Chart {
    type RequestLayoutState = ();
    type PrepaintState = Prepaint;

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
        (): &mut (),
        window: &mut Window,
        _: &mut App,
    ) -> Prepaint {
        let plot = Bounds::new(
            bounds.origin + point(px(LEFT), px(MARGIN)),
            size(
                (bounds.size.width - px(LEFT + MARGIN)).max(px(1.)),
                (bounds.size.height - px(2. * MARGIN)).max(px(1.)),
            ),
        );
        let (w, h) = (f32::from(plot.size.width), f32::from(plot.size.height));
        let p = &self.profile;
        let curve = p
            .points
            .iter()
            .map(|&(along, altitude)| {
                let (x, y) = p.place(along, altitude, w, h);
                plot.origin + point(px(x), px(y))
            })
            .collect();
        let shape = |text: String, color: Hsla, window: &mut Window| {
            super::map::shape(text.into(), color, window)
        };
        let from = p.from_words();
        let mut labels = vec![
            (
                bounds.origin + point(px(4.), plot.origin.y - bounds.origin.y - px(2.)),
                shape(metres(p.high), self.colors.muted, window),
            ),
            (
                bounds.origin
                    + point(
                        px(4.),
                        plot.origin.y - bounds.origin.y + plot.size.height - px(12.),
                    ),
                shape(metres(p.low), self.colors.muted, window),
            ),
        ];
        let length = shape(
            format!(
                "{}{}",
                distance(p.length),
                if p.planned { " planned" } else { "" }
            ),
            self.colors.muted,
            window,
        );
        labels.push((
            plot.origin + point(plot.size.width - length.width - px(2.), px(0.)),
            length,
        ));
        let readout = self.hover.get().and_then(|i| {
            let &(along, altitude) = p.points.get(i)?;
            let (x, y) = p.place(along, altitude, w, h);
            let at = plot.origin + point(px(x), px(y));
            let text = shape(
                format!("{} · {}{from}", distance(along), metres(altitude)),
                self.colors.text,
                window,
            );
            // Beside the point, on the side with room.
            let left = if x + f32::from(text.width) + 10. > w {
                at.x - text.width - px(8.)
            } else {
                at.x + px(8.)
            };
            Some((at, point(left, plot.origin.y), text))
        });
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        Prepaint {
            hitbox,
            plot,
            curve,
            labels,
            readout,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        (): &mut (),
        prepaint: &mut Prepaint,
        window: &mut Window,
        cx: &mut App,
    ) {
        let colors = &self.colors;
        let plot = prepaint.plot;
        window.paint_quad(gpui::fill(bounds, colors.paper));
        // The top and the bottom of the range.
        for y in [plot.origin.y, plot.origin.y + plot.size.height] {
            window.paint_quad(gpui::fill(
                Bounds::new(point(plot.origin.x, y), size(plot.size.width, px(1.))),
                colors.grid,
            ));
        }
        if let (Some(first), Some(last)) = (prepaint.curve.first(), prepaint.curve.last()) {
            let floor = plot.origin.y + plot.size.height;
            let mut area = PathBuilder::fill();
            area.move_to(point(first.x, floor));
            for p in &prepaint.curve {
                area.line_to(*p);
            }
            area.line_to(point(last.x, floor));
            area.close();
            if let Ok(path) = area.build() {
                window.paint_path(path, colors.line.opacity(0.18));
            }
            let mut line = PathBuilder::stroke(px(1.5));
            line.move_to(*first);
            for p in &prepaint.curve[1..] {
                line.line_to(*p);
            }
            if let Ok(path) = line.build() {
                window.paint_path(path, colors.line);
            }
        }
        for (at, text) in &prepaint.labels {
            let _ = text.paint(*at, px(14.), gpui::TextAlign::Left, None, window, cx);
        }
        if let Some((at, text_at, text)) = &prepaint.readout {
            window.paint_quad(gpui::fill(
                Bounds::new(point(at.x, plot.origin.y), size(px(1.), plot.size.height)),
                colors.text.opacity(0.4),
            ));
            window.paint_quad(gpui::quad(
                Bounds::new(*at - point(px(4.), px(4.)), size(px(8.), px(8.))),
                px(4.),
                colors.line,
                px(1.5),
                colors.paper,
                gpui::BorderStyle::Solid,
            ));
            window.paint_quad(gpui::quad(
                Bounds::new(
                    *text_at - point(px(3.), px(0.)),
                    size(text.width + px(6.), px(14.)),
                ),
                px(3.),
                colors.paper.opacity(0.9),
                px(0.),
                gpui::transparent_black(),
                gpui::BorderStyle::default(),
            ));
            let _ = text.paint(*text_at, px(14.), gpui::TextAlign::Left, None, window, cx);
        }

        let (hitbox, profile) = (
            prepaint.hitbox.clone(),
            std::sync::Arc::clone(&self.profile),
        );
        let (mark, hover) = (Rc::clone(&self.mark), Rc::clone(&self.hover));
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, _| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            let over = hitbox.is_hovered(window).then(|| {
                let x = f32::from(event.position.x - plot.origin.x)
                    .clamp(0., f32::from(plot.size.width));
                profile.nearest(x, f32::from(plot.size.width))
            });
            if over != hover.get() {
                hover.set(over);
                mark.set(over.and_then(|i| profile.places.get(i).copied()));
                window.refresh();
            }
        });
    }
}

fn distance(metres: f64) -> String {
    if metres < 1000. {
        format!("{metres:.0} m")
    } else {
        format!("{:.1} km", metres / 1000.)
    }
}

#[cfg(test)]
// The values compared are sums and products of small integers and
// halves, exact in binary floating point.
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use skwir_core::geo::{Line, Point};

    fn line(kind: LineKind, datum: Datum, points: &[(f64, Option<f32>)]) -> Line {
        Line {
            name: String::new(),
            kind,
            datum,
            segments: vec![
                points
                    .iter()
                    .map(|&(lon, altitude)| Point {
                        at: LatLon::new(0., lon).unwrap(),
                        altitude,
                        time: None,
                    })
                    .collect(),
            ],
        }
    }

    #[test]
    fn a_profile_follows_the_track_and_skips_what_has_no_altitude() {
        let doc = GeoDoc {
            lines: vec![
                line(
                    LineKind::Route,
                    Datum::Sea,
                    &[(0., Some(9.)), (1., Some(9.))],
                ),
                line(
                    LineKind::Track,
                    Datum::Sea,
                    &[(0., Some(100.)), (0.5, None), (1., Some(300.))],
                ),
            ],
            ..GeoDoc::default()
        };
        let p = Profile::of(&doc).unwrap();
        assert!(!p.planned);
        assert_eq!(p.points.len(), 2);
        // The point without an altitude still counts its distance.
        assert!((p.length - 111_195.).abs() < 1., "{}", p.length);
        assert_eq!((p.low, p.high), (100., 300.));
        // Across, then up: the low end at the bottom, the high at the top.
        assert_eq!(p.place(0., 100., 200., 50.), (0., 50.));
        assert_eq!(p.place(p.length, 300., 200., 50.), (200., 0.));
        assert_eq!(p.nearest(10., 200.), 0);
        assert_eq!(p.nearest(190., 200.), 1);
    }

    #[test]
    fn a_plan_without_a_track_gives_its_planned_altitudes() {
        let doc = GeoDoc {
            lines: vec![line(
                LineKind::Route,
                Datum::Takeoff,
                &[(0., Some(0.)), (0.01, Some(60.)), (0.02, Some(60.))],
            )],
            ..GeoDoc::default()
        };
        let p = Profile::of(&doc).unwrap();
        assert!(p.planned);
        assert_eq!(p.from_words(), " above takeoff");
        assert!(Profile::of(&GeoDoc::default()).is_none());
    }
}
