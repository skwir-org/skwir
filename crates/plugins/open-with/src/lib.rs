//! skwir plugin: open-with. Opens entries with the default application,
//! enters directories, opens a terminal in the current directory, and opens
//! files or folders with an application the user chooses.
//!
//! The default application is the catalog's (`Catalog::default_for_lineage`,
//! the desktop's rule), for Open as for the menu that names it: not
//! `xdg-open`'s, which some desktops reduce to a simpler rule that ignores
//! the applications the user added for a type. `xdg-open` opens only what
//! the catalog has no application for.
//!
//! Choosing is the shell's `ui.choose` picker: `open_with.choices` lists
//! the applications for the selection, each choice an `open_with` that
//! names its application, pane and entries. The desktop's catalog of
//! applications is read on a worker and kept until an install or a changed
//! default makes it stale (`skwir_platform::apps::Stamp`).

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use parking_lot::Mutex;
use skwir_core::app::{App, NotifyLevel, PaneArg};
use skwir_core::cancel::CancelToken;
use skwir_core::command::{Invocation, Outcome, Topic};
use skwir_core::entry::{EntryId, EntryKind};
use skwir_core::fs::reach;
use skwir_core::location::Location;
use skwir_core::os_path::PathArg;
use skwir_core::plugin::Plugin;
use skwir_core::registry::{KeyBinding, MenuItem, MenuSource};
use skwir_core::scheduler::{Lane, Priority, on_window_thread};
use skwir_core::workspace::PaneId;
use skwir_platform::apps::{Application, AssociationEdit, Catalog, Stamp};
use skwir_platform::mime::{self, Mime};

const ID: &str = "open-with";

/// Where the applications are read from, and the catalog last read with
/// the stamp it was read at.
#[derive(Debug, Default)]
struct CatalogCache {
    /// Explicit applications directories and lists, most important first,
    /// the lists also where defaults are written (tests); the desktop's
    /// when `None`.
    sources: Option<(Vec<PathBuf>, Vec<PathBuf>)>,
    last: Mutex<Option<(Stamp, Arc<Catalog>)>>,
    /// A worker is reading the catalog again for the menu (`refresh`).
    refreshing: AtomicBool,
    /// The revision of Properties' Opens with (`open_with.properties`),
    /// moved when a default changes.
    properties: Arc<std::sync::atomic::AtomicU64>,
    /// What that worker lives for: until the plugin is disabled.
    work: Mutex<Option<CancelToken>>,
}

type Cached = Arc<CatalogCache>;

impl CatalogCache {
    fn stamp(&self) -> Stamp {
        match &self.sources {
            Some((dirs, lists)) => Stamp::of(dirs, lists),
            None => Stamp::current(),
        }
    }

    fn load(&self) -> Catalog {
        match &self.sources {
            Some((dirs, lists)) => Catalog::from_paths(dirs, lists),
            None => Catalog::load(),
        }
    }

    /// The catalog last read, as it was: no stat, for the menu, which
    /// cannot wait on the disk.
    fn remembered(&self) -> Option<Arc<Catalog>> {
        self.last
            .lock()
            .as_ref()
            .map(|(_, catalog)| Arc::clone(catalog))
    }

    /// The lists a default may be written to.
    fn user_lists(&self) -> Vec<PathBuf> {
        match &self.sources {
            Some((_, lists)) => lists.clone(),
            None => skwir_platform::apps::user_lists(),
        }
    }
}

/// The open-with plugin.
#[derive(Debug, Default)]
pub struct OpenWithPlugin {
    catalog: Cached,
}

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
struct OpenArgs {
    #[serde(default)]
    pane: Option<PaneId>,
    /// Entries to open; the selection when omitted.
    #[serde(default)]
    ids: Vec<EntryId>,
    /// Files to open, by absolute path, instead of a pane's entries.
    #[serde(default)]
    entries: Vec<skwir_core::op::EntryArg>,
}

fn open(args: OpenArgs, cache: &Cached, app: &App) -> anyhow::Result<Outcome> {
    let pane = app.pane_or_active(args.pane);
    if on_window_thread() && !args.entries.is_empty() {
        let lifetime = pane_lifetime(app, pane)?;
        let cache = Arc::clone(cache);
        let cancel = lifetime.clone();
        resolve_named(app, args.entries, &lifetime, move |app, targets| {
            open_targets(pane, targets, &[], cancel, &cache, app).map(|_| ())
        })?;
        return Ok(Outcome::Done);
    }
    // Resolve the targets while holding the read lock, then release it;
    // files named by path are read without it.
    let (targets, places, lifetime): (Vec<Target>, Vec<Location>, _) = if args.entries.is_empty() {
        let ws = app.workspace.read();
        let p = ws
            .pane(pane)
            .ok_or_else(|| anyhow::anyhow!("no such pane"))?;
        anyhow::ensure!(p.open.is_some(), "pane has no location");
        let ids: Vec<EntryId> = if args.ids.is_empty() {
            p.selection.iter().collect()
        } else {
            args.ids
        };
        // A place (a tag's, in a tag location) opens where it leads, as a
        // folder that does not open at its path does (a trashed one).
        let places = ids.iter().filter_map(|id| p.location_of(*id)).collect();
        let targets = ids
            .into_iter()
            .filter(|id| p.location_of(*id).is_none())
            .filter_map(|id| {
                let entry = p.entry(id)?;
                let path = p.path_of(id)?;
                Some(Target {
                    path,
                    kind: entry.kind,
                    entry: Some(entry.clone()),
                    typed: None,
                })
            })
            .collect();
        (targets, places, p.location_lifetime())
    } else {
        (
            named_targets(args.entries)?,
            Vec::new(),
            app.lifetime().child(),
        )
    };
    open_targets(pane, targets, &places, lifetime, cache, app)
}

fn pane_lifetime(app: &App, pane: PaneId) -> anyhow::Result<CancelToken> {
    app.workspace
        .read()
        .pane(pane)
        .map(skwir_core::workspace::Pane::location_lifetime)
        .ok_or_else(|| anyhow::anyhow!("no such pane"))
}

fn open_targets(
    pane: PaneId,
    targets: Vec<Target>,
    places: &[Location],
    lifetime: CancelToken,
    cache: &Cached,
    app: &App,
) -> anyhow::Result<Outcome> {
    if lifetime.is_cancelled() {
        return Ok(Outcome::Done);
    }
    if targets.is_empty() {
        let [place] = places else {
            anyhow::bail!("nothing to open");
        };
        app.navigate(pane, place)?;
        return Ok(Outcome::Done);
    }
    match targets.as_slice() {
        [
            Target {
                kind: EntryKind::Directory,
                path,
                ..
            },
        ] => {
            app.navigate(pane, &Location::from_path(path))?;
            return Ok(Outcome::Done);
        }
        // A link may lead to a folder, which opens here like one; asking
        // follows the link, which can wait on a sleeping disk, so a worker
        // asks. Navigating away first cancels it.
        [
            Target {
                kind: EntryKind::Symlink | EntryKind::Unknown,
                path,
                ..
            },
        ] => {
            let lane = targets[0].lane();
            let path = path.clone();
            let weak = app.weak();
            let cancel = lifetime.clone();
            let cache = Arc::clone(cache);
            app.scheduler.spawn_in(
                lane,
                Priority::Normal,
                "open-with.resolve",
                lifetime,
                move || {
                    let Some(app) = weak.upgrade() else { return };
                    if path.is_dir() {
                        if cancel.is_cancelled() {
                            return;
                        }
                        match app.navigate(pane, &Location::from_path(&path)) {
                            Ok(()) => app.wake(),
                            Err(e) => app.notify(NotifyLevel::Error, format!("{e:#}")),
                        }
                    } else {
                        launch_defaults(&app, &cache, targets, &cancel);
                    }
                },
            );
            return Ok(Outcome::Done);
        }
        _ => {}
    }
    // Typing a file can read its head, and starting a program touches the
    // disk.
    let cache = Arc::clone(cache);
    for (lane, targets) in by_lane(targets, Target::lane) {
        let cache = Arc::clone(&cache);
        let cancel = lifetime.clone();
        spawn_in(app, lane, "open-with.open", cancel.clone(), move |app| {
            launch_defaults(app, &cache, targets, &cancel);
        });
    }
    Ok(Outcome::Done)
}

/// Open each target with the application the catalog chooses for its type
/// (what the menu names), the files of one application in one run, as
/// its command line takes them; what no application opens, with the
/// desktop's opener. On a worker: reads the files' heads.
fn launch_defaults(app: &App, cache: &Cached, targets: Vec<Target>, cancel: &CancelToken) {
    let catalog = catalog(cache);
    let mut runs: Vec<(&Application, Vec<PathBuf>)> = Vec::new();
    let mut rest: Vec<PathBuf> = Vec::new();
    for target in targets {
        if cancel.is_cancelled() {
            return;
        }
        // A pipe, a socket or a device is never opened: a pipe would wait
        // for a writer, a device may act.
        if target
            .entry
            .as_ref()
            .is_some_and(|e| e.opens().is_special())
        {
            let name = target
                .path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy();
            app.notify(
                NotifyLevel::Info,
                format!("“{name}” is not a file: skwir does not open it"),
            );
            continue;
        }
        let lineage = mime::lineage(&target.mime());
        match catalog.default_for_lineage(&lineage) {
            Some(application) => match runs.iter_mut().find(|(a, _)| a.id == application.id) {
                Some((_, paths)) => paths.push(target.path),
                None => runs.push((application, vec![target.path])),
            },
            None => rest.push(target.path),
        }
    }
    if cancel.is_cancelled() {
        return;
    }
    let started = runs
        .iter()
        .map(|(application, paths)| application.launch(paths))
        .chain(
            rest.iter()
                .map(|path| skwir_platform::open::open_path(path)),
        );
    for e in started.filter_map(Result::err) {
        app.notify(NotifyLevel::Error, format!("{e:#}"));
    }
}

