//! A Mobipocket index: an INDX record saying how its entries are tagged
//! (TAGX), the INDX records of its entries, and the CNCX records holding
//! the labels entries point at. An entry is a short name and tagged lists
//! of numbers: a contents entry's position, depth and label; a KF8
//! skeleton's or fragment's place in the text.
use std::collections::HashMap;

use skwir_core::{CancelToken, preview::FileSource};
use skwir_platform::inspection::limits;

use super::palmdb::Database;

/// Entries read at most, in all of an index's records.
const ENTRIES: usize = 1 << 16;

/// One entry: its name, and its tags' numbers.
#[derive(Debug, Default)]
pub(crate) struct Entry {
    pub name: Vec<u8>,
    pub tags: HashMap<u8, Vec<u32>>,
}

impl Entry {
    /// The first number of `tag`.
    pub(crate) fn first(&self, tag: u8) -> Option<u32> {
        self.tags.get(&tag)?.first().copied()
    }
    /// The numbers of `tag`.
    pub(crate) fn all(&self, tag: u8) -> &[u32] {
        self.tags.get(&tag).map_or(&[], Vec::as_slice)
    }
}

/// An index's entries, in order, and the labels they point at.
#[derive(Debug, Default)]
pub(crate) struct Index {
    pub entries: Vec<Entry>,
    labels: HashMap<u32, String>,
}

impl Index {
    /// A table of contents' entries in reading order, each with its depth:
    /// the entries without a parent (tag 21), each followed by its
    /// children (tags 22 and 23 name the first and the last), as the book
    /// may list every level before the next. An entry no parent leads to
    /// comes at the end, at the depth it says (tag 4).
    pub(crate) fn in_order(&self) -> Vec<(&Entry, usize)> {
        fn walk<'a>(
            index: &'a Index,
            at: usize,
            depth: usize,
            seen: &mut [bool],
            out: &mut Vec<(&'a Entry, usize)>,
        ) {
            let Some(entry) = index.entries.get(at) else {
                return;
            };
            if std::mem::replace(&mut seen[at], true) {
                return;
            }
            out.push((entry, depth.min(limits::NESTING)));
            if let (Some(first), Some(last)) = (entry.first(22), entry.first(23))
                && depth < limits::NESTING
            {
                for child in first as usize..=(last as usize).min(index.entries.len()) {
                    walk(index, child, depth + 1, seen, out);
                }
            }
        }
        let mut seen = vec![false; self.entries.len()];
        let mut out = Vec::with_capacity(self.entries.len());
        for (at, entry) in self.entries.iter().enumerate() {
            if entry.first(21).is_none() {
                walk(self, at, 0, &mut seen, &mut out);
            }
        }
        for (at, entry) in self.entries.iter().enumerate() {
            if !seen[at] {
                out.push((
                    entry,
                    (entry.first(4).unwrap_or(0) as usize).min(limits::NESTING),
                ));
            }
        }
        out
    }

    /// The label at `offset` in the index's CNCX records.
    pub(crate) fn label(&self, offset: u32) -> Option<&str> {
        self.labels.get(&offset).map(String::as_str)
    }
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    let b = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    let b = bytes.get(at..at.checked_add(2)?)?;
    Some(u16::from_be_bytes([b[0], b[1]]))
}

/// A number written in 7-bit groups, high first, its last byte's top bit
/// set: the number and the bytes it took.
fn forward(bytes: &[u8], at: usize) -> Option<(u32, usize)> {
    let mut value = 0u32;
    for (k, &byte) in bytes.get(at..)?.iter().take(5).enumerate() {
        value = (value << 7) | u32::from(byte & 0x7F);
        if byte & 0x80 != 0 {
            return Some((value, k + 1));
        }
    }
    None
}

