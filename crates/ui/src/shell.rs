//! The shell entity: owns the window's UI state (per pane and global),
//! routes every command, and wakes only when the core signals new events.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Instant;

use bibli_core::app::NotifyLevel;
use bibli_core::command::{Invocation, Outcome};
use bibli_core::os_path::PathArg;
use bibli_core::workspace::PaneId;
use bibli_core::{App, Location, Plan, SortKey};
use gpui::{
    ClipboardItem, Context, Entity, FocusHandle, IntoElement, SharedString, Subscription, Window,
    div, prelude::*, px,
};

use crate::choose::ChooseState;
use crate::icons::IconCache;
use crate::input::{InputEvent, TextInput};
use crate::jump::JumpState;
use crate::keymap::{self, KeyHint, RunCommand};
use crate::list::{ListState, Step};
use crate::menu::MenuState;
use crate::palette::PaletteState;
use crate::preview::PreviewState;
use crate::prompts::{ConfirmState, PromptKind, PromptState};
use crate::renamebulk::BulkState;
use crate::theme::Theme;
use crate::thumbs::ThumbCache;
use crate::timeline::TimelineState;
use crate::usage::UsageState;
use crate::view::{self, Nav, PaneView};
use crate::views::grid::{DEFAULT_ZOOM, ZOOMS};
use crate::{
    address, jump, menu, palette, preview, prompts, renamebulk, review, sidebar, status, timeline,
};

/// What the shell says when a command needs a selection and there is none.
pub const NOTHING_SELECTED: &str = "Select a file or folder first";

/// What the shelf publishes in `App::tallies`: how many items it holds.
pub const SHELF_ITEMS: &str = "shelf.items";

/// UI state of one pane: focus, address bar, cached order, views.
pub struct PaneUi {
    pub id: PaneId,
    pub focus: FocusHandle,
    pub address: Entity<TextInput>,
    /// The location string the address bar currently mirrors.
    pub address_shown: String,
    pub list: ListState,
    /// Instantiated views by id; the active one is taken out during render.
    pub views: HashMap<&'static str, Box<dyn PaneView>>,
    view_registrations: HashMap<String, bibli_core::registry::Factory>,
    /// Registry revision the registrations above were reconciled against.
    views_revision: u64,
    pub view_id: &'static str,
    /// The view showing the listing last (list, grid, columns), where a
    /// lens goes back to.
    pub listing_view: &'static str,
    pub usage: UsageState,
    /// The Tags view's counts and selection (`tags_view.rs`).
    pub tags: crate::tags_view::TagsLens,
    pub loading: crate::loading::State,
    /// What this pane last searched for, for its "Nothing found".
    pub last_search: String,
    /// The address field while it is typed in (`omnibox.rs`).
    pub omni: Option<crate::omnibox::Omni>,
    /// What the files it shows weigh, for its footer (`weight.rs`).
    pub weight: crate::weight::Weight,
    /// What its search of tags can be narrowed to.
    pub narrow: crate::narrow::Narrow,
    /// The chips of the search it shows (`search_bar`).
    pub chips: crate::search_bar::Chips,
    /// The trail and verbs of the location it shows, when that is no
    /// folder (`trail.rs`).
    pub place: crate::trail::Shown,
    /// The placeholder's button, kept so Tab can reach it.
    pub(crate) placeholder_focus: FocusHandle,
    /// Its pane came back (or is new) and has not been put in place yet
    /// (`Shell::place_pane`).
    pub(crate) returning: bool,
    /// Where it was scrolled when its tab was left: the entry at the top,
    /// its index then, and how many pixels into its row the view began.
    pub(crate) anchor: Option<(bibli_core::EntryId, usize, f32)>,
    /// What it will show first, still waiting for metadata.
    pub(crate) warm: Vec<bibli_core::EntryId>,
    _sub: Subscription,
}

impl PaneUi {
    /// Its pane went dormant with its tab (ADR-018): nothing it drew from is
    /// kept. The views start over when the listing comes back, as after a
    /// navigation.
    pub(crate) fn let_go(&mut self) {
        self.anchor = self
            .views
            .get(self.view_id)
            .and_then(|view| view.scroll_anchor())
            .and_then(|(ix, within)| Some((*self.list.sorted.get(ix)?, ix, within)));
        let (rows, items) = (self.list.visible_rows, self.list.visible_items);
        self.list = ListState::default();
        self.list.visible_rows = rows;
        self.list.visible_items = items;
        self.returning = true;
        self.warm.clear();
        for view in self.views.values_mut() {
            view.reset();
        }
        self.usage = UsageState::default();
        self.loading = crate::loading::State::default();
    }
}

/// A pending "show me this file": the pane it belongs to, the file name to
/// look for, and when to stop waiting for a listing that never brings it.
struct Reveal {
    pane: PaneId,
    name: std::ffi::OsString,
    until: Instant,
}

/// The shell's switches read from the settings (`Shell::sync`, when their
/// revision changes), never per frame.
/// Where Split and Preview are (`Shell::toggles_at`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TogglesAt {
    Preview,
    Strip,
    Pane(PaneId),
}

// Independent switches, one per setting, each read from its own key: not
// states of one thing that an enum would name.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug)]
pub struct Preferences {
    /// `ui.date_format`: Today 14:02 rather than an ISO date.
    pub friendly_dates: bool,
    /// `ui.grid_fill`: thumbnails cropped to their box.
    pub grid_fill: bool,
    /// `ui.single_tab`: the tab of a window that has only one is drawn.
    pub single_tab: bool,
    /// `ui.printed_pages`: a book's position counts its printed pages.
    pub printed_pages: bool,
    /// How audio and video play.
    pub player: crate::player::PlayerPrefs,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            friendly_dates: true,
            grid_fill: false,
            single_tab: false,
            printed_pages: false,
            player: crate::player::PlayerPrefs::default(),
        }
    }
}

/// The keymap GPUI has: `sync` installs it again when the registries or
/// the user's bindings change.
pub(crate) struct Installed {
    stamp: keymap::Stamp,
    /// The settings revision the user's bindings were last read at.
    settings: u64,
    /// Whether letters typed in the file list jump to a name
    /// (`keymap::Keymap::typeahead`).
    pub typeahead: bool,
}

pub struct Shell {
    pub app: Arc<App>,
    /// The pane commands act on: the workspace's active pane, or the pane
    /// being rendered while a pane renders.
    pub pane: PaneId,
    pub panes: Vec<PaneUi>,
    /// The panes of the tabs not shown, what they drew from let go
    /// (`Shell::sync_tabs`).
    pub(crate) hidden: HashMap<PaneId, PaneUi>,
    /// Workspace revision the panes above follow.
    pub(crate) tabs_revision: u64,
    pub(crate) shown_tab: Option<bibli_core::workspace::TabId>,
    /// The splits of tabs not shown: their own arrangement and share.
    pub(crate) tab_splits: HashMap<bibli_core::workspace::TabId, crate::layout::TabSplit>,
    pub(crate) tab_labels: crate::tabs::Labels,
    /// The panes of the tab left, still drawn while the tab shown gets
    /// ready (`Shell::present`), and which tab they are.
    pub(crate) presented: Option<Vec<PaneUi>>,
    pub(crate) presented_tab: Option<bibli_core::workspace::TabId>,
    /// When the tab shown started waiting to be drawn, and the wake at its
    /// deadline.
    pub(crate) held_since: Option<Instant>,
    pub(crate) hold_timer: Option<gpui::Task<()>>,
    pub font: SharedString,
    pub mono_font: SharedString,
    pub key_hints: Vec<KeyHint>,
    /// What `key_hints` and GPUI's bindings were built from.
    pub(crate) keys: Installed,
    /// Icon size of the grid's tiles.
    pub zoom: u32,
    /// Icon size of the list's and the columns' rows; their height follows.
    pub row_zoom: u32,
    /// The sidebar put away with F9. A narrow window hides it too.
    pub sidebar_hidden: bool,
    /// How wide the sidebar was dragged (`Shell::sidebar_drawn_width`).
    pub sidebar_width: f32,
    /// The first pane's share of the shown tab's split, the second has the
    /// rest. Each tab keeps its own (`tab_splits`); a new split starts even.
    pub split_share: f32,
    pub thumbs: ThumbCache,
    pub icons: IconCache,
    pub type_names: crate::typenames::TypeNames,
    /// Switches of the settings, resolved when their revision changes.
    pub prefs: Preferences,
    /// Listing events were left for the next sync (`PumpReport::backlog`).
    backlog: bool,
    /// Preferences changed from the toolbars and not saved yet
    /// (`settings::remember`).
    pub(crate) unsaved: std::collections::BTreeMap<&'static str, String>,
    /// How the user arranged the sidebar's groups (`ui.sidebar_section`).
    pub sidebar_sections: crate::sidebar::Sections,
    /// Size, Modified, Type, Rating: which detail columns the list shows
    /// (`settings::COLUMN_KEYS`). Read through `shown_columns`.
    pub columns: [bool; 4],
    /// `reading.width`: the reading column's widest, in ems, or `None`
    /// for the whole panel (`markdown::measure`).
    pub reading_measure: Option<f32>,
    /// The reading width before Full width was switched on from a preview,
    /// to return to when it is switched off.
    reading_width_before: Option<String>,
    /// Ctrl and the wheel over the grid: a trackpad must mean it.
    pub zoom_wheel: crate::reader_scroll::WheelTurn,
    /// The wheel over the tab strip, counted in notches like the zoom's.
    pub tab_wheel: crate::reader_scroll::WheelTurn,
    pub column_widths: [f32; 4],
    /// A header edge being dragged: column, pointer x and width at the start.
    pub column_drag: Option<(usize, f32, f32)>,
    pub preview: PreviewState,
    /// A track kept playing while the selection moves (`player.rs`).
    pub player: crate::player::PlayerState,
    pub palette: Option<PaletteState>,
    pub jump: Option<JumpState>,
    /// `ui.choose`: a list to pick one from.
    pub choose: Option<ChooseState>,
    /// `ui.tags_prompt`: the tags of the selection, to change.
    pub tag_editor: Option<crate::tags::TagEditor>,
    /// The colors the user chose for tags, read when the settings change.
    pub tag_colors: crate::tags::TagColors,
    /// The categories a file has one tag of at a time: fields
    /// (`metadata::TAG_EXCLUSIVE_SETTING`), read when the settings change.
    pub tag_fields: Vec<String>,
    pub bulk: Option<BulkState>,
    pub prompt: Option<PromptState>,
    pub confirm: Option<ConfirmState>,
    pub menu: Option<MenuState>,
    pub toolbar_menu: Option<crate::toolbar::Popup>,
    /// A pane row's menu of places: the history, or a folder's folders.
    pub nav_menu: Option<crate::nav_menu::NavMenu>,
    /// Counts the folder menus asked for: an answer for an older one is
    /// dropped.
    pub nav_asked: u64,
    /// How wide the sidebar is drawn this frame, from the window's size
    /// (`sidebar_drawn_width`; 0 when it is not): drawn, its header holds
    /// the window's toggle and More, else the first pane's row does.
    pub(crate) sidebar_drawn: f32,
    /// Where More hangs from this frame, wherever its button is.
    pub(crate) more_anchor: crate::elements::popover::PopoverAnchor,
    /// Counts the presses of a mouse button in the window, and which popup
    /// the last press closed from outside (`note_dismissed`).
    pub(crate) presses: u64,
    pub(crate) dismissed: Option<(u64, String)>,
    /// A search chip's open choices.
    pub search_menu: Option<crate::search_bar::ChipMenu>,
    pub settings: Option<Entity<crate::settings::SettingsView>>,
    pub(crate) settings_subscription: Option<Subscription>,
    /// Settings revision the theme was last resolved against.
    theme_revision: u64,
    /// Plans proposed by background work, oldest first; the front is shown.
    pub reviews: VecDeque<crate::review::Review>,
    pub review_focus: Option<crate::review::ReviewFocus>,
    pub timeline: TimelineState,
    /// The tiles maps are drawn on, while a map is shown (`map_tiles`).
    pub(crate) map_tiles: std::rc::Rc<std::cell::RefCell<crate::map_tiles::TileStore>>,
    pub(crate) map_notes: crate::map_preview::Notes,
    /// Panes that showed a Map view: their scan is let go when they stop.
    pub(crate) map_views: std::collections::HashSet<PaneId>,
    /// Panes whose Map view spreads the files at one place
    /// (`ui.map_view_spread`).
    pub(crate) map_spread: std::collections::HashSet<PaneId>,
    /// The area each pane's Map view drew last, for `ui.map_view_search_area`.
    pub(crate) map_areas:
        std::collections::HashMap<PaneId, std::rc::Rc<std::cell::Cell<Option<[f64; 4]>>>>,
    /// The terminal under the panes (F4).
    pub terminal: crate::terminal::TerminalState,
    /// Panes side by side or stacked, the preview at the right or the bottom.
    pub layout: crate::layout::Layout,
    pub toasts: crate::toast::ToastState,
    /// This frame's snapshot of what waits for a paste; rows and cells test
    /// membership here instead of taking the lock each.
    pub pending: Arc<bibli_core::pending::Pending>,
    pub shortcuts: Option<crate::shortcuts::Overview>,
    /// A rubber-band selection in progress.
    pub marquee: Option<crate::elements::marquee::Drag>,
    /// The encrypted volume the passphrase prompt is for.
    pub unlock: Option<crate::prompts::Unlock>,
    pub nudges: crate::shortcuts::Nudges,
    /// What `/` looked for last, and whether backward: `n` and `N` look
    /// for it again.
    pub last_find: Option<(String, bool)>,
    /// Keys typed that wait for the next, and what can follow.
    pub(crate) sequence: Option<crate::sequence::Sequence>,
    pub shelf_count: usize,
    /// `App::tallies` revision `shelf_count` was read at.
    tallies_revision: Option<u64>,
    /// A plain press on a row of a multi-selection, waiting for its
    /// release to make the selection that row (unless a drag starts).
    pub press_collapse: Option<(PaneId, bibli_core::EntryId)>,
    /// A file to select once the folder it lives in has listed it: a listing
    /// streams, so a link followed to another folder cannot select its target
    /// in the same frame it navigates.
    reveal: Option<Reveal>,
    /// Advances with each `reveal_path`, so only the latest one's answer
    /// from its worker is acted on.
    reveal_asked: u64,
    /// Links as the pointer and Ctrl left them, and a line waiting for its
    /// file's preview (`links.rs`).
    pub(crate) links: crate::links::LinkState,
    /// The window's shape as last written to settings; `remember_session`
    /// compares against it so nothing is written on an ordinary frame.
    /// `None` until the last window's shape was read (`restore_session`):
    /// until then nothing is written.
    pub(crate) stored_session: Option<crate::session::Session>,
    /// Scroll position of the sidebar, so it can have a scrollbar.
    pub sidebar_scroll: gpui::ScrollHandle,
    started: Instant,
    first_frame_logged: bool,
    _subscriptions: Vec<Subscription>,
}

