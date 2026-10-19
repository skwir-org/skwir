//! Completing what the address field is typed (ADR-070): a path a name at
//! a time, as a shell does, and a place a provider's text names.
//!
//! - A path splits at its last separator: `~/wip/sk` is the folder `~/wip`
//!   and the name typed, `sk`. `./` and `../` start from the folder shown,
//!   `~` from the home folder; `..` takes the name before it off, as a
//!   shell's `cd` does.
//! - The folder is read once, on a worker (`folders`), and every key typed
//!   after matches its names in memory (`rank`). A folder a pane lists
//!   already gives them from its listing, current and read by nothing
//!   again; any other is read through `fs::Dir`, names and kinds, a link
//!   stat-ed to know whether it leads to a folder.
//! - A folder part that names nothing is taken a name's start at a time
//!   (`~/w/sk/` for `~/wip/skwir/`): the folders it may stand for, at most
//!   `BRANCHES`, each level ranked as names are.
//! - Names match forgivingly: case ignored unless a capital is typed; the
//!   whole name first, then its start, the start of one of its words,
//!   anywhere in it. Hidden names only when a dot is typed or the pane
//!   shows them. Folders, then files; among each, where the user goes most
//!   (`jump.scores`), then by name.

use std::cmp::Ordering;
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use skwir_core::App;
use skwir_core::cancel::CancelToken;
use skwir_core::command::{Invocation, Outcome};
use skwir_core::entry::EntryKind;
use skwir_core::location::Location;
use skwir_core::os_path::PathArg;

/// The names the dropdown shows at most.
pub const ROWS: usize = 8;

/// The folders a folder part that names none may stand for, at most, at
/// each of its levels.
const BRANCHES: usize = 4;

/// Whether `text` is a path: from the root, from home, or from the folder
/// shown (`./`, `../`); on Windows also from a drive (`C:\`) or a share
/// (`\\server`).
pub fn is_path(text: &str) -> bool {
    let text = text.trim_start();
    let bytes = text.as_bytes();
    text.starts_with('/')
        || text == "~"
        || text.starts_with("~/")
        || matches!(text, "." | "..")
        || text.starts_with("./")
        || text.starts_with("../")
        || (cfg!(windows)
            && (text.starts_with('\\')
                || text.starts_with("~\\")
                || text.starts_with(".\\")
                || text.starts_with("..\\")
                || (bytes.len() >= 3
                    && bytes[0].is_ascii_alphabetic()
                    && bytes[1] == b':'
                    && matches!(bytes[2], b'/' | b'\\'))))
}

/// Whether `c` ends a folder's name in a path typed.
pub fn is_separator(c: char) -> bool {
    c == '/' || (cfg!(windows) && c == '\\')
}

/// The separator a completed folder is followed by: the one the text
/// already uses, else the system's.
pub fn separator_of(text: &str) -> char {
    text.chars()
        .rev()
        .find(|c| is_separator(*c))
        .unwrap_or(std::path::MAIN_SEPARATOR)
}

/// A path typed, split where it is completed: the folder part, up to and
/// with its last separator, kept as typed; the name typed after it.
pub fn split(text: &str) -> (&str, &str) {
    match text.char_indices().rev().find(|(_, c)| is_separator(*c)) {
        Some((at, c)) => text.split_at(at + c.len_utf8()),
        None => ("", text),
    }
}

/// The path `text` names: `~` the home folder, a relative path from
/// `base` (the folder shown), `.` and `..` taken as a shell's `cd` takes
/// them. `None` for a relative path with no folder shown.
pub fn resolve(text: &str, base: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    let tilde = text
        .strip_prefix('~')
        .filter(|rest| rest.is_empty() || rest.starts_with(is_separator));
    let path = if let Some(rest) = tilde {
        home?.join(rest.trim_start_matches(is_separator))
    } else {
        let path = PathBuf::from(text);
        if path.has_root() {
            path
        } else {
            base?.join(path)
        }
    };
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                }
            }
            other => out.push(other),
        }
    }
    Some(out)
}

