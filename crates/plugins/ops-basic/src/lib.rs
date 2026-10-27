//! skwir plugin: ops-basic. The default keymap and context menu, the
//! clipboard (copy / cut / paste as plans) and Copy Path, the paths as text.
//!
//! The clipboard is the system's (ADR-062): Copy and Cut hand their files to
//! the window (`Outcome::Files`), and remember them here by identity while
//! they are there (the window forgets them with `clipboard_clear` once
//! anything else is copied). Paste with nothing remembered asks the window
//! for what the system's holds (`WantsClipboard`), and takes it as
//! `clipboard`.
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
//!   (`skwir_core::location::Verbs`)
//! - `!token`: the token does not hold
//!
//! Absent (`None`) means always. For key bindings the UI also filters by
//! focus context: `pane` (the file list), `address`, `palette`, or none
//! for global bindings; that context is carried in the same `when` string
//! as its first token.

use std::sync::Arc;

use parking_lot::Mutex;
use skwir_core::app::{App, PaneArg, TargetsArgs};
use skwir_core::command::{ClipboardArg, Invocation, Outcome, WantsClipboard};
use skwir_core::op::{Change, Copy, EntryRef, Move, Operation, Plan};
use skwir_core::plugin::Plugin;
use skwir_core::registry::{KeyBinding, MenuItem};
use skwir_core::workspace::PaneId;

const ID: &str = "ops-basic";

#[derive(Debug, Default)]
struct Clipboard {
    entries: Vec<EntryRef>,
    cut: bool,
    /// Where they were cut or copied: a paste into a place that takes
    /// files (a tag's) is told, so a cut from another tag moves them.
    from: Option<skwir_core::Location>,
    /// Places cut or copied (a tag's): pasted in a folder, copied there as
    /// their location says (`Verbs::export`); in a place, given to it.
    places: Vec<skwir_core::Location>,
    /// The cut was pasted on a place (a tag's), as a plan that gives the
    /// files their notes there: each is done once that plan gave it them
    /// (`follow`), kept while it is refused, fails or is cancelled.
    pasted: bool,
}

/// `clipboard_paste`: into a pane, from Skwir's clipboard or, when given,
/// what the system's holds.
#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
struct PasteArgs {
    /// The pane to paste into; the active pane when omitted.
    #[serde(default)]
    pane: Option<PaneId>,
    /// Files to paste instead of Skwir's clipboard: what another
    /// application copied or cut, as the window read it.
    #[serde(default)]
    clipboard: Option<ClipboardArg>,
}

/// What a copy or a cut hands the window for the system's clipboard: the
/// files' paths; nothing for places alone (a tag's), which have none.
fn handed(entries: &[EntryRef], cut: bool) -> Outcome {
    if entries.is_empty() {
        return Outcome::Done;
    }
    Outcome::Files {
        paths: entries.iter().map(|entry| entry.path.clone()).collect(),
        cut,
    }
}

/// What a paste takes: the files, whether they were cut, places, and where
/// they were taken from; from Skwir's clipboard, else what the window read
/// of the system's.
type Taken = (
    Vec<EntryRef>,
    bool,
    Vec<skwir_core::Location>,
    Option<skwir_core::Location>,
);

fn taken(clipboard: &Mutex<Clipboard>, given: Option<ClipboardArg>) -> anyhow::Result<Taken> {
    if let Some(given) = given {
        let entries = skwir_core::op::EntryArg::refs(given.entries)?;
        if entries.is_empty() {
            anyhow::bail!("nothing to paste");
        }
        return Ok((entries, given.cut, Vec::new(), None));
    }
    let c = clipboard.lock();
    if c.entries.is_empty() && c.places.is_empty() {
        return Err(WantsClipboard.into());
    }
    Ok((c.entries.clone(), c.cut, c.places.clone(), c.from.clone()))
}

/// The ops-basic plugin.
#[derive(Debug, Default)]
pub struct OpsBasicPlugin {
    clipboard: Arc<Mutex<Clipboard>>,
}

