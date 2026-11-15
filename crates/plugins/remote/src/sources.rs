//! Where the Network section's servers are read, followed (ADR-086,
//! amended 2026-11-15): rclone's configuration, Dolphin's places, KDE's
//! network folders, fstab. Each reader says where it read
//! (`connector::Named`), and what it names is watched: its folder, or the
//! nearest folder above that is there while it is not (`rclone config`
//! making `~/.config/rclone`), and the folder a link leads into (a dotfile
//! manager's), as writers replace a file rather than write into it. A
//! change, by any program, settles, then the servers are read again,
//! which follows what they then name.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use skwir_core::fs::Settled;

use crate::connector::Source;

/// How long changes are let settle before the servers are read again: an
/// editor saving, a tool writing several files.
const SETTLE: Duration = Duration::from_millis(400);

/// The sources followed, and the folders watched for them.
pub struct Sources {
    watcher: Settled,
    /// What counts among the events: shared with the watcher's handler.
    followed: Arc<Mutex<Vec<Source>>>,
    watched: HashSet<PathBuf>,
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
        })
    }

    /// Follow `sources` from now on, and what no longer is no more.
    pub fn follow(&mut self, sources: Vec<Source>) {
        let mut every: Vec<Source> = Vec::new();
        for source in sources {
            // A link: the file it leads to is replaced, in its own folder.
            if let Source::File(file) = &source
                && let Ok(target) = skwir_core::os_path::canonical(file)
                && target != *file
            {
                every.push(Source::File(target));
            }
            every.push(source);
        }
        every.sort();
        every.dedup();
        let wanted: HashSet<PathBuf> = every.iter().filter_map(folder_of).collect();
        for gone in self
            .watched
            .difference(&wanted)
            .cloned()
            .collect::<Vec<_>>()
        {
            self.watcher.unwatch(&gone);
            self.watched.remove(&gone);
        }
        for folder in wanted {
            if !self.watched.contains(&folder) && self.watcher.watch(&folder) {
                self.watched.insert(folder);
            }
        }
        *self.followed.lock() = every;
    }

    /// The folders watched now: what a test asks.
    #[cfg(all(test, unix))]
    fn watched(&self) -> Vec<PathBuf> {
        let mut watched: Vec<PathBuf> = self.watched.iter().cloned().collect();
        watched.sort();
        watched
    }
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
        use super::Sources;
        use std::time::{Duration, Instant};
        let root = tempfile::tempdir().unwrap();
        let (tx, rx) = crossbeam_channel::unbounded();
        let mut sources = Sources::start(move || tx.send(()).is_ok()).unwrap();
        let told = |what: &str| {
            rx.recv_timeout(Duration::from_secs(10))
                .unwrap_or_else(|_| panic!("{what} was not told"));
            // One call a burst.
            while rx.recv_timeout(Duration::from_millis(600)).is_ok() {}
        };
        let config = root.path().join("rclone").join("rclone.conf");
        let network = root.path().join("remoteview");
        let follow = |sources: &mut Sources| {
            sources.follow(vec![
                Source::File(config.clone()),
                Source::Folder(network.clone()),
            ]);
        };
        follow(&mut sources);
        assert_eq!(sources.watched(), [root.path().to_path_buf()]);

        // Beside them: nothing.
        std::fs::write(root.path().join("recently-used.xbel"), "x").unwrap();
        assert!(rx.recv_timeout(Duration::from_millis(900)).is_err());

        // `rclone config` makes its folder, then its file.
        std::fs::create_dir(config.parent().unwrap()).unwrap();
        std::fs::write(&config, "[nas]\ntype = sftp\n").unwrap();
        told("rclone's folder made");
        // Read again: what it names now is followed.
        follow(&mut sources);
        assert_eq!(
            sources.watched(),
            [
                root.path().to_path_buf(),
                config.parent().unwrap().to_path_buf()
            ]
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
        let dotfiles = root.path().join("dotfiles");
        std::fs::create_dir(&dotfiles).unwrap();
        let places = root.path().join("user-places.xbel");
        std::fs::write(dotfiles.join("user-places.xbel"), "<xbel/>").unwrap();
        std::os::unix::fs::symlink(dotfiles.join("user-places.xbel"), &places).unwrap();
        sources.follow(vec![Source::File(places)]);
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
