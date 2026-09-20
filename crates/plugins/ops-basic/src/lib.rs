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
//!
//! Absent (`None`) means always. For key bindings the UI also filters by
//! focus context: `pane` (the file list), `address`, `palette`, or none
//! for global bindings; that context is carried in the same `when` string
//! as its first token.

use std::sync::Arc;

use bibli_core::app::App;
use bibli_core::command::{Invocation, Outcome};
use bibli_core::location::Location;
use bibli_core::op::{Copy, EntryRef, Move, Operation, Plan};
use bibli_core::plugin::Plugin;
use bibli_core::registry::{KeyBinding, MenuItem};
use bibli_core::workspace::PaneId;
use parking_lot::Mutex;

const ID: &str = "ops-basic";

#[derive(Debug, Default)]
struct Clipboard {
    entries: Vec<EntryRef>,
    cut: bool,
}

/// The ops-basic plugin.
#[derive(Debug, Default)]
pub struct OpsBasicPlugin {
    clipboard: Arc<Mutex<Clipboard>>,
}

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
struct PaneArgs {
    #[serde(default)]
    pane: Option<PaneId>,
}

fn pane_of(app: &App, pane: Option<PaneId>) -> PaneId {
    pane.unwrap_or_else(|| app.workspace.read().active_id())
}

fn take_selection(app: &App, pane: Option<PaneId>) -> anyhow::Result<Vec<EntryRef>> {
    let refs = app.selected_refs(pane_of(app, pane));
    if refs.is_empty() {
        anyhow::bail!("nothing selected");
    }
    Ok(refs)
}

/// Let every view show what is waiting for a paste.
fn publish(app: &App, entries: &[EntryRef], cut: bool) {
    app.pending
        .set(ID, entries.iter().map(|entry| entry.id), cut);
    app.wake();
}

fn paste(clipboard: &Mutex<Clipboard>, pane: Option<PaneId>, app: &App) -> anyhow::Result<Outcome> {
    let pane = pane_of(app, pane);
    let dest = {
        let ws = app.workspace.read();
        let p = ws
            .pane(pane)
            .ok_or_else(|| anyhow::anyhow!("no such pane"))?;
        p.location()
            .and_then(Location::path)
            .ok_or_else(|| anyhow::anyhow!("pane is not showing a directory"))?
    };
    let (entries, cut) = {
        let mut c = clipboard.lock();
        if c.entries.is_empty() {
            anyhow::bail!("clipboard is empty");
        }
        let cut = c.cut;
        let entries = if cut {
            std::mem::take(&mut c.entries)
        } else {
            c.entries.clone()
        };
        if cut {
            c.cut = false;
            // Moved away: nothing is waiting any more.
            app.pending.clear();
        }
        (entries, cut)
    };
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

const BINDINGS: &[(&str, &str, Option<&str>)] = &[
    ("enter", "open", Some("pane")),
    ("backspace", "up", Some("pane")),
    ("alt-up", "up", None),
    ("alt-left", "back", None),
    ("alt-right", "forward", None),
    ("f2", "ui.rename_prompt", Some("pane single")),
    ("delete", "trash", Some("pane selection")),
    ("shift-delete", "ui.confirm_delete", Some("pane selection")),
    ("ctrl-c", "clipboard_copy", Some("pane selection")),
    ("ctrl-x", "clipboard_cut", Some("pane selection")),
    ("ctrl-v", "clipboard_paste", Some("pane")),
    ("ctrl-z", "undo", None),
    ("ctrl-shift-z", "redo", None),
    ("ctrl-a", "select_all", Some("pane")),
    ("escape", "select_none", Some("pane")),
    // With nothing selected Escape has one thing left to cancel: a pending
    // cut or copy. Later bindings win, and `none` holds only then.
    ("escape", "clipboard_clear", Some("pane none")),
    ("f5", "refresh", None),
    ("ctrl-shift-n", "ui.mkdir_prompt", None),
    ("ctrl-l", "ui.focus_address", None),
    ("ctrl-shift-p", "ui.palette", None),
    ("ctrl-t", "open_terminal", None),
    ("ctrl-q", "ui.quit", None),
    ("ctrl-h", "ui.toggle_hidden", None),
];

// Context menu items: (title, command, when, order). Order follows the
// convention documented on `bibli_core::registry::MenuItem`: hundreds are
// groups (0 open, 100 edit, 200 create, 300 destructive, 400 misc), the UI
// draws a separator between groups. "Open" and "Open Terminal Here" are
// contributed by the open-with plugin, which owns those commands.
const MENU: &[(&str, &str, Option<&str>, i32, &str)] = &[
    ("Rename", "ui.rename_prompt", Some("single"), 100, "rename"),
    ("Cut", "clipboard_cut", Some("selection"), 110, "cut"),
    ("Copy", "clipboard_copy", Some("selection"), 120, "copy"),
    ("Paste", "clipboard_paste", None, 130, "paste"),
    // The folder's own menu, on its background.
    (
        "New Folder",
        "ui.mkdir_prompt",
        Some("empty"),
        200,
        "folder-plus",
    ),
    ("Move to Trash", "trash", Some("selection"), 300, "trash"),
    (
        "Delete Permanently",
        "ui.confirm_delete",
        Some("selection"),
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

    fn register(&self, app: &App) {
        let cb = Arc::clone(&self.clipboard);
        app.commands.register::<PaneArgs, _>(
            ID,
            "clipboard_copy",
            "Copy",
            "Remember the selection for a later paste (copy)",
            move |a, app| {
                let entries = take_selection(app, a.pane)?;
                publish(app, &entries, false);
                *cb.lock() = Clipboard {
                    entries,
                    cut: false,
                };
                Ok(Outcome::Done)
            },
        );
        let cb = Arc::clone(&self.clipboard);
        app.commands.register::<PaneArgs, _>(
            ID,
            "clipboard_cut",
            "Cut",
            "Remember the selection for a later paste (move)",
            move |a, app| {
                let entries = take_selection(app, a.pane)?;
                publish(app, &entries, true);
                *cb.lock() = Clipboard { entries, cut: true };
                Ok(Outcome::Done)
            },
        );
        let cb = Arc::clone(&self.clipboard);
        app.commands.register::<PaneArgs, _>(
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
        app.commands.register::<PaneArgs, _>(
            ID,
            "clipboard_paste",
            "Paste",
            "Copy or move the clipboard into the current directory (plan)",
            move |a, app| paste(&cb, a.pane, app),
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
                .any(|m| m.title == "Rename" && m.when.as_deref() == Some("single"))
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
}
