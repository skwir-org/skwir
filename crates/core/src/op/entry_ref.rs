//! Referencing an entry from a plan: by stable identity, with a path hint.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use serde::{Deserialize, Serialize};

use super::listed::Listed;
use crate::entry::FileId;
use crate::fs::stat::{id_of, named};
use crate::os_path::PathArg;

/// A stable reference: a file plus the name we last saw it under. Two
/// references are equal when they name the same file at the same path,
/// whatever they knew of its names.
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EntryRef {
    pub id: FileId,
    #[serde(with = "crate::os_path")]
    #[schemars(with = "crate::os_path::PathArg")]
    pub path: PathBuf,
    /// Whether the file had no other name when it was referenced (a
    /// folder never has; a file had when its link count was one). Only
    /// then may [`EntryRef::resolve`] follow it to another name: of a
    /// file with several names (hard links), the name referenced gone and
    /// that name renamed look alike, and following would act on another
    /// name. Not known is no.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub only_name: bool,
}

impl PartialEq for EntryRef {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.path == other.path
    }
}

impl Eq for EntryRef {}

impl std::hash::Hash for EntryRef {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
        self.path.hash(state);
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("“{}” is not there any more", name_of(.0))]
    Gone(PathBuf),
    #[error("“{}” is now another file", name_of(path))]
    Replaced {
        path: PathBuf,
        expected: FileId,
        found: FileId,
    },
}

/// An entry's own name, for a message.
fn name_of(path: &Path) -> std::path::Display<'_> {
    path.file_name()
        .map_or(path.display(), |name| Path::new(name).display())
}

/// Whether `path` is on a remote whose program makes its inode numbers up
/// (a FUSE one: sshfs, rclone, GVfs), asked of the mount table in memory.
fn made_up(path: &Path) -> bool {
    crate::fs::reach::of(path).is_some_and(|remote| !remote.by_kernel())
}

/// Whether the numbers `known` and `found` were made up by a mount's
/// program (`FileId::lasts`), or `path` is on a remote whose are.
fn numbers_made_up(known: FileId, found: FileId, path: &Path) -> bool {
    !known.lasts() || !found.lasts() || made_up(path)
}

impl EntryRef {
    /// A reference that knows nothing of the file's other names: it
    /// resolves at `path` alone ([`EntryRef::only_name`]).
    pub fn new(id: FileId, path: impl Into<PathBuf>) -> Self {
        EntryRef {
            id,
            path: path.into(),
            only_name: false,
        }
    }

    /// This reference, saying whether the file had no other name.
    #[must_use]
    pub fn with_only_name(mut self, only_name: bool) -> Self {
        self.only_name = only_name;
        self
    }

    /// Reference a path by stat-ing it now.
    pub fn from_path(path: impl Into<PathBuf>) -> Option<Self> {
        let path = path.into();
        let (id, only_name) = named(&path)?;
        Some(EntryRef {
            id,
            path,
            only_name,
        })
    }

    /// The entry this names, as a listing of its folder names it (the
    /// file, under this name: `EntryId::in_folder`): the folder is the one
    /// its path leads to, a link to it followed, as a listing opened there
    /// has it (`Dir::identity`). Reads the folder's identity: blocking.
    pub fn entry(&self) -> Option<crate::entry::EntryId> {
        let (dir, name) = (self.path.parent()?, self.path.file_name()?);
        let folder = crate::fs::stat::identity(dir, &std::fs::metadata(dir).ok()?)?;
        Some(crate::entry::EntryId::in_folder(self.id, folder, name))
    }

    /// Where the entry is right now. The hint is trusted only if its
    /// identity still matches, or where numbers are made up (a FUSE
    /// remote's, which tell no file from another across a remount);
    /// otherwise the file is followed to its new name in the hint's folder
    /// (a rename in place) when that can be told
    /// ([`EntryRef::only_name`], and one name holding it there), and
    /// failing that the entry is reported gone, or replaced.
    pub fn resolve(&self) -> Result<PathBuf, ResolveError> {
        self.resolve_by(|| self.renamed())
    }

    /// [`EntryRef::resolve`], the hint's folder listed by `listed` where
    /// the file is looked for under another name: a plan's steps share
    /// one listing a folder (`StepContext::listed`), where each would
    /// list it again.
    pub(super) fn resolve_with(
        &self,
        listed: &dyn Fn(&Path) -> Option<Rc<Listed>>,
    ) -> Result<PathBuf, ResolveError> {
        self.resolve_by(|| self.renamed_in(listed))
    }

