//! Embedded playback controls. Only the session manager touches media I/O.
use crate::sizes::TextSizes;
use crate::sizes::sized;
use crate::{preview::PreviewContent, shell::Shell, theme::Theme, toolbar::Button};
use gpui::{
    AnimationExt, AnyElement, Context, FocusHandle, RenderImage, Window, div, prelude::*, px,
};
use skwir_core::media::{Control, Frame, MediaSession, Playback};
use skwir_core::words;
use skwir_core::words::Words;
use std::{cell::Cell, rc::Rc, sync::Arc, time::Duration};

/// What stands for a film or a track before its player's frames do.
#[derive(Default)]
enum Poster {
    /// The poster is on its way. A film's first frame waits aside for it
    /// (`initial_frame`): one picture, then another, flickers.
    #[default]
    Waiting,
    /// A still stands for it (the poster, a track's cover): the player's
    /// frames wait for Play or a seek.
    Still,
    /// No still yet: the player's frames are the picture, the one waiting
    /// aside first. A track's cover is still welcome.
    Unavailable,
    /// Played, sought or shown live: frames only, a late poster refused.
    Dismissed,
}

impl MediaState {
    /// Whether a seek bar's knob is being dragged.
    pub(crate) fn scrubbing(&self) -> bool {
        self.scrub.is_some()
    }

    /// A poster stands for the film until it plays or seeks, whether it
    /// came before its player or after.
    pub(crate) fn accept_poster(&mut self) -> bool {
        if matches!(self.poster, Poster::Dismissed) {
            return false;
        }
        self.poster = Poster::Still;
        self.initial_frame = None;
        true
    }

    /// No poster comes for the film (no `ffmpeg`, or it failed): its
    /// player's frames are its picture.
    pub(crate) fn without_poster(&mut self) {
        if matches!(self.poster, Poster::Waiting) {
            self.poster = Poster::Unavailable;
        }
    }

    /// Whether the film's poster may still come.
    pub(crate) fn poster_awaited(&self) -> bool {
        matches!(self.poster, Poster::Waiting)
    }

    /// Whether the poster stands for the film or the track (`Poster::Still`).
    pub(crate) fn poster_shown(&self) -> bool {
        matches!(self.poster, Poster::Still)
    }

    /// Whether a player may still open for the file shown: its picture can
    /// come first (`Media::NoPlayer`).
    pub(crate) fn awaiting_player(&self) -> bool {
        !self.settled && self.session.is_none()
    }

    /// The player's outcome is known: one opened, or none will.
    pub(crate) fn settle(&mut self) {
        self.settled = true;
    }
}

#[derive(Default)]
pub struct MediaState {
    pub session: Option<Arc<dyn MediaSession>>,
    pub state: Playback,
    pub focus: Option<FocusHandle>,
    poster: Poster,
    /// Whether the file's player opened or none will (`awaiting_player`).
    settled: bool,
    initial_frame: Option<Frame>,
    scrub: Option<f64>,
    /// The file the session went on to by itself (gapless), for whoever
    /// follows the session's track: taken by the pin.
    pub advanced: Option<std::path::PathBuf>,
    /// Ends this preview's media worker. A reader's lifetime ends with the
    /// preview; a session that is pinned must not, so it has its own, which
    /// the pin takes with it (`release_lifetime`).
    lifetime: skwir_core::CancelToken,
    /// The colour of the cover shown, by image: worked out once, when the
    /// cover changes, not per frame.
    tint: std::cell::Cell<Option<(gpui::ImageId, Option<gpui::Hsla>)>>,
    /// The answer to a click on the picture: which way playback went, shown
    /// in the middle for an instant. The number restarts the entrance for
    /// every click.
    flash: Option<(u64, bool)>,
    flashes: u64,
    /// The frames shown and replaced, until their buffers go back to the
    /// session.
    recycled: Recycled,
    /// The poster read again with more pixels (`Shell::sharpen_poster`).
    pub(crate) sharper: crate::latest::Latest<anyhow::Result<PreviewContent>>,
}

/// A film's frames the window drew and let go of, kept until nothing
/// draws them any more, then given back to their session for its next
/// frames (`MediaSession::recycle`, `RenderImage::into_frames`): a fresh
/// buffer for each, 8 MB at 1080p thirty times a second, was mapped and
/// zeroed by the kernel anew. A few are kept: one still held elsewhere
/// after them is let go of.
#[derive(Default)]
pub(crate) struct Recycled(Vec<Arc<RenderImage>>);

impl Recycled {
    const KEPT: usize = 4;

    /// Keep `frame`, its texture dropped by the caller, until it can be
    /// given back.
    pub(crate) fn keep(&mut self, frame: Arc<RenderImage>) {
        if self.0.len() >= Self::KEPT {
            self.0.remove(0);
        }
        self.0.push(frame);
    }

    /// Give `session` the buffers of the frames nothing holds any more.
    pub(crate) fn give_back(&mut self, session: &dyn MediaSession) {
        let mut held = Vec::new();
        for frame in self.0.drain(..) {
            match Arc::try_unwrap(frame) {
                Ok(image) => {
                    for frame in image.into_frames() {
                        session.recycle(frame.into_buffer().into_raw());
                    }
                }
                Err(frame) => held.push(frame),
            }
        }
        self.0 = held;
    }
}

impl Drop for MediaState {
    fn drop(&mut self) {
        self.lifetime.cancel();
    }
}

impl MediaState {
    /// Show the session's frames as they come, with no poster first: for a
    /// session that is already playing.
    pub(crate) fn show_live(&mut self) {
        self.settled = true;
        self.poster = Poster::Dismissed;
        self.initial_frame = None;
    }

    /// The lifetime the next session of this preview is opened with.
    pub(crate) fn lifetime(&self) -> skwir_core::CancelToken {
        self.lifetime.clone()
    }

    /// Hand the session's lifetime to the pin: this preview going away no
    /// longer ends it.
    pub(crate) fn release_lifetime(&mut self) -> skwir_core::CancelToken {
        std::mem::take(&mut self.lifetime)
    }

    /// Take a lifetime back from the pin: the session is this preview's again.
    pub(crate) fn adopt_lifetime(&mut self, lifetime: skwir_core::CancelToken) {
        self.lifetime = lifetime;
    }
}

impl Shell {
    pub(crate) fn media_is_audio(&self) -> bool {
        self.preview.media.state.audio_only
            || self
                .preview
                .info
                .mime
                .as_deref()
                .is_some_and(|m| m.starts_with("audio/"))
    }

    /// Whether a player shows the file, or may still: its picture came
    /// first (`MediaState::awaiting_player`), or a track's thumbnail stands
    /// in for its cover (`preview::placeholder`). What is drawn of it is the
    /// player's from the picture on, so nothing changes when it opens.
    pub(crate) fn shows_player(&self) -> bool {
        self.preview.media.session.is_some()
            || (self.preview.media.awaiting_player()
                && matches!(
                    self.preview
                        .content
                        .as_ref()
                        .or_else(|| crate::preview::placeholder(self)),
                    Some(PreviewContent::Image {
                        detail: Some(skwir_core::preview::PreviewInfo::Video { .. }),
                        ..
                    })
                ))
    }

