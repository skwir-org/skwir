//! Lazy metadata: an entry stat-ed by name in its folder held open
//! ([`Dir`]): statx on Linux, fstatat on other Unix systems, a handle's
//! records on Windows (ADR-036).

use std::borrow::Cow;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::dir::Dir;
use crate::entry::{Entry, EntryId, EntryKind, FileId, LinkTarget, Provenance, Value, keys};

/// What a stat says of a file, alike on every system; what only Unix
/// says (a mode, an owner) is `None` elsewhere.
struct Stat {
    id: FileId,
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

/// The device number a [`FileId`] carries, from a stat's `st_dev`.
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

/// The file `ino` of the device `dev`, as every stat of it here names it:
/// on a mount whose program makes inode numbers up, that number told
/// apart by the mount's own ([`on_mount`]; `reach::mount_of`, asked of
/// the mount table in memory).
pub fn file(dev: u64, ino: u64) -> FileId {
    FileId::new(dev, on_mount(ino, super::reach::mount_of(dev)))
}

/// The inode number `ino`, made up by the mount numbered `mount`, told
/// apart from the numbers other mounts made up: mixed with the mount's
/// number in every bit, one to one for each mount, so two files of one
/// mount stay two and another mount's number is, but by chance, another.
/// `mount` 0 is none: the number is the file's, kept as it is.
pub(crate) fn on_mount(ino: u64, mount: u64) -> u64 {
    if mount == 0 {
        ino
    } else {
        ino ^ mount.wrapping_mul(0x9e37_79b9_7f4a_7c15).rotate_left(29)
    }
}

/// `st`, its identity told apart by its mount ([`file`]), `dir` saying it
/// for the folder's own device: a listing asks the table once.
fn mounted(mut st: Stat, dir: Option<&Dir>) -> Stat {
    let mount = dir
        .filter(|dir| dir.identity().is_ok_and(|folder| folder.dev == st.id.dev))
        .and_then(|dir| dir.mount().ok())
        .unwrap_or_else(|| super::reach::mount_of(st.id.dev));
    st.id.ino = on_mount(st.id.ino, mount);
    st
}

/// `name` in `dir`, as `sys::stat_at` says it, told by its mount.
fn stat_at(dir: &Dir, name: &OsStr, follow: bool, all: bool) -> std::io::Result<Stat> {
    sys::stat_at(dir, name, follow, all).map(|st| mounted(st, Some(dir)))
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

/// What the system keeps of a file beside its mode (`chattr`'s flags on
/// Linux, `chflags`' on a Mac, Windows' attributes): what says why a
/// delete fails, or why a file takes less room than its size.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)] // independent flags of one file
pub struct Flags {
    /// Nothing may change, rename or delete it, its owner included.
    pub immutable: bool,
    /// It may only grow.
    pub append_only: bool,
    /// The filesystem compresses it.
    pub compressed: bool,
    /// The filesystem encrypts it.
    pub encrypted: bool,
    /// Windows' read-only attribute: what it keeps for a mode.
    pub read_only: bool,
}

/// The [`Flags`] of what `path` leads to; none where the system says
/// none or cannot be asked. Reads the file's metadata: on a worker.
pub fn flags(path: &Path) -> Flags {
    #[cfg(target_os = "linux")]
    {
        use rustix::fs::{AtFlags, CWD, StatxAttributes as A, StatxFlags};
        let Ok(st) = rustix::fs::statx(CWD, path, AtFlags::empty(), StatxFlags::BASIC_STATS) else {
            return Flags::default();
        };
        // A flag the filesystem does not keep is not said to be off.
        let has = |a: A| st.stx_attributes_mask.contains(a) && st.stx_attributes.contains(a);
        Flags {
            immutable: has(A::IMMUTABLE),
            append_only: has(A::APPEND),
            compressed: has(A::COMPRESSED),
            encrypted: has(A::ENCRYPTED),
            read_only: false,
        }
    }
    #[cfg(target_os = "macos")]
    {
        use std::os::macos::fs::MetadataExt;
        // <sys/stat.h>: the user's and the system's own of each.
        const IMMUTABLE: u32 = 0x2 | 0x2_0000;
        const APPEND: u32 = 0x4 | 0x4_0000;
        const COMPRESSED: u32 = 0x20;
        let Ok(meta) = std::fs::metadata(path) else {
            return Flags::default();
        };
        let f = meta.st_flags();
        Flags {
            immutable: f & IMMUTABLE != 0,
            append_only: f & APPEND != 0,
            compressed: f & COMPRESSED != 0,
            ..Flags::default()
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const READ_ONLY: u32 = 0x1;
        const COMPRESSED: u32 = 0x800;
        const ENCRYPTED: u32 = 0x4000;
        let Ok(meta) = std::fs::metadata(path) else {
            return Flags::default();
        };
        let a = meta.file_attributes();
        Flags {
            compressed: a & COMPRESSED != 0,
            encrypted: a & ENCRYPTED != 0,
            read_only: a & READ_ONLY != 0,
            ..Flags::default()
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = path;
        Flags::default()
    }
}

/// The identity of the file at `path`, as [`id_of`] says it, its
/// metadata `meta` read already: through a link when `meta` says it is
/// no link (the file a link leads to), the link's own otherwise. Unix
/// reads it from `meta`; Windows asks the file (`std` does not say it).
pub fn identity(path: &Path, meta: &std::fs::Metadata) -> Option<FileId> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let _ = path;
        Some(file(meta.dev(), meta.ino()))
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

    use super::{Dir, FileId, OsStr, Path, PathBuf, Stat, kind_of, time};

    fn of(st: &rustix::fs::Statx) -> Stat {
        Stat {
            // Its mount said by the caller (`mounted`).
            id: FileId::new(
                rustix::fs::makedev(st.stx_dev_major, st.stx_dev_minor),
                st.stx_ino,
            ),
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

    pub(super) fn id_of(path: &Path) -> Option<FileId> {
        let st = rustix::fs::statx(
            rustix::fs::CWD,
            path,
            AtFlags::SYMLINK_NOFOLLOW,
            StatxFlags::INO,
        )
        .ok()?;
        Some(of(&st).id)
    }

    pub(super) fn named(path: &Path) -> Option<Stat> {
        let st = rustix::fs::statx(
            rustix::fs::CWD,
            path,
            AtFlags::SYMLINK_NOFOLLOW,
            StatxFlags::TYPE | StatxFlags::INO | StatxFlags::NLINK,
        )
        .ok()?;
        Some(of(&st))
    }
}

#[cfg(all(unix, not(target_os = "linux")))]
mod sys {
    pub(super) use super::read_link_at;
    use rustix::fs::AtFlags;

    use super::{Dir, FileId, OsStr, Path, Stat, device_number, kind_of, time};

    fn of(st: &rustix::fs::Stat) -> Stat {
        Stat {
            // Its mount said by the caller (`mounted`).
            id: FileId::new(device_number(st.st_dev), st.st_ino),
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

    pub(super) fn id_of(path: &Path) -> Option<FileId> {
        Some(of(&rustix::fs::lstat(path).ok()?).id)
    }

    pub(super) fn named(path: &Path) -> Option<Stat> {
        Some(of(&rustix::fs::lstat(path).ok()?))
    }
}

#[cfg(windows)]
mod sys {
    use super::{Dir, FileId, OsStr, Path, Stat};
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

    pub(super) fn id_of(path: &Path) -> Option<FileId> {
        win::identity(&win::open(path, false, false).ok()?).ok()
    }

    pub(super) fn named(path: &Path) -> Option<Stat> {
        let info = win::info(&win::open(path, false, false).ok()?).ok()?;
        Some(Stat {
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

    pub(super) fn read_link_at(dir: &Dir, name: &OsStr) -> Option<std::ffi::OsString> {
        std::fs::read_link(dir.path_to(name)?)
            .ok()
            .map(std::path::PathBuf::into_os_string)
    }
}

/// What a step that writes an entry elsewhere in another form (an archive
/// made of it) reads of it ([`own`]): the entry itself, a link not
/// followed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Own {
    pub id: FileId,
    pub kind: EntryKind,
    /// Its permission bits (`0o7777`); `None` where the system keeps none
    /// (Windows).
    pub mode: Option<u32>,
    pub size: u64,
    /// How many names the file has: more than one, each is a hard link.
    pub names: u64,
    pub modified: Option<SystemTime>,
    /// What a link says, as written.
    pub link: Option<std::ffi::OsString>,
}

/// What the entry `name` in `dir` is, itself ([`Own`]): `None` when it is
/// not there. Blocking.
pub fn own(dir: &Dir, name: &OsStr) -> Option<Own> {
    let st = stat_at(dir, name, false, true).ok()?;
    let link = (st.kind == EntryKind::Symlink)
        .then(|| sys::read_link_at(dir, name))
        .flatten();
    Some(Own {
        id: st.id,
        kind: st.kind,
        mode: st.mode.map(|mode| mode & 0o7777),
        size: st.size,
        names: st.nlink,
        modified: st.modified,
        link,
    })
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
        let (st, path) = sys::follow_at(dir, name, all).ok()?;
        Some((mounted(st, Some(dir)), path))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let st = stat_at(dir, name, true, all).ok()?;
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
    let Ok(own) = stat_at(dir, name, false, true) else {
        return false;
    };
    // Which of the file's names this is: this one, in this folder.
    let folder = dir.identity().unwrap_or(FileId::new(own.id.dev, 0));
    entry.id = EntryId::in_folder(own.id, folder, name);
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
        let mut e = Entry::new(EntryId::UNKNOWN, ".", EntryKind::Unknown);
        if !stat_entry(&root, &mut e) {
            return None;
        }
        path.as_os_str().clone_into(&mut e.name);
        return Some(e);
    };
    let name: &OsStr = name;
    let dir = Dir::open(parent).ok()?;
    let mut e = Entry::new(EntryId::UNKNOWN, name, EntryKind::Unknown);
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
        let mut e = Entry::new(EntryId::UNKNOWN, name, EntryKind::Unknown);
        stat_entry(dir, &mut e).then_some(e)
    }
}

/// The file at a path, without building an entry.
///
/// This is what a [`FileId`] means everywhere: the device and inode a
/// stat reports, without following a final symlink. [`stat_entry`] and
/// [`stat_path`] name their entry by it and the name's folder
/// (`EntryId::in_folder`); a listing's provisional file for a mount point
/// is replaced by this one when the entry is stat-ed.
pub fn id_of(path: &Path) -> Option<FileId> {
    sys::id_of(path).map(|id| file(id.dev, id.ino))
}

/// [`id_of`], and whether `path` is its file's one name: a folder always
/// is (no folder has two), any other file when its link count is one. A
/// file with several names (hard links) is one file under each.
pub fn named(path: &Path) -> Option<(FileId, bool)> {
    let st = mounted(sys::named(path)?, None);
    Some((st.id, st.kind == EntryKind::Directory || st.nlink == 1))
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
        assert_eq!(Some(root.file()), id_of(Path::new("/")));
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("it's a folder");
        std::fs::create_dir(&dir).unwrap();
        let entry = stat_path(&dir).expect("a folder");
        assert!(entry.is_dir());
        assert_eq!(entry.name, "it's a folder");
        assert_eq!(Some(entry.file()), id_of(&dir));
    }
}