/// `path` written as the text it completes started: under the home
/// folder as `~/…` when it did.
pub fn written(path: &Path, typed: &str, home: Option<&Path>) -> String {
    if typed.starts_with('~')
        && let Some(home) = home
        && let Ok(rest) = path.strip_prefix(home)
    {
        let sep = separator_of(typed);
        let rest = rest.to_string_lossy();
        return if rest.is_empty() {
            "~".to_owned()
        } else {
            format!("~{sep}{rest}")
        };
    }
    path.to_string_lossy().into_owned()
}

// ---------------------------------------------------------------- names

/// A name a folder (or a place) holds, as matching reads it.
#[derive(Clone, Debug)]
pub struct Name {
    /// The name as the system holds it.
    pub exact: OsString,
    /// The name as text: what is shown and typed.
    pub text: String,
    lower: String,
    /// It opens as a folder (a link to one included), or holds places.
    pub dir: bool,
    pub hidden: bool,
    /// How much the user goes there (`jump.scores`); 0 when unknown.
    pub score: f64,
}

impl Name {
    pub fn new(exact: OsString, dir: bool, hidden: bool) -> Self {
        let text = exact.to_string_lossy().into_owned();
        Name {
            lower: text.to_lowercase(),
            text,
            exact,
            dir,
            hidden,
            score: 0.0,
        }
    }
}

/// A folder's names, read for completion.
#[derive(Debug, Default)]
pub struct Folder {
    pub path: PathBuf,
    pub names: Vec<Name>,
}

/// What the folder part of a path typed stands for, once read.
#[derive(Debug, Default)]
pub struct Resolved {
    /// The folders it may be, best first: the one it names when it names
    /// one; none when it can be none (or could not be read).
    pub folders: Vec<Arc<Folder>>,
    /// It names no folder, and these are what its names start.
    pub abbreviated: bool,
}

/// Whether `typed` asks for case to count: it holds a capital.
fn sensitive(typed: &str) -> bool {
    typed.chars().any(char::is_uppercase)
}

/// What can come before the start of a word in a name.
fn starts_word(before: char) -> bool {
    matches!(
        before,
        ' ' | '-' | '_' | '.' | '(' | '[' | '+' | ',' | '@' | '\''
    )
}

/// How `name` matches `typed` (lowercased in `lower` unless case counts):
/// 0 the whole name, 1 its start, 2 the start of one of its words, 3
/// anywhere in it.
fn tier(name: &Name, typed: &str, lower: &str, sensitive: bool) -> Option<u8> {
    let (hay, needle) = if sensitive {
        (name.text.as_str(), typed)
    } else {
        (name.lower.as_str(), lower)
    };
    // One scan for the names that do not match, most of a large folder's.
    let first = hay.find(needle)?;
    Some(if first == 0 {
        u8::from(hay.len() != needle.len())
    } else if hay
        .match_indices(needle)
        .any(|(at, _)| hay[..at].chars().next_back().is_some_and(starts_word))
    {
        2
    } else {
        3
    })
}

/// A name matched, in the folder of `Resolved::folders` at `folder`.
#[derive(Clone, Copy, Debug)]
pub struct Match {
    pub folder: usize,
    pub index: usize,
    pub tier: u8,
}

