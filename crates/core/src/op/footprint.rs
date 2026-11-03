//! What an action touches, to tell which actions may run at once
//! (ADR-079). Each step says it ([`super::Operation::footprint`]): the
//! trees it reads, the trees it changes (made, written or removed), the
//! names it makes, takes or changes in a folder, and the stores several
//! actions keep (a volume's trash, the notes'): what they hold, or only a
//! record of its own there. Two actions conflict when one changes what the
//! other touches; reads never meet, nor two records of their own, nor two
//! names of one folder that the volume tells apart. A step that cannot say
//! touches everything: its action runs alone, as every action did.
//!
//! Names are compared as a volume that tells no case apart compares them
//! ([`Names`]). A name a step makes that is settled only when its turn
//! comes (a copy's, kept beside one taken as `name (copy).ext`) claims
//! the names it may be settled as, and the staging folders a copy makes
//! there ([`Footprint::settles_name`]): a new folder beside a long copy,
//! running or waiting its turn, starts at once.
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

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::ops::Bound;
use std::path::{Path, PathBuf};
use std::sync::Arc;

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
    /// Folders whose names change, and which names.
    names: BTreeMap<PathBuf, Names>,
    /// Stores several actions keep, what they hold: read, taken, emptied.
    stores: BTreeSet<&'static str>,
    /// Stores it keeps only a record of its own in ([`Footprint::record_in`]).
    records: BTreeSet<&'static str>,
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

/// The names an action makes, takes or changes in one folder.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Names {
    /// These, each as [`name_key`] gives it, and every name beginning with
    /// one of `kept` (the names one settled later may be kept as,
    /// [`Footprint::settles_name`]).
    These {
        names: BTreeSet<String>,
        kept: BTreeSet<String>,
    },
    /// Any name there: one the volume may take for another ([`name_key`]).
    Every,
}

impl Default for Names {
    fn default() -> Self {
        Names::These {
            names: BTreeSet::new(),
            kept: BTreeSet::new(),
        }
    }
}

impl Names {
    /// The name `key` too ([`name_key`]; `None`, any name).
    fn add(&mut self, key: Option<String>) {
        match (self, key) {
            (Names::Every, _) => {}
            (names, None) => *names = Names::Every,
            (Names::These { names, .. }, Some(key)) => {
                names.insert(key);
            }
        }
    }

    /// Every name beginning with `key` too (`None`, any name).
    fn add_kept(&mut self, key: Option<String>) {
        match (self, key) {
            (Names::Every, _) => {}
            (names, None) => *names = Names::Every,
            (Names::These { kept, .. }, Some(key)) => {
                kept.insert(key);
            }
        }
    }

    fn merge(&mut self, other: Names) {
        match other {
            Names::Every => *self = Names::Every,
            Names::These { names, kept } => {
                if let Names::These {
                    names: these,
                    kept: beside,
                } = self
                {
                    these.extend(names);
                    beside.extend(kept);
                }
            }
        }
    }

    /// How many claims they are: the folder's, and each name and
    /// beginning of names.
    fn claims(&self) -> usize {
        match self {
            Names::These { names, kept } => 1 + names.len() + kept.len(),
            Names::Every => 1,
        }
    }

    /// Whether two actions' names of one folder may be one name.
    fn meets(&self, other: &Names) -> bool {
        let (
            Names::These {
                names: a,
                kept: a_kept,
            },
            Names::These {
                names: b,
                kept: b_kept,
            },
        ) = (self, other)
        else {
            return true;
        };
        let (small, large) = if a.len() <= b.len() { (a, b) } else { (b, a) };
        // Two prefixes meet where one begins the other: each of the fewer
        // looked up in the others, as a beginning of theirs and by its own
        // beginnings that may be one ([`kept_ends`]).
        let (few, many) = if a_kept.len() <= b_kept.len() {
            (a_kept, b_kept)
        } else {
            (b_kept, a_kept)
        };
        let kept_meet = || {
            few.iter().any(|p| {
                begins(many, p) || kept_ends(p).any(|end| end < p.len() && many.contains(&p[..end]))
            })
        };
        small.iter().any(|name| large.contains(name))
            || begins_any(b, a_kept)
            || begins_any(a, b_kept)
            || kept_meet()
    }
}

/// Whether one of `names` begins with `prefix`: it follows `prefix` in
/// the set's order.
fn begins(names: &BTreeSet<String>, prefix: &str) -> bool {
    names
        .range::<str, _>((Bound::Included(prefix), Bound::Unbounded))
        .next()
        .is_some_and(|name| name.starts_with(prefix))
}

/// Whether one of `names` begins with one of `prefixes`, the fewer looked
/// up in the others: each prefix as a beginning of the names, or each name
/// by its beginnings that may be a kept prefix ([`kept_ends`]). A copy of
/// a hundred thousand files claims as many prefixes; a rename beside it,
/// two names.
fn begins_any(names: &BTreeSet<String>, prefixes: &BTreeSet<String>) -> bool {
    if prefixes.len() <= names.len() {
        return prefixes.iter().any(|prefix| begins(names, prefix));
    }
    names.iter().any(|name| {
        kept_ends(name)
            .chain(std::iter::once(name.len()))
            .any(|end| prefixes.contains(&name[..end]))
    })
}

