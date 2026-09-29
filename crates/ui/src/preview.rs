//! Preview panel and peek overlay for the cursor entry: images (decoded on
//! a worker, cancelled when the cursor moves), text with syntax highlighting,
//! directories, document pages and seekable video stills via preview plugins,
//! and a generic card for everything else.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use bibli_core::op::EntryRef;
use bibli_core::preview::{PreviewInfo, PreviewRequest};
use bibli_core::workspace::PaneId;
use std::sync::Arc;

use bibli_core::entry::keys;
use bibli_core::scheduler::Priority;
use bibli_core::{CancelToken, EntryId};
use gpui::{
    AnyElement, Context, CursorStyle, DragMoveEvent, FocusHandle, MouseButton, ObjectFit, Render,
    RenderImage, Window, div, img, prelude::*, px,
};

use crate::format;
use crate::icons;
use crate::keymap;
use crate::shell::Shell;
use crate::theme::Theme;

const TEXT_LIMIT: usize = 64 * 1024;
/// The text size a book is read at before the reader changes it, in pixels.
pub(crate) const TEXT_SIZE: f32 = 16.;
/// The zoom range of a picture or a page, as a share of its pixels. The
/// floor is low enough that Zoom out from Fit always shrinks: a large page in
/// a narrow panel is shown well under a quarter of its size.
const MIN_ZOOM: f32 = 0.1;
const MAX_ZOOM: f32 = 4.0;
const IMAGE_MAX_SIDE: u32 = 2048;
/// The side a document's first page is rendered at, before any page was
/// shown to say what the panel needs.
const FIRST_PAGE_SIDE: u32 = 2048;

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
        /// How many pixels it can have, which bounds a zoom's sharpening.
        pixels: Pixels,
    },
    Dir {
        items: usize,
        folders: usize,
    },
    Generic,
    Error(String),
}

/// How many pixels a picture can be drawn with (`Sharpen::most`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pixels {
    /// A raster: its own, at most.
    Own,
    /// A drawing (SVG): as many as it is asked for, past its own size.
    Any,
    /// What another program drew (a thumbnailer, for a format bibli does
    /// not decode): what it gave is all there is.
    Drawn,
}

/// The player a preview result brings.
enum Media {
    /// A session of its own, just opened.
    Opened(Arc<dyn bibli_core::media::MediaSession>),
    /// The file is the pinned track: the panel shows the session that plays
    /// it rather than open a second one.
    Pinned,
}

struct PreviewResult {
    generation: u64,
    content: PreviewContent,
    complete: bool,
    session: Option<Arc<dyn bibli_core::document::DocumentSession>>,
    media: Option<Media>,
    /// The type the worker read from the bytes when the name did not settle
    /// it (`mime::identify`): the header shows it and the next request of
    /// the same file carries it, so the bytes are asked once.
    sniffed: Option<String>,
    /// The desktop's thumbnail of the file, sent ahead of its preview: kept
    /// aside (`PreviewState::placeholder`), never shown as its content.
    placeholder: bool,
}

/// Facts shown in the header, read from the entry's metadata.
#[derive(Clone, Default)]
pub struct Info {
    pub name: String,
    pub mime: Option<String>,
    /// Whether `mime` is settled: the name alone decides it, or the bytes
    /// were read. A name several types claim (`*.ogg`) gives the most
    /// general of them until the worker reads the file (`mime::identify`).
    pub identified: bool,
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
    /// What the container calls it, as its rows said when it was opened.
    target: String,
    entry: EntryRef,
    mime: Option<String>,
    path: std::path::PathBuf,
    /// The private directory an extracted member lives in; none for a file of
    /// its own that the container only names (a playlist's track).
    _directory: Option<tempfile::TempDir>,
}

/// The tracks the player walks instead of the folder's: the rows of the
/// playlist the panel went into, in the order it showed them.
pub struct Queue {
    /// How deep the playlist itself is: 0 when it is the entry.
    pub depth: usize,
    pub targets: Vec<String>,
    pub index: usize,
}

/// A track the player stepped to while it was playing: it starts once the
/// panel shows it. It names the entry and, inside a playlist, the row, so it
/// is never taken by a preview of anything else.
pub struct Autoplay {
    pub pane: PaneId,
    pub id: EntryId,
    pub member: Option<String>,
}

/// Marker type for dragging the panel's left edge (panel at the right).
#[derive(Clone)]
pub struct ResizeHandle;

/// Marker type for dragging the panel's top edge (panel at the bottom).
#[derive(Clone)]
pub struct ResizeHeightHandle;

/// What an edge being dragged shows under the pointer: nothing, the edge
/// itself follows. Shared by every drag handle of the panel and the dock.
pub(crate) struct NoDragView;

impl Render for NoDragView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// Which preview the panel shows. A change is another preview: everything
/// about the last one (page, zoom, find, folds, its player) goes with it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct PreviewKey {
    pane: PaneId,
    id: EntryId,
    path: PathBuf,
    providers: u64,
}

/// What the shown file is as the listing last said, and how it is drawn. A
/// change reads the same file again and keeps the reader where they were
/// (`Shell::preview_changed`): a log that grows, a PDF rebuilt, a song
/// whose tags were saved, the desktop turning dark.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Version {
    modified: Option<SystemTime>,
    size: Option<u64>,
    dark: bool,
}

impl Version {
    /// Whether the file itself changed. A date or size learnt for the first
    /// time (the entry's metadata arriving after it was selected) is no
    /// change: the preview already read the file as it is.
    fn rewritten(&self, now: &Version) -> bool {
        fn differs<T: PartialEq>(before: Option<T>, after: Option<T>) -> bool {
            before.is_some() && before != after
        }
        differs(self.modified, now.modified) || differs(self.size, now.size)
    }
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
    /// The panel's width at the right edge, and its height at the bottom:
    /// each remembered, so moving it back finds it as it was. The height is
    /// `None` until its edge is dragged: the default makes room for the
    /// pinned player's dock (`Shell::preview_panel_height`).
    pub width: f32,
    pub height: Option<f32>,
    pub info: Info,
    pub content: Option<PreviewContent>,
    pub(crate) phase: PreviewPhase,
    /// Whether the wait has gone on long enough to be shown (`sync_busy`):
    /// what the "Loading…" text, the caption and the bar under the toolbar
    /// all read, so a preview that comes at once never flickers through them.
    busy: bool,
    /// The grace of one request: its generation, and the timer.
    busy_timer: Option<(u64, gpui::Task<()>)>,
    cancel: Option<CancelToken>,
    pub(crate) generation: u64,
    key: Option<PreviewKey>,
    version: Option<Version>,
    /// The request in flight reads the shown file again after it changed:
    /// what it brings keeps the folds, the selection and the place.
    pub(crate) refresh: bool,
    pub(crate) page: u32,
    pub(crate) seconds: Option<f64>,
    pub(crate) zoom: Option<f32>,
    pub(crate) fit_width: bool,
    /// Where a picture zoomed past the panel is scrolled to (`picture`).
    pub(crate) picture_scroll: gpui::ScrollHandle,
    /// The file's thumbnail while its preview is on its way: shown only
    /// once the wait has lasted, as "Loading…" is (`body`).
    pub(crate) placeholder: Option<PreviewContent>,
    /// The side an image was asked to be decoded at for a zoom
    /// (`sharpen_picture`); 0 for the display side.
    pub(crate) picture_side: u32,
    /// The side document pages are rendered at: the one they were last
    /// shown at (`Shell::document_shown`), kept from one document to the
    /// next, so a page opens at the size the panel shows it.
    pub(crate) page_side: u32,
    /// The size a book laid out by its engine is read at (`MuPDF`'s books),
    /// in pixels; `None` until the reader changes it. A book in reading mode
    /// keeps its own (`inspector::State`), pictures and pages zoom instead.
    pub(crate) text_size: Option<f32>,
    tx: flume::Sender<PreviewResult>,
    rx: flume::Receiver<PreviewResult>,
    pub peek_focus: Option<FocusHandle>,
    /// The files of the same kind (music, or films) before and after this
    /// one in the pane's order; found when the listing, its order or the
    /// entry changes (`details_at`), never while painting.
    pub neighbours: (Option<EntryId>, Option<EntryId>),
    /// The track to start once shown: the one before it ended, or the user
    /// stepped to it while listening.
    pub autoplay: Option<Autoplay>,
    /// Set while the panel plays a track of a playlist.
    pub queue: Option<Queue>,
    /// The members the panel went down into, outermost first.
    pub nested: Vec<Arc<Nested>>,
    /// A member asked for and not extracted yet: the worker's next job.
    pub descend: Option<String>,
    /// The rows each member was opened from, when they are kept beside it
    /// (`members.rs`): one per member of `nested`, `None` for a container
    /// whose rows are not kept.
    members: Vec<Option<crate::members::Members>>,
    /// The rows the member of `descend` is asked from, until it arrives.
    pub(crate) members_asked: Option<crate::members::Members>,
    /// Their share of the room beside the member, as their edge was dragged.
    pub members_share: crate::members::Share,
    nested_tx: flume::Sender<(u64, Arc<Nested>)>,
    nested_rx: flume::Receiver<(u64, Arc<Nested>)>,
    /// Fields read on demand that Details shows (a picture's place,
    /// `bibli_core::fields`), for the entry of `details_at`: read on a
    /// worker, arriving with the generation they were asked at.
    pub fields: Vec<crate::details::Row>,
    fields_generation: u64,
    fields_tx: flume::Sender<(u64, Vec<crate::details::Row>)>,
    fields_rx: flume::Receiver<(u64, Vec<crate::details::Row>)>,
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
    /// Where a map preview is moved and zoomed to: the file's, kept when
    /// it is read again, dropped with it (`cancel_preview`).
    pub(crate) map: crate::map_preview::MapState,
    /// Whether the hexadecimal provider is registered, keyed by the preview
    /// registry revision it was read at (see `preview_toolbar::hex_available`).
    pub(crate) hex_provider: std::cell::Cell<Option<(u64, bool)>>,
}

