//! Embedded playback controls. Only the session manager touches media I/O.
use crate::{preview::PreviewContent, shell::Shell, theme::Theme, toolbar::Button};
use bibli_core::media::{Control, Frame, MediaSession, Playback};
use gpui::{
    AnimationExt, AnyElement, Bounds, Context, FocusHandle, MouseButton, Pixels, RenderImage,
    Window, div, prelude::*, px,
};
use std::{cell::Cell, rc::Rc, sync::Arc, time::Duration};

#[derive(Default)]
enum Poster {
    #[default]
    Waiting,
    Thumbnail,
    Unavailable,
    Dismissed,
}

struct FullscreenRestore {
    entered: bool,
    was_peek: bool,
}

pub struct Fullscreen {
    pub controls_hidden: bool,
    restore: FullscreenRestore,
    controls_hovered: bool,
    controls_focus: FocusHandle,
    pointer: gpui::Point<Pixels>,
    hide_task: Option<gpui::Task<()>>,
}

impl MediaState {
    pub(crate) fn accept_poster(&mut self) -> bool {
        if self.session.is_none() || matches!(self.poster, Poster::Dismissed) {
            return false;
        }
        self.poster = Poster::Thumbnail;
        self.initial_frame = None;
        true
    }
}

#[derive(Default)]
pub struct MediaState {
    pub session: Option<Arc<dyn MediaSession>>,
    pub state: Playback,
    pub focus: Option<FocusHandle>,
    pub fullscreen: Option<Fullscreen>,
    poster: Poster,
    initial_frame: Option<Frame>,
    scrub: Option<f64>,
    /// The file the session went on to by itself (gapless), for whoever
    /// follows the session's track: taken by the pin.
    pub advanced: Option<std::path::PathBuf>,
    /// Ends this preview's media worker. A reader's lifetime ends with the
    /// preview; a session that is pinned must not, so it has its own, which
    /// the pin takes with it (`release_lifetime`).
    lifetime: bibli_core::CancelToken,
    /// The colour of the cover shown, by image: worked out once, when the
    /// cover changes, not per frame.
    tint: std::cell::Cell<Option<(gpui::ImageId, Option<gpui::Hsla>)>>,
    /// The answer to a click on the picture: which way playback went, shown
    /// in the middle for an instant. The number restarts the entrance for
    /// every click.
    flash: Option<(u64, bool)>,
    flashes: u64,
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
        self.poster = Poster::Dismissed;
        self.initial_frame = None;
    }

    /// The lifetime the next session of this preview is opened with.
    pub(crate) fn lifetime(&self) -> bibli_core::CancelToken {
        self.lifetime.clone()
    }

    /// Hand the session's lifetime to the pin: this preview going away no
    /// longer ends it.
    pub(crate) fn release_lifetime(&mut self) -> bibli_core::CancelToken {
        std::mem::take(&mut self.lifetime)
    }

    /// Take a lifetime back from the pin: the session is this preview's again.
    pub(crate) fn adopt_lifetime(&mut self, lifetime: bibli_core::CancelToken) {
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

    /// How far a search for the neighbouring audio file looks: a folder of a
    /// hundred thousand documents with two songs in it is not scanned whole.
    const NEIGHBOUR_REACH: usize = 4000;

    /// The audio files around entry `id` in the pane's order.
    pub(crate) fn audio_neighbours(
        &self,
        id: bibli_core::EntryId,
    ) -> (Option<usize>, Option<usize>) {
        let sorted = self.ui().list.sorted.clone();
        let Some(here) = sorted.iter().position(|entry| *entry == id) else {
            return (None, None);
        };
        let ws = self.app.workspace.read();
        let Some(pane) = ws.pane(self.pane) else {
            return (None, None);
        };
        let audio = |index: &usize| {
            pane.listing.get(sorted[*index]).is_some_and(|entry| {
                !entry.is_dir()
                    && bibli_platform::mime::by_name(&entry.name)
                        .is_some_and(|mime| mime.as_str().starts_with("audio/"))
            })
        };
        (
            (here.saturating_sub(Self::NEIGHBOUR_REACH)..here)
                .rev()
                .find(audio),
            (here + 1..sorted.len().min(here + 1 + Self::NEIGHBOUR_REACH)).find(audio),
        )
    }

    /// `ui.media_step { delta }`: the audio file after or before this one. It
    /// is the pane's selection that moves; the preview follows it as always,
    /// and keeps playing if it was.
    /// Whether there is a track before this one, and one after: in the
    /// playlist the panel went into, otherwise in the folder.
    pub(crate) fn track_neighbours(&self) -> (bool, bool) {
        if let Some(queue) = &self.preview.queue {
            return (queue.index > 0, queue.index + 1 < queue.targets.len());
        }
        let (previous, next) = self.preview.audio_neighbours;
        (previous.is_some(), next.is_some())
    }

    pub fn media_step(&mut self, delta: i64, window: &mut Window, cx: &mut Context<Self>) {
        let playing = self.preview.media.state.playing() || self.preview.media.state.ended();
        if let Some(queue) = &mut self.preview.queue {
            let Some(index) = queue
                .index
                .checked_add_signed(delta.signum() as isize)
                .filter(|index| *index < queue.targets.len())
            else {
                return;
            };
            queue.index = index;
            let args = serde_json::json!({
                "action": "member",
                "depth": queue.depth,
                "target": queue.targets[index],
            });
            self.preview.autoplay = playing;
            self.run("ui.inspect", args, window, cx);
            return;
        }
        let (previous, next) = self.preview.audio_neighbours;
        let Some(target) = (if delta < 0 { previous } else { next }) else {
            return;
        };
        let Some(pane) = self.preview.pane() else {
            return;
        };
        self.preview.autoplay = playing;
        self.with_pane(pane, |this| {
            this.cursor_to(target as isize, false, window, cx);
        });
    }

    pub fn sync_media(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = &self.preview.media.session else {
            return;
        };
        let update = session.take_update();
        if let Some(path) = update.advanced {
            self.preview.media.advanced = Some(path);
        }
        if let Some(state) = update.state {
            // A track that plays to its end hands over to the next one.
            let finished = state.ended() && !self.preview.media.state.ended();
            self.preview.media.state = state;
            if finished && self.media_is_audio() && self.track_neighbours().1 {
                self.media_step(1, window, cx);
                return;
            }
        }
        let mut frame = update.frame;
        if !matches!(self.preview.media.poster, Poster::Dismissed) {
            if matches!(self.preview.media.poster, Poster::Waiting) {
                use bibli_core::thumbs::ThumbState;
                match self.video_thumbnail() {
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
                                }),
                                window,
                                cx,
                            );
                        }
                        self.preview.media.poster = Poster::Thumbnail;
                        self.preview.media.initial_frame = None;
                    }
                    ThumbState::Unavailable => {
                        self.preview.media.poster = Poster::Unavailable;
                        frame = frame.or_else(|| self.preview.media.initial_frame.take());
                    }
                    ThumbState::Pending => {
                        if frame.is_some() {
                            self.preview.media.initial_frame = frame.take();
                        }
                    }
                }
            }
            if !matches!(self.preview.media.poster, Poster::Unavailable) {
                frame = None;
            }
        }
        if let Some(frame) = frame {
            let Some(buffer) = image::RgbaImage::from_raw(frame.width, frame.height, frame.bgra)
            else {
                return;
            };
            let image = Arc::new(RenderImage::new(smallvec::SmallVec::from_buf([
                image::Frame::new(buffer),
            ])));
            let state = &self.preview.media.state;
            self.replace_preview_content(
                Some(PreviewContent::Image {
                    image,
                    width: frame.width,
                    height: frame.height,
                    detail: Some(bibli_core::preview::PreviewInfo::Video {
                        seconds: state.position,
                        duration: state.duration,
                    }),
                    document: None,
                    facts: Vec::new(),
                }),
                window,
                cx,
            );
        }
        if let Some(notice) = update.notice {
            // The session continues; its next state update corrects the UI.
            self.set_status(notice, false, cx);
        }
        if let Some(error) = update.error {
            // The manager has ended the session. Withdraw the controls and
            // keep the last picture; every later control would only fail.
            self.media_exit_fullscreen(window, cx);
            self.preview.media.session = None;
            self.preview.media.state.status = bibli_core::media::PlaybackStatus::Paused;
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
                    Some(peek) => window.focus(peek),
                    None => window.focus(&self.ui().focus),
                }
            }
            self.set_status_about(error, "preview-media.backend", cx);
        }
        self.arm_media_hide(window, cx);
    }
    pub fn media_send(&mut self, command: Control, cx: &mut Context<Self>) {
        if let Some(session) = &self.preview.media.session {
            match session.control(command) {
                Ok(()) => {
                    if let Control::Seek(position) = command {
                        self.preview.media.state.position =
                            position.min(self.preview.media.state.duration.unwrap_or(position));
                    }
                    if matches!(command, Control::Play(true) | Control::Seek(_))
                        && !self.media_is_audio()
                    {
                        self.preview.media.poster = Poster::Dismissed;
                        self.preview.media.initial_frame = None;
                        self.cancel_pending_preview();
                    }
                }
                Err(error) => self.set_status(error.to_string(), true, cx),
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
                    self.media_fullscreen(window, cx);
                }
                playing
            }
            Flash::Dock => {
                let Some(pinned) = &self.player.pinned else {
                    return;
                };
                let (playing, video) = (!pinned.state.playing(), pinned.video);
                self.player_request(bibli_core::playing::PlayerRequest::PlayPause, window, cx);
                if double && video {
                    // Fullscreen is the panel's: show the film there first.
                    self.player.fullscreen_wanted = true;
                    self.player_reveal(window, cx);
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
            Some("volume") => value.map(Control::Volume),
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
            // One player at a time: starting another track while one is
            // pinned moves the pin to it.
            let takes_over = matches!(command, Control::Play(true))
                && self.player.pinned.is_some()
                && !self.preview_is_pinned();
            self.media_send(command, cx);
            if takes_over {
                self.media_pin(cx);
            }
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
        match args.get("phase").and_then(serde_json::Value::as_str) {
            Some("down") => self.preview.media.scrub = Some(fraction * duration),
            Some("move") if self.preview.media.scrub.is_some() => {
                self.preview.media.scrub = Some(fraction * duration);
            }
            Some("up") if self.preview.media.scrub.take().is_some() => {
                self.media_send(Control::Seek(fraction * duration), cx);
            }
            _ => {}
        }
        cx.notify();
    }
    pub fn media_fullscreen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.preview.media.fullscreen.is_some() {
            self.media_exit_fullscreen(window, cx);
            return;
        }
        if self.preview.media.session.is_none() {
            return;
        }
        let entered = !window.is_fullscreen();
        if entered {
            window.toggle_fullscreen();
        }
        self.preview.media.fullscreen = Some(Fullscreen {
            restore: FullscreenRestore {
                entered,
                was_peek: self.preview.peek,
            },
            controls_hidden: false,
            controls_hovered: false,
            controls_focus: cx.focus_handle(),
            pointer: window.mouse_position(),
            hide_task: None,
        });
        self.preview.peek = true;
        let focus = self
            .preview
            .media
            .focus
            .clone()
            .unwrap_or_else(|| cx.focus_handle());
        self.preview
            .peek_focus
            .get_or_insert_with(|| cx.focus_handle());
        window.focus(&focus);
        self.preview.menu = None;
        self.arm_media_hide(window, cx);
        cx.notify();
    }
    pub fn media_exit_fullscreen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(fullscreen) = self.preview.media.fullscreen.take() else {
            return;
        };
        if fullscreen.restore.entered && window.is_fullscreen() {
            window.toggle_fullscreen();
        }
        self.preview.peek = fullscreen.restore.was_peek;
        self.preview.menu = None;
        if !self.preview.peek {
            window.focus(&self.ui().focus);
        }
        cx.notify();
    }
    pub fn media_toggle_controls(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(fullscreen) = &mut self.preview.media.fullscreen {
            fullscreen.controls_hidden = !fullscreen.controls_hidden;
            fullscreen.hide_task = None;
            self.preview.menu = None;
            if let Some(focus) = &self.preview.media.focus {
                window.focus(focus);
            }
            cx.notify();
        }
    }

    fn arm_media_hide(&mut self, window: &Window, cx: &mut Context<Self>) {
        let Some(fullscreen) = &mut self.preview.media.fullscreen else {
            return;
        };
        if fullscreen.controls_hidden
            || fullscreen.controls_hovered
            || fullscreen.hide_task.is_some()
            || self.preview.menu.is_some()
            || self.preview.media.scrub.is_some()
            || fullscreen.controls_focus.contains_focused(window, cx)
        {
            return;
        }
        let focus = fullscreen.controls_focus.clone();
        let pane = self.pane;
        tracing::trace!("fullscreen controls: inactivity timer armed");
        fullscreen.hide_task = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.pane != pane {
                    return;
                }
                let Some(fullscreen) = &mut this.preview.media.fullscreen else {
                    return;
                };
                fullscreen.hide_task = None;
                tracing::trace!(
                    hovered = fullscreen.controls_hovered,
                    menu = this.preview.menu.is_some(),
                    scrub = this.preview.media.scrub.is_some(),
                    focused = focus.contains_focused(window, cx),
                    "fullscreen controls: inactivity deadline"
                );
                if !fullscreen.controls_hovered
                    && this.preview.menu.is_none()
                    && this.preview.media.scrub.is_none()
                    && !focus.contains_focused(window, cx)
                {
                    fullscreen.controls_hidden = true;
                    cx.notify();
                }
            });
        }));
    }

    fn media_pointer_moved(
        &mut self,
        position: gpui::Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(fullscreen) = &mut self.preview.media.fullscreen else {
            return;
        };
        // GPUI can redispatch a stationary pointer after layout/paint. Only real
        // movement should reveal the toolbar or restart its inactivity deadline.
        if fullscreen.pointer == position {
            return;
        }
        fullscreen.pointer = position;
        tracing::trace!(
            hovered = fullscreen.controls_hovered,
            menu = self.preview.menu.is_some(),
            focused = fullscreen.controls_focus.contains_focused(window, cx),
            "fullscreen controls: pointer moved"
        );
        fullscreen.hide_task = None;
        if !fullscreen.controls_hovered
            && self.preview.menu.is_none()
            && fullscreen.controls_focus.contains_focused(window, cx)
            && let Some(focus) = &self.preview.media.focus
        {
            window.focus(focus);
        }
        if fullscreen.controls_hidden {
            fullscreen.controls_hidden = false;
            cx.notify();
        }
        self.arm_media_hide(window, cx);
    }

    fn media_controls_hovered(
        &mut self,
        hovered: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(fullscreen) = &mut self.preview.media.fullscreen {
            fullscreen.controls_hovered = hovered;
            fullscreen.hide_task = None;
            if !hovered
                && self.preview.menu.is_none()
                && fullscreen.controls_focus.contains_focused(window, cx)
                && let Some(focus) = &self.preview.media.focus
            {
                window.focus(focus);
            }
        }
        self.arm_media_hide(window, cx);
    }
}

