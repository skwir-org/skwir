//! Preview panel and peek overlay for the cursor entry: images (decoded on
//! a worker, cancelled when the cursor moves), text with syntax highlighting,
//! directories, document pages and seekable video stills via preview plugins,
//! and a generic card for everything else.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use bibli_core::op::EntryRef;
use bibli_core::preview::{PreviewInfo, PreviewRequest};
use bibli_core::workspace::PaneId;
use std::sync::{Arc, OnceLock};

use bibli_core::entry::keys;
use bibli_core::scheduler::Priority;
use bibli_core::{CancelToken, EntryId};
use gpui::{
    AnyElement, Context, CursorStyle, DragMoveEvent, FocusHandle, Hsla, MouseButton, ObjectFit,
    Render, RenderImage, ScrollStrategy, UniformListScrollHandle, Window, deferred, div, img,
    prelude::*, px, rgb,
};
use smallvec::SmallVec;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;

use crate::format;
use crate::icons;
use crate::keymap;
use crate::shell::Shell;
use crate::theme::Theme;

const TEXT_LIMIT: usize = 64 * 1024;
const MAX_LINES: usize = 4000;
const IMAGE_MAX_SIDE: u32 = 2048;

pub enum PreviewContent {
    Image {
        image: Arc<RenderImage>,
        width: u32,
        height: u32,
        detail: Option<PreviewInfo>,
    },
    Text {
        lines: Vec<Vec<(Hsla, String)>>,
        truncated: bool,
    },
    Dir {
        items: usize,
    },
    Generic,
    Error(String),
}

struct PreviewResult {
    generation: u64,
    content: PreviewContent,
    complete: bool,
}

/// Facts shown in the header, read from the entry's metadata.
#[derive(Clone, Default)]
pub struct Info {
    pub name: String,
    pub mime: Option<String>,
    pub size: Option<String>,
    pub modified: Option<String>,
    pub is_dir: bool,
    pub icon: Option<Arc<Path>>,
}

/// Marker type for dragging the panel's edge.
#[derive(Clone)]
pub struct ResizeHandle;

struct NoPreview;

impl Render for NoPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// Everything that can invalidate a preview without moving the cursor.
#[derive(Clone, Debug, PartialEq, Eq)]
struct PreviewKey {
    pane: PaneId,
    id: EntryId,
    path: PathBuf,
    modified: Option<SystemTime>,
    size: Option<u64>,
    providers: u64,
    dark: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PreviewPhase {
    Idle,
    Queued,
    Loading,
}

pub struct PreviewState {
    pub open: bool,
    pub peek: bool,
    pub width: f32,
    pub info: Info,
    pub content: Option<PreviewContent>,
    phase: PreviewPhase,
    cancel: Option<CancelToken>,
    generation: u64,
    key: Option<PreviewKey>,
    page: u32,
    seconds: Option<f64>,
    zoom: Option<f32>,
    tx: flume::Sender<PreviewResult>,
    rx: flume::Receiver<PreviewResult>,
    text_scroll: UniformListScrollHandle,
    pub peek_focus: Option<FocusHandle>,
}

impl Default for PreviewState {
    fn default() -> Self {
        let (tx, rx) = flume::unbounded();
        PreviewState {
            open: false,
            peek: false,
            width: 320.0,
            info: Info::default(),
            content: None,
            phase: PreviewPhase::Idle,
            cancel: None,
            generation: 0,
            key: None,
            page: 1,
            seconds: None,
            zoom: None,
            tx,
            rx,
            text_scroll: UniformListScrollHandle::new(),
            peek_focus: None,
        }
    }
}

impl Drop for PreviewState {
    fn drop(&mut self) {
        if let Some(cancel) = &self.cancel {
            cancel.cancel();
        }
    }
}

fn syntaxes() -> &'static SyntaxSet {
    static S: OnceLock<SyntaxSet> = OnceLock::new();
    S.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn themes() -> &'static ThemeSet {
    static T: OnceLock<ThemeSet> = OnceLock::new();
    T.get_or_init(ThemeSet::load_defaults)
}

fn to_render_image(img: image::RgbaImage) -> Arc<RenderImage> {
    let (w, h) = (img.width(), img.height());
    let mut buf = img.into_raw();
    for p in buf.chunks_exact_mut(4) {
        p.swap(0, 2);
    }
    let buffer = image::RgbaImage::from_raw(w, h, buf).expect("same size");
    Arc::new(RenderImage::new(SmallVec::from_elem(
        image::Frame::new(buffer),
        1,
    )))
}

