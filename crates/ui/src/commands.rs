//! The shell's own commands (`ui.*`), declared in the core's registry as
//! every command is (owner `ui`): their titles, what they do and the
//! schemas of their arguments, so the palette, F1, the keymap, a user's
//! bindings, scripts and agents all know them, and arguments a command
//! does not take are refused, never read as defaults. The shell carries
//! them out (`Shell::run_ui`); one dispatched from elsewhere (an agent, a
//! script) waits for the window (`App::request_front`) and runs when it
//! next wakes, on the pane active then.

use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use skwir_core::app::App;
use skwir_core::command::{
    Effect, Form, Invocation, LeadsTo, NoArgs, Outcome, Repeats, Topic, always,
};
use skwir_core::words;
use skwir_core::words::Words;

/// Who declares them.
pub const OWNER: &str = "ui";

/// `ui.grammar_choose`: a language (its name in lower case) and what
/// colors it.
#[derive(serde::Deserialize, JsonSchema)]
pub struct GrammarArgs {
    pub language: String,
    pub grammar: String,
}

/// The arguments of the commands, as their schemas say them. Only read
/// through serde, to check what a command is given and to describe it.
#[allow(dead_code)] // Fields are read by `Deserialize` and `JsonSchema` only.
mod args {
    use schemars::JsonSchema;
    use serde::Deserialize;
    use skwir_core::op::EntryRef;
    use skwir_core::os_path::PathArg;
    use skwir_core::words::Words;

    /// Arguments that depend on what the command is asked to do (its
    /// `action`, its `key`): any object.
    #[derive(Deserialize, JsonSchema)]
    #[serde(transparent)]
    pub(super) struct Object(serde_json::Map<String, serde_json::Value>);

    /// The filter another program's dialog shows by: one of its own, by
    /// its place in the request's (from 0), or all files (none).
    #[derive(Deserialize, JsonSchema)]
    pub(super) struct ChooserFilter {
        #[serde(default)]
        index: Option<u64>,
    }

    /// One of the choices another program's dialog offers, set: `true` or
    /// `false` for a switch, an option's id for a list.
    #[derive(Deserialize, JsonSchema)]
    pub(super) struct ChooserChoice {
        id: String,
        value: serde_json::Value,
    }

    /// How long a slideshow shows each picture, in seconds.
    #[derive(Deserialize, JsonSchema)]
    pub(super) struct Pace {
        seconds: u64,
    }

    /// A step along something: `1` forward, `-1` back.
    #[derive(Deserialize, JsonSchema)]
    pub(super) struct Delta {
        #[serde(default)]
        delta: Option<i64>,
    }

    /// A setting's value changed by steps, or a slider's page or end.
    #[derive(Deserialize, JsonSchema)]
    pub(super) struct Adjust {
        /// Steps to change by: a number's, a choice's.
        #[serde(default)]
        delta: Option<i64>,
        /// `page_up` or `page_down` (a tenth of a slider's range), `first`
        /// or `last` instead.
        #[serde(default)]
        to: Option<String>,
    }

    /// An amount to move by: seconds, or a fraction of the volume.
    #[derive(Deserialize, JsonSchema)]
    pub(super) struct Amount {
        #[serde(default)]
        delta: Option<f64>,
    }

    /// On or off; switched when absent.
    #[derive(Deserialize, JsonSchema)]
    pub(super) struct On {
        #[serde(default)]
        on: Option<bool>,
    }

    /// Which way: `in`, `out` or `reset`; the preview also takes `width` and
    /// a scale (`"1"` for 100%).
    #[derive(Deserialize, JsonSchema)]
    pub(super) struct Dir {
        #[serde(default)]
        dir: Option<String>,
    }

    /// A part of the window's text size (ADR-117): `interface`, `files`,
    /// `reading` or `code` (`files` when absent), and which way (`reset`
    /// when absent).
    #[derive(Deserialize, JsonSchema)]
    pub(super) struct TextSize {
        #[serde(default)]
        part: Option<String>,
        #[serde(default)]
        dir: Option<TextStep>,
    }

    /// Which way a text size goes: any other word is refused, naming
    /// these, rather than taken for a reset.
    #[derive(Deserialize, JsonSchema)]
    #[serde(rename_all = "lowercase")]
    pub(super) enum TextStep {
        /// A step larger.
        In,
        /// A step smaller.
        Out,
        /// Back to 100 %.
        Reset,
    }

    /// What the Usage map's blocks say.
    #[derive(Deserialize, JsonSchema)]
    pub(super) struct UsageMode {
        mode: crate::usage::Mode,
    }

    /// A text to start from.
    #[derive(Deserialize, JsonSchema)]
    pub(super) struct Text {
        #[serde(default)]
        text: Option<String>,
    }

    /// Where the cursor goes.
    #[derive(Deserialize, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub(super) enum CursorTo {
        Up,
        Down,
        Left,
        Right,
        PageUp,
        PageDown,
        HalfPageUp,
        HalfPageDown,
        Home,
        End,
        /// The item under the cursor.
        Here,
        /// One item along the pane's order, whatever the view draws.
        Previous,
        Next,
    }

    /// What the selection does as the cursor moves.
    #[derive(Deserialize, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub(super) enum CursorStep {
        /// It becomes the item reached (the arrows).
        Select,
        /// It grows to the item reached (Shift).
        Extend,
        /// It stays (Ctrl).
        Move,
        /// It follows the cursor if it did, and stays if it was picked.
        Follow,
    }

    #[derive(Deserialize, JsonSchema)]
    pub(super) struct Cursor {
        #[serde(default)]
        to: Option<CursorTo>,
        #[serde(default)]
        step: Option<CursorStep>,
    }

    #[derive(Deserialize, JsonSchema)]
    pub(super) struct Pick {
        /// Then go to the next item, as yazi's Space does (the default).
        #[serde(default)]
        advance: Option<bool>,
    }

    /// Properties' count of what folders hold: started, or stopped.
    #[derive(Deserialize, JsonSchema)]
    pub(super) struct Count {
        /// Stop counting, keeping what was counted.
        #[serde(default)]
        stop: bool,
    }

    /// Properties' group folded or unfolded.
    #[derive(Deserialize, JsonSchema)]
    pub(super) struct Fold {
        /// The group's key, the same in every language (`Permissions`).
        group: String,
        /// Fold it (`true`) or unfold it (`false`); else the other way.
        #[serde(default)]
        fold: Option<bool>,
    }

    /// A popup's toggle: `kind` names which, `close` closes it (`pointer`: by
    /// a press outside it).
    #[derive(Deserialize, JsonSchema)]
    pub(super) struct Popup {
        #[serde(default)]
        kind: Option<String>,
        #[serde(default)]
        close: bool,
        #[serde(default)]
        pointer: bool,
    }

    /// What the field being typed in takes, under it (ADR-137).
    #[derive(Deserialize, JsonSchema)]
    pub(super) struct FieldReference {
        /// The address field's `?` pressed: its card's reference on or
        /// off.
        #[serde(default)]
        field: Option<String>,
    }

    /// What the field being typed in offers (ADR-137): `step` moves the
    /// highlight; `group` and `item` name the item to take; neither takes
    /// the one lit.
    #[derive(Deserialize, JsonSchema)]
    pub(super) struct FieldComplete {
        #[serde(default)]
        step: Option<i64>,
        #[serde(default)]
        group: Option<usize>,
        #[serde(default)]
        item: Option<usize>,
    }

    /// The preview's More's submenu beside its row: `open` one by its
    /// name, the keyboard on its first row; `back` to its row; `step` the
    /// keyboard along its rows, or to the last (`end`: true) or first.
    #[derive(Deserialize, JsonSchema)]
    pub(super) struct Submenu {
        #[serde(default)]
        open: Option<String>,
        /// The one whose row has the keyboard.
        #[serde(default)]
        here: bool,
        #[serde(default)]
        back: bool,
        #[serde(default)]
        step: Option<i64>,
        #[serde(default)]
        end: Option<bool>,
        /// On a setting's row: the value before (-1) or after (1), or the
        /// next one round (0).
        #[serde(default)]
        value: Option<i64>,
    }

    /// Activity's toggle: `close` folds it, and never opens it; `pointer`,
    /// a press on its strip, opens or folds it, leaving the keyboard where
    /// it was; `plan`, the id of an action under way or waiting, opens it
    /// on that action's card (what arrives in a folder, pressed, ADR-135)
    /// and never folds it.
    #[derive(Deserialize, JsonSchema)]
    pub(super) struct Toggle {
        #[serde(default)]
        close: bool,
        #[serde(default)]
        pointer: bool,
        #[serde(default)]
        plan: Option<u64>,
    }

    /// A dialog's toggle: `close` closes it, and never opens it.
    #[derive(Deserialize, JsonSchema)]
    pub(super) struct Close {
        #[serde(default)]
        close: bool,
    }

    #[derive(Deserialize, JsonSchema)]
    pub(super) struct History {
        /// Ahead of Forward instead of behind Back.
        #[serde(default)]
        forward: bool,
        #[serde(default)]
        close: bool,
        #[serde(default)]
        pointer: bool,
    }

    #[derive(Deserialize, JsonSchema)]
    pub(super) struct Folders {
        /// The place whose folders, or the folders above which, are listed.
        #[serde(default)]
        location: Option<PathArg>,
        /// Where the trail goes on, marked in the list.
        #[serde(default)]
        current: Option<PathArg>,
        #[serde(default)]
        close: bool,
        #[serde(default)]
        pointer: bool,
    }

    #[derive(Deserialize, JsonSchema)]
    pub(super) struct ListMoveArgs {
        /// Rows to move by, round the ends.
        #[serde(default)]
        delta: Option<i64>,
        /// `page_up`, `page_down`, `first` or `last` instead.
        #[serde(default)]
        to: Option<String>,
    }

    #[derive(Deserialize, JsonSchema)]
    pub(super) struct Find {
        /// Before the cursor.
        #[serde(default)]
        backward: bool,
    }

    /// What the address field is asked to offer (`ui.suggest`).
    #[derive(Deserialize, JsonSchema)]
    pub(super) struct Suggest {
        /// The text, as it would be typed in the address field.
        pub(super) text: String,
        /// Where words search, as a location (a folder, `tags:/x`). When
        /// omitted, none, as Ctrl+L and Ctrl+J start the field: words then
        /// search the folder the pane shows, and the tabs, the history and
        /// the places one goes to are offered too.
        #[serde(default)]
        pub(super) scope: Option<String>,
        #[serde(default)]
        pub(super) pane: Option<skwir_core::workspace::PaneId>,
    }

    #[derive(Deserialize, JsonSchema)]
    pub(super) struct Submit {
        /// Search the home folder instead of where the pill says.
        #[serde(default)]
        everywhere: bool,
        /// Open the file the address names, rather than show it in its
        /// folder.
        #[serde(default)]
        open: bool,
    }

    #[derive(Deserialize, JsonSchema)]
    pub(super) struct TagsPrompt {
        /// The files whose tags are edited; the selection when absent.
        #[serde(default)]
        entries: Option<Vec<EntryRef>>,
        /// A tag to start typing from.
        #[serde(default)]
        text: Option<String>,
    }

    #[derive(Deserialize, JsonSchema)]
    pub(super) struct CopyText {
        text: String,
        /// What it is, for the message ("the path"): words as text, or as
        /// their data (the window's own, said in its language).
        #[serde(default)]
        what: Option<Words>,
    }

    #[derive(Deserialize, JsonSchema)]
    pub(super) struct Url {
        url: String,
    }

    #[derive(Deserialize, JsonSchema)]
    pub(super) struct Bucket {
        /// `day`, `week`, `month` or `year`.
        #[serde(default)]
        bucket: Option<String>,
    }

    #[derive(Deserialize, JsonSchema)]
    pub(super) struct Toward {
        /// `earlier` or `later`.
        toward: String,
    }

    #[derive(Deserialize, JsonSchema)]
    pub(super) struct Measure {
        /// `files` or `size`.
        by: String,
    }

    #[derive(Deserialize, JsonSchema)]
    pub(super) struct TimelineDate {
        /// `modified`, `created`, `accessed` or `taken` (a picture's or a
        /// video's clock).
        date: String,
    }

    #[derive(Deserialize, JsonSchema)]
    pub(super) struct TimelineCursor {
        /// Bars on, negative for earlier ones.
        #[serde(default)]
        by: Option<i64>,
        /// `first` or `last`, in place of `by`.
        #[serde(default)]
        to: Option<String>,
        /// Extend the bars chosen rather than choose this one.
        #[serde(default)]
        extend: bool,
    }

    #[derive(Deserialize, JsonSchema)]
    pub(super) struct Split {
        /// `other` splits the other way, for this split only.
        #[serde(default)]
        axis: Option<String>,
    }

    #[derive(Deserialize, JsonSchema)]
    pub(super) struct Layout {
        /// Where split panes go: `auto`, `row` or `column`.
        #[serde(default)]
        panes: Option<String>,
        /// Where the preview goes: `auto`, `right` or `bottom`.
        #[serde(default)]
        preview: Option<String>,
        /// Where a file of an archive is previewed: `auto`, `right` or `bottom`.
        #[serde(default)]
        member: Option<String>,
    }

    #[derive(Deserialize, JsonSchema)]
    pub(super) struct Key {
        key: String,
    }

    /// What a Exclusions page row's menu does (`ui.settings_exclusion_item`).
    #[derive(Deserialize, JsonSchema)]
    pub(super) struct FolderItem {
        /// `duplicate`, `edit`, `move`, `reset` or `remove` an entry;
        /// `rename`, `delete` or `add_here` a category.
        #[serde(rename = "do")]
        action: String,
        /// The entry's id.
        #[serde(default)]
        entry: Option<String>,
        /// The category's id.
        #[serde(default)]
        category: Option<String>,
        /// Where `move` takes the entry: a category's id.
        #[serde(default)]
        to: Option<String>,
    }

    #[derive(Deserialize, JsonSchema)]
    pub(super) struct ViewButton {
        /// The view's id (`list`, `grid`, `columns`, a plugin's).
        id: String,
        /// Give it a button (true) or take it away (false); without, the
        /// other of what it has.
        #[serde(default)]
        on: Option<bool>,
    }

    #[derive(Deserialize, JsonSchema)]
    pub(super) struct View {
        /// The view's id (`list`, `grid`, `columns`, a plugin's).
        #[serde(default)]
        id: Option<String>,
        /// A location to show it at first.
        #[serde(default)]
        location: Option<serde_json::Value>,
    }

    #[derive(Deserialize, JsonSchema)]
    pub(super) struct Continue {
        /// `off`, `next` or `shuffle`; the next of them when absent.
        #[serde(default)]
        mode: Option<String>,
    }

    #[derive(Deserialize, JsonSchema)]
    pub(super) struct Player {
        /// What the player is asked (`playing::PlayerRequest`).
        request: serde_json::Value,
    }

    #[derive(Deserialize, JsonSchema)]
    pub(super) struct Pan {
        #[serde(default)]
        dx: Option<f64>,
        #[serde(default)]
        dy: Option<f64>,
    }

    /// Which way an arrow moves what the preview shows.
    #[derive(Deserialize, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub(super) enum Arrow {
        Up,
        Down,
        Left,
        Right,
    }

    #[derive(Deserialize, JsonSchema)]
    pub(super) struct PreviewScroll {
        to: Arrow,
    }

    #[derive(Deserialize, JsonSchema)]
    pub(super) struct Pointer {
        #[serde(default)]
        phase: Option<String>,
        #[serde(default)]
        page: Option<u32>,
        #[serde(default)]
        x: Option<f64>,
        #[serde(default)]
        y: Option<f64>,
    }

    #[derive(Deserialize, JsonSchema)]
    pub(super) struct Page {
        #[serde(default)]
        page: Option<u32>,
    }

    /// A link of a page, and whether a file it names is shown in a tab of
    /// its own.
    #[derive(Deserialize, JsonSchema)]
    pub(super) struct PageLink {
        index: usize,
        #[serde(default)]
        tab: bool,
        /// The page it is on, by its number: the page of what is
        /// selected, else the page shown, where left out.
        #[serde(default)]
        page: Option<u32>,
    }

    #[derive(Deserialize, JsonSchema)]
    pub(super) struct Pages {
        #[serde(default)]
        pages: Option<i64>,
    }

    /// What is pasted into the terminal.
    #[derive(Deserialize, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub(super) enum PasteFrom {
        Clipboard,
        /// What the middle button pastes: the text selected last anywhere
        /// on the desktop, where it keeps one (Linux); the clipboard
        /// elsewhere.
        Primary,
    }

    #[derive(Deserialize, JsonSchema)]
    pub(super) struct TerminalPaste {
        /// The clipboard, unless said.
        #[serde(default)]
        from: Option<PasteFrom>,
    }

    #[derive(Deserialize, JsonSchema)]
    pub(super) struct Ask {
        /// Words as text, or as their data (the window's own questions,
        /// said in its language).
        #[serde(default)]
        title: Option<Words>,
        #[serde(default)]
        placeholder: Option<Words>,
        /// The answer to start from.
        #[serde(default)]
        text: Option<String>,
        /// The part of `text` selected at first, `[start, end]` in bytes.
        #[serde(default)]
        select: Option<[usize; 2]>,
        /// The verb on the button that answers (Rename, Create); OK when
        /// absent.
        #[serde(default)]
        accept: Option<Words>,
        /// A line under the field: what the answer may be.
        #[serde(default)]
        note: Option<Words>,
        /// What the field takes (ADR-137), opened from the `?` inside it
        /// or F1: its groups of items, each as written, what it means, what
        /// it gives, and what choosing it writes.
        #[serde(default)]
        reference: Option<skwir_core::reference::Reference>,
        /// What says, as it is typed, what the text comes to: a command
        /// that only answers (a data command; any other is refused), run
        /// on a worker with the text as its `field`; its `says` is shown
        /// under the field, its error there too, holding the answer back.
        #[serde(default)]
        describe: Option<AskDescribe>,
        /// What runs with the answer.
        command: String,
        #[serde(default)]
        args: Option<serde_json::Value>,
        /// The argument of `command` the answer is.
        field: String,
    }

    /// What `ui.ask`'s `describe` runs: `command`, with `args` and the
    /// text typed as the asked field.
    #[derive(Deserialize, JsonSchema)]
    pub(super) struct AskDescribe {
        command: String,
        #[serde(default)]
        args: Option<serde_json::Value>,
        /// It runs as an agent's call (`permissions::as_agent`): set where
        /// an agent asks the question, whatever the agent said.
        #[serde(default)]
        agent: bool,
    }

    /// Which group `ui.select_group` selects, and how.
    #[derive(Deserialize, JsonSchema)]
    pub(super) struct SelectGroup {
        /// The group, counted from the first heading from 0; the cursor's
        /// when omitted.
        #[serde(default)]
        group: Option<usize>,
        /// Beside what is selected, rather than in its place.
        #[serde(default)]
        add: bool,
        /// The pane; the active one when omitted.
        #[serde(default)]
        pane: Option<skwir_core::workspace::PaneId>,
    }

    #[derive(Deserialize, JsonSchema)]
    pub(super) struct Settings {
        /// A setting to show (`plugin.option`).
        #[serde(default)]
        key: Option<String>,
        /// A page to show (`plugins`, a plugin's id).
        #[serde(default)]
        page: Option<String>,
        /// What the Keyboard page's list is narrowed to, with `page`
        /// `keyboard`: words of the actions' or keys (`ctrl+t`); the
        /// Plugins page's, with `page` `plugins`: words of a plugin's
        /// name, what it does, its id or its category.
        #[serde(default)]
        filter: Option<String>,
        /// What the Exclusions page lists, with `page` `exclusions`:
        /// `searches` (by default) or `agents`, its Secrets unfolded.
        #[serde(default)]
        role: Option<String>,
    }

    #[derive(Deserialize, JsonSchema)]
    pub(super) struct Scrub {
        #[serde(default)]
        fraction: Option<f64>,
        #[serde(default)]
        phase: Option<String>,
    }
}

