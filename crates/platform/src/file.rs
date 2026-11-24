//! Opening the user's files for reading, and rewriting the desktop's shared
//! text files.
//!
//! Every read of a file the user pointed at (content sniffing, previews,
//! thumbnails, EXIF) or that another application keeps beside one (a
//! sidecar, a `.trashinfo`) goes through [`open_regular`] or
//! [`read_regular`], the core's (`skwir_core::fs::file`), which never block
//! and refuse anything that is not a regular file: a named pipe, socket or
//! device called `x.png` is not an image.
//!
//! Files other programs share with us (GTK bookmarks, `mimeapps.list`, the
//! recently used files) are changed through [`rewrite_words`] or
//! [`rewrite_words_within`] only.

use std::io;
use std::path::Path;

pub use skwir_core::fs::file::{
    SIDE_FILE_LIMIT, open_regular, read_regular, read_regular_text, take_place,
};

/// Rewrite the text file at `link` with `change`, which gets its current
/// text (empty when there is no file) and answers the new text with a value
/// for the caller, or `None` to leave the file alone. Writes a file: call
/// from a worker.
///
/// The file is replaced whole (a reader never sees half of it), keeping its
/// mode. A file that is a link (a dotfile manager's) is updated where it
/// points; if that is read-only, as a declarative configuration is, it is
/// refused saying `read_only`, a whole message naming it ("The bookmarks
/// file is read-only; change it where it is managed"), rather than
/// replaced by a copy that silently parts from the declaration. It is read
/// as a desktop's file is ([`DESKTOP_FILE`] at most, a regular file):
/// one larger, or a pipe of its name, is refused, never read whole.
///
/// # Errors
/// When the file cannot be read or written.
pub fn rewrite_words<T>(
    link: &Path,
    read_only: impl Into<skwir_core::words::Words>,
    change: impl FnOnce(&str) -> Option<(String, T)>,
) -> anyhow::Result<Option<T>> {
    rewrite_words_within(link, DESKTOP_FILE, read_only, change)
}

/// [`rewrite_words`] of a file that may hold `limit` bytes (a list the
/// desktop's programs keep growing, its recently used files).
///
/// # Errors
/// When the file cannot be read or written, or holds more than `limit`.
pub fn rewrite_words_within<T>(
    link: &Path,
    limit: u64,
    read_only: impl Into<skwir_core::words::Words>,
    change: impl FnOnce(&str) -> Option<(String, T)>,
) -> anyhow::Result<Option<T>> {
    rewrite_with(
        link,
        limit,
        || {
            skwir_core::words!(
                file_no_folder = "{ $path } has no folder",
                path = link.display().to_string(),
                note = "Why a file of the desktop's (its bookmarks, its default applications) was not changed: its path names no folder. path: the file's path."
            )
            .into()
        },
        move || anyhow::Error::from(Into::<skwir_core::words::Words>::into(read_only)),
        change,
    )
}

/// [`rewrite_words`], read `limit` bytes at most, refused with what
/// `no_folder` and `read_only` make.
fn rewrite_with<T>(
    link: &Path,
    limit: u64,
    no_folder: impl FnOnce() -> anyhow::Error,
    read_only: impl FnOnce() -> anyhow::Error,
    change: impl FnOnce(&str) -> Option<(String, T)>,
) -> anyhow::Result<Option<T>> {
    use std::io::Write as _;
    let file = match skwir_core::os_path::canonical(link) {
        Ok(target) => target,
        Err(error) if error.kind() == io::ErrorKind::NotFound => link.to_path_buf(),
        Err(error) => return Err(error.into()),
    };
    let text = match read_regular_text(&file, limit) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };
    let Some((updated, value)) = change(&text) else {
        return Ok(None);
    };
    let parent = file.parent().ok_or_else(no_folder)?;
    std::fs::create_dir_all(parent)?;
    let refused = |error: &io::Error| {
        matches!(
            error.kind(),
            io::ErrorKind::ReadOnlyFilesystem | io::ErrorKind::PermissionDenied
        )
    };
    let mut staged = match tempfile::NamedTempFile::new_in(parent) {
        Ok(staged) => staged,
        Err(error) if refused(&error) => return Err(read_only()),
        Err(error) => return Err(error.into()),
    };
    if let Ok(metadata) = std::fs::metadata(&file) {
        std::fs::set_permissions(staged.path(), metadata.permissions())?;
    }
    staged.write_all(updated.as_bytes())?;
    // On the disk before it takes the file's place: renamed over it first,
    // a power cut could leave the desktop's bookmarks or its default
    // applications empty (XFS, ext4 without `auto_da_alloc`).
    staged.as_file().sync_all()?;
    take_place(staged, &file)?;
    Ok(Some(value))
}

