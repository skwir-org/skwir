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
            let built = exe.parent()?.parent()?.join(format!(
                "skwir-archive-worker{}",
                std::env::consts::EXE_SUFFIX
            ));
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
        .add(Arc::new(skwir_plugin_archives::PreviewPlugin), &app);
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
        let extract = plan_of(
            &app,
            "archive.extract",
            &json!({ "entries": [archive], "dest": back }),
        );
        applied(&app, &extract);
        let extracted = back.join("Synthetic album l'été");
        assert_eq!(tree(&extracted), before, "{}", format.name());
        // Again: what it extracts is there, links and other names alike.
        assert_eq!(
            app.executor
                .execute_blocking(&extract, app.lifetime())
                .applied,
            0,
            "{}",
            format.name()
        );
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

    // Undone to the trash, as a copy is; redone. Wine's shell trashes
    // nothing: that needs Windows.
    if skwir_core::system::under_wine("trashing through the shell") {
        return;
    }
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
    // An archive of one folder, named otherwise (a release's `v1.0.zip`):
    // what it makes is that folder, whose name is taken.
    let folder = tmp.path().join("project-1.0");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join("README"), "read me").unwrap();
    applied(
        &app,
        &plan_of(&app, "archive.compress", &json!({ "entries": [folder] })),
    );
    let archive = tmp.path().join("v1.0.zip");
    std::fs::rename(tmp.path().join("project-1.0.zip"), &archive).unwrap();

    // Nothing there: settled under the folder's name, undone, and redone
    // as it was settled.
    let fresh = tmp.path().join("fresh");
    std::fs::create_dir(&fresh).unwrap();
    let ended = whole(
        &app,
        plan_of(
            &app,
            "archive.extract",
            &json!({ "entries": [archive], "dest": fresh }),
        ),
        |_| {},
    );
    assert!(
        matches!(ended, ExecEvent::Finished { error: None, .. }),
        "{ended:?}"
    );
    assert_eq!(
        std::fs::read_to_string(fresh.join("project-1.0/README")).unwrap(),
        "read me"
    );
    // Undone to the trash (Wine's shell trashes nothing: that needs
    // Windows).
    if !skwir_core::system::under_wine("trashing through the shell") {
        let undone = app.executor.undo_blocking(app.lifetime()).unwrap();
        assert!(undone.error.is_none(), "{:?}", undone.error);
        assert_eq!(std::fs::read_dir(&fresh).unwrap().count(), 0);
        let redone = app.executor.redo_blocking(app.lifetime()).unwrap();
        assert!(redone.error.is_none(), "{:?}", redone.error);
        assert!(fresh.join("project-1.0/README").is_file());
    }
    assert!(!fresh.join("v1.0").exists(), "no folder around it");

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
        // What comes is the folder it makes, not the archive it reads: no
        // size yet, as new as the archive.
        assert!(clash.source.folder, "{:?}", clash.source);
        assert_eq!(clash.source.size, None);
        assert_eq!(
            clash.source.modified,
            std::fs::metadata(&archive).unwrap().modified().ok()
        );
        assert!(app.answer_clashes(question.id, Some(Answer::all(Policy::default()))));
    });
    assert!(matches!(kept, ExecEvent::Finished { error: None, .. }));
    assert_eq!(
        std::fs::read_to_string(dest.join("project-1.0 (2)/README")).unwrap(),
        "read me",
        "kept beside, its one folder unwrapped"
    );
    assert!(dest.join("project-1.0/mine.txt").is_file());

    // Replaced: what was there goes to the trash, which Wine's shell
    // does not.
    if skwir_core::system::under_wine("trashing through the shell") {
        return;
    }
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
    assert!(!dest.join("v1.0").exists(), "no folder around it");
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
            id: None,
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
    assert_eq!(titles, ["To ZIP", "Choose Format…", "With a Password…"]);
    for item in &items {
        assert_eq!(item.menu, "context");
        assert_eq!(item.when.as_deref(), Some("selection !place !restores"));
        // One submenu, among what is made from the files (ADR-099).
        assert_eq!(item.submenu.as_deref(), Some("Compress"));
        assert_eq!(item.order / 100, 3);
    }
    // Withdrawn with its plugin.
    app.unregister_owner("archives");
    assert!(
        !app.registry
            .menus
            .all()
            .iter()
            .any(|item| item.command.name.starts_with("archive.compress"))
    );
}

