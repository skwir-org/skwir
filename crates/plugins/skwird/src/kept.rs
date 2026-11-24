//! Kept live (ADR-150, 6): the folders Skwir asks a user's engine of
//! skwird's to keep live, so that a search below one is answered at once
//! and current. The user's own folders and their bookmarks, each source
//! followed or not, any folder they add, a folder of a followed source
//! unticked kept by its path: their data (ADR-057), `skwird.kept`. On the
//! Index page as a list this plugin keeps (`RecordSource`), grouped as the
//! Exclusions page groups its categories, each row saying what skwird says
//! of it, the share of watches under the list.
//!
//! The list is sent as differences (`watch {add, remove}`) at each search's
//! connection and at each change, in the order the share fills in: what
//! the user added, then their folders, then their bookmarks. Only to a
//! user's engine: the system service hears every folder. A change, or the
//! list asked, never starts an engine: the next search that does sends it.
//! Nothing is asked of skwird at start (ADR-150, 3): what it says is asked
//! when the list is (the Index page, an agent), after a change, and at
//! each search's connection.
//!
//! Edits are the user's data, written off the window's thread in the order
//! made (`docs/invariants/settings.md`): queued where they are asked, kept
//! together by one at a time, the window woken once they are, as it is
//! whenever what skwird says changes.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use skwir_core::app::{App, NotifyLevel};
use skwir_core::cancel::CancelToken;
use skwir_core::command::{Invocation, NoArgs, Outcome, Topic};
use skwir_core::os_path::PathArg;
use skwir_core::registry::{MenuItem, MenuSource};
use skwir_core::settings::{RecordField, RecordSource, SettingSpec, page, section};
use skwir_core::words::Words;
use skwird_client::{Client, protocol};

use crate::{Engines, ID};

/// Whether an engine is started where none runs (ADR-150, 9).
pub(crate) const START: &str = "skwird.start";
/// The list on the Index page, which this plugin keeps.
pub(crate) const LIST: &str = "skwird.kept_live";
/// What the user chose of it: their data.
pub(crate) const CHOSEN: &str = "skwird.kept";

/// What the user chose: whether their folders and their bookmarks are
/// followed, the folders they added, and those they unticked. A folder
/// whose name is not UTF-8 is kept whole (`os_path::PathArg`'s form).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Chosen {
    pub own: bool,
    pub bookmarks: bool,
    #[serde(with = "paths")]
    pub added: Vec<PathBuf>,
    #[serde(with = "paths")]
    pub off: Vec<PathBuf>,
}

/// Folders written as `os_path::PathArg` writes a path: text where it is,
/// else its bytes, so that every name is kept (serde's own refuses a path
/// that is not UTF-8).
mod paths {
    use std::path::PathBuf;

    use serde::{Deserialize, Deserializer, Serializer};
    use skwir_core::os_path::PathArg;

    #[allow(clippy::ptr_arg)] // serde's `with` hands the field as it is
    pub fn serialize<S: Serializer>(paths: &Vec<PathBuf>, s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(paths.iter().map(|path| PathArg::from(path.as_path())))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<PathBuf>, D::Error> {
        Ok(Vec::<PathArg>::deserialize(d)?
            .into_iter()
            .map(PathArg::into_inner)
            .collect())
    }
}

impl Default for Chosen {
    fn default() -> Self {
        Chosen {
            own: true,
            bookmarks: true,
            added: Vec::new(),
            off: Vec::new(),
        }
    }
}

impl Chosen {
    fn of(app: &App) -> Chosen {
        app.settings.get::<Chosen>(CHOSEN).unwrap_or_default()
    }
}

/// Where a folder of the list comes from: its group, and the order the
/// share fills in (what was added, then the user's folders, then
/// bookmarks).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Source {
    Added,
    Own,
    Bookmarks,
}

impl Source {
    fn id(self) -> &'static str {
        match self {
            Source::Added => "added",
            Source::Own => "own",
            Source::Bookmarks => "bookmarks",
        }
    }
}

/// A folder of the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Row {
    pub path: PathBuf,
    pub name: String,
    pub source: Source,
    /// Not unticked.
    pub ticked: bool,
    /// Ticked, its source followed: sent to skwird.
    pub kept: bool,
}

/// The folders the user's own folders and bookmarks offer, as last read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Offered {
    pub own: Vec<(String, PathBuf)>,
    pub bookmarks: Vec<(String, PathBuf)>,
}

impl Offered {
    /// Read from the desktop's files (on a worker).
    pub(crate) fn read() -> Offered {
        Offered {
            own: skwir_platform::places::user_dirs()
                .into_iter()
                .map(|d| (d.name, d.path))
                .collect(),
            bookmarks: skwir_platform::places::bookmarks()
                .into_iter()
                .map(|b| (b.title, b.path))
                .collect(),
        }
    }

    /// Whether a source the user follows offers `path`: one not followed
    /// offers nothing (its rows are listed, dimmed, and kept by none).
    fn follows(&self, chosen: &Chosen, path: &Path) -> bool {
        let offers = |folders: &[(String, PathBuf)]| folders.iter().any(|(_, p)| p == path);
        (chosen.own && offers(&self.own)) || (chosen.bookmarks && offers(&self.bookmarks))
    }
}

/// Whether `path` is kept live: not unticked, and added or offered by a
/// source followed (`rows`' `kept`).
fn is_kept(chosen: &Chosen, offered: &Offered, path: &Path) -> bool {
    !chosen.off.iter().any(|p| p == path)
        && (chosen.added.iter().any(|p| p == path) || offered.follows(chosen, path))
}

/// What a folder of the list is known by (a record's `id`, what its tick
/// and Remove send back): its path where it is text, else its bytes
/// (`OsStr::as_encoded_bytes`) in hex after `bytes:`, which no absolute
/// path begins with. `path_of_id` reads it back whole.
const BYTES: &str = "bytes:";

pub(crate) fn id_of(path: &Path) -> String {
    use std::fmt::Write;
    if let Some(text) = path.to_str() {
        return text.to_owned();
    }
    let mut id = String::from(BYTES);
    for byte in path.as_os_str().as_encoded_bytes() {
        let _ = write!(id, "{byte:02x}");
    }
    id
}

