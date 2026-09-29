//! The pinned player's dock: a strip at the foot of the preview panel that
//! holds what keeps playing while the selection moves (`player.rs`),
//! wherever the panel is. At the bottom of the window the panel is short,
//! so the dock is bounded by the panel's height rather than the window's
//! (`Shell::dock_height_limit`).
//!
//! It has two sizes, chosen by the height the user dragged it to, which is
//! remembered with the window's shape: compact, a row with the cover, the
//! title, a bar meter of the spectrum (beside the text when the text has
//! room, under it otherwise: `Form`) and the transport; tall, the same
//! figure the panel shows (the halo round the cover) with the transport
//! under it. A pinned film shows its frames in the place of the cover, at
//! whatever height was dragged.
//!
//! Only the picture (the film, the cover, the file's icon without one)
//! answers clicks like the panel's player (`media::clickable`): one toggles
//! playback, two reveal a film in the panel and go fullscreen there. A
//! click anywhere else but the transport shows the track in the panel, as
//! it plays.
//!
//! The height is a remembered preference, not a measured size, so the form
//! is chosen here in render, as the panel's width is. The dock steps aside
//! while the panel shows the pinned session itself, and gives way to the
//! last pane's footer's strip while the panel is closed.
use std::rc::Rc;
use std::sync::Arc;

use gpui::{AnyElement, Context, CursorStyle, DragMoveEvent, Window, div, prelude::*, px};

use crate::media::{Flash, Scrub};
use crate::shell::Shell;
use crate::theme::Theme;
use crate::toolbar::Button;

/// The dock's height until it is dragged: one row and the transport.
pub const COMPACT: f32 = 108.;
/// From this height up the dock shows the figure instead of the row.
pub const TALL_FROM: f32 = 200.;
/// What a film is given when pinned into a compact dock.
pub const VIDEO_HEIGHT: f32 = 240.;
/// The transport row.
const TRANSPORT: f32 = 36.;
/// The compact row's cover.
const COVER: f32 = 64.;
/// The width the compact row's text gets beside the meter when it does
/// not fit whole: enough for a title to be read.
const TEXT_LEAST: f32 = 200.;
/// The least the meter beside the text gets; it takes what the panel has
/// beyond it.
const METER_W: f32 = 80.;
/// What the panel keeps above the dock at the least: its header, its toolbar
/// and something of its content.
const PANEL_KEEP: f32 = 220.;

/// Marker type for dragging the dock's top edge.
#[derive(Clone)]
pub struct DockHandle;

impl Shell {
    /// The tallest the dock may be: the panel it stands in keeps the rest.
    /// At the right edge the panel is as tall as the window, to its foot;
    /// at the bottom, as tall as it is drawn.
    pub fn dock_height_limit(&self, window: &Window) -> f32 {
        let panel = if self.layout.bottom() {
            self.preview_panel_height()
                .min(self.preview_height_limit(window))
        } else {
            f32::from(window.viewport_size().height)
        };
        (panel - PANEL_KEEP).max(COMPACT)
    }

    /// The dock's top edge follows the pointer; its bottom is the panel's.
    pub(crate) fn on_dock_drag(
        &mut self,
        ev: &DragMoveEvent<DockHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The panel reaches the window's foot, at the right or the bottom.
        let bottom = f32::from(window.viewport_size().height);
        let y = f32::from(ev.event.position.y);
        self.player.height = (bottom - y).clamp(COMPACT, self.dock_height_limit(window));
        cx.notify();
    }

    /// Whether the dock is drawn: a pin, the panel open, and the panel
    /// showing something other than the pinned session itself, nor on its
    /// way to it (the pin went on to the next track and the selection
    /// follows) or to a track about to take its place (a step, Play on
    /// selection): the dock would only flash in between.
    pub fn dock_shown(&self) -> bool {
        let Some(pinned) = &self.player.pinned else {
            return false;
        };
        self.preview.open
            && !self.preview_is_pinned()
            && self.preview.autoplay.is_none()
            && self.preview.playing_path().as_ref() != Some(&pinned.path)
    }
}

/// A bar meter of the spectrum: one bar per band, the cover's colour.
/// It fills the box it is given: the bars are shares of its height.
fn meter(spectrum: &[f32], playing: bool, tint: gpui::Hsla) -> AnyElement {
    let mut row = div().flex().items_end().gap(px(1.)).size_full();
    for level in spectrum {
        let level = if playing {
            crate::elements::halo::level(*level)
        } else {
            0.
        };
        row = row.child(
            div()
                .flex_1()
                .h(gpui::relative(0.06 + level * 0.94))
                .rounded(px(1.))
                .bg(tint.opacity(0.35 + 0.65 * level)),
        );
    }
    row.into_any_element()
}

