//! A search under a names index (ADR-150): what it proves is considered
//! instead of a walk, what it does not cover is walked, and nothing shown is
//! taken back. A fake index, of each test's making, through the public
//! location provider.
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};
use std::time::{Duration, SystemTime};

use parking_lot::Mutex;
use skwir_core::cancel::CancelToken;
use skwir_core::coverage::{Coverage, Hole, Proof, Why};
use skwir_core::location::{ListingEvent, LocationProvider, OpenContext, RestsOn};
use skwir_core::names::{Answer, NameIndex, NameIndexes, Opened, Query, Session};
use skwir_core::scheduler::Scheduler;
use skwir_core::words::Words;
use skwir_plugin_query::provider::{QueryProvider, Searches};

fn at() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_007)
}

fn proven(names: Proof, holes: Vec<Hole>) -> Coverage {
    Coverage {
        names,
        attrs: Proof::AsOf { at: Some(at()) },
        holes,
    }
}

/// What a query of the fake answers: the paths it names (relative to the
/// root), the coverage that held, whether it was cut short. `None`: it
/// cannot answer.
type Said = Option<(Vec<&'static str>, Coverage, bool)>;

/// A names index of a test's making.
struct Fake {
    by: &'static str,
    root: PathBuf,
    /// How it covers the root at open; below `elsewhere`, not at all.
    coverage: Coverage,
    elsewhere: Option<(PathBuf, Why)>,
    syncs: bool,
    /// What a sync answers.
    synced: Option<Coverage>,
    /// What each query answers, in turn (the last one again after).
    queries: Vec<Said>,
    /// What was asked of it, in order.
    calls: Arc<Mutex<Vec<String>>>,
}

impl Fake {
    fn new(root: &Path, coverage: Coverage, queries: Vec<Said>) -> Fake {
        Fake {
            by: "the fake index",
            root: root.to_path_buf(),
            coverage,
            elsewhere: None,
            syncs: true,
            synced: None,
            queries,
            calls: Arc::default(),
        }
    }
}

struct FakeSession {
    root: PathBuf,
    synced: Option<Coverage>,
    queries: Vec<Said>,
    asked: usize,
    calls: Arc<Mutex<Vec<String>>>,
}

impl Session for FakeSession {
    fn sync(&mut self) -> Option<Coverage> {
        self.calls.lock().push("sync".into());
        self.synced.clone()
    }

    fn query(&mut self, q: &Query, each: &mut dyn FnMut(Vec<PathBuf>) -> bool) -> Option<Answer> {
        self.calls.lock().push(format!(
            "query {}",
            q.names
                .iter()
                .map(|n| n.text.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        ));
        let said = self.queries[self.asked.min(self.queries.len() - 1)].clone();
        self.asked += 1;
        let (paths, coverage, truncated) = said?;
        // One path a batch, as an index streams.
        for path in paths {
            if !each(vec![self.root.join(path)]) {
                break;
            }
        }
        Some(Answer {
            coverage,
            truncated,
        })
    }
}

impl NameIndex for Fake {
    fn id(&self) -> &'static str {
        self.by
    }
    fn by(&self) -> Words {
        Words::as_written(self.by)
    }
    fn syncs(&self) -> bool {
        self.syncs
    }
    fn open(&self, root: &Path, _: &CancelToken) -> Option<Opened> {
        self.calls.lock().push("open".into());
        let coverage = match &self.elsewhere {
            Some((below, why)) if root.starts_with(below) => Coverage::not_covered(*why),
            _ => self.coverage.clone(),
        };
        Some(Opened {
            coverage,
            session: Box::new(FakeSession {
                root: self.root.clone(),
                synced: self.synced.clone(),
                queries: self.queries.clone(),
                asked: 0,
                calls: Arc::clone(&self.calls),
            }),
        })
    }
}

/// What a search found (relative to the root, written with `/`), what its
/// answer rests on, and whether it took anything back.
struct Searched {
    found: BTreeSet<String>,
    rests_on: Vec<Option<RestsOn>>,
    /// The order things were said in: "added <names>", "rests on".
    said: Vec<String>,
    removed: bool,
}

fn search(root: &Path, expr: &str, indexes: Vec<Fake>) -> Searched {
    let names = NameIndexes::default();
    for index in indexes {
        names.register("test", Arc::new(index));
    }
    let provider = QueryProvider::new(CancelToken::new(), Searches::default(), Weak::new());
    let scheduler = Scheduler::new(2);
    let ctx = OpenContext {
        scheduler: scheduler.clone(),
        cancel: CancelToken::new(),
        batch_size: 1,
        waker: None,
        metadata: Arc::default(),
        fields: Arc::default(),
        hints: Arc::default(),
        names: Arc::new(names),
        companions: Arc::default(),
        hidden_shown: false,
        reads_metadata: false,
    };
    let location = skwir_plugin_query::location_for(expr, Some(root));
    let open = provider.open(&location, ctx).unwrap();
    assert!(scheduler.wait_idle(Duration::from_secs(10)));
    let mut searched = Searched {
        found: BTreeSet::new(),
        rests_on: Vec::new(),
        said: Vec::new(),
        removed: false,
    };
    let mut complete = false;
    for event in open.events().try_iter() {
        match event {
            ListingEvent::Added(entries) => {
                let names: Vec<String> = entries
                    .into_iter()
                    .map(|entry| {
                        let path = open.path_of(entry.id).unwrap();
                        path.strip_prefix(root)
                            .unwrap()
                            .components()
                            .map(|c| c.as_os_str().to_string_lossy().into_owned())
                            .collect::<Vec<_>>()
                            .join("/")
                    })
                    .collect();
                searched.said.push(format!("added {}", names.join(" ")));
                searched.found.extend(names);
            }
            ListingEvent::RestsOn(rests_on) => {
                searched.said.push("rests on".into());
                searched.rests_on.push(rests_on);
            }
            ListingEvent::Removed(_) => searched.removed = true,
            ListingEvent::Complete => complete = true,
            ListingEvent::Error(e) => panic!("{expr}: {e}"),
            _ => {}
        }
    }
    assert!(complete, "{expr}: complete");
    searched
}

/// A tree of reports: two found by any search, one in a denied subtree a
/// hole names, one in a snapshot, one hidden, and one no index names.
fn tree() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    // The real path: an index knows no other.
    let root = std::fs::canonicalize(tmp.path()).unwrap().join("search");
    for name in [
        "docs/report.pdf",
        "docs/notes.txt",
        "docs/deep/report old.txt",
        "shut/report in a hole.txt",
        "snap/report copy.txt",
        ".hidden/report secret.txt",
        "unnamed/report nobody named.txt",
    ] {
        let path = root.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "fixture").unwrap();
    }
    (tmp, root)
}

