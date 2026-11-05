//! A pill's ×: a short state in a line, as the address shows them (a
//! search's facets, the filter typed, what narrows a folder), is the shared
//! toolbar button drawn round (`search_bar.rs`), so the keyboard reaches
//! every pill; one that can be taken away ends with this ×, the pointer's
//! shortcut, which runs its command with a tooltip of its own and takes no
//! focus: it goes with what it removes, and a focused element that goes
//! takes the keyboard with it.

use gpui::{Context, Div, SharedString, Stateful, div, prelude::*};
use skwir_core::command::Invocation;
use skwir_core::workspace::PaneId;

use crate::shell::Shell;
use crate::sizes::sized;
use crate::theme::Theme;
use crate::toolbar::tooltip;

/// The × that takes a pill away: `remove` run on the pane being drawn, as
/// the pointer's shortcut for what the keyboard does otherwise, in `ink`,
/// the pill's text color. It is as tall as the pill's text, which grows
/// with the text size (ADR-117); its glyph keeps its size.
pub fn clear(
    shell: &Shell,
    remove: Invocation,
    tip: SharedString,
    ink: gpui::Hsla,
    cx: &mut Context<Shell>,
) -> Stateful<Div> {
    let theme = cx.global::<Theme>().clone();
    let pane: PaneId = shell.pane;
    let hover = theme.hover;
    let id = SharedString::from(format!("pill-clear-{}-{tip}", pane.0));
    div()
        .id(id)
        .size(sized(18.))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded_full()
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .tooltip(move |_, cx| tooltip(tip.clone(), cx))
        .on_click(cx.listener(move |this, _, window, cx| {
            cx.stop_propagation();
            this.activate_pane(pane, cx);
            this.note_pointer_use(&remove.name, &remove.args, cx);
            let remove = remove.clone().with("pane", pane);
            this.run(&remove.name, remove.args, window, cx);
        }))
        .child(crate::controls::glyph("close", ink, 10.))
}
