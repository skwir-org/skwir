//! Extract Here (ADR-048) through the command the menus run and the
//! executor: what an archive holds made a folder, never outside it, taken
//! back to the trash, paused and cancelled while the helper works.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::json;
use skwir_core::app::{App, AppConfig};
use skwir_core::command::{Invocation, Outcome};
use skwir_core::op::{Change, ExecEvent, Plan, StepStatus, Tally};

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

/// A folder on the home disk, where the trash works (never tmpfs).
fn scratch() -> tempfile::TempDir {
    let base = dirs::home_dir().unwrap().join(".cache/skwir-tests");
    std::fs::create_dir_all(&base).unwrap();
    tempfile::Builder::new()
        .prefix("extract-")
        .tempdir_in(base)
        .unwrap()
}

fn app(root: &Path) -> Arc<App> {
    let app = App::new(AppConfig::ephemeral(root.join("state"))).unwrap();
    app.plugins
        .add(Arc::new(skwir_plugin_archives::PreviewPlugin), &app);
    app
}

/// A zip of `entries`, a name ending in `/` a folder; one ending in `/!`
/// a read-only folder.
fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
    let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    for (name, body) in entries {
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        if let Some(folder) = name.strip_suffix("/!") {
            zip.add_directory(folder, options.unix_permissions(0o555))
                .unwrap();
        } else if let Some(folder) = name.strip_suffix('/') {
            zip.add_directory(folder, options).unwrap();
        } else {
            zip.start_file(*name, options).unwrap();
            zip.write_all(body).unwrap();
        }
    }
    zip.finish().unwrap();
}

fn extract(app: &App, archive: &Path, dest: &Path) -> Plan {
    let args = json!({ "entries": [archive], "dest": dest });
    match app
        .dispatch(&Invocation::with_args("archive.extract", &args))
        .unwrap()
    {
        Outcome::Plan(plan) => plan,
        other => panic!("expected a plan, got {other:?}"),
    }
}

/// What a folder holds, as sorted relative paths with each file's text.
fn tree(root: &Path) -> Vec<(String, Option<String>)> {
    let mut found = Vec::new();
    let mut folders = vec![root.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            // Its parts joined by `/` on every system, as an archive
            // names them.
            let name = path
                .strip_prefix(root)
                .unwrap()
                .components()
                .map(|part| part.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            if path.is_dir() {
                folders.push(path);
                found.push((name, None));
            } else {
                found.push((name, Some(std::fs::read_to_string(&path).unwrap())));
            }
        }
    }
    found.sort();
    found
}

/// What a staging folder would leave in `dest`.
fn staged_in(dest: &Path) -> Vec<String> {
    std::fs::read_dir(dest)
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|name| name.starts_with(".skwir-"))
        .collect()
}

#[test]
fn an_archive_is_extracted_as_a_folder_again_undone_and_redone() {
    if !with_archive_worker() {
        return;
    }
    let tmp = scratch();
    let app = app(tmp.path());
    let dest = tmp.path().join("Extracted it's here");
    std::fs::create_dir(&dest).unwrap();
    let archive = tmp.path().join("Synthetic photos.zip");
    write_zip(
        &archive,
        &[
            ("top.txt", b"at the top"),
            ("album/", b""),
            ("album/2026/day one.txt", b"first"),
            (
                "Cr\u{e8}me br\u{fb}l\u{e9}e/r\u{e9}sum\u{e9}.txt",
                b"caf\xc3\xa9",
            ),
        ],
    );

    let plan = extract(&app, &archive, &dest);
    assert_eq!(plan.title, "extract “Synthetic photos.zip”");
    let result = app.executor.execute_blocking(&plan, app.lifetime());
    assert!(result.error.is_none(), "{:?}", result.error);
    let made = dest.join("Synthetic photos");
    assert_eq!(
        tree(&made),
        [
            ("Cr\u{e8}me br\u{fb}l\u{e9}e".into(), None),
            (
                "Cr\u{e8}me br\u{fb}l\u{e9}e/r\u{e9}sum\u{e9}.txt".into(),
                Some("caf\u{e9}".into())
            ),
            ("album".into(), None),
            ("album/2026".into(), None),
            ("album/2026/day one.txt".into(), Some("first".into())),
            ("top.txt".into(), Some("at the top".into())),
        ]
    );
    assert!(staged_in(&dest).is_empty(), "{:?}", staged_in(&dest));
    assert!(archive.is_file(), "the archive stays");

    // Again: what it makes is there.
    let again = app.executor.execute_blocking(&plan, app.lifetime());
    assert!(again.error.is_none(), "{:?}", again.error);
    assert_eq!(again.applied, 0);

    // Undone to the trash, as a copy is; redone from it. Wine's shell
    // trashes nothing: that needs Windows.
    if skwir_core::system::under_wine("trashing through the shell") {
        return;
    }
    let undone = app.executor.undo_blocking(app.lifetime()).unwrap();
    assert!(undone.error.is_none(), "{:?}", undone.error);
    assert!(!made.exists());
    let redone = app.executor.redo_blocking(app.lifetime()).unwrap();
    assert!(redone.error.is_none(), "{:?}", redone.error);
    assert!(made.join("top.txt").is_file());
}

