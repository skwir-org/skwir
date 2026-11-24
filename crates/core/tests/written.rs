//! Files another program wrote, marked so while it wrote them (ADR-135,
//! `keys::WRITING`), and marked no more once it is done, whatever way it
//! wrote them.
#![cfg(target_os = "linux")]

mod common;

use std::io::{Read as _, Seek as _, Write as _};
use std::path::Path;
use std::time::{Duration, Instant};

use common::*;
use skwir_core::entry::{Value, keys};

/// What the active pane shows of `names`: whether each is marked as being
/// written, and whether its size is `sizes`'.
fn shown(app: &skwir_core::app::App, names: &[String], sizes: &[u64]) -> Vec<(bool, bool)> {
    let ws = app.workspace.read();
    let listing = &ws.pane(ws.active_id()).unwrap().listing;
    names
        .iter()
        .zip(sizes)
        .map(|(n, size)| {
            let Some(e) = listing.iter().find(|e| e.name == n.as_str()) else {
                return (false, false);
            };
            (
                e.metadata.value(keys::SIZE).and_then(Value::as_u64) == Some(*size),
                e.metadata.value(keys::WRITING) == Some(&Value::Bool(true)),
            )
        })
        .collect()
}

/// Pumps the pane until each of `names` shows its size and no mark, then
/// for a while after: nothing told later marks one again.
fn written_no_more(app: &skwir_core::app::App, dir: &Path, names: &[String]) {
    let sizes: Vec<u64> = names
        .iter()
        .map(|n| std::fs::metadata(dir.join(n)).unwrap().len())
        .collect();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        app.pump();
        let now = shown(app, names, &sizes);
        if now.iter().all(|(sized, writing)| *sized && !writing) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "still marked as written (sized, writing): {now:?}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    // Three times the second a name only written to waits.
    let settled = Instant::now() + Duration::from_secs(3);
    while Instant::now() < settled {
        app.pump();
        let now = shown(app, names, &sizes);
        assert!(
            now.iter().all(|(_, writing)| !writing),
            "marked again: {now:?}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Writes `path` again as `ExifTool` does with `-overwrite_original_in_place`
/// (what Skwir's geotagging runs): the new file beside it, copied over the
/// original in place a block at a time and closed, the one beside it
/// removed, then the original's time put back from another descriptor.
fn rewrite_in_place(path: &Path, bytes: &[u8]) {
    let modified = std::fs::metadata(path).unwrap().modified().unwrap();
    let mut beside = path.as_os_str().to_owned();
    beside.push("_exiftool_tmp");
    let beside = std::path::PathBuf::from(beside);
    {
        let mut original = std::fs::File::open(path).unwrap();
        let mut read = Vec::new();
        original.read_to_end(&mut read).unwrap();
        std::fs::write(&beside, bytes).unwrap();
    }
    {
        let mut new = std::fs::File::open(&beside).unwrap();
        let mut original = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .unwrap();
        let mut block = vec![0; 65536];
        loop {
            let n = new.read(&mut block).unwrap();
            if n == 0 {
                break;
            }
            original.write_all(&block[..n]).unwrap();
        }
        let at = original.stream_position().unwrap();
        original.set_len(at).unwrap();
    }
    std::fs::remove_file(&beside).unwrap();
    let original = std::fs::File::open(path).unwrap();
    original.set_modified(modified).unwrap();
}

/// Several pictures written again in place one after the other (their
/// GPS position taken out), in a folder shown: none is marked as being
/// written once done.
#[test]
fn files_rewritten_in_place_one_after_another_are_written_no_more() {
    let tmp = home_tempdir();
    let dir = tmp.path().join("l'été à Noël");
    std::fs::create_dir(&dir).unwrap();
    let names: Vec<String> = (1..=8)
        .map(|i| format!("Photo n°{i} (l'île).jpg"))
        .collect();
    let big = vec![7u8; 2 << 20];
    for name in &names {
        std::fs::write(dir.join(name), &big).unwrap();
    }
    let app = app(tmp.path());
    navigate(&app, &dir);
    let smaller = vec![9u8; (2 << 20) - 4096];
    for name in &names {
        rewrite_in_place(&dir.join(name), &smaller);
    }
    written_no_more(&app, &dir, &names);
}