fn decode_image(path: &Path, cancel: &CancelToken) -> anyhow::Result<PreviewContent> {
    let (w, h) = image::ImageReader::open(path)?
        .with_guessed_format()?
        .into_dimensions()?;
    anyhow::ensure!(
        u64::from(w) * u64::from(h) <= 64 * 1024 * 1024,
        "image is too large to preview"
    );
    let img = image::ImageReader::open(path)?
        .with_guessed_format()?
        .decode()?;
    if cancel.is_cancelled() {
        anyhow::bail!("cancelled");
    }
    let img = if img.width().max(img.height()) > IMAGE_MAX_SIDE {
        img.thumbnail(IMAGE_MAX_SIDE, IMAGE_MAX_SIDE)
    } else {
        img
    };
    let rgba = img.to_rgba8();
    let (width, height) = (rgba.width(), rgba.height());
    Ok(PreviewContent::Image {
        image: to_render_image(rgba),
        width,
        height,
        detail: None,
    })
}

fn highlight(path: &Path, bytes: &[u8], dark: bool) -> PreviewContent {
    let truncated = bytes.len() >= TEXT_LIMIT;
    let text = String::from_utf8_lossy(bytes);
    let ss = syntaxes();
    let syntax = path
        .extension()
        .and_then(|e| e.to_str())
        .and_then(|e| ss.find_syntax_by_extension(e))
        .or_else(|| {
            text.lines()
                .next()
                .and_then(|l| ss.find_syntax_by_first_line(l))
        })
        .unwrap_or_else(|| ss.find_syntax_plain_text());
    let theme = &themes().themes[if dark {
        "base16-ocean.dark"
    } else {
        "base16-ocean.light"
    }];
    let mut h = syntect::easy::HighlightLines::new(syntax, theme);
    let mut lines = Vec::new();
    for line in syntect::util::LinesWithEndings::from(&text).take(MAX_LINES) {
        let spans = match h.highlight_line(line, ss) {
            Ok(regions) => regions
                .into_iter()
                .map(|(style, s)| {
                    let c = style.foreground;
                    let color: Hsla =
                        rgb(u32::from(c.r) << 16 | u32::from(c.g) << 8 | u32::from(c.b)).into();
                    (
                        color,
                        s.trim_end_matches(['\n', '\r']).replace('\t', "    "),
                    )
                })
                .filter(|(_, s)| !s.is_empty())
                .collect(),
            Err(_) => vec![(gpui::white(), line.trim_end().to_owned())],
        };
        lines.push(spans);
    }
    PreviewContent::Text { lines, truncated }
}

fn compute(
    path: &Path,
    is_dir: bool,
    mime: Option<&str>,
    dark: bool,
    cancel: &CancelToken,
) -> PreviewContent {
    if is_dir {
        return match std::fs::read_dir(path) {
            Ok(rd) => PreviewContent::Dir { items: rd.count() },
            Err(e) => PreviewContent::Error(e.to_string()),
        };
    }
    let top = mime.and_then(|m| m.split('/').next()).unwrap_or("");
    if top == "image" {
        return decode_image(path, cancel)
            .unwrap_or_else(|e| PreviewContent::Error(format!("{e:#}")));
    }
    if top == "video" || mime == Some("application/pdf") {
        return PreviewContent::Generic;
    }
    let head = match read_head(path) {
        Ok(h) => h,
        Err(e) => return PreviewContent::Error(e.to_string()),
    };
    let textual = top == "text"
        || matches!(
            mime,
            Some(
                "application/json"
                    | "application/xml"
                    | "application/toml"
                    | "application/x-shellscript"
                    | "application/javascript"
                    | "application/x-yaml"
            )
        )
        || (!head.contains(&0) && std::str::from_utf8(&head).is_ok());
    if textual && !head.contains(&0) {
        return highlight(path, &head, dark);
    }
    PreviewContent::Generic
}

fn read_head(path: &Path) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut buf = vec![0u8; TEXT_LIMIT];
    let mut n = 0;
    while n < TEXT_LIMIT {
        let k = f.read(&mut buf[n..])?;
        if k == 0 {
            break;
        }
        n += k;
    }
    buf.truncate(n);
    Ok(buf)
}

