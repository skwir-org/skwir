//! Keep playing: an audio preview pinned so that it outlives the selection.
//!
//! A preview dies with the selection, and its media session with it. Pinned,
//! the session belongs to the window instead: browsing goes on, the music
//! (or the film) too, and the dock at the foot of the preview panel
//! (`dock.rs`) holds it: cover or picture, visualizer, transport. With the
//! panel closed the last pane's footer keeps a strip of the controls instead. The
//! panel shows the same session again whenever its file is selected, and
//! the dock steps aside while it does; there is never a second player for
//! one file, and never two playing at once.
//!
//! Whichever plays, pinned or previewed, is published as core's
//! `playing::NowPlaying`, and what others ask of it (`PlayerRequest`: the
//! desktop's media keys through a plugin, a script, an agent) is taken here
//! when the shell wakes. Nothing calls into the UI from another thread.
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use bibli_core::media::{Control, MediaSession, Playback};
use bibli_core::playing::{NowPlaying, PlayerRequest};
use bibli_core::preview::PreviewRequest;
use bibli_core::{CancelToken, op::EntryRef, scheduler::Priority};
use gpui::{AnyElement, Context, RenderImage, Window, div, prelude::*, px};

use crate::shell::Shell;
use crate::theme::Theme;
use crate::toolbar::Button;

/// What follows a track that plays to its end (`preview-media.continue`,
/// set with `ui.media_continue`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Continue {
    /// Nothing: the player stops.
    #[default]
    Stop,
    /// The next file of the same kind in the pane's order, or of the playlist.
    Next,
    /// One of them at random. The pin draws an order once and walks it, so
    /// Previous goes back the way Next came.
    Shuffle,
}

impl Continue {
    pub const ALL: [Self; 3] = [Self::Stop, Self::Next, Self::Shuffle];

    /// The setting's value.
    pub const fn key(self) -> &'static str {
        match self {
            Self::Stop => "off",
            Self::Next => "next",
            Self::Shuffle => "shuffle",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Stop => "Stop",
            Self::Next => "Play next",
            Self::Shuffle => "Shuffle",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Shuffle => "shuffle",
            Self::Stop | Self::Next => "play-next",
        }
    }

    pub fn parse(key: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|mode| mode.key() == key)
            .unwrap_or_default()
    }

    /// The one after, for a command given no mode: Stop, Next, Shuffle.
    pub fn next(self) -> Self {
        match self {
            Self::Stop => Self::Next,
            Self::Next => Self::Shuffle,
            Self::Shuffle => Self::Stop,
        }
    }
}

/// The player's preferences (`Shell::prefs`), declared by the audio and
/// video plugin and read with the others when the settings change.
#[derive(Clone, Copy, Debug)]
pub struct PlayerPrefs {
    /// `preview-media.keep_playing`: a track that starts to play is pinned.
    pub keep_playing: bool,
    /// `preview-media.autoplay`: audio and video play once selected.
    pub autoplay: bool,
    /// `preview-media.continue`: what follows a track that ends.
    pub continue_with: Continue,
}

impl Default for PlayerPrefs {
    fn default() -> Self {
        Self {
            keep_playing: crate::settings::KEEP_PLAYING_DEFAULT,
            autoplay: crate::settings::AUTOPLAY_DEFAULT,
            continue_with: Continue::Stop,
        }
    }
}

/// A number below `bound`, which must not be 0. `RandomState` draws fresh
/// keys for every instance, so a hash of nothing under them is random
/// enough to shuffle a folder without a dependency for it.
pub(crate) fn random_below(bound: usize) -> usize {
    use std::hash::BuildHasher;
    let draw = std::hash::RandomState::new().hash_one(());
    usize::try_from(draw % bound as u64).unwrap_or(0)
}

/// The indices `0..len` in a random order that starts with `first`.
fn shuffled(len: usize, first: usize) -> Vec<usize> {
    let mut rest: Vec<usize> = (0..len).filter(|index| *index != first).collect();
    for end in (1..rest.len()).rev() {
        rest.swap(end, random_below(end + 1));
    }
    (first < len)
        .then_some(first)
        .into_iter()
        .chain(rest)
        .collect()
}

/// The track `delta` steps from `index` in a queue of `len`: along `order`
/// while shuffling (when it is drawn over the whole queue), otherwise along
/// the queue.
fn step(len: usize, order: &[usize], index: usize, delta: i64, mode: Continue) -> Option<usize> {
    let delta = delta.signum() as isize;
    if mode == Continue::Shuffle && order.len() == len {
        let at = order.iter().position(|track| *track == index)?;
        return at
            .checked_add_signed(delta)
            .and_then(|at| order.get(at).copied());
    }
    index.checked_add_signed(delta).filter(|index| *index < len)
}

/// Where, under the cache and this process's id, the covers of what plays
/// are written for others to show (the desktop's media widget).
pub const COVERS: &str = "player-covers";

