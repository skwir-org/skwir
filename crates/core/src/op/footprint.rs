//! What an action touches, to tell which actions may run at once
//! (ADR-079). Each step says it ([`super::Operation::footprint`]): the
//! trees it reads, the trees it changes (made, written or removed), the
//! folders whose names it changes, and the stores several actions keep (a
//! volume's trash, the notes'). Two actions conflict when one changes
//! what the other touches; reads never meet. A step that cannot say
//! touches everything: its action runs alone, as every action did.
//!
//! Paths are compared by their components, each folder made canonical
//! on this machine's volumes (a link to a folder is the folder: one look
//! a folder), as named on a remote (where a look is a round trip, and may
//! wait for good).
//!
//! A heavy action moves bytes or walks a tree (a copy, a move to another
//! volume, a folder deleted for good, an archive): one at a time
//! (ADR-079, 3).

use std::collections::BTreeSet;
use std::ops::Bound;
use std::path::{Path, PathBuf};

/// The most steps of an action that is light: more, and it writes a disk
/// as a copy does (ten thousand photos renamed).
pub const LIGHT_STEPS: usize = 1_000;

/// What an action touches, and whether it is heavy.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Footprint {
    /// Trees read.
    reads: BTreeSet<PathBuf>,
    /// Trees made, written or removed.
    writes: BTreeSet<PathBuf>,
    /// Folders whose names change.
    names: BTreeSet<PathBuf>,
    /// Stores several actions keep.
    stores: BTreeSet<&'static str>,
    /// What it touches cannot be told: everything.
    every: bool,
    /// It moves bytes or walks a tree.
    heavy: bool,
}

/// A volume's trash, its `files` and `info`, and every trash a restore may
/// read: one store.
pub const TRASH: &str = "trash";
/// The notes' stores (tags, ratings, comments) and their index.
pub const NOTES: &str = "notes";

impl Footprint {
    /// Touching everything: what a step that cannot say touches.
    pub fn every() -> Self {
        Footprint {
            every: true,
            heavy: true,
            ..Footprint::default()
        }
    }

    /// The tree at `path` read.
    #[must_use]
    pub fn read(mut self, path: &Path) -> Self {
        self.reads.insert(path.to_path_buf());
        self
    }

    /// The tree at `path` made, written or removed, and the names of its
    /// folder.
    #[must_use]
    pub fn made(mut self, path: &Path) -> Self {
        self.writes.insert(path.to_path_buf());
        if let Some(folder) = path.parent() {
            self.names.insert(folder.to_path_buf());
        }
        self
    }

    /// The tree at `path` written in place: its attributes, its notes.
    #[must_use]
    pub fn written(mut self, path: &Path) -> Self {
        self.writes.insert(path.to_path_buf());
        self
    }

    /// A store several actions keep.
    #[must_use]
    pub fn store(mut self, store: &'static str) -> Self {
        self.stores.insert(store);
        self
    }

    /// A store several actions keep, where `kept` says.
    #[must_use]
    pub fn store_if(self, kept: bool, store: &'static str) -> Self {
        if kept { self.store(store) } else { self }
    }

    /// Heavy where `heavy` says.
    #[must_use]
    pub fn heavy_if(mut self, heavy: bool) -> Self {
        self.heavy |= heavy;
        self
    }

    pub fn is_heavy(&self) -> bool {
        self.heavy
    }

    /// Whether it touches everything.
    pub fn is_every(&self) -> bool {
        self.every
    }

    /// The same, each path's folder made canonical on this machine's
    /// volumes (module documentation). Reads: never on the window's
    /// thread.
    #[must_use]
    pub fn canonical(self) -> Self {
        let mut folders: std::collections::HashMap<PathBuf, PathBuf> =
            std::collections::HashMap::new();
        let mut canonical = |path: PathBuf| -> PathBuf {
            let (Some(folder), Some(name)) = (path.parent(), path.file_name()) else {
                return path;
            };
            if crate::fs::reach::of(folder).is_some() {
                return path;
            }
            let folder = folders
                .entry(folder.to_path_buf())
                .or_insert_with(|| {
                    std::fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf())
                })
                .clone();
            folder.join(name)
        };
        let mut all = |set: BTreeSet<PathBuf>| set.into_iter().map(&mut canonical).collect();
        Footprint {
            reads: all(self.reads),
            writes: all(self.writes),
            names: all(self.names),
            ..self
        }
    }

    /// What `self` and `other` touch together.
    pub fn merge(&mut self, other: Footprint) {
        self.reads.extend(other.reads);
        self.writes.extend(other.writes);
        self.names.extend(other.names);
        self.stores.extend(other.stores);
        self.every |= other.every;
        self.heavy |= other.heavy;
    }

    /// What the steps of an action touch together, `None` for a step that
    /// cannot say (`every`); more than [`LIGHT_STEPS`] steps are heavy.
    pub fn of_steps(steps: impl IntoIterator<Item = Option<Footprint>>) -> Footprint {
        let mut all = Footprint::default();
        let mut count = 0;
        for step in steps {
            count += 1;
            match step {
                Some(step) => all.merge(step),
                None => return Footprint::every(),
            }
        }
        all.heavy_if(count > LIGHT_STEPS)
    }

    /// Whether `self` and `other` may not run at once: one changes what
    /// the other touches, or both keep one store.
    pub fn conflicts(&self, other: &Footprint) -> bool {
        if self.every || other.every || self.stores.intersection(&other.stores).next().is_some() {
            return true;
        }
        let changes = |a: &Footprint, b: &Footprint| {
            a.writes.iter().any(|tree| {
                meets(&b.reads, tree) || meets(&b.writes, tree) || within(&b.names, tree)
            }) || a.names.iter().any(|folder| {
                b.names.contains(folder)
                    || folder
                        .ancestors()
                        .any(|above| b.reads.contains(above) || b.writes.contains(above))
            })
        };
        changes(self, other) || changes(other, self)
    }
}