/// The directory a "here" action refers to: the single selected directory if
/// there is exactly one, otherwise the pane's current directory.
fn target_dir(pane: PaneId, app: &App) -> anyhow::Result<std::path::PathBuf> {
    let ws = app.workspace.read();
    let p = ws
        .pane(pane)
        .ok_or_else(|| anyhow::anyhow!("no such pane"))?;
    if p.selection.len() == 1
        && let Some(id) = p.selection.iter().next()
        && let Some(e) = p.entry(id)
        && e.is_dir()
        && let Some(path) = p.path_of(id)
    {
        return Ok(path);
    }
    drop(ws);
    // The folder shown, or the one holding the selection in a search.
    app.work_dir(pane)
}

fn open_terminal(pane: Option<PaneId>, app: &App) -> anyhow::Result<Outcome> {
    let dir = target_dir(app.pane_or_active(pane), app)?;
    // A program started in a folder waits on its disk (the spawn returns
    // once the child is in it), a server's for a remote folder: from the
    // window, on a worker in the folder's lane, a failure said.
    if skwir_core::scheduler::on_window_thread() {
        let lane = skwir_core::fs::reach::lane(&dir);
        spawn_in(
            app,
            lane,
            "open-with.terminal",
            app.lifetime().child(),
            move |app| {
                if let Err(e) = skwir_platform::open::open_terminal(&dir) {
                    app.notify(NotifyLevel::Error, format!("{e:#}"));
                }
            },
        );
    } else {
        skwir_platform::open::open_terminal(&dir)?;
    }
    Ok(Outcome::Done)
}

/// The applications, read again only when the stamp moved. Stats the
/// applications directories and lists, and reads them when they changed:
/// call from a worker.
fn catalog(cache: &Cached) -> Arc<Catalog> {
    let stamp = cache.stamp();
    if let Some((at, catalog)) = &*cache.last.lock()
        && *at == stamp
    {
        return Arc::clone(catalog);
    }
    let catalog = Arc::new(cache.load());
    *cache.last.lock() = Some((stamp, Arc::clone(&catalog)));
    catalog
}

/// Read the catalog again on a worker if it changed, once at a time: what
/// the next menu names. An install or a default chosen elsewhere shows
/// from the menu after the one that noticed it.
fn refresh(cache: &Cached, app: &App) {
    if cache.refreshing.swap(true, Ordering::AcqRel) {
        return;
    }
    let token = cache
        .work
        .lock()
        .get_or_insert_with(|| app.lifetime().child())
        .clone();
    let cache = Arc::clone(cache);
    let job = token.clone();
    app.scheduler.spawn(
        Priority::Background,
        "open-with.catalog",
        token,
        move || {
            catalog(&cache);
            if job.is_cancelled() {
                // Disabled while it read: keep nothing.
                cache.last.lock().take();
            }
            cache.refreshing.store(false, Ordering::Release);
        },
    );
}

/// `open_with.menu`: the context menu's Open for the selection, from the
/// catalog in memory. Files whose names say their types, sharing one
/// default application: "Open With" it, the application `open` starts
/// (which types a file by its bytes where the name is ambiguous, `.ogg`);
/// the picker stays Open With….
/// Anything else (a folder, a type the name does not say, types with no
/// common default, the catalog not read yet) answers no items, and the
/// menu keeps its plain Open.
fn menu(pane: Option<PaneId>, cache: &Cached, app: &App) -> anyhow::Result<Outcome> {
    let nothing = || Ok(Outcome::Value(serde_json::json!({ "items": [] })));
    let pane = app.pane_or_active(pane);
    let catalog = cache.remembered();
    refresh(cache, app);
    let Some(catalog) = catalog else {
        return nothing();
    };
    let names: Vec<std::ffi::OsString> = {
        let ws = app.workspace.read();
        let Some(p) = ws.pane(pane) else {
            return nothing();
        };
        let mut names = Vec::with_capacity(p.selection.len());
        for id in p.selection.iter() {
            match p.entry(id) {
                // A link opens what it leads to, typed by its name; a
                // pipe or a device is no file an application opens.
                Some(e) if e.opens() == EntryKind::File => {
                    names.push(e.type_name().to_owned());
                }
                _ => return nothing(),
            }
        }
        names
    };
    let mut types: Vec<Mime> = Vec::new();
    for m in mime::by_names(names.iter().map(std::ffi::OsString::as_os_str)) {
        let Some(m) = m else { return nothing() };
        if !types.contains(&m) {
            types.push(m);
        }
    }
    let open_with = |application: &Application| {
        Invocation::with_args(
            "open_with",
            &serde_json::json!({ "app": application.id, "pane": pane }),
        )
    };
    // The default the files share opens them first; without one, Open
    // stays, said again since this answer takes its place.
    let ranked = rank(&catalog, &types);
    let default = ranked
        .iter()
        .find(|(_, _, is_default)| *is_default)
        .map(|(application, _, _)| *application);
    let mut items = vec![match default {
        Some(default) => MenuItem::new(
            "context",
            format!("Open With {}", default.name),
            "open",
            0,
            open_with(default),
        ),
        None => MenuItem::new("context", "Open", "open", 0, Invocation::new("open")),
    }];
    // The other applications for their types, best fit first, in the
    // Open With submenu before the chooser.
    items.extend(
        ranked
            .iter()
            .filter(|(_, fit, is_default)| fit.is_some() && !is_default)
            .take(OPEN_WITH_APPS)
            .map(|(application, _, _)| {
                MenuItem::new(
                    "context",
                    application.name.clone(),
                    "",
                    5,
                    open_with(application),
                )
                .submenu(OPEN_WITH)
            }),
    );
    Ok(Outcome::Value(serde_json::json!({ "items": items })))
}

/// The context menu's submenu of what opens the selection (ADR-099).
const OPEN_WITH: &str = "Open With";

/// The same, on the folder's background, for the folder.
const OPEN_FOLDER_WITH: &str = "Open Folder With";