    /// The kind of media the panel shows, as the start of its type:
    /// `audio/` or `video/`. Stepping and a shuffle keep to that kind.
    pub(crate) fn media_kind(&self) -> Option<&'static str> {
        let mime = self.preview.info.mime.as_deref()?;
        ["audio/", "video/"]
            .into_iter()
            .find(|kind| mime.starts_with(kind))
    }

    /// How far a search for the neighbouring track looks: a folder of a
    /// hundred thousand documents with two songs in it is not scanned whole.
    const NEIGHBOUR_REACH: usize = 4000;

    /// The files of the same kind (`same`: a track of the kind playing, a
    /// picture) around entry `id` in the pane's order. They are kept as
    /// entries, not positions, so a sort in between cannot turn a step into
    /// a jump to whatever took their place.
    pub(crate) fn neighbours(
        &self,
        id: skwir_core::EntryId,
        same: impl Fn(&skwir_core::Entry) -> bool,
    ) -> (Option<skwir_core::EntryId>, Option<skwir_core::EntryId>) {
        let sorted = self.ui().list.sorted.clone();
        let Some(here) = sorted.iter().position(|entry| *entry == id) else {
            return (None, None);
        };
        let ws = self.app.workspace.read();
        let Some(pane) = ws.pane(self.pane) else {
            return (None, None);
        };
        let track = |index: &usize| pane.listing.get(sorted[*index]).is_some_and(&same);
        (
            (here.saturating_sub(Self::NEIGHBOUR_REACH)..here)
                .rev()
                .find(track)
                .map(|index| sorted[index]),
            (here + 1..sorted.len().min(here + 1 + Self::NEIGHBOUR_REACH))
                .find(track)
                .map(|index| sorted[index]),
        )
    }

    /// Another file of the kind shown, drawn at random from the pane: what
    /// a shuffle goes on with when nothing is pinned. Worked out on a step,
    /// never while painting.
    fn random_track(&self) -> Option<skwir_core::EntryId> {
        let (kind, here) = (self.media_kind()?, self.preview.entry_id()?);
        // Typed as a selection is, off the workspace's lock.
        let tracks = self.typed_in_order(
            self.pane,
            |_, id| id != here,
            |mime, _| mime.as_str().starts_with(kind).then_some(()),
        );
        (!tracks.is_empty()).then(|| tracks[crate::player::random_below(tracks.len())].0)
    }

    /// The entry of the pane the panel previews whose file is `path`.
    pub(crate) fn listed(&self, path: &std::path::Path) -> Option<skwir_core::EntryId> {
        let pane = self.preview.pane()?;
        let ui = self.panes.iter().find(|ui| ui.id == pane)?;
        let ws = self.app.workspace.read();
        let pane = ws.pane(pane)?;
        ui.list
            .sorted
            .iter()
            .copied()
            .find(|id| pane.path_of(*id).is_some_and(|there| there == path))
    }

    /// Whether there is a track before this one, and one after: in the
    /// playlist the panel went into, in the pin's order while the panel
    /// shows the pinned track, otherwise in the folder. A shuffle can go on
    /// as long as there is another track.
    pub(crate) fn track_neighbours(&self) -> (bool, bool) {
        let mode = self.prefs.player.continue_with;
        let shuffle = mode == crate::player::Continue::Shuffle;
        if let Some(queue) = &self.preview.queue {
            let others = queue.targets.len() > 1;
            return (
                queue.index > 0,
                (shuffle && others) || queue.index + 1 < queue.targets.len(),
            );
        }
        if let Some(pinned) = self
            .player
            .pinned
            .as_ref()
            .filter(|_| self.preview_is_pinned())
        {
            return (pinned.can_step(-1, mode), pinned.can_step(1, mode));
        }
        let (previous, next) = self.preview.neighbours;
        let next = next.is_some() || (shuffle && previous.is_some());
        (previous.is_some(), next)
    }

    /// The entry a step of `delta` goes to: the pin's next or previous
    /// while the panel shows the pinned track, so the panel and the dock
    /// walk one order; else one at random for Next in a shuffle, else the
    /// neighbour.
    fn step_target(&self, delta: i64) -> Option<skwir_core::EntryId> {
        let mode = self.prefs.player.continue_with;
        if let Some(pinned) = self
            .player
            .pinned
            .as_ref()
            .filter(|_| self.preview_is_pinned())
            && let Some(index) = pinned.step_target(delta, mode)
            && let Some(id) = self.listed(&pinned.queue[index])
        {
            return Some(id);
        }
        if mode == crate::player::Continue::Shuffle
            && delta > 0
            && let Some(id) = self.random_track()
        {
            return Some(id);
        }
        // The neighbours of a picture are pictures (`fullscreen.rs`).
        self.media_kind()?;
        let (previous, next) = self.preview.neighbours;
        if delta < 0 { previous } else { next }
    }

    /// `ui.media_step { delta }`: the track after or before this one. It is
    /// the pane's selection that moves; the preview follows it as always,
    /// and keeps playing if it was.
    pub fn media_step(&mut self, delta: i64, window: &mut Window, cx: &mut Context<Self>) {
        let playing = self.preview.media.state.playing() || self.preview.media.state.ended();
        let shuffle = self.prefs.player.continue_with == crate::player::Continue::Shuffle;
        if let Some(queue) = &mut self.preview.queue {
            let count = queue.targets.len();
            let index = if shuffle && delta > 0 && count > 1 {
                // Any track but this one.
                Some((queue.index + 1 + crate::player::random_below(count - 1)) % count)
            } else {
                queue
                    .index
                    .checked_add_signed(delta.signum() as isize)
                    .filter(|index| *index < count)
            };
            let Some(index) = index else {
                return;
            };
            queue.index = index;
            let target = queue.targets[index].clone();
            let args = serde_json::json!({
                "action": "member",
                "depth": queue.depth,
                "target": target,
            });
            self.run("ui.inspect", args, window, cx);
            // Set after the step, which forgets any earlier wish.
            if playing && let Some(pane) = self.preview.pane() {
                let id = self.preview.entry_id();
                self.preview.autoplay = id.map(|id| crate::preview::Autoplay {
                    pane,
                    id,
                    member: Some(target),
                });
            }
            return;
        }
        let Some(target) = self.step_target(delta) else {
            return;
        };
        let Some(pane) = self.preview.pane() else {
            return;
        };
        self.preview.autoplay = playing.then_some(crate::preview::Autoplay {
            pane,
            id: target,
            member: None,
        });
        self.with_pane(pane, |this| {
            if let Some(index) = this.ui().list.index_of(target) {
                this.cursor_to(index as isize, crate::list::Step::Select, window, cx);
            }
        });
    }

    /// A film's picture is shown, or none will be: the panel stops
    /// waiting. While a player is open the panel waits for nothing else.
    pub(crate) fn film_waited(&mut self, cx: &mut Context<Self>) {
        if self.preview.phase == crate::preview::PreviewPhase::Loading {
            self.preview.phase = crate::preview::PreviewPhase::Idle;
            self.sync_busy(cx);
        }
    }

    /// No frame of the film comes: no poster, and its player ended before
    /// its first. The desktop's thumbnail, kept aside, is its picture.
    pub(crate) fn film_without_frame(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.preview.content.is_none()
            && let Some(thumbnail) = self.preview.placeholder.take()
        {
            self.replace_preview_content(Some(thumbnail), window, cx);
        }
        self.film_waited(cx);
    }

    /// A sharper poster, once read: it takes the place of the one shown,
    /// while that one still stands for the film.
    pub fn sync_poster(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.preview.media.sharper.take() {
            Some(Ok(sharper)) if self.preview.media.poster_shown() => {
                self.replace_preview_content(Some(sharper), window, cx);
            }
            Some(Err(error)) => tracing::debug!("poster not sharpened: {error:#}"),
            _ => {}
        }
    }

    pub fn sync_media(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let MediaState {
            session, recycled, ..
        } = &mut self.preview.media;
        let Some(session) = session else {
            return;
        };
        recycled.give_back(&**session);
        let update = session.take_update();
        if let Some(path) = update.advanced {
            self.preview.media.advanced = Some(path);
        }
        // A track that plays to its end hands over to the next one, once
        // what this update says (a notice, an error) has been heard.
        let mut hand_over = false;
        if let Some(state) = update.state {
            let finished = state.ended() && !self.preview.media.state.ended();
            self.preview.media.state = state;
            hand_over = finished
                && self.prefs.player.continue_with != crate::player::Continue::Stop
                && self.track_neighbours().1;
        }
        let mut frame = update.frame.filter(|_| !hand_over);
        let audio = self.media_is_audio();
        match self.preview.media.poster {
            // A film's poster on its way: the player's first frame waits
            // aside, its picture only if no poster comes.
            Poster::Waiting if !audio => {
                if frame.is_some() {
                    self.preview.media.initial_frame = frame.take();
                }
            }
            // A track's cover: the desktop's thumbnail, until the poster's.
            Poster::Waiting => {
                use skwir_core::thumbs::ThumbState;
                match self.cover_thumbnail() {
                    ThumbState::Ready(image) => {
                        if let Some(render) = crate::thumbs::to_render_image(&image) {
                            self.replace_preview_content(
                                Some(PreviewContent::Image {
                                    image: render,
                                    width: image.width,
                                    height: image.height,
                                    detail: None,
                                    document: None,
                                    facts: Vec::new(),
                                    pixels: crate::preview::Pixels::Own,
                                    ink: None,
                                }),
                                window,
                                cx,
                            );
                        }
                        self.preview.media.poster = Poster::Still;
                        self.preview.media.initial_frame = None;
                    }
                    ThumbState::Unavailable => {
                        self.preview.media.poster = Poster::Unavailable;
                    }
                    ThumbState::Pending => {
                        if frame.is_some() {
                            self.preview.media.initial_frame = frame.take();
                        }
                    }
                }
                if !matches!(self.preview.media.poster, Poster::Unavailable) {
                    frame = None;
                }
            }
            Poster::Still => frame = None,
            Poster::Unavailable | Poster::Dismissed => {}
        }
        if matches!(
            self.preview.media.poster,
            Poster::Unavailable | Poster::Dismissed
        ) {
            frame = frame.or_else(|| self.preview.media.initial_frame.take());
        }
        let picture = frame.and_then(frame_image);
        // The film's picture came, or a recording turned out to have none:
        // the panel stops waiting (a poster ends the wait as it arrives).
        if picture.is_some() || audio {
            self.film_waited(cx);
        }
        if let Some((image, width, height)) = picture {
            // The frame it replaces goes back to the session once nothing
            // draws it.
            if let Some(PreviewContent::Image {
                image: shown,
                detail: Some(skwir_core::preview::PreviewInfo::Video { .. }),
                ..
            }) = &self.preview.content
            {
                self.preview.media.recycled.keep(Arc::clone(shown));
            }
            let frame = film_frame(image, width, height, &self.preview.media.state);
            self.replace_preview_content(Some(frame), window, cx);
        }
        if let Some(notice) = update.notice {
            // The session continues; its next state update corrects the UI.
            self.set_status(notice, false, cx);
        }
        if let Some(plan) = update.taken {
            // A plan took the file (ADR-136): the session let go of it and
            // ended, quietly, as a pause does. The pin, or the panel's own
            // player, waits where it stood for the plan's end, which says
            // where the file went.
            self.exit_fullscreen(window, cx);
            if self.preview_is_pinned() {
                self.pin_taken(plan, window, cx);
            } else {
                self.panel_taken(plan, window, cx);
            }
            self.end_panel_session(window, cx);
        }
        if let Some(error) = update.error {
            // The manager has ended the session. Withdraw the controls and
            // keep the last picture; every later control would only fail.
            // The pin goes with it when it was this session: the desktop's
            // controls and the dock would go on with a dead one.
            self.exit_fullscreen(window, cx);
            if self.preview_is_pinned() {
                self.unpin_dead();
            }
            self.end_panel_session(window, cx);
            if !self.preview.media.poster_awaited() {
                self.film_without_frame(window, cx);
            }
            self.set_status_about(error, "media.backend", cx);
        }
        if hand_over {
            return self.media_step(1, window, cx);
        }
        self.arm_fullscreen_hide(window, cx);
    }

    /// The panel's session ended under it: its controls withdrawn, its
    /// last picture kept, every later control would only fail.
    fn end_panel_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.preview.media.session = None;
        self.preview.media.state.status = skwir_core::media::PlaybackStatus::Paused;
        // The player's key context follows its focus handle; return keys
        // to the peek overlay or the pane instead of a dead player.
        if let Some(focus) = self.preview.media.focus.take()
            && focus.contains_focused(window, cx)
        {
            match self
                .preview
                .peek_focus
                .as_ref()
                .filter(|_| self.preview.peek)
            {
                Some(peek) => window.focus(peek, cx),
                None => window.focus(&self.ui().focus, cx),
            }
        }
    }

    /// The one way a control reaches the panel's session: whatever asked
    /// (a button, a key, `ui.media_control` from a script), a value out of
    /// range is refused here.
    /// The window shown or not (`Window::is_visible`, told by the shell as
    /// it changes): a film's frames are sent only while it is, by the
    /// panel's player and the pinned one, which decode on meanwhile. A
    /// player opened while it is not starts without (`show_media`).
    pub(crate) fn media_shown(&mut self, shown: bool) {
        if self.preview.window_shown == shown {
            return;
        }
        self.preview.window_shown = shown;
        let pinned = self.player.pinned.as_ref().map(|pinned| &pinned.session);
        for session in self.preview.media.session.iter().chain(pinned) {
            let _ = session.control(Control::Frames(shown));
        }
    }

    pub fn media_send(&mut self, command: Control, cx: &mut Context<Self>) {
        if !command.valid() {
            return;
        }
        if let Some(session) = &self.preview.media.session {
            match session.control(command) {
                Ok(()) => {
                    if let Control::Seek(position) = command {
                        self.preview.media.state.position =
                            position.min(self.preview.media.state.duration.unwrap_or(position));
                    }
                    if matches!(command, Control::Play(true)) {
                        self.media_started(cx);
                    }
                    if matches!(command, Control::Play(true) | Control::Seek(_))
                        && !self.media_is_audio()
                    {
                        self.preview.media.poster = Poster::Dismissed;
                        self.preview.media.initial_frame = None;
                        self.cancel_pending_preview();
                    }
                }
                Err(error) => self.set_status(Words::of_error(error), true, cx),
            }
        }
        cx.notify();
    }
    /// A click on the picture, as on any video site: one toggles playback and
    /// says so with a flash in the middle; a double click on a film also
    /// goes fullscreen (or comes back), after the second click undid the
    /// first, so the film plays as it did before. `on` says which player
    /// answers: the panel's, or the pinned one in the dock.
    pub fn media_clicked(
        &mut self,
        on: Flash,
        event: &gpui::ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let double = event.click_count() == 2;
        let playing = match on {
            Flash::Preview => {
                if self.preview.media.session.is_none() {
                    return;
                }
                let playing = !self.preview.media.state.playing();
                self.media_control(&serde_json::json!({"action": "play"}), cx);
                if double && !self.media_is_audio() {
                    self.preview_fullscreen(window, cx);
                }
                playing
            }
            Flash::Dock => {
                let Some(pinned) = &self.player.pinned else {
                    return;
                };
                let (playing, video) = (!pinned.state.playing(), pinned.video);
                self.player_request(skwir_core::playing::PlayerRequest::PlayPause, window, cx);
                if double && video {
                    self.player_fullscreen(window, cx);
                }
                playing
            }
        };
        if double {
            *self.flash_slot(on) = None;
            return cx.notify();
        }
        self.preview.media.flashes += 1;
        let flash = self.preview.media.flashes;
        *self.flash_slot(on) = Some((flash, playing));
        // Gone once its entrance has ended; one repaint, not a loop.
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(FLASH_MS + 60))
                .await;
            this.update(cx, |shell, cx| {
                let slot = shell.flash_slot(on);
                if slot.is_some_and(|(seq, _)| seq == flash) {
                    *slot = None;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn flash_slot(&mut self, on: Flash) -> &mut Option<(u64, bool)> {
        match on {
            Flash::Preview => &mut self.preview.media.flash,
            Flash::Dock => &mut self.player.flash,
        }
    }

    pub fn media_control(&mut self, args: &serde_json::Value, cx: &mut Context<Self>) {
        let state = &self.preview.media.state;
        let value = args.get("value").and_then(serde_json::Value::as_f64);
        let command = match args.get("action").and_then(serde_json::Value::as_str) {
            Some("play") => Some(Control::Play(!state.playing())),
            Some("mute") => Some(Control::Mute(!state.muted)),
            // A level, or ↑ and ↓ over the player: a step either way, as
            // every volume moves (`set_volume`).
            Some("volume") => {
                if let Some(volume) = value {
                    self.set_volume(VolumeOf::Panel, volume, cx);
                }
                None
            }
            Some("volume_by") => {
                if let Some(delta) = value {
                    self.step_volume(VolumeOf::Panel, delta, cx);
                }
                None
            }
            // Rate changes are seeks; a non-seekable stream keeps 1x.
            Some("rate") => value.filter(|_| state.seekable).map(Control::Rate),
            Some("audio") => args
                .get("value")
                .and_then(serde_json::Value::as_i64)
                .and_then(|n| i32::try_from(n).ok())
                .map(Control::Audio),
            Some("subtitle") => args
                .get("value")
                .and_then(serde_json::Value::as_i64)
                .and_then(|n| i32::try_from(n).ok())
                .map(Control::Subtitle),
            _ => None,
        };
        if let Some(command) = command {
            self.media_send(command, cx);
        }
    }
    pub fn media_scrub(&mut self, args: &serde_json::Value, cx: &mut Context<Self>) {
        let Some(duration) = self
            .preview
            .media
            .state
            .duration
            .filter(|_| self.preview.media.state.seekable)
        else {
            return;
        };
        let fraction = args
            .get("fraction")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(0.)
            .clamp(0., 1.);
        let phase = args.get("phase").and_then(serde_json::Value::as_str);
        if let Some(seconds) = scrub(&mut self.preview.media.scrub, phase, fraction * duration) {
            self.media_send(Control::Seek(seconds), cx);
        }
        cx.notify();
    }
}

/// How long the flash lasts. Longer than the entrance the rules allow
/// elsewhere, on purpose: it is the one animation that answers a click, and
/// players hold the glyph in view before it goes, so the eye can read it.
const FLASH_MS: u64 = 800;
/// The share of `FLASH_MS` the glyph stays whole before it fades out.
const FLASH_HOLD: f32 = 0.45;

/// The flash's opacity and size at `delta` of its life: whole and slowly
/// growing while held, then fading and growing on.
pub fn flash_at(delta: f32) -> (f32, f32) {
    let delta = delta.clamp(0., 1.);
    let fade = ((delta - FLASH_HOLD) / (1. - FLASH_HOLD)).max(0.);
    let eased = 1. - (1. - fade).powi(3);
    (1. - eased, 56. + 12. * delta + 24. * eased)
}

/// Which player a click on a picture steers: the panel's preview, or the
/// pinned one in the dock. Each has a flash of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flash {
    Preview,
    Dock,
}

/// The play/pause flash while it lasts, to lay over a picture: it sits in
/// the middle of whatever `relative` box it is given, so it goes on the
/// cover's disc for music (`halo_square`) and over the whole film, at
/// `scale` of its size for a small picture such as the dock's cover.
pub fn flash(on: Flash, shell: &Shell, scale: f32) -> Option<AnyElement> {
    let flash = match on {
        Flash::Preview => shell.preview.media.flash,
        Flash::Dock => shell.player.flash,
    };
    flash.map(|(seq, playing)| {
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(gpui::black().opacity(0.55))
                    .child(
                        gpui::svg()
                            .path(if playing {
                                "toolbar/play.svg"
                            } else {
                                "toolbar/pause.svg"
                            })
                            .size(px(28. * scale))
                            .text_color(gpui::white()),
                    )
                    .with_animation(
                        ("media-flash", seq),
                        gpui::Animation::new(Duration::from_millis(FLASH_MS)),
                        move |disc, delta| {
                            let (opacity, side) = flash_at(delta);
                            let side = px(side * scale);
                            disc.w(side).h(side).opacity(opacity)
                        },
                    ),
            )
            .into_any_element()
    })
}

