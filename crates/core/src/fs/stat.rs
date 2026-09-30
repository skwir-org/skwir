//! Lazy metadata via statx through a directory fd.

use std::borrow::Cow;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rustix::fd::AsFd;
use rustix::fs::{AtFlags, FileType, StatxFlags};

use crate::entry::{Entry, EntryId, EntryKind, LinkTarget, Provenance, Value, keys};

fn time(sec: i64, nsec: u32) -> Option<SystemTime> {
    if sec < 0 {
        return None;
    }
    Some(UNIX_EPOCH + Duration::new(sec as u64, nsec))
}

fn kind_of(mode: u32) -> EntryKind {
    match FileType::from_raw_mode(mode) {
        FileType::RegularFile => EntryKind::File,
        FileType::Directory => EntryKind::Directory,
        FileType::Symlink => EntryKind::Symlink,
        FileType::Fifo => EntryKind::Fifo,
        FileType::Socket => EntryKind::Socket,
        FileType::CharacterDevice => EntryKind::CharDevice,
        FileType::BlockDevice => EntryKind::BlockDevice,
        FileType::Unknown => EntryKind::Unknown,
    }
}

/// What the link `name` in `dirfd` leads to, followed to the end: `None`
/// when it leads nowhere. Only the type and the identity are asked of
/// what is there.
pub fn follow(dirfd: &impl AsFd, name: &OsStr) -> Option<LinkTarget> {
    let asked = StatxFlags::TYPE | StatxFlags::INO;
    let st = rustix::fs::statx(dirfd, name, AtFlags::empty(), asked).ok()?;
    Some(target_of(dirfd, name, &st))
}

fn target_of(dirfd: &impl AsFd, name: &OsStr, st: &rustix::fs::Statx) -> LinkTarget {
    LinkTarget {
        kind: kind_of(u32::from(st.stx_mode)),
        id: EntryId {
            dev: rustix::fs::makedev(st.stx_dev_major, st.stx_dev_minor),
            ino: st.stx_ino,
        },
        path: resolved(dirfd, name),
    }
}

/// Where the link `name` finally leads, through every link of a chain:
/// the kernel resolves the directory's descriptor (`/proc/self/fd`), so
/// the directory's own path is not needed.
fn resolved(dirfd: &impl AsFd, name: &OsStr) -> Option<PathBuf> {
    use rustix::fd::AsRawFd;
    let link = Path::new("/proc/self/fd")
        .join(dirfd.as_fd().as_raw_fd().to_string())
        .join(name);
    std::fs::canonicalize(link).ok()
}

/// The file read to show what is at `path`, for whoever holds a path (an
/// `EntryRef`) rather than an entry: what it finally leads to when it is
/// a link, `path` itself otherwise (`Entry::content` for a path). Its
/// name says the file's type: never type a path by its own `file_name()`,
/// a link is typed as what it opens. `None` when there is no file to
/// read: a folder, a pipe, a socket, a device, nothing there, or a link
/// to one of those. Reads the disk (an lstat, and a link's chain): on a
/// worker.
pub fn content_path(path: &Path) -> Option<Cow<'_, Path>> {
    let own = std::fs::symlink_metadata(path).ok()?;
    let (content, meta) = if own.file_type().is_symlink() {
        let target = std::fs::canonicalize(path).ok()?;
        let meta = std::fs::metadata(&target).ok()?;
        (Cow::Owned(target), meta)
    } else {
        (Cow::Borrowed(path), own)
    };
    meta.is_file().then_some(content)
}

