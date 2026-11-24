//! Breakdowns from a names index's folder totals (ADR-150, D3).
//!
//! Where an index follows the folder measured and holds its sizes and
//! times live (skwird's: `names::Session::totals`), Usage asks it what each
//! folder holds rather than reading the disk.
//!
//! - **What it does not count is scanned:** a folder below that the index
//!   keeps out or has not read (a Secrets folder, a private one), on the
//!   folder's own device, is read by Usage's own scan and added in, so
//!   nothing goes missing. Mounts, network mounts and snapshots are not:
//!   Usage's scan does not enter them either.
//! - **What caches hold** (`reclaimable`) is found through the index too:
//!   the folders holding a signed `CACHEDIR.TAG` and the trashes, asked by
//!   name once per measure, each one's room summed.
//! - **Kept current by its journal**, where Usage's own scan keeps each
//!   folder current by a watch of its own (the worker's `journals`).
//!
//! Elsewhere (no index, sizes not live, an index that cannot sum), the
//! scan reads the disk as before.

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
    MAX_CHILDREN, MAX_CROWDED, add_totals, file_of, folder_entry, own_time, reclaim_whole,
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
    /// changed since says which to sum again.
    pub(in crate::worker) mark: Option<Mark>,
    /// The folders below the root it does not count that a scan reads.
    holes: Vec<PathBuf>,
    /// The folders below the root whose room can be had back whole (a
    /// cache, a trash), outermost, with the bytes each holds: asked once.
    reclaims: Option<Vec<(PathBuf, Reclaim, u64)>>,
    cancel: CancelToken,
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

impl Indexed {
    /// The index that follows `root` (a real path) with its sizes and times
    /// live, nothing started; `None` where none does.
    pub(in crate::worker) fn open(app: &App, root: &Path, cancel: &CancelToken) -> Option<Indexed> {
        let (_, opened) = app.names.follow(root, None, cancel)?;
        let coverage = &opened.coverage;
        if !matches!(coverage.attrs, Proof::Live { .. }) || !coverage.names.proves() {
            return None;
        }
        let holes = coverage
            .holes
            .iter()
            .filter(|h| scanned(h.why) && h.path.starts_with(root))
            .map(|h| h.path.clone())
            .collect::<Vec<_>>();
        // The root itself not counted: nothing to ask.
        if holes.iter().any(|h| h == root) {
            return None;
        }
        Some(Indexed {
            session: opened.session,
            root: root.to_path_buf(),
            mark: None,
            holes,
            reclaims: None,
            cancel: cancel.clone(),
        })
    }

    /// The holes below `folder` a scan reads, outermost.
    pub(in crate::worker) fn holes_in(&self, folder: &Path) -> Vec<PathBuf> {
        let mut inside: Vec<PathBuf> = self
            .holes
            .iter()
            .filter(|h| h.starts_with(folder) && h.as_path() != folder)
            .cloned()
            .collect();
        inside.sort();
        let mut outermost: Vec<PathBuf> = Vec::new();
        for hole in inside {
            if !outermost
                .last()
                .is_some_and(|above| hole.starts_with(above))
            {
                outermost.push(hole);
            }
        }
        outermost
    }

