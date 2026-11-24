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

/// The mounts as last seen, against which a change is told, and the
/// changes seen and not acted on yet.
#[derive(Default)]
pub(crate) struct Seen(parking_lot::Mutex<Sight>);

#[derive(Default)]
struct Sight {
    /// `None` until first seen.
    mounts: Option<Mounts>,
    /// The changes seen and not acted on yet, in the order seen.
    waiting: std::collections::VecDeque<Change>,
    /// Whether a worker acts on them: one at a time, in the order seen,
    /// never under this lock (what acts calls the locations' own code).
    acting: bool,
}

/// A worker acting on the changes seen (`App::see_volumes`): if it unwinds,
/// another acts on what is left.
struct Acting<'a>(&'a Seen, bool);

impl Drop for Acting<'_> {
    fn drop(&mut self) {
        if self.1 {
            self.0.0.lock().acting = false;
        }
    }
}

impl Seen {
    /// Whether the mounts were seen once.
    #[cfg(all(test, target_os = "linux"))]
    fn once(&self) -> bool {
        self.0.lock().mounts.is_some()
    }
}

/// What is mounted, by point.
#[derive(Default)]
struct Mounts {
    /// The table's points, each with the mounts it holds and whether one
    /// is a network's.
    table: hashing::Map<PathBuf, Point>,
    /// The folders a plugin knows mounted that the table does not list
    /// (a GVfs bridge's), each with that plugin: one it no longer knows
    /// is let go of only while it is on, as a plugin turned off lets go
    /// of what it knows, not of the mounts.
    known: hashing::Map<PathBuf, std::sync::Arc<str>>,
}

#[derive(Clone, Default)]
struct Point {
    /// What tells each mount apart (`reach::Mount::id`): one let go of and
    /// made again within a change, of the same drive even, is another.
    mounts: hashing::Set<u64>,
    remote: bool,
    /// One is an automount point (`reach::Mount::on_demand`).
    on_demand: bool,
    /// One is not: an automount point with its filesystem mounted on it
    /// (systemd's direct automount), where one holding nothing but
    /// automount points is an indirect map's (`/net`).
    other: bool,
}

impl Point {
    /// An indirect map's point: automount points alone, what is mounted
    /// below it the automounter's.
    fn is_map(&self) -> bool {
        self.on_demand && !self.other
    }
}

impl Mounts {
    /// Whether what is at `point` is mounted on demand: it holds an
    /// automount point (a direct one's), or the deepest point above it
    /// holds automount points alone (an indirect map's, `/net`). A mount
    /// there let go of is the automounter's, after a while unused, which is
    /// no volume going: looked up again, it is mounted again (`systemd`'s
    /// `x-systemd.idle-timeout`, autofs's). Below a direct point, which
    /// holds its own filesystem too, a mount is someone's own (a stick
    /// mounted in a folder systemd automounts): its going is a volume's, as
    /// `reach::on_demand` says too.
    fn on_demand(&self, point: &Path) -> bool {
        if self.table.get(point).is_some_and(|own| own.on_demand) {
            return true;
        }
        point
            .ancestors()
            .skip(1)
            .find_map(|at| self.table.get(at))
            .is_some_and(Point::is_map)
    }

    /// What is mounted now, read from memory.
    fn now(app: &App) -> Self {
        let mut table: hashing::Map<PathBuf, Point> = hashing::Map::default();
        for mount in crate::fs::reach::points() {
            let seen = table.entry(mount.point).or_default();
            seen.mounts.insert(mount.id);
            seen.remote |= mount.remote;
            seen.on_demand |= mount.on_demand;
            seen.other |= !mount.on_demand;
        }
        let known = app
            .registry
            .remotes
            .all_owned()
            .into_iter()
            .map(|(owner, known)| (known.point, owner))
            .filter(|(point, _)| !table.contains_key(point))
            .collect();
        Mounts { table, known }
    }
}

/// What changed between two sights of the mounts: the points that hold a
/// mount they did not before, and those that no longer hold one they did,
/// each with whether it was a network's. By what each mount is, not by
/// how many a point holds: a drive let go of and mounted again at once
/// (`umount -l` then `mount`) is both, its folders let go of and opened
/// again, never left holding the filesystem let go of.
#[derive(Default)]
struct Change {
    made: hashing::Set<PathBuf>,
    gone: hashing::Map<PathBuf, bool>,
}