fn set(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|s| (*s).to_owned()).collect()
}

fn rests_on(proof: Proof) -> Vec<Option<RestsOn>> {
    vec![Some(RestsOn {
        by: Words::as_written("the fake index"),
        proof,
    })]
}

/// An index that names every match outside its holes, and more (a file
/// that does not match, one gone since, one hidden), answers what the walk
/// does: its holes walked, the rest never.
#[test]
fn a_proven_answer_equals_the_walk_s() {
    let (_tmp, root) = tree();
    let walked = search(&root, "report", Vec::new());
    assert_eq!(
        walked.found,
        set(&[
            "docs/report.pdf",
            "docs/deep/report old.txt",
            "shut/report in a hole.txt",
            "snap/report copy.txt",
            "unnamed/report nobody named.txt",
        ])
    );
    assert_eq!(walked.rests_on, [None], "a walk rests on no index");

    let holes = vec![
        Hole {
            path: root.join("shut"),
            why: Why::Denied,
        },
        Hole {
            path: root.join("shut/inner"),
            why: Why::Unread,
        },
        Hole {
            path: root.join(".hidden/below"),
            why: Why::Failed(5),
        },
    ];
    let covered = proven(Proof::Checked { at: at() }, holes);
    let names = vec![
        "docs/report.pdf",
        "docs/notes.txt",
        "docs/deep/report old.txt",
        "snap/report copy.txt",
        "unnamed/report nobody named.txt",
        ".hidden/report secret.txt",
        "gone/report gone.txt",
    ];
    let mut index = Fake::new(
        &root,
        covered.clone(),
        vec![Some((names.clone(), covered.clone(), false))],
    );
    index.synced = Some(covered.clone());
    let calls = Arc::clone(&index.calls);
    let indexed = search(&root, "report", vec![index]);
    assert_eq!(indexed.found, walked.found, "what the walk finds");
    assert!(!indexed.removed);
    assert_eq!(indexed.rests_on, rests_on(Proof::Checked { at: at() }));
    assert_eq!(
        *calls.lock(),
        ["open", "query report", "sync", "query report"]
    );
}

