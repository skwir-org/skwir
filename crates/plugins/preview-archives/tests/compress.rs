//! Compress (ADR-059) through the commands the menus run and the executor:
//! an archive of each format made of a folder, extracted back as it was;
//! named after what it holds; a name taken kept beside; links, a file's
//! other names, tags and companions kept; a pipe left out; a password
//! asked twice to make it and once to extract it; cancelled while the
//! helper works.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use skwir_core::app::{App, AppConfig, SecretQuestion};
use skwir_core::command::{Invocation, Outcome};
use skwir_core::op::clash::{Act, Answer, FileChoice, Policy};
use skwir_core::op::{ExecEvent, Plan};
use skwir_platform::archive::{self, Format};

/// The libarchive helper: `SKWIR_ARCHIVE_WORKER`, else the one built into
/// the target folder this test runs from (its binary sits in `deps`).
/// Without it the test says so and passes, unless `SKWIR_REQUIRE_WORKERS`
/// says the workers are built.
fn with_archive_worker() -> bool {
    let worker = std::env::var_os("SKWIR_ARCHIVE_WORKER")
        .map(PathBuf::from)
        .or_else(|| {
            let exe = std::env::current_exe().ok()?;
            let built = exe.parent()?.parent()?.join("skwir-archive-worker");
            built.is_file().then_some(built)
        });
    let Some(worker) = worker else {
        assert!(
            std::env::var_os("SKWIR_REQUIRE_WORKERS").is_none(),
            "skwir-archive-worker is not built"
        );
        eprintln!("skipped: skwir-archive-worker is not built");
        return false;
    };
    // nextest runs each test in a process of its own.
    // SAFETY: no other thread of this test process reads the environment.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("SKWIR_ARCHIVE_WORKER", &worker);
    }
    true
}

/// A folder on the home disk, where the trash and attributes work (never
/// tmpfs).
fn scratch() -> tempfile::TempDir {
    let base = dirs::home_dir().unwrap().join(".cache/skwir-tests");
    std::fs::create_dir_all(&base).unwrap();
    tempfile::Builder::new()
        .prefix("compress-")
        .tempdir_in(base)
        .unwrap()
}

fn app(root: &Path) -> Arc<App> {
    let app = App::new(AppConfig::ephemeral(root.join("state"))).unwrap();
    app.plugins
        .add(Arc::new(skwir_plugin_preview_archives::PreviewPlugin), &app);
    app
}

fn plan_of(app: &App, command: &str, args: &serde_json::Value) -> Plan {
    match app.dispatch(&Invocation::with_args(command, args)).unwrap() {
        Outcome::Plan(plan) => plan,
        other => panic!("expected a plan, got {other:?}"),
    }
}

fn applied(app: &App, plan: &Plan) {
    let result = app.executor.execute_blocking(plan, app.lifetime());
    assert!(result.error.is_none(), "{:?}", result.error);
}

/// `plan` applied as the user's one action is (its names taken settled,
/// asked about), as `answer` answers what it asks meanwhile; how it ended.
fn whole(app: &App, plan: Plan, answer: impl FnOnce(&App)) -> ExecEvent {
    let id = plan.id;
    let events = app.executor.events();
    app.apply_whole(plan);
    answer(app);
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let event = events.recv_deadline(deadline).expect("the plan finishes");
        if matches!(&event, ExecEvent::Finished { plan, .. } if *plan == id) {
            return event;
        }
    }
}

fn clashes_asked(app: &App) -> skwir_core::app::ClashQuestion {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(q) = app.take_clash_questions().pop() {
            return q;
        }
        assert!(Instant::now() < deadline, "never asked");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn secret_asked(app: &App) -> SecretQuestion {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(q) = app.take_secret_questions().pop() {
            return q;
        }
        assert!(Instant::now() < deadline, "never asked");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// What a folder holds, as sorted relative paths with each file's text
/// and each link's target.
fn tree(root: &Path) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut folders = vec![root.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            let name = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let meta = std::fs::symlink_metadata(&path).unwrap();
            if meta.is_symlink() {
                let target = std::fs::read_link(&path).unwrap();
                found.push((name, format!("-> {}", target.display())));
            } else if meta.is_dir() {
                folders.push(path);
                found.push((name, "/".into()));
            } else {
                found.push((name, std::fs::read_to_string(&path).unwrap()));
            }
        }
    }
    found.sort();
    found
}

