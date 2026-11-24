//! skwird's index of names (ADR-050, ADR-150): a names index behind core's
//! seam (`skwir_core::names`), which a search asks whether it covers its
//! root. skwird is a project of its own: Skwir speaks its protocol through
//! the client it copies (`vendor/skwird-client`), never links its engine,
//! and walks wherever it does not answer.
//!
//! Each search opens a connection of its own, at its first ask and never
//! at start (ADR-128, ADR-141), and gives it up through the client's
//! `Stopper` once the search's token is cancelled, its connect included,
//! or once its engine says no hello within `HELLO` (`connect`); none is
//! kept between searches, so an engine started on demand ends a minute after the last
//! one. Every error is no engine, and the search walks. The Secrets folders
//! (ADR-125) are kept out of the index through `exclude`, once a run and
//! again once the exclusions change. Where none runs, the first search
//! starts one unless the Index page's switch says not (`skwird.start`).
//! What the user keeps live is `kept`'s (ADR-150, 6).

mod alarm;
mod kept;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};
use std::time::{Duration, SystemTime};

use parking_lot::Mutex;
use skwir_core::app::App;
use skwir_core::cancel::{CancelToken, OnCancel};
use skwir_core::coverage::{Coverage, Hole, Proof, Why};
use skwir_core::entry::EntryKind;
use skwir_core::exclusions::{BUILTIN, Catalog, Role, Rule};
use skwir_core::names::{
    Answer, Change, Child, Extension, Journal, Largest, Mark, NameIndex, Noted, Opened, Query,
    Session, Since, Summed, Totals,
};
use skwir_core::permissions::{Capabilities, Capability};
use skwir_core::plugin::{Category, Plugin, Requirement};
use skwir_core::words::Words;
use skwird_client::protocol;
use skwird_client::{Client, Error, Find, Stopper};

pub const ID: &str = "skwird";

/// The name skwird keeps this client's registrations by.
const CLIENT: &str = "skwir";

/// The longest the engine may say nothing while a search waits on it, past
/// which the search walks: a backstop, longer than a first build of the
/// user's home (6 to 20 s, saying nothing meanwhile). A search given up on
/// stops its call at once, through its token.
const SILENCE: Duration = Duration::from_secs(60);

/// How long an engine may take to say how far it covers a root, asked as
/// each key is typed (`NameIndex::covers`): a round trip, its hello
/// included; one busier than that is not counted on.
const PROBE_SILENCE: Duration = Duration::from_millis(500);

/// The longest an engine may take to say hello, a started one's wait
/// besides (`connect`): an engine answers it at once, a first build under
/// way included. One that takes the connection and says nothing (stopped,
/// not gone) is not waited for past it, its whole `SILENCE`.
const HELLO: Duration = Duration::from_secs(5);

/// What answered, said where the answer is (`RestsOn::by`).
pub fn by() -> Words {
    skwir_core::words!(
        skwird_index = "skwird's index",
        note = "What a search's answer rests on, said in its footer's tooltip: the index skwird keeps of every file's name. skwird is a program's name, as written."
    )
}

/// The plugin: skwird's engines, asked by searches.
#[derive(Default)]
pub struct SkwirdPlugin {
    /// The engine asked in place of the machine's, none started: a test's
    /// fake, or a check's real engine.
    socket: Option<PathBuf>,
}

impl SkwirdPlugin {
    /// The plugin asking the engine at `socket` alone, and starting none.
    pub fn at(socket: PathBuf) -> Self {
        SkwirdPlugin {
            socket: Some(socket),
        }
    }
}

impl Plugin for SkwirdPlugin {
    fn id(&self) -> &'static str {
        ID
    }

    fn title(&self) -> Words {
        skwir_core::words!(
            skwird_plugin_title = "skwird's index of names",
            note = "A plugin's name on the Plugins page. skwird is a program's name, as written."
        )
    }

    fn description(&self) -> Words {
        skwir_core::words!(
            skwird_plugin_description = "Searches answered from skwird's index of every file's name where it proves it is current, rather than by looking through every folder",
            note = "What a plugin does, under its name on the Plugins page."
        )
    }

    fn category(&self) -> Category {
        Category::Browsing
    }

    fn capabilities(&self) -> Capabilities {
        // It starts skwird's engine where none runs.
        Capabilities::of(&[Capability::Programs])
    }

    fn requirements(&self) -> Vec<Requirement> {
        vec![Requirement::new(
            "skwird",
            skwir_core::words!(
                skwird_needs = "Searches answered from its index of names",
                note = "On the Plugins page, what needs skwird, a program the user installs."
            ),
            || {
                skwir_core::os_path::find_program("skwird").is_some()
                    || skwird_client::system_socket().exists()
            },
        )]
    }

    fn register(&self, app: &App) {
        let engines = match &self.socket {
            Some(socket) => Engines::At(socket.clone()),
            None if app.index_services() => Engines::Machine,
            None => Engines::None,
        };
        let kept = kept::Kept::new(app.weak(), engines.clone());
        kept::register(&kept, app);
        app.names.register(
            ID,
            Arc::new(Index {
                app: app.weak(),
                engines,
                secrets: Mutex::default(),
                kept,
                hello: HELLO,
            }),
        );
    }
}