impl Shell {
    pub fn new(
        app: Arc<App>,
        initial: Option<Location>,
        started: Instant,
        font: SharedString,
        mono_font: SharedString,
        keymap: keymap::Keymap,
        wake_rx: flume::Receiver<()>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        crate::settings::declare(&app);
        crate::session::declare(&app);
        // Owner and group names for Details: two small files, but a read can
        // block, so not here.
        let wake = app.waker_handle();
        app.scheduler.spawn(
            bibli_core::scheduler::Priority::Background,
            "user-names",
            app.lifetime().child(),
            {
                let cache = app.cache.root().to_owned();
                move || {
                    bibli_platform::users::load();
                    // Members a killed process extracted for its previews.
                    crate::preview::sweep_members(&cache);
                    wake();
                }
            },
        );
        cx.set_global(Theme::resolve(&app, window.appearance()));
        let mut subs = Vec::new();
        subs.push(cx.observe_window_appearance(window, |this, window, cx| {
            cx.set_global(Theme::resolve(&this.app, window.appearance()));
            cx.notify();
        }));
        subs.push(cx.observe_pending_input(window, Self::on_pending_input));
        // The keyboard is never left with nothing to act on. When what had
        // focus leaves the window (a tab or pane closed, a placeholder or a
        // toolbar variant replaced, a dialog giving focus back to something
        // gone), GPUI routes keys from above the shell's root, where no
        // binding and no action handler is: every key, Ctrl+W included,
        // would do nothing until a click. The pane drawn as active takes
        // it. Deferred: GPUI reports the loss while drawing, when moving
        // focus cannot ask for the frame that shows it.
        subs.push(cx.on_focus_lost(window, |_, window, cx| {
            cx.defer_in(window, |this, window, cx| {
                if let Some(focus) = this.drawn_focus() {
                    window.focus(&focus);
                    cx.notify();
                }
            });
        }));

        // Wake-up, not polling: the core signals, the shell applies what it
        // sent, GPUI re-renders. Applying is done here, not left to the draw
        // that `notify` asks for: a window the compositor is not showing (a
        // workspace that is not on screen) is never asked to draw, and what
        // the core sent (a media key's request, a track that ended) must not
        // wait until it is. `render` syncs too, for what the user did since.
        cx.spawn_in(window, async move |this, cx| {
            loop {
                if wake_rx.recv_async().await.is_err() {
                    break;
                }
                loop {
                    while wake_rx.try_recv().is_ok() {}
                    let Ok(backlog) = this.update_in(cx, |this, window, cx| {
                        this.sync(window, cx);
                        cx.notify();
                        this.backlog
                    }) else {
                        return;
                    };
                    if !backlog {
                        break;
                    }
                    // A listing is still being applied, a slice per sync
                    // (`PUMP_BUDGET`): give the event loop a turn, so the
                    // window draws what has arrived, then go on. The wait
                    // must go through the platform's loop: a zero timer is
                    // a ready task in gpui, and yields to nothing.
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(1))
                        .await;
                }
            }
        })
        .detach();

        // Friendly dates say "Today" and "Yesterday": repaint once when the
        // day changes. One timer per day, not a clock.
        cx.spawn(async move |this, cx| {
            loop {
                let now = jiff::Zoned::now();
                let wait = now
                    .date()
                    .tomorrow()
                    .ok()
                    .and_then(|date| date.to_zoned(now.time_zone().clone()).ok())
                    .and_then(|midnight| now.duration_until(&midnight).try_into().ok())
                    .unwrap_or(std::time::Duration::from_secs(3600));
                cx.background_executor()
                    .timer(wait + std::time::Duration::from_secs(1))
                    .await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        })
        .detach();

        let pane = app.workspace.read().active_id();
        let map_tiles = std::rc::Rc::new(std::cell::RefCell::new(
            crate::map_tiles::TileStore::new(&app),
        ));
        let mut shell = Shell {
            app,
            pane,
            panes: Vec::new(),
            hidden: HashMap::new(),
            tabs_revision: u64::MAX,
            shown_tab: None,
            tab_splits: HashMap::new(),
            tab_labels: crate::tabs::Labels::default(),
            presented: None,
            presented_tab: None,
            held_since: None,
            hold_timer: None,
            font,
            mono_font,
            key_hints: keymap.hints,
            keys: Installed {
                stamp: keymap.stamp,
                settings: 0,
                typeahead: keymap.typeahead,
            },
            zoom: DEFAULT_ZOOM,
            row_zoom: crate::views::list::DEFAULT_ROW_ZOOM,
            sidebar_hidden: false,
            sidebar_width: sidebar::DEFAULT_WIDTH,
            split_share: SPLIT_SHARE,
            thumbs: ThumbCache::default(),
            icons: IconCache::default(),
            type_names: crate::typenames::TypeNames::default(),
            prefs: Preferences::default(),
            unsaved: std::collections::BTreeMap::new(),
            backlog: false,
            sidebar_sections: crate::sidebar::Sections::default(),
            columns: [true, true, true, false],
            reading_measure: crate::markdown::measure("comfortable"),
            reading_width_before: None,
            zoom_wheel: crate::reader_scroll::WheelTurn::default(),
            tab_wheel: crate::reader_scroll::WheelTurn::default(),
            column_widths: crate::views::list::DEFAULT_WIDTHS,
            column_drag: None,
            preview: PreviewState::default(),
            player: crate::player::PlayerState::default(),
            palette: None,
            jump: None,
            choose: None,
            tag_editor: None,
            tag_colors: crate::tags::TagColors::default(),
            tag_fields: Vec::new(),
            bulk: None,
            prompt: None,
            confirm: None,
            menu: None,
            toolbar_menu: None,
            nav_menu: None,
            nav_asked: 0,
            presses: 0,
            dismissed: None,
            sidebar_drawn: 0.,
            more_anchor: crate::elements::popover::PopoverAnchor::default(),
            search_menu: None,
            settings: None,
            settings_subscription: None,
            theme_revision: 0,
            reviews: VecDeque::new(),
            review_focus: None,
            timeline: TimelineState::default(),
            map_tiles,
            map_notes: crate::map_preview::Notes::default(),
            map_views: std::collections::HashSet::new(),
            map_spread: std::collections::HashSet::new(),
            map_areas: std::collections::HashMap::new(),
            terminal: crate::terminal::TerminalState::new(cx.focus_handle()),
            layout: crate::layout::Layout::default(),
            toasts: crate::toast::ToastState::default(),
            pending: Arc::default(),
            shortcuts: None,
            marquee: None,
            unlock: None,
            nudges: crate::shortcuts::Nudges::default(),
            last_find: None,
            sequence: None,
            shelf_count: 0,
            tallies_revision: None,
            press_collapse: None,
            reveal: None,
            reveal_asked: 0,
            links: crate::links::LinkState::default(),
            stored_session: None,
            sidebar_scroll: gpui::ScrollHandle::new(),
            started,
            first_frame_logged: false,
            _subscriptions: subs,
        };
        // The tab left stays drawn until the tab shown is ready (`tabs.rs`).
        shell.app.hold_tabs(true);
        shell.sync_tabs(window, cx);
        // Without a location to show, the last window's tabs came back
        // (`App::restore_tabs`), their splits with them.
        let initial_given = initial.is_some();
        if let Some(loc) = initial
            && let Err(e) = shell.app.navigate(shell.pane, &loc)
        {
            shell.set_status(format!("{e:#}"), true, cx);
        }
        // Tabs restored from the last window open the one shown, now that
        // the listing has a UI to wake.
        if let Err(e) = shell.app.resume_tabs() {
            shell.set_status(format!("{e:#}"), true, cx);
        }
        shell.restore_session(initial_given, window, cx);
        shell
    }