impl Shell {
    /// Called on UI wake. Only a changed request schedules worker work.
    pub fn sync_preview(&mut self, cx: &mut Context<Self>) {
        if !(self.preview.open || self.preview.peek) {
            if self.preview.key.is_some() {
                self.cancel_preview();
            }
            while self.preview.rx.try_recv().is_ok() {}
            return;
        }
        let Some(id) = self.cursor_id() else {
            self.cancel_preview();
            return;
        };
        let current = {
            let ws = self.app.workspace.read();
            ws.pane(self.pane).and_then(|pane| {
                let e = pane.listing.get(id)?;
                let path = pane.open.as_ref()?.path_of(id)?;
                Some((
                    PreviewKey {
                        pane: self.pane,
                        id,
                        path,
                        modified: e
                            .metadata
                            .value(keys::MODIFIED)
                            .and_then(bibli_core::Value::as_time),
                        size: e
                            .metadata
                            .value(keys::SIZE)
                            .and_then(bibli_core::Value::as_u64),
                        providers: self.app.previews.revision(),
                        dark: cx.global::<Theme>().is_dark,
                    },
                    e.clone(),
                ))
            })
        };
        let Some((key, entry)) = current else {
            self.cancel_preview();
            return;
        };
        // Invalidate before draining results: an old result must never land on
        // a newly selected file, a changed file, or a different pane.
        if self.preview.key.as_ref() != Some(&key) {
            self.cancel_preview();
            self.preview.key = Some(key.clone());
            self.preview.phase = PreviewPhase::Queued;
            let mime = if entry.is_dir() {
                Some("inode/directory".into())
            } else {
                bibli_platform::mime::by_name(&entry.name).map(|m| m.to_string())
            };
            self.preview.info = Info {
                name: entry.name_str().into_owned(),
                mime,
                size: key.size.map(format::size),
                modified: key.modified.map(format::time),
                is_dir: entry.is_dir(),
                icon: self.icons.for_entry(&entry, 64),
            };
            if !entry.metadata.is_hydrated() {
                self.request_hydrate(vec![id]);
            }
        }
        while let Ok(result) = self.preview.rx.try_recv() {
            if result.generation == self.preview.generation {
                if let PreviewContent::Image {
                    detail: Some(detail),
                    ..
                } = &result.content
                {
                    match detail {
                        PreviewInfo::Document { page, .. } => self.preview.page = *page,
                        PreviewInfo::Video { seconds, .. } => self.preview.seconds = Some(*seconds),
                    }
                }
                self.preview.content = Some(result.content);
                self.preview.phase = if result.complete {
                    PreviewPhase::Idle
                } else {
                    PreviewPhase::Loading
                };
            }
        }
        if self.preview.phase != PreviewPhase::Queued {
            return;
        }

        let cancel = self.app.lifetime().child();
        self.preview.cancel = Some(cancel.clone());
        self.preview.phase = PreviewPhase::Loading;
        let generation = self.preview.generation;
        let request = PreviewRequest {
            entry: EntryRef::new(id, &key.path),
            mime: self.preview.info.mime.clone(),
            page: self.preview.page,
            seconds: self.preview.seconds,
            max_side: IMAGE_MAX_SIDE,
        };
        let tx = self.preview.tx.clone();
        let previews = Arc::clone(&self.app.previews);
        let wake = self.app.waker_handle();
        let is_dir = entry.is_dir();
        // A desktop thumbnail represents the file, not the requested page or
        // timestamp. Only use it before this file has any preview content.
        let show_thumbnail = !is_dir && self.preview.content.is_none();
        self.app
            .scheduler
            .spawn(Priority::Visible, "preview", cancel.clone(), move || {
                // Reuse only an already cached desktop thumbnail as initial feedback.
                if show_thumbnail {
                    use bibli_core::thumbs::{ThumbRequest, ThumbSize, ThumbnailProvider};
                    let cached =
                        bibli_platform::thumb_providers::FreedesktopCache { write_back: false };
                    let thumb = ThumbRequest {
                        entry: request.entry.clone(),
                        mime: request.mime.clone().map(Into::into),
                        mtime: key.modified,
                        size: ThumbSize::Large,
                    };
                    if let Ok(Some(image)) = cached.thumbnail(&thumb, &cancel)
                        && let Some(render) = crate::thumbs::to_render_image(&image)
                        && !cancel.is_cancelled()
                    {
                        let _ = tx.send(PreviewResult {
                            generation,
                            complete: false,
                            content: PreviewContent::Image {
                                image: render,
                                width: image.width,
                                height: image.height,
                                detail: None,
                            },
                        });
                        wake();
                    }
                }
                let result = if is_dir {
                    Ok(None)
                } else {
                    previews.render(&request, &cancel)
                };
                let content = match result {
                    Ok(Some(preview)) => match crate::thumbs::to_render_image(&preview.image) {
                        Some(image) => PreviewContent::Image {
                            image,
                            width: preview.image.width,
                            height: preview.image.height,
                            detail: Some(preview.info),
                        },
                        None => PreviewContent::Error("Invalid preview image".into()),
                    },
                    Ok(None) => compute(
                        &key.path,
                        is_dir,
                        request.mime.as_deref(),
                        key.dark,
                        &cancel,
                    ),
                    Err(e) => PreviewContent::Error(format!("{e:#}")),
                };
                if !cancel.is_cancelled() {
                    let _ = tx.send(PreviewResult {
                        generation,
                        content,
                        complete: true,
                    });
                    wake();
                }
            });
    }

