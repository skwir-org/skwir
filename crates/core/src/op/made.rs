//! What a step makes, item by item (an archive's members), so that a
//! folder there under its name is merged into as a copy's is (ADR-054,
//! ADR-059 amended 2026-11-14): each item told the same as what is there,
//! all of them in one walk ([`MadeTree::sames`]), and where each goes once
//! the names are settled ([`Placing`]).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::CancelToken;
use crate::os_path::PathArg;

/// How far a file's time on the disk may be from what was made: a FAT
/// drive keeps two-second times.
pub const TIME_KEPT: Duration = Duration::from_secs(2);

/// One item of what a step makes, as it will be on the disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MadeItem {
    Folder,
    /// A file of this size and time, where they are said: one that says
    /// neither is never told the same as what is there.
    File {
        size: Option<u64>,
        modified: Option<SystemTime>,
    },
    /// A link saying this.
    Link(String),
    /// Another name of the file at this path, below what is made.
    Hard(PathBuf),
}

/// What a step makes, all the way down: each item by its path below what
/// is made, what is made itself under the empty path.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MadeTree {
    items: BTreeMap<PathBuf, MadeItem>,
}

impl MadeTree {
    /// Of `items`, each by its path below what is made, the empty path for
    /// what is made itself; the folders above an item are folders, where
    /// they are not said.
    pub fn new(items: BTreeMap<PathBuf, MadeItem>) -> Self {
        let mut tree = MadeTree { items };
        let above: Vec<PathBuf> = tree
            .items
            .keys()
            .flat_map(|path| path.ancestors().skip(1).map(Path::to_path_buf))
            .collect();
        for path in above {
            tree.items.entry(path).or_insert(MadeItem::Folder);
        }
        tree
    }

    pub fn get(&self, path: &Path) -> Option<&MadeItem> {
        self.items.get(path)
    }

    /// How many items it has, what is made itself counted.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// What is made at `path`, by paths below it: `None` where nothing is.
    /// Another name of a file outside it is a file of nothing said, as it
    /// cannot be told the same there.
    pub fn below(&self, path: &Path) -> Option<MadeTree> {
        self.items.get(path)?;
        // Below a path, its items stand together: paths are ordered by
        // their parts.
        let items = self
            .items
            .range(path.to_path_buf()..)
            .map_while(|(at, item)| Some((at.strip_prefix(path).ok()?, item)))
            .map(|(at, item)| {
                let item = match item {
                    MadeItem::Hard(first) => first.strip_prefix(path).map_or(
                        MadeItem::File {
                            size: None,
                            modified: None,
                        },
                        |first| MadeItem::Hard(first.to_path_buf()),
                    ),
                    other => other.clone(),
                };
                (at.to_path_buf(), item)
            })
            .collect();
        Some(MadeTree { items })
    }

    /// The items of each folder, in their names' order.
    pub fn children(&self) -> HashMap<&Path, Vec<&Path>> {
        let mut children: HashMap<&Path, Vec<&Path>> = HashMap::new();
        for path in self.items.keys() {
            if let Some(parent) = path.parent() {
                children.entry(parent).or_default().push(path);
            }
        }
        children
    }

    /// The items `target` holds the same of, all the way down, by their
    /// paths: a folder there holding the same items and nothing else, each
    /// file of its size and time (to two seconds), each link saying the
    /// same, each other name of a file to its first. One walk, a link
    /// never followed: each item is read once, a folder's names once, and
    /// what is under a folder that is not there is never read. Cancelled,
    /// none are.
    pub fn sames(&self, target: &Path, cancel: &CancelToken) -> HashSet<&Path> {
        let children = self.children();
        let at = |path: &Path| {
            // Never `target/` for the empty path: it follows a link.
            if path.as_os_str().is_empty() {
                target.to_path_buf()
            } else {
                target.join(path)
            }
        };
        let none: &[&Path] = &[];
        let mut same = HashSet::new();
        // Each folder's items before the folder itself.
        let mut walk: Vec<(&Path, bool)> = self
            .items
            .get_key_value(Path::new(""))
            .map(|(top, _)| (top.as_path(), false))
            .into_iter()
            .collect();
        while let Some((path, after)) = walk.pop() {
            if cancel.is_cancelled() {
                return HashSet::new();
            }
            let there = at(path);
            let items = children.get(path).map_or(none, Vec::as_slice);
            match &self.items[path] {
                MadeItem::Folder if !after => {
                    if std::fs::symlink_metadata(&there).is_ok_and(|meta| meta.is_dir()) {
                        walk.push((path, true));
                        walk.extend(items.iter().map(|item| (*item, false)));
                    }
                }
                MadeItem::Folder => {
                    // Its names counted no further than one past its
                    // items: a folder of other things is not read whole.
                    let held = items.iter().all(|item| same.contains(item))
                        && std::fs::read_dir(&there)
                            .is_ok_and(|names| names.take(items.len() + 1).count() == items.len());
                    if held {
                        same.insert(path);
                    }
                }
                item => {
                    if is_same(item, &there, &|first| at(first)) {
                        same.insert(path);
                    }
                }
            }
        }
        same
    }

    /// Whether `target` holds what is made, and nothing else.
    pub fn held_at(&self, target: &Path, cancel: &CancelToken) -> bool {
        self.sames(target, cancel).contains(Path::new(""))
    }
}

