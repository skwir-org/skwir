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
    engine_saying(dir, root, silent, Vec::new())
}

/// [`engine`], its hello saying `features`.
fn engine_saying(dir: &Path, root: &Path, silent: bool, features: Vec<String>) -> Fake {
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
        features,
        ..Script::default()
    };
    Fake::start(dir.join("socket"), script).unwrap()
}

fn index(engines: Engines) -> Index {
    Index {
        app: Weak::new(),
        kept: kept::Kept::new(Weak::new(), engines.clone()),
        engines,
        presence: Arc::new(program::Presence::never()),
        secrets: Mutex::default(),
        hello: HELLO,
        release_within: RELEASE_WITHIN,
    }
}

/// What a client asks an engine, heard on its way: a socket of the test's
/// own in front of `fake`, each line read as the request it is and passed
/// on, the fake's answers passed back as they come.
struct Heard {
    socket: PathBuf,
    asked: Arc<Mutex<Vec<protocol::Ask>>>,
}

impl Heard {
    fn before(fake: &Fake, dir: &Path) -> Heard {
        use std::io::{BufRead, Write};
        use std::net::Shutdown;
        use std::os::unix::net::{UnixListener, UnixStream};
        let socket = dir.join("heard");
        let listener = UnixListener::bind(&socket).unwrap();
        let asked: Arc<Mutex<Vec<protocol::Ask>>> = Arc::default();
        let (engine_at, record) = (fake.socket.clone(), Arc::clone(&asked));
        std::thread::spawn(move || {
            for client in listener.incoming() {
                let (Ok(client), Ok(mut engine)) = (client, UnixStream::connect(&engine_at)) else {
                    return;
                };
                let (mut answers, mut back) =
                    (engine.try_clone().unwrap(), client.try_clone().unwrap());
                std::thread::spawn(move || {
                    let _ = std::io::copy(&mut answers, &mut back);
                    let _ = back.shutdown(Shutdown::Both);
                });
                let record = Arc::clone(&record);
                std::thread::spawn(move || {
                    let mut asking = std::io::BufReader::new(client);
                    let mut line = Vec::new();
                    while asking.read_until(b'\n', &mut line).is_ok_and(|n| n > 0) {
                        if let Ok(request) = serde_json::from_slice::<protocol::Request>(&line) {
                            record.lock().push(request.ask);
                        }
                        if engine.write_all(&line).is_err() {
                            break;
                        }
                        line.clear();
                    }
                    let _ = engine.shutdown(Shutdown::Both);
                });
            }
        });
        Heard { socket, asked }
    }

    /// What was asked so far, hellos left out.
    fn asked(&self) -> Vec<protocol::Ask> {
        self.asked
            .lock()
            .iter()
            .filter(|ask| !matches!(ask, protocol::Ask::Hello(_)))
            .cloned()
            .collect()
    }

    /// The `watch` asked, each its folders added and taken back.
    fn watches(&self) -> Vec<(Vec<PathBuf>, Vec<PathBuf>)> {
        let paths = |list: &[protocol::WirePath]| -> Vec<PathBuf> {
            list.iter().map(protocol::WirePath::to_path).collect()
        };
        self.asked()
            .iter()
            .filter_map(|ask| match ask {
                protocol::Ask::Watch { add, remove } => Some((paths(add), paths(remove))),
                _ => None,
            })
            .collect()
    }
}

