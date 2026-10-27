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
/// A 3D model's preview, once clicked: arrows turn it, Shift and the
/// arrows move it, the zoom keys zoom it.
pub const MODEL: &str = "ModelPreview";
/// The pinned player's dock.
pub const PLAYER: &str = "Player";
pub const REVIEW: &str = "Review";
pub const JUMP: &str = "Jump";
/// `ui.choose`'s picker.
pub const CHOOSE: &str = "Choose";
/// A form a plugin asks the user to fill (`crate::form`).
pub const FORM: &str = "Form";
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
/// The preview on the whole screen (`fullscreen.rs`).
pub const FULLSCREEN: &str = "Fullscreen";
/// A picture on the whole screen, which plays as a slideshow.
pub const SLIDESHOW: &str = "Slideshow";
/// The sidebar while it has the keyboard (F6): the arrows walk its groups
/// and places (`sidebar.rs`).
pub const SIDEBAR: &str = "Sidebar";
/// A pane's view buttons, one of which has the keyboard: the Menu key
/// says which views have one (`address::view_control`).
pub const VIEW_BUTTONS: &str = "ViewButtons";
/// The timeline's bars, once clicked or reached by F6: the arrows go
/// from bar to bar, Enter lists the bars chosen (`timeline.rs`).
pub const TIMELINE: &str = "Timeline";

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
/// takes the palette or Quit with it, and so do the core's commands no
/// plugin is about (hidden files, the filter). Keys for the other core and
/// plugin commands are the plugins' (ops-basic binds the file operations).
const BUILTIN: &[(&str, &str, Option<&str>)] = &[
    ("f2", "ui.rename_prompt", Some("pane single !restores")),
    // Into the folder shown, like Ctrl+V: a search or the shelf has none.
    ("secondary-shift-n", "ui.mkdir_prompt", Some("pane folder")),
    // A new place where the location makes them (a tag's: a tag in it).
    ("secondary-shift-n", "ui.create_here", Some("pane creates")),
    ("secondary-l", "ui.focus_address", None),
    ("secondary-shift-p", "ui.palette", None),
    ("secondary-q", "ui.quit", None),
    ("ctrl-h", "toggle_hidden", None),
    // Dolphin's key for it, beside GNOME's.
    ("alt-.", "toggle_hidden", None),
    ("f9", "ui.toggle_sidebar", None),
    // The keyboard into the sidebar, and back to the files.
    ("f6", "ui.focus_next_part", None),
    ("escape", "ui.focus_pane", Some("sidebar")),
    ("escape", "ui.focus_pane", Some("timeline")),
    ("enter", "ui.timeline_list", Some("timeline")),
    ("tab", "ui.toolbar_next", None),
    ("shift-tab", "ui.toolbar_prev", None),
    // Tab completes a path or a place typed in the address, while there
    // is something to complete; else it walks on, as everywhere.
    ("tab", "ui.omnibox_complete", Some("address completes")),
    // Where else the address field's words could search; a recent search
    // forgotten (ADR-098).
    ("alt-down", "ui.omnibox_scopes", Some("address")),
    ("shift-delete", "ui.omnibox_forget", Some("address")),
    ("enter", "ui.toolbar_activate", Some("toolbar_button")),
    ("space", "ui.toolbar_activate", Some("toolbar_button")),
    ("escape", "ui.toolbar_close", Some("toolbar_popup")),
    // A popup's rows are buttons: the arrows walk them as Tab does, and
    // Home and End reach its ends.
    ("down", "ui.toolbar_next", Some("toolbar_popup")),
    ("up", "ui.toolbar_prev", Some("toolbar_popup")),
    ("home", "ui.toolbar_first", Some("toolbar_popup")),
    ("end", "ui.toolbar_last", Some("toolbar_popup")),
    // A setting's values on one row of a popup, or its tiles: Left and
    // Right walk them too (`toolbar::CHOICE`).
    ("left", "ui.toolbar_prev", Some("toolbar_button choice")),
    ("right", "ui.toolbar_next", Some("toolbar_button choice")),
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
    ("secondary-enter", "tab.open", Some("pane selection")),
    ("enter", "ui.accept", Some("confirm")),
    ("escape", "ui.dismiss", Some("confirm")),
    ("space", "ui.peek", Some("pane")),
    // The space bar with Shift is Space whatever the layout types there (a
    // narrow no-break space on French Ergo-L): gpui patch 0039, on Linux.
    ("ctrl-shift-space", "ui.toggle_preview", None),
    // F3 splits and F11 shows the side panel, as in Dolphin; inside a
    // document F3 stays "next match", the deeper context wins.
    ("f11", "ui.toggle_preview", None),
    // Windows' and KDE's Properties first, the one menus name; then
    // GNOME's and a Mac's Get Info.
    ("alt-enter", "ui.properties", None),
    ("secondary-i", "ui.properties", None),
    // The address field on the folder shown alone (Ctrl+F, the query
    // plugin's, looks below it too): its words filter it as typed.
    ("ctrl-shift-f", "ui.filter", None),
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
    // Asked, as a drop asks: what a plugin offers too (Extract Here).
    ("shift-f7", "ui.give_other", Some("pane split selection")),
    ("ctrl-shift-o", "ui.terminal_links", Some("terminal")),
    // Every terminal's copy and paste: Ctrl+C and Ctrl+V are the
    // program's. Unbound, Ctrl+Shift+C reached it as Ctrl+C.
    ("ctrl-shift-c", "ui.terminal_copy", Some("terminal")),
    ("ctrl-shift-v", "ui.terminal_paste", Some("terminal")),
    ("ctrl-shift-a", "ui.terminal_select_all", Some("terminal")),
    // The selection's paths at the terminal's prompt, from the files or
    // the terminal, as Midnight Commander puts a path on its command line.
    // In the terminal it was Enter to the program.
    (
        "ctrl-shift-enter",
        "ui.terminal_paths",
        Some("pane|terminal"),
    ),
    // The terminal's menu from the keyboard. Shift+F10 stays the
    // program's, as the other F-keys are.
    ("menu", "ui.terminal_menu", Some("terminal")),
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
    // The timeline's bars: one at a time, Shift extending what is chosen,
    // the page keys a window of them.
    (
        "left",
        "ui.timeline_cursor",
        r#"{"by":-1}"#,
        Some("timeline"),
    ),
    (
        "right",
        "ui.timeline_cursor",
        r#"{"by":1}"#,
        Some("timeline"),
    ),
    (
        "shift-left",
        "ui.timeline_cursor",
        r#"{"by":-1,"extend":true}"#,
        Some("timeline"),
    ),
    (
        "shift-right",
        "ui.timeline_cursor",
        r#"{"by":1,"extend":true}"#,
        Some("timeline"),
    ),
    (
        "home",
        "ui.timeline_cursor",
        r#"{"to":"first"}"#,
        Some("timeline"),
    ),
    (
        "end",
        "ui.timeline_cursor",
        r#"{"to":"last"}"#,
        Some("timeline"),
    ),
    (
        "shift-home",
        "ui.timeline_cursor",
        r#"{"to":"first","extend":true}"#,
        Some("timeline"),
    ),
    (
        "shift-end",
        "ui.timeline_cursor",
        r#"{"to":"last","extend":true}"#,
        Some("timeline"),
    ),
    (
        "pageup",
        "ui.timeline_pan",
        r#"{"toward":"earlier"}"#,
        Some("timeline"),
    ),
    (
        "pagedown",
        "ui.timeline_pan",
        r#"{"toward":"later"}"#,
        Some("timeline"),
    ),
    // The lists one picks from while typing (the palette, a choice, Jump,
    // the tag editor, the address's suggestions, a long list of choices in
    // Settings): one command, the same
    // keys, the arrows round their ends, the page keys a page at a time.
    // What a question lists before deleting scrolls by the same keys.
    (
        "up",
        "ui.list_move",
        r#"{"delta":-1}"#,
        Some("palette|jump|choose|form|tag_editor|address|review|confirm|setting_choices"),
    ),
    (
        "down",
        "ui.list_move",
        r#"{"delta":1}"#,
        Some("palette|jump|choose|form|tag_editor|address|review|confirm|setting_choices"),
    ),
    (
        "pageup",
        "ui.list_move",
        r#"{"to":"page_up"}"#,
        Some("palette|jump|choose|form|tag_editor|address|review|confirm|setting_choices"),
    ),
    (
        "pagedown",
        "ui.list_move",
        r#"{"to":"page_down"}"#,
        Some("palette|jump|choose|form|tag_editor|address|review|confirm|setting_choices"),
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
    (
        "shift-enter",
        "ui.omnibox_submit",
        r#"{"open":true}"#,
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
    // What the view buttons' right-click opens: which views have one.
    (
        "menu",
        "ui.toolbar_menu",
        r#"{"kind":"view_buttons"}"#,
        Some("view_buttons"),
    ),
    (
        "shift-f10",
        "ui.toolbar_menu",
        r#"{"kind":"view_buttons"}"#,
        Some("view_buttons"),
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
    // The whole screen, from whatever shows the preview, and back.
    ("f", "ui.preview_fullscreen", "{}", Some("media")),
    ("f", "ui.preview_fullscreen", "{}", Some("image")),
    ("f", "ui.preview_fullscreen", "{}", Some("preview")),
    ("f", "ui.preview_fullscreen", "{}", Some("peek")),
    ("f", "ui.preview_fullscreen", "{}", Some("fullscreen")),
    ("escape", "ui.preview_fullscreen", "{}", Some("fullscreen")),
    ("c", "ui.fullscreen_controls", "{}", Some("fullscreen")),
    (
        "left",
        "ui.fullscreen_step",
        r#"{"delta":-1}"#,
        Some("fullscreen"),
    ),
    (
        "right",
        "ui.fullscreen_step",
        r#"{"delta":1}"#,
        Some("fullscreen"),
    ),
    ("space", "ui.slideshow", "{}", Some("slideshow")),
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
    // The arrows over what a preview shows, as a document reader's: Up and
    // Down move it (a page read a page at a time turns at its edge), Left
    // and Right move it sideways when it is wider than the panel, else go
    // to the page, section or picture before or after, as on the whole
    // screen. What has arrows of its own takes them where it has the
    // keyboard, deeper: an archive's files, a map, rows of text.
    ("up", "ui.preview_scroll", r#"{"to":"up"}"#, Some("preview")),
    (
        "down",
        "ui.preview_scroll",
        r#"{"to":"down"}"#,
        Some("preview"),
    ),
    (
        "left",
        "ui.preview_scroll",
        r#"{"to":"left"}"#,
        Some("preview"),
    ),
    (
        "right",
        "ui.preview_scroll",
        r#"{"to":"right"}"#,
        Some("preview"),
    ),
    ("up", "ui.preview_scroll", r#"{"to":"up"}"#, Some("image")),
    (
        "down",
        "ui.preview_scroll",
        r#"{"to":"down"}"#,
        Some("image"),
    ),
    (
        "left",
        "ui.preview_scroll",
        r#"{"to":"left"}"#,
        Some("image"),
    ),
    (
        "right",
        "ui.preview_scroll",
        r#"{"to":"right"}"#,
        Some("image"),
    ),
    // A map moves under the arrows, as it does under a drag.
    ("left", "ui.map_pan", r#"{"dx":-1,"dy":0}"#, Some("map")),
    ("right", "ui.map_pan", r#"{"dx":1,"dy":0}"#, Some("map")),
    ("up", "ui.map_pan", r#"{"dx":0,"dy":-1}"#, Some("map")),
    ("down", "ui.map_pan", r#"{"dx":0,"dy":1}"#, Some("map")),
    // Its places one by one, as the Map view's go (`PaneView::nav`).
    ("pagedown", "ui.map_point", r#"{"delta":1}"#, Some("map")),
    ("pageup", "ui.map_point", r#"{"delta":-1}"#, Some("map")),
    // A model turns under the arrows, as under a drag; Shift moves it, as
    // with the pointer.
    (
        "left",
        "ui.model_turn",
        r#"{"dx":-1,"dy":0}"#,
        Some("model"),
    ),
    (
        "right",
        "ui.model_turn",
        r#"{"dx":1,"dy":0}"#,
        Some("model"),
    ),
    ("up", "ui.model_turn", r#"{"dx":0,"dy":-1}"#, Some("model")),
    ("down", "ui.model_turn", r#"{"dx":0,"dy":1}"#, Some("model")),
    (
        "shift-left",
        "ui.model_pan",
        r#"{"dx":-1,"dy":0}"#,
        Some("model"),
    ),
    (
        "shift-right",
        "ui.model_pan",
        r#"{"dx":1,"dy":0}"#,
        Some("model"),
    ),
    (
        "shift-up",
        "ui.model_pan",
        r#"{"dx":0,"dy":-1}"#,
        Some("model"),
    ),
    (
        "shift-down",
        "ui.model_pan",
        r#"{"dx":0,"dy":1}"#,
        Some("model"),
    ),
    // A text read where it is shown goes row by row; Alt+Z wraps its lines
    // or not. Every structured preview goes a screen at a time, or to
    // either end.
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
        Some("inspector"),
    ),
    (
        "pagedown",
        "ui.inspect",
        r#"{"action":"scroll","to":"page_down"}"#,
        Some("inspector"),
    ),
    (
        "ctrl-home",
        "ui.inspect",
        r#"{"action":"scroll","to":"start"}"#,
        Some("inspector"),
    ),
    (
        "ctrl-end",
        "ui.inspect",
        r#"{"action":"scroll","to":"end"}"#,
        Some("inspector"),
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
    // The Keyboard page's actions, walked as the sidebar's places; Enter
    // changes the key the cursor is on, or adds one.
    (
        "up",
        "ui.settings_key",
        r#"{"key":"up"}"#,
        Some("setting_keys"),
    ),
    (
        "down",
        "ui.settings_key",
        r#"{"key":"down"}"#,
        Some("setting_keys"),
    ),
    (
        "home",
        "ui.settings_key",
        r#"{"key":"first"}"#,
        Some("setting_keys"),
    ),
    (
        "end",
        "ui.settings_key",
        r#"{"key":"last"}"#,
        Some("setting_keys"),
    ),
    (
        "pageup",
        "ui.settings_key",
        r#"{"key":"page_up"}"#,
        Some("setting_keys"),
    ),
    (
        "pagedown",
        "ui.settings_key",
        r#"{"key":"page_down"}"#,
        Some("setting_keys"),
    ),
    (
        "left",
        "ui.settings_key",
        r#"{"key":"left"}"#,
        Some("setting_keys"),
    ),
    (
        "right",
        "ui.settings_key",
        r#"{"key":"right"}"#,
        Some("setting_keys"),
    ),
    (
        "enter",
        "ui.settings_key",
        r#"{"key":"open"}"#,
        Some("setting_keys"),
    ),
    (
        "space",
        "ui.settings_key",
        r#"{"key":"open"}"#,
        Some("setting_keys"),
    ),
    (
        "delete",
        "ui.settings_key",
        r#"{"key":"remove"}"#,
        Some("setting_keys"),
    ),
    (
        "backspace",
        "ui.settings_key",
        r#"{"key":"remove"}"#,
        Some("setting_keys"),
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

/// The terminal's keys that differ by system, as `BUILTIN_ARGS`: what
/// that system's terminals do. On Linux, Shift+Insert pastes what the
/// middle button pastes, the primary selection (xterm, Konsole, foot,
/// kitty, Alacritty).
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
const TERMINAL_KEYS: &[(&str, &str, &str, Option<&str>)] = &[(
    "shift-insert",
    "ui.terminal_paste",
    r#"{"from":"primary"}"#,
    Some("terminal"),
)];
/// A Mac's terminals copy and paste with Cmd, which a program never gets.
#[cfg(target_os = "macos")]
const TERMINAL_KEYS: &[(&str, &str, &str, Option<&str>)] = &[
    ("cmd-c", "ui.terminal_copy", "{}", Some("terminal")),
    ("cmd-v", "ui.terminal_paste", "{}", Some("terminal")),
];
/// Windows Terminal's and the console's other keys for the clipboard.
#[cfg(not(any(target_os = "linux", target_os = "freebsd", target_os = "macos")))]
const TERMINAL_KEYS: &[(&str, &str, &str, Option<&str>)] = &[
    ("ctrl-insert", "ui.terminal_copy", "{}", Some("terminal")),
    ("shift-insert", "ui.terminal_paste", "{}", Some("terminal")),
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
const ZOOMS: [(&str, &str, Option<&str>); 6] = [
    ("ui.zoom", "dir", None),
    ("ui.preview_zoom", "dir", Some("reader")),
    ("ui.preview_zoom", "dir", Some("image")),
    ("ui.preview_zoom", "dir", Some("map")),
    ("ui.preview_zoom", "dir", Some("model")),
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

/// A place a binding can name: its `when` token, the key context it is,
/// what F1 and the Keyboard page call it, and whether it is inside a
/// dialog or a menu (the Keyboard page lists those keys last, folded).
pub(crate) struct Place {
    pub token: &'static str,
    pub context: &'static str,
    pub label: &'static str,
    pub inside: bool,
}

const fn place(token: &'static str, context: &'static str, label: &'static str) -> Place {
    Place {
        token,
        context,
        label,
        inside: false,
    }
}

const fn inside(token: &'static str, context: &'static str, label: &'static str) -> Place {
    Place {
        token,
        context,
        label,
        inside: true,
    }
}

/// Every place a binding can name, once, in the order the Keyboard page
/// lists them. `context_of`, `context_label` and `place_of` read it, so a
/// new place is one row. Any other token is taken as it is: a condition
/// (`single`), or a context by its own name (`ContextMenu`).
pub(crate) const PLACES: &[Place] = &[
    place("pane", PANE, "File list"),
    place("sidebar", SIDEBAR, "Sidebar"),
    place("address", ADDRESS, "Address bar"),
    place("view_buttons", VIEW_BUTTONS, "View buttons"),
    place("peek", PEEK, "Peek"),
    place("preview", PREVIEW, "Documents and structured previews"),
    place("reader", READER, "Document reader"),
    place("inspector", INSPECTOR, "Structured previews"),
    place("text", TEXT, "Text in the preview"),
    place("preview_find", PREVIEW_FIND, "Preview search"),
    place("members", MEMBERS, "Files of an archive or a message"),
    place(
        "member",
        MEMBER,
        "A file shown from an archive or a message",
    ),
    place("image", IMAGE, "Picture preview"),
    place("map", MAP, "Map preview"),
    place("model", MODEL, "3D model preview"),
    place("media", MEDIA, "Media player"),
    place("fullscreen", FULLSCREEN, "Preview on the whole screen"),
    place("slideshow", SLIDESHOW, "Picture on the whole screen"),
    place("player", PLAYER, "Pinned player"),
    place("terminal", TERMINAL, "Terminal"),
    place("timeline", TIMELINE, "Timeline"),
    inside("palette", PALETTE, "Command palette"),
    inside("jump", JUMP, "Jump"),
    inside("choose", CHOOSE, "Choosing from a list"),
    inside("form", FORM, "Forms"),
    inside("tag_editor", TAG_EDITOR, "Tag editor"),
    inside("bulk", BULK, "Bulk rename"),
    inside("review", REVIEW, "Plan review"),
    inside("confirm", CONFIRM, "Confirmation dialogs"),
    inside("menu", MENU, "Context menu"),
    inside("toolbar_button", "ToolbarButton", "Toolbars"),
    inside("toolbar_popup", "ToolbarPopup", "Toolbars"),
    inside("shortcuts", "Shortcuts", "Shortcut overview"),
    inside("about", "About", "About skwir"),
    inside("settings", "Settings", "Settings"),
    inside("settings_nav", "SettingsNav", "Settings"),
    inside("setting_control", "SettingControl", "Settings"),
    inside("setting_choices", "SettingChoices", "Settings"),
    inside("setting_number", "SettingNumber", "Settings"),
    inside("setting_record", "SettingRecord", "Settings"),
    inside("setting_keys", "SettingKeys", "Settings"),
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
        PLACES
            .iter()
            .find(|place| place.token == t)
            .map_or_else(|| t.to_owned(), |place| place.context.to_owned())
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
pub(crate) fn condition(flag: &str) -> String {
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
        "search" => "in a search",
        "sweep" => "selecting as the cursor moves",
        "place" => "on a tag or another place",
        "refs" => "where removing leaves files where they are",
        "drops" => "where files can be dropped",
        "creates" => "where new places can be made",
        "restores" => "in the trash",
        "filtered" => "filtered by name",
        "completes" => "while what is typed can be completed",
        "labelling" => "while links are labelled",
        other => other,
    }
    .to_owned()
}

/// Where a binding's keys work, as the Keyboard page groups them: the
/// place its context names (by its label: the settings' several contexts
/// are one place), where that stands among the places, whether it is
/// inside a dialog or a menu, and what conditions it adds, in words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Where {
    pub place: String,
    /// Anywhere is 0, then `PLACES`' order; a context no place names
    /// (a plugin's own) after them.
    pub rank: usize,
    pub inside: bool,
    /// Empty for none.
    pub conditions: String,
}

/// Where the keys of a binding of `context` work.
pub fn place_of(context: &str) -> Where {
    let known = |atom: &str| PLACES.iter().position(|place| place.context == atom);
    let mut found = Where {
        place: "Anywhere".to_owned(),
        rank: 0,
        inside: false,
        conditions: String::new(),
    };
    let mut placed = false;
    let mut words: Vec<String> = Vec::new();
    for atom in conditions(context) {
        let alts: Vec<&str> = match atom.strip_prefix('(').and_then(|a| a.strip_suffix(')')) {
            Some(alts) => alts.split("||").map(str::trim).collect(),
            None => vec![atom],
        };
        let at: Vec<usize> = alts.iter().filter_map(|alt| known(alt)).collect();
        if atom == ROOT {
            continue;
        }
        if !placed && at.len() == alts.len() {
            // A place, or places that share their keys (the lists one
            // picks from), each named once.
            let mut labels: Vec<&str> = Vec::new();
            for &i in &at {
                if !labels.contains(&PLACES[i].label) {
                    labels.push(PLACES[i].label);
                }
            }
            found.place = labels.join(", ");
            found.rank = at.iter().min().map_or(0, |i| i + 1);
            found.inside = at.iter().all(|&i| PLACES[i].inside);
            placed = true;
            continue;
        }
        if !placed && known(atom).is_none() && atom.chars().next().is_some_and(char::is_uppercase) {
            // A plugin's own context: a place of its own, after skwir's.
            atom.clone_into(&mut found.place);
            found.rank = PLACES.len() + 1;
            placed = true;
            continue;
        }
        let said: Vec<String> = alts.iter().map(|alt| condition(alt)).collect();
        words.push(said.join(" or "));
    }
    found.conditions = words.join(", ");
    found
}

/// A GPUI context predicate (`Pane && single`) in plain words.
pub fn context_label(context: &str) -> String {
    let at = place_of(context);
    if at.conditions.is_empty() {
        at.place
    } else {
        format!("{} ({})", at.place, at.conditions)
    }
}

/// A binding's context as the `when` a record of the user's says it:
/// `Pane && single && !restores` is `pane single !restores`, anywhere is
/// `None`. The inverse of `context_of`.
pub fn when_of(context: &str) -> Option<String> {
    let token = |atom: &str| -> String {
        let (not, name) = match atom.strip_prefix('!') {
            Some(name) => ("!", name.trim()),
            None => ("", atom),
        };
        let name = PLACES
            .iter()
            .find(|place| place.context == name)
            .map_or(name, |place| place.token);
        format!("{not}{name}")
    };
    let tokens: Vec<String> = conditions(context)
        .into_iter()
        .filter(|atom| *atom != ROOT)
        .map(
            |atom| match atom.strip_prefix('(').and_then(|a| a.strip_suffix(')')) {
                Some(alts) => alts
                    .split("||")
                    .map(|alt| token(alt.trim()))
                    .collect::<Vec<_>>()
                    .join("|"),
                None => token(atom),
            },
        )
        .collect();
    (!tokens.is_empty()).then(|| tokens.join(" "))
}

/// What a binding does, in words (see `title`).
pub fn title_of(app: &App, hint: &KeyHint) -> String {
    title(app, &hint.command, &hint.args)
}

/// What a command run with `args` does, in words: the title of the form
/// its declaration gives these arguments, else its own with the arguments
/// said after it (`CommandSpec::title_for`, ADR-024); a readable form of
/// the name for a command nothing registers now. Only a title naming what
/// an argument names elsewhere is worked out here: a folder to go to, a
/// view.
pub fn title(app: &App, command: &str, args: &serde_json::Value) -> String {
    if let Some(title) = named_title(app, command, args) {
        return title;
    }
    if let Some(title) = app.commands.title(command, args) {
        return title;
    }
    let name = command
        .strip_prefix("ui.")
        .unwrap_or(command)
        .replace(['_', '.'], " ");
    let mut chars = name.chars();
    let title: String = chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    });
    skwir_core::command::with_args(&title, args)
}

/// A title naming what an argument names elsewhere: the folder a binding
/// goes to, the view it shows, the choice it asks for (Open with…).
fn named_title(app: &App, command: &str, args: &serde_json::Value) -> Option<String> {
    let text = |key| args.get(key).and_then(serde_json::Value::as_str);
    match command {
        "ui.choose" => text("title").map(|title| format!("{title}…")),
        "navigate" => {
            let location = text("location")?;
            let place = match location.trim_end_matches('/') {
                "~" => "your home folder",
                path => path.rsplit('/').next().filter(|name| !name.is_empty())?,
            };
            Some(format!("Go to {place}"))
        }
        "ui.view" => {
            let id = text("id")?;
            let title = app
                .registry
                .views
                .all()
                .into_iter()
                .find(|view| &*view.id == id)
                .map_or_else(|| id.to_owned(), |view| view.title.clone());
            Some(format!("{title} view"))
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
pub(crate) fn normal_keys(keys: &str) -> String {
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

/// Whether what was typed to find a shortcut finds this one (the Keyboard
/// page's filter, the Settings search, F1's): every word of `query` in its
/// `words` (its title, its place, its command); or, where `query` names
/// keys (a `+`, a modifier, a named key: `ctrl+t`, `Ctrl T`, `f2`,
/// `shift-delete`), one of its `bindings` (GPUI notation) holding every
/// key named, the other words in its `words`: `ctrl t` finds Ctrl+T and
/// Ctrl+Shift+T, `rename f2` Rename… on F2. A letter is a key there, never
/// a word. `query` is matched ignoring case.
pub fn finds(query: &str, words: &[&str], bindings: &[&str]) -> bool {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return true;
    }
    let said = words.join(" ").to_lowercase();
    if query.split_whitespace().all(|word| said.contains(word)) {
        return true;
    }
    let mut tokens: Vec<String> = Vec::new();
    for part in query
        .split(|c: char| c.is_whitespace() || c == '+' || c == '-')
        .filter(|part| !part.is_empty())
    {
        // `Page Down` in two words.
        if let ("up" | "down", Some("page")) = (part, tokens.last().map(String::as_str)) {
            tokens.pop();
            tokens.push(format!("page{part}"));
        } else {
            tokens.push(part.to_owned());
        }
    }
    let names_keys = query.contains('+')
        || tokens.iter().any(|token| {
            let key = key_word(token);
            KEY_MODIFIERS.contains(&key) || named_key(key)
        });
    names_keys
        && bindings.iter().any(|binding| {
            let parts = key_parts(binding);
            tokens.iter().all(|token| {
                parts.iter().any(|part| part == key_word(token))
                    // `ctrl t` is no search for Ctrl+Tab, "Next tab".
                    || (token.chars().count() > 1 && said.contains(token.as_str()))
            })
        })
}

/// The modifiers as a search names them, after `key_word`.
const KEY_MODIFIERS: [&str; 5] = ["ctrl", "alt", "shift", "super", "fn"];

/// A word of a search for keys, as GPUI names the key: `Control` is
/// `ctrl`, `Esc` is `escape`.
fn key_word(word: &str) -> &str {
    match word {
        "control" | "ctl" => "ctrl",
        "option" | "opt" => "alt",
        "cmd" | "command" | "win" | "meta" | "logo" | "platform" => "super",
        "esc" => "escape",
        "del" => "delete",
        "return" => "enter",
        "pgup" => "pageup",
        "pgdn" => "pagedown",
        "ins" => "insert",
        other => other,
    }
}

/// Whether `word` names a key alone, a search for it being one for keys.
fn named_key(word: &str) -> bool {
    word.strip_prefix('f')
        .is_some_and(|n| !n.is_empty() && n.parse::<u8>().is_ok())
        || [
            "escape",
            "enter",
            "delete",
            "backspace",
            "space",
            "tab",
            "home",
            "end",
            "pageup",
            "pagedown",
            "insert",
            "up",
            "down",
            "left",
            "right",
            "menu",
        ]
        .contains(&word)
}

/// A binding's modifiers and keys, every stroke's, as `key_word` names
/// them.
fn key_parts(binding: &str) -> Vec<String> {
    binding
        .split_whitespace()
        .flat_map(|stroke| match Keystroke::parse(stroke) {
            Ok(stroke) => {
                let held = stroke.modifiers;
                [
                    (held.control, "ctrl"),
                    (held.alt, "alt"),
                    (held.shift, "shift"),
                    (held.platform, "super"),
                    (held.function, "fn"),
                ]
                .into_iter()
                .filter(|(on, _)| *on)
                .map(|(_, name)| name.to_owned())
                .chain([stroke.key.to_lowercase()])
                .collect::<Vec<_>>()
            }
            Err(_) => vec![stroke.to_lowercase()],
        })
        .collect()
}

#[cfg(test)]
mod finds_tests {
    use super::finds;

    #[test]
    fn a_search_finds_by_words_or_by_the_keys_it_names() {
        let next_tab = (
            ["Next tab", "Anywhere", "tab.next"],
            ["ctrl-tab", "ctrl-pagedown"],
        );
        let reopen = (
            ["Reopen closed tab", "Anywhere", "tab.reopen"],
            ["ctrl-shift-t"],
        );
        let rename = (["Rename…", "File list", "ui.rename_prompt"], ["f2"]);
        let found = |query: &str, (words, keys): &([&str; 3], [&str; 2])| finds(query, words, keys);
        let one = |query: &str, (words, keys): &([&str; 3], [&str; 1])| finds(query, words, keys);
        // Words, every one, in any of its words, whatever their case.
        assert!(found("next TAB", &next_tab));
        assert!(found("tab.next", &next_tab));
        assert!(!found("next window", &next_tab));
        assert!(one("closed", &reopen));
        // Keys, as people write them.
        assert!(found("ctrl+tab", &next_tab));
        assert!(found("Ctrl Page Down", &next_tab));
        assert!(found("control pgdn", &next_tab));
        assert!(one("ctrl+shift+t", &reopen));
        assert!(one("ctrl t", &reopen), "the keys held among more");
        assert!(!found("ctrl t", &next_tab), "a letter is a key, not a word");
        assert!(one("F2", &rename));
        assert!(!one("f3", &rename));
        // Words and keys together.
        assert!(one("rename f2", &rename));
        assert!(!one("rename f3", &rename));
        assert!(found("next ctrl", &next_tab));
        let trash = (["Move to trash", "File list", "ui.trash"], ["delete"]);
        let erase = (
            ["Delete permanently", "File list", "ui.delete"],
            ["shift-delete"],
        );
        assert!(one("shift delete", &erase));
        assert!(!one("shift delete", &trash));
        assert!(one("Shift+Del", &erase));
        // A letter alone is a word, not a key: "q" is no search for Ctrl+Q.
        assert!(!finds(
            "q",
            &["Close window", "Anywhere", "window.close"],
            &["ctrl-q"]
        ));
        assert!(finds(
            "ctrl q",
            &["Close window", "Anywhere", "window.close"],
            &["ctrl-q"]
        ));
        assert!(finds("", &["anything"], &[]));
    }
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
    /// The id of the user's record it comes from.
    record: Option<String>,
}

/// A binding of the keymap, one a later binding hides too: what the
/// Keyboard page lists and edits (ADR-024).
#[derive(Clone, Debug, PartialEq)]
pub struct Bound {
    pub keys: String,
    /// What the keys run, and with what; `None` for a record of the user's
    /// that frees them.
    pub command: Option<(String, serde_json::Value)>,
    /// The GPUI context predicate it is active in.
    pub context: String,
    pub layer: Layer,
    /// The id of the user's record it comes from.
    pub record: Option<String>,
    /// The later binding that hides it, by its index: GPUI never runs it.
    pub hidden_by: Option<usize>,
}

/// The keymap as the shell knows it: what GPUI binds, what the palette,
/// menus and F1 show, and what it was built from.
pub struct Keymap {
    pub hints: Vec<KeyHint>,
    /// Every binding, those hidden too, in the order of their layers.
    pub bound: Vec<Bound>,
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
            record: None,
        })
    };
    let shell = BUILTIN
        .iter()
        .map(|(keys, command, when)| (*keys, *command, serde_json::json!({}), *when))
        .chain(
            BUILTIN_ARGS
                .iter()
                .chain(TERMINAL_KEYS)
                .map(|(keys, command, args, when)| {
                    (
                        *keys,
                        *command,
                        serde_json::from_str(args).expect("valid builtin args"),
                        *when,
                    )
                }),
        )
        .map(|(keys, command, args, when)| Spec {
            keys: keys.into(),
            command: Some((command.into(), args)),
            context: context_of(when),
            layer: Layer::Shell,
            record: None,
        })
        .chain(zoom_specs().map(|(keys, command, args, context)| Spec {
            keys,
            command: Some((command, args)),
            context,
            layer: Layer::Shell,
            record: None,
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
        record: Some(b.id.clone()),
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
        &*command.name != skwir_core::keymap::LIKE && !app.commands.contains(&command.name)
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
            record: spec.record.clone(),
        })
        .collect()
}

/// The conditions of a context predicate (`Pane && single`), each an
/// atom: a place, a flag, or a group of alternatives.
pub(crate) fn conditions(context: &str) -> Vec<&str> {
    context
        .split("&&")
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .collect()
}

/// Which spec hides each, if one does: GPUI takes the deepest place
/// first, then the latest binding, so a later binding of the same keys
/// that asks for no more than an earlier one (`Pane` over `Pane && empty`)
/// always wins where the earlier would apply. The last such binding is
/// named, the one that runs.
/// Each spec's keys are read once, and compared only with the specs of
/// the same keys: every spec against every later one, parsing both each
/// time, cost the first frame milliseconds.
fn shadowed(specs: &[Spec]) -> Vec<Option<usize>> {
    let mut by_keys: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, spec) in specs.iter().enumerate() {
        by_keys.entry(normal_keys(&spec.keys)).or_default().push(i);
    }
    let mut hidden = vec![None; specs.len()];
    for same in by_keys.values() {
        for (n, &i) in same.iter().enumerate() {
            let mine = conditions(&specs[i].context);
            hidden[i] = same[n + 1..].iter().rev().copied().find(|&later| {
                conditions(&specs[later].context)
                    .iter()
                    .all(|atom| mine.contains(atom))
            });
        }
    }
    hidden
}

/// Every binding of the keymap the registries and the user's records make,
/// in the order of their layers, each saying which hides it: what
/// `install` binds, and what the Keyboard page lists. Keys GPUI cannot
/// read are left out, with a warning.
pub fn bindings(app: &App, user: &[UserBinding]) -> Vec<Bound> {
    let specs: Vec<Spec> = specs(app, user)
        .into_iter()
        .filter(|spec| {
            let valid = valid_keys(&spec.keys);
            if !valid {
                tracing::warn!(
                    keys = spec.keys,
                    layer = ?spec.layer,
                    "skipping key binding with unparsable keys"
                );
            }
            valid
        })
        .collect();
    let hidden = shadowed(&specs);
    specs
        .into_iter()
        .zip(hidden)
        .map(|(spec, hidden_by)| Bound {
            keys: spec.keys,
            command: spec.command,
            context: spec.context,
            layer: spec.layer,
            record: spec.record,
            hidden_by,
        })
        .collect()
}

/// Whether `keys` type a character where a text field has the keyboard:
/// their first stroke is one character or Space, with Shift at most (`j`,
/// `G`, `.`, `space`, `g g`).
pub(crate) fn types_a_character(keys: &str) -> bool {
    let Some(first) = keys.split_whitespace().next() else {
        return false;
    };
    let Ok(stroke) = Keystroke::parse(first) else {
        return false;
    };
    let m = stroke.modifiers;
    (stroke.key.chars().count() == 1 || stroke.key == "space")
        && !(m.control || m.alt || m.platform || m.function)
}

/// The keys of bindings that a text field keeps for typing, wherever they
/// are bound: `install` binds each to nothing in the field's own context
/// (`input::KEY_CONTEXT`, the deepest place), so `.` or `j` bound anywhere
/// still types in the address bar, a search or a rename, as the terminal
/// keeps its keys (`TERMINAL_PASSTHROUGH`).
pub(crate) fn typed_in_fields(bound: &[Bound]) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    for binding in bound {
        if binding.command.is_some()
            && types_a_character(&binding.keys)
            && !keys.contains(&binding.keys)
        {
            keys.push(binding.keys.clone());
        }
    }
    keys
}

/// Whether a binding takes a character typed in the file list: its first
/// stroke is one character, with Shift at most (`j`, `G`, `/`, `g g`), and
/// it `binds` a command (freeing a key takes nothing).
pub(crate) fn takes_a_character(keys: &str, context: &str, binds: bool) -> bool {
    let place = conditions(context).first().copied();
    let Some(first) = keys.split_whitespace().next() else {
        return false;
    };
    let Ok(stroke) = Keystroke::parse(first) else {
        return false;
    };
    let m = stroke.modifiers;
    place == Some(PANE)
        && binds
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
    let bound = bindings(app, &user);
    let mut hints = Vec::new();
    let mut typeahead = true;
    for binding in &bound {
        let hidden = binding.hidden_by.is_some();
        if !hidden && takes_a_character(&binding.keys, &binding.context, binding.command.is_some())
        {
            typeahead = false;
        }
        if let Some((name, args)) = &binding.command
            && !hidden
        {
            hints.push(KeyHint {
                keys: binding.keys.clone(),
                command: name.clone(),
                args: args.clone(),
                context: binding.context.clone(),
                layer: binding.layer,
            });
        }
    }
    cx.clear_key_bindings();
    cx.bind_keys(crate::input::key_bindings());
    cx.bind_keys(gpui_bindings(&bound));
    Keymap {
        hints,
        bound,
        stamp: Stamp { revisions, user },
        typeahead,
    }
}

/// What GPUI binds for `bound`, beside the text input's own keys.
fn gpui_bindings(bound: &[Bound]) -> Vec<KeyBinding> {
    // The window's keys, bound to nothing in the terminal: the deeper
    // place wins, and the key goes on to the terminal's own listener.
    // First, so a key bound in the terminal itself (Ctrl+Shift+C) is the
    // later of its place and wins, whichever layer binds the same keys to
    // the window.
    let mut out: Vec<KeyBinding> = bound
        .iter()
        .filter(|binding| {
            binding.context == ROOT
                && binding
                    .command
                    .as_ref()
                    .is_some_and(|(name, _)| !TERMINAL_PASSTHROUGH.contains(&name.as_str()))
        })
        .map(|binding| KeyBinding::new(&binding.keys, gpui::NoAction, Some(TERMINAL)))
        .collect();
    for binding in bound {
        out.push(match binding.command.clone() {
            Some((name, args)) => KeyBinding::new(
                &binding.keys,
                RunCommand { name, args },
                Some(&binding.context),
            ),
            // Freed keys: nothing below this binding runs where it applies.
            None => KeyBinding::new(&binding.keys, gpui::NoAction, Some(&binding.context)),
        });
    }
    // After the bindings they shadow, so a sequence they start is no
    // longer awaited in a field either.
    for keys in typed_in_fields(bound) {
        out.push(KeyBinding::new(
            &keys,
            gpui::NoAction,
            Some(crate::input::KEY_CONTEXT),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{
        BUILTIN, BUILTIN_ARGS, KeyHint, Layer, Spec, context_label, context_of, display_keys,
        key_caps, keys_for_where, shadowed, specs, takes_a_character, typed_in_fields,
        types_a_character, valid_keys, zoom_specs,
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
            record: None,
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
            record: None,
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
            spec("escape", "up", Some("pane empty search"), Layer::Plugin),
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
            .filter(|(_, hidden)| hidden.is_some())
            .map(|(spec, _)| spec.command.as_ref().unwrap().0.as_str())
            .collect();
        // F2 for one item and for several both apply somewhere; Ctrl+F in
        // the list leaves Ctrl+F elsewhere; Escape while sweeping adds a
        // case, it hides nothing.
        assert_eq!(hidden, ["up", "ui.peek"]);
    }

    #[test]
    fn letters_bound_in_the_file_list_stop_type_ahead() {
        let takes = |keys: &str, when: Option<&str>| {
            let spec = spec(keys, "x", when, Layer::Scheme);
            takes_a_character(&spec.keys, &spec.context, spec.command.is_some())
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
        assert!(
            !takes_a_character(&freed.keys, &freed.context, freed.command.is_some()),
            "freeing a key binds nothing"
        );
    }

    /// A key that types a character, bound anywhere or in a place, is
    /// kept by a text field for typing: `install` binds it to nothing in
    /// the field's context. One with Ctrl or Alt is the binding's.
    #[test]
    fn text_fields_keep_the_characters_bound_elsewhere() {
        for typed in ["j", "G", "shift-g", ".", "space", "g g", "?"] {
            assert!(types_a_character(typed), "{typed}");
        }
        for not in [
            "ctrl-j",
            "alt-.",
            "f2",
            "escape",
            "enter",
            "ctrl-k j",
            "super-space",
        ] {
            assert!(!types_a_character(not), "{not}");
        }
        let tmp = tempfile::tempdir().unwrap();
        let app = skwir_core::App::new(skwir_core::app::AppConfig::ephemeral(tmp.path())).unwrap();
        crate::commands::declare_all(&app);
        let user = [
            UserBinding {
                id: "1".into(),
                keys: ".".into(),
                command: Some(Invocation::new("toggle_hidden")),
                when: None,
            },
            UserBinding {
                id: "2".into(),
                keys: "ctrl-.".into(),
                command: Some(Invocation::new("toggle_hidden")),
                when: None,
            },
            UserBinding {
                id: "3".into(),
                keys: "q".into(),
                command: None,
                when: Some("pane".into()),
            },
        ];
        let kept = typed_in_fields(&super::bindings(&app, &user));
        assert!(kept.contains(&".".to_owned()), "{kept:?}");
        assert!(!kept.contains(&"ctrl-.".to_owned()), "{kept:?}");
        assert!(!kept.contains(&"q".to_owned()), "freed, it runs nothing");
    }

    /// A key held down runs again only what steps (ADR-066): every key of
    /// the shell's that moves (an arrow, a page, Home, End) runs a command
    /// that repeats, but for the few that act, and no switch repeats, so a
    /// key left down neither flips one over and over nor stops a walk.
    #[test]
    fn held_keys_repeat_only_steps() {
        use serde_json::json;
        const MOVING: [&str; 8] = [
            "up", "down", "left", "right", "pageup", "pagedown", "home", "end",
        ];
        let tmp = tempfile::tempdir().unwrap();
        let app = skwir_core::App::new(skwir_core::app::AppConfig::ephemeral(tmp.path())).unwrap();
        crate::commands::declare_all(&app);
        // Moving keys that act once a press: a settings page entered,
        // another track, the address field's scopes listed or closed.
        let acts = |name: &str, args: &serde_json::Value| match name {
            "ui.settings_enter" | "ui.omnibox_scopes" => true,
            "ui.player" => matches!(args["request"].as_str(), Some("previous" | "next")),
            _ => false,
        };
        let mut moving = 0;
        for bound in super::bindings(&app, &[]) {
            let Some((name, args)) = &bound.command else {
                continue;
            };
            let last = bound.keys.rsplit(' ').next().unwrap_or_default();
            let key = last.rsplit('-').next().unwrap_or_default();
            if MOVING.contains(&key) && !acts(name, args) {
                moving += 1;
                assert!(
                    app.commands.repeats(name, args),
                    "{} runs {name} {args} once a press",
                    bound.keys
                );
            }
        }
        assert!(moving > 50, "{moving}");
        for (name, args) in [
            ("ui.peek", json!({})),
            ("ui.peek_close", json!({})),
            ("ui.preview_fullscreen", json!({})),
            ("ui.toggle_preview", json!({})),
            ("ui.toggle_sidebar", json!({})),
            ("ui.split", json!({})),
            ("ui.cursor_toggle", json!({})),
            ("ui.slideshow", json!({})),
            ("ui.media_control", json!({"action": "play"})),
            ("ui.media_control", json!({"action": "mute"})),
            ("ui.player", json!({"request": "play_pause"})),
            ("ui.sidebar_key", json!({"key": "rename"})),
            ("ui.sidebar_key", json!({"key": "open"})),
            ("ui.cursor_pick", json!({"advance": false})),
        ] {
            assert!(!app.commands.repeats(name, &args), "{name} {args} repeats");
        }
        // Picked, then the next: held, a run of files is picked.
        assert!(app.commands.repeats("ui.cursor_pick", &json!({})));
    }

    /// In the terminal its own keys run, even where a later layer binds the
    /// same keys to the window, and the window's other keys go on to the
    /// program but F4 and the palette: unbound, Ctrl+Shift+C reached the
    /// shell as Ctrl+C. Read as GPUI reads the bindings `install` makes.
    #[test]
    fn the_terminals_keys_are_its_own_there() {
        let tmp = tempfile::tempdir().unwrap();
        let app = skwir_core::App::new(skwir_core::app::AppConfig::ephemeral(tmp.path())).unwrap();
        crate::commands::declare_all(&app);
        app.registry.keymap.add(
            "terminal",
            KeyBinding {
                keys: "f4".into(),
                command: Invocation::new("ui.terminal"),
                when: None,
            },
        );
        let user = [UserBinding {
            id: "1".into(),
            keys: "ctrl-shift-c".into(),
            command: Some(Invocation::new("toggle_hidden")),
            when: None,
        }];
        let keymap = gpui::Keymap::new(super::gpui_bindings(&super::bindings(&app, &user)));
        let run = |keys: &str, places: &[&str]| -> Option<String> {
            let stack: Vec<gpui::KeyContext> = places
                .iter()
                .map(|place| gpui::KeyContext::parse(place).unwrap())
                .collect();
            let stroke = gpui::Keystroke::parse(keys).unwrap();
            let (found, _) = keymap.bindings_for_input(&[stroke], &stack);
            found.first().map(|binding| {
                let run = binding
                    .action()
                    .as_any()
                    .downcast_ref::<super::RunCommand>();
                run.expect("a command").name.clone()
            })
        };
        let terminal = [super::ROOT, super::TERMINAL];
        assert_eq!(
            run("ctrl-shift-c", &terminal).as_deref(),
            Some("ui.terminal_copy")
        );
        assert_eq!(
            run("ctrl-shift-v", &terminal).as_deref(),
            Some("ui.terminal_paste")
        );
        assert_eq!(
            run("ctrl-shift-c", &[super::ROOT]).as_deref(),
            Some("toggle_hidden")
        );
        assert_eq!(
            run("ctrl-h", &[super::ROOT]).as_deref(),
            Some("toggle_hidden")
        );
        assert_eq!(run("ctrl-h", &terminal), None, "the program's");
        assert_eq!(run("f4", &terminal).as_deref(), Some("ui.terminal"));
        assert_eq!(
            run("secondary-shift-p", &terminal).as_deref(),
            Some("ui.palette")
        );
        #[cfg(any(target_os = "linux", target_os = "freebsd"))]
        assert_eq!(
            run("shift-insert", &terminal).as_deref(),
            Some("ui.terminal_paste")
        );
    }

    /// The arrows go to what has the keyboard in a preview, deepest first,
    /// read as GPUI reads the bindings `install` makes: the pages, a
    /// picture, a page of reading scroll (`ui.preview_scroll`); an
    /// archive's files, a map, rows of text, a player and peek keep their
    /// own; on the whole screen, what is shown takes them before the
    /// screen's step, which only what holds no keyboard of its own leaves.
    #[test]
    fn the_arrows_go_to_what_has_the_keyboard_in_a_preview() {
        use super::{
            FULLSCREEN, IMAGE, INSPECTOR, MAP, MEDIA, PEEK, PREVIEW, READER, ROOT, SLIDESHOW,
        };
        let tmp = tempfile::tempdir().unwrap();
        let app = skwir_core::App::new(skwir_core::app::AppConfig::ephemeral(tmp.path())).unwrap();
        crate::commands::declare_all(&app);
        let keymap = gpui::Keymap::new(super::gpui_bindings(&super::bindings(&app, &[])));
        let run = |keys: &str, places: &[&str]| -> Option<(String, serde_json::Value)> {
            let stack: Vec<gpui::KeyContext> = places
                .iter()
                .map(|place| gpui::KeyContext::parse(place).unwrap())
                .collect();
            let stroke = gpui::Keystroke::parse(keys).unwrap();
            let (found, _) = keymap.bindings_for_input(&[stroke], &stack);
            found.first().map(|binding| {
                let run = binding
                    .action()
                    .as_any()
                    .downcast_ref::<super::RunCommand>()
                    .expect("a command");
                (run.name.clone(), run.args.clone())
            })
        };
        let scroll = |to: &str| {
            Some((
                "ui.preview_scroll".to_owned(),
                serde_json::json!({ "to": to }),
            ))
        };
        let name = |keys: &str, places: &[&str]| run(keys, places).map(|(name, _)| name);
        // The frame of what is shown, then what holds the keyboard in it.
        let reader = [ROOT, "Preview DocumentReader", READER];
        for (keys, to) in [
            ("up", "up"),
            ("down", "down"),
            ("left", "left"),
            ("right", "right"),
        ] {
            assert_eq!(run(keys, &reader), scroll(to), "{keys}");
            assert_eq!(run(keys, &[ROOT, IMAGE]), scroll(to), "{keys}");
            let reading = [ROOT, PREVIEW, INSPECTOR];
            assert_eq!(run(keys, &reading), scroll(to), "{keys}");
        }
        assert_eq!(
            name("pagedown", &reader).as_deref(),
            Some("ui.preview_page")
        );
        let reading = [ROOT, PREVIEW, INSPECTOR];
        assert_eq!(
            run("pagedown", &reading),
            Some((
                "ui.inspect".to_owned(),
                serde_json::json!({"action":"scroll","to":"page_down"})
            ))
        );
        // Rows of text go a row up or down; sideways is the preview's.
        let text = [ROOT, PREVIEW, "Inspector PreviewText"];
        assert_eq!(name("up", &text).as_deref(), Some("ui.inspect"));
        assert_eq!(run("left", &text), scroll("left"));
        let members = [ROOT, PREVIEW, "Inspector PreviewMembers"];
        for keys in ["up", "down", "left", "right"] {
            assert_eq!(name(keys, &members).as_deref(), Some("ui.inspect"));
            let map = [ROOT, PREVIEW, INSPECTOR, MAP];
            assert_eq!(name(keys, &map).as_deref(), Some("ui.map_pan"));
            let model = [ROOT, PREVIEW, INSPECTOR, super::MODEL];
            assert_eq!(name(keys, &model).as_deref(), Some("ui.model_turn"));
            let shifted = format!("shift-{keys}");
            assert_eq!(name(&shifted, &model).as_deref(), Some("ui.model_pan"));
            assert_eq!(name(keys, &[ROOT, PEEK]).as_deref(), Some("ui.cursor"));
        }
        assert_eq!(
            name("left", &[ROOT, MEDIA]).as_deref(),
            Some("ui.preview_seek")
        );
        // On the whole screen.
        let screen = format!("{FULLSCREEN} {SLIDESHOW}");
        let pages = [ROOT, FULLSCREEN, "Preview DocumentReader", READER];
        assert_eq!(run("right", &pages), scroll("right"));
        assert_eq!(run("right", &[ROOT, &screen, IMAGE]), scroll("right"));
        assert_eq!(
            name("right", &[ROOT, &screen]).as_deref(),
            Some("ui.fullscreen_step")
        );
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
            .chain(
                BUILTIN_ARGS
                    .iter()
                    .chain(super::TERMINAL_KEYS)
                    .map(|(keys, ..)| (*keys).to_owned()),
            )
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
        let tmp = tempfile::tempdir().unwrap();
        let app = skwir_core::App::new(skwir_core::app::AppConfig::ephemeral(tmp.path())).unwrap();
        let title = |args| super::named_title(&app, "navigate", &args);
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

    /// A record's `when` reads back from the context it made, so a key
    /// added beside another works where that one does.
    #[test]
    fn a_context_reads_back_as_its_when() {
        for when in [
            "pane single !restores",
            "pane selection|folder",
            "palette|jump|choose",
            "sidebar",
            "split",
        ] {
            assert_eq!(
                super::when_of(&context_of(Some(when))).as_deref(),
                Some(when)
            );
        }
        assert_eq!(super::when_of(&context_of(None)), None);
    }

    /// A binding's place, as the Keyboard page groups it: the settings'
    /// contexts are one place, the lists one picks from are their places
    /// named once, a plugin's own context a place after skwir's.
    #[test]
    fn a_context_is_a_place_with_conditions() {
        let anywhere = super::place_of(&context_of(None));
        assert_eq!((anywhere.place.as_str(), anywhere.rank), ("Anywhere", 0));
        let rename = super::place_of(&context_of(Some("pane single !restores")));
        assert_eq!(rename.place, "File list");
        assert_eq!(rename.conditions, "one item selected, not in the trash");
        assert!(!rename.inside);
        let settings = super::place_of(&context_of(Some("setting_control")));
        assert_eq!(settings.place, "Settings");
        assert!(settings.inside);
        let lists = super::place_of(&context_of(Some("palette|jump")));
        assert_eq!(lists.place, "Command palette, Jump");
        assert!(lists.inside);
        let own = super::place_of("UsageMap && selection");
        assert_eq!(own.place, "UsageMap");
        assert!(own.rank > rename.rank);
        assert_eq!(own.conditions, "with a selection");
    }

    #[test]
    fn when_tokens_map_to_contexts_and_flags() {
        assert_eq!(context_of(None), "Shell");
        assert_eq!(context_of(Some("pane")), "Pane");
        assert_eq!(context_of(Some("pane single")), "Pane && single");
        assert_eq!(context_of(Some("address")), "AddressBar");
        assert_eq!(context_of(Some("  ")), "Shell");
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
