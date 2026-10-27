//! Each pane's row (navigation, the address, how the pane shows its
//! folder), the sidebar's header, and where the window's own tools go
//! among them (see `corner`).
use crate::{
    elements::{adaptive::AdaptiveRow, popover::PopoverAnchor},
    keymap,
    shell::Shell,
    theme::Theme,
    toolbar::{self, Button, MenuPart},
};
use gpui::{AnyElement, Context, Window, div, prelude::*, px};
use skwir_core::location::{Crumb, Location, Tool, ToolPane};
use skwir_core::registry::{ViewRole, ViewSpec};
use skwir_core::workspace::PaneId;
use std::path::PathBuf;

/// What the address field keeps before the view buttons fold into one.
const FIELD_MIN: f32 = 180.;
/// The address field's height, which the field's card leaves for it.
pub(crate) const FIELD_HEIGHT: f32 = 30.;
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
            Some("view_buttons") => VIEW_BUTTONS,
            Some("grid_captions") => GRID_CAPTIONS,
            Some(crate::map_view::BACKGROUNDS) => crate::map_view::BACKGROUNDS,
            Some(crate::dock::MENU) => crate::dock::MENU,
            Some(crate::timeline::DATES_MENU) => crate::timeline::DATES_MENU,
            _ => MORE,
        };
        if self.toggle_popup(kind, args, window, cx) {
            self.toolbar_menu = Some(toolbar::Popup::new(kind, window, cx));
        }
        cx.notify();
    }
}

const VIEW_OPTIONS: &str = "view_options";
/// Which views have a button: the buttons' right-click, the Menu key on
/// one, or the View menu's Buttons row.
const VIEW_BUTTONS: &str = "view_buttons";
/// What the grid's tiles say under their names: the View menu's Under
/// names row, in the grid.
const GRID_CAPTIONS: &str = "grid_captions";
const MORE: &str = "more";

/// The views the user gave a button, or took it from (`ui.view_button`,
/// `settings::VIEW_BUTTONS_KEY`), by id. A view not named keeps what its
/// role gives it, so a view added later comes as its plugin meant.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ViewButtons(std::collections::BTreeMap<String, bool>);

impl ViewButtons {
    pub fn read(app: &skwir_core::App) -> Self {
        Self(
            app.settings
                .get(crate::settings::VIEW_BUTTONS_KEY)
                .unwrap_or_default(),
        )
    }

    /// Whether `spec` has a button while another view is shown: as chosen,
    /// else if it shows the listing (`ViewRole::Listing`).
    pub fn pinned(&self, spec: &ViewSpec) -> bool {
        self.0
            .get(&*spec.id)
            .copied()
            .unwrap_or(spec.role == ViewRole::Listing)
    }

    /// `spec` with a button (`on`) or without: a choice that is its
    /// role's is not kept, so Default buttons is nothing chosen.
    #[must_use]
    pub fn with(mut self, spec: &ViewSpec, on: bool) -> Self {
        if on == (spec.role == ViewRole::Listing) {
            self.0.remove(&*spec.id);
        } else {
            self.0.insert(spec.id.to_string(), on);
        }
        self
    }

    pub fn is_default(&self) -> bool {
        self.0.is_empty()
    }

    pub fn value(&self) -> toml::Value {
        toml::Value::Table(
            self.0
                .iter()
                .map(|(id, on)| (id.clone(), toml::Value::Boolean(*on)))
                .collect(),
        )
    }
}

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

/// The pane row's view buttons: those the user chose, else the views that
/// show the listing, and any other only while it is shown, so the row
/// always says which view is shown and is there to be right-clicked; the
/// View menu offers them all.
fn toolbar_views(views: Vec<ViewSpec>, current: &str, chosen: &ViewButtons) -> Vec<ViewSpec> {
    views
        .into_iter()
        .filter(|s| &*s.id == current || chosen.pinned(s))
        .collect()
}