/// The click behaviour of a player on `inner`: one click toggles playback,
/// two go fullscreen (`Shell::media_clicked`). The click ends here: what
/// is around the picture may answer a click of its own (the dock reveals
/// the track), and a press on the picture is not one of those.
pub fn clickable(
    id: &'static str,
    inner: gpui::Div,
    on: Flash,
    cx: &mut Context<Shell>,
) -> gpui::Stateful<gpui::Div> {
    inner.id(id).cursor_pointer().on_click(cx.listener(
        move |this, event: &gpui::ClickEvent, window, cx| {
            cx.stop_propagation();
            this.media_clicked(on, event, window, cx);
        },
    ))
}

/// A film with the click behaviour of a player on it: `inner` is laid out
/// as it was, and the flash sits over its middle while it lasts.
pub fn surface(
    id: &'static str,
    inner: AnyElement,
    on: Flash,
    shell: &Shell,
    cx: &mut Context<Shell>,
) -> AnyElement {
    clickable(
        id,
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(inner)
            .children(flash(on, shell, 1.)),
        on,
        cx,
    )
    .into_any_element()
}

/// A file of the kind a player steps through (`audio/`, `video/`).
pub(crate) fn is_track(entry: &skwir_core::Entry, kind: &str) -> bool {
    skwir_platform::mime::of_entry(entry).is_some_and(|mime| mime.as_str().starts_with(kind))
}