/// The folder `id_of` gave `id`: absolute, or refused.
pub(crate) fn path_of_id(id: &str) -> anyhow::Result<PathBuf> {
    let bytes = id.strip_prefix(BYTES).and_then(|hex| {
        (hex.len() % 2 == 0)
            .then(|| {
                (0..hex.len())
                    .step_by(2)
                    .map(|at| u8::from_str_radix(hex.get(at..at + 2)?, 16).ok())
                    .collect::<Option<Vec<u8>>>()
            })
            .flatten()
    });
    let path = match bytes {
        Some(bytes) => PathBuf::from(skwir_core::os_path::from_bytes(bytes)),
        None => PathBuf::from(id),
    };
    skwir_core::os_path::absolute(&path)?;
    Ok(path)
}

/// The list in the order the share fills in, each folder once, where it
/// comes first.
pub(crate) fn rows(chosen: &Chosen, offered: &Offered) -> Vec<Row> {
    let off: HashSet<&Path> = chosen.off.iter().map(PathBuf::as_path).collect();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut rows = Vec::new();
    let mut push = |path: &Path, name: String, source: Source, followed: bool| {
        if !seen.insert(path.to_path_buf()) {
            return;
        }
        let ticked = !off.contains(path);
        rows.push(Row {
            path: path.to_path_buf(),
            name,
            source,
            ticked,
            kept: followed && ticked,
        });
    };
    for path in &chosen.added {
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        push(path, name, Source::Added, true);
    }
    for (name, path) in &offered.own {
        push(path, name.clone(), Source::Own, chosen.own);
    }
    for (name, path) in &offered.bookmarks {
        push(path, name.clone(), Source::Bookmarks, chosen.bookmarks);
    }
    rows
}

/// What to send skwird so that the folders this client keeps (`have`, in
/// the order skwird keeps them, which is the order its share fills in)
/// become `wanted`, in its order: what is no longer wanted taken back; of
/// the rest, the longest run that already stands as `wanted` begins kept,
/// and everything after it taken back and added again in order (skwird
/// takes back first, then adds, after what it keeps). A folder moved ahead
/// costs skwird a check of the folders sent again; nothing is sent where
/// the order holds.
pub(crate) fn sent<'a>(
    have: &'a [PathBuf],
    wanted: &'a [PathBuf],
) -> (Vec<&'a Path>, Vec<&'a Path>) {
    let want: HashSet<&PathBuf> = wanted.iter().collect();
    let kept: Vec<&PathBuf> = have.iter().filter(|path| want.contains(path)).collect();
    let standing = kept
        .iter()
        .zip(wanted)
        .take_while(|(kept, wanted)| **kept == *wanted)
        .count();
    let mut remove: Vec<&Path> = have
        .iter()
        .filter(|path| !want.contains(path))
        .map(PathBuf::as_path)
        .collect();
    remove.extend(kept[standing..].iter().map(|path| path.as_path()));
    let add = wanted[standing..].iter().map(PathBuf::as_path).collect();
    (add, remove)
}

/// What skwird last said of this client's folders.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Said {
    /// The engine that answered (a user's, the system service); none
    /// where none did.
    pub scope: Option<protocol::Scope>,
    pub folders: HashMap<PathBuf, protocol::Kept>,
    pub share: Option<protocol::Share>,
}

/// An edit of what the user chose.
#[derive(Clone, Debug)]
pub(crate) enum Edit {
    /// A folder added by the user (Add Folder…, an agent), ticked: kept
    /// under Added by you whatever source offers it too, so that it stays
    /// once that source no longer does.
    Add(PathBuf),
    /// A folder the user added, taken off the list.
    Remove(PathBuf),
    Tick(PathBuf, bool),
    Follow(Source, bool),
    /// Kept if it is not, not if it is (the menu's): on, a folder a source
    /// followed offers ticked again, any other added; off, one the user
    /// added taken off the list, one a source followed offers unticked.
    Toggle(PathBuf),
}

impl Edit {
    /// Apply it, whether it changed anything.
    pub(crate) fn apply(&self, chosen: &mut Chosen, offered: &Offered) -> bool {
        let before = chosen.clone();
        let tick = |chosen: &mut Chosen, path: &PathBuf, on: bool| {
            let unticked = chosen.off.contains(path);
            if on {
                chosen.off.retain(|p| p != path);
            } else if !unticked {
                chosen.off.push(path.clone());
            }
        };
        let add = |chosen: &mut Chosen, path: &PathBuf| {
            if !chosen.added.contains(path) {
                chosen.added.push(path.clone());
            }
        };
        match self {
            Edit::Add(path) => {
                tick(chosen, path, true);
                add(chosen, path);
            }
            Edit::Remove(path) => {
                chosen.added.retain(|p| p != path);
                chosen.off.retain(|p| p != path);
            }
            Edit::Tick(path, on) => tick(chosen, path, *on),
            Edit::Follow(Source::Own, on) => chosen.own = *on,
            Edit::Follow(Source::Bookmarks, on) => chosen.bookmarks = *on,
            Edit::Follow(Source::Added, _) => {}
            Edit::Toggle(path) => {
                let followed = offered.follows(chosen, path);
                if is_kept(chosen, offered, path) {
                    chosen.added.retain(|p| p != path);
                    if followed {
                        tick(chosen, path, false);
                    }
                } else {
                    tick(chosen, path, true);
                    if !followed {
                        add(chosen, path);
                    }
                }
            }
        }
        *chosen != before
    }
}

/// Reads what the user's own folders and bookmarks offer (`Offered::read`;
/// a test's own).
pub(crate) type Reader = Box<dyn Fn() -> Offered + Send + Sync>;

/// Whether a refresh runs, and whether another was asked while it did.
#[derive(Default)]
pub(crate) struct Refreshing {
    pub running: bool,
    again: bool,
    /// When a listing of the page last asked one (`LISTED_EVERY`).
    listed: Option<std::time::Instant>,
}

/// How often a listing of the Index page asks skwird again: what it says
/// moves while it reads a folder just kept, each answer that differs
/// wakes the window, and the page listed again would ask again at once.
const LISTED_EVERY: std::time::Duration = std::time::Duration::from_secs(5);

/// The list's keeper: what the user chose, what the sources offer, what
/// skwird said, and the list on the Index page.
pub(crate) struct Kept {
    pub source: RecordSource,
    app: Weak<App>,
    engines: Engines,
    read: Reader,
    pub(crate) offered: Mutex<Offered>,
    pub(crate) said: Mutex<Said>,
    /// Whether an engine this run said it was a user's (ADR-150, 10): the
    /// menu's entry stays once one did, whatever a later ask finds.
    found_user: AtomicBool,
    /// The edits asked and not kept yet, in the order asked.
    pub(crate) edits: Mutex<Vec<Edit>>,
    /// One keeps the edits at a time: what the user chose, read and written
    /// by two at once, would lose one's edit.
    pub(crate) keeping: Mutex<()>,
    pub(crate) refreshing: Mutex<Refreshing>,
}