    fn cancel_pending_preview(&mut self) {
        if let Some(cancel) = self.preview.cancel.take() {
            cancel.cancel();
        }
        self.preview.generation = self.preview.generation.wrapping_add(1);
    }

    fn cancel_preview(&mut self) {
        self.cancel_pending_preview();
        self.preview.key = None;
        self.preview.info = Info::default();
        while self.preview.rx.try_recv().is_ok() {}
        self.preview.content = None;
        self.preview.phase = PreviewPhase::Idle;

        self.preview.page = 1;
        self.preview.seconds = None;
        self.preview.zoom = None;
        self.preview
            .text_scroll
            .scroll_to_item_strict(0, ScrollStrategy::Top);
    }

    pub fn preview_page(&mut self, delta: i32, cx: &mut Context<Self>) {
        let Some(PreviewContent::Image {
            detail: Some(PreviewInfo::Document { pages, .. }),
            ..
        }) = &self.preview.content
        else {
            return;
        };
        let next = self
            .preview
            .page
            .saturating_add_signed(delta)
            .clamp(1, *pages);
        if next != self.preview.page {
            self.cancel_pending_preview();
            self.preview.page = next;
            self.preview.phase = PreviewPhase::Queued;
            cx.notify();
        }
    }

    pub fn preview_seek(&mut self, delta: f64, cx: &mut Context<Self>) {
        let Some(PreviewContent::Image {
            detail: Some(PreviewInfo::Video { duration, .. }),
            ..
        }) = &self.preview.content
        else {
            return;
        };
        let next = (self.preview.seconds.unwrap_or(0.0) + delta)
            .max(0.0)
            .min(duration.map_or(f64::MAX, |d| (d - 0.1).max(0.0)));
        self.cancel_pending_preview();
        self.preview.seconds = Some(next);
        self.preview.phase = PreviewPhase::Queued;
        cx.notify();
    }

    pub fn preview_zoom(&mut self, direction: &str, cx: &mut Context<Self>) {
        self.preview.zoom = match direction {
            "in" => Some(self.preview.zoom.map_or(1.0, |z| (z * 1.25).min(4.0))),
            "out" => Some(self.preview.zoom.map_or(0.75, |z| (z / 1.25).max(0.25))),
            _ => None,
        };
        cx.notify();
    }

    pub fn toggle_preview(&mut self, cx: &mut Context<Self>) {
        self.preview.open = !self.preview.open;
        // Pane sizes are predicted at the start of the next render.
        cx.notify();
    }

    pub fn open_peek(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.cursor_id().is_none() {
            self.set_status("nothing to peek at", false, cx);
            return;
        }
        let focus = cx.focus_handle();
        window.focus(&focus);
        self.preview.peek_focus = Some(focus);
        self.preview.peek = true;
        cx.notify();
    }

    pub fn close_peek(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.preview.peek = false;
        self.preview.peek_focus = None;
        window.focus(&self.ui().focus);
        cx.notify();
    }

    pub fn toggle_peek(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.preview.peek {
            self.close_peek(window, cx);
        } else {
            self.open_peek(window, cx);
        }
    }

