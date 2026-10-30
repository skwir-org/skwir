//! A Palm database, the file every Mobipocket book is: a name, a type and
//! a creator, then records found by their offsets. The first record of a
//! book says how its text is compressed and, in a MOBI header, where the
//! rest is (pictures, indexes, a KF8 part); an EXTH header after it holds
//! what the book says of itself.
use std::borrow::Cow;

use skwir_core::{CancelToken, preview::FileSource};

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
        Ok(Self { kind, offsets })
    }

    /// Where record `index` starts, and its length.
    pub(crate) fn range(&self, index: usize) -> Option<(u64, usize)> {
        let start = *self.offsets.get(index)?;
        let end = *self.offsets.get(index + 1)?;
        Some((start, usize::try_from(end - start).ok()?))
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
    /// Whether the text is encrypted (DRM).
    pub encrypted: bool,
    pub mobi: Option<Mobi>,
}

/// What a MOBI header says of the book.
#[derive(Clone, Debug, Default)]
pub(crate) struct Mobi {
    /// 6 for MOBI 6, 8 for KF8.
    pub version: u32,
}

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
        let encrypted = u16_at(12).ok_or_else(short)? != 0;
        let mut header = Self {
            compression,
            encrypted,
            mobi: None,
        };
        if record.get(16..20) != Some(b"MOBI") {
            return Ok(header);
        }
        let length = u32_at(20).ok_or_else(short)? as usize;
        // A field is read only when the header is long enough to hold it.
        let field = |at: usize| (at + 4 <= 16 + length).then(|| u32_at(at)).flatten();
        let mobi = Mobi {
            version: field(36).unwrap_or(0),
        };
        header.mobi = Some(mobi);
        Ok(header)
    }
}
