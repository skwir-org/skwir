//! A Palm database, the file every Mobipocket book is: a name, a type and
//! a creator, then records found by their offsets. The first record of a
//! book says how its text is compressed and, in a MOBI header, where the
//! rest is (pictures, indexes, a KF8 part); an EXTH header after it holds
//! what the book says of itself.
use std::borrow::Cow;

use skwir_core::{CancelToken, preview::FileSource};
use skwir_platform::inspection::limits;

/// A Palm database's header: its name, its type and creator, how many
/// records it has. The record list follows it.
const HEADER: usize = 78;

/// What a Palm database holds, from its type and creator (or, for a KFX
/// book, the container that is not one).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// `BOOKMOBI`: a Mobipocket book, MOBI 6 or KF8 or both.
    Mobi,
    /// `TEXtREAd`: a `PalmDOC` text.
    Text,
    /// Amazon's KFX, which is not a Palm database at all.
    Kfx,
    Other,
}

impl Kind {
    /// The kind of book whose first bytes are `head`.
    pub(crate) fn of(head: &[u8]) -> Self {
        if head.starts_with(b"CONT") || head.starts_with(b"\xeaDRMION\xee") {
            return Self::Kfx;
        }
        match head.get(60..68) {
            Some(b"BOOKMOBI") => Self::Mobi,
            Some(b"TEXtREAd") => Self::Text,
            _ => Self::Other,
        }
    }
}

/// A Palm database's records, by where each starts and ends.
pub(crate) struct Database {
    pub kind: Kind,
    /// Its name: a `PalmDOC` text's only title.
    pub name: String,
    /// Where each record starts, then where the last ends (the file's end).
    offsets: Vec<u64>,
}

impl Database {
    /// The records of the database in `file`: their offsets, which must
    /// rise and stay inside the file.
    pub(crate) fn open<S: FileSource + ?Sized>(
        file: &S,
        cancel: &CancelToken,
    ) -> anyhow::Result<Self> {
        let length = file.length()?;
        let head = file.read_range(0, HEADER, cancel)?;
        anyhow::ensure!(head.len() == HEADER, "This book is cut short");
        let kind = Kind::of(&head);
        let name = head[..32].split(|&b| b == 0).next().unwrap_or_default();
        let name = skwir_platform::inspection::Encoding::Windows1252
            .decode(name)
            .trim()
            .to_owned();
        let count = usize::from(u16::from_be_bytes([head[76], head[77]]));
        anyhow::ensure!(count > 0, "This book has no records");
        let list = file.read_range(HEADER as u64, count * 8, cancel)?;
        anyhow::ensure!(list.len() == count * 8, "This book is cut short");
        let mut offsets = Vec::with_capacity(count + 1);
        for entry in list.as_chunks::<8>().0 {
            let offset = u64::from(u32::from_be_bytes([entry[0], entry[1], entry[2], entry[3]]));
            anyhow::ensure!(
                offset >= (HEADER + count * 8) as u64
                    && offset <= length
                    && offsets.last().is_none_or(|&last| offset >= last),
                "This book's records are out of order"
            );
            offsets.push(offset);
        }
        offsets.push(length);
        Ok(Self {
            kind,
            name,
            offsets,
        })
    }

    /// How many records it has.
    pub(crate) fn len(&self) -> usize {
        self.offsets.len() - 1
    }

    /// Where record `index` starts, and its length.
    pub(crate) fn range(&self, index: usize) -> Option<(u64, usize)> {
        let start = *self.offsets.get(index)?;
        let end = *self.offsets.get(index + 1)?;
        Some((start, usize::try_from(end - start).ok()?))
    }

