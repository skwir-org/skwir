//! Where a trash keeps an item and what it says of it (ADR-034, ADR-035):
//! freedesktop.org's trash (`files/x` and `info/x.trashinfo`, text) and,
//! on Windows, the Recycle Bin (`$Rxxxxxx.ext` and `$Ixxxxxx.ext` beside
//! it in `$Recycle.Bin\<SID>`, a binary record). Read from paths alone,
//! but what a record says, which is read from its bytes: a volume's trash
//! by its names (`.Trash-<uid>`, `.Trash/<uid>`), the home trash by where
//! it is (`$XDG_DATA_HOME/Trash`), never a folder merely named `Trash`.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// When an item was put in the trash, as its record says it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Deleted {
    /// freedesktop's `DeletionDate`, as written: a local time without a
    /// zone, which a calendar reads.
    Local(String),
    /// Windows' record: an instant.
    At(SystemTime),
}

/// What a trash's record says of its item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Said {
    /// Where it was put in the trash from.
    pub original: PathBuf,
    pub deleted: Option<Deleted>,
}

/// The record of the item at `item`, when `item` is an item of a trash:
/// freedesktop's `info/<name>.trashinfo` for `files/<name>`, and on
/// Windows the `$I` record beside a `$R` item.
pub fn record_of(item: &Path) -> Option<PathBuf> {
    freedesktop_record_of(item).or_else(|| {
        if cfg!(windows) {
            recycle_record_of(item)
        } else {
            None
        }
    })
}

/// The item the record at `record` speaks of.
pub fn item_of(record: &Path) -> Option<PathBuf> {
    freedesktop_item_of(record).or_else(|| {
        if cfg!(windows) {
            recycle_item_of(record)
        } else {
            None
        }
    })
}

/// What the record at `record` says, read from its `bytes`; a relative
/// path in a volume's freedesktop trash is read from `top`.
pub fn read(record: &Path, bytes: &[u8], top: Option<&Path>) -> Option<Said> {
    if record
        .file_name()?
        .as_encoded_bytes()
        .ends_with(b".trashinfo")
    {
        let text = std::str::from_utf8(bytes).ok()?;
        let (original, deleted) = parse_trash_info(text, top)?;
        return Some(Said {
            original,
            deleted: deleted.map(|d| Deleted::Local(d.to_owned())),
        });
    }
    let (original, deleted) = parse_recycle_record(bytes)?;
    Some(Said {
        original,
        deleted: deleted.map(Deleted::At),
    })
}

/// The home trash, `$XDG_DATA_HOME/Trash` as `xdg::data_home` says it (a
/// relative value ignored), whether it exists yet or not: where a plan
/// puts in it is where the trash is read from and known by.
pub(crate) fn home_trash() -> Option<PathBuf> {
    crate::xdg::data_home().map(|data| data.join("Trash"))
}

/// Whether `trash` is the home trash: as named, or as its links resolve
/// (`/home` a link to `/var/home`), which is looked up once.
fn is_home_trash(trash: &Path) -> bool {
    static HOME: std::sync::OnceLock<Vec<PathBuf>> = std::sync::OnceLock::new();
    HOME.get_or_init(|| {
        let Some(named) = home_trash() else {
            return Vec::new();
        };
        let resolved = named
            .parent()
            .and_then(|data| std::fs::canonicalize(data).ok())
            .map(|data| data.join("Trash"))
            .filter(|resolved| *resolved != named);
        std::iter::once(named).chain(resolved).collect()
    })
    .iter()
    .any(|home| home == trash)
}

/// freedesktop: a trash is the home trash (`$XDG_DATA_HOME/Trash`), or
/// on another volume `.Trash/<uid>` or `.Trash-<uid>`, by its name (its
/// place at the volume's top is not looked up: a path is all there is).
fn freedesktop_record_of(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?;
    let files = path.parent()?;
    if files.file_name()? != "files" {
        return None;
    }
    let trash = files.parent()?;
    let named = trash.file_name()?.to_str()?;
    let is_trash = named
        .strip_prefix(".Trash-")
        .is_some_and(|uid| uid.parse::<u32>().is_ok())
        || (named.parse::<u32>().is_ok()
            && trash.parent().and_then(Path::file_name) == Some(".Trash".as_ref()))
        || named == "Trash" && is_home_trash(trash);
    if !is_trash {
        return None;
    }
    let mut info = name.to_owned();
    info.push(".trashinfo");
    Some(trash.join("info").join(info))
}

fn freedesktop_item_of(info: &Path) -> Option<PathBuf> {
    let name = info
        .file_name()?
        .as_encoded_bytes()
        .strip_suffix(b".trashinfo")?;
    Some(
        info.parent()?
            .parent()?
            .join("files")
            .join(crate::os_path::from_bytes(name.to_vec())),
    )
}

