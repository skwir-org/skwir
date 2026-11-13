//! The shell entity: owns the window's UI state (per pane and global),
//! routes every command, and wakes only when the core signals new events.

use crate::sizes::TextSizes;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Instant;

use gpui::{
    ClipboardItem, Context, Entity, FocusHandle, IntoElement, Subscription, Window, div,
    prelude::*, px,
};
use skwir_core::app::NotifyLevel;
use skwir_core::command::{Invocation, Outcome};
use skwir_core::os_path::PathArg;
use skwir_core::scheduler::Lane;
use skwir_core::workspace::PaneId;
use skwir_core::{App, Location, Plan};

use crate::choose::ChooseState;
use crate::icons::IconCache;
use crate::input::{InputEvent, TextInput};
use crate::keymap::{self, RunCommand};
use crate::list::{ListState, Step};
use crate::menu::MenuState;
use crate::palette::PaletteState;
use crate::preview::PreviewState;
use crate::prompts::{ConfirmState, Confirmed, PromptKind, PromptState};
use crate::renamebulk::BulkState;
use crate::theme::Theme;
use crate::thumbs::ThumbCache;
use crate::timeline::TimelineState;
use crate::usage::UsageState;
use crate::view::{self, PaneView};
use crate::views::grid::{DEFAULT_ZOOM, ZOOMS};
use crate::{
    address, menu, palette, preview, prompts, renamebulk, review, sidebar, status, timeline,
};

/// What the shell says when a command needs a selection and there is none.
pub const NOTHING_SELECTED: &str = "Select a file or folder first";

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
    view_registrations: HashMap<String, skwir_core::registry::Factory>,
    /// Registry revision the registrations above were reconciled against.
    views_revision: u64,
    pub view_id: &'static str,
    /// The pane's own view, set aside while the folder it shows is shown
    /// in the view that folder remembers (ADR-132, `Shell::sync_place_views`);
    /// none while it shows its own.
    pub(crate) own_view: Option<&'static str>,
    /// The view the place shown remembered when last looked at, so a sync
    /// acts only on a change.
    place_view: Option<std::sync::Arc<str>>,
    /// The pane's own view while a lens is shown over it (`set_view`): what
    /// a view a folder remembers sets aside when one is chosen from the
    /// lens, as it would from the pane's own (ADR-132).
    own_under_lens: Option<&'static str>,
    pub usage: UsageState,
    pub loading: crate::loading::State,
    /// The address field while it is typed in (`omnibox.rs`).
    pub omni: Option<crate::omnibox::Omni>,
    /// The list of the address card's rows, kept from one opening to the
    /// next: it knows the width it was drawn at, where gpui measures rows
    /// that wrap (an inferred list), so the card is as tall as they are
    /// from its first frame.
    pub card_rows: crate::omnibox::Listed,
    /// What the files it shows weigh, for its footer (`weight.rs`).
    pub weight: crate::weight::Weight,
    /// What Quick filters offer over what it shows.
    pub quick_filters: crate::quick_filters::QuickFilters,
    /// Quick filters asked for above its files (`ui.quick_filters`),
    /// wherever it goes: they show by themselves where the location offers
    /// them (a tag's place, a search with a tag) whatever this says.
    pub quick_filters_on: bool,
    /// Its timeline strip, under its files (`timeline.rs`).
    pub timeline: TimelineState,
    /// The chips of the search it shows (`search_bar`).
    pub chips: crate::search_bar::Chips,
    /// What narrows the folder it shows, as the same chips (`search_bar`).
    pub filters: crate::search_bar::Filters,
    /// The trail and verbs of the location it shows, when that is no
    /// folder (`trail.rs`).
    pub place: crate::trail::Shown,
    /// The first segment of its trail the last layout left in sight: a
    /// narrow pane wraps those before it out (`address::breadcrumbs`).
    /// The layout's answer, read when a chevron is pointed at or pressed.
    pub(crate) trail_shown: std::rc::Rc<std::cell::Cell<usize>>,
    /// The placeholder's button, kept so Tab can reach it.
    pub(crate) placeholder_focus: FocusHandle,
    /// Its pane came back (or is new) and has not been put in place yet
    /// (`Shell::place_pane`).
    pub(crate) returning: bool,
    /// Where it was scrolled when its tab was left: the entry at the top,
    /// its index then, and how many pixels into its row the view began.
    pub(crate) anchor: Option<(skwir_core::EntryId, usize, f32)>,
    /// What it will show first, still waiting for metadata.
    pub(crate) warm: Vec<skwir_core::EntryId>,
    /// The address field's events, its hint taken, the start of its text,
    /// and its focus lost.
    _sub: (Subscription, Subscription, Subscription, Subscription),
}

/// The views a pane holds (ADR-132): the one `shown`, its `own` set aside
/// while a folder's is shown, and its own `under_lens` while a lens is
/// shown over it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Views {
    shown: &'static str,
    own: Option<&'static str>,
    under_lens: Option<&'static str>,
}

impl Views {
    /// Once view `k` is chosen: a `lens` over the pane's own, which stays
    /// the pane's under it; a view a folder remembers there the folder's
    /// (`folder`), the pane's own set aside where that is one a folder may
    /// remember too (`remembered`), else this one the pane's own as well;
    /// any other the pane's own.
    fn choose(
        self,
        k: &'static str,
        lens: bool,
        folder: bool,
        remembered: impl Fn(&str) -> bool,
    ) -> Views {
        let own = self.under_lens.or(self.own).unwrap_or(self.shown);
        let (own, under_lens) = if lens {
            (None, Some(own))
        } else if folder {
            (remembered(own).then_some(own), None)
        } else {
            (None, None)
        };
        Views {
            shown: k,
            own,
            under_lens,
        }
    }
}

impl PaneUi {
    /// Its own view: what it shows wherever no folder's view is shown
    /// (ADR-132), and what a window, a tab or a pane made from it keeps.
    pub(crate) fn own_view(&self) -> &'static str {
        self.own_view.unwrap_or(self.view_id)
    }

    /// What it carries to the window its tab moves to.
    pub(crate) fn carried(&self) -> Carried {
        Carried {
            view_id: self.own_view(),
            quick_filters_on: self.quick_filters_on,
            timeline: self.timeline.like(),
        }
    }

    /// Show its files as `carried` from the window its tab came from.
    pub(crate) fn shows_as(&mut self, carried: Carried) {
        if self.views.contains_key(carried.view_id) {
            self.view_id = carried.view_id;
        }
        self.quick_filters_on = carried.quick_filters_on;
        self.timeline = carried.timeline;
    }

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
        // Its strip counts again when it is shown again, as it was asked.
        self.timeline = self.timeline.like();
    }
}

/// A pending "show me these": the pane they belong to, the names to look
/// for (the file a link leads to; what a plan put in the folder shown),
/// and when to stop waiting for a listing that never brings them.
struct Reveal {
    pane: PaneId,
    names: Vec<std::ffi::OsString>,
    until: Instant,
    /// The listing and its names when last looked through: looked through
    /// again only once they change, not at every sync while it waits.
    looked: Option<(u64, u64)>,
    /// The selection it made of those listed so far, while the others are
    /// waited for (what a paste put, listed as the watcher tells it): a
    /// selection made since wins.
    selected: Option<u64>,
}

impl Reveal {
    fn new(pane: PaneId, names: Vec<std::ffi::OsString>) -> Self {
        Self {
            pane,
            names,
            until: Instant::now() + std::time::Duration::from_secs(10),
            looked: None,
            selected: None,
        }
    }
}

/// The latest plan the user asked, and the folders the panes shown then
/// showed: what it puts in one of them (a new folder, what was pasted or
/// dropped, an archive made or extracted) is selected there once it is
/// done, as other file managers do (`Shell::made`).
struct Making {
    plan: skwir_core::op::PlanId,
    /// The pane it was asked from: chosen where several show the folder.
    asked: PaneId,
    shown: Vec<Shown>,
    /// What its steps put in those folders, as the executor told each
    /// (`Shell::made_step`): what it did where its steps are its items.
    placed: Vec<std::path::PathBuf>,
}

/// A pane as a plan found it: the folder it showed, and its selection
/// (`Selection::revision`). Shown elsewhere by the plan's end, or selected
/// otherwise meanwhile, it is left as it is.
struct Shown {
    pane: PaneId,
    folder: std::path::PathBuf,
    location: Location,
    selection: u64,
}

/// The shell's switches read from the settings (`Shell::sync`, when their
/// revision changes), never per frame.
/// Where Split, Properties and Preview are (`Shell::toggles_at`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TogglesAt {
    Preview,
    Strip,
    Pane(PaneId),
}

/// Where the sidebar's toggle and More are (`Shell::corner_at`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CornerAt {
    Sidebar,
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
    /// `ui.tabs_at`: the tabs are listed in the sidebar, not the strip.
    pub tabs_in_sidebar: bool,
    /// `ui.full_path`: a folder's trail starts at the root.
    pub full_path: bool,
    /// `ui.view_buttons_shown`: the views have buttons beside the
    /// address; off, one button, the view shown, opens the View menu.
    pub view_buttons_shown: bool,
    /// `ui.up_button`: Up stands after Back and Forward.
    pub up_button: bool,
    /// `ui.printed_pages`: a book's position counts its printed pages.
    pub printed_pages: bool,
    /// `ui.wrap_lines`: a text's long lines wrap to the panel, else go on
    /// sideways (`text_view`).
    pub wrap_lines: bool,
    /// `ui.text_continuous`: what is read scrolls as one column, else
    /// turns a screen at a time (`inspector_pages`).
    pub text_continuous: bool,
    /// `reading.read_ahead`: the picture the next step shows is decoded
    /// ahead (`Shell::read_ahead_next`).
    pub read_ahead: bool,
    /// `ui.drag_image`: how a drag of several files is drawn.
    pub drag_image: crate::dragicon::Layout,
    /// `ui.model_perspective` and `ui.model_smooth`: how a 3D model is
    /// drawn.
    pub model_look: crate::elements::model::Look,
    /// `reading.contents_pinned`: the contents stay open beside what is
    /// read (`outline.rs`).
    pub contents_pinned: bool,
    /// `core.close_split`: which pane closing the split takes away.
    pub close_split: crate::layout::CloseSplit,
    /// How audio and video play.
    pub player: crate::player::PlayerPrefs,
    /// `ui.notify_desktop`: a long action that ended unwatched is a
    /// notification (ADR-114).
    pub notify_desktop: bool,
    /// `ui.keep_awake`: the computer kept awake while an action runs.
    pub keep_awake: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            friendly_dates: true,
            grid_fill: false,
            single_tab: false,
            tabs_in_sidebar: false,
            full_path: false,
            view_buttons_shown: true,
            up_button: false,
            printed_pages: false,
            wrap_lines: true,
            text_continuous: true,
            read_ahead: true,
            drag_image: crate::dragicon::Layout::Fan,
            model_look: crate::elements::model::Look::default(),
            contents_pinned: false,
            close_split: crate::layout::CloseSplit::Other,
            player: crate::player::PlayerPrefs::default(),
            notify_desktop: true,
            keep_awake: true,
        }
    }
}

/// The keymap GPUI has: `sync` installs it again when the registries or
/// the user's bindings change.
pub(crate) struct Installed {
    stamp: keymap::Stamp,
    /// Every binding, those hidden too: what the Keyboard page lists.
    bound: Arc<Vec<keymap::Bound>>,
    /// The settings revision the user's bindings were last read at.
    settings: u64,
    /// Whether letters typed in the file list jump to a name
    /// (`keymap::Keymap::typeahead`).
    pub typeahead: bool,
}

impl Installed {
    pub(crate) fn bound(&self) -> Arc<Vec<keymap::Bound>> {
        Arc::clone(&self.bound)
    }
}