/// What matches the name typed in `folders`, best first: at most `ROWS`,
/// and the part the names it starts share, when they are several and it
/// is longer (what Tab fills in), as the first of them ranked writes it.
/// One pass, keeping the best rows as they come: a folder of a hundred
/// thousand names is matched at each key typed.
pub fn rank(
    folders: &[Arc<Folder>],
    typed: &str,
    show_hidden: bool,
) -> (Vec<Match>, Option<String>) {
    let lower = typed.to_lowercase();
    let sensitive = sensitive(typed);
    let hidden_ok = show_hidden || typed.starts_with('.');
    let typed_chars = typed.chars().count();
    let name = |m: &Match| &folders[m.folder].names[m.index];
    let order = |a: &Match, b: &Match| {
        let (na, nb) = (name(a), name(b));
        (a.tier != 0)
            .cmp(&(b.tier != 0))
            .then(nb.dir.cmp(&na.dir))
            .then(a.tier.cmp(&b.tier))
            .then(a.folder.cmp(&b.folder))
            .then(nb.score.partial_cmp(&na.score).unwrap_or(Ordering::Equal))
            .then_with(|| na.lower.cmp(&nb.lower))
            .then_with(|| na.text.cmp(&nb.text))
    };
    let mut best: Vec<Match> = Vec::with_capacity(ROWS + 1);
    // What the names starting it share, in the one folder typed.
    let mut shared = Shared {
        key: None,
        several: false,
        over: folders.len() != 1,
        sensitive,
        typed: typed_chars,
    };
    for (f, folder) in folders.iter().enumerate() {
        for (index, n) in folder.names.iter().enumerate() {
            if n.hidden && !hidden_ok {
                continue;
            }
            let Some(tier) = tier(n, typed, &lower, sensitive) else {
                continue;
            };
            if tier <= 1 {
                shared.add(n);
            }
            let m = Match {
                folder: f,
                index,
                tier,
            };
            if best.len() == ROWS && order(&m, &best[ROWS - 1]) != Ordering::Less {
                continue;
            }
            let at = best.partition_point(|b| order(b, &m) == Ordering::Less);
            best.insert(at, m);
            best.truncate(ROWS);
        }
    }
    let shared = shared.count().and_then(|count| {
        let first = best.first().filter(|m| m.tier <= 1).map(name)?;
        let shared: String = first.text.chars().take(count).collect();
        (count > typed_chars).then_some(shared)
    });
    (best, shared)
}

/// The start the names that start the name typed share, as they come
/// (case ignored unless it counts): asked no further once it is no longer
/// than what was typed, as nothing is then left to fill in.
struct Shared<'a> {
    key: Option<&'a str>,
    several: bool,
    over: bool,
    sensitive: bool,
    typed: usize,
}

impl<'a> Shared<'a> {
    fn add(&mut self, name: &'a Name) {
        if self.over {
            return;
        }
        let other = if self.sensitive {
            name.text.as_str()
        } else {
            name.lower.as_str()
        };
        let Some(key) = self.key else {
            self.key = Some(other);
            return;
        };
        self.several = true;
        let alike = key
            .char_indices()
            .zip(other.chars())
            .take_while(|((_, a), b)| a == b)
            .last()
            .map_or(0, |((at, c), _)| at + c.len_utf8());
        let key = &key[..alike];
        self.over = key.chars().count() <= self.typed;
        self.key = Some(key);
    }

    /// How many characters they share, when they are several and it is
    /// more than was typed.
    fn count(&self) -> Option<usize> {
        (!self.over && self.several)
            .then_some(self.key)
            .flatten()
            .map(|key| key.chars().count())
    }
}

/// What a name completes `typed` with, drawn after it: the rest of the
/// name, when the name starts with what was typed (case ignored unless it
/// counts).
pub fn rest_of(name: &str, typed: &str) -> Option<String> {
    let count = typed.chars().count();
    let start: String = name.chars().take(count).collect();
    let alike = if sensitive(typed) {
        start == typed
    } else {
        start.to_lowercase() == typed.to_lowercase()
    };
    let rest: String = name.chars().skip(count).collect();
    (alike && !rest.is_empty()).then_some(rest)
}

// ---------------------------------------------------------------- reading

