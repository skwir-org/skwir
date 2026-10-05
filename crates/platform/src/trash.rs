//! The desktop's trash, laid out as freedesktop.org's trash specification
//! says: a trash is a folder of `files`, what was put there, and `info`,
//! one `<name>.trashinfo` each, saying where it came from and when. The
//! home trash is `$XDG_DATA_HOME/Trash`; a volume's are `.Trash/<uid>`
//! (in a sticky `.Trash` the administrator made) and `.Trash-<uid>` at its
//! top. Putting files there, restoring and deleting them is the core's
//! (the trash crate, `bibli_core::op`); this reads what each trash holds,
//! from the trashes a caller names.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::calendar::Calendar;

/// A file in a trash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trashed {
    /// Where it is now: in the trash's `files`.
    pub file: PathBuf,
    /// What the trash says of it: its `info/<name>.trashinfo`.
    pub info: PathBuf,
    /// Where it was put there from, where restoring it puts it back.
    pub original: PathBuf,
    /// When it was put there; `None` when the trash does not say.
    pub deleted: Option<SystemTime>,
}

/// The home trash, whether it exists yet or not.
pub fn home_trash() -> Option<PathBuf> {
    crate::xdg::data_home().map(|data| data.join("Trash"))
}

/// The trashes of the volume mounted at `top` for the user `uid` that
/// exist: `.Trash/<uid>` where `.Trash` is a sticky folder and no link
/// (one anybody could have made is not trusted), and `.Trash-<uid>`.
/// Asks the volume: call it from a worker.
pub fn volume_trashes(top: &Path, uid: u32) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let shared = top.join(".Trash");
    if let Ok(meta) = std::fs::symlink_metadata(&shared) {
        use std::os::unix::fs::PermissionsExt;
        let sticky = meta.permissions().mode() & 0o1000 != 0;
        let own = shared.join(uid.to_string());
        if meta.is_dir() && sticky && own.is_dir() {
            found.push(own);
        }
    }
    let own = top.join(format!(".Trash-{uid}"));
    if std::fs::symlink_metadata(&own).is_ok_and(|meta| meta.is_dir()) {
        found.push(own);
    }
    found
}

/// Every trash of the user's: the home trash, and those of the volumes
/// the sidebar shows (`places::mounts`) but network and FUSE ones, where
/// asking can hang. Asks each volume: call it from a worker.
pub fn trashes() -> Vec<PathBuf> {
    let uid = rustix::process::getuid().as_raw();
    let mut found: Vec<PathBuf> = home_trash().into_iter().collect();
    for mount in crate::places::mounts() {
        if mount.fstype.starts_with("fuse") {
            continue;
        }
        for trash in volume_trashes(&mount.path, uid) {
            if !found.contains(&trash) {
                found.push(trash);
            }
        }
    }
    found
}

/// What `trash` holds: each file its `info` speaks of that is in its
/// `files`, its date read in `calendar`. A `.trashinfo` without its file,
/// or that does not say where the file came from, is left out. Reads the
/// trash: call it from a worker.
pub fn list(trash: &Path, calendar: &Calendar) -> Vec<Trashed> {
    let Ok(infos) = std::fs::read_dir(trash.join("info")) else {
        return Vec::new();
    };
    // Where a relative `Path` starts: the volume a volume's trash is at the
    // top of (`<top>/.Trash-<uid>`, `<top>/.Trash/<uid>`).
    let top = trash
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| {
            if name.starts_with(".Trash-") {
                trash.parent()
            } else {
                trash
                    .parent()
                    .filter(|p| p.file_name() == Some(".Trash".as_ref()))?
                    .parent()
            }
        });
    let mut found = Vec::new();
    for info in infos.flatten() {
        let info = info.path();
        // Its file is named as it is, less `.trashinfo`: bytes, whatever
        // they are.
        let Some(name) = info.file_name().and_then(|name| {
            use std::os::unix::ffi::{OsStrExt, OsStringExt};
            let stem = name.as_bytes().strip_suffix(b".trashinfo")?;
            Some(OsString::from_vec(stem.to_vec()))
        }) else {
            continue;
        };
        let file = trash.join("files").join(&name);
        if std::fs::symlink_metadata(&file).is_err() {
            continue;
        }
        // A volume's trash is anyone's to write in: a pipe named like an
        // info file must not hold the listing.
        let Ok(text) = crate::file::read_regular_text(&info, crate::file::SIDE_FILE_LIMIT) else {
            continue;
        };
        let Some((original, deleted)) = parse_info(&text, top) else {
            continue;
        };
        found.push(Trashed {
            file,
            info,
            original,
            deleted: deleted.and_then(|date| calendar.local_time(date)),
        });
    }
    found
}

