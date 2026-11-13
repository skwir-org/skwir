//! A repository's history and a commit's patch, read by gitoxide in a
//! process of its own (ADR-133): skwir runs it isolated, the repository's
//! git folder readable and nothing else, and it runs nothing a repository
//! names (no hook, filter, converter or pager is built). Reads one request
//! as JSON on standard input, at most 64 KiB:
//!
//! ```json
//! {"ask": "log", "git_dir": "/r/.git", "path": "crates/ui", "limit": 500, "deadline_ms": 10000}
//! ```
//!
//! answered with the commits that reached `HEAD`, newest first and never a
//! parent before all its children (git's `--date-order`), those that
//! changed `path` alone when it is not empty (its tree entry differs from
//! every parent's). A file is followed across its renames, as
//! `git log --follow` follows it (`rename.rs`), each commit from before a
//! rename naming the file as it was named in it (`path`, left out where it
//! is the one asked):
//!
//! ```json
//! {"head": "main", "commits": [{"id": "…", "short": "1a2b3c4", "parents": ["…"], "author": "A", "email": "a@b",
//!   "time": 1700000000, "offset": 3600, "summary": "…", "refs": ["main", "remote: origin/main", "tag: v1"],
//!   "path": "docs/old-name.md"}],
//!  "more": false, "stopped": false}
//! ```
//!
//! `more`: the limit was reached with commits left; `stopped`: the deadline
//! was, and what was found is answered. And
//!
//! ```json
//! {"ask": "patch", "git_dir": "/r/.git", "commit": "…", "path": "", "out": "/w/x.patch", "bytes": 2097152}
//! ```
//!
//! writes to `out` the commit's patch as `git show` prints it (against its
//! first parent, the files under `path` alone; a file `path` names that the
//! parent lacks shown renamed from where it came from, as `git show -M`
//! shows it), and answers
//! `{"files": 3, "bytes": 1234, "cut": false}`. And
//!
//! ```json
//! {"ask": "status", "git_dir": "/r/.git", "work_tree": "/r", "path": "crates", "excludes_file": "/h/.config/git/ignore"}
//! ```
//!
//! answers what the files under `path` are to the repository, as
//! `git status` tells them (`status.rs`): each path not as `HEAD` has it,
//! from `path`, at its strongest, a folder new or ignored whole said as one
//! (`folder` when it is `path` itself):
//!
//! ```json
//! {"folder": null, "states": [["src/a.rs", "changed"], ["notes/", "new"]], "cut": false, "stopped": false}
//! ```
//!
//! A request it refuses or cannot answer is an exit status of 1 and a
//! reason on standard error. Nothing is written to the repository: no lock
//! is taken, the index is read and never refreshed.
use serde::Deserialize;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

mod log;
mod patch;
mod rename;
mod status;
mod tree;

/// A request read at most.
const INPUT: u64 = 64 * 1024;
/// Commits a log lists at most.
const LIMIT: usize = 200_000;
/// A log's limit when the request names none.
const DEFAULT_LIMIT: usize = 500;
/// How long a request may take at most, and when it names none.
const DEADLINE: u64 = 60_000;
const DEFAULT_DEADLINE: u64 = 10_000;
/// A patch written at most.
const PATCH_BYTES: u64 = 64 << 20;
/// Paths a status says at most, and when the request names none.
const STATES: usize = 200_000;
const DEFAULT_STATES: usize = 20_000;
/// Components of a path at most: deeper is no repository's.
const DEPTH: usize = 256;
/// The objects kept decoded while a request reads: commits read as a
/// child and again as a parent, trees shared by neighbouring commits.
const OBJECT_CACHE: usize = 32 << 20;

/// What fails a request: said on standard error, the exit status 1.
pub(crate) type Fail = Box<dyn std::error::Error + Send + Sync>;
pub(crate) type Result<T> = std::result::Result<T, Fail>;

#[derive(Deserialize)]
#[serde(tag = "ask", rename_all = "snake_case")]
enum Ask {
    Log {
        git_dir: PathBuf,
        #[serde(default)]
        path: String,
        #[serde(default = "default_limit")]
        limit: usize,
        deadline_ms: Option<u64>,
    },
    Patch {
        git_dir: PathBuf,
        commit: String,
        #[serde(default)]
        path: String,
        out: PathBuf,
        bytes: u64,
        deadline_ms: Option<u64>,
    },
    Status {
        git_dir: PathBuf,
        work_tree: PathBuf,
        #[serde(default)]
        path: String,
        /// The user's file of patterns ignored everywhere (git's
        /// `core.excludesFile`), which skwir finds: the user's and the
        /// system's configuration is not read here.
        excludes_file: Option<PathBuf>,
        #[serde(default = "default_states")]
        limit: usize,
        deadline_ms: Option<u64>,
    },
}

fn default_limit() -> usize {
    DEFAULT_LIMIT
}

fn default_states() -> usize {
    DEFAULT_STATES
}

/// When a request must be answered by.
pub(crate) struct Deadline(Instant);

