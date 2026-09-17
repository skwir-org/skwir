//! Floem spike: virtualized list of `--rows` entries with lazy thumbnails,
//! driven by the shared scroll script and metrics from `spike-common`.
//!
//! Frame hook: Floem has no per-frame callback. A zero-size `Driver` view is
//! placed after the scroll view; its `paint` runs once per window repaint, so
//! `Metrics::frame` is called at paint-submission time of every frame. The
//! driver then sets the scroll offset signal (which schedules a deferred
//! `ScrollTo`) and requests another paint, so the loop sustains itself.
//!
//! Thumbnail readiness: worker threads call `register_ext_trigger`, which
//! wakes the winit loop; an effect tracking that trigger requests a repaint.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use floem::context::PaintCx;
use floem::ext_event::register_ext_trigger;
use floem::kurbo::{Point, Size};
use floem::peniko::{Blob, Format, Image};
use floem::prelude::*;
use floem::reactive::{Trigger, create_effect};
use floem::window::WindowConfig;
use floem::{Application, ViewId};
use floem_renderer::Renderer;
use spike_common::{Args, Metrics, ROW_HEIGHT_PX, ScrollDriver, THUMB_PX, Thumbs};

// ------------------------------------------------------------ virtual vector

struct Rows(usize);

impl VirtualVector<usize> for Rows {
    fn total_len(&self) -> usize {
        self.0
    }
    fn slice(&mut self, range: Range<usize>) -> impl Iterator<Item = usize> {
        range
    }
}

// ------------------------------------------------------------ thumbnail view

thread_local! {
    /// Converted `peniko::Image` per image index, so the Blob is built once.
    static IMG_CACHE: RefCell<HashMap<usize, Image>> = RefCell::new(HashMap::new());
}

fn cached_image(thumbs: &Thumbs, row: usize) -> Option<(usize, Image)> {
    let idx = thumbs.image_index(row);
    if let Some(img) = IMG_CACHE.with(|c| c.borrow().get(&idx).cloned()) {
        return Some((idx, img));
    }
    let d = thumbs.get(row)?;
    let img = Image::new(Blob::from(d.rgba.clone()), Format::Rgba8, d.width, d.height);
    IMG_CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() > 2048 {
            c.clear();
        }
        c.insert(idx, img.clone());
    });
    Some((idx, img))
}

struct Thumb {
    id: ViewId,
    row: usize,
    thumbs: Arc<Thumbs>,
}

fn thumb(row: usize, thumbs: Arc<Thumbs>) -> Thumb {
    Thumb { id: ViewId::new(), row, thumbs }
}

impl View for Thumb {
    fn id(&self) -> ViewId {
        self.id
    }

    fn debug_name(&self) -> std::borrow::Cow<'static, str> {
        "Thumb".into()
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        let rect = self.id.get_content_rect();
        match cached_image(&self.thumbs, self.row) {
            Some((idx, img)) => {
                let hash = (idx as u64).to_le_bytes();
                cx.draw_img(floem_renderer::Img { img, hash: &hash }, rect);
            }
            None => cx.fill(&rect, Color::rgb8(60, 60, 60), 0.0),
        }
    }
}

// ------------------------------------------------------------ frame driver

struct DriverState {
    metrics: Metrics,
    driver: ScrollDriver,
    thumbs: Arc<Thumbs>,
    offset: RwSignal<f64>,
    args: Args,
}

struct Driver {
    id: ViewId,
    state: Rc<RefCell<DriverState>>,
}

impl View for Driver {
    fn id(&self) -> ViewId {
        self.id
    }

    fn debug_name(&self) -> std::borrow::Cow<'static, str> {
        "Driver".into()
    }

    fn paint(&mut self, _cx: &mut PaintCx) {
        let mut s = self.state.borrow_mut();
        s.thumbs.tick();
        let t = s.metrics.frame();
        if s.driver.done(t) {
            let report = s.metrics.report("floem", s.args.rows, &s.thumbs);
            spike_common::finish(&report, &s.args);
        }
        let off = f64::from(s.driver.offset_at(t));
        s.offset.set(off);
        self.id.request_paint();
    }
}

// ------------------------------------------------------------ app

fn row_view(i: usize, thumbs: Arc<Thumbs>) -> impl IntoView {
    let r = spike_common::row(i);
    h_stack((
        thumb(i, thumbs).style(|s| s.size(f64::from(THUMB_PX), f64::from(THUMB_PX))),
        static_label(r.name).style(|s| s.width(420.0)),
        static_label(r.size).style(|s| s.width(120.0)),
        static_label(r.modified).style(|s| s.width(200.0)),
    ))
    .style(|s| s.height(f64::from(ROW_HEIGHT_PX)).items_center().gap(16.0).padding_left(8.0))
}

fn app_view(metrics: Metrics, args: Args) -> impl IntoView {
    let thumbs = Thumbs::new(&args.images, 4);
    let rows = args.rows;
    let offset = create_rw_signal(0.0f64);

    // Wake the UI when a thumbnail lands.
    let ready = Trigger::new();
    thumbs.set_on_ready(move || register_ext_trigger(ready));

    let list_thumbs = Arc::clone(&thumbs);
    let list = virtual_stack(
        VirtualDirection::Vertical,
        VirtualItemSize::Fixed(Box::new(|| f64::from(ROW_HEIGHT_PX))),
        move || Rows(rows),
        |i| *i,
        move |i| row_view(i, Arc::clone(&list_thumbs)),
    )
    .style(|s| s.flex_col().width_full());

    let scroll_view = scroll(list)
        .scroll_to(move || Some(Point::new(0.0, offset.get())))
        .style(|s| s.size_full());
    let scroll_id = scroll_view.id();

    create_effect(move |_| {
        ready.track();
        scroll_id.request_paint();
    });

    let driver = Driver {
        id: ViewId::new(),
        state: Rc::new(RefCell::new(DriverState {
            metrics,
            driver: ScrollDriver::new(rows, args.duration_secs),
            thumbs,
            offset,
            args,
        })),
    };

    v_stack((scroll_view, driver.style(|s| s.size(0.0, 0.0)))).style(|s| s.size_full())
}

fn main() {
    let metrics = Metrics::start();
    let args = spike_common::parse_args();
    let config = WindowConfig::default().size(Size::new(1200.0, 800.0)).title("spike-floem");
    Application::new().window(move |_| app_view(metrics, args), Some(config)).run();
}