/// What Play (`ui.media_play`) does with a file: music goes to the pinned
/// player, a film to the quick preview, a playlist gives its tracks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Played {
    Track,
    Film,
    Playlist,
}

/// The types of entries read under the workspace's lock (each its
/// `Entry::inode_type` and `Entry::type_name`, a link's target's), in their
/// order, typed once the lock is let go as a selection is
/// (`mime::by_names`: past a few, a suffix is asked once, so a hundred
/// thousand names cost a few hundred questions, not a hundred thousand).
pub(crate) fn types_of<'a>(
    parts: impl ExactSizeIterator<Item = (Option<&'static str>, &'a std::ffi::OsStr)> + Clone,
) -> Vec<Option<skwir_platform::mime::Mime>> {
    let named = skwir_platform::mime::by_names(parts.clone().map(|(_, name)| name));
    parts
        .zip(named)
        .map(|((inode, _), named)| match inode {
            Some(inode) => Some(skwir_platform::mime::Mime(inode.to_owned())),
            None => named,
        })
        .collect()
}

/// What a file of type `mime` named `name` plays as, if anything: a
/// playlist by the one question the previews ask (`preview::playlist`),
/// whatever kind its type is filed under, else music or a film by its
/// type's kind.
pub(crate) fn played_as(
    mime: &skwir_platform::mime::Mime,
    name: &std::ffi::OsStr,
) -> Option<Played> {
    if skwir_platform::preview::playlist(std::path::Path::new(name), Some(mime.as_str())).is_some()
    {
        return Some(Played::Playlist);
    }
    let kind = mime.as_str();
    if kind.starts_with("audio/") {
        Some(Played::Track)
    } else if kind.starts_with("video/") {
        Some(Played::Film)
    } else {
        None
    }
}

/// A film's frame as the panel shows it: a film (`PreviewInfo::Video`),
/// so Fit makes it as large as the panel, a small one enlarged
/// (`preview::ShownAs::Film`), whoever drew it: the panel's player, or the
/// pinned one the panel shows again (`Shell::show_pinned`). Shown as a
/// picture, a small film paused there stayed at its own size.
pub(crate) fn film_frame(
    image: Arc<RenderImage>,
    width: u32,
    height: u32,
    state: &Playback,
) -> PreviewContent {
    PreviewContent::Image {
        image,
        width,
        height,
        detail: Some(skwir_core::preview::PreviewInfo::Video {
            seconds: state.position,
            duration: state.duration,
            size: None,
        }),
        document: None,
        facts: Vec::new(),
        pixels: crate::preview::Pixels::Own,
        ink: None,
    }
}

/// A frame of a film as a picture for GPUI: the worker sends BGRA, which is
/// what GPUI wants, so the buffer moves in without a copy.
pub(crate) fn frame_image(frame: Frame) -> Option<(Arc<RenderImage>, u32, u32)> {
    let (width, height) = (frame.width, frame.height);
    let buffer = image::RgbaImage::from_raw(width, height, frame.bgra)?;
    let image = Arc::new(RenderImage::new(smallvec::SmallVec::from_buf([
        image::Frame::new(buffer),
    ])));
    Some((image, width, height))
}

/// One phase of dragging a seek bar's knob, for the panel's player and the
/// dock's alike: the knob follows a press and its moves, and the release
/// says where to seek. A move or a release without a press is ignored.
pub(crate) fn scrub(knob: &mut Option<f64>, phase: Option<&str>, seconds: f64) -> Option<f64> {
    match phase {
        Some("down") => *knob = Some(seconds),
        Some("move") if knob.is_some() => *knob = Some(seconds),
        Some("up") if knob.take().is_some() => return Some(seconds),
        _ => {}
    }
    None
}

/// What a track is called: its title, else its file's name without the
/// extension. The panel, the dock, the footer's strip and the desktop agree.
pub(crate) fn track_title(state: &Playback, path: &std::path::Path) -> String {
    if state.title.is_empty() {
        path.file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default()
    } else {
        state.title.clone()
    }
}

/// A duration, or a dash while it is unknown.
pub(crate) fn duration(state: &Playback) -> String {
    state.duration.map_or_else(|| "—".into(), time)
}

pub(crate) fn time(seconds: f64) -> String {
    let total = seconds.max(0.) as u64;
    if total >= 3600 {
        format!("{}:{:02}:{:02}", total / 3600, total / 60 % 60, total % 60)
    } else {
        format!("{}:{:02}", total / 60, total % 60)
    }
}
/// What a screen reader calls a player's volume: its slider and its popup.
fn volume_name() -> Words {
    words!(
        media_volume = "Volume",
        note = "What a screen reader calls a player's volume slider and its popup."
    )
}

/// A player's button: the panel's, the pinned player's.
pub(crate) fn previous_track() -> Words {
    words!(
        media_previous_track = "Previous track",
        note = "A button of the player."
    )
}

/// A player's button: the panel's, the pinned player's.
pub(crate) fn next_track() -> Words {
    words!(
        media_next_track = "Next track",
        note = "A button of the player."
    )
}

/// A player's button: the panel's, the pinned player's.
pub(crate) fn play_pause() -> Words {
    words!(
        media_play_pause = "Play / pause",
        note = "A button of the player."
    )
}

fn control(
    id: impl Into<gpui::SharedString>,
    icon: &str,
    title: impl Into<gpui::SharedString>,
    action: &str,
    value: serde_json::Value,
) -> Button {
    // Without a value the arguments are the key binding's own, so the
    // button's tooltip finds its shortcut (`Shell::keys_for`).
    let args = if value.is_null() {
        serde_json::json!({ "action": action })
    } else {
        serde_json::json!({ "action": action, "value": value })
    };
    Button::new(id, icon.to_owned(), title, "ui.media_control").args(args)
}

/// The volume's popup (`ui.preview_menu` of this kind): above the panel's
/// speaker, or above More when the bar left the speaker out.
pub const VOLUME_POPUP: &str = "volume";

/// How far Up, Down and a notch of the wheel move a volume.
pub const VOLUME_STEP: f64 = 0.05;

/// Which player a volume control steers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VolumeOf {
    /// The panel's (and the whole screen's, which is the panel's).
    Panel,
    /// The pinned one, in the dock.
    Dock,
}

/// The volume's slider, the panel's popup's or the dock's (one is open at
/// a time). Whether a popup the wheel opened is passing is the popup's
/// (`toolbar::Passing`).
#[derive(Default)]
pub struct Volume {
    /// Its keyboard, given as its popup opens, in the `volume` place: Up and
    /// Down (Right and Left) step it (`ui.volume_step`), Home and End take
    /// it to its ends, Escape closes the popup; a stop of the popup's tab
    /// order, before Mute.
    pub focus: Option<gpui::FocusHandle>,
    /// Pressed and not yet let go: the moves are followed over the window.
    /// A popup opening forgets it: a press its slider saw is no new one's.
    pub held: bool,
    /// The wheel's turn under way, over a speaker or the slider: a mouse's
    /// notch is a step, a touchpad's slide goes a step at 60 px
    /// (`reader_scroll::Wheeled`).
    pub turn: crate::reader_scroll::WheelTurn,
}

/// How long a volume popup the wheel opened stays once the pointer has
/// left the speaker and the popup: room to cross from one to the other.
const VOLUME_LINGER: Duration = Duration::from_millis(700);

