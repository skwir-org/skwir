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

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use skwir_core::app::{App, NotifyLevel};
use skwir_core::command::{Invocation, NoArgs, Outcome, Topic};
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
/// followed, the folders they added, and those they unticked.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Chosen {
    pub own: bool,
    pub bookmarks: bool,
    pub added: Vec<PathBuf>,
    pub off: Vec<PathBuf>,
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
    fn read() -> Offered {
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
    /// A folder added; one of a source, ticked again.
    Add(PathBuf),
    /// A folder the user added, taken off the list.
    Remove(PathBuf),
    Tick(PathBuf, bool),
    Follow(Source, bool),
    /// Kept if it is not, not if it is (the menu's).
    Toggle(PathBuf),
}

impl Edit {
    /// Apply it, whether it changed anything.
    fn apply(&self, chosen: &mut Chosen, offered: &Offered) -> bool {
        let before = chosen.clone();
        let offers = |path: &Path| {
            offered
                .own
                .iter()
                .chain(&offered.bookmarks)
                .any(|(_, p)| p == path)
        };
        match self {
            Edit::Add(path) => {
                chosen.off.retain(|p| p != path);
                if !chosen.added.contains(path) && !offers(path) {
                    chosen.added.push(path.clone());
                }
            }
            Edit::Remove(path) => {
                chosen.added.retain(|p| p != path);
                chosen.off.retain(|p| p != path);
            }
            Edit::Tick(path, true) => chosen.off.retain(|p| p != path),
            Edit::Tick(path, false) => {
                if !chosen.off.contains(path) {
                    chosen.off.push(path.clone());
                }
            }
            Edit::Follow(Source::Own, on) => chosen.own = *on,
            Edit::Follow(Source::Bookmarks, on) => chosen.bookmarks = *on,
            Edit::Follow(Source::Added, _) => {}
            Edit::Toggle(path) => {
                let kept = rows(chosen, offered)
                    .iter()
                    .any(|row| row.kept && &row.path == path);
                let edit = match (kept, chosen.added.contains(path)) {
                    (true, true) => Edit::Remove(path.clone()),
                    (true, false) => Edit::Tick(path.clone(), false),
                    (false, _) => Edit::Add(path.clone()),
                };
                edit.apply(chosen, offered);
            }
        }
        *chosen != before
    }
}

/// The list's keeper: what the user chose, what the sources offer, what
/// skwird said, and the list on the Index page.
pub(crate) struct Kept {
    pub source: RecordSource,
    app: Weak<App>,
    engines: Engines,
    offered: Mutex<Offered>,
    said: Mutex<Said>,
    /// Edits run one at a time.
    saving: Mutex<()>,
    /// A refresh asked and not done.
    asking: AtomicBool,
}

