//! Where the Network section's servers are read, followed (ADR-086,
//! amended 2026-11-15): rclone's configuration, Dolphin's places, KDE's
//! network folders, fstab. Each reader says where it read
//! (`connector::Named`), and the way to what it names is watched
//! (`Followed::of`): its folder, or the nearest folder above that is there
//! while it is not (`rclone config` making `~/.config/rclone`), and each
//! link on the way, in its own folder: writers replace a file rather than
//! write into it, and a link re-pointed (NixOS's `/etc/static`, a dotfile
//! manager's) names another file at the same path. A change, by any
//! program, settles, then the servers are read again, which follows what
//! they then name.

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use skwir_core::fs::{Settled, reach, volume};
use skwir_core::os_path;

use crate::connector::Source;

/// How long changes are let settle before the servers are read again: an
/// editor saving, a tool writing several files.
const SETTLE: Duration = Duration::from_millis(400);

/// How many links a way follows at most, as Linux does (`MAXSYMLINKS`):
/// a loop of links ends there.
const MAX_LINKS: usize = 40;

/// The sources followed, and the folders watched for them.
pub struct Sources {
    watcher: Settled,
    /// What counts among the events: shared with the watcher's handler.
    followed: Arc<Mutex<Vec<Source>>>,
    watched: HashSet<PathBuf>,
    /// Whether a folder followed could not be watched: what it holds is
    /// read again as a window comes to the front (`Sources::misses`).
    misses: bool,
    /// The read of the servers whose `Followed` is followed: an older
    /// one, ending after, never takes its place.
    at: u64,
    /// The folders on a volume mounted read-only, left unwatched, each
    /// with its device (`Followed::fixed`).
    fixed: Vec<(PathBuf, Option<u64>)>,
}

impl Sources {
    /// Follow nothing yet: `then` runs once a change of what is followed
    /// settles, on the watcher's own thread, and answers whether to go on.
    /// `None` when the system gives no watcher.
    pub fn start(then: impl Fn() -> bool + Send + 'static) -> Option<Self> {
        let followed: Arc<Mutex<Vec<Source>>> = Arc::default();
        let concerned = Arc::clone(&followed);
        let watcher = Settled::start(
            "skwir-remote-sources",
            SETTLE,
            move |path| concerned.lock().iter().any(|source| concerns(source, path)),
            then,
        )?;
        Some(Sources {
            watcher,
            followed,
            watched: HashSet::new(),
            misses: false,
            at: 0,
            fixed: Vec::new(),
        })
    }

    /// Follow `followed`, found by the read of the servers `at`, from now
    /// on, and what no longer is no more; nothing where a later read's is
    /// followed already. Each folder is asked again, one watched before
    /// too: a folder removed and made again is another, which the watcher
    /// let go of with the first. Watching resolves each folder's path: never
    /// under a lock another thread waits on.
    ///
    /// Whether a folder is watched now that was not after the read before
    /// (the way to a file grown, its folder made since): what changed there
    /// between this read and its watch was told to no one, so the caller
    /// reads the servers once more, the watch in place. Never after the
    /// first read, which nothing was watched before.
    pub fn follow(&mut self, followed: Followed, at: u64) -> bool {
        if at < self.at {
            return false;
        }
        let first = self.at == 0;
        self.at = at;
        let Followed {
            every,
            folders,
            unwatched,
            fixed,
        } = followed;
        self.fixed = fixed;
        // What counts first: what is told from the folders watched next
        // is weighed by it.
        *self.followed.lock() = every;
        for gone in self
            .watched
            .difference(&folders)
            .cloned()
            .collect::<Vec<_>>()
        {
            self.watcher.unwatch(&gone);
            self.watched.remove(&gone);
        }
        self.misses = unwatched;
        let mut added = false;
        for folder in folders {
            if self.watcher.watch(&folder) {
                added |= self.watched.insert(folder);
            } else {
                self.misses = true;
                self.watched.remove(&folder);
            }
        }
        added && !first
    }