/// The most a desktop's own file read here may hold (a desktop entry, a
/// `mimeapps.list`, an icon theme's `index.theme`, a toolkit's settings):
/// far above any made by hand or by a tool, so one that is larger is no
/// such file.
pub const DESKTOP_FILE: u64 = 4 << 20;

/// A private folder for what a helper hands skwir and is read at once (a
/// video's frames, a document's pages): in memory through `/dev/shm` where
/// that exists and is writable, else the ordinary temporary folder, which
/// may be on a disk. There a file rewritten in place (the video's one
/// frame slot) stays in the page cache, its pages written out at most at
/// the system's pace, not at each frame; Windows is asked to keep the
/// video's slot in memory (`FILE_ATTRIBUTE_TEMPORARY`).
///
/// Removed when dropped; one its process left, killed before (a helper
/// killed, a test stopped), is removed by the next process to make one:
/// each is named by the process that made it (`Owner`), whose lock the
/// system lets go of only as it ends, so no folder of a process still
/// running is removed, whichever process it is and however many run
/// (`sweep`). `prefix` begins with `skwir-` and ends with `-`.
///
/// # Errors
/// When no folder can be made.
pub fn memory_dir(prefix: &str) -> std::io::Result<tempfile::TempDir> {
    debug_assert!(prefix.starts_with("skwir-") && prefix.ends_with('-'));
    if let Some(dir) = owner().and_then(|owner| owner.make(prefix).ok()) {
        return Ok(dir);
    }
    // Named by no one, never swept: as made before owners were named.
    #[cfg(target_os = "linux")]
    if let Ok(dir) = tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in("/dev/shm")
    {
        return Ok(dir);
    }
    tempfile::Builder::new().prefix(prefix).tempdir()
}

/// What names the folders a process makes ([`memory_dir`]): a lock file
/// of its own beside them, `skwir-owner-<token>.lock`, locked while it
/// lives and let go of by the system as it ends, killed or not; each
/// folder is `<prefix><token>.<random>`. Made at the process's first
/// folder, which sweeps what processes ended left ([`sweep`]).
struct Owner {
    /// Where its folders are made: `/dev/shm` where it can be, else the
    /// temporary folder.
    base: std::path::PathBuf,
    /// The random part of its lock file's name.
    token: String,
    /// Its lock file, locked for as long as the process lives: never
    /// closed (a static's), so only the process's end lets go of it.
    _held: std::fs::File,
}

/// What begins an owner's lock file's name, and ends it.
const OWNER_PREFIX: &str = "skwir-owner-";
const OWNER_SUFFIX: &str = ".lock";
/// How long an owner's token is: letters and digits, never the same twice.
const TOKEN_LEN: usize = 12;

/// This process's owner, made with its first folder; `None` where no lock
/// file can be made (its folders then named by no one).
fn owner() -> Option<&'static Owner> {
    static OWNER: std::sync::OnceLock<Option<Owner>> = std::sync::OnceLock::new();
    OWNER
        .get_or_init(|| {
            #[cfg(target_os = "linux")]
            let bases = [Path::new("/dev/shm").to_path_buf(), std::env::temp_dir()];
            #[cfg(not(target_os = "linux"))]
            let bases = [std::env::temp_dir()];
            let owner = bases.iter().find_map(|base| Owner::new(base).ok())?;
            let removed = sweep(&owner.base, Some(&owner.token));
            if removed > 0 {
                tracing::debug!(removed, "scratch folders of ended processes removed");
            }
            Some(owner)
        })
        .as_ref()
}

impl Owner {
    /// A lock file of its own in `base`, locked, under its name still: a
    /// sweep that found it before it was locked took it for an ended
    /// process's and removed it (with no folder yet), and another is made.
    fn new(base: &Path) -> io::Result<Owner> {
        let mut last = io::Error::other("no lock file could be made");
        for _ in 0..4 {
            let made = tempfile::Builder::new()
                .prefix(OWNER_PREFIX)
                .suffix(OWNER_SUFFIX)
                .rand_bytes(TOKEN_LEN)
                .disable_cleanup(true)
                .make_in(base, lock_file)?;
            let (file, path) = made.keep().map_err(|e| e.error)?;
            if let Err(error) = file.lock() {
                last = error;
                let _ = std::fs::remove_file(&path);
                continue;
            }
            if still_named(&path, &file) {
                let token = path
                    .file_name()
                    .and_then(|name| lock_token(name.to_str()?))
                    .ok_or_else(|| io::Error::other("a lock file of no token"))?
                    .to_owned();
                return Ok(Owner {
                    base: base.to_path_buf(),
                    token,
                    _held: file,
                });
            }
        }
        Err(last)
    }

