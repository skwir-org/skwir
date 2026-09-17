//! Lazy metadata via statx through a directory fd.

use std::ffi::OsStr;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rustix::fd::AsFd;
use rustix::fs::{AtFlags, FileType, StatxFlags};

use crate::entry::{Entry, EntryId, EntryKind, Provenance, Value, keys};

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
        FileType::Unknown => EntryKind::Unknown,
        _ => EntryKind::Other,
    }
}

/// statx `name` relative to `dirfd`; fill `entry` in place. Returns false if gone.
pub fn stat_entry(dirfd: &impl AsFd, entry: &mut Entry) -> bool {
    let Ok(st) = rustix::fs::statx(
        dirfd,
        entry.name.as_os_str(),
        AtFlags::SYMLINK_NOFOLLOW,
        StatxFlags::BASIC_STATS | StatxFlags::BTIME,
    ) else {
        return false;
    };
    entry.id = EntryId {
        dev: rustix::fs::makedev(st.stx_dev_major, st.stx_dev_minor),
        ino: st.stx_ino,
    };
    entry.kind = kind_of(u32::from(st.stx_mode));
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
        && let Ok(target) = rustix::fs::readlinkat(dirfd, entry.name.as_os_str(), Vec::new())
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
    let parent = path.parent()?;
    let name: &OsStr = path.file_name()?;
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