/// `path`'s modification time moved `by` from now, later or earlier: a
/// file's or a folder's.
fn touch(path: &Path, later: bool, by: Duration) {
    let now = std::time::SystemTime::now();
    let at = if later { now + by } else { now - by };
    #[cfg(not(windows))]
    let held = std::fs::File::open(path).unwrap();
    // Windows sets a time through a handle allowed to write attributes,
    // and opens a folder only with backup semantics.
    #[cfg(windows)]
    let held = {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_WRITE_ATTRIBUTES: u32 = 0x0100;
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        std::fs::File::options()
            .access_mode(FILE_WRITE_ATTRIBUTES)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)
            .unwrap()
    };
    held.set_modified(at).unwrap();
}

/// A file's text changed, its size kept, its time later: what a copy and
/// an archive tell apart from the file they hold by its time alone.
fn edit_in_place(path: &Path, text: &str) {
    assert_eq!(std::fs::metadata(path).unwrap().len(), text.len() as u64);
    std::fs::write(path, text).unwrap();
    touch(path, true, Duration::from_secs(60));
}

/// The status the dry run gives the plan's first step.
fn first_status(plan: &Plan) -> skwir_core::op::StepStatus {
    plan.diff(&Arc::default()).steps[0].1.clone()
}

#[test]
fn an_archive_is_done_only_while_what_it_holds_is_unchanged() {
    if !with_archive_worker() {
        return;
    }
    let tmp = scratch();
    let app = app(tmp.path());
    app.set_window_shown();
    let notes = tmp.path().join("notes");
    std::fs::create_dir(&notes).unwrap();
    std::fs::write(notes.join("config.txt"), "value 1").unwrap();
    let again = |app: &App| {
        plan_of(
            app,
            "archive.compress",
            &json!({ "entries": [notes], "on_conflict": { "files": "keep_both" } }),
        )
    };
    // The dry run of the plan with nothing said of a name taken.
    let asked = |app: &App| plan_of(app, "archive.compress", &json!({ "entries": [notes] }));
    let made = whole(&app, again(&app), |_| {});
    assert!(matches!(made, ExecEvent::Finished { error: None, .. }));
    assert!(matches!(
        first_status(&asked(&app)),
        skwir_core::op::StepStatus::AlreadyDone
    ));

    // One digit changed, the size kept: not the archive's any more.
    edit_in_place(&notes.join("config.txt"), "value 2");
    assert!(matches!(
        first_status(&asked(&app)),
        skwir_core::op::StepStatus::Conflict { .. }
    ));
    let made = whole(&app, again(&app), |_| {});
    assert!(matches!(made, ExecEvent::Finished { error: None, .. }));
    let fresh = tmp.path().join("notes (2).zip");
    assert!(fresh.is_file(), "made again, beside");

    // Extracted, the second holds the change.
    let back = tmp.path().join("back");
    std::fs::create_dir(&back).unwrap();
    applied(
        &app,
        &plan_of(
            &app,
            "archive.extract",
            &json!({ "entries": [fresh], "dest": back }),
        ),
    );
    assert_eq!(
        // Its one folder, whatever the archive is called.
        std::fs::read_to_string(back.join("notes/config.txt")).unwrap(),
        "value 2"
    );
    assert!(!back.join("notes (2)").exists(), "no folder around it");
}

