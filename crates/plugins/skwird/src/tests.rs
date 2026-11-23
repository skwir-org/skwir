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
