//! What the built-in operations ask of the system, one function each, so
//! `builtin.rs` says what an operation means and this file how each
//! system does it (ADR-035). Errors are `std::io::Error`s: their kind
//! (`CrossesDevices`, `AlreadyExists`) is what the operations read, on
//! every system alike. What is not written for a system yet says so
//! ([`crate::system::not_yet`]).

use std::fs::{File, FileType, Metadata, Permissions};
use std::io;
use std::path::Path;

/// `clonefile`'s flag that clones a link itself, not what it leads to
/// (`<sys/clonefile.h>`; `libc` names it not).
#[cfg(target_os = "macos")]
const CLONE_NOFOLLOW: u32 = 0x0001;

/// Rename `from` to `to`, refusing to replace what is there
/// (`AlreadyExists`); across volumes it fails with `CrossesDevices`.
pub(super) fn rename_noreplace(from: &Path, to: &Path) -> io::Result<()> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use rustix::io::Errno;
        // Many FUSE filesystems (sshfs, bindfs, exfat-fuse) and some
        // network ones take no flags (macOS says so with ENOTSUP): look,
        // then rename. What appears in between is replaced; nothing better
        // is offered there.
        let takes_no_flags = |e: Errno| {
            matches!(e, Errno::INVAL | Errno::NOSYS)
                || (cfg!(target_os = "macos") && e == Errno::NOTSUP)
        };
        match rustix::fs::renameat_with(
            rustix::fs::CWD,
            from,
            rustix::fs::CWD,
            to,
            rustix::fs::RenameFlags::NOREPLACE,
        ) {
            Err(e) if takes_no_flags(e) && std::fs::symlink_metadata(to).is_err() => {
                std::fs::rename(from, to)
            }
            Err(e) if takes_no_flags(e) => Err(Errno::EXIST.into()),
            r => r.map_err(io::Error::from),
        }
    }
    #[cfg(windows)]
    {
        crate::fs::win::rename_noreplace(from, to)
    }
}

/// Exchange `a` and `b` at once: each takes the other's place. Refused on
/// Windows, which cannot.
pub(super) fn exchange(a: &Path, b: &Path) -> io::Result<()> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        rustix::fs::renameat_with(
            rustix::fs::CWD,
            a,
            rustix::fs::CWD,
            b,
            rustix::fs::RenameFlags::EXCHANGE,
        )
        .map_err(io::Error::from)
    }
    // Windows has no exchange of two names in one step: a swap there is
    // refused rather than done in three renames another program could
    // see half done.
    #[cfg(windows)]
    {
        let _ = (a, b);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Windows cannot swap two names in one step",
        ))
    }
}

/// Make `dst` share `src`'s blocks (a reflink) where the filesystem can;
/// false where it cannot, and the bytes are to be copied.
pub(super) fn clone_file(src: &File, dst: &File) -> bool {
    #[cfg(target_os = "linux")]
    {
        rustix::fs::ioctl_ficlone(dst, src).is_ok()
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (src, dst);
        false
    }
}

/// Copy the file `src` to `dst` (which is not there yet) in one call of
/// the system's, where it keeps more than the bytes: on Windows
/// `CopyFileExW`, its alternate streams (a download's mark), attributes
/// and times with it; on macOS `clonefile`, which shares the blocks on
/// APFS and keeps the attributes, mode and times, used where they are to
/// be kept (`keep_attributes`). `wrote` is told the
/// bytes written so far, and stops the copy when it says no. `None`
/// where the bytes are copied here (`copy_range`, a buffer): Linux, and
/// a Mac volume that clones nothing.
// One signature on every system: Windows' always copies, Linux's never.
#[cfg_attr(not(target_os = "macos"), allow(clippy::unnecessary_wraps))]
pub(super) fn copy_whole(
    src: &Path,
    dst: &Path,
    keep_attributes: bool,
    wrote: &mut dyn FnMut(u64) -> bool,
) -> Option<io::Result<()>> {
    #[cfg(windows)]
    {
        let _ = keep_attributes;
        Some(crate::fs::win::copy_file(src, dst, wrote))
    }
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStrExt;
        // A clone keeps every attribute: not for a copy asked without.
        if !keep_attributes {
            return None;
        }
        let size = std::fs::symlink_metadata(src).map(|m| m.len());
        let (Ok(from), Ok(to)) = (
            std::ffi::CString::new(src.as_os_str().as_bytes()),
            std::ffi::CString::new(dst.as_os_str().as_bytes()),
        ) else {
            return None;
        };
        // SAFETY: both are NUL-terminated paths that outlive the call.
        #[allow(unsafe_code)]
        let cloned = unsafe { libc::clonefile(from.as_ptr(), to.as_ptr(), CLONE_NOFOLLOW) };
        if cloned != 0 {
            let error = io::Error::last_os_error();
            // Not APFS, or another volume: the bytes are copied here.
            return match error.raw_os_error() {
                Some(libc::ENOTSUP | libc::EXDEV) => None,
                _ => Some(Err(error)),
            };
        }
        let _ = wrote(size.unwrap_or(0));
        Some(Ok(()))
    }
    #[cfg(target_os = "linux")]
    {
        let _ = (src, dst, keep_attributes, wrote);
        None
    }
}

