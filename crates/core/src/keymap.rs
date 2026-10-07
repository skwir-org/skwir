//! The user's key bindings, the last layer of the keymap (ADR-024).
//!
//! Keys come in layers, lowest first: the plugins' defaults
//! (`Registry::keymap`), the shell's own, key schemes (`Registry::schemes`),
//! then these. A later layer wins where both apply. They are a setting, so
//! the settings dialog, the CLI and agents edit them the same way, and the
//! settings file holds them as the user wrote them.

use crate::command::Invocation;
use crate::settings::{RecordField, SettingSpec, Settings};

/// The settings namespace, shown as the Keyboard page.
pub const OWNER: &str = "keyboard";

/// A binding to this does what other keys (`{"keys": "delete"}`) do by
/// default, wherever they do it: a key scheme's `d` is Delete, which trashes,
/// takes out of a tag or the shelf, or deletes in the trash, as the place
/// says. The keymap copies those keys' bindings, conditions and all, so a
/// scheme never copies a condition that could drift.
pub const LIKE: &str = "keys.like";
pub const BINDINGS_KEY: &str = "keyboard.bindings";

/// One of the user's bindings. Without a command it unbinds the keys where
/// it applies: the key then does what it would with nothing bound there.
#[derive(Clone, Debug, PartialEq)]
pub struct UserBinding {
    pub keys: String,
    pub command: Option<Invocation>,
    pub when: Option<String>,
}

/// Declare the setting. `app` checks each binding's command when the
/// user saves it: a name no command has, or arguments it does not take,
/// are refused then, never at the press.
pub fn declare(settings: &Settings, app: std::sync::Weak<crate::App>) {
    settings.declare(
        SettingSpec::records(
            BINDINGS_KEY,
            "Your shortcuts",
            "Keys you bind, over the ones bibli and its plugins set. Leave the command empty to free a key.",
            "shortcut",
            vec![
                RecordField::text(
                    "keys",
                    "Keys",
                    "One key with its modifiers, or several one after another: ctrl-shift-n, g h, shift-g",
                    "ctrl-alt-n",
                )
                .required(),
                RecordField::text(
                    "command",
                    "Command",
                    "What the keys run, by name as the palette's commands are registered: mkdir, ui.jump, tab.new. Empty frees the keys",
                    "ui.jump",
                ),
                RecordField::text(
                    "args",
                    "Arguments",
                    "The command's arguments, as a JSON object",
                    "{\"location\": \"~\"}",
                ),
                RecordField::text(
                    "when",
                    "Where",
                    "Where the keys work: empty for anywhere, pane for the file list, with conditions such as selection, single, dir, file, folder",
                    "pane selection",
                ),
            ],
        )
        .validate_with(move |value| check(value, app.upgrade().as_deref())),
    );
}

/// The user's bindings, in the order they were written; records that do
/// not hold together are left out (the save refuses them, a hand-edited
/// file may still have them).
pub fn user_bindings(settings: &Settings) -> Vec<UserBinding> {
    let Some(toml::Value::Array(records)) = settings.get::<toml::Value>(BINDINGS_KEY) else {
        return Vec::new();
    };
    records.iter().filter_map(|r| binding(r).ok()).collect()
}

