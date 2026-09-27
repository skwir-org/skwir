//! A map: what a file places on the Earth (`bibli_core::geo`), drawn in the
//! Web Mercator projection every tile source uses (ADR-020), over a
//! graticule until there are tiles to draw under it.
//!
//! Like every element here it reads sizes where they are real: the view is
//! fitted, the lines are projected, thinned to the pixels they cover and
//! clipped to the element in `prepaint`, never in `render`. The drawn view
//! is published (`Map::shown`) so a key can zoom from it; that is its only
//! later reader.
//!
//! Coordinates: a place's *world* position is its projection on a square
//! of side 1 (x east from −180°, y south from 85.05° N); at zoom `z` the
//! world is `TILE · 2^z` pixels wide. A line keeps its x continuous
//! across ±180° (`Shapes::new`), so a track over the Bering Strait is
//! drawn across it, not across the world.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use std::collections::{HashMap, HashSet};

use bibli_core::EntryId;
use bibli_core::geo::{BoundsBuilder, GeoDoc, LatLon, LineKind, WaypointKind};
use gpui::{
    App, BorderStyle, Bounds, ContentMask, CursorStyle, DispatchPhase, Element, ElementId,
    FillOptions, FillRule, GlobalElementId, Hitbox, HitboxBehavior, Hsla, InspectorElementId,
    IntoElement, LayoutId, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    PathBuilder, PathStyle, Pixels, ScrollDelta, ScrollWheelEvent, ShapedLine, SharedString, Style,
    TextRun, Window, point, px, quad, relative, size,
};

pub use math::View;

/// Pixels of the world's side at zoom 0: the tile size of every source.
pub const TILE: f64 = 256.;
pub const MAX_ZOOM: f64 = 19.;
/// A single place is shown at this zoom: a few streets around it.
const POINT_ZOOM: f64 = 15.;
/// Room left around what is fitted, in pixels.
const FIT_PADDING: f64 = 32.;
/// Markers are labelled while at most this many are in view.
const LABELS: usize = 40;

/// The pure part: projection, views, fitting and the scales drawn on top.
pub mod math {
    use std::collections::HashMap;

    use super::{FIT_PADDING, MAX_ZOOM, POINT_ZOOM, TILE};
    use bibli_core::geo::{EARTH_RADIUS, LatLon};

    /// The latitude where the square world ends.
    pub const MAX_LAT: f64 = 85.051_128_779_806_59;

    pub fn project(at: LatLon) -> (f64, f64) {
        let lat = at.lat.clamp(-MAX_LAT, MAX_LAT).to_radians();
        let x = (at.lon + 180.) / 360.;
        let y = (1. - (lat.tan() + 1. / lat.cos()).ln() / std::f64::consts::PI) / 2.;
        (x, y)
    }

    /// Latitude and longitude of a world position; the longitude is not
    /// brought back into ±180°, so a graticule east of the meridian goes on
    /// counting.
    pub fn unproject(x: f64, y: f64) -> (f64, f64) {
        let n = std::f64::consts::PI * (1. - 2. * y);
        (n.sinh().atan().to_degrees(), x * 360. - 180.)
    }

    /// The world's side in pixels at `zoom`.
    pub fn scale(zoom: f64) -> f64 {
        TILE * zoom.exp2()
    }

    /// A rectangle of the world, x continuous (it may pass 1).
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Rect {
        pub x0: f64,
        pub y0: f64,
        pub x1: f64,
        pub y1: f64,
    }

    impl Rect {
        pub fn center(&self) -> (f64, f64) {
            (
                f64::midpoint(self.x0, self.x1),
                f64::midpoint(self.y0, self.y1),
            )
        }
    }

    /// What is shown: the world position at the element's centre, and the
    /// zoom (fractional: the wheel zooms smoothly).
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct View {
        pub x: f64,
        pub y: f64,
        pub zoom: f64,
    }

    impl View {
        /// The whole world, as large as `size` holds it.
        pub fn world(size: (f64, f64)) -> View {
            View {
                x: 0.5,
                y: 0.5,
                zoom: (size.0.min(size.1) / TILE).log2().clamp(0., MAX_ZOOM),
            }
        }

        /// Within what can be shown: zoom in range, x on the first copy of
        /// the world, and no sky above the pole while the world is taller
        /// than the view.
        #[must_use]
        pub fn clamp(self, height: f64) -> View {
            let zoom = self.zoom.clamp(0., MAX_ZOOM);
            let half = height / 2. / scale(zoom);
            let y = if half >= 0.5 {
                0.5
            } else {
                self.y.clamp(half, 1. - half)
            };
            View {
                x: self.x.rem_euclid(1.),
                y,
                zoom,
            }
        }

        /// A world position in pixels from the element's top left.
        pub fn to_screen(&self, (x, y): (f64, f64), size: (f64, f64)) -> (f64, f64) {
            let s = scale(self.zoom);
            (
                (x - self.x) * s + size.0 / 2.,
                (y - self.y) * s + size.1 / 2.,
            )
        }

        pub fn to_world(&self, (sx, sy): (f64, f64), size: (f64, f64)) -> (f64, f64) {
            let s = scale(self.zoom);
            (
                (sx - size.0 / 2.) / s + self.x,
                (sy - size.1 / 2.) / s + self.y,
            )
        }

        /// Moved by `dx`, `dy` pixels: what was under the pointer stays
        /// under it.
        #[must_use]
        pub fn pan(self, dx: f64, dy: f64) -> View {
            let s = scale(self.zoom);
            View {
                x: self.x - dx / s,
                y: self.y - dy / s,
                ..self
            }
        }

        /// Zoomed by `levels` (each doubles the scale) around `anchor`, in
        /// pixels from the top left: the place under it stays there.
        #[must_use]
        pub fn zoom_at(self, levels: f64, anchor: (f64, f64), size: (f64, f64)) -> View {
            let zoom = (self.zoom + levels).clamp(0., MAX_ZOOM);
            let under = self.to_world(anchor, size);
            let s = scale(zoom);
            View {
                x: under.0 - (anchor.0 - size.0 / 2.) / s,
                y: under.1 - (anchor.1 - size.1 / 2.) / s,
                zoom,
            }
        }
    }

    /// The view that shows all of `rect` in `size`, with room around it.
    pub fn fit(rect: Rect, size: (f64, f64)) -> View {
        let (x, y) = rect.center();
        let (w, h) = (rect.x1 - rect.x0, rect.y1 - rect.y0);
        let room = (
            (size.0 - 2. * FIT_PADDING).max(1.),
            (size.1 - 2. * FIT_PADDING).max(1.),
        );
        let zoom = if w <= 0. && h <= 0. {
            POINT_ZOOM
        } else {
            let across = if w > 0. {
                room.0 / (w * TILE)
            } else {
                f64::MAX
            };
            let down = if h > 0. {
                room.1 / (h * TILE)
            } else {
                f64::MAX
            };
            across.min(down).log2().min(POINT_ZOOM)
        };
        View {
            x,
            y,
            zoom: zoom.clamp(0., MAX_ZOOM),
        }
    }

    /// The copy of the world things at `x` are drawn in: the one nearest
    /// the view, so panning past ±180° finds them again.
    pub fn copy_offset(view_x: f64, x: f64) -> f64 {
        (view_x - x).round()
    }

    /// Degrees between graticule lines: the finest step that leaves at
    /// least `gap` pixels between lines.
    pub fn graticule_step(zoom: f64, gap: f64) -> f64 {
        const STEPS: [f64; 17] = [
            0.0005, 0.001, 0.002, 0.005, 0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1., 2., 5., 10., 20.,
            30., 60.,
        ];
        let per_degree = scale(zoom) / 360.;
        STEPS
            .into_iter()
            .find(|step| step * per_degree >= gap)
            .unwrap_or(90.)
    }

    /// A graticule line's label: `45°N`, `6.5°E`, as precise as the step.
    pub fn degrees(value: f64, step: f64, positive: char, negative: char) -> String {
        let decimals = (0..4)
            .find(|d| (step * 10f64.powi(*d)).fract().abs() < 1e-6)
            .unwrap_or(4) as usize;
        let hemisphere = if value > 1e-9 {
            positive
        } else if value < -1e-9 {
            negative
        } else {
            return "0°".into();
        };
        format!("{:.*}°{hemisphere}", decimals, value.abs())
    }

    /// A longitude counted on past ±180° brought back into range.
    pub fn wrap_lon(lon: f64) -> f64 {
        (lon + 180.).rem_euclid(360.) - 180.
    }

    /// The scale bar at latitude `lat` and `zoom`: its length in pixels, at
    /// most `longest`, for a round distance, and that distance in words.
    pub fn scale_bar(lat: f64, zoom: f64, longest: f64) -> (f64, String) {
        let per_pixel =
            lat.to_radians().cos().abs().max(1e-9) * 2. * std::f64::consts::PI * EARTH_RADIUS
                / scale(zoom);
        let most = longest * per_pixel;
        let mut unit = 10f64.powf(most.log10().floor());
        for step in [5., 2., 1.] {
            if step * unit <= most {
                unit *= step;
                break;
            }
        }
        let label = if unit >= 1000. {
            format!("{} km", unit / 1000.)
        } else {
            format!("{unit} m")
        };
        (unit / per_pixel, label)
    }

