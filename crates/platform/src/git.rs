//! A repository's history and a commit's patch, read by `skwir-git-worker`
//! in a process of its own (ADR-133): gix and its walks are skwir's only
//! while a history is shown, and nothing a repository names is run. Where
//! a repository is, is found here, by the names it leaves on the disk
//! (`Repo::find`), and kept a minute by folder.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use skwir_core::CancelToken;

/// The helper. Plugins that require it name this, not the literals.
pub const WORKER: crate::process::Worker = crate::process::Worker {
    variable: "SKWIR_GIT_WORKER",
    executable: "skwir-git-worker",
};

/// Commits a history lists quickly; Full preview lists up to `WHOLE`.
pub const QUICK: usize = 500;
/// Commits a history lists at most, asked whole.
pub const WHOLE: usize = 100_000;
/// A patch's bytes at most, quickly and asked whole.
pub const PATCH: u64 = 2 * 1024 * 1024;
pub const PATCH_WHOLE: u64 = 64 * 1024 * 1024;
/// How long the helper reads before it answers what it found.
const DEADLINE: Duration = Duration::from_secs(10);
const DEADLINE_WHOLE: Duration = Duration::from_secs(50);
/// A history's answer at most: a commit's ids, names and summary, escaped.
const ANSWER_PER_COMMIT: u64 = 1536;
/// How long where a folder's repository is, is kept.
const KEPT: Duration = Duration::from_secs(60);
/// Folders kept at most.
const KEPT_FOLDERS: usize = 256;
/// The folders a repository's alternates name, read at most.
const ALTERNATES: usize = 8;
/// Folders walked up at most, looking for a repository.
const DEPTH: usize = 64;

/// A repository, as found on the disk: what the helper is to read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Repo {
    /// Its git folder: `.git`, a linked worktree's own, or a bare one.
    pub git_dir: PathBuf,
    /// Where its files are checked out; `None` for a bare repository.
    pub work_tree: Option<PathBuf>,
    /// What the helper reads: the git folder, the common one a linked
    /// worktree shares, and the folders their alternates name.
    pub readable: Vec<PathBuf>,
}

/// Where each folder's repository was found, and when.
static FOUND: Mutex<Option<HashMap<PathBuf, (Instant, Option<Repo>)>>> = Mutex::new(None);

impl Repo {
    /// The repository `path` is in (a file's folder's, a folder's own),
    /// found by its `.git` folder or file in it or above it, or by being a
    /// bare one itself; kept a minute by folder, so stepping through a
    /// folder's files asks the disk once. Never a path inside a git folder.
    /// Reads the disk: on a worker, in the path's lane.
    pub fn find(path: &Path, folder: bool) -> Option<Repo> {
        let start = if folder { path } else { path.parent()? };
        let now = Instant::now();
        if let Some((at, found)) = FOUND
            .lock()
            .ok()
            .and_then(|kept| kept.as_ref()?.get(start).cloned())
            && now.duration_since(at) < KEPT
        {
            return found;
        }
        let found = Self::look(start);
        if let Ok(mut kept) = FOUND.lock() {
            let kept = kept.get_or_insert_with(HashMap::new);
            if kept.len() >= KEPT_FOLDERS {
                kept.retain(|_, (at, _)| now.duration_since(*at) < KEPT);
                if kept.len() >= KEPT_FOLDERS {
                    kept.clear();
                }
            }
            kept.insert(start.to_owned(), (now, found.clone()));
        }
        found
    }

    fn look(start: &Path) -> Option<Repo> {
        if start
            .components()
            .any(|part| part.as_os_str() == std::ffi::OsStr::new(".git"))
        {
            return None;
        }
        if is_git_dir(start) {
            return Some(Self::of(start.to_owned(), None));
        }
        for folder in start.ancestors().take(DEPTH) {
            let dot = folder.join(".git");
            let Ok(meta) = std::fs::metadata(&dot) else {
                continue;
            };
            let git_dir = if meta.is_dir() {
                dot
            } else if meta.is_file() {
                git_file(&dot)?
            } else {
                continue;
            };
            return is_git_dir(&git_dir).then(|| Self::of(git_dir, Some(folder.to_owned())));
        }
        None
    }