#[test]
fn an_archive_of_one_folder_or_one_file_is_that_folder_or_file() {
    if !with_archive_worker() {
        return;
    }
    let tmp = scratch();
    let app = app(tmp.path());
    // Beside it, with no folder named; the folder named otherwise than
    // the archive, as a release's `v1.0.zip` names it.
    let folder = tmp.path().join("v1.0 l'été.zip");
    write_zip(
        &folder,
        &[
            ("project-1.0/", b""),
            ("project-1.0/README", b"read me"),
            ("project-1.0/src/main.rs", b"fn main() {}"),
        ],
    );
    let plan = match app
        .dispatch(&Invocation::with_args(
            "archive.extract",
            &json!({ "entries": [folder] }),
        ))
        .unwrap()
    {
        Outcome::Plan(plan) => plan,
        other => panic!("expected a plan, got {other:?}"),
    };
    // Its review says what it makes, by the member's name.
    let made = tmp.path().join("project-1.0");
    let (review, asked) = plan.review(&Arc::default());
    assert_eq!(asked, 0);
    assert!(
        matches!(
            &review.steps[0].0,
            Change::Create { path, directory: true, .. } if *path == made
        ),
        "{:?}",
        review.steps[0]
    );
    let result = app.executor.execute_blocking(&plan, app.lifetime());
    assert!(result.error.is_none(), "{:?}", result.error);
    assert_eq!(
        std::fs::read_to_string(made.join("README")).unwrap(),
        "read me"
    );
    assert!(made.join("src/main.rs").is_file());
    assert!(!made.join("project-1.0").exists(), "not twice");
    assert!(
        !tmp.path().join("v1.0 l'été").exists(),
        "no folder around it"
    );
    // Again: what it makes is there.
    assert_eq!(
        app.executor.execute_blocking(&plan, app.lifetime()).applied,
        0
    );
    // Undone to the trash, redone as it was (Wine's shell trashes
    // nothing: that needs Windows).
    if !skwir_core::system::under_wine("trashing through the shell") {
        let undone = app.executor.undo_blocking(app.lifetime()).unwrap();
        assert!(undone.error.is_none(), "{:?}", undone.error);
        assert!(!made.exists());
        let redone = app.executor.redo_blocking(app.lifetime()).unwrap();
        assert!(redone.error.is_none(), "{:?}", redone.error);
        assert!(made.join("README").is_file());
    }
    assert!(!tmp.path().join("v1.0 l'été").exists());

    // One file, whatever the archive is called, is that file.
    let dest = tmp.path().join("into");
    std::fs::create_dir(&dest).unwrap();
    let bundle = tmp.path().join("bundle.zip");
    write_zip(&bundle, &[("report l'été.pdf", b"a report")]);
    let result = app
        .executor
        .execute_blocking(&extract(&app, &bundle, &dest), app.lifetime());
    assert!(result.error.is_none(), "{:?}", result.error);
    assert_eq!(
        tree(&dest),
        [("report l'été.pdf".into(), Some("a report".into()))]
    );

    // A file only compressed is that file.
    let compressed = tmp.path().join("notes l’été.txt.gz");
    let mut gz = flate2::write::GzEncoder::new(
        std::fs::File::create(&compressed).unwrap(),
        flate2::Compression::default(),
    );
    gz.write_all(b"the notes\n").unwrap();
    gz.finish().unwrap();
    let result = app
        .executor
        .execute_blocking(&extract(&app, &compressed, &dest), app.lifetime());
    assert!(result.error.is_none(), "{:?}", result.error);
    assert_eq!(
        std::fs::read_to_string(dest.join("notes l’été.txt")).unwrap(),
        "the notes\n"
    );

    // A hidden member alone stays in a folder named after the archive,
    // in sight, never a dotfile dropped where it is extracted.
    let dotfiles = tmp.path().join("dotfiles.zip");
    write_zip(
        &dotfiles,
        &[(".config/", b""), (".config/app.conf", b"key = 1")],
    );
    let hidden = tmp.path().join("hidden");
    std::fs::create_dir(&hidden).unwrap();
    let result = app
        .executor
        .execute_blocking(&extract(&app, &dotfiles, &hidden), app.lifetime());
    assert!(result.error.is_none(), "{:?}", result.error);
    assert_eq!(
        tree(&hidden),
        [
            ("dotfiles".into(), None),
            ("dotfiles/.config".into(), None),
            ("dotfiles/.config/app.conf".into(), Some("key = 1".into())),
        ]
    );
}

