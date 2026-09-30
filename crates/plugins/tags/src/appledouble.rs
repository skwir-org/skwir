//! `AppleDouble` files: where a Mac keeps a file's attributes (its Finder
//! tags, comment, where it was downloaded from) on a filesystem that cannot
//! hold them, in a file beside it named `._` and the file's name.
//!
//! The format is RFC 1740's, version 2, as macOS writes it (Apple's
//! `copyfile.c`): big-endian, a header naming two entries, the Finder info
//! first and the resource fork last. The Finder info entry is 32 bytes of
//! Finder info followed by the attributes: an `ATTR` header, one record
//! per attribute (its data's offset and length, flags, and its name, NUL
//! ended, each record padded to four bytes), then their values, as the
//! attributes hold them. The resource fork follows, usually empty.
//!
//! ```text
//!   0  magic 0x00051607, version 0x00020000, "Mac OS X        "
//!  24  2 entries: (9 Finder info, 50, length) (2 resource fork, offset, length)
//!  50  Finder info (32 bytes), 2 bytes of padding
//!  84  "ATTR", debug tag, total size, data start, data length,
//!      3 reserved words, flags, number of attributes
//! 120  records, then the values, then the resource fork
//! ```
//!
//! Read wherever a Mac writes them (`bibli_platform::filesystem`). A file
//! laid out otherwise (other entries, an older writer) is read but never
//! written: what we would write back could lose what we do not know.

