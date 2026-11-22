//! What panes show as volumes come and go (ADR-149). A folder shown below
//! a mount let go of (a drive unmounted or pulled out, a server
//! disconnected), by whoever let go of it, lets go of what it held open
//! there and says so (`Failure::Left`). One shown at or below a mount
//! made, by whoever made it (the sidebar, `udisksctl` in a terminal, an
//! automounter, a remote's program), is opened again from its path and
//! shows what is there now: a folder that could not be shown comes back
//! by itself, and an empty mount point something mounted on shows what
//! it mounted. Told by the table of mounts (`reach::follow`) and by what
//! a plugin knows mounted that the table does not list (a GVfs bridge's
//! folders, `Registry::remotes`); never by reading the paths shown.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::app::App;
use crate::hashing;
use crate::location::{ListingError, OpenLocation};
use crate::scheduler::Priority;
use crate::words;

/// The mounts as last seen, against which a change is told; `None` until
/// first seen.
#[derive(Default)]
pub(crate) struct Seen(parking_lot::Mutex<Option<Mounts>>);

/// What is mounted, by point.
#[derive(Default)]
struct Mounts {
    /// The table's points, each with how many mounts it holds and
    /// whether one is a network's.
    table: hashing::Map<PathBuf, Point>,
    /// The folders a plugin knows mounted that the table does not list
    /// (a GVfs bridge's): only ever told as mounted, as a plugin turned
    /// off lets go of what it knows, not of the mounts.
    known: hashing::Set<PathBuf>,
}

#[derive(Clone, Copy, Default)]
struct Point {
    mounts: usize,
    remote: bool,
}

impl Mounts {
    /// What is mounted now, read from memory.
    fn now(app: &App) -> Self {
        let mut table: hashing::Map<PathBuf, Point> = hashing::Map::default();
        for (point, remote) in crate::fs::reach::points() {
            let seen = table.entry(point).or_default();
            seen.mounts += 1;
            seen.remote |= remote;
        }
        let known = app
            .registry
            .remotes
            .all()
            .into_iter()
            .map(|known| known.point)
            .filter(|point| !table.contains_key(point))
            .collect();
        Mounts { table, known }
    }
}

/// What changed between two sights of the mounts: the points that hold
/// more mounts than before, and those that hold fewer, each with whether
/// it was a network's.
#[derive(Default)]
struct Change {
    made: hashing::Set<PathBuf>,
    gone: hashing::Map<PathBuf, bool>,
}

impl Change {
    fn between(before: &Mounts, now: &Mounts) -> Self {
        let mut change = Change::default();
        for (point, seen) in &now.table {
            if before
                .table
                .get(point)
                .is_none_or(|b| b.mounts < seen.mounts)
            {
                change.made.insert(point.clone());
            }
        }
        for (point, seen) in &before.table {
            if now.table.get(point).is_none_or(|n| n.mounts < seen.mounts) {
                change.gone.insert(point.clone(), seen.remote);
            }
        }
        for point in &now.known {
            if !before.known.contains(point) && !before.table.contains_key(point) {
                change.made.insert(point.clone());
            }
        }
        change
    }

    fn is_empty(&self) -> bool {
        self.made.is_empty() && self.gone.is_empty()
    }

    /// Whether a mount was made at `path` or above it.
    fn made_over(&self, path: &Path) -> bool {
        path.ancestors().any(|at| self.made.contains(at))
    }

    /// The deepest mount let go of at `path` or above it: whether it was a
    /// network's.
    fn gone_from(&self, path: &Path) -> Option<bool> {
        path.ancestors().find_map(|at| self.gone.get(at).copied())
    }
}

/// Follow the mounts for `app`'s panes: seen once at start, on a worker,
/// then told each change (`reach::follow`).
pub(crate) fn follow(app: &Arc<App>) {
    let weak = Arc::downgrade(app);
    let lifetime = app.lifetime().child();
    app.scheduler.spawn(
        Priority::Background,
        "volumes.follow",
        lifetime.clone(),
        move || {
            let Some(app) = weak.upgrade() else {
                return;
            };
            let told = Arc::downgrade(&app);
            crate::fs::reach::follow(&lifetime, move || {
                if let Some(app) = told.upgrade() {
                    app.volumes_changed();
                }
            });
            // Seen before any change is told against it.
            app.see_volumes();
        },
    );
}