/// A full-window canvas, independent of the preview card's chrome and zoom.
/// The image fills its bounds with Contain, including upscaling small videos.
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

/// The picture with the click behaviour of a player on it: `inner` is the
/// film or the audio figure, laid out as it was, and the flash sits over its
/// middle while it lasts.
pub fn surface(
    id: &'static str,
    inner: AnyElement,
    on: Flash,
    shell: &Shell,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let flash = match on {
        Flash::Preview => shell.preview.media.flash,
        Flash::Dock => shell.player.flash,
    };
    let flash = flash.map(|(seq, playing)| {
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
                            .size(px(28.))
                            .text_color(gpui::white()),
                    )
                    .with_animation(
                        ("media-flash", seq),
                        gpui::Animation::new(Duration::from_millis(FLASH_MS)),
                        |disc, delta| {
                            let (opacity, side) = flash_at(delta);
                            disc.w(px(side)).h(px(side)).opacity(opacity)
                        },
                    ),
            )
    });
    div()
        .id(id)
        .relative()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .child(inner)
        .children(flash)
        .on_click(
            cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                this.media_clicked(on, event, window, cx);
            }),
        )
        .into_any_element()
}

pub fn fullscreen(shell: &Shell, cx: &mut Context<Shell>) -> AnyElement {
    let theme = cx.global::<Theme>().clone();
    let hidden = shell
        .preview
        .media
        .fullscreen
        .as_ref()
        .is_some_and(|s| s.controls_hidden);
    div()
        .id("video-fullscreen-canvas")
        .absolute()
        .inset_0()
        .bg(gpui::black())
        .occlude()
        .key_context("MediaPlayer")
        .on_mouse_move(
            cx.listener(|this, event: &gpui::MouseMoveEvent, window, cx| {
                this.media_pointer_moved(event.position, window, cx);
            }),
        )
        .when_some(shell.preview.media.focus.as_ref(), |d, focus| {
            let handle = focus.clone();
            d.track_focus(focus)
                .on_mouse_down(MouseButton::Left, move |_, window, _| window.focus(&handle))
        })
        .when_some(shell.preview.content.as_ref(), |d, content| {
            if let PreviewContent::Image { image, .. } = content {
                // The surface is a flex child: give it a column to fill.
                d.child(
                    div().absolute().inset_0().flex().flex_col().child(surface(
                        "video-fullscreen-surface",
                        gpui::img(image.clone())
                            .absolute()
                            .size_full()
                            .object_fit(gpui::ObjectFit::Contain)
                            .into_any_element(),
                        Flash::Preview,
                        shell,
                        cx,
                    )),
                )
            } else {
                d
            }
        })
        .when(!hidden, |d| {
            d.child(
                div()
                    .id("video-fullscreen-controls")
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .flex()
                    .flex_col()
                    .when_some(shell.preview.media.fullscreen.as_ref(), |d, fullscreen| {
                        d.track_focus(&fullscreen.controls_focus)
                    })
                    .on_hover(cx.listener(|this, hovered, window, cx| {
                        this.media_controls_hovered(*hovered, window, cx);
                    }))
                    .bg(theme.surface)
                    .child(toolbar(shell, &theme, cx)),
            )
        })
        .into_any_element()
}

