//! GPUI spike: a 100k-row `uniform_list` with lazy thumbnails, driven by the
//! shared scroll script and metrics from `spike-common`.
//!
//! Diagnostic env toggles (off by default, the default run is the contract):
//! `SPIKE_STATIC` skips the scroll script, `SPIKE_NO_TEXT` draws rows as bare
//! colored bars, `SPIKE_NO_THUMBS` never loads thumbnails, `SPIKE_FONT` picks
//! the font family (default "DejaVu Sans": GPUI's `.SystemUIFont` resolves to
//! nothing on this machine and the per-frame fallback search costs ~35 ms).

use std::collections::HashMap;
use std::sync::Arc;

use gpui::{
    AnyElement, App, Application, Bounds, Context, Div, RenderImage, Stateful, UniformListScrollHandle,
    Window, WindowBounds, WindowOptions, div, img, point, prelude::*, px, rgb, size,
    uniform_list,
};
use smallvec::SmallVec;
use spike_common::{Args, Metrics, ROW_HEIGHT_PX, ScrollDriver, THUMB_PX, Thumbs};

struct ListView {
    args: Args,
    metrics: Metrics,
    thumbs: Arc<Thumbs>,
    driver: ScrollDriver,
    scroll: UniformListScrollHandle,
    /// Converted GPUI images, keyed by image index (not row), so a decoded
    /// PNG is turned into a `RenderImage` exactly once.
    images: HashMap<usize, Arc<RenderImage>>,
}

impl ListView {
    fn image(&mut self, row: usize) -> Option<Arc<RenderImage>> {
        if std::env::var_os("SPIKE_NO_THUMBS").is_some() {
            return None;
        }
        let idx = self.thumbs.image_index(row);
        if let Some(im) = self.images.get(&idx) {
            return Some(Arc::clone(im));
        }
        let d = self.thumbs.get(row)?;
        // GPUI's sprite atlas expects BGRA8.
        let mut buf = d.rgba.clone();
        for p in buf.chunks_exact_mut(4) {
            p.swap(0, 2);
        }
        let buffer = image::RgbaImage::from_raw(d.width, d.height, buf)?;
        let frame = image::Frame::new(buffer);
        let ri = Arc::new(RenderImage::new(SmallVec::from_elem(frame, 1)));
        // Mirror the loader's cap so we never hold more than it does.
        if self.images.len() >= 1024 {
            self.images.clear();
        }
        self.images.insert(idx, Arc::clone(&ri));
        Some(ri)
    }

    fn row_element(&mut self, i: usize) -> Stateful<Div> {
        let r = spike_common::row(i);
        if std::env::var_os("SPIKE_NO_TEXT").is_some() {
            return div().id(i).h(px(ROW_HEIGHT_PX)).w_full().bg(rgb(0x2a2a2a + (i as u32 % 2) * 0x080808));
        }
        let thumb: AnyElement = match self.image(i) {
            Some(src) => img(src).size(px(THUMB_PX)).flex_none().rounded_sm().into_any_element(),
            None => div().size(px(THUMB_PX)).flex_none().rounded_sm().bg(rgb(0x3a3a3a)).into_any_element(),
        };
        div()
            .id(i)
            .h(px(ROW_HEIGHT_PX))
            .w_full()
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .px_2()
            .child(thumb)
            .child(div().flex_1().overflow_hidden().child(r.name))
            .child(div().w(px(100.)).text_color(rgb(0x9a9a9a)).child(r.size))
            .child(div().w(px(160.)).text_color(rgb(0x9a9a9a)).child(r.modified))
    }
}

impl Render for ListView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // One call per frame: the view re-renders every frame because we
        // request an animation frame below.
        self.thumbs.tick();
        let t = self.metrics.frame();
        if self.driver.done(t) {
            let report = self.metrics.report("gpui", self.args.rows, &self.thumbs);
            spike_common::finish(&report, &self.args);
        }
        // Pixel-precise offset through the uniform list's base scroll handle.
        // GPUI's convention: the offset grows negative as you scroll down.
        let offset = self.driver.offset_at(t);
        if std::env::var_os("SPIKE_STATIC").is_none() {
            self.scroll.0.borrow().base_handle.set_offset(point(px(0.), px(-offset)));
        }
        window.request_animation_frame();

        div()
            .size_full()
            .font_family(std::env::var("SPIKE_FONT").unwrap_or_else(|_| "DejaVu Sans".into()))
            .bg(rgb(0x1e1e1e))
            .text_color(rgb(0xdddddd))
            .text_sm()
            .child(
                uniform_list(
                    "rows",
                    self.args.rows,
                    cx.processor(|this, range: std::ops::Range<usize>, _window, _cx| {
                        range.map(|i| this.row_element(i)).collect::<Vec<_>>()
                    }),
                )
                .track_scroll(self.scroll.clone())
                .h_full(),
            )
    }
}

fn main() {
    let metrics = Metrics::start();
    let args = spike_common::parse_args();
    let thumbs = Thumbs::new(&args.images, 4);
    // The view requests an animation frame on every render, so the window
    // already redraws continuously; nothing extra is needed to surface a
    // freshly decoded thumbnail.
    thumbs.set_on_ready(|| {});
    let driver = ScrollDriver::new(args.rows, args.duration_secs);

    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| {
                cx.activate(true);
                cx.new(|_| ListView {
                    args,
                    metrics,
                    thumbs,
                    driver,
                    scroll: UniformListScrollHandle::new(),
                    images: HashMap::new(),
                })
            },
        )
        .unwrap();
    });
}
