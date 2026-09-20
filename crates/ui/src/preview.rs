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
    Render, RenderImage, ScrollStrategy, UniformListScrollHandle, Window, div, img, prelude::*, px,
    rgb,
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
    Inspection(crate::inspector::Display),
    Image {
        image: Arc<RenderImage>,
        width: u32,
        height: u32,
        detail: Option<PreviewInfo>,
        document: Option<Arc<bibli_core::document::PageContent>>,
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
    session: Option<Arc<dyn bibli_core::document::DocumentSession>>,
    media: Option<Arc<dyn bibli_core::media::MediaSession>>,
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
pub(crate) enum PreviewPhase {
    Idle,
    Queued,
    Loading,
}

pub struct PreviewState {
    pub reader: crate::reader::ReaderState,
    pub inspection: crate::inspector::State,
    pub media: crate::media::MediaState,
    pub menu: Option<crate::toolbar::Popup>,
    pub open: bool,
    pub peek: bool,
    pub width: f32,
    pub info: Info,
    pub content: Option<PreviewContent>,
    pub(crate) phase: PreviewPhase,
    cancel: Option<CancelToken>,
    pub(crate) generation: u64,
    key: Option<PreviewKey>,
    pub(crate) page: u32,
    pub(crate) seconds: Option<f64>,
    pub(crate) zoom: Option<f32>,
    pub(crate) fit_width: bool,
    tx: flume::Sender<PreviewResult>,
    rx: flume::Receiver<PreviewResult>,
    text_scroll: UniformListScrollHandle,
    pub peek_focus: Option<FocusHandle>,
    /// Whether the hexadecimal provider is registered, keyed by the preview
    /// registry revision it was read at (see `preview_toolbar::hex_available`).
    pub(crate) hex_provider: std::cell::Cell<Option<(u64, bool)>>,
}

impl Default for PreviewState {
    fn default() -> Self {
        let (tx, rx) = flume::unbounded();
        PreviewState {
            reader: crate::reader::ReaderState::default(),
            inspection: crate::inspector::State::default(),
            media: crate::media::MediaState::default(),
            menu: None,
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
            fit_width: false,
            tx,
            rx,
            text_scroll: UniformListScrollHandle::new(),
            peek_focus: None,
            hex_provider: std::cell::Cell::new(None),
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

pub(crate) fn syntaxes(language: Option<&str>) -> &'static SyntaxSet {
    static DEFAULTS: OnceLock<SyntaxSet> = OnceLock::new();
    static TOML: OnceLock<SyntaxSet> = OnceLock::new();
    if language.is_some_and(|language| language.eq_ignore_ascii_case("toml")) {
        TOML.get_or_init(|| {
            // Keep the bundled grammar separate: rebuilding the precompiled
            // default set would delay every language's first preview.
            let mut builder = syntect::parsing::SyntaxSetBuilder::new();
            builder.add_plain_text_syntax();
            builder.add(
                syntect::parsing::SyntaxDefinition::load_from_str(
                    include_str!("../assets/syntaxes/TOML.sublime-syntax"),
                    true,
                    None,
                )
                .expect("bundled TOML grammar"),
            );
            builder.build()
        })
    } else {
        DEFAULTS.get_or_init(SyntaxSet::load_defaults_newlines)
    }
}

pub(crate) fn themes() -> &'static ThemeSet {
    static T: OnceLock<ThemeSet> = OnceLock::new();
    T.get_or_init(ThemeSet::load_defaults)
}

fn to_render_image(img: image::RgbaImage) -> Arc<RenderImage> {
    let (w, h) = (img.width(), img.height());
    let mut buf = img.into_raw();
    for p in buf.as_chunks_mut::<4>().0 {
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
        document: None,
    })
}

fn highlight(path: &Path, bytes: &[u8], dark: bool) -> PreviewContent {
    let truncated = bytes.len() >= TEXT_LIMIT;
    let text = String::from_utf8_lossy(bytes);
    let ss = syntaxes(path.extension().and_then(|extension| extension.to_str()));
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
    pub fn preview_menu(
        &mut self,
        args: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let kind = if args.get("kind").and_then(serde_json::Value::as_str) == Some("zoom") {
            "zoom"
        } else {
            "preview"
        };
        let previous = self.preview.menu.take();
        let was_open = previous.as_ref().is_some_and(|p| p.kind == kind);
        if let Some(popup) = previous {
            popup.restore(window);
        }
        if !was_open && args.get("close").and_then(serde_json::Value::as_bool) != Some(true) {
            self.preview.menu = Some(crate::toolbar::Popup::new(kind, window, cx));
        }
        cx.notify();
    }

    /// Called on UI wake. Only a changed request schedules worker work.
    pub fn sync_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !(self.preview.open || self.preview.peek) {
            if self.preview.key.is_some() {
                self.cancel_preview(window, cx);
            }
            while self.preview.rx.try_recv().is_ok() {}
            return;
        }
        let Some(id) = self.cursor_id() else {
            self.cancel_preview(window, cx);
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
            self.cancel_preview(window, cx);
            return;
        };
        // Invalidate before draining results: an old result must never land on
        // a newly selected file, a changed file, or a different pane.
        if self.preview.key.as_ref() != Some(&key) {
            self.cancel_preview(window, cx);
            self.preview.key = Some(key.clone());
            self.preview.phase = PreviewPhase::Queued;
            let mime = if entry.is_dir() {
                Some("inode/directory".into())
            } else {
                bibli_platform::mime::by_name(&entry.name).map(|m| {
                    // Ask for the type's name now; the header reads it later.
                    self.type_names.get(&m);
                    m.to_string()
                })
            };
            let app = Arc::clone(&self.app);
            self.type_names.request(&app);
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
                if result.complete {
                    self.preview.reader.tried = true;
                    if let Some(session) = result.media {
                        self.preview.media.session = Some(session);
                        let focus = cx.focus_handle();
                        if self.preview.peek
                            && self
                                .preview
                                .peek_focus
                                .as_ref()
                                .is_some_and(|f| f.contains_focused(window, cx))
                        {
                            window.focus(&focus);
                        }
                        self.preview.media.focus = Some(focus);
                    }
                    if let Some(session) = result.session {
                        self.preview.reader.session = Some(session);
                        self.preview
                            .reader
                            .focus
                            .get_or_insert_with(|| cx.focus_handle());
                    }
                    self.preview.reader.selection = None;
                    self.preview.reader.selection_page = None;
                    self.preview.reader.down = None;
                }
                if let PreviewContent::Image {
                    detail: Some(detail),
                    ..
                } = &result.content
                {
                    match detail {
                        PreviewInfo::Document { page, .. } => {
                            self.preview.page = *page;
                            if std::mem::take(&mut self.preview.reader.scroll.bottom_on_load) {
                                self.preview.reader.scroll.single.scroll_to_bottom();
                            }
                        }
                        PreviewInfo::Video { seconds, .. } => self.preview.seconds = Some(*seconds),
                    }
                }
                if let PreviewContent::Error(error) = &result.content
                    && let Some(PreviewContent::Image {
                        detail: Some(PreviewInfo::Document { page, .. }),
                        ..
                    }) = &self.preview.content
                {
                    // Keep the last usable page and its controls on a failed turn.
                    self.preview.page = *page;
                    self.preview.phase = PreviewPhase::Idle;
                    self.set_status(error.clone(), true, cx);
                    continue;
                }
                // Opening a player must not replace its thumbnail with a generic
                // card while the initial paused pipeline is warming up.
                let poster = matches!(
                    &result.content,
                    PreviewContent::Image {
                        detail: Some(PreviewInfo::Video { .. }),
                        ..
                    }
                ) && self.preview.media.accept_poster();
                if self.preview.media.session.is_none() || poster {
                    self.replace_preview_content(Some(result.content), window, cx);
                }
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
        let inspection = self.preview.inspection.request.clone();
        let tx = self.preview.tx.clone();
        let previews = Arc::clone(&self.app.previews);
        let wake = self.app.waker_handle();
        let is_dir = entry.is_dir();
        let session = self.preview.reader.session.clone();
        let open_document = !is_dir && !self.preview.reader.tried;
        let lifetime = self.preview.reader.lifetime.clone();
        // A desktop thumbnail represents the file, not the requested page or
        // timestamp. Only use it before this file has any preview content.
        let show_thumbnail = !is_dir && self.preview.content.is_none();
        self.app
            .scheduler
            .spawn(Priority::Visible, "preview", cancel.clone(), move || {
                if !is_dir {
                    let result =
                        previews
                            .inspect(&request, &inspection, &cancel)
                            .and_then(|result| {
                                if result.is_none() && inspection.source {
                                    bibli_platform::inspection::source(
                                        &key.path,
                                        inspection.offset,
                                        &cancel,
                                    )
                                    .map(Some)
                                } else {
                                    Ok(result)
                                }
                            });
                    match result {
                        Ok(Some(data)) => {
                            if !cancel.is_cancelled() {
                                let _ = tx.send(PreviewResult {
                                    generation,
                                    content: PreviewContent::Inspection(
                                        crate::inspector::Display::new(data, key.dark),
                                    ),
                                    complete: true,
                                    session: None,
                                    media: None,
                                });
                                wake();
                            }
                            return;
                        }
                        Err(error) => {
                            if !cancel.is_cancelled() {
                                let _ = tx.send(PreviewResult {
                                    generation,
                                    content: PreviewContent::Error(error.to_string()),
                                    complete: true,
                                    session: None,
                                    media: None,
                                });
                                wake();
                            }
                            return;
                        }
                        Ok(None) => {}
                    }
                }
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
                            session: None,
                            media: None,
                            content: PreviewContent::Image {
                                image: render,
                                width: image.width,
                                height: image.height,
                                detail: None,
                                document: None,
                            },
                        });
                        wake();
                    }
                }
                // Provider registration owns media recognition, including container MIME types.
                match previews.open_media(&request, &lifetime, wake.clone()) {
                    Ok(Some(media)) => {
                        if !cancel.is_cancelled() {
                            let _ = tx.send(PreviewResult {
                                generation,
                                content: PreviewContent::Generic,
                                complete: true,
                                session: None,
                                media: Some(media),
                            });
                            wake();
                        }
                        // Keep startup/playback independent of optional poster
                        // extraction. The same cancellation token stops FFmpeg
                        // when Play/Seek, selection or provider changes supersede it.
                        let mut poster = request.clone();
                        poster.max_side = u32::MAX;
                        if let Ok(Some(preview)) = previews.render(&poster, &cancel)
                            && let Some(image) = crate::thumbs::to_render_image(&preview.image)
                            && !cancel.is_cancelled()
                        {
                            let _ = tx.send(PreviewResult {
                                generation,
                                content: PreviewContent::Image {
                                    image,
                                    width: preview.image.width,
                                    height: preview.image.height,
                                    detail: Some(preview.info),
                                    document: None,
                                },
                                complete: true,
                                session: None,
                                media: None,
                            });
                            wake();
                        }
                        return;
                    }
                    Err(error) => {
                        if !cancel.is_cancelled() {
                            let _ = tx.send(PreviewResult {
                                generation,
                                content: PreviewContent::Error(error.to_string()),
                                complete: true,
                                session: None,
                                media: None,
                            });
                            wake();
                        }
                        return;
                    }
                    Ok(None) => {}
                }
                let reader = if let Some(session) = session {
                    Ok(Some(session))
                } else if open_document {
                    previews.open_document(&request, &lifetime)
                } else {
                    Ok(None)
                };
                match reader {
                    Ok(Some(session)) => {
                        let content = session
                            .render(request.page, request.max_side, &cancel)
                            .and_then(|page| {
                                let image = crate::thumbs::owned_render_image(
                                    page.width,
                                    page.height,
                                    page.rgba,
                                )
                                .ok_or_else(|| anyhow::anyhow!("invalid document bitmap"))?;
                                Ok(PreviewContent::Image {
                                    image,
                                    width: page.width,
                                    height: page.height,
                                    detail: Some(PreviewInfo::Document {
                                        page: page.content.page,
                                        pages: session.info().pages,
                                    }),
                                    document: Some(page.content),
                                })
                            })
                            .unwrap_or_else(|e| PreviewContent::Error(format!("{e:#}")));
                        if !cancel.is_cancelled() {
                            let _ = tx.send(PreviewResult {
                                generation,
                                content,
                                complete: true,
                                session: Some(session),
                                media: None,
                            });
                            wake();
                        }
                        return;
                    }
                    Err(error) => {
                        if !cancel.is_cancelled() {
                            let _ = tx.send(PreviewResult {
                                generation,
                                content: PreviewContent::Error(format!("{error:#}")),
                                complete: true,
                                session: None,
                                media: None,
                            });
                            wake();
                        }
                        return;
                    }
                    Ok(None) => {}
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
                            document: None,
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
                        session: None,
                        media: None,
                    });
                    wake();
                }
            });
    }

    pub(crate) fn cancel_pending_preview(&mut self) {
        if let Some(cancel) = self.preview.cancel.take() {
            cancel.cancel();
        }
        self.preview.generation = self.preview.generation.wrapping_add(1);
    }

    pub(crate) fn replace_preview_content(
        &mut self,
        content: Option<PreviewContent>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // RenderImage's Arc does not own its uploaded atlas entry. Evict it
        // explicitly, just like the thumbnail LRU, on replacement and close.
        match self.preview.content.take() {
            Some(PreviewContent::Image { image, .. }) => {
                cx.drop_image(image, Some(window));
            }
            Some(PreviewContent::Inspection(display)) => {
                for image in display.images.into_iter().flatten() {
                    cx.drop_image(image, Some(window));
                }
            }
            _ => {}
        }
        if let Some(PreviewContent::Inspection(display)) = &content {
            self.preview.inspection.arrived(display);
            self.preview
                .inspection
                .focus
                .get_or_insert_with(|| cx.focus_handle());
        }
        self.preview.content = content;
    }

    fn cancel_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel_pending_preview();
        if let Some(menu) = self.preview.menu.take()
            && menu.focus.contains_focused(window, cx)
        {
            // The old preview's trigger is about to disappear. Keep an
            // existing address/pane focus, or return popup focus to the pane.
            window.focus(&self.ui().focus);
        }
        self.preview.key = None;
        self.preview.info = Info::default();
        while self.preview.rx.try_recv().is_ok() {}
        self.replace_preview_content(None, window, cx);
        self.media_exit_fullscreen(window, cx);
        self.preview.media = crate::media::MediaState::default();
        self.clear_document_pages(window, cx);
        self.preview.reader = crate::reader::ReaderState::default();
        self.preview.inspection = crate::inspector::State::default();
        self.preview.phase = PreviewPhase::Idle;

        self.preview.page = 1;
        self.preview.seconds = None;
        self.preview.zoom = None;
        self.preview.fit_width = false;
        self.preview
            .text_scroll
            .scroll_to_item_strict(0, ScrollStrategy::Top);
    }

    pub fn preview_page(&mut self, delta: i32, cx: &mut Context<Self>) {
        if let Some(PreviewContent::Inspection(display)) = &self.preview.content {
            if let Some(offset) = if delta < 0 {
                display.data.previous
            } else {
                display.data.next
            } {
                self.preview.inspection.request.offset = offset;
                self.preview.inspection.request.target = None;
                self.preview.inspection.request.query = None;
                self.cancel_pending_preview();
                self.preview.phase = PreviewPhase::Queued;
                cx.notify();
            }
            return;
        }
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
        self.preview_go_page(next, cx);
    }

    pub(crate) fn reader_mode_changed(&mut self) {
        self.cancel_pending_preview();
        let loaded = matches!(&self.preview.content, Some(PreviewContent::Image { document: Some(page), .. }) if page.page == self.preview.page);
        self.preview.phase = if self.preview.reader.scroll.continuous || loaded {
            PreviewPhase::Idle
        } else {
            PreviewPhase::Queued
        };
    }

    pub fn preview_go_page(&mut self, next: u32, cx: &mut Context<Self>) {
        let pages = self
            .preview
            .reader
            .session
            .as_ref()
            .map(|s| s.info().pages)
            .or(match &self.preview.content {
                Some(PreviewContent::Image {
                    detail: Some(PreviewInfo::Document { pages, .. }),
                    ..
                }) => Some(*pages),
                _ => None,
            });
        let Some(pages) = pages else {
            return;
        };
        let next = next.clamp(1, pages);
        if self.preview.reader.scroll.continuous {
            self.preview.reader.scroll.list.scroll_to(gpui::ListOffset {
                item_ix: next as usize - 1,
                offset_in_item: px(0.),
            });
            self.preview.page = next;
            self.preview.reader.page_input = None;
            // An explicit visit retries a page whose render failed earlier.
            self.reader_retry_page(next);
            cx.notify();
            return;
        }
        if next != self.preview.page {
            self.preview.reader.page_input = None;
            self.preview.reader.cancel_search();
            self.cancel_pending_preview();
            self.preview
                .reader
                .scroll
                .single
                .set_offset(gpui::point(px(0.), px(0.)));
            self.preview.page = next;
            self.preview.phase = PreviewPhase::Queued;
            cx.notify();
        }
    }

    pub fn preview_seek(&mut self, delta: f64, cx: &mut Context<Self>) {
        if self.preview.media.session.is_some() {
            // Same gate as the seek bar: a non-seekable stream ignores keys.
            if self.preview.media.state.seekable {
                self.media_send(
                    bibli_core::media::Control::Seek(
                        (self.preview.media.state.position + delta).max(0.),
                    ),
                    cx,
                );
            }
            return;
        }
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

    pub fn preview_open_external(&mut self, _cx: &mut Context<Self>) {
        let Some(key) = &self.preview.key else {
            return;
        };
        let entry = EntryRef::new(key.id, &key.path);
        let app = self.app.clone();
        self.app.scheduler.spawn(
            Priority::Visible,
            "preview-open",
            self.app.lifetime().child(),
            move || {
                if let Err(error) = entry
                    .resolve()
                    .map_err(anyhow::Error::from)
                    .and_then(|p| bibli_platform::open::open_path(&p))
                {
                    app.notify(bibli_core::app::NotifyLevel::Error, error.to_string());
                }
            },
        );
    }

    pub fn preview_zoom(&mut self, direction: &str, cx: &mut Context<Self>) {
        self.preview.fit_width = direction == "width";
        self.preview.zoom = match direction {
            "in" => Some(self.preview.zoom.map_or(1.0, |z| (z * 1.25).min(4.0))),
            "out" => Some(self.preview.zoom.map_or(0.75, |z| (z / 1.25).max(0.25))),
            value => value
                .parse::<f32>()
                .ok()
                .filter(|v| v.is_finite())
                .map(|v| v.clamp(0.25, 4.0)),
        };
        cx.notify();
    }

    pub fn toggle_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.preview.open = !self.preview.open;
        if !self.preview.open && !self.preview.peek {
            self.preview.menu = None;
            window.focus(&self.ui().focus);
        }
        cx.notify();
    }

    pub fn open_peek(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.cursor_id().is_none() {
            self.set_status("nothing to peek at", false, cx);
            return;
        }
        self.preview.menu = None;
        let focus = cx.focus_handle();
        window.focus(self.preview.media.focus.as_ref().unwrap_or(&focus));
        self.preview.peek_focus = Some(focus);
        self.preview.peek = true;
        cx.notify();
    }

    pub fn close_peek(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.media_exit_fullscreen(window, cx);
        if let Some(session) = &self.preview.media.session {
            let _ = session.control(bibli_core::media::Control::Play(false));
        }
        self.preview.menu = None;
        self.preview.peek = false;
        self.preview.peek_focus = None;
        window.focus(&self.ui().focus);
        cx.notify();
    }

    pub fn video_thumbnail(&self) -> bibli_core::thumbs::ThumbState {
        use bibli_core::thumbs::{ThumbRequest, ThumbSize, ThumbState};
        let Some(key) = &self.preview.key else {
            return ThumbState::Unavailable;
        };
        self.app.thumbnails.get(
            ThumbRequest {
                entry: EntryRef::new(key.id, &key.path),
                mime: self.preview.info.mime.clone().map(Into::into),
                mtime: key.modified,
                size: ThumbSize::Large,
            },
            Priority::Visible,
        )
    }

    pub fn preview_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.preview.peek {
            self.close_peek(window, cx);
        } else {
            self.preview.open = false;
            self.preview.menu = None;
            window.focus(&self.ui().focus);
            cx.notify();
        }
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
            .on_drag_move(cx.listener(Self::on_column_drag))
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(|this, _, _, _| this.finish_column_drag()),
            )
    }
}

