//! bibli plugin: ops-basic. The default keymap and context menu, plus an
//! in-app clipboard (copy / cut / paste as plans).
//!
//! # The `when` mini-language
//!
//! Key bindings and menu items carry an optional `when` string of
//! space-separated tokens that must all hold:
//!
//! - `selection`: at least one entry is selected
//! - `single`: exactly one entry is selected
//! - `dir`: every selected entry is a directory
//! - `file`: every selected entry is a regular file
//! - `empty`: nothing is selected; for a menu, the one opened on the
//!   folder's background, about the folder itself
//! - `folder`: the pane shows a folder
//! - `place`: every selected entry is a place, no file (a tag's)
//! - `refs`, `drops`, `creates`: the location shown lists files kept
//!   elsewhere, takes files dropped or pasted, makes new places
//!   (`bibli_core::location::Verbs`)
//! - `!token`: the token does not hold
//!
//! Absent (`None`) means always. For key bindings the UI also filters by
//! focus context: `pane` (the file list), `address`, `palette`, or none
//! for global bindings; that context is carried in the same `when` string
//! as its first token.

use std::sync::Arc;

use bibli_core::app::{App, PaneArg};
use bibli_core::command::{Invocation, Outcome};
use bibli_core::op::{Change, Copy, EntryRef, Followed, Move, Operation, Plan};
use bibli_core::plugin::Plugin;
use bibli_core::registry::{KeyBinding, MenuItem};
use bibli_core::workspace::PaneId;
use parking_lot::Mutex;

const ID: &str = "ops-basic";

#[derive(Debug, Default)]
struct Clipboard {
    entries: Vec<EntryRef>,
    cut: bool,
    /// Where they were cut or copied: a paste into a place that takes
    /// files (a tag's) is told, so a cut from another tag moves them.
    from: Option<bibli_core::Location>,
    /// Places cut or copied (a tag's): pasted in a folder, copied there as
    /// their location says (`Verbs::export`); in a place, given to it.
    places: Vec<bibli_core::Location>,
}

/// The ops-basic plugin.
#[derive(Debug, Default)]
pub struct OpsBasicPlugin {
    clipboard: Arc<Mutex<Clipboard>>,
}

/// The selection's files, and its places (a tag's), what a copy or a cut
/// remembers.
fn take_selection(
    app: &App,
    pane: Option<PaneId>,
) -> anyhow::Result<(Vec<EntryRef>, Vec<bibli_core::Location>)> {
    let pane = app.pane_or_active(pane);
    let refs = app.selected_refs(pane);
    let places: Vec<bibli_core::Location> = {
        let ws = app.workspace.read();
        ws.pane(pane)
            .map(|p| {
                p.selection
                    .iter()
                    .filter_map(|id| p.location_of(id))
                    .collect()
            })
            .unwrap_or_default()
    };
    if refs.is_empty() && places.is_empty() {
        return Err(bibli_core::command::NothingSelected.into());
    }
    Ok((refs, places))
}

/// Let every view show what is waiting for a paste.
fn publish(app: &App, entries: &[EntryRef], cut: bool) {
    app.pending
        .set(ID, entries.iter().map(|entry| entry.id), cut);
    app.wake();
}

/// Follow what the clipboard holds through applied changes. A cut entry
/// that was moved (by its paste or otherwise), trashed or deleted leaves
/// it; once none is left, nothing waits for a paste. So a paste that is
/// refused, fails or is cancelled keeps what was cut.
fn follow(clipboard: &Mutex<Clipboard>, changes: &[Change], app: &App) {
    let mut c = clipboard.lock();
    if c.entries.is_empty() {
        return;
    }
    let before = c.entries.len();
    let cut = c.cut;
    c.entries.retain_mut(|entry| {
        let mut kept = true;
        for change in changes {
            match change.follow(&entry.path) {
                Followed::Same => {}
                // A cut entry that moved went where it was going.
                Followed::Moved(_) if cut => kept = false,
                Followed::Moved(to) => entry.path = to,
                Followed::Gone => kept = false,
            }
            if !kept {
                break;
            }
        }
        kept
    });
    if c.entries.len() != before {
        if c.entries.is_empty() {
            *c = Clipboard::default();
            app.pending.clear();
            app.wake();
        } else {
            publish(app, &c.entries, cut);
        }
    }
}