    /// The area `view` shows at `size`: south, west, north, east, in
    /// degrees; the whole of the longitudes when it shows a world or more.
    pub fn area(view: &View, size: (f64, f64)) -> [f64; 4] {
        let (x0, y0) = view.to_world((0., 0.), size);
        let (x1, y1) = view.to_world(size, size);
        let (north, west) = unproject(x0, y0.clamp(0., 1.));
        let (south, east) = unproject(x1, y1.clamp(0., 1.));
        if x1 - x0 >= 1. {
            [south, -180., north, 180.]
        } else {
            [south, wrap_lon(west), north, wrap_lon(east)]
        }
    }

    /// The spacing of a sunflower spiral (item `k` at `SPACING · pitch ·
    /// √(k + ½)`, each turned by the golden angle from the one before) that
    /// keeps axis-aligned squares of side `pitch` apart however many there
    /// are (`spiral_squares_never_overlap`). The half leaves the centre to
    /// the point they share.
    const SPACING: f64 = 0.86;

    /// Where the `k`th square of a spiral of squares of side `pitch` sits,
    /// from the spiral's centre. The spiral grows outwards without moving
    /// the squares already placed.
    pub fn spiral(k: usize, pitch: f64) -> (f64, f64) {
        let golden = std::f64::consts::PI * (3. - 5f64.sqrt());
        let k = k as f64;
        let r = SPACING * pitch * (k + 0.5).sqrt();
        let angle = k * golden;
        (r * angle.cos(), r * angle.sin())
    }

    /// How far from its centre a spiral of `n` squares of side `side`,
    /// `gap` apart, reaches; a single square stands on its point.
    pub fn spiral_reach(n: usize, side: f64, gap: f64) -> f64 {
        let corner = side * std::f64::consts::FRAC_1_SQRT_2;
        if n <= 1 {
            corner
        } else {
            SPACING * (side + gap) * (n as f64 - 0.5).sqrt() + corner
        }
    }

    /// Where an item of `spread` goes.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Spot {
        /// Its square's centre.
        pub at: (f64, f64),
        pub side: f64,
        /// Where it really is.
        pub point: (f64, f64),
        /// It shares its spiral: drawn with a line to `point`.
        pub spread: bool,
    }

    /// Items at `points` (pixels), in the order they take along a spiral,
    /// laid out so that none hides another: those whose spirals would
    /// overlap join one, around their mean point, until no two spirals
    /// overlap, and each spiral's squares are `side(n)` for its `n`
    /// items, `gap` apart. An item alone stays on its point. Pure, and in
    /// `O(n log n)` a pass while the spirals are apart.
    pub fn spread(points: &[(f64, f64)], side: impl Fn(usize) -> f64, gap: f64) -> Vec<Spot> {
        fn root(parent: &mut [usize], mut i: usize) -> usize {
            while parent[i] != i {
                parent[i] = parent[parent[i]];
                i = parent[i];
            }
            i
        }
        let mut groups: Vec<Vec<usize>> = (0..points.len()).map(|i| vec![i]).collect();
        let centre = |members: &[usize]| {
            let n = members.len() as f64;
            let (x, y) = members
                .iter()
                .fold((0., 0.), |(x, y), &i| (x + points[i].0, y + points[i].1));
            (x / n, y / n)
        };
        // Each pass joins every two spirals that overlap; one that joins
        // nothing ends. A pass joins at least two, so there are fewer
        // passes than items.
        loop {
            let discs: Vec<(f64, f64, f64)> = groups
                .iter()
                .map(|members| {
                    let (x, y) = centre(members);
                    let n = members.len();
                    (x, y, spiral_reach(n, side(n), gap))
                })
                .collect();
            let mut order: Vec<usize> = (0..discs.len()).collect();
            order.sort_by(|&a, &b| (discs[a].0 - discs[a].2).total_cmp(&(discs[b].0 - discs[b].2)));
            let mut parent: Vec<usize> = (0..discs.len()).collect();
            let mut joined = false;
            let mut active: Vec<usize> = Vec::new();
            for &i in &order {
                let (x, y, r) = discs[i];
                active.retain(|&j| discs[j].0 + discs[j].2 + gap > x - r);
                for &j in &active {
                    let (jx, jy, jr) = discs[j];
                    if (x - jx).hypot(y - jy) < r + jr + gap {
                        let (a, b) = (root(&mut parent, i), root(&mut parent, j));
                        if a != b {
                            parent[a] = b;
                            joined = true;
                        }
                    }
                }
                active.push(i);
            }
            if !joined {
                break;
            }
            let mut merged: HashMap<usize, Vec<usize>> = HashMap::new();
            for (g, members) in groups.into_iter().enumerate() {
                merged
                    .entry(root(&mut parent, g))
                    .or_default()
                    .extend(members);
            }
            groups = merged.into_values().collect();
            for members in &mut groups {
                members.sort_unstable();
            }
        }
        let mut spots = vec![
            Spot {
                at: (0., 0.),
                side: 0.,
                point: (0., 0.),
                spread: false,
            };
            points.len()
        ];
        for members in &groups {
            let n = members.len();
            let side = side(n);
            let (cx, cy) = centre(members);
            for (k, &i) in members.iter().enumerate() {
                let at = if n == 1 {
                    points[i]
                } else {
                    let (dx, dy) = spiral(k, side + gap);
                    (cx + dx, cy + dy)
                };
                spots[i] = Spot {
                    at,
                    side,
                    point: points[i],
                    spread: n > 1,
                };
            }
        }
        spots
    }
}

/// A file's places, projected once, on the worker that received them.
#[derive(Debug, Default)]
pub struct Shapes {
    pub lines: Vec<ShapeLine>,
    /// Polygons: an outer ring, then its holes.
    pub areas: Vec<Vec<Vec<(f64, f64)>>>,
    pub markers: Vec<Marker>,
    /// What all of it covers; `None` when nothing has a position.
    pub bounds: Option<math::Rect>,
}

#[derive(Debug)]
pub struct ShapeLine {
    pub kind: LineKind,
    pub segments: Vec<Vec<(f64, f64)>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkerKind {
    Place,
    Stop,
    Start,
    End,
    /// A picture or a video of the location shown (the Map view), drawn
    /// as its thumbnail.
    Picture,
    /// Where a track of the location shown starts (the Map view).
    Track,
}

#[derive(Debug)]
pub struct Marker {
    pub at: (f64, f64),
    pub label: SharedString,
    pub kind: MarkerKind,
    /// The file it stands for, in the Map view: what a click selects.
    pub entry: Option<EntryId>,
    /// A picture's camera clock: markers spread around one place follow
    /// it (`bibli_platform::exif::Taken::clock`).
    pub clock: Option<u64>,
}

/// A file the Map view places: where, its name, and a track's line.
pub struct Place {
    pub entry: EntryId,
    pub at: LatLon,
    pub name: SharedString,
    pub line: Vec<Vec<LatLon>>,
    pub clock: Option<u64>,
}

/// `x` moved by whole worlds to within half a world of `near`.
fn beside(x: f64, near: f64) -> f64 {
    x + (near - x).round()
}

impl Shapes {
    pub fn new(doc: &GeoDoc) -> Shapes {
        let Some(bounds) = doc.bounds else {
            return Shapes::default();
        };
        let center = math::project(bounds.center()).0;
        let mut rect: Option<math::Rect> = None;
        let mut grow = |(x, y): (f64, f64)| {
            let r = rect.get_or_insert(math::Rect {
                x0: x,
                y0: y,
                x1: x,
                y1: y,
            });
            r.x0 = r.x0.min(x);
            r.x1 = r.x1.max(x);
            r.y0 = r.y0.min(y);
            r.y1 = r.y1.max(y);
        };
        // A line stays continuous: each point within half a world of the
        // one before, the first near the centre.
        let mut path = |points: &mut dyn Iterator<Item = LatLon>| -> Vec<(f64, f64)> {
            let mut last = center;
            points
                .map(|at| {
                    let (x, y) = math::project(at);
                    let x = beside(x, last);
                    last = x;
                    grow((x, y));
                    (x, y)
                })
                .collect()
        };
        let lines: Vec<ShapeLine> = doc
            .lines
            .iter()
            .map(|line| ShapeLine {
                kind: line.kind,
                segments: line
                    .segments
                    .iter()
                    .map(|segment| path(&mut segment.iter().map(|p| p.at)))
                    .collect(),
            })
            .collect();
        let areas = doc
            .areas
            .iter()
            .map(|area| {
                area.rings
                    .iter()
                    .map(|ring| path(&mut ring.iter().copied()))
                    .collect()
            })
            .collect();
        let mut markers: Vec<Marker> = doc
            .waypoints
            .iter()
            .map(|w| {
                let (x, y) = math::project(w.at);
                let at = (beside(x, center), y);
                grow(at);
                Marker {
                    at,
                    label: w.name.clone().into(),
                    kind: if w.kind == WaypointKind::Stop {
                        MarkerKind::Stop
                    } else {
                        MarkerKind::Place
                    },
                    entry: None,
                    clock: None,
                }
            })
            .collect();
        // Where the first track starts and the last one ends.
        let tracks = || lines.iter().filter(|l| l.kind == LineKind::Track);
        if let Some(&start) = tracks().flat_map(|l| &l.segments).flatten().next() {
            markers.push(Marker {
                at: start,
                label: SharedString::default(),
                kind: MarkerKind::Start,
                entry: None,
                clock: None,
            });
        }
        if let Some(&end) = tracks()
            .flat_map(|l| &l.segments)
            .filter_map(|s| s.last())
            .next_back()
        {
            markers.push(Marker {
                at: end,
                label: SharedString::default(),
                kind: MarkerKind::End,
                entry: None,
                clock: None,
            });
        }
        Shapes {
            lines,
            areas,
            markers,
            bounds: rect,
        }
    }

