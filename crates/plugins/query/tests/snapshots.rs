//! A btrfs snapshot below the folder searched is passed over by what it is
//! (ADR-125, 14): a read-only subvolume, whatever its name. It is found
//! itself, counted among what the search left out, walked with
//! `excluded:yes`, and walked when searched in. Made with the system's
//! `btrfs` under `~/.cache/skwir-tests`; skipped, saying so, where there is
//! no btrfs. Linux's.
#![cfg(target_os = "linux")]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use skwir_core::app::{App, AppConfig};
use skwir_core::location::ListingEvent;

/// A writable subvolume and a read-only snapshot of it beside a plain
/// folder, each holding a report, taken apart when dropped.
struct Tree {
    dir: tempfile::TempDir,
}

/// The test's own program, never one Skwir starts.
#[allow(clippy::disallowed_methods, reason = "a test's own program")]
fn btrfs(args: &[&std::ffi::OsStr]) -> bool {
    std::process::Command::new("btrfs")
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

impl Tree {
    fn made() -> Option<Tree> {
        let base = dirs::home_dir()?.join(".cache").join("skwir-tests");
        std::fs::create_dir_all(&base).ok()?;
        let dir = tempfile::Builder::new()
            .prefix("search-snapshots-")
            .tempdir_in(base)
            .ok()?;
        let root = dir.path();
        std::fs::create_dir(root.join("plain")).ok()?;
        std::fs::write(root.join("plain/report.txt"), "fixture").ok()?;
        let volume = root.join("volume");
        if !btrfs(&["subvolume".as_ref(), "create".as_ref(), volume.as_os_str()]) {
            eprintln!("skipped: no btrfs here, or no `btrfs` program");
            return None;
        }
        std::fs::write(volume.join("report.txt"), "fixture").ok()?;
        let snapshot = root.join("its copy");
        if !btrfs(&[
            "subvolume".as_ref(),
            "snapshot".as_ref(),
            "-r".as_ref(),
            volume.as_os_str(),
            snapshot.as_os_str(),
        ]) {
            eprintln!("skipped: no snapshot could be made");
            return None;
        }
        Some(Tree { dir })
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let snapshot = self.root().join("its copy");
        btrfs(&[
            "property".as_ref(),
            "set".as_ref(),
            snapshot.as_os_str(),
            "ro".as_ref(),
            "false".as_ref(),
        ]);
        for volume in [snapshot, self.root().join("volume")] {
            let _ = std::fs::remove_file(volume.join("report.txt"));
            let _ = std::fs::remove_dir(&volume);
        }
    }
}

/// What a search found, relative to `root`, and how many it left out.
fn search(app: &App, expr: &str, root: &Path) -> (BTreeSet<String>, usize) {
    let location = skwir_plugin_query::location_for(expr, Some(root));
    let open = app.open_location(&location).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let (mut found, mut left_out) = (BTreeSet::new(), 0);
    loop {
        match open
            .events()
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("the search completes")
        {
            ListingEvent::Added(batch) => {
                for e in batch {
                    let path: PathBuf = open.path_of(e.id).unwrap();
                    found.insert(path.strip_prefix(root).unwrap().display().to_string());
                }
            }
            ListingEvent::LeftOut(n) => left_out = n,
            ListingEvent::Complete => break,
            ListingEvent::Error(e) => panic!("{e}"),
            _ => {}
        }
    }
    open.close();
    (found, left_out)
}

#[test]
fn a_snapshot_below_the_folder_searched_is_passed_over() {
    let Some(tree) = Tree::made() else {
        return;
    };
    let state = tempfile::tempdir().unwrap();
    let app = App::new(AppConfig::ephemeral(state.path())).unwrap();
    app.plugins
        .add(Arc::new(skwir_plugin_query::QueryPlugin::default()), &app);
    let set = |names: &[&str]| -> BTreeSet<String> { names.iter().map(|s| (*s).into()).collect() };

    let (found, left_out) = search(&app, "report", tree.root());
    assert_eq!(found, set(&["plain/report.txt", "volume/report.txt"]));
    assert!(
        left_out >= 1,
        "the snapshot counted among what was left out"
    );

    // Found itself, by its name, as a folder left out is.
    let (found, _) = search(&app, "copy", tree.root());
    assert_eq!(found, set(&["its copy"]));

    let (found, _) = search(&app, "report excluded:yes", tree.root());
    assert_eq!(
        found,
        set(&[
            "plain/report.txt",
            "volume/report.txt",
            "its copy/report.txt"
        ])
    );

    let inside = tree.root().join("its copy");
    let (found, _) = search(&app, "report", &inside);
    assert_eq!(found, set(&["report.txt"]), "walked when searched in");
}