    /// Put the window back the way it was left: the same view, the same
    /// split (unless `split` is false: tabs that came back bring their own),
    /// the same panels. The view waits for `sync_views`, which is what
    /// decides which views this build and its plugins actually have.
    fn restore_session(&mut self, split: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = crate::session::restore(&self.app) else {
            self.stored_session = Some(self.session());
            return;
        };
        if session.zoom > 0 {
            self.zoom = session.zoom;
        }
        if session.row_zoom > 0 {
            self.row_zoom = session.row_zoom;
        }
        self.sidebar_hidden = session.sidebar_hidden;
        self.sidebar_sections = session.sidebar_sections;
        if session.sidebar_width > 0. {
            self.sidebar_width = session.sidebar_width;
        }
        if session.split_share > 0. && session.split_share < 1. {
            self.split_share = session.split_share;
        }
        self.preview.open = session.preview;
        self.preview.details_open = session.details;
        if session.preview_width > 0. {
            self.preview.width = session.preview_width;
        }
        if session.preview_height > 0. {
            self.preview.height = Some(session.preview_height);
        }
        self.layout.panes = session.panes;
        self.layout.preview = session.preview_at;
        self.layout.member = session.member_at;
        let dragged = |share: f32| (share > 0. && share < 1.).then_some(share);
        if let Some(share) = dragged(session.members_height_share) {
            self.preview.members_share.above = share;
        }
        if let Some(share) = dragged(session.members_width_share) {
            self.preview.members_share.beside = share;
        }
        self.layout.split = session.split_panes;
        if session.player_height > 0. {
            self.player.height = session.player_height;
        }
        self.timeline.open = session.timeline;
        // Its shell starts on a worker at the first sync.
        self.terminal.open = session.terminal && self.app.terminals.provider().is_some();
        if session.terminal_height > 0. {
            self.terminal.height = Some(session.terminal_height);
        }
        // Restored tabs bring their own splits. One made here starts even:
        // the remembered share is put back after it.
        if split && session.split && !self.is_split() {
            let (split, share) = (self.layout.split, self.split_share);
            self.toggle_split(window, cx);
            self.layout.split = split;
            self.split_share = share;
        }
        // The views this build and its plugins have were instantiated with
        // the panes; a remembered one that is gone leaves the List.
        for pane in &mut self.panes {
            if let Some(known) = pane.views.keys().find(|k| ***k == *session.view).copied() {
                pane.view_id = known;
            }
        }
        self.stored_session = Some(self.session());
    }

