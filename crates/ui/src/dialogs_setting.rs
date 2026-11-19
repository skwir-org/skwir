//! Settings › "Open and save dialogs" (ADR-141, 10): which back end of
//! the desktop portal answers other programs' Open and Save dialogs, a
//! choice the portal's configuration keeps (`SettingSpec::choice_kept_by`):
//! nothing is saved here. Listed on the window's thread from what was last
//! read, read again on a worker each time it is listed (an answer that
//! moves what the row says has the dialog list it again); chosen on a
//! worker, said with Undo. The portal reads its configuration as it
//! starts: a choice applies then, the next login, and the row says so.
//! Linux's alone (ADR-141, 11).

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use parking_lot::Mutex;
use schemars::JsonSchema;
use serde::Deserialize;
use skwir_core::App;
use skwir_core::app::NotifyLevel;
use skwir_core::command::{Invocation, NoArgs, Outcome, Topic};
use skwir_core::scheduler::Priority;
use skwir_core::settings::{ChoiceSource, SettingSpec};
use skwir_core::words;
use skwir_core::words::Words;
use skwir_platform::portals::{Before, Named, Places, Portals};

/// The setting: a choice the portal's configuration keeps.
pub(crate) const KEY: &str = "ui.file_chooser";
/// The commands that list it, choose and put back: the core's, not the
/// window's (`ui.*` the window carries out itself), so Undo's runs from a
/// notice too.
const LIST: &str = "dialogs.list";
const SET: &str = "dialogs.choose";
const PUT_BACK: &str = "dialogs.put_back";
const OWNER: &str = "ui";
/// The back end skwir's `.portal` file names.
const OWN: &str = "skwir";

/// Where the portal looks, read when asked: the session's, or a test's.
type Where = Arc<dyn Fn() -> Places + Send + Sync>;

/// What the row is made of.
struct Row {
    source: ChoiceSource,
    places: Where,
    /// What was last read.
    read: Mutex<Option<Portals>>,
    /// A worker reads it now.
    asking: AtomicBool,
    /// The choices made, by the serial their Undo names: kept here, so
    /// that Undo names no file nor its text.
    done: Mutex<HashMap<u64, Done>>,
    next: AtomicU64,
}

/// A choice made: what to put back, and what was chosen (for Redo).
struct Done {
    before: Before,
    portal: String,
}

#[derive(Deserialize, JsonSchema)]
struct SetArgs {
    /// The back end's name, as its `.portal` file gives it (`skwir`,
    /// `kde`); empty: none named, the desktop's configuration deciding.
    portal: String,
}

#[derive(Deserialize, JsonSchema)]
struct PutBackArgs {
    /// The choice to take back, as its notification's Undo names it.
    done: u64,
}

/// Declare the row and its commands, for the session's portal.
pub(crate) fn register(app: &App) {
    register_with(app, Arc::new(Places::here));
}

fn register_with(app: &App, places: Where) {
    let row = Arc::new(Row {
        source: ChoiceSource::new(LIST),
        places,
        read: Mutex::default(),
        asking: AtomicBool::default(),
        done: Mutex::default(),
        next: AtomicU64::new(1),
    });
    app.settings.declare(
        SettingSpec::choice_kept_by(
            KEY,
            words!(
                settings_file_chooser = "Open and save dialogs",
                note = "A setting's title on the General page: what answers other programs' Open and Save dialogs."
            ),
            (
                words!(
                    settings_file_chooser_description = "What answers other programs' Open and Save dialogs, those that ask the desktop for one: Skwir's windows, or another program's dialogs.",
                    note = "Open and Save are what programs call their dialogs for choosing a file."
                ),
                words!(
                    settings_file_chooser_description_more = "The desktop portal reads this as it starts: a change applies at the next login. Programs that draw their own dialogs keep them.",
                    note = "What a setting's description says after its first sentence, in its tooltip. The desktop portal is the desktop's service programs ask for dialogs."
                ),
            ),
            row.source.clone(),
        )
        .page(skwir_core::settings::page::GENERAL)
        .section(skwir_core::settings::section::STARTUP)
        // Beside the default file manager, which is one: it decides what
        // other programs see (ADR-141, 10; ADR-072).
        .essential(),
    );
    let listed = Arc::clone(&row);
    app.commands
        .register::<NoArgs, _>(
            OWNER,
            Topic::Skwir,
            LIST,
            words!(commands_file_chooser_list = "What answers open and save dialogs"),
            words!(
                commands_file_chooser_list_description = "What answers other programs' Open and Save dialogs, as Settings lists it: the desktop portal's back ends installed for them, each with the `dialogs.choose` that chooses it, and the one its configuration names. Answers from memory"
            ),
            move |_, app| {
                ask(app, &listed);
                Ok(Outcome::Value(answer(listed.read.lock().as_ref())))
            },
        )
        .data();
    let chosen = Arc::clone(&row);
    app.commands
        .register::<SetArgs, _>(
            OWNER,
            Topic::Skwir,
            SET,
            words!(commands_file_chooser_set = "Choose what answers open and save dialogs"),
            words!(
                commands_file_chooser_set_description = "Name `portal` (a back end of the desktop portal, by its .portal file's name; empty for none) for other programs' Open and Save dialogs in the user's portal configuration, its other lines kept; it applies when the portal starts again. The notification offers Undo"
            ),
            move |a, app| {
                let row = Arc::clone(&chosen);
                spawn(app, move |app| set(app, &row, &a.portal, false));
                Ok(Outcome::Done)
            },
        )
        .user_only();
    let back = Arc::clone(&row);
    app.commands
        .register::<PutBackArgs, _>(
            OWNER,
            Topic::Skwir,
            PUT_BACK,
            words!(commands_file_chooser_put_back = "Put back what answered open and save dialogs"),
            words!(
                commands_file_chooser_put_back_description = "Put the user's portal configuration back as it was before the choice `done` (what Undo of `dialogs.choose` runs, from its notification), only while the file holds what that choice wrote"
            ),
            move |a, app| {
                let row = Arc::clone(&back);
                spawn(app, move |app| put_back(app, &row, a));
                Ok(Outcome::Done)
            },
        )
        .user_only();
}