fn paste(clipboard: &Mutex<Clipboard>, pane: Option<PaneId>, app: &App) -> anyhow::Result<Outcome> {
    let pane = app.pane_or_active(pane);
    let Ok(dest) = app.pane_dir(pane) else {
        return paste_on_place(clipboard, pane, app);
    };
    let (entries, cut, places) = {
        let c = clipboard.lock();
        if c.entries.is_empty() && c.places.is_empty() {
            anyhow::bail!("clipboard is empty");
        }
        (c.entries.clone(), c.cut, c.places.clone())
    };
    // Places (a tag's) are copied in as their location says: a folder of
    // their files.
    for place in places {
        if let Some(mut export) = app.verbs(&place).export {
            if !export.args.is_object() {
                export.args = serde_json::json!({});
            }
            export.args["dest"] =
                serde_json::json!(bibli_core::os_path::PathArg::from(dest.as_path()));
            app.dispatch(&export)?;
        }
    }
    if entries.is_empty() {
        return Ok(Outcome::Done);
    }
    let steps: Vec<Box<dyn Operation>> = entries
        .into_iter()
        .map(|entry| -> Box<dyn Operation> {
            if cut {
                Box::new(Move {
                    entry,
                    dest_dir: dest.clone(),
                })
            } else {
                Box::new(Copy::from_settings(entry, dest.clone(), &app.settings))
            }
        })
        .collect();
    let title = format!(
        "{} to {}",
        if cut { "move" } else { "copy" },
        dest.display()
    );
    Ok(Outcome::Plan(Plan::with_steps(title, steps)))
}

/// A paste into a location that is no folder but takes files (a tag's):
/// what its provider says a drop means there (`Verbs::drop`), with the
/// clipboard's files, whether they were cut, and where from. A cut is
/// done once pasted.
fn paste_on_place(
    clipboard: &Mutex<Clipboard>,
    pane: PaneId,
    app: &App,
) -> anyhow::Result<Outcome> {
    let shown = app
        .pane_location(pane)
        .ok_or_else(|| anyhow::anyhow!("the pane shows nothing"))?;
    let Some(drop) = app.verbs(&shown).drop else {
        anyhow::bail!("nothing can be pasted here");
    };
    let (entries, cut, from, places) = {
        let c = clipboard.lock();
        if c.entries.is_empty() && c.places.is_empty() {
            anyhow::bail!("clipboard is empty");
        }
        (c.entries.clone(), c.cut, c.from.clone(), c.places.clone())
    };
    let mut invocation = drop;
    if !invocation.args.is_object() {
        invocation.args = serde_json::json!({});
    }
    invocation.args["entries"] = serde_json::json!(entries);
    invocation.args["move"] = serde_json::json!(cut);
    invocation.args["places"] =
        serde_json::json!(places.iter().map(ToString::to_string).collect::<Vec<_>>());
    if let Some(from) = from {
        invocation.args["from"] = serde_json::json!(from.to_string());
    }
    let outcome = app.dispatch(&invocation)?;
    if cut {
        *clipboard.lock() = Clipboard::default();
        app.pending.clear();
        app.wake();
    }
    Ok(outcome)
}

