//! skwird as a names index, asked of the client's fake engine on a socket
//! of each test's own: what it covers, names and proves, said in core's
//! words; no engine is none; a search given up on stops its call.

use super::*;
use skwir_core::names::NameTerm;
use skwird_client::fake::{Fake, Script};

const AT: i64 = 1_800_000_007;

fn instant() -> protocol::Instant {
    protocol::Instant { sec: AT, nsec: 5 }
}

fn at() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_007) + Duration::from_nanos(5)
}

fn entry(path: &Path, kind: protocol::Kind) -> protocol::Entry {
    protocol::Entry {
        path: protocol::WirePath::of(path),
        kind,
        id: protocol::Id { dev: 1, ino: 2 },
        folder: protocol::Id { dev: 1, ino: 1 },
        attrs: None,
    }
}

/// A fake engine covering `root` checked, with a denied hole, and naming
/// three entries below it.
fn engine(dir: &Path, root: &Path, silent: bool) -> Fake {
    let script = Script {
        roots: vec![root.to_path_buf()],
        coverage: vec![(
            root.to_path_buf(),
            protocol::Coverage {
                names: protocol::Proof::Checked(instant()),
                attrs: protocol::Proof::AsOf(instant()),
                holes: vec![protocol::Hole {
                    path: protocol::WirePath::of(&root.join("shut")),
                    why: protocol::Why::Denied,
                }],
            },
        )],
        entries: vec![
            entry(&root.join("a report.txt"), protocol::Kind::File),
            entry(&root.join("notes.txt"), protocol::Kind::File),
            entry(&root.join("docs/Report.pdf"), protocol::Kind::File),
        ],
        silent,
        ..Script::default()
    };
    Fake::start(dir.join("socket"), script).unwrap()
}

fn index(engines: Engines) -> Index {
    Index {
        app: Weak::new(),
        kept: kept::Kept::new(Weak::new(), engines.clone()),
        engines,
        secrets: Mutex::default(),
    }
}

#[test]
fn a_search_is_answered_as_skwird_covers_it() {
    let tmp = tempfile::tempdir().unwrap();
    let root = PathBuf::from("/fixture/root");
    let fake = engine(tmp.path(), &root, false);
    let index = index(Engines::At(fake.socket.clone()));
    let cancel = CancelToken::new();
    let Opened {
        coverage,
        mut session,
    } = index.open(&root, &cancel).expect("the fake answers");
    let checked = Coverage {
        names: Proof::Checked { at: at() },
        attrs: Proof::AsOf { at: Some(at()) },
        holes: vec![Hole {
            path: root.join("shut"),
            why: Why::Denied,
        }],
    };
    assert_eq!(coverage, checked, "in core's words, to the nanosecond");
    assert_eq!(session.sync(), Some(checked.clone()));
    let query = Query {
        root: root.clone(),
        names: vec![NameTerm {
            text: "report".into(),
            case_sensitive: false,
        }],
        kinds: Some(vec![EntryKind::File, EntryKind::Symlink]),
        size: None,
        modified: None,
        whole: true,
    };
    let mut named = Vec::new();
    let answer = session
        .query(&query, &mut |paths| {
            named.extend(paths);
            true
        })
        .expect("answered");
    named.sort();
    assert_eq!(
        named,
        [root.join("a report.txt"), root.join("docs/Report.pdf")]
    );
    assert_eq!(
        answer,
        Answer {
            coverage: checked,
            truncated: false
        }
    );
}

#[test]
fn what_skwird_is_asked_is_one_name_the_longest_and_the_kinds() {
    let term = |text: &str| NameTerm {
        text: text.into(),
        case_sensitive: true,
    };
    let asked = query_of(&Query {
        root: PathBuf::from("/r"),
        names: vec![term("q3"), term("report"), term("*.pdf")],
        kinds: Some(vec![
            EntryKind::Symlink,
            EntryKind::Directory,
            EntryKind::Symlink,
        ]),
        size: None,
        modified: None,
        whole: false,
    });
    let name = asked.name.expect("a name");
    assert_eq!(name.text, "report");
    assert_eq!(name.how, protocol::Match::Skwir);
    assert!(name.case_sensitive);
    assert_eq!(
        asked.kinds,
        Some(vec![protocol::Kind::Folder, protocol::Kind::Link]),
        "each once"
    );
    assert_eq!((asked.size, asked.mtime, asked.limit), (None, None, None));
    let kinds_alone = query_of(&Query {
        root: PathBuf::from("/r"),
        names: Vec::new(),
        kinds: Some(vec![EntryKind::Directory]),
        size: None,
        modified: None,
        whole: true,
    });
    assert!(kinds_alone.name.is_none());
}

