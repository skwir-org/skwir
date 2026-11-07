//! The mark that hides a file from other programs where its name does
//! not: Windows' hidden attribute, which FAT and exFAT keep in a file's
//! entry, so Linux sets it there too, through their drivers
//! (`FAT_IOCTL_SET_ATTRIBUTES`: vfat's, exFAT's since Linux 6.7).
//!
//! Skwir gives it to what it makes for a companion (ADR-028) that a dot
//! already hides on Unix: a Mac's `._x`, `TagSpaces`' `.ts` folder
//! ([`made`], [`create_dir_all`]), so Windows' Explorer hides them as a
//! Mac and Linux do. A sidecar a person handles (`x.jpg.xmp`) keeps no
//! dot and no mark, and a file another program made is never marked. A
//! file replaced whole keeps the mark of the one it replaces
//! (`file::take_place`, [`worn_by`]).
//!
//! Best effort: a volume without the mark (ext4, NTFS from Linux, exFAT
//! before Linux 6.7 or through FUSE) leaves files as they were. On macOS
//! nothing is done: it makes its own `._` files.

use std::ffi::OsStr;
use std::io;
use std::path::Path;

/// Whether a dot hides `name` on Unix.
pub fn dotted(name: &OsStr) -> bool {
    name.as_encoded_bytes().starts_with(b".")
}

/// Whether `path` itself (never a link's target) wears the mark.
///
/// # Errors
/// When it cannot be asked, or its volume keeps no such mark
/// (`Unsupported`).
pub fn is_set(path: &Path) -> io::Result<bool> {
    #[cfg(windows)]
    {
        crate::fs::win::hidden(path)
    }
    #[cfg(target_os = "linux")]
    {
        Ok(fat::get(&fat::open(path)?)? & fat::HIDDEN != 0)
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = path;
        Err(none())
    }
}

/// Give `path` itself (a file or a folder, never a link) the mark,
/// keeping its other attributes.
///
/// # Errors
/// When it cannot be set, or its volume keeps no such mark
/// (`Unsupported`).
pub fn set(path: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        crate::fs::win::hide(path)
    }
    #[cfg(target_os = "linux")]
    {
        let file = fat::open(path)?;
        let now = fat::get(&file)?;
        if now & fat::HIDDEN != 0 {
            return Ok(());
        }
        fat::set(&file, now | fat::HIDDEN)
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = path;
        Err(none())
    }
}

/// `path`, which Skwir just made for a companion, takes the mark if a dot
/// hides its name: best effort, a volume without the mark leaving it as it
/// is.
pub fn made(path: &Path) {
    if !path.file_name().is_some_and(dotted) {
        return;
    }
    if let Err(error) = set(path)
        && error.kind() != io::ErrorKind::Unsupported
    {
        tracing::debug!(%error, path = %path.display(), "not marked hidden");
    }
}

/// Make `dir` and the folders missing above it, as
/// `std::fs::create_dir_all`, for a companion to go in: each one made that
/// a dot hides (`.ts`) takes the mark ([`made`]).
///
/// # Errors
/// When a folder cannot be made.
pub fn create_dir_all(dir: &Path) -> io::Result<()> {
    let mut missing = Vec::new();
    let mut at = Some(dir);
    while let Some(folder) = at {
        if folder.as_os_str().is_empty() || std::fs::symlink_metadata(folder).is_ok() {
            break;
        }
        missing.push(folder);
        at = folder.parent();
    }
    std::fs::create_dir_all(dir)?;
    for folder in missing {
        made(folder);
    }
    Ok(())
}

/// Whether `target`, which the file `staged` (open, beside it) is about
/// to replace, wears the mark: on Linux asked of `target` only where
/// `staged`'s volume keeps one, so a replace elsewhere opens nothing more.
pub(crate) fn worn_by(target: &Path, staged: &std::fs::File) -> bool {
    #[cfg(windows)]
    {
        let _ = staged;
        crate::fs::win::hidden(target).unwrap_or(false)
    }
    #[cfg(target_os = "linux")]
    {
        fat::get(staged).is_ok()
            && fat::open(target)
                .and_then(|file| fat::get(&file))
                .is_ok_and(|attributes| attributes & fat::HIDDEN != 0)
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = (target, staged);
        false
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
fn none() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        format!("{} keeps no hidden mark", crate::system::NAME),
    )
}

/// The attributes of a file in a FAT or exFAT entry, as Linux's drivers
/// read and set them (`linux/msdos_fs.h`).
#[cfg(target_os = "linux")]
mod fat {
    use std::fs::File;
    use std::io;
    use std::path::Path;

    use rustix::ioctl::{Getter, Opcode, Setter, opcode};

    /// `FAT_IOCTL_GET_ATTRIBUTES`: `_IOR('r', 0x10, __u32)`.
    const GET: Opcode = opcode::read::<u32>(b'r', 0x10);
    /// `FAT_IOCTL_SET_ATTRIBUTES`: `_IOW('r', 0x11, __u32)`.
    const SET: Opcode = opcode::write::<u32>(b'r', 0x11);
    /// `ATTR_HIDDEN`.
    pub const HIDDEN: u32 = 2;

