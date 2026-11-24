//! Breakdowns from a names index's folder totals (ADR-150, D3).
//!
//! Where an index follows the folder measured and holds its sizes and
//! times live (skwird's: `names::Session::totals`), Usage asks it what each
//! folder holds rather than reading the disk.
//!
//! - **What it does not count is scanned:** a folder below that the index
//!   keeps out or has not read (a Secrets folder, a private one), on the
//!   folder's own device, is read by Usage's own scan and added in, so
//!   nothing goes missing. The folders each answer says it did not count
//!   are those (`Summed::coverage`'s holes). Mounts, network mounts and
//!   snapshots are not: Usage's scan does not enter them either.
//! - **What caches hold** (`reclaimable`) is found through the index too:
//!   the folders holding a signed `CACHEDIR.TAG` and the trashes, asked by
//!   name once per measure, each one's room summed when a breakdown needs
//!   it. A cache inside a cache is one too, as the scan finds it, and what
//!   a scan adds inside one is room it gives back (`add_hole`).
//! - **A figure means what the scan's does:** a file of several names
//!   says how many (its stat's, read where its totals say it has names
//!   elsewhere); an answer whose counts of names are not all known is not
//!   taken (the scan reads that folder).
//! - **Kept current by its journal:** a breakdown taken from it is kept
//!   with the journal that tells its changes (`Cache::insert_journaled`),
//!   followed while anything kept needs it (`Worker::journals`), where
//!   Usage's own scan keeps each folder current by a watch of its own. A
//!   scan of the disk never counts with one (`Cache::counted`).
//!
//! Elsewhere (no index, sizes not live, an index that cannot sum), the
//! scan reads the disk as before.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use skwir_core::app::App;
use skwir_core::cancel::CancelToken;
use skwir_core::coverage::{Proof, Why};
use skwir_core::entry::{EntryId, EntryKind, FileId};
use skwir_core::names::{Child, Largest, Mark, NameTerm, Query, Session, Summed, Totals};

use super::super::Node;
use super::marks::{self, Kinds, Places, Reclaim, Types};
use super::{
    MAX_CHILDREN, MAX_CROWDED, add_totals, below, file_of, folder_entry, meta, own_time,
    reclaim_whole,
};

/// The entries a breakdown is sent: those Usage keeps (`trim`), the
/// heaviest and those with the most files, the rest summed.
const LARGEST: Largest = Largest {
    alloc: MAX_CHILDREN as u64,
    files: MAX_CROWDED as u64,
};

/// A names index followed for one measure.
pub(in crate::worker) struct Indexed {
    session: Box<dyn Session>,
    /// The folder measured, its real path.
    root: PathBuf,
    /// Where its journal stood as the first breakdown was summed: what
    /// changed since says which to sum again. Kept with every breakdown
    /// taken (`Cache::insert_journaled`).
    mark: Option<Mark>,
    /// The folders below the root it does not cover, as it said when
    /// opened: one its journal names beside them is a folder whose changes
    /// it tells no more.
    not_covered: BTreeSet<PathBuf>,
    /// The folders below the root whose room can be had back whole (a
    /// cache, a trash), each with what it is and the bytes it holds once
    /// asked; found once a measure.
    reclaims: Option<BTreeMap<PathBuf, (Reclaim, Option<u64>)>>,
    cancel: CancelToken,
}

/// A folder's breakdown from the index, and the folders in it the index
/// did not count that the scan reads (`add_hole`), outermost.
pub(in crate::worker) struct Breakdown {
    pub(in crate::worker) node: Node,
    pub(in crate::worker) holes: Vec<PathBuf>,
}

/// Whether Usage's scan reads what a hole of `why` holds: what is on the
/// folder's own device. A mount, a network mount and a snapshot (another
/// device, a btrfs subvolume's) it does not enter either.
fn scanned(why: Why) -> bool {
    !matches!(why, Why::Mount | Why::NetworkMount | Why::Snapshot)
}

/// Seconds since 1970, as a node says a time; before, negative.
fn secs(t: SystemTime) -> i64 {
    match t.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(after) => i64::try_from(after.as_secs()).unwrap_or(i64::MAX),
        Err(before) => -i64::try_from(before.duration().as_secs()).unwrap_or(i64::MAX),
    }
}

/// A folder's time as Usage dates it: its newest file's, else its own
/// (`own_time`); 0 when neither is known.
fn dated(totals: &Totals, own: Option<SystemTime>) -> i64 {
    totals.newest_file.or(own).map_or(0, secs)
}