/// The speaker's icon for `state`: crossed out when muted, else with as
/// many waves as it is loud: none at nothing, one below half, two from
/// half.
pub(crate) fn speaker_icon(state: &Playback) -> &'static str {
    if state.muted {
        "muted"
    } else if state.volume <= 0. {
        "volume-none"
    } else if state.volume < 0.5 {
        "volume-low"
    } else {
        "volume"
    }
}

/// A player's speaker button: it opens the player's volume (`kind`, through
/// `command`), its icon as loud as the player (`speaker_icon`), its name
/// the level (More's row says "Volume").
pub(crate) fn volume_button(
    id: &'static str,
    state: &Playback,
    command: &'static str,
    kind: &'static str,
) -> Button {
    let title = if state.muted {
        words!(
            media_volume_muted = "Volume · muted",
            note = "Pointing at a player's speaker button while its sound is muted."
        )
    } else {
        words!(
            media_volume_level = "Volume · { $level }",
            level = percent(state.volume),
            note = "Pointing at a player's speaker button: how loud it plays (80 %)."
        )
    };
    Button::new(id, speaker_icon(state), title.say(), command)
        .args(serde_json::json!({ "kind": kind }))
}

fn percent(volume: f64) -> String {
    format!("{:.0}%", (volume * 100.).round())
}

/// What a player's volume popup holds, top to bottom: the level, an
/// upright slider (`controls::upright_slider`) whose press, moves and
/// release set the volume of `of` as the pointer goes, and `mute`, the
/// player's Mute, at its foot. The wheel over it steps the volume. The
/// slider is a slider of the volume in percent to assistive technology.
pub(crate) fn volume_control(
    id: &'static str,
    of: VolumeOf,
    state: &Playback,
    mute: Button,
    shell: &Shell,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    use crate::controls::{Slid, Slide};
    let volume = &shell.player.volume;
    let level = if state.muted {
        words!(
            media_muted = "Muted",
            note = "What a screen reader hears of the volume slider while the sound is muted."
        )
        .say()
    } else {
        percent(state.volume)
    };
    let slid = cx.listener(move |this, slid: &Slid, _, cx| {
        // A move or a release it never saw the press of is not its own:
        // the slider tells every release outside it (a press on Mute).
        let held = this.player.volume.held;
        match slid.slide {
            Slide::Press => this.player.volume.held = true,
            Slide::Drag | Slide::Release if !held => return,
            Slide::Release => this.player.volume.held = false,
            Slide::Drag => {}
        }
        this.set_volume(of, f64::from(slid.at), cx);
    });
    let wheel = cx.listener(move |this, event: &gpui::ScrollEvent, window, cx| {
        this.volume_wheel(of, event, window, cx);
    });
    let figures = gpui::FontFeatures(std::sync::Arc::new(vec![("tnum".into(), 1)]));
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap_1()
        .on_scroll(wheel)
        .child(
            div()
                .text_body()
                .font_features(figures)
                .when(state.muted, |d| d.text_color(theme.text_muted))
                .child(level.clone()),
        )
        .child(
            crate::controls::upright_slider(
                id,
                state.volume as f32,
                volume.held,
                theme,
                Rc::new(slid),
            )
            .h(crate::sizes::sized(VOLUME_HEIGHT))
            .when_some(volume.focus.as_ref(), InteractiveElement::track_focus)
            .key_context(crate::keymap::VOLUME)
            .aria_label(volume_name().say())
            .aria_numeric_value((state.volume * 100.).round())
            .aria_min_numeric_value(0.)
            .aria_max_numeric_value(100.)
            .aria_value(level),
        )
        // Muting leaves the popup open, its level still there to set.
        .child(mute.active(state.muted).keeps_open().render(shell, cx))
        .into_any_element()
}

/// The upright slider's height, at 100 % (`sizes::sized`).
const VOLUME_HEIGHT: f32 = 120.;

