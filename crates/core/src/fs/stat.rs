//! Lazy metadata: an entry stat-ed by name in its folder held open
//! ([`Dir`]): statx on Linux, fstatat on other Unix systems, a handle's
//! records on Windows (ADR-036).

use std::borrow::Cow;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::dir::Dir;
use crate::entry::{Entry, EntryId, EntryKind, LinkTarget, Provenance, Value, keys};

/// What a stat says of a file, alike on every system; what only Unix
/// says (a mode, an owner) is `None` elsewhere.
struct Stat {
    id: EntryId,
    kind: EntryKind,
    mode: Option<u32>,
    size: u64,
    nlink: u64,
    uid: Option<u32>,
    gid: Option<u32>,
    modified: Option<SystemTime>,
    accessed: Option<SystemTime>,
    created: Option<SystemTime>,
    /// Marked hidden by the system (a Mac's `UF_HIDDEN`, Windows' hidden
    /// attribute).
    hidden: bool,
}

#[cfg(unix)]
fn time(sec: i64, nsec: i64) -> Option<SystemTime> {
    let (Ok(sec), Ok(nsec)) = (u64::try_from(sec), u32::try_from(nsec)) else {
        return None;
    };
    Some(std::time::UNIX_EPOCH + std::time::Duration::new(sec, nsec))
}

#[cfg(unix)]
fn kind_of(mode: rustix::fs::RawMode) -> EntryKind {
    super::dir::kind_of(rustix::fs::FileType::from_raw_mode(mode))
}

/// The device number an [`EntryId`] carries, from a stat's `st_dev`.
#[cfg(unix)]
pub(crate) fn device_number(dev: rustix::fs::Dev) -> u64 {
    #[cfg(target_os = "linux")]
    {
        dev
    }
    #[cfg(not(target_os = "linux"))]
    {
        // Signed on macOS: widened as `MetadataExt::dev` widens it, so
        // both say the same of a file.
        dev as u64
    }
}

/// When the file at `path` last changed, its bytes or what is said of it
/// (its inode's change time, which no one can set back), its metadata
/// `meta` read already. Unix reads it from `meta`; Windows asks the file
/// (`std` does not say it).
pub fn changed(path: &Path, meta: &std::fs::Metadata) -> Option<SystemTime> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let _ = path;
        time(meta.ctime(), meta.ctime_nsec())
    }
    #[cfg(windows)]
    {
        let file = super::win::open(path, false, !meta.file_type().is_symlink()).ok()?;
        super::win::info(&file).ok()?.changed
    }
}

/// What the file whose metadata is `meta` takes on its volume: its blocks
/// where the system says them (Unix), else its length to the next 4 KiB,
/// a volume's usual block. A record of a few bytes costs a block.
pub fn allocated(meta: &std::fs::Metadata) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        meta.blocks().saturating_mul(512)
    }
    #[cfg(not(unix))]
    {
        meta.len().div_ceil(BLOCK).saturating_mul(BLOCK)
    }
}

/// A volume's usual block: what a file of a few bytes takes on it.
pub const BLOCK: u64 = 4096;

/// The identity of the file at `path`, as [`id_of`] says it, its
/// metadata `meta` read already: through a link when `meta` says it is
/// no link (the file a link leads to), the link's own otherwise. Unix
/// reads it from `meta`; Windows asks the file (`std` does not say it).
pub fn identity(path: &Path, meta: &std::fs::Metadata) -> Option<EntryId> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let _ = path;
        Some(EntryId {
            dev: meta.dev(),
            ino: meta.ino(),
        })
    }
    #[cfg(windows)]
    {
        let file = super::win::open(path, false, !meta.file_type().is_symlink()).ok()?;
        super::win::identity(&file).ok()
    }
}

/// What the link `name` in `dir` says, as written.
#[cfg(unix)]
fn read_link_at(dir: &Dir, name: &OsStr) -> Option<std::ffi::OsString> {
    let target = rustix::fs::readlinkat(dir.fd(), name, Vec::new()).ok()?;
    Some(std::os::unix::ffi::OsStringExt::from_vec(
        target.into_bytes(),
    ))
}

#[cfg(target_os = "linux")]
mod sys {
    pub(super) use super::read_link_at;
    use rustix::fs::{AtFlags, StatxFlags};

    use super::{Dir, EntryId, OsStr, Path, PathBuf, Stat, kind_of, time};

