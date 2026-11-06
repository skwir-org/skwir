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
    /// Whether `name` may be one of its companions, or a folder of them:
    /// what is kept of a folder's names to find companions in, where the
    /// folder has no listing (`BesideProvider`). Its own by default, and
    /// what `owner` or `is_folder` says; a rule whose companions' names do
    /// not say their file (Adobe's `x.xmp`) says which.
    fn claims(&self, name: &OsStr) -> bool {
        self.owner(name).is_some() || self.is_folder(name)
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
    keep: Box<dyn Fn(&OsStr) -> Option<OsString> + Send + Sync>,
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
        Self::keeping(keep)
    }

    /// [`FolderNames::new`], what is kept said by a closure (one that asks
    /// the rules, `BesideProvider`).
    pub fn keeping(keep: impl Fn(&OsStr) -> Option<OsString> + Send + Sync + 'static) -> Self {
        FolderNames {
            folders: parking_lot::Mutex::default(),
            keep: Box::new(keep),
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

    /// The store that asks wrote the file `name` in `dir` (a sidecar made
    /// or replaced): the names kept of `dir` take it, and the folder's new
    /// time, rather than the folder being read again for the next file.
    /// Tagging a folder of raw files and their sidecars read it again for
    /// each, the folder's time moved by each sidecar written.
    pub fn wrote(&self, dir: &Path, name: &OsStr) {
        let modified = std::fs::metadata(dir).ok().and_then(|m| m.modified().ok());
        let mut folders = self.folders.lock();
        if let Some(seen) = folders.iter_mut().find(|s| s.dir == dir) {
            if let Some(kept) = (self.keep)(name) {
                Arc::make_mut(&mut seen.names).insert(kept);
            }
            seen.modified = modified;
            seen.at = std::time::Instant::now();
        }
    }
}

/// Whether a folder, or one above it, holds a marker folder (`.ts`:
/// `TagSpaces` is used there), each folder asked once a moment: a store
/// asked for every folder a walk reads asked again for every folder above
/// it, a `stat` each, the depth of the tree for every folder.
pub struct MarkedAbove {
    marker: &'static str,
    known: parking_lot::Mutex<std::collections::HashMap<PathBuf, (bool, std::time::Instant)>>,
}

impl MarkedAbove {
    /// How long an answer is trusted.
    const FRESH: std::time::Duration = std::time::Duration::from_secs(5);
    /// Answers kept at most; past it, all are asked again.
    const KEPT: usize = 4096;

    pub fn new(marker: &'static str) -> Self {
        MarkedAbove {
            marker,
            known: parking_lot::Mutex::default(),
        }
    }

    /// Whether `dir` or a folder above it holds the marker. Reads the
    /// folders not asked of a moment ago: a worker's.
    pub fn covers(&self, dir: &Path) -> bool {
        if let Some(&(covered, at)) = self.known.lock().get(dir)
            && at.elapsed() < Self::FRESH
        {
            return covered;
        }
        let covered =
            dir.join(self.marker).is_dir() || dir.parent().is_some_and(|up| self.covers(up));
        let mut known = self.known.lock();
        if known.len() >= Self::KEPT {
            known.clear();
        }
        known.insert(dir.to_path_buf(), (covered, std::time::Instant::now()));
        covered
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

    /// Whether any rule claims `name` as a companion or a folder of them
    /// (`Companion::claims`). Asked of every name a folder holds: the
    /// rules are not cloned for it.
    pub fn claims(&self, name: &OsStr) -> bool {
        self.list
            .find_map(|r| r.claims(name).then_some(()))
            .is_some()
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
        self.existing_where(path, |_| true, &|at| std::fs::symlink_metadata(at).is_ok())
    }

    /// [`Companions::existing`], whether each might be there told by
    /// `there` (a plan's: from what it knows of a remote's folder, where
    /// each look is a round trip to the server).
    pub fn existing_as(&self, path: &Path, there: &dyn Fn(&Path) -> bool) -> Vec<Found> {
        self.existing_where(path, |_| true, there)
    }

    /// The companions of the file at `path` that hold its attributes (a
    /// Mac's `._x`) and are there: the others are not looked for.
    /// Blocking.
    pub fn existing_holding_attributes(&self, path: &Path) -> Vec<Found> {
        self.existing_where(path, |r| r.holds_attributes(), &|at| {
            std::fs::symlink_metadata(at).is_ok()
        })
    }

    fn existing_where(
        &self,
        path: &Path,
        wanted: impl Fn(&dyn Companion) -> bool,
        there: &dyn Fn(&Path) -> bool,
    ) -> Vec<Found> {
        let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for (rule, r) in self.rules().iter().enumerate() {
            if !wanted(r.as_ref()) {
                continue;
            }
            for (index, relative) in r.of(name).into_iter().enumerate() {
                let at = dir.join(relative);
                let known = out.iter().any(|f: &Found| f.path == at);
                if !known && there(&at) && r.follows(path, index) {
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

/// What is beside each file of a batch (ADR-028), read with its metadata,
/// wherever its folder's files are read (a folder shown, a search's hits,
/// the Trash, Recent): its companions there (`keys::COMPANIONS`), and for
/// a companion the file it belongs to (`keys::COMPANION_OF`, gone where it
/// is not there). Each rule's names are looked up, never stat-ed one by
/// one: in the folder's listing where it knows them all (`fill_listed`),
/// else in the names the rules claim of the folder, read once while it is
/// unchanged; a folder of companions (`.ts`) is read once while unchanged,
/// and only where it is there.
pub struct BesideProvider {
    companions: Arc<Companions>,
    /// The names of folders read without a listing, those a rule claims.
    claimed: FolderNames,
    /// The names in folders of companions (`.ts`).
    folders: FolderNames,
}

impl BesideProvider {
    pub fn new(companions: Arc<Companions>) -> Self {
        let rules = Arc::clone(&companions);
        BesideProvider {
            companions,
            claimed: FolderNames::keeping(move |name| rules.claims(name).then(|| name.to_owned())),
            folders: FolderNames::new(|name| Some(name.to_owned())),
        }
    }

    fn beside(
        &self,
        dir: &Path,
        listed: Option<&crate::metadata::Listed<'_>>,
        entries: &mut [crate::entry::Entry],
    ) {
        use crate::entry::{Provenance, Value, keys};
        let rules = self.companions.rules();
        if rules.is_empty() || entries.is_empty() {
            return;
        }
        // Without a listing, the names the rules claim, read once.
        let claimed = listed.is_none().then(|| self.claimed.of(dir));
        let named = |name: &OsStr| match (listed, &claimed) {
            (Some(listed), _) => listed(name),
            (None, Some(claimed)) => claimed.contains(name),
            (None, None) => false,
        };
        // Each folder of companions read once for the batch, where it is.
        let mut folders: std::collections::HashMap<
            PathBuf,
            Arc<std::collections::HashSet<OsString>>,
        > = std::collections::HashMap::new();
        let mut there = |relative: &Path| -> bool {
            let Some(file) = relative.file_name() else {
                return false;
            };
            match relative.parent().filter(|p| !p.as_os_str().is_empty()) {
                None => named(file),
                Some(sub) => {
                    let first = sub.components().next().map(|c| c.as_os_str().to_owned());
                    // A listing says whether the folder is there; without
                    // one, it is read, once, to say.
                    if listed.is_some() && !first.is_some_and(|first| named(&first)) {
                        return false;
                    }
                    folders
                        .entry(sub.to_path_buf())
                        .or_insert_with(|| self.folders.of(&dir.join(sub)))
                        .contains(file)
                }
            }
        };
        for entry in entries.iter_mut() {
            let name = entry.name.clone();
            let path = dir.join(&name);
            let mut found: Vec<Value> = Vec::new();
            for rule in &rules {
                for (index, relative) in rule.of(&name).into_iter().enumerate() {
                    if there(&relative) && rule.follows(&path, index) {
                        let said = Value::Text(Arc::from(relative.display().to_string()));
                        if !found.contains(&said) {
                            found.push(said);
                        }
                    }
                }
            }
            if !found.is_empty() {
                entry
                    .metadata
                    .set(keys::COMPANIONS, Value::List(found), Provenance::Filesystem);
            }
            if let Some(owner) = self.companions.owner(&name).filter(|o| !o.is_empty()) {
                // A plain name, which the names claimed do not hold: looked
                // up in the listing, else looked at, for a companion alone.
                let gone = match listed {
                    Some(listed) => !listed(&owner),
                    None => std::fs::symlink_metadata(dir.join(&owner)).is_err(),
                };
                entry.metadata.set(
                    keys::COMPANION_OF,
                    Value::Text(Arc::from(owner.to_string_lossy())),
                    Provenance::Filesystem,
                );
                if gone {
                    entry.metadata.set(
                        keys::COMPANION_OF_GONE,
                        Value::Bool(true),
                        Provenance::Filesystem,
                    );
                }
            }
        }
    }
}

impl crate::metadata::MetadataProvider for BesideProvider {
    fn id(&self) -> &'static str {
        "core.companions"
    }
    fn fill(&self, dir: &Path, entries: &mut [crate::entry::Entry]) {
        self.beside(dir, None, entries);
    }
    fn fill_listed(
        &self,
        dir: &Path,
        listed: &crate::metadata::Listed<'_>,
        entries: &mut [crate::entry::Entry],
    ) {
        self.beside(dir, Some(listed), entries);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// What is beside a file is read with its metadata, the same from a
    /// folder's listing as from the folder itself: a companion beside it,
    /// one in a folder of its own, and for a companion the file it belongs
    /// to, said gone where it is not there.
    #[test]
    fn what_is_beside_a_file_is_read_with_its_metadata() {
        use crate::entry::{Entry, EntryId, EntryKind, Value, keys};
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path();
        for name in ["it's a.txt", "._it's a.txt", "._gone.txt", "other.txt"] {
            std::fs::write(dir.join(name), b"").unwrap();
        }
        std::fs::create_dir(dir.join(".notes")).unwrap();
        std::fs::write(dir.join(".notes/it's a.txt.note"), b"").unwrap();
        let companions = Arc::new(Companions::default());
        companions.register("test", Arc::new(Rules));
        let provider = BesideProvider::new(Arc::clone(&companions));
        let entries = || {
            ["it's a.txt", "._it's a.txt", "._gone.txt", "other.txt"]
                .into_iter()
                .enumerate()
                .map(|(ino, name)| {
                    Entry::new(
                        EntryId {
                            file: crate::FileId::new(1, ino as u64 + 1),
                            at: 0,
                        },
                        name,
                        EntryKind::File,
                    )
                })
                .collect::<Vec<_>>()
        };
        let said = |entries: &[Entry]| -> Vec<(Vec<String>, Option<String>, bool)> {
            entries
                .iter()
                .map(|e| {
                    let beside = match e.metadata.value(keys::COMPANIONS) {
                        Some(Value::List(names)) => names
                            .iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect(),
                        _ => Vec::new(),
                    };
                    let of = e
                        .metadata
                        .value(keys::COMPANION_OF)
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                    (
                        beside,
                        of,
                        e.metadata.value(keys::COMPANION_OF_GONE).is_some(),
                    )
                })
                .collect()
        };
        let note = Path::new(".notes").join("it's a.txt.note");
        let expected = vec![
            (
                vec!["._it's a.txt".to_owned(), note.display().to_string()],
                None,
                false,
            ),
            (Vec::new(), Some("it's a.txt".to_owned()), false),
            (Vec::new(), Some("gone.txt".to_owned()), true),
            (Vec::new(), None, false),
        ];
        let mut read = entries();
        crate::metadata::MetadataProvider::fill(&provider, dir, &mut read);
        assert_eq!(said(&read), expected, "looked up in the folder");
        let names: std::collections::HashSet<OsString> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        let listed = |name: &OsStr| names.contains(name);
        let mut listed_read = entries();
        crate::metadata::MetadataProvider::fill_listed(&provider, dir, &listed, &mut listed_read);
        assert_eq!(said(&listed_read), expected, "looked up in its listing");
    }

    /// A file the store wrote joins the names it keeps of its folder.
    #[test]
    fn a_file_written_joins_the_names_kept() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.xmp"), b"").unwrap();
        let names = FolderNames::new(|name| Some(name.to_owned()));
        assert_eq!(names.of(dir.path()).len(), 1);
        std::fs::write(dir.path().join("b's.xmp"), b"").unwrap();
        names.wrote(dir.path(), OsStr::new("b's.xmp"));
        let kept = names.of(dir.path());
        assert!(kept.contains(OsStr::new("b's.xmp")) && kept.contains(OsStr::new("a.xmp")));
    }

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