#[test]
fn replace_if_newer_reads_the_files_a_folder_holds() {
    if !with_archive_worker() {
        return;
    }
    let tmp = scratch();
    let app = app(tmp.path());
    app.set_window_shown();
    let deep = tmp.path().join("deep");
    std::fs::create_dir_all(deep.join("inner")).unwrap();
    std::fs::write(deep.join("inner/file.txt"), "first").unwrap();
    let plan = |app: &App| {
        plan_of(
            app,
            "archive.compress",
            &json!({ "entries": [deep], "on_conflict": { "files": "replace_if_newer" } }),
        )
    };
    assert!(matches!(
        whole(&app, plan(&app), |_| {}),
        ExecEvent::Finished { error: None, .. }
    ));
    let archive_path = tmp.path().join("deep.zip");
    // The folders older than the archive; a file deep inside changed
    // after it.
    edit_in_place(&deep.join("inner/file.txt"), "again");
    touch(&deep.join("inner"), false, Duration::from_secs(3600));
    touch(&deep, false, Duration::from_secs(3600));
    // The archive replaced goes to the trash, which Wine's shell does not
    // do: that needs Windows.
    if skwir_core::system::under_wine("trashing through the shell") {
        return;
    }
    let replaced = whole(&app, plan(&app), |_| {});
    assert!(
        matches!(replaced, ExecEvent::Finished { error: None, .. }),
        "{replaced:?}"
    );
    let into = tmp.path().join("into");
    std::fs::create_dir(&into).unwrap();
    applied(
        &app,
        &plan_of(
            &app,
            "archive.extract",
            &json!({ "entries": [archive_path], "dest": into }),
        ),
    );
    assert_eq!(
        std::fs::read_to_string(into.join("deep/inner/file.txt")).unwrap(),
        "again",
        "replaced: something it holds is newer"
    );
    assert!(!tmp.path().join("deep (2).zip").exists());

    // Nothing newer than the archive: kept as it is.
    touch(&archive_path, true, Duration::from_secs(7200));
    std::fs::write(deep.join("inner/file.txt"), "third, longer").unwrap();
    touch(
        &deep.join("inner/file.txt"),
        true,
        Duration::from_secs(3600),
    );
    let kept = whole(&app, plan(&app), |_| {});
    assert!(matches!(
        kept,
        ExecEvent::Finished {
            error: None,
            applied: 0,
            ..
        }
    ));
}

#[test]
fn a_name_taken_by_an_archive_made_of_a_folder_is_asked_about_the_archive() {
    if !with_archive_worker() {
        return;
    }
    let tmp = scratch();
    let app = app(tmp.path());
    app.set_window_shown();
    let deep = tmp.path().join("deep");
    std::fs::create_dir_all(deep.join("inner")).unwrap();
    std::fs::write(deep.join("inner/file.txt"), "first").unwrap();
    let compress = || plan_of(&app, "archive.compress", &json!({ "entries": [deep] }));
    applied(&app, &compress());
    edit_in_place(&deep.join("inner/file.txt"), "again");
    let skipped = whole(&app, compress(), |app| {
        let question = clashes_asked(app);
        // What comes is an archive, not the folder it is made of, as new
        // as the file deep in it.
        let source = &question.clashes[0].source;
        assert!(!source.folder && source.size.is_none(), "{source:?}");
        assert_eq!(
            source.modified,
            std::fs::metadata(deep.join("inner/file.txt"))
                .unwrap()
                .modified()
                .ok()
        );
        let skip = Answer::all(Policy {
            files: FileChoice::Skip,
            ..Policy::default()
        });
        assert!(app.answer_clashes(question.id, Some(skip)));
    });
    assert!(
        matches!(skipped, ExecEvent::Finished { error: None, .. }),
        "{skipped:?}"
    );
}

#[test]
fn replace_never_puts_in_the_trash_what_it_reads() {
    if !with_archive_worker() {
        return;
    }
    let tmp = scratch();
    let app = app(tmp.path());
    app.set_window_shown();
    let replace = json!({ "files": "replace" });

    // An archive made before, among what is compressed under its name.
    let holiday = tmp.path().join("Holiday");
    std::fs::create_dir(&holiday).unwrap();
    std::fs::write(holiday.join("a.txt"), "a").unwrap();
    std::fs::write(holiday.join("b.txt"), "b").unwrap();
    applied(
        &app,
        &plan_of(
            &app,
            "archive.compress",
            &json!({ "entries": [holiday.join("a.txt"), holiday.join("b.txt")] }),
        ),
    );
    let old = holiday.join("Holiday.zip");
    let before = std::fs::read(&old).unwrap();
    let ended = whole(
        &app,
        plan_of(
            &app,
            "archive.compress",
            &json!({
                "entries": [holiday.join("a.txt"), old],
                "on_conflict": replace,
            }),
        ),
        |_| {},
    );
    let ExecEvent::Finished { error, .. } = ended else {
        panic!("{ended:?}")
    };
    let error = error.expect("refused");
    assert!(error.contains("is among what it compresses"), "{error}");
    assert_eq!(std::fs::read(&old).unwrap(), before, "never trashed");

    // A folder holding the archive extracted under its name.
    let x = tmp.path().join("x");
    let photos = x.join("photos");
    std::fs::create_dir_all(&photos).unwrap();
    std::fs::write(photos.join("pic.txt"), "pic").unwrap();
    applied(
        &app,
        &plan_of(&app, "archive.compress", &json!({ "entries": [photos] })),
    );
    let inside = photos.join("photos.zip");
    std::fs::rename(x.join("photos.zip"), &inside).unwrap();
    std::fs::write(photos.join("pic.txt"), "pic, since").unwrap();
    let ended = whole(
        &app,
        plan_of(
            &app,
            "archive.extract",
            &json!({ "entries": [inside], "dest": x }),
        ),
        |app| {
            let question = clashes_asked(app);
            let replace = Answer::all(Policy {
                files: FileChoice::Replace,
                ..Policy::default()
            });
            assert!(app.answer_clashes(question.id, Some(replace)));
        },
    );
    let ExecEvent::Finished { error, .. } = ended else {
        panic!("{ended:?}")
    };
    let error = error.expect("refused");
    assert!(error.contains("holds “photos.zip”"), "{error}");
    assert!(inside.is_file(), "never trashed");
    assert_eq!(
        std::fs::read_to_string(photos.join("pic.txt")).unwrap(),
        "pic, since"
    );
}

