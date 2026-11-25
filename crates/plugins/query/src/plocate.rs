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

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use skwir_core::cancel::CancelToken;
use skwir_core::coverage::Coverage;
use skwir_core::names::{Answer, NameIndex, Opened, Session};

/// How long the database may take to answer before the walk is used: all
/// of a search's runs ([`runs`]) together.
const TIMEOUT: Duration = Duration::from_secs(30);
/// The most of an answer read, all of a search's runs together: about a
/// million paths. A broader one is left to the walk, which streams.
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
/// substrings, as it did before the seam, each asked in its forms
/// ([`runs`]). It cannot be synced: its answer is taken as of when it is,
/// and dated.
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
        asked(q)?;
        let bin = self.program()?;
        let at = updated(&self.databases_of(&bin, cancel))?;
        Some(Coverage::as_of(Some(at)))
    }

    /// As [`covers`](Self::covers): it is asked each term in its forms
    /// ([`runs`]).
    fn covers_both_forms(
        &self,
        q: &skwir_core::names::Query,
        cancel: &CancelToken,
    ) -> Option<Coverage> {
        self.covers(q, cancel)
    }
}

/// A search of `/` asked of plocate.
struct Located {
    bin: PathBuf,
    cancel: CancelToken,
}

impl Session for Located {
    /// Asked each term in its forms ([`runs`]).
    fn both_forms(&self) -> bool {
        true
    }

    fn sync(&mut self) -> Option<Coverage> {
        None
    }

    /// Its runs ([`runs`]) asked in turn ([`located`]).
    fn query(
        &mut self,
        q: &skwir_core::names::Query,
        each: &mut dyn FnMut(Vec<PathBuf>) -> bool,
    ) -> Option<Answer> {
        let (runs, case_sensitive) = asked(q)?;
        let budget = (TIMEOUT, ANSWER);
        located(&self.bin, &runs, case_sensitive, &self.cancel, budget, each)?;
        Some(Answer {
            coverage: Coverage::as_of(updated(&databases(&self.bin, &self.cancel))),
            truncated: false,
        })
    }
}