/// Until `done`, under a deadline for a hang.
fn until(done: impl Fn() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while !done() {
        assert!(std::time::Instant::now() < deadline, "never done");
        std::thread::sleep(Duration::from_millis(5));
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

/// An engine is taken as matching a term in both its forms only where
/// the hello of the connection asked says `names-both-forms`: a session
/// opened or followed says what its own hello said, a word not known is
/// none, and the probe for a term whose forms differ answers nothing from
/// an engine that matches one form.
#[test]
fn an_engine_matches_both_forms_only_where_its_hello_says_so() {
    let words = |words: &[&str]| words.iter().map(|w| (*w).to_owned()).collect::<Vec<_>>();
    for (features, says) in [
        (words(&[]), false),
        (words(&["names-both-forms"]), true),
        (words(&["volumes", "names-both-forms"]), true),
        (words(&["names-both-form", "names"]), false),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let root = PathBuf::from("/fixture/root");
        let fake = engine_saying(tmp.path(), &root, false, features.clone());
        let index = index(Engines::At(fake.socket.clone()));
        let cancel = CancelToken::new();
        let opened = index.open(&root, &cancel).expect("the fake answers");
        assert_eq!(opened.session.both_forms(), says, "opened: {features:?}");
        let followed = index.follow(&root, &cancel).expect("the fake answers");
        assert_eq!(
            followed.session.both_forms(),
            says,
            "followed: {features:?}"
        );
        let typed = |text: &str| Query {
            root: root.clone(),
            names: vec![NameTerm {
                text: text.into(),
                case_sensitive: false,
            }],
            kinds: None,
            size: None,
            modified: None,
            whole: true,
        };
        let two_forms = typed("caf\u{e9}");
        assert!(!two_forms.as_written());
        assert_eq!(
            two_forms.answered_by(&*opened.session),
            says,
            "{features:?}"
        );
        assert!(
            index.covers(&two_forms, &cancel).is_some(),
            "asked as written"
        );
        assert_eq!(
            index.covers_both_forms(&two_forms, &cancel).is_some(),
            says,
            "the probe: {features:?}"
        );
        let ascii = typed("report");
        assert!(
            ascii.answered_by(&*opened.session),
            "every engine: {features:?}"
        );
    }
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

/// [`index`], whether skwird is here looked at as `here` says.
fn index_looking(engines: Engines, here: bool) -> Index {
    Index {
        presence: Arc::new(program::Presence::new(Some(Arc::new(move || here)), None)),
        ..index(engines)
    }
}

/// skwird is said missing, by the index a walked search's footer and the
/// Index page ask, once a search found neither an engine nor skwird; never
/// before, nor where skwird is found or an engine answers. What it says
/// leads to the website's download page, at skwird.
#[test]
fn skwird_is_said_missing_once_a_search_finds_neither_it_nor_an_engine() {
    let tmp = tempfile::tempdir().unwrap();
    let cancel = CancelToken::new();
    let root = PathBuf::from("/fixture/root");
    let missing = index_looking(Engines::At(tmp.path().join("no socket")), false);
    assert!(missing.absent().is_none(), "not looked at yet");
    assert!(missing.open(&root, &cancel).is_none());
    let absent = missing.absent().expect("said missing");
    assert!(
        absent
            .why
            .english()
            .starts_with("skwird is not installed, so searches go through the folders one by one."),
        "{}",
        absent.why.english()
    );
    assert!(
        absent
            .why
            .english()
            .ends_with("from the download page of skwir.org.")
    );
    assert_eq!(absent.label.english(), "Get skwird");
    assert_eq!(absent.url, "https://skwir.org/download#skwird");
    // Installed, no engine running yet: nothing said.
    let installed = index_looking(Engines::At(tmp.path().join("no socket")), true);
    assert!(installed.open(&root, &cancel).is_none());
    assert!(installed.absent().is_none());
    // An engine answering: nothing said, whatever a look found before.
    let fake = engine(tmp.path(), &root, false);
    let answering = index_looking(Engines::At(fake.socket.clone()), false);
    answering.presence.look();
    assert!(answering.absent().is_some());
    assert!(answering.open(&root, &cancel).is_some());
    assert!(answering.absent().is_none(), "an engine answered");
}

/// The app's names indexes say skwird is missing once a window was
/// looked at and it was not found, until the plugin is turned off; a
/// headless test's app, which asks the machine nothing, never says it.
#[test]
fn a_look_at_the_window_says_whether_skwird_is_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let app = App::new(skwir_core::app::AppConfig::ephemeral(
        tmp.path().join("run"),
    ))
    .unwrap();
    let plugin = Arc::new(SkwirdPlugin {
        looks: Some(Arc::new(|| false)),
        ..SkwirdPlugin::at(tmp.path().join("no socket"))
    });
    app.plugins.add(
        Arc::clone(&plugin) as Arc<dyn skwir_core::plugin::Plugin>,
        &app,
    );
    assert!(app.names.absent().is_empty(), "not looked at yet");
    app.plugins.looked_at(&app);
    until(|| !app.names.absent().is_empty());
    assert!(app.plugins.set_enabled(ID, false, &app));
    assert!(app.names.absent().is_empty(), "turned off");

    let headless = App::new(skwir_core::app::AppConfig::ephemeral(
        tmp.path().join("other"),
    ))
    .unwrap();
    assert!(!headless.index_services());
    let plugin = Arc::new(SkwirdPlugin {
        looks: Some(Arc::new(|| false)),
        ..SkwirdPlugin::default()
    });
    headless.plugins.add(
        Arc::clone(&plugin) as Arc<dyn skwir_core::plugin::Plugin>,
        &headless,
    );
    // The look the window's would start, made here.
    let presence = plugin.presence.lock().clone().expect("registered");
    presence.look();
    assert!(headless.names.absent().is_empty());
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

/// An engine that takes the connection and says no hello (stopped, not
/// gone: the system still takes its connections) is given up on once the
/// hello's time is up, not its whole silence, and so is a connect given up
/// on before it answers, whichever asks: a search, a journal's follower.
#[test]
fn an_engine_saying_no_hello_is_not_waited_for() {
    use std::os::unix::net::UnixListener;
    let tmp = tempfile::tempdir().unwrap();
    let socket = tmp.path().join("stalled");
    let listener = UnixListener::bind(&socket).unwrap();
    // Every connection taken and held, never read nor answered.
    std::thread::spawn(move || {
        let held: Vec<_> = listener.incoming().map_while(Result::ok).collect();
        drop(held);
    });
    let mut stalled = index(Engines::At(socket));
    stalled.hello = Duration::from_millis(200);
    let root = Path::new("/fixture/root");
    let cancel = CancelToken::new();
    for ask in ["open", "follow"] {
        let started = std::time::Instant::now();
        let answered = match ask {
            "open" => stalled.open(root, &cancel).is_some(),
            _ => stalled.follow(root, &cancel).is_some(),
        };
        assert!(!answered, "{ask}: no engine");
        let took = started.elapsed();
        assert!(
            took >= stalled.hello && took < Duration::from_secs(10),
            "{ask}: the hello's time, not the silence: {took:?}"
        );
    }

    // A connect given up on before its hello's time ends at once.
    let stalled = Arc::new({
        stalled.hello = Duration::from_secs(30);
        stalled
    });
    let cancel = CancelToken::new();
    let started = std::time::Instant::now();
    let asking = std::thread::spawn({
        let (stalled, cancel) = (Arc::clone(&stalled), cancel.clone());
        move || stalled.open(Path::new("/fixture/root"), &cancel).is_some()
    });
    std::thread::sleep(Duration::from_millis(200));
    cancel.cancel();
    assert!(!asking.join().unwrap(), "given up on");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "stopped in its connect: {:?}",
        started.elapsed()
    );
}

/// Where skwird's live sizes and times are not proven (Windows, as yet),
/// its coverage says them checked, as its journal says them incomplete: a
/// dated search, Usage and a tag set's follower stat for themselves.
#[test]
fn live_times_are_said_checked_where_they_are_not_proven() {
    let live = protocol::Coverage {
        names: protocol::Proof::Live(instant()),
        attrs: protocol::Proof::Live(instant()),
        holes: Vec::new(),
    };
    assert_eq!(
        coverage_in(live.clone(), true).attrs,
        Proof::Live { since: at() }
    );
    let unproven = coverage_in(live.clone(), false);
    assert_eq!(unproven.attrs, Proof::Checked { at: at() });
    assert_eq!(
        unproven.names,
        Proof::Live { since: at() },
        "names stay live"
    );
    assert_eq!(
        matches!(coverage_of(live).attrs, Proof::Live { .. }),
        !cfg!(windows),
        "as the journal says them"
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
    let heard = Heard::before(&fake, tmp.path());
    let mut client = Client::at(&heard.socket, CLIENT).unwrap();
    let folders = vec![
        PathBuf::from("/fixture/keys"),
        PathBuf::from("/fixture/vault"),
    ];
    assert!(keep_out(&mut client, &folders));
    assert_eq!(
        heard.asked(),
        [protocol::Ask::Exclude {
            add: folders.iter().map(|p| protocol::WirePath::of(p)).collect(),
            remove: Vec::new(),
        }],
        "the folders asked kept out, nothing taken back"
    );
}

/// Before a volume is unmounted, an engine already running whose hello
/// says it keeps volumes is asked to let go of what it holds there, by the
/// volume's mount point, and answers once it holds nothing; one that says
/// no such word is asked nothing; with no engine, nothing is started and
/// nothing waits.
#[test]
fn an_engine_that_keeps_volumes_lets_go_of_one_before_its_unmount() {
    let point = PathBuf::from("/media/someone/it's USB, \u{e9}t\u{e9}");
    for (features, asked) in [(vec!["volumes".to_owned()], true), (Vec::new(), false)] {
        let tmp = tempfile::tempdir().unwrap();
        let script = Script {
            roots: vec![PathBuf::from("/fixture/root")],
            features: features.clone(),
            volumes: Some(protocol::Volumes {
                kept: Vec::new(),
                seen: Vec::new(),
            }),
            ..Script::default()
        };
        let fake = Fake::start(tmp.path().join("socket"), script).unwrap();
        let heard = Heard::before(&fake, tmp.path());
        let index = index(Engines::At(heard.socket.clone()));
        index.release(&point, &CancelToken::new());
        let released: Vec<protocol::Ask> = heard
            .asked()
            .into_iter()
            .filter(|ask| matches!(ask, protocol::Ask::Volumes { .. }))
            .collect();
        if asked {
            assert_eq!(
                released,
                [protocol::Ask::Volumes {
                    keep: Vec::new(),
                    forget: Vec::new(),
                    release: Vec::new(),
                    release_at: vec![protocol::WirePath::of(&point)],
                }],
                "released by its mount point, nothing kept or forgotten"
            );
        } else {
            assert!(released.is_empty(), "{features:?}: asked nothing");
        }
    }
    let tmp = tempfile::tempdir().unwrap();
    let index = index(Engines::At(tmp.path().join("no engine")));
    let asked_at = std::time::Instant::now();
    index.release(&point, &CancelToken::new());
    assert!(
        asked_at.elapsed() < Duration::from_secs(2),
        "no engine waited for"
    );
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
    let heard = Heard::before(&fake, tmp.path());
    let indexes = skwir_core::names::NameIndexes::default();
    indexes.register(ID, Arc::new(index(Engines::At(heard.socket.clone()))));
    assert!(
        indexes.open(&root, &CancelToken::new()).is_empty(),
        "not covered: the search walks"
    );
    let asked = heard.asked();
    assert!(
        asked
            .iter()
            .any(|ask| matches!(ask, protocol::Ask::Coverage { root: r } if r.to_path() == root)),
        "its coverage asked: {asked:?}"
    );
    assert!(
        !asked
            .iter()
            .any(|ask| matches!(ask, protocol::Ask::Sync { .. } | protocol::Ask::Query(_))),
        "never synced nor queried: {asked:?}"
    );
}

/// An app whose Kept live asks the engine `heard` stands before, the
/// user's folders offering Documents.
fn kept_app(dir: &Path, heard: &Heard) -> (Arc<App>, Arc<kept::Kept>) {
    let app = App::new(skwir_core::app::AppConfig::ephemeral(dir.join("app"))).unwrap();
    let kept = kept::Kept::reading(
        app.weak(),
        Engines::At(heard.socket.clone()),
        Box::new(|| kept::Offered {
            own: vec![("Documents".into(), PathBuf::from("/fixture/Documents"))],
            bookmarks: Vec::new(),
        }),
    );
    kept::register(&kept, &app);
    (app, kept)
}

/// Kept live sends a user's engine its list as differences (ADR-150, 6):
/// asked what it keeps, then what it lacks added, in the share's order
/// (what the user added, then their folders); once it keeps them, a
/// folder unticked is taken back alone. What it says is kept, and the
/// menu's entry shows from then on.
#[test]
fn what_is_kept_live_is_sent_as_differences() {
    let tmp = tempfile::tempdir().unwrap();
    let root = PathBuf::from("/fixture");
    let fake = engine(tmp.path(), &root, false);
    let heard = Heard::before(&fake, tmp.path());
    let (app, kept) = kept_app(tmp.path(), &heard);
    let (projects, documents) = (
        PathBuf::from("/data/projects"),
        PathBuf::from("/fixture/Documents"),
    );
    app.dispatch(&skwir_core::command::Invocation::with_args(
        "skwird.kept_add",
        &serde_json::json!({ "path": projects }),
    ))
    .unwrap();
    until(|| !kept.refreshing.lock().running);
    assert_eq!(
        heard.watches(),
        [
            (Vec::new(), Vec::new()),
            (vec![projects.clone(), documents.clone()], Vec::new())
        ]
    );
    assert!(kept.user_engine(), "a user's engine found");
    assert!(matches!(
        kept.said.lock().folders.get(&documents).map(|k| k.state),
        Some(protocol::Keeping::Live(_))
    ));

    // An engine that keeps both: unticking Documents takes it back alone.
    let live = |path: &Path| protocol::Kept {
        path: protocol::WirePath::of(path),
        state: protocol::Keeping::Live(instant()),
        folders: Some(1),
        within: None,
    };
    let keeping = Fake::start(
        tmp.path().join("keeping"),
        Script {
            watching: Some(protocol::Watching {
                folders: vec![live(&projects), live(&documents)],
                share: Some(protocol::Share { used: 2, size: 100 }),
            }),
            ..Script::default()
        },
    )
    .unwrap();
    let second = tmp.path().join("second");
    std::fs::create_dir(&second).unwrap();
    let heard = Heard::before(&keeping, &second);
    let (app, kept) = kept_app(&second, &heard);
    app.dispatch(&skwir_core::command::Invocation::with_args(
        "skwird.kept_add",
        &serde_json::json!({ "path": projects }),
    ))
    .unwrap();
    until(|| !kept.refreshing.lock().running);
    assert_eq!(
        heard.watches(),
        [(Vec::new(), Vec::new())],
        "nothing sent where the order holds"
    );
    let id = kept::id_of(&documents);
    app.dispatch(&skwir_core::command::Invocation::with_args(
        "skwird.kept_tick",
        &serde_json::json!({ "id": id, "on": false }),
    ))
    .unwrap();
    until(|| !kept.refreshing.lock().running);
    assert_eq!(
        heard.watches()[1..],
        [(Vec::new(), Vec::new()), (Vec::new(), vec![documents])]
    );
}

/// Once an engine this run was a user's, the menu's entry stays, whatever
/// a later ask finds: none running (ADR-150, 10).
#[test]
fn the_menu_s_entry_stays_once_a_user_s_engine_was_found() {
    let tmp = tempfile::tempdir().unwrap();
    let fake = engine(tmp.path(), Path::new("/fixture"), false);
    let heard = Heard::before(&fake, tmp.path());
    let (app, kept) = kept_app(tmp.path(), &heard);
    kept.refresh();
    until(|| !kept.refreshing.lock().running);
    assert!(kept.user_engine());
    drop(fake);
    kept.refresh();
    until(|| !kept.refreshing.lock().running);
    assert_eq!(kept.said.lock().scope, None, "none answers now");
    let menu = app
        .dispatch(&skwir_core::command::Invocation::with_args(
            "skwird.keep_live_menu",
            &serde_json::json!({ "path": "/fixture/Documents" }),
        ))
        .unwrap();
    let skwir_core::command::Outcome::Value(menu) = menu else {
        panic!("an answer");
    };
    assert_eq!(menu["items"].as_array().map(Vec::len), Some(1), "{menu}");
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

/// A removable drive skwird sees mounted at `at`.
fn seen(id: Option<&str>, at: &Path, label: Option<&str>) -> protocol::SeenVolume {
    protocol::SeenVolume {
        id: id.map(str::to_owned),
        at: protocol::WirePath::of(at),
        filesystem: "vfat".into(),
        label: label.map(str::to_owned),
        kept: false,
    }
}

/// A drive this client keeps, standing as `standing`.
fn kept_drive(id: &str, standing: protocol::Standing, at: Option<&Path>) -> protocol::KeptVolume {
    protocol::KeptVolume {
        id: id.to_owned(),
        standing,
        at: at.map(protocol::WirePath::of),
        coverage: None,
    }
}

/// What a drives engine of the test's own sees and keeps, the test's to
/// change as it serves.
#[derive(Default)]
struct Drive {
    seen: Vec<protocol::SeenVolume>,
    kept: Vec<protocol::KeptVolume>,
    /// Keep and forget refused, saying this.
    refuse: Option<String>,
    /// Keep and forget answered as if nothing was asked.
    deaf: bool,
    /// What it sees once it answered one `volumes` more: the other volume
    /// of a disk, described a moment after the first.
    then_seen: Option<Vec<protocol::SeenVolume>>,
}

/// An engine of the test's own on `dir`'s socket for drives: its hello says
/// `features`; every `volumes` is answered with what it sees and keeps
/// (refused where it is given none, as by an engine that says no
/// `volumes`), keep and forget changing what it keeps; every request noted.
/// Gone once dropped: its socket taken away, as an engine that ended.
struct DrivesEngine {
    socket: PathBuf,
    drive: Arc<Mutex<Option<Drive>>>,
    asked: Arc<Mutex<Vec<protocol::Ask>>>,
    gone: Arc<std::sync::atomic::AtomicBool>,
}

impl DrivesEngine {
    fn start(dir: &Path, features: &[&str], volumes: Option<protocol::Volumes>) -> DrivesEngine {
        use std::os::unix::net::UnixListener;
        let socket = dir.join("socket");
        let _ = std::fs::remove_file(&socket);
        let listener = UnixListener::bind(&socket).unwrap();
        let drive = Arc::new(Mutex::new(volumes.map(|v| Drive {
            seen: v.seen,
            kept: v.kept,
            ..Drive::default()
        })));
        let asked: Arc<Mutex<Vec<protocol::Ask>>> = Arc::default();
        let gone = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let features: Vec<String> = features.iter().map(|w| (*w).to_owned()).collect();
        let (serving, noting, ended) = (Arc::clone(&drive), Arc::clone(&asked), Arc::clone(&gone));
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                if ended.load(std::sync::atomic::Ordering::Acquire) {
                    return;
                }
                let Ok(stream) = stream else {
                    continue;
                };
                let (drive, asked, features) =
                    (Arc::clone(&serving), Arc::clone(&noting), features.clone());
                std::thread::spawn(move || serve_drives(stream, &features, &drive, &asked));
            }
        });
        DrivesEngine {
            socket,
            drive,
            asked,
            gone,
        }
    }

    /// What was asked so far, hellos left out.
    fn asked(&self) -> Vec<protocol::Ask> {
        self.asked
            .lock()
            .iter()
            .filter(|ask| !matches!(ask, protocol::Ask::Hello(_)))
            .cloned()
            .collect()
    }

    /// What it sees from now on.
    fn see(&self, seen: Vec<protocol::SeenVolume>) {
        if let Some(drive) = self.drive.lock().as_mut() {
            drive.seen = seen;
        }
    }

    /// Change how it answers.
    fn with(&self, change: impl FnOnce(&mut Drive)) {
        if let Some(drive) = self.drive.lock().as_mut() {
            change(drive);
        }
    }
}

impl Drop for DrivesEngine {
    fn drop(&mut self) {
        self.gone.store(true, std::sync::atomic::Ordering::Release);
        // Its wait for a connection ended, then no way to it.
        let _ = std::os::unix::net::UnixStream::connect(&self.socket);
        let _ = std::fs::remove_file(&self.socket);
    }
}

fn serve_drives(
    stream: std::os::unix::net::UnixStream,
    features: &[String],
    drive: &Mutex<Option<Drive>>,
    asked: &Mutex<Vec<protocol::Ask>>,
) {
    let Ok(mut out) = stream.try_clone() else {
        return;
    };
    let mut reader = std::io::BufReader::new(stream);
    while let Ok(Some(request)) = protocol::read::<_, protocol::Request>(&mut reader) {
        asked.lock().push(request.ask.clone());
        let failure = |what: &str, message: &str| {
            protocol::Answer::Error(protocol::Failure {
                what: what.to_owned(),
                message: message.to_owned(),
            })
        };
        let answer = match request.ask {
            protocol::Ask::Hello(_) => protocol::Answer::Hello(protocol::Welcome {
                version: protocol::VERSION,
                scope: protocol::Scope::User,
                engine: "a test's engine of drives".into(),
                roots: Vec::new(),
                features: features.to_vec(),
            }),
            protocol::Ask::Volumes { keep, forget, .. } => match drive.lock().as_mut() {
                None => failure("not_here", "this engine keeps no removable volume"),
                Some(drive) => match &drive.refuse {
                    Some(why) if !(keep.is_empty() && forget.is_empty()) => failure("busy", why),
                    _ => {
                        if !drive.deaf {
                            let forget: HashSet<String> = forget.into_iter().collect();
                            drive.kept.retain(|k| !forget.contains(&k.id));
                            for id in keep {
                                if drive.kept.iter().any(|k| k.id == id) {
                                    continue;
                                }
                                let at = drive
                                    .seen
                                    .iter()
                                    .find(|s| s.id.as_deref() == Some(id.as_str()))
                                    .map(|s| s.at.clone());
                                drive.kept.push(protocol::KeptVolume {
                                    id,
                                    standing: protocol::Standing::Indexed,
                                    at,
                                    coverage: None,
                                });
                            }
                        }
                        let answer = protocol::Answer::Volumes(protocol::Volumes {
                            kept: drive.kept.clone(),
                            seen: drive.seen.clone(),
                        });
                        if let Some(then) = drive.then_seen.take() {
                            drive.seen = then;
                        }
                        answer
                    }
                },
            },
            _ => failure("bad_request", "not asked of a test's engine of drives"),
        };
        let response = protocol::Response {
            id: request.id,
            answer,
        };
        if protocol::write(&mut out, &response).is_err() {
            return;
        }
    }
}

/// An engine whose hello says `features`, answering every `volumes` with
/// `volumes` (refusing it where none is given), keep and forget changing
/// what it keeps.
fn drives_engine(
    dir: &Path,
    features: &[&str],
    volumes: Option<protocol::Volumes>,
) -> DrivesEngine {
    DrivesEngine::start(dir, features, volumes)
}

/// The `volumes` asked that keep, forget or release something.
fn volumes_asked(engine: &DrivesEngine) -> Vec<protocol::Ask> {
    engine
        .asked()
        .into_iter()
        .filter(|ask| {
            matches!(ask, protocol::Ask::Volumes { keep, forget, release, release_at }
                if !(keep.is_empty() && forget.is_empty() && release.is_empty() && release_at.is_empty()))
        })
        .collect()
}

/// How many `volumes` were asked.
fn volumes_count(engine: &DrivesEngine) -> usize {
    engine
        .asked()
        .iter()
        .filter(|ask| matches!(ask, protocol::Ask::Volumes { .. }))
        .count()
}

/// skwird's plugin turned on in an app of its own, asking the engine at
/// `socket`, its settings and state files in `dir`, which another run (an
/// app made later, `run` another name) reads again; its window the one a
/// look looks at, of the user's own.
struct Drives {
    app: Arc<App>,
    plugin: Arc<SkwirdPlugin>,
    window: skwir_core::workspace::WindowId,
}

impl Drives {
    /// Its table of mounts one of its own, empty and never changed: a test
    /// never meets the machine's mounts (another test's FUSE folder).
    fn at(dir: &Path, run: &str, socket: &Path) -> Drives {
        Drives::with_table(dir, run, socket, &Arc::default())
    }

    /// [`at`](Self::at), the mount points read from `table`, which the test
    /// changes, telling each change ([`plugged`](Self::plugged)).
    fn with_table(
        dir: &Path,
        run: &str,
        socket: &Path,
        table: &Arc<Mutex<HashSet<PathBuf>>>,
    ) -> Drives {
        let table = Arc::clone(table);
        Drives::mounted(dir, run, socket, Arc::new(move || table.lock().clone()))
    }

    fn mounted(dir: &Path, run: &str, socket: &Path, mounted: volumes::Mounted) -> Drives {
        let config = skwir_core::app::AppConfig {
            settings_path: Some(dir.join("settings.toml")),
            state_path: Some(dir.join("state.toml")),
            ..skwir_core::app::AppConfig::ephemeral(dir.join(run))
        };
        let app = App::new(config).unwrap();
        let plugin = Arc::new(SkwirdPlugin {
            mounted: Some(mounted),
            ..SkwirdPlugin::at(socket.to_path_buf())
        });
        app.plugins.add(
            Arc::clone(&plugin) as Arc<dyn skwir_core::plugin::Plugin>,
            &app,
        );
        let window = app.workspace.read().active_window_id();
        Drives {
            app,
            plugin,
            window,
        }
    }

    fn volumes(&self) -> Arc<volumes::Volumes> {
        self.plugin
            .volumes
            .lock()
            .clone()
            .expect("the plugin is on")
    }

    /// The window looked at, as it tells it, until what the look started
    /// ended. It stays looked at.
    fn look(&self) {
        let volumes = self.volumes();
        let before = volumes.looks.load(std::sync::atomic::Ordering::Acquire);
        self.app.set_window_focused(self.window, true);
        until(|| {
            volumes.looks.load(std::sync::atomic::Ordering::Acquire) > before && volumes.idle()
        });
    }

    /// A mount made, as the table of mounts tells it, until what it
    /// started ended.
    fn plugged(&self) {
        let volumes = self.volumes();
        volumes.mounts_changed();
        until(|| volumes.idle());
    }

    /// The questions asked since the last call.
    fn questions(&self) -> Vec<skwir_core::app::Notification> {
        self.app
            .take_notifications()
            .into_iter()
            .filter(|said| said.stays)
            .collect()
    }

    /// A question's button pressed, as the window runs it (here, off its
    /// thread: at once).
    fn press(&self, question: &skwir_core::app::Notification, button: usize) {
        self.app
            .dispatch(&question.actions[button].command)
            .unwrap();
    }

    fn value(&self, command: &str, args: &serde_json::Value) -> serde_json::Value {
        match self
            .app
            .dispatch(&skwir_core::command::Invocation::with_args(command, args))
            .unwrap()
        {
            skwir_core::command::Outcome::Value(value) => value,
            other => panic!("{command}: {other:?}"),
        }
    }
}

const STICK: &str = "0a1b-2c3d";

fn stick_point() -> PathBuf {
    PathBuf::from("/media/someone/it's a stick, \u{e9}t\u{e9}")
}

/// A drive plugged in that has an identity, from an engine that says
/// `volumes`, not kept and never asked about, is asked about once, at the
/// first look, in a message that stays; and never again, after Yes, No or
/// the message closed unanswered, at a mount, a look, or in another run.
/// Yes alone has skwird keep it, by its identity.
#[test]
fn a_drive_plugged_in_is_asked_about_once_whatever_the_answer() {
    for answer in [Some(true), Some(false), None] {
        let tmp = tempfile::tempdir().unwrap();
        let heard = drives_engine(
            tmp.path(),
            &["volumes"],
            Some(protocol::Volumes {
                kept: Vec::new(),
                seen: vec![seen(Some(STICK), &stick_point(), Some("STICK"))],
            }),
        );
        let drives = Drives::at(tmp.path(), "first", &heard.socket);
        drives.look();
        let asked = drives.questions();
        assert_eq!(asked.len(), 1, "{answer:?}: asked once");
        assert_eq!(
            asked[0].text.english(),
            "Index the names on \u{201c}STICK\u{201d} so searches find them at once? The index stays on this computer, never on the drive."
        );
        let buttons: Vec<String> = asked[0].actions.iter().map(|a| a.label.english()).collect();
        assert_eq!(buttons, ["Yes", "No"]);
        if let Some(keep) = answer {
            drives.press(&asked[0], usize::from(!keep));
        }
        let keep = protocol::Ask::Volumes {
            keep: vec![STICK.to_owned()],
            forget: Vec::new(),
            release: Vec::new(),
            release_at: Vec::new(),
        };
        if answer == Some(true) {
            assert_eq!(volumes_asked(&heard), [keep], "kept by its identity");
        } else {
            assert!(volumes_asked(&heard).is_empty(), "{answer:?}: nothing kept");
        }
        drives.plugged();
        drives.look();
        assert!(drives.questions().is_empty(), "{answer:?}: never again");
        // Another run, on the same state.
        let again = Drives::at(tmp.path(), "second", &heard.socket);
        again.look();
        again.plugged();
        assert!(
            again.questions().is_empty(),
            "{answer:?}: never again in another run"
        );
    }
}

/// A drive plugged in while Skwir runs is asked about at once; one waiting
/// behind another is asked once that one is answered; a question about a
/// drive no longer mounted is taken back.
#[test]
fn drives_are_asked_about_one_at_a_time() {
    let tmp = tempfile::tempdir().unwrap();
    let (other, other_at) = ("5e6f-7a8b", PathBuf::from("/media/someone/CARD"));
    let both = protocol::Volumes {
        kept: Vec::new(),
        // No label: called by its mount point's name.
        seen: vec![
            seen(Some(STICK), &stick_point(), Some("STICK")),
            seen(Some(other), &other_at, None),
        ],
    };
    let heard = drives_engine(tmp.path(), &["volumes"], Some(both));
    let drives = Drives::at(tmp.path(), "run", &heard.socket);
    drives.look();
    let first = drives.questions();
    assert_eq!(first.len(), 1, "one at a time");
    assert!(first[0].text.english().contains("STICK"));
    drives.press(&first[0], 1);
    let second = drives.questions();
    assert_eq!(second.len(), 1, "the next once answered");
    assert!(
        second[0].text.english().contains("\u{201c}CARD\u{201d}"),
        "{}",
        second[0].text.english()
    );
    assert!(
        drives
            .app
            .question_stands(ID, second[0].question.expect("a question")),
        "it stands"
    );
    // Its drive pulled out: taken back.
    heard.see(vec![seen(Some(STICK), &stick_point(), Some("STICK"))]);
    drives.plugged();
    assert!(
        !drives
            .app
            .question_stands(ID, second[0].question.expect("a question")),
        "taken back"
    );
}

/// No question where it cannot be answered so: a drive with no identity,
/// an engine whose hello says no `volumes` (asked nothing of drives), no
/// engine at all.
#[test]
fn no_identity_no_word_or_no_engine_asks_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let anonymous = protocol::Volumes {
        kept: Vec::new(),
        seen: vec![seen(None, &stick_point(), Some("STICK"))],
    };
    let heard = drives_engine(tmp.path(), &["volumes"], Some(anonymous));
    let drives = Drives::at(tmp.path(), "run", &heard.socket);
    drives.look();
    drives.plugged();
    assert!(drives.questions().is_empty(), "no identity");
    let menu = drives.value(
        "skwird.volume_menu",
        &serde_json::json!({ "location": stick_point() }),
    );
    assert_eq!(
        menu["items"],
        serde_json::json!([]),
        "nor offered in its menu"
    );

    let tmp = tempfile::tempdir().unwrap();
    let wordless = protocol::Volumes {
        kept: Vec::new(),
        seen: vec![seen(Some(STICK), &stick_point(), Some("STICK"))],
    };
    let heard = drives_engine(tmp.path(), &[], Some(wordless));
    let drives = Drives::at(tmp.path(), "run", &heard.socket);
    drives.look();
    drives.plugged();
    assert!(drives.questions().is_empty(), "no word");
    assert!(
        !heard
            .asked()
            .iter()
            .any(|ask| matches!(ask, protocol::Ask::Volumes { .. })),
        "asked nothing of drives"
    );

    let tmp = tempfile::tempdir().unwrap();
    let drives = Drives::at(tmp.path(), "run", &tmp.path().join("no engine"));
    let started = std::time::Instant::now();
    drives.look();
    drives.plugged();
    assert!(drives.questions().is_empty(), "no engine");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "nothing waited for"
    );
}