// Flags of independent parts of the window (the first frame logged, a
// screen reader listening…), not the states of one machine.
#[allow(clippy::struct_excessive_bools)]
pub struct Shell {
    pub app: Arc<App>,
    /// The core's window this one draws (ADR-122): its tabs, its panes.
    pub(crate) window_id: skwir_core::workspace::WindowId,
    /// What the program took from the core for this window, taken in at
    /// the next sync (`finishing::deliver`).
    pub(crate) inbox: crate::finishing::Inbox,
    /// The pane commands act on: this window's active pane, or the pane
    /// being rendered while a pane renders.
    pub pane: PaneId,
    pub panes: Vec<PaneUi>,
    /// The panes of the tabs not shown, what they drew from let go
    /// (`Shell::sync_tabs`).
    pub(crate) hidden: HashMap<PaneId, PaneUi>,
    /// Workspace revision the panes above follow.
    pub(crate) tabs_revision: u64,
    pub(crate) shown_tab: Option<skwir_core::workspace::TabId>,
    /// How many tabs the workspace has, as of `tabs_revision`: read while
    /// drawing, under a workspace guard the caller may already hold.
    pub(crate) tab_count: usize,
    /// The splits of tabs not shown: their own arrangement and share.
    pub(crate) tab_splits: HashMap<skwir_core::workspace::TabId, crate::layout::TabSplit>,
    pub(crate) tab_labels: crate::tabs::Labels,
    /// The tab under the pointer, which shows its × (`tabs::closes`).
    pub(crate) tab_hovered: Option<skwir_core::workspace::TabId>,
    /// The panes of the tab left, still drawn while the tab shown gets
    /// ready (`Shell::present`), and which tab they are.
    pub(crate) presented: Option<Vec<PaneUi>>,
    pub(crate) presented_tab: Option<skwir_core::workspace::TabId>,
    /// When the tab shown started waiting to be drawn, and the wake at its
    /// deadline.
    pub(crate) held_since: Option<Instant>,
    pub(crate) hold_timer: Option<gpui::Task<()>>,
    /// Syncs once `tabs::HOLD` has passed while a pane draws the files of
    /// the folder it left (`ListState::holds_left`).
    files_hold: Option<gpui::Task<()>>,
    pub key_hints: keymap::Hints,
    /// What `key_hints` and GPUI's bindings were built from.
    pub(crate) keys: Installed,
    /// Icon size of the grid's tiles.
    pub zoom: u32,
    /// Icon size of the list's and the columns' rows; their height follows.
    pub row_zoom: u32,
    /// Levels of folders the Usage map draws inside each other.
    pub levels: u32,
    /// What the Usage map's blocks say: the room, the files, their age
    /// or their kinds.
    pub usage_mode: crate::usage::Mode,
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
    /// The panes, any window's, whose listings the last sync's pump moved:
    /// their windows draw (`finishing::woken`).
    pumped: Vec<skwir_core::workspace::PaneId>,
    /// Preferences changed from the toolbars and not saved yet
    /// (`settings::remember`).
    pub(crate) unsaved: std::collections::BTreeMap<String, String>,
    /// How the user arranged the sidebar's groups (`ui.sidebar_section`).
    pub sidebar_sections: crate::sidebar::Sections,
    /// The places as the user arranged them, and the places' revision
    /// and arrangement they were arranged for (`Shell::arranged_places`).
    pub(crate) arranged_places: std::cell::RefCell<Option<crate::sidebar::ArrangedAt>>,
    /// The folders the sidebar shows, where trails start.
    pub roots: crate::sidebar::Roots,
    /// Which detail columns the list shows (`views::list::COLUMNS`), as
    /// chosen. Read through `shown_columns`.
    pub columns: crate::views::list::PerColumn<bool>,
    /// Which captions the grid's tiles say under their names
    /// (`views::grid::CAPTIONS`), as chosen. Read through `grid_captions`.
    pub captions: [bool; crate::views::grid::CAPTIONS.len()],
    /// `reading.width`: the reading column's widest, in ems, or `None`
    /// for the whole panel (`markdown::measure`).
    pub reading_measure: Option<f32>,
    /// `ui.folder_preview_file` and `ui.folder_preview_names`: the files
    /// that stand for a folder in its preview; `None` while it is off or
    /// names none.
    pub(crate) folder_stand_ins: Option<crate::preview::StandIns>,
    /// The reading width before Full width was switched on from a preview,
    /// to return to when it is switched off.
    reading_width_before: Option<String>,
    /// Ctrl and the wheel over what zooms: a trackpad must mean it.
    pub zoom_wheel: crate::reader_scroll::WheelTurn,
    /// The wheel over the tab strip, counted in notches like the zoom's.
    pub tab_wheel: crate::reader_scroll::WheelTurn,
    pub column_widths: crate::views::list::PerColumn<f32>,
    /// Where the pointer and the size were as an edge's drag began, for an
    /// edge whose area grows by the pointer's travel (a list column's, a
    /// table preview's): the pointer where it pressed the edge
    /// (`edge_pressed`), the size at the drag's first move. Every press on
    /// an edge clears it (`elements::edge`).
    pub edge_start: Option<(f32, f32)>,
    /// Where the pointer pressed an edge, across. A drag measures from
    /// there: gpui reports its moves from the one after the move that
    /// began it, which would leave the area behind the pointer.
    pub edge_pressed: f32,
    pub preview: PreviewState,
    /// A track kept playing while the selection moves (`player.rs`).
    pub player: crate::player::PlayerState,
    pub palette: Option<PaletteState>,
    /// `ui.choose`: a list to pick one from.
    pub choose: Option<ChooseState>,
    /// `ui.tags_prompt`: the tags of the selection, to change.
    pub tag_editor: Option<crate::tags::TagEditor>,
    /// The colors the user chose for tags, read when the settings change.
    pub tag_colors: crate::tags::TagColors,
    /// The tags pinned to the sidebar, which come first among an entry's.
    pub tag_pins: crate::tags::TagPins,
    /// The place of the sidebar pointed at that says tags of what it
    /// lists, and those tags: the files that have them are ringed
    /// (`Shell::ring_of`).
    pub ringed: Option<(skwir_core::Location, Vec<String>)>,
    /// The categories a file has one tag of at a time: fields
    /// (`metadata::TAG_EXCLUSIVE_SETTING`), read when the settings change.
    pub tag_fields: Vec<String>,
    /// The views given a button beside the address, or taken it from
    /// (`ui.view_button`), read when the settings change.
    pub view_buttons: crate::address::ViewButtons,
    pub bulk: Option<BulkState>,
    pub prompt: Option<PromptState>,
    /// The form a plugin asked shown, and those waiting their turn
    /// (`ask_forms`).
    pub form: Option<crate::form::FormState>,
    pub forms: std::collections::VecDeque<skwir_core::form::Asked>,
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
    /// the window's toggle and More (`corner_at`).
    pub(crate) sidebar_drawn: f32,
    /// Where the window's cards are this frame (`Theme::cards`): written
    /// as they are laid out, the frame painted around them
    /// (`elements::frame`).
    pub(crate) frame_cards: crate::elements::frame::Cards,
    /// The parts of the tab strip and of the sidebar this frame (the tab
    /// shown), holes in their grounds apart by lines (`elements::ground`).
    pub(crate) strip_holes: crate::elements::ground::Holes,
    pub(crate) sidebar_holes: crate::elements::ground::Holes,
    /// While a pane is drawn as a card of a split (`render_pane`), whether
    /// it is the one commands act on (raised) or not (lowered).
    pub(crate) drawn_raised: Option<bool>,
    /// Where More hangs from this frame, wherever its button is.
    pub(crate) more_anchor: crate::elements::popover::PopoverAnchor,
    /// Counts the presses of a mouse button in the window, and which popup
    /// the last press closed from outside (`note_dismissed`).
    pub(crate) presses: u64,
    pub(crate) dismissed: Option<(u64, String)>,
    /// A press on a button that does something else when held
    /// (`toolbar::Button::on_hold`).
    pub(crate) held: Option<crate::toolbar::Held>,
    /// A search chip's open choices.
    pub search_menu: Option<crate::search_bar::ChipMenu>,
    pub settings: Option<Entity<crate::settings::SettingsView>>,
    pub(crate) settings_subscription: Option<Subscription>,
    /// Settings revision the theme was last resolved against.
    theme_revision: u64,
    /// Plans proposed by background work, oldest first; the front is shown.
    pub reviews: VecDeque<crate::review::Review>,
    pub review_focus: Option<crate::review::ReviewFocus>,
    /// The tiles maps are drawn on, while a map is shown (`map_tiles`).
    pub(crate) map_tiles: std::rc::Rc<std::cell::RefCell<crate::map_tiles::TileStore>>,
    pub(crate) map_notes: crate::map_preview::Notes,
    /// Panes that showed a Map view: their scan is let go when they stop.
    pub(crate) map_views: std::collections::HashSet<PaneId>,
    /// Panes whose Map view spreads the files at one place
    /// (`ui.map_view_spread`).
    pub(crate) map_spread: std::collections::HashSet<PaneId>,
    /// What each pane's Map view drew last (its map's handles): the area,
    /// for `ui.map_view_search_area`; the view, for the hints a map says.
    pub(crate) map_drawn: std::collections::HashMap<PaneId, crate::elements::map::Handles>,
    /// The terminal under the panes (F4).
    pub terminal: crate::terminal::TerminalState,
    /// Panes side by side or stacked, the preview at the right or the bottom.
    pub layout: crate::layout::Layout,
    pub toasts: crate::toast::ToastState,
    /// What runs, docked at the window's foot, and what ended (ADR-114).
    pub activity: crate::activity::ActivityState,
    /// The History, opened from the footer's Undo (ADR-079, 13).
    pub undo_history: crate::history::HistoryState,
    /// What runs and what was said of it, the desktop told (ADR-114): the
    /// program's for its life, shared with the window while it is up
    /// (ADR-115, `finishing.rs`). Never borrowed across a call that may
    /// reach the shell again.
    pub(crate) work: std::rc::Rc<std::cell::RefCell<crate::work::Work>>,
    /// The window left (`leave`): the shell takes nothing more of what the
    /// core sends or asks, which the program takes in without it.
    left: bool,
    /// Whether the window has the focus: what ends while it has not is
    /// told to the desktop.
    pub(crate) window_active: bool,
    /// This frame's snapshot of what waits for a paste; rows and cells test
    /// membership here instead of taking the lock each.
    pub pending: Arc<skwir_core::pending::Pending>,
    pub shortcuts: Option<crate::shortcuts::Overview>,
    pub about: Option<crate::about::About>,
    /// A rubber-band selection in progress.
    pub marquee: Option<crate::elements::marquee::Drag>,
    /// What plans' steps ask that only the user knows (an archive's
    /// password), waiting for the prompt to be free (`ask_secrets`).
    pub secrets: std::collections::VecDeque<skwir_core::app::SecretQuestion>,
    /// The question the secret prompt shows, handed on with it if its
    /// window closes (`take_held`).
    pub(crate) secret_shown: Option<skwir_core::app::SecretQuestion>,
    pub nudges: crate::shortcuts::Nudges,
    /// What `/` looked for last, and whether backward: `n` and `N` look
    /// for it again.
    pub last_find: Option<(String, bool)>,
    /// Keys typed that wait for the next, and what can follow.
    pub(crate) sequence: Option<crate::sequence::Sequence>,
    /// What plugins count for the footer (`App::tallies`).
    pub tallies: Vec<skwir_core::tally::Tally>,
    /// The `App::tallies` revision they were read at.
    tallies_revision: Option<u64>,
    /// A plain press on a row of a multi-selection, waiting for its
    /// release to make the selection that row (unless a drag starts).
    pub press_collapse: Option<(PaneId, skwir_core::EntryId)>,
    /// The icon of the files dragged last (`dnd::FilesIcon`).
    pub files_icon: Option<crate::dnd::FilesIcon>,
    /// A file to select once the folder it lives in has listed it: a listing
    /// streams, so a link followed to another folder cannot select its target
    /// in the same frame it navigates.
    reveal: Option<Reveal>,
    /// Advances with each `reveal_path`, so only the latest one's answer
    /// from its worker is acted on.
    reveal_asked: u64,
    /// The latest plan the user asked, what it puts in a folder shown to
    /// select once it is done.
    making: Option<Making>,
    /// The window's commands asked from elsewhere (an agent, a script)
    /// that the dialog open would take instead (`Shell::takes_now`),
    /// oldest first: they run once it closes.
    front_waiting: VecDeque<Invocation>,
    /// Links as the pointer and Ctrl left them, and a line waiting for its
    /// file's preview (`links.rs`).
    pub(crate) links: crate::links::LinkState,
    /// The window's shape as last written to settings; `remember_session`
    /// compares against it so nothing is written on an ordinary frame.
    /// `None` until the last window's shape was read (`restore_session`):
    /// until then nothing is written.
    pub(crate) stored_session: Option<crate::session::Session>,
    /// Where the window is and how big (`Session::window`), followed as it
    /// moves and is resized, and remembered once it stays a moment
    /// (`window_moved`).
    pub(crate) placement: crate::session::Placement,
    placement_wait: Option<gpui::Task<()>>,
    /// Scroll position of the sidebar, so it can have a scrollbar.
    pub sidebar_scroll: gpui::ScrollHandle,
    /// The sidebar's keyboard (F6): its focus and its cursor.
    pub sidebar_keys: crate::sidebar::Keys,
    /// What the window's first frame is timed from, until it is logged.
    first_frame: Option<FirstFrame>,
    /// Whether assistive technology listens this frame
    /// (`Window::is_a11y_active`): what only it reads (a row's label, its
    /// place in the list) is built only then
    /// (`docs/invariants/accessibility.md`).
    pub(crate) a11y: bool,
    /// Whether the window turns 3D models (ADR-067): it draws meshes
    /// (`Window::draws_meshes`) on a GPU, not a software rasterizer, whose
    /// frame grows with the mesh while the window's thread waits on it. A
    /// model's preview then asks for its mesh, else for its still. Read at
    /// each render: the renderer may come after the shell.
    pub(crate) draws_meshes: bool,
    /// What the window's title was last set from: the active pane's
    /// location's title.
    title: gpui::SharedString,
    /// What of Skwir's the system's clipboard may hold (ADR-062).
    pub(crate) clipboard_holds: crate::clipboard::Held,
    _subscriptions: Vec<Subscription>,
}

