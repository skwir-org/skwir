//! Putting a file in the trash on Linux (freedesktop.org's trash
//! specification) where the `trash` crate puts it, but with the mount
//! table read, and the home trash and each folder resolved, once for a
//! plan rather than once for each file: of the 410 µs the crate took to
//! trash a file, 290 went to finding again where.
//!
//! The crate's way: a file whose volume (the longest mount point holding
//! its resolved path) is the home trash's goes to the home trash; another
//! to its volume's `.Trash/<uid>` when that is a real folder with the
//! sticky bit and the user's folder is in it, else to `.Trash-<uid>`,
//! made if missing, and to the home trash when the volume refuses it
//! that. Its record is written first, under the first name free (`a`,
//! `a.2`…), then the file renamed onto it without replacing anything. One
//! a rename cannot take there (another filesystem: a subvolume of the
//! home folder's, a volume refusing a trash folder) is left for the step
//! to move as any move across volumes is (`builtin::move_entry`), its
//! name and record kept for it.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::OsString;
use std::io::{self, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// Where a plan puts files in the trash, learned as it goes.
#[derive(Default)]
pub(super) struct Trashes(RefCell<Option<Known>>);

/// A file put in the trash, or to be.
pub(super) enum Put {
    Done,
    /// Its place in the trash, on another filesystem, for the step to
    /// move it to (nothing is there yet), and its record, written: removed
    /// if the move fails.
    Across {
        item: PathBuf,
        info: PathBuf,
    },
}

struct Known {
    /// Every mount point, the longest first: none when the system says
    /// nothing of them (no `/proc`), every file then on one filesystem.
    mounts: Vec<PathBuf>,
    /// The home trash, resolved, and the mount point holding it: `None`
    /// with neither `$XDG_DATA_HOME` nor `$HOME`.
    home: Option<(PathBuf, PathBuf)>,
    /// Folders resolved, by the path they were given as.
    folders: HashMap<PathBuf, PathBuf>,
    /// The trash of each mount point, its `files` and `info` made.
    trashes: HashMap<PathBuf, PathBuf>,
}

impl Trashes {
    /// Put the file at `path` in the trash, or say where it goes.
    pub(super) fn put(&self, path: &Path) -> io::Result<Put> {
        let mut known = self.0.borrow_mut();
        let known = known.get_or_insert_with(Known::new);
        let (Some(folder), Some(name)) = (path.parent(), path.file_name()) else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the top of a filesystem cannot be put in the trash",
            ));
        };
        let folder = if let Some(resolved) = known.folders.get(folder) {
            resolved.clone()
        } else {
            let resolved = resolved_or_above(folder)?;
            known.folders.insert(folder.to_owned(), resolved.clone());
            resolved
        };
        let path = folder.join(name);
        let top = known.top_of(&path);
        let trash = if let Some(trash) = known.trashes.get(&top) {
            trash.clone()
        } else {
            let home = || {
                known
                    .home
                    .as_ref()
                    .map(|(home, _)| home.clone())
                    .ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::NotFound,
                            "there is no home folder for its trash",
                        )
                    })
            };
            let trash = if known.home.as_ref().map(|(_, top)| top) == Some(&top) {
                home()?
            } else {
                match volume_trash(&top) {
                    Ok(trash) => trash,
                    // Refused a trash folder of its own: the home trash, as
                    // the crate and the specification have it.
                    Err(error) if error.kind() == io::ErrorKind::PermissionDenied => home()?,
                    Err(error) => return Err(error),
                }
            };
            std::fs::create_dir_all(trash.join("files"))?;
            std::fs::create_dir_all(trash.join("info"))?;
            known.trashes.insert(top, trash.clone());
            trash
        };
        put_in(&path, &trash)
    }
}

