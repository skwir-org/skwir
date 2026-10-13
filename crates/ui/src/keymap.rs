//! Translate the core keymap registry (plus UI defaults) into GPUI key bindings.
//! Every binding runs one action, [`RunCommand`], which the shell routes.

use std::collections::HashMap;

use gpui::{Action, KeyBinding, Keystroke};
use skwir_core::App;
use skwir_core::keymap::UserBinding;

/// The single action every key binding maps to.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = skwir, no_json)]
pub struct RunCommand {
    pub name: String,
    pub args: serde_json::Value,
}

/// Key context of the window root: bindings with no `when` live here, so
/// deeper contexts (the text input) win on collisions.
pub const ROOT: &str = "Shell";
pub const PANE: &str = "Pane";
pub const ADDRESS: &str = "AddressBar";
pub const PALETTE: &str = "Palette";
pub const CONFIRM: &str = "Confirm";
pub const PEEK: &str = "Peek";
pub const READER: &str = "DocumentReader";
/// A preview with something to find in and copy from (a document, a
/// structured preview): the same keys, one command each, whichever.
pub const PREVIEW: &str = "Preview";
/// A preview's find field and its buttons.
pub const PREVIEW_FIND: &str = "PreviewFind";
pub const IMAGE: &str = "ImagePreview";
/// A map preview, once clicked: arrows move it, the zoom keys zoom it.
pub const MAP: &str = "MapPreview";
/// The pinned player's dock.
pub const PLAYER: &str = "Player";
pub const REVIEW: &str = "Review";
pub const JUMP: &str = "Jump";
/// `ui.choose`'s picker.
pub const CHOOSE: &str = "Choose";
/// The tag editor (`tags.rs`).
pub const TAG_EDITOR: &str = "TagEditor";
/// The terminal panel: it takes every key the window binds, but these.
pub const TERMINAL: &str = "Terminal";

/// What still reaches the window from the terminal: the key that closes
/// the panel, and the palette, from which everything else can be run. Every
/// other key bound anywhere in the window belongs to the shell and its
/// programs while the terminal has the keyboard (Tab completes, Ctrl+L
/// clears, F-keys drive a file manager). `install` shadows them.
const TERMINAL_PASSTHROUGH: [&str; 2] = ["ui.terminal", "ui.palette"];
pub const BULK: &str = "BulkRename";
/// The files of an archive or a message, as its preview lists them: the
/// arrows walk them and the member reached is previewed (`members.rs`).
pub const MEMBERS: &str = "PreviewMembers";
/// A member shown under its container's view, and that view: Escape
/// closes the member (`members.rs`).
pub const MEMBER: &str = "PreviewMember";
pub const MENU: &str = "ContextMenu";
/// A structured preview's content (`inspector.rs`).
pub const INSPECTOR: &str = "Inspector";
/// A text read where it is shown (`text_view.rs`): the keys scroll it.
pub const TEXT: &str = "PreviewText";
/// What the preview plays.
pub const MEDIA: &str = "MediaPlayer";
/// The sidebar while it has the keyboard (F6): the arrows walk its groups
/// and places (`sidebar.rs`).
pub const SIDEBAR: &str = "Sidebar";

/// A binding as the palette shows it: keys and command name. Only the
/// bindings that can run are hints: one a later layer hides is not.
#[derive(Clone, Debug)]
pub struct KeyHint {
    pub keys: String,
    pub command: String,
    pub args: serde_json::Value,
    /// The GPUI context predicate the binding is active in.
    pub context: String,
    pub layer: Layer,
}

/// Every binding's hint, and which are each command's: every button drawn
/// asks for its shortcut at each frame (a menu of folders, five hundred
/// times), which scanned every binding.
#[derive(Clone, Default)]
pub struct Hints {
    all: Vec<KeyHint>,
    by_command: HashMap<String, Vec<usize>>,
}

impl Hints {
    pub fn new(all: Vec<KeyHint>) -> Self {
        let mut by_command: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, hint) in all.iter().enumerate() {
            by_command.entry(hint.command.clone()).or_default().push(i);
        }
        Hints { all, by_command }
    }

    /// The hints of `command`, in the order of all of them.
    pub fn of<'a>(&'a self, command: &str) -> impl Iterator<Item = &'a KeyHint> + 'a {
        self.by_command
            .get(command)
            .into_iter()
            .flatten()
            .map(|&i| &self.all[i])
    }
}

impl std::ops::Deref for Hints {
    type Target = [KeyHint];
    fn deref(&self) -> &[KeyHint] {
        &self.all
    }
}

impl<'a> IntoIterator for &'a Hints {
    type Item = &'a KeyHint;
    type IntoIter = std::slice::Iter<'a, KeyHint>;
    fn into_iter(self) -> Self::IntoIter {
        self.all.iter()
    }
}

/// Bindings the shell always needs, independent of plugins: every `ui.*`
/// command the shell owns has its key here, so disabling a plugin never
/// takes the palette or Quit with it. Keys for core and plugin commands are
/// the plugins' (ops-basic binds the file operations).
const BUILTIN: &[(&str, &str, Option<&str>)] = &[
    ("f2", "ui.rename_prompt", Some("pane single !restores")),
    // Into the folder shown, like Ctrl+V: a search or the shelf has none.
    ("secondary-shift-n", "ui.mkdir_prompt", Some("pane folder")),
    // A new place where the location makes them (a tag's: a tag in it).
    ("secondary-shift-n", "ui.create_here", Some("pane creates")),
    ("secondary-l", "ui.focus_address", None),
    ("secondary-shift-p", "ui.palette", None),
    ("secondary-q", "ui.quit", None),
    ("ctrl-h", "ui.toggle_hidden", None),
    // Dolphin's key for it, beside GNOME's.
    ("alt-.", "ui.toggle_hidden", None),
    ("f9", "ui.toggle_sidebar", None),
    // The keyboard into the sidebar, and back to the files.
    ("f6", "ui.focus_sidebar", None),
    ("escape", "ui.focus_pane", Some("sidebar")),
    ("tab", "ui.toolbar_next", None),
    ("shift-tab", "ui.toolbar_prev", None),
    ("enter", "ui.toolbar_activate", Some("toolbar_button")),
    ("space", "ui.toolbar_activate", Some("toolbar_button")),
    ("escape", "ui.toolbar_close", Some("toolbar_popup")),
    // A popup's rows are buttons: the arrows walk them as Tab does, and
    // Home and End reach its ends.
    ("down", "ui.toolbar_next", Some("toolbar_popup")),
    ("up", "ui.toolbar_prev", Some("toolbar_popup")),
    ("home", "ui.toolbar_first", Some("toolbar_popup")),
    ("end", "ui.toolbar_last", Some("toolbar_popup")),
    ("escape", "ui.preview_find_close", Some("preview_find")),
    ("secondary-,", "ui.settings", None),
    ("f1", "ui.shortcuts", None),
    // GNOME's key for the shortcuts window. GPUI leaves out the Shift a
    // symbol took, so Ctrl+Shift+/ (or wherever `?` is) arrives as `ctrl-?`.
    ("secondary-?", "ui.shortcuts", None),
    ("escape", "ui.shortcuts", Some("shortcuts")),
    ("escape", "ui.menu_close", Some("menu")),
    ("up", "ui.menu_up", Some("menu")),
    ("down", "ui.menu_down", Some("menu")),
    ("home", "ui.menu_first", Some("menu")),
    ("end", "ui.menu_last", Some("menu")),
    ("right", "ui.menu_expand", Some("menu")),
    ("left", "ui.menu_collapse", Some("menu")),
    ("enter", "ui.menu_activate", Some("menu")),
    ("space", "ui.menu_activate", Some("menu")),
    ("escape", "ui.settings_close", Some("settings")),
    ("tab", "ui.settings_next", Some("settings")),
    ("shift-tab", "ui.settings_prev", Some("settings")),
    ("secondary-f", "ui.settings_search", Some("settings")),
    ("enter", "ui.settings_enter", Some("settings_nav")),
    ("right", "ui.settings_enter", Some("settings_nav")),
    ("secondary-c", "ui.preview_copy", Some("preview")),
    ("secondary-a", "ui.preview_select_all", Some("preview")),
    ("secondary-f", "ui.preview_find", Some("preview")),
    ("ctrl-space", "ui.cursor_toggle", Some("pane")),
    // The keyboard's way to what a right-click opens.
    ("shift-f10", "ui.context_menu", Some("pane")),
    ("menu", "ui.context_menu", Some("pane")),
    // What the middle button does to a folder: a tab behind this one.
    ("secondary-enter", "ui.open_in_tab", Some("pane selection")),
    ("enter", "ui.accept", Some("confirm")),
    ("escape", "ui.dismiss", Some("confirm")),
    ("space", "ui.peek", Some("pane")),
    ("ctrl-shift-space", "ui.toggle_preview", None),
    // Layouts where shift+space types a character (fr: U+202F) never
    // produce `ctrl-shift-space`; F11 is the layout-independent fallback.
    // F3 splits and F11 shows the side panel, as in Dolphin; inside a
    // document F3 stays "next match", the deeper context wins.
    ("f11", "ui.toggle_preview", None),
    ("secondary-i", "ui.preview_details", None),
    ("alt-enter", "ui.preview_details", None),
    ("f3", "ui.split", None),
    ("space", "ui.peek_close", Some("peek")),
    ("escape", "ui.peek_close", Some("peek")),
    ("secondary-shift-d", "ui.split", None),
    ("tab", "ui.focus_other_pane", Some("pane split")),
    ("shift-tab", "ui.focus_other_pane", Some("pane split")),
    // F5 refreshes whatever the window shows; with Shift it sends the
    // selection across a split, as F6 moves it.
    ("shift-f5", "ui.copy_other", Some("pane split selection")),
    ("shift-f6", "ui.move_other", Some("pane split selection")),
    ("ctrl-shift-o", "ui.terminal_links", Some("terminal")),
    ("enter", "ui.review_apply", Some("review")),
    ("escape", "ui.review_cancel", Some("review")),
    ("alt-a", "ui.choose_option", Some("choose")),
    ("secondary-enter", "ui.tag_editor_apply", Some("tag_editor")),
    ("tab", "ui.rename_bulk_next", Some("bulk")),
    ("shift-tab", "ui.rename_bulk_prev", Some("bulk")),
    ("secondary-k", "ui.rename_bulk_case", Some("bulk")),
];

