//! Grouped application tools and pane-owned navigation/address controls.
use crate::{
    elements::adaptive::AdaptiveRow,
    keymap,
    shell::Shell,
    theme::Theme,
    toolbar::{self, Button},
    view,
};
use gpui::{AnyElement, Context, Window, div, prelude::*, px};

impl Shell {
    pub fn toggle_toolbar_menu(
        &mut self,
        args: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(menu) = self.toolbar_menu.take() {
            menu.restore(window);
        } else if args.get("close").and_then(serde_json::Value::as_bool) != Some(true) {
            self.toolbar_menu = Some(toolbar::Popup::new("app", window, cx));
        }
        cx.notify();
    }
}

fn tools(shell: &Shell) -> [Button; 2] {
    [
        Button::new(
            "usage-toggle",
            "usage",
            "Disk usage · Ctrl+Shift+U",
            if shell.usage.active {
                "ui.usage_close"
            } else {
                "ui.usage"
            },
        )
        .active(shell.usage.active),
        Button::new(
            "timeline-toggle",
            "calendar",
            "Timeline · Ctrl+Shift+L",
            "ui.timeline",
        )
        .active(shell.timeline.open),
    ]
}

fn app_row(shell: &Shell, mode: usize, cx: &mut Context<Shell>) -> AnyElement {
    let theme = cx.global::<Theme>().clone();
    let mut bar = toolbar::row(&theme).id(("app-tools", mode));
    let mut views = div()
        .flex()
        .flex_none()
        .gap(px(1.))
        .p(px(2.))
        .rounded_lg()
        .border_1()
        .border_color(theme.border)
        .bg(theme.bg);
    for (id, icon, title, label, view) in [
        ("seg-list", "list", "List view · Ctrl+1", "List", view::LIST),
        ("seg-grid", "grid", "Grid view · Ctrl+2", "Grid", view::GRID),
        (
            "seg-cols",
            "columns",
            "Columns view · Ctrl+3",
            "Columns",
            view::COLUMNS,
        ),
    ] {
        let button = Button::new(id, icon, title, "ui.view")
            .args(serde_json::json!({"id":view}))
            .active(shell.ui().view_id == view && !shell.usage.active);
        views = views.child(
            if mode == 0 {
                button.label(label)
            } else {
                button
            }
            .render(shell, cx),
        );
    }
    bar = bar.child(views).child(toolbar::separator(&theme));
    for (button, label) in [
        (
            Button::new(
                "preview-toggle",
                "preview",
                "Preview panel · F3",
                "ui.toggle_preview",
            )
            .active(shell.preview.open),
            "Preview",
        ),
        (
            Button::new(
                "split-toggle",
                "split",
                "Split panes · Ctrl+Shift+D",
                "ui.split",
            )
            .active(shell.is_split()),
            "Split",
        ),
    ] {
        bar = bar.child(
            if mode == 0 {
                button.label(label)
            } else {
                button
            }
            .render(shell, cx),
        );
    }
    if mode < 2 {
        bar = bar.child(toolbar::separator(&theme));
        for (button, label) in tools(shell).into_iter().zip(["Usage", "Timeline"]) {
            bar = bar.child(
                if mode == 0 {
                    button.label(label)
                } else {
                    button
                }
                .render(shell, cx),
            );
        }
    }
    bar = bar.child(div().flex_1().min_w(px(12.)));
    if mode == 0 && shell.sort_spec().show_hidden {
        bar = bar.child(
            Button::new(
                "hidden-indicator",
                "eye",
                "Hide hidden files · Ctrl+H",
                "ui.toggle_hidden",
            )
            .label("Hidden")
            .active(true)
            .render(shell, cx),
        );
    }
    bar = bar.child(
        Button::new(
            "file-search",
            "search",
            "Search files · Ctrl+F",
            "ui.search",
        )
        .render(shell, cx),
    );
    if mode == 2 {
        bar = bar.child(
            Button::new("tools-more", "more", "More tools", "ui.toolbar_menu")
                .active(shell.toolbar_menu.is_some())
                .render(shell, cx),
        );
    }
    bar.child(
        Button::new("settings", "settings", "Settings · Ctrl+,", "ui.settings").render(shell, cx),
    )
    .into_any_element()
}

pub fn render_toolbar(shell: &Shell, cx: &mut Context<Shell>) -> AnyElement {
    let theme = cx.global::<Theme>().clone();
    let mut root = div().relative().flex_none().child(AdaptiveRow {
        variants: (0..3).map(|mode| app_row(shell, mode, cx)).collect(),
        height: 42.,
    });
    if let Some(popup) = &shell.toolbar_menu {
        let mut panel = toolbar::popup_panel(popup, "ui.toolbar_menu", &theme, cx);
        for (button, label) in tools(shell).into_iter().zip(["Disk usage", "Timeline"]) {
            panel = panel.child(button.label(label).render(shell, cx));
        }
        panel = panel.child(
            Button::new(
                "menu-hidden",
                "eye",
                "Toggle hidden files · Ctrl+H",
                "ui.toggle_hidden",
            )
            .label("Show hidden files")
            .active(shell.sort_spec().show_hidden)
            .render(shell, cx),
        );
        root = root.child(toolbar::anchored_popup(panel));
    }
    root.into_any_element()
}

pub fn render_address(shell: &Shell, window: &Window, cx: &mut Context<Shell>) -> AnyElement {
    let theme = cx.global::<Theme>().clone();
    let ui = shell.ui();
    let focused = ui.address.read(cx).is_focused(window);
    let (back, up, is_shelf, active) = {
        let ws = shell.app.workspace.read();
        let pane = ws.pane(shell.pane);
        (
            pane.is_some_and(|p| p.history_pos > 0),
            pane.and_then(|p| p.location())
                .and_then(bibli_core::Location::path)
                .is_some_and(|p| p.parent().is_some()),
            pane.and_then(|p| p.location())
                .is_some_and(|l| &*l.scheme == "shelf"),
            ws.active_id() == shell.pane,
        )
    };
    let mut bar = div()
        .id(("pane-address", shell.pane.0 as usize))
        .tab_group()
        .h(px(42.))
        .w_full()
        .flex_none()
        .flex()
        .items_center()
        .gap(px(3.))
        .px(px(6.))
        .border_b_1()
        .border_color(theme.border)
        .bg(theme.surface)
        .child(
            Button::new("back", "back", "Back · Alt+Left", "back")
                .enabled(back)
                .render(shell, cx),
        )
        .child(
            Button::new("up", "up", "Parent folder · Alt+Up", "up")
                .enabled(up)
                .render(shell, cx),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .h(px(30.))
                .px_2()
                .flex()
                .items_center()
                .rounded_md()
                .border_1()
                .border_color(if focused {
                    theme.accent
                } else if active {
                    theme.border
                } else {
                    theme.surface
                })
                .bg(theme.bg)
                .key_context(keymap::ADDRESS)
                .child(ui.address.clone()),
        );
    if is_shelf {
        for (id, icon, title, command) in [
            (
                "shelf-move",
                "move",
                "Move shelf files to the other pane",
                "ui.shelf_move_here",
            ),
            (
                "shelf-copy",
                "copy",
                "Copy shelf files to the other pane",
                "ui.shelf_copy_here",
            ),
        ] {
            bar = bar.child(
                Button::new(id, icon, title, command)
                    .enabled(shell.is_split())
                    .render(shell, cx),
            );
        }
    }
    bar.into_any_element()
}