    fn on_resize_drag(
        &mut self,
        ev: &DragMoveEvent<ResizeHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let total = f32::from(window.viewport_size().width);
        let x = f32::from(ev.event.position.x);
        self.preview.width = (total - x).clamp(200.0, 900.0);
        cx.notify();
    }

    /// Root-level hook so edge drags are tracked anywhere in the window.
    pub fn resize_hook<E: InteractiveElement>(&self, el: E, cx: &mut Context<Self>) -> E {
        el.on_drag_move(cx.listener(Self::on_resize_drag))
    }
}

fn preview_button(
    id: &'static str,
    label: &'static str,
    command: &'static str,
    args: serde_json::Value,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    div()
        .id(id)
        .px_2()
        .py_0p5()
        .rounded_md()
        .cursor_pointer()
        .hover(|s| s.bg(theme.border))
        .child(label)
        .on_click(
            cx.listener(move |this, _, window, cx| this.run(command, args.clone(), window, cx)),
        )
        .into_any_element()
}

fn body(shell: &Shell, theme: &Theme, cx: &mut Context<Shell>) -> AnyElement {
    let p = &shell.preview;
    match &p.content {
        None if p.phase != PreviewPhase::Idle => div()
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .text_color(theme.text_muted)
            .child("Loading…")
            .into_any_element(),
        None | Some(PreviewContent::Generic) => generic(&p.info, theme),
        Some(PreviewContent::Image {
            image,
            width,
            height,
            detail,
        }) => {
            let picture = if let Some(zoom) = p.zoom {
                div()
                    .id("preview-image-scroll")
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_scroll()
                    .child(
                        img(Arc::clone(image))
                            .w(px(*width as f32 * zoom))
                            .h(px(*height as f32 * zoom)),
                    )
                    .into_any_element()
            } else {
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        img(Arc::clone(image))
                            .max_w_full()
                            .max_h_full()
                            .object_fit(ObjectFit::Contain),
                    )
                    .into_any_element()
            };
            let mut controls = div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .p_2()
                .text_xs();
            match detail {
                Some(PreviewInfo::Document { page, pages }) => {
                    controls = controls
                        .child(preview_button(
                            "preview-prev",
                            "‹",
                            "ui.preview_page",
                            serde_json::json!({"delta": -1}),
                            theme,
                            cx,
                        ))
                        .child(format!("Page {page} / {pages}"))
                        .child(preview_button(
                            "preview-next",
                            "›",
                            "ui.preview_page",
                            serde_json::json!({"delta": 1}),
                            theme,
                            cx,
                        ));
                }
                Some(PreviewInfo::Video { seconds, duration }) => {
                    let label = duration.map_or_else(
                        || format!("{seconds:.1}s"),
                        |d| format!("{seconds:.1}s / {d:.1}s"),
                    );
                    controls = controls
                        .child(preview_button(
                            "preview-back",
                            "−5s",
                            "ui.preview_seek",
                            serde_json::json!({"delta": -5}),
                            theme,
                            cx,
                        ))
                        .child(label)
                        .child(preview_button(
                            "preview-forward",
                            "+5s",
                            "ui.preview_seek",
                            serde_json::json!({"delta": 5}),
                            theme,
                            cx,
                        ));
                }
                None => {
                    controls = controls.child(format!("{width} × {height}"));
                }
            }
            controls = controls
                .child(preview_button(
                    "preview-zoom-out",
                    "−",
                    "ui.preview_zoom",
                    serde_json::json!({"dir":"out"}),
                    theme,
                    cx,
                ))
                .child(preview_button(
                    "preview-fit",
                    "Fit",
                    "ui.preview_zoom",
                    serde_json::json!({"dir":"reset"}),
                    theme,
                    cx,
                ))
                .child(preview_button(
                    "preview-zoom-in",
                    "+",
                    "ui.preview_zoom",
                    serde_json::json!({"dir":"in"}),
                    theme,
                    cx,
                ))
                .when(p.phase != PreviewPhase::Idle, |d| d.child("Loading…"));
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .child(picture)
                .child(controls)
                .into_any_element()
        }
        Some(PreviewContent::Text { lines, truncated }) => {
            let lines = lines.clone();
            let n = lines.len();
            let mono = shell.mono_font.clone();
            let list = gpui::uniform_list(
                "preview-text",
                n,
                move |range: std::ops::Range<usize>, _window: &mut Window, _cx: &mut gpui::App| {
                    range
                        .map(|i| {
                            let mut row = div().h(px(18.)).flex().flex_row().whitespace_nowrap();
                            for (color, text) in &lines[i] {
                                row = row.child(div().text_color(*color).child(text.clone()));
                            }
                            row
                        })
                        .collect::<Vec<_>>()
                },
            )
            .track_scroll(p.text_scroll.clone())
            .size_full();
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .font_family(mono)
                .text_xs()
                .px_2()
                .bg(if theme.is_dark {
                    rgb(0x2b303b)
                } else {
                    rgb(0xeff1f5)
                })
                .child(div().flex_1().min_h_0().w_full().child(list))
                .when(*truncated, |d| {
                    d.child(
                        div()
                            .text_color(theme.text_muted)
                            .child("first 64 KB shown"),
                    )
                })
                .into_any_element()
        }
        Some(PreviewContent::Dir { items }) => div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_2()
            .children(p.info.icon.as_ref().map(|i| icons::element(i, 64.)))
            .child(format!("{} items", format::thousands(*items)))
            .into_any_element(),
        Some(PreviewContent::Error(e)) => div()
            .flex_1()
            .p_2()
            .text_color(theme.error)
            .child(e.clone())
            .into_any_element(),
    }
}