/// statx `name` relative to `dirfd`; fill `entry` in place. Returns false if
/// gone.
///
/// The entry is the link itself (its identity, its kind), but what it says
/// of size, times, owner and permissions is what it leads to, as a link
/// opens that: a link that leads nowhere says its own.
pub fn stat_entry(dirfd: &impl AsFd, entry: &mut Entry) -> bool {
    let wanted = StatxFlags::BASIC_STATS | StatxFlags::BTIME;
    // Owned: the entry is filled while the name is used.
    let name = entry.name.clone();
    let name = name.as_os_str();
    let Ok(own) = rustix::fs::statx(dirfd, name, AtFlags::SYMLINK_NOFOLLOW, wanted) else {
        return false;
    };
    entry.id = EntryId {
        dev: rustix::fs::makedev(own.stx_dev_major, own.stx_dev_minor),
        ino: own.stx_ino,
    };
    entry.kind = kind_of(u32::from(own.stx_mode));
    let followed = (entry.kind == EntryKind::Symlink)
        .then(|| rustix::fs::statx(dirfd, name, AtFlags::empty(), wanted).ok())
        .flatten();
    entry.target = followed.as_ref().map(|st| target_of(dirfd, name, st));
    let st = followed.as_ref().unwrap_or(&own);
    let m = &mut entry.metadata;
    let p = Provenance::Filesystem;
    m.set(keys::SIZE, Value::UInt(st.stx_size), p.clone());
    m.set(keys::MODE, Value::UInt(u64::from(st.stx_mode)), p.clone());
    m.set(keys::NLINK, Value::UInt(u64::from(st.stx_nlink)), p.clone());
    m.set(keys::UID, Value::UInt(u64::from(st.stx_uid)), p.clone());
    m.set(keys::GID, Value::UInt(u64::from(st.stx_gid)), p.clone());
    if let Some(t) = time(st.stx_mtime.tv_sec, st.stx_mtime.tv_nsec) {
        m.set(keys::MODIFIED, Value::Time(t), p.clone());
    }
    if let Some(t) = time(st.stx_atime.tv_sec, st.stx_atime.tv_nsec) {
        m.set(keys::ACCESSED, Value::Time(t), p.clone());
    }
    if st.stx_mask & StatxFlags::BTIME.bits() != 0
        && let Some(t) = time(st.stx_btime.tv_sec, st.stx_btime.tv_nsec)
    {
        m.set(keys::CREATED, Value::Time(t), p.clone());
    }
    if entry.kind == EntryKind::Symlink
        && let Ok(target) = rustix::fs::readlinkat(dirfd, name, Vec::new())
    {
        m.set(
            keys::LINK_TARGET,
            Value::Text(target.to_string_lossy().into()),
            p,
        );
    }
    true
}

/// Stat a full path into a fresh entry (used by the watcher and by operations).
pub fn stat_path(path: &Path) -> Option<Entry> {
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        // The root has no parent to be stat-ed from: it is its own `.`.
        let root = rustix::fs::open(
            path,
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::DIRECTORY
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )
        .ok()?;
        let mut e = Entry::new(EntryId { dev: 0, ino: 0 }, ".", EntryKind::Unknown);
        if !stat_entry(&root, &mut e) {
            return None;
        }
        path.as_os_str().clone_into(&mut e.name);
        return Some(e);
    };
    let name: &OsStr = name;
    let dir = rustix::fs::open(
        parent,
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::DIRECTORY | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .ok()?;
    let mut e = Entry::new(EntryId { dev: 0, ino: 0 }, name, EntryKind::Unknown);
    stat_entry(&dir, &mut e).then_some(e)
}

/// The stable id of a path, without building an entry.
///
/// This is what an [`EntryId`] means everywhere: statx's device and inode,
/// without following a final symlink. [`stat_entry`] and [`stat_path`]
/// report the same; a listing's provisional id for a mount point is
/// replaced by this one when the entry is stat-ed.
pub fn id_of(path: &Path) -> Option<EntryId> {
    let st = rustix::fs::statx(
        rustix::fs::CWD,
        path,
        AtFlags::SYMLINK_NOFOLLOW,
        StatxFlags::INO,
    )
    .ok()?;
    Some(EntryId {
        dev: rustix::fs::makedev(st.stx_dev_major, st.stx_dev_minor),
        ino: st.stx_ino,
    })
}

#[cfg(test)]
mod tests {
    use super::{id_of, stat_path};
    use std::path::Path;

    #[test]
    fn the_root_and_a_folder_are_stat_ed_by_their_path() {
        let root = stat_path(Path::new("/")).expect("the root");
        assert!(root.is_dir());
        assert_eq!(root.name, "/");
        assert_eq!(Some(root.id), id_of(Path::new("/")));
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("it's a folder");
        std::fs::create_dir(&dir).unwrap();
        let entry = stat_path(&dir).expect("a folder");
        assert!(entry.is_dir());
        assert_eq!(entry.name, "it's a folder");
        assert_eq!(Some(entry.id), id_of(&dir));
    }
}