/// Where in `name` a kept prefix that begins it may end
/// (`clash::kept_prefixes`): right after each ` (`, which every one ends
/// with unless cut to the longest, and, for one cut so, at each character
/// past the fewest bytes such a one keeps once its case is left out (at
/// least a character of each of `KEPT_PREFIX`'s bytes but the three a
/// cut may leave out, four bytes each at most).
fn kept_ends(name: &str) -> impl Iterator<Item = usize> + '_ {
    const SHORTEST_CUT: usize = super::clash::KEPT_PREFIX.saturating_sub(3) / 4;
    name.match_indices(" (").map(|(at, _)| at + 2).chain(
        name.char_indices()
            .map(|(at, _)| at)
            .filter(|&at| at >= SHORTEST_CUT),
    )
}

/// What a name is compared as: its case left out, as a volume that tells
/// none apart (FAT, exFAT, NTFS, SMB, APFS as it comes) compares it; on
/// Windows its trailing dots and spaces too, which Windows drops. Every
/// staging folder is one name ([`super::staging`]): a copy claims the
/// ones it makes there by it. `None` where a volume may take the name for
/// another this cannot tell: on macOS one that is not ASCII (APFS and
/// HFS+ take each Unicode normalization of it as one), on Windows one
/// holding a `~` (a short 8.3 name another file's long one may have).
fn name_key(name: &OsStr) -> Option<String> {
    let folded = folded(name)?;
    let folded = if cfg!(windows) {
        folded.trim_end_matches(['.', ' ']).to_owned()
    } else {
        folded
    };
    if folded.starts_with(super::staging::PREFIX) {
        return Some(super::staging::PREFIX.to_owned());
    }
    Some(folded)
}

/// What the beginning of names is compared as, as [`name_key`] compares
/// a name.
fn prefix_key(prefix: &OsStr) -> Option<String> {
    folded(prefix)
}

/// A name, its case left out (`context::folded`); `None` where this
/// system's volumes may take it for another this cannot tell
/// ([`name_key`]).
fn folded(name: &OsStr) -> Option<String> {
    let folded = super::context::folded(name);
    if cfg!(target_os = "macos") && !folded.is_ascii() {
        return None;
    }
    if cfg!(windows) && folded.contains('~') {
        return None;
    }
    Some(folded)
}

/// A volume's trash, its `files` and `info`, and every trash a restore may
/// read: one store. A file trashed adds an item of its own; a restore, an
/// item deleted for good or moved, keeps it whole.
pub const TRASH: &str = "trash";
/// The notes' stores (tags, ratings, comments): each file's record in the
/// file or beside it, which an edit's paths claim.
pub const NOTES: &str = "notes";

impl Footprint {
    /// The trees it reads, writes or names in; none when it touches
    /// everything.
    pub fn paths(&self) -> impl Iterator<Item = &Path> {
        self.reads
            .iter()
            .chain(&self.writes)
            .chain(self.names.keys())
            .map(PathBuf::as_path)
    }

    /// Whether it touches anything at or below `tree`: reads, changes or
    /// names there; everything, where it cannot say.
    pub fn touches(&self, tree: &Path) -> bool {
        self.every || self.paths().any(|path| path.starts_with(tree))
    }

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

    /// The tree at `path` made, written or removed, and its name in its
    /// folder.
    #[must_use]
    pub fn made(mut self, path: &Path) -> Self {
        self.writes.insert(path.to_path_buf());
        if let Some(folder) = path.parent() {
            let key = path.file_name().and_then(name_key);
            self.names.entry(folder.to_path_buf()).or_default().add(key);
        }
        self
    }

    /// The tree at `path` made under a name settled only when its turn
    /// comes: its own, or one kept beside it where its own is taken
    /// (`name (copy).ext`, ADR-054), which begins as
    /// `clash::kept_prefixes` says; and, where it `stages` (a copy, what a step makes), the
    /// staging folders made beside it first. What it replaces there, or in
    /// a folder it merges into, is put in the trash by a step of its own
    /// (`clash::put_aside`), whose record it claims: what its plan touches
    /// is claimed before its names are settled.
    #[must_use]
    pub fn settles_name(mut self, path: &Path, stages: bool) -> Self {
        self = self.record_in(TRASH);
        let (Some(folder), Some(name)) = (path.parent(), path.file_name()) else {
            return self.made(path);
        };
        self = self.made(path);
        let names = self.names.entry(folder.to_path_buf()).or_default();
        for prefix in super::clash::kept_prefixes(name) {
            names.add_kept(prefix_key(&prefix));
        }
        if stages {
            names.add(Some(super::staging::PREFIX.to_owned()));
        }
        self
    }

    /// The tree at `path` made, written or removed, as [`Footprint::made`],
    /// with its companions' places (ADR-028), as `volumes` has their rules
    /// (none looked at): they go where it goes, in a folder of their own
    /// too (`.ts`).
    #[must_use]
    pub fn made_with(self, path: &Path, volumes: &Volumes) -> Self {
        volumes
            .beside(path)
            .iter()
            .fold(self.made(path), |all, place| all.made(place))
    }

