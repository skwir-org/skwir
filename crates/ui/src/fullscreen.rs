//! The preview on the whole screen (`ui.preview_fullscreen`): a picture, a
//! document's pages, text, a film or music, drawn as the panel draws it, on
//! a canvas over the window. Above pages or text, the bar of what is shown
//! stays at the top, what is shown under it; over a picture, a film or
//! music it lies over the top and hides after a second of stillness, the
//! pointer moving bringing it back
//! (`arm_fullscreen_hide`, one cancellable timer, never a loop). C hides
//! and shows it for any.
//!
//! The canvas takes peek's place (`preview::render_peek`): while it shows,
//! the panel is only a picture of the preview, as under peek, and what the
//! preview shows changes with the pane's selection. A picture walks the
//! pane's pictures with the arrows (`picture_step`) and plays them as a
//! slideshow (`ui.slideshow`); the next one is read ahead
//! (`Shell::read_ahead`), so a step shows it at once.
use std::time::Duration;

use gpui::{AnyElement, Context, FocusHandle, Pixels, Point, Task, Window, div, prelude::*};

use crate::{
    keymap,
    preview::{PreviewContent, PreviewPhase},
    shell::Shell,
    theme::Theme,
};

pub struct Fullscreen {
    pub controls_hidden: bool,
    restore: Restore,
    controls_hovered: bool,
    controls_focus: FocusHandle,
    pointer: Point<Pixels>,
    hide_task: Option<Task<()>>,
    /// The slideshow, while it plays.
    pub slideshow: Option<Slideshow>,
    /// The way the last step went: the picture read ahead is the next one
    /// that way (`read_ahead_next`).
    toward: i64,
}

/// What leaving the whole screen gives back.
struct Restore {
    /// The window was not on the whole screen before: it goes back.
    entered: bool,
    /// Peek showed the preview before: it shows it again after.
    was_peek: bool,
    /// What had the keyboard before, which has it again after (F goes
    /// back and forth from the preview it was pressed in).
    before: Option<FocusHandle>,
}

/// A slideshow that plays: its wait before the next picture, armed once
/// the picture shown has arrived (`Shell::arm_slideshow`).
pub struct Slideshow {
    next: Option<Task<()>>,
}

/// What the canvas shows, from what the preview holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shows {
    Film,
    Music,
    Picture,
    Pages,
    Text,
}

/// Whether `entry` is a picture, as the desktop groups types
/// (`mime::Kind::Image`), by a type its name settles: what the arrows and
/// the slideshow walk. A name several types claim is left to the bytes,
/// which the walk does not read: a `.djvu` is a page or a book of them,
/// and a book stepped onto would keep the arrows to turn its pages.
pub(crate) fn is_picture(entry: &skwir_core::Entry) -> bool {
    !skwir_platform::mime::ambiguous(entry.type_name())
        && skwir_platform::mime::of_entry(entry).is_some_and(|mime| {
            skwir_platform::mime::is_kind(&mime, skwir_platform::mime::Kind::Image)
        })
}

/// Whether the type `mime` is a picture's, as `is_picture` tells.
pub(crate) fn is_picture_type(mime: &str) -> bool {
    skwir_platform::mime::is_kind(
        &skwir_platform::mime::Mime(mime.to_owned()),
        skwir_platform::mime::Kind::Image,
    )
}

impl Shell {
    /// What the whole screen would show of the preview: `None` for a
    /// folder, a file with no preview or one still on its way.
    pub(crate) fn fullscreen_shows(&self) -> Option<Shows> {
        let preview = &self.preview;
        if preview.entry_id().is_none() || preview.info.is_dir {
            return None;
        }
        if self.shows_player() {
            return Some(if self.media_is_audio() {
                Shows::Music
            } else {
                Shows::Film
            });
        }
        match &preview.content {
            Some(PreviewContent::Image {
                document: Some(_), ..
            }) => Some(Shows::Pages),
            Some(PreviewContent::Image { .. }) => Some(Shows::Picture),
            Some(PreviewContent::Inspection(_)) => Some(Shows::Text),
            _ => None,
        }
    }

