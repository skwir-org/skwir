//! A 3D model turned in the window and drawn by its GPU
//! (`Window::paint_mesh`, ADR-067): the preview of a mesh (`model_preview`).
//!
//! It opens as `skwir-model-worker` draws its still: 35° right of the
//! front, 28° above, without perspective, framed alike and lit by the same
//! two lights, so the thumbnail standing in for it gives way without a
//! jump. A drag turns it around its up axis and tilts it, Shift or the
//! middle button moves it, the wheel zooms at the pointer. Each input asks
//! for one frame; nothing is drawn otherwise. Until the GPU has it (gpui
//! uploads it off the window's thread, then draws the window again), it
//! says it is loading; a mesh the GPU cannot hold, that it is too large.
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    App, Bounds, CursorStyle, DispatchPhase, Element, ElementId, GlobalElementId, Hitbox,
    HitboxBehavior, Hsla, InspectorElementId, IntoElement, LayoutId, MeshLight, MeshPainted,
    MeshView, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point,
    ScrollDelta, ScrollEvent, Size, Style, Window, point, px, relative,
};

/// Where the still is drawn from: degrees right of the front, and above.
const AZIMUTH: f32 = 35.;
const ELEVATION: f32 = 28.;
/// The surface's colour in linear light, and its lights, as the still's:
/// a grey that reads on a light or a dark panel.
const ALBEDO: [f32; 3] = [0.56, 0.60, 0.68];
const AMBIENT: f32 = 0.22;
/// Toward each light, as the picture's right, up and toward the viewer
/// weigh it, and its strength.
const KEY: ([f32; 3], f32) = ([-0.4, 0.8, 0.6], 0.62);
const FILL: ([f32; 3], f32) = ([0.7, -0.2, 0.5], 0.28);
/// Degrees a pixel of drag turns it.
const TURN_PER_PIXEL: f32 = 0.4;
/// The most it is zoomed in or out from how it opened.
const ZOOM_RANGE: (f32, f32) = (0.05, 200.);
/// With perspective, the camera stands this many times the model's radius
/// from its middle: the nearest of it a quarter larger than its middle, as
/// a camera's normal lens shows it (about 25° high).
const DISTANCE: f32 = 5.;

/// How the model is drawn, as the user chose (`reading.model_*`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Look {
    /// Nearer parts larger, as a camera sees them; else sizes kept at any
    /// depth, as the still is drawn.
    pub perspective: bool,
    /// Faces shaded smoothly across; else each flat.
    pub smooth: bool,
}

type Vec3 = [f32; 3];

fn dot(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalized(a: Vec3) -> Vec3 {
    let length = dot(a, a).sqrt();
    if length > 0. {
        a.map(|v| v / length)
    } else {
        a
    }
}

/// The picture's right, up and toward the viewer, in the model's
/// coordinates (Z up), seen from `azimuth` and `elevation` degrees.
fn axes(azimuth: f32, elevation: f32) -> [Vec3; 3] {
    let (az, el) = (azimuth.to_radians(), elevation.to_radians());
    let toward = [az.sin() * el.cos(), -az.cos() * el.cos(), el.sin()];
    let right = [az.cos(), az.sin(), 0.];
    [right, cross(toward, right), toward]
}

/// How the model is seen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    /// Degrees right of the front, around the up axis.
    pub azimuth: f32,
    /// Degrees above, from -90 to 90.
    pub elevation: f32,
    /// How much larger than it opened.
    pub zoom: f32,
    /// What the middle of the view shows, from the model's middle, in its
    /// units along the picture's right and up.
    pub pan: [f32; 2],
}

impl Camera {
    /// Turned by `right` and `up` degrees.
    #[must_use]
    pub fn turned(self, right: f32, up: f32) -> Self {
        Camera {
            azimuth: (self.azimuth + right).rem_euclid(360.),
            elevation: (self.elevation + up).clamp(-90., 90.),
            ..self
        }
    }