/// A drive's place offers Index Its Names where skwird last saw it mounted
/// with an identity, checked while it is kept: Yes keeps it, checked forgets
/// it. Elsewhere, nothing.
#[test]
fn a_drive_s_menu_indexes_its_names_or_forgets_them() {
    let tmp = tempfile::tempdir().unwrap();
    let card = PathBuf::from("/media/someone/CARD");
    let heard = drives_engine(
        tmp.path(),
        &["volumes"],
        Some(protocol::Volumes {
            kept: vec![kept_drive(
                "5e6f-7a8b",
                protocol::Standing::Indexed,
                Some(&card),
            )],
            seen: vec![
                seen(Some(STICK), &stick_point(), Some("STICK")),
                seen(Some("5e6f-7a8b"), &card, Some("CARD")),
            ],
        }),
    );
    let drives = Drives::at(tmp.path(), "run", &heard.socket);
    drives.look();
    let item = |location: &serde_json::Value| -> Vec<(String, String, serde_json::Value, serde_json::Value)> {
        let menu = drives.value(
            "skwird.volume_menu",
            &serde_json::json!({ "location": location }),
        );
        menu["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| {
                let title: Words = serde_json::from_value(item["title"].clone()).unwrap();
                (
                    title.english(),
                    item["command"]["name"].as_str().unwrap().to_owned(),
                    item["command"]["args"].clone(),
                    item["checked"].clone(),
                )
            })
            .collect()
    };
    assert_eq!(
        item(&serde_json::json!(stick_point())),
        [(
            "Index Its Names".to_owned(),
            "skwird.keep_volume".to_owned(),
            serde_json::json!({ "id": STICK }),
            serde_json::json!("off"),
        )]
    );
    assert_eq!(
        item(&serde_json::json!(card)),
        [(
            "Index Its Names".to_owned(),
            "skwird.forget_volume".to_owned(),
            serde_json::json!({ "id": "5e6f-7a8b" }),
            serde_json::json!("on"),
        )]
    );
    assert!(
        item(&serde_json::json!("/media/someone")).is_empty(),
        "no drive there"
    );
    assert!(
        item(&serde_json::json!("device:/dev/sdz1")).is_empty(),
        "not mounted"
    );
    assert!(item(&serde_json::json!("/fixture")).is_empty(), "a folder");
}

