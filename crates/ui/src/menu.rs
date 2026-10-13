//! Context menu from the registry's `context` menu contributions, filtered by
//! `when` tokens evaluated against the selection.

use std::collections::HashMap;
use std::ffi::OsStr;

use gpui::{
    AnyElement, Context, FocusHandle, MouseButton, Pixels, Point, Window, anchored, deferred, div,
    prelude::*, px,
};
use skwir_core::registry::{MenuItem, MenuSource};
use skwir_platform::mime::{self, Kind, Mime};

use crate::elements::popover::PopoverAnchor;
use crate::modal::FocusReturn;
use crate::shell::Shell;
use crate::theme::Theme;

/// What the selection looks like, and where it is, for `when` evaluation
/// and key contexts.
#[derive(Clone, Copy, Debug, Default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent flags, one per token of the `when` vocabulary"
)]
pub struct SelectionCtx {
    pub count: usize,
    pub all_dirs: bool,
    pub all_files: bool,
    /// Total size of the selected files, once every one's is known: a
    /// part is never shown as the total, so until then none
    /// (`Shell::sync_selection_hydration` asks for them).
    pub bytes: Option<u64>,
    /// The pane shows a folder, not a search, the shelf or another virtual
    /// location.
    pub folder: bool,
    /// Every selected entry is a place, no file (`EntryId::is_place`: a
    /// tag's in a tag location).
    pub all_places: bool,
    /// The location shown lists files kept elsewhere, which taking out
    /// leaves where they are (`Verbs::remove`): Delete runs `take_out`.
    pub refs: bool,
    /// The location shown takes files dropped or pasted (`Verbs::drop`).
    pub drops: bool,
    /// The location shown makes new places (`Verbs::create`).
    pub creates: bool,
    /// The location shown lists the trash's files, which it restores
    /// (`Verbs::restore`): Move to Trash does not apply there.
    pub restores: bool,
}

/// What `when` can say about the selection, for menu items and key bindings
/// alike: one vocabulary, one table. `empty` is nothing selected; `folder`
/// is the pane showing a folder, which an item about the folder shown (New
/// Folder, Bookmark This Folder) needs beside `empty`.
pub const SELECTION_TOKENS: [&str; 11] = [
    "empty",
    "selection",
    "single",
    "dir",
    "file",
    "folder",
    "place",
    "refs",
    "drops",
    "creates",
    "restores",
];

/// A token as the table spells it: `none` is the old spelling of `empty`.
pub fn canonical_token(token: &str) -> &str {
    if token == "none" { "empty" } else { token }
}

impl SelectionCtx {
    /// Whether one selection token holds; unknown tokens do not.
    pub fn holds(self, token: &str) -> bool {
        match canonical_token(token) {
            "empty" => self.count == 0,
            "selection" => self.count > 0,
            "single" => self.count == 1,
            "dir" => self.count > 0 && self.all_dirs,
            "file" => self.count > 0 && self.all_files,
            "folder" => self.folder,
            "place" => self.count > 0 && self.all_places,
            "refs" => self.refs,
            "drops" => self.drops,
            "creates" => self.creates,
            "restores" => self.restores,
            _ => false,
        }
    }

    /// Flags added to the pane key context: the tokens that hold.
    pub fn flags(self) -> Vec<&'static str> {
        SELECTION_TOKENS
            .into_iter()
            .filter(|token| self.holds(token))
            .collect()
    }
}

/// Space-separated tokens, all of which must hold; a token of
/// alternatives (`selection|folder`) holds when one of them does, and one
/// after `!` when it does not (`!refs`). `None` always holds. `kind:` and
/// `has:` ask `types` (`skwir_core::registry::MenuItem` has the
/// vocabulary).
pub fn when_holds(when: Option<&str>, ctx: SelectionCtx, types: &SelectionTypes) -> bool {
    when.is_none_or(|when| {
        when.split_whitespace()
            .all(|t| t.split('|').any(|alt| token_holds(alt, ctx, types)))
    })
}

fn token_holds(token: &str, ctx: SelectionCtx, types: &SelectionTypes) -> bool {
    if let Some(not) = token.strip_prefix('!') {
        return !token_holds(not, ctx, types);
    }
    match token.split_once(':') {
        Some(("kind", what)) => types.every(what),
        Some(("has", what)) => types.some(what),
        _ => ctx.holds(token),
    }
}

/// Whether `when` says anything about the selection's types, which are
/// worth working out only then.
pub fn asks_types(when: Option<&str>) -> bool {
    when.is_some_and(|when| when.contains("kind:") || when.contains("has:"))
}

/// What the selected entries are, as their names say (`mime::by_names`),
/// for the `kind:` and `has:` tokens. Made when a menu opens, never per
/// frame: typing names costs.
#[derive(Clone, Debug, Default)]
pub struct SelectionTypes {
    /// Each distinct type of the selection once.
    types: Vec<EntryType>,
}

