//! Referencing an entry from a plan: by stable identity, with a path hint.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::entry::EntryId;
use crate::fs::stat::id_of;
use crate::os_path::PathArg;

/// A stable reference: the entry's identity plus where we last saw it.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EntryRef {
    pub id: EntryId,
    #[serde(with = "crate::os_path")]
    #[schemars(with = "crate::os_path::PathArg")]
    pub path: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("{0} no longer exists")]
    Gone(PathBuf),
    #[error("{path} is now a different file (expected {expected:?}, found {found:?})")]
    Replaced {
        path: PathBuf,
        expected: EntryId,
        found: EntryId,
    },
}

impl EntryRef {
    pub fn new(id: EntryId, path: impl Into<PathBuf>) -> Self {
        EntryRef {
            id,
            path: path.into(),
        }
    }

    /// Reference a path by stat-ing it now.
    pub fn from_path(path: impl Into<PathBuf>) -> Option<Self> {
        let path = path.into();
        let id = id_of(&path)?;
        Some(EntryRef { id, path })
    }

    /// Where the entry is right now. The hint is trusted only if its identity
    /// still matches; otherwise the parent directory is searched for the inode
    /// (a rename in place), and failing that the entry is reported gone.
    pub fn resolve(&self) -> Result<PathBuf, ResolveError> {
        match id_of(&self.path) {
            Some(id) if id == self.id => return Ok(self.path.clone()),
            Some(found) => {
                if let Some(p) = self.find_in_parent() {
                    return Ok(p);
                }
                return Err(ResolveError::Replaced {
                    path: self.path.clone(),
                    expected: self.id,
                    found,
                });
            }
            None => {}
        }
        self.find_in_parent()
            .ok_or_else(|| ResolveError::Gone(self.path.clone()))
    }

    /// The entry's current path in the hint's directory. The directory
    /// listing carries each inode number (`d_ino`), so only a matching name
    /// is stat-ed: a folder of 100k entries costs one read, not 100k stats.
    /// `d_ino` differs from the stat inode only on a mount point, which
    /// cannot be renamed in place anyway.
    fn find_in_parent(&self) -> Option<PathBuf> {
        let parent: &Path = self.path.parent()?;
        let rd = std::fs::read_dir(parent).ok()?;
        rd.flatten()
            .filter(|e| self.may_be(e))
            .map(|e| e.path())
            .find(|p| id_of(p) == Some(self.id))
    }

    /// Whether the folder's entry `e` may be this one, by what the listing
    /// says of it: its inode on Unix; elsewhere every entry may.
    #[cfg_attr(not(unix), allow(clippy::unused_self))]
    fn may_be(&self, e: &std::fs::DirEntry) -> bool {
        #[cfg(unix)]
        {
            std::os::unix::fs::DirEntryExt::ino(e) == self.id.ino
        }
        #[cfg(not(unix))]
        {
            let _ = e;
            true
        }
    }
}

/// An entry a command's arguments name: by its absolute path, as a script
/// or an agent names a file (ADR-043), or by its identity and path, as the
/// window's surfaces name what they show (an `EntryRef`). A path alone is
/// read when the command runs (`App::targets`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum EntryArg {
    /// Its absolute path.
    Path(PathArg),
    /// Its identity and absolute path; the identity may be left out.
    Named {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<EntryId>,
        path: PathArg,
    },
}

impl From<EntryRef> for EntryArg {
    fn from(r: EntryRef) -> Self {
        EntryArg::Named {
            id: Some(r.id),
            path: r.path.into(),
        }
    }
}

impl EntryArg {
    /// The entry named. One named by its path alone is read now (its
    /// identity, as a listing has it: a link is named, not what it leads
    /// to), on the thread of whoever named it so: a script's, an agent's.
    ///
    /// # Errors
    /// A relative path, or one that leads nowhere.
    pub fn into_ref(self) -> anyhow::Result<EntryRef> {
        let (id, path) = match self {
            EntryArg::Path(path) => (None, path.into_inner()),
            EntryArg::Named { id, path } => (id, path.into_inner()),
        };
        crate::os_path::absolute(&path)?;
        match id {
            Some(id) => Ok(EntryRef { id, path }),
            None => EntryRef::from_path(&path)
                .ok_or_else(|| anyhow::anyhow!("{} does not exist", path.display())),
        }
    }

    /// Every entry named, or why one is not.
    ///
    /// # Errors
    /// As [`EntryArg::into_ref`].
    pub fn refs(entries: Vec<EntryArg>) -> anyhow::Result<Vec<EntryRef>> {
        entries.into_iter().map(EntryArg::into_ref).collect()
    }
}