/// Which engines a search asks.
#[derive(Clone, Debug)]
pub(crate) enum Engines {
    /// The machine's: the system service, then the user's engine, then one
    /// started where skwird is installed.
    Machine,
    /// The engine at this socket alone, none started.
    At(PathBuf),
    /// None: a headless test's app (`App::index_services`).
    None,
}

/// skwird as a names index.
struct Index {
    app: Weak<App>,
    engines: Engines,
    /// The Secrets folders this run kept out of the index, and what they
    /// were made from.
    secrets: Mutex<Option<Secrets>>,
    /// What the user keeps live, sent at each connection.
    kept: Arc<kept::Kept>,
    /// How long a connect waits for a hello: `HELLO`, or a test's.
    hello: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Secrets {
    /// The settings' revision and the environment's generation they were
    /// made from (`App::left_out` keys its rule alike).
    from: (u64, u64),
    folders: Vec<PathBuf>,
}

/// How to find an engine, starting one where none runs if `start`; none
/// where none may be asked.
pub(crate) fn find(engines: &Engines, start: bool) -> Option<Find> {
    let mut find = Find::new(CLIENT);
    find.silence = Some(SILENCE);
    // Where the client looks, and where an engine it starts serves and
    // logs: the XDG folders as Skwir reads them.
    find.runtime = skwir_core::xdg::runtime_dir();
    find.state = skwir_core::xdg::state_home();
    match engines {
        // Where the Plugins page finds it (`requirements`): on macOS an
        // application's `PATH` lacks Homebrew's folders, which the client,
        // given a bare name, would not look in.
        Engines::Machine if start => find.start = skwir_core::os_path::find_program("skwird"),
        Engines::Machine => find.start = None,
        Engines::At(socket) => {
            find.socket = Some(socket.clone());
            find.start = None;
        }
        Engines::None => return None,
    }
    Some(find)
}

/// A connection made as `find` says, given up on once `cancel` is, its
/// connect included, and once its engine says no hello within `hello` (a
/// started engine's wait besides): what keeps the stop is held as long as
/// the connection.
pub(crate) fn connect(
    find: &Find,
    cancel: &CancelToken,
    hello: Duration,
) -> Result<(Client, OnCancel), Error> {
    let stopper = Stopper::new();
    let stop = cancel.on_cancel({
        let stopper = stopper.clone();
        move || stopper.stop()
    });
    let time = if find.start.is_some() {
        hello + find.wait
    } else {
        hello
    };
    let client = alarm::within(time, &stopper, || Client::connect_stopped(find, &stopper))
        .unwrap_or(Err(Error::Stopped))?;
    Ok((client, stop))
}

impl Index {
    /// How a search finds an engine: started where none runs, unless the
    /// Index page's switch says not (ADR-150, 9).
    fn find(&self) -> Option<Find> {
        let start = self
            .app
            .upgrade()
            .is_none_or(|app| app.settings.get::<bool>(kept::START).unwrap_or(true));
        find(&self.engines, start)
    }