impl Kept {
    pub(crate) fn new(app: Weak<App>, engines: Engines) -> Arc<Kept> {
        Kept::reading(app, engines, Box::new(Offered::read))
    }

    /// The keeper, what the sources offer read by `read`.
    pub(crate) fn reading(app: Weak<App>, engines: Engines, read: Reader) -> Arc<Kept> {
        Arc::new(Kept {
            source: RecordSource::new(
                "skwird.kept_list",
                "skwird.kept_add",
                "",
                "skwird.kept_remove",
            )
            .adding_folders(),
            app,
            engines,
            read,
            offered: Mutex::default(),
            said: Mutex::default(),
            found_user: AtomicBool::new(false),
            edits: Mutex::default(),
            keeping: Mutex::default(),
            refreshing: Mutex::default(),
        })
    }

    /// Whether an engine this run was a user's, which keeps folders live:
    /// where the menu's entry does something.
    pub(crate) fn user_engine(&self) -> bool {
        self.found_user.load(Ordering::Acquire)
    }

    /// Whether `path` is kept live.
    fn keeps(&self, app: &App, path: &Path) -> bool {
        is_kept(&Chosen::of(app), &self.offered.lock(), path)
    }

    /// Say that the list changed: the dialog asks again, woken.
    fn changed(&self) {
        self.source.changed();
        if let Some(app) = self.app.upgrade() {
            app.wake();
        }
    }

    pub(crate) fn said(&self, said: Said, offered: Option<Offered>) {
        let mut changed = false;
        if let Some(offered) = offered {
            let mut held = self.offered.lock();
            changed |= *held != offered;
            *held = offered;
        }
        {
            let mut held = self.said.lock();
            changed |= *held != said;
            *held = said;
        }
        if changed {
            self.changed();
        }
    }

    /// Make what this client keeps live in `client`'s engine the list's
    /// folders, and keep what it says: a user's engine only. Blocking, on
    /// a worker.
    pub(crate) fn reconcile(&self, client: &mut Client) {
        let scope = client.welcome().scope;
        let offered = (self.read)();
        if scope == protocol::Scope::User {
            self.found_user.store(true, Ordering::Release);
        }
        if scope != protocol::Scope::User {
            self.said(
                Said {
                    scope: Some(scope),
                    ..Said::default()
                },
                Some(offered),
            );
            return;
        }
        let Some(app) = self.app.upgrade() else {
            return;
        };
        let wanted: Vec<PathBuf> = rows(&Chosen::of(&app), &offered)
            .into_iter()
            .filter(|row| row.kept)
            .map(|row| row.path)
            .collect();
        // Before phase 2, an engine keeps nothing live.
        let Ok(now) = client.watch(&[], &[]) else {
            self.said(
                Said {
                    scope: Some(scope),
                    ..Said::default()
                },
                Some(offered),
            );
            return;
        };
        let have: Vec<PathBuf> = now.folders.iter().map(|k| k.path.to_path()).collect();
        let (add, remove) = sent(&have, &wanted);
        let answer = if add.is_empty() && remove.is_empty() {
            now
        } else {
            match client.watch(&add, &remove) {
                Ok(answer) => answer,
                Err(_) => return,
            }
        };
        self.said(
            Said {
                scope: Some(scope),
                folders: answer
                    .folders
                    .into_iter()
                    .map(|kept| (kept.path.to_path(), kept))
                    .collect(),
                share: answer.share,
            },
            Some(offered),
        );
    }

    /// Ask a running engine what it keeps, and send it the list, on a
    /// worker: never starting one. One at a time: one asked while another
    /// runs makes it go once more, so that what was asked after it began
    /// (a folder added meanwhile) is sent.
    pub(crate) fn refresh(self: &Arc<Self>) {
        {
            let mut refreshing = self.refreshing.lock();
            if refreshing.running {
                refreshing.again = true;
                return;
            }
            refreshing.running = true;
        }
        let Some(app) = self.app.upgrade() else {
            self.refreshing.lock().running = false;
            return;
        };
        let this = Arc::clone(self);
        let cancel = app.lifetime().child();
        let stop = cancel.clone();
        app.scheduler.spawn(
            skwir_core::scheduler::Priority::Background,
            "skwird.kept",
            cancel,
            move || {
                loop {
                    this.ask_engine(&stop);
                    let mut refreshing = this.refreshing.lock();
                    if !std::mem::take(&mut refreshing.again) || stop.is_cancelled() {
                        refreshing.running = false;
                        return;
                    }
                }
            },
        );
    }

    /// [`refresh`](Self::refresh), for a listing of the page: once every
    /// `LISTED_EVERY` at most. An edit refreshes at once.
    fn refresh_listed(self: &Arc<Self>) {
        {
            let mut refreshing = self.refreshing.lock();
            if refreshing
                .listed
                .is_some_and(|at| at.elapsed() < LISTED_EVERY)
            {
                return;
            }
            refreshing.listed = Some(std::time::Instant::now());
        }
        self.refresh();
    }

    /// What a running engine keeps, the list sent to it: what is asked
    /// stopped once `cancel` is (the program ending).
    fn ask_engine(&self, cancel: &CancelToken) {
        let client = crate::find(&self.engines, false)
            .and_then(|find| crate::connect(&find, cancel, crate::HELLO).ok());
        let Some((mut client, _stop)) = client else {
            self.said(Said::default(), Some((self.read)()));
            return;
        };
        self.reconcile(&mut client);
    }

    /// Queue `edit` of what the user chose, in the order asked, and keep
    /// what is queued: on a worker when asked from the window, where asked
    /// otherwise (an agent's call, a script's).
    pub(crate) fn edit(self: &Arc<Self>, edit: Edit) {
        self.edits.lock().push(edit);
        match self.app.upgrade() {
            Some(app) if skwir_core::scheduler::on_window_thread() => {
                let this = Arc::clone(self);
                app.scheduler.spawn(
                    skwir_core::scheduler::Priority::Normal,
                    "skwird.kept_edit",
                    app.lifetime().child(),
                    move || this.keep_edits(),
                );
            }
            _ => self.keep_edits(),
        }
    }

