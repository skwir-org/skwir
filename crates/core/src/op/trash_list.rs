//! The trash a plan reads on Linux (freedesktop.org's trash
//! specification): the home trash and each volume's (`.Trash/<uid>`,
//! `.Trash-<uid>` at its top), each `.trashinfo` read as the `trash` crate
//! reads it. The crate's own listing looks at the top of every mount, and
//! a mount whose server stopped answering holds that look for good
//! (ADR-077): an Undo of a trash on this machine waited on a server gone.
//! So a network mount's trash is read only when a step names a path there
//! ([`Listed::near`]), which reaches that server anyway; and no automount
//! is looked into.

use std::collections::HashSet;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use crate::fs::volume::Mounted;

/// The mounts, the longest point first, and which trashes were read.
pub(super) struct Listed {
    mounts: Vec<Mounted>,
    /// The tops whose trashes are in the listing.
    read: HashSet<PathBuf>,
}

impl Listed {
    /// The items of the home trash and of each volume of this machine's
    /// own (no network mount, no automount).
    pub(super) fn here() -> (Listed, Vec<trash::TrashItem>) {
        let mut listed = Listed {
            mounts: mounts(),
            read: HashSet::new(),
        };
        let mut items: Vec<trash::TrashItem> = crate::xdg::data_home()
            .map(|data| read(&data.join("Trash"), None))
            .unwrap_or_default();
        let here: Vec<PathBuf> = listed
            .mounts
            .iter()
            .filter(|m| own(m))
            .map(|m| m.point.clone())
            .collect();
        for top in here {
            items.extend(listed.read_top(&top));
        }
        (listed, items)
    }

    /// The items of the trashes of the volume holding `path`, when not
    /// read yet: a network mount's, which [`Listed::here`] leaves out.
    pub(super) fn near(&mut self, path: &Path) -> Vec<trash::TrashItem> {
        let Some(top) = self.top_of(path) else {
            return Vec::new();
        };
        self.read_top(&top)
    }

    fn top_of(&self, path: &Path) -> Option<PathBuf> {
        self.mounts
            .iter()
            .find(|m| path.starts_with(&m.point))
            .filter(|m| m.fstype != "autofs")
            .map(|m| m.point.clone())
    }

    fn read_top(&mut self, top: &Path) -> Vec<trash::TrashItem> {
        if !self.read.insert(top.to_path_buf()) {
            return Vec::new();
        }
        volume_trashes(top)
            .iter()
            .flat_map(|trash| read(trash, Some(top)))
            .collect()
    }
}

/// What the trashes of this machine's own volumes hold
/// ([`Listed::here`]): what a test cleans up after itself.
pub fn items_here() -> Vec<trash::TrashItem> {
    Listed::here().1
}

/// The trash folders of the user's on this machine's own volumes: the
/// home trash and each volume's that exists, as [`Listed::here`] reads
/// them (what the trash place lists). Asks each volume: call it from a
/// worker.
pub fn folders_here() -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = crate::xdg::data_home()
        .map(|data| data.join("Trash"))
        .into_iter()
        .collect();
    for mount in mounts().iter().filter(|m| own(m)) {
        for trash in volume_trashes(&mount.point) {
            if !found.contains(&trash) {
                found.push(trash);
            }
        }
    }
    found
}

/// The mount table, the longest point first.
fn mounts() -> Vec<Mounted> {
    let mut mounts = std::fs::read("/proc/self/mountinfo")
        .map(|text| crate::fs::volume::mountinfo(&text))
        .unwrap_or_default();
    mounts.retain(|m| !m.point.as_os_str().is_empty());
    mounts.sort_by_key(|m| std::cmp::Reverse(m.point.as_os_str().len()));
    mounts
}

/// Whether the mount `m` is this machine's own: no network's, whose
/// server may not answer, and no automount, which a look would mount.
fn own(m: &Mounted) -> bool {
    !crate::fs::reach::network(&m.fstype, &m.source) && m.fstype != "autofs"
}

/// The trashes of the volume mounted at `top` that exist: `.Trash/<uid>`
/// where `.Trash` is a sticky folder and no link, and `.Trash-<uid>`.
fn volume_trashes(top: &Path) -> Vec<PathBuf> {
    let uid = rustix::process::getuid().as_raw();
    let mut found = Vec::new();
    let shared = top.join(".Trash");
    if std::fs::symlink_metadata(&shared).is_ok_and(|m| m.is_dir() && m.mode() & 0o1000 != 0) {
        let own = shared.join(uid.to_string());
        if std::fs::symlink_metadata(&own).is_ok_and(|m| m.is_dir()) {
            found.push(own);
        }
    }
    let own = top.join(format!(".Trash-{uid}"));
    if std::fs::symlink_metadata(&own).is_ok_and(|m| m.is_dir()) {
        found.push(own);
    }
    found
}