use args::{
    Adjust, Amount, Ask, Bucket, ChooserChoice, ChooserFilter, Close, Continue, CopyText, Count,
    Cursor, Delta, Dir, FieldComplete, FieldReference, Find, Fold, FolderItem, Folders, History,
    Key, Layout, ListMoveArgs, Object, On, Pace, Page, PageLink, Pages, Pan, Pick, Player, Pointer,
    Popup, PreviewScroll, Scrub, SelectGroup, Settings, Split, Submenu, Submit, TagsPrompt,
    TerminalPaste, Text, TextSize, TimelineCursor, TimelineDate, Toggle, Toward, Url, UsageMode,
    View, ViewButton,
};

/// The window's commands an agent may run (ADR-044): they change only
/// what the window shows, or open a prompt the user answers.
const SHOWS: &[&str] = &[
    "ui.cursor",
    "ui.cursor_pick",
    "ui.cursor_toggle",
    "ui.context_menu",
    "ui.find",
    "ui.find_next",
    "ui.view",
    "ui.columns",
    "ui.columns_reset",
    "ui.select_group",
    "ui.grid_caption",
    "ui.view_button",
    "ui.view_buttons_reset",
    "ui.view_buttons_shown",
    "ui.up_button",
    "ui.zoom",
    "ui.text_size",
    "ui.grid_fill",
    "ui.quick_filters",
    "ui.peek",
    "ui.peek_close",
    "ui.usage_refresh",
    "ui.usage_mode",
    "ui.rename_prompt",
    "ui.mkdir_prompt",
    "ui.new_file_prompt",
    "ui.create_here",
    "ui.rename_bulk_prompt",
    "ui.rename_bulk_next",
    "ui.rename_bulk_prev",
    "ui.rename_bulk_case",
    "ui.tags_prompt",
    "ui.focus_address",
    "ui.search",
    "ui.filter",
    "ui.search_chip",
    "ui.history_menu",
    "ui.subfolder_menu",
    "ui.parents_menu",
    "ui.reveal",
    "ui.jump",
    "ui.palette",
    "ui.list_move",
    "ui.shortcuts",
    "ui.about",
    "ui.settings",
    "ui.settings_close",
    "ui.settings_next",
    "ui.settings_prev",
    "ui.settings_search",
    "ui.settings_page",
    "ui.focus_pane",
    "ui.focus_other_pane",
    "ui.focus_sidebar",
    "ui.focus_next_part",
    "ui.toggle_sidebar",
    "ui.toolbar_menu",
    "ui.activity",
    "ui.activity_earlier",
    "ui.undo_history",
    "ui.toolbar_close",
    "ui.toolbar_next",
    "ui.toolbar_prev",
    "ui.toolbar_first",
    "ui.toolbar_last",
    "ui.split",
    "ui.layout",
    "ui.timeline",
    "ui.timeline_bucket",
    "ui.timeline_pan",
    "ui.timeline_subtree",
    "ui.timeline_measure",
    "ui.timeline_date",
    "ui.timeline_cursor",
    "ui.timeline_list",
    "ui.terminal",
    "ui.terminal_page",
    "ui.terminal_links",
    "ui.terminal_menu",
    "ui.menu_up",
    "ui.menu_down",
    "ui.menu_first",
    "ui.menu_last",
    "ui.menu_expand",
    "ui.menu_collapse",
    "ui.menu_close",
    "ui.toggle_preview",
    "ui.preview_close",
    "ui.properties",
    "ui.properties_count",
    "ui.properties_fold",
    "ui.preview_menu",
    "ui.toolbar_submenu",
    "ui.preview_page",
    "ui.preview_scroll",
    "ui.preview_seek",
    "ui.preview_zoom",
    "ui.inspect",
    "ui.printed_pages",
    "ui.wrap_lines",
    "ui.text_continuous",
    "ui.model_perspective",
    "ui.model_smooth",
    "ui.full_width",
    "ui.note",
    "ui.reader_continuous",
    "ui.reader_drag",
    "ui.reader_page",
    "ui.preview_goto",
    "ui.reader_page_submit",
    "ui.reader_page_cancel",
    "ui.preview_select_all",
    "ui.preview_contents",
    "ui.contents_pin",
    "ui.preview_find",
    "ui.preview_find_close",
    "ui.preview_match",
    "ui.reader_search",
    "ui.media_control",
    "ui.media_play",
    "ui.media_pin",
    "ui.media_keep_playing",
    "ui.media_autoplay",
    "ui.media_continue",
    "ui.media_step",
    "ui.media_scrub",
    "ui.media_escape",
    "ui.preview_fullscreen",
    "ui.fullscreen_controls",
    "ui.fullscreen_step",
    "ui.slideshow",
    "ui.slideshow_pace",
    "ui.player",
    "ui.player_volume",
    "ui.volume_step",
    "ui.map_pan",
    "ui.map_point",
    "ui.model_turn",
    "ui.model_pan",
    "ui.map_view_fit",
    "ui.map_view_spread",
    "ui.map_view_search_area",
    // The window's own state, as its buttons change it (ADR-138).
    "ui.maximize_window",
    "ui.minimize_window",
];

/// The window's commands an agent's call runs only once the user allows
/// it: they reach beyond the window (a program, the clipboard, an
/// address; the terminal's selection is the desktop's primary one), take
/// away what the terminal kept, change how the sidebar is arranged, stop
/// or pause what runs, or prompt the user for a command the agent named.
const ACTS: &[&str] = &[
    "ui.ask",
    "ui.choose",
    "ui.open_url",
    "ui.preview_open_external",
    "ui.map_open",
    "ui.copy_text",
    "ui.compare_copied",
    "ui.preview_copy",
    "ui.preview_copy_path",
    "ui.terminal_copy",
    "ui.terminal_select_all",
    "ui.terminal_clear",
    "ui.reader_link",
    "ui.sidebar_place",
    "ui.sidebar_section",
    "ui.activity_cancel_all",
    "ui.activity_pause_all",
    "ui.quit",
    "ui.close_window",
];

/// What a command that shows (`SHOWS`) would do beyond the window by its
/// arguments, said with the command that does it and asks the user first:
/// `ui.inspect`'s copy (the clipboard), a link to an address, and a label
/// typed, which follows whichever link it labels. An agent's call of it is
/// refused, so that showing never opens an address or writes the
/// clipboard unasked. An address is told as the window tells it
/// (`links::is_address`), so the two cannot disagree. Its words are the
/// refusal, which names the command that asks.
fn reaches_out(name: &str, args: &serde_json::Value) -> Option<Words> {
    if name != "ui.inspect" {
        return None;
    }
    match args["action"].as_str()? {
        "copy" => Some(words!(
            commands_agent_copies = "for an agent, { $command } copies through ui.preview_copy, which asks the user first",
            command = name,
            note = "Why an agent's call is refused. command: the command's id (ui.inspect); ui.preview_copy is another's, as written."
        )),
        "link"
            if args["target"]
                .as_str()
                .is_some_and(crate::links::is_address) =>
        {
            Some(words!(
                commands_agent_opens_address = "for an agent, { $command } opens an address through ui.open_url, which asks the user first",
                command = name,
                note = "Why an agent's call is refused. command: the command's id (ui.inspect); ui.open_url is another's, as written."
            ))
        }
        "label_key" => Some(words!(
            commands_agent_follows_label = "for an agent, { $command } follows a link by its target (`link`), not by a label typed",
            command = name,
            note = "Why an agent's call is refused. command: the command's id (ui.inspect); `link` is the argument naming a link's target, as written."
        )),
        _ => None,
    }
}

/// What a window command does for an agent: every one in neither list
/// answers a dialog or presses for the user's hand (a review's Apply, a
/// menu's item, the focused button, a key in the sidebar or the
/// settings), the user's alone.
fn effect_of(name: &str) -> Effect {
    if SHOWS.contains(&name) {
        Effect::Shows
    } else if ACTS.contains(&name) {
        Effect::Acts
    } else {
        Effect::UserOnly
    }
}