/// Whether what is at `there` is `item`, a file, a link or another name
/// (`at` says where a first name is).
fn is_same(item: &MadeItem, there: &Path, at: &dyn Fn(&Path) -> PathBuf) -> bool {
    let Ok(meta) = std::fs::symlink_metadata(there) else {
        return false;
    };
    match item {
        MadeItem::Folder => meta.is_dir(),
        MadeItem::File { size, modified } => {
            let near = |a: SystemTime, b: SystemTime| {
                a.duration_since(b)
                    .or_else(|_| b.duration_since(a))
                    .is_ok_and(|apart| apart < TIME_KEPT)
            };
            meta.is_file()
                && *size == Some(meta.len())
                && modified
                    .zip(meta.modified().ok())
                    .is_some_and(|(a, b)| near(a, b))
        }
        MadeItem::Link(says) => {
            meta.is_symlink()
                && std::fs::read_link(there)
                    .is_ok_and(|read| read.as_os_str().to_string_lossy() == says.as_str())
        }
        MadeItem::Hard(first) => {
            meta.is_file()
                && crate::fs::stat::id_of(there)
                    .is_some_and(|id| crate::fs::stat::id_of(&at(first)) == Some(id))
        }
    }
}

/// An item of what a step makes put in the folder there it is merged
/// into: from its path below what is made, to its path below that folder
/// (another name where both are kept, `photo (2).jpg`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Placing {
    #[serde(with = "crate::os_path")]
    #[schemars(with = "PathArg")]
    pub from: PathBuf,
    #[serde(with = "crate::os_path")]
    #[schemars(with = "PathArg")]
    pub to: PathBuf,
}

impl Placing {
    /// The item at `path` placed under its own path.
    pub fn same(path: &Path) -> Self {
        Placing {
            from: path.to_path_buf(),
            to: path.to_path_buf(),
        }
    }

    /// Whether both its paths stay below where they are read: names alone,
    /// never `..`, a root or nothing. What a plan from elsewhere says is
    /// refused otherwise.
    pub fn stays_below(&self) -> bool {
        let below = |path: &Path| {
            !path.as_os_str().is_empty()
                && path
                    .components()
                    .all(|part| matches!(part, Component::Normal(_)))
        };
        below(&self.from) && below(&self.to)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(size: u64, modified: SystemTime) -> MadeItem {
        MadeItem::File {
            size: Some(size),
            modified: Some(modified),
        }
    }

    fn written(path: &Path, text: &str) -> SystemTime {
        std::fs::write(path, text).unwrap();
        std::fs::metadata(path).unwrap().modified().unwrap()
    }

    #[test]
    fn the_items_held_the_same_are_told_all_the_way_down() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("it's made");
        std::fs::create_dir_all(root.join("album/2026")).unwrap();
        let kept = written(&root.join("album/2026/day one.txt"), "first");
        let top = written(&root.join("top.txt"), "top");
        let tree = MadeTree::new(BTreeMap::from([
            (PathBuf::new(), MadeItem::Folder),
            ("top.txt".into(), file(3, top)),
            ("album/2026/day one.txt".into(), file(5, kept)),
            ("album/2026/day two.txt".into(), file(6, kept)),
            ("gone/far/away.txt".into(), file(1, kept)),
        ]));
        assert_eq!(tree.get(Path::new("album")), Some(&MadeItem::Folder));
        let cancel = CancelToken::new();
        let same = tree.sames(&root, &cancel);
        let mut said: Vec<_> = same.iter().map(|p| p.to_string_lossy()).collect();
        said.sort();
        // A folder missing an item is not the same; what is under a folder
        // that is not there is never read.
        assert_eq!(said, ["album/2026/day one.txt", "top.txt"]);
        assert!(!tree.held_at(&root, &cancel));

        // Whole, and nothing else: the same.
        written(&root.join("album/2026/day two.txt"), "second");
        std::fs::File::open(root.join("album/2026/day two.txt"))
            .unwrap()
            .set_modified(kept)
            .unwrap();
        let whole = tree.below(Path::new("album")).unwrap();
        assert!(whole.held_at(&root.join("album"), &cancel));
        // Something else in it: not.
        written(&root.join("album/mine.txt"), "mine");
        assert!(!whole.held_at(&root.join("album"), &cancel));
        assert!(
            tree.below(Path::new("album/2026"))
                .unwrap()
                .held_at(&root.join("album/2026"), &cancel)
        );

        let cancelled = CancelToken::new();
        cancelled.cancel();
        assert!(tree.sames(&root, &cancelled).is_empty());
    }

    #[test]
    fn what_is_below_a_path_stands_apart_from_names_beside_it() {
        let tree = MadeTree::new(BTreeMap::from([
            ("a/one".into(), MadeItem::Link("one".into())),
            ("a b".into(), MadeItem::Folder),
            ("a/two".into(), MadeItem::Hard("a/one".into())),
            ("a/three".into(), MadeItem::Hard("elsewhere".into())),
        ]));
        let below = tree.below(Path::new("a")).unwrap();
        let mut paths: Vec<_> = below.items.keys().map(|p| p.to_string_lossy()).collect();
        paths.sort();
        assert_eq!(paths, ["", "one", "three", "two"]);
        assert_eq!(
            below.get(Path::new("two")),
            Some(&MadeItem::Hard("one".into()))
        );
        assert!(matches!(
            below.get(Path::new("three")),
            Some(MadeItem::File { size: None, .. })
        ));
        assert!(tree.below(Path::new("nothing")).is_none());
    }

    #[test]
    fn a_placing_stays_below_where_it_is_read() {
        assert!(Placing::same(Path::new("album/day one.txt")).stays_below());
        for bad in ["../outside", "/etc/passwd", "", "a/../../b", "./a"] {
            let placing = Placing {
                from: PathBuf::from("a"),
                to: PathBuf::from(bad),
            };
            assert!(!placing.stays_below(), "{bad}");
        }
    }
}