impl Known {
    fn new() -> Known {
        let mut mounts: Vec<PathBuf> = std::fs::read("/proc/self/mountinfo")
            .map(|text| crate::fs::volume::mountinfo(&text))
            .unwrap_or_default()
            .into_iter()
            .map(|mount| mount.point)
            .filter(|point| !point.as_os_str().is_empty())
            .collect();
        mounts.sort_by_key(|point| std::cmp::Reverse(point.as_os_str().len()));
        let mut known = Known {
            mounts,
            home: None,
            folders: HashMap::new(),
            trashes: HashMap::new(),
        };
        known.home = home_trash()
            .and_then(|home| resolved_or_above(&home).ok())
            .map(|home| {
                let top = known.top_of(&home);
                (home, top)
            });
        known
    }

    /// The mount point holding `path`, the root when none says so.
    fn top_of(&self, path: &Path) -> PathBuf {
        self.mounts
            .iter()
            .find(|point| path.starts_with(point))
            .cloned()
            .unwrap_or_else(|| PathBuf::from("/"))
    }
}

/// The home trash, where the crate takes it: `$XDG_DATA_HOME/Trash`, else
/// `~/.local/share/Trash`.
fn home_trash() -> Option<PathBuf> {
    let set = |name| std::env::var_os(name).filter(|value| !value.is_empty());
    set("XDG_DATA_HOME")
        .map(|data| PathBuf::from(data).join("Trash"))
        .or_else(|| set("HOME").map(|home| PathBuf::from(home).join(".local/share/Trash")))
}

/// `path` with its links and dots resolved, the part that does not exist
/// yet kept as it is.
fn resolved_or_above(path: &Path) -> io::Result<PathBuf> {
    let mut missing = Vec::new();
    let mut at = path;
    loop {
        match std::fs::canonicalize(at) {
            Ok(resolved) => {
                return Ok(missing
                    .into_iter()
                    .rev()
                    .fold(resolved, |path, name: &std::ffi::OsStr| path.join(name)));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let (Some(parent), Some(name)) = (at.parent(), at.file_name()) else {
                    return Err(error);
                };
                missing.push(name);
                at = parent;
            }
            Err(error) => return Err(error),
        }
    }
}

/// A volume's trash for this user, at its top `top`: `.Trash/<uid>` when
/// the volume keeps a shared trash and the user's folder is in it, else
/// `.Trash-<uid>`, made if missing.
fn volume_trash(top: &Path) -> io::Result<PathBuf> {
    let uid = rustix::process::getuid().as_raw();
    let shared = top.join(".Trash");
    // A link is not the volume's shared trash, nor one without the sticky
    // bit: anyone could take what is in it.
    if std::fs::symlink_metadata(&shared).is_ok_and(|m| m.is_dir() && m.mode() & 0o1000 != 0) {
        let own = shared.join(uid.to_string());
        if own.is_dir() {
            return Ok(own);
        }
    }
    let own = top.join(format!(".Trash-{uid}"));
    if !own.is_dir() {
        std::fs::create_dir(&own)?;
    }
    Ok(own)
}

/// Put the file at `path` (resolved) in the trash `trash`: its record
/// first, under the first name free there, then the file renamed onto it.
fn put_in(path: &Path, trash: &Path) -> io::Result<Put> {
    let Some(name) = path.file_name() else {
        return Err(io::ErrorKind::InvalidInput.into());
    };
    let record = record(path);
    for n in 1usize.. {
        let mut named = name.to_owned();
        if n > 1 {
            named.push(format!(".{n}"));
        }
        let mut info_name = named.clone();
        info_name.push(".trashinfo");
        let info = trash.join("info").join(&info_name);
        let mut file = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&info)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };
        if let Err(error) = file.write_all(record.as_bytes()) {
            drop(file);
            let _ = std::fs::remove_file(&info);
            return Err(error);
        }
        drop(file);
        let item = trash.join("files").join(&named);
        match renamed(path, &item) {
            Ok(()) => return Ok(Put::Done),
            Err(error) if error.kind() == io::ErrorKind::CrossesDevices => {
                return Ok(Put::Across { item, info });
            }
            Err(error) => {
                let _ = std::fs::remove_file(&info);
                if error.kind() != io::ErrorKind::AlreadyExists {
                    return Err(error);
                }
            }
        }
    }
    Err(io::ErrorKind::AlreadyExists.into())
}