/// What the keys of the shell's places run with arguments, beside what
/// the palette offers (`palette::ui_commands`): the command, its arguments
/// and the title they make, which every list of keys says (ADR-024). The
/// cursor's moves are made from their words (`cursor_forms`).
const KEY_FORMS: &[(&str, &str, fn() -> Words)] = &[
    // The lists one picks from while typing, and the names a copy found
    // taken.
    ("ui.list_move", r#"{"delta":-1}"#, || {
        words!(commands_key_list_previous = "Previous in the list")
    }),
    ("ui.list_move", r#"{"delta":1}"#, || {
        words!(commands_key_list_next = "Next in the list")
    }),
    ("ui.list_move", r#"{"to":"page_up"}"#, || {
        words!(commands_key_list_page_up = "A page up the list")
    }),
    ("ui.list_move", r#"{"to":"page_down"}"#, || {
        words!(commands_key_list_page_down = "A page down the list")
    }),
    ("ui.review_choose", r#"{"delta":-1}"#, || {
        words!(commands_key_review_previous = "Previous choice")
    }),
    ("ui.rename_bulk_case", r#"{"delta":-1}"#, || {
        words!(
            commands_key_case_previous = "Previous case",
            note = "What a key does in Rename several: the case its names are given (lowercase, UPPERCASE, Title Case…)."
        )
    }),
    ("ui.rename_bulk_case", r#"{"delta":1}"#, || {
        words!(
            commands_key_case_next = "Next case",
            note = "What a key does in Rename several: the case its names are given (lowercase, UPPERCASE, Title Case…)."
        )
    }),
    // What a field offers as it is typed (ADR-137).
    ("ui.field_complete", r#"{"step":1}"#, || {
        words!(
            commands_key_offer_next = "Next of what the field offers",
            note = "What a key does in a field being typed in: the next of the items it offers as it is typed (a placeholder begun, a sign)."
        )
    }),
    ("ui.field_complete", r#"{"step":-1}"#, || {
        words!(
            commands_key_offer_previous = "Previous of what the field offers",
            note = "What a key does in a field being typed in: the previous of the items it offers as it is typed."
        )
    }),
    // A preview's More, its submenu beside a row (Open With).
    ("ui.toolbar_submenu", r#"{"here":true}"#, || {
        words!(commands_key_submenu_open = "Open the submenu")
    }),
    ("ui.toolbar_submenu", r#"{"step":1}"#, || {
        words!(commands_key_submenu_next = "Next in the submenu")
    }),
    ("ui.toolbar_submenu", r#"{"step":-1}"#, || {
        words!(commands_key_submenu_previous = "Previous in the submenu")
    }),
    ("ui.toolbar_submenu", r#"{"end":false}"#, || {
        words!(commands_key_submenu_first = "First in the submenu")
    }),
    ("ui.toolbar_submenu", r#"{"end":true}"#, || {
        words!(commands_key_submenu_last = "Last in the submenu")
    }),
    ("ui.toolbar_submenu", r#"{"back":true}"#, || {
        words!(commands_key_submenu_back = "Back from the submenu")
    }),
    ("ui.toolbar_submenu", r#"{"value":1}"#, || {
        words!(commands_key_submenu_next_value = "Next value in the submenu")
    }),
    ("ui.toolbar_submenu", r#"{"value":-1}"#, || {
        words!(commands_key_submenu_previous_value = "Previous value in the submenu")
    }),
    ("ui.toolbar_submenu", r#"{"value":0}"#, || {
        words!(commands_key_submenu_change_value = "Change the value in the submenu")
    }),
    // The timeline's bars.
    ("ui.timeline_cursor", r#"{"by":-1}"#, || {
        words!(commands_key_bar_previous = "Previous bar")
    }),
    ("ui.timeline_cursor", r#"{"by":1}"#, || {
        words!(commands_key_bar_next = "Next bar")
    }),
    ("ui.timeline_cursor", r#"{"by":-1,"extend":true}"#, || {
        words!(commands_key_bar_previous_too = "Choose the previous bar too")
    }),
    ("ui.timeline_cursor", r#"{"by":1,"extend":true}"#, || {
        words!(commands_key_bar_next_too = "Choose the next bar too")
    }),
    ("ui.timeline_cursor", r#"{"to":"first"}"#, || {
        words!(commands_key_bar_first = "First bar")
    }),
    ("ui.timeline_cursor", r#"{"to":"last"}"#, || {
        words!(commands_key_bar_last = "Last bar")
    }),
    (
        "ui.timeline_cursor",
        r#"{"to":"first","extend":true}"#,
        || words!(commands_key_bars_to_first = "Choose the bars back to the first"),
    ),
    (
        "ui.timeline_cursor",
        r#"{"to":"last","extend":true}"#,
        || words!(commands_key_bars_to_last = "Choose the bars up to the last"),
    ),
    ("ui.timeline_pan", r#"{"toward":"earlier"}"#, || {
        words!(commands_key_bars_earlier = "Earlier bars")
    }),
    ("ui.timeline_pan", r#"{"toward":"later"}"#, || {
        words!(commands_key_bars_later = "Later bars")
    }),
    ("ui.review_choose", r#"{"delta":1}"#, || {
        words!(commands_key_review_next = "Next choice")
    }),
    // The file list.
    ("ui.cursor_pick", "{}", || {
        words!(
            commands_key_pick_advance = "Pick or unpick, then go to the next",
            note = "What a key does in the file list. To pick is to mark an item as chosen, as yazi's mark does."
        )
    }),
    ("ui.cursor_pick", r#"{"advance":false}"#, || {
        words!(
            commands_key_pick = "Pick or unpick",
            note = "What a key does in the file list. To pick is to mark an item as chosen, as yazi's mark does."
        )
    }),
    ("ui.search", r#"{"text":"content:"}"#, || {
        words!(commands_key_search_contents = "Search the contents…")
    }),
    ("ui.omnibox_submit", r#"{"everywhere":true}"#, || {
        words!(commands_key_search_everywhere = "Search everywhere")
    }),
    ("ui.omnibox_submit", r#"{"open":true}"#, || {
        words!(commands_key_open_named = "Open the file named")
    }),
    ("ui.about", r#"{"close":true}"#, || {
        words!(commands_key_about_close = "Close About")
    }),
    // The sidebar, walked as the file list's rows.
    ("ui.sidebar_key", r#"{"key":"up"}"#, || {
        words!(commands_key_sidebar_up = "Go to the place above")
    }),
    ("ui.sidebar_key", r#"{"key":"down"}"#, || {
        words!(commands_key_sidebar_down = "Go to the place below")
    }),
    ("ui.sidebar_key", r#"{"key":"first"}"#, || {
        words!(commands_key_sidebar_first = "Go to the first place")
    }),
    ("ui.sidebar_key", r#"{"key":"last"}"#, || {
        words!(commands_key_sidebar_last = "Go to the last place")
    }),
    ("ui.sidebar_key", r#"{"key":"left"}"#, || {
        words!(commands_key_sidebar_left = "Go to its group, or fold the group")
    }),
    ("ui.sidebar_key", r#"{"key":"right"}"#, || {
        words!(commands_key_sidebar_right = "Unfold the group, or go into it")
    }),
    ("ui.sidebar_key", r#"{"key":"open"}"#, || {
        words!(commands_key_sidebar_open = "Open the place, or fold the group")
    }),
    ("ui.sidebar_key", r#"{"key":"tab"}"#, || {
        words!(commands_key_sidebar_tab = "Open the place in a new tab")
    }),
    ("ui.sidebar_key", r#"{"key":"menu"}"#, || {
        words!(commands_key_sidebar_menu = "Open its menu")
    }),
    ("ui.sidebar_key", r#"{"key":"move_up"}"#, || {
        words!(commands_key_sidebar_move_up = "Move it up")
    }),
    ("ui.sidebar_key", r#"{"key":"move_down"}"#, || {
        words!(commands_key_sidebar_move_down = "Move it down")
    }),
    ("ui.sidebar_key", r#"{"key":"rename"}"#, || {
        words!(commands_key_sidebar_rename = "Rename it")
    }),
    ("ui.sidebar_key", r#"{"key":"remove"}"#, || {
        words!(commands_key_sidebar_remove = "Remove or hide it")
    }),
    ("ui.toolbar_menu", r#"{"kind":"view_buttons"}"#, || {
        words!(
            commands_key_view_buttons = "Choose the row's buttons: the views', Up",
            note = "What a key does: choose which buttons the row above the files has, the views' and Up's."
        )
    }),
    // What the preview plays, and the pinned player.
    ("ui.media_control", r#"{"action":"play"}"#, || {
        words!(commands_key_media_play = "Play or pause")
    }),
    ("ui.media_control", r#"{"action":"mute"}"#, || {
        words!(commands_key_media_mute = "Mute or unmute")
    }),
    (
        "ui.media_control",
        r#"{"action":"volume_by","value":0.05}"#,
        || words!(commands_key_media_volume_up = "Volume up"),
    ),
    (
        "ui.media_control",
        r#"{"action":"volume_by","value":-0.05}"#,
        || words!(commands_key_media_volume_down = "Volume down"),
    ),
    ("ui.player", r#"{"request":{"seek_by":-5.0}}"#, || {
        words!(commands_key_player_back = "Player: back five seconds")
    }),
    ("ui.player", r#"{"request":{"seek_by":5.0}}"#, || {
        words!(commands_key_player_forward = "Player: forward five seconds")
    }),
    ("ui.player", r#"{"request":"previous"}"#, || {
        words!(commands_key_player_previous = "Player: previous track")
    }),
    ("ui.player", r#"{"request":"next"}"#, || {
        words!(commands_key_player_next = "Player: next track")
    }),
    ("ui.player", r#"{"request":"mute"}"#, || {
        words!(commands_key_player_mute = "Player: mute or unmute")
    }),
    ("ui.player", r#"{"request":"fullscreen"}"#, || {
        words!(commands_key_player_fullscreen = "Player: full screen")
    }),
    ("ui.player_volume", r#"{"delta":0.05}"#, || {
        words!(commands_key_player_volume_up = "Player: volume up")
    }),
    ("ui.player_volume", r#"{"delta":-0.05}"#, || {
        words!(commands_key_player_volume_down = "Player: volume down")
    }),
    ("ui.volume_step", r#"{"delta":0.05}"#, || {
        words!(commands_key_volume_up = "Volume: up")
    }),
    ("ui.volume_step", r#"{"delta":-0.05}"#, || {
        words!(commands_key_volume_down = "Volume: down")
    }),
    ("ui.volume_step", r#"{"delta":1.0}"#, || {
        words!(commands_key_volume_highest = "Volume: highest")
    }),
    ("ui.volume_step", r#"{"delta":-1.0}"#, || {
        words!(commands_key_volume_lowest = "Volume: lowest")
    }),
    // The arrows over what a preview shows.
    ("ui.preview_scroll", r#"{"to":"up"}"#, || {
        words!(commands_key_preview_up = "Preview: scroll up")
    }),
    ("ui.preview_scroll", r#"{"to":"down"}"#, || {
        words!(commands_key_preview_down = "Preview: scroll down")
    }),
    ("ui.preview_scroll", r#"{"to":"left"}"#, || {
        words!(commands_key_preview_left = "Preview: scroll left, or the previous page or picture")
    }),
    ("ui.preview_scroll", r#"{"to":"right"}"#, || {
        words!(commands_key_preview_right = "Preview: scroll right, or the next page or picture")
    }),
    // The arrows on the whole screen: a picture's neighbours, or a page.
    ("ui.fullscreen_step", r#"{"delta":-1}"#, || {
        words!(commands_key_fullscreen_previous = "Full screen: previous picture or page")
    }),
    ("ui.fullscreen_step", r#"{"delta":1}"#, || {
        words!(commands_key_fullscreen_next = "Full screen: next picture or page")
    }),
    // A structured preview: the files of an archive, a text read where it
    // is shown, its links, its size.
    ("ui.inspect", r#"{"action":"step","delta":-1}"#, || {
        words!(commands_key_inspect_previous = "Previous file")
    }),
    ("ui.inspect", r#"{"action":"step","delta":1}"#, || {
        words!(commands_key_inspect_next = "Next file")
    }),
    ("ui.inspect", r#"{"action":"fold","delta":-1}"#, || {
        words!(commands_key_inspect_fold = "Fold the folder, or go to it")
    }),
    ("ui.inspect", r#"{"action":"fold","delta":1}"#, || {
        words!(commands_key_inspect_unfold = "Unfold the folder")
    }),
    ("ui.inspect", r#"{"action":"back"}"#, || {
        words!(commands_key_inspect_back = "Close the file shown")
    }),
    ("ui.inspect", r#"{"action":"scroll","to":"up"}"#, || {
        words!(commands_key_inspect_scroll_up = "Scroll up")
    }),
    ("ui.inspect", r#"{"action":"scroll","to":"down"}"#, || {
        words!(commands_key_inspect_scroll_down = "Scroll down")
    }),
    (
        "ui.inspect",
        r#"{"action":"scroll","to":"page_up"}"#,
        || words!(commands_key_inspect_page_up = "Scroll a page up"),
    ),
    (
        "ui.inspect",
        r#"{"action":"scroll","to":"page_down"}"#,
        || words!(commands_key_inspect_page_down = "Scroll a page down"),
    ),
    ("ui.inspect", r#"{"action":"scroll","to":"start"}"#, || {
        words!(commands_key_inspect_start = "Scroll to the start")
    }),
    ("ui.inspect", r#"{"action":"scroll","to":"end"}"#, || {
        words!(commands_key_inspect_end = "Scroll to the end")
    }),
    ("ui.inspect", r#"{"action":"links_close"}"#, || {
        words!(commands_key_inspect_links_close = "Stop labelling the links")
    }),
    ("ui.inspect", r#"{"action":"larger"}"#, || {
        words!(commands_key_inspect_larger = "Preview: larger text")
    }),
    ("ui.inspect", r#"{"action":"smaller"}"#, || {
        words!(commands_key_inspect_smaller = "Preview: smaller text")
    }),
    ("ui.inspect", r#"{"action":"reset"}"#, || {
        words!(
            commands_key_inspect_reset = "Preview: text at its size",
            note = "What a key does in a structured preview: its text back at its own size."
        )
    }),
    ("ui.map_pan", r#"{"dx":-1,"dy":0}"#, || {
        words!(commands_key_map_left = "Map: move left")
    }),
    ("ui.map_pan", r#"{"dx":1,"dy":0}"#, || {
        words!(commands_key_map_right = "Map: move right")
    }),
    ("ui.map_pan", r#"{"dx":0,"dy":-1}"#, || {
        words!(commands_key_map_up = "Map: move up")
    }),
    ("ui.map_pan", r#"{"dx":0,"dy":1}"#, || {
        words!(commands_key_map_down = "Map: move down")
    }),
    ("ui.map_point", r#"{"delta":1}"#, || {
        words!(commands_key_map_next = "Map: next place")
    }),
    ("ui.map_point", r#"{"delta":-1}"#, || {
        words!(commands_key_map_previous = "Map: previous place")
    }),
    ("ui.model_turn", r#"{"dx":-1,"dy":0}"#, || {
        words!(commands_key_model_turn_left = "Model: turn left")
    }),
    ("ui.model_turn", r#"{"dx":1,"dy":0}"#, || {
        words!(commands_key_model_turn_right = "Model: turn right")
    }),
    ("ui.model_turn", r#"{"dx":0,"dy":-1}"#, || {
        words!(commands_key_model_turn_up = "Model: turn up")
    }),
    ("ui.model_turn", r#"{"dx":0,"dy":1}"#, || {
        words!(commands_key_model_turn_down = "Model: turn down")
    }),
    ("ui.model_pan", r#"{"dx":-1,"dy":0}"#, || {
        words!(commands_key_model_move_left = "Model: move left")
    }),
    ("ui.model_pan", r#"{"dx":1,"dy":0}"#, || {
        words!(commands_key_model_move_right = "Model: move right")
    }),
    ("ui.model_pan", r#"{"dx":0,"dy":-1}"#, || {
        words!(commands_key_model_move_up = "Model: move up")
    }),
    ("ui.model_pan", r#"{"dx":0,"dy":1}"#, || {
        words!(commands_key_model_move_down = "Model: move down")
    }),
    ("ui.terminal_paste", r#"{"from":"primary"}"#, || {
        words!(
            commands_key_terminal_paste_primary = "Terminal: paste the text selected last",
            note = "What a key does in the terminal: paste the text selected last anywhere on the desktop (the middle button's paste)."
        )
    }),
    ("ui.terminal_page", r#"{"pages":1}"#, || {
        words!(commands_key_terminal_page_up = "Terminal: a page up")
    }),
    ("ui.terminal_page", r#"{"pages":-1}"#, || {
        words!(commands_key_terminal_page_down = "Terminal: a page down")
    }),
    // The settings, and the Keyboard page's list of actions.
    ("ui.settings_key", r#"{"key":"up"}"#, || {
        words!(commands_key_actions_up = "Go to the action above")
    }),
    ("ui.settings_key", r#"{"key":"down"}"#, || {
        words!(commands_key_actions_down = "Go to the action below")
    }),
    ("ui.settings_key", r#"{"key":"first"}"#, || {
        words!(commands_key_actions_first = "Go to the first action")
    }),
    ("ui.settings_key", r#"{"key":"last"}"#, || {
        words!(commands_key_actions_last = "Go to the last action")
    }),
    ("ui.settings_key", r#"{"key":"page_up"}"#, || {
        words!(commands_key_actions_page_up = "Go a page up the actions")
    }),
    ("ui.settings_key", r#"{"key":"page_down"}"#, || {
        words!(commands_key_actions_page_down = "Go a page down the actions")
    }),
    ("ui.settings_key", r#"{"key":"left"}"#, || {
        words!(commands_key_actions_left = "Go to the key on the left")
    }),
    ("ui.settings_key", r#"{"key":"right"}"#, || {
        words!(commands_key_actions_right = "Go to the key on the right")
    }),
    ("ui.settings_key", r#"{"key":"open"}"#, || {
        words!(commands_key_actions_open = "Change the key, or add one")
    }),
    ("ui.settings_key", r#"{"key":"remove"}"#, || {
        words!(commands_key_actions_remove = "Remove the key")
    }),
    // The Exclusions page's list.
    ("ui.settings_exclusion", r#"{"key":"up"}"#, || {
        words!(commands_key_exclusions_up = "Go to the folder above")
    }),
    ("ui.settings_exclusion", r#"{"key":"down"}"#, || {
        words!(commands_key_exclusions_down = "Go to the folder below")
    }),
    ("ui.settings_exclusion", r#"{"key":"first"}"#, || {
        words!(commands_key_exclusions_first = "Go to the first category")
    }),
    ("ui.settings_exclusion", r#"{"key":"last"}"#, || {
        words!(commands_key_exclusions_last = "Go to the last folder")
    }),
    ("ui.settings_exclusion", r#"{"key":"page_up"}"#, || {
        words!(commands_key_exclusions_page_up = "Go a page up the folders")
    }),
    ("ui.settings_exclusion", r#"{"key":"page_down"}"#, || {
        words!(commands_key_exclusions_page_down = "Go a page down the folders")
    }),
    ("ui.settings_exclusion", r#"{"key":"left"}"#, || {
        words!(commands_key_exclusions_left = "Fold the category, or go to it")
    }),
    ("ui.settings_exclusion", r#"{"key":"right"}"#, || {
        words!(commands_key_exclusions_right = "Unfold the category")
    }),
    ("ui.settings_exclusion", r#"{"key":"open"}"#, || {
        words!(
            commands_key_exclusions_open = "Turn the switch or the tick",
            note = "What a key does in the list of the settings' Exclusions page: turn the switch or the tick of the row."
        )
    }),
    ("ui.settings_exclusion", r#"{"key":"remove"}"#, || {
        words!(commands_key_exclusions_remove = "Remove the folder you added")
    }),
    ("ui.settings_exclusion", r#"{"key":"menu"}"#, || {
        words!(commands_key_exclusions_menu = "Open the folder's menu")
    }),
    ("ui.settings_page", r#"{"delta":-1}"#, || {
        words!(commands_key_settings_page_previous = "Previous page")
    }),
    ("ui.settings_page", r#"{"delta":1}"#, || {
        words!(commands_key_settings_page_next = "Next page")
    }),
    ("ui.settings_move", r#"{"delta":-1}"#, || {
        words!(commands_key_record_up = "Move the record up")
    }),
    ("ui.settings_move", r#"{"delta":1}"#, || {
        words!(commands_key_record_down = "Move the record down")
    }),
    ("ui.settings_adjust", r#"{"delta":-1}"#, || {
        words!(
            commands_key_adjust_less = "Less, or the previous choice",
            note = "What a key does on a setting's control: a number made smaller, or the choice before."
        )
    }),
    ("ui.settings_adjust", r#"{"delta":1}"#, || {
        words!(
            commands_key_adjust_more = "More, or the next choice",
            note = "What a key does on a setting's control: a number made larger, or the choice after."
        )
    }),
    ("ui.settings_adjust", r#"{"to":"page_down"}"#, || {
        words!(
            commands_key_adjust_much_less = "Much less",
            note = "What a key does on a slider of the settings: a tenth of its range less."
        )
    }),
    ("ui.settings_adjust", r#"{"to":"page_up"}"#, || {
        words!(
            commands_key_adjust_much_more = "Much more",
            note = "What a key does on a slider of the settings: a tenth of its range more."
        )
    }),
    ("ui.settings_adjust", r#"{"to":"first"}"#, || {
        words!(
            commands_key_adjust_least = "The least",
            note = "What a key does on a slider of the settings: to its lowest."
        )
    }),
    ("ui.settings_adjust", r#"{"to":"last"}"#, || {
        words!(
            commands_key_adjust_most = "The most",
            note = "What a key does on a slider of the settings: to its highest."
        )
    }),
];

/// The shell's commands a window answering another program's dialog
/// offers though their topic is not offered there (ADR-144, 1): making a
/// folder and renaming, Properties and copying what it says, a plan's
/// Cancel and a review's answers, the window's own buttons, the palette
/// and the list of keys. Not Quit, which would end the program the user's
/// own windows are: Ctrl+Q there is the dialog's Cancel (`keymap`).
const IN_DIALOGS: &[&str] = &[
    "ui.mkdir_prompt",
    "ui.rename_prompt",
    "ui.properties",
    "ui.properties_count",
    "ui.properties_fold",
    "ui.copy_text",
    "ui.plan_cancel",
    "ui.review_apply",
    "ui.review_cancel",
    "ui.close_window",
    "ui.maximize_window",
    "ui.minimize_window",
    "ui.palette",
    "ui.shortcuts",
];

/// The shell's commands a window answering another program's dialog does
/// not offer though their topic is offered there (ADR-144, 1): what a
/// window of the user's own shows (Activity, the pinned player, what plays
/// there from a menu and whether what plays goes on there), a lens's own
/// (Usage's, the Map's), a slideshow.
const NOT_IN_DIALOGS: &[&str] = &[
    "ui.activity",
    "ui.activity_earlier",
    "ui.usage_mode",
    "ui.usage_refresh",
    "ui.map_view_fit",
    "ui.map_view_spread",
    "ui.map_view_search_area",
    "ui.slideshow",
    "ui.slideshow_pace",
    "ui.media_pin",
    "ui.media_play",
    "ui.media_keep_playing",
];

/// The shell's commands that open something in another program
/// (`Declared::opens_elsewhere`): an address, the file shown, the place
/// shown. A window answering another program's dialog offers none
/// (ADR-144, 1).
const OPENS_ELSEWHERE: &[&str] = &["ui.open_url", "ui.preview_open_external", "ui.map_open"];