/// Run `work` on a worker, for the program's life.
fn spawn(app: &App, work: impl FnOnce(&App) + Send + 'static) {
    let weak = app.weak();
    app.scheduler.spawn(
        Priority::Background,
        "ui.file_chooser",
        app.lifetime().child(),
        move || {
            if let Some(app) = weak.upgrade() {
                work(&app);
            }
        },
    );
}

/// Read again on a worker, once at a time: an answer that moves what the
/// row says has the dialog list it again.
fn ask(app: &App, row: &Arc<Row>) {
    if row.asking.swap(true, Ordering::AcqRel) {
        return;
    }
    let row = Arc::clone(row);
    spawn(app, move |app| {
        read_now(app, &row);
        row.asking.store(false, Ordering::Release);
    });
}

fn read_now(app: &App, row: &Row) {
    let now = skwir_platform::portals::read(&(row.places)());
    let moved = row.read.lock().replace(now.clone()) != Some(now);
    if moved {
        row.source.changed();
        app.wake();
    }
}

/// A back end as people know it: its name, as its makers write it.
fn label(name: &str) -> Words {
    let known = [
        ("skwir", "Skwir"),
        ("kde", "KDE"),
        ("gtk", "GTK"),
        ("gnome", "GNOME"),
        ("lxqt", "LXQt"),
        ("xapp", "XApp"),
        ("cosmic", "COSMIC"),
        ("deepin", "Deepin"),
        ("pantheon", "Pantheon"),
    ];
    let written = known
        .iter()
        .find(|(id, _)| *id == name)
        .map_or(name, |(_, label)| label);
    Words::as_written(written.to_owned())
}

fn unnamed() -> Words {
    words!(
        settings_file_chooser_unnamed = "As the desktop chooses",
        note = "The Open and save dialogs setting's choice when the portal's configuration names nothing for them: the desktop's own way of choosing."
    )
}

/// The row's answer (`ChoiceSource`): what answers the dialogs, the back
/// ends to choose from, and when a choice applies. Its words are data
/// (`Words::to_data`): the dialog says them in the window's language, an
/// agent or a script reads their English.
fn answer(read: Option<&Portals>) -> serde_json::Value {
    let Some(read) = read else {
        return serde_json::json!({
            "detail": words!(
                settings_file_chooser_reading = "Reading the desktop portal's configuration…",
                note = "Said under the Open and save dialogs setting while it is read."
            )
            .to_data(),
        });
    };
    let mut choices: Vec<serde_json::Value> = vec![serde_json::json!({
        "value": "",
        "label": unnamed().to_data(),
    })];
    choices.extend(
        read.choosers
            .iter()
            .map(|name| serde_json::json!({ "value": name, "label": label(name).to_data() })),
    );
    let (value, shown) = match &read.chooser {
        Named::BackEnd(name) => (name.as_str(), label(name)),
        Named::Unnamed => ("", unnamed()),
        // No choice of the row's: the configuration's own `none`.
        Named::Refused => (
            "none",
            words!(
                settings_file_chooser_refused = "None: the desktop portal shows no dialog",
                note = "The Open and save dialogs setting's value where the portal's configuration says none answers them (its 'none'): programs that ask the portal get no dialog."
            ),
        ),
    };
    let detail = if !read.choosers.iter().any(|name| name == OWN) {
        words!(
            settings_file_chooser_not_installed =
                "Skwir cannot be chosen here: its portal file is not installed",
            note = "Said under the Open and save dialogs setting where Skwir's package did not install the file that offers it to the desktop portal (a build run from its folder)."
        )
    } else if read.pending {
        words!(
            settings_file_chooser_pending = "Changed since the desktop portal started: this applies at its next start, the next login",
            note = "Said under the Open and save dialogs setting when its configuration was changed after the desktop portal read it: what the setting shows is not what answers yet."
        )
    } else {
        words!(
            settings_file_chooser_applies =
                "Applies when the desktop portal starts again, at the next login",
            note = "Said under the Open and save dialogs setting: the desktop portal reads its configuration as it starts."
        )
    };
    serde_json::json!({
        "value": value,
        "label": shown.to_data(),
        "choices": choices,
        "command": SET,
        "args": {},
        "field": "portal",
        "detail": detail.to_data(),
    })
}

