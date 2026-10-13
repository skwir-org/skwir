//! The user's key bindings, the last layer of the keymap (ADR-024).
//!
//! Keys come in layers, lowest first: the plugins' defaults
//! (`Registry::keymap`), the shell's own, key schemes (`Registry::schemes`),
//! then these. A later layer wins where both apply. They are a setting, so
//! the settings dialog, the CLI and agents edit them the same way, and the
//! settings file holds them as the user wrote them.

use crate::command::Invocation;
use crate::settings::{Beside, RecordField, SettingSpec, Settings};

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
    /// The record's id, which the Keyboard page edits it by.
    pub id: String,
    pub keys: String,
    pub command: Option<Invocation>,
    pub when: Option<String>,
}

/// Declare the setting. `app` checks the command of each binding a save
/// adds or changes: a name no command has, or arguments it does not take,
/// are refused then, never at the press. A binding stored as it is passes:
/// its plugin may be off, and it waits for it (ADR-024). A file checked as
/// it stands (`settings.check`) has every binding judged: a command
/// nothing has is refused unless it is named under a namespace no command
/// here has (a plugin that is off, the window's own with no window).
pub fn declare(settings: &Settings, app: std::sync::Weak<crate::App>) {
    settings.declare(
        SettingSpec::records(
            BINDINGS_KEY,
            "Your shortcuts",
            "Keys you bind, over the ones skwir and its plugins set. Leave the command empty to free a key.",
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
        .validate_with(move |value, beside| check(value, beside, app.upgrade().as_deref())),
    );
}

/// The user's bindings, in the order they were written; records that do
/// not hold together are left out (the save refuses them, a hand-edited
/// file may still have them).
pub fn user_bindings(settings: &Settings) -> Vec<UserBinding> {
    settings
        .get::<toml::Value>(BINDINGS_KEY)
        .map(|value| bindings_in(&value))
        .unwrap_or_default()
}

/// The bindings a value of the setting holds, as `user_bindings` reads
/// them: what the Keyboard page reads of the records a change would leave.
pub fn bindings_in(value: &toml::Value) -> Vec<UserBinding> {
    let Some(records) = value.as_array() else {
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
        id: text("id").unwrap_or_default().to_owned(),
        keys: keys.split_whitespace().collect::<Vec<_>>().join(" "),
        command: text("command").map(|name| Invocation {
            name: name.into(),
            args,
        }),
        when: text("when").map(str::to_owned),
    })
}

