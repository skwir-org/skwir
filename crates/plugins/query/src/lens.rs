//! The lens (ADR-107) and the words typed over a folder: the core's
//! filter reader. A lens is what of a filter or a search the listing can
//! decide of a file (a kind, an extension, a tag, a date, a size, a
//! rating, a comment), kept by the pane as it shows other locations until
//! taken off; words that find names go with the location, and what only a
//! search can tell (what files contain, where and when they were taken)
//! is a search's. Its words are a query's, written by [`facets::refine`];
//! the core keeps them (`workspace::Lens`) with a test of each file, which
//! reads what the listing knows of the file, never the file. Its pills,
//! and the filter's, are the search's chips (`query.describe_filter`).

use std::path::Path;
use std::sync::Arc;

use skwir_core::entry::Entry;
use skwir_core::workspace::{Lens, ReadFilter};

use crate::facets;
use crate::lang::{self, Candidate, Expr, Term, Verdict};

/// Whether `t` is read from the file itself: only a search tells it.
fn reads_file(t: &Term) -> bool {
    matches!(
        t,
        Term::Content(_) | Term::Near { .. } | Term::Inside { .. } | Term::Taken { .. }
    )
}

/// Whether `t` asks what a file's metadata says (a size, a tag, a date):
/// a name, an extension, a type or a kind is read from the name.
fn reads_metadata(t: &Term) -> bool {
    !matches!(
        t,
        Term::Name(_) | Term::Ext(_) | Term::Type(_) | Term::Mime(..) | Term::Kind(_)
    )
}

/// Whether `item`, one of the terms a query ANDs, is a lens's: no word
/// that finds names in it, nothing read from the file. What decides it
/// everywhere: the lens a query holds, what a search is without it, and
/// which of its chips stay as the pane goes elsewhere (`facets`).
pub(crate) fn is_lens(item: &Expr) -> bool {
    !item.any_term(&|t| matches!(t, Term::Name(_)) || reads_file(t))
}

/// `q` with only its terms of the lens (`keep`) or only the others: where
/// it looks and how stay.
pub(crate) fn part(q: &lang::Query, keep: bool) -> lang::Query {
    let kept: Vec<Expr> = facets::items(q)
        .into_iter()
        .filter(|e| is_lens(e) == keep)
        .cloned()
        .collect();
    lang::Query {
        expr: and(kept),
        ..q.clone()
    }
}

/// The lens the query `expr` holds: its terms the listing can decide,
/// tested as a file's; `None` when it holds none (or does not read).
pub fn lens_of(expr: &str) -> Option<Lens> {
    let q = lang::parse(expr).ok()?;
    lens_in(&q)
}

/// The lens of a query read already.
fn lens_in(q: &lang::Query) -> Option<Lens> {
    let lens = part(q, true);
    let items = facets::items(&lens);
    if items.is_empty() {
        return None;
    }
    let words = items
        .iter()
        .map(|e| facets::print(e))
        .collect::<Vec<_>>()
        .join(" ");
    let metadata = items.iter().any(|e| e.any_term(&reads_metadata));
    let set = facets::set_facets(&words).unwrap_or_default();
    let test = lens.expr?;
    // Asked of every file at each redraw of the order, on the window's
    // thread: a type once per suffix (`lang::Types`).
    let types = lang::Types::default();
    Some(Lens {
        words: words.into(),
        set,
        keeps: Arc::new(move |entry| keeps(&test, entry, &types)),
        metadata,
    })
}

/// `expr` without its lens: its words, what it reads from the files and
/// where it looks, written as `facets::refine` writes a query (names as
/// typed). Two queries with the same are the same search but for their
/// lens.
pub fn without_lens(expr: &str) -> Option<String> {
    let q = lang::parse(expr).ok()?;
    Some(facets::written(&part(&q, false)))
}

/// The search `expr`, a step of a pane's history shown again, with the
/// pane's lens (`lens`, its words) in place of the one its terms held
/// (ADR-107: the lens is the pane's, never a step's). A search whose
/// every term is of the lens keeps them, as what it looks for, and takes
/// the pane's lens beside them, as any search brought one does.
pub fn in_place_of_lens(expr: &str, lens: &str) -> Result<String, String> {
    let q = lang::parse(expr).map_err(|e| e.to_string())?;
    let rest = part(&q, false);
    if rest.expr.is_none() {
        return facets::with_lens(expr, lens);
    }
    facets::with_lens(&facets::written(&rest), lens)
}

