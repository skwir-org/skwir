//! The app object: owns the registries, the scheduler, the executor, the
//! workspace, and hosts the built-in commands. Toolkit-agnostic; the UI
//! calls `pump` to apply pending events and `dispatch` for everything else.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::RwLock;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::cache::Cache;
use crate::cancel::CancelToken;
use crate::command::{CommandRegistry, Form, Invocation, NoArgs, Outcome, Topic};
use crate::entry::{Entry, EntryId};
use crate::fs::PathProvider;
use crate::location::{Location, LocationProvider, OpenContext, OpenLocation, Waker};
use crate::op::{self, EntryRef, Executor, OpRegistry, Plan, PlanData};
use crate::os_path::PathArg;
use crate::plugin::PluginHost;
use crate::registry::Registry;
use crate::scheduler::{Priority, Scheduler};
use crate::settings::Settings;
use crate::workspace::{PaneId, Workspace};

/// Severity of a [`Notification`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NotifyLevel {
    Info,
    Warning,
    Error,
}

/// A question for the user, yes or no (`App::ask`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Question {
    pub id: u64,
    /// What is asked, in a line: "An agent asks to run “Undo”".
    pub title: String,
    /// What it would do, in full: the command and its arguments.
    pub detail: String,
    /// Who waits for the answer: what the answers say.
    pub asker: Asker,
}

/// Who waits for a question's answer, which says how it is answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asker {
    /// An agent asking to run a command (ADR-044): Refuse or Allow.
    Agent,
    /// A plan short of room where it writes (ADR-049): Cancel, or copy
    /// anyway.
    Room,
    /// A connection to a server Skwir cannot vouch for (a host key never
    /// seen, a certificate not trusted), or one an agent asked for
    /// (ADR-077): Cancel, or connect.
    Connection,
}

/// The names a plan found taken where it copies or moves (ADR-054), asked
/// all at once (`App::take_clash_questions`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClashQuestion {
    pub id: u64,
    /// The plan's title: "copy to /example/folder".
    pub title: String,
    pub clashes: Vec<op::clash::Clash>,
}

/// What a step asks the user that only they know (an archive's password,
/// ADR-059), until the window takes it (`App::take_secret_questions`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SecretQuestion {
    pub id: u64,
    /// The question: "Password for “photos.zip”".
    pub title: String,
    /// The verb on its button: "Extract".
    pub accept: String,
    /// What the empty field says is asked: "Password", "Passphrase".
    pub placeholder: String,
    /// What keeping the answer means, said under Remember while it is on
    /// (a volume's passphrase, ADR-088).
    pub keeping: Option<String>,
    /// Whether the window offers to keep the answer in the desktop's
    /// keyring (a server's password, ADR-082; a volume's passphrase,
    /// ADR-088): the asker keeps it.
    pub remember: bool,
}

/// What the user answered a [`SecretQuestion`]. No `Debug`: it holds the
/// secret.
pub struct SecretAnswer {
    pub text: String,
    /// The user asked for it to be kept (offered alone).
    pub remember: bool,
}

/// An answer not to be kept: what a script or a test answers.
impl From<String> for SecretAnswer {
    fn from(text: String) -> Self {
        SecretAnswer {
            text,
            remember: false,
        }
    }
}

impl From<&str> for SecretAnswer {
    fn from(text: &str) -> Self {
        SecretAnswer::from(text.to_owned())
    }
}

/// A message from background work for the UI.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Notification {
    pub level: NotifyLevel,
    pub text: String,
    /// Key of the setting that resolves this message, if one does; the UI
    /// offers to open it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setting: Option<String>,
    /// A button that goes with the message: Undo for what a command did
    /// outside the journal (a bookmark), Redo after that undo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<NotifyAction>,
}

/// A button on a [`Notification`]: its label and the command it runs.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct NotifyAction {
    pub label: String,
    pub command: Invocation,
}

/// Construction options.
pub struct AppConfig {
    pub cache_root: PathBuf,
    /// `None` keeps settings in memory (tests).
    pub settings_path: Option<PathBuf>,
    /// Where the window's state is kept apart from the settings (ADR-056);
    /// `None` keeps it with them.
    pub state_path: Option<PathBuf>,
    /// Where what the user made in skwir (saved searches, pinned tags,
    /// tags' colours) is kept apart from the settings (ADR-057); `None`
    /// keeps it with them.
    pub data_path: Option<PathBuf>,
    /// Scheduler threads; 0 picks a default.
    pub workers: usize,
    /// Whether a worker a job holds past `scheduler::STUCK` is stood in
    /// for: always, but in a test that holds its workers to prove work
    /// waits (`Scheduler::without_stand_ins`).
    pub stand_ins: bool,
    /// Entries per `Added` event.
    pub batch_size: usize,
}

impl Default for AppConfig {
    fn default() -> Self {
        AppConfig {
            cache_root: Cache::default_root(),
            settings_path: Some(Settings::default_path()),
            state_path: Settings::default_state_path(),
            data_path: Settings::default_data_path(),
            workers: 0,
            stand_ins: true,
            batch_size: 1024,
        }
    }
}

impl AppConfig {
    /// Everything in memory or under a temp dir: for tests and benches.
    pub fn ephemeral(root: impl Into<PathBuf>) -> Self {
        AppConfig {
            cache_root: root.into().join("cache"),
            settings_path: None,
            state_path: None,
            data_path: None,
            workers: 0,
            stand_ins: true,
            batch_size: 1024,
        }
    }
}

/// The app.
pub struct App {
    pub scheduler: Scheduler,
    pub commands: CommandRegistry,
    pub ops: OpRegistry,
    pub registry: Registry,
    pub plugins: PluginHost,
    pub executor: Arc<Executor>,
    pub settings: Settings,
    pub cache: Cache,
    pub workspace: RwLock<Workspace>,
    /// Entries waiting for a paste, published by the clipboard's owner.
    pub pending: crate::pending::PendingTransfer,
    /// What is playing, published by the player, and what is asked of it.
    pub player: crate::playing::Player,
    /// Counts plugins publish for others to show (`shelf.items`).
    pub tallies: crate::tally::Tallies,
    /// Who runs terminals (the terminal plugin), for the UI's panel.
    pub terminals: crate::terminal::Terminals,
    pub thumbnails: Arc<crate::thumbs::Thumbnails>,
    pub metadata: Arc<crate::metadata::MetadataProviders>,
    /// Fields read when asked, not at hydration (ADR-023).
    pub fields: Arc<crate::fields::Fields>,
    /// Indexes a search asks where files may be first (ADR-026).
    pub hints: Arc<crate::hints::Hints>,
    /// Where files' notes are kept, and how each place is read and
    /// written (ADR-027).
    pub notes: Arc<crate::notes::NoteStores>,
    /// Files beside a file, which go where it goes (ADR-028).
    pub companions: Arc<crate::companions::Companions>,
    /// What copies to and from a remote in batches, plugins' (ADR-078).
    pub transfers: Arc<crate::op::transfer::Transfers>,
    pub previews: Arc<crate::preview::Previews>,
    /// Where map backgrounds come from (the map-tiles plugin, ADR-020).
    pub tiles: Arc<crate::tiles::Tiles>,
    /// The files a pane's Map view places (the geo plugin).
    pub maps: Arc<crate::map_points::MapSources>,
    /// Plans proposed for review (by plugins whose work finishes later, or by
    /// agents). The UI drains them with `take_proposals`.
    proposals: parking_lot::Mutex<Vec<Plan>>,
    /// Questions for the user (`ask`), until the window takes them, and
    /// who waits for each answer.
    questions: parking_lot::Mutex<Vec<Question>>,
    answers: parking_lot::Mutex<HashMap<u64, crossbeam_channel::Sender<bool>>>,
    /// The names plans found taken (ADR-054), until the window takes them,
    /// and who waits for each answer.
    clashes: parking_lot::Mutex<Vec<ClashQuestion>>,
    settled: parking_lot::Mutex<HashMap<u64, crossbeam_channel::Sender<Option<op::clash::Answer>>>>,
    /// What steps ask that only the user knows, until the window takes
    /// them, and who waits for each answer.
    secrets: parking_lot::Mutex<Vec<SecretQuestion>>,
    secret_replies:
        parking_lot::Mutex<HashMap<u64, crossbeam_channel::Sender<Option<SecretAnswer>>>>,
    /// The forms plugins ask the user to fill (ADR-086), until the window
    /// takes them, and who waits for each answer.
    pub(crate) forms: crate::form::Forms,
    /// Messages from background work for the UI to show.
    notifications: parking_lot::Mutex<Vec<Notification>>,
    /// Commands the window carries out (`ui.*`), dispatched from elsewhere
    /// (an agent, a script): they wait for it, which takes them when it
    /// next wakes (`take_front_requests`).
    front: parking_lot::Mutex<Vec<Invocation>>,
    /// Whether the window is up and has shown what it starts on
    /// (`window_shown`), with what waits for it.
    shown: (parking_lot::Mutex<bool>, parking_lot::Condvar),
    /// The tabs closed last, to reopen (`tab.reopen`).
    pub(crate) closed_tabs: parking_lot::Mutex<crate::tabs::Closed>,
    /// Self reference so background work can keep a `Weak<App>`.
    weak: std::sync::Weak<App>,
    /// The one waiter reading the panes' filters again when the first of
    /// them reads otherwise (`roll_filters_over`): when, and its token.
    filter_rollover: parking_lot::Mutex<Option<(std::time::SystemTime, CancelToken)>>,
    providers: RwLock<HashMap<Arc<str>, (Arc<str>, Arc<dyn LocationProvider>)>>,
    /// Shared with every `waker_handle`, so a handle taken before the UI
    /// installs its hook still reaches it.
    waker: Arc<RwLock<Option<Waker>>>,
    batch_size: usize,
    /// Cancelled on drop: stops every background task started by this app.
    lifetime: CancelToken,
}

impl App {
    pub fn new(config: AppConfig) -> anyhow::Result<Arc<Self>> {
        let scheduler = if config.stand_ins {
            Scheduler::new(config.workers)
        } else {
            Scheduler::without_stand_ins(config.workers)
        };
        let lifetime = CancelToken::new();
        let settings = match config.settings_path {
            Some(path) => {
                let mut settings = Settings::load(path);
                if let Some(state) = config.state_path {
                    settings = settings.with_state(state);
                }
                if let Some(data) = config.data_path {
                    settings = settings.with_data(data);
                }
                settings
            }
            None => Settings::in_memory(),
        };
        let cache = Cache::open(config.cache_root);
        let fields = Arc::new(crate::fields::Fields::new(cache.namespace("fields")));
        // One gate for every store a plugin publishes in: a disabled
        // plugin's worker publishing late is refused everywhere.
        let registry = Registry::default();
        let gate = registry.gate();
        let app = Arc::new_cyclic(|weak| App {
            weak: weak.clone(),
            filter_rollover: parking_lot::Mutex::new(None),
            notifications: parking_lot::Mutex::new(Vec::new()),
            closed_tabs: parking_lot::Mutex::default(),
            executor: Executor::new(scheduler.clone()),
            thumbnails: crate::thumbs::Thumbnails::with_lifetime(
                scheduler.clone(),
                lifetime.child(),
            ),
            metadata: Arc::new(crate::metadata::MetadataProviders::gated(Arc::clone(&gate))),
            fields,
            hints: Arc::new(crate::hints::Hints::gated(Arc::clone(&gate))),
            notes: Arc::new(crate::notes::NoteStores::default()),
            companions: Arc::new(crate::companions::Companions::gated(Arc::clone(&gate))),
            transfers: Arc::default(),
            previews: Arc::new(crate::preview::Previews::default()),
            tiles: Arc::new(crate::tiles::Tiles::default()),
            maps: Arc::new(crate::map_points::MapSources::default()),
            proposals: parking_lot::Mutex::new(Vec::new()),
            questions: parking_lot::Mutex::new(Vec::new()),
            answers: parking_lot::Mutex::new(HashMap::new()),
            clashes: parking_lot::Mutex::new(Vec::new()),
            settled: parking_lot::Mutex::new(HashMap::new()),
            secrets: parking_lot::Mutex::new(Vec::new()),
            secret_replies: parking_lot::Mutex::new(HashMap::new()),
            forms: crate::form::Forms::default(),
            front: parking_lot::Mutex::new(Vec::new()),
            shown: (parking_lot::Mutex::new(false), parking_lot::Condvar::new()),
            scheduler,
            commands: CommandRegistry::default(),
            ops: OpRegistry::default(),
            registry,
            plugins: PluginHost::default(),
            settings,
            cache,
            workspace: RwLock::new(Workspace::default()),
            pending: crate::pending::PendingTransfer::gated(Arc::clone(&gate)),
            player: crate::playing::Player::gated(Arc::clone(&gate)),
            tallies: crate::tally::Tallies::gated(gate),
            terminals: crate::terminal::Terminals::default(),
            providers: RwLock::new(HashMap::new()),
            waker: Arc::default(),
            batch_size: config.batch_size,
            lifetime,
        });
        app.register_provider("core", Arc::new(PathProvider));
        op::register_builtin(&app.ops);
        {
            let notes = Arc::clone(&app.notes);
            app.ops
                .register_with("core", crate::notes::SetNotes::KIND, move |v| {
                    let edit: crate::notes::SetNotes = serde_json::from_value(v)?;
                    Ok(Box::new(edit.with_stores(Arc::clone(&notes))))
                });
            let notes = Arc::clone(&app.notes);
            app.ops
                .register_with("core", crate::notes::RestoreNotes::KIND, move |v| {
                    let undo: crate::notes::RestoreNotes = serde_json::from_value(v)?;
                    Ok(Box::new(undo.with_stores(Arc::clone(&notes))))
                });
            app.notes.keep_snapshots(app.cache.namespace("notes"));
            app.executor.set_companions(Arc::clone(&app.companions));
            app.executor.set_transfers(Arc::clone(&app.transfers));
            // What a plan's transfers changed beside a mount (a tool's
            // copies onto a server), the folders showing it read again.
            let told = Arc::downgrade(&app);
            app.executor
                .set_changed_beside(Some(Arc::new(move |folders: &[PathBuf]| {
                    if let Some(app) = told.upgrade() {
                        app.refresh_showing(folders);
                    }
                })));
            // A plan short of room asks the user whether to go on anyway;
            // with no window up, no one is asked, and nothing is copied.
            let asking = Arc::downgrade(&app);
            app.executor.set_room_asker(Some(Arc::new(
                move |title: &str, short: &op::room::Short| match asking.upgrade() {
                    Some(app) if app.window_shown() => {
                        let detail = format!(
                            "{title}: a drive, a network share above all, may say it has \
                             less room than it has. Copy anyway, or cancel and make room."
                        );
                        app.ask_as(Asker::Room, short.said(), detail).1
                    }
                    // Gone, or no window: no one answers, so nothing is
                    // copied.
                    _ => crossbeam_channel::bounded(0).1,
                },
            )));
            // A plan that finds names taken asks the window what becomes
            // of them; with no window up, no one is asked.
            let asking = Arc::downgrade(&app);
            app.executor.set_clash_asker(Some(Arc::new(
                move |title: &str, clashes: &[op::clash::Clash]| {
                    let app = asking.upgrade()?;
                    app.window_shown()
                        .then(|| app.ask_clashes(title, clashes.to_vec()))
                },
            )));
            // A step that needs what only the user knows (an archive's
            // password) asks the window and waits; with no window up, no
            // one is asked, and the step says so.
            let asking = Arc::downgrade(&app);
            app.executor.set_secret_asker(Some(Arc::new(
                move |title: &str, accept: &str, cancel: &crate::CancelToken| {
                    use crossbeam_channel::RecvTimeoutError;
                    let (id, answer) = {
                        let app = asking.upgrade().ok_or(op::Unanswered::NoOne)?;
                        if !app.window_shown() {
                            return Err(op::Unanswered::NoOne);
                        }
                        app.ask_secret(title, accept)
                    };
                    loop {
                        match answer.recv_timeout(std::time::Duration::from_millis(100)) {
                            Ok(said) => {
                                return said.map(|a| a.text).ok_or(op::Unanswered::Declined);
                            }
                            Err(RecvTimeoutError::Disconnected) => {
                                return Err(op::Unanswered::Declined);
                            }
                            Err(RecvTimeoutError::Timeout) if cancel.is_cancelled() => {
                                if let Some(app) = asking.upgrade() {
                                    app.withdraw_secret(id);
                                }
                                return Err(op::Unanswered::Declined);
                            }
                            Err(RecvTimeoutError::Timeout) => {}
                        }
                    }
                },
            )));
            // What a run that ended mid-copy left is swept as this one
            // starts, out of its way.
            let under_way =
                op::staging::UnderWay::new(app.cache.namespace("staging").folder("under-way"));
            app.executor.set_under_way(under_way.clone());
            app.scheduler.spawn(
                Priority::Background,
                "staging.sweep",
                app.lifetime.child(),
                move || under_way.sweep_left(),
            );
        }
        // Where the home trash's links lead, which telling a trashed item
        // asks: looked up here, never on the window's thread.
        app.scheduler.spawn(
            Priority::Background,
            "trash.home",
            app.lifetime.child(),
            op::records::resolve_home_trash,
        );
        builtin::register(&app);
        crate::notes::commands::register(&app);
        // General's own settings before the tabs': a page shows its
        // settings in the order they are declared.
        crate::settings::register(&app);
        crate::tabs::register(&app);
        crate::keymap::declare(&app.settings, app.weak());
        crate::permissions::declare(&app.settings);
        crate::plugin::register(&app);
        crate::playing::register(&app);
        if let Some(problem) = app.settings.take_problem() {
            app.notify(NotifyLevel::Warning, problem);
        }
        // What changes in the settings file while skwir runs (another
        // window, a hand edit, Home Manager) is taken in as it changes.
        if app.settings.path().is_some() {
            let following = Arc::downgrade(&app);
            app.scheduler.spawn(
                Priority::Background,
                "settings-follow",
                app.lifetime.child(),
                move || {
                    if let Some(app) = following.upgrade() {
                        crate::settings::follow(&app);
                    }
                },
            );
        }
        Ok(app)
    }