pub struct Pinned {
    pub session: Arc<dyn MediaSession>,
    /// The track the session was last asked to go on with, whether or not
    /// it could: it is asked again only when the queue changes, not on
    /// every wake after a refusal.
    queued: Option<usize>,
    /// Ends the worker when the pin goes.
    lifetime: CancelToken,
    pub state: Playback,
    pub path: PathBuf,
    /// The tracks Next and Previous walk: the folder's, or the playlist's.
    pub queue: Vec<PathBuf>,
    pub index: usize,
    /// The order a shuffle walks `queue` in, as indices; empty unless
    /// shuffling (`Continue::Shuffle`).
    order: Vec<usize>,
    /// A film rather than music: the dock shows its frames, not a cover.
    pub video: bool,
    /// The cover of the track, for the dock: the panel's picture when
    /// pinned, then whatever the cover worker finds for each track.
    pub cover: Option<Arc<RenderImage>>,
    /// The film's last frame, with its size.
    pub frame: Option<(Arc<RenderImage>, u32, u32)>,
    /// Where the seek bar's knob is being dragged to, in seconds.
    pub scrub: Option<f64>,
    /// The track's file icon, in the place of a cover it has none of: the
    /// icon of the track it was looked up for, so it follows the track.
    pub icon: Option<(PathBuf, Option<crate::icons::Pic>)>,
}

impl Drop for Pinned {
    fn drop(&mut self) {
        self.lifetime.cancel();
    }
}

impl Pinned {
    /// Show `cover`, the one it replaces given back (`PlayerState::retired`).
    fn set_cover(&mut self, cover: Option<Arc<RenderImage>>, retired: &mut Vec<Arc<RenderImage>>) {
        retired.extend(std::mem::replace(&mut self.cover, cover));
    }

    /// Show `frame`, the one it replaces given back.
    fn set_frame(
        &mut self,
        frame: Option<(Arc<RenderImage>, u32, u32)>,
        retired: &mut Vec<Arc<RenderImage>>,
    ) {
        retired.extend(std::mem::replace(&mut self.frame, frame).map(|(image, ..)| image));
    }

    /// The track `delta` steps away in the order played: the shuffle's
    /// while shuffling, otherwise the queue's own.
    pub fn step_target(&self, delta: i64, mode: Continue) -> Option<usize> {
        step(self.queue.len(), &self.order, self.index, delta, mode)
    }

    pub fn can_step(&self, delta: i64, mode: Continue) -> bool {
        self.step_target(delta, mode).is_some()
    }

    /// The track to go on with when this one ends.
    fn upcoming(&self, mode: Continue) -> Option<usize> {
        self.step_target(1, mode).filter(|_| mode != Continue::Stop)
    }

    /// Draw a shuffle that starts with the track playing, or forget it.
    fn reorder(&mut self, mode: Continue) {
        self.order = if mode == Continue::Shuffle {
            shuffled(self.queue.len(), self.index)
        } else {
            Vec::new()
        };
    }

    /// A shuffle drawn while shuffling and only then, over the queue as it is.
    fn keep_order(&mut self, mode: Continue) {
        let shuffling = mode == Continue::Shuffle;
        if shuffling == self.order.is_empty() || (shuffling && self.order.len() != self.queue.len())
        {
            self.reorder(mode);
        }
    }

    /// The session went on to `path` by itself.
    fn advance(&mut self, path: &std::path::Path) {
        if let Some(index) = self.queue.iter().position(|track| track == path) {
            self.index = index;
            self.path = self.queue[index].clone();
            self.queued = None;
        }
    }
}

/// A track opened for the pin, back from its worker.
pub struct Opened {
    generation: u64,
    /// The track opened: the queue may have changed since it was asked for.
    path: PathBuf,
    lifetime: CancelToken,
    session: anyhow::Result<Option<Arc<dyn MediaSession>>>,
}

/// The shell's part of the pin that is not the pin itself.
pub struct PlayerState {
    /// Set and cleared through `Shell::set_pin` only, which starts a new
    /// generation: a track opened for one pin never lands on another.
    pub pinned: Option<Pinned>,
    generation: u64,
    tx: flume::Sender<Opened>,
    rx: flume::Receiver<Opened>,
    /// A playlist's tracks, resolved on a worker after the pin.
    queue_tx: flume::Sender<(PathBuf, Vec<PathBuf>)>,
    queue_rx: flume::Receiver<(PathBuf, Vec<PathBuf>)>,
    /// The track whose cover was asked for, and the file it was written to
    /// once a worker has done so.
    cover: Option<(PathBuf, Option<PathBuf>)>,
    cover_tx: flume::Sender<(PathBuf, PathBuf, bibli_core::thumbs::Image)>,
    cover_rx: flume::Receiver<(PathBuf, PathBuf, bibli_core::thumbs::Image)>,
    /// The dock's height, as dragged; `dock::COMPACT` until it is.
    pub height: f32,
    /// The colour of the cover shown, by image: worked out once per cover.
    pub tint: std::cell::Cell<Option<(gpui::ImageId, Option<gpui::Hsla>)>>,
    /// The dock's answer to a click on its picture (`media::surface`).
    pub flash: Option<(u64, bool)>,
    /// The dock asked for fullscreen, which is the panel's: granted once the
    /// panel shows the pinned film.
    pub fullscreen_wanted: bool,
    pub focus: Option<gpui::FocusHandle>,
    /// Whether anything was published, to say once that nothing plays.
    published: bool,
    /// The previewed track that has actually played. Selecting a song is not
    /// playing it: a file manager announces itself as a player only once it
    /// is one, and stops being one when told to stop.
    engaged: Option<PathBuf>,
    /// Whether the preview was playing when last looked at: it is starting to
    /// play that engages, so a Stop still being carried out does not undo itself.
    was_playing: bool,
    /// Pictures the dock drew and will not draw again (a cover, a film's
    /// frame, a pin let go of): given back to the atlas at the next sync,
    /// where a window is at hand (`drop_image` runs in update code, never
    /// while drawing). A film's frames would otherwise stay in it, a
    /// texture each, for as long as it plays.
    retired: Vec<Arc<RenderImage>>,
}