#[derive(Clone, Debug, PartialEq)]
struct EntryType {
    dir: bool,
    /// The type and those it descends from, nearest first; empty when the
    /// name says nothing.
    lineage: Vec<Mime>,
    /// The last extension in lower case: `gpx`.
    extension: String,
}

impl SelectionTypes {
    /// The types of entries, each given by the name that says its type
    /// (`Entry::type_name`) and the type it has whatever its name
    /// (`Entry::inode_type`: a folder, a pipe, a device), which wins.
    pub fn of<'a>(entries: impl IntoIterator<Item = (&'a OsStr, Option<&'a str>)>) -> Self {
        let (inodes, files): (Vec<_>, Vec<_>) =
            entries.into_iter().partition(|(_, inode)| inode.is_some());
        let mut types = Vec::new();
        for inode in inodes.into_iter().filter_map(|(_, inode)| inode) {
            let ty = EntryType {
                dir: inode == "inode/directory",
                lineage: vec![Mime(inode.to_owned())],
                extension: String::new(),
            };
            if !types.contains(&ty) {
                types.push(ty);
            }
        }
        let names: Vec<&OsStr> = files.iter().map(|(name, _)| *name).collect();
        let mut lineages: HashMap<Mime, Vec<Mime>> = HashMap::new();
        for (name, mime) in names.iter().zip(mime::by_names(names.iter().copied())) {
            let extension = std::path::Path::new(name)
                .extension()
                .unwrap_or_default()
                .to_string_lossy()
                .to_ascii_lowercase();
            let lineage = mime.map_or_else(Vec::new, |m| {
                lineages.entry(m).or_insert_with_key(mime::lineage).clone()
            });
            let ty = EntryType {
                dir: false,
                lineage,
                extension,
            };
            if !types.contains(&ty) {
                types.push(ty);
            }
        }
        SelectionTypes { types }
    }

    /// `kind:`: every selected entry is `what`.
    fn every(&self, what: &str) -> bool {
        !self.types.is_empty() && self.types.iter().all(|t| t.is(what))
    }

    /// `has:`: one selected entry at least is `what`.
    fn some(&self, what: &str) -> bool {
        self.types.iter().any(|t| t.is(what))
    }
}

impl EntryType {
    /// Of `what`: an extension (`.gpx`), a MIME type or `image/*`, or a
    /// kind's word (`image`, `pictures`).
    fn is(&self, what: &str) -> bool {
        if let Some(extension) = what.strip_prefix('.') {
            return !self.dir && self.extension.eq_ignore_ascii_case(extension);
        }
        if let Some((top, sub)) = what.split_once('/') {
            if sub == "*" {
                return self.lineage.iter().any(|m| m.top_level() == top);
            }
            // By any of its names, whichever the database calls canonical
            // (a disc image is `application/x-cd-image` in one, an alias
            // of `application/vnd.efi.iso` in another).
            let named = mime::unalias(&Mime(what.to_ascii_lowercase()));
            return self.lineage.iter().any(|m| {
                m.as_str().eq_ignore_ascii_case(what)
                    || m.as_str().eq_ignore_ascii_case(named.as_str())
            });
        }
        match Kind::from_name(what) {
            Some(Kind::Folder) => self.dir,
            Some(kind) => !self.dir && self.lineage.first().is_some_and(|m| mime::is_kind(m, kind)),
            None => false,
        }
    }
}

pub struct MenuState {
    pub position: Point<Pixels>,
    pub items: Vec<MenuItem>,
    /// The menu owns the keyboard while it is open: otherwise keys reach the
    /// pane behind it and act on a selection the user can no longer see.
    pub focus: FocusHandle,
    /// Row under the keyboard cursor; the pointer moves it too.
    pub highlighted: Option<usize>,
    /// The submenu whose items are shown, in place, under its parent row.
    pub expanded: Option<String>,
    /// What had the keyboard before: a preview's menu gives it back to the
    /// preview, not to the pane.
    back: FocusReturn,
    /// Opened from the keyboard: no pointer says where, so the menu hangs
    /// from the cursor's row of this pane, or from the pane.
    pub keyboard: Option<KeyboardAnchor>,
    /// What is said beside an item in place of its binding's keys, by its
    /// index, where it is done another way: the drop menu's keys held
    /// while dropping.
    pub keys: Vec<Option<String>>,
}

/// Where a menu opened with the Menu key or Shift+F10 is drawn: under the
/// cursor's row of the pane or the sidebar it was opened in, when that
/// row is on screen, else at the top of the area. Both are measured in
/// the frame's prepaint (`elements::popover`).
pub struct KeyboardAnchor {
    pub host: MenuHost,
    pub row: PopoverAnchor,
    pub area: PopoverAnchor,
}

/// What a menu opened from the keyboard was opened in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuHost {
    Pane(skwir_core::workspace::PaneId),
    Sidebar,
}

impl KeyboardAnchor {
    /// Anchors for a menu opened in `host`, measured when next drawn.
    pub fn new(host: MenuHost) -> Self {
        KeyboardAnchor {
            host,
            row: PopoverAnchor::default(),
            area: PopoverAnchor::default(),
        }
    }
}

