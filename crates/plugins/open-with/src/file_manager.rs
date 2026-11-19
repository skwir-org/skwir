//! The default file manager in Settings (ADR-140): a row of General ›
//! Startup choosing what opens folders for every program among the
//! applications that do (`open_with.set_default`, whose notice offers
//! Undo), and saying what that means for other programs' Show in Folder.
//! The choice is the desktop's own (`mimeapps.list`), kept by this plugin
//! for the dialog (`SettingSpec::choice_kept_by`): nothing is saved here.
//!
//! Listed on the window's thread, from memory: the catalog last read
//! (read again on a worker when it is stale), and who shows files for
//! other programs as last asked of the bus, asked again on a worker each
//! time the row is listed; each answer that moves what the row says tells
//! the dialog to list it again.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use parking_lot::Mutex;
use skwir_core::App;
use skwir_core::command::{NoArgs, Outcome, Topic};
use skwir_core::scheduler::Priority;
use skwir_core::settings::{ChoiceSource, SettingSpec};
use skwir_core::words;
use skwir_core::words::Words;
use skwir_platform::apps::Catalog;
use skwir_platform::file_manager::{FOLDERS, Shower};

use super::{Cached, ID};

/// The setting: a choice its owner keeps.
pub(crate) const KEY: &str = "open-with.file_manager";
/// The command that says it.
const LIST: &str = "open_with.file_manager";

/// What the row is made of, beside the catalog.
#[derive(Debug)]
pub(crate) struct Row {
    /// Moved when what the row says may have changed: the dialog lists it
    /// again.
    pub(crate) source: ChoiceSource,
    /// Who shows files for other programs, as last asked.
    shower: Mutex<Option<Shower>>,
    /// A worker asks it now.
    asking: AtomicBool,
    /// Asked again meanwhile: asked once more after.
    again: AtomicBool,
    /// Its holder followed, while it is another program: what the row
    /// says then changes when it ends, the dialog open or not.
    following: Mutex<Option<skwir_core::CancelToken>>,
}

impl Default for Row {
    fn default() -> Self {
        Row {
            source: ChoiceSource::new(LIST),
            shower: Mutex::default(),
            asking: AtomicBool::default(),
            again: AtomicBool::default(),
            following: Mutex::default(),
        }
    }
}

impl Row {
    /// What the plugin knew is let go of (disabled).
    pub(crate) fn forget(&self) {
        self.shower.lock().take();
        self.asking.store(false, Ordering::Release);
        self.again.store(false, Ordering::Release);
        if let Some(following) = self.following.lock().take() {
            following.cancel();
        }
    }
}

/// Skwir's own desktop entry.
fn own() -> String {
    format!("{}.desktop", skwir_core::APP_ID)
}

/// Declare the setting and the command that lists it. On Linux alone:
/// elsewhere Open With reads no applications, and no other program asks
/// Skwir to show files (ADR-140, 7).
pub(crate) fn register(app: &App, cache: &Cached) {
    if !cfg!(target_os = "linux") {
        return;
    }
    app.settings.declare(
        SettingSpec::choice_kept_by(
            KEY,
            words!(
                open_with_file_manager = "Default file manager",
                note = "A setting's title on the General page: the application that opens folders for every program."
            ),
            (
                words!(
                    open_with_file_manager_description = "Which application opens folders for every program: the same choice as Open With's Always use on a folder, and as the desktop's own settings.",
                    note = "Open With and Always use are what a menu and its check box are called."
                ),
                words!(
                    open_with_file_manager_description_more = "With Skwir chosen, another program's Show in Folder opens the folder in Skwir with its files selected, even when Skwir is not running.",
                    note = "What a setting's description says after its first sentence, in its tooltip. Show in Folder is what browsers call the command."
                ),
            ),
            cache.file_manager.source.clone(),
        )
        .page(skwir_core::settings::page::GENERAL)
        .section(skwir_core::settings::section::STARTUP)
        // It decides what other programs see (ADR-072, amended
        // 2026-11-19: in place of Ask what dropped files do).
        .essential(),
    );
    let cache = Arc::clone(cache);
    app.commands
        .register::<NoArgs, _>(
            ID,
            Topic::Files,
            LIST,
            words!(open_with_file_manager_list = "The default file manager"),
            words!(
                open_with_file_manager_list_description = "The application that opens folders for every program, as Settings lists it: the applications that open folders to choose from, each with the `open_with.set_default` that makes it the default, and whether Skwir shows the files other programs show in their folder. Answers from memory"
            ),
            move |_, app| Ok(Outcome::Value(list(app, &cache))),
        )
        .data();
}

