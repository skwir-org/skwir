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

use std::ffi::{OsStr, OsString};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

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
    fn a_sidecar_is_named_after_its_file() {
        assert_eq!(
            sidecar(Path::new("/stick/it's Été.txt")).unwrap(),
            Path::new("/stick/._it's Été.txt")
        );
        assert!(is_sidecar(OsStr::new("._x")));
        assert!(!is_sidecar(OsStr::new(".x")));
    }
}