/// What a choice of `portal` is said as: the desktop's choice (empty) in
/// words of its own, a back end by its name.
fn chosen_words(portal: &str, already: bool) -> Words {
    match (portal.is_empty(), already) {
        (true, true) => words!(
            settings_file_chooser_already_desktop =
                "The desktop already chooses what answers other programs' Open and Save dialogs",
            note = "Said once the desktop's own choice is chosen for what answers other programs' Open and Save dialogs and the user's portal configuration named nothing for them already."
        ),
        (true, false) => words!(
            settings_file_chooser_chosen_desktop = "The desktop chooses what answers other programs' Open and Save dialogs, from the desktop portal's next start",
            note = "Said once the desktop's own choice is chosen for what answers other programs' Open and Save dialogs, with Undo: the user's portal configuration names nothing for them any more; it applies when the portal starts again, at the next login."
        ),
        (false, true) => words!(
            settings_file_chooser_already =
                "Other programs' Open and Save dialogs are already { $chooser }",
            chooser = label(portal),
            note = "Said once what answers other programs' dialogs is chosen and the configuration said so already: chooser is the back end's name (Skwir, KDE)."
        ),
        (false, false) => words!(
            settings_file_chooser_chosen = "Other programs' Open and Save dialogs: { $chooser }, from the desktop portal's next start",
            chooser = label(portal),
            note = "Said once what answers other programs' dialogs is chosen, with Undo: chooser is the back end's name (Skwir, KDE); it applies when the portal starts again, at the next login."
        ),
    }
}

fn failed(error: &std::io::Error) -> Words {
    words!(
        settings_file_chooser_failed =
            "What answers Open and Save dialogs is unchanged: { $error }",
        error = skwir_core::op::io_words(error),
        note = "Said when the portal's configuration cannot be read, written or put back: error is why."
    )
}

/// Choose `portal`, on a worker; Undo puts the file back as it was,
/// naming what it keeps here.
fn set(app: &App, row: &Row, portal: &str, redo: bool) {
    match skwir_platform::portals::choose(&(row.places)(), portal) {
        Ok(Some(before)) => {
            read_now(app, row);
            let serial = row.next.fetch_add(1, Ordering::Relaxed);
            row.done.lock().insert(
                serial,
                Done {
                    before,
                    portal: portal.to_owned(),
                },
            );
            let undo = Invocation::with_args(PUT_BACK, &serde_json::json!({ "done": serial }));
            app.notify_undoable(chosen_words(portal, false), redo, undo);
        }
        Ok(None) => {
            answered(app, row);
            app.notify(NotifyLevel::Info, chosen_words(portal, true));
        }
        Err(error) => {
            answered(app, row);
            app.notify(NotifyLevel::Error, failed(&error));
        }
    }
}

/// A choice asked answered with nothing to read again (as it was, or
/// refused): the row is listed again all the same, as the dialog waits for
/// that before asking anything more of it (`ChoiceSource`).
fn answered(app: &App, row: &Row) {
    row.source.changed();
    app.wake();
}

