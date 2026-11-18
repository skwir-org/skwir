//! Each pane's row (navigation, the address, how the pane shows its
//! folder), the sidebar's header, and where the window's own tools go
//! among them (see `corner`).
use crate::sides::Sides;
use crate::sizes::sized;
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
use skwir_core::words;
use skwir_core::words::{Folder, Words};
use skwir_core::workspace::PaneId;
use std::path::PathBuf;

/// What the address field keeps before the view buttons fold into one.
const FIELD_MIN: f32 = 180.;
/// The address field's height, which the field's card leaves for it.
pub(crate) const FIELD_HEIGHT: f32 = 30.;
/// The line the address is written on, as a trail or as text: one height,
/// so its words stay at their level when the field takes over.
const ADDRESS_LINE: f32 = 24.;
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
            Some(crate::map_view::BACKGROUNDS) => crate::map_view::BACKGROUNDS,
            Some(crate::dock::MENU) => crate::dock::MENU,
            Some(crate::dock::VOLUME) => crate::dock::VOLUME,
            Some(crate::timeline::DATES_MENU) => crate::timeline::DATES_MENU,
            _ => MORE,
        };
        if self.toggle_popup(kind, args, window, cx) {
            let mut popup = toolbar::Popup::new(kind, window, cx);
            if kind == VIEW_OPTIONS {
                // The focus of the rows whose submenus open beside them, in
                // the popup's tab order as its other rows.
                popup.sub.parents = [SORT_BY, GROUP_BY, UNDER_NAMES]
                    .into_iter()
                    .map(|name| (name.into(), cx.focus_handle().tab_stop(true).tab_index(0)))
                    .collect();
            }
            self.toolbar_menu = Some(popup);
            if kind == crate::dock::VOLUME {
                self.focus_volume(window, cx);
            }
        }
        cx.notify();
    }
}

pub(crate) const VIEW_OPTIONS: &str = "view_options";
/// The View menu's submenus, by name: what to sort by, what to group by
/// (ADR-130). Each is called by its own words where it is drawn
/// (`submenu_label`).
const SORT_BY: &str = "Sort by";
const GROUP_BY: &str = "Group by";
/// What the grid's tiles say under their names, a submenu as the sort's.
const UNDER_NAMES: &str = "Under names";

/// What the View menu's submenu `name` is called.
fn submenu_label(name: &str) -> gpui::SharedString {
    match name {
        SORT_BY => words!(
            address_sort_by_menu = "Sort by",
            note = "A row of the View menu opening the sort's choices beside it; what it is set to follows (Name, A to Z)."
        ),
        GROUP_BY => words!(
            address_group_by_menu = "Group by",
            note = "A row of the View menu opening the choices of how the files are grouped beside it."
        ),
        _ => words!(
            address_under_names_menu = "Under names",
            note = "A row of the View menu, in the grid: what each tile says under its name (its size, its date)."
        ),
    }
    .say()
    .into()
}
/// The pane row's buttons (which views have one, whether Up is there):
/// the view buttons' right-click, the Menu key on one, the same on the one
/// button that stands for them, or the View menu's Buttons row.
const VIEW_BUTTONS: &str = "view_buttons";
/// What the grid's tiles say under their names: the View menu's Under
/// names row, in the grid.
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
        keymap::view_title(spec.title.clone()),
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
    holes: &crate::elements::ground::Holes,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let choosing = open == Some(VIEW_BUTTONS);
    let menu = |id: &'static str, icon: gpui::SharedString, title: Words| {
        Button::new(id, icon, title, "ui.toolbar_menu")
            .args(serde_json::json!({"kind": VIEW_OPTIONS}))
            .anchor_if(anchor, !(wide && choosing))
            .active(open == Some(VIEW_OPTIONS))
            .dropdown()
    };
    let views = shell.available_views();
    let current = shell.ui().view_id;
    let pane = shell.pane;
    // Right-clicked, or the Menu key on one of them, the buttons say which
    // views have one, as the list's header says which columns it shows;
    // the one button that stands for them says it too. Taken on its way
    // in: a button takes the keyboard on any press before its row hears
    // of it, and the popup must take it from what had it, not from the
    // button.
    let choose = cx.listener(move |this, ev: &gpui::MouseDownEvent, window, cx| {
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
    });
    if !wide {
        let shown = views.iter().find(|s| &*s.id == current);
        return div()
            .key_context(keymap::VIEW_BUTTONS)
            .flex_none()
            .capture_any_mouse_down(choose)
            .child(
                menu(
                    "view-menu",
                    shown.map_or_else(|| "list".into(), view_icon),
                    match shown {
                        Some(spec) => words!(
                            address_view_menu_of = "{ $view } view, sort, hidden files and zoom",
                            view = spec.title.clone(),
                            note = "The tooltip of the button that opens the View menu where the pane's row is narrow; view: the view shown (List, Grid)."
                        ),
                        None => words!(
                            address_view_menu_views = "Views, sort, hidden files and zoom",
                            note = "The tooltip of the button that opens the View menu."
                        ),
                    },
                )
                .render(shell, cx),
            )
            .into_any_element();
    }
    let theme = cx.global::<Theme>().clone();
    let mut group = div()
        .id(("view-buttons", pane.0 as usize))
        .key_context(keymap::VIEW_BUTTONS)
        .relative()
        .flex()
        .flex_none()
        .gap(px(1.))
        .p(sized(2.))
        .rounded_lg()
        .border_1()
        .border_color(theme.border)
        // A part of the row, as the field is.
        .bg(theme.bg.opacity(theme.opacity.interface))
        .capture_any_mouse_down(choose)
        .when(choosing, |d| d.child(anchor.marker()));
    for spec in toolbar_views(views, current, &shell.view_buttons) {
        group = group.child(
            view_button(shell, &spec, "view")
                .render(shell, cx)
                .h(sized(24.)),
        );
    }
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(sized(2.))
        .child(crate::elements::ground::hole(
            holes,
            gpui::rems(0.5),
            theme.opacity.interface,
            group,
        ))
        .child(
            menu(
                "view-options",
                "sliders".into(),
                words!(
                    address_view_menu = "Sort, hidden files and zoom",
                    note = "The tooltip of the button beside the view buttons that opens the View menu."
                ),
            )
            .render(shell, cx),
        )
        .into_any_element()
}

/// A menu row: the shared button, full width, with its state as a check.
fn menu_item(button: Button, label: impl Into<Words>) -> Button {
    button.label(label).menu()
}

/// A direction of `key` for the View menu's submenus, each in the key's
/// words (A to Z, Largest), `ascending` the one chosen; `enabled` while
/// they can be chosen (Group by while grouped by a criterion of its own).
fn directions(
    key: skwir_core::SortKey,
    ascending: bool,
    command: &str,
    enabled: bool,
) -> Vec<Button> {
    let [(up, _), (down, _)] = key.direction_words();
    [(true, "up", up), (false, "down", down)]
        .into_iter()
        .map(|(going_up, icon, label)| {
            Button::new(
                format!(
                    "{command}-{}",
                    if going_up { "ascending" } else { "descending" }
                ),
                icon,
                direction_said(key, going_up).0,
                command.to_owned(),
            )
            .args(serde_json::json!({ "ascending": going_up }))
            .label(label)
            .active(ascending == going_up)
            .enabled(enabled)
            .segment()
        })
        .collect()
}

