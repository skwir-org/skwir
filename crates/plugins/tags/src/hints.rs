//! Where tagged files are, for a `tag:` search to consider first
//! (ADR-026): Baloo's index when Baloo runs, and an index of our own in
//! the cache, learned from what the provider reads. Both only hint: the
//! search checks every file named and walks as it would have.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bibli_core::app::App;
use bibli_core::cache::CacheSlot;
use bibli_core::cancel::CancelToken;
use bibli_core::entry::keys;
use bibli_core::hints::HintProvider;
use bibli_core::metadata::same_tag;
use bibli_core::scheduler::Priority;
use parking_lot::{Mutex, MutexGuard};

/// Tagged files remembered at most. Past it, new ones are not learned (a
/// search still finds them by walking); those known are kept up to date.
const AT_MOST: usize = 50_000;
/// The first index kept tags in lowercase; the second as spelled; this one
/// with the folders it covers.
const KEY: &[u8] = b"index.3";
const KEY_2: &[u8] = b"index.2";

/// A folder whose every tagged file, outside hidden folders, the index
/// holds as of a time: a count of it ended then (`count.rs`), and what was
/// learned since keeps it so.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Covered {
    pub root: PathBuf,
    /// Seconds and nanoseconds since the epoch.
    pub since: (u64, u32),
}