    /// Where the entry is, `renamed` saying where the file went from the
    /// hint's name, when it can tell.
    fn resolve_by(&self, renamed: impl Fn() -> Option<PathBuf>) -> Result<PathBuf, ResolveError> {
        match id_of(&self.path) {
            Some(id) if id == self.id => return Ok(self.path.clone()),
            Some(found) if numbers_made_up(self.id, found, &self.path) => {
                return Ok(self.path.clone());
            }
            Some(found) => {
                if let Some(p) = renamed() {
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
        renamed().ok_or_else(|| ResolveError::Gone(self.path.clone()))
    }

    /// Whether the file may be followed to another name in the hint's
    /// folder: it had no other name when referenced (two names of it now
    /// would be a link made since, either of which may be the one
    /// renamed), and its number tells it from another file. Never where
    /// numbers are made up: sshfs numbers files in the order they are
    /// looked up, again once the kernel let go of one and from the start
    /// when mounted again, so the number may be another file's (ADR-077).
    fn may_follow(&self) -> bool {
        self.only_name && self.id.lasts() && !made_up(&self.path)
    }

    /// The file's new name in the hint's folder, when that can be told
    /// ([`EntryRef::may_follow`]): exactly one name there holds it now.
    fn renamed(&self) -> Option<PathBuf> {
        if !self.may_follow() {
            return None;
        }
        let mut names = self.names_in_parent()?;
        let first = names.next()?;
        names.next().is_none().then_some(first)
    }

    /// [`EntryRef::renamed`], from the hint's folder as `listed` lists it.
    fn renamed_in(&self, listed: &dyn Fn(&Path) -> Option<Rc<Listed>>) -> Option<PathBuf> {
        if !self.may_follow() {
            return None;
        }
        let parent = self.path.parent()?;
        let listed = listed(parent)?;
        let mut names = listed
            .names_of(self.id.ino)
            .iter()
            .map(|name| parent.join(name))
            .filter(|p| id_of(p) == Some(self.id));
        let first = names.next()?;
        names.next().is_none().then_some(first)
    }

    /// The names of the file in the hint's directory. The directory
    /// listing carries each inode number (`d_ino`), so only a matching name
    /// is stat-ed: a folder of 100k entries costs one read, not 100k stats.
    /// `d_ino` differs from the stat inode only on a mount point, which
    /// cannot be renamed in place anyway.
    fn names_in_parent(&self) -> Option<impl Iterator<Item = PathBuf> + '_> {
        let parent: &Path = self.path.parent()?;
        let rd = std::fs::read_dir(parent).ok()?;
        Some(
            rd.flatten()
                .filter(|e| self.may_be(e))
                .map(|e| e.path())
                .filter(|p| id_of(p) == Some(self.id)),
        )
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
        id: Option<FileId>,
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
            // What it knew of the file's names is not said: it resolves
            // at its path alone.
            Some(id) => Ok(EntryRef::new(id, path)),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A file named through a link to its folder is the entry its folder's
    /// listing has, as named directly.
    #[cfg(unix)]
    #[test]
    fn an_entry_through_a_link_to_its_folder_is_its_folder_s() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("real  photos");
        std::fs::create_dir(&real).unwrap();
        std::fs::write(real.join("it’s a.jpg"), "a").unwrap();
        std::os::unix::fs::symlink(&real, tmp.path().join("to photos")).unwrap();
        let through = EntryRef::from_path(tmp.path().join("to photos/it’s a.jpg")).unwrap();
        let direct = EntryRef::from_path(real.join("it’s a.jpg")).unwrap();
        assert!(direct.entry().is_some());
        assert_eq!(through.entry(), direct.entry());
    }

    #[test]
    fn a_number_a_mount_made_up_tells_nothing_and_the_path_is_taken() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("it's here.txt");
        std::fs::write(&path, "here").unwrap();
        let id = id_of(&path).unwrap();
        let other = FileId::new(id.dev, id.ino + 1);
        assert!(
            matches!(
                EntryRef::new(other, &path).resolve(),
                Err(ResolveError::Replaced { .. })
            ),
            "where numbers are the files', another number is another file"
        );
        // Its device a mount's whose program makes numbers up: another
        // number is what a remount gives the same file.
        crate::fs::reach::pretend_made_up(id.dev, 1 << 40);
        let mounted_before = other;
        assert_eq!(
            EntryRef::new(mounted_before, &path)
                .with_only_name(true)
                .resolve()
                .unwrap(),
            path,
            "a remount numbered it anew"
        );
        std::fs::remove_file(&path).unwrap();
        assert!(matches!(
            EntryRef::new(mounted_before, &path)
                .with_only_name(true)
                .resolve(),
            Err(ResolveError::Gone(_))
        ));
    }
}
