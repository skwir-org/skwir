//! The Exclusions page (ADR-125), drawn: the exclusions as
//! `settings_exclusions` lists them for the role chosen, in one list that
//! draws the rows in view, walked by the keys (`ui.settings_exclusion`), each
//! category's heading with its switch for the role, every category folded
//! when the page opens. A row's menu is the window's context menu
//! (`Shell::open_settings_menu`), its items the page's commands
//! (`ui.settings_exclusion_item`). Every change is saved at once through
//! `settings.apply` (both tables, and the 0.3.1 keys emptied where a file
//! still held them) and said at the page's foot; one that takes folders
//! from agents asks first, in a modal of the dialog. Which folders are on
//! this computer is looked up on a worker.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use gpui::{
    AnyElement, App, ClickEvent, Context, Entity, FocusHandle, Focusable, KeyDownEvent,
    MouseButton, MouseDownEvent, ScrollStrategy, SharedString, Subscription,
    UniformListScrollHandle, Window, div, prelude::*, px,
};
use skwir_core::command::Invocation;
use skwir_core::exclusions::{
    CATEGORIES_KEY, Catalog, ENTRIES_KEY, LEGACY_FOLDERS_KEY, LEGACY_MARKER_KEY, Legacy, Names,
    Role, YOURS,
};
use skwir_core::registry::MenuItem;
use skwir_core::scheduler::{Lane, Priority};
use skwir_core::settings::SettingSpec;

use crate::controls::{self, ButtonStyle};
use crate::elements::popover::PopoverAnchor;
use crate::input::{InputEvent, TextInput};
use crate::latest::Latest;
use crate::settings::SettingsView;
use crate::settings_exclusions::{
    self as model, Ask, Badge, Cursor, Draft, Field, Heading, Kind, Line, ROLES, Row,
};
use crate::settings_view::{Edge, card_row, revert_slot};
use crate::sizes::{TextSizes, sized};
use crate::theme::Theme;

/// The height of every row of the list, a heading's and a sub-heading's
/// too: the same for all, so only the rows in view are drawn.
const ROW: f32 = 46.;
/// Rows a page key moves by.
const PAGE: usize = 8;
/// What a modal of the page lists before it scrolls.
const LISTED: usize = 8;

/// What the Exclusions page shows, and what it is editing.
pub(crate) struct ExclusionsPage {
    /// The role chosen: what the ticks and switches say.
    pub role: Role,
    /// What narrows the list: folders, what they are, what they mean.
    pub filter: Entity<TextInput>,
    pub only_here: bool,
    pub changed_only: bool,
    /// The categories unfolded: the page opens with every one folded, and
    /// keeps what was unfolded while the dialog lives.
    unfolded: HashSet<String>,
    /// Moves when `unfolded` or `revealed` does: the rows are worked out
    /// again.
    folds: u64,
    /// The entries shown whatever the filters say until the page is left:
    /// what was just added, edited, moved, copied or shown.
    revealed: HashSet<String>,
    pub cursor: Option<Cursor>,
    pub list: UniformListScrollHandle,
    /// The list's: it walks the rows (`ui.settings_exclusion`).
    pub focus: FocusHandle,
    /// The rules as the page shows them: as the settings say, or as just
    /// changed, while the save that carries the change waits or runs.
    catalog: Rc<Catalog>,
    generation: u64,
    rows: RefCell<Option<(Asked, Rc<Vec<Row>>)>>,
    /// How many entries the dialog's search finds, for the rules'
    /// generation and the words searched: not counted at each frame.
    found: RefCell<Option<(u64, String, usize)>>,
    presence: Presence,
    /// What the last change did, said at the page's foot.
    said: Option<String>,
    /// The row whose menu the window shows, while it does: the menu holds
    /// what this is a weak hold of (`SettingsMenu::held`).
    menu: Option<(Cursor, std::rc::Weak<()>)>,
    /// Where a menu opened from the keyboard hangs: the row it is about,
    /// else the list (`menu::MenuHost::Settings`), drawn here.
    menu_row: PopoverAnchor,
    menu_area: PopoverAnchor,
    dialog: Option<Dialog>,
    asking: Option<Asking>,
}

/// Which folders of the list are on this computer: each path or path
/// glob written (`Matcher::looked_up`), as a worker found. Looked up once
/// each, as the list grows: no look stands for another.
#[derive(Default)]
struct Presence {
    known: HashMap<String, bool>,
    /// Moves when `known` does: the rows are worked out again.
    generation: u64,
    /// The folders asked of a worker, answered or not.
    asked: HashSet<String>,
    /// The catalog's generation they were last asked for.
    asked_for: Option<u64>,
    /// What workers found and the window has not taken in yet.
    found: Arc<Mutex<Vec<(String, bool)>>>,
    /// Whether the first look came back: until then, with Only on this
    /// computer on, the list waits rather than shows folders it would hide.
    answered: bool,
}

#[derive(Clone, PartialEq)]
struct Asked {
    generation: u64,
    role: Role,
    filter: String,
    only_here: bool,
    changed_only: bool,
    folds: u64,
    here: u64,
}

impl ExclusionsPage {
    pub fn new(cx: &mut App) -> Self {
        let filter = cx.new(|cx| {
            let mut input = TextInput::new("Filter by name, path or what it is", cx);
            input.set_label(format!("Filter {}", model::ENTRIES));
            input
        });
        ExclusionsPage {
            role: Role::Searches,
            filter,
            only_here: true,
            changed_only: false,
            unfolded: HashSet::new(),
            folds: 0,
            revealed: HashSet::new(),
            cursor: None,
            list: UniformListScrollHandle::new(),
            focus: cx.focus_handle(),
            catalog: Rc::new(Catalog::default()),
            generation: 0,
            rows: RefCell::default(),
            found: RefCell::default(),
            presence: Presence::default(),
            said: None,
            menu: None,
            menu_row: PopoverAnchor::default(),
            menu_area: PopoverAnchor::default(),
            dialog: None,
            asking: None,
        }
    }

    fn set_catalog(&mut self, catalog: Catalog) {
        self.catalog = Rc::new(catalog);
        self.generation += 1;
        self.rows.take();
    }

    /// Show the entry `id` whatever the filters say.
    fn reveal(&mut self, id: &str) {
        if self.revealed.insert(id.to_owned()) {
            self.folds += 1;
        }
    }

    fn fold(&mut self, id: &str, folded: bool) {
        let changed = if folded {
            self.unfolded.remove(id)
        } else {
            self.unfolded.insert(id.to_owned())
        };
        if changed {
            self.folds += 1;
        }
    }

    /// Whether the window shows the menu of `row`.
    fn menu_of(&self, row: &Cursor) -> bool {
        self.menu
            .as_ref()
            .is_some_and(|(at, held)| at == row && held.strong_count() > 0)
    }
}

/// What a row's menu does (`ui.settings_exclusion_item`'s `do`), and Reset's
/// button.
#[derive(Clone, Debug, PartialEq)]
enum Item {
    Duplicate,
    Edit,
    Reset,
    Remove,
    Rename,
    Delete,
    AddHere,
    /// Move to this category.
    To(String),
}

/// skwir's Secrets category: Privacy's Private folders… opens the page on
/// agents with it unfolded.
const SECRETS: &str = "skwir.secrets";

/// What the page asks in a modal of the dialog.
enum Dialog {
    Entry(Box<EntryDialog>),
    Category(CategoryDialog),
}

/// Add a folder, or edit one of the user's.
struct EntryDialog {
    /// The entry edited; `None` adds one.
    editing: Option<String>,
    kind: Kind,
    pattern: Entity<TextInput>,
    beside: Entity<TextInput>,
    holding: Entity<TextInput>,
    what: Entity<TextInput>,
    category: String,
    category_open: bool,
    category_anchor: PopoverAnchor,
    /// What is wrong, said under its field; worked out as it is typed.
    problem: Option<(Field, String)>,
    /// What it means, said live.
    means: Option<String>,
    /// The entry already naming these folders: Show it instead.
    already: Option<String>,
    /// The folder typed and whether it is on this computer, once a worker
    /// looked.
    here: Option<(String, bool)>,
    /// The look for the folder typed last: an older one's answer is dropped.
    found: Latest<Option<(String, bool)>>,
    kind_focus: FocusHandle,
    /// What a pattern names: files, folders or both.
    names: Names,
    names_focus: FocusHandle,
    choose_focus: FocusHandle,
    category_focus: FocusHandle,
    cancel: FocusHandle,
    accept: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl EntryDialog {
    fn draft(&self, cx: &App) -> Draft {
        Draft {
            kind: self.kind,
            pattern: self.pattern.read(cx).text().to_owned(),
            names: self.names,
            beside: self.beside.read(cx).text().to_owned(),
            holding: self.holding.read(cx).text().to_owned(),
            what: self.what.read(cx).text().to_owned(),
            category: self.category.clone(),
        }
    }

    /// What Tab walks, in order.
    fn stops(&self, cx: &App) -> Vec<FocusHandle> {
        let field = |input: &Entity<TextInput>| input.read(cx).focus_handle(cx);
        let mut stops = vec![self.kind_focus.clone()];
        match self.kind {
            Kind::Pattern => {
                stops.push(self.names_focus.clone());
                stops.push(field(&self.pattern));
                stops.push(self.choose_focus.clone());
            }
            Kind::Beside => {
                stops.push(field(&self.pattern));
                stops.push(field(&self.beside));
            }
            Kind::Holding => stops.push(field(&self.holding)),
        }
        stops.push(field(&self.what));
        stops.push(self.category_focus.clone());
        stops.push(self.cancel.clone());
        stops.push(self.accept.clone());
        stops
    }
}

/// Make a category, or rename one of the user's.
struct CategoryDialog {
    renaming: Option<String>,
    name: Entity<TextInput>,
    problem: Option<String>,
    cancel: FocusHandle,
    accept: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

/// A change that takes folders from agents, asked before it is saved.
struct Asking {
    after: Catalog,
    said: String,
    folders: Vec<String>,
    keep: FocusHandle,
    expose: FocusHandle,
    scroll: gpui::ScrollHandle,
}

impl SettingsView {
    // ------------------------------------------------------------ the rules