fn body(shell: &Shell, theme: &Theme, interactive: bool, cx: &mut Context<Shell>) -> AnyElement {
    let document = matches!(
        &shell.preview.content,
        Some(PreviewContent::Image {
            detail: Some(PreviewInfo::Document { .. }),
            ..
        })
    );
    let mut frame = div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .when(document && interactive, |d| d.key_context(keymap::READER));
    if interactive && !shell.preview.info.name.is_empty() {
        let controls = if matches!(shell.preview.content, Some(PreviewContent::Inspection(_))) {
            crate::inspector::toolbar(shell, theme, cx)
        } else if shell.preview.media.session.is_some() {
            crate::media::toolbar(shell, theme, cx)
        } else if document {
            crate::reader::toolbar(shell, theme, cx)
        } else {
            crate::preview_toolbar::render(shell, theme, cx)
        };
        frame = frame.child(div().relative().flex_none().child(controls).when(
            shell.preview.phase != PreviewPhase::Idle,
            |d| {
                d.child(
                    div()
                        .absolute()
                        .bottom_0()
                        .w_full()
                        .h(px(2.))
                        .bg(theme.accent),
                )
            },
        ));
    }
    frame
        .when_some(
            shell.preview.media.focus.as_ref().filter(|_| interactive),
            |d, focus| d.track_focus(focus).key_context("MediaPlayer"),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .when_some(
                    shell.preview.media.focus.as_ref().filter(|_| interactive),
                    |d, focus| {
                        let focus = focus.clone();
                        d.on_mouse_down(MouseButton::Left, move |_, window, _| window.focus(&focus))
                    },
                )
                .child(content(shell, theme, interactive, cx)),
        )
        .into_any_element()
}