impl Kept {
    pub(crate) fn new(app: Weak<App>, engines: Engines) -> Arc<Kept> {
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
            offered: Mutex::default(),
            said: Mutex::default(),
            saving: Mutex::default(),
            asking: AtomicBool::new(false),
        })
    }

    /// Whether a search found a user's engine, which keeps folders live:
    /// where the menu's entry does something.
    fn user_engine(&self) -> bool {
        self.said.lock().scope == Some(protocol::Scope::User)
    }

    /// Whether `path` is kept live.
    fn keeps(&self, app: &App, path: &Path) -> bool {
        rows(&Chosen::of(app), &self.offered.lock())
            .iter()
            .any(|row| row.kept && row.path == path)
    }

    fn said(&self, said: Said, offered: Option<Offered>) {
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
            self.source.changed();
        }
    }

    /// Make what this client keeps live in `client`'s engine the list's
    /// folders, and keep what it says: a user's engine only. Blocking, on
    /// a worker.
    pub(crate) fn reconcile(&self, client: &mut Client) {
        let scope = client.welcome().scope;
        let offered = Offered::read();
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
        let have: HashSet<PathBuf> = now.folders.iter().map(|k| k.path.to_path()).collect();
        let want: HashSet<&PathBuf> = wanted.iter().collect();
        let add: Vec<&Path> = wanted
            .iter()
            .filter(|path| !have.contains(*path))
            .map(PathBuf::as_path)
            .collect();
        let remove: Vec<&Path> = have
            .iter()
            .filter(|path| !want.contains(path))
            .map(PathBuf::as_path)
            .collect();
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
    /// worker: never starting one. Once at a time.
    pub(crate) fn refresh(self: &Arc<Self>) {
        if self.asking.swap(true, Ordering::AcqRel) {
            return;
        }
        let Some(app) = self.app.upgrade() else {
            self.asking.store(false, Ordering::Release);
            return;
        };
        let this = Arc::clone(self);
        app.scheduler.spawn(
            skwir_core::scheduler::Priority::Background,
            "skwird.kept",
            app.lifetime().child(),
            move || {
                match crate::find(&this.engines, false).map(|find| Client::connect(&find)) {
                    Some(Ok(mut client)) => this.reconcile(&mut client),
                    _ => this.said(Said::default(), Some(Offered::read())),
                }
                this.asking.store(false, Ordering::Release);
            },
        );
    }

    /// Apply `edit` to what the user chose and save it, off the window's
    /// thread, then send the list.
    fn edit(self: &Arc<Self>, edit: Edit) {
        let this = Arc::clone(self);
        let run = move || {
            let Some(app) = this.app.upgrade() else {
                return;
            };
            {
                let _saving = this.saving.lock();
                let mut chosen = Chosen::of(&app);
                if !edit.apply(&mut chosen, &this.offered.lock()) {
                    return;
                }
                let Ok(value) = toml::Value::try_from(&chosen) else {
                    return;
                };
                app.settings.set(CHOSEN, value);
                if let Err(error) = app.settings.save() {
                    app.notify(
                        NotifyLevel::Error,
                        skwir_core::words!(
                            skwird_kept_not_saved =
                                "The folders kept live were not saved: { $error }",
                            error = error,
                            note = "error: why the settings' file could not be written."
                        ),
                    );
                }
            }
            this.source.changed();
            this.refresh();
        };
        match self.app.upgrade() {
            Some(app) if skwir_core::scheduler::on_window_thread() => app.scheduler.spawn(
                skwir_core::scheduler::Priority::Normal,
                "skwird.kept_edit",
                app.lifetime().child(),
                run,
            ),
            _ => run(),
        }
    }

    /// The list as the Index page draws it (`RecordSource`): from memory.
    fn list(self: &Arc<Self>, app: &App) -> serde_json::Value {
        let chosen = Chosen::of(app);
        let offered = self.offered.lock().clone();
        let said = self.said.lock().clone();
        // What skwird says now, and the sources, for the next listing.
        self.refresh();
        let records: Vec<serde_json::Value> = rows(&chosen, &offered)
            .iter()
            .map(|row| {
                let id = row.path.to_string_lossy();
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
        })
    }
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
    /// A folder of the list, as it names it.
    id: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct TickArgs {
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
    path: PathBuf,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct KeepArgs {
    /// A folder; none, the one selected in `pane`.
    #[serde(default)]
    path: Option<PathBuf>,
    #[serde(default)]
    pane: Option<skwir_core::workspace::PaneId>,
}

/// The folder a command is about: given, else the one selected.
fn folder_of(
    app: &App,
    path: Option<PathBuf>,
    pane: Option<skwir_core::workspace::PaneId>,
) -> anyhow::Result<PathBuf> {
    if let Some(path) = path {
        anyhow::ensure!(
            path.is_absolute(),
            "{} is not an absolute path",
            path.display()
        );
        return Ok(path);
    }
    let targets = app.targets(pane, Vec::new())?;
    match targets.as_slice() {
        [one] => Ok(one.path.clone()),
        _ => anyhow::bail!("one folder is to be selected"),
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
                anyhow::ensure!(
                    args.path.is_absolute(),
                    "{} is not an absolute path",
                    args.path.display()
                );
                k.edit(Edit::Add(args.path));
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
                k.edit(Edit::Remove(PathBuf::from(args.id)));
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
                k.edit(Edit::Tick(PathBuf::from(args.id), args.on));
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
                    other => anyhow::bail!("no source {other}: own or bookmarks"),
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
                                Invocation::with_args("skwird.keep_live", &serde_json::json!({ "path": path })),
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
    // What skwird says now, for the list and the menu.
    kept.refresh();
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

    #[test]
    fn edits_add_remove_tick_follow_and_toggle() {
        let offered = offered();
        let mut chosen = Chosen::default();
        assert!(Edit::Add(PathBuf::from("/data/x")).apply(&mut chosen, &offered));
        assert!(
            !Edit::Add(PathBuf::from("/data/x")).apply(&mut chosen, &offered),
            "once"
        );
        assert!(
            !Edit::Add(PathBuf::from("/h/Documents")).apply(&mut chosen, &offered),
            "a source's folder is not added again"
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
}