// Keys for the plugin's own and core commands. Keys for the shell's `ui.*`
// commands live in the shell's builtin table, so they outlive this plugin.
const BINDINGS: &[(&str, &str, Option<&str>)] = &[
    ("enter", "open", Some("pane")),
    ("backspace", "up", Some("pane")),
    ("alt-up", "up", None),
    ("alt-left", "back", None),
    ("alt-right", "forward", None),
    // The keyboard's own Back and Forward keys (XF86Back, XF86Forward). The
    // mouse's side buttons act on the pane under the pointer (`Shell`).
    ("back", "back", None),
    ("forward", "forward", None),
    // Where the location takes files out without deleting them (a tag,
    // the shelf), Delete does that instead (`take_out`, the shell's).
    // Where files from many folders are listed, the folder one is in, as
    // Nautilus's Open Item Location.
    (
        "ctrl-alt-o",
        "show_in_folder",
        Some("pane single !folder !place !restores"),
    ),
    ("delete", "trash", Some("pane selection !refs !restores")),
    ("delete", "take_out", Some("pane selection refs")),
    // In the trash, Delete deletes for good: the shell asks first, as for
    // any plan Undo cannot take back.
    ("delete", "delete", Some("pane selection restores")),
    // Deleting for good from anywhere: the shell asks first, as for any
    // plan Undo cannot take back (`Shell::apply_user_plan`).
    ("shift-delete", "delete", Some("pane selection !place")),
    ("ctrl-c", "clipboard_copy", Some("pane selection")),
    ("ctrl-x", "clipboard_cut", Some("pane selection !restores")),
    // Into the folder shown, or a place that takes files (a tag gives
    // them itself): a search has neither.
    ("ctrl-v", "clipboard_paste", Some("pane folder|drops")),
    ("ctrl-z", "undo", None),
    ("ctrl-shift-z", "redo", None),
    ("ctrl-y", "redo", None),
    ("ctrl-a", "select_all", Some("pane")),
    ("ctrl-shift-i", "select_invert", Some("pane")),
    ("escape", "select_none", Some("pane")),
    // With nothing selected Escape has one thing left to cancel: a pending
    // cut or copy. Later bindings win, and `empty` holds only then.
    ("escape", "clipboard_clear", Some("pane empty")),
    ("f5", "refresh", None),
    ("ctrl-r", "refresh", None),
    // The folder or search being looked at, as in Nautilus and browsers.
    ("ctrl-d", "keep", None),
    // Dolphin's key for it: Ctrl+T opens a tab, as everywhere.
    ("shift-f4", "open_terminal", None),
    ("ctrl-t", "tab.new", None),
    ("ctrl-w", "tab.close", None),
    ("ctrl-shift-t", "tab.reopen", None),
];