/// Where a window answering another program's dialog offers a shell's
/// command by the arguments it runs with (ADR-144, 1;
/// `Declared::in_dialogs_by`), the one answer for each of its forms,
/// keys and buttons: where the split's panes go is the split's, which
/// such a window has none of, while the preview's place and an archive's
/// file's are its own; a view is offered by its role, a listing's shape
/// or an alternate one, never a lens, which answers a question of its own
/// (Usage, the map).
fn dialogs_by(name: &str) -> Option<fn(&App, &serde_json::Value) -> bool> {
    Some(match name {
        "ui.layout" => |_, args| args.get("panes").is_none(),
        "ui.view" => |app, args| {
            args.get("id")
                .and_then(serde_json::Value::as_str)
                .is_none_or(|id| {
                    app.registry
                        .views
                        .find(|view| &*view.id == id)
                        .is_none_or(|view| view.role != skwir_core::registry::ViewRole::Lens)
                })
        },
        _ => return None,
    })
}

/// What a shell's command runs in turn with its arguments
/// (`Declared::leads_to`): a picker its source and each choice's command,
/// a prompt the command its answer goes to, an item of the preview's
/// menu the command it runs with what is shown. A window answering
/// another program's dialog offers them only where it offers what they
/// lead to (ADR-144, 1): Open With's picker is about files, wherever it is
/// drawn.
fn leads_to_of(name: &str) -> Option<LeadsTo> {
    /// `args[command]` with `args[args]`, as a prompt and the preview's
    /// items name what they run.
    fn named(args: &serde_json::Value) -> Vec<Invocation> {
        args.get("command")
            .and_then(serde_json::Value::as_str)
            .map(|command| Invocation {
                name: command.into(),
                args: args
                    .get("args")
                    .filter(|args| args.is_object())
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({})),
            })
            .into_iter()
            .collect()
    }
    Some(match name {
        "ui.choose" => |args| {
            let source = args
                .get("source")
                .and_then(|source| serde_json::from_value::<Invocation>(source.clone()).ok());
            let choices = args
                .get("choices")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|choice| {
                    serde_json::from_value::<Invocation>(choice.get("command")?.clone()).ok()
                });
            source.into_iter().chain(choices).collect()
        },
        "ui.ask" | "ui.preview_invoke" => named,
        _ => return None,
    })
}

/// The shell's commands whose keys may change but never all go (ADR-024):
/// the palette, which reaches every command, and what gets the keyboard
/// out of a dialog or a menu, or through the settings.
const KEEP_A_KEY: &[&str] = &[
    "ui.palette",
    "ui.settings_close",
    "ui.settings_next",
    "ui.settings_prev",
    "ui.accept",
    "ui.dismiss",
    "ui.review_apply",
    "ui.review_cancel",
    "ui.menu_close",
];

/// When a key held down runs the shell's command `name` again (ADR-066),
/// by its arguments: a step along something (the cursor, a list, a page,
/// the volume, the map), never an act or a switch, which the key's
/// repeats would run over and over (peek, the whole screen, a panel,
/// Enter on a folder). `None`: once a press.
fn repeats_of(name: &str) -> Option<Repeats> {
    /// Whether `args[field]` is one of `steps`.
    fn among(args: &serde_json::Value, field: &str, steps: &[&str]) -> bool {
        args.get(field)
            .and_then(serde_json::Value::as_str)
            .is_some_and(|value| steps.contains(&value))
    }
    let rule: Repeats = match name {
        "ui.cursor"
        | "ui.find_next"
        | "ui.zoom"
        | "ui.text_size"
        | "ui.list_move"
        | "ui.review_choose"
        | "ui.rename_bulk_next"
        | "ui.rename_bulk_prev"
        | "ui.toolbar_next"
        | "ui.toolbar_prev"
        | "ui.toolbar_first"
        | "ui.toolbar_last"
        | "ui.menu_up"
        | "ui.menu_down"
        | "ui.menu_first"
        | "ui.menu_last"
        // A submenu opened or left, which holding the key does once; a
        // choice's ring moved along its values.
        | "ui.menu_expand"
        | "ui.menu_collapse"
        | "ui.toolbar_submenu"
        | "ui.settings_next"
        | "ui.settings_prev"
        | "ui.settings_page"
        | "ui.settings_adjust"
        | "ui.settings_move"
        // A dialog's Save as, a folder up at each repeat.
        | "ui.form_up"
        | "ui.terminal_page"
        | "ui.preview_page"
        | "ui.preview_scroll"
        | "ui.preview_seek"
        | "ui.preview_zoom"
        | "ui.preview_match"
        | "ui.media_step"
        | "ui.fullscreen_step"
        | "ui.player_volume"
        | "ui.volume_step"
        | "ui.map_pan"
        | "ui.map_point"
        | "ui.model_turn"
        | "ui.model_pan"
        | "ui.timeline_cursor"
        | "ui.timeline_pan" => always,
        // What a field offers: held, the arrows walk it; taking one is
        // once a press.
        "ui.field_complete" => |args| args.get("step").is_some(),
        // The case of a bulk rename: held, the arrows step it; its key
        // round the cases is once a press.
        "ui.rename_bulk_case" => |args| args.get("delta").is_some(),
        // Pick, then the next: held, it picks a run of files.
        "ui.cursor_pick" => {
            |args| args.get("advance").and_then(serde_json::Value::as_bool) != Some(false)
        }
        "ui.sidebar_key" => |args| {
            among(
                args,
                "key",
                &[
                    "up",
                    "down",
                    "first",
                    "last",
                    "left",
                    "right",
                    "move_up",
                    "move_down",
                ],
            )
        },
        "ui.settings_key" | "ui.settings_exclusion" => |args| {
            among(
                args,
                "key",
                &[
                    "up",
                    "down",
                    "first",
                    "last",
                    "page_up",
                    "page_down",
                    "left",
                    "right",
                ],
            )
        },
        "ui.inspect" => |args| {
            among(
                args,
                "action",
                &["step", "fold", "scroll", "larger", "smaller"],
            )
        },
        "ui.media_control" => |args| among(args, "action", &["volume_by"]),
        "ui.player" => |args| args["request"].get("seek_by").is_some(),
        _ => return None,
    };
    Some(rule)
}

/// The cursor's moves (`ui.cursor`), each where it goes and what the
/// selection does, in words: Select up, Extend the selection a page down.
fn cursor_forms() -> impl Iterator<Item = Form> {
    const TO: [&str; 13] = [
        "up",
        "down",
        "left",
        "right",
        "page_up",
        "page_down",
        "half_page_up",
        "half_page_down",
        "home",
        "end",
        "here",
        "previous",
        "next",
    ];
    const STEP: [Option<&str>; 5] = [
        None,
        Some("select"),
        Some("extend"),
        Some("move"),
        Some("follow"),
    ];
    STEP.into_iter().flat_map(|step| {
        TO.into_iter().map(move |to| {
            let args = match step {
                Some(step) => serde_json::json!({ "to": to, "step": step }),
                None => serde_json::json!({ "to": to }),
            };
            // A page at a time is a row of its own: with the arrows and
            // the ends, one row would be too long to read. Its family is
            // known by the step and by whether it pages, as data.
            let paged = to.contains("page");
            let (title, family) = cursor_words(step, to, paged);
            let id = format!(
                "cursor.{}{}",
                step.unwrap_or("go"),
                if paged { ".paged" } else { "" }
            );
            Form::new(args, title).in_family(id, family)
        })
    })
}

/// A move of the cursor in words, by what the selection does (`step`) and
/// where the cursor goes (`to`), and its family's: the step's alone, or the
/// step's a page at a time (`paged`).
fn cursor_words(step: Option<&str>, to: &'static str, paged: bool) -> (Words, Words) {
    match step {
        Some("extend") => (
            words!(
                commands_cursor_extend_to = "{ $to ->
                    [up] Extend the selection up
                    [down] Extend the selection down
                    [left] Extend the selection left
                    [right] Extend the selection right
                    [page_up] Extend the selection a page up
                    [page_down] Extend the selection a page down
                    [half_page_up] Extend the selection half a page up
                    [half_page_down] Extend the selection half a page down
                    [home] Extend the selection to the first item
                    [end] Extend the selection to the last item
                    [here] Extend the selection to the item under the cursor
                    [previous] Extend the selection to the previous item
                   *[next] Extend the selection to the next item
                }",
                to = to,
                note = "A move of the file list's cursor, in F1 and the Keyboard settings: the selection grows to the item reached (Shift). to: where the cursor goes, as the identifier in brackets names it (never translated)."
            ),
            if paged {
                words!(
                    commands_cursor_family_extend_paged = "Extend the selection a page up or down",
                    note = "A family of the cursor's moves, one row of F1's list of keys: the selection grows to the item reached."
                )
            } else {
                words!(
                    commands_cursor_family_extend = "Extend the selection",
                    note = "A family of the cursor's moves, one row of F1's list of keys: the selection grows to the item reached (up, down, to the first item…)."
                )
            },
        ),
        Some("move") => (
            words!(
                commands_cursor_move_to = "{ $to ->
                    [up] Move the cursor up
                    [down] Move the cursor down
                    [left] Move the cursor left
                    [right] Move the cursor right
                    [page_up] Move the cursor a page up
                    [page_down] Move the cursor a page down
                    [half_page_up] Move the cursor half a page up
                    [half_page_down] Move the cursor half a page down
                    [home] Move the cursor to the first item
                    [end] Move the cursor to the last item
                    [here] Move the cursor to the item under the cursor
                    [previous] Move the cursor to the previous item
                   *[next] Move the cursor to the next item
                }",
                to = to,
                note = "A move of the file list's cursor, in F1 and the Keyboard settings: the selection stays (Ctrl). to: where the cursor goes, as the identifier in brackets names it (never translated)."
            ),
            if paged {
                words!(
                    commands_cursor_family_move_paged = "Move the cursor a page up or down",
                    note = "A family of the cursor's moves, one row of F1's list of keys: the selection stays."
                )
            } else {
                words!(
                    commands_cursor_family_move = "Move the cursor",
                    note = "A family of the cursor's moves, one row of F1's list of keys: the selection stays (up, down, to the first item…)."
                )
            },
        ),
        Some("follow") => (
            words!(
                commands_cursor_go_to = "{ $to ->
                    [up] Go up
                    [down] Go down
                    [left] Go left
                    [right] Go right
                    [page_up] Go a page up
                    [page_down] Go a page down
                    [half_page_up] Go half a page up
                    [half_page_down] Go half a page down
                    [home] Go to the first item
                    [end] Go to the last item
                    [here] Go to the item under the cursor
                    [previous] Go to the previous item
                   *[next] Go to the next item
                }",
                to = to,
                note = "A move of the file list's cursor, in F1 and the Keyboard settings: the selection follows the cursor if it did, and stays if it was picked. to: where the cursor goes, as the identifier in brackets names it (never translated)."
            ),
            if paged {
                words!(
                    commands_cursor_family_go_paged = "Go a page up or down",
                    note = "A family of the cursor's moves, one row of F1's list of keys: the selection follows the cursor if it did."
                )
            } else {
                words!(
                    commands_cursor_family_go = "Go",
                    note = "A family of the cursor's moves, one row of F1's list of keys: the selection follows the cursor if it did (up, down, to the first item…)."
                )
            },
        ),
        // `select`, and no step said.
        _ => (
            words!(
                commands_cursor_select_to = "{ $to ->
                    [up] Select up
                    [down] Select down
                    [left] Select left
                    [right] Select right
                    [page_up] Select a page up
                    [page_down] Select a page down
                    [half_page_up] Select half a page up
                    [half_page_down] Select half a page down
                    [home] Select to the first item
                    [end] Select to the last item
                    [here] Select only the item under the cursor
                    [previous] Select to the previous item
                   *[next] Select to the next item
                }",
                to = to,
                note = "A move of the file list's cursor, in F1 and the Keyboard settings: the selection becomes the item reached (the arrows). to: where the cursor goes, as the identifier in brackets names it (never translated)."
            ),
            if paged {
                words!(
                    commands_cursor_family_select_paged = "Select a page up or down",
                    note = "A family of the cursor's moves, one row of F1's list of keys: the selection becomes the item reached."
                )
            } else {
                words!(
                    commands_cursor_family_select = "Select",
                    note = "A family of the cursor's moves, one row of F1's list of keys: the selection becomes the item reached (up, down, to the first item…)."
                )
            },
        ),
    }
}

/// Every form of the shell's command `name`: what the palette offers, then
/// what its places' keys run.
fn forms_of(name: &str) -> Vec<Form> {
    let offered = crate::palette::ui_commands()
        .iter()
        .filter(|(n, _, _)| *n == name)
        .map(|(_, args, title)| Form::new(args.clone(), title.clone()));
    let keys = KEY_FORMS
        .iter()
        .filter(|(n, _, _)| *n == name)
        .map(|(_, args, title)| {
            Form::new(
                serde_json::from_str(args).expect("valid form args"),
                title(),
            )
        });
    let moves = (name == "ui.cursor")
        .then(cursor_forms)
        .into_iter()
        .flatten();
    let family = family_of(name);
    let mut forms: Vec<Form> = Vec::new();
    for form in offered.chain(keys).chain(moves) {
        if !forms.iter().any(|known| known.args == form.args) {
            // One family a command, known by the command's name.
            forms.push(match &family {
                Some(family) => form.in_family(name, family.clone()),
                None => form,
            });
        }
    }
    forms
}

/// The family the forms of the shell's command `name` are one of
/// (`Form::family`), when they are: what differs is only which way they
/// go.
fn family_of(name: &str) -> Option<Words> {
    Some(match name {
        "ui.zoom" => words!(
            commands_family_zoom = "Zoom",
            note = "A family of keys, one row of F1's list: zoom in, out or back."
        ),
        "ui.text_size" => words!(
            commands_family_text_size = "Text size",
            note =
                "A family of keys, one row of F1's list: the text larger, smaller or back at 100%."
        ),
        "ui.preview_zoom" => words!(
            commands_family_preview_zoom = "Preview: zoom",
            note = "A family of keys, one row of F1's list: the preview zoomed in, out or back."
        ),
        "ui.preview_page" => words!(
            commands_family_preview_page = "Preview: turn the page",
            note = "A family of keys, one row of F1's list: the preview's next or previous page."
        ),
        _ => return None,
    })
}

/// Declare `name`, carried out by the window.
fn declare<A: DeserializeOwned + JsonSchema + 'static>(
    app: &App,
    topic: Topic,
    name: &'static str,
    title: Words,
    description: Words,
) {
    let declared = app.commands.register_raw::<A, _>(
        OWNER,
        topic,
        name,
        title,
        description,
        move |mut args, app| {
            // An agent's call runs here (`permissions::as_agent`), before
            // the window, which runs what it is handed as the user's.
            let agent = skwir_core::permissions::agent_call().is_some();
            if agent && let Some(instead) = reaches_out(name, &args) {
                anyhow::bail!(instead);
            }
            // What a question runs as it is typed, before anyone answers,
            // only answers (ADR-137); an agent's runs as the agent's.
            if name == "ui.ask" {
                if let Some(refused) = crate::prompts::describe_refused(app, &args) {
                    anyhow::bail!(refused);
                }
                if agent
                    && let Some(describe) = args
                        .get_mut("describe")
                        .and_then(serde_json::Value::as_object_mut)
                {
                    describe.insert("agent".into(), true.into());
                }
            }
            app.request_front(Invocation {
                name: name.into(),
                args,
            });
            Ok(Outcome::Done)
        },
    );
    declared.forms(forms_of(name));
    if KEEP_A_KEY.contains(&name) {
        declared.keeps_a_key();
    }
    if let Some(rule) = repeats_of(name) {
        declared.repeats(rule);
    }
    if let Some(rule) = dialogs_by(name) {
        declared.in_dialogs_by(rule);
    }
    if let Some(leads_to) = leads_to_of(name) {
        declared.leads_to(leads_to);
    }
    let declared = if IN_DIALOGS.contains(&name) {
        declared.in_dialogs()
    } else if NOT_IN_DIALOGS.contains(&name) {
        declared.not_in_dialogs()
    } else {
        declared
    };
    let declared = if OPENS_ELSEWHERE.contains(&name) {
        declared.opens_elsewhere()
    } else {
        declared
    };
    declared.effect(effect_of(name));
}

/// `ui.suggest`: what the address field would offer for a text, answered
/// here and now, by every source (`skwir_core::suggest::gather`), for an
/// agent or a script; nothing in the window changes.
fn declare_suggest(app: &App) {
    app.commands
        .register_raw::<args::Suggest, _>(
            OWNER, Topic::Go,
            "ui.suggest",
            words!(commands_suggest = "Address suggestions"),
            words!(commands_suggest_description = "What the address field offers for `text`: its rows, each with its section, title, detail and the invocation Enter runs, and the spans the text is drawn with", note = "Read by agents and in F1's list of actions. What is in backquotes is written as it is: the command's arguments and the values they take."),
            |args, app| {
                let a: args::Suggest = serde_json::from_value(args)?;
                let pane = app.pane_or_active(a.pane);
                let dir = app.pane_dir(pane).ok();
                let scope = a.scope.as_deref().map(skwir_core::Location::parse);
                let ask = skwir_core::suggest::Ask {
                    typed: crate::omnibox::typed_for(app, &a.text, true),
                    caret: a.text.len(),
                    text: a.text,
                    scope,
                    shown: app.pane_location(pane),
                    dir,
                    pane: Some(pane),
                    // The text says the lens whole, as an agent writes it.
                    lens: None,
                };
                let cancel = app.lifetime().child();
                let mut answer = skwir_core::suggest::gather(app, &ask, &cancel);
                // A path's names are the field's own: read here, in the
                // caller's thread, as the field reads them in their lane.
                if ask.typed == skwir_core::suggest::Typed::Path {
                    let names = crate::omnibox::path_suggestions(
                        app,
                        ask.text.trim_start(),
                        ask.dir.as_deref(),
                        ask.pane,
                        &cancel,
                    );
                    answer.rows.splice(0..0, names);
                }
                Ok(Outcome::Value(serde_json::to_value(answer)?))
            },
        )
        .data();
}