/// A folder of synthetic names: nested, Unicode, an apostrophe, an empty
/// folder, and on Unix a link and a file of two names.
fn album(root: &Path) -> PathBuf {
    let album = root.join("Synthetic album l'été");
    std::fs::create_dir_all(album.join("2026/day one")).unwrap();
    std::fs::create_dir_all(album.join("empty")).unwrap();
    std::fs::write(album.join("top.txt"), "at the top").unwrap();
    std::fs::write(album.join("2026/day one/crème brûlée.txt"), "café").unwrap();
    std::fs::write(album.join("2026/notes.md"), "# notes\n").unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("top.txt", album.join("to the top")).unwrap();
        std::fs::hard_link(album.join("top.txt"), album.join("2026/also top.txt")).unwrap();
    }
    album
}

#[test]
fn a_folder_made_an_archive_of_each_format_is_extracted_back_as_it_was() {
    if !with_archive_worker() {
        return;
    }
    let tmp = scratch();
    let app = app(tmp.path());
    let album = album(tmp.path());
    let before = tree(&album);
    for format in Format::ALL {
        let made_in = tmp.path().join(format!("made {}", format.name()));
        std::fs::create_dir(&made_in).unwrap();
        let plan = plan_of(
            &app,
            "archive.compress",
            &json!({ "entries": [album], "dest": made_in, "format": format.name() }),
        );
        assert_eq!(plan.title, "compress “Synthetic album l'été”");
        applied(&app, &plan);
        let archive = made_in.join(format!("Synthetic album l'été{}", format.extension()));
        assert!(archive.is_file(), "{}", archive.display());
        assert_eq!(
            std::fs::read_dir(&made_in).unwrap().count(),
            1,
            "nothing staged is left"
        );
        // Again: what it makes is there.
        assert_eq!(
            app.executor.execute_blocking(&plan, app.lifetime()).applied,
            0,
            "{}",
            format.name()
        );

        let back = tmp.path().join(format!("back {}", format.name()));
        std::fs::create_dir(&back).unwrap();
        applied(
            &app,
            &plan_of(
                &app,
                "archive.extract",
                &json!({ "entries": [archive], "dest": back }),
            ),
        );
        let extracted = back.join("Synthetic album l'été");
        assert_eq!(tree(&extracted), before, "{}", format.name());
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let names = std::fs::metadata(extracted.join("top.txt"))
                .unwrap()
                .nlink();
            assert_eq!(
                names,
                if format.keeps_hard_links() { 2 } else { 1 },
                "{}: a file's other name",
                format.name()
            );
        }
    }

    // Undone to the trash, as a copy is; redone.
    let archive = tmp
        .path()
        .join("made tar")
        .join("Synthetic album l'été.tar");
    let undone = app.executor.undo_blocking(app.lifetime()).unwrap();
    assert!(undone.error.is_none(), "{:?}", undone.error);
    let undone = app.executor.undo_blocking(app.lifetime()).unwrap();
    assert!(undone.error.is_none(), "{:?}", undone.error);
    assert!(!archive.exists());
    let redone = app.executor.redo_blocking(app.lifetime()).unwrap();
    assert!(redone.error.is_none(), "{:?}", redone.error);
    assert!(archive.is_file());
}

