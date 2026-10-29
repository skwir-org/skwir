//! The file the preview showed before the one it shows, as it was read
//! (ADR-101): kept a minute unseen, so flipping between two files reads
//! neither again. One for the process, as the grammars are
//! (`code_preview`), let go of by a thread that lives only while one is
//! kept. A file larger than a quick preview holds is not kept, and Full
//! preview holds the file it shows alone (`let_go`): the two together hold
//! `2 × preview::WHOLE` at most.

use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use parking_lot::Mutex;
use skwir_core::op::EntryRef;
use skwir_core::preview::Held;

/// How long the file shown before is kept unseen.
const KEPT: Duration = Duration::from_secs(60);

struct Before {
    entry: EntryRef,
    modified: Option<SystemTime>,
    held: Arc<Held>,
    since: Instant,
}

struct Kept {
    before: Option<Before>,
    /// Whether a thread waits to let go of it.
    waiting: bool,
}

static KEPT_BEFORE: Mutex<Kept> = Mutex::new(Kept {
    before: None,
    waiting: false,
});

/// Keep `held`, what the preview read of `entry` as it was at `modified`,
/// as the file shown before, in place of the one kept.
pub(crate) fn keep(entry: EntryRef, modified: Option<SystemTime>, held: Arc<Held>) {
    if held.kept() as u64 > skwir_core::preview::WHOLE {
        return;
    }
    let replaced = {
        let mut kept = KEPT_BEFORE.lock();
        let replaced = kept.before.replace(Before {
            entry,
            modified,
            held,
            since: Instant::now(),
        });
        if !kept.waiting {
            // A thread lives only while a file is kept.
            kept.waiting = std::thread::Builder::new()
                .name("skwir-held-before".into())
                .spawn(let_go_when_due)
                .is_ok();
        }
        replaced
    };
    // What it frees is freed outside the lock.
    drop(replaced);
}

/// What was kept of `entry` as it was at `modified`, taken back to be
/// shown again.
pub(crate) fn take(entry: &EntryRef, modified: Option<SystemTime>) -> Option<Arc<Held>> {
    let mut kept = KEPT_BEFORE.lock();
    let found = kept
        .before
        .as_ref()
        .is_some_and(|before| before.entry == *entry && before.modified == modified);
    if found {
        kept.before.take().map(|before| before.held)
    } else {
        None
    }
}

/// Let go of the file kept: Full preview holds the one it shows alone.
pub(crate) fn let_go() {
    let before = KEPT_BEFORE.lock().before.take();
    drop(before);
}

/// Wait until the file kept has been unseen for `KEPT`, then let go of it.
fn let_go_when_due() {
    loop {
        let wait = {
            let mut kept = KEPT_BEFORE.lock();
            let Some(due) = kept.before.as_ref().map(|before| before.since + KEPT) else {
                kept.waiting = false;
                return;
            };
            let now = Instant::now();
            if due <= now {
                let before = kept.before.take();
                kept.waiting = false;
                drop(kept);
                // `Held` gives back what workers read of it as it goes.
                drop(before);
                return;
            }
            due - now
        };
        std::thread::sleep(wait);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn held(dir: &std::path::Path, name: &str, bytes: usize) -> (EntryRef, Arc<Held>) {
        let path = dir.join(name);
        std::fs::write(&path, b"").unwrap();
        let entry = EntryRef::from_path(&path).unwrap();
        (entry, Arc::new(Held::of_bytes(path, vec![0; bytes])))
    }

    #[test]
    fn the_file_shown_before_comes_back_as_it_was_and_only_so() {
        let dir = tempfile::tempdir().unwrap();
        let (entry, file) = held(dir.path(), "before.txt", 10);
        keep(entry.clone(), None, Arc::clone(&file));
        let other = SystemTime::UNIX_EPOCH;
        assert!(
            take(&entry, Some(other)).is_none(),
            "another version is not it"
        );
        let back = take(&entry, None).expect("kept");
        assert!(Arc::ptr_eq(&back, &file));
        assert!(take(&entry, None).is_none(), "taken once");
        let (large, bigger) = held(
            dir.path(),
            "large.bin",
            skwir_core::preview::WHOLE as usize + 1,
        );
        keep(large.clone(), None, bigger);
        assert!(take(&large, None).is_none(), "past a quick preview's hold");
    }
}