    /// A folder of its own, `<prefix><token>.<random>`.
    fn make(&self, prefix: &str) -> io::Result<tempfile::TempDir> {
        tempfile::Builder::new()
            .prefix(&format!("{prefix}{}.", self.token))
            .tempdir_in(&self.base)
    }
}

/// A new lock file at `path`, the user's alone: no one else may hold its
/// lock and so keep it from being swept. On Windows no other process may
/// delete it while it is open: a sweep that locked it before its owner
/// did cannot take it away.
fn lock_file(path: &Path) -> io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};
        options.share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE);
    }
    options.open(path)
}

/// Whether `path` names `file` still: a sweep may have removed it before
/// it was locked. Windows lets no one remove it while it is open
/// (`lock_file`).
fn still_named(path: &Path, file: &std::fs::File) -> bool {
    #[cfg(unix)]
    {
        let identity = |meta: &std::fs::Metadata| skwir_core::fs::stat::identity(path, meta);
        match (std::fs::symlink_metadata(path), file.metadata()) {
            (Ok(named), Ok(held)) => identity(&named).is_some_and(|id| identity(&held) == Some(id)),
            _ => false,
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (path, file);
        true
    }
}

/// The token of an owner's lock file named `name`.
fn lock_token(name: &str) -> Option<&str> {
    let token = name
        .strip_prefix(OWNER_PREFIX)?
        .strip_suffix(OWNER_SUFFIX)?;
    is_token(token).then_some(token)
}

/// The token of the owner of a folder named `name`
/// (`<prefix><token>.<random>`, the prefix `skwir-…-`).
fn folder_token(name: &str) -> Option<&str> {
    if !name.starts_with("skwir-") {
        return None;
    }
    let (head, random) = name.rsplit_once('.')?;
    let (_, token) = head.rsplit_once('-')?;
    (is_token(token) && !random.is_empty() && random.bytes().all(|b| b.is_ascii_alphanumeric()))
        .then_some(token)
}

fn is_token(token: &str) -> bool {
    token.len() == TOKEN_LEN && token.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// Remove from `base` the folders of the owners that ended: those whose
/// lock file this process can lock, which the system lets go of only as
/// its process ends. Each owner's folders go, then its lock file, kept
/// while one of them could not be removed, for the next sweep. Left: an
/// owner's whose lock is held (its process runs), `own`, a folder whose
/// owner has no lock file (named by no one: made before owners were
/// named, or by another program), what is not the user's. How many
/// folders were removed.
fn sweep(base: &Path, own: Option<&str>) -> usize {
    let Ok(listed) = std::fs::read_dir(base) else {
        return 0;
    };
    let mut owners = Vec::new();
    let mut folders: std::collections::HashMap<String, Vec<std::ffi::OsString>> =
        std::collections::HashMap::new();
    for entry in listed.flatten() {
        let name = entry.file_name();
        let Some(text) = name.to_str() else { continue };
        if let Some(token) = lock_token(text) {
            owners.push(token.to_owned());
        } else if let Some(token) = folder_token(text) {
            folders
                .entry(token.to_owned())
                .or_default()
                .push(name.clone());
        }
    }
    let mut removed = 0;
    for token in owners {
        if own == Some(token.as_str()) {
            continue;
        }
        let lock = base.join(format!("{OWNER_PREFIX}{token}{OWNER_SUFFIX}"));
        let Ok(file) = std::fs::File::open(&lock) else {
            continue;
        };
        // Held: its process runs; refused otherwise: nothing is known.
        if !file.metadata().is_ok_and(|meta| users(&meta)) || file.try_lock().is_err() {
            continue;
        }
        let mut all = true;
        for name in folders.remove(&token).unwrap_or_default() {
            let path = base.join(name);
            let folder = std::fs::symlink_metadata(&path)
                .is_ok_and(|meta| meta.file_type().is_dir() && users(&meta));
            if folder && std::fs::remove_dir_all(&path).is_ok() {
                removed += 1;
            } else {
                all = false;
            }
        }
        if all {
            let _ = std::fs::remove_file(&lock);
        }
    }
    removed
}

/// Whether what `meta` describes is the user's: `/dev/shm` is every
/// user's.
fn users(meta: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        meta.uid() == rustix::process::geteuid().as_raw()
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A lock file and a folder of an owner `token`, made by hand: an
    /// ended process's, its lock let go of.
    fn left_by(base: &Path, token: &str, folder: &str) -> std::path::PathBuf {
        std::fs::write(
            base.join(format!("{OWNER_PREFIX}{token}{OWNER_SUFFIX}")),
            b"",
        )
        .unwrap();
        let dir = base.join(format!("skwir-pages-{token}.{folder}"));
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("it's a page, été.png"), b"x").unwrap();
        dir
    }

    /// What an ended process left is swept: its folders, then its lock
    /// file. A running owner's folders stay (its lock held, here by
    /// another file of this process), this process's own too, and what
    /// is named by no owner, or by one without a lock file.
    #[test]
    fn what_ended_processes_left_is_swept_and_nothing_else() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path();
        let ended = left_by(base, "EndedAbc1234", "Zx81Qp");
        let ended_too = base.join("skwir-video-EndedAbc1234.Ab12Cd");
        std::fs::create_dir(&ended_too).unwrap();

        let running = left_by(base, "Running12345", "Qw3rTy");
        let held =
            std::fs::File::open(base.join(format!("{OWNER_PREFIX}Running12345{OWNER_SUFFIX}")))
                .unwrap();
        held.lock().unwrap();

        let own = Owner::new(base).unwrap();
        let mine = own.make("skwir-archive-").unwrap();
        let unnamed = base.join("skwir-video-AbCdEf");
        std::fs::create_dir(&unnamed).unwrap();
        let no_lock = base.join("skwir-pages-NoLockFile12.Ab12Cd");
        std::fs::create_dir(&no_lock).unwrap();
        let other = base.join("other-program-Running12345.Ab12Cd");
        std::fs::create_dir(&other).unwrap();

        assert_eq!(sweep(base, Some(&own.token)), 2);
        assert!(
            !ended.exists() && !ended_too.exists(),
            "an ended one's gone"
        );
        assert!(
            !base
                .join(format!("{OWNER_PREFIX}EndedAbc1234{OWNER_SUFFIX}"))
                .exists(),
            "its lock file after them"
        );
        for kept in [
            &running,
            &mine.path().to_path_buf(),
            &unnamed,
            &no_lock,
            &other,
        ] {
            assert!(kept.exists(), "{} kept", kept.display());
        }
        assert_eq!(sweep(base, Some(&own.token)), 0, "nothing more");

        // Its owner ended: its lock let go of.
        drop(held);
        assert_eq!(sweep(base, Some(&own.token)), 1);
        assert!(!running.exists());
        // Swept by another process: this one's own is held.
        assert_eq!(sweep(base, None), 0);
        assert!(mine.path().exists());
    }

    /// The names an owner gives are the ones a sweep reads.
    #[test]
    fn an_owner_s_names_are_read_back() {
        let tmp = tempfile::tempdir().unwrap();
        let own = Owner::new(tmp.path()).unwrap();
        let dir = own.make("skwir-djvu-test-").unwrap();
        let name = dir.path().file_name().unwrap().to_str().unwrap();
        assert_eq!(folder_token(name), Some(own.token.as_str()));
        assert_eq!(
            lock_token(&format!("{OWNER_PREFIX}{}{OWNER_SUFFIX}", own.token)),
            Some(own.token.as_str())
        );
        assert_eq!(folder_token("skwir-video-AbCdEf"), None, "named by no one");
        assert_eq!(lock_token("skwir-owner-short.lock"), None);
    }

    /// What a process killed left goes at the next sweep, and not before:
    /// while it runs, its folder stays. The process is this test's own
    /// program, started again to make a folder and wait.
    #[cfg(unix)]
    #[test]
    fn a_killed_process_s_folder_goes_and_a_running_one_s_stays() {
        use std::io::BufRead;
        const CHILD: &str = "SKWIR_TEST_SCRATCH_OWNER";
        if let Some(base) = std::env::var_os(CHILD) {
            let own = Owner::new(Path::new(&base)).unwrap();
            let dir = own.make("skwir-pages-").unwrap();
            println!("made {}", dir.path().display());
            std::thread::sleep(std::time::Duration::from_secs(60));
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        // This test's own program, from the console the tests run in.
        #[allow(clippy::disallowed_methods)]
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "file::tests::a_killed_process_s_folder_goes_and_a_running_one_s_stays",
                "--nocapture",
            ])
            .env(CHILD, tmp.path())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let out = std::io::BufReader::new(child.stdout.take().unwrap());
        let made = out
            .lines()
            .map_while(Result::ok)
            .find_map(|line| line.strip_prefix("made ").map(std::path::PathBuf::from))
            .expect("the child made its folder");
        assert!(made.exists());
        assert_eq!(sweep(tmp.path(), None), 0, "its process runs");
        assert!(made.exists());
        child.kill().unwrap();
        child.wait().unwrap();
        assert_eq!(sweep(tmp.path(), None), 1, "killed");
        assert!(!made.exists());
        assert_eq!(
            std::fs::read_dir(tmp.path()).unwrap().count(),
            0,
            "its lock file gone too"
        );
    }
}