    fn of(git_dir: PathBuf, work_tree: Option<PathBuf>) -> Repo {
        let common = read_small(&git_dir.join("commondir"))
            .map_or_else(|| git_dir.clone(), |text| git_dir.join(text.trim()));
        let mut readable = vec![git_dir.clone()];
        if common != git_dir {
            readable.push(common.clone());
        }
        let objects = common.join("objects");
        if let Some(text) = read_small(&objects.join("info").join("alternates")) {
            readable.extend(
                text.lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty() && !line.starts_with('#'))
                    .take(ALTERNATES)
                    .map(|line| objects.join(line)),
            );
        }
        Repo {
            git_dir,
            work_tree,
            readable,
        }
    }

    /// `path` as the repository names it: `/`-separated from its top, ""
    /// for the top itself or a bare repository. `None` outside it, or for a
    /// name not written in UTF-8, which the helper is not handed.
    pub fn relative(&self, path: &Path) -> Option<String> {
        let Some(top) = &self.work_tree else {
            return Some(String::new());
        };
        let inside = path.strip_prefix(top).ok()?;
        let parts: Option<Vec<&str>> = inside
            .components()
            .map(|part| part.as_os_str().to_str())
            .collect();
        Some(parts?.join("/"))
    }
}

/// Whether `dir` is a git folder: its `HEAD`, its objects and its
/// references, as git tells one.
fn is_git_dir(dir: &Path) -> bool {
    dir.join("HEAD").is_file() && dir.join("objects").is_dir() && dir.join("refs").is_dir()
        || dir.join("commondir").is_file() && dir.join("HEAD").is_file()
}

/// The git folder a `.git` file names (`gitdir: …`, a linked worktree's or
/// a submodule's), from the folder holding it.
fn git_file(dot: &Path) -> Option<PathBuf> {
    let text = read_small(dot)?;
    let named = text.lines().next()?.strip_prefix("gitdir:")?.trim();
    Some(dot.parent()?.join(named))
}

/// A small text file whole: one of a repository's pointers.
fn read_small(path: &Path) -> Option<String> {
    let bytes = skwir_core::fs::file::read_regular(path, 4096).ok()?;
    String::from_utf8(bytes).ok()
}

/// A commit of a history, as the helper reads it.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Commit {
    pub id: String,
    /// Its id as git shortens it (`--abbrev-commit`).
    #[serde(default)]
    pub short: String,
    pub parents: Vec<String>,
    pub author: String,
    pub email: String,
    /// Seconds since the epoch, and the author's offset from UTC in
    /// seconds east.
    pub time: i64,
    pub offset: i32,
    pub summary: String,
    /// The branches and tags at it: a local branch by its name (`main`),
    /// a remote-tracking one and a tag after what they are
    /// (`remote: origin/main`, `tag: v1`).
    #[serde(default)]
    pub refs: Vec<String>,
}

/// A history, newest first, parents after their children.
#[derive(Clone, Debug, Deserialize)]
pub struct Log {
    /// The branch the repository is on, if it is on one.
    pub head: Option<String>,
    pub commits: Vec<Commit>,
    /// More commits are left past the limit.
    pub more: bool,
    /// The helper stopped at its deadline.
    pub stopped: bool,
}

/// What a patch written says of itself.
#[derive(Clone, Debug, Deserialize)]
pub struct Patched {
    pub files: usize,
    pub bytes: u64,
    /// It goes on past what was written.
    pub cut: bool,
}

#[derive(Serialize)]
#[serde(tag = "ask", rename_all = "lowercase")]
enum Ask {
    Log {
        git_dir: PathBuf,
        path: String,
        limit: usize,
        deadline_ms: u64,
    },
    Patch {
        git_dir: PathBuf,
        commit: String,
        path: String,
        out: PathBuf,
        bytes: u64,
        deadline_ms: u64,
    },
}

/// The commits of `repo` that changed `path` (as `Repo::relative` names
/// it; "" for all), newest first: `QUICK` of them, or `WHOLE` asked whole.
pub fn log(repo: &Repo, path: &str, whole: bool, cancel: &CancelToken) -> anyhow::Result<Log> {
    let limit = if whole { WHOLE } else { QUICK };
    let deadline = if whole { DEADLINE_WHOLE } else { DEADLINE };
    let work = tempfile::tempdir()?;
    let output = (limit as u64 + 1) * ANSWER_PER_COMMIT;
    let stdout = run(repo, work.path(), deadline, output, cancel, |_| Ask::Log {
        git_dir: repo.git_dir.clone(),
        path: path.to_owned(),
        limit,
        deadline_ms: millis(deadline),
    })?;
    Ok(serde_json::from_slice(&stdout)?)
}

