//! Where a table's pages begin in its file: the byte at which the record
//! after so many begins, learned as pages are read. A page turned to is
//! read from the nearest start before it, where each was read from the
//! file's start, every page before it read again. A file's starts, once
//! `PER_FILE`, lose every other one: they stay spread over all of the file
//! read, twice as far apart, so a page past the millionth record is read
//! from a start a few pages before it, never from the last one that fit.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::SystemTime;

use skwir_core::preview::Held;

/// A version of a file: its path, length and modification time, as a
/// preview read it (`Held::version`, ADR-101): the path is asked nothing.
pub(crate) type Version = (PathBuf, u64, Option<SystemTime>);

/// The files' page starts: `(records before, byte)`, by file, the last few;
/// each file's sorted by records before.
static STARTS: Mutex<Vec<(Version, Vec<(u64, u64)>)>> = Mutex::new(Vec::new());

/// Files whose starts are kept.
const FILES: usize = 8;
/// Starts kept for one file.
const PER_FILE: usize = 4096;

/// The version of the file `held` holds.
pub(crate) fn version(held: &Held) -> Version {
    let (len, modified) = held.version();
    (held.path().to_path_buf(), len, modified)
}

/// The start known nearest before `records` records of the version of
/// the file `held` holds: how many records come before it, and its byte.
/// `(0, 0)` when none is known.
pub(crate) fn nearest(held: &Held, records: u64) -> (u64, u64) {
    let version = version(held);
    let starts = STARTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    starts
        .iter()
        .find(|(v, _)| *v == version)
        .and_then(|(_, known)| nearest_in(known, records))
        .unwrap_or((0, 0))
}

/// The record after `records` begins at `byte` in the version of the file
/// `held` holds.
pub(crate) fn learn(held: &Held, records: u64, byte: u64) {
    let version = version(held);
    let mut starts = STARTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let known = if let Some(at) = starts.iter().position(|(v, _)| *v == version) {
        &mut starts[at].1
    } else {
        starts.retain(|(v, _)| v.0 != version.0);
        if starts.len() >= FILES {
            starts.remove(0);
        }
        starts.push((version, Vec::new()));
        &mut starts.last_mut().expect("just pushed").1
    };
    learn_into(known, records, byte);
}

/// The start of `known` (sorted by records before) nearest before
/// `records` records.
fn nearest_in(known: &[(u64, u64)], records: u64) -> Option<(u64, u64)> {
    let after = known.partition_point(|(before, _)| *before <= records);
    after.checked_sub(1).map(|at| known[at])
}

/// `known` (sorted by records before) told that the record after
/// `records` begins at `byte`: kept in its place, every other start let go
/// first when `PER_FILE` are known.
fn learn_into(known: &mut Vec<(u64, u64)>, records: u64, byte: u64) {
    let place = |known: &[(u64, u64)]| known.partition_point(|(before, _)| *before < records);
    let at = place(known);
    if known.get(at).is_some_and(|(before, _)| *before == records) {
        return;
    }
    let at = if known.len() >= PER_FILE {
        let mut index = 0;
        known.retain(|_| {
            index += 1;
            index % 2 == 1
        });
        place(known)
    } else {
        at
    };
    known.insert(at, (records, byte));
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: u64 = 500;

    #[test]
    fn starts_past_what_fits_thin_out_and_keep_the_file_covered() {
        let mut known = Vec::new();
        for page in 1..=10_000 {
            learn_into(&mut known, page * PAGE, page * PAGE * 10);
        }
        assert!(known.len() <= PER_FILE, "{} starts kept", known.len());
        assert!(
            known.windows(2).all(|w| w[0].0 < w[1].0),
            "sorted, once each"
        );
        // Near the end, a start a few pages before: not the last that fit.
        let (before, byte) = nearest_in(&known, 9_999 * PAGE).unwrap();
        assert!(before >= 9_996 * PAGE, "read from {before}");
        assert_eq!(byte, before * 10);
        // Everywhere else, a start a few pages before: three halvings,
        // eight pages apart at most.
        assert_eq!(known[0].0, PAGE);
        assert!(known.windows(2).all(|w| w[1].0 - w[0].0 <= 8 * PAGE));
    }

    #[test]
    fn a_start_is_kept_once_in_its_place() {
        let mut known = Vec::new();
        for page in [5, 1, 3, 3, 9, 1] {
            learn_into(&mut known, page * PAGE, page);
        }
        assert_eq!(
            known,
            [(PAGE, 1), (3 * PAGE, 3), (5 * PAGE, 5), (9 * PAGE, 9)]
        );
        assert_eq!(nearest_in(&known, 4 * PAGE), Some((3 * PAGE, 3)));
        assert_eq!(nearest_in(&known, 3 * PAGE), Some((3 * PAGE, 3)));
        assert_eq!(nearest_in(&known, PAGE - 1), None);
    }
}