    /// Apply the edits queued to what the user chose, in order, save it,
    /// wake the window, then send the list. What the sources offer is read
    /// again first: an edit before any search is judged against them.
    fn keep_edits(self: &Arc<Self>) {
        let Some(app) = self.app.upgrade() else {
            return;
        };
        {
            let _keeping = self.keeping.lock();
            let edits = std::mem::take(&mut *self.edits.lock());
            if edits.is_empty() {
                return;
            }
            let offered = (self.read)();
            let before = Chosen::of(&app);
            let mut chosen = before.clone();
            for edit in &edits {
                edit.apply(&mut chosen, &offered);
            }
            *self.offered.lock() = offered;
            if chosen != before {
                match toml::Value::try_from(&chosen) {
                    Ok(value) => {
                        app.settings.set(CHOSEN, value);
                        if let Err(error) = app.settings.save() {
                            not_saved(&app, error);
                        }
                    }
                    Err(error) => not_saved(&app, error.into()),
                }
            }
            self.changed();
        }
        self.refresh();
    }

    /// The list as the Index page draws it (`RecordSource`): from memory.
    fn list(self: &Arc<Self>, app: &App) -> serde_json::Value {
        let chosen = Chosen::of(app);
        let offered = self.offered.lock().clone();
        let said = self.said.lock().clone();
        // What skwird says now, and the sources, for the next listing.
        self.refresh_listed();
        let records: Vec<serde_json::Value> = rows(&chosen, &offered)
            .iter()
            .map(|row| {
                let id = id_of(&row.path);
                serde_json::json!({
                    "id": id,
                    "name": Words::as_written(row.name.clone()).to_data(),
                    "state": state(row, &said).to_data(),
                    "group": row.source.id(),
                    "tick": {
                        "on": row.ticked,
                        "command": "skwird.kept_tick",
                        "args": { "id": id },
                    },
                    // Only what the user added is removed.
                    "fixed": row.source != Source::Added,
                })
            })
            .collect();
        let follow = |source: Source, on: bool| {
            serde_json::json!({
                "on": on,
                "command": "skwird.kept_follow",
                "args": { "source": source.id() },
            })
        };
        serde_json::json!({
            "records": records,
            "groups": [
                {
                    "id": Source::Own.id(),
                    "title": skwir_core::words!(
                        skwird_kept_own = "Your folders",
                        note = "A group of the folders kept live, on the Index page: the desktop's folders (Documents, Pictures…), with a switch following them."
                    ).to_data(),
                    "switch": follow(Source::Own, chosen.own),
                },
                {
                    "id": Source::Bookmarks.id(),
                    "title": skwir_core::words!(
                        skwird_kept_bookmarks = "Bookmarks",
                        note = "A group of the folders kept live, on the Index page: the bookmarked folders, with a switch following them."
                    ).to_data(),
                    "switch": follow(Source::Bookmarks, chosen.bookmarks),
                },
                {
                    "id": Source::Added.id(),
                    "title": skwir_core::words!(
                        skwird_kept_added = "Added by you",
                        note = "A group of the folders kept live, on the Index page: those the user added."
                    ).to_data(),
                },
            ],
            "note": note(&said, app.settings.get::<bool>(START).unwrap_or(true)).to_data(),
            // The system service hears every folder: the list does nothing
            // there (ADR-150, 6), as its note says.
            "dimmed": said.scope == Some(protocol::Scope::System),
        })
    }
}

/// Say that what the user chose of the folders kept live was not saved.
fn not_saved(app: &App, error: anyhow::Error) {
    app.notify(
        NotifyLevel::Error,
        skwir_core::words!(
            skwird_kept_not_saved = "The folders kept live were not saved: { $error }",
            error = error,
            note = "error: why the settings' file could not be written."
        ),
    );
}

/// What a row says under its name: its folder, and how skwird keeps it.
fn state(row: &Row, said: &Said) -> Words {
    let folder = skwir_core::exclusions::tilde(&row.path);
    if !row.kept {
        return skwir_core::words!(
            skwird_kept_not = "{ $folder } · not kept live",
            folder = folder,
            note = "Under a folder's name in the list of folders kept live: unticked, or its group switched off. folder: where it is."
        );
    }
    let kept = match said.scope {
        None => {
            return skwir_core::words!(
                skwird_kept_waiting = "{ $folder } · kept live once skwird runs",
                folder = folder,
                note = "Under a folder's name in the list of folders kept live, while no engine of skwird's answers. skwird is a program's name, as written."
            );
        }
        Some(protocol::Scope::System) => {
            return skwir_core::words!(
                skwird_kept_system = "{ $folder } · live, as every folder",
                folder = folder,
                note = "Under a folder's name in the list of folders kept live, where skwird's system service hears every folder."
            );
        }
        Some(protocol::Scope::User) => said.folders.get(&row.path),
    };
    let Some(kept) = kept else {
        return skwir_core::words!(
            skwird_kept_sending = "{ $folder } · being sent to skwird",
            folder = folder,
            note = "Under a folder's name in the list of folders kept live, before skwird answered for it."
        );
    };
    match (kept.state, &kept.within) {
        (protocol::Keeping::Live(_), Some(within)) => skwir_core::words!(
            skwird_kept_within = "{ $folder } · live, inside { $within }",
            folder = folder,
            within = skwir_core::exclusions::tilde(&within.to_path()),
            note = "Under a folder's name in the list of folders kept live: another folder kept live holds it. within: that folder."
        ),
        (protocol::Keeping::Live(_), None) => skwir_core::words!(
            skwird_kept_live = "{ $folder } · live",
            folder = folder,
            note = "Under a folder's name in the list of folders kept live: every change in it heard as it comes."
        ),
        (protocol::Keeping::NotLive(protocol::NotLive::Share), _) => skwir_core::words!(
            skwird_kept_share = "{ $folder } · not live: the share of watches is spent before it",
            folder = folder,
            note = "Under a folder's name in the list of folders kept live: the folders before it took the watches skwird may use; it is checked instead."
        ),
        (protocol::Keeping::NotLive(protocol::NotLive::WatchLimit), _) => skwir_core::words!(
            skwird_kept_limit = "{ $folder } · not live: other programs use the system's watches",
            folder = folder,
            note = "Under a folder's name in the list of folders kept live: the system's limit of watched folders is reached."
        ),
        (protocol::Keeping::NotLive(protocol::NotLive::NotCovered(_)), _) => skwir_core::words!(
            skwird_kept_not_covered =
                "{ $folder } · not indexed there (a network, a mount, an exclusion)",
            folder = folder,
            note = "Under a folder's name in the list of folders kept live: skwird indexes nothing there."
        ),
        (protocol::Keeping::NotLive(protocol::NotLive::Missing), _) => skwir_core::words!(
            skwird_kept_missing = "{ $folder } · no folder there",
            folder = folder,
            note = "Under a folder's name in the list of folders kept live: nothing is there now."
        ),
        (protocol::Keeping::NotLive(protocol::NotLive::Pending), _) => skwir_core::words!(
            skwird_kept_pending = "{ $folder } · becoming live…",
            folder = folder,
            note =
                "Under a folder's name in the list of folders kept live: skwird is setting it up."
        ),
        (protocol::Keeping::NotLive(protocol::NotLive::Failed(_)), _) => skwir_core::words!(
            skwird_kept_failed = "{ $folder } · not live: a watch failed",
            folder = folder,
            note = "Under a folder's name in the list of folders kept live: the system refused to watch it; it is checked instead."
        ),
    }
}

