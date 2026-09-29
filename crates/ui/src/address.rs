//! Grouped application tools and pane-owned navigation/address controls.
use crate::{
    elements::adaptive::AdaptiveRow,
    keymap,
    shell::Shell,
    theme::Theme,
    toolbar::{self, Button},
};
use bibli_core::os_path::PathArg;
use bibli_core::registry::{ViewRole, ViewSpec};
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
            Some("view_options") => VIEW_OPTIONS,
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

const VIEW_OPTIONS: &str = "view_options";
const MORE: &str = "more";

/// A labelled button in wide toolbars, an icon in narrower ones.
fn labelled(button: Button, label: &'static str, mode: usize) -> Button {
    if mode == 0 {
        button.label(label)
    } else {
        button
    }
}

fn view_button(shell: &Shell, spec: &ViewSpec, prefix: &str) -> Button {
    let icon = if toolbar::asset(&format!("toolbar/{}.svg", spec.icon)).is_some() {
        spec.icon.clone()
    } else {
        "grid".into()
    };
    Button::new(
        format!("{prefix}-{}", spec.id),
        icon,
        format!("{} view", spec.title),
        "ui.view",
    )
    .args(serde_json::json!({"id": spec.id}))
    .active(shell.ui().view_id == &*spec.id)
}

/// The toolbar's view buttons: the views that show the listing, and an
/// alternate shape or a lens only while it is shown; the View menu offers
/// them all. The narrowest
/// variant keeps the current view alone.
fn toolbar_views(views: Vec<ViewSpec>, current: &str, narrowest: bool) -> Vec<ViewSpec> {
    views
        .into_iter()
        .filter(|s| &*s.id == current || (!narrowest && s.role == ViewRole::Listing))
        .collect()
}