/// The devices of a file read and of one written (their metadata): what
/// the kernel refusing to copy between them is remembered by.
// One signature on every system: Unix always says them, Windows never.
#[cfg_attr(unix, allow(clippy::unnecessary_wraps))]
pub(super) fn devices(src: &std::fs::Metadata, dst: &std::fs::Metadata) -> Option<(u64, u64)> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Some((src.dev(), dst.dev()))
    }
    #[cfg(not(unix))]
    {
        let _ = (src, dst);
        None
    }
}

/// Whether the file holds holes: fewer blocks than its length needs.
pub(super) fn is_sparse(meta: &std::fs::Metadata) -> bool {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::MetadataExt;
        meta.blocks().saturating_mul(512) < meta.len()
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = meta;
        false
    }
}

/// The next stretch of data in `file` at or after `from`, up to the hole
/// after it: `None` once only a hole is left.
///
/// # Errors
/// When the file cannot be asked.
pub(super) fn next_data(file: &File, from: u64) -> io::Result<Option<(u64, u64)>> {
    #[cfg(target_os = "linux")]
    {
        use rustix::fs::{SeekFrom, seek};
        let data = match seek(file, SeekFrom::Data(from)) {
            Ok(data) => data,
            Err(rustix::io::Errno::NXIO) => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let hole = seek(file, SeekFrom::Hole(data))?;
        Ok(Some((data, hole)))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (file, from);
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }
}

/// [`copy_range`] at `at` in both files, leaving their offsets alone.
#[cfg_attr(not(target_os = "linux"), allow(clippy::unnecessary_wraps))]
pub(super) fn copy_range_at(
    src: &File,
    dst: &File,
    at: u64,
    len: usize,
) -> io::Result<Option<usize>> {
    #[cfg(target_os = "linux")]
    {
        use rustix::io::Errno;
        let (mut from, mut to) = (at, at);
        match rustix::fs::copy_file_range(src, Some(&mut from), dst, Some(&mut to), len) {
            Ok(n) => Ok(Some(n)),
            Err(Errno::XDEV | Errno::NOSYS | Errno::INVAL | Errno::OPNOTSUPP | Errno::PERM) => {
                Ok(None)
            }
            Err(e) => Err(e.into()),
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (src, dst, at, len);
        Ok(None)
    }
}

/// Copy up to `len` bytes from `src` to `dst` at their offsets, in the
/// kernel: `Some(n)` copied (0 is not trusted as the end, some
/// filesystems say it early), `None` where the kernel cannot, and a
/// buffer is to take over from where both offsets are.
#[cfg_attr(not(target_os = "linux"), allow(clippy::unnecessary_wraps))]
pub(super) fn copy_range(src: &File, dst: &File, len: usize) -> io::Result<Option<usize>> {
    #[cfg(target_os = "linux")]
    {
        use rustix::io::Errno;
        match rustix::fs::copy_file_range(src, None, dst, None, len) {
            Ok(n) => Ok(Some(n)),
            // Across filesystems, or a filesystem that cannot (procfs, a
            // few FUSE).
            Err(Errno::XDEV | Errno::NOSYS | Errno::INVAL | Errno::OPNOTSUPP | Errno::PERM) => {
                Ok(None)
            }
            Err(e) => Err(e.into()),
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (src, dst, len);
        Ok(None)
    }
}

/// Hand the drive what `file` holds in memory of its `len` bytes from
/// `from`, without waiting; or, `wait`, wait until the drive has them
/// all (`sync_file_range`). Their data only: the file's size and name,
/// and the drive's own cache, are its volume's write-out
/// (`Dir::sync_volume`). Of a file a program writes
/// ([`written_by_a_program`]) it reaches nothing.
pub(super) fn write_range(file: &File, from: u64, len: u64, wait: bool) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        use std::os::fd::AsRawFd;
        let flags = if wait {
            libc::SYNC_FILE_RANGE_WAIT_BEFORE
                | libc::SYNC_FILE_RANGE_WRITE
                | libc::SYNC_FILE_RANGE_WAIT_AFTER
        } else {
            libc::SYNC_FILE_RANGE_WRITE
        };
        let too_far = |_| io::Error::from(io::ErrorKind::InvalidInput);
        let (from, len) = (
            from.try_into().map_err(too_far)?,
            len.try_into().map_err(too_far)?,
        );
        // SAFETY: a call on a descriptor `file` holds open; no memory is
        // passed. rustix has no `sync_file_range`.
        #[allow(unsafe_code)]
        let done = unsafe { libc::sync_file_range(file.as_raw_fd(), from, len, flags) };
        if done == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (file, from, len, wait);
        Err(crate::system::not_yet("Writing through to a drive"))
    }
}

/// Whether a program, not the kernel, writes the volume `dir` is on
/// (FUSE: ntfs-3g, exfat-fuse). A file's bytes pass to it as they are
/// written, so none wait in the file's memory for [`write_range`]: what
/// it wrote only the volume's write-out reaches.
pub(super) fn written_by_a_program(dir: &crate::fs::Dir) -> bool {
    #[cfg(target_os = "linux")]
    {
        const FUSE_SUPER_MAGIC: u64 = 0x6573_5546;
        rustix::fs::fstatfs(dir.fd())
            .is_ok_and(|fs| u64::try_from(fs.f_type).is_ok_and(|t| t == FUSE_SUPER_MAGIC))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = dir;
        false
    }
}

/// Whether `kind` is a named pipe.
pub(super) fn is_fifo(kind: FileType) -> bool {
    #[cfg(unix)]
    {
        std::os::unix::fs::FileTypeExt::is_fifo(&kind)
    }
    #[cfg(not(unix))]
    {
        let _ = kind;
        false
    }
}

/// Whether `kind` is a socket.
pub(super) fn is_socket(kind: FileType) -> bool {
    #[cfg(unix)]
    {
        std::os::unix::fs::FileTypeExt::is_socket(&kind)
    }
    #[cfg(not(unix))]
    {
        let _ = kind;
        false
    }
}

/// Make a new named pipe at `dst`, with the permissions `like` has.
pub(super) fn make_fifo(dst: &Path, like: &Metadata) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::PermissionsExt;
        rustix::fs::mknodat(
            rustix::fs::CWD,
            dst,
            rustix::fs::FileType::Fifo,
            rustix::fs::Mode::from_raw_mode(like.permissions().mode() & 0o7777),
            0,
        )
        .map_err(io::Error::from)
    }
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::PermissionsExt;
        let path = std::ffi::CString::new(dst.as_os_str().as_bytes())
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
        let mode = libc::mode_t::try_from(like.permissions().mode() & 0o7777).unwrap_or(0o600);
        // SAFETY: a NUL-terminated path that outlives the call.
        #[allow(unsafe_code)]
        let result = unsafe { libc::mkfifo(path.as_ptr(), mode) };
        if result == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
    #[cfg(windows)]
    {
        let _ = (dst, like);
        Err(crate::system::not_yet("Copying a pipe"))
    }
}

