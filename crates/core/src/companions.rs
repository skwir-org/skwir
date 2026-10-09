//! Files beside a file (ADR-028): a Mac's `._x`, an XMP sidecar
//! `x.ext.xmp`, `TagSpaces`' `.ts/x.json`. Only their names tie them to their
//! file, so the rules are names, declared by the plugins whose stores use
//! them and agreed on here, where the listing hides them and the
//! operations carry them wherever their file goes.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::registry::{Contributions, OwnerGate};

/// One kind of companion.
pub trait Companion: Send + Sync {
    fn id(&self) -> &'static str;
    /// Where the companions of a file named `name` are, relative to its
    /// folder, in an order that is the same for every name: the one at an
    /// index is the one at that index for a file renamed.
    fn of(&self, name: &OsStr) -> Vec<PathBuf>;
    /// The name of the file a companion named `name` (in the same folder)
    /// belongs to, if it is one.
    fn owner(&self, name: &OsStr) -> Option<OsString>;
    /// Whether `name` is a folder of companions for its whole folder
    /// (`TagSpaces`' `.ts`): hidden like them.
    fn is_folder(&self, _name: &OsStr) -> bool {
        false
    }
    /// Whether the companion at `index` of the file at `path` follows the
    /// file: `x.xmp` (Adobe's name, the extension replaced) only while no
    /// other file shares its stem.
    fn follows(&self, _path: &Path, _index: usize) -> bool {
        true
    }
    /// Whether it holds its file's extended attributes (a Mac's `._x`),
    /// where the file's volume keeps none.
    fn holds_attributes(&self) -> bool {
        false
    }
    /// For a companion that holds a file's attributes (a Mac's `._x`):
    /// those it holds for the file at `path`, named as on this system
    /// (`user.…`).
    fn attributes(&self, _path: &Path) -> Option<Vec<(String, Vec<u8>)>> {
        None
    }
    /// Keep `attributes` (named as on this system) beside the file at
    /// `path`, whose volume refuses them; whether it did.
    ///
    /// # Errors
    /// When it holds attributes and cannot write them.
    fn keep_attributes(
        &self,
        _path: &Path,
        _attributes: &[(String, Vec<u8>)],
    ) -> std::io::Result<bool> {
        Ok(false)
    }
}

/// A companion of a file, found: which rule and index, and where it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub rule: usize,
    pub index: usize,
    pub path: PathBuf,
}

/// The setting that shows companions with hidden files.
pub const SHOW: &str = "core.show_companions";

/// The names of the companions in the folders a store's batches were in
/// (`._x`, `x.xmp`, `.ts/x.json`): a folder is read once for its batch, not
/// once per file, and again when it changes. A few folders are kept at
/// once (a search walks several in parallel), each trusted a moment while
/// its time says it did not change: FAT stores times to two seconds.
pub struct FolderNames {
    folders: parking_lot::Mutex<Vec<Seen>>,
    /// Which names of a folder are kept, and as what (`._x` as `x`).
    keep: fn(&OsStr) -> Option<OsString>,
}

struct Seen {
    dir: PathBuf,
    modified: Option<std::time::SystemTime>,
    at: std::time::Instant,
    names: Arc<std::collections::HashSet<OsString>>,
}

impl FolderNames {
    /// Folders remembered at once.
    const FOLDERS: usize = 16;
    /// How long a folder's names are trusted while its time is unchanged.
    const FRESH: std::time::Duration = std::time::Duration::from_secs(5);

    pub fn new(keep: fn(&OsStr) -> Option<OsString>) -> Self {
        FolderNames {
            folders: parking_lot::Mutex::default(),
            keep,
        }
    }

    /// What `keep` takes of `dir`'s names; none when it cannot be read
    /// (it is not there). Reads the folder: on a worker.
    pub fn of(&self, dir: &Path) -> Arc<std::collections::HashSet<OsString>> {
        let modified = std::fs::metadata(dir).ok().and_then(|m| m.modified().ok());
        let known = self
            .folders
            .lock()
            .iter()
            .find(|s| s.dir == dir && s.modified == modified && s.at.elapsed() < Self::FRESH)
            .map(|s| Arc::clone(&s.names));
        if let Some(names) = known {
            return names;
        }
        let names = Arc::new(
            std::fs::read_dir(dir)
                .map(|entries| {
                    entries
                        .flatten()
                        .filter_map(|e| (self.keep)(&e.file_name()))
                        .collect()
                })
                .unwrap_or_default(),
        );
        let mut folders = self.folders.lock();
        folders.retain(|s| s.dir != dir);
        if folders.len() >= Self::FOLDERS {
            folders.remove(0);
        }
        folders.push(Seen {
            dir: dir.to_path_buf(),
            modified,
            at: std::time::Instant::now(),
            names: Arc::clone(&names),
        });
        names
    }
}

/// The registered kinds, in registration order.
#[derive(Default)]
pub struct Companions {
    list: Contributions<Arc<dyn Companion>>,
    /// Shown with hidden files ([`SHOW`]).
    shown: std::sync::atomic::AtomicBool,
}

impl std::fmt::Debug for Companions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Companions")
            .field("ids", &self.ids())
            .finish_non_exhaustive()
    }
}