impl Covered {
    pub fn since(&self) -> SystemTime {
        UNIX_EPOCH + Duration::new(self.since.0, self.since.1)
    }
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Stored {
    tags: Vec<(String, Vec<String>)>,
    #[serde(default)]
    covered: Vec<Covered>,
}

#[derive(Default)]
struct State {
    /// Each tagged file's tags, as spelled on it (tags ignore case when
    /// compared: [`same_tag`]).
    tags: HashMap<PathBuf, Vec<String>>,
    covered: Vec<Covered>,
    loaded: bool,
}

/// A path's 64-bit FNV-1a hash: the same in every build, so what was
/// stored with one is read by the next.
pub fn stable_hash(parts: &[&[u8]]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for part in parts {
        for byte in *part {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        hash ^= 0xff;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// Where the folders seen by a count of `root` are kept.
fn dirs_key(root: &Path) -> Vec<u8> {
    format!(
        "dirs.{:016x}",
        stable_hash(&[root.as_os_str().as_encoded_bytes()])
    )
    .into_bytes()
}

/// Whether `path`, under `root`, is inside a hidden folder or hidden
/// itself: left out of the counts as a search leaves it out.
fn hidden_under(path: &Path, root: &Path) -> bool {
    path.strip_prefix(root).is_ok_and(|rest| {
        rest.components()
            .any(|c| c.as_os_str().as_encoded_bytes().first() == Some(&b'.'))
    })
}

/// What the index has to say when it changed: the sidebar's tags.
pub type OnChange = Box<dyn Fn(&Arc<TagIndex>) + Send + Sync>;

/// The tagged files bibli has seen: every file the provider reads tells
/// it whether the file has tags (a folder shown, a search's walk, a file
/// changed and read again), so an edit, here or elsewhere, is learned the
/// next time the file is read. Deleting it (it lives in the deletable
/// cache) only makes the first answers of the next search slower.
pub struct TagIndex {
    state: Mutex<State>,
    slot: Option<CacheSlot>,
    app: Weak<App>,
    lifetime: CancelToken,
    writing: AtomicBool,
    on_change: Mutex<Option<OnChange>>,
}

impl TagIndex {
    pub fn new(slot: Option<CacheSlot>, app: &App, lifetime: CancelToken) -> Arc<Self> {
        Arc::new(TagIndex {
            state: Mutex::default(),
            slot,
            app: app.weak(),
            lifetime,
            writing: AtomicBool::new(false),
            on_change: Mutex::default(),
        })
    }

    /// Call `f` on a worker after the index changed, once for changes
    /// close together.
    pub fn on_change(&self, f: OnChange) {
        *self.on_change.lock() = Some(f);
    }

    /// The index, read from the cache the first time it is needed: on the
    /// worker that needs it, never at startup.
    fn state(&self) -> MutexGuard<'_, State> {
        let mut state = self.state.lock();
        if !state.loaded {
            state.loaded = true;
            let slot = self.slot.as_ref();
            let stored: Stored = slot
                .and_then(|slot| slot.get(KEY))
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                .or_else(|| {
                    // The index before it covered folders: its files.
                    let tags = slot
                        .and_then(|slot| slot.get(KEY_2))
                        .and_then(|bytes| serde_json::from_slice(&bytes).ok())?;
                    Some(Stored {
                        tags,
                        covered: Vec::new(),
                    })
                })
                .unwrap_or_default();
            state.tags = stored
                .tags
                .into_iter()
                .map(|(path, tags)| (PathBuf::from(path), tags))
                .collect();
            state.covered = stored.covered;
        }
        state
    }

    /// What the provider read of files: each path with its tags, none for
    /// a file without. Writes the index soon when that changed it.
    pub fn learn(self: &Arc<Self>, read: Vec<(PathBuf, Vec<String>)>) {
        let changed = {
            let mut state = self.state();
            let mut changed = false;
            for (path, tags) in read {
                if tags.is_empty() {
                    changed |= state.tags.remove(&path).is_some();
                    continue;
                }
                let full = state.tags.len() >= AT_MOST;
                match state.tags.get_mut(&path) {
                    Some(known) if *known == tags => {}
                    Some(known) => {
                        *known = tags;
                        changed = true;
                    }
                    None if !full => {
                        state.tags.insert(path, tags);
                        changed = true;
                    }
                    // A tagged file it cannot hold: no folder is covered.
                    None => {
                        changed |= !state.covered.is_empty();
                        state.covered.clear();
                    }
                }
            }
            changed
        };
        if changed {
            self.write_soon();
        }
    }

    /// Whether it cannot learn another tagged file.
    pub fn is_full(&self) -> bool {
        self.state().tags.len() >= AT_MOST
    }

    /// The coverage of `root`: a folder covered that holds it, if any.
    pub fn covering(&self, root: &Path) -> Option<Covered> {
        self.state()
            .covered
            .iter()
            .find(|c| root.starts_with(&c.root))
            .cloned()
    }

    /// `root` is covered as of `since`, a count having seen `dirs` (the
    /// folders' [`stable_hash`]es), which the next check compares with.
    pub fn cover(self: &Arc<Self>, root: &Path, since: SystemTime, dirs: &[u64]) {
        let Ok(since) = since.duration_since(UNIX_EPOCH) else {
            return;
        };
        if self.is_full() {
            return;
        }
        if let Some(slot) = &self.slot {
            let bytes: Vec<u8> = dirs.iter().flat_map(|d| d.to_le_bytes()).collect();
            if let Err(error) = slot.put(&dirs_key(root), &bytes) {
                tracing::debug!(%error, "tags: the folders counted were not written");
                return;
            }
        }
        {
            let mut state = self.state();
            state.covered.retain(|c| !c.root.starts_with(root));
            state.covered.push(Covered {
                root: root.to_path_buf(),
                since: (since.as_secs(), since.subsec_nanos()),
            });
        }
        self.write_soon();
    }

    /// The folders the count of `root` saw, as [`cover`](Self::cover) kept
    /// them.
    pub fn dirs(&self, root: &Path) -> std::collections::HashSet<u64> {
        self.slot
            .as_ref()
            .and_then(|slot| slot.get(&dirs_key(root)))
            .map(|bytes| {
                bytes
                    .as_chunks::<8>()
                    .0
                    .iter()
                    .map(|c| u64::from_le_bytes(*c))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The tagged files under `root` it knows, by their folder.
    pub fn by_folder(&self, root: &Path) -> HashMap<PathBuf, Vec<PathBuf>> {
        let mut out: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
        for path in self.state().tags.keys().filter(|p| p.starts_with(root)) {
            if let Some(parent) = path.parent() {
                out.entry(parent.to_path_buf())
                    .or_default()
                    .push(path.clone());
            }
        }
        out
    }

    /// What bibli applied moved files: `from` (and what is under it) is
    /// at `to`, or gone. A folder covered that moved is no longer.
    pub fn moved(self: &Arc<Self>, moves: &[(PathBuf, Option<PathBuf>)]) {
        let changed = {
            let mut state = self.state();
            let mut changed = false;
            for (from, to) in moves {
                let under: Vec<PathBuf> = state
                    .tags
                    .keys()
                    .filter(|p| p.starts_with(from))
                    .cloned()
                    .collect();
                for path in under {
                    let tags = state.tags.remove(&path).unwrap_or_default();
                    if let Some(to) = to
                        && let Ok(rest) = path.strip_prefix(from)
                    {
                        let at = if rest.as_os_str().is_empty() {
                            to.clone()
                        } else {
                            to.join(rest)
                        };
                        state.tags.insert(at, tags);
                    }
                    changed = true;
                }
                let before = state.covered.len();
                state.covered.retain(|c| !c.root.starts_with(from));
                changed |= state.covered.len() != before;
            }
            changed
        };
        if changed {
            self.write_soon();
        }
    }

    /// The tags of the files under `root` outside hidden folders, each
    /// with how many files have it (spelled as most of them spell it, in
    /// the order of their names), and how many files have a tag.
    pub fn tally(&self, root: &Path) -> (Vec<(String, usize)>, usize) {
        let mut counts: HashMap<String, (usize, HashMap<&str, usize>)> = HashMap::new();
        let mut files = 0;
        let state = self.state();
        for (_, tags) in state.tags.iter().filter(|(path, _)| {
            *path != root && path.starts_with(root) && !hidden_under(path, root)
        }) {
            files += 1;
            for tag in tags {
                let (n, spellings) = counts.entry(tag.to_lowercase()).or_default();
                *n += 1;
                *spellings.entry(tag.as_str()).or_default() += 1;
            }
        }
        let mut out: Vec<(String, usize)> = counts
            .into_values()
            .filter_map(|(n, spellings)| {
                let spelled = spellings
                    .into_iter()
                    .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(a.0)))?
                    .0;
                Some((spelled.to_owned(), n))
            })
            .collect();
        drop(state);
        out.sort_by_key(|(t, _)| t.to_lowercase());
        (out, files)
    }

    /// The files under `root` known to have `tag`. Those gone since are
    /// forgotten on the way.
    pub fn files_with(self: &Arc<Self>, tag: &str, root: &Path) -> Vec<PathBuf> {
        let found: Vec<PathBuf> = self
            .state()
            .tags
            .iter()
            .filter(|(path, tags)| path.starts_with(root) && tags.iter().any(|t| same_tag(t, tag)))
            .map(|(path, _)| path.clone())
            .collect();
        let (here, gone): (Vec<PathBuf>, Vec<PathBuf>) = found.into_iter().partition(|path| {
            !matches!(std::fs::symlink_metadata(path),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound)
        });
        if !gone.is_empty() {
            let mut state = self.state();
            for path in &gone {
                state.tags.remove(path);
            }
            drop(state);
            self.write_soon();
        }
        here
    }

    /// The `at_most` tags on the most files under `root`, each with how
    /// many, the most used first (then by name); each spelled as most of
    /// its files spell it.
    pub fn top(&self, at_most: usize, root: &Path) -> Vec<(String, usize)> {
        // By lowercase form: the files, and each spelling's count.
        let mut counts: HashMap<String, (usize, HashMap<&str, usize>)> = HashMap::new();
        let state = self.state();
        for (_, tags) in state.tags.iter().filter(|(path, _)| path.starts_with(root)) {
            for tag in tags {
                let (files, spellings) = counts.entry(tag.to_lowercase()).or_default();
                *files += 1;
                *spellings.entry(tag.as_str()).or_default() += 1;
            }
        }
        let mut top: Vec<(String, usize)> = counts
            .into_values()
            .filter_map(|(files, spellings)| {
                let spelled = spellings
                    .into_iter()
                    .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(a.0)))?
                    .0;
                Some((spelled.to_owned(), files))
            })
            .collect();
        drop(state);
        top.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        top.truncate(at_most);
        top
    }

    fn write_soon(self: &Arc<Self>) {
        let Some(app) = self.app.upgrade() else {
            return;
        };
        if self.writing.swap(true, Ordering::AcqRel) {
            return;
        }
        let this = Arc::clone(self);
        app.scheduler.spawn(
            Priority::Background,
            "tags.index",
            self.lifetime.child(),
            move || {
                this.writing.store(false, Ordering::Release);
                this.write();
                if let Some(f) = &*this.on_change.lock() {
                    f(&this);
                }
            },
        );
    }

    fn write(&self) {
        let Some(slot) = &self.slot else { return };
        let bytes = {
            let state = self.state();
            // A path that is not text stays out: a walk finds it. A folder
            // covered whose name is not text cannot be stored either.
            let stored = Stored {
                tags: state
                    .tags
                    .iter()
                    .filter_map(|(path, tags)| Some((path.to_str()?.to_owned(), tags.clone())))
                    .collect(),
                covered: state
                    .covered
                    .iter()
                    .filter(|c| c.root.to_str().is_some())
                    .filter(|c| {
                        !state
                            .tags
                            .keys()
                            .any(|p| p.starts_with(&c.root) && p.to_str().is_none())
                    })
                    .cloned()
                    .collect(),
            };
            serde_json::to_vec(&stored)
        };
        if let Ok(bytes) = bytes
            && let Err(error) = slot.put(KEY, &bytes)
        {
            tracing::debug!(%error, "tags: the index was not written");
        }
    }
}

/// The index as a hint provider.
pub struct Learned(pub Arc<TagIndex>);

impl HintProvider for Learned {
    fn id(&self) -> &'static str {
        "tags.index"
    }
    fn candidates(
        &self,
        key: &str,
        value: &str,
        root: &Path,
        _: &CancelToken,
    ) -> Option<Vec<PathBuf>> {
        (key == keys::TAGS).then(|| self.0.files_with(value, root))
    }
}

/// Baloo, KDE's file index, which indexes the desktop's tags (ours), when
/// the user runs it: asked with `baloosearch`, never started by us.
#[derive(Default)]
pub struct Baloo {
    /// Where its settings and its index are; the user's when `None`.
    pub private: Option<PrivateBaloo>,
}

/// A Baloo of its own (tests): its settings file and its data directory.
pub struct PrivateBaloo {
    pub config: PathBuf,
    pub data_home: PathBuf,
}

/// How long Baloo may take: it answers in milliseconds from its index.
const BALOO_TIMEOUT: Duration = Duration::from_secs(2);

impl Baloo {
    /// `baloosearch`, when Baloo is on: its settings say indexing is
    /// enabled (an index left by a Baloo turned off is out of date).
    fn search_program(&self) -> Option<PathBuf> {
        let config = match &self.private {
            Some(private) => private.config.clone(),
            None => bibli_platform::xdg::config_home()?.join("baloofilerc"),
        };
        let text = std::fs::read_to_string(config).ok()?;
        if !indexing_enabled(&text) {
            return None;
        }
        ["baloosearch6", "baloosearch"]
            .into_iter()
            .find_map(bibli_core::os_path::find_program)
    }
}

/// Whether a `baloofilerc` says indexing is on (the default when unsaid).
fn indexing_enabled(config: &str) -> bool {
    let mut section = "";
    for line in config.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = name;
        } else if section == "Basic Settings"
            && let Some(value) = line.strip_prefix("Indexing-Enabled=")
        {
            return !value.trim().eq_ignore_ascii_case("false");
        }
    }
    true
}