/// Its files' bytes by kind, each extension typed as a name with it is.
fn kinds_of(totals: &Totals, types: &mut Types) -> Kinds {
    let mut kinds = Kinds::default();
    for e in &totals.by_extension {
        let kind = e.ext.as_ref().and_then(|ext| {
            let mut name = OsString::from("x.");
            name.push(ext);
            types.of(&name)
        });
        kinds.add(kind, e.alloc);
    }
    kinds
}

/// A file's count of names, as its stat says it (the scan's `nlink`):
/// one, unless its totals say it has names elsewhere (`shared`, which a
/// file of several names that takes no room cannot say), read then. An
/// index's entries carry no count of their own.
fn names_of(child: &Child) -> u64 {
    if child.totals.shared == 0 {
        return 1;
    }
    std::fs::symlink_metadata(&child.path).map_or(0, |m| meta::links(&m))
}

/// The outermost of `paths` (sorted): none inside another.
fn outermost<'a>(paths: impl Iterator<Item = &'a PathBuf>) -> Vec<PathBuf> {
    let mut kept: Vec<PathBuf> = Vec::new();
    for path in paths {
        if !kept.last().is_some_and(|above| path.starts_with(above)) {
            kept.push(path.clone());
        }
    }
    kept
}

impl Indexed {
    /// The index that follows `root` (a real path) with its sizes and times
    /// live, nothing started; `None` where none does.
    pub(in crate::worker) fn open(app: &App, root: &Path, cancel: &CancelToken) -> Option<Indexed> {
        let (_, opened) = app.names.follow(root, None, cancel)?;
        let coverage = &opened.coverage;
        if !matches!(coverage.attrs, Proof::Live { .. }) || !coverage.names.proves() {
            return None;
        }
        let not_covered: BTreeSet<PathBuf> = coverage
            .holes
            .iter()
            .filter(|h| h.path.starts_with(root))
            .map(|h| h.path.clone())
            .collect();
        // The root itself not counted: nothing to ask.
        if not_covered.contains(root) {
            return None;
        }
        Some(Indexed {
            session: opened.session,
            root: root.to_path_buf(),
            mark: None,
            not_covered,
            reclaims: None,
            cancel: cancel.clone(),
        })
    }

    /// The folder it is followed at, where its journal is asked.
    pub(in crate::worker) fn root(&self) -> &Path {
        &self.root
    }

    /// Where its journal stood as it first summed, and the folders it
    /// did not cover then: what keeps its breakdowns current. `None` until
    /// a breakdown is summed.
    pub(in crate::worker) fn journal(&self) -> Option<(&Mark, &BTreeSet<PathBuf>)> {
        self.mark.as_ref().map(|mark| (mark, &self.not_covered))
    }