fn check(value: &toml::Value, beside: Beside<'_>, app: Option<&crate::App>) -> anyhow::Result<()> {
    let Some(records) = value.as_array() else {
        return Ok(());
    };
    let kept: &[toml::Value] = match beside {
        Beside::Stored(stored) => stored
            .and_then(toml::Value::as_array)
            .map_or(&[], Vec::as_slice),
        Beside::File => &[],
    };
    // The namespaces of the commands registered now (`tags` of
    // `tags.toggle`), asked once and only for a file's unknown command.
    let mut namespaces: Option<std::collections::BTreeSet<String>> = None;
    records.iter().try_for_each(|r| {
        let binding = binding(r)?;
        // Kept as it was: a command nothing registers is a plugin's that
        // is off, and the binding waits for it.
        if kept.contains(r) {
            return Ok(());
        }
        let Some(command) = binding.command.filter(|c| &*c.name != LIKE) else {
            return Ok(());
        };
        let Some(app) = app else {
            return Ok(());
        };
        match app.commands.check(&command) {
            Ok(()) => Ok(()),
            Err(crate::command::CommandError::Unknown(name)) => {
                // A file as it stands: a name under a namespace nothing
                // here registers is a plugin's that is off (or the
                // window's, checked with no window), and waits for it; one
                // under a namespace that is here, or with none, is a typo.
                if beside == Beside::File
                    && let Some((namespace, _)) = name.split_once('.')
                {
                    let known = namespaces.get_or_insert_with(|| {
                        app.commands
                            .list()
                            .iter()
                            .filter_map(|spec| spec.name.split_once('.'))
                            .map(|(namespace, _)| namespace.to_owned())
                            .collect()
                    });
                    if !known.contains(namespace) {
                        return Ok(());
                    }
                }
                Err(anyhow::anyhow!(
                    "{} runs {name}, which no command is called (is its plugin on?)",
                    binding.keys
                ))
            }
            Err(
                error @ (crate::command::CommandError::InvalidArgs { .. }
                | crate::command::CommandError::Panicked(_)),
            ) => Err(anyhow::anyhow!("{}: {error}", binding.keys)),
        }
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
                id: "2".into(),
                keys: "ctrl-q".into(),
                command: None,
                when: None,
            }
        );
    }

    /// A shortcut kept for a plugin turned off never blocks a save: only
    /// a binding the save adds or changes must name a command that is
    /// there (ADR-024).
    #[test]
    fn a_shortcut_kept_for_a_plugin_that_is_off_waits_for_it() {
        let tmp = tempfile::tempdir().unwrap();
        let app = crate::App::new(crate::app::AppConfig::ephemeral(tmp.path())).unwrap();
        let spec = app.settings.spec(BINDINGS_KEY).unwrap();
        let kept = r#"[{"id": "1", "keys": "ctrl-m", "command": "gone.open"}]"#;
        let stored = toml::Value::Array(Vec::new());
        assert!(
            spec.parse_over(kept, Some(&stored))
                .unwrap_err()
                .to_string()
                .contains("gone.open"),
            "a new binding to nothing is refused"
        );
        let stored = spec.parse(r#"[{"id": "1", "keys": "ctrl-m"}]"#).unwrap();
        let kept_value = {
            let mut value = stored.clone();
            value.as_array_mut().unwrap()[0]
                .as_table_mut()
                .unwrap()
                .insert("command".into(), "gone.open".into());
            value
        };
        let both = r#"[
            {"id": "1", "keys": "ctrl-m", "command": "gone.open"},
            {"id": "2", "keys": "ctrl-j", "command": "settings.apply", "args": "{\"changes\": {}}"}
        ]"#;
        assert!(
            spec.parse_over(both, Some(&kept_value)).is_ok(),
            "the binding stored as it was passes beside a new one"
        );
        let changed = r#"[{"id": "1", "keys": "ctrl-n", "command": "gone.open"}]"#;
        assert!(
            spec.parse_over(changed, Some(&kept_value)).is_err(),
            "changed, it is checked again"
        );
        assert!(
            spec.check(&kept_value).is_ok(),
            "a file's binding under a namespace nothing here has waits for its plugin"
        );
    }

    /// A file checked as it stands (`settings.check`, Home Manager's
    /// build): a command nothing could take is reported, a typo under a
    /// namespace that is here or with none; one under a namespace nothing
    /// here registers is a plugin's that is off, and waits.
    #[test]
    fn a_file_s_shortcut_to_no_command_is_reported_unless_its_plugin_may_be_off() {
        let tmp = tempfile::tempdir().unwrap();
        let app = crate::App::new(crate::app::AppConfig::ephemeral(tmp.path())).unwrap();
        let spec = app.settings.spec(BINDINGS_KEY).unwrap();
        // As a file holds it: read, never typed in.
        let file = |command: &str| {
            let mut record = toml::Table::new();
            record.insert("id".into(), "1".into());
            record.insert("keys".into(), "ctrl-m".into());
            record.insert("command".into(), command.into());
            toml::Value::Array(vec![toml::Value::Table(record)])
        };
        for typo in ["toggle_hiden", "settings.aply"] {
            let said = spec.check(&file(typo)).unwrap_err().to_string();
            assert!(said.contains(typo), "{typo}: {said}");
        }
        assert!(spec.check(&file("toggle_hidden")).is_ok());
        assert!(
            spec.check(&file("gone.open")).is_ok(),
            "a plugin that is off"
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
