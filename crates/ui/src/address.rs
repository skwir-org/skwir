//! Grouped application tools and pane-owned navigation/address controls.
use crate::{
    elements::adaptive::AdaptiveRow,
    keymap,
    shell::Shell,
    theme::Theme,
    toolbar::{self, Button},
};
use gpui::{AnyElement, Context, Window, div, prelude::*, px};
use std::path::PathBuf;

impl Shell {
    /// `ui.toolbar_menu { kind }`: open that menu, or close the open one.
    pub fn toggle_toolbar_menu(
        &mut self,
        args: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let kind = match args.get("kind").and_then(serde_json::Value::as_str) {
            Some("sort") => SORT,
            _ => MORE,
        };
        let close = args.get("close").and_then(serde_json::Value::as_bool) == Some(true);
        let open = self.toolbar_menu.take().map(|menu| {
            let kind = menu.kind;
            menu.restore(window);
            kind
        });
        if !close && open != Some(kind) {
            self.toolbar_menu = Some(toolbar::Popup::new(kind, window, cx));
        }
        cx.notify();
    }
}

const SORT: &str = "sort";
const MORE: &str = "more";

/// A labelled button in wide toolbars, an icon in narrower ones.
fn labelled(button: Button, label: &'static str, mode: usize) -> Button {
    if mode == 0 {
        button.label(label)
    } else {
        button
    }
}

fn view_button(shell: &Shell, spec: &bibli_core::registry::ViewSpec, prefix: &str) -> Button {
    let icon = if toolbar::asset(&format!("toolbar/{}.svg", spec.icon)).is_some() {
        spec.icon.clone()
    } else {
        "grid".into()
    };
    Button::new(
        format!("{prefix}-{}", spec.id),
        icon,
        format!(
            "{} view{}",
            spec.title,
            shell
                .view_shortcut(&spec.id)
                .map_or_else(String::new, |key| format!(" · {key}"))
        ),
        "ui.view",
    )
    .args(serde_json::json!({"id": spec.id}))
    .active(shell.ui().view_id == &*spec.id)
}

/// The application row. Left: how the folder is shown (views, sorting) and
/// what can be added to it. Right: finding things, the two panels, and the
/// rest behind More. Navigation belongs to each pane's own row.
///
/// Modes: 0 has labels, 1 icons only, 2 keeps the current view, Search and
/// More, whose menu then carries everything that was dropped.
fn app_row(
    shell: &Shell,
    mode: usize,
    anchors: &[crate::elements::popover::PopoverAnchor; 2],
    cx: &mut Context<Shell>,
) -> AnyElement {
    let theme = cx.global::<Theme>().clone();
    let open = shell.toolbar_menu.as_ref().map(|menu| menu.kind);
    // One id for every variant, so a focused button keeps its focus handle
    // (gpui element state keyed by the id path) across a responsive reflow.
    let mut bar = toolbar::row(&theme).id("app-tools");
    let mut views = div()
        .flex()
        .flex_none()
        .gap(px(1.))
        .p(px(2.))
        .rounded_lg()
        .border_1()
        .border_color(theme.border)
        .bg(theme.bg);
    for spec in shell
        .available_views()
        .into_iter()
        .filter(|s| mode != 2 || &*s.id == shell.ui().view_id)
    {
        views = views.child(view_button(shell, &spec, "view").render(shell, cx));
    }
    bar = bar.child(views);
    if mode < 2 {
        bar = bar
            .child(toolbar::separator(&theme))
            .child(
                labelled(
                    Button::new("sort-menu", "sort", "Sort and display", "ui.toolbar_menu")
                        .args(serde_json::json!({"kind": SORT}))
                        .anchor_if(&anchors[0], true)
                        .active(open == Some(SORT)),
                    "Sort",
                    mode,
                )
                .render(shell, cx),
            )
            .child(
                labelled(
                    Button::new(
                        "new-folder",
                        "folder-plus",
                        "New folder · Ctrl+Shift+N",
                        "ui.mkdir_prompt",
                    ),
                    "New folder",
                    mode,
                )
                .render(shell, cx),
            );
    }
    bar = bar.child(div().flex_1().min_w(px(12.)));
    bar = bar.child(
        Button::new(
            "file-search",
            "search",
            "Search files · Ctrl+F",
            "ui.search",
        )
        .render(shell, cx),
    );
    if mode < 2 {
        bar = bar
            .child(toolbar::separator(&theme))
            .child(
                labelled(
                    Button::new(
                        "preview-toggle",
                        "preview",
                        "Preview panel · F11",
                        "ui.toggle_preview",
                    )
                    .active(shell.preview.open),
                    "Preview",
                    mode,
                )
                .render(shell, cx),
            )
            .child(
                labelled(
                    Button::new("split-toggle", "split", "Split panes · F3", "ui.split")
                        .active(shell.is_split()),
                    "Split",
                    mode,
                )
                .render(shell, cx),
            );
    }
    bar.child(
        Button::new("tools-more", "more", "More", "ui.toolbar_menu")
            .args(serde_json::json!({"kind": MORE}))
            .anchor_if(&anchors[1], true)
            .active(open == Some(MORE))
            .render(shell, cx),
    )
    .into_any_element()
}