    /// Whether a folder followed could not be watched (the system's
    /// watches all taken, a remote's, `Followed::of`): the servers are read
    /// again as a window comes to the front, no change there told.
    pub fn misses(&self) -> bool {
        self.misses
    }

    /// The folders left unwatched on a volume mounted read-only, each with
    /// its device: [`remounted`] tells whether one was mounted again.
    pub fn fixed(&self) -> Vec<(PathBuf, Option<u64>)> {
        self.fixed.clone()
    }

    /// The folders watched now: what a test asks.
    #[cfg(all(test, unix))]
    fn watched(&self) -> Vec<PathBuf> {
        let mut watched: Vec<PathBuf> = self.watched.iter().cloned().collect();
        watched.sort();
        watched
    }
}

/// What is followed for some sources: the links on the way to each and
/// where its way ends, and the folders watched for them. Found where the
/// servers were read, as it reads the folders on the way; `Sources::follow`
/// only watches.
pub struct Followed {
    every: Vec<Source>,
    folders: HashSet<PathBuf>,
    /// Whether a source is on a remote, left unwatched
    /// (`Sources::misses`).
    unwatched: bool,
    /// The folders left unwatched on a volume mounted read-only, each with
    /// its device: nothing there is replaced, but the volume may be
    /// mounted again in its place (NixOS's `/etc` as an overlay, mounted
    /// anew at each switch), which the table tells ([`remounted`]).
    fixed: Vec<(PathBuf, Option<u64>)>,
}

impl Followed {
    /// What `sources` lead through and to. I/O: the reading thread's, and
    /// none on a remote (asked of the table in memory, `reach::of`): a
    /// server that stopped answering would hold the walk, and a watch
    /// there never hears what other machines change.
    pub fn of(sources: Vec<Source>) -> Self {
        Self::with(
            sources,
            |folder| volume::read_only(folder) == Some(true),
            |path| reach::of(path).is_some(),
        )
    }

    /// [`Followed::of`], a folder `read_only` says is on a volume mounted
    /// read-only left unwatched (nothing there is replaced, the Nix
    /// store: what leads into it is re-pointed), and what `remote` says
    /// is on a remote neither walked nor watched.
    fn with(
        sources: Vec<Source>,
        read_only: impl Fn(&Path) -> bool,
        remote: impl Fn(&Path) -> bool,
    ) -> Self {
        let mut every: Vec<Source> = Vec::new();
        for source in sources {
            let (links, end) = match &source {
                Source::File(path) | Source::Folder(path) => way_to(path, &remote),
            };
            every.extend(links.into_iter().map(Source::File));
            every.push(match source {
                Source::File(_) => Source::File(end),
                Source::Folder(_) => Source::Folder(end),
            });
        }
        every.sort();
        every.dedup();
        let (remotes, here): (Vec<&Source>, Vec<&Source>) = every.iter().partition(|source| {
            remote(match source {
                Source::File(path) | Source::Folder(path) => path,
            })
        });
        let (fixed, folders): (HashSet<PathBuf>, HashSet<PathBuf>) = here
            .into_iter()
            .filter_map(folder_of)
            .partition(|folder| read_only(folder));
        let mut fixed: Vec<(PathBuf, Option<u64>)> = fixed
            .into_iter()
            .map(|folder| {
                let device = device_of(&folder);
                (folder, device)
            })
            .collect();
        fixed.sort();
        Followed {
            unwatched: !remotes.is_empty(),
            every,
            folders,
            fixed,
        }
    }
}