/// A `.trashinfo`'s `Path` and `DeletionDate`: the core's reading
/// (`bibli_core::op::parse_trash_info`), which the operations share.
pub use bibli_core::op::parse_trash_info as parse_info;

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{list, parse_info, volume_trashes};
    use crate::calendar::Calendar;

    #[test]
    fn a_trashinfo_says_where_a_file_came_from_and_when() {
        let text = "[Trash Info]\nPath=/data/it%27s%20a%20caf%C3%A9.txt\nDeletionDate=2026-10-04T09:30:00\n";
        assert_eq!(
            parse_info(text, None),
            Some((
                PathBuf::from("/data/it's a café.txt"),
                Some("2026-10-04T09:30:00")
            ))
        );
        // A volume's trash may say it from the volume's top.
        assert_eq!(
            parse_info(
                "[Trash Info]\nPath=photos/a.jpg\n",
                Some(Path::new("/media/stick"))
            ),
            Some((PathBuf::from("/media/stick/photos/a.jpg"), None))
        );
        assert_eq!(parse_info("[Trash Info]\nPath=photos/a.jpg\n", None), None);
        assert_eq!(parse_info("[Desktop Entry]\nPath=/a\n", None), None);
        assert_eq!(
            parse_info("[Trash Info]\nDeletionDate=2026-10-04T09:30:00\n", None),
            None
        );
        let date = Calendar::utc().local_time("2026-10-04T09:30:00").unwrap();
        assert_eq!(
            date.duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
                % 86_400,
            9 * 3600 + 30 * 60
        );
    }

    #[test]
    fn a_trash_lists_the_files_it_holds_and_a_volume_its_own_trashes() {
        let top = tempfile::tempdir().unwrap();
        let trash = top.path().join(".Trash-1000");
        std::fs::create_dir_all(trash.join("files")).unwrap();
        std::fs::create_dir_all(trash.join("info")).unwrap();
        std::fs::write(trash.join("files/notes.txt"), "x").unwrap();
        std::fs::write(
            trash.join("info/notes.txt.trashinfo"),
            "[Trash Info]\nPath=docs/notes.txt\nDeletionDate=2026-10-04T09:30:00\n",
        )
        .unwrap();
        // Its file gone: nothing to list.
        std::fs::write(
            trash.join("info/gone.txt.trashinfo"),
            "[Trash Info]\nPath=/elsewhere/gone.txt\n",
        )
        .unwrap();
        let found = list(&trash, &Calendar::utc());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].file, trash.join("files/notes.txt"));
        assert_eq!(found[0].info, trash.join("info/notes.txt.trashinfo"));
        assert_eq!(found[0].original, top.path().join("docs/notes.txt"));
        assert!(found[0].deleted.is_some());
        assert_eq!(
            volume_trashes(top.path(), 1000),
            std::slice::from_ref(&trash)
        );
        assert!(volume_trashes(top.path(), 1001).is_empty());
        // A `.Trash` that is not sticky is not trusted.
        std::fs::create_dir_all(top.path().join(".Trash/1000")).unwrap();
        assert_eq!(volume_trashes(top.path(), 1000), [trash]);
    }
}