impl Companions {
    /// Whether companions are shown with hidden files.
    pub fn shown(&self) -> bool {
        self.shown.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn set_shown(&self, shown: bool) {
        self.shown
            .store(shown, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn gated(gate: Arc<OwnerGate>) -> Self {
        Companions {
            list: Contributions::gated(gate),
            shown: std::sync::atomic::AtomicBool::default(),
        }
    }

    pub fn register(&self, owner: &str, rule: Arc<dyn Companion>) {
        self.list.add(owner, rule);
    }

    pub fn unregister_owner(&self, owner: &str) {
        self.list.remove_owner(owner);
    }

    pub fn ids(&self) -> Vec<&'static str> {
        self.rules().iter().map(|r| r.id()).collect()
    }

    fn rules(&self) -> Vec<Arc<dyn Companion>> {
        self.list.all()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// Where the companions of a file named `name` would be, relative to
    /// its folder, by every rule.
    pub fn of(&self, name: &OsStr) -> Vec<PathBuf> {
        self.rules().iter().flat_map(|r| r.of(name)).collect()
    }

    /// The file a companion named `name` belongs to, by any rule. Asked of
    /// every entry listed: the rules are not cloned for it.
    pub fn owner(&self, name: &OsStr) -> Option<OsString> {
        self.list.find_map(|r| r.owner(name))
    }

    /// Whether `name` is a folder of companions (`.ts`).
    pub fn is_folder(&self, name: &OsStr) -> bool {
        self.list
            .find_map(|r| r.is_folder(name).then_some(()))
            .is_some()
    }

    /// The companions of the file at `path` that are there. Blocking: it
    /// looks at the disk.
    pub fn existing(&self, path: &Path) -> Vec<Found> {
        let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for (rule, r) in self.rules().iter().enumerate() {
            for (index, relative) in r.of(name).into_iter().enumerate() {
                let at = dir.join(relative);
                let known = out.iter().any(|f: &Found| f.path == at);
                if !known && std::fs::symlink_metadata(&at).is_ok() && r.follows(path, index) {
                    out.push(Found {
                        rule,
                        index,
                        path: at,
                    });
                }
            }
        }
        out
    }

    /// Where the companion `found` goes for the file at `path` (renamed,
    /// moved, copied there).
    pub fn place(&self, found: &Found, path: &Path) -> Option<PathBuf> {
        let rules = self.rules();
        let r = rules.get(found.rule)?;
        let (dir, name) = (path.parent()?, path.file_name()?);
        r.of(name).into_iter().nth(found.index).map(|p| dir.join(p))
    }

    /// The companion rule that holds a file's attributes (a Mac's `._x`).
    fn container(&self) -> Option<Arc<dyn Companion>> {
        self.rules().into_iter().find(|r| r.holds_attributes())
    }

    /// Whether `found` is a companion that holds its file's attributes.
    pub fn holds_attributes(&self, found: &Found) -> bool {
        self.rules()
            .get(found.rule)
            .is_some_and(|r| r.holds_attributes())
    }

    /// The attributes a companion holds for the file at `path`, named as on
    /// this system; none when no companion holds any.
    pub fn attributes(&self, path: &Path) -> Option<Vec<(String, Vec<u8>)>> {
        self.container()?.attributes(path)
    }

    /// Keep `attributes` beside the file at `path`, whose volume refuses
    /// them; whether a companion could.
    ///
    /// # Errors
    /// When one holds attributes and cannot write them.
    pub fn keep_attributes(
        &self,
        path: &Path,
        attributes: &[(String, Vec<u8>)],
    ) -> std::io::Result<bool> {
        match self.container() {
            Some(c) => c.keep_attributes(path, attributes),
            None => Ok(false),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// `._x`, and `x.note` in a `.notes` folder.
    pub(crate) struct Rules;

    impl Companion for Rules {
        fn id(&self) -> &'static str {
            "test"
        }
        fn of(&self, name: &OsStr) -> Vec<PathBuf> {
            let mut aside = OsString::from("._");
            aside.push(name);
            let mut note = name.to_os_string();
            note.push(".note");
            vec![aside.into(), Path::new(".notes").join(note)]
        }
        fn owner(&self, name: &OsStr) -> Option<OsString> {
            let s = name.to_str()?;
            s.strip_prefix("._")
                .filter(|s| !s.is_empty())
                .map(Into::into)
        }
        fn is_folder(&self, name: &OsStr) -> bool {
            name == ".notes"
        }
    }

    #[test]
    fn companions_are_found_and_placed_by_their_names() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("it's a.txt");
        std::fs::write(&file, b"x").unwrap();
        std::fs::write(tmp.path().join("._it's a.txt"), b"").unwrap();
        let rules = Companions::default();
        rules.register("test", Arc::new(Rules));
        let found = rules.existing(&file);
        assert_eq!(found.len(), 1, "the note is not there");
        assert_eq!(
            rules
                .place(&found[0], &tmp.path().join("sub/b.txt"))
                .unwrap(),
            tmp.path().join("sub/._b.txt")
        );
        assert_eq!(
            rules.owner(OsStr::new("._it's a.txt")).as_deref(),
            Some(OsStr::new("it's a.txt"))
        );
        assert!(rules.owner(OsStr::new("._")).is_none());
        assert!(rules.is_folder(OsStr::new(".notes")));
        rules.unregister_owner("test");
        assert!(rules.existing(&file).is_empty());
    }
}
