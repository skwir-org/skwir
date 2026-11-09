//! A search's results as the tree of the folders they are in, as the
//! Tree view shows them: the same answer as the flat list, each hit under
//! its folders, and the folders on the way that are no hit themselves
//! (the ways, lent to the pane, ADR-116, which draws them dimmed).
//!
//! Made on a worker from the paths of the flat order: files stand in the
//! order the pane sorts them, folders by name before them.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::entry::{Entry, EntryId};
use crate::hashing::{Map, Set};
use crate::workspace::Loaned;

/// A tree made.
#[derive(Debug, Default)]
pub(crate) struct Made {
    /// Every row, each under its folder.
    pub order: Arc<[EntryId]>,
    /// Each row's depth below the root searched.
    pub depth: Map<EntryId, u32>,
    /// The folders with rows under them.
    pub parents: Set<EntryId>,
    /// The folders that are only on the way to a hit.
    pub ways: Vec<Loaned>,
    pub way_ids: Set<EntryId>,
}

/// What stands in a folder of the tree.
#[derive(Default)]
struct Held<'a> {
    folders: Vec<&'a Path>,
    files: Vec<(EntryId, &'a Path)>,
}

/// A row to come of a folder being written out.
#[derive(Clone, Copy)]
enum Row<'a> {
    Folder(&'a Path),
    File(EntryId),
}

/// The tree of `hits`, the flat order's entries with their paths, below
/// `root`: `None` once `cancelled`. A folder on the way is `known` from
/// what was made before, else stat-ed; one that cannot be read has no row
/// and what is under it stands where it would.
pub(crate) fn make(
    root: &Path,
    hits: &[(EntryId, PathBuf)],
    ascending: bool,
    stat: &mut dyn FnMut(&Path) -> Option<Entry>,
    known: &mut HashMap<PathBuf, Entry>,
    cancelled: &dyn Fn() -> bool,
) -> Option<Made> {
    let by_path: HashMap<&Path, EntryId> = hits.iter().map(|(id, p)| (p.as_path(), *id)).collect();
    let mut held: HashMap<&Path, Held<'_>> = HashMap::new();
    let mut placed: HashSet<&Path> = HashSet::new();
    for (id, path) in hits {
        let Some(parent) = path
            .parent()
            .filter(|_| path.starts_with(root) && path != root)
        else {
            continue;
        };
        held.entry(parent).or_default().files.push((*id, path));
        let mut folder = parent;
        while folder != root && placed.insert(folder) {
            let Some(above) = folder.parent() else { break };
            held.entry(above).or_default().folders.push(folder);
            folder = above;
        }
    }
    let mut made = Made::default();
    let mut order = Vec::with_capacity(hits.len());
    let mut stack = vec![(rows_of(root, &held, ascending), 0_usize, 0_u32)];
    while let Some((rows, next, depth)) = stack.last_mut() {
        let Some(&row) = rows.get(*next) else {
            stack.pop();
            continue;
        };
        *next += 1;
        let depth = *depth;
        match row {
            Row::File(id) => {
                order.push(id);
                made.depth.insert(id, depth);
            }
            Row::Folder(dir) => {
                if cancelled() {
                    return None;
                }
                let id = by_path.get(dir).copied().or_else(|| {
                    if !known.contains_key(dir) {
                        known.insert(dir.to_path_buf(), stat(dir)?);
                    }
                    let entry = known.get(dir)?.clone();
                    let id = entry.id;
                    made.way_ids.insert(id);
                    made.ways.push(Loaned {
                        entry,
                        path: dir.to_path_buf(),
                        seen: 0,
                    });
                    Some(id)
                });
                if let Some(id) = id {
                    order.push(id);
                    made.depth.insert(id, depth);
                    made.parents.insert(id);
                }
                stack.push((rows_of(dir, &held, ascending), 0, depth + 1));
            }
        }
    }
    made.order = order.into();
    Some(made)
}