/// The most applications the Open With submenu names; the chooser has
/// them all.
const OPEN_WITH_APPS: usize = 8;

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
struct TargetArgs {
    #[serde(default)]
    pane: Option<PaneId>,
    /// Entries to open; the selection when omitted.
    #[serde(default)]
    ids: Vec<EntryId>,
    /// Files to open, by absolute path, instead of a pane's entries.
    #[serde(default)]
    entries: Vec<skwir_core::op::EntryArg>,
    /// The pane's folder itself, not entries in it.
    #[serde(default)]
    here: bool,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct OpenWithArgs {
    /// The application's desktop file ID (`org.gnome.Loupe.desktop`), as
    /// `open_with.choices` lists them.
    app: String,
    /// Also make it the default for the entries' type (they must share one).
    #[serde(default)]
    set_default: bool,
    #[serde(flatten)]
    target: TargetArgs,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct SetDefaultArgs {
    /// The type, as `image/png`, or `inode/directory` for folders.
    mime: String,
    /// The application's desktop file ID.
    app: String,
}

/// An [`AssociationEdit`] as a command argument: the lines one type has in
/// one `mimeapps.list`, each `None` for no line.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct EditArgs {
    file: PathArg,
    mime: String,
    #[serde(default)]
    default: Option<String>,
    #[serde(default)]
    added: Option<String>,
    #[serde(default)]
    removed: Option<String>,
    /// This takes back an earlier edit: the notification offers Redo.
    #[serde(default)]
    undo: bool,
}

/// What is opened: a path, what is known of it, and the entry it is
/// (none for the folder a pane shows, which is opened as a folder).
struct Target {
    path: PathBuf,
    kind: EntryKind,
    entry: Option<skwir_core::Entry>,
    /// A type read in the target's lane, for a choice spanning mounts.
    typed: Option<Mime>,
}

/// The pane the targets are in, the entries named (for the command a
/// choice runs), and the targets themselves. Holds the workspace lock only
/// to read them.
fn targets(args: TargetArgs, app: &App) -> anyhow::Result<(PaneId, Vec<EntryId>, Vec<Target>)> {
    let pane = app.pane_or_active(args.pane);
    if args.here {
        let path = app.pane_dir(pane)?;
        let target = Target {
            path,
            kind: EntryKind::Directory,
            entry: None,
            typed: None,
        };
        return Ok((pane, Vec::new(), vec![target]));
    }
    if !args.entries.is_empty() {
        return Ok((pane, Vec::new(), named_targets(args.entries)?));
    }
    let ws = app.workspace.read();
    let p = ws
        .pane(pane)
        .ok_or_else(|| anyhow::anyhow!("no such pane"))?;
    anyhow::ensure!(p.open.is_some(), "pane has no location");
    let ids: Vec<EntryId> = if args.ids.is_empty() {
        p.selection.iter().collect()
    } else {
        args.ids
    };
    let mut named = Vec::new();
    let mut targets = Vec::new();
    for id in ids {
        let (Some(entry), Some(path)) = (p.entry(id), p.path_of(id)) else {
            continue;
        };
        named.push(id);
        targets.push(Target {
            path,
            kind: entry.kind,
            entry: Some(entry.clone()),
            typed: None,
        });
    }
    drop(ws);
    anyhow::ensure!(!targets.is_empty(), "nothing to open");
    Ok((pane, named, targets))
}

/// Files named by path as a listing would show them. Blocking: commands
/// from the window hand these to `resolve_named` before calling this.
fn named_targets(entries: Vec<skwir_core::op::EntryArg>) -> anyhow::Result<Vec<Target>> {
    debug_assert!(
        !on_window_thread(),
        "explicit targets must be read on a worker"
    );
    skwir_core::op::EntryArg::refs(entries)?
        .into_iter()
        .map(|r| {
            let entry = skwir_core::fs::stat_path(&r.path)
                .ok_or_else(|| anyhow::anyhow!("{} does not exist", r.path.display()))?;
            Ok(Target {
                path: r.path,
                kind: entry.kind,
                entry: Some(entry),
                typed: None,
            })
        })
        .collect()
}

fn by_lane<T>(
    items: impl IntoIterator<Item = T>,
    lane: impl Fn(&T) -> Lane,
) -> Vec<(Lane, Vec<T>)> {
    let mut groups: Vec<(Lane, Vec<T>)> = Vec::new();
    for item in items {
        let lane = lane(&item);
        match groups.iter_mut().find(|(known, _)| *known == lane) {
            Some((_, items)) => items.push(item),
            None => groups.push((lane, vec![item])),
        }
    }
    groups
}

/// Explicit targets are still only names on the window thread. Resolve
/// each mount's names in its own lane, without blocking another lane to
/// wait for them. Finish only once all succeeded, in the caller's order.
fn resolve_named(
    app: &App,
    entries: Vec<skwir_core::op::EntryArg>,
    cancel: &CancelToken,
    done: impl FnOnce(&App, Vec<Target>) -> anyhow::Result<()> + Send + 'static,
) -> anyhow::Result<()> {
    fn path_of(entry: &skwir_core::op::EntryArg) -> &std::path::Path {
        match entry {
            skwir_core::op::EntryArg::Path(path) | skwir_core::op::EntryArg::Named { path, .. } => {
                path.as_ref()
            }
        }
    }
    prepare_targets(
        app,
        entries,
        path_of,
        |entry| Ok(named_targets(vec![entry])?.pop().unwrap()),
        cancel,
        done,
    )
}

fn prepare_targets<T: Send + 'static>(
    app: &App,
    entries: Vec<T>,
    path_of: impl Fn(&T) -> &std::path::Path,
    prepare: impl Fn(T) -> anyhow::Result<Target> + Send + Sync + 'static,
    cancel: &CancelToken,
    done: impl FnOnce(&App, Vec<Target>) -> anyhow::Result<()> + Send + 'static,
) -> anyhow::Result<()> {
    struct Pending<F> {
        left: usize,
        targets: Vec<(usize, Target)>,
        error: Option<anyhow::Error>,
        done: Option<F>,
    }
    // Invalid arguments remain immediate errors; validation does no I/O.
    for entry in &entries {
        skwir_core::os_path::absolute(path_of(entry))?;
    }
    let groups = by_lane(entries.into_iter().enumerate(), |(_, entry)| {
        reach::lane(path_of(entry))
    });
    let pending = Arc::new(Mutex::new(Pending {
        left: groups.len(),
        targets: Vec::new(),
        error: None,
        done: Some(done),
    }));
    let prepare = Arc::new(prepare);
    for (lane, entries) in groups {
        let pending = Arc::clone(&pending);
        let prepare = Arc::clone(&prepare);
        let cancel = cancel.clone();
        spawn_in(app, lane, "open-with.targets", cancel.clone(), move |app| {
            let (positions, entries): (Vec<_>, Vec<_>) = entries.into_iter().unzip();
            let resolved: anyhow::Result<Vec<_>> = entries
                .into_iter()
                .map(|entry| {
                    anyhow::ensure!(!cancel.is_cancelled(), "opening cancelled");
                    prepare(entry)
                })
                .collect();
            if cancel.is_cancelled() {
                return;
            }
            let completed = {
                let mut pending = pending.lock();
                match resolved {
                    Ok(targets) => pending.targets.extend(positions.into_iter().zip(targets)),
                    Err(error) => {
                        pending.error.get_or_insert(error);
                    }
                }
                pending.left -= 1;
                if pending.left == 0 {
                    Some((
                        pending.done.take().unwrap(),
                        pending.error.take(),
                        std::mem::take(&mut pending.targets),
                    ))
                } else {
                    None
                }
            };
            if let Some((done, error, mut targets)) = completed {
                let result = if let Some(error) = error {
                    Err(error)
                } else {
                    targets.sort_by_key(|(position, _)| *position);
                    done(app, targets.into_iter().map(|(_, target)| target).collect())
                };
                if let Err(error) = result {
                    app.notify(NotifyLevel::Error, format!("{error:#}"));
                }
                app.wake();
            }
        });
    }
    Ok(())
}

impl Target {
    fn read_path(&self) -> &std::path::Path {
        self.entry
            .as_ref()
            .and_then(|entry| entry.target.as_ref())
            .and_then(|target| target.path.as_deref())
            .unwrap_or(&self.path)
    }

    fn lane(&self) -> Lane {
        reach::lane(self.read_path())
    }

    fn read_type(mut self, read: bool) -> Self {
        if read {
            self.typed = Some(self.mime());
        }
        self
    }
    /// What the entry is (a pipe is `inode/fifo`, never read), else what
    /// it opens says; reads the head of a file: call from a worker.
    fn mime(&self) -> Mime {
        if let Some(mime) = &self.typed {
            return mime.clone();
        }
        match &self.entry {
            Some(entry) => skwir_platform::apps::type_of(entry, &self.path),
            None => Mime("inode/directory".into()),
        }
    }
}

/// How closely an application fits the targets: every one of their types
/// declared as such, or some only through a parent type (plain text for a
/// C source).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Fit {
    Exact,
    Related,
}

/// One entry of the picker, as `ui.choose` reads it.
#[derive(Debug, serde::Serialize)]
struct Choice {
    title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    icon: Option<String>,
    section: &'static str,
    command: Invocation,
}