    /// The files the Map view places: a marker each, at a picture's
    /// position or where a track starts, and the tracks' lines.
    pub fn places(places: &[Place]) -> Shapes {
        let mut bounds = BoundsBuilder::default();
        for place in places {
            bounds.add(place.at);
        }
        let Some(center) = bounds.finish().map(|b| math::project(b.center()).0) else {
            return Shapes::default();
        };
        let mut rect: Option<math::Rect> = None;
        let mut grow = |(x, y): (f64, f64)| {
            let r = rect.get_or_insert(math::Rect {
                x0: x,
                y0: y,
                x1: x,
                y1: y,
            });
            r.x0 = r.x0.min(x);
            r.x1 = r.x1.max(x);
            r.y0 = r.y0.min(y);
            r.y1 = r.y1.max(y);
        };
        let mut lines = Vec::new();
        let mut markers = Vec::with_capacity(places.len());
        for place in places {
            let (x, y) = math::project(place.at);
            let at = (beside(x, center), y);
            grow(at);
            let track = !place.line.is_empty();
            markers.push(Marker {
                at,
                label: place.name.clone(),
                kind: if track {
                    MarkerKind::Track
                } else {
                    MarkerKind::Picture
                },
                entry: Some(place.entry),
                clock: place.clock,
            });
            if track {
                let segments = place
                    .line
                    .iter()
                    .map(|segment| {
                        let mut last = at.0;
                        segment
                            .iter()
                            .map(|p| {
                                let (x, y) = math::project(*p);
                                let x = beside(x, last);
                                last = x;
                                (x, y)
                            })
                            .collect()
                    })
                    .collect();
                lines.push(ShapeLine {
                    kind: LineKind::Track,
                    segments,
                });
            }
        }
        Shapes {
            lines,
            areas: Vec::new(),
            markers,
            bounds: rect,
        }
    }
}

/// What a click on the map picked (the Map view).
#[derive(Clone, Debug, PartialEq)]
pub enum Pick {
    Entry(EntryId),
    /// Markers merged into one: its files, and what they cover.
    Cluster(Vec<EntryId>, math::Rect),
    /// The files a Shift+drag rectangle held.
    Band(Vec<EntryId>),
    Nothing,
}

type PickFn = Rc<dyn Fn(Pick, Modifiers, usize, &mut Window, &mut App)>;
type ThumbFn = Rc<dyn Fn(EntryId, &mut Window, &mut App) -> Option<Arc<gpui::RenderImage>>>;

/// How the Map view's markers answer: what is selected, what a click or a
/// rectangle picks (with its modifiers and click count), and the pictures
/// markers standing alone show.
pub struct Picking {
    pub selected: Arc<HashSet<EntryId>>,
    pub on_pick: PickFn,
    pub thumb: ThumbFn,
}

/// Markers closer than this on screen merge into one: more than a
/// picture marker's side, so pictures standing alone never overlap.
const CLUSTER: f32 = 48.;
/// Spread markers (`Map::spread`, or a bubble opened in place): a spiral
/// of a few shows its pictures at `THUMB`, a larger one smaller, down to
/// `SPREAD_LEAST`; past `SPREAD_DOTS` in a spiral they are dots, and
/// past `SPREAD_THUMBS` pictures in view the rest are dots in their places.
const SPREAD_LEAST: f32 = 20.;
const SPREAD_FULL_UP_TO: f32 = 8.;
const SPREAD_DOTS: usize = 120;
const SPREAD_THUMBS: usize = 200;
const SPREAD_DOT: f32 = 12.;
const SPREAD_GAP: f32 = 4.;
/// Markers this far outside the map are spread too, so a spiral that
/// reaches into view is whole and panning does not regroup what shows.
const SPREAD_MARGIN: f32 = 400.;
/// Past this many markers in reach, `Map::spread` gives way to bubbles:
/// the spirals would cover the map.
const SPREAD_MOST: usize = 3000;
/// Markers standing alone show their picture while at most this many do.
const THUMBS: usize = 60;
const THUMB: f32 = 40.;
/// A press that moved less than this is a click, not a drag.
const CLICK_SLOP: f32 = 4.;

/// The colours a map is drawn in, from the theme's tokens.
#[derive(Clone, Copy)]
pub struct Colors {
    pub paper: Hsla,
    pub grid: Hsla,
    pub grid_text: Hsla,
    pub track: Hsla,
    pub route: Hsla,
    pub text: Hsla,
}

/// State kept between frames by whoever shows the map.
#[derive(Clone, Default)]
pub struct Handles {
    /// The view the user chose by panning or zooming; `None` fits the map
    /// to what it shows, at whatever size it is drawn.
    pub view: Rc<Cell<Option<View>>>,
    /// The view drawn last, for a key to zoom from.
    pub shown: Rc<Cell<Option<View>>>,
    /// The area drawn last, south, west, north, east in degrees (west
    /// past east across ±180°): read by an action on what is shown
    /// (search this area), never for layout.
    pub shown_area: Rc<Cell<Option<[f64; 4]>>>,
    /// Where a drag was last seen.
    drag: Rc<Cell<Option<gpui::Point<Pixels>>>>,
    /// Where the press was and how many clicks it counted: a release near
    /// it is a click.
    press: Rc<Cell<Option<(gpui::Point<Pixels>, usize)>>>,
    /// A Shift+drag rectangle: where it started, where it is.
    band: Rc<Cell<Option<(gpui::Point<Pixels>, gpui::Point<Pixels>)>>>,
    /// The files of a bubble opened in place: markers zooming could not
    /// pull apart, spread around where they are until a click beside them.
    opened: Rc<RefCell<HashSet<EntryId>>>,
    /// A place to mark: what the pointer is on in the elevation profile.
    pub mark: Rc<Cell<Option<LatLon>>>,
    /// The profile's point under the pointer.
    pub profile_hover: Rc<Cell<Option<usize>>>,
}

impl Handles {
    /// Fold a bubble opened in place; whether one was.
    pub fn fold(&self) -> bool {
        let mut opened = self.opened.borrow_mut();
        let was = !opened.is_empty();
        opened.clear();
        was
    }
}

pub struct Map {
    id: ElementId,
    shapes: Arc<Shapes>,
    handles: Handles,
    colors: Colors,
    tiles: Option<TileLayer>,
    picking: Option<Picking>,
    spread: bool,
}

impl Map {
    /// Markers stand for files that a click selects (the Map view).
    #[must_use]
    pub fn picking(mut self, picking: Picking) -> Self {
        self.picking = Some(picking);
        self
    }