/// What a folder shown below a mount let go of says: its drive's or its
/// server's going, and that it comes back.
fn gone(remote: bool) -> ListingError {
    ListingError::left(if remote {
        words!(
            volumes_server_gone = "The server holding this folder was disconnected. The folder shows here again once it is connected.",
            note = "Shown in place of a folder's files when the network server it is on was disconnected while it was shown."
        )
    } else {
        words!(
            volumes_drive_gone = "The drive holding this folder was unmounted or removed. The folder shows here again once it is mounted.",
            note = "Shown in place of a folder's files when the drive it is on (a USB stick, a disk) was unmounted or pulled out while it was shown."
        )
    })
}

/// Where `open` lists, as far as mounts are concerned: the folder it holds
/// as the system resolved it (a link on the way followed) and the path its
/// location names.
fn paths_of(open: &Arc<dyn OpenLocation>) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = open.holds().map(Path::to_path_buf).into_iter().collect();
    if let Some(path) = open.location().path()
        && !paths.contains(&path)
    {
        paths.push(path);
    }
    paths
}

impl App {
    /// A mount was made or let go of (`reach::follow`), or a plugin knows
    /// one mounted that the table does not list (a GVfs bridge's folder,
    /// published in `Registry::remotes`): what the panes show below a
    /// mount let go of lets go of it and says so, and what they show at
    /// or below one made is opened again from its path (ADR-149). Starts
    /// the work on a worker and returns; from any thread.
    pub fn volumes_changed(&self) {
        let weak = self.weak();
        self.scheduler.spawn(
            Priority::Normal,
            "volumes.changed",
            self.lifetime().child(),
            move || {
                if let Some(app) = weak.upgrade() {
                    app.see_volumes();
                }
            },
        );
    }

    /// See the mounts again, and act on what changed since they were
    /// last seen: on a worker. Under the lock of what was seen, so two
    /// changes told close together act in the order they were seen.
    fn see_volumes(&self) {
        let mut seen = self.volumes.0.lock();
        let now = Mounts::now(self);
        let change = seen.as_ref().map(|before| Change::between(before, &now));
        *seen = Some(now);
        let Some(change) = change.filter(|change| !change.is_empty()) else {
            return;
        };
        self.act_on(&change);
    }

    /// What the panes hold open below a mount let go of lets go of it and
    /// says so; what they hold at or below one made is opened again.
    fn act_on(&self, change: &Change) {
        let open: Vec<Arc<dyn OpenLocation>> = {
            let ws = self.workspace.read();
            let mut held: hashing::Set<*const ()> = hashing::Set::default();
            ws.panes()
                .iter()
                // A tab not shown yet opens what is there once it is.
                .filter(|pane| !pane.is_dormant())
                .flat_map(crate::workspace::Pane::held_open)
                .filter(|open| held.insert(Arc::as_ptr(open).cast::<()>()))
                .collect()
        };
        for open in open {
            let paths = paths_of(&open);
            if let Some(remote) = paths.iter().find_map(|path| change.gone_from(path)) {
                open.volume_gone(gone(remote));
            }
            if paths.iter().any(|path| change.made_over(path)) {
                open.reopen();
            }
        }
    }

    /// Open again each folder the panes show that was not there, where no
    /// table tells a volume mounted (Windows' drives): as a window comes
    /// to the front. Each in its own lane, failing again as it failed.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) fn reopen_missing(&self) {
        let missing: Vec<Arc<dyn OpenLocation>> = {
            let ws = self.workspace.read();
            ws.panes()
                .iter()
                .filter(|pane| {
                    !pane.is_dormant()
                        && pane.listing.failure() == Some(crate::location::Failure::Missing)
                        && pane
                            .location()
                            .is_some_and(crate::location::Location::is_path)
                })
                .filter_map(|pane| pane.open.clone())
                .collect()
        };
        for open in missing {
            open.reopen();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Change, Mounts, Point};
    use crate::app::{App, AppConfig};
    use crate::location::Failure;
    use crate::workspace::PaneId;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    fn mounts(table: &[(&str, usize, bool)], known: &[&str]) -> Mounts {
        Mounts {
            table: table
                .iter()
                .map(|&(point, mounts, remote)| (PathBuf::from(point), Point { mounts, remote }))
                .collect(),
            known: known.iter().map(PathBuf::from).collect(),
        }
    }