/// The Index page lists the drives kept, each by its label (as mounted
/// now, else as remembered when it was kept, which a drive away has not
/// in skwird's answer) and where it stands; Forget has skwird forget it,
/// by its identity, and Skwir its label. With no engine, what is
/// remembered.
#[test]
fn the_index_page_lists_the_drives_kept_and_forgets_one() {
    let tmp = tempfile::tempdir().unwrap();
    let (photos, photos_at) = ("9c8d-7e6f", PathBuf::from("/media/someone/PHOTOS"));
    let card = PathBuf::from("/media/someone/COPY");
    // Kept in a first run, mounted then.
    {
        let first = tmp.path().join("first");
        std::fs::create_dir(&first).unwrap();
        let heard = drives_engine(
            &first,
            &["volumes"],
            Some(protocol::Volumes {
                kept: vec![kept_drive(
                    photos,
                    protocol::Standing::Indexed,
                    Some(&photos_at),
                )],
                seen: vec![seen(Some(photos), &photos_at, Some("Holiday photos"))],
            }),
        );
        let drives = Drives::at(tmp.path(), "first", &heard.socket);
        drives.look();
    }
    let checked = protocol::Coverage {
        names: protocol::Proof::Checked(instant()),
        attrs: protocol::Proof::Checked(instant()),
        holes: Vec::new(),
    };
    let heard = drives_engine(
        tmp.path(),
        &["volumes"],
        Some(protocol::Volumes {
            kept: vec![
                protocol::KeptVolume {
                    coverage: Some(checked),
                    ..kept_drive(STICK, protocol::Standing::Indexed, Some(&stick_point()))
                },
                kept_drive(photos, protocol::Standing::Away, None),
                kept_drive("5e6f-7a8b", protocol::Standing::Twin, Some(&card)),
            ],
            seen: vec![
                seen(Some(STICK), &stick_point(), Some("STICK")),
                seen(Some("5e6f-7a8b"), &card, None),
            ],
        }),
    );
    let drives = Drives::at(tmp.path(), "second", &heard.socket);
    drives.look();
    let rows = |drives: &Drives| -> Vec<(String, String, String)> {
        let listed = drives.value("skwird.volumes_list", &serde_json::json!({}));
        listed["records"]
            .as_array()
            .unwrap()
            .iter()
            .map(|record| {
                let said = |field: &str| -> String {
                    serde_json::from_value::<Words>(record[field].clone())
                        .unwrap()
                        .english()
                };
                (
                    record["id"].as_str().unwrap().to_owned(),
                    said("name"),
                    said("state"),
                )
            })
            .collect()
    };
    let listed = rows(&drives);
    assert_eq!(listed.len(), 3, "{listed:?}");
    assert_eq!(listed[0].0, STICK);
    assert_eq!(listed[0].1, "STICK");
    assert!(
        listed[0].2.starts_with("Indexed, checked "),
        "{}",
        listed[0].2
    );
    assert_eq!(
        listed[1],
        (
            photos.to_owned(),
            "Holiday photos".to_owned(),
            "Not plugged in".to_owned()
        ),
        "remembered when it was kept"
    );
    assert_eq!(
        listed[2],
        (
            "5e6f-7a8b".to_owned(),
            "COPY".to_owned(),
            "Refused: another drive of the same identity is indexed".to_owned()
        ),
        "no label: its mount point's name"
    );
    let record = &drives.value("skwird.volumes_list", &serde_json::json!({}))["records"][1];
    let forget = &record["actions"][0];
    assert_eq!(
        serde_json::from_value::<Words>(forget["label"].clone())
            .unwrap()
            .english(),
        "Forget"
    );
    drives
        .app
        .dispatch(&skwir_core::command::Invocation::with_args(
            forget["command"].as_str().unwrap(),
            &forget["args"],
        ))
        .unwrap();
    assert_eq!(
        volumes_asked(&heard),
        [protocol::Ask::Volumes {
            keep: Vec::new(),
            forget: vec![photos.to_owned()],
            release: Vec::new(),
            release_at: Vec::new(),
        }],
        "forgotten by its identity"
    );
    let on_disk: volumes::Remembered = drives
        .app
        .settings
        .kept_on_disk(volumes::STATE)
        .and_then(|value| value.try_into().ok())
        .expect("kept");
    assert!(!on_disk.labels.contains_key(photos), "{on_disk:?}");
    assert_eq!(on_disk.labels.get(STICK).map(String::as_str), Some("STICK"));

    // No engine: what is remembered, said to stand once skwird runs.
    let nobody = Drives::at(tmp.path(), "third", &tmp.path().join("no engine"));
    nobody.look();
    let _ = rows(&nobody);
    until(|| nobody.volumes().idle());
    let listed = rows(&nobody);
    let names: Vec<&str> = listed.iter().map(|(_, name, _)| name.as_str()).collect();
    assert_eq!(names, ["STICK", "COPY"], "by identity: {listed:?}");
    assert!(
        listed
            .iter()
            .all(|(_, _, state)| state == "Indexed when skwird runs"),
        "{listed:?}"
    );
}