/// What the settings' keys are, as each command of them says.
fn key_in_settings() -> Words {
    words!(
        commands_key_in_settings = "A key in the settings",
        note = "What a window command is, read by agents and in F1's list of actions: one of the keys of the settings."
    )
}

/// Declare every `ui.*` command in the core's registry.
#[allow(clippy::too_many_lines)] // One line of the table per command.
pub fn declare_all(app: &App) {
    let none =
        |topic, name, title, description| declare::<NoArgs>(app, topic, name, title, description);
    // The file list.
    declare::<Cursor>(
        app,
        Topic::Select,
        "ui.cursor",
        words!(commands_cursor = "Move the cursor"),
        words!(
            commands_cursor_description =
                "Move the file list's cursor (`to`), and with it the selection as `step` says",
            note = "Read by agents and in F1's list of actions. What is in backquotes is written as it is: the command's arguments and the values they take."
        ),
    );
    declare::<Pick>(
        app,
        Topic::Select,
        "ui.cursor_pick",
        words!(
            commands_cursor_pick = "Pick or unpick",
            note = "A window command's title. To pick is to mark an item as chosen, as yazi's mark does, beside the selection."
        ),
        words!(
            commands_cursor_pick_description =
                "Pick or unpick the item under the cursor, as yazi's mark, then go to the next"
        ),
    );
    none(
        Topic::Select,
        "ui.cursor_toggle",
        words!(commands_cursor_toggle = "Select or deselect"),
        words!(commands_cursor_toggle_description = "Select or deselect the item under the cursor"),
    );
    none(
        Topic::Window,
        "ui.context_menu",
        words!(commands_context_menu = "Open the menu"),
        words!(
            commands_context_menu_description =
                "Open the menu of the selection, or of the folder shown"
        ),
    );
    declare::<Find>(
        app,
        Topic::Find,
        "ui.find",
        words!(commands_find = "Find a name…"),
        words!(
            commands_find_description =
                "Find a name in the pane as it is typed, after the cursor or before it"
        ),
    );
    declare::<Delta>(
        app,
        Topic::Find,
        "ui.find_next",
        words!(commands_find_next = "Find the next name"),
        words!(
            commands_find_next_description =
                "Find the name last looked for again, after the cursor (or before it)"
        ),
    );
    declare::<View>(
        app,
        Topic::View,
        "ui.view",
        words!(commands_view = "Show a view"),
        words!(commands_view_description = "Show the pane's files in a view"),
    );
    declare::<Key>(
        app,
        Topic::View,
        "ui.columns",
        words!(commands_columns = "Show or hide a column"),
        words!(commands_columns_description = "Show or hide one of the list's detail columns"),
    );
    none(
        Topic::View,
        "ui.columns_reset",
        words!(commands_columns_reset = "Reset column widths"),
        words!(
            commands_columns_reset_description =
                "Give the list's detail columns their default widths"
        ),
    );
    declare::<SelectGroup>(
        app,
        Topic::Select,
        "ui.select_group",
        words!(commands_select_group = "Select the group"),
        words!(
            commands_select_group_description = "Select the entries of a group the pane draws under its heading (ADR-130): group `group`, counted from the first heading from 0, else the cursor's; with `add`, beside what is selected. A heading's click and its menu's Select these run it",
            note = "Read by agents and in F1's list of actions. What is in backquotes is written as it is: the command's arguments and the values they take."
        ),
    );
    declare::<Key>(
        app,
        Topic::View,
        "ui.grid_caption",
        words!(commands_grid_caption = "Show or hide a grid caption"),
        words!(
            commands_grid_caption_description = "Say one caption under every name in the grid, or no longer: its tags or one of the list's details, by its setting (ui.grid_caption_tags, ui.grid_caption_size…)"
        ),
    );
    declare::<ViewButton>(
        app,
        Topic::View,
        "ui.view_button",
        words!(commands_view_button = "Show or hide a view's button"),
        words!(
            commands_view_button_description = "Give a view a button beside the address, or take it away; the view shown keeps its button while shown"
        ),
    );
    none(
        Topic::View,
        "ui.view_buttons_reset",
        words!(commands_view_buttons_reset = "Default view buttons"),
        words!(
            commands_view_buttons_reset_description = "Give a button to the views that show the files as listed (list, grid, columns), and no other"
        ),
    );
    declare::<On>(
        app,
        Topic::View,
        "ui.view_buttons_shown",
        words!(commands_view_buttons_shown = "Show view buttons"),
        words!(
            commands_view_buttons_shown_description = "Show the views' buttons beside the address, or one button for them, the view shown, that opens the View menu"
        ),
    );
    declare::<On>(
        app,
        Topic::View,
        "ui.up_button",
        words!(commands_up_button = "Show the Up button"),
        words!(
            commands_up_button_description = "Show Up after Back and Forward on each pane's row, or leave it to the trail and Alt+Up"
        ),
    );
    declare::<Dir>(
        app,
        Topic::View,
        "ui.zoom",
        words!(
            commands_zoom = "Zoom",
            note = "A window command's title: the size of what the view draws (tiles, rows, a map's scale)."
        ),
        words!(
            commands_zoom_description =
                "Size what the view sizes (tiles, rows, a map's scale): in, out or back"
        ),
    );
    declare::<TextSize>(
        app,
        Topic::View,
        "ui.text_size",
        words!(commands_text_size = "Text size"),
        words!(
            commands_text_size_description = "Write a part of the window larger or smaller, a step at a time, or back at 100%: the file views by default, else the interface, reading or code, as its setting does"
        ),
    );
    none(
        Topic::View,
        "ui.grid_fill",
        words!(
            commands_grid_fill = "Toggle filled thumbnails",
            note = "A window command's title: thumbnails cropped to fill their box in the grid, or not."
        ),
        words!(commands_grid_fill_description = "Crop thumbnails in the grid to fill their box"),
    );
    none(
        Topic::Find,
        "ui.quick_filters",
        words!(commands_quick_filters = "Toggle Quick filters"),
        words!(
            commands_quick_filters_description = "Show Quick filters above the active pane's files, wherever it goes: their kinds, years and tags, each with how many files, a choice filtering the pane until taken off"
        ),
    );
    none(
        Topic::Preview,
        "ui.peek",
        words!(
            commands_peek = "Peek",
            note = "A window command's title: look at the file under the cursor in a large preview, while its key is held."
        ),
        words!(
            commands_peek_description = "Look at the file under the cursor in a large preview, which closes when this runs again, or when its key, held, is let go"
        ),
    );
    none(
        Topic::Preview,
        "ui.peek_close",
        words!(commands_peek_close = "Close peek"),
        words!(commands_peek_close_description = "Close the large preview"),
    );
    none(
        Topic::View,
        "ui.usage_refresh",
        words!(commands_usage_refresh = "Measure again"),
        words!(commands_usage_refresh_description = "Measure what the Usage view shows again"),
    );
    declare::<UsageMode>(
        app,
        Topic::View,
        "ui.usage_mode",
        words!(
            commands_usage_mode = "Usage map shows",
            note = "A window command's title: what the blocks of the Usage view's map say (room taken, files, when they changed, kinds)."
        ),
        words!(
            commands_usage_mode_description = "Size the Usage map's blocks by the room or the files they hold, or color them by when they changed or the kinds of their files"
        ),
    );
    // Asking and naming.
    none(
        Topic::Files,
        "ui.rename_prompt",
        words!(commands_rename_prompt = "Rename…"),
        words!(
            commands_rename_prompt_description = "Rename the selected item, or a place of the sidebar in the way its location renames it"
        ),
    );
    none(
        Topic::Files,
        "ui.mkdir_prompt",
        words!(commands_mkdir_prompt = "New folder…"),
        words!(
            commands_mkdir_prompt_description =
                "Ask for a name and make a folder in the folder shown"
        ),
    );
    none(
        Topic::Files,
        "ui.new_file_prompt",
        words!(commands_new_file_prompt = "New file…"),
        words!(
            commands_new_file_prompt_description =
                "Ask for a name and make an empty file (a folder for a name ending in /)"
        ),
    );
    none(
        Topic::Files,
        "ui.create_here",
        words!(commands_create_here = "New here…"),
        words!(
            commands_create_here_description =
                "Make what the location makes (a tag in a tag's folder)"
        ),
    );
    none(
        Topic::Files,
        "ui.rename_bulk_prompt",
        words!(commands_rename_bulk_prompt = "Rename several…"),
        words!(commands_rename_bulk_prompt_description = "Rename the selected items together"),
    );
    none(
        Topic::Window,
        "ui.rename_bulk_next",
        words!(commands_rename_bulk_next = "Next field"),
        words!(commands_rename_bulk_next_description = "Go to the next field of Rename several"),
    );
    none(
        Topic::Window,
        "ui.rename_bulk_prev",
        words!(commands_rename_bulk_prev = "Previous field"),
        words!(
            commands_rename_bulk_prev_description = "Go to the previous field of Rename several"
        ),
    );
    declare::<Delta>(
        app,
        Topic::Files,
        "ui.rename_bulk_case",
        words!(commands_rename_bulk_case = "Change case"),
        words!(
            commands_rename_bulk_case_description = "Choose the next case Rename several gives the names, back to the first after the last; with a `delta`, move that many cases along, stopping at the first or the last",
            note = "Read by agents and in F1's list of actions. What is in backquotes is written as it is: the command's argument."
        ),
    );
    declare::<TagsPrompt>(
        app,
        Topic::Notes,
        "ui.tags_prompt",
        words!(commands_tags_prompt = "Tags…"),
        words!(
            commands_tags_prompt_description =
                "Edit the tags of the selection, or of the files named"
        ),
    );
    none(
        Topic::Notes,
        "ui.tag_editor_apply",
        words!(commands_tag_editor_apply = "Apply tags"),
        words!(commands_tag_editor_apply_description = "Apply what the tag editor changed"),
    );
    declare::<Ask>(
        app,
        Topic::Window,
        "ui.ask",
        words!(commands_ask = "Ask for a line of text"),
        words!(
            commands_ask_description = "Ask the user for a line of text and run a command with it",
            note = "Read by agents and in F1's list of actions."
        ),
    );
    declare::<Object>(
        app,
        Topic::Window,
        "ui.choose",
        words!(commands_choose = "Ask for a choice"),
        words!(
            commands_choose_description = "Ask the user to choose among choices, given or listed by a command, and run the one chosen"
        ),
    );
    none(
        Topic::Window,
        "ui.choose_option",
        words!(commands_choose_option = "Switch the option"),
        words!(commands_choose_option_description = "Switch the choice's option (Always use for…)"),
    );
    none(
        Topic::Window,
        "ui.accept",
        words!(
            commands_accept = "Confirm",
            note = "A window command's title: answer yes to the question a dialog asks."
        ),
        words!(commands_accept_description = "Answer yes to the question asked"),
    );
    none(
        Topic::Window,
        "ui.dismiss",
        words!(
            commands_dismiss = "Cancel",
            note = "A window command's title: answer no to the question a dialog asks."
        ),
        words!(commands_dismiss_description = "Answer no to the question asked"),
    );
    // Another program's Open or Save dialog, answered by the user alone
    // (ADR-141, 8): none of these is an agent's.
    none(
        Topic::Window,
        "ui.chooser_accept",
        words!(commands_chooser_accept = "Answer the dialog"),
        words!(
            commands_chooser_accept_description = "In a window answering another program's Open or Save dialog: choose what is selected, or the name typed in the folder shown, as the program asked"
        ),
    );
    none(
        Topic::Window,
        "ui.chooser_cancel",
        words!(commands_chooser_cancel = "Cancel the dialog"),
        words!(
            commands_chooser_cancel_description = "In a window answering another program's Open or Save dialog: choose nothing, and close the window"
        ),
    );
    // What Escape does first in a dialog's window while a name is typed to
    // reach a file (`pane choosing typing`, the user's choice, 2026-11-19).
    none(
        Topic::Find,
        "ui.type_end",
        words!(
            commands_type_end = "Stop typing to find",
            note = "An action's name: let go of what was typed in the file list to reach a file by its name, its label at the foot of the list going."
        ),
        words!(
            commands_type_end_description = "Let go of what was typed in the file list to reach a file by its name: its label goes, and the next character begins a new name"
        ),
    );
    declare::<ChooserFilter>(
        app,
        Topic::Window,
        "ui.chooser_filter",
        words!(commands_chooser_filter = "Show the files of a filter"),
        words!(
            commands_chooser_filter_description = "In a window answering another program's dialog: show the files of the program's filter `index` (from 0), or all files without it"
        ),
    );
    declare::<ChooserChoice>(
        app,
        Topic::Window,
        "ui.chooser_choice",
        words!(commands_chooser_choice = "Set a choice of the dialog"),
        words!(
            commands_chooser_choice_description = "In a window answering another program's dialog: set its choice `id` to `value`, true or false for a switch, an option's id for a list"
        ),
    );
    none(
        Topic::Files,
        "ui.review_apply",
        words!(commands_review_apply = "Apply the plan"),
        words!(commands_review_apply_description = "Apply the plan being reviewed"),
    );
    none(
        Topic::Files,
        "ui.review_cancel",
        words!(commands_review_cancel = "Discard the plan"),
        words!(commands_review_cancel_description = "Discard the plan being reviewed"),
    );
    declare::<Delta>(
        app,
        Topic::Window,
        "ui.review_choose",
        words!(commands_review_choose = "Next choice"),
        words!(
            commands_review_choose_description = "In the names a copy, a move or a link found taken: the next choice of the row the keyboard is on, or the previous with a negative delta"
        ),
    );
    // The address and searching.
    none(
        Topic::Go,
        "ui.focus_address",
        words!(commands_focus_address = "Focus address bar"),
        words!(commands_focus_address_description = "Edit the active pane's path"),
    );
    declare::<Text>(
        app,
        Topic::Find,
        "ui.search",
        words!(commands_search = "Search…"),
        words!(
            commands_search_description = "Search the folder shown and its subfolders from the address field, starting from `text` where it is given; what is typed there is looked for below the folder",
            note = "Read by agents and in F1's list of actions. What is in backquotes is written as it is: the command's argument."
        ),
    );
    declare::<Text>(
        app,
        Topic::Find,
        "ui.filter",
        words!(commands_filter = "Filter…"),
        words!(
            commands_filter_description = "Filter the folder shown from the address field as you type, its subfolders left out, starting from `text` where it is given; what is typed there is looked for in the folder alone",
            note = "Read by agents and in F1's list of actions. What is in backquotes is written as it is: the command's argument."
        ),
    );
    declare::<Submit>(
        app,
        Topic::Go,
        "ui.omnibox_submit",
        words!(
            commands_omnibox_submit = "Go",
            note = "A window command's title: run what the address field holds (Enter there)."
        ),
        words!(
            commands_omnibox_submit_description =
                "Run the address's suggestion, search the home folder, or open the file it names"
        ),
    );
    none(
        Topic::Find,
        "ui.omnibox_scopes",
        words!(commands_omnibox_scopes = "Where to search"),
        words!(
            commands_omnibox_scopes_description =
                "List the scopes the address field's words could search in, or close the list"
        ),
    );
    none(
        Topic::Go,
        "ui.omnibox_forget",
        words!(commands_omnibox_forget = "Forget the suggestion"),
        words!(
            commands_omnibox_forget_description =
                "Forget the recent search highlighted in the address field's suggestions"
        ),
    );
    declare_suggest(app);
    app.commands.register_raw::<NoArgs, _>(
        OWNER,
        Topic::Skwir,
        "ui.languages",
        words!(commands_languages = "Languages and their grammars"),
        words!(commands_languages_description = "The languages a grammar was found or added for, as last found: each one's files and what colors it, Automatic or chosen, with what may be chosen; the grammars combined from two sources checked first"),
        |_, app| {
            crate::settings::check_grammars_where_asked(app);
            Ok(Outcome::Value(crate::settings::language_records(app)))
        },
    );
    app.commands.register_raw::<NoArgs, _>(
        OWNER,
        Topic::Skwir,
        "ui.grammar_folders",
        words!(commands_grammar_folders = "Where grammars were looked for"),
        words!(commands_grammar_folders_description = "Where tree-sitter grammars were looked for when last found: what was nearly found, each saying what to do, then each folder read and the languages it held"),
        |_, _| Ok(Outcome::Value(crate::settings::grammar_folder_records())),
    );
    app.commands.register::<GrammarArgs, _>(
        OWNER,
        Topic::Skwir,
        "ui.grammar_choose",
        words!(commands_grammar_choose = "Choose a language's grammar"),
        words!(commands_grammar_choose_description = "Color `language` (its name in lower case) with `grammar`: auto (as Automatic uses says), bat, plain, or where its tree-sitter grammar was found (skwir, tree-sitter, zed, helix, neovim, system, emacs)", note = "Read by agents and in F1's list of actions. What is in backquotes is written as it is: the command's arguments and the values they take."),
        |args, app| {
            crate::settings::choose_grammar(app, &args.language, &args.grammar)?;
            Ok(Outcome::Done)
        },
    );
    none(
        Topic::Window,
        "ui.form_complete",
        words!(commands_form_complete = "Complete the folder"),
        words!(
            commands_form_complete_description = "Complete the folder typed in a dialog's Save as: the folder highlighted in its list, else the start the folders completing it share, else the first of them, the name kept"
        ),
    );
    none(
        Topic::Window,
        "ui.form_up",
        words!(commands_form_up = "Folder above"),
        words!(
            commands_form_up_description =
                "In a dialog's Save as, the folder above the one typed, the name kept"
        ),
    );
    none(
        Topic::Go,
        "ui.omnibox_complete",
        words!(
            commands_omnibox_complete = "Complete",
            note = "A window command's title: complete what is typed in the address field (Tab there)."
        ),
        words!(
            commands_omnibox_complete_description = "Complete what is typed in the address field: with the part the names that match it share, else with the suggestion highlighted; a folder completed is then listed"
        ),
    );
    declare::<Object>(
        app,
        Topic::Find,
        "ui.search_chip",
        words!(
            commands_search_chip = "Search pill's menu",
            note = "A window command's title: a pill is a term of a search, drawn in the address field."
        ),
        words!(commands_search_chip_description = "Open or close a search pill's menu"),
    );
    declare::<History>(
        app,
        Topic::Go,
        "ui.history_menu",
        words!(
            commands_history_menu = "Back: history…",
            note =
                "A window command's title: Back is the button; this lists the history behind it."
        ),
        words!(
            commands_history_menu_description = "List the history behind Back, or ahead of Forward"
        ),
    );
    declare::<Folders>(
        app,
        Topic::Go,
        "ui.subfolder_menu",
        words!(
            commands_subfolder_menu = "Folders inside…",
            note = "A window command's title: the folders inside one of the trail's, listed."
        ),
        words!(
            commands_subfolder_menu_description =
                "List the folders or places inside one of the trail's"
        ),
    );
    declare::<Folders>(
        app,
        Topic::Go,
        "ui.parents_menu",
        words!(
            commands_parents_menu = "Folders above…",
            note = "A window command's title: the folders above the trail's first, listed."
        ),
        words!(
            commands_parents_menu_description =
                "List the folders above the trail's first, from the root down"
        ),
    );
    declare::<Url>(
        app,
        Topic::Go,
        "ui.open_url",
        words!(commands_open_url = "Open an address"),
        words!(
            commands_open_url_description =
                "Open an address (http, https, mailto) with the desktop's application"
        ),
    );
    declare::<crate::links::RevealArgs>(
        app,
        Topic::Go,
        "ui.reveal",
        words!(commands_reveal = "Show in folder"),
        words!(commands_reveal_description = "Show a file in its folder, at a line of its preview"),
    );
    declare::<CopyText>(
        app,
        Topic::Files,
        "ui.copy_text",
        words!(commands_copy_text = "Copy"),
        words!(commands_copy_text_description = "Put a text on the clipboard"),
    );
    declare::<CopyText>(
        app,
        Topic::Files,
        "ui.compare_copied",
        words!(commands_compare_copied = "Compare with copied"),
        words!(
            commands_compare_copied_description = "Say whether the clipboard holds a text (a checksum a download page gives, one word of it or all), ignoring case"
        ),
    );
    none(
        Topic::Go,
        "ui.jump",
        words!(
            commands_jump = "Jump…",
            note = "A window command's title: go to a place by typing in the address field."
        ),
        words!(
            commands_jump_description =
                "Type in the address field, the places one goes to offered first"
        ),
    );
    // The window.
    none(
        Topic::Skwir,
        "ui.palette",
        words!(commands_palette = "Command palette"),
        words!(commands_palette_description = "Find a command and run it"),
    );
    declare::<ListMoveArgs>(
        app,
        Topic::Window,
        "ui.list_move",
        words!(commands_list_move = "Next in the list"),
        words!(
            commands_list_move_description = "Move in the list being typed into (the palette, a choice, Jump, the tag editor, the address's suggestions) or answered (the names a copy found taken): rows round its ends, or a page, or to an end"
        ),
    );
    none(
        Topic::Skwir,
        "ui.shortcuts",
        words!(
            commands_shortcuts = "Shortcuts",
            note = "A window command's title: the keys of where the keyboard is."
        ),
        words!(commands_shortcuts_description = "List the keys of where the keyboard is"),
    );
    declare::<Close>(
        app,
        Topic::Skwir,
        "ui.about",
        words!(commands_about = "About Skwir"),
        words!(
            commands_about_description = "Show Skwir's version and licence, and what it runs on"
        ),
    );
    declare::<Settings>(
        app,
        Topic::Skwir,
        "ui.settings",
        words!(commands_settings = "Settings…"),
        words!(commands_settings_description = "Open the settings, at a setting or a page"),
    );
    none(
        Topic::Window,
        "ui.focus_pane",
        words!(commands_focus_pane = "Focus file list"),
        words!(commands_focus_pane_description = "Give the keyboard to the active pane"),
    );
    none(
        Topic::Tabs,
        "ui.focus_other_pane",
        words!(commands_focus_other_pane = "Focus other pane"),
        words!(
            commands_focus_other_pane_description = "Give the keyboard to the split's other pane"
        ),
    );
    none(
        Topic::Window,
        "ui.focus_sidebar",
        words!(commands_focus_sidebar = "Focus sidebar"),
        words!(commands_focus_sidebar_description = "Give the keyboard to the sidebar"),
    );
    none(
        Topic::Window,
        "ui.focus_next_part",
        words!(commands_focus_next_part = "Focus next part of the window"),
        words!(
            commands_focus_next_part_description = "Give the keyboard to the next part of the window shown: the sidebar, the files, the preview"
        ),
    );
    none(
        Topic::View,
        "ui.toggle_sidebar",
        words!(commands_toggle_sidebar = "Toggle sidebar"),
        words!(commands_toggle_sidebar_description = "Show or hide the sidebar"),
    );
    declare::<Key>(
        app,
        Topic::Window,
        "ui.sidebar_key",
        words!(
            commands_sidebar_key = "Sidebar key",
            note = "A window command's title: what a key does in the sidebar."
        ),
        words!(
            commands_sidebar_key_description =
                "A key in the sidebar: walk, open, move, rename or remove its places"
        ),
    );
    declare::<Key>(
        app,
        Topic::Window,
        "ui.settings_key",
        words!(
            commands_settings_key = "Keyboard page key",
            note = "A window command's title: what a key does in the list of actions of the settings' Keyboard page."
        ),
        words!(
            commands_settings_key_description = "A key in the Keyboard page's list of actions: walk the actions and their keys, change, add or remove one"
        ),
    );
    declare::<FolderItem>(
        app,
        Topic::Window,
        "ui.settings_exclusion_item",
        words!(commands_settings_exclusion_item = "Exclusions page menu item"),
        words!(
            commands_settings_exclusion_item_description = "What the menu of a row of the Exclusions page does: duplicate, edit, move, reset or remove a folder; rename or delete one of your categories, or add a folder to one"
        ),
    );
    declare::<Key>(
        app,
        Topic::Window,
        "ui.settings_exclusion",
        words!(
            commands_settings_exclusion = "Exclusions page key",
            note = "A window command's title: what a key does in the list of the settings' Exclusions page."
        ),
        words!(
            commands_settings_exclusion_description = "A key in the Exclusions page's list: walk the categories and their folders, fold one, turn a switch or a tick, remove a folder you added, open a folder's menu"
        ),
    );
    declare::<Object>(
        app,
        Topic::Places,
        "ui.sidebar_section",
        words!(commands_sidebar_section = "Arrange a sidebar group"),
        words!(
            commands_sidebar_section_description = "Move, show, hide or reset the sidebar's groups"
        ),
    );
    declare::<Object>(
        app,
        Topic::Places,
        "ui.sidebar_place",
        words!(commands_sidebar_place = "Arrange a sidebar place"),
        words!(
            commands_sidebar_place_description =
                "Move, rename, show or hide a place the sidebar offers"
        ),
    );
    declare::<Popup>(
        app,
        Topic::Window,
        "ui.toolbar_menu",
        words!(
            commands_toolbar_menu = "Toolbar menu",
            note = "A window command's title: one of the menus of the row above the files or of the preview."
        ),
        words!(
            commands_toolbar_menu_description = "Open or close More, the View menu, the choice of view buttons, the Map view's backgrounds or the pinned player's More (kind: more, view_options, view_buttons, map_backgrounds, player)"
        ),
    );
    declare::<FieldReference>(
        app,
        Topic::Window,
        "ui.field_reference",
        words!(
            commands_field_reference = "What the field takes",
            note = "A window command's title: what the field being typed in takes (its placeholders, terms, signs), shown under it."
        ),
        words!(
            commands_field_reference_description = "Show under the field being typed in what it takes: its placeholders, terms or signs, each with what it means and gives here, a row taken written where the caret is (ADR-137); shown, F1 shows the keys of the place. The address field says it in its card; `field` (its `?`) turns that on and off",
            note = "Read by agents and in F1's list of actions. What is in backquotes is written as it is: the command's arguments and the values they take."
        ),
    );
    declare::<FieldComplete>(
        app,
        Topic::Window,
        "ui.field_complete",
        words!(
            commands_field_complete = "Take what the field offers",
            note =
                "A window command's title: take one of the items the field being typed in offers."
        ),
        words!(
            commands_field_complete_description = "Among what the field being typed in offers as it goes on (a placeholder begun, a sign), move the highlight by `step`, take the item `group` and `item` name, or the one lit: it takes the place of the part typed (ADR-137)",
            note = "Read by agents and in F1's list of actions. What is in backquotes is written as it is: the command's arguments and the values they take."
        ),
    );
    // Answered by the focused button and the open popup themselves.
    none(
        Topic::Window,
        "ui.toolbar_activate",
        words!(commands_toolbar_activate = "Press the button"),
        words!(commands_toolbar_activate_description = "Press the button that has the keyboard"),
    );
    declare::<Toggle>(
        app,
        Topic::Window,
        "ui.activity",
        words!(
            commands_activity = "Activity",
            note =
                "A window command's title: the panel of what runs, waits and ended this session."
        ),
        words!(
            commands_activity_description = "Open Activity above its strip in the footer, with the keyboard, or fold it once it has the keyboard: what runs, with Pause and Cancel, what waits, and what ended this session that the Undo history does not hold (what failed, was cancelled or cannot be undone, and answers that are no action's), with Retry or Undo. It stays open until folded, and opens by itself once an action asked in the window lasts (Open Activity after, 1.5\u{a0}s by default) (ADR-114)"
        ),
    );
    none(
        Topic::Window,
        "ui.activity_earlier",
        words!(commands_activity_earlier = "Show Earlier in Activity"),
        words!(
            commands_activity_earlier_description = "Unfold Earlier at the foot of Activity, or fold it again: what ended this session that the Undo history does not hold"
        ),
    );
    none(
        Topic::Files,
        "ui.activity_cancel_all",
        words!(commands_activity_cancel_all = "Cancel all actions"),
        words!(
            commands_activity_cancel_all_description = "Cancel every action under way and every one waiting its turn: what each did stays, undoable, and what it left half done is taken back"
        ),
    );
    none(
        Topic::Files,
        "ui.plan_cancel",
        words!(
            commands_plan_cancel = "Stop working out",
            note =
                "A window command's title: let go of the plan being worked out for what was asked."
        ),
        words!(
            commands_plan_cancel_description = "Let go of the plan being worked out for what was asked (Escape while it is): its answer is forgotten, and what waited for it runs on"
        ),
    );
    none(
        Topic::Files,
        "ui.activity_pause_all",
        words!(commands_activity_pause_all = "Pause all actions"),
        words!(
            commands_activity_pause_all_description = "Pause every action under way, or resume them all once every one is paused; what waits keeps waiting"
        ),
    );
    declare::<Toggle>(
        app,
        Topic::Files,
        "ui.undo_history",
        words!(commands_undo_history = "Undo history"),
        words!(
            commands_undo_history_description = "Open or close the history of what Undo can take back, above the footer's Undo. Each action there has its own Undo, an undone one its own Redo, which takes it back alone when nothing done after it touched what it did; else the history names the action that did, and offers to undo it with those after it (ADR-079, 13)"
        ),
    );
    none(
        Topic::Window,
        "ui.toolbar_close",
        words!(commands_toolbar_close = "Close the menu"),
        words!(commands_toolbar_close_description = "Close the toolbar's open menu"),
    );
    none(
        Topic::Window,
        "ui.toolbar_next",
        words!(commands_toolbar_next = "Next button"),
        words!(commands_toolbar_next_description = "Give the keyboard to the next button"),
    );
    none(
        Topic::Window,
        "ui.toolbar_prev",
        words!(commands_toolbar_prev = "Previous button"),
        words!(commands_toolbar_prev_description = "Give the keyboard to the previous button"),
    );
    none(
        Topic::Window,
        "ui.toolbar_first",
        words!(commands_toolbar_first = "First row"),
        words!(
            commands_toolbar_first_description =
                "Give the keyboard to the first row of the open menu"
        ),
    );
    none(
        Topic::Window,
        "ui.toolbar_last",
        words!(commands_toolbar_last = "Last row"),
        words!(
            commands_toolbar_last_description =
                "Give the keyboard to the last row of the open menu"
        ),
    );
    declare::<Split>(
        app,
        Topic::Tabs,
        "ui.split",
        words!(commands_split = "Split pane"),
        words!(commands_split_description = "Split the window in two panes, or join them"),
    );
    declare::<Layout>(
        app,
        Topic::View,
        "ui.layout",
        words!(commands_layout = "Arrange the window"),
        words!(
            commands_layout_description =
                "Where split panes, the preview and a file of an archive go"
        ),
    );
    none(
        Topic::Files,
        "ui.copy_other",
        words!(commands_copy_other = "Copy to the other pane"),
        words!(commands_copy_other_description = "Copy the selection to what the other pane shows"),
    );
    none(
        Topic::Files,
        "ui.move_other",
        words!(commands_move_other = "Move to the other pane"),
        words!(commands_move_other_description = "Move the selection to what the other pane shows"),
    );
    none(
        Topic::Files,
        "ui.give_other",
        words!(commands_give_other = "Send to the other pane…"),
        words!(
            commands_give_other_description = "Ask what the selection does in what the other pane shows, as a drop asks: move, copy or link it there, or what a plugin offers (extract an archive)"
        ),
    );
    none(
        Topic::View,
        "ui.timeline",
        words!(commands_timeline = "Timeline"),
        words!(
            commands_timeline_description = "Show or hide the timeline under the active pane's files, wherever it goes: when they changed, a bar for each period, a bar pressed showing only its files"
        ),
    );
    declare::<Bucket>(
        app,
        Topic::View,
        "ui.timeline_bucket",
        words!(commands_timeline_bucket = "Timeline bars"),
        words!(
            commands_timeline_bucket_description = "What each bar of the timeline counts, a day, a week, a month or a year, while the pane shows what it shows: by default the finest that shows every date"
        ),
    );
    declare::<Toward>(
        app,
        Topic::View,
        "ui.timeline_pan",
        words!(commands_timeline_pan = "Timeline: move along"),
        words!(
            commands_timeline_pan_description = "Show the timeline's earlier or later bars, when there are too many of them to show at once"
        ),
    );
    declare::<args::Measure>(
        app,
        Topic::View,
        "ui.timeline_measure",
        words!(commands_timeline_measure = "Timeline: files or size"),
        words!(
            commands_timeline_measure_description =
                "Make the timeline's bars as tall as their number of files, or as their size"
        ),
    );
    declare::<TimelineDate>(
        app,
        Topic::View,
        "ui.timeline_date",
        words!(commands_timeline_date = "Timeline: which time"),
        words!(
            commands_timeline_date_description = "Count files on the timeline by when they were modified, created, last opened, or taken (a picture's or a video's clock)"
        ),
    );
    declare::<TimelineCursor>(
        app,
        Topic::View,
        "ui.timeline_cursor",
        words!(commands_timeline_cursor = "Timeline: go to a bar"),
        words!(
            commands_timeline_cursor_description =
                "Move the timeline's keyboard bar, or extend the bars chosen"
        ),
    );
    none(
        Topic::View,
        "ui.timeline_list",
        words!(commands_timeline_list = "Timeline: list the bars chosen"),
        words!(commands_timeline_list_description = "Show the files of the timeline's bars chosen"),
    );
    none(
        Topic::View,
        "ui.timeline_subtree",
        words!(commands_timeline_subtree = "Timeline: folders inside"),
        words!(commands_timeline_subtree_description = "Count the files of the folders inside too"),
    );
    none(
        Topic::Terminal,
        "ui.terminal",
        words!(commands_terminal = "Toggle terminal"),
        words!(commands_terminal_description = "Show or hide the terminal"),
    );
    none(
        Topic::Terminal,
        "ui.terminal_copy",
        words!(commands_terminal_copy = "Terminal: copy"),
        words!(commands_terminal_copy_description = "Copy the terminal's selection"),
    );
    declare::<TerminalPaste>(
        app,
        Topic::Terminal,
        "ui.terminal_paste",
        words!(commands_terminal_paste = "Terminal: paste"),
        words!(commands_terminal_paste_description = "Paste into the terminal"),
    );
    none(
        Topic::Terminal,
        "ui.terminal_select_all",
        words!(commands_terminal_select_all = "Terminal: select all"),
        words!(
            commands_terminal_select_all_description =
                "Select everything the terminal shows and kept"
        ),
    );
    none(
        Topic::Terminal,
        "ui.terminal_clear",
        words!(commands_terminal_clear = "Terminal: clear the history"),
        words!(
            commands_terminal_clear_description =
                "Forget the lines scrolled away above the terminal's screen"
        ),
    );
    none(
        Topic::Terminal,
        "ui.terminal_paths",
        words!(commands_terminal_paths = "Terminal: type the selection's paths"),
        words!(
            commands_terminal_paths_description = "Type the paths of the selected files at the terminal's prompt, quoted for its shell"
        ),
    );
    none(
        Topic::Terminal,
        "ui.terminal_menu",
        words!(commands_terminal_menu = "Terminal: menu"),
        words!(
            commands_terminal_menu_description = "Open the terminal's menu: copy, paste, its links"
        ),
    );
    declare::<Pages>(
        app,
        Topic::Terminal,
        "ui.terminal_page",
        words!(commands_terminal_page = "Terminal: scroll"),
        words!(commands_terminal_page_description = "Scroll the terminal by pages"),
    );
    none(
        Topic::Terminal,
        "ui.terminal_links",
        words!(commands_terminal_links = "Terminal: follow a link…"),
        words!(commands_terminal_links_description = "Label the terminal's links for the keyboard"),
    );
    for (name, title) in [
        ("ui.menu_up", words!(commands_menu_up = "Previous item")),
        ("ui.menu_down", words!(commands_menu_down = "Next item")),
        ("ui.menu_first", words!(commands_menu_first = "First item")),
        ("ui.menu_last", words!(commands_menu_last = "Last item")),
        (
            "ui.menu_expand",
            words!(commands_menu_expand = "Open the submenu, or the next value"),
        ),
        (
            "ui.menu_collapse",
            words!(commands_menu_collapse = "Leave the submenu, or the previous value"),
        ),
        (
            "ui.menu_activate",
            words!(commands_menu_activate = "Run the item"),
        ),
        (
            "ui.menu_close",
            words!(commands_menu_close = "Close the menu"),
        ),
    ] {
        let description = words!(
            commands_key_in_menu = "A key in a menu",
            note = "What a window command is, read by agents and in F1's list of actions: one of the keys of a menu."
        );
        none(Topic::Window, name, title, description);
    }
    for (name, title) in [
        (
            "ui.settings_close",
            words!(commands_settings_close = "Close settings"),
        ),
        (
            "ui.settings_next",
            words!(commands_settings_next = "Next control"),
        ),
        (
            "ui.settings_prev",
            words!(commands_settings_prev = "Previous control"),
        ),
        (
            "ui.settings_search",
            words!(commands_settings_search = "Search settings"),
        ),
        (
            "ui.settings_page",
            words!(commands_settings_page = "Next page"),
        ),
        (
            "ui.settings_enter",
            words!(
                commands_settings_enter = "Into the page",
                note = "A key in the settings: the keyboard goes from the list of pages into the page."
            ),
        ),
        (
            "ui.settings_move",
            words!(
                commands_settings_move = "Move the record",
                note = "A key in the settings: move a record of a list up or down."
            ),
        ),
    ] {
        declare::<Delta>(app, Topic::Window, name, title, key_in_settings());
    }
    declare::<Adjust>(
        app,
        Topic::Window,
        "ui.settings_adjust",
        words!(commands_settings_adjust = "Change the value"),
        key_in_settings(),
    );
    none(
        Topic::Skwir,
        "ui.quit",
        words!(commands_quit = "Quit"),
        words!(
            commands_quit_description =
                "Quit, finishing what runs without the window or stopping it"
        ),
    );
    none(
        Topic::Tabs,
        "ui.close_window",
        words!(commands_close_window = "Close window"),
        words!(
            commands_close_window_description = "Close this window and its tabs, which Reopen closed tab brings back; the last window closes as its close button does, what runs going on without it"
        ),
    );
    none(
        Topic::Tabs,
        "ui.maximize_window",
        words!(commands_maximize_window = "Maximize window"),
        words!(
            commands_maximize_window_description =
                "Maximize this window, or give it back the size it had when it is maximized"
        ),
    );
    none(
        Topic::Tabs,
        "ui.minimize_window",
        words!(commands_minimize_window = "Minimize window"),
        words!(commands_minimize_window_description = "Hide this window until it is chosen again"),
    );
    // The preview.
    none(
        Topic::View,
        "ui.toggle_preview",
        words!(commands_toggle_preview = "Toggle preview"),
        words!(
            commands_toggle_preview_description = "Show or hide the preview in the side panel; the panel stays while Properties is shown"
        ),
    );
    none(
        Topic::Preview,
        "ui.preview_close",
        words!(commands_preview_close = "Preview: close"),
        words!(
            commands_preview_close_description =
                "Close peek, or hide the preview in the side panel"
        ),
    );
    none(
        Topic::Files,
        "ui.properties",
        words!(commands_properties = "Properties"),
        words!(
            commands_properties_description = "Show or hide the properties of the selection (or of the folder shown) in the side panel, over the preview or alone; the panel stays while the preview is shown"
        ),
    );
    declare::<Count>(
        app,
        Topic::Files,
        "ui.properties_count",
        words!(commands_properties_count = "Properties: count what folders hold"),
        words!(
            commands_properties_count_description = "Count the size, room taken, files and folders of what Properties shows (the selection, with several selected), or stop counting with `stop`",
            note = "Read by agents and in F1's list of actions. What is in backquotes is written as it is: the command's arguments and the values they take."
        ),
    );
    declare::<Fold>(
        app,
        Topic::Files,
        "ui.properties_fold",
        words!(
            commands_properties_fold = "Properties: fold a group",
            note = "A window command's title: fold one of the groups of Properties (Permissions, Checksums)."
        ),
        words!(
            commands_properties_fold_description = "Fold one of Properties' groups, by its key (`Checksums`, `Volume`…), or unfold it, for every item from now on",
            note = "Read by agents and in F1's list of actions. `Checksums` and `Volume` are the keys of two of Properties' groups, written as they are in every language."
        ),
    );
    declare::<Popup>(
        app,
        Topic::Preview,
        "ui.preview_menu",
        words!(commands_preview_menu = "Preview menu"),
        words!(commands_preview_menu_description = "Open or close one of the preview's menus"),
    );
    declare::<Submenu>(
        app,
        Topic::Preview,
        "ui.toolbar_submenu",
        words!(commands_toolbar_submenu = "Preview menu: submenu"),
        words!(
            commands_toolbar_submenu_description = "Open a submenu of the preview's More or the View menu beside its row (`open`, or `here`: the one whose row has the keyboard), go back to its row (`back`), move the keyboard along its rows (`step`, `end`), or choose the value before (`value` -1), after (1) or the next round (0) on a setting's row",
            note = "Read by agents and in F1's list of actions. What is in backquotes is written as it is: the command's arguments and the values they take."
        ),
    );
    none(
        Topic::Preview,
        "ui.preview_open_external",
        words!(commands_preview_open_external = "Preview: open externally"),
        words!(
            commands_preview_open_external_description =
                "Open the file shown with the desktop's application"
        ),
    );
    declare::<Object>(
        app,
        Topic::Preview,
        "ui.preview_invoke",
        words!(commands_preview_invoke = "Preview: run with what is shown"),
        words!(
            commands_preview_invoke_description = "Run `command` with the file the preview shows, the page it is on (`page`) and what is selected in it (`selection`) added to its `args`: what a plugin's items of the preview's menu and its keys there run",
            note = "Read by agents and in F1's list of actions. What is in backquotes is written as it is: the command's arguments and the values they take."
        ),
    );
    none(
        Topic::Preview,
        "ui.preview_copy_path",
        words!(commands_preview_copy_path = "Preview: copy path"),
        words!(commands_preview_copy_path_description = "Copy the path of the file shown"),
    );
    declare::<Delta>(
        app,
        Topic::Preview,
        "ui.preview_page",
        words!(commands_preview_page = "Preview: next page"),
        words!(commands_preview_page_description = "Go a page, a section or a chapter on or back"),
    );
    declare::<PreviewScroll>(
        app,
        Topic::Preview,
        "ui.preview_scroll",
        words!(commands_preview_scroll = "Preview: scroll"),
        words!(
            commands_preview_scroll_description = "Move what the preview shows a step up, down or sideways; where it fits, Left and Right go to the page or section before or after, and on the whole screen to the picture"
        ),
    );
    declare::<Amount>(
        app,
        Topic::Preview,
        "ui.preview_seek",
        words!(commands_preview_seek = "Preview: seek"),
        words!(commands_preview_seek_description = "Go seconds on or back in what plays"),
    );
    declare::<Dir>(
        app,
        Topic::Preview,
        "ui.preview_zoom",
        words!(commands_preview_zoom = "Preview: zoom"),
        words!(
            commands_preview_zoom_description = "Zoom in or out of what the preview shows (a picture, a page, a map, a model), or make larger or smaller the text of what it reads (a book, a document)"
        ),
    );
    declare::<Object>(
        app,
        Topic::Preview,
        "ui.inspect",
        words!(
            commands_inspect = "Preview action",
            note = "A window command's title: something done in a structured preview (search, copy, outline, source, hex)."
        ),
        words!(
            commands_inspect_description = "Do something in a structured preview (`action`: search, copy, outline, source, hex…)",
            note = "Read by agents and in F1's list of actions. What is in backquotes is written as it is: the command's arguments and the values they take."
        ),
    );
    declare::<On>(
        app,
        Topic::Preview,
        "ui.printed_pages",
        words!(commands_printed_pages = "Printed page numbers"),
        words!(
            commands_printed_pages_description =
                "Count a book's pages as printed, where it lists them"
        ),
    );
    declare::<On>(
        app,
        Topic::Preview,
        "ui.wrap_lines",
        words!(commands_wrap_lines = "Wrap lines"),
        words!(
            commands_wrap_lines_description =
                "Wrap a text's long lines to the panel, or let them go on sideways"
        ),
    );
    declare::<On>(
        app,
        Topic::Preview,
        "ui.text_continuous",
        words!(commands_text_continuous = "Continuous scrolling of text"),
        words!(
            commands_text_continuous_description = "Read a book, a document or a message as one scrolling column, or a screen at a time"
        ),
    );
    none(
        Topic::Preview,
        "ui.full_width",
        words!(commands_full_width = "Toggle full-width reading"),
        words!(commands_full_width_description = "Read text across the whole panel"),
    );
    declare::<Object>(
        app,
        Topic::Notes,
        "ui.note",
        words!(
            commands_note = "Note",
            note = "A window command's title: a book's note (a footnote), closed or gone to."
        ),
        words!(commands_note_description = "Close a book's note, or go to where it is"),
    );
    none(
        Topic::Preview,
        "ui.reader_continuous",
        words!(commands_reader_continuous = "Document: toggle continuous scrolling"),
        words!(
            commands_reader_continuous_description =
                "Scroll a document's pages as one, or one at a time"
        ),
    );
    none(
        Topic::Preview,
        "ui.reader_drag",
        words!(commands_reader_drag = "Document: toggle dragging to move the page"),
        words!(
            commands_reader_drag_description =
                "Drag across a page's text to move the page, or to select the text"
        ),
    );
    declare::<Pointer>(
        app,
        Topic::Preview,
        "ui.reader_pointer",
        words!(commands_reader_pointer = "Document: pointer"),
        words!(
            commands_reader_pointer_description =
                "A press, a drag or a release on a document's page"
        ),
    );
    declare::<Page>(
        app,
        Topic::Preview,
        "ui.reader_page",
        words!(commands_reader_page = "Document: go to page"),
        words!(commands_reader_page_description = "Show a document's page"),
    );
    none(
        Topic::Preview,
        "ui.preview_goto",
        words!(commands_preview_goto = "Preview: go to page or chapter…"),
        words!(commands_preview_goto_description = "Type the page to show, or a book's chapter"),
    );
    none(
        Topic::Preview,
        "ui.reader_page_submit",
        words!(commands_reader_page_submit = "Document: show the page typed"),
        words!(commands_reader_page_submit_description = "Show the page typed"),
    );
    none(
        Topic::Preview,
        "ui.reader_page_cancel",
        words!(commands_reader_page_cancel = "Document: leave the page field"),
        words!(commands_reader_page_cancel_description = "Leave the page field as it was"),
    );
    // What a document and a structured preview are both asked, one
    // command each, carried out as the one shown does it.
    none(
        Topic::Preview,
        "ui.preview_copy",
        words!(commands_preview_copy = "Preview: copy"),
        words!(
            commands_preview_copy_description = "Copy the text selected, else what is selected (a block), else all the preview shows"
        ),
    );
    none(
        Topic::Preview,
        "ui.preview_select_all",
        words!(commands_preview_select_all = "Preview: select all"),
        words!(commands_preview_select_all_description = "Select all the text the preview shows"),
    );
    none(
        Topic::Preview,
        "ui.preview_contents",
        words!(commands_preview_contents = "Preview: contents"),
        words!(
            commands_preview_contents_description =
                "Show or hide the contents: a document's, a structured preview's headings"
        ),
    );
    declare::<On>(
        app,
        Topic::Preview,
        "ui.contents_pin",
        words!(commands_contents_pin = "Preview: keep contents open"),
        words!(
            commands_contents_pin_description = "Keep the contents open beside what is read, the content narrowed, or let choosing a part close them"
        ),
    );
    none(
        Topic::Preview,
        "ui.preview_find",
        words!(commands_preview_find = "Preview: find text"),
        words!(commands_preview_find_description = "Find text in what the preview shows"),
    );
    none(
        Topic::Preview,
        "ui.preview_find_close",
        words!(commands_preview_find_close = "Preview: close find"),
        words!(commands_preview_find_close_description = "Close the preview's find field"),
    );
    declare::<Delta>(
        app,
        Topic::Preview,
        "ui.preview_match",
        words!(commands_preview_match = "Preview: next search match"),
        words!(
            commands_preview_match_description =
                "Go to the next or previous match of what the find looks for"
        ),
    );
    none(
        Topic::Preview,
        "ui.reader_search",
        words!(commands_reader_search = "Document: search"),
        words!(
            commands_reader_search_description = "Search a document for what the find field holds"
        ),
    );
    declare::<PageLink>(
        app,
        Topic::Preview,
        "ui.reader_link",
        words!(commands_reader_link = "Document: follow a link"),
        words!(commands_reader_link_description = "Follow one of a page's links"),
    );
    // What plays.
    declare::<Object>(
        app,
        Topic::Player,
        "ui.media_control",
        words!(commands_media_control = "Player: control"),
        words!(
            commands_media_control_description =
                "Play, pause, seek or set the volume of what the preview plays"
        ),
    );
    none(
        Topic::Player,
        "ui.media_play",
        words!(commands_media_play = "Player: play the selection"),
        words!(
            commands_media_play_description = "Play what is selected without opening the preview: music in the player at the foot of the panel, the tracks selected in their order (a playlist's tracks in its place, a track alone with the folder's around it); a film in the quick preview"
        ),
    );
    none(
        Topic::Player,
        "ui.media_pin",
        words!(
            commands_media_pin = "Player: pin or let go of this track",
            note = "A window command's title: keep what plays playing while the selection moves, or let it go."
        ),
        words!(
            commands_media_pin_description =
                "Keep what plays playing while the selection moves, or let it go"
        ),
    );
    declare::<On>(
        app,
        Topic::Player,
        "ui.media_keep_playing",
        words!(commands_media_keep_playing = "Player: keep playing while you browse"),
        words!(commands_media_keep_playing_description = "Keep playing when the selection moves"),
    );
    declare::<On>(
        app,
        Topic::Player,
        "ui.media_autoplay",
        words!(commands_media_autoplay = "Player: play on selection"),
        words!(commands_media_autoplay_description = "Play what is selected at once"),
    );
    declare::<Continue>(
        app,
        Topic::Player,
        "ui.media_continue",
        words!(commands_media_continue = "Player: when a track ends"),
        words!(
            commands_media_continue_description =
                "What plays when a track ends: nothing, the next one, or one shuffled"
        ),
    );
    declare::<Delta>(
        app,
        Topic::Player,
        "ui.media_step",
        words!(commands_media_step = "Player: next track"),
        words!(commands_media_step_description = "Select the next or previous track"),
    );
    declare::<Scrub>(
        app,
        Topic::Player,
        "ui.media_scrub",
        words!(
            commands_media_scrub = "Player: scrub",
            note = "A window command's title: drag the playing position."
        ),
        words!(commands_media_scrub_description = "Drag the playing position"),
    );
    none(
        Topic::Player,
        "ui.media_escape",
        words!(
            commands_media_escape = "Player: leave",
            note = "A window command's title: leave full screen, else close the preview."
        ),
        words!(commands_media_escape_description = "Leave full screen, else close the preview"),
    );
    none(
        Topic::Preview,
        "ui.preview_fullscreen",
        words!(commands_preview_fullscreen = "Preview: full screen"),
        words!(
            commands_preview_fullscreen_description = "Show the preview on the whole screen (a picture, pages, text, a film), or leave it"
        ),
    );
    none(
        Topic::Preview,
        "ui.fullscreen_controls",
        words!(commands_fullscreen_controls = "Full screen: controls"),
        words!(
            commands_fullscreen_controls_description =
                "Show or hide the bar over the preview on the whole screen"
        ),
    );
    declare::<Delta>(
        app,
        Topic::Preview,
        "ui.fullscreen_step",
        words!(commands_fullscreen_step = "Full screen: next"),
        words!(
            commands_fullscreen_step_description = "On the whole screen, show the next or previous picture of the pane, or turn the page"
        ),
    );
    none(
        Topic::Preview,
        "ui.slideshow",
        words!(commands_slideshow = "Preview: slideshow"),
        words!(
            commands_slideshow_description = "Play the pane's pictures one after another on the whole screen, from the one shown, or pause"
        ),
    );
    declare::<Pace>(
        app,
        Topic::Preview,
        "ui.slideshow_pace",
        words!(commands_slideshow_pace = "Slideshow: pace"),
        words!(
            commands_slideshow_pace_description =
                "How long a slideshow shows each picture, in seconds"
        ),
    );
    declare::<Player>(
        app,
        Topic::Player,
        "ui.player",
        words!(
            commands_player = "Player",
            note = "A window command's title: the player pinned at the foot of the side panel."
        ),
        words!(
            commands_player_description = "Ask the pinned player what `request` says: `play`, `pause`, `next`, `stop`, `seek_by` some seconds…",
            note = "Read by agents and in F1's list of actions. What is in backquotes is written as it is: the command's argument and the values it takes."
        ),
    );
    declare::<Amount>(
        app,
        Topic::Player,
        "ui.player_volume",
        words!(commands_player_volume = "Player: volume"),
        words!(commands_player_volume_description = "Change the pinned player's volume"),
    );
    declare::<Amount>(
        app,
        Topic::Player,
        "ui.volume_step",
        words!(commands_volume_step = "Volume: step"),
        words!(
            commands_volume_step_description =
                "Step the volume of the player whose volume popup is open"
        ),
    );
    // The map.
    declare::<Pan>(
        app,
        Topic::Preview,
        "ui.map_pan",
        words!(commands_map_pan = "Map: pan"),
        words!(commands_map_pan_description = "Move the map"),
    );
    // A 3D model (ADR-067).
    declare::<On>(
        app,
        Topic::Preview,
        "ui.model_perspective",
        words!(commands_model_perspective = "Model: perspective"),
        words!(
            commands_model_perspective_description =
                "Draw 3D models in perspective, nearer parts larger, or not"
        ),
    );
    declare::<On>(
        app,
        Topic::Preview,
        "ui.model_smooth",
        words!(commands_model_smooth = "Model: smooth shading"),
        words!(
            commands_model_smooth_description =
                "Shade 3D models smoothly across their faces, or each face flat"
        ),
    );
    declare::<Pan>(
        app,
        Topic::Preview,
        "ui.model_turn",
        words!(commands_model_turn = "Model: turn"),
        words!(
            commands_model_turn_description =
                "Go round the 3D model shown, by steps right (dx) and down (dy)"
        ),
    );
    declare::<Pan>(
        app,
        Topic::Preview,
        "ui.model_pan",
        words!(commands_model_pan = "Model: move"),
        words!(
            commands_model_pan_description =
                "Move the view of the 3D model shown, by steps right (dx) and down (dy)"
        ),
    );
    declare::<Delta>(
        app,
        Topic::Preview,
        "ui.map_point",
        words!(commands_map_point = "Map: next place"),
        words!(
            commands_map_point_description =
                "Go to the next place on the map (or the previous one), the map centred on it"
        ),
    );
    none(
        Topic::Preview,
        "ui.map_open",
        words!(commands_map_open = "Map: open in Maps"),
        words!(commands_map_open_description = "Open the place shown in the desktop's maps"),
    );
    none(
        Topic::Preview,
        "ui.map_view_fit",
        words!(commands_map_view_fit = "Map: fit to its files"),
        words!(commands_map_view_fit_description = "Fit the Map view to its files"),
    );
    none(
        Topic::Preview,
        "ui.map_view_spread",
        words!(commands_map_view_spread = "Map: spread the files at one place, or group them"),
        words!(
            commands_map_view_spread_description = "Spread the files at one place, or group them"
        ),
    );
    none(
        Topic::Preview,
        "ui.map_view_search_area",
        words!(commands_map_view_search_area = "Map: search the pictures and videos of this area"),
        words!(
            commands_map_view_search_area_description =
                "Search the pictures and videos taken in the area shown"
        ),
    );
}