    /// Register a place notes are kept (ADR-027) and its settings on the
    /// Tags and notes page: whether it is read, and how it is written.
    pub fn register_notes_store(&self, owner: &str, store: Arc<dyn crate::notes::NotesStore>) {
        use crate::notes::{Kept, Write};
        let (id, title) = (store.id(), store.title());
        let holds = store.holds();
        // First what sets it apart: the line of its table says it.
        let holds = if holds.limits.is_empty() {
            "It holds all of a file's notes.".to_owned()
        } else {
            format!("What it cannot hold: {}.", holds.limits)
        };
        let read = crate::notes::read_key(id);
        self.settings.declare(
            crate::settings::SettingSpec::toggle(
                &read,
                "Read",
                &format!(
                    "{holds} Read, what it keeps is shown merged with the other places' notes."
                ),
                store.default_read(),
            )
            .section(title)
            .essential(),
        );
        self.notes.declared(owner, read);
        let mut choices: Vec<(&str, &str)> = vec![
            ("off", Write::Off.title()),
            ("update", Write::Update.title()),
        ];
        // A name is written by a rename of its own, where it holds tags
        // already: never started.
        let said = if store.kept() == Kept::Name {
            "Never; or, where a file's name already holds tags, rename the file as an edit changes them."
        } else {
            if store.kept() == Kept::Beside {
                choices.push(("fallback", Write::Fallback.title()));
            }
            choices.push(("create", Write::Create.title()));
            "Never; only where a file already has notes here; also where a file's volume keeps no attributes; or for every file edited."
        };
        let default = match store.default_write() {
            Write::Off => "off",
            Write::Update => "update",
            Write::Fallback => "fallback",
            Write::Create => "create",
        };
        let write = crate::notes::write_key(id);
        self.settings.declare(
            crate::settings::SettingSpec::choice(&write, "Write", said, default, &choices)
                .section(title)
                .essential(),
        );
        self.notes.declared(owner, write);
        self.notes.register(owner, store);
        self.apply_notes_settings();
    }

    /// Bring the stores' policy in line with the Tags and notes page.
    pub fn apply_notes_settings(&self) {
        use crate::notes::{Policy, Write};
        let mut policy = Policy::default();
        for id in self.notes.ids() {
            let default = self.notes.get(id).is_none_or(|s| s.default_read());
            if self
                .settings
                .get::<bool>(&crate::notes::read_key(id))
                .unwrap_or(default)
            {
                policy.read.push(id.to_owned());
            }
            let write = self
                .settings
                .get::<String>(&crate::notes::write_key(id))
                .and_then(|w| serde_json::from_value::<Write>(serde_json::json!(w)).ok());
            if let Some(write) = write {
                policy.write.push((id.to_owned(), write));
            }
        }
        self.notes.choose(Some(policy));
    }

    /// A token that fires when the app is dropped.
    pub fn lifetime(&self) -> &CancelToken {
        &self.lifetime
    }

    /// Install the UI wake hook: called (from any thread) whenever listing or
    /// executor events are queued, so the UI can `pump` instead of polling.
    pub fn set_waker(&self, waker: Option<Waker>) {
        self.executor.set_waker(waker.clone());
        self.thumbnails.set_waker(waker.clone());
        *self.waker.write() = waker;
    }

    /// A callable wake hook for background work that outlives the borrow of
    /// `App`. It calls the hook installed when it is called, not when it was
    /// taken: plugins take theirs at registration, before the UI installs
    /// one (a no-op while none is).
    pub fn waker_handle(&self) -> Waker {
        let waker = Arc::clone(&self.waker);
        Arc::new(move || {
            let hook = waker.read().clone();
            if let Some(hook) = hook {
                hook();
            }
        })
    }

    /// Invoke the wake hook, if any, with no lock held (as
    /// [`Self::waker_handle`]): a hook may set the waker or wake again.
    pub fn wake(&self) {
        let hook = self.waker.read().clone();
        if let Some(hook) = hook {
            hook();
        }
    }

    // ------------------------------------------------------------ providers

    pub fn register_provider(&self, owner: &str, provider: Arc<dyn LocationProvider>) {
        self.providers
            .write()
            .insert(provider.scheme().into(), (owner.into(), provider));
    }

    pub fn provider(&self, scheme: &str) -> Option<Arc<dyn LocationProvider>> {
        self.providers
            .read()
            .get(scheme)
            .map(|(_, p)| Arc::clone(p))
    }

    /// The schemes a provider opens, the plain paths' among them, sorted.
    pub fn schemes(&self) -> Vec<Arc<str>> {
        let mut schemes: Vec<Arc<str>> = self.providers.read().keys().cloned().collect();
        schemes.sort();
        schemes
    }

    /// Withdraw everything a plugin contributed.
    pub fn unregister_owner(&self, owner: &str) {
        self.commands.unregister_owner(owner);
        self.ops.unregister_owner(owner);
        self.executor.remove_observers(owner);
        self.thumbnails.unregister_owner(owner);
        self.metadata.unregister_owner(owner);
        self.fields.unregister_owner(owner);
        self.hints.unregister_owner(owner);
        self.notes.unregister_owner(owner);
        for key in self.notes.take_declared(owner) {
            self.settings.undeclare(&key);
        }
        self.companions.unregister_owner(owner);
        self.transfers.unregister_owner(owner);
        self.previews.unregister_owner(owner);
        self.pending.unregister_owner(owner);
        self.player.unregister_owner(owner);
        self.tallies.unregister_owner(owner);
        self.terminals.unregister_owner(owner);
        self.tiles.unregister_owner(owner);
        self.maps.unregister_owner(owner);
        self.registry.remove_owner(owner);
        let withdrawn: Vec<Arc<str>> = {
            let mut providers = self.providers.write();
            let withdrawn = providers
                .iter()
                .filter(|(_, (o, _))| &**o == owner)
                .map(|(scheme, _)| Arc::clone(scheme))
                .collect();
            providers.retain(|_, (o, _)| &**o != owner);
            withdrawn
        };
        if !withdrawn.is_empty() {
            self.let_go_of_schemes(&withdrawn);
        }
        for spec in self.settings.schema() {
            if spec.owner() == owner {
                self.settings.undeclare(&spec.key);
            }
        }
    }

    /// Read every pane's filter again with the filter reader there is now
    /// (`Registry::filter_readers`), its text kept: a plugin that reads
    /// filters turned off leaves nothing of its reading in a pane, its
    /// lens gone (ADR-107) and names filtered instead, and turned on reads
    /// them (`PluginHost`).
    pub(crate) fn read_filters_again(&self) {
        let filters: Vec<(PaneId, Option<Arc<str>>)> = self
            .workspace
            .read()
            .panes()
            .iter()
            .filter(|p| p.filter().is_some() || p.lens().is_some())
            .map(|p| (p.id, p.filter().map(Arc::from)))
            .collect();
        if filters.is_empty() {
            self.roll_filters_over();
            return;
        }
        // Read before the workspace's lock: the reader is a plugin's.
        let reader = self.registry.filter_readers.all().into_iter().next();
        let read: Vec<ReadAgain> = filters
            .into_iter()
            .map(|(pane, text)| {
                let read = text
                    .as_deref()
                    .zip(reader.as_ref())
                    .and_then(|(text, reader)| (reader.0)(text));
                (pane, text, read)
            })
            .collect();
        let mut ws = self.workspace.write();
        for (pane, text, read) in read {
            if let Some(p) = ws.pane_mut(pane)
                && p.filter() == text.as_deref()
                && (text.is_some() || reader.is_none())
            {
                p.read_filter_again(read);
            }
        }
        drop(ws);
        self.roll_filters_over();
        self.wake();
    }

    /// Read the panes' filters again when the first of them reads
    /// otherwise (`ReadFilter::next_change`: `modified:today` at midnight,
    /// in a folder kept open as in one the lens is carried to): one waiter,
    /// for the earliest change any pane's filter says, set again whenever a
    /// filter is set and after each reading, and gone with the app. A
    /// search holds its own (the query plugin lists it again).
    fn roll_filters_over(&self) {
        /// How often the waiter looks at the clock: one set forward or back
        /// meanwhile moves the change.
        const CHECK: std::time::Duration = std::time::Duration::from_secs(600);
        let when = self
            .workspace
            .read()
            .panes()
            .iter()
            .filter_map(|p| p.read_filter().and_then(|r| r.next_change))
            .min();
        let mut waiting = self.filter_rollover.lock();
        if waiting.as_ref().map(|(at, _)| *at) == when {
            return;
        }
        if let Some((_, cancel)) = waiting.take() {
            cancel.cancel();
        }
        let Some(when) = when else {
            return;
        };
        let cancel = self.lifetime.child();
        *waiting = Some((when, cancel.clone()));
        drop(waiting);
        let app = self.weak.clone();
        let spawned = std::thread::Builder::new()
            .name("skwir-filter-rollover".into())
            .spawn(move || {
                let signal = cancel.subscribe();
                while let Ok(left) = when.duration_since(std::time::SystemTime::now()) {
                    match signal.receiver().recv_timeout(left.min(CHECK)) {
                        Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                        _ => return,
                    }
                }
                if let Some(app) = app.upgrade()
                    && !cancel.is_cancelled()
                {
                    app.filter_rollover.lock().take();
                    app.read_filters_again();
                }
            });
        if let Err(error) = spawned {
            tracing::warn!("filter rollover: {error}");
        }
    }

    /// Let go of what panes show and keep of `schemes`, which nothing opens
    /// any more (their plugin turned off): a pane showing one goes home, as
    /// one whose volume went does, and what one keeps beside it or opened in
    /// place of one closes, the plugin's work with it. A dormant pane keeps
    /// its history: shown again, it says why it cannot be opened.
    fn let_go_of_schemes(&self, schemes: &[Arc<str>]) {
        let gone = |location: &Location| schemes.iter().any(|s| **s == *location.scheme);
        let showing: Vec<PaneId> = self
            .workspace
            .read()
            .panes()
            .iter()
            .filter(|p| p.location().is_some_and(gone))
            .map(|p| p.id)
            .collect();
        let home = Location::from_path(
            crate::os_path::home().unwrap_or_else(|| PathBuf::from(std::path::MAIN_SEPARATOR_STR)),
        );
        for pane in showing {
            if let Err(error) = self.navigate(pane, &home) {
                tracing::warn!("cannot leave a location no plugin opens: {error:#}");
            }
        }
        for p in self.workspace.write().panes_mut() {
            p.release_where(gone);
        }
        self.wake();
    }

    // ------------------------------------------------------------ locations

    /// The folders opening `location` reads under: a folder itself, else
    /// what its provider says (`LocationProvider::folders_read`).
    pub fn folders_read(&self, location: &Location) -> Vec<PathBuf> {
        match location.path() {
            Some(path) => vec![path],
            None => self
                .provider(&location.scheme)
                .map(|provider| provider.folders_read(location))
                .unwrap_or_default(),
        }
    }

    /// An agent's call running on this thread (`permissions::as_agent`)
    /// opens no location whose folders are private or would reach another
    /// machine, as they resolve: a relative path read in the pane's folder,
    /// `path:` text, a search's `in:` (ADR-044). Anyone else's opens.
    fn agent_may_open(&self, location: &Location) -> anyhow::Result<()> {
        let Some(privacy) = crate::permissions::agent_call() else {
            return Ok(());
        };
        let mut check = privacy.checker();
        self.folders_read(location)
            .iter()
            .try_for_each(|folder| check.admits(folder))
    }

    /// Open a location without attaching it to a pane.
    pub fn open_location(&self, location: &Location) -> anyhow::Result<Arc<dyn OpenLocation>> {
        self.agent_may_open(location)?;
        self.open_admitted(location)
    }

    /// `open_location`, what an agent's call may open checked already.
    fn open_admitted(&self, location: &Location) -> anyhow::Result<Arc<dyn OpenLocation>> {
        let provider = self
            .provider(&location.scheme)
            .ok_or_else(|| anyhow::anyhow!("no provider for scheme `{}`", location.scheme))?;
        provider.open(
            location,
            OpenContext {
                scheduler: self.scheduler.clone(),
                cancel: self.lifetime.child(),
                batch_size: self.batch_size,
                waker: Some(self.waker_handle()),
                metadata: Arc::clone(&self.metadata),
                fields: Arc::clone(&self.fields),
                hints: Arc::clone(&self.hints),
                companions: Arc::clone(&self.companions),
            },
        )
    }

    /// `location` as a trail of places (`LocationProvider::trail`),
    /// answered from its text. A location that is no folder reads as one
    /// place, named as its tab is (`location_title`), when its provider
    /// gives no trail: the address draws it as a place, never as text.
    pub fn trail(&self, location: &Location) -> Option<crate::location::Trail> {
        if location.is_path() {
            return None;
        }
        let provider = self.provider(&location.scheme)?;
        provider.trail(location).or_else(|| {
            Some(crate::location::Trail {
                crumbs: vec![crate::location::Crumb::new(
                    self.location_title(location),
                    location.clone(),
                )],
                ..crate::location::Trail::default()
            })
        })
    }