impl Default for PlayerState {
    fn default() -> Self {
        let (tx, rx) = flume::unbounded();
        let (queue_tx, queue_rx) = flume::unbounded();
        let (cover_tx, cover_rx) = flume::unbounded();
        PlayerState {
            cover: None,
            cover_tx,
            cover_rx,
            retired: Vec::new(),
            pinned: None,
            generation: 0,
            tx,
            rx,
            queue_tx,
            queue_rx,
            published: false,
            engaged: None,
            was_playing: false,
            height: crate::dock::COMPACT,
            tint: std::cell::Cell::new(None),
            flash: None,
            fullscreen_wanted: false,
            focus: None,
        }
    }
}

fn same(a: &Arc<dyn MediaSession>, b: &Arc<dyn MediaSession>) -> bool {
    std::ptr::addr_eq(Arc::as_ptr(a), Arc::as_ptr(b))
}

impl Shell {
    /// Pin `pin`, or let go of the pin: whatever was being opened for the
    /// old one is now late. Returns the pin that was.
    fn set_pin(&mut self, pin: Option<Pinned>) -> Option<Pinned> {
        self.player.generation += 1;
        // Full screen was asked for the film pinned, not whatever follows.
        self.player.fullscreen_wanted = false;
        let mut old = std::mem::replace(&mut self.player.pinned, pin);
        if let Some(old) = &mut old {
            old.set_cover(None, &mut self.player.retired);
            old.set_frame(None, &mut self.player.retired);
        }
        old
    }

    /// Let go of a pin whose session ended under it (`sync_media`).
    pub(crate) fn unpin_dead(&mut self) {
        if let Some(pinned) = self.set_pin(None) {
            pinned.lifetime.cancel();
        }
    }

    /// Whether the panel shows the pinned session itself.
    pub fn preview_is_pinned(&self) -> bool {
        match (&self.player.pinned, &self.preview.media.session) {
            (Some(pinned), Some(session)) => same(&pinned.session, session),
            _ => false,
        }
    }

    /// `ui.media_pin`: keep the track in the panel playing when the selection
    /// moves, or let go of it again.
    pub fn media_pin(&mut self, cx: &mut Context<Self>) {
        if self.preview_is_pinned() {
            self.unpin_preview();
            return cx.notify();
        }
        self.pin_preview(cx);
    }

    /// The panel keeps the pinned session; it is a preview again, and ends
    /// with the selection.
    fn unpin_preview(&mut self) {
        if let Some(mut pinned) = self.set_pin(None) {
            self.preview
                .media
                .adopt_lifetime(std::mem::take(&mut pinned.lifetime));
        }
    }

    /// Playback starts in the panel, and one player plays at a time: with
    /// Keep playing on, the track is pinned, which moves the pin from any
    /// other; off, another pinned track stops.
    pub(crate) fn media_started(&mut self, cx: &mut Context<Self>) {
        if self.preview_is_pinned() {
            return;
        }
        if self.prefs.player.keep_playing {
            self.pin_preview(cx);
        } else if self.set_pin(None).is_some() {
            cx.notify();
        }
    }

    /// Pin the track in the panel: its session belongs to the window now.
    fn pin_preview(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.preview.media.session.clone() else {
            return;
        };
        let Some(path) = self.preview.playing_path() else {
            return;
        };
        let video = !self.media_is_audio();
        let (queue, index) = self.pin_queue(&path, video);
        let picture = match &self.preview.content {
            Some(crate::preview::PreviewContent::Image {
                image,
                width,
                height,
                ..
            }) => Some((Arc::clone(image), *width, *height)),
            _ => None,
        };
        let mut pin = Pinned {
            session,
            queued: None,
            lifetime: self.preview.media.release_lifetime(),
            state: self.preview.media.state.clone(),
            path,
            queue,
            index,
            order: Vec::new(),
            video,
            cover: picture
                .as_ref()
                .filter(|_| !video)
                .map(|(image, ..)| Arc::clone(image)),
            frame: picture.filter(|_| video),
            scrub: None,
            icon: None,
        };
        // A step in the same folder keeps the shuffle it is walking.
        match self.player.pinned.as_ref() {
            Some(old) if old.queue == pin.queue && !old.order.is_empty() => {
                pin.order.clone_from(&old.order);
            }
            _ => pin.reorder(self.prefs.player.continue_with),
        }
        self.set_pin(Some(pin));
        // A film needs room to be seen; music is fine in a row.
        if video && self.player.height < crate::dock::TALL_FROM {
            self.player.height = crate::dock::VIDEO_HEIGHT;
        }
        self.player.focus.get_or_insert_with(|| cx.focus_handle());
        self.remember_session();
        cx.notify();
    }