    /// The tree at `path` read, as [`Footprint::read`], with its
    /// companions' places (a copy carries them).
    #[must_use]
    pub fn read_with(self, path: &Path, volumes: &Volumes) -> Self {
        volumes
            .beside(path)
            .iter()
            .fold(self.read(path), |all, place| all.read(place))
    }

    /// The tree at `path` written in place: its attributes, its notes.
    #[must_use]
    pub fn written(mut self, path: &Path) -> Self {
        self.writes.insert(path.to_path_buf());
        self
    }

    /// A store several actions keep: what it holds read, taken, emptied.
    /// It meets every action that keeps that store.
    #[must_use]
    pub fn store(mut self, store: &'static str) -> Self {
        self.stores.insert(store);
        self
    }

    /// Only a record of its own in a store several actions keep: one it
    /// adds under a name it alone takes (a file put in the trash, its
    /// record made new), or its file's, which its paths claim (a file's
    /// notes, beside it or in it). Two such meet only where their paths
    /// do; one that keeps the store whole ([`Footprint::store`]: a restore,
    /// an item deleted for good) meets it.
    #[must_use]
    pub fn record_in(mut self, store: &'static str) -> Self {
        self.records.insert(store);
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
        let reads = all(self.reads);
        let writes = all(self.writes);
        let mut names: BTreeMap<PathBuf, Names> = BTreeMap::new();
        for (folder, these) in self.names {
            names.entry(canonical(folder)).or_default().merge(these);
        }
        Footprint {
            reads,
            writes,
            names,
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
        for (folder, names) in other.names {
            self.names.entry(folder).or_default().merge(names);
        }
        self.stores.extend(other.stores);
        self.records.extend(other.records);
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

    /// Whether `self` and `other` may not run at once, nor be taken back
    /// apart: one changes what the other touches, or one keeps whole a
    /// store the other keeps (two records of their own there, nothing).
    pub fn conflicts(&self, other: &Footprint) -> bool {
        if self.meets_whole(other) {
            return true;
        }
        // Each claim of the one of fewer looked up in the other's sets: an
        // action of a hundred thousand paths meets one of a thousand in a
        // thousand lookups.
        let size = |f: &Footprint| f.reads.len() + f.writes.len() + f.names.len();
        let (few, many) = if size(self) <= size(other) {
            (self, other)
        } else {
            (other, self)
        };
        few.paths_meet(many)
    }

    /// Whether what `self` and `other` touch by their paths meets, each
    /// claim of `self` looked up in `other`'s sets: a tree changed meets
    /// what the other reads or changes at, above or below it, and the
    /// names the other changes at or below it; a tree read, what the other
    /// changes so; the names changed in a folder, the other's there that
    /// may be one of them, and a tree the other reads or changes holding
    /// the folder. Reads never meet reads.
    fn paths_meet(&self, other: &Footprint) -> bool {
        self.writes.iter().any(|tree| {
            meets(&other.reads, tree)
                || meets(&other.writes, tree)
                || folder_within(&other.names, tree)
        }) || self
            .reads
            .iter()
            .any(|tree| meets(&other.writes, tree) || folder_within(&other.names, tree))
            || self.names.iter().any(|(folder, names)| {
                other
                    .names
                    .get(folder)
                    .is_some_and(|theirs| names.meets(theirs))
                    || folder
                        .ancestors()
                        .any(|above| other.reads.contains(above) || other.writes.contains(above))
            })
    }

    /// The names right below `folder` its paths below it go through
    /// (those at it are names it makes there, compared as names); `None`
    /// when it reads or writes `folder` itself, which meets all below it.
    /// Its paths below a folder follow it in each set's order.
    fn heads_at(&self, folder: &Path) -> Option<Heads> {
        if self.reads.contains(folder) || self.writes.contains(folder) {
            return None;
        }
        let depth = folder.components().count();
        let from = (Bound::Included(folder), Bound::Unbounded);
        let below = |p: &&PathBuf| p.starts_with(folder);
        let paths = self
            .reads
            .range::<Path, _>(from)
            .take_while(below)
            .chain(self.writes.range::<Path, _>(from).take_while(below))
            .chain(
                self.names
                    .range::<Path, _>(from)
                    .map(|(f, _)| f)
                    .take_while(below),
            );
        Some(
            paths
                .filter_map(|p| p.components().nth(depth))
                .map(|head| head_key(head.as_os_str()))
                .collect(),
        )
    }

    /// Whether the names `self` and `other` make or change in `folder`
    /// may be one name.
    fn names_meet_in(&self, other: &Footprint, folder: &Path) -> bool {
        match (self.names.get(folder), other.names.get(folder)) {
            (Some(mine), Some(theirs)) => mine.meets(theirs),
            _ => false,
        }
    }

    /// Whether `self` and `other` meet whatever their paths: one cannot
    /// say what it touches, or keeps whole a store the other keeps.
    fn meets_whole(&self, other: &Footprint) -> bool {
        let whole = |a: &Footprint, b: &Footprint| {
            a.stores
                .iter()
                .any(|store| b.stores.contains(store) || b.records.contains(store))
        };
        self.every || other.every || whole(self, other) || whole(other, self)
    }

    /// How many claims it holds: the trees it reads and writes, the
    /// folders it names in and the names and beginnings of names there.
    fn claims(&self) -> usize {
        self.reads.len() + self.writes.len() + self.names.values().map(Names::claims).sum::<usize>()
    }

    /// The same, or one touching more, of at most `most` claims where
    /// widening can make it so: each folder's names first any name there,
    /// then each such folder written whole (which meets all its names
    /// meet), then trees taken for their folders, the deepest first. It
    /// meets everything `self` meets, and more: what the history keeps
    /// ([`Kept`]) is taken back alone a little less often, never wrongly.
    #[must_use]
    pub fn bounded(&self, most: usize) -> Footprint {
        let mut kept = self.clone();
        if kept.every || kept.claims() <= most {
            return kept;
        }
        for names in kept.names.values_mut() {
            *names = Names::Every;
        }
        if kept.claims() <= most {
            return kept;
        }
        let folders = std::mem::take(&mut kept.names);
        kept.writes.extend(folders.into_keys());
        loop {
            kept.writes = outermost(std::mem::take(&mut kept.writes));
            let writes = &kept.writes;
            let reads = outermost(std::mem::take(&mut kept.reads));
            // A tree read within one written: the written meets all it does.
            kept.reads = reads
                .into_iter()
                .filter(|read| !read.ancestors().any(|above| writes.contains(above)))
                .collect();
            if kept.reads.len() + kept.writes.len() <= most {
                return kept;
            }
            let deepest = kept
                .reads
                .iter()
                .chain(&kept.writes)
                .map(|path| path.components().count())
                .max()
                .unwrap_or(0);
            let mut widened = false;
            let mut up = |set: BTreeSet<PathBuf>| -> BTreeSet<PathBuf> {
                set.into_iter()
                    .map(|path| match path.parent() {
                        Some(folder) if path.components().count() == deepest => {
                            widened = true;
                            folder.to_path_buf()
                        }
                        _ => path,
                    })
                    .collect()
            };
            kept.reads = up(std::mem::take(&mut kept.reads));
            kept.writes = up(std::mem::take(&mut kept.writes));
            // Only tops left (two drives' roots): it cannot widen further.
            if !widened {
                return kept;
            }
        }
    }
}

/// `set` without the paths within another of it: those follow it in the
/// set's order, by their components.
fn outermost(set: BTreeSet<PathBuf>) -> BTreeSet<PathBuf> {
    let mut out: BTreeSet<PathBuf> = BTreeSet::new();
    let mut last: Option<PathBuf> = None;
    for path in set {
        if last.as_ref().is_some_and(|above| path.starts_with(above)) {
            continue;
        }
        last = Some(path.clone());
        out.insert(path);
    }
    out
}

/// The most claims a footprint the history keeps holds ([`Kept`]): each
/// entry's is compared with every other's on its stack as it joins
/// (ADR-079, 13), `JOURNAL_ENTRIES` at most, so each comparison stays
/// bounded however large the action.
pub const KEPT_CLAIMS: usize = 1_000;

/// What an action touched, as the history keeps it (ADR-079, 13): its
/// footprint bounded to [`KEPT_CLAIMS`], the folder every path of it is
/// in, and the names right below that folder its paths go through, so
/// that two in folders apart, or through different names of one folder,
/// are told apart at once: an action in one folder (files renamed, put in
/// the trash or copied there) by the names it makes there, compared as
/// names, and the names right below it the rest goes through.
#[derive(Clone, Debug, Default)]
pub struct Kept {
    footprint: Arc<Footprint>,
    within: Within,
    /// The names right below a folder at or below `within` its paths below
    /// it go through, by folder, each worked out once it is asked
    /// (`Kept::heads_at`): a copy of a hundred thousand files compared with
    /// two hundred entries in the folder it copies into reads its names
    /// there once. `None` for a folder it reads or writes itself, which
    /// meets everything below it. The names it makes or changes in that
    /// folder are not among them: they are compared as names
    /// ([`Names::meets`]).
    heads: Arc<std::sync::Mutex<std::collections::HashMap<PathBuf, Option<Arc<Heads>>>>>,
}

/// The names right below a folder that a footprint's paths go through, by
/// [`head_key`]: two that share none go through different names there;
/// two names taken for one only send them to the whole comparison.
type Heads = std::collections::HashSet<u64>;

/// What a name right below a folder is kept as among [`Heads`]: its hash,
/// one hasher's for all, with no name copied.
fn head_key(name: &OsStr) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    name.as_encoded_bytes().hash(&mut hasher);
    hasher.finish()
}

/// Where every path a footprint touches is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum Within {
    /// It touches no path (a store alone, nothing, or everything).
    #[default]
    Nothing,
    /// Each is at or below this folder.
    Under(PathBuf),
    /// They share no folder (two drives).
    Anywhere,
}