    /// Whether the file shown is a picture by its type, so the arrows and
    /// the slideshow walk the pane's pictures (`preview.neighbours`).
    pub(crate) fn walks_pictures(&self) -> bool {
        self.preview.media.session.is_none() && self.preview.info.picture
    }

    /// The keyboard of what is shown: the player's, the picture's, the
    /// reader's or the inspection's own.
    fn shown_focus(&self) -> Option<FocusHandle> {
        match self.fullscreen_shows()? {
            Shows::Film | Shows::Music => self.preview.media.focus.clone(),
            Shows::Picture => self.preview.image_focus.clone(),
            Shows::Pages => self.preview.reader.focus.clone(),
            Shows::Text => self.preview.inspection.focus.clone(),
        }
    }

    /// `ui.preview_fullscreen`: show the preview on the whole screen, or
    /// leave it. The keys go to what is shown, so its own (zoom, pages,
    /// find, play) work at once.
    pub fn preview_fullscreen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.preview.fullscreen.is_some() {
            return self.exit_fullscreen(window, cx);
        }
        if self.fullscreen_shows().is_none() {
            return;
        }
        let entered = !window.is_fullscreen();
        if entered {
            window.toggle_fullscreen();
        }
        self.preview.fullscreen = Some(Fullscreen {
            controls_hidden: false,
            restore: Restore {
                entered,
                was_peek: self.preview.peek,
                before: window.focused(cx),
            },
            controls_hovered: false,
            controls_focus: cx.focus_handle(),
            pointer: window.mouse_position(),
            hide_task: None,
            slideshow: None,
            toward: 1,
        });
        self.preview.peek = true;
        // The canvas's own keyboard, where what is shown has none, and
        // where keys held in the preview go while it changes
        // (`cancel_preview`).
        let canvas = self
            .preview
            .peek_focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone();
        window.focus(&self.shown_focus().unwrap_or(canvas));
        self.preview.menu = None;
        self.arm_fullscreen_hide(window, cx);
        self.read_ahead_next(window, cx);
        cx.notify();
    }

    pub fn exit_fullscreen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Fullscreen { restore, .. }) = self.preview.fullscreen.take() else {
            return;
        };
        self.let_ahead_go(window, cx);
        if restore.entered && window.is_fullscreen() {
            window.toggle_fullscreen();
        }
        self.preview.peek = restore.was_peek;
        self.preview.menu = None;
        if !self.preview.peek {
            // Gone from the window by then (another file is shown), the
            // focus-lost listener gives it to the pane.
            window.focus(restore.before.as_ref().unwrap_or(&self.ui().focus));
        }
        cx.notify();
    }

    /// A preview that arrived while on the whole screen takes the keys
    /// the canvas held meanwhile, as entering gave them.
    pub(crate) fn fullscreen_arrived(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.preview.fullscreen.is_none() {
            return;
        }
        let canvas = self.preview.peek_focus.as_ref();
        if canvas.is_some_and(|canvas| canvas.is_focused(window))
            && let Some(focus) = self.shown_focus()
        {
            window.focus(&focus);
        }
        self.arm_slideshow(window, cx);
        self.read_ahead_next(window, cx);
    }

    /// Read ahead the picture the next step shows, the way the last went
    /// (the slideshow's is the next), once the one shown has arrived: the
    /// two never wait for each other's decode.
    fn read_ahead_next(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(fullscreen) = &self.preview.fullscreen else {
            return;
        };
        let (previous, next) = self.preview.neighbours;
        let ahead = if !self.walks_pictures() {
            None
        } else if fullscreen.toward < 0 {
            previous
        } else {
            next
        };
        match ahead {
            None => self.let_ahead_go(window, cx),
            Some(id) if self.preview.phase == PreviewPhase::Idle => {
                self.read_ahead(id, crate::preview::screen_side(window), window, cx);
            }
            Some(_) => {}
        }
    }

    /// `ui.fullscreen_controls`: show or hide the bar over the whole screen.
    pub fn fullscreen_controls(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(fullscreen) = &mut self.preview.fullscreen {
            fullscreen.controls_hidden = !fullscreen.controls_hidden;
            fullscreen.hide_task = None;
            self.preview.menu = None;
            if let Some(focus) = self.shown_focus() {
                window.focus(&focus);
            }
            cx.notify();
        }
    }

    /// Whether the whole screen's bar lies over what is shown and hides by
    /// itself: over what is looked at, a picture, a film or music, which
    /// keep the whole screen. Pages and text are read: their bar stays,
    /// above them. A file on its way counts as what its type shows, so a
    /// step or the next track never moves what is shown.
    pub(crate) fn fullscreen_bar_floats(&self) -> bool {
        match self.fullscreen_shows() {
            Some(shows) => matches!(shows, Shows::Picture | Shows::Film | Shows::Music),
            None => self.media_kind().is_some() || self.walks_pictures(),
        }
    }

    /// Hide a floating bar (`fullscreen_bar_floats`) after a second without
    /// the pointer moving, unless it is pointed at, holds the keyboard, has
    /// a popup open or a knob dragged.
    pub(crate) fn arm_fullscreen_hide(&mut self, window: &Window, cx: &mut Context<Self>) {
        if !self.fullscreen_bar_floats() {
            return;
        }
        let busy =
            crate::preview::active_popup(self, false).is_some() || self.preview.media.scrubbing();
        let Some(fullscreen) = &mut self.preview.fullscreen else {
            return;
        };
        if fullscreen.controls_hidden
            || fullscreen.controls_hovered
            || fullscreen.hide_task.is_some()
            || busy
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
                let busy = crate::preview::active_popup(this, false).is_some()
                    || this.preview.media.scrubbing();
                let Some(fullscreen) = &mut this.preview.fullscreen else {
                    return;
                };
                fullscreen.hide_task = None;
                tracing::trace!(
                    hovered = fullscreen.controls_hovered,
                    busy,
                    focused = focus.contains_focused(window, cx),
                    "fullscreen controls: inactivity deadline"
                );
                if !fullscreen.controls_hovered && !busy && !focus.contains_focused(window, cx) {
                    fullscreen.controls_hidden = true;
                    cx.notify();
                }
            });
        }));
    }

    fn fullscreen_pointer_moved(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let popup = crate::preview::active_popup(self, false).is_some();
        let shown = self.shown_focus();
        let Some(fullscreen) = &mut self.preview.fullscreen else {
            return;
        };
        // GPUI can redispatch a stationary pointer after layout/paint. Only
        // real movement reveals the bar or restarts its inactivity deadline.
        if fullscreen.pointer == position {
            return;
        }
        fullscreen.pointer = position;
        tracing::trace!(
            hovered = fullscreen.controls_hovered,
            popup,
            focused = fullscreen.controls_focus.contains_focused(window, cx),
            "fullscreen controls: pointer moved"
        );
        fullscreen.hide_task = None;
        if !fullscreen.controls_hovered
            && !popup
            && fullscreen.controls_focus.contains_focused(window, cx)
            && let Some(focus) = &shown
        {
            window.focus(focus);
        }
        if fullscreen.controls_hidden {
            fullscreen.controls_hidden = false;
            cx.notify();
        }
        self.arm_fullscreen_hide(window, cx);
    }

    fn fullscreen_controls_hovered(
        &mut self,
        hovered: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let popup = crate::preview::active_popup(self, false).is_some();
        let shown = self.shown_focus();
        if let Some(fullscreen) = &mut self.preview.fullscreen {
            fullscreen.controls_hovered = hovered;
            fullscreen.hide_task = None;
            if !hovered
                && !popup
                && fullscreen.controls_focus.contains_focused(window, cx)
                && let Some(focus) = &shown
            {
                window.focus(focus);
            }
        }
        self.arm_fullscreen_hide(window, cx);
    }

    /// Keep the screen awake while a slideshow or a film plays, the panel's
    /// or the pinned one (`Window::set_idle_inhibited`, vendored patch
    /// 0023): nothing but the time passing happens, and the screen saver
    /// would take it for an empty room. Music lets it sleep, and so does
    /// anything at rest. Told on every wake; the window does nothing when
    /// told what it already does.
    pub(crate) fn sync_awake(&self, window: &mut Window) {
        let slideshow = self
            .preview
            .fullscreen
            .as_ref()
            .is_some_and(|fullscreen| fullscreen.slideshow.is_some());
        let film = self.preview.media.session.is_some()
            && !self.media_is_audio()
            && self.preview.media.state.playing();
        let pinned = self
            .player
            .pinned
            .as_ref()
            .is_some_and(|pinned| pinned.video && pinned.state.playing());
        window.set_idle_inhibited(slideshow || film || pinned);
    }

    /// `ui.fullscreen_step { delta }`: on the whole screen, the arrows go
    /// to the picture before or after, or turn the page or the section.
    pub fn fullscreen_step(&mut self, delta: i64, window: &mut Window, cx: &mut Context<Self>) {
        if self.walks_pictures() {
            self.picture_step(delta, window, cx);
        } else {
            self.preview_page(delta.signum() as i32, window, cx);
        }
    }

    /// The picture before or after the one shown in the pane's order: the
    /// pane's selection moves there and the preview follows, as a track's
    /// step does. The slideshow's wait starts again once it has arrived.
    pub fn picture_step(&mut self, delta: i64, window: &mut Window, cx: &mut Context<Self>) {
        let (previous, next) = self.preview.neighbours;
        let Some(target) = (if delta < 0 { previous } else { next }) else {
            return;
        };
        let Some(pane) = self.preview.pane() else {
            return;
        };
        if let Some(fullscreen) = &mut self.preview.fullscreen {
            fullscreen.toward = delta.signum();
            if let Some(slideshow) = &mut fullscreen.slideshow {
                slideshow.next = None;
            }
        }
        self.with_pane(pane, |this| {
            if let Some(index) = this.ui().list.index_of(target) {
                this.cursor_to(index as isize, crate::list::Step::Select, window, cx);
            }
        });
    }

    /// `ui.slideshow`: show the pane's pictures one after another on the
    /// whole screen, from the one shown to the last, or pause.
    pub fn slideshow(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.walks_pictures() {
            return self.set_status("A slideshow shows pictures", false, cx);
        }
        if self.preview.fullscreen.is_none() {
            self.preview_fullscreen(window, cx);
        }
        let Some(fullscreen) = &mut self.preview.fullscreen else {
            return;
        };
        fullscreen.slideshow = match fullscreen.slideshow {
            Some(_) => None,
            None => Some(Slideshow { next: None }),
        };
        self.arm_slideshow(window, cx);
        cx.notify();
    }

    /// `ui.slideshow_pace { seconds }`: how long each picture is shown, the
    /// Reading page's setting (`reading.slideshow`). Applies from the next
    /// picture.
    pub fn slideshow_pace(&mut self, seconds: u64, window: &mut Window, cx: &mut Context<Self>) {
        crate::settings::remember(
            self,
            crate::settings::SLIDESHOW_KEY,
            seconds.to_string(),
            cx,
        );
        // The picker closes on its choice.
        if let Some(popup) = self
            .preview
            .menu
            .take_if(|popup| popup.kind == SLIDESHOW_POPUP)
        {
            popup.restore(window);
        }
        cx.notify();
    }

    /// The slideshow's wait before the next picture, once the one shown
    /// has arrived (or could not be read: it says why its time too), so a
    /// picture slow to decode is shown as long as the others.
    pub(crate) fn arm_slideshow(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ready = self.preview.phase == PreviewPhase::Idle && self.preview.content.is_some();
        let seconds = crate::settings::slideshow_seconds(self);
        let shown = (self.preview.pane(), self.preview.entry_id());
        let Some(slideshow) = self
            .preview
            .fullscreen
            .as_mut()
            .and_then(|fullscreen| fullscreen.slideshow.as_mut())
        else {
            return;
        };
        if slideshow.next.is_some() || !ready {
            return;
        }
        slideshow.next = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_secs(seconds))
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                // Something else moved the preview meanwhile: its own
                // arrival arms the wait again.
                if (this.preview.pane(), this.preview.entry_id()) != shown {
                    return;
                }
                this.slideshow_next(window, cx);
            });
        }));
    }

    /// The slideshow's next picture; after the last, it stops there.
    fn slideshow_next(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.preview.neighbours.1.is_some() {
            return self.picture_step(1, window, cx);
        }
        // It says so, and its bar shows its Play again.
        if let Some(fullscreen) = &mut self.preview.fullscreen {
            fullscreen.slideshow = None;
            fullscreen.controls_hidden = false;
        }
        self.set_status("The slideshow ended at the last picture", false, cx);
        self.arm_fullscreen_hide(window, cx);
        cx.notify();
    }
}