/// Whether `set` holds `tree`, a tree above it, or one within it.
fn meets(set: &BTreeSet<PathBuf>, tree: &Path) -> bool {
    within(set, tree) || tree.ancestors().skip(1).any(|above| set.contains(above))
}

/// Whether `set` holds `tree` or a path within it: those follow it in the
/// set's order, by their components.
fn within(set: &BTreeSet<PathBuf>, tree: &Path) -> bool {
    set.range::<Path, _>((Bound::Included(tree), Bound::Unbounded))
        .next()
        .is_some_and(|first| first.starts_with(tree))
}

/// What a footprint asks of the volumes, read once for an action's steps:
/// which volume holds a path (its mount, by the mount table), and whether
/// a path on this machine is a folder.
pub struct Volumes {
    /// Mount points, the longest first.
    points: Option<Vec<PathBuf>>,
}

impl Volumes {
    /// The volumes as the mount table says now; where it cannot be read
    /// (not Linux), none is told apart.
    pub fn now() -> Self {
        #[cfg(target_os = "linux")]
        let points = std::fs::read("/proc/self/mountinfo").ok().map(|text| {
            let mut points: Vec<PathBuf> = crate::fs::volume::mountinfo(&text)
                .into_iter()
                .map(|m| m.point)
                .collect();
            points.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
            points
        });
        #[cfg(not(target_os = "linux"))]
        let points = None;
        Volumes { points }
    }

    /// Whether `a` and `b` are on one volume: a move between them is a
    /// rename. Where it cannot be told, not.
    pub fn same(&self, a: &Path, b: &Path) -> bool {
        let Some(points) = &self.points else {
            return false;
        };
        let of = |path: &Path| points.iter().find(|point| path.starts_with(point));
        matches!((of(a), of(b)), (Some(x), Some(y)) if x == y)
    }

    /// Whether `path` is a folder: looked at on this machine's volumes
    /// alone; on a remote, where a look may wait for good, taken to be.
    pub fn is_folder(&self, path: &Path) -> bool {
        crate::fs::reach::of(path).is_some()
            || std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(path: &str) -> PathBuf {
        PathBuf::from(path)
    }

    #[test]
    fn what_meets_and_what_does_not() {
        let copy = Footprint::default()
            .read(&p("/disk/photos"))
            .made(&p("/stick/photos"));
        // A rename elsewhere: nothing shared.
        let rename = Footprint::default()
            .made(&p("/disk/notes/a.txt"))
            .made(&p("/disk/notes/b.txt"));
        assert!(!copy.conflicts(&rename));
        // A rename in the tree read.
        let inside = Footprint::default()
            .made(&p("/disk/photos/2026/a.jpg"))
            .made(&p("/disk/photos/2026/b.jpg"));
        assert!(copy.conflicts(&inside) && inside.conflicts(&copy));
        // A name made beside the tree read, in its folder: no matter.
        let beside = Footprint::default().made(&p("/disk/new folder"));
        assert!(!copy.conflicts(&beside));
        // Into the folder the copy makes names in.
        let into = Footprint::default().made(&p("/stick/other.txt"));
        assert!(copy.conflicts(&into));
        // Reads never meet.
        let again = Footprint::default()
            .read(&p("/disk/photos"))
            .made(&p("/elsewhere/photos"));
        assert!(!copy.conflicts(&again));
        // A folder above the tree read, removed.
        let gone = Footprint::default().made(&p("/disk"));
        assert!(copy.conflicts(&gone));
        // One store, kept by both.
        let trash = Footprint::default().made(&p("/a")).store(TRASH);
        let restore = Footprint::default().made(&p("/b")).store(TRASH);
        assert!(trash.conflicts(&restore));
        // A name like another, not within it.
        let photos2 = Footprint::default().made(&p("/disk/photos2"));
        assert!(!copy.conflicts(&photos2));
        // What cannot be told meets everything.
        assert!(Footprint::every().conflicts(&Footprint::default()));
    }

    /// A folder reached through a link is the folder: two plans naming
    /// it two ways meet.
    #[cfg(unix)]
    #[test]
    fn a_folder_named_through_a_link_is_the_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("photos");
        std::fs::create_dir(&real).unwrap();
        std::os::unix::fs::symlink(&real, tmp.path().join("pictures")).unwrap();
        let one = Footprint::default().made(&real.join("a.jpg")).canonical();
        let other = Footprint::default()
            .made(&tmp.path().join("pictures/a.jpg"))
            .canonical();
        assert!(one.conflicts(&other));
    }

    #[test]
    fn many_steps_are_heavy_and_one_unknown_is_every() {
        let light = Footprint::of_steps(
            (0..10).map(|n| Some(Footprint::default().made(&p(&format!("/a/{n}"))))),
        );
        assert!(!light.is_heavy() && !light.is_every());
        let many = Footprint::of_steps(
            (0..=LIGHT_STEPS).map(|n| Some(Footprint::default().made(&p(&format!("/a/{n}"))))),
        );
        assert!(many.is_heavy());
        let unknown = Footprint::of_steps([Some(Footprint::default()), None]);
        assert!(unknown.is_every());
    }
}