/// The dock, or `None` when there is nothing to show in it.
pub fn render(shell: &Shell, limit: f32, cx: &mut Context<Shell>) -> Option<AnyElement> {
    if !shell.dock_shown() {
        return None;
    }
    let pinned = shell.player.pinned.as_ref()?;
    let theme = cx.global::<Theme>().clone();
    let height = shell.player.height.min(limit);
    let tall = height >= TALL_FROM;
    let state = &pinned.state;
    let title = crate::media::track_title(state, &pinned.path);
    let by = [state.artist.as_str(), state.album.as_str()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
    let tint = crate::media::cover_tint(pinned.cover.as_ref(), &shell.player.tint, &theme);

    let handle = div()
        .id("dock-resize")
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .h(px(6.))
        .cursor(CursorStyle::ResizeUpDown)
        .hover(|style| style.bg(theme.handle))
        .on_drag(DockHandle, |_, _, _, cx| {
            cx.new(|_| crate::preview::NoDragView)
        })
        .on_click(cx.listener(|this, event: &gpui::ClickEvent, _, cx| {
            if event.click_count() == 2 {
                this.player.height = COMPACT;
                this.remember_session();
                cx.notify();
            }
        }));

    // What is shown above the transport: the film, the figure, or the row.
    // The picture answers clicks as a player (`media::clickable`); the rest
    // of the dock, transport aside, shows the track.
    let picture: AnyElement = if pinned.video {
        let frame = pinned.frame.as_ref().map(|(image, ..)| {
            gpui::img(Arc::clone(image))
                .max_w_full()
                .max_h_full()
                .object_fit(gpui::ObjectFit::Contain)
        });
        let film = div()
            .flex_1()
            .min_h_0()
            .w_full()
            .flex()
            .items_center()
            .justify_center()
            // The letterbox round a film is the panel's own surface.
            .bg(theme.surface)
            .children(frame)
            .into_any_element();
        crate::media::surface("dock-film", film, Flash::Dock, shell, cx)
    } else if tall {
        let square = crate::media::halo_square(state, pinned.cover.clone(), tint, &theme)
            .children(crate::media::flash(Flash::Dock, shell, 1.));
        let square =
            crate::media::clickable("dock-cover", square, Flash::Dock, cx).tooltip(play_tip(shell));
        let figure =
            crate::media::audio_figure(state, title.clone(), square.into_any_element(), &theme);
        reveal(shell, "dock-figure", figure, cx).into_any_element()
    } else {
        let row = crate::elements::adaptive::AdaptiveRow {
            variants: [Form::Whole, Form::Least, Form::Stacked]
                .into_iter()
                .map(|form| compact_row(shell, pinned, &title, &by, tint, &theme, form, cx))
                .collect(),
            height: height - 6. - TRANSPORT,
            chosen: None,
        };
        reveal(
            shell,
            "dock-row",
            div().flex_1().min_h_0().flex().child(row),
            cx,
        )
        .into_any_element()
    };

    // The transport: the same requests the footer's strip and the desktop send.
    let request = crate::player::request;
    let duration = state.duration.unwrap_or(0.);
    let position = pinned.scrub.unwrap_or(state.position);
    let fraction = if duration > 0. {
        (position / duration).clamp(0., 1.) as f32
    } else {
        0.
    };
    let scrub: Scrub = Rc::new(|this, phase, fraction, _, cx| {
        this.player_scrub(phase, fraction, cx);
    });
    let time = |text: String, muted: bool| {
        div()
            .flex_none()
            .text_xs()
            .when(muted, |d| d.text_color(theme.text_muted))
            .child(text)
    };
    let transport = div()
        .flex_none()
        .h(px(TRANSPORT))
        .px_1()
        .flex()
        .items_center()
        .gap_1()
        .child(
            request(
                "dock-previous",
                "skip-previous",
                "Previous track",
                "previous",
            )
            .enabled(pinned.can_step(-1, shell.prefs.player.continue_with))
            .render(shell, cx),
        )
        .child(
            request(
                "dock-play",
                if state.playing() { "pause" } else { "play" },
                "Play / pause",
                "play_pause",
            )
            .render(shell, cx),
        )
        .child(
            request("dock-next", "skip-next", "Next track", "next")
                .enabled(pinned.can_step(1, shell.prefs.player.continue_with))
                .render(shell, cx),
        )
        .child(time(crate::media::time(position), false))
        .when(state.seekable && duration > 0., |d| {
            d.child(crate::media::seek_bar_of(
                "dock-seek",
                fraction,
                &theme,
                scrub,
                cx,
            ))
        })
        .when(!(state.seekable && duration > 0.), |d| {
            d.child(div().flex_1())
        })
        .child(time(crate::media::duration(state), true))
        .child(
            Button::new(
                "dock-mute",
                if state.muted { "muted" } else { "volume" },
                if state.muted { "Unmute" } else { "Mute" },
                "ui.player_mute",
            )
            .render(shell, cx),
        )
        .child(request("dock-close", "close", "Stop and let go", "stop").render(shell, cx));

    // A film's title goes under its picture; the figure carries its own.
    let caption = pinned.video.then(|| {
        let line = div()
            .flex_none()
            .px_3()
            .py_1()
            .flex()
            .items_center()
            .gap_2()
            .text_xs()
            .child(div().min_w_0().truncate().child(title.clone()))
            .when(!by.is_empty(), |d| {
                d.child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_color(theme.text_muted)
                        .child(by.clone()),
                )
            });
        reveal(shell, "dock-caption", line, cx)
    });
    let mut dock = div()
        .id("player-dock")
        .relative()
        .flex_none()
        .h(px(height))
        .flex()
        .flex_col()
        .border_t_1()
        .border_color(theme.border)
        .bg(theme.surface)
        .key_context(crate::keymap::PLAYER);
    if let Some(focus) = &shell.player.focus {
        let clicked = focus.clone();
        dock =
            dock.track_focus(focus)
                .on_mouse_down(gpui::MouseButton::Left, move |_, window, _| {
                    window.focus(&clicked);
                });
    }
    Some(
        dock.child(div().h(px(6.)).flex_none())
            .child(picture)
            .children(caption)
            .child(transport)
            .child(handle)
            .into_any_element(),
    )
}