impl Deadline {
    fn of(asked: Option<u64>) -> Result<Self> {
        let ms = asked.unwrap_or(DEFAULT_DEADLINE);
        if ms > DEADLINE {
            return Err(format!("a deadline of {ms} ms is past the {DEADLINE} ms allowed").into());
        }
        Ok(Self(Instant::now() + Duration::from_millis(ms)))
    }

    /// Whether it has passed.
    pub(crate) fn passed(&self) -> bool {
        Instant::now() >= self.0
    }
}

/// The repository whose git folder is `git_dir`, isolated from the user's,
/// the system's and the environment's configuration.
fn open(git_dir: &std::path::Path) -> Result<gix::Repository> {
    let mut repo = gix::open_opts(git_dir, gix::open::Options::isolated())?;
    repo.object_cache_size_if_unset(OBJECT_CACHE);
    Ok(repo)
}

/// `open`, the user's patterns ignored everywhere read from
/// `excludes_file`, as git reads `core.excludesFile`: the user's
/// configuration is not, which only skwir's side can find it in.
fn open_ignoring(
    git_dir: &std::path::Path,
    excludes_file: Option<&std::path::Path>,
) -> Result<gix::Repository> {
    let mut options = gix::open::Options::isolated();
    if let Some(file) = excludes_file {
        let file = gix::path::os_str_into_bstr(file.as_os_str())?;
        let mut value = gix::bstr::BString::from("core.excludesFile=");
        value.extend_from_slice(file);
        options = options.config_overrides([value]);
    }
    let mut repo = gix::open_opts(git_dir, options)?;
    repo.object_cache_size_if_unset(OBJECT_CACHE);
    Ok(repo)
}

/// `path`'s components, `/`-separated, a leading or trailing `/` and empty
/// components ignored: none for the whole repository.
pub(crate) fn components(path: &str) -> Result<Vec<&[u8]>> {
    let parts: Vec<&[u8]> = path
        .split('/')
        .filter(|part| !part.is_empty())
        .map(str::as_bytes)
        .collect();
    if parts.len() > DEPTH {
        return Err(format!("a path deeper than {DEPTH} folders").into());
    }
    Ok(parts)
}

fn main() {
    // `--build`, skwir asking which build of it this helper is, when a
    // call of it failed (`platform::process::explain`).
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == "--build")
    {
        println!("{}", env!("SKWIR_BUILD"));
        return;
    }
    // `--probe`, skwir's settings asking whether this helper works here
    // (`platform::process::probe`): being started at all says it does.
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == "--probe")
    {
        return;
    }
    if let Err(reason) = run() {
        // One line: what skwir says of the failure.
        let reason = reason.to_string().replace('\n', " ");
        eprintln!("{reason}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let mut input = Vec::new();
    std::io::stdin()
        .take(INPUT + 1)
        .read_to_end(&mut input)
        .map_err(|error| format!("reading the request: {error}"))?;
    if input.len() as u64 > INPUT {
        return Err(format!("the request is longer than {INPUT} bytes").into());
    }
    let ask: Ask =
        serde_json::from_slice(&input).map_err(|error| format!("reading the request: {error}"))?;
    let answer = match ask {
        Ask::Log {
            git_dir,
            path,
            limit,
            deadline_ms,
        } => {
            if limit > LIMIT {
                return Err(
                    format!("a limit of {limit} commits is past the {LIMIT} allowed").into(),
                );
            }
            let deadline = Deadline::of(deadline_ms)?;
            let repo = open(&git_dir)?;
            serde_json::to_vec(&log::log(&repo, &components(&path)?, limit, &deadline)?)?
        }
        Ask::Patch {
            git_dir,
            commit,
            path,
            out,
            bytes,
            deadline_ms,
        } => {
            if bytes > PATCH_BYTES {
                return Err(
                    format!("a patch of {bytes} bytes is past the {PATCH_BYTES} allowed").into(),
                );
            }
            let deadline = Deadline::of(deadline_ms)?;
            let repo = open(&git_dir)?;
            let id = gix::ObjectId::from_hex(commit.as_bytes())
                .map_err(|_| format!("“{commit}” is not a commit's id"))?;
            let (wrote, patch) = patch::patch(
                &repo,
                id,
                &components(&path)?,
                usize::try_from(bytes).unwrap_or(usize::MAX),
                &deadline,
            )?;
            std::fs::write(&out, &patch)
                .map_err(|error| format!("writing {}: {error}", out.display()))?;
            serde_json::to_vec(&wrote)?
        }
        Ask::Status {
            git_dir,
            work_tree,
            path,
            excludes_file,
            limit,
            deadline_ms,
        } => {
            if limit > STATES {
                return Err(
                    format!("a limit of {limit} paths is past the {STATES} allowed").into(),
                );
            }
            let deadline = Deadline::of(deadline_ms)?;
            let repo = open_ignoring(&git_dir, excludes_file.as_deref())?;
            serde_json::to_vec(&status::status(
                &repo,
                &work_tree,
                &components(&path)?,
                limit,
                &deadline,
            )?)?
        }
    };
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(&answer)?;
    stdout.write_all(b"\n")?;
    stdout.flush()?;
    Ok(())
}