pub(crate) fn time(seconds: f64) -> String {
    let total = seconds.max(0.) as u64;
    if total >= 3600 {
        format!("{}:{:02}:{:02}", total / 3600, total / 60 % 60, total % 60)
    } else {
        format!("{}:{:02}", total / 60, total % 60)
    }
}
fn control(
    id: impl Into<gpui::SharedString>,
    icon: &str,
    title: impl Into<gpui::SharedString>,
    action: &str,
    value: serde_json::Value,
) -> Button {
    Button::new(id, icon.to_owned(), title, "ui.media_control")
        .args(serde_json::json!({"action": action, "value":value}))
}

/// A phase of dragging a seek bar's knob, with the bar's fraction under the
/// pointer. Whoever owns the player decides what a phase does, and ignores
/// a move or a release it never saw the press of.
pub type Scrub = Rc<dyn Fn(&mut Shell, &str, f32, &mut Window, &mut Context<Shell>)>;

/// A seek bar drawn at `fraction`, reporting presses, moves and releases
/// through `scrub`. The same bar serves the panel's player and the dock's.
pub(crate) fn seek_bar_of(
    id: &'static str,
    fraction: f32,
    theme: &Theme,
    scrub: Scrub,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let bounds = Rc::new(Cell::new(Bounds::<Pixels>::default()));
    let measured = bounds.clone();
    let coordinate = |bounds: Bounds<Pixels>, x: Pixels| {
        f32::from(x - bounds.origin.x) / f32::from(bounds.size.width).max(1.)
    };
    let phase = |name: &'static str| {
        let (bounds, scrub) = (bounds.clone(), scrub.clone());
        cx.listener(move |this, position: &gpui::Point<Pixels>, window, cx| {
            scrub(this, name, coordinate(bounds.get(), position.x), window, cx);
        })
    };
    let (down, moved, up, up_out) = (phase("down"), phase("move"), phase("up"), phase("up"));
    div()
        .id(id)
        .relative()
        .flex_1()
        .min_w(px(40.))
        .h(px(30.))
        .cursor_pointer()
        .child(
            gpui::canvas(move |b, _, _| measured.set(b), |_, (), _, _| {})
                .absolute()
                .size_full(),
        )
        .child(
            div()
                .absolute()
                .top(px(13.))
                .h(px(4.))
                .w_full()
                .rounded_full()
                .bg(theme.border),
        )
        .child(
            div()
                .absolute()
                .top(px(13.))
                .h(px(4.))
                .w(gpui::relative(fraction))
                .rounded_full()
                .bg(theme.accent),
        )
        .on_mouse_down(
            MouseButton::Left,
            move |ev: &gpui::MouseDownEvent, window, cx| {
                down(&ev.position, window, cx);
                cx.stop_propagation();
            },
        )
        .on_mouse_move(move |ev: &gpui::MouseMoveEvent, window, cx| {
            moved(&ev.position, window, cx);
        })
        .on_mouse_up(
            MouseButton::Left,
            move |ev: &gpui::MouseUpEvent, window, cx| {
                up(&ev.position, window, cx);
            },
        )
        .on_mouse_up_out(
            MouseButton::Left,
            move |ev: &gpui::MouseUpEvent, window, cx| {
                up_out(&ev.position, window, cx);
            },
        )
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
    seek_bar_of("video-seek", fraction, theme, scrub, cx)
}