/// The items `trash` speaks of in its `info`, a relative `Path` read from
/// `top`: each `.trashinfo` that says where its file came from, as the
/// crate lists it (`id` the record's path, the deletion date in local
/// time; `-1` where it says none).
fn read(trash: &Path, top: Option<&Path>) -> Vec<trash::TrashItem> {
    let Ok(infos) = std::fs::read_dir(trash.join("info")) else {
        return Vec::new();
    };
    let mut items = Vec::new();
    for info in infos.flatten() {
        let path = info.path();
        if !info.file_type().is_ok_and(|t| t.is_file())
            || !info.file_name().as_encoded_bytes().ends_with(b".trashinfo")
        {
            continue;
        }
        let Ok(bytes) = crate::fs::file::read_regular(&path, 64 * 1024) else {
            continue;
        };
        let Some((original, deleted)) = std::str::from_utf8(&bytes)
            .ok()
            .and_then(|text| super::records::parse_trash_info(text, top))
        else {
            continue;
        };
        let (Some(name), Some(parent)) = (original.file_name(), original.parent()) else {
            continue;
        };
        items.push(trash::TrashItem {
            id: path.clone().into_os_string(),
            name: name.to_os_string(),
            original_parent: parent.to_path_buf(),
            time_deleted: deleted.and_then(local_seconds).unwrap_or(-1),
        });
    }
    items
}

/// `2026-10-21T15:04:05`, a local time, in seconds since the epoch.
fn local_seconds(date: &str) -> Option<i64> {
    use chrono::TimeZone;
    let naive = chrono::NaiveDateTime::parse_from_str(date, "%Y-%m-%dT%H:%M:%S").ok()?;
    Some(
        chrono::Local
            .from_local_datetime(&naive)
            .earliest()?
            .timestamp(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mounted(point: &str, fstype: &str, source: &str) -> Mounted {
        crate::fs::volume::mountinfo(
            format!("36 25 0:1 / {point} rw - {fstype} {source} rw\n").as_bytes(),
        )
        .pop()
        .unwrap()
    }

    /// A network mount's trash and an automount are left out of what is
    /// read unasked; a volume of this machine's is not.
    #[test]
    fn a_network_mount_s_trash_is_read_only_when_named() {
        assert!(own(&mounted("/media/stick", "vfat", "/dev/sdb1")));
        assert!(own(&mounted("/media/disk", "fuseblk", "/dev/sdc1")));
        assert!(!own(&mounted("/home/me/srv", "fuse.sshfs", "me@host:/")));
        assert!(!own(&mounted("/mnt/share", "nfs4", "nas:/export")));
        assert!(!own(&mounted("/net", "autofs", "systemd-1")));
    }

    /// A volume's trash read as the crate reads it: its relative path from
    /// the top, its date in local time, a record of no path left out.
    #[test]
    fn a_volume_s_records_are_read_from_its_top() {
        let tmp = tempfile::tempdir().unwrap();
        let top = tmp.path();
        let trash = top.join(format!(".Trash-{}", rustix::process::getuid().as_raw()));
        std::fs::create_dir_all(trash.join("info")).unwrap();
        std::fs::create_dir_all(trash.join("files")).unwrap();
        std::fs::write(
            trash.join("info/it's a.txt.trashinfo"),
            "[Trash Info]\nPath=docs/it%27s%20a.txt\nDeletionDate=2026-10-04T09:30:00\n",
        )
        .unwrap();
        std::fs::write(trash.join("info/broken.trashinfo"), "[Trash Info]\n").unwrap();
        assert_eq!(volume_trashes(top), std::slice::from_ref(&trash));
        let items = read(&trash, Some(top));
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, "it's a.txt");
        assert_eq!(items[0].original_parent, top.join("docs"));
        assert_eq!(
            items[0].id,
            trash.join("info/it's a.txt.trashinfo").into_os_string()
        );
        assert_eq!(
            items[0].time_deleted,
            local_seconds("2026-10-04T09:30:00").unwrap()
        );
    }
}