/// The selection's files, and its places (a tag's), what a copy or a cut
/// remembers; or the entries given, from where they are.
fn take_selection(
    app: &App,
    pane: Option<PaneId>,
    entries: Vec<skwir_core::op::EntryArg>,
) -> anyhow::Result<(Vec<EntryRef>, Vec<skwir_core::Location>)> {
    if !entries.is_empty() {
        return Ok((skwir_core::op::EntryArg::refs(entries)?, Vec::new()));
    }
    let pane = app.pane_or_active(pane);
    let refs = app.selected_refs(pane);
    let places: Vec<skwir_core::Location> = {
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
        return Err(skwir_core::command::NothingSelected.into());
    }
    Ok((refs, places))
}

/// Where a copy or a cut takes its files from: the pane's location for
/// its selection (a cut from a tag moves the files out of it), nowhere
/// in particular for entries named by path.
fn taken_from(app: &App, a: &TargetsArgs) -> Option<skwir_core::Location> {
    a.entries
        .is_empty()
        .then(|| app.pane_location(app.pane_or_active(a.pane)))
        .flatten()
}

/// The paths of the selection, or of the entries given, as text: one per
/// line, as other applications take them (a file chooser, a chat). With
/// nothing selected, the folder shown. A path that is not text is
/// refused: written lossily, it would name another file.
fn copy_paths(app: &App, a: TargetsArgs) -> anyhow::Result<Outcome> {
    let pane = app.pane_or_active(a.pane);
    let paths = match app.targets(Some(pane), a.entries) {
        Ok(refs) => refs.into_iter().map(|r| r.path).collect(),
        Err(e) if skwir_core::command::is_nothing_selected(&e) => {
            vec![app.pane_dir(pane).map_err(|_| e)?]
        }
        Err(e) => return Err(e),
    };
    let lines = paths
        .iter()
        .map(|path| {
            path.to_str().ok_or_else(|| {
                let name = std::path::Path::new(path.file_name().unwrap_or(path.as_os_str()));
                anyhow::anyhow!(
                    "The path of {} is not text the clipboard can hold",
                    name.display()
                )
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let said = match lines.len() {
        1 => "Copied the path".to_owned(),
        n => format!("Copied {n} paths"),
    };
    Ok(Outcome::Clipboard {
        text: lines.join("\n"),
        said,
    })
}

/// Let every view show what is waiting for a paste.
fn publish(app: &App, entries: &[EntryRef], cut: bool) {
    let named = entries
        .iter()
        .filter_map(|entry| Some((entry.id, entry.path.file_name()?.to_owned())));
    app.pending.set(ID, named, cut);
    app.wake();
}

/// Follow what the clipboard holds through applied changes. A cut entry
/// that was moved (by its paste or otherwise), trashed or deleted leaves
/// it, as does one pasted on a place once given its notes there; once
/// none is left, nothing waits for a paste. So a paste that is refused,
/// fails or is cancelled keeps what was cut.
fn follow(clipboard: &Mutex<Clipboard>, changes: &[Change], app: &App) {
    let mut c = clipboard.lock();
    if c.entries.is_empty() {
        return;
    }
    let before = c.entries.len();
    let (cut, pasted) = (c.cut, c.pasted);
    let follow = skwir_core::op::Follow::new(changes);
    let noted: Vec<&std::path::Path> = changes
        .iter()
        .filter_map(|change| match change {
            Change::Metadata { path, .. } => Some(path.as_path()),
            _ => None,
        })
        .collect();
    c.entries.retain_mut(|entry| {
        let through = follow.through(&entry.path);
        match through.at {
            None => false,
            // A cut entry that moved went where it was going.
            Some(_) if cut && through.moved => false,
            // One pasted on a place is there once given its notes.
            Some(at)
                if cut
                    && pasted
                    && noted
                        .iter()
                        .any(|p| *p == entry.path.as_path() || *p == at.as_path()) =>
            {
                false
            }
            Some(at) => {
                entry.path = at;
                true
            }
        }
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

fn paste(clipboard: &Mutex<Clipboard>, a: PasteArgs, app: &App) -> anyhow::Result<Outcome> {
    let pane = app.pane_or_active(a.pane);
    let Ok(dest) = app.pane_dir(pane) else {
        return paste_on_place(clipboard, pane, a.clipboard, app);
    };
    let (entries, cut, places, _) = taken(clipboard, a.clipboard)?;
    // Places (a tag's) are copied in as their location says: a folder of
    // their files.
    for place in places {
        if let Some(mut export) = app.verbs(&place).export {
            if !export.args.is_object() {
                export.args = serde_json::json!({});
            }
            export.args["dest"] =
                serde_json::json!(skwir_core::os_path::PathArg::from(dest.as_path()));
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
                Box::new(Move::new(entry, dest.clone()))
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
/// done once pasted: once the plan it answers applies, when it answers
/// one.
fn paste_on_place(
    clipboard: &Mutex<Clipboard>,
    pane: PaneId,
    given: Option<ClipboardArg>,
    app: &App,
) -> anyhow::Result<Outcome> {
    let shown = app
        .pane_location(pane)
        .ok_or_else(|| anyhow::anyhow!("the pane shows nothing"))?;
    let Some(drop) = app.verbs(&shown).drop else {
        anyhow::bail!("nothing can be pasted here");
    };
    let own = given.is_none();
    let (entries, cut, places, from) = taken(clipboard, given)?;
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
    if cut && own {
        if let Outcome::Plan(_) = &outcome {
            // Done once that plan applies (`follow`).
            clipboard.lock().pasted = true;
        } else {
            *clipboard.lock() = Clipboard::default();
            app.pending.clear();
            app.wake();
        }
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
        "secondary-alt-o",
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
    ("secondary-c", "clipboard_copy", Some("pane selection")),
    // In the trash too: pasted, a file of the trash leaves it as a
    // restore does, to where it is pasted (`op::Move`).
    ("secondary-x", "clipboard_cut", Some("pane selection")),
    // Windows' Copy as path and Dolphin's Copy Location; with nothing
    // selected, the folder's.
    ("ctrl-shift-c", "copy_paths", Some("pane selection|folder")),
    ("ctrl-alt-c", "copy_paths", Some("pane selection|folder")),
    // Into the folder shown, or a place that takes files (a tag gives
    // them itself): a search has neither.
    ("secondary-v", "clipboard_paste", Some("pane folder|drops")),
    ("secondary-z", "undo", None),
    ("secondary-shift-z", "redo", None),
    ("ctrl-y", "redo", None),
    ("secondary-a", "select_all", Some("pane")),
    ("secondary-shift-i", "select_invert", Some("pane")),
    ("escape", "select_none", Some("pane")),
    // With nothing selected, Escape leaves a search for the folder it looks
    // in: its pills hide the trail that would lead back. A tag keeps its
    // trail: Escape never leaves a place, as it never leaves a folder.
    // Later bindings win, and `empty` holds only then. A cut or a copy
    // waits through Escape, to be pasted where it leads (the footer's
    // count, or `clipboard_clear` in the palette, forgets it).
    ("escape", "up", Some("pane empty search")),
    ("f5", "refresh", None),
    ("secondary-r", "refresh", None),
    // The folder or search being looked at, as in Nautilus and browsers.
    ("secondary-d", "keep", None),
    // Dolphin's key for it: Ctrl+T opens a tab, as everywhere.
    ("shift-f4", "open_terminal", None),
    ("secondary-t", "tab.new", None),
    ("secondary-w", "tab.close", None),
    ("secondary-shift-t", "tab.reopen", None),
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
// convention documented on `skwir_core::registry::MenuItem`: hundreds are
// groups (0 open it, 100 copy and keep it, 200 change it, 300 make or find
// from it, 400 views and tools, 500 describe it: ADR-099), the UI draws a
// separator between groups. "Open" and "Open
// Terminal Here" are contributed by the open-with plugin, which owns those
// commands.
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
        200,
        "rename",
    ),
    // A place too: a tag copied into a folder is a folder of its files,
    // cut and pasted in a tag it goes inside. A file of the trash pasted
    // elsewhere leaves the trash.
    ("Cut", "clipboard_cut", Some("selection"), 110, "cut"),
    ("Copy", "clipboard_copy", Some("selection"), 120, "copy"),
    // Their paths as text; a place (a tag's) has none. On the background,
    // the folder's own.
    (
        "Copy Path",
        "copy_paths",
        Some("single !place"),
        125,
        "link",
    ),
    (
        "Copy Paths",
        "copy_paths",
        Some("selection !single !place"),
        125,
        "link",
    ),
    ("Copy Path", "copy_paths", Some("empty folder"), 125, "link"),
    // The folder's own menu, on its background: Paste puts what was cut or
    // copied in the folder shown, never in a file right-clicked.
    (
        "Paste",
        "clipboard_paste",
        Some("empty folder|drops"),
        130,
        "paste",
    ),
    // The folder's own menu, on its background.
    (
        "New Folder",
        "ui.mkdir_prompt",
        Some("empty folder"),
        300,
        "folder-plus",
    ),
    (
        "New File",
        "ui.new_file_prompt",
        Some("empty folder"),
        301,
        "file-plus",
    ),
    // Files, never places (a tag's has no file to trash), nor what is in
    // the trash already: the end of what changes them, beside Rename.
    (
        "Move to Trash",
        "trash",
        Some("selection !place !restores"),
        290,
        "trash",
    ),
    (
        "Delete Permanently",
        "delete",
        Some("selection !place"),
        295,
        "close",
    ),
    ("Refresh", "refresh", Some("empty"), 490, "refresh"),
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

    fn category(&self) -> skwir_core::plugin::Category {
        skwir_core::plugin::Category::Files
    }

    fn capabilities(&self) -> skwir_core::permissions::Capabilities {
        use skwir_core::permissions::Capability;
        skwir_core::permissions::Capabilities::of(&[Capability::Write, Capability::Programs])
    }
    fn register(&self, app: &App) {
        let cb = Arc::clone(&self.clipboard);
        app.commands.register::<TargetsArgs, _>(
            ID,
            "clipboard_copy",
            "Copy",
            "Remember the selection, or the entries given, for a later paste (copy)",
            move |a, app| {
                let from = taken_from(app, &a);
                let (entries, places) = take_selection(app, a.pane, a.entries)?;
                publish(app, &entries, false);
                let outcome = handed(&entries, false);
                *cb.lock() = Clipboard {
                    entries,
                    cut: false,
                    from,
                    places,
                    pasted: false,
                };
                Ok(outcome)
            },
        );
        let cb = Arc::clone(&self.clipboard);
        app.commands.register::<TargetsArgs, _>(
            ID,
            "clipboard_cut",
            "Cut",
            "Remember the selection, or the entries given, for a later paste (move)",
            move |a, app| {
                let from = taken_from(app, &a);
                let (entries, places) = take_selection(app, a.pane, a.entries)?;
                publish(app, &entries, true);
                let outcome = handed(&entries, true);
                *cb.lock() = Clipboard {
                    entries,
                    cut: true,
                    from,
                    places,
                    pasted: false,
                };
                Ok(outcome)
            },
        );
        app.commands
            .register::<TargetsArgs, _>(
                ID,
                "copy_paths",
                "Copy paths",
                "The selected paths, or the entries', or with nothing selected the folder shown, as text for the clipboard, one per line",
                |a, app| copy_paths(app, a),
            )
            .reads();
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
        app.commands
            .register::<PasteArgs, _>(
                ID,
                "clipboard_paste",
                "Paste",
                "Copy or move the clipboard into the current directory (plan): Skwir's, or the files given as `clipboard`",
                move |a, app| paste(&cb, a, app),
            )
            .plans();
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
                    checked: None,
                    choice: None,
                    color: None,
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
    use skwir_core::app::AppConfig;
    use skwir_core::location::Location;
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
                .any(|b| b.keys == "secondary-z" && &*b.command.name == "undo")
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
            pending.cut && pending.len() == 1,
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
            app.pending.get().is_empty(),
            "nothing is waiting after the move"
        );
    }

    fn copied(app: &App) -> (String, String) {
        match app.dispatch(&Invocation::new("copy_paths")).unwrap() {
            Outcome::Clipboard { text, said } => (text, said),
            other => panic!("expected text for the clipboard, got {other:?}"),
        }
    }

    #[test]
    fn copy_paths_gives_plain_text_and_the_folder_with_nothing_selected() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("it's a folder");
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["a b.txt", "café's.txt"] {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(OpsBasicPlugin::default()), &app);
        let pane = app.workspace.read().active_id();
        app.navigate(pane, &Location::from_path(&dir)).unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
        assert_eq!(
            copied(&app),
            (dir.to_str().unwrap().to_owned(), "Copied the path".into()),
            "nothing selected: the folder shown, unquoted"
        );

        app.dispatch(&Invocation::with_args(
            "select",
            &serde_json::json!({ "names": ["a b.txt", "café's.txt"] }),
        ))
        .unwrap();
        let (text, said) = copied(&app);
        let mut lines: Vec<&str> = text.lines().collect();
        lines.sort_unstable();
        assert_eq!(
            lines,
            [
                dir.join("a b.txt").to_str().unwrap(),
                dir.join("café's.txt").to_str().unwrap(),
            ],
            "one path a line, as it is"
        );
        assert_eq!(said, "Copied 2 paths");
    }

    /// A name that is not UTF-8 has no text that names it: the copy is
    /// refused rather than naming another file.
    #[cfg(target_os = "linux")]
    #[test]
    fn copy_paths_refuses_a_path_that_is_not_text() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("latin1");
        std::fs::create_dir_all(&dir).unwrap();
        let name = skwir_core::os_path::from_bytes(b"caf\xe9.txt".to_vec());
        std::fs::write(dir.join(&name), b"x").unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(OpsBasicPlugin::default()), &app);
        let pane = app.workspace.read().active_id();
        app.navigate(pane, &Location::from_path(&dir)).unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
        app.dispatch(&Invocation::new("select_all")).unwrap();
        let error = app.dispatch(&Invocation::new("copy_paths")).unwrap_err();
        assert!(
            error.to_string().contains("is not text"),
            "refused: {error}"
        );
    }

    /// ADR-062: Copy and Cut hand their files to the window; Paste with
    /// nothing in Skwir's clipboard asks for the system's, and takes it as
    /// `clipboard`, a copy or a move as it says.
    #[test]
    fn the_clipboard_is_handed_to_the_window_and_taken_from_it() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("it's from");
        let dst = tmp.path().join("to");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dst).unwrap();
        std::fs::write(src.join("a b.txt"), b"a").unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(OpsBasicPlugin::default()), &app);
        let pane = app.workspace.read().active_id();
        app.navigate(pane, &Location::from_path(&src)).unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
        app.dispatch(&Invocation::with_args(
            "select",
            &serde_json::json!({ "names": ["a b.txt"] }),
        ))
        .unwrap();
        for (command, was_cut) in [("clipboard_copy", false), ("clipboard_cut", true)] {
            let Outcome::Files { paths, cut } = app.dispatch(&Invocation::new(command)).unwrap()
            else {
                panic!("{command}: files for the window");
            };
            assert_eq!((paths, cut), (vec![src.join("a b.txt")], was_cut));
        }
        app.dispatch(&Invocation::new("clipboard_clear")).unwrap();

        app.navigate(pane, &Location::from_path(&dst)).unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
        let error = app
            .dispatch(&Invocation::new("clipboard_paste"))
            .unwrap_err();
        assert!(
            skwir_core::command::wants_clipboard(&error),
            "nothing of Skwir's: the window reads the system's ({error})"
        );
        let given = |cut: bool| {
            Invocation::with_args(
                "clipboard_paste",
                &serde_json::json!({ "clipboard": {
                    "entries": [skwir_core::os_path::PathArg::from(src.join("a b.txt"))],
                    "cut": cut,
                }}),
            )
        };
        for (cut, kind) in [(false, "copy"), (true, "move")] {
            let Outcome::Plan(plan) = app.dispatch(&given(cut)).unwrap() else {
                panic!("expected a plan")
            };
            assert_eq!(plan.steps.len(), 1);
            assert_eq!(plan.steps[0].kind(), kind);
        }
        let Outcome::Plan(plan) = app.dispatch(&given(true)).unwrap() else {
            panic!("expected a plan")
        };
        assert!(
            app.executor
                .execute_blocking(&plan, app.lifetime())
                .error
                .is_none()
        );
        assert!(dst.join("a b.txt").exists() && !src.join("a b.txt").exists());
        let gone = app.dispatch(&given(false)).unwrap_err();
        assert!(gone.to_string().contains("does not exist"), "{gone}");
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
        assert_eq!(app.pending.get().len(), 1, "still waiting");

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
        assert!(app.pending.get().is_empty());
    }
}