    fn of(st: &rustix::fs::Statx) -> Stat {
        Stat {
            id: EntryId {
                dev: rustix::fs::makedev(st.stx_dev_major, st.stx_dev_minor),
                ino: st.stx_ino,
            },
            kind: kind_of(u32::from(st.stx_mode)),
            mode: Some(u32::from(st.stx_mode)),
            size: st.stx_size,
            nlink: u64::from(st.stx_nlink),
            uid: Some(st.stx_uid),
            gid: Some(st.stx_gid),
            modified: time(st.stx_mtime.tv_sec, i64::from(st.stx_mtime.tv_nsec)),
            accessed: time(st.stx_atime.tv_sec, i64::from(st.stx_atime.tv_nsec)),
            created: (st.stx_mask & StatxFlags::BTIME.bits() != 0)
                .then(|| time(st.stx_btime.tv_sec, i64::from(st.stx_btime.tv_nsec)))
                .flatten(),
            hidden: false,
        }
    }

    /// `name` in `dir`, the link itself unless `follow`; with `all`
    /// everything, else its type and identity alone.
    pub(super) fn stat_at(
        dir: &Dir,
        name: &OsStr,
        follow: bool,
        all: bool,
    ) -> std::io::Result<Stat> {
        let flags = if follow {
            AtFlags::empty()
        } else {
            AtFlags::SYMLINK_NOFOLLOW
        };
        Ok(of(&rustix::fs::statx(dir.fd(), name, flags, asked(all))?))
    }

    /// With `all` everything, else the type and the identity alone.
    fn asked(all: bool) -> StatxFlags {
        if all {
            StatxFlags::BASIC_STATS | StatxFlags::BTIME
        } else {
            StatxFlags::TYPE | StatxFlags::INO
        }
    }

    /// What the link `name` in `dir` finally leads to, as `stat_at`
    /// following it says, and where that is. One descriptor of it
    /// (`O_PATH`, which opens no file: a pipe's or a device's neither)
    /// tells both, the kernel saying where (`/proc/self/fd`), in four
    /// calls where `realpath` makes one or two per folder of the path.
    pub(super) fn follow_at(
        dir: &Dir,
        name: &OsStr,
        all: bool,
    ) -> std::io::Result<(Stat, Option<PathBuf>)> {
        use rustix::fs::{Mode, OFlags};
        use std::os::fd::AsRawFd;
        let fd = rustix::fs::openat(
            dir.fd(),
            name,
            OFlags::PATH | OFlags::CLOEXEC,
            Mode::empty(),
        )?;
        let st = of(&rustix::fs::statx(
            &fd,
            c"",
            AtFlags::EMPTY_PATH,
            asked(all),
        )?);
        let at = std::fs::read_link(format!("/proc/self/fd/{}", fd.as_raw_fd()))
            .ok()
            .filter(|at| {
                at.is_absolute() && !at.as_os_str().as_encoded_bytes().ends_with(b" (deleted)")
            });
        // Where the kernel cannot say (no `/proc`), the path walked.
        let at = at.or_else(|| super::resolved(dir, name));
        Ok((st, at))
    }

    pub(super) fn id_of(path: &Path) -> Option<EntryId> {
        let st = rustix::fs::statx(
            rustix::fs::CWD,
            path,
            AtFlags::SYMLINK_NOFOLLOW,
            StatxFlags::INO,
        )
        .ok()?;
        Some(of(&st).id)
    }
}

#[cfg(all(unix, not(target_os = "linux")))]
mod sys {
    pub(super) use super::read_link_at;
    use rustix::fs::AtFlags;

    use super::{Dir, EntryId, OsStr, Path, Stat, device_number, kind_of, time};

    fn of(st: &rustix::fs::Stat) -> Stat {
        Stat {
            id: EntryId {
                dev: device_number(st.st_dev),
                ino: st.st_ino,
            },
            kind: kind_of(st.st_mode),
            mode: Some(u32::from(st.st_mode)),
            size: u64::try_from(st.st_size).unwrap_or(0),
            nlink: u64::from(st.st_nlink),
            uid: Some(st.st_uid),
            gid: Some(st.st_gid),
            modified: time(st.st_mtime, st.st_mtime_nsec),
            accessed: time(st.st_atime, st.st_atime_nsec),
            #[cfg(target_os = "macos")]
            created: time(st.st_birthtime, st.st_birthtime_nsec),
            #[cfg(not(target_os = "macos"))]
            created: None,
            // `UF_HIDDEN`, what Finder hides.
            #[cfg(target_os = "macos")]
            hidden: st.st_flags & 0x8000 != 0,
            #[cfg(not(target_os = "macos"))]
            hidden: false,
        }
    }

