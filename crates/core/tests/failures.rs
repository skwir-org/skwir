//! Operations under failure: a destination that refuses, a copy that
//! fails partway, a source that cannot all be removed, a name too long.
//! Whatever fails, no file is lost, nothing half made is left under a
//! name of its own, and what was applied can still be undone.
#![cfg(unix)]

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use common::*;
use skwir_core::op::{Copy, Delete, EntryRef, Move, Operation, Plan, Rename};
use skwir_core::words::Words;

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
        Words::as_written("copy"),
        vec![copy(&root.join("a.txt"), &root.join("locked"))],
    );
    let r = app.executor.execute_blocking(&plan, app.lifetime());
    let error = r.error.expect("refused").english();
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
        Words::as_written("copy three"),
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
    let plan = Plan::with_steps(
        Words::as_written("copy"),
        vec![copy(&src, &root.join("dest"))],
    );
    let r = app.executor.execute_blocking(&plan, app.lifetime());
    let error = r
        .error
        .expect("an unreadable file stops the copy")
        .english();
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
    let here = skwir_core::fs::stat::id_of(reference)?.dev;
    ["/dev/shm", "/tmp"].into_iter().find_map(|candidate| {
        let there = skwir_core::fs::stat::id_of(Path::new(candidate))?.dev;
        (there != here)
            .then(|| tempfile::tempdir_in(candidate).ok())
            .flatten()
    })
}

#[test]
fn a_move_across_filesystems_that_cannot_remove_its_source_keeps_both_whole() {
    // Different filesystems enumerate these entries differently. Exercise
    // both creation orders without requiring either removal order.
    for notes_first in [false, true] {
        move_with_unremovable_source(notes_first);
    }
}

fn move_with_unremovable_source(notes_first: bool) {
    let tmp = home_tempdir();
    let root = tmp.path();
    let _writable = Writable(root.to_path_buf());
    let Some(there) = other_device(root) else {
        eprintln!("skipped: no second filesystem available");
        return;
    };
    let _destination_writable = Writable(there.path().to_path_buf());
    let src = root.join("project");
    std::fs::create_dir(&src).unwrap();
    let make_notes = || std::fs::write(src.join("notes.txt"), b"notes").unwrap();
    if notes_first {
        make_notes();
    }
    std::fs::create_dir(src.join("kept")).unwrap();
    if !notes_first {
        make_notes();
    }
    std::fs::write(src.join("kept/data.txt"), b"data").unwrap();
    // Read, so copied; never emptied, so the source cannot all go.
    set_mode(&src.join("kept"), 0o555);
    if !refuses_writes(&src.join("kept")) {
        eprintln!("skipped: this user writes in a read-only folder");
        return;
    }
    let original = snapshot(&src);
    let app = app(root);
    let plan = Plan::with_steps(
        Words::as_written("move"),
        vec![Box::new(Move {
            entry: EntryRef::from_path(&src).unwrap(),
            dest_dir: there.path().to_path_buf(),
            new_name: None,
        })],
    );
    let r = app.executor.execute_blocking(&plan, app.lifetime());
    let moved = there.path().join("project");
    assert_eq!(
        snapshot(&moved),
        original,
        "the copy is whole: {:?}",
        r.error
    );
    assert!(
        std::fs::read(src.join("kept/data.txt")).is_ok_and(|b| b == b"data"),
        "what could not be removed is still there"
    );
    let error = r
        .error
        .expect("the source could not all be removed")
        .english();
    assert!(
        error.contains("copied whole"),
        "says the copy is whole: {error}"
    );
    // Removal may reach the writable notes before the protected file.
    // Check the actual survivors, never a filesystem's traversal order.
    let remaining = snapshot(&src);
    assert!(
        remaining.iter().all(|entry| original.contains(entry)),
        "the source remainder is unchanged: {remaining:?}"
    );
    let files: Vec<_> = remaining
        .iter()
        .filter(|(_, state)| state != "<dir>")
        .collect();
    let suffix = match files.len() {
        1 => "is left".to_owned(),
        count => format!("and {} more are left", count - 1),
    };
    eprintln!(
        "writable entry created first: {notes_first}; {} source files remain",
        files.len()
    );
    assert!(
        files
            .iter()
            .any(|(path, _)| { error.contains(&format!("project/{} {suffix}", path.display())) }),
        "names and counts the actual files left: {error}"
    );
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
        Words::as_written("delete"),
        vec![Box::new(Delete {
            entry: EntryRef::from_path(&src).unwrap(),
        })],
    );
    let r = app.executor.execute_blocking(&plan, app.lifetime());
    let error = r.error.expect("refused").english();
    assert!(
        error.contains("“old” could not all be deleted: old/kept/data.txt is left"),
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
        Words::as_written("rename"),
        vec![Box::new(Rename {
            entry: EntryRef::from_path(root.join("a.txt")).unwrap(),
            new_name: long.into(),
        })],
    );
    let r = app.executor.execute_blocking(&plan, app.lifetime());
    let error = r.error.expect("too long for the filesystem").english();
    eprintln!("said: {error}");
    assert!(error.contains("too long"), "says why: {error}");
    assert_eq!(names(root), ["a.txt"]);
}