impl Shell {
    pub(crate) fn new(
        app: Arc<App>,
        window_id: skwir_core::workspace::WindowId,
        start: Start,
        keymap: keymap::Keymap,
        work: std::rc::Rc<std::cell::RefCell<crate::work::Work>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let Start {
            shape,
            split,
            first_frame,
            placed,
        } = start;
        let theme = Theme::resolve(&app, window.appearance(), cx.text_system());
        tracing::info!(
            ui = %theme.font,
            reading = %theme.reading_font,
            code = %theme.code_font,
            sizes = ?theme.sizes,
            "fonts"
        );
        // The window is written at the interface's size (ADR-117).
        window.set_rem_size(theme.sizes.rem(crate::sizes::Part::Interface));
        show_behind(&theme, window);
        cx.set_global(theme);
        show_resolved(&app, cx);
        let mut subs = Vec::new();
        subs.push(cx.observe_window_appearance(window, |this, window, cx| {
            // Where the system's colors are not followed (macOS), its
            // new look's are read again (ADR-071).
            crate::desktop_style::read_again(&this.app);
            this.resolve_theme(window, cx);
            cx.notify();
        }));
        subs.push(cx.observe_pending_input(window, Self::on_pending_input));
        subs.push(cx.observe_window_bounds(window, Self::window_moved));
        // What no watch tells is asked again as the window comes back: the
        // folders shown on a remote (a server does not say what changed on
        // it, ADR-077), how full each volume is (`App::set_window_focused`).
        subs.push(cx.observe_window_activation(window, |this, window, cx| {
            let active = window.is_window_active();
            this.app.set_window_focused(this.window_id, active);
            if active {
                // The window used last is where commands that name no pane
                // act (ADR-122), whose shape a new window takes.
                this.app.workspace.write().set_active_window(this.window_id);
                crate::finishing::raised(cx, this.window_id);
                this.work.borrow().shown_in(Some(window));
                this.remember_session();
                // An accent or a contrast changed while away (macOS).
                crate::desktop_style::read_again(&this.app);
            }
            // Looked at again: what the desktop was told is taken back.
            this.window_activated(active);
        }));
        // A film's frames are sent only while the window is shown: on
        // another workspace or minimized, nothing would draw them
        // (`Shell::media_shown`).
        subs.push(
            cx.observe_window_visibility(window, |this, visibility, _, _| {
                this.media_shown(visibility.is_visible());
            }),
        );
        // Anything else copied, here or in another application: Skwir's
        // own clipboard forgets the files it put on the system's.
        let shell = cx.entity().downgrade();
        subs.push(cx.on_clipboard_files_gone(move |cx| {
            shell.update(cx, Shell::clipboard_files_gone).ok();
        }));
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
                // The Settings dialog, while it is open, over the pane
                // behind it: what it drew last went (a bar closed).
                let focus = match &this.settings {
                    Some(settings) => Some(gpui::Focusable::focus_handle(settings, cx)),
                    None => this.drawn_focus(),
                };
                if let Some(focus) = focus {
                    window.focus(&focus, cx);
                    cx.notify();
                }
            });
        }));

        // The window's close button closes it, asking nothing: with other
        // windows up, it and its tabs go, Reopen closed tab bringing them
        // back (ADR-122); the last one, what runs goes on without it
        // (ADR-115, `finishing.rs`).
        let shell = cx.weak_entity();
        window.on_window_should_close(cx, move |window, cx| {
            shell
                .update(cx, |shell, cx| shell.close_window(window, cx))
                .ok();
            true
        });

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

        let pane = app.workspace.read().window(window_id).map_or_else(
            || app.workspace.read().active_id(),
            skwir_core::workspace::WindowTabs::active_pane,
        );
        let map_tiles = std::rc::Rc::new(std::cell::RefCell::new(
            crate::map_tiles::TileStore::new(&app, app.window_waker(window_id)),
        ));
        let mut shell = Shell {
            app,
            window_id,
            inbox: crate::finishing::Inbox::default(),
            pane,
            panes: Vec::new(),
            hidden: HashMap::new(),
            tabs_revision: u64::MAX,
            shown_tab: None,
            tab_count: 1,
            tab_splits: HashMap::new(),
            tab_labels: crate::tabs::Labels::default(),
            tab_hovered: None,
            presented: None,
            presented_tab: None,
            held_since: None,
            hold_timer: None,
            files_hold: None,
            key_hints: keymap::Hints::new(keymap.hints),
            keys: Installed {
                stamp: keymap.stamp,
                bound: Arc::new(keymap.bound),
                settings: 0,
                typeahead: keymap.typeahead,
            },
            zoom: DEFAULT_ZOOM,
            row_zoom: crate::views::list::DEFAULT_ROW_ZOOM,
            levels: crate::usage::DEFAULT_LEVELS,
            usage_mode: crate::usage::Mode::default(),
            sidebar_hidden: false,
            sidebar_width: sidebar::DEFAULT_WIDTH,
            split_share: SPLIT_SHARE,
            thumbs: ThumbCache::default(),
            icons: IconCache::default(),
            type_names: crate::typenames::TypeNames::default(),
            prefs: Preferences::default(),
            unsaved: std::collections::BTreeMap::new(),
            pumped: Vec::new(),
            sidebar_sections: crate::sidebar::Sections::default(),
            arranged_places: std::cell::RefCell::default(),
            roots: crate::sidebar::Roots::default(),
            columns: crate::views::list::PerColumn(
                crate::views::list::COLUMNS
                    .each_ref()
                    .map(|column| column.shown),
            ),
            captions: [false; crate::views::grid::CAPTIONS.len()],
            reading_measure: crate::markdown::measure("comfortable"),
            folder_stand_ins: crate::preview::StandIns::of_names(
                crate::settings::FOLDER_PREVIEW_NAMES_DEFAULT,
            ),
            reading_width_before: None,
            zoom_wheel: crate::reader_scroll::WheelTurn::default(),
            tab_wheel: crate::reader_scroll::WheelTurn::default(),
            column_widths: crate::views::list::default_widths(),
            edge_start: None,
            edge_pressed: 0.,
            preview: PreviewState::default(),
            player: crate::player::PlayerState::default(),
            palette: None,
            choose: None,
            tag_editor: None,
            tag_colors: crate::tags::TagColors::default(),
            tag_pins: crate::tags::TagPins::default(),
            ringed: None,
            tag_fields: Vec::new(),
            view_buttons: crate::address::ViewButtons::default(),
            bulk: None,
            prompt: None,
            confirm: None,
            menu: None,
            toolbar_menu: None,
            nav_menu: None,
            nav_asked: 0,
            presses: 0,
            dismissed: None,
            held: None,
            sidebar_drawn: 0.,
            frame_cards: crate::elements::frame::Cards::default(),
            drawn_raised: None,
            strip_holes: crate::elements::ground::Holes::default(),
            sidebar_holes: crate::elements::ground::Holes::default(),
            more_anchor: crate::elements::popover::PopoverAnchor::default(),
            search_menu: None,
            settings: None,
            settings_subscription: None,
            theme_revision: 0,
            reviews: VecDeque::new(),
            review_focus: None,
            map_tiles,
            map_notes: crate::map_preview::Notes::default(),
            map_views: std::collections::HashSet::new(),
            map_spread: std::collections::HashSet::new(),
            map_drawn: std::collections::HashMap::new(),
            terminal: crate::terminal::TerminalState::new(cx.focus_handle()),
            layout: crate::layout::Layout::default(),
            toasts: crate::toast::ToastState::default(),
            activity: crate::activity::ActivityState::default(),
            undo_history: crate::history::HistoryState::default(),
            work,
            left: false,
            window_active: window.is_window_active(),
            pending: Arc::default(),
            shortcuts: None,
            about: None,
            marquee: None,
            secrets: std::collections::VecDeque::new(),
            secret_shown: None,
            form: None,
            forms: std::collections::VecDeque::new(),
            nudges: crate::shortcuts::Nudges::default(),
            last_find: None,
            sequence: None,
            tallies: Vec::new(),
            tallies_revision: None,
            press_collapse: None,
            files_icon: None,
            reveal: None,
            reveal_asked: 0,
            making: None,
            front_waiting: VecDeque::new(),
            links: crate::links::LinkState::default(),
            stored_session: None,
            placement: crate::session::Placement::of(window, Some(&placed), cx),
            placement_wait: None,
            sidebar_scroll: gpui::ScrollHandle::new(),
            sidebar_keys: crate::sidebar::Keys::new(cx.focus_handle()),
            first_frame: Some(first_frame),
            a11y: false,
            draws_meshes: turns_models(window),
            title: gpui::SharedString::default(),
            clipboard_holds: crate::clipboard::Held::Nothing,
            _subscriptions: subs,
        };
        shell.sync_tabs(window, cx);
        // Its window closed while it opened (a script's `window.close` just
        // after its `window.new`): a shell always has a pane to draw
        // (`ui`), no pane of the core's, and the next `follow_windows`
        // takes it down.
        if shell.panes.is_empty() {
            let none = PaneId(u32::MAX);
            shell.pane = none;
            let ui = shell.make_pane_ui(none, window, cx);
            shell.panes.push(ui);
        }
        shell.restore_session(shape, split, window, cx);
        // The tab it opened for shows its files as it did where it was: what
        // a pane carries stands over the shape this window took from there.
        let panes: Vec<PaneId> = shell.panes.iter().map(|ui| ui.id).collect();
        let mut carried = crate::finishing::take_carried(cx, &panes);
        for ui in &mut shell.panes {
            if let Some(carried) = carried.remove(&ui.id) {
                ui.shows_as(carried);
            }
        }
        // The taskbar's button shows the progress of the window used.
        shell.work.borrow().shown_in(Some(window));
        // A thumbnail this window asked for wakes it alone (`leave` forgets).
        shell
            .app
            .thumbnails
            .wake_asker_with(shell.thumb_asker(), Some(shell.waker()));
        shell
    }

    /// Whether the window has the focus.
    pub(crate) fn is_window_active(&self) -> bool {
        self.window_active
    }

    /// This window's active pane, the one with the keys while it has the
    /// focus (ADR-122).
    pub(crate) fn window_pane(&self) -> PaneId {
        self.window_pane_of(&self.app.workspace.read())
    }

    /// `window_pane`, of the workspace `ws` read.
    pub(crate) fn window_pane_of(&self, ws: &skwir_core::workspace::Workspace) -> PaneId {
        ws.window(self.window_id).map_or_else(
            || ws.active_id(),
            skwir_core::workspace::WindowTabs::active_pane,
        )
    }

    /// Who this window is to the thumbnails: its frames keep its own
    /// requests asked, never another window's (ADR-122).
    pub(crate) fn thumb_asker(&self) -> skwir_core::thumbs::Asker {
        skwir_core::thumbs::Asker(u64::from(self.window_id.0) + 1)
    }

    /// What wakes this window alone, for what it shows alone (its preview,
    /// its player, its terminal, its readers): the others do not draw
    /// (ADR-122, `finishing::woken`).
    pub(crate) fn waker(&self) -> skwir_core::location::Waker {
        self.app.window_waker(self.window_id)
    }

    /// Put the window back the way it was left: the same view, the same
    /// split (unless `split` is false: tabs that came back bring their own),
    /// the same panels. The view waits for `sync_views`, which is what
    /// decides which views this build and its plugins actually have.
    fn restore_session(
        &mut self,
        kept: Option<crate::session::Session>,
        split: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Quick filters turned on in Settings, before each pane had its own.
        let former = crate::session::former_quick_filters(&self.app);
        if former.is_some() {
            self.save_settings_later();
        }
        let Some(session) = kept else {
            for pane in &mut self.panes {
                pane.quick_filters_on = former == Some(true);
            }
            self.stored_session = Some(self.session());
            self.keep_shape();
            return;
        };
        if session.zoom > 0 {
            self.zoom = session.zoom;
        }
        if session.row_zoom > 0 {
            self.row_zoom = session.row_zoom;
        }
        if session.levels > 0 {
            self.levels = session.levels.min(crate::usage::MAX_LEVELS);
        }
        self.usage_mode = session.usage_mode;
        self.sidebar_hidden = session.sidebar_hidden;
        self.sidebar_sections = session.sidebar_sections;
        if session.sidebar_width > 0. {
            self.sidebar_width = session.sidebar_width;
        }
        if session.split_share > 0. && session.split_share < 1. {
            self.split_share = session.split_share;
        }
        self.preview.preview_on = session.preview;
        self.preview.properties_on = session.properties_on;
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
        if let Some(share) = dragged(session.properties_height_share) {
            self.preview.properties_share.above = share;
        }
        if let Some(share) = dragged(session.properties_width_share) {
            self.preview.properties_share.beside = share;
        }
        self.preview.properties_folds = session.properties_folds;
        self.preview.source_types = session.source_types;
        self.preview.turned_types = session.turned_types;
        self.preview.folder_way = session.folder_way;
        self.layout.split = session.split_panes;
        if session.player_height > 0. {
            self.player.height = session.player_height;
        }
        // What the panes show above and under their files, as the active
        // one showed it; a pane made later takes the active one's.
        let date = timeline::DATES
            .iter()
            .position(|(word, _)| *word == session.timeline_date)
            .unwrap_or(0);
        for pane in &mut self.panes {
            pane.quick_filters_on = session.quick_filters || former == Some(true);
            pane.timeline.open = session.timeline;
            if session.timeline_size {
                pane.timeline.measure = timeline::Measure::Size;
            }
            pane.timeline.date = date;
        }
        // Its shell starts on a worker at the first sync.
        self.terminal.open = session.terminal && self.app.terminals.provider().is_some();
        if session.terminal_height > 0. {
            self.terminal.height = Some(session.terminal_height);
        }
        for column in &crate::views::list::COLUMNS {
            if let Some(width) = session.column_widths.get(column.key) {
                self.column_widths[column.detail] = crate::views::list::clamp_width(*width);
            }
        }
        // Restored tabs bring their own splits. One made here starts even:
        // the remembered share is put back after it.
        if split
            && session.split
            && !self.is_split()
            && self.app.workspace.read().window(self.window_id).is_some()
        {
            let (split, share) = (self.layout.split, self.split_share);
            self.toggle_split(window, cx);
            self.layout.split = split;
            self.split_share = share;
        }
        // The views this build and its plugins have were instantiated with
        // the panes; a remembered one that is gone leaves the first view.
        for pane in &mut self.panes {
            if let Some(known) = pane.views.keys().find(|k| ***k == *session.view).copied() {
                pane.view_id = known;
            }
        }
        self.stored_session = Some(self.session());
        self.keep_shape();
    }

    /// The window moved or was resized: where it is now, remembered once it
    /// stays half a second, not at each step of a drag.
    fn window_moved(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.placement = crate::session::Placement::of(window, Some(&self.placement), cx);
        self.placement_wait = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(500))
                .await;
            let _ = this.update(cx, |this, cx| {
                this.remember_session();
                // No wake records it: moving a window wakes nothing.
                crate::finishing::record_windows(cx);
            });
        }));
    }

    /// The shape this window starts with, kept with its tabs from the
    /// start (ADR-122): remembered even if it never changes.
    fn keep_shape(&self) {
        if let Some(session) = &self.stored_session
            && let Ok(value) = toml::Value::try_from(session)
        {
            self.app
                .workspace
                .write()
                .set_shape(self.window_id, Some(value));
        }
    }

    pub(crate) fn make_pane_ui(
        &self,
        id: PaneId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> PaneUi {
        let address = cx.new(|cx| {
            let mut input = TextInput::new("Path, query:… or location", cx);
            input.set_label("Address");
            input
        });
        let sub = cx.subscribe_in(
            &address,
            window,
            move |this, _, ev: &InputEvent, window, cx| match ev {
                // What the field does is the omnibox's (`omnibox.rs`).
                InputEvent::Submit => {
                    this.activate_pane(id, cx);
                    this.omnibox_submit(false, false, window, cx);
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
        let hint = cx.subscribe(&address, move |this, _, _: &crate::input::HintTaken, cx| {
            this.activate_pane(id, cx);
            this.omnibox_take_rest(cx);
        });
        // Backspace or Left before the text: the scope's (`omnibox.rs`).
        let start = cx.subscribe_in(
            &address,
            window,
            move |this, _, at: &crate::input::AtStart, window, cx| {
                this.activate_pane(id, cx);
                this.omnibox_at_start(*at, window, cx);
            },
        );
        // The field let go of while typed in (a click elsewhere, Tab, F6):
        // what it shows stays, as Enter keeps it, and nothing waits to
        // show (`omnibox_left`). The window put behind another keeps it.
        let left = {
            let handle = gpui::Focusable::focus_handle(address.read(cx), cx);
            cx.on_blur(&handle, window, move |this, window, cx| {
                if window.is_window_active() {
                    this.omnibox_left(id, cx);
                }
            })
        };
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
            view_id: view::FIRST,
            own_view: None,
            place_view: None,
            own_under_lens: None,
            usage: UsageState::default(),
            loading: crate::loading::State::default(),
            omni: None,
            card_rows: crate::omnibox::Listed::default(),
            weight: crate::weight::Weight::default(),
            quick_filters: crate::quick_filters::QuickFilters::default(),
            quick_filters_on: false,
            timeline: TimelineState::default(),
            chips: crate::search_bar::Chips::default(),
            filters: crate::search_bar::Filters::default(),
            place: crate::trail::Shown::default(),
            trail_shown: std::rc::Rc::default(),
            placeholder_focus: cx.focus_handle(),
            returning: true,
            anchor: None,
            warm: Vec::new(),
            _sub: (sub, hint, start, left),
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

    /// Where the sidebar toggle and More are this frame (`address::corner`),
    /// Split, Properties and Preview in mirror: the sidebar's header while it is
    /// drawn, else the start of the row beside it, the tab strip while it
    /// shows, else the first pane's (left, or top).
    pub fn corner_at(&self) -> CornerAt {
        if self.sidebar_drawn > 0. {
            return CornerAt::Sidebar;
        }
        if self.strip_shown() {
            return CornerAt::Strip;
        }
        self.panes
            .first()
            .map_or(CornerAt::Strip, |p| CornerAt::Pane(p.id))
    }

    /// Where Split, Properties and Preview are this frame, the sidebar's toggle and
    /// More in mirror (`corner_at`): the panel's own bar while it is open at the
    /// right, else the right end of the tab strip while it shows, else the
    /// end of the row of the pane at the window's right (the top one when
    /// the split is stacked).
    pub fn toggles_at(&self) -> TogglesAt {
        if self.preview.panel_open() && !self.layout.bottom() {
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

    /// Whether the tabs are listed: two tabs or more, or one asked for
    /// (`ui.single_tab`). Read from what `sync_tabs` followed, never the
    /// workspace: a row asks this under the workspace's read guard, and a
    /// second read behind a waiting writer would never be granted
    /// (`parking_lot` locks are fair).
    pub fn tabs_listed(&self) -> bool {
        self.prefs.single_tab || self.tab_count > 1
    }

    /// Whether the sidebar lists the tabs, as its first group, when it is
    /// drawn (`ui.tabs_at`).
    pub fn sidebar_lists_tabs(&self) -> bool {
        self.prefs.tabs_in_sidebar && self.tabs_listed()
    }

    /// Whether the tab strip shows: the tabs are listed, and not by the
    /// sidebar drawn this frame. A sidebar put away, or a window too narrow
    /// for it, gives them the strip back, so the tabs are always in sight.
    pub fn strip_shown(&self) -> bool {
        self.tabs_listed() && !(self.sidebar_lists_tabs() && self.sidebar_drawn > 0.)
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

    /// Where the parts of the chrome around the panes (the strip's, the
    /// sidebar's) leave holes in its ground: in the frame as cards, which is
    /// their ground there, else in the area's own (`area`).
    pub(crate) fn chrome_holes(
        &self,
        theme: &Theme,
        area: &crate::elements::ground::Holes,
    ) -> crate::elements::ground::Holes {
        if theme.cards {
            self.frame_cards.holes().clone()
        } else {
            area.clone()
        }
    }

    /// Whether the pane drawn is lowered: as cards, a split's pane commands
    /// do not act on, its grounds a step down (`Theme::files_ground`).
    pub(crate) fn lowered(&self) -> bool {
        self.drawn_raised == Some(false)
    }

    pub fn is_split(&self) -> bool {
        self.panes.len() > 1
    }

    /// The other pane when split.
    pub fn other_pane(&self) -> Option<PaneId> {
        self.panes.iter().map(|p| p.id).find(|id| *id != self.pane)
    }

    /// Run `work` on a scheduler worker in `lane` (it may block: a stat, a
    /// plugin reading files) and hand its result to `then` on the UI
    /// thread. The one way the shell waits for something without blocking
    /// a frame; `then` decides whether the answer is still wanted.
    ///
    /// Work that reads a path runs in that path's lane (`lane_of`,
    /// `Shell::pane_lane`; ADR-077), so a server that stopped answering
    /// holds only its own; `Lane::SHARED` is for work that reads nothing a
    /// remote holds. Every caller names one, so none forgets.
    pub fn off_thread<T: Send + 'static>(
        &self,
        lane: Lane,
        label: &'static str,
        work: impl FnOnce() -> T + Send + 'static,
        then: impl FnOnce(&mut Shell, T, &mut Window, &mut Context<Shell>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let cancel = self.app.lifetime().child();
        self.off_thread_until(lane, label, cancel, work, then, window, cx);
    }

    /// [`Shell::off_thread`], the job stopped by `cancel`: not begun once
    /// it is cancelled, and what it calls asks
    /// `scheduler::job_cancelled` to stop early. Work that ends cancelled
    /// hands `then` nothing: what it read stopping short (an empty folder)
    /// is no answer. Work for a pane is stopped with what it shows
    /// (`Shell::pane_lifetime`).
    #[allow(clippy::too_many_arguments)] // reason: the token beside off_thread's own
    pub fn off_thread_until<T: Send + 'static>(
        &self,
        lane: Lane,
        label: &'static str,
        cancel: skwir_core::CancelToken,
        work: impl FnOnce() -> T + Send + 'static,
        then: impl FnOnce(&mut Shell, T, &mut Window, &mut Context<Shell>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (tx, rx) = flume::bounded(1);
        let stopped = cancel.clone();
        self.app.scheduler.spawn_in(
            lane,
            skwir_core::scheduler::Priority::Visible,
            label,
            cancel,
            move || {
                let answer = work();
                if !stopped.is_cancelled() {
                    let _ = tx.send(answer);
                }
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

    /// What work asked for `pane` ends with: a token that its navigation,
    /// Back and closing cancel (`Pane::location_lifetime`), and the
    /// location it shows now, which an answer compares with what it shows
    /// by then. The app's lifetime for a pane gone.
    pub fn pane_lifetime(&self, pane: PaneId) -> (skwir_core::CancelToken, Option<Location>) {
        let ws = self.app.workspace.read();
        match ws.pane(pane) {
            Some(p) => (p.location_lifetime(), p.location().cloned()),
            None => (self.app.lifetime().child(), None),
        }
    }

    /// The lane of work that reads what `pane` shows: its folder's, else
    /// that of the folder holding its selection (a search, the shelf).
    pub fn pane_lane(&self, pane: PaneId) -> Lane {
        self.app
            .work_dir(pane)
            .map_or(Lane::SHARED, |dir| skwir_core::fs::reach::lane(&dir))
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
    /// opens from gives it its sort), and where its preview shows it.
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
        let from = at.from.clone();
        // Asked for the pane as it is: shown elsewhere since, it is left.
        let (cancel, shown) = self.pane_lifetime(pane);
        self.off_thread_until(
            skwir_core::fs::reach::lane(&target),
            "reveal-stat",
            cancel,
            move || {
                let here = from
                    .as_deref()
                    .is_none_or(|from| skwir_core::fs::volume::Here::now().holds(&target, from));
                let is_dir = here.then(|| std::fs::metadata(&target).map(|m| m.is_dir()));
                (target, is_dir)
            },
            move |this, (path, is_dir), window, cx| {
                if this.reveal_asked != asked || this.app.pane_location(pane) != shown {
                    return;
                }
                let Some(is_dir) = is_dir else {
                    this.set_status(
                        "Skwir follows no link to another machine or a drive not mounted yet",
                        true,
                        cx,
                    );
                    return;
                };
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
                    this.set_status(format!("“{name}” is not there"), true, cx);
                    return;
                };
                let pane = if at.tab {
                    let location = skwir_core::Location::from_path(&folder);
                    match this.app.new_tab(Some(pane), Some(location), true) {
                        Ok((_, opened)) => opened,
                        Err(error) => {
                            this.set_status(format!("{error:#}"), true, cx);
                            return;
                        }
                    }
                } else {
                    this.activate_pane(pane, cx);
                    // A file beside the one shown: selected where it is, the
                    // folder not listed again, a step for Back all the same.
                    let here = this.app.pane_location(pane)
                        == Some(skwir_core::Location::from_path(&folder));
                    if here {
                        if let Err(error) = this.app.step_here(pane) {
                            this.set_status(format!("{error:#}"), true, cx);
                            return;
                        }
                    } else {
                        this.run(
                            "navigate",
                            serde_json::json!({ "location": PathArg::from(folder), "pane": pane }),
                            window,
                            cx,
                        );
                    }
                    pane
                };
                if let (Some(mark), true) = (at.mark.clone(), name.is_some()) {
                    this.wait_for_mark(pane, path.clone(), mark, cx);
                }
                this.reveal = name.map(|name| Reveal::new(pane, vec![name]));
                cx.notify();
            },
            window,
            cx,
        );
        true
    }

    /// Select what `reveal_path` or a plan's end is waiting for, as the
    /// listing has it: those listed so far, then all once all are, unless
    /// something else was selected in between.
    fn take_reveal(&mut self, cx: &mut Context<Self>) {
        let Some(reveal) = self.reveal.as_ref() else {
            return;
        };
        let (found, looked) = {
            let ws = self.app.workspace.read();
            let Some(pane) = ws.pane(reveal.pane) else {
                return;
            };
            if reveal
                .selected
                .is_some_and(|ours| ours != pane.selection.revision())
            {
                self.reveal = None;
                return;
            }
            // A folder held back whole (ADR-120) shows nothing to scroll
            // to yet: looked at once it shows what it holds.
            if pane.listing.is_held() {
                return;
            }
            let names = (pane.listing.id(), pane.listing.names_version());
            let found = (reveal.looked != Some(names)).then(|| {
                // The listing walked once, however many are looked for.
                let wanted: std::collections::HashSet<&std::ffi::OsStr> = reveal
                    .names
                    .iter()
                    .map(std::ffi::OsString::as_os_str)
                    .collect();
                let listed: HashMap<&std::ffi::OsStr, (skwir_core::EntryId, bool)> = pane
                    .listing
                    .iter()
                    .filter(|entry| wanted.contains(entry.name.as_os_str()))
                    .map(|entry| (entry.name.as_os_str(), (entry.id, entry.is_hidden())))
                    .collect();
                let hidden = reveal
                    .names
                    .iter()
                    .find(|name| {
                        listed
                            .get(name.as_os_str())
                            .is_some_and(|(_, hidden)| *hidden)
                    })
                    .filter(|_| !pane.sort.show_hidden)
                    .map(|name| name.to_string_lossy().into_owned());
                let ids: Vec<skwir_core::EntryId> = reveal
                    .names
                    .iter()
                    .filter_map(|name| listed.get(name.as_os_str()).map(|(id, _)| *id))
                    .collect();
                (ids, hidden)
            });
            (found, names)
        };
        let Some((ids, hidden)) = found.filter(|(ids, _)| !ids.is_empty()) else {
            if reveal.until < Instant::now() {
                self.reveal = None;
            } else if let Some(reveal) = self.reveal.as_mut() {
                reveal.looked = Some(looked);
            }
            return;
        };
        let pane = reveal.pane;
        let whole = ids.len() >= reveal.names.len();
        let selected = {
            let mut ws = self.app.workspace.write();
            ws.pane_mut(pane).map(|pane| {
                pane.select_revealed(&ids);
                pane.selection.revision()
            })
        };
        match self.reveal.as_mut() {
            Some(reveal) if !whole && reveal.until >= Instant::now() => {
                reveal.looked = Some(looked);
                reveal.selected = selected;
            }
            _ => self.reveal = None,
        }
        if let Some(name) = hidden {
            self.set_status(
                format!("Hidden files are shown, “{name}” among them"),
                false,
                cx,
            );
        }
        self.with_pane(pane, |shell| {
            if let Some(index) = shell.cursor_index() {
                shell.with_view(|view, _| view.scroll_to(index));
            }
        });
        cx.notify();
    }

    /// `plan`, asked by the user with `args`, and the folders the panes
    /// shown show: what it puts in one is selected there once done. Its
    /// steps are not read here: where they put what they do is told by
    /// the executor, from its thread (`made_step`, `made`).
    fn making(&self, plan: skwir_core::op::PlanId, args: &serde_json::Value) -> Option<Making> {
        let asked = args
            .get("pane")
            .and_then(|pane| serde_json::from_value::<PaneId>(pane.clone()).ok());
        let asked = self.app.pane_or_active(asked);
        let ws = self.app.workspace.read();
        let shown: Vec<Shown> = self
            .panes
            .iter()
            .filter_map(|ui| {
                let pane = ws.pane(ui.id)?;
                let location = pane.location()?;
                Some(Shown {
                    pane: ui.id,
                    folder: location.path()?,
                    location: location.clone(),
                    selection: pane.selection.revision(),
                })
            })
            .collect();
        (!shown.is_empty()).then_some(Making {
            plan,
            asked,
            shown,
            placed: Vec::new(),
        })
    }

    /// A step of the plan asked last ended (`ExecEvent::Step`), as it was
    /// applied: where it put something, in a folder shown, is kept for its
    /// end (`Change::placed`, a name taken settled).
    pub(crate) fn made_step(&mut self, progress: &skwir_core::op::Progress) {
        let Some(making) = self.making.as_mut().filter(|m| m.plan == progress.plan) else {
            return;
        };
        if progress.error.is_none()
            && let Some(path) = progress.change.placed()
            && making
                .shown
                .iter()
                .any(|shown| path.parent() == Some(shown.folder.as_path()))
        {
            making.placed.push(path.to_path_buf());
        }
    }

    /// The plan `plan` ended, `done` when nothing stopped it, `items` what
    /// it says it did item by item where its steps are not its items (a
    /// folder merged into, a name taken settled): what it put in a folder
    /// a pane shows is selected there once the listing has it
    /// (`take_reveal`), in the pane it was asked from first, if that pane
    /// still shows the folder and nothing else was selected since. A
    /// folder of the name asked that was there already is selected too.
    pub(crate) fn made(
        &mut self,
        plan: skwir_core::op::PlanId,
        done: bool,
        items: Option<&skwir_core::op::Items>,
    ) {
        let Some(making) = self.making.take_if(|m| m.plan == plan) else {
            return;
        };
        if !done {
            return;
        }
        let placed = items.map_or(&making.placed, |items| &items.placed);
        let pane = {
            let ws = self.app.workspace.read();
            let still = |shown: &&Shown| {
                ws.pane(shown.pane).is_some_and(|p| {
                    p.location() == Some(&shown.location)
                        && p.selection.revision() == shown.selection
                })
            };
            let holds = |shown: &&Shown| {
                placed
                    .iter()
                    .any(|path| path.parent() == Some(shown.folder.as_path()))
            };
            let candidates: Vec<&Shown> = making.shown.iter().filter(still).filter(holds).collect();
            candidates
                .iter()
                .find(|shown| shown.pane == making.asked)
                .or(candidates.first())
                .copied()
        };
        let Some(shown) = pane else {
            return;
        };
        let names: Vec<std::ffi::OsString> = placed
            .iter()
            .filter(|path| path.parent() == Some(shown.folder.as_path()))
            .filter_map(|path| path.file_name().map(ToOwned::to_owned))
            .collect();
        self.reveal = Some(Reveal::new(shown.pane, names));
    }

    /// Make `id` the active pane (workspace and shell agree).
    pub fn activate_pane(&mut self, id: PaneId, cx: &mut Context<Self>) {
        if self.window_pane() == id && self.pane == id {
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
        let pane = pane.unwrap_or_else(|| self.window_pane());
        if self.app.workspace.read().pane(pane).is_none() {
            return;
        }
        self.activate_pane(pane, cx);
        self.run(command, serde_json::json!({ "pane": pane }), window, cx);
    }

    /// Split the tab shown, or close its split, arranged as `layout` says.
    /// The workspace changes; `sync_tabs` makes and releases the panes' UI.
    pub(crate) fn toggle_split(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The popup open closes, as a click on Split closes it: F3 or a
        // script would otherwise leave a pane's menu after its pane.
        self.close_popup(window, cx);
        if self.is_split() {
            // An arrangement made for this split ends with it.
            self.layout.split = None;
            // The pane the preferences name; the one left becomes active.
            if let Some(pane) = self.split_closes() {
                self.app.workspace.write().close_pane(pane);
            }
            self.sync_tabs(window, cx);
            window.focus(&self.ui().focus, cx);
            self.remember_session();
            cx.notify();
            return;
        }
        let here = {
            let ws = self.app.workspace.read();
            ws.pane(self.pane).and_then(|p| p.location().cloned())
        };
        // A split shows the same folder, in the same view, through the
        // same lens (ADR-107): showing it otherwise would be a surprise.
        let id = self.app.workspace.write().add_pane_from(self.pane);
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
            window.focus(&self.ui().focus, cx);
        }
    }

    /// `ui.focus_next_part` (F6): the keyboard goes round the parts of the
    /// window shown, as F6 does in other applications: the sidebar, the
    /// files, the preview (what it shows takes the keys, `shown_focus`). A
    /// part not shown is passed; the files, the address field theirs, are
    /// always there. The terminal is not one of them: its programs have F6
    /// (Midnight Commander's Move), and F4 takes the keyboard in and out.
    fn focus_next_part(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let sidebar = self
            .sidebar_shown(window)
            .then(|| self.sidebar_keys.focus.clone());
        let preview = self
            .preview
            .preview_on
            .then(|| self.shown_focus())
            .flatten();
        let parts = [
            sidebar,
            Some(self.ui().focus.clone()),
            self.timeline_focus(),
            preview,
        ];
        let here = parts
            .iter()
            .position(|part| {
                part.as_ref()
                    .is_some_and(|f| f.contains_focused(window, cx))
            })
            .unwrap_or(1);
        let next = (1..=parts.len())
            .map(|step| (here + step) % parts.len())
            .find(|at| parts[*at].is_some())
            .unwrap_or(1);
        if next == 0 {
            self.sidebar_keys.arrive();
        }
        if let Some(focus) = &parts[next] {
            window.focus(focus, cx);
        }
        cx.notify();
    }

    /// Directory shown by the other pane, for Shift+F5/F6 and the shelf buttons.
    /// Shift+F5 and Shift+F6: the selection given to the other pane, as a
    /// drag onto it would be (`give_to_pane`), copied or moved as the key
    /// says.
    fn send_to_other_pane(&mut self, cmd: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(other) = self.other_pane() else {
            self.set_status("There is no other pane: split the window first", false, cx);
            return;
        };
        let how = if cmd == "move" {
            crate::dnd::DropOp::Move
        } else {
            crate::dnd::DropOp::Copy
        };
        self.give_to_pane(self.pane, other, Some(how), window, cx);
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
    pub fn available_views(&self) -> Vec<skwir_core::registry::ViewSpec> {
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
        keymap::keys_for(self.key_hints.of(command), command, args)
    }

    /// The keys that run `command` in the pane being rendered, as it is
    /// now: a binding whose condition does not hold there is not named.
    pub fn keys_here(&self, command: &str, args: &serde_json::Value) -> Option<String> {
        let ctx = self.selection_ctx();
        keymap::keys_for_where(self.key_hints.of(command), command, args, |flag| {
            crate::menu::SELECTION_TOKENS
                .contains(&flag)
                .then(|| ctx.holds(flag))
        })
    }

    /// Install the key bindings again when a plugin added or withdrew some.
    fn sync_keymap(&mut self, cx: &mut Context<Self>) {
        let revisions = keymap::Stamp::revisions(&self.app);
        let settings = self.app.settings.revision();
        if revisions == self.keys.stamp.revisions && settings == self.keys.settings {
            return;
        }
        self.keys.settings = settings;
        let user = skwir_core::keymap::user_bindings(&self.app.settings);
        if revisions == self.keys.stamp.revisions && user == self.keys.stamp.user {
            return;
        }
        let keymap = keymap::install(&self.app, user, cx);
        self.key_hints = keymap::Hints::new(keymap.hints);
        self.keys.stamp = keymap.stamp;
        self.keys.bound = Arc::new(keymap.bound);
        self.keys.typeahead = keymap.typeahead;
        if let Some(settings) = &self.settings {
            let bound = Arc::clone(&self.keys.bound);
            settings.update(cx, |settings, cx| settings.set_bound(bound, cx));
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
            if replaced.iter().any(|id| id == view::USAGE) {
                pane.usage = UsageState::default();
            }
            if !pane.views.contains_key(pane.view_id) {
                pane.view_id = view::FIRST;
            }
            if pane
                .own_view
                .is_some_and(|own| !pane.views.contains_key(own))
            {
                pane.own_view = Some(view::FIRST);
            }
            if pane
                .own_under_lens
                .is_some_and(|own| !pane.views.contains_key(own))
            {
                pane.own_under_lens = Some(view::FIRST);
            }
        }
    }

    /// Show each pane's folder in the view it remembers (ADR-132), where
    /// the pane's own is a view a folder may remember too, the pane's own
    /// set aside; its own again once it shows a place that remembers none.
    /// A pane in the columns, the tree or a lens stays there.
    fn sync_place_views(&mut self) {
        let wanted: Vec<Option<std::sync::Arc<str>>> = {
            let ws = self.app.workspace.read();
            self.panes
                .iter()
                .map(|ui| ws.pane(ui.id).and_then(|p| p.place_view().cloned()))
                .collect()
        };
        let app = &self.app;
        for (ui, wanted) in self.panes.iter_mut().zip(wanted) {
            if ui.place_view == wanted {
                continue;
            }
            let own = ui.own_view();
            let shown = wanted
                .as_deref()
                .and_then(|view| ui.views.keys().find(|k| **k == view).copied())
                .filter(|_| app.remembered_view(own));
            match shown {
                Some(view) => {
                    ui.own_view = Some(own);
                    ui.view_id = view;
                }
                None => {
                    if let Some(own) = ui.own_view.take() {
                        ui.view_id = own;
                    }
                }
            }
            ui.place_view = wanted;
        }
    }

    pub(crate) fn set_view(&mut self, id: &str, cx: &mut Context<Self>) {
        self.sync_views();
        let known = self.ui().views.keys().find(|k| **k == id).copied();
        match known {
            Some(k) => {
                // A lens (Usage, the map) is shown over the pane's own view,
                // never in its place: the list or grid left for it is still
                // the pane's when one comes back (ADR-132).
                let lens = self
                    .app
                    .registry
                    .views
                    .find(|s| &*s.id == k)
                    .is_some_and(|s| s.role == skwir_core::registry::ViewRole::Lens);
                // Where the folder shown remembers its order, a view a
                // folder may remember is that folder's (ADR-132); any other
                // is the pane's own.
                let pane = self.pane;
                let folder_view = self.app.remembered_view(k)
                    && self
                        .app
                        .workspace
                        .read()
                        .pane(pane)
                        .is_some_and(skwir_core::workspace::Pane::keeps_order);
                let ui = self.ui();
                let views = Views {
                    shown: ui.view_id,
                    own: ui.own_view,
                    under_lens: ui.own_under_lens,
                };
                let chosen =
                    views.choose(k, lens, folder_view, |view| self.app.remembered_view(view));
                let ui = self.ui_mut();
                ui.view_id = chosen.shown;
                ui.own_view = chosen.own;
                ui.own_under_lens = chosen.under_lens;
                if folder_view && !lens {
                    ui.place_view = Some(k.into());
                    self.app.remember_view(pane, k);
                }
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

    /// `ui.view_button { id, on }`: give the view a button beside the
    /// address (`on`), take it away, or, without `on`, the other.
    fn set_view_button(&mut self, id: &str, on: Option<bool>, cx: &mut Context<Self>) {
        let Some(spec) = self.available_views().into_iter().find(|s| &*s.id == id) else {
            self.set_status(format!("unknown view `{id}`"), true, cx);
            return;
        };
        let on = on.unwrap_or(!self.view_buttons.pinned(&spec));
        let chosen = self.view_buttons.clone().with(&spec, on);
        self.save_view_buttons(chosen, cx);
    }

    /// What the user made, kept with their data (ADR-057) as pinned tags
    /// are, on a worker.
    fn save_view_buttons(&mut self, chosen: crate::address::ViewButtons, cx: &mut Context<Self>) {
        if chosen == self.view_buttons {
            return;
        }
        self.app
            .settings
            .set(crate::settings::VIEW_BUTTONS_KEY, chosen.value());
        self.view_buttons = chosen;
        self.save_settings_later();
        cx.notify();
    }

    /// `ui.grid_fill`: a preference, saved as the dialog's switch is
    /// (`settings::remember`: one save at a time, the next waiting).
    fn toggle_grid_fill(&mut self, cx: &mut Context<Self>) {
        self.prefs.grid_fill = !self.prefs.grid_fill;
        cx.notify();
        let on = self.prefs.grid_fill.to_string();
        crate::settings::remember(self, crate::settings::GRID_FILL_KEY, on, cx);
    }

    /// `ui.quick_filters`: Quick filters above the pane's files, or not,
    /// as it shows its view: the pane's own, remembered with the window's
    /// shape (`session.rs`).
    fn toggle_quick_filters(&mut self, cx: &mut Context<Self>) {
        let ui = self.ui_mut();
        ui.quick_filters_on = !ui.quick_filters_on;
        self.remember_session();
        cx.notify();
    }

    /// `ui.printed_pages`: count a book by chapters or by printed pages. A
    /// preference, saved as the dialog's switch is; the unit picker closes
    /// on its choice.
    fn set_printed_pages(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(popup) = self.preview.menu.take_if(|popup| popup.kind == "unit") {
            popup.restore(window, cx);
        }
        self.prefs.printed_pages = on;
        cx.notify();
        crate::settings::remember(self, crate::settings::PRINTED_PAGES_KEY, on.to_string(), cx);
    }

    /// `ui.wrap_lines`: a text's long lines wrap to the panel, or go on
    /// sideways. A preference, saved as the dialog's switch is.
    fn set_wrap_lines(&mut self, on: bool, cx: &mut Context<Self>) {
        self.prefs.wrap_lines = on;
        cx.notify();
        crate::settings::remember(self, crate::settings::WRAP_LINES_KEY, on.to_string(), cx);
    }

    /// `ui.text_continuous`: what is read scrolls as one column, or turns
    /// a screen at a time; what is shown changes at once, from the line at
    /// its top. A preference, saved as the dialog's switch is.
    fn set_text_continuous(&mut self, on: bool, cx: &mut Context<Self>) {
        self.prefs.text_continuous = on;
        self.page_text();
        cx.notify();
        crate::settings::remember(
            self,
            crate::settings::TEXT_CONTINUOUS_KEY,
            on.to_string(),
            cx,
        );
    }

    /// `ui.contents_pin`: the contents stay open beside what is read, or
    /// close once an entry is chosen. A preference, saved as the dialog's
    /// switch is.
    fn set_contents_pinned(&mut self, on: bool, cx: &mut Context<Self>) {
        self.prefs.contents_pinned = on;
        cx.notify();
        crate::settings::remember(
            self,
            crate::settings::CONTENTS_PINNED_KEY,
            on.to_string(),
            cx,
        );
    }

    /// `ui.model_perspective`, `ui.model_smooth`: how a 3D model is drawn.
    /// Preferences, saved together as the dialog's switches are.
    fn set_model_look(&mut self, look: crate::elements::model::Look, cx: &mut Context<Self>) {
        self.prefs.model_look = look;
        cx.notify();
        self.unsaved.insert(
            crate::settings::MODEL_PERSPECTIVE_KEY.to_owned(),
            look.perspective.to_string(),
        );
        crate::settings::remember(
            self,
            crate::settings::MODEL_SMOOTH_KEY,
            look.smooth.to_string(),
            cx,
        );
    }

    /// `ui.full_width`: the reading column takes the whole panel, or goes
    /// back to the width it had. The Reading width preference, saved as
    /// the choice in the dialog is.
    fn toggle_full_width(&mut self, cx: &mut Context<Self>) {
        let current = crate::settings::preference(self, crate::settings::READING_WIDTH_KEY)
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
        crate::settings::remember(self, crate::settings::READING_WIDTH_KEY, next, cx);
    }

    /// Ctrl and the wheel over what zooms (`elements::zoom_wheel`): the step
    /// its zoom keys take. A view stays where it is (it keeps its first
    /// visible item); only the keys follow the cursor.
    pub fn zoom_wheel(
        &mut self,
        delta: f32,
        lines: bool,
        zoomed: crate::elements::zoom_wheel::Zoomed,
        pointer: gpui::Point<gpui::Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::elements::zoom_wheel::Zoomed;
        let larger = match self.zoom_wheel.turn(delta, lines, false) {
            0 => return,
            turn => turn < 0,
        };
        let dir = if larger { "in" } else { "out" };
        match zoomed {
            Zoomed::View(pane) => {
                self.with_pane(pane, |shell| shell.zoom_to(dir, cx));
            }
            Zoomed::Preview => self.preview_zoom_toward(dir, pointer, window, cx),
            Zoomed::Text { parked } => self.inspect_text_step(larger, parked, window, cx),
        }
    }

    /// Zoom what the pane's view sizes (`PaneView::zooms`): the grid's
    /// tiles, the rows the list, the tree and the columns share, Usage's
    /// levels, or the view's own scale. Returns whether something changed.
    fn zoom_to(&mut self, dir: &str, cx: &mut Context<Self>) -> bool {
        fn step(levels: &[u32], current: u32, default: u32, dir: &str) -> u32 {
            let at = |zoom| levels.iter().position(|z| *z == zoom);
            let pos = at(current).or_else(|| at(default)).unwrap_or(0);
            match dir {
                "in" => levels[(pos + 1).min(levels.len() - 1)],
                "out" => levels[pos.saturating_sub(1)],
                _ => default,
            }
        }
        use crate::usage::DEFAULT_LEVELS;
        use crate::views::list::{DEFAULT_ROW_ZOOM, ROW_ZOOMS};
        match self.view_zooms() {
            view::Zooms::Tiles => self.zoom = step(&ZOOMS, self.zoom, DEFAULT_ZOOM, dir),
            view::Zooms::Rows => {
                self.row_zoom = step(&ROW_ZOOMS, self.row_zoom, DEFAULT_ROW_ZOOM, dir);
            }
            view::Zooms::Levels => {
                self.levels = step(&crate::usage::LEVELS, self.levels, DEFAULT_LEVELS, dir);
            }
            view::Zooms::Own => {
                self.with_view(|v, _| v.zoom(dir));
                cx.notify();
                return false;
            }
            view::Zooms::Nothing => return false,
        }
        self.remember_session();
        cx.notify();
        true
    }

    /// What the active pane's view knows its entries weigh, folders
    /// included (`PaneView::weighs`: Usage).
    pub fn view_weighs(&self) -> Option<view::Weighs> {
        let ui = self.ui();
        ui.views.get(ui.view_id).and_then(|v| v.weighs(self))
    }

    /// What the active pane's view zooms.
    pub fn view_zooms(&self) -> view::Zooms {
        let ui = self.ui();
        ui.views
            .get(ui.view_id)
            .map_or(view::Zooms::Nothing, |v| v.zooms())
    }

    /// `ui.text_size`: a part's text a step larger or smaller, or back at
    /// 100 % (ADR-117), saved as its setting is.
    fn text_size_step(&mut self, part: crate::sizes::Part, dir: &str, cx: &mut Context<Self>) {
        let now = crate::sizes::step_of(crate::settings::preference(self, part.key()).as_deref());
        let next = crate::sizes::stepped(now, dir);
        if next != now {
            crate::settings::remember(self, part.key(), next.to_string(), cx);
        }
        let label = crate::sizes::step_label(next);
        self.set_status(format!("{}: {label}", part.title()), false, cx);
        cx.notify();
    }

    fn zoom_step(&mut self, dir: &str, cx: &mut Context<Self>) {
        if !self.zoom_to(dir, cx) {
            return;
        }
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

    /// A key's command, before any part of the window takes it (ADR-066):
    /// a key held down runs again only what steps (`CommandSpec::repeats`),
    /// so a switch bound to a key is not flipped over and over; its repeats
    /// tell peek that the key which opened it is held.
    fn capture_run_command(&mut self, a: &RunCommand, window: &mut Window, cx: &mut Context<Self>) {
        let Some(key) = window.key_down_event() else {
            return;
        };
        if key.is_held {
            if let Some(press) = &mut self.preview.peek_press {
                press.repeated(&key.keystroke.key);
            }
            if !self.app.commands.repeats(&a.name, &a.args) {
                cx.stop_propagation();
            }
        } else if a.name != "ui.cursor"
            && let Some(press) = &mut self.preview.peek_press
        {
            // Peek used while its key is held, beyond walking the files.
            press.used();
        }
    }

    /// Whether `name`, which the user asked in this window (a key, a click,
    /// a menu), runs now (`admit_here`). The one gate for what the user
    /// does in a window: it acts in this window. A command that names no
    /// pane acts in the core's active window (ADR-122), which an agent's
    /// or a script's `window.select`, `tab.move` or `window.new` may have
    /// made another without the focus moving; so this window, the one with
    /// the keyboard, is made active first. What is asked from elsewhere
    /// (`sync`) is not, nor what runs here with no one at this window (a
    /// track that ended in one behind another): they leave the active
    /// window as it is.
    fn admit(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.follow_tabs(window, cx);
        if !self.admit_here(name, window, cx) {
            return false;
        }
        if window.is_window_active() {
            self.app.workspace.write().set_active_window(self.window_id);
            crate::finishing::raised(cx, self.window_id);
        }
        true
    }

    /// Follow the workspace's tabs before the user's command acts, when
    /// they moved since the last sync: a command just before it (Ctrl+W,
    /// a script's `tab.close`) may have closed the pane this window last
    /// followed, or shown another tab, and a split, a view or a sort asked
    /// of a pane gone acts on nothing. The pane a caller chose
    /// (`with_pane`) stays chosen while the tab shown holds it.
    fn follow_tabs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.app.workspace.read().revision() == self.tabs_revision {
            return;
        }
        let chosen = self.pane;
        self.sync_tabs(window, cx);
        if self.panes.iter().any(|ui| ui.id == chosen) {
            self.pane = chosen;
        }
    }

    /// Whether `name` runs now in this window. When the dialog open takes
    /// it instead, Settings keeps what it is not asked, and the overview
    /// and About close. What the user does acts on the tab shown: it is
    /// drawn first. Moving between tabs keeps holding, so cycling through
    /// them draws only the one it stops on.
    fn admit_here(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.takes_now(name) {
            if self.settings.is_none() && self.shortcuts.is_some() {
                self.toggle_shortcuts(window, cx);
            } else if self.settings.is_none() && self.about.is_some() {
                self.toggle_about(window, cx);
            }
            return false;
        }
        if self.presented.is_some() && !name.starts_with("tab.") {
            self.present(false, window, cx);
        }
        // What a command acts on is the folder gone to: the files of the
        // one left, drawn while it is filled, go at the next sync.
        for ui in &mut self.panes {
            ui.list.end_hold();
        }
        true
    }

    /// `run` for a core command the views send by the thousand, with its
    /// arguments typed (`App::dispatch_args`: a band's `select`), never
    /// made into JSON. Such a command answers `Done`; one refused says why,
    /// as `run` says it.
    pub fn run_args<A: serde::Serialize + Send + 'static>(
        &mut self,
        name: &str,
        args: A,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.admit(name, window, cx) {
            return;
        }
        tracing::debug!(name, "run");
        match self.app.dispatch_args(name, args) {
            Ok(Outcome::Done | Outcome::Value(_)) => {}
            Ok(_) => tracing::warn!(name, "an answer only `run` follows"),
            Err(e) => self.report_error(&e, cx),
        }
        cx.notify();
    }

    /// Route a command: `ui.*` to the shell, everything else to the core.
    pub fn run(
        &mut self,
        name: &str,
        args: serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.admit(name, window, cx) {
            return;
        }
        self.carry_out(name, args, window, cx);
    }

    /// Carry out a command `admit` or `admit_here` let through: `ui.*` the
    /// shell's, everything else the core's.
    fn carry_out(
        &mut self,
        name: &str,
        args: serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Settings may contain private paths; never trace their values.
        if !name.starts_with("ui.settings") && !name.starts_with("settings.") {
            tracing::debug!(name, %args, "run");
        }
        // A setting the window changes (a menu's switch, a background
        // chosen) waits behind a save that runs, never refused by it
        // (`settings::remember`); the palette's too.
        if name == "settings.apply"
            && let Some(changes) = args["changes"].as_object()
            && !changes.is_empty()
            && changes.values().all(serde_json::Value::is_string)
        {
            for (key, value) in changes {
                let value = value.as_str().unwrap_or_default().to_owned();
                self.unsaved.insert(key.clone(), value);
            }
            return crate::settings::flush_remembered(self, cx);
        }
        if let Some(ui) = name.strip_prefix("ui.") {
            // Declared as every command is (`commands.rs`): a name or
            // arguments it does not know are refused, never guessed.
            let checked = self.app.commands.check(&Invocation {
                name: name.into(),
                args: args.clone(),
            });
            if let Err(error) = checked {
                self.set_status(error.to_string(), true, cx);
                return;
            }
            self.run_ui(ui, &args, window, cx);
            cx.notify();
            return;
        }
        // The last tab of the last window, closed from the window (Ctrl+W,
        // the palette, a script asking the window), closes the window, as
        // its close button does (ADR-122): the core keeps a last tab, which
        // only the window lets go of (ADR-115).
        if name == "tab.close" && self.closes_last_window(&args) {
            self.close_window(window, cx);
            window.remove_window();
            return;
        }
        if name == "open" && self.app.commands.get("open").is_none() {
            self.open_fallback(window, cx);
            return;
        }
        if name == "refresh" {
            self.with_view(|view, shell| view.refresh(shell, cx));
            self.reader_retry_pages();
        }
        let inv = Invocation {
            name: name.into(),
            args,
        };
        match self.app.dispatch(&inv) {
            Ok(Outcome::Done | Outcome::Value(_)) => {}
            Ok(Outcome::Clipboard { text, said }) => {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                self.set_status(said, false, cx);
            }
            Ok(Outcome::Files { paths, cut }) => self.put_files_on_clipboard(paths, cut, cx),
            Ok(Outcome::Plan(plan)) => {
                self.making = self.making(plan.id, &inv.args);
                crate::finishing::ran(cx, plan.id, self.window_id);
                self.apply_user_plan(plan, window, cx);
            }
            Err(e) if skwir_core::command::wants_clipboard(&e) => {
                self.run_with_clipboard(inv, window, cx);
            }
            Err(e) => self.report_error(&e, cx),
        }
        cx.notify();
    }

    /// Whether `name` runs now, rather than the dialog open taking it:
    /// Settings runs only its own and Up and Down in a list of its (a
    /// long list of choices: the one command every list one picks from
    /// shares); the F1 overview and About, something to read, close
    /// instead of acting unseen. One rule for keys and for what is asked
    /// from elsewhere, which waits for the dialog to close.
    fn takes_now(&self, name: &str) -> bool {
        if self.settings.is_some() {
            // A menu the dialog opened (a Exclusions page row's) is walked by
            // the menu's keys, above it.
            name.starts_with("ui.settings")
                || name == "ui.list_move"
                || (self.menu.is_some() && name.starts_with("ui.menu_"))
        } else if self.shortcuts.is_some() {
            matches!(name, "ui.shortcuts" | "ui.settings")
        } else if self.about.is_some() {
            name == "ui.about"
        } else {
            true
        }
    }

    /// A switch as it stands: the change waiting to be saved, else the
    /// stored one (`settings::preference`), else `default`.
    fn switch(&self, key: &str, default: bool) -> bool {
        crate::settings::preference(self, key)
            .and_then(|text| text.parse().ok())
            .unwrap_or(default)
    }

    /// A user-initiated plan applies immediately (it is undoable), as a
    /// whole: the executor checks every step first, on its lane, and a
    /// conflict applies nothing and comes back as the toast's error. One
    /// that Undo could not take back (deleting permanently, from anywhere:
    /// Shift+Delete, the trash's Delete, Empty Trash) asks first.
    pub fn apply_user_plan(&mut self, plan: Plan, window: &mut Window, cx: &mut Context<Self>) {
        if !plan.undoable() && !plan.is_empty() {
            // What it deletes is listed under the question (`deleting.rs`).
            self.confirm_deleting(plan, window, cx);
            return;
        }
        self.app.apply_whole(plan);
        cx.notify();
    }

    /// The panes whose listings the last sync's pump moved, taken.
    pub(crate) fn take_pumped(&mut self) -> Vec<skwir_core::workspace::PaneId> {
        std::mem::take(&mut self.pumped)
    }

    /// Quit (ADR-115). While something runs or waits its turn, ask: Skwir
    /// finishes it without its window (the answer Enter gives, and Quit
    /// again), or stops now. With nothing under way, quit at once: an
    /// action an error stopped, still shown in the strip, is let go.
    pub fn quit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .confirm
            .as_ref()
            .is_some_and(|c| matches!(c.then, Confirmed::Quit))
        {
            self.confirm_accept(window, cx);
        } else if crate::finishing::under_way(&self.app) {
            let question = self.quit_question();
            self.open_confirm(question, Confirmed::Quit, window, cx);
        } else {
            self.stop_and_quit(window, cx);
        }
    }

    /// What Quit asks, of what is under way: a plan first, else a task (a
    /// print being sent), else a plan that waits for its server to go on
    /// by itself (ADR-080, 9).
    fn quit_question(&self) -> String {
        let work = self.work.borrow();
        let tasks = self.app.tasks.running();
        let running = work
            .running
            .first()
            .map(|running| {
                let paused = self.app.executor.is_paused(running.plan);
                (running.title.as_str(), paused)
            })
            .or_else(|| tasks.first().map(|task| (task.title.as_str(), false)));
        let waiting = self.app.executor.queued();
        if running.is_none() && waiting == 0 && work.waits_for_server() {
            // Nothing runs nor waits its turn: what keeps it waits for
            // its server, the one the strip says stopped when it is that.
            let what = work
                .stopped
                .as_ref()
                .filter(|stopped| self.app.executor.waits_for_server(stopped.plan))
                .map_or_else(
                    || "An action".to_owned(),
                    |stopped| crate::toast::capitalized(&stopped.title),
                );
            return format!(
                "{what} waits for its server to answer again, to go on by itself. \
                 Skwir can wait for it without its window, then quit. Or stop now: it \
                 does not go on, and what it finished stays."
            );
        }
        let others = (work.running.len() + tasks.len()).saturating_sub(1);
        crate::toast::quit_question(running, others, waiting)
    }

    /// Stop what runs and quit: the window closes at once, and the program
    /// ends once its plans took back what they were in the middle of
    /// (`lib.rs`, `wind_down`).
    pub(crate) fn stop_and_quit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        crate::finishing::stopping(cx);
        self.close_every_window(window, cx);
    }

    /// The window is about to close (its close button, Quit): what it asks
    /// and was not answered goes to the next window up, or, none being up,
    /// is answered Cancel, as its own Close would (`take_held`,
    /// `finishing::hand_on`); from now on the shell takes nothing (`sync`),
    /// the program taking in what the core sends and asks, which finishes
    /// without the window (`finishing.rs`) or ends. The window goes later
    /// (gpui removes it in a task of its own), and what is asked meanwhile
    /// waits for the next window rather than going with this one.
    pub(crate) fn leave(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let _ = window;
        if self.left {
            return;
        }
        let held = self.take_held();
        self.left = true;
        // A window closed with the keyboard may not be told it lost it.
        self.app.set_window_focused(self.window_id, false);
        // What it played stops with it (ADR-122), and the desktop's
        // controls let go of it.
        self.let_go_of_player();
        self.app.thumbnails.forget_asker(self.thumb_asker());
        crate::finishing::left(cx, self.window_id);
        crate::finishing::hand_on(cx, held);
        if crate::finishing::window_count(cx) == 0 {
            self.work.borrow().shown_in(None);
        }
    }

    /// The window's close button, or Ctrl+Shift+W: with other windows up,
    /// this one and its tabs go, Reopen closed tab bringing them back
    /// (ADR-122); the last one leaves as ADR-115 says, what runs going on
    /// without it.
    pub(crate) fn close_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if crate::finishing::window_count(cx) > 1
            && let Err(error) = self.app.close_window(self.window_id)
        {
            tracing::warn!("a window closes: {error:#}");
        }
        self.leave(window, cx);
    }

    /// Whether `tab.close` with `args` closes the last tab of the last
    /// window: the tab named, else this window's shown one.
    fn closes_last_window(&self, args: &serde_json::Value) -> bool {
        let ws = self.app.workspace.read();
        if ws.windows().len() != 1 {
            return false;
        }
        let tab = match args.get("tab").filter(|tab| !tab.is_null()) {
            Some(tab) => serde_json::from_value::<skwir_core::workspace::TabId>(tab.clone()).ok(),
            None => ws.window(self.window_id).map(|w| w.shown().id),
        };
        tab.and_then(|tab| ws.window_of_tab(tab))
            .and_then(|w| ws.window(w))
            .is_some_and(|w| w.tabs().len() == 1)
    }

    /// Quit's windows: every one goes, this one first, none forgotten by
    /// what is remembered for the next start (`finishing::closed`).
    pub(crate) fn close_every_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let others = crate::finishing::others(cx, self.window_id);
        self.leave(window, cx);
        window.remove_window();
        for other in others {
            cx.defer(move |cx| {
                let _ = other.update(cx, |shell, window, cx| {
                    shell.leave(window, cx);
                    window.remove_window();
                });
            });
        }
    }

    /// All the window took to ask and was not answered, shown or waiting
    /// its turn, and all that waits to be taken in (`Inbox`): what the next
    /// window up takes over, or what is answered Cancel with no window left
    /// (`finishing::hand_on`). Asked again from the start.
    fn take_held(&mut self) -> crate::finishing::Inbox {
        let mut held = crate::finishing::Inbox::default();
        for review in self.reviews.drain(..) {
            match review {
                crate::review::Review::Plan { plan, .. } => held.proposals.push(plan),
                crate::review::Review::Question(question) => held.questions.push(question),
                crate::review::Review::Clashes(clashes) => held.clashes.push(clashes.question),
            }
        }
        let prompted = self.prompt.take().map(|prompt| prompt.kind);
        if let Some(crate::prompts::PromptKind::Secret { id }) = prompted {
            let shown = self
                .secret_shown
                .take()
                .filter(|question| question.id == id);
            match shown {
                Some(question) => held.secrets.push(question),
                None => {
                    self.app.answer_secret(id, None);
                }
            }
        }
        held.secrets.extend(self.secrets.drain(..));
        if let Some(form) = self.form.take() {
            held.forms.push(form.into_asked());
        }
        held.forms.extend(self.forms.drain(..));
        held.front.extend(self.front_waiting.drain(..));
        held.take_in(std::mem::take(&mut self.inbox));
        held
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
            "focus_address" => {
                self.omnibox_begin(crate::omnibox::Begin::Path, None, window, cx);
            }
            "focus_pane" => window.focus(&self.ui().focus, cx),
            "settings" => self.open_settings(args, window, cx),
            // The focused button and the open popup answer these; one
            // that reaches here found neither.
            "toolbar_activate" | "toolbar_close" => {}
            // A dialog keeps the keyboard: Tab walks its own controls.
            "toolbar_next" | "toolbar_prev"
                if self.walk_dialog(name == "toolbar_next", window, cx) => {}
            "toolbar_next" | "toolbar_prev" | "toolbar_first" | "toolbar_last"
                if self.popup_sub_walk(name, window, cx) => {}
            "toolbar_next" => {
                window.focus_next(cx);
                self.reveal_focused_row(window, cx);
            }
            "toolbar_prev" => {
                window.focus_prev(cx);
                self.reveal_focused_row(window, cx);
            }
            "toolbar_first" => {
                self.toolbar_popup_end(false, window, cx);
                self.reveal_focused_row(window, cx);
            }
            "toolbar_last" => {
                self.toolbar_popup_end(true, window, cx);
                self.reveal_focused_row(window, cx);
            }
            "toolbar_menu" => self.toggle_toolbar_menu(args, window, cx),
            "activity" => match args.get("plan").and_then(serde_json::Value::as_u64) {
                Some(plan) => self.show_activity_of(skwir_core::op::PlanId(plan), window, cx),
                None => self.toggle_activity(args, window, cx),
            },
            "activity_earlier" => self.toggle_activity_earlier(cx),
            "activity_cancel_all" => {
                self.activity_cancel_all();
                cx.notify();
            }
            "activity_pause_all" => {
                self.activity_pause_all();
                cx.notify();
            }
            "undo_history" => self.toggle_undo_history(args, window, cx),
            "history_menu" => self.toggle_history_menu(args, window, cx),
            "subfolder_menu" => self.toggle_subfolder_menu(args, window, cx),
            "parents_menu" => self.toggle_parents_menu(args, window, cx),
            "search_chip" => self.toggle_search_chip(args, window, cx),
            name if name.starts_with("settings_") => self.settings_command(name, args, window, cx),
            "shortcuts" => self.toggle_shortcuts(window, cx),
            "about" => {
                let close = args.get("close").and_then(serde_json::Value::as_bool) == Some(true);
                if !close || self.about.is_some() {
                    self.toggle_about(window, cx);
                }
            }
            "ask" => self.open_ask(args, window, cx),
            "choose" => self.open_choose(args, window, cx),
            "choose_option" => self.choose_toggle_option(window, cx),
            name if name.starts_with("menu_") => self.menu_command(name, window, cx),
            "palette" => self.open_palette(window, cx),
            // Up and Down in whichever list has the keyboard: what a
            // question before deleting lists, the names a plan found taken,
            // a review's steps, the palette, a choice, Jump, the tag
            // editor, a long list of choices in Settings, the address's
            // suggestions.
            "list_move" => {
                let mv = crate::modal::ListMove::of(args);
                if self.confirm_move(mv, window, cx)
                    || self.clashes_move(mv, cx)
                    || self.review_move(mv, window, cx)
                    || self.form_move(mv, window, cx)
                {
                } else if self.palette.is_some() {
                    self.palette_move(mv, cx);
                } else if self.choose.is_some() {
                    self.choose_move(mv, cx);
                } else if self.tag_editor.is_some() {
                    self.tag_editor_move(mv, cx);
                } else if self.settings.is_some() {
                    self.settings_command("settings_list_move", args, window, cx);
                } else {
                    self.omnibox_move(mv, cx);
                }
            }
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
                        // The stem selected, so typing keeps the extension.
                        let stem = name.rfind('.').filter(|&i| i > 0).unwrap_or(name.len());
                        let pane = self.pane;
                        let ask = serde_json::json!({
                            "title": "Rename",
                            "accept": "Rename",
                            "placeholder": "New name",
                            "text": name,
                            "select": [0, stem],
                            "command": "rename",
                            "args": { "pane": pane },
                            "field": "new_name",
                        });
                        self.open_ask(&ask, window, cx);
                    }
                    None => self.set_status("select exactly one entry to rename", true, cx),
                }
            }
            "new_file_prompt" => {
                let pane = self.pane;
                let ask = serde_json::json!({
                    "title": "New file",
                    "accept": "Create",
                    "placeholder": "File name, or a folder name ending in /",
                    "command": "new_file",
                    "args": { "pane": pane },
                    "field": "name",
                });
                self.open_ask(&ask, window, cx);
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
                let pane = self.pane;
                let ask = serde_json::json!({
                    "title": "New folder",
                    "accept": "Create",
                    "placeholder": "Folder name",
                    "command": "mkdir",
                    "args": { "pane": pane },
                    "field": "name",
                });
                self.open_ask(&ask, window, cx);
            }
            // A new place where the location shown makes them (a tag in
            // the one shown), as it says (`Verbs::create`).
            "create_here" => match self.ui().place.verbs.create.clone() {
                Some(create) => self.run(&create.command.name, create.command.args, window, cx),
                None => self.set_status("Nothing new can be made here", true, cx),
            },
            // The address field, in search: where it looks as a pill, and a
            // text to start from (`content:`) typed after.
            "search" => {
                self.omnibox_begin(crate::omnibox::Begin::Search, arg("text"), window, cx);
            }
            // The same, this folder only: its words filter it.
            "filter" => {
                self.omnibox_begin(crate::omnibox::Begin::Filter, arg("text"), window, cx);
            }
            "omnibox_submit" => {
                let flag =
                    |name: &str| args.get(name).and_then(serde_json::Value::as_bool) == Some(true);
                self.omnibox_submit(flag("everywhere"), flag("open"), window, cx);
            }
            // Tab in the address field: what is typed completed.
            "omnibox_complete" => self.omnibox_complete(cx),
            // Tab in a dialog's Save as: the folder typed completed.
            "form_complete" => self.form_complete(window, cx),
            // Alt+Up there: the folder above, as Alt+Up goes up in a pane.
            "form_up" => self.form_up(window, cx),
            // Alt+Down in the address field: where else words could search.
            "omnibox_scopes" => self.omnibox_scopes(cx),
            // Shift+Delete there: a recent search forgotten.
            "omnibox_forget" => self.omnibox_forget(window, cx),
            "tags_prompt" => {
                let entries = args
                    .get("entries")
                    .and_then(|e| serde_json::from_value(e.clone()).ok());
                let text = arg("text");
                self.open_tag_editor(entries, text, window, cx);
            }
            // A value to copy, from a link beside it (Properties).
            "copy_text" => {
                if let Some(text) = arg("text") {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                    let what = arg("what").unwrap_or_else(|| "text".into());
                    self.set_status(format!("Copied {what}"), false, cx);
                }
            }
            "compare_copied" => {
                if let Some(text) = arg("text") {
                    let copied = cx
                        .read_from_clipboard()
                        .and_then(|item| item.text())
                        .unwrap_or_default();
                    let what = arg("what").unwrap_or_else(|| "text".into());
                    let (said, wrong) = compared(&text, &copied, &what);
                    self.set_status(said, wrong, cx);
                }
            }
            "tag_editor_apply" => self.tag_editor_apply(window, cx),
            // An address followed (a document's link, one found in text,
            // where a file was downloaded from), opened by the desktop when
            // the user asks: skwir fetches nothing (ADR-020).
            "open_url" => self.open_address(&arg("url").unwrap_or_default(), cx),
            // A file shown in its folder, from a link.
            "reveal" => self.reveal_command(args, window, cx),
            // Jump is the address field, places first (ADR-098).
            "jump" => self.omnibox_begin(crate::omnibox::Begin::Jump, None, window, cx),
            "rename_bulk_prompt" => self.open_bulk(window, cx),
            "rename_bulk_next" => self.bulk_focus_next(1, window, cx),
            "rename_bulk_prev" => self.bulk_focus_next(-1, window, cx),
            "rename_bulk_case" => self.bulk_cycle_case(window, cx),
            "review_apply" => self.review_apply(cx),
            "review_choose" => {
                let delta = args
                    .get("delta")
                    .and_then(serde_json::Value::as_i64)
                    .and_then(|d| isize::try_from(d).ok())
                    .unwrap_or(1);
                self.clashes_choose(delta, cx);
            }
            "review_cancel" => self.review_cancel(cx),
            "usage_refresh" => self.refresh_usage(cx),
            "usage_mode" => {
                if let Some(mode) = args
                    .get("mode")
                    .and_then(|m| serde_json::from_value(m.clone()).ok())
                {
                    self.usage_mode = mode;
                    self.remember_session();
                    cx.notify();
                }
            }
            "timeline" => self.toggle_timeline(cx),
            "terminal" => self.toggle_terminal(window, cx),
            "terminal_copy" => self.terminal_copy(cx),
            "terminal_paste" => self.terminal_paste(args, cx),
            "terminal_select_all" => {
                self.terminal_input(skwir_core::terminal::Input::SelectAll);
            }
            "terminal_clear" => self.terminal_input(skwir_core::terminal::Input::ClearHistory),
            "terminal_menu" => self.open_terminal_menu(None, None, window, cx),
            "terminal_paths" => self.terminal_type_selection(window, cx),
            "terminal_page" => self.terminal_page(args),
            "terminal_links" => self.label_terminal_links(cx),
            "timeline_bucket" => {
                let b = arg("bucket").unwrap_or_else(|| "month".into());
                self.timeline_set_bucket(&b, cx);
            }
            "timeline_subtree" => self.timeline_toggle_subtree(cx),
            "timeline_pan" => {
                if let Some(toward) = arg("toward") {
                    self.timeline_pan_toward(&toward, cx);
                }
            }
            "timeline_measure" => {
                if let Some(by) = arg("by") {
                    self.timeline_set_measure(&by, cx);
                }
            }
            "timeline_date" => {
                if let Some(date) = arg("date") {
                    self.timeline_set_date(&date, cx);
                }
            }
            "timeline_cursor" => {
                let by = args
                    .get("by")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(0);
                let extend = args
                    .get("extend")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false);
                self.timeline_cursor(by, arg("to").as_deref(), extend, cx);
            }
            "timeline_list" => self.timeline_list(window, cx),
            "split" => self.split_command(args, window, cx),
            "layout" => self.layout_command(args, cx),
            "focus_other_pane" => self.focus_other_pane(window, cx),
            "copy_other" => self.send_to_other_pane("copy", window, cx),
            "move_other" => self.send_to_other_pane("move", window, cx),
            "give_other" => self.ask_other_pane(window, cx),
            "accept" => self.confirm_accept(window, cx),
            "dismiss" => self.confirm_dismiss(window, cx),
            "toggle_sidebar" => self.toggle_sidebar(window, cx),
            "sidebar_section" => self.sidebar_section(args, cx),
            "sidebar_place" => self.sidebar_place(args, cx),
            "focus_sidebar" => self.focus_sidebar(window, cx),
            "focus_next_part" => self.focus_next_part(window, cx),
            "sidebar_key" => {
                if let Some(key) = arg("key") {
                    self.sidebar_key(&key, window, cx);
                }
            }
            "context_menu" => self.open_context_menu_here(window, cx),
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
                let id = arg("id").unwrap_or_else(|| view::FIRST.into());
                self.set_view(&id, cx);
            }
            "columns" => {
                if let Some(key) = arg("key") {
                    self.toggle_column(&key, cx);
                }
            }
            "columns_reset" => self.reset_column_widths(cx),
            "select_group" => {
                let group = args
                    .get("group")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|n| usize::try_from(n).ok());
                let add = args
                    .get("add")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false);
                let pane = args
                    .get("pane")
                    .and_then(|p| serde_json::from_value(p.clone()).ok())
                    .unwrap_or(self.pane);
                self.with_pane(pane, |this| this.select_group(group, add, window, cx));
            }
            "grid_caption" => {
                if let Some(key) = arg("key") {
                    self.toggle_grid_caption(&key, cx);
                }
            }
            "view_button" => {
                if let Some(id) = arg("id") {
                    let on = args.get("on").and_then(serde_json::Value::as_bool);
                    self.set_view_button(&id, on, cx);
                }
            }
            "view_buttons_reset" => {
                self.save_view_buttons(crate::address::ViewButtons::default(), cx);
            }
            "view_buttons_shown" => {
                let on = args
                    .get("on")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(!self.prefs.view_buttons_shown);
                self.prefs.view_buttons_shown = on;
                cx.notify();
                let key = crate::settings::VIEW_BUTTONS_SHOWN_KEY;
                crate::settings::remember(self, key, on.to_string(), cx);
            }
            "up_button" => {
                let on = args
                    .get("on")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(!self.prefs.up_button);
                self.prefs.up_button = on;
                cx.notify();
                crate::settings::remember(self, crate::settings::UP_BUTTON_KEY, on.to_string(), cx);
            }
            "grid_fill" => self.toggle_grid_fill(cx),
            "quick_filters" => self.toggle_quick_filters(cx),
            "full_width" => self.toggle_full_width(cx),
            "printed_pages" => {
                let on = args
                    .get("on")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(!self.prefs.printed_pages);
                self.set_printed_pages(on, window, cx);
            }
            "wrap_lines" => {
                let on = args
                    .get("on")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(!self.prefs.wrap_lines);
                self.set_wrap_lines(on, cx);
            }
            "text_continuous" => {
                let on = args
                    .get("on")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(!self.prefs.text_continuous);
                self.set_text_continuous(on, cx);
            }
            "contents_pin" => {
                let on = args
                    .get("on")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(!self.prefs.contents_pinned);
                self.set_contents_pinned(on, cx);
            }
            "model_perspective" | "model_smooth" => {
                let mut look = self.prefs.model_look;
                let switch = if name == "model_perspective" {
                    &mut look.perspective
                } else {
                    &mut look.smooth
                };
                *switch = args
                    .get("on")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(!*switch);
                self.set_model_look(look, cx);
            }
            "zoom" => {
                let dir = arg("dir").unwrap_or_else(|| "reset".into());
                self.zoom_step(&dir, cx);
            }
            "text_size" => {
                let part = arg("part").unwrap_or_else(|| "files".into());
                let dir = arg("dir").unwrap_or_else(|| "reset".into());
                match crate::sizes::Part::named(&part) {
                    Some(part) => self.text_size_step(part, &dir, cx),
                    None => {
                        self.set_status(
                            format!("No part of the window is called {part}"),
                            true,
                            cx,
                        );
                    }
                }
            }
            "inspect" => self.inspect_command(args, window, cx),
            "media_control" => self.media_control(args, cx),
            "media_play" => self.media_play(window, cx),
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
            "media_escape" => {
                if self.preview.fullscreen.is_some() {
                    self.exit_fullscreen(window, cx);
                } else {
                    self.preview_close(window, cx);
                }
            }
            "preview_fullscreen" => self.preview_fullscreen(window, cx),
            "fullscreen_controls" => self.fullscreen_controls(window, cx),
            "fullscreen_step" => self.fullscreen_step(
                args.get("delta")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(1),
                window,
                cx,
            ),
            "slideshow" => self.slideshow(window, cx),
            "slideshow_pace" => {
                if let Some(seconds) = args.get("seconds").and_then(serde_json::Value::as_u64) {
                    self.slideshow_pace(seconds, window, cx);
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
            "preview_scroll" => {
                if let Some(to) = arg("to") {
                    self.preview_scroll(&to, window, cx);
                }
            }
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
            "map_point" => {
                let delta = args.get("delta").and_then(serde_json::Value::as_i64);
                self.map_point(delta.map_or(1, |d| d.clamp(-1, 1) as isize), cx);
            }
            "model_turn" | "model_pan" => {
                let step = |key| {
                    args.get(key)
                        .and_then(serde_json::Value::as_f64)
                        .filter(|v| v.is_finite())
                        .map_or(0., |v| v.clamp(-10., 10.) as f32)
                };
                let (dx, dy) = (step("dx"), step("dy"));
                if name == "model_turn" {
                    self.model_turn(dx, dy, cx);
                } else {
                    self.model_pan(dx, dy, cx);
                }
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
            "preview_menu" => self.preview_menu(args, window, cx),
            "toolbar_submenu" => self.toolbar_submenu(args, window, cx),
            "note" => self.note_command(args, window, cx),
            "preview_goto" => self.preview_goto(window, cx),
            "reader_page_submit" => self.reader_page_submit(window, cx),
            "reader_page_cancel" => self.reader_page_cancel(window, cx),
            "preview_open_external" => self.preview_open_external(cx),
            "preview_copy_path" => self.preview_copy_path(cx),
            "preview_invoke" => self.preview_invoke(args, window, cx),
            "properties" => self.properties(window, cx),
            "properties_count" => self.properties_count(args, cx),
            "properties_fold" => self.properties_fold(args, cx),
            "preview_copy" => self.preview_copy(window, cx),
            "preview_select_all" => self.preview_select_all(window, cx),
            "preview_contents" => self.preview_contents(window, cx),
            "preview_find" => self.preview_find(window, cx),
            "preview_find_close" => self.preview_find_close(window, cx),
            "reader_search" => self.reader_search(cx),
            "preview_match" => self.preview_match(
                args.get("delta")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(1)
                    .clamp(-1, 1) as i32,
                window,
                cx,
            ),
            "reader_link" => self.reader_link(
                args.get("index")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|n| usize::try_from(n).ok())
                    .unwrap_or(usize::MAX),
                args.get("tab").and_then(serde_json::Value::as_bool) == Some(true),
                window,
                cx,
            ),
            "cursor_toggle" => self.toggle_cursor(window, cx),
            "peek" => self.peek_command(window, cx),
            "peek_close" => self.close_peek(window, cx),
            "quit" => self.quit(window, cx),
            "close_window" => {
                self.close_window(window, cx);
                window.remove_window();
            }
            // Declared (`commands.rs`) but carried out nowhere: a mistake
            // the test of the declarations catches.
            other => self.set_status(format!("ui.{other} is not carried out here"), true, cx),
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
    /// The look again, from the settings and the desktop's style: what
    /// was measured in a font or at a text size that changed is measured
    /// again (the preview's rows, as its own text size change does,
    /// `inspector.rs`); the rest is laid out in the theme's fonts and
    /// sizes as it is drawn.
    fn resolve_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Taken in up to now: what the settings declared again here, to say
        // what the look came to, is taken in too, so that saying it starts
        // no second pass (`theme_inputs_of`).
        let current = self.theme_revision == theme_inputs_of(&self.app);
        let was = (cx.global::<Theme>().families(), cx.global::<Theme>().sizes);
        let theme = Theme::resolve(&self.app, window.appearance(), cx.text_system());
        let now = (theme.families(), theme.sizes);
        // Icons in KDE's color scheme drawn in other colors go once no
        // palette is shown before it is taken.
        let previewing =
            crate::settings::previewed(&self.app.settings, crate::settings::COLORS_FROM_KEY)
                .is_some();
        crate::icons::colors_now(&theme, previewing, window, cx);
        // The window is written at the interface's size (ADR-117).
        window.set_rem_size(now.1.rem(crate::sizes::Part::Interface));
        show_behind(&theme, window);
        cx.set_global(theme);
        let declared = show_resolved(&self.app, cx);
        if current {
            self.theme_revision += declared;
        }
        if now.0 != was.0 {
            let fonts = &now.0;
            tracing::info!(ui = %fonts.ui, reading = %fonts.reading, code = %fonts.code, "fonts");
        }
        if now.1 != was.1 {
            tracing::info!(sizes = ?now.1, "text sizes");
        }
        if now != was {
            self.preview.remeasure();
            cx.refresh_windows();
        }
        let reading = |sizes: &crate::sizes::Sizes| sizes.of(crate::sizes::Part::Reading);
        if (reading(&now.1) - reading(&was.1)).abs() > f32::EPSILON {
            self.reading_size_changed(window, cx);
        }
    }

    pub(crate) fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Left: what the core sends and asks is the program's to take in.
        if self.left {
            return;
        }
        let report = self.app.pump_report();
        self.pumped.extend_from_slice(&report.panes_changed);
        self.sync_pumped(report.applied, window, cx);
    }

    /// Whether the program took something for this window, or its tabs
    /// moved: a sync to do though no wake named it (`finishing::woken`).
    pub(crate) fn has_to_sync(&self) -> bool {
        !self.inbox.is_empty() || self.app.workspace.read().revision() != self.tabs_revision
    }

    /// `sync`, the listings pumped already (`finishing::woken`, once for
    /// every window: never one window's turn each per wake).
    pub(crate) fn sync_pumped(
        &mut self,
        applied: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.left {
            return;
        }
        let t0 = Instant::now();
        self.sync_views();
        self.sync_place_views();
        self.sync_keymap(cx);
        // What the program took from the core for this window
        // (`finishing::deliver`).
        let inbox = std::mem::take(&mut self.inbox);
        self.take_plans(inbox.taken, cx);
        for n in inbox.notifications {
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
                        actions: vec![crate::toast::Action {
                            label: action.label,
                            command: action.command.name.to_string(),
                            args: action.command.args,
                        }],
                    },
                    cx,
                ),
                _ => self.set_status(n.text, error, cx),
            }
        }
        self.pending = self.app.pending.get();
        self.sync_clipboard_files(cx);
        // What the theme is made of (`theme_inputs_of`), counted up, never
        // down (ADR-071, ADR-118).
        let revision = theme_inputs_of(&self.app);
        if revision != self.theme_revision {
            self.theme_revision = revision;
            self.resolve_theme(window, cx);
            self.tag_colors = crate::tags::TagColors::read(&self.app);
            self.tag_pins = crate::tags::TagPins::read(&self.app);
            self.tag_fields = self
                .app
                .settings
                .get(skwir_core::metadata::TAG_EXCLUSIVE_SETTING)
                .unwrap_or_default();
            self.view_buttons = crate::address::ViewButtons::read(&self.app);
            self.prefs.friendly_dates = self
                .app
                .settings
                .get::<String>(crate::settings::DATES_KEY)
                .is_none_or(|format| format != "iso");
            // What the window saves itself is read as it stands, a change
            // still waiting its turn included (`settings::remember`).
            self.prefs.grid_fill = self.switch(crate::settings::GRID_FILL_KEY, false);
            self.prefs.view_buttons_shown =
                self.switch(crate::settings::VIEW_BUTTONS_SHOWN_KEY, true);
            self.prefs.up_button = self.switch(crate::settings::UP_BUTTON_KEY, false);
            self.prefs.single_tab = self
                .app
                .settings
                .get::<bool>(crate::settings::SINGLE_TAB_KEY)
                .unwrap_or(false);
            self.prefs.tabs_in_sidebar = self
                .app
                .settings
                .get::<String>(crate::settings::TABS_AT_KEY)
                .is_some_and(|at| at == crate::settings::TABS_IN_SIDEBAR);
            self.prefs.full_path = self
                .app
                .settings
                .get::<bool>(crate::settings::FULL_PATH_KEY)
                .unwrap_or(false);
            self.prefs.notify_desktop = self
                .app
                .settings
                .get::<bool>(crate::settings::NOTIFY_DESKTOP_KEY)
                .unwrap_or(true);
            self.prefs.keep_awake = self
                .app
                .settings
                .get::<bool>(crate::settings::KEEP_AWAKE_KEY)
                .unwrap_or(true);
            self.prefs.drag_image = crate::dragicon::Layout::read(
                self.app
                    .settings
                    .get::<String>(crate::settings::DRAG_IMAGE_KEY)
                    .as_deref(),
            );
            self.prefs.close_split = crate::layout::CloseSplit::read(
                self.app
                    .settings
                    .get::<String>(skwir_core::settings::CLOSE_SPLIT)
                    .as_deref(),
            );
            let icon_theme = self
                .app
                .settings
                .get::<String>(crate::settings::ICON_THEME_KEY)
                .filter(|theme| theme != crate::settings::ICON_THEME_AUTO);
            self.icons.set_theme(icon_theme);
            self.reading_measure = crate::markdown::measure(
                &crate::settings::preference(self, crate::settings::READING_WIDTH_KEY)
                    .unwrap_or_default(),
            );
            self.prefs.printed_pages = self.switch(crate::settings::PRINTED_PAGES_KEY, false);
            self.prefs.wrap_lines = self.switch(crate::settings::WRAP_LINES_KEY, true);
            self.prefs.text_continuous = self.switch(crate::settings::TEXT_CONTINUOUS_KEY, true);
            self.page_text();
            self.prefs.read_ahead = self
                .app
                .settings
                .get::<bool>(crate::settings::READ_AHEAD_KEY)
                .unwrap_or(true);
            self.prefs.model_look = crate::elements::model::Look {
                perspective: self.switch(crate::settings::MODEL_PERSPECTIVE_KEY, false),
                smooth: self.switch(crate::settings::MODEL_SMOOTH_KEY, false),
            };
            self.prefs.contents_pinned = self.switch(crate::settings::CONTENTS_PINNED_KEY, false);
            self.folder_stand_ins = self
                .switch(crate::settings::FOLDER_PREVIEW_FILE_KEY, true)
                .then(|| {
                    crate::settings::preference(self, crate::settings::FOLDER_PREVIEW_NAMES_KEY)
                        .map_or_else(
                            || {
                                crate::preview::StandIns::of_names(
                                    crate::settings::FOLDER_PREVIEW_NAMES_DEFAULT,
                                )
                            },
                            |records| crate::preview::StandIns::of_records(&records),
                        )
                })
                .flatten();
            self.read_player_prefs();
            self.columns = crate::views::list::PerColumn(
                crate::views::list::COLUMNS
                    .each_ref()
                    .map(|column| self.switch(column.setting, column.shown)),
            );
            self.captions = crate::views::grid::CAPTIONS
                .each_ref()
                .map(|caption| self.switch(caption.setting, false));
        }
        if self.type_names.absorb() | self.icons.absorb() {
            cx.notify();
        }
        // The window's commands asked from elsewhere (an agent, a
        // script): run as if pressed, on the active pane, in the order
        // asked, once what changed in the settings is taken in (a command
        // asked after a setting changed acts with it). One the dialog
        // open would take (Settings, the F1 overview, About) waits, with
        // those after it, until it closes: the caller was told it reached
        // the window.
        self.front_waiting.extend(inbox.front);
        while let Some(request) = self.front_waiting.front()
            && self.takes_now(&request.name)
        {
            let request = self.front_waiting.pop_front().expect("one waits");
            // On the active pane as the workspace has it now: one asked
            // before (a tab closed, another shown, a pane made active) may
            // have changed it, here or in the core, with no sync between.
            self.sync_tabs(window, cx);
            if self.admit_here(&request.name, window, cx) {
                self.carry_out(&request.name, request.args, window, cx);
            }
        }
        if let Some(settings) = self.settings.clone() {
            settings.update(cx, |settings, cx| settings.sync(window, cx));
        }
        if !inbox.proposals.is_empty() {
            self.add_reviews(inbox.proposals, window, cx);
        }
        if !inbox.questions.is_empty() {
            self.add_questions(inbox.questions, cx);
        }
        // A question whose asker stopped waiting (a connection cancelled
        // meanwhile) is no longer asked.
        let before = self.reviews.len();
        let app = Arc::clone(&self.app);
        self.reviews.retain(|review| match review {
            crate::review::Review::Question(q) => app.awaited(q.id),
            _ => true,
        });
        if self.reviews.len() != before {
            cx.notify();
        }
        if !inbox.clashes.is_empty() {
            self.add_clash_questions(inbox.clashes, cx);
        }
        self.secrets.extend(inbox.secrets);
        self.ask_secrets(window, cx);
        self.forms.extend(inbox.forms);
        self.ask_forms(window, cx);
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
            // What the location says of itself (its trail, its verbs),
            // read when it changed, outside the lock.
            crate::trail::sync(self);
            // What its view shows that is read on demand (a column of when
            // each picture was taken), read as its sort's is (ADR-131).
            let fields = self.fields_shown();
            let (wanted, close, found_as_tree, fields, folds) = {
                let ws = app.workspace.read();
                let Some(pane) = ws.pane(id) else { continue };
                let fields = (pane.fields_shown() != fields.as_slice()).then_some(fields);
                let view = self.ui().views.get(self.ui().view_id);
                let lays_out = view.is_some_and(|view| view.lays_found_out_as_tree());
                let found_as_tree = (pane.wants_found_tree() != lays_out).then_some(lays_out);
                // Folds take files out only where their headings are drawn
                // (ADR-131, 2): never under the columns, Usage or the map.
                let draws = view.is_some_and(|view| view.draws_headings());
                let folds = (pane.folds_drawn() != draws).then_some(draws);
                let trail = self.ui().place.trail.as_ref();
                let wanted = view
                    .map(|view| view.kept_locations(pane.location(), trail))
                    .unwrap_or_default();
                let close =
                    !pane.expanded().is_empty() && !view.is_some_and(|view| view.opens_in_place());
                (
                    (!wanted.is_empty() || pane.keeps_any()).then_some(wanted),
                    close,
                    found_as_tree,
                    fields,
                    folds,
                )
            };
            if let Some(found_as_tree) = found_as_tree {
                app.set_found_as_tree(id, found_as_tree);
            }
            if let Some(drawn) = folds {
                app.set_folds_drawn(id, drawn);
            }
            if let Some(fields) = fields {
                app.show_fields(id, &fields);
            }
            if close {
                app.collapse_all(id);
            }
            if let Some(wanted) = wanted {
                app.keep_locations(id, &wanted);
            }
            let (location, resorted, replaced, due) = {
                let ws = app.workspace.read();
                let Some(pane) = ws.pane(id) else { continue };
                let location = pane.location().map(ToString::to_string).unwrap_or_default();
                let list = &mut self.ui_mut().list;
                let before = list.listing();
                let resorted = list.sync(pane);
                // What plans bring into its folder, among its rows (ADR-135).
                let now = std::time::Instant::now();
                let (_, due) = list.sync_arriving(pane, &app.executor, now);
                let due = list.arrival_timer(due, now);
                (location, resorted, list.listing() != before, due)
            };
            // What arrives once it lasted, shown then, and what landed let
            // go of: one timer for the pane, waking its window alone.
            if let Some(due) = due {
                crate::work::wake_at(due, app.pane_waker(id), cx);
            }
            resorted_any |= resorted;
            // The files of the folder left drawn while the one gone to is
            // filled: shown as it is once the wait is over, whatever wakes.
            if self.ui().list.holding() && self.files_hold.is_none() {
                self.files_hold = Some(cx.spawn_in(window, async move |this, cx| {
                    cx.background_executor().timer(crate::tabs::HOLD).await;
                    let _ = this.update_in(cx, |this, window, cx| {
                        this.files_hold = None;
                        this.sync(window, cx);
                        cx.notify();
                    });
                }));
            }
            if replaced {
                for v in self.ui_mut().views.values_mut() {
                    v.reset();
                }
            }
            ready &= self.place_pane();
            // After the place a tab shown again restores: what was typed
            // is the cursor to show.
            if resorted {
                self.seek_typed_again(cx);
            }
            if location != self.ui().address_shown {
                self.ui_mut().address_shown.clone_from(&location);
                // The field typed in keeps what is typed: what the words
                // show as they are typed (a search, ADR-098) changes the
                // location under it.
                let address = self.ui().address.clone();
                let typed_in = self.ui().omni.is_some() && address.read(cx).is_focused(window);
                if !typed_in {
                    address.update(cx, |i, cx| i.set_text(location, cx));
                }
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
        if applied > 0 || resorted_any {
            tracing::debug!(
                applied,
                resorted = resorted_any,
                rows = self.ui().list.sorted.len(),
                groups = self.ui().list.rows.spans().len(),
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
            this.sync_text(cx);
            this.sync_bytes(cx);
            this.sync_notes(window, cx);
            this.sync_poster(window, cx);
            this.sync_media(window, cx);
            this.sync_player(window, cx);
            this.sync_awake(window);
        });
        for id in self.panes.iter().map(|p| p.id).collect::<Vec<_>>() {
            self.with_pane(id, |shell| {
                shell.sync_usage(cx);
            });
        }
        self.sync_timelines(cx);
        self.sync_tallies(cx);
        self.sync_terminal(window, cx);
    }

    /// The window says where it is, as a file manager's does: the
    /// system's list of windows and a screen reader name it by its active
    /// pane's location. Set when that changed, after the panes read it.
    fn sync_title(&mut self, window: &mut Window) {
        let place = self.ui().place.title.clone();
        if place == self.title {
            return;
        }
        self.title = place.clone();
        if place.is_empty() {
            window.set_window_title("skwir");
        } else {
            window.set_window_title(&format!("{place} — skwir"));
        }
    }

    /// What plugins count for the footer (`App::tallies`: the shelf's
    /// items); read when that moved, never by asking them each frame.
    fn sync_tallies(&mut self, cx: &mut Context<Self>) {
        let revision = self.app.tallies.revision();
        if self.tallies_revision == Some(revision) {
            return;
        }
        self.tallies_revision = Some(revision);
        let tallies = self.app.tallies.shown();
        if tallies != self.tallies {
            self.tallies = tallies;
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
        // As cards, a split's pane commands act on is raised, the other
        // lowered, while its parts are built (`Shell::lowered`).
        self.drawn_raised = (theme.cards && self.is_split()).then_some(active);
        self.sync_roots();
        crate::trail::sync(self);
        // After the trail's sync, which says whether it is a search.
        crate::search_bar::sync(self, window, cx);
        // What Quick filters counted, taken before anything says it: the
        // empty state and the footer read it in the same frame.
        self.sync_quick_filters(cx);
        let address_bar = address::render_address(self, window, cx);
        // A view that draws its own empty states (Usage) keeps them.
        let draws_empty = self
            .ui()
            .views
            .get(self.ui().view_id)
            .is_some_and(|view| view.draws_empty());
        let placeholder = (!draws_empty)
            .then(|| crate::placeholder::of(self, id))
            .flatten();
        // What it says in place of the entries, said to assistive
        // technology as the pane's description.
        let said = placeholder
            .as_ref()
            .filter(|_| self.a11y)
            .map(|p| match p.detail.as_str() {
                "" => p.title.clone(),
                detail => format!("{}. {detail}", p.title),
            });
        let body = if let Some(placeholder) = placeholder {
            let focus = self.ui().placeholder_focus.clone();
            crate::placeholder::render(placeholder, &focus, cx)
        } else {
            match self.take_view() {
                Some(mut v) => {
                    let el = v.render(self, window, cx);
                    self.put_view(v);
                    // The view's text at the files' size (ADR-117).
                    let rem = cx.global::<Theme>().sizes.rem(crate::sizes::Part::Files);
                    crate::elements::rem::rem(rem, el).into_any_element()
                }
                None => div()
                    .flex_1()
                    .child("no view registered")
                    .into_any_element(),
            }
        };
        // Its buttons act on this pane: drawn while it is the one drawn.
        let banner = crate::loading::message(self, id)
            .map(|banner| crate::loading::render(banner, self, cx));
        self.sync_selection_hydration();
        self.sync_lent_hydration();
        let narrow = crate::quick_filters::render(self, cx);
        let file_area = self.pane_container(body, said, cx);
        let strip = timeline::render(self, active, window, cx);
        // Its footer; the last pane's ends with the window's own state.
        self.sync_weight(window, cx);
        let last = self.panes.last().map(|p| p.id) == Some(id);
        // What runs, read once a frame for the footer and Activity.
        let under = last.then(|| self.under_way());
        if let Some(under) = &under {
            self.sync_unfolded(under, window, cx);
        }
        let footer = status::render(self, under.as_ref(), cx);
        // The window's own, with its state: the last answer over the files
        // at their lower right, where the list shows sizes and dates rather
        // than names; above Activity while it is open there (its strip draws
        // both). On the whole screen the answer is said over the preview
        // instead: one place a frame (`Shell::answer_at`).
        let answer = last
            .then(|| crate::toast::render_answer(self, crate::toast::AnswerAt::Files, cx))
            .flatten();
        let file_area = if last {
            div()
                .relative()
                .flex_1()
                .min_w_0()
                .min_h_0()
                .flex()
                .flex_col()
                .child(file_area)
                .children(answer.map(|answer| {
                    div()
                        .absolute()
                        .left(px(8.))
                        .right(px(8.))
                        .bottom(px(8.))
                        .flex()
                        .justify_end()
                        .child(answer)
                }))
                .into_any_element()
        } else {
            file_area
        };
        self.pane = saved;
        self.drawn_raised = None;
        // Split panes are told apart by the edge they share: the left one
        // side by side, the top one stacked. The active pane's is the accent.
        // As cards (`Theme::cards`), by the gutter between them (`render`),
        // the other's grounds a step down and the active one's row
        // underlined in the accent.
        let stacked = self.layout.stacked();
        let edge = (divider && theme.lines()).then(|| split_edge(stacked, cx));
        let pane = div()
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
            .when(self.is_split() && theme.lines(), |d| {
                d.when(stacked, gpui::Styled::border_t_1)
                    .when(!stacked, gpui::Styled::border_l_1)
                    .border_color(if active { theme.accent } else { theme.border })
            })
            .child(address_bar)
            .children(narrow)
            .children(banner)
            .child(file_area)
            .children(strip)
            .child(footer)
            .children(edge);
        if theme.cards {
            crate::elements::frame::card(
                &self.frame_cards,
                // Its row and its foot, bands of the chrome, at every corner.
                gpui::Corners::all(theme.opacity.interface),
                pane,
            )
            .into_any_element()
        } else {
            pane.into_any_element()
        }
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
        // As cards, the panes share the room the gutter between them
        // leaves, the pointer at the gutter's middle.
        let gutter = px(2. * crate::terminal::half_gutter(cx));
        let (along, size) = (along - gutter / 2., size - gutter);
        let keep = if stacked {
            crate::preview::PANE_KEEP_HEIGHT
        } else {
            crate::preview::PANE_KEEP
        };
        if let Some(share) =
            crate::elements::edge::share(f32::from(along), f32::from(size), keep, SPLIT_SHARE)
        {
            self.split_share = share;
            cx.notify();
        }
    }
}

/// A split's panes share the room evenly until their edge is dragged, and
/// again after a double click on it.
pub(crate) const SPLIT_SHARE: f32 = 0.5;

/// Marker type for dragging the edge between split panes.
#[derive(Clone)]
pub struct SplitResize;

/// The lane of work that reads `paths` (`fs::reach::lane`, ADR-077): the
/// first's, since what one action reads lies together (a folder's files,
/// what was dropped or pasted), and the shared one when there is none.
pub fn lane_of<P: AsRef<std::path::Path>>(paths: impl IntoIterator<Item = P>) -> Lane {
    paths.into_iter().next().map_or(Lane::SHARED, |path| {
        skwir_core::fs::reach::lane(path.as_ref())
    })
}

/// How a window starts (`finishing::open_window`): the shape it takes, if
/// any (its own, the window it was made from, the one used last), whether
/// it is split as that was, and what its first frame is timed from.
pub(crate) struct Start {
    pub(crate) shape: Option<crate::session::Session>,
    pub(crate) split: bool,
    pub(crate) first_frame: FirstFrame,
    /// Where and how big it was asked to open: what it keeps while it opens
    /// maximized (`Placement::of`).
    pub(crate) placed: crate::session::Placement,
}

/// What a pane's window carries to the window its tab moves to (ADR-122):
/// what it shows its files as, and the bars above and under them. The rest
/// of its state is its window's (its views are made in it).
pub(crate) struct Carried {
    pub(crate) view_id: &'static str,
    pub(crate) quick_filters_on: bool,
    pub(crate) timeline: crate::timeline::TimelineState,
}

/// What a window's first frame is timed from: the process's start for the
/// program's first window (what `just idle-pss` reads), its opening for one
/// opened again (from the tray, by a later launch).
#[derive(Clone, Copy)]
pub(crate) struct FirstFrame {
    pub(crate) from: Instant,
    /// What the log says it is after: "start" or "opening".
    pub(crate) after: &'static str,
}

/// Whether `window` turns 3D models (`Shell::draws_meshes`).
fn turns_models(window: &Window) -> bool {
    window.draws_meshes()
        && !window
            .gpu_specs()
            .is_some_and(|specs| specs.is_software_emulated)
}

/// The draggable edge a split's second pane has at its start: its left edge
/// side by side, its top edge stacked, over the border that tells the panes
/// apart.
fn split_edge(stacked: bool, cx: &mut Context<Shell>) -> gpui::AnyElement {
    use crate::elements::edge::{Side, edge};
    let side = if stacked { Side::Top } else { Side::Left };
    edge(
        "split-resize",
        side,
        SplitResize,
        |this| this.split_share = SPLIT_SHARE,
        cx,
    )
    .into_any_element()
}

/// The same edge between split panes drawn as cards (`Theme::cards`): the
/// gutter between them.
fn split_gutter(stacked: bool, cx: &mut Context<Shell>) -> gpui::AnyElement {
    use crate::elements::edge::{Side, gutter};
    let side = if stacked { Side::Top } else { Side::Left };
    gutter(
        "split-resize",
        side,
        SplitResize,
        |this| this.split_share = SPLIT_SHARE,
        cx,
    )
    .into_any_element()
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(first) = self.first_frame.take() {
            tracing::info!(
                "first frame {:.1} ms after {}",
                first.from.elapsed().as_secs_f64() * 1000.0,
                first.after
            );
        }
        self.a11y = window.is_a11y_active();
        self.draws_meshes = turns_models(window);
        self.sync(window, cx);
        // One frame of map tiles per window frame, whatever maps it draws:
        // what the last frame's maps asked for is kept, the rest cancelled
        // (`TileStore::end_frame`), and this frame's maps ask anew.
        {
            let mut tiles = self.map_tiles.borrow_mut();
            tiles.end_frame();
            tiles.begin_frame();
        }
        // Thumbnails count frames drawn, not wakes: a request not asked for
        // a few frames is let go, and a wake that draws nothing (a worker
        // done) would let go of what the next frame still wants.
        self.app.thumbnails.tick_for(self.thumb_asker());
        for image in self.thumbs.begin_frame(self.app.thumbnails.budget()) {
            cx.drop_image(image, Some(window));
        }
        // Files' own icons (an application's, a shortcut's) drawn no more.
        crate::icons::let_go(self.icons.begin_frame(), window, cx);
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
        // Where the window's own tools go this frame (`corner_at`), read by
        // the sidebar, the strip and the panes' rows built below.
        self.sidebar_drawn = sidebar_width;
        self.more_anchor = crate::elements::popover::PopoverAnchor::default();
        let side = (sidebar_width > 0.).then(|| sidebar::render(self, sidebar_width, window, cx));
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
            // As cards, the gutter between them is the edge (`render_pane`
            // draws it on the second pane's border otherwise).
            if index > 0 && theme.cards {
                columns.push(split_gutter(stacked, cx));
            }
            columns.push(self.render_pane(id, share, index > 0, window, cx));
        }
        self.sync_title(window);
        let limit = if bottom {
            self.preview_height_limit(window)
        } else {
            self.preview_width_limit(window)
        };
        let panel = preview::render_panel(self, bottom, limit, self.dock_height_limit(window), cx);
        let terminal = self.render_terminal(window, cx);
        // As cards, the terminal's edge is the gutter above it.
        let terminal_gutter = (theme.cards && terminal.is_some()).then(|| self.terminal_gutter(cx));
        // Not deferred: GPUI cannot nest deferred draws and the dialog's own
        // dropdowns are. It is the last overlay, so it paints above the rest.
        let settings = self
            .settings
            .clone()
            .map(gpui::IntoElement::into_any_element);
        // The whole screen covers the window: what answers the user is
        // said over it (else over the last pane's files, `render_pane`, or
        // above Activity: `Shell::answer_at`).
        let toast_over =
            crate::toast::render_answer(self, crate::toast::AnswerAt::Screen, cx).map(|answer| {
                div()
                    .absolute()
                    .bottom(crate::sizes::sized(38.))
                    .left_0()
                    .right_0()
                    .flex()
                    .justify_center()
                    .child(answer)
            });
        let sequence = crate::sequence::render(self, cx);
        let shortcuts = crate::shortcuts::render(self, window, cx);
        let about = crate::about::render(self, cx);
        let overlays = [
            address::more_popup(self, cx),
            menu::render(self, cx),
            prompts::render_prompt(self, cx),
            crate::form::render(self, window, cx),
            prompts::render_confirm(self, cx),
            crate::choose::render(self, cx),
            crate::tags::render(self, cx),
            renamebulk::render(self, window, cx),
            palette::render(self, cx),
            preview::render_peek(self, cx),
            review::render(self, cx),
        ];

        let root = div()
            .key_context(keymap::ROOT)
            .capture_action(cx.listener(Self::capture_run_command))
            .on_action(cx.listener(Self::on_run_command))
            .capture_key_up(cx.listener(|this, event: &gpui::KeyUpEvent, window, cx| {
                this.peek_key_released(&event.keystroke.key, window, cx);
            }))
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
            .font_family(theme.font.clone())
            .text_body()
            // Nothing painted under the window's areas: each paints its own
            // ground once (`Theme::window_bg`, `window_bar`), so what is
            // behind the window shows through as much as they let it.
            .text_color(theme.text)
            .child(
                self.framed(
                    &theme,
                    window,
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
                                        .children(terminal_gutter)
                                        .children(terminal),
                                )
                                .children(panel),
                        ),
                ),
            )
            .children(about)
            .children(sequence)
            .children(overlays.into_iter().flatten())
            .children(toast_over)
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