/// Under a proof, what the index does not name and no hole holds is not
/// looked for: the walk is skipped.
#[test]
fn a_proof_skips_the_walk() {
    let (_tmp, root) = tree();
    let covered = proven(Proof::Live { since: at() }, Vec::new());
    let mut index = Fake::new(
        &root,
        covered.clone(),
        vec![Some((vec!["docs/report.pdf"], covered.clone(), false))],
    );
    index.synced = Some(covered);
    let calls = Arc::clone(&index.calls);
    let indexed = search(&root, "report", vec![index]);
    assert_eq!(indexed.found, set(&["docs/report.pdf"]));
    assert_eq!(indexed.rests_on, rests_on(Proof::Live { since: at() }));
    // Live: the barrier first, then what it holds, asked once.
    assert_eq!(*calls.lock(), ["open", "sync", "query report"]);
}

/// Not yet proven, what the index names shows at once; the sync proves it,
/// what it names then is added, and nothing is taken back; the proof is
/// said once, after every file.
#[test]
fn an_answer_shows_at_once_and_is_proven_after() {
    let (_tmp, root) = tree();
    let read_back = proven(Proof::AsOf { at: Some(at()) }, Vec::new());
    let checked = proven(Proof::Checked { at: at() }, Vec::new());
    let mut index = Fake::new(
        &root,
        read_back.clone(),
        vec![
            Some((vec!["docs/report.pdf"], read_back, false)),
            Some((
                vec!["docs/report.pdf", "docs/deep/report old.txt"],
                checked.clone(),
                false,
            )),
        ],
    );
    index.synced = Some(checked);
    let indexed = search(&root, "report", vec![index]);
    assert_eq!(
        indexed.found,
        set(&["docs/report.pdf", "docs/deep/report old.txt"])
    );
    assert!(!indexed.removed, "nothing shown is taken back");
    assert_eq!(
        indexed.said,
        [
            "added docs/report.pdf",
            "added docs/deep/report old.txt",
            "rests on"
        ],
        "each file once, the proof once, last"
    );
    assert_eq!(indexed.rests_on, rests_on(Proof::Checked { at: at() }));
}

/// An index that cannot be synced (plocate) answers as of when it is, the
/// walk skipped; one that can and whose sync fails is walked, what it
/// showed kept.
#[test]
fn as_of_stands_where_no_sync_can_prove_it_and_is_walked_where_one_failed() {
    let (_tmp, root) = tree();
    let as_of = Coverage::as_of(Some(at()));
    let answer = Some((vec!["docs/report.pdf"], as_of.clone(), false));
    let mut snapshot = Fake::new(&root, as_of.clone(), vec![answer.clone()]);
    snapshot.syncs = false;
    let indexed = search(&root, "report", vec![snapshot]);
    assert_eq!(indexed.found, set(&["docs/report.pdf"]));
    assert_eq!(
        indexed.rests_on,
        rests_on(Proof::AsOf { at: Some(at()) }),
        "dated"
    );

    let failing = Fake::new(&root, as_of.clone(), vec![answer.clone()]);
    let calls = Arc::clone(&failing.calls);
    let walked = search(&root, "report", vec![failing]);
    assert_eq!(*calls.lock(), ["open", "query report", "sync"]);
    assert!(walked.found.contains("unnamed/report nobody named.txt"));
    assert!(walked.found.contains("docs/report.pdf"));
    assert!(!walked.removed);
    assert_eq!(walked.rests_on, [None], "a walk answered");

    // Synced, but answering still only as of: not proven, so walked.
    let mut unproven = Fake::new(&root, as_of, vec![answer]);
    unproven.synced = Some(proven(Proof::Checked { at: at() }, Vec::new()));
    let calls = Arc::clone(&unproven.calls);
    let walked = search(&root, "report", vec![unproven]);
    assert_eq!(
        *calls.lock(),
        ["open", "query report", "sync", "query report"]
    );
    assert!(walked.found.contains("unnamed/report nobody named.txt"));
    assert_eq!(walked.rests_on, [None], "a walk answered");
}