/// The applications for `types`, best first: the default of every type,
/// then those that open each type as such, then those that open them as a
/// parent type, then everything else the desktop lists.
fn rank<'a>(catalog: &'a Catalog, types: &[Mime]) -> Vec<(&'a Application, Option<Fit>, bool)> {
    let lineages: Vec<Vec<Mime>> = types.iter().map(mime::lineage).collect();
    // Per type: each associated application's fit and first position.
    let fits: Vec<Vec<(&Application, Fit)>> = lineages
        .iter()
        .map(|lineage| {
            let mut out: Vec<(&Application, Fit)> = Vec::new();
            for (depth, m) in lineage.iter().enumerate() {
                let fit = if depth == 0 { Fit::Exact } else { Fit::Related };
                for app in catalog.for_type(m.as_str()) {
                    if !out.iter().any(|(a, _)| a.id == app.id) {
                        out.push((app, fit));
                    }
                }
            }
            out
        })
        .collect();
    let defaults: Vec<Option<&Application>> = lineages
        .iter()
        .map(|l| catalog.default_for_lineage(l))
        .collect();
    let default = match defaults.split_first() {
        Some((Some(first), rest)) if rest.iter().all(|d| d.is_some_and(|d| d.id == first.id)) => {
            Some(first.id.as_str())
        }
        _ => None,
    };
    let mut ranked: Vec<(&Application, Option<Fit>, bool)> = Vec::new();
    if let Some(first) = fits.first() {
        for &(app, _) in first {
            // The worst fit over every type; absent from one, not associated.
            let fit = fits.iter().try_fold(Fit::Exact, |worst, per_type| {
                per_type
                    .iter()
                    .find(|(a, _)| a.id == app.id)
                    .map(|&(_, fit)| worst.max(fit))
            });
            if let Some(fit) = fit {
                ranked.push((app, Some(fit), default == Some(app.id.as_str())));
            }
        }
    }
    // Stable: the default first, then by fit, each in the lists' order.
    ranked.sort_by_key(|&(_, fit, is_default)| (!is_default, fit));
    let mut others: Vec<&Application> = catalog
        .all()
        .iter()
        .filter(|app| app.shown && !ranked.iter().any(|(a, _, _)| a.id == app.id))
        .collect();
    others.sort_by_cached_key(|app| (app.name.to_lowercase(), app.id.clone()));
    ranked.extend(others.into_iter().map(|app| (app, None, false)));
    // A package may install one program under two IDs (`brave-browser` and
    // `com.brave.Browser`): the same name running the same command is one
    // choice, the best placed.
    let mut seen = std::collections::HashSet::new();
    ranked.retain(|(app, _, _)| seen.insert((app.name.as_str(), app.exec.as_str())));
    ranked
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct ChoicesArgs {
    #[serde(flatten)]
    target: TargetArgs,
    /// Choices that make the application the default for the entries'
    /// type (they must share one) instead of opening them.
    #[serde(default)]
    defaults: bool,
}

/// `open_with.properties`: Properties' Opens with (ADR-058) for one file:
/// the application its type opens with, and the way to change it. Reads
/// the catalog where it changed and the head of a file whose name does
/// not say its type: on Properties' worker.
fn properties(args: TargetArgs, cache: &Cached, app: &App) -> anyhow::Result<Outcome> {
    let named = args.entries.clone();
    let (_, _, targets) = targets(args, app)?;
    let rows = match targets.as_slice() {
        [one]
            if one
                .entry
                .as_ref()
                .is_some_and(|e| e.opens() == EntryKind::File) =>
        {
            let m = one.mime();
            let catalog = catalog(cache);
            let value = catalog
                .default_for_lineage(&mime::lineage(&m))
                .map_or_else(|| "No application".to_owned(), |a| a.name.clone());
            let change = Invocation::with_args(
                "ui.choose",
                &serde_json::json!({
                    "title": format!("Open {} with", files_of(m.as_str())),
                    "placeholder": "Search applications",
                    "source": {
                        "name": "open_with.choices",
                        "args": { "entries": named, "defaults": true },
                    },
                }),
            );
            vec![serde_json::json!({
                "group": "General",
                "label": "Opens with",
                "value": value,
                "actions": [{ "words": "Change…", "command": change }],
            })]
        }
        _ => Vec::new(),
    };
    Ok(Outcome::Value(serde_json::json!({ "rows": rows })))
}

fn choices(args: ChoicesArgs, cache: &Cached, app: &App) -> anyhow::Result<Outcome> {
    let ChoicesArgs {
        target: args,
        defaults,
    } = args;
    let here = args.here;
    // A choice opens what was asked about, as it was named.
    let named = args.entries.clone();
    let (pane, ids, targets) = targets(args, app)?;
    let mut types: Vec<Mime> = Vec::new();
    for t in &targets {
        let m = t.mime();
        if !types.contains(&m) {
            types.push(m);
        }
    }
    let catalog = catalog(cache);
    if defaults {
        let [only] = types.as_slice() else {
            anyhow::bail!("A default application is chosen for one type of file at a time");
        };
        return Ok(Outcome::Value(default_choices(&catalog, only)));
    }
    let title = match targets.as_slice() {
        [one] => {
            let name = one
                .path
                .file_name()
                .map_or_else(|| one.path.to_string_lossy(), |n| n.to_string_lossy());
            format!("Open “{name}” with")
        }
        many => format!("Open {} items with", many.len()),
    };
    let choices: Vec<Choice> = rank(&catalog, &types)
        .into_iter()
        .map(|(application, fit, is_default)| {
            let mut args = serde_json::json!({ "app": application.id, "pane": pane });
            if here {
                args["here"] = true.into();
            } else if !named.is_empty() {
                args["entries"] = serde_json::to_value(&named).unwrap_or_default();
            } else {
                args["ids"] = serde_json::to_value(&ids).unwrap_or_default();
            }
            let detail = match (is_default, &application.generic_name) {
                (true, Some(generic)) => Some(format!("Default · {generic}")),
                (true, None) => Some("Default".to_owned()),
                (false, generic) => generic.clone(),
            };
            Choice {
                title: application.name.clone(),
                detail,
                icon: application.icon.clone(),
                section: match fit {
                    Some(Fit::Exact) => "Recommended",
                    Some(Fit::Related) => "Related",
                    None => "Other applications",
                },
                command: Invocation::with_args("open_with", &args),
            }
        })
        .collect();
    let mut answer = serde_json::json!({
        "title": title,
        "empty": "No applications found",
        "choices": choices,
    });
    // One type: the chosen application can become its default.
    if let [only] = types.as_slice() {
        answer["option"] = serde_json::json!({
            "title": format!("Always use for {}", files_of(only.as_str())),
            "field": "set_default",
        });
    }
    Ok(Outcome::Value(answer))
}

/// The choices that make an application the default for `only`, the one
/// that is now first: what Properties' Opens with changes.
fn default_choices(catalog: &Catalog, only: &Mime) -> serde_json::Value {
    let choices: Vec<Choice> = rank(catalog, std::slice::from_ref(only))
        .into_iter()
        .map(|(application, fit, is_default)| Choice {
            title: application.name.clone(),
            detail: is_default
                .then(|| "Now".to_owned())
                .or_else(|| application.generic_name.clone()),
            icon: application.icon.clone(),
            section: match fit {
                Some(Fit::Exact) => "Recommended",
                Some(Fit::Related) => "Related",
                None => "Other applications",
            },
            command: Invocation::with_args(
                "open_with.set_default",
                &serde_json::json!({ "mime": only.as_str(), "app": application.id }),
            ),
        })
        .collect();
    serde_json::json!({
        "title": format!("Open {} with", files_of(only.as_str())),
        "empty": "No applications found",
        "choices": choices,
    })
}

fn open_with(args: OpenWithArgs, cache: &Cached, app: &App) -> anyhow::Result<Outcome> {
    let pane = app.pane_or_active(args.target.pane);
    let lifetime = if on_window_thread() {
        pane_lifetime(app, pane)?
    } else {
        app.lifetime().child()
    };
    let id = args.app;
    let set_default = args.set_default;
    // Reading the catalog, the types and the lists, and starting the
    // program, touch the disk.
    let cache = Arc::clone(cache);
    let cancel = lifetime.clone();
    let launch = move |app: &App, targets: Vec<Target>| {
        if cancel.is_cancelled() {
            return Ok(());
        }
        if set_default {
            let mut types = targets.iter().map(Target::mime);
            let first = types.next().expect("targets are never empty");
            if types.any(|m| m != first) {
                app.notify(
                    NotifyLevel::Error,
                    "A default application is chosen for one type of file at a time",
                );
            } else {
                make_default(app, &cache, first.as_str(), &id);
            }
        }
        let paths: Vec<PathBuf> = targets.into_iter().map(|t| t.path).collect();
        let catalog = catalog(&cache);
        if cancel.is_cancelled() {
            return Ok(());
        }
        let result = match catalog.get(&id) {
            Some(application) => application.launch(&paths),
            None => Err(anyhow::anyhow!("no application “{id}” is installed")),
        };
        if let Err(e) = result {
            app.notify(NotifyLevel::Error, format!("{e:#}"));
        }
        Ok(())
    };
    let cancel = lifetime.clone();
    let prepare = move |app: &App, targets| {
        prepare_targets(
            app,
            targets,
            Target::read_path,
            move |target| Ok(target.read_type(set_default)),
            &cancel,
            launch,
        )
    };
    if on_window_thread() && !args.target.here && !args.target.entries.is_empty() {
        resolve_named(app, args.target.entries, &lifetime, prepare)?;
    } else {
        let (_, _, targets) = targets(args.target, app)?;
        prepare(app, targets)?;
    }
    Ok(Outcome::Done)
}

/// Run `work` on a worker for as long as the app lives.
fn spawn(app: &App, label: &'static str, work: impl FnOnce(&App) + Send + 'static) {
    spawn_in(app, Lane::SHARED, label, app.lifetime().child(), work);
}

fn spawn_in(
    app: &App,
    lane: Lane,
    label: &'static str,
    cancel: CancelToken,
    work: impl FnOnce(&App) + Send + 'static,
) {
    let weak = app.weak();
    app.scheduler
        .spawn_in(lane, Priority::Visible, label, cancel, move || {
            if let Some(app) = weak.upgrade() {
                work(&app);
            }
        });
}

/// How a sentence names the files of `mime`: `folders`, `PNG image files`,
/// `plain text document files`.
fn files_of(mime: &str) -> String {
    if mime == "inode/directory" {
        return "folders".to_owned();
    }
    let Some(description) = Mime(mime.to_owned()).description() else {
        return format!("{mime} files");
    };
    // Lower the first letter of a word, not of an acronym ("PNG").
    let mut chars = description.chars();
    let lowered = match (chars.next(), chars.next()) {
        (Some(first), Some(second)) if !second.is_uppercase() => first
            .to_lowercase()
            .chain(description.chars().skip(1))
            .collect(),
        _ => description,
    };
    format!("{lowered} files")
}

/// Make `id` the default for `mime` in the user's lists, and say so with
/// Undo. On a worker.
fn make_default(app: &App, cache: &Cached, mime: &str, id: &str) {
    if catalog(cache).get(id).is_none() {
        app.notify(
            NotifyLevel::Error,
            format!("no application “{id}” is installed"),
        );
        return;
    }
    match AssociationEdit::make_default_in(&cache.user_lists(), mime, id) {
        Ok(edit) => apply_edit(app, cache, &edit, false),
        Err(e) => app.notify(
            NotifyLevel::Error,
            format!("The default application was not changed: {e:#}"),
        ),
    }
}

/// Write `edit` and say what opens its type now, with the way back: Undo,
/// or Redo when this was an undo. On a worker.
fn apply_edit(app: &App, cache: &Cached, edit: &AssociationEdit, undo: bool) {
    let files = files_of(&edit.mime);
    match skwir_platform::apps::edit_associations(edit) {
        Ok(changed) => {
            let catalog = catalog(cache);
            // Properties says the application again.
            cache.properties.fetch_add(1, Ordering::Release);
            app.wake();
            // What opens the type now, named or not: an undo may leave
            // the desktop's own choice.
            let now = catalog.default_for_lineage(&mime::lineage(&Mime(edit.mime.clone())));
            let text = match (now, &changed) {
                (Some(application), Some(_)) => format!("{} now opens {files}", application.name),
                (Some(application), None) => format!("{} already opens {files}", application.name),
                (None, _) => format!("No application opens {files}"),
            };
            match changed {
                Some(inverse) => app.notify_undoable(text, undo, edit_invocation(&inverse, !undo)),
                None => app.notify(NotifyLevel::Info, text),
            }
        }
        Err(e) => app.notify(
            NotifyLevel::Error,
            format!("The default application was not changed: {e:#}"),
        ),
    }
}

/// The command that writes `edit`, from a notification's button.
fn edit_invocation(edit: &AssociationEdit, undo: bool) -> Invocation {
    Invocation::with_args(
        "open_with.edit_associations",
        &serde_json::json!({
            "file": PathArg(edit.file.clone()),
            "mime": edit.mime,
            "default": edit.default,
            "added": edit.added,
            "removed": edit.removed,
            "undo": undo,
        }),
    )
}

/// `ui.choose` over `open_with.choices` with these arguments.
fn choose(args: &serde_json::Value) -> Invocation {
    Invocation::with_args(
        "ui.choose",
        &serde_json::json!({
            "title": "Open with",
            "placeholder": "Search applications",
            "source": { "name": "open_with.choices", "args": args },
        }),
    )
}

impl Plugin for OpenWithPlugin {
    fn id(&self) -> &'static str {
        ID
    }

    fn title(&self) -> &'static str {
        "Open with"
    }

    fn description(&self) -> &'static str {
        "Open files with their default application or one you choose, and open a terminal here"
    }

    fn category(&self) -> skwir_core::plugin::Category {
        skwir_core::plugin::Category::Desktop
    }

    fn capabilities(&self) -> skwir_core::permissions::Capabilities {
        use skwir_core::permissions::Capability;
        skwir_core::permissions::Capabilities::of(&[Capability::Programs])
    }
    fn register(&self, app: &App) {
        let cache = Arc::clone(&self.catalog);
        app.commands.register::<OpenArgs, _>(
            ID, Topic::Go,
            "open",
            "Open",
            "Enter the selected directory, or open the selected files with the default application of their type, as the desktop's lists choose it (the one the context menu names)",
            move |a, app| open(a, &cache, app),
        );
        app.commands.register::<PaneArg, _>(
            ID, Topic::Terminal,
            "open_terminal",
            "Open terminal here",
            "Open a terminal in the selected folder, else the folder shown, else (in a search) the folder holding the selection",
            |a, app| open_terminal(a.pane, app),
        );
        app.registry.menus.add(
            ID,
            MenuItem::new("context", "Open", "open", 0, Invocation::new("open")).when("selection"),
        );
        app.registry.menus.add(
            ID,
            MenuItem::new(
                "context",
                "Open Terminal Here",
                "command",
                10,
                Invocation::new("open_terminal"),
            )
            .when("selection|folder !place"),
        );
        let cache = Arc::clone(&self.catalog);
        app.commands.register::<ChoicesArgs, _>(
            ID, Topic::Files,
            "open_with.choices",
            "List applications for the selection",
            "List the applications that open the selected entries (or the pane's folder with `here`), recommended first, each with the `open_with` invocation that opens them; with `defaults`, each with the `open_with.set_default` that makes it the default for their type",
            move |a, app| choices(a, &cache, app),
        )
        .data();
        let cache = Arc::clone(&self.catalog);
        app.commands.register::<TargetArgs, _>(
            ID, Topic::Files,
            "open_with.properties",
            "Name the default application in Properties",
            "Properties' Opens with row for one file: the application its type opens with, and the way to change it",
            move |a, app| properties(a, &cache, app),
        )
        .data();
        app.registry.property_sources.add(
            ID,
            skwir_core::registry::PropertySource {
                command: Invocation::new("open_with.properties"),
                revision: Arc::clone(&self.catalog.properties),
            },
        );
        let cache = Arc::clone(&self.catalog);
        app.commands.register::<OpenWithArgs, _>(
            ID, Topic::Files,
            "open_with",
            "Open with an application by desktop ID",
            "Open the selected entries (or the pane's folder with `here`) with the application whose desktop file ID is `app`",
            move |a, app| open_with(a, &cache, app),
        );
        let cache = Arc::clone(&self.catalog);
        app.commands.register::<SetDefaultArgs, _>(
            ID, Topic::Files,
            "open_with.set_default",
            "Set the default application for a type",
            "Make the application whose desktop file ID is `app` the default for the MIME type `mime`, in the user's mimeapps.list; the notification offers Undo",
            move |a, app| {
                let cache = Arc::clone(&cache);
                spawn(app, "open-with.default", move |app| {
                    make_default(app, &cache, &a.mime, &a.app);
                });
                Ok(Outcome::Done)
            },
        );
        let cache = Arc::clone(&self.catalog);
        app.commands.register::<EditArgs, _>(
            ID, Topic::Files,
            "open_with.edit_associations",
            "Rewrite a type's default application lines",
            "Set the lines a MIME type has in one mimeapps.list (default, added and removed associations); what Undo and Redo of a default run",
            move |a, app| {
                // Only the user's own lists: the arguments name a file.
                anyhow::ensure!(
                    cache.user_lists().contains(&a.file.0),
                    "{} is not one of your lists of default applications",
                    a.file.display()
                );
                let edit = AssociationEdit {
                    file: a.file.into_inner(),
                    mime: a.mime,
                    default: a.default,
                    added: a.added,
                    removed: a.removed,
                };
                let cache = Arc::clone(&cache);
                spawn(app, "open-with.default", move |app| {
                    apply_edit(app, &cache, &edit, a.undo);
                });
                Ok(Outcome::Done)
            },
        );
        // The chooser, last of the Open With submenu, after the
        // applications `open_with.menu` names (ADR-099); on the folder's
        // background, for the folder.
        app.registry.menus.add(
            ID,
            MenuItem::new(
                "context",
                "Other Application…",
                "open",
                8,
                choose(&serde_json::json!({})),
            )
            .when("selection !place")
            .submenu(OPEN_WITH),
        );
        app.registry.menus.add(
            ID,
            MenuItem::new(
                "context",
                "Other Application…",
                "open",
                8,
                choose(&serde_json::json!({ "here": true })),
            )
            .when("empty folder")
            .submenu(OPEN_FOLDER_WITH),
        );
        let cache = Arc::clone(&self.catalog);
        app.commands.register::<PaneArg, _>(
            ID, Topic::Files,
            "open_with.menu",
            "Name the default application in the context menu",
            "The context menu's Open for the selection, naming the default application its files share, and the other applications for their types in its Open With submenu; no items when the selection's types or the applications are not known yet. Answers from memory",
            move |a, app| menu(a.pane, &cache, app),
        )
        .data();
        app.registry.menu_sources.add(
            ID,
            MenuSource {
                menu: "context".into(),
                when: Some("selection !place".into()),
                command: Invocation::new("open_with.menu"),
                replaces: vec![Invocation::new("open")],
            },
        );
        // Read before the first menu asks.
        refresh(&self.catalog, app);
        app.registry.keymap.add(
            ID,
            KeyBinding {
                keys: "shift-enter".into(),
                command: choose(&serde_json::json!({})),
                when: Some("pane selection".into()),
            },
        );
    }

    fn on_disable(&self, _app: &App) {
        if let Some(work) = self.catalog.work.lock().take() {
            work.cancel();
        }
        // A job cancelled before it started never says it ended.
        self.catalog.refreshing.store(false, Ordering::Release);
        self.catalog.last.lock().take();
    }
}