impl Default for PreviewState {
    fn default() -> Self {
        let (tx, rx) = flume::unbounded();
        let (nested_tx, nested_rx) = flume::unbounded();
        let (fields_tx, fields_rx) = flume::unbounded();
        PreviewState {
            reader: crate::reader::ReaderState::default(),
            inspection: crate::inspector::State::default(),
            media: crate::media::MediaState::default(),
            menu: None,
            open: false,
            peek: false,
            width: DEFAULT_WIDTH,
            height: None,
            info: Info::default(),
            content: None,
            phase: PreviewPhase::Idle,
            busy: false,
            busy_timer: None,
            cancel: None,
            generation: 0,
            key: None,
            version: None,
            refresh: false,
            page: 1,
            seconds: None,
            zoom: None,
            fit_width: false,
            picture_scroll: gpui::ScrollHandle::new(),
            placeholder: None,
            picture_side: 0,
            page_side: FIRST_PAGE_SIDE,
            text_size: None,
            tx,
            rx,
            peek_focus: None,
            neighbours: (None, None),
            queue: None,
            autoplay: None,
            nested: Vec::new(),
            members: Vec::new(),
            members_asked: None,
            members_share: crate::members::Share::default(),
            descend: None,
            nested_tx,
            nested_rx,
            fields: Vec::new(),
            fields_generation: 0,
            fields_tx,
            fields_rx,
            details: Vec::new(),
            details_at: None,
            selection_summary: None,
            details_open: false,
            details_scroll: gpui::ScrollHandle::new(),
            image_focus: None,
            map: crate::map_preview::MapState::default(),
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

    /// The file that plays when the panel plays: the track of a playlist it
    /// went into, otherwise the entry itself.
    pub fn playing_path(&self) -> Option<PathBuf> {
        match self.nested.last() {
            Some(member) => Some(member.path.clone()),
            None => self.path().map(Path::to_owned),
        }
    }

    /// The pane whose entry the panel is showing.
    pub fn pane(&self) -> Option<bibli_core::workspace::PaneId> {
        self.key.as_ref().map(|key| key.pane)
    }

    /// The entry the panel is showing.
    pub fn entry_id(&self) -> Option<EntryId> {
        self.key.as_ref().map(|key| key.id)
    }

    /// A request for the file shown `depth` members deep (0 is the entry),
    /// as a provider would be asked about it.
    pub(crate) fn request_at(&self, depth: usize) -> Option<PreviewRequest> {
        let (entry, mime) = match depth.checked_sub(1) {
            None => {
                let key = self.key.as_ref()?;
                (EntryRef::new(key.id, &key.path), self.info.mime.clone())
            }
            Some(index) => {
                let member = self.nested.get(index)?;
                (member.entry.clone(), member.mime.clone())
            }
        };
        Some(PreviewRequest {
            entry,
            mime,
            page: 1,
            seconds: None,
            max_side: 0,
            text_size: None,
        })
    }

    /// The rows kept beside the member shown or asked for, if any.
    pub fn members(&self) -> Option<&crate::members::Members> {
        match &self.members_asked {
            Some(asked) => Some(asked),
            None => self.members.last()?.as_ref(),
        }
    }

    pub(crate) fn members_mut(&mut self) -> Option<&mut crate::members::Members> {
        match &mut self.members_asked {
            Some(asked) => Some(asked),
            None => self.members.last_mut()?.as_mut(),
        }
    }

    /// Take the rows kept beside the member shown, to ask for another.
    pub(crate) fn take_members(&mut self) -> Option<crate::members::Members> {
        self.members_asked
            .take()
            .or_else(|| self.members.last_mut()?.take())
    }

    /// Whether the kept rows have the keyboard.
    pub(crate) fn members_hold_focus(&self, window: &Window, cx: &gpui::App) -> bool {
        self.members()
            .is_some_and(|members| members.focus.contains_focused(window, cx))
    }

    /// Whether a preview result is for the track the player stepped to.
    fn autoplay_matches(&self) -> bool {
        let (Some(wanted), Some(key)) = (&self.autoplay, &self.key) else {
            return false;
        };
        wanted.pane == key.pane
            && wanted.id == key.id
            && wanted.member.as_ref() == self.nested.last().map(|member| &member.target)
    }
}

impl Drop for PreviewState {
    fn drop(&mut self) {
        if let Some(cancel) = &self.cancel {
            cancel.cancel();
        }
    }
}

/// Whose turn it is to decode a picture for the preview. A photo decodes
/// at full size before it is scaled down, tens of megabytes at a time, and
/// a decode that has started cannot be stopped: holding an arrow key through
/// a folder of photos otherwise ran one on every worker at once. One at a
/// time, and a request that waited for its turn gives it up if the panel
/// moved on meanwhile, so the peak is one picture and passed-over files are
/// never decoded.
static DECODE_TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Whether the file shown opens in the document reader, as pages: its
/// thumbnail, while it loads, stands where the reader will put its first.
fn opens_as_pages(shell: &Shell) -> bool {
    let mime = shell.preview.info.mime.as_deref().unwrap_or_default();
    let extension = shell
        .preview
        .key
        .as_ref()
        .map(|key| bibli_platform::inspection::extension(&key.path))
        .unwrap_or_default();
    mime == "application/pdf"
        || mime.contains("comicbook")
        || matches!(extension.as_str(), "pdf" | "cbz" | "cbr" | "cb7" | "cbt")
}

/// An image's own size, from its header, turned as it is shown; a
/// drawing's, from its document.
fn image_size(path: &Path, mime: &str, cancel: &CancelToken) -> Option<(u32, u32)> {
    use bibli_platform::inspection;
    if matches!(mime, "image/svg+xml" | "image/svg+xml-compressed") {
        return inspection::drawing_size(&inspection::read_drawing(path, cancel).ok()?).ok();
    }
    bibli_platform::picture::size(bibli_platform::picture::open(path).ok()?).ok()
}

/// An image decoded at most `side` pixels on its longer side (at least the
/// display side, `IMAGE_MAX_SIDE`; a zoom asks for more), with its own size,
/// what 100% is, as its width and height.
fn decode_image(path: &Path, side: u32, cancel: &CancelToken) -> anyhow::Result<PreviewContent> {
    let side = side.clamp(
        IMAGE_MAX_SIDE,
        bibli_platform::inspection::limits::PICTURE_SIDE,
    );
    let reader = || bibli_platform::picture::open(path);
    let (w, h) = bibli_platform::picture::size(reader()?)?;
    anyhow::ensure!(
        u64::from(w) * u64::from(h) <= 64 * 1024 * 1024,
        "image is too large to preview"
    );
    // A decode that panicked held nothing worth protecting: take the turn.
    let _turn = DECODE_TURN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if cancel.is_cancelled() {
        anyhow::bail!("cancelled");
    }
    let img = bibli_platform::picture::decode(reader()?)?;
    if cancel.is_cancelled() {
        anyhow::bail!("cancelled");
    }
    let img = if img.width().max(img.height()) > side {
        img.thumbnail(side, side)
    } else {
        img
    };
    let rgba = img.to_rgba8();
    let image = crate::thumbs::owned_render_image(rgba.width(), rgba.height(), rgba.into_raw())
        .ok_or_else(|| anyhow::anyhow!("invalid image"))?;
    Ok(PreviewContent::Image {
        image,
        width: w,
        height: h,
        detail: None,
        document: None,
        facts: bibli_platform::exif::details(path),
        pixels: Pixels::Own,
    })
}

/// A drawing (SVG) rasterized at its own size, or, once the panel asked
/// for `side` pixels (a zoom, a high-density panel), at that; its own size
/// is what 100% is.
fn decode_drawing(path: &Path, side: u32, cancel: &CancelToken) -> anyhow::Result<PreviewContent> {
    use bibli_platform::inspection;
    let bytes = inspection::read_drawing(path, cancel)?;
    let decoded = inspection::drawing(&bytes, (side > 0).then_some(side))?;
    if cancel.is_cancelled() {
        anyhow::bail!("cancelled");
    }
    let image = crate::thumbs::owned_render_image(decoded.width, decoded.height, decoded.rgba)
        .ok_or_else(|| anyhow::anyhow!("invalid image"))?;
    Ok(PreviewContent::Image {
        image,
        width: decoded.own.0,
        height: decoded.own.1,
        detail: None,
        document: None,
        facts: Vec::new(),
        pixels: Pixels::Any,
    })
}

/// Where this process keeps the members it extracted: in the one deletable
/// cache, under its own pid, so that what a killed process left behind can be
/// told from what a running one still shows.
pub(crate) fn members_dir(cache: &Path) -> PathBuf {
    cache
        .join("preview-members")
        .join(std::process::id().to_string())
}

/// Remove what processes that no longer run left in the cache's per-process
/// directories: extracted members, and the covers of what they played.
/// Filesystem work: call it from a worker.
pub fn sweep_members(cache: &Path) {
    for directory in ["preview-members", crate::player::COVERS] {
        sweep(&cache.join(directory));
    }
}

fn sweep(directory: &Path) {
    let Ok(entries) = std::fs::read_dir(directory) else {
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
        member.own_file || member.path.starts_with(directory.path()),
        "A member was written outside its directory"
    );
    let entry = EntryRef::from_path(&member.path)
        .ok_or_else(|| anyhow::anyhow!("The extracted member is missing"))?;
    Ok(Some(Arc::new(Nested {
        target: target.to_owned(),
        // Resolved once, here on the worker: by name, and by the bytes
        // where the name is not enough.
        mime: bibli_platform::mime::identify(std::ffi::OsStr::new(&member.name), &member.path)
            .map(|mime| mime.to_string()),
        name: member.name,
        entry,
        path: member.path,
        // Nothing was written for a file of its own: the empty directory goes
        // now, here on the worker.
        _directory: (!member.own_file).then_some(directory),
    })))
}

fn compute(
    path: &Path,
    is_dir: bool,
    mime: Option<&str>,
    dark: bool,
    offset: u64,
    picture_side: u32,
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
    if matches!(mime, Some("image/svg+xml" | "image/svg+xml-compressed")) {
        return decode_drawing(path, picture_side, cancel)
            .unwrap_or_else(|e| PreviewContent::Error(format!("{e:#}")));
    }
    if top == "image" {
        return decode_image(path, picture_side, cancel)
            .unwrap_or_else(|e| PreviewContent::Error(format!("{e:#}")));
    }
    if top == "video" || mime == Some("application/pdf") {
        return PreviewContent::Generic;
    }
    let head = match read_head(path) {
        Ok(h) => h,
        Err(e) => return PreviewContent::Error(e.to_string()),
    };
    // Whatever the name declared, the bytes decide, by the same rule that
    // names an unknown file's bytes (`mime::by_content`): a `.txt` holding
    // NULs is not text, and a PEM key named like a Keynote file is.
    if bibli_platform::inspection::text_prefix(&head).is_some() {
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
        let kind = match args.get("kind").and_then(serde_json::Value::as_str) {
            Some("zoom") => "zoom",
            Some("text") => "text",
            Some("unit") => "unit",
            Some(crate::map_preview::BACKGROUND_POPUP) => crate::map_preview::BACKGROUND_POPUP,
            Some(crate::media::CONTINUE_POPUP) => crate::media::CONTINUE_POPUP,
            _ => "preview",
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
        let dark = cx.global::<Theme>().is_dark;
        let current = {
            let ws = self.app.workspace.read();
            ws.pane(self.pane).and_then(|pane| {
                let e = pane.entry(id)?;
                let path = pane.path_of(id)?;
                // What Details and the neighbouring tracks were collected
                // for: the listing as it is, in the order shown (a new sort
                // is a new order), and how much of it is selected.
                let order = std::sync::Arc::as_ptr(&self.ui().list.sorted).cast::<()>() as u64;
                let seen = pane
                    .listing
                    .version()
                    .wrapping_mul(31)
                    .wrapping_add(pane.tree_version())
                    .wrapping_mul(31)
                    .wrapping_add(order)
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
                        providers: self.app.previews.revision(),
                    },
                    Version {
                        modified: e
                            .metadata
                            .value(keys::MODIFIED)
                            .and_then(bibli_core::Value::as_time),
                        size: e
                            .metadata
                            .value(keys::SIZE)
                            .and_then(bibli_core::Value::as_u64),
                        dark,
                    },
                    e.clone(),
                ))
            })
        };
        let Some((seen, key, version, entry)) = current else {
            self.cancel_preview(window, cx);
            return;
        };
        // Invalidate before draining results: an old result must never land on
        // a newly selected file, a changed file, or a different pane. Inside a
        // container, a changed container is another preview: what was
        // extracted from it is stale.
        let changed = self.preview.version.as_ref() != Some(&version);
        let rewritten = self
            .preview
            .version
            .as_ref()
            .is_some_and(|before| before.rewritten(&version));
        if self.preview.key.as_ref() != Some(&key) || (rewritten && !self.preview.nested.is_empty())
        {
            if self
                .preview
                .autoplay
                .as_ref()
                .is_some_and(|wanted| wanted.pane != key.pane || wanted.id != key.id)
            {
                self.preview.autoplay = None;
            }
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
            let identified =
                entry.is_dir() || (mime.is_some() && !bibli_platform::mime::ambiguous(&entry.name));
            self.preview.info = Info {
                name: entry.name_str().into_owned(),
                mime,
                identified,
                size: None,
                modified: None,
                is_dir: entry.is_dir(),
                icon: self.icons.for_entry(&entry, 48),
            };
            // Play on selection is the wish a step makes, made by the
            // selection: the track plays once shown, and the dock of what
            // it replaces stays aside meanwhile (`dock_shown`).
            if self.prefs.player.autoplay
                && self.preview.autoplay.is_none()
                && self.media_kind().is_some()
            {
                self.preview.autoplay = Some(Autoplay {
                    pane: key.pane,
                    id: key.id,
                    member: None,
                });
            }
            self.preview.version = Some(version.clone());
            self.describe_version(&version, entry.is_dir());
            if !entry.metadata.is_hydrated() {
                self.request_hydrate(vec![id]);
            }
        } else if changed {
            let before = self.preview.version.replace(version.clone());
            self.describe_version(&version, entry.is_dir());
            self.preview_changed(before, &version, window, cx);
        }
        if self.preview.details_at != Some(seen) {
            self.preview.details_at = Some(seen);
            self.preview.details = crate::details::entry_groups(&entry, &key.path);
            self.read_fields(&entry, &key.path);
            self.preview.neighbours = self
                .media_kind()
                .map_or((None, None), |kind| self.media_neighbours(id, kind));
            let selection = self.selection_ctx();
            self.preview.selection_summary = (selection.count > 1).then(|| crate::details::Group {
                title: "Selection".into(),
                rows: vec![
                    crate::details::Row::new("Selected", format::count(selection.count, "item")),
                    crate::details::Row::new("Total size", format::size(selection.bytes)),
                ],
            });
        }
        while let Ok((generation, rows)) = self.preview.fields_rx.try_recv() {
            if generation == self.preview.fields_generation {
                self.preview.fields = rows;
                cx.notify();
            }
        }
        while let Ok((generation, member)) = self.preview.nested_rx.try_recv() {
            if generation == self.preview.generation && self.preview.nested.len() < NESTING {
                self.preview.nested.push(member);
                let members = self.preview.members_asked.take();
                self.preview.members.push(members);
                self.preview.descend = None;
            } else {
                self.discard_nested(vec![member]);
            }
        }
        while let Ok(result) = self.preview.rx.try_recv() {
            if result.generation == self.preview.generation {
                self.preview_arrived(result, window, cx);
            }
        }
        if self.preview.phase != PreviewPhase::Queued {
            return;
        }
        self.preview_request(&key, &version, &entry);
    }