    /// Keep the Secrets folders out of the index (ADR-125, ADR-150 3),
    /// once this run and again once the exclusions or the variables their
    /// paths start from change. Best effort: an engine busy or failing is
    /// asked again at the next search.
    fn keep_secrets_out(&self, client: &mut Client) {
        let Some(app) = self.app.upgrade() else {
            return;
        };
        let from = (
            app.settings.revision(),
            skwir_core::environment::generation(),
        );
        if self.secrets.lock().as_ref().is_some_and(|s| s.from == from) {
            return;
        }
        let catalog = Catalog::of(&app.settings);
        skwir_core::environment::want(&catalog.variables());
        let folders = secret_folders(&catalog);
        // Asked outside the lock: an engine may take its time.
        if keep_out(client, &folders) {
            *self.secrets.lock() = Some(Secrets { from, folders });
        }
    }
}

impl Index {
    /// What a running engine says of `q.root` (`NameIndex::covers`): no
    /// engine started, nothing sent but the question, given up on past a
    /// round trip; none, where `both_forms`, from an engine whose hello
    /// says it matches one form.
    fn probe(&self, q: &Query, cancel: &CancelToken, both_forms: bool) -> Option<Coverage> {
        let mut find = find(&self.engines, false)?;
        find.silence = Some(PROBE_SILENCE);
        if cancel.is_cancelled() {
            return None;
        }
        // Its hello waited for no longer than the probe's silence.
        let (mut client, _stop) = connect(&find, cancel, PROBE_SILENCE).ok()?;
        if both_forms && !says_both_forms(&client) {
            return None;
        }
        client.coverage(&q.root).ok().map(coverage_of)
    }
}

impl NameIndex for Index {
    fn id(&self) -> &'static str {
        ID
    }

    fn by(&self) -> Words {
        by()
    }

    fn syncs(&self) -> bool {
        true
    }

    fn open(&self, root: &Path, cancel: &CancelToken) -> Option<Opened> {
        let find = self.find()?;
        if cancel.is_cancelled() {
            return None;
        }
        let (mut client, stop) = connect(&find, cancel, self.hello)
            .map_err(|e| said("connect", &e))
            .ok()?;
        self.keep_secrets_out(&mut client);
        // What the user keeps live, sent as differences (a user's engine).
        self.kept.reconcile(&mut client);
        let coverage = client
            .coverage(root)
            .map_err(|e| said("coverage", &e))
            .ok()?;
        Some(Opened {
            coverage: coverage_of(coverage),
            session: Box::new(Asking::new(client, root, stop)),
        })
    }

    /// What a running engine says of `q.root`: no engine started, nothing
    /// sent but the question, given up on past a round trip.
    fn covers(&self, q: &Query, cancel: &CancelToken) -> Option<Coverage> {
        self.probe(q, cancel, false)
    }

    /// [`covers`](Self::covers), where the engine asked now says it matches
    /// both forms ([`BOTH_FORMS`]); none where it does not.
    fn covers_both_forms(&self, q: &Query, cancel: &CancelToken) -> Option<Coverage> {
        self.probe(q, cancel, true)
    }

    /// A session on an engine already running, nothing started nor sent
    /// but what is asked: what follows `root`'s journal (ADR-150, D5).
    fn follow(&self, root: &Path, cancel: &CancelToken) -> Option<Opened> {
        let find = find(&self.engines, false)?;
        if cancel.is_cancelled() {
            return None;
        }
        let (mut client, stop) = connect(&find, cancel, self.hello)
            .map_err(|e| said("connect", &e))
            .ok()?;
        let coverage = client
            .coverage(root)
            .map_err(|e| said("coverage", &e))
            .ok()?;
        Some(Opened {
            coverage: coverage_of(coverage),
            session: Box::new(Asking::new(client, root, stop)),
        })
    }
}

/// The word an engine's hello says once it matches a name term in both
/// its forms, composed and decomposed, as Skwir's search does (skwird's
/// 0.2; `Session::both_forms`).
const BOTH_FORMS: &str = "names-both-forms";

/// Whether the engine `client` talks to said, in this connection's hello,
/// that it matches a name term in both its forms: never kept from an
/// earlier connection, since an engine replaced by an older one says less.
fn says_both_forms(client: &Client) -> bool {
    client
        .welcome()
        .features
        .iter()
        .any(|word| word == BOTH_FORMS)
}

/// One search's connection.
struct Asking {
    client: Client,
    root: PathBuf,
    /// Whether its engine said it matches both forms ([`says_both_forms`]).
    both_forms: bool,
    /// Stops the connection's call once the search is given up on.
    _stop: OnCancel,
}

impl Asking {
    fn new(client: Client, root: &Path, stop: OnCancel) -> Asking {
        Asking {
            both_forms: says_both_forms(&client),
            client,
            root: root.to_path_buf(),
            _stop: stop,
        }
    }
}

impl Session for Asking {
    fn both_forms(&self) -> bool {
        self.both_forms
    }