    pub(super) fn stat_at(
        dir: &Dir,
        name: &OsStr,
        follow: bool,
        _all: bool,
    ) -> std::io::Result<Stat> {
        let flags = if follow {
            AtFlags::empty()
        } else {
            AtFlags::SYMLINK_NOFOLLOW
        };
        Ok(of(&rustix::fs::statat(dir.fd(), name, flags)?))
    }

    pub(super) fn id_of(path: &Path) -> Option<EntryId> {
        Some(of(&rustix::fs::lstat(path).ok()?).id)
    }
}

#[cfg(windows)]
mod sys {
    use super::{Dir, EntryId, OsStr, Path, Stat};
    use crate::fs::win;

    pub(super) fn stat_at(
        dir: &Dir,
        name: &OsStr,
        follow: bool,
        _all: bool,
    ) -> std::io::Result<Stat> {
        let path = dir
            .path_to(name)
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))?;
        let info = win::info(&win::open(&path, false, follow)?)?;
        Ok(Stat {
            id: info.id,
            kind: info.kind,
            mode: None,
            size: info.size,
            nlink: info.links,
            uid: None,
            gid: None,
            modified: info.modified,
            accessed: info.accessed,
            created: info.created,
            hidden: info.hidden,
        })
    }

    pub(super) fn id_of(path: &Path) -> Option<EntryId> {
        win::identity(&win::open(path, false, false).ok()?).ok()
    }

    pub(super) fn read_link_at(dir: &Dir, name: &OsStr) -> Option<std::ffi::OsString> {
        std::fs::read_link(dir.path_to(name)?)
            .ok()
            .map(std::path::PathBuf::into_os_string)
    }
}

/// What the link `name` in `dir` leads to, followed to the end: `None`
/// when it leads nowhere. Only the type and the identity are asked of
/// what is there.
pub fn follow(dir: &Dir, name: &OsStr) -> Option<LinkTarget> {
    let (st, path) = followed(dir, name, false)?;
    Some(target_of(&st, path))
}

fn target_of(st: &Stat, path: Option<PathBuf>) -> LinkTarget {
    LinkTarget {
        kind: st.kind,
        id: st.id,
        path,
    }
}

/// What the link `name` in `dir` finally leads to (with `all`
/// everything said of it, else its type and identity), and where that
/// is: `None` when it leads nowhere.
fn followed(dir: &Dir, name: &OsStr, all: bool) -> Option<(Stat, Option<PathBuf>)> {
    #[cfg(target_os = "linux")]
    {
        sys::follow_at(dir, name, all).ok()
    }
    #[cfg(not(target_os = "linux"))]
    {
        let st = sys::stat_at(dir, name, true, all).ok()?;
        Some((st, resolved(dir, name)))
    }
}

/// Where the link `name` finally leads, through every link of a chain,
/// from the folder held open (on Linux the kernel resolves its
/// descriptor, `/proc/self/fd`), so the folder's own path is not needed.
fn resolved(dir: &Dir, name: &OsStr) -> Option<PathBuf> {
    crate::os_path::canonical(dir.path_to(name)?).ok()
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
        let target = crate::os_path::canonical(path).ok()?;
        let meta = std::fs::metadata(&target).ok()?;
        (Cow::Owned(target), meta)
    } else {
        (Cow::Borrowed(path), own)
    };
    meta.is_file().then_some(content)
}

