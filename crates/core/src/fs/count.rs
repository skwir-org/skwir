//! What folders hold, counted when someone asks (Properties' Count,
//! ADR-058): never for a listing, never on the UI thread.
//!
//! A walk that follows no link: a link counts as itself, as `du` counts
//! it, so a link to a big folder is not that folder twice. It stays on the
//! drive each folder asked about is on: a drive mounted inside is said, not
//! counted (`/` would otherwise count `/proc`). A file of several names is
//! counted once, by its identity, wherever its names are. What it has
//! counted so far is reported as it goes, so a big folder shows a growing
//! lower bound; a folder it cannot read makes the count a lower bound too.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::cancel::CancelToken;

/// What the counted entries hold.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Count {
    /// Their sizes added up: files', links' own; a folder's own size is
    /// not what it holds and is left out.
    pub size: u64,
    /// What they take on their drives (`stat::allocated`), folders'
    /// own blocks included, as the drive spends them.
    pub allocated: u64,
    /// Files, links and other entries that are not folders.
    pub files: u64,
    /// Folders inside the ones asked about (those not counted themselves).
    pub folders: u64,
    /// Folders it could not read: the count is at least this much.
    pub unreadable: u64,
    /// Drives mounted inside, left out.
    pub other_drives: u64,
    /// The walk is over (not stopped: a stopped count is not done).
    pub done: bool,
}

impl Count {
    /// Whether it says less than there is: still counting, or folders it
    /// could not read.
    pub fn lower_bound(&self) -> bool {
        !self.done || self.unreadable > 0
    }
}

/// How often `count` reports what it has so far.
pub const REPORT_EVERY: Duration = Duration::from_millis(150);

/// Count what `roots` hold: a folder what is inside it, a file itself. A
/// root that is a link counts what it leads to, as Properties shows it;
/// nothing inside is followed. Reports what it has to `report` every
/// `every`, and returns it when the walk ends or `cancel` stops it (then
/// not `done`). Reads the disk: on a worker.
pub fn count(
    roots: &[PathBuf],
    cancel: &CancelToken,
    every: Duration,
    mut report: impl FnMut(&Count),
) -> Count {
    let mut count = Count::default();
    let mut seen = Seen::default();
    let mut last = Instant::now();
    // Each folder still to read, with the drive it must be on.
    let mut folders: Vec<(PathBuf, Option<u64>)> = Vec::new();
    // A root inside another (a search shows a folder and a file in it) is
    // counted with it, once.
    let mut roots: Vec<&PathBuf> = roots.iter().collect();
    roots.sort();
    roots.dedup();
    let inside = |root: &Path, all: &[&PathBuf]| {
        all.iter()
            .any(|other| other.as_path() != root && root.starts_with(other))
    };
    let kept: Vec<&PathBuf> = roots
        .iter()
        .copied()
        .filter(|root| !inside(root, &roots))
        .collect();
    for root in kept {
        let Ok(meta) = std::fs::metadata(root) else {
            count.unreadable += 1;
            continue;
        };
        if meta.is_dir() {
            count.allocated += super::stat::allocated(&meta);
            folders.push((root.clone(), device(&meta)));
        } else if seen.first(&meta) {
            count.files += 1;
            count.size += meta.len();
            count.allocated += super::stat::allocated(&meta);
        }
    }
    while let Some((folder, drive)) = folders.pop() {
        if cancel.is_cancelled() {
            return count;
        }
        let Ok(entries) = std::fs::read_dir(&folder) else {
            count.unreadable += 1;
            continue;
        };
        for entry in entries {
            if cancel.is_cancelled() {
                return count;
            }
            let Ok(meta) = entry.and_then(|e| Ok((e.path(), e.metadata()?))) else {
                count.unreadable += 1;
                continue;
            };
            let (path, meta) = meta;
            if meta.is_dir() {
                if drive.is_some() && device(&meta) != drive {
                    count.other_drives += 1;
                    continue;
                }
                count.folders += 1;
                count.allocated += super::stat::allocated(&meta);
                folders.push((path, drive));
            } else if seen.first(&meta) {
                count.files += 1;
                count.size += meta.len();
                count.allocated += super::stat::allocated(&meta);
            }
            if last.elapsed() >= every {
                last = Instant::now();
                report(&count);
            }
        }
    }
    count.done = true;
    report(&count);
    count
}

