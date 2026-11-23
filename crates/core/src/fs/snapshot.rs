//! Whether a folder is a btrfs snapshot (ADR-125, 14): a read-only
//! subvolume, a copy of what it was taken of, which walks pass over below
//! the folder they start at (`walks::Below::passes_over`). Told by what it
//! is, never by its name: snapper's `/.snapshots`, timeshift's and btrbk's
//! alike. Linux's; elsewhere no folder is one.

use std::path::Path;

/// The inode a stat says of every btrfs subvolume's top folder
/// (`BTRFS_FIRST_FREE_OBJECTID`). Never a listing's: btrfs lists a
/// subvolume by its id.
pub const SUBVOLUME_ROOT: u64 = 256;

/// Whether a walk of `root` may meet a snapshot: btrfs holds `root`, or is
/// mounted below it, as the system's mount table says (read once a walk).
/// Elsewhere no folder is asked anything.
#[cfg(target_os = "linux")]
pub fn may_hold(root: &Path) -> bool {
    let Ok(table) = std::fs::read("/proc/self/mountinfo") else {
        return false;
    };
    let root = crate::os_path::canonical(root).unwrap_or_else(|_| root.to_path_buf());
    let mounts = crate::fs::volume::mountinfo(&table);
    let holding = mounts
        .iter()
        .filter(|m| root.starts_with(&m.point))
        .max_by_key(|m| m.point.components().count());
    holding.is_some_and(|m| m.fstype == "btrfs")
        || mounts
            .iter()
            .any(|m| m.fstype == "btrfs" && m.point.starts_with(&root))
}

/// No walk meets a btrfs snapshot elsewhere than Linux.
#[cfg(not(target_os = "linux"))]
pub fn may_hold(_: &Path) -> bool {
    false
}

/// Whether `dir` is a read-only btrfs subvolume. Its inode (`ino`, as a
/// stat says it; `None`: read here) is looked at first, so only a
/// subvolume's top folder is opened and asked; on another filesystem a
/// folder of that inode is opened, and the question refused.
#[cfg(target_os = "linux")]
pub fn is_snapshot(dir: &Path, ino: Option<u64>) -> bool {
    use std::os::fd::AsRawFd as _;
    use std::os::unix::fs::MetadataExt as _;

    use rustix::fs::{Mode, OFlags};

    /// `BTRFS_IOC_SUBVOL_GETFLAGS`: `_IOR(0x94, 25, __u64)`.
    const SUBVOL_GETFLAGS: u64 = 0x8008_9419;
    /// `BTRFS_SUBVOL_RDONLY`.
    const SUBVOL_RDONLY: u64 = 1 << 1;

    let ino = match ino {
        Some(ino) => ino,
        None => match std::fs::symlink_metadata(dir) {
            Ok(meta) if meta.is_dir() => meta.ino(),
            _ => return false,
        },
    };
    if ino != SUBVOLUME_ROOT {
        return false;
    }
    let Ok(folder) = rustix::fs::open(
        dir,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    ) else {
        return false;
    };
    let mut flags: u64 = 0;
    #[allow(
        unsafe_code,
        reason = "btrfs's flags of a subvolume are asked by an ioctl, which rustix does not name"
    )]
    // SAFETY: the descriptor is a folder held open above; the ioctl writes
    // one u64 into `flags`, which lives until it returns. Any other
    // filesystem refuses it (ENOTTY), and nothing is written.
    let done = unsafe {
        libc::ioctl(
            folder.as_raw_fd(),
            SUBVOL_GETFLAGS as libc::Ioctl,
            &raw mut flags,
        )
    };
    done == 0 && flags & SUBVOL_RDONLY != 0
}

/// No folder is a btrfs snapshot elsewhere than Linux.
#[cfg(not(target_os = "linux"))]
pub fn is_snapshot(_: &Path, _: Option<u64>) -> bool {
    false
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A writable subvolume and a read-only snapshot of it, made with the
    /// system's `btrfs` in a folder on btrfs, taken apart when dropped
    /// (made writable, emptied, removed: all a user may do with their own).
    struct Subvolumes {
        dir: tempfile::TempDir,
        volume: PathBuf,
        snapshot: PathBuf,
    }

    impl Subvolumes {
        /// `None`, saying why, where no btrfs or no `btrfs` program is.
        fn made() -> Option<Subvolumes> {
            let base = dirs::home_dir()?.join(".cache").join("skwir-tests");
            std::fs::create_dir_all(&base).ok()?;
            let dir = tempfile::Builder::new()
                .prefix("snapshots-")
                .tempdir_in(base)
                .ok()?;
            let btrfs = |args: &[&std::ffi::OsStr]| {
                // The test's own program, never one Skwir starts.
                #[allow(clippy::disallowed_methods, reason = "a test's own program")]
                std::process::Command::new("btrfs")
                    .args(args)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .is_ok_and(|s| s.success())
            };
            let volume = dir.path().join("volume");
            let snapshot = dir.path().join("snapshot");
            if !btrfs(&["subvolume".as_ref(), "create".as_ref(), volume.as_os_str()]) {
                eprintln!("skipped: no btrfs here, or no `btrfs` program");
                return None;
            }
            std::fs::write(volume.join("report.txt"), "fixture").ok()?;
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
            Some(Subvolumes {
                dir,
                volume,
                snapshot,
            })
        }
    }

    impl Drop for Subvolumes {
        fn drop(&mut self) {
            #[allow(clippy::disallowed_methods, reason = "a test's own program")]
            let _ = std::process::Command::new("btrfs")
                .args(["property", "set"])
                .arg(&self.snapshot)
                .args(["ro", "false"])
                .status();
            for volume in [&self.snapshot, &self.volume] {
                let _ = std::fs::remove_file(volume.join("report.txt"));
                let _ = std::fs::remove_dir(volume);
            }
        }
    }

    #[test]
    fn a_read_only_subvolume_is_a_snapshot_and_its_source_is_not() {
        let Some(made) = Subvolumes::made() else {
            return;
        };
        assert!(is_snapshot(&made.snapshot, None));
        assert!(is_snapshot(&made.snapshot, Some(SUBVOLUME_ROOT)));
        assert!(!is_snapshot(&made.volume, None), "writable");
        assert!(!is_snapshot(made.dir.path(), None), "a folder");
        // Told by its inode first: one a stat says is not a subvolume's
        // top is never opened.
        assert!(!is_snapshot(&made.snapshot, Some(5)));
        assert!(may_hold(made.dir.path()), "btrfs holds it");
        assert!(!is_snapshot(&made.dir.path().join("gone"), None));
    }
}
