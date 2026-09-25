//! Translate the core keymap registry (plus UI defaults) into GPUI key bindings.
//! Every binding runs one action, [`RunCommand`], which the shell routes.

use bibli_core::App;
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
/// The pinned player's dock.
pub const PLAYER: &str = "Player";
pub const REVIEW: &str = "Review";
pub const JUMP: &str = "Jump";
/// `ui.choose`'s picker.
pub const CHOOSE: &str = "Choose";
pub const BULK: &str = "BulkRename";
pub const MENU: &str = "ContextMenu";

/// A binding as the palette shows it: keys and command name.
#[derive(Clone, Debug)]
pub struct KeyHint {
    pub keys: String,
    pub command: String,
    pub args: serde_json::Value,
    /// The GPUI context predicate the binding is active in.
    pub context: String,
}

/// Bindings the shell always needs, independent of plugins: every `ui.*`
/// command the shell owns has its key here, so disabling a plugin never
/// takes the palette or Quit with it. Keys for core and plugin commands are
/// the plugins' (ops-basic binds the file operations).
const BUILTIN: &[(&str, &str, Option<&str>)] = &[
    ("f2", "ui.rename_prompt", Some("pane single")),
    ("shift-delete", "ui.confirm_delete", Some("pane selection")),
    ("ctrl-shift-n", "ui.mkdir_prompt", None),
    ("ctrl-l", "ui.focus_address", None),
    ("ctrl-shift-p", "ui.palette", None),
    ("ctrl-q", "ui.quit", None),
    ("ctrl-h", "ui.toggle_hidden", None),
    ("tab", "ui.toolbar_next", None),
    ("shift-tab", "ui.toolbar_prev", None),
    ("enter", "ui.toolbar_activate", Some("ToolbarButton")),
    ("space", "ui.toolbar_activate", Some("ToolbarButton")),
    ("escape", "ui.toolbar_close", Some("ToolbarPopup")),
    ("escape", "ui.reader_find_close", Some("DocumentFind")),
    ("ctrl-,", "ui.settings", None),
    ("f1", "ui.shortcuts", None),
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
    ("f3", "ui.split", None),
    ("space", "ui.peek_close", Some("peek")),
    ("escape", "ui.peek_close", Some("peek")),
    ("left", "ui.peek_prev", Some("peek")),
    ("up", "ui.peek_prev", Some("peek")),
    ("right", "ui.peek_next", Some("peek")),
    ("down", "ui.peek_next", Some("peek")),
    ("ctrl-shift-d", "ui.split", None),
    ("tab", "ui.focus_other_pane", Some("pane split")),
    ("f5", "ui.copy_other", Some("pane split")),
    ("f6", "ui.move_other", Some("pane split")),
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
        "ctrl-=",
        "ui.preview_zoom",
        r#"{"dir":"in"}"#,
        Some("reader"),
    ),
    (
        "ctrl-+",
        "ui.preview_zoom",
        r#"{"dir":"in"}"#,
        Some("reader"),
    ),
    (
        "ctrl--",
        "ui.preview_zoom",
        r#"{"dir":"out"}"#,
        Some("reader"),
    ),
    (
        "ctrl-0",
        "ui.preview_zoom",
        r#"{"dir":"reset"}"#,
        Some("reader"),
    ),
    (
        "ctrl-=",
        "ui.preview_zoom",
        r#"{"dir":"in"}"#,
        Some("ImagePreview"),
    ),
    (
        "ctrl-+",
        "ui.preview_zoom",
        r#"{"dir":"in"}"#,
        Some("ImagePreview"),
    ),
    (
        "ctrl--",
        "ui.preview_zoom",
        r#"{"dir":"out"}"#,
        Some("ImagePreview"),
    ),
    (
        "ctrl-0",
        "ui.preview_zoom",
        r#"{"dir":"reset"}"#,
        Some("ImagePreview"),
    ),
    (
        "ctrl-=",
        "ui.inspect",
        r#"{"action":"larger"}"#,
        Some("Inspector"),
    ),
    (
        "ctrl-+",
        "ui.inspect",
        r#"{"action":"larger"}"#,
        Some("Inspector"),
    ),
    (
        "ctrl--",
        "ui.inspect",
        r#"{"action":"smaller"}"#,
        Some("Inspector"),
    ),
    (
        "ctrl-0",
        "ui.inspect",
        r#"{"action":"reset"}"#,
        Some("Inspector"),
    ),
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
    ("ctrl-1", "ui.view", r#"{"id":"list"}"#, None),
    ("ctrl-2", "ui.view", r#"{"id":"grid"}"#, None),
    ("ctrl-3", "ui.view", r#"{"id":"columns"}"#, None),
    ("ctrl-=", "ui.zoom", r#"{"dir":"in"}"#, None),
    ("ctrl-shift-=", "ui.zoom", r#"{"dir":"in"}"#, None),
    ("ctrl--", "ui.zoom", r#"{"dir":"out"}"#, None),
    ("ctrl-0", "ui.zoom", r#"{"dir":"reset"}"#, None),
];

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
            "DocumentFind" => place = "Document search",
            "InspectorFind" => place = "Preview search",
            "Review" => place = "Plan review",
            "Jump" => place = "Jump",
            "Choose" => place = "Choosing from a list",
            "BulkRename" => place = "Bulk rename",
            "MediaPlayer" => place = "Media player",
            "Player" => place = "Pinned player",
            "Inspector" => place = "Structured previews",
            "ToolbarButton" | "ToolbarPopup" => place = "Toolbars",
            "ContextMenu" => place = "Context menu",
            "Shortcuts" => place = "Shortcut overview",
            "Settings" | "SettingsNav" | "SettingControl" | "SettingNumber" => place = "Settings",
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

/// The keymap as the shell knows it: what GPUI binds, what the palette,
/// menus and F1 show, and the registry revision it was built from.
pub struct Keymap {
    pub hints: Vec<KeyHint>,
    pub revision: u64,
}

/// The keys bound to `command` with exactly `args`, as people read them.
/// The one lookup behind every shortcut the interface shows (menus, the
/// palette, toolbar tooltips, the status bar): matching the name alone would
/// show Zoom in's keys beside Zoom out.
pub fn keys_for(hints: &[KeyHint], command: &str, args: &serde_json::Value) -> Option<String> {
    let empty = serde_json::Value::Object(serde_json::Map::new());
    let args = if args.is_null() { &empty } else { args };
    hints
        .iter()
        .find(|hint| hint.command == command && hint.args == *args)
        .map(|hint| display_keys(&hint.keys))
}

/// Every binding: the plugins' from the registry, then the shell's own.
/// A binding the shell and a plugin both declare is listed once.
fn specs(app: &App) -> Vec<(String, String, serde_json::Value, String)> {
    let mut specs: Vec<(String, String, serde_json::Value, String)> = app
        .registry
        .keymap
        .all()
        .into_iter()
        .map(|b| {
            (
                b.keys.clone(),
                b.command.name.to_string(),
                b.command.args.clone(),
                context_of(b.when.as_deref()),
            )
        })
        .collect();
    for (keys, command, when) in BUILTIN {
        specs.push((
            (*keys).into(),
            (*command).into(),
            serde_json::json!({}),
            context_of(*when),
        ));
    }
    for (keys, command, args, when) in BUILTIN_ARGS {
        specs.push((
            (*keys).into(),
            (*command).into(),
            serde_json::from_str(args).expect("valid builtin args"),
            context_of(*when),
        ));
    }
    let mut seen = std::collections::HashSet::new();
    specs.retain(|spec| seen.insert(spec.clone()));
    specs
}

/// Replace every binding in GPUI with the registry's current keymap plus
/// the shell's and the text input's own. Called at startup and again
/// whenever the registry's keymap revision moves (a plugin enabled or
/// disabled), so a plugin's keys come and go with it.
pub fn install(app: &App, cx: &mut gpui::App) -> Keymap {
    // Read before the bindings: a change in between is picked up next time.
    let revision = app.registry.keymap.revision();
    let mut hints = Vec::new();
    let mut bindings = Vec::new();
    for (keys, name, args, context) in specs(app) {
        if !valid_keys(&keys) {
            tracing::warn!(keys, name, "skipping key binding with unparsable keys");
            continue;
        }
        hints.push(KeyHint {
            keys: keys.clone(),
            command: name.clone(),
            args: args.clone(),
            context: context.clone(),
        });
        bindings.push(KeyBinding::new(
            &keys,
            RunCommand { name, args },
            Some(&context),
        ));
    }
    cx.clear_key_bindings();
    cx.bind_keys(crate::input::key_bindings());
    cx.bind_keys(bindings);
    Keymap { hints, revision }
}

#[cfg(test)]
mod tests {
    use super::{context_label, context_of, display_keys, key_caps};

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