    /// What `folder` (at or below the root) holds, as Usage keeps a
    /// breakdown: its entries, the heaviest and those with the most files,
    /// the others among its other items; and the folders in it the index
    /// did not count, which the scan reads and adds (`add_hole`). `None`
    /// where the index does not sum it as the scan would (its sizes not
    /// live, or a file's count of names not known, so that what deleting
    /// frees is only at least what it says): the scan reads it then.
    pub(in crate::worker) fn breakdown(
        &mut self,
        folder: &Path,
        types: &mut Types,
        places: &Places,
    ) -> Option<Breakdown> {
        let meta = std::fs::metadata(folder).ok()?;
        let mut children: Vec<Child> = Vec::new();
        let cancel = self.cancel.clone();
        let summed: Summed = self.session.totals(folder, Some(LARGEST), &mut |batch| {
            children.extend(batch);
            !cancel.is_cancelled()
        })?;
        if !matches!(summed.coverage.attrs, Proof::Live { .. })
            || !summed.links_known
            || cancel.is_cancelled()
        {
            return None;
        }
        if self.mark.is_none() {
            self.mark = Some(summed.mark.clone());
        }
        self.read_reclaims(places);
        let mut holes: Vec<&PathBuf> = summed
            .coverage
            .holes
            .iter()
            .filter(|h| scanned(h.why) && h.path.starts_with(folder) && h.path != folder)
            .map(|h| &h.path)
            .collect();
        holes.sort();
        let holes = outermost(holes.into_iter());
        let folder_file = file_of(&meta);
        let mut node = Node {
            id: Some(folder_entry(folder, &meta)),
            path: folder.to_path_buf(),
            name: folder.file_name().map_or_else(
                || folder.to_string_lossy().into_owned(),
                |n| n.to_string_lossy().into_owned(),
            ),
            is_dir: true,
            size: summed.totals.size,
            alloc: summed.totals.alloc,
            // Usage counts names, as a listing shows them.
            files: summed.totals.names,
            dirs: summed.totals.folders,
            modified: dated(&summed.totals, summed.mtime),
            kinds: kinds_of(&summed.totals, types),
            shared: summed.totals.shared,
            reclaim: self.reclaim_of(folder, places),
            reclaimable: self.reclaimable_below(folder),
            ..Node::default()
        };
        for child in children {
            let Some(name) = child.path.file_name().map(OsString::from) else {
                continue;
            };
            let is_dir = child.kind == EntryKind::Directory;
            let file = child.id.map_or(FileId::new(0, 0), |(dev, ino)| {
                skwir_core::fs::stat::file(dev, ino)
            });
            let (reclaim, reclaimable) = if is_dir {
                (
                    self.reclaim_of(&child.path, places),
                    self.reclaimable_below(&child.path),
                )
            } else {
                (None, 0)
            };
            let t = &child.totals;
            let mut entry = Node {
                id: Some(EntryId::in_folder(file, folder_file, &name)),
                name: name.to_string_lossy().into_owned(),
                is_dir,
                size: t.size,
                alloc: t.alloc,
                files: if is_dir { t.names } else { 1 },
                dirs: t.folders,
                modified: if is_dir {
                    dated(t, child.mtime)
                } else {
                    child.mtime.map_or(0, secs)
                },
                kind: if is_dir { None } else { types.of(&name) },
                entry_kind: (!is_dir).then_some(child.kind),
                kinds: if is_dir {
                    kinds_of(t, types)
                } else {
                    Kinds::default()
                },
                nlink: if is_dir { 0 } else { names_of(&child) },
                shared: if is_dir { t.shared } else { 0 },
                reclaim,
                reclaimable,
                path: child.path,
                ..Node::default()
            };
            if is_dir {
                reclaim_whole(&mut entry);
            }
            node.children.push(entry);
        }
        if let Some((count, rest)) = &summed.rest {
            node.other_count = *count;
            node.other_alloc = rest.alloc;
            node.other_size = rest.size;
            node.other_files = rest.names;
            node.other_modified = rest.newest_file.map_or(0, secs);
        }
        own_time(&mut node, summed.mtime.map_or(0, secs));
        Some(Breakdown { node, holes })
    }

    /// Whether the room `path` takes can be had back whole: a place known
    /// by its path, or a cache or a trash the index named.
    fn reclaim_of(&self, path: &Path, places: &Places) -> Option<Reclaim> {
        places.of(path).or_else(|| {
            self.reclaims
                .as_ref()?
                .get(path)
                .map(|(reclaim, _)| *reclaim)
        })
    }

    /// Whether a cache or a trash at `level` or below it holds `hole`
    /// (below `level`): its room, which the index summed without what the
    /// hole holds, is the hole's too.
    pub(in crate::worker) fn reclaimed(&self, level: &Path, hole: &Path, places: &Places) -> bool {
        hole.ancestors()
            .take_while(|folder| folder.starts_with(level))
            .any(|folder| self.reclaim_of(folder, places).is_some())
    }

    /// What the caches and trashes strictly below `path` hold: the
    /// outermost of them, each one's room asked once. Ranges of the
    /// reclaims found, never a pass over all of them.
    fn reclaimable_below(&mut self, path: &Path) -> u64 {
        let Some(reclaims) = &self.reclaims else {
            return 0;
        };
        let inside = outermost(below(reclaims, path).filter(|p| p.as_path() != path));
        inside.iter().map(|folder| self.room_of(folder)).sum()
    }

    /// The bytes the cache or trash `folder` holds, as the index sums
    /// them, asked once.
    fn room_of(&mut self, folder: &Path) -> u64 {
        if let Some((_, Some(alloc))) = self.reclaims.as_ref().and_then(|r| r.get(folder)) {
            return *alloc;
        }
        let alloc = if self.cancel.is_cancelled() {
            0
        } else {
            self.session
                .totals(folder, Some(Largest { alloc: 0, files: 0 }), &mut |_| true)
                .filter(|s| matches!(s.coverage.attrs, Proof::Live { .. }))
                .map_or(0, |s| s.totals.alloc)
        };
        if let Some((_, room)) = self.reclaims.as_mut().and_then(|r| r.get_mut(folder)) {
            *room = Some(alloc);
        }
        alloc
    }