    /// `ui.media_keep_playing`: the preference, and the track shown follows
    /// it at once: pinned if it plays, a preview again otherwise.
    pub fn set_keep_playing(&mut self, on: bool, cx: &mut Context<Self>) {
        self.prefs.player.keep_playing = on;
        crate::settings::remember(self, crate::settings::KEEP_PLAYING_KEY, on.to_string(), cx);
        if on && self.preview.media.state.playing() {
            self.media_started(cx);
        } else if !on && self.preview_is_pinned() {
            self.unpin_preview();
        }
        cx.notify();
    }

    /// `ui.media_autoplay`: audio and video play once selected.
    pub fn set_autoplay(&mut self, on: bool, cx: &mut Context<Self>) {
        self.prefs.player.autoplay = on;
        crate::settings::remember(self, crate::settings::AUTOPLAY_KEY, on.to_string(), cx);
        cx.notify();
    }

    /// `ui.media_continue`: what follows a track that ends. Choosing Shuffle
    /// draws a new order; the popup closes on its choice.
    pub fn set_continue(&mut self, mode: Continue, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(popup) = self
            .preview
            .menu
            .take_if(|popup| popup.kind == crate::media::CONTINUE_POPUP)
        {
            popup.restore(window);
        }
        self.prefs.player.continue_with = mode;
        crate::settings::remember(
            self,
            crate::settings::CONTINUE_KEY,
            mode.key().to_owned(),
            cx,
        );
        if let Some(pinned) = &mut self.player.pinned {
            pinned.reorder(mode);
        }
        cx.notify();
    }

    /// The player's preferences, from the settings or a change not saved yet.
    pub(crate) fn read_player_prefs(&mut self) {
        use crate::settings::{AUTOPLAY_KEY, CONTINUE_KEY, KEEP_PLAYING_KEY, preference};
        let defaults = PlayerPrefs::default();
        self.prefs.player.keep_playing =
            preference(self, KEEP_PLAYING_KEY).map_or(defaults.keep_playing, |on| on == "true");
        self.prefs.player.autoplay =
            preference(self, AUTOPLAY_KEY).map_or(defaults.autoplay, |on| on == "true");
        self.prefs.player.continue_with = preference(self, CONTINUE_KEY)
            .map_or_else(Continue::default, |mode| Continue::parse(&mode));
    }

    /// Select `path` in the pane the panel previews, when it is listed there:
    /// the panel then shows it, as it shows any selection.
    pub(crate) fn select_track(
        &mut self,
        path: &std::path::Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.preview.nested.is_empty() {
            // Inside a playlist the panel shows its member, not the pane's.
            return;
        }
        let (Some(pane), Some(id)) = (self.preview.pane(), self.listed(path)) else {
            return;
        };
        self.with_pane(pane, |this| {
            if let Some(index) = this.ui().list.index_of(id) {
                this.cursor_to(index as isize, crate::list::Step::Select, window, cx);
            }
        });
    }

    /// The tracks a pin walks: the files of the same kind (music, or films)
    /// in the folder, known at once; a playlist's are resolved by its
    /// provider on a worker and arrive later.
    fn pin_queue(&mut self, path: &std::path::Path, video: bool) -> (Vec<PathBuf>, usize) {
        if let Some(queue) = &self.preview.queue
            && let Some(request) = self.preview.request_at(queue.depth)
        {
            let (previews, tx, wake) = (
                Arc::clone(&self.app.previews),
                self.player.queue_tx.clone(),
                self.app.waker_handle(),
            );
            let (targets, current) = (queue.targets.clone(), path.to_owned());
            let members = crate::preview::members_dir(self.app.cache.root());
            let cancel = self.app.lifetime().child();
            let job = cancel.clone();
            self.app
                .scheduler
                .spawn(Priority::Normal, "player-queue", cancel, move || {
                    // Only files of their own are taken, so nothing should be
                    // written; a directory of its own, gone with the job,
                    // makes sure of it.
                    let Ok(scratch) = std::fs::create_dir_all(&members).and_then(|()| {
                        tempfile::Builder::new()
                            .prefix("queue-")
                            .tempdir_in(&members)
                    }) else {
                        return;
                    };
                    let paths = targets
                        .iter()
                        .map_while(|target| {
                            (!job.is_cancelled()).then(|| {
                                let member =
                                    previews.extract(&request, target, scratch.path(), &job);
                                member.ok().flatten().filter(|m| m.own_file).map(|m| m.path)
                            })
                        })
                        .flatten()
                        .collect();
                    if !job.is_cancelled() {
                        let _ = tx.send((current, paths));
                        wake();
                    }
                });
            return (vec![path.to_owned()], 0);
        }
        let sorted = self.ui().list.sorted.clone();
        let ws = self.app.workspace.read();
        let Some(pane) = self.preview.pane().and_then(|pane| ws.pane(pane)) else {
            return (vec![path.to_owned()], 0);
        };
        let queue: Vec<PathBuf> = sorted
            .iter()
            .filter_map(|id| {
                let entry = pane.entry(*id)?;
                let kind = if video { "video/" } else { "audio/" };
                crate::media::is_track(entry, kind)
                    .then(|| pane.path_of(*id))
                    .flatten()
            })
            .take(Self::QUEUE_TRACKS)
            .collect();
        match queue.iter().position(|track| track == path) {
            Some(index) => (queue, index),
            None => (vec![path.to_owned()], 0),
        }
    }

