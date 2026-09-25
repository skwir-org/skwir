//! bibli plugin: places. Fills the sidebar from what the desktop already
//! knows: home, XDG user dirs, GTK bookmarks, mounted volumes.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use bibli_core::app::{App, NotifyLevel};
use bibli_core::cancel::CancelToken;
use bibli_core::command::{Invocation, NoArgs, Outcome};
use bibli_core::location::Location;
use bibli_core::os_path::PathArg;
use bibli_core::plugin::Plugin;
use bibli_core::registry::{KeyBinding, MenuItem, Place};
use bibli_core::scheduler::Priority;
use bibli_core::workspace::PaneId;
use bibli_platform::places::BookmarkEdit;
use parking_lot::Mutex;

const ID: &str = "places";
/// A disk brings several partitions: events this close together are one change.
const SETTLE: Duration = Duration::from_millis(400);

/// The places plugin.
#[derive(Default)]
pub struct PlacesPlugin {
    places: Places,
    /// Watches for volumes being plugged, unplugged, mounted or unmounted,
    /// and for the bookmarks changing, whoever changes them.
    watcher: Watcher,
}

/// The plugin's watcher, shared with the bookmark commands: the first
/// bookmark can create the folder that is then to be watched.
#[derive(Clone, Default)]
struct Watcher(Arc<Mutex<Option<notify::RecommendedWatcher>>>);

impl Watcher {
    /// Watch the bookmarks' folders that exist; watching one again is
    /// harmless.
    fn watch_bookmarks(&self) {
        use notify::Watcher as _;
        if let Some(watcher) = self.0.lock().as_mut() {
            for folder in bibli_platform::places::bookmarks_folders() {
                let _ = watcher.watch(&folder, notify::RecursiveMode::NonRecursive);
            }
        }
    }
}

/// What the plugin's workers share: which scan is the latest, and the
/// plugin's lifetime.
#[derive(Clone, Default)]
struct Places(Arc<Mutex<Publisher>>);

#[derive(Default)]
struct Publisher {
    /// Bumped by every scan; only the latest one may publish, so a slow
    /// scan from before a mount cannot overwrite the list from after it.
    generation: u64,
    /// Cancelled on disable: nothing the plugin started publishes after.
    lifetime: Option<CancelToken>,
    /// What the worker published last, measured: the quick scan borrows
    /// from it until the worker measures again.
    measured: Vec<Place>,
}

/// The quick scan, with what the worker measured last (a volume's space, a
/// bookmark found missing, the volumes present but not mounted) until it
/// measures again: otherwise every rescan (a bookmark renamed, a disk
/// plugged) blanks them for as long as the worker takes.
fn carry_over(fresh: &[Place], measured: &[Place]) -> Vec<Place> {
    let mut out: Vec<Place> = fresh
        .iter()
        .cloned()
        .map(|mut place| {
            if let Some(before) = measured
                .iter()
                .find(|m| m.group == place.group && m.location == place.location)
            {
                place.space = before.space;
                place.unavailable.clone_from(&before.unavailable);
            }
            place
        })
        .collect();
    out.extend(measured.iter().filter(|m| m.activate.is_some()).cloned());
    out
}

/// `4.0 TB`, for a volume without a label.
fn size_label(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000. && unit < UNITS.len() - 1 {
        value /= 1000.;
        unit += 1;
    }
    // Disks are sold in powers of ten; match the number on the label.
    format!("{value:.0} {}", UNITS[unit])
}

/// A present but unmounted volume as a sidebar entry: activating it mounts
/// it, asking for the passphrase first when it is locked.
fn volume_place(volume: &bibli_platform::places::Volume) -> Place {
    use bibli_core::command::Invocation;
    let title = match (volume.label.is_empty(), volume.locked) {
        (false, _) => volume.label.clone(),
        (true, true) => format!("{} encrypted", size_label(volume.size)),
        (true, false) => format!("{} volume", size_label(volume.size)),
    };
    let activate = if volume.locked {
        // The shell asks for the passphrase and calls `places.unlock`.
        Invocation::with_args(
            "ui.unlock_prompt",
            &serde_json::json!({ "device": volume.device, "title": title }),
        )
    } else {
        Invocation::with_args(
            "places.mount",
            &serde_json::json!({ "device": volume.device }),
        )
    };
    let unavailable = if volume.locked {
        "Locked. Click to enter its passphrase and open it."
    } else {
        "Not mounted. Click to mount and open it."
    };
    Place {
        title,
        location: Location::parse(&format!("device:{}", volume.device)),
        icon: Some(
            if volume.locked {
                "drive-harddisk-encrypted"
            } else if volume.hotplug {
                "drive-removable-media"
            } else {
                "drive-harddisk"
            }
            .into(),
        ),
        group: "devices".into(),
        space: None,
        eject: None,
        activate: Some(activate),
        unavailable: Some(unavailable.into()),
        menu: Vec::new(),
    }
}