/// The row's choice, from memory; what is not known yet is asked on a
/// worker.
fn list(app: &App, cache: &Cached) -> serde_json::Value {
    let catalog = cache.remembered();
    // Read again if an install or a default chosen elsewhere moved it.
    super::refresh(cache, app);
    let chosen = catalog
        .as_deref()
        .and_then(|catalog| catalog.default_for(FOLDERS))
        .is_some_and(|application| application.id == own());
    if chosen {
        ask_shower(app, cache);
    }
    let shower = cache.file_manager.shower.lock().clone();
    answer(catalog.as_deref(), shower.as_ref())
}

/// A window came to the front (`Plugin::on_looked_at`): what the row says
/// is asked again, as no watch tells it: the catalog, where its stamp
/// moved (a default chosen in the desktop's settings), and who shows files
/// for other programs, once the row asked it and while Skwir is chosen.
/// Each answer that moves the row has the dialog list it again.
pub(crate) fn looked_at(app: &App, cache: &Cached) {
    super::refresh(cache, app);
    let chosen = cache
        .remembered()
        .as_deref()
        .and_then(|catalog| catalog.default_for(FOLDERS))
        .is_some_and(|application| application.id == own());
    if chosen && cache.file_manager.shower.lock().is_some() {
        ask_shower(app, cache);
    }
}

/// Ask who shows files on a worker, once at a time; an answer that moves
/// what the row says has the dialog list it again. While another program
/// holds it, its holder is followed (`file_manager::follow_shower`), each
/// change asking again: the row says "until it ends", and sees it end.
fn ask_shower(app: &App, cache: &Cached) {
    let row = &cache.file_manager;
    if row.asking.swap(true, Ordering::AcqRel) {
        // The one under way may have read it before: once more after.
        row.again.store(true, Ordering::Release);
        return;
    }
    let token = cache
        .work
        .lock()
        .get_or_insert_with(|| app.lifetime().child())
        .clone();
    let (cache, wake, weak) = (Arc::clone(cache), app.waker_handle(), app.weak());
    let lifetime = token.clone();
    app.scheduler
        .spawn(Priority::Background, "open-with.shower", token, move || {
            let now = skwir_platform::file_manager::shower();
            let held = matches!(now, Shower::Program(_));
            let row = &cache.file_manager;
            let moved = row.shower.lock().replace(now.clone()) != Some(now);
            row.asking.store(false, Ordering::Release);
            {
                let mut following = row.following.lock();
                if held && following.is_none() && !lifetime.is_cancelled() {
                    let stop = lifetime.child();
                    let (weak, cache) = (weak.clone(), Arc::clone(&cache));
                    skwir_platform::file_manager::follow_shower(&stop, move || {
                        if let Some(app) = weak.upgrade() {
                            ask_shower(&app, &cache);
                        }
                    });
                    *following = Some(stop);
                } else if !held && let Some(stop) = following.take() {
                    stop.cancel();
                }
            }
            if moved {
                row.source.changed();
                wake();
            }
            if row.again.swap(false, Ordering::AcqRel)
                && let Some(app) = weak.upgrade()
            {
                ask_shower(&app, &cache);
            }
        });
}