/// The popup of the slideshow's pace.
pub const SLIDESHOW_POPUP: &str = "slideshow";

/// The canvas over the window: what the preview shows, with its keys, and
/// its bar at the top (`Shell::fullscreen_bar_floats`): above pages or
/// text, which start under it; over a picture, a film or music, which keep
/// the whole screen, until it hides.
pub fn render(shell: &Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let fullscreen = shell.preview.fullscreen.as_ref()?;
    let theme = cx.global::<Theme>().clone();
    let shows = shell.fullscreen_shows();
    // What is read stays on paper; what is looked at is on the stage.
    let paper = crate::preview::on_paper(shell);
    let floats = shell.fullscreen_bar_floats();
    let inner = match shows {
        Some(Shows::Film) => crate::media::film(shell, cx),
        _ => None,
    }
    .unwrap_or_else(|| crate::preview::content(shell, &theme, true, cx));
    let content = crate::preview::content_area(shell, &theme, true, false, inner, cx);
    let keys = if shell.walks_pictures() {
        format!("{} {}", keymap::FULLSCREEN, keymap::SLIDESHOW)
    } else {
        keymap::FULLSCREEN.to_owned()
    };
    let mut bar = (!fullscreen.controls_hidden)
        .then(|| crate::preview::controls(shell, &theme, cx))
        .flatten()
        .map(|controls| {
            div()
                .id("preview-fullscreen-controls")
                .flex_none()
                .when(floats, |bar| bar.absolute().top_0().left_0().right_0())
                // What is under the bar answers clicks too (a film plays):
                // the bar takes the pointer.
                .occlude()
                .track_focus(&fullscreen.controls_focus)
                .on_hover(cx.listener(|this, hovered, window, cx| {
                    this.fullscreen_controls_hovered(*hovered, window, cx);
                }))
                .bg(theme.surface)
                .border_b_1()
                .border_color(theme.border)
                .child(controls)
        });
    let mut canvas = div()
        .id("preview-fullscreen")
        .absolute()
        .inset_0()
        .flex()
        .flex_col()
        .bg(if paper { theme.bg } else { theme.stage })
        .occlude()
        .key_context(keys.as_str())
        .on_mouse_move(
            cx.listener(|this, event: &gpui::MouseMoveEvent, window, cx| {
                this.fullscreen_pointer_moved(event.position, window, cx);
            }),
        )
        .children(bar.take_if(|_| !floats))
        .child(
            crate::preview::frame(shell, true)
                .flex_1()
                .min_h_0()
                .child(content),
        )
        .children(bar);
    if let Some(focus) = shell.preview.peek_focus.as_ref() {
        canvas = canvas.track_focus(focus);
    }
    // A floating bar hidden takes the pointer with it, over this window
    // only (a cursor style is the window's): on top of whatever asks for
    // another (a zoomed picture's hand), taking no click, wheel or drag
    // from what is under it. The pointer moving brings both back.
    if fullscreen.controls_hidden && floats {
        canvas = canvas.child(div().absolute().inset_0().cursor(gpui::CursorStyle::None));
    }
    Some(canvas.into_any_element())
}

// Types from the free desktops' database (step E, ADR-035).
#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::is_picture;
    use skwir_core::entry::{Entry, EntryId, EntryKind};

    fn file(name: &str) -> Entry {
        Entry::new(
            EntryId {
                file: skwir_core::FileId { dev: 1, ino: 2 },
                at: 0,
            },
            name,
            EntryKind::File,
        )
    }

    #[test]
    fn the_walk_takes_a_picture_only_where_its_name_says_so() {
        assert!(is_picture(&file("Synthetic sky.png")));
        assert!(!is_picture(&file("notes.txt")));
        // A page or a book of them: the bytes say which.
        assert!(!is_picture(&file("Synthetic scan.djvu")));
    }
}