/// How the pane shows its folder: its view buttons and the View menu
/// beside them (`wide`), or one button, the current view's icon, that
/// opens the menu, which starts with every view. `open` is the popup open
/// from here: the View menu, or which views have a button, which hangs
/// from the buttons, or from the one button when there are none.
fn view_control(
    shell: &Shell,
    wide: bool,
    anchor: &PopoverAnchor,
    open: Option<&'static str>,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let choosing = open == Some(VIEW_BUTTONS);
    let menu = |id: &'static str, icon: gpui::SharedString, title: String| {
        Button::new(id, icon, title, "ui.toolbar_menu")
            .args(serde_json::json!({"kind": VIEW_OPTIONS}))
            .anchor_if(anchor, !(wide && choosing))
            .active(matches!(open, Some(VIEW_OPTIONS | GRID_CAPTIONS)))
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
    let pane = shell.pane;
    // Right-clicked, or the Menu key on one of them, the buttons say which
    // views have one, as the list's header says which columns it shows.
    let mut group = div()
        .id(("view-buttons", pane.0 as usize))
        .key_context(keymap::VIEW_BUTTONS)
        .relative()
        .flex()
        .flex_none()
        .gap(px(1.))
        .p(px(2.))
        .rounded_lg()
        .border_1()
        .border_color(theme.border)
        .bg(theme.bg)
        // Taken on its way in: a button takes the keyboard on any press
        // before its row hears of it, and the popup must take it from
        // what had it, not from the button.
        .capture_any_mouse_down(
            cx.listener(move |this, ev: &gpui::MouseDownEvent, window, cx| {
                if ev.button != gpui::MouseButton::Right {
                    return;
                }
                cx.stop_propagation();
                window.prevent_default();
                this.activate_pane(pane, cx);
                this.run(
                    "ui.toolbar_menu",
                    serde_json::json!({ "kind": VIEW_BUTTONS }),
                    window,
                    cx,
                );
            }),
        )
        .when(choosing, |d| d.child(anchor.marker()));
    for spec in toolbar_views(views, current, &shell.view_buttons) {
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

/// The View menu: how the active pane shows its folder, never how the
/// window is arranged (that is More's). Every view as a tile, the lenses
/// the pane row leaves out included (a narrow pane row has only the button
/// that opens this menu); the sort, its direction on the row of its name;
/// what is shown; the size, on one row.
fn view_options(shell: &Shell) -> Vec<MenuPart> {
    use skwir_core::SortKey;
    let views: Vec<Button> = shell
        .available_views()
        .into_iter()
        .map(|spec| {
            view_button(shell, &spec, "menu-view")
                .label(spec.title.clone())
                .tile()
        })
        .collect();
    // Where one looks for them first; the buttons' right-click says the same.
    let buttons = Button::new(
        "menu-view-buttons",
        "",
        "Choose which views have a button beside the address",
        "ui.toolbar_menu",
    )
    .args(serde_json::json!({ "kind": VIEW_BUTTONS }))
    .label("Buttons…");
    let spec = shell.sort_spec();
    // By name, then by what each column that sorts shows, in its words.
    let order: Vec<MenuPart> = shell
        .sort_keys()
        .into_iter()
        .map(|(label, sort_key)| {
            let current = spec.key == sort_key;
            // The current key again keeps its direction: the row of its
            // name says it.
            let args = if current {
                serde_json::json!({"key": sort_key, "ascending": spec.ascending})
            } else {
                serde_json::json!({"key": sort_key})
            };
            MenuPart::Row(menu_item(
                Button::new(
                    format!("sort-{sort_key:?}"),
                    if current { "check" } else { "" },
                    format!("Sort by {}", label.to_lowercase()),
                    "sort",
                )
                .args(args)
                .active(current)
                .keeps_open(),
                label,
            ))
        })
        .collect();
    // Each key names its directions in its own words, short on the row,
    // whole in the tooltip.
    let ((up, up_tip), (down, down_tip)) = match spec.key {
        SortKey::Name => (("A to Z", "A to Z"), ("Z to A", "Z to A")),
        SortKey::Size => (("Smallest", "Smallest first"), ("Largest", "Largest first")),
        SortKey::Modified | SortKey::Deleted => {
            (("Oldest", "Oldest first"), ("Newest", "Newest first"))
        }
        SortKey::Rating => (("Lowest", "Lowest first"), ("Highest", "Highest first")),
    };
    let direction: Vec<Button> = [(true, "up", up, up_tip), (false, "down", down, down_tip)]
        .into_iter()
        .map(|(ascending, icon, label, tip)| {
            Button::new(
                if ascending {
                    "sort-ascending"
                } else {
                    "sort-descending"
                },
                icon,
                tip,
                "sort",
            )
            .args(serde_json::json!({"ascending": ascending}))
            .label(label)
            .active(spec.ascending == ascending)
            .segment()
        })
        .collect();
    let mut show = vec![menu_item(
        Button::new("menu-hidden", "", "Show hidden files", "toggle_hidden")
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
                "",
                "Also list the files that keep another file's notes beside it (a Mac's ._ files, sidecars)",
                "settings.apply",
            )
            .args(serde_json::json!({
                "changes": { skwir_core::companions::SHOW: (!shown).to_string() }
            }))
            .active(shown),
            "Companion files",
        ));
    }
    // How the location shown can show otherwise (a tag location's
    // "Include Sub-Tags"): where it leads switched.
    if let Some(trail) = &shell.ui().place.trail {
        let pane = shell.pane;
        for (i, toggle) in trail.toggles.iter().enumerate() {
            show.push(menu_item(
                Button::new(
                    format!("menu-toggle-{i}"),
                    "",
                    toggle.label.clone(),
                    "navigate",
                )
                .args(serde_json::json!({
                    "location": serde_json::json!(toggle.location.arg()),
                    "pane": pane,
                }))
                .active(toggle.on),
                toggle.label.clone(),
            ));
        }
    }
    // A folder narrowed by its files' kinds, years and tags (`narrow.rs`).
    if shell.ui().place.folder {
        show.push(menu_item(
            Button::new(
                "menu-narrow-folders",
                "",
                "Offer the kinds, years and tags of a folder's files above them, to show only those",
                "ui.narrow_folders",
            )
            .active(shell.prefs.narrow_folders),
            "Narrow by",
        ));
    }
    // Only the grid draws thumbnails in a box.
    if shell.ui().view_id == crate::view::GRID {
        show.push(menu_item(
            Button::new(
                "menu-grid-fill",
                "",
                "Crop thumbnails in the grid to fill their box",
                "ui.grid_fill",
            )
            .active(shell.prefs.grid_fill),
            "Fill thumbnails",
        ));
        show.push(menu_item(
            Button::new(
                "menu-grid-captions",
                "contents",
                "Choose what each tile says under its name: its tags, its size, when it changed…",
                "ui.toolbar_menu",
            )
            .args(serde_json::json!({ "kind": GRID_CAPTIONS })),
            "Under names…",
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
    let mut parts = vec![
        MenuPart::Title("Views".into(), Some(buttons)),
        MenuPart::Tiles(views),
        MenuPart::Line,
        MenuPart::Choice("Sort by".into(), direction),
    ];
    parts.extend(order);
    parts.push(MenuPart::Line);
    parts.extend(show.into_iter().map(MenuPart::Row));
    // Only where the view sizes something (`PaneView::zooms`): smaller, as
    // it was, larger, the keys in the tooltips.
    let zooms = shell.view_zooms();
    if zooms != crate::view::Zooms::Nothing {
        let words = if zooms == crate::view::Zooms::Levels {
            crate::usage::STEPS
        } else {
            [
                ("Larger", "Larger (or Ctrl and the wheel)"),
                ("Smaller", "Smaller (or Ctrl and the wheel)"),
                ("Default size", "Default size"),
            ]
        };
        let [larger, smaller, reset] = words;
        let size = [
            ("out", "minus", None, smaller.1),
            ("reset", "", Some("Default"), reset.1),
            ("in", "plus", None, larger.1),
        ]
        .into_iter()
        .map(|(dir, icon, label, tip)| {
            let button = Button::new(format!("zoom-{dir}"), icon, tip, "ui.zoom")
                .args(serde_json::json!({"dir": dir}))
                .segment();
            match label {
                Some(label) => button.label(label),
                None => button,
            }
        })
        .collect();
        parts.push(MenuPart::Line);
        let name = if zooms == crate::view::Zooms::Levels {
            "Levels"
        } else {
            "Size"
        };
        parts.push(MenuPart::Choice(name.into(), size));
    }
    parts
}

/// What the grid's tiles say under their names: each caption that applies
/// here (`views::grid::CAPTIONS`), checked when chosen. A row keeps the
/// menu open, for the next.
fn grid_captions_menu(shell: &Shell) -> Vec<(&'static str, Vec<Button>)> {
    let said = shell.grid_captions();
    let rows = crate::views::grid::CAPTIONS
        .iter()
        .enumerate()
        .filter(|(_, spec)| shell.caption_applies(spec.caption))
        .map(|(i, spec)| {
            let on = said.contains(&spec.caption);
            // A tooltip is kept as built: it says what the row is about.
            let tip = format!(
                "Whether each tile says its {} under its name",
                spec.label.to_lowercase()
            );
            menu_item(
                Button::new(
                    format!("menu-caption-{i}"),
                    if on { "check" } else { "" },
                    tip,
                    "ui.grid_caption",
                )
                .args(serde_json::json!({ "key": spec.setting }))
                .active(on)
                .keeps_open(),
                spec.label,
            )
        })
        .collect();
    vec![("Under names", rows)]
}

/// Which views have a button beside the address, each checked or not, and
/// the buttons skwir comes with once others were chosen. The view shown
/// keeps its button while it is shown, checked or not. A view's row keeps
/// the menu open, for the next; Default buttons closes it.
fn view_buttons_menu(shell: &Shell) -> Vec<(&'static str, Vec<Button>)> {
    let chosen = &shell.view_buttons;
    let mut rows: Vec<Button> = shell
        .available_views()
        .into_iter()
        .map(|spec| {
            let pinned = chosen.pinned(&spec);
            // The row stays, open, as its check changes: gpui keeps a
            // tooltip as it was built, so it says what the row is about,
            // never what pressing it does now.
            let tip = format!("Whether the {} view has a button", spec.title);
            menu_item(
                Button::new(
                    format!("view-button-{}", spec.id),
                    if pinned { "check" } else { "" },
                    tip,
                    "ui.view_button",
                )
                .args(serde_json::json!({ "id": spec.id, "on": !pinned }))
                .active(pinned)
                .keeps_open(),
                spec.title.clone(),
            )
        })
        .collect();
    if !chosen.is_default() {
        rows.push(menu_item(
            Button::new(
                "view-buttons-reset",
                "revert",
                "Back to the buttons skwir comes with",
                "ui.view_buttons_reset",
            ),
            "Default buttons",
        ));
    }
    vec![("View buttons", rows)]
}

/// More's layout: the panels without a button of their own, checked
/// while open, then where the split's panes, the preview and a file of an
/// archive go, each a setting on one row.
fn layout_parts(shell: &Shell) -> Vec<MenuPart> {
    let mut parts = vec![MenuPart::Row(menu_item(
        Button::new("menu-timeline", "", "Timeline", "ui.timeline").active(shell.timeline.open),
        "Timeline",
    ))];
    // The terminal is a plugin's: without it, nothing says it exists.
    if shell.app.terminals.provider().is_some() {
        parts.push(MenuPart::Row(menu_item(
            Button::new("menu-terminal", "", "Terminal", "ui.terminal").active(shell.terminal.open),
            "Terminal",
        )));
    }
    parts.extend([
        MenuPart::Line,
        MenuPart::Choice("Split".into(), layout_values(shell, "panes")),
        MenuPart::Choice("Preview".into(), layout_values(shell, "preview")),
        MenuPart::Choice("File in an archive".into(), layout_values(shell, "member")),
    ]);
    parts
}

/// The values of one of the layout's settings (`what`: `panes`,
/// `preview`, `member`): Automatic, in words, then the two places as
/// icons. Automatic chosen, the place it gives now is outlined, and its
/// tooltip says why.
fn layout_values(shell: &Shell, what: &'static str) -> Vec<Button> {
    use crate::layout::{Member, Panes, Preview};
    let layout = &shell.layout;
    // Each value: its argument, icon, words, whether it is chosen, and
    // whether Automatic gives it now.
    let (automatic, values): (&str, [(&str, &str, &str, bool, bool); 2]) = match what {
        // Where a file of an archive, or an attachment, is previewed beside
        // the view it came from; Automatic follows the panel's shape.
        "member" => {
            let beside = layout.member.beside(layout.bottom());
            (
                "Automatic: under the archive's files while the preview is at the right, at \
                 their right while it is at the bottom",
                [
                    (
                        "right",
                        "preview",
                        "On the right",
                        layout.member == Member::Right,
                        beside,
                    ),
                    (
                        "bottom",
                        "preview-bottom",
                        "At the bottom",
                        layout.member == Member::Bottom,
                        !beside,
                    ),
                ],
            )
        }
        "panes" => (
            "Automatic: one above the other while the window is taller than wide, else side \
             by side",
            [
                (
                    "row",
                    "split",
                    "Side by side",
                    layout.panes == Panes::Row,
                    !layout.tall(),
                ),
                (
                    "column",
                    "split-rows",
                    "One above the other",
                    layout.panes == Panes::Column,
                    layout.tall(),
                ),
            ],
        ),
        _ => (
            "Automatic: at the bottom while the window is taller than wide, else on the right",
            [
                (
                    "right",
                    "preview",
                    "On the right",
                    layout.preview == Preview::Right,
                    !layout.tall(),
                ),
                (
                    "bottom",
                    "preview-bottom",
                    "At the bottom",
                    layout.preview == Preview::Bottom,
                    layout.tall(),
                ),
            ],
        ),
    };
    let auto = !values.iter().any(|(_, _, _, chosen, _)| *chosen);
    let mut buttons = vec![
        Button::new(format!("layout-{what}-auto"), "", automatic, "ui.layout")
            .args(serde_json::json!({ what: "auto" }))
            .label("Auto")
            .active(auto)
            .segment(),
    ];
    buttons.extend(values.into_iter().map(|(value, icon, words, chosen, now)| {
        Button::new(format!("layout-{what}-{value}"), icon, words, "ui.layout")
            .args(serde_json::json!({ what: value }))
            .active(chosen)
            .now(auto && now)
            .segment()
    }));
    buttons
}

/// A popup's titled groups as its parts, a group with nothing to offer
/// left out, title and all.
fn titled(groups: Vec<(&'static str, Vec<Button>)>) -> Vec<MenuPart> {
    groups
        .into_iter()
        .filter(|(_, items)| !items.is_empty())
        .flat_map(|(title, items)| {
            std::iter::once(MenuPart::Title(title.into(), None))
                .chain(items.into_iter().map(MenuPart::Row))
        })
        .collect()
}

/// The More menu: the panels without a button, the window's layout, New
/// tab, the palette, Settings and About.
fn more_panel(
    shell: &Shell,
    popup: &toolbar::Popup,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let panel = toolbar::menu_panel(popup, "ui.toolbar_menu", theme, cx);
    let mut parts = layout_parts(shell);
    parts.push(MenuPart::Line);
    parts.extend(
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
            (
                Button::new("menu-about", "info", "About skwir", "ui.about"),
                "About skwir",
            ),
        ]
        .into_iter()
        .map(|(button, label)| MenuPart::Row(menu_item(button, label))),
    );
    toolbar::menu_parts(panel, parts, shell, theme, cx).into_any_element()
}

/// The window's own tools have no row of their own: each area owns its
/// top. The sidebar's header holds its toggle and More; a sidebar not
/// drawn leaves them at the start of the row beside it: the tab strip
/// while it shows, else the first pane's row (`Shell::corner_at`). Split
/// and Preview mirror them at the right, in
/// the order of what they open (the middle of the window, then its edge):
/// in the preview's own bar while it is open at the right, else at the end
/// of the tab strip, else of the row of the pane at the window's right
/// (`Shell::toggles_at`). Tabs, when there are two or more (or one
/// asked for, `ui.single_tab`), are a strip above the panes (`tabs.rs`).
/// So a window of one tab has one row above its lists, split or not.
///
/// The sidebar toggle and More, as the sidebar's header, the tab strip or
/// a pane row starts with them. More hangs from `Shell::more_anchor`.
pub fn corner(shell: &Shell, cx: &mut Context<Shell>) -> [AnyElement; 2] {
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
/// `labeled`: each says its name beside its icon, so they are found; each
/// place that holds them offers this as its richest variant, chosen by
/// `AdaptiveRow` while nothing else gives up room for it.
pub fn panel_toggles(shell: &Shell, labeled: bool, cx: &mut Context<Shell>) -> [AnyElement; 2] {
    let (split_icon, split_tip) = shell.layout.split_button();
    let split_tip = shell.split_close_tip().unwrap_or(split_tip);
    let split = Button::new("split-toggle", split_icon, split_tip, "ui.split");
    let preview = Button::new(
        "preview-toggle",
        shell.layout.preview_icon(),
        "Preview panel",
        "ui.toggle_preview",
    );
    let (split, preview) = if labeled {
        (split.label("Split"), preview.label("Preview"))
    } else {
        (split, preview)
    };
    [
        split
            .active(shell.is_split())
            .render(shell, cx)
            .into_any_element(),
        preview
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
    /// A folder, or a location that is none (a tag's, the trash:
    /// `trail.rs`), as a trail of places.
    Trail {
        segments: Vec<Crumb>,
        /// Where the location's own places begin after the folders it is
        /// of, with the glyph that stands for it (`Shown::icon`): `Home ›
        /// 🏷 Tags`, so that its first place never reads as a folder.
        glyph: Option<(usize, &'static str)>,
        /// The last place holds places, which a chevron after it lists
        /// (`Trail::holds_places`; a folder's folders).
        holds_places: bool,
    },
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
    location: Option<skwir_core::Location>,
    /// The View menu or the view buttons' menu, open from this row.
    menu_open: Option<&'static str>,
    /// The menu of places open from this row, if one is.
    nav: Option<crate::nav_menu::NavWhat>,
    /// The window's tools this row holds (see `corner`).
    ends: Ends,
    /// The location's own buttons (`Verbs::tools`), each with the pane it
    /// is run with, `None` while it cannot be pressed (`tool_pane`).
    tools: Vec<(Tool, Option<PaneId>)>,
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
/// Variants chosen in prepaint by `AdaptiveRow`: the view buttons while
/// the address keeps `FIELD_MIN`, else the one button that opens the View
/// menu; a row holding Split first tries it with its label. The address counts as `FIELD_MIN` when a variant is measured
/// (its content has no width of its own) and takes the rest when drawn.
pub fn render_address(shell: &Shell, window: &Window, cx: &mut Context<Shell>) -> AnyElement {
    let theme = cx.global::<Theme>().clone();
    let roots = &shell.roots.roots;
    let row = {
        let ws = shell.app.workspace.read();
        let pane = ws.pane(shell.pane);
        let location = pane.and_then(|p| p.location()).cloned();
        let active = ws.active_id() == shell.pane;
        let focused = shell.ui().address.read(cx).is_focused(window);
        PaneRow {
            back: pane.is_some_and(|p| p.history_pos > 0),
            forward: pane.is_some_and(|p| p.history_pos + 1 < p.history.len()),
            // Every location as a trail (a folder's from the sidebar's
            // place that holds it); a search as the trail of the folder it
            // looks in, its pills after it (ADR-098).
            address: match &shell.ui().place.trail {
                _ if focused => Address::Text,
                Some(trail) => {
                    let mut segments = crate::trail::segments(trail, roots);
                    let search = shell.ui().place.search;
                    let own = segments.len() - trail.crumbs.len();
                    if search {
                        segments.truncate(segments.len() - 1);
                    }
                    Address::Trail {
                        glyph: shell
                            .ui()
                            .place
                            .icon
                            .filter(|_| !trail.crumbs.is_empty() && !search)
                            .map(|icon| (own, icon)),
                        segments,
                        holds_places: trail.holds_places && !search,
                    }
                }
                None => Address::Text,
            },
            edge: if focused {
                theme.accent
            } else if active {
                theme.border
            } else {
                theme.surface
            },
            location,
            // The View menu hangs from the active pane's button: the pane it
            // acts on.
            menu_open: shell
                .toolbar_menu
                .as_ref()
                .map(|menu| menu.kind)
                .filter(|kind| {
                    active && [VIEW_OPTIONS, VIEW_BUTTONS, GRID_CAPTIONS].contains(kind)
                }),
            nav: shell
                .nav_menu
                .as_ref()
                .filter(|menu| menu.pane == shell.pane)
                .map(|menu| menu.what.clone()),
            ends: Ends {
                corner: shell.corner_at() == crate::shell::CornerAt::Pane(shell.pane),
                toggles: shell.toggles_at() == crate::shell::TogglesAt::Pane(shell.pane),
            },
            tools: {
                let listed = pane.is_some_and(|p| !p.listing.is_empty());
                let other = shell
                    .other_pane()
                    .map(|other| (other, ws.pane(other).and_then(|p| p.location())));
                shell
                    .ui()
                    .place
                    .verbs
                    .tools
                    .iter()
                    .map(|tool| {
                        (
                            tool.clone(),
                            tool_pane(tool.pane, shell.pane, listed, other),
                        )
                    })
                    .collect()
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
            // The toggles' labels go first, the view buttons next.
            variants: [(true, true), (true, false), (false, false)]
                .into_iter()
                .filter(|&(_, labeled)| !labeled || row.ends.toggles)
                .map(|(wide, labeled)| {
                    let anchors = [&anchor, &nav_anchor, &chip_anchor, &omni_anchor];
                    pane_row(shell, &row, wide, labeled, anchors, cx)
                })
                .collect(),
            height: PANE_ROW_HEIGHT - 1.,
            chosen: None,
        });
    if let Some(kind) = row.menu_open
        && let Some(popup) = &shell.toolbar_menu
    {
        let mut panel = toolbar::menu_panel(popup, "ui.toolbar_menu", &theme, cx);
        let parts = match kind {
            VIEW_BUTTONS => titled(view_buttons_menu(shell)),
            GRID_CAPTIONS => titled(grid_captions_menu(shell)),
            _ => view_options(shell),
        };
        panel = toolbar::menu_parts(panel, parts, shell, &theme, cx);
        // Which views have a button hangs from the buttons' right edge,
        // which stays where it is as buttons come and go under it.
        bar = bar.child(if kind == VIEW_BUTTONS {
            anchor.popup_right(panel)
        } else {
            anchor.popup(panel)
        });
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

/// Back or Forward: its right button, or a long press as in a browser,
/// lists the history that way.
fn history_button(
    shell: &Shell,
    row: &PaneRow,
    forward: bool,
    anchor: &PopoverAnchor,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let (id, title, command, enabled) = if forward {
        (
            "forward",
            "Forward · hold or right-click for the history",
            "forward",
            row.forward,
        )
    } else {
        (
            "back",
            "Back · hold or right-click for the history",
            "back",
            row.back,
        )
    };
    let open = row.nav == Some(crate::nav_menu::NavWhat::History { forward });
    let pane = shell.pane;
    Button::new(id, id, title, command)
        .on_hold("ui.history_menu", serde_json::json!({ "forward": forward }))
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
    labeled: bool,
    [anchor, nav_anchor, chip_anchor, omni_anchor]: [&PopoverAnchor; 4],
    cx: &mut Context<Shell>,
) -> AnyElement {
    let theme = cx.global::<Theme>().clone();
    let pane = shell.pane;
    // A location reads as a trail of places, a search's or what narrows a
    // folder as pills after it; the text field takes over while it is
    // being edited. No width of its own: the field's minimum is what a
    // variant is measured with.
    let address = match &row.address {
        // The pills follow the trail; the room after them edits the
        // address.
        Address::Trail {
            segments,
            glyph,
            holds_places,
        } => div()
            .flex_1()
            .w(px(0.))
            .min_w_0()
            .h_full()
            .overflow_hidden()
            .flex()
            .items_center()
            .child(
                div()
                    .id(("address-trail", pane.0 as usize))
                    .flex_initial()
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
                        segments,
                        *glyph,
                        *holds_places,
                        row.nav.as_ref(),
                        nav_anchor,
                        &theme,
                        cx,
                    )),
            )
            .children(crate::search_bar::after_trail(shell, chip_anchor, cx))
            .into_any_element(),
        // While a search is typed, where it looks stands before the text.
        // Over the field's card (`omnibox::dropdown`), the text draws above
        // the card, where it is typed.
        Address::Text => {
            let typed = div()
                .flex_1()
                .w(px(0.))
                .min_w_0()
                .h_full()
                .flex()
                .items_center()
                .children(crate::omnibox::scope_token(shell, cx))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .px_2()
                        .child(shell.ui().address.clone()),
                );
            if shell.omnibox_card() {
                gpui::deferred(typed)
                    .with_priority(crate::omnibox::CARD_PRIORITY + 1)
                    .into_any_element()
            } else {
                typed.into_any_element()
            }
        }
    };
    // Tab completes while there is something to complete (`completes`).
    let mut context = gpui::KeyContext::default();
    context.add(keymap::ADDRESS);
    if shell.omnibox_completes(cx) {
        context.add("completes");
    }
    let mut field = div()
        .id(("address-field", pane.0 as usize))
        .flex_1()
        .min_w(px(FIELD_MIN))
        .h(px(FIELD_HEIGHT))
        .flex()
        .items_center()
        .rounded_md()
        .border_1()
        .border_color(row.edge)
        .bg(theme.bg)
        .key_context(context)
        .child(address)
        // The field's card stands around all of it (`popup_around`).
        .child(omni_anchor.marker());
    // Search, as Ctrl+F: the field is where words search (ADR-098), and
    // nothing else in sight says so. Offered where Ctrl+F has a folder, a
    // tag or Recent to look in, or a search to edit, while something
    // searches; gone while the field is typed in, its token wearing the
    // same magnifier.
    if let Some(title) = search_title(shell, row) {
        field = field.child(
            Button::new(format!("search-{}", pane.0), "search", title, "ui.search")
                .render(shell, cx)
                .h(px(24.))
                .min_w(px(24.))
                .px(px(4.))
                .mr(px(2.)),
        );
    }
    // Keep what the pane shows in the sidebar, a folder as a bookmark or a
    // search as a saved search, while a plugin keeps such locations:
    // filled when it is kept, and pressing it then takes it back with the
    // place's own command. It is at the field's end: it is about what the
    // field shows.
    if let Some(location) = &row.location {
        let registry = &shell.app.registry;
        // Offered only where its keeper keeps what is shown (`keep`).
        let keeper = registry.keepers.find(|k| k.keeps(location));
        let kept = registry
            .places
            .find(|p| p.forget.is_some() && location.same_place(&p.location));
        let button = match (kept, keeper) {
            (Some(place), keeper) => {
                let forget = place.forget.expect("kept");
                let title = keeper.map_or_else(|| "Remove from sidebar".into(), |k| k.forget_title);
                Some(
                    Button::new("keep", "bookmark-filled", title, forget.name.to_string())
                        .args(forget.args),
                )
            }
            (None, Some(keeper)) => {
                Some(Button::new("keep", "bookmark", keeper.save_title, "keep"))
            }
            (None, None) => None,
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
    // Another way to see the folder shown (`Aspect`: its tags), offered on
    // the folder and lit on its own locations, from which its command goes
    // back to the folder. Last, at the field's edge, so that it stays where
    // it was pressed whether or not the keep button is offered.
    if let Some(location) = &row.location {
        for (i, aspect) in shell.app.registry.aspects.all().into_iter().enumerate() {
            let lit = *aspect.scheme == *location.scheme;
            if !lit && !location.is_path() {
                continue;
            }
            let id = gpui::SharedString::from(format!("aspect-{}-{i}", pane.0));
            let title = if lit {
                aspect.close_title
            } else {
                aspect.open_title
            };
            let button = Button::new(id, aspect.icon, title, aspect.command.name.to_string())
                .args(aspect.command.args)
                .active(lit);
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
    for (i, (tool, pane)) in row.tools.iter().enumerate() {
        let mut args = tool.command.args.clone();
        if let (Some(args), Some(pane)) = (args.as_object_mut(), pane) {
            args.insert("pane".into(), serde_json::json!(pane));
        }
        line = line.child(
            Button::new(
                format!("location-tool-{}-{i}", shell.pane.0),
                tool.icon,
                tool.title.clone(),
                tool.command.name.to_string(),
            )
            .args(args)
            .enabled(pane.is_some())
            .render(shell, cx),
        );
    }
    if row.ends.toggles {
        let [split, preview] = panel_toggles(shell, labeled, cx);
        line = line
            .child(toolbar::separator(&theme))
            .child(split)
            .child(preview);
    }
    line.into_any_element()
}

/// What the field's Search button says, as Ctrl+F does it: a search shown
/// edited, a folder searched with its subfolders, a tag or Recent
/// searched. `None` while the field is typed in, where words cannot look
/// (the trash), or while nothing searches.
fn search_title(shell: &Shell, row: &PaneRow) -> Option<String> {
    if !matches!(row.address, Address::Trail { .. })
        || shell.app.commands.get("query.search").is_none()
    {
        return None;
    }
    if !shell.ui().chips.chips.is_empty() {
        return Some("Edit this search".to_owned());
    }
    let location = row.location.as_ref()?;
    if location.is_path() {
        return Some("Search this folder and its subfolders".to_owned());
    }
    crate::suggestions::scope_of(location)
        .map(|scope| format!("Search {}", shell.app.location_title(&scope)))
}

/// The pane a location's tool is run with (`ToolPane`), or `None` while
/// it cannot be pressed: the location `shown` lists nothing for it to act
/// on, or it puts files in the other pane's folder and there is none (no
/// split, or the `other` pane shows the shelf too, a search, a tag).
fn tool_pane(
    to: ToolPane,
    shown: PaneId,
    listed: bool,
    other: Option<(PaneId, Option<&Location>)>,
) -> Option<PaneId> {
    let pane = match to {
        ToolPane::Shown => Some(shown),
        ToolPane::OtherFolder => other
            .filter(|(_, location)| location.is_some_and(Location::is_path))
            .map(|(other, _)| other),
    };
    pane.filter(|_| listed)
}

/// The folders leading to `path`, outermost first, each with the path it
/// opens. The deepest of the sidebar's folders that holds it
/// (`sidebar::roots`, the home folder among them) stands for everything
/// above it, under the sidebar's name: `Papers › 2026` for a folder of
/// Documents the user named Papers, `stick › Photos` on a drive. Of two
/// on one folder, the first in the sidebar. With no roots (`ui.full_path`)
/// it starts at `/`.
pub fn crumbs(path: &std::path::Path, roots: &[crate::sidebar::Root]) -> Vec<(String, PathBuf)> {
    let root = roots
        .iter()
        .filter(|root| path.starts_with(&root.path))
        .min_by_key(|root| std::cmp::Reverse(root.path.components().count()));
    let (mut out, mut current, rest) = match root {
        Some(root) => (
            vec![(root.name.clone(), root.path.clone())],
            root.path.clone(),
            path.strip_prefix(&root.path).unwrap_or(path),
        ),
        None => (Vec::new(), PathBuf::new(), path),
    };
    let mut components = rest.components().peekable();
    while let Some(component) = components.next() {
        current.push(component);
        let label = match component {
            std::path::Component::RootDir => "/".to_owned(),
            // A drive or a share and its root are one segment, named as
            // Windows names the drive: `C:`, `\\server\share`.
            std::path::Component::Prefix(prefix) => {
                if let Some(root) =
                    components.next_if(|c| matches!(c, std::path::Component::RootDir))
                {
                    current.push(root);
                }
                prefix.as_os_str().to_string_lossy().into_owned()
            }
            other => other.as_os_str().to_string_lossy().into_owned(),
        };
        out.push((label, current.clone()));
    }
    out
}

/// The trail, laid out by flexbox alone: the segments are given last first to
/// a right-to-left wrapping line one row tall, so the ones nearest the current
/// place stay and the outermost wrap out of sight in a narrow pane, with
/// nothing measured. Every segment but the outermost carries its chevron:
/// a trail that starts with one visibly continues to the left.
///
/// A chevron lists the places inside the segment before it (a folder's
/// folders, a tag's tags), the one the trail goes on into marked; one more
/// after the last lists those inside the place shown, when it `holds`
/// places (not the trash's files). The outermost has one too when the
/// trail leaves folders out above it (it starts at a folder of the
/// sidebar, Home or a drive, not at `/`), which lists them from the root
/// down (`ui.parents_menu`), and wraps out of sight with it. The first
/// segment a narrow pane leaves in sight then has its chevron list what
/// is above it instead, the segments wrapped out included: which is first
/// is the layout's answer (`PaneUi::trail_shown`), read when the chevron
/// is pointed at or pressed, never here. `open` is the menu of places
/// open from this row, whose chevron hangs it from `anchor`. Plain text
/// is a folder: the segment `glyph` names carries the location's icon,
/// and one standing for a tag (`Crumb::tag`) is the tag's chip.
fn breadcrumbs(
    shell: &Shell,
    crumbs: &[Crumb],
    glyph: Option<(usize, &'static str)>,
    holds: bool,
    open: Option<&crate::nav_menu::NavWhat>,
    anchor: &PopoverAnchor,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let pane = shell.pane;
    let last = crumbs.len().saturating_sub(1);
    let in_sight = shell.ui().trail_shown.clone();
    let laid_out = in_sight.clone();
    let mut trail = div()
        // The segments are its children last first: those on its first
        // line are in sight, the rest wrapped below, out of it.
        .on_children_prepainted(move |bounds, _, _| {
            let top = bounds.first().map(|b| b.origin.y);
            let shown = bounds
                .iter()
                .take_while(|b| Some(b.origin.y) == top)
                .count();
            laid_out.set(bounds.len() - shown);
        })
        .id(("trail", pane.0 as usize))
        .role(gpui::Role::Navigation)
        .aria_label("Path")
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
    // `index` names it among the row's: the segment it stands before, and
    // one past the last for the chevron after it.
    // `above`, for a segment's own chevron: what it lists instead while
    // the segments before it are out of sight (`ui.parents_menu`).
    let chevron = |index: usize,
                   menu: &'static str,
                   what: crate::nav_menu::NavWhat,
                   args: serde_json::Value,
                   tip: gpui::SharedString,
                   above: Option<(
        crate::nav_menu::NavWhat,
        serde_json::Value,
        gpui::SharedString,
    )>,
                   cx: &mut Context<Shell>| {
        let shown = open == Some(&what) || above.as_ref().is_some_and(|(a, ..)| open == Some(a));
        let hover = theme.hover;
        let hidden = {
            let in_sight = in_sight.clone();
            move || in_sight.get() == index
        };
        let (tip_above, click_above) = (above.clone(), above);
        let hidden_at_click = hidden.clone();
        div()
            // Known by what it lists, as its tooltip says.
            .id(crate::elements::known_by("crumb-folders", &what))
            // What it lists, as its tooltip says.
            .role(gpui::Role::Button)
            .when(shell.a11y, |d| d.aria_label(tip.clone()))
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
            .tooltip(move |_, cx| {
                let tip = match &tip_above {
                    Some((_, _, above)) if hidden() => above.clone(),
                    _ => tip.clone(),
                };
                toolbar::tooltip(tip, cx)
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.activate_pane(pane, cx);
                match &click_above {
                    Some((_, above, _)) if hidden_at_click() => {
                        this.run("ui.parents_menu", above.clone(), window, cx);
                    }
                    _ => this.run(menu, args.clone(), window, cx),
                }
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
    // The places inside `of`, `current` the one the trail goes on into
    // (whose chevron it is, listing what is above it while those before
    // are out of sight).
    let inside = |index: usize, of: &Crumb, current: Option<&Crumb>, cx: &mut Context<Shell>| {
        let above = current.map(|crumb| {
            (
                crate::nav_menu::NavWhat::Parents {
                    of: crumb.location.clone(),
                },
                serde_json::json!({ "location": crumb.location.arg() }),
                gpui::SharedString::from(if crumb.location.is_path() {
                    format!("Folders above {}", crumb.label)
                } else {
                    format!("Above {}", crumb.label)
                }),
            )
        });
        chevron(
            index,
            "ui.subfolder_menu",
            crate::nav_menu::NavWhat::Subfolders {
                of: of.location.clone(),
            },
            serde_json::json!({
                "location": serde_json::json!(of.location.arg()),
                "current": current.map(|crumb| crumb.location.arg()),
            }),
            // A folder's menu lists the places of it too (its tags).
            format!("In {}", of.label).into(),
            above,
            cx,
        )
    };
    // The folders above the first, when the trail leaves them out.
    // How many nodes each segment's group gives the tree, in the row's
    // order: its chevron before it, itself, the one after the last.
    let mut groups = Vec::with_capacity(crumbs.len());
    let above = crumbs.first().and_then(|first| {
        let path = first.location.path()?;
        path.parent().is_some().then(|| (first.label.clone(), path))
    });
    for (index, crumb) in crumbs.iter().enumerate().rev() {
        let hover = theme.hover;
        let open = serde_json::json!(crumb.location.arg());
        let in_tab = open.clone();
        let (text, clip) = crate::elements::clip::text(crumb.label.clone());
        let ink = if index == last {
            theme.text
        } else {
            theme.text_muted
        };
        let icon = glyph.filter(|(at, _)| *at == index).map(|(_, icon)| icon);
        // A link to the place it names; the last is where the pane is.
        let segment = div()
            .id(crate::elements::known_by("crumb", &crumb.location))
            .role(gpui::Role::Link)
            .when(shell.a11y, |d| {
                d.aria_label(crumb.label.clone())
                    .when(index == last, |d| d.aria_selected(true))
            })
            .optional_tooltip(clip.tooltip(None))
            .max_w(px(260.))
            .px(px(6.))
            .flex()
            .items_center()
            .gap(px(4.))
            .rounded_sm()
            .cursor_pointer()
            .text_color(ink)
            .map(|d| match crumb.tag.as_ref().map(|tag| tag.name.as_str()) {
                // A tag is its chip, as on a file's row (its dot, its
                // tint), the one shown filled deeper as in the sidebar:
                // never a folder's name, even when the folders before it
                // are out of sight.
                Some(tag) => {
                    let color = shell.tag_colors.of(tag);
                    let tone = theme.tag(color);
                    d.child(crate::tags::dot(color, theme).size(px(7.)))
                        .h(px(20.))
                        .line_height(px(20.))
                        .border_1()
                        .border_color(gpui::transparent_black())
                        .bg(if index == last {
                            theme.tag_shown(color)
                        } else {
                            theme.tag_fill(color)
                        })
                        .text_color(theme.text)
                        .hover(move |s| s.border_color(tone))
                }
                None => d.h(px(24.)).hover(move |s| s.bg(hover)),
            })
            .children(icon.map(|icon| {
                gpui::svg()
                    .path(format!("toolbar/{icon}.svg"))
                    .size(px(14.))
                    .flex_none()
                    .text_color(ink)
            }))
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
            // The middle button opens the place in a tab behind this one.
            .on_mouse_down(
                gpui::MouseButton::Middle,
                cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.run(
                        "tab.new",
                        serde_json::json!({
                            "location": in_tab,
                            "show": false,
                            "pane": pane,
                        }),
                        window,
                        cx,
                    );
                }),
            )
            .child(div().min_w_0().truncate().child(text));
        let segment = match crumb.location.path() {
            Some(dir) => crate::dnd::droppable_dir(segment, dir, cx),
            None if shell.ui().place.takes_drops(&crumb.location) => {
                crate::dnd::droppable_place(segment, crumb.location.clone(), cx)
            }
            None => segment,
        };
        let before = match index.checked_sub(1) {
            Some(parent) => Some(inside(index, &crumbs[parent], Some(crumb), cx)),
            None => above.as_ref().map(|(label, path)| {
                chevron(
                    index,
                    "ui.parents_menu",
                    crate::nav_menu::NavWhat::Parents {
                        of: Location::from_path(path),
                    },
                    serde_json::json!({ "location": skwir_core::os_path::PathArg::from(path.as_path()) }),
                    format!("Folders above {label}").into(),
                    None,
                    cx,
                )
            }),
        };
        let after = (index == last && holds).then(|| inside(index + 1, crumb, None, cx));
        groups.push(usize::from(before.is_some()) + 1 + usize::from(after.is_some()));
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
    // To assistive technology, the path as it reads: the row is given its
    // segments from the last, laid out from the right; the tree takes them
    // back in reading order, the first folder first, each with its
    // chevrons. Left as they came should the count not match.
    if shell.a11y {
        trail = trail.a11y_synthetic_children(move |builder| {
            let path = builder.parent_node();
            let given = path.children().to_vec();
            if groups.iter().sum::<usize>() != given.len() {
                return;
            }
            let mut reading = Vec::with_capacity(given.len());
            let mut end = given.len();
            // The last given is the first read.
            for size in groups.iter().rev() {
                reading.extend_from_slice(&given[end - size..end]);
                end -= size;
            }
            path.set_children(reading);
        });
    }
    trail.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{ViewButtons, ViewSpec, crumbs, tool_pane, toolbar_views};
    use crate::sidebar::Root;
    use skwir_core::location::{Location, ToolPane};
    use skwir_core::workspace::PaneId;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    /// The shelf's Move and Copy go to the other pane's folder: not
    /// without a split, nor when the other pane shows the shelf too; the
    /// trash's Empty runs in its own pane; neither with nothing listed.
    #[test]
    fn a_location_tool_is_pressed_only_where_it_can_act() {
        let (shown, other) = (PaneId(1), PaneId(2));
        let folder = Location::from_path("/x/dest");
        let shelf = Location::parse("shelf:");
        let to_other = |listed, beside: Option<&Location>| {
            tool_pane(
                ToolPane::OtherFolder,
                shown,
                listed,
                beside.map(|l| (other, Some(l))),
            )
        };
        assert_eq!(to_other(true, Some(&folder)), Some(other));
        assert_eq!(to_other(true, None), None, "no split");
        assert_eq!(to_other(true, Some(&shelf)), None, "both show the shelf");
        assert_eq!(to_other(false, Some(&folder)), None, "nothing to move");
        assert_eq!(tool_pane(ToolPane::Shown, shown, true, None), Some(shown));
        assert_eq!(tool_pane(ToolPane::Shown, shown, false, None), None);
    }

    fn views() -> Vec<ViewSpec> {
        vec![
            ViewSpec::named("list", "List", "list", Arc::default()),
            ViewSpec::named("tree", "Tree", "tree", Arc::default()).alternate(),
            ViewSpec::named("grid", "Grid", "grid", Arc::default()),
            ViewSpec::named("usage", "Usage", "usage", Arc::default()).lens(),
        ]
    }

    fn shown_with(current: &str, chosen: &ViewButtons) -> Vec<String> {
        toolbar_views(views(), current, chosen)
            .into_iter()
            .map(|spec| spec.id.to_string())
            .collect()
    }

    fn shown(current: &str) -> Vec<String> {
        shown_with(current, &ViewButtons::default())
    }

    fn spec(id: &str) -> ViewSpec {
        views().into_iter().find(|s| &*s.id == id).expect("a view")
    }

    #[test]
    fn a_lens_or_an_alternate_has_a_button_only_while_it_is_shown() {
        assert_eq!(shown("grid"), ["list", "grid"]);
        assert_eq!(shown("usage"), ["list", "grid", "usage"]);
        assert_eq!(shown("tree"), ["list", "tree", "grid"]);
    }

    /// Given a button, a lens keeps it; taken away, a listing's button
    /// stays only while that view is shown, in the views' own order.
    #[test]
    fn the_buttons_chosen_replace_what_the_roles_give() {
        let chosen = ViewButtons::default()
            .with(&spec("usage"), true)
            .with(&spec("grid"), false);
        assert_eq!(shown_with("list", &chosen), ["list", "usage"]);
        assert_eq!(shown_with("grid", &chosen), ["list", "grid", "usage"]);
        let none = ViewButtons::default()
            .with(&spec("list"), false)
            .with(&spec("grid"), false);
        assert_eq!(
            shown_with("tree", &none),
            ["tree"],
            "the view shown keeps one"
        );
    }

    /// A choice that is the role's is not kept: choosing back what skwir
    /// gives is Default buttons, and a view added later comes with its own.
    #[test]
    fn a_choice_back_to_the_role_is_forgotten() {
        let grid = spec("grid");
        let chosen = ViewButtons::default().with(&grid, false);
        assert!(!chosen.pinned(&grid));
        assert_eq!(chosen.value().to_string(), "{ grid = false }");
        let back = chosen.with(&grid, true);
        assert!(back.is_default() && back.pinned(&grid));
        let usage = spec("usage");
        assert!(!ViewButtons::default().with(&usage, false).pinned(&usage));
        assert!(ViewButtons::default().with(&usage, false).is_default());
    }

    fn root(path: &str, name: &str) -> Root {
        Root {
            path: path.into(),
            name: name.into(),
        }
    }

    fn labels(path: &str, roots: &[Root]) -> Vec<String> {
        crumbs(Path::new(path), roots)
            .into_iter()
            .map(|(label, _)| label)
            .collect()
    }

    #[test]
    fn home_stands_for_everything_above_it() {
        let home = [root("/home/example", "Home")];
        assert_eq!(
            labels("/home/example/Projects/it's a report", &home),
            ["Home", "Projects", "it's a report"]
        );
        assert_eq!(labels("/home/example", &home), ["Home"]);
        assert_eq!(labels("/srv/data", &home), ["/", "srv", "data"]);
        assert_eq!(labels("/", &[]), ["/"]);
        // A sibling that merely shares a prefix is not inside the home folder.
        assert_eq!(labels("/home/example2", &home), ["/", "home", "example2"]);
    }

    /// The deepest of the sidebar's folders holding a folder starts its
    /// trail, under the sidebar's name; of two on one folder, the first.
    #[test]
    fn a_trail_starts_at_the_deepest_place_of_the_sidebar_under_its_name() {
        let roots = [
            root("/home/example", "Home"),
            root("/home/example/Documents", "Papers"),
            root("/run/media/example/stick", "stick"),
            root("/", "Root"),
            root("/home/example/Documents", "Docs too"),
        ];
        assert_eq!(
            labels("/home/example/Documents/2026/taxes", &roots),
            ["Papers", "2026", "taxes"]
        );
        assert_eq!(labels("/home/example/Music", &roots), ["Home", "Music"]);
        assert_eq!(
            labels("/run/media/example/stick/Photos", &roots),
            ["stick", "Photos"]
        );
        assert_eq!(labels("/etc/fonts", &roots), ["Root", "etc", "fonts"]);
        let trail = crumbs(Path::new("/home/example/Documents/2026"), &roots);
        assert_eq!(trail[0].1, PathBuf::from("/home/example/Documents"));
    }

    #[test]
    fn every_segment_opens_its_own_folder() {
        let trail = crumbs(
            Path::new("/home/example/a/b"),
            &[root("/home/example", "Home")],
        );
        assert_eq!(trail[0].1, PathBuf::from("/home/example"));
        assert_eq!(trail[2].1, PathBuf::from("/home/example/a/b"));
    }
}