/// Stat `name` relative to `dir`; fill `entry` in place. Returns false if
/// gone.
///
/// The entry is the link itself (its identity, its kind), but what it says
/// of size, times, owner and permissions is what it leads to, as a link
/// opens that: a link that leads nowhere says its own.
pub fn stat_entry(dir: &Dir, entry: &mut Entry) -> bool {
    // Owned: the entry is filled while the name is used.
    let name = entry.name.clone();
    let name = name.as_os_str();
    let Ok(own) = sys::stat_at(dir, name, false, true) else {
        return false;
    };
    entry.id = own.id;
    entry.kind = own.kind;
    entry.hidden = own.hidden;
    let followed = (entry.kind == EntryKind::Symlink)
        .then(|| followed(dir, name, true))
        .flatten();
    let (followed, path) = followed.unzip();
    entry.target = followed.as_ref().map(|st| target_of(st, path.flatten()));
    let st = followed.as_ref().unwrap_or(&own);
    let m = &mut entry.metadata;
    let p = Provenance::Filesystem;
    m.set(keys::SIZE, Value::UInt(st.size), p.clone());
    if let Some(mode) = st.mode {
        m.set(keys::MODE, Value::UInt(u64::from(mode)), p.clone());
    }
    m.set(keys::NLINK, Value::UInt(st.nlink), p.clone());
    if let Some(uid) = st.uid {
        m.set(keys::UID, Value::UInt(u64::from(uid)), p.clone());
    }
    if let Some(gid) = st.gid {
        m.set(keys::GID, Value::UInt(u64::from(gid)), p.clone());
    }
    if let Some(t) = st.modified {
        m.set(keys::MODIFIED, Value::Time(t), p.clone());
    }
    if let Some(t) = st.accessed {
        m.set(keys::ACCESSED, Value::Time(t), p.clone());
    }
    if let Some(t) = st.created {
        m.set(keys::CREATED, Value::Time(t), p.clone());
    }
    if entry.kind == EntryKind::Symlink
        && let Some(target) = sys::read_link_at(dir, name)
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
        let root = Dir::open(path).ok()?;
        let mut e = Entry::new(EntryId { dev: 0, ino: 0 }, ".", EntryKind::Unknown);
        if !stat_entry(&root, &mut e) {
            return None;
        }
        path.as_os_str().clone_into(&mut e.name);
        return Some(e);
    };
    let name: &OsStr = name;
    let dir = Dir::open(parent).ok()?;
    let mut e = Entry::new(EntryId { dev: 0, ino: 0 }, name, EntryKind::Unknown);
    stat_entry(&dir, &mut e).then_some(e)
}

/// Stats paths one after another as [`stat_path`] does, keeping the last
/// folder open: the files of one folder, as a walk visits them, cost a
/// stat each instead of opening the folder, a stat and a close (a fifth
/// of the time over a million files). One per walking thread.
#[derive(Default)]
pub struct Statter {
    dir: Option<(PathBuf, Dir)>,
}

impl Statter {
    pub fn stat(&mut self, path: &Path) -> Option<Entry> {
        let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
            return stat_path(path);
        };
        if self.dir.as_ref().is_none_or(|(open, _)| open != parent) {
            self.dir = None;
            self.dir = Some((parent.to_path_buf(), Dir::open(parent).ok()?));
        }
        let (_, dir) = self.dir.as_ref()?;
        let mut e = Entry::new(EntryId { dev: 0, ino: 0 }, name, EntryKind::Unknown);
        stat_entry(dir, &mut e).then_some(e)
    }
}

/// The stable id of a path, without building an entry.
///
/// This is what an [`EntryId`] means everywhere: the device and inode a
/// stat reports, without following a final symlink. [`stat_entry`] and
/// [`stat_path`] report the same; a listing's provisional id for a mount
/// point is replaced by this one when the entry is stat-ed.
pub fn id_of(path: &Path) -> Option<EntryId> {
    sys::id_of(path)
}

#[cfg(test)]
mod tests {
    use super::{id_of, stat_path};
    use std::path::Path;

    #[test]
    fn a_statter_says_what_stat_path_says() {
        let tmp = tempfile::tempdir().unwrap();
        for dir in ["a", "b"] {
            std::fs::create_dir(tmp.path().join(dir)).unwrap();
            std::fs::write(tmp.path().join(dir).join("it's here.txt"), dir).unwrap();
        }
        let mut statter = super::Statter::default();
        for path in [
            tmp.path().join("a/it's here.txt"),
            tmp.path().join("a"),
            tmp.path().join("b/it's here.txt"),
            tmp.path().join("a/it's here.txt"),
            Path::new("/").to_path_buf(),
        ] {
            let alone = stat_path(&path).unwrap();
            let kept = statter.stat(&path).unwrap();
            assert_eq!(
                (kept.id, &kept.name, kept.kind),
                (alone.id, &alone.name, alone.kind)
            );
            assert_eq!(kept.metadata.value("size"), alone.metadata.value("size"));
        }
        assert!(statter.stat(&tmp.path().join("a/gone")).is_none());
        assert!(statter.stat(&tmp.path().join("gone/x")).is_none());
    }

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
