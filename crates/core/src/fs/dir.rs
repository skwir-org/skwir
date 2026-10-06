//! A folder held open, listed and stat-ed by its entries' names: the one
//! seam between the listing and the system (ADR-035).
//!
//! On Unix it is a directory descriptor: names are looked up relative to
//! it, so a listing follows its folder when the folder moves, and a stat
//! costs no path walk. Linux lists with getdents and stats with statx
//! (`stat.rs`), other Unix systems with readdir and fstatat. On Windows
//! it is a handle opened with every share mode, listed through the IDs
//! the folder's records carry and stat-ed by its path (ADR-036).

use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};

use crate::entry::EntryKind;

/// One name a listing reads, before anything is stat-ed: what the folder
/// itself says of it.
pub struct Listed {
    pub name: OsString,
    /// Its number within the folder's device (an inode): with
    /// [`Dir::device`], the provisional identity a listing names it by.
    pub ino: u64,
    pub kind: EntryKind,
    /// Marked hidden by the system, where the folder says so (Windows).
    pub hidden: bool,
}

#[cfg(unix)]
pub use unix::Dir;
#[cfg(unix)]
pub(crate) use unix::kind_of;

#[cfg(unix)]
mod unix {
    use rustix::fd::{AsFd, BorrowedFd, OwnedFd};
    use rustix::fs::{FileType, Mode, OFlags};

    use super::{EntryKind, Listed, OsStr, Path, PathBuf, io};

    pub(crate) fn kind_of(t: FileType) -> EntryKind {
        match t {
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

    /// A folder held open by its descriptor.
    pub struct Dir(OwnedFd);

    impl Dir {
        /// Open the folder at `path`. I/O: a sleeping disk can hold it.
        ///
        /// # Errors
        /// When it is no folder, or cannot be opened.
        pub fn open(path: &Path) -> io::Result<Self> {
            let fd = rustix::fs::open(
                path,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
                Mode::empty(),
            )?;
            Ok(Self(fd))
        }

        pub(crate) fn fd(&self) -> BorrowedFd<'_> {
            self.0.as_fd()
        }

        /// The device the folder is on, as its entries' identities say it
        /// (`stat::device_number`).
        ///
        /// # Errors
        /// When the folder cannot be stat-ed.
        pub fn device(&self) -> io::Result<u64> {
            let st = rustix::fs::fstat(&self.0)?;
            Ok(crate::fs::stat::device_number(st.st_dev))
        }

        /// The names the folder holds now, `.` and `..` left out, read as
        /// the iterator is walked.
        ///
        /// # Errors
        /// When the folder cannot be read; each name read may fail too.
        pub fn read(&self) -> io::Result<impl Iterator<Item = io::Result<Listed>> + use<>> {
            let mut dir = rustix::fs::Dir::read_from(&self.0)?;
            Ok(std::iter::from_fn(move || {
                loop {
                    let ent = match dir.next()? {
                        Ok(ent) => ent,
                        Err(e) => return Some(Err(e.into())),
                    };
                    let name = ent.file_name();
                    if name == c"." || name == c".." {
                        continue;
                    }
                    return Some(Ok(Listed {
                        name: std::os::unix::ffi::OsStringExt::from_vec(name.to_bytes().to_vec()),
                        ino: ent.ino(),
                        kind: kind_of(ent.file_type()),
                        hidden: false,
                    }));
                }
            }))
        }

        /// Write out what the folder's volume holds in memory, and say
        /// what failed to be written since the folder was opened
        /// (`syncfs`).
        ///
        /// # Errors
        /// When the volume could not write it all.
        pub fn sync_volume(&self) -> io::Result<()> {
            #[cfg(target_os = "linux")]
            {
                rustix::fs::syncfs(&self.0).map_err(io::Error::from)
            }
            #[cfg(not(target_os = "linux"))]
            {
                Err(crate::system::not_yet("Writing through to a drive"))
            }
        }

        /// Whether the folder was removed: it has no links left, while we
        /// hold it open.
        pub fn is_unlinked(&self) -> bool {
            rustix::fs::fstat(&self.0).is_ok_and(|st| st.st_nlink == 0)
        }

        /// Where the folder is now, wherever it moved: `None` when the
        /// system cannot say, or it was removed.
        pub fn path_now(&self) -> Option<PathBuf> {
            #[cfg(target_os = "linux")]
            {
                use std::os::fd::AsRawFd;
                let link = format!("/proc/self/fd/{}", self.0.as_raw_fd());
                std::fs::read_link(link).ok().filter(|now| {
                    now.is_absolute()
                        && !now.as_os_str().as_encoded_bytes().ends_with(b" (deleted)")
                })
            }
            #[cfg(target_os = "macos")]
            {
                let path = rustix::fs::getpath(&self.0).ok()?;
                let path: &OsStr = std::os::unix::ffi::OsStrExt::from_bytes(path.to_bytes());
                Some(PathBuf::from(path)).filter(|now| now.is_absolute())
            }
            #[cfg(not(any(target_os = "linux", target_os = "macos")))]
            None
        }

        /// A path to `name` in the folder that holds wherever the folder
        /// moved: the kernel's own on Linux (`/proc/self/fd`), the folder's
        /// path now elsewhere.
        pub fn path_to(&self, name: &OsStr) -> Option<PathBuf> {
            #[cfg(target_os = "linux")]
            {
                use std::os::fd::AsRawFd;
                Some(
                    Path::new("/proc/self/fd")
                        .join(self.0.as_raw_fd().to_string())
                        .join(name),
                )
            }
            #[cfg(not(target_os = "linux"))]
            {
                Some(self.path_now()?.join(name))
            }
        }
    }
}

#[cfg(windows)]
pub use windows::Dir;

#[cfg(windows)]
mod windows {
    use std::fs::File;