/// Baloo's query for files tagged `tag`; `None` for a tag its query
/// language cannot quote.
fn baloo_query(tag: &str) -> Option<String> {
    (!tag.contains('"') && !tag.contains('\\')).then(|| format!("tag:\"{tag}\""))
}

/// The paths in `baloosearch`'s output: one per line, then a line of
/// timing in the user's language, which is not a path.
fn baloo_paths(stdout: &[u8]) -> Vec<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    stdout
        .split(|b| *b == b'\n')
        .filter(|line| line.starts_with(b"/"))
        .map(|line| PathBuf::from(std::ffi::OsStr::from_bytes(line)))
        .collect()
}

impl HintProvider for Baloo {
    fn id(&self) -> &'static str {
        "tags.baloo"
    }
    fn candidates(
        &self,
        key: &str,
        value: &str,
        root: &Path,
        cancel: &CancelToken,
    ) -> Option<Vec<PathBuf>> {
        if key != keys::TAGS {
            return None;
        }
        let program = self.search_program()?;
        let query = baloo_query(value)?;
        let mut command = std::process::Command::new(program);
        command.arg("-d").arg(root).arg(query);
        if let Some(private) = &self.private {
            command.env("XDG_DATA_HOME", &private.data_home);
        }
        match bibli_platform::process::run(&mut command, cancel, BALOO_TIMEOUT) {
            Ok(stdout) => Some(baloo_paths(&stdout)),
            Err(error) => {
                tracing::debug!(%error, "tags: Baloo did not answer");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index() -> Arc<TagIndex> {
        Arc::new(TagIndex {
            state: Mutex::new(State {
                tags: HashMap::new(),
                covered: Vec::new(),
                loaded: true,
            }),
            slot: None,
            app: Weak::new(),
            lifetime: CancelToken::new(),
            writing: AtomicBool::new(false),
            on_change: Mutex::default(),
        })
    }

    #[test]
    fn the_most_used_tags_come_first_spelled_as_most_files_spell_them() {
        let index = index();
        let file = |i: usize| PathBuf::from(format!("/r/{i}"));
        index.learn(vec![
            (file(0), vec!["Work".into(), "home".into()]),
            (file(1), vec!["work".into(), "Été".into()]),
            (file(2), vec!["Work".into()]),
            (file(3), vec!["été".into(), "zeta".into()]),
            (file(4), vec!["alpha".into()]),
        ]);
        index.learn(vec![(PathBuf::from("/elsewhere/5"), vec!["zeta".into()])]);
        assert_eq!(
            index.top(4, Path::new("/r")),
            [
                ("Work".to_owned(), 3),
                // Two files, one spelling each: the one sorting first.
                ("Été".to_owned(), 2),
                ("alpha".to_owned(), 1),
                ("home".to_owned(), 1),
            ]
        );
        assert!(index.top(0, Path::new("/r")).is_empty());
        assert_eq!(
            index.top(9, Path::new("/elsewhere")),
            [("zeta".to_owned(), 1)]
        );
    }

    #[test]
    fn a_folder_covered_is_remembered_and_moves_carry_tags_along() {
        let tmp = tempfile::Builder::new()
            .prefix("tags-index-")
            .tempdir_in({
                let base = dirs::home_dir().unwrap().join(".cache/bibli-tests");
                std::fs::create_dir_all(&base).unwrap();
                base
            })
            .unwrap();
        let slot = || Some(bibli_core::cache::Cache::new(tmp.path()).namespace("tags"));
        let made = |slot| {
            Arc::new(TagIndex {
                state: Mutex::default(),
                slot,
                app: Weak::new(),
                lifetime: CancelToken::new(),
                writing: AtomicBool::new(false),
                on_change: Mutex::default(),
            })
        };
        let index = made(slot());
        index.learn(vec![
            (PathBuf::from("/r/a"), vec!["x".into()]),
            (PathBuf::from("/r/.hidden/b"), vec!["x".into()]),
            (PathBuf::from("/r/d/c"), vec!["X".into(), "y".into()]),
        ]);
        // Hidden folders are not counted, as a search leaves them out; a
        // tie between spellings goes to the first in order.
        assert_eq!(
            index.tally(Path::new("/r")),
            (vec![("X".into(), 2), ("y".into(), 1)], 2)
        );
        let since = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        index.cover(Path::new("/r"), since, &[7, 9]);
        index.write();
        // Read again, as the next run reads it: what it covers and the
        // folders the count saw.
        let again = made(slot());
        let covered = again.covering(Path::new("/r/d")).unwrap();
        assert_eq!(
            (covered.root.as_path(), covered.since()),
            (Path::new("/r"), since)
        );
        assert_eq!(again.dirs(Path::new("/r")), [7, 9].into());
        assert!(again.covering(Path::new("/s")).is_none());
        // A folder moved carries its files' tags; one gone, its files.
        again.moved(&[
            (PathBuf::from("/r/d"), Some(PathBuf::from("/r/e"))),
            (PathBuf::from("/r/a"), None),
        ]);
        assert_eq!(
            again
                .by_folder(Path::new("/r"))
                .get(Path::new("/r/e"))
                .map(Vec::len),
            Some(1)
        );
        assert_eq!(again.tally(Path::new("/r")).1, 1);
        // A covered folder moved away is no longer covered.
        again.moved(&[(PathBuf::from("/r"), Some(PathBuf::from("/q")))]);
        assert!(again.covering(Path::new("/r")).is_none());
    }

    #[test]
    fn the_index_learns_tags_and_their_removal_and_forgets_the_gone() {
        let tmp = tempfile::tempdir().unwrap();
        let (a, b) = (tmp.path().join("it's a"), tmp.path().join("b"));
        std::fs::write(&a, b"x").unwrap();
        std::fs::write(&b, b"x").unwrap();
        let index = index();
        index.learn(vec![
            (a.clone(), vec!["Work".into(), "home".into()]),
            (b.clone(), vec!["work".into()]),
        ]);
        let mut found = index.files_with("WORK", tmp.path());
        found.sort();
        assert_eq!(found, [b.clone(), a.clone()]);
        assert_eq!(
            index.files_with("home", tmp.path()),
            std::slice::from_ref(&a)
        );
        assert!(
            index
                .files_with("home", &tmp.path().join("elsewhere"))
                .is_empty()
        );
        // Read again without tags: forgotten.
        index.learn(vec![(a.clone(), Vec::new())]);
        assert_eq!(
            index.files_with("work", tmp.path()),
            std::slice::from_ref(&b)
        );
        // Gone from the disk: forgotten when asked.
        std::fs::remove_file(&b).unwrap();
        assert!(index.files_with("work", tmp.path()).is_empty());
        assert!(index.state().tags.is_empty());
    }

    #[test]
    fn past_its_bound_it_learns_no_new_file_but_keeps_those_known_current() {
        let index = index();
        let read: Vec<(PathBuf, Vec<String>)> = (0..AT_MOST + 3)
            .map(|i| (PathBuf::from(format!("/r/{i}")), vec!["t".into()]))
            .collect();
        index.learn(read);
        assert_eq!(index.state().tags.len(), AT_MOST);
        index.learn(vec![(PathBuf::from("/r/0"), vec!["u".into()])]);
        assert_eq!(index.state().tags[Path::new("/r/0")], ["u"]);
    }

    /// Against a real Baloo of its own: a private D-Bus session, settings
    /// and index in a temporary directory, indexing only the fixture.
    /// Needs `baloo_file`, `baloosearch6` and `dbus-run-session`:
    /// `cargo nextest run -p bibli-plugin-tags --run-ignored only baloo`.
    #[test]
    #[ignore = "starts a private Baloo"]
    fn a_private_baloo_names_the_tagged_files() {
        use std::os::unix::process::CommandExt;
        let search = bibli_core::os_path::find_program("baloosearch6").expect("baloosearch6");
        let daemon = std::fs::canonicalize(&search)
            .ok()
            .and_then(|real| Some(real.parent()?.parent()?.join("libexec/kf6/baloo_file")))
            .filter(|p| p.exists())
            .or_else(|| {
                [
                    "/usr/libexec/kf6/baloo_file",
                    "/usr/lib/x86_64-linux-gnu/libexec/kf6/baloo_file",
                ]
                .into_iter()
                .map(PathBuf::from)
                .find(|p| p.exists())
            })
            .expect("baloo_file");
        let base = dirs::home_dir().unwrap().join(".cache").join("bibli-tests");
        std::fs::create_dir_all(&base).unwrap();
        let tmp = tempfile::Builder::new()
            .prefix("baloo-")
            .tempdir_in(base)
            .unwrap();
        let tree = tmp.path().join("tree");
        let nested = tree.join("it's nested");
        std::fs::create_dir_all(&nested).unwrap();
        for (path, tags) in [
            (tree.join("a.txt"), Some("work,to do")),
            (nested.join("b.txt"), Some("Work")),
            (tree.join("c.txt"), Some("home")),
            (tree.join("d.txt"), None),
        ] {
            std::fs::write(&path, b"x").unwrap();
            if let Some(tags) = tags {
                xattr::set(&path, "user.xdg.tags", tags.as_bytes()).unwrap();
            }
        }
        let (config, data, cache, run) = (
            tmp.path().join("config"),
            tmp.path().join("data"),
            tmp.path().join("cache"),
            tmp.path().join("run"),
        );
        for dir in [&config, &data, &cache, &run] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let settings = config.join("baloofilerc");
        std::fs::write(
            &settings,
            format!(
                "[Basic Settings]\nIndexing-Enabled=true\n\n[General]\nfolders[$e]={}/\nonly basic indexing=true\n",
                tree.display()
            ),
        )
        .unwrap();
        let mut child = std::process::Command::new("dbus-run-session")
            .arg("--")
            .arg(&daemon)
            .env_remove("DBUS_SESSION_BUS_ADDRESS")
            .env("XDG_CONFIG_HOME", &config)
            .env("XDG_DATA_HOME", &data)
            .env("XDG_CACHE_HOME", &cache)
            .env("XDG_RUNTIME_DIR", &run)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .process_group(0)
            .spawn()
            .expect("dbus-run-session");
        let baloo = Baloo {
            private: Some(PrivateBaloo {
                config: settings,
                data_home: data,
            }),
        };
        let cancel = CancelToken::new();
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        let mut found = Vec::new();
        while std::time::Instant::now() < deadline {
            found = baloo
                .candidates(keys::TAGS, "WORK", &tree, &cancel)
                .unwrap_or_default();
            if found.len() == 2 {
                break;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        let only_nested = baloo.candidates(keys::TAGS, "work", &nested, &cancel);
        let two_words = baloo.candidates(keys::TAGS, "to do", &tree, &cancel);
        // The daemon and its bus, whatever the assertions say.
        let group = rustix::process::Pid::from_child(&child);
        let _ = rustix::process::kill_process_group(group, rustix::process::Signal::TERM);
        let _ = child.wait();
        found.sort();
        assert_eq!(found, [tree.join("a.txt"), nested.join("b.txt")]);
        assert_eq!(only_nested.unwrap(), [nested.join("b.txt")]);
        assert_eq!(two_words.unwrap(), [tree.join("a.txt")]);
    }

    #[test]
    fn baloo_is_asked_only_while_it_indexes_and_its_answers_are_paths() {
        assert!(indexing_enabled("[General]\nfolders[$e]=$HOME/\n"));
        assert!(indexing_enabled(
            "[Basic Settings]\nIndexing-Enabled=true\n"
        ));
        assert!(!indexing_enabled(
            "[General]\nIndexing-Enabled=true\n[Basic Settings]\nIndexing-Enabled=false\n"
        ));
        assert_eq!(baloo_query("to do").unwrap(), "tag:\"to do\"");
        assert_eq!(baloo_query("a\"b"), None);
        assert_eq!(
            baloo_paths(b"/r/it's a.txt\n/r/b\nTemps \xc3\xa9coul\xc3\xa9 : 0,1 ms\n"),
            [PathBuf::from("/r/it's a.txt"), PathBuf::from("/r/b")]
        );
    }
}