/// A player's volume popup (`popup`, closed by `close`): its control
/// (`volume_control`) and what makes one the wheel opened used, a press
/// anywhere in it (`toolbar::Passing`), and what follows the pointer onto
/// and off it.
pub(crate) fn volume_panel(
    popup: &crate::toolbar::Popup,
    close: &'static str,
    of: VolumeOf,
    state: &Playback,
    mute: Button,
    shell: &Shell,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> gpui::Stateful<gpui::Div> {
    crate::toolbar::popup_panel(popup, close, theme, cx)
        .w_auto()
        .items_center()
        .when(shell.a11y, |d| d.aria_label(volume_name().say()))
        .on_hover(cx.listener(move |this, over: &bool, window, cx| {
            this.volume_over(of, true, *over, window, cx);
        }))
        // Before what is pressed takes the keyboard: what had it is what
        // the popup gives it back to.
        .capture_any_mouse_down(cx.listener(move |this, _, window, cx| {
            this.volume_used(of, window, cx);
        }))
        .child(volume_control(
            match of {
                VolumeOf::Panel => "preview-volume",
                VolumeOf::Dock => "dock-volume-slider",
            },
            of,
            state,
            mute,
            shell,
            theme,
            cx,
        ))
}

/// The panel's volume popup.
fn volume_popup(shell: &Shell, theme: &Theme, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let popup = shell.preview.menu.as_ref()?;
    let state = &shell.preview.media.state;
    let mute = control(
        "preview-volume-mute",
        speaker_icon(state),
        if state.muted {
            words!(
                media_unmute = "Unmute",
                note = "A button of a player's volume, while its sound is muted."
            )
        } else {
            words!(media_mute = "Mute", note = "A button of a player's volume.")
        }
        .say(),
        "mute",
        serde_json::Value::Null,
    );
    Some(
        volume_panel(
            popup,
            "ui.preview_menu",
            VolumeOf::Panel,
            state,
            mute,
            shell,
            theme,
            cx,
        )
        .into_any_element(),
    )
}

impl Shell {
    /// The volume of `of`'s player set to `volume` (0 to 1, where the
    /// control stands): shown at once, its player told. A volume moved is
    /// heard: it unmutes, as the desktops' own volume controls do (GNOME's,
    /// KDE's, Windows').
    pub(crate) fn set_volume(&mut self, of: VolumeOf, volume: f64, cx: &mut Context<Self>) {
        let volume = volume.clamp(0., 1.);
        match of {
            VolumeOf::Panel => {
                // Nothing playing there: no level to keep for nothing.
                if self.preview.media.session.is_none() {
                    return;
                }
                let state = &mut self.preview.media.state;
                let muted = std::mem::replace(&mut state.muted, false);
                state.volume = volume;
                if muted {
                    self.media_send(Control::Mute(false), cx);
                }
                self.media_send(Control::Volume(volume), cx);
            }
            VolumeOf::Dock => {
                let Some(pinned) = &mut self.player.pinned else {
                    return;
                };
                let muted = std::mem::replace(&mut pinned.state.muted, false);
                pinned.state.volume = volume;
                let session = Arc::clone(&pinned.session);
                let sent = if muted {
                    session.control(Control::Mute(false))
                } else {
                    Ok(())
                }
                .and_then(|()| session.control(Control::Volume(volume)));
                if let Err(error) = sent {
                    self.set_status(Words::of_error(error), true, cx);
                }
            }
        }
        cx.notify();
    }

    /// `of`'s volume moved by `delta`: every step of a volume, its
    /// slider's keys, the wheel, the player's Up and Down and the desktop's
    /// (`Shell::player_volume_by`), goes from what is shown, which a step
    /// changes at once, so a key held steps every time.
    pub(crate) fn step_volume(&mut self, of: VolumeOf, delta: f64, cx: &mut Context<Self>) {
        let now = match of {
            VolumeOf::Panel => self.preview.media.state.volume,
            VolumeOf::Dock => match &self.player.pinned {
                Some(pinned) => pinned.state.volume,
                None => return,
            },
        };
        self.set_volume(of, now + delta, cx);
    }

    /// Whose volume the player's keys and the desktop's requests steer:
    /// the pin's (the panel's while the panel shows it), else the panel's
    /// music, as `Shell::playing` says.
    pub(crate) fn steered_volume(&self) -> Option<VolumeOf> {
        if self.player.pinned.is_some() {
            return Some(if self.preview_is_pinned() {
                VolumeOf::Panel
            } else {
                VolumeOf::Dock
            });
        }
        (self.preview.media.session.is_some() && self.media_is_audio()).then_some(VolumeOf::Panel)
    }

    /// `ui.volume_step`: Up and Down on a volume's slider: the volume of the
    /// player whose popup is open, the dock's or the panel's.
    pub fn volume_step(&mut self, delta: f64, window: &mut Window, cx: &mut Context<Self>) {
        let of = if self.volume_open(VolumeOf::Dock) {
            VolumeOf::Dock
        } else {
            VolumeOf::Panel
        };
        // Keyed into, a popup the wheel opened stays as one a click opened.
        if let Some(popup) = self.volume_popup_mut(of) {
            popup.take_keyboard(window, cx);
        }
        self.step_volume(of, delta, cx);
    }

    /// The wheel over a speaker or a volume's slider: up is louder. Over a
    /// speaker whose popup is closed, it opens it to show the level,
    /// passing (`toolbar::Passing`).
    pub(crate) fn volume_wheel(
        &mut self,
        of: VolumeOf,
        event: &gpui::ScrollEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A step a notch, a touchpad's slide a step at 60 px: never more
        // than one for a notch, however the system says it (`Wheeled`).
        let wheeled = crate::reader_scroll::Wheeled::vertical(event);
        let turn = self.player.volume.turn.turn(wheeled, false);
        if turn != 0 {
            if !self.volume_open(of) {
                self.open_volume_passing(of, window, cx);
            }
            self.step_volume(of, -f64::from(turn) * VOLUME_STEP, cx);
        }
        cx.stop_propagation();
    }

    /// `of`'s volume popup opened by the wheel over its speaker, passing
    /// (`toolbar::Passing`): the keyboard stays where it was, a field
    /// being typed in too, until a press in it or a key uses it.
    fn open_volume_passing(&mut self, of: VolumeOf, window: &mut Window, cx: &mut Context<Self>) {
        let key = match of {
            VolumeOf::Panel => format!("preview {VOLUME_POPUP}"),
            VolumeOf::Dock => crate::dock::VOLUME.to_owned(),
        };
        if !self.toggle_popup(&key, &serde_json::Value::Null, window, cx) {
            return;
        }
        self.player.volume.held = false;
        match of {
            VolumeOf::Panel => {
                // The player's bar is the side of what is shown, never the
                // bar of a container's view kept above it (`toolbar`
                // reads `active_popup(shell, false)`): the wheel gave no
                // side the keyboard to say which.
                self.preview.menu_parked = false;
                self.preview.menu = Some(crate::toolbar::Popup::passing(VOLUME_POPUP, cx));
            }
            VolumeOf::Dock => {
                self.toolbar_menu = Some(crate::toolbar::Popup::passing(crate::dock::VOLUME, cx));
            }
        }
        cx.notify();
    }

    /// Whether `of`'s volume popup is open.
    fn volume_open(&self, of: VolumeOf) -> bool {
        match of {
            VolumeOf::Panel => self
                .preview
                .menu
                .as_ref()
                .is_some_and(|menu| menu.kind == VOLUME_POPUP),
            VolumeOf::Dock => self
                .toolbar_menu
                .as_ref()
                .is_some_and(|menu| menu.kind == crate::dock::VOLUME),
        }
    }

    /// `of`'s volume popup, while it is open.
    fn volume_popup_mut(&mut self, of: VolumeOf) -> Option<&mut crate::toolbar::Popup> {
        match of {
            VolumeOf::Panel => self
                .preview
                .menu
                .as_mut()
                .filter(|menu| menu.kind == VOLUME_POPUP),
            VolumeOf::Dock => self
                .toolbar_menu
                .as_mut()
                .filter(|menu| menu.kind == crate::dock::VOLUME),
        }
    }

    /// The pointer onto or off `of`'s speaker (`popup` false) or its
    /// volume popup: a passing popup closes once it has been off both
    /// `VOLUME_LINGER`. Its close is the popup's own, gone with it.
    pub(crate) fn volume_over(
        &mut self,
        of: VolumeOf,
        popup: bool,
        over: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(passing) = self
            .volume_popup_mut(of)
            .and_then(|popup| popup.passing.as_mut())
        else {
            return;
        };
        if popup {
            passing.over.1 = over;
        } else {
            passing.over.0 = over;
        }
        if passing.over.0 || passing.over.1 {
            passing.leaving = None;
            return;
        }
        passing.leaving = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(VOLUME_LINGER).await;
            let _ = this.update_in(cx, |this, window, cx| this.volume_left(of, window, cx));
        }));
    }

    /// The pointer off `of`'s passing popup and its speaker long enough:
    /// that popup closes, if it is still open and still passing.
    fn volume_left(&mut self, of: VolumeOf, window: &mut Window, cx: &mut Context<Self>) {
        let Some(popup) = self.volume_popup_mut(of) else {
            return;
        };
        let Some(passing) = popup.passing.as_mut() else {
            return;
        };
        // This very close, which ends as it runs.
        if let Some(leaving) = passing.leaving.take() {
            leaving.detach();
        }
        if passing.over.0 || passing.over.1 {
            return;
        }
        // The keyboard reached it meanwhile (Tab): used, it stays.
        if popup.focus.contains_focused(window, cx) {
            popup.passing = None;
            return;
        }
        let popup = match of {
            VolumeOf::Panel => self.preview.menu.take(),
            VolumeOf::Dock => self.toolbar_menu.take(),
        };
        if let Some(popup) = popup {
            popup.restore(window, cx);
        }
        cx.notify();
    }

    /// A press in `of`'s volume popup: one the wheel opened takes the
    /// keyboard as a click's does, its slider given it.
    fn volume_used(&mut self, of: VolumeOf, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .volume_popup_mut(of)
            .is_some_and(|popup| popup.take_keyboard(window, cx))
        {
            self.focus_volume(window, cx);
        }
    }

    /// The volume's slider given the keyboard, as its popup opens (a
    /// click's) or is used (one the wheel opened).
    pub(crate) fn focus_volume(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let volume = &mut self.player.volume;
        volume.held = false;
        // A stop of the popup's tab order, before Mute: Tab goes between
        // them, as between a popup's rows.
        let focus = volume
            .focus
            .get_or_insert_with(|| cx.focus_handle().tab_stop(true).tab_index(0))
            .clone();
        window.focus(&focus, cx);
    }
}

/// A phase of dragging a seek bar's knob, with the bar's fraction under the
/// pointer. Whoever owns the player decides what a phase does, and ignores
/// a move or a release it never saw the press of.
pub type Scrub = Rc<dyn Fn(&mut Shell, &str, f32, &mut Window, &mut Context<Shell>)>;

/// A seek bar drawn at `fraction` (`controls::slider`), reporting presses,
/// moves and releases through `scrub`, which ignores a move or a release
/// without a press: `held` while a knob is dragged, followed then over the
/// whole window. The same bar serves the panel's player and the dock's;
/// their keys are the player's (Left and Right seek). To assistive
/// technology it is a slider of the position in seconds, said as its time
/// of the whole (`at`: position and duration, while a screen reader
/// listens).
pub(crate) fn seek_bar_of(
    id: &'static str,
    fraction: f32,
    held: bool,
    at: Option<(f64, f64)>,
    theme: &Theme,
    scrub: Scrub,
    cx: &mut Context<Shell>,
) -> AnyElement {
    use crate::controls::{Slid, Slide};
    let slid = cx.listener(move |this, slid: &Slid, window, cx| {
        let phase = match slid.slide {
            Slide::Press => "down",
            Slide::Drag => "move",
            Slide::Release => "up",
        };
        scrub(this, phase, slid.at, window, cx);
    });
    crate::controls::slider(id, fraction, held, theme, Rc::new(slid))
        .aria_label(
            words!(
                media_position = "Position",
                note = "What a screen reader calls a player's seek bar."
            )
            .say(),
        )
        .when_some(at, |d, (position, duration)| {
            d.aria_numeric_value(position)
                .aria_min_numeric_value(0.)
                .aria_max_numeric_value(duration)
                .aria_value(
                    words!(
                        media_position_of = "{ $position } of { $duration }",
                        position = time(position),
                        duration = time(duration),
                        note = "What a screen reader hears of a player's seek bar: where it is (1:02), of how long the track lasts (3:45)."
                    )
                    .say(),
                )
        })
        .flex_1()
        .min_w(sized(40.))
        .h(sized(30.))
        .into_any_element()
}

/// The panel's seek bar: scrubbing goes through `ui.media_scrub` on the
/// panel's pane.
fn seek_bar(shell: &Shell, theme: &Theme, cx: &mut Context<Shell>) -> AnyElement {
    let state = &shell.preview.media.state;
    let duration = state.duration.unwrap_or(0.);
    let position = shell.preview.media.scrub.unwrap_or(state.position);
    let fraction = if duration > 0. {
        (position / duration).clamp(0., 1.) as f32
    } else {
        0.
    };
    let pane = shell.pane;
    let scrub: Scrub = Rc::new(move |this, phase, fraction, window, cx| {
        if phase != "down" && this.preview.media.scrub.is_none() {
            return;
        }
        this.with_pane(pane, |this| {
            this.run(
                "ui.media_scrub",
                serde_json::json!({"phase": phase, "fraction": fraction}),
                window,
                cx,
            );
        });
    });
    let at = shell.a11y.then_some((position, duration));
    let held = shell.preview.media.scrubbing();
    seek_bar_of("video-seek", fraction, held, at, theme, scrub, cx)
}