/// Whether `entry` is what `test` asks, from what the listing knows of
/// it: until its metadata comes, what depends on it is not known, and
/// the file is not shown.
fn keeps(test: &Expr, entry: &Entry, types: &lang::Types) -> bool {
    let mut c = Candidate::of_entry(entry, Path::new(&entry.name)).with_types(types);
    if !entry.metadata.is_hydrated() {
        c.metadata = None;
    }
    test.eval(&c, false) == Verdict::Yes
}

/// What words typed over a folder keep (ADR-098): the core's filter
/// reader. The longest start of `text` that reads as a query is read, so
/// a term still being typed (`dunes kind:im`) leaves the words before it
/// at work; when nothing reads, every entry stays (the address field says
/// what is wrong), never names that hold the text.
///
/// A word and `name:` hold for folders too; any other term holds for
/// files, and every folder stays, as a lens keeps them: those of them
/// alone in their part are its lens (ADR-107). What only a search can tell
/// (what files contain, where and when they were taken) is left out, and
/// said, until Enter.
pub fn read_filter(text: &str) -> Option<ReadFilter> {
    let everything = || ReadFilter {
        keeps: Arc::new(|_| true),
        waits: Vec::new(),
        metadata: false,
        lens: None,
    };
    let Some((q, _)) = longest_query(text) else {
        return Some(everything());
    };
    let all: Vec<Expr> = facets::items(&q).into_iter().cloned().collect();
    if all.is_empty() {
        return Some(everything());
    }
    let mut waits = Vec::new();
    let (mut names, mut files) = (Vec::new(), Vec::new());
    for item in all {
        if item.any_term(&reads_file) {
            let what = if item.any_term(&|t| matches!(t, Term::Content(_))) {
                "contents"
            } else if item.any_term(&|t| matches!(t, Term::Taken { .. })) {
                "dates taken"
            } else {
                "places"
            };
            if !waits.contains(&what) {
                waits.push(what);
            }
        } else if item.any_term(&|t| !matches!(t, Term::Name(_))) {
            files.push(item);
        } else {
            names.push(item);
        }
    }
    let lens = lens_in(&lang::Query {
        expr: and(files.clone()),
        ..q.clone()
    });
    let metadata = files.iter().any(|e| e.any_term(&reads_metadata));
    let names = Names::of(names);
    let files = and(files);
    let types = lang::Types::default();
    Some(ReadFilter {
        keeps: Arc::new(move |entry| {
            if !names.hold(entry) {
                return false;
            }
            if entry.is_dir() {
                return true;
            }
            let Some(files) = &files else {
                return true;
            };
            let mut c = Candidate::of_entry(entry, Path::new(&entry.name)).with_types(&types);
            if !entry.metadata.is_hydrated() {
                c.metadata = None;
            }
            files.eval(&c, false) == Verdict::Yes
        }),
        waits: waits.into_iter().map(str::to_owned).collect(),
        metadata,
        lens,
    })
}

/// What a filter's name terms ask of every entry at each key, over folders
/// of a hundred thousand names: plain words each found in the name as the
/// query finds them (case ignored unless `case:sensitive`), byte by byte
/// where both are ASCII, never making a candidate of the entry; anything
/// else (a glob, `ext:`, an OR) as the query evaluates it.
enum Names {
    None,
    Words(Vec<Word>),
    Expr(Expr),
}

/// A word to find in names: lowercased unless case counts.
struct Word {
    text: String,
    case_sensitive: bool,
}

impl Names {
    fn of(items: Vec<Expr>) -> Self {
        let words: Option<Vec<Word>> = items
            .iter()
            .map(|item| match item {
                Expr::Term(Term::Name(p)) if p.glob.is_none() => Some(Word {
                    text: if p.case_sensitive {
                        p.text.clone()
                    } else {
                        p.text.to_lowercase()
                    },
                    case_sensitive: p.case_sensitive,
                }),
                _ => None,
            })
            .collect();
        match (words, and(items)) {
            (_, None) => Names::None,
            (Some(words), Some(_)) => Names::Words(words),
            (None, Some(expr)) => Names::Expr(expr),
        }
    }