/// A menu row: the shared button, full width, with its state as a check.
fn menu_item(button: Button, label: impl Into<gpui::SharedString>) -> Button {
    button.label(label).menu()
}

fn sort_items(shell: &Shell) -> Vec<Button> {
    let spec = shell.sort_spec();
    let mut items: Vec<Button> = [
        ("name", "Name", bibli_core::SortKey::Name),
        ("size", "Size", bibli_core::SortKey::Size),
        ("modified", "Date modified", bibli_core::SortKey::Modified),
    ]
    .into_iter()
    .map(|(key, label, sort_key)| {
        let current = spec.key == sort_key;
        let arrow = match (current, spec.ascending) {
            (false, _) => "",
            (true, true) => "  ↑",
            (true, false) => "  ↓",
        };
        menu_item(
            Button::new(
                format!("sort-{key}"),
                if current { "check" } else { "" },
                "Choose again to reverse the order",
                "ui.sort",
            )
            .args(serde_json::json!({"key": key}))
            .active(current),
            format!("{label}{arrow}"),
        )
    })
    .collect();
    items.push(menu_item(
        Button::new(
            "menu-hidden",
            "eye",
            "Show hidden files · Ctrl+H",
            "ui.toggle_hidden",
        )
        .active(spec.show_hidden),
        "Hidden files",
    ));
    items.push(menu_item(
        Button::new(
            "menu-grid-fill",
            "crop",
            "Crop thumbnails in the grid to fill their box",
            "ui.grid_fill",
        )
        .active(shell.grid_fill),
        "Fill thumbnails",
    ));
    for (dir, icon, label, keys) in [
        ("in", "plus", "Larger", "Ctrl+= or Ctrl+wheel up"),
        ("out", "minus", "Smaller", "Ctrl+- or Ctrl+wheel down"),
        ("reset", "fit", "Default size", "Ctrl+0"),
    ] {
        items.push(menu_item(
            Button::new(
                format!("zoom-{dir}"),
                icon,
                format!("{label} · {keys}"),
                "ui.zoom",
            )
            .args(serde_json::json!({"dir": dir})),
            label,
        ));
    }
    items
}

