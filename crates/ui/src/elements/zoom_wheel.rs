//! Ctrl and the wheel zoom what is under the pointer as the zoom keys do
//! where it has the keyboard: the pane's view, a picture or a page, the text
//! of an inspection. One overlay over what scrolls, taking the wheel in the
//! capture phase: GPUI's scrolling reads no modifier, so what is under it
//! would scroll too. A map has none: its wheel zooms around the pointer,
//! with Ctrl or without.
use gpui::{Context, ScrollWheelEvent, prelude::*};

use crate::shell::Shell;

/// What a step of Ctrl and the wheel sizes (`Shell::zoom_wheel`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zoomed {
    /// The pane's view (`ui.zoom`), which stays where it is.
    View,
    /// What the preview shows to be looked at: a picture or a page
    /// (`ui.preview_zoom`; a book laid out into pages, its text).
    Preview,
    /// An inspection's text (`ui.inspect` larger or smaller): the panel's,
    /// or the container's view kept above a member (`parked`).
    Text { parked: bool },
}

/// An overlay over the area that scrolls, its last child, so what is drawn
/// over it (a drawer, a popup) keeps the wheel.
pub fn overlay(zoomed: Zoomed, cx: &mut Context<Shell>) -> impl IntoElement {
    let shell = cx.weak_entity();
    gpui::canvas(
        |bounds, window, _| window.insert_hitbox(bounds, gpui::HitboxBehavior::Normal),
        move |_, hitbox, window, _| {
            window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, cx| {
                if !phase.capture()
                    || !event.modifiers.secondary()
                    || !hitbox.should_handle_scroll(window)
                {
                    return;
                }
                let delta = event.delta.pixel_delta(window.line_height()).y;
                let lines = matches!(event.delta, gpui::ScrollDelta::Lines(_));
                shell
                    .update(cx, |shell, cx| {
                        shell.zoom_wheel(f32::from(delta), lines, zoomed, window, cx);
                    })
                    .ok();
                cx.stop_propagation();
            });
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}