    pub(crate) fn make_pane_ui(
        &self,
        id: PaneId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> PaneUi {
        let address = cx.new(|cx| TextInput::new("Path, query:… or location", cx));
        let sub = cx.subscribe_in(
            &address,
            window,
            move |this, _, ev: &InputEvent, window, cx| match ev {
                // What the field does is the omnibox's (`omnibox.rs`).
                InputEvent::Submit => {
                    this.activate_pane(id, cx);
                    this.omnibox_submit(false, window, cx);
                }
                InputEvent::Cancel => {
                    this.activate_pane(id, cx);
                    this.omnibox_cancel(window, cx);
                }
                InputEvent::Changed => {
                    this.activate_pane(id, cx);
                    this.omnibox_changed(window, cx);
                }
            },
        );
        // Read the revision before the descriptors: a registration landing
        // between the two reads is then picked up by the next sync_views.
        let views_revision = self.app.registry.views.revision();
        let mut views = HashMap::new();
        let mut view_registrations = HashMap::new();
        view::reconcile(
            &mut views,
            &mut view_registrations,
            &self.app.registry.views.all(),
        );
        PaneUi {
            id,
            focus: cx.focus_handle(),
            address,
            address_shown: String::new(),
            list: ListState::default(),
            view_registrations,
            views_revision,
            views,
            view_id: view::LIST,
            listing_view: view::LIST,
            usage: UsageState::default(),
            tags: crate::tags_view::TagsLens::default(),
            loading: crate::loading::State::default(),
            last_search: String::new(),
            omni: None,
            weight: crate::weight::Weight::default(),
            narrow: crate::narrow::Narrow::default(),
            chips: crate::search_bar::Chips::default(),
            place: crate::trail::Shown::default(),
            placeholder_focus: cx.focus_handle(),
            returning: true,
            anchor: None,
            warm: Vec::new(),
            _sub: sub,
        }
    }

    // ------------------------------------------------------------ layout

    // ------------------------------------------------------------ panes

    /// The UI of the pane commands act on: one of the tab shown, or of the
    /// tab left while it is still drawn (its views lay out after `render`).
    pub fn ui(&self) -> &PaneUi {
        self.panes
            .iter()
            .chain(self.presented.iter().flatten())
            .find(|p| p.id == self.pane)
            .or_else(|| self.panes.first())
            .expect("at least one pane")
    }

    /// The pane whose row starts with the sidebar toggle and More: the
    /// first (left, or top), while the sidebar is not drawn; the sidebar's
    /// header has them otherwise (`address::corner`).
    pub fn corner_pane(&self) -> Option<PaneId> {
        (self.sidebar_drawn <= 0.)
            .then(|| self.panes.first().map(|p| p.id))
            .flatten()
    }

    /// Where Split and Preview are this frame, the sidebar's toggle and
    /// More in mirror: the preview's own bar while it is open at the
    /// right, else the right end of the tab strip while it shows, else the
    /// end of the row of the pane at the window's right (the top one when
    /// the split is stacked).
    pub fn toggles_at(&self) -> TogglesAt {
        if self.preview.open && !self.layout.bottom() {
            return TogglesAt::Preview;
        }
        if self.strip_shown() {
            return TogglesAt::Strip;
        }
        let pane = if self.layout.stacked() {
            self.panes.first()
        } else {
            self.panes.last()
        };
        pane.map_or(TogglesAt::Strip, |p| TogglesAt::Pane(p.id))
    }

    /// Whether the tab strip shows: two tabs or more, or one asked for
    /// (`ui.single_tab`).
    pub fn strip_shown(&self) -> bool {
        self.prefs.single_tab || self.app.workspace.read().tabs().len() > 1
    }

    /// The UI of pane `id`, wherever it is: drawn, held, or dormant.
    pub fn pane_ui_mut(&mut self, id: PaneId) -> Option<&mut PaneUi> {
        self.panes
            .iter_mut()
            .chain(self.presented.iter_mut().flatten())
            .find(|p| p.id == id)
            .or_else(|| self.hidden.get_mut(&id))
    }

    pub fn ui_mut(&mut self) -> &mut PaneUi {
        let pane = self.pane;
        if !self.panes.iter().any(|p| p.id == pane)
            && let Some(ui) = self.presented.iter_mut().flatten().find(|p| p.id == pane)
        {
            return ui;
        }
        let ix = self.panes.iter().position(|p| p.id == pane).unwrap_or(0);
        &mut self.panes[ix]
    }

    /// Run `f` with `pane` as the pane commands act on (processors run at
    /// layout time, after `render_pane` restored the active pane).
    pub fn with_pane<R>(&mut self, pane: PaneId, f: impl FnOnce(&mut Self) -> R) -> R {
        let saved = self.pane;
        self.pane = pane;
        let r = f(self);
        self.pane = saved;
        r
    }

    pub fn is_split(&self) -> bool {
        self.panes.len() > 1
    }

    /// The other pane when split.
    pub fn other_pane(&self) -> Option<PaneId> {
        self.panes.iter().map(|p| p.id).find(|id| *id != self.pane)
    }

    /// Run `work` on a scheduler worker (it may block: a stat, a plugin
    /// reading files) and hand its result to `then` on the UI thread. The
    /// one way the shell waits for something without blocking a frame;
    /// `then` decides whether the answer is still wanted.
    pub fn off_thread<T: Send + 'static>(
        &self,
        label: &'static str,
        work: impl FnOnce() -> T + Send + 'static,
        then: impl FnOnce(&mut Shell, T, &mut Window, &mut Context<Shell>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (tx, rx) = flume::bounded(1);
        self.app.scheduler.spawn(
            bibli_core::scheduler::Priority::Visible,
            label,
            self.app.lifetime().child(),
            move || {
                let _ = tx.send(work());
            },
        );
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(result) = rx.recv_async().await {
                this.update_in(cx, |shell, window, cx| then(shell, result, window, cx))
                    .ok();
            }
        })
        .detach();
    }

    /// Show `path` in `pane`: navigate to the folder that holds it and select
    /// the file itself once the listing reaches it. A directory is simply
    /// opened. Whether it is a directory, or there at all, is asked on a
    /// worker (the disk may be asleep); a path that is gone is reported.
    /// Returns false only for a path that cannot name a file (no parent).
    pub fn reveal_path(
        &mut self,
        pane: PaneId,
        path: &std::path::Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.reveal_path_at(pane, path, crate::links::RevealAt::default(), window, cx)
    }

    /// `reveal_path`, in a tab of its own when `at` says so (the pane it
    /// opens from gives it its sort), and with the line its preview shows.
    pub fn reveal_path_at(
        &mut self,
        pane: PaneId,
        path: &std::path::Path,
        at: crate::links::RevealAt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if path.parent().is_none() && !path.has_root() {
            return false;
        }
        self.reveal_asked += 1;
        let asked = self.reveal_asked;
        let target = path.to_path_buf();
        self.off_thread(
            "reveal-stat",
            move || {
                let is_dir = std::fs::metadata(&target).map(|m| m.is_dir());
                (target, is_dir)
            },
            move |this, (path, is_dir), window, cx| {
                if this.reveal_asked != asked {
                    return;
                }
                let found = match is_dir {
                    Ok(true) => Some((path.clone(), None)),
                    Ok(false) => path
                        .parent()
                        .zip(path.file_name())
                        .map(|(parent, name)| (parent.to_path_buf(), Some(name.to_owned()))),
                    Err(_) => None,
                };
                let Some((folder, name)) = found else {
                    let name = path.file_name().map_or_else(
                        || path.display().to_string(),
                        |name| name.to_string_lossy().into_owned(),
                    );
                    this.set_status(format!("“{name}” is not there any more"), true, cx);
                    return;
                };
                let pane = if at.tab {
                    let location = bibli_core::Location::from_path(&folder);
                    match this.app.new_tab(Some(pane), Some(location), true) {
                        Ok((_, opened)) => opened,
                        Err(error) => {
                            this.set_status(format!("{error:#}"), true, cx);
                            return;
                        }
                    }
                } else {
                    this.activate_pane(pane, cx);
                    this.run(
                        "navigate",
                        serde_json::json!({ "location": PathArg::from(folder), "pane": pane }),
                        window,
                        cx,
                    );
                    pane
                };
                if let (Some(line), true) = (at.line, name.is_some()) {
                    this.wait_for_line(pane, path.clone(), line, cx);
                }
                this.reveal = name.map(|name| Reveal {
                    pane,
                    name,
                    until: Instant::now() + std::time::Duration::from_secs(10),
                });
                cx.notify();
            },
            window,
            cx,
        );
        true
    }

    /// Select what `reveal_path` is waiting for, once the listing has it.
    fn take_reveal(&mut self, cx: &mut Context<Self>) {
        let Some(reveal) = self.reveal.as_ref() else {
            return;
        };
        let found = self
            .app
            .workspace
            .read()
            .pane(reveal.pane)
            .and_then(|pane| {
                pane.listing
                    .find_by_name(&reveal.name)
                    .map(|entry| entry.id)
            });
        let Some(id) = found else {
            if reveal.until < Instant::now() {
                self.reveal = None;
            }
            return;
        };
        let pane = reveal.pane;
        self.reveal = None;
        {
            let mut ws = self.app.workspace.write();
            if let Some(pane) = ws.pane_mut(pane) {
                pane.selection.set([id]);
            }
        }
        self.with_pane(pane, |shell| {
            if let Some(index) = shell.cursor_index() {
                shell.with_view(|view, _| view.scroll_to(index));
            }
        });
        cx.notify();
    }

    /// Make `id` the active pane (workspace and shell agree).
    pub fn activate_pane(&mut self, id: PaneId, cx: &mut Context<Self>) {
        if self.app.workspace.read().active_id() == id && self.pane == id {
            return;
        }
        self.app.workspace.write().set_active(id);
        self.pane = id;
        cx.notify();
    }

    /// The mouse's Back or Forward button: `command` in the pane under the
    /// pointer, which becomes the active one, or in the active pane when the
    /// pointer is elsewhere in the window (the sidebar, the preview).
    fn navigate_history(
        &mut self,
        pane: Option<PaneId>,
        command: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        // A dialog or a menu is answered first: nothing moves behind it.
        if self.modal_open() || !self.reviews.is_empty() {
            return;
        }
        let pane = pane.unwrap_or_else(|| self.app.workspace.read().active_id());
        if self.app.workspace.read().pane(pane).is_none() {
            return;
        }
        self.activate_pane(pane, cx);
        self.run(command, serde_json::json!({ "pane": pane }), window, cx);
    }

    /// Split the tab shown, or close its split, arranged as `layout` says.
    /// The workspace changes; `sync_tabs` makes and releases the panes' UI.
    pub(crate) fn toggle_split(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_split() {
            // An arrangement made for this split ends with it.
            self.layout.split = None;
            // Close the other pane; keep the active one.
            if let Some(other) = self.other_pane() {
                self.app.workspace.write().close_pane(other);
            }
            self.sync_tabs(window, cx);
            window.focus(&self.ui().focus);
            self.remember_session();
            cx.notify();
            return;
        }
        let here = {
            let ws = self.app.workspace.read();
            ws.pane(self.pane).and_then(|p| p.location().cloned())
        };
        // A split shows the same folder, in the same view: showing it in
        // another view as well would be a second surprise.
        let id = self.app.workspace.write().add_pane();
        self.sync_tabs(window, cx);
        if let Some(loc) = here
            && let Err(e) = self.app.navigate(id, &loc)
        {
            self.set_status(format!("{e:#}"), true, cx);
        }
        self.remember_session();
        cx.notify();
    }

    fn focus_other_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(o) = self.other_pane() {
            self.activate_pane(o, cx);
            window.focus(&self.ui().focus);
        }
    }

    /// Directory shown by the other pane, for Shift+F5/F6 and the shelf buttons.
    pub fn other_pane_dir(&self) -> Option<std::path::PathBuf> {
        let other = self.other_pane()?;
        let ws = self.app.workspace.read();
        ws.pane(other)
            .and_then(|p| p.location().and_then(Location::path))
    }

    fn send_to_other_pane(&mut self, cmd: &str, window: &mut Window, cx: &mut Context<Self>) {
        match self.other_pane_dir() {
            Some(dest) => self.run(
                cmd,
                serde_json::json!({ "dest": PathArg::from(dest), "pane": self.pane }),
                window,
                cx,
            ),
            None => self.set_status("the other pane is not showing a directory", true, cx),
        }
    }

    // ------------------------------------------------------------ views

    pub fn take_view(&mut self) -> Option<Box<dyn PaneView>> {
        let id = self.ui().view_id;
        self.ui_mut().views.remove(id)
    }

    pub fn put_view(&mut self, v: Box<dyn PaneView>) {
        self.ui_mut().views.insert(v.id(), v);
    }

    pub fn with_view<R>(&mut self, f: impl FnOnce(&mut dyn PaneView, &mut Self) -> R) -> Option<R> {
        let mut v = self.take_view()?;
        let r = f(v.as_mut(), self);
        self.put_view(v);
        Some(r)
    }

    /// The registry controls availability; panes retain only live renderers.
    pub fn available_views(&self) -> Vec<bibli_core::registry::ViewSpec> {
        let mut specs: Vec<_> = self
            .app
            .registry
            .views
            .all()
            .into_iter()
            .filter(|s| self.ui().views.contains_key(&*s.id))
            .collect();
        specs.sort_by_key(|s| s.order);
        specs
    }

    /// The keys bound to `command` with `args`, as people read them.
    pub fn keys_for(&self, command: &str, args: &serde_json::Value) -> Option<String> {
        keymap::keys_for(&self.key_hints, command, args)
    }

    /// Install the key bindings again when a plugin added or withdrew some.
    fn sync_keymap(&mut self, cx: &mut Context<Self>) {
        let revisions = keymap::Stamp::revisions(&self.app);
        let settings = self.app.settings.revision();
        if revisions == self.keys.stamp.revisions && settings == self.keys.settings {
            return;
        }
        self.keys.settings = settings;
        let user = bibli_core::keymap::user_bindings(&self.app.settings);
        if revisions == self.keys.stamp.revisions && user == self.keys.stamp.user {
            return;
        }
        let keymap = keymap::install(&self.app, user, cx);
        self.key_hints = keymap.hints;
        self.keys.stamp = keymap.stamp;
        self.keys.typeahead = keymap.typeahead;
        if let Some(settings) = &self.settings {
            let hints = self.key_hints.clone();
            settings.update(cx, |settings, cx| settings.set_key_hints(hints, cx));
        }
        cx.notify();
    }

    fn sync_views(&mut self) {
        // One atomic per frame: reconcile only when the registry changed since
        // a pane last looked, so descriptors are cloned per change, not per frame.
        let revision = self.app.registry.views.revision();
        if self
            .panes
            .iter()
            .all(|pane| pane.views_revision == revision)
        {
            return;
        }
        let specs = self.app.registry.views.all();
        for pane in &mut self.panes {
            if pane.views_revision == revision {
                continue;
            }
            pane.views_revision = revision;
            let replaced = view::reconcile(&mut pane.views, &mut pane.view_registrations, &specs);
            if replaced.iter().any(|id| id == "usage") {
                pane.usage = UsageState::default();
            }
            if !pane.views.contains_key(pane.view_id) {
                pane.view_id = view::LIST;
            }
        }
    }

    pub(crate) fn set_view(&mut self, id: &str, cx: &mut Context<Self>) {
        self.sync_views();
        let known = self.ui().views.keys().find(|k| **k == id).copied();
        match known {
            Some(k) => {
                let leaving = self.ui().view_id;
                let listing = self
                    .app
                    .registry
                    .views
                    .find(|s| &*s.id == leaving)
                    .is_none_or(|s| s.role == bibli_core::registry::ViewRole::Listing);
                if listing {
                    self.ui_mut().listing_view = leaving;
                }
                self.ui_mut().view_id = k;
                self.remember_session();
                let ix = self.cursor_index();
                self.with_view(|v, _| {
                    if let Some(ix) = ix {
                        v.scroll_to(ix);
                    }
                });
                cx.notify();
            }
            None => self.set_status(format!("unknown view `{id}`"), true, cx),
        }
    }

    /// `ui.grid_fill`: a preference, so it goes through `settings.apply` like
    /// the switch in the dialog.
    fn toggle_grid_fill(&mut self, cx: &mut Context<Self>) {
        self.prefs.grid_fill = !self.prefs.grid_fill;
        cx.notify();
        if let Err(error) = self.app.dispatch(&Invocation::with_args(
            "settings.apply",
            &serde_json::json!({
                "changes": { crate::settings::GRID_FILL_KEY: self.prefs.grid_fill.to_string() }
            }),
        )) {
            self.set_status(format!("{error:#}"), true, cx);
        }
    }

    /// `ui.printed_pages`: count a book by chapters or by printed pages. A
    /// preference, so it goes through `settings.apply` like the dialog's
    /// switch; the unit picker closes on its choice.
    fn set_printed_pages(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(popup) = self.preview.menu.take_if(|popup| popup.kind == "unit") {
            popup.restore(window);
        }
        self.prefs.printed_pages = on;
        cx.notify();
        if let Err(error) = self.app.dispatch(&Invocation::with_args(
            "settings.apply",
            &serde_json::json!({
                "changes": { crate::settings::PRINTED_PAGES_KEY: on.to_string() }
            }),
        )) {
            self.set_status(format!("{error:#}"), true, cx);
        }
    }

    /// `ui.full_width`: the reading column takes the whole panel, or goes
    /// back to the width it had. The Reading width preference, so it goes
    /// through `settings.apply` like the choice in the dialog.
    fn toggle_full_width(&mut self, cx: &mut Context<Self>) {
        let current = self
            .app
            .settings
            .get::<String>(crate::settings::READING_WIDTH_KEY)
            .unwrap_or_else(|| "comfortable".into());
        let next = if current == "full" {
            self.reading_width_before
                .take()
                .unwrap_or_else(|| "comfortable".into())
        } else {
            self.reading_width_before = Some(current);
            "full".into()
        };
        self.reading_measure = crate::markdown::measure(&next);
        cx.notify();
        if let Err(error) = self.app.dispatch(&Invocation::with_args(
            "settings.apply",
            &serde_json::json!({
                "changes": { crate::settings::READING_WIDTH_KEY: next }
            }),
        )) {
            self.set_status(format!("{error:#}"), true, cx);
        }
    }

    /// Ctrl and the wheel over a view that zooms. The view stays where it is
    /// (it keeps its first visible item); only the keys follow the cursor.
    pub fn zoom_wheel(&mut self, delta: f32, lines: bool, cx: &mut Context<Self>) {
        match self.zoom_wheel.turn(delta, lines, false) {
            0 => {}
            turn => self.zoom_to(if turn < 0 { "in" } else { "out" }, cx),
        }
    }

    /// Zoom what the pane shows: the grid's tiles, or the rows of the list
    /// and the columns, each keeping its own size. Other views have none.
    fn zoom_to(&mut self, dir: &str, cx: &mut Context<Self>) {
        fn step(levels: &[u32], current: u32, default: u32, dir: &str) -> u32 {
            let at = |zoom| levels.iter().position(|z| *z == zoom);
            let pos = at(current).or_else(|| at(default)).unwrap_or(0);
            match dir {
                "in" => levels[(pos + 1).min(levels.len() - 1)],
                "out" => levels[pos.saturating_sub(1)],
                _ => default,
            }
        }
        use crate::views::list::{DEFAULT_ROW_ZOOM, ROW_ZOOMS};
        match self.ui().view_id {
            view::GRID => self.zoom = step(&ZOOMS, self.zoom, DEFAULT_ZOOM, dir),
            view::LIST | view::TREE | view::COLUMNS => {
                self.row_zoom = step(&ROW_ZOOMS, self.row_zoom, DEFAULT_ROW_ZOOM, dir);
            }
            _ => return,
        }
        self.remember_session();
        cx.notify();
    }

    fn zoom_step(&mut self, dir: &str, cx: &mut Context<Self>) {
        if self.with_view(|v, _| v.zoom(dir)).unwrap_or(false) {
            cx.notify();
            return;
        }
        self.zoom_to(dir, cx);
        let ix = self.cursor_index();
        self.with_view(|v, _| {
            if let Some(ix) = ix {
                v.scroll_to(ix);
            }
        });
    }

    fn on_run_command(&mut self, a: &RunCommand, window: &mut Window, cx: &mut Context<Self>) {
        self.run(&a.name, a.args.clone(), window, cx);
    }

    /// Route a command: `ui.*` to the shell, everything else to the core.
    pub fn run(
        &mut self,
        name: &str,
        args: serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.settings.is_some() && !name.starts_with("ui.settings") {
            return;
        }
        // The overview is something to read: a key meant for the window
        // behind it closes it instead of acting unseen.
        if self.shortcuts.is_some() && !matches!(name, "ui.shortcuts" | "ui.settings") {
            self.toggle_shortcuts(window, cx);
            return;
        }
        // Settings may contain private paths; never trace their values.
        if !name.starts_with("ui.settings") && !name.starts_with("settings.") {
            tracing::debug!(name, %args, "run");
        }
        // What the user does acts on the tab shown: draw it first. Moving
        // between tabs keeps holding, so cycling through them draws only the
        // one it stops on.
        if self.presented.is_some() && !name.starts_with("tab.") {
            self.present(false, window, cx);
        }
        // Ctrl+D on a tag's search pins the tag, as the button does.
        if name == "keep"
            && self.app.commands.get("tags.pin").is_some()
            && let Some(shown) = self.app.pane_location(self.pane)
            && let Some(tag) = self.ui().chips.only_tag(&shown.to_string())
        {
            let tag = tag.to_owned();
            let pinned = self.app.registry.places.find(|p| {
                p.color.is_some()
                    && p.forget.is_some()
                    && bibli_core::metadata::same_tag(&p.title, &tag)
            });
            if pinned.is_some() {
                self.set_status(format!("The tag “{tag}” is already pinned"), false, cx);
            } else {
                self.run("tags.pin", serde_json::json!({ "tag": tag }), window, cx);
            }
            return;
        }
        // A lens acts on what it shows, not on the files it hides.
        if self
            .with_view(|view, shell| view.command(name, &args, shell, window, cx))
            .unwrap_or(false)
        {
            cx.notify();
            return;
        }
        if let Some(ui) = name.strip_prefix("ui.") {
            self.run_ui(ui, &args, window, cx);
            cx.notify();
            return;
        }
        if name == "open" && self.app.commands.get("open").is_none() {
            self.open_fallback(window, cx);
            return;
        }
        if name == "refresh" {
            if self.ui().view_id == "usage" {
                self.refresh_usage(cx);
            }
            if self.ui().view_id == crate::tags_view::ID {
                self.tags_refresh(cx);
            }
            self.reader_retry_pages();
        }
        let inv = Invocation {
            name: name.into(),
            args,
        };
        match self.app.dispatch(&inv) {
            Ok(Outcome::Done) => {}
            Ok(Outcome::Value(v)) => self.on_value(name, v, cx),
            Ok(Outcome::Plan(plan)) => self.apply_user_plan(plan, cx),
            Err(e) => self.report_error(&e, cx),
        }
        cx.notify();
    }

    /// A user-initiated plan applies immediately (it is undoable), as a
    /// whole: the executor checks every step first, on its lane, and a
    /// conflict applies nothing and comes back as the toast's error.
    pub fn apply_user_plan(&mut self, plan: Plan, cx: &mut Context<Self>) {
        self.app.apply_whole(plan);
        cx.notify();
    }

    /// Values from commands the shell knows how to present; others are ignored.
    fn on_value(&mut self, name: &str, v: serde_json::Value, cx: &mut Context<Self>) {
        if name == "shell.copy_paths"
            && let Some(s) = v.as_str()
        {
            let n = self.selection_ctx().count;
            cx.write_to_clipboard(ClipboardItem::new_string(s.to_owned()));
            self.set_status(
                format!("Copied {n} path{}", if n == 1 { "" } else { "s" }),
                false,
                cx,
            );
        }
    }

    /// Dispatch and hand back a `Value` (for plugin queries the UI renders).
    pub fn query_value(
        &mut self,
        name: &str,
        args: serde_json::Value,
        cx: &mut Context<Self>,
    ) -> Option<serde_json::Value> {
        match self.app.dispatch(&Invocation {
            name: name.into(),
            args,
        }) {
            Ok(Outcome::Value(v)) => Some(v),
            Ok(_) => None,
            Err(e) => {
                self.set_status(format!("{e:#}"), true, cx);
                None
            }
        }
    }

    fn run_ui(
        &mut self,
        name: &str,
        args: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let arg = |k: &str| args.get(k).and_then(|v| v.as_str()).map(str::to_owned);
        match name {
            "open" => self.open_selection(window, cx),
            "focus_address" => self.omnibox_begin(false, None, window, cx),
            "focus_pane" => window.focus(&self.ui().focus),
            "settings" => self.open_settings(args, window, cx),
            "toolbar_next" => window.focus_next(),
            "toolbar_prev" => window.focus_prev(),
            "toolbar_menu" => self.toggle_toolbar_menu(args, window, cx),
            "history_menu" => self.toggle_history_menu(args, window, cx),
            "subfolder_menu" => self.toggle_subfolder_menu(args, window, cx),
            "search_chip" => self.toggle_search_chip(args, window, cx),
            name if name.starts_with("settings_") => self.settings_command(name, args, window, cx),
            "shortcuts" => self.toggle_shortcuts(window, cx),
            "ask" => self.open_ask(args, window, cx),
            "choose" => self.open_choose(args, window, cx),
            "choose_up" => self.choose_move(-1, cx),
            "choose_down" => self.choose_move(1, cx),
            "choose_option" => self.choose_toggle_option(window, cx),
            "unlock_prompt" => {
                if let Some(device) = arg("device") {
                    let title = arg("title").unwrap_or_else(|| "this volume".into());
                    let tab = args.get("tab").and_then(serde_json::Value::as_bool) == Some(true);
                    self.unlock = Some(crate::prompts::Unlock { device, tab });
                    self.open_prompt(
                        crate::prompts::PromptKind::Passphrase,
                        &format!("Unlock “{title}”"),
                        "",
                        None,
                        window,
                        cx,
                    );
                }
            }
            name if name.starts_with("menu_") => self.menu_command(name, window, cx),
            "palette" => self.open_palette(window, cx),
            "palette_up" => self.palette_move(-1, cx),
            "palette_down" => self.palette_move(1, cx),
            "rename_prompt" => {
                let (single, place) = {
                    let ws = self.app.workspace.read();
                    ws.pane(self.pane)
                        .and_then(|p| {
                            let mut it = p.selection.iter();
                            let id = it.next()?;
                            if it.next().is_some() {
                                return None;
                            }
                            let name = p.entry(id).map(|e| e.name_str().into_owned());
                            Some((name, p.location_of(id)))
                        })
                        .unwrap_or_default()
                };
                // A place (a tag's) is renamed as its location says.
                if let Some(place) = place {
                    match self.app.verbs(&place).rename {
                        Some(rename) => {
                            self.run(&rename.command.name, rename.command.args, window, cx);
                        }
                        None => self.set_status("This cannot be renamed", true, cx),
                    }
                    return;
                }
                match single {
                    Some(name) => {
                        let stem = name.rfind('.').filter(|&i| i > 0).unwrap_or(name.len());
                        self.open_prompt(
                            PromptKind::Rename,
                            "Rename",
                            &name,
                            Some(0..stem),
                            window,
                            cx,
                        );
                    }
                    None => self.set_status("select exactly one entry to rename", true, cx),
                }
            }
            "new_file_prompt" => {
                self.open_prompt(PromptKind::NewFile, "New file", "", None, window, cx);
            }
            "find" => {
                let backward = args
                    .get("backward")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false);
                let origin = self.cursor_id();
                let title = if backward { "Find before" } else { "Find" };
                self.open_prompt(
                    PromptKind::Find { origin, backward },
                    title,
                    "",
                    None,
                    window,
                    cx,
                );
            }
            "find_next" => {
                let delta = args
                    .get("delta")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(1);
                match self.last_find.clone() {
                    Some((pattern, _)) => match self.find_from(None, &pattern, delta < 0) {
                        Some(ix) => self.cursor_to(ix as isize, Step::Follow, window, cx),
                        None => self.set_status(format!("No name holds “{pattern}”"), false, cx),
                    },
                    None => self.set_status("Nothing to find again: press / first", false, cx),
                }
            }
            "cursor" => self.cursor_command(args, window, cx),
            "cursor_pick" => {
                let advance = args
                    .get("advance")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(true);
                self.pick_cursor(advance, window, cx);
            }
            "mkdir_prompt" => {
                self.open_prompt(PromptKind::Mkdir, "New folder", "", None, window, cx);
            }
            // A new place where the location shown makes them (a tag in
            // the one shown), as it says (`Verbs::create`).
            "create_here" => match self.ui().place.verbs.create.clone() {
                Some(create) => self.run(&create.command.name, create.command.args, window, cx),
                None => self.set_status("Nothing new can be made here", true, cx),
            },
            // The address field, in search: where it looks as a pill, and a
            // text to start from (`content:`) typed after.
            "search" => self.omnibox_begin(true, arg("text"), window, cx),
            "omnibox_move" => {
                let delta = args
                    .get("delta")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(1);
                self.omnibox_move(isize::try_from(delta).unwrap_or(1), cx);
            }
            "omnibox_submit" => {
                let everywhere =
                    args.get("everywhere").and_then(serde_json::Value::as_bool) == Some(true);
                self.omnibox_submit(everywhere, window, cx);
            }
            "tags_prompt" => {
                let entries = args
                    .get("entries")
                    .and_then(|e| serde_json::from_value(e.clone()).ok());
                let text = arg("text");
                self.open_tag_editor(entries, text, window, cx);
            }
            // A value to copy, from a link beside it (Details).
            "copy_text" => {
                if let Some(text) = arg("text") {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                    let what = arg("what").unwrap_or_else(|| "text".into());
                    self.set_status(format!("Copied {what}"), false, cx);
                }
            }
            "tag_editor_up" => self.tag_editor_move(-1, cx),
            "tag_editor_down" => self.tag_editor_move(1, cx),
            "tag_editor_apply" => self.tag_editor_apply(window, cx),
            // An address followed (a document's link, one found in text,
            // where a file was downloaded from), opened by the desktop when
            // the user asks: bibli fetches nothing (ADR-020).
            "open_url" => self.open_address(&arg("url").unwrap_or_default(), cx),
            // A file shown in its folder, from a link.
            "reveal" => self.reveal_command(args, window, cx),
            "jump" => self.open_jump(window, cx),
            "jump_up" => self.jump_move(-1, cx),
            "jump_down" => self.jump_move(1, cx),
            "rename_bulk_prompt" => self.open_bulk(window, cx),
            "rename_bulk_next" => self.bulk_focus_next(1, window, cx),
            "rename_bulk_prev" => self.bulk_focus_next(-1, window, cx),
            "rename_bulk_case" => self.bulk_cycle_case(window, cx),
            "review_apply" => self.review_apply(cx),
            "review_cancel" => self.review_cancel(cx),
            "usage" => self.set_view("usage", cx),
            "usage_close" => self.set_view(view::LIST, cx),
            "usage_refresh" => self.refresh_usage(cx),
            "tags_refresh" => self.tags_refresh(cx),
            "tags_sort" => {
                let lens = &mut self.ui_mut().tags;
                lens.by_files = !lens.by_files;
            }
            "tags_open" => {
                if let Some(tag) = arg("tag") {
                    let tab = args.get("tab").and_then(serde_json::Value::as_bool) == Some(true);
                    self.tags_open(&tag, tab, window, cx);
                }
            }
            "tags_color" => self.tags_color(args, window, cx),
            "timeline" => self.toggle_timeline(cx),
            "terminal" => self.toggle_terminal(window, cx),
            "terminal_copy" => self.terminal_copy(cx),
            "terminal_paste" => self.terminal_paste(cx),
            "terminal_page" => self.terminal_page(args),
            "terminal_links" => self.label_terminal_links(cx),
            "timeline_bucket" => {
                let b = arg("bucket").unwrap_or_else(|| "month".into());
                self.timeline_set_bucket(&b, cx);
            }
            "timeline_subtree" => self.timeline_toggle_subtree(cx),
            "split" => self.split_command(args, window, cx),
            "layout" => self.layout_command(args, cx),
            "focus_other_pane" => self.focus_other_pane(window, cx),
            "copy_other" => self.send_to_other_pane("copy", window, cx),
            "move_other" => self.send_to_other_pane("move", window, cx),
            "shelf_open" => self.run(
                "navigate",
                serde_json::json!({ "location": "shelf:" }),
                window,
                cx,
            ),
            "shelf_move_here" | "shelf_copy_here" => {
                let cmd = if name == "shelf_move_here" {
                    "shelf.move_here"
                } else {
                    "shelf.copy_here"
                };
                match self.other_pane() {
                    Some(other) => self.run(cmd, serde_json::json!({ "pane": other }), window, cx),
                    None => {
                        self.set_status("split the window to choose a destination pane", true, cx);
                    }
                }
            }
            "confirm_delete" => {
                // The plan is built now, from what is selected now, so Enter
                // deletes what the question named even if the selection or
                // the active pane changes before it is answered.
                let pane = self.pane;
                match self.app.dispatch(&Invocation::with_args(
                    "delete",
                    &serde_json::json!({ "pane": pane }),
                )) {
                    Ok(Outcome::Plan(plan)) if !plan.is_empty() => {
                        let n = plan.len();
                        let noun = if n == 1 { "item" } else { "items" };
                        self.open_confirm(
                            format!("Delete {n} {noun} permanently? This cannot be undone."),
                            plan,
                            window,
                            cx,
                        );
                    }
                    Ok(_) => self.set_status(NOTHING_SELECTED, false, cx),
                    Err(error) => self.set_status(format!("{error:#}"), true, cx),
                }
            }
            "accept" => self.confirm_accept(window, cx),
            "dismiss" => self.confirm_dismiss(window, cx),
            "toggle_hidden" => self.toggle_hidden(cx),
            "toggle_sidebar" => self.toggle_sidebar(window, cx),
            "sidebar_section" => self.sidebar_section(args, cx),
            "context_menu" => self.open_context_menu_here(window, cx),
            "open_in_tab" => self.open_selection_in_tabs(window, cx),
            "sort" => {
                let ascending = args.get("ascending").and_then(serde_json::Value::as_bool);
                let key = match arg("key").as_deref() {
                    None if ascending.is_some() => None,
                    Some("size") => Some(SortKey::Size),
                    Some("modified") => Some(SortKey::Modified),
                    Some("rating") => Some(SortKey::Rating),
                    _ => Some(SortKey::Name),
                };
                self.sort(key, ascending, cx);
            }
            "view" => {
                // With a location, shown there (the sidebar's All Tags).
                if let Some(location) = args.get("location").filter(|l| !l.is_null()) {
                    self.run(
                        "navigate",
                        serde_json::json!({ "location": location }),
                        window,
                        cx,
                    );
                }
                let id = arg("id").unwrap_or_else(|| view::LIST.into());
                self.set_view(&id, cx);
            }
            "columns" => {
                if let Some(key) = arg("key") {
                    self.toggle_column(&key, cx);
                }
            }
            "columns_reset" => self.reset_column_widths(cx),
            "grid_fill" => self.toggle_grid_fill(cx),
            "full_width" => self.toggle_full_width(cx),
            "printed_pages" => {
                let on = args
                    .get("on")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(!self.prefs.printed_pages);
                self.set_printed_pages(on, window, cx);
            }
            "zoom" => {
                let dir = arg("dir").unwrap_or_else(|| "reset".into());
                self.zoom_step(&dir, cx);
            }
            "inspect" => self.inspect_command(args, window, cx),
            "media_control" => self.media_control(args, cx),
            "media_pin" => self.media_pin(cx),
            "media_keep_playing" => {
                let on = args.get("on").and_then(serde_json::Value::as_bool);
                self.set_keep_playing(on.unwrap_or(!self.prefs.player.keep_playing), cx);
            }
            "media_autoplay" => {
                let on = args.get("on").and_then(serde_json::Value::as_bool);
                self.set_autoplay(on.unwrap_or(!self.prefs.player.autoplay), cx);
            }
            "media_continue" => {
                let mode = args
                    .get("mode")
                    .and_then(serde_json::Value::as_str)
                    .map_or_else(
                        || self.prefs.player.continue_with.next(),
                        crate::player::Continue::parse,
                    );
                self.set_continue(mode, window, cx);
            }
            "player_mute" => self.player_mute(cx),
            "player_volume" => {
                let delta = args.get("delta").and_then(serde_json::Value::as_f64);
                self.player_volume_by(delta.unwrap_or(0.), window, cx);
            }
            "player" => {
                let request = args.get("request").cloned().unwrap_or_default();
                match serde_json::from_value(request) {
                    Ok(request) => self.player_request(request, window, cx),
                    Err(error) => self.set_status(error.to_string(), true, cx),
                }
            }
            "media_step" => {
                let delta = args
                    .get("delta")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(1);
                self.media_step(delta, window, cx);
            }
            "media_scrub" => self.media_scrub(args, cx),
            "media_fullscreen" => self.media_fullscreen(window, cx),
            "media_controls" => self.media_toggle_controls(window, cx),
            "media_escape" => {
                if self.preview.media.fullscreen.is_some() {
                    self.media_exit_fullscreen(window, cx);
                } else {
                    self.preview_close(window, cx);
                }
            }
            "preview_page" => self.preview_page(
                args.get("delta")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(1)
                    .clamp(-1, 1) as i32,
                window,
                cx,
            ),
            "preview_seek" => self.preview_seek(
                args.get("delta")
                    .and_then(serde_json::Value::as_f64)
                    .filter(|v| v.is_finite())
                    .unwrap_or(5.0),
                cx,
            ),
            "preview_zoom" => {
                self.preview_zoom(arg("dir").as_deref().unwrap_or("reset"), window, cx);
            }
            "map_pan" => {
                let step = |key| {
                    args.get(key)
                        .and_then(serde_json::Value::as_f64)
                        .filter(|v| v.is_finite())
                        .map_or(0., |v| v.clamp(-10., 10.))
                };
                self.map_pan(step("dx"), step("dy"), cx);
            }
            "map_open" => self.map_open_external(cx),
            "map_view_fit" => self.map_view_fit(cx),
            "map_view_spread" => self.map_view_spread(cx),
            "map_view_search_area" => self.map_view_search_area(window, cx),
            "toggle_preview" => self.toggle_preview(window, cx),
            "preview_close" => self.preview_close(window, cx),
            "reader_continuous" => self.reader_continuous(window, cx),
            "reader_pointer" => self.reader_pointer(
                args.get("phase")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("up"),
                args.get("page")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|p| u32::try_from(p).ok())
                    .unwrap_or(self.preview.page),
                args.get("x")
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or(0.) as f32,
                args.get("y")
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or(0.) as f32,
                window,
                cx,
            ),
            "reader_page" => self.preview_go_page(
                args.get("page")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|n| u32::try_from(n).ok())
                    .unwrap_or(1),
                cx,
            ),
            "reader_menu" | "preview_menu" => self.preview_menu(args, window, cx),
            "note" => self.note_command(args, window, cx),
            "reader_page_edit" => self.reader_page_edit(window, cx),
            "reader_page_submit" => self.reader_page_submit(window, cx),
            "reader_page_cancel" => self.reader_page_cancel(window, cx),
            "reader_open_external" | "preview_open_external" => self.preview_open_external(cx),
            "preview_copy_path" => self.preview_copy_path(cx),
            "preview_details" => self.preview_details(cx),
            "reader_copy" => self.reader_copy(cx),
            "reader_select_all" => self.reader_select_all(cx),
            "reader_outline" => self.reader_outline(window, cx),
            "reader_find" => self.reader_find(window, cx),
            "reader_find_close" => self.reader_find_close(window, cx),
            "reader_search" => self.reader_search(cx),
            "reader_match" => self.reader_match(
                args.get("delta")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(1)
                    .clamp(-1, 1) as i32,
                cx,
            ),
            "reader_link" => self.reader_link(
                args.get("index")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|n| usize::try_from(n).ok())
                    .unwrap_or(usize::MAX),
                cx,
            ),
            "peek" => self.toggle_peek(window, cx),
            "peek_close" => self.close_peek(window, cx),
            "peek_prev" => self.cursor_move(-1, Step::Select, window, cx),
            "peek_next" => self.cursor_move(1, Step::Select, window, cx),
            "cursor_up" => self.navigate_by(Nav::Up, Step::Select, window, cx),
            "cursor_down" => self.navigate_by(Nav::Down, Step::Select, window, cx),
            "cursor_left" => self.navigate_by(Nav::Left, Step::Select, window, cx),
            "cursor_right" => self.navigate_by(Nav::Right, Step::Select, window, cx),
            "cursor_extend_up" => self.navigate_by(Nav::Up, Step::Extend, window, cx),
            "cursor_extend_down" => self.navigate_by(Nav::Down, Step::Extend, window, cx),
            "cursor_extend_left" => self.navigate_by(Nav::Left, Step::Extend, window, cx),
            "cursor_extend_right" => self.navigate_by(Nav::Right, Step::Extend, window, cx),
            "cursor_home" => self.navigate_by(Nav::Home, Step::Select, window, cx),
            "cursor_end" => self.navigate_by(Nav::End, Step::Select, window, cx),
            "cursor_extend_home" => self.navigate_by(Nav::Home, Step::Extend, window, cx),
            "cursor_extend_end" => self.navigate_by(Nav::End, Step::Extend, window, cx),
            "cursor_move_up" => self.navigate_by(Nav::Up, Step::Cursor, window, cx),
            "cursor_move_down" => self.navigate_by(Nav::Down, Step::Cursor, window, cx),
            "cursor_move_left" => self.navigate_by(Nav::Left, Step::Cursor, window, cx),
            "cursor_move_right" => self.navigate_by(Nav::Right, Step::Cursor, window, cx),
            "cursor_move_home" => self.navigate_by(Nav::Home, Step::Cursor, window, cx),
            "cursor_move_end" => self.navigate_by(Nav::End, Step::Cursor, window, cx),
            "cursor_toggle" => self.toggle_cursor(window, cx),
            "page_up" => self.navigate_by(Nav::PageUp, Step::Select, window, cx),
            "page_down" => self.navigate_by(Nav::PageDown, Step::Select, window, cx),
            "quit" => {
                crate::remember_location(&self.app);
                let (decoded, skipped, bytes) = self.app.thumbnails.stats();
                tracing::info!(decoded, skipped, cached_bytes = bytes, "thumbnail stats");
                cx.quit();
            }
            other => self.set_status(format!("unknown ui command `ui.{other}`"), true, cx),
        }
    }

    /// `open` through the open-with plugin, or the built-in fallback.
    pub fn open_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.app.commands.get("open").is_some() {
            self.run("open", serde_json::json!({}), window, cx);
        } else {
            self.open_fallback(window, cx);
        }
    }

    /// Without an `open` command: enter a single selected directory, ignore files.
    fn open_fallback(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let target = {
            let ws = self.app.workspace.read();
            ws.pane(self.pane).and_then(|p| {
                let mut it = p.selection.iter();
                let id = it.next()?;
                if it.next().is_some() {
                    return None;
                }
                let e = p.entry(id)?;
                if !e.is_dir() {
                    return None;
                }
                // A place opens where it leads.
                match p.location_of(id) {
                    Some(place) => Some(serde_json::json!(place.to_string())),
                    None => p
                        .path_of(id)
                        .map(|path| serde_json::json!(PathArg::from(path))),
                }
            })
        };
        if let Some(location) = target {
            self.run(
                "navigate",
                serde_json::json!({ "location": location, "pane": self.pane }),
                window,
                cx,
            );
        }
    }

    /// Apply core events and refresh derived state. Called when the core
    /// wakes the shell (whether or not the window can draw) and first in
    /// `render`, for what the user did since; a second call finds nothing
    /// new to apply.
    pub(crate) fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let t0 = Instant::now();
        let report = self.app.pump_report();
        self.backlog = report.backlog;
        self.sync_views();
        self.sync_keymap(cx);
        self.app.thumbnails.tick();
        self.drain_executor(cx);
        for n in self.app.take_notifications() {
            let error = matches!(n.level, NotifyLevel::Error | NotifyLevel::Warning);
            match (n.setting, n.action) {
                (Some(key), _) if error => self.set_status_about(n.text, &key, cx),
                // Something done outside the journal, with its own way back.
                (_, Some(action)) => self.toast(
                    crate::toast::Toast {
                        text: n.text,
                        tone: if error {
                            crate::toast::Tone::Error
                        } else {
                            crate::toast::Tone::Success
                        },
                        action: Some(crate::toast::Action {
                            label: action.label,
                            command: action.command.name.to_string(),
                            args: action.command.args,
                        }),
                    },
                    cx,
                ),
                _ => self.set_status(n.text, error, cx),
            }
        }
        self.pending = self.app.pending.get();
        let revision = self.app.settings.revision();
        if revision != self.theme_revision {
            self.theme_revision = revision;
            cx.set_global(Theme::resolve(&self.app, window.appearance()));
            self.tag_colors = crate::tags::TagColors::read(&self.app);
            self.tag_fields = self
                .app
                .settings
                .get(bibli_core::metadata::TAG_EXCLUSIVE_SETTING)
                .unwrap_or_default();
            self.prefs.friendly_dates = self
                .app
                .settings
                .get::<String>(crate::settings::DATES_KEY)
                .is_none_or(|format| format != "iso");
            self.prefs.grid_fill = self
                .app
                .settings
                .get::<bool>(crate::settings::GRID_FILL_KEY)
                .unwrap_or(false);
            self.prefs.single_tab = self
                .app
                .settings
                .get::<bool>(crate::settings::SINGLE_TAB_KEY)
                .unwrap_or(false);
            self.reading_measure = crate::markdown::measure(
                &self
                    .app
                    .settings
                    .get::<String>(crate::settings::READING_WIDTH_KEY)
                    .unwrap_or_default(),
            );
            self.prefs.printed_pages = self
                .app
                .settings
                .get::<bool>(crate::settings::PRINTED_PAGES_KEY)
                .unwrap_or(false);
            self.read_player_prefs();
            self.columns = crate::settings::COLUMN_KEYS
                .map(|key| self.app.settings.get::<bool>(key).unwrap_or(false));
            if self.column_drag.is_none()
                && let Some(widths) = self
                    .app
                    .settings
                    .get::<Vec<f32>>(crate::settings::COLUMN_WIDTHS_KEY)
            {
                // Widths saved before a column was added leave it its own.
                let mut all = crate::views::list::DEFAULT_WIDTHS;
                for (width, saved) in all.iter_mut().zip(widths) {
                    *width = crate::views::list::clamp_width(saved);
                }
                self.column_widths = all;
            }
        }
        if self.type_names.absorb() | self.icons.absorb() {
            cx.notify();
        }
        if let Some(settings) = self.settings.clone() {
            settings.update(cx, |settings, cx| settings.sync(window, cx));
        }
        let proposals = self.app.take_proposals();
        if !proposals.is_empty() {
            self.add_reviews(proposals, window, cx);
        }
        self.sync_review_focus(window, cx);
        self.sync_map_tiles(window, cx);
        // Follow the workspace's tabs and active pane (commands change them).
        self.sync_tabs(window, cx);
        let saved = self.pane;
        let ids: Vec<PaneId> = self.panes.iter().map(|p| p.id).collect();
        let mut resorted_any = false;
        let mut ready = true;
        for id in ids {
            self.pane = id;
            let app = Arc::clone(&self.app);
            // A new listing, not a new address, is what starts the pane's
            // state over: reopening the folder already shown (revealing the
            // playing track in it) replaces the listing under the same text.
            // What the view shows beside the location (the columns of the
            // folders above) stays open; the rest, the folder just left
            // included when it is not among them, is let go.
            // Only a view that draws folders opened in place (the tree)
            // keeps them: in any other, their entries would be selected
            // and acted on unseen.
            let (wanted, close) = {
                let ws = app.workspace.read();
                let Some(pane) = ws.pane(id) else { continue };
                let view = self.ui().views.get(self.ui().view_id);
                let wanted = view
                    .map(|view| view.kept_locations(pane.location()))
                    .unwrap_or_default();
                let close =
                    !pane.expanded().is_empty() && !view.is_some_and(|view| view.opens_in_place());
                (
                    (!wanted.is_empty() || pane.keeps_any()).then_some(wanted),
                    close,
                )
            };
            if close {
                app.collapse_all(id);
            }
            if let Some(wanted) = wanted {
                app.keep_locations(id, &wanted);
            }
            let (location, resorted, replaced) = {
                let ws = app.workspace.read();
                let Some(pane) = ws.pane(id) else { continue };
                let location = pane.location().map(ToString::to_string).unwrap_or_default();
                let list = &mut self.ui_mut().list;
                let before = list.listing();
                let resorted = list.sync(pane);
                (location, resorted, list.listing() != before)
            };
            resorted_any |= resorted;
            if replaced {
                for v in self.ui_mut().views.values_mut() {
                    v.reset();
                }
            }
            ready &= self.place_pane();
            if location != self.ui().address_shown {
                self.ui_mut().address_shown.clone_from(&location);
                let address = self.ui().address.clone();
                address.update(cx, |i, cx| i.set_text(location, cx));
            }
        }
        self.pane = saved;
        if self.presented.is_some()
            && (ready
                || self
                    .held_since
                    .is_none_or(|since| since.elapsed() >= crate::tabs::HOLD))
        {
            self.present(ready, window, cx);
        }
        self.take_reveal(cx);
        self.sync_loading(cx);
        if report.applied > 0 || resorted_any {
            tracing::debug!(
                applied = report.applied,
                resorted = resorted_any,
                rows = self.ui().list.sorted.len(),
                took_us = t0.elapsed().as_micros() as u64,
                "sync"
            );
        }
        crate::settings::flush_remembered(self, cx);
        // The preview follows what is drawn: the tab left, while it is.
        let drawn = self.drawn_pane();
        self.with_pane(drawn, |this| {
            this.sync_preview(window, cx);
            this.sync_busy(cx);
            this.sync_reader(cx);
            this.sync_document_scroll(window, cx);
            this.sync_pictures(window, cx);
            this.sync_notes(window, cx);
            this.sync_media(window, cx);
            this.sync_player(window, cx);
        });
        for id in self.panes.iter().map(|p| p.id).collect::<Vec<_>>() {
            self.with_pane(id, |shell| {
                shell.sync_usage(cx);
                shell.sync_tags_lens(cx);
            });
        }
        self.sync_timeline(cx);
        self.sync_shelf(cx);
        self.sync_terminal(window, cx);
    }

    /// The shelf publishes how many items it holds in `App::tallies`; read
    /// it when that moved, never by asking the shelf each frame.
    fn sync_shelf(&mut self, cx: &mut Context<Self>) {
        let revision = self.app.tallies.revision();
        if self.tallies_revision == Some(revision) {
            return;
        }
        self.tallies_revision = Some(revision);
        let n = self
            .app
            .tallies
            .get(SHELF_ITEMS)
            .map_or(0, |n| usize::try_from(n).unwrap_or(usize::MAX));
        if n != self.shelf_count {
            self.shelf_count = n;
            cx.notify();
        }
    }

    /// One pane column: its address bar and its view (or the usage panel).
    /// Pane `id`: all the room it is given, or `share` of a split's. The
    /// second pane of a split (`divider`) carries the edge between them.
    fn render_pane(
        &mut self,
        id: PaneId,
        share: Option<f32>,
        divider: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let saved = self.pane;
        self.pane = id;
        let theme = cx.global::<Theme>().clone();
        // The pane commands act on among those drawn.
        let active = saved == id;
        crate::search_bar::sync(self, window);
        crate::trail::sync(self);
        let address_bar = address::render_address(self, window, cx);
        // The Usage view draws its own scanning and empty states.
        let placeholder = (self.ui().view_id != "usage")
            .then(|| crate::placeholder::of(self, id))
            .flatten();
        let body = if let Some(placeholder) = placeholder {
            let focus = self.ui().placeholder_focus.clone();
            crate::placeholder::render(placeholder, &focus, cx)
        } else {
            match self.take_view() {
                Some(mut v) => {
                    let el = v.render(self, window, cx);
                    self.put_view(v);
                    el
                }
                None => div()
                    .flex_1()
                    .child("no view registered")
                    .into_any_element(),
            }
        };
        let message = crate::loading::message(self, id);
        self.sync_narrow();
        let narrow = crate::narrow::render(self, cx);
        let file_area = self.pane_container(body, cx);
        let strip = if active {
            timeline::render(self, cx)
        } else {
            None
        };
        // Its footer; the last pane's ends with the window's own state.
        self.sync_weight(window, cx);
        let last = self.panes.last().map(|p| p.id) == Some(id);
        let footer = status::render(self, last, cx);
        self.pane = saved;
        // Split panes are told apart by the edge they share: the left one
        // side by side, the top one stacked. The active pane's is the accent.
        let stacked = self.layout.stacked();
        let edge = divider.then(|| split_edge(stacked, theme.handle, cx));
        div()
            .relative()
            .flex_1()
            // Shares of the room left once each pane is laid out from
            // nothing (`flex_1`'s basis is 0): exactly the dragged split.
            .when_some(share, |mut d, share| {
                d.style().flex_grow = Some(share);
                d
            })
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .on_mouse_down(
                gpui::MouseButton::Navigate(gpui::NavigationDirection::Back),
                cx.listener(move |this, _, window, cx| {
                    this.navigate_history(Some(id), "back", window, cx);
                }),
            )
            .on_mouse_down(
                gpui::MouseButton::Navigate(gpui::NavigationDirection::Forward),
                cx.listener(move |this, _, window, cx| {
                    this.navigate_history(Some(id), "forward", window, cx);
                }),
            )
            .when(self.is_split(), |d| {
                d.when(stacked, gpui::Styled::border_t_1)
                    .when(!stacked, gpui::Styled::border_l_1)
                    .border_color(if active { theme.accent } else { theme.border })
            })
            .child(address_bar)
            .children(narrow)
            .when_some(message, |d, message| {
                d.child(
                    div()
                        .flex_none()
                        .px_3()
                        .py_2()
                        .text_sm()
                        .text_color(theme.text_muted)
                        .bg(theme.surface)
                        .child(message),
                )
            })
            .child(file_area)
            .children(strip)
            .child(footer)
            .children(edge)
            .into_any_element()
    }

    /// The edge between split panes follows the pointer across the room
    /// they share: GPUI hands the drag the bounds of the element listening
    /// (the panes' container), so nothing is measured here. Each pane keeps
    /// what a pane needs, as against the preview and the sidebar.
    fn on_split_drag(
        &mut self,
        ev: &gpui::DragMoveEvent<SplitResize>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let stacked = self.layout.stacked();
        let (along, size) = if stacked {
            (ev.event.position.y - ev.bounds.top(), ev.bounds.size.height)
        } else {
            (ev.event.position.x - ev.bounds.left(), ev.bounds.size.width)
        };
        let keep = if stacked {
            crate::preview::PANE_KEEP_HEIGHT
        } else {
            crate::preview::PANE_KEEP
        };
        self.split_share = split_share(f32::from(along), f32::from(size), keep);
        cx.notify();
    }
}

