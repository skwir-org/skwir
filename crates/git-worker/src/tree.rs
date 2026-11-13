//! What a tree holds, read without gix's diff (which would bring its
//! filters): its entries, and the entry at a path.
use crate::Result;
use gix::ObjectId;
use gix::bstr::{BString, ByteSlice};
use gix::objs::tree::{EntryKind, EntryMode};
use std::collections::HashMap;

/// An entry of a tree: what it is and the object it names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    pub mode: EntryMode,
    pub id: ObjectId,
}

impl Entry {
    pub fn is_tree(self) -> bool {
        self.mode.kind() == EntryKind::Tree
    }
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
    let mut entry = Entry {
        mode: EntryKind::Tree.into(),
        id,
    };
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
pub(crate) struct Lookups<'p> {
    path: &'p [&'p [u8]],
    /// What tree `id` holds at `path[depth..]`, by depth.
    known: Vec<HashMap<ObjectId, Option<Entry>>>,
}

impl<'p> Lookups<'p> {
    pub fn new(path: &'p [&'p [u8]]) -> Self {
        Self {
            path,
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
            return Ok(Some(Entry {
                mode: EntryKind::Tree.into(),
                id: tree,
            }));
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