/// The way to `path`: each link met, where it is (in a folder reached
/// without a link), and where the way ends, `path` itself where no link
/// is on it. A part not there ends nothing: what follows it is named as
/// written; nor a part `remote` says is on a remote, which is never read.
/// Every folder these name is reached without a link, so no two name one
/// folder.
fn way_to(path: &Path, remote: impl Fn(&Path) -> bool) -> (Vec<PathBuf>, PathBuf) {
    let mut links = Vec::new();
    let mut at = PathBuf::new();
    let mut ahead = Vec::new();
    take_ahead(&mut at, &mut ahead, path);
    while let Some(part) = ahead.pop() {
        let Some(name) = part else {
            at.pop();
            continue;
        };
        let next = at.join(&name);
        if links.len() < MAX_LINKS
            && !remote(&next)
            && std::fs::symlink_metadata(&next).is_ok_and(|meta| meta.file_type().is_symlink())
            && let Ok(target) = std::fs::read_link(&next)
        {
            links.push(next);
            take_ahead(&mut at, &mut ahead, &os_path::plain(target));
        } else {
            at = next;
        }
    }
    (links, at)
}

/// Put `path`'s parts ahead, its first on top (`None` for `..`): a path
/// from a root goes from there, another from where the way is. A root
/// without a drive (Windows' `\dir`) is the drive's of where the way is.
fn take_ahead(at: &mut PathBuf, ahead: &mut Vec<Option<OsString>>, path: &Path) {
    let mut root = PathBuf::new();
    let mut parts = Vec::new();
    for part in path.components() {
        match part {
            Component::Prefix(_) => root.push(part),
            Component::RootDir => {
                if root.as_os_str().is_empty()
                    && let Some(drive @ Component::Prefix(_)) = at.components().next()
                {
                    root.push(drive);
                }
                root.push(part);
            }
            Component::CurDir => {}
            Component::ParentDir => parts.push(None),
            Component::Normal(name) => parts.push(Some(name.to_owned())),
        }
    }
    if !root.as_os_str().is_empty() {
        *at = root;
    }
    ahead.extend(parts.into_iter().rev());
}

/// The device `folder` is on, as a stat says.
fn device_of(folder: &Path) -> Option<u64> {
    skwir_core::fs::stat::id_of(folder).map(|id| id.dev)
}

/// Whether a folder of `fixed` (`Sources::fixed`) is on another device
/// than it was: its volume, read-only, mounted again in its place, what it
/// holds may name other files. A stat each, of this machine's folders
/// (what is on a remote is never followed): on the reading thread.
pub fn remounted(fixed: &[(PathBuf, Option<u64>)]) -> bool {
    fixed
        .iter()
        .any(|(folder, device)| device_of(folder) != *device)
}

/// The folder to watch for `source`: a file's folder, a folder itself,
/// else the nearest folder above that is there.
fn folder_of(source: &Source) -> Option<PathBuf> {
    let start = match source {
        Source::File(file) => file.parent()?,
        Source::Folder(folder) => folder,
    };
    start
        .ancestors()
        .find(|a| a.is_dir())
        .map(Path::to_path_buf)
}