/// What the list says under it: the share of watches, or why there is
/// none.
fn note(said: &Said, starts: bool) -> Words {
    match (said.scope, said.share) {
        (Some(protocol::Scope::System), _) => skwir_core::words!(
            skwird_kept_note_system =
                "skwird's system service hears every local folder: each is live already.",
            note = "Under the list of folders kept live, where skwird's system service answers. skwird is a program's name, as written."
        ),
        (Some(protocol::Scope::User), Some(share)) => skwir_core::words!(
            skwird_kept_note_share = "Watches used: { $used } of { $size }. Where they run short, the folders you added come first, then your folders, then bookmarks.",
            used = share.used,
            size = share.size,
            note = "Under the list of folders kept live: how many of the watches skwird may use are taken, all its clients together."
        ),
        (Some(protocol::Scope::User), None) => skwir_core::words!(
            skwird_kept_note_user = "Where watches run short, the folders you added come first, then your folders, then bookmarks.",
            note = "Under the list of folders kept live, where skwird says nothing of its share."
        ),
        (None, _) if starts => skwir_core::words!(
            skwird_kept_note_none = "No engine of skwird's runs now: the list is sent at the next search, which starts one.",
            note = "Under the list of folders kept live, where none answers. skwird is a program's name, as written."
        ),
        (None, _) => skwir_core::words!(
            skwird_kept_note_none_off =
                "No engine of skwird's runs now: the list is sent once one runs.",
            note = "Under the list of folders kept live, where none answers and Skwir starts none."
        ),
    }
}