    /// Files' markers never merge into bubbles: those at one place are
    /// spread on a spiral around it, in the order they were taken, each
    /// with a line to where it is (`math::spread`).
    #[must_use]
    pub fn spread(mut self, spread: bool) -> Self {
        self.spread = spread;
        self
    }
}

/// What a map is drawn on: the tiles of the registered source.
pub struct TileLayer {
    pub store: Rc<RefCell<crate::map_tiles::TileStore>>,
    pub palette: bibli_core::tiles::Palette,
    /// Follow the attribution's link.
    pub on_link: Rc<dyn Fn(String, &mut Window, &mut App)>,
}

pub fn map(
    id: impl Into<ElementId>,
    shapes: Arc<Shapes>,
    handles: Handles,
    colors: Colors,
    tiles: Option<TileLayer>,
) -> Map {
    Map {
        id: id.into(),
        shapes,
        handles,
        colors,
        tiles,
        picking: None,
        spread: false,
    }
}

/// A tile to draw: where, and the part of it that shows (a coarser tile
/// standing in for a finer one shows only that one's square).
struct TileDraw {
    image: Arc<gpui::RenderImage>,
    bounds: Bounds<Pixels>,
    clip: Bounds<Pixels>,
}

/// Where a tile of zoom `z` drawn at `(dx, dy)` (whole tiles from the
/// world's corner, x continuous) lands on screen, its edges on whole
/// pixels so neighbours meet without a seam.
fn tile_bounds(
    view: &View,
    z: u8,
    (dx, dy): (i64, i64),
    size: (f64, f64),
    origin: gpui::Point<Pixels>,
) -> Bounds<Pixels> {
    let n = f64::from(1u32 << z);
    let corner = |tx: i64, ty: i64| {
        let (x, y) = view.to_screen((tx as f64 / n, ty as f64 / n), size);
        (x.round() as f32, y.round() as f32)
    };
    let (x0, y0) = corner(dx, dy);
    let (x1, y1) = corner(dx + 1, dy + 1);
    Bounds::new(
        origin + point(px(x0), px(y0)),
        gpui::size(px(x1 - x0), px(y1 - y0)),
    )
}

/// How many coarser zooms stand in for a tile on its way.
const STAND_IN: u8 = 4;

impl IntoElement for Map {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

struct Label {
    origin: gpui::Point<Pixels>,
    line: ShapedLine,
    /// Drawn on a card, so it reads over lines.
    card: bool,
}

struct Dot {
    at: gpui::Point<Pixels>,
    kind: MarkerKind,
    selected: bool,
    /// A marker standing alone for a picture, drawn as the picture.
    picture: Option<Arc<gpui::RenderImage>>,
    /// The picture's side.
    side: f32,
}

/// Markers merged into one bubble.
struct Cluster {
    at: gpui::Point<Pixels>,
    radius: f32,
    count: ShapedLine,
    selected: bool,
}

/// A file's marker in the Map view: the file, where it is on screen,
/// what it is, its name and its camera clock.
type File = (
    EntryId,
    gpui::Point<Pixels>,
    MarkerKind,
    SharedString,
    Option<u64>,
);

/// What a point on screen picks, topmost last.
struct Hit {
    at: gpui::Point<Pixels>,
    radius: f32,
    pick: Pick,
}

pub struct Prepaint {
    hitbox: Hitbox,
    view: View,
    tiles: Vec<TileDraw>,
    /// The source's attribution, where it is drawn, and its link with the
    /// hitbox that takes a click on it.
    attribution: Option<(ShapedLine, Bounds<Pixels>, Option<(String, Hitbox)>)>,
    grid: Vec<(gpui::Point<Pixels>, gpui::Point<Pixels>)>,
    labels: Vec<Label>,
    /// Runs of screen points, by what they draw.
    areas: Vec<Vec<Vec<gpui::Point<Pixels>>>>,
    routes: Vec<Vec<gpui::Point<Pixels>>>,
    tracks: Vec<Vec<gpui::Point<Pixels>>>,
    dots: Vec<Dot>,
    /// Spread markers' lines to where they are, and those places.
    leaders: Vec<(gpui::Point<Pixels>, gpui::Point<Pixels>)>,
    anchors: Vec<gpui::Point<Pixels>>,
    clusters: Vec<Cluster>,
    hits: Vec<Hit>,
    /// Where each file's marker is, for a rectangle to hold.
    placed: Vec<(EntryId, gpui::Point<Pixels>)>,
    /// `Handles::mark`, on screen.
    mark: Option<gpui::Point<Pixels>>,
    scale: (f64, Option<ShapedLine>),
}

const LABEL_SIZE: f32 = 11.;
const LINE_HEIGHT: f32 = 14.;

pub(crate) fn shape(text: SharedString, color: Hsla, window: &mut Window) -> ShapedLine {
    let run = TextRun {
        len: text.len(),
        font: window.text_style().font(),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
        position: gpui::RunPosition::Baseline,
    };
    window
        .text_system()
        .shape_line(text, px(LABEL_SIZE), &[run], None)
}

/// A line of world points as runs of screen points: thinned to what the
/// pixels can show, and cut where it leaves the element (with a margin, so
/// a thick line's end is not clipped square).
fn runs(
    points: &[(f64, f64)],
    view: &View,
    offset: f64,
    size: (f64, f64),
    origin: gpui::Point<Pixels>,
    out: &mut Vec<Vec<gpui::Point<Pixels>>>,
) {
    const MARGIN: f64 = 16.;
    let code = |(x, y): (f64, f64)| {
        u8::from(x < -MARGIN)
            | u8::from(x > size.0 + MARGIN) << 1
            | u8::from(y < -MARGIN) << 2
            | u8::from(y > size.1 + MARGIN) << 3
    };
    let to_px = |(x, y): (f64, f64)| origin + point(px(x as f32), px(y as f32));
    let mut run: Vec<gpui::Point<Pixels>> = Vec::new();
    let mut last: Option<(f64, f64)> = None;
    let mut kept: Option<(f64, f64)> = None;
    for &(x, y) in points {
        let here = view.to_screen((x + offset, y), size);
        if let Some(before) = last {
            if code(before) & code(here) != 0 {
                // Both ends beyond the same edge: nothing of it shows.
                if run.len() > 1 {
                    out.push(std::mem::take(&mut run));
                }
                run.clear();
                kept = None;
            } else {
                if run.is_empty() {
                    run.push(to_px(before));
                    kept = Some(before);
                }
                let far = kept.is_none_or(|k| (k.0 - here.0).hypot(k.1 - here.1) >= 1.);
                if far {
                    run.push(to_px(here));
                    kept = Some(here);
                }
            }
        }
        last = Some(here);
    }
    // A line ends where it ended, however close to the last point kept.
    if let (Some(end), Some(k)) = (last, kept)
        && end != k
        && !run.is_empty()
    {
        run.push(to_px(end));
    }
    if run.len() > 1 {
        out.push(run);
    }
}

impl Element for Map {
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
        cx: &mut App,
    ) -> Prepaint {
        let size = (
            f64::from(f32::from(bounds.size.width)),
            f64::from(f32::from(bounds.size.height)),
        );
        let origin = bounds.origin;
        // Fitted to the bounds it really has, until the user moves it.
        let view = self
            .handles
            .view
            .get()
            .unwrap_or_else(|| {
                self.shapes
                    .bounds
                    .map_or_else(|| View::world(size), |rect| math::fit(rect, size))
            })
            .clamp(size.1);
        self.handles.shown.set(Some(view));
        self.handles.shown_area.set(Some(math::area(&view, size)));
        let offset = self
            .shapes
            .bounds
            .map_or(0., |rect| math::copy_offset(view.x, rect.center().0));
        let at = |world: (f64, f64)| {
            let (x, y) = view.to_screen(world, size);
            origin + point(px(x as f32), px(y as f32))
        };

        // The background's tiles, a coarser one standing in for each that
        // is not here yet.
        let mut tiles = Vec::new();
        let mut attribution = None;
        let mut detailed = false;
        if let Some(layer) = &self.tiles {
            let mut store = layer.store.borrow_mut();
            if let Some(info) = store.info() {
                detailed = info.detailed;
                store.begin_frame();
                let z = (view.zoom.round() as u8).min(info.max_zoom);
                let scale = window.scale_factor().round().clamp(1., 2.) as u8;
                let request = |z: u8, x: u32, y: u32| bibli_core::tiles::TileRequest {
                    z,
                    x,
                    y,
                    scale,
                    palette: layer.palette,
                };
                let corners = (view.to_world((0., 0.), size), view.to_world(size, size));
                let mut cover = bibli_core::tiles::cover(z, corners.0, corners.1);
                // The farthest from the middle asked first: a source serves
                // its newest request first, so the middle arrives first.
                let n = f64::from(1u32 << z);
                let away = |&(_, _, dx, dy): &(u32, u32, i64, i64)| {
                    let (cx, cy) = ((dx as f64 + 0.5) / n, (dy as f64 + 0.5) / n);
                    (cx - view.x).hypot(cy - view.y)
                };
                cover.sort_by(|a, b| away(b).total_cmp(&away(a)));
                if let Some(&(x, y, _, _)) = cover.last() {
                    store.middle(request(z, x, y));
                }
                for (x, y, dx, dy) in cover {
                    let bounds = tile_bounds(&view, z, (dx, dy), size, origin);
                    if let Some(image) = store.get(request(z, x, y)) {
                        tiles.push(TileDraw {
                            image,
                            bounds,
                            clip: bounds,
                        });
                        continue;
                    }
                    // Zooming out: the four finer tiles already here stand
                    // in, over any coarser one found below.
                    if z < info.max_zoom {
                        for (i, j) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                            let child = request(z + 1, 2 * x + i, 2 * y + j);
                            if let Some(image) = store.cached(child) {
                                let at = (2 * dx + i64::from(i), 2 * dy + i64::from(j));
                                let bounds = tile_bounds(&view, z + 1, at, size, origin);
                                tiles.push(TileDraw {
                                    image,
                                    bounds,
                                    clip: bounds,
                                });
                            }
                        }
                    }
                    for up in 1..=STAND_IN.min(z) {
                        let (pz, step) = (z - up, 1i64 << up);
                        if let Some(image) = store.cached(request(pz, x >> up, y >> up)) {
                            tiles.push(TileDraw {
                                image,
                                bounds: tile_bounds(
                                    &view,
                                    pz,
                                    (dx.div_euclid(step), dy.div_euclid(step)),
                                    size,
                                    origin,
                                ),
                                clip: bounds,
                            });
                            break;
                        }
                    }
                }
                store.end_frame();
                // Coarser stand-ins are drawn first, under the tiles they
                // wait for, and finer ones last, over them.
                tiles.sort_by_key(|t| std::cmp::Reverse(t.bounds.size.width));
                if !info.attribution.is_empty() {
                    let line = shape(info.attribution.into(), self.colors.text, window);
                    let width = line.width + px(8.);
                    let at = bounds.origin
                        + point(
                            bounds.size.width - width - px(4.),
                            bounds.size.height - px(LINE_HEIGHT + 6.),
                        );
                    attribution = Some((
                        line,
                        Bounds::new(at, gpui::size(width, px(LINE_HEIGHT + 2.))),
                        info.attribution_url,
                    ));
                }
            }
        }

