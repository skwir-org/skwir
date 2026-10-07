//! Operations under failure: a destination that refuses, a copy that
//! fails partway, a source that cannot all be removed, a name too long.
//! Whatever fails, no file is lost, nothing half made is left under a
//! name of its own, and what was applied can still be undone.
#![cfg(unix)]

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use bibli_core::op::{Copy, Delete, EntryRef, Move, Operation, Plan, Rename};
use common::*;

fn set_mode(path: &Path, mode: u32) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("chmod");
}

/// Whether the system holds a folder of mode 0o555 to it: root writes
/// there all the same, and then these tests have nothing to show.
fn refuses_writes(dir: &Path) -> bool {
    let probe = dir.join(".probe");
    if std::fs::write(&probe, b"").is_ok() {
        let _ = std::fs::remove_file(probe);
        return false;
    }
    true
}

/// What a folder holds, sorted, hidden names (a staging folder) included.
fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// Gives a tree its modes back when the test ends, so that its temporary
/// folder can be removed.
struct Writable(PathBuf);

impl Drop for Writable {
    fn drop(&mut self) {
        fn walk(path: &Path) {
            if path.is_dir() {
                set_mode(path, 0o755);
                if let Ok(rd) = std::fs::read_dir(path) {
                    for e in rd.flatten() {
                        walk(&e.path());
                    }
                }
            } else if path.exists() {
                set_mode(path, 0o644);
            }
        }
        walk(&self.0);
    }
}

fn copy(entry: &Path, dest_dir: &Path) -> Box<dyn Operation> {
    Box::new(Copy {
        entry: EntryRef::from_path(entry).expect("entry"),
        dest_dir: dest_dir.to_path_buf(),
        new_name: None,
        preserve_attributes: true,
    })
}

#[test]
fn a_copy_into_a_folder_that_refuses_writes_changes_nothing() {
    let tmp = home_tempdir();
    let root = tmp.path();
    let _writable = Writable(root.to_path_buf());
    std::fs::write(root.join("a.txt"), b"a").unwrap();
    std::fs::create_dir(root.join("locked")).unwrap();
    set_mode(&root.join("locked"), 0o555);
    if !refuses_writes(&root.join("locked")) {
        eprintln!("skipped: this user writes in a read-only folder");
        return;
    }
    let app = app(root);
    let plan = Plan::with_steps(
        "copy",
        vec![copy(&root.join("a.txt"), &root.join("locked"))],
    );
    let r = app.executor.execute_blocking(&plan, app.lifetime());
    let error = r.error.expect("refused");
    assert!(error.contains("ermission denied"), "says why: {error}");
    assert_eq!(r.applied, 0);
    assert!(
        names(&root.join("locked")).is_empty(),
        "nothing staged left"
    );
    assert_eq!(std::fs::read(root.join("a.txt")).unwrap(), b"a");
    assert!(!app.executor.can_undo(), "nothing to undo");
}

#[test]
fn a_plan_stopped_by_a_failure_keeps_what_it_did_undoable() {
    let tmp = home_tempdir();
    let root = tmp.path();
    let _writable = Writable(root.to_path_buf());
    for name in ["a", "b", "c"] {
        std::fs::write(root.join(name), name).unwrap();
    }
    std::fs::create_dir(root.join("dest")).unwrap();
    std::fs::create_dir(root.join("locked")).unwrap();
    set_mode(&root.join("locked"), 0o555);
    if !refuses_writes(&root.join("locked")) {
        eprintln!("skipped: this user writes in a read-only folder");
        return;
    }
    let app = app(root);
    let plan = Plan::with_steps(
        "copy three",
        vec![
            copy(&root.join("a"), &root.join("dest")),
            copy(&root.join("b"), &root.join("locked")),
            copy(&root.join("c"), &root.join("dest")),
        ],
    );
    let r = app.executor.execute_blocking(&plan, app.lifetime());
    assert!(r.error.is_some());
    assert_eq!(r.applied, 1, "the first step, and none after the failure");
    assert_eq!(names(&root.join("dest")), ["a"]);
    let undone = app
        .executor
        .undo_blocking(app.lifetime())
        .expect("undoable");
    assert!(undone.error.is_none(), "{:?}", undone.error);
    assert!(names(&root.join("dest")).is_empty(), "the copy taken back");
    for name in ["a", "b", "c"] {
        assert_eq!(std::fs::read_to_string(root.join(name)).unwrap(), name);
    }
}

#[test]
fn a_folder_copy_that_fails_partway_leaves_no_part_of_itself() {
    let tmp = home_tempdir();
    let root = tmp.path();
    let _writable = Writable(root.to_path_buf());
    let src = root.join("album");
    std::fs::create_dir_all(src.join("inner")).unwrap();
    std::fs::write(src.join("first.txt"), b"first").unwrap();
    std::fs::write(src.join("inner/second.txt"), b"second").unwrap();
    std::fs::write(src.join("inner/unreadable.txt"), b"secret").unwrap();
    set_mode(&src.join("inner/unreadable.txt"), 0o000);
    if std::fs::read(src.join("inner/unreadable.txt")).is_ok() {
        eprintln!("skipped: this user reads a file of mode 000");
        return;
    }
    std::fs::create_dir(root.join("dest")).unwrap();
    let app = app(root);
    let plan = Plan::with_steps("copy", vec![copy(&src, &root.join("dest"))]);
    let r = app.executor.execute_blocking(&plan, app.lifetime());
    let error = r.error.expect("an unreadable file stops the copy");
    assert!(error.contains("unreadable.txt"), "names the file: {error}");
    assert!(
        names(&root.join("dest")).is_empty(),
        "neither the folder nor its staging is left"
    );
    assert_eq!(std::fs::read(src.join("first.txt")).unwrap(), b"first");
    assert!(!app.executor.can_undo());
}