    /// Record `index`, read whole: at most `limits::BINARY_INPUT` bytes.
    pub(crate) fn record<'a, S: FileSource + ?Sized>(
        &self,
        file: &'a S,
        index: usize,
        cancel: &CancelToken,
    ) -> anyhow::Result<Cow<'a, [u8]>> {
        let (start, length) = self
            .range(index)
            .ok_or_else(|| anyhow::anyhow!("This book has no record {index}"))?;
        anyhow::ensure!(
            length <= limits::BINARY_INPUT,
            "A record of this book is too large to read"
        );
        let bytes = file.read_range(start, length, cancel)?;
        anyhow::ensure!(bytes.len() == length, "This book is cut short");
        Ok(bytes)
    }

    /// The first `limit` bytes of record `index`, or all of it if shorter.
    pub(crate) fn record_head<'a, S: FileSource + ?Sized>(
        &self,
        file: &'a S,
        index: usize,
        limit: usize,
        cancel: &CancelToken,
    ) -> anyhow::Result<Cow<'a, [u8]>> {
        let (start, length) = self
            .range(index)
            .ok_or_else(|| anyhow::anyhow!("This book has no record {index}"))?;
        Ok(file.read_range(start, length.min(limit), cancel)?)
    }
}

/// How a book's text records are compressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Compression {
    None,
    /// `PalmDOC`'s LZ77.
    PalmDoc,
    /// Mobipocket's Huffman coding with a dictionary (HUFF and CDIC records).
    HuffCdic,
}

/// A book's first record: how its text is stored and, for a Mobipocket
/// book, its MOBI header.
#[derive(Clone, Debug)]
pub(crate) struct Header {
    pub compression: Compression,
    /// The length of the text once decompressed.
    pub text_length: u32,
    /// How many records after this one hold the text.
    pub text_records: u16,
    /// Whether the text is encrypted (DRM).
    pub encrypted: bool,
    pub mobi: Option<Mobi>,
}

/// What a MOBI header says of the book, in the header's own record
/// numbers (relative to the record that holds it).
#[derive(Clone, Debug, Default)]
pub(crate) struct Mobi {
    /// 6 for MOBI 6, 8 for KF8.
    pub version: u32,
    /// UTF-8 (65001) or Windows-1252 (1252).
    pub utf8: bool,
    /// The book's full name.
    pub title: Option<String>,
    /// The first picture's record.
    pub first_image: Option<u32>,
    /// The HUFF record and how many HUFF and CDIC records there are.
    pub huffman: Option<(u32, u32)>,
    /// What trails each text record (`ExtraData`).
    pub extra_flags: u16,
    /// The table of contents' index (INDX records).
    pub ncx: Option<u32>,
    /// KF8: the flow table, the skeleton and fragment indexes.
    pub fdst: Option<u32>,
    pub skeleton: Option<u32>,
    pub fragment: Option<u32>,
    pub exth: Vec<(u32, Vec<u8>)>,
}

/// A record number a header gives: none when it says `0xFFFFFFFF`.
fn index(value: u32) -> Option<u32> {
    (value != u32::MAX).then_some(value)
}

/// Bytes of an EXTH header read at most: more than any book's metadata.
const EXTH: usize = 1024 * 1024;
/// EXTH records read at most.
const EXTH_RECORDS: usize = 4096;

impl Header {
    /// The header in `record`, a book's first record.
    pub(crate) fn parse(record: &[u8]) -> anyhow::Result<Self> {
        let u16_at = |at: usize| {
            record
                .get(at..at + 2)
                .map(|b| u16::from_be_bytes([b[0], b[1]]))
        };
        let u32_at = |at: usize| {
            record
                .get(at..at + 4)
                .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
        };
        let short = || anyhow::anyhow!("This book's first record is cut short");
        let compression = match u16_at(0).ok_or_else(short)? {
            1 => Compression::None,
            2 => Compression::PalmDoc,
            17480 => Compression::HuffCdic,
            other => anyhow::bail!("This book's text is compressed in a way not read ({other})"),
        };
        let text_length = u32_at(4).ok_or_else(short)?;
        let text_records = u16_at(8).ok_or_else(short)?;
        let encrypted = u16_at(12).ok_or_else(short)? != 0;
        let mut header = Self {
            compression,
            text_length,
            text_records,
            encrypted,
            mobi: None,
        };
        if record.get(16..20) != Some(b"MOBI") {
            return Ok(header);
        }
        let length = u32_at(20).ok_or_else(short)? as usize;
        // A field is read only when the header is long enough to hold it.
        let field = |at: usize| (at + 4 <= 16 + length).then(|| u32_at(at)).flatten();
        let version = field(36).unwrap_or(0);
        let title = field(84).zip(field(88)).and_then(|(at, len)| {
            let bytes = record.get(at as usize..(at as usize).checked_add(len as usize)?)?;
            Some(String::from_utf8_lossy(bytes).into_owned())
        });
        let mut mobi = Mobi {
            version,
            utf8: field(28) == Some(65001),
            title: title.filter(|title| !title.trim().is_empty()),
            first_image: field(108).and_then(index),
            huffman: field(112).and_then(index).zip(field(116)),
            extra_flags: if 16 + length >= 0xF4 {
                u16_at(0xF2).unwrap_or(0)
            } else {
                0
            },
            ncx: field(0xF4).and_then(index),
            ..Mobi::default()
        };
        if version >= 8 {
            mobi.fdst = field(0xC0).and_then(index);
            mobi.fragment = field(0xF8).and_then(index);
            mobi.skeleton = field(0xFC).and_then(index);
        }
        if field(128).is_some_and(|flags| flags & 0x40 != 0) {
            mobi.exth = exth(record.get(16 + length..).unwrap_or_default());
        }
        header.mobi = Some(mobi);
        Ok(header)
    }