#[test]
fn nothing_is_written_outside_and_a_name_taken_is_kept() {
    if !with_archive_worker() {
        return;
    }
    let tmp = scratch();
    let app = app(tmp.path());
    let dest = tmp.path().join("into");
    std::fs::create_dir(&dest).unwrap();
    let hostile = tmp.path().join("hostile.zip");
    // What it made before the refusal includes a read-only folder, which
    // is removed all the same.
    write_zip(
        &hostile,
        &[
            ("fine.txt", b"fine"),
            ("locked/!", b""),
            ("locked/inside.txt", b"inside"),
            ("../outside.txt", b"never there"),
        ],
    );
    let result = app
        .executor
        .execute_blocking(&extract(&app, &hostile, &dest), app.lifetime());
    let error = result.error.expect("refused");
    assert!(error.contains("outside.txt"), "{error}");
    assert!(!tmp.path().join("outside.txt").exists());
    assert!(!dest.join("outside.txt").exists());
    assert!(!dest.join("hostile").exists(), "nothing of it placed");
    assert!(staged_in(&dest).is_empty(), "{:?}", staged_in(&dest));

    // Another folder under the name: a conflict, never merged.
    let taken = tmp.path().join("taken.zip");
    write_zip(&taken, &[("new.txt", b"new"), ("also new.txt", b"also")]);
    std::fs::create_dir(dest.join("taken")).unwrap();
    std::fs::write(dest.join("taken/mine.txt"), "mine").unwrap();
    let plan = extract(&app, &taken, &dest);
    assert!(matches!(
        plan.diff(&std::sync::Arc::default()).steps[0].1,
        StepStatus::Conflict { .. }
    ));
    assert!(!dest.join("taken/new.txt").exists());

    // What is no archive is refused before any plan.
    let text = tmp.path().join("plain.txt");
    std::fs::write(&text, "text").unwrap();
    let refused = app
        .dispatch(&Invocation::with_args(
            "archive.extract",
            &json!({ "entries": [text], "dest": dest }),
        ))
        .unwrap_err();
    assert!(refused.to_string().contains("not an archive"), "{refused}");
}