/// A direction of `key`, going up or not, as the View menu says it beside
/// its button's word (`SortKey::direction_words`): whole, for the button's
/// tooltip ("Newest first"), and short, in the phrase of the row that
/// opens the submenu ("Modified, newest").
fn direction_said(key: skwir_core::SortKey, up: bool) -> (Words, Words) {
    use skwir_core::SortKey;
    match (key, up) {
        (SortKey::Size | SortKey::Dimensions, true) => (
            words!(address_smallest_first = "Smallest first"),
            words!(
                address_smallest = "smallest",
                note = "A sort's direction after its criterion, in the View menu's row: Size, smallest."
            ),
        ),
        (SortKey::Size | SortKey::Dimensions, false) => (
            words!(address_largest_first = "Largest first"),
            words!(
                address_largest = "largest",
                note = "A sort's direction after its criterion, in the View menu's row: Size, largest."
            ),
        ),
        (SortKey::Duration, true) => (
            words!(address_shortest_first = "Shortest first"),
            words!(
                address_shortest = "shortest",
                note = "A sort's direction after its criterion, in the View menu's row: Duration, shortest."
            ),
        ),
        (SortKey::Duration, false) => (
            words!(address_longest_first = "Longest first"),
            words!(
                address_longest = "longest",
                note = "A sort's direction after its criterion, in the View menu's row: Duration, longest."
            ),
        ),
        (
            SortKey::Modified
            | SortKey::Created
            | SortKey::Accessed
            | SortKey::Deleted
            | SortKey::Taken,
            true,
        ) => (
            words!(address_oldest_first = "Oldest first"),
            words!(
                address_oldest = "oldest",
                note = "A sort's direction after its criterion, in the View menu's row: Modified, oldest."
            ),
        ),
        (
            SortKey::Modified
            | SortKey::Created
            | SortKey::Accessed
            | SortKey::Deleted
            | SortKey::Taken,
            false,
        ) => (
            words!(address_newest_first = "Newest first"),
            words!(
                address_newest = "newest",
                note = "A sort's direction after its criterion, in the View menu's row: Modified, newest."
            ),
        ),
        (SortKey::Rating, true) => (
            words!(address_lowest_first = "Lowest first"),
            words!(
                address_lowest = "lowest",
                note = "A sort's direction after its criterion, in the View menu's row: Rating, lowest."
            ),
        ),
        (SortKey::Rating, false) => (
            words!(address_highest_first = "Highest first"),
            words!(
                address_highest = "highest",
                note = "A sort's direction after its criterion, in the View menu's row: Rating, highest."
            ),
        ),
        (SortKey::Vcs, true) => (
            words!(
                address_changes_first = "Changes first",
                note = "A sort by the files' state in Git: the files changed first."
            ),
            words!(
                address_changes_first_said = "changes first",
                note = "A sort's direction after its criterion, in the View menu's row: Git, Changes first."
            ),
        ),
        (SortKey::Vcs, false) => (
            words!(
                address_changes_last = "Changes last",
                note = "A sort by the files' state in Git: the files changed last."
            ),
            words!(
                address_changes_last_said = "changes last",
                note = "A sort's direction after its criterion, in the View menu's row: Git, Changes last."
            ),
        ),
        (
            SortKey::Name
            | SortKey::Kind
            | SortKey::Type
            | SortKey::Extension
            | SortKey::Tags
            | SortKey::Owner
            | SortKey::Folder,
            true,
        ) => (
            words!(
                address_a_to_z = "A to Z",
                note = "A sort's direction, alphabetical."
            ),
            words!(
                address_a_to_z_said = "A to Z",
                note =
                    "A sort's direction after its criterion, in the View menu's row: Name, A to Z."
            ),
        ),
        (
            SortKey::Name
            | SortKey::Kind
            | SortKey::Type
            | SortKey::Extension
            | SortKey::Tags
            | SortKey::Owner
            | SortKey::Folder,
            false,
        ) => (
            words!(
                address_z_to_a = "Z to A",
                note = "A sort's direction, alphabetical backwards."
            ),
            words!(
                address_z_to_a_said = "Z to A",
                note =
                    "A sort's direction after its criterion, in the View menu's row: Name, Z to A."
            ),
        ),
    }
}

/// What a sort or a group by `key` going `ascending` is, as its row says
/// it: the criterion and the word of its direction's button beside it,
/// "Name, A to Z", "Modified, newest", short enough for the row to keep
/// its name whole.
fn said(key: skwir_core::SortKey, ascending: bool) -> Words {
    words!(
        address_sort_said = "{ $key }, { $direction }",
        key = key.title_words(),
        direction = direction_said(key, ascending).1,
        note = "What the View menu's Sort by and Group by rows are set to; key: the criterion (Name, Modified); direction: its direction (A to Z, newest)."
    )
}

/// What a caption of the grid's tiles says, as its row's tooltip in the
/// View menu: by the criterion that sorts by it.
fn caption_tip(key: skwir_core::SortKey) -> Words {
    use skwir_core::SortKey;
    match key {
        SortKey::Name => {
            words!(address_caption_name = "Whether each tile says its name")
        }
        SortKey::Kind => {
            words!(address_caption_kind = "Whether each tile says its kind under its name")
        }
        SortKey::Type => {
            words!(address_caption_type = "Whether each tile says its type under its name")
        }
        SortKey::Extension => words!(
            address_caption_extension = "Whether each tile says its extension under its name"
        ),
        SortKey::Size => {
            words!(address_caption_size = "Whether each tile says its size under its name")
        }
        SortKey::Modified => words!(
            address_caption_modified = "Whether each tile says under its name when it was modified",
            note = "The tooltip of a row of the grid's Under names: whether tiles say when the file was modified."
        ),
        SortKey::Created => words!(
            address_caption_created = "Whether each tile says under its name when it was created",
            note = "The tooltip of a row of the grid's Under names: whether tiles say when the file was created."
        ),
        SortKey::Accessed => words!(
            address_caption_accessed =
                "Whether each tile says under its name when it was last read",
            note = "The tooltip of a row of the grid's Under names: whether tiles say when the file was last read."
        ),
        SortKey::Deleted => words!(
            address_caption_deleted = "Whether each tile says under its name when it was deleted",
            note = "The tooltip of a row of the grid's Under names, in the trash: whether tiles say when the file was deleted."
        ),
        SortKey::Rating => {
            words!(address_caption_rating = "Whether each tile says its rating under its name")
        }
        SortKey::Tags => {
            words!(address_caption_tags = "Whether each tile says its tags under its name")
        }
        SortKey::Owner => {
            words!(address_caption_owner = "Whether each tile says its owner under its name")
        }
        SortKey::Folder => {
            words!(address_caption_folder = "Whether each tile says its folder under its name")
        }
        SortKey::Taken => words!(
            address_caption_taken = "Whether each tile says under its name when it was taken",
            note = "The tooltip of a row of the grid's Under names: whether tiles say when a picture was taken."
        ),
        SortKey::Dimensions => words!(
            address_caption_dimensions = "Whether each tile says its dimensions under its name"
        ),
        SortKey::Duration => {
            words!(address_caption_duration = "Whether each tile says its duration under its name")
        }
        SortKey::Vcs => words!(
            address_caption_vcs = "Whether each tile says its state in Git under its name",
            note = "The tooltip of a row of the grid's Under names: whether tiles say the file's state in Git."
        ),
    }
}

/// The View menu's submenu `name`: the direction first, in the criterion's
/// words, then every criterion that applies here (`Shell::sort_keys`), the
/// one chosen checked, in groups of words, dates and notes; Group by's
/// after None and Same as sort (ADR-130). Group by's direction is always
/// there, so that nothing moves under the pointer as a choice makes it
/// apply or not: while not grouped by a criterion of its own it cannot be
/// chosen, and Same as sort shows the sort's. The one chosen again keeps
/// its direction.
pub(crate) fn view_submenu(shell: &Shell, name: &str) -> Vec<toolbar::FlyRow> {
    use skwir_core::SortKey;
    use skwir_core::grouping::GroupBy;
    use toolbar::FlyRow;
    let spec = shell.sort_spec();
    let kind = family;
    let mut rows = Vec::new();
    let by = |rows: &mut Vec<FlyRow>, keys: Vec<SortKey>, row: &dyn Fn(SortKey) -> Button| {
        let mut last = None;
        for key in keys {
            if last.is_some_and(|last| last != kind(key)) {
                rows.push(FlyRow::Line);
            }
            last = Some(kind(key));
            rows.push(FlyRow::Row(row(key)));
        }
    };
    match name {
        SORT_BY => {
            rows.push(FlyRow::Choice(
                direction_label(),
                directions(spec.key, spec.ascending, "sort", true),
            ));
            rows.push(FlyRow::Line);
            let keys = shell.sort_keys();
            by(&mut rows, keys, &|key| {
                let current = spec.key == key;
                let args = if current {
                    serde_json::json!({ "key": key, "ascending": spec.ascending })
                } else {
                    serde_json::json!({ "key": key })
                };
                menu_item(
                    Button::new(
                        format!("sort-{key:?}"),
                        if current { "check" } else { "" },
                        words!(
                            address_sort_by = "Sort by { $key }",
                            key = key.phrase_words(),
                            note = "The tooltip of a criterion of the View menu's Sort by; key: the criterion inside a sentence (name, date modified)."
                        ),
                        "sort",
                    )
                    .args(args)
                    .active(current)
                    .keeps_open(),
                    key.title_words(),
                )
            });
        }
        GROUP_BY => {
            let plain = |by: GroupBy, label: Words, tip: Words| {
                let current = spec.group == by;
                menu_item(
                    Button::new(
                        format!("group-{by:?}"),
                        if current { "check" } else { "" },
                        tip,
                        "group",
                    )
                    .args(serde_json::json!({ "by": by }))
                    .active(current)
                    .keeps_open(),
                    label,
                )
            };
            let mut values = match spec.group {
                GroupBy::Key(key) => directions(key, spec.group_ascending, "group", true),
                GroupBy::SameAsSort => directions(spec.key, spec.ascending, "group", false),
                GroupBy::None => directions(spec.key, true, "group", false),
            };
            if spec.group == GroupBy::None {
                for value in &mut values {
                    value.active = false;
                }
            }
            rows.push(FlyRow::Choice(direction_label(), values));
            rows.push(FlyRow::Line);
            rows.push(FlyRow::Row(plain(
                GroupBy::None,
                group_none(),
                words!(address_group_none_tip = "Do not group"),
            )));
            rows.push(FlyRow::Row(plain(
                GroupBy::SameAsSort,
                group_same(),
                words!(
                    address_group_same_tip =
                        "Group by what the files are sorted by, in its direction"
                ),
            )));
            rows.push(FlyRow::Line);
            by(&mut rows, shell.group_keys(), &|key| {
                let current = spec.group == GroupBy::Key(key);
                let args = if current {
                    serde_json::json!({ "by": key, "ascending": spec.group_ascending })
                } else {
                    serde_json::json!({ "by": key })
                };
                menu_item(
                    Button::new(
                        format!("group-{key:?}"),
                        if current { "check" } else { "" },
                        words!(
                            address_group_by = "Group by { $key }",
                            key = key.phrase_words(),
                            note = "The tooltip of a criterion of the View menu's Group by; key: the criterion inside a sentence (name, date modified)."
                        ),
                        "group",
                    )
                    .args(args)
                    .active(current)
                    .keeps_open(),
                    key.title_words(),
                )
            });
        }
        // Each caption checked while chosen, in the sort's order and
        // groups; a row keeps the submenu open, for the next.
        UNDER_NAMES => {
            let said = shell.grid_captions();
            let mut last = None;
            for (i, spec, key) in under_names(shell) {
                if last.is_some_and(|last| last != kind(key)) {
                    rows.push(FlyRow::Line);
                }
                last = Some(kind(key));
                let on = said.contains(&spec.caption);
                // A tooltip is kept as built: it says what the row is about.
                rows.push(FlyRow::Row(menu_item(
                    Button::new(
                        format!("menu-caption-{i}"),
                        if on { "check" } else { "" },
                        caption_tip(key),
                        "ui.grid_caption",
                    )
                    .args(serde_json::json!({ "key": spec.setting }))
                    .active(on)
                    .keeps_open(),
                    spec.label_words(),
                )));
            }
        }
        _ => {}
    }
    rows
}