    /// The EXTH record `kind`'s first value, as text.
    pub(crate) fn text(&self, kind: u32) -> Option<String> {
        let value = self.values(kind).next()?;
        let text = String::from_utf8_lossy(value).trim().to_owned();
        (!text.is_empty()).then_some(text)
    }

    /// Every EXTH record `kind`'s value, as text, joined.
    pub(crate) fn texts(&self, kind: u32) -> Option<String> {
        let all: Vec<String> = self
            .values(kind)
            .map(|value| String::from_utf8_lossy(value).trim().to_owned())
            .filter(|text| !text.is_empty())
            .collect();
        (!all.is_empty()).then(|| all.join(", "))
    }

    /// The EXTH record `kind`'s first value, as a number.
    pub(crate) fn number(&self, kind: u32) -> Option<u32> {
        let value = self.values(kind).next()?;
        let bytes: [u8; 4] = value.try_into().ok()?;
        Some(u32::from_be_bytes(bytes))
    }

    fn values(&self, kind: u32) -> impl Iterator<Item = &[u8]> {
        self.mobi
            .iter()
            .flat_map(|mobi| mobi.exth.iter())
            .filter(move |(k, _)| *k == kind)
            .map(|(_, value)| value.as_slice())
    }
}

/// The records of an EXTH header at the start of `bytes`.
fn exth(bytes: &[u8]) -> Vec<(u32, Vec<u8>)> {
    let u32_at = |at: usize| {
        bytes
            .get(at..at + 4)
            .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    };
    let mut records = Vec::new();
    if bytes.get(..4) != Some(b"EXTH") {
        return records;
    }
    let length = (u32_at(4).unwrap_or(0) as usize).min(bytes.len()).min(EXTH);
    let count = u32_at(8).unwrap_or(0) as usize;
    let mut at = 12;
    for _ in 0..count.min(EXTH_RECORDS) {
        let (Some(kind), Some(size)) = (u32_at(at), u32_at(at + 4)) else {
            break;
        };
        let size = size as usize;
        let Some(value) = (size >= 8)
            .then(|| bytes.get(at + 8..at + size))
            .flatten()
            .filter(|_| at + size <= length)
        else {
            break;
        };
        records.push((kind, value.to_vec()));
        at += size;
    }
    records
}

/// The EXTH records a book's metadata is read from.
pub(crate) mod exth {
    pub const AUTHOR: u32 = 100;
    pub const PUBLISHER: u32 = 101;
    pub const ISBN: u32 = 104;
    pub const SUBJECT: u32 = 105;
    pub const DATE: u32 = 106;
    /// The KF8 part's first record, in a file holding MOBI 6 and KF8.
    pub const KF8_BOUNDARY: u32 = 121;
    /// The cover, as a picture after the first.
    pub const COVER: u32 = 201;
    pub const TITLE: u32 = 503;
    pub const LANGUAGE: u32 = 524;
}