        // The graticule, labelled along the top and the left edges, over
        // a background without streets.
        let step = math::graticule_step(view.zoom, 90.);
        let (north_west, south_east) = (view.to_world((0., 0.), size), view.to_world(size, size));
        let (top, left) = math::unproject(north_west.0, north_west.1);
        let (bottom, right) = math::unproject(south_east.0, south_east.1);
        let mut grid = Vec::new();
        let mut labels = Vec::new();
        let edge = |x: f64, y: f64| origin + point(px(x as f32), px(y as f32));
        let mut lon = (left / step).ceil() * step;
        while !detailed && lon <= right && grid.len() < 200 {
            let (x, _) = view.to_screen(((lon + 180.) / 360., view.y), size);
            grid.push((edge(x, 0.), edge(x, size.1)));
            let text = math::degrees(math::wrap_lon(lon), step, 'E', 'W');
            labels.push(Label {
                origin: edge(x + 3., 2.),
                line: shape(text.into(), self.colors.grid_text, window),
                card: false,
            });
            lon += step;
        }
        let mut lat = (bottom.max(-math::MAX_LAT) / step).ceil() * step;
        while !detailed && lat <= top.min(math::MAX_LAT) && grid.len() < 400 {
            let (_, y) = view.to_screen(math::project(LatLon { lat, lon: 0. }), size);
            grid.push((edge(0., y), edge(size.0, y)));
            let text = math::degrees(lat, step, 'N', 'S');
            labels.push(Label {
                origin: edge(3., y - f64::from(LINE_HEIGHT)),
                line: shape(text.into(), self.colors.grid_text, window),
                card: false,
            });
            lat += step;
        }

        let mut areas = Vec::new();
        for area in &self.shapes.areas {
            let mut rings = Vec::new();
            for ring in area {
                // A polygon is filled whole: only thinned, never cut.
                let mut drawn: Vec<gpui::Point<Pixels>> = Vec::with_capacity(ring.len());
                for &(x, y) in ring {
                    let p = at((x + offset, y));
                    if drawn.last().is_none_or(|l| {
                        f32::from((l.x - p.x).abs()) + f32::from((l.y - p.y).abs()) >= 1.
                    }) {
                        drawn.push(p);
                    }
                }
                if drawn.len() > 2 {
                    rings.push(drawn);
                }
            }
            if !rings.is_empty() {
                areas.push(rings);
            }
        }
        let (mut routes, mut tracks) = (Vec::new(), Vec::new());
        for line in &self.shapes.lines {
            let out = match line.kind {
                LineKind::Track => &mut tracks,
                LineKind::Route => &mut routes,
            };
            for segment in &line.segments {
                runs(segment, &view, offset, size, origin, out);
            }
        }