/// The More menu. It holds what has no button in the chosen toolbar variant:
/// always Timeline, the palette and Settings; in the narrowest variant also
/// the views, panels and folder tools that variant dropped.
fn more_panel(
    shell: &Shell,
    popup: &toolbar::Popup,
    everything: bool,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let mut panel = toolbar::popup_panel(popup, "ui.toolbar_menu", theme, cx);
    let group = |panel: gpui::Stateful<gpui::Div>, title: &'static str| {
        panel.child(
            div()
                .px_2()
                .pt_2()
                .pb_1()
                .text_xs()
                .text_color(theme.text_muted)
                .child(title),
        )
    };
    if everything {
        panel = group(panel, "View");
        for spec in shell.available_views() {
            panel = panel.child(
                view_button(shell, &spec, "menu-view")
                    .label(spec.title.clone())
                    .menu()
                    .render(shell, cx),
            );
        }
        for (button, label) in [
            (
                Button::new(
                    "menu-preview",
                    "preview",
                    "Preview panel · F11",
                    "ui.toggle_preview",
                )
                .active(shell.preview.open),
                "Preview panel",
            ),
            (
                Button::new("menu-split", "split", "Split panes · F3", "ui.split")
                    .active(shell.is_split()),
                "Split panes",
            ),
        ] {
            panel = panel.child(menu_item(button, label).render(shell, cx));
        }
        panel = group(panel, "Folder");
        panel = panel.child(
            menu_item(
                Button::new(
                    "menu-new-folder",
                    "folder-plus",
                    "New folder · Ctrl+Shift+N",
                    "ui.mkdir_prompt",
                ),
                "New folder…",
            )
            .render(shell, cx),
        );
        for item in sort_items(shell).into_iter().take(4) {
            panel = panel.child(item.render(shell, cx));
        }
        panel = group(panel, "bibli");
    }
    for (button, label) in [
        (
            Button::new(
                "menu-timeline",
                "calendar",
                "Timeline · Ctrl+Shift+L",
                "ui.timeline",
            )
            .active(shell.timeline.open),
            "Timeline",
        ),
        (
            Button::new(
                "menu-palette",
                "command",
                "Command palette · Ctrl+Shift+P",
                "ui.palette",
            ),
            "Command palette…",
        ),
        (
            Button::new(
                "menu-shortcuts",
                "keyboard",
                "Shortcuts here · F1",
                "ui.shortcuts",
            ),
            "Shortcuts…",
        ),
        (
            Button::new(
                "menu-settings",
                "settings",
                "Settings · Ctrl+,",
                "ui.settings",
            ),
            "Settings…",
        ),
    ] {
        panel = panel.child(menu_item(button, label).render(shell, cx));
    }
    panel.into_any_element()
}

pub fn render_toolbar(shell: &Shell, cx: &mut Context<Shell>) -> AnyElement {
    let theme = cx.global::<Theme>().clone();
    let anchors = [
        crate::elements::popover::PopoverAnchor::default(),
        crate::elements::popover::PopoverAnchor::default(),
    ];
    let chosen = crate::elements::adaptive::Chosen::default();
    let mut root = div().relative().flex_none().child(AdaptiveRow {
        variants: (0..3)
            .map(|mode| app_row(shell, mode, &anchors, cx))
            .collect(),
        height: 42.,
        chosen: Some(chosen.clone()),
    });
    let Some(popup) = &shell.toolbar_menu else {
        return root.into_any_element();
    };
    if popup.kind == SORT {
        let mut panel = toolbar::popup_panel(popup, "ui.toolbar_menu", &theme, cx);
        for (index, item) in sort_items(shell).into_iter().enumerate() {
            let title = match index {
                0 => Some("Sort by"),
                3 => Some("Show"),
                5 => Some("Icon size"),
                _ => None,
            };
            if let Some(title) = title {
                panel = panel.child(
                    div()
                        .px_2()
                        .pt_2()
                        .pb_1()
                        .text_xs()
                        .text_color(theme.text_muted)
                        .child(title),
                );
            }
            panel = panel.child(item.render(shell, cx));
        }
        root = root.child(anchors[0].popup(panel));
    } else {
        // One panel per toolbar variant; the row's choice picks it, this frame.
        let panels = (0..3)
            .map(|mode| more_panel(shell, popup, mode == 2, &theme, cx))
            .collect();
        root = root.child(anchors[1].popup_for(&chosen, panels));
    }
    root.into_any_element()
}

pub fn render_address(shell: &Shell, window: &Window, cx: &mut Context<Shell>) -> AnyElement {
    let theme = cx.global::<Theme>().clone();
    let ui = shell.ui();
    let focused = ui.address.read(cx).is_focused(window);
    let (back, forward, path, up, is_shelf, active) = {
        let ws = shell.app.workspace.read();
        let pane = ws.pane(shell.pane);
        (
            pane.is_some_and(|p| p.history_pos > 0),
            pane.is_some_and(|p| p.history_pos + 1 < p.history.len()),
            pane.and_then(|p| p.location())
                .and_then(bibli_core::Location::path),
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
            Button::new("forward", "forward", "Forward · Alt+Right", "forward")
                .enabled(forward)
                .render(shell, cx),
        )
        .child(
            Button::new("up", "up", "Parent folder · Alt+Up", "up")
                .enabled(up)
                .render(shell, cx),
        )
        .child({
            let field = div()
                .id(("address-field", shell.pane.0 as usize))
                .flex_1()
                .min_w_0()
                .h(px(30.))
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
                .key_context(keymap::ADDRESS);
            match path.filter(|_| !focused) {
                // A folder reads as a trail of places; the text field takes
                // over while it is being edited (Ctrl+L, or a click beside
                // the trail) and for locations that are not folders.
                Some(path) => {
                    let pane = shell.pane;
                    field
                        .px(px(3.))
                        .cursor_text()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.activate_pane(pane, cx);
                            this.run("ui.focus_address", serde_json::json!({}), window, cx);
                        }))
                        .child(breadcrumbs(shell, &path, &theme, cx))
                }
                None => field.px_2().child(ui.address.clone()),
            }
        });
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

