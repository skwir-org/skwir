//! Use the system's `plocate` database for name-only queries over `/`, when
//! it is installed: it answers in milliseconds what a walk takes minutes for.
//!
//! The database knows nothing of our options, so its answer is a superset
//! that the search still matches, as it matches any names index's
//! (ADR-150): what `hidden:` and `depth:` exclude, which a walk would never
//! have visited, it drops.
//!
//! Its answer is as of the database's last update (`updatedb`, usually
//! daily), which the search says (ADR-050): a file made, renamed or
//! removed since is not in it.

use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime};

use skwir_core::cancel::CancelToken;
use skwir_core::coverage::Coverage;
use skwir_core::names::{Answer, NameIndex, Opened, Session};

/// How long the database may take to answer before the walk is used.
const TIMEOUT: Duration = Duration::from_secs(30);
/// The most of an answer read: about a million paths. A broader one is
/// left to the walk, which streams.
const ANSWER: u64 = 64 << 20;
/// How long its help may take to name its database, and how much of it is
/// read.
const HELP_TIMEOUT: Duration = Duration::from_secs(5);
const HELP: u64 = 64 << 10;

/// What answered (`RestsOn::by`).
pub fn by() -> skwir_core::words::Words {
    skwir_core::words!(
        query_plocate_database = "plocate's database",
        note = "What a search's answer rests on, said in its footer: the index plocate keeps of every file's name."
    )
}

/// plocate's database as a names index (ADR-150, 4): it covers `/` as of
/// its databases' update, and answers a search all of whose terms are name
/// substrings, as it did before the seam. It cannot be synced: its answer
/// is taken as of when it is, and dated.
#[derive(Default)]
pub struct Index {
    /// The program asked; `None` for the `plocate` on the `PATH`.
    program: Option<PathBuf>,
    /// The databases a program reads, as its help named them, asked once
    /// a program ([`NameIndex::covers`] is asked at each key typed).
    databases: parking_lot::Mutex<Option<(PathBuf, Vec<PathBuf>)>>,
}

impl Index {
    /// The index of the database `program` reads (a test's stand-in).
    #[cfg(test)]
    pub fn at(program: PathBuf) -> Index {
        Index {
            program: Some(program),
            databases: parking_lot::Mutex::default(),
        }
    }

    fn program(&self) -> Option<PathBuf> {
        match &self.program {
            Some(program) => Some(program.clone()),
            None => skwir_core::os_path::find_program("plocate"),
        }
    }

    /// The databases `bin` reads, its help asked once.
    fn databases_of(&self, bin: &Path, cancel: &CancelToken) -> Vec<PathBuf> {
        if let Some((known, databases)) = self.databases.lock().as_ref()
            && known == bin
        {
            return databases.clone();
        }
        let databases = databases(bin, cancel);
        if !cancel.is_cancelled() {
            *self.databases.lock() = Some((bin.to_path_buf(), databases.clone()));
        }
        databases
    }
}

impl NameIndex for Index {
    fn id(&self) -> &'static str {
        "plocate"
    }

    fn by(&self) -> skwir_core::words::Words {
        by()
    }

    fn syncs(&self) -> bool {
        false
    }

    fn open(&self, root: &Path, cancel: &CancelToken) -> Option<Opened> {
        if root != Path::new("/") {
            return None;
        }
        let bin = self.program()?;
        Some(Opened {
            // As of a time read once it answered (`Session::query`): no
            // process is started for a search it cannot answer.
            coverage: Coverage::as_of(None),
            session: Box::new(Located {
                bin,
                cancel: cancel.clone(),
            }),
        })
    }

    /// What runs is its databases: opening starts nothing.
    fn follow(&self, root: &Path, cancel: &CancelToken) -> Option<Opened> {
        self.open(root, cancel)
    }

    /// As of its databases' update where it would answer `q`: a search of
    /// `/` whose terms are all name substrings, a database there to read
    /// (a stat; its help, which names it, asked once).
    fn covers(&self, q: &skwir_core::names::Query, cancel: &CancelToken) -> Option<Coverage> {
        if q.root != Path::new("/") {
            return None;
        }
        substrings(q)?;
        let bin = self.program()?;
        let at = updated(&self.databases_of(&bin, cancel))?;
        Some(Coverage::as_of(Some(at)))
    }
}

/// A search of `/` asked of plocate.
struct Located {
    bin: PathBuf,
    cancel: CancelToken,
}

impl Session for Located {
    fn sync(&mut self) -> Option<Coverage> {
        None
    }

    fn query(
        &mut self,
        q: &skwir_core::names::Query,
        each: &mut dyn FnMut(Vec<PathBuf>) -> bool,
    ) -> Option<Answer> {
        let patterns = substrings(q)?;
        let case_sensitive = q.names.iter().any(|n| n.case_sensitive);
        let paths = locate_with(&self.bin, &patterns, case_sensitive, &self.cancel)?;
        each(paths);
        Some(Answer {
            coverage: Coverage::as_of(updated(&databases(&self.bin, &self.cancel))),
            truncated: false,
        })
    }
}

/// The names a search asks for, every one of them as a substring, when
/// they are all it asks: what a locate database answers. One holding what
/// plocate reads as a glob (`*`, `?`, `[`, `\`) is not one, even where it
/// does not compile as a glob here and is searched for as written (`a[b`):
/// plocate would match it whole, as a glob.
fn substrings(q: &skwir_core::names::Query) -> Option<Vec<String>> {
    if !q.whole
        || q.kinds.is_some()
        || q.size.is_some()
        || q.modified.is_some()
        || q.names.is_empty()
    {
        return None;
    }
    q.names
        .iter()
        .map(|n| {
            (!n.text.is_empty() && !n.text.contains(['*', '?', '[', '\\'])).then(|| n.text.clone())
        })
        .collect()
}

/// The databases `bin` reads when given none, as plocate (and mlocate
/// before it) does: its own default, which its help names, then those
/// `LOCATE_PATH` adds. Without the default (a help that does not name it)
/// none: the answer is then as of a time that cannot be told.
pub fn databases(bin: &Path, cancel: &CancelToken) -> Vec<PathBuf> {
    let mut cmd = skwir_platform::process::command(bin);
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
    let mut cmd = skwir_platform::process::command(bin);
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
