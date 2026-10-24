//! Use the system's `plocate` database for name-only queries over `/`, when
//! it is installed: it answers in milliseconds what a walk takes minutes for.
//!
//! The database knows nothing of our options, so its answer is a superset
//! that the caller still matches: we only drop what `hidden:` and `depth:`
//! exclude, which a walk would never have visited.
//!
//! Its answer is as of the database's last update (`updatedb`, usually
//! daily), which the search says (ADR-050): a file made, renamed or
//! removed since is not in it.

use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use skwir_core::cancel::CancelToken;

use crate::lang::Query;

/// How long the database may take to answer before the walk is used.
const TIMEOUT: Duration = Duration::from_secs(30);
/// The most of an answer read: about a million paths. A broader one is
/// left to the walk, which streams.
const ANSWER: u64 = 64 << 20;
/// How long its help may take to name its database, and how much of it is
/// read.
const HELP_TIMEOUT: Duration = Duration::from_secs(5);
const HELP: u64 = 64 << 10;

/// What answered, in the user's words (`AsOf::by`).
pub const BY: &str = "plocate's database";

/// What `plocate` answered: the paths it names, and when its databases
/// were last brought up to date, as far as that can be told.
#[derive(Debug, PartialEq, Eq)]
pub struct Located {
    pub paths: Vec<PathBuf>,
    /// The oldest of its databases' updates: what the answer is as of.
    pub updated: Option<SystemTime>,
}

/// Candidate paths from `plocate`, or `None` when it does not apply: not
/// installed, root is not `/`, the query is not an AND of name substrings,
/// or the database could not answer, in time and in a bounded answer (the
/// walk is used instead). Stops with the search.
pub fn try_locate(q: &Query, cancel: &CancelToken) -> Option<Located> {
    if q.root != Path::new("/") {
        return None;
    }
    let patterns = q.name_substrings()?;
    let bin = skwir_core::os_path::find_program("plocate")?;
    let paths = locate_with(&bin, &patterns, q.case_sensitive, cancel)?;
    // What a walk of `/` would not enter, its database may have indexed.
    let unreached = crate::provider::mounts_below(&q.root);
    Some(Located {
        paths: paths
            .into_iter()
            .filter(|p| {
                visible(p, q.hidden, q.depth) && !unreached.iter().any(|point| p.starts_with(point))
            })
            .collect(),
        updated: updated(&databases(&bin, cancel)),
    })
}

/// The databases `bin` reads when given none, as plocate (and mlocate
/// before it) does: its own default, which its help names, then those
/// `LOCATE_PATH` adds. Without the default (a help that does not name it)
/// none: the answer is then as of a time that cannot be told.
pub fn databases(bin: &Path, cancel: &CancelToken) -> Vec<PathBuf> {
    let mut cmd = Command::new(bin);
    cmd.arg("--help");
    let Some(default) =
        skwir_platform::process::run_capped(&mut cmd, None, cancel, HELP_TIMEOUT, HELP)
            .ok()
            .and_then(|out| {
                default_database(&out.stdout).or_else(|| default_database(out.stderr.as_bytes()))
            })
    else {
        return Vec::new();
    };
    let added = std::env::var_os("LOCATE_PATH").unwrap_or_default();
    std::iter::once(default)
        .chain(std::env::split_paths(&added).filter(|p| !p.as_os_str().is_empty()))
        .collect()
}

/// The default database plocate's help names: `(default is
/// /var/lib/plocate/plocate.db)`.
pub(crate) fn default_database(help: &[u8]) -> Option<PathBuf> {
    const SAYS: &[u8] = b"(default is ";
    let at = help.windows(SAYS.len()).position(|w| w == SAYS)? + SAYS.len();
    let rest = &help[at..];
    let path = &rest[..rest.iter().position(|b| *b == b')')?];
    (path.first() == Some(&b'/'))
        .then(|| PathBuf::from(skwir_core::os_path::from_bytes(path.to_vec())))
}