#[test]
fn compress_asking_its_name_is_the_users_alone() {
    use skwir_core::command::Effect;
    let tmp = scratch();
    let app = app(tmp.path());
    let effect = |name: &str| app.commands.get(name).expect(name).effect;
    // A prompt that runs what is answered as the user's own: never an
    // agent's, whose folder the prompt would not show.
    assert_eq!(effect("archive.compress_ask"), Effect::UserOnly);
    // What an agent may run is proposed for review.
    assert_eq!(effect("archive.compress"), Effect::Plans);
    assert_eq!(effect("archive.extract"), Effect::Plans);
}

#[test]
fn a_one_file_archive_kept_beside_keeps_the_files_extension() {
    if !with_archive_worker() {
        return;
    }
    let tmp = scratch();
    let app = app(tmp.path());
    app.set_window_shown();
    let report = tmp.path().join("report.pdf");
    std::fs::write(&report, "the report").unwrap();
    applied(
        &app,
        &plan_of(&app, "archive.compress", &json!({ "entries": [report] })),
    );
    std::fs::write(&report, "the report, since").unwrap();
    let kept = whole(
        &app,
        plan_of(
            &app,
            "archive.extract",
            &json!({ "entries": [tmp.path().join("report.pdf.zip")] }),
        ),
        |app| {
            let question = clashes_asked(app);
            // The file it makes, of the size the archive lists: the
            // report as it was compressed, not as it is now.
            let source = &question.clashes[0].source;
            assert!(!source.folder, "{source:?}");
            assert_eq!(source.size, Some("the report".len() as u64));
            assert!(app.answer_clashes(question.id, Some(Answer::all(Policy::default()))));
        },
    );
    assert!(matches!(kept, ExecEvent::Finished { error: None, .. }));
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("report (2).pdf")).unwrap(),
        "the report"
    );
    assert!(!tmp.path().join("report.pdf (2)").exists());
}

#[test]
fn a_file_that_changes_as_it_is_read_is_not_archived() {
    if !with_archive_worker() {
        return;
    }
    let tmp = scratch();
    let file = tmp.path().join("growing.log");
    std::fs::write(&file, "line one\n").unwrap();
    let meta = std::fs::metadata(&file).unwrap();
    let id = skwir_core::fs::stat::id_of(&file);
    let part = |size: u64, id, modified| archive::Part {
        name: b"growing.log".to_vec(),
        item: archive::Item::File {
            path: file.clone(),
            size,
            id,
        },
        mode: 0o644,
        modified,
        attributes: Vec::new(),
    };
    let make = |part: archive::Part, name: &str| {
        let into = tmp.path().join(name);
        std::fs::create_dir(&into).unwrap();
        archive::create(
            &[part],
            Format::Tar,
            None,
            &into.join("archive"),
            &skwir_core::CancelToken::new(),
            |_| {},
            || false,
        )
    };
    let walked = meta.modified().ok();
    // As walked: made.
    make(part(meta.len(), id, walked), "as-walked").unwrap();
    // Longer than walked: never cut short in the archive.
    let error = make(part(meta.len() - 1, id, walked), "longer").unwrap_err();
    assert!(
        error.to_string().contains("changed as it was read"),
        "{error}"
    );
    // Another file at its name.
    let other = skwir_core::fs::stat::id_of(tmp.path());
    let error = make(part(meta.len(), other, walked), "another").unwrap_err();
    assert!(
        error.to_string().contains("changed as it was read"),
        "{error}"
    );
    // Written to since it was walked.
    let earlier = walked.map(|t| t - Duration::from_secs(5));
    let error = make(part(meta.len(), id, earlier), "written").unwrap_err();
    assert!(
        error.to_string().contains("changed as it was read"),
        "{error}"
    );
    // A link put at its name leads elsewhere: not the file walked.
    #[cfg(unix)]
    {
        let secret = tmp.path().join("elsewhere.txt");
        std::fs::write(&secret, "not to be archived").unwrap();
        std::fs::remove_file(&file).unwrap();
        std::os::unix::fs::symlink(&secret, &file).unwrap();
        let error = make(part(meta.len(), id, walked), "linked").unwrap_err();
        assert!(
            error.to_string().contains("changed as it was read"),
            "{error}"
        );
    }
}