    use parking_lot::Mutex;

    use super::{EntryKind, Listed, OsStr, Path, PathBuf, io};
    use crate::fs::win;

    /// A folder held open by a handle, and where it was last known to be:
    /// its entries are opened by their path in it.
    pub struct Dir {
        file: File,
        path: Mutex<PathBuf>,
    }

    impl Dir {
        /// Open the folder at `path`. I/O: a sleeping disk can hold it.
        ///
        /// # Errors
        /// When it is no folder, or cannot be opened.
        pub fn open(path: &Path) -> io::Result<Self> {
            let file = win::open(path, true, true)?;
            if win::info(&file)?.kind != EntryKind::Directory {
                return Err(io::Error::from(io::ErrorKind::NotADirectory));
            }
            Ok(Self {
                file,
                path: Mutex::new(path.to_path_buf()),
            })
        }

        /// The volume the folder is on, as its entries' identities say it.
        ///
        /// # Errors
        /// When the folder cannot be asked.
        pub fn device(&self) -> io::Result<u64> {
            Ok(win::identity(&self.file)?.dev)
        }

        /// The names the folder holds now, `.` and `..` left out, read as
        /// the iterator is walked, each with the ID its record carries.
        ///
        /// # Errors
        /// When the folder cannot be read; each batch read may fail too.
        pub fn read(&self) -> io::Result<impl Iterator<Item = io::Result<Listed>> + use<>> {
            let listing = win::Listing::new(win::reopen(&self.file)?);
            Ok(listing.map(|listed| {
                listed.map(|l| Listed {
                    name: l.name,
                    ino: l.ino,
                    kind: l.kind,
                    hidden: l.hidden,
                })
            }))
        }

        /// # Errors
        /// Always, for now.
        #[allow(clippy::unused_self)]
        pub fn sync_volume(&self) -> io::Result<()> {
            Err(crate::system::not_yet("Writing through to a drive"))
        }

        /// Whether the folder was removed while we hold it open.
        pub fn is_unlinked(&self) -> bool {
            win::info(&self.file).is_ok_and(|info| info.deleting)
        }

        /// Where the folder is now, wherever it moved; its entries are
        /// opened there from now on.
        pub fn path_now(&self) -> Option<PathBuf> {
            let now = win::path_now(&self.file)?;
            now.clone_into(&mut self.path.lock());
            Some(now)
        }

        /// The path of `name` in the folder, where the folder was last
        /// known to be.
        pub fn path_to(&self, name: &OsStr) -> Option<PathBuf> {
            Some(self.path.lock().join(name))
        }
    }
}
