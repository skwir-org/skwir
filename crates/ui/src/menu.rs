//! Context menu from the registry's `context` menu contributions, filtered by
//! `when` tokens evaluated against the selection.

use std::collections::HashMap;
use std::ffi::OsStr;

use gpui::{
    AnyElement, Context, FocusHandle, MouseButton, Pixels, Point, ScrollHandle, Window, div,
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
    /// The pane shows a search (`LocationProvider::searches`), whose pills
    /// take the trail's place.
    pub search: bool,
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
/// Folder, Bookmark This Folder) needs beside `empty`; `search` is the pane
/// showing a search.
pub const SELECTION_TOKENS: [&str; 12] = [
    "empty",
    "selection",
    "single",
    "dir",
    "file",
    "folder",
    "search",
    "place",
    "refs",
    "drops",
    "creates",
    "restores",
];

impl SelectionCtx {
    /// Whether one selection token holds; unknown tokens do not.
    pub fn holds(self, token: &str) -> bool {
        match token {
            "empty" => self.count == 0,
            "selection" => self.count > 0,
            "single" => self.count == 1,
            "dir" => self.count > 0 && self.all_dirs,
            "file" => self.count > 0 && self.all_files,
            "folder" => self.folder,
            "search" => self.search,
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
    /// The submenu shown beside its row (`Row::Parent`), and the row of it
    /// the keyboard is on: `None` while the keyboard is in the menu's own
    /// rows, the submenu open beside them.
    pub open: Option<String>,
    pub sub_highlighted: Option<usize>,
    /// Where the open submenu's row stands, and where the submenu is
    /// drawn: both measured in prepaint (`PopoverAnchor`). A press there is
    /// no press outside the menu.
    sub_row: PopoverAnchor,
    sub_area: PopoverAnchor,
    sub_scroll: ScrollHandle,
    /// The pointer resting on a row: the submenu it opens, or the one it
    /// closes, once it has rested `REST` (`MenuState::rest_on`).
    rest: Option<gpui::Task<()>>,
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
    /// The rows of a menu taller than the window scroll (it is never
    /// taller: `PopoverAnchor::popup_at`), and the keyboard's row is
    /// brought into view (`MenuState::reveal`).
    scroll: ScrollHandle,
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
    /// Its row is the cursor's cell, its area the panel.
    Terminal,
}

impl MenuHost {
    /// The menu stands above its anchor: the terminal is at the window's
    /// foot, with no room under it.
    fn above(self) -> bool {
        self == MenuHost::Terminal
    }
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
        window.focus(&focus, cx);
        // GPUI focuses the focusable element under a mouse-down after our
        // handler ran (the pane, here) unless the event is marked handled;
        // without this the menu opens unfocused and keys reach the pane.
        window.prevent_default();
        MenuState {
            position,
            items,
            focus,
            highlighted: None,
            open: None,
            sub_highlighted: None,
            sub_row: PopoverAnchor::default(),
            sub_area: PopoverAnchor::default(),
            sub_scroll: ScrollHandle::new(),
            rest: None,
            back,
            keyboard: None,
            keys: Vec::new(),
            scroll: ScrollHandle::new(),
        }
    }
}

/// What the menu shows, top to bottom.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    /// Index into `MenuState::items`.
    Item(usize),
    /// The row of a submenu, which opens beside it.
    Parent(String),
}

/// How long the pointer rests on a row before the submenu it opens opens,
/// or the one it leaves closes: long enough to cross another row on the
/// way to a submenu, short enough not to wait for it.
const REST: std::time::Duration = std::time::Duration::from_millis(250);

impl MenuState {
    pub fn rows(&self) -> Vec<Row> {
        rows_of(&self.items)
    }

    fn drawn(&self) -> Vec<(Row, bool)> {
        drawn_of(&self.items, &self.rows())
    }

    /// The open submenu's items, by index into `items`.
    fn sub_rows(&self) -> Vec<usize> {
        self.open
            .as_deref()
            .map(|name| sub_rows_of(&self.items, name))
            .unwrap_or_default()
    }

    fn sub_drawn(&self) -> Vec<(Row, bool)> {
        let rows: Vec<Row> = self.sub_rows().into_iter().map(Row::Item).collect();
        drawn_of(&self.items, &rows)
    }

    /// The keyboard reached `row`: the rows scroll the least that shows
    /// it, in the prepaint of the frame that draws it.
    fn reveal(&self, row: usize) {
        self.scroll.scroll_to_item(child_of(&self.drawn(), row));
    }

    fn reveal_sub(&self, row: usize) {
        self.sub_scroll
            .scroll_to_item(child_of(&self.sub_drawn(), row));
    }

    /// Show `name`'s submenu beside its row, the keyboard left where it is.
    fn open_sub(&mut self, name: String) {
        if self.open.as_ref() != Some(&name) {
            self.open = Some(name);
            self.sub_highlighted = None;
            self.sub_area = PopoverAnchor::default();
            self.sub_scroll = ScrollHandle::new();
        }
    }

    fn close_sub(&mut self) {
        self.open = None;
        self.sub_highlighted = None;
        self.sub_area = PopoverAnchor::default();
    }

    /// Whether a press at `at` lands on the open submenu, which is part of
    /// the menu though drawn beside it.
    fn in_sub(&self, at: Point<Pixels>) -> bool {
        self.open.is_some()
            && self
                .sub_area
                .bounds()
                .is_some_and(|area| area.contains(&at))
    }
}

/// The pointer rests on the menu's `row`: once it has rested `REST`, the
/// submenu that row opens opens, or, on another row, the one open closes.
/// A row of the submenu itself, reached in time, keeps it (`rest` dropped).
fn rest_on(shell: &mut Shell, row: &Row, window: &mut Window, cx: &mut Context<Shell>) {
    let Some(menu) = shell.menu.as_mut() else {
        return;
    };
    let opens = match row {
        Row::Parent(name) => Some(name.clone()),
        Row::Item(_) => None,
    };
    if menu.open == opens {
        menu.rest = None;
        return;
    }
    menu.rest = Some(cx.spawn_in(window, async move |this, cx| {
        cx.background_executor().timer(REST).await;
        let _ = this.update(cx, |this, cx| {
            if let Some(menu) = this.menu.as_mut() {
                menu.rest = None;
                match opens {
                    Some(name) => menu.open_sub(name),
                    None => menu.close_sub(),
                }
                cx.notify();
            }
        });
    }));
}

/// `rows` drawn, each with whether a separator stands above it: between
/// order groups (the hundreds digit, per the convention on
/// `skwir_core::registry::MenuItem`), above the first submenu's row, and
/// after the last.
fn drawn_of(items: &[MenuItem], rows: &[Row]) -> Vec<(Row, bool)> {
    let mut previous: Option<&MenuItem> = None;
    let mut after_parent = false;
    rows.iter()
        .map(|row| {
            let separated = match row {
                Row::Item(index) => {
                    let item = &items[*index];
                    let separated = after_parent
                        || previous.is_some_and(|before| before.order / 100 != item.order / 100);
                    previous = Some(item);
                    after_parent = false;
                    separated
                }
                Row::Parent(_) => {
                    let first = !after_parent && previous.is_some();
                    after_parent = true;
                    first
                }
            };
            (row.clone(), separated)
        })
        .collect()
}

/// Which child of the menu's rows `row` is drawn as: the separators above
/// it are children too.
fn child_of(drawn: &[(Row, bool)], row: usize) -> usize {
    let separators = drawn
        .iter()
        .take(row + 1)
        .filter(|(_, separated)| *separated)
        .count();
    row + separators
}

/// The menu's own rows: its items in order, then a row for each submenu,
/// which opens beside it (`render`'s `popup_beside`), then the menu's end
/// (order 9xx: Properties). A submenu holds no submenu.
pub fn rows_of(items: &[MenuItem]) -> Vec<Row> {
    let top = |last: bool| {
        items
            .iter()
            .enumerate()
            .filter(move |(_, item)| item.submenu.is_none() && (item.order >= LAST) == last)
            .map(|(index, _)| Row::Item(index))
    };
    let mut rows: Vec<Row> = top(false).collect();
    let mut parents: Vec<&str> = Vec::new();
    for name in items.iter().filter_map(|item| item.submenu.as_deref()) {
        if !parents.contains(&name) {
            parents.push(name);
        }
    }
    rows.extend(parents.into_iter().map(|name| Row::Parent(name.to_owned())));
    rows.extend(top(true));
    rows
}

/// The items of the submenu `name`, in order.
fn sub_rows_of(items: &[MenuItem], name: &str) -> Vec<usize> {
    items
        .iter()
        .enumerate()
        .filter(|(_, item)| item.submenu.as_deref() == Some(name))
        .map(|(index, _)| index)
        .collect()
}

/// The order from which an item ends the menu, after the submenus
/// (`skwir_core::registry::MenuItem`'s convention).
const LAST: i32 = 900;

/// The command of the menu's Properties item.
const PROPERTIES: &str = "ui.properties";

/// The words of the menu's Properties item as things stand: what a click
/// does now. Read at each frame, not when the menu opens: a right-click on
/// a row not selected selects it as the menu opens, before the panel
/// shows it, and the item speaks of the entry clicked.
fn properties_title(shell: &Shell) -> &'static str {
    if shell.properties_hide() {
        "Hide Properties"
    } else {
        "Properties"
    }
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
            window.focus(&menu.focus, cx);
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
    /// names its keys; its words are read again at each frame
    /// (`properties_title`).
    fn properties_item(&self) -> MenuItem {
        MenuItem::new(
            "context",
            properties_title(self),
            "info",
            LAST,
            skwir_core::command::Invocation::new(PROPERTIES),
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

    /// The anchor of the keyboard's menu's row in `host`, for an element
    /// that draws its rows itself (the terminal's grid) to mark in its
    /// prepaint.
    pub fn host_row_anchor(&self, host: MenuHost) -> Option<PopoverAnchor> {
        let anchor = self.menu.as_ref()?.keyboard.as_ref()?;
        (anchor.host == host).then(|| anchor.row.clone())
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

    /// A tag's menu where a file shows it (an entry's chip, Properties,
    /// the preview's header): the tag's own menu, as the sources of tags'
    /// menus answer it (the tags plugin's: the same as its pill's and its
    /// place's in a tag location). `files`: shown on the selected files,
    /// which the menu then also acts on.
    pub fn open_tag_menu(
        &mut self,
        tag: &str,
        files: bool,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut items = self.sourced("tag", serde_json::json!({ "tags": [tag], "files": files }));
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

    /// The terminal's menu: copy, paste and select all; on a link, its
    /// own items; clearing the history. `at` is where the pointer opened
    /// it, `None` from the keyboard (the Menu key: hung above the cursor,
    /// its first item ready for Enter).
    pub fn open_terminal_menu(
        &mut self,
        at: Option<Point<Pixels>>,
        link: Option<&skwir_core::terminal::Link>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use skwir_core::terminal::Target;
        if !self.terminal.open {
            return;
        }
        let item = |order, title: &str, icon: &str, name: &str, args: serde_json::Value| {
            MenuItem::new(
                "terminal",
                title,
                icon,
                order,
                skwir_core::command::Invocation::with_args(name, &args),
            )
        };
        let none = serde_json::json!({});
        let mut items = Vec::new();
        if let Some(link) = link {
            // What a program wrote as a link may say something else than
            // where it leads: the item says where (ADR-029).
            let follow = if link.written {
                format!("Follow link to {}", crate::links::shortened(&link.target))
            } else {
                "Follow link".to_owned()
            };
            match &link.target {
                Target::Uri(uri) => {
                    let url = serde_json::json!({ "url": uri });
                    items.push(item(0, &follow, "external", "ui.open_url", url));
                    let text = serde_json::json!({ "text": uri, "what": "the link's address" });
                    items.push(item(110, "Copy link address", "link", "ui.copy_text", text));
                }
                Target::Path { path, line, .. } => {
                    let reveal = |tab: bool| {
                        let mut args = serde_json::json!({
                            "path": skwir_core::os_path::PathArg::from(path.clone()),
                            "tab": tab,
                        });
                        if let Some(line) = line {
                            args["line"] = (*line).into();
                        }
                        args
                    };
                    items.push(item(0, &follow, "folder", "ui.reveal", reveal(false)));
                    items.push(item(
                        1,
                        "Follow link in a new tab",
                        "tab-new",
                        "ui.reveal",
                        reveal(true),
                    ));
                    // A name the clipboard cannot hold as text has no item.
                    if let Some(text) = path.to_str() {
                        let text = serde_json::json!({ "text": text, "what": "the path" });
                        items.push(item(110, "Copy path", "link", "ui.copy_text", text));
                    }
                }
            }
        }
        if self.terminal_selected() {
            items.push(item(100, "Copy", "copy", "ui.terminal_copy", none.clone()));
        }
        items.push(item(
            101,
            "Paste",
            "paste",
            "ui.terminal_paste",
            none.clone(),
        ));
        items.push(item(
            102,
            "Select all",
            "check",
            "ui.terminal_select_all",
            none.clone(),
        ));
        if self.terminal_history() > 0 {
            items.push(item(
                300,
                "Clear the history",
                "close",
                "ui.terminal_clear",
                none,
            ));
        }
        items.sort_by_key(|item| item.order);
        let keyboard = at.is_none();
        self.menu = Some(MenuState::open(at.unwrap_or_default(), items, window, cx));
        if keyboard && let Some(menu) = &mut self.menu {
            menu.highlighted = Some(0);
            menu.keyboard = Some(KeyboardAnchor::new(MenuHost::Terminal));
        }
        cx.notify();
    }

    pub fn close_context_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(menu) = self.menu.take() {
            menu.back.restore(window, &self.ui().focus, cx);
            cx.notify();
        }
    }

    /// Run the row, or open its submenu beside it.
    fn menu_activate(&mut self, row: &Row, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = self.menu.as_mut() else {
            return;
        };
        match row {
            Row::Parent(name) => {
                menu.rest = None;
                menu.open_sub(name.clone());
                cx.notify();
            }
            Row::Item(index) => {
                let Some(command) = menu.items.get(*index).map(|item| item.command.clone()) else {
                    return;
                };
                self.close_context_menu(window, cx);
                self.run(&command.name, command.args, window, cx);
            }
        }
    }

    /// `ui.menu_*`: keyboard control of the open context menu. In the
    /// submenu once Right, Enter or Space opened it from its row: Left or
    /// Escape go back to that row, the submenu closed.
    pub fn menu_command(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = self.menu.as_mut() else {
            return;
        };
        // The keys take over from the pointer resting somewhere.
        menu.rest = None;
        if let Some(at) = menu.sub_highlighted {
            let rows = menu.sub_rows();
            let last = rows.len().saturating_sub(1);
            match name {
                "menu_close" | "menu_collapse" => menu.close_sub(),
                "menu_down" => menu.sub_highlighted = Some((at + 1).min(last)),
                "menu_up" => menu.sub_highlighted = Some(at.saturating_sub(1)),
                "menu_first" => menu.sub_highlighted = Some(0),
                "menu_last" => menu.sub_highlighted = Some(last),
                "menu_activate" => {
                    if let Some(&index) = rows.get(at) {
                        self.menu_activate(&Row::Item(index), window, cx);
                    }
                }
                _ => {}
            }
            if let Some(menu) = &self.menu
                && let Some(row) = menu.sub_highlighted
            {
                menu.reveal_sub(row);
            }
            cx.notify();
            return;
        }
        let rows = menu.rows();
        let last = rows.len().saturating_sub(1);
        let current = menu.highlighted.and_then(|i| rows.get(i).cloned());
        match name {
            // A submenu the pointer opened closes first.
            "menu_close" if menu.open.is_some() => menu.close_sub(),
            "menu_close" => self.close_context_menu(window, cx),
            "menu_down" => {
                menu.close_sub();
                menu.highlighted = Some(menu.highlighted.map_or(0, |i| (i + 1).min(last)));
            }
            "menu_up" => {
                menu.close_sub();
                menu.highlighted = Some(menu.highlighted.map_or(last, |i| i.saturating_sub(1)));
            }
            "menu_first" => {
                menu.close_sub();
                menu.highlighted = Some(0);
            }
            "menu_last" => {
                menu.close_sub();
                menu.highlighted = Some(last);
            }
            "menu_expand" | "menu_activate" if matches!(current, Some(Row::Parent(_))) => {
                if let Some(Row::Parent(name)) = current {
                    menu.open_sub(name);
                    menu.sub_highlighted = Some(0);
                }
            }
            "menu_collapse" => menu.close_sub(),
            "menu_activate" => {
                if let Some(row) = current {
                    self.menu_activate(&row, window, cx);
                }
            }
            _ => {}
        }
        if let Some(menu) = &self.menu {
            if let Some(row) = menu.highlighted {
                menu.reveal(row);
            }
            if let Some(row) = menu.sub_highlighted {
                menu.reveal_sub(row);
            }
        }
        cx.notify();
    }
}

pub fn render(shell: &Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let menu = shell.menu.as_ref()?;
    let theme = cx.global::<Theme>().clone();
    // A menu to assistive technology, its highlighted row said as focused
    // while the menu keeps the keyboard.
    let panel = div()
        .id("context-menu")
        .role(gpui::Role::Menu)
        .aria_label("Context menu")
        .min_w(px(220.))
        .p_1()
        .flex()
        .flex_col()
        .min_h_0()
        .rounded_lg()
        .border_1()
        .border_color(theme.border)
        .bg(theme.surface)
        .shadow_md()
        .occlude()
        .track_focus(&menu.focus)
        .key_context(crate::keymap::MENU)
        // A press on the submenu beside it is no press outside the menu.
        .on_mouse_down_out(
            cx.listener(|this, event: &gpui::MouseDownEvent, window, cx| {
                if this
                    .menu
                    .as_ref()
                    .is_some_and(|menu| menu.in_sub(event.position))
                {
                    return;
                }
                this.close_context_menu(window, cx);
            }),
        );
    // The rows scroll inside the panel's padding, so the first and the
    // last brought into view keep it.
    let mut rows = div()
        .id("context-menu-rows")
        .min_h_0()
        .overflow_y_scroll()
        .track_scroll(&menu.scroll);
    for (i, (row, separated)) in menu.drawn().into_iter().enumerate() {
        if separated {
            rows = rows.child(separator(&theme));
        }
        let at = Spot {
            row: i,
            sub: false,
            highlighted: menu.highlighted == Some(i),
        };
        rows = rows.child(row_element(shell, menu, row, at, &theme, cx));
    }
    let panel = panel.child(rows).into_any_element();
    let panel = if let Some(anchor) = &menu.keyboard {
        let anchors = [anchor.row.clone(), anchor.area.clone()];
        if anchor.host.above() {
            PopoverAnchor::popup_first_above(&anchors, 2, panel)
        } else {
            PopoverAnchor::popup_first(&anchors, 2, panel)
        }
    } else {
        PopoverAnchor::popup_at(menu.position, 2, panel)
    };
    let Some(name) = menu.open.clone() else {
        return Some(panel);
    };
    let sub = sub_panel(shell, menu, &name, &theme, cx);
    // Above the menu's own panel, beside the row that opens it.
    Some(
        div()
            .absolute()
            .size_0()
            .child(panel)
            .child(menu.sub_row.popup_beside(3, SUB_INSET, sub))
            .into_any_element(),
    )
}

/// How far a submenu's first row stands from its panel's edge: the
/// panel's padding and border. Its row stands level with the row that
/// opened it, and its panel meets the menu's.
const SUB_INSET: Pixels = px(5.);

/// The open submenu `name`'s panel: a menu of its own to assistive
/// technology, named by its row; the keyboard stays with the menu
/// (`menu_command`), its row said as focused.
fn sub_panel(
    shell: &Shell,
    menu: &MenuState,
    name: &str,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let mut rows = div()
        .id("context-submenu-rows")
        .min_h_0()
        .overflow_y_scroll()
        .track_scroll(&menu.sub_scroll);
    for (i, (row, separated)) in menu.sub_drawn().into_iter().enumerate() {
        if separated {
            rows = rows.child(separator(theme));
        }
        let at = Spot {
            row: i,
            sub: true,
            highlighted: menu.sub_highlighted == Some(i),
        };
        rows = rows.child(row_element(shell, menu, row, at, theme, cx));
    }
    div()
        .id("context-submenu")
        .role(gpui::Role::Menu)
        .aria_label(name.to_owned())
        .relative()
        .min_w(px(180.))
        .p_1()
        .flex()
        .flex_col()
        .min_h_0()
        .rounded_lg()
        .border_1()
        .border_color(theme.border)
        .bg(theme.surface)
        .shadow_md()
        .occlude()
        // The pointer reaching it keeps it open (`rest_on`).
        .on_mouse_move(cx.listener(|this, _, _, _| {
            if let Some(menu) = this.menu.as_mut() {
                menu.rest = None;
            }
        }))
        .child(rows)
        .child(menu.sub_area.marker())
        .into_any_element()
}

/// A line between groups of rows.
fn separator(theme: &Theme) -> gpui::Div {
    div().flex_none().my_1().h(px(1.)).bg(theme.border)
}

/// Where a row is drawn: its index among the menu's rows or the open
/// submenu's, and whether the keyboard or the pointer is on it.
#[derive(Clone, Copy)]
struct Spot {
    row: usize,
    sub: bool,
    highlighted: bool,
}

/// One row of the menu or of its open submenu: an item, or a submenu's
/// row, which opens it beside itself.
fn row_element(
    shell: &Shell,
    menu: &MenuState,
    row: Row,
    at: Spot,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let muted = if at.highlighted {
        theme.selection_text
    } else {
        theme.text_muted
    };
    let (title, icon, keys) = match &row {
        Row::Item(index) => {
            let item = &menu.items[*index];
            let keys = menu
                .keys
                .get(*index)
                .cloned()
                .flatten()
                .or_else(|| shell.keys_here(&item.command.name, &item.command.args));
            let title = if &*item.command.name == PROPERTIES {
                properties_title(shell).to_owned()
            } else {
                item.title.clone()
            };
            (title, item.icon.clone(), keys)
        }
        Row::Parent(name) => (name.clone(), None, None),
    };
    let open = matches!(&row, Row::Parent(name) if menu.open.as_ref() == Some(name));
    let is_parent = matches!(row, Row::Parent(_));
    let icon = icon.filter(|name| crate::toolbar::asset(&format!("toolbar/{name}.svg")).is_some());
    // The row the submenu opens from stays highlighted while it is open.
    let lit = at.highlighted || open;
    let Spot { row: i, sub, .. } = at;
    div()
        .id((if sub { "submenu" } else { "menu" }, i))
        .role(gpui::Role::MenuItem)
        .when(shell.a11y, |d| {
            d.aria_label(title.clone())
                .when_some(keys.clone(), StatefulInteractiveElement::aria_keyshortcuts)
                .when(is_parent, |d| d.aria_expanded(open))
        })
        // Said as focused while the menu keeps the keyboard: a row of the
        // menu while the keyboard is in its rows; one of the submenu,
        // which is beside the menu, not under it, once it is there.
        .when(
            at.highlighted && !sub && menu.sub_highlighted.is_none(),
            StatefulInteractiveElement::aria_active_descendant,
        )
        .when(at.highlighted && sub, |d| {
            d.aria_active_descendant_for(&menu.focus)
        })
        .relative()
        .h(px(crate::toolbar::MENU_ROW))
        .flex_none()
        .px_2()
        .flex()
        .items_center()
        .gap_2()
        .rounded_md()
        .cursor_pointer()
        .when(lit, |d| {
            d.bg(theme.selection_bg).text_color(theme.selection_text)
        })
        .when(open, |d| d.child(menu.sub_row.marker()))
        .on_mouse_move(cx.listener({
            let row = row.clone();
            move |this, _, window, cx| {
                let Some(menu) = this.menu.as_mut() else {
                    return;
                };
                if sub {
                    if menu.sub_highlighted != Some(i) {
                        menu.sub_highlighted = Some(i);
                        cx.notify();
                    }
                    return;
                }
                if menu.highlighted != Some(i) || menu.sub_highlighted.is_some() {
                    menu.highlighted = Some(i);
                    menu.sub_highlighted = None;
                    rest_on(this, &row, window, cx);
                    cx.notify();
                }
            }
        }))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |this, _, window, cx| {
            if let (Row::Item(index), Some(menu)) = (&row, this.menu.as_ref())
                && let Some(command) = menu.items.get(*index).map(|item| item.command.clone())
            {
                this.note_pointer_use(&command.name, &command.args, cx);
            }
            this.menu_activate(&row, window, cx);
        }))
        // An icon column, empty where an item has none, keeps titles aligned.
        .child(div().size(px(16.)).flex_none().children(icon.map(|name| {
            gpui::svg()
                .path(format!("toolbar/{name}.svg"))
                .size(px(16.))
                .text_color(muted)
        })))
        .child(div().flex_1().child(title))
        .children(keys.map(|keys| {
            div()
                .flex_none()
                .pl_6()
                .text_xs()
                .text_color(muted)
                .child(keys)
        }))
        .when(is_parent, |d| {
            d.child(
                gpui::svg()
                    .path("toolbar/chevron-right.svg")
                    .size(px(13.))
                    .text_color(muted),
            )
        })
        .into_any_element()
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
    fn a_submenus_items_wait_beside_its_row() {
        let items = [
            item("Open", None),
            item("Compress…", Some("More")),
            item("Rename", None),
            item("Run Command…", Some("More")),
            MenuItem {
                order: super::LAST,
                ..item("Properties", None)
            },
        ];
        assert_eq!(
            rows_of(&items),
            [
                Row::Item(0),
                Row::Item(2),
                Row::Parent("More".into()),
                Row::Item(4)
            ],
            "the menu's end comes after the submenus' rows"
        );
        assert_eq!(super::sub_rows_of(&items, "More"), [1, 3]);
        assert!(super::sub_rows_of(&items, "Notes").is_empty());
    }

    #[test]
    fn the_keyboards_row_is_found_among_the_separators() {
        let at = |title: &str, order, submenu| MenuItem {
            order,
            ..item(title, submenu)
        };
        let items = [
            at("Open", 0, None),
            at("Copy", 120, None),
            at("Paste", 130, None),
            at("Move to Trash", 300, None),
            at("Compress…", 400, Some("More")),
            at("Same Kind", 470, Some("Find Similar")),
            at("Properties", super::LAST, None),
        ];
        let drawn = super::drawn_of(&items, &rows_of(&items));
        let separated: Vec<bool> = drawn.iter().map(|(_, separated)| *separated).collect();
        assert_eq!(
            separated,
            [false, true, false, true, true, false, true],
            "between the hundreds, above the submenus' rows (one line for \
             them all), before the menu's end"
        );
        // Open, a line, Copy, Paste, a line, Move to Trash, a line, More,
        // Find Similar, a line, Properties.
        let children: Vec<usize> = (0..drawn.len())
            .map(|row| super::child_of(&drawn, row))
            .collect();
        assert_eq!(children, [0, 2, 3, 5, 7, 8, 10]);
        // A submenu's own rows: lines between its hundreds only.
        let rows: Vec<Row> = super::sub_rows_of(&items, "More")
            .into_iter()
            .map(Row::Item)
            .collect();
        assert_eq!(super::drawn_of(&items, &rows), [(Row::Item(4), false)]);
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
        assert!(when_holds(Some("empty"), ctx(0, true, true)));
        assert!(!when_holds(Some("empty"), ctx(1, true, true)));
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
        let search = SelectionCtx {
            search: true,
            ..ctx(0, true, true)
        };
        assert!(when_holds(Some("empty search"), search));
        assert!(!when_holds(Some("search"), folder));
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
