//! Each pane's row (navigation, the address, how the pane shows its
//! folder), the sidebar's header, and where the window's own tools go
//! among them (see `corner`).
use crate::{
    elements::{adaptive::AdaptiveRow, popover::PopoverAnchor},
    keymap,
    shell::Shell,
    theme::Theme,
    toolbar::{self, Button},
};
use bibli_core::os_path::PathArg;
use bibli_core::registry::{ViewRole, ViewSpec};
use gpui::{AnyElement, Context, Window, div, prelude::*, px};
use std::path::PathBuf;

/// What the address field keeps before the view buttons fold into one.
const FIELD_MIN: f32 = 180.;
/// A pane row's height, and the sidebar header's.
pub const PANE_ROW_HEIGHT: f32 = 42.;

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
        if close {
            if let Some(open) = open {
                self.note_dismissed(args, open.to_owned());
            }
        } else if open != Some(kind) && !self.just_dismissed(kind) {
            self.toolbar_menu = Some(toolbar::Popup::new(kind, window, cx));
        }
        cx.notify();
    }
}

const VIEW_OPTIONS: &str = "view_options";
const MORE: &str = "more";

/// A view's bundled icon, or the grid's for a plugin view without one.
fn view_icon(spec: &ViewSpec) -> gpui::SharedString {
    if toolbar::asset(&format!("toolbar/{}.svg", spec.icon)).is_some() {
        spec.icon.clone().into()
    } else {
        "grid".into()
    }
}

fn view_button(shell: &Shell, spec: &ViewSpec, prefix: &str) -> Button {
    Button::new(
        format!("{prefix}-{}", spec.id),
        view_icon(spec),
        format!("{} view", spec.title),
        "ui.view",
    )
    .args(serde_json::json!({"id": spec.id}))
    .active(shell.ui().view_id == &*spec.id)
}

/// The pane row's view buttons: the views that show the listing, and an
/// alternate shape or a lens only while it is shown; the View menu offers
/// them all.
fn toolbar_views(views: Vec<ViewSpec>, current: &str) -> Vec<ViewSpec> {
    views
        .into_iter()
        .filter(|s| &*s.id == current || s.role == ViewRole::Listing)
        .collect()
}

/// How the pane shows its folder: its view buttons and the View menu
/// beside them (`wide`), or one button, the current view's icon, that
/// opens the menu, which starts with every view.
fn view_control(
    shell: &Shell,
    wide: bool,
    anchor: &PopoverAnchor,
    open: bool,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let menu = |id: &'static str, icon: gpui::SharedString, title: String| {
        Button::new(id, icon, title, "ui.toolbar_menu")
            .args(serde_json::json!({"kind": VIEW_OPTIONS}))
            .anchor_if(anchor, true)
            .active(open)
            .dropdown()
    };
    let views = shell.available_views();
    let current = shell.ui().view_id;
    if !wide {
        let shown = views.iter().find(|s| &*s.id == current);
        return menu(
            "view-menu",
            shown.map_or_else(|| "list".into(), view_icon),
            match shown {
                Some(spec) => format!("{} view, sort, hidden files and zoom", spec.title),
                None => "Views, sort, hidden files and zoom".into(),
            },
        )
        .render(shell, cx)
        .into_any_element();
    }
    let theme = cx.global::<Theme>().clone();
    let mut group = div()
        .flex()
        .flex_none()
        .gap(px(1.))
        .p(px(2.))
        .rounded_lg()
        .border_1()
        .border_color(theme.border)
        .bg(theme.bg);
    for spec in toolbar_views(views, current) {
        group = group.child(
            view_button(shell, &spec, "view")
                .render(shell, cx)
                .h(px(24.)),
        );
    }
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(2.))
        .child(group)
        .child(
            menu(
                "view-options",
                "sliders".into(),
                "Sort, hidden files and zoom".into(),
            )
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
/// Layout). It starts with every view, the lenses the pane row leaves out
/// included: a narrow pane row has only the button that opens it.
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
    // Ratings are the tags plugin's: no sort by them while it is off.
    let rated = shell.app.commands.get("tags.rate").is_some();
    let order: Vec<Button> = [
        ("name", "Name", SortKey::Name),
        ("size", "Size", SortKey::Size),
        ("modified", "Date modified", SortKey::Modified),
        ("rating", "Rating", SortKey::Rating),
    ]
    .into_iter()
    .filter(|(_, _, sort_key)| rated || *sort_key != SortKey::Rating)
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
        SortKey::Rating => ("Lowest first", "Highest first"),
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
    // With hidden files, the files that keep another's notes beside it
    // (ADR-028): the same setting as General's, where one looks for a file
    // that seems missing.
    if spec.show_hidden && !shell.app.companions.is_empty() {
        let shown = shell.app.companions.shown();
        show.push(menu_item(
            Button::new(
                "menu-companions",
                "layers",
                "Also list the files that keep another file's notes beside it (a Mac's ._ files, sidecars)",
                "settings.apply",
            )
            .args(serde_json::json!({
                "changes": { bibli_core::companions::SHOW: (!shown).to_string() }
            }))
            .active(shown),
            "Companion files",
        ));
    }
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

/// More's Layout groups: the panels without a button of their own, then
/// where the split's panes and the preview go.
fn layout_groups(shell: &Shell) -> Vec<(&'static str, Vec<Button>)> {
    let mut panels = vec![menu_item(
        Button::new("menu-timeline", "calendar", "Timeline", "ui.timeline")
            .active(shell.timeline.open),
        "Timeline",
    )];
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
        ("File in an archive", layout_items(shell, "member")),
    ]
}