        let inside = |p: gpui::Point<Pixels>| bounds.contains(&p);
        let reach = bounds.dilate(px(SPREAD_MARGIN));
        let mut dots = Vec::new();
        let mut named = Vec::new();
        // Files' markers (the Map view): the file, where, what, its name
        // and its clock. Those near enough to be spread into view too.
        let mut files: Vec<File> = Vec::new();
        for marker in &self.shapes.markers {
            let p = at((marker.at.0 + offset, marker.at.1));
            if let (Some(entry), Some(_)) = (marker.entry, &self.picking) {
                if reach.contains(&p) {
                    files.push((entry, p, marker.kind, marker.label.clone(), marker.clock));
                }
                continue;
            }
            if !inside(p) {
                continue;
            }
            dots.push(Dot {
                at: p,
                kind: marker.kind,
                selected: false,
                picture: None,
                side: THUMB,
            });
            if !marker.label.is_empty() {
                named.push((p, marker.label.clone()));
            }
        }
        // Spread: every file with `Map::spread` while they are not so many
        // that spirals would cover the map, else a bubble opened in place.
        let spread_all = self.spread && files.len() <= SPREAD_MOST;
        let (mut spread, clustered): (Vec<File>, Vec<File>) = {
            let opened = self.handles.opened.borrow();
            files
                .into_iter()
                .partition(|file| spread_all || opened.contains(&file.0))
        };
        // The others close together merge, as map libraries do: each joins
        // the group whose first marker is within `CLUSTER` pixels, looked
        // for in its cell and the eight around it, or starts one.
        // Recomputed as the view moves.
        let mut groups: Vec<Vec<File>> = Vec::new();
        let mut seeds: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        for file in clustered.into_iter().filter(|file| inside(file.1)) {
            let local = file.1 - origin;
            let cell = (
                (f32::from(local.x) / CLUSTER).floor() as i32,
                (f32::from(local.y) / CLUSTER).floor() as i32,
            );
            let near = (-1..=1)
                .flat_map(|dx| (-1..=1).map(move |dy| (cell.0 + dx, cell.1 + dy)))
                .filter_map(|c| seeds.get(&c))
                .flatten()
                .copied()
                .find(|&g| {
                    let d = groups[g][0].1 - file.1;
                    f32::from(d.x).hypot(f32::from(d.y)) < CLUSTER
                });
            if let Some(g) = near {
                groups[g].push(file);
            } else {
                seeds.entry(cell).or_default().push(groups.len());
                groups.push(vec![file]);
            }
        }
        let mut clusters = Vec::new();
        let mut hits = Vec::new();
        let mut placed = Vec::new();
        let (mut leaders, mut anchors) = (Vec::new(), Vec::new());
        if let Some(picking) = &self.picking {
            let alone = groups.iter().filter(|members| members.len() == 1).count();
            let mut cells = groups;
            // Drawn top to bottom, so a lower marker overlaps a higher one.
            cells.sort_by(|a, b| {
                (f32::from(a[0].1.y), f32::from(a[0].1.x))
                    .partial_cmp(&(f32::from(b[0].1.y), f32::from(b[0].1.x)))
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            for members in cells {
                placed.extend(members.iter().map(|(entry, p, ..)| (*entry, *p)));
                let selected = members
                    .iter()
                    .any(|(entry, ..)| picking.selected.contains(entry));
                if let [(entry, p, kind, label, _)] = members.as_slice() {
                    let picture = (*kind == MarkerKind::Picture && alone <= THUMBS)
                        .then(|| (picking.thumb)(*entry, window, cx))
                        .flatten();
                    let radius = if picture.is_some() { THUMB / 2. } else { 7. };
                    dots.push(Dot {
                        at: *p,
                        kind: *kind,
                        selected,
                        picture,
                        side: THUMB,
                    });
                    hits.push(Hit {
                        at: *p,
                        radius,
                        pick: Pick::Entry(*entry),
                    });
                    if selected && !label.is_empty() {
                        named.push((*p + point(px(radius - 6.), px(0.)), label.clone()));
                    }
                    continue;
                }
                let n = members.len() as f32;
                let (sx, sy) = members.iter().fold((0f32, 0f32), |(x, y), (_, p, ..)| {
                    (x + f32::from(p.x), y + f32::from(p.y))
                });
                let centre = point(px(sx / n), px(sy / n));
                let text: SharedString = if members.len() > 999 {
                    format!("{}k", members.len() / 1000).into()
                } else {
                    members.len().to_string().into()
                };
                let radius = 11. + 2. * (text.len() as f32);
                let mut rect: Option<math::Rect> = None;
                for (_, p, ..) in &members {
                    let (x, y) = view.to_world(
                        (
                            f64::from(f32::from(p.x - origin.x)),
                            f64::from(f32::from(p.y - origin.y)),
                        ),
                        size,
                    );
                    let r = rect.get_or_insert(math::Rect {
                        x0: x,
                        y0: y,
                        x1: x,
                        y1: y,
                    });
                    r.x0 = r.x0.min(x);
                    r.x1 = r.x1.max(x);
                    r.y0 = r.y0.min(y);
                    r.y1 = r.y1.max(y);
                }
                hits.push(Hit {
                    at: centre,
                    radius,
                    pick: Pick::Cluster(
                        members.iter().map(|(entry, ..)| *entry).collect(),
                        rect.unwrap_or(math::Rect {
                            x0: 0.,
                            y0: 0.,
                            x1: 0.,
                            y1: 0.,
                        }),
                    ),
                });
                clusters.push(Cluster {
                    at: centre,
                    radius,
                    count: shape(text, self.colors.paper, window),
                    selected,
                });
            }
            if !spread.is_empty() {
                // Along each spiral in the order they were taken, the
                // undated after, then by name.
                spread.sort_by(|a, b| (a.4.is_none(), a.4, &a.3).cmp(&(b.4.is_none(), b.4, &b.3)));
                let square_side = |n: usize| {
                    f64::from(if n > SPREAD_DOTS {
                        SPREAD_DOT
                    } else {
                        (THUMB * (SPREAD_FULL_UP_TO / n as f32).sqrt()).clamp(SPREAD_LEAST, THUMB)
                    })
                };
                let points: Vec<(f64, f64)> = spread
                    .iter()
                    .map(|file| {
                        let local = file.1 - origin;
                        (f64::from(f32::from(local.x)), f64::from(f32::from(local.y)))
                    })
                    .collect();
                let spots = math::spread(&points, square_side, f64::from(SPREAD_GAP));
                let mut anchored = HashSet::new();
                let mut pictures = 0;
                for ((entry, p, kind, label, _), spot) in spread.iter().zip(&spots) {
                    let at = origin + point(px(spot.at.0 as f32), px(spot.at.1 as f32));
                    let square = spot.side as f32;
                    if spot.spread {
                        leaders.push((at, *p));
                        if anchored.insert((f32::from(p.x).to_bits(), f32::from(p.y).to_bits())) {
                            anchors.push(*p);
                        }
                    }
                    let selected = picking.selected.contains(entry);
                    // Pictures in view ask for theirs, as many as the
                    // budget allows; the others are dots in their places.
                    let picture = (*kind == MarkerKind::Picture
                        && square >= SPREAD_LEAST
                        && pictures < SPREAD_THUMBS
                        && bounds.dilate(px(square)).contains(&at))
                    .then(|| (picking.thumb)(*entry, window, cx))
                    .flatten();
                    pictures += usize::from(picture.is_some());
                    let radius = if picture.is_some() { square / 2. } else { 7. };
                    dots.push(Dot {
                        at,
                        kind: *kind,
                        selected,
                        picture,
                        side: square,
                    });
                    hits.push(Hit {
                        at,
                        radius,
                        pick: Pick::Entry(*entry),
                    });
                    placed.push((*entry, at));
                    if selected && !label.is_empty() {
                        named.push((at + point(px(radius - 6.), px(0.)), label.clone()));
                    }
                }
            }
        }
        if named.len() <= LABELS {
            for (p, label) in named {
                labels.push(Label {
                    origin: p + point(px(8.), px(-LINE_HEIGHT / 2.)),
                    line: shape(label, self.colors.text, window),
                    card: true,
                });
            }
        }

        let mark = self.handles.mark.get().map(|place| {
            let (x, y) = math::project(place);
            at((beside(x, view.x), y))
        });
        let (lat, _) = math::unproject(view.x, view.y);
        let (length, words) = math::scale_bar(lat, view.zoom, 100.);
        let scale = (length, Some(shape(words.into(), self.colors.text, window)));

        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        // Over the map's own hitbox, so a click there follows the link and
        // does not start a drag.
        let attribution = attribution.map(
            |(line, card, url): (ShapedLine, Bounds<Pixels>, Option<String>)| {
                let link =
                    url.map(|url| (url, window.insert_hitbox(card, HitboxBehavior::BlockMouse)));
                (line, card, link)
            },
        );
        Prepaint {
            hitbox,
            view,
            tiles,
            attribution,
            grid,
            labels,
            areas,
            routes,
            tracks,
            dots,
            leaders,
            anchors,
            clusters,
            hits,
            placed,
            mark,
            scale,
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
        let colors = self.colors;
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            window.paint_quad(gpui::fill(bounds, colors.paper));
            for tile in &prepaint.tiles {
                window.with_content_mask(Some(ContentMask { bounds: tile.clip }), |window| {
                    if let Err(error) = window.paint_image(
                        tile.bounds,
                        gpui::Corners::default(),
                        Arc::clone(&tile.image),
                        0,
                        false,
                    ) {
                        tracing::debug!(%error, "map tile");
                    }
                });
            }
            for (from, to) in &prepaint.grid {
                let line = if from.x == to.x {
                    Bounds::new(*from, size(px(1.), to.y - from.y))
                } else {
                    Bounds::new(*from, size(to.x - from.x, px(1.)))
                };
                window.paint_quad(gpui::fill(line, colors.grid));
            }
            for rings in &prepaint.areas {
                let mut fill = PathBuilder::fill().with_style(PathStyle::Fill(
                    FillOptions::default().with_fill_rule(FillRule::EvenOdd),
                ));
                for ring in rings {
                    fill.move_to(ring[0]);
                    for p in &ring[1..] {
                        fill.line_to(*p);
                    }
                    fill.close();
                }
                if let Ok(path) = fill.build() {
                    window.paint_path(path, colors.route.opacity(0.15));
                }
                for ring in rings {
                    stroke(window, ring, true, 1.5, colors.route);
                }
            }
            // Under each line a band of paper, so it reads over the grid
            // and over the lines it crosses.
            for (lines, color) in [
                (&prepaint.routes, colors.route),
                (&prepaint.tracks, colors.track),
            ] {
                for run in lines {
                    stroke(window, run, false, 6., colors.paper);
                }
                for run in lines {
                    stroke(window, run, false, 3., color);
                }
            }
            // Spread markers' lines to where they are, in one path, and
            // those places, under the markers.
            if !prepaint.leaders.is_empty() {
                let mut path = PathBuilder::stroke(px(1.));
                for (from, to) in &prepaint.leaders {
                    path.move_to(*from);
                    path.line_to(*to);
                }
                if let Ok(path) = path.build() {
                    window.paint_path(path, colors.track.opacity(0.6));
                }
            }
            for anchor in &prepaint.anchors {
                window.paint_quad(quad(
                    Bounds::new(*anchor - point(px(3.), px(3.)), size(px(6.), px(6.))),
                    px(3.),
                    colors.track,
                    px(1.),
                    colors.paper,
                    BorderStyle::Solid,
                ));
            }
            for cluster in &prepaint.clusters {
                let r = cluster.radius;
                window.paint_quad(quad(
                    Bounds::new(
                        cluster.at - point(px(r), px(r)),
                        size(px(2. * r), px(2. * r)),
                    ),
                    px(r),
                    colors.track,
                    px(if cluster.selected { 3. } else { 2. }),
                    if cluster.selected {
                        colors.text
                    } else {
                        colors.paper
                    },
                    BorderStyle::Solid,
                ));
                let text = cluster.at - point(cluster.count.width / 2., px(LINE_HEIGHT / 2.));
                let _ = cluster.count.paint(text, px(LINE_HEIGHT), window, cx);
            }
            for dot in &prepaint.dots {
                if let Some(picture) = &dot.picture {
                    let half = dot.side / 2.;
                    let frame = Bounds::new(
                        dot.at - point(px(half), px(half)),
                        size(px(dot.side), px(dot.side)),
                    );
                    window.paint_quad(quad(
                        frame,
                        px((dot.side * 0.15).min(6.)),
                        colors.paper,
                        px(if dot.selected { 3. } else { 2. }),
                        if dot.selected {
                            colors.track
                        } else {
                            colors.paper
                        },
                        BorderStyle::Solid,
                    ));
                    let side = dot.side - 6.;
                    let inner = Bounds::new(
                        frame.origin + point(px(3.), px(3.)),
                        size(px(side), px(side)),
                    );
                    // Cropped to fill the square, at its own proportions.
                    let natural = picture.size(0);
                    let aspect = natural.width.0.max(1) as f32 / natural.height.0.max(1) as f32;
                    let (w, h) = if aspect >= 1. {
                        (side * aspect, side)
                    } else {
                        (side, side / aspect)
                    };
                    let cover = Bounds::new(
                        inner.origin - point(px((w - side) / 2.), px((h - side) / 2.)),
                        size(px(w), px(h)),
                    );
                    window.with_content_mask(Some(ContentMask { bounds: inner }), |window| {
                        let _ = window.paint_image(
                            cover,
                            gpui::Corners::default(),
                            Arc::clone(picture),
                            0,
                            false,
                        );
                    });
                    continue;
                }
                let (radius, fill, ring) = match dot.kind {
                    MarkerKind::Place => (6., colors.route, colors.paper),
                    MarkerKind::Stop => (4., colors.paper, colors.route),
                    MarkerKind::Start => (5., colors.track, colors.paper),
                    MarkerKind::End => (5., colors.paper, colors.track),
                    MarkerKind::Picture | MarkerKind::Track if dot.selected => {
                        (8., colors.track, colors.text)
                    }
                    MarkerKind::Picture => (6., colors.track, colors.paper),
                    MarkerKind::Track => (6., colors.paper, colors.track),
                };
                window.paint_quad(quad(
                    Bounds::new(
                        dot.at - point(px(radius), px(radius)),
                        size(px(radius * 2.), px(radius * 2.)),
                    ),
                    px(radius),
                    fill,
                    px(2.),
                    ring,
                    BorderStyle::Solid,
                ));
            }
            // What the pointer is on in the profile, over the lines.
            if let Some(mark) = prepaint.mark {
                window.paint_quad(quad(
                    Bounds::new(mark - point(px(7.), px(7.)), size(px(14.), px(14.))),
                    px(7.),
                    colors.paper,
                    px(3.),
                    colors.track,
                    BorderStyle::Solid,
                ));
            }
            for label in &prepaint.labels {
                if label.card {
                    let card = Bounds::new(
                        label.origin - point(px(3.), px(0.)),
                        size(label.line.width + px(6.), px(LINE_HEIGHT)),
                    );
                    window.paint_quad(quad(
                        card,
                        px(3.),
                        colors.paper.opacity(0.85),
                        px(0.),
                        gpui::transparent_black(),
                        BorderStyle::default(),
                    ));
                }
                if let Err(error) = label.line.paint(label.origin, px(LINE_HEIGHT), window, cx) {
                    tracing::debug!(%error, "map label");
                }
            }
            // The scale, at the bottom left.
            let (length, words) = &prepaint.scale;
            let foot = bounds.origin + point(px(12.), bounds.size.height - px(14.));
            let bar = Bounds::new(foot, size(px(*length as f32), px(2.)));
            window.paint_quad(gpui::fill(bar, colors.text));
            for x in [foot.x, foot.x + px(*length as f32 - 2.)] {
                window.paint_quad(gpui::fill(
                    Bounds::new(point(x, foot.y - px(4.)), size(px(2.), px(6.))),
                    colors.text,
                ));
            }
            if let Some(words) = words {
                let _ = words.paint(
                    foot - point(px(0.), px(LINE_HEIGHT + 4.)),
                    px(LINE_HEIGHT),
                    window,
                    cx,
                );
            }
            // Whose tiles these are, whenever they are drawn.
            if let Some((line, card, _)) = &prepaint.attribution {
                window.paint_quad(quad(
                    *card,
                    px(3.),
                    colors.paper.opacity(0.85),
                    px(0.),
                    gpui::transparent_black(),
                    BorderStyle::default(),
                ));
                let _ = line.paint(
                    card.origin + point(px(4.), px(1.)),
                    px(LINE_HEIGHT),
                    window,
                    cx,
                );
            }
        });

        let hitbox = prepaint.hitbox.clone();
        let dragging = self.handles.drag.get().is_some();
        window.set_cursor_style(
            if dragging {
                CursorStyle::ClosedHand
            } else {
                CursorStyle::OpenHand
            },
            &hitbox,
        );
        let size = (
            f64::from(f32::from(bounds.size.width)),
            f64::from(f32::from(bounds.size.height)),
        );
        let local = move |p: gpui::Point<Pixels>| {
            (
                f64::from(f32::from(p.x - bounds.origin.x)),
                f64::from(f32::from(p.y - bounds.origin.y)),
            )
        };
        let shown = prepaint.view;

        if let (Some((_, _, Some((url, link)))), Some(layer)) = (&prepaint.attribution, &self.tiles)
        {
            window.set_cursor_style(CursorStyle::PointingHand, link);
            let (url, link, on_link) = (url.clone(), link.clone(), Rc::clone(&layer.on_link));
            window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                if phase == DispatchPhase::Bubble
                    && event.button == MouseButton::Left
                    && link.is_hovered(window)
                {
                    on_link(url.clone(), window, cx);
                }
            });
        }
        // A band being drawn, over everything.
        if let Some((from, to)) = self.handles.band.get() {
            let band = Bounds::from_corners(
                point(from.x.min(to.x), from.y.min(to.y)),
                point(from.x.max(to.x), from.y.max(to.y)),
            );
            window.paint_quad(quad(
                band,
                px(2.),
                self.colors.track.opacity(0.12),
                px(1.),
                self.colors.track,
                BorderStyle::Solid,
            ));
        }
        let hits: Rc<Vec<(gpui::Point<Pixels>, f32, Pick)>> = Rc::new(
            prepaint
                .hits
                .iter()
                .map(|h| (h.at, h.radius, h.pick.clone()))
                .collect(),
        );
        let placed: Rc<Vec<(EntryId, gpui::Point<Pixels>)>> = Rc::new(prepaint.placed.clone());
        let on_pick = self.picking.as_ref().map(|p| Rc::clone(&p.on_pick));
        let (handles, hit) = (self.handles.clone(), hitbox.clone());
        let banding = on_pick.is_some();
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, _| {
            if phase != DispatchPhase::Bubble
                || event.button != MouseButton::Left
                || !hit.is_hovered(window)
            {
                return;
            }
            handles.press.set(Some((event.position, event.click_count)));
            if banding && event.modifiers.shift {
                handles.band.set(Some((event.position, event.position)));
            } else {
                handles.drag.set(Some(event.position));
            }
            window.refresh();
        });
        let handles = self.handles.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, _| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            // A release outside the window is never delivered here.
            if event.pressed_button != Some(MouseButton::Left) {
                if handles.drag.take().is_some() || handles.band.take().is_some() {
                    window.refresh();
                }
                return;
            }
            if let Some((from, _)) = handles.band.get() {
                handles.band.set(Some((from, event.position)));
                window.refresh();
                return;
            }
            let Some(last) = handles.drag.get() else {
                return;
            };
            let view = handles.view.get().unwrap_or(shown);
            let moved = event.position - last;
            handles.view.set(Some(
                view.pan(f64::from(f32::from(moved.x)), f64::from(f32::from(moved.y))),
            ));
            handles.drag.set(Some(event.position));
            window.refresh();
        });
        let handles = self.handles.clone();
        window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
                return;
            }
            let press = handles.press.take();
            if let Some((from, to)) = handles.band.take() {
                let band = Bounds::from_corners(
                    point(from.x.min(to.x), from.y.min(to.y)),
                    point(from.x.max(to.x), from.y.max(to.y)),
                );
                let held: Vec<EntryId> = placed
                    .iter()
                    .filter(|(_, p)| band.contains(p))
                    .map(|(entry, _)| *entry)
                    .collect();
                if let Some(on_pick) = &on_pick {
                    on_pick(Pick::Band(held), event.modifiers, 1, window, cx);
                }
                window.refresh();
                return;
            }
            if handles.drag.take().is_none() {
                return;
            }
            window.refresh();
            let Some((at, clicks)) = press else {
                return;
            };
            let moved = event.position - at;
            if f32::from(moved.x).hypot(f32::from(moved.y)) > CLICK_SLOP {
                return;
            }
            // A click: the topmost marker under it, if any.
            let picked = hits
                .iter()
                .rev()
                .find(|(p, radius, _)| {
                    let d = event.position - *p;
                    f32::from(d.x).hypot(f32::from(d.y)) <= radius + 2.
                })
                .map_or(Pick::Nothing, |(_, _, pick)| pick.clone());
            // A cluster opens where it is: zoomed to its files, as long as
            // zooming can pull them apart.
            // Files at one place, or zoomed in all the way, open in place
            // instead: spread around where they are.
            if let Pick::Cluster(files, rect) = &picked {
                let view = handles.view.get().unwrap_or(shown);
                let wide = rect.x1 - rect.x0 > 1e-12 || rect.y1 - rect.y0 > 1e-12;
                if wide && view.zoom < MAX_ZOOM - 0.5 {
                    let fitted = math::fit(*rect, size);
                    handles.view.set(Some(View {
                        zoom: fitted.zoom.max(view.zoom + 1.),
                        ..fitted
                    }));
                } else {
                    *handles.opened.borrow_mut() = files.iter().copied().collect();
                }
                return;
            }
            // A click beside a bubble opened in place folds it.
            if picked == Pick::Nothing && handles.fold() && clicks == 1 {
                return;
            }
            if picked == Pick::Nothing && clicks == 2 {
                // A double click on the map itself zooms in there.
                let view = handles.view.get().unwrap_or(shown);
                handles
                    .view
                    .set(Some(view.zoom_at(1., local(event.position), size)));
                return;
            }
            if let Some(on_pick) = &on_pick {
                on_pick(picked, event.modifiers, clicks, window, cx);
            }
        });
        let handles = self.handles.clone();
        window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || !hitbox.should_handle_scroll(window) {
                return;
            }
            // A notch of the wheel (three lines, as GPUI reports it on
            // Linux) is half a level; a touchpad zooms as smoothly as it
            // scrolls.
            let levels = match event.delta {
                ScrollDelta::Lines(lines) => f64::from(lines.y) / 6.,
                ScrollDelta::Pixels(pixels) => f64::from(f32::from(pixels.y)) / 240.,
            }
            .clamp(-2., 2.);
            if levels == 0. {
                return;
            }
            let view = handles.view.get().unwrap_or(shown);
            handles
                .view
                .set(Some(view.zoom_at(levels, local(event.position), size)));
            cx.stop_propagation();
            window.refresh();
        });
    }
}