/// Make a link at `dst` that says `target`, a copy of a link of `kind`:
/// on Windows a link to a folder is made as one.
pub(super) fn symlink_like(target: &Path, dst: &Path, kind: FileType) -> io::Result<()> {
    #[cfg(unix)]
    {
        let _ = kind;
        std::os::unix::fs::symlink(target, dst)
    }
    #[cfg(windows)]
    {
        if std::os::windows::fs::FileTypeExt::is_symlink_dir(&kind) {
            std::os::windows::fs::symlink_dir(target, dst)
        } else {
            std::os::windows::fs::symlink_file(target, dst)
        }
    }
}

/// Make a new link at `dst` that says `target`, a folder when `folder`:
/// on Windows a link to a folder is made as one.
pub(super) fn symlink_to(target: &Path, dst: &Path, folder: bool) -> io::Result<()> {
    #[cfg(unix)]
    {
        let _ = folder;
        std::os::unix::fs::symlink(target, dst)
    }
    #[cfg(windows)]
    {
        if folder {
            std::os::windows::fs::symlink_dir(target, dst)
        } else {
            std::os::windows::fs::symlink_file(target, dst)
        }
    }
}

/// Remove the link at `link`, never what it leads to: on Windows a link
/// to a folder is removed as a folder.
pub(super) fn remove_link(link: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileTypeExt;
        if std::fs::symlink_metadata(link)?
            .file_type()
            .is_symlink_dir()
        {
            return std::fs::remove_dir(link);
        }
    }
    std::fs::remove_file(link)
}

