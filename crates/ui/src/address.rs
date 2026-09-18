//! Toolbar (navigation buttons, view switcher, preview/split toggles, shelf
//! actions) and the per-pane address bar.

use gpui::{AnyElement, Context, Div, Stateful, Window, div, prelude::*, px};

use crate::keymap;
use crate::shell::Shell;
use crate::theme::Theme;
use crate::view;

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

fn toggle(
    id: &'static str,
    label: &'static str,
    active: bool,
    enabled: bool,
    theme: &Theme,
    cx: &mut Context<Shell>,
    on: impl Fn(&mut Shell, &mut Window, &mut Context<Shell>) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(24.))
        .px_2()
        .flex()
        .items_center()
        .text_xs()
        .rounded_md()
        .when(enabled, |d| {
            d.cursor_pointer().hover(|s| s.bg(theme.border))
        })
        .when(!enabled, |d| d.text_color(theme.text_muted).opacity(0.6))
        .when(active, |d| {
            d.bg(theme.selection_bg).text_color(theme.selection_text)
        })
        .on_click(cx.listener(move |this, _, window, cx| {
            if enabled {
                on(this, window, cx);
            }
        }))
        .child(label)
}

fn segment(
    id: &'static str,
    label: &'static str,
    view_id: &'static str,
    shell: &Shell,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> Stateful<Div> {
    let active = shell.ui().view_id == view_id && !shell.usage.active;
    div()
        .id(id)
        .h(px(24.))
        .px_2()
        .flex()
        .items_center()
        .text_xs()
        .cursor_pointer()
        .when(active, |d| {
            d.bg(theme.selection_bg).text_color(theme.selection_text)
        })
        .when(!active, |d| d.hover(|s| s.bg(theme.border)))
        .on_click(cx.listener(move |this, _, window, cx| {
            this.close_usage(cx);
            this.run("ui.view", serde_json::json!({ "id": view_id }), window, cx);
        }))
        .child(label)
}

/// The global toolbar row (acts on the active pane).
pub fn render_toolbar(shell: &Shell, cx: &mut Context<Shell>) -> AnyElement {
    let theme = cx.global::<Theme>().clone();
    let is_shelf = shell
        .app
        .workspace
        .read()
        .pane(shell.pane)
        .and_then(|p| p.location().map(|l| &*l.scheme == "shelf"))
        .unwrap_or(false);
    let split = shell.is_split();
    let mut bar = div()
        .h(px(36.))
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
                .flex()
                .rounded_md()
                .border_1()
                .border_color(theme.border)
                .overflow_hidden()
                .child(segment("seg-list", "List", view::LIST, shell, &theme, cx))
                .child(segment("seg-grid", "Grid", view::GRID, shell, &theme, cx))
                .child(segment(
                    "seg-cols",
                    "Columns",
                    view::COLUMNS,
                    shell,
                    &theme,
                    cx,
                )),
        )
        .child(toggle(
            "usage-toggle",
            "Usage",
            shell.usage.active,
            true,
            &theme,
            cx,
            |this, window, cx| {
                if this.usage.active {
                    this.close_usage(cx);
                } else {
                    this.open_usage(window, cx);
                }
            },
        ))
        .child(toggle(
            "preview-toggle",
            "Preview",
            shell.preview.open,
            true,
            &theme,
            cx,
            |this, _, cx| {
                this.toggle_preview(cx);
            },
        ))
        .child(toggle(
            "split-toggle",
            "Split",
            split,
            true,
            &theme,
            cx,
            |this, window, cx| {
                this.run("ui.split", serde_json::json!({}), window, cx);
            },
        ))
        .child(toggle(
            "timeline-toggle",
            "Timeline",
            shell.timeline.open,
            true,
            &theme,
            cx,
            |this, _, cx| {
                this.toggle_timeline(cx);
            },
        ));
    if is_shelf {
        bar = bar
            .child(div().w(px(12.)))
            .child(toggle(
                "shelf-move",
                "Move here",
                false,
                split,
                &theme,
                cx,
                |this, window, cx| {
                    this.run("ui.shelf_move_here", serde_json::json!({}), window, cx);
                },
            ))
            .child(toggle(
                "shelf-copy",
                "Copy here",
                false,
                split,
                &theme,
                cx,
                |this, window, cx| {
                    this.run("ui.shelf_copy_here", serde_json::json!({}), window, cx);
                },
            ))
            .child(
                div()
                    .text_xs()
                    .text_color(theme.text_muted)
                    .child(if split {
                        "→ other pane"
                    } else {
                        "(split to choose a destination)"
                    }),
            );
    }
    bar = bar.child(div().flex_1());
    if shell.sort_spec().show_hidden {
        bar = bar.child(
            div()
                .text_xs()
                .text_color(theme.text_muted)
                .child("hidden shown"),
        );
    }
    bar.into_any_element()
}

/// The address input of the pane being rendered.
pub fn render_address(shell: &Shell, window: &Window, cx: &mut Context<Shell>) -> AnyElement {
    let theme = cx.global::<Theme>().clone();
    let ui = shell.ui();
    let focused = ui.address.read(cx).is_focused(window);
    div()
        .h(px(34.))
        .w_full()
        .flex()
        .items_center()
        .px_2()
        .border_b_1()
        .border_color(theme.border)
        .bg(theme.surface)
        .child(
            div()
                .flex_1()
                .h(px(26.))
                .px_2()
                .flex()
                .items_center()
                .rounded_md()
                .border_1()
                .border_color(if focused { theme.accent } else { theme.border })
                .bg(theme.bg)
                .key_context(keymap::ADDRESS)
                .child(ui.address.clone()),
        )
        .into_any_element()
}