fn layout_items(shell: &Shell, what: &'static str) -> Vec<Button> {
    use crate::layout::{Member, Panes, Preview};
    let layout = &shell.layout;
    let mut automatic = "Follow the window's shape: stacked when it is taller than wide";
    let choices: [(&str, &str, bool); 3] = if what == "member" {
        // Where a file of an archive, or an attachment, is previewed beside
        // the rows it came from; Automatic follows the panel's shape.
        automatic = "Follow the preview panel's shape: under the archive's files when it is \
                     at the right, at their right when it is at the bottom";
        [
            (
                "auto",
                if layout.member.beside(layout.bottom()) {
                    "Automatic (on the right)"
                } else {
                    "Automatic (at the bottom)"
                },
                layout.member == Member::Auto,
            ),
            ("right", "On the right", layout.member == Member::Right),
            ("bottom", "At the bottom", layout.member == Member::Bottom),
        ]
    } else if what == "panes" {
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
            let tip = if value == "auto" { automatic } else { label };
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

/// The More menu: what has no button, the window's layout, New tab, the
/// palette and Settings.
fn more_panel(
    shell: &Shell,
    popup: &toolbar::Popup,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let mut panel = toolbar::popup_panel(popup, "ui.toolbar_menu", theme, cx);
    let mut groups = layout_groups(shell);
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

/// The window's own tools have no row of their own: each area owns its
/// top. The sidebar's header holds its toggle and More; a sidebar not
/// drawn leaves them at the start of the first pane's row
/// (`Shell::corner_pane`). Split and Preview mirror them at the right, in
/// the order of what they open (the middle of the window, then its edge):
/// in the preview's own bar while it is open at the right, else at the end
/// of the tab strip, else of the row of the pane at the window's right
/// (`Shell::toggles_at`). Tabs, when there are two or more (or one
/// asked for, `ui.single_tab`), are a strip above the panes (`tabs.rs`).
/// So a window of one tab has one row above its lists, split or not.
///
/// The sidebar toggle and More, as the sidebar's header or a pane row
/// starts with them. More hangs from `Shell::more_anchor`.
fn corner(shell: &Shell, cx: &mut Context<Shell>) -> [AnyElement; 2] {
    let open = shell.toolbar_menu.as_ref().map(|menu| menu.kind);
    [
        Button::new("sidebar-toggle", "sidebar", "Sidebar", "ui.toggle_sidebar")
            .active(!shell.sidebar_hidden)
            .render(shell, cx)
            .into_any_element(),
        Button::new("tools-more", "more", "More", "ui.toolbar_menu")
            .args(serde_json::json!({"kind": MORE}))
            .anchor_if(&shell.more_anchor, true)
            .active(open == Some(MORE))
            .render(shell, cx)
            .into_any_element(),
    ]
}

/// Split and Preview, at the end of what has them (`Shell::toggles_at`).
pub fn panel_toggles(shell: &Shell, cx: &mut Context<Shell>) -> [AnyElement; 2] {
    let (split_icon, split_tip) = shell.layout.split_button();
    [
        Button::new("split-toggle", split_icon, split_tip, "ui.split")
            .active(shell.is_split())
            .render(shell, cx)
            .into_any_element(),
        Button::new(
            "preview-toggle",
            shell.layout.preview_icon(),
            "Preview panel",
            "ui.toggle_preview",
        )
        .active(shell.preview.open)
        .render(shell, cx)
        .into_any_element(),
    ]
}

/// The sidebar's header: its toggle, and More. As tall as a pane's row,
/// so their edges meet.
pub fn sidebar_header(shell: &Shell, cx: &mut Context<Shell>) -> AnyElement {
    let theme = cx.global::<Theme>().clone();
    let [toggle, more] = corner(shell, cx);
    div()
        .id("sidebar-header")
        .tab_group()
        .flex_none()
        .h(px(PANE_ROW_HEIGHT))
        .flex()
        .items_center()
        .gap(px(2.))
        .px(px(6.))
        .bg(theme.surface)
        .border_b_1()
        .border_r_1()
        .border_color(theme.border)
        .child(toggle)
        .child(div().flex_1())
        .child(more)
        .into_any_element()
}

/// The More menu, under its button wherever it is this frame.
pub fn more_popup(shell: &Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let popup = shell.toolbar_menu.as_ref().filter(|p| p.kind == MORE)?;
    let theme = cx.global::<Theme>().clone();
    Some(
        shell
            .more_anchor
            .popup(more_panel(shell, popup, &theme, cx)),
    )
}

/// What the address field shows.
enum Address {
    /// A folder, as a trail of places.
    Trail(PathBuf),
    /// A search, as pills (`search_bar::pills`).
    Pills,
    /// The text: while it is being edited (Ctrl+L, or a click beside the
    /// trail or the pills), and for other locations.
    Text,
}

/// What a pane's row shows, read once for both of its variants.
struct PaneRow {
    back: bool,
    forward: bool,
    address: Address,
    /// The field's edge: focused, the active pane's, or none.
    edge: gpui::Hsla,
    location: Option<bibli_core::Location>,
    /// What the names shown must hold, typed in the field (`omnibox.rs`).
    filter: Option<String>,
    menu_open: bool,
    /// The menu of places open from this row, if one is.
    nav: Option<crate::nav_menu::NavWhat>,
    /// The window's tools this row holds (see `corner`).
    ends: Ends,
}

/// The window's tools a pane's row holds: the sidebar toggle and More at
/// its start, Split and Preview at its end.
#[derive(Clone, Copy)]
struct Ends {
    corner: bool,
    toggles: bool,
}

/// A pane's row: Back and Forward, the address (a trail of folders, or the
/// text being edited) with the button that keeps it in the sidebar, then
/// how the pane shows its folder. Up is the trail's parent segment, or
/// Alt+Up.
///
/// Two variants, chosen in prepaint by `AdaptiveRow`: the view buttons
/// while the address keeps `FIELD_MIN`, else the one button that opens the
/// View menu. The address counts as `FIELD_MIN` when a variant is measured
/// (its content has no width of its own) and takes the rest when drawn.
pub fn render_address(shell: &Shell, window: &Window, cx: &mut Context<Shell>) -> AnyElement {
    let theme = cx.global::<Theme>().clone();
    let row = {
        let ws = shell.app.workspace.read();
        let pane = ws.pane(shell.pane);
        let location = pane.and_then(|p| p.location()).cloned();
        let active = ws.active_id() == shell.pane;
        let focused = shell.ui().address.read(cx).is_focused(window);
        PaneRow {
            back: pane.is_some_and(|p| p.history_pos > 0),
            forward: pane.is_some_and(|p| p.history_pos + 1 < p.history.len()),
            address: match location.as_ref().and_then(bibli_core::Location::path) {
                _ if focused => Address::Text,
                Some(path) => Address::Trail(path),
                None if !shell.ui().chips.chips.is_empty() => Address::Pills,
                None => Address::Text,
            },
            edge: if focused {
                theme.accent
            } else if active {
                theme.border
            } else {
                theme.surface
            },
            filter: pane.and_then(|p| p.filter().map(str::to_owned)),
            location,
            // The View menu hangs from the active pane's button: the pane it
            // acts on.
            menu_open: active
                && shell
                    .toolbar_menu
                    .as_ref()
                    .is_some_and(|menu| menu.kind == VIEW_OPTIONS),
            nav: shell
                .nav_menu
                .as_ref()
                .filter(|menu| menu.pane == shell.pane)
                .map(|menu| menu.what.clone()),
            ends: Ends {
                corner: shell.corner_pane() == Some(shell.pane),
                toggles: shell.toggles_at() == crate::shell::TogglesAt::Pane(shell.pane),
            },
        }
    };
    let anchor = PopoverAnchor::default();
    let nav_anchor = PopoverAnchor::default();
    let chip_anchor = PopoverAnchor::default();
    let omni_anchor = PopoverAnchor::default();
    let mut bar = div()
        .id(("pane-address", shell.pane.0 as usize))
        .tab_group()
        .relative()
        .px(px(6.))
        .w_full()
        .flex_none()
        .border_b_1()
        .border_color(theme.border)
        .bg(theme.surface)
        .child(AdaptiveRow {
            variants: [true, false]
                .into_iter()
                .map(|wide| {
                    let anchors = [&anchor, &nav_anchor, &chip_anchor, &omni_anchor];
                    pane_row(shell, &row, wide, anchors, cx)
                })
                .collect(),
            height: PANE_ROW_HEIGHT - 1.,
            chosen: None,
        });
    if row.menu_open
        && let Some(popup) = &shell.toolbar_menu
    {
        let mut panel = toolbar::popup_panel(popup, "ui.toolbar_menu", &theme, cx);
        for (title, items) in view_options(shell) {
            panel = panel.child(group_title(title, &theme));
            for item in items {
                panel = panel.child(item.render(shell, cx));
            }
        }
        bar = bar.child(anchor.popup(panel));
    }
    // What the text typed will do, under the field typed in.
    if matches!(row.address, Address::Text)
        && let Some(dropdown) = crate::omnibox::dropdown(shell, &omni_anchor, cx)
    {
        bar = bar.child(dropdown);
    }
    if let Some(popup) = crate::search_bar::popup(shell, &chip_anchor, cx) {
        bar = bar.child(popup);
    }
    if let Some(panel) = crate::nav_menu::panel(shell, shell.pane, cx) {
        bar = bar.child(nav_anchor.popup(panel));
    }
    bar.into_any_element()
}

/// The filter typed in the field, once the field let go of the keyboard:
/// pressed, it is typed in again (Enter then searches the folders inside);
/// its × shows everything, as Escape in the pane does.
fn filter_pill(shell: &Shell, filter: &str, theme: &Theme, cx: &mut Context<Shell>) -> AnyElement {
    let pane = shell.pane;
    let hover = theme.hover;
    Button::new(
        "filter-pill",
        "search",
        format!("Only the names holding “{filter}”"),
        "ui.search",
    )
    .args(serde_json::json!({ "text": filter }))
    .label(format!("“{filter}”"))
    .active(true)
    .render(shell, cx)
    .h(px(22.))
    .max_w(px(200.))
    .rounded_full()
    .pr(px(2.))
    .mr(px(2.))
    .child(
        div()
            .id("filter-clear")
            .size(px(18.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.activate_pane(pane, cx);
                this.set_filter(None);
                cx.notify();
            }))
            .child(crate::controls::glyph("close", theme.selection_text, 10.)),
    )
    .into_any_element()
}

/// Back or Forward: its right button lists the history that way.
fn history_button(
    shell: &Shell,
    row: &PaneRow,
    forward: bool,
    anchor: &PopoverAnchor,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let (id, title, command, enabled) = if forward {
        ("forward", "Forward", "forward", row.forward)
    } else {
        ("back", "Back", "back", row.back)
    };
    let open = row.nav == Some(crate::nav_menu::NavWhat::History { forward });
    let pane = shell.pane;
    Button::new(id, id, title, command)
        .enabled(enabled)
        .active(open)
        .anchor_if(anchor, open)
        .render(shell, cx)
        .on_mouse_down(
            gpui::MouseButton::Right,
            cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                // The menu takes the keyboard; the button must not take it
                // back once this handler returns.
                window.prevent_default();
                this.activate_pane(pane, cx);
                this.run(
                    "ui.history_menu",
                    serde_json::json!({ "forward": forward }),
                    window,
                    cx,
                );
            }),
        )
        .into_any_element()
}