impl Kept {
    /// What the history keeps of `footprint`: bounded to [`KEPT_CLAIMS`].
    pub fn new(footprint: &Footprint) -> Kept {
        Kept::of(Arc::new(footprint.bounded(KEPT_CLAIMS)))
    }

    /// `footprint` whole, with the folder its paths are in, the Arc
    /// shared: what joins the history, runs or waits is compared so with
    /// the entries before it, which keep theirs bounded (ADR-079, 13). A
    /// kept footprint only ever widens its own entry's.
    pub fn exact(footprint: &Arc<Footprint>) -> Kept {
        Kept::of(Arc::clone(footprint))
    }

    /// What the history keeps of `exact` ([`Kept::new`]): `exact` itself,
    /// shared, where it fits the bound.
    pub fn bound(exact: &Arc<Kept>) -> Arc<Kept> {
        if exact.footprint.every || exact.footprint.claims() <= KEPT_CLAIMS {
            Arc::clone(exact)
        } else {
            Arc::new(Kept::new(&exact.footprint))
        }
    }

    /// `footprint` as it is, with the folder its paths are in: the one
    /// the first and the last of each of its sets share, as every path
    /// between them in their order does.
    fn of(footprint: Arc<Footprint>) -> Kept {
        fn ends<'a>(
            mut set: impl DoubleEndedIterator<Item = &'a PathBuf>,
        ) -> [Option<&'a PathBuf>; 2] {
            let first = set.next();
            [first, set.next_back().or(first)]
        }
        let within = {
            let mut paths = ends(footprint.reads.iter())
                .into_iter()
                .chain(ends(footprint.writes.iter()))
                .chain(ends(footprint.names.keys()))
                .flatten();
            match paths.next() {
                None => Within::Nothing,
                Some(first) => {
                    let mut shared = first.components().count();
                    for path in paths {
                        shared = first
                            .components()
                            .zip(path.components())
                            .take(shared)
                            .take_while(|(a, b)| a == b)
                            .count();
                    }
                    if shared == 0 {
                        Within::Anywhere
                    } else {
                        Within::Under(first.components().take(shared).collect())
                    }
                }
            }
        };
        Kept {
            footprint,
            within,
            heads: Arc::default(),
        }
    }

    /// The names right below `folder`, at or below its own folder, its
    /// paths below it go through ([`Footprint::heads_at`]): worked out
    /// once for each folder asked.
    fn heads_at(&self, folder: &Path) -> Option<Arc<Heads>> {
        let mut heads = self
            .heads
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(known) = heads.get(folder) {
            return known.clone();
        }
        let found = self.footprint.heads_at(folder).map(Arc::new);
        heads.insert(folder.to_path_buf(), found.clone());
        found
    }

    pub fn footprint(&self) -> &Footprint {
        &self.footprint
    }

    /// Whether `self` and `other` touched something in common, as
    /// [`Footprint::conflicts`] tells. Two paths meet only where one is
    /// at or below the other: so two whose paths are in folders apart
    /// (neither within the other), or whose paths below the deeper one's
    /// folder go through different names right below it, read or write
    /// nothing at or above it and make no name there the other makes,
    /// meet only by a store, or where one cannot say what it touches.
    pub fn conflicts(&self, other: &Kept) -> bool {
        let apart = match (&self.within, &other.within) {
            (Within::Under(a), Within::Under(b)) if b.starts_with(a) => self.apart_from(other, b),
            (Within::Under(a), Within::Under(b)) if a.starts_with(b) => other.apart_from(self, a),
            // One touches no path, or theirs are in folders apart.
            (Within::Nothing, _) | (_, Within::Nothing) | (Within::Under(_), Within::Under(_)) => {
                true
            }
            _ => false,
        };
        if apart {
            return self.footprint.meets_whole(&other.footprint);
        }
        self.footprint.conflicts(&other.footprint)
    }

    /// Whether `self`, whose folder is at or above `inner`, the folder
    /// all of `deeper`'s paths are in, and `deeper` meet by no path: it
    /// reads and writes nothing at or above `inner`, makes no name there
    /// that `deeper` makes, and its paths below `inner` go through other
    /// names right below it than `deeper`'s, which reads and writes
    /// nothing at `inner` itself. A name made in a folder above `inner`
    /// meets nothing below it.
    fn apart_from(&self, deeper: &Kept, inner: &Path) -> bool {
        let Some(theirs) = deeper.heads_at(inner) else {
            return false;
        };
        let at_or_above = inner.ancestors().any(|above| {
            self.footprint.reads.contains(above) || self.footprint.writes.contains(above)
        });
        if at_or_above || self.footprint.names_meet_in(&deeper.footprint, inner) {
            return false;
        }
        let Some(mine) = self.heads_at(inner) else {
            return false;
        };
        let (few, many) = if mine.len() <= theirs.len() {
            (&*mine, &*theirs)
        } else {
            (&*theirs, &*mine)
        };
        !few.iter().any(|head| many.contains(head))
    }
}