/// A stroke along `points`, closed into a ring or not.
fn stroke(
    window: &mut Window,
    points: &[gpui::Point<Pixels>],
    closed: bool,
    width: f32,
    color: Hsla,
) {
    let Some((first, rest)) = points.split_first() else {
        return;
    };
    let mut path = PathBuilder::stroke(px(width));
    path.move_to(*first);
    for p in rest {
        path.line_to(*p);
    }
    if closed {
        path.close();
    }
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

#[cfg(test)]
// The values compared are sums and products of small integers and
// halves, exact in binary floating point.
#[allow(clippy::float_cmp)]
mod tests {
    use super::math::*;
    use super::*;
    use bibli_core::geo::{BoundsBuilder, Line, Point as GeoPoint};

    fn at(lat: f64, lon: f64) -> LatLon {
        LatLon::new(lat, lon).unwrap()
    }

    /// Two squares, by centre and side, share some area.
    fn overlap(a: &Spot, b: &Spot) -> bool {
        let reach = f64::midpoint(a.side, b.side);
        (a.at.0 - b.at.0).abs() < reach - 1e-9 && (a.at.1 - b.at.1).abs() < reach - 1e-9
    }

    #[test]
    fn spiral_squares_never_overlap() {
        for n in [2, 3, 5, 8, 13, 40, 150, 1000] {
            let squares: Vec<Spot> = (0..n)
                .map(|k| Spot {
                    at: spiral(k, 1.),
                    side: 1.,
                    point: (0., 0.),
                    spread: true,
                })
                .collect();
            for (i, a) in squares.iter().enumerate() {
                for b in &squares[i + 1..] {
                    assert!(!overlap(a, b), "{n}: {a:?} and {b:?}");
                }
                // Inside what the spiral says it reaches.
                assert!(a.at.0.hypot(a.at.1) + 0.5 * 2f64.sqrt() <= spiral_reach(n, 1., 0.) + 1e-9);
            }
        }
    }

    #[test]
    fn a_spread_hides_nothing_and_keeps_its_order() {
        // Three piles of pictures taken at one spot each, two of them close
        // enough that their spirals would overlap, and one alone.
        let mut points = Vec::new();
        points.extend(std::iter::repeat_n((100., 100.), 30));
        points.extend(std::iter::repeat_n((130., 110.), 12));
        points.extend(std::iter::repeat_n((600., 400.), 5));
        points.push((900., 50.));
        let side = |n: usize| if n > 20 { 20. } else { 40. };
        let spots = spread(&points, side, 4.);
        for (i, a) in spots.iter().enumerate() {
            assert_eq!(a.point, points[i]);
            for b in &spots[i + 1..] {
                assert!(!overlap(a, b), "{a:?} and {b:?}");
            }
        }
        // The alone stays where it is; the others share their spirals.
        assert_eq!(spots[47].at, (900., 50.));
        assert!(!spots[47].spread);
        assert!(spots[..47].iter().all(|s| s.spread));
        // The two close piles joined (the sizes of 42), the far one did not.
        assert!(spots[..42].iter().all(|s| s.side == 20.));
        assert!(spots[42..47].iter().all(|s| s.side == 40.));
        // The first in the order is the nearest the centre, and so on out.
        let centre = (
            (30. * 100. + 12. * 130.) / 42.,
            (30. * 100. + 12. * 110.) / 42.,
        );
        let from = |s: &Spot| (s.at.0 - centre.0).hypot(s.at.1 - centre.1);
        assert!(spots[..42].windows(2).all(|w| from(&w[0]) <= from(&w[1])));
    }

    #[test]
    fn the_projection_is_web_mercator() {
        assert_eq!(project(at(0., 0.)), (0.5, 0.5));
        let (x, y) = project(at(MAX_LAT, -180.));
        assert!(x.abs() < 1e-12 && y.abs() < 1e-9, "{x} {y}");
        let (lat, lon) = unproject(0.25, 0.3);
        let (x, y) = project(at(lat, lon));
        assert!((x - 0.25).abs() < 1e-12 && (y - 0.3).abs() < 1e-12);
        // Beyond the square world, latitudes are held at its edge.
        assert_eq!(project(at(89., 0.)).1, project(at(MAX_LAT, 0.)).1);
    }

    #[test]
    fn zooming_keeps_the_place_under_the_pointer() {
        let size = (800., 600.);
        let view = View {
            x: 0.52,
            y: 0.35,
            zoom: 10.,
        };
        let anchor = (123., 456.);
        let under = view.to_world(anchor, size);
        let zoomed = view.zoom_at(1.5, anchor, size);
        let after = zoomed.to_world(anchor, size);
        assert!((under.0 - after.0).abs() < 1e-12 && (under.1 - after.1).abs() < 1e-12);
        assert_eq!(zoomed.zoom, 11.5);
        // Dragging by a distance moves the view by as many pixels.
        let panned = view.pan(100., -50.);
        let moved = panned.to_screen(view.to_world((400., 300.), size), size);
        assert!((moved.0 - 500.).abs() < 1e-6 && (moved.1 - 250.).abs() < 1e-6);
    }

    #[test]
    fn a_view_stays_on_the_world() {
        let view = View {
            x: 1.25,
            y: -3.,
            zoom: 30.,
        }
        .clamp(600.);
        assert_eq!(view.zoom, MAX_ZOOM);
        assert!((view.x - 0.25).abs() < 1e-12);
        assert!(view.y > 0.);
        // A world shorter than the view is centred.
        let small = View {
            x: 0.1,
            y: 0.1,
            zoom: 0.,
        }
        .clamp(600.);
        assert_eq!(small.y, 0.5);
    }

    #[test]
    fn the_area_shown_is_its_edges_in_degrees() {
        let size = (512., 512.);
        let (x, y) = project(at(10., 20.));
        let [south, west, north, east] = area(&View { x, y, zoom: 8. }, size);
        assert!(south < 10. && north > 10. && west < 20. && east > 20.);
        // Two tiles of 256 at zoom 8 cover 360 / 128 degrees of longitude.
        assert!(((east - west) - 360. / 128.).abs() < 1e-9, "{west} {east}");
        // Across ±180°: the west edge is past the east one.
        let (x, y) = project(at(60., 179.9));
        let [_, west, _, east] = area(&View { x, y, zoom: 8. }, size);
        assert!(west > east, "{west} {east}");
        // The world and more: every longitude.
        let [_, west, _, east] = area(
            &View {
                x: 0.5,
                y: 0.5,
                zoom: 0.,
            },
            (2048., 512.),
        );
        assert_eq!((west, east), (-180., 180.));
    }

    #[test]
    fn fitting_shows_everything_with_room_around() {
        let rect = Rect {
            x0: 0.5,
            y0: 0.3,
            x1: 0.51,
            y1: 0.305,
        };
        let size = (800., 600.);
        let view = fit(rect, size);
        let (left, top) = view.to_screen((rect.x0, rect.y0), size);
        let (right, bottom) = view.to_screen((rect.x1, rect.y1), size);
        assert!(left >= 31.9 && right <= 768.1 && top >= 31.9 && bottom <= 568.1);
        // One side fills its room.
        assert!((left - 32.).abs() < 1e-6 || (top - 32.).abs() < 1e-6);
        // A single place is shown at street level, not infinitely close.
        let point = fit(
            Rect {
                x0: 0.5,
                y0: 0.5,
                x1: 0.5,
                y1: 0.5,
            },
            size,
        );
        assert_eq!(point.zoom, 15.);
    }

    #[test]
    fn a_line_over_the_antimeridian_stays_whole() {
        let mut bounds = BoundsBuilder::default();
        let points = [at(65., 179.5), at(65.5, -179.5)];
        for p in points {
            bounds.add(p);
        }
        let doc = GeoDoc {
            lines: vec![Line {
                segments: vec![
                    points
                        .iter()
                        .map(|&at| GeoPoint {
                            at,
                            altitude: None,
                            time: None,
                        })
                        .collect(),
                ],
                ..Line::default()
            }],
            bounds: bounds.finish(),
            ..GeoDoc::default()
        };
        let shapes = Shapes::new(&doc);
        let segment = &shapes.lines[0].segments[0];
        // One degree apart, not 359.
        assert!((segment[1].0 - segment[0].0 - 1. / 360.).abs() < 1e-9);
        let rect = shapes.bounds.unwrap();
        assert!(rect.x1 - rect.x0 < 0.01);
        // Start and end are marked.
        assert!(shapes.markers.iter().any(|m| m.kind == MarkerKind::Start));
        assert!(shapes.markers.iter().any(|m| m.kind == MarkerKind::End));
        // The copy drawn is the one nearest the view, however far it panned.
        assert_eq!(copy_offset(2.99, rect.center().0), 2.);
    }

    #[test]
    fn a_line_is_thinned_to_its_pixels_and_cut_at_the_edges() {
        let view = View {
            x: 0.5,
            y: 0.5,
            zoom: 2.,
        };
        let size = (100., 100.);
        // A thousand points within a pixel, then far off to the east.
        let mut points: Vec<(f64, f64)> = (0..1000)
            .map(|i| (0.5 + f64::from(i) * 1e-7, 0.5))
            .collect();
        points.push((0.9, 0.5));
        points.push((0.95, 0.5));
        let mut out = Vec::new();
        runs(&points, &view, 0., size, point(px(0.), px(0.)), &mut out);
        assert_eq!(out.len(), 1);
        // The start, the step leaving the view; the rest is off screen.
        assert!(out[0].len() <= 3, "{}", out[0].len());
    }

    #[test]
    fn graticules_and_scales_read_as_round_numbers() {
        assert_eq!(graticule_step(0., 90.), 90.);
        let step = graticule_step(10., 90.);
        assert!(step * scale(10.) / 360. >= 90.);
        assert_eq!(degrees(45., 5., 'N', 'S'), "45°N");
        assert_eq!(degrees(-6.5, 0.5, 'E', 'W'), "6.5°W");
        assert_eq!(degrees(0., 1., 'E', 'W'), "0°");
        assert_eq!(wrap_lon(190.), -170.);
        let (length, words) = scale_bar(0., 10., 100.);
        assert!(length <= 100. && length > 20., "{length}");
        assert!(words.ends_with(" km") || words.ends_with(" m"), "{words}");
        let (_, words) = scale_bar(45., 16., 100.);
        assert!(words.ends_with(" m"), "{words}");
    }
}