use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::io::{self, Read, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use bibli_core::notes::raw::RawKey;
use bibli_core::notes::{Holds, Kept, Notes, NotesStore, Probe, Tag, Write, Wrote, parse_tags};
use parking_lot::Mutex;

use crate::xdg;

pub const PREFIX: &str = "._";

const MAGIC: u32 = 0x0005_1607;
const VERSION: u32 = 0x0002_0000;
const FILLER: &[u8; 16] = b"Mac OS X        ";
const FINDER_INFO: u32 = 9;
const RESOURCE_FORK: u32 = 2;
const FINDER_INFO_LEN: usize = 32;
/// Where the Finder info starts: after the header and its two entries.
const FINDER_INFO_AT: usize = 26 + 2 * 12;
/// Where the attribute header starts: after the Finder info and 2 bytes
/// of padding.
const ATTR_AT: usize = FINDER_INFO_AT + FINDER_INFO_LEN + 2;
const ATTR_MAGIC: u32 = 0x4154_5452;
const ATTR_HEADER_LEN: usize = 36;
/// The header and the records must fit in this (copyfile's limit).
const MAX_HEADER: usize = 65_536 + 18;
/// A record's name, its NUL included.
const MAX_NAME: usize = 128;
/// A sidecar larger than this (a resource fork of megabytes) is not read
/// whole to be written back.
const READ_AT_MOST: u64 = 64 << 20;

/// An attribute: its name as the Mac names it, its record's flags, its
/// value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attribute {
    pub name: String,
    flags: u16,
    pub value: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppleDouble {
    finder_info: [u8; FINDER_INFO_LEN],
    debug_tag: u32,
    flags: u16,
    pub attributes: Vec<Attribute>,
    /// `None` when it lay past what was read or where no entry said.
    resource_fork: Option<Vec<u8>>,
    /// Laid out as macOS writes it, and read whole: it can be written back.
    writable: bool,
}

/// Tests read `._` files on any volume.
#[cfg(test)]
pub(crate) static EVERYWHERE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Whether a Mac keeps the attributes of files in `dir` in `._` files.
pub fn applies(dir: &Path) -> bool {
    #[cfg(test)]
    if EVERYWHERE.load(std::sync::atomic::Ordering::Relaxed) {
        return true;
    }
    bibli_platform::filesystem::apple_double(dir)
}

/// The `._` file beside `path`.
pub fn sidecar(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?;
    let mut aside = OsString::from(PREFIX);
    aside.push(name);
    Some(path.with_file_name(aside))
}

/// Whether `name` is itself a `._` file, which has none of its own.
pub fn is_sidecar(name: &OsStr) -> bool {
    name.as_encoded_bytes().starts_with(PREFIX.as_bytes())
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn usize_at(b: &[u8], at: usize) -> Option<usize> {
    usize::try_from(u32_at(b, at)?).ok()
}

fn record_len(name_len: usize) -> usize {
    (11 + name_len + 3) & !3
}

impl AppleDouble {
    /// A sidecar's content; `None` when it is not an `AppleDouble` file.
    /// Attributes whose data lies past `bytes` are left out, and the
    /// result is then not written back.
    pub fn parse(bytes: &[u8]) -> Option<AppleDouble> {
        if u32_at(bytes, 0)? != MAGIC || u32_at(bytes, 4)? != VERSION {
            return None;
        }
        let count = usize::from(u16_at(bytes, 24)?);
        let mut entries = Vec::with_capacity(count);
        for i in 0..count {
            let at = 26 + i * 12;
            entries.push((
                u32_at(bytes, at)?,
                usize_at(bytes, at + 4)?,
                usize_at(bytes, at + 8)?,
            ));
        }
        let &(_, info_at, info_len) = entries.iter().find(|(id, _, _)| *id == FINDER_INFO)?;
        let info = bytes.get(info_at..info_at.checked_add(FINDER_INFO_LEN)?)?;
        let mut ad = AppleDouble {
            finder_info: info.try_into().ok()?,
            debug_tag: 0,
            flags: 0,
            attributes: Vec::new(),
            resource_fork: None,
            writable: false,
        };
        let mut complete = true;
        let attr_at = info_at + FINDER_INFO_LEN + 2;
        if info_len > FINDER_INFO_LEN && u32_at(bytes, attr_at) == Some(ATTR_MAGIC) {
            ad.debug_tag = u32_at(bytes, attr_at + 4)?;
            ad.flags = u16_at(bytes, attr_at + 32)?;
            let count = u16_at(bytes, attr_at + 34)?;
            let mut at = attr_at + ATTR_HEADER_LEN;
            for _ in 0..count {
                let offset = usize_at(bytes, at)?;
                let len = usize_at(bytes, at + 4)?;
                let flags = u16_at(bytes, at + 8)?;
                let name_len = usize::from(*bytes.get(at + 10)?);
                let name = bytes.get(at + 11..at + 11 + name_len)?;
                let name = name.strip_suffix(&[0]).unwrap_or(name);
                let name = String::from_utf8(name.to_vec()).ok()?;
                match bytes.get(offset..offset.checked_add(len)?) {
                    Some(value) => ad.attributes.push(Attribute {
                        name,
                        flags,
                        value: value.to_vec(),
                    }),
                    None => complete = false,
                }
                at += record_len(name_len);
            }
        }
        let fork = entries.iter().find(|(id, _, _)| *id == RESOURCE_FORK);
        if let Some(&(_, at, len)) = fork {
            ad.resource_fork = bytes.get(at..at.checked_add(len)?).map(<[u8]>::to_vec);
        }
        // What macOS writes: the Finder info first where it always is, the
        // resource fork last, nothing else.
        ad.writable = complete
            && entries.len() == 2
            && entries[0].0 == FINDER_INFO
            && info_at == FINDER_INFO_AT
            && entries[1].0 == RESOURCE_FORK
            && entries[1].1 >= info_at + info_len
            && ad.resource_fork.is_some()
            && (info_len == FINDER_INFO_LEN || attr_at == ATTR_AT);
        Some(ad)
    }

    /// A sidecar with nothing in it yet, laid out as macOS writes one.
    pub fn empty() -> AppleDouble {
        AppleDouble {
            finder_info: [0; FINDER_INFO_LEN],
            debug_tag: 0,
            flags: 0,
            attributes: Vec::new(),
            resource_fork: Some(Vec::new()),
            writable: true,
        }
    }

    /// The value of the attribute the Mac names `name`.
    pub fn get(&self, name: &str) -> Option<&[u8]> {
        self.attributes
            .iter()
            .find(|a| a.name == name)
            .map(|a| a.value.as_slice())
    }

    /// Set the attribute `name` to `value`, or remove it with `None`.
    /// Returns whether that changed anything.
    pub fn set(&mut self, name: &str, value: Option<Vec<u8>>) -> bool {
        let at = self.attributes.iter().position(|a| a.name == name);
        match (at, value) {
            (Some(i), Some(value)) if self.attributes[i].value == value => false,
            (Some(i), Some(value)) => {
                self.attributes[i].value = value;
                true
            }
            (Some(i), None) => {
                self.attributes.remove(i);
                true
            }
            (None, Some(value)) => {
                self.attributes.push(Attribute {
                    name: name.to_owned(),
                    flags: 0,
                    value,
                });
                true
            }
            (None, None) => false,
        }
    }

    /// The file as macOS writes it; `None` when it was not read as one,
    /// or its attributes do not fit the format.
    pub fn encode(&self) -> Option<Vec<u8>> {
        if !self.writable {
            return None;
        }
        let fork = self.resource_fork.as_deref().unwrap_or_default();
        let mut records: Vec<u8> = Vec::new();
        let mut data: Vec<u8> = Vec::new();
        let names: usize = self
            .attributes
            .iter()
            .map(|a| record_len(a.name.len() + 1))
            .sum();
        let data_start = ATTR_AT + ATTR_HEADER_LEN + names;
        if data_start > MAX_HEADER || self.attributes.len() > usize::from(u16::MAX) {
            return None;
        }
        for a in &self.attributes {
            let name_len = a.name.len() + 1;
            if name_len > MAX_NAME || a.name.contains('\0') {
                return None;
            }
            let offset = u32::try_from(data_start + data.len()).ok()?;
            records.extend(offset.to_be_bytes());
            records.extend(u32::try_from(a.value.len()).ok()?.to_be_bytes());
            records.extend(a.flags.to_be_bytes());
            records.push(u8::try_from(name_len).ok()?);
            records.extend(a.name.as_bytes());
            records.push(0);
            records.resize(records.len().next_multiple_of(4), 0);
            data.extend(&a.value);
        }
        let fork_at = u32::try_from(data_start + data.len()).ok()?;
        let info_len = fork_at - u32::try_from(FINDER_INFO_AT).ok()?;
        let mut out = Vec::with_capacity(data_start + data.len() + fork.len());
        out.extend(MAGIC.to_be_bytes());
        out.extend(VERSION.to_be_bytes());
        out.extend(FILLER);
        out.extend(2u16.to_be_bytes());
        for (id, at, len) in [
            (FINDER_INFO, u32::try_from(FINDER_INFO_AT).ok()?, info_len),
            (RESOURCE_FORK, fork_at, u32::try_from(fork.len()).ok()?),
        ] {
            out.extend(id.to_be_bytes());
            out.extend(at.to_be_bytes());
            out.extend(len.to_be_bytes());
        }
        out.extend(self.finder_info);
        out.extend([0, 0]);
        out.extend(ATTR_MAGIC.to_be_bytes());
        out.extend(self.debug_tag.to_be_bytes());
        out.extend(fork_at.to_be_bytes());
        out.extend(u32::try_from(data_start).ok()?.to_be_bytes());
        out.extend(u32::try_from(data.len()).ok()?.to_be_bytes());
        out.extend([0; 12]);
        out.extend(self.flags.to_be_bytes());
        out.extend(u16::try_from(self.attributes.len()).ok()?.to_be_bytes());
        out.extend(records);
        out.extend(data);
        out.extend(fork);
        Some(out)
    }
}

/// The sidecar of `path`, read; `None` when there is none, or it is not
/// one. Reads it whole (up to `READ_AT_MOST`), so it can be written back.
pub fn read(path: &Path) -> Option<AppleDouble> {
    let file = bibli_platform::file::open_regular(&sidecar(path)?).ok()?;
    let mut bytes = Vec::new();
    file.take(READ_AT_MOST).read_to_end(&mut bytes).ok()?;
    AppleDouble::parse(&bytes)
}

/// Write `ad` as the sidecar of `path`, replacing it whole: a Mac reading
/// it meanwhile sees the old one or the new one.
pub fn write(path: &Path, ad: &AppleDouble) -> io::Result<()> {
    let bytes = ad.encode().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::Unsupported,
            "its Mac attributes are kept in a form bibli does not write",
        )
    })?;
    let aside = sidecar(path).ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
    let dir = aside
        .parent()
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
    let mut staged = tempfile::Builder::new()
        .prefix(PREFIX)
        .suffix(".tmp")
        .tempfile_in(dir)?;
    // The staged file is private; the one it replaces keeps its mode.
    if let Ok(metadata) = std::fs::metadata(&aside) {
        std::fs::set_permissions(staged.path(), metadata.permissions())?;
    }
    staged.write_all(&bytes)?;
    staged.persist(&aside).map_err(|e| e.error)?;
    Ok(())
}

