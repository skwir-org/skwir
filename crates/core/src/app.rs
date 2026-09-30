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
use crate::entry::EntryId;
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
    pub previews: Arc<crate::preview::Previews>,
    /// Where map backgrounds come from (the map-tiles plugin, ADR-020).
    pub tiles: Arc<crate::tiles::Tiles>,
    /// Plans proposed for review (by plugins whose work finishes later, or by
    /// agents). The UI drains them with `take_proposals`.
    proposals: parking_lot::Mutex<Vec<Plan>>,
    /// Messages from background work for the UI to show.
    notifications: parking_lot::Mutex<Vec<Notification>>,
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
            Some(p) => Settings::load(p)?,
            None => Settings::in_memory(),
        };
        let cache = Cache::new(config.cache_root);
        let fields = Arc::new(crate::fields::Fields::new(cache.namespace("fields")));
        let app = Arc::new_cyclic(|weak| App {
            weak: weak.clone(),
            notifications: parking_lot::Mutex::new(Vec::new()),
            closed_tabs: parking_lot::Mutex::default(),
            executor: Executor::new(scheduler.clone()),
            thumbnails: crate::thumbs::Thumbnails::with_lifetime(
                scheduler.clone(),
                lifetime.child(),
            ),
            metadata: Arc::new(crate::metadata::MetadataProviders::default()),
            fields,
            hints: Arc::new(crate::hints::Hints::default()),
            previews: Arc::new(crate::preview::Previews::default()),
            tiles: Arc::new(crate::tiles::Tiles::default()),
            proposals: parking_lot::Mutex::new(Vec::new()),
            scheduler,
            commands: CommandRegistry::default(),
            ops: OpRegistry::default(),
            registry: Registry::default(),
            plugins: PluginHost::default(),
            settings,
            cache,
            workspace: RwLock::new(Workspace::default()),
            pending: crate::pending::PendingTransfer::default(),
            player: crate::playing::Player::default(),
            tallies: crate::tally::Tallies::default(),
            terminals: crate::terminal::Terminals::default(),
            providers: RwLock::new(HashMap::new()),
            waker: Arc::default(),
            batch_size: config.batch_size,
            lifetime,
        });
        app.register_provider("core", Arc::new(PathProvider));
        op::register_builtin(&app.ops);
        builtin::register(&app);
        crate::tabs::register(&app);
        crate::keymap::declare(&app.settings);
        crate::settings::register(&app);
        crate::plugin::register(&app);
        crate::playing::register(&app);
        Ok(app)
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
        self.previews.unregister_owner(owner);
        self.pending.unregister_owner(owner);
        self.player.unregister_owner(owner);
        self.tallies.unregister_owner(owner);
        self.terminals.unregister_owner(owner);
        self.tiles.unregister_owner(owner);
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
            },
        )
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
        let kept = self
            .workspace
            .write()
            .pane_mut(pane)
            .ok_or_else(|| anyhow::anyhow!("no such pane"))?
            .take_kept(location);
        let (open, listing) = match kept {
            Some(kept) => kept,
            None => (self.open_location(location)?, crate::Listing::default()),
        };
        let mut ws = self.workspace.write();
        let p = ws
            .pane_mut(pane)
            .ok_or_else(|| anyhow::anyhow!("no such pane"))?;
        p.replace_open(open, listing);
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
                        p.add_kept(open);
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

    /// Offer a plan for the user to review. Never applies anything; wakes the UI.
    pub fn propose(&self, plan: Plan) {
        self.proposals.lock().push(plan);
        self.wake();
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

    /// Entry references for the current selection of a pane, in selection order.
    pub fn selected_refs(&self, pane: PaneId) -> Vec<EntryRef> {
        let Some((open, ids)) = self.with_open(pane, |p| {
            p.selection
                .iter()
                .map(|id| (id, p.opened_path(id)))
                .collect::<Vec<_>>()
        }) else {
            return Vec::new();
        };
        ids.into_iter()
            .filter_map(|(id, opened)| {
                opened
                    .or_else(|| open.path_of(id))
                    .map(|path| EntryRef::new(id, path))
            })
            .collect()
    }

    /// `selected_refs`, each with the name that says its type
    /// (`Entry::type_name`: a link's target's), as the listings know it:
    /// for a caller that may not touch the disk. `None` for what its name
    /// does not type (`Entry::inode_type`: a folder, a pipe, a device, a
    /// link that leads nowhere). A worker holding only a path asks
    /// `fs::content_path`.
    pub fn selected_typed(&self, pane: PaneId) -> Vec<(EntryRef, Option<std::ffi::OsString>)> {
        let Some((open, ids)) = self.with_open(pane, |p| {
            p.selection
                .iter()
                .map(|id| {
                    let type_name = p
                        .entry(id)
                        .map(|e| e.inode_type().is_none().then(|| e.type_name().to_owned()));
                    (id, p.opened_path(id), type_name)
                })
                .collect::<Vec<_>>()
        }) else {
            return Vec::new();
        };
        ids.into_iter()
            .filter_map(|(id, opened, type_name)| {
                let path = opened.or_else(|| open.path_of(id))?;
                let type_name = type_name
                    .unwrap_or_else(|| path.file_name().map(std::ffi::OsStr::to_os_string));
                Some((EntryRef::new(id, path), type_name))
            })
            .collect()
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
                        p.add_kept(open);
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
        let showing: Vec<PaneId> = self
            .workspace
            .read()
            .panes()
            .iter()
            .filter(|p| {
                p.location()
                    .and_then(Location::path)
                    .is_some_and(|shown| shown.starts_with(path))
            })
            .map(|p| p.id)
            .collect();
        for pane in showing {
            self.navigate(pane, to)?;
        }
        let released: Vec<_> = self
            .workspace
            .write()
            .panes_mut()
            .iter_mut()
            .flat_map(|p| p.release_under(path))
            .collect();
        // Closed here, outside the lock: closing runs the location's code.
        drop(released);
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
    /// `pane` argument means.
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

    /// A pane's open location and a snapshot taken with it, outside the
    /// workspace lock: the location's methods are plugin code that may
    /// take other locks, send and wake.
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
    #[serde(default)]
    pub pane: Option<PaneId>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DestArgs {
    /// Destination directory.
    pub dest: PathArg,
    #[serde(default)]
    pub pane: Option<PaneId>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MkdirArgs {
    pub name: PathArg,
    #[serde(default)]
    pub pane: Option<PaneId>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct HydrateArgs {
    pub ids: Vec<EntryId>,
    #[serde(default)]
    pub pane: Option<PaneId>,
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

    fn selection_plan(
        app: &App,
        pane: PaneId,
        title: &str,
        f: impl Fn(EntryRef) -> Box<dyn op::Operation>,
    ) -> anyhow::Result<Outcome> {
        let refs = app.selected_refs(pane);
        if refs.is_empty() {
            return Err(crate::command::NothingSelected.into());
        }
        Ok(Outcome::Plan(Plan::with_steps(
            title,
            refs.into_iter().map(f).collect(),
        )))
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
        );

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
                    .find(|k| *k.scheme == *location.scheme)
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
        );

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
            });
        }

        c.register::<PaneArg, _>(
            "core",
            "up",
            "Parent directory",
            "Go to the parent of the current directory",
            |a, app| {
                let pane = app.pane_or_active(a.pane);
                let dir = app.pane_dir(pane)?;
                if let Some(parent) = dir.parent() {
                    app.navigate(pane, &Location::from_path(parent))?;
                }
                Ok(Outcome::Done)
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
                for n in &a.names {
                    match p.listing.find_by_name(n.as_os_str()) {
                        Some(e) => ids.push(e.id),
                        None => anyhow::bail!("no entry named `{}`", n.display()),
                    }
                }
                match a.mode {
                    SelectMode::Replace => p.selection.set(ids),
                    SelectMode::Add => ids.into_iter().for_each(|id| p.selection.add(id)),
                    SelectMode::Toggle => ids.into_iter().for_each(|id| p.selection.toggle(id)),
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
        );

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
                    // A rating is sought from the best down.
                    p.sort.ascending = a.ascending.unwrap_or(if p.sort.key == k {
                        !p.sort.ascending
                    } else {
                        k != crate::location::SortKey::Rating
                    });
                    p.sort.key = k;
                } else if let Some(asc) = a.ascending {
                    p.sort.ascending = asc;
                }
                if let Some(h) = a.show_hidden {
                    p.sort.show_hidden = h;
                }
                Ok(Outcome::Done)
            },
        );

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
        );

        c.register::<PaneArg, _>(
            "core",
            "toggle_hidden",
            "Toggle hidden files",
            "Show or hide dot-prefixed entries",
            |a, app| {
                let pane = app.pane_or_active(a.pane);
                if let Some(p) = app.workspace.write().pane_mut(pane) {
                    p.sort.show_hidden = !p.sort.show_hidden;
                }
                Ok(Outcome::Done)
            },
        );

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
        );

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
        );

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
        );

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
        );

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
        );

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
                // Which of them are folders, and where: the shown
                // location's own are asked outside the lock.
                let Some((open, folders)) = app.with_open(pane, |p| {
                    let ids: Vec<EntryId> = if a.ids.is_empty() {
                        p.selection.iter().collect()
                    } else {
                        a.ids.clone()
                    };
                    ids.into_iter()
                        .filter(|id| p.entry(*id).is_some_and(crate::Entry::is_dir))
                        .map(|id| (id, p.opened_path(id)))
                        .collect::<Vec<_>>()
                }) else {
                    anyhow::bail!("the pane shows nothing");
                };
                let paths: Vec<PathBuf> = folders
                    .into_iter()
                    .filter_map(|(id, opened)| opened.or_else(|| open.path_of(id)))
                    .collect();
                if paths.is_empty() {
                    return Err(crate::command::NothingSelected.into());
                }
                {
                    let mut ws = app.workspace.write();
                    let p = ws
                        .pane_mut(pane)
                        .ok_or_else(|| anyhow::anyhow!("no such pane"))?;
                    for path in paths {
                        let open = p.is_expanded(&path);
                        match a.mode {
                            ExpandMode::Expand | ExpandMode::Toggle if !open => {
                                p.expand(path);
                            }
                            ExpandMode::Collapse | ExpandMode::Toggle if open => {
                                p.collapse(&path);
                            }
                            _ => {}
                        }
                    }
                }
                app.open_expanded(pane);
                app.wake();
                Ok(Outcome::Done)
            },
        );

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
                for (open, ids) in groups {
                    open.hydrate(&ids, Priority::Visible);
                }
                Ok(Outcome::Done)
            },
        );

        c.register::<RenameArgs, _>(
            "core",
            "rename",
            "Rename",
            "Rename the selected entry (plan)",
            |a, app| {
                let pane = app.pane_or_active(a.pane);
                let refs = app.selected_refs(pane);
                let [r] = refs.as_slice() else {
                    anyhow::bail!("rename needs exactly one selected entry")
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
        );

        c.register::<DestArgs, _>(
            "core",
            "move",
            "Move",
            "Move the selection to a directory (plan)",
            |a, app| {
                let pane = app.pane_or_active(a.pane);
                let dest = Location::parse_os(a.dest.as_os_str())
                    .path()
                    .ok_or_else(|| anyhow::anyhow!("destination must be a path"))?;
                selection_plan(app, pane, &format!("move to {}", dest.display()), |e| {
                    Box::new(op::Move {
                        entry: e,
                        dest_dir: dest.clone(),
                    })
                })
            },
        );

        c.register::<DestArgs, _>(
            "core",
            "copy",
            "Copy",
            "Copy the selection to a directory (plan)",
            |a, app| {
                let pane = app.pane_or_active(a.pane);
                let dest = Location::parse_os(a.dest.as_os_str())
                    .path()
                    .ok_or_else(|| anyhow::anyhow!("destination must be a path"))?;
                selection_plan(app, pane, &format!("copy to {}", dest.display()), |e| {
                    Box::new(op::Copy::from_settings(e, dest.clone(), &app.settings))
                })
            },
        );

        c.register::<PaneArg, _>(
            "core",
            "trash",
            "Move to trash",
            "Trash the selection (plan)",
            |a, app| {
                let pane = app.pane_or_active(a.pane);
                selection_plan(app, pane, "move to trash", |e| {
                    Box::new(op::Trash { entry: e })
                })
            },
        );

        c.register::<PaneArg, _>(
            "core",
            "delete",
            "Delete permanently",
            "Delete the selection without trash (plan, irreversible)",
            |a, app| {
                let pane = app.pane_or_active(a.pane);
                selection_plan(app, pane, "delete permanently", |e| {
                    Box::new(op::Delete { entry: e })
                })
            },
        );

        c.register::<MkdirArgs, _>(
            "core",
            "mkdir",
            "New folder",
            "Create a folder in the current directory (plan)",
            |a, app| {
                let pane = app.pane_or_active(a.pane);
                let dir = app.pane_dir(pane)?;
                let name = a.name.into_inner().into_os_string();
                op::valid_file_name(&name)?;
                Ok(Outcome::Plan(Plan::with_steps(
                    format!("new folder {}", name.display()),
                    vec![Box::new(op::Mkdir {
                        path: dir.join(name),
                    })],
                )))
            },
        );

        c.register::<MkdirArgs, _>(
            "core",
            "new_file",
            "New file",
            "Create an empty file in the current directory (plan)",
            |a, app| {
                let pane = app.pane_or_active(a.pane);
                let dir = app.pane_dir(pane)?;
                let name = a.name.into_inner().into_os_string();
                op::valid_file_name(&name)?;
                Ok(Outcome::Plan(Plan::with_steps(
                    format!("new file {}", name.display()),
                    vec![Box::new(op::NewFile {
                        path: dir.join(name),
                    })],
                )))
            },
        );

        c.register::<PlanDiffArgs, _>(
            "core",
            "plan_diff",
            "Preview plan",
            "Dry-run a plan and return what each step would do",
            |a, app| {
                let plan = Plan::from_data(&a.plan, &app.ops)?;
                Ok(Outcome::Value(serde_json::to_value(plan.diff())?))
            },
        );

        c.register::<ApplyPlanArgs, _>(
            "core",
            "apply_plan",
            "Apply plan",
            "Execute a plan in the background; progress arrives as executor events",
            |a, app| {
                let plan = Plan::from_data(&a.plan, &app.ops)?;
                app.apply(plan);
                Ok(Outcome::Done)
            },
        );

        c.register::<NoArgs, _>(
            "core",
            "undo",
            "Undo",
            "Undo the most recent plan",
            |_, app| {
                if !app.executor.undo(app.lifetime().child()) {
                    anyhow::bail!("nothing to undo");
                }
                Ok(Outcome::Done)
            },
        );

        c.register::<NoArgs, _>(
            "core",
            "redo",
            "Redo",
            "Redo the most recently undone plan",
            |_, app| {
                if !app.executor.redo(app.lifetime().child()) {
                    anyhow::bail!("nothing to redo");
                }
                Ok(Outcome::Done)
            },
        );

        c.register::<NoArgs, _>(
            "core",
            "commands",
            "List commands",
            "Return every registered command with its argument schema",
            |_, app| Ok(Outcome::Value(serde_json::to_value(app.commands.list())?)),
        );
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
