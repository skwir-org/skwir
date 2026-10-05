//! The shell's own commands (`ui.*`), declared in the core's registry as
//! every command is (owner `ui`): their titles, what they do and the
//! schemas of their arguments, so the palette, F1, the keymap, a user's
//! bindings, scripts and agents all know them, and arguments a command
//! does not take are refused, never read as defaults. The shell carries
//! them out (`Shell::run_ui`); one dispatched from elsewhere (an agent, a
//! script) waits for the window (`App::request_front`) and runs when it
//! next wakes, on the pane active then.

use bibli_core::app::App;
use bibli_core::command::{Invocation, NoArgs, Outcome};
use schemars::JsonSchema;
use serde::de::DeserializeOwned;

/// Who declares them.
pub const OWNER: &str = "ui";

/// The arguments of the commands, as their schemas say them. Only read
/// through serde, to check what a command is given and to describe it.
#[allow(dead_code)] // Fields are read by `Deserialize` and `JsonSchema` only.
mod args {
    use bibli_core::location::SortKey;
    use bibli_core::op::EntryRef;
    use bibli_core::os_path::PathArg;
    use schemars::JsonSchema;
    use serde::Deserialize;

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
    Amount, Ask, Bucket, Continue, CopyText, Cursor, Delta, Dir, Find, Folders, History, Key,
    Layout, ListMoveArgs, Object, On, Page, PageLink, Pages, Pan, Pick, Player, Pointer, Popup,
    Scrub, Settings, Sort, Split, Submit, TagsPrompt, Text, Unlock, Url, View,
};

/// Declare `name`, carried out by the window.
fn declare<A: DeserializeOwned + JsonSchema + 'static>(
    app: &App,
    name: &'static str,
    title: &str,
    description: &str,
) {
    app.commands
        .register_raw::<A, _>(OWNER, name, title, description, move |args, app| {
            app.request_front(Invocation {
                name: name.into(),
                args,
            });
            Ok(Outcome::Done)
        });
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
        "Move in the list being typed into (the palette, a choice, Jump, the tag editor, the address's suggestions): rows round its ends, or a page, or to an end",
    );
    none(
        "ui.shortcuts",
        "Shortcuts",
        "List the keys of where the keyboard is",
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
        "Open or close More or the View menu",
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
    use bibli_core::command::{CommandError, Invocation};

    /// What the shell's keys run and the palette offers is declared, with
    /// arguments the declaration takes; arguments it does not are refused.
    #[test]
    fn every_ui_command_the_shell_runs_is_declared_with_its_arguments() {
        let tmp = tempfile::tempdir().unwrap();
        let app = bibli_core::App::new(bibli_core::app::AppConfig::ephemeral(tmp.path())).unwrap();
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