    fn sync(&mut self) -> Option<Coverage> {
        self.client
            .sync(&self.root)
            .map_err(|e| said("sync", &e))
            .ok()
            .map(coverage_of)
    }

    fn query(&mut self, q: &Query, each: &mut dyn FnMut(Vec<PathBuf>) -> bool) -> Option<Answer> {
        let done = self
            .client
            .query(&query_of(q), |entries| {
                each(entries.iter().map(|e| e.path.to_path()).collect())
            })
            .map_err(|e| said("query", &e))
            .ok()?;
        (!done.cancelled).then(|| Answer {
            coverage: coverage_of(done.coverage),
            truncated: done.truncated,
        })
    }

    fn changes(&mut self, since: Option<&Mark>) -> Option<Since> {
        let token = match since {
            Some(mark) if mark.by != ID => return None,
            Some(mark) => mark.token.as_str(),
            None => "",
        };
        match self.client.changes(&self.root, token) {
            Ok(Some(changes)) => Some(Since::Journal(journal_of(changes))),
            Ok(None) => Some(Since::TooOld),
            Err(e) => {
                said("changes", &e);
                None
            }
        }
    }

    fn totals(
        &mut self,
        folder: &Path,
        largest: Option<Largest>,
        each: &mut dyn FnMut(Vec<Child>) -> bool,
    ) -> Option<Summed> {
        let largest = largest.map(|l| protocol::Largest {
            alloc: l.alloc,
            files: l.files,
        });
        let done = self
            .client
            .totals(folder, largest, |children| {
                each(children.iter().map(child_of).collect())
            })
            .map_err(|e| said("totals", &e))
            .ok()?;
        (!done.cancelled).then(|| Summed {
            totals: totals_of(&done.totals),
            mtime: done.mtime.map(time_of),
            mark: Mark {
                by: ID.to_owned(),
                token: done.token,
            },
            coverage: coverage_of(done.coverage),
            links_known: done.nlink == protocol::Nlink::Complete,
            count: done.count,
            rest: done.rest.map(|rest| (rest.count, totals_of(&rest.totals))),
        })
    }
}

/// An entry a folder's totals name, in core's words.
fn child_of(child: &protocol::Child) -> Child {
    Child {
        path: child.path.to_path(),
        kind: entry_kind(child.kind),
        id: Some((child.id.dev, child.id.ino)),
        mtime: child.mtime.map(time_of),
        totals: totals_of(&child.totals),
    }
}

/// A subtree's totals in core's words.
fn totals_of(totals: &protocol::Totals) -> Totals {
    Totals {
        size: totals.size,
        alloc: totals.alloc,
        files: totals.files,
        names: totals.names,
        folders: totals.folders,
        newest_file: totals.newest_file_mtime.map(time_of),
        by_extension: totals
            .by_extension
            .iter()
            .map(|e| Extension {
                ext: e.ext.as_ref().map(|ext| ext.to_path().into_os_string()),
                files: e.files,
                alloc: e.alloc,
            })
            .collect(),
        shared: totals.shared,
    }
}

/// Whether skwird's live sizes and times are proven here. Not on Windows
/// yet: skwird's live reread takes times from a folder's listing there,
/// updated when the file is closed, so a file being written is behind.
/// There a journal's `attrs` is never said complete (`journal_of`), nor a
/// coverage's `attrs` live (`coverage_of`): a dated search, Usage's totals
/// and a tag set's follower stat for themselves.
const ATTRS_LIVE: bool = !cfg!(windows);

/// A journal in core's words, how far it is every change as skwird says
/// it, its `attrs` as `ATTRS_LIVE` says.
fn journal_of(changes: protocol::Changes) -> Journal {
    let noted = |entry: protocol::Entry| Noted {
        path: entry.path.to_path(),
        kind: entry_kind(entry.kind),
        id: Some((entry.id.dev, entry.id.ino)),
    };
    Journal {
        changes: changes
            .events
            .into_iter()
            .map(|event| match event {
                protocol::Event::Added(entry) => Change::Added(noted(entry)),
                protocol::Event::Removed(entry) => Change::Removed(noted(entry)),
                protocol::Event::Changed(entry) => Change::Changed(noted(entry)),
                protocol::Event::NotCovered { path, why } => Change::NotCovered(Hole {
                    path: path.to_path(),
                    why: why_of(why),
                }),
            })
            .collect(),
        mark: Mark {
            by: ID.to_owned(),
            token: changes.token,
        },
        names: changes.complete.names,
        attrs: changes.complete.attrs && ATTRS_LIVE,
        holes: changes
            .holes
            .into_iter()
            .map(|hole| Hole {
                path: hole.path.to_path(),
                why: why_of(hole.why),
            })
            .collect(),
    }
}

