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
//! Only the picture (the film's frame, `picture::OnPicture`, the cover of
//! the compact row, the cover's disc of the tall figure, `halo::OnDisc`)
//! answers clicks like the panel's player (`media::clickable`): one toggles
//! playback, two reveal a film in the panel and go fullscreen there. A click
//! anywhere else but the transport, the ring round the disc and the film's
//! letterbox included, shows the track in the panel, as it plays.
//!
//! The transport keeps what the panel's width has room for (`FORMS`,
//! measured by `AdaptiveRow`) and leaves the rest to its More, which also
//! holds the panel's player rows (what follows a track, volume, speed, a
//! film's tracks), so the pin can be steered there without being shown.
//! Fullscreen is the panel's: the film is shown there first.
//!
//! The height is a remembered preference, not a measured size, so the form
//! is chosen here in render, as the panel's width is. The dock steps aside
//! while the panel shows the pinned session itself, and gives way to the
//! last pane's footer's strip while the panel is closed.
use std::rc::Rc;
use std::sync::Arc;

use gpui::{AnyElement, Context, DragMoveEvent, Window, div, prelude::*, px};

use crate::elements::{
    adaptive::{AdaptiveRow, Chosen},
    popover::PopoverAnchor,
};
use crate::media::{Flash, Scrub};
use crate::shell::Shell;
use crate::theme::Theme;
use crate::toolbar::{self, Button};

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

    let handle = crate::elements::edge::edge(
        "dock-resize",
        crate::elements::edge::Side::Top,
        DockHandle,
        |this| this.player.height = COMPACT,
        cx,
    );

    // What is shown above the transport: the film, the figure, or the row.
    // The picture answers clicks as a player (`media::clickable`); the rest
    // of the dock, transport aside, shows the track.
    let picture: AnyElement = if pinned.video {
        // Only the frame plays and pauses, placed where it is drawn: shrunk
        // to fit, never grown. The letterbox round it shows the track, as
        // the ring round a cover's disc does.
        let frame = pinned.frame.as_ref().map(|(image, ..)| {
            let film = div()
                .relative()
                .size_full()
                .child(gpui::img(Arc::clone(image)).size_full())
                .children(crate::media::flash(Flash::Dock, shell, 1.));
            crate::elements::picture::OnPicture {
                child: crate::media::clickable("dock-film", film, Flash::Dock, cx)
                    // The letterbox under it lights up and explains itself
                    // only where a click shows the track.
                    .occlude()
                    .into_any_element(),
                image: image.size(0),
                fit: gpui::ObjectFit::ScaleDown,
            }
        });
        let letterbox = div()
            .relative()
            .flex_1()
            .min_h_0()
            .w_full()
            // The letterbox round a film is the panel's own surface.
            .bg(theme.surface)
            .children(frame);
        reveal(shell, "dock-letterbox", letterbox, cx).into_any_element()
    } else if tall {
        // Only the cover's disc plays and pauses, with its own tooltip and
        // highlight; the ring around it and the room beside it show the
        // track, as the rest of the figure does.
        let disc = Rc::new(std::cell::Cell::new(None));
        let hit = Rc::clone(&disc);
        let target = div()
            .id("dock-cover")
            .size_full()
            .rounded_full()
            .cursor_pointer()
            // Its edge, where the figure's click begins, drawn on hover.
            .border_2()
            .border_color(gpui::transparent_black())
            .hover(|d| d.border_color(theme.accent))
            .tooltip(play_tip(shell))
            // What is under the disc is not hovered while the pointer is on
            // it: the figure lights up only where a click shows the track.
            .occlude()
            .on_click(
                cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                    cx.stop_propagation();
                    let keyboard = matches!(event, gpui::ClickEvent::Keyboard(_));
                    if keyboard
                        || hit
                            .get()
                            .is_some_and(|d| crate::elements::halo::on_disc(event.position(), d))
                    {
                        this.media_clicked(Flash::Dock, event, window, cx);
                    } else {
                        // A corner of the disc's square is the figure's.
                        this.player_reveal(window, cx);
                    }
                }),
            );
        let square = crate::media::halo_square(state, pinned.cover.clone(), tint, &theme)
            .child(crate::elements::halo::OnDisc {
                child: target.into_any_element(),
                disc,
            })
            .children(crate::media::flash(Flash::Dock, shell, 1.));
        let figure = crate::media::audio_figure(
            state,
            title.clone(),
            square.into_any_element(),
            Some(REVEAL_TIP),
            &shell.font,
            &theme,
        );
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

    // The transport, in the richest form that fits, and its More.
    let open = shell
        .toolbar_menu
        .as_ref()
        .is_some_and(|menu| menu.kind == MENU);
    let anchor = PopoverAnchor::default();
    let chosen = Chosen::default();
    let transport = div().flex_none().child(AdaptiveRow {
        variants: FORMS
            .into_iter()
            .map(|kept| transport(shell, pinned, kept, &anchor, open, &theme, cx))
            .collect(),
        height: TRANSPORT,
        chosen: Some(chosen.clone()),
    });
    let menu = menu(shell, pinned, &anchor, &chosen, &theme, cx);

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
            .child(revealed(
                div().id("caption-title").min_w_0().truncate(),
                title.clone(),
                shell,
            ))
            .when(!by.is_empty(), |d| {
                d.child(revealed(
                    div()
                        .id("caption-by")
                        .min_w_0()
                        .truncate()
                        .text_color(theme.text_muted),
                    by.clone(),
                    shell,
                ))
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
                .on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
                    window.focus(&clicked, cx);
                });
    }
    Some(
        dock.child(div().h(px(6.)).flex_none())
            .child(picture)
            .children(caption)
            .child(transport)
            .children(menu)
            .child(handle)
            .into_any_element(),
    )
}