#[test]
fn an_archive_is_named_after_what_it_holds_and_a_name_taken_is_kept_beside() {
    if !with_archive_worker() {
        return;
    }
    let tmp = scratch();
    let app = app(tmp.path());
    let holiday = tmp.path().join("Holiday");
    std::fs::create_dir(&holiday).unwrap();
    std::fs::write(holiday.join("report.pdf"), "a report").unwrap();
    std::fs::write(holiday.join("b.txt"), "b").unwrap();

    // One file: its own name, extracted as the file itself.
    let report = holiday.join("report.pdf");
    applied(
        &app,
        &plan_of(&app, "archive.compress", &json!({ "entries": [report] })),
    );
    let zipped = holiday.join("report.pdf.zip");
    assert!(zipped.is_file());
    let back = tmp.path().join("back");
    std::fs::create_dir(&back).unwrap();
    applied(
        &app,
        &plan_of(
            &app,
            "archive.extract",
            &json!({ "entries": [zipped], "dest": back }),
        ),
    );
    assert_eq!(
        std::fs::read_to_string(back.join("report.pdf")).unwrap(),
        "a report"
    );
    std::fs::remove_file(&zipped).unwrap();

    // Several: the folder holding them, beside them; as the menu's
    // Compress to ZIP asks, a name taken is kept beside, unasked.
    app.set_window_shown();
    let quick = |app: &App| {
        plan_of(
            app,
            "archive.compress",
            &json!({
                "entries": [holiday.join("report.pdf"), holiday.join("b.txt")],
                "on_conflict": { "files": "keep_both" },
            }),
        )
    };
    assert!(matches!(
        whole(&app, quick(&app), |_| {}),
        ExecEvent::Finished { error: None, .. }
    ));
    assert!(holiday.join("Holiday.zip").is_file());
    // The same again: made already, nothing to do.
    assert!(matches!(
        whole(&app, quick(&app), |_| {}),
        ExecEvent::Finished {
            error: None,
            applied: 0,
            ..
        }
    ));
    assert!(!holiday.join("Holiday (2).zip").exists());
    // Changed since: another archive, beside.
    std::fs::write(holiday.join("b.txt"), "b, changed").unwrap();
    assert!(matches!(
        whole(&app, quick(&app), |_| {}),
        ExecEvent::Finished { error: None, .. }
    ));
    assert!(holiday.join("Holiday (2).zip").is_file());

    // A name that says its format; one that says none, or two.
    let named = plan_of(
        &app,
        "archive.compress",
        &json!({ "entries": [holiday.join("b.txt")], "name": "kept.tar.zst" }),
    );
    applied(&app, &named);
    let listed = archive::list(&holiday.join("kept.tar.zst"), app.lifetime()).unwrap();
    assert_eq!(listed.members.len(), 1);
    assert_eq!(listed.members[0].name, "b.txt");
    for (args, said) in [
        (json!({ "name": "kept.rar" }), "says no format"),
        (json!({ "name": "kept.zip", "format": "7z" }), "says zip"),
        (json!({ "format": "rar" }), "no format here"),
        (
            json!({ "name": "kept.7z", "password": true }),
            "Only a ZIP archive",
        ),
    ] {
        let mut args = args;
        args["entries"] = json!([holiday.join("b.txt")]);
        let error = app
            .dispatch(&Invocation::with_args("archive.compress", &args))
            .unwrap_err();
        assert!(error.to_string().contains(said), "{error}");
    }
}

/// What is beside a file, as a test's companion rule says (`x.side`).
struct Aside;
impl skwir_core::companions::Companion for Aside {
    fn id(&self) -> &'static str {
        "aside"
    }
    fn of(&self, name: &std::ffi::OsStr) -> Vec<PathBuf> {
        let mut side = name.to_os_string();
        side.push(".side");
        vec![PathBuf::from(side)]
    }
    fn owner(&self, name: &std::ffi::OsStr) -> Option<std::ffi::OsString> {
        name.to_str()?
            .strip_suffix(".side")
            .map(std::ffi::OsString::from)
    }
}