impl Shell {
    /// The gutter around the cards at each of the window's edges: at the
    /// top alone, where the frame's own row is (the tabs, the sidebar's
    /// header) and the cards hang below it, as a browser's page under its
    /// toolbar, unless the window meets another there (`Window::tiling`);
    /// elsewhere the cards run to the window's edge (`frame::card` squares
    /// their corners there). None apart by lines.
    pub(crate) fn frame_inset(&self, theme: &Theme, window: &Window) -> gpui::Edges<f32> {
        if !theme.cards {
            return gpui::Edges::default();
        }
        let tiling = window.tiling();
        gpui::Edges {
            top: if tiling.top {
                0.
            } else {
                crate::elements::frame::GUTTER
            },
            ..gpui::Edges::default()
        }
    }

    /// The window's areas as they are drawn: as cards (`Theme::cards`,
    /// ADR-123), on the frame, a gutter above them (`frame_inset`); else as
    /// they are.
    fn framed(&self, theme: &Theme, window: &Window, areas: gpui::Div) -> gpui::AnyElement {
        if !theme.cards {
            return areas.into_any_element();
        }
        let inset = self.frame_inset(theme, window);
        let areas = areas
            .pt(px(inset.top))
            .pr(px(inset.right))
            .pb(px(inset.bottom))
            .pl(px(inset.left));
        crate::elements::frame::frame(
            &self.frame_cards,
            theme.window_frame(),
            theme.window_card_edge(),
            theme.opacity.see_through(),
            areas,
        )
        .into_any_element()
    }
}