    /// The header's size and date, from the listing's metadata.
    fn describe_version(&mut self, version: &Version, is_dir: bool) {
        // A folder's own size says nothing about what it holds.
        self.preview.info.size = version.size.filter(|_| !is_dir).map(format::size);
        self.preview.info.modified = version.modified.map(format::time);
        self.preview.details_at = None;
    }

    /// The shown file changed, or the theme did: read it again without
    /// losing the reader's place. The page, the zoom, the find field, the
    /// folds and the place in the text stay (`refresh`); a document is
    /// opened again at the page it was on. A player that is live keeps
    /// playing: what it holds open is what it plays. Only text is drawn in
    /// the theme's colours, so a theme change re-reads nothing else.
    fn preview_changed(
        &mut self,
        before: Option<Version>,
        now: &Version,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.preview.media.session.is_some() {
            return;
        }
        let content = before.is_some_and(|before| before.rewritten(now));
        let text = matches!(self.preview.content, Some(PreviewContent::Inspection(_)));
        if !content && !text {
            return;
        }
        if content {
            self.reader_reopen(window, cx);
        }
        self.cancel_pending_preview();
        self.preview.refresh = true;
        self.preview.phase = PreviewPhase::Queued;
        cx.notify();
    }

    /// A result for the current request.
    fn preview_arrived(
        &mut self,
        mut result: PreviewResult,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // What the bytes said: the header names it, and the file's next
        // request (a page, a search) carries it instead of asking the bytes
        // again. A member the panel went down into resolved its own type
        // when it was extracted.
        if let Some(sniffed) = result.sniffed
            && !self.preview.info.identified
            && self.preview.nested.is_empty()
        {
            let mime = bibli_platform::mime::Mime(sniffed);
            self.type_names.get(&mime);
            self.type_names.request(&Arc::clone(&self.app));
            self.preview.info.mime = Some(mime.0);
            self.preview.info.identified = true;
        }
        // The thumbnail waits aside: it is shown only once the wait has
        // lasted (`BUSY_AFTER`), in place of "Loading…", so a preview that
        // comes at once never flashes it.
        if result.placeholder {
            if let Some(PreviewContent::Image { image, .. }) =
                self.preview.placeholder.replace(result.content)
            {
                cx.drop_image(image, Some(window));
            }
            cx.notify();
            return;
        }
        // A file with no picture of its own (a type without a preview, a
        // film until its first frame) keeps its thumbnail as its picture;
        // any other preview replaces it.
        if let Some(placeholder) = self.preview.placeholder.take() {
            if matches!(result.content, PreviewContent::Generic) && result.complete {
                result.content = placeholder;
            } else if let PreviewContent::Image { image, .. } = placeholder {
                cx.drop_image(image, Some(window));
            }
        }
        if result.complete {
            self.preview.reader.tried = true;
            // The track stepped to is shown now, playable or not: the wish
            // ends here either way.
            let autoplay = self.preview.autoplay_matches();
            if autoplay {
                self.preview.autoplay = None;
            }
            match result.media {
                Some(Media::Opened(session)) => {
                    let play = autoplay || self.prefs.player.autoplay;
                    self.show_media(session, play, window, cx);
                }
                Some(Media::Pinned) if !self.show_pinned(autoplay, window, cx) => {
                    // The pin moved on while this was on its way.
                    self.cancel_pending_preview();
                    self.preview.phase = PreviewPhase::Queued;
                    return;
                }
                Some(Media::Pinned) | None => {}
            }
            if let Some(session) = result.session {
                self.reader_opened(session, window, cx);
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
                PreviewInfo::Picture { .. } => {}
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
            return;
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
        // A film's thumbnail stands for it until its player has a frame.
        let thumbnail = self.preview.content.is_none()
            && matches!(result.content, PreviewContent::Image { detail: None, .. });
        if self.preview.media.session.is_none() || poster || thumbnail {
            self.replace_preview_content(Some(result.content), window, cx);
        }
        self.preview.phase = if result.complete {
            PreviewPhase::Idle
        } else {
            PreviewPhase::Loading
        };
    }

    /// A player opened for the shown file.
    fn show_media(
        &mut self,
        session: Arc<dyn bibli_core::media::MediaSession>,
        play: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
        if play {
            self.media_send(bibli_core::media::Control::Play(true), cx);
        }
    }

    /// The pinned track selected again: the panel shows the session that is
    /// playing, not a second one beside it, with its state and its frames as
    /// they are and no poster to wait behind. Its picture comes from the pin
    /// (the cover, a film's last frame) until the session sends another.
    /// False when the pin is no longer this file.
    fn show_pinned(&mut self, play: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let playing = self.preview.playing_path();
        let Some(pinned) = self
            .player
            .pinned
            .as_ref()
            .filter(|pinned| Some(&pinned.path) == playing.as_ref())
        else {
            return false;
        };
        let session = Arc::clone(&pinned.session);
        let picture = match (&pinned.frame, &pinned.cover) {
            (Some((image, width, height)), _) if pinned.video => {
                Some((Arc::clone(image), *width, *height))
            }
            (_, Some(cover)) if !pinned.video => {
                let size = cover.size(0);
                Some((
                    Arc::clone(cover),
                    u32::try_from(size.width.0).unwrap_or(0),
                    u32::try_from(size.height.0).unwrap_or(0),
                ))
            }
            _ => None,
        };
        self.preview.media.state = pinned.state.clone();
        self.preview.media.show_live();
        if let Some((image, width, height)) = picture {
            self.replace_preview_content(
                Some(PreviewContent::Image {
                    image,
                    width,
                    height,
                    detail: None,
                    document: None,
                    facts: Vec::new(),
                    pixels: Pixels::Own,
                }),
                window,
                cx,
            );
        }
        self.show_media(session, play, window, cx);
        true
    }

    /// Ask a worker for the preview of what the panel shows now.
    fn preview_request(&mut self, key: &PreviewKey, version: &Version, entry: &bibli_core::Entry) {
        let cancel = self.app.lifetime().child();
        self.preview.cancel = Some(cancel.clone());
        self.preview.phase = PreviewPhase::Loading;
        let generation = self.preview.generation;
        let request = PreviewRequest {
            entry: EntryRef::new(key.id, &key.path),
            mime: self.preview.info.mime.clone(),
            page: self.preview.page,
            seconds: self.preview.seconds,
            max_side: IMAGE_MAX_SIDE,
            text_size: self.preview.text_size,
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
        // The file's own name, while its type is not settled: the worker
        // reads the bytes to settle it, once.
        let unsettled = (!is_dir && !self.preview.info.identified).then(|| entry.name.clone());
        let session = self.preview.reader.session.clone();
        let picture_side = self.preview.picture_side;
        // A document's page is rendered at the side pages are shown at.
        let page_side = self.preview.page_side;
        self.preview.reader.shown_side = page_side;
        let open_document = !is_dir && !self.preview.reader.tried;
        let lifetime = self.preview.reader.lifetime.clone();
        let media_lifetime = self.preview.media.lifetime();
        // Never a second player for one file: the pinned track is shown
        // with the session that plays it.
        let pinned = self
            .player
            .pinned
            .as_ref()
            .map(|pinned| pinned.path.clone());
        let (mut path, modified, dark) = (key.path.clone(), version.modified, version.dark);
        // A desktop thumbnail represents the file, not the requested page or
        // timestamp. Only use it before this file has any preview content.
        let show_thumbnail = !is_dir && self.preview.content.is_none();
        self.app
            .scheduler
            .spawn(Priority::Visible, "preview", cancel.clone(), move || {
                // What the bytes said, once they were asked.
                let sniffed = std::cell::RefCell::new(None::<String>);
                // Every reply goes this one way: nothing after a cancel, and
                // the UI woken after each.
                let send = |content: PreviewContent,
                            complete: bool,
                            session: Option<Arc<dyn bibli_core::document::DocumentSession>>,
                            media: Option<Media>| {
                    if !cancel.is_cancelled() {
                        let _ = tx.send(PreviewResult {
                            generation,
                            content,
                            complete,
                            session,
                            media,
                            sniffed: sniffed.borrow().clone(),
                            placeholder: false,
                        });
                        wake();
                    }
                };
                let fail = |error: anyhow::Error| {
                    send(
                        PreviewContent::Error(format!("{error:#}")),
                        true,
                        None,
                        None,
                    );
                };
                // What is previewed: the entry, or the member of it the panel
                // went down into. Everything below treats it as any file.
                let mut request = request;
                // Whether the entry itself is shown, not a member of it.
                let own = nested.is_none() && descend.is_none();
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
                        Err(error) => return fail(error),
                    }
                }
                // A name that says nothing (`settings.cfg`, `README`) is not
                // a binary file, and one several types claim (`*.ogg`) is not
                // yet any of them: the bytes are asked, here on the worker,
                // before any provider is. From here on an
                // `application/octet-stream` request means binary bytes. A
                // member went down into was identified when extracted.
                if !is_dir && (request.mime.is_none() || (own && unsettled.is_some())) {
                    request.mime = match unsettled.as_deref().filter(|_| own) {
                        Some(name) => bibli_platform::mime::identify(name, &path),
                        None => bibli_platform::mime::by_content(&path),
                    }
                    .map(|m| m.to_string());
                    sniffed.replace(request.mime.clone());
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
                            let mut display = crate::inspector::Display::new(data, dark);
                            // Its pictures for later are asked for as it was.
                            display.origin = Some((request.clone(), inspection.clone()));
                            return send(PreviewContent::Inspection(display), true, None, None);
                        }
                        Err(error) => return fail(error),
                        Ok(None) => {}
                    }
                }
                // Reuse only an already cached desktop thumbnail as initial feedback.
                if show_thumbnail {
                    use bibli_core::thumbs::{ThumbRequest, ThumbSize, ThumbnailProvider};
                    let cached = bibli_platform::thumb_providers::FreedesktopCache::desktop(false);
                    let thumb = ThumbRequest {
                        entry: request.entry.clone(),
                        mime: request.mime.clone().map(Into::into),
                        mtime: modified,
                        size: ThumbSize::Large,
                    };
                    if let Ok(Some(image)) = cached.thumbnail(&thumb, &cancel)
                        && let Some(render) = crate::thumbs::to_render_image(&image)
                    {
                        // An image's thumbnail stands at the image's own
                        // size, read from its header: when the preview comes,
                        // only its sharpness changes. Any other keeps only
                        // its proportions and stands as large as a preview
                        // may be, filling the panel as a film or a page will.
                        let own = request
                            .mime
                            .as_deref()
                            .filter(|mime| mime.starts_with("image/"))
                            .and_then(|mime| image_size(&path, mime, &cancel))
                            .unwrap_or_else(|| {
                                let scale = f64::from(IMAGE_MAX_SIDE)
                                    / f64::from(image.width.max(image.height).max(1));
                                let side = |length: u32| {
                                    ((f64::from(length) * scale).round() as u32).max(1)
                                };
                                (side(image.width), side(image.height))
                            });
                        let content = PreviewContent::Image {
                            image: render,
                            width: own.0,
                            height: own.1,
                            detail: None,
                            document: None,
                            facts: Vec::new(),
                            pixels: Pixels::Own,
                        };
                        if !cancel.is_cancelled() {
                            let _ = tx.send(PreviewResult {
                                generation,
                                content,
                                complete: false,
                                session: None,
                                media: None,
                                sniffed: None,
                                placeholder: true,
                            });
                            wake();
                        }
                    }
                }
                if !is_dir && pinned.as_ref() == Some(&path) {
                    return send(PreviewContent::Generic, true, None, Some(Media::Pinned));
                }
                // Provider registration owns media recognition, including container MIME types.
                match previews.open_media(&request, &media_lifetime, wake.clone()) {
                    Ok(Some(media)) => {
                        send(
                            PreviewContent::Generic,
                            true,
                            None,
                            Some(Media::Opened(media)),
                        );
                        // Keep startup/playback independent of optional poster
                        // extraction. The same cancellation token stops FFmpeg
                        // when Play/Seek, selection or provider changes supersede it.
                        let mut poster = request.clone();
                        poster.max_side = u32::MAX;
                        if let Ok(Some(preview)) = previews.render(&poster, &cancel)
                            && let Some(image) = crate::thumbs::to_render_image(&preview.image)
                        {
                            let content = PreviewContent::Image {
                                image,
                                width: preview.image.width,
                                height: preview.image.height,
                                detail: Some(preview.info),
                                document: None,
                                // Where it was recorded, which Details
                                // shows beside what the player says.
                                facts: bibli_platform::mp4::details(&path),
                                pixels: Pixels::Own,
                            };
                            send(content, true, None, None);
                        }
                        return;
                    }
                    Err(error) => return fail(error),
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
                            .render(request.page, page_side, &cancel)
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
                                    pixels: Pixels::Own,
                                })
                            })
                            .unwrap_or_else(|e| PreviewContent::Error(format!("{e:#}")));
                        return send(content, true, Some(session), None);
                    }
                    Err(error) => return fail(error),
                    Ok(None) => {}
                }
                let result = if is_dir {
                    Ok(None)
                } else {
                    previews.render(&request, &cancel)
                };
                let content = match result {
                    Ok(Some(preview)) => match crate::thumbs::to_render_image(&preview.image) {
                        // Drawn by another program: its own size, not the
                        // one drawn, is what 100% and Details say.
                        Some(image) => match preview.info {
                            PreviewInfo::Picture { width, height } => PreviewContent::Image {
                                image,
                                width,
                                height,
                                detail: None,
                                document: None,
                                facts: bibli_platform::exif::details(&path),
                                pixels: Pixels::Drawn,
                            },
                            info => PreviewContent::Image {
                                image,
                                width: preview.image.width,
                                height: preview.image.height,
                                detail: Some(info),
                                document: None,
                                facts: Vec::new(),
                                pixels: Pixels::Own,
                            },
                        },
                        None => PreviewContent::Error("Invalid preview image".into()),
                    },
                    Ok(None) => compute(
                        &path,
                        is_dir,
                        request.mime.as_deref(),
                        dark,
                        inspection.offset,
                        picture_side,
                        &cancel,
                    ),
                    Err(e) => PreviewContent::Error(format!("{e:#}")),
                };
                send(content, true, None, None);
            });
    }

    pub(crate) fn cancel_pending_preview(&mut self) {
        if let Some(cancel) = self.preview.cancel.take() {
            cancel.cancel();
        }
        self.preview.generation = self.preview.generation.wrapping_add(1);
        // Whatever asks next says for itself whether it is a refresh.
        self.preview.refresh = false;
    }

    /// How long a preview may take before the panel says it is waiting.
    const BUSY_AFTER: std::time::Duration = std::time::Duration::from_millis(200);

    /// The one delayed "still loading" signal of the panel, like the pane's
    /// (`loading.rs`): a wait is shown only once it has lasted `BUSY_AFTER`,
    /// and each request (generation) gets a grace of its own. Called once
    /// per wake, after the results were drained.
    pub(crate) fn sync_busy(&mut self, cx: &mut Context<Self>) {
        if self.preview.phase == PreviewPhase::Idle {
            if self.preview.busy {
                cx.notify();
            }
            self.preview.busy = false;
            self.preview.busy_timer = None;
            return;
        }
        // A request started while another's grace ran gets a grace of its
        // own: the old timer, which would find the generation changed and
        // do nothing, is replaced.
        let generation = self.preview.generation;
        if self.preview.busy
            || self
                .preview
                .busy_timer
                .as_ref()
                .is_some_and(|(timing, _)| *timing == generation)
        {
            return;
        }
        let timer = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Self::BUSY_AFTER).await;
            let _ = this.update(cx, |shell, cx| {
                if shell.preview.generation != generation {
                    return;
                }
                let waiting = shell.preview.phase != PreviewPhase::Idle;
                shell.preview.busy_timer = None;
                if waiting {
                    shell.preview.busy = true;
                    cx.notify();
                }
            });
        });
        self.preview.busy_timer = Some((generation, timer));
    }

    /// Whether the panel is visibly waiting for its preview.
    pub fn preview_busy(&self) -> bool {
        self.preview.busy
    }

    pub(crate) fn replace_preview_content(
        &mut self,
        content: Option<PreviewContent>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // RenderImage's Arc does not own its uploaded atlas entry. Evict it
        // explicitly, just like the thumbnail LRU, on replacement and close.
        // A thumbnail kept aside for the wait goes with it.
        if let Some(PreviewContent::Image { image, .. }) = self.preview.placeholder.take() {
            cx.drop_image(image, Some(window));
        }
        match self.preview.content.take() {
            Some(PreviewContent::Image { image, .. }) => {
                cx.drop_image(image, Some(window));
            }
            Some(PreviewContent::Inspection(display)) => {
                for image in display.images.iter().flatten() {
                    cx.drop_image(Arc::clone(image), Some(window));
                }
            }
            _ => {}
        }
        // A note belongs to the text it was opened from.
        self.preview.inspection.notes.close(window);
        // Pictures decoded as they came into view belong to the page they
        // were of, whatever replaces it.
        for image in self.preview.inspection.pictures.clear() {
            cx.drop_image(image, Some(window));
        }
        let refresh = std::mem::take(&mut self.preview.refresh);
        if let Some(PreviewContent::Inspection(display)) = &content {
            self.preview.inspection.arrived(display, refresh);
            self.preview
                .inspection
                .focus
                .get_or_insert_with(|| cx.focus_handle());
        }
        self.preview.content = content;
    }

    /// The fields Details shows for `entry` at `path` (a picture's place,
    /// `bibli_core::fields`), read on a worker; what was shown for another
    /// entry goes at once, and a late answer for it is dropped.
    fn read_fields(&mut self, entry: &bibli_core::entry::Entry, path: &std::path::Path) {
        self.preview.fields_generation += 1;
        self.preview.fields.clear();
        if entry.is_dir() {
            return;
        }
        let shown = self.app.fields.shown_for(&entry.name);
        if shown.is_empty() {
            return;
        }
        let generation = self.preview.fields_generation;
        let tx = self.preview.fields_tx.clone();
        let fields = Arc::clone(&self.app.fields);
        let path = path.to_path_buf();
        let wake = self.app.waker_handle();
        let cancel = self.app.lifetime().child();
        self.app.scheduler.spawn(
            bibli_core::scheduler::Priority::Visible,
            "detail-fields",
            cancel.clone(),
            move || {
                let rows: Vec<crate::details::Row> = shown
                    .into_iter()
                    .filter_map(|(key, label)| {
                        let text = match fields.get(key, &path, &cancel)? {
                            bibli_core::entry::Value::Text(text) => text.to_string(),
                            _ => return None,
                        };
                        let name = path.file_name().unwrap_or_default();
                        Some(crate::details::Row {
                            source: fields.credit(key, name).map(Into::into),
                            ..crate::details::Row::new(label, text)
                        })
                    })
                    .collect();
                if !rows.is_empty() && tx.send((generation, rows)).is_ok() {
                    wake();
                }
            },
        );
    }

    fn cancel_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel_pending_preview();
        // Everything that can hold the keyboard goes with the preview: its
        // menu, its find fields, its content. Keys held there go back to
        // the peek overlay while it is open, else to the pane, so they are
        // never left with an element that no longer exists. An existing
        // address or pane focus is kept.
        let menu = self
            .preview
            .menu
            .take()
            .is_some_and(|menu| menu.focus.contains_focused(window, cx));
        let held = menu
            || self.preview.reader.holds_focus(window, cx)
            || self.preview.inspection.holds_focus(window, cx)
            || self.preview.members_hold_focus(window, cx)
            || [&self.preview.media.focus, &self.preview.image_focus]
                .into_iter()
                .flatten()
                .any(|focus| focus.contains_focused(window, cx));
        if held {
            match self
                .preview
                .peek_focus
                .as_ref()
                .filter(|_| self.preview.peek)
            {
                Some(peek) => window.focus(peek),
                None => window.focus(&self.ui().focus),
            }
        }
        self.preview.key = None;
        self.preview.version = None;
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
        self.preview.picture_scroll = gpui::ScrollHandle::new();
        self.preview.picture_side = 0;
        self.preview.text_size = None;
        self.preview.map = crate::map_preview::MapState::default();
    }

    pub fn preview_page(&mut self, delta: i32, window: &mut Window, cx: &mut Context<Self>) {
        // A book counted by its printed pages turns them.
        if let Some(PreviewContent::Inspection(display)) = &self.preview.content
            && crate::inspector::counts_printed(self, &display.data)
        {
            let current = self.preview.inspection.printed_page(&display.data);
            let index = if delta < 0 {
                current.and_then(|index| index.checked_sub(1))
            } else {
                Some(current.map_or(0, |index| index + 1))
            }
            .filter(|index| *index < display.data.printed_pages.len());
            if let Some(index) = index {
                let args = serde_json::json!({"action":"printed","index":index});
                self.run("ui.inspect", args, window, cx);
            }
            return;
        }
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
        if self
            .preview
            .queue
            .as_ref()
            .is_some_and(|queue| depth <= queue.depth)
        {
            self.preview.queue = None;
        }
        let left = self.preview.nested.split_off(depth);
        self.discard_nested(left);
        self.preview.members.truncate(depth);
        self.preview.members_asked = None;
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
        // With the panel closed, Ctrl+I asks for the details: open both.
        if !self.preview.open && !self.preview.peek {
            self.preview.open = true;
            self.preview.details_open = true;
        } else {
            self.preview.details_open = !self.preview.details_open;
        }
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

    /// Zoom what is looked at; size what is read. A book its engine laid out
    /// into pages (`MuPDF`'s) is read, not looked at: its steps change the text
    /// size and lay it out again, while a chosen zoom or fit still zooms its
    /// pages.
    pub fn preview_zoom(&mut self, direction: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.map_zoom(direction, cx) {
            return;
        }
        if matches!(direction, "in" | "out" | "reset") && self.reader_reflows() {
            let size = self.preview.text_size.unwrap_or(TEXT_SIZE);
            let next = match direction {
                "in" => (size + 2.).min(40.),
                "out" => (size - 2.).max(10.),
                _ => TEXT_SIZE,
            };
            if (next - size).abs() > f32::EPSILON {
                self.relayout(next, window, cx);
            }
            return;
        }
        // A step from Fit or Fit width starts at the scale they show, so
        // Zoom out never makes a large picture larger.
        let shown = self.preview.zoom.or_else(|| self.picture_scale());
        self.preview.fit_width = direction == "width";
        self.preview.zoom = match direction {
            "in" => Some(shown.map_or(1.0, |z| (z * 1.25).clamp(MIN_ZOOM, MAX_ZOOM))),
            "out" => Some(shown.map_or(0.75, |z| (z / 1.25).clamp(MIN_ZOOM, MAX_ZOOM))),
            value => value
                .parse::<f32>()
                .ok()
                .filter(|v| v.is_finite())
                .map(|v| v.clamp(MIN_ZOOM, MAX_ZOOM)),
        };
        cx.notify();
    }

    /// The scale Fit or Fit width shows the picture at: an image's, from the
    /// bounds its container had in the last prepaint; a document's pages',
    /// as they reported it (`Shell::document_shown`). Read when a key asks,
    /// never to lay anything out. `None` before it was drawn.
    fn picture_scale(&self) -> Option<f32> {
        let (scroll, (width, height)) = match &self.preview.content {
            Some(PreviewContent::Image {
                width,
                height,
                document: None,
                ..
            }) => (&self.preview.picture_scroll, (*width, *height)),
            // A document's pages report the scale they are shown at.
            Some(PreviewContent::Image {
                document: Some(_), ..
            }) => return self.preview.reader.shown_scale,
            _ => return None,
        };
        let bounds = scroll.bounds().size;
        let (across, down) = (
            f32::from(bounds.width) / width.max(1) as f32,
            f32::from(bounds.height) / height.max(1) as f32,
        );
        let scale = if self.preview.fit_width {
            across
        } else {
            // Contain never enlarges a small picture.
            across.min(down).min(1.)
        };
        (scale.is_finite() && scale > 0.).then_some(scale)
    }

    /// A picture covers more device pixels than it has (a zoom, a high-density
    /// panel): read it again with `side` pixels on its longer side, where
    /// it is. The zoom, the scroll and the page stay; the pixels are
    /// replaced when they arrive, the old ones evicted like any content's.
    pub(crate) fn sharpen_picture(
        &mut self,
        side: u32,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match &self.preview.content {
            Some(PreviewContent::Image { document: None, .. })
                if self.preview.media.session.is_none() =>
            {
                if side <= self.preview.picture_side {
                    return;
                }
                self.preview.picture_side = side;
            }
            _ => return,
        }
        tracing::debug!(side, "draw the picture sharper");
        self.cancel_pending_preview();
        self.preview.refresh = true;
        self.preview.phase = PreviewPhase::Queued;
        cx.notify();
    }

    /// Whether the reader shows a book its engine laid out into pages.
    pub(crate) fn reader_reflows(&self) -> bool {
        self.preview
            .reader
            .session
            .as_ref()
            .is_some_and(|session| session.info().reflowable)
    }

    /// Lay the book out again at text `size`, where the reader is: its
    /// pages are others now, so the reader finds what it was reading in them
    /// (`reader_anchor`, `reader_land`).
    fn relayout(&mut self, size: f32, window: &mut Window, cx: &mut Context<Self>) {
        let anchor = self.reader_anchor();
        self.reader_reopen(window, cx);
        self.preview.reader.relayout = Some(anchor);
        self.preview.text_size = Some(size);
        self.cancel_pending_preview();
        self.preview.phase = PreviewPhase::Queued;
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
                mtime: self.preview.version.as_ref().and_then(|v| v.modified),
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
        let sidebar = self.sidebar_drawn_width(window);
        let window = f32::from(window.viewport_size().width);
        max_width(window, self.panes_across(), sidebar)
    }

    /// The tallest the panel may be at the bottom: everything the stacked
    /// panes, the toolbar and the status bar do not need.
    /// How tall the panel at the bottom wants to be: as dragged, else the
    /// default with the pinned player's dock added under it, so a pin does
    /// not take its room from the preview. `preview_height_limit` bounds it.
    pub fn preview_panel_height(&self) -> f32 {
        self.preview.height.unwrap_or_else(|| {
            DEFAULT_HEIGHT
                + if self.dock_shown() {
                    self.player.height
                } else {
                    0.
                }
        })
    }

    pub fn preview_height_limit(&self, window: &Window) -> f32 {
        let window = f32::from(window.viewport_size().height);
        max_height(window, self.panes_down())
    }

    /// How many panes share the window's width, and its height.
    pub fn panes_across(&self) -> usize {
        if self.layout.stacked() {
            1
        } else {
            self.panes.len()
        }
    }

    pub fn panes_down(&self) -> usize {
        if self.layout.stacked() {
            self.panes.len()
        } else {
            1
        }
    }

    /// Whether this window has room for the sidebar beside the panes.
    pub fn sidebar_fits(&self, window: f32) -> bool {
        window >= 520. + 300. * self.panes_across().saturating_sub(1) as f32
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

    /// The top edge of the panel at the bottom follows the pointer; its
    /// bottom is the status bar's top.
    fn on_resize_height_drag(
        &mut self,
        ev: &DragMoveEvent<ResizeHeightHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let bottom = f32::from(window.viewport_size().height) - crate::status::HEIGHT;
        let y = f32::from(ev.event.position.y);
        self.preview.height =
            Some((bottom - y).clamp(MIN_HEIGHT, self.preview_height_limit(window)));
        cx.notify();
    }

    /// Root-level hook so edge drags are tracked anywhere in the window.
    pub fn resize_hook<E: InteractiveElement>(&self, el: E, cx: &mut Context<Self>) -> E {
        el.on_drag_move(cx.listener(Self::on_resize_drag))
            .on_drag_move(cx.listener(Self::on_resize_height_drag))
            .on_drag_move(cx.listener(Self::on_sidebar_drag))
            .on_drag_move(cx.listener(Self::on_dock_drag))
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

/// Everything under the header's card. A member of a container whose rows
/// are kept is shown beside them (`members::split`), and Details, which the
/// card opens, stays right under the card, above both: it is about what the
/// card names. `wide` is whether the room is wider than tall.
fn under_card(
    shell: &Shell,
    theme: &Theme,
    interactive: bool,
    wide: bool,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let kept = interactive && shell.preview.members().is_some();
    if !kept {
        return body(shell, theme, interactive, true, cx);
    }
    let details = Some(())
        .filter(|()| !crate::details::alone(shell))
        .and_then(|()| crate::details::render(shell, theme, cx));
    let body = body(shell, theme, interactive, false, cx);
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .children(details)
        .child(crate::members::split(body, wide, shell, cx))
        .into_any_element()
}

/// The toolbar and the content, under Details when `details` is set (and
/// always when there is nothing else to show).
fn body(
    shell: &Shell,
    theme: &Theme,
    interactive: bool,
    details: bool,
    cx: &mut Context<Shell>,
) -> AnyElement {
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
    // Details opens right under the card that opened it, above the toolbar
    // and the content. With nothing to preview it is the whole body instead,
    // under the toolbar and whatever the content has to say (why the preview
    // failed), so neither ends up at the bottom.
    let alone = crate::details::alone(shell);
    let mut details = Some(())
        .filter(|()| (interactive && details) || alone)
        .and_then(|()| crate::details::render(shell, theme, cx));
    if !alone {
        frame = frame.children(details.take());
    }
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
            shell.preview_busy(),
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
                .when(!alone, gpui::Styled::flex_1)
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
        .children(details)
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
/// how the content is shown and what was left out. Why there is no preview
/// at all is the body's to say (`failure`).
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
    !matches!(preview.content, Some(PreviewContent::Image { .. })) && placeholder(shell).is_none()
}

/// The thumbnail standing in for the preview, once the wait has lasted
/// (`BUSY_AFTER`) and while nothing else is shown.
fn placeholder(shell: &Shell) -> Option<&PreviewContent> {
    let preview = &shell.preview;
    preview
        .placeholder
        .as_ref()
        .filter(|_| preview.content.is_none() && shell.preview_busy())
}

fn content(shell: &Shell, theme: &Theme, interactive: bool, cx: &mut Context<Shell>) -> AnyElement {
    let p = &shell.preview;
    if p.media.session.is_some() && shell.media_is_audio() {
        return crate::media::audio_content(shell, theme, interactive, cx);
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
        // A wait that has lasted shows the file's thumbnail, where its
        // preview will stand; a shorter one shows nothing yet, so a preview
        // that comes at once never flickers through a stand-in.
        None if let Some(PreviewContent::Image {
            image,
            width,
            height,
            ..
        }) = placeholder(shell) =>
        {
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .child(if opens_as_pages(shell) {
                    crate::reader::page_placeholder(
                        Arc::clone(image),
                        (*width as f32, *height as f32),
                        crate::settings::reader_mode(shell),
                    )
                } else {
                    picture(
                        image,
                        (*width, *height),
                        (None, false),
                        &p.picture_scroll,
                        "preview-placeholder",
                        None,
                    )
                })
                .into_any_element()
        }
        None if shell.preview_busy() => div()
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .text_color(theme.text_muted)
            .child("Loading…")
            .into_any_element(),
        // Nothing to look at: the header says what it is and Details the rest.
        None | Some(PreviewContent::Generic | PreviewContent::Dir { .. }) => {
            div().into_any_element()
        }
        Some(PreviewContent::Image {
            image,
            width,
            height,
            document,
            pixels,
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
            } else {
                picture(
                    image,
                    (*width, *height),
                    (p.zoom, p.fit_width),
                    &p.picture_scroll,
                    "preview-image-scroll",
                    // A picture shown smaller than its own size was decoded
                    // smaller; one given at its pixels has no more.
                    Some(Sharpen {
                        shell: cx.entity().downgrade(),
                        asked: p.picture_side,
                        most: match pixels {
                            Pixels::Own => (*width).max(*height),
                            Pixels::Any => bibli_platform::inspection::limits::PICTURE_SIDE,
                            Pixels::Drawn => 0,
                        },
                    }),
                )
            };
            // A film answers a click like a player does.
            let picture = if p.media.session.is_some() && interactive {
                crate::media::surface(
                    "video-surface",
                    picture,
                    crate::media::Flash::Preview,
                    shell,
                    cx,
                )
            } else {
                picture
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
                                        this.preview_zoom(
                                            if up { "in" } else { "out" },
                                            window,
                                            cx,
                                        );
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
        Some(PreviewContent::Error(error)) => failure(error, theme),
    }
}

/// Why there is no preview, in the content's place; the entry itself is
/// already in the header.
fn failure(error: &str, theme: &Theme) -> AnyElement {
    div()
        .flex_none()
        .px_3()
        .py_4()
        .text_xs()
        .text_center()
        .text_color(theme.error)
        .child(error.to_owned())
        .into_any_element()
}

/// The name of what is shown. Inside a container it is a trail,
/// `bundle.zip › notes.md`, and every step before the last leads back to it,
/// unless the container's rows are kept beside the member (`members.rs`):
/// then the card is only what opens Details, and Back and the rows lead
/// elsewhere, so a click meant for Details never leaves the member.
fn trail(shell: &Shell, theme: &Theme, cx: &mut Context<Shell>) -> gpui::Div {
    let preview = &shell.preview;
    let mut line = div().min_w_0().flex_1().flex().items_center().gap_1();
    let name = |name: String| {
        div()
            .min_w_0()
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis_middle()
            .child(name)
    };
    if preview.nested.is_empty() {
        return line.child(name(preview.info.name.clone()));
    }
    let names = std::iter::once(preview.info.name.clone())
        .chain(preview.nested.iter().map(|member| member.name.clone()));
    let last = preview.nested.len();
    for (depth, name_at) in names.enumerate() {
        if depth > 0 {
            line = line.child(div().flex_none().text_color(theme.text_muted).child("›"));
        }
        line = line.child(if depth == last {
            name(name_at).into_any_element()
        } else if preview.members().is_some() {
            name(name_at)
                .flex_shrink()
                .text_color(theme.text_muted)
                .into_any_element()
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
                    cx.stop_propagation();
                    this.run(
                        "ui.inspect",
                        serde_json::json!({"action":"ascend","depth":depth}),
                        window,
                        cx,
                    );
                }))
                .child(name_at)
                .into_any_element()
        });
    }
    line
}

/// Everything the panel knows at a glance: the entry's icon, its name (a
/// trail inside a container), and one line of type, size and date. The card
/// is the one place that says what the panel shows; Details, which opens
/// right under it when the card is clicked (or Ctrl+I), holds the rest and
/// repeats none of it. With nothing to preview Details is always open and the card
/// has no chevron. The peek overlay alone keeps a close button: the panel
/// is closed by the Preview button that opened it.
fn header(shell: &Shell, theme: &Theme, cx: &mut Context<Shell>) -> AnyElement {
    let preview = &shell.preview;
    let info = &preview.info;
    let alone = crate::details::alone(shell);
    let toggles = !alone && !info.name.is_empty();
    let open = alone || preview.details_open;
    // Each fact is one unbreakable segment of a wrapping line: a narrow
    // panel wraps between the facts, never inside the date.
    let facts = [
        type_label(shell, info),
        info.size.clone(),
        info.modified.clone(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    let last = facts.len().saturating_sub(1);
    let facts = facts.into_iter().enumerate().map(|(index, fact)| {
        div().whitespace_nowrap().child(if index == last {
            fact
        } else {
            format!("{fact} ·")
        })
    });
    let card = div()
        .id("preview-info")
        .flex_1()
        .min_w_0()
        .flex()
        .items_center()
        .gap_2()
        .px_1()
        .py_1()
        .rounded_md()
        .when(toggles, |d| {
            d.cursor_pointer()
                .hover(|d| d.bg(theme.hover))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.run("ui.preview_details", serde_json::json!({}), window, cx);
                }))
        })
        .children(
            info.icon
                .as_ref()
                .map(|icon| div().flex_none().child(icons::element(icon, ICON))),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(trail(shell, theme, cx))
                .child(
                    div()
                        .id("preview-meta")
                        .flex()
                        .flex_wrap()
                        .gap_x_1()
                        .text_xs()
                        .text_color(theme.text_muted)
                        .when_some(info.mime.clone(), |d, mime| {
                            let font = shell.font.clone();
                            d.tooltip(move |_, cx| {
                                crate::toolbar::tooltip(mime.clone().into(), font.clone(), cx)
                            })
                        })
                        .children(facts),
                ),
        )
        .when(toggles, |d| {
            d.child(
                gpui::svg()
                    .path(if open {
                        "toolbar/chevron.svg"
                    } else {
                        "toolbar/chevron-right.svg"
                    })
                    .size(px(12.))
                    .flex_none()
                    .text_color(theme.text_muted),
            )
        });
    div()
        .px_1()
        .py(px(2.))
        .border_b_1()
        .border_color(theme.border)
        .flex()
        .items_center()
        .child(card)
        .when(preview.peek, |d| {
            d.child(
                crate::toolbar::Button::new(
                    "preview-close",
                    "close",
                    "Close preview",
                    "ui.preview_close",
                )
                .render(shell, cx),
            )
        })
        .into_any_element()
}

/// The header's icon: two lines of text tall.
const ICON: f32 = 36.;

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

/// The narrowest the panel gets, and the width a double click returns it to.
pub(crate) const MIN_WIDTH: f32 = 200.;
const DEFAULT_WIDTH: f32 = 320.;
/// The same at the bottom: the lowest it gets, and its height by default.
const MIN_HEIGHT: f32 = 140.;
const DEFAULT_HEIGHT: f32 = 280.;
/// What each pane keeps when the panel or the sidebar is dragged wide, or
/// its neighbour in a split is.
pub(crate) const PANE_KEEP: f32 = 240.;
/// What each stacked pane keeps when the panel is dragged tall: its own
/// row and a few of its entries.
pub(crate) const PANE_KEEP_HEIGHT: f32 = 160.;
/// The application toolbar above everything (`address::render_toolbar`).
const TOOLBAR_HEIGHT: f32 = 42.;

/// A picture as every preview shows one: whole in the panel (Fit), as wide
/// as the panel (Fit width), or at a share of its own size, scrolling in
/// both directions past the panel, with a scrollbar when it scrolls. Pages
/// of a document are the reader's (`elements::document`).
///
/// Its layout is its own size (`own`, what 100% is), never its pixels': the
/// pixels may be fewer (shrunk for display) or more (drawn again for a
/// zoom), and only fill the box. After layout, `sharpen` compares the device
/// pixels the picture covers with the ones it has and asks for more when
/// they fall short (`Shell::sharpen_picture`).
pub(crate) fn picture(
    image: &Arc<gpui::RenderImage>,
    own: (u32, u32),
    (zoom, fit_width): (Option<f32>, bool),
    scroll: &gpui::ScrollHandle,
    id: &'static str,
    sharpen: Option<Sharpen>,
) -> AnyElement {
    let (width, height) = (own.0.max(1) as f32, own.1.max(1) as f32);
    let scrolling = |content: AnyElement| {
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .w_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .id(id)
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_scroll()
                    .track_scroll(scroll)
                    .flex()
                    .flex_col()
                    .child(content),
            )
            .child(crate::elements::scrollbar::vertical(
                (id, 1usize),
                crate::elements::scrollbar::Scrolls::Offset(scroll.clone()),
            ))
            .into_any_element()
    };
    // The pixels fill the box the layout gave the picture, as large as they
    // are or not; Fit's box can be taller or wider than the picture, which
    // stays whole in it.
    let pixels = img(Arc::clone(image))
        .absolute()
        .size_full()
        .object_fit(ObjectFit::Contain);
    let raster = {
        let size = image.size(0);
        u32::try_from(size.width.0.max(size.height.0)).unwrap_or(0)
    };
    let watch = sharpen.map(|sharpen| sharpen.watch(raster, (width, height)));
    if let Some(zoom) = zoom {
        // Smaller than the panel, it sits in the middle, as whole; larger,
        // the wrapper grows with it and it scrolls from its corner.
        scrolling(
            div()
                .flex_grow()
                .flex_shrink_0()
                .min_w_full()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .relative()
                        .flex_none()
                        .w(px(width * zoom))
                        .h(px(height * zoom))
                        .child(pixels)
                        .children(watch),
                )
                .into_any_element(),
        )
    } else if fit_width {
        // As wide as the panel, as tall as its proportions say.
        scrolling(
            div()
                .relative()
                .flex_shrink_0()
                .w_full()
                .map(|mut d| {
                    d.style().aspect_ratio = Some(width / height);
                    d
                })
                .child(pixels)
                .children(watch)
                .into_any_element(),
        )
    } else {
        // Whole, and never larger than its own size. It never scrolls; it
        // sits in the same tracked container so the zoom steps know the
        // scale it is shown at (`picture_scale`).
        scrolling(
            div()
                .flex_1()
                .min_h_0()
                .w_full()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .relative()
                        .size_full()
                        .max_w(px(width))
                        .max_h(px(height))
                        .child(pixels)
                        .children(watch),
                )
                .into_any_element(),
        )
    }
}

