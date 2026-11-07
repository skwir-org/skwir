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

/// A read of `path`'s attributes, unless its FUSE remote refused them
/// while mounted (`reach::refuses_attributes`): answered as it was then,
/// the server not asked again for each file. A refusal is remembered.
fn read<T>(path: &Path, ask: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
    if crate::fs::reach::refuses_attributes(path) {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "this server keeps no extended attributes",
        ));
    }
    let answer = ask();
    if let Err(error) = &answer
        && refused(error)
    {
        crate::fs::reach::note_refused_attributes(path);
    }
    answer
}

/// A write of `path`'s attributes, tried whatever a read was answered:
/// one that succeeds lets go of a refusal its mount made
/// (`reach::note_accepted_attributes`).
fn written(path: &Path, answer: io::Result<()>) -> io::Result<()> {
    if answer.is_ok() {
        crate::fs::reach::note_accepted_attributes(path);
    }
    answer
}

/// The attribute `name` of the file (or link) at `path`; `None` when it
/// has none of that name.
///
/// # Errors
/// When it cannot be read, or the volume keeps no attributes ([`refused`]).
pub fn get(path: impl AsRef<Path>, name: impl AsRef<OsStr>) -> io::Result<Option<Vec<u8>>> {
    let (path, name) = (path.as_ref(), name.as_ref());
    read(path, || imp::get(path, name))
}

/// [`get`] through a link, of the file it leads to.
///
/// # Errors
/// As [`get`].
pub fn get_deref(path: impl AsRef<Path>, name: impl AsRef<OsStr>) -> io::Result<Option<Vec<u8>>> {
    let (path, name) = (path.as_ref(), name.as_ref());
    read(path, || imp::get_deref(path, name))
}

/// Set the attribute `name` of the file (or link) at `path` to `value`.
///
/// # Errors
/// When it cannot be written, or the volume keeps no attributes.
pub fn set(path: impl AsRef<Path>, name: impl AsRef<OsStr>, value: &[u8]) -> io::Result<()> {
    let path = path.as_ref();
    written(path, imp::set(path, name.as_ref(), value))
}

/// [`set`] through a link, on the file it leads to.
///
/// # Errors
/// As [`set`].
pub fn set_deref(path: impl AsRef<Path>, name: impl AsRef<OsStr>, value: &[u8]) -> io::Result<()> {
    let path = path.as_ref();
    written(path, imp::set_deref(path, name.as_ref(), value))
}

/// Remove the attribute `name` of the file (or link) at `path`.
///
/// # Errors
/// When it is not there ([`missing`]), or cannot be removed.
pub fn remove(path: impl AsRef<Path>, name: impl AsRef<OsStr>) -> io::Result<()> {
    let path = path.as_ref();
    written(path, imp::remove(path, name.as_ref()))
}

/// [`remove`] through a link, on the file it leads to.
///
/// # Errors
/// As [`remove`].
pub fn remove_deref(path: impl AsRef<Path>, name: impl AsRef<OsStr>) -> io::Result<()> {
    let path = path.as_ref();
    written(path, imp::remove_deref(path, name.as_ref()))
}

/// The names of the attributes of the file (or link) at `path`.
///
/// # Errors
/// When they cannot be listed, or the volume keeps no attributes.
pub fn list(path: impl AsRef<Path>) -> io::Result<Vec<OsString>> {
    let path = path.as_ref();
    read(path, || imp::list(path))
}

/// [`list`] through a link, of the file it leads to.
///
/// # Errors
/// As [`list`].
pub fn list_deref(path: impl AsRef<Path>) -> io::Result<Vec<OsString>> {
    let path = path.as_ref();
    read(path, || imp::list_deref(path))
}

/// Whether the volume holding the folder `dir` keeps extended attributes,
/// as a write there would find: asked by reading one of the folder's,
/// which the kernel answers by the same handler a write goes to (a
/// listing would not tell: Linux lists none, without refusing, where a
/// volume keeps none). Every volume refuses them on Windows. One call on
/// the folder, through a link to it: ask once per folder, on a worker (in
/// its remote's lane). Where it cannot tell (an unreadable folder), yes,
/// as a write is then tried.
pub fn kept_in(dir: &Path) -> bool {
    /// Any name the user may set: what is asked is the volume.
    const ASKED: &str = "user.xdg.tags";
    !matches!(get_deref(dir, ASKED), Err(error) if refused(&error))
}

/// Whether what carries a file elsewhere (a copy, an archive made of it,
/// one extracted) keeps its attribute `name`: on Linux the user's
/// (`user.*`); on macOS, whose names have no namespace, every one but
/// those the system sets itself and refuses to anyone else
/// (`com.apple.rootless`, `com.apple.provenance`), Finder's tags and info
/// among those kept.
pub fn kept(name: &OsStr) -> bool {
    if cfg!(target_os = "macos") {
        !matches!(
            name.to_str(),
            Some("com.apple.rootless" | "com.apple.provenance")
        )
    } else {
        name.as_encoded_bytes().starts_with(b"user.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The home folder's volume keeps attributes on Linux and macOS;
    /// Windows keeps none anywhere.
    #[test]
    fn a_folder_says_whether_its_volume_keeps_attributes() {
        let base = dirs::home_dir().unwrap().join(".cache").join("skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        let tmp = tempfile::Builder::new()
            .prefix("xattr-kept-")
            .tempdir_in(base)
            .unwrap();
        assert_eq!(kept_in(tmp.path()), cfg!(unix));
    }

    /// `SKWIR_NO_XATTR_DIR=<folder>` on a volume refusing them (a FAT
    /// stick, `bindfs --xattr-none`): `cargo nextest run -p skwir-core
    /// --run-ignored only refusing`.
    #[cfg(unix)]
    #[test]
    #[ignore = "needs a folder on a volume without attributes"]
    fn a_folder_on_a_volume_refusing_attributes_says_so() {
        let dir = std::path::PathBuf::from(
            std::env::var_os("SKWIR_NO_XATTR_DIR").expect("SKWIR_NO_XATTR_DIR"),
        );
        assert!(set(&dir, "user.test", b"x").is_err_and(|e| refused(&e)));
        assert!(!kept_in(&dir));
    }
}
