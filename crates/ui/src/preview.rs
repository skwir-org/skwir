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
    AnyElement, Context, CursorStyle, DragMoveEvent, FocusHandle, MouseButton, ObjectFit, Render,
    RenderImage, Window, div, img, prelude::*, px,
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
const IMAGE_MAX_SIDE: u32 = 2048;

pub enum PreviewContent {
    Inspection(crate::inspector::Display),
    Image {
        image: Arc<RenderImage>,
        width: u32,
        height: u32,
        detail: Option<PreviewInfo>,
        document: Option<Arc<bibli_core::document::PageContent>>,
        /// What the file says about the picture (EXIF), read on the worker
        /// that decoded it, for the panel's Details.
        facts: Vec<bibli_core::inspection::Detail>,
    },
    Dir {
        items: usize,
        folders: usize,
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

/// How deep the panel follows containers: a file in an archive in an archive.
pub const NESTING: usize = 2;

/// A member of a container, extracted so that it is previewed as the file it
/// is. The directory is private to this preview and goes with it; it is only
/// ever dropped on a worker (`Shell::preview_ascend`), never while painting.
pub struct Nested {
    /// The member's own name, for the header's trail: a label, not a path.
    pub name: String,
    entry: EntryRef,
    mime: Option<String>,
    path: std::path::PathBuf,
    _directory: tempfile::TempDir,
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

// Independent switches of one panel, not a state machine in disguise.
#[allow(clippy::struct_excessive_bools)]
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
    pub peek_focus: Option<FocusHandle>,
    /// The audio files before and after this one in the pane's order, as
    /// indices of its sorted list; found when the listing or the entry
    /// changes (`details_at`), never while painting.
    pub audio_neighbours: (Option<usize>, Option<usize>),
    /// The next media session starts playing: the track before it ended, or
    /// the user stepped to it while listening.
    pub autoplay: bool,
    /// The members the panel went down into, outermost first.
    pub nested: Vec<Arc<Nested>>,
    /// A member asked for and not extracted yet: the worker's next job.
    pub descend: Option<String>,
    nested_tx: flume::Sender<(u64, Arc<Nested>)>,
    nested_rx: flume::Receiver<(u64, Arc<Nested>)>,
    /// What the entry says about itself, collected when it or its listing
    /// changes (`details_at`), never while painting.
    pub details: Vec<crate::details::Group>,
    details_at: Option<u64>,
    /// Several entries selected: how many, and what they weigh together.
    pub selection_summary: Option<crate::details::Group>,
    /// Whether Details is open under a preview; part of the window's shape.
    pub details_open: bool,
    pub details_scroll: gpui::ScrollHandle,
    /// Focus of a picture, so the zoom keys reach it once it is clicked.
    pub image_focus: Option<FocusHandle>,
    /// Whether the hexadecimal provider is registered, keyed by the preview
    /// registry revision it was read at (see `preview_toolbar::hex_available`).
    pub(crate) hex_provider: std::cell::Cell<Option<(u64, bool)>>,
}

impl Default for PreviewState {
    fn default() -> Self {
        let (tx, rx) = flume::unbounded();
        let (nested_tx, nested_rx) = flume::unbounded();
        PreviewState {
            reader: crate::reader::ReaderState::default(),
            inspection: crate::inspector::State::default(),
            media: crate::media::MediaState::default(),
            menu: None,
            open: false,
            peek: false,
            width: DEFAULT_WIDTH,
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
            peek_focus: None,
            audio_neighbours: (None, None),
            autoplay: false,
            nested: Vec::new(),
            descend: None,
            nested_tx,
            nested_rx,
            details: Vec::new(),
            details_at: None,
            selection_summary: None,
            details_open: false,
            details_scroll: gpui::ScrollHandle::new(),
            image_focus: None,
            hex_provider: std::cell::Cell::new(None),
        }
    }
}

impl PreviewState {
    /// The file the panel is showing, when it is a file on disk. Links inside
    /// a document are relative to it.
    pub fn path(&self) -> Option<&Path> {
        self.key.as_ref().map(|key| key.path.as_path())
    }

    /// The pane whose entry the panel is showing.
    pub fn pane(&self) -> Option<bibli_core::workspace::PaneId> {
        self.key.as_ref().map(|key| key.pane)
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
        facts: bibli_platform::exif::details(path),
    })
}

/// Where this process keeps the members it extracted: in the one deletable
/// cache, under its own pid, so that what a killed process left behind can be
/// told from what a running one still shows.
fn members_dir(cache: &Path) -> PathBuf {
    cache
        .join("preview-members")
        .join(std::process::id().to_string())
}

/// Remove what processes that no longer run left in `preview-members`.
/// Filesystem work: call it from a worker.
pub fn sweep_members(cache: &Path) {
    let Ok(entries) = std::fs::read_dir(cache.join("preview-members")) else {
        return;
    };
    for entry in entries.flatten() {
        let alive = entry
            .file_name()
            .to_str()
            .and_then(|pid| pid.parse::<u32>().ok())
            .is_some_and(|pid| Path::new("/proc").join(pid.to_string()).exists());
        if !alive {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// Have the container write `target` out as a file of its own, in a private
/// directory that lives as long as the panel shows it.
fn extract_member(
    previews: &bibli_core::preview::Previews,
    request: &PreviewRequest,
    target: &str,
    members: &Path,
    cancel: &CancelToken,
) -> anyhow::Result<Option<Arc<Nested>>> {
    std::fs::create_dir_all(members)?;
    let directory = tempfile::Builder::new()
        .prefix("member-")
        .tempdir_in(members)?;
    let Some(member) = previews.extract(request, target, directory.path(), cancel)? else {
        return Ok(None);
    };
    anyhow::ensure!(
        member.path.starts_with(directory.path()),
        "A member was written outside its directory"
    );
    let entry = EntryRef::from_path(&member.path)
        .ok_or_else(|| anyhow::anyhow!("The extracted member is missing"))?;
    Ok(Some(Arc::new(Nested {
        mime: bibli_platform::mime::by_name(std::ffi::OsStr::new(&member.name))
            .map(|mime| mime.to_string()),
        name: member.name,
        entry,
        path: member.path,
        _directory: directory,
    })))
}

fn compute(
    path: &Path,
    is_dir: bool,
    mime: Option<&str>,
    dark: bool,
    offset: u64,
    cancel: &CancelToken,
) -> PreviewContent {
    if is_dir {
        return match std::fs::read_dir(path) {
            Ok(entries) => {
                let (mut items, mut folders) = (0, 0);
                for entry in entries.flatten() {
                    if cancel.is_cancelled() {
                        break;
                    }
                    items += 1;
                    folders += usize::from(entry.file_type().is_ok_and(|kind| kind.is_dir()));
                }
                PreviewContent::Dir { items, folders }
            }
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
        // Every text file is read the way structured code is: a gutter,
        // selection, find and copy. There is no second, poorer text view.
        return match bibli_platform::inspection::plain(path, offset, cancel) {
            Ok(data) => PreviewContent::Inspection(crate::inspector::Display::new(data, dark)),
            Err(error) => PreviewContent::Error(format!("{error:#}")),
        };
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
                // What Details was collected for: the listing as it is, and
                // how much of it is selected.
                let seen = pane
                    .listing
                    .version()
                    .wrapping_mul(31)
                    .wrapping_add(pane.selection.len() as u64)
                    .wrapping_mul(2)
                    .wrapping_add(u64::from(bibli_platform::users::loaded()));
                Some((
                    seen,
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
        let Some((seen, key, entry)) = current else {
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
            self.preview.details_at = None;
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
        if self.preview.details_at != Some(seen) {
            self.preview.details_at = Some(seen);
            let kind = type_label(self, &self.preview.info).map(|label| {
                match self
                    .preview
                    .info
                    .mime
                    .as_deref()
                    .filter(|_| !entry.is_dir())
                {
                    Some(mime) => format!("{label} ({mime})"),
                    None => label,
                }
            });
            self.preview.details = crate::details::entry_groups(&entry, &key.path, kind);
            self.preview.audio_neighbours = if self.media_is_audio() {
                self.audio_neighbours(id)
            } else {
                (None, None)
            };
            let selection = self.selection_ctx();
            self.preview.selection_summary = (selection.count > 1).then(|| crate::details::Group {
                title: "Selection".into(),
                rows: vec![
                    crate::details::Row::new("Selected", format::count(selection.count, "item")),
                    crate::details::Row::new("Total size", format::size(selection.bytes)),
                ],
            });
        }
        while let Ok((generation, member)) = self.preview.nested_rx.try_recv() {
            if generation == self.preview.generation && self.preview.nested.len() < NESTING {
                self.preview.nested.push(member);
                self.preview.descend = None;
            } else {
                self.discard_nested(vec![member]);
            }
        }
        while let Ok(result) = self.preview.rx.try_recv() {
            if result.generation == self.preview.generation {
                if result.complete {
                    self.preview.reader.tried = true;
                    if let Some(session) = result.media {
                        if std::mem::take(&mut self.preview.autoplay) {
                            let _ = session.control(bibli_core::media::Control::Play(true));
                        }
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
                        // A document opens the way the last one was read.
                        let continuous = self
                            .app
                            .settings
                            .get::<bool>(crate::settings::READER_CONTINUOUS_KEY)
                            .unwrap_or(true);
                        self.set_reader_continuous(continuous, window, cx);
                    }
                    if matches!(result.content, PreviewContent::Image { .. }) {
                        self.preview
                            .image_focus
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
        let mut inspection = self.preview.inspection.request.clone();
        let tx = self.preview.tx.clone();
        let nested = self.preview.nested.last().cloned();
        let descend = self.preview.descend.clone();
        let nested_tx = self.preview.nested_tx.clone();
        let members = members_dir(self.app.cache.root());
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
                // What is previewed: the entry, or the member of it the panel
                // went down into. Everything below treats it as any file.
                let mut request = request;
                let mut path = key.path.clone();
                let source = |member: &Nested, request: &mut PreviewRequest, path: &mut PathBuf| {
                    request.entry = member.entry.clone();
                    request.mime.clone_from(&member.mime);
                    path.clone_from(&member.path);
                };
                if let Some(member) = &nested {
                    source(member, &mut request, &mut path);
                }
                if let Some(target) = descend {
                    match extract_member(&previews, &request, &target, &members, &cancel) {
                        Ok(Some(member)) => {
                            source(&member, &mut request, &mut path);
                            let _ = nested_tx.send((generation, member));
                        }
                        // Not a file here: the provider shows the target itself.
                        Ok(None) => inspection.target = Some(target),
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
                    }
                }
                if !is_dir {
                    let result =
                        previews
                            .inspect(&request, &inspection, &cancel)
                            .and_then(|result| {
                                if result.is_none() && inspection.source {
                                    bibli_platform::inspection::source(
                                        &path,
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
                                facts: Vec::new(),
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
                                    facts: Vec::new(),
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
                                    facts: Vec::new(),
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
                            facts: Vec::new(),
                        },
                        None => PreviewContent::Error("Invalid preview image".into()),
                    },
                    Ok(None) => compute(
                        &path,
                        is_dir,
                        request.mime.as_deref(),
                        key.dark,
                        inspection.offset,
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
        self.preview_ascend(0);
        self.preview.phase = PreviewPhase::Idle;

        self.preview.page = 1;
        self.preview.seconds = None;
        self.preview.zoom = None;
        self.preview.fit_width = false;
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

    /// Go back up to `depth` members deep; 0 is the entry itself.
    pub(crate) fn preview_ascend(&mut self, depth: usize) {
        let depth = depth.min(self.preview.nested.len());
        let left = self.preview.nested.split_off(depth);
        self.discard_nested(left);
        self.preview.descend = None;
    }

    /// Removing an extracted member is filesystem work: hand it to a worker.
    fn discard_nested(&mut self, members: Vec<Arc<Nested>>) {
        if !members.is_empty() {
            self.app.scheduler.spawn(
                Priority::Background,
                "preview-cleanup",
                self.app.lifetime().child(),
                move || drop(members),
            );
        }
    }

    /// `ui.preview_details`: open or close Details under the preview.
    pub fn preview_details(&mut self, cx: &mut Context<Self>) {
        self.preview.details_open = !self.preview.details_open;
        self.remember_session();
        cx.notify();
    }

    /// `ui.preview_copy_path`: the path of what is shown, as text.
    pub fn preview_copy_path(&mut self, cx: &mut Context<Self>) {
        let Some(key) = &self.preview.key else {
            return;
        };
        let path = key.path.to_string_lossy().into_owned();
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(path));
        self.set_status("Copied the path", false, cx);
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
        self.remember_session();
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
            self.remember_session();
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

    /// The widest the panel may be in this window: everything the panes and
    /// the sidebar do not need. The window's width is known before layout.
    pub fn preview_width_limit(&self, window: &Window) -> f32 {
        let window = f32::from(window.viewport_size().width);
        max_width(window, self.panes.len(), self.sidebar_fits(window))
    }

    /// Whether this window has room for the sidebar beside the panes.
    pub fn sidebar_fits(&self, window: f32) -> bool {
        window >= 520. + 300. * self.panes.len().saturating_sub(1) as f32
    }

    fn on_resize_drag(
        &mut self,
        ev: &DragMoveEvent<ResizeHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let total = f32::from(window.viewport_size().width);
        let x = f32::from(ev.event.position.x);
        self.preview.width = (total - x).clamp(MIN_WIDTH, self.preview_width_limit(window));
        cx.notify();
    }

    /// Root-level hook so edge drags are tracked anywhere in the window.
    pub fn resize_hook<E: InteractiveElement>(&self, el: E, cx: &mut Context<Self>) -> E {
        el.on_drag_move(cx.listener(Self::on_resize_drag))
            .on_drag_move(cx.listener(Self::on_column_drag))
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(|this, _, _, _| {
                    this.finish_column_drag();
                    // The panel's edge has no drag-end of its own; this is it.
                    this.remember_session();
                }),
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
                .when(!crate::details::alone(shell), gpui::Styled::flex_1)
                .min_h_0()
                // What is read is on paper; a picture, a page or a film keeps
                // the panel's own surface as its letterbox.
                .when(on_paper(shell), |d| d.bg(theme.bg))
                .when(interactive, |d| {
                    d.on_mouse_down(
                        MouseButton::Right,
                        cx.listener(|this, event: &gpui::MouseDownEvent, window, cx| {
                            this.open_preview_menu(event.position, window, cx);
                        }),
                    )
                })
                .when_some(
                    shell.preview.media.focus.as_ref().filter(|_| interactive),
                    |d, focus| {
                        let focus = focus.clone();
                        d.on_mouse_down(MouseButton::Left, move |_, window, _| window.focus(&focus))
                    },
                )
                .child(content(shell, theme, interactive, cx)),
        )
        .children(
            Some(())
                .filter(|()| interactive || crate::details::alone(shell))
                .and_then(|()| crate::details::render(shell, theme, cx)),
        )
        .into_any_element()
}

/// The popup of the preview's bar that is open: More, the zoom menu, or the
/// reader's find field, which is no menu but is anchored like one.
pub fn active_popup(shell: &Shell) -> Option<&'static str> {
    match &shell.preview.menu {
        Some(popup) => Some(popup.kind),
        None => {
            (shell.preview.reader.finding() || shell.preview.inspection.finding()).then_some("find")
        }
    }
}

/// One line under the toolbar for what a preview has to say about itself:
/// how the content is shown, what was left out, why there is none.
fn notice_strip(text: String, theme: &Theme) -> gpui::Div {
    div()
        .flex_none()
        .px_2()
        .py_1()
        .text_xs()
        .text_color(theme.text_muted)
        .bg(theme.surface)
        .border_b_1()
        .border_color(theme.border)
        .child(text)
}

/// Whether the content is something to read rather than something to look at.
fn on_paper(shell: &Shell) -> bool {
    let preview = &shell.preview;
    if preview.media.session.is_some() {
        return shell.media_is_audio();
    }
    !matches!(preview.content, Some(PreviewContent::Image { .. }))
}

fn content(shell: &Shell, theme: &Theme, interactive: bool, cx: &mut Context<Shell>) -> AnyElement {
    let p = &shell.preview;
    if p.media.session.is_some() && shell.media_is_audio() {
        return crate::media::audio_content(shell, theme);
    }
    match &p.content {
        Some(PreviewContent::Inspection(display)) => div()
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .children(crate::inspector::notice(display).map(|text| notice_strip(text, theme)))
            .child(crate::inspector::content(shell, display, theme, cx))
            .children(
                Some(())
                    .filter(|()| interactive)
                    .and_then(|()| crate::inspector::outline_drawer(shell, display, theme, cx)),
            )
            .into_any_element(),
        None if p.phase != PreviewPhase::Idle => div()
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .text_color(theme.text_muted)
            .child("Loading…")
            .into_any_element(),
        None | Some(PreviewContent::Generic | PreviewContent::Dir { .. }) => {
            hero(&p.info, None, theme)
        }
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
                .when_some(
                    p.image_focus
                        .as_ref()
                        .filter(|_| document.is_none() && interactive),
                    |d, focus| {
                        let clicked = focus.clone();
                        d.track_focus(focus)
                            .key_context(keymap::IMAGE)
                            .on_mouse_down(MouseButton::Left, move |_, window, _| {
                                window.focus(&clicked);
                            })
                            .on_scroll_wheel(cx.listener(
                                |this, event: &gpui::ScrollWheelEvent, window, cx| {
                                    if event.modifiers.control {
                                        let up = event.delta.pixel_delta(window.line_height()).y
                                            > px(0.);
                                        this.preview_zoom(if up { "in" } else { "out" }, cx);
                                        cx.stop_propagation();
                                    }
                                },
                            ))
                    },
                )
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
        Some(PreviewContent::Error(error)) => hero(&p.info, Some(error), theme),
    }
}

/// What stands for an entry that has no preview: its icon and, when there
/// should have been one, why there is none. Details says the rest.
fn hero(info: &Info, error: Option<&String>, theme: &Theme) -> AnyElement {
    div()
        .flex_none()
        .flex()
        .flex_col()
        .items_center()
        .gap_2()
        .pt_6()
        .pb_2()
        .px_3()
        .children(info.icon.as_ref().map(|icon| icons::element(icon, 64.)))
        .when_some(error, |d, error| {
            d.child(
                div()
                    .text_xs()
                    .text_center()
                    .text_color(theme.error)
                    .child(error.clone()),
            )
        })
        .into_any_element()
}

/// The name of what is shown. Inside a container it is a trail,
/// `bundle.zip › notes.md`, and every step before the last leads back to it.
fn trail(shell: &Shell, theme: &Theme, cx: &mut Context<Shell>) -> gpui::Div {
    let preview = &shell.preview;
    let mut line = div().min_w_0().flex_1().flex().items_center().gap_1();
    if preview.nested.is_empty() {
        return line.child(div().min_w_0().truncate().child(preview.info.name.clone()));
    }
    let names = std::iter::once(preview.info.name.clone())
        .chain(preview.nested.iter().map(|member| member.name.clone()));
    let last = preview.nested.len();
    for (depth, name) in names.enumerate() {
        if depth > 0 {
            line = line.child(div().flex_none().text_color(theme.text_muted).child("›"));
        }
        line = line.child(if depth == last {
            div().min_w_0().truncate().child(name).into_any_element()
        } else {
            div()
                .id(("preview-trail", depth))
                .min_w_0()
                .flex_shrink()
                .truncate()
                .text_color(theme.text_muted)
                .cursor_pointer()
                .hover(|d| d.text_color(theme.accent))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.run(
                        "ui.inspect",
                        serde_json::json!({"action":"ascend","depth":depth}),
                        window,
                        cx,
                    );
                }))
                .child(name)
                .into_any_element()
        });
    }
    line
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
                .child(trail(shell, theme, cx))
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
/// The narrowest the panel gets, and the width a double click returns it to.
const MIN_WIDTH: f32 = 200.;
const DEFAULT_WIDTH: f32 = 320.;
/// What each pane keeps when the panel is dragged wide, and the sidebar.
const PANE_KEEP: f32 = 240.;
const SIDEBAR_WIDTH: f32 = 200.;

/// The widest the panel may be: the window less what the panes and the
/// sidebar keep. Never a fixed number: a wide window has room for a wide page.
pub fn max_width(window: f32, panes: usize, sidebar: bool) -> f32 {
    let kept = PANE_KEEP * panes.max(1) as f32 + if sidebar { SIDEBAR_WIDTH } else { 0. };
    (window - kept).max(MIN_WIDTH)
}

/// `limit` is `Shell::preview_width_limit`: the remembered width is kept as
/// the user left it and only drawn narrower while the window is too small.
pub fn render_panel(shell: &Shell, limit: f32, cx: &mut Context<Shell>) -> Option<AnyElement> {
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
        .hover(|style| style.bg(theme.accent.opacity(0.35)))
        .on_drag(ResizeHandle, |_, _, _, cx| cx.new(|_| NoPreview))
        .on_click(cx.listener(|this, event: &gpui::ClickEvent, _, cx| {
            if event.click_count() == 2 {
                this.preview.width = DEFAULT_WIDTH;
                this.remember_session();
                cx.notify();
            }
        }));
    Some(
        div()
            .id("preview-panel")
            .relative()
            .w(px(shell.preview.width.min(limit)))
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

#[cfg(test)]
mod tests {
    use super::max_width;

    #[test]
    fn the_panel_may_take_what_the_panes_do_not_need() {
        // One pane and the sidebar keep 440 px of a wide window.
        assert!((max_width(2560., 1, true) - 2120.).abs() < 1e-3);
        // Two panes keep twice a pane; without the sidebar, only the panes.
        assert!((max_width(1600., 2, true) - 920.).abs() < 1e-3);
        assert!((max_width(1600., 2, false) - 1120.).abs() < 1e-3);
        // A small window still leaves the panel its minimum.
        assert!((max_width(500., 2, false) - 200.).abs() < 1e-3);
    }
}