/// The patch of commit `id` under `path`, written in `into` (a folder the
/// caller owns) as `<id>.patch`: where it is, and what it holds.
pub fn patch(
    repo: &Repo,
    id: &str,
    path: &str,
    into: &Path,
    whole: bool,
    cancel: &CancelToken,
) -> anyhow::Result<(PathBuf, Patched)> {
    anyhow::ensure!(
        !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_hexdigit()),
        "Not a commit: {id}"
    );
    let name = format!("{}.patch", &id[..id.len().min(12)]);
    let deadline = if whole { DEADLINE_WHOLE } else { DEADLINE };
    // Written in its working folder, `into`, named as the helper sees it.
    let stdout = run(repo, into, deadline, 4096, cancel, |inside| Ask::Patch {
        git_dir: repo.git_dir.clone(),
        commit: id.to_owned(),
        path: path.to_owned(),
        out: inside.join(&name),
        bytes: if whole { PATCH_WHOLE } else { PATCH },
        deadline_ms: millis(deadline),
    })?;
    let patched: Patched = serde_json::from_slice(&stdout)?;
    let written = into.join(&name);
    anyhow::ensure!(written.is_file(), "The patch was not written");
    Ok((written, patched))
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// Run the helper isolated, the repository's folders readable and `work`
/// its working folder, on what `ask` makes of that folder as it sees it
/// (`/work` in a sandbox): what it printed, or why it failed.
fn run(
    repo: &Repo,
    work: &Path,
    deadline: Duration,
    output: u64,
    cancel: &CancelToken,
    ask: impl FnOnce(&Path) -> Ask,
) -> anyhow::Result<Vec<u8>> {
    let worker = WORKER.require("show a history of commits")?;
    let readable: Vec<&Path> = repo.readable.iter().map(PathBuf::as_path).collect();
    let mut untrusted = crate::sandbox::fed_reading_writing(
        &worker,
        work,
        crate::sandbox::Otherwise::Run,
        &readable,
        &[],
    )?;
    let input = serde_json::to_vec(&ask(&untrusted.work))?;
    let finished = crate::process::run_capped(
        &mut untrusted.command,
        Some(&input),
        cancel,
        deadline + Duration::from_secs(5),
        output,
    )
    .map_err(|error| crate::process::explain(&worker, cancel, error))?;
    anyhow::ensure!(
        finished.status.success(),
        "The history cannot be read: {}",
        finished.stderr.trim()
    );
    Ok(finished.stdout)
}

#[cfg(test)]
mod tests {
    use super::Repo;

    /// A repository is found by its `.git` folder above, or by a `.git`
    /// file naming its git folder (a linked worktree's), never from inside
    /// a git folder; a path in it is named from its top, `/`-separated.
    #[test]
    fn a_repository_is_found_by_its_git_folder_above() {
        let dir = tempfile::tempdir().unwrap();
        let top = dir.path().join("project");
        let git = top.join(".git");
        for part in ["objects", "refs"] {
            std::fs::create_dir_all(git.join(part)).unwrap();
        }
        std::fs::write(git.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        let docs = top.join("docs").join("guide");
        std::fs::create_dir_all(&docs).unwrap();
        std::fs::write(docs.join("a.md"), "# A").unwrap();
        let repo = Repo::find(&docs.join("a.md"), false).unwrap();
        assert_eq!(repo.git_dir, git);
        assert_eq!(repo.work_tree.as_deref(), Some(top.as_path()));
        assert_eq!(repo.readable, std::slice::from_ref(&git));
        assert_eq!(repo.relative(&docs).as_deref(), Some("docs/guide"));
        assert_eq!(repo.relative(&top).as_deref(), Some(""));
        assert!(Repo::find(&git.join("objects"), true).is_none(), "inside");
        assert_eq!(Repo::find(&git, true), None, "a .git folder itself");
        // A linked worktree: a `.git` file, its folder sharing the main one's.
        let linked = dir.path().join("linked");
        let own = git.join("worktrees").join("linked");
        std::fs::create_dir_all(&own).unwrap();
        std::fs::write(own.join("HEAD"), "ref: refs/heads/side\n").unwrap();
        std::fs::write(own.join("commondir"), "../..\n").unwrap();
        std::fs::create_dir_all(&linked).unwrap();
        std::fs::write(linked.join(".git"), format!("gitdir: {}\n", own.display())).unwrap();
        let repo = Repo::find(&linked, true).unwrap();
        assert_eq!(repo.git_dir, own);
        assert_eq!(repo.readable, [own.clone(), own.join("../..")]);
        let elsewhere = tempfile::tempdir().unwrap();
        assert!(
            Repo::find(elsewhere.path(), true).is_none(),
            "no repository"
        );
    }
}