/// An entry's own kind, as skwird says it: what it calls other is none
/// that is read.
fn entry_kind(kind: protocol::Kind) -> EntryKind {
    match kind {
        protocol::Kind::File => EntryKind::File,
        protocol::Kind::Folder => EntryKind::Directory,
        protocol::Kind::Link => EntryKind::Symlink,
        protocol::Kind::Other => EntryKind::Unknown,
    }
}

/// What failed, logged without a path or a name: what a call to skwird
/// came to.
fn said(call: &'static str, error: &Error) {
    let kind = match error {
        Error::NoEngine(_) => "no engine".to_owned(),
        Error::Version => "another protocol".to_owned(),
        Error::Engine(failure) => format!("engine: {}", failure.what),
        Error::Unexpected => "unexpected answer".to_owned(),
        Error::Stopped => "stopped".to_owned(),
        Error::Silent(_) => "silent".to_owned(),
        Error::Io(e) => format!("{:?}", e.kind()),
    };
    tracing::debug!(call, error = kind, "skwird did not answer");
}

/// What the search asks, as skwird takes it: one name, the longest (likely
/// naming the fewest files; the search checks the others), the kinds, the
/// sizes and the modification times (asked only where skwird holds them
/// live: `names::Query::as_covered`), a link answered whatever its own.
fn query_of(q: &Query) -> protocol::Query {
    protocol::Query {
        root: protocol::WirePath::of(&q.root),
        name: q
            .names
            .iter()
            .max_by_key(|n| n.text.chars().count())
            .map(|n| protocol::NameTerm {
                how: protocol::Match::Skwir,
                text: n.text.clone(),
                case_sensitive: n.case_sensitive,
            }),
        kinds: q.kinds.as_ref().map(|kinds| {
            let asked: HashSet<protocol::Kind> = kinds.iter().map(|k| kind_of(*k)).collect();
            [
                protocol::Kind::File,
                protocol::Kind::Folder,
                protocol::Kind::Link,
                protocol::Kind::Other,
            ]
            .into_iter()
            .filter(|k| asked.contains(k))
            .collect()
        }),
        size: q.size.map(|(lo, hi)| [lo, hi]),
        mtime: q.modified.map(|times| {
            [
                times.from.map_or(
                    protocol::Instant {
                        sec: i64::MIN,
                        nsec: 0,
                    },
                    instant_of,
                ),
                times.to.map_or(
                    protocol::Instant {
                        sec: i64::MAX,
                        nsec: 999_999_999,
                    },
                    instant_of,
                ),
            ]
        }),
        limit: None,
        // A link's sizes and times are its target's in a search, its own in
        // skwird's index: answered whatever they are, the search stats each.
        links: if q.size.is_some() || q.modified.is_some() {
            protocol::Links::Unfiltered
        } else {
            protocol::Links::Own
        },
    }
}

/// A time as skwird says it: seconds and nanoseconds since 1970, before
/// it negative.
fn instant_of(t: SystemTime) -> protocol::Instant {
    match t.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(after) => protocol::Instant {
            sec: i64::try_from(after.as_secs()).unwrap_or(i64::MAX),
            nsec: after.subsec_nanos(),
        },
        Err(before) => {
            let before = before.duration();
            let sec = i64::try_from(before.as_secs()).unwrap_or(i64::MAX);
            // 1.5 s before is -2 s and 0.5 s after it.
            if before.subsec_nanos() == 0 {
                protocol::Instant { sec: -sec, nsec: 0 }
            } else {
                protocol::Instant {
                    sec: -sec - 1,
                    nsec: 1_000_000_000 - before.subsec_nanos(),
                }
            }
        }
    }
}

fn kind_of(kind: EntryKind) -> protocol::Kind {
    match kind {
        EntryKind::File => protocol::Kind::File,
        EntryKind::Directory => protocol::Kind::Folder,
        EntryKind::Symlink => protocol::Kind::Link,
        EntryKind::Fifo
        | EntryKind::Socket
        | EntryKind::CharDevice
        | EntryKind::BlockDevice
        | EntryKind::Unknown => protocol::Kind::Other,
    }
}

