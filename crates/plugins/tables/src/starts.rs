//! Where a table's pages begin in its file: the byte at which the record
//! after so many begins, learned as pages are read. A page turned to is
//! read from the nearest start before it, where each was read from the
//! file's start, every page before it read again.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::SystemTime;

use skwir_core::preview::Held;

/// A version of a file: its path, length and modification time, as a
/// preview read it (`Held::version`, ADR-101): the path is asked nothing.
pub(crate) type Version = (PathBuf, u64, Option<SystemTime>);

/// The files' page starts: `(records before, byte)`, by file, the last few.
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
        .and_then(|(_, known)| {
            known
                .iter()
                .filter(|(before, _)| *before <= records)
                .max_by_key(|(before, _)| *before)
                .copied()
        })
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
    if known.len() < PER_FILE && !known.iter().any(|(before, _)| *before == records) {
        known.push((records, byte));
    }
}