#[cfg(test)]
mod tests {
    use skwir_core::command::{CommandError, Effect, Invocation};

    /// A text size goes in, out or back to 100 %: any other way is refused,
    /// the refusal naming the three, never taken for a reset.
    #[test]
    fn a_text_size_s_way_is_in_out_or_reset() {
        let tmp = tempfile::tempdir().unwrap();
        let app = skwir_core::App::new(skwir_core::app::AppConfig::ephemeral(tmp.path())).unwrap();
        super::declare_all(&app);
        let check = |args: serde_json::Value| {
            app.commands.check(&Invocation {
                name: "ui.text_size".into(),
                args,
            })
        };
        for dir in ["in", "out", "reset"] {
            assert!(check(serde_json::json!({"part": "reading", "dir": dir})).is_ok());
        }
        assert!(check(serde_json::json!({"part": "files"})).is_ok(), "reset");
        let refused = check(serde_json::json!({"part": "reading", "dir": "larger"}))
            .expect_err("a way not known")
            .to_string();
        for named in ["larger", "`in`", "`out`", "`reset`"] {
            assert!(refused.contains(named), "{refused}");
        }
    }

    /// The lists of what an agent may run name only commands declared,
    /// and what answers the user's own dialogs is never an agent's.
    #[test]
    fn what_an_agent_may_run_of_the_window_is_named_and_the_rest_is_the_users() {
        let tmp = tempfile::tempdir().unwrap();
        let app = skwir_core::App::new(skwir_core::app::AppConfig::ephemeral(tmp.path())).unwrap();
        super::declare_all(&app);
        for name in super::SHOWS.iter().chain(super::ACTS) {
            assert!(app.commands.get(name).is_some(), "{name} is declared");
        }
        let effect = |name: &str| app.commands.get(name).unwrap().effect;
        for answer in [
            "ui.review_apply",
            "ui.review_cancel",
            "ui.accept",
            "ui.dismiss",
            "ui.menu_activate",
            "ui.toolbar_activate",
            "ui.copy_other",
            "ui.move_other",
            "ui.give_other",
            "ui.settings_adjust",
            // What a field the user types in takes, written there by its
            // keys (ADR-137): the user's hand.
            "ui.field_reference",
            "ui.field_complete",
        ] {
            assert_eq!(effect(answer), Effect::UserOnly, "{answer}");
        }
        assert_eq!(effect("ui.cursor"), Effect::Shows);
        assert_eq!(effect("ui.maximize_window"), Effect::Shows);
        assert_eq!(effect("ui.minimize_window"), Effect::Shows);
        assert_eq!(effect("ui.open_url"), Effect::Acts);
    }