/// Open the folder `dir` to set its times: as any file on Unix; on
/// Windows with the flag a folder needs and the right to write what is
/// said of it.
pub(super) fn open_to_set_times(dir: &Path) -> io::Result<File> {
    #[cfg(unix)]
    {
        File::open(dir)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem as fs;
        std::fs::OpenOptions::new()
            .access_mode(fs::FILE_WRITE_ATTRIBUTES)
            .custom_flags(fs::FILE_FLAG_BACKUP_SEMANTICS)
            .open(dir)
    }
}

/// Permissions only the owner has, for a private staging folder; `None`
/// where a new folder's own are kept (Windows, whose folders inherit
/// their parent's access).
#[cfg_attr(unix, allow(clippy::unnecessary_wraps))]
pub(super) fn owner_only() -> Option<Permissions> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        Some(Permissions::from_mode(0o700))
    }
    #[cfg(not(unix))]
    None
}

/// `current` opened to its owner, so what is inside can be written and
/// removed: a read-only folder made writable.
pub(super) fn opened_to_owner(current: &Permissions) -> Permissions {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        Permissions::from_mode(current.mode() | 0o700)
    }
    #[cfg(not(unix))]
    {
        let mut opened = current.clone();
        #[allow(clippy::permissions_set_readonly_false)]
        opened.set_readonly(false);
        opened
    }
}

/// Whether `current` lets its owner write and walk what is inside.
pub(super) fn open_to_owner(current: &Permissions) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        current.mode() & 0o700 == 0o700
    }
    #[cfg(not(unix))]
    {
        !current.readonly()
    }
}

/// Put `path` in the trash, and say where it went where only the system
/// can tell (macOS: `trashItemAtURL`'s resulting path, kept for undo, the
/// Mac's trash keeping no record of where an item came from); `None`
/// where the trash's own records say it (Windows). Linux's is the plan's
/// own (`builtin::trash_item`).
#[cfg(not(target_os = "linux"))]
pub(super) fn trash_item(path: &Path) -> Result<Option<std::path::PathBuf>, trash::Error> {
    #[cfg(target_os = "macos")]
    {
        use objc2_foundation::{NSFileManager, NSString, NSURL};
        let unknown = |description: String| trash::Error::Unknown { description };
        let text = path
            .to_str()
            .ok_or_else(|| unknown(format!("{} is no UTF-8 path", path.display())))?;
        let url = NSURL::fileURLWithPath(&NSString::from_str(text));
        let mut landed = None;
        NSFileManager::defaultManager()
            .trashItemAtURL_resultingItemURL_error(&url, Some(&mut landed))
            .map_err(|e| unknown(format!("trash {}: {e}", path.display())))?;
        Ok(landed
            .and_then(|url| url.path())
            .map(|p| std::path::PathBuf::from(p.to_string())))
    }
    #[cfg(not(target_os = "macos"))]
    {
        trash::delete(path).map(|()| None)
    }
}

/// What the system's trash holds, as the `trash` crate lists it; Linux's
/// is the plan's own (`trash_list`), which reads no server unasked.
#[cfg(not(target_os = "linux"))]
pub(super) fn trash_list() -> Result<Vec<trash::TrashItem>, trash::Error> {
    #[cfg(not(target_os = "macos"))]
    {
        trash::os_limited::list()
    }
    #[cfg(target_os = "macos")]
    {
        Err(trash::Error::Unknown {
            description: crate::system::not_yet("Listing the trash").to_string(),
        })
    }
}

/// Put `items` of the trash back where they came from. Linux's is the
/// plan's own (`builtin::restore_listed`).
#[cfg(not(target_os = "linux"))]
#[cfg_attr(target_os = "macos", allow(clippy::needless_pass_by_value))]
pub(super) fn trash_restore(items: Vec<trash::TrashItem>) -> Result<(), trash::Error> {
    #[cfg(not(target_os = "macos"))]
    {
        trash::os_limited::restore_all(items)
    }
    #[cfg(target_os = "macos")]
    {
        let _ = items;
        Err(trash::Error::Unknown {
            description: crate::system::not_yet("Restoring from the trash").to_string(),
        })
    }
}

/// What a change of access (`access.rs`, ADR-058) asks of the system: what
/// an item is, without following a link inside a folder; a hold on an
/// item through which its mode and group change, never through a link
/// or another file put in its place since; who the user is; and the umask
/// `chmod` applies to a clause that names no class.
pub(super) mod access {
    use std::ffi::OsStr;
    use std::io;
    use std::path::Path;