/// What stands in `dir`, in the order it is shown: its folders by name,
/// then its files as sorted.
fn rows_of<'a>(dir: &Path, held: &HashMap<&'a Path, Held<'a>>, ascending: bool) -> Vec<Row<'a>> {
    let Some(here) = held.get(dir) else {
        return Vec::new();
    };
    let mut folders = here.folders.clone();
    folders.sort_by(|a, b| {
        let order = crate::collate::compare(
            a.file_name().unwrap_or_default(),
            b.file_name().unwrap_or_default(),
        );
        if ascending { order } else { order.reverse() }
    });
    let files = here
        .files
        .iter()
        // A folder found, with something found in it: among the
        // folders, as what it holds is under it.
        .filter(|(_, path)| !held.contains_key(path))
        .map(|(id, _)| Row::File(*id));
    folders.into_iter().map(Row::Folder).chain(files).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::{EntryKind, FileId};

    fn id(n: u64) -> EntryId {
        EntryId::in_folder(
            FileId::new(1, n),
            FileId::new(1, 0),
            std::ffi::OsStr::new("x"),
        )
    }

    fn folder(path: &Path) -> Entry {
        let name = path.file_name().unwrap().to_owned();
        let n = name.len() as u64 + 1000;
        Entry::new(id(n), name, EntryKind::Directory)
    }

    fn made(hits: &[(EntryId, &str)], ascending: bool) -> Made {
        let hits: Vec<(EntryId, PathBuf)> = hits
            .iter()
            .map(|(id, path)| (*id, PathBuf::from(path)))
            .collect();
        make(
            Path::new("/r"),
            &hits,
            ascending,
            &mut |p| Some(folder(p)),
            &mut HashMap::new(),
            &|| false,
        )
        .unwrap()
    }

    #[test]
    fn each_hit_stands_under_its_folders_files_as_sorted() {
        let (a, b, c) = (id(1), id(2), id(3));
        let tree = made(
            &[
                (a, "/r/sub/deep/z.txt"),
                (b, "/r/sub/a.txt"),
                (c, "/r/top.txt"),
            ],
            true,
        );
        assert_eq!(tree.ways.len(), 2, "sub and deep");
        let way = |name: &str| tree.ways.iter().find(|w| w.path.ends_with(name)).unwrap();
        let (sub, deep) = (way("sub").entry.id, way("deep").entry.id);
        // Folders before the files of the same folder.
        assert_eq!(&*tree.order, [sub, deep, a, b, c]);
        let depth = |id| tree.depth[&id];
        assert_eq!(
            [depth(sub), depth(deep), depth(a), depth(b), depth(c)],
            [0, 1, 2, 1, 0]
        );
        assert!(tree.parents.contains(&sub) && tree.parents.contains(&deep));
        assert!(!tree.parents.contains(&a));
    }

    #[test]
    fn a_folder_found_is_no_way_and_holds_what_was_found_in_it() {
        let (dir, file, leaf) = (id(1), id(2), id(3));
        let tree = made(
            &[(dir, "/r/src"), (leaf, "/r/empty"), (file, "/r/src/a.rs")],
            true,
        );
        assert!(tree.ways.is_empty());
        assert_eq!(&*tree.order, [dir, file, leaf]);
        assert_eq!(tree.depth[&file], 1);
        assert!(tree.parents.contains(&dir));
        assert!(!tree.parents.contains(&leaf), "nothing was found in it");
    }

    #[test]
    fn folders_follow_the_direction_and_a_folder_read_before_is_not_read_again() {
        let (a, b) = (id(1), id(2));
        let hits = [(a, "/r/a it's/f"), (b, "/r/b/f")];
        let tree = made(&hits, false);
        let names: Vec<_> = tree
            .order
            .iter()
            .filter(|id| tree.way_ids.contains(id))
            .map(|id| {
                let way = tree.ways.iter().find(|w| w.entry.id == *id).unwrap();
                way.entry.name_str().into_owned()
            })
            .collect();
        assert_eq!(names, ["b", "a it's"], "the direction asked");
        let hits: Vec<(EntryId, PathBuf)> =
            hits.iter().map(|(i, p)| (*i, PathBuf::from(p))).collect();
        let mut known = HashMap::new();
        let mut stats = 0;
        let mut stat = |p: &Path| {
            stats += 1;
            Some(folder(p))
        };
        make(Path::new("/r"), &hits, true, &mut stat, &mut known, &|| {
            false
        })
        .unwrap();
        make(Path::new("/r"), &hits, true, &mut stat, &mut known, &|| {
            false
        })
        .unwrap();
        assert_eq!(stats, 2, "once each");
    }
}