/// The drive a file is on, where the system says it from its metadata.
// Always known on Unix; Windows says none (a drive mounted in a folder is
// a link there, which the walk never follows).
#[cfg_attr(unix, allow(clippy::unnecessary_wraps))]
fn device(meta: &std::fs::Metadata) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Some(meta.dev())
    }
    #[cfg(not(unix))]
    {
        // A drive mounted in a folder is a link there, never followed.
        let _ = meta;
        None
    }
}

/// The files of several names met so far, by identity.
#[derive(Default)]
struct Seen(HashSet<(u64, u64)>);

impl Seen {
    /// Whether the file whose metadata is `meta` is met for the first
    /// time: always for one of a single name. Windows says no count of
    /// names in `std`: there, each name counts.
    fn first(&mut self, meta: &std::fs::Metadata) -> bool {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            meta.nlink() < 2 || self.0.insert((meta.dev(), meta.ino()))
        }
        #[cfg(not(unix))]
        {
            let _ = (meta, &mut self.0);
            true
        }
    }
}

/// [`count`] of one path, unreported: for tests and callers that only
/// want the total.
pub fn count_one(path: &Path, cancel: &CancelToken) -> Count {
    count(&[path.to_path_buf()], cancel, Duration::MAX, |_| {})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_counts_what_it_holds_and_a_file_of_two_names_once() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("it's a café");
        std::fs::create_dir_all(root.join("sub/deeper")).unwrap();
        std::fs::write(root.join("a.txt"), vec![b'a'; 1000]).unwrap();
        std::fs::write(root.join("sub/b.bin"), vec![b'b'; 5000]).unwrap();
        std::fs::write(root.join("sub/deeper/c"), b"").unwrap();
        let first = count_one(&root, &CancelToken::new());
        assert_eq!(
            (first.files, first.folders, first.size, first.done),
            (3, 2, 6000, true)
        );
        assert!(!first.lower_bound());
        assert!(first.allocated >= 8192, "two files take a block each");
        #[cfg(unix)]
        {
            std::fs::hard_link(root.join("sub/b.bin"), root.join("b again")).unwrap();
            std::os::unix::fs::symlink(root.join("sub"), root.join("to sub")).unwrap();
            let linked = count_one(&root, &CancelToken::new());
            assert_eq!(
                (linked.files, linked.size),
                (
                    4,
                    6000 + "/sub".len() as u64 + root.as_os_str().len() as u64
                ),
                "the second name is not counted again; the link is itself"
            );
            // A link asked about is what it leads to.
            let through = count_one(&root.join("to sub"), &CancelToken::new());
            assert_eq!((through.files, through.folders), (2, 1));
        }
        // A folder and a file in it asked about together: the file once.
        let alone = count_one(&root, &CancelToken::new());
        let file = root.join("a.txt");
        let both = count(
            &[file.clone(), root.clone(), file],
            &CancelToken::new(),
            Duration::MAX,
            |_| {},
        );
        assert_eq!(both, alone);
    }

    #[test]
    fn a_stopped_count_is_not_done_and_reports_as_it_goes() {
        let tmp = tempfile::tempdir().unwrap();
        for i in 0..50 {
            std::fs::write(tmp.path().join(format!("{i}")), b"x").unwrap();
        }
        let cancel = CancelToken::new();
        cancel.cancel();
        let stopped = count_one(tmp.path(), &cancel);
        assert!(!stopped.done && stopped.lower_bound());
        let mut reports = Vec::new();
        let full = count(
            &[tmp.path().to_path_buf()],
            &CancelToken::new(),
            Duration::ZERO,
            |c| reports.push(*c),
        );
        assert_eq!(full.files, 50);
        assert!(reports.len() > 1, "reported while counting");
        assert!(reports.windows(2).all(|w| w[0].files <= w[1].files));
        assert_eq!(reports.last(), Some(&full));
    }

    #[test]
    #[cfg(unix)]
    fn a_folder_it_cannot_read_makes_a_lower_bound() {
        use std::os::unix::fs::PermissionsExt;
        if rustix::process::geteuid().is_root() {
            return; // root reads it all the same
        }
        let tmp = tempfile::tempdir().unwrap();
        let locked = tmp.path().join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::write(locked.join("x"), b"x").unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        let partial = count_one(tmp.path(), &CancelToken::new());
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            (partial.folders, partial.unreadable, partial.files),
            (1, 1, 0)
        );
        assert!(partial.done && partial.lower_bound());
    }
}