#[derive(Deserialize, schemars::JsonSchema)]
struct IdArgs {
    /// A folder of the list, by its `id` there (its path, or `bytes:` and
    /// its bytes in hex where its name is not UTF-8).
    id: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct TickArgs {
    /// A folder of the list, by its `id` there.
    id: String,
    /// Kept live or not.
    on: bool,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct FollowArgs {
    /// `own` (the desktop's folders) or `bookmarks`.
    source: String,
    on: bool,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct PathArgs {
    /// A folder, absolute.
    path: PathArg,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct KeepArgs {
    /// A folder; none, the one selected in `pane`.
    #[serde(default)]
    path: Option<PathArg>,
    #[serde(default)]
    pane: Option<skwir_core::workspace::PaneId>,
}

/// The folder a command is about: given, else the one selected.
fn folder_of(
    app: &App,
    path: Option<PathArg>,
    pane: Option<skwir_core::workspace::PaneId>,
) -> anyhow::Result<PathBuf> {
    if let Some(path) = path {
        skwir_core::os_path::absolute(&path)?;
        return Ok(path.into_inner());
    }
    let targets = app.targets(pane, Vec::new())?;
    match targets.as_slice() {
        [one] => Ok(one.path.clone()),
        _ => Err(skwir_core::words!(
            skwird_keep_live_one = "Select one folder to keep live",
            note = "An error of the command keeping a folder live, asked with no folder named and none or several selected."
        )
        .into()),
    }
}

/// Declare the settings, the list's commands and the menu's entry.
pub(crate) fn register(kept: &Arc<Kept>, app: &App) {
    app.settings.declare(
        SettingSpec::toggle(
            START,
            skwir_core::words!(
                skwird_start = "Start skwird where none runs",
                note = "A setting's title on the Index page: whether a search starts skwird's engine. skwird is a program's name, as written."
            ),
            (
                skwir_core::words!(
                    skwird_start_description = "The first search starts an engine of skwird's, which ends a minute after the last one.",
                    note = "What the setting starting skwird's engine says."
                ),
                skwir_core::words!(
                    skwird_start_description_more = "A first start reads your whole home: seconds, and memory while it builds. Off, only an engine already running is asked.",
                    note = "What a setting's description says after its first sentence, in its tooltip."
                ),
            ),
            true,
        )
        .page(page::INDEX)
        .section(section::NAMES_INDEX),
    );
    app.settings.declare(SettingSpec::data(
        CHOSEN,
        skwir_core::words!(
            skwird_kept_data = "The folders kept live: { $shape }",
            shape = "{ own, bookmarks, added, off }",
            note = "Read by agents: the data holding the folders kept live. shape: how it is written, as written."
        ),
        toml::Value::try_from(Chosen::default()).unwrap_or(toml::Value::Table(toml::Table::new())),
    ));
    app.settings.declare(
        SettingSpec::records(
            LIST,
            skwir_core::words!(
                skwird_kept_title = "Kept live",
                note = "A setting's title on the Index page: the folders skwird keeps live for searches."
            ),
            (
                skwir_core::words!(
                    skwird_kept_description = "A search below one of these folders is answered at once and up to date; elsewhere skwird checks first, about half a second.",
                    note = "What the list of folders kept live says. skwird is a program's name, as written."
                ),
                skwir_core::words!(
                    skwird_kept_description_more = "Your Secrets are kept out of skwird's index; caches are indexed, and left out of searches as Exclusions says.",
                    note = "What a setting's description says after its first sentence, in its tooltip."
                ),
            ),
            skwir_core::words!(
                skwird_kept_add = "Add Folder…",
                note = "The button adding a folder to the folders kept live."
            ),
            vec![
                RecordField::text(
                    "name",
                    skwir_core::words!(skwird_kept_name = "Folder"),
                    Words::default(),
                    Words::default(),
                ),
                RecordField::text(
                    "state",
                    skwir_core::words!(skwird_kept_state = "State"),
                    Words::default(),
                    Words::default(),
                ),
            ],
        )
        .records_kept_by(kept.source.clone())
        .page(page::INDEX)
        .section(section::NAMES_INDEX),
    );

    let k = Arc::clone(kept);
    app.commands
        .register::<NoArgs, _>(
            ID,
            Topic::Find,
            "skwird.kept_list",
            skwir_core::words!(skwird_kept_list = "Folders Kept Live"),
            skwir_core::words!(
                skwird_kept_list_description = "The folders skwird keeps live for searches, as the Index page lists them: each one's group, whether it is ticked, and what skwird says of it"
            ),
            move |_, app| Ok(Outcome::Value(k.list(app))),
        )
        .data();
    let k = Arc::clone(kept);
    app.commands
        .register::<PathArgs, _>(
            ID,
            Topic::Find,
            "skwird.kept_add",
            skwir_core::words!(skwird_kept_add_command = "Keep a Folder Live"),
            skwir_core::words!(
                skwird_kept_add_command_description =
                    "Add a folder to those skwird keeps live for searches"
            ),
            move |args, _| {
                skwir_core::os_path::absolute(&args.path)?;
                k.edit(Edit::Add(args.path.into_inner()));
                Ok(Outcome::Done)
            },
        )
        .keeps();
    let k = Arc::clone(kept);
    app.commands
        .register::<IdArgs, _>(
            ID,
            Topic::Find,
            "skwird.kept_remove",
            skwir_core::words!(skwird_kept_remove_command = "Stop Keeping a Folder Live"),
            skwir_core::words!(
                skwird_kept_remove_command_description =
                    "Take a folder you added off those skwird keeps live"
            ),
            move |args, _| {
                k.edit(Edit::Remove(path_of_id(&args.id)?));
                Ok(Outcome::Done)
            },
        )
        .keeps();
    let k = Arc::clone(kept);
    app.commands
        .register::<TickArgs, _>(
            ID,
            Topic::Find,
            "skwird.kept_tick",
            skwir_core::words!(skwird_kept_tick_command = "Tick a Folder Kept Live"),
            skwir_core::words!(
                skwird_kept_tick_command_description =
                    "Keep a folder of the list live, or not, as its tick says"
            ),
            move |args, _| {
                k.edit(Edit::Tick(path_of_id(&args.id)?, args.on));
                Ok(Outcome::Done)
            },
        )
        .keeps();
    let k = Arc::clone(kept);
    app.commands
        .register::<FollowArgs, _>(
            ID,
            Topic::Find,
            "skwird.kept_follow",
            skwir_core::words!(skwird_kept_follow_command = "Follow a Source of Folders Kept Live"),
            skwir_core::words!(
                skwird_kept_follow_command_description = "Keep live the desktop's folders (own) or the bookmarks, as they come and go, or not"
            ),
            move |args, _| {
                let source = match args.source.as_str() {
                    "own" => Source::Own,
                    "bookmarks" => Source::Bookmarks,
                    other => {
                        return Err(skwir_core::words!(
                            skwird_kept_no_source =
                                "{ $source } is no source of folders kept live: own or bookmarks",
                            source = skwir_core::words::Name(other.to_owned()),
                            note = "An error of the command following a source of the folders kept live: source is what was asked, as written; own and bookmarks are the values it takes, as written."
                        )
                        .into());
                    }
                };
                k.edit(Edit::Follow(source, args.on));
                Ok(Outcome::Done)
            },
        )
        .keeps();
    let k = Arc::clone(kept);
    app.commands
        .register::<KeepArgs, _>(
            ID,
            Topic::Find,
            "skwird.keep_live",
            skwir_core::words!(skwird_keep_live = "Keep Live for Searches"),
            skwir_core::words!(
                skwird_keep_live_description = "Keep a folder live in skwird's index, or stop: the one given, else the one selected"
            ),
            move |args, app| {
                k.edit(Edit::Toggle(folder_of(app, args.path, args.pane)?));
                Ok(Outcome::Done)
            },
        )
        .keeps();
    let k = Arc::clone(kept);
    app.commands
        .register::<KeepArgs, _>(
            ID,
            Topic::Find,
            "skwird.keep_live_menu",
            skwir_core::words!(skwird_keep_live_menu = "Keep Live in the Menu"),
            skwir_core::words!(
                skwird_keep_live_menu_description = "The menu's entry keeping a folder live: only once a search found a user's engine of skwird's, which keeps folders live"
            ),
            move |args, app| {
                let items = match folder_of(app, args.path, args.pane) {
                    Ok(path) if k.user_engine() => {
                        let on = k.keeps(app, &path);
                        vec![
                            MenuItem::new(
                                "context",
                                skwir_core::words!(
                                    skwird_keep_live_item = "Keep Live for Searches",
                                    note = "An entry of a folder's menu: skwird keeps it live, so that a search below it answers at once. Checked while it is."
                                ),
                                "eye",
                                165,
                                // A name that is not UTF-8 written whole.
                                Invocation::with_args(
                                    "skwird.keep_live",
                                    &serde_json::json!({ "path": PathArg::from(path.as_path()) }),
                                ),
                            )
                            .checked(on),
                        ]
                    }
                    _ => Vec::new(),
                };
                Ok(Outcome::Value(serde_json::json!({ "items": items })))
            },
        )
        .data();
    // Asked as a folder's menu opens: from memory.
    app.registry.menu_sources.add(
        ID,
        MenuSource {
            menu: "context".into(),
            when: Some("single dir".into()),
            command: Invocation::new("skwird.keep_live_menu"),
            replaces: Vec::new(),
        },
    );
    // Nothing is asked of skwird here (ADR-150, 3): the list asks when it
    // is listed, a search at its connection.
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offered() -> Offered {
        Offered {
            own: vec![
                ("Documents".into(), PathBuf::from("/h/Documents")),
                ("Pictures".into(), PathBuf::from("/h/Pictures")),
            ],
            bookmarks: vec![
                ("wip".into(), PathBuf::from("/h/wip")),
                ("Docs again".into(), PathBuf::from("/h/Documents")),
            ],
        }
    }

    /// In the share's order (added, the user's folders, bookmarks), each
    /// folder once, an unticked one or one of a source not followed not
    /// kept.
    #[test]
    fn the_list_is_in_the_share_s_order_each_folder_once() {
        let chosen = Chosen {
            own: true,
            bookmarks: false,
            added: vec![PathBuf::from("/data/projects")],
            off: vec![PathBuf::from("/h/Pictures")],
        };
        let rows = rows(&chosen, &offered());
        let said: Vec<(&str, Source, bool, bool)> = rows
            .iter()
            .map(|r| (r.path.to_str().unwrap(), r.source, r.ticked, r.kept))
            .collect();
        assert_eq!(
            said,
            [
                ("/data/projects", Source::Added, true, true),
                ("/h/Documents", Source::Own, true, true),
                ("/h/Pictures", Source::Own, false, false),
                ("/h/wip", Source::Bookmarks, true, false),
            ]
        );
    }

    /// The share fills in the order skwird keeps a client's folders: what
    /// is sent keeps the list's order, sending nothing where it holds.
    #[test]
    fn what_is_sent_keeps_the_list_s_order() {
        let p = |names: &[&str]| -> Vec<PathBuf> { names.iter().map(PathBuf::from).collect() };
        let shown = |(add, remove): (Vec<&Path>, Vec<&Path>)| {
            let names = |list: Vec<&Path>| -> Vec<String> {
                list.iter().map(|p| p.display().to_string()).collect()
            };
            (names(add), names(remove))
        };
        let none: Vec<String> = Vec::new();
        // Holding: nothing sent.
        assert_eq!(
            shown(sent(&p(&["a", "b"]), &p(&["a", "b"]))),
            (none.clone(), none.clone())
        );
        // New at the end: added after.
        assert_eq!(
            shown(sent(&p(&["a"]), &p(&["a", "b"]))),
            (vec!["b".into()], none.clone())
        );
        // New in the middle: what follows it sent again, after it.
        assert_eq!(
            shown(sent(&p(&["a", "c"]), &p(&["a", "b", "c"]))),
            (vec!["b".into(), "c".into()], vec!["c".into()])
        );
        // Moved ahead: from where the order breaks.
        assert_eq!(
            shown(sent(&p(&["a", "b", "c"]), &p(&["c", "a", "b"]))),
            (
                vec!["c".into(), "a".into(), "b".into()],
                vec!["a".into(), "b".into(), "c".into()]
            )
        );
        // No longer wanted: taken back, the order of the rest kept.
        assert_eq!(
            shown(sent(&p(&["a", "x", "b"]), &p(&["a", "b"]))),
            (none.clone(), vec!["x".into()])
        );
        assert_eq!(shown(sent(&p(&[]), &p(&[]))), (none.clone(), none));
    }

    #[test]
    fn edits_add_remove_tick_follow_and_toggle() {
        let offered = offered();
        let mut chosen = Chosen::default();
        assert!(Edit::Add(PathBuf::from("/data/x")).apply(&mut chosen, &offered));
        assert!(
            !Edit::Add(PathBuf::from("/data/x")).apply(&mut chosen, &offered),
            "once"
        );
        assert!(Edit::Tick(PathBuf::from("/h/Documents"), false).apply(&mut chosen, &offered));
        assert_eq!(chosen.off, [PathBuf::from("/h/Documents")]);
        // Added back from the menu: ticked again, not added.
        assert!(Edit::Toggle(PathBuf::from("/h/Documents")).apply(&mut chosen, &offered));
        assert!(chosen.off.is_empty() && chosen.added == [PathBuf::from("/data/x")]);
        // Off from the menu: a source's unticked, an added one removed.
        assert!(Edit::Toggle(PathBuf::from("/h/Pictures")).apply(&mut chosen, &offered));
        assert_eq!(chosen.off, [PathBuf::from("/h/Pictures")]);
        assert!(Edit::Toggle(PathBuf::from("/data/x")).apply(&mut chosen, &offered));
        assert!(chosen.added.is_empty());
        assert!(Edit::Follow(Source::Bookmarks, false).apply(&mut chosen, &offered));
        assert!(!chosen.bookmarks);
        assert!(Edit::Add(PathBuf::from("/data/y")).apply(&mut chosen, &offered));
        assert!(Edit::Remove(PathBuf::from("/data/y")).apply(&mut chosen, &offered));
        assert!(chosen.added.is_empty());
    }

    /// A source switched off offers nothing: its folder kept from the menu
    /// is added, and off again taken off the list. One added by the user
    /// that a source offers too is kept under Added by you, so that it
    /// stays once the source no longer offers it; off from the menu, it is
    /// unticked under its source as well.
    #[test]
    fn a_folder_of_a_source_not_followed_is_added_and_an_explicit_add_is_kept() {
        let offered = offered();
        let wip = PathBuf::from("/h/wip");
        let mut chosen = Chosen {
            bookmarks: false,
            ..Chosen::default()
        };
        assert!(!is_kept(&chosen, &offered, &wip));
        assert!(Edit::Toggle(wip.clone()).apply(&mut chosen, &offered));
        assert_eq!(chosen.added, std::slice::from_ref(&wip));
        assert!(is_kept(&chosen, &offered, &wip), "kept, as the menu said");
        assert!(Edit::Toggle(wip.clone()).apply(&mut chosen, &offered));
        assert!(chosen.added.is_empty() && chosen.off.is_empty());
        assert!(!is_kept(&chosen, &offered, &wip));

        let mut chosen = Chosen::default();
        let documents = PathBuf::from("/h/Documents");
        assert!(Edit::Add(documents.clone()).apply(&mut chosen, &offered));
        assert_eq!(chosen.added, std::slice::from_ref(&documents), "recorded");
        let listed = rows(&chosen, &offered);
        assert_eq!(
            listed.iter().filter(|row| row.path == documents).count(),
            1,
            "listed once"
        );
        assert_eq!(listed[0].source, Source::Added);
        let gone = Offered::default();
        assert!(
            is_kept(&chosen, &gone, &documents),
            "kept once no source offers it"
        );
        assert!(Edit::Toggle(documents.clone()).apply(&mut chosen, &offered));
        assert!(chosen.added.is_empty() && chosen.off == [documents.clone()]);
        assert!(!is_kept(&chosen, &offered, &documents));
    }

    /// A folder's id names it whole, a name that is not UTF-8 too, and what
    /// the user chose is saved so.
    #[test]
    fn every_folder_is_named_and_kept_whole() {
        let plain = PathBuf::from("/h/Documents");
        assert_eq!(id_of(&plain), "/h/Documents");
        assert_eq!(path_of_id(&id_of(&plain)).unwrap(), plain);
        assert!(path_of_id("relative/folder").is_err(), "absolute only");
        assert!(
            path_of_id("bytes:zz").is_err(),
            "no bytes: read as a relative path"
        );
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let odd = PathBuf::from(std::ffi::OsStr::from_bytes(b"/h/caf\xe9 \xff"));
            let id = id_of(&odd);
            assert!(id.starts_with(BYTES), "{id}");
            assert_eq!(path_of_id(&id).unwrap(), odd);
            let chosen = Chosen {
                added: vec![odd.clone(), plain.clone()],
                off: vec![odd.clone()],
                ..Chosen::default()
            };
            let saved = toml::Value::try_from(&chosen).expect("saved whole");
            assert_eq!(saved.try_into::<Chosen>().unwrap(), chosen);
        }
        // What was saved before, paths as text, is read as it was.
        let before: Chosen = toml::from_str("added = [\"/data/x\"]\nown = false\n").unwrap();
        assert_eq!(before.added, [PathBuf::from("/data/x")]);
        assert!(!before.own && before.bookmarks);
    }

    /// An app with Kept live registered, what the sources offer read by a
    /// counter, and the window's wakes counted.
    struct Fixture {
        _dir: tempfile::TempDir,
        app: Arc<App>,
        kept: Arc<Kept>,
        reads: Arc<std::sync::atomic::AtomicUsize>,
        wakes: Arc<std::sync::atomic::AtomicUsize>,
    }

    fn fixture(engines: Engines) -> Fixture {
        use std::sync::atomic::AtomicUsize;
        let dir = tempfile::tempdir().unwrap();
        let app = App::new(skwir_core::app::AppConfig::ephemeral(dir.path())).unwrap();
        let reads = Arc::new(AtomicUsize::new(0));
        let kept = Kept::reading(
            app.weak(),
            engines,
            Box::new({
                let reads = Arc::clone(&reads);
                move || {
                    reads.fetch_add(1, Ordering::Relaxed);
                    offered()
                }
            }),
        );
        register(&kept, &app);
        let wakes = Arc::new(AtomicUsize::new(0));
        app.set_waker(Some({
            let wakes = Arc::clone(&wakes);
            Arc::new(move || {
                wakes.fetch_add(1, Ordering::Relaxed);
            })
        }));
        Fixture {
            _dir: dir,
            app,
            kept,
            reads,
            wakes,
        }
    }

    /// Until `done`, under a deadline for a hang.
    fn until(done: impl Fn() -> bool) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while !done() {
            assert!(std::time::Instant::now() < deadline, "never done");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    /// Ticks pressed in the window faster than they are saved are kept in
    /// the order pressed, on a worker, and the window is woken to show them.
    #[test]
    fn edits_from_the_window_are_kept_in_the_order_made_and_wake_it() {
        let f = fixture(Engines::None);
        skwir_core::scheduler::mark_window_thread(&f.app.scheduler);
        {
            // Asked while another keeps edits: they wait, then are kept.
            let _keeping = f.kept.keeping.lock();
            for n in 0..21 {
                let args = serde_json::json!({ "id": "/h/Documents", "on": n % 2 == 1 });
                f.app
                    .dispatch(&Invocation::with_args("skwird.kept_tick", &args))
                    .unwrap();
            }
            assert_eq!(f.kept.edits.lock().len(), 21, "queued, none kept here");
        }
        until(|| f.kept.edits.lock().is_empty());
        drop(f.kept.keeping.lock());
        assert_eq!(
            Chosen::of(&f.app).off,
            [PathBuf::from("/h/Documents")],
            "the last pressed holds"
        );
        assert!(
            f.wakes.load(Ordering::Relaxed) > 0,
            "the window is woken once they are kept"
        );
        assert!(f.kept.source.revision() > 0, "the list asked again");
    }

    /// A refresh asked while one runs (a folder added meanwhile) makes it
    /// go once more, however many were asked.
    #[test]
    fn a_refresh_asked_while_one_runs_goes_once_more() {
        let f = fixture(Engines::None);
        {
            // Held, the refresh cannot say what it found until let go.
            let _said = f.kept.said.lock();
            for _ in 0..3 {
                f.kept.refresh();
            }
        }
        until(|| !f.kept.refreshing.lock().running);
        assert_eq!(f.reads.load(Ordering::Relaxed), 2, "once, then once more");
    }

    /// Listing the page asks skwird once every `LISTED_EVERY` at most:
    /// an answer that differs wakes the window, which lists again.
    #[test]
    fn a_listing_asks_skwird_at_most_every_few_seconds() {
        let f = fixture(Engines::None);
        for _ in 0..3 {
            let _ = f.kept.list(&f.app);
            until(|| !f.kept.refreshing.lock().running);
        }
        assert_eq!(f.reads.load(Ordering::Relaxed), 1, "asked once");
        f.kept.refresh();
        until(|| !f.kept.refreshing.lock().running);
        assert_eq!(f.reads.load(Ordering::Relaxed), 2, "an edit's at once");
    }

    /// Registering asks nothing of skwird (ADR-150, 3), and the menu's
    /// entry waits for a user's engine (10).
    #[test]
    fn nothing_is_asked_of_skwird_at_start() {
        let dir = tempfile::tempdir().unwrap();
        let f = fixture(Engines::At(dir.path().join("socket")));
        assert!(!f.kept.refreshing.lock().running, "no refresh");
        assert_eq!(f.reads.load(Ordering::Relaxed), 0, "no source read");
        assert!(!f.kept.user_engine());
        let menu = f
            .app
            .dispatch(&Invocation::with_args(
                "skwird.keep_live_menu",
                &serde_json::json!({ "path": "/h/Documents" }),
            ))
            .unwrap();
        let Outcome::Value(menu) = menu else {
            panic!("an answer");
        };
        assert_eq!(menu["items"], serde_json::json!([]));
    }

    /// Under the system service, which hears every folder, the list is
    /// dimmed (ADR-150, 6); under a user's engine, not.
    #[test]
    fn the_list_is_dimmed_under_the_system_service() {
        let f = fixture(Engines::None);
        f.kept.said(
            Said {
                scope: Some(protocol::Scope::System),
                ..Said::default()
            },
            None,
        );
        assert_eq!(f.kept.list(&f.app)["dimmed"], true);
        f.kept.said(
            Said {
                scope: Some(protocol::Scope::User),
                ..Said::default()
            },
            None,
        );
        assert_eq!(f.kept.list(&f.app)["dimmed"], false);
    }

    /// The commands' errors are words, in the reader's language.
    #[test]
    fn what_a_command_refuses_is_said_in_words() {
        let f = fixture(Engines::None);
        for (command, args) in [
            (
                "skwird.kept_follow",
                serde_json::json!({ "source": "nope", "on": true }),
            ),
            ("skwird.kept_add", serde_json::json!({ "path": "relative" })),
            (
                "skwird.kept_tick",
                serde_json::json!({ "id": "relative", "on": true }),
            ),
            (
                "skwird.kept_remove",
                serde_json::json!({ "id": "relative" }),
            ),
            (
                "skwird.keep_live",
                serde_json::json!({ "path": "relative" }),
            ),
        ] {
            let error = f
                .app
                .dispatch(&Invocation::with_args(command, &args))
                .expect_err(command);
            assert!(
                error
                    .chain()
                    .any(|link| link.downcast_ref::<Words>().is_some()),
                "{command}: {error:#}"
            );
        }
    }
}