/// A folder on another filesystem than `reference`, if the machine has
/// one (as `ops.rs` finds it).
fn other_device(reference: &Path) -> Option<tempfile::TempDir> {
    let here = bibli_core::fs::stat::id_of(reference)?.dev;
    ["/dev/shm", "/tmp"].into_iter().find_map(|candidate| {
        let there = bibli_core::fs::stat::id_of(Path::new(candidate))?.dev;
        (there != here)
            .then(|| tempfile::tempdir_in(candidate).ok())
            .flatten()
    })
}

#[test]
fn a_move_across_filesystems_that_cannot_remove_its_source_keeps_both_whole() {
    let tmp = home_tempdir();
    let root = tmp.path();
    let _writable = Writable(root.to_path_buf());
    let Some(there) = other_device(root) else {
        eprintln!("skipped: no second filesystem available");
        return;
    };
    let src = root.join("project");
    std::fs::create_dir_all(src.join("kept")).unwrap();
    std::fs::write(src.join("notes.txt"), b"notes").unwrap();
    std::fs::write(src.join("kept/data.txt"), b"data").unwrap();
    // Read, so copied; never emptied, so the source cannot all go.
    set_mode(&src.join("kept"), 0o555);
    if !refuses_writes(&src.join("kept")) {
        eprintln!("skipped: this user writes in a read-only folder");
        return;
    }
    let app = app(root);
    let plan = Plan::with_steps(
        "move",
        vec![Box::new(Move {
            entry: EntryRef::from_path(&src).unwrap(),
            dest_dir: there.path().to_path_buf(),
        })],
    );
    let r = app.executor.execute_blocking(&plan, app.lifetime());
    let moved = there.path().join("project");
    // Whatever is said, every file is somewhere, whole.
    let whole = |dir: &Path| {
        std::fs::read(dir.join("notes.txt")).is_ok_and(|b| b == b"notes")
            && std::fs::read(dir.join("kept/data.txt")).is_ok_and(|b| b == b"data")
    };
    assert!(whole(&moved), "the copy is whole: {:?}", r.error);
    assert!(
        std::fs::read(src.join("kept/data.txt")).is_ok_and(|b| b == b"data"),
        "what could not be removed is still there"
    );
    let error = r.error.expect("the source could not all be removed");
    assert!(
        error.contains("copied whole"),
        "says the copy is whole: {error}"
    );
    assert!(
        error.contains("“project/kept/data.txt” and 1 more are left")
            || error.contains("“project/notes.txt” and 1 more are left"),
        "names what is left: {error}"
    );
    set_mode(&src.join("kept"), 0o755);
    let _ = std::fs::remove_dir_all(&moved);
}

#[test]
fn a_folder_deleted_in_part_says_what_is_left() {
    let tmp = home_tempdir();
    let root = tmp.path();
    let _writable = Writable(root.to_path_buf());
    let src = root.join("old");
    std::fs::create_dir_all(src.join("kept")).unwrap();
    std::fs::write(src.join("kept/data.txt"), b"data").unwrap();
    set_mode(&src.join("kept"), 0o555);
    if !refuses_writes(&src.join("kept")) {
        eprintln!("skipped: this user writes in a read-only folder");
        return;
    }
    let app = app(root);
    let plan = Plan::with_steps(
        "delete",
        vec![Box::new(Delete {
            entry: EntryRef::from_path(&src).unwrap(),
        })],
    );
    let r = app.executor.execute_blocking(&plan, app.lifetime());
    let error = r.error.expect("refused");
    assert!(
        error.contains("“old” could not all be deleted: “old/kept/data.txt” is left"),
        "{error}"
    );
    assert!(src.join("kept/data.txt").exists());
}

#[test]
fn a_name_too_long_is_refused_before_anything_changes() {
    let tmp = home_tempdir();
    let app = app(tmp.path());
    let root = &tmp.path().join("files");
    std::fs::create_dir(root).unwrap();
    std::fs::write(root.join("a.txt"), b"a").unwrap();
    let long = format!("{}.txt", "é".repeat(200));
    let plan = Plan::with_steps(
        "rename",
        vec![Box::new(Rename {
            entry: EntryRef::from_path(root.join("a.txt")).unwrap(),
            new_name: long.into(),
        })],
    );
    let r = app.executor.execute_blocking(&plan, app.lifetime());
    let error = r.error.expect("too long for the filesystem");
    eprintln!("said: {error}");
    assert!(error.contains("too long"), "says why: {error}");
    assert_eq!(names(root), ["a.txt"]);
}