/// Held, the helper is given no more work and stops within a few units;
/// let go, it finishes. Cancelled, it stops.
#[test]
fn a_paused_extraction_stops_and_goes_on_and_a_cancelled_one_ends() {
    if !with_archive_worker() {
        return;
    }
    let tmp = scratch();
    let archive = tmp.path().join("many.zip");
    let names: Vec<String> = (0..200).map(|i| format!("file {i:03}.txt")).collect();
    let entries: Vec<(&str, &[u8])> = names.iter().map(|n| (n.as_str(), &b"x"[..])).collect();
    write_zip(&archive, &entries);
    let into = tmp.path().join("made");
    std::fs::create_dir(&into).unwrap();

    // Held at its first report, mid-way, until the counter counts what
    // was made meanwhile and lets it go on.
    let held = Arc::new(AtomicBool::new(false));
    let let_go = Arc::new(AtomicBool::new(false));
    let (pause, ended, folder) = (Arc::clone(&held), Arc::clone(&let_go), into.clone());
    let counter = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(800));
        let made = std::fs::read_dir(&folder).map_or(0, Iterator::count);
        ended.store(true, Ordering::SeqCst);
        pause.store(false, Ordering::SeqCst);
        made
    });
    let cancel = skwir_core::CancelToken::new();
    skwir_platform::archive::extract_all(
        &archive,
        &into,
        &cancel,
        |read| {
            if read > 0 && !let_go.load(Ordering::SeqCst) {
                held.store(true, Ordering::SeqCst);
            }
        },
        || held.load(Ordering::SeqCst),
        |_| None,
    )
    .unwrap();
    let while_held = counter.join().unwrap();
    assert!(
        (1..100).contains(&while_held),
        "held mid-way, it stopped short of the end ({while_held} made)"
    );
    assert_eq!(std::fs::read_dir(&into).unwrap().count(), 200);

    let cancelled = skwir_core::CancelToken::new();
    cancelled.cancel();
    let again = tmp.path().join("again");
    std::fs::create_dir(&again).unwrap();
    let error = skwir_platform::archive::extract_all(
        &archive,
        &again,
        &cancelled,
        |_| {},
        || false,
        |_| None,
    )
    .unwrap_err();
    assert!(error.to_string().contains("cancelled"), "{error}");
}

#[test]
fn extract_here_is_offered_for_archives_in_their_menu_and_a_drops() {
    let tmp = scratch();
    let app = app(tmp.path());
    let items: Vec<_> = app
        .registry
        .menus
        .all()
        .into_iter()
        .filter(|item| &*item.command.name == "archive.extract")
        .collect();
    let menus: Vec<&str> = items.iter().map(|item| item.menu.as_str()).collect();
    assert_eq!(menus.len(), 2);
    assert!(menus.contains(&"context") && menus.contains(&"drop"));
    for item in &items {
        let when = item.when.as_deref().unwrap();
        assert!(when.contains("kind:archive"), "{when}");
    }
    // Withdrawn with its plugin.
    app.unregister_owner("archives");
    assert!(
        !app.registry
            .menus
            .all()
            .iter()
            .any(|item| &*item.command.name == "archive.extract")
    );
}

/// An extraction is done, as a copy is, only while what it made holds what
/// the archive lists and nothing else, each file at its size and time.
#[test]
fn an_extraction_is_done_only_while_it_is_as_extracted() {
    if !with_archive_worker() {
        return;
    }
    let tmp = scratch();
    let app = app(tmp.path());
    let dest = tmp.path().join("into");
    std::fs::create_dir(&dest).unwrap();
    let archive = tmp.path().join("settings.zip");
    write_zip(
        &archive,
        &[
            ("one.conf", b"value 1"),
            ("nested/", b""),
            ("nested/two.conf", b"two"),
        ],
    );
    let status = || {
        extract(&app, &archive, &dest).diff(&Arc::default()).steps[0]
            .1
            .clone()
    };
    let result = app
        .executor
        .execute_blocking(&extract(&app, &archive, &dest), app.lifetime());
    assert!(result.error.is_none(), "{:?}", result.error);
    assert_eq!(status(), StepStatus::AlreadyDone);

    // Something else in it: not what the extraction made.
    let made = dest.join("settings");
    std::fs::write(made.join("nested/mine.txt"), "mine").unwrap();
    assert!(matches!(status(), StepStatus::Conflict { .. }));
    std::fs::remove_file(made.join("nested/mine.txt")).unwrap();
    assert_eq!(status(), StepStatus::AlreadyDone);

    // A file changed in place, its size kept.
    std::fs::write(made.join("one.conf"), "value 2").unwrap();
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(60);
    std::fs::File::open(made.join("one.conf"))
        .unwrap()
        .set_modified(later)
        .unwrap();
    assert!(matches!(status(), StepStatus::Conflict { .. }));
}