/// What a `.trashinfo` says (freedesktop.org's trash specification): the
/// `Path` its file was put in the trash from (percent-encoded bytes,
/// relative to `top` in a volume's trash) and its `DeletionDate`, as
/// written. `None` for a text that is no trash info, or says no path.
pub fn parse_trash_info<'a>(
    text: &'a str,
    top: Option<&Path>,
) -> Option<(PathBuf, Option<&'a str>)> {
    let mut lines = text.lines().map(str::trim);
    if lines.next()? != "[Trash Info]" {
        return None;
    }
    let mut path = None;
    let mut date = None;
    for line in lines {
        // A line of another group ends this one.
        if line.starts_with('[') {
            break;
        }
        match line.split_once('=') {
            Some(("Path", value)) => {
                path = Some(PathBuf::from(crate::os_path::from_bytes(
                    crate::location::unescape(value),
                )));
            }
            Some(("DeletionDate", value)) => date = Some(value),
            _ => {}
        }
    }
    let path = path?;
    let path = if path.is_absolute() {
        path
    } else {
        top?.join(path)
    };
    Some((path, date))
}

/// The Recycle Bin's folder of one user on one volume, `$Recycle.Bin` and
/// a SID, whatever the case Windows wrote it in.
fn in_recycle_bin(dir: &Path) -> bool {
    let sid = dir.file_name().and_then(|n| n.to_str());
    let bin = dir
        .parent()
        .and_then(Path::file_name)
        .and_then(|n| n.to_str());
    sid.is_some_and(|s| s.starts_with("S-"))
        && bin.is_some_and(|b| b.eq_ignore_ascii_case("$Recycle.Bin"))
}

/// The `$I` record of a `$R` item: the same name past the two letters.
/// Asked on every system for what reads a Windows volume's bin.
pub fn recycle_record_of(item: &Path) -> Option<PathBuf> {
    let rest = item.file_name()?.to_str()?.strip_prefix("$R")?;
    let dir = item.parent()?;
    in_recycle_bin(dir).then(|| dir.join(format!("$I{rest}")))
}

/// The `$R` item of a `$I` record.
pub fn recycle_item_of(record: &Path) -> Option<PathBuf> {
    let rest = record.file_name()?.to_str()?.strip_prefix("$I")?;
    let dir = record.parent()?;
    in_recycle_bin(dir).then(|| dir.join(format!("$R{rest}")))
}

/// What a Windows Recycle Bin record (`$I…`) says: where its item was put
/// there from, and when. Version 1 (Vista to 8.1) holds the path in a
/// field of 260 UTF-16 units, version 2 (10 and later) says its length
/// first; both start with the version, the item's size and the time
/// (a `FILETIME`). `None` for bytes that are no such record.
pub fn parse_recycle_record(bytes: &[u8]) -> Option<(PathBuf, Option<SystemTime>)> {
    let u64_at = |at: usize| {
        bytes
            .get(at..at + 8)
            .map(|b| u64::from_le_bytes(b.try_into().expect("eight bytes")))
    };
    let version = u64_at(0)?;
    let deleted = i64::try_from(u64_at(16)?).ok().and_then(filetime);
    let units = match version {
        1 => bytes.get(24..24 + 520)?,
        2 => {
            let len = u32::from_le_bytes(bytes.get(24..28)?.try_into().ok()?);
            bytes.get(28..28 + 2 * usize::try_from(len).ok()?)?
        }
        _ => return None,
    };
    let mut path: Vec<u16> = units
        .as_chunks::<2>()
        .0
        .iter()
        .map(|unit| u16::from_le_bytes(*unit))
        .collect();
    if let Some(end) = path.iter().position(|u| *u == 0) {
        path.truncate(end);
    }
    if path.is_empty() {
        return None;
    }
    Some((wide_path(&path), deleted))
}

/// A path from UTF-16: exact on Windows, read with what is no UTF-16
/// replaced elsewhere (where a Windows record is only read in tests).
fn wide_path(units: &[u16]) -> PathBuf {
    #[cfg(windows)]
    {
        let path: std::ffi::OsString = std::os::windows::ffi::OsStringExt::from_wide(units);
        PathBuf::from(path)
    }
    #[cfg(not(windows))]
    PathBuf::from(String::from_utf16_lossy(units))
}

