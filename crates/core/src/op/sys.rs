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

/// The devices of a file read (`src`, its metadata) and of one written:
/// what the kernel refusing to copy between them is remembered by.
pub(super) fn devices(src: &std::fs::Metadata, dst: &File) -> Option<(u64, u64)> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Some((src.dev(), dst.metadata().ok()?.dev()))
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
/// where the trash's own records say it (freedesktop, Windows). On Linux
/// the trash a plan puts files in is found once (`put_trash`).
pub(super) fn trash_item(
    path: &Path,
    ctx: &super::context::StepContext,
) -> Result<Option<std::path::PathBuf>, trash::Error> {
    #[cfg(not(target_os = "linux"))]
    let _ = ctx;
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
    #[cfg(target_os = "linux")]
    if ctx.trashes().put(path) {
        return Ok(None);
    }
    #[cfg(not(target_os = "macos"))]
    {
        trash::delete(path).map(|()| None)
    }
}

/// What the system's trash holds, as the `trash` crate lists it.
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

/// Put `items` of the trash back where they came from.
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