/// A coverage in core's words, its `attrs` as `ATTRS_LIVE` says.
fn coverage_of(coverage: protocol::Coverage) -> Coverage {
    coverage_in(coverage, ATTRS_LIVE)
}

/// A coverage in core's words; live sizes and times said checked where
/// they are not proven (`attrs_live`): as of the full read live began at,
/// which proves names and kinds alone (`Proof::Checked`).
fn coverage_in(coverage: protocol::Coverage, attrs_live: bool) -> Coverage {
    let attrs = match proof_of(coverage.attrs) {
        Proof::Live { since } if !attrs_live => Proof::Checked { at: since },
        proof => proof,
    };
    Coverage {
        names: proof_of(coverage.names),
        attrs,
        holes: coverage
            .holes
            .into_iter()
            .map(|hole| Hole {
                path: hole.path.to_path(),
                why: why_of(hole.why),
            })
            .collect(),
    }
}

fn proof_of(proof: protocol::Proof) -> Proof {
    match proof {
        protocol::Proof::Live(since) => Proof::Live {
            since: time_of(since),
        },
        protocol::Proof::Checked(at) => Proof::Checked { at: time_of(at) },
        protocol::Proof::AsOf(at) => Proof::AsOf {
            at: Some(time_of(at)),
        },
        protocol::Proof::NotCovered(why) => Proof::NotCovered(why_of(why)),
    }
}

fn why_of(why: protocol::Why) -> Why {
    match why {
        protocol::Why::Outside => Why::Outside,
        protocol::Why::Excluded => Why::Excluded,
        protocol::Why::Private => Why::Private,
        protocol::Why::Denied => Why::Denied,
        protocol::Why::Failed(errno) => Why::Failed(errno),
        protocol::Why::CutShort(errno) => Why::CutShort(errno),
        protocol::Why::Gone => Why::Gone,
        protocol::Why::Replaced => Why::Replaced,
        protocol::Why::Mount => Why::Mount,
        protocol::Why::NetworkMount => Why::NetworkMount,
        protocol::Why::Snapshot => Why::Snapshot,
        protocol::Why::Unread => Why::Unread,
        protocol::Why::WatchLimit => Why::WatchLimit,
    }
}

/// An instant of the protocol's (seconds and nanoseconds since 1970, which
/// may be before it) as the system's; one it cannot hold, 1970.
fn time_of(at: protocol::Instant) -> SystemTime {
    let nanos = Duration::from_nanos(u64::from(at.nsec));
    match u64::try_from(at.sec) {
        Ok(after) => SystemTime::UNIX_EPOCH.checked_add(Duration::from_secs(after) + nanos),
        Err(_) => SystemTime::UNIX_EPOCH
            .checked_sub(Duration::from_secs(at.sec.unsigned_abs()))
            .and_then(|t| t.checked_add(nanos)),
    }
    .unwrap_or(SystemTime::UNIX_EPOCH)
}

/// The folders the Secrets entries kept from agents name (ADR-125),
/// absolute and each once: what skwird is asked to keep out of its index.
/// Names and patterns stay Skwir's to filter, as skwird takes folders only.
fn secret_folders(catalog: &Catalog) -> Vec<PathBuf> {
    let secrets = format!("{BUILTIN}secrets");
    let rule = Rule::of(
        catalog
            .kept_from(Role::Agents)
            .filter(|e| e.category == secrets)
            .map(|e| &e.matcher),
    );
    let mut folders: Vec<PathBuf> = rule
        .listed()
        .iter()
        .filter(|p| p.is_absolute())
        .cloned()
        .collect();
    folders.sort();
    folders.dedup();
    folders
}

/// Make what this client keeps out of skwird's index `folders`, those it
/// kept before and no longer wants taken back: whether skwird took it.
fn keep_out(client: &mut Client, folders: &[PathBuf]) -> bool {
    let add: Vec<&Path> = folders.iter().map(PathBuf::as_path).collect();
    let Ok(kept) = client.exclude(&add, &[]).map_err(|e| said("exclude", &e)) else {
        return false;
    };
    let wanted: HashSet<&Path> = add.iter().copied().collect();
    let stale: Vec<&Path> = kept
        .iter()
        .map(PathBuf::as_path)
        .filter(|p| !wanted.contains(p))
        .collect();
    stale.is_empty()
        || client
            .exclude(&[], &stale)
            .map_err(|e| said("exclude", &e))
            .is_ok()
}

#[cfg(all(test, unix))]
mod tests;