    /// The bundled glyph that stands for `location`
    /// (`LocationProvider::icon`): `None` for a folder, and for a location
    /// whose provider names none.
    pub fn location_icon(&self, location: &Location) -> Option<&'static str> {
        self.provider(&location.scheme)?.icon(location)
    }

    /// Whether `location` is a search (`LocationProvider::searches`).
    pub fn is_search(&self, location: &Location) -> bool {
        self.provider(&location.scheme)
            .is_some_and(|provider| provider.searches(location))
    }

    /// Whether words typed in the address field can search in `location`,
    /// as their scope (`LocationProvider::searchable`): `false` for a
    /// scheme no provider opens.
    pub fn searchable(&self, location: &Location) -> bool {
        self.provider(&location.scheme)
            .is_some_and(|provider| provider.searchable(location))
    }

    /// How deep words look in `location`, a place that is no folder
    /// (`LocationProvider::depths`, ADR-108): `None` for a folder.
    pub fn depths(&self, location: &Location) -> Option<crate::location::Depths> {
        self.provider(&location.scheme)?.depths(location)
    }

    /// What a pane showing `location` says when it lists nothing
    /// (`LocationProvider::empty_state`).
    pub fn empty_state(&self, location: &Location) -> Option<crate::location::EmptyState> {
        self.provider(&location.scheme)?.empty_state(location)
    }

    /// The tags every entry `location` lists has because it lists them
    /// (`LocationProvider::tags_shown`).
    pub fn tags_shown(&self, location: &Location) -> Vec<String> {
        self.provider(&location.scheme)
            .map(|provider| provider.tags_shown(location))
            .unwrap_or_default()
    }

    /// Where Up goes from `location`: a folder's parent, else the place
    /// before it in its trail. `None` at the top.
    pub fn parent(&self, location: &Location) -> Option<Location> {
        match location.path() {
            Some(path) => path.parent().map(Location::from_path),
            None => self.trail(location)?.parent(),
        }
    }

    /// What `location` is called among the entries of its `parent`: a
    /// folder's name, or a place's label in the place before it in its
    /// trail (as the columns view marks the way down). `None` when its
    /// parent is no place it is listed in (the folder a tag's trail starts
    /// from, the folder a search looks in).
    pub fn name_in_parent(&self, location: &Location) -> Option<std::ffi::OsString> {
        if let Some(path) = location.path() {
            return path.file_name().map(ToOwned::to_owned);
        }
        let trail = self.trail(location)?;
        let [.., _, last] = trail.crumbs.as_slice() else {
            return None;
        };
        Some(last.label.clone().into())
    }

    /// Select the entry of `pane` called `name` as soon as its listing has
    /// it (`Pane::reveal`): right after navigating there.
    pub fn reveal(&self, pane: PaneId, name: std::ffi::OsString) {
        if let Some(p) = self.workspace.write().pane_mut(pane) {
            p.reveal(name);
        }
        self.wake();
    }

    /// What gestures mean in `location` (`LocationProvider::verbs`): a
    /// folder's meanings when its provider says nothing.
    pub fn verbs(&self, location: &Location) -> crate::location::Verbs {
        self.provider(&location.scheme)
            .map(|provider| provider.verbs(location))
            .unwrap_or_default()
    }

    /// What is inside `location`, for the trail's menus: a folder's
    /// folders (hidden ones with `hidden`), after the places the
    /// providers keep of it (`LocationProvider::places_of`: its tags, as
    /// the trail shown has them when it goes on into them, `along`), else
    /// the places its provider says. One answer for both,
    /// so the menus list them one way. Reads: on a worker.
    ///
    /// # Errors
    /// When a folder cannot be read.
    pub fn places_in(
        &self,
        location: &Location,
        along: Option<&Location>,
        hidden: bool,
    ) -> std::io::Result<Vec<crate::location::Crumb>> {
        if let Some(dir) = location.path() {
            let folders = crate::fs::subfolders(&dir, hidden)?;
            // Asked with the lock let go, in the order of their schemes.
            let mut providers: Vec<(Arc<str>, Arc<dyn LocationProvider>)> = self
                .providers
                .read()
                .iter()
                .map(|(scheme, (_, p))| (Arc::clone(scheme), Arc::clone(p)))
                .collect();
            providers.sort_by(|a, b| a.0.cmp(&b.0));
            let mut places: Vec<crate::location::Crumb> = providers
                .iter()
                .flat_map(|(_, provider)| provider.places_of(&dir, along))
                .collect();
            places.extend(folders.into_iter().map(|dir| {
                let name = dir
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                crate::location::Crumb::new(name, Location::from_path(dir))
            }));
            return Ok(places);
        }
        Ok(self
            .provider(&location.scheme)
            .map(|provider| provider.places(location))
            .unwrap_or_default())
    }

    /// Open a location in a pane, replacing what it showed, the pane's lens
    /// brought in (ADR-107). Returns immediately; entries arrive through
    /// `pump`.
    pub fn navigate(&self, pane: PaneId, location: &Location) -> anyhow::Result<()> {
        let location = &self.reached_in(pane, location)?;
        let left = self.cursor_of(pane);
        self.show(pane, location, |p, shown| p.go_on(shown.clone(), left))
    }

    /// Show `location` in `pane` as its own terms say its lens, the pane's
    /// not brought in (ADR-107): a search whose text says the whole lens
    /// (typed in the address field, a pill's choice), anything else with
    /// none (the folder a search's last term taken off leaves). In place
    /// of the step shown with `in_place`, as `navigate_in_place`. A
    /// location the pane shows already is left as it is (ADR-098).
    pub fn navigate_as_is(
        &self,
        pane: PaneId,
        location: &Location,
        in_place: bool,
    ) -> anyhow::Result<()> {
        let location = &self.reached_in(pane, location)?;
        if self.shows_as(pane, location, Lensing::AsIs) {
            return Ok(());
        }
        let as_is = Opening::lensing(Lensing::AsIs);
        if in_place {
            return self.open_in(pane, location, as_is, |p, shown| {
                p.replace_here(shown.clone());
            });
        }
        let left = self.cursor_of(pane);
        self.open_in(pane, location, as_is, |p, shown| {
            p.go_on(shown.clone(), left);
        })
    }

    /// Show `location` in `pane` in place of the step it shows, with no
    /// step of its own: Back goes where the step before led (a search
    /// shown as its words are typed, ADR-098). A location the pane shows
    /// already, as it would show it, is left as it is.
    pub fn navigate_in_place(&self, pane: PaneId, location: &Location) -> anyhow::Result<()> {
        let location = &self.reached_in(pane, location)?;
        if self.shows(pane, location) {
            return Ok(());
        }
        self.show(pane, location, |p, shown| p.replace_here(shown.clone()))
    }

    /// Whether `pane` shows `location` already, as it would show it with
    /// its lens brought in (ADR-107): what a search asked again is
    /// compared with, so one shown is left as it is.
    pub fn shows(&self, pane: PaneId, location: &Location) -> bool {
        self.shows_as(pane, location, Lensing::Brought)
    }

    /// Whether `pane` shows `location` already, as `lensing` shows it.
    fn shows_as(&self, pane: PaneId, location: &Location, lensing: Lensing) -> bool {
        let (shown, _) = self.lensed_in(pane, location, lensing);
        self.pane_location(pane).as_ref() == Some(&shown)
    }

    /// Take back the step `pane` shows, as if it had never been gone to:
    /// the step before is shown again, and the one taken back and those
    /// ahead of it go (Escape in the address field after a search shown as
    /// it was typed), as it was: a search as its text says, anything else
    /// with the pane's lens, which the field puts back as it found it.
    /// Nothing at the first step.
    pub fn take_step_back(&self, pane: PaneId) -> anyhow::Result<()> {
        let (from, before) = {
            let ws = self.workspace.read();
            let p = ws
                .pane(pane)
                .ok_or_else(|| anyhow::anyhow!("no such pane"))?;
            let before = p
                .history_pos
                .checked_sub(1)
                .and_then(|pos| p.history.get(pos).cloned());
            (p.history_pos, before)
        };
        let Some(location) = before else {
            return Ok(());
        };
        let left = self.cursor_of(pane);
        let as_it_was = Opening::lensing(Lensing::AsItWas);
        self.open_in(pane, &location, as_it_was, |p, _| {
            p.take_step_back(from, &location, left);
        })
    }

    /// `location` as `pane` reaches it: a relative path (`./`, `../`) is
    /// the folder shown's, never the process's (shell.md), `..` read as
    /// the trail reads it; refused where the pane shows no folder (a
    /// search, a tag).
    fn reached_in(&self, pane: PaneId, location: &Location) -> anyhow::Result<Location> {
        let Some(relative) = location.path().filter(|path| path.is_relative()) else {
            return Ok(location.clone());
        };
        let here = self.pane_dir(pane).map_err(|_| {
            anyhow::anyhow!(
                "{} is relative, and no folder is shown for it to be in",
                relative.display()
            )
        })?;
        let mut path = here;
        for part in relative.components() {
            match part {
                std::path::Component::CurDir => {}
                std::path::Component::ParentDir => {
                    path.pop();
                }
                part => path.push(part),
            }
        }
        Ok(Location::from_path(path))
    }

    /// Add a step to `pane`'s history for the location it shows, its
    /// listing kept as it is: a link followed to a file beside the one shown
    /// selects it there, and Back comes back to the entry the cursor is on.
    pub fn step_here(&self, pane: PaneId) -> anyhow::Result<()> {
        let left = self.cursor_of(pane);
        let mut ws = self.workspace.write();
        let p = ws
            .pane_mut(pane)
            .ok_or_else(|| anyhow::anyhow!("no such pane"))?;
        let here = p
            .location()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("the pane shows nothing"))?;
        p.go_on(here, left);
        Ok(())
    }

    /// The entry `pane`'s cursor is on, which its history keeps.
    fn cursor_of(&self, pane: PaneId) -> Option<crate::EntryId> {
        self.workspace.read().pane(pane)?.selection.cursor
    }

    /// Show `location` in `pane`, the pane's lens brought in (ADR-107),
    /// then let `then` update its history with the location shown. A
    /// location the pane keeps (a column of its view, the folder it just
    /// left) is adopted with its listing as it stands, kept current by its
    /// watcher, instead of listing it anew.
    pub(crate) fn show(
        &self,
        pane: PaneId,
        location: &Location,
        then: impl FnOnce(&mut crate::workspace::Pane, &Location),
    ) -> anyhow::Result<()> {
        self.open_in(pane, location, Opening::default(), then)
    }

    /// Open again what a dormant pane showed (its tab shown again): as
    /// `show`, but what the pane kept while it slept stays, the folders
    /// opened in place (opened again) and its filter.
    pub(crate) fn wake_pane(
        &self,
        pane: PaneId,
        location: &Location,
        then: impl FnOnce(&mut crate::workspace::Pane, &Location),
    ) -> anyhow::Result<()> {
        let waking = Opening {
            waking: true,
            ..Opening::default()
        };
        self.open_in(pane, location, waking, then)?;
        self.open_expanded(pane);
        Ok(())
    }

    /// Show a step of `pane`'s history again (Back, Forward), then let
    /// `then` update its history with the location shown: as `show`, but
    /// the lens a search's terms held then gives way to the pane's now
    /// (ADR-107: the lens is the pane's, never a step's).
    pub(crate) fn show_again(
        &self,
        pane: PaneId,
        location: &Location,
        then: impl FnOnce(&mut crate::workspace::Pane, &Location),
    ) -> anyhow::Result<()> {
        self.open_in(pane, location, Opening::lensing(Lensing::Again), then)
    }

    /// `location` as it takes `pane`'s lens (ADR-107, `LocationProvider::
    /// lensed`), as `lensing` says: a search holding it in its terms, or
    /// the pane filtered by it, read again. Asked of its provider and the
    /// filter reader with no lock held.
    fn lensed_in(
        &self,
        pane: PaneId,
        location: &Location,
        lensing: Lensing,
    ) -> (Location, crate::workspace::Lensed) {
        use crate::workspace::{Bring, Lensed};
        let carried = self
            .workspace
            .read()
            .pane(pane)
            .and_then(|p| p.lens().cloned());
        let bring = match lensing {
            Lensing::Brought => Bring::Added,
            Lensing::Again => Bring::InPlace,
            Lensing::AsIs | Lensing::AsItWas => Bring::Nothing,
        };
        if let Some((shown, own)) = self
            .provider(&location.scheme)
            .and_then(|provider| provider.lensed(location, carried.as_ref(), bring))
        {
            return (shown, Lensed::Holds(own));
        }
        // Its text says it has none; else the pane's, read as of now (a
        // date says today), and gone with its reader.
        let lens = match lensing {
            Lensing::AsIs => None,
            _ => carried.and_then(|lens| {
                let reader = self.registry.filter_readers.all().into_iter().next()?;
                (reader.0)(&lens.words)?.lens
            }),
        };
        (location.clone(), Lensed::Filters(lens))
    }

    /// Open `location` in `pane`, as `how` says, then let `then` update its
    /// history with the location shown.
    fn open_in(
        &self,
        pane: PaneId,
        location: &Location,
        how: Opening<'_>,
        then: impl FnOnce(&mut crate::workspace::Pane, &Location),
    ) -> anyhow::Result<()> {
        let (location, lensed) = self.lensed_in(pane, location, how.lensing);
        // A location kept open beside the pane is shown without opening it.
        self.agent_may_open(&location)?;
        let kept = self
            .workspace
            .write()
            .pane_mut(pane)
            .ok_or_else(|| anyhow::anyhow!("no such pane"))?
            .take_kept(&location);
        let (open, listing) = match kept {
            Some(kept) => kept,
            None => (
                self.open_admitted(&location)?,
                crate::Listing::with_companions(Arc::clone(&self.companions)),
            ),
        };
        let mut ws = self.workspace.write();
        let p = ws
            .pane_mut(pane)
            .ok_or_else(|| anyhow::anyhow!("no such pane"))?;
        if how.from.is_some_and(|from| p.location() != Some(from)) {
            drop(ws);
            open.close();
            return Ok(());
        }
        if how.waking {
            p.reopen(open, listing, lensed);
        } else {
            p.replace_open(open, listing, lensed);
        }
        then(p, &location);
        drop(ws);
        self.roll_filters_over();
        self.wake();
        Ok(())
    }

    /// Keep `wanted` open beside what `pane` shows, listed and current, and
    /// nothing else: what the pane's view shows aside (the columns of the
    /// folders above). The location just left stays if wanted. Opening only
    /// enqueues, like `open_location`.
    pub fn keep_locations(&self, pane: PaneId, wanted: &[Location]) {
        let missing = {
            let mut ws = self.workspace.write();
            let Some(p) = ws.pane_mut(pane) else { return };
            if wanted.is_empty() && !p.keeps_any() {
                return;
            }
            p.keep_only(wanted)
        };
        for location in missing {
            match self.open_location(&location) {
                Ok(open) => {
                    if let Some(p) = self.workspace.write().pane_mut(pane) {
                        p.add_kept(
                            open,
                            crate::Listing::with_companions(Arc::clone(&self.companions)),
                        );
                    }
                }
                Err(error) => tracing::warn!(%location, "cannot keep: {error:#}"),
            }
        }
    }

    /// Apply pending listing events on every pane. Returns the number applied.
    /// The UI calls this once per frame or on wake-up.
    pub fn pump(&self) -> usize {
        self.pump_report().applied
    }

    /// Like `pump`, and says which panes changed. A pane whose location
    /// said it is reached at another (an address connected, ADR-077)
    /// shows that one in its place.
    pub fn pump_report(&self) -> crate::workspace::PumpReport {
        let report = self.workspace.write().pump_report();
        for redirect in &report.redirects {
            let to = &redirect.to;
            let redirected = Opening {
                from: Some(&redirect.from),
                ..Opening::default()
            };
            let shown = self.open_in(redirect.pane, to, redirected, |p, shown| {
                p.replace_here(shown.clone());
            });
            if let Err(error) = shown {
                tracing::warn!(%to, "cannot show where a location leads: {error:#}");
            }
        }
        report
    }

    /// Wait until `pane`'s listing is complete or failed, at most
    /// [`LISTED_WITHIN`], off the window's thread: what a script or an
    /// agent reads right after showing a location is what it holds.
    /// Without a window nothing else applies what the listing sends, so
    /// this does.
    fn await_listing(&self, pane: PaneId) {
        if crate::scheduler::on_window_thread() {
            return;
        }
        let deadline = std::time::Instant::now() + LISTED_WITHIN;
        loop {
            if !crate::scheduler::has_window() {
                self.pump();
            }
            {
                let ws = self.workspace.read();
                match ws.pane(pane) {
                    Some(p)
                        if !p.is_dormant()
                            && !p.listing.is_complete()
                            && p.listing.error().is_none() => {}
                    _ => return,
                }
            }
            if std::time::Instant::now() > deadline {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    /// What `location` lists, read without showing it (`entries` with a
    /// `location`): a script's or an agent's look at a folder, which
    /// leaves the window as the user left it. Its entries by name, the
    /// sizes and times of the page answered, without the private ones
    /// for an agent's call. It waits: never on the window's thread.
    fn listed_elsewhere(
        &self,
        location: &Location,
        hidden: bool,
        offset: usize,
        limit: usize,
        info: impl Fn(EntryId, &crate::entry::Entry, Option<PathBuf>, bool) -> EntryInfo,
    ) -> anyhow::Result<Outcome> {
        use crate::location::{Listing, SortKey, SortSpec};
        anyhow::ensure!(
            !crate::scheduler::on_window_thread(),
            "a location is listed without showing it for a script or an agent, never in the window"
        );
        let mut open = self.open_location(location)?;
        let deadline = std::time::Instant::now() + 3 * LISTED_WITHIN;
        let mut listing = Listing::default();
        while !listing.is_complete() && listing.error().is_none() {
            let Ok(event) = open.events().recv_deadline(deadline) else {
                break;
            };
            listing.apply(event);
            // An address connected lists the folder it is (ADR-077).
            if let Some(to) = listing.take_redirect() {
                open.close();
                open = self.open_location(&to)?;
                listing = Listing::default();
            }
        }
        if let Some(error) = listing.error() {
            let error = error.to_owned();
            open.close();
            anyhow::bail!("{error}");
        }
        let privacy = crate::permissions::agent_call();
        let mut check = privacy.as_ref().map(crate::permissions::Privacy::checker);
        let order: Vec<EntryId> = listing
            .sorted(SortSpec {
                key: SortKey::Name,
                ascending: true,
                show_hidden: hidden,
            })
            .iter()
            .copied()
            .filter(|&id| match &mut check {
                Some(check) => !open.path_of(id).is_some_and(|path| check.holds(&path)),
                None => true,
            })
            .collect();
        let page: Vec<EntryId> = order.iter().skip(offset).take(limit).copied().collect();
        // Sizes and times are read when asked for: those of the page.
        open.hydrate(&page, Priority::Normal);
        let hydrated_by = std::time::Instant::now() + LISTED_WITHIN;
        while page
            .iter()
            .any(|&id| listing.get(id).is_some_and(|e| !e.metadata.is_hydrated()))
        {
            let Ok(event) = open.events().recv_deadline(hydrated_by) else {
                break;
            };
            listing.apply(event);
        }
        let entries: Vec<EntryInfo> = page
            .iter()
            .filter_map(|&id| Some(info(id, listing.get(id)?, open.path_of(id), false)))
            .collect();
        let complete = listing.is_complete();
        open.close();
        let mut answer = serde_json::json!({
            "location": location,
            "complete": complete,
            "total": order.len(),
            "entries": entries,
        });
        say_as_of(&mut answer, listing.as_of());
        Ok(Outcome::Value(answer))
    }

    /// Block until the active pane's listing is complete (tests and CLI only).
    pub fn pump_until_complete(&self, pane: PaneId, timeout: std::time::Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            self.pump();
            let ws = self.workspace.read();
            let Some(p) = ws.pane(pane) else { return false };
            if p.listing.is_complete() || p.listing.error().is_some() {
                return p.listing.is_complete();
            }
            drop(ws);
            if std::time::Instant::now() > deadline {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    // ------------------------------------------------------------ commands

    pub fn dispatch(&self, inv: &Invocation) -> anyhow::Result<Outcome> {
        self.commands.dispatch(inv, self)
    }

    /// [`App::dispatch`] with typed arguments, which a command registered
    /// with their type takes as they are (`CommandRegistry::dispatch_args`):
    /// what the window's views send by the thousand (`HydrateArgs`,
    /// `SelectArgs`).
    ///
    /// # Errors
    /// As [`App::dispatch`].
    pub fn dispatch_args<A: Serialize + Send + 'static>(
        &self,
        name: &str,
        args: A,
    ) -> anyhow::Result<Outcome> {
        self.commands.dispatch_args(name, args, self)
    }

    /// Execute a plan in the background.
    pub fn apply(&self, plan: Plan) {
        self.executor.execute(plan, self.lifetime.child());
    }

    /// Apply `plan` only if none of its steps conflicts, checked when its
    /// turn comes (see [`Executor::execute_whole`]): what a user's single
    /// action and a reviewed proposal need. Never blocks the caller.
    pub fn apply_whole(&self, plan: Plan) {
        self.executor.execute_whole(plan, self.lifetime.child());
    }

    /// Leave a command the window carries out (`ui.*`) for it, and wake
    /// it: how one dispatched from elsewhere reaches the window.
    pub fn request_front(&self, inv: Invocation) {
        self.front.lock().push(inv);
        self.wake();
    }

    /// The window's commands asked for since the last call, oldest first.
    pub fn take_front_requests(&self) -> Vec<Invocation> {
        std::mem::take(&mut *self.front.lock())
    }

    /// The window is up and shows what it starts on (the location it was
    /// given, the tabs it had): what a script or an agent asks of it from
    /// now on comes after, never undone by the window's own start.
    pub fn set_window_shown(&self) {
        *self.shown.0.lock() = true;
        self.shown.1.notify_all();
    }

    /// Whether the window is up (`set_window_shown`): someone can be
    /// asked.
    pub fn window_shown(&self) -> bool {
        *self.shown.0.lock()
    }

    /// Wait at most `timeout` for the window to be shown
    /// (`set_window_shown`); whether it is.
    pub fn wait_window_shown(&self, timeout: std::time::Duration) -> bool {
        let mut shown = self.shown.0.lock();
        if !*shown {
            self.shown.1.wait_for(&mut shown, timeout);
        }
        *shown
    }

    /// Offer a plan for the user to review. Never applies anything; wakes the UI.
    pub fn propose(&self, plan: Plan) {
        self.proposals.lock().push(plan);
        self.wake();
    }

    /// Offer `plan` for review, and learn what the user made of it: how it
    /// applied, or that it was dismissed (`Verdict`). An agent's plan,
    /// whose tool answers once the user has (ADR-043). The receiver ends
    /// unanswered if the window closes first.
    pub fn propose_awaiting(&self, plan: Plan) -> crossbeam_channel::Receiver<crate::op::Verdict> {
        let (reply, verdict) = crossbeam_channel::bounded(1);
        self.executor.watch(plan.id, reply);
        self.propose(plan);
        verdict
    }

    /// The user dismissed the proposal `id` (the review's Cancel): the one
    /// waiting for it, if any, hears so.
    pub fn dismissed(&self, id: crate::op::PlanId) {
        self.executor.dismiss(id);
    }

    /// Ask the user something the window shows like a proposal, Allow or
    /// Deny: an agent's call of a command that acts without a plan
    /// (ADR-044). The receiver hears the answer; it ends unanswered if
    /// the window closes, and whoever stops waiting `withdraw`s it.
    pub fn ask(
        &self,
        title: impl Into<String>,
        detail: impl Into<String>,
    ) -> (u64, crossbeam_channel::Receiver<bool>) {
        self.ask_as(Asker::Agent, title, detail)
    }

    /// [`App::ask`], for `asker`.
    pub fn ask_as(
        &self,
        asker: Asker,
        title: impl Into<String>,
        detail: impl Into<String>,
    ) -> (u64, crossbeam_channel::Receiver<bool>) {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let (reply, answer) = crossbeam_channel::bounded(1);
        self.answers.lock().insert(id, reply);
        self.questions.lock().push(Question {
            id,
            title: title.into(),
            detail: detail.into(),
            asker,
        });
        self.wake();
        (id, answer)
    }

    /// Ask the window what becomes of the names the plan titled so found
    /// taken: the receiver hears the answer, `None` for Cancel; it ends
    /// unanswered if the window closes.
    pub fn ask_clashes(
        &self,
        title: impl Into<String>,
        clashes: Vec<op::clash::Clash>,
    ) -> crossbeam_channel::Receiver<Option<op::clash::Answer>> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let (reply, answer) = crossbeam_channel::bounded(1);
        self.settled.lock().insert(id, reply);
        self.clashes.lock().push(ClashQuestion {
            id,
            title: title.into(),
            clashes,
        });
        self.wake();
        answer
    }

    /// The names found taken since the last call, oldest first.
    pub fn take_clash_questions(&self) -> Vec<ClashQuestion> {
        std::mem::take(&mut *self.clashes.lock())
    }

    /// The user answered the clashes `id`, `None` for Cancel: whether the
    /// plan still waited.
    pub fn answer_clashes(&self, id: u64, answer: Option<op::clash::Answer>) -> bool {
        let reply = self.settled.lock().remove(&id);
        reply.is_some_and(|reply| reply.send(answer).is_ok())
    }

    /// Ask the window for what only the user knows (an archive's
    /// password): `title` the question, `accept` its button's verb. The
    /// receiver hears the answer, `None` for Cancel; it ends unanswered if
    /// the window closes, and whoever stops waiting `withdraw_secret`s it.
    pub fn ask_secret(
        &self,
        title: impl Into<String>,
        accept: impl Into<String>,
    ) -> (u64, crossbeam_channel::Receiver<Option<SecretAnswer>>) {
        self.ask_secret_as(SecretQuestion {
            id: 0,
            title: title.into(),
            accept: accept.into(),
            placeholder: "Password".to_owned(),
            keeping: None,
            remember: false,
        })
    }

    /// [`App::ask_secret`] for a server's password, which the window offers
    /// to keep in the desktop's keyring (ADR-082): the answer says whether
    /// the user asked for it, and the asker keeps it once it served.
    pub fn ask_password(
        &self,
        title: impl Into<String>,
        accept: impl Into<String>,
    ) -> (u64, crossbeam_channel::Receiver<Option<SecretAnswer>>) {
        self.ask_secret_as(SecretQuestion {
            id: 0,
            title: title.into(),
            accept: accept.into(),
            placeholder: "Password".to_owned(),
            keeping: None,
            remember: true,
        })
    }

    /// [`App::ask_secret`] for an encrypted volume's passphrase, which the
    /// window offers to keep in the desktop's keyring (ADR-088) where
    /// `keeping` says what keeping it means (under Remember, while it is
    /// on): the answer says whether the user asked for it, and the asker
    /// keeps it once it unlocked the volume.
    pub fn ask_passphrase(
        &self,
        title: impl Into<String>,
        accept: impl Into<String>,
        keeping: Option<String>,
    ) -> (u64, crossbeam_channel::Receiver<Option<SecretAnswer>>) {
        self.ask_secret_as(SecretQuestion {
            id: 0,
            title: title.into(),
            accept: accept.into(),
            placeholder: "Passphrase".to_owned(),
            remember: keeping.is_some(),
            keeping,
        })
    }

    /// Ask `question`, numbered here.
    fn ask_secret_as(
        &self,
        question: SecretQuestion,
    ) -> (u64, crossbeam_channel::Receiver<Option<SecretAnswer>>) {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let (reply, answer) = crossbeam_channel::bounded(1);
        self.secret_replies.lock().insert(id, reply);
        self.secrets.lock().push(SecretQuestion { id, ..question });
        self.wake();
        (id, answer)
    }

    /// What steps asked of the user since the last call, oldest first.
    pub fn take_secret_questions(&self) -> Vec<SecretQuestion> {
        std::mem::take(&mut *self.secrets.lock())
    }

    /// The user answered the question `id`, `None` for Cancel: whether
    /// the step still waited.
    pub fn answer_secret(&self, id: u64, answer: Option<SecretAnswer>) -> bool {
        let reply = self.secret_replies.lock().remove(&id);
        reply.is_some_and(|reply| reply.send(answer).is_ok())
    }

    /// Whether the step that asked `id` still waits for its answer: one
    /// cancelled meanwhile withdrew it.
    pub fn secret_waits(&self, id: u64) -> bool {
        self.secret_replies.lock().contains_key(&id)
    }

    /// The step that asked `id` stopped waiting (its plan cancelled): an
    /// answer goes nowhere, and a question not taken yet is not shown.
    pub fn withdraw_secret(&self, id: u64) {
        self.secret_replies.lock().remove(&id);
        self.secrets.lock().retain(|q| q.id != id);
        // The window closes its prompt if it shows it already.
        self.wake();
    }

    /// Questions asked since the last call, oldest first.
    pub fn take_questions(&self) -> Vec<Question> {
        std::mem::take(&mut *self.questions.lock())
    }

    /// The user answered the question `id`: whether anyone still waited.
    pub fn answer(&self, id: u64, allowed: bool) -> bool {
        let reply = self.answers.lock().remove(&id);
        reply.is_some_and(|reply| reply.send(allowed).is_ok())
    }

    /// Whoever asked `id` stopped waiting: an answer goes nowhere, and a
    /// question the window has not taken yet is not shown.
    pub fn withdraw(&self, id: u64) {
        self.answers.lock().remove(&id);
        self.questions.lock().retain(|q| q.id != id);
        // The window lets go of one it shows already.
        self.wake();
    }

    /// Whether anyone still waits for the answer to `id`.
    pub fn awaited(&self, id: u64) -> bool {
        self.answers.lock().contains_key(&id)
    }

    /// The folders whose files never leave this machine, as the settings
    /// say now (`permissions::Privacy`), a path on a remote looked at in
    /// its lane.
    pub fn privacy(&self) -> crate::permissions::Privacy {
        crate::permissions::Privacy::of(&self.settings).looked_at_by(self.scheduler.clone())
    }

    /// The entries of `pane` an agent's call must not count (ADR-044):
    /// while one runs on this thread (`permissions::as_agent`), those the
    /// pane lists, shows or holds selected that are in a private folder; none
    /// otherwise, at no cost. A command that answers what it counted or
    /// gathered of a pane's entries (how many, their tags, their dates)
    /// leaves these out. A file whose path the pane cannot say is left
    /// out too: what cannot be checked is never counted public. A place (a
    /// tag) is no file and is counted. It reads markers from the disk,
    /// outside the workspace's lock: ask before taking it.
    pub fn withheld(&self, pane: PaneId) -> std::collections::HashSet<EntryId> {
        let Some(privacy) = crate::permissions::agent_call() else {
            return std::collections::HashSet::new();
        };
        let paths: Vec<(EntryId, Option<PathBuf>)> = {
            let ws = self.workspace.read();
            let Some(p) = ws.pane(pane) else {
                return std::collections::HashSet::new();
            };
            p.order()
                .iter()
                .copied()
                .chain(p.listing.ids())
                .chain(p.selection.iter())
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .filter(|id| !id.is_place())
                .map(|id| (id, p.path_of(id)))
                .collect()
        };
        let mut check = privacy.checker();
        paths
            .into_iter()
            .filter(|(_, path)| path.as_ref().is_none_or(|path| check.holds(path)))
            .map(|(id, _)| id)
            .collect()
    }

    /// Plans proposed since the last call, oldest first.
    pub fn take_proposals(&self) -> Vec<Plan> {
        std::mem::take(&mut *self.proposals.lock())
    }

    /// Number of plans waiting for review.
    pub fn pending_proposals(&self) -> usize {
        self.proposals.lock().len()
    }

    /// A weak handle for background work that outlives a command handler's
    /// `&App` (upgrade before use; `None` once the app is gone).
    pub fn weak(&self) -> std::sync::Weak<App> {
        self.weak.clone()
    }

    /// Queue a message for the UI (errors from background work, results
    /// worth a status line) and wake it.
    pub fn notify(&self, level: NotifyLevel, text: impl Into<String>) {
        self.notifications.lock().push(Notification {
            level,
            text: text.into(),
            setting: None,
            action: None,
        });
        self.wake();
    }

    /// Like [`App::notify`], with a button that runs `command`.
    pub fn notify_with_action(
        &self,
        level: NotifyLevel,
        text: impl Into<String>,
        label: impl Into<String>,
        command: Invocation,
    ) {
        self.notifications.lock().push(Notification {
            level,
            text: text.into(),
            setting: None,
            action: Some(NotifyAction {
                label: label.into(),
                command,
            }),
        });
        self.wake();
    }

    /// Say what a user's edit did, with `inverse` as its Undo, or its Redo
    /// when the edit was itself an undo (`undo`): how an edit made outside a
    /// plan answers (a bookmark, a saved search, a pinned tag, a default
    /// application), `inverse` carrying `!undo` for the next turn.
    pub fn notify_undoable(&self, text: impl Into<String>, undo: bool, inverse: Invocation) {
        self.notify_with_action(
            NotifyLevel::Info,
            text,
            if undo { "Redo" } else { "Undo" },
            inverse,
        );
    }

    /// Like [`App::notify`], pointing at the setting that fixes the problem.
    pub fn notify_about_setting(&self, level: NotifyLevel, text: impl Into<String>, key: &str) {
        self.notifications.lock().push(Notification {
            level,
            text: text.into(),
            setting: Some(key.into()),
            action: None,
        });
        self.wake();
    }

    /// Messages queued since the last call, oldest first.
    pub fn take_notifications(&self) -> Vec<Notification> {
        std::mem::take(&mut *self.notifications.lock())
    }

    /// What a command acts on: the entries it names (a surface acting on
    /// what it showed: Properties, the tag editor; a script or an agent, which
    /// names files rather than select them, ADR-043), else the selection
    /// of `pane`, the active one when `None`. Every command that acts on
    /// entries asks this, so each can be given them. None at all is
    /// [`NothingSelected`](crate::command::NothingSelected), which a
    /// surface answers as the hint it is.
    ///
    /// # Errors
    /// Nothing named or selected, or an entry named by a relative path or
    /// one that leads nowhere (`EntryArg::into_ref`).
    pub fn targets(
        &self,
        pane: Option<PaneId>,
        entries: Vec<op::EntryArg>,
    ) -> anyhow::Result<Vec<EntryRef>> {
        let entries = if entries.is_empty() {
            self.selected_refs(self.pane_or_active(pane))
        } else {
            op::EntryArg::refs(entries)?
        };
        if entries.is_empty() {
            Err(crate::command::NothingSelected.into())
        } else {
            Ok(entries)
        }
    }

    /// [`App::targets`], each with the name that says its type, as
    /// [`App::selected_typed`] gives the selection's: an entry the pane
    /// lists is typed from memory, any other by what it leads to
    /// (`fs::content_path`, which reads: one named by a script or an
    /// agent, on its thread).
    ///
    /// # Errors
    /// As [`App::targets`].
    pub fn targets_typed(
        &self,
        pane: Option<PaneId>,
        entries: Vec<op::EntryArg>,
    ) -> anyhow::Result<Vec<(EntryRef, Option<std::ffi::OsString>)>> {
        let pane = self.pane_or_active(pane);
        if entries.is_empty() {
            let typed = self.selected_typed(pane);
            if typed.is_empty() {
                return Err(crate::command::NothingSelected.into());
            }
            return Ok(typed);
        }
        let refs = op::EntryArg::refs(entries)?;
        let listed: Vec<Option<Option<std::ffi::OsString>>> = {
            let ws = self.workspace.read();
            let p = ws.pane(pane);
            refs.iter()
                .map(|r| {
                    let e = p?.entry_at(r.id, &r.path)?;
                    Some(e.inode_type().is_none().then(|| e.type_name().to_owned()))
                })
                .collect()
        };
        Ok(refs
            .into_iter()
            .zip(listed)
            .map(|(r, listed)| {
                let name = listed.unwrap_or_else(|| {
                    crate::fs::content_path(&r.path)
                        .and_then(|c| c.file_name().map(std::ffi::OsStr::to_os_string))
                });
                (r, name)
            })
            .collect())
    }

    /// What deleting the selection of `pane` would leave on the disk,
    /// its files having names elsewhere (`entry::kept_by_other_names`),
    /// from what its listings know: `None` while one is not stat-ed.
    pub fn selection_kept(&self, pane: PaneId) -> Option<crate::entry::Kept> {
        let ws = self.workspace.read();
        let p = ws.pane(pane)?;
        crate::entry::kept_by_other_names(p.selection.iter().filter_map(|id| p.entry(id)))
    }

    /// Entry references for the current selection of a pane, in selection order.
    pub fn selected_refs(&self, pane: PaneId) -> Vec<EntryRef> {
        let refs = {
            let ws = self.workspace.read();
            let Some(p) = ws.pane(pane) else {
                return Vec::new();
            };
            p.selection
                .iter()
                .filter_map(|id| {
                    let only = p.entry(id).is_some_and(Entry::is_only_name);
                    Some(EntryRef::new(id.file, p.path_of(id)?).with_only_name(only))
                })
                .collect()
        };
        Self::unwithheld(refs, |r| &r.path)
    }

    /// `items` without those in a private folder while an agent's call
    /// runs on this thread (`permissions::as_agent`): what the selection
    /// hands a command is what an agent may name. As is, at no cost,
    /// otherwise. Reads markers: never under the workspace's lock.
    fn unwithheld<T>(mut items: Vec<T>, path: impl Fn(&T) -> &Path) -> Vec<T> {
        if let Some(privacy) = crate::permissions::agent_call() {
            let mut check = privacy.checker();
            items.retain(|item| !check.holds(path(item)));
        }
        items
    }

    /// `selected_refs`, each with the name that says its type
    /// (`Entry::type_name`: a link's target's), as the listings know it:
    /// for a caller that may not touch the disk. `None` for what its name
    /// does not type (`Entry::inode_type`: a folder, a pipe, a device, a
    /// link that leads nowhere). A worker holding only a path asks
    /// `fs::content_path`.
    pub fn selected_typed(&self, pane: PaneId) -> Vec<(EntryRef, Option<std::ffi::OsString>)> {
        let typed = {
            let ws = self.workspace.read();
            let Some(p) = ws.pane(pane) else {
                return Vec::new();
            };
            p.selection
                .iter()
                .filter_map(|id| {
                    let path = p.path_of(id)?;
                    let entry = p.entry(id);
                    let type_name = match entry {
                        Some(e) => e.inode_type().is_none().then(|| e.type_name().to_owned()),
                        None => path.file_name().map(std::ffi::OsStr::to_os_string),
                    };
                    let only = entry.is_some_and(Entry::is_only_name);
                    Some((EntryRef::new(id.file, path).with_only_name(only), type_name))
                })
                .collect()
        };
        Self::unwithheld(typed, |(r, _)| &r.path)
    }

    /// Open the listings of the folders `pane` opened in place that are not
    /// open yet. Opening only enqueues, like `open_location`.
    pub fn open_expanded(&self, pane: PaneId) {
        let missing = match self.workspace.read().pane(pane) {
            Some(p) => p.unopened(),
            None => return,
        };
        for location in missing {
            match self.open_location(&location) {
                Ok(open) => {
                    if let Some(p) = self.workspace.write().pane_mut(pane) {
                        p.add_kept(
                            open,
                            crate::Listing::with_companions(Arc::clone(&self.companions)),
                        );
                    }
                }
                Err(error) => tracing::warn!(%location, "cannot open in place: {error:#}"),
            }
        }
    }

    /// Let go of everything under `path` before its volume is unmounted: a
    /// pane that shows a folder there goes to `to`, and every folder there
    /// that a pane keeps beside the one it shows, opened in place or just
    /// left, closes. An open listing holds its directory, and the kernel
    /// refuses to unmount a volume anything holds. The listings let go of
    /// their directories on a worker, a moment later; a dormant pane holds
    /// nothing.
    pub fn release(&self, path: &Path, to: &Location) -> anyhow::Result<()> {
        // Whatever its scheme: a trashed folder there is listed by the
        // folder's own listing (`OpenLocation::holds`).
        let showing: Vec<PaneId> = self
            .workspace
            .read()
            .panes()
            .iter()
            .filter(|p| {
                p.open
                    .as_ref()
                    .and_then(|open| open.holds())
                    .is_some_and(|held| held.starts_with(path))
            })
            .map(|p| p.id)
            .collect();
        for pane in showing {
            self.navigate(pane, to)?;
        }
        for p in self.workspace.write().panes_mut() {
            p.release_under(path);
        }
        self.wake();
        Ok(())
    }

    /// Close every folder `pane` opened in place: what a view that does not
    /// draw them asks (only the tree does).
    pub fn collapse_all(&self, pane: PaneId) {
        let changed = self
            .workspace
            .write()
            .pane_mut(pane)
            .is_some_and(crate::workspace::Pane::collapse_all);
        if changed {
            self.wake();
        }
    }

    /// `pane`, or the active pane when `None`: what a command's optional
    /// `pane` argument means. It reads the workspace: under a guard of it
    /// already, ask the guard's `active_id` instead (a second read waits
    /// behind any writer queued between the two, forever).
    pub fn pane_or_active(&self, pane: Option<PaneId>) -> PaneId {
        pane.unwrap_or_else(|| self.workspace.read().active_id())
    }

    /// The location a pane shows, if it shows one.
    pub fn pane_location(&self, pane: PaneId) -> Option<Location> {
        self.workspace.read().pane(pane)?.location().cloned()
    }

    /// The directory a pane shows, or an error a command can return as is.
    pub fn pane_dir(&self, pane: PaneId) -> anyhow::Result<PathBuf> {
        let ws = self.workspace.read();
        let p = ws
            .pane(pane)
            .ok_or_else(|| anyhow::anyhow!("no such pane"))?;
        p.location()
            .and_then(Location::path)
            .ok_or_else(|| anyhow::anyhow!("pane is not showing a directory"))
    }

    /// The folder work on `pane` happens in (a terminal, a command): the
    /// one it shows, else, for a search or the shelf, the one holding the
    /// selected entries (the nearest folder they share). An error a
    /// command can return as is when there is neither.
    pub fn work_dir(&self, pane: PaneId) -> anyhow::Result<PathBuf> {
        if let Ok(dir) = self.pane_dir(pane) {
            return Ok(dir);
        }
        common_parent(self.selected_refs(pane).iter().map(|r| r.path.as_path()))
            .ok_or_else(|| anyhow::anyhow!("select something, or open a folder"))
    }

    /// A pane's open location and a snapshot taken with it, for what is
    /// asked of it without the workspace lock (`refresh`, `hydrate`: see
    /// `OpenLocation`).
    fn with_open<T>(
        &self,
        pane: PaneId,
        snapshot: impl FnOnce(&crate::workspace::Pane) -> T,
    ) -> Option<(Arc<dyn OpenLocation>, T)> {
        let ws = self.workspace.read();
        let p = ws.pane(pane)?;
        Some((Arc::clone(p.open.as_ref()?), snapshot(p)))
    }
}

impl Drop for App {
    fn drop(&mut self) {
        self.lifetime.cancel();
    }
}

// ================================================================ built-in commands

/// The nearest folder holding every one of `paths`; `None` for none.
pub fn common_parent<'a>(paths: impl IntoIterator<Item = &'a Path>) -> Option<PathBuf> {
    let mut common: Option<PathBuf> = None;
    for path in paths {
        let parent = path.parent()?;
        common = Some(match common {
            None => parent.to_path_buf(),
            Some(so_far) => so_far
                .components()
                .zip(parent.components())
                .take_while(|(a, b)| a == b)
                .map(|(a, _)| a)
                .collect(),
        });
    }
    common.filter(|c| !c.as_os_str().is_empty())
}

/// Arguments shared by commands that act on a pane.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct PaneArg {
    /// The pane to act on; the active pane when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<PaneId>,
}