/// Who a picture asks for more pixels (`picture`), the side it already
/// asked for, so it asks once per step, and the most it can have: its own
/// pixels, within `limits::PICTURE_SIDE`.
pub(crate) struct Sharpen {
    pub(crate) shell: gpui::WeakEntity<Shell>,
    pub(crate) asked: u32,
    pub(crate) most: u32,
}

/// Steps a picture is drawn again at: a zoom step of 1.25 crosses at most
/// one, and a picture a little short of its pixels is left as it is.
const SHARPEN_STEP: u32 = 512;

impl Sharpen {
    /// A canvas over the picture's box that, once laid out, measures the
    /// device pixels the picture covers (whole in the box, like `Contain`).
    fn watch(self, raster: u32, (width, height): (f32, f32)) -> gpui::Canvas<()> {
        gpui::canvas(
            move |bounds, window, cx| {
                let scale = (f32::from(bounds.size.width) / width)
                    .min(f32::from(bounds.size.height) / height);
                let covered = width.max(height) * scale * window.scale_factor();
                if let Some(side) = sharper(covered, raster, self.asked, self.most) {
                    let shell = self.shell.clone();
                    window.defer(cx, move |window, cx| {
                        if let Some(shell) = shell.upgrade() {
                            shell.update(cx, |shell, cx| shell.sharpen_picture(side, window, cx));
                        }
                    });
                }
            },
            |_, (), _, _| {},
        )
        .absolute()
        .size_full()
    }
}

