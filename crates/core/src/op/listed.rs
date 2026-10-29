//! A folder's names as one listing read them: what a plan's steps ask of
//! a folder more than once (where a file gone from its name went, how a
//! name is spelled on a volume that tells no case apart) read once a
//! folder rather than once a step (`StepContext::listed`, which keeps it
//! while the folder is as it was).

use std::cell::OnceCell;
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::path::Path;

/// How many folders [`Listed::read`] listed, for a test to count.
#[cfg(test)]
pub(super) static READ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// The names of a folder, each with the inode number its listing gives
/// it (`d_ino`; 0 where a listing gives none, as on Windows).
#[derive(Debug, Default)]
pub(super) struct Listed {
    names: HashMap<OsString, u64>,
    /// The names by number, made when first asked ([`Listed::names_of`]).
    by_inode: OnceCell<HashMap<u64, Vec<OsString>>>,
}

impl Listed {
    /// The names of the folder `dir`, read now; `None` when it cannot be
    /// listed. Blocking.
    pub(super) fn read(dir: &Path) -> Option<Listed> {
        #[cfg(test)]
        READ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let names = std::fs::read_dir(dir)
            .ok()?
            .flatten()
            .map(|entry| (entry.file_name(), inode_of(&entry)))
            .collect();
        Some(Listed {
            names,
            by_inode: OnceCell::new(),
        })
    }

    /// The entry `from` renamed `to` in the folder, as the plan renamed
    /// it: false, and nothing changed, when the listing holds no `from`.
    pub(super) fn renamed(&mut self, from: &OsStr, to: &OsStr) -> bool {
        let Some(ino) = self.names.remove(from) else {
            return false;
        };
        self.names.insert(to.to_owned(), ino);
        self.by_inode.take();
        true
    }

    /// Whether the folder holds an entry spelled exactly `name`.
    pub(super) fn holds(&self, name: &OsStr) -> bool {
        self.names.contains_key(name)
    }

    /// The names the listing gives the inode number `ino`: those that may
    /// name that file (on a mount point the listing's number is the one
    /// under the mount, which no rename in place reaches). Every name
    /// where the listing gives no numbers.
    pub(super) fn names_of(&self, ino: u64) -> &[OsString] {
        let ino = if cfg!(unix) { ino } else { 0 };
        let by_inode = self.by_inode.get_or_init(|| {
            let mut by_inode: HashMap<u64, Vec<OsString>> = HashMap::new();
            for (name, &ino) in &self.names {
                by_inode.entry(ino).or_default().push(name.clone());
            }
            by_inode
        });
        by_inode.get(&ino).map_or(&[], Vec::as_slice)
    }
}

/// The inode number a listing gives `entry`: 0 where it gives none.
fn inode_of(entry: &std::fs::DirEntry) -> u64 {
    #[cfg(unix)]
    {
        std::os::unix::fs::DirEntryExt::ino(entry)
    }
    #[cfg(not(unix))]
    {
        let _ = entry;
        0
    }
}
