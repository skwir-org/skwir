//! The tests' own folders, as nextest's setup script makes them
//! (`tools/test/isolate-xdg.sh`): their trash, which every run shares and
//! every trash listing reads whole, pruned of what was trashed more than a
//! day ago, by its record's date: the file and its record, names of any
//! byte, a link trashed removed and never followed. Run with a home of the
//! test's own: never the tests' shared folders.
#![cfg(target_os = "linux")]

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, SystemTime};

/// Trash `name` (a file, a folder or a link, by `make`) in `trash`, its
/// record dated `ago`.
fn trashed(trash: &Path, name: &str, ago: Duration, make: impl FnOnce(&Path)) {
    make(&trash.join("files").join(name));
    let record = trash.join("info").join(format!("{name}.trashinfo"));
    std::fs::write(
        &record,
        "[Trash Info]\nPath=/example/somewhere\nDeletionDate=2026-01-01T00:00:00\n",
    )
    .unwrap();
    std::fs::File::options()
        .write(true)
        .open(&record)
        .unwrap()
        .set_modified(SystemTime::now() - ago)
        .unwrap();
}

#[test]
fn the_tests_trash_keeps_what_was_trashed_within_a_day() {
    let base = dirs::home_dir().unwrap().join(".cache/skwir-tests");
    std::fs::create_dir_all(&base).unwrap();
    let home = tempfile::Builder::new()
        .prefix("test-folders-")
        .tempdir_in(base)
        .unwrap();
    let trash = home.path().join(".cache/skwir-tests/xdg/data/Trash");
    std::fs::create_dir_all(trash.join("files")).unwrap();
    std::fs::create_dir_all(trash.join("info")).unwrap();
    // What no prune may reach: a link trashed leads there.
    let outside = home.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("kept.txt"), "kept").unwrap();
    std::fs::set_permissions(&outside, std::fs::Permissions::from_mode(0o750)).unwrap();

    let day = Duration::from_hours(24);
    let file = |path: &Path| std::fs::write(path, "trashed").unwrap();
    let odd = "it's \"quoted\" \\ $x\nnew line é.txt";
    trashed(&trash, "old plain.txt", 3 * day, file);
    trashed(&trash, odd, 2 * day, file);
    trashed(&trash, "old folder", 2 * day, |path| {
        std::fs::create_dir_all(path.join("kept unwritable")).unwrap();
        std::fs::write(path.join("kept unwritable/inside.txt"), "x").unwrap();
        std::fs::set_permissions(
            path.join("kept unwritable"),
            std::fs::Permissions::from_mode(0o500),
        )
        .unwrap();
    });
    trashed(&trash, "old link", 2 * day, |path| {
        std::os::unix::fs::symlink(&outside, path).unwrap();
    });
    // A record whose file went already, and two whose names would be the
    // trash's own folder of files, or the trash.
    trashed(&trash, "gone.txt", 2 * day, |_| {});
    trashed(&trash, "", 2 * day, |_| {});
    trashed(&trash, ".", 2 * day, |_| {});
    // Another run's, of today.
    trashed(&trash, "an hour ago.txt", Duration::from_hours(1), file);
    trashed(&trash, "just now's.txt", Duration::ZERO, file);

    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/test/isolate-xdg.sh");
    let out = Command::new("sh")
        .arg(&script)
        .env("HOME", home.path())
        .env("NEXTEST_ENV", home.path().join("nextest.env"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let mut left: Vec<String> = std::fs::read_dir(trash.join("files"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    left.sort();
    assert_eq!(left, ["an hour ago.txt", "just now's.txt"]);
    let mut records: Vec<String> = std::fs::read_dir(trash.join("info"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    records.sort();
    assert_eq!(
        records,
        [
            "..trashinfo",
            ".trashinfo",
            "an hour ago.txt.trashinfo",
            "just now's.txt.trashinfo"
        ]
    );
    assert_eq!(
        std::fs::read_to_string(outside.join("kept.txt")).unwrap(),
        "kept",
        "a link trashed is not followed"
    );
    assert_eq!(
        std::fs::metadata(&outside).unwrap().permissions().mode() & 0o777,
        0o750
    );
}