pub fn toolbar(shell: &Shell, theme: &Theme, cx: &mut Context<Shell>) -> AnyElement {
    use crate::preview_bar::{Bar, Item, Slot};
    let state = &shell.preview.media.state;
    let mut bar = Bar::new("video-tools");
    crate::preview_bar::parent_tool(&mut bar, shell);
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
                "Previous track · Shift+Left",
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
            "Play / pause · Space",
            "play",
            serde_json::Value::Null,
        ),
    ));
    if tracks {
        bar.push(Item::button(
            Slot::Navigate,
            1,
            step(
                "audio-next",
                "skip-next",
                "Next track · Shift+Right",
                1,
                next,
            ),
        ));
    }
    bar.push(Item::widget(Slot::Navigate, 1, |shell, _| {
        let media = &shell.preview.media;
        div()
            .text_xs()
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
    bar.push(Item::widget(Slot::Navigate, 0, move |shell, _| {
        div()
            .text_xs()
            .flex_none()
            .text_color(muted)
            .child(
                shell
                    .preview
                    .media
                    .state
                    .duration
                    .map_or_else(|| "—".into(), time),
            )
            .into_any_element()
    }));
    bar.push(Item::button(
        Slot::View,
        0,
        control(
            "video-mute",
            if state.muted { "muted" } else { "volume" },
            if state.muted {
                "Unmute · M"
            } else {
                "Mute · M"
            },
            "mute",
            serde_json::Value::Null,
        ),
    ));
    bar.push(Item::button(
        Slot::View,
        1,
        Button::new(
            "video-fullscreen",
            "fullscreen",
            "Fullscreen · F",
            "ui.media_fullscreen",
        )
        .active(shell.preview.media.fullscreen.is_some()),
    ));
    // Music and films alike keep playing in the dock.
    bar.push(Item::button(
        Slot::View,
        1,
        Button::new(
            "media-pin",
            "pin",
            "Keep playing while you browse",
            "ui.media_pin",
        )
        .active(shell.preview_is_pinned()),
    ));
    bar.settings = Some(std::rc::Rc::new(settings));
    crate::preview_bar::render(&bar, crate::preview::active_popup(shell), shell, theme, cx)
}

/// What playback has no button for: volume, speed and the tracks.
fn settings(shell: &Shell, theme: &Theme, cx: &mut Context<Shell>) -> Vec<AnyElement> {
    let state = &shell.preview.media.state;
    let heading = |title: &'static str| {
        div()
            .text_xs()
            .text_color(theme.text_muted)
            .child(title)
            .into_any_element()
    };
    let mut rows = vec![
        div()
            .text_xs()
            .child(format!(
                "{} / {}",
                time(state.position),
                state.duration.map_or_else(|| "—".into(), time)
            ))
            .into_any_element(),
        heading("Volume"),
    ];
    let mut row = div().flex().gap_1();
    for value in [0., 0.25, 0.5, 0.75, 1.] {
        let label = format!("{:.0}%", value * 100.);
        row = row.child(
            control(
                format!("volume-{label}"),
                "",
                label.clone(),
                "volume",
                value.into(),
            )
            .label(label)
            .active((state.volume - value).abs() < 0.01)
            .render(shell, cx),
        );
    }
    rows.push(row.into_any_element());
    rows.push(heading("Playback speed"));
    let mut row = div().flex().gap_1();
    for value in [0.5, 1., 1.25, 1.5, 2.] {
        let label = format!("{value}×");
        row = row.child(
            control(
                format!("rate-{label}"),
                "",
                label.clone(),
                "rate",
                value.into(),
            )
            .label(label)
            .active((state.rate - value).abs() < 0.01)
            .enabled(state.seekable)
            .render(shell, cx),
        );
    }
    rows.push(row.into_any_element());
    for (title, action, tracks, selected) in [
        ("Audio", "audio", &state.audio_tracks, state.audio),
        (
            "Subtitles",
            "subtitle",
            &state.subtitle_tracks,
            state.subtitle,
        ),
    ] {
        if tracks.is_empty() {
            continue;
        }
        rows.push(heading(title));
        if action == "subtitle" {
            rows.push(
                control("subtitles-off", "", "Off", action, (-1).into())
                    .label("Off")
                    .active(selected == -1)
                    .menu()
                    .render(shell, cx)
                    .into_any_element(),
            );
        }
        for (i, label) in tracks.iter().enumerate() {
            rows.push(
                control(format!("{action}-{i}"), "", label.clone(), action, i.into())
                    .label(label.clone())
                    .active(i as i32 == selected)
                    .menu()
                    .render(shell, cx)
                    .into_any_element(),
            );
        }
    }
    rows
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
                .and_then(crate::elements::halo::tint)
                .map(crate::elements::halo::vivid);
            cache.set(Some((cover.id, colour)));
            colour
        })
        .unwrap_or(theme.accent)
}