/// Extracted again as the window applies it, its name settled, an archive
/// whose folder holds what it lists is found extracted, never said
/// extracted again: its run counts it found done, said in its step's
/// words, and nothing done.
#[test]
fn an_archive_extracted_again_is_found_extracted() {
    if !with_archive_worker() {
        return;
    }
    let tmp = scratch();
    let app = app(tmp.path());
    let dest = tmp.path().join("it's here");
    std::fs::create_dir(&dest).unwrap();
    let archive = tmp.path().join("it's photos.zip");
    write_zip(&archive, &[("a.txt", b"a"), ("b.txt", b"b")]);
    let tally = |plan: Plan| -> Tally {
        let id = plan.id;
        app.apply_whole(plan);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            match app.executor.events().recv_deadline(deadline).unwrap() {
                ExecEvent::Finished {
                    plan, error, tally, ..
                } if plan == id => {
                    assert!(error.is_none(), "{error:?}");
                    break *tally.expect("counted");
                }
                _ => {}
            }
        }
    };
    let first = tally(extract(&app, &archive, &dest));
    assert_eq!((first.done.count, first.already.count), (1, 0));
    let again = tally(extract(&app, &archive, &dest));
    assert_eq!((again.done.count, again.already.count), (0, 1));
    let Some(Change::Create {
        path,
        words: Some(words),
        ..
    }) = &again.already.first
    else {
        panic!("said by its step: {:?}", again.already.first);
    };
    assert_eq!(path, &dest.join("it's photos"));
    assert_eq!(
        words.already,
        "“it's photos.zip” was already extracted to it's here"
    );
}

/// `plan` applied as the user's one action is (its names taken settled,
/// asked about), as `answer` answers what it asks meanwhile; how it ended.
fn whole(app: &App, plan: Plan, answer: impl FnOnce(&App)) -> ExecEvent {
    let id = plan.id;
    let events = app.executor.events();
    app.apply_whole(plan);
    answer(app);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let event = events.recv_deadline(deadline).expect("the plan finishes");
        if matches!(&event, ExecEvent::Finished { plan, .. } if *plan == id) {
            return event;
        }
    }
}