/// A split's panes share the room evenly until their edge is dragged, and
/// again after a double click on it.
pub(crate) const SPLIT_SHARE: f32 = 0.5;

/// Marker type for dragging the edge between split panes.
#[derive(Clone)]
pub struct SplitResize;

/// The first pane's share when the edge between the panes is `along` px
/// into `size` px of room, each pane keeping at least `keep` px. A room too
/// small for two panes that size is shared evenly.
fn split_share(along: f32, size: f32, keep: f32) -> f32 {
    if size.is_nan() || size <= 2. * keep || !along.is_finite() {
        return SPLIT_SHARE;
    }
    (along.clamp(keep, size - keep)) / size
}

/// The draggable edge a split's second pane has at its start: its left edge
/// side by side, its top edge stacked, over the border that tells the panes
/// apart.
fn split_edge(stacked: bool, hover: gpui::Hsla, cx: &mut Context<Shell>) -> gpui::AnyElement {
    let edge = div()
        .id("split-resize")
        .absolute()
        .top_0()
        .left_0()
        .hover(move |style| style.bg(hover))
        .on_click(cx.listener(|this, event: &gpui::ClickEvent, _, cx| {
            if event.click_count() == 2 {
                this.split_share = SPLIT_SHARE;
                this.remember_session();
                cx.notify();
            }
        }))
        .on_drag(SplitResize, |_, _, _, cx| {
            cx.new(|_| crate::preview::NoDragView)
        });
    if stacked {
        edge.w_full()
            .h(px(6.))
            .cursor(gpui::CursorStyle::ResizeUpDown)
    } else {
        edge.h_full()
            .w(px(6.))
            .cursor(gpui::CursorStyle::ResizeLeftRight)
    }
    .into_any_element()
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.first_frame_logged {
            self.first_frame_logged = true;
            tracing::info!(
                "first frame {:.1} ms after start",
                self.started.elapsed().as_secs_f64() * 1000.0
            );
        }
        self.sync(window, cx);
        // While the tab shown gets ready, the whole window draws the tab left
        // as it was: its panes, toolbars, status and preview.
        let held = self.presented.take().map(|mut panes| {
            let drawn = self.drawn_active(&panes);
            // `panes` becomes the tab shown's, put back after the frame.
            std::mem::swap(&mut self.panes, &mut panes);
            (panes, std::mem::replace(&mut self.pane, drawn))
        });
        // Like the sidebar below, the arrangement follows the window's own
        // size, known before layout; decide it before anything is built.
        let viewport = window.viewport_size();
        self.layout
            .observe(f32::from(viewport.width), f32::from(viewport.height));
        let (stacked, bottom) = (self.layout.stacked(), self.layout.bottom());
        let theme = cx.global::<Theme>().clone();
        // The window's own width is known before layout, unlike an element's.
        // Below it the panes keep the room: places stay reachable through the
        // address bar, the palette and Jump.
        let sidebar_width = self.sidebar_drawn_width(window);
        // Where the window's own tools go this frame (`address::corner`):
        // the sidebar's header, or the first pane's row.
        self.sidebar_drawn = sidebar_width;
        self.more_anchor = crate::elements::popover::PopoverAnchor::default();
        let side = (sidebar_width > 0.).then(|| sidebar::render(self, sidebar_width, cx));
        let strip = crate::tabs::render(self, cx);
        let pane_ids: Vec<PaneId> = self.panes.iter().map(|p| p.id).collect();
        let split = pane_ids.len() == 2;
        let mut columns = Vec::new();
        for (index, id) in pane_ids.into_iter().enumerate() {
            // Split, the panes share the room as the edge between them was
            // dragged; the second draws that edge.
            let share = match (split, index) {
                (false, _) => None,
                (true, 0) => Some(self.split_share),
                (true, _) => Some(1. - self.split_share),
            };
            columns.push(self.render_pane(id, share, index > 0, window, cx));
        }
        let limit = if bottom {
            self.preview_height_limit(window)
        } else {
            self.preview_width_limit(window)
        };
        let panel = preview::render_panel(self, bottom, limit, self.dock_height_limit(window), cx);
        let terminal = self.render_terminal(window, cx);
        // Not deferred: GPUI cannot nest deferred draws and the dialog's own
        // dropdowns are. It is the last overlay, so it paints above the rest.
        let settings = self
            .settings
            .clone()
            .map(gpui::IntoElement::into_any_element);
        let toast = crate::toast::render(self, cx);
        let sequence = crate::sequence::render(self, cx);
        let shortcuts = crate::shortcuts::render(self, cx);
        let overlays = [
            address::more_popup(self, cx),
            menu::render(self, cx),
            prompts::render_prompt(self, cx),
            prompts::render_confirm(self, cx),
            jump::render(self, cx),
            crate::choose::render(self, cx),
            crate::tags::render(self, cx),
            renamebulk::render(self, window, cx),
            palette::render(self, cx),
            preview::render_peek(self, cx),
            review::render(self, cx),
        ];

        let root = div()
            .key_context(keymap::ROOT)
            .on_action(cx.listener(Self::on_run_command))
            // Ctrl (Cmd) held over a link found in text makes it one.
            .on_modifiers_changed(cx.listener(Self::on_modifiers))
            // Before anything under the pointer: a popup closed by this
            // press notes it (`note_dismissed`).
            .capture_any_mouse_down(cx.listener(|this, _, _, _| this.presses += 1))
            // The mouse's side buttons, where no pane under the pointer
            // took them first.
            .on_mouse_down(
                gpui::MouseButton::Navigate(gpui::NavigationDirection::Back),
                cx.listener(|this, _, window, cx| {
                    this.navigate_history(None, "back", window, cx);
                }),
            )
            .on_mouse_down(
                gpui::MouseButton::Navigate(gpui::NavigationDirection::Forward),
                cx.listener(|this, _, window, cx| {
                    this.navigate_history(None, "forward", window, cx);
                }),
            )
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .font_family(self.font.clone())
            .text_sm()
            .bg(theme.bg)
            .text_color(theme.text)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .children(side)
                    // The panes and the preview: a row with the panel at the
                    // right edge, a column with it at the bottom; the panes
                    // among themselves, side by side or stacked.
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .when(bottom, gpui::Styled::flex_col)
                            // The tabs over the panes they arrange, and the
                            // terminal under them, whose top edge is dragged
                            // within this column.
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .min_w_0()
                                    .min_h_0()
                                    .on_drag_move(cx.listener(Self::on_terminal_height_drag))
                                    .children(strip)
                                    .child(
                                        div()
                                            .flex()
                                            .flex_1()
                                            .min_w_0()
                                            .min_h_0()
                                            .when(stacked, gpui::Styled::flex_col)
                                            // The edge between split panes is
                                            // dragged within these bounds.
                                            .on_drag_move(cx.listener(Self::on_split_drag))
                                            .children(columns),
                                    )
                                    .children(terminal),
                            )
                            .children(panel),
                    ),
            )
            .children(toast)
            .children(sequence)
            .children(overlays.into_iter().flatten())
            .children(settings)
            .children(shortcuts)
            .child(div().h(px(0.)));
        // Icons the panels and the sidebar missed while building this frame;
        // the views ask for theirs as their rows are built.
        let app = Arc::clone(&self.app);
        self.icons.request(&app);
        if let Some((current, pane)) = held {
            self.presented = Some(std::mem::replace(&mut self.panes, current));
            self.pane = pane;
        }
        self.resize_hook(root, cx)
    }
}

#[cfg(test)]
mod tests {
    use super::{SPLIT_SHARE, split_share};

    #[test]
    fn the_split_follows_the_pointer_and_each_pane_keeps_its_room() {
        // A quarter of 1000 px of room.
        assert!((split_share(250., 1000., 240.) - 0.25).abs() < 1e-6);
        // Past what the other pane keeps, or before what this one keeps.
        assert!((split_share(900., 1000., 240.) - 0.76).abs() < 1e-6);
        assert!((split_share(-50., 1000., 240.) - 0.24).abs() < 1e-6);
        // Room for less than two panes: shared evenly.
        assert!((split_share(100., 400., 240.) - SPLIT_SHARE).abs() < f32::EPSILON);
    }
}