/// Turned off, the plugin follows no mount and asks nothing more: what
/// told the run before of a mount does nothing; turned on again, its run
/// is new, and asks as the first did.
#[test]
fn turned_off_it_asks_nothing_and_on_again_it_asks_anew() {
    let tmp = tempfile::tempdir().unwrap();
    let heard = drives_engine(
        tmp.path(),
        &["volumes"],
        Some(protocol::Volumes {
            kept: Vec::new(),
            seen: vec![seen(Some(STICK), &stick_point(), Some("STICK"))],
        }),
    );
    let drives = Drives::at(tmp.path(), "run", &heard.socket);
    let before = drives.volumes();
    assert!(drives.app.plugins.set_enabled(ID, false, &drives.app));
    assert!(drives.plugin.volumes.lock().is_none(), "let go of");
    let asked = heard.asked().len();
    drives.app.set_window_focused(drives.window, true);
    before.mounts_changed();
    until(|| before.idle());
    assert!(drives.questions().is_empty(), "nothing asked");
    assert_eq!(heard.asked().len(), asked, "nothing asked of skwird");
    assert!(drives.app.plugins.set_enabled(ID, true, &drives.app));
    drives.look();
    assert_eq!(drives.questions().len(), 1, "asked by the new run");
}

/// A command of skwird's run as an agent's or a script's call is (off the
/// window's thread: at once), what it came to.
fn run(drives: &Drives, command: &str, id: &str) -> anyhow::Result<()> {
    drives
        .app
        .dispatch(&skwir_core::command::Invocation::with_args(
            command,
            &serde_json::json!({ "id": id }),
        ))
        .map(drop)
}