/// The folders leading to `path`, outermost first, each with the path it
/// opens. The home folder stands for everything above it.
pub fn crumbs(path: &std::path::Path, home: Option<&std::path::Path>) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let mut current = PathBuf::new();
    let home = home.filter(|home| path.starts_with(home) && *home != std::path::Path::new("/"));
    for component in path.components() {
        current.push(component);
        if let Some(home) = home
            && home.starts_with(&current)
        {
            if current == home {
                out.push(("Home".to_owned(), current.clone()));
            }
            continue;
        }
        let label = match component {
            std::path::Component::RootDir => "/".to_owned(),
            other => other.as_os_str().to_string_lossy().into_owned(),
        };
        out.push((label, current.clone()));
    }
    out
}

/// The trail, laid out by flexbox alone: the segments are given last first to
/// a right-to-left wrapping line one row tall, so the ones nearest the current
/// folder stay and the outermost wrap out of sight in a narrow pane, with
/// nothing measured. Every segment but the outermost carries its separator:
/// a trail that starts with one visibly continues to the left.
fn breadcrumbs(
    shell: &Shell,
    path: &std::path::Path,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let crumbs = crumbs(path, home.as_deref());
    let pane = shell.pane;
    let last = crumbs.len().saturating_sub(1);
    let mut trail = div()
        .h(px(24.))
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .flex()
        .flex_row_reverse()
        .flex_wrap()
        // `Start` is the left edge whatever the row's direction (taffy's
        // `Start`/`End` follow the writing mode, not the flex direction).
        .justify_start()
        .line_height(px(24.));
    for (index, (label, target)) in crumbs.into_iter().enumerate().rev() {
        let hover = theme.hover;
        let open = target.clone();
        let segment = div()
            .id(("crumb", index))
            .h(px(24.))
            .max_w(px(260.))
            .px(px(6.))
            .rounded_sm()
            .truncate()
            .cursor_pointer()
            .text_color(if index == last {
                theme.text
            } else {
                theme.text_muted
            })
            .hover(move |s| s.bg(hover))
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.activate_pane(pane, cx);
                this.run(
                    "navigate",
                    serde_json::json!({ "location": open, "pane": pane }),
                    window,
                    cx,
                );
            }))
            .child(label);
        let segment = crate::dnd::droppable_dir(segment, target, cx);
        trail = trail.child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .when(index > 0, |d| {
                    d.child(
                        gpui::svg()
                            .path("toolbar/chevron-right.svg")
                            .size(px(12.))
                            .flex_none()
                            .text_color(theme.text_muted),
                    )
                })
                .child(segment),
        );
    }
    trail.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::crumbs;
    use std::path::{Path, PathBuf};

    fn labels(path: &str, home: Option<&str>) -> Vec<String> {
        crumbs(Path::new(path), home.map(Path::new))
            .into_iter()
            .map(|(label, _)| label)
            .collect()
    }

    #[test]
    fn home_stands_for_everything_above_it() {
        assert_eq!(
            labels(
                "/home/example/Projects/it's a report",
                Some("/home/example")
            ),
            ["Home", "Projects", "it's a report"]
        );
        assert_eq!(labels("/home/example", Some("/home/example")), ["Home"]);
        assert_eq!(
            labels("/srv/data", Some("/home/example")),
            ["/", "srv", "data"]
        );
        assert_eq!(labels("/", None), ["/"]);
        // A sibling that merely shares a prefix is not inside the home folder.
        assert_eq!(
            labels("/home/example2", Some("/home/example")),
            ["/", "home", "example2"]
        );
    }

    #[test]
    fn every_segment_opens_its_own_folder() {
        let trail = crumbs(
            Path::new("/home/example/a/b"),
            Some(Path::new("/home/example")),
        );
        assert_eq!(trail[0].1, PathBuf::from("/home/example"));
        assert_eq!(trail[2].1, PathBuf::from("/home/example/a/b"));
    }
}