    /// What changed is what holds more mounts than before and what holds
    /// fewer: a mount stacked on a point is one made, one of two let go of
    /// one gone; a folder a plugin knows mounted is only ever made.
    #[test]
    fn a_change_is_what_was_mounted_and_what_was_let_go_of() {
        let before = mounts(
            &[
                ("/", 1, false),
                ("/media/u/usb", 1, false),
                ("/mnt/nas", 1, true),
            ],
            &["/run/user/1/gvfs/sftp:host=a"],
        );
        let now = mounts(
            &[
                ("/", 1, false),
                ("/mnt/nas", 2, true),
                ("/media/u/disk", 1, false),
            ],
            &["/run/user/1/gvfs/sftp:host=b"],
        );
        let change = Change::between(&before, &now);
        let mut made: Vec<_> = change.made.iter().cloned().collect();
        made.sort();
        assert_eq!(
            made,
            [
                PathBuf::from("/media/u/disk"),
                "/mnt/nas".into(),
                "/run/user/1/gvfs/sftp:host=b".into()
            ]
        );
        assert_eq!(change.gone.len(), 1);
        assert_eq!(change.gone.get(Path::new("/media/u/usb")), Some(&false));
        // Below a mount made or gone, by the path's text alone.
        assert!(change.made_over(Path::new("/media/u/disk/it's here/été")));
        assert!(change.made_over(Path::new("/media/u/disk")));
        assert!(!change.made_over(Path::new("/media/u")));
        assert_eq!(
            change.gone_from(Path::new("/media/u/usb/photos")),
            Some(false)
        );
        assert_eq!(change.gone_from(Path::new("/media/u/usbs")), None);
        // Let go of the other way: a server's going says so.
        let back = Change::between(&now, &before);
        assert_eq!(back.gone_from(Path::new("/mnt/nas/a")), Some(true));
        assert!(Change::between(&now, &now).is_empty());
    }

    fn wait(app: &App, pane: PaneId, what: &str, done: impl Fn(&crate::workspace::Pane) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            app.pump();
            if done(app.workspace.read().pane(pane).unwrap()) {
                return;
            }
            assert!(Instant::now() < deadline, "never {what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn named(pane: &crate::workspace::Pane, name: &str) -> bool {
        pane.listing.find_by_name(name.as_ref()).is_some()
    }

    fn show(app: &App, folder: &Path) -> PaneId {
        let pane = app.workspace.read().active_id();
        app.dispatch(&crate::command::Invocation::navigate(
            &crate::location::Location::from_path(folder),
        ))
        .unwrap();
        pane
    }

    /// A folder shown below a mount let go of says its drive went and
    /// lets go of what it held there; one made there again shows it again,
    /// as it is now, with nothing asked.
    #[test]
    fn a_folder_shown_says_its_volume_went_and_comes_back_with_it() {
        let tmp = tempfile::tempdir().unwrap();
        let folder = tmp.path().join("it's on a drive");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("before.txt"), "1").unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        let pane = show(&app, &folder);
        wait(&app, pane, "listed", |p| {
            p.listing.is_complete() && named(p, "before.txt")
        });

        let gone = Change {
            gone: [(folder.clone(), false)].into_iter().collect(),
            ..Change::default()
        };
        app.act_on(&gone);
        wait(&app, pane, "lost", |p| {
            p.listing.failure() == Some(Failure::Left)
        });
        {
            let ws = app.workspace.read();
            let p = ws.pane(pane).unwrap();
            assert!(p.listing.is_empty(), "its files go with it");
            assert!(
                p.listing
                    .error()
                    .is_some_and(|said| said.english().contains("drive")),
                "{:?}",
                p.listing.error()
            );
        }
        // What it held open there is let go of: an unmount would wait.
        assert!(crate::fs::wait_let_go(&folder, Duration::from_secs(20)));

        std::fs::write(folder.join("after.txt"), "2").unwrap();
        let made = Change {
            made: [tmp.path().to_path_buf()].into_iter().collect(),
            ..Change::default()
        };
        app.act_on(&made);
        wait(&app, pane, "back", |p| {
            p.listing.failure().is_none() && named(p, "after.txt") && named(p, "before.txt")
        });
    }