pub fn toolbar(shell: &Shell, theme: &Theme, cx: &mut Context<Shell>) -> AnyElement {
    use crate::preview_bar::{Bar, Item, Slot};
    let state = &shell.preview.media.state;
    let mut bar = Bar::new("video-tools");
    crate::preview_bar::close_tool(&mut bar, shell, theme);
    // Other audio files in the folder: the track before and the one after.
    let (previous, next) = shell.track_neighbours();
    let step = |id, icon, title, delta: i64, there: bool| {
        Button::new(id, icon, title, "ui.media_step")
            .args(serde_json::json!({"delta":delta}))
            .enabled(there)
    };
    let tracks = previous || next;
    if tracks {
        bar.push(Item::button(
            Slot::Navigate,
            1,
            step(
                "audio-previous",
                "skip-previous",
                previous_track().say(),
                -1,
                previous,
            ),
        ));
    }
    bar.push(Item::button(
        Slot::Navigate,
        2,
        control(
            "video-play",
            if state.playing() { "pause" } else { "play" },
            play_pause().say(),
            "play",
            serde_json::Value::Null,
        ),
    ));
    if tracks {
        bar.push(Item::button(
            Slot::Navigate,
            1,
            step("audio-next", "skip-next", next_track().say(), 1, next),
        ));
    }
    bar.push(Item::widget(Slot::Navigate, 1, |shell, _| {
        let media = &shell.preview.media;
        div()
            .text_body()
            .flex_none()
            .child(time(media.scrub.unwrap_or(media.state.position)))
            .into_any_element()
    }));
    let seek_theme = theme.clone();
    bar.push(
        Item::widget(Slot::Navigate, 2, move |shell, cx| {
            seek_bar(shell, &seek_theme, cx)
        })
        .fills(),
    );
    let muted = theme.text_muted;
    bar.push(
        Item::widget(Slot::Navigate, 0, move |shell, _| {
            div()
                .text_body()
                .flex_none()
                .text_color(muted)
                .child(duration(&shell.preview.media.state))
                .into_any_element()
        })
        // The first the bar leaves out: then More says where it is, of
        // what (the position goes next), and only then.
        .in_more(Rc::new(|shell, _| {
            let media = &shell.preview.media;
            div()
                .px_2()
                .text_body()
                .child(format!(
                    "{} / {}",
                    time(media.scrub.unwrap_or(media.state.position)),
                    duration(&media.state)
                ))
                .into_any_element()
        })),
    );
    // The speaker opens the volume; the wheel over it moves the volume,
    // the popup opened to show it (`toolbar::Passing`).
    bar.push(
        Item::button(
            Slot::View,
            0,
            volume_button("video-volume", state, "ui.preview_menu", VOLUME_POPUP),
        )
        .opens(VOLUME_POPUP)
        .wheel(Rc::new(|this, event, window, cx| {
            this.volume_wheel(VolumeOf::Panel, event, window, cx);
        }))
        .hovered(Rc::new(|this, over, window, cx| {
            this.volume_over(VolumeOf::Panel, false, over, window, cx);
        })),
    );
    bar.popups.push((VOLUME_POPUP, Rc::new(volume_popup)));
    // What follows the track: in the row while it has room, in More after.
    let mode = shell.prefs.player.continue_with;
    bar.push(
        Item::button(
            Slot::View,
            1,
            Button::new(
                "media-continue",
                mode.icon(),
                words!(
                    media_continue_tip = "When a track ends: { $mode }",
                    mode = mode.label(),
                    note = "Pointing at the button of what a player does when a track ends. mode: Stop, Play next or Shuffle."
                )
                .say(),
                "ui.preview_menu",
            )
            .args(serde_json::json!({ "kind": CONTINUE_POPUP }))
            .active(mode != crate::player::Continue::Stop),
        )
        .opens(CONTINUE_POPUP),
    );
    bar.popups.push((CONTINUE_POPUP, Rc::new(continue_popup)));
    // The bytes, a row of More as in every preview that has them; shown,
    // the bytes' own bar has the way back, lit at its left (`look_tools`).
    crate::preview_bar::look_tools(
        &mut bar,
        crate::preview_bar::Looks {
            hex: crate::preview_toolbar::hex_available(shell)
                .then_some(crate::preview_bar::Hex::Row),
            ways: crate::preview_bar::ways(shell, false, None),
            ..crate::preview_bar::Looks::default()
        },
    );
    crate::preview_bar::fullscreen_tool(&mut bar, shell, theme);
    bar.settings = Some(std::rc::Rc::new(settings));
    crate::preview_bar::render(
        &bar,
        crate::preview::active_popup(shell, false),
        shell,
        theme,
        cx,
    )
}

/// The popup of what follows a track that ends (`ui.media_continue`).
pub const CONTINUE_POPUP: &str = "continue";

fn continue_popup(shell: &Shell, theme: &Theme, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let popup = shell.preview.menu.as_ref()?;
    Some(
        crate::toolbar::menu_parts(
            crate::toolbar::menu_panel(popup, "ui.preview_menu", theme, cx),
            continue_parts(shell),
            shell,
            theme,
            cx,
        )
        .into_any_element(),
    )
}

/// What follows a track that ends, one checked row a choice under its
/// title: the panel's popup and the dock's More.
pub(crate) fn continue_parts(shell: &Shell) -> Vec<crate::toolbar::MenuPart> {
    use crate::{player::Continue, toolbar::MenuPart};
    let mut parts = vec![MenuPart::Title(
        words!(
            media_continue_title = "When a track ends",
            note = "The title of the choices of what a player does when a track ends."
        )
        .say()
        .into(),
        None,
    )];
    for mode in Continue::ALL {
        let label = mode.label().say();
        parts.push(MenuPart::Row(
            Button::new(
                format!("continue-{}", mode.key()),
                "",
                label.clone(),
                "ui.media_continue",
            )
            .args(serde_json::json!({ "mode": mode.key() }))
            .label(label)
            .active(shell.prefs.player.continue_with == mode)
            .menu(),
        ));
    }
    parts
}

/// How a row of a player's menu reaches its player: the row's id, its
/// title, and the `action` with its `value`, named as `ui.media_control`
/// takes them for the panel's player and as a `PlayerRequest` names them
/// for the pinned one (`dock.rs`).
pub(crate) type Send<'a> = &'a dyn Fn(String, String, &str, serde_json::Value) -> Button;

/// What playback has no button for: whether it goes on while you browse,
/// whether it starts on selection, volume, speed and the tracks.
fn settings(shell: &Shell, theme: &Theme, cx: &mut Context<Shell>) -> Vec<AnyElement> {
    use crate::toolbar::MenuPart;
    let state = &shell.preview.media.state;
    let toggle = |id: &'static str, icon: &'static str, title: Words, command, on| {
        let title = title.say();
        MenuPart::Row(
            Button::new(id, icon, title.clone(), command)
                .label(title)
                .active(on)
                .menu(),
        )
    };
    let mut parts = vec![
        toggle(
            "media-keep-playing",
            "pin",
            words!(
                media_keep_playing = "Keep playing while you browse",
                note = "A player's menu row: go on playing when another file is selected."
            ),
            "ui.media_keep_playing",
            shell.prefs.player.keep_playing,
        ),
        toggle(
            "media-autoplay",
            "play",
            words!(
                media_autoplay = "Play on selection",
                note = "A player's menu row: start playing a sound or a film as soon as it is selected."
            ),
            "ui.media_autoplay",
            shell.prefs.player.autoplay,
        ),
        MenuPart::Line,
    ];
    let send =
        |id: String, title: String, action: &str, value| control(id, "", title, action, value);
    parts.extend(playback_parts(state, &send));
    crate::toolbar::menu_part_elements(parts, shell, theme, cx)
}

/// The speed and a film's tracks, as parts of a player's menu reaching it
/// through `send`: the panel's More and the dock's. The speed is a setting
/// on one row, its values segments; the tracks need their words, checked
/// rows under their title (ADR-099). The volume is the speaker's popup
/// (`volume_control`).
pub(crate) fn playback_parts(state: &Playback, send: Send) -> Vec<crate::toolbar::MenuPart> {
    use crate::toolbar::MenuPart;
    let speeds = [0.5, 1., 1.25, 1.5, 2.]
        .into_iter()
        .map(|value| {
            let label = format!("{value}×");
            send(format!("rate-{label}"), label.clone(), "rate", value.into())
                .label(label)
                .active((state.rate - value).abs() < 0.01)
                .enabled(state.seekable)
                .segment()
        })
        .collect();
    let mut parts = vec![MenuPart::Choice(
        words!(
            media_speed = "Playback speed",
            note = "A player's menu row: how fast it plays, chosen among 0.5×, 1×, 2×."
        )
        .say()
        .into(),
        speeds,
    )];
    // A choice of tracks: one sound track is none, one subtitle track is
    // still a choice with Off.
    for (title, action, tracks, selected, least) in [
        (
            words!(
                media_audio_tracks = "Audio",
                note = "The title of a film's sound tracks, among which one is chosen (its languages)."
            ),
            "audio",
            &state.audio_tracks,
            state.audio,
            2,
        ),
        (
            words!(
                media_subtitles = "Subtitles",
                note = "The title of a film's subtitles, among which one is chosen, or none."
            ),
            "subtitle",
            &state.subtitle_tracks,
            state.subtitle,
            1,
        ),
    ] {
        if tracks.len() < least {
            continue;
        }
        parts.push(MenuPart::Title(title.say().into(), None));
        if action == "subtitle" {
            let off = words!(
                media_subtitles_off = "Off",
                note = "The choice of no subtitles for a film."
            )
            .say();
            parts.push(MenuPart::Row(
                send("subtitles-off".into(), off.clone(), action, (-1).into())
                    .label(off)
                    .active(selected == -1)
                    .menu(),
            ));
        }
        for (i, label) in tracks.iter().enumerate() {
            parts.push(MenuPart::Row(
                send(format!("{action}-{i}"), label.clone(), action, i.into())
                    .label(label.clone())
                    .active(i as i32 == selected)
                    .menu(),
            ));
        }
    }
    parts
}

