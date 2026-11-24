//! What a client asks of macOS: the user's own temporary folder, where
//! their engine's socket is when no `XDG_RUNTIME_DIR` names a runtime
//! folder, which macOS never sets (`docs/DESIGN.md`, Protocol).

// confstr, which std does not wrap.
#![allow(unsafe_code)]

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;

/// The user's own temporary folder (`/var/folders/…/T/`), as the system
/// gives it for their user (`confstr(_CS_DARWIN_USER_TEMP_DIR)`), whatever
/// `TMPDIR` says: the same for every process of theirs, theirs alone, and
/// on the machine's own disk, where a home may be on a server. None where
/// the system gives none.
pub(crate) fn user_temp() -> Option<PathBuf> {
    let mut buf = vec![0u8; 1024];
    // SAFETY: a buffer of its length, which the call fills with at most
    // that many bytes, its NUL among them, and says how many it needed.
    let needed = unsafe {
        libc::confstr(
            libc::_CS_DARWIN_USER_TEMP_DIR,
            buf.as_mut_ptr().cast(),
            buf.len(),
        )
    };
    if needed == 0 || needed > buf.len() {
        return None;
    }
    buf.truncate(needed - 1);
    let path = PathBuf::from(OsStr::from_bytes(&buf));
    path.is_absolute().then_some(path)
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::MetadataExt;

    /// The user's temporary folder is a folder of theirs alone: absolute,
    /// owned by whoever runs this, as a file they make there is, and open
    /// to no one else.
    #[test]
    fn the_users_temporary_folder_is_theirs_alone() {
        let folder = super::user_temp().expect("the system gives one");
        assert!(folder.is_absolute(), "{}", folder.display());
        let meta = std::fs::metadata(&folder).unwrap();
        assert!(meta.is_dir());
        assert_eq!(meta.mode() & 0o077, 0, "{:o}", meta.mode());
        let made = folder.join(format!("skwird-test-{}", std::process::id()));
        std::fs::write(&made, b"").unwrap();
        let mine = std::fs::metadata(&made).unwrap().uid();
        std::fs::remove_file(&made).unwrap();
        assert_eq!(meta.uid(), mine);
    }
}