/// The names of the `._` files in the folders batches were in (their
/// files' names): a folder is read once for its listing, not once per
/// file, and again when it changes.
#[derive(Default)]
pub struct AsideNames {
    folders: Mutex<Vec<Seen>>,
}

struct Seen {
    dir: PathBuf,
    modified: Option<SystemTime>,
    at: Instant,
    names: Arc<HashSet<OsString>>,
}

/// Folders remembered at once: a search walks several in parallel.
const FOLDERS: usize = 16;
/// How long a folder's names are trusted while its time says it did not
/// change: FAT stores times to two seconds.
const FRESH: Duration = Duration::from_secs(5);

#[cfg(unix)]
fn os_from_bytes(bytes: &[u8]) -> OsString {
    use std::os::unix::ffi::OsStrExt;
    OsStr::from_bytes(bytes).to_owned()
}

#[cfg(not(unix))]
fn os_from_bytes(bytes: &[u8]) -> OsString {
    OsString::from(String::from_utf8_lossy(bytes).into_owned())
}

impl AsideNames {
    /// The files of `dir` that have a `._` file (maybe none).
    fn of(&self, dir: &Path) -> Arc<HashSet<OsString>> {
        let modified = std::fs::metadata(dir).ok().and_then(|m| m.modified().ok());
        let known = self
            .folders
            .lock()
            .iter()
            .find(|s| s.dir == dir && s.modified == modified && s.at.elapsed() < FRESH)
            .map(|s| Arc::clone(&s.names));
        if let Some(names) = known {
            return names;
        }
        let names: HashSet<OsString> = std::fs::read_dir(dir)
            .map(|entries| {
                entries
                    .filter_map(|e| {
                        let name = e.ok()?.file_name();
                        let file = name.as_encoded_bytes().strip_prefix(PREFIX.as_bytes())?;
                        Some(os_from_bytes(file))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let names = Arc::new(names);
        let mut folders = self.folders.lock();
        folders.retain(|s| s.dir != dir);
        if folders.len() >= FOLDERS {
            folders.remove(0);
        }
        folders.push(Seen {
            dir: dir.to_path_buf(),
            modified,
            at: Instant::now(),
            names: Arc::clone(&names),
        });
        names
    }
}

/// The attributes a Mac keeps outside its attribute list, named as
/// `rsync -X` names them from a Mac: the Finder info and the resource
/// fork.
const FINDER_INFO_ATTRIBUTE: &str = "user.com.apple.FinderInfo";
const RESOURCE_FORK_ATTRIBUTE: &str = "user.com.apple.ResourceFork";

impl AppleDouble {
    /// Every attribute it holds, named as on this system (`user.…`), the
    /// Finder info and the resource fork included when they say anything.
    pub fn as_attributes(&self) -> Vec<(String, Vec<u8>)> {
        let mut out: Vec<(String, Vec<u8>)> = self
            .attributes
            .iter()
            .map(|a| (format!("user.{}", a.name), a.value.clone()))
            .collect();
        if self.finder_info.iter().any(|b| *b != 0) {
            out.push((FINDER_INFO_ATTRIBUTE.to_owned(), self.finder_info.to_vec()));
        }
        if let Some(fork) = self.resource_fork.as_ref().filter(|f| !is_blank_fork(f)) {
            out.push((RESOURCE_FORK_ATTRIBUTE.to_owned(), fork.clone()));
        }
        out
    }

    /// Hold `attributes` (named as on this system) as well.
    pub fn take_attributes(&mut self, attributes: &[(String, Vec<u8>)]) {
        for (name, value) in attributes {
            match name.as_str() {
                FINDER_INFO_ATTRIBUTE => {
                    if let Ok(info) = <[u8; FINDER_INFO_LEN]>::try_from(value.as_slice()) {
                        self.finder_info = info;
                    }
                }
                RESOURCE_FORK_ATTRIBUTE => self.resource_fork = Some(value.clone()),
                _ => {
                    self.set(inside(name), Some(value.clone()));
                }
            }
        }
    }
}

/// The resource fork macOS writes when there is none: empty, or its
/// header with the words "This resource fork intentionally left blank".
fn is_blank_fork(fork: &[u8]) -> bool {
    fork.is_empty() || fork.windows(RF_BLANK.len()).any(|w| w == RF_BLANK) && fork.len() <= 512
}

const RF_BLANK: &[u8] = b"This resource fork intentionally left blank";

/// A file's `._` file as its companion (ADR-028): it goes where the file
/// goes, and it holds the file's attributes where its volume keeps none.
pub struct AsideCompanion;

impl bibli_core::companions::Companion for AsideCompanion {
    fn id(&self) -> &'static str {
        "appledouble"
    }
    fn of(&self, name: &OsStr) -> Vec<PathBuf> {
        let mut aside = OsString::from(PREFIX);
        aside.push(name);
        vec![PathBuf::from(aside)]
    }
    fn owner(&self, name: &OsStr) -> Option<OsString> {
        let file = name.as_encoded_bytes().strip_prefix(PREFIX.as_bytes())?;
        (!file.is_empty()).then(|| os_from_bytes(file))
    }
    fn holds_attributes(&self) -> bool {
        true
    }
    fn attributes(&self, path: &Path) -> Option<Vec<(String, Vec<u8>)>> {
        Some(read(path)?.as_attributes())
    }
    fn keep_attributes(&self, path: &Path, attributes: &[(String, Vec<u8>)]) -> io::Result<bool> {
        let mut ad = read(path).unwrap_or_else(AppleDouble::empty);
        ad.take_attributes(attributes);
        write(path, &ad)?;
        Ok(true)
    }
}

/// The desktop's attributes, as named inside a sidecar: a Mac has no
/// `user.` namespace, so `user.xdg.tags` is `xdg.tags` there, and a Mac
/// copying the file home makes it an attribute of that name, which
/// `rsync -X` brings back to Linux as `user.xdg.tags`.
fn inside(linux: &str) -> &str {
    linux.strip_prefix("user.").unwrap_or(linux)
}

/// What a sidecar says: its Finder tags (the Mac's own list, the one a
/// Mac edits) or else the desktop's, the Finder's comment or the
/// desktop's, the desktop's rating, where it was downloaded from.
pub fn notes_of(ad: &AppleDouble) -> Option<Notes> {
    let text = |linux: &str| {
        ad.get(inside(linux))
            .map(|v| String::from_utf8_lossy(v).trim().to_owned())
            .filter(|t| !t.is_empty())
    };
    let mut notes = crate::finder::notes_from(ad.attributes.iter().map(|a| {
        (
            a.name.as_str(),
            a.value.as_slice(),
            crate::finder::Framing::Plain,
        )
    }))
    .unwrap_or_default();
    if ad.get(crate::finder::TAGS).is_none()
        && let Some(tags) = text(xdg::TAGS)
    {
        notes.tags = parse_tags(&tags).into_iter().map(Tag::named).collect();
    }
    notes.comment = text(xdg::COMMENT).or(notes.comment);
    notes.rating = text(xdg::RATING).as_deref().and_then(xdg::parse_rating);
    notes.origin = text(xdg::ORIGIN).or(notes.origin);
    notes.referrer = text(xdg::REFERRER).or(notes.referrer);
    (!notes.is_empty()).then_some(notes)
}

/// A Mac's `._` files, on volumes where a Mac writes them: read, updated,
/// and created where the file's own attributes cannot keep its notes. It
/// holds the Mac's attributes and the desktop's alike.
#[derive(Default)]
pub struct AppleDoubleStore {
    aside: AsideNames,
}

impl NotesStore for AppleDoubleStore {
    fn id(&self) -> &'static str {
        "appledouble"
    }
    fn title(&self) -> &'static str {
        "A Mac's ._ files, beside files on volumes without attributes"
    }
    fn kept(&self) -> Kept {
        Kept::Beside
    }
    fn holds(&self) -> Holds {
        Holds {
            tags: true,
            tag_colors: true,
            rating: true,
            comment: true,
            origin: true,
            limits: "",
        }
    }
    fn default_write(&self) -> Write {
        Write::Fallback
    }
    fn applies(&self, dir: &Path) -> bool {
        applies(dir)
    }
    fn in_use(&self, dir: &Path) -> bool {
        !self.aside.of(dir).is_empty()
    }
    fn read(
        &self,
        dir: &Path,
        files: &[Probe<'_>],
        listed: Option<&bibli_core::metadata::Listed<'_>>,
    ) -> Vec<Option<Notes>> {
        // The folder's names from its listing when it knows them all, else
        // one read of the folder for its batches.
        let scanned = if listed.is_none() {
            self.aside.of(dir)
        } else {
            Arc::default()
        };
        let has = |name: &OsStr| {
            let mut aside = OsString::from(PREFIX);
            aside.push(name);
            match listed {
                Some(listed) => listed(&aside),
                None => scanned.contains(name),
            }
        };
        files
            .iter()
            .map(|f| {
                if is_sidecar(f.name) || !has(f.name) {
                    return None;
                }
                notes_of(&read(f.path)?)
            })
            .collect()
    }
    fn keys(&self, file: &Probe<'_>, create: bool) -> Vec<RawKey> {
        let Some(aside) = sidecar(file.path) else {
            return Vec::new();
        };
        if !create && !aside.exists() {
            return Vec::new();
        }
        vec![RawKey::Beside {
            path: aside.file_name().map(PathBuf::from).unwrap_or_default(),
        }]
    }
    fn write(&self, file: &Probe<'_>, notes: &Notes, create: bool) -> io::Result<Wrote> {
        let existing = read(file.path);
        let has_notes = !notes.tags.is_empty() || notes.rating.is_some() || notes.comment.is_some();
        let mut ad = match existing {
            Some(ad) => ad,
            None if create && has_notes => AppleDouble::empty(),
            None => return Ok(Wrote::NotHere),
        };
        let before = ad.clone();
        let theirs = ad
            .get(crate::finder::TAGS)
            .and_then(|v| crate::finder::parse(v, crate::finder::Framing::Plain))
            .unwrap_or_default();
        ad.set(
            crate::finder::TAGS,
            (!notes.tags.is_empty()).then(|| {
                crate::finder::encode(
                    &crate::finder::from_notes(notes, &theirs),
                    crate::finder::Framing::Plain,
                )
            }),
        );
        let [tags, rating, comment] = xdg::written(notes);
        for (linux, value) in [
            (xdg::TAGS, tags),
            (xdg::RATING, rating),
            (xdg::COMMENT, comment),
        ] {
            ad.set(inside(linux), value.map(String::into_bytes));
        }
        if ad.get(crate::finder::COMMENT).is_some() {
            ad.set(
                crate::finder::COMMENT,
                notes
                    .comment
                    .as_deref()
                    .map(|c| crate::finder::encode_comment(c, crate::finder::Framing::Plain)),
            );
        }
        if ad == before {
            return Ok(Wrote::Unchanged);
        }
        write(file.path, &ad)?;
        Ok(Wrote::Written)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A sidecar laid out as the macOS kernel leaves one on a USB stick,
    /// built here by hand from the format rather than by our encoder: 4 KiB,
    /// the attributes at the front, free space, then the empty resource
    /// fork at the end.
    pub(crate) fn as_a_mac_writes(attributes: &[(&str, &[u8])]) -> Vec<u8> {
        const SIZE: usize = 4096;
        const FORK_LEN: usize = 286;
        let fork_at = SIZE - FORK_LEN;
        let mut b = vec![0u8; SIZE];
        let put32 = |b: &mut Vec<u8>, at: usize, v: u32| {
            b[at..at + 4].copy_from_slice(&v.to_be_bytes());
        };
        put32(&mut b, 0, 0x0005_1607);
        put32(&mut b, 4, 0x0002_0000);
        b[8..24].copy_from_slice(b"Mac OS X        ");
        b[24..26].copy_from_slice(&2u16.to_be_bytes());
        put32(&mut b, 26, 9);
        put32(&mut b, 30, 50);
        put32(&mut b, 34, u32::try_from(fork_at - 50).unwrap());
        put32(&mut b, 38, 2);
        put32(&mut b, 42, u32::try_from(fork_at).unwrap());
        put32(&mut b, 46, u32::try_from(FORK_LEN).unwrap());
        // Finder info: a type and creator, as a Mac sets for some files.
        b[50..58].copy_from_slice(b"TEXTttxt");
        b[84..88].copy_from_slice(b"ATTR");
        put32(&mut b, 88, 0x00ab_cdef);
        put32(&mut b, 92, u32::try_from(fork_at).unwrap());
        let records: usize = attributes
            .iter()
            .map(|(n, _)| (11 + n.len() + 1 + 3) & !3)
            .sum();
        let mut data_at = 120 + records;
        put32(&mut b, 96, u32::try_from(data_at).unwrap());
        let data_len: usize = attributes.iter().map(|(_, v)| v.len()).sum();
        put32(&mut b, 100, u32::try_from(data_len).unwrap());
        b[118..120].copy_from_slice(&u16::try_from(attributes.len()).unwrap().to_be_bytes());
        let mut at = 120;
        for (name, value) in attributes {
            put32(&mut b, at, u32::try_from(data_at).unwrap());
            put32(&mut b, at + 4, u32::try_from(value.len()).unwrap());
            b[at + 10] = u8::try_from(name.len() + 1).unwrap();
            b[at + 11..at + 11 + name.len()].copy_from_slice(name.as_bytes());
            b[data_at..data_at + value.len()].copy_from_slice(value);
            at += (11 + name.len() + 1 + 3) & !3;
            data_at += value.len();
        }
        // The empty resource fork's tag, as the kernel writes it.
        b[fork_at + 16..fork_at + 16 + 44]
            .copy_from_slice(b"This resource fork intentionally left blank ");
        b
    }

    #[test]
    fn a_sidecar_a_mac_wrote_is_read_and_written_back_as_copyfile_would() {
        let quarantine = b"0083;66e0f1a2;Safari;".as_slice();
        let bytes = as_a_mac_writes(&[
            ("com.apple.quarantine", quarantine),
            ("com.apple.metadata:_kMDItemUserTags", b"bplist00..."),
        ]);
        let ad = AppleDouble::parse(&bytes).unwrap();
        assert_eq!(ad.get("com.apple.quarantine"), Some(quarantine));
        assert_eq!(
            ad.get("com.apple.metadata:_kMDItemUserTags"),
            Some(b"bplist00...".as_slice())
        );
        assert_eq!(&ad.finder_info[..8], b"TEXTttxt");
        assert_eq!(ad.resource_fork.as_ref().map(Vec::len), Some(286));

        let mut edited = ad.clone();
        assert!(edited.set("com.apple.metadata:_kMDItemUserTags", Some(b"new".to_vec())));
        assert!(!edited.set("com.apple.metadata:_kMDItemUserTags", Some(b"new".to_vec())));
        assert!(edited.set(
            "com.apple.metadata:kMDItemFinderComment",
            Some(b"c".to_vec())
        ));
        let written = edited.encode().unwrap();
        // Packed as copyfile packs it: records at 120, then the values,
        // then the resource fork, which the Finder info entry runs up to.
        assert_eq!(&written[84..88], b"ATTR");
        assert_eq!(u32_at(&written, 42), u32_at(&written, 92), "total size");
        assert_eq!(
            usize_at(&written, 42).unwrap() + 286,
            written.len(),
            "the resource fork is last"
        );
        let again = AppleDouble::parse(&written).unwrap();
        assert_eq!(again, edited, "every other attribute and the fork kept");
        assert_eq!(again.encode().unwrap(), written);
        // Nothing left but the Finder info and the fork: still valid.
        let mut bare = again;
        bare.attributes.clear();
        let bare = AppleDouble::parse(&bare.encode().unwrap()).unwrap();
        assert!(bare.attributes.is_empty() && bare.writable);
    }

    #[test]
    fn what_is_not_a_sidecar_macos_writes_is_read_but_never_written() {
        assert_eq!(AppleDouble::parse(b""), None);
        assert_eq!(AppleDouble::parse(&[0u8; 200]), None);
        let bytes = as_a_mac_writes(&[("a", b"1")]);
        // AppleSingle's magic, or a version 1 file.
        let mut single = bytes.clone();
        single[3] = 0x00;
        assert_eq!(AppleDouble::parse(&single), None);
        // Cut short after the records: the value of `a` is past the end.
        let cut = AppleDouble::parse(&bytes[..136]).unwrap();
        assert!(cut.attributes.is_empty());
        assert_eq!(cut.encode(), None, "it would lose `a` and the fork");
        // A third entry (a real name, as netatalk writes): not ours to write.
        let mut three = bytes.clone();
        three[25] = 3;
        let ad = AppleDouble::parse(&three).unwrap();
        assert_eq!(ad.get("a"), Some(b"1".as_slice()));
        assert_eq!(ad.encode(), None);
        // A record that says its name runs past the file.
        let mut long = bytes;
        long[130] = 255;
        assert!(AppleDouble::parse(&long[..200]).is_none());
    }

    #[test]
    fn a_sidecar_started_for_a_file_is_one_a_mac_reads_and_holds_every_note() {
        let base = dirs::home_dir().unwrap().join(".cache").join("bibli-tests");
        std::fs::create_dir_all(&base).unwrap();
        let tmp = tempfile::Builder::new()
            .prefix("appledouble-")
            .tempdir_in(base)
            .unwrap();
        let file = tmp.path().join("it's new.pdf");
        std::fs::write(&file, b"x").unwrap();
        let store = AppleDoubleStore::default();
        let probe = Probe {
            path: &file,
            name: file.file_name().unwrap(),
            attributes: &[],
            folder: false,
        };
        let notes = Notes {
            tags: vec![
                Tag {
                    name: "Rouge".into(),
                    color: Some("red".into()),
                },
                Tag::named("a, b"),
            ],
            rating: Some(7),
            comment: Some("note".into()),
            ..Notes::default()
        };
        assert_eq!(store.write(&probe, &notes, false).unwrap(), Wrote::NotHere);
        assert!(store.keys(&probe, false).is_empty());
        assert_eq!(store.write(&probe, &notes, true).unwrap(), Wrote::Written);
        let bytes = std::fs::read(sidecar(&file).unwrap()).unwrap();
        assert_eq!(&bytes[..4], &[0, 5, 22, 7]);
        let ad = AppleDouble::parse(&bytes).unwrap();
        assert!(ad.writable, "laid out as macOS writes it");
        let read = notes_of(&ad).unwrap();
        assert_eq!(
            read.tag_names(),
            ["Rouge", "a, b"],
            "the Finder's list holds a comma"
        );
        assert_eq!(read.tag("rouge").unwrap().color.as_deref(), Some("red"));
        assert_eq!(
            (read.rating, read.comment.as_deref()),
            (Some(7), Some("note"))
        );
        assert_eq!(ad.get("xdg.tags"), Some(b"Rouge".as_slice()));
        assert_eq!(
            store.write(&probe, &notes, false).unwrap(),
            Wrote::Unchanged
        );
    }

    #[test]
    fn a_sidecar_is_named_after_its_file() {
        assert_eq!(
            sidecar(Path::new("/stick/it's Été.txt")).unwrap(),
            Path::new("/stick/._it's Été.txt")
        );
        assert!(is_sidecar(OsStr::new("._x")));
        assert!(!is_sidecar(OsStr::new(".x")));
    }
}