/// The colour a cover is drawn with, worked out once per cover through
/// `cache`, never per frame; the accent without a cover or for a grey one.
pub(crate) fn cover_tint(
    cover: Option<&Arc<RenderImage>>,
    cache: &Cell<Option<(gpui::ImageId, Option<gpui::Hsla>)>>,
    theme: &Theme,
) -> gpui::Hsla {
    cover
        .and_then(|cover| {
            if let Some((id, colour)) = cache.get()
                && id == cover.id
            {
                return colour;
            }
            let colour = cover
                .as_bytes(0)
                .and_then(|bgra| crate::elements::halo::tint(bgra, cover.size(0).width.0 as usize))
                .map(crate::elements::halo::vivid);
            cache.set(Some((cover.id, colour)));
            colour
        })
        .unwrap_or(theme.accent)
}

/// The square the halo draws in, as wide as the panel allows, giving way in
/// height when the panel is short: the halo draws in the smaller of the two,
/// centred, so a flash laid in this box lands on the cover's disc.
pub(crate) fn halo_square(
    state: &Playback,
    cover: Option<Arc<RenderImage>>,
    tint: gpui::Hsla,
    theme: &Theme,
) -> gpui::Div {
    div()
        .relative()
        .w_full()
        // A picture: its size whatever the text's.
        .max_w(px(420.))
        .min_h_0()
        .flex_shrink_1()
        .map(|mut d| {
            d.style().aspect_ratio = Some(1.);
            d
        })
        .mb_2()
        .child(crate::elements::halo::Halo {
            spectrum: state.spectrum.clone(),
            playing: state.playing(),
            cover,
            tint,
            accent: theme.accent,
            surface: theme.code_bg,
            border: theme.border,
        })
}

/// The audio player's figure: `picture` (a `halo_square`, with whatever
/// the caller laid over it) with the title and the artist and album under
/// it. Drawn for the panel and, pinned, for a tall dock. A line cut short
/// says itself whole when pointed at, then `also`, the tooltip of what the
/// figure lies in.
pub(crate) fn audio_figure(
    state: &Playback,
    title: String,
    picture: AnyElement,
    also: Option<&'static str>,
    theme: &Theme,
) -> gpui::Div {
    let line = |id: &'static str, text: String| {
        crate::elements::clip::clipped_with(
            div().id(id).flex_none().max_w_full().truncate(),
            text,
            also.map(gpui::SharedString::from),
        )
    };
    div()
        .flex_1()
        .min_h_0()
        .p_4()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_1()
        .child(picture)
        .child(line("figure-title", title).text_title())
        .child(line("figure-artist", state.artist.clone()).text_color(theme.text_muted))
        .child(line("figure-album", state.album.clone()).text_color(theme.text_muted))
}

/// The panel's audio player: the figure, with a click anywhere on it
/// answered as a player's when it is interactive (the flash lands on the
/// disc). Cover and metadata remain visible during playback; the spectrum
/// has no UI timer.
pub fn audio_content(
    shell: &Shell,
    theme: &Theme,
    interactive: bool,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let p = &shell.preview;
    // The track's cover, or its thumbnail standing in for it meanwhile.
    let cover = match p
        .content
        .as_ref()
        .or_else(|| crate::preview::placeholder(shell))
    {
        Some(PreviewContent::Image { image, .. }) => Some(image.clone()),
        _ => None,
    };
    let tint = cover_tint(cover.as_ref(), &p.media.tint, theme);
    let square = halo_square(&p.media.state, cover, tint, theme)
        .children(flash(Flash::Preview, shell, 1.))
        .into_any_element();
    let title = p
        .playing_path()
        .map(|path| track_title(&p.media.state, &path))
        .unwrap_or_default();
    let figure = audio_figure(&p.media.state, title, square, None, theme);
    if interactive {
        clickable("audio-surface", figure, Flash::Preview, cx).into_any_element()
    } else {
        figure.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A film's frame is a film wherever it comes from (the panel's player,
    /// the pinned one shown again): Fit enlarges a small one.
    #[test]
    fn a_film_s_frame_is_laid_out_as_a_film() {
        let image = crate::thumbs::to_render_image(&skwir_core::thumbs::Image {
            width: 2,
            height: 2,
            rgba: vec![255; 16].into(),
        })
        .expect("a picture");
        let frame = film_frame(image, 2, 2, &Playback::default());
        assert_eq!(
            crate::preview::ShownAs::of(&frame),
            Some(crate::preview::ShownAs::Film)
        );
    }

    /// The speaker has as many waves as it is loud, none at nothing, and
    /// is crossed out when muted, whatever its level.
    #[test]
    fn the_speaker_says_how_loud() {
        let at = |volume: f64, muted: bool| {
            speaker_icon(&Playback {
                volume,
                muted,
                ..Playback::default()
            })
        };
        assert_eq!(at(0., false), "volume-none");
        assert_eq!(at(0.05, false), "volume-low");
        assert_eq!(at(0.49, false), "volume-low");
        assert_eq!(at(0.5, false), "volume");
        assert_eq!(at(1., false), "volume");
        assert_eq!(at(1., true), "muted");
        assert_eq!(at(0., true), "muted");
        for name in ["volume-none", "volume-low", "volume", "muted"] {
            assert!(
                crate::toolbar::asset(&format!("toolbar/{name}.svg")).is_some(),
                "{name}"
            );
        }
    }

    /// Entries past a few are typed by their suffix, once each, an
    /// inode's type before its name: what Play and a pin's queue ask of a
    /// whole folder.
    #[test]
    fn many_entries_are_typed_as_one_selection() {
        // More than a few names: asked by suffix, the inode's type first.
        let names: Vec<std::ffi::OsString> = (0..200)
            .map(|n| format!("song {n}.flac").into())
            .chain([std::ffi::OsString::from("Album")])
            .collect();
        let parts = names.iter().enumerate().map(|(at, name)| {
            let inode = (at == 200).then_some("inode/directory");
            (inode, name.as_os_str())
        });
        let types = types_of(parts);
        assert_eq!(types.len(), 201);
        let flac = skwir_platform::mime::by_name(std::ffi::OsStr::new("song.flac"));
        assert!(flac.is_some());
        assert!(types[..200].iter().all(|mime| *mime == flac));
        assert_eq!(
            types[200].as_ref().map(skwir_platform::mime::Mime::as_str),
            Some("inode/directory")
        );
    }

    /// Play goes by the file's type: music to the pin, a film to the quick
    /// preview, a playlist (filed as audio or video by the database) gives
    /// its tracks; anything else does not play.
    #[test]
    fn what_play_does_goes_by_the_type() {
        let played = |name: &str| {
            let name = std::ffi::OsStr::new(name);
            played_as(&skwir_platform::mime::by_name(name)?, name)
        };
        assert_eq!(played("song.flac"), Some(Played::Track));
        assert_eq!(played("song.ogg"), Some(Played::Track));
        assert_eq!(played("film.mkv"), Some(Played::Film));
        assert_eq!(played("mix.m3u"), Some(Played::Playlist));
        assert_eq!(played("mix.pls"), Some(Played::Playlist));
        assert_eq!(played("notes.txt"), None);
        assert_eq!(played("photo.png"), None);
    }

    #[test]
    fn a_seek_is_asked_for_on_release_of_a_press_it_saw() {
        let mut knob = None;
        assert_eq!(scrub(&mut knob, Some("move"), 3.), None);
        assert_eq!(knob, None, "a move without a press is ignored");
        assert_eq!(scrub(&mut knob, Some("up"), 3.), None);
        assert_eq!(scrub(&mut knob, Some("down"), 1.), None);
        assert_eq!(scrub(&mut knob, Some("move"), 2.), None);
        assert_eq!(knob, Some(2.));
        assert_eq!(scrub(&mut knob, Some("up"), 4.), Some(4.));
        assert_eq!(knob, None);
    }

    #[test]
    fn the_flash_holds_before_it_fades() {
        let (start, _) = flash_at(0.);
        let (held, _) = flash_at(FLASH_HOLD * 0.9);
        let (late, _) = flash_at(0.8);
        let (end, side) = flash_at(1.);
        assert!((start - 1.).abs() < 1e-6 && (held - 1.).abs() < 1e-6);
        assert!(late < 1. && late > 0.);
        assert!(end.abs() < 1e-6 && side > 80.);
    }
}