    /// A queue is a folder's worth of music, not a disk's.
    pub(crate) const QUEUE_TRACKS: usize = 5000;

    /// Open track `index` of the pin's queue on a worker.
    fn pinned_open(&mut self, index: usize) {
        let Some(path) = self
            .player
            .pinned
            .as_ref()
            .and_then(|pinned| pinned.queue.get(index).cloned())
        else {
            return;
        };
        let generation = self.player.generation;
        let (previews, tx, wake) = (
            Arc::clone(&self.app.previews),
            self.player.tx.clone(),
            self.app.waker_handle(),
        );
        self.app.scheduler.spawn(
            Priority::Visible,
            "player-open",
            self.app.lifetime().child(),
            move || {
                let lifetime = CancelToken::new();
                // A link plays, and is typed as, what it leads to.
                let session = bibli_core::fs::content_path(&path)
                    .and_then(|content| Some((EntryRef::from_path(&*content)?, content)))
                    .ok_or_else(|| anyhow::anyhow!("{} is no file to play", path.display()))
                    .and_then(|(entry, content)| {
                        let request = PreviewRequest {
                            entry,
                            mime: bibli_platform::mime::by_name(content.as_os_str())
                                .map(|mime| mime.to_string()),
                            page: 1,
                            seconds: None,
                            max_side: 0,
                            text_size: None,
                        };
                        previews.open_media(&request, &lifetime, wake.clone())
                    });
                let _ = tx.send(Opened {
                    generation,
                    path,
                    lifetime,
                    session,
                });
                wake();
            },
        );
    }