/// The side to draw a picture again at, when `covered` device pixels show
/// one of `raster` pixels: the next step up, within `most` (what the
/// picture can have) and `limits::PICTURE_SIDE`, and above what was already
/// `asked`. `None` while its pixels suffice or can be no more.
pub(crate) fn sharper(covered: f32, raster: u32, asked: u32, most: u32) -> Option<u32> {
    use bibli_platform::inspection::limits::PICTURE_SIDE;
    let most = most.min(PICTURE_SIDE);
    if !covered.is_finite() || covered <= raster as f32 * 1.05 || raster >= most {
        return None;
    }
    let side = ((covered / SHARPEN_STEP as f32).ceil() as u32 * SHARPEN_STEP).min(most);
    (side > raster && side > asked).then_some(side)
}

/// The widest the panel may be: the window less what the panes keep and
/// the sidebar is drawn at (`sidebar`, 0 when it is not). Never a fixed
/// number: a wide window has room for a wide page.
pub fn max_width(window: f32, panes: usize, sidebar: f32) -> f32 {
    let kept = PANE_KEEP * panes.max(1) as f32 + sidebar;
    (window - kept).max(MIN_WIDTH)
}

/// The tallest the panel may be at the bottom, from the window's height the
/// same way.
pub fn max_height(window: f32, panes: usize) -> f32 {
    let kept = PANE_KEEP_HEIGHT * panes.max(1) as f32 + TOOLBAR_HEIGHT + crate::status::HEIGHT;
    (window - kept).max(MIN_HEIGHT)
}