/// The row's answer (`ChoiceSource`): what opens folders, the choice of
/// what may, and what it means for other programs' Show in Folder. Its
/// words are data (`Words::to_data`): the dialog says them in the
/// window's language, an agent or a script reads their English.
pub(crate) fn answer(catalog: Option<&Catalog>, shower: Option<&Shower>) -> serde_json::Value {
    let Some(catalog) = catalog else {
        return serde_json::json!({
            "detail": words!(
                open_with_file_manager_reading = "Reading the applications that open folders…",
                note = "Said under the Default file manager setting while the desktop's applications are read."
            )
            .to_data(),
        });
    };
    let own = own();
    let offered = catalog.for_type(FOLDERS);
    let named = catalog.default_for(FOLDERS);
    let mut choices: Vec<serde_json::Value> = offered
        .iter()
        .map(
            |application| serde_json::json!({ "value": application.id, "label": application.name }),
        )
        .collect();
    let (value, label) = if let Some(application) = named {
        (application.id.clone(), serde_json::json!(application.name))
    } else {
        // None named: the desktop opens folders with the first that says
        // it does, which is no one's choice.
        let opens = catalog.default_for_lineage(&[skwir_platform::mime::Mime(FOLDERS.into())]);
        let label = match opens {
            Some(application) => words!(
                open_with_file_manager_unchosen_opens = "Not chosen ({ $app })",
                app = application.name.clone(),
                note = "The Default file manager setting's choice when the desktop names none: app is what opens folders meanwhile, as its desktop entry names it."
            ),
            None => words!(
                open_with_file_manager_unchosen = "Not chosen",
                note = "The Default file manager setting's choice when the desktop names none, and nothing opens folders."
            ),
        }
        .to_data();
        choices.insert(
            0,
            serde_json::json!({ "value": "", "label": label, "disabled": true }),
        );
        (String::new(), label)
    };
    let detail = if !offered.iter().any(|application| application.id == own) {
        words!(
            open_with_file_manager_not_installed =
                "Skwir cannot be chosen here: its desktop entry is not installed",
            note = "Said under the Default file manager setting where Skwir's package did not install its entry (a build run from its folder)."
        )
    } else if value != own {
        words!(
            open_with_file_manager_offer =
                "Chosen, Skwir also shows the files other programs show in their folder",
            note = "Said under the Default file manager setting while another application is chosen: what choosing Skwir gives."
        )
    } else if let Some(Shower::Program(started)) = shower {
        words!(
            open_with_file_manager_held = "{ $program } still shows the files other programs show in their folder, until it ends",
            program = program_name(catalog, started.as_deref()),
            note = "Said under the Default file manager setting with Skwir chosen, while another program (Dolphin's daemon) answers other programs and does not let go: program is its name."
        )
    } else {
        words!(
            open_with_file_manager_shows =
                "Skwir also shows the files other programs show in their folder",
            note = "Said under the Default file manager setting with Skwir chosen."
        )
    };
    serde_json::json!({
        "value": value,
        "label": label,
        "choices": choices,
        "command": "open_with.set_default",
        "args": { "mime": FOLDERS },
        "field": "app",
        "detail": detail.to_data(),
    })
}