#[test]
fn a_companion_goes_with_its_file_and_a_pipe_is_left_out() {
    if !with_archive_worker() {
        return;
    }
    let tmp = scratch();
    let app = app(tmp.path());
    app.companions.register("test", Arc::new(Aside));
    let folder = tmp.path().join("pictures");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join("x.jpg"), "picture").unwrap();
    std::fs::write(folder.join("x.jpg.side"), "about it").unwrap();
    applied(
        &app,
        &plan_of(
            &app,
            "archive.compress",
            &json!({ "entries": [folder.join("x.jpg")], "format": "tar.gz" }),
        ),
    );
    let listed = archive::list(&folder.join("x.jpg.tar.gz"), app.lifetime()).unwrap();
    let mut names: Vec<&str> = listed.members.iter().map(|m| m.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["x.jpg", "x.jpg.side"]);

    #[cfg(target_os = "linux")]
    {
        let pipe = folder.join("a pipe");
        let made = std::process::Command::new("mkfifo")
            .arg(&pipe)
            .status()
            .unwrap();
        assert!(made.success());
        // Opened, a pipe with no writer would wait for ever.
        applied(
            &app,
            &plan_of(
                &app,
                "archive.compress",
                &json!({ "entries": [folder], "name": "all.zip", "dest": tmp.path() }),
            ),
        );
        let listed = archive::list(&tmp.path().join("all.zip"), app.lifetime()).unwrap();
        let mut names: Vec<&str> = listed
            .members
            .iter()
            .map(|m| m.name.trim_end_matches('/'))
            .collect();
        names.sort_unstable();
        assert_eq!(
            names,
            [
                "pictures",
                "pictures/x.jpg",
                "pictures/x.jpg.side",
                "pictures/x.jpg.tar.gz"
            ]
        );
    }
}

/// Tags kept on a file travel in a tar and come back with it extracted.
#[cfg(target_os = "linux")]
#[test]
fn tags_kept_on_a_file_travel_in_a_tar() {
    if !with_archive_worker() {
        return;
    }
    let tmp = scratch();
    let app = app(tmp.path());
    let folder = tmp.path().join("tagged");
    std::fs::create_dir(&folder).unwrap();
    let file = folder.join("red.txt");
    std::fs::write(&file, "red").unwrap();
    if skwir_core::fs::xattr::set(&file, "user.xdg.tags", b"red,warm").is_err() {
        eprintln!("skipped: this volume keeps no attributes");
        return;
    }
    applied(
        &app,
        &plan_of(
            &app,
            "archive.compress",
            &json!({ "entries": [folder], "format": "tar.xz" }),
        ),
    );
    let back = tmp.path().join("back");
    std::fs::create_dir(&back).unwrap();
    applied(
        &app,
        &plan_of(
            &app,
            "archive.extract",
            &json!({ "entries": [tmp.path().join("tagged.tar.xz")], "dest": back }),
        ),
    );
    assert_eq!(
        skwir_core::fs::xattr::get(back.join("tagged/red.txt"), "user.xdg.tags").unwrap(),
        Some(b"red,warm".to_vec())
    );
}

