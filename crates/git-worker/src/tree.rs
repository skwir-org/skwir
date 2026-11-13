//! What a tree holds, read without gix's diff (which would bring its
//! filters): its entries, and the entry at a path.
use crate::Result;
use gix::ObjectId;
use gix::bstr::{BString, ByteSlice};
use gix::objs::tree::{EntryKind, EntryMode};
use std::cmp::Ordering;
use std::collections::HashMap;

/// An entry of a tree: what it is and the object it names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    pub mode: EntryMode,
    pub id: ObjectId,
}

impl Entry {
    /// The tree `id`, as an entry.
    pub fn tree(id: ObjectId) -> Self {
        Self {
            mode: EntryKind::Tree.into(),
            id,
        }
    }

    pub fn is_tree(self) -> bool {
        self.mode.kind() == EntryKind::Tree
    }

    /// A file's: its bytes, executable or not, or a link's target; not a
    /// tree, nor a submodule's commit.
    pub fn is_file(self) -> bool {
        matches!(
            self.mode.kind(),
            EntryKind::Blob | EntryKind::BlobExecutable | EntryKind::Link
        )
    }

    /// A regular file's, executable or not: what is compared by its bytes.
    pub fn is_regular(self) -> bool {
        matches!(
            self.mode.kind(),
            EntryKind::Blob | EntryKind::BlobExecutable
        )
    }
}

/// An entry that differs between two trees, what it was and is: a file
/// (or a submodule's commit) added, deleted or changed.
pub(crate) struct Change {
    pub path: BString,
    pub old: Option<Entry>,
    pub new: Option<Entry>,
}

/// The order of tree entries: by name, a tree's as if it ended in `/`.
fn order(a: (&[u8], bool), b: (&[u8], bool)) -> Ordering {
    fn key((name, tree): (&[u8], bool)) -> impl Iterator<Item = u8> + '_ {
        name.iter().copied().chain(tree.then_some(b'/'))
    }
    key(a).cmp(key(b))
}

/// The files that differ between `old` and `new`, the entries at `path`
/// (a tree's files, recursively), in tree order: a subtree alike on both
/// sides (one id) is not read, so the cost is in what changed.
pub(crate) fn changed(
    repo: &gix::Repository,
    path: &BString,
    old: Option<Entry>,
    new: Option<Entry>,
    out: &mut Vec<Change>,
) -> Result<()> {
    let old_tree = old.filter(|entry| entry.is_tree());
    let new_tree = new.filter(|entry| entry.is_tree());
    let old_file = old.filter(|entry| !entry.is_tree());
    let new_file = new.filter(|entry| !entry.is_tree());
    // A file and a tree of one name are two entries, the file first.
    if old_file != new_file {
        out.push(Change {
            path: path.clone(),
            old: old_file,
            new: new_file,
        });
    }
    if old_tree.map(|e| e.id) == new_tree.map(|e| e.id) {
        return Ok(());
    }
    let old_entries = match old_tree {
        Some(tree) => entries(repo, tree.id)?,
        None => Vec::new(),
    };
    let new_entries = match new_tree {
        Some(tree) => entries(repo, tree.id)?,
        None => Vec::new(),
    };
    let join = |name: &BString| -> BString {
        if path.is_empty() {
            name.clone()
        } else {
            let mut joined = path.clone();
            joined.push(b'/');
            joined.extend_from_slice(name);
            joined
        }
    };
    let (mut o, mut n) = (0, 0);
    while o < old_entries.len() || n < new_entries.len() {
        let pick = match (old_entries.get(o), new_entries.get(n)) {
            (Some((a, ea)), Some((b, eb))) => order((a, ea.is_tree()), (b, eb.is_tree())),
            (Some(_), None) => Ordering::Less,
            _ => Ordering::Greater,
        };
        match pick {
            Ordering::Less => {
                let (name, entry) = &old_entries[o];
                changed(repo, &join(name), Some(*entry), None, out)?;
                o += 1;
            }
            Ordering::Greater => {
                let (name, entry) = &new_entries[n];
                changed(repo, &join(name), None, Some(*entry), out)?;
                n += 1;
            }
            Ordering::Equal => {
                let ((name, a), (_, b)) = (&old_entries[o], &new_entries[n]);
                if a != b {
                    changed(repo, &join(name), Some(*a), Some(*b), out)?;
                }
                o += 1;
                n += 1;
            }
        }
    }
    Ok(())
}

/// The entries of tree `id`, in the tree's own order (git's: by name, a
/// tree's as if it ended in `/`).
pub(crate) fn entries(repo: &gix::Repository, id: ObjectId) -> Result<Vec<(BString, Entry)>> {
    let tree = repo.find_tree(id)?;
    let decoded = tree.decode()?;
    Ok(decoded
        .entries
        .iter()
        .map(|entry| {
            (
                entry.filename.to_owned(),
                Entry {
                    mode: entry.mode,
                    id: entry.oid.to_owned(),
                },
            )
        })
        .collect())
}

/// The entry of tree `id` named by `path`'s components, none for the tree
/// itself: `None` where nothing is there, or a component above it is no
/// tree.
pub(crate) fn entry_at(
    repo: &gix::Repository,
    id: ObjectId,
    path: &[&[u8]],
) -> Result<Option<Entry>> {
    let mut entry = Entry::tree(id);
    for component in path {
        if !entry.is_tree() {
            return Ok(None);
        }
        let Some(found) = find(repo, entry.id, component)? else {
            return Ok(None);
        };
        entry = found;
    }
    Ok(Some(entry))
}

/// The entry `name` of tree `id`.
fn find(repo: &gix::Repository, id: ObjectId, name: &[u8]) -> Result<Option<Entry>> {
    let tree = repo.find_tree(id)?;
    let decoded = tree.decode()?;
    Ok(decoded
        .entries
        .iter()
        .find(|entry| entry.filename.as_bytes() == name)
        .map(|entry| Entry {
            mode: entry.mode,
            id: entry.oid.to_owned(),
        }))
}

/// The entry at one path, looked up in many trees: each tree, at each depth
/// of the path, read once, so commits that share a subtree (most of them,
/// in a history) share its answer, as git's own pruning by tree ids does.
pub(crate) struct Lookups {
    path: Vec<Vec<u8>>,
    /// What tree `id` holds at `path[depth..]`, by depth.
    known: Vec<HashMap<ObjectId, Option<Entry>>>,
}

impl Lookups {
    pub fn new(path: &[&[u8]]) -> Self {
        Self {
            path: path.iter().map(|component| component.to_vec()).collect(),
            known: vec![HashMap::new(); path.len()],
        }
    }

    /// The entry at the path in the tree `root`.
    pub fn at(&mut self, repo: &gix::Repository, root: ObjectId) -> Result<Option<Entry>> {
        self.below(repo, root, 0)
    }

    fn below(
        &mut self,
        repo: &gix::Repository,
        tree: ObjectId,
        depth: usize,
    ) -> Result<Option<Entry>> {
        let Some(component) = self.path.get(depth) else {
            return Ok(Some(Entry::tree(tree)));
        };
        if let Some(known) = self.known[depth].get(&tree) {
            return Ok(*known);
        }
        let found = match find(repo, tree, component)? {
            Some(entry) if depth + 1 == self.path.len() => Some(entry),
            Some(entry) if entry.is_tree() => self.below(repo, entry.id, depth + 1)?,
            _ => None,
        };
        self.known[depth].insert(tree, found);
        Ok(found)
    }
}