/// What the View menu's submenus call the row of a criterion's
/// directions.
fn direction_label() -> gpui::SharedString {
    words!(
        address_direction = "Direction",
        note =
            "A row of the View menu's Sort by and Group by: the two directions (A to Z, Z to A)."
    )
    .say()
    .into()
}

/// Group by's choice not to group, as its row and the View menu's say it.
fn group_none() -> Words {
    words!(
        address_group_none = "None",
        note = "A choice of the View menu's Group by: the files are not grouped."
    )
}

/// Group by's choice to group as the files are sorted.
fn group_same() -> Words {
    words!(
        address_group_same = "Same as sort",
        note = "A choice of the View menu's Group by: grouped by what the files are sorted by."
    )
}

/// The groups the View menu's submenus list criteria in: words (and
/// sizes), then dates, then what the user says of files.
fn family(key: skwir_core::SortKey) -> u8 {
    use skwir_core::SortKey;
    match key {
        SortKey::Modified
        | SortKey::Created
        | SortKey::Accessed
        | SortKey::Taken
        | SortKey::Deleted => 1,
        SortKey::Rating | SortKey::Tags | SortKey::Owner => 2,
        _ => 0,
    }
}

/// The View menu: how the active pane shows its folder, never how the
/// window is arranged (that is More's). Every view as a tile, the lenses
/// the pane row leaves out included (a narrow pane row has only the button
/// that opens this menu); the order: the sort and the groups, each one
/// row saying how they are, their direction and criteria beside it
/// (`view_submenu`), and Folders first; what is listed (hidden files, what
/// the location switches); what is drawn with it (the bars above and
/// under the files, what the view itself offers); the size, on one row.
fn view_options(shell: &Shell) -> Vec<MenuPart> {
    use skwir_core::grouping::GroupBy;
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
        words!(
            address_buttons_tip = "Choose the buttons beside the address: which views have one, and whether Up stands after Back and Forward"
        ),
        "ui.toolbar_menu",
    )
    .args(serde_json::json!({ "kind": VIEW_BUTTONS }))
    .label(
        words!(
            address_buttons = "Buttons…",
            note = "At the end of the View menu's Views heading: choose the buttons beside the address."
        ),
    );
    let spec = shell.sort_spec();
    // The sort and the groups each one row saying how they are ("Name, A
    // to Z"), which opens beside it their direction and their criteria.
    let mut order = vec![MenuPart::Sub(
        SORT_BY.into(),
        submenu_label(SORT_BY),
        Some(said(spec.key, spec.ascending).say().into()),
    )];
    // Grouped only while the sorter is there, which makes every group.
    if !shell.group_keys().is_empty() {
        let value = match spec.group {
            GroupBy::Key(key) => said(key, spec.group_ascending),
            GroupBy::SameAsSort => group_same(),
            GroupBy::None => group_none(),
        };
        order.push(MenuPart::Sub(
            GROUP_BY.into(),
            submenu_label(GROUP_BY),
            Some(value.say().into()),
        ));
    }
    // Whether the place shown is sorted and grouped so wherever it is
    // shown, apart from the pane's order (ADR-132): under what it
    // remembers, above folders first, which is every pane's. Greyed over a
    // search, whose terms are what it is, so no row moves between them.
    let shown = {
        let ws = shell.app.workspace.read();
        ws.pane(shell.pane)
            .and_then(|p| Some((p.location()?.clone(), p.keeps_order())))
    };
    if let Some((at, remembers)) = shown {
        let search = shell.app.is_search(&at);
        let remember_place = || {
            words!(
                address_remember_place = "Remember for this place",
                note = "A switch of the View menu: this place (a tag, a search) keeps its order and groups wherever it is shown."
            )
        };
        let (label, tooltip) = if search {
            (
                remember_place(),
                words!(
                    address_remember_search_tip =
                        "A search's order is not remembered: its terms are what it is"
                ),
            )
        } else if at.is_path() {
            (
                words!(
                    address_remember_folder = "Remember for this folder",
                    note = "A switch of the View menu: this folder keeps its order and groups wherever it is shown."
                ),
                words!(
                    address_remember_folder_tip = "Show this folder sorted and grouped as now wherever it is shown, in this view where the pane shows the list or the grid, apart from the pane's own; off, it takes the pane's"
                ),
            )
        } else {
            (
                remember_place(),
                words!(
                    address_remember_place_tip = "Show this place sorted and grouped as now wherever it is shown, in this view where the pane shows the list or the grid, apart from the pane's own; off, it takes the pane's"
                ),
            )
        };
        let mut args = serde_json::json!({ "on": !remembers, "pane": shell.pane });
        // The view shown, where a folder may remember it (the list, the
        // grid): not the columns nor the tree, which are the pane's.
        let view = shell.ui().view_id;
        if shell.app.remembered_view(view) {
            args["view"] = serde_json::json!(view);
        }
        order.push(MenuPart::Row(menu_item(
            Button::new("menu-remember-order", "", tooltip, "remember_order")
                .args(args)
                .active(remembers)
                .enabled(!search),
            label,
        )));
    }
    // Folders before files, or sorted with them, within each group: the
    // order's too, under the sort and the groups (their criteria are in
    // their submenus, so a check here reads as none of them). The setting
    // itself, in every pane, so the row never disagrees with Settings.
    order.push(MenuPart::Row(menu_item(
        Button::new(
            "menu-folders-first",
            "",
            words!(
                address_folders_first_tip =
                    "List folders before files, in every pane; off, sort them together"
            ),
            "settings.apply",
        )
        .args(serde_json::json!({
            "changes": {
                skwir_core::location::FOLDERS_FIRST: (!spec.folders_first).to_string()
            }
        }))
        .active(spec.folders_first),
        words!(
            address_folders_first = "Folders first",
            note = "A switch of the View menu: folders are listed before files."
        ),
    )));
    // What is listed.
    let mut show = vec![menu_item(
        Button::new(
            "menu-hidden",
            "",
            words!(address_hidden_tip = "Show hidden files"),
            "toggle_hidden",
        )
        .active(spec.show_hidden),
        words!(
            address_hidden = "Hidden files",
            note = "A switch of the View menu: whether hidden files (their names begin with a dot) are listed."
        ),
    )];
    // With hidden files, the files that keep another's notes beside it
    // (ADR-028): the same setting as Tags and notes', where one looks for
    // a file that seems missing.
    if spec.show_hidden && !shell.app.companions.is_empty() {
        let shown = shell.app.companions.shown();
        show.push(menu_item(
            Button::new(
                "menu-companions",
                "",
                words!(
                    address_companions_tip = "Also list the files that keep another file's notes beside it (a Mac's ._ files, sidecars)"
                ),
                "settings.apply",
            )
            .args(serde_json::json!({
                "changes": { skwir_core::companions::SHOW: (!shown).to_string() }
            }))
            .active(shown),
            words!(
                address_companions = "Companion files",
                note = "A switch of the View menu: whether the files that keep another file's notes beside it are listed."
            ),
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
    // What is drawn with them: the kinds, years and tags of the files
    // shown, to filter by (`quick_filters.rs`), wherever the query plugin
    // can; and when they changed, under them, wherever the timeline's can.
    // Both the pane's own, as its view is.
    let mut drawn = Vec::new();
    if shell.app.commands.get("query.narrow").is_some() {
        drawn.push(menu_item(
            Button::new(
                "menu-quick-filters",
                "",
                words!(
                    address_quick_filters_tip = "Offer the kinds, years and tags of this pane's files above them, each with how many files, to show only those until taken off"
                ),
                "ui.quick_filters",
            )
            .active(shell.ui().quick_filters_on),
            words!(
                address_quick_filters = "Quick filters",
                note = "A switch of the View menu: a row above the files offering their kinds, years and tags to filter by."
            ),
        ));
    }
    if shell.app.commands.get("timeline.histogram").is_some() {
        drawn.push(menu_item(
            Button::new(
                "menu-timeline",
                "",
                words!(
                    address_timeline_tip = "Show when this pane's files changed under them, a bar for each day, week, month or year; a bar pressed shows only its files"
                ),
                "ui.timeline",
            )
            .active(shell.timeline().open),
            words!(
                address_timeline = "Timeline",
                note = "A switch of the View menu: a strip under the files showing when they changed."
            ),
        ));
    }
    // Only the grid draws thumbnails in a box.
    if shell.ui().view_id == crate::view::GRID {
        drawn.push(menu_item(
            Button::new(
                "menu-grid-fill",
                "",
                words!(address_grid_fill_tip = "Crop thumbnails in the grid to fill their box"),
                "ui.grid_fill",
            )
            .active(shell.prefs.grid_fill),
            words!(
                address_grid_fill = "Fill thumbnails",
                note =
                    "A switch of the View menu, in the grid: pictures cropped to fill their tiles."
            ),
        ));
    }
    // The tree closes what it opened in one go.
    if shell.ui().view_id == crate::view::TREE {
        drawn.push(menu_item(
            Button::new(
                "menu-collapse-all",
                "fold",
                words!(address_collapse_all_tip = "Close every folder opened in place"),
                "expand",
            )
            .args(serde_json::json!({"mode": "collapse_all"})),
            words!(
                address_collapse_all = "Collapse all",
                note = "An item of the View menu, in the tree: close every folder opened in place."
            ),
        ));
    }
    let mut parts = vec![
        MenuPart::Title(
            words!(
                address_views = "Views",
                note = "The heading of the View menu's views (list, grid, columns)."
            )
            .say()
            .into(),
            Some(buttons),
        ),
        MenuPart::Tiles(views),
        MenuPart::Line,
    ];
    parts.extend(order);
    parts.push(MenuPart::Line);
    parts.extend(show.into_iter().map(MenuPart::Row));
    if !drawn.is_empty() {
        parts.push(MenuPart::Line);
        parts.extend(drawn.into_iter().map(MenuPart::Row));
    }
    // What the tiles say under their names, one row saying which ("Size,
    // Modified"), opening them beside it to choose.
    if shell.ui().view_id == crate::view::GRID {
        let said: Vec<String> = under_names(shell)
            .into_iter()
            .filter(|(_, spec, _)| shell.grid_captions().contains(&spec.caption))
            .map(|(_, spec, _)| spec.label_words().say())
            .collect();
        let value = if said.is_empty() {
            words!(
                address_under_names_none = "None",
                note = "What the View menu's Under names row says when tiles say nothing under their names."
            )
            .say()
        } else {
            // Transitional: a list argument (ADR-139).
            said.join(", ")
        };
        parts.push(MenuPart::Sub(
            UNDER_NAMES.into(),
            submenu_label(UNDER_NAMES),
            Some(value.into()),
        ));
    }
    // Only where the view sizes something (`PaneView::zooms`): smaller, as
    // it was, larger, the keys in the tooltips.
    let zooms = shell.view_zooms();
    if zooms != crate::view::Zooms::Nothing {
        let levels = zooms == crate::view::Zooms::Levels;
        // The tooltips of larger, smaller and as it was: Usage's map
        // draws levels, the other views a size.
        let [larger, smaller, reset] = if levels {
            [
                words!(
                    address_more_levels = "More levels (or Ctrl and the wheel)",
                    note = "A button of the View menu, in Usage: its map draws more levels of folders."
                ),
                words!(
                    address_fewer_levels = "Fewer levels (or Ctrl and the wheel)",
                    note = "A button of the View menu, in Usage: its map draws fewer levels of folders."
                ),
                words!(address_default_levels = "Default levels"),
            ]
        } else {
            [
                words!(
                    address_larger = "Larger (or Ctrl and the wheel)",
                    note = "A button of the View menu: draw the files larger."
                ),
                words!(
                    address_smaller = "Smaller (or Ctrl and the wheel)",
                    note = "A button of the View menu: draw the files smaller."
                ),
                words!(address_default_size = "Default size"),
            ]
        };
        let size = [
            ("out", "minus", None, smaller),
            (
                "reset",
                "",
                Some(words!(
                    address_zoom_default = "Default",
                    note = "The middle button of the View menu's size: back to the size it had."
                )),
                reset,
            ),
            ("in", "plus", None, larger),
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
        let name = if levels {
            words!(
                address_levels = "Levels",
                note = "The View menu's row of Usage's map: how many levels of folders it draws."
            )
        } else {
            words!(
                address_size = "Size",
                note = "The View menu's row of how large the files are drawn."
            )
        };
        parts.push(MenuPart::Choice(name.say().into(), size));
    }
    parts
}

/// What the grid's tiles can say under their names here (`Shell::caption_applies`),
/// each caption with its place in `views::grid::CAPTIONS` and the criterion
/// that sorts by it, in Sort by's order: as the sort's submenu lists them.
fn under_names(
    shell: &Shell,
) -> Vec<(
    usize,
    &'static crate::views::grid::CaptionSpec,
    skwir_core::SortKey,
)> {
    use skwir_core::SortKey;
    let mut captions: Vec<_> = crate::views::grid::CAPTIONS
        .iter()
        .enumerate()
        .filter(|(_, spec)| shell.caption_applies(spec.caption))
        .map(|(i, spec)| (i, spec, spec.sorts_by()))
        .collect();
    captions.sort_by_key(|(_, _, key)| SortKey::ALL.iter().position(|k| k == key));
    captions
}

/// The pane row's buttons: the views', under a heading whose switch says
/// whether they have any (`MenuPart::Heading`), each view checked or not,
/// and the buttons skwir comes with once others were chosen; then whether
/// Up stands after Back and Forward. The view shown keeps its button while
/// it is shown, checked or not; with the buttons off, one button stands
/// for them all and the views' rows wait, dimmed, as chosen. A row keeps
/// the menu open, for the next; Default buttons closes it.
fn view_buttons_menu(shell: &Shell) -> Vec<MenuPart> {
    let chosen = &shell.view_buttons;
    let shown = shell.prefs.view_buttons_shown;
    // A row stays, open, as its check or switch changes: gpui keeps a
    // tooltip as it was built, so it says what the row is about, never
    // what pressing it does now.
    let all = Button::new(
        "view-buttons-shown",
        "",
        words!(
            address_view_buttons_tip = "Whether the views have buttons beside the address; off, one button, the view shown, opens the View menu"
        ),
        "ui.view_buttons_shown",
    )
    .args(serde_json::json!({ "on": !shown }))
    .active(shown)
    .label(
        words!(
            address_view_buttons = "View buttons",
            note = "A heading with a switch, in the menu of the buttons beside the address: whether the views have buttons."
        ),
    )
    .switch();
    let mut parts = vec![MenuPart::Heading(all)];
    parts.extend(shell.available_views().into_iter().map(|spec| {
        let pinned = chosen.pinned(&spec);
        let tip = words!(
            address_view_button_tip = "Whether the { $view } view has a button",
            view = spec.title.clone(),
            note = "view: a view's name (List, Grid, Columns)."
        );
        MenuPart::Row(menu_item(
            Button::new(
                format!("view-button-{}", spec.id),
                if pinned { "check" } else { "" },
                tip,
                "ui.view_button",
            )
            .args(serde_json::json!({ "id": spec.id, "on": !pinned }))
            .active(pinned)
            .enabled(shown)
            .keeps_open(),
            spec.title.clone(),
        ))
    }));
    if !chosen.is_default() {
        parts.push(MenuPart::Row(
            menu_item(
                Button::new(
                    "view-buttons-reset",
                    "revert",
                    words!(address_default_buttons_tip = "Back to the buttons Skwir comes with"),
                    "ui.view_buttons_reset",
                ),
                words!(
                    address_default_buttons = "Default buttons",
                    note = "An item of the menu of the buttons beside the address: the buttons as they first were."
                ),
            )
            .enabled(shown),
        ));
    }
    let up = shell.prefs.up_button;
    parts.extend([
        MenuPart::Title(
            words!(
                address_navigation_buttons = "Navigation buttons",
                note = "A heading of the menu of the buttons beside the address: Back, Forward, Up."
            )
            .say()
            .into(),
            None,
        ),
        MenuPart::Row(menu_item(
            Button::new(
                "up-button",
                if up { "check" } else { "" },
                words!(
                    address_up_button_tip = "Whether Up stands after Back and Forward: it goes to the folder that holds this one, as Alt+Up does"
                ),
                "ui.up_button",
            )
            .args(serde_json::json!({ "on": !up }))
            .active(up)
            .keeps_open(),
            words!(
                address_up_button = "Up",
                note = "An item of the menu of the buttons beside the address: whether the Up button is there."
            ),
        )),
    ]);
    parts
}

/// More's layout: the window's panel without a button of its own (the
/// terminal), checked while open, then where the split's panes, the
/// preview and a file of an archive go, each a setting on one row. What a
/// pane shows above and under its files (Quick filters, the timeline) is
/// the View menu's, as its view is.
fn layout_parts(shell: &Shell) -> Vec<MenuPart> {
    let mut parts = Vec::new();
    // The terminal is a plugin's: without it, nothing says it exists.
    if shell.app.terminals.provider().is_some() {
        parts.extend([
            MenuPart::Row({
                let terminal = words!(
                    address_terminal = "Terminal",
                    note = "An item of the More menu: open or close the terminal panel."
                );
                menu_item(
                    Button::new("menu-terminal", "", terminal.clone(), "ui.terminal")
                        .active(shell.terminal.open),
                    terminal,
                )
            }),
            MenuPart::Line,
        ]);
    }
    parts.extend([
        MenuPart::Choice(
            words!(
                address_layout_split = "Split",
                note = "A row of the More menu: where split panes go (Auto, side by side, one above the other)."
            )
            .say()
            .into(),
            layout_values(shell, "panes"),
        ),
        MenuPart::Choice(
            words!(
                address_layout_preview = "Preview",
                note = "A row of the More menu: where the preview goes (Auto, on the right, at the bottom)."
            )
            .say()
            .into(),
            layout_values(shell, "preview"),
        ),
        MenuPart::Choice(
            words!(
                address_layout_member = "File in an archive",
                note = "A row of the More menu: where a file of an archive is previewed beside its files."
            )
            .say()
            .into(),
            layout_values(shell, "member"),
        ),
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
    let right = || {
        words!(
            address_on_the_right = "On the right",
            note = "A choice of where a panel goes, in the More menu."
        )
    };
    let bottom = || {
        words!(
            address_at_the_bottom = "At the bottom",
            note = "A choice of where a panel goes, in the More menu."
        )
    };
    let (automatic, values): (Words, [(&str, &str, Words, bool, bool); 2]) = match what {
        // Where a file of an archive, or an attachment, is previewed beside
        // the view it came from; Automatic follows the panel's shape.
        "member" => {
            let beside = layout.member.beside(layout.bottom());
            (
                words!(
                    address_member_auto = "Automatic: under the archive's files while the preview is at the right, at \
                     their right while it is at the bottom"
                ),
                [
                    (
                        "right",
                        "preview",
                        right(),
                        layout.member == Member::Right,
                        beside,
                    ),
                    (
                        "bottom",
                        "preview-bottom",
                        bottom(),
                        layout.member == Member::Bottom,
                        !beside,
                    ),
                ],
            )
        }
        "panes" => (
            words!(
                address_panes_auto = "Automatic: one above the other while the window is taller than wide, else side \
                 by side"
            ),
            [
                (
                    "row",
                    "split",
                    words!(
                        address_side_by_side = "Side by side",
                        note = "A choice of where split panes go, in the More menu."
                    ),
                    layout.panes == Panes::Row,
                    !layout.tall(),
                ),
                (
                    "column",
                    "split-rows",
                    words!(
                        address_one_above = "One above the other",
                        note = "A choice of where split panes go, in the More menu."
                    ),
                    layout.panes == Panes::Column,
                    layout.tall(),
                ),
            ],
        ),
        _ => (
            words!(
                address_preview_auto = "Automatic: at the bottom while the window is taller than wide, else on the right"
            ),
            [
                (
                    "right",
                    "preview",
                    right(),
                    layout.preview == Preview::Right,
                    !layout.tall(),
                ),
                (
                    "bottom",
                    "preview-bottom",
                    bottom(),
                    layout.preview == Preview::Bottom,
                    layout.tall(),
                ),
            ],
        ),
    };
    let auto = !values.iter().any(|(_, _, _, chosen, _)| *chosen);
    let mut buttons = vec![
        Button::new(format!("layout-{what}-auto"), "", automatic.clone(), "ui.layout")
            .args(serde_json::json!({ what: "auto" }))
            .label(
                words!(
                    address_auto = "Auto",
                    note = "The first choice of where a panel goes, in the More menu: chosen by the window's shape."
                ),
            )
            .active(auto)
            .segment(),
    ];
    buttons.extend(values.into_iter().map(|(value, icon, words, chosen, now)| {
        Button::new(
            format!("layout-{what}-{value}"),
            icon,
            words.clone(),
            "ui.layout",
        )
        .args(serde_json::json!({ what: value }))
        .active(chosen)
        .now(auto && now)
        .segment()
    }));
    buttons
}

/// What the More menu's Undo and Redo say: each names what it takes back
/// (`Executor::undo_target`: its title, and whether it has not ended, Undo
/// waiting for it then; `last_redo_title`), as the footer's Undo does.
fn undo_words(undo: Option<&(Words, bool)>, redo: Option<&Words>) -> (Words, Words) {
    let undo = match undo {
        Some((title, true)) => words!(
            address_undo_once_ended = "Undo { $plan } (once it ends)",
            plan = title.clone(),
            note = "An item of the More menu: undo what is still running, once it ends; plan: what it does (copy a.txt to Backup)."
        ),
        Some((title, false)) => words!(
            address_undo_plan = "Undo { $plan }",
            plan = title.clone(),
            note = "An item of the More menu; plan: what it undoes (rename to final.txt)."
        ),
        None => undo_verb(),
    };
    let redo = redo.map_or_else(redo_verb, |title| {
        words!(
            address_redo_plan = "Redo { $plan }",
            plan = title.clone(),
            note = "An item of the More menu; plan: what it does again (new folder Reports)."
        )
    });
    (undo, redo)
}

/// Undo with nothing named, as the More menu's item says it.
fn undo_verb() -> Words {
    words!(
        address_undo = "Undo",
        note = "An item of the More menu, while there is nothing to undo; and its tooltip."
    )
}

/// Redo with nothing named.
fn redo_verb() -> Words {
    words!(
        address_redo = "Redo",
        note = "An item of the More menu, while there is nothing to redo; and its tooltip."
    )
}

/// The More menu's Undo and Redo, each named by what it takes back and off
/// when there is nothing to, and the History (ADR-079, 13).
fn undo_rows(shell: &Shell) -> Vec<MenuPart> {
    let undo = shell.app.executor.undo_target();
    let redo = shell.app.executor.last_redo_title();
    let (undo_said, redo_said) = undo_words(undo.as_ref(), redo.as_ref());
    [
        menu_item(
            Button::new("menu-undo", "revert", undo_verb(), "undo"),
            undo_said,
        )
        .enabled(undo.is_some()),
        menu_item(
            Button::new("menu-redo", "redo", redo_verb(), "redo"),
            redo_said,
        )
        .enabled(redo.is_some()),
        menu_item(
            Button::new(
                "menu-undo-history",
                "recent",
                words!(address_undo_history_tip = "Undo history"),
                "ui.undo_history",
            ),
            words!(
                address_undo_history = "Undo history…",
                note = "An item of the More menu: the list of what was done, to undo or redo."
            ),
        ),
    ]
    .into_iter()
    .map(MenuPart::Row)
    .collect()
}

/// The More menu: the panels without a button, the window's layout, Undo,
/// Redo and the History, New tab, the palette, Settings and About, then
/// Quit.
fn more_panel(
    shell: &Shell,
    popup: &toolbar::Popup,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let panel = toolbar::menu_panel(popup, "ui.toolbar_menu", theme, cx);
    let mut parts = layout_parts(shell);
    parts.push(MenuPart::Line);
    parts.extend(undo_rows(shell));
    parts.push(MenuPart::Line);
    parts.extend(
        [
            (
                Button::new(
                    "menu-new-tab",
                    "tab-new",
                    words!(address_new_tab = "New tab"),
                    "tab.new",
                ),
                words!(address_new_tab = "New tab"),
            ),
            (
                Button::new(
                    "menu-new-window",
                    "window-new",
                    words!(address_new_window = "New window"),
                    "window.new",
                ),
                words!(address_new_window = "New window"),
            ),
            (
                Button::new(
                    "menu-palette",
                    "command",
                    words!(address_palette_tip = "Command palette"),
                    "ui.palette",
                ),
                words!(address_palette = "Command palette…"),
            ),
            (
                Button::new(
                    "menu-shortcuts",
                    "keyboard",
                    words!(
                        address_shortcuts_tip = "Shortcuts here",
                        note = "The tooltip of the More menu's Shortcuts…: the keys that work where one is."
                    ),
                    "ui.shortcuts",
                ),
                words!(address_shortcuts = "Shortcuts…"),
            ),
            (
                Button::new(
                    "menu-settings",
                    "settings",
                    words!(address_settings_tip = "Settings"),
                    "ui.settings",
                ),
                words!(address_settings = "Settings…"),
            ),
            (
                Button::new(
                    "menu-about",
                    "info",
                    words!(address_about = "About Skwir"),
                    "ui.about",
                ),
                words!(address_about = "About Skwir"),
            ),
        ]
        .into_iter()
        .map(|(button, label)| MenuPart::Row(menu_item(button, label))),
    );
    // Quit, last: it asks while something runs; closing the window stops
    // nothing (ADR-115).
    parts.push(MenuPart::Line);
    parts.push(MenuPart::Row(menu_item(
        Button::new(
            "menu-quit",
            "power",
            words!(address_quit_tip = "Quit Skwir"),
            "ui.quit",
        ),
        words!(
            address_quit = "Quit",
            note = "The last item of the More menu: end skwir, every window."
        ),
    )));
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
        Button::new(
            "sidebar-toggle",
            "sidebar",
            words!(
                address_sidebar = "Sidebar",
                note = "The tooltip of the button that shows or hides the sidebar."
            ),
            "ui.toggle_sidebar",
        )
            .active(!shell.sidebar_hidden)
            .render(shell, cx)
            .into_any_element(),
        Button::new(
            "tools-more",
            "more",
            words!(
                address_more = "More",
                note = "The tooltip of the button of the window's menu (layout, undo, settings, quit)."
            ),
            "ui.toolbar_menu",
        )
            .args(serde_json::json!({"kind": MORE}))
            .anchor_if(&shell.more_anchor, true)
            .active(open == Some(MORE))
            .render(shell, cx)
            .into_any_element(),
    ]
}

/// Split, Properties and Preview, at the end of what has them
/// (`Shell::toggles_at`): the panel's two switches side by side, each lit
/// while on, the panel there while either is (`PreviewState::panel_open`).
/// `labeled`: each says its name beside its icon, so they are found; each
/// place that holds them offers this as its richest variant, chosen by
/// `AdaptiveRow` while nothing else gives up room for it.
pub fn panel_toggles(shell: &Shell, labeled: bool, cx: &mut Context<Shell>) -> [AnyElement; 3] {
    let (split_icon, split_tip) = shell.layout.split_button();
    let split_tip = shell.split_close_tip().unwrap_or(split_tip);
    let split = Button::new("split-toggle", split_icon, split_tip, "ui.split");
    let properties_said = words!(
        address_properties = "Properties",
        note = "A button beside the preview's: show the selected file's properties in the panel."
    );
    let preview_said = words!(
        address_preview = "Preview",
        note = "A button that shows or hides the preview panel."
    );
    let properties = Button::new(
        "properties-toggle",
        "info",
        properties_said.clone(),
        "ui.properties",
    );
    let preview = Button::new(
        "preview-toggle",
        shell.layout.preview_icon(),
        preview_said.clone(),
        "ui.toggle_preview",
    );
    let (split, properties, preview) = if labeled {
        (
            split.label(words!(
                address_split = "Split",
                note = "A button's label: split the pane in two, or close the split."
            )),
            properties.label(properties_said),
            preview.label(preview_said),
        )
    } else {
        (split, properties, preview)
    };
    [
        split
            .active(shell.is_split())
            .render(shell, cx)
            .into_any_element(),
        properties
            .active(shell.preview.properties_on)
            .render(shell, cx)
            .into_any_element(),
        preview
            .active(shell.preview.preview_on)
            .render(shell, cx)
            .into_any_element(),
    ]
}

/// The sidebar's header: its toggle, and More. As tall as a pane's row,
/// so their edges meet.
pub fn sidebar_header(shell: &Shell, cx: &mut Context<Shell>) -> AnyElement {
    let theme = cx.global::<Theme>().clone();
    let [toggle, more] = corner(shell, cx);
    let buttons = crate::title_bar::top_row_buttons(shell, crate::title_bar::End::Start, cx);
    div()
        .id("sidebar-header")
        .tab_group()
        .relative()
        .children(crate::title_bar::top_row_drag(shell, true))
        .flex_none()
        .h(sized(PANE_ROW_HEIGHT))
        .flex()
        .items_center()
        .gap(sized(2.))
        .px(sized(6.))
        // On the frame as cards (`Theme::cards`), which paints it.
        .when_some(theme.chrome_ground(), gpui::Styled::bg)
        .when(theme.lines(), |d| {
            d.border_b_1().border_end(px(1.)).border_color(theme.border)
        })
        .children(buttons)
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
    /// The parts of the row that lie in its band (the field, the view
    /// buttons' group), holes in its ground (`elements::ground`).
    holes: crate::elements::ground::Holes,
    back: bool,
    forward: bool,
    /// Up, while the row has it (`ui.up_button`): whether there is a
    /// place above the one shown.
    up: Option<bool>,
    address: Address,
    /// The field's edge: focused, the active pane's, or none.
    edge: gpui::Hsla,
    location: Option<skwir_core::Location>,
    /// What filters the location shown, as the keep button keeps it
    /// (ADR-109).
    filter: Option<String>,
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
/// its start, Split, Properties and Preview at its end.
#[derive(Clone, Copy)]
struct Ends {
    corner: bool,
    toggles: bool,
}

/// A pane's row: Back and Forward, Up after them where the user asked for
/// it (`ui.up_button`; else the trail's parent segment, or Alt+Up), the
/// address (a trail of folders, or the text being edited) with the button
/// that keeps it in the sidebar, then how the pane shows its folder.
///
/// Variants chosen in prepaint by `AdaptiveRow`: the view buttons while
/// the address keeps `FIELD_MIN`, else the one button that opens the View
/// menu, which is all there is with the buttons off
/// (`ui.view_buttons_shown`); a row holding Split first tries it with its
/// label. Narrower still, the field gives up its own buttons (Search, the
/// keep button, an aspect's; Ctrl+F, Ctrl+D and Alt+G remain) and keeps
/// only the room left. The address counts as `FIELD_MIN` when a variant
/// is measured (its content has no width of its own) and takes the rest
/// when drawn.
pub fn render_address(shell: &Shell, window: &Window, cx: &mut Context<Shell>) -> AnyElement {
    let theme = cx.global::<Theme>().clone();
    let roots = &shell.roots.roots;
    let row = {
        let ws = shell.app.workspace.read();
        let pane = ws.pane(shell.pane);
        let location = pane.and_then(|p| p.location()).cloned();
        let filter = pane.and_then(|p| p.filter()).map(str::to_owned);
        let active = shell.window_pane_of(&ws) == shell.pane;
        let focused = shell.ui().address.read(cx).is_focused(window);
        PaneRow {
            holes: crate::elements::ground::Holes::default(),
            back: pane.is_some_and(|p| p.history_pos > 0),
            forward: pane.is_some_and(|p| p.history_pos + 1 < p.history.len()),
            up: shell
                .prefs
                .up_button
                .then(|| goes_up(location.as_ref(), shell.ui().place.trail.as_ref())),
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
            // As cards, the ring says which pane is active.
            edge: if focused {
                theme.accent
            } else if active && theme.lines() {
                theme.border
            } else {
                // None: the field's own ground runs to its edge.
                gpui::transparent_black()
            },
            location,
            filter,
            // The View menu hangs from the active pane's button: the pane it
            // acts on.
            menu_open: shell
                .toolbar_menu
                .as_ref()
                .map(|menu| menu.kind)
                .filter(|kind| active && [VIEW_OPTIONS, VIEW_BUTTONS].contains(kind)),
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
    // As a card, what the row holds of the window's lies in notches on the
    // frame (`pane_row`), the band painted by its own part.
    let notched = notched(&theme, row.ends);
    let mut bar = div()
        .id(("pane-address", shell.pane.0 as usize))
        .tab_group()
        .relative()
        // Its empty parts move the window while the window's buttons are
        // in the top row and this row holds the window's tools.
        .when(row.ends.corner || row.ends.toggles, |d| {
            d.children(crate::title_bar::top_row_drag(shell, true))
        })
        .when(!notched, |d| d.px(sized(6.)))
        .w_full()
        .flex_none()
        // A card's band over its files (`Theme::cards`): no line under it.
        .when(theme.lines(), |d| d.border_b_1().border_color(theme.border))
        // The raised pane's row, underlined in the accent (`render_pane`);
        // the band's alone where notches cut it.
        .when(!notched && shell.drawn_raised == Some(true), |d| {
            d.child(underline(&theme))
        })
        .child(AdaptiveRow {
            variants: row_variants(shell.prefs.view_buttons_shown, row.ends.toggles)
                .into_iter()
                .map(|variant| {
                    let anchors = [&anchor, &nav_anchor, &chip_anchor, &omni_anchor];
                    pane_row(shell, &row, variant, anchors, cx)
                })
                .collect(),
            height: PANE_ROW_HEIGHT - theme.line_width(),
            chosen: None,
        });
    if let Some(kind) = row.menu_open
        && let Some(popup) = &shell.toolbar_menu
    {
        let mut panel = toolbar::menu_panel(popup, "ui.toolbar_menu", &theme, cx);
        let parts = match kind {
            VIEW_BUTTONS => view_buttons_menu(shell),
            _ => view_options(shell),
        };
        panel = toolbar::menu_parts(panel, parts, shell, &theme, cx)
            // Resting on a row closes the submenu open, but on its own.
            .on_mouse_move(cx.listener(|this, _, window, cx| {
                this.popup_sub_rest(None, window, cx);
            }));
        // Which views have a button hangs from the buttons' right edge,
        // which stays where it is as buttons come and go under it.
        bar = bar.child(if kind == VIEW_BUTTONS {
            anchor.popup_right(panel)
        } else {
            anchor.popup(panel)
        });
        // Sort by's or Group by's criteria, or what is said under the
        // names, beside their row.
        if let Some(name) = popup.sub.open.clone() {
            let fly = toolbar::fly_out(
                popup,
                submenu_label(&name),
                view_submenu(shell, &name),
                shell,
                &theme,
                cx,
            );
            bar = bar.child(popup.sub.row.popup_beside(4, crate::menu::SUB_INSET, fly));
        }
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
    // Its ground, once, around the field and the view buttons' group; the
    // band's between the notches paints its own.
    if notched {
        bar.into_any_element()
    } else {
        crate::elements::ground::ground(&row.holes, theme.bar_ground(shell.lowered()), bar)
            .into_any_element()
    }
}

/// Whether Up goes anywhere from `shown`, as the `up` command goes: a
/// folder to the one holding it, another place to the one before it in
/// its `trail`, as read for the place shown (`trail::sync`), never asked
/// of its provider again while drawing.
fn goes_up(shown: Option<&Location>, trail: Option<&skwir_core::location::Trail>) -> bool {
    match shown.map(Location::path) {
        Some(Some(path)) => path.parent().is_some(),
        Some(None) => trail
            .and_then(skwir_core::location::Trail::parent)
            .is_some(),
        None => false,
    }
}

/// One way of laying out a pane's row (`row_variants`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Variant {
    /// The view buttons beside the View menu, else its one button.
    wide: bool,
    /// Split and the panel's switches say their names.
    labeled: bool,
    /// The field holds its own buttons and keeps `FIELD_MIN`; else it has
    /// only the room the rest leaves.
    field_buttons: bool,
}

/// A pane row's variants, richest first: whether it has the view buttons
/// (`wide`), whether Split and the panel's switches say their names
/// (`labeled`, only where the row holds them, `toggles`), and whether the
/// field has its buttons. The labels go first, the view buttons next, the
/// field's buttons last, so a narrow pane's row fits rather than drawing
/// past its end (over a notch, as a card). With the view buttons off
/// (`buttons`), no variant has them: the one button is all there is.
fn row_variants(buttons: bool, toggles: bool) -> Vec<Variant> {
    let variant = |wide, labeled, field_buttons| Variant {
        wide,
        labeled,
        field_buttons,
    };
    let variants = if buttons {
        vec![
            variant(true, true, true),
            variant(true, false, true),
            variant(false, false, true),
            variant(false, false, false),
        ]
    } else {
        vec![
            variant(false, true, true),
            variant(false, false, true),
            variant(false, false, false),
        ]
    };
    variants
        .into_iter()
        .filter(|v| !v.labeled || toggles)
        .collect()
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
            words!(
                address_forward_tip = "Forward · hold or right-click for the history",
                note = "The tooltip of the Forward button."
            ),
            "forward",
            row.forward,
        )
    } else {
        (
            "back",
            words!(
                address_back_tip = "Back · hold or right-click for the history",
                note = "The tooltip of the Back button."
            ),
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
    variant: Variant,
    [anchor, nav_anchor, chip_anchor, omni_anchor]: [&PopoverAnchor; 4],
    cx: &mut Context<Shell>,
) -> AnyElement {
    let Variant {
        wide,
        labeled,
        field_buttons,
    } = variant;
    let theme = cx.global::<Theme>().clone();
    let pane = shell.pane;
    // A location reads as a trail of places, a search's or what narrows a
    // folder as pills after it; the text field takes over while it is
    // being edited. No width of its own: the field's minimum is what a
    // variant is measured with. Its basis is nothing in pixels: one in
    // percent (`flex_1`) comes to its content where a variant is measured
    // at its natural width, with no width to take a share of, and the
    // whole trail would count.
    let address = match &row.address {
        // The pills follow the trail; the room after them edits the
        // address.
        Address::Trail {
            segments,
            glyph,
            holds_places,
        } => div()
            .flex_grow_1()
            .flex_shrink(1.)
            .flex_basis(px(0.))
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
                    .px(sized(3.))
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
            // What the field takes (ADR-137), at its end as in any field
            // that says it, the typing going on: at the card's while the card
            // stands around the field (`omnibox::dropdown`), its room kept
            // here so the text never runs under it.
            let help = if shell.omnibox_card() {
                crate::omnibox::question_room()
            } else {
                crate::omnibox::question(shell, cx)
            };
            let typed = div()
                .flex_grow_1()
                .flex_shrink(1.)
                .flex_basis(px(0.))
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
                        .line_height(sized(ADDRESS_LINE))
                        .child(shell.ui().address.clone()),
                )
                .child(
                    div()
                        .pad_end(px(crate::omnibox::QUESTION_INSET))
                        .child(help),
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
    // F1 says what the field takes, in its card (ADR-137).
    if shell.ui().omni.is_some() {
        context.add("reference");
    }
    let mut field = div()
        .id(("address-field", pane.0 as usize))
        .flex_1()
        .when(field_buttons, |d| d.min_w(sized(FIELD_MIN)))
        .when(!field_buttons, gpui::Styled::min_w_0)
        .h(sized(FIELD_HEIGHT))
        .flex()
        .items_center()
        .rounded_md()
        .border_1()
        .border_color(row.edge)
        // A part of the row: its own ground at the row's opacity, a hole
        // in the row's (`elements::ground`).
        .bg(theme.bg.opacity(theme.opacity.interface))
        .key_context(context)
        .child(address)
        // The field's card stands around all of it (`popup_around`).
        .child(omni_anchor.marker());
    // Search, as Ctrl+F: the field is where words search (ADR-098), and
    // nothing else in sight says so. Offered where Ctrl+F has a place to
    // look in or a search to edit, while a source searches; gone while
    // the field is typed in, its token wearing the same magnifier.
    let title = (field_buttons && matches!(row.address, Address::Trail { .. }))
        .then(|| {
            search_title(
                &shell.app,
                row.location.as_ref(),
                !shell.ui().chips.chips.is_empty(),
            )
        })
        .flatten();
    if let Some(title) = title {
        field = field.child(
            Button::new(format!("search-{}", pane.0), "search", title, "ui.search")
                .render(shell, cx)
                .h(sized(24.))
                .min_w(px(24.))
                .px(sized(4.))
                .margin_end(px(2.)),
        );
    }
    // Keep what the pane shows in the sidebar as it shows it (ADR-109), a
    // folder as a bookmark, a search as a saved search, anything filtered
    // with its filter, while a plugin keeps such locations: filled when it
    // is kept, and pressing it then takes it back with the place's own
    // command. It is at the field's end: it is about what the field shows.
    if let Some(location) = row.location.as_ref().filter(|_| field_buttons) {
        let filter = row.filter.as_deref();
        // Offered only where its keeper keeps what is shown (`keep`).
        let keeper = shell.app.keeper_of(location, filter);
        // Kept plain where nothing keeps a filter (ADR-109 rule 7); a
        // remote folder by its address (`App::kept_place`).
        let kept_filter = match &keeper {
            Some((_, false)) => None,
            _ => filter,
        };
        let kept = shell.app.kept_place(location, kept_filter);
        let keeper = keeper.map(|(keeper, _)| keeper);
        let button = match (kept, keeper) {
            (Some(place), keeper) => {
                let forget = place.forget.expect("kept");
                let title = keeper.map_or_else(
                    || {
                        words!(
                            address_remove_kept = "Remove from sidebar",
                            note = "The tooltip of the button at the address's end while what is shown is kept in the sidebar."
                        )
                    },
                    |k| k.forget_title,
                );
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
                    .h(sized(24.))
                    .min_w(px(24.))
                    .px(sized(4.))
                    .margin_end(px(2.)),
            );
        }
    }
    // Another way to see the folder shown (`Aspect`: its tags), offered on
    // the folder and lit on its own locations, from which its command goes
    // back to the folder. Last, at the field's edge, so that it stays where
    // it was pressed whether or not the keep button is offered.
    if let Some(location) = row.location.as_ref().filter(|_| field_buttons) {
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
                    .h(sized(24.))
                    .min_w(px(24.))
                    .px(sized(4.))
                    .margin_end(px(2.)),
            );
        }
    }
    let theme = cx.global::<Theme>().clone();
    let notched = notched(&theme, row.ends);
    let mut line = div().size_full().flex().items_center().gap(sized(3.));
    let mut start = None;
    if row.ends.corner {
        let [toggle, more] = corner(shell, cx);
        let buttons = crate::title_bar::top_row_buttons(shell, crate::title_bar::End::Start, cx);
        if notched {
            start = Some(on_frame(
                shell,
                &theme,
                buttons.into_iter().chain([toggle, more]),
            ));
        } else {
            line = line
                .children(buttons)
                .child(toggle)
                .child(more)
                .child(toolbar::separator(&theme));
        }
    }
    line = line
        .child(history_button(shell, row, false, nav_anchor, cx))
        .child(history_button(shell, row, true, nav_anchor, cx))
        .when_some(row.up, |line, above| {
            line.child(
                Button::new(
                    "up",
                    "up",
                    words!(
                        address_up_tip = "Up one level",
                        note = "The tooltip of the Up button: the folder that holds this one."
                    ),
                    "up",
                )
                .enabled(above)
                .render(shell, cx),
            )
        })
        .child(crate::elements::ground::hole(
            &row.holes,
            gpui::rems(0.375),
            theme.opacity.interface,
            field,
        ))
        .child(view_control(
            shell,
            wide,
            anchor,
            row.menu_open,
            &row.holes,
            cx,
        ));
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
    let mut end = None;
    if row.ends.toggles {
        let [split, properties, preview] = panel_toggles(shell, labeled, cx);
        let buttons = crate::title_bar::top_row_buttons(shell, crate::title_bar::End::End, cx);
        if notched {
            end = Some(on_frame(
                shell,
                &theme,
                [split, properties, preview].into_iter().chain(buttons),
            ));
        } else {
            line = line
                .child(toolbar::separator(&theme))
                .child(split)
                .child(properties)
                .child(preview)
                .children(buttons);
        }
    }
    if !notched {
        return line.into_any_element();
    }
    // The band between the notches: the card's ground, rounded where a
    // notch ends it. What it cannot hold is cut at its end, never drawn
    // over a notch.
    let band = line
        .relative()
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .px(sized(6.))
        .when(shell.drawn_raised == Some(true), |d| {
            d.child(underline(&theme))
        });
    let (from_start, to_end) = (start.is_some(), end.is_some());
    div()
        .size_full()
        .flex()
        .children(start)
        // Its ground, rounded where a notch ends it, once around its parts.
        .child(
            crate::elements::ground::ground(&row.holes, theme.bar_ground(shell.lowered()), band)
                .rounded(gpui::Corners {
                    top_left: gpui::rems(if from_start { 0.75 } else { 0. }),
                    top_right: gpui::rems(if to_end { 0.75 } else { 0. }),
                    ..gpui::Corners::default()
                }),
        )
        .children(end)
        .into_any_element()
}

/// The line under the raised pane's row (`render_pane`): the accent, two
/// pixels, along the row's foot.
fn underline(theme: &Theme) -> gpui::Div {
    div()
        .absolute()
        .left_0()
        .right_0()
        .bottom_0()
        .h(px(2.))
        .bg(theme.accent)
}

/// Whether a pane's row, as a card (`Theme::cards`), leaves what it holds
/// of the window's (`Ends`) on the frame, in notches of its card.
fn notched(theme: &Theme, ends: Ends) -> bool {
    theme.cards && (ends.corner || ends.toggles)
}

/// The window's buttons a pane's row holds, on the frame in a notch of its
/// card (`frame::notch`), the band's fillet beside them.
fn on_frame(
    shell: &Shell,
    theme: &Theme,
    buttons: impl IntoIterator<Item = AnyElement>,
) -> AnyElement {
    crate::elements::frame::notch(
        &shell.frame_cards,
        theme.bar_ground(shell.lowered()),
        div()
            .flex_none()
            .h_full()
            .flex()
            .items_center()
            .gap(sized(2.))
            .px(sized(6.))
            .children(buttons),
    )
    .into_any_element()
}

/// What a chevron of the trail before the folder `name` says: the folders
/// above it.
fn folders_above(name: String) -> Words {
    words!(
        address_folders_above = "Folders above { $folder }",
        folder = Folder(name),
        note = "The tooltip of a chevron of the address's trail: the folders that hold this one; folder: its name."
    )
}

/// What the field's Search button says over `location`, as Ctrl+F does
/// it: a search shown (`editing`) edited, a folder searched with its
/// subfolders, another place words can search (`App::searchable`: a tag,
/// every tag, Recent) searched. `None` where words cannot look (the
/// trash), or while no source searches them (`suggest::searched`: the
/// query plugin off).
pub fn search_title(
    app: &skwir_core::App,
    location: Option<&Location>,
    editing: bool,
) -> Option<Words> {
    if !skwir_core::suggest::searched(app) {
        return None;
    }
    if editing {
        return Some(words!(
            address_edit_search = "Edit this search",
            note = "The tooltip of the magnifier at the address's end over a search."
        ));
    }
    let location = location.filter(|l| app.searchable(l))?;
    Some(if location.is_path() {
        words!(
            address_search_folder = "Search this folder and its subfolders",
            note = "The tooltip of the magnifier at the address's end over a folder."
        )
    } else {
        words!(
            address_search_place = "Search { $place }",
            place = app.location_title(location),
            note = "The tooltip of the magnifier at the address's end over a place words can search (a tag, Recent); place: its name."
        )
    })
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
/// a reversed wrapping line one row tall (its first child at the line's
/// end, the right, the left mirrored), so the ones nearest the current
/// place stay and the outermost wrap out of sight in a narrow pane, with
/// nothing measured. Every segment but the outermost carries its chevron:
/// a trail that starts with one visibly continues toward its start.
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
        .aria_label(
            words!(
                address_path = "Path",
                note = "What a screen reader calls the address's trail of folders."
            )
            .say(),
        )
        .h(sized(ADDRESS_LINE))
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .flex()
        .flex_row_reverse()
        .flex_wrap()
        // `Start` is the window's start edge (the left, the right
        // mirrored) whatever the row's flex direction: taffy's `Start` and
        // `End` follow the layout direction, not the flex direction.
        .justify_start()
        .line_height(sized(ADDRESS_LINE));
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
            .h(sized(24.))
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
                crate::sides::symbol(if shown { "chevron" } else { "chevron-right" })
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
                gpui::SharedString::from(
                    // Its label plain: a name in these words, isolated
                    // there.
                    if crumb.location.is_path() {
                        folders_above(crumb.label.say_plain())
                    } else {
                        words!(
                            address_above = "Above { $place }",
                            place = Folder(crumb.label.say_plain()),
                            note = "The tooltip of a chevron of the address's trail: the places above this one; place: its name."
                        )
                    }
                    .say(),
                ),
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
            words!(
                address_in = "In { $place }",
                place = Folder(of.label.say_plain()),
                note = "The tooltip of a chevron of the address's trail: the folders and places inside this one; place: its name."
            )
            .say()
            .into(),
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
        // Plain: a name in the words of its chevron (`folders_above`).
        path.parent()
            .is_some()
            .then(|| (first.label.say_plain(), path))
    });
    for (index, crumb) in crumbs.iter().enumerate().rev() {
        let hover = theme.hover;
        let open = serde_json::json!(crumb.location.arg());
        let in_tab = open.clone();
        // What it is called, as the window says it.
        let said = crumb.label.say();
        let (text, clip) = crate::elements::clip::text(said.clone());
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
                d.aria_label(said.clone())
                    .when(index == last, |d| d.aria_selected(true))
            })
            .optional_tooltip(clip.tooltip(None))
            .max_w(sized(260.))
            .px(sized(6.))
            .flex()
            .items_center()
            .gap(sized(4.))
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
                        .h(sized(20.))
                        .line_height(sized(20.))
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
                None => d.h(sized(24.)).hover(move |s| s.bg(hover)),
            })
            .children(icon.map(|icon| {
                crate::sides::symbol(icon)
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
                    folders_above(label.clone()).say().into(),
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
    use super::{
        ViewButtons, ViewSpec, crumbs, goes_up, row_variants, tool_pane, toolbar_views, undo_words,
    };
    use crate::sidebar::Root;
    use skwir_core::location::{Location, ToolPane};
    use skwir_core::words::Words;
    use skwir_core::workspace::PaneId;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    /// The More menu's Undo and Redo name what each takes back, Undo of
    /// what has not ended saying it waits for it; with nothing to, the
    /// bare verb (the row is off).
    #[test]
    fn the_more_menu_s_undo_and_redo_say_what_they_take_back() {
        use skwir_core::words::Words;
        let english = |(undo, redo): (Words, Words)| (undo.english(), redo.english());
        let running = (Words::as_written("copy it's a.txt to Backup"), true);
        let ended = (Words::as_written("rename to final.txt"), false);
        assert_eq!(
            english(undo_words(
                Some(&ended),
                Some(&Words::as_written("new folder Reports"))
            )),
            (
                "Undo rename to final.txt".to_owned(),
                "Redo new folder Reports".to_owned()
            )
        );
        assert_eq!(
            undo_words(Some(&running), None).0.english(),
            "Undo copy it's a.txt to Backup (once it ends)"
        );
        assert_eq!(
            english(undo_words(None, None)),
            ("Undo".to_owned(), "Redo".to_owned())
        );
    }

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
            ViewSpec::named("list", Words::as_written("List"), "list", Arc::default()),
            ViewSpec::named("tree", Words::as_written("Tree"), "tree", Arc::default()).alternate(),
            ViewSpec::named("grid", Words::as_written("Grid"), "grid", Arc::default()),
            ViewSpec::named("usage", Words::as_written("Usage"), "usage", Arc::default()).lens(),
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

    /// With the view buttons off, no variant of the row has them, the
    /// toggles' labels are tried first only where the row holds them, and
    /// the field gives up its buttons last.
    #[test]
    fn the_row_without_view_buttons_has_the_one_button_only() {
        let variants = |buttons, toggles| -> Vec<(bool, bool, bool)> {
            row_variants(buttons, toggles)
                .into_iter()
                .map(|v| (v.wide, v.labeled, v.field_buttons))
                .collect()
        };
        assert_eq!(
            variants(true, true),
            [
                (true, true, true),
                (true, false, true),
                (false, false, true),
                (false, false, false)
            ]
        );
        assert_eq!(
            variants(true, false),
            [
                (true, false, true),
                (false, false, true),
                (false, false, false)
            ]
        );
        assert_eq!(
            variants(false, true),
            [
                (false, true, true),
                (false, false, true),
                (false, false, false)
            ]
        );
        assert_eq!(
            variants(false, false),
            [(false, false, true), (false, false, false)]
        );
    }

    /// Up goes where the `up` command would: a folder's to the one holding
    /// it, never above the root; another place's to the place before it in
    /// its trail, or to the folder it is of, from its first place.
    #[test]
    fn up_is_pressed_only_where_there_is_somewhere_to_go() {
        use skwir_core::location::{Crumb, Trail};
        let folder = Location::from_path("/x/photos");
        assert!(goes_up(Some(&folder), None));
        assert!(!goes_up(Some(&Location::from_path("/")), None));
        assert!(!goes_up(None, None));
        let tag = |name: &str| Location::new("tag", name);
        let deep = Trail {
            crumbs: vec![
                Crumb::new(Words::as_written("Places"), tag("places")),
                Crumb::new(Words::as_written("France"), tag("places/france")),
            ],
            ..Trail::default()
        };
        assert!(goes_up(Some(&tag("places/france")), Some(&deep)));
        let first = Trail {
            crumbs: vec![Crumb::new(Words::as_written("Places"), tag("places"))],
            ..Trail::default()
        };
        assert!(!goes_up(Some(&tag("places")), Some(&first)));
        let of_a_folder = Trail {
            base: Some(PathBuf::from("/x")),
            ..first
        };
        assert!(goes_up(Some(&tag("places")), Some(&of_a_folder)));
        assert!(!goes_up(Some(&tag("places")), None), "nothing read yet");
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
