//! Where a file that a commit has and its parent lacks came from: a file
//! the commit deleted, the same or at least half alike, as git's rename
//! detection (`diffcore-rename`) finds it for `git log --follow`.
use crate::tree::{self, Entry};
use crate::{Deadline, Result};
use gix::bstr::BString;
use std::collections::HashMap;

/// git's score of two files alike, whole, and the least a rename has
/// (`-M50%`, its default).
pub(crate) const MAX_SCORE: u64 = 60_000;
const MINIMUM: u64 = MAX_SCORE / 2;
/// Files deleted compared by their bytes at most (git's `renameLimit`).
const CANDIDATES: usize = 1000;
/// A file larger is not compared by its bytes, only by its id.
const LARGEST: u64 = 8 << 20;
/// Where a NUL says a file is binary, as git looks.
const FIRST_FEW: usize = 8000;
/// The modulus of git's span hashes, and a span's bytes at most.
const HASHBASE: u32 = 107_927;
const SPAN: u64 = 64;

/// A file renamed: where it was, what it was there, and how alike.
pub(crate) struct Source {
    pub path: BString,
    pub entry: Entry,
    /// Out of `MAX_SCORE`.
    pub score: u64,
}

impl Source {
    /// How alike, in percent, as git writes it (`similarity index`).
    pub fn percent(&self) -> u64 {
        self.score * 100 / MAX_SCORE
    }
}

/// Where file `target`, named `name` in tree `new_root`, came from: a file
/// tree `old_root` has and `new_root` lacks, the same one first (by its id,
/// of the same base name first), else the most alike of the regular files
/// at least half alike (of two as alike, the one of the same base name).
/// The trees are compared where they differ alone (`tree::changed`); a
/// file whose size is not within half of the target's cannot be half alike
/// and is not read, nor one larger than 8 MiB, nor more than 1000. None
/// when nothing qualifies, or the deadline passed.
pub(crate) fn source(
    repo: &gix::Repository,
    old_root: gix::ObjectId,
    new_root: gix::ObjectId,
    name: &[u8],
    target: Entry,
    deadline: &Deadline,
) -> Result<Option<Source>> {
    if !target.is_file() {
        return Ok(None);
    }
    let mut changes = Vec::new();
    tree::changed(
        repo,
        &BString::default(),
        Some(Entry::tree(old_root)),
        Some(Entry::tree(new_root)),
        &mut changes,
    )?;
    // Deleted, and of the target's kind: a link is renamed to a link.
    let deleted: Vec<(BString, Entry)> = changes
        .into_iter()
        .filter_map(|change| match (change.old, change.new) {
            (Some(old), None) if old.is_file() && old.is_regular() == target.is_regular() => {
                Some((change.path, old))
            }
            _ => None,
        })
        .collect();
    let base = |path: &[u8]| -> Vec<u8> {
        path.rsplit(|byte| *byte == b'/')
            .next()
            .unwrap_or(path)
            .to_vec()
    };
    let own_base = base(name);
    let same_base = |path: &BString| base(path) == own_base;
    let same: Vec<&(BString, Entry)> = deleted
        .iter()
        .filter(|(_, entry)| entry.id == target.id)
        .collect();
    if let Some((path, entry)) = same
        .iter()
        .find(|(path, _)| same_base(path))
        .or_else(|| same.first())
    {
        return Ok(Some(Source {
            path: path.clone(),
            entry: *entry,
            score: MAX_SCORE,
        }));
    }
    // Alike by its bytes: regular files alone, as git compares no link's.
    if !target.is_regular() {
        return Ok(None);
    }
    let size = repo.find_header(target.id)?.size();
    if size == 0 || size > LARGEST {
        return Ok(None);
    }
    let mut own: Option<Spans> = None;
    let mut best: Option<(u64, bool, usize)> = None;
    let mut compared = 0;
    for (at, (path, entry)) in deleted.iter().enumerate() {
        if compared >= CANDIDATES || deadline.passed() {
            break;
        }
        let other = repo.find_header(entry.id)?.size();
        if other == 0 || other > LARGEST {
            continue;
        }
        let (small, large) = (size.min(other), size.max(other));
        // What two files share is at most the smaller: under half of the
        // larger, they cannot be half alike (git's estimate, at 50 %).
        if small * 2 < large {
            continue;
        }
        compared += 1;
        if own.is_none() {
            own = Some(Spans::of(&repo.find_blob(target.id)?.data));
        }
        let Some(own) = &own else {
            continue;
        };
        let theirs = Spans::of(&repo.find_blob(entry.id)?.data);
        let score = theirs.common(own) * MAX_SCORE / large;
        if score < MINIMUM {
            continue;
        }
        let key = (score, same_base(path));
        if best.is_none_or(|(score, same, _)| key > (score, same)) {
            best = Some((key.0, key.1, at));
        }
    }
    Ok(best.map(|(score, _, at)| {
        let (path, entry) = &deleted[at];
        Source {
            path: path.clone(),
            entry: *entry,
            score,
        }
    }))
}

/// A file's bytes as git's similarity estimate counts them: cut in spans
/// that end at a line's end or at 64 bytes, each span's hash with the bytes
/// of the spans of that hash (`diffcore-delta.c`). A carriage return before
/// a line feed is not counted in a text (a file without a NUL near its
/// start).
struct Spans(HashMap<u32, u64>);

impl Spans {
    fn of(data: &[u8]) -> Self {
        let text = !data[..data.len().min(FIRST_FEW)].contains(&0);
        let mut spans: HashMap<u32, u64> = HashMap::new();
        let (mut first, mut second, mut length) = (0u32, 0u32, 0u64);
        let mut end = |first: u32, second: u32, length: u64| {
            let hash = first.wrapping_add(second.wrapping_mul(0x61)) % HASHBASE;
            *spans.entry(hash).or_default() += length;
        };
        for (at, &byte) in data.iter().enumerate() {
            if text && byte == b'\r' && data.get(at + 1) == Some(&b'\n') {
                continue;
            }
            let before = first;
            first = (first << 7) ^ (second >> 25);
            second = (second << 7) ^ (before >> 25);
            first = first.wrapping_add(u32::from(byte));
            length += 1;
            if length < SPAN && byte != b'\n' {
                continue;
            }
            end(first, second, length);
            (first, second, length) = (0, 0, 0);
        }
        if length > 0 {
            end(first, second, length);
        }
        Spans(spans)
    }

    /// The bytes these spans share with `other`'s: of each hash, the
    /// fewer.
    fn common(&self, other: &Spans) -> u64 {
        self.0
            .iter()
            .map(|(hash, bytes)| other.0.get(hash).map_or(0, |theirs| (*bytes).min(*theirs)))
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_SCORE, Spans};

    /// Two texts share the bytes of their lines alike; a carriage return
    /// before a line's end is not counted, a long line is cut every 64
    /// bytes.
    #[test]
    fn texts_share_the_bytes_of_their_lines_alike() {
        let a = Spans::of(b"one\ntwo\nthree\n");
        let b = Spans::of(b"one\r\ntwo\r\nfour\r\n");
        assert_eq!(a.common(&b), "one\ntwo\n".len() as u64);
        assert_eq!(a.common(&a), 14);
        let long = vec![b'x'; 200];
        let spans = Spans::of(&long);
        assert_eq!(spans.0.values().sum::<u64>(), 200);
        assert_eq!(spans.0.len(), 2, "three spans of 64 alike, one of 8");
        assert_eq!(MAX_SCORE / 2, 30_000);
    }
}