/// Whether `set` holds `tree`, a tree above it, or one within it.
fn meets(set: &BTreeSet<PathBuf>, tree: &Path) -> bool {
    within(set, tree) || tree.ancestors().skip(1).any(|above| set.contains(above))
}

/// Whether `names` holds the folder `tree` or one within it, as [`within`].
fn folder_within(names: &BTreeMap<PathBuf, Names>, tree: &Path) -> bool {
    names
        .range::<Path, _>((Bound::Included(tree), Bound::Unbounded))
        .next()
        .is_some_and(|(first, _)| first.starts_with(tree))
}

/// Whether `set` holds `tree` or a path within it: those follow it in the
/// set's order, by their components.
fn within(set: &BTreeSet<PathBuf>, tree: &Path) -> bool {
    set.range::<Path, _>((Bound::Included(tree), Bound::Unbounded))
        .next()
        .is_some_and(|first| first.starts_with(tree))
}

/// What a footprint asks of the volumes, read once for an action's steps:
/// which volume holds a path (its mount, by the mount table), whether a
/// path on this machine is a folder, and where its companions would be.
pub struct Volumes {
    /// Mount points, the longest first.
    points: Option<Vec<PathBuf>>,
    /// Those of volumes kept in memory (tmpfs, ramfs): on no disk.
    in_memory: Vec<PathBuf>,
    /// The companions' rules (ADR-028) the plan is applied with.
    companions: Option<std::sync::Arc<crate::companions::Companions>>,
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
                companions: None,
            };
        }
        Volumes {
            points: None,
            in_memory: Vec::new(),
            companions: None,
        }
    }

    /// The same, the companions' places as `companions` has them.
    #[must_use]
    pub fn with_companions(
        mut self,
        companions: std::sync::Arc<crate::companions::Companions>,
    ) -> Self {
        self.companions = Some(companions);
        self
    }

    /// Where the companions of the file at `path` would be, by every rule:
    /// names, none looked at.
    pub fn beside(&self, path: &Path) -> Vec<PathBuf> {
        let (Some(rules), Some(dir), Some(name)) =
            (&self.companions, path.parent(), path.file_name())
        else {
            return Vec::new();
        };
        rules
            .of(name)
            .into_iter()
            .map(|place| dir.join(place))
            .collect()
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

    /// What goes with a file is claimed with it (ADR-028): a rename of
    /// one meets what writes beside it in a companions' folder of its own.
    #[test]
    fn a_file_s_companions_are_claimed_with_it() {
        let rules = crate::companions::Companions::default();
        rules.register("test", Arc::new(crate::companions::tests::Rules));
        let volumes = Volumes::now().with_companions(Arc::new(rules));
        let rename = Footprint::default()
            .made_with(&p("/disk/notes/it’s a.txt"), &volumes)
            .made_with(&p("/disk/notes/b.txt"), &volumes);
        let note = Footprint::default().written(&p("/disk/notes/.notes/it’s a.txt.note"));
        assert!(rename.conflicts(&note));
        let without = Footprint::default()
            .made(&p("/disk/notes/it’s a.txt"))
            .made(&p("/disk/notes/b.txt"));
        assert!(!without.conflicts(&note), "what it said before");
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
        // Into the folder the copy makes names in: another name, beside
        // it, no matter; the same name meets, its case aside (a stick's
        // exFAT takes one for the other).
        let into = Footprint::default().made(&p("/stick/other.txt"));
        assert!(!copy.conflicts(&into) && !into.conflicts(&copy));
        let same = Footprint::default().made(&p("/stick/Photos"));
        assert!(copy.conflicts(&same) && same.conflicts(&copy));
        // A name settled only when its turn comes claims the names it may
        // be kept as beside one taken, and the staging folders made there.
        let settles = Footprint::default()
            .read(&p("/disk/photos"))
            .settles_name(&p("/stick/photos"), true);
        assert!(!settles.conflicts(&into) && settles.conflicts(&same));
        for kept in [
            "/stick/photos (copy)",
            "/stick/Photos (Copy 2)",
            "/stick/photos (2)",
        ] {
            let kept = Footprint::default().made(&p(kept));
            assert!(
                settles.conflicts(&kept) && kept.conflicts(&settles),
                "{kept:?}"
            );
        }
        let staging = Footprint::default().made(&p("/stick/.skwir-copy-it's AbC1"));
        assert!(settles.conflicts(&staging) && !copy.conflicts(&staging));
        let photos2 = Footprint::default().made(&p("/stick/photos2"));
        assert!(!settles.conflicts(&photos2));
        // Two settled later meet where their kept names may: one's prefix
        // begins the other's.
        let other = Footprint::default().settles_name(&p("/stick/photos (copy).txt"), false);
        assert!(settles.conflicts(&other) && other.conflicts(&settles));
        let apart = Footprint::default().settles_name(&p("/stick/videos"), false);
        assert!(!apart.conflicts(&Footprint::default().settles_name(&p("/stick/photos"), false)));
        // Reads never meet.
        let again = Footprint::default()
            .read(&p("/disk/photos"))
            .made(&p("/elsewhere/photos"));
        assert!(!copy.conflicts(&again));
        // A folder above the tree read, removed.
        let gone = Footprint::default().made(&p("/disk"));
        assert!(copy.conflicts(&gone));
        // One store, kept whole by one: a restore meets a trashing, an
        // item deleted for good another; two trashings, each an item of
        // its own, and two notes edits in two folders, do not meet; one
        // file's notes written twice do, by its path.
        let trash = Footprint::default().made(&p("/a")).record_in(TRASH);
        let restore = Footprint::default().made(&p("/b")).store(TRASH);
        assert!(trash.conflicts(&restore) && restore.conflicts(&trash));
        let purge = Footprint::default().made(&p("/t/files/c")).store(TRASH);
        assert!(restore.conflicts(&purge));
        let other = Footprint::default().made(&p("/d/e")).record_in(TRASH);
        assert!(!trash.conflicts(&other) && !other.conflicts(&trash));
        let tag = |path: &str| Footprint::default().made(&p(path)).record_in(NOTES);
        assert!(!tag("/disk/it’s a.jpg").conflicts(&tag("/stick/b.jpg")));
        assert!(tag("/disk/it’s a.jpg").conflicts(&tag("/disk/it’s a.jpg")));
        let mut both = tag("/stick/b.jpg");
        both.merge(Footprint::default().store(NOTES));
        assert!(both.conflicts(&tag("/disk/it’s a.jpg")), "kept whole");
        // A name like another, not within it.
        let photos2 = Footprint::default().made(&p("/disk/photos2"));
        assert!(!copy.conflicts(&photos2));
        // What cannot be told meets everything.
        assert!(Footprint::every().conflicts(&Footprint::default()));
    }

    /// Two actions of many names in one folder are compared in time
    /// linear in their names (AGENTS.md, "Scale"): fifty thousand copies
    /// each, settled later, apart and then meeting on one name.
    #[test]
    fn many_names_in_one_folder_are_compared_once_each() {
        let many = |word: &str, n: usize| {
            Footprint::of_steps((0..n).map(|i| {
                Some(
                    Footprint::default().settles_name(&p(&format!("/stick/{word} {i}.jpg")), false),
                )
            }))
        };
        let (a, b) = (many("it's a", 50_000), many("b", 50_000));
        assert!(!a.conflicts(&b) && !b.conflicts(&a));
        let c = many("it's a", 1);
        assert!(a.conflicts(&c) && c.conflicts(&b) == b.conflicts(&c));
        let kept = Footprint::default().made(&p("/stick/B 49999 (copy).jpg"));
        assert!(b.conflicts(&kept) && !a.conflicts(&kept));
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

    /// What the history keeps of a large action fits its bound, and still
    /// meets what the action met (ADR-079, 13): ten thousand files put
    /// in the trash from one folder meet a rename of one of them and a
    /// copy of the folder, and, widened, a new file beside them; never a
    /// rename elsewhere.
    #[test]
    fn a_large_action_is_kept_bounded_and_meets_what_it_met() {
        let trashed = Footprint::of_steps((0..10_000).map(|n| {
            Some(
                Footprint::default()
                    .made(&p(&format!("/disk/downloads/it's {n}.jpg")))
                    .record_in(TRASH),
            )
        }));
        let kept = Kept::new(&trashed);
        assert!(kept.footprint().claims() <= KEPT_CLAIMS);
        let one = |footprint: Footprint| Kept::new(&footprint);
        let rename = one(Footprint::default()
            .made(&p("/disk/downloads/it's 7.jpg"))
            .made(&p("/disk/downloads/b.jpg")));
        assert!(kept.conflicts(&rename) && rename.conflicts(&kept));
        // Widened, it meets a name beside them too: the folder written whole.
        let beside = one(Footprint::default().made(&p("/disk/downloads/new.txt")));
        assert!(!trashed.conflicts(beside.footprint()) && kept.conflicts(&beside));
        let copy = one(Footprint::default()
            .read(&p("/disk/downloads"))
            .made(&p("/stick/downloads")));
        assert!(kept.conflicts(&copy));
        let elsewhere = one(Footprint::default()
            .made(&p("/disk/notes/a.txt"))
            .made(&p("/disk/notes/b.txt")));
        assert!(!kept.conflicts(&elsewhere) && !elsewhere.conflicts(&kept));
        // A store kept whole meets it whatever the folders.
        let emptied = one(Footprint::default().store(TRASH));
        assert!(kept.conflicts(&emptied));
        assert!(one(Footprint::every()).conflicts(&elsewhere));
    }

    /// Paths of two drives share no folder: no quick answer, the whole
    /// comparison is made.
    #[test]
    fn paths_under_no_one_folder_are_compared_whole() {
        let a = Kept::new(&Footprint::default().made(&p("x/a")).made(&p("y/b")));
        let b = Kept::new(&Footprint::default().made(&p("y/b")));
        assert_eq!(a.within, Within::Anywhere);
        assert!(a.conflicts(&b) && b.conflicts(&a));
        let c = Kept::new(&Footprint::default().made(&p("z/c")));
        assert!(!a.conflicts(&c));
    }

    /// Actions in one folder (files put in the trash there) are told apart
    /// by the names they make there, compared as names: one making a name
    /// another made, in another case, meets it; one reading the folder
    /// whole meets them all.
    #[test]
    fn actions_in_one_folder_are_told_apart_by_their_names() {
        let trash = |names: &[&str]| {
            Kept::new(&Footprint::of_steps(names.iter().map(|name| {
                Some(
                    Footprint::default()
                        .made(&p(&format!("/disk/photos/{name}")))
                        .record_in(TRASH),
                )
            })))
        };
        let a = trash(&["it's a.jpg", "b.jpg"]);
        let b = trash(&["c.jpg"]);
        assert!(
            a.heads_at(Path::new("/disk/photos")).is_some(),
            "the folder named, not read or written"
        );
        assert!(!a.conflicts(&b) && !b.conflicts(&a));
        let again = trash(&["B.JPG"]);
        assert!(a.conflicts(&again) && again.conflicts(&a));
        let copy = Kept::new(
            &Footprint::default()
                .read(&p("/disk/photos"))
                .made(&p("/stick/photos")),
        );
        assert!(copy.conflicts(&a) && a.conflicts(&copy));
        // Inside a folder made there: through the name it is made as.
        let inside = Kept::new(&Footprint::default().made(&p("/disk/photos/c.jpg/x")));
        assert!(!a.conflicts(&inside) && b.conflicts(&inside));
    }

    mod widening {
        use super::*;
        use proptest::prelude::*;

        /// A footprint of a few claims among a few folders and names.
        fn footprint() -> impl Strategy<Value = Footprint> {
            // Names in two cases: a volume that tells none apart takes
            // them for one.
            let path = (0..3usize, 0..3usize, 0..4usize, any::<bool>()).prop_map(
                |(a, b, n, upper)| match n {
                    0 => p(&format!("/d{a}")),
                    1 => p(&format!("/d{a}/e{b}")),
                    _ if upper => p(&format!("/d{a}/e{b}/IT'S {n}")),
                    _ => p(&format!("/d{a}/e{b}/it's {n}")),
                },
            );
            let claim = (0..5usize, path).prop_map(|(kind, path)| match kind {
                0 => Footprint::default().read(&path),
                1 => Footprint::default().written(&path),
                2 => Footprint::default().settles_name(&path, true),
                3 => Footprint::default().store(TRASH),
                _ => Footprint::default().made(&path),
            });
            prop::collection::vec(claim, 1..8)
                .prop_map(|claims| Footprint::of_steps(claims.into_iter().map(Some)))
        }

        proptest! {
            /// Widened to any bound, a footprint meets everything it met,
            /// and fits that bound where it can; kept with its folder, it
            /// answers as the whole comparison does.
            #[test]
            fn widening_never_lets_through_what_met(
                a in footprint(),
                b in footprint(),
                most in 0..12usize,
            ) {
                let wide = a.bounded(most);
                if a.conflicts(&b) {
                    prop_assert!(wide.conflicts(&b) && b.conflicts(&wide));
                }
                if !a.is_every() {
                    // Tops alone cannot widen: one per drive, `/` here.
                    prop_assert!(wide.claims() <= most.max(1));
                }
                let (ka, kb) = (
                    Kept::of(Arc::new(wide.clone())),
                    Kept::exact(&Arc::new(b.clone())),
                );
                prop_assert_eq!(ka.conflicts(&kb), wide.conflicts(&b));
                prop_assert_eq!(kb.conflicts(&ka), b.conflicts(&wide));
            }

            /// Looked up from the one of fewer claims, two footprints meet
            /// as each one's claims compared with the other's both ways do.
            #[test]
            fn the_fewer_claims_looked_up_answer_as_both_ways(
                a in footprint(),
                b in footprint(),
            ) {
                prop_assert_eq!(a.conflicts(&b), both_ways(&a, &b));
                prop_assert_eq!(b.conflicts(&a), both_ways(&a, &b));
            }
        }

        /// The comparison as it was made, each one's writes and names
        /// against all the other touches, both ways.
        fn both_ways(a: &Footprint, b: &Footprint) -> bool {
            let changes = |a: &Footprint, b: &Footprint| {
                a.writes.iter().any(|tree| {
                    meets(&b.reads, tree) || meets(&b.writes, tree) || folder_within(&b.names, tree)
                }) || a.names.iter().any(|(folder, names)| {
                    b.names
                        .get(folder)
                        .is_some_and(|theirs| names.meets(theirs))
                        || folder
                            .ancestors()
                            .any(|above| b.reads.contains(above) || b.writes.contains(above))
                })
            };
            a.meets_whole(b) || changes(a, b) || changes(b, a)
        }
    }

    /// Kept prefixes meet the names they may be kept as whichever side is
    /// larger: a copy of many files, its names settled later, meets a
    /// name one of them may take, and no other.
    #[test]
    fn kept_prefixes_meet_names_looked_up_either_way() {
        let copy = Footprint::of_steps((0..50).map(|n| {
            Some(Footprint::default().settles_name(&p(&format!("/disk/photos/it's {n}.jpg")), true))
        }));
        let taken = Footprint::default().made(&p("/disk/photos/IT'S 7 (copy).jpg"));
        let other = Footprint::default().made(&p("/disk/photos/notes.txt"));
        assert!(copy.conflicts(&taken) && taken.conflicts(&copy));
        assert!(!copy.conflicts(&other) && !other.conflicts(&copy));
        // Names too long for their prefix to end with ` (`: cut.
        let long = "x".repeat(300);
        let cut = Footprint::of_steps((0..5).map(|n| {
            Some(Footprint::default().settles_name(&p(&format!("/disk/photos/{n}{long}")), true))
        }));
        let beside = Footprint::default().made(&p(&format!("/disk/photos/3{long} (copy 2)")));
        assert!(cut.conflicts(&beside) && beside.conflicts(&cut));
        let apart = Footprint::default().made(&p(&format!("/disk/photos/7{long} (copy 2)")));
        assert!(!cut.conflicts(&apart));
    }
}