/// One variant of a pane's row (see `render_address`). `anchors` are the
/// View menu's, the menu of places', the search's popover's and the
/// address field's dropdown.
fn pane_row(
    shell: &Shell,
    row: &PaneRow,
    wide: bool,
    [anchor, nav_anchor, chip_anchor, omni_anchor]: [&PopoverAnchor; 4],
    cx: &mut Context<Shell>,
) -> AnyElement {
    let theme = cx.global::<Theme>().clone();
    let pane = shell.pane;
    // A folder reads as a trail of places and a search as pills; the text
    // field takes over while it is being edited. No width of its own: the
    // field's minimum is what a variant is measured with.
    let address = match &row.address {
        Address::Pills => crate::search_bar::pills(shell, chip_anchor, cx)
            .unwrap_or_else(|| div().into_any_element()),
        Address::Trail(path) => div()
            .flex_1()
            .w(px(0.))
            .min_w_0()
            .h_full()
            .flex()
            .items_center()
            .child(
                div()
                    .id(("address-trail", pane.0 as usize))
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .items_center()
                    .px(px(3.))
                    .cursor_text()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.activate_pane(pane, cx);
                        this.run("ui.focus_address", serde_json::json!({}), window, cx);
                    }))
                    .child(breadcrumbs(
                        shell,
                        path,
                        row.nav.as_ref(),
                        nav_anchor,
                        &theme,
                        cx,
                    )),
            )
            .children(
                row.filter
                    .as_deref()
                    .map(|filter| filter_pill(shell, filter, &theme, cx)),
            )
            .into_any_element(),
        // While a search is typed, where it looks stands before the text.
        Address::Text => div()
            .flex_1()
            .w(px(0.))
            .min_w_0()
            .h_full()
            .relative()
            .flex()
            .items_center()
            .children(crate::omnibox::scope_pill(shell, cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .px_2()
                    .child(shell.ui().address.clone()),
            )
            .child(omni_anchor.marker())
            .into_any_element(),
    };
    let mut field = div()
        .id(("address-field", pane.0 as usize))
        .flex_1()
        .min_w(px(FIELD_MIN))
        .h(px(30.))
        .flex()
        .items_center()
        .rounded_md()
        .border_1()
        .border_color(row.edge)
        .bg(theme.bg)
        .key_context(keymap::ADDRESS)
        .child(address);
    // Keep what the pane shows in the sidebar, a folder as a bookmark or a
    // search as a saved search, while a plugin keeps such locations:
    // filled when it is kept, and pressing it then takes it back with the
    // place's own command. It ends the field: it is about what the field
    // shows.
    if let Some(location) = &row.location {
        let registry = &shell.app.registry;
        let keeper = registry.keepers.find(|k| *k.scheme == *location.scheme);
        let kept = registry
            .places
            .find(|p| p.forget.is_some() && crate::sidebar::same_place(&p.location, location));
        // A tag's search from the home folder is kept as the tag's pin.
        let tag = shell
            .ui()
            .chips
            .only_tag(&location.to_string())
            .filter(|_| shell.app.commands.get("tags.pin").is_some())
            .map(str::to_owned);
        let button = match (kept, keeper, tag) {
            (Some(place), keeper, _) => {
                let forget = place.forget.expect("kept");
                let title = if place.color.is_some() {
                    "Unpin this tag from the sidebar".into()
                } else {
                    keeper.map_or_else(|| "Remove from sidebar".into(), |k| k.forget_title)
                };
                Some(
                    Button::new("keep", "bookmark-filled", title, forget.name.to_string())
                        .args(forget.args),
                )
            }
            (None, _, Some(tag)) => Some(
                Button::new(
                    "keep",
                    "bookmark",
                    "Pin this tag to the sidebar",
                    "tags.pin",
                )
                .args(serde_json::json!({ "tag": tag })),
            ),
            (None, Some(keeper), None) => {
                Some(Button::new("keep", "bookmark", keeper.save_title, "keep"))
            }
            (None, None, None) => None,
        };
        if let Some(button) = button {
            field = field.child(
                button
                    .render(shell, cx)
                    .h(px(24.))
                    .min_w(px(24.))
                    .px(px(4.))
                    .mr(px(2.)),
            );
        }
    }
    let theme = cx.global::<Theme>().clone();
    let mut line = div().size_full().flex().items_center().gap(px(3.));
    if row.ends.corner {
        let [toggle, more] = corner(shell, cx);
        line = line
            .child(toggle)
            .child(more)
            .child(toolbar::separator(&theme));
    }
    line = line
        .child(history_button(shell, row, false, nav_anchor, cx))
        .child(history_button(shell, row, true, nav_anchor, cx))
        .child(field)
        .child(view_control(shell, wide, anchor, row.menu_open, cx));
    if row.location.as_ref().is_some_and(|l| &*l.scheme == "shelf") {
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
            line = line.child(
                Button::new(id, icon, title, command)
                    .enabled(shell.is_split())
                    .render(shell, cx),
            );
        }
    }
    if row.ends.toggles {
        let [split, preview] = panel_toggles(shell, cx);
        line = line
            .child(toolbar::separator(&theme))
            .child(split)
            .child(preview);
    }
    line.into_any_element()
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
/// nothing measured. Every segment but the outermost carries its chevron:
/// a trail that starts with one visibly continues to the left.
///
/// A chevron lists the folders inside the segment before it, the one the
/// trail goes on into marked; one more after the last lists the folders
/// inside the folder shown. `open` is the menu of places open from this
/// row, whose chevron hangs it from `anchor`.
fn breadcrumbs(
    shell: &Shell,
    path: &std::path::Path,
    open: Option<&crate::nav_menu::NavWhat>,
    anchor: &PopoverAnchor,
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
    let chevron =
        |index: usize, folder: &PathBuf, current: Option<&PathBuf>, cx: &mut Context<Shell>| {
            let shown = open
                == Some(&crate::nav_menu::NavWhat::Subfolders {
                    folder: folder.clone(),
                });
            let args = serde_json::json!({
                "path": PathArg::from(folder.as_path()),
                "current": current.map(|c| PathArg::from(c.as_path())),
            });
            let font = shell.font.clone();
            let name = folder.file_name().map_or_else(
                || folder.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            );
            let tip: gpui::SharedString = format!("Folders in {name}").into();
            let hover = theme.hover;
            div()
                .id(("crumb-folders", index))
                .relative()
                .flex_none()
                .h(px(24.))
                .w(px(16.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_sm()
                .cursor_pointer()
                .hover(move |s| s.bg(hover))
                .when(shown, |d| d.bg(theme.hover))
                .tooltip(move |_, cx| toolbar::tooltip(tip.clone(), font.clone(), cx))
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.activate_pane(pane, cx);
                    this.run("ui.subfolder_menu", args.clone(), window, cx);
                }))
                .child(
                    gpui::svg()
                        .path(if shown {
                            "toolbar/chevron.svg"
                        } else {
                            "toolbar/chevron-right.svg"
                        })
                        .size(px(12.))
                        .flex_none()
                        .text_color(theme.text_muted),
                )
                .when(shown, |d| d.child(anchor.marker()))
        };
    for (index, (label, target)) in crumbs.iter().enumerate().rev() {
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
            .child(label.clone());
        let segment = crate::dnd::droppable_dir(segment, target.clone(), cx);
        let before = index
            .checked_sub(1)
            .map(|parent| chevron(index, &crumbs[parent].1, Some(target), cx));
        let after = (index == last).then(|| chevron(index + 1, target, None, cx));
        trail = trail.child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .children(before)
                .child(segment)
                .children(after),
        );
    }
    trail.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{ViewSpec, crumbs, toolbar_views};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    fn shown(current: &str) -> Vec<String> {
        let views = vec![
            ViewSpec::named("list", "List", "list", Arc::default()),
            ViewSpec::named("tree", "Tree", "tree", Arc::default()).alternate(),
            ViewSpec::named("grid", "Grid", "grid", Arc::default()),
            ViewSpec::named("usage", "Usage", "usage", Arc::default()).lens(),
        ];
        toolbar_views(views, current)
            .into_iter()
            .map(|spec| spec.id.to_string())
            .collect()
    }

    #[test]
    fn a_lens_or_an_alternate_has_a_button_only_while_it_is_shown() {
        assert_eq!(shown("grid"), ["list", "grid"]);
        assert_eq!(shown("usage"), ["list", "grid", "usage"]);
        assert_eq!(shown("tree"), ["list", "tree", "grid"]);
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