fn generic(info: &Info, theme: &Theme) -> AnyElement {
    let mut card = div()
        .flex_1()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_1()
        .text_color(theme.text_muted)
        .children(info.icon.as_ref().map(|i| icons::element(i, 64.)))
        .child(div().text_color(theme.text).child(info.name.clone()));
    if let Some(m) = &info.mime {
        card = card.child(m.clone());
    }
    if let Some(s) = &info.size {
        card = card.child(s.clone());
    }
    if let Some(m) = &info.modified {
        card = card.child(m.clone());
    }
    card.into_any_element()
}

fn header(info: &Info, theme: &Theme) -> AnyElement {
    div()
        .px_2()
        .py_1()
        .border_b_1()
        .border_color(theme.border)
        .flex()
        .flex_col()
        .child(div().truncate().child(info.name.clone()))
        .child(
            div()
                .text_xs()
                .text_color(theme.text_muted)
                .truncate()
                .child(
                    [info.mime.clone(), info.size.clone(), info.modified.clone()]
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>()
                        .join(" · "),
                ),
        )
        .into_any_element()
}

/// The side panel, or `None` when closed.
pub fn render_panel(shell: &Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    if !shell.preview.open {
        return None;
    }
    let theme = cx.global::<Theme>().clone();
    let handle = div()
        .id("preview-resize")
        .absolute()
        .left_0()
        .top_0()
        .h_full()
        .w(px(6.))
        .cursor(CursorStyle::ResizeLeftRight)
        .on_drag(ResizeHandle, |_, _, _, cx| cx.new(|_| NoPreview));
    Some(
        div()
            .relative()
            .w(px(shell.preview.width))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(theme.border)
            .bg(theme.surface)
            .child(header(&shell.preview.info, &theme))
            .child(body(shell, &theme, cx))
            .child(handle)
            .into_any_element(),
    )
}

/// The centered peek overlay, or `None`.
pub fn render_peek(shell: &Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    if !shell.preview.peek {
        return None;
    }
    let theme = cx.global::<Theme>().clone();
    let focus = shell.preview.peek_focus.clone()?;
    Some(
        deferred(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(gpui::black().opacity(0.4))
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| this.close_peek(window, cx)),
                )
                .child(
                    div()
                        .track_focus(&focus)
                        .key_context(keymap::PEEK)
                        .w(gpui::relative(0.8))
                        .h(gpui::relative(0.8))
                        .flex()
                        .flex_col()
                        .rounded_lg()
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.surface)
                        .shadow_lg()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(header(&shell.preview.info, &theme))
                        .child(body(shell, &theme, cx))
                        .child(
                            div()
                                .px_2()
                                .py_1()
                                .text_xs()
                                .text_color(theme.text_muted)
                                .border_t_1()
                                .border_color(theme.border)
                                .child("Space or Escape to close · arrows to move"),
                        ),
                ),
        )
        .with_priority(5)
        .into_any_element(),
    )
}
