//! Extended attributes, the one way every crate reads and writes them.
//!
//! On Unix they are the system's (the `xattr` crate: `user.*` on Linux,
//! any name on macOS); `_deref` follows a link to its file, the others act
//! on the link itself. Windows keeps none that skwir uses, so there every
//! call answers as a volume refusing attributes does ([`refused`]), and
//! what keeps notes on the file falls back to the stores beside it
//! (ADR-027, ADR-028).

use std::ffi::{OsStr, OsString};
use std::io;
use std::path::Path;

/// Whether `error` says the volume keeps no attributes (or none of this
/// namespace): nothing is there to read, and a write must go elsewhere.
pub fn refused(error: &io::Error) -> bool {
    if error.kind() == io::ErrorKind::Unsupported {
        return true;
    }
    #[cfg(unix)]
    {
        rustix::io::Errno::from_io_error(error) == Some(rustix::io::Errno::NOTSUP)
    }
    #[cfg(not(unix))]
    false
}

/// Whether `error` says the file has no attribute of that name.
pub fn missing(error: &io::Error) -> bool {
    #[cfg(unix)]
    {
        // Linux says ENODATA, macOS ENOATTR (the same number on neither).
        #[cfg(target_os = "linux")]
        let absent = rustix::io::Errno::NODATA;
        #[cfg(not(target_os = "linux"))]
        let absent = rustix::io::Errno::NOATTR;
        rustix::io::Errno::from_io_error(error) == Some(absent)
    }
    #[cfg(not(unix))]
    {
        let _ = error;
        false
    }
}

#[cfg(unix)]
mod imp {
    use super::{OsStr, OsString, Path, io};

    pub fn get(path: &Path, name: &OsStr) -> io::Result<Option<Vec<u8>>> {
        xattr::get(path, name)
    }
    pub fn get_deref(path: &Path, name: &OsStr) -> io::Result<Option<Vec<u8>>> {
        xattr::get_deref(path, name)
    }
    pub fn set(path: &Path, name: &OsStr, value: &[u8]) -> io::Result<()> {
        xattr::set(path, name, value)
    }
    pub fn set_deref(path: &Path, name: &OsStr, value: &[u8]) -> io::Result<()> {
        xattr::set_deref(path, name, value)
    }
    pub fn remove(path: &Path, name: &OsStr) -> io::Result<()> {
        xattr::remove(path, name)
    }
    pub fn remove_deref(path: &Path, name: &OsStr) -> io::Result<()> {
        xattr::remove_deref(path, name)
    }
    pub fn list(path: &Path) -> io::Result<Vec<OsString>> {
        xattr::list(path).map(Iterator::collect)
    }
    pub fn list_deref(path: &Path) -> io::Result<Vec<OsString>> {
        xattr::list_deref(path).map(Iterator::collect)
    }
}

#[cfg(not(unix))]
// One signature on every system: the Unix twin returns these.
#[allow(clippy::unnecessary_wraps)]
mod imp {
    use super::{OsStr, OsString, Path, io};

    fn none() -> io::Error {
        io::Error::new(
            io::ErrorKind::Unsupported,
            format!("{} keeps no extended attributes", crate::system::NAME),
        )
    }

    pub fn get(_: &Path, _: &OsStr) -> io::Result<Option<Vec<u8>>> {
        Err(none())
    }
    pub fn get_deref(_: &Path, _: &OsStr) -> io::Result<Option<Vec<u8>>> {
        Err(none())
    }
    pub fn set(_: &Path, _: &OsStr, _: &[u8]) -> io::Result<()> {
        Err(none())
    }
    pub fn set_deref(_: &Path, _: &OsStr, _: &[u8]) -> io::Result<()> {
        Err(none())
    }
    pub fn remove(_: &Path, _: &OsStr) -> io::Result<()> {
        Err(none())
    }
    pub fn remove_deref(_: &Path, _: &OsStr) -> io::Result<()> {
        Err(none())
    }
    pub fn list(_: &Path) -> io::Result<Vec<OsString>> {
        Err(none())
    }
    pub fn list_deref(_: &Path) -> io::Result<Vec<OsString>> {
        Err(none())
    }
}

/// The attribute `name` of the file (or link) at `path`; `None` when it
/// has none of that name.
///
/// # Errors
/// When it cannot be read, or the volume keeps no attributes ([`refused`]).
pub fn get(path: impl AsRef<Path>, name: impl AsRef<OsStr>) -> io::Result<Option<Vec<u8>>> {
    imp::get(path.as_ref(), name.as_ref())
}

/// [`get`] through a link, of the file it leads to.
///
/// # Errors
/// As [`get`].
pub fn get_deref(path: impl AsRef<Path>, name: impl AsRef<OsStr>) -> io::Result<Option<Vec<u8>>> {
    imp::get_deref(path.as_ref(), name.as_ref())
}

/// Set the attribute `name` of the file (or link) at `path` to `value`.
///
/// # Errors
/// When it cannot be written, or the volume keeps no attributes.
pub fn set(path: impl AsRef<Path>, name: impl AsRef<OsStr>, value: &[u8]) -> io::Result<()> {
    imp::set(path.as_ref(), name.as_ref(), value)
}

/// [`set`] through a link, on the file it leads to.
///
/// # Errors
/// As [`set`].
pub fn set_deref(path: impl AsRef<Path>, name: impl AsRef<OsStr>, value: &[u8]) -> io::Result<()> {
    imp::set_deref(path.as_ref(), name.as_ref(), value)
}

/// Remove the attribute `name` of the file (or link) at `path`.
///
/// # Errors
/// When it is not there ([`missing`]), or cannot be removed.
pub fn remove(path: impl AsRef<Path>, name: impl AsRef<OsStr>) -> io::Result<()> {
    imp::remove(path.as_ref(), name.as_ref())
}

/// [`remove`] through a link, on the file it leads to.
///
/// # Errors
/// As [`remove`].
pub fn remove_deref(path: impl AsRef<Path>, name: impl AsRef<OsStr>) -> io::Result<()> {
    imp::remove_deref(path.as_ref(), name.as_ref())
}

/// The names of the attributes of the file (or link) at `path`.
///
/// # Errors
/// When they cannot be listed, or the volume keeps no attributes.
pub fn list(path: impl AsRef<Path>) -> io::Result<Vec<OsString>> {
    imp::list(path.as_ref())
}

/// [`list`] through a link, of the file it leads to.
///
/// # Errors
/// As [`list`].
pub fn list_deref(path: impl AsRef<Path>) -> io::Result<Vec<OsString>> {
    imp::list_deref(path.as_ref())
}
