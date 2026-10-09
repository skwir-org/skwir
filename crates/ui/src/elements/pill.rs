//! A pill: a short state in a line, as the address shows them (a search's
//! facets, the filter typed, what narrows a folder). Its body is the shared
//! toolbar button (focus, keys, its tooltip and the key that does it), so
//! the keyboard reaches every pill; one that can be taken away ends with a
//! ×, the pointer's shortcut, which runs its command with a tooltip of its
//! own and takes no focus: it goes with what it removes, and a focused
//! element that goes takes the keyboard with it.

use bibli_core::command::Invocation;
use bibli_core::workspace::PaneId;
use gpui::{Context, Div, SharedString, Stateful, div, prelude::*, px};

use crate::shell::Shell;
use crate::theme::Theme;
use crate::toolbar::{Button, tooltip};

/// `button` drawn as a pill, ending with a × that runs `remove` (said by
/// `tip`) on `pane` when there is one.
pub fn pill(
    shell: &Shell,
    button: Button,
    remove: Option<(Invocation, SharedString)>,
    cx: &mut Context<Shell>,
) -> Stateful<Div> {
    let theme = cx.global::<Theme>().clone();
    let ink = if button.tinted() {
        theme.text_muted
    } else {
        theme.selection_text
    };
    let body = button
        .render(shell, cx)
        .h(px(22.))
        .max_w(px(200.))
        .rounded_full()
        .mr(px(2.));
    match remove {
        Some((remove, tip)) => body.pr(px(2.)).child(clear(shell, remove, tip, ink, cx)),
        None => body,
    }
}

/// The × that takes a pill away: `remove` run on the pane being drawn, as
/// the pointer's shortcut for what the keyboard does otherwise, in `ink`,
/// the pill's text color.
pub fn clear(
    shell: &Shell,
    remove: Invocation,
    tip: SharedString,
    ink: gpui::Hsla,
    cx: &mut Context<Shell>,
) -> Stateful<Div> {
    let theme = cx.global::<Theme>().clone();
    let pane: PaneId = shell.pane;
    let font = shell.font.clone();
    let hover = theme.hover;
    let id = SharedString::from(format!("pill-clear-{}-{tip}", pane.0));
    div()
        .id(id)
        .size(px(18.))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded_full()
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .tooltip(move |_, cx| tooltip(tip.clone(), font.clone(), cx))
        .on_click(cx.listener(move |this, _, window, cx| {
            cx.stop_propagation();
            this.activate_pane(pane, cx);
            this.note_pointer_use(&remove.name, &remove.args, cx);
            let remove = remove.clone().with("pane", pane);
            this.run(&remove.name, remove.args, window, cx);
        }))
        .child(crate::controls::glyph("close", ink, 10.))
}