/// A file of `len` bytes at `path` that differ along its length (no hole,
/// no two MiB alike: each begins with its number), so a copy cut anywhere
/// reads as cut. Written a MiB at a time, a block made once.
// Only the FAT test reads it, Linux's.
#[cfg(target_os = "linux")]
fn varied_file(path: &Path, len: u64) {
    use std::io::Write;
    let mut x = 0x9e37_79b9_7f4a_7c15_u64;
    let mut block: Vec<u8> = (0..1 << 20)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect();
    let mut file = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
    let mut at = 0u64;
    while at < len {
        block[..8].copy_from_slice(&at.to_le_bytes());
        let n = usize::try_from((len - at).min(block.len() as u64)).unwrap();
        file.write_all(&block[..n]).unwrap();
        at += n as u64;
    }
    file.flush().unwrap();
}

/// What this process holds open at or below `dir`: what makes the kernel
/// refuse to unmount its volume.
#[cfg(target_os = "linux")]
fn held_under(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir("/proc/self/fd")
        .unwrap()
        .flatten()
        .filter_map(|fd| std::fs::read_link(fd.path()).ok())
        .filter(|target| target.starts_with(dir))
        .collect()
}

/// The end of the next plan `app` runs.
// Only the FAT test reads it, Linux's.
#[cfg(target_os = "linux")]
fn finished(app: &skwir_core::app::App) -> skwir_core::op::ExecEvent {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(600);
    loop {
        let event = app
            .executor
            .events()
            .recv_deadline(deadline)
            .expect("it ends");
        if matches!(event, skwir_core::op::ExecEvent::Finished { .. }) {
            return event;
        }
    }
}

