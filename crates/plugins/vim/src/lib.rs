//! yazi's keys in the file list (ADR-024): a key scheme, off by default.
//!
//! The bindings are yazi's default keymap (`yazi-config/preset/
//! keymap-default.toml`, its `[mgr]` table), in its order, each running the
//! command bibli has for it; they go to `Registry::schemes`, over the
//! shell's own keys and every plugin's. Moves follow the cursor
//! (`ui.cursor` with `step: follow`), so what Space picks stays picked while
//! the cursor moves, and `v` holds a range (`select_sweep`). What yazi binds
//! and bibli has not (line modes, links, tasks, the trash as a place) is
//! left out; so are Left and Right, which keep each view's meaning (a grid
//! moves sideways), and Tab, which moves the keyboard to the toolbar or the
//! other pane.

use std::path::Path;

use bibli_core::app::App;
use bibli_core::cancel::CancelToken;
use bibli_core::command::Invocation;
use bibli_core::keymap::LIKE;
use bibli_core::os_path::PathArg;
use bibli_core::plugin::{Category, Plugin};
use bibli_core::registry::KeyBinding;
use bibli_core::scheduler::Priority;
use parking_lot::Mutex;

const ID: &str = "vim";

/// `(keys, command, arguments, where)`. A later binding of the same keys
/// wins where both apply, so the narrower one comes second (Escape in a
/// sweep after Escape).
const KEYS: &[(&str, &str, &str, &str)] = &[
    // Escape: out of a sweep, else back to the item under the cursor, as
    // yazi's "exit visual mode, clear selection".
    (
        "escape",
        "ui.cursor",
        r#"{"to":"here","step":"select"}"#,
        "pane",
    ),
    (
        "ctrl-[",
        "ui.cursor",
        r#"{"to":"here","step":"select"}"#,
        "pane",
    ),
    ("escape", "select_sweep_end", "{}", "pane sweep"),
    ("ctrl-[", "select_sweep_end", "{}", "pane sweep"),
    ("q", "ui.quit", "{}", "pane"),
    ("ctrl-c", "tab.close", "{}", "pane"),
    // Hop around.
    ("k", "ui.cursor", r#"{"to":"up","step":"follow"}"#, "pane"),
    ("j", "ui.cursor", r#"{"to":"down","step":"follow"}"#, "pane"),
    ("up", "ui.cursor", r#"{"to":"up","step":"follow"}"#, "pane"),
    (
        "down",
        "ui.cursor",
        r#"{"to":"down","step":"follow"}"#,
        "pane",
    ),
    (
        "g g",
        "ui.cursor",
        r#"{"to":"home","step":"follow"}"#,
        "pane",
    ),
    ("G", "ui.cursor", r#"{"to":"end","step":"follow"}"#, "pane"),
    (
        "home",
        "ui.cursor",
        r#"{"to":"home","step":"follow"}"#,
        "pane",
    ),
    (
        "end",
        "ui.cursor",
        r#"{"to":"end","step":"follow"}"#,
        "pane",
    ),
    (
        "ctrl-u",
        "ui.cursor",
        r#"{"to":"half_page_up","step":"follow"}"#,
        "pane",
    ),
    (
        "ctrl-d",
        "ui.cursor",
        r#"{"to":"half_page_down","step":"follow"}"#,
        "pane",
    ),
    (
        "ctrl-b",
        "ui.cursor",
        r#"{"to":"page_up","step":"follow"}"#,
        "pane",
    ),
    (
        "ctrl-f",
        "ui.cursor",
        r#"{"to":"page_down","step":"follow"}"#,
        "pane",
    ),
    (
        "shift-pageup",
        "ui.cursor",
        r#"{"to":"half_page_up","step":"follow"}"#,
        "pane",
    ),
    (
        "shift-pagedown",
        "ui.cursor",
        r#"{"to":"half_page_down","step":"follow"}"#,
        "pane",
    ),
    (
        "pageup",
        "ui.cursor",
        r#"{"to":"page_up","step":"follow"}"#,
        "pane",
    ),
    (
        "pagedown",
        "ui.cursor",
        r#"{"to":"page_down","step":"follow"}"#,
        "pane",
    ),
    // Navigation: yazi's `enter` goes into a folder only.
    ("h", "up", "{}", "pane"),
    ("l", "open", "{}", "pane single dir"),
    ("H", "back", "{}", "pane"),
    ("L", "forward", "{}", "pane"),
    // Picking.
    ("space", "ui.cursor_pick", "{}", "pane"),
    ("ctrl-a", "select_all", "{}", "pane"),
    ("ctrl-r", "select_invert", "{}", "pane"),
    // Visual mode: a range held while the cursor moves; the same key again
    // ends it, as in vim.
    ("v", "select_sweep", "{}", "pane"),
    ("V", "select_sweep", r#"{"unset":true}"#, "pane"),
    ("v", "select_sweep_end", "{}", "pane sweep"),
    ("V", "select_sweep_end", "{}", "pane sweep"),
    // Seeking in the preview.
    ("K", "ui.preview_page", r#"{"delta":-1}"#, "pane"),
    ("J", "ui.preview_page", r#"{"delta":1}"#, "pane"),
    // Operations: the standard keys' gestures (`keys.like`), so each does
    // what its key does where the place changes it (in a tag `d` takes the
    // tag off, in the trash it deletes for good).
    ("o", "open", "{}", "pane selection"),
    ("y", LIKE, r#"{"keys":"ctrl-c"}"#, "pane"),
    ("x", LIKE, r#"{"keys":"ctrl-x"}"#, "pane"),
    ("p", LIKE, r#"{"keys":"ctrl-v"}"#, "pane"),
    ("Y", "clipboard_clear", "{}", "pane"),
    ("X", "clipboard_clear", "{}", "pane"),
    ("d", LIKE, r#"{"keys":"delete"}"#, "pane"),
    ("D", LIKE, r#"{"keys":"shift-delete"}"#, "pane"),
    // A name ending in `/` is a folder.
    ("a", "ui.new_file_prompt", "{}", "pane folder"),
    // Several renamed at once, one on its own: F2.
    ("r", LIKE, r#"{"keys":"f2"}"#, "pane"),
    (".", "ui.toggle_hidden", "{}", "pane"),
    ("s", "ui.search", "{}", "pane"),
    ("S", "ui.search", r#"{"text":"content:"}"#, "pane"),
    ("z", "ui.jump", "{}", "pane"),
    ("Z", "ui.jump", "{}", "pane"),
    // Copying paths.
    ("c c", "shell.copy_paths", "{}", "pane selection"),
    // Filtering is what bibli's search in a folder does.
    ("f", "ui.search", "{}", "pane"),
    // Finding a name.
    ("/", "ui.find", "{}", "pane"),
    ("?", "ui.find", r#"{"backward":true}"#, "pane"),
    ("n", "ui.find_next", r#"{"delta":1}"#, "pane"),
    ("N", "ui.find_next", r#"{"delta":-1}"#, "pane"),
    // Sorting.
    (
        ", m",
        "sort",
        r#"{"key":"modified","ascending":true}"#,
        "pane",
    ),
    (
        ", M",
        "sort",
        r#"{"key":"modified","ascending":false}"#,
        "pane",
    ),
    (", a", "sort", r#"{"key":"name","ascending":true}"#, "pane"),
    (", A", "sort", r#"{"key":"name","ascending":false}"#, "pane"),
    (", n", "sort", r#"{"key":"name","ascending":true}"#, "pane"),
    (", N", "sort", r#"{"key":"name","ascending":false}"#, "pane"),
    (", s", "sort", r#"{"key":"size","ascending":true}"#, "pane"),
    (", S", "sort", r#"{"key":"size","ascending":false}"#, "pane"),
    // Goto; `g c` and `g d` are the desktop's folders (`bindings`).
    ("g h", "navigate", r#"{"location":"~"}"#, "pane"),
    ("g space", "ui.focus_address", "{}", "pane"),
    // Tabs.
    ("t t", "tab.new", "{}", "pane"),
    ("1", "tab.select", r#"{"index":0}"#, "pane"),
    ("2", "tab.select", r#"{"index":1}"#, "pane"),
    ("3", "tab.select", r#"{"index":2}"#, "pane"),
    ("4", "tab.select", r#"{"index":3}"#, "pane"),
    ("5", "tab.select", r#"{"index":4}"#, "pane"),
    ("6", "tab.select", r#"{"index":5}"#, "pane"),
    ("7", "tab.select", r#"{"index":6}"#, "pane"),
    ("8", "tab.select", r#"{"index":7}"#, "pane"),
    ("9", "tab.select", r#"{"index":8}"#, "pane"),
    ("[", "tab.select", r#"{"delta":-1}"#, "pane"),
    ("]", "tab.select", r#"{"delta":1}"#, "pane"),
    ("{", "tab.move", r#"{"delta":-1}"#, "pane"),
    ("}", "tab.move", r#"{"delta":1}"#, "pane"),
    // Help.
    ("~", "ui.shortcuts", "{}", "pane"),
    // Not yazi's: vim's undo, since bibli can take an operation back.
    ("u", "undo", "{}", "pane"),
];

fn binding(keys: &str, command: &str, args: &serde_json::Value, when: &str) -> KeyBinding {
    KeyBinding {
        keys: keys.into(),
        command: Invocation::with_args(command, args),
        when: Some(when.into()),
    }
}

fn go_to(keys: &str, folder: &Path) -> KeyBinding {
    binding(
        keys,
        "navigate",
        &serde_json::json!({ "location": PathArg::from(folder.to_path_buf()) }),
        "pane",
    )
}

/// The scheme's bindings: [`KEYS`], then the desktop's folders. Until a
/// worker has read where the downloads are, `g d` goes where the XDG
/// specification puts them when nothing says otherwise.
fn bindings(downloads: Option<&Path>) -> Vec<KeyBinding> {
    let mut out: Vec<KeyBinding> = KEYS
        .iter()
        .map(|(keys, command, args, when)| {
            let args = serde_json::from_str(args).expect("valid arguments");
            binding(keys, command, &args, when)
        })
        .collect();
    if let Some(config) = bibli_platform::xdg::config_home() {
        out.push(go_to("g c", &config));
    }
    out.push(match downloads {
        Some(downloads) => go_to("g d", downloads),
        None => binding(
            "g d",
            "navigate",
            &serde_json::json!({ "location": "~/Downloads" }),
            "pane",
        ),
    });
    out
}

#[derive(Default)]
pub struct VimPlugin {
    /// The read of the desktop's folders, cancelled on disable.
    lookup: Mutex<Option<CancelToken>>,
}

impl Plugin for VimPlugin {
    fn id(&self) -> &'static str {
        ID
    }

    fn title(&self) -> &'static str {
        "Vim keys"
    }

    fn description(&self) -> &'static str {
        "The keys of yazi in the file list: hjkl to move, Space to pick, v to select a range, y x p to copy, cut and paste, / to find, g and , sequences. Letters stop jumping to names while it is on."
    }

    fn category(&self) -> Category {
        Category::Browsing
    }

    fn enabled_by_default(&self) -> bool {
        false
    }

    fn register(&self, app: &App) {
        app.registry.schemes.replace_owner(ID, bindings(None));
        // Where the downloads are is in a file of the user's: read it on a
        // worker, then publish the bindings again with `g d`.
        let cancel = app.lifetime().child();
        if let Some(previous) = self.lookup.lock().replace(cancel.clone()) {
            previous.cancel();
        }
        let weak = app.weak();
        app.scheduler.spawn(
            Priority::Background,
            "vim-user-dirs",
            cancel.clone(),
            move || {
                let downloads = bibli_platform::places::user_dirs()
                    .into_iter()
                    .find(|dir| dir.kind == "download")
                    .map(|dir| dir.path);
                let (Some(downloads), Some(app)) = (downloads, weak.upgrade()) else {
                    return;
                };
                if !cancel.is_cancelled()
                    && app
                        .registry
                        .schemes
                        .replace_owner(ID, bindings(Some(&downloads)))
                {
                    app.wake();
                }
            },
        );
    }

    fn on_disable(&self, _app: &App) {
        if let Some(lookup) = self.lookup.lock().take() {
            lookup.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{KEYS, bindings};

    /// Every key parses as the core reads keys, every argument is JSON, and
    /// no binding of the same keys in the same place is listed twice (the
    /// second would hide the first).
    #[test]
    fn the_scheme_is_well_formed() {
        let mut seen = std::collections::HashSet::new();
        for (keys, command, args, when) in KEYS {
            bibli_core::keymap::valid_keys(keys).unwrap();
            let args: serde_json::Value = serde_json::from_str(args).unwrap();
            assert!(args.is_object(), "{keys}");
            assert!(when.starts_with("pane"), "{keys}: only the file list");
            assert!(seen.insert((*keys, *when)), "{keys} twice in {when}");
            assert!(!command.is_empty());
        }
        let downloads = std::path::Path::new("/nowhere/Téléchargements");
        let all = bindings(Some(downloads));
        let gd = all.iter().find(|b| b.keys == "g d").expect("g d");
        assert_eq!(
            gd.command.args["location"],
            serde_json::json!("/nowhere/Téléchargements")
        );
    }
}