/// Sizes and modification times are passed on, both ends included, an open
/// end skwird's widest; a link then answered whatever its own, its being
/// its target's in a search (ADR-150, D2).
#[test]
fn sizes_and_times_are_asked_and_links_answered_whatever_their_own() {
    let at = |sec: u64| SystemTime::UNIX_EPOCH + Duration::from_secs(sec);
    let q = Query {
        root: PathBuf::from("/r"),
        names: Vec::new(),
        kinds: None,
        size: Some((10, u64::MAX)),
        modified: Some(skwir_core::names::Times {
            from: Some(at(1_800_000_000)),
            to: None,
        }),
        whole: true,
    };
    let asked = query_of(&q);
    assert_eq!(asked.size, Some([10, u64::MAX]));
    let [from, to] = asked.mtime.expect("dated");
    assert_eq!((from.sec, from.nsec, to.sec), (1_800_000_000, 0, i64::MAX));
    assert_eq!(asked.links, protocol::Links::Unfiltered);
    let undated = Query {
        size: None,
        modified: None,
        ..q
    };
    assert_eq!(
        query_of(&undated).links,
        protocol::Links::Own,
        "nothing a link's own would change"
    );
    // Before 1970, as `time_of` reads it back.
    let before = SystemTime::UNIX_EPOCH - Duration::from_millis(1500);
    let said = instant_of(before);
    assert_eq!((said.sec, said.nsec), (-2, 500_000_000));
    assert_eq!(time_of(said), before);
}

#[test]
fn no_engine_is_no_index() {
    let tmp = tempfile::tempdir().unwrap();
    let cancel = CancelToken::new();
    let nobody = index(Engines::At(tmp.path().join("no socket")));
    assert!(nobody.open(Path::new("/fixture"), &cancel).is_none());
    // A headless test's app asks the machine nothing.
    assert!(
        index(Engines::None)
            .open(Path::new("/fixture"), &cancel)
            .is_none()
    );
}

/// An engine that says hello and then nothing holds the search until it is
/// given up on, then lets it go at once, long before its silence ends.
#[test]
fn a_search_given_up_on_stops_its_call() {
    let tmp = tempfile::tempdir().unwrap();
    let root = PathBuf::from("/fixture/root");
    let fake = engine(tmp.path(), &root, true);
    let index = Arc::new(index(Engines::At(fake.socket.clone())));
    let cancel = CancelToken::new();
    let started = std::time::Instant::now();
    let asking = std::thread::spawn({
        let (index, cancel, root) = (Arc::clone(&index), cancel.clone(), root.clone());
        move || index.open(&root, &cancel).is_some()
    });
    // Time for the call to block on the silent engine; whether it has or
    // not, a stop given before or during it ends it alike.
    std::thread::sleep(Duration::from_millis(200));
    cancel.cancel();
    assert!(!asking.join().unwrap(), "given up on");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "stopped, not waited out: {:?}",
        started.elapsed()
    );
}

#[test]
fn instants_are_the_system_s_before_1970_too() {
    assert_eq!(time_of(instant()), at());
    assert_eq!(
        time_of(protocol::Instant { sec: -2, nsec: 5 }),
        SystemTime::UNIX_EPOCH - Duration::from_secs(2) + Duration::from_nanos(5)
    );
}

#[test]
fn the_secrets_kept_out_are_the_folders_of_the_secrets_category() {
    let folders = secret_folders(&Catalog::shipped());
    assert!(!folders.is_empty());
    assert!(folders.iter().all(|p| p.is_absolute()), "{folders:?}");
    let mut once = folders.clone();
    once.dedup();
    assert_eq!(once, folders, "each once");
    assert!(
        folders.iter().any(|p| p.ends_with(".ssh")),
        "SSH's keys among them"
    );
}