/// A program by the path it was started as, as its desktop entry names
/// it where one runs a program of that name (Dolphin), else by its file's
/// name.
fn program_name(catalog: &Catalog, started: Option<&std::path::Path>) -> Words {
    let Some(name) = started.and_then(std::path::Path::file_name) else {
        return words!(
            open_with_file_manager_another = "Another program",
            note = "Said in place of a program's name, under the Default file manager setting, when it cannot be read."
        );
    };
    catalog
        .all()
        .iter()
        .find(|application| {
            application
                .command_lines(&[])
                .first()
                .and_then(|line| line.first())
                .and_then(|program| std::path::Path::new(program).file_name())
                == Some(name)
        })
        .map_or_else(
            || Words::as_written(name.to_string_lossy().into_owned()),
            |application| Words::as_written(application.name.clone()),
        )
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use skwir_core::app::AppConfig;
    use skwir_core::command::Invocation;

    /// Applications that open folders, and the lists naming `default`.
    fn folders(dir: &Path, skwir: bool, default: Option<&str>) -> (Vec<PathBuf>, Vec<PathBuf>) {
        let apps = dir.join("applications");
        std::fs::create_dir_all(&apps).unwrap();
        let entry = |id: &str, name: &str, exec: &str| {
            std::fs::write(
                apps.join(id),
                format!(
                    "[Desktop Entry]\nType=Application\nName={name}\nExec={exec}\nMimeType=inode/directory;\n"
                ),
            )
            .unwrap();
        };
        entry("org.kde.dolphin.desktop", "Dolphin", "dolphin %u");
        if skwir {
            entry("org.skwir.skwir.desktop", "skwir", "skwir %f");
        }
        let list = dir.join("mimeapps.list");
        let text = default.map_or_else(String::new, |id| {
            format!("[Default Applications]\ninode/directory={id};\n")
        });
        std::fs::write(&list, text).unwrap();
        (vec![apps], vec![list])
    }

    fn catalog(dir: &Path, skwir: bool, default: Option<&str>) -> Catalog {
        let (apps, lists) = folders(dir, skwir, default);
        Catalog::from_paths(&apps, &lists)
    }

    /// Words the answer carries, as an agent reads them: their English.
    fn english(words: &serde_json::Value) -> String {
        serde_json::from_value::<Words>(words.clone())
            .unwrap()
            .english()
    }

    fn said(choice: &serde_json::Value) -> (String, String, Vec<String>) {
        let values = choice["choices"]
            .as_array()
            .map(|choices| {
                choices
                    .iter()
                    .map(|c| c["value"].as_str().unwrap().to_owned())
                    .collect()
            })
            .unwrap_or_default();
        (
            choice["value"].as_str().unwrap_or_default().to_owned(),
            english(&choice["detail"]),
            values,
        )
    }

    /// What the row says as the choice and the bus stand: what choosing
    /// Skwir gives, that it shows files, that another program does until
    /// it ends (named as its entry names it), that it cannot be chosen
    /// where its entry is missing, and no one's choice when none is named.
    #[test]
    fn the_row_says_what_the_choice_means() {
        let tmp = tempfile::tempdir().unwrap();
        let dolphin = "org.kde.dolphin.desktop";
        let skwir = "org.skwir.skwir.desktop";

        let other = catalog(&tmp.path().join("other"), true, Some(dolphin));
        let (value, detail, values) = said(&answer(Some(&other), None));
        assert_eq!(value, dolphin);
        assert_eq!(
            detail,
            "Chosen, Skwir also shows the files other programs show in their folder"
        );
        assert_eq!(values, [dolphin, skwir], "the default first");

        let chosen = catalog(&tmp.path().join("chosen"), true, Some(skwir));
        let (value, detail, _) = said(&answer(Some(&chosen), Some(&Shower::Here)));
        assert_eq!(value, skwir);
        assert_eq!(
            detail,
            "Skwir also shows the files other programs show in their folder"
        );
        let held = Shower::Program(Some("/nix/store/x-dolphin/bin/dolphin".into()));
        let (_, detail, _) = said(&answer(Some(&chosen), Some(&held)));
        assert_eq!(
            detail,
            "Dolphin still shows the files other programs show in their folder, until it ends"
        );
        let unread = Shower::Program(None);
        let (_, detail, _) = said(&answer(Some(&chosen), Some(&unread)));
        assert!(detail.starts_with("Another program still shows"));

        let missing = catalog(&tmp.path().join("missing"), false, Some(dolphin));
        let (_, detail, values) = said(&answer(Some(&missing), None));
        assert_eq!(
            detail,
            "Skwir cannot be chosen here: its desktop entry is not installed"
        );
        assert_eq!(values, [dolphin]);

        let none = catalog(&tmp.path().join("none"), true, None);
        let unnamed = answer(Some(&none), None);
        let (value, _, values) = said(&unnamed);
        assert_eq!(value, "");
        assert_eq!(values.first().map(String::as_str), Some(""));
        assert!(unnamed["choices"][0]["disabled"].as_bool().unwrap());
        assert!(english(&unnamed["label"]).starts_with("Not chosen ("));
    }

    /// Its words travel as data: the dialog says them in the window's
    /// language (here the pseudo-language), while what an agent or a
    /// script reads of the answer is English (UA-1).
    #[test]
    fn the_row_answers_words_as_data() {
        let tmp = tempfile::tempdir().unwrap();
        let other = catalog(tmp.path(), true, Some("org.kde.dolphin.desktop"));
        let listed = answer(Some(&other), None);
        let detail: Words = serde_json::from_value(listed["detail"].clone()).unwrap();
        let shows = "Chosen, Skwir also shows the files other programs show in their folder";
        assert_eq!(detail.english(), shows);
        let pseudo = detail.say_in(&[skwir_core::words::PSEUDO.to_owned()]);
        assert_ne!(pseudo, shows, "said in the window's language");
        assert_eq!(
            listed["detail"]["english"].as_str(),
            Some(shows),
            "an agent reads English: {listed}"
        );
        let reading = answer(None, None);
        assert!(english(&reading["detail"]).starts_with("Reading"));
    }

    /// Listed from memory, then again once the catalog is read; chosen
    /// from the row, Skwir becomes the default, said with Undo, and the row
    /// moves with it; it is never applied as a preference.
    #[test]
    fn skwir_is_chosen_from_its_row() {
        let tmp = tempfile::tempdir().unwrap();
        let (apps, lists) = folders(tmp.path(), true, Some("org.kde.dolphin.desktop"));
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        let plugin = crate::OpenWithPlugin {
            catalog: Arc::new(crate::CatalogCache {
                sources: Some((apps, lists.clone())),
                ..crate::CatalogCache::default()
            }),
        };
        let row = Arc::clone(&plugin.catalog);
        app.plugins.add(Arc::new(plugin), &app);
        assert!(app.settings.schema().iter().any(|spec| spec.key == KEY));
        let listed = |app: &App| -> serde_json::Value {
            match app.dispatch(&Invocation::new(LIST)).unwrap() {
                Outcome::Value(value) => value,
                other => panic!("a value, not {other:?}"),
            }
        };
        let first = row.file_manager.source.revision();
        // Not read yet: said so, and read on a worker, which moves it.
        assert!(listed(&app)["choices"].is_null());
        assert!(english(&listed(&app)["detail"]).starts_with("Reading"));
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while row.file_manager.source.revision() == first {
            assert!(std::time::Instant::now() < deadline, "never read");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(listed(&app)["value"], "org.kde.dolphin.desktop");

        let before = row.file_manager.source.revision();
        let mut choose = Invocation::with_args("open_with.set_default", &listed(&app)["args"]);
        choose.args["app"] = serde_json::json!("org.skwir.skwir.desktop");
        app.dispatch(&choose).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let notice = loop {
            if let Some(n) = app.take_notifications().into_iter().next() {
                break n;
            }
            assert!(std::time::Instant::now() < deadline, "nothing was said");
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(notice.text.english(), "skwir now opens folders");
        assert!(!notice.actions.is_empty(), "with Undo");
        assert_ne!(row.file_manager.source.revision(), before);
        assert_eq!(listed(&app)["value"], "org.skwir.skwir.desktop");
        // The desktop's, never a preference of Skwir's.
        let apply = std::collections::BTreeMap::from([(KEY.to_owned(), "x".to_owned())]);
        assert!(app.settings.validate(&apply).is_err());
        assert!(
            std::fs::read_to_string(&lists[0])
                .unwrap()
                .contains("inode/directory=org.skwir.skwir.desktop;")
        );

        app.plugins.set_enabled(ID, false, &app);
        assert!(!app.settings.schema().iter().any(|spec| spec.key == KEY));
    }

    /// A default chosen elsewhere (the desktop's settings) while Settings
    /// is open is read once a window comes to the front, and the row says
    /// it (UA-5).
    #[test]
    fn a_default_chosen_elsewhere_is_said_once_looked_at() {
        let tmp = tempfile::tempdir().unwrap();
        let (apps, lists) = folders(tmp.path(), true, Some("org.skwir.skwir.desktop"));
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        let plugin = crate::OpenWithPlugin {
            catalog: Arc::new(crate::CatalogCache {
                sources: Some((apps, lists.clone())),
                ..crate::CatalogCache::default()
            }),
        };
        let row = Arc::clone(&plugin.catalog);
        app.plugins.add(Arc::new(plugin), &app);
        let listed = |app: &App| -> serde_json::Value {
            match app.dispatch(&Invocation::new(LIST)).unwrap() {
                Outcome::Value(value) => value,
                other => panic!("a value, not {other:?}"),
            }
        };
        let until = |what: &str, done: &dyn Fn() -> bool| {
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            while !done() {
                assert!(std::time::Instant::now() < deadline, "{what}");
                std::thread::sleep(Duration::from_millis(5));
            }
        };
        listed(&app);
        until("read", &|| {
            listed(&app)["value"] == "org.skwir.skwir.desktop"
        });
        // Dolphin chosen in the desktop's settings: a list written anew,
        // with another stamp.
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(
            &lists[0],
            "[Default Applications]\ninode/directory=org.kde.dolphin.desktop;\n",
        )
        .unwrap();
        let before = row.file_manager.source.revision();
        crate::file_manager::looked_at(&app, &row);
        until("listed again", &|| {
            row.file_manager.source.revision() != before
        });
        assert_eq!(listed(&app)["value"], "org.kde.dolphin.desktop");
    }
}