#[test]
fn a_password_is_asked_twice_to_make_an_archive_and_once_to_extract_it() {
    if !with_archive_worker() {
        return;
    }
    let tmp = scratch();
    let app = app(tmp.path());
    let folder = tmp.path().join("private");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join("secret.txt"), "the secret").unwrap();
    app.set_window_shown();
    let made_it = whole(
        &app,
        plan_of(
            &app,
            "archive.compress",
            &json!({ "entries": [folder], "password": true }),
        ),
        |app| {
            // Two that differ: asked again from the start.
            let first = secret_asked(app);
            assert_eq!(first.title, "A password for “private.zip”");
            assert_eq!(first.accept, "Next");
            assert!(app.answer_secret(first.id, Some("pass word 1".into())));
            let again = secret_asked(app);
            assert_eq!(again.accept, "Compress");
            assert!(app.answer_secret(again.id, Some("another".into())));
            let first = secret_asked(app);
            assert!(
                first.title.starts_with("The two passwords differed"),
                "{}",
                first.title
            );
            assert!(app.answer_secret(first.id, Some("pass word 1".into())));
            let again = secret_asked(app);
            assert!(app.answer_secret(again.id, Some("pass word 1".into())));
        },
    );
    assert!(matches!(made_it, ExecEvent::Finished { error: None, .. }));
    let made = tmp.path().join("private.zip");
    let listed = archive::list(&made, app.lifetime()).unwrap();
    assert!(
        listed
            .members
            .iter()
            .filter(|m| m.regular)
            .all(|m| m.encrypted),
        "{:?}",
        listed.members
    );

    // Extracted: asked, a wrong one asked again, then the right one.
    let into = tmp.path().join("into");
    std::fs::create_dir(&into).unwrap();
    let extract = || {
        plan_of(
            &app,
            "archive.extract",
            &json!({ "entries": [made], "dest": into }),
        )
    };
    let extracted = whole(&app, extract(), |app| {
        let asked = secret_asked(app);
        assert_eq!(asked.title, "Password for “private.zip”");
        assert_eq!(asked.accept, "Extract");
        assert!(app.answer_secret(asked.id, Some("wrong".into())));
        let asked = secret_asked(app);
        assert!(asked.title.starts_with("Wrong password"), "{}", asked.title);
        assert!(app.answer_secret(asked.id, Some("pass word 1".into())));
    });
    assert!(matches!(extracted, ExecEvent::Finished { error: None, .. }));
    assert_eq!(
        std::fs::read_to_string(into.join("private/secret.txt")).unwrap(),
        "the secret"
    );

    // Cancelled when asked: the plan is, and nothing is placed.
    let elsewhere = tmp.path().join("elsewhere");
    std::fs::create_dir(&elsewhere).unwrap();
    let declined = whole(
        &app,
        plan_of(
            &app,
            "archive.extract",
            &json!({ "entries": [made], "dest": elsewhere }),
        ),
        |app| {
            let asked = secret_asked(app);
            assert!(app.answer_secret(asked.id, None));
        },
    );
    assert!(matches!(
        declined,
        ExecEvent::Finished {
            cancelled: true,
            ..
        }
    ));
    assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 0);

    // With no window, no one is asked, and it says where one is.
    let headless = self::app(&tmp.path().join("headless"));
    let result = headless.executor.execute_blocking(
        &plan_of(
            &headless,
            "archive.extract",
            &json!({ "entries": [made], "dest": elsewhere }),
        ),
        headless.lifetime(),
    );
    let error = result.error.expect("not extracted");
    assert!(error.contains("asked in Skwir's window"), "{error}");
}

#[test]
fn an_extraction_onto_a_name_taken_asks_and_keeps_both_or_replaces() {
    if !with_archive_worker() {
        return;
    }
    let tmp = scratch();
    let app = app(tmp.path());
    app.set_window_shown();
    let dest = tmp.path().join("into");
    std::fs::create_dir_all(dest.join("project-1.0")).unwrap();
    std::fs::write(dest.join("project-1.0/mine.txt"), "mine").unwrap();
    // An archive of one folder named as it is.
    let folder = tmp.path().join("project-1.0");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join("README"), "read me").unwrap();
    applied(
        &app,
        &plan_of(&app, "archive.compress", &json!({ "entries": [folder] })),
    );
    let archive = tmp.path().join("project-1.0.zip");
    let extract = || {
        plan_of(
            &app,
            "archive.extract",
            &json!({ "entries": [archive], "dest": dest }),
        )
    };

    let kept = whole(&app, extract(), |app| {
        let question = clashes_asked(app);
        assert_eq!(question.clashes.len(), 1);
        let clash = &question.clashes[0];
        assert_eq!(
            clash.act,
            Act::Make {
                verb: "Extract",
                done: "extracted"
            }
        );
        assert!(!clash.folders(), "what is made is never merged");
        assert_eq!(clash.there.path, dest.join("project-1.0"));
        assert!(app.answer_clashes(question.id, Some(Answer::all(Policy::default()))));
    });
    assert!(matches!(kept, ExecEvent::Finished { error: None, .. }));
    assert_eq!(
        std::fs::read_to_string(dest.join("project-1.0 (2)/README")).unwrap(),
        "read me",
        "kept beside, its one folder unwrapped"
    );
    assert!(dest.join("project-1.0/mine.txt").is_file());

    // Replaced: what was there goes to the trash.
    std::fs::remove_dir_all(dest.join("project-1.0 (2)")).unwrap();
    let replaced = whole(&app, extract(), |app| {
        let question = clashes_asked(app);
        let replace = Answer::all(Policy {
            files: FileChoice::Replace,
            ..Policy::default()
        });
        assert!(app.answer_clashes(question.id, Some(replace)));
    });
    assert!(
        matches!(replaced, ExecEvent::Finished { error: None, .. }),
        "{replaced:?}"
    );
    assert!(dest.join("project-1.0/README").is_file());
    assert!(!dest.join("project-1.0/mine.txt").exists());
}