/// Where no index covers the root, where the one that does cannot answer
/// the query, or where its answer was cut short, the walk answers.
#[test]
fn the_walk_answers_where_no_index_does() {
    let (_tmp, root) = tree();
    let everything = set(&[
        "docs/report.pdf",
        "docs/deep/report old.txt",
        "shut/report in a hole.txt",
        "snap/report copy.txt",
        "unnamed/report nobody named.txt",
    ]);
    let not_covered = Fake::new(&root, Coverage::not_covered(Why::Outside), vec![None]);
    let calls = Arc::clone(&not_covered.calls);
    let walked = search(&root, "report", vec![not_covered]);
    assert_eq!(walked.found, everything);
    assert_eq!(*calls.lock(), ["open"], "never asked");
    assert_eq!(walked.rests_on, [None]);

    let live = proven(Proof::Live { since: at() }, Vec::new());
    let mut declining = Fake::new(&root, live.clone(), vec![None]);
    declining.synced = Some(live.clone());
    let walked = search(&root, "report", vec![declining]);
    assert_eq!(walked.found, everything);
    assert_eq!(walked.rests_on, [None]);

    let mut cut_short = Fake::new(
        &root,
        live.clone(),
        vec![Some((vec!["docs/report.pdf"], live.clone(), true))],
    );
    cut_short.synced = Some(live);
    let walked = search(&root, "report", vec![cut_short]);
    assert_eq!(walked.found, everything);
    assert!(!walked.removed);
    assert_eq!(walked.rests_on, [None]);

    // A search an index cannot be asked (no name, no kind): never asked.
    let tagged = Fake::new(
        &root,
        proven(Proof::Live { since: at() }, Vec::new()),
        vec![None],
    );
    let calls = Arc::clone(&tagged.calls);
    search(&root, "tag:x", vec![tagged]);
    assert!(calls.lock().is_empty(), "{:?}", calls.lock());
}

/// A snapshot below the folder searched is passed over, walked with
/// `excluded:yes`, and walked when searched in.
#[test]
fn a_snapshot_is_passed_over_below_the_folder_searched() {
    let (_tmp, root) = tree();
    let covered = proven(
        Proof::Live { since: at() },
        vec![Hole {
            path: root.join("snap"),
            why: Why::Snapshot,
        }],
    );
    let index = || {
        let mut index = Fake::new(
            &root,
            covered.clone(),
            vec![Some((vec!["docs/report.pdf"], covered.clone(), false))],
        );
        index.synced = Some(covered.clone());
        index.elsewhere = Some((root.join("snap"), Why::Snapshot));
        index
    };
    let passed = search(&root, "report", vec![index()]);
    assert_eq!(passed.found, set(&["docs/report.pdf"]));
    let walked = search(&root, "report excluded:yes", vec![index()]);
    assert_eq!(
        walked.found,
        set(&["docs/report.pdf", "snap/report copy.txt"])
    );
    let inside = search(&root.join("snap"), "report", vec![index()]);
    assert_eq!(inside.found, set(&["report copy.txt"]));
    assert_eq!(inside.rests_on, [None], "walked");
}

/// Of two indexes covering the root, the stronger proof answers.
#[test]
fn the_strongest_proof_answers() {
    let (_tmp, root) = tree();
    let as_of = Coverage::as_of(Some(at()));
    let mut weaker = Fake::new(
        &root,
        as_of.clone(),
        vec![Some((vec!["docs/report.pdf"], as_of, false))],
    );
    weaker.by = "the weaker index";
    weaker.syncs = false;
    let live = proven(Proof::Live { since: at() }, Vec::new());
    let mut stronger = Fake::new(
        &root,
        live.clone(),
        vec![Some((
            vec!["docs/deep/report old.txt"],
            live.clone(),
            false,
        ))],
    );
    stronger.synced = Some(live);
    let calls = Arc::clone(&weaker.calls);
    let indexed = search(&root, "report", vec![weaker, stronger]);
    assert_eq!(indexed.found, set(&["docs/deep/report old.txt"]));
    assert_eq!(indexed.rests_on, rests_on(Proof::Live { since: at() }));
    assert_eq!(*calls.lock(), ["open"], "opened, never asked");
}
