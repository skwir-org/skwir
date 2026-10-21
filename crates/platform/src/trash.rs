//! The desktop's trash, laid out as freedesktop.org's trash specification
//! says: a trash is a folder of `files`, what was put there, and `info`,
//! one `<name>.trashinfo` each, saying where it came from and when. The
//! home trash is `$XDG_DATA_HOME/Trash`; a volume's are `.Trash/<uid>`
//! (in a sticky `.Trash` the administrator made) and `.Trash-<uid>` at its
//! top. Putting files there, restoring and deleting them is the core's
//! (the trash crate, `skwir_core::op`); this reads what each trash holds,
//! from the trashes a caller names.

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
        let sticky = is_sticky(&meta.permissions());
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

/// Whether `folder` is named as the user's trash on a volume is:
/// `.Trash-<uid>`, or `<uid>` in a `.Trash`. Reads nothing: a walk asks
/// it of each folder, where asking the volumes for theirs (`trashes`)
/// costs a read of each one's device.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn named_as_trash(folder: &Path) -> bool {
    let uid = || rustix::process::getuid().as_raw().to_string();
    match folder.file_name().and_then(std::ffi::OsStr::to_str) {
        Some(name) if name.starts_with(".Trash-") => name[".Trash-".len()..] == uid(),
        Some(name) if name.bytes().all(|b| b.is_ascii_digit()) => {
            folder.parent().and_then(Path::file_name) == Some(".Trash".as_ref()) && name == uid()
        }
        _ => false,
    }
}

/// No volume keeps a freedesktop trash of the user's here.
#[cfg(not(all(unix, not(target_os = "macos"))))]
pub fn named_as_trash(_: &Path) -> bool {
    false
}

#[cfg(unix)]
fn is_sticky(permissions: &std::fs::Permissions) -> bool {
    use std::os::unix::fs::PermissionsExt;
    permissions.mode() & 0o1000 != 0
}

/// No folder is sticky off Unix: no shared `.Trash` is trusted.
#[cfg(not(unix))]
fn is_sticky(_: &std::fs::Permissions) -> bool {
    false
}

/// Every trash of the user's on macOS: none read yet (ADR-035), its
/// `~/.Trash` keeping no record of where a file came from.
#[cfg(target_os = "macos")]
pub fn trashes() -> Vec<PathBuf> {
    Vec::new()
}

/// Every trash of the user's on Windows: their folder (named by their
/// SID) in the Recycle Bin of each local drive. Asks each drive: call it
/// from a worker.
#[cfg(windows)]
pub fn trashes() -> Vec<PathBuf> {
    let Some(sid) = crate::windows::user_sid() else {
        return Vec::new();
    };
    crate::windows::local_drives()
        .into_iter()
        .map(|root| root.join("$Recycle.Bin").join(&sid))
        .filter(|bin| bin.is_dir())
        .collect()
}

/// Every trash of the user's on Linux: the home trash and each volume's
/// that a plan reads too (`skwir_core::op::trash_folders_here`): no
/// network mount's, where asking can hang, nor an automount's. Asks each
/// volume: call it from a worker.
#[cfg(target_os = "linux")]
pub fn trashes() -> Vec<PathBuf> {
    skwir_core::op::trash_folders_here()
}