fn clashes_asked(app: &App) -> skwir_core::app::ClashQuestion {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if let Some(q) = app.take_clash_questions().pop() {
            return q;
        }
        assert!(std::time::Instant::now() < deadline, "never asked");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// Extracted again into its folder, some of its files removed since, one
/// changed and one added: merged into it as a copy's folder is (ADR-059
/// amended), what was removed comes back, what is there the same is left
/// as it is, the file changed is asked about and kept beside, and the
/// user's own stays. Undone, only what it placed goes; redone, it is back.
#[test]
fn what_was_removed_comes_back_when_extracted_into_its_folder_again() {
    if !with_archive_worker() {
        return;
    }
    let tmp = scratch();
    let app = app(tmp.path());
    app.set_window_shown();
    let dest = tmp.path().join("it's here");
    std::fs::create_dir(&dest).unwrap();
    let archive = tmp.path().join("Synthetic photos.zip");
    write_zip(
        &archive,
        &[
            ("top.txt", b"at the top"),
            ("album/", b""),
            ("album/2026/day one.txt", b"first"),
            ("album/2026/day two.txt", b"second"),
            ("locked/!", b""),
            ("locked/inside.txt", b"inside"),
            ("notes/changed.txt", b"as made"),
        ],
    );
    let result = app
        .executor
        .execute_blocking(&extract(&app, &archive, &dest), app.lifetime());
    assert!(result.error.is_none(), "{:?}", result.error);
    let made = dest.join("Synthetic photos");
    let extracted = tree(&made);

    std::fs::remove_file(made.join("top.txt")).unwrap();
    std::fs::remove_file(made.join("album/2026/day two.txt")).unwrap();
    // The folder the archive made read-only, opened to remove from it.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let locked = made.join("locked");
        assert!(std::fs::metadata(&locked).unwrap().permissions().readonly());
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    std::fs::remove_file(made.join("locked/inside.txt")).unwrap();
    std::fs::write(made.join("notes/changed.txt"), "mine, since").unwrap();
    std::fs::write(made.join("mine.txt"), "mine").unwrap();
    let before = tree(&made);
    let kept = skwir_core::fs::stat::id_of(&made.join("album/2026/day one.txt"));

    let ended = whole(&app, extract(&app, &archive, &dest), |app| {
        let question = clashes_asked(app);
        // The folder, each folder in it that differs, and the file changed
        // since, each under the folder it is in: what is there the same,
        // or missing, is never asked about.
        let asked: Vec<_> = question
            .clashes
            .iter()
            .map(|c| {
                (
                    c.there.path.strip_prefix(&dest).unwrap().to_owned(),
                    c.inside,
                    c.folders(),
                )
            })
            .collect();
        let at = |p: &str| PathBuf::from("Synthetic photos").join(p);
        assert_eq!(
            asked,
            [
                (PathBuf::from("Synthetic photos"), None, true),
                (at("album"), Some(0), true),
                (at("album/2026"), Some(1), true),
                (at("locked"), Some(0), true),
                (at("notes"), Some(0), true),
                (at("notes/changed.txt"), Some(4), false),
            ]
        );
        // As Enter answers: folders merged, both files kept.
        assert!(app.answer_clashes(question.id, Some(skwir_core::op::clash::Answer::default())));
    });
    assert!(
        matches!(ended, ExecEvent::Finished { error: None, .. }),
        "{ended:?}"
    );
    let mut merged = extracted.clone();
    merged.push(("mine.txt".into(), Some("mine".into())));
    merged.push(("notes/changed (2).txt".into(), Some("as made".into())));
    for (name, text) in &mut merged {
        if name == "notes/changed.txt" {
            *text = Some("mine, since".into());
        }
    }
    merged.sort();
    assert_eq!(tree(&made), merged);
    assert_eq!(
        skwir_core::fs::stat::id_of(&made.join("album/2026/day one.txt")),
        kept,
        "what was there the same is left as it is"
    );
    assert!(staged_in(&dest).is_empty(), "{:?}", staged_in(&dest));

    // Undone to the trash, each item placed on its own; redone from it.
    // Wine's shell trashes nothing: that needs Windows.
    if skwir_core::system::under_wine("trashing through the shell") {
        return;
    }
    let undone = app.executor.undo_blocking(app.lifetime()).unwrap();
    assert!(undone.error.is_none(), "{:?}", undone.error);
    assert_eq!(tree(&made), before);
    let redone = app.executor.redo_blocking(app.lifetime()).unwrap();
    assert!(redone.error.is_none(), "{:?}", redone.error);
    assert_eq!(tree(&made), merged);
}

/// A file changed since, kept beside where the archive itself holds the
/// name it would be kept as (`a (2).jpg` beside `a.jpg`), removed since:
/// kept under the next name free, both the archive's files back.
#[test]
fn a_file_kept_beside_never_takes_a_name_the_archive_brings() {
    if !with_archive_worker() {
        return;
    }
    let tmp = scratch();
    let app = app(tmp.path());
    app.set_window_shown();
    let dest = tmp.path().join("it's here");
    std::fs::create_dir(&dest).unwrap();
    let archive = tmp.path().join("photos.zip");
    write_zip(
        &archive,
        &[
            ("a.jpg", b"the first"),
            ("a (2).jpg", b"the second"),
            ("b.txt", b"left"),
        ],
    );
    let result = app
        .executor
        .execute_blocking(&extract(&app, &archive, &dest), app.lifetime());
    assert!(result.error.is_none(), "{:?}", result.error);
    let made = dest.join("photos");
    std::fs::remove_file(made.join("a (2).jpg")).unwrap();
    std::fs::write(made.join("a.jpg"), "mine, since").unwrap();

    let ended = whole(&app, extract(&app, &archive, &dest), |app| {
        let question = clashes_asked(app);
        let asked: Vec<_> = question
            .clashes
            .iter()
            .map(|c| {
                c.there
                    .path
                    .strip_prefix(&made)
                    .unwrap_or(&c.there.path)
                    .to_owned()
            })
            .collect();
        assert_eq!(asked, [PathBuf::new(), PathBuf::from("a.jpg")]);
        // As Enter answers: the folder merged, the file kept beside.
        assert!(app.answer_clashes(question.id, Some(skwir_core::op::clash::Answer::default())));
    });
    assert!(
        matches!(ended, ExecEvent::Finished { error: None, .. }),
        "{ended:?}"
    );
    let text = |name: &str| Some(String::from(name));
    assert_eq!(
        tree(&made),
        [
            ("a (2).jpg".into(), text("the second")),
            ("a (3).jpg".into(), text("the first")),
            ("a.jpg".into(), text("mine, since")),
            ("b.txt".into(), text("left")),
        ]
    );
}

/// What a plan from elsewhere says an extraction merged places, two items
/// to one name, is refused before anything is extracted.
#[test]
fn a_merge_placing_two_items_under_one_name_is_refused() {
    if !with_archive_worker() {
        return;
    }
    let tmp = scratch();
    let app = app(tmp.path());
    let dest = tmp.path().join("into");
    std::fs::create_dir(&dest).unwrap();
    let archive = tmp.path().join("photos.zip");
    write_zip(&archive, &[("a.jpg", b"one"), ("b.jpg", b"two")]);
    let result = app
        .executor
        .execute_blocking(&extract(&app, &archive, &dest), app.lifetime());
    assert!(result.error.is_none(), "{:?}", result.error);
    let made = dest.join("photos");
    std::fs::remove_file(made.join("a.jpg")).unwrap();
    std::fs::remove_file(made.join("b.jpg")).unwrap();

    let mut data = extract(&app, &archive, &dest).to_data();
    let args = &mut data.steps[0].args;
    args["name"] = json!("photos");
    args["merge"] = json!([
        { "from": "a.jpg", "to": "it's one.jpg" },
        { "from": "b.jpg", "to": "it's one.jpg" },
    ]);
    let plan = Plan::from_data(&data, &app.ops).unwrap();
    let diff = plan.diff(&app.companions);
    assert!(
        matches!(&diff.steps[..], [(_, StepStatus::Conflict { reason })]
            if reason.contains("two items")),
        "{:?}",
        diff.steps
    );
    let refused = app.executor.execute_blocking(&plan, app.lifetime());
    assert!(refused.error.is_some());
    assert!(tree(&made).is_empty(), "{:?}", tree(&made));
}

/// An archive of one read-only folder is extracted as that folder: opened
/// to be moved out of the staging folder, its permissions given back.
#[cfg(unix)]
#[test]
fn an_archive_of_one_read_only_folder_is_placed_as_it_is() {
    use std::os::unix::fs::PermissionsExt;
    if !with_archive_worker() {
        return;
    }
    let tmp = scratch();
    let app = app(tmp.path());
    let dest = tmp.path().join("into");
    std::fs::create_dir(&dest).unwrap();
    let archive = tmp.path().join("v1.zip");
    write_zip(
        &archive,
        &[("sealed/!", b""), ("sealed/inside.txt", b"inside")],
    );
    let result = app
        .executor
        .execute_blocking(&extract(&app, &archive, &dest), app.lifetime());
    assert!(result.error.is_none(), "{:?}", result.error);
    let sealed = dest.join("sealed");
    assert_eq!(
        std::fs::read_to_string(sealed.join("inside.txt")).unwrap(),
        "inside"
    );
    let mode = std::fs::metadata(&sealed).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o555, "{mode:o}");
    std::fs::set_permissions(&sealed, std::fs::Permissions::from_mode(0o755)).unwrap();
}
