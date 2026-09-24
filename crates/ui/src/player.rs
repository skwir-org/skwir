//! Keep playing: an audio preview pinned so that it outlives the selection.
//!
//! A preview dies with the selection, and its media session with it. Pinned,
//! the session belongs to the window instead: browsing goes on, the music
//! (or the film) too, and the dock at the foot of the preview panel
//! (`dock.rs`) holds it: cover or picture, visualizer, transport. With the
//! panel closed the status bar keeps a strip of the controls instead. The
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
    pub icon: Option<(PathBuf, Option<Arc<std::path::Path>>)>,
}

impl Drop for Pinned {
    fn drop(&mut self) {
        self.lifetime.cancel();
    }
}

impl Pinned {
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
        std::mem::replace(&mut self.player.pinned, pin)
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
            // The panel keeps its session; it is a preview again.
            if let Some(mut pinned) = self.set_pin(None) {
                self.preview
                    .media
                    .adopt_lifetime(std::mem::take(&mut pinned.lifetime));
            }
            return cx.notify();
        }
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
        let pin = Pinned {
            session,
            queued: None,
            lifetime: self.preview.media.release_lifetime(),
            state: self.preview.media.state.clone(),
            path,
            queue,
            index,
            video,
            cover: picture
                .as_ref()
                .filter(|_| !video)
                .map(|(image, ..)| Arc::clone(image)),
            frame: picture.filter(|_| video),
            scrub: None,
            icon: None,
        };
        self.set_pin(Some(pin));
        // A film needs room to be seen; music is fine in a row.
        if video && self.player.height < crate::dock::TALL_FROM {
            self.player.height = crate::dock::VIDEO_HEIGHT;
        }
        self.player.focus.get_or_insert_with(|| cx.focus_handle());
        self.remember_session();
        cx.notify();
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
                let entry = pane.listing.get(*id)?;
                let kind = if video { "video/" } else { "audio/" };
                let same_kind = !entry.is_dir()
                    && bibli_platform::mime::by_name(&entry.name)
                        .is_some_and(|mime| mime.as_str().starts_with(kind));
                same_kind
                    .then(|| pane.open.as_ref()?.path_of(*id))
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
    const QUEUE_TRACKS: usize = 5000;

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
                let session = EntryRef::from_path(&path)
                    .ok_or_else(|| anyhow::anyhow!("{} is missing", path.display()))
                    .and_then(|entry| {
                        let request = PreviewRequest {
                            entry,
                            mime: bibli_platform::mime::by_name(path.as_os_str())
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

    /// Step the pin's queue. Without a pin the preview steps the selection.
    pub fn player_step(&mut self, delta: i64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pinned) = &self.player.pinned else {
            return self.media_step(delta, window, cx);
        };
        let target = pinned
            .index
            .checked_add_signed(delta.signum() as isize)
            .filter(|index| *index < pinned.queue.len());
        if let Some(index) = target {
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
                    let request = PreviewRequest {
                        entry: EntryRef::from_path(&track)?,
                        mime: bibli_platform::mime::by_name(track.as_os_str())
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
                pinned.cover = crate::thumbs::to_render_image(&image);
                cx.notify();
            }
        }
        while let Ok((current, paths)) = self.player.queue_rx.try_recv() {
            if let Some(pinned) = &mut self.player.pinned
                && pinned.path == current
                && let Some(index) = paths.iter().position(|path| *path == current)
            {
                (pinned.queue, pinned.index) = (paths, index);
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
                    pinned.cover = None;
                    pinned.frame = None;
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
        let mut finished = false;
        let mut failed = None;
        if let Some(pinned) = &mut self.player.pinned {
            // Gapless: the session is told the next track ahead of time and
            // switches to it at the sample boundary; the pin only follows.
            let next = (pinned.index + 1 < pinned.queue.len()).then_some(pinned.index + 1);
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
                    pinned.frame = None;
                    cx.notify();
                }
                // A film's frames go to the dock while the panel shows
                // something else; the slot is bounded by the worker.
                if let Some(frame) = update
                    .frame
                    .filter(|_| pinned.video)
                    .and_then(crate::media::frame_image)
                {
                    pinned.frame = Some(frame);
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
        if let Some(error) = failed {
            self.set_pin(None);
            self.set_status(error, true, cx);
        } else if finished {
            self.player_step(1, window, cx);
        }

        for request in self.app.player.take_requests() {
            self.player_request(request, window, cx);
        }
        self.publish_now_playing();
    }

    /// `ui.player_mute`: the pinned player's sound, on or off.
    pub fn player_mute(&mut self, cx: &mut Context<Self>) {
        if let Some(pinned) = &self.player.pinned {
            let _ = pinned.session.control(Control::Mute(!pinned.state.muted));
            cx.notify();
        }
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
            PlayerRequest::SeekBy(_) | PlayerRequest::SeekTo(_) => None,
            PlayerRequest::Next => return self.player_step(1, window, cx),
            PlayerRequest::Previous => return self.player_step(-1, window, cx),
            PlayerRequest::Stop => {
                let _ = session.control(Control::Play(false));
                self.player.engaged = None;
                if self.preview_is_pinned() {
                    // The panel shows this very session: it goes back to
                    // being a preview rather than ending under the user.
                    return self.media_pin(cx);
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
            if let Err(error) = session.control(control) {
                self.set_status(error.to_string(), true, cx);
            }
            cx.notify();
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
            let (path, can_previous, can_next) = if let Some(pin) = &self.player.pinned {
                (
                    pin.path.clone(),
                    pin.index > 0,
                    pin.index + 1 < pin.queue.len(),
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

/// The pin in the status bar while the preview panel is closed and the dock
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
                .enabled(pinned.index > 0)
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
                    .enabled(pinned.index + 1 < pinned.queue.len())
                    .render(shell, cx),
            )
            .child(
                div()
                    .id("pin-title")
                    .min_w_0()
                    .px_1()
                    .rounded_md()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis_middle()
                    .cursor_pointer()
                    .hover(|d| d.bg(theme.hover))
                    .tooltip(move |_, cx| {
                        crate::toolbar::tooltip("Show this track".into(), font.clone(), cx)
                    })
                    .on_click(cx.listener(|this, _, window, cx| this.player_reveal(window, cx)))
                    .child(title),
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
/// the same the desktop's media keys send, from the dock and the status bar.
pub(crate) fn request(
    id: &'static str,
    icon: &'static str,
    tip: &'static str,
    what: &str,
) -> Button {
    Button::new(id, icon, tip, "ui.player").args(serde_json::json!({ "request": what }))
}