/// Whether an event's `path` concerns `source`: the file itself, a file
/// of the folder, or a folder on the way to either, made, moved or gone.
fn concerns(source: &Source, path: &Path) -> bool {
    match source {
        Source::File(file) => file == path || file.starts_with(path),
        Source::Folder(folder) => folder.starts_with(path) || path.parent() == Some(folder),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{Source, concerns, folder_of};

    /// Waits until the watcher tells, then lets the burst pass: one call a
    /// burst.
    #[cfg(unix)]
    fn told_on(rx: &crossbeam_channel::Receiver<()>, what: &str) {
        use std::time::Duration;
        rx.recv_timeout(Duration::from_secs(10))
            .unwrap_or_else(|_| panic!("{what} was not told"));
        while rx.recv_timeout(Duration::from_millis(600)).is_ok() {}
    }

    /// A tree laid out as NixOS lays out `/etc`, under `root`:
    /// `etc/fstab` a link to `etc/static/fstab`, `etc/static` a link
    /// (relative) to a generation's `etc` in the store, its `fstab` a
    /// link to the file in the store. `generation` names it; its file
    /// says `content`. The link `etc/static` is made, or re-pointed, as
    /// NixOS's activation does: beside it, then renamed onto it.
    #[cfg(unix)]
    fn nixos_like(root: &Path, generation: &str, content: &str) -> std::path::PathBuf {
        use std::os::unix::fs::symlink;
        let store = root.join("store");
        let file = store.join(format!("{generation}-fstab"));
        let etc_of = store.join(format!("{generation}-etc")).join("etc");
        std::fs::create_dir_all(&etc_of).unwrap();
        std::fs::write(&file, content).unwrap();
        symlink(&file, etc_of.join("fstab")).unwrap();
        let etc = root.join("etc");
        std::fs::create_dir_all(&etc).unwrap();
        let beside = etc.join("static.tmp");
        symlink(format!("../store/{generation}-etc/etc"), &beside).unwrap();
        std::fs::rename(&beside, etc.join("static")).unwrap();
        let fstab = etc.join("fstab");
        if std::fs::symlink_metadata(&fstab).is_err() {
            symlink(etc.join("static").join("fstab"), &fstab).unwrap();
        }
        fstab
    }

    /// The way to a file goes link by link, each where it is: NixOS's
    /// `/etc/fstab` through `/etc/static` into the store. A part not
    /// there ends nothing; a loop of links ends.
    #[cfg(unix)]
    #[test]
    fn the_way_to_a_file_goes_link_by_link() {
        use super::{MAX_LINKS, way_to};
        let dir = tempfile::tempdir().unwrap();
        let root = skwir_core::os_path::canonical(dir.path()).unwrap();
        let fstab = nixos_like(&root, "first", "nas:/share /mnt/nas nfs noauto 0 0\n");
        let etc = root.join("etc");
        let generation = root.join("store").join("first-etc").join("etc");
        let file = root.join("store").join("first-fstab");
        assert_eq!(
            way_to(&fstab, |_| false),
            (
                vec![fstab.clone(), etc.join("static"), generation.join("fstab")],
                file.clone()
            )
        );
        assert_eq!(
            way_to(&file, |_| false),
            (vec![], file.clone()),
            "no link: itself"
        );
        assert_eq!(
            way_to(&etc.join("static").join("gone").join("fstab"), |_| false),
            (
                vec![etc.join("static")],
                generation.join("gone").join("fstab")
            ),
            "what is not there is named as written"
        );
        std::os::unix::fs::symlink("loop-b", root.join("loop-a")).unwrap();
        std::os::unix::fs::symlink("loop-a", root.join("loop-b")).unwrap();
        assert_eq!(way_to(&root.join("loop-a"), |_| false).0.len(), MAX_LINKS);
    }

    /// Every link on the way counts, and a folder on a volume mounted
    /// read-only is not watched: on NixOS, `/etc` alone.
    #[cfg(unix)]
    #[test]
    fn what_is_read_only_is_not_watched() {
        use super::Followed;
        let dir = tempfile::tempdir().unwrap();
        let root = skwir_core::os_path::canonical(dir.path()).unwrap();
        let fstab = nixos_like(&root, "first", "");
        let store = root.join("store");
        let followed = Followed::with(
            vec![Source::File(fstab)],
            |folder| folder.starts_with(&store),
            |_| false,
        );
        assert_eq!(
            followed.folders.into_iter().collect::<Vec<_>>(),
            [root.join("etc")]
        );
        assert!(
            followed
                .every
                .contains(&Source::File(root.join("etc").join("static")))
        );
        // The store's folders are kept with their device: a volume mounted
        // read-only may be mounted again in its place (NixOS's `/etc` as
        // an overlay), which the table tells.
        let fixed = followed.fixed;
        assert_eq!(
            fixed.iter().map(|(folder, _)| folder).collect::<Vec<_>>(),
            [&store, &store.join("first-etc").join("etc")]
        );
        assert!(!super::remounted(&fixed), "as it was");
        let mut moved = fixed.clone();
        moved[1].1 = moved[1].1.map(|device| device ^ 1);
        assert!(super::remounted(&moved), "another device there now");
    }

    /// NixOS's switch re-points `/etc/static`: told, though nothing in
    /// the store is watched.
    #[cfg(unix)]
    #[test]
    fn a_link_re_pointed_on_the_way_is_told() {
        use super::{Followed, Sources};
        use std::time::Duration;
        let dir = tempfile::tempdir().unwrap();
        let root = skwir_core::os_path::canonical(dir.path()).unwrap();
        let fstab = nixos_like(&root, "first", "");
        let store = root.join("store");
        let (tx, rx) = crossbeam_channel::unbounded();
        let mut sources = Sources::start(move || tx.send(()).is_ok()).unwrap();
        let follow = |sources: &mut Sources| {
            sources.follow(
                Followed::with(
                    vec![Source::File(fstab.clone())],
                    |folder| folder.starts_with(&store),
                    |_| false,
                ),
                1,
            );
        };
        follow(&mut sources);
        assert_eq!(sources.watched(), [root.join("etc")]);
        // Beside it: nothing.
        std::fs::write(root.join("etc").join("hosts"), "").unwrap();
        assert!(rx.recv_timeout(Duration::from_millis(900)).is_err());
        nixos_like(&root, "second", "nas:/share /mnt/nas nfs noauto 0 0\n");
        told_on(&rx, "/etc/static re-pointed");
        follow(&mut sources);
        assert_eq!(sources.watched(), [root.join("etc")]);
    }

    /// A folder watched, removed and made again before the servers are
    /// read: watched again, though it was before.
    #[cfg(unix)]
    #[test]
    fn a_folder_made_again_is_watched_again() {
        use super::{Followed, Sources};
        let dir = tempfile::tempdir().unwrap();
        let root = skwir_core::os_path::canonical(dir.path()).unwrap();
        let rclone = root.join("rclone");
        let config = rclone.join("rclone.conf");
        std::fs::create_dir(&rclone).unwrap();
        std::fs::write(&config, "[nas]\ntype = sftp\n").unwrap();
        let (tx, rx) = crossbeam_channel::unbounded();
        let mut sources = Sources::start(move || tx.send(()).is_ok()).unwrap();
        let follow = |sources: &mut Sources| {
            sources.follow(Followed::of(vec![Source::File(config.clone())]), 1);
        };
        follow(&mut sources);
        assert_eq!(sources.watched(), std::slice::from_ref(&rclone));
        std::fs::remove_dir_all(&rclone).unwrap();
        std::fs::create_dir(&rclone).unwrap();
        told_on(&rx, "rclone's folder removed");
        follow(&mut sources);
        std::fs::write(&config, "[box]\ntype = box\n").unwrap();
        told_on(&rx, "a file written in the folder made again");
    }

    /// What is on a remote is neither walked nor watched: its links are
    /// not read (a server that stopped answering would hold the walk) and
    /// it counts as missed, read again as a window comes to the front.
    /// What an older read found never takes the place of a later's.
    #[cfg(unix)]
    #[test]
    fn what_is_on_a_remote_is_neither_walked_nor_watched() {
        use super::{Followed, Sources};
        let dir = tempfile::tempdir().unwrap();
        let root = skwir_core::os_path::canonical(dir.path()).unwrap();
        let served = root.join("served");
        let here = root.join("here");
        std::fs::create_dir_all(served.join("rclone's")).unwrap();
        std::fs::create_dir_all(&here).unwrap();
        std::os::unix::fs::symlink(&here, served.join("link")).unwrap();
        let on_remote = |path: &Path| path.starts_with(&served);
        let followed = Followed::with(
            vec![
                Source::File(served.join("link").join("rclone.conf")),
                Source::File(here.join("user-places.xbel")),
            ],
            |_| false,
            on_remote,
        );
        assert!(followed.unwatched);
        assert!(
            followed
                .every
                .contains(&Source::File(served.join("link").join("rclone.conf"))),
            "its link not read"
        );
        assert_eq!(
            followed.folders.iter().collect::<Vec<_>>(),
            [&here],
            "nothing on the remote watched"
        );
        let (tx, _rx) = crossbeam_channel::unbounded();
        let mut sources = Sources::start(move || tx.send(()).is_ok()).unwrap();
        sources.follow(followed, 2);
        assert!(sources.misses());
        assert_eq!(sources.watched(), std::slice::from_ref(&here));
        sources.follow(
            Followed::with(vec![Source::File(root.join("older"))], |_| false, |_| false),
            1,
        );
        assert!(sources.misses(), "an older read's never in its place");
        assert_eq!(sources.watched(), std::slice::from_ref(&here));
    }

    /// A folder watched only after a read (its file's folder made since)
    /// is said so, that the servers be read once more with the watch in
    /// place; never at the first read, nor for folders watched already.
    #[cfg(unix)]
    #[test]
    fn a_folder_watched_late_is_said_so() {
        use super::{Followed, Sources};
        let tmp = tempfile::tempdir().unwrap();
        let root = skwir_core::os_path::canonical(tmp.path()).unwrap();
        let config = root.join("its configuration");
        let file = config.join("servers");
        let (tx, _rx) = crossbeam_channel::unbounded();
        let mut sources = Sources::start(move || tx.send(()).is_ok()).unwrap();
        assert!(
            !sources.follow(Followed::of(vec![Source::File(file.clone())]), 1),
            "the first read watches what it found"
        );
        assert_eq!(sources.watched(), std::slice::from_ref(&root));
        assert!(
            !sources.follow(Followed::of(vec![Source::File(file.clone())]), 2),
            "the same folder watched again"
        );
        std::fs::create_dir(&config).unwrap();
        assert!(
            sources.follow(Followed::of(vec![Source::File(file.clone())]), 3),
            "its folder made since is watched late"
        );
        assert_eq!(sources.watched(), std::slice::from_ref(&config));
        assert!(!sources.follow(Followed::of(vec![Source::File(file)]), 4));
    }

    /// A link to a path from the root without a drive (`\dir\file`)
    /// leads to that drive's: where the way is.
    #[cfg(windows)]
    #[test]
    fn a_root_without_a_drive_is_the_drive_of_where_the_way_is() {
        let mut at = std::path::PathBuf::from(r"D:\Users\me");
        let mut ahead = Vec::new();
        super::take_ahead(&mut at, &mut ahead, Path::new(r"\dotfiles\rclone.conf"));
        assert_eq!(at, Path::new(r"D:\"));
        assert_eq!(ahead, [Some("rclone.conf".into()), Some("dotfiles".into())]);
    }

    /// A file's folder is watched, else the nearest above that is there;
    /// a folder itself.
    #[test]
    fn what_is_watched_is_the_nearest_folder_there() {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("config");
        std::fs::create_dir(&config).unwrap();
        let file = |p: &Path| Source::File(p.to_path_buf());
        assert_eq!(
            folder_of(&file(&config.join("rclone.conf"))).as_deref(),
            Some(config.as_path())
        );
        assert_eq!(
            folder_of(&file(&config.join("rclone").join("rclone.conf"))).as_deref(),
            Some(config.as_path()),
            "rclone's folder not made yet"
        );
        assert_eq!(
            folder_of(&Source::Folder(config.join("remoteview"))).as_deref(),
            Some(config.as_path())
        );
        assert_eq!(
            folder_of(&Source::Folder(config.clone())).as_deref(),
            Some(config.as_path())
        );
    }

    /// The file and the folders on the way to it count; a file beside it
    /// does not. A folder's files count, not what is below them.
    #[test]
    fn only_what_leads_to_a_source_counts() {
        let conf = Source::File("/home/me/.config/rclone/rclone.conf".into());
        assert!(concerns(
            &conf,
            Path::new("/home/me/.config/rclone/rclone.conf")
        ));
        assert!(concerns(&conf, Path::new("/home/me/.config/rclone")));
        assert!(!concerns(
            &conf,
            Path::new("/home/me/.config/rclone/rclone.conf.bak")
        ));
        assert!(!concerns(&conf, Path::new("/home/me/.config/gtk-3.0")));
        let kde = Source::Folder("/home/me/.local/share/remoteview".into());
        assert!(concerns(
            &kde,
            Path::new("/home/me/.local/share/remoteview/nas.desktop")
        ));
        assert!(concerns(
            &kde,
            Path::new("/home/me/.local/share/remoteview")
        ));
        assert!(concerns(&kde, Path::new("/home/me/.local/share")));
        assert!(!concerns(
            &kde,
            Path::new("/home/me/.local/share/recently-used.xbel")
        ));
        assert!(!concerns(
            &kde,
            Path::new("/home/me/.local/share/remoteview/a/b")
        ));
    }

    /// A file written by another program, its folder made after, a file of
    /// a folder followed, a link's target replaced: each is told once
    /// settled; what is beside them is not.
    #[cfg(unix)]
    #[test]
    fn a_change_by_another_program_is_told() {
        use super::{Followed, Sources};
        use std::time::{Duration, Instant};
        let dir = tempfile::tempdir().unwrap();
        let root = skwir_core::os_path::canonical(dir.path()).unwrap();
        let (tx, rx) = crossbeam_channel::unbounded();
        let mut sources = Sources::start(move || tx.send(()).is_ok()).unwrap();
        let told = |what: &str| told_on(&rx, what);
        let config = root.join("rclone").join("rclone.conf");
        let network = root.join("remoteview");
        let follow = |sources: &mut Sources| {
            sources.follow(
                Followed::of(vec![
                    Source::File(config.clone()),
                    Source::Folder(network.clone()),
                ]),
                1,
            );
        };
        follow(&mut sources);
        assert_eq!(sources.watched(), std::slice::from_ref(&root));

        // Beside them: nothing.
        std::fs::write(root.join("recently-used.xbel"), "x").unwrap();
        assert!(rx.recv_timeout(Duration::from_millis(900)).is_err());

        // `rclone config` makes its folder, then its file.
        std::fs::create_dir(config.parent().unwrap()).unwrap();
        std::fs::write(&config, "[nas]\ntype = sftp\n").unwrap();
        told("rclone's folder made");
        // Read again: what it names now is followed.
        follow(&mut sources);
        assert_eq!(
            sources.watched(),
            [root.clone(), config.parent().unwrap().to_path_buf()]
        );
        // Written again as rclone writes it: beside it, then in its place.
        let beside = config.with_extension("conf.new");
        std::fs::write(&beside, "[nas]\ntype = sftp\n[box]\ntype = box\n").unwrap();
        std::fs::rename(&beside, &config).unwrap();
        told("rclone's file replaced");

        // Add Network Folder makes KDE's folder and a link in it.
        std::fs::create_dir(&network).unwrap();
        std::fs::write(network.join("Team's share.desktop"), "[Desktop Entry]\n").unwrap();
        told("KDE's folder made");
        follow(&mut sources);
        std::fs::remove_file(network.join("Team's share.desktop")).unwrap();
        told("a network folder removed");

        // A file kept elsewhere, linked to: its target replaced.
        let dotfiles = root.join("dotfiles");
        std::fs::create_dir(&dotfiles).unwrap();
        let places = root.join("user-places.xbel");
        std::fs::write(dotfiles.join("user-places.xbel"), "<xbel/>").unwrap();
        std::os::unix::fs::symlink(dotfiles.join("user-places.xbel"), &places).unwrap();
        sources.follow(Followed::of(vec![Source::File(places)]), 2);
        let deadline = Instant::now() + Duration::from_secs(1);
        while rx.recv_timeout(Duration::from_millis(600)).is_ok() {
            assert!(Instant::now() < deadline);
        }
        assert!(sources.watched().contains(&dotfiles));
        let beside = dotfiles.join(".user-places.xbel.new");
        std::fs::write(&beside, "<xbel></xbel>").unwrap();
        std::fs::rename(&beside, dotfiles.join("user-places.xbel")).unwrap();
        told("the link's target replaced");
        assert!(
            !sources.watched().contains(&network),
            "what is no longer followed is let go of"
        );
    }
}
