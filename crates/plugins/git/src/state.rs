//! The files' Git state in the views (ADR-134): what a folder's files are
//! to their repository, read by the helper (`git::status`) for the pane
//! that shows the folder (`vcs::StatesProvider`). Each repository asked of
//! has its git folder followed, so a commit, a `git add`, a checkout or a
//! merge made elsewhere (a terminal, an editor) is told (`generation`);
//! what changes below a folder shown has no watch of its own, and is asked
//! again when a window comes to the front (`looked_at`).
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use skwir_core::vcs::{Answer, StatesProvider};
use skwir_core::{App, CancelToken};
use skwir_platform::git::{self, Repo};

/// The setting: whether the views show the files' Git state.
pub const SHOWN: &str = "git.states";
/// The provider's id.
pub const ID: &str = "git.states";
/// Repositories followed at most: the least recently asked of is let go.
const FOLLOWED: usize = 32;
/// How long a repository's changes settle before they are told: a commit
/// writes its index, its reference and its log one after the other.
const SETTLE: Duration = Duration::from_millis(300);
/// The names in a git folder (or its `logs`) whose change says the files'
/// state may have: the index, where `HEAD` is, the references packed, a
/// merge or a pick under way.
const TOLD: [&str; 6] = [
    "index",
    "HEAD",
    "packed-refs",
    "MERGE_HEAD",
    "CHERRY_PICK_HEAD",
    "REVERT_HEAD",
];

/// What the watcher's thread and the provider share: each repository's
/// generation by its key, the folders followed by the key they tell, and
/// the generation every answer moves with.
#[derive(Default)]
struct Shared {
    generations: Mutex<HashMap<u64, u64>>,
    folders: Mutex<HashMap<PathBuf, u64>>,
    changed: Mutex<HashSet<u64>>,
    everyone: AtomicU64,
}

/// A repository followed: its key, the folders watched for it, and when it
/// was last asked of (an order of asking).
struct Followed {
    key: u64,
    watched: Vec<PathBuf>,
    asked: u64,
}

pub struct States {
    shown: AtomicBool,
    shared: Arc<Shared>,
    /// Followed repositories by their git folder, and the watcher of
    /// their folders (`None` where the system gives none: what changes is
    /// then asked again only when a window comes to the front).
    followed: Mutex<(HashMap<PathBuf, Followed>, Option<skwir_core::fs::Settled>)>,
    next_key: AtomicU64,
    asking: AtomicU64,
}