    /// Read the rules again, as the settings say them, unless a change of
    /// them waits to be saved or is being saved: then what the page shows
    /// is that change.
    pub(crate) fn read_exclusions(&mut self) {
        if self.pending(CATEGORIES_KEY) || self.pending(ENTRIES_KEY) {
            return;
        }
        let catalog = Catalog::of(&self.app.settings);
        if self.exclusions.generation > 0 && catalog == *self.exclusions.catalog {
            return;
        }
        self.exclusions.set_catalog(catalog);
    }

    /// The page on `role`, from another page (Privacy's Private folders…,
    /// `ui.settings { page: "exclusions", role }`).
    pub(crate) fn show_exclusions(&mut self, role: Role, cx: &mut Context<Self>) {
        self.go(crate::settings::Page::Exclusions, cx);
        self.exclusions.role = role;
        // Asked for what agents never see: the secrets, unfolded, at the
        // top, wherever their name puts them.
        if role == Role::Agents {
            self.exclusions.fold(SECRETS, false);
        }
        self.exclusions.rows.take();
        let secrets = Cursor::Category(SECRETS.to_owned());
        let at = if role == Role::Agents {
            self.exclusion_rows(cx)
                .iter()
                .position(|r| r.cursor().as_ref() == Some(&secrets))
                .unwrap_or(0)
        } else {
            0
        };
        // Strict: Secrets in view, lower down, is still brought up.
        self.exclusions
            .list
            .scroll_to_item_strict(at, ScrollStrategy::Top);
        cx.notify();
    }

    /// Leaving the page: what it had open closes.
    pub(crate) fn leave_exclusions(&mut self) {
        if !self.exclusions.revealed.is_empty() {
            self.exclusions.revealed.clear();
            self.exclusions.folds += 1;
        }
        // What the page said of its last change was said there.
        self.exclusions.said = None;
        self.exclusions.menu = None;
        self.exclusions.dialog = None;
        self.exclusions.asking = None;
    }

    /// Take in what workers found: the folders on this computer, the one
    /// typed in the dialog. Whether anything changed.
    pub(crate) fn exclusions_sync(&mut self) -> bool {
        let mut changed = false;
        let presence = &mut self.exclusions.presence;
        let found: Vec<(String, bool)> = presence
            .found
            .lock()
            .map(|mut found| std::mem::take(&mut *found))
            .unwrap_or_default();
        if !found.is_empty() {
            presence.known.extend(found);
            presence.generation += 1;
            presence.answered = true;
            changed = true;
        }
        if let Some(Dialog::Entry(dialog)) = &mut self.exclusions.dialog
            && let Some(answer) = dialog.found.take()
        {
            dialog.here = answer;
            changed = true;
        }
        changed
    }

    /// Ask a worker which folders of the list are on this computer: those
    /// not asked yet, once the list changed.
    fn look_up_exclusions(&mut self) {
        let generation = self.exclusions.generation;
        let presence = &mut self.exclusions.presence;
        if presence.asked_for == Some(generation) {
            return;
        }
        presence.asked_for = Some(generation);
        let texts: Vec<String> = model::asked_presence(&self.exclusions.catalog)
            .into_iter()
            .filter(|text| !presence.asked.contains(text))
            .collect();
        if texts.is_empty() {
            if presence.asked.is_empty() {
                presence.answered = true;
            }
            return;
        }
        presence.asked.extend(texts.iter().cloned());
        let slot = Arc::clone(&presence.found);
        let weak = self.app.weak();
        self.app.scheduler.spawn(
            skwir_core::scheduler::Priority::Normal,
            "settings-exclusions-here",
            self.app.lifetime().child(),
            move || {
                let home = skwir_core::os_path::home();
                let found = model::presence(&texts, home.as_deref());
                if let Ok(mut slot) = slot.lock() {
                    slot.extend(found);
                }
                if let Some(app) = weak.upgrade() {
                    app.wake();
                }
            },
        );
    }

    /// The rows for what the page asks: kept until that or the rules
    /// change, not worked out at each frame.
    fn exclusion_rows(&self, cx: &App) -> Rc<Vec<Row>> {
        let page = &self.exclusions;
        let asked = Asked {
            generation: page.generation,
            role: page.role,
            filter: page.filter.read(cx).text().trim().to_owned(),
            only_here: page.only_here,
            changed_only: page.changed_only,
            folds: page.folds,
            here: page.presence.generation,
        };
        if let Some((kept, rows)) = page.rows.borrow().as_ref()
            && *kept == asked
        {
            return Rc::clone(rows);
        }
        let rows = Rc::new(model::rows(
            &page.catalog,
            &Ask {
                role: asked.role,
                filter: &asked.filter,
                only_here: asked.only_here,
                here: &page.presence.known,
                changed_only: asked.changed_only,
                unfolded: &page.unfolded,
                revealed: &page.revealed,
            },
        ));
        *page.rows.borrow_mut() = Some((asked, Rc::clone(&rows)));
        rows
    }

    /// Make `after` the rules: asked first where it takes folders from
    /// agents (`settings_exclusions::exposure`), else saved at once.
    fn propose_exclusions(
        &mut self,
        after: Catalog,
        said: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let folders = model::exposure(&self.exclusions.catalog, &after);
        if folders.is_empty() {
            self.save_exclusions(after, said, cx);
            return;
        }
        let keep = cx.focus_handle();
        window.focus(&keep, cx);
        self.exclusions.asking = Some(Asking {
            after,
            said,
            folders,
            keep,
            expose: cx.focus_handle(),
            scroll: gpui::ScrollHandle::new(),
        });
        cx.notify();
    }

    /// Save `after`: both tables, the 0.3.1 keys emptied where the file
    /// still holds them (what they said is in the tables now). What it did
    /// is said at the page's foot; a tick or a switch undoes itself.
    fn save_exclusions(&mut self, after: Catalog, said: String, cx: &mut Context<Self>) {
        let (categories, entries) = after.values();
        let mut changes = BTreeMap::from([
            (
                CATEGORIES_KEY.to_owned(),
                SettingSpec::format_value(&categories),
            ),
            (ENTRIES_KEY.to_owned(), SettingSpec::format_value(&entries)),
        ]);
        if !Legacy::of(&self.app.settings).is_empty() {
            changes.insert(LEGACY_FOLDERS_KEY.to_owned(), "[]".to_owned());
            changes.insert(LEGACY_MARKER_KEY.to_owned(), "true".to_owned());
        }
        if let Err(error) = self.change_drawn(changes, cx) {
            self.errors.insert(ENTRIES_KEY.to_owned(), error);
            cx.notify();
            return;
        }
        self.exclusions.said = Some(said);
        self.exclusions.set_catalog(after);
        cx.notify();
    }

    /// An entry by its id.
    fn exclusion_entry(&self, id: &str) -> Option<&skwir_core::exclusions::Entry> {
        self.exclusions.catalog.entries.iter().find(|e| e.id == id)
    }

    fn exclusion_words(&self, id: &str) -> String {
        self.exclusion_entry(id)
            .map_or_else(String::new, |e| model::pattern_of(&e.matcher))
    }

    fn category_name(&self, id: &str) -> String {
        self.exclusions
            .catalog
            .category(id)
            .map_or_else(|| id.to_owned(), |c| c.name.clone())
    }

    /// Tick or untick the entry for the role chosen.
    fn tick_exclusion(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let role = self.exclusions.role;
        let catalog = Rc::clone(&self.exclusions.catalog);
        let Some(entry) = catalog.entries.iter().find(|e| e.id == id) else {
            return;
        };
        let on = !entry.ticks.get(role);
        let category_on = catalog
            .category(&entry.category)
            .is_some_and(|c| c.switches.get(role));
        let after = model::ticked(&catalog, id, role, on);
        let said = model::said_tick(&model::pattern_of(&entry.matcher), role, on, category_on);
        self.propose_exclusions(after, said, window, cx);
    }

    /// Switch the category for the role chosen.
    fn switch_category(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let role = self.exclusions.role;
        let Some(on) = self
            .exclusions
            .catalog
            .category(id)
            .map(|c| !c.switches.get(role))
        else {
            return;
        };
        let after = model::switched(&self.exclusions.catalog, id, role, on);
        let said = model::said_switch(&self.category_name(id), role, on);
        self.propose_exclusions(after, said, window, cx);
    }

    fn remove_exclusion(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        match self.exclusion_entry(id) {
            Some(entry) if !entry.builtin => {
                let said = format!("Removed {}", model::pattern_of(&entry.matcher));
                // The cursor goes to the row after it, or before it.
                let rows = self.exclusion_rows(cx);
                let at = rows
                    .iter()
                    .position(|r| matches!(r, Row::Entry(l) if l.id == id));
                let next = at.and_then(|at| {
                    rows[at + 1..]
                        .iter()
                        .chain(rows[..at].iter().rev())
                        .find_map(Row::cursor)
                });
                let after = model::removed(&self.exclusions.catalog, id);
                self.exclusions.cursor = next;
                self.propose_exclusions(after, said, window, cx);
            }
            Some(_) => {
                self.exclusions.said = Some(format!(
                    "skwir's own {} are unticked, never removed",
                    model::ENTRIES
                ));
                cx.notify();
            }
            None => {}
        }
    }

    // ------------------------------------------------------------ the page