/// A drive kept or forgotten outside its question (its menu, the Index
/// page) is decided as its question's answer is: never asked about, its
/// question, standing, taken back at once.
#[test]
fn a_drive_kept_from_its_menu_is_never_asked_about() {
    let tmp = tempfile::tempdir().unwrap();
    let (card, card_at) = ("5e6f-7a8b", PathBuf::from("/media/someone/CARD"));
    let engine = drives_engine(
        tmp.path(),
        &["volumes"],
        Some(protocol::Volumes {
            kept: Vec::new(),
            seen: vec![
                seen(Some(STICK), &stick_point(), Some("STICK")),
                seen(Some(card), &card_at, Some("CARD")),
            ],
        }),
    );
    let drives = Drives::at(tmp.path(), "run", &engine.socket);
    drives.look();
    let first = drives.questions();
    assert_eq!(first.len(), 1);
    assert!(first[0].text.english().contains("STICK"));
    // The card, waiting, kept from its menu; then the stick answered.
    run(&drives, "skwird.keep_volume", card).unwrap();
    drives.press(&first[0], 1);
    until(|| drives.volumes().idle());
    assert!(
        drives.questions().is_empty(),
        "the card kept is not asked about"
    );

    // A question standing, its drive kept from its menu: taken back.
    let tmp = tempfile::tempdir().unwrap();
    let engine = drives_engine(
        tmp.path(),
        &["volumes"],
        Some(protocol::Volumes {
            kept: Vec::new(),
            seen: vec![seen(Some(STICK), &stick_point(), Some("STICK"))],
        }),
    );
    let drives = Drives::at(tmp.path(), "run", &engine.socket);
    drives.look();
    let asked = drives.questions();
    let question = asked[0].question.expect("a question");
    assert!(drives.app.question_stands(ID, question));
    run(&drives, "skwird.keep_volume", STICK).unwrap();
    assert!(!drives.app.question_stands(ID, question), "taken back");
    run(&drives, "skwird.forget_volume", STICK).unwrap();
    drives.plugged();
    drives.look();
    assert!(
        drives.questions().is_empty(),
        "forgotten, never asked again"
    );
}