/// `filter`: what the names shown must hold.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct FilterArgs {
    /// The pane to act on; the active pane when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<PaneId>,
    /// Part of a name; empty or omitted shows every entry again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// `back` and `forward`: how far through the pane's history.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct HistoryArgs {
    /// The pane to act on; the active pane when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<PaneId>,
    /// How many locations to go through at once; one when omitted (a
    /// history menu jumps several).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steps: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NavigateArgs {
    /// A path or `scheme:rest`; a path that is not UTF-8 as `{"bytes": [..]}`.
    pub location: PathArg,
    #[serde(default)]
    pub pane: Option<PaneId>,
    /// An entry of it to select once listed, by its name: what was clicked
    /// in the column of the folder it is in.
    #[serde(default, with = "crate::os_path::opt")]
    #[schemars(with = "Option<PathArg>")]
    pub select: Option<std::ffi::OsString>,
    /// Put it in place of the step the pane shows, with no step of its
    /// own: Back goes where the step before led. A location already shown
    /// is then left as it is.
    #[serde(default)]
    pub in_place: bool,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SelectMode {
    #[default]
    Replace,
    Add,
    Toggle,
    /// Replace the selection with the range from the anchor to the given id,
    /// in the pane's current sort order (shift-click).
    RangeTo,
    /// Add the range from the anchor to the given id to the selection
    /// (ctrl+shift-click).
    AddRange,
    /// Move the cursor to the given id and leave the selection as it is
    /// (ctrl+arrows), so the keyboard can pick items that are not adjacent.
    Cursor,
    /// Move the cursor to the given id: a selection that follows the
    /// cursor moves with it, a picked one stays, a sweep covers up to it
    /// (a key scheme's moves, ADR-024).
    Follow,
    /// Pick the given ids, or unpick those picked (yazi's mark): a
    /// selection that followed the cursor is not a pick.
    Pick,
    /// Add the given ids and take out those in `removed`, leaving the rest
    /// as it is: what changed of a selection being drawn (a rubber band),
    /// rather than all of it again.
    Change,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SortArgs {
    #[serde(default)]
    pub key: Option<crate::location::SortKey>,
    #[serde(default)]
    pub ascending: Option<bool>,
    #[serde(default)]
    pub show_hidden: Option<bool>,
    #[serde(default)]
    pub pane: Option<PaneId>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EntriesArgs {
    /// The first entry to answer, counted in the pane's order.
    #[serde(default)]
    pub offset: usize,
    /// How many entries to answer at most; 200 when not given.
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub pane: Option<PaneId>,
    /// A folder or a location to list without showing it: an absolute
    /// path or `scheme:rest`. The window stays as it is.
    #[serde(default)]
    pub location: Option<PathArg>,
    /// With `location`: list hidden entries too (names starting with a dot).
    #[serde(default)]
    pub hidden: bool,
}

/// How long a listing is waited for: a script or an agent reading what a
/// pane shows right after showing it reads what it holds, not its first
/// batch; one that takes longer answers `complete: false`.
const LISTED_WITHIN: std::time::Duration = std::time::Duration::from_secs(3);

/// How `App::open_in` shows a location.
#[derive(Clone, Copy, Default)]
struct Opening<'a> {
    /// A dormant pane woken: what it kept while it slept stays.
    waking: bool,
    /// Only while the pane still shows this (a redirect, which a
    /// navigation meanwhile overrides).
    from: Option<&'a Location>,
    /// What of the pane's lens it is shown with (ADR-107).
    lensing: Lensing,
}

impl Opening<'_> {
    fn lensing(lensing: Lensing) -> Self {
        Opening {
            lensing,
            ..Opening::default()
        }
    }
}