/// The application row. Left: how the folder is shown (views, their options) and
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
    let (split_icon, split_tip) = shell.layout.split_button();
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
    for spec in toolbar_views(shell.available_views(), shell.ui().view_id, mode == 2) {
        views = views.child(view_button(shell, &spec, "view").render(shell, cx));
    }
    bar = bar.child(views);
    if mode < 2 {
        // No separator: the views' group has its own border, and the View
        // menu is their options, not another subject.
        bar = bar
            .child(
                labelled(
                    Button::new(
                        "view-options",
                        "sliders",
                        "Views, sort, hidden files and zoom",
                        "ui.toolbar_menu",
                    )
                    .args(serde_json::json!({"kind": VIEW_OPTIONS}))
                    .anchor_if(&anchors[0], true)
                    .active(open == Some(VIEW_OPTIONS)),
                    "View",
                    mode,
                )
                .render(shell, cx)
                .ml(px(4.)),
            )
            .child(
                labelled(
                    Button::new("new-folder", "folder-plus", "New folder", "ui.mkdir_prompt"),
                    "New folder",
                    mode,
                )
                .render(shell, cx),
            );
    }
    bar = bar.child(div().flex_1().min_w(px(12.)));
    bar = bar
        .child(Button::new("file-search", "search", "Search files", "ui.search").render(shell, cx));
    if mode < 2 {
        bar = bar
            .child(toolbar::separator(&theme))
            // In the order of what they open: the split is in the middle of
            // the window, the preview panel at its right edge.
            .child(
                labelled(
                    Button::new("split-toggle", split_icon, split_tip, "ui.split")
                        .active(shell.is_split()),
                    "Split",
                    mode,
                )
                .render(shell, cx),
            )
            .child(
                labelled(
                    Button::new(
                        "preview-toggle",
                        shell.layout.preview_icon(),
                        "Preview panel",
                        "ui.toggle_preview",
                    )
                    .active(shell.preview.open),
                    "Preview",
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

/// The View menu, in its titled groups: how the active pane shows
/// its folder, never how the window is arranged (that is in More, under
/// Layout). It starts with every view, the lenses the toolbar leaves out
/// included. The narrowest toolbar, which has no View button, puts
/// all of it in More.
fn view_options(shell: &Shell) -> Vec<(&'static str, Vec<Button>)> {
    use bibli_core::SortKey;
    let views = shell
        .available_views()
        .into_iter()
        .map(|spec| {
            view_button(shell, &spec, "menu-view")
                .label(spec.title.clone())
                .menu()
        })
        .collect();
    let spec = shell.sort_spec();
    let order: Vec<Button> = [
        ("name", "Name", SortKey::Name),
        ("size", "Size", SortKey::Size),
        ("modified", "Date modified", SortKey::Modified),
    ]
    .into_iter()
    .map(|(key, label, sort_key)| {
        let current = spec.key == sort_key;
        // The current key again keeps its direction: the next group says it.
        let args = if current {
            serde_json::json!({"key": key, "ascending": spec.ascending})
        } else {
            serde_json::json!({"key": key})
        };
        menu_item(
            Button::new(
                format!("sort-{key}"),
                if current { "check" } else { "" },
                format!("Sort by {}", label.to_lowercase()),
                "ui.sort",
            )
            .args(args)
            .active(current),
            label,
        )
    })
    .collect();
    // Each key names its directions in its own words.
    let (up, down) = match spec.key {
        SortKey::Name => ("A to Z", "Z to A"),
        SortKey::Size => ("Smallest first", "Largest first"),
        SortKey::Modified => ("Oldest first", "Newest first"),
    };
    let direction: Vec<Button> = [(true, up), (false, down)]
        .into_iter()
        .map(|(ascending, label)| {
            let current = spec.ascending == ascending;
            menu_item(
                Button::new(
                    if ascending {
                        "sort-ascending"
                    } else {
                        "sort-descending"
                    },
                    if current { "check" } else { "" },
                    label,
                    "ui.sort",
                )
                .args(serde_json::json!({"ascending": ascending}))
                .active(current),
                label,
            )
        })
        .collect();
    let mut show = vec![menu_item(
        Button::new(
            "menu-hidden",
            "eye",
            "Show hidden files",
            "ui.toggle_hidden",
        )
        .active(spec.show_hidden),
        "Hidden files",
    )];
    // Only the grid draws thumbnails in a box.
    if shell.ui().view_id == crate::view::GRID {
        show.push(menu_item(
            Button::new(
                "menu-grid-fill",
                "crop",
                "Crop thumbnails in the grid to fill their box",
                "ui.grid_fill",
            )
            .active(shell.prefs.grid_fill),
            "Fill thumbnails",
        ));
    }
    // The tree closes what it opened in one go.
    if shell.ui().view_id == crate::view::TREE {
        show.push(menu_item(
            Button::new(
                "menu-collapse-all",
                "fold",
                "Close every folder opened in place",
                "expand",
            )
            .args(serde_json::json!({"mode": "collapse_all"})),
            "Collapse all",
        ));
    }
    let mut size = Vec::new();
    for (dir, icon, label, tip) in [
        ("in", "plus", "Larger", "Larger (or Ctrl and the wheel)"),
        ("out", "minus", "Smaller", "Smaller (or Ctrl and the wheel)"),
        ("reset", "fit", "Default size", "Default size"),
    ] {
        size.push(menu_item(
            Button::new(format!("zoom-{dir}"), icon, tip, "ui.zoom")
                .args(serde_json::json!({"dir": dir})),
            label,
        ));
    }
    vec![
        ("Views", views),
        ("Sort by", order),
        ("Order", direction),
        ("Show", show),
        ("Zoom", size),
    ]
}

/// More's Layout groups: the window's panels, then where the split's panes
/// and the preview go. Split and Preview are toggled from the toolbar
/// unless `everything` (the narrowest variant, which dropped them).
fn layout_groups(shell: &Shell, everything: bool) -> Vec<(&'static str, Vec<Button>)> {
    let (split_icon, split_tip) = shell.layout.split_button();
    let mut panels = vec![menu_item(
        Button::new(
            "menu-sidebar",
            "sidebar",
            "Show the sidebar",
            "ui.toggle_sidebar",
        )
        .active(!shell.sidebar_hidden),
        "Sidebar",
    )];
    if everything {
        panels.push(menu_item(
            Button::new("menu-split", split_icon, split_tip, "ui.split").active(shell.is_split()),
            "Split panes",
        ));
        panels.push(menu_item(
            Button::new(
                "menu-preview",
                shell.layout.preview_icon(),
                "Preview panel",
                "ui.toggle_preview",
            )
            .active(shell.preview.open),
            "Preview panel",
        ));
    }
    panels.push(menu_item(
        Button::new("menu-timeline", "calendar", "Timeline", "ui.timeline")
            .active(shell.timeline.open),
        "Timeline",
    ));
    // The terminal is a plugin's: without it, nothing says it exists.
    if shell.app.terminals.provider().is_some() {
        panels.push(menu_item(
            Button::new("menu-terminal", "terminal", "Terminal", "ui.terminal")
                .active(shell.terminal.open),
            "Terminal",
        ));
    }
    vec![
        ("Layout", panels),
        ("Split direction", layout_items(shell, "panes")),
        ("Preview position", layout_items(shell, "preview")),
    ]
}

/// The choices of one side of the window's arrangement (`ui.layout`), the
/// current one checked. Automatic says what it means in this window.
fn layout_items(shell: &Shell, what: &'static str) -> Vec<Button> {
    use crate::layout::{Panes, Preview};
    let layout = &shell.layout;
    let choices: [(&str, &str, bool); 3] = if what == "panes" {
        [
            (
                "auto",
                if layout.tall() {
                    "Automatic (one above the other)"
                } else {
                    "Automatic (side by side)"
                },
                layout.panes == Panes::Auto,
            ),
            ("row", "Side by side", layout.panes == Panes::Row),
            (
                "column",
                "One above the other",
                layout.panes == Panes::Column,
            ),
        ]
    } else {
        [
            (
                "auto",
                if layout.tall() {
                    "Automatic (at the bottom)"
                } else {
                    "Automatic (on the right)"
                },
                layout.preview == Preview::Auto,
            ),
            ("right", "On the right", layout.preview == Preview::Right),
            ("bottom", "At the bottom", layout.preview == Preview::Bottom),
        ]
    };
    choices
        .into_iter()
        .map(|(value, label, current)| {
            let tip = if value == "auto" {
                "Follow the window's shape: stacked when it is taller than wide"
            } else {
                label
            };
            menu_item(
                Button::new(
                    format!("layout-{what}-{value}"),
                    if current { "check" } else { "" },
                    tip,
                    "ui.layout",
                )
                .args(serde_json::json!({ what: value }))
                .active(current),
                label,
            )
        })
        .collect()
}

/// A group's title in a popup menu.
fn group_title(title: &'static str, theme: &Theme) -> gpui::Div {
    div()
        .px_2()
        .pt_2()
        .pb_1()
        .text_xs()
        .text_color(theme.text_muted)
        .child(title)
}

/// The More menu. It holds what has no button in the chosen toolbar variant:
/// always the window's layout, New tab, the palette and Settings; in the
/// narrowest variant also the View menu, views first, and the folder tools
/// that variant dropped.
fn more_panel(
    shell: &Shell,
    popup: &toolbar::Popup,
    everything: bool,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let mut panel = toolbar::popup_panel(popup, "ui.toolbar_menu", theme, cx);
    let mut groups = Vec::new();
    if everything {
        // Without the View button, its whole menu is here: the views first.
        let mut options = view_options(shell).into_iter();
        groups.extend(options.next());
        groups.push((
            "Folder",
            vec![menu_item(
                Button::new(
                    "menu-new-folder",
                    "folder-plus",
                    "New folder",
                    "ui.mkdir_prompt",
                ),
                "New folder…",
            )],
        ));
        groups.extend(options);
    }
    groups.extend(layout_groups(shell, everything));
    groups.push((
        "bibli",
        [
            (
                Button::new("menu-new-tab", "tab-new", "New tab", "tab.new"),
                "New tab",
            ),
            (
                Button::new("menu-palette", "command", "Command palette", "ui.palette"),
                "Command palette…",
            ),
            (
                Button::new(
                    "menu-shortcuts",
                    "keyboard",
                    "Shortcuts here",
                    "ui.shortcuts",
                ),
                "Shortcuts…",
            ),
            (
                Button::new("menu-settings", "settings", "Settings", "ui.settings"),
                "Settings…",
            ),
        ]
        .into_iter()
        .map(|(button, label)| menu_item(button, label))
        .collect(),
    ));
    for (title, items) in groups {
        panel = panel.child(group_title(title, theme));
        for item in items {
            panel = panel.child(item.render(shell, cx));
        }
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
    if popup.kind == VIEW_OPTIONS {
        let mut panel = toolbar::popup_panel(popup, "ui.toolbar_menu", &theme, cx);
        for (title, items) in view_options(shell) {
            panel = panel.child(group_title(title, &theme));
            for item in items {
                panel = panel.child(item.render(shell, cx));
            }
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
    let (back, forward, path, up, is_shelf, active, location) = {
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
            pane.and_then(|p| p.location()).cloned(),
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
            Button::new("back", "back", "Back", "back")
                .enabled(back)
                .render(shell, cx),
        )
        .child(
            Button::new("forward", "forward", "Forward", "forward")
                .enabled(forward)
                .render(shell, cx),
        )
        .child(
            Button::new("up", "up", "Parent folder", "up")
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
            match path.clone().filter(|_| !focused) {
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
    // Keep what the pane shows in the sidebar, a folder as a bookmark or a
    // search as a saved search, while a plugin keeps such locations:
    // filled when it is kept, and pressing it then takes it back with the
    // place's own command.
    if let Some(location) = &location {
        let registry = &shell.app.registry;
        let keeper = registry.keepers.find(|k| *k.scheme == *location.scheme);
        let kept = registry
            .places
            .find(|p| p.forget.is_some() && crate::sidebar::same_place(&p.location, location))
            .and_then(|p| p.forget);
        let button = match (kept, keeper) {
            (Some(forget), keeper) => Some(
                Button::new(
                    "keep",
                    "bookmark-filled",
                    keeper.map_or_else(|| "Remove from sidebar".into(), |k| k.forget_title),
                    forget.name.to_string(),
                )
                .args(forget.args),
            ),
            (None, Some(keeper)) => {
                Some(Button::new("keep", "bookmark", keeper.save_title, "keep"))
            }
            (None, None) => None,
        };
        if let Some(button) = button {
            bar = bar.child(button.render(shell, cx));
        }
    }
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
        let in_tab = target.clone();
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
                    serde_json::json!({ "location": PathArg::from(open.as_path()), "pane": pane }),
                    window,
                    cx,
                );
            }))
            // The middle button opens the folder in a tab behind this one.
            .on_mouse_down(
                gpui::MouseButton::Middle,
                cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.run(
                        "tab.new",
                        serde_json::json!({
                            "location": PathArg::from(in_tab.as_path()),
                            "show": false,
                            "pane": pane,
                        }),
                        window,
                        cx,
                    );
                }),
            )
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
    use super::{ViewSpec, crumbs, toolbar_views};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    fn shown(current: &str, narrowest: bool) -> Vec<String> {
        let views = vec![
            ViewSpec::named("list", "List", "list", Arc::default()),
            ViewSpec::named("tree", "Tree", "tree", Arc::default()).alternate(),
            ViewSpec::named("grid", "Grid", "grid", Arc::default()),
            ViewSpec::named("usage", "Usage", "usage", Arc::default()).lens(),
        ];
        toolbar_views(views, current, narrowest)
            .into_iter()
            .map(|spec| spec.id.to_string())
            .collect()
    }

    #[test]
    fn a_lens_or_an_alternate_has_a_button_only_while_it_is_shown() {
        assert_eq!(shown("grid", false), ["list", "grid"]);
        assert_eq!(shown("usage", false), ["list", "grid", "usage"]);
        assert_eq!(shown("tree", false), ["list", "tree", "grid"]);
        assert_eq!(shown("grid", true), ["grid"]);
        assert_eq!(shown("usage", true), ["usage"]);
    }

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