    /// The folders below the root whose room can be had back whole, asked
    /// of the index once a measure: the places known by their path (the
    /// cache folder, the home trash), a folder holding a signed
    /// `CACHEDIR.TAG`, a trash named as one; inner ones too (a cache in
    /// the cache folder), as the scan finds them. Their rooms are asked
    /// as breakdowns need them (`room_of`).
    fn read_reclaims(&mut self, places: &Places) {
        if self.reclaims.is_some() {
            return;
        }
        let root = self.root.clone();
        let mut found: BTreeMap<PathBuf, (Reclaim, Option<u64>)> = places
            .paths()
            .filter(|(p, _)| p.starts_with(&root) && *p != root)
            .map(|(p, r)| (p.to_path_buf(), (r, None)))
            .collect();
        for tag in self.named(marks::CACHEDIR_TAG, EntryKind::File) {
            if tag.file_name() == Some(marks::CACHEDIR_TAG.as_ref())
                && marks::tags_a_cache(&tag)
                && let Some(folder) = tag.parent()
            {
                found
                    .entry(folder.to_path_buf())
                    .or_insert((Reclaim::Cache, None));
            }
        }
        for trash in self.named(".Trash", EntryKind::Directory) {
            let mut candidates = vec![trash.clone()];
            // A volume's `.Trash` holds one per user, named by their id.
            if let Ok(inside) = std::fs::read_dir(&trash) {
                candidates.extend(inside.flatten().map(|e| e.path()));
            }
            for candidate in candidates {
                if places.of(&candidate) == Some(Reclaim::Trash) {
                    found.entry(candidate).or_insert((Reclaim::Trash, None));
                }
            }
        }
        self.reclaims = Some(found);
    }

    /// The entries below the root whose name holds `name`, of `kind`, as
    /// the index names them; none where it cannot say.
    fn named(&mut self, name: &str, kind: EntryKind) -> Vec<PathBuf> {
        let q = Query {
            root: self.root.clone(),
            names: vec![NameTerm {
                text: name.to_owned(),
                case_sensitive: false,
            }],
            kinds: Some(vec![kind]),
            size: None,
            modified: None,
            whole: true,
        };
        let mut paths = Vec::new();
        let cancel = self.cancel.clone();
        self.session.query(&q, &mut |batch| {
            paths.extend(batch);
            !cancel.is_cancelled()
        });
        paths
    }
}

/// What a scan of `hole`, a folder below `node`'s the index did not
/// count, adds: to the folder's totals, and to the entry holding it (the
/// entry itself where it is one, which the index counted nothing in), or
/// to its other items where that entry is among them. Where a cache or a
/// trash holds it (`reclaimed`, asked of each level: one at that level or
/// below it), all it takes is room given back there, as the scan counts a
/// cache whole; elsewhere, what caches inside it hold.
pub(in crate::worker) fn add_hole(node: &mut Node, hole: Node, reclaimed: &dyn Fn(&Path) -> bool) {
    let more = |level: &Path| {
        if reclaimed(level) {
            hole.alloc.saturating_sub(hole.reclaimable)
        } else {
            0
        }
    };
    add_totals(node, &hole);
    node.reclaimable += more(&node.path);
    match node
        .children
        .iter()
        .position(|c| hole.path.starts_with(&c.path))
    {
        Some(at) if node.children[at].path == hole.path => {
            let id = node.children[at].id;
            node.children[at] = Node {
                id,
                children: Vec::new(),
                links: Arc::default(),
                ..hole
            };
        }
        Some(at) => {
            let extra = more(&node.children[at].path);
            let entry = &mut node.children[at];
            add_totals(entry, &hole);
            entry.reclaimable += extra;
        }
        None if hole.path.parent() == Some(node.path.as_path()) => {
            // Summed among the other items, with nothing in it.
            node.other_count = node.other_count.saturating_sub(1);
            node.children.push(Node {
                children: Vec::new(),
                links: Arc::default(),
                ..hole
            });
        }
        None => {
            node.other_alloc += hole.alloc;
            node.other_size += hole.size;
            node.other_files += hole.files;
            node.other_modified = node.other_modified.max(hole.modified);
        }
    }
}