#[test]
fn an_empty_password_is_asked_again() {
    if !with_archive_worker() {
        return;
    }
    let tmp = scratch();
    let app = app(tmp.path());
    let folder = tmp.path().join("locked");
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
            for _ in 0..2 {
                let asked = secret_asked(app);
                assert!(app.answer_secret(asked.id, Some("words".into())));
            }
        },
    );
    assert!(matches!(made_it, ExecEvent::Finished { error: None, .. }));
    let into = tmp.path().join("into");
    std::fs::create_dir(&into).unwrap();
    let extracted = whole(
        &app,
        plan_of(
            &app,
            "archive.extract",
            &json!({ "entries": [tmp.path().join("locked.zip")], "dest": into }),
        ),
        |app| {
            let asked = secret_asked(app);
            assert!(app.answer_secret(asked.id, Some(String::new().into())));
            let asked = secret_asked(app);
            assert!(
                asked.title.ends_with("it cannot be empty"),
                "{}",
                asked.title
            );
            assert!(app.answer_secret(asked.id, Some("words".into())));
        },
    );
    assert!(
        matches!(extracted, ExecEvent::Finished { error: None, .. }),
        "{extracted:?}"
    );
    assert_eq!(
        std::fs::read_to_string(into.join("locked/secret.txt")).unwrap(),
        "the secret"
    );
}

#[test]
fn room_is_asked_for_what_is_made_and_extracted() {
    if !with_archive_worker() {
        return;
    }
    let tmp = scratch();
    let app = app(tmp.path());
    app.set_window_shown();
    let folder = tmp.path().join("bulky");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join("a.bin"), vec![1u8; 3000]).unwrap();
    std::fs::write(folder.join("b.bin"), vec![2u8; 2000]).unwrap();
    applied(
        &app,
        &plan_of(&app, "archive.compress", &json!({ "entries": [folder] })),
    );
    // A drive with a byte free; the room asked is answered no.
    app.executor.set_free(Arc::new(|_| Some(1)));
    for (args, needs) in [
        (json!({ "entries": [folder], "name": "bulky.tar" }), 5000),
        (json!({ "entries": [folder], "name": "bulky.7z" }), 10_000),
        (
            json!({ "entries": [tmp.path().join("bulky.zip")], "dest": tmp.path().join("bulky") }),
            5000,
        ),
    ] {
        let command = if args["name"].is_null() {
            "archive.extract"
        } else {
            "archive.compress"
        };
        let ended = whole(&app, plan_of(&app, command, &args), |app| {
            let deadline = Instant::now() + Duration::from_secs(10);
            let question = loop {
                if let Some(q) = app.take_questions().pop() {
                    break q;
                }
                assert!(Instant::now() < deadline, "room never asked: {args}");
                std::thread::sleep(Duration::from_millis(5));
            };
            let needed = format!("{} needed", skwir_core::fs::size_said(needs));
            assert!(
                question.title.contains(&needed),
                "{}: {args}",
                question.title
            );
            assert!(app.answer(question.id, false));
        });
        let ExecEvent::Finished { error, .. } = ended else {
            panic!("{ended:?}")
        };
        let error = error.expect("short of room");
        assert!(error.contains("Not enough room"), "{error}");
    }
}