    /// Moved by `right` and `up` of its units.
    #[must_use]
    pub fn moved(self, right: f32, up: f32) -> Self {
        Camera {
            pan: [self.pan[0] + right, self.pan[1] + up],
            ..self
        }
    }
}

/// A mesh, ready to be turned: what is worked out once, on the worker.
pub struct Model {
    pub mesh: Arc<gpui::Mesh>,
    /// The middle of its box, which it turns around.
    middle: Vec3,
    /// Nothing of it is further from the middle.
    radius: f32,
    /// What the still shows: the middle of its picture, from the middle's,
    /// and its width and height, in the model's units.
    framed: ([f32; 2], [f32; 2]),
}

impl Model {
    /// `positions` and the triangles between them (`indices`), and the
    /// positions' colours if any, checked again on the way to the GPU.
    pub fn new(
        positions: Vec<[f32; 3]>,
        indices: Vec<u32>,
        colours: Option<Vec<[u8; 4]>>,
    ) -> Result<Self, gpui::MeshError> {
        let mut mesh = gpui::Mesh::new(positions, indices)?;
        if let Some(colours) = colours {
            mesh = mesh.with_colors(colours)?;
        }
        let points = mesh.positions();
        let mut lo = [f32::INFINITY; 3];
        let mut hi = [f32::NEG_INFINITY; 3];
        for p in points {
            for axis in 0..3 {
                lo[axis] = lo[axis].min(p[axis]);
                hi[axis] = hi[axis].max(p[axis]);
            }
        }
        let middle = [0, 1, 2].map(|axis| f32::midpoint(lo[axis], hi[axis]));
        let radius = (0..3)
            .map(|axis| (hi[axis] - lo[axis]) / 2.)
            .map(|half| half * half)
            .sum::<f32>()
            .sqrt();
        // The picture's box, as the still frames it.
        let [right, up, _] = axes(AZIMUTH, ELEVATION);
        let (mut low, mut high) = ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]);
        for p in points {
            for (axis, direction) in [right, up].into_iter().enumerate() {
                let along = dot(*p, direction);
                low[axis] = low[axis].min(along);
                high[axis] = high[axis].max(along);
            }
        }
        let framed = (
            [
                f32::midpoint(low[0], high[0]) - dot(middle, right),
                f32::midpoint(low[1], high[1]) - dot(middle, up),
            ],
            [high[0] - low[0], high[1] - low[1]],
        );
        Ok(Model {
            mesh: Arc::new(mesh),
            middle,
            radius: radius.max(f32::MIN_POSITIVE),
            framed,
        })
    }

    /// How it opens: as the still is drawn.
    pub fn opening(&self) -> Camera {
        Camera {
            azimuth: AZIMUTH,
            elevation: ELEVATION,
            zoom: 1.,
            pan: self.framed.0,
        }
    }

    /// Pixels a unit of the model takes in `size`, as it opened: as the
    /// still fits there, with its margin (a twentieth of its side around
    /// it, its longer side's eighteenth on each side).
    fn fit(&self, size: Size<Pixels>) -> f32 {
        let [width, height] = self.framed.1;
        let margin = width.max(height).max(self.radius * 1e-3) / 9.;
        (f32::from(size.width) / (width + margin)).min(f32::from(size.height) / (height + margin))
    }

    /// Pixels a unit of the model takes in `size`, seen by `camera`.
    fn scale(&self, camera: &Camera, size: Size<Pixels>) -> f32 {
        self.fit(size) * camera.zoom
    }

    /// The matrices and lights `camera` sees it with in `size`, drawn as
    /// `look` says. Either way the plane through its middle facing the
    /// camera is drawn at the same scale, so perspective changes nothing
    /// there.
    pub fn view(&self, camera: &Camera, size: Size<Pixels>, look: Look) -> MeshView {
        let [right, up, toward] = axes(camera.azimuth, camera.elevation);
        let scale = self.scale(camera, size);
        let (width, height) = (f32::from(size.width), f32::from(size.height));
        let (middle, radius) = (self.middle, self.radius);
        // With perspective the camera stands back from the middle, at the
        // origin of its coordinates, looking down -z.
        let distance = if look.perspective {
            DISTANCE * radius
        } else {
            0.
        };
        // Columns: the model's axes in the camera's, then where its origin
        // goes, the middle of the view showing `pan`.
        let model_view = [
            [right[0], up[0], toward[0], 0.],
            [right[1], up[1], toward[1], 0.],
            [right[2], up[2], toward[2], 0.],
            [
                -dot(right, middle) - camera.pan[0],
                -dot(up, middle) - camera.pan[1],
                -dot(toward, middle) - distance,
                1.,
            ],
        ];
        // Depth from the near side of its sphere (0) to the far side (1).
        let (x, y) = (2. * scale / width.max(1.), 2. * scale / height.max(1.));
        let projection = if look.perspective {
            // x and y divided by how far a point is, `distance` at the
            // middle; depth 0 to 1 from `near` to `far`.
            let (near, far) = (distance - radius, distance + radius);
            let a = far / (far - near);
            [
                [x * distance, 0., 0., 0.],
                [0., y * distance, 0., 0.],
                [0., 0., -a, -1.],
                [0., 0., -near * a, 0.],
            ]
        } else {
            [
                [x, 0., 0., 0.],
                [0., y, 0., 0.],
                [0., 0., -0.5 / radius, 0.],
                [0., 0., 0.5, 1.],
            ]
        };
        let light = |(direction, strength): (Vec3, f32)| MeshLight {
            direction: normalized(direction),
            strength,
        };
        MeshView {
            model_view,
            projection,
            perspective: look.perspective,
            smooth: look.smooth,
            color: ALBEDO,
            ambient: AMBIENT,
            lights: [light(KEY), light(FILL)],
        }
    }

    /// `camera` zoomed by `factor` around the point `at` pixels from the
    /// middle of a view of `size` (right and down), which stays where it is.
    pub fn zoomed_at(
        &self,
        camera: Camera,
        factor: f32,
        at: [f32; 2],
        size: Size<Pixels>,
    ) -> Camera {
        let zoom = (camera.zoom * factor).clamp(ZOOM_RANGE.0, ZOOM_RANGE.1);
        let before = self.scale(&camera, size);
        let after = self.scale(&Camera { zoom, ..camera }, size);
        Camera {
            zoom,
            pan: [
                camera.pan[0] + at[0] / before - at[0] / after,
                camera.pan[1] - at[1] / before + at[1] / after,
            ],
            ..camera
        }
    }

    /// `camera` moved by `pixels` (right and down), as a drag moves it.
    pub fn dragged(&self, camera: Camera, pixels: [f32; 2], size: Size<Pixels>) -> Camera {
        let scale = self.scale(&camera, size);
        camera.moved(-pixels[0] / scale, pixels[1] / scale)
    }

    /// `camera` moved by `pixels` of the view (right and down), as the
    /// keys move it: the view, not the model.
    pub fn stepped(&self, camera: Camera, pixels: [f32; 2], size: Size<Pixels>) -> Camera {
        self.dragged(camera, [-pixels[0], -pixels[1]], size)
    }
}