impl MenuState {
    /// Take the keyboard for a menu of `items` at `position`.
    pub(crate) fn open(
        position: Point<Pixels>,
        items: Vec<MenuItem>,
        window: &mut Window,
        cx: &mut Context<Shell>,
    ) -> Self {
        let back = FocusReturn::capture(window, cx);
        let focus = cx.focus_handle();
        window.focus(&focus);
        // GPUI focuses the focusable element under a mouse-down after our
        // handler ran (the pane, here) unless the event is marked handled;
        // without this the menu opens unfocused and keys reach the pane.
        window.prevent_default();
        MenuState {
            position,
            items,
            focus,
            highlighted: None,
            expanded: None,
            back,
            keyboard: None,
            keys: Vec::new(),
        }
    }
}

/// What the menu shows, top to bottom.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    /// Index into `MenuState::items`; `nested` items belong to a submenu.
    Item { index: usize, nested: bool },
    /// The parent row of a submenu.
    Parent(String),
}

impl MenuState {
    pub fn rows(&self) -> Vec<Row> {
        rows_of(&self.items, self.expanded.as_deref())
    }
}

/// Top-level items in order, then each submenu's parent row, followed by its
/// items while it is expanded. Expanding in place keeps one panel and one
/// linear keyboard order; a second popup would have to be a deferred draw
/// inside this one, which GPUI cannot nest.
pub fn rows_of(items: &[MenuItem], expanded: Option<&str>) -> Vec<Row> {
    let mut rows: Vec<Row> = items
        .iter()
        .enumerate()
        .filter(|(_, item)| item.submenu.is_none())
        .map(|(index, _)| Row::Item {
            index,
            nested: false,
        })
        .collect();
    let mut parents: Vec<&str> = Vec::new();
    for name in items.iter().filter_map(|item| item.submenu.as_deref()) {
        if !parents.contains(&name) {
            parents.push(name);
        }
    }
    for name in parents {
        rows.push(Row::Parent(name.to_owned()));
        if expanded == Some(name) {
            rows.extend(
                items
                    .iter()
                    .enumerate()
                    .filter(|(_, item)| item.submenu.as_deref() == Some(name))
                    .map(|(index, _)| Row::Item {
                        index,
                        nested: true,
                    }),
            );
        }
    }
    rows
}

/// The items, with each source's answer in place of what it replaces
/// (an empty answer replaces nothing), in order.
fn merge(mut items: Vec<MenuItem>, answers: Vec<(MenuSource, Vec<MenuItem>)>) -> Vec<MenuItem> {
    for (source, answer) in answers {
        if answer.is_empty() {
            continue;
        }
        items.retain(|m| !source.replaces.contains(&m.command));
        items.extend(answer);
    }
    items.sort_by_key(|m| m.order);
    items
}