    use crate::entry::FileId;
    use crate::fs::Dir;

    /// What a change of access reads of an item.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Meta {
        pub id: FileId,
        /// Its permission bits (`0o7777`). On Windows, the read-only
        /// attribute as the owner's write bit, the rest as Windows lets
        /// everyone have it.
        pub mode: u32,
        /// Its owner and group; `None` where they are not numbers
        /// (Windows).
        pub uid: Option<u32>,
        pub gid: Option<u32>,
        pub folder: bool,
        pub link: bool,
    }

    /// Who runs skwir: whose files it may change, and the groups it may
    /// give them to.
    #[derive(Clone, Debug)]
    pub struct Me {
        pub uid: u32,
        pub root: bool,
        pub groups: Vec<u32>,
    }

    impl Me {
        /// Whether a file of theirs may be given to the group `gid`, or
        /// given back to it.
        pub fn may_give(&self, gid: u32) -> bool {
            self.root || self.groups.contains(&gid)
        }
    }

    /// The user skwir runs as and their groups; `None` where users are not
    /// numbers (Windows).
    #[cfg(unix)]
    #[allow(clippy::unnecessary_wraps)] // Windows' answer is `None`.
    pub fn me() -> Option<Me> {
        let uid = rustix::process::geteuid();
        let mut groups = vec![rustix::process::getegid().as_raw()];
        for group in rustix::process::getgroups().unwrap_or_default() {
            if !groups.contains(&group.as_raw()) {
                groups.push(group.as_raw());
            }
        }
        Some(Me {
            uid: uid.as_raw(),
            root: uid.is_root(),
            groups,
        })
    }

    #[cfg(windows)]
    pub fn me() -> Option<Me> {
        None
    }

    /// The umask, as `chmod` applies it to a clause naming no class: read
    /// on Linux from `/proc/self/status` (never by setting it and setting
    /// it back, which races with every thread creating a file meanwhile).
    /// skwir never changes it, so it is read once. `None` where it cannot
    /// be read (macOS, a Linux before 4.7 or without `/proc`). Windows
    /// keeps none: a mode there is the read-only attribute alone.
    // Windows' answer is always `Some(0)`; the others' may be `None`.
    #[cfg_attr(windows, allow(clippy::unnecessary_wraps))]
    pub fn umask() -> Option<u32> {
        #[cfg(target_os = "linux")]
        {
            static UMASK: std::sync::OnceLock<Option<u32>> = std::sync::OnceLock::new();
            *UMASK.get_or_init(|| {
                let status = std::fs::read_to_string("/proc/self/status").ok()?;
                let line = status.lines().find_map(|l| l.strip_prefix("Umask:"))?;
                u32::from_str_radix(line.trim(), 8)
                    .ok()
                    .map(|mask| mask & 0o777)
            })
        }
        #[cfg(all(unix, not(target_os = "linux")))]
        {
            None
        }
        #[cfg(windows)]
        {
            Some(0)
        }
    }

    /// What a hold found in place of the item it was asked for.
    #[derive(Debug)]
    struct Replaced(&'static str);

    impl std::fmt::Display for Replaced {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(self.0)
        }
    }

    impl std::error::Error for Replaced {}

    /// Another file where the one asked for was.
    pub fn replaced() -> io::Error {
        io::Error::other(Replaced("it is now another file"))
    }

    fn a_link() -> io::Error {
        io::Error::other(Replaced("it is a link now"))
    }

    /// The folder at `path`, a link there followed, opened to be listed:
    /// the one `expect` names, not another put in its place since.
    pub fn open_top(path: &Path, expect: FileId) -> io::Result<Dir> {
        let folder = Dir::open(path)?;
        if folder_id(&folder)? == expect {
            Ok(folder)
        } else {
            Err(replaced())
        }
    }

    /// The folder `name` in `dir`, opened to be listed: the one `expect`
    /// names, never through a link (one put there since is said to be a
    /// link) nor another folder put in its place.
    pub fn open_folder(dir: &Dir, name: &OsStr, expect: FileId) -> io::Result<Dir> {
        match dir.open_folder(name) {
            Ok(folder) if folder_id(&folder)? == expect => Ok(folder),
            Ok(_) => Err(replaced()),
            Err(e) if stat_at(dir, name).is_ok_and(|meta| meta.link) => {
                let _ = e;
                Err(a_link())
            }
            Err(e) => Err(e),
        }
    }

    /// Whether `e` says the item asked for is not there any more: gone, a
    /// path through it gone, or another file (a link) in its place.
    pub fn is_gone(e: &io::Error) -> bool {
        matches!(
            e.kind(),
            io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
        ) || e
            .get_ref()
            .is_some_and(<dyn std::error::Error + Send + Sync>::is::<Replaced>)
    }

    // ---------------------------------------------------------------- Linux

    #[cfg(target_os = "linux")]
    use rustix::fs::{AtFlags, StatxFlags};

    #[cfg(target_os = "linux")]
    const ASKED: StatxFlags = StatxFlags::TYPE
        .union(StatxFlags::MODE)
        .union(StatxFlags::UID)
        .union(StatxFlags::GID)
        .union(StatxFlags::INO);

    #[cfg(target_os = "linux")]
    fn of(st: &rustix::fs::Statx) -> Meta {
        use rustix::fs::FileType;
        let mode = u32::from(st.stx_mode);
        let kind = FileType::from_raw_mode(mode);
        Meta {
            // As `fs::stat` names a file.
            id: crate::fs::stat::file(
                rustix::fs::makedev(st.stx_dev_major, st.stx_dev_minor),
                st.stx_ino,
            ),
            mode: mode & 0o7777,
            uid: Some(st.stx_uid),
            gid: Some(st.stx_gid),
            folder: kind == FileType::Directory,
            link: kind == FileType::Symlink,
        }
    }

    /// The item at `path`, what a link there leads to when `follow`.
    #[cfg(target_os = "linux")]
    pub fn stat(path: &Path, follow: bool) -> io::Result<Meta> {
        let flags = if follow {
            AtFlags::empty()
        } else {
            AtFlags::SYMLINK_NOFOLLOW
        };
        Ok(of(&rustix::fs::statx(rustix::fs::CWD, path, flags, ASKED)?))
    }

    /// The item `name` in `dir`, itself: a link is not followed.
    #[cfg(target_os = "linux")]
    pub fn stat_at(dir: &Dir, name: &OsStr) -> io::Result<Meta> {
        Ok(of(&rustix::fs::statx(
            dir.fd(),
            name,
            AtFlags::SYMLINK_NOFOLLOW,
            ASKED,
        )?))
    }

    /// The identity of the folder `dir` holds open, as [`stat`] says it.
    #[cfg(target_os = "linux")]
    pub fn folder_id(dir: &Dir) -> io::Result<FileId> {
        Ok(of(&rustix::fs::statx(
            dir.fd(),
            c"",
            AtFlags::EMPTY_PATH,
            ASKED,
        )?)
        .id)
    }

    /// An item held to change who may use it. On Linux a descriptor that
    /// opens nothing (`O_PATH`: no read access needed, a pipe or a device
    /// not opened), checked to be the item stat-ed and no link: its group
    /// changes through it (`fchownat` on the descriptor itself), its mode
    /// through `/proc/self/fd`, which names the very file it holds (as
    /// glibc's own `fchmodat` does for `AT_SYMLINK_NOFOLLOW`): Linux's
    /// `fchmodat` takes no flags before `fchmodat2` (6.6), and `fchmod`
    /// refuses such a descriptor.
    #[cfg(target_os = "linux")]
    pub struct Held(rustix::fd::OwnedFd);

    #[cfg(target_os = "linux")]
    impl Held {
        /// Hold the item at `path`, the one `expect` names; what a link
        /// there leads to when `follow`, else the item itself, which must
        /// be no link.
        pub fn path(path: &Path, expect: FileId, follow: bool) -> io::Result<Held> {
            use rustix::fs::{Mode, OFlags};
            let mut flags = OFlags::PATH | OFlags::CLOEXEC;
            if !follow {
                flags |= OFlags::NOFOLLOW;
            }
            Self::checked(rustix::fs::open(path, flags, Mode::empty())?, expect)
        }

        /// Hold the item `name` in `dir`, the one `expect` names, itself:
        /// a link put there since is refused.
        pub fn at(dir: &Dir, name: &OsStr, expect: FileId) -> io::Result<Held> {
            use rustix::fs::{Mode, OFlags};
            let fd = rustix::fs::openat(
                dir.fd(),
                name,
                OFlags::PATH | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )?;
            Self::checked(fd, expect)
        }

        fn checked(fd: rustix::fd::OwnedFd, expect: FileId) -> io::Result<Held> {
            let held = Held(fd);
            let meta = held.stat()?;
            if meta.link {
                return Err(a_link());
            }
            if meta.id != expect {
                return Err(replaced());
            }
            Ok(held)
        }

        /// What the held item is now.
        pub fn stat(&self) -> io::Result<Meta> {
            Ok(of(&rustix::fs::statx(
                &self.0,
                c"",
                AtFlags::EMPTY_PATH,
                ASKED,
            )?))
        }

        /// Give it to the group `gid`.
        pub fn set_group(&self, gid: u32) -> io::Result<()> {
            rustix::fs::chownat(
                &self.0,
                c"",
                None,
                Some(rustix::fs::Gid::from_raw(gid)),
                AtFlags::EMPTY_PATH,
            )?;
            Ok(())
        }

        /// Set its permission bits to `mode`.
        pub fn set_mode(&self, mode: u32) -> io::Result<()> {
            use std::os::fd::AsRawFd;
            let at = format!("/proc/self/fd/{}", self.0.as_raw_fd());
            match rustix::fs::chmod(at.as_str(), rustix::fs::Mode::from_raw_mode(mode & 0o7777)) {
                Err(rustix::io::Errno::NOENT) if !Path::new("/proc/self/fd").is_dir() => {
                    Err(io::Error::other(
                        "the system's /proc is not mounted, which changing access needs",
                    ))
                }
                r => r.map_err(io::Error::from),
            }
        }
    }

    // ----------------------------------------------------- macOS (other Unix)

    #[cfg(all(unix, not(target_os = "linux")))]
    fn of(st: &rustix::fs::Stat) -> Meta {
        use rustix::fs::FileType;
        let kind = FileType::from_raw_mode(st.st_mode);
        Meta {
            id: crate::fs::stat::file(crate::fs::stat::device_number(st.st_dev), st.st_ino),
            mode: u32::from(st.st_mode) & 0o7777,
            uid: Some(st.st_uid),
            gid: Some(st.st_gid),
            folder: kind == FileType::Directory,
            link: kind == FileType::Symlink,
        }
    }

    #[cfg(all(unix, not(target_os = "linux")))]
    fn follow_flags(follow: bool) -> rustix::fs::AtFlags {
        if follow {
            rustix::fs::AtFlags::empty()
        } else {
            rustix::fs::AtFlags::SYMLINK_NOFOLLOW
        }
    }

    /// The item at `path`, what a link there leads to when `follow`.
    #[cfg(all(unix, not(target_os = "linux")))]
    pub fn stat(path: &Path, follow: bool) -> io::Result<Meta> {
        Ok(of(&rustix::fs::statat(
            rustix::fs::CWD,
            path,
            follow_flags(follow),
        )?))
    }

    /// The item `name` in `dir`, itself: a link is not followed.
    #[cfg(all(unix, not(target_os = "linux")))]
    pub fn stat_at(dir: &Dir, name: &OsStr) -> io::Result<Meta> {
        Ok(of(&rustix::fs::statat(
            dir.fd(),
            name,
            rustix::fs::AtFlags::SYMLINK_NOFOLLOW,
        )?))
    }

    /// The identity of the folder `dir` holds open, as [`stat`] says it.
    #[cfg(all(unix, not(target_os = "linux")))]
    pub fn folder_id(dir: &Dir) -> io::Result<FileId> {
        Ok(of(&rustix::fs::fstat(dir.fd())?).id)
    }

    /// An item held to change who may use it: its name in a folder held
    /// open (or its path), changed with `AT_SYMLINK_NOFOLLOW`, which these
    /// systems' `fchmodat` and `fchownat` take: a link put in its place
    /// since is changed itself, never what it leads to. Checked to be the
    /// item stat-ed when held.
    #[cfg(all(unix, not(target_os = "linux")))]
    pub struct Held {
        /// The folder it is named in; `None` for a path.
        dir: Option<rustix::fd::OwnedFd>,
        name: std::path::PathBuf,
        follow: bool,
    }

    #[cfg(all(unix, not(target_os = "linux")))]
    impl Held {
        /// Hold the item at `path`, the one `expect` names; what a link
        /// there leads to when `follow`, else the item itself, which must
        /// be no link.
        pub fn path(path: &Path, expect: FileId, follow: bool) -> io::Result<Held> {
            Held {
                dir: None,
                name: path.to_path_buf(),
                follow,
            }
            .checked(expect)
        }

        /// Hold the item `name` in `dir`, the one `expect` names, itself.
        pub fn at(dir: &Dir, name: &OsStr, expect: FileId) -> io::Result<Held> {
            Held {
                dir: Some(dir.fd().try_clone_to_owned()?),
                name: name.into(),
                follow: false,
            }
            .checked(expect)
        }

        fn checked(self, expect: FileId) -> io::Result<Held> {
            let meta = self.stat()?;
            if meta.link {
                return Err(a_link());
            }
            if meta.id != expect {
                return Err(replaced());
            }
            Ok(self)
        }

        fn base(&self) -> rustix::fd::BorrowedFd<'_> {
            use rustix::fd::AsFd;
            self.dir.as_ref().map_or(rustix::fs::CWD, |dir| dir.as_fd())
        }

        /// What the held item is now.
        pub fn stat(&self) -> io::Result<Meta> {
            Ok(of(&rustix::fs::statat(
                self.base(),
                &self.name,
                follow_flags(self.follow),
            )?))
        }

        /// Give it to the group `gid`.
        pub fn set_group(&self, gid: u32) -> io::Result<()> {
            rustix::fs::chownat(
                self.base(),
                &self.name,
                None,
                Some(rustix::fs::Gid::from_raw(gid)),
                follow_flags(self.follow),
            )?;
            Ok(())
        }

        /// Set its permission bits to `mode`.
        pub fn set_mode(&self, mode: u32) -> io::Result<()> {
            let bits = rustix::fs::RawMode::try_from(mode & 0o7777).unwrap_or_default();
            rustix::fs::chmodat(
                self.base(),
                &self.name,
                rustix::fs::Mode::from_raw_mode(bits),
                follow_flags(self.follow),
            )?;
            Ok(())
        }
    }

    // -------------------------------------------------------------- Windows

    /// The item at `path`, what a link there leads to when `follow`.
    #[cfg(windows)]
    pub fn stat(path: &Path, follow: bool) -> io::Result<Meta> {
        use crate::entry::EntryKind;
        use crate::fs::win;
        let info = win::info(&win::open(path, false, follow)?)?;
        let meta = if follow {
            std::fs::metadata(path)?
        } else {
            std::fs::symlink_metadata(path)?
        };
        let folder = info.kind == EntryKind::Directory;
        let base = if folder { 0o777 } else { 0o666 };
        Ok(Meta {
            id: info.id,
            mode: if meta.permissions().readonly() {
                base & !0o222
            } else {
                base
            },
            uid: None,
            gid: None,
            folder,
            link: info.kind == EntryKind::Symlink,
        })
    }

    /// The item `name` in `dir`, itself: a link is not followed.
    #[cfg(windows)]
    pub fn stat_at(dir: &Dir, name: &OsStr) -> io::Result<Meta> {
        let path = dir
            .path_to(name)
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        stat(&path, false)
    }

    /// The identity of the folder `dir` holds open.
    #[cfg(windows)]
    pub fn folder_id(dir: &Dir) -> io::Result<FileId> {
        dir.identity()
    }

    /// An item held to change who may use it: on Windows its path,
    /// checked to be the item stat-ed and no link when held, and changed
    /// by that path (a link put there between the two aside: see
    /// PORTING.md).
    #[cfg(windows)]
    pub struct Held {
        path: std::path::PathBuf,
        follow: bool,
    }

    #[cfg(windows)]
    impl Held {
        /// Hold the item at `path`, the one `expect` names; what a link
        /// there leads to when `follow`, else the item itself.
        pub fn path(path: &Path, expect: FileId, follow: bool) -> io::Result<Held> {
            Held {
                path: path.to_path_buf(),
                follow,
            }
            .checked(expect)
        }

        /// Hold the item `name` in `dir`, the one `expect` names, itself.
        pub fn at(dir: &Dir, name: &OsStr, expect: FileId) -> io::Result<Held> {
            let path = dir
                .path_to(name)
                .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
            Self::path(&path, expect, false)
        }

        fn checked(self, expect: FileId) -> io::Result<Held> {
            let meta = self.stat()?;
            if meta.link {
                return Err(a_link());
            }
            if meta.id != expect {
                return Err(replaced());
            }
            Ok(self)
        }

        /// What the held item is now.
        pub fn stat(&self) -> io::Result<Meta> {
            stat(&self.path, self.follow)
        }

        /// Windows says no group by number.
        #[allow(clippy::unused_self)]
        pub fn set_group(&self, gid: u32) -> io::Result<()> {
            let _ = gid;
            Err(crate::system::not_yet("Changing a file's group"))
        }

        /// The owner's write bit of `mode` as the read-only attribute.
        pub fn set_mode(&self, mode: u32) -> io::Result<()> {
            let mut permissions = std::fs::metadata(&self.path)?.permissions();
            permissions.set_readonly(mode & 0o200 == 0);
            std::fs::set_permissions(&self.path, permissions)
        }
    }
}