/// Only a drive skwird sees at a mount point made since the check before
/// is plugged in: one it came to see otherwise (an engine started since,
/// another mount made) is not asked about, nor waited for; one decided
/// (kept, then forgotten) is not asked about when plugged in again; one
/// skwird does not see yet is asked of it once more a moment later, where
/// it may be a drive.
#[test]
fn only_a_drive_plugged_in_is_asked_about() {
    let tmp = tempfile::tempdir().unwrap();
    let table = Arc::new(Mutex::new(HashSet::from([stick_point()])));
    let socket = tmp.path().join("socket");
    let drives = Drives::with_table(tmp.path(), "run", &socket, &table);
    // No engine at the first look: nothing found.
    drives.look();
    let engine = drives_engine(
        tmp.path(),
        &["volumes"],
        Some(protocol::Volumes {
            kept: Vec::new(),
            seen: vec![seen(Some(STICK), &stick_point(), Some("STICK"))],
        }),
    );
    // A search started one; then a container's layers are mounted.
    table.lock().insert(tmp.path().join("layers"));
    let before = volumes_count(&engine);
    drives.plugged();
    assert!(
        drives.questions().is_empty(),
        "the stick was not plugged in"
    );
    assert_eq!(volumes_count(&engine), before + 1, "no drive waited for");

    // Kept from its menu, then forgotten; pulled out and plugged in again.
    run(&drives, "skwird.keep_volume", STICK).unwrap();
    run(&drives, "skwird.forget_volume", STICK).unwrap();
    table.lock().remove(&stick_point());
    engine.see(Vec::new());
    drives.plugged();
    table.lock().insert(stick_point());
    engine.see(vec![seen(Some(STICK), &stick_point(), Some("STICK"))]);
    drives.plugged();
    assert!(drives.questions().is_empty(), "decided, never asked");

    // Another plugged in is asked about at once.
    let card = PathBuf::from("/media/someone/CARD");
    table.lock().insert(card.clone());
    engine.see(vec![
        seen(Some(STICK), &stick_point(), Some("STICK")),
        seen(Some("5e6f-7a8b"), &card, Some("CARD")),
    ]);
    drives.plugged();
    let asked = drives.questions();
    assert_eq!(asked.len(), 1);
    assert!(asked[0].text.english().contains("CARD"));

    // One skwird does not see yet: asked once more.
    table.lock().insert(PathBuf::from("/media/someone/LATE"));
    let before = volumes_count(&engine);
    drives.plugged();
    assert_eq!(volumes_count(&engine), before + 2, "asked once more");
}

