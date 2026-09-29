//! Translate the core keymap registry (plus UI defaults) into GPUI key bindings.
//! Every binding runs one action, [`RunCommand`], which the shell routes.

use bibli_core::App;
use bibli_core::keymap::UserBinding;
use gpui::{Action, KeyBinding, Keystroke};

/// The single action every key binding maps to.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = bibli, no_json)]
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
pub const IMAGE: &str = "ImagePreview";
/// A map preview, once clicked: arrows move it, the zoom keys zoom it.
pub const MAP: &str = "MapPreview";
/// The pinned player's dock.
pub const PLAYER: &str = "Player";
pub const REVIEW: &str = "Review";
pub const JUMP: &str = "Jump";
/// `ui.choose`'s picker.
pub const CHOOSE: &str = "Choose";
/// The terminal panel: it takes every key the window binds, but these.
pub const TERMINAL: &str = "Terminal";

/// What still reaches the window from the terminal: the key that closes
/// the panel, and the palette, from which everything else can be run. Every
/// other key bound anywhere in the window belongs to the shell and its
/// programs while the terminal has the keyboard (Tab completes, Ctrl+L
/// clears, F-keys drive a file manager). `install` shadows them.
const TERMINAL_PASSTHROUGH: [&str; 2] = ["ui.terminal", "ui.palette"];
pub const BULK: &str = "BulkRename";
/// The rows of a container kept beside the member previewed (`members.rs`).
pub const MEMBERS: &str = "PreviewMembers";
pub const MENU: &str = "ContextMenu";

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