/// Undo: the file as it was, while it holds what the choice wrote; Redo
/// chooses again.
fn put_back(app: &App, row: &Row, a: PutBackArgs) {
    let Some(done) = row.done.lock().remove(&a.done) else {
        app.notify(
            NotifyLevel::Info,
            words!(
                settings_file_chooser_put_back_gone = "That choice of what answers Open and Save dialogs was undone already",
                note = "Said where Undo of a choice of what answers other programs' Open and Save dialogs is asked again, or of a choice this Skwir did not make."
            ),
        );
        return;
    };
    match skwir_platform::portals::put_back(&done.before) {
        Ok(()) => {
            read_now(app, row);
            let redo = Invocation::with_args(SET, &serde_json::json!({ "portal": done.portal }));
            app.notify_undoable(
                words!(
                    settings_file_chooser_put_back = "What answered other programs' Open and Save dialogs is back, from the desktop portal's next start",
                    note = "Said once a choice of what answers other programs' dialogs is undone, with Redo."
                ),
                true,
                redo,
            );
        }
        Err(error) => app.notify(NotifyLevel::Error, failed(&error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skwir_core::app::AppConfig;
    use std::path::Path;
    use std::time::{Duration, Instant};

    fn portal(dir: &Path, name: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            dir.join(format!("{name}.portal")),
            format!("[portal]\nDBusName=org.freedesktop.impl.portal.desktop.{name}\nInterfaces=org.freedesktop.impl.portal.FileChooser;\n"),
        )
        .unwrap();
    }

    fn places(root: &Path) -> Places {
        Places {
            portal_dirs: vec![root.join("portals")],
            config_dirs: vec![root.join("config"), root.join("etc")],
            user_dir: Some(root.join("config")),
            desktops: vec!["hyprland".into()],
            started: None,
        }
    }

    fn until(what: &str, mut done: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done() {
            assert!(Instant::now() < deadline, "{what}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// The row lists the back ends installed and the one named, says when
    /// a choice applies or that Skwir cannot be chosen; choosing writes
    /// the user's file and says so with Undo, which puts it back.
    #[test]
    fn the_row_chooses_and_undoes() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        portal(&root.join("portals"), "kde");
        std::fs::create_dir_all(root.join("etc")).unwrap();
        std::fs::write(
            root.join("etc/hyprland-portals.conf"),
            "[preferred]\ndefault=hyprland;kde\norg.freedesktop.impl.portal.FileChooser=kde\n",
        )
        .unwrap();
        let app = App::new(AppConfig::ephemeral(root.join("app"))).unwrap();
        let at = root.clone();
        register_with(&app, Arc::new(move || places(&at)));
        let list = || match app.dispatch(&Invocation::new(LIST)).unwrap() {
            Outcome::Value(value) => value,
            other => panic!("{other:?}"),
        };
        until("read", || list()["value"] == "kde");
        let said = list();
        assert_eq!(said["label"], "KDE");
        // Words as data, which an agent reads in English.
        assert_eq!(
            said["detail"]["english"],
            "Skwir cannot be chosen here: its portal file is not installed"
        );

        portal(&root.join("portals"), "skwir");
        until("skwir offered", || {
            list()["choices"]
                .as_array()
                .is_some_and(|c| c.iter().any(|c| c["value"] == "skwir"))
        });
        assert_eq!(
            list()["detail"]["english"],
            "Applies when the desktop portal starts again, at the next login"
        );
        let values: Vec<String> = list()["choices"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["value"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(values, ["", "kde", "skwir"]);

        app.take_notifications();
        app.dispatch(&Invocation::with_args(
            SET,
            &serde_json::json!({ "portal": "skwir" }),
        ))
        .unwrap();
        let mut said = Vec::new();
        until("said", || {
            said.extend(app.take_notifications());
            !said.is_empty()
        });
        assert_eq!(
            said[0].text.english(),
            "Other programs' Open and Save dialogs: Skwir, from the desktop portal's next start"
        );
        let undo = said[0].action.clone().expect("Undo");
        let user = root.join("config/hyprland-portals.conf");
        assert!(
            std::fs::read_to_string(&user)
                .unwrap()
                .contains("default=hyprland;kde")
        );
        until("named", || list()["value"] == "skwir");

        // Undo names what the row keeps, never the file nor its text.
        assert_eq!(undo.command.args, serde_json::json!({ "done": 1 }));
        app.dispatch(&undo.command).unwrap();
        until("put back", || !user.exists());
        until("kde again", || list()["value"] == "kde");
        let said = |app: &App| {
            let mut said = Vec::new();
            until("said", || {
                said.extend(app.take_notifications());
                !said.is_empty()
            });
            said[0].text.english()
        };
        assert_eq!(
            said(&app),
            "What answered other programs' Open and Save dialogs is back, from the desktop portal's next start"
        );
        app.dispatch(&undo.command).unwrap();
        assert_eq!(
            said(&app),
            "That choice of what answers Open and Save dialogs was undone already"
        );

        // The desktop's choice where the user has no file: nothing to take
        // out, said in words of its own.
        app.dispatch(&Invocation::with_args(
            SET,
            &serde_json::json!({ "portal": "" }),
        ))
        .unwrap();
        assert_eq!(
            said(&app),
            "The desktop already chooses what answers other programs' Open and Save dialogs"
        );
        assert!(!user.exists());

        // The configuration's own none: shown as none, chosen by nothing.
        std::fs::create_dir_all(root.join("config")).unwrap();
        std::fs::write(
            &user,
            "[preferred]\norg.freedesktop.impl.portal.FileChooser=none\n",
        )
        .unwrap();
        until("none", || list()["value"] == "none");
        assert_eq!(
            list()["label"]["english"],
            "None: the desktop portal shows no dialog"
        );
    }
}