impl Change {
    /// What changed from `before` to `now`; `on` says whether a plugin is
    /// on still.
    fn between(before: &Mounts, now: &Mounts, on: impl Fn(&str) -> bool) -> Self {
        let mut change = Change::default();
        for (point, seen) in &now.table {
            if before
                .table
                .get(point)
                .is_none_or(|b| !seen.mounts.is_subset(&b.mounts))
            {
                change.made.insert(point.clone());
            }
        }
        for (point, seen) in &before.table {
            if now
                .table
                .get(point)
                .is_none_or(|n| !seen.mounts.is_subset(&n.mounts))
                && !now.on_demand(point)
            {
                change.gone.insert(point.clone(), seen.remote);
            }
        }
        for point in now.known.keys() {
            if !before.known.contains_key(point) && !before.table.contains_key(point) {
                change.made.insert(point.clone());
            }
        }
        // A bridge's folder its plugin knows no more while it is on: let go
        // of (GVfs disconnected it), a network's.
        for (point, owner) in &before.known {
            if !now.known.contains_key(point) && !now.table.contains_key(point) && on(owner) {
                change.gone.insert(point.clone(), true);
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

/// Follow the mounts for `app`'s panes: seen once as it starts, then told
/// each change (`reach::follow`, on a worker).
pub(crate) fn follow(app: &Arc<App>) {
    // Seen at once, from the table in memory (read if it was not: procfs,
    // or `getfsstat` waiting on no mount): a mount made from now on, as the
    // panes restored open their folders, is a change, never taken into a
    // first sight a worker makes after it.
    app.see_volumes();
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
            // What changed between the first sight and the follower's being
            // told, seen against it.
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
    let mut paths: Vec<PathBuf> = open.holds().into_iter().collect();
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
    /// last seen: on a worker. Seen under the lock of what was seen, and
    /// acted on out of it by one worker at a time, so two changes told
    /// close together act in the order they were seen, and no lock is held
    /// as the locations' own code runs (AGENTS.md, "Locks and re-entry").
    fn see_volumes(&self) {
        {
            let mut sight = self.volumes.0.lock();
            let now = Mounts::now(self);
            let gate = self.registry.gate();
            let change = sight
                .mounts
                .as_ref()
                .map(|before| Change::between(before, &now, |owner| gate.is_open(owner)));
            sight.mounts = Some(now);
            if let Some(change) = change.filter(|change| !change.is_empty()) {
                sight.waiting.push_back(change);
            }
            if sight.acting || sight.waiting.is_empty() {
                return;
            }
            sight.acting = true;
        }
        // Acted on out of the lock, in the order seen: another worker seeing
        // a change meanwhile leaves it to this one, which ends only under
        // the lock that found none left. One that unwound lets another act.
        let mut acting = Acting(&self.volumes, true);
        loop {
            let change = {
                let mut sight = self.volumes.0.lock();
                let Some(change) = sight.waiting.pop_front() else {
                    sight.acting = false;
                    acting.1 = false;
                    return;
                };
                change
            };
            self.act_on(&change);
        }
    }

    /// What the panes hold open below a mount let go of lets go of it and
    /// says so; what they hold at or below one made is opened again. What
    /// copies cut short left on a drive that went with it is swept as it
    /// comes back (`staging::UnderWay::sweep_left_under`), out of the way.
    fn act_on(&self, change: &Change) {
        if !change.made.is_empty()
            && let Some(under_way) = self.executor.under_way()
        {
            let made: Vec<PathBuf> = change.made.iter().cloned().collect();
            self.scheduler.spawn(
                Priority::Background,
                "staging.sweep",
                self.lifetime().child(),
                move || under_way.sweep_left_under(&made),
            );
        }
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

    /// The mounts as seen: each point with the mounts it holds, by what
    /// tells each apart, and whether one is a network's.
    fn mounts(table: &[(&str, &[u64], bool)], known: &[&str]) -> Mounts {
        Mounts {
            table: table
                .iter()
                .map(|&(point, mounts, remote)| {
                    let other = !mounts.is_empty();
                    let mounts = mounts.iter().copied().collect();
                    let on_demand = false;
                    (
                        PathBuf::from(point),
                        Point {
                            mounts,
                            remote,
                            on_demand,
                            other,
                        },
                    )
                })
                .collect(),
            known: known
                .iter()
                .map(|point| (PathBuf::from(point), std::sync::Arc::from("remote")))
                .collect(),
        }
    }

    /// What changed is what a point holds that it did not and what it
    /// held that it does not: a mount stacked on a point is one made, one
    /// of two let go of one gone, one let go of and made again in its place
    /// both; a folder a plugin knows mounted, made, and gone only while
    /// that plugin is on.
    #[test]
    fn a_change_is_what_was_mounted_and_what_was_let_go_of() {
        let before = mounts(
            &[
                ("/", &[1], false),
                ("/media/u/usb", &[2], false),
                ("/mnt/nas", &[3], true),
                ("/mnt/again", &[4], false),
            ],
            &["/run/user/1/gvfs/sftp:host=a"],
        );
        let now = mounts(
            &[
                ("/", &[1], false),
                ("/mnt/nas", &[3, 5], true),
                ("/media/u/disk", &[6], false),
                ("/mnt/again", &[7], false),
            ],
            &["/run/user/1/gvfs/sftp:host=b"],
        );
        let change = Change::between(&before, &now, |_| true);
        let mut made: Vec<_> = change.made.iter().cloned().collect();
        made.sort();
        assert_eq!(
            made,
            [
                PathBuf::from("/media/u/disk"),
                "/mnt/again".into(),
                "/mnt/nas".into(),
                "/run/user/1/gvfs/sftp:host=b".into()
            ]
        );
        assert_eq!(change.gone.len(), 3);
        assert_eq!(change.gone.get(Path::new("/media/u/usb")), Some(&false));
        // Let go of and made again within one change: both, so what was
        // held there is let go of before it is opened again.
        assert_eq!(change.gone.get(Path::new("/mnt/again")), Some(&false));
        // A bridge's folder its plugin knows no more, a network's: let go
        // of while the plugin is on, never as it is turned off.
        assert_eq!(
            change.gone.get(Path::new("/run/user/1/gvfs/sftp:host=a")),
            Some(&true)
        );
        let off = Change::between(&before, &now, |_| false);
        assert!(
            !off.gone
                .contains_key(Path::new("/run/user/1/gvfs/sftp:host=a"))
        );
        // Below a mount made or gone, by the path's text alone.
        assert!(change.made_over(Path::new("/media/u/disk/it's here/été")));
        assert!(change.made_over(Path::new("/media/u/disk")));
        assert!(!change.made_over(Path::new("/media/u")));
        assert_eq!(
            change.gone_from(Path::new("/media/u/usb/photos")),
            Some(false)
        );
        assert_eq!(change.gone_from(Path::new("/media/u/usbs")), None);
        // A share mounted on an automount point and let go of by the
        // automounter, unused a while, is no volume going; nor one mounted
        // below an indirect map's point (`/net/host`). The point itself
        // let go of is.
        let mut automounted = mounts(
            &[
                ("/", &[1], false),
                ("/mnt/nas", &[2, 3], true),
                ("/net", &[4], false),
            ],
            &[],
        );
        automounted
            .table
            .get_mut(Path::new("/mnt/nas"))
            .unwrap()
            .on_demand = true;
        // An indirect map's point holds its autofs alone.
        let map = |point: &mut Point| {
            point.on_demand = true;
            point.other = false;
        };
        map(automounted.table.get_mut(Path::new("/net")).unwrap());
        automounted.table.insert(
            "/net/host".into(),
            Point {
                mounts: [5].into_iter().collect(),
                remote: true,
                on_demand: false,
                other: true,
            },
        );
        let mut expired = mounts(
            &[
                ("/", &[1], false),
                ("/mnt/nas", &[2], false),
                ("/net", &[4], false),
            ],
            &[],
        );
        expired
            .table
            .get_mut(Path::new("/mnt/nas"))
            .unwrap()
            .on_demand = true;
        map(expired.table.get_mut(Path::new("/net")).unwrap());
        assert!(
            Change::between(&automounted, &expired, |_| true)
                .gone
                .is_empty()
        );
        // A stick mounted in a folder systemd automounts (a direct point,
        // its own filesystem stacked on its autofs) and taken out: a volume
        // gone, not the automounter's.
        let mut stick = mounts(
            &[
                ("/", &[1], false),
                ("/data", &[6, 7], false),
                ("/data/usb", &[8], false),
            ],
            &[],
        );
        stick.table.get_mut(Path::new("/data")).unwrap().on_demand = true;
        let mut out = mounts(&[("/", &[1], false), ("/data", &[6, 7], false)], &[]);
        out.table.get_mut(Path::new("/data")).unwrap().on_demand = true;
        let change = Change::between(&stick, &out, |_| true);
        assert_eq!(change.gone.get(Path::new("/data/usb")), Some(&false));
        let stopped = mounts(&[("/", &[1], false)], &[]);
        let change = Change::between(&expired, &stopped, |_| true);
        assert!(change.gone.contains_key(Path::new("/mnt/nas")));
        assert!(change.gone.contains_key(Path::new("/net")));
        // Let go of the other way: a server's going says so.
        let back = Change::between(&now, &before, |_| true);
        assert_eq!(back.gone_from(Path::new("/mnt/nas/a")), Some(true));
        assert!(Change::between(&now, &now, |_| true).is_empty());
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

    /// A drive come back is swept of what a copy cut short left on it as
    /// it went (a staging folder let go of out of reach, the folder it is
    /// in noted, `staging::Staging::stays`), as its mount is made. On the
    /// home's disk: a sweep looks only where this machine alone writes,
    /// never on tmpfs.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_drive_come_back_is_swept_of_what_copies_cut_short_left() {
        let base = dirs::home_dir().unwrap().join(".cache/skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        let tmp = tempfile::tempdir_in(base).unwrap();
        let stick = tmp.path().join("a stick");
        let dest = stick.join("photos, l'été");
        std::fs::create_dir_all(&dest).unwrap();
        let config = AppConfig::ephemeral(tmp.path());
        let notes = crate::cache::Cache::open(config.cache_root.clone())
            .namespace("staging")
            .folder("under-way");
        let app = App::new(config).unwrap();
        // The start's own sweep done first.
        assert!(app.scheduler.wait_idle(Duration::from_secs(10)));
        let left = dest.join(".skwir-copy-AbC123");
        std::fs::create_dir(&left).unwrap();
        std::fs::write(left.join("lock"), b"").unwrap();
        std::fs::write(left.join("0"), b"part").unwrap();
        std::fs::create_dir_all(&notes).unwrap();
        std::fs::write(notes.join("copy-left"), dest.as_os_str().as_encoded_bytes()).unwrap();

        let elsewhere = Change {
            made: [tmp.path().join("another stick")].into_iter().collect(),
            ..Change::default()
        };
        app.act_on(&elsewhere);
        assert!(app.scheduler.wait_idle(Duration::from_secs(10)));
        assert!(left.is_dir(), "another drive's coming leaves it");
        let back = Change {
            made: [stick.clone()].into_iter().collect(),
            ..Change::default()
        };
        app.act_on(&back);
        assert!(app.scheduler.wait_idle(Duration::from_secs(10)));
        assert!(!left.exists(), "swept as its drive is back");
        assert!(!notes.join("copy-left").exists(), "and forgotten");
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
            crate::testing::as_root_in_a_namespace(
                "volumes::tests::a_folder_the_user_may_let_go_of_never_refuses_its_unmount",
            );
        }
    }

    /// A folder shown on a volume the user may let go of (here a remote,
    /// as pretended) is held by nothing: a plain unmount by another
    /// program, which asks the system and nothing else, is not refused as
    /// busy, and the pane says its volume went (ADR-149, amended). Does
    /// nothing outside its namespace.
    #[cfg(target_os = "linux")]
    #[test]
    // Tests' own, on Linux: no console window to keep away.
    #[allow(clippy::disallowed_methods)]
    fn a_folder_the_user_may_let_go_of_never_refuses_its_unmount() {
        use std::process::Command;
        if !crate::testing::in_namespace() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let point = tmp.path().join("a server's folder");
        std::fs::create_dir(&point).unwrap();
        assert!(
            Command::new("mount")
                .args(["-t", "tmpfs", "skwir-test"])
                .arg(&point)
                .status()
                .unwrap()
                .success()
        );
        let shown = point.join("it's shown");
        std::fs::create_dir(&shown).unwrap();
        std::fs::write(shown.join("a.txt"), "a").unwrap();
        crate::fs::reach::pretend_remote(&point);
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        // Seen as it starts: a change from now on is told against it.
        assert!(app.volumes.once());
        let pane = show(&app, &shown);
        wait(&app, pane, "listed", |p| named(p, "a.txt"));
        assert!(
            Command::new("umount")
                .arg(&point)
                .status()
                .unwrap()
                .success(),
            "refused as busy"
        );
        wait(&app, pane, "said gone", |p| {
            p.listing.failure() == Some(Failure::Left)
        });
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
        // Seen as it starts: a change from now on is told against it.
        assert!(app.volumes.once());
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

        // Let go of lazily and another drive mounted in its place at once,
        // within one change of the table: the folder held is let go of and
        // the new drive's shown, never the one let go of held still.
        let other = tmp.path().join("another drive");
        std::fs::create_dir(&other).unwrap();
        mount(&[
            "-t".as_ref(),
            "tmpfs".as_ref(),
            "skwir-test".as_ref(),
            other.as_os_str(),
        ]);
        std::fs::create_dir(other.join("photos, été")).unwrap();
        std::fs::write(other.join("photos, été").join("c.jpg"), "c").unwrap();
        assert!(
            Command::new("sh")
                .arg("-c")
                .arg(r#"umount -l "$1" && mount --bind "$2" "$1""#)
                .arg("sh")
                .arg(&point)
                .arg(&other)
                .status()
                .unwrap()
                .success()
        );
        wait(&app, pane, "the drive mounted in its place shown", |p| {
            p.listing.failure().is_none() && named(p, "c.jpg") && !named(p, "b.jpg")
        });
        drop(app);
        let _ = Command::new("umount").arg("-l").arg(&point).status();
        let _ = Command::new("umount").arg("-l").arg(&other).status();
        let _ = Command::new("umount").arg("-l").arg(&drive).status();
    }
}
