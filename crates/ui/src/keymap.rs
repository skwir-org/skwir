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
pub const REVIEW: &str = "Review";
pub const JUMP: &str = "Jump";
pub const BULK: &str = "BulkRename";

/// A binding as the palette shows it: keys and command name.
#[derive(Clone, Debug)]
pub struct KeyHint {
    pub keys: String,
    pub command: String,
}

/// Defaults used when no plugin contributed bindings (`keys, command, when`).
const DEFAULTS: &[(&str, &str, Option<&str>)] = &[
    ("enter", "open", Some("pane")),
    ("backspace", "up", Some("pane")),
    ("alt-up", "up", None),
    ("alt-left", "back", None),
    ("f2", "ui.rename_prompt", Some("pane single")),
    ("delete", "trash", Some("pane")),
    ("shift-delete", "ui.confirm_delete", Some("pane")),
    ("ctrl-z", "undo", None),
    ("ctrl-shift-z", "redo", None),
    ("ctrl-a", "select_all", Some("pane")),
    ("escape", "select_none", Some("pane")),
    ("f5", "refresh", None),
    ("ctrl-shift-n", "ui.mkdir_prompt", None),
    ("ctrl-l", "ui.focus_address", None),
    ("ctrl-shift-p", "ui.palette", None),
    ("ctrl-q", "ui.quit", None),
    ("ctrl-h", "ui.toggle_hidden", None),
];

/// Bindings the shell always needs, independent of plugins.
const BUILTIN: &[(&str, &str, Option<&str>)] = &[
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
    // produce `ctrl-shift-space`; F3 is the layout-independent fallback.
    ("f3", "ui.toggle_preview", None),
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
    ("escape", "ui.usage_close", Some("pane usage")),
    ("enter", "ui.review_apply", Some("review")),
    ("escape", "ui.review_cancel", Some("review")),
    ("up", "ui.jump_up", Some("jump")),
    ("down", "ui.jump_down", Some("jump")),
    ("tab", "ui.rename_bulk_next", Some("bulk")),
    ("shift-tab", "ui.rename_bulk_prev", Some("bulk")),
    ("ctrl-k", "ui.rename_bulk_case", Some("bulk")),
];

/// Builtin bindings that carry arguments (`keys, command, args json, when`).
const BUILTIN_ARGS: &[(&str, &str, &str, Option<&str>)] = &[
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
/// else (`selection`, `single`, `dir`, `file`, `none`) is a flag the pane
/// sets on its key context every frame. All tokens must hold.
pub fn context_of(when: Option<&str>) -> String {
    let Some(when) = when else {
        return ROOT.to_owned();
    };
    let parts: Vec<String> = when
        .split_whitespace()
        .map(|t| match t {
            "pane" => PANE.to_owned(),
            "address" => ADDRESS.to_owned(),
            "palette" => PALETTE.to_owned(),
            "confirm" => CONFIRM.to_owned(),
            "peek" => PEEK.to_owned(),
            "reader" => READER.to_owned(),
            "review" => REVIEW.to_owned(),
            "jump" => JUMP.to_owned(),
            "bulk" => BULK.to_owned(),
            other => other.to_owned(),
        })
        .collect();
    if parts.is_empty() {
        ROOT.to_owned()
    } else {
        parts.join(" && ")
    }
}

fn valid_keys(keys: &str) -> bool {
    !keys.is_empty() && keys.split_whitespace().all(|k| Keystroke::parse(k).is_ok())
}

/// Install every binding into GPUI. Returns the key hints for the palette.
pub fn install(app: &App, cx: &mut gpui::App) -> Vec<KeyHint> {
    let mut specs: Vec<(String, String, serde_json::Value, String)> = Vec::new();

    let registered = app.registry.keymap.all();
    if registered.is_empty() {
        tracing::info!("no plugin key bindings registered; using UI defaults");
        for (keys, command, when) in DEFAULTS {
            specs.push((
                (*keys).into(),
                (*command).into(),
                serde_json::json!({}),
                context_of(*when),
            ));
        }
    } else {
        for b in registered {
            specs.push((
                b.keys.clone(),
                b.command.name.to_string(),
                b.command.args.clone(),
                context_of(b.when.as_deref()),
            ));
        }
    }
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

    let mut hints = Vec::new();
    let mut bindings = Vec::new();
    for (keys, name, args, context) in specs {
        if !valid_keys(&keys) {
            tracing::warn!(keys, name, "skipping key binding with unparsable keys");
            continue;
        }
        hints.push(KeyHint {
            keys: keys.clone(),
            command: name.clone(),
        });
        bindings.push(KeyBinding::new(
            &keys,
            RunCommand { name, args },
            Some(&context),
        ));
    }
    cx.bind_keys(bindings);
    hints
}

#[cfg(test)]
mod tests {
    use super::context_of;

    #[test]
    fn when_tokens_map_to_contexts_and_flags() {
        assert_eq!(context_of(None), "Shell");
        assert_eq!(context_of(Some("pane")), "Pane");
        assert_eq!(context_of(Some("pane single")), "Pane && single");
        assert_eq!(context_of(Some("address")), "AddressBar");
        assert_eq!(context_of(Some("  ")), "Shell");
    }
}