/// Whether `copied` holds `text` (a checksum), ignoring case: the whole
/// clipboard or one of its words (`sha256sum` writes the name after it),
/// in words, and whether that is wrong. One word as long as it, but
/// another, differs; nothing of its length is nothing to compare.
fn compared(text: &str, copied: &str, what: &str) -> (String, bool) {
    let same = |word: &str| word.eq_ignore_ascii_case(text);
    let words = || copied.split_whitespace();
    if same(copied.trim()) || words().any(same) {
        (format!("The {what} is the one copied"), false)
    } else if words().any(|word| word.len() == text.len()) {
        (format!("The {what} differs from the one copied"), true)
    } else {
        (format!("No {what} is copied to compare with"), true)
    }
}

/// The window shows what is behind it where its grounds let it through
/// (`Theme::opacity`), blurred where the system blurs what is behind a
/// window that asks; whole otherwise, as the compositor need not draw
/// what is behind it.
fn show_behind(theme: &Theme, window: &Window) {
    window.set_background_appearance(if theme.opacity.see_through() {
        gpui::WindowBackgroundAppearance::Blurred
    } else {
        gpui::WindowBackgroundAppearance::Opaque
    });
}

/// What the theme is made of, counted up, never down: the settings, the
/// desktop's style, what a list previews and the fonts the window found
/// (ADR-071). The shell resolves the theme again when it moves.
fn theme_inputs_of(app: &App) -> u64 {
    app.settings.revision()
        + crate::desktop_style::revision()
        + crate::settings::preview_revision()
        + crate::fonts::revision()
}

