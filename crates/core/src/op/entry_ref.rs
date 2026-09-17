//! Referencing an entry from a plan: by stable identity, with a path hint.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::entry::EntryId;
use crate::fs::stat::id_of;

/// A stable reference: the entry's identity plus where we last saw it.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EntryRef {
    pub id: EntryId,
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

    fn find_in_parent(&self) -> Option<PathBuf> {
        let parent: &Path = self.path.parent()?;
        let rd = std::fs::read_dir(parent).ok()?;
        for e in rd.flatten() {
            if let Ok(m) = e.metadata() {
                use std::os::unix::fs::MetadataExt;
                if m.ino() == self.id.ino && m.dev() == self.id.dev {
                    return Some(e.path());
                }
            }
        }
        None
    }
}