/// What of the pane's lens a location is shown with (ADR-107,
/// `App::lensed_in`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Lensing {
    /// Brought in: a search adds it to its terms, anything else is
    /// filtered by it.
    #[default]
    Brought,
    /// A step of the history shown again (Back, Forward): brought in, in
    /// place of the lens a search's terms held then.
    Again,
    /// Its text says its lens whole: a search's terms, anything else none.
    AsIs,
    /// A step taken back, as it was: a search as its text says, anything
    /// else with the pane's.
    AsItWas,
}

/// A pane's filter text, and what the filter reader there is now reads in
/// it (`App::read_filters_again`).
type ReadAgain = (
    PaneId,
    Option<Arc<str>>,
    Option<crate::workspace::ReadFilter>,
);

/// What an undo or a redo did, for a script or an agent: the plan's
/// title under `what`, and how its steps went.
/// Where `id` is in `order`: at `at` when the caller said so rightly (a
/// view, by its row), else searched for.
fn at_in(order: &[EntryId], id: EntryId, at: Option<usize>) -> Option<usize> {
    at.filter(|&at| order.get(at) == Some(&id))
        .or_else(|| order.iter().position(|x| *x == id))
}

fn replayed(what: &str, title: Option<&str>, result: &op::ExecResult) -> Outcome {
    Outcome::Value(serde_json::json!({
        what: title,
        "applied": result.applied,
        "skipped": result.skipped,
        "error": result.error,
    }))
}

/// One entry as `entries` answers it.
#[derive(Serialize)]
struct EntryInfo {
    id: EntryId,
    name: PathArg,
    /// None for a place, which is no file (ADR-032).
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<PathArg>,
    kind: crate::entry::EntryKind,
    /// Opens as a folder: a folder, a link to one, or a place.
    folder: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    size: Option<u64>,
    /// Seconds since 1970, UTC.
    #[serde(skip_serializing_if = "Option::is_none")]
    modified: Option<u64>,
    selected: bool,
}

/// An `entries` answer that is an index's says so, and as of when
/// (`Listing::as_of`); one that is the files as they are says nothing.
fn say_as_of(answer: &mut serde_json::Value, as_of: Option<&crate::location::AsOf>) {
    let (Some(as_of), Some(answer)) = (as_of, answer.as_object_mut()) else {
        return;
    };
    let updated = as_of
        .updated
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());
    answer.insert(
        "as_of".into(),
        serde_json::json!({ "by": as_of.by, "updated": updated }),
    );
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
pub struct SelectArgs {
    #[serde(default)]
    pub ids: Vec<EntryId>,
    /// Select by name instead of (or in addition to) ids.
    #[serde(default)]
    pub names: Vec<PathArg>,
    #[serde(default)]
    pub mode: SelectMode,
    #[serde(default)]
    pub pane: Option<PaneId>,
    /// With `change`: the ids taken out of the selection.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed: Vec<EntryId>,
    /// Where the last of `ids` is in the pane's order, when the caller
    /// knows it (a view, by its row): spares a search of the order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<usize>,
}

/// What `expand` does to the folders it is given.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExpandMode {
    /// Open the closed ones and close the open ones.
    #[default]
    Toggle,
    Expand,
    Collapse,
    /// Close every folder opened in place; `ids` are ignored.
    CollapseAll,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExpandArgs {
    /// Folders the pane shows; the selected ones when empty.
    #[serde(default)]
    pub ids: Vec<EntryId>,
    #[serde(default)]
    pub mode: ExpandMode,
    #[serde(default)]
    pub pane: Option<PaneId>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
pub struct SweepArgs {
    /// Deselect what the range covers instead of selecting it.
    #[serde(default)]
    pub unset: bool,
    #[serde(default)]
    pub pane: Option<PaneId>,
    /// Where the cursor is in the pane's order, when the caller knows it
    /// (a view, by its row): spares a search of the order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RenameArgs {
    pub new_name: PathArg,
    /// The entry to rename, by absolute path; the pane's selection when
    /// omitted.
    #[serde(default)]
    pub entries: Vec<op::EntryArg>,
    #[serde(default)]
    pub pane: Option<PaneId>,
}

/// Where a copy, a move or a link goes, what it takes, and what becomes of
/// a name taken there.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct TransferArgs {
    /// Destination directory, an absolute path.
    pub dest: PathArg,
    /// The entries to act on, by absolute path; the pane's selection when
    /// omitted.
    #[serde(default)]
    pub entries: Vec<op::EntryArg>,
    #[serde(default)]
    pub pane: Option<PaneId>,
    /// What becomes of a name already taken in the destination (ADR-054):
    /// `files` `keep_both` (named "name (copy).ext", a link "name
    /// (link).ext"), `replace` (what was there to the Trash),
    /// `replace_if_newer` or `skip`; `folders` `merge`, `keep_both` or
    /// `skip` (a link takes the files' choice). Omitted, the window asks
    /// when the plan is applied; with no window, nothing is applied.
    #[serde(default)]
    pub on_conflict: Option<op::clash::Policy>,
}