#[test]
fn what_is_kept_out_is_sent_to_the_engine() {
    let tmp = tempfile::tempdir().unwrap();
    let root = PathBuf::from("/fixture/root");
    let fake = engine(tmp.path(), &root, false);
    let mut client = Client::at(&fake.socket, CLIENT).unwrap();
    let folders = vec![
        PathBuf::from("/fixture/keys"),
        PathBuf::from("/fixture/vault"),
    ];
    assert!(keep_out(&mut client, &folders));
}

/// A root skwird is still reading (a first build) says so at once, and is
/// asked nothing more: the search walks it, never syncing it, as a sync
/// waits for the build's end.
#[test]
fn a_root_being_read_is_walked_and_never_synced() {
    let tmp = tempfile::tempdir().unwrap();
    let root = PathBuf::from("/fixture/root");
    let unread = protocol::Coverage {
        names: protocol::Proof::NotCovered(protocol::Why::Unread),
        attrs: protocol::Proof::NotCovered(protocol::Why::Unread),
        holes: Vec::new(),
    };
    let fake = Fake::start(
        tmp.path().join("socket"),
        Script {
            roots: vec![root.clone()],
            coverage: vec![(root.clone(), unread)],
            entries: vec![entry(&root.join("a report.txt"), protocol::Kind::File)],
            ..Script::default()
        },
    )
    .unwrap();
    let indexes = skwir_core::names::NameIndexes::default();
    indexes.register(ID, Arc::new(index(Engines::At(fake.socket.clone()))));
    assert!(
        indexes.open(&root, &CancelToken::new()).is_empty(),
        "not covered: the search walks"
    );
}

/// A root's journal followed through the client, from a running engine,
/// nothing started (ADR-150, D5): asked from a mark and from now, its
/// changes in core's words, how far they are every change as skwird says
/// it, its mark skwird's.
#[test]
fn a_journal_is_followed_as_skwird_tells_it() {
    let tmp = tempfile::tempdir().unwrap();
    let root = PathBuf::from("/fixture/root");
    let live = protocol::Coverage {
        names: protocol::Proof::Live(instant()),
        attrs: protocol::Proof::Live(instant()),
        holes: Vec::new(),
    };
    let fake = Fake::start(
        tmp.path().join("socket"),
        Script {
            roots: vec![root.clone()],
            coverage: vec![(root.clone(), live)],
            changes: Some(protocol::Changes {
                events: vec![
                    protocol::Event::Changed(entry(&root.join("tagged.txt"), protocol::Kind::File)),
                    protocol::Event::Removed(entry(&root.join("gone"), protocol::Kind::Folder)),
                    protocol::Event::NotCovered {
                        path: protocol::WirePath::of(&root.join("shut")),
                        why: protocol::Why::Denied,
                    },
                ],
                token: "e1.42".into(),
                complete: protocol::Complete {
                    names: true,
                    attrs: true,
                },
                holes: vec![protocol::Hole {
                    path: protocol::WirePath::of(&root.join("private")),
                    why: protocol::Why::Private,
                }],
            }),
            ..Script::default()
        },
    )
    .unwrap();
    let cancel = CancelToken::new();
    let index = index(Engines::At(fake.socket.clone()));
    let mut session = index
        .follow(&root, &cancel)
        .expect("an engine runs")
        .session;
    let mark = Mark {
        by: ID.into(),
        token: "e1.7".into(),
    };
    let Some(Since::Journal(journal)) = session.changes(Some(&mark)) else {
        panic!("a journal");
    };
    assert_eq!(
        journal.changes,
        [
            Change::Changed(Noted {
                path: root.join("tagged.txt"),
                kind: EntryKind::File,
                id: Some((1, 2)),
            }),
            Change::Removed(Noted {
                path: root.join("gone"),
                kind: EntryKind::Directory,
                id: Some((1, 2)),
            }),
            Change::NotCovered(Hole {
                path: root.join("shut"),
                why: Why::Denied,
            }),
        ]
    );
    assert_eq!(journal.mark.token, "e1.42");
    assert_eq!(journal.mark.by, ID);
    assert!(journal.names);
    assert_eq!(journal.attrs, !cfg!(windows), "not proven on Windows yet");
    assert_eq!(journal.holes.len(), 1);
    // From now, and another index's mark asks nothing.
    assert!(matches!(session.changes(None), Some(Since::Journal(_))));
    let other = Mark {
        by: "plocate".into(),
        token: "x".into(),
    };
    assert!(session.changes(Some(&other)).is_none());
    assert_eq!(
        fake.changes_asked(),
        [(root.clone(), "e1.7".to_owned()), (root, String::new())]
    );
    // No engine: none followed.
    let nobody = index_at_nothing(tmp.path());
    assert!(nobody.follow(Path::new("/fixture/root"), &cancel).is_none());
}