/// A disk of two volumes plugged in, skwird describing the second a moment
/// after the first (a Ventoy stick's): the first one's question stands, the
/// second waits behind it and is asked once it is answered, never taking
/// its place a moment after it was asked. A drive plugged in later takes
/// the place of a question up.
#[test]
fn a_volume_described_a_moment_later_waits_behind_its_disk_s_question() {
    let tmp = tempfile::tempdir().unwrap();
    let table = Arc::new(Mutex::new(HashSet::new()));
    let engine = drives_engine(
        tmp.path(),
        &["volumes"],
        Some(protocol::Volumes {
            kept: Vec::new(),
            seen: Vec::new(),
        }),
    );
    let drives = Drives::with_table(tmp.path(), "run", &engine.socket, &table);
    drives.look();
    assert!(drives.questions().is_empty());
    let (data, efi) = (
        PathBuf::from("/media/someone/DATA"),
        PathBuf::from("/media/someone/VTOYEFI"),
    );
    table.lock().extend([data.clone(), efi.clone()]);
    let first = seen(Some(STICK), &data, Some("DATA"));
    let second = seen(Some("5e6f-7a8b"), &efi, Some("VTOYEFI"));
    engine.with(|drive| {
        drive.seen = vec![first.clone()];
        drive.then_seen = Some(vec![first.clone(), second.clone()]);
    });
    let before = volumes_count(&engine);
    drives.plugged();
    assert_eq!(volumes_count(&engine), before + 2, "asked once more");
    let asked = drives.questions();
    assert_eq!(asked.len(), 1);
    assert!(
        asked[0].text.english().contains("\u{201c}DATA\u{201d}"),
        "the first one's stands: {}",
        asked[0].text.english()
    );
    assert!(
        drives
            .app
            .question_stands(ID, asked[0].question.expect("a question")),
        "not taken away"
    );
    drives.press(&asked[0], 1);
    let next = drives.questions();
    assert_eq!(next.len(), 1, "the second once it is answered");
    assert!(
        next[0].text.english().contains("\u{201c}VTOYEFI\u{201d}"),
        "{}",
        next[0].text.english()
    );
    // Another drive plugged in later: asked in its place.
    let card = PathBuf::from("/media/someone/CARD");
    table.lock().insert(card.clone());
    engine.see(vec![
        first,
        second,
        seen(Some("9c0d-1e2f"), &card, Some("CARD")),
    ]);
    drives.plugged();
    let later = drives.questions();
    assert_eq!(later.len(), 1);
    assert!(later[0].text.english().contains("\u{201c}CARD\u{201d}"));
    assert!(
        !drives
            .app
            .question_stands(ID, next[0].question.expect("a question")),
        "its place taken"
    );
}

/// While no engine answers, nothing is asked, and a drive the mount table
/// lists no more is let go of: its question, standing, taken back.
#[test]
fn with_no_engine_nothing_is_asked_and_a_drive_gone_is_let_go_of() {
    let tmp = tempfile::tempdir().unwrap();
    let card = PathBuf::from("/media/someone/CARD");
    let table = Arc::new(Mutex::new(HashSet::from([stick_point(), card.clone()])));
    let engine = drives_engine(
        tmp.path(),
        &["volumes"],
        Some(protocol::Volumes {
            kept: Vec::new(),
            seen: vec![
                seen(Some(STICK), &stick_point(), Some("STICK")),
                seen(Some("5e6f-7a8b"), &card, Some("CARD")),
            ],
        }),
    );
    let socket = engine.socket.clone();
    let drives = Drives::with_table(tmp.path(), "run", &socket, &table);
    drives.look();
    let first = drives.questions();
    assert_eq!(first.len(), 1, "the card waits");
    // The engine ends, a mount changes elsewhere; the stick answered.
    drop(engine);
    table.lock().insert(PathBuf::from("/media/someone/OTHER"));
    drives.plugged();
    drives.press(&first[0], 1);
    until(|| drives.volumes().idle());
    assert!(drives.questions().is_empty(), "no engine: nothing asked");

    // A question standing, its drive pulled out while no engine answers.
    let tmp = tempfile::tempdir().unwrap();
    let table = Arc::new(Mutex::new(HashSet::from([stick_point()])));
    let engine = drives_engine(
        tmp.path(),
        &["volumes"],
        Some(protocol::Volumes {
            kept: Vec::new(),
            seen: vec![seen(Some(STICK), &stick_point(), Some("STICK"))],
        }),
    );
    let drives = Drives::with_table(tmp.path(), "run", &engine.socket, &table);
    drives.look();
    let question = drives.questions()[0].question.expect("a question");
    drop(engine);
    table.lock().remove(&stick_point());
    drives.plugged();
    assert!(!drives.app.question_stands(ID, question), "taken back");
}

/// A keep or a forget skwird refuses says what it said; one it answers
/// without doing says so.
#[test]
fn a_keep_skwird_refuses_or_lets_be_says_so() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = drives_engine(
        tmp.path(),
        &["volumes"],
        Some(protocol::Volumes {
            kept: Vec::new(),
            seen: vec![seen(Some(STICK), &stick_point(), Some("STICK"))],
        }),
    );
    let drives = Drives::at(tmp.path(), "run", &engine.socket);
    // Seen once: what names it in what is said.
    drives.look();
    let said = |command: &str| {
        let error = run(&drives, command, STICK).expect_err(command);
        format!("{error:#}")
    };
    engine.with(|drive| drive.refuse = Some("the drive is being checked".into()));
    let refused = said("skwird.keep_volume");
    assert!(
        refused.contains(
            "skwird refused to index the names on \u{201c}STICK\u{201d}: the drive is being checked"
        ),
        "{refused}"
    );
    let refused = said("skwird.forget_volume");
    assert!(
        refused.contains(
            "skwird refused to remove the index of \u{201c}STICK\u{201d}: the drive is being checked"
        ),
        "{refused}"
    );
    engine.with(|drive| {
        drive.refuse = None;
        drive.deaf = true;
    });
    let ignored = said("skwird.keep_volume");
    assert!(
        ignored.contains("skwird did not take the drive"),
        "{ignored}"
    );
    engine.with(|drive| drive.deaf = false);
    run(&drives, "skwird.keep_volume", STICK).unwrap();
    engine.with(|drive| drive.deaf = true);
    let ignored = said("skwird.forget_volume");
    assert!(
        ignored.contains("skwird still keeps the drive"),
        "{ignored}"
    );
}

/// A release is given up on past its one bound, its connect and hello
/// included: an engine that takes the connection and says no hello, and
/// one that says hello and then nothing, each cost the bound, not a hello's
/// time and a silence after it.
#[test]
fn a_release_is_given_up_on_past_its_bound() {
    use std::os::unix::net::UnixListener;
    let point = PathBuf::from("/media/someone/it's USB");
    let tmp = tempfile::tempdir().unwrap();
    let socket = tmp.path().join("stalled");
    let listener = UnixListener::bind(&socket).unwrap();
    std::thread::spawn(move || {
        let held: Vec<_> = listener.incoming().map_while(Result::ok).collect();
        drop(held);
    });
    let silent = Fake::start(
        tmp.path().join("silent"),
        Script {
            features: vec!["volumes".to_owned()],
            silent: true,
            ..Script::default()
        },
    )
    .unwrap();
    for (engine, at) in [
        ("no hello", socket),
        ("silent after it", silent.socket.clone()),
    ] {
        let mut index = index(Engines::At(at));
        index.release_within = Duration::from_millis(300);
        let started = std::time::Instant::now();
        index.release(&point, &CancelToken::new());
        let took = started.elapsed();
        assert!(
            took >= index.release_within && took < Duration::from_secs(3),
            "{engine}: {took:?}"
        );
    }
}