/// What a command acts on: entries it is given, else a pane's selection
/// (`App::targets`).
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct TargetsArgs {
    /// The entries to act on, by absolute path; the pane's selection when
    /// omitted.
    #[serde(default)]
    pub entries: Vec<op::EntryArg>,
    /// The pane whose selection, the active pane's when omitted.
    #[serde(default)]
    pub pane: Option<PaneId>,
}

/// `set_access`: who may use the entries.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct AccessArgs {
    /// The mode, as chmod takes it: `644`, `u+x`, `go-w`, `a=rX` (`X`:
    /// folders, and what someone may run already). A class left out is
    /// everyone but what the umask leaves out, as chmod; where the system
    /// does not say its umask (macOS), such a clause that gives something
    /// is refused.
    #[serde(default)]
    pub mode: Option<String>,
    /// The group to give them, by number (as Properties and `ls -n` show
    /// it): one of the user's.
    #[serde(default)]
    pub group: Option<u32>,
    /// What the folders hold too, never through a link nor onto another
    /// drive mounted inside, nor into a trash.
    #[serde(default)]
    pub inside: bool,
    #[serde(flatten)]
    pub targets: TargetsArgs,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MkdirArgs {
    pub name: PathArg,
    /// The folder to make it in, an absolute path; the folder the pane
    /// shows when omitted.
    #[serde(default, rename = "in")]
    pub folder: Option<PathArg>,
    #[serde(default)]
    pub pane: Option<PaneId>,
}