/// Every trash of the user's: the home trash, and those of the volumes
/// the sidebar shows (`places::mounts`) but network and FUSE ones, where
/// asking can hang. Asks each volume: call it from a worker.
#[cfg(all(unix, not(target_os = "macos"), not(target_os = "linux")))]
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
        return list_recycle_bin(trash);
    };
    let top = top_of(trash);
    let mut found = Vec::new();
    for info in infos.flatten() {
        let info = info.path();
        // Its file is named as it is, less `.trashinfo`: bytes, whatever
        // they are.
        let Some(name) = info.file_name().and_then(|name| {
            let stem = name.as_encoded_bytes().strip_suffix(b".trashinfo")?;
            Some(skwir_core::os_path::from_bytes(stem.to_vec()))
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

/// Where a relative `Path` of `trash`'s records starts: the volume a
/// volume's trash is at the top of (`<top>/.Trash-<uid>`,
/// `<top>/.Trash/<uid>`); `None` for the home trash.
fn top_of(trash: &Path) -> Option<&Path> {
    let name = trash.file_name()?.to_str()?;
    if name.starts_with(".Trash-") {
        trash.parent()
    } else {
        trash
            .parent()
            .filter(|p| p.file_name() == Some(".Trash".as_ref()))?
            .parent()
    }
}

/// What its trash says of the item `file` (in a trash's `files`, or a
/// Recycle Bin's `$R` item): its record alone read, the trash's others
/// left unread. `None` for a file that is no item of a trash, or whose
/// record says nothing of it. Reads the record: call it from a worker.
pub fn item(file: &Path, calendar: &Calendar) -> Option<Trashed> {
    use skwir_core::op::records::{self, Deleted};
    let info = records::record_of(file)?;
    let bytes = crate::file::read_regular(&info, crate::file::SIDE_FILE_LIMIT).ok()?;
    let trash = info.parent().and_then(Path::parent);
    let said = records::read(&info, &bytes, trash.and_then(top_of))?;
    Some(Trashed {
        file: file.to_path_buf(),
        info,
        original: said.original,
        deleted: match said.deleted {
            Some(Deleted::Local(date)) => calendar.local_time(&date),
            Some(Deleted::At(at)) => Some(at),
            None => None,
        },
    })
}

/// What a Windows Recycle Bin folder (`$Recycle.Bin\\<SID>`) holds: each
/// `$R` item its `$I` record speaks of, where it came from and when.
/// Nothing for a folder that is no such bin.
fn list_recycle_bin(bin: &Path) -> Vec<Trashed> {
    use skwir_core::op::records;
    let Ok(names) = std::fs::read_dir(bin) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for info in names.flatten().map(|e| e.path()) {
        let Some(file) = records::recycle_item_of(&info) else {
            continue;
        };
        if std::fs::symlink_metadata(&file).is_err() {
            continue;
        }
        let Ok(bytes) = crate::file::read_regular(&info, crate::file::SIDE_FILE_LIMIT) else {
            continue;
        };
        let Some((original, deleted)) = records::parse_recycle_record(&bytes) else {
            continue;
        };
        found.push(Trashed {
            file,
            info,
            original,
            deleted,
        });
    }
    found
}

/// A `.trashinfo`'s `Path` and `DeletionDate`: the core's reading
/// (`skwir_core::op::parse_trash_info`), which the operations share.
pub use skwir_core::op::parse_trash_info as parse_info;

#[cfg(test)]
// Helpers of the tests that run on Unix alone (ADR-035).
#[cfg_attr(not(unix), allow(dead_code, unused_imports))]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{list, parse_info, volume_trashes};
    use crate::calendar::Calendar;

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn a_volume_trash_is_known_by_its_name() {
        use super::named_as_trash;
        let uid = rustix::process::getuid().as_raw();
        assert!(named_as_trash(
            &Path::new("/media/stick").join(format!(".Trash-{uid}"))
        ));
        assert!(named_as_trash(
            &Path::new("/media/stick/.Trash").join(uid.to_string())
        ));
        assert!(!named_as_trash(
            &Path::new("/media/stick").join(format!(".Trash-{}", uid + 1))
        ));
        assert!(!named_as_trash(
            &Path::new("/media/stick/photos").join(uid.to_string())
        ));
        assert!(!named_as_trash(Path::new("/media/stick/.Trash-")));
    }

    // The freedesktop trash.
    #[cfg(unix)]
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

    /// What a Windows volume's Recycle Bin holds, read from its records
    /// (laid out here as Windows lays them out).
    #[test]
    #[allow(
        clippy::duration_suboptimal_units,
        reason = "a time since 1970 is written as the seconds or days it is"
    )]
    fn a_recycle_bin_lists_what_its_records_speak_of() {
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("$Recycle.Bin").join("S-1-5-21-1-2-3-1001");
        std::fs::create_dir_all(&bin).unwrap();
        let record = |path: &str| {
            let units: Vec<u16> = path.encode_utf16().chain([0]).collect();
            let mut out = Vec::new();
            out.extend_from_slice(&2u64.to_le_bytes());
            out.extend_from_slice(&9u64.to_le_bytes());
            // 2026-10-05T12:00:00Z as a FILETIME.
            out.extend_from_slice(
                &(116_444_736_000_000_000i64 + 1_791_201_600 * 10_000_000).to_le_bytes(),
            );
            out.extend_from_slice(&u32::try_from(units.len()).unwrap().to_le_bytes());
            for u in units {
                out.extend_from_slice(&u.to_le_bytes());
            }
            out
        };
        std::fs::write(bin.join("$IAB12CD.txt"), record(r"C:\Users\me\it's a.txt")).unwrap();
        std::fs::write(bin.join("$RAB12CD.txt"), b"the file").unwrap();
        // A record whose item is gone, and an item without a record.
        std::fs::write(bin.join("$IGONE.txt"), record(r"C:\gone.txt")).unwrap();
        std::fs::write(bin.join("$RLONE.txt"), b"x").unwrap();
        let found = list(&bin, &Calendar::utc());
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].file, bin.join("$RAB12CD.txt"));
        assert_eq!(found[0].info, bin.join("$IAB12CD.txt"));
        assert_eq!(found[0].original, PathBuf::from(r"C:\Users\me\it's a.txt"));
        assert_eq!(
            found[0].deleted,
            Some(std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_791_201_600))
        );
    }
}