    /// What a command that shows would do beyond the window by its
    /// arguments is refused to an agent, which is pointed at the command
    /// that asks; the rest of it waits for the window, and the user's own
    /// call does it all.
    #[test]
    fn an_agent_never_opens_an_address_or_copies_through_a_command_that_shows() {
        let tmp = tempfile::tempdir().unwrap();
        let app = skwir_core::App::new(skwir_core::app::AppConfig::ephemeral(tmp.path())).unwrap();
        super::declare_all(&app);
        let inspect = |args: serde_json::Value| Invocation::with_args("ui.inspect", &args);
        let as_agent = |inv: &Invocation| {
            skwir_core::permissions::as_agent(app.privacy(), || app.dispatch(inv))
        };
        for (args, instead) in [
            (
                serde_json::json!({"action": "link", "target": "https://example.org/it%27s%20here"}),
                "ui.open_url",
            ),
            (
                serde_json::json!({"action": "link", "target": "mailto:someone@example.org"}),
                "ui.open_url",
            ),
            (serde_json::json!({"action": "copy"}), "ui.preview_copy"),
            (
                serde_json::json!({"action": "label_key", "key": "a"}),
                "target",
            ),
        ] {
            let error = as_agent(&inspect(args.clone())).expect_err(&args.to_string());
            assert!(error.to_string().contains(instead), "{args}: {error}");
        }
        assert!(
            app.take_front_requests().is_empty(),
            "nothing reached the window"
        );
        // Within the document, or a file beside it: shown.
        for target in ["#chapter’s end", "notes/it's here.html"] {
            let args = serde_json::json!({"action": "link", "target": target});
            as_agent(&inspect(args)).unwrap();
        }
        as_agent(&inspect(
            serde_json::json!({"action": "scroll", "to": "down"}),
        ))
        .unwrap();
        assert_eq!(app.take_front_requests().len(), 3);
        // The user's own call (a key, a script) is the user's.
        let address = serde_json::json!({"action": "link", "target": "https://example.org/"});
        app.dispatch(&inspect(address)).unwrap();
        assert_eq!(app.take_front_requests().len(), 1);
    }