/// Builtin bindings that carry arguments (`keys, command, args json, when`).
const BUILTIN_ARGS: &[(&str, &str, &str, Option<&str>)] = &[
    // The lists one picks from while typing (the palette, a choice, Jump,
    // the tag editor, the address's suggestions): one command, the same
    // keys, the arrows round their ends, the page keys a page at a time.
    (
        "up",
        "ui.list_move",
        r#"{"delta":-1}"#,
        Some("palette|jump|choose|tag_editor|address|review"),
    ),
    (
        "down",
        "ui.list_move",
        r#"{"delta":1}"#,
        Some("palette|jump|choose|tag_editor|address|review"),
    ),
    (
        "pageup",
        "ui.list_move",
        r#"{"to":"page_up"}"#,
        Some("palette|jump|choose|tag_editor|address|review"),
    ),
    (
        "pagedown",
        "ui.list_move",
        r#"{"to":"page_down"}"#,
        Some("palette|jump|choose|tag_editor|address|review"),
    ),
    // The cursor in the file list (`ui.cursor`): the arrows select what
    // they reach, Shift extends, Ctrl moves the cursor alone (and
    // Ctrl+Space, `ui.cursor_toggle`, selects or deselects what it is on).
    ("up", "ui.cursor", r#"{"to":"up"}"#, Some("pane")),
    ("down", "ui.cursor", r#"{"to":"down"}"#, Some("pane")),
    // The names a copy, a move or a link found taken: the row's choice.
    (
        "left",
        "ui.review_choose",
        r#"{"delta":-1}"#,
        Some("review"),
    ),
    (
        "right",
        "ui.review_choose",
        r#"{"delta":1}"#,
        Some("review"),
    ),
    ("left", "ui.cursor", r#"{"to":"left"}"#, Some("pane")),
    ("right", "ui.cursor", r#"{"to":"right"}"#, Some("pane")),
    ("home", "ui.cursor", r#"{"to":"home"}"#, Some("pane")),
    ("end", "ui.cursor", r#"{"to":"end"}"#, Some("pane")),
    ("pageup", "ui.cursor", r#"{"to":"page_up"}"#, Some("pane")),
    (
        "pagedown",
        "ui.cursor",
        r#"{"to":"page_down"}"#,
        Some("pane"),
    ),
    (
        "shift-up",
        "ui.cursor",
        r#"{"to":"up","step":"extend"}"#,
        Some("pane"),
    ),
    (
        "shift-down",
        "ui.cursor",
        r#"{"to":"down","step":"extend"}"#,
        Some("pane"),
    ),
    (
        "shift-left",
        "ui.cursor",
        r#"{"to":"left","step":"extend"}"#,
        Some("pane"),
    ),
    (
        "shift-right",
        "ui.cursor",
        r#"{"to":"right","step":"extend"}"#,
        Some("pane"),
    ),
    (
        "shift-home",
        "ui.cursor",
        r#"{"to":"home","step":"extend"}"#,
        Some("pane"),
    ),
    (
        "shift-end",
        "ui.cursor",
        r#"{"to":"end","step":"extend"}"#,
        Some("pane"),
    ),
    (
        "ctrl-up",
        "ui.cursor",
        r#"{"to":"up","step":"move"}"#,
        Some("pane"),
    ),
    (
        "ctrl-down",
        "ui.cursor",
        r#"{"to":"down","step":"move"}"#,
        Some("pane"),
    ),
    (
        "ctrl-left",
        "ui.cursor",
        r#"{"to":"left","step":"move"}"#,
        Some("pane"),
    ),
    (
        "ctrl-right",
        "ui.cursor",
        r#"{"to":"right","step":"move"}"#,
        Some("pane"),
    ),
    (
        "ctrl-home",
        "ui.cursor",
        r#"{"to":"home","step":"move"}"#,
        Some("pane"),
    ),
    (
        "ctrl-end",
        "ui.cursor",
        r#"{"to":"end","step":"move"}"#,
        Some("pane"),
    ),
    // Peek walks the files in their order.
    ("left", "ui.cursor", r#"{"to":"previous"}"#, Some("peek")),
    ("up", "ui.cursor", r#"{"to":"previous"}"#, Some("peek")),
    ("right", "ui.cursor", r#"{"to":"next"}"#, Some("peek")),
    ("down", "ui.cursor", r#"{"to":"next"}"#, Some("peek")),
    // A pane filtered by name shows everything again before Escape
    // deselects (`select_none`, the plugins' binding).
    ("escape", "filter", "{}", Some("pane filtered")),
    // Escape only closes About: bound with arguments of its own, it is not
    // named beside About in the palette (`keys_for` matches them).
    ("escape", "ui.about", r#"{"close":true}"#, Some("about")),
    // The address field's dropdown: the keyboard stays in the field.
    (
        "alt-enter",
        "ui.omnibox_submit",
        r#"{"everywhere":true}"#,
        Some("address"),
    ),
    // The sidebar's keys, its places walked as the file list's rows.
    ("up", "ui.sidebar_key", r#"{"key":"up"}"#, Some("sidebar")),
    (
        "down",
        "ui.sidebar_key",
        r#"{"key":"down"}"#,
        Some("sidebar"),
    ),
    (
        "home",
        "ui.sidebar_key",
        r#"{"key":"first"}"#,
        Some("sidebar"),
    ),
    (
        "end",
        "ui.sidebar_key",
        r#"{"key":"last"}"#,
        Some("sidebar"),
    ),
    (
        "left",
        "ui.sidebar_key",
        r#"{"key":"left"}"#,
        Some("sidebar"),
    ),
    (
        "right",
        "ui.sidebar_key",
        r#"{"key":"right"}"#,
        Some("sidebar"),
    ),
    (
        "enter",
        "ui.sidebar_key",
        r#"{"key":"open"}"#,
        Some("sidebar"),
    ),
    (
        "space",
        "ui.sidebar_key",
        r#"{"key":"open"}"#,
        Some("sidebar"),
    ),
    (
        "ctrl-enter",
        "ui.sidebar_key",
        r#"{"key":"tab"}"#,
        Some("sidebar"),
    ),
    (
        "menu",
        "ui.sidebar_key",
        r#"{"key":"menu"}"#,
        Some("sidebar"),
    ),
    (
        "shift-f10",
        "ui.sidebar_key",
        r#"{"key":"menu"}"#,
        Some("sidebar"),
    ),
    (
        "alt-up",
        "ui.sidebar_key",
        r#"{"key":"move_up"}"#,
        Some("sidebar"),
    ),
    (
        "alt-down",
        "ui.sidebar_key",
        r#"{"key":"move_down"}"#,
        Some("sidebar"),
    ),
    (
        "f2",
        "ui.sidebar_key",
        r#"{"key":"rename"}"#,
        Some("sidebar"),
    ),
    (
        "delete",
        "ui.sidebar_key",
        r#"{"key":"remove"}"#,
        Some("sidebar"),
    ),
    // Shift+F3 splits the other way from F3, for this split only.
    ("shift-f3", "ui.split", r#"{"axis":"other"}"#, None),
    (
        "space",
        "ui.media_control",
        r#"{"action":"play"}"#,
        Some("media"),
    ),
    ("left", "ui.preview_seek", r#"{"delta":-5}"#, Some("media")),
    ("right", "ui.preview_seek", r#"{"delta":5}"#, Some("media")),
    (
        "m",
        "ui.media_control",
        r#"{"action":"mute"}"#,
        Some("media"),
    ),
    (
        "shift-left",
        "ui.media_step",
        r#"{"delta":-1}"#,
        Some("media"),
    ),
    (
        "shift-right",
        "ui.media_step",
        r#"{"delta":1}"#,
        Some("media"),
    ),
    ("f", "ui.media_fullscreen", "{}", Some("media")),
    (
        "up",
        "ui.media_control",
        r#"{"action":"volume_by","value":0.05}"#,
        Some("media"),
    ),
    (
        "down",
        "ui.media_control",
        r#"{"action":"volume_by","value":-0.05}"#,
        Some("media"),
    ),
    // The dock steers the pinned player through the same requests the
    // desktop's media keys send.
    (
        "space",
        "ui.player",
        r#"{"request":"play_pause"}"#,
        Some("player"),
    ),
    (
        "left",
        "ui.player",
        r#"{"request":{"seek_by":-5.0}}"#,
        Some("player"),
    ),
    (
        "right",
        "ui.player",
        r#"{"request":{"seek_by":5.0}}"#,
        Some("player"),
    ),
    (
        "shift-left",
        "ui.player",
        r#"{"request":"previous"}"#,
        Some("player"),
    ),
    (
        "shift-right",
        "ui.player",
        r#"{"request":"next"}"#,
        Some("player"),
    ),
    ("m", "ui.player", r#"{"request":"mute"}"#, Some("player")),
    (
        "f",
        "ui.player",
        r#"{"request":"fullscreen"}"#,
        Some("player"),
    ),
    (
        "up",
        "ui.player_volume",
        r#"{"delta":0.05}"#,
        Some("player"),
    ),
    (
        "down",
        "ui.player_volume",
        r#"{"delta":-0.05}"#,
        Some("player"),
    ),
    ("c", "ui.media_controls", "{}", Some("media")),
    ("escape", "ui.media_escape", "{}", Some("media")),
    (
        "pageup",
        "ui.preview_page",
        r#"{"delta":-1}"#,
        Some("reader"),
    ),
    (
        "pagedown",
        "ui.preview_page",
        r#"{"delta":1}"#,
        Some("reader"),
    ),
    // The matches of what a preview's find looks for, from the field or
    // the preview.
    ("f3", "ui.preview_match", r#"{"delta":1}"#, Some("preview")),
    (
        "shift-f3",
        "ui.preview_match",
        r#"{"delta":-1}"#,
        Some("preview"),
    ),
    (
        "f3",
        "ui.preview_match",
        r#"{"delta":1}"#,
        Some("preview_find"),
    ),
    (
        "shift-f3",
        "ui.preview_match",
        r#"{"delta":-1}"#,
        Some("preview_find"),
    ),
    // The same keys in every preview: zoom what is looked at, size what is
    // read, step through sections or pages.
    // The files of an archive or a message: the arrows walk them, the
    // member reached is previewed under them, Left and Right fold a folder
    // (or go up to it), Escape closes the member.
    (
        "up",
        "ui.inspect",
        r#"{"action":"step","delta":-1}"#,
        Some("members"),
    ),
    (
        "down",
        "ui.inspect",
        r#"{"action":"step","delta":1}"#,
        Some("members"),
    ),
    (
        "left",
        "ui.inspect",
        r#"{"action":"fold","delta":-1}"#,
        Some("members"),
    ),
    (
        "right",
        "ui.inspect",
        r#"{"action":"fold","delta":1}"#,
        Some("members"),
    ),
    (
        "escape",
        "ui.inspect",
        r#"{"action":"back"}"#,
        Some("member"),
    ),
    // A map moves under the arrows, as it does under a drag.
    ("left", "ui.map_pan", r#"{"dx":-1,"dy":0}"#, Some("map")),
    ("right", "ui.map_pan", r#"{"dx":1,"dy":0}"#, Some("map")),
    ("up", "ui.map_pan", r#"{"dx":0,"dy":-1}"#, Some("map")),
    ("down", "ui.map_pan", r#"{"dx":0,"dy":1}"#, Some("map")),
    // A text read where it is shown goes row by row, a screen at a time,
    // or to either end; Alt+Z wraps its lines or not.
    (
        "up",
        "ui.inspect",
        r#"{"action":"scroll","to":"up"}"#,
        Some("text"),
    ),
    (
        "down",
        "ui.inspect",
        r#"{"action":"scroll","to":"down"}"#,
        Some("text"),
    ),
    (
        "pageup",
        "ui.inspect",
        r#"{"action":"scroll","to":"page_up"}"#,
        Some("text"),
    ),
    (
        "pagedown",
        "ui.inspect",
        r#"{"action":"scroll","to":"page_down"}"#,
        Some("text"),
    ),
    (
        "ctrl-home",
        "ui.inspect",
        r#"{"action":"scroll","to":"start"}"#,
        Some("text"),
    ),
    (
        "ctrl-end",
        "ui.inspect",
        r#"{"action":"scroll","to":"end"}"#,
        Some("text"),
    ),
    ("alt-z", "ui.wrap_lines", "{}", Some("text")),
    ("[", "ui.preview_page", r#"{"delta":-1}"#, Some("inspector")),
    ("]", "ui.preview_page", r#"{"delta":1}"#, Some("inspector")),
    ("[", "ui.preview_page", r#"{"delta":-1}"#, Some("reader")),
    ("]", "ui.preview_page", r#"{"delta":1}"#, Some("reader")),
    ("alt-pageup", "ui.preview_page", r#"{"delta":-1}"#, None),
    ("alt-pagedown", "ui.preview_page", r#"{"delta":1}"#, None),
    (
        "ctrl-left",
        "ui.preview_seek",
        r#"{"delta":-5}"#,
        Some("peek"),
    ),
    (
        "ctrl-right",
        "ui.preview_seek",
        r#"{"delta":5}"#,
        Some("peek"),
    ),
    (
        "up",
        "ui.settings_page",
        r#"{"delta":-1}"#,
        Some("settings_nav"),
    ),
    (
        "down",
        "ui.settings_page",
        r#"{"delta":1}"#,
        Some("settings_nav"),
    ),
    // A record of a list moves with its buttons' focus, as lines move in
    // an editor.
    (
        "alt-up",
        "ui.settings_move",
        r#"{"delta":-1}"#,
        Some("setting_record"),
    ),
    (
        "alt-down",
        "ui.settings_move",
        r#"{"delta":1}"#,
        Some("setting_record"),
    ),
    (
        "left",
        "ui.settings_adjust",
        r#"{"delta":-1}"#,
        Some("setting_control"),
    ),
    (
        "right",
        "ui.settings_adjust",
        r#"{"delta":1}"#,
        Some("setting_control"),
    ),
    (
        "down",
        "ui.settings_adjust",
        r#"{"delta":-1}"#,
        Some("setting_number"),
    ),
    (
        "up",
        "ui.settings_adjust",
        r#"{"delta":1}"#,
        Some("setting_number"),
    ),
    // The links in view labelled, for the keyboard to follow, as foot's
    // and kitty's hints label a terminal's.
    (
        "ctrl-shift-o",
        "ui.inspect",
        r#"{"action":"links"}"#,
        Some("inspector"),
    ),
    (
        "escape",
        "ui.inspect",
        r#"{"action":"links_close"}"#,
        Some("inspector labelling"),
    ),
    (
        "shift-pageup",
        "ui.terminal_page",
        r#"{"pages":1}"#,
        Some("terminal"),
    ),
    (
        "shift-pagedown",
        "ui.terminal_page",
        r#"{"pages":-1}"#,
        Some("terminal"),
    ),
    ("secondary-1", "ui.view", r#"{"id":"list"}"#, None),
    ("secondary-2", "ui.view", r#"{"id":"grid"}"#, None),
    ("secondary-3", "ui.view", r#"{"id":"columns"}"#, None),
];

/// The keys of zooming, the same wherever something zooms: `=` and `+`
/// (GPUI leaves out the Shift a symbol took, so Ctrl+Shift+= arrives as
/// `ctrl-+` on layouts where `+` is shifted), and the keypad's.
const ZOOM_KEYS: [(&str, &str); 6] = [
    ("secondary-=", "in"),
    ("secondary-+", "in"),
    ("secondary-add", "in"),
    ("secondary--", "out"),
    ("secondary-subtract", "out"),
    ("secondary-0", "reset"),
];

/// What zooms, where: the pane's view anywhere, what a preview shows where
/// it has the keyboard. The inspector's text is sized in steps of its own.
const ZOOMS: [(&str, &str, Option<&str>); 5] = [
    ("ui.zoom", "dir", None),
    ("ui.preview_zoom", "dir", Some("reader")),
    ("ui.preview_zoom", "dir", Some("image")),
    ("ui.preview_zoom", "dir", Some("map")),
    ("ui.inspect", "action", Some("inspector")),
];

/// Every command the shell's own keys run, with its arguments.
#[cfg(test)]
pub(crate) fn shell_invocations() -> Vec<(String, serde_json::Value)> {
    BUILTIN
        .iter()
        .map(|(_, command, _)| ((*command).to_owned(), serde_json::json!({})))
        .chain(BUILTIN_ARGS.iter().map(|(_, command, args, _)| {
            (
                (*command).to_owned(),
                serde_json::from_str(args).expect("valid builtin args"),
            )
        }))
        .chain(zoom_specs().map(|(_, command, args, _)| (command, args)))
        .collect()
}

fn zoom_specs() -> impl Iterator<Item = (String, String, serde_json::Value, String)> {
    ZOOMS.into_iter().flat_map(|(command, arg, when)| {
        ZOOM_KEYS.into_iter().map(move |(keys, dir)| {
            let value = match (command, dir) {
                ("ui.inspect", "in") => "larger",
                ("ui.inspect", "out") => "smaller",
                _ => dir,
            };
            (
                keys.to_owned(),
                command.to_owned(),
                serde_json::json!({ arg: value }),
                context_of(when),
            )
        })
    })
}

/// Every place a binding can name, once: its `when` token, the key
/// context it is, and what F1 calls it. `context_of` and `context_label`
/// read it, so a new place is one row. A context named as it is
/// (`ContextMenu`) is taken too, as plugins wrote them before.
const PLACES: &[(&str, &str, &str)] = &[
    ("pane", PANE, "File list"),
    ("address", ADDRESS, "Address bar"),
    ("palette", PALETTE, "Command palette"),
    ("confirm", CONFIRM, "Confirmation dialogs"),
    ("peek", PEEK, "Peek"),
    ("reader", READER, "Document reader"),
    ("preview", PREVIEW, "Documents and structured previews"),
    ("preview_find", PREVIEW_FIND, "Preview search"),
    ("inspector", INSPECTOR, "Structured previews"),
    ("text", TEXT, "Text in the preview"),
    ("members", MEMBERS, "Files of an archive or a message"),
    (
        "member",
        MEMBER,
        "A file shown from an archive or a message",
    ),
    ("image", IMAGE, "Picture preview"),
    ("map", MAP, "Map preview"),
    ("media", MEDIA, "Media player"),
    ("player", PLAYER, "Pinned player"),
    ("review", REVIEW, "Plan review"),
    ("jump", JUMP, "Jump"),
    ("choose", CHOOSE, "Choosing from a list"),
    ("tag_editor", TAG_EDITOR, "Tag editor"),
    ("bulk", BULK, "Bulk rename"),
    ("terminal", TERMINAL, "Terminal"),
    ("sidebar", SIDEBAR, "Sidebar"),
    ("menu", MENU, "Context menu"),
    ("toolbar_button", "ToolbarButton", "Toolbars"),
    ("toolbar_popup", "ToolbarPopup", "Toolbars"),
    ("shortcuts", "Shortcuts", "Shortcut overview"),
    ("about", "About", "About skwir"),
    ("settings", "Settings", "Settings"),
    ("settings_nav", "SettingsNav", "Settings"),
    ("setting_control", "SettingControl", "Settings"),
    ("setting_number", "SettingNumber", "Settings"),
    ("setting_record", "SettingRecord", "Settings"),
];

/// Map a registry `when` string to a GPUI context predicate. A token of
/// `PLACES` names a place; anything else is a flag the pane sets on its
/// key context (the tokens of `menu::SELECTION_TOKENS`, `folder` among
/// them, `split`, `usage`). All tokens must hold.
pub fn context_of(when: Option<&str>) -> String {
    let Some(when) = when else {
        return ROOT.to_owned();
    };
    let one = |t: &str| {
        PLACES.iter().find(|(token, _, _)| *token == t).map_or_else(
            || crate::menu::canonical_token(t).to_owned(),
            |(_, context, _)| (*context).to_owned(),
        )
    };
    let parts: Vec<String> = when
        .split_whitespace()
        .map(|t| {
            if t.contains('|') {
                let alts: Vec<String> = t.split('|').map(one).collect();
                format!("({})", alts.join(" || "))
            } else {
                one(t)
            }
        })
        .collect();
    if parts.is_empty() {
        ROOT.to_owned()
    } else {
        parts.join(" && ")
    }
}

/// Whether a context predicate (`Pane && (split || single) && !restores`)
/// holds: `holds` says whether a place or a flag does, `None` when it
/// cannot tell, which then holds. The one reading of a binding's
/// condition: F1, the key-sequence strip and the keys a menu names.
pub fn predicate_holds(context: &str, holds: &dyn Fn(&str) -> Option<bool>) -> bool {
    let one = |atom: &str| match atom.strip_prefix('!') {
        Some(not) => holds(not.trim()).map(|h| !h),
        None => holds(atom),
    };
    conditions(context).into_iter().all(|atom| {
        match atom.strip_prefix('(').and_then(|a| a.strip_suffix(')')) {
            Some(alts) => alts.split("||").any(|alt| one(alt.trim()).unwrap_or(true)),
            None => one(atom).unwrap_or(true),
        }
    })
}

/// A flag of the pane's key context in plain words.
fn condition(flag: &str) -> String {
    if let Some(not) = flag.strip_prefix('!') {
        return format!("not {}", condition(not));
    }
    match flag {
        "single" => "one item selected",
        "selection" => "with a selection",
        "empty" => "nothing selected",
        "split" => "split view",
        "dir" => "on a folder",
        "file" => "on a file",
        "folder" => "in a folder",
        "sweep" => "selecting as the cursor moves",
        "place" => "on a tag or another place",
        "refs" => "where removing leaves files where they are",
        "drops" => "where files can be dropped",
        "creates" => "where new places can be made",
        "restores" => "in the trash",
        "filtered" => "filtered by name",
        "labelling" => "while links are labelled",
        other => other,
    }
    .to_owned()
}

/// A GPUI context predicate (`Pane && single`) in plain words.
pub fn context_label(context: &str) -> String {
    let mut place = "Anywhere";
    let mut conditions: Vec<String> = Vec::new();
    for token in context.split("&&").map(str::trim) {
        // Alternatives, `(selection || folder)`: each in words, joined.
        if let Some(alts) = token.strip_prefix('(').and_then(|t| t.strip_suffix(')')) {
            let words: Vec<String> = alts.split("||").map(|a| condition(a.trim())).collect();
            conditions.push(words.join(" or "));
            continue;
        }
        match token {
            "Shell" | "" => {}
            context => match PLACES.iter().find(|(_, c, _)| *c == context) {
                Some((_, _, label)) => place = label,
                None => conditions.push(condition(context)),
            },
        }
    }
    if conditions.is_empty() {
        place.to_owned()
    } else {
        format!("{place} ({})", conditions.join(", "))
    }
}

/// What a binding does, in words: the palette's title for shell commands,
/// the registered title for core and plugin commands, a readable form of
/// the name otherwise.
pub fn title_of(app: &App, hint: &KeyHint) -> String {
    if let Some(title) = move_title(&hint.command, &hint.args) {
        return title;
    }
    if let Some((_, _, title)) = crate::palette::ui_commands()
        .iter()
        .find(|(name, args, _)| *name == hint.command && *args == hint.args)
    {
        return (*title).to_owned();
    }
    if hint.command == "ui.view"
        && let Some(id) = hint.args.get("id").and_then(serde_json::Value::as_str)
    {
        let title = app
            .registry
            .views
            .all()
            .into_iter()
            .find(|view| &*view.id == id)
            .map_or_else(|| id.to_owned(), |view| view.title.clone());
        return format!("{title} view");
    }
    if let Some(spec) = app.commands.get(&hint.command) {
        return spec.title.clone();
    }
    let name = hint
        .command
        .strip_prefix("ui.")
        .unwrap_or(&hint.command)
        .replace(['_', '.'], " ");
    let mut chars = name.chars();
    let mut title: String = chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    });
    if let Some(args) = hint.args.as_object().filter(|args| !args.is_empty()) {
        let detail: Vec<String> = args
            .iter()
            .map(|(name, value)| match value.as_str() {
                Some(text) => format!("{name} {text}"),
                None => format!("{name} {value}"),
            })
            .collect();
        title = format!("{title} ({})", detail.join(", "));
    }
    title
}

/// What a binding with arguments does, in words, where the command's title
/// alone would not say: a cursor move (`ui.cursor`, `ui.cursor_pick`, not in
/// the palette but listed with their keys), a place to go (`navigate`).
fn move_title(command: &str, args: &serde_json::Value) -> Option<String> {
    let text = |key| args.get(key).and_then(serde_json::Value::as_str);
    match command {
        "navigate" => {
            let location = text("location")?;
            let place = match location.trim_end_matches('/') {
                "~" => "your home folder",
                path => path.rsplit('/').next().filter(|name| !name.is_empty())?,
            };
            Some(format!("Go to {place}"))
        }
        "ui.cursor_pick" => Some(
            if args.get("advance").and_then(serde_json::Value::as_bool) == Some(false) {
                "Pick or unpick".to_owned()
            } else {
                "Pick or unpick, then go to the next".to_owned()
            },
        ),
        "ui.sidebar_key" => Some(
            match text("key")? {
                "up" => "Go to the place above",
                "down" => "Go to the place below",
                "first" => "Go to the first place",
                "last" => "Go to the last place",
                "left" => "Go to its group, or fold the group",
                "right" => "Unfold the group, or go into it",
                "open" => "Open the place, or fold the group",
                "tab" => "Open the place in a new tab",
                "menu" => "Open its menu",
                "move_up" => "Move it up",
                "move_down" => "Move it down",
                "rename" => "Rename it",
                "remove" => "Remove or hide it",
                _ => return None,
            }
            .to_owned(),
        ),
        "ui.cursor" => {
            let verb = match text("step") {
                Some("extend") => "Extend the selection",
                Some("move") => "Move the cursor",
                Some("follow") => "Go",
                _ => "Select",
            };
            let to = match text("to").unwrap_or("down") {
                "page_up" => "a page up",
                "page_down" => "a page down",
                "half_page_up" => "half a page up",
                "half_page_down" => "half a page down",
                "home" => "to the first item",
                "end" => "to the last item",
                "here" if verb == "Select" => {
                    return Some("Select only the item under the cursor".into());
                }
                "here" => "to the item under the cursor",
                other => other,
            };
            Some(format!("{verb} {to}"))
        }
        _ => None,
    }
}

/// `ctrl-shift-p` as key caps; a binding may be a sequence of strokes.
pub fn key_caps(keys: &str) -> Vec<Vec<String>> {
    keys.split_whitespace()
        .map(|stroke| {
            let mut caps: Vec<String> = Vec::new();
            let mut parts = stroke.split('-');
            while let Some(part) = parts.next() {
                // `ctrl--` binds the minus key: an empty part is a literal `-`.
                let part = if part.is_empty() {
                    parts.next();
                    "-"
                } else {
                    part
                };
                // `G` is how a scheme binds Shift+G: say so, since `g`
                // is drawn as the same cap.
                if part.len() == 1
                    && part.as_bytes()[0].is_ascii_uppercase()
                    && !caps.iter().any(|cap| cap == "Shift")
                {
                    caps.push("Shift".into());
                }
                caps.push(match part {
                    // `secondary` is the standard shortcuts' modifier: Cmd
                    // on a Mac, Ctrl elsewhere, as GPUI reads it.
                    "secondary" | "super" | "cmd" | "win" if cfg!(target_os = "macos") => {
                        "⌘".into()
                    }
                    "ctrl" | "secondary" => "Ctrl".into(),
                    "shift" => "Shift".into(),
                    "alt" if cfg!(target_os = "macos") => "⌥".into(),
                    "alt" => "Alt".into(),
                    "super" | "cmd" | "win" => "Super".into(),
                    "escape" => "Esc".into(),
                    "pageup" => "Page Up".into(),
                    "pagedown" => "Page Down".into(),
                    "left" => "←".into(),
                    "right" => "→".into(),
                    "up" => "↑".into(),
                    "down" => "↓".into(),
                    other if other.chars().count() == 1 => other.to_uppercase(),
                    other => {
                        let mut chars = other.chars();
                        chars.next().map_or_else(String::new, |first| {
                            first.to_uppercase().chain(chars).collect()
                        })
                    }
                });
            }
            caps
        })
        .collect()
}

/// `keys` as GPUI reads them, written one way: `secondary-c` is
/// `ctrl-c` off a Mac, `cmd-c` on one. Two bindings are of the same keys
/// when these agree, whatever spelling each used.
fn normal_keys(keys: &str) -> String {
    keys.split_whitespace()
        .map(|stroke| Keystroke::parse(stroke).map_or_else(|_| stroke.to_owned(), |k| k.unparse()))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A binding as people read it: `ctrl-shift-n` becomes `Ctrl+Shift+N`.
pub fn display_keys(keys: &str) -> String {
    key_caps(keys)
        .iter()
        .map(|stroke| stroke.join("+"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn valid_keys(keys: &str) -> bool {
    !keys.is_empty() && keys.split_whitespace().all(|k| Keystroke::parse(k).is_ok())
}

/// Where a binding comes from, lowest precedence first (ADR-024): a later
/// layer wins where both apply, whatever order plugins were enabled in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Layer {
    /// A plugin's default keys (`Registry::keymap`).
    Plugin,
    /// The shell's own ([`BUILTIN`]).
    Shell,
    /// A key scheme's (`Registry::schemes`: the vim plugin).
    Scheme,
    /// The user's (`keyboard.bindings`).
    User,
}

/// A binding before GPUI has it: `command` is `None` for a user's record
/// that frees its keys.
#[derive(Clone, Debug, PartialEq)]
struct Spec {
    keys: String,
    command: Option<(String, serde_json::Value)>,
    context: String,
    layer: Layer,
}

/// The keymap as the shell knows it: what GPUI binds, what the palette,
/// menus and F1 show, and what it was built from.
pub struct Keymap {
    pub hints: Vec<KeyHint>,
    pub stamp: Stamp,
    /// Whether letters typed in the file list jump to a name: not while the
    /// keymap takes an unmodified character there.
    pub typeahead: bool,
}

/// What a keymap was built from: the registries' revisions, the plugins'
/// (a binding of the user's waits for its plugin's command), and the
/// user's bindings.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stamp {
    pub revisions: (u64, u64, u64),
    pub user: Vec<UserBinding>,
}

impl Stamp {
    /// The registries' revisions now, for a cheap comparison per sync.
    pub fn revisions(app: &App) -> (u64, u64, u64) {
        (
            app.registry.keymap.revision(),
            app.registry.schemes.revision(),
            app.plugins.revision(),
        )
    }
}

/// The keys bound to `command` with exactly `args`, as people read them.
/// The one lookup behind every shortcut the interface shows (menus, the
/// palette, toolbar tooltips, the footers): matching the name alone would
/// show Zoom in's keys beside Zoom out. The highest layer's keys, the first
/// of a layer that binds several (Ctrl+H before Alt+.).
pub fn keys_for<'a>(
    hints: impl IntoIterator<Item = &'a KeyHint>,
    command: &str,
    args: &serde_json::Value,
) -> Option<String> {
    let empty = serde_json::Value::Object(serde_json::Map::new());
    let args = if args.is_null() { &empty } else { args };
    hints
        .into_iter()
        .filter(|hint| hint.command == command && hint.args == *args)
        .fold(None, |best: Option<&KeyHint>, hint| match best {
            Some(best) if best.layer >= hint.layer => Some(best),
            _ => Some(hint),
        })
        .map(|hint| display_keys(&hint.keys))
}

/// `keys_for`, among the bindings whose condition holds where the item is
/// shown: `holds` says whether a flag of the pane's key context holds
/// (`!refs` too), `None` for one it cannot tell, taken to hold. A menu
/// item names the key that does it there (on a tag, Delete is Remove from
/// the tag, not Move to Trash).
pub fn keys_for_where<'a>(
    hints: impl IntoIterator<Item = &'a KeyHint>,
    command: &str,
    args: &serde_json::Value,
    holds: impl Fn(&str) -> Option<bool>,
) -> Option<String> {
    let applies = |context: &str| predicate_holds(context, &holds);
    let empty = serde_json::Value::Object(serde_json::Map::new());
    let args = if args.is_null() { &empty } else { args };
    hints
        .into_iter()
        .filter(|hint| hint.command == command && hint.args == *args && applies(&hint.context))
        .fold(None, |best: Option<&KeyHint>, hint| match best {
            Some(best) if best.layer >= hint.layer => Some(best),
            _ => Some(hint),
        })
        .map(|hint| display_keys(&hint.keys))
}

/// Every binding, in the order of their layers: the plugins', the shell's
/// own, the schemes', the user's. A binding declared twice is listed once,
/// in the higher of its layers.
fn specs(app: &App, user: &[UserBinding]) -> Vec<Spec> {
    let registered = |bindings: Vec<skwir_core::registry::KeyBinding>, layer| {
        bindings.into_iter().map(move |b| Spec {
            keys: b.keys,
            command: Some((b.command.name.to_string(), b.command.args)),
            context: context_of(b.when.as_deref()),
            layer,
        })
    };
    let shell = BUILTIN
        .iter()
        .map(|(keys, command, when)| (*keys, *command, serde_json::json!({}), *when))
        .chain(BUILTIN_ARGS.iter().map(|(keys, command, args, when)| {
            (
                *keys,
                *command,
                serde_json::from_str(args).expect("valid builtin args"),
                *when,
            )
        }))
        .map(|(keys, command, args, when)| Spec {
            keys: keys.into(),
            command: Some((command.into(), args)),
            context: context_of(when),
            layer: Layer::Shell,
        })
        .chain(zoom_specs().map(|(keys, command, args, context)| Spec {
            keys,
            command: Some((command, args)),
            context,
            layer: Layer::Shell,
        }));
    // A binding to a command nothing registers waits for its plugin: it
    // binds nothing meanwhile, and the keys under it work (ADR-024).
    let mine = user.iter().filter(|b| !dormant(app, b)).map(|b| Spec {
        keys: b.keys.clone(),
        command: b
            .command
            .as_ref()
            .map(|c| (c.name.to_string(), c.args.clone())),
        context: context_of(b.when.as_deref()),
        layer: Layer::User,
    });
    let standard: Vec<Spec> = registered(app.registry.keymap.all(), Layer::Plugin)
        .chain(shell)
        .collect();
    let above: Vec<Spec> = registered(app.registry.schemes.all(), Layer::Scheme)
        .chain(mine)
        .flat_map(|spec| like(spec, &standard))
        .collect();
    let specs: Vec<Spec> = standard.into_iter().chain(above).collect();
    let mut seen = std::collections::HashSet::new();
    let mut kept: Vec<Spec> = specs
        .into_iter()
        .rev()
        .filter(|spec| {
            seen.insert((
                spec.keys.clone(),
                spec.command
                    .as_ref()
                    .map(|(name, args)| (name.clone(), args.to_string())),
                spec.context.clone(),
            ))
        })
        .collect();
    kept.reverse();
    kept
}

/// Whether a binding of the user's runs a command nothing registers now:
/// its plugin's, turned off. It is kept, and waits for it.
pub fn dormant(app: &App, binding: &UserBinding) -> bool {
    binding.command.as_ref().is_some_and(|command| {
        &*command.name != skwir_core::keymap::LIKE && app.commands.get(&command.name).is_none()
    })
}

/// A binding to `keys.like` as the bindings of the keys it names, under
/// its own keys and in its layer, where they apply within its own place
/// (`pane`): a scheme's `d` does what Delete does in the file list, and
/// not Delete's meaning in the sidebar. Any other binding as it is.
fn like(spec: Spec, standard: &[Spec]) -> Vec<Spec> {
    let target = match &spec.command {
        Some((name, args)) if name == skwir_core::keymap::LIKE => {
            args["keys"].as_str().unwrap_or_default().to_owned()
        }
        _ => return vec![spec],
    };
    let within = conditions(&spec.context);
    let target = normal_keys(&target);
    standard
        .iter()
        .filter(|s| normal_keys(&s.keys) == target && s.command.is_some())
        .filter(|s| {
            let theirs = conditions(&s.context);
            within.iter().all(|atom| theirs.contains(atom))
        })
        .map(|s| Spec {
            keys: spec.keys.clone(),
            command: s.command.clone(),
            context: s.context.clone(),
            layer: spec.layer,
        })
        .collect()
}

/// The conditions of a context predicate (`Pane && single`), each an
/// atom: a place, a flag, or a group of alternatives.
fn conditions(context: &str) -> Vec<&str> {
    context
        .split("&&")
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .collect()
}

/// Which specs a later one hides: GPUI takes the deepest place first, then
/// the latest binding, so a later binding of the same keys that asks for no
/// more than an earlier one (`Pane` over `Pane && empty`) always wins where
/// the earlier would apply.
/// Each spec's keys are read once, and compared only with the specs of
/// the same keys: every spec against every later one, parsing both each
/// time, cost the first frame milliseconds.
fn shadowed(specs: &[Spec]) -> Vec<bool> {
    let mut by_keys: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, spec) in specs.iter().enumerate() {
        by_keys.entry(normal_keys(&spec.keys)).or_default().push(i);
    }
    let mut hidden = vec![false; specs.len()];
    for same in by_keys.values() {
        for (n, &i) in same.iter().enumerate() {
            let mine = conditions(&specs[i].context);
            hidden[i] = same[n + 1..].iter().any(|&later| {
                conditions(&specs[later].context)
                    .iter()
                    .all(|atom| mine.contains(atom))
            });
        }
    }
    hidden
}

/// Whether a binding takes a character typed in the file list: its first
/// stroke is one character, with Shift at most (`j`, `G`, `/`, `g g`).
fn takes_a_character(spec: &Spec) -> bool {
    let place = conditions(&spec.context).first().copied();
    let Some(first) = spec.keys.split_whitespace().next() else {
        return false;
    };
    let Ok(stroke) = Keystroke::parse(first) else {
        return false;
    };
    let m = stroke.modifiers;
    place == Some(PANE)
        && spec.command.is_some()
        && stroke.key.chars().count() == 1
        && !(m.control || m.alt || m.platform || m.function)
}

/// Replace every binding in GPUI with the registries' current keymaps, the
/// shell's and the text input's own, and the user's. Called at startup and
/// again whenever one of them changes (a plugin enabled or disabled, a
/// shortcut saved), so a plugin's keys come and go with it.
pub fn install(app: &App, user: Vec<UserBinding>, cx: &mut gpui::App) -> Keymap {
    // Read before the bindings: a change in between is picked up next time.
    let revisions = Stamp::revisions(app);
    let specs = specs(app, &user);
    let hidden = shadowed(&specs);
    let mut hints = Vec::new();
    let mut bindings = Vec::new();
    let mut typeahead = true;
    for (spec, hidden) in specs.into_iter().zip(hidden) {
        if !valid_keys(&spec.keys) {
            tracing::warn!(
                keys = spec.keys,
                layer = ?spec.layer,
                "skipping key binding with unparsable keys"
            );
            continue;
        }
        if !hidden && takes_a_character(&spec) {
            typeahead = false;
        }
        let Some((name, args)) = spec.command else {
            // Freed keys: nothing below this binding runs where it applies.
            bindings.push(KeyBinding::new(
                &spec.keys,
                gpui::NoAction,
                Some(&spec.context),
            ));
            continue;
        };
        if spec.context == ROOT && !TERMINAL_PASSTHROUGH.contains(&name.as_str()) {
            // The deeper context's binding to nothing wins, and the key
            // goes on to the terminal's own listener.
            bindings.push(KeyBinding::new(&spec.keys, gpui::NoAction, Some(TERMINAL)));
        }
        if !hidden {
            hints.push(KeyHint {
                keys: spec.keys.clone(),
                command: name.clone(),
                args: args.clone(),
                context: spec.context.clone(),
                layer: spec.layer,
            });
        }
        bindings.push(KeyBinding::new(
            &spec.keys,
            RunCommand { name, args },
            Some(&spec.context),
        ));
    }
    cx.clear_key_bindings();
    cx.bind_keys(crate::input::key_bindings());
    cx.bind_keys(bindings);
    Keymap {
        hints,
        stamp: Stamp { revisions, user },
        typeahead,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BUILTIN, BUILTIN_ARGS, KeyHint, Layer, Spec, context_label, context_of, display_keys,
        key_caps, keys_for_where, shadowed, specs, takes_a_character, valid_keys, zoom_specs,
    };
    use skwir_core::command::Invocation;
    use skwir_core::keymap::UserBinding;
    use skwir_core::registry::KeyBinding;

    fn spec(keys: &str, command: &str, when: Option<&str>, layer: Layer) -> Spec {
        Spec {
            keys: keys.into(),
            command: Some((command.into(), serde_json::json!({}))),
            context: context_of(when),
            layer,
        }
    }

    /// A scheme's key that does what Delete does takes each of Delete's
    /// bindings in the file list, conditions and all, and none of its
    /// meanings elsewhere (the sidebar's).
    #[test]
    fn a_key_like_another_takes_its_bindings_where_they_apply() {
        let standard = vec![
            spec(
                "delete",
                "trash",
                Some("pane selection !refs !restores"),
                Layer::Plugin,
            ),
            spec(
                "delete",
                "take_out",
                Some("pane selection refs"),
                Layer::Plugin,
            ),
            spec("delete", "ui.sidebar_key", Some("sidebar"), Layer::Shell),
            spec("f2", "ui.rename_prompt", Some("pane single"), Layer::Shell),
        ];
        let d = Spec {
            keys: "d".into(),
            command: Some((
                skwir_core::keymap::LIKE.into(),
                serde_json::json!({"keys": "delete"}),
            )),
            context: context_of(Some("pane")),
            layer: Layer::Scheme,
        };
        let got: Vec<(String, String, String, Layer)> = super::like(d, &standard)
            .into_iter()
            .map(|s| (s.keys, s.command.unwrap().0, s.context, s.layer))
            .collect();
        assert_eq!(
            got,
            [
                (
                    "d".to_owned(),
                    "trash".to_owned(),
                    standard[0].context.clone(),
                    Layer::Scheme
                ),
                (
                    "d".to_owned(),
                    "take_out".to_owned(),
                    standard[1].context.clone(),
                    Layer::Scheme
                ),
            ]
        );
        let plain = spec("q", "ui.quit", Some("pane"), Layer::Scheme);
        assert_eq!(super::like(plain, &standard).len(), 1);
    }

    /// A later binding hides an earlier one of the same keys when it asks
    /// for no more: GPUI would never run the earlier where both apply.
    #[test]
    fn a_later_binding_that_asks_for_no_more_hides_an_earlier_one() {
        let specs = [
            spec(
                "escape",
                "clipboard_clear",
                Some("pane empty"),
                Layer::Plugin,
            ),
            spec(
                "f2",
                "ui.rename_bulk_prompt",
                Some("pane selection"),
                Layer::Plugin,
            ),
            spec("space", "ui.peek", Some("pane"), Layer::Shell),
            spec("f2", "ui.rename_prompt", Some("pane single"), Layer::Shell),
            spec("ctrl-f", "ui.search", None, Layer::Plugin),
            spec("escape", "ui.cursor", Some("pane"), Layer::Scheme),
            spec("space", "ui.cursor_pick", Some("pane"), Layer::Scheme),
            spec("ctrl-f", "ui.cursor", Some("pane"), Layer::Scheme),
            spec(
                "escape",
                "select_sweep_end",
                Some("pane sweep"),
                Layer::Scheme,
            ),
        ];
        let hidden: Vec<&str> = specs
            .iter()
            .zip(shadowed(&specs))
            .filter(|(_, hidden)| *hidden)
            .map(|(spec, _)| spec.command.as_ref().unwrap().0.as_str())
            .collect();
        // F2 for one item and for several both apply somewhere; Ctrl+F in
        // the list leaves Ctrl+F elsewhere; Escape while sweeping adds a
        // case, it hides nothing.
        assert_eq!(hidden, ["clipboard_clear", "ui.peek"]);
    }

    #[test]
    fn letters_bound_in_the_file_list_stop_type_ahead() {
        let takes = |keys: &str, when: Option<&str>| {
            takes_a_character(&spec(keys, "x", when, Layer::Scheme))
        };
        assert!(takes("j", Some("pane")));
        assert!(takes("G", Some("pane")));
        assert!(takes("shift-g", Some("pane selection")));
        assert!(takes("g g", Some("pane")));
        assert!(takes("/", Some("pane")));
        assert!(!takes("space", Some("pane")));
        assert!(!takes("ctrl-j", Some("pane")));
        assert!(!takes("j", None), "anywhere is not the file list alone");
        assert!(!takes("m", Some("reader")));
        let freed = Spec {
            command: None,
            ..spec("j", "x", Some("pane"), Layer::User)
        };
        assert!(!takes_a_character(&freed), "freeing a key binds nothing");
    }

    /// The layers stand in their order whatever order the plugins came in:
    /// a scheme over the shell over a plugin, the user over everything.
    #[test]
    fn layers_stand_in_their_order() {
        let tmp = tempfile::tempdir().unwrap();
        let app = skwir_core::App::new(skwir_core::app::AppConfig::ephemeral(tmp.path())).unwrap();
        let bind = |keys: &str, command: &str| KeyBinding {
            keys: keys.into(),
            command: Invocation::new(command),
            when: Some("pane".into()),
        };
        app.registry
            .schemes
            .add("scheme", bind("space", "ui.cursor_pick"));
        app.registry
            .keymap
            .add("late-plugin", bind("space", "late"));
        let user = [UserBinding {
            id: "1".into(),
            keys: "space".into(),
            command: None,
            when: Some("pane".into()),
        }];
        let space: Vec<(Option<String>, Layer)> = specs(&app, &user)
            .into_iter()
            .filter(|s| s.keys == "space" && s.context == "Pane")
            .map(|s| (s.command.map(|(name, _)| name), s.layer))
            .collect();
        assert_eq!(
            space,
            [
                (Some("late".into()), Layer::Plugin),
                (Some("ui.peek".into()), Layer::Shell),
                (Some("ui.cursor_pick".into()), Layer::Scheme),
                (None, Layer::User),
            ]
        );
    }

    /// A binding of the user's to a command nothing registers (its plugin
    /// is off) binds nothing: the key under it works until the plugin is
    /// back, and then the binding is the user's again.
    #[test]
    fn a_binding_waits_for_the_plugin_of_its_command() {
        let tmp = tempfile::tempdir().unwrap();
        let app = skwir_core::App::new(skwir_core::app::AppConfig::ephemeral(tmp.path())).unwrap();
        app.registry.keymap.add(
            "plugin",
            KeyBinding {
                keys: "ctrl-m".into(),
                command: Invocation::new("plugin.default"),
                when: None,
            },
        );
        let user = [UserBinding {
            id: "1".into(),
            keys: "ctrl-m".into(),
            command: Some(Invocation::new("later.open")),
            when: None,
        }];
        let on_ctrl_m = |app: &skwir_core::App| -> Vec<String> {
            specs(app, &user)
                .into_iter()
                .filter(|s| s.keys == "ctrl-m")
                .filter_map(|s| s.command.map(|(name, _)| name))
                .collect()
        };
        assert!(super::dormant(&app, &user[0]));
        assert_eq!(on_ctrl_m(&app), ["plugin.default"]);
        app.commands.register::<skwir_core::command::NoArgs, _>(
            "later",
            "later.open",
            "Open",
            "",
            |_, _| Ok(skwir_core::command::Outcome::Done),
        );
        assert!(!super::dormant(&app, &user[0]));
        assert_eq!(on_ctrl_m(&app), ["plugin.default", "later.open"]);
    }

    /// A key GPUI cannot parse is skipped with a warning at startup: the
    /// shell's own must all parse, including the unusual names (the Menu
    /// key, the keypad's plus).
    #[test]
    fn every_builtin_key_parses() {
        let keys = BUILTIN
            .iter()
            .map(|(keys, ..)| (*keys).to_owned())
            .chain(BUILTIN_ARGS.iter().map(|(keys, ..)| (*keys).to_owned()))
            .chain(zoom_specs().map(|(keys, ..)| keys));
        for keys in keys {
            assert!(valid_keys(&keys), "{keys}");
        }
    }

    #[test]
    fn every_zoom_key_zooms_everything_that_zooms() {
        let zooms: Vec<_> = zoom_specs().collect();
        for context in [context_of(None), context_of(Some("reader"))] {
            for keys in ["ctrl-=", "ctrl-+", "ctrl-add", "ctrl--", "ctrl-0"] {
                assert!(
                    zooms.iter().any(|(k, _, _, c)| super::normal_keys(k)
                        == super::normal_keys(keys)
                        && *c == context),
                    "{keys} in {context}"
                );
            }
        }
    }

    #[test]
    fn a_place_to_go_is_named() {
        let title = |args| super::move_title("navigate", &args);
        assert_eq!(
            title(serde_json::json!({ "location": "~" })).as_deref(),
            Some("Go to your home folder")
        );
        assert_eq!(
            title(serde_json::json!({ "location": "/nowhere/Téléchargements" })).as_deref(),
            Some("Go to Téléchargements")
        );
        assert_eq!(title(serde_json::json!({})), None);
    }

    #[test]
    fn contexts_read_as_places_and_conditions() {
        assert_eq!(context_label("Shell"), "Anywhere");
        assert_eq!(
            context_label("Pane && single"),
            "File list (one item selected)"
        );
    }

    #[test]
    fn keys_become_caps_including_the_minus_key() {
        assert_eq!(key_caps("ctrl-shift-p"), vec![vec!["Ctrl", "Shift", "P"]]);
        assert_eq!(key_caps("ctrl--"), vec![vec!["Ctrl", "-"]]);
        assert_eq!(key_caps("g g"), vec![vec!["G"], vec!["G"]]);
        assert_eq!(key_caps("G"), vec![vec!["Shift", "G"]]);
        assert_eq!(key_caps("shift-g"), vec![vec!["Shift", "G"]]);
        assert_eq!(display_keys(", M"), ", Shift+M");
        assert_eq!(key_caps("alt-pageup"), vec![vec!["Alt", "Page Up"]]);
        assert_eq!(display_keys("ctrl-shift-n"), "Ctrl+Shift+N");
        if !cfg!(target_os = "macos") {
            assert_eq!(display_keys("secondary-shift-n"), "Ctrl+Shift+N");
            assert_eq!(
                super::normal_keys("secondary-c g"),
                super::normal_keys("ctrl-c g")
            );
        }
    }

    #[test]
    fn a_menu_names_the_key_that_does_its_item_where_it_is() {
        let hint = |keys: &str, command: &str, when: &str| KeyHint {
            keys: keys.into(),
            command: command.into(),
            args: serde_json::json!({}),
            context: context_of(Some(when)),
            layer: Layer::Plugin,
        };
        let hints = [
            hint("delete", "trash", "pane selection !refs"),
            hint("delete", "take_out", "pane selection refs"),
        ];
        let in_a_tag = |flag: &str| match flag {
            "selection" | "refs" => Some(true),
            _ => None,
        };
        let args = serde_json::json!({});
        assert_eq!(keys_for_where(&hints, "trash", &args, in_a_tag), None);
        assert_eq!(
            keys_for_where(&hints, "take_out", &args, in_a_tag).as_deref(),
            Some("Delete")
        );
        let in_a_folder = |flag: &str| (flag == "refs").then_some(false);
        assert_eq!(
            keys_for_where(&hints, "trash", &args, in_a_folder).as_deref(),
            Some("Delete")
        );
    }

    #[test]
    fn when_tokens_map_to_contexts_and_flags() {
        assert_eq!(context_of(None), "Shell");
        assert_eq!(context_of(Some("pane")), "Pane");
        assert_eq!(context_of(Some("pane single")), "Pane && single");
        assert_eq!(context_of(Some("address")), "AddressBar");
        assert_eq!(context_of(Some("  ")), "Shell");
        assert_eq!(
            context_of(Some("pane none")),
            "Pane && empty",
            "the old spelling of nothing selected"
        );
        assert_eq!(
            context_of(Some("pane selection|folder")),
            "Pane && (selection || folder)"
        );
        assert_eq!(
            context_label(&context_of(Some("pane selection|folder"))),
            "File list (with a selection or in a folder)"
        );
    }
}