// Keys for core commands that take arguments: moving between tabs, and
// home.
const BINDINGS_ARGS: &[(&str, &str, &str)] = &[
    ("ctrl-tab", "tab.select", r#"{"delta":1}"#),
    ("ctrl-shift-tab", "tab.select", r#"{"delta":-1}"#),
    ("ctrl-pagedown", "tab.select", r#"{"delta":1}"#),
    ("ctrl-pageup", "tab.select", r#"{"delta":-1}"#),
    ("ctrl-shift-pagedown", "tab.move", r#"{"delta":1}"#),
    ("ctrl-shift-pageup", "tab.move", r#"{"delta":-1}"#),
    ("alt-1", "tab.select", r#"{"index":0}"#),
    ("alt-2", "tab.select", r#"{"index":1}"#),
    ("alt-3", "tab.select", r#"{"index":2}"#),
    ("alt-4", "tab.select", r#"{"index":3}"#),
    ("alt-5", "tab.select", r#"{"index":4}"#),
    ("alt-6", "tab.select", r#"{"index":5}"#),
    ("alt-7", "tab.select", r#"{"index":6}"#),
    ("alt-8", "tab.select", r#"{"index":7}"#),
    // The last tab, however many there are, as in browsers.
    ("alt-9", "tab.select", r#"{"index":-1}"#),
    ("alt-home", "navigate", r#"{"location":"~"}"#),
];

// Context menu items: (title, command, when, order). Order follows the
// convention documented on `bibli_core::registry::MenuItem`: hundreds are
// groups (0 open, 100 edit, 200 create, 300 destructive, 400 misc), the UI
// draws a separator between groups. "Open" and "Open Terminal Here" are
// contributed by the open-with plugin, which owns those commands.
const MENU: &[(&str, &str, Option<&str>, i32, &str)] = &[
    ("Open in New Tab", "tab.open", Some("dir"), 2, "tab-new"),
    (
        "Show in Folder",
        "show_in_folder",
        Some("single !folder !place !restores"),
        3,
        "folder",
    ),
    // Not a file of the trash, which goes elsewhere only restored.
    (
        "Rename",
        "ui.rename_prompt",
        Some("single !restores"),
        100,
        "rename",
    ),
    // A place too: a tag copied into a folder is a folder of its files,
    // cut and pasted in a tag it goes inside.
    (
        "Cut",
        "clipboard_cut",
        Some("selection !restores"),
        110,
        "cut",
    ),
    ("Copy", "clipboard_copy", Some("selection"), 120, "copy"),
    (
        "Paste",
        "clipboard_paste",
        Some("folder|drops"),
        130,
        "paste",
    ),
    // The folder's own menu, on its background.
    (
        "New Folder",
        "ui.mkdir_prompt",
        Some("empty folder"),
        200,
        "folder-plus",
    ),
    (
        "New File",
        "ui.new_file_prompt",
        Some("empty folder"),
        201,
        "file-plus",
    ),
    // Files, never places (a tag's has no file to trash), nor what is in
    // the trash already.
    (
        "Move to Trash",
        "trash",
        Some("selection !place !restores"),
        300,
        "trash",
    ),
    (
        "Delete Permanently",
        "delete",
        Some("selection !place"),
        310,
        "close",
    ),
    ("Refresh", "refresh", Some("empty"), 400, "refresh"),
];

impl Plugin for OpsBasicPlugin {
    fn id(&self) -> &'static str {
        ID
    }

    fn title(&self) -> &'static str {
        "Basic operations"
    }

    fn description(&self) -> &'static str {
        "Default key bindings, context menu, and clipboard"
    }

    fn category(&self) -> bibli_core::plugin::Category {
        bibli_core::plugin::Category::Files
    }

    fn register(&self, app: &App) {
        let cb = Arc::clone(&self.clipboard);
        app.commands.register::<PaneArg, _>(
            ID,
            "clipboard_copy",
            "Copy",
            "Remember the selection for a later paste (copy)",
            move |a, app| {
                let (entries, places) = take_selection(app, a.pane)?;
                publish(app, &entries, false);
                *cb.lock() = Clipboard {
                    entries,
                    cut: false,
                    from: app.pane_location(app.pane_or_active(a.pane)),
                    places,
                };
                Ok(Outcome::Done)
            },
        );
        let cb = Arc::clone(&self.clipboard);
        app.commands.register::<PaneArg, _>(
            ID,
            "clipboard_cut",
            "Cut",
            "Remember the selection for a later paste (move)",
            move |a, app| {
                let (entries, places) = take_selection(app, a.pane)?;
                publish(app, &entries, true);
                *cb.lock() = Clipboard {
                    entries,
                    cut: true,
                    from: app.pane_location(app.pane_or_active(a.pane)),
                    places,
                };
                Ok(Outcome::Done)
            },
        );
        let cb = Arc::clone(&self.clipboard);
        app.commands.register::<PaneArg, _>(
            ID,
            "clipboard_clear",
            "Forget cut or copied files",
            "Empty the clipboard; files that were cut stay where they are",
            move |_, app| {
                *cb.lock() = Clipboard::default();
                app.pending.clear();
                app.wake();
                Ok(Outcome::Done)
            },
        );
        let cb = Arc::clone(&self.clipboard);
        app.commands.register::<PaneArg, _>(
            ID,
            "clipboard_paste",
            "Paste",
            "Copy or move the clipboard into the current directory (plan)",
            move |a, app| paste(&cb, a.pane, app),
        );
        let cb = Arc::clone(&self.clipboard);
        let weak = app.weak();
        app.executor.observe(
            ID,
            Arc::new(move |changes| {
                if let Some(app) = weak.upgrade() {
                    follow(&cb, changes, &app);
                }
            }),
        );

        for (keys, command, when) in BINDINGS {
            app.registry.keymap.add(
                ID,
                KeyBinding {
                    keys: (*keys).into(),
                    command: Invocation::new(*command),
                    when: when.map(str::to_owned),
                },
            );
        }
        for (keys, command, args) in BINDINGS_ARGS {
            let args: serde_json::Value = serde_json::from_str(args).expect("valid arguments");
            app.registry.keymap.add(
                ID,
                KeyBinding {
                    keys: (*keys).into(),
                    command: Invocation::with_args(*command, &args),
                    when: None,
                },
            );
        }
        for (title, command, when, order, icon) in MENU {
            app.registry.menus.add(
                ID,
                MenuItem {
                    menu: "context".into(),
                    title: (*title).into(),
                    command: Invocation::new(*command),
                    when: when.map(str::to_owned),
                    order: *order,
                    icon: Some((*icon).into()),
                    submenu: None,
                },
            );
        }
    }

    fn on_disable(&self, _app: &App) {
        *self.clipboard.lock() = Clipboard::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bibli_core::app::AppConfig;
    use bibli_core::location::Location;
    use std::time::Duration;

    #[test]
    fn registers_and_withdraws_bindings_menus_commands() {
        let tmp = tempfile::tempdir().unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(OpsBasicPlugin::default()), &app);
        assert!(
            app.registry
                .keymap
                .all()
                .iter()
                .any(|b| b.keys == "ctrl-z" && &*b.command.name == "undo")
        );
        assert!(
            app.registry
                .menus
                .all()
                .iter()
                .any(|m| m.title == "Rename" && m.when.as_deref() == Some("single !restores"))
        );
        assert!(app.commands.get("clipboard_paste").is_some());
        app.plugins.set_enabled(ID, false, &app);
        assert!(app.registry.keymap.all().is_empty());
        assert!(app.registry.menus.all().is_empty());
        assert!(app.commands.get("clipboard_paste").is_none());
    }

    #[test]
    fn copy_then_paste_yields_a_copy_plan_and_cut_clears() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        let dst = tmp.path().join("dst");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dst).unwrap();
        std::fs::write(src.join("a.txt"), b"a").unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(OpsBasicPlugin::default()), &app);
        let pane = app.workspace.read().active_id();
        assert!(
            app.dispatch(&Invocation::new("clipboard_paste")).is_err(),
            "empty clipboard errors"
        );

        app.navigate(pane, &Location::from_path(&src)).unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
        app.dispatch(&Invocation::with_args(
            "select",
            &serde_json::json!({ "names": ["a.txt"] }),
        ))
        .unwrap();
        app.dispatch(&Invocation::new("clipboard_cut")).unwrap();
        let pending = app.pending.get();
        assert!(
            pending.cut && pending.entries.len() == 1,
            "views can see what was cut"
        );
        app.navigate(pane, &Location::from_path(&dst)).unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
        let Outcome::Plan(plan) = app.dispatch(&Invocation::new("clipboard_paste")).unwrap() else {
            panic!("expected plan")
        };
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].kind(), "move");
        let r = app.executor.execute_blocking(&plan, app.lifetime());
        assert!(r.error.is_none());
        assert!(dst.join("a.txt").exists() && !src.join("a.txt").exists());
        assert!(
            app.dispatch(&Invocation::new("clipboard_paste")).is_err(),
            "cut clears the clipboard"
        );
        assert!(
            app.pending.get().entries.is_empty(),
            "nothing is waiting after the move"
        );
    }

    #[test]
    fn a_cut_survives_a_paste_that_fails() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        let dst = tmp.path().join("dst");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dst).unwrap();
        std::fs::write(src.join("a.txt"), b"a").unwrap();
        std::fs::write(dst.join("a.txt"), b"in the way").unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(OpsBasicPlugin::default()), &app);
        let pane = app.workspace.read().active_id();
        app.navigate(pane, &Location::from_path(&src)).unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
        app.dispatch(&Invocation::with_args(
            "select",
            &serde_json::json!({ "names": ["a.txt"] }),
        ))
        .unwrap();
        app.dispatch(&Invocation::new("clipboard_cut")).unwrap();
        app.navigate(pane, &Location::from_path(&dst)).unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
        let Outcome::Plan(plan) = app.dispatch(&Invocation::new("clipboard_paste")).unwrap() else {
            panic!("expected plan")
        };
        assert!(
            app.pending.get().cut,
            "building the plan keeps the cut until it is applied"
        );
        let r = app.executor.execute_blocking(&plan, app.lifetime());
        assert!(r.error.is_some(), "the name is taken");
        assert_eq!(app.pending.get().entries.len(), 1, "still waiting");

        std::fs::remove_file(dst.join("a.txt")).unwrap();
        let Outcome::Plan(plan) = app.dispatch(&Invocation::new("clipboard_paste")).unwrap() else {
            panic!("expected plan")
        };
        assert!(
            app.executor
                .execute_blocking(&plan, app.lifetime())
                .error
                .is_none()
        );
        assert!(dst.join("a.txt").exists());
        assert!(app.pending.get().entries.is_empty());
    }
}