/// A `FILETIME` (100 ns since 1601) as a time; `None` before 1970.
fn filetime(at: i64) -> Option<SystemTime> {
    /// 1601 to 1970, in 100 ns.
    const EPOCH: i64 = 116_444_736_000_000_000;
    let since = u64::try_from(at.checked_sub(EPOCH)?).ok()?;
    Some(
        SystemTime::UNIX_EPOCH
            + Duration::from_secs(since / 10_000_000)
            + Duration::from_nanos((since % 10_000_000) * 100),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A version 2 record of `path`, deleted at `filetime`.
    fn record_v2(path: &str, filetime: i64) -> Vec<u8> {
        let units: Vec<u16> = path.encode_utf16().chain([0]).collect();
        let mut out = Vec::new();
        out.extend_from_slice(&2u64.to_le_bytes());
        out.extend_from_slice(&1234u64.to_le_bytes());
        out.extend_from_slice(&filetime.to_le_bytes());
        out.extend_from_slice(&u32::try_from(units.len()).unwrap().to_le_bytes());
        for u in units {
            out.extend_from_slice(&u.to_le_bytes());
        }
        out
    }

    #[test]
    #[allow(
        clippy::duration_suboptimal_units,
        reason = "a time since 1970 is written as the seconds or days it is"
    )]
    fn a_windows_record_says_where_its_item_came_from_and_when() {
        // 2026-10-05T12:00:00Z.
        let at = 116_444_736_000_000_000 + 1_791_201_600 * 10_000_000;
        let (path, deleted) =
            parse_recycle_record(&record_v2(r"C:\Users\it's me\café notes.txt", at)).unwrap();
        assert_eq!(path, PathBuf::from(r"C:\Users\it's me\café notes.txt"));
        assert_eq!(
            deleted,
            Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1_791_201_600))
        );
        // Version 1: a fixed field of 260 units.
        let mut v1 = Vec::new();
        v1.extend_from_slice(&1u64.to_le_bytes());
        v1.extend_from_slice(&5u64.to_le_bytes());
        v1.extend_from_slice(&at.to_le_bytes());
        let mut field: Vec<u16> = r"D:\old.doc".encode_utf16().collect();
        field.resize(260, 0);
        for u in field {
            v1.extend_from_slice(&u.to_le_bytes());
        }
        assert_eq!(
            parse_recycle_record(&v1).map(|(p, _)| p),
            Some(PathBuf::from(r"D:\old.doc"))
        );
        // Too short, another version, no path.
        assert_eq!(parse_recycle_record(&v1[..100]), None);
        let mut v3 = record_v2("x", at);
        v3[0] = 3;
        assert_eq!(parse_recycle_record(&v3), None);
        assert_eq!(parse_recycle_record(&record_v2("", at)), None);
    }

    #[test]
    fn a_windows_item_and_its_record_are_named_alike() {
        let bin = Path::new("/c/$Recycle.Bin/S-1-5-21-1-2-3-1001");
        assert_eq!(
            recycle_record_of(&bin.join("$RAB12CD.txt")),
            Some(bin.join("$IAB12CD.txt"))
        );
        assert_eq!(
            recycle_item_of(&bin.join("$IAB12CD.txt")),
            Some(bin.join("$RAB12CD.txt"))
        );
        let lower = Path::new("/c/$RECYCLE.BIN/S-1-5-18/$R0.png");
        assert!(recycle_record_of(lower).is_some(), "any case");
        assert_eq!(recycle_record_of(&bin.join("$IAB12CD.txt")), None);
        assert_eq!(recycle_record_of(Path::new("/c/Users/$Rnot.txt")), None);
        assert_eq!(
            recycle_record_of(Path::new("/c/$Recycle.Bin/desktop.ini/$Rx")),
            None
        );
    }

    // The free desktops' record, of Unix's paths.
    #[cfg(unix)]
    #[test]
    fn a_freedesktop_record_is_read_from_its_text() {
        let said = read(
            Path::new("/t/info/a.txt.trashinfo"),
            b"[Trash Info]\nPath=/home/u/a%20b.txt\nDeletionDate=2026-10-05T12:00:00\n",
            None,
        )
        .unwrap();
        assert_eq!(said.original, PathBuf::from("/home/u/a b.txt"));
        assert_eq!(
            said.deleted,
            Some(Deleted::Local("2026-10-05T12:00:00".into()))
        );
    }

    proptest::proptest! {
        /// A trash's records as no trash writes them: never a panic.
        #[test]
        fn any_record_is_read_without_a_panic(
            text in proptest::prop_oneof![
                crate::testing::mutated_text(
                    "[Trash Info]\nPath=/home/a/caf%C3%A9%20notes.txt\nDeletionDate=2026-03-12T08:00:00\n"
                ),
                proptest::prelude::any::<String>(),
            ],
            bytes in proptest::prop_oneof![
                crate::testing::mutated(record_v2(r"C:\Users\a\café notes.txt", 133_000_000_000_000_000)),
                crate::testing::bytes(256),
            ],
        ) {
            let _ = parse_trash_info(&text, None);
            let _ = parse_trash_info(&text, Some(std::path::Path::new("/run/media/a/stick")));
            let _ = parse_recycle_record(&bytes);
        }

        /// What one types where a location is read: never a panic.
        #[test]
        fn any_location_and_tags_typed_are_read_without_a_panic(text in proptest::prop_oneof![
            crate::testing::mutated_text("tags:/Places/France?in=%2Fhome%2Fa&deep=1"),
            crate::testing::mutated_text("~/My Files/café, draft, Places/Paris"),
            proptest::prelude::any::<String>(),
        ]) {
            let _ = crate::location::Location::parse(&text);
            let _ = crate::notes::parse_tags(&text);
            let _ = crate::fs::volume::mountinfo(&text);
        }
    }
}
