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
//! volume, a folder deleted for good, an archive): one at a time on each
//! of its channels, the disks and servers it reads or writes (ADR-079, 3
//! and 4): two copies on one disk take turns, which ends sooner than
//! sharing it; on two disks, or a disk and a server, they run at once.

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
    /// Where a heavy one does ([`Footprint::with_channels`]).
    channels: BTreeSet<Channel>,
}

/// Where a heavy action moves bytes: one heavy action at a time each.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Channel {
    /// A physical disk of this machine, by its name in sysfs, under any
    /// encrypted, mapped or RAID volume, all its partitions one.
    Disk(String),
    /// A remote, by its mount.
    Remote(PathBuf),
    /// A volume of this machine kept in memory (tmpfs), by its mount.
    Volume(PathBuf),
    /// A volume of this machine whose disks cannot be told (all of them on
    /// macOS and Windows for now, a ZFS pool): it may be on any disk, so
    /// it meets every disk's channel and its own.
    Here,
}

impl Channel {
    /// Whether one heavy action on `self` and one on `other` would share a
    /// device.
    fn meets(&self, other: &Channel) -> bool {
        match (self, other) {
            (Channel::Here, Channel::Here | Channel::Disk(_))
            | (Channel::Disk(_), Channel::Here) => true,
            _ => self == other,
        }
    }
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

    /// The same, a heavy one given the channels of what it touches: the
    /// disks under each folder of this machine's, the remote holding one
    /// of a remote's. Reads (sysfs, the mount table): never on the
    /// window's thread.
    #[must_use]
    pub fn with_channels(mut self, volumes: &Volumes) -> Self {
        if !self.heavy || self.every {
            return self;
        }
        let mut known: std::collections::HashMap<PathBuf, Vec<Channel>> =
            std::collections::HashMap::new();
        let mut channels = BTreeSet::new();
        for path in self.reads.iter().chain(&self.writes) {
            // A file is on its folder's; a volume's top on its own.
            let folder = if volumes.is_point(path) {
                path.as_path()
            } else {
                path.parent().unwrap_or(path)
            };
            let of = known
                .entry(folder.to_path_buf())
                .or_insert_with(|| volumes.channels_of(folder));
            channels.extend(of.iter().cloned());
        }
        self.channels = channels;
        self
    }

    /// Whether two heavy ones may not run at once: a channel in common,
    /// or one not told.
    pub fn shares_channel(&self, other: &Footprint) -> bool {
        self.every
            || other.every
            || self.channels.is_empty()
            || other.channels.is_empty()
            || self
                .channels
                .iter()
                .any(|mine| other.channels.iter().any(|theirs| mine.meets(theirs)))
    }

    /// Whether the channel two heavy ones share is a server's (else a
    /// disk of this machine's, or one not told): what a waiting one says
    /// it waits for.
    pub fn shares_a_server(&self, other: &Footprint) -> bool {
        self.channels.iter().any(|mine| {
            matches!(mine, Channel::Remote(_)) && other.channels.iter().any(|t| mine.meets(t))
        })
    }

    /// What `self` and `other` touch together.
    pub fn merge(&mut self, other: Footprint) {
        self.reads.extend(other.reads);
        self.writes.extend(other.writes);
        self.names.extend(other.names);
        self.stores.extend(other.stores);
        self.every |= other.every;
        self.heavy |= other.heavy;
        self.channels.extend(other.channels);
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
    /// Those of volumes kept in memory (tmpfs, ramfs): on no disk.
    in_memory: Vec<PathBuf>,
}

impl Volumes {
    /// The volumes as the mount table says now; where it cannot be read
    /// (not Linux), none is told apart.
    pub fn now() -> Self {
        #[cfg(target_os = "linux")]
        if let Ok(text) = std::fs::read("/proc/self/mountinfo") {
            let mounts = crate::fs::volume::mountinfo(&text);
            let in_memory = mounts
                .iter()
                .filter(|m| matches!(m.fstype.as_str(), "tmpfs" | "ramfs"))
                .map(|m| m.point.clone())
                .collect();
            let mut points: Vec<PathBuf> = mounts.into_iter().map(|m| m.point).collect();
            points.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
            return Volumes {
                points: Some(points),
                in_memory,
            };
        }
        Volumes {
            points: None,
            in_memory: Vec::new(),
        }
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

    /// Whether a volume is mounted at `path`.
    pub fn is_point(&self, path: &Path) -> bool {
        self.points
            .as_ref()
            .is_some_and(|points| points.iter().any(|p| p == path))
    }

    /// The channels the folder `folder` is on (module documentation): its
    /// remote's, else its disks', else its volume's when it is kept in
    /// memory; where none can be told, any of this machine's disks.
    pub fn channels_of(&self, folder: &Path) -> Vec<Channel> {
        if let Some(remote) = crate::fs::reach::of(folder) {
            return vec![Channel::Remote(remote.point.clone())];
        }
        // What is made is on the volume of the nearest folder there.
        let Some(existing) = folder.ancestors().find(|a| a.exists()) else {
            return vec![Channel::Here];
        };
        let disks = crate::fs::volume::disks_of(existing);
        if !disks.is_empty() {
            return disks.into_iter().map(Channel::Disk).collect();
        }
        let point = self
            .points
            .iter()
            .flatten()
            .find(|point| existing.starts_with(point));
        match point {
            Some(point) if self.in_memory.contains(point) => vec![Channel::Volume(point.clone())],
            _ => vec![Channel::Here],
        }
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

    /// Heavy actions on two remotes share no channel; on one, they do.
    #[cfg(target_os = "linux")]
    #[test]
    fn heavy_actions_on_two_servers_share_no_channel() {
        crate::fs::reach::pretend_remote(&p("/servers/one"));
        crate::fs::reach::pretend_remote(&p("/servers/two"));
        let volumes = Volumes::now();
        let heavy = |path: &str| {
            Footprint::default()
                .made(&p(path))
                .heavy_if(true)
                .with_channels(&volumes)
        };
        let (one, two) = (heavy("/servers/one/a"), heavy("/servers/two/b"));
        assert!(!one.shares_channel(&two));
        assert!(one.shares_channel(&heavy("/servers/one/c")));
        assert!(!one.conflicts(&two), "nothing touched in common either");
    }

    /// A volume whose disks cannot be told may be on any of them: it meets
    /// every disk, never a server or a volume kept in memory.
    #[test]
    fn a_volume_not_told_meets_every_disk() {
        let on = |channels: &[Channel]| Footprint {
            heavy: true,
            channels: channels.iter().cloned().collect(),
            ..Footprint::default()
        };
        let sda = on(&[Channel::Disk("sda".into())]);
        let pool = on(&[Channel::Here]);
        let server = on(&[Channel::Remote(p("/servers/one"))]);
        let shm = on(&[Channel::Volume(p("/dev/shm"))]);
        assert!(pool.shares_channel(&sda));
        assert!(pool.shares_channel(&pool.clone()));
        assert!(!pool.shares_channel(&server));
        assert!(!pool.shares_channel(&shm));
        assert!(!sda.shares_channel(&on(&[Channel::Disk("sdb".into())])));
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