fn icon_for_user_dir(kind: &str) -> &'static str {
    match kind {
        "desktop" => "user-desktop",
        "download" => "folder-download",
        "documents" => "folder-documents",
        "pictures" => "folder-pictures",
        "music" => "folder-music",
        "videos" => "folder-videos",
        "templates" => "folder-templates",
        "publicshare" => "folder-publicshare",
        _ => "folder",
    }
}

/// An item of a place's own menu.
fn menu_item(title: &str, command: Invocation, order: i32, icon: &str) -> MenuItem {
    MenuItem {
        menu: "place".into(),
        title: title.into(),
        command,
        when: None,
        order,
        icon: Some(icon.into()),
        submenu: None,
    }
}

/// The menu of a folder that is not a bookmark: it can become one.
fn folder_menu(path: &Path) -> Vec<MenuItem> {
    vec![menu_item(
        "Add to Bookmarks",
        Invocation::with_args(
            "places.bookmark",
            &serde_json::json!({ "path": PathArg::from(path) }),
        ),
        200,
        "bookmark",
    )]
}

/// The menu of a bookmark: rename it, remove it.
fn bookmark_menu(bookmark: &bibli_platform::places::Bookmark) -> Vec<MenuItem> {
    let path = PathArg::from(bookmark.path.as_path());
    vec![
        menu_item(
            "Rename…",
            Invocation::with_args(
                "ui.ask",
                &serde_json::json!({
                    "title": "Rename bookmark",
                    "placeholder": "Name (empty: the folder's own)",
                    "text": bookmark.title,
                    "command": "places.rename_bookmark",
                    "args": { "path": path },
                    "field": "title",
                }),
            ),
            100,
            "rename",
        ),
        menu_item(
            "Remove from Bookmarks",
            Invocation::with_args("places.unbookmark", &serde_json::json!({ "path": path })),
            300,
            "minus",
        ),
    ]
}

/// Scan the environment and return every place, in sidebar order.
pub fn scan() -> Vec<Place> {
    let folder = |title: String, path: PathBuf, icon: &str, group: &str| Place {
        title,
        menu: folder_menu(&path),
        location: Location::from_path(path),
        icon: Some(icon.into()),
        group: group.into(),
        space: None,
        eject: None,
        activate: None,
        unavailable: None,
    };
    let mut out = Vec::new();
    if let Some(home) = bibli_platform::places::home() {
        out.push(folder("Home".into(), home, "user-home", "places"));
    }
    for d in bibli_platform::places::user_dirs() {
        out.push(folder(d.name, d.path, icon_for_user_dir(&d.kind), "places"));
    }
    for b in bibli_platform::places::bookmarks() {
        out.push(Place {
            menu: bookmark_menu(&b),
            ..folder(b.title, b.path, "folder", "bookmarks")
        });
    }
    for m in bibli_platform::places::mounts() {
        let icon = if m.removable {
            "drive-removable-media"
        } else {
            "drive-harddisk"
        };
        out.push(Place {
            // Only what the user mounted: system volumes are not ours to detach.
            eject: m.removable.then(|| {
                Invocation::with_args(
                    "places.unmount",
                    &serde_json::json!({ "path": PathArg::from(m.path.as_path()) }),
                )
            }),
            ..folder(m.label, m.path, icon, "devices")
        });
    }
    out
}

impl Places {
    /// The plugin's lifetime, for the jobs it starts; `None` once disabled.
    fn lifetime(&self) -> Option<CancelToken> {
        self.0.lock().lifetime.as_ref().map(CancelToken::child)
    }

    /// Publish `places` if the scan numbered `generation` is still the latest
    /// and the plugin is enabled. Under the lock, so a newer scan cannot slip
    /// in between the check and the publication (the registry itself refuses
    /// a disabled owner).
    fn publish(&self, app: &App, generation: u64, places: Vec<Place>, measured: bool) {
        let mut publisher = self.0.lock();
        if publisher.generation != generation
            || publisher
                .lifetime
                .as_ref()
                .is_none_or(CancelToken::is_cancelled)
        {
            return;
        }
        if measured {
            publisher.measured.clone_from(&places);
        }
        app.registry.places.replace_owner(ID, places);
        drop(publisher);
        app.wake();
    }