    /// Open `path` itself to ask its attributes: a regular file as
    /// `file::open_regular_unfollowed` opens one, a folder through
    /// `O_DIRECTORY`; anything else (a link, a pipe, a device) refused,
    /// never opened.
    pub fn open(path: &Path) -> io::Result<File> {
        let meta = std::fs::symlink_metadata(path)?;
        if meta.is_dir() {
            use rustix::fs::{Mode, OFlags};
            let fd = rustix::fs::open(
                path,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )?;
            Ok(File::from(fd))
        } else {
            crate::fs::file::open_regular_unfollowed(path)
        }
    }

    /// A volume that keeps no such attributes (ext4, FUSE, an older
    /// exFAT driver) says it does not know the call.
    fn unsupported(error: rustix::io::Errno) -> io::Error {
        if matches!(
            error,
            rustix::io::Errno::NOTTY | rustix::io::Errno::NOSYS | rustix::io::Errno::OPNOTSUPP
        ) {
            io::Error::new(
                io::ErrorKind::Unsupported,
                "its volume keeps no hidden mark",
            )
        } else {
            error.into()
        }
    }

    pub fn get(file: &File) -> io::Result<u32> {
        // SAFETY: the opcode is the drivers' own, and the kernel writes
        // the `u32` it says into the place `Getter` holds.
        #[allow(unsafe_code)]
        let asked = unsafe { rustix::ioctl::ioctl(file, Getter::<GET, u32>::new()) };
        asked.map_err(unsupported)
    }

    pub fn set(file: &File, attributes: u32) -> io::Result<()> {
        // SAFETY: the opcode is the drivers' own, and the kernel reads
        // the `u32` it says from the place `Setter` holds.
        #[allow(unsafe_code)]
        let set = unsafe { rustix::ioctl::ioctl(file, Setter::<SET, u32>::new(attributes)) };
        set.map_err(unsupported)
    }
}

// macOS keeps no mark: nothing there to test.
#[cfg(all(test, any(windows, target_os = "linux")))]
mod tests {
    use super::*;

    fn home_tempdir() -> tempfile::TempDir {
        let base = dirs::home_dir().unwrap().join(".cache").join("skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        tempfile::Builder::new()
            .prefix("hidden-")
            .tempdir_in(base)
            .unwrap()
    }

    /// Replace `target` whole, as an edit of a sidecar does.
    fn replace(target: &Path, bytes: &[u8]) {
        use std::io::Write as _;
        let mut staged = tempfile::NamedTempFile::new_in(target.parent().unwrap()).unwrap();
        staged.write_all(bytes).unwrap();
        crate::fs::file::take_place(staged, target).unwrap();
    }

    /// What a volume with the mark does: a dotted name made takes it, a
    /// plain one does not, a folder made for a companion takes it, and a
    /// file replaced whole keeps the mark it had.
    fn marks_as_a_volume_keeping_them(dir: &Path) {
        let ours = dir.join("._it's a.txt");
        std::fs::write(&ours, b"x").unwrap();
        made(&ours);
        assert!(is_set(&ours).unwrap(), "a dot hides it");
        let sidecar = dir.join("it's a.txt.xmp");
        std::fs::write(&sidecar, b"x").unwrap();
        made(&sidecar);
        assert!(!is_set(&sidecar).unwrap(), "a sidecar a person handles");
        create_dir_all(&dir.join(".ts")).unwrap();
        assert!(is_set(&dir.join(".ts")).unwrap());
        // Replaced whole: the mark stays, and a file without one gets none.
        let marked = dir.join("Été.txt");
        std::fs::write(&marked, b"before").unwrap();
        set(&marked).unwrap();
        replace(&marked, b"after");
        assert_eq!(std::fs::read(&marked).unwrap(), b"after");
        assert!(is_set(&marked).unwrap(), "kept by take_place");
        replace(&sidecar, b"after");
        assert!(!is_set(&sidecar).unwrap());
    }

    #[cfg(windows)]
    #[test]
    fn windows_marks_what_skwir_makes_for_a_companion_and_keeps_it() {
        let tmp = home_tempdir();
        // Wine says a dotted name is hidden by itself: the plain names
        // here are what tells the mark set from Wine's own answer.
        marks_as_a_volume_keeping_them(tmp.path());
    }

    /// Where the volume keeps no mark (the home folder's ext4 or btrfs),
    /// nothing is marked and nothing fails: a replace goes on as before.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_volume_without_the_mark_is_left_as_it_is() {
        let tmp = home_tempdir();
        let ours = tmp.path().join("._a");
        std::fs::write(&ours, b"x").unwrap();
        assert_eq!(set(&ours).unwrap_err().kind(), io::ErrorKind::Unsupported);
        made(&ours);
        create_dir_all(&tmp.path().join(".ts").join("deeper")).unwrap();
        replace(&ours, b"after");
        assert_eq!(std::fs::read(&ours).unwrap(), b"after");
    }

    /// On FAT and exFAT volumes (udisks loop mounts, say):
    /// `SKWIR_FAT_DIR=<folder> SKWIR_EXFAT_DIR=<folder> cargo nextest run
    /// -p skwir-core --run-ignored only fat_`.
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "needs folders on FAT and exFAT volumes"]
    fn fat_and_exfat_volumes_keep_the_mark() {
        for var in ["SKWIR_FAT_DIR", "SKWIR_EXFAT_DIR"] {
            let base = std::path::PathBuf::from(std::env::var_os(var).expect(var));
            let dir = tempfile::Builder::new()
                .prefix("hidden-")
                .tempdir_in(base)
                .unwrap();
            marks_as_a_volume_keeping_them(dir.path());
        }
    }
}