/// Bindings the shell always needs, independent of plugins: every `ui.*`
/// command the shell owns has its key here, so disabling a plugin never
/// takes the palette or Quit with it. Keys for core and plugin commands are
/// the plugins' (ops-basic binds the file operations).
const BUILTIN: &[(&str, &str, Option<&str>)] = &[
    ("f2", "ui.rename_prompt", Some("pane single")),
    ("shift-delete", "ui.confirm_delete", Some("pane selection")),
    // Into the folder shown, like Ctrl+V: a search or the shelf has none.
    ("ctrl-shift-n", "ui.mkdir_prompt", Some("pane folder")),
    ("ctrl-l", "ui.focus_address", None),
    ("ctrl-shift-p", "ui.palette", None),
    ("ctrl-q", "ui.quit", None),
    ("ctrl-h", "ui.toggle_hidden", None),
    // Dolphin's key for it, beside GNOME's.
    ("alt-.", "ui.toggle_hidden", None),
    ("f9", "ui.toggle_sidebar", None),
    ("tab", "ui.toolbar_next", None),
    ("shift-tab", "ui.toolbar_prev", None),
    ("enter", "ui.toolbar_activate", Some("ToolbarButton")),
    ("space", "ui.toolbar_activate", Some("ToolbarButton")),
    ("escape", "ui.toolbar_close", Some("ToolbarPopup")),
    ("escape", "ui.reader_find_close", Some("DocumentFind")),
    ("ctrl-,", "ui.settings", None),
    ("f1", "ui.shortcuts", None),
    // GNOME's key for the shortcuts window. GPUI leaves out the Shift a
    // symbol took, so Ctrl+Shift+/ (or wherever `?` is) arrives as `ctrl-?`.
    ("ctrl-?", "ui.shortcuts", None),
    ("escape", "ui.shortcuts", Some("Shortcuts")),
    ("escape", "ui.menu_close", Some("ContextMenu")),
    ("up", "ui.menu_up", Some("ContextMenu")),
    ("down", "ui.menu_down", Some("ContextMenu")),
    ("home", "ui.menu_first", Some("ContextMenu")),
    ("end", "ui.menu_last", Some("ContextMenu")),
    ("right", "ui.menu_expand", Some("ContextMenu")),
    ("left", "ui.menu_collapse", Some("ContextMenu")),
    ("enter", "ui.menu_activate", Some("ContextMenu")),
    ("space", "ui.menu_activate", Some("ContextMenu")),
    ("escape", "ui.settings_close", Some("Settings")),
    ("tab", "ui.settings_next", Some("Settings")),
    ("shift-tab", "ui.settings_prev", Some("Settings")),
    ("ctrl-f", "ui.settings_search", Some("Settings")),
    ("enter", "ui.settings_enter", Some("SettingsNav")),
    ("right", "ui.settings_enter", Some("SettingsNav")),
    ("ctrl-c", "ui.reader_copy", Some("reader")),
    ("ctrl-a", "ui.reader_select_all", Some("reader")),
    ("ctrl-f", "ui.reader_find", Some("reader")),
    ("up", "ui.cursor_up", Some("pane")),
    ("down", "ui.cursor_down", Some("pane")),
    ("shift-up", "ui.cursor_extend_up", Some("pane")),
    ("shift-down", "ui.cursor_extend_down", Some("pane")),
    ("home", "ui.cursor_home", Some("pane")),
    ("end", "ui.cursor_end", Some("pane")),
    ("shift-home", "ui.cursor_extend_home", Some("pane")),
    ("shift-end", "ui.cursor_extend_end", Some("pane")),
    // Ctrl moves the cursor and leaves the selection; Ctrl+Space picks.
    ("ctrl-up", "ui.cursor_move_up", Some("pane")),
    ("ctrl-down", "ui.cursor_move_down", Some("pane")),
    ("ctrl-left", "ui.cursor_move_left", Some("pane")),
    ("ctrl-right", "ui.cursor_move_right", Some("pane")),
    ("ctrl-home", "ui.cursor_move_home", Some("pane")),
    ("ctrl-end", "ui.cursor_move_end", Some("pane")),
    ("ctrl-space", "ui.cursor_toggle", Some("pane")),
    // The keyboard's way to what a right-click opens.
    ("shift-f10", "ui.context_menu", Some("pane")),
    ("menu", "ui.context_menu", Some("pane")),
    // What the middle button does to a folder: a tab behind this one.
    ("ctrl-enter", "ui.open_in_tab", Some("pane selection")),
    ("pageup", "ui.page_up", Some("pane")),
    ("pagedown", "ui.page_down", Some("pane")),
    ("up", "ui.palette_up", Some("palette")),
    ("down", "ui.palette_down", Some("palette")),
    ("enter", "ui.accept", Some("confirm")),
    ("escape", "ui.dismiss", Some("confirm")),
    ("left", "ui.cursor_left", Some("pane")),
    ("right", "ui.cursor_right", Some("pane")),
    ("shift-left", "ui.cursor_extend_left", Some("pane")),
    ("shift-right", "ui.cursor_extend_right", Some("pane")),
    ("space", "ui.peek", Some("pane")),
    ("ctrl-shift-space", "ui.toggle_preview", None),
    // Layouts where shift+space types a character (fr: U+202F) never
    // produce `ctrl-shift-space`; F11 is the layout-independent fallback.
    // F3 splits and F11 shows the side panel, as in Dolphin; inside a
    // document F3 stays "next match", the deeper context wins.
    ("f11", "ui.toggle_preview", None),
    ("ctrl-i", "ui.preview_details", None),
    ("alt-enter", "ui.preview_details", None),
    ("f3", "ui.split", None),
    ("space", "ui.peek_close", Some("peek")),
    ("escape", "ui.peek_close", Some("peek")),
    ("left", "ui.peek_prev", Some("peek")),
    ("up", "ui.peek_prev", Some("peek")),
    ("right", "ui.peek_next", Some("peek")),
    ("down", "ui.peek_next", Some("peek")),
    ("ctrl-shift-d", "ui.split", None),
    ("tab", "ui.focus_other_pane", Some("pane split")),
    ("shift-tab", "ui.focus_other_pane", Some("pane split")),
    // F5 refreshes whatever the window shows; with Shift it sends the
    // selection across a split, as F6 moves it.
    ("shift-f5", "ui.copy_other", Some("pane split selection")),
    ("shift-f6", "ui.move_other", Some("pane split selection")),
    ("enter", "ui.review_apply", Some("review")),
    ("escape", "ui.review_cancel", Some("review")),
    ("up", "ui.jump_up", Some("jump")),
    ("down", "ui.jump_down", Some("jump")),
    ("up", "ui.choose_up", Some("choose")),
    ("down", "ui.choose_down", Some("choose")),
    ("alt-a", "ui.choose_option", Some("choose")),
    ("tab", "ui.rename_bulk_next", Some("bulk")),
    ("shift-tab", "ui.rename_bulk_prev", Some("bulk")),
    ("ctrl-k", "ui.rename_bulk_case", Some("bulk")),
];