impl Shell {
    pub fn open_context_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A row and its pane both see the right-click, and the pane's handler
        // focuses the pane first: keep the one menu and give it focus back.
        if let Some(menu) = self.menu.as_ref().filter(|menu| menu.position == position) {
            window.focus(&menu.focus);
            window.prevent_default();
            return;
        }
        let items = self.context_items();
        if items.is_empty() {
            return;
        }
        self.menu = Some(MenuState::open(position, items, window, cx));
        cx.notify();
    }

    /// The context menu's items for the selection: the registered items
    /// whose `when` holds, with what the sources that apply answer in place
    /// of the items they replace, in order.
    fn context_items(&self) -> Vec<MenuItem> {
        let ctx = self.selection_ctx();
        let registry = &self.app.registry;
        let statics: Vec<MenuItem> = registry
            .menus
            .all()
            .into_iter()
            .filter(|m| m.menu == "context")
            .collect();
        let sources: Vec<MenuSource> = registry
            .menu_sources
            .all()
            .into_iter()
            .filter(|s| s.menu == "context")
            .collect();
        let whens = statics.iter().map(|m| m.when.as_deref());
        let types = if whens
            .chain(sources.iter().map(|s| s.when.as_deref()))
            .any(asks_types)
        {
            self.selection_types()
        } else {
            SelectionTypes::default()
        };
        let mut items: Vec<MenuItem> = statics
            .into_iter()
            .filter(|m| when_holds(m.when.as_deref(), ctx, &types))
            .collect();
        items.extend(self.verb_items(ctx));
        items.push(self.properties_item());
        let answers = sources
            .into_iter()
            .filter(|source| when_holds(source.when.as_deref(), ctx, &types))
            .map(|source| {
                let answer = self.source_items(&source);
                (source, answer)
            })
            .collect();
        merge(items, answers)
    }

    /// What `source` answers for the active pane; nothing when it fails.
    fn source_items(&self, source: &MenuSource) -> Vec<MenuItem> {
        #[derive(serde::Deserialize)]
        struct Answer {
            items: Vec<MenuItem>,
        }
        let command = source.command.clone().with("pane", self.pane);
        match self.app.dispatch(&command) {
            Ok(skwir_core::command::Outcome::Value(value)) => {
                match serde_json::from_value::<Answer>(value) {
                    Ok(answer) => answer.items,
                    Err(e) => {
                        tracing::debug!(command = %command.name, "menu source answer: {e}");
                        Vec::new()
                    }
                }
            }
            Ok(_) => Vec::new(),
            Err(e) => {
                tracing::debug!(command = %command.name, "menu source: {e:#}");
                Vec::new()
            }
        }
    }

    /// The menu's last item, as other file managers have it: Properties of
    /// what it is about (the folder, on its background), or hiding them
    /// where they cover a preview. One command either way, so the item
    /// names its keys.
    fn properties_item(&self) -> MenuItem {
        let title = if self.properties_hide() {
            "Hide Properties"
        } else {
            "Properties"
        };
        MenuItem::new(
            "context",
            title,
            "info",
            490,
            skwir_core::command::Invocation::new("ui.properties"),
        )
    }

    /// `ui.context_menu` (Menu key, Shift+F10): the menu a right-click on
    /// the selection opens, or on the background with nothing selected,
    /// from the keyboard, its first item ready for Enter.
    /// What the location shown says its gestures are (`Verbs`), as
    /// items in its words: taking the selection out (what Delete does
    /// there, beside Move to Trash, which keeps its meaning) and making a
    /// new place, on the background.
    fn verb_items(&self, ctx: SelectionCtx) -> Vec<MenuItem> {
        let verbs = &self.ui().place.verbs;
        let item = |title: &str, command: &str, order: i32, icon: &str| {
            MenuItem::new(
                "context",
                title,
                icon,
                order,
                skwir_core::command::Invocation::new(command),
            )
        };
        let mut items = Vec::new();
        // A place selected alone has items of its own (a tag's: Remove Tag
        // From Every File).
        if let Some(remove) = verbs
            .remove
            .as_ref()
            .filter(|_| ctx.count > 0 && !ctx.all_places)
        {
            items.push(item(&remove.title, "take_out", 295, "minus"));
        }
        if let Some(create) = verbs.create.as_ref().filter(|_| ctx.count == 0) {
            items.push(item(&create.title, "ui.create_here", 200, "folder-plus"));
        }
        // The trash's files go back where they came from.
        if let Some(restore) = verbs.restore.as_ref().filter(|_| ctx.count > 0) {
            items.push(MenuItem {
                command: restore.clone().with("pane", self.pane),
                ..item("Restore", "", 1, "revert")
            });
        }
        items
    }

    pub fn open_context_menu_here(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self.cursor_index() {
            self.with_view(|v, _| v.scroll_to(ix));
        }
        self.menu = None;
        self.open_context_menu(Point::default(), window, cx);
        let pane = self.pane;
        if let Some(menu) = &mut self.menu {
            menu.highlighted = Some(0);
            menu.keyboard = Some(KeyboardAnchor::new(MenuHost::Pane(pane)));
        }
    }

    /// Where the keyboard's menu hangs from in `pane`: its cursor row
    /// (`cursor`) or the pane itself.
    pub fn menu_marker(
        &self,
        pane: skwir_core::workspace::PaneId,
        cursor: bool,
    ) -> Option<AnyElement> {
        self.host_marker(MenuHost::Pane(pane), cursor)
    }

    pub fn menu_area_marker(&self, pane: skwir_core::workspace::PaneId) -> Option<AnyElement> {
        self.host_area_marker(MenuHost::Pane(pane))
    }

    /// Where the keyboard's menu hangs from in `host`: the cursor's row
    /// there (`cursor`).
    pub fn host_marker(&self, host: MenuHost, cursor: bool) -> Option<AnyElement> {
        let anchor = self.menu.as_ref()?.keyboard.as_ref()?;
        (anchor.host == host && cursor).then(|| anchor.row.marker())
    }

    /// Where the keyboard's menu hangs from in `host` when its cursor's
    /// row is not drawn: the area itself.
    pub fn host_area_marker(&self, host: MenuHost) -> Option<AnyElement> {
        let anchor = self.menu.as_ref()?.keyboard.as_ref()?;
        (anchor.host == host).then(|| anchor.area.marker())
    }

    /// The menu of a tab of the strip.
    pub fn open_tab_menu(
        &mut self,
        tab: skwir_core::workspace::TabId,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let item = |title: &str, command, order, icon: &str| {
            MenuItem::new("tab", title, icon, order, command)
        };
        let tab_args = serde_json::json!({ "tab": tab });
        let items = vec![
            item(
                "New Tab",
                skwir_core::command::Invocation::new("tab.new"),
                200,
                "tab-new",
            ),
            item(
                "Reopen Closed Tab",
                skwir_core::command::Invocation::new("tab.reopen"),
                210,
                "revert",
            ),
            item(
                "Close Tab",
                skwir_core::command::Invocation::with_args("tab.close", &tab_args),
                300,
                "close",
            ),
        ];
        self.menu = Some(MenuState::open(position, items, window, cx));
        cx.notify();
    }

    /// A sidebar place's menu, for the place of `group` named `key`: Open
    /// and Open in New Tab, the place's own items (`Place::menu`: edit a
    /// saved search, a tag's color), Rename… and Remove From Sidebar from
    /// `Place::rename` and `Place::forget` for every place that has them,
    /// then how the sidebar arranges it: a place its owner offers is
    /// renamed, moved and hidden here, one suggested hidden, one kept
    /// moved through its owner. Eject last.
    pub fn open_place_menu(
        &mut self,
        group: &str,
        key: &str,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(arranged) = self.sidebar_place_of(group, key) else {
            return;
        };
        let place = &arranged.place;
        let item = |title: &str, command, order, icon: &str| {
            MenuItem::new("place", title, icon, order, command)
        };
        // While its owner works on it (unlocking, unmounting), the menu
        // offers nothing that would start over or race with that work,
        // as its row does.
        let busy = place.busy.is_some();
        let mut items = Vec::new();
        if !busy {
            items.push(item(
                "Open",
                crate::sidebar::open_invocation(place),
                0,
                "open",
            ));
            items.push(item(
                "Open in New Tab",
                crate::sidebar::open_in_tab(place),
                2,
                "tab-new",
            ));
        }
        items.extend(place.menu.iter().cloned());
        // What the sources of places' menus answer for it (a tag's pill:
        // the tag's own menu).
        items.extend(self.sourced(
            "place",
            serde_json::json!({ "location": place.location.arg() }),
        ));
        // Renamed by its owner (a bookmark, a saved search, a tag on every
        // file) or here, for one offered: F2 does the same.
        if let Some(rename) = self.sidebar_rename(&arranged) {
            items.push(item("Rename…", rename, 100, "rename"));
        }
        // A place the user kept is taken out by its owner (the bookmark
        // removed, the search forgotten, the tag unpinned), as the pane's
        // keep button does; one offered or suggested is hidden here.
        if let Some(forget) = place.forget.clone() {
            items.push(item("Remove From Sidebar", forget, 320, "minus"));
        }
        if let Some(up) = self.sidebar_move(&arranged, -1) {
            items.push(item("Move Up", up, 140, "up"));
        }
        if let Some(down) = self.sidebar_move(&arranged, 1) {
            items.push(item("Move Down", down, 141, "down"));
        }
        if arranged.role.hides() {
            items.push(item(
                "Hide From Sidebar",
                skwir_core::command::Invocation::with_args(
                    "ui.sidebar_place",
                    &serde_json::json!({ "group": group, "key": key, "hidden": true }),
                ),
                320,
                "eye-off",
            ));
        }
        if let Some(eject) = place.eject.as_ref().filter(|_| !busy) {
            items.push(item("Eject", eject.clone(), 350, "eject"));
        }
        if items.is_empty() {
            return;
        }
        items.sort_by_key(|m| m.order);
        self.menu = Some(MenuState::open(position, items, window, cx));
        cx.notify();
    }

    /// A tag's menu where a file shows it (Properties): the tag's own menu,
    /// as the sources of tags' menus answer it (the tags plugin's: the same
    /// as its pill's and its place's in a tag location).
    pub fn open_tag_menu(
        &mut self,
        tag: &str,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut items = self.sourced("tag", serde_json::json!({ "tags": [tag] }));
        if items.is_empty() {
            return;
        }
        items.sort_by_key(|m| m.order);
        self.menu = Some(MenuState::open(position, items, window, cx));
        cx.notify();
    }

    /// What every source of the menu `menu` answers, each asked with
    /// `args` beside its own (`MenuSource`).
    fn sourced(&self, menu: &str, args: serde_json::Value) -> Vec<MenuItem> {
        self.app
            .registry
            .menu_sources
            .all()
            .into_iter()
            .filter(|source| source.menu == menu)
            .flat_map(|mut source| {
                if let (Some(own), Some(given)) =
                    (source.command.args.as_object_mut(), args.as_object())
                {
                    own.extend(given.clone());
                }
                self.source_items(&source)
            })
            .collect()
    }

    /// The list header's own menu: which detail columns it shows. Choosing
    /// columns belongs where they are, not three clicks away in Settings, so
    /// the header answers a right-click with this instead of the dialog.
    pub fn open_column_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut items: Vec<MenuItem> = crate::views::list::COLUMNS
            .iter()
            .filter(|column| self.column_offered(column.detail))
            .enumerate()
            .map(|(i, column)| MenuItem {
                menu: "columns".into(),
                title: column.label.to_owned(),
                command: skwir_core::command::Invocation::with_args(
                    "ui.columns",
                    &serde_json::json!({ "key": column.setting }),
                ),
                when: None,
                order: i as i32,
                icon: self.columns[column.detail].then(|| "check".to_owned()),
                submenu: None,
            })
            .collect();
        items.push(MenuItem::new(
            "columns",
            "Reset widths",
            "revert",
            100,
            skwir_core::command::Invocation::new("ui.columns_reset"),
        ));
        self.menu = Some(MenuState::open(position, items, window, cx));
        cx.notify();
    }

    /// The preview's own menu: what people right-click a text for. Copy has
    /// no toolbar button; it lives here and on Ctrl+C.
    pub fn open_preview_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::preview::PreviewContent;
        let item = |order, title: &str, icon: &str, name: &str, args: serde_json::Value| {
            MenuItem::new(
                "preview",
                title,
                icon,
                order,
                skwir_core::command::Invocation::with_args(name, &args),
            )
        };
        let none = serde_json::json!({});
        // What a document and a structured preview are both asked, one
        // command each (`ui.preview_copy`…), as their keys are.
        let items = match &self.preview.content {
            Some(
                PreviewContent::Inspection(_)
                | PreviewContent::Image {
                    document: Some(_), ..
                },
            ) => vec![
                item(0, "Copy", "copy", "ui.preview_copy", none.clone()),
                item(
                    1,
                    "Select all",
                    "check",
                    "ui.preview_select_all",
                    none.clone(),
                ),
                item(2, "Find…", "search", "ui.preview_find", none.clone()),
                item(100, "Copy path", "copy", "ui.preview_copy_path", none),
            ],
            Some(_) => vec![item(100, "Copy path", "copy", "ui.preview_copy_path", none)],
            None => return,
        };
        self.menu = Some(MenuState::open(position, items, window, cx));
        cx.notify();
    }

    pub fn close_context_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(menu) = self.menu.take() {
            menu.back.restore(window, &self.ui().focus);
            cx.notify();
        }
    }

    /// Run the row, or open and close a submenu's parent.
    fn menu_activate(&mut self, row: &Row, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = self.menu.as_mut() else {
            return;
        };
        match row {
            Row::Parent(name) => {
                menu.expanded = (menu.expanded.as_ref() != Some(name)).then(|| name.clone());
                cx.notify();
            }
            Row::Item { index, .. } => {
                let Some(command) = menu.items.get(*index).map(|item| item.command.clone()) else {
                    return;
                };
                self.close_context_menu(window, cx);
                self.run(&command.name, command.args, window, cx);
            }
        }
    }

    /// `ui.menu_*`: keyboard control of the open context menu.
    pub fn menu_command(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = self.menu.as_mut() else {
            return;
        };
        let rows = menu.rows();
        let last = rows.len().saturating_sub(1);
        let current = menu.highlighted.and_then(|i| rows.get(i).cloned());
        match name {
            "menu_close" => self.close_context_menu(window, cx),
            "menu_down" => {
                menu.highlighted = Some(menu.highlighted.map_or(0, |i| (i + 1).min(last)));
            }
            "menu_up" => {
                menu.highlighted = Some(menu.highlighted.map_or(last, |i| i.saturating_sub(1)));
            }
            "menu_first" => menu.highlighted = Some(0),
            "menu_last" => menu.highlighted = Some(last),
            "menu_expand" => {
                if let Some(Row::Parent(name)) = current {
                    menu.expanded = Some(name);
                }
            }
            "menu_collapse" => {
                if let Some(name) = menu.expanded.take() {
                    menu.highlighted = menu
                        .rows()
                        .iter()
                        .position(|row| *row == Row::Parent(name.clone()));
                }
            }
            "menu_activate" => {
                if let Some(row) = current {
                    self.menu_activate(&row, window, cx);
                }
            }
            _ => {}
        }
        cx.notify();
    }
}