impl States {
    pub fn new(app: &App) -> Self {
        let shared = Arc::new(Shared::default());
        let relevant = {
            let shared = Arc::clone(&shared);
            move |path: &Path| {
                let told = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| TOLD.contains(&name));
                let Some(folder) = path.parent() else {
                    return false;
                };
                let key = shared
                    .folders
                    .lock()
                    .ok()
                    .and_then(|f| f.get(folder).copied());
                match key {
                    Some(key) if told => {
                        if let Ok(mut changed) = shared.changed.lock() {
                            changed.insert(key);
                        }
                        true
                    }
                    _ => false,
                }
            }
        };
        let then = {
            let shared = Arc::clone(&shared);
            let weak = app.weak();
            move || {
                let Some(app) = weak.upgrade() else {
                    return false;
                };
                let changed: Vec<u64> = shared
                    .changed
                    .lock()
                    .map(|mut changed| changed.drain().collect())
                    .unwrap_or_default();
                if let Ok(mut generations) = shared.generations.lock() {
                    for key in changed {
                        *generations.entry(key).or_default() += 1;
                    }
                }
                app.wake();
                true
            }
        };
        let watcher = skwir_core::fs::Settled::start("skwir-git", SETTLE, relevant, then);
        States {
            shown: AtomicBool::new(app.settings.get::<bool>(SHOWN).unwrap_or(true)),
            shared,
            followed: Mutex::new((HashMap::new(), watcher)),
            next_key: AtomicU64::new(1),
            asking: AtomicU64::new(0),
        }
    }

    /// The setting changed: every answer is asked again, saying nothing
    /// while it is off.
    pub fn settings_changed(&self, app: &App) {
        self.shown.store(
            app.settings.get::<bool>(SHOWN).unwrap_or(true),
            Ordering::Release,
        );
        self.looked_at();
    }

    /// A window came to the front: what changed below the folders shown,
    /// which no watch tells, is asked again.
    pub fn looked_at(&self) {
        self.shared.everyone.fetch_add(1, Ordering::AcqRel);
    }

    /// The key of `repo`'s answers, its git folders followed from now on:
    /// the least recently asked of let go past `FOLLOWED`.
    fn follow(&self, repo: &Repo) -> u64 {
        let asked = self.asking.fetch_add(1, Ordering::AcqRel);
        let Ok(mut followed) = self.followed.lock() else {
            return 0;
        };
        let (repos, watcher) = &mut *followed;
        if let Some(known) = repos.get_mut(&repo.git_dir) {
            known.asked = asked;
            return known.key;
        }
        if repos.len() >= FOLLOWED
            && let Some(oldest) = repos
                .iter()
                .min_by_key(|(_, known)| known.asked)
                .map(|(dir, _)| dir.clone())
            && let Some(gone) = repos.remove(&oldest)
        {
            if let Ok(mut folders) = self.shared.folders.lock() {
                for folder in &gone.watched {
                    folders.remove(folder);
                }
            }
            if let Some(watcher) = watcher {
                for folder in &gone.watched {
                    watcher.unwatch(folder);
                }
            }
        }
        let key = self.next_key.fetch_add(1, Ordering::AcqRel);
        // Its own folder (its index, its `HEAD`), its log of where `HEAD`
        // went (a commit on a branch moves no file of the folder itself),
        // and the common folder a linked worktree shares (its packed
        // references).
        let mut dirs = vec![repo.git_dir.clone(), repo.git_dir.join("logs")];
        if repo.common_dir != repo.git_dir {
            dirs.push(repo.common_dir.clone());
        }
        if let Ok(mut folders) = self.shared.folders.lock() {
            for folder in &dirs {
                folders.insert(folder.clone(), key);
            }
        }
        if let Some(watcher) = watcher {
            for folder in &dirs {
                watcher.watch(folder);
            }
        }
        repos.insert(
            repo.git_dir.clone(),
            Followed {
                key,
                watched: dirs,
                asked,
            },
        );
        key
    }
}

impl StatesProvider for States {
    fn id(&self) -> &'static str {
        ID
    }

    /// A folder in a repository's checked-out files, read by the helper
    /// from the highest folder shown above it in the same repository (a
    /// columns view's), so one reading marks them all, and what the
    /// folders opened in place below it hold that is ignored; a bare repository,
    /// a folder in none, or the setting off say nothing. What the helper
    /// stopped short of (its limits) is said not whole.
    fn states(
        &self,
        folder: &Path,
        above: &[PathBuf],
        opened: &[PathBuf],
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<Answer>> {
        if !self.shown.load(Ordering::Acquire) {
            return Ok(None);
        }
        let Some(repo) = Repo::find(folder, true) else {
            return Ok(None);
        };
        let Some(work_tree) = &repo.work_tree else {
            return Ok(None);
        };
        let root = above
            .iter()
            .filter(|up| {
                up.starts_with(work_tree)
                    && folder.starts_with(up)
                    && Repo::find(up, true).as_ref() == Some(&repo)
            })
            .min_by_key(|up| up.components().count())
            .map_or(folder, PathBuf::as_path);
        let Some(relative) = repo.relative(root) else {
            return Ok(None);
        };
        // The folders opened in place below, as the repository names them.
        let open: Vec<String> = opened
            .iter()
            .filter(|below| below.starts_with(root))
            .filter_map(|below| repo.relative(below))
            .collect();
        let key = self.follow(&repo);
        let status = git::status(&repo, &relative, &open, cancel)?;
        Ok(Some(Answer {
            root: root.to_owned(),
            folder: status.folder,
            states: status.states,
            whole: !status.cut && !status.stopped,
            key,
        }))
    }

    fn generation(&self, key: u64) -> u64 {
        let own = self
            .shared
            .generations
            .lock()
            .ok()
            .and_then(|generations| generations.get(&key).copied())
            .unwrap_or(0);
        own.wrapping_add(self.shared.everyone.load(Ordering::Acquire))
    }
}
