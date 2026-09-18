//! Address bar: the current location as editable text, plus Back and Up.

use gpui::{AnyElement, Context, Div, Stateful, Window, div, prelude::*, px};

use crate::keymap;
use crate::shell::Shell;
use crate::theme::Theme;

fn button(
    id: &'static str,
    label: &'static str,
    command: &'static str,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(28.))
        .w(px(30.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .cursor_pointer()
        .hover(|s| s.bg(theme.border))
        .on_click(cx.listener(move |this, _, window, cx| {
            this.run(command, serde_json::json!({}), window, cx);
        }))
        .child(label)
}

pub fn render(shell: &Shell, window: &Window, cx: &mut Context<Shell>) -> AnyElement {
    let theme = cx.global::<Theme>().clone();
    let focused = shell.address.read(cx).is_focused(window);
    div()
        .h(px(40.))
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .px_2()
        .border_b_1()
        .border_color(theme.border)
        .bg(theme.surface)
        .child(button("back", "←", "back", &theme, cx))
        .child(button("up", "↑", "up", &theme, cx))
        .child(
            div()
                .flex_1()
                .h(px(28.))
                .px_2()
                .flex()
                .items_center()
                .rounded_md()
                .border_1()
                .border_color(if focused { theme.accent } else { theme.border })
                .bg(theme.bg)
                .key_context(keymap::ADDRESS)
                .child(shell.address.clone()),
        )
        .when(shell.list.show_hidden, |d| {
            d.child(
                div()
                    .text_xs()
                    .text_color(theme.text_muted)
                    .child("hidden shown"),
            )
        })
        .into_any_element()
}