pub fn render(shell: &Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let menu = shell.menu.as_ref()?;
    let theme = cx.global::<Theme>().clone();
    let mut panel = div()
        .min_w(px(220.))
        .py_1()
        .rounded_lg()
        .border_1()
        .border_color(theme.border)
        .bg(theme.surface)
        .shadow_md()
        .occlude()
        .track_focus(&menu.focus)
        .key_context(crate::keymap::MENU)
        .on_mouse_down_out(cx.listener(|this, _, window, cx| {
            this.close_context_menu(window, cx);
        }));
    let mut previous: Option<&MenuItem> = None;
    for (i, row) in menu.rows().into_iter().enumerate() {
        let highlighted = menu.highlighted == Some(i);
        let muted = if highlighted {
            theme.selection_text
        } else {
            theme.text_muted
        };
        let (title, icon, keys, nested, separated) = match &row {
            Row::Item { index, nested } => {
                let item = &menu.items[*index];
                // Separator between order groups (hundreds digit), per the
                // convention on `skwir_core::registry::MenuItem`.
                let separated = !nested
                    && previous.is_some_and(|before| before.order / 100 != item.order / 100);
                if !nested {
                    previous = Some(item);
                }
                let keys = menu
                    .keys
                    .get(*index)
                    .cloned()
                    .flatten()
                    .or_else(|| shell.keys_here(&item.command.name, &item.command.args));
                (
                    item.title.clone(),
                    item.icon.clone(),
                    keys,
                    *nested,
                    separated,
                )
            }
            Row::Parent(name) => (name.clone(), None, None, false, true),
        };
        if separated {
            panel = panel.child(div().my_1().h(px(1.)).bg(theme.border));
        }
        let expanded = matches!(&row, Row::Parent(name) if menu.expanded.as_ref() == Some(name));
        let is_parent = matches!(row, Row::Parent(_));
        let icon =
            icon.filter(|name| crate::toolbar::asset(&format!("toolbar/{name}.svg")).is_some());
        panel = panel.child(
            div()
                .id(("menu", i))
                .mx_1()
                .px_2()
                .py_1()
                .when(nested, gpui::Styled::pl_6)
                .flex()
                .items_center()
                .gap_2()
                .rounded_sm()
                .cursor_pointer()
                .when(highlighted, |d| {
                    d.bg(theme.selection_bg).text_color(theme.selection_text)
                })
                .on_mouse_move(cx.listener(move |this, _, _, cx| {
                    if let Some(menu) = this.menu.as_mut()
                        && menu.highlighted != Some(i)
                    {
                        menu.highlighted = Some(i);
                        cx.notify();
                    }
                }))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _, window, cx| {
                    if let (Row::Item { index, .. }, Some(menu)) = (&row, this.menu.as_ref())
                        && let Some(command) =
                            menu.items.get(*index).map(|item| item.command.clone())
                    {
                        this.note_pointer_use(&command.name, &command.args, cx);
                    }
                    this.menu_activate(&row, window, cx);
                }))
                // An icon column, empty where an item has none, keeps titles aligned.
                .child(div().size(px(15.)).flex_none().children(icon.map(|name| {
                    gpui::svg()
                        .path(format!("toolbar/{name}.svg"))
                        .size(px(15.))
                        .text_color(muted)
                })))
                .child(div().flex_1().child(title))
                .children(keys.map(|keys| div().pl_6().text_xs().text_color(muted).child(keys)))
                .when(is_parent, |d| {
                    d.child(
                        gpui::svg()
                            .path(if expanded {
                                "toolbar/chevron.svg"
                            } else {
                                "toolbar/chevron-right.svg"
                            })
                            .size(px(13.))
                            .text_color(muted),
                    )
                }),
        );
    }
    if let Some(anchor) = &menu.keyboard {
        let anchors = [anchor.row.clone(), anchor.area.clone()];
        return Some(PopoverAnchor::popup_first(
            &anchors,
            2,
            panel.into_any_element(),
        ));
    }
    Some(
        deferred(
            anchored()
                .position(menu.position)
                .snap_to_window_with_margin(px(8.))
                .child(panel),
        )
        .with_priority(2)
        .into_any_element(),
    )
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "linux")]
    use std::ffi::OsStr;

    use skwir_core::command::Invocation;
    use skwir_core::registry::{MenuItem, MenuSource};

    use super::{Row, SelectionCtx, SelectionTypes, rows_of};

    /// `when` without types: nothing is known of the selection's.
    fn when_holds(when: Option<&str>, ctx: SelectionCtx) -> bool {
        super::when_holds(when, ctx, &SelectionTypes::default())
    }

    #[cfg(target_os = "linux")]
    fn types(entries: &[(&str, bool)]) -> SelectionTypes {
        SelectionTypes::of(
            entries
                .iter()
                .map(|(name, dir)| (OsStr::new(*name), dir.then_some("inode/directory"))),
        )
    }

    #[test]
    fn a_source_answering_takes_the_place_of_what_it_replaces() {
        let at = |title: &str, name: &str, order| MenuItem {
            command: Invocation::new(name),
            order,
            ..item(title, None)
        };
        let statics = vec![
            at("Open", "open", 0),
            at("Open With…", "choose", 1),
            at("Copy", "copy", 120),
        ];
        let source = MenuSource {
            menu: "context".into(),
            when: None,
            command: Invocation::new("source"),
            replaces: vec![Invocation::new("open"), Invocation::new("choose")],
        };
        let titles =
            |items: Vec<MenuItem>| -> Vec<String> { items.into_iter().map(|m| m.title).collect() };
        let answer = vec![
            at("Open With Viewer", "open_with", 0),
            at("Other…", "choose", 1),
        ];
        assert_eq!(
            titles(super::merge(
                statics.clone(),
                vec![(source.clone(), answer)]
            )),
            ["Open With Viewer", "Other…", "Copy"]
        );
        assert_eq!(
            titles(super::merge(statics, vec![(source, Vec::new())])),
            ["Open", "Open With…", "Copy"],
            "no answer: the registered items stay"
        );
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn types_say_what_every_or_some_selected_entry_is() {
        let holds = |when: &str, entries: &[(&str, bool)]| {
            let ctx = ctx(entries.len(), false, false);
            super::when_holds(Some(when), ctx, &types(entries))
        };
        let photos = [("a.jpg", false), ("b.PNG", false)];
        assert!(holds("kind:image", &photos));
        assert!(holds("kind:pictures", &photos), "a kind's synonym");
        assert!(holds("has:image/*", &photos));
        assert!(holds("has:image/png", &photos));
        assert!(!holds("kind:image/png", &photos));
        assert!(!holds("kind:video", &photos));
        let with_track = [("a.jpg", false), ("walk.GPX", false)];
        assert!(!holds("kind:image", &with_track));
        assert!(holds("has:image has:.gpx", &with_track));
        assert!(holds("has:image has:.fit|has:.gpx", &with_track));
        assert!(!holds("has:image has:.fit", &with_track));
        // Code is text through its parents; a folder is of no file type.
        assert!(holds(
            "kind:text",
            &[("main.rs", false), ("notes.txt", false)]
        ));
        assert!(holds("has:text/plain", &[("main.rs", false)]));
        assert!(holds("kind:folder", &[("src", true)]));
        assert!(!holds("has:image", &[("x.png", true)]));
        assert!(!holds("has:.png", &[("x.png", true)]));
        // A type by any of its names: a disc image's old one, whichever
        // the database calls canonical.
        let disc = [("disc.iso", false)];
        assert!(holds("kind:application/x-cd-image", &disc));
        assert!(holds("kind:application/x-iso9660-image", &disc));
        // A name that says nothing is of no kind.
        let unknown = [("a.jpg", false), ("blob.zzq9", false)];
        assert!(!holds("kind:image", &unknown));
        assert!(holds("has:image", &unknown));
        // Nothing selected: nothing holds.
        assert!(!holds("kind:image", &[]));
        assert!(!holds("has:image", &[]));
        assert!(!holds("kind:bogus", &photos));
    }
    fn item(title: &str, submenu: Option<&str>) -> MenuItem {
        MenuItem {
            menu: "context".into(),
            title: title.into(),
            command: Invocation::new("noop"),
            when: None,
            order: 0,
            icon: None,
            submenu: submenu.map(str::to_owned),
        }
    }

    #[test]
    fn submenu_items_wait_under_their_parent_until_it_is_expanded() {
        let items = [
            item("Open", None),
            item("Copy Paths", Some("More")),
            item("Rename", None),
            item("Run Command…", Some("More")),
        ];
        let top = |index| Row::Item {
            index,
            nested: false,
        };
        let nested = |index| Row::Item {
            index,
            nested: true,
        };
        assert_eq!(
            rows_of(&items, None),
            [top(0), top(2), Row::Parent("More".into())]
        );
        assert_eq!(
            rows_of(&items, Some("More")),
            [
                top(0),
                top(2),
                Row::Parent("More".into()),
                nested(1),
                nested(3)
            ]
        );
    }

    fn ctx(count: usize, all_dirs: bool, all_files: bool) -> SelectionCtx {
        SelectionCtx {
            count,
            all_dirs,
            all_files,
            ..SelectionCtx::default()
        }
    }

    #[test]
    fn when_evaluation() {
        assert!(when_holds(None, ctx(0, true, true)));
        assert!(when_holds(Some("none"), ctx(0, true, true)), "old spelling");
        assert!(!when_holds(Some("none"), ctx(1, true, true)));
        assert!(!when_holds(Some("selection"), ctx(0, true, true)));
        assert!(when_holds(Some("selection"), ctx(2, false, true)));
        assert!(!when_holds(Some("single"), ctx(2, false, true)));
        assert!(when_holds(Some("single file"), ctx(1, false, true)));
        assert!(!when_holds(Some("single dir"), ctx(1, false, true)));
        assert!(!when_holds(Some("bogus"), ctx(1, true, true)));
        let folder = SelectionCtx {
            folder: true,
            ..ctx(0, true, true)
        };
        assert!(when_holds(Some("empty folder"), folder));
        assert!(
            !when_holds(Some("empty folder"), ctx(0, true, true)),
            "a search"
        );
        // Alternatives: a selection anywhere, or the folder itself.
        assert!(when_holds(Some("selection|folder"), folder));
        assert!(when_holds(Some("selection|folder"), ctx(1, false, true)));
        assert!(!when_holds(Some("selection|folder"), ctx(0, true, true)));
        assert!(!when_holds(Some("single selection|folder"), folder));
        assert!(when_holds(Some("empty"), ctx(0, true, true)));
        assert!(!when_holds(Some("empty"), ctx(1, true, true)));
        // A token after `!` holds when it does not.
        let places = SelectionCtx {
            all_places: true,
            refs: true,
            ..ctx(2, true, false)
        };
        assert!(when_holds(Some("place"), places));
        assert!(!when_holds(Some("selection !place"), places));
        assert!(when_holds(Some("selection !place"), ctx(2, true, false)));
        assert!(when_holds(Some("selection refs"), places));
        assert!(!when_holds(Some("selection !refs"), places));
        assert!(
            !when_holds(
                Some("place"),
                SelectionCtx {
                    all_places: true,
                    ..ctx(0, true, true)
                }
            ),
            "nothing selected is no place"
        );
    }

    #[test]
    fn flags_follow_selection() {
        assert_eq!(ctx(0, true, true).flags(), ["empty"]);
        assert_eq!(ctx(1, true, false).flags(), ["selection", "single", "dir"]);
        assert_eq!(ctx(3, false, true).flags(), ["selection", "file"]);
    }
}
