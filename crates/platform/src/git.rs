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
use skwir_core::vcs::State;

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
/// Paths a status says at most: past them, the rest is not said.
const STATES: usize = 20_000;
/// A status's answer at most, per path: its name escaped, and its state.
const ANSWER_PER_STATE: u64 = 1024;
/// How long a status reads before it answers what it found.
const STATUS_DEADLINE: Duration = Duration::from_secs(10);

/// A repository, as found on the disk: what the helper is to read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Repo {
    /// Its git folder: `.git`, a linked worktree's own, or a bare one.
    pub git_dir: PathBuf,
    /// Where its files are checked out; `None` for a bare repository.
    pub work_tree: Option<PathBuf>,
    /// The folder its references and objects are in: its git folder, or
    /// the main one a linked worktree shares.
    pub common_dir: PathBuf,
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
            common_dir: common,
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
    /// In a file's history, the file's name in this commit where it had
    /// another than the one asked (before a rename), as the repository
    /// names it from its top.
    #[serde(default)]
    pub path: Option<String>,
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

/// What the files under a folder are to its repository, as the helper
/// reads them.
#[derive(Clone, Debug, Deserialize)]
pub struct Status {
    /// The folder itself, new or ignored as a whole (all it holds is too).
    pub folder: Option<State>,
    /// Each path not as `HEAD` has it, `/`-separated from the folder, at
    /// its strongest: a file (one gone from the disk too), or a folder new
    /// or ignored whole.
    pub states: Vec<(String, State)>,
    /// More paths than the helper says were found.
    pub cut: bool,
    /// The helper stopped at its deadline.
    pub stopped: bool,
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
    Status {
        git_dir: PathBuf,
        work_tree: PathBuf,
        path: String,
        excludes_file: Option<PathBuf>,
        user: std::collections::BTreeMap<String, String>,
        limit: usize,
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
    let stdout = run(
        repo,
        &[],
        Ran::History,
        work.path(),
        deadline,
        output,
        cancel,
        |_| Ask::Log {
            git_dir: repo.git_dir.clone(),
            path: path.to_owned(),
            limit,
            deadline_ms: millis(deadline),
        },
    )?;
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
    let stdout = run(
        repo,
        &[],
        Ran::History,
        into,
        deadline,
        4096,
        cancel,
        |inside| Ask::Patch {
            git_dir: repo.git_dir.clone(),
            commit: id.to_owned(),
            path: path.to_owned(),
            out: inside.join(&name),
            bytes: if whole { PATCH_WHOLE } else { PATCH },
            deadline_ms: millis(deadline),
        },
    )?;
    let patched: Patched = serde_json::from_slice(&stdout)?;
    let written = into.join(&name);
    anyhow::ensure!(written.is_file(), "The patch was not written");
    Ok((written, patched))
}

/// What the files under `path` (as `Repo::relative` names it; "" for the
/// whole work tree) are to `repo` (ADR-134), as `git status` tells them.
/// The helper reads the work tree, the repository's folders and the user's
/// file of patterns ignored everywhere, and nothing else; what the user's
/// configuration says of how files are compared it is told (`User`).
pub fn status(repo: &Repo, path: &str, cancel: &CancelToken) -> anyhow::Result<Status> {
    let work_tree = repo
        .work_tree
        .clone()
        .ok_or_else(|| anyhow::anyhow!("A bare repository has no files checked out"))?;
    let User {
        excludes_file,
        settings,
    } = User::read();
    let mut also = vec![work_tree.clone()];
    also.extend(excludes_file.clone());
    let work = tempfile::tempdir()?;
    let output = (STATES as u64 + 1) * ANSWER_PER_STATE;
    let stdout = run(
        repo,
        &also,
        Ran::Status,
        work.path(),
        STATUS_DEADLINE,
        output,
        cancel,
        |_| Ask::Status {
            git_dir: repo.git_dir.clone(),
            work_tree,
            path: path.to_owned(),
            excludes_file,
            user: settings,
            limit: STATES,
            deadline_ms: millis(STATUS_DEADLINE),
        },
    )?;
    Ok(serde_json::from_slice(&stdout)?)
}

/// The settings of the user's configuration that change what `git status`
/// says of a file (ADR-134): how a file is compared with what the index
/// kept of it, and whether what is not tracked is told. The helper takes
/// each where the repository's own configuration says nothing, as git
/// lets a repository's word win over the user's.
const USER_SETTINGS: [&str; 6] = [
    "core.filemode",
    "core.symlinks",
    "core.ignorecase",
    "core.trustctime",
    "core.checkstat",
    "status.showuntrackedfiles",
];

/// What the user's configuration tells the helper, which is isolated from
/// it: read as text from `$XDG_CONFIG_HOME/git/config`, then
/// `~/.gitconfig`, so the latter's word wins, as git reads them; their
/// includes not followed.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct User {
    /// The file of patterns ignored in every repository: `core.excludesFile`,
    /// else `$XDG_CONFIG_HOME/git/ignore`; `None` where there is none.
    pub excludes_file: Option<PathBuf>,
    /// The `USER_SETTINGS` it sets, by their names in lowercase, a name
    /// without a value said `true`, as git takes it.
    pub settings: std::collections::BTreeMap<String, String>,
}