    /// The page under its title: the role chosen, the ways to narrow the
    /// list, Add and New category.
    pub(crate) fn exclusions_toolbar(
        &mut self,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let roles = self.handle("exclusions-role", cx);
        self.stop(&roles, None, None);
        let current = self.exclusions.role;
        let mut segmented = controls::segmented("exclusions-role", &roles, theme)
            .when(self.a11y, |d| d.aria_label(model::EXCLUDED_FROM))
            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                if matches!(event, ClickEvent::Keyboard(_)) {
                    this.exclusions.role = if current == Role::Searches {
                        Role::Agents
                    } else {
                        Role::Searches
                    };
                    this.exclusions.rows.take();
                    cx.notify();
                }
            }));
        for role in ROLES {
            let focus = roles.clone();
            segmented = segmented.child(
                controls::segment(
                    SharedString::from(format!("exclusions-role-{}", role.key())),
                    model::role_name(role),
                    current == role,
                    theme,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    window.focus(&focus, cx);
                    this.exclusions.role = role;
                    this.exclusions.rows.take();
                    cx.notify();
                })),
            );
        }
        let add = self.handle("exclusions-add", cx);
        self.stop(&add, None, None);
        let new_category = self.handle("exclusions-new-category", cx);
        self.stop(&new_category, None, None);
        let filter_focus = self.exclusions.filter.focus_handle(cx);
        self.stop(&filter_focus, None, None);
        let focused = self.exclusions.filter.read(cx).is_focused(window);
        let filter = controls::field(
            &self.exclusions.filter,
            None,
            focused,
            false,
            Some("search"),
            theme,
        );
        let here = self.handle("exclusions-here", cx);
        self.stop(&here, None, None);
        let changed_focus = self.handle("exclusions-changed", cx);
        self.stop(&changed_focus, None, None);
        let mut bar = div()
            .flex_none()
            .px_6()
            .pb_3()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_body()
                            .text_color(theme.text_muted)
                            .child(model::EXCLUDED_FROM),
                    )
                    .child(segmented)
                    .child(div().flex_1())
                    .child(
                        controls::button(
                            "exclusions-add",
                            "Add…",
                            Some("plus"),
                            ButtonStyle::Plain,
                            &add,
                            theme,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.open_entry_dialog(None, None, window, cx);
                        })),
                    )
                    .child(
                        controls::button(
                            "exclusions-new-category",
                            "New category…",
                            Some("folder-plus"),
                            ButtonStyle::Plain,
                            &new_category,
                            theme,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.open_category_dialog(None, window, cx);
                        })),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(filter)
                    .child(
                        controls::check(
                            "exclusions-here",
                            "Only on this computer",
                            self.exclusions.only_here,
                            &here,
                            theme,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.exclusions.only_here = !this.exclusions.only_here;
                            cx.notify();
                        })),
                    )
                    .child(
                        controls::check(
                            "exclusions-changed",
                            "Changed",
                            self.exclusions.changed_only,
                            &changed_focus,
                            theme,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.exclusions.changed_only = !this.exclusions.changed_only;
                            cx.notify();
                        })),
                    ),
            );
        if let Some(error) = self
            .errors
            .get(ENTRIES_KEY)
            .or_else(|| self.errors.get(CATEGORIES_KEY))
        {
            bar = bar.child(
                controls::noted("warning", theme.error, 13., error.clone())
                    .text_body()
                    .text_color(theme.error),
            );
        }
        // What the settings hold that cannot be read: it keeps nothing out
        // until it is fixed, and is never erased by a change here.
        if let Some(problems) = model::unreadable(&self.exclusions.catalog) {
            bar = bar.child(
                controls::noted("warning", theme.warning, 13., problems)
                    .text_body()
                    .text_color(theme.warning),
            );
        }
        bar.into_any_element()
    }

    /// Every row asked for, in a list that draws those in view, and what
    /// the last change did under it.
    pub(crate) fn exclusions_list(
        &mut self,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.look_up_exclusions();
        let rows = self.exclusion_rows(cx);
        let focus = self.exclusions.focus.clone();
        self.stop(&focus, None, None);
        let foot = self.exclusions_foot(theme);
        let waiting = self.exclusions.only_here && !self.exclusions.presence.answered;
        if rows.is_empty() || waiting {
            let filter = self.exclusions.filter.read(cx).text().trim().to_owned();
            let said = if waiting {
                "Looking for these files and folders on this computer…".to_owned()
            } else if !filter.is_empty() {
                format!("No {} matches “{filter}”.", model::ENTRY)
            } else if self.exclusions.changed_only {
                format!(
                    "You changed none of skwir's {} and added none yet.",
                    model::ENTRIES
                )
            } else {
                format!("No {} here.", model::ENTRIES)
            };
            return div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .id("exclusion-rows")
                        .role(gpui::Role::Group)
                        .aria_label(page_title())
                        .track_focus(&focus)
                        .key_context("SettingExclusions")
                        .flex_1()
                        .px_6()
                        .py_6()
                        .text_body()
                        .text_color(theme.text_muted)
                        .child(said),
                )
                .children(foot)
                .into_any_element();
        }
        let focused = focus.contains_focused(window, cx);
        let count = rows.len();
        let role = self.exclusions.role;
        let a11y = self.a11y;
        let list_rows = Rc::clone(&rows);
        let list = gpui::uniform_list(
            "exclusion-rows-rows",
            count,
            cx.processor(move |this: &mut Self, range: Range<usize>, _, cx| {
                let theme = cx.global::<Theme>().clone();
                range
                    .filter_map(|n| {
                        let element = match list_rows.get(n)? {
                            Row::Heading(heading) => this
                                .exclusion_heading(heading, focused, &theme, cx)
                                .into_any_element(),
                            Row::Group(name) => div()
                                .h(sized(ROW))
                                .flex()
                                .items_end()
                                .pb_2()
                                .px_1()
                                .text_small()
                                .text_color(theme.text_muted)
                                .child(name.clone())
                                .into_any_element(),
                            Row::Entry(line) => this
                                .exclusion_line(line, focused, &theme, cx)
                                .into_any_element(),
                            Row::Empty(_, why) => div()
                                .h(sized(ROW))
                                .flex()
                                .items_center()
                                .px_4()
                                .text_body()
                                .text_color(theme.text_muted)
                                .child(match why {
                                    model::Narrowed::Here => {
                                        "Nothing it names is on this computer".to_owned()
                                    }
                                    model::Narrowed::Found => {
                                        format!("None of its {} is shown", model::ENTRIES)
                                    }
                                    model::Narrowed::No => format!(
                                        "Nothing in it yet: Add… puts {} here, or Move to from a row's menu",
                                        an(model::ENTRY)
                                    ),
                                })
                                .into_any_element(),
                        };
                        Some(div().w_full().px_6().child(element).into_any_element())
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.exclusions.list)
        .size_full();
        // A menu opened from the keyboard hangs from its row, else from
        // the list, its row scrolled out of view.
        let area = self
            .exclusions
            .menu
            .as_ref()
            .filter(|(_, held)| held.strong_count() > 0)
            .map(|_| self.exclusions.menu_area.marker());
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .id("exclusion-rows")
                    .role(gpui::Role::Group)
                    .when(a11y, |d| {
                        d.aria_label(format!("{} {}", page_title(), model::excluded_from(role)))
                    })
                    .track_focus(&focus)
                    .key_context("SettingExclusions")
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .on_click(cx.listener(|this, _, window, cx| {
                        window.focus(&this.exclusions.focus, cx);
                        cx.notify();
                    }))
                    .child(list)
                    .child(crate::elements::scrollbar::vertical(
                        "exclusion-rows-scrollbar",
                        crate::elements::scrollbar::Scrolls::Offset(
                            self.exclusions.list.0.borrow().base_handle.clone(),
                        ),
                    ))
                    .children(area),
            )
            .children(foot)
            .into_any_element()
    }

    /// What the last change did: said where it was asked, and to a screen
    /// reader as it is said (a polite live region). No Undo: a tick or a
    /// switch undoes itself, as on every other page.
    fn exclusions_foot(&self, theme: &Theme) -> Option<AnyElement> {
        let said = self.exclusions.said.clone()?;
        Some(
            div()
                .id("exclusions-said")
                .role(gpui::Role::Status)
                .aria_live(gpui::Live::Polite)
                .when(self.a11y, |d| d.aria_label(said.clone()))
                .flex_none()
                .mx_6()
                .mb_4()
                .mt_2()
                .px_3()
                .py_2()
                .flex()
                .items_center()
                .gap_3()
                .rounded_md()
                .border_1()
                .border_color(theme.border)
                .bg(theme.surface)
                .text_body()
                .child(div().flex_1().min_w_0().child(said))
                .into_any_element(),
        )
    }

    /// A category's heading: its chevron, its name and count, its menu,
    /// and its switch for the role at its end. A press on it folds or
    /// unfolds the category; only the switch turns it on or off for the
    /// role. To a screen reader it is a button that expands, the switch a
    /// control of its own, named by the category and the role. The keys
    /// are the list's: Left and Right fold, Space and Enter turn its
    /// switch (`exclusions_key`).
    fn exclusion_heading(
        &self,
        heading: &Heading,
        focused: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let role = self.exclusions.role;
        let row = Cursor::Category(heading.id.clone());
        let cursor = self.exclusions.cursor.as_ref() == Some(&row);
        let menu_open = self.exclusions.menu_of(&row);
        let hover = theme.hover;
        let id = heading.id.clone();
        let folded = heading.folded;
        let chevron = div()
            .flex_none()
            .size(px(22.))
            .flex()
            .items_center()
            .justify_center()
            .child(controls::glyph(
                if folded { "chevron-right" } else { "chevron" },
                theme.text_muted,
                14.,
            ));
        let more = self.more_button(
            row.clone(),
            menu_open,
            &format!("exclusion-heading-more-{id}"),
            theme,
            cx,
        );
        let switch_id = id.clone();
        let switch = div()
            .id(SharedString::from(format!("exclusion-switch-{id}")))
            .role(gpui::Role::Switch)
            .aria_toggled(if heading.on {
                gpui::Toggled::True
            } else {
                gpui::Toggled::False
            })
            .when(self.a11y, |d| {
                d.aria_label(format!("{} {}", heading.name, model::excluded_from(role)))
            })
            .flex_none()
            .p(px(2.))
            .rounded_full()
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.exclusions.cursor = Some(Cursor::Category(switch_id.clone()));
                window.focus(&this.exclusions.focus, cx);
                this.switch_category(&switch_id, window, cx);
            }))
            .child(controls::pill(heading.on, theme));
        let fold_id = id.clone();
        let pointed = row.clone();
        div().h(sized(ROW)).flex().items_end().pb_1().child(
            div()
                .id(SharedString::from(format!("exclusion-heading-{id}")))
                .role(gpui::Role::Button)
                .aria_expanded(!folded)
                .when(self.a11y, |d| d.aria_label(heading.said(role)))
                .when(cursor, StatefulInteractiveElement::aria_active_descendant)
                .w_full()
                .h(sized(34.))
                .pl_1()
                .pr_2()
                .flex()
                .items_center()
                .gap_2()
                .rounded_md()
                .border_1()
                .border_color(if cursor && focused {
                    theme.accent
                } else {
                    gpui::transparent_black()
                })
                .relative()
                .when(menu_open, |d| {
                    d.bg(theme.highlight)
                        .child(self.exclusions.menu_row.marker())
                })
                .cursor_pointer()
                .hover(move |s| s.bg(hover))
                .tooltip(move |_, cx| {
                    crate::toolbar::tooltip(if folded { "Unfold" } else { "Fold" }.into(), cx)
                })
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.exclusions.fold(&fold_id, !folded);
                    this.exclusions.cursor = Some(Cursor::Category(fold_id.clone()));
                    window.focus(&this.exclusions.focus, cx);
                    cx.notify();
                }))
                // The right button's menu, as on every row of skwir.
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        this.exclusions.cursor = Some(pointed.clone());
                        this.open_exclusion_menu(pointed.clone(), Some(event.position), window, cx);
                    }),
                )
                .child(chevron)
                // The name before the count: the count gives way first.
                .child(
                    div()
                        .flex_none()
                        .max_w(gpui::relative(0.5))
                        .truncate()
                        .text_body()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(heading.name.clone()),
                )
                .child(
                    div()
                        .flex_shrink_1()
                        .min_w_0()
                        .truncate()
                        .text_small()
                        .text_color(theme.text_muted)
                        .child(heading.count()),
                )
                .child(div().flex_1())
                .child(more)
                .child(switch),
        )
    }

    /// An entry's row: its check, its folder and what it is, its state in
    /// words, Reset where it is one of skwir's changed, its menu.
    fn exclusion_line(
        &self,
        line: &Line,
        focused: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let role = self.exclusions.role;
        let row = Cursor::Entry(line.id.clone());
        let cursor = self.exclusions.cursor.as_ref() == Some(&row);
        let menu_open = self.exclusions.menu_of(&row);
        let hover = theme.hover;
        let reset = line.badges.contains(&Badge::Changed).then(|| {
            let id = line.id.clone();
            div()
                .id(SharedString::from(format!("exclusion-reset-{}", line.id)))
                .flex_none()
                .size(px(24.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_md()
                .cursor_pointer()
                .hover(move |s| s.bg(hover))
                .child(controls::glyph("revert", theme.text_muted, 14.))
                .tooltip(move |_, cx| {
                    crate::toolbar::tooltip("Reset: back as skwir ships it".into(), cx)
                })
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.run_item(&Cursor::Entry(id.clone()), &Item::Reset, window, cx);
                }))
                .into_any_element()
        });
        let more = self.more_button(
            row.clone(),
            menu_open,
            &format!("exclusion-more-{}", line.id),
            theme,
            cx,
        );
        let pointed = row;
        let badges = line.badges.iter().map(|badge| {
            let color = match badge {
                Badge::Absent => theme.text_muted,
                Badge::Changed | Badge::Moved | Badge::Yours => theme.accent_text,
            };
            div()
                .flex_none()
                .text_small()
                .text_color(color)
                .child(badge.words())
        });
        let under = if line.dimmed {
            let under = line.under();
            if under.is_empty() {
                Line::off_words(role)
            } else {
                format!("{} · {under}", Line::off_words(role))
            }
        } else {
            line.under()
        };
        let id = line.id.clone();
        card_row(
            theme,
            Edge {
                first: line.first,
                last: line.last,
            },
        )
        .id(SharedString::from(format!("exclusion-entry-{}", line.id)))
        .role(gpui::Role::CheckBox)
        .aria_toggled(if line.tick {
            gpui::Toggled::True
        } else {
            gpui::Toggled::False
        })
        .when(self.a11y, |d| {
            d.aria_label(line.pattern.clone())
                .aria_description(line.description(role))
        })
        .when(cursor, StatefulInteractiveElement::aria_active_descendant)
        .h(sized(ROW))
        .py_0()
        .px_3()
        .gap_3()
        .when(focused && cursor, |d| d.border_color(theme.accent))
        .relative()
        .when(menu_open, |d| {
            d.bg(theme.highlight)
                .child(self.exclusions.menu_row.marker())
        })
        .cursor_pointer()
        .on_click(cx.listener(move |this, _, window, cx| {
            this.exclusions.cursor = Some(Cursor::Entry(id.clone()));
            window.focus(&this.exclusions.focus, cx);
            this.tick_exclusion(&id, window, cx);
        }))
        // The right button's menu, as on every row of skwir.
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                this.exclusions.cursor = Some(pointed.clone());
                this.open_exclusion_menu(pointed.clone(), Some(event.position), window, cx);
            }),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .gap_3()
                .when(line.dimmed, |d| d.opacity(0.5))
                .child(controls::check_box(line.tick, theme))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .flex()
                                .items_baseline()
                                .gap_2()
                                .child(
                                    div()
                                        .flex_shrink_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_body()
                                        .child(line.pattern.clone()),
                                )
                                .children(badges),
                        )
                        .when(!under.is_empty(), |d| {
                            d.child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_small()
                                    .text_color(theme.text_muted)
                                    .child(under),
                            )
                        }),
                ),
        )
        .child(revert_slot(24., reset))
        .child(more)
    }

    /// A row's ⋯: its menu, where it was pressed. Pressed again while its
    /// menu shows, it leaves it closed: the press outside closed it.
    fn more_button(
        &self,
        target: Cursor,
        open: bool,
        id: &str,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let hover = theme.hover;
        div()
            .id(SharedString::from(id.to_owned()))
            .role(gpui::Role::Button)
            .aria_label("More")
            .flex_none()
            .size(px(24.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_md()
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .when(open, |d| d.bg(hover))
            .child(controls::glyph("more", theme.text_muted, 14.))
            .tooltip(|_, cx| crate::toolbar::tooltip("More (Menu key)".into(), cx))
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                cx.stop_propagation();
                if open {
                    return;
                }
                this.exclusions.cursor = Some(target.clone());
                this.open_exclusion_menu(target.clone(), Some(event.position()), window, cx);
            }))
    }

    // ------------------------------------------------------------ the menu

    /// Ask the window for `target`'s menu, the window's context menu over
    /// the dialog (`Shell::open_settings_menu`): where the pointer pressed
    /// (`at`), else, from the keyboard, under the row. The keyboard is on
    /// the list as it opens, and goes back there when it closes.
    fn open_exclusion_menu(
        &mut self,
        target: Cursor,
        at: Option<gpui::Point<gpui::Pixels>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let items = self.menu_items(&target);
        if items.is_empty() {
            return;
        }
        window.focus(&self.exclusions.focus, cx);
        if at.is_none() {
            self.reveal_cursor(cx);
        }
        let held = Rc::new(());
        // Anchors of its own: none of a menu before it is taken for this
        // one's before it is drawn.
        self.exclusions.menu_row = PopoverAnchor::default();
        self.exclusions.menu_area = PopoverAnchor::default();
        self.exclusions.menu = Some((target, Rc::downgrade(&held)));
        cx.emit(crate::settings::SettingsEvent::Menu(
            crate::settings::SettingsMenu {
                items,
                at,
                row: self.exclusions.menu_row.clone(),
                area: self.exclusions.menu_area.clone(),
                held,
            },
        ));
        cx.notify();
    }

    /// What `target`'s menu offers, in its order groups (ADR-099): copy
    /// (Duplicate), change (Edit…, Move to, Reset; Remove last), make (a
    /// folder in a category). One of skwir's is never edited nor removed;
    /// Reset only where it is changed; Move to lists the categories it
    /// can go to.
    fn menu_items(&self, target: &Cursor) -> Vec<MenuItem> {
        let item = |title: &str, icon: &str, order: i32, args: serde_json::Value| {
            MenuItem::new(
                "settings_exclusions",
                title,
                icon,
                order,
                Invocation::with_args("ui.settings_exclusion_item", &args),
            )
        };
        let mut items = Vec::new();
        match target {
            Cursor::Entry(id) => {
                let Some(entry) = self.exclusion_entry(id) else {
                    return items;
                };
                let of = |what: &str| serde_json::json!({ "do": what, "entry": id });
                items.push(item("Duplicate", "copy", 100, of("duplicate")));
                if !entry.builtin {
                    items.push(item("Edit…", "rename", 200, of("edit")));
                }
                for (n, category) in model::categories(&self.exclusions.catalog)
                    .into_iter()
                    .filter(|c| c.id != entry.category)
                    .enumerate()
                {
                    let mut to = item(
                        &category.name,
                        "folder",
                        210 + i32::try_from(n).unwrap_or(0),
                        serde_json::json!({ "do": "move", "entry": id, "to": category.id }),
                    );
                    to.submenu = Some("Move to".to_owned());
                    items.push(to);
                }
                if entry.builtin {
                    if entry.changed() {
                        items.push(item("Reset", "revert", 250, of("reset")));
                    }
                } else {
                    items.push(item("Remove", "minus", 290, of("remove")));
                }
            }
            Cursor::Category(id) => {
                let of = |what: &str| serde_json::json!({ "do": what, "category": id });
                if self
                    .exclusions
                    .catalog
                    .category(id)
                    .is_some_and(|c| !c.builtin)
                {
                    items.push(item("Rename…", "rename", 200, of("rename")));
                    items.push(item("Delete", "trash", 290, of("delete")));
                }
                let add = format!("Add {} here…", an(model::ENTRY));
                items.push(item(&add, "plus", 300, of("add_here")));
            }
        }
        items
    }

    /// `ui.settings_exclusion_item`: what a row's menu chose.
    pub(crate) fn exclusion_item(
        &mut self,
        args: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = |name: &str| args.get(name).and_then(serde_json::Value::as_str);
        let (target, item) = match (text("do"), text("entry"), text("category")) {
            (Some(what), Some(id), _) => {
                let item = match what {
                    "duplicate" => Item::Duplicate,
                    "edit" => Item::Edit,
                    "reset" => Item::Reset,
                    "remove" => Item::Remove,
                    "move" => match text("to") {
                        Some(to) => Item::To(to.to_owned()),
                        None => return,
                    },
                    _ => return,
                };
                (Cursor::Entry(id.to_owned()), item)
            }
            (Some(what), None, Some(id)) => {
                let item = match what {
                    "rename" => Item::Rename,
                    "delete" => Item::Delete,
                    "add_here" => Item::AddHere,
                    _ => return,
                };
                (Cursor::Category(id.to_owned()), item)
            }
            _ => return,
        };
        self.run_item(&target, &item, window, cx);
    }

    /// What a menu's row, or Reset's button, does to its row.
    fn run_item(
        &mut self,
        target: &Cursor,
        item: &Item,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let catalog = Rc::clone(&self.exclusions.catalog);
        match (target, item) {
            (Cursor::Entry(id), Item::Duplicate) => {
                let (after, copy) = model::duplicated(&catalog, id);
                let Some(copy) = copy else { return };
                let folder = self.exclusion_words(id);
                let builtin = self.exclusion_entry(id).is_some_and(|e| e.builtin);
                let said = if builtin {
                    format!("Copied {folder}: the copy is yours to edit, skwir's is unticked")
                } else {
                    format!("Copied {folder}")
                };
                self.exclusions.cursor = Some(Cursor::Entry(copy.clone()));
                self.exclusions.reveal(&copy);
                self.propose_exclusions(after, said, window, cx);
                // Shown where its name puts it.
                self.reveal_cursor(cx);
                // The copy is there to be changed.
                if self.exclusions.asking.is_none() {
                    self.open_entry_dialog(Some(copy), None, window, cx);
                }
            }
            (Cursor::Entry(id), Item::Edit) => {
                self.open_entry_dialog(Some(id.clone()), None, window, cx);
            }
            (Cursor::Entry(id), Item::Reset) => {
                let after = model::reset(&catalog, id);
                let said = format!("{} is back as skwir ships it", self.exclusion_words(id));
                self.propose_exclusions(after, said, window, cx);
            }
            (Cursor::Entry(id), Item::Remove) => self.remove_exclusion(id, window, cx),
            (Cursor::Entry(id), Item::To(category)) => {
                let after = model::moved(&catalog, id, category);
                let said = format!(
                    "Moved {} to {}",
                    self.exclusion_words(id),
                    self.category_name(category)
                );
                self.fold_category(category, false);
                self.exclusions.reveal(id);
                self.propose_exclusions(after, said, window, cx);
                self.reveal_cursor(cx);
            }
            (Cursor::Category(id), Item::AddHere) => {
                self.open_entry_dialog(None, Some(id.clone()), window, cx);
            }
            (Cursor::Category(id), Item::Rename) => {
                self.open_category_dialog(Some(id.clone()), window, cx);
            }
            (Cursor::Category(id), Item::Delete) => {
                let name = self.category_name(id);
                let after = model::deleted_category(&catalog, id);
                let said = format!(
                    "Deleted the category {name}: its {} went back",
                    model::ENTRIES
                );
                // The keyboard on the heading before it.
                let rows = self.exclusion_rows(cx);
                self.exclusions.cursor = rows
                    .iter()
                    .position(|r| matches!(r, Row::Heading(h) if h.id == *id))
                    .and_then(|at| {
                        rows[..at]
                            .iter()
                            .rev()
                            .find(|r| matches!(r, Row::Heading(_)))
                            .and_then(Row::cursor)
                    });
                self.propose_exclusions(after, said, window, cx);
            }
            _ => {}
        }
    }

    fn fold_category(&mut self, id: &str, folded: bool) {
        self.exclusions.fold(id, folded);
    }

    /// Bring the cursor's row into view.
    fn reveal_cursor(&self, cx: &App) {
        let Some(cursor) = &self.exclusions.cursor else {
            return;
        };
        let rows = self.exclusion_rows(cx);
        if let Some(n) = rows
            .iter()
            .position(|r| r.cursor().as_ref() == Some(cursor))
        {
            self.exclusions
                .list
                .scroll_to_item(n, ScrollStrategy::Nearest);
        }
    }

    // ------------------------------------------------------------ keys

    /// `ui.settings_exclusion { key }`: walk the headings and rows, fold and
    /// unfold, turn a switch or a tick, remove one of the user's, open the
    /// row's menu.
    pub(crate) fn exclusions_key(
        &mut self,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.exclusions.dialog.is_some() || self.exclusions.asking.is_some() {
            return;
        }
        let rows = self.exclusion_rows(cx);
        let walked: Vec<(usize, Cursor)> = rows
            .iter()
            .enumerate()
            .filter_map(|(n, row)| row.cursor().map(|c| (n, c)))
            .collect();
        if walked.is_empty() {
            return;
        }
        let at = self
            .exclusions
            .cursor
            .as_ref()
            .and_then(|cursor| walked.iter().position(|(_, c)| c == cursor));
        let last = walked.len() - 1;
        let moved = match (key, at) {
            ("first", _) | (_, None) => Some(0),
            ("last", _) => Some(last),
            ("up", Some(at)) => Some(at.saturating_sub(1)),
            ("down", Some(at)) => Some((at + 1).min(last)),
            ("page_up", Some(at)) => Some(at.saturating_sub(PAGE)),
            ("page_down", Some(at)) => Some((at + PAGE).min(last)),
            _ => None,
        };
        if let Some(to) = moved {
            let (n, cursor) = walked[to].clone();
            self.exclusions.cursor = Some(cursor);
            self.exclusions
                .list
                .scroll_to_item(n, ScrollStrategy::Nearest);
            cx.notify();
            return;
        }
        let Some(at) = at else { return };
        let (n, cursor) = walked[at].clone();
        match (key, &cursor, &rows[n]) {
            ("left", Cursor::Category(id), Row::Heading(h)) if !h.folded => {
                self.fold_category(id, true);
            }
            ("left", Cursor::Entry(_), Row::Entry(line)) => {
                let heading = Cursor::Category(line.category.clone());
                if let Some(n) = rows
                    .iter()
                    .position(|r| r.cursor().as_ref() == Some(&heading))
                {
                    self.exclusions
                        .list
                        .scroll_to_item(n, ScrollStrategy::Nearest);
                }
                self.exclusions.cursor = Some(heading);
            }
            ("right", Cursor::Category(id), Row::Heading(h)) if h.folded => {
                self.fold_category(id, false);
            }
            ("open", Cursor::Category(id), _) => {
                let id = id.clone();
                self.switch_category(&id, window, cx);
            }
            ("open", Cursor::Entry(id), _) => {
                let id = id.clone();
                self.tick_exclusion(&id, window, cx);
            }
            ("remove", Cursor::Entry(id), _) => {
                let id = id.clone();
                self.remove_exclusion(&id, window, cx);
            }
            ("menu", _, _) => self.open_exclusion_menu(cursor.clone(), None, window, cx),
            _ => {}
        }
        cx.notify();
    }

    /// Escape, while the page has something open: closes it, the
    /// innermost first. Whether it closed anything.
    pub(crate) fn close_exclusions_popup(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.exclusions.asking.is_some() {
            self.keep_private(window, cx);
            return true;
        }
        if let Some(Dialog::Entry(dialog)) = &mut self.exclusions.dialog
            && dialog.category_open
        {
            dialog.category_open = false;
            cx.notify();
            return true;
        }
        if self.exclusions.dialog.is_some() {
            self.close_exclusion_dialog(window, cx);
            return true;
        }
        false
    }

    // ------------------------------------------------------------ dialogs

    fn field(
        &self,
        placeholder: &str,
        label: &str,
        text: &str,
        cx: &mut Context<Self>,
    ) -> Entity<TextInput> {
        let label = label.to_owned();
        let text = text.to_owned();
        let placeholder = placeholder.to_owned();
        cx.new(move |cx| {
            let mut input = TextInput::new(placeholder, cx);
            input.set_label(label);
            input.set_text(text, cx);
            input
        })
    }

    /// Add a folder (to `into`, else to Yours), or edit the user's `editing`.
    fn open_entry_dialog(
        &mut self,
        editing: Option<String>,
        into: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let draft = editing
            .as_deref()
            .and_then(|id| self.exclusion_entry(id))
            .map_or_else(
                || Draft {
                    category: into.clone().unwrap_or_else(|| YOURS.to_owned()),
                    ..Draft::default()
                },
                Draft::of,
            );
        let folder = self.field(
            "~/VMs, node_modules, cmake-build-*, regex:…",
            "Name or path",
            &draft.pattern,
            cx,
        );
        let beside = self.field("Cargo.toml, *.csproj", "Only beside", &draft.beside, cx);
        let holding = self.field(".nosearch, CACHEDIR.TAG", "Holding", &draft.holding, cx);
        let what = self.field("Virtual machines", "What it is", &draft.what, cx);
        let mut subscriptions = Vec::new();
        for input in [&folder, &beside, &holding, &what] {
            subscriptions.push(cx.subscribe_in(
                input,
                window,
                |this, _, event: &InputEvent, window, cx| match event {
                    InputEvent::Changed => {
                        this.judge_entry_dialog(cx);
                        cx.notify();
                    }
                    InputEvent::Submit => this.accept_entry_dialog(window, cx),
                    InputEvent::Cancel => this.close_exclusion_dialog(window, cx),
                },
            ));
        }
        let first = folder.read(cx).focus_handle(cx);
        let first = if draft.kind == Kind::Holding {
            holding.read(cx).focus_handle(cx)
        } else {
            first
        };
        self.exclusions.dialog = Some(Dialog::Entry(Box::new(EntryDialog {
            editing,
            kind: draft.kind,
            pattern: folder,
            names: draft.names,
            names_focus: cx.focus_handle(),
            beside,
            holding,
            what,
            category: draft.category,
            category_open: false,
            category_anchor: PopoverAnchor::default(),
            problem: None,
            means: None,
            already: None,
            here: None,
            found: Latest::default(),
            kind_focus: cx.focus_handle(),
            choose_focus: cx.focus_handle(),
            category_focus: cx.focus_handle(),
            cancel: cx.focus_handle(),
            accept: cx.focus_handle(),
            _subscriptions: subscriptions,
        })));
        self.judge_entry_dialog(cx);
        window.focus(&first, cx);
        cx.notify();
    }

    /// What the dialog's draft means, what is wrong with it, whether it
    /// is in the list already, and for a folder whether it is here
    /// (looked up on a worker): worked out as it is typed.
    fn judge_entry_dialog(&mut self, cx: &mut Context<Self>) {
        let catalog = Rc::clone(&self.exclusions.catalog);
        let role = self.exclusions.role;
        let Some(Dialog::Entry(dialog)) = &mut self.exclusions.dialog else {
            return;
        };
        let draft = dialog.draft(cx);
        let home = skwir_core::os_path::home();
        dialog.already = None;
        dialog.means = None;
        let matcher = match draft.matcher() {
            Ok(matcher) => matcher,
            Err(problem) => {
                // Said once something is typed: an empty form is no error.
                let typed = !draft.pattern.trim().is_empty()
                    || !draft.beside.trim().is_empty()
                    || !draft.holding.trim().is_empty();
                dialog.problem = typed.then_some(problem);
                dialog.here = None;
                dialog.found.forget();
                return;
            }
        };
        // Its own folders, unchanged, are no second copy.
        let unchanged = dialog
            .editing
            .as_deref()
            .and_then(|id| catalog.entries.iter().find(|e| e.id == id))
            .is_some_and(|e| e.matcher == matcher);
        if !unchanged
            && let Some(found) = model::already(
                &catalog,
                &matcher,
                dialog.editing.as_deref(),
                home.as_deref(),
            )
        {
            dialog.already = Some(found.id.clone());
        }
        let after = match &dialog.editing {
            Some(id) => (
                model::edited(&catalog, id, matcher.clone(), &draft),
                id.clone(),
            ),
            None => model::added(&catalog, matcher.clone(), &draft, role),
        };
        dialog.problem = model::problem(&after.0, &after.1).map(|why| {
            let field = match draft.kind {
                Kind::Beside if why.contains("beside") => Field::Beside,
                Kind::Pattern | Kind::Beside => Field::Pattern,
                Kind::Holding => Field::Holding,
            };
            (field, why)
        });
        if dialog.problem.is_none() {
            // A path's words are the path itself: say what it takes.
            dialog.means = Some(match matcher.one_path() {
                Some(text) => format!("{text} and all it holds"),
                None => matcher.said(),
            });
        }
        // Whether the folder typed is here: looked up on a worker.
        let one = matcher.one_path().map(str::to_owned);
        if dialog.here.as_ref().map(|(text, _)| text) != one.as_ref() {
            dialog.here = None;
        }
        match one.filter(|_| dialog.here.is_none() && dialog.problem.is_none()) {
            Some(text) => dialog.found.ask(
                &self.app,
                Lane::SHARED,
                self.app.lifetime(),
                Priority::Normal,
                "settings-exclusions-typed",
                move |_| {
                    let home = skwir_core::os_path::home();
                    model::presence(std::slice::from_ref(&text), home.as_deref())
                        .into_iter()
                        .next()
                },
            ),
            None => dialog.found.forget(),
        }
    }

    fn accept_entry_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.judge_entry_dialog(cx);
        let catalog = Rc::clone(&self.exclusions.catalog);
        let role = self.exclusions.role;
        let Some(Dialog::Entry(dialog)) = &mut self.exclusions.dialog else {
            return;
        };
        if let Some(found) = dialog.already.clone() {
            self.close_exclusion_dialog(window, cx);
            self.show_exclusion_entry(&found, cx);
            return;
        }
        let draft = dialog.draft(cx);
        let matcher = match draft.matcher() {
            Ok(matcher) => matcher,
            Err(problem) => {
                dialog.problem = Some(problem);
                cx.notify();
                return;
            }
        };
        if dialog.problem.is_some() {
            cx.notify();
            return;
        }
        let editing = dialog.editing.clone();
        let category = self.category_name(&draft.category);
        let (after, id, said) = if let Some(id) = editing {
            let after = model::edited(&catalog, &id, matcher.clone(), &draft);
            (
                after,
                id,
                format!("Changed {}", model::pattern_of(&matcher)),
            )
        } else {
            let (after, id) = model::added(&catalog, matcher.clone(), &draft, role);
            let said = format!("Added {} to {category}", model::pattern_of(&matcher));
            (after, id, said)
        };
        self.close_exclusion_dialog(window, cx);
        self.fold_category(&draft.category, false);
        self.exclusions.reveal(&id);
        self.exclusions.cursor = Some(Cursor::Entry(id));
        self.propose_exclusions(after, said, window, cx);
        self.reveal_cursor(cx);
    }

    /// Show the entry `id`: its category unfolded, shown whatever the
    /// filters say, the cursor on it.
    fn show_exclusion_entry(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(category) = self.exclusion_entry(id).map(|entry| entry.category.clone()) else {
            return;
        };
        self.fold_category(&category, false);
        self.exclusions.reveal(id);
        // The words typed would hide it.
        self.exclusions
            .filter
            .update(cx, |input, cx| input.set_text("", cx));
        self.exclusions.cursor = Some(Cursor::Entry(id.to_owned()));
        self.reveal_cursor(cx);
        cx.notify();
    }

    fn open_category_dialog(
        &mut self,
        renaming: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = renaming
            .as_deref()
            .map(|id| self.category_name(id))
            .unwrap_or_default();
        let name = self.field("Work, Virtual machines", "Name", &name, cx);
        let subscriptions =
            vec![
                cx.subscribe_in(
                    &name,
                    window,
                    |this, _, event: &InputEvent, window, cx| match event {
                        InputEvent::Changed => {
                            if let Some(Dialog::Category(dialog)) = &mut this.exclusions.dialog {
                                dialog.problem = None;
                            }
                            cx.notify();
                        }
                        InputEvent::Submit => this.accept_category_dialog(window, cx),
                        InputEvent::Cancel => this.close_exclusion_dialog(window, cx),
                    },
                ),
            ];
        let first = name.read(cx).focus_handle(cx);
        self.exclusions.dialog = Some(Dialog::Category(CategoryDialog {
            renaming,
            name,
            problem: None,
            cancel: cx.focus_handle(),
            accept: cx.focus_handle(),
            _subscriptions: subscriptions,
        }));
        window.focus(&first, cx);
        cx.notify();
    }

    fn accept_category_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let catalog = Rc::clone(&self.exclusions.catalog);
        let Some(Dialog::Category(dialog)) = &mut self.exclusions.dialog else {
            return;
        };
        let name = dialog.name.read(cx).text().trim().to_owned();
        let taken = catalog
            .categories
            .iter()
            .any(|c| Some(&c.id) != dialog.renaming.as_ref() && c.name.eq_ignore_ascii_case(&name));
        if name.is_empty() {
            dialog.problem = Some("A category needs a name".to_owned());
        } else if taken {
            dialog.problem = Some(format!("There is a category {name} already"));
        }
        if dialog.problem.is_some() {
            cx.notify();
            return;
        }
        let renaming = dialog.renaming.clone();
        self.close_exclusion_dialog(window, cx);
        let (after, id, said) = if let Some(id) = renaming {
            let said = format!("Renamed the category {} to {name}", self.category_name(&id));
            (model::renamed(&catalog, &id, &name), id, said)
        } else {
            let (after, id) = model::new_category(&catalog, &name);
            (after, id, format!("Made the category {name}"))
        };
        self.exclusions.cursor = Some(Cursor::Category(id));
        self.propose_exclusions(after, said, window, cx);
        self.reveal_cursor(cx);
    }

    fn close_exclusion_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.exclusions.dialog.take().is_some() {
            window.focus(&self.exclusions.focus, cx);
            cx.notify();
        }
    }

    fn keep_private(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Nothing changed: what the foot says stays.
        if self.exclusions.asking.take().is_some() {
            window.focus(&self.exclusions.focus, cx);
            cx.notify();
        }
    }

    fn let_agents_read(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(asking) = self.exclusions.asking.take() else {
            return;
        };
        window.focus(&self.exclusions.focus, cx);
        self.save_exclusions(asking.after, asking.said, cx);
    }

    /// What the page has open over the dialog: a question, the Add or
    /// Edit dialog, a category's name. While open it alone takes Tab
    /// (`stops`).
    pub(crate) fn exclusions_modal(
        &mut self,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let panel = if self.exclusions.asking.is_some() {
            self.asking_panel(theme, cx)
        } else {
            match self.exclusions.dialog.as_ref()? {
                Dialog::Entry(_) => self.entry_panel(theme, window, cx),
                Dialog::Category(_) => self.category_panel(theme, window, cx),
            }
        };
        Some(
            div()
                .absolute()
                .inset_0()
                .flex()
                .justify_center()
                .items_start()
                .pt(sized(72.))
                .bg(theme.scrim)
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        cx.stop_propagation();
                        if this.exclusions.asking.is_some() {
                            this.keep_private(window, cx);
                        } else {
                            this.close_exclusion_dialog(window, cx);
                        }
                    }),
                )
                .child(panel.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()))
                .into_any_element(),
        )
    }

    /// Before a change that takes folders from agents: which, and the two
    /// answers, Keep private first, the one Enter and Escape give.
    fn asking_panel(&mut self, theme: &Theme, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let asking = self.exclusions.asking.as_ref().expect("asked");
        let (keep, expose) = (asking.keep.clone(), asking.expose.clone());
        let question = model::exposure_question(&asking.folders);
        let folders = asking.folders.clone();
        let scroll = asking.scroll.clone();
        self.stops.clear();
        self.stop(&keep, None, None);
        self.stop(&expose, None, None);
        let list = div()
            .id("exclusions-exposed")
            .max_h(sized(22. * LISTED as f32))
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .px_2()
            .py_1()
            .rounded_md()
            .border_1()
            .border_color(theme.border)
            .bg(theme.bg)
            .flex()
            .flex_col()
            .children(
                folders
                    .into_iter()
                    .map(|folder| div().min_w_0().truncate().text_body().child(folder)),
            );
        crate::modal::panel("exclusions-asking", 460., theme)
            .role(gpui::Role::AlertDialog)
            .aria_label(question.clone())
            .p_5()
            .gap_3()
            .child(div().text_title().child("Let agents read these files and folders?"))
            .child(div().text_body().child(question))
            .child(list)
            .child(
                div()
                    .text_body()
                    .text_color(theme.text_muted)
                    .child("An agent driving skwir, a plugin from outside skwir and a model could then list them and read what they hold."),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap_2()
                    .child(
                        controls::button(
                            "exclusions-keep-private",
                            "Keep private",
                            Some("lock"),
                            ButtonStyle::Plain,
                            &keep,
                            theme,
                        )
                        .on_click(cx.listener(|this, _, window, cx| this.keep_private(window, cx))),
                    )
                    .child(
                        controls::button(
                            "exclusions-let-agents-read",
                            "Let agents read",
                            None,
                            ButtonStyle::Danger,
                            &expose,
                            theme,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.let_agents_read(window, cx);
                        })),
                    ),
            )
    }

    /// The Add and Edit dialog.
    #[allow(clippy::too_many_lines)] // one form, its fields in order
    fn entry_panel(
        &mut self,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let catalog = Rc::clone(&self.exclusions.catalog);
        let a11y = self.a11y;
        let Some(Dialog::Entry(dialog)) = &self.exclusions.dialog else {
            return div().id("exclusions-entry");
        };
        let stops = dialog.stops(cx);
        let editing = dialog.editing.is_some();
        let kind = dialog.kind;
        let names = dialog.names;
        let names_focus = dialog.names_focus.clone();
        let (folder, beside, holding, what) = (
            dialog.pattern.clone(),
            dialog.beside.clone(),
            dialog.holding.clone(),
            dialog.what.clone(),
        );
        let problem = dialog.problem.clone();
        let means = dialog.means.clone();
        let already = dialog.already.clone();
        let here = dialog.here.clone();
        let category = dialog.category.clone();
        let category_open = dialog.category_open;
        let category_anchor = dialog.category_anchor.clone();
        let (kind_focus, choose_focus, category_focus, cancel, accept) = (
            dialog.kind_focus.clone(),
            dialog.choose_focus.clone(),
            dialog.category_focus.clone(),
            dialog.cancel.clone(),
            dialog.accept.clone(),
        );
        self.stops.clear();
        for stop in &stops {
            self.stop(stop, None, None);
        }
        let label = |text: &'static str| div().text_body().text_color(theme.text_muted).child(text);
        let problem_under = |field: Field| {
            problem
                .as_ref()
                .filter(|(at, _)| *at == field)
                .map(|(_, why)| {
                    // A regex's error is several lines: the mark by the first.
                    controls::noted("warning", theme.error, 13., why.clone())
                        .text_body()
                        .text_color(theme.error)
                })
        };
        let text_field = |input: &Entity<TextInput>, field: Field| {
            let focused = input.read(cx).is_focused(window);
            let invalid = problem.as_ref().is_some_and(|(at, _)| *at == field);
            controls::field(input, None, focused, invalid, None, theme)
        };
        // What it names: a name or a path, a name beside a file, or the
        // folders holding a file. One list, so a kind more is one row more.
        let mut kinds = controls::segmented("exclusions-kind", &kind_focus, theme)
            .when(a11y, |d| d.aria_label("What it names"))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                let by = match event.keystroke.key.as_str() {
                    "left" => -1,
                    "right" => 1,
                    _ => return,
                };
                cx.stop_propagation();
                let next = step_kind(kind, by);
                this.set_entry_kind(next, cx);
            }));
        for (choice, words) in [
            (Kind::Pattern, "Name or path"),
            (Kind::Beside, "Name beside a file"),
            (Kind::Holding, "Folders holding a file"),
        ] {
            let focus = kind_focus.clone();
            kinds = kinds.child(
                controls::segment(
                    SharedString::from(format!("exclusions-kind-{words}")),
                    words,
                    kind == choice,
                    theme,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    window.focus(&focus, cx);
                    this.set_entry_kind(choice, cx);
                })),
            );
        }
        let mut form = div().flex().flex_col().gap_2().child(kinds);
        if kind == Kind::Pattern {
            // Files, folders or both: `*.kdbx` is no folder.
            const CHOICES: [(Names, &str); 3] = [
                (Names::Both, "Files and folders"),
                (Names::Files, "Files"),
                (Names::Folders, "Folders"),
            ];
            let mut which = controls::segmented("exclusions-names", &names_focus, theme)
                .when(a11y, |d| d.aria_label("Excludes"))
                .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                    let by = match event.keystroke.key.as_str() {
                        "left" => CHOICES.len() - 1,
                        "right" => 1,
                        _ => return,
                    };
                    cx.stop_propagation();
                    let at = CHOICES
                        .iter()
                        .position(|(choice, _)| *choice == names)
                        .unwrap_or(0);
                    this.set_entry_names(CHOICES[(at + by) % CHOICES.len()].0, cx);
                }));
            for (choice, words) in CHOICES {
                let focus = names_focus.clone();
                which = which.child(
                    controls::segment(
                        SharedString::from(format!("exclusions-names-{words}")),
                        words,
                        names == choice,
                        theme,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        window.focus(&focus, cx);
                        this.set_entry_names(choice, cx);
                    })),
                );
            }
            form = form.child(which);
        }
        match kind {
            Kind::Pattern => {
                form = form
                    .child(label("Name or path"))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(text_field(&folder, Field::Pattern))
                            .child(
                                controls::button(
                                    "exclusions-choose",
                                    "Choose folder…",
                                    Some("folder"),
                                    ButtonStyle::Plain,
                                    &choose_focus,
                                    theme,
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.choose_folder(cx))),
                            ),
                    )
                    .children(problem_under(Field::Pattern));
            }
            Kind::Beside => {
                form = form
                    .child(label("Name"))
                    .child(text_field(&folder, Field::Pattern))
                    .children(problem_under(Field::Pattern))
                    .child(label("Only beside"))
                    .child(text_field(&beside, Field::Beside))
                    .children(problem_under(Field::Beside));
            }
            Kind::Holding => {
                form = form
                    .child(label("Holding"))
                    .child(text_field(&holding, Field::Holding))
                    .children(problem_under(Field::Holding));
            }
        }
        // What it means, live, and whether a folder typed is here.
        let means_line = means.map(|means| {
            let place = here.map(|(_, here)| {
                if here {
                    "on this computer"
                } else {
                    "not on this computer"
                }
            });
            let text = match place {
                Some(place) => format!("Means {means}, {place}."),
                None => format!("Means {means}."),
            };
            div().text_body().text_color(theme.text_muted).child(text)
        });
        let already_line = already
            .as_ref()
            .and_then(|id| catalog.entries.iter().find(|e| e.id == *id))
            .map(|found| {
                let name = catalog
                    .category(&found.category)
                    .map_or_else(String::new, |c| c.name.clone());
                controls::noted(
                    "info",
                    theme.warning,
                    13.,
                    format!("Already in the list, in {name}: Show it, rather than a second."),
                )
                .text_body()
                .text_color(theme.warning)
            });
        form = form
            .children(means_line)
            .children(already_line)
            .child(label("What it is"))
            .child(text_field(&what, Field::What));
        // Its category: a dropdown; Left and Right step it.
        let category_name = catalog
            .category(&category)
            .map_or_else(|| category.clone(), |c| c.name.clone());
        let ids: Vec<(String, String)> = model::categories(&catalog)
            .into_iter()
            .map(|c| (c.id.clone(), c.name.clone()))
            .collect();
        let step_ids = ids.clone();
        let mut dropdown =
            controls::dropdown("exclusions-category", category_name, &category_focus, theme)
                .aria_expanded(category_open)
                .when(a11y, |d| d.aria_label("Category"))
                .child(category_anchor.marker())
                .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                    let by: isize = match event.keystroke.key.as_str() {
                        "left" | "up" => -1,
                        "right" | "down" => 1,
                        _ => return,
                    };
                    cx.stop_propagation();
                    if let Some(Dialog::Entry(dialog)) = &mut this.exclusions.dialog {
                        let at = step_ids
                            .iter()
                            .position(|(id, _)| *id == dialog.category)
                            .unwrap_or(0);
                        let len = step_ids.len().cast_signed();
                        let next = (at.cast_signed() + by).clamp(0, len - 1).cast_unsigned();
                        dialog.category.clone_from(&step_ids[next].0);
                        this.judge_entry_dialog(cx);
                        cx.notify();
                    }
                }))
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Some(Dialog::Entry(dialog)) = &mut this.exclusions.dialog {
                        dialog.category_open = !dialog.category_open;
                        cx.notify();
                    }
                }));
        if category_open {
            let hover = theme.hover;
            let mut panel = div()
                .id("exclusions-category-list")
                .min_w(sized(200.))
                .max_h(sized(320.))
                .overflow_y_scroll()
                .p_1()
                .flex()
                .flex_col()
                .rounded_md()
                .border_1()
                .border_color(theme.border)
                .bg(theme.bg)
                .text_color(theme.text)
                .text_body()
                .shadow_lg()
                .occlude()
                .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, cx| {
                    if let Some(Dialog::Entry(dialog)) = &mut this.exclusions.dialog {
                        dialog.category_open = false;
                        cx.notify();
                    }
                }));
            for (id, name) in ids {
                let chosen = id == category;
                panel = panel.child(
                    div()
                        .id(SharedString::from(format!("exclusions-category-{id}")))
                        .h(sized(26.))
                        .px_2()
                        .flex()
                        .items_center()
                        .gap_2()
                        .rounded_sm()
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover))
                        .child(div().w(px(14.)).when(chosen, |d| {
                            d.child(controls::glyph("check", theme.accent, 14.))
                        }))
                        .child(name)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if let Some(Dialog::Entry(dialog)) = &mut this.exclusions.dialog {
                                dialog.category.clone_from(&id);
                                dialog.category_open = false;
                                let focus = dialog.category_focus.clone();
                                window.focus(&focus, cx);
                                this.judge_entry_dialog(cx);
                                cx.notify();
                            }
                        })),
                );
            }
            dropdown = dropdown.child(category_anchor.popup(panel));
        }
        form = form
            .child(label("Category"))
            .child(div().flex().child(dropdown));
        let answer = if already.is_some() {
            "Show it"
        } else if editing {
            "Save"
        } else {
            "Add"
        };
        let hint = if already.is_some() {
            "Enter shows it in the list".to_owned()
        } else if editing {
            "Enter saves it".to_owned()
        } else {
            format!(
                "Enter adds it, {}",
                model::excluded_from(self.exclusions.role)
            )
        };
        let title = if editing {
            format!("Edit {}", model::ENTRY)
        } else {
            format!("Add {}", an(model::ENTRY))
        };
        crate::modal::panel("exclusions-entry", 520., theme)
            .aria_label(title.clone())
            .p_5()
            .gap_3()
            .child(div().text_title().child(title))
            .child(form)
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_end()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w(sized(180.))
                            .text_body()
                            .text_color(theme.text_muted)
                            .child(hint),
                    )
                    .child(
                        controls::button(
                            "exclusions-entry-cancel",
                            "Cancel",
                            None,
                            ButtonStyle::Plain,
                            &cancel,
                            theme,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.close_exclusion_dialog(window, cx);
                        })),
                    )
                    .child(
                        controls::button(
                            "exclusions-entry-accept",
                            answer,
                            Some("check"),
                            ButtonStyle::Plain,
                            &accept,
                            theme,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.accept_entry_dialog(window, cx);
                        })),
                    ),
            )
    }

    fn set_entry_names(&mut self, names: Names, cx: &mut Context<Self>) {
        if let Some(Dialog::Entry(dialog)) = &mut self.exclusions.dialog {
            dialog.names = names;
            dialog.problem = None;
            self.judge_entry_dialog(cx);
            cx.notify();
        }
    }

    fn set_entry_kind(&mut self, kind: Kind, cx: &mut Context<Self>) {
        if let Some(Dialog::Entry(dialog)) = &mut self.exclusions.dialog {
            dialog.kind = kind;
            dialog.problem = None;
            self.judge_entry_dialog(cx);
            cx.notify();
        }
    }

    /// The platform's folder picker: the folder chosen, written from `~`.
    fn choose_folder(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose".into()),
        });
        cx.spawn(async move |this, cx| {
            let picked = paths.await;
            this.update(cx, |this, cx| {
                let Ok(Ok(Some(paths))) = picked else { return };
                let Some(path) = paths.first() else { return };
                let text = skwir_core::exclusions::tilde(path);
                if let Some(Dialog::Entry(dialog)) = &mut this.exclusions.dialog {
                    dialog.kind = Kind::Pattern;
                    dialog.names = Names::Folders;
                    dialog
                        .pattern
                        .update(cx, |input, cx| input.set_text(text, cx));
                    this.judge_entry_dialog(cx);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// A category's name, new or renamed.
    fn category_panel(
        &mut self,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let Some(Dialog::Category(dialog)) = &self.exclusions.dialog else {
            return div().id("exclusions-category");
        };
        let renaming = dialog.renaming.is_some();
        let name = dialog.name.clone();
        let problem = dialog.problem.clone();
        let (cancel, accept) = (dialog.cancel.clone(), dialog.accept.clone());
        let field_focus = name.read(cx).focus_handle(cx);
        self.stops.clear();
        self.stop(&field_focus, None, None);
        self.stop(&cancel, None, None);
        self.stop(&accept, None, None);
        let focused = name.read(cx).is_focused(window);
        let title = if renaming {
            "Rename category"
        } else {
            "New category"
        };
        crate::modal::panel("exclusions-category", 420., theme)
            .aria_label(title)
            .p_5()
            .gap_3()
            .child(div().text_title().child(title))
            .child(controls::field(&name, None, focused, problem.is_some(), None, theme))
            .children(problem.map(|why| {
                controls::noted("warning", theme.error, 13., why)
                    .text_body()
                    .text_color(theme.error)
            }))
            .when(!renaming, |d| {
                d.child(
                    div()
                        .text_body()
                        .text_color(theme.text_muted)
                        .child(format!(
                            "Its {} hold for every role you tick them for; its switches turn them all off at once.",
                            model::ENTRIES
                        )),
                )
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap_2()
                    .child(
                        controls::button(
                            "exclusions-category-cancel",
                            "Cancel",
                            None,
                            ButtonStyle::Plain,
                            &cancel,
                            theme,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.close_exclusion_dialog(window, cx);
                        })),
                    )
                    .child(
                        controls::button(
                            "exclusions-category-accept",
                            if renaming { "Rename" } else { "Create" },
                            Some("check"),
                            ButtonStyle::Plain,
                            &accept,
                            theme,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.accept_category_dialog(window, cx);
                        })),
                    ),
            )
    }

    // ------------------------------------------------------------ search

    /// The exclusions the dialog's search finds, said in one line with
    /// the way to them (the page, filtered by the words searched); how
    /// many lines it added.
    pub(crate) fn exclusions_found(
        &mut self,
        query: &str,
        items: &mut Vec<AnyElement>,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> usize {
        let generation = self.exclusions.generation;
        let kept = self
            .exclusions
            .found
            .borrow()
            .as_ref()
            .filter(|(at, words, _)| *at == generation && words == query)
            .map(|(_, _, count)| *count);
        let count = kept.unwrap_or_else(|| {
            // Words searched: every category shows what they find, folded or not.
            let (here, unfolded, revealed) = (HashMap::new(), HashSet::new(), HashSet::new());
            let rows = model::rows(
                &self.exclusions.catalog,
                &Ask {
                    role: Role::Searches,
                    filter: query,
                    only_here: false,
                    here: &here,
                    changed_only: false,
                    unfolded: &unfolded,
                    revealed: &revealed,
                },
            );
            let count = rows.iter().filter(|r| matches!(r, Row::Entry(_))).count();
            *self.exclusions.found.borrow_mut() = Some((generation, query.to_owned(), count));
            count
        });
        if count == 0 {
            return 0;
        }
        let focus = self.handle("exclusions-found", cx);
        self.stop(&focus, Some(items.len()), None);
        let words = query.to_owned();
        items.push(
            div()
                .flex_none()
                .pt_5()
                .pb_2()
                .text_small()
                .text_color(theme.text_muted)
                .child(page_title())
                .into_any_element(),
        );
        items.push(
            card_row(
                theme,
                Edge {
                    first: true,
                    last: true,
                },
            )
            .child(div().flex_1().min_w_0().child(if count == 1 {
                format!("One {} matches “{query}”", model::ENTRY)
            } else {
                format!("{count} {} match “{query}”", model::ENTRIES)
            }))
            .child(
                controls::button(
                    "exclusions-found",
                    "Show them",
                    None,
                    ButtonStyle::Plain,
                    &focus,
                    theme,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.go(crate::settings::Page::Exclusions, cx);
                    this.exclusions.only_here = false;
                    let words = words.clone();
                    this.exclusions
                        .filter
                        .update(cx, |input, cx| input.set_text(words, cx));
                    window.focus(&this.exclusions.filter.focus_handle(cx), cx);
                })),
            )
            .into_any_element(),
        );
        1
    }

    // ------------------------------------------------------------ Privacy

    /// The Privacy page's way to the folders kept from agents.
    pub(crate) fn privacy_folders(
        &mut self,
        items: &mut Vec<AnyElement>,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) {
        let focus = self.handle("privacy-exclusions", cx);
        self.stop(&focus, Some(items.len()), None);
        let kept = self.exclusions.catalog.kept_from(Role::Agents).count();
        items.push(
            card_row(
                theme,
                Edge {
                    first: true,
                    last: true,
                },
            )
            .mt_4()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(sized(3.))
                    .child(page_title())
                    .child(
                        div()
                            .text_body()
                            .text_color(theme.text_muted)
                            .child(format!(
                                "What agents, plugins from outside skwir and models never see: \
                         {kept} {}, skwir's secrets and yours.",
                                model::ENTRIES
                            )),
                    ),
            )
            .child(
                controls::button(
                    "privacy-exclusions",
                    format!("{}…", page_title()),
                    Some("lock"),
                    ButtonStyle::Plain,
                    &focus,
                    theme,
                )
                .on_click(cx.listener(|this, _, _, cx| this.show_exclusions(Role::Agents, cx))),
            )
            .into_any_element(),
        );
    }
}

/// The page's name, as the dialog's sidebar and title say it.
fn page_title() -> &'static str {
    crate::settings::Page::Exclusions
        .builtin()
        .map_or("", |page| page.title)
}

/// `word` after its article: "an exclusion".
fn an(word: &str) -> String {
    let vowel = word.starts_with(['a', 'e', 'i', 'o', 'u']);
    format!("{} {word}", if vowel { "an" } else { "a" })
}

/// The next kind of what an entry names, `by` along, round the ends.
fn step_kind(kind: Kind, by: isize) -> Kind {
    const ALL: [Kind; 3] = [Kind::Pattern, Kind::Beside, Kind::Holding];
    let at = ALL.iter().position(|k| *k == kind).unwrap_or(0);
    ALL[(at.cast_signed() + by).rem_euclid(3).cast_unsigned()]
}

#[cfg(test)]
mod tests {
    use super::{Kind, step_kind};

    #[test]
    fn the_kinds_step_round() {
        assert_eq!(step_kind(Kind::Pattern, -1), Kind::Holding);
        assert_eq!(step_kind(Kind::Holding, 1), Kind::Pattern);
    }
}