/// The audio player's figure: the halo round the cover, the title and the
/// artist and album under it. Drawn for the panel and, pinned, for the dock.
pub(crate) fn audio_figure(
    state: &Playback,
    name: &str,
    cover: Option<Arc<RenderImage>>,
    tint: gpui::Hsla,
    theme: &Theme,
) -> AnyElement {
    let line = |text: &str, muted: bool| {
        div()
            .flex_none()
            .max_w_full()
            .truncate()
            .when(muted, |d| d.text_color(theme.text_muted))
            .child(text.to_owned())
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
        .child(
            // A square as wide as the panel allows, which gives way in height
            // when the panel is short: the halo draws in the smaller of the two.
            div()
                .w_full()
                .max_w(px(420.))
                .min_h_0()
                .flex_shrink()
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
                }),
        )
        .child(div().flex_none().max_w_full().truncate().text_lg().child(
            if state.title.is_empty() {
                name.to_owned()
            } else {
                state.title.clone()
            },
        ))
        .child(line(&state.artist, true))
        .child(line(&state.album, true))
        .into_any_element()
}

/// Cover and metadata remain visible during playback; spectrum has no UI timer.
pub fn audio_content(shell: &Shell, theme: &Theme) -> AnyElement {
    let p = &shell.preview;
    let cover = match &p.content {
        Some(PreviewContent::Image { image, .. }) => Some(image.clone()),
        _ => None,
    };
    let tint = cover_tint(cover.as_ref(), &p.media.tint, theme);
    audio_figure(&p.media.state, &p.info.name, cover, tint, theme)
}

#[cfg(test)]
mod tests {
    use super::*;

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