fn content(shell: &Shell, theme: &Theme, interactive: bool, cx: &mut Context<Shell>) -> AnyElement {
    let p = &shell.preview;
    if p.media.session.is_some() && shell.media_is_audio() {
        return crate::media::audio_content(shell, theme);
    }
    match &p.content {
        Some(PreviewContent::Inspection(display)) => {
            crate::inspector::content(shell, display, theme, cx)
        }
        None if p.phase != PreviewPhase::Idle => div()
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .text_color(theme.text_muted)
            .child("Loading…")
            .into_any_element(),
        None | Some(PreviewContent::Generic) => generic(shell, &p.info, theme),
        Some(PreviewContent::Image {
            image,
            width,
            height,
            document,
            ..
        }) => {
            let picture = if let Some(document) = document.as_ref().filter(|_| interactive) {
                crate::reader::page_element(
                    shell,
                    Arc::clone(image),
                    Arc::clone(document),
                    p.zoom,
                    cx,
                )
            } else if let Some(zoom) = p.zoom {
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
            } else if p.fit_width {
                div()
                    .id("preview-image-scroll")
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_scroll()
                    .child(
                        // GPUI's img resolves an auto height to the source's
                        // pixel height when width is relative. Let the parent
                        // aspect ratio size the image instead.
                        div()
                            .relative()
                            .w_full()
                            .map(|mut d| {
                                d.style().aspect_ratio =
                                    Some(*width as f32 / (*height).max(1) as f32);
                                d
                            })
                            .child(img(Arc::clone(image)).absolute().size_full()),
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
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .when(document.is_some() && interactive, |d| {
                    d.key_context(keymap::READER)
                })
                .child(
                    div()
                        .relative()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_h_0()
                        .child(picture)
                        .when(document.is_some() && interactive, |d| {
                            d.children(crate::reader::outline_drawer(shell, theme, cx))
                        }),
                )
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
            .child(format::count(*items, "item"))
            .into_any_element(),
        Some(PreviewContent::Error(e)) => div()
            .flex_1()
            .p_2()
            .text_color(theme.error)
            .child(e.clone())
            .into_any_element(),
    }
}

fn generic(shell: &Shell, info: &Info, theme: &Theme) -> AnyElement {
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
    if let Some(kind) = type_label(shell, info) {
        card = card.child(kind);
    }
    if let Some(s) = &info.size {
        card = card.child(s.clone());
    }
    if let Some(m) = &info.modified {
        card = card.child(m.clone());
    }
    card.into_any_element()
}

fn header(shell: &Shell, theme: &Theme, interactive: bool, cx: &mut Context<Shell>) -> AnyElement {
    let info = &shell.preview.info;
    div()
        .px_2()
        .py_1()
        .border_b_1()
        .border_color(theme.border)
        .flex()
        .flex_col()
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .child(div().min_w_0().flex_1().truncate().child(info.name.clone()))
                .when(interactive, |d| {
                    d.child(
                        crate::toolbar::Button::new(
                            "preview-close",
                            "close",
                            "Close preview",
                            "ui.preview_close",
                        )
                        .render(shell, cx),
                    )
                }),
        )
        .child(
            div()
                .id("preview-meta")
                .text_xs()
                .text_color(theme.text_muted)
                .truncate()
                .when_some(info.mime.clone(), |d, mime| {
                    let font = shell.font.clone();
                    d.tooltip(move |_, cx| {
                        crate::toolbar::tooltip(mime.clone().into(), font.clone(), cx)
                    })
                })
                .child(
                    [
                        type_label(shell, info),
                        info.size.clone(),
                        info.modified.clone(),
                    ]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" · "),
                ),
        )
        .into_any_element()
}

/// "PDF document" rather than `application/pdf`; the raw type is the tooltip.
fn type_label(shell: &Shell, info: &Info) -> Option<String> {
    if info.is_dir {
        return Some("Folder".into());
    }
    let mime = info.mime.as_deref()?;
    Some(shell.type_names.peek(mime).map_or_else(
        || crate::typenames::fallback(&info.name),
        |name| name.to_string(),
    ))
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
            .id("preview-panel")
            .relative()
            .w(px(shell.preview.width))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(theme.border)
            .bg(theme.surface)
            .child(header(shell, &theme, !shell.preview.peek, cx))
            .child(body(shell, &theme, !shell.preview.peek, cx))
            .child(handle)
            .into_any_element(),
    )
}

/// The centered peek overlay, or `None`.
pub fn render_peek(shell: &Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    if !shell.preview.peek {
        return None;
    }
    if shell.preview.media.fullscreen.is_some() {
        return Some(crate::media::fullscreen(shell, cx));
    }
    let theme = cx.global::<Theme>().clone();
    let focus = shell.preview.peek_focus.clone()?;
    Some(
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
                    .id("preview-peek")
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
                    .child(header(shell, &theme, true, cx))
                    .child(body(shell, &theme, true, cx))
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .text_xs()
                            .text_color(theme.text_muted)
                            .border_t_1()
                            .border_color(theme.border)
                            .child(if shell.preview.media.session.is_some() {
                                "Space play/pause · arrows seek · M mute · F fullscreen · Escape exit"
                            } else {
                                "Space or Escape to close · arrows to move"
                            }),
                    ),
            )
            .into_any_element(),
    )
}