/// What a drag does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Drag {
    Turn,
    Move,
}

/// The model's view, kept with the preview (`PreviewState::model`).
#[derive(Clone, Default)]
pub struct Handles {
    /// The camera the user chose; `None` shows it as it opened.
    pub camera: Rc<Cell<Option<Camera>>>,
    /// The size it was drawn at last, for a key to move it by pixels: read
    /// by an action, never for layout.
    pub shown: Rc<Cell<Option<Size<Pixels>>>>,
    /// Where a drag was last seen, and what it does.
    drag: Rc<Cell<Option<(Point<Pixels>, Drag)>>>,
}

pub struct ModelView {
    id: ElementId,
    model: Arc<Model>,
    handles: Handles,
    look: Look,
    /// What it says while it loads is written in.
    muted: Hsla,
}

/// `model` drawn in the space it is given as `look` says, seen and moved
/// through `handles`; until it can be, "Loading…" in `muted`.
pub fn model(
    id: impl Into<ElementId>,
    model: Arc<Model>,
    handles: Handles,
    look: Look,
    muted: Hsla,
) -> ModelView {
    ModelView {
        id: id.into(),
        model,
        handles,
        look,
        muted,
    }
}

impl IntoElement for ModelView {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for ModelView {
    type RequestLayoutState = ();
    type PrepaintState = Hitbox;

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
    ) -> Hitbox {
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
        let model = &self.model;
        let handles = &self.handles;
        let size = bounds.size;
        handles.shown.set(Some(size));
        let camera = handles.camera.get().unwrap_or_else(|| model.opening());
        let view = model.view(&camera, size, self.look);
        let said = match window.paint_mesh(bounds, Arc::clone(&model.mesh), view, cx) {
            MeshPainted::Painted => None,
            MeshPainted::Loading => Some("Loading…"),
            // The 3D button shows its still again.
            MeshPainted::Refused => Some("Too large for this GPU to draw"),
        };
        if let Some(said) = said {
            let words = crate::elements::map::shape(said.into(), self.muted, window);
            let at = bounds.center() - point(words.width / 2., px(7.));
            let _ = words.paint(at, px(14.), gpui::TextAlign::Left, None, window, cx);
        }
        window.set_cursor_style(
            if handles.drag.get().is_some() {
                CursorStyle::ClosedHand
            } else {
                CursorStyle::OpenHand
            },
            hitbox,
        );

        let (hit, drag) = (hitbox.clone(), Rc::clone(&handles.drag));
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, _| {
            if phase != DispatchPhase::Bubble || !hit.is_hovered(window) {
                return;
            }
            let what = match event.button {
                MouseButton::Left if event.modifiers.shift => Drag::Move,
                MouseButton::Left => Drag::Turn,
                MouseButton::Middle => Drag::Move,
                _ => return,
            };
            drag.set(Some((event.position, what)));
            window.refresh();
        });
        let (shown, opened) = (handles.clone(), Arc::clone(model));
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, _| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            let Some((last, what)) = shown.drag.get() else {
                return;
            };
            // A release outside the window is never delivered here.
            if event.pressed_button.is_none() {
                shown.drag.set(None);
                window.refresh();
                return;
            }
            let moved = event.position - last;
            let (dx, dy) = (f32::from(moved.x), f32::from(moved.y));
            let camera = shown.camera.get().unwrap_or_else(|| opened.opening());
            shown.camera.set(Some(match what {
                // Grabbed: dragged right, its front turns right; dragged
                // down, its top comes forward.
                Drag::Turn => camera.turned(-dx * TURN_PER_PIXEL, dy * TURN_PER_PIXEL),
                Drag::Move => opened.dragged(camera, [dx, dy], size),
            }));
            shown.drag.set(Some((event.position, what)));
            window.refresh();
        });
        let drag = Rc::clone(&handles.drag);
        window.on_mouse_event(move |_: &MouseUpEvent, phase, window, _| {
            if phase == DispatchPhase::Bubble && drag.take().is_some() {
                window.refresh();
            }
        });
        let (hit, wheel, opened) = (hitbox.clone(), handles.clone(), Arc::clone(model));
        window.on_mouse_event(move |event: &ScrollEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || !hit.should_handle_scroll(window) {
                return;
            }
            // As the map zooms: a notch of the wheel (three lines, as gpui
            // reports it on Linux) is half a doubling; a touchpad zooms as
            // smoothly as it scrolls.
            let doublings = match event.delta {
                ScrollDelta::Lines(lines) => lines.y / 6.,
                ScrollDelta::Pixels(pixels) => f32::from(pixels.y) / 240.,
            }
            .clamp(-2., 2.);
            if doublings == 0. {
                return;
            }
            let at = event.position - bounds.center();
            let camera = wheel.camera.get().unwrap_or_else(|| opened.opening());
            wheel.camera.set(Some(opened.zoomed_at(
                camera,
                doublings.exp2(),
                [f32::from(at.x), f32::from(at.y)],
                size,
            )));
            cx.stop_propagation();
            window.refresh();
        });
    }
}