    /// What `folder` (at or below the root) holds, as Usage keeps a
    /// breakdown: its entries, the heaviest and those with the most files,
    /// the others among its other items. `None` where the index does not
    /// sum it with its sizes live: the scan reads it then. The holes in it
    /// are not added here ([`holes_in`](Self::holes_in)).
    pub(in crate::worker) fn breakdown(
        &mut self,
        folder: &Path,
        types: &mut Types,
        places: &Places,
    ) -> Option<Node> {
        let meta = std::fs::metadata(folder).ok()?;
        let mut children: Vec<Child> = Vec::new();
        let cancel = self.cancel.clone();
        let summed: Summed = self.session.totals(folder, Some(LARGEST), &mut |batch| {
            children.extend(batch);
            !cancel.is_cancelled()
        })?;
        if !matches!(summed.coverage.attrs, Proof::Live { .. }) || cancel.is_cancelled() {
            return None;
        }
        if self.mark.is_none() {
            self.mark = Some(summed.mark.clone());
        }
        self.read_reclaims(places);
        let reclaims = self.reclaims.as_deref().unwrap_or_default();
        let reclaim_of = |path: &Path| {
            places.of(path).or_else(|| {
                reclaims
                    .iter()
                    .find(|(p, _, _)| p == path)
                    .map(|(_, reclaim, _)| *reclaim)
            })
        };
        let reclaimable_below = |path: &Path| -> u64 {
            reclaims
                .iter()
                .filter(|(p, _, _)| p.starts_with(path) && p != path)
                .map(|(_, _, alloc)| alloc)
                .sum()
        };
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
            reclaim: reclaim_of(folder),
            reclaimable: reclaimable_below(folder),
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
                shared: if is_dir { t.shared } else { 0 },
                reclaim: if is_dir {
                    reclaim_of(&child.path)
                } else {
                    None
                },
                reclaimable: if is_dir {
                    reclaimable_below(&child.path)
                } else {
                    0
                },
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
        Some(node)
    }

    /// The folders below the root whose room can be had back whole, asked
    /// of the index once a measure: the places known by their path (the
    /// cache folder, the home trash), a folder holding a signed
    /// `CACHEDIR.TAG`, a trash named as one; outermost, each with its
    /// bytes.
    fn read_reclaims(&mut self, places: &Places) {
        if self.reclaims.is_some() {
            return;
        }
        let root = self.root.clone();
        let mut found: Vec<(PathBuf, Reclaim)> = places
            .paths()
            .filter(|(p, _)| p.starts_with(&root) && *p != root)
            .map(|(p, r)| (p.to_path_buf(), r))
            .collect();
        for tag in self.named(marks::CACHEDIR_TAG, EntryKind::File) {
            if tag.file_name() == Some(marks::CACHEDIR_TAG.as_ref())
                && marks::tags_a_cache(&tag)
                && let Some(folder) = tag.parent()
            {
                found.push((folder.to_path_buf(), Reclaim::Cache));
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
                    found.push((candidate, Reclaim::Trash));
                }
            }
        }
        found.sort_by(|a, b| a.0.cmp(&b.0));
        found.dedup_by(|b, a| b.0.starts_with(&a.0));
        let mut reclaims = Vec::new();
        for (folder, reclaim) in found {
            if self.cancel.is_cancelled() {
                break;
            }
            let alloc = self
                .session
                .totals(&folder, Some(Largest { alloc: 0, files: 0 }), &mut |_| true)
                .filter(|s| matches!(s.coverage.attrs, Proof::Live { .. }))
                .map_or(0, |s| s.totals.alloc);
            reclaims.push((folder, reclaim, alloc));
        }
        self.reclaims = Some(reclaims);
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
/// to its other items where that entry is among them.
pub(in crate::worker) fn add_hole(node: &mut Node, hole: Node) {
    add_totals(node, &hole);
    if let Some(entry) = node
        .children
        .iter_mut()
        .find(|c| hole.path.starts_with(&c.path))
    {
        if entry.path == hole.path {
            let id = entry.id;
            *entry = Node {
                id,
                children: Vec::new(),
                links: Arc::default(),
                ..hole
            };
        } else {
            add_totals(entry, &hole);
        }
    } else if hole.path.parent() == Some(node.path.as_path()) {
        // Summed among the other items, with nothing in it.
        node.other_count = node.other_count.saturating_sub(1);
        node.children.push(Node {
            children: Vec::new(),
            links: Arc::default(),
            ..hole
        });
    } else {
        node.other_alloc += hole.alloc;
        node.other_size += hole.size;
        node.other_files += hole.files;
        node.other_modified = node.other_modified.max(hole.modified);
    }
}