/// The dock's More (`ui.toolbar_menu`'s `kind`): what the panel's player
/// has that the transport has no button for.
pub const MENU: &str = "player";

/// What a form of the transport keeps as buttons; More has the rest.
#[derive(Clone, Copy)]
struct Kept {
    /// Previous, Next and the times either side of the seek bar.
    steps: bool,
    mute: bool,
    /// A film's: music has nothing to show on the whole screen.
    fullscreen: bool,
}

/// The transport's forms, richest first, each giving More one thing more
/// as the panel's bar gives them up: Mute, then Fullscreen, then the steps
/// and the times, so a panel at its narrowest keeps Play, the seek bar,
/// More and the close button.
const FORMS: [Kept; 4] = [
    Kept {
        steps: true,
        mute: true,
        fullscreen: true,
    },
    Kept {
        steps: true,
        mute: false,
        fullscreen: true,
    },
    Kept {
        steps: true,
        mute: false,
        fullscreen: false,
    },
    Kept {
        steps: false,
        mute: false,
        fullscreen: false,
    },
];

/// The transport in one of its forms: the same requests the footer's strip
/// and the desktop send.
fn transport(
    shell: &Shell,
    pinned: &crate::player::Pinned,
    kept: Kept,
    anchor: &PopoverAnchor,
    open: bool,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let state = &pinned.state;
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
    let play = crate::player::request(
        "dock-play",
        if state.playing() { "pause" } else { "play" },
        "Play / pause",
        "play_pause",
    );
    div()
        .id("dock-transport")
        .size_full()
        .px_1()
        .flex()
        .items_center()
        .gap_1()
        .map(|d| {
            if kept.steps {
                d.child(step("dock-previous", -1, shell).render(shell, cx))
                    .child(play.render(shell, cx))
                    .child(step("dock-next", 1, shell).render(shell, cx))
                    .child(time(crate::media::time(position), false))
            } else {
                d.child(play.render(shell, cx))
            }
        })
        .when(state.seekable && duration > 0., |d| {
            d.child(crate::media::seek_bar_of(
                "dock-seek",
                fraction,
                theme,
                scrub,
                cx,
            ))
        })
        .when(!(state.seekable && duration > 0.), |d| {
            d.child(div().flex_1())
        })
        .when(kept.steps, |d| {
            d.child(time(crate::media::duration(state), true))
        })
        .when(kept.mute, |d| {
            d.child(mute("dock-mute", state).render(shell, cx))
        })
        .when(kept.fullscreen && pinned.video, |d| {
            d.child(fullscreen("dock-fullscreen").render(shell, cx))
        })
        .child(
            Button::new("dock-more", "more", "More", "ui.toolbar_menu")
                .args(serde_json::json!({ "kind": MENU }))
                .anchor_if(anchor, true)
                .active(open)
                .render(shell, cx),
        )
        .child(
            crate::player::request("dock-close", "close", "Stop and let go", "stop")
                .render(shell, cx),
        )
        .into_any_element()
}