/// On a FAT32 volume, which keeps no file of 4 GiB or more: a file that
/// large is refused before a byte of it is copied, alone (its step's
/// check) or in a folder (the plan's sizing), saying why, and nothing of
/// it is left there. A copy that fills the drive keeps what it wrote for
/// Retry, and lets go of it as the drive is let go of: nothing holds the
/// drive then, which unmounts.
///
/// Run on demand, on a FAT32 volume of the test's own (udisks mounts an
/// image without root), with 11 GB free in `~/.cache/skwir-tests`:
///
/// ```text
/// truncate -s 6G ~/.cache/skwir-tests/fat.img
/// mkfs.vfat -F 32 -n SKWIRCOPY ~/.cache/skwir-tests/fat.img
/// udisksctl loop-setup --no-user-interaction -f ~/.cache/skwir-tests/fat.img
/// udisksctl mount --no-user-interaction -b /dev/loopN
/// SKWIR_FAT32_DIR=/run/media/$USER/SKWIRCOPY cargo nextest run -p skwir-core \
///     --test failures --run-ignored only -E 'test(/fat32/)'
/// ```
///
/// then `udisksctl unmount -b /dev/loopN`, `udisksctl loop-delete -b
/// /dev/loopN`, and remove the image.
#[cfg(target_os = "linux")]
#[test]
#[ignore = "needs a FAT32 volume of its own: SKWIR_FAT32_DIR"]
fn on_fat32_a_file_too_large_is_refused_and_a_full_drive_let_go_of() {
    use skwir_core::op::ExecEvent;
    let fat = PathBuf::from(std::env::var_os("SKWIR_FAT32_DIR").expect("SKWIR_FAT32_DIR"));
    let tmp = home_tempdir();
    let films = tmp.path().join("films");
    std::fs::create_dir(&films).unwrap();
    let film = films.join("it's a film.mkv");
    varied_file(&film, (4 << 30) + (300 << 20));
    std::fs::write(films.join("notes.txt"), b"notes").unwrap();
    let app = app(tmp.path());

    // Alone: its check refuses it, at once.
    let dest = tempfile::tempdir_in(&fat).unwrap();
    let began = std::time::Instant::now();
    let r = app.executor.execute_blocking(
        &Plan::with_steps(Words::as_written("copy"), vec![copy(&film, dest.path())]),
        app.lifetime(),
    );
    let error = r.error.expect("refused").english();
    assert!(
        error.contains("is too large for") && error.contains("formatted as FAT"),
        "says why: {error}"
    );
    assert!(
        began.elapsed().as_secs() < 10,
        "refused before copying: {:?}",
        began.elapsed()
    );
    assert!(
        names(dest.path()).is_empty(),
        "nothing left: {:?}",
        names(dest.path())
    );
    assert!(
        held_under(&fat).is_empty(),
        "nothing held: {:?}",
        held_under(&fat)
    );

    // In a folder, asked as one action: the plan's sizing refuses it.
    app.apply_whole(Plan::with_steps(
        Words::as_written("copy films"),
        vec![copy(&films, dest.path())],
    ));
    let ExecEvent::Finished { applied, error, .. } = finished(&app) else {
        unreachable!()
    };
    let error = error.expect("refused").english();
    assert_eq!(applied, 0);
    assert!(
        error.contains("“it's a film.mkv” is too large for"),
        "names it: {error}"
    );
    assert!(
        names(dest.path()).is_empty(),
        "nothing copied: {:?}",
        names(dest.path())
    );

    // A drive filled: what was written is kept for Retry, its lock held,
    // until the drive is let go of.
    let part = tmp.path().join("part.bin");
    varied_file(&part, 2 << 30);
    let steps = (0..4)
        .map(|i| {
            Box::new(Copy {
                entry: EntryRef::from_path(&part).unwrap(),
                dest_dir: dest.path().to_path_buf(),
                new_name: Some(format!("part {i}.bin").into()),
                preserve_attributes: false,
            }) as Box<dyn Operation>
        })
        .collect();
    let plan = Plan::with_steps(Words::as_written("copy parts"), steps);
    let r = app.executor.execute_blocking(&plan, app.lifetime());
    let error = r.error.expect("the drive filled").english();
    assert!(error.contains("the drive is full"), "says why: {error}");
    assert!(app.executor.can_retry(plan.id));
    assert!(
        !held_under(&fat).is_empty(),
        "the partial's lock holds the drive"
    );
    // The drive let go of, as before its unmount (`App::release`): removed
    // on a thread of its own, an unmount meanwhile trying again.
    app.release(&fat, &skwir_core::location::Location::from_path(tmp.path()))
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let staged = || {
        names(dest.path())
            .iter()
            .any(|name| name.starts_with(".skwir-copy"))
    };
    while (!held_under(&fat).is_empty() || staged()) && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(
        held_under(&fat).is_empty(),
        "let go of: {:?}",
        held_under(&fat)
    );
    assert!(
        names(dest.path())
            .iter()
            .all(|name| !name.starts_with(".skwir-copy")),
        "the partial removed: {:?}",
        names(dest.path())
    );
    assert!(
        app.executor.can_retry(plan.id),
        "still retried, from its start"
    );
}