/// A folder's names, from a listing a pane keeps of it when one is
/// complete, else read from the disk. Worker-only: it may wait on a disk.
fn names_of(app: &App, folder: &Path, cancel: &CancelToken) -> std::io::Result<Folder> {
    let mut names = Vec::new();
    // Links not stat-ed yet: whether they lead to a folder is asked here.
    let mut links: Vec<usize> = Vec::new();
    let listed = {
        let ws = app.workspace.read();
        let shown = Location::from_path(folder);
        ws.panes().iter().find_map(|pane| {
            let listing = if pane.location().and_then(Location::path).as_deref() == Some(folder) {
                Some(&pane.listing)
            } else {
                pane.kept(&shown)
            };
            listing.filter(|l| l.is_complete()).map(|listing| {
                listing
                    .iter()
                    .filter(|entry| !entry.is_place())
                    .map(|entry| {
                        let unknown = entry.target.is_none()
                            && matches!(entry.kind, EntryKind::Symlink | EntryKind::Unknown);
                        (
                            Name::new(entry.name.clone(), entry.is_dir(), entry.is_hidden()),
                            unknown,
                        )
                    })
                    .collect::<Vec<_>>()
            })
        })
    };
    if let Some(listed) = listed {
        for (name, unknown) in listed {
            if unknown {
                links.push(names.len());
            }
            names.push(name);
        }
    } else {
        let dir = skwir_core::fs::Dir::open(folder)?;
        for (n, listed) in dir.read()?.enumerate() {
            if n % 256 == 0 && cancel.is_cancelled() {
                return Err(std::io::ErrorKind::Interrupted.into());
            }
            let Ok(listed) = listed else { continue };
            if matches!(listed.kind, EntryKind::Symlink | EntryKind::Unknown) {
                links.push(names.len());
            }
            let hidden = listed.hidden || listed.name.as_encoded_bytes().starts_with(b".");
            let dir = listed.kind == EntryKind::Directory;
            names.push(Name::new(listed.name, dir, hidden));
        }
    }
    for (n, at) in links.into_iter().enumerate() {
        if n % 64 == 0 && cancel.is_cancelled() {
            return Err(std::io::ErrorKind::Interrupted.into());
        }
        let name = &mut names[at];
        name.dir = std::fs::metadata(folder.join(&name.exact)).is_ok_and(|m| m.is_dir());
    }
    score(app, folder, &mut names);
    Ok(Folder {
        path: folder.to_path_buf(),
        names,
    })
}

/// Where the user goes among `names` of `folder`, as Jump says
/// (`jump.scores`); nothing when Jump is off.
fn score(app: &App, folder: &Path, names: &mut [Name]) {
    #[derive(serde::Deserialize)]
    struct Score {
        #[serde(with = "skwir_core::os_path")]
        path: PathBuf,
        score: f64,
    }
    if app.commands.get("jump.scores").is_none() {
        return;
    }
    let asked = Invocation::with_args(
        "jump.scores",
        &serde_json::json!({ "folder": PathArg::from(folder) }),
    );
    let Ok(Outcome::Value(value)) = app.dispatch(&asked) else {
        return;
    };
    let Ok(scores) = serde_json::from_value::<Vec<Score>>(value) else {
        return;
    };
    let scores: std::collections::HashMap<OsString, f64> = scores
        .into_iter()
        .filter_map(|s| Some((s.path.file_name()?.to_owned(), s.score)))
        .collect();
    for name in names {
        if let Some(score) = scores.get(&name.exact) {
            name.score = *score;
        }
    }
}