/// Rename `from` to `to`, never replacing what is there (`AlreadyExists`).
fn renamed(from: &Path, to: &Path) -> io::Result<()> {
    match rustix::fs::renameat_with(
        rustix::fs::CWD,
        from,
        rustix::fs::CWD,
        to,
        rustix::fs::RenameFlags::NOREPLACE,
    ) {
        // A filesystem that cannot rename without replacing: a placeholder
        // made new holds the name, then the rename takes its place.
        Err(rustix::io::Errno::INVAL | rustix::io::Errno::NOSYS) => {
            if std::fs::symlink_metadata(from)?.is_dir() {
                std::fs::create_dir(to)?;
            } else {
                std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(to)?;
            }
            std::fs::rename(from, to).inspect_err(|_| {
                let _ = std::fs::remove_dir(to).or_else(|_| std::fs::remove_file(to));
            })
        }
        other => other.map_err(io::Error::from),
    }
}

/// The record of the file at `path` put in the trash now: its path, every
/// byte of a name but the unreserved ones percent-encoded, and the local
/// time, as the crate writes them.
fn record(path: &Path) -> String {
    let mut encoded = OsString::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(part) => {
                encoded.push(percent_encoded(part.as_bytes()));
            }
            other => encoded.push(other.as_os_str()),
        }
        if !matches!(component, std::path::Component::RootDir) {
            encoded.push("/");
        }
    }
    let mut encoded = encoded.to_string_lossy().into_owned();
    if encoded.len() > 1 && encoded.ends_with('/') {
        encoded.pop();
    }
    format!(
        "[Trash Info]\nPath={encoded}\nDeletionDate={}\n",
        chrono::Local::now().format("%Y-%m-%dT%H:%M:%S")
    )
}

/// `bytes` with every byte but the unreserved ones (letters, digits,
/// `-._~`) written `%XX`.
fn percent_encoded(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for &b in bytes {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            let _ = std::fmt::Write::write_fmt(&mut out, format_args!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_taken_in_the_trash_is_followed_by_a_number() {
        let base = dirs::home_dir().unwrap().join(".cache/skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        let tmp = tempfile::tempdir_in(base).unwrap();
        let trash = tmp.path().join("Trash");
        std::fs::create_dir_all(trash.join("files")).unwrap();
        std::fs::create_dir_all(trash.join("info")).unwrap();
        for (folder, body) in [("one", "first"), ("two", "second")] {
            let path = tmp.path().join(folder).join("it's a.txt");
            std::fs::create_dir(path.parent().unwrap()).unwrap();
            std::fs::write(&path, body).unwrap();
            assert!(matches!(put_in(&path, &trash).unwrap(), Put::Done));
            assert!(!path.exists());
        }
        let read = |name: &str| std::fs::read_to_string(trash.join(name)).unwrap();
        assert_eq!(read("files/it's a.txt"), "first");
        assert_eq!(read("files/it's a.txt.2"), "second");
        let record = read("info/it's a.txt.2.trashinfo");
        let (from, _) = crate::op::records::parse_trash_info(&record, None).unwrap();
        assert_eq!(from, tmp.path().join("two/it's a.txt"));
    }

    #[test]
    fn a_record_says_the_path_as_the_crate_encodes_it() {
        let text = record(Path::new("/data/it's a café/notes~1.txt"));
        let (path, date) = crate::op::records::parse_trash_info(&text, None).unwrap();
        assert_eq!(path, Path::new("/data/it's a café/notes~1.txt"));
        assert!(text.starts_with(
            "[Trash Info]\nPath=/data/it%27s%20a%20caf%C3%A9/notes~1.txt\nDeletionDate="
        ));
        let date = date.unwrap();
        assert_eq!((date.len(), &date[4..5], &date[10..11]), (19, "-", "T"));
    }
}
