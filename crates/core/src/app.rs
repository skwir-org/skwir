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
use crate::command::{CommandRegistry, Invocation, NoArgs, Outcome};
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

/// A question for the user, Allow or Deny (`App::ask`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Question {
    pub id: u64,
    /// What is asked, in a line: "An agent asks to run “Undo”".
    pub title: String,
    /// What it would do, in full: the command and its arguments.
    pub detail: String,
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
    /// Scheduler threads; 0 picks a default.
    pub workers: usize,
    /// Entries per `Added` event.
    pub batch_size: usize,
}

impl Default for AppConfig {
    fn default() -> Self {
        AppConfig {
            cache_root: Cache::default_root(),
            settings_path: Some(Settings::default_path()),
            workers: 0,
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
            workers: 0,
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
        let scheduler = Scheduler::new(config.workers);
        let lifetime = CancelToken::new();
        let settings = match config.settings_path {
            Some(p) => Settings::load(p),
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
            previews: Arc::new(crate::preview::Previews::default()),
            tiles: Arc::new(crate::tiles::Tiles::default()),
            maps: Arc::new(crate::map_points::MapSources::default()),
            proposals: parking_lot::Mutex::new(Vec::new()),
            questions: parking_lot::Mutex::new(Vec::new()),
            answers: parking_lot::Mutex::new(HashMap::new()),
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
        builtin::register(&app);
        crate::notes::commands::register(&app);
        crate::tabs::register(&app);
        crate::keymap::declare(&app.settings, app.weak());
        crate::settings::register(&app);
        crate::permissions::declare(&app.settings);
        crate::plugin::register(&app);
        crate::playing::register(&app);
        if let Some(problem) = app.settings.take_problem() {
            app.notify(NotifyLevel::Warning, problem);
        }
        Ok(app)
    }

    /// Register a place notes are kept (ADR-027) and its settings on the
    /// Notes page: whether it is read, and how it is written.
    pub fn register_notes_store(&self, owner: &str, store: Arc<dyn crate::notes::NotesStore>) {
        use crate::notes::{Kept, Write};
        let (id, title) = (store.id(), store.title());
        let holds = store.holds();
        let limits = if holds.limits.is_empty() {
            String::new()
        } else {
            format!(" What it cannot hold: {}.", holds.limits)
        };
        let read = crate::notes::read_key(id);
        self.settings.declare(
            crate::settings::SettingSpec::toggle(
                &read,
                "Read",
                &format!("Show the notes it keeps, merged with the others'.{limits}"),
                store.default_read(),
            )
            .section(title),
        );
        self.notes.declared(owner, read);
        // A name is written by a rename of its own, never by an edit.
        if store.kept() == Kept::Name {
            self.notes.register(owner, store);
            self.apply_notes_settings();
            return;
        }
        let mut choices: Vec<(&str, &str)> = vec![
            ("off", Write::Off.title()),
            ("update", Write::Update.title()),
        ];
        if store.kept() == Kept::Beside {
            choices.push(("fallback", Write::Fallback.title()));
        }
        choices.push(("create", Write::Create.title()));
        let default = match store.default_write() {
            Write::Off => "off",
            Write::Update => "update",
            Write::Fallback => "fallback",
            Write::Create => "create",
        };
        let write = crate::notes::write_key(id);
        self.settings.declare(
            crate::settings::SettingSpec::choice(
                &write,
                "Write",
                "Never; only where a file already has notes here; also where a file's volume keeps no attributes; or for every file edited.",
                default,
                &choices,
            )
            .section(title),
        );
        self.notes.declared(owner, write);
        self.notes.register(owner, store);
        self.apply_notes_settings();
    }

    /// Bring the stores' policy in line with the Notes page.
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

    /// Invoke the wake hook, if any.
    pub fn wake(&self) {
        if let Some(w) = self.waker.read().as_ref() {
            w();
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
        self.previews.unregister_owner(owner);
        self.pending.unregister_owner(owner);
        self.player.unregister_owner(owner);
        self.tallies.unregister_owner(owner);
        self.terminals.unregister_owner(owner);
        self.tiles.unregister_owner(owner);
        self.maps.unregister_owner(owner);
        self.registry.remove_owner(owner);
        self.providers.write().retain(|_, (o, _)| &**o != owner);
        for spec in self.settings.schema() {
            if spec.owner() == owner {
                self.settings.undeclare(&spec.key);
            }
        }
    }

    // ------------------------------------------------------------ locations

    /// Open a location without attaching it to a pane.
    pub fn open_location(&self, location: &Location) -> anyhow::Result<Arc<dyn OpenLocation>> {
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

    /// What a pane showing `location` says when it lists nothing
    /// (`LocationProvider::empty_state`).
    pub fn empty_state(&self, location: &Location) -> Option<crate::location::EmptyState> {
        self.provider(&location.scheme)?.empty_state(location)
    }

    /// Where Up goes from `location`: a folder's parent, else the place
    /// before it in its trail. `None` at the top.
    pub fn parent(&self, location: &Location) -> Option<Location> {
        match location.path() {
            Some(path) => path.parent().map(Location::from_path),
            None => self.trail(location)?.parent(),
        }
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
    /// providers keep of it (`LocationProvider::places_of`: its tags,
    /// narrowed as the trail shown has them when it goes on into them,
    /// `along`), else the places its provider says. One answer for both,
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

    /// Open a location in a pane, replacing what it showed. Returns immediately;
    /// entries arrive through `pump`.
    pub fn navigate(&self, pane: PaneId, location: &Location) -> anyhow::Result<()> {
        self.show(pane, location, |p| {
            p.history.truncate(p.history_pos + 1);
            p.history.push(location.clone());
            p.history_pos = p.history.len() - 1;
        })
    }

    /// Show `location` in `pane`, then let `then` update its history. A
    /// location the pane keeps (a column of its view, the folder it just
    /// left) is adopted with its listing as it stands, kept current by its
    /// watcher, instead of listing it anew.
    pub(crate) fn show(
        &self,
        pane: PaneId,
        location: &Location,
        then: impl FnOnce(&mut crate::workspace::Pane),
    ) -> anyhow::Result<()> {
        self.open_in(pane, location, false, then)
    }

    /// Open again what a dormant pane showed (its tab shown again): as
    /// `show`, but what the pane kept while it slept stays, the folders
    /// opened in place (opened again) and its filter and narrowing.
    pub(crate) fn wake_pane(
        &self,
        pane: PaneId,
        location: &Location,
        then: impl FnOnce(&mut crate::workspace::Pane),
    ) -> anyhow::Result<()> {
        self.open_in(pane, location, true, then)?;
        self.open_expanded(pane);
        Ok(())
    }

    fn open_in(
        &self,
        pane: PaneId,
        location: &Location,
        waking: bool,
        then: impl FnOnce(&mut crate::workspace::Pane),
    ) -> anyhow::Result<()> {
        let kept = self
            .workspace
            .write()
            .pane_mut(pane)
            .ok_or_else(|| anyhow::anyhow!("no such pane"))?
            .take_kept(location);
        let (open, listing) = match kept {
            Some(kept) => kept,
            None => (
                self.open_location(location)?,
                crate::Listing::with_companions(Arc::clone(&self.companions)),
            ),
        };
        let mut ws = self.workspace.write();
        let p = ws
            .pane_mut(pane)
            .ok_or_else(|| anyhow::anyhow!("no such pane"))?;
        if waking {
            p.reopen(open, listing);
        } else {
            p.replace_open(open, listing);
        }
        then(p);
        drop(ws);
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
        self.workspace.write().pump()
    }

    /// Like `pump`, and says which panes changed.
    pub fn pump_report(&self) -> crate::workspace::PumpReport {
        self.workspace.write().pump_report()
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
        let open = self.open_location(location)?;
        let deadline = std::time::Instant::now() + 3 * LISTED_WITHIN;
        let mut listing = Listing::default();
        while !listing.is_complete() && listing.error().is_none() {
            let Ok(event) = open.events().recv_deadline(deadline) else {
                break;
            };
            listing.apply(event);
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
        Ok(Outcome::Value(serde_json::json!({
            "location": location,
            "complete": complete,
            "total": order.len(),
            "entries": entries,
        })))
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
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let (reply, answer) = crossbeam_channel::bounded(1);
        self.answers.lock().insert(id, reply);
        self.questions.lock().push(Question {
            id,
            title: title.into(),
            detail: detail.into(),
        });
        self.wake();
        (id, answer)
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
    }

    /// Whether anyone still waits for the answer to `id`.
    pub fn awaited(&self, id: u64) -> bool {
        self.answers.lock().contains_key(&id)
    }

    /// The folders whose files never leave this machine, as the settings
    /// say now (`permissions::Privacy`).
    pub fn privacy(&self) -> crate::permissions::Privacy {
        crate::permissions::Privacy::of(&self.settings)
    }

    /// The entries of `pane` an agent's call must not count (ADR-044):
    /// while one runs on this thread (`permissions::as_agent`), those the
    /// pane lists, shows or holds selected that are in a private folder; none
    /// otherwise, at no cost. A command that answers what it counted or
    /// gathered of a pane's entries (how many, their tags, their dates)
    /// leaves these out. It reads markers from the disk, outside the
    /// workspace's lock: ask before taking it.
    pub fn withheld(&self, pane: PaneId) -> std::collections::HashSet<EntryId> {
        let Some(privacy) = crate::permissions::agent_call() else {
            return std::collections::HashSet::new();
        };
        let paths: Vec<(EntryId, PathBuf)> = {
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
                .filter_map(|id| Some((id, p.path_of(id)?)))
                .collect()
        };
        let mut check = privacy.checker();
        paths
            .into_iter()
            .filter(|(_, path)| check.holds(path))
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
    /// what it showed: Details, the tag editor; a script or an agent, which
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

/// What an undo or a redo did, for a script or an agent: the plan's
/// title under `what`, and how its steps went.
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

#[derive(Debug, Deserialize, JsonSchema)]
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

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SweepArgs {
    /// Deselect what the range covers instead of selecting it.
    #[serde(default)]
    pub unset: bool,
    #[serde(default)]
    pub pane: Option<PaneId>,
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

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DestArgs {
    /// Destination directory, an absolute path.
    pub dest: PathArg,
    /// The entries to act on, by absolute path; the pane's selection when
    /// omitted.
    #[serde(default)]
    pub entries: Vec<op::EntryArg>,
    #[serde(default)]
    pub pane: Option<PaneId>,
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

#[derive(Debug, Deserialize, JsonSchema)]
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
            "core",
            "navigate",
            "Go to location",
            "Open a path or location in a pane",
            |a, app| {
                let pane = app.pane_or_active(a.pane);
                app.navigate(pane, &Location::parse_os(a.location.as_os_str()))?;
                Ok(Outcome::Done)
            },
        )
        .shows();

        c.register::<PaneArg, _>(
            "core",
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
            "refresh",
            "Refresh",
            "Re-list the pane's location",
            |a, app| {
                let pane = app.pane_or_active(a.pane);
                // The folders opened in place are shown too.
                if let Some((open, opened)) =
                    app.with_open(pane, crate::workspace::Pane::opened_locations)
                {
                    open.refresh();
                    for open in opened {
                        open.refresh();
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
            c.register::<HistoryArgs, _>("core", name, title, description, move |a, app| {
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
                    pos.and_then(|pos| Some((pos, p.history.get(pos)?.clone())))
                };
                if let Some((pos, loc)) = target {
                    app.show(pane, &loc, |p| p.history_pos = pos)?;
                }
                Ok(Outcome::Done)
            })
            .shows();
        }

        c.register::<PaneArg, _>(
            "core",
            "up",
            "Parent directory",
            "Go to the parent of the current directory, or to the place before the one shown in its trail (a tag's category, the folder a search looks in)",
            |a, app| {
                let pane = app.pane_or_active(a.pane);
                let shown = app
                    .pane_location(pane)
                    .ok_or_else(|| anyhow::anyhow!("the pane shows nothing"))?;
                if let Some(parent) = app.parent(&shown) {
                    app.navigate(pane, &parent)?;
                }
                Ok(Outcome::Done)
            },
        )
        .shows();

        c.register::<TargetsArgs, _>(
            "core",
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
                if let Some(p) = app.workspace.write().pane_mut(pane) {
                    p.reveal(name.to_owned());
                }
                app.wake();
                Ok(Outcome::Done)
            },
        )
        .shows();

        c.register::<PaneArg, _>(
            "core",
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
                // Names looked up in one pass over the listing: one each
                // was the whole listing's for every name given.
                if !a.names.is_empty() {
                    let mut by_name: HashMap<&std::ffi::OsStr, EntryId> = p
                        .listing
                        .iter()
                        .map(|e| (e.name.as_os_str(), e.id))
                        .collect();
                    for n in &a.names {
                        match by_name
                            .remove(n.as_os_str())
                            .or_else(|| p.listing.find_by_name(n.as_os_str()).map(|e| e.id))
                        {
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
                        match (pos(anchor), pos(target)) {
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
                            p.selection.follow(target, &order);
                        }
                    }
                    SelectMode::Pick => ids.into_iter().for_each(|id| p.selection.pick(id)),
                }
                Ok(Outcome::Done)
            },
        )
        .shows();

        c.register::<SortArgs, _>(
            "core",
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
        )
        .shows();

        c.register::<FilterArgs, _>(
            "core",
            "filter",
            "Filter by name",
            "Show only the entries whose name holds `text` (case ignored unless it has a capital), until the pane shows another location; without `text`, all of them",
            |a, app| {
                let pane = app.pane_or_active(a.pane);
                if let Some(p) = app.workspace.write().pane_mut(pane) {
                    p.set_filter(a.text.as_deref());
                }
                app.wake();
                Ok(Outcome::Done)
            },
        )
        .shows();

        c.register::<PaneArg, _>(
            "core",
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

        c.register::<SweepArgs, _>(
            "core",
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
                p.selection.sweep_start(a.unset, &order);
                Ok(Outcome::Done)
            },
        )
        .shows();

        c.register::<PaneArg, _>(
            "core",
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
            "core",
            "entries",
            "List entries",
            "What a pane shows, in its order (sorted, filtered, with the folders opened in place), or with `location` what a folder holds, listed without showing it: each entry's id, name, path, kind, size and modification time (seconds since 1970), and whether it is selected; from `offset`, at most `limit`. `complete: false`: the listing was still arriving, ask again",
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
                Ok(Outcome::Value(serde_json::json!({
                    "pane": pane,
                    "location": p.shown(),
                    "complete": p.listing.is_complete(),
                    "total": order.len(),
                    "selected": p.selection.iter().filter(|id| !withheld.contains(id)).count(),
                    "entries": entries,
                })))
            },
        )
        .reads();

        c.register::<RenameArgs, _>(
            "core",
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

        c.register::<DestArgs, _>(
            "core",
            "move",
            "Move",
            "Move the selection, or the entries given, to a directory (plan)",
            |a, app| {
                let dest = destination(&a.dest)?;
                targets_plan(
                    app,
                    a.pane,
                    a.entries,
                    &format!("move to {}", dest.display()),
                    |e| {
                        Box::new(op::Move {
                            entry: e,
                            dest_dir: dest.clone(),
                        })
                    },
                )
            },
        )
        .plans();

        c.register::<DestArgs, _>(
            "core",
            "copy",
            "Copy",
            "Copy the selection, or the entries given, to a directory (plan)",
            |a, app| {
                let dest = destination(&a.dest)?;
                targets_plan(
                    app,
                    a.pane,
                    a.entries,
                    &format!("copy to {}", dest.display()),
                    |e| Box::new(op::Copy::from_settings(e, dest.clone(), &app.settings)),
                )
            },
        )
        .plans();

        c.register::<DestArgs, _>(
            "core",
            "link",
            "Link",
            "Make a link in a directory to each of the selection, or the entries given, under its name (plan)",
            |a, app| {
                let dest = destination(&a.dest)?;
                targets_plan(
                    app,
                    a.pane,
                    a.entries,
                    &format!("link in {}", dest.display()),
                    |e| {
                        Box::new(op::Link {
                            entry: e,
                            dest_dir: dest.clone(),
                        })
                    },
                )
            },
        )
        .plans();

        c.register::<TargetsArgs, _>(
            "core",
            "trash",
            "Move to trash",
            "Trash the selection, or the entries given (plan)",
            |a, app| {
                targets_plan(app, a.pane, a.entries, "move to trash", |e| {
                    Box::new(op::Trash { entry: e })
                })
            },
        )
        .plans();

        c.register::<TargetsArgs, _>(
            "core",
            "delete",
            "Delete permanently",
            "Delete the selection, or the entries given, without trash (plan, irreversible)",
            |a, app| {
                targets_plan(app, a.pane, a.entries, "delete permanently", |e| {
                    Box::new(op::Delete { entry: e })
                })
            },
        )
        .plans();

        c.register::<MkdirArgs, _>(
            "core",
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
            "core",
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
        .reads();

        c.register::<ApplyPlanArgs, _>(
            "core",
            "apply_plan",
            "Apply plan",
            "Apply a plan given as data (plan): as any command's, the caller applies it, at once for the user's own gesture, after review for an agent",
            |a, app| Ok(Outcome::Plan(Plan::from_data(&a.plan, &app.ops)?)),
        )
        .plans();

        c.register::<NoArgs, _>(
            "core",
            "undo",
            "Undo",
            "Undo the most recent plan; asked from outside the window, says what it took back",
            |_, app| {
                if crate::scheduler::on_window_thread() {
                    anyhow::ensure!(app.executor.undo(app.lifetime().child()), "nothing to undo");
                    return Ok(Outcome::Done);
                }
                // A script or an agent waits, and is told what it took back.
                let result = app
                    .executor
                    .undo_blocking(&app.lifetime().child())
                    .ok_or_else(|| anyhow::anyhow!("nothing to undo"))?;
                Ok(replayed(
                    "undone",
                    app.executor.last_redo_title().as_deref(),
                    &result,
                ))
            },
        );

        c.register::<NoArgs, _>(
            "core",
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

        c.register::<NoArgs, _>(
            "core",
            "commands",
            "List commands",
            "Return every registered command with its argument schema",
            |_, app| Ok(Outcome::Value(serde_json::to_value(app.commands.list())?)),
        )
        .reads();
    }
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
}