    /// Step the pin's queue. Without a pin, or while the panel shows it, the
    /// panel steps the selection: the list shows what plays, and the panel
    /// never holds a session the pin has let go of.
    pub fn player_step(&mut self, delta: i64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pinned) = self
            .player
            .pinned
            .as_ref()
            .filter(|_| !self.preview_is_pinned())
        else {
            return self.media_step(delta, window, cx);
        };
        if let Some(index) = pinned.step_target(delta, self.prefs.player.continue_with) {
            // A newer step supersedes one still opening.
            self.player.generation += 1;
            self.pinned_open(index);
        }
    }

    /// Write the cover of `track` where others can read it, on a worker.
    fn request_cover(&mut self, track: PathBuf) {
        self.player.cover = Some((track.clone(), None));
        let (previews, tx, wake) = (
            Arc::clone(&self.app.previews),
            self.player.cover_tx.clone(),
            self.app.waker_handle(),
        );
        let directory = self
            .app
            .cache
            .root()
            .join(COVERS)
            .join(std::process::id().to_string());
        self.app.scheduler.spawn(
            Priority::Background,
            "player-cover",
            self.app.lifetime().child(),
            move || {
                let cover = || -> Option<(PathBuf, bibli_core::thumbs::Image)> {
                    let content = bibli_core::fs::content_path(&track)?;
                    let request = PreviewRequest {
                        entry: EntryRef::from_path(&*content)?,
                        mime: bibli_platform::mime::by_name(content.as_os_str())
                            .map(|mime| mime.to_string()),
                        page: 1,
                        seconds: None,
                        max_side: 512,
                        text_size: None,
                    };
                    let image = previews.render(&request, &CancelToken::new()).ok()??.image;
                    let picture =
                        image::RgbaImage::from_raw(image.width, image.height, image.rgba.to_vec())?;
                    std::fs::create_dir_all(&directory).ok()?;
                    // One file per track: a widget that caches by location
                    // must see a new one when the track changes.
                    let mut hasher = std::collections::hash_map::DefaultHasher::new();
                    track.hash(&mut hasher);
                    let file = directory.join(format!("{:016x}.png", hasher.finish()));
                    picture.save(&file).ok()?;
                    Some((file, image))
                };
                if let Some((file, image)) = cover() {
                    let _ = tx.send((track, file, image));
                    wake();
                }
            },
        );
    }

    /// Once per wake: tracks that finished opening, the pin's state, what
    /// others asked for, and the record of what plays.
    pub fn sync_player(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for image in self.player.retired.drain(..) {
            cx.drop_image(image, Some(window));
        }
        while let Ok((track, file, image)) = self.player.cover_rx.try_recv() {
            if let Some((wanted, cover)) = &mut self.player.cover
                && *wanted == track
            {
                *cover = Some(file);
            }
            if let Some(pinned) = &mut self.player.pinned
                && pinned.path == track
                && !pinned.video
            {
                pinned.set_cover(
                    crate::thumbs::to_render_image(&image),
                    &mut self.player.retired,
                );
                cx.notify();
            }
        }
        while let Ok((current, paths)) = self.player.queue_rx.try_recv() {
            if let Some(pinned) = &mut self.player.pinned
                && pinned.path == current
                && let Some(index) = paths.iter().position(|path| *path == current)
            {
                (pinned.queue, pinned.index) = (paths, index);
                pinned.reorder(self.prefs.player.continue_with);
                cx.notify();
            }
        }
        while let Ok(opened) = self.player.rx.try_recv() {
            if opened.generation != self.player.generation {
                opened.lifetime.cancel();
                continue;
            }
            let Some(pinned) = &mut self.player.pinned else {
                opened.lifetime.cancel();
                continue;
            };
            match opened.session {
                Ok(Some(session)) => {
                    let _ = session.control(Control::Volume(pinned.state.volume));
                    let _ = session.control(Control::Play(true));
                    let old = std::mem::replace(&mut pinned.lifetime, opened.lifetime);
                    old.cancel();
                    pinned.session = session;
                    pinned.queued = None;
                    if let Some(index) = pinned.queue.iter().position(|t| *t == opened.path) {
                        pinned.index = index;
                    }
                    pinned.path = opened.path;
                    pinned.state = Playback {
                        volume: pinned.state.volume,
                        ..Playback::default()
                    };
                    // Another track: its own cover comes when found.
                    pinned.set_cover(None, &mut self.player.retired);
                    pinned.set_frame(None, &mut self.player.retired);
                }
                Ok(None) => {
                    opened.lifetime.cancel();
                    self.set_status("This track cannot be played here", true, cx);
                }
                Err(error) => {
                    opened.lifetime.cancel();
                    self.set_status(format!("{error:#}"), true, cx);
                }
            }
            cx.notify();
        }

        // The panel pulls the updates of a session it shows; otherwise the
        // pin does. One reader, so no update is split between two.
        let shown = self.preview_is_pinned();
        let mode = self.prefs.player.continue_with;
        let mut finished = false;
        let mut failed = None;
        let mut follow = None;
        if let Some(pinned) = &mut self.player.pinned {
            pinned.keep_order(mode);
            // Gapless: the session is told the next track ahead of time and
            // switches to it at the sample boundary; the pin only follows.
            let next = pinned.upcoming(mode);
            if pinned.queued != next {
                pinned.queued = next;
                if let Err(error) = pinned
                    .session
                    .play_next(next.map(|index| pinned.queue[index].clone()))
                {
                    // It opens the next track itself at the end, with a gap.
                    tracing::debug!("no gapless hand-over: {error:#}");
                }
            }
            if shown {
                pinned.state = self.preview.media.state.clone();
                if let Some(path) = self.preview.media.advanced.take() {
                    pinned.advance(&path);
                    follow = Some(pinned.path.clone());
                }
            } else {
                let update = pinned.session.take_update();
                if let Some(state) = update.state {
                    finished = state.ended() && !pinned.state.ended();
                    pinned.state = state;
                    cx.notify();
                }
                if let Some(path) = update.advanced {
                    pinned.advance(&path);
                    pinned.set_frame(None, &mut self.player.retired);
                    cx.notify();
                }
                // A film's frames go to the dock while the panel shows
                // something else; the slot is bounded by the worker.
                if let Some(frame) = update
                    .frame
                    .filter(|_| pinned.video)
                    .and_then(crate::media::frame_image)
                {
                    pinned.set_frame(Some(frame), &mut self.player.retired);
                    cx.notify();
                }
                failed = update.error;
            }
        }
        // The icon follows the track, whichever way the track changed.
        if let Some(pinned) = &mut self.player.pinned
            && pinned
                .icon
                .as_ref()
                .is_none_or(|(of, _)| *of != pinned.path)
        {
            let icon = pinned
                .path
                .file_name()
                .and_then(|name| self.icons.for_name(name, 48));
            pinned.icon = Some((pinned.path.clone(), icon));
        }
        if shown && std::mem::take(&mut self.player.fullscreen_wanted) {
            self.media_fullscreen(window, cx);
        }
        // The panel showed the track that ended: it shows the next, as a
        // step would, and the list selects it.
        if let Some(path) = follow {
            self.select_track(&path, window, cx);
        }
        if let Some(error) = failed {
            self.set_pin(None);
            self.set_status(error, true, cx);
        } else if finished && mode != Continue::Stop {
            self.player_step(1, window, cx);
        }

        for request in self.app.player.take_requests() {
            self.player_request(request, window, cx);
        }
        // The dock's More goes with the dock: it steers the pin from there.
        if !self.dock_shown()
            && self
                .toolbar_menu
                .as_ref()
                .is_some_and(|menu| menu.kind == crate::dock::MENU)
        {
            self.close_popup(window);
            cx.notify();
        }
        self.publish_now_playing();
    }

    /// ↑ and ↓ over the player: the volume, a step at a time, of whatever
    /// the transport keys steer (the pinned track, else the panel's audio).
    pub fn player_volume_by(&mut self, delta: f64, window: &mut Window, cx: &mut Context<Self>) {
        let Some((_, state)) = self.playing() else {
            return;
        };
        let volume = (state.volume + delta).clamp(0., 1.);
        self.player_request(PlayerRequest::Volume(volume), window, cx);
    }

    /// The dock's seek bar being dragged: the knob follows the pointer and
    /// the session is told where on release.
    pub fn player_scrub(&mut self, phase: &str, fraction: f32, cx: &mut Context<Self>) {
        let Some(pinned) = &mut self.player.pinned else {
            return;
        };
        let Some(duration) = pinned.state.duration.filter(|_| pinned.state.seekable) else {
            return;
        };
        let seconds = f64::from(fraction.clamp(0., 1.)) * duration;
        if let Some(seconds) = crate::media::scrub(&mut pinned.scrub, Some(phase), seconds) {
            pinned.state.position = seconds;
            let _ = pinned.session.control(Control::Seek(seconds));
        }
        cx.notify();
    }

    /// The session requests go to, and its state: the pin's, else the panel's
    /// when it plays audio.
    fn playing(&self) -> Option<(Arc<dyn MediaSession>, &Playback)> {
        if let Some(pinned) = &self.player.pinned {
            return Some((Arc::clone(&pinned.session), &pinned.state));
        }
        let session = self.preview.media.session.as_ref()?;
        self.media_is_audio()
            .then(|| (Arc::clone(session), &self.preview.media.state))
    }

    pub fn player_request(
        &mut self,
        request: PlayerRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((session, state)) = self.playing() else {
            return;
        };
        let (position, playing, seekable) = (state.position, state.playing(), state.seekable);
        let control = match request {
            PlayerRequest::Play => Some(Control::Play(true)),
            PlayerRequest::Pause => Some(Control::Play(false)),
            PlayerRequest::PlayPause => Some(Control::Play(!playing)),
            PlayerRequest::SeekBy(seconds) if seekable => {
                Some(Control::Seek((position + seconds).max(0.)))
            }
            PlayerRequest::SeekTo(seconds) if seekable => Some(Control::Seek(seconds)),
            PlayerRequest::Volume(volume) => Some(Control::Volume(volume)),
            PlayerRequest::Mute => Some(Control::Mute(!state.muted)),
            // Rate changes are seeks; a stream that cannot seek keeps 1×.
            PlayerRequest::Rate(rate) if seekable => Some(Control::Rate(rate)),
            PlayerRequest::Audio(track) => Some(Control::Audio(track)),
            PlayerRequest::Subtitle(track) => Some(Control::Subtitle(track)),
            PlayerRequest::SeekBy(_) | PlayerRequest::SeekTo(_) | PlayerRequest::Rate(_) => None,
            PlayerRequest::Fullscreen => return self.player_fullscreen(window, cx),
            PlayerRequest::Next => return self.player_step(1, window, cx),
            PlayerRequest::Previous => return self.player_step(-1, window, cx),
            PlayerRequest::Stop => {
                let _ = session.control(Control::Play(false));
                self.player.engaged = None;
                if self.preview_is_pinned() {
                    // The panel shows this very session: it goes back to
                    // being a preview rather than ending under the user.
                    self.unpin_preview();
                    return cx.notify();
                }
                self.set_pin(None);
                return cx.notify();
            }
            PlayerRequest::Raise => {
                window.activate_window();
                return self.player_reveal(window, cx);
            }
        };
        if let Some(control) = control.filter(Control::valid) {
            if self.player.pinned.is_none() {
                // The panel's player, the panel's way: starting pins it.
                return self.media_send(control, cx);
            }
            if let Err(error) = session.control(control) {
                self.set_status(error.to_string(), true, cx);
            }
            cx.notify();
        }
    }

    /// A film on the whole screen, which is the panel's: one the panel
    /// shows goes there at once; a pinned one the dock shows is shown in
    /// the panel first (its folder, the file selected) and goes there once
    /// the panel shows it (`fullscreen_wanted`). Music has nothing to show.
    pub fn player_fullscreen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.preview.media.fullscreen.is_some() || self.preview_is_pinned() {
            if !self.media_is_audio() {
                self.media_fullscreen(window, cx);
            }
            return;
        }
        if self
            .player
            .pinned
            .as_ref()
            .is_some_and(|pinned| pinned.video)
            && self.preview.open
        {
            self.player.fullscreen_wanted = true;
            self.player_reveal(window, cx);
        }
    }

    /// Show the playing track in the pane: its folder, and the file selected.
    pub fn player_reveal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(path) = self
            .player
            .pinned
            .as_ref()
            .map(|pinned| pinned.path.clone())
        {
            self.reveal_path(self.pane, &path, window, cx);
        }
    }

    fn publish_now_playing(&mut self) {
        let pinned = self.player.pinned.is_some();
        if !pinned {
            let path = self.preview.playing_path();
            let playing = self.preview.media.state.playing() && self.media_is_audio();
            if playing && !self.player.was_playing {
                self.player.engaged = path;
            } else if self.player.engaged != path {
                self.player.engaged = None;
            }
            self.player.was_playing = playing;
        }
        let announced = pinned || self.player.engaged.is_some();
        let track = match &self.player.pinned {
            Some(pin) => Some(pin.path.clone()),
            None => self.player.engaged.clone(),
        };
        match (&track, &self.player.cover) {
            (Some(track), Some((known, _))) if track == known => {}
            (Some(track), _) => self.request_cover(track.clone()),
            (None, _) => self.player.cover = None,
        }
        let cover = self
            .player
            .cover
            .as_ref()
            .and_then(|(_, file)| file.clone());
        let now = self.playing().filter(|_| announced).map(|(_, state)| {
            let mode = self.prefs.player.continue_with;
            let (path, can_previous, can_next) = if let Some(pin) = &self.player.pinned {
                (
                    pin.path.clone(),
                    pin.can_step(-1, mode),
                    pin.can_step(1, mode),
                )
            } else {
                let (previous, next) = self.track_neighbours();
                (
                    self.preview.playing_path().unwrap_or_default(),
                    previous,
                    next,
                )
            };
            NowPlaying {
                title: crate::media::track_title(state, &path),
                path,
                artist: state.artist.clone(),
                album: state.album.clone(),
                status: state.status,
                position: state.position,
                at: Instant::now(),
                rate: state.rate,
                duration: state.duration,
                volume: state.volume,
                can_next,
                can_previous,
                can_seek: state.seekable,
                cover: cover.clone(),
                pinned,
            }
        });
        if now.is_some() || self.player.published {
            self.player.published = now.is_some();
            self.app.player.publish(now);
        }
    }
}