fn index_at_nothing(dir: &Path) -> Index {
    index(Engines::At(dir.join("no socket")))
}

/// A folder's totals through the client, in core's words (ADR-150, D3):
/// each entry's, the folder's own, the journal's mark as of them, the
/// coverage; asked for the largest alone, the others summed.
#[test]
fn a_folder_is_summed_as_skwird_sums_it() {
    let tmp = tempfile::tempdir().unwrap();
    let root = PathBuf::from("/fixture/root");
    let file = |name: &str, alloc: u64| protocol::Child {
        path: protocol::WirePath::of(&root.join(name)),
        kind: protocol::Kind::File,
        id: protocol::Id { dev: 1, ino: alloc },
        mtime: Some(instant()),
        totals: protocol::Totals {
            size: alloc - 1,
            alloc,
            files: 1,
            newest_file_mtime: Some(instant()),
            by_extension: vec![protocol::Extension {
                ext: Some(protocol::WirePath::of(Path::new("txt"))),
                files: 1,
                alloc,
            }],
            ..protocol::Totals::default()
        },
    };
    let children: Vec<protocol::Child> = (1..=5)
        .map(|n| file(&format!("f{n}.txt"), n * 10))
        .collect();
    let mut own = protocol::Totals::default();
    for child in &children {
        own.add_child(child);
    }
    let live = protocol::Coverage {
        names: protocol::Proof::Live(instant()),
        attrs: protocol::Proof::Live(instant()),
        holes: Vec::new(),
    };
    let fake = Fake::start(
        tmp.path().join("socket"),
        Script {
            roots: vec![root.clone()],
            coverage: vec![(root.clone(), live.clone())],
            totals: vec![(
                root.clone(),
                children,
                protocol::TotalsDone {
                    totals: own.clone(),
                    mtime: Some(instant()),
                    token: "e1.9".into(),
                    coverage: live,
                    nlink: protocol::Nlink::Complete,
                    count: 5,
                    rest: None,
                    cancelled: false,
                },
            )],
            ..Script::default()
        },
    )
    .unwrap();
    let cancel = CancelToken::new();
    let index = index(Engines::At(fake.socket.clone()));
    let mut session = index
        .follow(&root, &cancel)
        .expect("an engine runs")
        .session;
    let mut sent = Vec::new();
    let summed = session
        .totals(&root, None, &mut |batch| {
            sent.extend(batch);
            true
        })
        .expect("summed");
    assert_eq!(sent.len(), 5);
    assert_eq!(sent[0].path, root.join("f1.txt"));
    assert_eq!(sent[0].kind, EntryKind::File);
    assert_eq!(sent[0].id, Some((1, 10)));
    assert_eq!(sent[0].totals.newest_file, Some(at()));
    assert_eq!(sent[0].mtime, Some(at()));
    assert_eq!(
        sent[0].totals.by_extension,
        [Extension {
            ext: Some("txt".into()),
            files: 1,
            alloc: 10,
        }]
    );
    assert_eq!((summed.totals.alloc, summed.totals.files), (150, 5));
    assert_eq!(summed.mtime, Some(at()));
    assert_eq!(summed.mark.token, "e1.9");
    assert!(summed.links_known && summed.rest.is_none());
    assert!(matches!(summed.coverage.attrs, Proof::Live { .. }));
    // The two heaviest and the one with the most files, the rest summed.
    let mut largest = Vec::new();
    let summed = session
        .totals(&root, Some(Largest { alloc: 2, files: 0 }), &mut |batch| {
            largest.extend(batch.into_iter().map(|c| c.totals.alloc));
            true
        })
        .expect("summed");
    largest.sort_unstable();
    assert_eq!(largest, [40, 50]);
    let (count, rest) = summed.rest.expect("the rest summed");
    assert_eq!((count, rest.alloc, rest.files), (3, 60, 3));
    // A folder it does not know is none.
    assert!(
        session
            .totals(&root.join("unknown"), None, &mut |_| true)
            .is_none()
    );
}