/// Builtin bindings that carry arguments (`keys, command, args json, when`).
const BUILTIN_ARGS: &[(&str, &str, &str, Option<&str>)] = &[
    // The address field's dropdown: the keyboard stays in the field.
    ("up", "ui.omnibox_move", r#"{"delta":-1}"#, Some("address")),
    ("down", "ui.omnibox_move", r#"{"delta":1}"#, Some("address")),
    (
        "alt-enter",
        "ui.omnibox_submit",
        r#"{"everywhere":true}"#,
        Some("address"),
    ),
    // Shift+F3 splits the other way from F3, for this split only.
    ("shift-f3", "ui.split", r#"{"axis":"other"}"#, None),
    (
        "ctrl-c",
        "ui.inspect",
        r#"{"action":"copy"}"#,
        Some("Inspector"),
    ),
    (
        "ctrl-f",
        "ui.inspect",
        r#"{"action":"search"}"#,
        Some("Inspector"),
    ),
    (
        "space",
        "ui.media_control",
        r#"{"action":"play"}"#,
        Some("MediaPlayer"),
    ),
    (
        "left",
        "ui.preview_seek",
        r#"{"delta":-5}"#,
        Some("MediaPlayer"),
    ),
    (
        "right",
        "ui.preview_seek",
        r#"{"delta":5}"#,
        Some("MediaPlayer"),
    ),
    (
        "m",
        "ui.media_control",
        r#"{"action":"mute"}"#,
        Some("MediaPlayer"),
    ),
    (
        "shift-left",
        "ui.media_step",
        r#"{"delta":-1}"#,
        Some("MediaPlayer"),
    ),
    (
        "shift-right",
        "ui.media_step",
        r#"{"delta":1}"#,
        Some("MediaPlayer"),
    ),
    ("f", "ui.media_fullscreen", "{}", Some("MediaPlayer")),
    (
        "up",
        "ui.media_control",
        r#"{"action":"volume_by","value":0.05}"#,
        Some("MediaPlayer"),
    ),
    (
        "down",
        "ui.media_control",
        r#"{"action":"volume_by","value":-0.05}"#,
        Some("MediaPlayer"),
    ),
    // The dock steers the pinned player through the same requests the
    // desktop's media keys send.
    (
        "space",
        "ui.player",
        r#"{"request":"play_pause"}"#,
        Some("Player"),
    ),
    (
        "left",
        "ui.player",
        r#"{"request":{"seek_by":-5.0}}"#,
        Some("Player"),
    ),
    (
        "right",
        "ui.player",
        r#"{"request":{"seek_by":5.0}}"#,
        Some("Player"),
    ),
    (
        "shift-left",
        "ui.player",
        r#"{"request":"previous"}"#,
        Some("Player"),
    ),
    (
        "shift-right",
        "ui.player",
        r#"{"request":"next"}"#,
        Some("Player"),
    ),
    ("m", "ui.player_mute", "{}", Some("Player")),
    (
        "up",
        "ui.player_volume",
        r#"{"delta":0.05}"#,
        Some("Player"),
    ),
    (
        "down",
        "ui.player_volume",
        r#"{"delta":-0.05}"#,
        Some("Player"),
    ),
    ("c", "ui.media_controls", "{}", Some("MediaPlayer")),
    ("escape", "ui.media_escape", "{}", Some("MediaPlayer")),
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
    ("f3", "ui.reader_match", r#"{"delta":1}"#, Some("reader")),
    (
        "shift-f3",
        "ui.reader_match",
        r#"{"delta":-1}"#,
        Some("reader"),
    ),
    // The same keys in every preview: zoom what is looked at, size what is
    // read, step through sections or pages.
    (
        "f3",
        "ui.inspect",
        r#"{"action":"find_next"}"#,
        Some("Inspector"),
    ),
    (
        "f3",
        "ui.inspect",
        r#"{"action":"find_next"}"#,
        Some("InspectorFind"),
    ),
    // The rows kept beside a member step through the others.
    (
        "up",
        "ui.inspect",
        r#"{"action":"sibling_step","delta":-1}"#,
        Some("PreviewMembers"),
    ),
    (
        "down",
        "ui.inspect",
        r#"{"action":"sibling_step","delta":1}"#,
        Some("PreviewMembers"),
    ),
    // A map moves under the arrows, as it does under a drag.
    (
        "left",
        "ui.map_pan",
        r#"{"dx":-1,"dy":0}"#,
        Some("MapPreview"),
    ),
    (
        "right",
        "ui.map_pan",
        r#"{"dx":1,"dy":0}"#,
        Some("MapPreview"),
    ),
    (
        "up",
        "ui.map_pan",
        r#"{"dx":0,"dy":-1}"#,
        Some("MapPreview"),
    ),
    (
        "down",
        "ui.map_pan",
        r#"{"dx":0,"dy":1}"#,
        Some("MapPreview"),
    ),
    ("[", "ui.preview_page", r#"{"delta":-1}"#, Some("Inspector")),
    ("]", "ui.preview_page", r#"{"delta":1}"#, Some("Inspector")),
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
        Some("SettingsNav"),
    ),
    (
        "down",
        "ui.settings_page",
        r#"{"delta":1}"#,
        Some("SettingsNav"),
    ),
    // A record of a list moves with its buttons' focus, as lines move in
    // an editor.
    (
        "alt-up",
        "ui.settings_move",
        r#"{"delta":-1}"#,
        Some("SettingRecord"),
    ),
    (
        "alt-down",
        "ui.settings_move",
        r#"{"delta":1}"#,
        Some("SettingRecord"),
    ),
    (
        "left",
        "ui.settings_adjust",
        r#"{"delta":-1}"#,
        Some("SettingControl"),
    ),
    (
        "right",
        "ui.settings_adjust",
        r#"{"delta":1}"#,
        Some("SettingControl"),
    ),
    (
        "down",
        "ui.settings_adjust",
        r#"{"delta":-1}"#,
        Some("SettingNumber"),
    ),
    (
        "up",
        "ui.settings_adjust",
        r#"{"delta":1}"#,
        Some("SettingNumber"),
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
    ("ctrl-1", "ui.view", r#"{"id":"list"}"#, None),
    ("ctrl-2", "ui.view", r#"{"id":"grid"}"#, None),
    ("ctrl-3", "ui.view", r#"{"id":"columns"}"#, None),
];

/// The keys of zooming, the same wherever something zooms: `=` and `+`
/// (GPUI leaves out the Shift a symbol took, so Ctrl+Shift+= arrives as
/// `ctrl-+` on layouts where `+` is shifted), and the keypad's.
const ZOOM_KEYS: [(&str, &str); 6] = [
    ("ctrl-=", "in"),
    ("ctrl-+", "in"),
    ("ctrl-add", "in"),
    ("ctrl--", "out"),
    ("ctrl-subtract", "out"),
    ("ctrl-0", "reset"),
];

/// What zooms, where: the pane's view anywhere, what a preview shows where
/// it has the keyboard. The inspector's text is sized in steps of its own.
const ZOOMS: [(&str, &str, Option<&str>); 5] = [
    ("ui.zoom", "dir", None),
    ("ui.preview_zoom", "dir", Some("reader")),
    ("ui.preview_zoom", "dir", Some("ImagePreview")),
    ("ui.preview_zoom", "dir", Some("MapPreview")),
    ("ui.inspect", "action", Some("Inspector")),
];

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

/// Map a registry `when` string to a GPUI context predicate.
/// Tokens: `pane`, `address`, `palette`, `confirm` name a context; anything
/// else is a flag the pane sets on its key context (the tokens of
/// `menu::SELECTION_TOKENS`, `folder` among them, `split`, `usage`). All
/// tokens must hold.
pub fn context_of(when: Option<&str>) -> String {
    let Some(when) = when else {
        return ROOT.to_owned();
    };
    let one = |t: &str| match t {
        "pane" => PANE.to_owned(),
        "address" => ADDRESS.to_owned(),
        "palette" => PALETTE.to_owned(),
        "confirm" => CONFIRM.to_owned(),
        "peek" => PEEK.to_owned(),
        "reader" => READER.to_owned(),
        "review" => REVIEW.to_owned(),
        "jump" => JUMP.to_owned(),
        "choose" => CHOOSE.to_owned(),
        "bulk" => BULK.to_owned(),
        "terminal" => TERMINAL.to_owned(),
        other => crate::menu::canonical_token(other).to_owned(),
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

/// A flag of the pane's key context in plain words.
fn condition(flag: &str) -> String {
    match flag {
        "single" => "one item selected",
        "selection" => "with a selection",
        "empty" => "nothing selected",
        "split" => "split view",
        "dir" => "on a folder",
        "file" => "on a file",
        "folder" => "in a folder",
        "sweep" => "selecting as the cursor moves",
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
            "Pane" => place = "File list",
            "AddressBar" => place = "Address bar",
            "Palette" => place = "Command palette",
            "Confirm" => place = "Confirmation dialogs",
            "Peek" => place = "Peek",
            "DocumentReader" => place = "Document reader",
            "ImagePreview" => place = "Picture preview",
            "MapPreview" => place = "Map preview",
            "DocumentFind" => place = "Document search",
            "InspectorFind" => place = "Preview search",
            "Review" => place = "Plan review",
            "Jump" => place = "Jump",
            "Choose" => place = "Choosing from a list",
            "BulkRename" => place = "Bulk rename",
            "Terminal" => place = "Terminal",
            "MediaPlayer" => place = "Media player",
            "Player" => place = "Pinned player",
            "Inspector" => place = "Structured previews",
            "PreviewMembers" => place = "Files of an archive or a message",
            "ToolbarButton" | "ToolbarPopup" => place = "Toolbars",
            "ContextMenu" => place = "Context menu",
            "Shortcuts" => place = "Shortcut overview",
            "Settings" | "SettingsNav" | "SettingControl" | "SettingNumber" | "SettingRecord" => {
                place = "Settings";
            }
            other => conditions.push(condition(other)),
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
                    "ctrl" => "Ctrl".into(),
                    "shift" => "Shift".into(),
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

/// What a keymap was built from: the registries' revisions and the user's
/// bindings.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stamp {
    pub revisions: (u64, u64),
    pub user: Vec<UserBinding>,
}

impl Stamp {
    /// The registries' revisions now, for a cheap comparison per sync.
    pub fn revisions(app: &App) -> (u64, u64) {
        (
            app.registry.keymap.revision(),
            app.registry.schemes.revision(),
        )
    }
}

/// The keys bound to `command` with exactly `args`, as people read them.
/// The one lookup behind every shortcut the interface shows (menus, the
/// palette, toolbar tooltips, the status bar): matching the name alone would
/// show Zoom in's keys beside Zoom out. The highest layer's keys, the first
/// of a layer that binds several (Ctrl+H before Alt+.).
pub fn keys_for(hints: &[KeyHint], command: &str, args: &serde_json::Value) -> Option<String> {
    let empty = serde_json::Value::Object(serde_json::Map::new());
    let args = if args.is_null() { &empty } else { args };
    hints
        .iter()
        .filter(|hint| hint.command == command && hint.args == *args)
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
    let registered = |bindings: Vec<bibli_core::registry::KeyBinding>, layer| {
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
    let mine = user.iter().map(|b| Spec {
        keys: b.keys.clone(),
        command: b
            .command
            .as_ref()
            .map(|c| (c.name.to_string(), c.args.clone())),
        context: context_of(b.when.as_deref()),
        layer: Layer::User,
    });
    let specs: Vec<Spec> = registered(app.registry.keymap.all(), Layer::Plugin)
        .chain(shell)
        .chain(registered(app.registry.schemes.all(), Layer::Scheme))
        .chain(mine)
        .collect();
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
fn shadowed(specs: &[Spec]) -> Vec<bool> {
    (0..specs.len())
        .map(|i| {
            let mine = conditions(&specs[i].context);
            specs[i + 1..].iter().any(|later| {
                later.keys == specs[i].keys
                    && conditions(&later.context)
                        .iter()
                        .all(|atom| mine.contains(atom))
            })
        })
        .collect()
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
        BUILTIN, BUILTIN_ARGS, Layer, Spec, context_label, context_of, display_keys, key_caps,
        shadowed, specs, takes_a_character, valid_keys, zoom_specs,
    };
    use bibli_core::command::Invocation;
    use bibli_core::keymap::UserBinding;
    use bibli_core::registry::KeyBinding;

    fn spec(keys: &str, command: &str, when: Option<&str>, layer: Layer) -> Spec {
        Spec {
            keys: keys.into(),
            command: Some((command.into(), serde_json::json!({}))),
            context: context_of(when),
            layer,
        }
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
        let app = bibli_core::App::new(bibli_core::app::AppConfig::ephemeral(tmp.path())).unwrap();
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
                    zooms.iter().any(|(k, _, _, c)| k == keys && *c == context),
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
