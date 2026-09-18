//! Preview panel and peek overlay for the cursor entry: images (decoded on
//! a worker, cancelled when the cursor moves), text with syntax highlighting,
//! directories (item count), video first frame via ffmpegthumbnailer/ffmpeg,
//! and a generic card for everything else.

use std::path::Path;
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

pub struct PreviewState {
    pub open: bool,
    pub peek: bool,
    pub width: f32,
    pub for_id: Option<EntryId>,
    pub info: Info,
    pub content: Option<PreviewContent>,
    pub loading: bool,
    cancel: Option<CancelToken>,
    generation: u64,
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
            for_id: None,
            info: Info::default(),
            content: None,
            loading: false,
            cancel: None,
            generation: 0,
            tx,
            rx,
            text_scroll: UniformListScrollHandle::new(),
            peek_focus: None,
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
    })
}

fn video_frame(
    path: &Path,
    generation: u64,
    cancel: &CancelToken,
) -> anyhow::Result<PreviewContent> {
    let tmp = std::env::temp_dir().join(format!(
        "bibli-preview-{}-{generation}.png",
        std::process::id()
    ));
    let ok = if which("ffmpegthumbnailer") {
        std::process::Command::new("ffmpegthumbnailer")
            .args(["-i"])
            .arg(path)
            .args(["-o"])
            .arg(&tmp)
            .args(["-s", "1024", "-c", "png"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    } else if which("ffmpeg") {
        std::process::Command::new("ffmpeg")
            .args(["-y", "-loglevel", "error", "-i"])
            .arg(path)
            .args(["-frames:v", "1", "-vf", "scale=1024:-1"])
            .arg(&tmp)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    } else {
        false
    };
    let result = if ok && !cancel.is_cancelled() {
        decode_image(&tmp, cancel)
    } else {
        Ok(PreviewContent::Generic)
    };
    let _ = std::fs::remove_file(&tmp);
    result
}

fn which(bin: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(bin).is_file()))
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
    generation: u64,
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
    if top == "video" {
        return video_frame(path, generation, cancel)
            .unwrap_or_else(|e| PreviewContent::Error(format!("{e:#}")));
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
    /// Keep the preview in step with the cursor entry. Called from `render`.
    pub fn sync_preview(&mut self, cx: &mut Context<Self>) {
        // Results from workers.
        let mut got = false;
        while let Ok(r) = self.preview.rx.try_recv() {
            if r.generation == self.preview.generation {
                self.preview.content = Some(r.content);
                self.preview.loading = false;
                got = true;
            }
        }
        if got {
            cx.notify();
        }
        if !(self.preview.open || self.preview.peek) {
            if self.preview.for_id.is_some() {
                self.cancel_preview();
            }
            return;
        }
        let cursor = self.cursor_id();
        if cursor == self.preview.for_id && self.preview.for_id.is_some() {
            return;
        }
        self.cancel_preview();
        self.preview.for_id = cursor;
        let Some(id) = cursor else {
            return;
        };
        let (info, path, hydrated) = {
            let ws = self.app.workspace.read();
            let Some(pane) = ws.pane(self.pane) else {
                return;
            };
            let Some(e) = pane.listing.get(id) else {
                return;
            };
            let mime = if e.is_dir() {
                Some("inode/directory".to_owned())
            } else {
                bibli_platform::mime::by_name(&e.name).map(|m| m.as_str().to_owned())
            };
            let info = Info {
                name: e.name_str().into_owned(),
                mime,
                size: if e.is_dir() {
                    None
                } else {
                    e.metadata
                        .value(keys::SIZE)
                        .and_then(bibli_core::Value::as_u64)
                        .map(format::size)
                },
                modified: e
                    .metadata
                    .value(keys::MODIFIED)
                    .and_then(bibli_core::Value::as_time)
                    .map(format::time),
                is_dir: e.is_dir(),
                icon: self.icons.for_entry(e, 64),
            };
            (
                info,
                pane.open.as_ref().and_then(|o| o.path_of(id)),
                e.metadata.is_hydrated(),
            )
        };
        if !hydrated {
            self.request_hydrate(vec![id]);
        }
        self.preview.info = info.clone();
        let Some(path) = path else { return };
        let cancel = self.app.lifetime().child();
        self.preview.cancel = Some(cancel.clone());
        self.preview.generation += 1;
        self.preview.loading = true;
        let generation = self.preview.generation;
        let tx = self.preview.tx.clone();
        let app = Arc::clone(&self.app);
        let dark = cx.global::<Theme>().is_dark;
        let is_dir = info.is_dir;
        let mime = info.mime.clone();
        self.app
            .scheduler
            .spawn(Priority::Visible, "preview", cancel.clone(), move || {
                let content = compute(&path, is_dir, mime.as_deref(), dark, generation, &cancel);
                if !cancel.is_cancelled() {
                    let _ = tx.send(PreviewResult {
                        generation,
                        content,
                    });
                    app.wake();
                }
            });
        cx.notify();
    }

    fn cancel_preview(&mut self) {
        if let Some(c) = self.preview.cancel.take() {
            c.cancel();
        }
        self.preview.for_id = None;
        self.preview.content = None;
        self.preview.loading = false;
        self.preview
            .text_scroll
            .scroll_to_item_strict(0, ScrollStrategy::Top);
    }

    pub fn toggle_preview(&mut self, cx: &mut Context<Self>) {
        self.preview.open = !self.preview.open;
        // Tell the views their width now: the canvas that measures it only
        // runs after layout, which would leave one frame at the old column
        // count (visible as a lag in the grid). The measurement still
        // corrects any sub-pixel difference afterwards.
        let delta = if self.preview.open {
            -self.preview.width
        } else {
            self.preview.width
        };
        let w = &self.list.content_width;
        w.set(w.get() + delta);
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
        window.focus(&self.pane_focus);
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
        let new_width = (total - x).clamp(200.0, 900.0);
        let w = &self.list.content_width;
        w.set(w.get() + (self.preview.width - new_width));
        self.preview.width = new_width;
        cx.notify();
    }

    /// Root-level hook so edge drags are tracked anywhere in the window.
    pub fn resize_hook<E: InteractiveElement>(&self, el: E, cx: &mut Context<Self>) -> E {
        el.on_drag_move(cx.listener(Self::on_resize_drag))
    }
}

fn body(shell: &Shell, theme: &Theme, _cx: &mut Context<Shell>) -> AnyElement {
    let p = &shell.preview;
    match &p.content {
        None if p.loading => div()
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
        }) => div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_2()
            .child(
                img(Arc::clone(image))
                    .max_w_full()
                    .max_h_full()
                    .object_fit(ObjectFit::Contain),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme.text_muted)
                    .child(format!("{width} × {height}")),
            )
            .into_any_element(),
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