    fn hold(&self, entry: &Entry) -> bool {
        match self {
            Names::None => true,
            Names::Words(words) => words.iter().all(|w| w.in_name(&entry.name)),
            Names::Expr(expr) => {
                let c = Candidate::of_entry(entry, Path::new(&entry.name));
                expr.eval(&c, false) == Verdict::Yes
            }
        }
    }
}

impl Word {
    /// Whether `name` holds it, as `NamePat` finds a word.
    fn in_name(&self, name: &std::ffi::OsStr) -> bool {
        let bytes = name.as_encoded_bytes();
        let needle = self.text.as_bytes();
        if needle.is_empty() {
            return true;
        }
        if self.text.is_ascii() && bytes.is_ascii() {
            return if self.case_sensitive {
                bytes.windows(needle.len()).any(|w| w == needle)
            } else {
                bytes
                    .windows(needle.len())
                    .any(|w| w.eq_ignore_ascii_case(needle))
            };
        }
        let text = name.to_string_lossy();
        if self.case_sensitive {
            text.contains(&self.text)
        } else {
            text.to_lowercase().contains(&self.text)
        }
    }
}

/// What of `text` a filter applies: its longest start that reads, whole
/// words at a time; `None` when not even its first word reads.
pub fn applied(text: &str) -> Option<String> {
    longest_query(text).map(|(_, start)| start)
}

/// The query the longest start of `text` reads as, whole words at a time,
/// and that start; `None` when not even its first word reads.
fn longest_query(text: &str) -> Option<(lang::Query, String)> {
    let mut end = text.trim_end().len();
    loop {
        let start = text[..end].trim_end();
        if start.is_empty() {
            return None;
        }
        if let Ok(q) = lang::parse(start) {
            return Some((q, start.to_owned()));
        }
        end = start.rfind(char::is_whitespace)?;
    }
}