/// The index whose first record is `first`, its labels decoded as UTF-8
/// (else Windows-1252, a MOBI 6 book's other encoding).
pub(crate) fn read<S: FileSource + ?Sized>(
    database: &Database,
    file: &S,
    first: usize,
    utf8: bool,
    cancel: &CancelToken,
) -> anyhow::Result<Index> {
    let damaged = || anyhow::anyhow!("This book's index is damaged");
    let main = database.record(file, first, cancel)?;
    anyhow::ensure!(main.get(..4) == Some(b"INDX"), damaged());
    let header = u32_at(&main, 4).ok_or_else(damaged)? as usize;
    let records = u32_at(&main, 24).ok_or_else(damaged)? as usize;
    let labels_records = u32_at(&main, 52).unwrap_or(0) as usize;
    let tags = tag_table(&main, header).ok_or_else(damaged)?;
    let mut index = Index::default();
    for j in 0..labels_records.min(16) {
        let Ok(record) = database.record(file, first + records + 1 + j, cancel) else {
            break;
        };
        labels(&record, (j as u32) << 16, utf8, &mut index.labels);
    }
    for i in 1..=records.min(database.len()) {
        skwir_platform::inspection::check(cancel)?;
        let record = database.record(file, first + i, cancel)?;
        anyhow::ensure!(record.get(..4) == Some(b"INDX"), damaged());
        let start = u32_at(&record, 20).ok_or_else(damaged)? as usize;
        let count = u32_at(&record, 24).ok_or_else(damaged)? as usize;
        anyhow::ensure!(record.get(start..start + 4) == Some(b"IDXT"), damaged());
        let mut positions = Vec::with_capacity(count);
        for j in 0..count {
            positions.push(usize::from(
                u16_at(&record, start + 4 + 2 * j).ok_or_else(damaged)?,
            ));
        }
        for &at in &positions[..count] {
            let length = usize::from(*record.get(at).ok_or_else(damaged)?);
            let name = record
                .get(at + 1..at + 1 + length)
                .ok_or_else(damaged)?
                .to_vec();
            let tags = entry_tags(&tags, &record, at + 1 + length).ok_or_else(damaged)?;
            index.entries.push(Entry { name, tags });
            anyhow::ensure!(index.entries.len() <= ENTRIES, damaged());
        }
    }
    Ok(index)
}

/// A tag of the TAGX table: its number, how many values each of its
/// entries has, the mask of the control byte saying how many, and whether
/// it ends a control byte's tags.
struct Tag {
    number: u8,
    values: u8,
    mask: u8,
    end: bool,
}

/// The TAGX table at `at` in the index's first record, and how many
/// control bytes each entry starts with.
fn tag_table(record: &[u8], at: usize) -> Option<(usize, Vec<Tag>)> {
    if record.get(at..at + 4)? != b"TAGX" {
        return Some((0, Vec::new()));
    }
    let length = u32_at(record, at + 4)? as usize;
    let control = u32_at(record, at + 8)? as usize;
    let mut tags = Vec::new();
    let mut k = 12;
    while k + 4 <= length {
        let entry = record.get(at + k..at + k + 4)?;
        tags.push(Tag {
            number: entry[0],
            values: entry[1],
            mask: entry[2],
            end: entry[3] & 1 != 0,
        });
        k += 4;
    }
    Some((control, tags))
}

/// An entry's tags, read from its control bytes at `at` and the numbers
/// after them.
fn entry_tags(
    (control, table): &(usize, Vec<Tag>),
    record: &[u8],
    at: usize,
) -> Option<HashMap<u8, Vec<u32>>> {
    // What each tag of the entry holds: a count of values, or a number of
    // bytes of them.
    enum Held {
        Values(u32),
        Bytes(u32),
    }
    let mut data = at + control;
    let mut held = Vec::new();
    let mut byte = 0;
    for tag in table {
        if tag.end {
            byte += 1;
            continue;
        }
        if tag.mask == 0 {
            continue;
        }
        let value = record.get(at + byte)? & tag.mask;
        if value == 0 {
            continue;
        }
        if value == tag.mask && tag.mask.count_ones() > 1 {
            let (bytes, used) = forward(record, data)?;
            data += used;
            held.push((tag, Held::Bytes(bytes)));
        } else {
            held.push((
                tag,
                Held::Values(u32::from(value >> tag.mask.trailing_zeros())),
            ));
        }
    }
    let mut tags = HashMap::new();
    for (tag, holds) in held {
        let mut values = Vec::new();
        match holds {
            Held::Values(count) => {
                for _ in 0..count.saturating_mul(u32::from(tag.values)).min(256) {
                    let (value, used) = forward(record, data)?;
                    data += used;
                    values.push(value);
                }
            }
            Held::Bytes(bytes) => {
                let stop = data + bytes as usize;
                while data < stop && values.len() < 256 {
                    let (value, used) = forward(record, data)?;
                    data += used;
                    values.push(value);
                }
            }
        }
        tags.insert(tag.number, values);
    }
    Some(tags)
}

/// The labels of a CNCX record, by their offset (plus `base`, the
/// record's place among the index's CNCX records).
fn labels(record: &[u8], base: u32, utf8: bool, out: &mut HashMap<u32, String>) {
    let mut at = 0;
    while at < record.len() && record[at] != 0 {
        let Some((length, used)) = forward(record, at) else {
            break;
        };
        let start = at + used;
        let Some(bytes) = record.get(start..start + length as usize) else {
            break;
        };
        let label = if utf8 {
            String::from_utf8_lossy(bytes).into_owned()
        } else {
            skwir_platform::inspection::Encoding::Windows1252
                .decode(bytes)
                .into_owned()
        };
        out.insert(base + at as u32, label);
        at = start + length as usize;
    }
}