/// `runs` asked of `bin` in turn, each run's answer handed to `each` as it
/// ends, each path once: where there are several runs, what an earlier one
/// named is left out of a later one's (a name holding a term in two
/// forms), from a set of what was handed, added to while a run follows; a
/// single run, every ASCII search, keeps none. One time limit and one
/// answer's size (`time`, bytes) for all of them: a search of several
/// forms waits and holds no more than one of one. `None` where a run could
/// not answer within what was left, or `each` said to stop.
fn located(
    bin: &Path,
    runs: &[Vec<String>],
    case_sensitive: bool,
    cancel: &CancelToken,
    (time, answer): (Duration, u64),
    each: &mut dyn FnMut(Vec<PathBuf>) -> bool,
) -> Option<()> {
    let deadline = Instant::now() + time;
    let mut left = answer;
    let mut named: HashSet<PathBuf> = HashSet::new();
    for (at, run) in runs.iter().enumerate() {
        let time = deadline.saturating_duration_since(Instant::now());
        if time.is_zero() {
            return None;
        }
        let (mut paths, read) = locate_within(bin, run, case_sensitive, cancel, time, left)?;
        left = left.saturating_sub(read);
        if at + 1 < runs.len() {
            paths.retain(|path| named.insert(path.clone()));
        } else if at > 0 {
            paths.retain(|path| !named.contains(path));
        }
        if !each(paths) {
            return None;
        }
    }
    Some(())
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

/// What plocate is asked of `q`, and whether its case counts: [`runs`] of
/// its [`substrings`]; `None` where it is asked nothing.
fn asked(q: &skwir_core::names::Query) -> Option<(Vec<Vec<String>>, bool)> {
    let patterns = substrings(q)?;
    let case_sensitive = q.names.iter().any(|n| n.case_sensitive);
    Some((runs(&patterns, case_sensitive)?, case_sensitive))
}

/// The most runs a search asks of plocate: every form of three terms of
/// two forms each.
const RUNS: usize = 8;

/// What plocate is asked, a run each, for `patterns` all found in a name:
/// each in its forms, as typed, composed and decomposed (NFC and NFD,
/// `name_match::Forms`), its case as typed, which `--ignore-case` folds
/// unless `case_sensitive` (a sigma then asked as a wildcard,
/// [`ignoring_case`]); every combination of them, as plocate matches bytes
/// and finds a name holding all a run's patterns (`name_match`: a term
/// matches in any of its forms). At most [`RUNS`]: a term whose forms would
/// make more is left out, which the search checks of every name, and
/// plocate is asked for more, never fewer. So is a term of sigmas alone
/// where case is ignored, which narrows nothing. `None` where every term
/// is left out.
fn runs(patterns: &[String], case_sensitive: bool) -> Option<Vec<Vec<String>>> {
    let mut runs: Vec<Vec<String>> = vec![Vec::new()];
    for pattern in patterns {
        let forms = skwir_core::name_match::Forms::of(pattern);
        if runs.len() * forms.count() > RUNS {
            continue;
        }
        let asked: Option<Vec<String>> = forms
            .iter()
            .map(|form| {
                if case_sensitive {
                    Some(form.to_owned())
                } else {
                    ignoring_case(form)
                }
            })
            .collect();
        let Some(asked) = asked else {
            continue;
        };
        let mut next = Vec::with_capacity(runs.len() * asked.len());
        for run in &runs {
            for form in &asked {
                let mut run = run.clone();
                run.push(form.clone());
                next.push(run);
            }
        }
        runs = next;
    }
    runs.first()
        .is_some_and(|run| !run.is_empty())
        .then_some(runs)
}

/// `form`, a term's, as plocate is asked it where case is ignored: as it
/// is, but where it holds a sigma (what [`name_match::fold`] takes as `σ`:
/// `σ`, `ς`, `Σ`), a glob of it with `?` for each, `*` around it, as
/// plocate matches a pattern holding a wildcard against the whole name.
/// plocate folds as `towlower` does, its trigrams and its
/// `fnmatch(FNM_CASEFOLD)` alike, which takes `Σ` to `σ` alone, never `ς`,
/// where `fold` takes `ς` as `σ` on both sides: asked as typed, `οδος`
/// would leave out `ΟΔΟΣ.txt` and `οδοσ`. Its literal runs still give
/// plocate its trigrams. `?` is one character, as plocate reads text in
/// UTF-8 ([`plocate`]). `None` where it holds nothing but sigmas: `*?*`
/// narrows nothing. ([`substrings`] keeps out a term holding what a glob
/// reads, so nothing else needs escaping.)
///
/// [`name_match::fold`]: skwir_core::name_match::fold
fn ignoring_case(form: &str) -> Option<String> {
    let sigma = |c: char| skwir_core::name_match::fold(c.encode_utf8(&mut [0; 4])).contains('σ');
    if form.is_ascii() || !skwir_core::name_match::fold(form).contains('σ') {
        return Some(form.to_owned());
    }
    let glob: String = std::iter::once('*')
        .chain(form.chars().map(|c| if sigma(c) { '?' } else { c }))
        .chain(std::iter::once('*'))
        .collect();
    form.chars().any(|c| !sigma(c)).then_some(glob)
}

/// The databases `bin` reads when given none, as plocate (and mlocate
/// before it) does: its own default, which its help names, then those
/// `LOCATE_PATH` adds. Without the default (a help that does not name it)
/// none: the answer is then as of a time that cannot be told.
pub fn databases(bin: &Path, cancel: &CancelToken) -> Vec<PathBuf> {
    let mut cmd = plocate(bin);
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

/// `bin` to be run, reading text as a search reads it, in UTF-8 and its
/// case folded the same in every language, whatever the session's locale
/// (`C.UTF-8`): plocate reads its patterns and names, and folds their case
/// (`towlower`, `--ignore-case`), in the locale's encoding and by its rules.
/// In a Turkish one `I` folds to `ı`, so `file` would not find `FILE.txt`;
/// in one of a byte a character (`el_GR.ISO-8859-7`) Greek folds not at
/// all, `οδο` not finding `ΟΔΟΣ`, and `?` takes a byte. Where the system
/// has no `C.UTF-8`, plocate refuses a pattern beyond ASCII with case
/// ignored, which is an answer it could not give: the search walks.
fn plocate(bin: &Path) -> std::process::Command {
    let mut cmd = skwir_platform::process::command(bin);
    cmd.env("LC_ALL", "C.UTF-8");
    cmd
}

/// Run `bin` with plocate's flags and parse NUL-separated output. `None`
/// when plocate failed rather than found nothing, or was stopped.
pub fn locate_with(
    bin: &Path,
    patterns: &[String],
    case_sensitive: bool,
    cancel: &CancelToken,
) -> Option<Vec<PathBuf>> {
    locate_within(bin, patterns, case_sensitive, cancel, TIMEOUT, ANSWER).map(|(paths, _)| paths)
}

/// [`locate_with`] within `time` and `answer` bytes, and how many it read.
fn locate_within(
    bin: &Path,
    patterns: &[String],
    case_sensitive: bool,
    cancel: &CancelToken,
    time: Duration,
    answer: u64,
) -> Option<(Vec<PathBuf>, u64)> {
    let mut cmd = plocate(bin);
    cmd.arg("--basename").arg("--null");
    if !case_sensitive {
        cmd.arg("--ignore-case");
    }
    cmd.arg("--").args(patterns);
    let out = skwir_platform::process::run_capped(&mut cmd, None, cancel, time, answer).ok()?;
    if !out.status.success() {
        // plocate exits 1, silently, when nothing matched. A message (a
        // missing or unreadable database) means it could not answer.
        return (out.status.code() == Some(1) && out.stdout.is_empty() && out.stderr.is_empty())
            .then(|| (Vec::new(), 0));
    }
    let read = out.stdout.len() as u64;
    let paths = out
        .stdout
        .split(|b| *b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| PathBuf::from(skwir_core::os_path::from_bytes(s.to_vec())))
        .collect();
    Some((paths, read))
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

    /// Each term is asked in its forms (as typed, composed, decomposed),
    /// every combination of them, an ASCII one once; past [`RUNS`], a term
    /// of more forms is left out, never one of one. A sigma, where case is
    /// ignored, is a wildcard; a term of sigmas alone is left out, and with
    /// none left, nothing is asked.
    #[test]
    fn plocate_is_asked_each_term_in_its_forms() {
        let strings = |run: &[&str]| run.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        let runs = |patterns: &[&str]| runs(&strings(patterns), false);
        assert_eq!(
            runs(&["report", "q3"]),
            Some(vec![strings(&["report", "q3"])])
        );
        assert_eq!(
            runs(&["CAF\u{c9}", "report"]),
            Some(vec![
                strings(&["CAF\u{c9}", "report"]),
                strings(&["CAFE\u{301}", "report"])
            ]),
            "its case as typed"
        );
        assert_eq!(
            runs(&["cafe\u{301}", "th\u{e9}"]),
            Some(vec![
                strings(&["cafe\u{301}", "th\u{e9}"]),
                strings(&["cafe\u{301}", "the\u{301}"]),
                strings(&["caf\u{e9}", "th\u{e9}"]),
                strings(&["caf\u{e9}", "the\u{301}"]),
            ]),
            "as typed first"
        );
        assert_eq!(
            runs(&["\u{434}\u{43e}\u{43c}"]),
            Some(vec![strings(&["\u{434}\u{43e}\u{43c}"])]),
            "one form"
        );
        // As typed too, where neither form keeps it.
        assert_eq!(
            runs(&["\u{f900}"]),
            Some(vec![strings(&["\u{f900}"]), strings(&["\u{8c48}"])])
        );
        assert_eq!(
            runs(&["\u{1f71}"]),
            Some(vec![
                strings(&["\u{1f71}"]),
                strings(&["\u{3ac}"]),
                strings(&["\u{3b1}\u{301}"])
            ])
        );
        let many = ["\u{e9}1", "\u{e9}2", "\u{e9}3", "\u{e9}4", "x"];
        let asked = runs(&many).unwrap();
        assert_eq!(asked.len(), RUNS);
        for run in &asked {
            assert_eq!(run.len(), 4, "the fourth left out: {run:?}");
            assert_eq!(run.last().map(String::as_str), Some("x"));
        }
        // Three forms, then three more: the second left out.
        let asked = runs(&["\u{1f71}", "\u{1f71}x", "y"]).unwrap();
        assert_eq!(asked.len(), 3);
        assert!(asked.iter().all(|run| run.len() == 2 && run[1] == "y"));
        // A sigma, case ignored: plocate folds `Σ` to `σ` alone, so each
        // is a wildcard, the term a glob of the whole name; its other
        // forms alike.
        for (sigma, glob) in [
            ("\u{39f}\u{394}\u{39f}\u{3a3}", "*\u{39f}\u{394}\u{39f}?*"),
            ("\u{3bf}\u{3b4}\u{3bf}\u{3c2}", "*\u{3bf}\u{3b4}\u{3bf}?*"),
            ("\u{3c3}\u{3b1}", "*?\u{3b1}*"),
            ("\u{3c3}\u{3c3}\u{3c2}x\u{3a3}", "*???x?*"),
        ] {
            assert_eq!(
                runs(&[sigma, "x"]),
                Some(vec![strings(&[glob, "x"])]),
                "{sigma:?}"
            );
            assert_eq!(runs(&[sigma]), Some(vec![strings(&[glob])]), "{sigma:?}");
            assert_eq!(
                super::runs(&strings(&[sigma]), true),
                Some(vec![strings(&[sigma])]),
                "{sigma:?}: as typed where case counts"
            );
        }
        assert_eq!(
            runs(&["\u{3bf}\u{3b4}\u{3cc}\u{3c2}"]),
            Some(vec![
                strings(&["*\u{3bf}\u{3b4}\u{3cc}?*"]),
                strings(&["*\u{3bf}\u{3b4}\u{3bf}\u{301}?*"])
            ]),
            "each form a glob"
        );
        // Sigmas alone narrow nothing: left out.
        assert_eq!(runs(&["\u{3c3}", "x"]), Some(vec![strings(&["x"])]));
        assert_eq!(runs(&["\u{3a3}\u{3c2}"]), None);
        assert_eq!(
            runs(&["\u{3bf}\u{3b4}\u{3bf}"]),
            Some(vec![strings(&["\u{3bf}\u{3b4}\u{3bf}"])])
        );
    }

    /// A search of `/` whose every term holds a sigma is asked of plocate
    /// (each sigma a wildcard), not walked; one of sigmas alone is not.
    #[test]
    fn a_search_of_sigmas_is_asked_as_a_glob() {
        let named = |s: &str| crate::lang::parse(s).unwrap().named("/".into()).unwrap();
        let ask = |s: &str| asked(&named(s)).map(|(runs, _)| runs);
        assert_eq!(
            ask("\u{3bf}\u{3b4}\u{3bf}\u{3c2} in:/"),
            Some(vec![vec!["*\u{3bf}\u{3b4}\u{3bf}?*".to_owned()]])
        );
        assert_eq!(
            ask("\u{3bf}\u{3b4}\u{3bf}\u{3c2} case:sensitive in:/"),
            Some(vec![vec!["\u{3bf}\u{3b4}\u{3bf}\u{3c2}".to_owned()]])
        );
        assert!(ask("\u{3c3} in:/").is_none());
        assert!(ask("\u{3c3} report in:/").is_some());
    }

    /// plocate is asked in UTF-8 and by one case folding, whatever the
    /// session's locale: a Turkish one folds `I` to `ı`, one of a byte a
    /// character folds no Greek, and its `?` would take a byte.
    #[cfg(unix)]
    #[test]
    fn plocate_reads_text_in_utf8_whatever_the_locale() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let log = tmp.path().join("asked");
        let bin = tmp.path().join("plocate");
        std::fs::write(
            &bin,
            format!(
                "#!/bin/sh\nprintf '%s %s\\n' \"$LC_ALL\" \"$*\" >> '{}'\nprintf '/x\\0'\n",
                log.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let index = Index::at(bin);
        let never = CancelToken::new();
        let mut opened = index.open(Path::new("/"), &never).expect("over /");
        let q = crate::lang::parse("\u{39f}\u{394}\u{39f}\u{3a3} in:/")
            .unwrap()
            .named("/".into())
            .unwrap();
        let mut found = Vec::new();
        opened
            .session
            .query(&q, &mut |paths| {
                found.extend(paths);
                true
            })
            .expect("asked");
        assert_eq!(found, [PathBuf::from("/x")]);
        assert_eq!(
            std::fs::read_to_string(&log).unwrap(),
            "C.UTF-8 --basename --null --ignore-case -- *\u{39f}\u{394}\u{39f}?*\n\
             C.UTF-8 --help\n"
        );
    }

    /// A search's runs share one time limit and one answer's size, each
    /// run's answer handed as it ends, a path an earlier run named left
    /// out of a later one's.
    #[cfg(unix)]
    #[test]
    fn runs_share_one_time_limit_and_one_answer() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let log = tmp.path().join("asked");
        // Each run said on a line; 13 bytes: what both name, and its own.
        let script = |pause: &str| {
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nsleep {pause}\nfor a; do last=$a; done\nprintf '/same\\0/run-%s\\0' \"$last\"\n",
                log.display()
            )
        };
        let bin = tmp.path().join("plocate");
        let write = |pause: &str| {
            std::fs::write(&bin, script(pause)).unwrap();
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
            std::fs::write(&log, "").unwrap();
        };
        let runs = vec![vec!["a".to_owned()], vec!["b".to_owned()]];
        let never = CancelToken::new();
        let ample = Duration::from_secs(60);
        let ask = |budget: (Duration, u64)| {
            let mut handed = Vec::new();
            let answered = located(&bin, &runs, false, &never, budget, &mut |paths| {
                handed.push(paths);
                true
            });
            let asked = std::fs::read_to_string(&log).unwrap().lines().count();
            (answered, handed, asked)
        };
        write("0");
        let (answered, handed, asked) = ask((ample, 26));
        assert_eq!(answered, Some(()));
        assert_eq!(asked, 2);
        assert_eq!(
            handed,
            [
                vec![PathBuf::from("/same"), PathBuf::from("/run-a")],
                vec![PathBuf::from("/run-b")]
            ],
            "each as its run ends, each once"
        );
        // The second run is given what the first left: too little.
        write("0");
        let (answered, handed, asked) = ask((ample, 20));
        assert_eq!(answered, None);
        assert_eq!(asked, 2);
        assert_eq!(handed.len(), 1, "the first run's answer handed");
        // Likewise the time: a second run of a second each, within 1.5 s
        // all together, is stopped.
        write("1");
        let started = Instant::now();
        let (answered, _, _) = ask((Duration::from_millis(1500), 1 << 20));
        assert_eq!(answered, None);
        assert!(started.elapsed() < Duration::from_secs(30));
        write("0");
        assert_eq!(ask((Duration::ZERO, 1 << 20)).0, None);
        assert_eq!(
            std::fs::read_to_string(&log).unwrap(),
            "",
            "nothing started"
        );
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