    /// Publish the places now; ask each volume for its free space on a
    /// worker (a sleeping or unreachable volume can block that call) and
    /// publish again with the answers.
    fn populate(&self, app: &App) {
        let places = scan();
        let (generation, lifetime, quick) = {
            let mut publisher = self.0.lock();
            publisher.generation += 1;
            (
                publisher.generation,
                publisher.lifetime.clone(),
                carry_over(&places, &publisher.measured),
            )
        };
        let Some(lifetime) = lifetime else { return };
        self.publish(app, generation, quick, false);
        let (this, weak) = (self.clone(), app.weak());
        app.scheduler.spawn(
            Priority::Background,
            "places-space",
            lifetime.child(),
            move || {
                let mut measured: Vec<Place> = places
                    .into_iter()
                    .map(|mut place| {
                        if place.group == "devices" {
                            place.space = place
                                .location
                                .path()
                                .and_then(|path| bibli_platform::places::space(&path));
                        }
                        // A bookmark outlives its folder: it may have moved,
                        // or live on a volume that is not mounted.
                        if place.group == "bookmarks"
                            && place.location.path().is_some_and(|path| !path.is_dir())
                        {
                            place.unavailable = Some(
                                "Not found. It may have moved, or be on a volume that is not mounted."
                                    .into(),
                            );
                        }
                        place
                    })
                    .collect();
                // Volumes that are plugged in but not mounted: `lsblk` is a
                // process, so it belongs here and not in the synchronous scan.
                measured.extend(
                    bibli_platform::places::unmounted_volumes()
                        .iter()
                        .map(volume_place),
                );
                if let Some(app) = weak.upgrade() {
                    this.publish(&app, generation, measured, true);
                }
            },
        );
    }