    /// What a question says as it is typed runs before anyone answers:
    /// only a command that answers is taken (ADR-137), whoever asks, and an
    /// agent's question says it as the agent's call.
    #[test]
    fn a_question_says_what_is_typed_comes_to_only_by_a_command_that_answers() {
        let tmp = tempfile::tempdir().unwrap();
        let app = skwir_core::App::new(skwir_core::app::AppConfig::ephemeral(tmp.path())).unwrap();
        super::declare_all(&app);
        let ask = |describe: &str| {
            Invocation::with_args(
                "ui.ask",
                &serde_json::json!({
                    "title": "Access",
                    "command": "set_access",
                    "field": "mode",
                    "describe": { "command": describe, "args": { "folders": false } },
                }),
            )
        };
        let as_agent = |inv: &Invocation| {
            skwir_core::permissions::as_agent(app.privacy(), || app.dispatch(inv))
        };
        // One that acts (a plan, the window), or none known: refused, the
        // agent's and the user's alike, and nothing reaches the window.
        for acting in ["set_access", "ui.cursor", "no.such.command"] {
            let error = as_agent(&ask(acting)).expect_err(acting);
            assert!(error.to_string().contains(acting), "{acting}: {error}");
            app.dispatch(&ask(acting)).expect_err(acting);
        }
        assert!(app.take_front_requests().is_empty());
        // One that answers: the agent's says it as the agent's call, even
        // where the agent said otherwise; the user's as the user's.
        as_agent(&ask("set_access.describe")).unwrap();
        let mut forged = ask("set_access.describe");
        forged.args["describe"]["agent"] = false.into();
        as_agent(&forged).unwrap();
        app.dispatch(&ask("set_access.describe")).unwrap();
        let asked = app.take_front_requests();
        let agent: Vec<bool> = asked
            .iter()
            .map(|inv| inv.args["describe"]["agent"].as_bool().unwrap_or(false))
            .collect();
        assert_eq!(agent, [true, true, false]);
        // What reaches the window is what the window takes.
        for inv in &asked {
            app.commands.check(inv).unwrap();
        }
    }

    /// A window answering another program's dialog offers the shell's
    /// commands by the arguments they run with and by what they lead to
    /// (ADR-144, 1): placing the preview, not the split's panes; a
    /// listing's shape, not a lens; a picker, a prompt and an item of the
    /// preview's menu only where what they run is offered.
    #[test]
    fn a_dialog_offers_by_arguments_and_by_what_they_lead_to() {
        use serde_json::json;
        use skwir_core::command::InDialogs;
        use skwir_core::registry::ViewSpec;
        use skwir_core::words::Words;
        use std::sync::Arc;
        let tmp = tempfile::tempdir().unwrap();
        let app = skwir_core::App::new(skwir_core::app::AppConfig::ephemeral(tmp.path())).unwrap();
        super::declare_all(&app);
        let revision = Arc::default();
        let list = ViewSpec::named(
            "list",
            Words::as_written("List"),
            "list",
            Arc::clone(&revision),
        );
        let usage = ViewSpec::named("usage", Words::as_written("Usage"), "usage", revision).lens();
        app.registry.views.add("ui", list);
        app.registry.views.add("ui", usage);
        let here = |name: &str, args: serde_json::Value| app.in_dialogs(name, &args);
        assert_eq!(
            here("ui.layout", json!({"preview": "right"})),
            InDialogs::Offered
        );
        assert_eq!(
            here("ui.layout", json!({"member": "auto"})),
            InDialogs::Offered
        );
        assert_eq!(here("ui.layout", json!({"panes": "row"})), InDialogs::Not);
        assert_eq!(here("ui.view", json!({"id": "list"})), InDialogs::Offered);
        assert_eq!(here("ui.view", json!({"id": "usage"})), InDialogs::Not);
        // A picker whose choices are given leads to each of them.
        let choose = |command: &str| {
            json!({ "choices": [
                { "title": "A", "command": { "name": "ui.zoom", "args": {} } },
                { "title": "B", "command": { "name": command, "args": {} } },
            ]})
        };
        assert_eq!(here("ui.choose", choose("ui.find")), InDialogs::Offered);
        assert_eq!(here("ui.choose", choose("ui.terminal")), InDialogs::Not);
        // A prompt leads to its command, an item of the preview's menu to
        // what it runs with what is shown, through a picker too.
        let ask = |command: &str| json!({ "command": command, "field": "name" });
        assert_eq!(here("ui.ask", ask("ui.find")), InDialogs::Offered);
        assert_eq!(here("ui.ask", ask("ui.split")), InDialogs::Not);
        assert_eq!(
            here(
                "ui.preview_invoke",
                json!({ "command": "ui.open_url", "args": {} })
            ),
            InDialogs::Not
        );
        assert_eq!(
            here(
                "ui.preview_invoke",
                json!({ "command": "ui.choose", "args": choose("ui.terminal") })
            ),
            InDialogs::Not
        );
        assert_eq!(
            here(
                "ui.preview_invoke",
                json!({ "command": "ui.preview_find", "args": {} })
            ),
            InDialogs::Offered
        );
        // What opens another program is not offered whatever its topic.
        assert_eq!(here("ui.open_url", json!({})), InDialogs::Not);
        assert_eq!(here("ui.map_open", json!({})), InDialogs::Not);
    }

    /// What the shell's keys run and the palette offers is declared, with
    /// arguments the declaration takes; arguments it does not are refused.
    #[test]
    fn every_ui_command_the_shell_runs_is_declared_with_its_arguments() {
        let tmp = tempfile::tempdir().unwrap();
        let app = skwir_core::App::new(skwir_core::app::AppConfig::ephemeral(tmp.path())).unwrap();
        super::declare_all(&app);
        let offered = crate::palette::ui_commands()
            .iter()
            .map(|(name, args, _)| ((*name).to_owned(), args.clone()));
        for (name, args) in crate::keymap::shell_invocations()
            .into_iter()
            .chain(offered)
        {
            if !name.starts_with("ui.") {
                continue;
            }
            let inv = Invocation {
                name: name.as_str().into(),
                args: args.clone(),
            };
            assert!(app.commands.check(&inv).is_ok(), "{name} {args}");
        }
        // Every form names arguments its command takes, and the commands
        // that keep a key are declared so.
        for spec in app.commands.list() {
            for form in &spec.forms {
                let inv = Invocation {
                    name: spec.name.clone(),
                    args: form.args.clone(),
                };
                assert!(
                    app.commands.check(&inv).is_ok(),
                    "{} {}: {}",
                    spec.name,
                    form.args,
                    form.title.english()
                );
            }
        }
        for name in super::KEEP_A_KEY {
            assert!(
                app.commands.get(name).is_some_and(|spec| spec.keeps_a_key),
                "{name}"
            );
        }
        // What a dialog's window offers apart from its topic and effect
        // says the opposite of them (ADR-144, 1): a line that says what
        // they already say is not one.
        let none = serde_json::json!({});
        for name in super::IN_DIALOGS {
            let spec = app.commands.get(name).expect(name);
            assert!(
                app.in_dialogs(name, &none).offered() && !spec.in_dialogs_by_default(),
                "{name}"
            );
        }
        for name in super::NOT_IN_DIALOGS {
            let spec = app.commands.get(name).expect(name);
            assert!(
                !app.in_dialogs(name, &none).runs() && spec.in_dialogs_by_default(),
                "{name}"
            );
        }
        for name in super::OPENS_ELSEWHERE {
            let spec = app.commands.get(name).expect(name);
            assert!(
                spec.opens == skwir_core::command::Opens::Elsewhere
                    && !app.in_dialogs(name, &none).runs(),
                "{name}"
            );
        }
        let wrong = Invocation::with_args("ui.cursor", &serde_json::json!({ "to": "sideways" }));
        assert!(matches!(
            app.commands.check(&wrong),
            Err(CommandError::InvalidArgs { .. })
        ));
        assert!(matches!(
            app.commands.check(&Invocation::new("ui.no_such_thing")),
            Err(CommandError::Unknown(_))
        ));
        // Asked from elsewhere, it waits for the window.
        app.dispatch(&Invocation::new("ui.toggle_sidebar")).unwrap();
        let waiting = app.take_front_requests();
        assert_eq!(waiting.len(), 1);
        assert_eq!(&*waiting[0].name, "ui.toggle_sidebar");
    }
}
