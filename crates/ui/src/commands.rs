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
use skwir_core::command::{Effect, Form, Invocation, NoArgs, Outcome};

/// Who declares them.
pub const OWNER: &str = "ui";

/// The arguments of the commands, as their schemas say them. Only read
/// through serde, to check what a command is given and to describe it.
#[allow(dead_code)] // Fields are read by `Deserialize` and `JsonSchema` only.
mod args {
    use schemars::JsonSchema;
    use serde::Deserialize;
    use skwir_core::location::SortKey;
    use skwir_core::op::EntryRef;
    use skwir_core::os_path::PathArg;

    /// Arguments that depend on what the command is asked to do (its
    /// `action`, its `key`): any object.
    #[derive(Deserialize, JsonSchema)]
    #[serde(transparent)]
    pub(super) struct Object(serde_json::Map<String, serde_json::Value>);

    /// A step along something: `1` forward, `-1` back.
    #[derive(Deserialize, JsonSchema)]
    pub(super) struct Delta {
        #[serde(default)]
        delta: Option<i64>,
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

    #[derive(Deserialize, JsonSchema)]
    pub(super) struct Sort {
        /// What to sort by; the direction alone when absent.
        #[serde(default)]
        key: Option<SortKey>,
        #[serde(default)]
        ascending: Option<bool>,
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
        /// `location`'s old name.
        #[serde(default)]
        path: Option<PathArg>,
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

    #[derive(Deserialize, JsonSchema)]
    pub(super) struct Submit {
        /// Search the home folder instead of where the pill says.
        #[serde(default)]
        everywhere: bool,
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
        /// What it is, for the message ("the path").
        #[serde(default)]
        what: Option<String>,
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
    }

    #[derive(Deserialize, JsonSchema)]
    pub(super) struct Pages {
        #[serde(default)]
        pages: Option<i64>,
    }

    #[derive(Deserialize, JsonSchema)]
    pub(super) struct Unlock {
        /// The locked container's device.
        device: String,
        /// What the question calls it.
        #[serde(default)]
        title: Option<String>,
        /// Open the volume in a tab behind the one shown.
        #[serde(default)]
        tab: bool,
    }

    #[derive(Deserialize, JsonSchema)]
    pub(super) struct Ask {
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        placeholder: Option<String>,
        /// The answer to start from.
        #[serde(default)]
        text: Option<String>,
        /// The part of `text` selected at first, `[start, end]` in bytes.
        #[serde(default)]
        select: Option<[usize; 2]>,
        /// What runs with the answer.
        command: String,
        #[serde(default)]
        args: Option<serde_json::Value>,
        /// The argument of `command` the answer is.
        field: String,
    }

