//! What a store keeps for a file, as bytes: an extended attribute of the
//! file, or a file beside it. An edit of notes records these before it
//! writes and its inverse puts them back exactly, so undo depends on no
//! store reading and writing its format perfectly.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Where a store keeps a file's notes.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum RawKey {
    /// An extended attribute of the file, through a link.
    Attribute { name: String },
    /// A file beside it: a path relative to its folder (`._x`,
    /// `.ts/x.json`).
    Beside {
        #[serde(with = "crate::os_path")]
        #[schemars(with = "String")]
        path: PathBuf,
    },
}

/// The bytes at a key, `None` for nothing there.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Raw {
    pub key: RawKey,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Vec<u8>>,
}

fn missing(e: &std::io::Error) -> bool {
    e.kind() == std::io::ErrorKind::NotFound
        || crate::fs::xattr::missing(e)
        || crate::fs::xattr::refused(e)
}

/// Where `key` is for the file at `path`.
fn beside(path: &Path, relative: &Path) -> PathBuf {
    path.parent().unwrap_or(Path::new("")).join(relative)
}

/// The bytes at `key` for the file at `path` now.
///
/// # Errors
/// When they are there and cannot be read.
pub fn read(path: &Path, key: &RawKey) -> std::io::Result<Raw> {
    let value = match key {
        RawKey::Attribute { name } => match crate::fs::xattr::get_deref(path, name) {
            Ok(value) => value,
            Err(e) if missing(&e) => None,
            Err(e) => return Err(e),
        },
        RawKey::Beside { path: relative } => match crate::fs::file::read_regular(
            &beside(path, relative),
            crate::fs::file::SIDE_FILE_LIMIT,
        ) {
            Ok(value) => Some(value),
            Err(e) if missing(&e) => None,
            Err(e) => return Err(e),
        },
    };
    Ok(Raw {
        key: key.clone(),
        value,
    })
}

/// Put `raw` back for the file at `path`: the attribute or the file
/// beside it set to its bytes, or removed. A file beside it is replaced
/// whole, keeping the mode of the one it replaces.
///
/// # Errors
/// When it cannot be written or removed.
pub fn write(path: &Path, raw: &Raw) -> std::io::Result<()> {
    match (&raw.key, &raw.value) {
        (RawKey::Attribute { name }, Some(value)) => crate::fs::xattr::set_deref(path, name, value),
        (RawKey::Attribute { name }, None) => match crate::fs::xattr::remove_deref(path, name) {
            Err(e) if missing(&e) => Ok(()),
            r => r,
        },
        (RawKey::Beside { path: relative }, Some(value)) => replace(&beside(path, relative), value),
        (RawKey::Beside { path: relative }, None) => {
            match std::fs::remove_file(beside(path, relative)) {
                Err(e) if missing(&e) => Ok(()),
                r => r,
            }
        }
    }
}

/// Replace the file at `target` with `bytes` whole (a reader sees the old
/// one or the new one), creating its folder if needed and keeping the
/// mode of the file it replaces.
///
/// # Errors
/// When it cannot be written.
pub fn replace(target: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = target
        .parent()
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    std::fs::create_dir_all(dir)?;
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    let prefix = format!(".{name}.");
    let mut builder = tempfile::Builder::new();
    builder.prefix(&prefix).suffix(".tmp");
    // A new file gets what the umask allows, as one created in place would.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o666));
    }
    let staged = builder.tempfile_in(dir)?;
    if let Ok(metadata) = std::fs::metadata(target) {
        std::fs::set_permissions(staged.path(), metadata.permissions())?;
    }
    let mut staged = staged;
    staged.write_all(bytes)?;
    crate::fs::file::take_place(staged, target)
}

/// The bytes at each of `keys` now, once each.
///
/// # Errors
/// When one is there and cannot be read.
pub fn snapshot(path: &Path, keys: &[RawKey]) -> std::io::Result<Vec<Raw>> {
    let mut out: Vec<Raw> = Vec::new();
    for key in keys {
        if !out.iter().any(|r| &r.key == key) {
            out.push(read(path, key)?);
        }
    }
    Ok(out)
}

/// Put every one of `raws` back, trying each; the first error.
///
/// # Errors
/// The first that could not be put back.
pub fn restore(path: &Path, raws: &[Raw]) -> std::io::Result<()> {
    let mut first = None;
    for raw in raws {
        if let Err(e) = write(path, raw) {
            first.get_or_insert(e);
        }
    }
    first.map_or(Ok(()), Err)
}

// Extended attributes: Unix's.
#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn home_tempdir() -> tempfile::TempDir {
        let base = dirs::home_dir().unwrap().join(".cache").join("skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        tempfile::Builder::new()
            .prefix("notes-raw-")
            .tempdir_in(base)
            .unwrap()
    }

    // Extended attributes: Unix's.

    #[cfg(unix)]
    #[test]
    fn what_a_store_kept_is_put_back_byte_for_byte() {
        let tmp = home_tempdir();
        let file = tmp.path().join("it's a.txt");
        std::fs::write(&file, b"x").unwrap();
        let keys = [
            RawKey::Attribute {
                name: "user.xdg.tags".into(),
            },
            RawKey::Beside {
                path: "._it's a.txt".into(),
            },
            RawKey::Beside {
                path: ".ts/it's a.txt.json".into(),
            },
        ];
        crate::fs::xattr::set(&file, "user.xdg.tags", b"a,b").unwrap();
        std::fs::write(tmp.path().join("._it's a.txt"), [0, 5, 22, 7]).unwrap();
        let before = snapshot(&file, &keys).unwrap();
        assert_eq!(before[0].value.as_deref(), Some(b"a,b".as_slice()));
        assert_eq!(before[2].value, None, "nothing there");

        crate::fs::xattr::remove(&file, "user.xdg.tags").unwrap();
        std::fs::write(tmp.path().join("._it's a.txt"), b"changed").unwrap();
        replace(&tmp.path().join(".ts/it's a.txt.json"), b"{}").unwrap();
        restore(&file, &before).unwrap();
        assert_eq!(snapshot(&file, &keys).unwrap(), before);
        assert!(!tmp.path().join(".ts/it's a.txt.json").exists());
        let left: Vec<_> = std::fs::read_dir(tmp.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .filter(|n| n.to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(left.is_empty(), "{left:?}");
    }
}