impl User {
    pub fn read() -> User {
        let home = skwir_core::os_path::home();
        let xdg = crate::xdg::config_home().map(|config| config.join("git"));
        let mut found = std::collections::BTreeMap::new();
        for config in [
            xdg.as_ref().map(|git| git.join("config")),
            home.as_ref().map(|home| home.join(".gitconfig")),
        ]
        .into_iter()
        .flatten()
        {
            if let Some(text) = read_config(&config) {
                settings_in(&text, &mut found);
            }
        }
        let named = found
            .remove("core.excludesfile")
            .filter(|value| !value.is_empty());
        let file = match named {
            Some(value) => match value.strip_prefix("~/") {
                Some(rest) => home.map(|home| home.join(rest)),
                None => Some(PathBuf::from(value)),
            },
            None => xdg.map(|git| git.join("ignore")),
        };
        User {
            excludes_file: file.filter(|file| file.is_absolute() && file.is_file()),
            settings: found,
        }
    }
}

/// A configuration file whole, a user's at most this long.
fn read_config(path: &Path) -> Option<String> {
    let bytes = skwir_core::fs::file::read_regular(path, 1 << 20).ok()?;
    String::from_utf8(bytes).ok()
}

/// The `core.excludesFile` and `USER_SETTINGS` a git configuration's text
/// sets, into `found`, a later one's word winning: names case ignored, a
/// value's quotes and a comment after it taken off, a name alone `true`.
fn settings_in(text: &str, found: &mut std::collections::BTreeMap<String, String>) {
    let mut section = String::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(header) = line.strip_prefix('[') {
            // A subsection (`[remote "origin"]`) is no section asked of.
            section = match header.split_once(']') {
                Some((name, _)) if !name.contains(['"', ' ']) => name.to_ascii_lowercase(),
                _ => String::new(),
            };
            continue;
        }
        if section.is_empty() || line.is_empty() || line.starts_with(['#', ';']) {
            continue;
        }
        let (key, value) = line.split_once('=').unwrap_or((line, "true"));
        let name = format!("{section}.{}", key.trim().to_ascii_lowercase());
        if name != "core.excludesfile" && !USER_SETTINGS.contains(&name.as_str()) {
            continue;
        }
        let value = value.trim();
        let value = match value.strip_prefix('"') {
            Some(quoted) => quoted.split_once('"').map_or(quoted, |(inside, _)| inside),
            None => value.split(['#', ';']).next().unwrap_or_default().trim(),
        };
        found.insert(name, value.to_owned());
    }
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// What the helper is run for: what a failure says could not be done.
#[derive(Clone, Copy)]
enum Ran {
    History,
    Status,
}

impl Ran {
    /// What it needs the helper for, as `Worker::require` says it.
    fn need(self) -> &'static str {
        match self {
            Ran::History => "show a history of commits",
            Ran::Status => "show the files' Git state",
        }
    }

    /// What a failure says could not be read.
    fn failed(self) -> &'static str {
        match self {
            Ran::History => "The history cannot be read",
            Ran::Status => "The files' Git state cannot be read",
        }
    }
}

/// Run the helper isolated, the repository's folders and `also` readable
/// and `work` its working folder, on what `ask` makes of that folder as it
/// sees it (`/work` in a sandbox): what it printed, or why it failed.
#[allow(clippy::too_many_arguments)]
fn run(
    repo: &Repo,
    also: &[PathBuf],
    ran: Ran,
    work: &Path,
    deadline: Duration,
    output: u64,
    cancel: &CancelToken,
    ask: impl FnOnce(&Path) -> Ask,
) -> anyhow::Result<Vec<u8>> {
    let worker = WORKER.require(ran.need())?;
    let readable: Vec<&Path> = repo
        .readable
        .iter()
        .chain(also)
        .map(PathBuf::as_path)
        .collect();
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
        "{}: {}",
        ran.failed(),
        finished.stderr.trim()
    );
    Ok(finished.stdout)
}

#[cfg(test)]
mod tests {
    use super::Repo;

    /// The user's settings that change a file's state are read as git reads
    /// them: names in any case, a later word winning, a name alone true, a
    /// value's quotes and comment taken off; another section's not.
    #[test]
    fn the_user_s_settings_are_read_as_git_reads_them() {
        let mut found = std::collections::BTreeMap::new();
        super::settings_in(
            "[core]\n\tfileMode = false\n\texcludesFile = \"~/my ignore\" ; mine\n\
             [status]\n\tshowUntrackedFiles = no # dotfiles\n\
             [remote \"origin\"]\n\tfilemode = true\n\
             [core]\n\tignoreCase\n",
            &mut found,
        );
        super::settings_in("[core]\nfilemode = true\n", &mut found);
        let found: Vec<(&str, &str)> = found
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        assert_eq!(
            found,
            [
                ("core.excludesfile", "~/my ignore"),
                ("core.filemode", "true"),
                ("core.ignorecase", "true"),
                ("status.showuntrackedfiles", "no"),
            ]
        );
    }

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