    #[derive(Deserialize, JsonSchema)]
    pub(super) struct Settings {
        /// A setting to show (`plugin.option`).
        #[serde(default)]
        key: Option<String>,
        /// A page to show (`plugins`, a plugin's id).
        #[serde(default)]
        page: Option<String>,
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
    Amount, Ask, Bucket, Close, Continue, CopyText, Cursor, Delta, Dir, Find, Folders, History,
    Key, Layout, ListMoveArgs, Object, On, Page, PageLink, Pages, Pan, Pick, Player, Pointer,
    Popup, Scrub, Settings, Sort, Split, Submit, TagsPrompt, Text, Unlock, Url, UsageMode, View,
};

/// The window's commands an agent may run (ADR-044): they change only
/// what the window shows, or open a prompt the user answers.
const SHOWS: &[&str] = &[
    "ui.cursor",
    "ui.cursor_pick",
    "ui.cursor_toggle",
    "ui.context_menu",
    "ui.open_in_tab",
    "ui.find",
    "ui.find_next",
    "ui.sort",
    "ui.view",
    "ui.columns",
    "ui.columns_reset",
    "ui.zoom",
    "ui.grid_fill",
    "ui.narrow_folders",
    "ui.toggle_hidden",
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
    "ui.toggle_sidebar",
    "ui.toolbar_menu",
    "ui.toolbar_close",
    "ui.toolbar_next",
    "ui.toolbar_prev",
    "ui.toolbar_first",
    "ui.toolbar_last",
    "ui.split",
    "ui.layout",
    "ui.timeline",
    "ui.timeline_bucket",
    "ui.timeline_subtree",
    "ui.terminal",
    "ui.terminal_page",
    "ui.terminal_links",
    "ui.menu_up",
    "ui.menu_down",
    "ui.menu_first",
    "ui.menu_last",
    "ui.menu_expand",
    "ui.menu_collapse",
    "ui.menu_close",
    "ui.toggle_preview",
    "ui.preview_close",
    "ui.preview_details",
    "ui.preview_menu",
    "ui.preview_page",
    "ui.preview_seek",
    "ui.preview_zoom",
    "ui.inspect",
    "ui.printed_pages",
    "ui.wrap_lines",
    "ui.full_width",
    "ui.note",
    "ui.reader_continuous",
    "ui.reader_page",
    "ui.preview_goto",
    "ui.reader_page_submit",
    "ui.reader_page_cancel",
    "ui.preview_select_all",
    "ui.preview_contents",
    "ui.preview_find",
    "ui.preview_find_close",
    "ui.preview_match",
    "ui.reader_search",
    "ui.media_control",
    "ui.media_pin",
    "ui.media_keep_playing",
    "ui.media_autoplay",
    "ui.media_continue",
    "ui.media_step",
    "ui.media_scrub",
    "ui.media_fullscreen",
    "ui.media_controls",
    "ui.media_escape",
    "ui.player",
    "ui.player_volume",
    "ui.map_pan",
    "ui.map_view_fit",
    "ui.map_view_spread",
    "ui.map_view_search_area",
];

/// The window's commands an agent's call runs only once the user allows
/// it: they reach beyond the window (a program, the clipboard, an
/// address), change how the sidebar is arranged, or prompt the user for
/// a command the agent named.
const ACTS: &[&str] = &[
    "ui.ask",
    "ui.choose",
    "ui.unlock_prompt",
    "ui.open_url",
    "ui.preview_open_external",
    "ui.map_open",
    "ui.copy_text",
    "ui.preview_copy",
    "ui.preview_copy_path",
    "ui.terminal_copy",
    "ui.reader_link",
    "ui.sidebar_place",
    "ui.sidebar_section",
    "ui.quit",
];

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
/// the palette offers (`palette::UI_COMMANDS`): the command, its arguments
/// and the title they make, which every list of keys says (ADR-024). The
/// cursor's moves are made from their words (`cursor_forms`).
const KEY_FORMS: &[(&str, &str, &str)] = &[
    // The lists one picks from while typing, and the names a copy found
    // taken.
    ("ui.list_move", r#"{"delta":-1}"#, "Previous in the list"),
    ("ui.list_move", r#"{"delta":1}"#, "Next in the list"),
    ("ui.list_move", r#"{"to":"page_up"}"#, "A page up the list"),
    (
        "ui.list_move",
        r#"{"to":"page_down"}"#,
        "A page down the list",
    ),
    ("ui.review_choose", r#"{"delta":-1}"#, "Previous choice"),
    ("ui.review_choose", r#"{"delta":1}"#, "Next choice"),
    // The file list.
    (
        "ui.cursor_pick",
        "{}",
        "Pick or unpick, then go to the next",
    ),
    ("ui.cursor_pick", r#"{"advance":false}"#, "Pick or unpick"),
    (
        "ui.search",
        r#"{"text":"content:"}"#,
        "Search the contents…",
    ),
    (
        "ui.omnibox_submit",
        r#"{"everywhere":true}"#,
        "Search everywhere",
    ),
    ("ui.about", r#"{"close":true}"#, "Close About"),
    // The sidebar, walked as the file list's rows.
    ("ui.sidebar_key", r#"{"key":"up"}"#, "Go to the place above"),
    (
        "ui.sidebar_key",
        r#"{"key":"down"}"#,
        "Go to the place below",
    ),
    (
        "ui.sidebar_key",
        r#"{"key":"first"}"#,
        "Go to the first place",
    ),
    (
        "ui.sidebar_key",
        r#"{"key":"last"}"#,
        "Go to the last place",
    ),
    (
        "ui.sidebar_key",
        r#"{"key":"left"}"#,
        "Go to its group, or fold the group",
    ),
    (
        "ui.sidebar_key",
        r#"{"key":"right"}"#,
        "Unfold the group, or go into it",
    ),
    (
        "ui.sidebar_key",
        r#"{"key":"open"}"#,
        "Open the place, or fold the group",
    ),
    (
        "ui.sidebar_key",
        r#"{"key":"tab"}"#,
        "Open the place in a new tab",
    ),
    ("ui.sidebar_key", r#"{"key":"menu"}"#, "Open its menu"),
    ("ui.sidebar_key", r#"{"key":"move_up"}"#, "Move it up"),
    ("ui.sidebar_key", r#"{"key":"move_down"}"#, "Move it down"),
    ("ui.sidebar_key", r#"{"key":"rename"}"#, "Rename it"),
    ("ui.sidebar_key", r#"{"key":"remove"}"#, "Remove or hide it"),
    // What the preview plays, and the pinned player.
    ("ui.media_control", r#"{"action":"play"}"#, "Play or pause"),
    ("ui.media_control", r#"{"action":"mute"}"#, "Mute or unmute"),
    (
        "ui.media_control",
        r#"{"action":"volume_by","value":0.05}"#,
        "Volume up",
    ),
    (
        "ui.media_control",
        r#"{"action":"volume_by","value":-0.05}"#,
        "Volume down",
    ),
    (
        "ui.player",
        r#"{"request":{"seek_by":-5.0}}"#,
        "Player: back five seconds",
    ),
    (
        "ui.player",
        r#"{"request":{"seek_by":5.0}}"#,
        "Player: forward five seconds",
    ),
    (
        "ui.player",
        r#"{"request":"previous"}"#,
        "Player: previous track",
    ),
    ("ui.player", r#"{"request":"next"}"#, "Player: next track"),
    (
        "ui.player",
        r#"{"request":"mute"}"#,
        "Player: mute or unmute",
    ),
    (
        "ui.player",
        r#"{"request":"fullscreen"}"#,
        "Player: full screen",
    ),
    ("ui.player_volume", r#"{"delta":0.05}"#, "Player: volume up"),
    (
        "ui.player_volume",
        r#"{"delta":-0.05}"#,
        "Player: volume down",
    ),
    // A structured preview: the files of an archive, a text read where it
    // is shown, its links, its size.
    (
        "ui.inspect",
        r#"{"action":"step","delta":-1}"#,
        "Previous file",
    ),
    ("ui.inspect", r#"{"action":"step","delta":1}"#, "Next file"),
    (
        "ui.inspect",
        r#"{"action":"fold","delta":-1}"#,
        "Fold the folder, or go to it",
    ),
    (
        "ui.inspect",
        r#"{"action":"fold","delta":1}"#,
        "Unfold the folder",
    ),
    ("ui.inspect", r#"{"action":"back"}"#, "Close the file shown"),
    (
        "ui.inspect",
        r#"{"action":"scroll","to":"up"}"#,
        "Scroll up",
    ),
    (
        "ui.inspect",
        r#"{"action":"scroll","to":"down"}"#,
        "Scroll down",
    ),
    (
        "ui.inspect",
        r#"{"action":"scroll","to":"page_up"}"#,
        "Scroll a page up",
    ),
    (
        "ui.inspect",
        r#"{"action":"scroll","to":"page_down"}"#,
        "Scroll a page down",
    ),
    (
        "ui.inspect",
        r#"{"action":"scroll","to":"start"}"#,
        "Scroll to the start",
    ),
    (
        "ui.inspect",
        r#"{"action":"scroll","to":"end"}"#,
        "Scroll to the end",
    ),
    (
        "ui.inspect",
        r#"{"action":"links_close"}"#,
        "Stop labelling the links",
    ),
    (
        "ui.inspect",
        r#"{"action":"larger"}"#,
        "Preview: larger text",
    ),
    (
        "ui.inspect",
        r#"{"action":"smaller"}"#,
        "Preview: smaller text",
    ),
    (
        "ui.inspect",
        r#"{"action":"reset"}"#,
        "Preview: text at its size",
    ),
    ("ui.map_pan", r#"{"dx":-1,"dy":0}"#, "Map: move left"),
    ("ui.map_pan", r#"{"dx":1,"dy":0}"#, "Map: move right"),
    ("ui.map_pan", r#"{"dx":0,"dy":-1}"#, "Map: move up"),
    ("ui.map_pan", r#"{"dx":0,"dy":1}"#, "Map: move down"),
    ("ui.terminal_page", r#"{"pages":1}"#, "Terminal: a page up"),
    (
        "ui.terminal_page",
        r#"{"pages":-1}"#,
        "Terminal: a page down",
    ),
    // The settings, and the Keyboard page's list of actions.
    (
        "ui.settings_key",
        r#"{"key":"up"}"#,
        "Go to the action above",
    ),
    (
        "ui.settings_key",
        r#"{"key":"down"}"#,
        "Go to the action below",
    ),
    (
        "ui.settings_key",
        r#"{"key":"first"}"#,
        "Go to the first action",
    ),
    (
        "ui.settings_key",
        r#"{"key":"last"}"#,
        "Go to the last action",
    ),
    (
        "ui.settings_key",
        r#"{"key":"page_up"}"#,
        "Go a page up the actions",
    ),
    (
        "ui.settings_key",
        r#"{"key":"page_down"}"#,
        "Go a page down the actions",
    ),
    (
        "ui.settings_key",
        r#"{"key":"left"}"#,
        "Go to the key on the left",
    ),
    (
        "ui.settings_key",
        r#"{"key":"right"}"#,
        "Go to the key on the right",
    ),
    (
        "ui.settings_key",
        r#"{"key":"open"}"#,
        "Change the key, or add one",
    ),
    ("ui.settings_key", r#"{"key":"remove"}"#, "Remove the key"),
    ("ui.settings_page", r#"{"delta":-1}"#, "Previous page"),
    ("ui.settings_page", r#"{"delta":1}"#, "Next page"),
    ("ui.settings_move", r#"{"delta":-1}"#, "Move the record up"),
    ("ui.settings_move", r#"{"delta":1}"#, "Move the record down"),
    (
        "ui.settings_adjust",
        r#"{"delta":-1}"#,
        "Less, or the previous choice",
    ),
    (
        "ui.settings_adjust",
        r#"{"delta":1}"#,
        "More, or the next choice",
    ),
];

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

/// The cursor's moves (`ui.cursor`), each where it goes and what the
/// selection does, in words: Select up, Extend the selection a page down.
fn cursor_forms() -> impl Iterator<Item = Form> {
    const TO: [(&str, &str); 13] = [
        ("up", "up"),
        ("down", "down"),
        ("left", "left"),
        ("right", "right"),
        ("page_up", "a page up"),
        ("page_down", "a page down"),
        ("half_page_up", "half a page up"),
        ("half_page_down", "half a page down"),
        ("home", "to the first item"),
        ("end", "to the last item"),
        ("here", "to the item under the cursor"),
        ("previous", "to the previous item"),
        ("next", "to the next item"),
    ];
    const STEP: [(Option<&str>, &str); 5] = [
        (None, "Select"),
        (Some("select"), "Select"),
        (Some("extend"), "Extend the selection"),
        (Some("move"), "Move the cursor"),
        (Some("follow"), "Go"),
    ];
    STEP.into_iter().flat_map(|(step, verb)| {
        TO.into_iter().map(move |(to, words)| {
            let args = match step {
                Some(step) => serde_json::json!({ "to": to, "step": step }),
                None => serde_json::json!({ "to": to }),
            };
            let title = if verb == "Select" && to == "here" {
                "Select only the item under the cursor".to_owned()
            } else {
                format!("{verb} {words}")
            };
            Form::new(args, title)
        })
    })
}

/// Every form of the shell's command `name`: what the palette offers, then
/// what its places' keys run.
fn forms_of(name: &str) -> Vec<Form> {
    let offered = crate::palette::ui_commands()
        .iter()
        .filter(|(n, _, _)| *n == name)
        .map(|(_, args, title)| Form::new(args.clone(), *title));
    let keys = KEY_FORMS
        .iter()
        .filter(|(n, _, _)| *n == name)
        .map(|(_, args, title)| {
            Form::new(serde_json::from_str(args).expect("valid form args"), *title)
        });
    let moves = (name == "ui.cursor")
        .then(cursor_forms)
        .into_iter()
        .flatten();
    let mut forms: Vec<Form> = Vec::new();
    for form in offered.chain(keys).chain(moves) {
        if !forms.iter().any(|known| known.args == form.args) {
            forms.push(form);
        }
    }
    forms
}

/// Declare `name`, carried out by the window.
fn declare<A: DeserializeOwned + JsonSchema + 'static>(
    app: &App,
    name: &'static str,
    title: &str,
    description: &str,
) {
    let declared =
        app.commands
            .register_raw::<A, _>(OWNER, name, title, description, move |args, app| {
                app.request_front(Invocation {
                    name: name.into(),
                    args,
                });
                Ok(Outcome::Done)
            });
    declared.forms(forms_of(name));
    if KEEP_A_KEY.contains(&name) {
        declared.keeps_a_key();
    }
    declared.effect(effect_of(name));
}

/// Declare every `ui.*` command in the core's registry.
#[allow(clippy::too_many_lines)] // One line of the table per command.
pub fn declare_all(app: &App) {
    let none = |name, title, description| declare::<NoArgs>(app, name, title, description);
    // The file list.
    declare::<Cursor>(
        app,
        "ui.cursor",
        "Move the cursor",
        "Move the file list's cursor (`to`), and with it the selection as `step` says",
    );
    declare::<Pick>(
        app,
        "ui.cursor_pick",
        "Pick or unpick",
        "Pick or unpick the item under the cursor, as yazi's mark, then go to the next",
    );
    none(
        "ui.cursor_toggle",
        "Select or deselect",
        "Select or deselect the item under the cursor",
    );
    none(
        "ui.context_menu",
        "Open the menu",
        "Open the menu of the selection, or of the folder shown",
    );
    none(
        "ui.open_in_tab",
        "Open in new tab",
        "Open the selected folders and places in tabs behind the one shown, else the selection",
    );
    declare::<Find>(
        app,
        "ui.find",
        "Find a name…",
        "Find a name in the pane as it is typed, after the cursor or before it",
    );
    declare::<Delta>(
        app,
        "ui.find_next",
        "Find the next name",
        "Find the name last looked for again, after the cursor (or before it)",
    );
    declare::<Sort>(
        app,
        "ui.sort",
        "Sort",
        "Sort the pane by a key, or turn its direction",
    );
    declare::<View>(
        app,
        "ui.view",
        "Show a view",
        "Show the pane's files in a view",
    );
    declare::<Key>(
        app,
        "ui.columns",
        "Show or hide a column",
        "Show or hide one of the list's detail columns",
    );
    none(
        "ui.columns_reset",
        "Reset column widths",
        "Give the list's detail columns their default widths",
    );
    declare::<Dir>(
        app,
        "ui.zoom",
        "Zoom",
        "Size what the view sizes (tiles, rows, a map's scale): in, out or back",
    );
    none(
        "ui.grid_fill",
        "Toggle filled thumbnails",
        "Crop thumbnails in the grid to fill their box",
    );
    none(
        "ui.narrow_folders",
        "Toggle the Narrow by bar over folders",
        "Offer the Narrow by bar over folders too",
    );
    none(
        "ui.toggle_hidden",
        "Toggle hidden files",
        "Show or hide hidden files",
    );
    none(
        "ui.peek",
        "Peek",
        "Look at the file under the cursor in a large preview",
    );
    none("ui.peek_close", "Close peek", "Close the large preview");
    none(
        "ui.usage_refresh",
        "Measure again",
        "Measure what the Usage view shows again",
    );
    declare::<UsageMode>(
        app,
        "ui.usage_mode",
        "Usage map shows",
        "Size the Usage map's blocks by the room or the files they hold, or color them by when they changed or the kinds of their files",
    );
    // Asking and naming.
    none(
        "ui.rename_prompt",
        "Rename…",
        "Rename the selected item, or a place as its location says",
    );
    none(
        "ui.mkdir_prompt",
        "New folder…",
        "Ask for a name and make a folder in the folder shown",
    );
    none(
        "ui.new_file_prompt",
        "New file…",
        "Ask for a name and make an empty file (a folder for a name ending in /)",
    );
    none(
        "ui.create_here",
        "New here…",
        "Make what the location makes (a tag in a tag's folder)",
    );
    none(
        "ui.rename_bulk_prompt",
        "Rename several…",
        "Rename the selected items together",
    );
    none(
        "ui.rename_bulk_next",
        "Next field",
        "Go to the next field of Rename several",
    );
    none(
        "ui.rename_bulk_prev",
        "Previous field",
        "Go to the previous field of Rename several",
    );
    none(
        "ui.rename_bulk_case",
        "Change case",
        "Cycle the case Rename several gives names",
    );
    declare::<TagsPrompt>(
        app,
        "ui.tags_prompt",
        "Tags…",
        "Edit the tags of the selection, or of the files named",
    );
    none(
        "ui.tag_editor_apply",
        "Apply tags",
        "Apply what the tag editor changed",
    );
    declare::<Ask>(
        app,
        "ui.ask",
        "Ask for a line of text",
        "Ask the user for a line of text and run a command with it",
    );
    declare::<Object>(
        app,
        "ui.choose",
        "Ask for a choice",
        "Ask the user to choose among choices, given or listed by a command, and run the one chosen",
    );
    none(
        "ui.choose_option",
        "Switch the option",
        "Switch the choice's option (Always use for…)",
    );
    declare::<Unlock>(
        app,
        "ui.unlock_prompt",
        "Unlock…",
        "Ask for the passphrase of a locked volume and unlock it",
    );
    none("ui.accept", "Confirm", "Answer yes to the question asked");
    none("ui.dismiss", "Cancel", "Answer no to the question asked");
    none(
        "ui.review_apply",
        "Apply the plan",
        "Apply the plan being reviewed",
    );
    none(
        "ui.review_cancel",
        "Discard the plan",
        "Discard the plan being reviewed",
    );
    declare::<Delta>(
        app,
        "ui.review_choose",
        "Next choice",
        "In the names a copy, a move or a link found taken: the next choice of the row the keyboard is on, or the previous with a negative delta",
    );
    // The address and searching.
    none(
        "ui.focus_address",
        "Focus address bar",
        "Edit the active pane's path",
    );
    declare::<Text>(
        app,
        "ui.search",
        "Search…",
        "Start a search where the pane is, from a text",
    );
    declare::<Submit>(
        app,
        "ui.omnibox_submit",
        "Go",
        "Run the address's suggestion, or search the home folder",
    );
    declare::<Object>(
        app,
        "ui.search_chip",
        "Search pill's menu",
        "Open or close a search pill's menu",
    );
    declare::<History>(
        app,
        "ui.history_menu",
        "Back: history…",
        "List the history behind Back, or ahead of Forward",
    );
    declare::<Folders>(
        app,
        "ui.subfolder_menu",
        "Folders inside…",
        "List the folders or places inside one of the trail's",
    );
    declare::<Folders>(
        app,
        "ui.parents_menu",
        "Folders above…",
        "List the folders above the trail's first, from the root down",
    );
    declare::<Url>(
        app,
        "ui.open_url",
        "Open an address",
        "Open an address (http, https, mailto) with the desktop's application",
    );
    declare::<crate::links::RevealArgs>(
        app,
        "ui.reveal",
        "Show in folder",
        "Show a file in its folder, at a line of its preview",
    );
    declare::<CopyText>(app, "ui.copy_text", "Copy", "Put a text on the clipboard");
    none("ui.jump", "Jump…", "Go to a folder or file Jump knows");
    // The window.
    none("ui.palette", "Command palette", "Find a command and run it");
    declare::<ListMoveArgs>(
        app,
        "ui.list_move",
        "Next in the list",
        "Move in the list being typed into (the palette, a choice, Jump, the tag editor, the address's suggestions) or answered (the names a copy found taken): rows round its ends, or a page, or to an end",
    );
    none(
        "ui.shortcuts",
        "Shortcuts",
        "List the keys of where the keyboard is",
    );
    declare::<Close>(
        app,
        "ui.about",
        "About skwir",
        "Show skwir's version and licence, and what it runs on",
    );
    declare::<Settings>(
        app,
        "ui.settings",
        "Settings…",
        "Open the settings, at a setting or a page",
    );
    none(
        "ui.focus_pane",
        "Focus file list",
        "Give the keyboard to the active pane",
    );
    none(
        "ui.focus_other_pane",
        "Focus other pane",
        "Give the keyboard to the split's other pane",
    );
    none(
        "ui.focus_sidebar",
        "Focus sidebar",
        "Give the keyboard to the sidebar",
    );
    none(
        "ui.toggle_sidebar",
        "Toggle sidebar",
        "Show or hide the sidebar",
    );
    declare::<Key>(
        app,
        "ui.sidebar_key",
        "Sidebar key",
        "A key in the sidebar: walk, open, move, rename or remove its places",
    );
    declare::<Key>(
        app,
        "ui.settings_key",
        "Keyboard page key",
        "A key in the Keyboard page's list of actions: walk the actions and their keys, change, add or remove one",
    );
    declare::<Object>(
        app,
        "ui.sidebar_section",
        "Arrange a sidebar group",
        "Move, show, hide or reset the sidebar's groups",
    );
    declare::<Object>(
        app,
        "ui.sidebar_place",
        "Arrange a sidebar place",
        "Move, rename, show or hide a place the sidebar offers",
    );
    declare::<Popup>(
        app,
        "ui.toolbar_menu",
        "Toolbar menu",
        "Open or close More, the View menu, the Map view's backgrounds or the pinned player's More (kind: more, view_options, map_backgrounds, player)",
    );
    // Answered by the focused button and the open popup themselves.
    none(
        "ui.toolbar_activate",
        "Press the button",
        "Press the button that has the keyboard",
    );
    none(
        "ui.toolbar_close",
        "Close the menu",
        "Close the toolbar's open menu",
    );
    none(
        "ui.toolbar_next",
        "Next button",
        "Give the keyboard to the next button",
    );
    none(
        "ui.toolbar_prev",
        "Previous button",
        "Give the keyboard to the previous button",
    );
    none(
        "ui.toolbar_first",
        "First row",
        "Give the keyboard to the first row of the open menu",
    );
    none(
        "ui.toolbar_last",
        "Last row",
        "Give the keyboard to the last row of the open menu",
    );
    declare::<Split>(
        app,
        "ui.split",
        "Split pane",
        "Split the window in two panes, or join them",
    );
    declare::<Layout>(
        app,
        "ui.layout",
        "Arrange the window",
        "Where split panes, the preview and a file of an archive go",
    );
    none(
        "ui.copy_other",
        "Copy to the other pane",
        "Copy the selection to what the other pane shows",
    );
    none(
        "ui.move_other",
        "Move to the other pane",
        "Move the selection to what the other pane shows",
    );
    none("ui.timeline", "Timeline", "Show or hide the timeline");
    declare::<Bucket>(
        app,
        "ui.timeline_bucket",
        "Timeline bars",
        "What each bar of the timeline counts: a day, a week, a month or a year",
    );
    none(
        "ui.timeline_subtree",
        "Timeline: folders inside",
        "Count the files of the folders inside too",
    );
    none(
        "ui.terminal",
        "Toggle terminal",
        "Show or hide the terminal",
    );
    none(
        "ui.terminal_copy",
        "Terminal: copy",
        "Copy the terminal's selection",
    );
    none(
        "ui.terminal_paste",
        "Terminal: paste",
        "Paste into the terminal",
    );
    declare::<Pages>(
        app,
        "ui.terminal_page",
        "Terminal: scroll",
        "Scroll the terminal by pages",
    );
    none(
        "ui.terminal_links",
        "Terminal: follow a link…",
        "Label the terminal's links for the keyboard",
    );
    for (name, title) in [
        ("ui.menu_up", "Previous item"),
        ("ui.menu_down", "Next item"),
        ("ui.menu_first", "First item"),
        ("ui.menu_last", "Last item"),
        ("ui.menu_expand", "Open the group"),
        ("ui.menu_collapse", "Close the group"),
        ("ui.menu_activate", "Run the item"),
        ("ui.menu_close", "Close the menu"),
    ] {
        none(name, title, "A key in a menu");
    }
    for (name, title) in [
        ("ui.settings_close", "Close settings"),
        ("ui.settings_next", "Next control"),
        ("ui.settings_prev", "Previous control"),
        ("ui.settings_search", "Search settings"),
        ("ui.settings_page", "Next page"),
        ("ui.settings_enter", "Into the page"),
        ("ui.settings_adjust", "Change the value"),
        ("ui.settings_move", "Move the record"),
    ] {
        declare::<Delta>(app, name, title, "A key in the settings");
    }
    none("ui.quit", "Quit", "Close the window");
    // The preview.
    none(
        "ui.toggle_preview",
        "Toggle preview panel",
        "Show or hide the preview panel",
    );
    none("ui.preview_close", "Preview: close", "Close the preview");
    none(
        "ui.preview_details",
        "Preview: show or hide details",
        "Show or hide the preview's details",
    );
    declare::<Popup>(
        app,
        "ui.preview_menu",
        "Preview menu",
        "Open or close one of the preview's menus",
    );
    none(
        "ui.preview_open_external",
        "Preview: open externally",
        "Open the file shown with the desktop's application",
    );
    none(
        "ui.preview_copy_path",
        "Preview: copy path",
        "Copy the path of the file shown",
    );
    declare::<Delta>(
        app,
        "ui.preview_page",
        "Preview: next page",
        "Go a page, a section or a chapter on or back",
    );
    declare::<Amount>(
        app,
        "ui.preview_seek",
        "Preview: seek",
        "Go seconds on or back in what plays",
    );
    declare::<Dir>(
        app,
        "ui.preview_zoom",
        "Preview: zoom",
        "Zoom what the preview shows, size what it reads",
    );
    declare::<Object>(
        app,
        "ui.inspect",
        "Preview action",
        "Do something in a structured preview (`action`: search, copy, outline, source, hex…)",
    );
    declare::<On>(
        app,
        "ui.printed_pages",
        "Printed page numbers",
        "Count a book's pages as printed, where it lists them",
    );
    declare::<On>(
        app,
        "ui.wrap_lines",
        "Wrap lines",
        "Wrap a text's long lines to the panel, or let them go on sideways",
    );
    none(
        "ui.full_width",
        "Toggle full-width reading",
        "Read text across the whole panel",
    );
    declare::<Object>(
        app,
        "ui.note",
        "Note",
        "Close a book's note, or go to where it is",
    );
    none(
        "ui.reader_continuous",
        "Document: toggle continuous scrolling",
        "Scroll a document's pages as one, or one at a time",
    );
    declare::<Pointer>(
        app,
        "ui.reader_pointer",
        "Document: pointer",
        "A press, a drag or a release on a document's page",
    );
    declare::<Page>(
        app,
        "ui.reader_page",
        "Document: go to page",
        "Show a document's page",
    );
    none(
        "ui.preview_goto",
        "Preview: go to page or chapter…",
        "Type the page to show, or a book's chapter",
    );
    none(
        "ui.reader_page_submit",
        "Document: show the page typed",
        "Show the page typed",
    );
    none(
        "ui.reader_page_cancel",
        "Document: leave the page field",
        "Leave the page field as it was",
    );
    // What a document and a structured preview are both asked, one
    // command each, carried out as the one shown does it.
    none(
        "ui.preview_copy",
        "Preview: copy",
        "Copy the text selected, else what is selected (a block), else all the preview shows",
    );
    none(
        "ui.preview_select_all",
        "Preview: select all",
        "Select all the text the preview shows",
    );
    none(
        "ui.preview_contents",
        "Preview: contents",
        "Show or hide the contents: a document's, a structured preview's headings",
    );
    none(
        "ui.preview_find",
        "Preview: find text",
        "Find text in what the preview shows",
    );
    none(
        "ui.preview_find_close",
        "Preview: close find",
        "Close the preview's find field",
    );
    declare::<Delta>(
        app,
        "ui.preview_match",
        "Preview: next search match",
        "Go to the next or previous match of what the find looks for",
    );
    none(
        "ui.reader_search",
        "Document: search",
        "Search a document for what the find field holds",
    );
    declare::<PageLink>(
        app,
        "ui.reader_link",
        "Document: follow a link",
        "Follow one of a page's links",
    );
    // What plays.
    declare::<Object>(
        app,
        "ui.media_control",
        "Player: control",
        "Play, pause, seek or set the volume of what the preview plays",
    );
    none(
        "ui.media_pin",
        "Player: pin or let go of this track",
        "Keep what plays playing while the selection moves, or let it go",
    );
    declare::<On>(
        app,
        "ui.media_keep_playing",
        "Player: keep playing while you browse",
        "Keep playing when the selection moves",
    );
    declare::<On>(
        app,
        "ui.media_autoplay",
        "Player: play on selection",
        "Play what is selected at once",
    );
    declare::<Continue>(
        app,
        "ui.media_continue",
        "Player: when a track ends",
        "What plays when a track ends: nothing, the next one, or one shuffled",
    );
    declare::<Delta>(
        app,
        "ui.media_step",
        "Player: next track",
        "Select the next or previous track",
    );
    declare::<Scrub>(
        app,
        "ui.media_scrub",
        "Player: scrub",
        "Drag the playing position",
    );
    none(
        "ui.media_fullscreen",
        "Player: full screen",
        "Show the film on the whole screen, or leave it",
    );
    none(
        "ui.media_controls",
        "Player: controls",
        "Show or hide the player's controls",
    );
    none(
        "ui.media_escape",
        "Player: leave",
        "Leave full screen, else close the preview",
    );
    declare::<Player>(
        app,
        "ui.player",
        "Player",
        "Ask the pinned player something (play, pause, next, stop…)",
    );
    declare::<Amount>(
        app,
        "ui.player_volume",
        "Player: volume",
        "Change the pinned player's volume",
    );
    // The map.
    declare::<Pan>(app, "ui.map_pan", "Map: pan", "Move the map");
    none(
        "ui.map_open",
        "Map: open in Maps",
        "Open the place shown in the desktop's maps",
    );
    none(
        "ui.map_view_fit",
        "Map: fit to its files",
        "Fit the Map view to its files",
    );
    none(
        "ui.map_view_spread",
        "Map: spread the files at one place, or group them",
        "Spread the files at one place, or group them",
    );
    none(
        "ui.map_view_search_area",
        "Map: search the pictures and videos of this area",
        "Search the pictures and videos taken in the area shown",
    );
}

#[cfg(test)]
mod tests {
    use skwir_core::command::{CommandError, Effect, Invocation};

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
            "ui.settings_adjust",
        ] {
            assert_eq!(effect(answer), Effect::UserOnly, "{answer}");
        }
        assert_eq!(effect("ui.cursor"), Effect::Shows);
        assert_eq!(effect("ui.open_url"), Effect::Acts);
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
                    form.title
                );
            }
        }
        for name in super::KEEP_A_KEY {
            assert!(
                app.commands.get(name).is_some_and(|spec| spec.keeps_a_key),
                "{name}"
            );
        }
        let wrong = Invocation::with_args("ui.sort", &serde_json::json!({ "key": "type" }));
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