impl MkdirArgs {
    /// The folder it is made in: the one named, else the pane's.
    fn folder(&self, app: &App) -> anyhow::Result<PathBuf> {
        match &self.folder {
            Some(folder) => Ok(crate::os_path::absolute(folder)?.to_owned()),
            None => app.pane_dir(app.pane_or_active(self.pane)),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct HydrateArgs {
    pub ids: Vec<EntryId>,
    #[serde(default)]
    pub pane: Option<PaneId>,
    /// Entries not on screen (a sort by size reads every entry's): asked
    /// after what is drawn.
    #[serde(default)]
    pub off_screen: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ApplyPlanArgs {
    pub plan: PlanData,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PlanDiffArgs {
    pub plan: PlanData,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UndoArgs {
    /// The plan to take back, as its progress named it (a toast's Undo,
    /// of the action it reports): taken back alone when nothing asked
    /// after it touched what it did, else refused, saying what did. The
    /// last plan asked when omitted.
    #[serde(default)]
    pub plan: Option<op::PlanId>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RetryArgs {
    /// The plan a step's error stopped, as its progress named it; the
    /// last one stopped when omitted.
    #[serde(default)]
    pub plan: Option<op::PlanId>,
}

mod builtin {
    use super::*;

    /// A plan of one step per entry the command acts on (`App::targets`).
    fn targets_plan(
        app: &App,
        pane: Option<PaneId>,
        entries: Vec<op::EntryArg>,
        title: &str,
        f: impl Fn(EntryRef) -> Box<dyn op::Operation>,
    ) -> anyhow::Result<Outcome> {
        let refs = app.targets(pane, entries)?;
        Ok(Outcome::Plan(Plan::with_steps(
            title,
            refs.into_iter().map(f).collect(),
        )))
    }

    /// A copy's, a move's or a link's plan, settling the names it finds
    /// taken as `policy` says (ADR-054).
    fn transfer_plan(outcome: Outcome, policy: Option<op::clash::Policy>) -> Outcome {
        match outcome {
            Outcome::Plan(plan) => Outcome::Plan(plan.on_conflict(policy)),
            other => other,
        }
    }

    /// The folder a move or a copy goes to: a path, whole (it may come
    /// from another process than the window, ADR-043).
    fn destination(dest: &Path) -> anyhow::Result<PathBuf> {
        let dest = Location::parse_os(dest.as_os_str())
            .path()
            .ok_or_else(|| anyhow::anyhow!("destination must be a path"))?;
        crate::os_path::absolute(&dest)?;
        Ok(dest)
    }

    pub fn register(app: &App) {
        let c = &app.commands;

        c.register::<NavigateArgs, _>(
            "core", Topic::Go,
            "navigate",
            "Go to location",
            "Open a path or location in a pane, an entry of it selected once listed when `select` names one; `in_place` puts it in place of the step the pane shows, with no step of its own",
            |a, app| {
                let pane = app.pane_or_active(a.pane);
                let location = Location::parse_os(a.location.as_os_str());
                if a.in_place {
                    app.navigate_in_place(pane, &location)?;
                } else {
                    app.navigate(pane, &location)?;
                }
                if let Some(name) = a.select {
                    app.reveal(pane, name);
                }
                Ok(Outcome::Done)
            },
        )
        .shows();

        c.register::<PaneArg, _>(
            "core", Topic::Places,
            "keep",
            "Add to sidebar",
            "Keep what the pane shows in the sidebar: a folder as a bookmark, a search as a saved search",
            |a, app| {
                let pane = app.pane_or_active(a.pane);
                let location = app
                    .pane_location(pane)
                    .ok_or_else(|| anyhow::anyhow!("the pane shows nothing to keep"))?;
                let keeper = app
                    .registry
                    .keepers
                    .find(|k| k.keeps(&location))
                    .ok_or_else(|| anyhow::anyhow!("this location cannot be kept in the sidebar"))?;
                let mut save = keeper.save;
                if !save.args.is_object() {
                    save.args = serde_json::json!({});
                }
                save.args["pane"] = serde_json::json!(pane);
                app.dispatch(&save)
            },
        );

        c.register::<PaneArg, _>(
            "core",
            Topic::Go,
            "refresh",
            "Refresh",
            "Re-list the pane's location",
            |a, app| {
                let pane = app.pane_or_active(a.pane);
                // The folders opened in place are shown too.
                if let Some((open, opened)) =
                    app.with_open(pane, crate::workspace::Pane::opened_locations)
                {
                    app.read_again(&open);
                    for open in opened {
                        app.read_again(&open);
                    }
                }
                Ok(Outcome::Done)
            },
        )
        .shows();

        for (name, title, description, forward) in [
            ("back", "Back", "Go to the previous location", false),
            (
                "forward",
                "Forward",
                "Return to the location Back came from",
                true,
            ),
        ] {
            let step = c.register::<HistoryArgs, _>(
                "core",
                Topic::Go,
                name,
                title,
                description,
                move |a, app| {
                    let pane = app.pane_or_active(a.pane);
                    let steps = a.steps.unwrap_or(1).max(1);
                    let target = {
                        let ws = app.workspace.read();
                        let p = ws
                            .pane(pane)
                            .ok_or_else(|| anyhow::anyhow!("no such pane"))?;
                        let pos = if forward {
                            p.history_pos.checked_add(steps)
                        } else {
                            p.history_pos.checked_sub(steps)
                        };
                        pos.and_then(|pos| {
                            Some((pos, p.history.get(pos)?.clone(), p.selection.cursor))
                        })
                    };
                    if let Some((pos, loc, left)) = target {
                        app.show_again(pane, &loc, |p, shown| {
                            p.go_to(pos, &loc, left);
                            // The step as shown, with the pane's lens.
                            p.replace_here(shown.clone());
                        })?;
                    }
                    Ok(Outcome::Done)
                },
            );
            // Steps through the history: a key held down goes on.
            step.repeats(crate::command::always);
            step.shows();
        }

        let up = c.register::<PaneArg, _>(
            "core", Topic::Go,
            "up",
            "Parent directory",
            "Go to the parent of the current directory, or to the place before the one shown in its trail (a tag's category, the folder a search looks in), the one left selected there",
            |a, app| {
                let pane = app.pane_or_active(a.pane);
                let shown = app
                    .pane_location(pane)
                    .ok_or_else(|| anyhow::anyhow!("the pane shows nothing"))?;
                if let Some(parent) = app.parent(&shown) {
                    let left = app.name_in_parent(&shown);
                    app.navigate(pane, &parent)?;
                    // As every file manager does: the way back down is
                    // where the cursor is.
                    if let Some(name) = left {
                        app.reveal(pane, name);
                    }
                }
                Ok(Outcome::Done)
            },
        );
        up.repeats(crate::command::always);
        up.shows();

        c.register::<TargetsArgs, _>(
            "core", Topic::Go,
            "show_in_folder",
            "Show in folder",
            "Open the folder the selected file is in, the file selected: from a search, a tag, the shelf, where files from many folders are listed",
            |a, app| {
                let pane = app.pane_or_active(a.pane);
                let refs = app.targets(Some(pane), a.entries)?;
                let [one] = refs.as_slice() else {
                    anyhow::bail!("select one file to show in its folder");
                };
                let (Some(parent), Some(name)) = (one.path.parent(), one.path.file_name()) else {
                    anyhow::bail!("{} is in no folder", one.path.display());
                };
                app.navigate(pane, &Location::from_path(parent))?;
                app.reveal(pane, name.to_owned());
                Ok(Outcome::Done)
            },
        )
        .shows();

        c.register::<PaneArg, _>(
            "core", Topic::Files,
            "take_out",
            "Take out of here",
            "Take the selection out of a location that lists files kept elsewhere, as it says (a tag's takes its tag off them, the shelf's unshelves them): the files stay where they are. What the Delete key does there",
            |a, app| {
                let pane = app.pane_or_active(a.pane);
                let shown = app
                    .pane_location(pane)
                    .ok_or_else(|| anyhow::anyhow!("the pane shows nothing"))?;
                let Some(remove) = app.verbs(&shown).remove else {
                    anyhow::bail!("nothing is taken out of here: Move to Trash deletes");
                };
                let mut invocation = remove.command;
                if !invocation.args.is_object() {
                    invocation.args = serde_json::json!({});
                }
                invocation.args["pane"] = serde_json::json!(pane);
                app.dispatch(&invocation)
            },
        );

        c.register::<SelectArgs, _>(
            "core",
            Topic::Select,
            "select",
            "Select",
            "Select entries by id or name",
            |a, app| {
                let pane = app.pane_or_active(a.pane);
                let mut ws = app.workspace.write();
                let p = ws
                    .pane_mut(pane)
                    .ok_or_else(|| anyhow::anyhow!("no such pane"))?;
                let mut ids = a.ids;
                // Each name looked up by the listing's index of names.
                if !a.names.is_empty() {
                    for n in &a.names {
                        match p.listing.find_by_name(n.as_os_str()).map(|e| e.id) {
                            // Nothing acts on what cannot be seen: a dot
                            // file while hidden files are off, a name the
                            // filter leaves out (`Pane::select_revealed`
                            // shows it first).
                            Some(id) if !p.is_shown(id) => anyhow::bail!(
                                "`{}` is not shown: hidden, or left out by the filter",
                                n.display()
                            ),
                            Some(id) => ids.push(id),
                            None => anyhow::bail!("no entry named `{}`", n.display()),
                        }
                    }
                }
                match a.mode {
                    SelectMode::Replace => p.selection.set(ids),
                    SelectMode::Add => ids.into_iter().for_each(|id| p.selection.add(id)),
                    SelectMode::Toggle => p.selection.toggle_all(&ids),
                    mode @ (SelectMode::RangeTo | SelectMode::AddRange) => {
                        let Some(&target) = ids.last() else {
                            anyhow::bail!("a range needs a target id")
                        };
                        let order = p.order();
                        let anchor = p.selection.anchor.or(p.selection.cursor).unwrap_or(target);
                        let pos = |id: EntryId| order.iter().position(|x| *x == id);
                        match (pos(anchor), at_in(&order, target, a.at)) {
                            (Some(a), Some(b)) => {
                                let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
                                let range = order[lo..=hi].iter().copied();
                                if matches!(mode, SelectMode::AddRange) {
                                    range.for_each(|id| p.selection.add(id));
                                } else {
                                    p.selection.set(range);
                                }
                                p.selection.anchor = Some(anchor);
                                p.selection.cursor = Some(target);
                            }
                            _ => p.selection.add(target),
                        }
                    }
                    SelectMode::Cursor => {
                        if let Some(&target) = ids.last() {
                            p.selection.cursor = Some(target);
                        }
                    }
                    SelectMode::Follow => {
                        if let Some(&target) = ids.last() {
                            let order = p.order();
                            match at_in(&order, target, a.at) {
                                Some(at) => p.selection.follow_at(target, at, &order),
                                None => p.selection.follow(target, &order),
                            }
                        }
                    }
                    SelectMode::Pick => ids.into_iter().for_each(|id| p.selection.pick(id)),
                    SelectMode::Change => {
                        // Only what it added may be an id the pane no
                        // longer shows: the rest was selected, and shown,
                        // already.
                        p.change_selection(&ids, &a.removed);
                        return Ok(Outcome::Done);
                    }
                }
                // An id the pane no longer shows (a row drawn a moment
                // ago) is not selected.
                p.retain_shown();
                Ok(Outcome::Done)
            },
        )
        .shows();

        let sort = c.register::<SortArgs, _>(
            "core",
            Topic::View,
            "sort",
            "Sort",
            "Set the sort key, direction, and hidden-file visibility of a pane",
            |a, app| {
                let pane = app.pane_or_active(a.pane);
                let mut ws = app.workspace.write();
                let p = ws
                    .pane_mut(pane)
                    .ok_or_else(|| anyhow::anyhow!("no such pane"))?;
                if let Some(k) = a.key {
                    // The same key again flips the direction unless one was given.
                    // A rating is sought from the best down, the trash from
                    // what was put there last.
                    p.sort.ascending = a.ascending.unwrap_or(if p.sort.key == k {
                        !p.sort.ascending
                    } else {
                        !matches!(
                            k,
                            crate::location::SortKey::Rating | crate::location::SortKey::Deleted
                        )
                    });
                    p.sort.key = k;
                } else if let Some(asc) = a.ascending {
                    p.sort.ascending = asc;
                }
                if let Some(h) = a.show_hidden {
                    p.set_show_hidden(h);
                }
                Ok(Outcome::Done)
            },
        );
        sort.forms(sort_forms());
        sort.shows();

        c.register::<FilterArgs, _>(
            "core", Topic::Find,
            "filter",
            "Filter",
            "Show only the entries `text` keeps: as the filter's reader reads it (the query plugin: words find names, terms the listing can decide such as kind:image, ext:pdf or size>1M hold for files, every folder staying), else those whose name holds it (case ignored unless it has a capital); without `text`, all of them. Its terms that hold for files are the pane's lens, kept as it shows other locations until taken off; its words go with the location",
            |a, app| {
                let pane = app.pane_or_active(a.pane);
                let text = a.text.as_deref().map(str::trim).filter(|t| !t.is_empty());
                // Read before the workspace's lock: the reader is a plugin's.
                let reader = app.registry.filter_readers.all().into_iter().next();
                let read = text.zip(reader).and_then(|(text, reader)| (reader.0)(text));
                if let Some(p) = app.workspace.write().pane_mut(pane) {
                    p.set_filter(text, read);
                }
                app.roll_filters_over();
                app.wake();
                Ok(Outcome::Done)
            },
        )
        .shows();

        c.register::<PaneArg, _>(
            "core",
            Topic::View,
            "toggle_hidden",
            "Toggle hidden files",
            "Show or hide dot-prefixed entries",
            |a, app| {
                let pane = app.pane_or_active(a.pane);
                if let Some(p) = app.workspace.write().pane_mut(pane) {
                    p.set_show_hidden(!p.sort.show_hidden);
                }
                Ok(Outcome::Done)
            },
        )
        .shows();

        c.register::<PaneArg, _>(
            "core",
            Topic::Select,
            "select_all",
            "Select all",
            "Select every entry",
            |a, app| {
                let pane = app.pane_or_active(a.pane);
                let mut ws = app.workspace.write();
                let p = ws
                    .pane_mut(pane)
                    .ok_or_else(|| anyhow::anyhow!("no such pane"))?;
                // What the pane shows: hidden files it does not show are
                // not selected, or a Delete would take them unseen.
                let ids = p.order();
                let cursor = p.selection.cursor;
                p.selection.set(ids.iter().copied());
                p.selection.cursor = cursor.or(p.selection.cursor);
                Ok(Outcome::Done)
            },
        )
        .shows();

        c.register::<PaneArg, _>(
            "core",
            Topic::Select,
            "select_invert",
            "Invert selection",
            "Select what is shown and not selected, and deselect the rest",
            |a, app| {
                let pane = app.pane_or_active(a.pane);
                let mut ws = app.workspace.write();
                let p = ws
                    .pane_mut(pane)
                    .ok_or_else(|| anyhow::anyhow!("no such pane"))?;
                let order = p.order();
                let inverted: Vec<EntryId> = order
                    .iter()
                    .copied()
                    .filter(|id| !p.selection.contains(*id))
                    .collect();
                let cursor = p.selection.cursor;
                p.selection.set(inverted);
                p.selection.cursor = cursor.or(p.selection.cursor);
                Ok(Outcome::Done)
            },
        )
        .shows();

        c.register::<PaneArg, _>(
            "core",
            Topic::Select,
            "select_none",
            "Clear selection",
            "Deselect everything",
            |a, app| {
                let pane = app.pane_or_active(a.pane);
                if let Some(p) = app.workspace.write().pane_mut(pane) {
                    p.selection.clear();
                }
                Ok(Outcome::Done)
            },
        )
        .shows();

        let sweep = c.register::<SweepArgs, _>(
            "core", Topic::Select,
            "select_sweep",
            "Select as the cursor moves",
            "Hold a range from the cursor: moving selects, or with `unset` deselects, what lies between",
            |a, app| {
                let pane = app.pane_or_active(a.pane);
                let mut ws = app.workspace.write();
                let p = ws
                    .pane_mut(pane)
                    .ok_or_else(|| anyhow::anyhow!("no such pane"))?;
                let order = p.order();
                match p.selection.cursor.and_then(|cursor| at_in(&order, cursor, a.at)) {
                    Some(at) => p.selection.sweep_start_at(a.unset, at, &order),
                    None => p.selection.sweep_start(a.unset, &order),
                }
                Ok(Outcome::Done)
            },
        );
        sweep.forms([Form::new(
            serde_json::json!({ "unset": true }),
            "Deselect as the cursor moves",
        )]);
        sweep.shows();

        c.register::<PaneArg, _>(
            "core",
            Topic::Select,
            "select_sweep_end",
            "Stop selecting as the cursor moves",
            "End the range held from the cursor, keeping what it covers",
            |a, app| {
                let pane = app.pane_or_active(a.pane);
                if let Some(p) = app.workspace.write().pane_mut(pane) {
                    p.selection.sweep_end();
                }
                Ok(Outcome::Done)
            },
        )
        .shows();

        c.register::<ExpandArgs, _>(
            "core",
            Topic::View,
            "expand",
            "Expand folder",
            "Open folders in place under the one shown, as the tree view draws them, or close them",
            |a, app| {
                let pane = app.pane_or_active(a.pane);
                if a.mode == ExpandMode::CollapseAll {
                    app.collapse_all(pane);
                    return Ok(Outcome::Done);
                }
                // Which of them are folders or places, and where they lead.
                let leads: Vec<(EntryId, Location)> = {
                    let ws = app.workspace.read();
                    let p = ws
                        .pane(pane)
                        .filter(|p| p.open.is_some())
                        .ok_or_else(|| anyhow::anyhow!("the pane shows nothing"))?;
                    let ids: Vec<EntryId> = if a.ids.is_empty() {
                        p.selection.iter().collect()
                    } else {
                        a.ids.clone()
                    };
                    ids.into_iter()
                        .filter_map(|id| Some((id, p.leads_to(id)?)))
                        .collect()
                };
                if leads.is_empty() {
                    return Err(crate::command::NothingSelected.into());
                }
                {
                    let mut ws = app.workspace.write();
                    let p = ws
                        .pane_mut(pane)
                        .ok_or_else(|| anyhow::anyhow!("no such pane"))?;
                    for (at, location) in leads {
                        let open = p.is_expanded(&location);
                        match a.mode {
                            ExpandMode::Expand | ExpandMode::Toggle if !open => {
                                p.expand(location, at);
                            }
                            ExpandMode::Collapse | ExpandMode::Toggle if open => {
                                p.collapse(&location);
                            }
                            _ => {}
                        }
                    }
                }
                app.open_expanded(pane);
                app.wake();
                Ok(Outcome::Done)
            },
        )
        .shows();

        c.register::<HydrateArgs, _>(
            "core",
            Topic::Files,
            "hydrate",
            "Load metadata",
            "Request metadata for entries (views call this for visible rows)",
            |a, app| {
                let pane = app.pane_or_active(a.pane);
                let groups = app
                    .workspace
                    .read()
                    .pane(pane)
                    .map(|p| p.by_location(&a.ids))
                    .unwrap_or_default();
                let priority = if a.off_screen {
                    Priority::Normal
                } else {
                    Priority::Visible
                };
                for (open, ids) in groups {
                    open.hydrate(&ids, priority);
                }
                Ok(Outcome::Done)
            },
        )
        .shows();

        c.register::<EntriesArgs, _>(
            "core", Topic::Files,
            "entries",
            "List entries",
            "What a pane shows, in its order (sorted, filtered, with the folders opened in place), or with `location` what a folder holds, listed without showing it: each entry's id, name, path, kind, size and modification time (seconds since 1970), and whether it is selected; from `offset`, at most `limit`. `complete: false`: the listing was still arriving, ask again. `as_of`, when present: the answer is an index's as of its last update (`by` says which, `updated` when, in seconds since 1970), so a file made, renamed or removed since is not in it",
            |a, app| {
                use crate::entry::keys;
                let info = |id: EntryId, e: &crate::entry::Entry, path: Option<PathBuf>, selected: bool| EntryInfo {
                    id,
                    name: PathArg(e.name.clone().into()),
                    path: path.map(PathArg),
                    kind: e.kind,
                    folder: e.is_dir() || id.is_place(),
                    size: e.metadata.value(keys::SIZE).and_then(crate::Value::as_u64),
                    modified: e
                        .metadata
                        .value(keys::MODIFIED)
                        .and_then(crate::Value::as_time)
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs()),
                    selected,
                };
                let limit = a.limit.unwrap_or(200);
                if let Some(location) = &a.location {
                    let location = Location::parse_os(location.as_os_str());
                    return app.listed_elsewhere(&location, a.hidden, a.offset, limit, info);
                }
                let pane = app.pane_or_active(a.pane);
                app.await_listing(pane);
                let withheld = app.withheld(pane);
                let ws = app.workspace.read();
                let p = ws
                    .pane(pane)
                    .ok_or_else(|| anyhow::anyhow!("no pane {pane:?}"))?;
                let order: Vec<EntryId> = p
                    .order()
                    .iter()
                    .copied()
                    .filter(|id| !withheld.contains(id))
                    .collect();
                let entries: Vec<EntryInfo> = order
                    .iter()
                    .skip(a.offset)
                    .take(limit)
                    .filter_map(|&id| {
                        Some(info(id, p.entry(id)?, p.path_of(id), p.selection.contains(id)))
                    })
                    .collect();
                let mut answer = serde_json::json!({
                    "pane": pane,
                    "location": p.shown(),
                    "complete": p.listing.is_complete(),
                    "total": order.len(),
                    "selected": p.selection.iter().filter(|id| !withheld.contains(id)).count(),
                    "entries": entries,
                });
                say_as_of(&mut answer, p.listing.as_of());
                Ok(Outcome::Value(answer))
            },
        )
        .data();

        c.register::<RenameArgs, _>(
            "core",
            Topic::Files,
            "rename",
            "Rename",
            "Rename the selected entry, or the one given (plan)",
            |a, app| {
                let refs = app.targets(a.pane, a.entries)?;
                let [r] = refs.as_slice() else {
                    anyhow::bail!("rename needs exactly one entry")
                };
                let new_name = a.new_name.into_inner().into_os_string();
                op::valid_file_name(&new_name)?;
                Ok(Outcome::Plan(Plan::with_steps(
                    format!("rename to {}", new_name.display()),
                    vec![Box::new(op::Rename {
                        entry: r.clone(),
                        new_name,
                    })],
                )))
            },
        )
        .plans();

        c.register::<TransferArgs, _>(
            "core", Topic::Files,
            "move",
            "Move to…",
            "Move the selection, or the entries given, to a directory (plan); on_conflict says what becomes of a name taken there",
            |a, app| {
                let dest = destination(&a.dest)?;
                Ok(transfer_plan(
                    targets_plan(
                        app,
                        a.pane,
                        a.entries,
                        &format!("move to {}", dest.display()),
                        |e| Box::new(op::Move::new(e, dest.clone())),
                    )?,
                    a.on_conflict,
                ))
            },
        )
        .plans();

        c.register::<TransferArgs, _>(
            "core", Topic::Files,
            "copy",
            "Copy to…",
            "Copy the selection, or the entries given, to a directory (plan); on_conflict says what becomes of a name taken there",
            |a, app| {
                let dest = destination(&a.dest)?;
                Ok(transfer_plan(
                    targets_plan(
                        app,
                        a.pane,
                        a.entries,
                        &format!("copy to {}", dest.display()),
                        |e| Box::new(op::Copy::from_settings(e, dest.clone(), &app.settings)),
                    )?,
                    a.on_conflict,
                ))
            },
        )
        .plans();

        c.register::<TransferArgs, _>(
            "core", Topic::Files,
            "link",
            "Link to…",
            "Make a link in a directory to each of the selection, or the entries given, under its name (plan); on_conflict says what becomes of a name taken there",
            |a, app| {
                let dest = destination(&a.dest)?;
                Ok(transfer_plan(
                    targets_plan(
                        app,
                        a.pane,
                        a.entries,
                        &format!("link in {}", dest.display()),
                        |e| Box::new(op::Link::new(e, dest.clone())),
                    )?,
                    a.on_conflict,
                ))
            },
        )
        .plans();

        c.register::<AccessArgs, _>(
            "core", Topic::Files,
            "set_access",
            "Change access",
            "Change who may read, write and run the selection, or the entries given: `mode` as chmod takes it, `group` by number, `inside` for what folders hold (plan). What a link leads to changes; the owner never does",
            |a, app| {
                anyhow::ensure!(
                    a.mode.is_some() || a.group.is_some(),
                    "say a mode, a group or both"
                );
                if let Some(mode) = &a.mode {
                    op::access::Mode::parse(mode)?;
                }
                let title = match (&a.mode, a.group) {
                    (Some(mode), None) => format!("change access ({mode})"),
                    (None, Some(group)) => format!("change group ({group})"),
                    _ => "change access and group".to_owned(),
                };
                targets_plan(app, a.targets.pane, a.targets.entries, &title, |e| {
                    Box::new(op::access::SetAccess {
                        entry: e,
                        mode: a.mode.clone(),
                        group: a.group,
                        inside: a.inside,
                    })
                })
            },
        )
        .plans();

        c.register::<TargetsArgs, _>(
            "core",
            Topic::Files,
            "trash",
            "Move to trash",
            "Trash the selection, or the entries given (plan)",
            |a, app| {
                // A folder a program mounts across a network (sshfs, rclone,
                // GVfs) keeps no trash: what is there is deleted for good,
                // asked first as any deletion is (ADR-077).
                // A mount point itself stays a trash step, which refuses
                // it in words (`Trash`'s check): never a deletion that would
                // reach into the server.
                let without = |e: &EntryRef| {
                    crate::fs::reach::of(&e.path)
                        .is_some_and(|remote| !remote.by_kernel() && remote.point != e.path)
                };
                let refs = app.targets(a.pane, a.entries)?;
                let deleted = refs.iter().filter(|e| without(e)).count();
                // A server that keeps what is deleted in a trash of its own
                // (a cloud's, ADR-086) is said by the question itself.
                let for_good = refs
                    .iter()
                    .filter(|e| without(e))
                    .filter(|e| {
                        app.remote(&e.path)
                            .and_then(|(_, known)| known?.trash)
                            .is_none()
                    })
                    .count();
                let steps: Vec<Box<dyn op::Operation>> = refs
                    .into_iter()
                    .map(|e| -> Box<dyn op::Operation> {
                        if without(&e) {
                            Box::new(op::Delete { entry: e })
                        } else {
                            Box::new(op::Trash { entry: e })
                        }
                    })
                    .collect();
                let all = steps.len();
                let plan = Plan::with_steps(
                    if deleted == all && all > 0 {
                        "delete permanently"
                    } else {
                        "move to trash"
                    },
                    steps,
                );
                // Said after the question, which counts what is deleted
                // (`deleting.rs`): where they are, and what goes to the
                // trash beside them.
                let kept = all - deleted;
                let said = if for_good == 1 {
                    "It is in a network folder, which keeps no trash"
                } else {
                    "They are in network folders, which keep no trash"
                };
                Ok(Outcome::Plan(match (for_good, kept) {
                    (0, _) => plan,
                    (_, 0) => plan.with_note(said),
                    (_, 1) => plan.with_note(format!("{said}; the other item goes to the trash")),
                    (_, kept) => {
                        plan.with_note(format!("{said}; the other {kept} items go to the trash"))
                    }
                }))
            },
        )
        .plans();

        c.register::<TargetsArgs, _>(
            "core",
            Topic::Files,
            "delete",
            "Delete permanently",
            "Delete the selection, or the entries given, without trash (plan, irreversible)",
            |a, app| {
                // What stays on the disk, said where the listing knows it:
                // of the selection, whose rows it holds.
                let kept = a
                    .entries
                    .is_empty()
                    .then(|| app.selection_kept(app.pane_or_active(a.pane)))
                    .flatten();
                let plan = targets_plan(app, a.pane, a.entries, "delete permanently", |e| {
                    Box::new(op::Delete { entry: e })
                })?;
                Ok(match (plan, kept.and_then(|k| k.said())) {
                    (Outcome::Plan(plan), Some(note)) => Outcome::Plan(plan.with_note(note)),
                    (plan, _) => plan,
                })
            },
        )
        .plans();

        c.register::<MkdirArgs, _>(
            "core",
            Topic::Files,
            "mkdir",
            "New folder",
            "Create a folder in the folder given in `in`, or the one the pane shows (plan)",
            |a, app| {
                let dir = a.folder(app)?;
                let name = a.name.into_inner().into_os_string();
                op::valid_file_name(&name)?;
                Ok(Outcome::Plan(Plan::with_steps(
                    format!("new folder {}", name.display()),
                    vec![Box::new(op::Mkdir {
                        path: dir.join(name),
                    })],
                )))
            },
        )
        .plans();

        c.register::<MkdirArgs, _>(
            "core", Topic::Files,
            "new_file",
            "New file",
            "Create an empty file in the folder given in `in`, or the one the pane shows (plan); a name ending in / makes a folder",
            |a, app| {
                let dir = a.folder(app)?;
                let name = a.name.into_inner().into_os_string();
                // A name ending in `/` is a folder, as yazi makes one.
                if let Some(folder) = name.to_str().and_then(|n| n.strip_suffix('/')) {
                    op::valid_file_name(std::ffi::OsStr::new(folder))?;
                    return Ok(Outcome::Plan(Plan::with_steps(
                        format!("new folder {folder}"),
                        vec![Box::new(op::Mkdir {
                            path: dir.join(folder),
                        })],
                    )));
                }
                op::valid_file_name(&name)?;
                Ok(Outcome::Plan(Plan::with_steps(
                    format!("new file {}", name.display()),
                    vec![Box::new(op::NewFile {
                        path: dir.join(name),
                    })],
                )))
            },
        )
        .plans();

        c.register::<PlanDiffArgs, _>(
            "core",
            Topic::Files,
            "plan_diff",
            "Preview plan",
            "Dry-run a plan and return what each step would do",
            |a, app| {
                let plan = Plan::from_data(&a.plan, &app.ops)?;
                Ok(Outcome::Value(serde_json::to_value(
                    plan.diff(&app.companions),
                )?))
            },
        )
        .data();

        c.register::<ApplyPlanArgs, _>(
            "core", Topic::Files,
            "apply_plan",
            "Apply plan",
            "Apply a plan given as data (plan): as any command's, the caller applies it, at once for the user's own gesture, after review for an agent",
            |a, app| Ok(Outcome::Plan(Plan::from_data(&a.plan, &app.ops)?)),
        )
        .plans();

        c.register::<UndoArgs, _>(
            "core", Topic::Files,
            "undo",
            "Undo",
            "Undo the most recent plan, or the plan named when nothing asked after it touched what it did; asked from outside the window, says what it took back",
            |a, app| {
                if crate::scheduler::on_window_thread() {
                    match a.plan {
                        // Its run says what it did, or why it could not.
                        Some(plan) => app.executor.undo_of(plan, app.lifetime().child()),
                        None => anyhow::ensure!(
                            app.executor.undo(app.lifetime().child()),
                            "nothing to undo"
                        ),
                    }
                    return Ok(Outcome::Done);
                }
                // A script or an agent waits, and is told what it took back.
                let cancel = app.lifetime().child();
                let result = match a.plan {
                    Some(plan) => app.executor.undo_of_blocking(plan, &cancel),
                    None => app.executor.undo_blocking(&cancel),
                }
                .ok_or_else(|| anyhow::anyhow!("nothing to undo"))?;
                Ok(replayed(
                    "undone",
                    app.executor.last_redo_title().as_deref(),
                    &result,
                ))
            },
        );

        c.register::<NoArgs, _>(
            "core", Topic::Files,
            "redo",
            "Redo",
            "Redo the most recently undone plan; asked from outside the window, says what it applied again",
            |_, app| {
                if crate::scheduler::on_window_thread() {
                    anyhow::ensure!(app.executor.redo(app.lifetime().child()), "nothing to redo");
                    return Ok(Outcome::Done);
                }
                let result = app
                    .executor
                    .redo_blocking(&app.lifetime().child())
                    .ok_or_else(|| anyhow::anyhow!("nothing to redo"))?;
                Ok(replayed("redone", app.executor.last_undo_title().as_deref(), &result))
            },
        );

        c.register::<RetryArgs, _>(
            "core", Topic::Files,
            "retry",
            "Retry",
            "Run again what a plan stopped by an error had left, the last one stopped unless told which; what it applies is undone with that plan; asked from outside the window, says what it applied",
            |a, app| {
                let plan = a
                    .plan
                    .or_else(|| app.executor.last_stopped())
                    .ok_or_else(|| anyhow::anyhow!("nothing to retry"))?;
                if crate::scheduler::on_window_thread() {
                    anyhow::ensure!(
                        app.executor.retry(plan, app.lifetime().child()),
                        "nothing to retry"
                    );
                    return Ok(Outcome::Done);
                }
                let title = app.executor.stopped_title(plan);
                let result = app
                    .executor
                    .retry_blocking(plan, &app.lifetime().child())
                    .ok_or_else(|| anyhow::anyhow!("nothing to retry"))?;
                Ok(replayed("retried", title.as_deref(), &result))
            },
        );

        c.register::<NoArgs, _>(
            "core",
            Topic::Skwir,
            "commands",
            "List commands",
            "Return every registered command with its argument schema",
            |_, app| Ok(Outcome::Value(serde_json::to_value(app.commands.list())?)),
        )
        .data();
    }
}

/// `sort`'s forms: each key, each way, in words (ADR-024).
fn sort_forms() -> Vec<Form> {
    const KEYS: [(&str, &str, &str, &str); 5] = [
        ("name", "name", "A to Z", "Z to A"),
        ("size", "size", "smallest first", "largest first"),
        ("modified", "date modified", "oldest first", "newest first"),
        ("rating", "rating", "lowest first", "highest first"),
        ("deleted", "date deleted", "oldest first", "newest first"),
    ];
    let mut forms = vec![
        Form::new(serde_json::json!({ "ascending": true }), "Sort ascending"),
        Form::new(serde_json::json!({ "ascending": false }), "Sort descending"),
    ];
    for (key, words, up, down) in KEYS {
        forms.push(Form::new(
            serde_json::json!({ "key": key }),
            format!("Sort by {words}"),
        ));
        forms.push(Form::new(
            serde_json::json!({ "key": key, "ascending": true }),
            format!("Sort by {words}, {up}"),
        ));
        forms.push(Form::new(
            serde_json::json!({ "key": key, "ascending": false }),
            format!("Sort by {words}, {down}"),
        ));
    }
    forms
}

#[cfg(test)]
mod tests {
    use super::common_parent;
    use std::path::{Path, PathBuf};

    #[test]
    fn the_common_parent_is_the_nearest_folder_they_share() {
        let parent = |paths: &[&str]| common_parent(paths.iter().map(Path::new));
        assert_eq!(parent(&[]), None);
        assert_eq!(parent(&["/a/b/c.txt"]), Some(PathBuf::from("/a/b")));
        assert_eq!(
            parent(&["/a/b/c.txt", "/a/b/d e/f.txt", "/a/b/g"]),
            Some(PathBuf::from("/a/b"))
        );
        // By whole components, never a shared prefix of a name.
        assert_eq!(
            parent(&["/data/ab/x", "/data/ac/y"]),
            Some(PathBuf::from("/data"))
        );
        assert_eq!(parent(&["/x/1", "/y/2"]), Some(PathBuf::from("/")));
    }

    /// A plugin takes its handle when it registers, before the UI installs
    /// its hook: the handle still wakes the UI.
    #[test]
    fn a_wake_handle_taken_early_reaches_the_hook_installed_later() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};
        let tmp = tempfile::tempdir().unwrap();
        let app = super::App::new(super::AppConfig::ephemeral(tmp.path())).unwrap();
        let early = app.waker_handle();
        early();
        let woken = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&woken);
        app.set_waker(Some(Arc::new(move || {
            counted.fetch_add(1, Ordering::SeqCst);
        })));
        early();
        assert_eq!(woken.load(Ordering::SeqCst), 1);
        app.set_waker(None);
        early();
        assert_eq!(woken.load(Ordering::SeqCst), 1);
    }

    /// Where a location said it is reached replaces it only while its pane
    /// still shows it: a navigation meanwhile wins (ADR-077).
    #[test]
    fn a_redirect_replaces_only_what_still_shows() {
        use crate::Location;
        let tmp = tempfile::tempdir().unwrap();
        let [a, b, c] = ["a", "b", "c"].map(|name| {
            let dir = tmp.path().join(name);
            std::fs::create_dir(&dir).unwrap();
            Location::from_path(dir)
        });
        let app = super::App::new(super::AppConfig::ephemeral(tmp.path())).unwrap();
        let pane = app.workspace.read().active_id();
        let from = |from| super::Opening {
            from: Some(from),
            ..super::Opening::default()
        };
        app.navigate(pane, &a).unwrap();
        app.navigate(pane, &b).unwrap();
        // `a` said it is reached at `c`, as the pane went on to `b`.
        app.open_in(pane, &c, from(&a), |p, _| p.replace_here(c.clone()))
            .unwrap();
        assert_eq!(app.pane_location(pane).as_ref(), Some(&b));
        app.open_in(pane, &c, from(&b), |p, _| p.replace_here(c.clone()))
            .unwrap();
        assert_eq!(app.pane_location(pane).as_ref(), Some(&c));
        let ws = app.workspace.read();
        assert_eq!(ws.pane(pane).unwrap().history.last(), Some(&c));
    }

    /// A lens is read again wherever a location is shown (ADR-107): a date
    /// it holds (`modified:today`) says the day it is read on, in a folder
    /// as in a search. Shown as it is, a folder has none.
    #[test]
    fn a_lens_is_read_again_where_a_location_is_shown() {
        use crate::Location;
        use crate::workspace::{Lens, ReadFilter};
        use std::sync::atomic::{AtomicUsize, Ordering};
        let tmp = tempfile::tempdir().unwrap();
        let [a, b, c] = ["a", "b", "c"].map(|name| {
            let dir = tmp.path().join(name);
            std::fs::create_dir(&dir).unwrap();
            Location::from_path(dir)
        });
        let app = super::App::new(super::AppConfig::ephemeral(tmp.path())).unwrap();
        let reads = std::sync::Arc::new(AtomicUsize::new(0));
        let counted = std::sync::Arc::clone(&reads);
        let added = app.registry.filter_readers.add(
            "test",
            crate::registry::FilterReader(std::sync::Arc::new(move |text: &str| {
                counted.fetch_add(1, Ordering::Relaxed);
                let lens = Lens {
                    words: text.into(),
                    set: Vec::new(),
                    keeps: std::sync::Arc::new(|_| true),
                    metadata: false,
                    next_change: None,
                };
                Some(ReadFilter {
                    keeps: std::sync::Arc::new(|_| true),
                    waits: Vec::new(),
                    metadata: false,
                    lens: Some(lens),
                    next_change: None,
                })
            })),
        );
        assert!(added);
        let pane = app.workspace.read().active_id();
        let keeps = || {
            let ws = app.workspace.read();
            let p = ws.pane(pane).unwrap();
            p.lens().map(|l| std::sync::Arc::clone(&l.keeps))
        };
        app.navigate(pane, &a).unwrap();
        app.dispatch(&crate::command::Invocation::with_args(
            "filter",
            &serde_json::json!({ "text": "modified:today" }),
        ))
        .unwrap();
        let first = keeps().expect("a lens");
        assert_eq!(reads.load(Ordering::Relaxed), 1);
        app.navigate(pane, &b).unwrap();
        let again = keeps().expect("kept");
        assert_eq!(reads.load(Ordering::Relaxed), 2, "read again");
        assert!(!std::sync::Arc::ptr_eq(&first, &again), "its test anew");
        app.navigate_as_is(pane, &c, false).unwrap();
        assert!(keeps().is_none(), "shown as it is: none");
    }

    /// A filter whose words read otherwise later (`modified:today` at
    /// midnight) is read again then, in a folder kept open: no location
    /// need be shown again for its day to move on.
    #[test]
    fn a_filter_is_read_again_when_its_words_read_otherwise() {
        use crate::workspace::{Lens, ReadFilter};
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::time::{Duration, Instant, SystemTime};
        let tmp = tempfile::tempdir().unwrap();
        let app = super::App::new(super::AppConfig::ephemeral(tmp.path())).unwrap();
        let reads = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&reads);
        let added = app.registry.filter_readers.add(
            "test",
            crate::registry::FilterReader(Arc::new(move |text: &str| {
                // The first reading reads otherwise soon; the next never.
                let first = counted.fetch_add(1, Ordering::Relaxed) == 0;
                let next_change = first.then(|| SystemTime::now() + Duration::from_millis(200));
                let lens = Lens {
                    words: text.into(),
                    set: Vec::new(),
                    keeps: Arc::new(|_| true),
                    metadata: false,
                    next_change,
                };
                Some(ReadFilter {
                    keeps: Arc::new(|_| true),
                    waits: Vec::new(),
                    metadata: false,
                    lens: Some(lens),
                    next_change,
                })
            })),
        );
        assert!(added);
        let pane = app.workspace.read().active_id();
        app.navigate(pane, &crate::Location::from_path(tmp.path()))
            .unwrap();
        app.dispatch(&crate::command::Invocation::with_args(
            "filter",
            &serde_json::json!({ "text": "modified:today" }),
        ))
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while reads.load(Ordering::Relaxed) < 2 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(reads.load(Ordering::Relaxed), 2, "read again, once");
        let ws = app.workspace.read();
        let p = ws.pane(pane).unwrap();
        assert_eq!(p.filter(), Some("modified:today"), "its words kept");
        assert_eq!(p.read_filter().and_then(|r| r.next_change), None);
    }

    /// A search shown as it is typed: one step, each change in its place,
    /// and taken back whole (ADR-098).
    #[test]
    fn a_step_shown_in_place_is_one_and_is_taken_back_whole() {
        use crate::Location;
        let tmp = tempfile::tempdir().unwrap();
        let [a, b, c, d] = ["a", "b", "c", "d"].map(|name| {
            let dir = tmp.path().join(name);
            std::fs::create_dir(&dir).unwrap();
            Location::from_path(dir)
        });
        let app = super::App::new(super::AppConfig::ephemeral(tmp.path())).unwrap();
        let pane = app.workspace.read().active_id();
        let history = || {
            let ws = app.workspace.read();
            let p = ws.pane(pane).unwrap();
            (p.history.clone(), p.history_pos)
        };
        app.navigate(pane, &a).unwrap();
        app.navigate(pane, &b).unwrap();
        app.step_here(pane).unwrap();
        app.navigate_in_place(pane, &c).unwrap();
        app.navigate_in_place(pane, &d).unwrap();
        assert_eq!(history(), (vec![a.clone(), b.clone(), d.clone()], 2));
        // What is shown already is left as it is.
        app.navigate_in_place(pane, &d).unwrap();
        assert_eq!(history(), (vec![a.clone(), b.clone(), d.clone()], 2));
        app.take_step_back(pane).unwrap();
        assert_eq!(app.pane_location(pane).as_ref(), Some(&b));
        assert_eq!(history(), (vec![a.clone(), b.clone()], 1));
        // The first step is never taken back.
        app.take_step_back(pane).unwrap();
        app.take_step_back(pane).unwrap();
        assert_eq!(app.pane_location(pane).as_ref(), Some(&a));
        assert_eq!(history(), (vec![a.clone()], 0));
        // A navigation from elsewhere lands between reading the step
        // before and taking it back: what is shown is a step of its own.
        app.navigate(pane, &b).unwrap();
        let from = history().1;
        app.navigate(pane, &c).unwrap();
        app.workspace
            .write()
            .pane_mut(pane)
            .unwrap()
            .take_step_back(from, &a, None);
        assert_eq!(history(), (vec![a.clone(), b, c, a], 3));
    }

    /// What an agent's call opens is checked as it resolves, never as its
    /// argument is written: a relative path read in the pane's folder, a
    /// folder's location (`path:`), `..` around an automount point; the
    /// user's own call opens it.
    #[test]
    fn an_agent_opens_no_private_folder_however_it_is_written() {
        use crate::Location;
        use crate::permissions::{Privacy, as_agent};
        let tmp = tempfile::tempdir().unwrap();
        let keys = tmp.path().join("it's keys");
        let open = tmp.path().join("open notes");
        std::fs::create_dir_all(keys.join("old")).unwrap();
        std::fs::create_dir(&open).unwrap();
        let app = super::App::new(super::AppConfig::ephemeral(tmp.path())).unwrap();
        let pane = app.workspace.read().active_id();
        app.navigate(pane, &Location::from_path(&open)).unwrap();
        let privacy = Privacy::folders([keys.clone()]);
        let agent = |location: &str| {
            as_agent(privacy.clone(), || {
                app.navigate(pane, &Location::parse(location))
            })
        };
        let error = agent("../it's keys/old").expect_err("relative");
        assert!(error.to_string().contains("private"), "{error:#}");
        assert!(agent(&format!("path:{}", keys.display())).is_err());
        assert!(
            as_agent(privacy.clone(), || app
                .open_location(&Location::from_path(&keys)))
            .is_err(),
            "listed without showing it"
        );
        assert_eq!(app.pane_dir(pane).unwrap(), open, "the pane stays");
        agent("../open notes").unwrap();
        app.navigate(pane, &Location::parse("../it's keys"))
            .unwrap();
        assert_eq!(app.pane_dir(pane).unwrap(), keys, "the user goes there");
    }

    /// Whether words can search a place is its provider's answer, whatever
    /// its scheme: one the shell has never heard of says yes for a place
    /// and no for its top; a folder is searched; what no provider opens is
    /// not.
    #[test]
    fn a_scope_is_what_its_provider_says() {
        use crate::Location;
        use crate::location::{LocationProvider, OpenContext, OpenLocation};
        use std::sync::Arc;

        struct Shelves;
        impl LocationProvider for Shelves {
            fn scheme(&self) -> &'static str {
                "shelves"
            }
            fn open(&self, _: &Location, _: OpenContext) -> anyhow::Result<Arc<dyn OpenLocation>> {
                anyhow::bail!("never opened here")
            }
            fn searchable(&self, location: &Location) -> bool {
                location.rest != "/"
            }
        }
        struct Bins;
        impl LocationProvider for Bins {
            fn scheme(&self) -> &'static str {
                "bins"
            }
            fn open(&self, _: &Location, _: OpenContext) -> anyhow::Result<Arc<dyn OpenLocation>> {
                anyhow::bail!("never opened here")
            }
        }
        let tmp = tempfile::tempdir().unwrap();
        let app = super::App::new(super::AppConfig::ephemeral(tmp.path())).unwrap();
        app.register_provider("test", Arc::new(Shelves));
        app.register_provider("test", Arc::new(Bins));
        assert!(app.searchable(&Location::new("shelves", "/l'été/naïve")));
        assert!(!app.searchable(&Location::new("shelves", "/")), "its top");
        assert!(!app.searchable(&Location::new("bins", "")), "says nothing");
        assert!(!app.searchable(&Location::new("unknown", "/x")));
        assert!(app.searchable(&Location::from_path(tmp.path().join("it's here"))));
    }
}