#[cfg(test)]
// The angles and zooms compared are sums and products of whole numbers,
// exact in binary floating point.
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use gpui::px;

    /// A box 20 × 10 × 5 standing on the floor, its corner at the origin.
    fn boxed() -> Model {
        let mut positions = Vec::new();
        for z in [0., 5.] {
            for (x, y) in [(0., 0.), (20., 0.), (20., 10.), (0., 10.)] {
                positions.push([x, y, z]);
            }
        }
        let indices = vec![
            0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4, 1, 2, 6, 1, 6, 5, 2, 3, 7, 2, 7,
            6, 3, 0, 4, 3, 4, 7,
        ];
        Model::new(positions, indices, None).unwrap()
    }

    /// Where `p` lands in a view of `size`, in pixels from its top left.
    fn pixel(view: &MeshView, p: Vec3, size: Size<Pixels>) -> [f32; 2] {
        let m = view.model_view;
        let eye: Vec<f32> = (0..3)
            .map(|row| m[0][row] * p[0] + m[1][row] * p[1] + m[2][row] * p[2] + m[3][row])
            .collect();
        let x = view.projection[0][0] * eye[0];
        let y = view.projection[1][1] * eye[1];
        [
            f32::midpoint(x, 1.) * f32::from(size.width),
            f32::midpoint(-y, 1.) * f32::from(size.height),
        ]
    }

    fn near(a: [f32; 2], b: [f32; 2]) -> bool {
        (a[0] - b[0]).abs() < 0.01 && (a[1] - b[1]).abs() < 0.01
    }

    /// It opens framed as the still: its picture's box in the middle, its
    /// longer side across the view but for the still's margin.
    #[test]
    fn a_model_opens_as_its_still_is_drawn() {
        let model = boxed();
        let size = Size {
            width: px(400.),
            height: px(400.),
        };
        let camera = model.opening();
        assert_eq!((camera.azimuth, camera.elevation), (35., 28.));
        let view = model.view(&camera, size, Look::default());
        let corners: Vec<[f32; 2]> = model
            .mesh
            .positions()
            .iter()
            .map(|p| pixel(&view, *p, size))
            .collect();
        let low = corners
            .iter()
            .fold([f32::INFINITY; 2], |a, c| [a[0].min(c[0]), a[1].min(c[1])]);
        let high = corners.iter().fold([f32::NEG_INFINITY; 2], |a, c| {
            [a[0].max(c[0]), a[1].max(c[1])]
        });
        // Centred, and as wide as the view less a twentieth on each side.
        assert!(near(
            [
                f32::midpoint(low[0], high[0]),
                f32::midpoint(low[1], high[1])
            ],
            [200., 200.]
        ));
        let (width, height) = (high[0] - low[0], high[1] - low[1]);
        assert!(
            (width.max(height) - 360.).abs() < 0.01,
            "{width} × {height}"
        );
        // Its depth spans the sphere around it, near side first.
        let toward = axes(35., 28.)[2];
        let depth = |p: Vec3| {
            let m = view.model_view;
            let z = m[0][2] * p[0] + m[1][2] * p[1] + m[2][2] * p[2] + m[3][2];
            view.projection[2][2] * z + view.projection[3][2]
        };
        let middle = [10., 5., 2.5];
        let front = [0, 1, 2].map(|i| middle[i] + toward[i] * model.radius);
        let back = [0, 1, 2].map(|i| middle[i] - toward[i] * model.radius);
        assert!(depth(front).abs() < 1e-4 && (depth(back) - 1.).abs() < 1e-4);
    }

    #[test]
    fn a_zoom_keeps_the_point_under_the_pointer() {
        let model = boxed();
        let size = Size {
            width: px(600.),
            height: px(300.),
        };
        let camera = model.opening();
        let corner = [20., 10., 5.];
        let before = pixel(&model.view(&camera, size, Look::default()), corner, size);
        let at = [before[0] - 300., before[1] - 150.];
        let zoomed = model.zoomed_at(camera, 2., at, size);
        assert_eq!(zoomed.zoom, 2.);
        assert!(near(
            pixel(&model.view(&zoomed, size, Look::default()), corner, size),
            before
        ));
        // A drag moves what is under the pointer with it.
        let dragged = model.dragged(zoomed, [30., -12.], size);
        let after = pixel(&model.view(&dragged, size, Look::default()), corner, size);
        assert!(near(after, [before[0] + 30., before[1] - 12.]));
        // A key moves the view: what is shown goes the other way.
        let stepped = model.stepped(zoomed, [30., -12.], size);
        let after = pixel(&model.view(&stepped, size, Look::default()), corner, size);
        assert!(near(after, [before[0] - 30., before[1] + 12.]));
    }

    /// With perspective, the plane through the middle is drawn as without;
    /// a corner nearer the camera lands further from the middle of the view,
    /// a farther one nearer; depth spans the model's sphere.
    #[test]
    fn perspective_keeps_the_middle_s_scale() {
        let model = boxed();
        let size = Size {
            width: px(400.),
            height: px(300.),
        };
        let camera = model.opening();
        let flat = model.view(&camera, size, Look::default());
        let deep = model.view(
            &camera,
            size,
            Look {
                perspective: true,
                smooth: false,
            },
        );
        assert!(deep.perspective && !deep.smooth);
        // Where `p` lands: the clip coordinates divided by w.
        let projected = |view: &MeshView, p: Vec3| {
            let (m, q) = (view.model_view, view.projection);
            let eye: Vec<f32> = (0..3)
                .map(|row| m[0][row] * p[0] + m[1][row] * p[1] + m[2][row] * p[2] + m[3][row])
                .collect();
            let clip: Vec<f32> = (0..4)
                .map(|row| q[0][row] * eye[0] + q[1][row] * eye[1] + q[2][row] * eye[2] + q[3][row])
                .collect();
            [clip[0] / clip[3], clip[1] / clip[3], clip[2] / clip[3]]
        };
        let [right, up, toward] = axes(35., 28.);
        let middle = [10., 5., 2.5];
        // On the middle's plane: where it is without perspective.
        let beside = [0, 1, 2].map(|i| middle[i] + 3. * right[i] + 2. * up[i]);
        let (a, b) = (projected(&deep, beside), projected(&flat, beside));
        assert!((a[0] - b[0]).abs() < 1e-5 && (a[1] - b[1]).abs() < 1e-5);
        // Nearer, further out; farther, further in.
        let nearer = [0, 1, 2].map(|i| beside[i] + toward[i] * 4.);
        let farther = [0, 1, 2].map(|i| beside[i] - toward[i] * 4.);
        assert!(projected(&deep, nearer)[0] > projected(&flat, nearer)[0]);
        assert!(projected(&deep, farther)[0] < projected(&flat, farther)[0]);
        // Depth: the sphere's near side 0, its far side 1.
        let front = [0, 1, 2].map(|i| middle[i] + toward[i] * model.radius);
        let back = [0, 1, 2].map(|i| middle[i] - toward[i] * model.radius);
        assert!(projected(&deep, front)[2].abs() < 1e-4);
        assert!((projected(&deep, back)[2] - 1.).abs() < 1e-4);
    }

    #[test]
    fn it_turns_all_the_way_round_and_tilts_to_the_poles() {
        let camera = boxed().opening();
        assert_eq!(camera.turned(330., 0.).azimuth, 5.);
        assert_eq!(camera.turned(-40., 0.).azimuth, 355.);
        assert_eq!(camera.turned(0., 100.).elevation, 90.);
        assert_eq!(camera.turned(0., -200.).elevation, -90.);
        // Seen from straight above, the picture's up is still a direction.
        let [right, up, toward] = axes(0., 90.);
        assert!((dot(up, up) - 1.).abs() < 1e-6);
        assert!(dot(right, up).abs() < 1e-6 && dot(up, toward).abs() < 1e-6);
    }
}