/// What the folder part typed, made a path (`resolve`), stands for: the
/// folder it names, else the folders its names start, from the nearest
/// folder above it (`Resolved`). Worker-only.
pub fn folders(app: &App, typed: &Path, show_hidden: bool, cancel: &CancelToken) -> Resolved {
    match names_of(app, typed, cancel) {
        Ok(folder) => {
            return Resolved {
                folders: vec![Arc::new(folder)],
                abbreviated: false,
            };
        }
        Err(e)
            if !matches!(
                e.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            return Resolved::default();
        }
        Err(_) => {}
    }
    let Some(base) = typed.ancestors().skip(1).find(|a| a.is_dir()) else {
        return Resolved::default();
    };
    let Ok(rest) = typed.strip_prefix(base) else {
        return Resolved::default();
    };
    let mut level = vec![Arc::new(Folder {
        path: base.to_path_buf(),
        names: Vec::new(),
    })];
    for (depth, part) in rest.components().enumerate() {
        let Component::Normal(part) = part else {
            return Resolved::default();
        };
        if cancel.is_cancelled() {
            return Resolved::default();
        }
        let part = part.to_string_lossy();
        // The folders at this level, each read for its names.
        let read: Vec<Arc<Folder>> = level
            .iter()
            .filter_map(|f| names_of(app, &f.path, cancel).ok().map(Arc::new))
            .collect();
        let hidden = show_hidden || part.starts_with('.');
        let lower = part.to_lowercase();
        let sensitive = sensitive(&part);
        let mut next: Vec<(u8, usize, f64, PathBuf)> = Vec::new();
        for (f, folder) in read.iter().enumerate() {
            let matched = folder
                .names
                .iter()
                .filter(|n| n.dir && (hidden || !n.hidden));
            for name in matched {
                if let Some(tier) = tier(name, &part, &lower, sensitive).filter(|t| *t <= 2) {
                    next.push((tier, f, name.score, folder.path.join(&name.exact)));
                }
            }
        }
        // A name typed whole stands for itself alone.
        if next.iter().any(|(tier, ..)| *tier == 0) {
            next.retain(|(tier, ..)| *tier == 0);
        }
        next.sort_by(|a, b| {
            a.0.cmp(&b.0)
                .then(a.1.cmp(&b.1))
                .then(b.2.partial_cmp(&a.2).unwrap_or(Ordering::Equal))
                .then_with(|| a.3.cmp(&b.3))
        });
        next.truncate(BRANCHES);
        if next.is_empty() {
            return Resolved::default();
        }
        tracing::trace!(depth, branches = next.len(), "address: abbreviated");
        level = next
            .into_iter()
            .map(|(.., path)| {
                Arc::new(Folder {
                    path,
                    names: Vec::new(),
                })
            })
            .collect();
    }
    let folders = level
        .iter()
        .filter_map(|f| names_of(app, &f.path, cancel).ok().map(Arc::new))
        .collect();
    Resolved {
        folders,
        abbreviated: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(names: &[(&str, bool)]) -> Arc<Folder> {
        Arc::new(Folder {
            path: PathBuf::from("/f"),
            names: names
                .iter()
                .map(|(n, dir)| Name::new((*n).into(), *dir, n.starts_with('.')))
                .collect(),
        })
    }

    fn ranked(folders: &[Arc<Folder>], typed: &str, hidden: bool) -> Vec<String> {
        rank(folders, typed, hidden)
            .0
            .iter()
            .map(|m| folders[m.folder].names[m.index].text.clone())
            .collect()
    }

    #[test]
    fn paths_are_told_from_words() {
        for text in ["/srv/data", "~", "~/it's here", "./x", "../x", ".", ".."] {
            assert!(is_path(text), "{text}");
        }
        for text in [
            "report 2026",
            "it's a draft",
            "size>10M",
            "kind:image",
            "~x",
            ".x",
        ] {
            assert!(!is_path(text), "{text}");
        }
    }

    #[test]
    fn a_path_splits_at_its_last_separator() {
        assert_eq!(split("~/wip/sk"), ("~/wip/", "sk"));
        assert_eq!(split("~/wip/"), ("~/wip/", ""));
        assert_eq!(split("/"), ("/", ""));
        assert_eq!(split("~/été/ça"), ("~/été/", "ça"));
        assert_eq!(split("words"), ("", "words"));
    }

    #[cfg(unix)]
    #[test]
    fn a_path_resolves_from_home_and_the_folder_shown_as_cd_does() {
        let home = Path::new("/home/u");
        let base = Path::new("/srv/a/b");
        let r = |t| resolve(t, Some(base), Some(home));
        assert_eq!(r("~"), Some(PathBuf::from("/home/u")));
        assert_eq!(r("~/x/"), Some(PathBuf::from("/home/u/x")));
        assert_eq!(r("./"), Some(PathBuf::from("/srv/a/b")));
        assert_eq!(r("../c/"), Some(PathBuf::from("/srv/a/c")));
        assert_eq!(r("../../../../"), Some(PathBuf::from("/")));
        assert_eq!(r("/x/./y/../z"), Some(PathBuf::from("/x/z")));
        assert_eq!(resolve("./x", None, Some(home)), None);
        assert_eq!(
            written(Path::new("/home/u/wip/skwir"), "~/w/", Some(home)),
            "~/wip/skwir"
        );
        assert_eq!(written(Path::new("/srv/x"), "~/w/", Some(home)), "/srv/x");
        assert_eq!(
            written(Path::new("/home/u/x"), "/home/", Some(home)),
            "/home/u/x"
        );
    }

    #[test]
    fn names_match_whole_then_start_then_word_then_anywhere_folders_first() {
        let f = folder(&[
            ("report.pdf", false),
            ("2026 Reports", true),
            ("Rep", true),
            ("prepare", true),
            ("notes", false),
        ]);
        // A capital counts: only names with it.
        assert_eq!(
            ranked(&[Arc::clone(&f)], "Rep", false),
            ["Rep", "2026 Reports"]
        );
        // None typed: case ignored. The whole name, then folders: their
        // start, a word's start, anywhere; then the file.
        assert_eq!(
            ranked(&[f], "rep", false),
            ["Rep", "2026 Reports", "prepare", "report.pdf"]
        );
    }

    #[test]
    fn hidden_names_only_when_a_dot_is_typed_or_shown() {
        let f = folder(&[(".config", true), ("config.toml", false), ("docs", true)]);
        assert_eq!(
            ranked(&[Arc::clone(&f)], "", false),
            ["docs", "config.toml"]
        );
        assert_eq!(ranked(&[Arc::clone(&f)], ".c", false), [".config"]);
        assert_eq!(ranked(&[f], "", true), [".config", "docs", "config.toml"]);
    }

    #[test]
    fn where_the_user_goes_ranks_first_among_alike() {
        let mut f = Folder {
            path: PathBuf::from("/f"),
            names: vec![
                Name::new("Desktop".into(), true, false),
                Name::new("Documents".into(), true, false),
                Name::new("Downloads".into(), true, false),
            ],
        };
        f.names[2].score = 30.0;
        f.names[1].score = 2.0;
        assert_eq!(
            ranked(&[Arc::new(f)], "D", false),
            ["Downloads", "Documents", "Desktop"]
        );
    }

    #[test]
    fn tab_fills_in_what_the_names_started_share() {
        let f = folder(&[
            ("Documents", true),
            ("documentation", true),
            ("old docs", true),
        ]);
        let shared = |typed| rank(&[Arc::clone(&f)], typed, false).1;
        // Case ignored, written as the first name ranked writes it.
        assert_eq!(shared("doc").as_deref(), Some("document"));
        assert_eq!(shared("document"), None, "nothing more is shared");
        assert_eq!(shared("Doc"), None, "one name: Tab takes it whole");
        assert_eq!(shared("x"), None);
        // Folders being guessed share nothing.
        assert_eq!(
            rank(&[Arc::clone(&f), Arc::clone(&f)], "doc", false).1,
            None
        );
    }

    #[test]
    fn many_names_keep_the_best_rows() {
        let names: Vec<(String, bool)> = (0..1000)
            .map(|i| (format!("n{i:04}"), i % 2 == 0))
            .collect();
        let names: Vec<(&str, bool)> = names.iter().map(|(n, d)| (n.as_str(), *d)).collect();
        let f = folder(&names);
        let rows = ranked(&[f], "n", false);
        assert_eq!(rows.len(), ROWS);
        assert_eq!(rows[0], "n0000");
        assert_eq!(rows[1], "n0002", "folders first");
    }

    #[test]
    fn the_rest_of_a_name_follows_what_was_typed() {
        assert_eq!(rest_of("Downloads", "dow").as_deref(), Some("nloads"));
        assert_eq!(rest_of("Downloads", "Dow").as_deref(), Some("nloads"));
        assert_eq!(rest_of("Downloads", "DOW"), None, "a capital counts");
        assert_eq!(rest_of("2026 Reports", "rep"), None, "not its start");
        assert_eq!(rest_of("été", "é").as_deref(), Some("té"));
        assert_eq!(rest_of("x", "x"), None);
    }
}