fn binding(record: &toml::Value) -> anyhow::Result<UserBinding> {
    let text = |key: &str| {
        record
            .get(key)
            .and_then(toml::Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
    };
    let keys = text("keys").ok_or_else(|| anyhow::anyhow!("a shortcut needs its keys"))?;
    valid_keys(keys)?;
    let args: serde_json::Value = match text("args") {
        None => serde_json::json!({}),
        Some(args) => serde_json::from_str(args)
            .map_err(|error| anyhow::anyhow!("the arguments of {keys} are not JSON: {error}"))?,
    };
    anyhow::ensure!(
        args.is_object(),
        "the arguments of {keys} are an object: {{\"name\": value}}"
    );
    Ok(UserBinding {
        keys: keys.split_whitespace().collect::<Vec<_>>().join(" "),
        command: text("command").map(|name| Invocation {
            name: name.into(),
            args,
        }),
        when: text("when").map(str::to_owned),
    })
}

fn check(value: &toml::Value, app: Option<&crate::App>) -> anyhow::Result<()> {
    let Some(records) = value.as_array() else {
        return Ok(());
    };
    records.iter().try_for_each(|r| {
        let binding = binding(r)?;
        let Some(command) = binding.command.filter(|c| &*c.name != LIKE) else {
            return Ok(());
        };
        let Some(app) = app else {
            return Ok(());
        };
        app.commands.check(&command).map_err(|error| match error {
            crate::command::CommandError::Unknown(name) => anyhow::anyhow!(
                "{} runs {name}, which no command is called (is its plugin on?)",
                binding.keys
            ),
            error @ (crate::command::CommandError::InvalidArgs { .. }
            | crate::command::CommandError::Panicked(_)) => {
                anyhow::anyhow!("{}: {error}", binding.keys)
            }
        })
    })
}

const MODIFIERS: [&str; 9] = [
    "ctrl",
    "alt",
    "shift",
    "fn",
    "secondary",
    "cmd",
    "super",
    "win",
    "platform",
];

/// Whether GPUI can bind `keys`: strokes separated by spaces, each
/// modifiers then a key joined by `-` (`ctrl-shift-n`, `ctrl--` for the
/// minus key, `G` for Shift+G), as `gpui::Keystroke::parse` reads them.
pub fn valid_keys(keys: &str) -> anyhow::Result<()> {
    let strokes: Vec<&str> = keys.split_whitespace().collect();
    anyhow::ensure!(!strokes.is_empty(), "a shortcut needs its keys");
    for stroke in strokes {
        let (mods, key) = match stroke.strip_suffix("--") {
            Some(mods) => (mods, "-"),
            None if stroke == "-" => ("", "-"),
            None => stroke.rsplit_once('-').unwrap_or(("", stroke)),
        };
        anyhow::ensure!(
            !key.is_empty(),
            "“{stroke}” names no key: write ctrl-- for Ctrl and the minus key"
        );
        if !mods.is_empty() {
            for modifier in mods.split('-') {
                anyhow::ensure!(
                    MODIFIERS.iter().any(|m| m.eq_ignore_ascii_case(modifier)),
                    "“{modifier}” in “{stroke}” is not a modifier: use ctrl, alt, shift or super"
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{BINDINGS_KEY, UserBinding, declare, user_bindings, valid_keys};
    use crate::settings::Settings;

    #[test]
    fn keys_are_read_as_gpui_reads_them() {
        for good in [
            "j",
            "G",
            "g g",
            "ctrl-shift-n",
            "ctrl--",
            "-",
            ", m",
            "shift-f10",
            "alt-.",
            "?",
            "super-space",
        ] {
            assert!(valid_keys(good).is_ok(), "{good}");
        }
        for bad in ["", "  ", "ctrl-", "hyper-x", "a-b"] {
            assert!(valid_keys(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn records_become_bindings_and_an_empty_command_frees_the_keys() {
        let settings = Settings::in_memory();
        declare(&settings, std::sync::Weak::new());
        let spec = settings.spec(BINDINGS_KEY).unwrap();
        let text = r#"[
            {"id": "1", "keys": "g  h", "command": "navigate", "args": "{\"location\": \"~\"}", "when": "pane"},
            {"id": "2", "keys": "ctrl-q"}
        ]"#;
        settings.set(BINDINGS_KEY, spec.parse(text).unwrap());
        let bindings = user_bindings(&settings);
        assert_eq!(bindings.len(), 2);
        assert_eq!(bindings[0].keys, "g h");
        let command = bindings[0].command.as_ref().unwrap();
        assert_eq!(&*command.name, "navigate");
        assert_eq!(command.args, serde_json::json!({ "location": "~" }));
        assert_eq!(bindings[0].when.as_deref(), Some("pane"));
        assert_eq!(
            bindings[1],
            UserBinding {
                keys: "ctrl-q".into(),
                command: None,
                when: None,
            }
        );
    }

    #[test]
    fn a_record_that_does_not_hold_together_is_refused_with_its_reason() {
        let settings = Settings::in_memory();
        declare(&settings, std::sync::Weak::new());
        let spec = settings.spec(BINDINGS_KEY).unwrap();
        let refused = |text: &str| spec.parse(text).unwrap_err().to_string();
        assert!(refused(r#"[{"id": "1", "keys": "hyper-x", "command": "x"}]"#).contains("hyper"));
        assert!(
            refused(r#"[{"id": "1", "keys": "x", "command": "x", "args": "[1]"}]"#)
                .contains("object")
        );
        assert!(
            refused(r#"[{"id": "1", "keys": "x", "command": "x", "args": "{"}]"#).contains("JSON")
        );
    }
}