#[test]
fn making_an_archive_waits_while_held_and_stops_when_cancelled() {
    if !with_archive_worker() {
        return;
    }
    let tmp = scratch();
    let big = tmp.path().join("big.bin");
    std::fs::write(&big, vec![7u8; 4 << 20]).unwrap();
    let parts = vec![archive::Part {
        name: b"big.bin".to_vec(),
        item: archive::Item::File {
            path: big.clone(),
            size: 4 << 20,
        },
        mode: 0o644,
        modified: None,
        attributes: Vec::new(),
    }];
    let into = tmp.path().join("made");
    std::fs::create_dir(&into).unwrap();

    // Held until a moment has passed: nothing more is handed meanwhile,
    // and it ends whole once let go.
    let started = Instant::now();
    let let_go = Duration::from_millis(400);
    let mut sent = 0u64;
    let mut sent_while_held = 0u64;
    archive::create(
        &parts,
        Format::TarGz,
        None,
        &into.join("archive"),
        &skwir_core::CancelToken::new(),
        |n| {
            sent += n;
            if started.elapsed() < let_go {
                sent_while_held = sent;
            }
        },
        || started.elapsed() < let_go,
    )
    .unwrap();
    assert!(
        sent_while_held < 4 << 20,
        "{sent_while_held} handed while held"
    );
    assert_eq!(sent, 4 << 20);
    assert!(started.elapsed() >= let_go);
    let listed = archive::list(&into.join("archive"), &skwir_core::CancelToken::new()).unwrap();
    assert_eq!(listed.members[0].size, 4 << 20);

    let cancelled = skwir_core::CancelToken::new();
    cancelled.cancel();
    let error = archive::create(
        &parts,
        Format::Zip,
        None,
        &into.join("again"),
        &cancelled,
        |_| {},
        || false,
    )
    .unwrap_err();
    assert!(error.to_string().contains("cancelled"), "{error}");
}

#[test]
fn compress_is_offered_for_files_and_folders_in_their_menu() {
    let tmp = scratch();
    let app = app(tmp.path());
    let items: Vec<_> = app
        .registry
        .menus
        .all()
        .into_iter()
        .filter(|item| item.command.name.starts_with("archive.compress"))
        .collect();
    let titles: Vec<&str> = items.iter().map(|item| item.title.as_str()).collect();
    assert_eq!(
        titles,
        ["Compress to ZIP", "Compress…", "Compress with a Password…"]
    );
    for item in &items {
        assert_eq!(item.menu, "context");
        assert_eq!(item.when.as_deref(), Some("selection !place !restores"));
    }
    // Withdrawn with its plugin.
    app.unregister_owner("preview-archives");
    assert!(
        !app.registry
            .menus
            .all()
            .iter()
            .any(|item| item.command.name.starts_with("archive.compress"))
    );
}