/// The items together, `None` for none.
fn and(mut items: Vec<Expr>) -> Option<Expr> {
    match items.len() {
        0 => None,
        1 => items.pop(),
        _ => Some(Expr::And(items)),
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use skwir_core::entry::{EntryId, EntryKind, FileId, Provenance, Value, keys};
    use skwir_core::{Entry, Metadata};

    use super::{applied, read_filter};

    fn entry(name: &str, kind: EntryKind, size: Option<u64>, tags: &[&str]) -> Entry {
        let id = EntryId::in_folder(
            FileId::new(1, name.len() as u64),
            FileId::new(1, 0),
            OsStr::new(name),
        );
        let mut e = Entry::new(id, name, kind);
        let mut m = Metadata::default();
        if let Some(size) = size {
            m.set(keys::SIZE, Value::UInt(size), Provenance::Filesystem);
        }
        if !tags.is_empty() {
            m.set(
                keys::TAGS,
                Value::List(tags.iter().map(|t| Value::Text((*t).into())).collect()),
                Provenance::Filesystem,
            );
        }
        e.metadata = m;
        e
    }

    fn kept(text: &str, entries: &[Entry]) -> Vec<String> {
        let read = read_filter(text).expect("read");
        entries
            .iter()
            .filter(|e| (read.keeps)(e))
            .map(|e| e.name.to_string_lossy().into_owned())
            .collect()
    }

    fn folder() -> Vec<Entry> {
        vec![
            entry("Maps", EntryKind::Directory, Some(0), &[]),
            entry("dunes", EntryKind::Directory, Some(0), &[]),
            entry("dunes.jpg", EntryKind::File, Some(6_000_000), &[]),
            entry("pines.jpg", EntryKind::File, Some(4_000_000), &["print"]),
            entry("README.md", EntryKind::File, Some(485), &["draft"]),
            entry("route.gpx", EntryKind::File, None, &[]),
        ]
    }

    #[test]
    fn every_word_holds_on_its_own_as_the_search_reads_it() {
        assert_eq!(kept("dunes jpg", &folder()), ["dunes.jpg"]);
        // Case ignored as the search ignores it, unless it is asked to count.
        assert_eq!(kept("DUNES JPG", &folder()), ["dunes.jpg"]);
        assert_eq!(kept("README", &folder()), ["README.md"]);
        assert!(kept("readme case:sensitive", &folder()).is_empty());
        // Not ASCII, and a glob: as the query matches them.
        let names = vec![
            entry("Été.txt", EntryKind::File, Some(1), &[]),
            entry("hiver.txt", EntryKind::File, Some(1), &[]),
        ];
        assert_eq!(kept("été", &names), ["Été.txt"]);
        assert_eq!(kept("h*.txt", &names), ["hiver.txt"]);
    }

    #[test]
    fn terms_the_listing_knows_hold_for_files_and_folders_stay() {
        assert_eq!(
            kept("kind:image tag:print", &folder()),
            ["Maps", "dunes", "pines.jpg"]
        );
        // A file whose metadata has not come waits hidden.
        assert_eq!(
            kept("size>1k", &folder()),
            ["Maps", "dunes", "dunes.jpg", "pines.jpg"]
        );
    }

    #[test]
    fn a_term_still_typed_leaves_the_words_before_it_at_work() {
        assert_eq!(kept("dunes kind:im", &folder()), ["dunes", "dunes.jpg"]);
        // Nothing reads yet: everything stays, never names holding `kind:im`.
        assert_eq!(kept("kind:im", &folder()).len(), folder().len());
    }

    #[test]
    fn what_only_a_search_can_tell_waits_and_is_said() {
        let read = read_filter(r#"README content:"water filter""#).expect("read");
        assert_eq!(read.waits, ["contents"]);
        assert!(!read.metadata);
        let names: Vec<String> = folder()
            .iter()
            .filter(|e| (read.keeps)(e))
            .map(|e| e.name.to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["README.md"]);
    }

    /// What a key costs over a folder of 100 000 names, as the filter
    /// typed asks it of each: `cargo nextest run --release -p
    /// skwir-plugin-query --run-ignored only -E 'test(a_key_over)'`.
    #[test]
    #[ignore = "a measure, run in release"]
    fn a_key_over_a_hundred_thousand_names() {
        let names: Vec<Entry> = (0..100_000)
            .map(|i| {
                let kind = if i % 10 == 0 {
                    EntryKind::Directory
                } else {
                    EntryKind::File
                };
                entry(&format!("file-{i:06}.jpg"), kind, Some(i), &[])
            })
            .collect();
        // The type database is loaded once in a process: not a key's cost.
        let _ = skwir_platform::mime::by_name(OsStr::new("warm.jpg"));
        let lens = super::lens_of("kind:image").expect("a lens");
        let start = std::time::Instant::now();
        let kept = names.iter().filter(|e| lens.shows(e)).count();
        eprintln!(
            "{:>24}: {kept:>6} kept in {:?}",
            "Lens kind:image",
            start.elapsed()
        );
        for text in [
            "file-0420",
            "dunes jpg",
            "size>50k",
            "tag:print",
            "ext:jpg",
            "kind:image",
            "kind:image size>50k",
        ] {
            let read = read_filter(text).expect("read");
            let start = std::time::Instant::now();
            let kept = names.iter().filter(|e| (read.keeps)(e)).count();
            eprintln!("{text:>24}: {kept:>6} kept in {:?}", start.elapsed());
        }
    }

    #[test]
    fn a_filter_applies_its_longest_start_that_reads() {
        assert_eq!(
            applied("dunes kind:image").as_deref(),
            Some("dunes kind:image")
        );
        assert_eq!(applied("dunes kind:im").as_deref(), Some("dunes"));
        assert_eq!(applied("kind:im"), None);
        // A kind is read from the name; a size from the file's metadata.
        assert!(!read_filter("dunes kind:image").expect("read").metadata);
        assert!(read_filter("dunes size>1k").expect("read").metadata);
    }

    /// An extension is a lens's (ADR-107): it holds for files, and every
    /// folder stays, as a kind does.
    #[test]
    fn an_extension_keeps_the_folders() {
        assert_eq!(
            kept("ext:jpg", &folder()),
            ["Maps", "dunes", "dunes.jpg", "pines.jpg"]
        );
        let read = read_filter("dunes ext:jpg").expect("read");
        assert_eq!(
            read.lens.map(|l| l.words.to_string()).as_deref(),
            Some("ext:jpg")
        );
    }
}