    /// Run `job` on a worker for as long as the plugin is enabled.
    fn spawn(&self, app: &App, label: &'static str, job: impl FnOnce(&App) + Send + 'static) {
        let Some(lifetime) = self.lifetime() else {
            return;
        };
        let weak = app.weak();
        app.scheduler
            .spawn(Priority::Normal, label, lifetime, move || {
                if let Some(app) = weak.upgrade() {
                    job(&app);
                }
            });
    }
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct MountArgs {
    /// Block device, as listed in the sidebar (`/dev/sdb1`).
    device: String,
    #[serde(default)]
    pane: Option<bibli_core::workspace::PaneId>,
}

// No `Debug`: the passphrase must not reach a log through a formatter.
#[derive(serde::Deserialize, schemars::JsonSchema)]
struct UnlockArgs {
    device: String,
    /// Secret. Handed to udisks on standard input; never stored or logged.
    passphrase: String,
    #[serde(default)]
    pane: Option<bibli_core::workspace::PaneId>,
}

/// Unlock if a passphrase is given, mount, republish the places and open the
/// volume in the pane that asked. Everything blocks, so it runs on a worker;
/// the pane is resolved now, not when the work ends.
fn attach(
    places: &Places,
    app: &App,
    device: String,
    passphrase: Option<String>,
    pane: Option<bibli_core::workspace::PaneId>,
) {
    let pane = app.pane_or_active(pane);
    let this = places.clone();
    places.spawn(app, "places-attach", move |app| {
        use bibli_core::app::NotifyLevel;
        let mounted = passphrase
            .map_or_else(
                || Ok(device.clone()),
                |passphrase| bibli_platform::places::unlock(&device, &passphrase),
            )
            .and_then(|cleartext| bibli_platform::places::mount(&cleartext));
        // Whatever happened, the set of volumes may have changed (a
        // container can unlock and its filesystem still fail to mount).
        this.populate(app);
        match mounted {
            Ok(path) => {
                let name = path.file_name().map_or_else(
                    || path.display().to_string(),
                    |name| name.to_string_lossy().into_owned(),
                );
                match app.navigate(pane, &Location::from_path(&path)) {
                    Ok(()) => app.notify(NotifyLevel::Info, format!("Mounted “{name}”")),
                    Err(error) => app.notify(NotifyLevel::Error, format!("{error:#}")),
                }
            }
            Err(error) => app.notify(
                NotifyLevel::Error,
                format!("The volume was not mounted: {error:#}"),
            ),
        }
    });
}

impl PlacesPlugin {
    /// Republish when a volume appears, disappears, or is mounted by anyone:
    /// udev keeps `/dev/disk/by-uuid` and udisks keeps `/run/media/<user>`;
    /// and when the bookmarks change, from here or another file manager.
    /// Bursts (a disk brings several partitions) collapse into one refresh:
    /// the plugin's own thread waits for the events to settle, so no shared
    /// worker sleeps. It ends when the watcher (and its sender) is dropped.
    fn watch(&self, app: &App) {
        use notify::Watcher as _;
        let (tx, rx) = crossbeam_channel::unbounded::<()>();
        // The bookmarks' folder holds other settings: only the file counts
        // (writers replace it, so its name is among the event's paths).
        let settings = bibli_platform::places::bookmarks_folders();
        let relevant = move |path: &PathBuf| {
            path.file_name().is_some_and(|name| name == "bookmarks")
                || !settings.iter().any(|folder| path.parent() == Some(folder))
        };
        let handler = move |event: notify::Result<notify::Event>| {
            if event.is_ok_and(|event| event.paths.iter().any(&relevant)) {
                let _ = tx.send(());
            }
        };
        let Ok(mut watcher) = notify::recommended_watcher(handler) else {
            return;
        };
        let (places, weak) = (self.places.clone(), app.weak());
        let settle = std::thread::Builder::new()
            .name("bibli-places".into())
            .spawn(move || {
                while rx.recv().is_ok() {
                    while rx.recv_timeout(SETTLE).is_ok() {}
                    let Some(app) = weak.upgrade() else { return };
                    places.populate(&app);
                }
            });
        if settle.is_err() {
            return;
        }
        let mut folders = vec![PathBuf::from("/dev/disk/by-uuid")];
        if let Some(user) = std::env::var_os("USER") {
            folders.push(std::path::Path::new("/run/media").join(user));
        }
        for folder in folders {
            // A folder that does not exist yet is simply not watched.
            let _ = watcher.watch(&folder, notify::RecursiveMode::NonRecursive);
        }
        *self.watcher.0.lock() = Some(watcher);
        self.watcher.watch_bookmarks();
    }
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct PathArgs {
    /// A volume's mount point.
    #[serde(default)]
    path: Option<PathArg>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct TargetArgs {
    /// A folder. When omitted: the pane's single selected folder, or the
    /// folder it shows when nothing is selected.
    #[serde(default)]
    path: Option<PathArg>,
    /// The folder the pane shows, whatever is selected.
    #[serde(default)]
    shown: bool,
    #[serde(default)]
    pane: Option<PaneId>,
    /// Set by a toast's Undo: the result offers Redo instead.
    #[serde(default)]
    undo: bool,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct BookmarkArgs {
    #[serde(flatten)]
    target: TargetArgs,
    /// Its label; the folder's own name when omitted.
    #[serde(default)]
    title: Option<String>,
    /// The bookmark it goes before; last when omitted.
    #[serde(default)]
    before: Option<PathArg>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct RenameArgs {
    /// The bookmarked folder.
    path: PathArg,
    /// Its new label; empty for the folder's own name.
    title: String,
    #[serde(default)]
    undo: bool,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct MoveArgs {
    /// The bookmarked folder.
    path: PathArg,
    /// The bookmark it goes before; last when omitted.
    #[serde(default)]
    before: Option<PathArg>,
    #[serde(default)]
    undo: bool,
}

/// The folder a bookmark command is about: the one named, else the pane's
/// single selected folder, else the folder the pane shows.
fn target(app: &App, args: TargetArgs) -> anyhow::Result<PathBuf> {
    if let Some(path) = args.path {
        return Ok(path.into_inner());
    }
    let pane = app.pane_or_active(args.pane);
    if !args.shown {
        match app.selected_refs(pane).as_slice() {
            [] => {}
            [one] => {
                let path = one.resolve()?;
                anyhow::ensure!(path.is_dir(), "only folders can be bookmarked");
                return Ok(path);
            }
            _ => anyhow::bail!("select one folder"),
        }
    }
    app.pane_dir(pane)
}

fn name_of(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// The command that applies `edit`, from a toast's button.
fn invocation(edit: &BookmarkEdit, undo: bool) -> Invocation {
    let arg = |path: &Path| PathArg::from(path);
    let before = |before: &Option<PathBuf>| before.as_deref().map(arg);
    let (name, args) = match edit {
        BookmarkEdit::Add {
            path,
            title,
            before: b,
        } => (
            "places.bookmark",
            serde_json::json!({ "path": arg(path), "title": title, "before": before(b) }),
        ),
        BookmarkEdit::Remove { path } => (
            "places.unbookmark",
            serde_json::json!({ "path": arg(path) }),
        ),
        BookmarkEdit::Rename { path, title } => (
            "places.rename_bookmark",
            serde_json::json!({ "path": arg(path), "title": title.clone().unwrap_or_default() }),
        ),
        BookmarkEdit::Move { path, before: b } => (
            "places.move_bookmark",
            serde_json::json!({ "path": arg(path), "before": before(b) }),
        ),
    };
    let mut args = args;
    args["undo"] = serde_json::json!(undo);
    Invocation::with_args(name, &args)
}

/// What an edit did, in a sentence, or what it found when it did nothing.
/// `shown` is the bookmark's label before the edit, when it had one.
fn describe(edit: &BookmarkEdit, shown: Option<&str>, done: bool) -> Option<String> {
    let own = || name_of(edit.path());
    let before = || shown.map_or_else(own, str::to_owned);
    Some(match (edit, done) {
        (BookmarkEdit::Add { title, .. }, true) => {
            format!("Bookmarked “{}”", title.clone().unwrap_or_else(own))
        }
        (BookmarkEdit::Add { .. }, false) => format!("“{}” is already bookmarked", before()),
        (BookmarkEdit::Remove { .. }, true) => {
            format!("Removed “{}” from bookmarks", before())
        }
        (BookmarkEdit::Remove { .. }, false) => format!("“{}” is not bookmarked", own()),
        (BookmarkEdit::Rename { title, .. }, true) => format!(
            "Renamed bookmark “{}” to “{}”",
            before(),
            title
                .as_deref()
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map_or_else(own, str::to_owned)
        ),
        (BookmarkEdit::Move { .. }, true) => format!("Moved bookmark “{}”", before()),
        // Renamed to its own name, dropped where it was: nothing to say.
        (BookmarkEdit::Rename { .. } | BookmarkEdit::Move { .. }, false) => return None,
    })
}

impl Places {
    /// Apply `edit` to the bookmarks on a worker, republish, and say what
    /// happened with the way back: Undo, or Redo when this was an undo.
    fn edit(&self, app: &App, edit: BookmarkEdit, undo: bool, watcher: Watcher) {
        let this = self.clone();
        self.spawn(app, "places-bookmarks", move |app| {
            // A new bookmark is a folder; one coming back (an undone
            // removal) may be missing, and was a bookmark all the same.
            if let BookmarkEdit::Add { path, .. } = &edit
                && !undo
                && !path.is_dir()
            {
                app.notify(
                    NotifyLevel::Error,
                    format!(
                        "“{}” is not a folder: only folders can be bookmarked",
                        name_of(path)
                    ),
                );
                return;
            }
            let shown = bibli_platform::places::bookmarks()
                .into_iter()
                .find(|b| b.path == edit.path())
                .map(|b| b.title);
            match bibli_platform::places::edit_bookmarks(&edit) {
                Ok(Some(inverse)) => {
                    watcher.watch_bookmarks();
                    this.populate(app);
                    if let Some(text) = describe(&edit, shown.as_deref(), true) {
                        app.notify_with_action(
                            NotifyLevel::Info,
                            text,
                            if undo { "Redo" } else { "Undo" },
                            invocation(&inverse, !undo),
                        );
                    }
                }
                Ok(None) => {
                    if let Some(text) = describe(&edit, shown.as_deref(), false) {
                        app.notify(NotifyLevel::Info, text);
                    }
                }
                Err(error) => app.notify(
                    NotifyLevel::Error,
                    format!("The bookmarks were not changed: {error:#}"),
                ),
            }
        });
    }
}

impl Plugin for PlacesPlugin {
    fn id(&self) -> &'static str {
        ID
    }

    fn title(&self) -> &'static str {
        "Places"
    }

    fn description(&self) -> &'static str {
        "Sidebar entries from your home, XDG user folders, GTK bookmarks, and mounted volumes"
    }

    fn category(&self) -> bibli_core::plugin::Category {
        bibli_core::plugin::Category::Browsing
    }

    fn requirements(&self) -> Vec<bibli_core::plugin::Requirement> {
        vec![bibli_core::plugin::Requirement::program(
            "udisksctl",
            "Unmounting removable volumes from the sidebar",
        )]
    }

    fn on_disable(&self, _app: &App) {
        *self.watcher.0.lock() = None;
        if let Some(lifetime) = self.places.0.lock().lifetime.take() {
            lifetime.cancel();
        }
    }

    fn register(&self, app: &App) {
        if let Some(previous) = self
            .places
            .0
            .lock()
            .lifetime
            .replace(app.lifetime().child())
        {
            previous.cancel();
        }
        self.places.populate(app);
        let places = self.places.clone();
        app.commands.register::<NoArgs, _>(
            ID,
            "places.refresh",
            "Refresh places",
            "Re-scan user folders, bookmarks, and mounts",
            move |_, app| {
                places.populate(app);
                Ok(Outcome::Done)
            },
        );
        let (places, watcher) = (self.places.clone(), self.watcher.clone());
        app.commands.register::<BookmarkArgs, _>(
            ID,
            "places.bookmark",
            "Add to bookmarks",
            "Add a folder to the desktop's shared bookmarks (the GTK bookmarks file)",
            move |args, app| {
                let undo = args.target.undo;
                let path = target(app, args.target)?;
                let edit = BookmarkEdit::Add {
                    path,
                    title: args.title,
                    before: args.before.map(PathArg::into_inner),
                };
                places.edit(app, edit, undo, watcher.clone());
                Ok(Outcome::Done)
            },
        );
        let (places, watcher) = (self.places.clone(), self.watcher.clone());
        app.commands.register::<TargetArgs, _>(
            ID,
            "places.unbookmark",
            "Remove from bookmarks",
            "Remove a folder from the desktop's shared bookmarks; the folder stays where it is",
            move |args, app| {
                let undo = args.undo;
                let path = target(app, args)?;
                places.edit(app, BookmarkEdit::Remove { path }, undo, watcher.clone());
                Ok(Outcome::Done)
            },
        );
        let (places, watcher) = (self.places.clone(), self.watcher.clone());
        app.commands.register::<RenameArgs, _>(
            ID,
            "places.rename_bookmark",
            "Rename bookmark",
            "Change the name a bookmark shows in the sidebar; the folder keeps its own",
            move |args, app| {
                let edit = BookmarkEdit::Rename {
                    path: args.path.into_inner(),
                    title: Some(args.title),
                };
                places.edit(app, edit, args.undo, watcher.clone());
                Ok(Outcome::Done)
            },
        );
        let (places, watcher) = (self.places.clone(), self.watcher.clone());
        app.commands.register::<MoveArgs, _>(
            ID,
            "places.move_bookmark",
            "Move bookmark",
            "Put a bookmark before another in the sidebar, or last",
            move |args, app| {
                let edit = BookmarkEdit::Move {
                    path: args.path.into_inner(),
                    before: args.before.map(PathArg::into_inner),
                };
                places.edit(app, edit, args.undo, watcher.clone());
                Ok(Outcome::Done)
            },
        );
        let places = self.places.clone();
        app.commands.register::<PathArgs, _>(
            ID,
            "places.unmount",
            "Unmount volume",
            "Detach a removable volume through udisks so it can be unplugged",
            move |args, app| {
                let path = args
                    .path
                    .ok_or_else(|| anyhow::anyhow!("which volume? pass its mount point"))?;
                let mount = bibli_platform::places::mounts()
                    .into_iter()
                    .find(|mount| mount.path == *path && mount.removable)
                    .ok_or_else(|| anyhow::anyhow!("not a removable volume"))?;
                let this = places.clone();
                places.spawn(app, "places-unmount", move |app| {
                    use bibli_core::app::NotifyLevel;
                    match bibli_platform::places::unmount(&mount.source) {
                        Ok(()) => {
                            this.populate(app);
                            app.notify(
                                NotifyLevel::Info,
                                format!("“{}” can be removed", mount.label),
                            );
                        }
                        Err(error) => app.notify(
                            NotifyLevel::Error,
                            format!("“{}” was not unmounted: {error:#}", mount.label),
                        ),
                    }
                });
                Ok(Outcome::Done)
            },
        );
        let places = self.places.clone();
        app.commands.register::<MountArgs, _>(
            ID,
            "places.mount",
            "Mount volume",
            "Mount a present volume through udisks and open it",
            move |args, app| {
                attach(&places, app, args.device, None, args.pane);
                Ok(Outcome::Done)
            },
        );
        let places = self.places.clone();
        app.commands.register::<UnlockArgs, _>(
            ID,
            "places.unlock",
            "Unlock volume",
            "Unlock an encrypted volume with its passphrase, mount it and open it. The passphrase is a secret: it is handed to udisks on standard input and never stored or logged",
            move |args, app| {
                attach(&places, app, args.device, Some(args.passphrase), args.pane);
                Ok(Outcome::Done)
            },
        );
        self.watch(app);
        let shown =
            || Invocation::with_args("places.bookmark", &serde_json::json!({ "shown": true }));
        for (title, command, when) in [
            (
                "Add to Bookmarks",
                Invocation::new("places.bookmark"),
                "single dir",
            ),
            // The folder's own menu, on its background.
            ("Bookmark This Folder", shown(), "empty"),
        ] {
            app.registry.menus.add(
                ID,
                MenuItem {
                    menu: "context".into(),
                    title: title.into(),
                    command,
                    when: Some(when.into()),
                    order: 250,
                    icon: Some("bookmark".into()),
                    submenu: None,
                },
            );
        }
        // As in Nautilus and browsers: the folder being looked at.
        app.registry.keymap.add(
            ID,
            KeyBinding {
                keys: "ctrl-d".into(),
                command: shown(),
                when: None,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bibli_core::app::AppConfig;

    #[test]
    fn registers_places_and_withdraws_them() {
        let tmp = tempfile::tempdir().unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(PlacesPlugin::default()), &app);
        let places = app.registry.places.all();
        assert!(
            places
                .iter()
                .any(|p| p.title == "Home" && p.group == "places")
        );
        assert!(app.commands.get("places.refresh").is_some());
        app.plugins.set_enabled(ID, false, &app);
        assert!(app.registry.places.all().is_empty());
        assert!(app.commands.get("places.refresh").is_none());
    }

    #[test]
    fn only_the_latest_scan_publishes_and_nothing_after_disable() {
        let tmp = tempfile::tempdir().unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        let places = Places::default();
        let place = |title: &str| Place {
            title: title.into(),
            location: Location::from_path("/"),
            icon: None,
            group: "places".into(),
            space: None,
            eject: None,
            activate: None,
            unavailable: None,
            menu: Vec::new(),
        };
        places.0.lock().lifetime = Some(CancelToken::new());
        places.0.lock().generation = 2;
        places.publish(&app, 2, vec![place("new")], true);
        // A scan started before the latest one finishes last: ignored.
        places.publish(&app, 1, vec![place("old")], true);
        let titles: Vec<String> = app
            .registry
            .places
            .all()
            .into_iter()
            .map(|p| p.title)
            .collect();
        assert_eq!(titles, ["new"]);
        places.0.lock().lifetime.take().unwrap().cancel();
        places.publish(&app, 2, vec![place("late")], true);
        assert_eq!(app.registry.places.all()[0].title, "new");
    }

    #[test]
    fn a_rescan_keeps_what_was_measured_until_it_is_measured_again() {
        let place = |group: &str, path: &str| Place {
            title: path.into(),
            location: Location::from_path(path),
            icon: None,
            group: group.into(),
            space: None,
            eject: None,
            activate: None,
            unavailable: None,
            menu: Vec::new(),
        };
        let mut disk = place("devices", "/run/media/example/disk");
        disk.space = Some((1, 2));
        let mut gone = place("bookmarks", "/example/gone");
        gone.unavailable = Some("Not found.".into());
        let mut unmounted = place("devices", "/dev/sdz1");
        unmounted.activate = Some(Invocation::new("places.mount"));
        let measured = [
            disk,
            gone,
            unmounted,
            place("bookmarks", "/example/renamed"),
        ];
        let fresh = [
            place("devices", "/run/media/example/disk"),
            place("bookmarks", "/example/gone"),
            place("bookmarks", "/example/new"),
        ];
        let quick = carry_over(&fresh, &measured);
        assert_eq!(quick[0].space, Some((1, 2)));
        assert!(quick[1].unavailable.is_some());
        assert!(quick[2].unavailable.is_none() && quick[2].space.is_none());
        assert!(quick[3].activate.is_some(), "volumes not mounted stay");
        assert_eq!(quick.len(), 4, "what the scan no longer finds goes");
    }

    /// The next message the plugin sends, within a deadline.
    fn next_notice(app: &App) -> bibli_core::app::Notification {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(notice) = app.take_notifications().into_iter().next() {
                return notice;
            }
            assert!(std::time::Instant::now() < deadline, "no message");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn bookmarks(app: &App) -> Vec<Place> {
        app.registry
            .places
            .all()
            .into_iter()
            .filter(|p| p.group == "bookmarks")
            .collect()
    }

    fn titles(app: &App) -> Vec<String> {
        bookmarks(app).into_iter().map(|p| p.title).collect()
    }

    /// The bookmark commands, through dispatch as the sidebar, the toasts
    /// and agents use them: every edit says what it did and its Undo
    /// (then Redo) takes it back.
    #[test]
    #[allow(unsafe_code)]
    fn bookmarks_are_edited_through_commands_and_undone() {
        let tmp = tempfile::tempdir().unwrap();
        // SAFETY: one test per process under nextest, set before the app
        // starts any thread; the bookmarks file is then this test's own.
        unsafe { std::env::set_var("XDG_CONFIG_HOME", tmp.path().join("config")) };
        let a = tmp.path().join("it's 100% a folder");
        let b = tmp.path().join("Ünïcode b");
        std::fs::create_dir(&a).unwrap();
        std::fs::create_dir(&b).unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(PlacesPlugin::default()), &app);
        let run = |name: &str, args: serde_json::Value| {
            app.dispatch(&Invocation::with_args(name, &args)).unwrap();
            next_notice(&app)
        };
        let undo = |notice: &bibli_core::app::Notification, label: &str| {
            let action = notice.action.as_ref().expect("a way back");
            assert_eq!(action.label, label);
            app.dispatch(&action.command).unwrap();
            next_notice(&app)
        };
        let path = |p: &Path| serde_json::json!(PathArg::from(p));

        // The folder the pane shows, as Ctrl+D and the pane's button do.
        let pane = app.workspace.read().active_id();
        app.navigate(pane, &Location::from_path(&a)).unwrap();
        let added = run("places.bookmark", serde_json::json!({ "shown": true }));
        assert_eq!(added.text, "Bookmarked “it's 100% a folder”");
        let again = run("places.bookmark", serde_json::json!({ "path": path(&a) }));
        assert_eq!(again.text, "“it's 100% a folder” is already bookmarked");
        assert!(again.action.is_none());
        run(
            "places.bookmark",
            serde_json::json!({ "path": path(&b), "title": "Bee" }),
        );
        assert_eq!(titles(&app), ["it's 100% a folder", "Bee"]);
        let menu = &bookmarks(&app)[0].menu;
        assert!(menu.iter().any(|m| m.title == "Remove from Bookmarks"));

        let moved = run(
            "places.move_bookmark",
            serde_json::json!({ "path": path(&b), "before": path(&a) }),
        );
        assert_eq!(moved.text, "Moved bookmark “Bee”");
        assert_eq!(titles(&app), ["Bee", "it's 100% a folder"]);

        let renamed = run(
            "places.rename_bookmark",
            serde_json::json!({ "path": path(&a), "title": "Reports" }),
        );
        assert_eq!(
            renamed.text,
            "Renamed bookmark “it's 100% a folder” to “Reports”"
        );
        assert_eq!(titles(&app), ["Bee", "Reports"]);
        let undone = undo(&renamed, "Undo");
        assert_eq!(titles(&app), ["Bee", "it's 100% a folder"]);
        undo(&undone, "Redo");
        assert_eq!(titles(&app), ["Bee", "Reports"]);

        let removed = run("places.unbookmark", serde_json::json!({ "path": path(&b) }));
        assert_eq!(removed.text, "Removed “Bee” from bookmarks");
        assert_eq!(titles(&app), ["Reports"]);
        undo(&removed, "Undo");
        assert_eq!(titles(&app), ["Bee", "Reports"], "back in its place, named");
        let file = std::fs::read_to_string(tmp.path().join("config/gtk-3.0/bookmarks")).unwrap();
        assert_eq!(
            file,
            format!(
                "{} Bee\n{} Reports\n",
                bibli_platform::uri::file_uri(&b),
                bibli_platform::uri::file_uri(&a)
            )
        );

        // Only folders; and a bookmark whose folder went is shown as such.
        let note = tmp.path().join("note.txt");
        std::fs::write(&note, b"x").unwrap();
        let refused = run(
            "places.bookmark",
            serde_json::json!({ "path": path(&note) }),
        );
        assert_eq!(refused.level, NotifyLevel::Error);
        std::fs::remove_dir(&b).unwrap();
        app.dispatch(&Invocation::new("places.refresh")).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while bookmarks(&app)[0].unavailable.is_none() {
            assert!(
                std::time::Instant::now() < deadline,
                "missing folder flagged"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(bookmarks(&app)[1].unavailable.is_none());
    }

    #[test]
    fn unmount_takes_a_path_that_is_not_utf8() {
        use std::os::unix::ffi::OsStrExt;
        let tmp = tempfile::tempdir().unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(PlacesPlugin::default()), &app);
        let path = std::path::Path::new(std::ffi::OsStr::from_bytes(b"/run/media/x/caf\xe9"));
        let invocation = bibli_core::command::Invocation::try_with_args(
            "places.unmount",
            &serde_json::json!({ "path": PathArg::from(path) }),
        )
        .unwrap();
        let error = app.dispatch(&invocation).unwrap_err().to_string();
        assert!(error.contains("not a removable volume"), "{error}");
    }
}