/// The preview panel, or `None` when closed.
///
/// At the right edge (`bottom` false) it is as wide as `limit` allows
/// (`Shell::preview_width_limit`); at the bottom, as tall as `limit` allows
/// (`Shell::preview_height_limit`). Either way the pinned player's dock
/// stands at its foot, up to `dock_limit` (`Shell::dock_height_limit`), and
/// the remembered size is kept as the user left it and only drawn smaller
/// while the window is.
pub fn render_panel(
    shell: &Shell,
    bottom: bool,
    limit: f32,
    dock_limit: f32,
    cx: &mut Context<Shell>,
) -> Option<AnyElement> {
    if !shell.preview.open {
        return None;
    }
    let theme = cx.global::<Theme>().clone();
    let reset = cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
        if event.click_count() == 2 {
            if bottom {
                this.preview.height = None;
            } else {
                this.preview.width = DEFAULT_WIDTH;
            }
            this.remember_session();
            cx.notify();
        }
    });
    let handle = div()
        .id("preview-resize")
        .absolute()
        .left_0()
        .top_0()
        .hover(|style| style.bg(theme.handle))
        .on_click(reset);
    let handle = if bottom {
        handle
            .w_full()
            .h(px(6.))
            .cursor(CursorStyle::ResizeUpDown)
            .on_drag(ResizeHeightHandle, |_, _, _, cx| cx.new(|_| NoDragView))
    } else {
        handle
            .h_full()
            .w(px(6.))
            .cursor(CursorStyle::ResizeLeftRight)
            .on_drag(ResizeHandle, |_, _, _, cx| cx.new(|_| NoDragView))
    };
    // While peek shows the preview, the panel is only a picture of it: the
    // kept rows, which take the keyboard, are drawn in peek alone.
    let interactive = !shell.preview.peek;
    let body = under_card(shell, &theme, interactive, bottom, cx);
    let content = div()
        .flex()
        .flex_col()
        .child(header(shell, &theme, cx))
        .child(body);
    let panel = div()
        .id("preview-panel")
        .relative()
        .flex_none()
        .flex()
        .border_color(theme.border)
        .bg(theme.surface);
    let panel = if bottom {
        panel
            .w_full()
            .h(px(shell.preview_panel_height().min(limit)))
            .border_t_1()
    } else {
        panel
            .w(px(shell.preview.width.min(limit)))
            .h_full()
            .border_l_1()
    };
    Some(
        panel
            .flex_col()
            .child(content.flex_1().min_h_0().w_full())
            .children(crate::dock::render(shell, dock_limit, cx))
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
            .bg(theme.scrim)
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
                    .child(header(shell, &theme, cx))
                    .child(under_card(shell, &theme, true, true, cx))
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
    use super::{SHARPEN_STEP, Version, max_width, sharper};

    #[test]
    fn a_picture_is_drawn_sharper_in_steps_up_to_what_it_can_have() {
        use bibli_platform::inspection::limits::PICTURE_SIDE;
        // Its pixels suffice, or nearly.
        assert_eq!(sharper(1500., 1600, 0, u32::MAX), None);
        assert_eq!(sharper(1650., 1600, 0, u32::MAX), None);
        // Short: the next step up, once.
        assert_eq!(sharper(2100., 1600, 0, u32::MAX), Some(5 * SHARPEN_STEP));
        assert_eq!(sharper(2100., 1600, 5 * SHARPEN_STEP, u32::MAX), None);
        // Never past what the picture has, nor the bound.
        assert_eq!(sharper(2100., 1600, 0, 1800), Some(1800));
        assert_eq!(sharper(2100., 1800, 0, 1800), None);
        assert_eq!(sharper(20_000., 1600, 0, u32::MAX), Some(PICTURE_SIDE));
        assert_eq!(sharper(20_000., PICTURE_SIDE, 0, u32::MAX), None);
        assert_eq!(sharper(f32::NAN, 1600, 0, u32::MAX), None);
    }

    #[test]
    fn a_drawing_is_shown_at_its_own_size_and_drawn_at_the_side_asked() {
        use super::{PreviewContent, compute};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a drawing’s name.svg");
        std::fs::write(
            &path,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect width="40" height="20"/></svg>"#,
        )
        .unwrap();
        let cancel = bibli_core::CancelToken::new();
        for (side, drawn) in [(0, 40), (1024, 1024)] {
            let content = compute(&path, false, Some("image/svg+xml"), false, 0, side, &cancel);
            let PreviewContent::Image {
                image,
                width,
                height,
                pixels,
                ..
            } = content
            else {
                panic!("a drawing is a picture");
            };
            assert_eq!((width, height, pixels), (40, 20, super::Pixels::Any));
            assert_eq!(image.size(0).width.0, drawn);
        }
    }

    #[test]
    fn metadata_learnt_late_is_not_a_changed_file() {
        let at = |modified: Option<u64>, size: Option<u64>, dark: bool| Version {
            modified: modified.map(|s| std::time::UNIX_EPOCH + std::time::Duration::from_secs(s)),
            size,
            dark,
        };
        assert!(!at(None, None, false).rewritten(&at(Some(5), Some(9), false)));
        assert!(at(Some(5), Some(9), false).rewritten(&at(Some(6), Some(9), false)));
        assert!(at(Some(5), Some(9), false).rewritten(&at(Some(5), Some(10), false)));
        assert!(!at(Some(5), Some(9), false).rewritten(&at(Some(5), Some(9), true)));
    }

    #[test]
    fn the_panel_may_take_what_the_panes_do_not_need() {
        // One pane and a 200 px sidebar keep 440 px of a wide window.
        assert!((max_width(2560., 1, 200.) - 2120.).abs() < 1e-3);
        // Two panes keep twice a pane; without the sidebar, only the panes.
        assert!((max_width(1600., 2, 200.) - 920.).abs() < 1e-3);
        assert!((max_width(1600., 2, 0.) - 1120.).abs() < 1e-3);
        // A sidebar dragged wider takes its width from the panel.
        assert!((max_width(1600., 2, 360.) - 760.).abs() < 1e-3);
        // A small window still leaves the panel its minimum.
        assert!((max_width(500., 2, 0.) - 200.).abs() < 1e-3);
    }
}