/// Something of the dock that shows the track when clicked: what is not
/// the picture and not the transport.
fn reveal(
    shell: &Shell,
    id: &'static str,
    inner: gpui::Div,
    cx: &mut Context<Shell>,
) -> gpui::Stateful<gpui::Div> {
    let font = shell.font.clone();
    let theme = cx.global::<Theme>().clone();
    inner
        .id(id)
        .cursor_pointer()
        .hover(move |d| d.bg(theme.hover))
        .tooltip(move |_, cx| crate::toolbar::tooltip("Show this track".into(), font.clone(), cx))
        .on_click(cx.listener(|this, _, window, cx| this.player_reveal(window, cx)))
}

/// The forms of the compact row, richest first: the meter beside the text
/// with the text whole, beside it with the text at least `TEXT_LEAST` wide,
/// and under the text. `AdaptiveRow` measures them and takes the first
/// that fits, so a short title gets the meter beside it in a narrow panel,
/// a long one in a wide panel, and neither is cut short for the meter.
#[derive(Clone, Copy)]
enum Form {
    Whole,
    Least,
    Stacked,
}

/// What the cover says: it is the one place a click plays or pauses.
fn play_tip(shell: &Shell) -> impl Fn(&mut Window, &mut gpui::App) -> gpui::AnyView + 'static {
    let font = shell.font.clone();
    move |_, cx| crate::toolbar::tooltip("Play / pause · Space".into(), font.clone(), cx)
}

/// The compact dock's row in one of its forms: the cover, the title and
/// who made it, and the meter.
#[allow(clippy::too_many_arguments)] // the row's parts, all of one render
fn compact_row(
    shell: &Shell,
    pinned: &crate::player::Pinned,
    title: &str,
    by: &str,
    tint: gpui::Hsla,
    theme: &Theme,
    form: Form,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let state = &pinned.state;
    let cover: AnyElement = match &pinned.cover {
        Some(cover) => gpui::img(Arc::clone(cover))
            .size(px(COVER))
            .object_fit(gpui::ObjectFit::Cover)
            .into_any_element(),
        None => div()
            .size(px(COVER))
            .flex()
            .items_center()
            .justify_center()
            .bg(theme.code_bg)
            .border_1()
            .border_color(theme.border)
            .children(
                pinned
                    .icon
                    .as_ref()
                    .and_then(|(_, icon)| icon.as_ref())
                    .map(|icon| crate::icons::element(icon, 48.)),
            )
            .into_any_element(),
    };
    let cover = crate::media::clickable(
        "dock-cover",
        div()
            .relative()
            .flex_none()
            .size(px(COVER))
            .overflow_hidden()
            .rounded_md()
            .child(cover)
            .children(crate::media::flash(Flash::Dock, shell, 0.6)),
        Flash::Dock,
        cx,
    )
    .tooltip(play_tip(shell));
    let line = |text: &str, muted: bool| {
        div()
            .when(muted, |d| d.text_xs().text_color(theme.text_muted))
            .truncate()
            .child(text.to_owned())
    };
    let text = div()
        .min_w_0()
        .overflow_hidden()
        .flex()
        .flex_col()
        .gap(px(2.))
        .child(line(title, false))
        .child(line(by, true));
    let meter = meter(&state.spectrum, state.playing(), tint);
    // The root of a variant: as wide as the row it is chosen for.
    let row = div()
        .size_full()
        .flex()
        .items_center()
        .gap_3()
        .px_3()
        .child(cover);
    // Measured, an overflow box contributes its flex basis: the text column
    // is as wide as its lines without one, or as wide as `TEXT_LEAST`, and
    // the meter `METER_W`. Laid out, the meter takes what the panel has
    // beyond that (with the text, when the text is not whole), as tall as
    // the two lines beside it.
    match form {
        Form::Whole | Form::Least => row
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .gap_3()
                    .child(match form {
                        Form::Least => text.flex_1().flex_basis(px(TEXT_LEAST)),
                        _ => text.flex_none(),
                    })
                    .child(
                        div()
                            .flex_1()
                            .flex_basis(px(METER_W))
                            .min_w(px(METER_W))
                            .overflow_hidden()
                            .child(meter),
                    ),
            )
            .into_any_element(),
        Form::Stacked => row
            .child(text.flex_1().child(div().h(px(14.)).child(meter)))
            .into_any_element(),
    }
}