#[cfg(test)]
// Helpers of the tests that run on Linux alone (ADR-035).
#[cfg_attr(not(target_os = "linux"), allow(dead_code, unused_imports))]
mod tests {
    use super::*;
    use skwir_core::app::AppConfig;
    use std::time::Duration;

    struct BlockedWorkers(Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>);

    impl Drop for BlockedWorkers {
        fn drop(&mut self) {
            *self.0.0.lock().unwrap() = true;
            self.0.1.notify_all();
        }
    }

    /// Occupy the pool and every bounded replacement worker before
    /// returning. Later jobs cannot begin until the guard is released.
    fn block_workers(app: &App) -> BlockedWorkers {
        assert!(app.scheduler.wait_idle(Duration::from_secs(10)));
        let guard = BlockedWorkers(Arc::new((
            std::sync::Mutex::new(false),
            std::sync::Condvar::new(),
        )));
        let (started, waiting) = std::sync::mpsc::channel();
        for _ in 0..2 * app.scheduler.workers() {
            let gate = Arc::clone(&guard.0);
            let started = started.clone();
            app.scheduler.spawn(
                Priority::Visible,
                "test.block",
                CancelToken::new(),
                move || {
                    let _ = started.send(());
                    let mut released = gate.0.lock().unwrap();
                    while !*released {
                        released = gate.1.wait(released).unwrap();
                    }
                },
            );
            // Submit the next blocker only once this one runs. After the
            // normal pool fills, each submission requests the next bounded
            // replacement; a single early burst can leave idle workers
            // counted at submission time and schedule no replacement look.
            waiting.recv_timeout(Duration::from_secs(10)).unwrap();
        }
        guard
    }