    /// Run in a user and mount namespace of the test's own, where it
    /// mounts as root (`a_folder_on_a_mount_goes_and_comes_back_with_it`).
    #[cfg(target_os = "linux")]
    #[test]
    fn the_folders_shown_follow_the_mounts() {
        if !crate::testing::in_namespace() {
            crate::testing::as_root_in_a_namespace(
                "volumes::tests::a_folder_on_a_mount_goes_and_comes_back_with_it",
            );
        }
    }

    /// The mounts as the system makes and lets go of them, told by its
    /// table: a folder a pane could not show, at a mount point empty,
    /// shows once a filesystem holding it is mounted there; let go of
    /// lazily (as udisks does a drive pulled out), it says so and lets go
    /// of its folder, which lets the filesystem go; mounted again, it is
    /// back. Does nothing outside its namespace.
    #[cfg(target_os = "linux")]
    #[test]
    // Tests' own, on Linux: no console window to keep away.
    #[allow(clippy::disallowed_methods)]
    fn a_folder_on_a_mount_goes_and_comes_back_with_it() {
        use std::process::Command;
        if !crate::testing::in_namespace() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let drive = tmp.path().join("the drive's files");
        let point = tmp.path().join("where it's mounted");
        std::fs::create_dir(&drive).unwrap();
        std::fs::create_dir(&point).unwrap();
        let mount = |what: &[&std::ffi::OsStr]| {
            assert!(Command::new("mount").args(what).status().unwrap().success());
        };
        mount(&[
            "-t".as_ref(),
            "tmpfs".as_ref(),
            "skwir-test".as_ref(),
            drive.as_os_str(),
        ]);
        let shown = point.join("photos, été");
        std::fs::create_dir(drive.join("photos, été")).unwrap();
        std::fs::write(drive.join("photos, été").join("it's a.jpg"), "a").unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        // Seen once at start, on a worker: then a change is told.
        let deadline = Instant::now() + Duration::from_secs(20);
        while app.volumes.0.lock().is_none() {
            assert!(Instant::now() < deadline, "the mounts were never seen");
            std::thread::sleep(Duration::from_millis(5));
        }
        let pane = show(&app, &shown);
        wait(&app, pane, "missing", |p| {
            p.listing.failure() == Some(Failure::Missing)
        });

        mount(&["--bind".as_ref(), drive.as_os_str(), point.as_os_str()]);
        wait(&app, pane, "shown once mounted", |p| {
            p.listing.failure().is_none() && named(p, "it's a.jpg")
        });

        assert!(
            Command::new("umount")
                .arg("-l")
                .arg(&point)
                .status()
                .unwrap()
                .success()
        );
        wait(&app, pane, "lost with its mount", |p| {
            p.listing.failure() == Some(Failure::Left) && p.listing.is_empty()
        });
        assert!(crate::fs::wait_let_go(&point, Duration::from_secs(20)));

        std::fs::write(drive.join("photos, été").join("b.jpg"), "b").unwrap();
        mount(&["--bind".as_ref(), drive.as_os_str(), point.as_os_str()]);
        wait(&app, pane, "back once mounted again", |p| {
            p.listing.failure().is_none() && named(p, "b.jpg")
        });
        drop(app);
        let _ = Command::new("umount").arg("-l").arg(&point).status();
        let _ = Command::new("umount").arg("-l").arg(&drive).status();
    }
}