/// Previous track (`delta` -1) or Next, there while the pin has one.
fn step(id: &'static str, delta: i64, shell: &Shell) -> Button {
    let there = shell
        .player
        .pinned
        .as_ref()
        .is_some_and(|pinned| pinned.can_step(delta, shell.prefs.player.continue_with));
    if delta < 0 {
        crate::player::request(id, "skip-previous", "Previous track", "previous")
    } else {
        crate::player::request(id, "skip-next", "Next track", "next")
    }
    .enabled(there)
}

fn mute(id: &'static str, state: &skwir_core::media::Playback) -> Button {
    crate::player::request(
        id,
        if state.muted { "muted" } else { "volume" },
        if state.muted { "Unmute" } else { "Mute" },
        "mute",
    )
}

/// Full screen is the panel's: the film is shown there first
/// (`Shell::player_fullscreen`).
fn fullscreen(id: &'static str) -> Button {
    crate::player::request(id, "fullscreen", "Fullscreen", "fullscreen")
}

/// The dock's More, above its button (the dock is at the window's foot):
/// what the transport left out, what follows a track, and the volume, the
/// speed and a film's tracks, each a request to the pinned player as the
/// transport's buttons are. One panel per form of the transport, the one
/// drawn chosen in the same frame.
fn menu(
    shell: &Shell,
    pinned: &crate::player::Pinned,
    anchor: &PopoverAnchor,
    chosen: &Chosen,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> Option<AnyElement> {
    let popup = shell
        .toolbar_menu
        .as_ref()
        .filter(|menu| menu.kind == MENU)?;
    let state = &pinned.state;
    let send = |id: String, title: String, action: &str, value: serde_json::Value| {
        let mut request = serde_json::Map::new();
        request.insert(action.to_owned(), value);
        Button::new(id, "", title, "ui.player").args(serde_json::json!({ "request": request }))
    };
    let panels = FORMS
        .into_iter()
        .map(|kept| {
            let mut panel = toolbar::popup_panel(popup, "ui.toolbar_menu", theme, cx);
            let mut left = Vec::new();
            if !kept.steps {
                let position = pinned.scrub.unwrap_or(state.position);
                panel = panel.child(div().px_2().text_xs().child(format!(
                    "{} / {}",
                    crate::media::time(position),
                    crate::media::duration(state)
                )));
                left.push(step("dock-menu-previous", -1, shell));
                left.push(step("dock-menu-next", 1, shell));
            }
            if !kept.mute {
                left.push(mute("dock-menu-mute", state));
            }
            if !kept.fullscreen && pinned.video {
                left.push(fullscreen("dock-menu-fullscreen"));
            }
            if !left.is_empty() {
                for button in left {
                    let title = button.title();
                    panel = panel.child(button.label(title).menu().render(shell, cx));
                }
                panel = panel.child(crate::preview_bar::separator(theme));
            }
            panel
                .children(crate::media::continue_rows(shell, theme, cx))
                .child(crate::preview_bar::separator(theme))
                .children(crate::media::playback_rows(state, &send, shell, theme, cx))
                .into_any_element()
        })
        .collect();
    Some(anchor.popup_above_for(chosen, panels))
}

/// What a click on the dock's text does, said by its tooltip.
pub(crate) const REVEAL_TIP: &str = "Show this track";

/// A line of the dock's text inside `reveal`: cut, it says itself whole
/// before what a click does.
fn revealed(
    el: gpui::Stateful<gpui::Div>,
    text: String,
    shell: &Shell,
) -> gpui::Stateful<gpui::Div> {
    crate::elements::clip::clipped_with(el, text, Some(REVEAL_TIP.into()), shell.font.clone())
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
        .tooltip(move |_, cx| crate::toolbar::tooltip(REVEAL_TIP.into(), font.clone(), cx))
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
                    .map(|icon| crate::icons::element(icon, 48., theme.text_muted)),
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
        revealed(
            div()
                .id(if muted { "row-by" } else { "row-title" })
                .when(muted, |d| d.text_xs().text_color(theme.text_muted))
                .truncate(),
            text.to_owned(),
            shell,
        )
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
