//! Extract Here (ADR-048) through the command the menus run and the
//! executor: what an archive holds made a folder, never outside it, taken
//! back to the trash, paused and cancelled while the helper works.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use bibli_core::app::{App, AppConfig};
use bibli_core::command::{Invocation, Outcome};
use bibli_core::op::{Plan, StepStatus};
use serde_json::json;

/// The libarchive helper: `BIBLI_ARCHIVE_WORKER`, else the one built into
/// the target folder this test runs from (its binary sits in `deps`).
/// Without it the test says so and passes, unless `BIBLI_REQUIRE_WORKERS`
/// says the workers are built.
fn with_archive_worker() -> bool {
    let worker = std::env::var_os("BIBLI_ARCHIVE_WORKER")
        .map(PathBuf::from)
        .or_else(|| {
            let exe = std::env::current_exe().ok()?;
            let built = exe.parent()?.parent()?.join("bibli-archive-worker");
            built.is_file().then_some(built)
        });
    let Some(worker) = worker else {
        assert!(
            std::env::var_os("BIBLI_REQUIRE_WORKERS").is_none(),
            "bibli-archive-worker is not built"
        );
        eprintln!("skipped: bibli-archive-worker is not built");
        return false;
    };
    // nextest runs each test in a process of its own.
    // SAFETY: no other thread of this test process reads the environment.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("BIBLI_ARCHIVE_WORKER", &worker);
    }
    true
}

/// A folder on the home disk, where the trash works (never tmpfs).
fn scratch() -> tempfile::TempDir {
    let base = dirs::home_dir().unwrap().join(".cache/bibli-tests");
    std::fs::create_dir_all(&base).unwrap();
    tempfile::Builder::new()
        .prefix("extract-")
        .tempdir_in(base)
        .unwrap()
}

fn app(root: &Path) -> Arc<App> {
    let app = App::new(AppConfig::ephemeral(root.join("state"))).unwrap();
    app.plugins
        .add(Arc::new(bibli_plugin_preview_archives::PreviewPlugin), &app);
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
            let name = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
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
        .filter(|name| name.starts_with(".bibli-"))
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

    // Undone to the trash, as a copy is; redone from it.
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
    // Beside it, with no folder named.
    let folder = tmp.path().join("project-1.0.zip");
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
    let result = app.executor.execute_blocking(&plan, app.lifetime());
    assert!(result.error.is_none(), "{:?}", result.error);
    let made = tmp.path().join("project-1.0");
    assert_eq!(
        std::fs::read_to_string(made.join("README")).unwrap(),
        "read me"
    );
    assert!(made.join("src/main.rs").is_file());
    assert!(!made.join("project-1.0").exists(), "not twice");
    // Again: what it makes is there.
    assert_eq!(
        app.executor.execute_blocking(&plan, app.lifetime()).applied,
        0
    );

    // A file only compressed is that file.
    let compressed = tmp.path().join("notes l’été.txt.gz");
    let mut gz = flate2::write::GzEncoder::new(
        std::fs::File::create(&compressed).unwrap(),
        flate2::Compression::default(),
    );
    gz.write_all(b"the notes\n").unwrap();
    gz.finish().unwrap();
    let dest = tmp.path().join("into");
    std::fs::create_dir(&dest).unwrap();
    let result = app
        .executor
        .execute_blocking(&extract(&app, &compressed, &dest), app.lifetime());
    assert!(result.error.is_none(), "{:?}", result.error);
    assert_eq!(
        std::fs::read_to_string(dest.join("notes l’été.txt")).unwrap(),
        "the notes\n"
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
    write_zip(&taken, &[("new.txt", b"new")]);
    std::fs::create_dir(dest.join("taken")).unwrap();
    std::fs::write(dest.join("taken/mine.txt"), "mine").unwrap();
    let plan = extract(&app, &taken, &dest);
    assert!(matches!(
        plan.diff().steps[0].1,
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
    let cancel = bibli_core::CancelToken::new();
    bibli_platform::archive::extract_all(
        &archive,
        &into,
        &cancel,
        |read| {
            if read > 0 && !let_go.load(Ordering::SeqCst) {
                held.store(true, Ordering::SeqCst);
            }
        },
        || held.load(Ordering::SeqCst),
    )
    .unwrap();
    let while_held = counter.join().unwrap();
    assert!(
        (1..100).contains(&while_held),
        "held mid-way, it stopped short of the end ({while_held} made)"
    );
    assert_eq!(std::fs::read_dir(&into).unwrap().count(), 200);

    let cancelled = bibli_core::CancelToken::new();
    cancelled.cancel();
    let again = tmp.path().join("again");
    std::fs::create_dir(&again).unwrap();
    let error =
        bibli_platform::archive::extract_all(&archive, &again, &cancelled, |_| {}, || false)
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
    app.unregister_owner("preview-archives");
    assert!(
        !app.registry
            .menus
            .all()
            .iter()
            .any(|item| &*item.command.name == "archive.extract")
    );
}