/// The pin in the last pane's footer while the preview panel is closed and the dock
/// with it: what plays, and the three controls that matter.
pub fn render_status(shell: &Shell, theme: &Theme, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let pinned = shell
        .player
        .pinned
        .as_ref()
        .filter(|_| !shell.preview.open)?;
    let state = &pinned.state;
    let title = crate::media::track_title(state, &pinned.path);
    let title = if state.artist.is_empty() {
        title
    } else {
        format!("{title} · {}", state.artist)
    };
    let font = shell.font.clone();
    Some(
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap_1()
            .max_w(px(420.))
            .text_color(theme.text)
            .child(
                request(
                    "pin-previous",
                    "skip-previous",
                    "Previous track",
                    "previous",
                )
                .enabled(pinned.can_step(-1, shell.prefs.player.continue_with))
                .render(shell, cx),
            )
            .child(
                request(
                    "pin-play",
                    if state.playing() { "pause" } else { "play" },
                    "Play / pause",
                    "play_pause",
                )
                .render(shell, cx),
            )
            .child(
                request("pin-next", "skip-next", "Next track", "next")
                    .enabled(pinned.can_step(1, shell.prefs.player.continue_with))
                    .render(shell, cx),
            )
            .child(
                crate::elements::clip::clipped_with(
                    div()
                        .id("pin-title")
                        .min_w_0()
                        .px_1()
                        .rounded_md()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis_middle()
                        .cursor_pointer()
                        .hover(|d| d.bg(theme.hover)),
                    title,
                    Some(crate::dock::REVEAL_TIP.into()),
                    font,
                )
                .on_click(cx.listener(|this, _, window, cx| this.player_reveal(window, cx))),
            )
            .child(
                div()
                    .flex_none()
                    .text_color(theme.text_muted)
                    .child(crate::media::time(state.position)),
            )
            .child(request("pin-stop", "close", "Stop and let go", "stop").render(shell, cx))
            .into_any_element(),
    )
}