/// When the oldest of `databases` was last written: what an answer from
/// all of them is as of. `None` when none can be stat-ed.
pub fn updated(databases: &[PathBuf]) -> Option<SystemTime> {
    databases
        .iter()
        .filter_map(|db| std::fs::metadata(db).and_then(|m| m.modified()).ok())
        .min()
}

/// Whether a walk of `/` with these options would reach `path`: absolute,
/// or relative to where the walk starts.
pub(crate) fn visible(path: &Path, hidden: bool, depth: Option<usize>) -> bool {
    let mut names = 0;
    for c in path.components() {
        match c {
            Component::Normal(name) => {
                names += 1;
                if !hidden && name.as_encoded_bytes().starts_with(b".") {
                    return false;
                }
            }
            Component::RootDir => {}
            // plocate prints absolute, normalized paths; anything else is not ours.
            _ => return false,
        }
    }
    names > 0 && depth.is_none_or(|d| names <= d)
}

/// Run `bin` with plocate's flags and parse NUL-separated output. `None`
/// when plocate failed rather than found nothing, or was stopped.
pub fn locate_with(
    bin: &Path,
    patterns: &[String],
    case_sensitive: bool,
    cancel: &CancelToken,
) -> Option<Vec<PathBuf>> {
    let mut cmd = Command::new(bin);
    cmd.arg("--basename").arg("--null");
    if !case_sensitive {
        cmd.arg("--ignore-case");
    }
    cmd.arg("--").args(patterns);
    let out = skwir_platform::process::run_capped(&mut cmd, None, cancel, TIMEOUT, ANSWER).ok()?;
    if !out.status.success() {
        // plocate exits 1, silently, when nothing matched. A message (a
        // missing or unreadable database) means it could not answer.
        return (out.status.code() == Some(1) && out.stdout.is_empty() && out.stderr.is_empty())
            .then(Vec::new);
    }
    Some(
        out.stdout
            .split(|b| *b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| PathBuf::from(skwir_core::os_path::from_bytes(s.to_vec())))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_and_depth_are_what_a_walk_would_skip() {
        assert!(visible(Path::new("/a/b.txt"), false, None));
        assert!(!visible(Path::new("/a/.git/b.txt"), false, None));
        assert!(!visible(Path::new("/.b"), false, None));
        assert!(visible(Path::new("/a/.git/b.txt"), true, None));
        assert!(visible(Path::new("/a/b"), false, Some(2)));
        assert!(!visible(Path::new("/a/b/c"), false, Some(2)));
        assert!(!visible(Path::new("/"), true, None));
    }

    #[test]
    fn the_default_database_is_the_one_the_help_names() {
        let help = b"  -d, --database DBPATH  search for files in DBPATH\n\
                     (default is /var/lib/plocate/plocate.db)\n";
        assert_eq!(
            default_database(help),
            Some(PathBuf::from("/var/lib/plocate/plocate.db"))
        );
        assert_eq!(
            default_database(b"(default is /var/cache/a db)"),
            Some(PathBuf::from("/var/cache/a db")),
            "a space in its path"
        );
        assert_eq!(
            default_database(b"Usage: plocate [OPTION]... PATTERN..."),
            None
        );
        assert_eq!(default_database(b"(default is relative.db)"), None);
        assert_eq!(default_database(b"(default is /cut"), None);
    }

    #[test]
    fn an_answer_is_as_of_its_oldest_database() {
        let tmp = tempfile::tempdir().unwrap();
        let (old, new) = (tmp.path().join("old.db"), tmp.path().join("new.db"));
        let at = |secs| SystemTime::UNIX_EPOCH + Duration::from_secs(secs);
        for (db, secs) in [(&old, 1_800_000_000), (&new, 1_800_086_400)] {
            let file = std::fs::File::create(db).unwrap();
            file.set_modified(at(secs)).unwrap();
        }
        let missing = tmp.path().join("missing.db");
        assert_eq!(
            updated(&[new.clone(), missing.clone(), old]),
            Some(at(1_800_000_000))
        );
        assert_eq!(updated(&[new]), Some(at(1_800_086_400)));
        assert_eq!(updated(&[missing]), None);
        assert_eq!(updated(&[]), None);
    }
}