    fn window_app(root: &std::path::Path) -> (Arc<App>, PaneId) {
        let mut config = AppConfig::ephemeral(root);
        config.workers = 2;
        let app = App::new(config).unwrap();
        app.plugins.add(Arc::new(OpenWithPlugin::default()), &app);
        let pane = app.workspace.read().active_id();
        app.navigate(pane, &Location::from_path(root)).unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(10)));
        (app, pane)
    }

    #[test]
    fn explicit_open_reads_after_dispatch_and_keeps_the_original_pane() {
        let tmp = tempfile::tempdir().unwrap();
        let (app, original) = window_app(tmp.path());
        let other = app.workspace.write().add_pane();
        app.navigate(other, &Location::from_path(tmp.path()))
            .unwrap();
        app.workspace.write().set_active(original);
        let blocked = block_workers(&app);
        skwir_core::scheduler::mark_window_thread(&app.scheduler);
        let target = tmp.path().join("created after dispatch");
        app.dispatch(&Invocation::with_args(
            "open",
            &serde_json::json!({"entries": [target]}),
        ))
        .unwrap();
        // A synchronous stat would have failed: the target does not exist
        // until the command has returned to the window.
        std::fs::create_dir(&target).unwrap();
        app.workspace.write().set_active(other);
        drop(blocked);
        assert!(app.scheduler.wait_idle(Duration::from_secs(10)));
        assert_eq!(app.pane_dir(original).unwrap(), target);
        assert_eq!(app.pane_dir(other).unwrap(), tmp.path());
        assert_eq!(app.workspace.read().active_id(), other);
    }

    #[test]
    fn navigating_away_cancels_explicit_open_before_it_resolves() {
        let tmp = tempfile::tempdir().unwrap();
        let (app, pane) = window_app(tmp.path());
        let blocked = block_workers(&app);
        skwir_core::scheduler::mark_window_thread(&app.scheduler);
        let target = tmp.path().join("target");
        let elsewhere = tmp.path().join("elsewhere");
        std::fs::create_dir(&target).unwrap();
        std::fs::create_dir(&elsewhere).unwrap();
        app.dispatch(&Invocation::with_args(
            "open",
            &serde_json::json!({"entries": [target]}),
        ))
        .unwrap();
        app.navigate(pane, &Location::from_path(&elsewhere))
            .unwrap();
        drop(blocked);
        assert!(app.scheduler.wait_idle(Duration::from_secs(10)));
        assert_eq!(app.pane_dir(pane).unwrap(), elsewhere);
        assert!(app.take_notifications().is_empty());
    }

    #[test]
    fn explicit_open_reports_worker_errors_and_keeps_script_errors_synchronous() {
        let tmp = tempfile::tempdir().unwrap();
        let (app, _) = window_app(tmp.path());
        let invocation = Invocation::with_args(
            "open",
            &serde_json::json!({"entries": [tmp.path().join("missing")]}),
        );
        assert!(
            app.dispatch(&invocation).is_err(),
            "a script still gets its error"
        );
        let blocked = block_workers(&app);
        skwir_core::scheduler::mark_window_thread(&app.scheduler);
        app.dispatch(&invocation).unwrap();
        assert!(app.take_notifications().is_empty());
        drop(blocked);
        assert!(app.scheduler.wait_idle(Duration::from_secs(10)));
        let messages = app.take_notifications();
        assert_eq!(messages.len(), 1);
        assert!(messages[0].text.contains("does not exist"));
    }

    #[test]
    fn terminal_targets_the_selected_folder_else_the_pane() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("f"), b"").unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        let pane = app.workspace.read().active_id();
        app.navigate(pane, &Location::from_path(&root)).unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
        assert_eq!(target_dir(pane, &app).unwrap(), root);
        let select = |name: &str| {
            let mut ws = app.workspace.write();
            let p = ws.pane_mut(pane).unwrap();
            let id = p.listing.find_by_name(name.as_ref()).unwrap().id;
            p.selection.set([id]);
        };
        select("sub");
        assert_eq!(target_dir(pane, &app).unwrap(), root.join("sub"));
        select("f");
        assert_eq!(
            target_dir(pane, &app).unwrap(),
            root,
            "a selected file does not change the target"
        );
    }

    /// A socket (as a pipe or a device) is what it is whatever its name
    /// says: typed by its kind, never read, so no image viewer is offered.
    #[cfg(unix)]
    #[test]
    fn a_socket_named_like_a_picture_is_typed_by_its_kind() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        std::fs::create_dir_all(&root).unwrap();
        let _socket = std::os::unix::net::UnixListener::bind(root.join("x.png")).unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
        let pane = app.workspace.read().active_id();
        app.navigate(pane, &Location::from_path(&root)).unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
        let id = {
            let ws = app.workspace.read();
            let p = ws.pane(pane).unwrap();
            p.listing.find_by_name("x.png".as_ref()).unwrap().id
        };
        let args = TargetArgs {
            pane: Some(pane),
            ids: vec![id],
            entries: Vec::new(),
            here: false,
        };
        let (_, _, listed) = targets(args, &app).unwrap();
        assert_eq!(listed[0].mime(), Mime("inode/socket".into()));
        // Named by its path, as a script names it: the same.
        let args = TargetArgs {
            pane: None,
            ids: Vec::new(),
            entries: vec![skwir_core::op::EntryArg::Path(root.join("x.png").into())],
            here: false,
        };
        let (_, _, named) = targets(args, &app).unwrap();
        assert_eq!(named[0].mime(), Mime("inode/socket".into()));
    }

    #[test]
    fn open_enters_a_single_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(OpenWithPlugin::default()), &app);
        let pane = app.workspace.read().active_id();
        app.navigate(pane, &Location::from_path(&root)).unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
        app.dispatch(&Invocation::with_args(
            "select",
            &serde_json::json!({ "names": ["sub"] }),
        ))
        .unwrap();
        app.dispatch(&Invocation::new("open")).unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
        assert_eq!(
            app.workspace
                .read()
                .pane(pane)
                .unwrap()
                .location()
                .unwrap()
                .path()
                .unwrap(),
            root.join("sub")
        );
        assert!(
            app.dispatch(&Invocation::new("open")).is_err(),
            "empty selection is an error"
        );
    }

    #[cfg(unix)]
    #[test]
    fn open_enters_a_link_to_a_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::os::unix::fs::symlink(root.join("sub"), root.join("link")).unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(OpenWithPlugin::default()), &app);
        let pane = app.workspace.read().active_id();
        app.navigate(pane, &Location::from_path(&root)).unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
        app.dispatch(&Invocation::with_args(
            "select",
            &serde_json::json!({ "names": ["link"] }),
        ))
        .unwrap();
        app.dispatch(&Invocation::new("open")).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let location = || {
            app.workspace
                .read()
                .pane(pane)
                .unwrap()
                .location()
                .unwrap()
                .path()
                .unwrap()
        };
        while location() != root.join("link") {
            assert!(
                std::time::Instant::now() < deadline,
                "never entered the link"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn withdraws_on_disable() {
        let tmp = tempfile::tempdir().unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(OpenWithPlugin::default()), &app);
        assert!(app.commands.get("open").is_some());
        assert_eq!(app.registry.menus.all().len(), 4);
        assert_eq!(app.registry.menu_sources.all().len(), 1);
        app.plugins.set_enabled(ID, false, &app);
        assert!(app.commands.get("open").is_none());
        assert!(app.registry.menus.all().is_empty());
        assert!(app.registry.menu_sources.all().is_empty());
    }

    /// Desktop entries under `dir/applications`, and a list making
    /// `viewer` the default for PNG pictures.
    fn fake_catalog(dir: &std::path::Path, record: &std::path::Path) -> Catalog {
        let apps = dir.join("applications");
        std::fs::create_dir_all(&apps).unwrap();
        let entry = |id: &str, name: &str, exec: &str, extra: &str| {
            std::fs::write(
                apps.join(id),
                format!("[Desktop Entry]\nType=Application\nName={name}\nExec={exec}\n{extra}\n"),
            )
            .unwrap();
        };
        let record = record.display();
        entry(
            "viewer.desktop",
            "Viewer",
            &format!("{record} %F"),
            "GenericName=Image Viewer\nMimeType=image/png;",
        );
        entry(
            "editor.desktop",
            "Editor",
            "editor %U",
            "MimeType=image/png;text/plain;",
        );
        entry(
            "hex.desktop",
            "Hex",
            "hex %f",
            "MimeType=application/octet-stream;",
        );
        entry(
            "browser.desktop",
            "Browser",
            "browser %f",
            "MimeType=inode/directory;",
        );
        entry("zed.desktop", "archiver", "archiver", "");
        entry("helper.desktop", "Helper", "helper", "NoDisplay=true");
        // The same program under a second ID, as some packages install it.
        entry(
            "org.editor.desktop",
            "Editor",
            "editor %U",
            "MimeType=image/png;",
        );
        let list = dir.join("mimeapps.list");
        std::fs::write(&list, "[Default Applications]\nimage/png=viewer.desktop\n").unwrap();
        Catalog::from_paths(&[apps], &[list])
    }

    /// The catalog [`fake_catalog`] wrote in `dir`, read from there.
    fn cached(dir: &std::path::Path) -> Cached {
        Arc::new(CatalogCache {
            sources: Some((
                vec![dir.join("applications")],
                vec![dir.join("mimeapps.list")],
            )),
            last: Mutex::default(),
            refreshing: AtomicBool::default(),
            properties: Arc::default(),
            work: Mutex::default(),
        })
    }

    fn ranked(catalog: &Catalog, types: &[&str]) -> Vec<(String, Option<Fit>, bool)> {
        let types: Vec<Mime> = types.iter().map(|t| Mime((*t).into())).collect();
        rank(catalog, &types)
            .into_iter()
            .map(|(app, fit, default)| (app.id.clone(), fit, default))
            .collect()
    }

    #[test]
    fn applications_rank_default_then_exact_then_related_then_the_rest() {
        let tmp = tempfile::tempdir().unwrap();
        let catalog = fake_catalog(tmp.path(), std::path::Path::new("record"));
        let own = |id: &str, fit, default| (id.to_owned(), fit, default);
        assert_eq!(
            ranked(&catalog, &["image/png"]),
            [
                own("viewer.desktop", Some(Fit::Exact), true),
                own("editor.desktop", Some(Fit::Exact), false),
                own("hex.desktop", Some(Fit::Related), false),
                own("zed.desktop", None, false),
                own("browser.desktop", None, false),
            ],
            "the rest by name, whatever the case; one not shown is left out; \
             a second ID for the same program is not a second choice"
        );
        assert_eq!(
            ranked(&catalog, &["image/png", "text/plain"])[..2],
            [
                own("editor.desktop", Some(Fit::Exact), false),
                own("hex.desktop", Some(Fit::Related), false),
            ],
            "several types: what opens them all, no default unless shared"
        );
        assert_eq!(
            ranked(&catalog, &["inode/directory"])[0],
            own("browser.desktop", Some(Fit::Exact), true),
            "a folder is not a stream of bytes"
        );
    }

    fn select(app: &App, pane: PaneId, names: &[&str]) {
        let mut ws = app.workspace.write();
        let p = ws.pane_mut(pane).unwrap();
        let ids: Vec<EntryId> = names
            .iter()
            .map(|n| p.listing.find_by_name(n.as_ref()).unwrap().id)
            .collect();
        p.selection.set(ids);
    }

    // The free desktops' files (step E, ADR-035).
    #[cfg(target_os = "linux")]
    #[test]
    fn choices_name_the_application_pane_and_entries_to_open() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        let photo = "it's a “photo” .png";
        std::fs::write(root.join(photo), b"").unwrap();
        std::fs::write(root.join("notes.txt"), b"text").unwrap();
        let plugin = OpenWithPlugin {
            catalog: {
                fake_catalog(tmp.path(), std::path::Path::new("record"));
                cached(tmp.path())
            },
        };
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(plugin), &app);
        let pane = app.workspace.read().active_id();
        app.navigate(pane, &Location::from_path(&root)).unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
        select(&app, pane, &[photo]);
        let id = app
            .workspace
            .read()
            .pane(pane)
            .unwrap()
            .selection
            .iter()
            .next()
            .unwrap();
        let Outcome::Value(answer) = app.dispatch(&Invocation::new("open_with.choices")).unwrap()
        else {
            panic!("choices are a value")
        };
        assert_eq!(answer["title"], format!("Open “{photo}” with"));
        let first = &answer["choices"][0];
        assert_eq!(first["title"], "Viewer");
        assert_eq!(first["detail"], "Default · Image Viewer");
        assert_eq!(first["section"], "Recommended");
        assert_eq!(first["command"]["name"], "open_with");
        let args = &first["command"]["args"];
        assert_eq!(args["app"], "viewer.desktop");
        assert_eq!(args["pane"], serde_json::to_value(pane).unwrap());
        assert_eq!(args["ids"], serde_json::json!([id]));
        assert_eq!(
            answer["option"],
            serde_json::json!({ "title": "Always use for PNG image files", "field": "set_default" }),
            "one type: the choice can become its default"
        );

        select(&app, pane, &[photo, "notes.txt"]);
        let Outcome::Value(answer) = app.dispatch(&Invocation::new("open_with.choices")).unwrap()
        else {
            panic!("choices are a value")
        };
        assert_eq!(answer["title"], "Open 2 items with");
        assert!(
            answer.get("option").is_none(),
            "no one default for two types"
        );

        select(&app, pane, &[]);
        let Outcome::Value(answer) = app
            .dispatch(&Invocation::with_args(
                "open_with.choices",
                &serde_json::json!({ "here": true }),
            ))
            .unwrap()
        else {
            panic!("choices are a value")
        };
        assert_eq!(answer["title"], "Open “root” with");
        assert_eq!(
            answer["choices"][0]["command"]["args"]["app"],
            "browser.desktop"
        );
        assert_eq!(answer["choices"][0]["command"]["args"]["here"], true);
        assert!(
            app.dispatch(&Invocation::new("open_with.choices")).is_err(),
            "nothing selected is nothing to open"
        );
    }

    // The free desktops' files (step E, ADR-035).
    #[cfg(target_os = "linux")]
    #[test]
    fn the_menu_names_the_default_application_of_the_selection() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        let photo = "it's a “photo” .png";
        for name in [photo, "b.png", "notes.txt", "no-type-9q"] {
            std::fs::write(root.join(name), b"").unwrap();
        }
        fake_catalog(tmp.path(), std::path::Path::new("record"));
        let cache = cached(tmp.path());
        let plugin = OpenWithPlugin {
            catalog: Arc::clone(&cache),
        };
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(plugin), &app);
        let pane = app.workspace.read().active_id();
        app.navigate(pane, &Location::from_path(&root)).unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
        // Registering read the catalog on a worker.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while cache.remembered().is_none() {
            assert!(std::time::Instant::now() < deadline, "never read");
            std::thread::sleep(Duration::from_millis(5));
        }
        let items = |names: &[&str]| {
            select(&app, pane, names);
            let Outcome::Value(answer) = app.dispatch(&Invocation::new("open_with.menu")).unwrap()
            else {
                panic!("the items are a value")
            };
            answer["items"].as_array().unwrap().clone()
        };
        let photos = items(&[photo, "b.png"]);
        let titles: Vec<&str> = photos
            .iter()
            .map(|item| item["title"].as_str().unwrap())
            .collect();
        assert_eq!(
            titles,
            ["Open With Viewer", "Editor", "Hex"],
            "the default first, the others in Open With"
        );
        for other in &photos[1..] {
            assert_eq!(other["submenu"], OPEN_WITH);
            assert_eq!(other["command"]["name"], "open_with");
        }
        let args = &photos[0]["command"]["args"];
        assert_eq!(photos[0]["command"]["name"], "open_with");
        assert_eq!(args["app"], "viewer.desktop");
        assert_eq!(args["pane"], serde_json::to_value(pane).unwrap());
        let text = items(&["notes.txt"]);
        assert_eq!(
            text[0]["title"], "Open With Editor",
            "no default named: the first application that declares the type"
        );
        let mixed = items(&[photo, "notes.txt"]);
        assert_eq!(
            mixed[0]["title"], "Open",
            "no default shared: Open stays, said again"
        );
        assert!(
            mixed[1..].iter().all(|item| item["submenu"] == OPEN_WITH),
            "what opens both, in Open With"
        );
        assert!(items(&[photo, "no-type-9q"]).is_empty(), "a type not known");
        assert!(items(&["sub"]).is_empty(), "a folder opens here");
    }

    /// A program that writes its arguments, one a line, to `out`.
    // Shell scripts stand in for the programs (ADR-035).
    #[cfg(unix)]
    fn recorder(dir: &std::path::Path, out: &std::path::Path) -> PathBuf {
        let record = dir.join("record");
        std::fs::write(
            &record,
            format!(
                "#!/bin/sh\nfor a in \"$@\"; do printf '%s\\n' \"$a\"; done > '{}.tmp' && mv '{0}.tmp' '{0}'\n",
                out.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&record, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
        record
    }

    /// The lines `out` holds once a program wrote it, sorted.
    #[cfg(unix)]
    fn recorded(out: &std::path::Path) -> Vec<String> {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !out.exists() {
            assert!(std::time::Instant::now() < deadline, "never launched");
            std::thread::sleep(Duration::from_millis(5));
        }
        let mut got: Vec<String> = std::fs::read_to_string(out)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect();
        got.sort();
        got
    }

    #[cfg(unix)]
    #[test]
    fn open_starts_the_default_application_the_menu_names() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        std::fs::create_dir_all(&root).unwrap();
        let names = ["a b.png", "it's.png"];
        for name in names {
            std::fs::write(root.join(name), b"").unwrap();
        }
        let out = tmp.path().join("out");
        let record = recorder(tmp.path(), &out);
        let plugin = OpenWithPlugin {
            catalog: {
                fake_catalog(tmp.path(), &record);
                cached(tmp.path())
            },
        };
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(plugin), &app);
        let pane = app.workspace.read().active_id();
        app.navigate(pane, &Location::from_path(&root)).unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
        select(&app, pane, &names);
        app.dispatch(&Invocation::new("open")).unwrap();
        let want: Vec<String> = names
            .iter()
            .map(|n| root.join(n).display().to_string())
            .collect();
        assert_eq!(
            recorded(&out),
            want,
            "the catalog's default, once with every file of its type, not xdg-open"
        );
    }

    #[cfg(unix)]
    #[test]
    fn explicit_open_with_reads_and_types_its_targets_after_window_dispatch() {
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("out");
        let record = recorder(tmp.path(), &out);
        fake_catalog(tmp.path(), &record);
        let mut config = AppConfig::ephemeral(tmp.path());
        config.workers = 2;
        let app = App::new(config).unwrap();
        app.plugins.add(
            Arc::new(OpenWithPlugin {
                catalog: cached(tmp.path()),
            }),
            &app,
        );
        let pane = app.workspace.read().active_id();
        app.navigate(pane, &Location::from_path(tmp.path()))
            .unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(10)));
        let blocked = block_workers(&app);
        skwir_core::scheduler::mark_window_thread(&app.scheduler);
        let target = tmp.path().join("created after dispatch.png");
        app.dispatch(&Invocation::with_args(
            "open_with",
            &serde_json::json!({
                "app": "viewer.desktop", "set_default": true, "entries": [target],
            }),
        ))
        .unwrap();
        std::fs::write(&target, b"").unwrap();
        drop(blocked);
        assert_eq!(recorded(&out), [target.display().to_string()]);
        assert!(app.scheduler.wait_idle(Duration::from_secs(10)));
        assert_eq!(
            next_notification(&app).text,
            "Viewer now opens PNG image files"
        );
    }

    #[cfg(unix)]
    #[test]
    fn open_with_launches_the_application_on_the_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        std::fs::create_dir_all(&root).unwrap();
        let names = ["a b.png", "it's.png"];
        for name in names {
            std::fs::write(root.join(name), b"").unwrap();
        }
        let out = tmp.path().join("out");
        let record = recorder(tmp.path(), &out);
        let plugin = OpenWithPlugin {
            catalog: {
                fake_catalog(tmp.path(), &record);
                cached(tmp.path())
            },
        };
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(plugin), &app);
        let pane = app.workspace.read().active_id();
        app.navigate(pane, &Location::from_path(&root)).unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
        select(&app, pane, &names);
        app.dispatch(&Invocation::with_args(
            "open_with",
            &serde_json::json!({ "app": "viewer.desktop", "set_default": true }),
        ))
        .unwrap();
        let want: Vec<String> = names
            .iter()
            .map(|n| root.join(n).display().to_string())
            .collect();
        assert_eq!(recorded(&out), want, "one run with every file, as %F says");
        assert_eq!(
            next_notification(&app).text,
            "Viewer now opens PNG image files",
            "and it is their default from now on"
        );
    }

    /// The next notification, waiting for a worker to send it.
    fn next_notification(app: &App) -> skwir_core::app::Notification {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(n) = app.take_notifications().into_iter().next() {
                return n;
            }
            assert!(std::time::Instant::now() < deadline, "nothing was said");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    // The free desktops' files (step E, ADR-035).
    #[cfg(target_os = "linux")]
    #[test]
    fn a_chosen_default_is_written_said_and_undone_then_redone() {
        let tmp = tempfile::tempdir().unwrap();
        fake_catalog(tmp.path(), std::path::Path::new("record"));
        let list = tmp.path().join("mimeapps.list");
        std::fs::write(
            &list,
            "# kept\n[Default Applications]\nimage/png=viewer.desktop\ntext/plain=editor.desktop\n",
        )
        .unwrap();
        let before = std::fs::read_to_string(&list).unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        let plugin = OpenWithPlugin {
            catalog: cached(tmp.path()),
        };
        app.plugins.add(Arc::new(plugin), &app);
        app.dispatch(&Invocation::with_args(
            "open_with.set_default",
            &serde_json::json!({ "mime": "image/png", "app": "editor.desktop" }),
        ))
        .unwrap();
        let said = next_notification(&app);
        assert_eq!(said.text, "Editor now opens PNG image files");
        let undo = said.action.expect("Undo");
        assert_eq!(undo.label, "Undo");
        let written = std::fs::read_to_string(&list).unwrap();
        assert!(written.starts_with(
            "# kept\n[Default Applications]\nimage/png=editor.desktop;\ntext/plain=editor.desktop\n"
        ));
        assert!(written.contains("[Added Associations]\nimage/png=editor.desktop;\n"));

        app.dispatch(&undo.command).unwrap();
        let said = next_notification(&app);
        assert_eq!(said.text, "Viewer now opens PNG image files");
        let redo = said.action.expect("Redo");
        assert_eq!(redo.label, "Redo");
        assert_eq!(
            std::fs::read_to_string(&list).unwrap(),
            before.clone() + "\n[Added Associations]\n",
            "the lines as they were; an emptied group stays"
        );

        app.dispatch(&redo.command).unwrap();
        let said = next_notification(&app);
        assert_eq!(said.text, "Editor now opens PNG image files");
        assert_eq!(said.action.expect("Undo again").label, "Undo");
    }

    /// Properties' Opens with: the application a file's type opens with,
    /// a picker of choices that change it, and a new answer once changed.
    // The free desktops' files (step E, ADR-035).
    #[cfg(target_os = "linux")]
    #[test]
    fn properties_say_what_a_file_opens_with_and_change_it() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        std::fs::create_dir_all(&root).unwrap();
        let photo = root.join("it's a “photo” .png");
        std::fs::write(&photo, b"").unwrap();
        fake_catalog(tmp.path(), std::path::Path::new("record"));
        let cache = cached(tmp.path());
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(
            Arc::new(OpenWithPlugin {
                catalog: Arc::clone(&cache),
            }),
            &app,
        );
        let source = app.registry.property_sources.all();
        assert_eq!(source.len(), 1);
        let ask = |path: &std::path::Path| {
            let Outcome::Value(answer) = app
                .dispatch(&source[0].command.clone().with(
                    "entries",
                    serde_json::json!([skwir_core::op::EntryRef::from_path(path).unwrap()]),
                ))
                .unwrap()
            else {
                panic!("rows are a value")
            };
            answer["rows"].clone()
        };
        let rows = ask(&photo);
        assert_eq!(rows[0]["group"], "General");
        assert_eq!(rows[0]["label"], "Opens with");
        assert_eq!(rows[0]["value"], "Viewer");
        let change = &rows[0]["actions"][0]["command"];
        assert_eq!(change["name"], "ui.choose");
        assert_eq!(change["args"]["title"], "Open PNG image files with");
        let source_args = &change["args"]["source"];
        let Outcome::Value(choices) = app
            .dispatch(&Invocation::with_args(
                source_args["name"].as_str().unwrap(),
                &source_args["args"],
            ))
            .unwrap()
        else {
            panic!("choices are a value")
        };
        let editor = choices["choices"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["title"] == "Editor")
            .unwrap();
        assert_eq!(choices["choices"][0]["detail"], "Now");
        assert_eq!(editor["command"]["name"], "open_with.set_default");
        assert!(choices.get("option").is_none(), "choosing is the change");
        let before = source[0]
            .revision
            .load(std::sync::atomic::Ordering::Acquire);
        app.dispatch(&serde_json::from_value(editor["command"].clone()).unwrap())
            .unwrap();
        assert_eq!(
            next_notification(&app).text,
            "Editor now opens PNG image files"
        );
        assert!(
            source[0]
                .revision
                .load(std::sync::atomic::Ordering::Acquire)
                > before,
            "Properties asks again"
        );
        assert_eq!(ask(&photo)[0]["value"], "Editor");
        // A folder opens in skwir: nothing to say.
        assert_eq!(ask(&root), serde_json::json!([]));
    }

    #[test]
    fn only_the_users_lists_are_rewritten() {
        let tmp = tempfile::tempdir().unwrap();
        fake_catalog(tmp.path(), std::path::Path::new("record"));
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        let plugin = OpenWithPlugin {
            catalog: cached(tmp.path()),
        };
        app.plugins.add(Arc::new(plugin), &app);
        let elsewhere = tmp.path().join("notes");
        std::fs::write(&elsewhere, "mine\n").unwrap();
        assert!(
            app.dispatch(&Invocation::with_args(
                "open_with.edit_associations",
                &serde_json::json!({
                    "file": PathArg(elsewhere.clone()),
                    "mime": "image/png",
                    "default": "viewer.desktop;",
                }),
            ))
            .is_err()
        );
        assert_eq!(std::fs::read_to_string(&elsewhere).unwrap(), "mine\n");
    }

    // Types from the free desktops' database (step E, ADR-035).
    #[cfg(target_os = "linux")]
    #[test]
    fn a_type_is_named_as_a_sentence_names_it() {
        assert_eq!(files_of("inode/directory"), "folders");
        assert_eq!(files_of("image/png"), "PNG image files");
        assert_eq!(files_of("text/plain"), "plain text document files");
        assert_eq!(
            files_of("application/x-no-such-type"),
            "application/x-no-such-type files"
        );
    }
}