/// Say beside the font and size settings what they come to as just
/// resolved (`settings::show_resolved`, ADR-117): how many settings were
/// declared again for it.
fn show_resolved(app: &Arc<App>, cx: &gpui::App) -> u64 {
    let theme = cx.global::<Theme>();
    let hundreds = crate::sizes::Part::ALL.map(|part| theme.sizes.hundred(part));
    let appearance = cx.window_appearance();
    let shown = crate::settings::Shown {
        automatic: crate::fonts::automatic(app, cx.text_system()),
        hundreds,
        from: crate::desktop_style::sizes(&app.settings).from,
        fonts_automatic: crate::desktop_style::fonts_source(),
        dark: theme.is_dark,
        // What the theme's Automatic gives, whatever it says.
        automatic_dark: Theme::look(
            None,
            &crate::desktop_style::colors(&app.settings),
            appearance,
        ),
        colors_from: crate::desktop_style::colors_source(),
        system_dark: matches!(
            appearance,
            gpui::WindowAppearance::Dark | gpui::WindowAppearance::VibrantDark
        ),
        colors: crate::desktop_style::revision(),
    };
    crate::settings::show_resolved(app, shown)
}

#[cfg(test)]
mod tests {
    /// A lens is shown over the pane's own view (ADR-132): from a folder
    /// that remembers the list, a pane whose own is the grid goes to Usage
    /// and back to the list, and its own is still the grid; a view of the
    /// pane's own (the columns) chosen is the pane's.
    #[test]
    fn a_lens_never_takes_the_place_of_the_panes_own_view() {
        use super::Views;
        let remembered = |view: &str| view == "list" || view == "grid";
        // In the folder: its list shown, the pane's grid set aside.
        let in_folder = Views {
            shown: "list",
            own: Some("grid"),
            under_lens: None,
        };
        let lens = in_folder.choose("usage", true, false, remembered);
        assert_eq!(
            (lens.shown, lens.own, lens.under_lens),
            ("usage", None, Some("grid"))
        );
        let back = lens.choose("list", false, true, remembered);
        assert_eq!(back, in_folder, "the grid still the pane's own");
        // A lens from a lens keeps the own under it.
        let again = lens.choose("map", true, false, remembered);
        assert_eq!(again.under_lens, Some("grid"));
        // From the lens to the columns: the pane's own is the columns.
        let columns = lens.choose("columns", false, false, remembered);
        assert_eq!((columns.own, columns.under_lens), (None, None));
        // From the columns to a folder's view: that view the pane's too.
        let chosen = columns.choose("grid", false, true, remembered);
        assert_eq!((chosen.shown, chosen.own), ("grid", None));
    }

    #[test]
    fn a_checksum_is_compared_with_what_is_copied() {
        let sum = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        let said = |copied: &str| super::compared(sum, copied, "SHA-256");
        assert_eq!(
            said(&sum.to_uppercase()),
            ("The SHA-256 is the one copied".into(), false)
        );
        assert_eq!(
            said(&format!("{sum}  it's a café.iso\n")),
            ("The SHA-256 is the one copied".into(), false),
            "as sha256sum writes it"
        );
        assert_eq!(
            said(&"0".repeat(64)),
            ("The SHA-256 differs from the one copied".into(), true)
        );
        assert_eq!(
            said("a sentence"),
            ("No SHA-256 is copied to compare with".into(), true)
        );
    }
}