/// A button of the pin's transport: it sends `what` as a `PlayerRequest`,
/// the same the desktop's media keys send, from the dock and the footer's strip.
pub(crate) fn request(
    id: &'static str,
    icon: &'static str,
    tip: &'static str,
    what: &str,
) -> Button {
    Button::new(id, icon, tip, "ui.player").args(serde_json::json!({ "request": what }))
}

#[cfg(test)]
mod tests {
    use super::{Continue, shuffled, step};

    #[test]
    fn a_shuffle_plays_every_track_once_starting_with_the_one_playing() {
        for first in [0, 3, 7] {
            let order = shuffled(8, first);
            assert_eq!(order[0], first);
            let mut sorted = order.clone();
            sorted.sort_unstable();
            assert_eq!(sorted, (0..8).collect::<Vec<_>>());
        }
        assert_eq!(shuffled(1, 0), vec![0]);
        assert!(shuffled(0, 0).is_empty());
    }

    #[test]
    fn previous_goes_back_the_way_next_came() {
        let order = [2, 0, 3, 1];
        let shuffle = Continue::Shuffle;
        assert_eq!(step(4, &order, 2, 1, shuffle), Some(0));
        assert_eq!(step(4, &order, 0, 1, shuffle), Some(3));
        assert_eq!(step(4, &order, 3, -1, shuffle), Some(0));
        assert_eq!(step(4, &order, 2, -1, shuffle), None);
        assert_eq!(step(4, &order, 1, 1, shuffle), None);
    }

    #[test]
    fn without_a_shuffle_steps_follow_the_queue() {
        for mode in [Continue::Stop, Continue::Next] {
            assert_eq!(step(4, &[], 1, 1, mode), Some(2));
            assert_eq!(step(4, &[], 3, 1, mode), None);
            assert_eq!(step(4, &[], 0, -1, mode), None);
        }
        // An order drawn over another queue is not walked.
        assert_eq!(step(4, &[1, 0], 1, 1, Continue::Shuffle), Some(2));
    }

    #[test]
    fn modes_round_trip_through_the_setting() {
        for mode in Continue::ALL {
            assert_eq!(Continue::parse(mode.key()), mode);
        }
        assert_eq!(Continue::parse("unknown"), Continue::Stop);
    }
}
