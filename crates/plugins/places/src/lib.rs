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
use bibli_core::registry::{Keeper, MenuItem, Place, Reorder};
use bibli_core::scheduler::Priority;
use bibli_core::workspace::PaneId;
use bibli_platform::places::BookmarkEdit;
use parking_lot::Mutex;

const ID: &str = "places";
/// A disk brings several partitions: events this close together are one change.
const SETTLE: Duration = Duration::from_millis(400);
/// How long an unmount waits for what bibli let go of to close: its
/// listings close their directories on a worker, and a preview's reader or
/// player, and the terminal's shell after its `cd`, are processes of their
/// own. Past this, what still uses the volume is not ours.
const RELEASE: Duration = Duration::from_secs(3);

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
struct Watcher(Arc<Mutex<Option<bibli_core::fs::Settled>>>);

impl Watcher {
    /// Watch the bookmarks' folders that exist; watching one again is
    /// harmless.
    fn watch_bookmarks(&self) {
        if let Some(watcher) = self.0.lock().as_mut() {
            for folder in bibli_platform::places::bookmarks_folders() {
                watcher.watch(&folder);
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
    /// What was published last, to publish again when `busy` changes.
    shown: Vec<Place>,
    /// The places being unlocked, mounted or unmounted, by location, with
    /// what the row says meanwhile.
    busy: Vec<Busy>,
    /// The volumes the last measured scan listed that are the cleartext of
    /// a container, with the container's place: once unlocked, a container
    /// is listed as that volume, which is still the same work.
    cleartext: Vec<Cleartext>,
}

/// An unlocked container's volume, as `lsblk` lists it in its stead.
struct Cleartext {
    volume: Location,
    container: Location,
}

struct Busy {
    location: Location,
    /// The volume's device, which a rescan lists in its stead once the
    /// mount is gone: the kernel takes a mount out of the list when the
    /// unmount begins, before it has written what it still had to.
    device: Option<Location>,
    what: &'static str,
}

/// Whether the place at `place` is the one at `location`, or the volume
/// that container is listed as once unlocked.
fn is_place(place: &Location, location: &Location, cleartext: &[Cleartext]) -> bool {
    place == location
        || cleartext
            .iter()
            .any(|c| &c.volume == place && &c.container == location)
}

/// `places` as the work in progress leaves them. A container being
/// attached is unlocked before it is mounted, and listed meanwhile as its
/// cleartext volume: that volume says what the container's row said.
fn mark_busy(places: &mut [Place], busy: &[Busy], cleartext: &[Cleartext]) {
    for place in places {
        place.busy = busy
            .iter()
            .find(|b| {
                is_place(&place.location, &b.location, cleartext)
                    || b.device.as_ref() == Some(&place.location)
            })
            .map(|b| b.what.to_owned());
    }
}

/// The place `volume_place` publishes for the volume at `device`.
fn device_location(device: &str) -> Location {
    Location::parse(&format!("device:{device}"))
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

/// What names a volume in the user's arrangement of the sidebar
/// (`Place::key`): its filesystem, and what of it is mounted (`root`, a
/// subvolume: `/` and `/home` can be one filesystem's), so that it is one
/// place mounted or not, wherever it is mounted.
fn volume_key(identity: &bibli_platform::places::Identity, root: &Path) -> Option<String> {
    let uuid = identity.uuid.as_deref()?;
    Some(if root == Path::new("/") {
        format!("volume:{uuid}")
    } else {
        format!("volume:{uuid}{}", root.display())
    })
}

/// A present but unmounted volume as a sidebar entry: activating it mounts
/// it, asking for the passphrase first when it is locked. One udisks says
/// not to show (an EFI partition) is offered hidden, as is a small FAT or
/// NTFS partition where udev says nothing.
fn volume_place(volume: &bibli_platform::places::Volume) -> Place {
    use bibli_core::command::Invocation;
    let identity = bibli_platform::places::identity(&volume.device);
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
    let icon = if volume.locked {
        "drive-harddisk-encrypted"
    } else if volume.hotplug {
        "drive-removable-media"
    } else {
        "drive-harddisk"
    };
    Place {
        activate: Some(activate),
        unavailable: Some(unavailable.into()),
        key: volume_key(&identity, Path::new("/")),
        hidden: identity.ignored.unwrap_or(volume.system),
        ..Place::new("devices", title, icon, device_location(&volume.device))
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
    MenuItem::new("place", title, icon, order, command)
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

/// Asking for a bookmark's new name: its menu's Rename…, and F2.
fn bookmark_rename(bookmark: &bibli_platform::places::Bookmark) -> Invocation {
    Invocation::with_args(
        "ui.ask",
        &serde_json::json!({
            "title": "Rename bookmark",
            "placeholder": "Name (empty: the folder's own)",
            "text": bookmark.title,
            "command": "places.rename_bookmark",
            "args": { "path": PathArg::from(bookmark.path.as_path()) },
            "field": "title",
        }),
    )
}

/// Scan the environment and return every place, in sidebar order.
pub fn scan() -> Vec<Place> {
    let folder = |title: String, path: PathBuf, icon: &str, group: &str| Place {
        menu: folder_menu(&path),
        ..Place::new(group, title, icon, Location::from_path(path))
    };
    let mut out = Vec::new();
    if let Some(home) = bibli_platform::places::home() {
        out.push(folder("Home".into(), home, "user-home", "places"));
    }
    for d in bibli_platform::places::user_dirs() {
        out.push(folder(d.name, d.path, icon_for_user_dir(&d.kind), "places"));
    }
    for b in bibli_platform::places::bookmarks() {
        let path = serde_json::json!(PathArg::from(b.path.as_path()));
        // Rename… and the item that removes it are the sidebar's, from
        // `rename` and `forget`.
        out.push(Place {
            menu: Vec::new(),
            rename: Some(bookmark_rename(&b)),
            forget: Some(Invocation::with_args(
                "places.unbookmark",
                &serde_json::json!({ "path": path }),
            )),
            reorder: Some(Reorder {
                command: Invocation::with_args(
                    "places.move_bookmark",
                    &serde_json::json!({ "path": path }),
                ),
                before: path,
            }),
            ..folder(b.title, b.path, "folder", "bookmarks")
        });
    }
    for m in bibli_platform::places::mounts() {
        let icon = if m.removable {
            "drive-removable-media"
        } else {
            "drive-harddisk"
        };
        // What udisks says of it: an EFI partition mounted at /boot is
        // offered hidden. Read from memory (sysfs, udev's database).
        let identity = bibli_platform::places::identity(&m.source);
        out.push(Place {
            key: volume_key(&identity, &m.root),
            hidden: identity.ignored == Some(true),
            // Only what the user mounted: system volumes are not ours to detach.
            eject: m.udisks.then(|| {
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
    /// `measured` is what a measured scan found: the volumes that are an
    /// unlocked container's; `None` for the quick scan.
    fn publish(
        &self,
        app: &App,
        generation: u64,
        places: Vec<Place>,
        measured: Option<Vec<Cleartext>>,
    ) {
        let mut guard = self.0.lock();
        let publisher = &mut *guard;
        if publisher.generation != generation
            || publisher
                .lifetime
                .as_ref()
                .is_none_or(CancelToken::is_cancelled)
        {
            return;
        }
        if let Some(cleartext) = measured {
            publisher.measured.clone_from(&places);
            publisher.cleartext = cleartext;
        }
        let mut places = places;
        mark_busy(&mut places, &publisher.busy, &publisher.cleartext);
        publisher.shown.clone_from(&places);
        app.registry.places.replace_owner(ID, places);
        drop(guard);
        app.wake();
    }

    /// Say on the place at `location` what is being done to it, at once:
    /// the work runs on a worker and can take a while (udisks asks the
    /// kernel to open a container, a drive to spin up).
    fn start_busy(&self, app: &App, location: Location, what: &'static str) {
        self.start_busy_as(app, location, None, what);
    }

    /// [`Self::start_busy`], said also on the place of the volume's
    /// `device` while it is listed as one.
    fn start_busy_as(
        &self,
        app: &App,
        location: Location,
        device: Option<Location>,
        what: &'static str,
    ) {
        let mut publisher = self.0.lock();
        if publisher
            .lifetime
            .as_ref()
            .is_none_or(CancelToken::is_cancelled)
        {
            return;
        }
        publisher.busy.retain(|b| b.location != location);
        publisher.busy.push(Busy {
            location,
            device,
            what,
        });
        let mut places = std::mem::take(&mut publisher.shown);
        mark_busy(&mut places, &publisher.busy, &publisher.cleartext);
        publisher.shown.clone_from(&places);
        app.registry.places.replace_owner(ID, places);
        drop(publisher);
        app.wake();
    }

    /// The work on the place at `location` ended; `gone` when it took the
    /// place away (a volume mounted is no longer one to mount). The quick
    /// scan borrows the volumes the last measured one saw (`carry_over`)
    /// until `lsblk` runs again, on a worker behind whatever the volume
    /// just opened is listing: without this, a volume just mounted would
    /// be offered for mounting again until then. Scans already running
    /// saw the place before the work ended, so they no longer publish.
    /// Call before `populate`.
    /// A container unlocked and mounted takes with it the volume it was
    /// listed as in between.
    fn end_busy(&self, location: &Location, gone: bool) {
        let mut publisher = self.0.lock();
        let publisher = &mut *publisher;
        publisher.busy.retain(|b| &b.location != location);
        if gone {
            let cleartext = &publisher.cleartext;
            publisher
                .measured
                .retain(|p| !is_place(&p.location, location, cleartext));
        }
        publisher.generation += 1;
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
        self.publish(app, generation, quick, None);
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
                let volumes = bibli_platform::places::unmounted_volumes();
                measured.extend(volumes.iter().map(volume_place));
                let cleartext = volumes
                    .iter()
                    .filter_map(|v| {
                        Some(Cleartext {
                            volume: device_location(&v.device),
                            container: device_location(v.container.as_deref()?),
                        })
                    })
                    .collect();
                if let Some(app) = weak.upgrade() {
                    this.publish(&app, generation, measured, Some(cleartext));
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
    /// Open the volume in a tab behind the one shown, not in the pane.
    #[serde(default)]
    tab: bool,
}

// No `Debug`: the passphrase must not reach a log through a formatter.
#[derive(serde::Deserialize, schemars::JsonSchema)]
struct UnlockArgs {
    device: String,
    /// Secret. Handed to udisks on standard input; never stored or logged.
    passphrase: String,
    #[serde(default)]
    pane: Option<bibli_core::workspace::PaneId>,
    /// Open the volume in a tab behind the one shown, not in the pane.
    #[serde(default)]
    tab: bool,
}

/// Unlock if a passphrase is given, mount, republish the places and open the
/// volume in the pane that asked, or with `tab` in a tab behind the one
/// shown. Everything blocks, so it runs on a worker; the pane is resolved
/// now, not when the work ends.
fn attach(
    places: &Places,
    app: &App,
    device: String,
    passphrase: Option<String>,
    pane: Option<bibli_core::workspace::PaneId>,
    tab: bool,
) {
    let pane = app.pane_or_active(pane);
    let this = places.clone();
    // The place `volume_place` published for it.
    let location = Location::parse(&format!("device:{device}"));
    places.start_busy(
        app,
        location.clone(),
        if passphrase.is_some() {
            "Unlocking…"
        } else {
            "Mounting…"
        },
    );
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
        this.end_busy(&location, mounted.is_ok());
        this.populate(app);
        match mounted {
            Ok(path) => {
                let name = path.file_name().map_or_else(
                    || path.display().to_string(),
                    |name| name.to_string_lossy().into_owned(),
                );
                let location = Location::from_path(&path);
                let opened = if tab {
                    app.new_tab(Some(pane), Some(location), false).map(|_| ())
                } else {
                    app.navigate(pane, &location)
                };
                match opened {
                    Ok(()) if tab => app.notify(
                        NotifyLevel::Info,
                        format!("Mounted “{name}”, opened in a new tab"),
                    ),
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
    /// Bursts collapse into one refresh (`fs::Settled`).
    fn watch(&self, app: &App) {
        // The bookmarks' folder holds other settings: only the file counts
        // (writers replace it, so its name is among the event's paths).
        let settings = bibli_platform::places::bookmarks_folders();
        let relevant = move |path: &std::path::Path| {
            path.file_name().is_some_and(|name| name == "bookmarks")
                || !settings.iter().any(|folder| path.parent() == Some(folder))
        };
        let (places, weak) = (self.places.clone(), app.weak());
        let Some(mut watcher) =
            bibli_core::fs::Settled::start("bibli-places", SETTLE, relevant, move || {
                let Some(app) = weak.upgrade() else {
                    return false;
                };
                places.populate(&app);
                true
            })
        else {
            return;
        };
        let mut folders = vec![PathBuf::from("/dev/disk/by-uuid")];
        if let Some(user) = std::env::var_os("USER") {
            folders.push(std::path::Path::new("/run/media").join(user));
        }
        for folder in folders {
            // A folder that does not exist yet is simply not watched.
            watcher.watch(&folder);
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
                        app.notify_undoable(text, undo, invocation(&inverse, !undo));
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

    fn capabilities(&self) -> bibli_core::permissions::Capabilities {
        use bibli_core::permissions::Capability;
        bibli_core::permissions::Capabilities::of(&[Capability::Programs])
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
        app.commands
            .register::<NoArgs, _>(
                ID,
                "places.refresh",
                "Refresh places",
                "Re-scan user folders, bookmarks, and mounts",
                move |_, app| {
                    places.populate(app);
                    Ok(Outcome::Done)
                },
            )
            .shows();
        let (places, watcher) = (self.places.clone(), self.watcher.clone());
        app.commands.register::<BookmarkArgs, _>(
            ID,
            "places.bookmark",
            "Add to bookmarks",
            "Add a folder to the desktop's shared bookmarks (the GTK bookmarks file)",
            move |args, app| {
                let undo = args.target.undo;
                let path = target(app, args.target)?;
                // Restored or deleted, a folder of the trash leaves the
                // bookmark leading nowhere.
                if bibli_core::op::in_trash(&path).is_some() {
                    anyhow::bail!(
                        "“{}” is in the trash: restore it first",
                        path.file_name().unwrap_or(path.as_os_str()).display()
                    );
                }
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
                    .find(|mount| mount.path == *path && mount.udisks)
                    .ok_or_else(|| anyhow::anyhow!("not a removable volume"))?;
                let this = places.clone();
                let location = Location::from_path(&mount.path);
                places.start_busy_as(
                    app,
                    location.clone(),
                    Some(device_location(&mount.source)),
                    "Unmounting…",
                );
                // Nothing of bibli's may hold the volume: a pane showing it
                // goes home, the folders kept there close.
                let home = bibli_platform::places::home().unwrap_or_else(|| PathBuf::from("/"));
                app.release(&mount.path, &Location::from_path(home))?;
                let lifetime = places.lifetime();
                places.spawn(app, "places-unmount", move |app| {
                    use bibli_core::app::NotifyLevel;
                    // Written first, while still mounted: the row stays busy
                    // until the drive has it all, and a write that fails is
                    // reported instead of lost.
                    if let Err(error) = bibli_platform::places::flush(&mount.path) {
                        this.end_busy(&location, false);
                        this.populate(app);
                        app.notify(
                            NotifyLevel::Error,
                            format!(
                                "“{}” was not unmounted: not everything could be written to it ({error:#})",
                                mount.label
                            ),
                        );
                        return;
                    }
                    let deadline = std::time::Instant::now() + RELEASE;
                    let unmounted = loop {
                        match bibli_platform::places::unmount(&mount.source) {
                            Err(error)
                                if bibli_platform::places::is_busy(&error)
                                    && std::time::Instant::now() < deadline
                                    && !lifetime.as_ref().is_none_or(CancelToken::is_cancelled) =>
                            {
                                std::thread::sleep(Duration::from_millis(150));
                            }
                            result => break result,
                        }
                    };
                    this.end_busy(&location, unmounted.is_ok());
                    this.populate(app);
                    match unmounted {
                        Ok(()) => app.notify(
                            NotifyLevel::Info,
                            format!("“{}” can be removed", mount.label),
                        ),
                        // bibli let go of it: another program has it.
                        Err(error) if bibli_platform::places::is_busy(&error) => app.notify(
                            NotifyLevel::Error,
                            format!(
                                "“{}” was not unmounted: another program is using it",
                                mount.label
                            ),
                        ),
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
                attach(&places, app, args.device, None, args.pane, args.tab);
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
                attach(
                    &places,
                    app,
                    args.device,
                    Some(args.passphrase),
                    args.pane,
                    args.tab,
                );
                Ok(Outcome::Done)
            },
        );
        self.watch(app);
        let shown =
            || Invocation::with_args("places.bookmark", &serde_json::json!({ "shown": true }));
        for (title, command, when) in [
            // Not a folder of the trash, which `places.bookmark` refuses.
            (
                "Add to Bookmarks",
                Invocation::new("places.bookmark"),
                "single dir !place !restores",
            ),
            // The folder's own menu, on its background.
            ("Bookmark This Folder", shown(), "empty folder"),
        ] {
            app.registry.menus.add(
                ID,
                MenuItem::new("context", title, "bookmark", 250, command).when(when),
            );
        }
        // Ctrl+D and the pane's button (the core `keep` command) bookmark
        // the folder being looked at, as in Nautilus and browsers.
        app.registry.keepers.add(
            ID,
            Keeper {
                scheme: bibli_core::location::PATH_SCHEME.into(),
                save: shown(),
                save_title: "Bookmark this folder".into(),
                forget_title: "Remove from bookmarks".into(),
                keeps: None,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bibli_core::app::AppConfig;

    /// The kernel takes a mount out of the list as its unmount begins and
    /// only then writes what it had to: the volume, listed by its device in
    /// the meantime, still says it is being unmounted.
    #[test]
    fn a_volume_being_unmounted_stays_busy_once_its_mount_is_gone() {
        let busy = [Busy {
            location: Location::from_path(Path::new("/run/media/example/STICK")),
            device: Some(device_location("/dev/sdz1")),
            what: "Unmounting…",
        }];
        let volume = bibli_platform::places::Volume {
            device: "/dev/sdz1".into(),
            label: "STICK".into(),
            size: 16 << 20,
            locked: false,
            hotplug: true,
            container: None,
            system: false,
        };
        let mut places = vec![volume_place(&volume)];
        mark_busy(&mut places, &busy, &[]);
        assert_eq!(places[0].busy.as_deref(), Some("Unmounting…"));
        let mut other = vec![volume_place(&bibli_platform::places::Volume {
            device: "/dev/sdy1".into(),
            ..volume
        })];
        mark_busy(&mut other, &busy, &[]);
        assert_eq!(other[0].busy, None);
    }

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
        let place = |title: &str| Place::new("places", title, "", Location::from_path("/"));
        places.0.lock().lifetime = Some(CancelToken::new());
        places.0.lock().generation = 2;
        places.publish(&app, 2, vec![place("new")], Some(Vec::new()));
        // A scan started before the latest one finishes last: ignored.
        places.publish(&app, 1, vec![place("old")], Some(Vec::new()));
        let titles: Vec<String> = app
            .registry
            .places
            .all()
            .into_iter()
            .map(|p| p.title)
            .collect();
        assert_eq!(titles, ["new"]);
        places.0.lock().lifetime.take().unwrap().cancel();
        places.publish(&app, 2, vec![place("late")], Some(Vec::new()));
        assert_eq!(app.registry.places.all()[0].title, "new");
    }

    #[test]
    fn a_rescan_keeps_what_was_measured_until_it_is_measured_again() {
        let place =
            |group: &str, path: &str| Place::new(group, path, "", Location::from_path(path));
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
    /// A locked volume says it is being unlocked from the moment the
    /// passphrase is given until the attempt ends, and the sidebar is right
    /// at once when it does: a refused passphrase leaves it locked, an
    /// accepted one takes it away, without waiting for `lsblk` to run
    /// again. Between the unlock and the mount, a rescan (udev announces
    /// the new device) lists the container as its cleartext volume, which
    /// is still being attached. `lsblk` and `udisksctl` are fakes first on
    /// PATH, never a real drive; udisks holds the unlock and the mount
    /// until the test lets them answer. Asked for a tab (a middle click),
    /// the volume opens in one behind the tab shown, the pane left where
    /// it was.
    #[cfg(unix)]
    #[test]
    #[allow(unsafe_code)]
    fn a_volume_says_it_is_unlocking_and_is_right_as_soon_as_it_ends() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("bin");
        let mnt = tmp.path().join("mnt");
        std::fs::create_dir(&bin).unwrap();
        std::fs::create_dir(&mnt).unwrap();
        let (gate, refuse, unlocked, mount_gate, mounted) = (
            tmp.path().join("gate"),
            tmp.path().join("refuse"),
            tmp.path().join("unlocked"),
            tmp.path().join("mount gate"),
            tmp.path().join("mounted"),
        );
        let script = |name: &str, body: String| {
            let path = bin.join(name);
            std::fs::write(&path, format!("#!/bin/sh\n{body}")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        };
        let container = r#"{"path":"/dev/sdz","label":null,"size":2000000000000,"fstype":"crypto_LUKS","mountpoint":null,"hotplug":true,"pkname":null}"#;
        let cleartext = |mountpoint: &str| {
            format!(
                r#"{{"path":"/dev/dm-9","label":"Backup","size":1999000000000,"fstype":"ext4","mountpoint":{mountpoint},"hotplug":false,"pkname":"sdz"}}"#
            )
        };
        let (unmounted, mounted_at) = (
            cleartext("null"),
            cleartext(&format!("\"{}\"", mnt.display())),
        );
        script(
            "lsblk",
            format!(
                "if [ -e '{mounted}' ]; then echo '{{\"blockdevices\": [{container},{mounted_at}]}}'\n\
                 elif [ -e '{unlocked}' ]; then echo '{{\"blockdevices\": [{container},{unmounted}]}}'\n\
                 else echo '{{\"blockdevices\": [{container}]}}'; fi\n",
                mounted = mounted.display(),
                unlocked = unlocked.display(),
            ),
        );
        script(
            "udisksctl",
            format!(
                "case \"$1\" in\n\
                 unlock) cat > /dev/null\n\
                   while [ ! -e '{gate}' ]; do sleep 0.02; done\n\
                   if [ -e '{refuse}' ]; then echo 'No key available with this passphrase' >&2; exit 1; fi\n\
                   touch '{unlocked}'; echo 'Unlocked /dev/sdz as /dev/dm-9.';;\n\
                 mount) while [ ! -e '{mount_gate}' ]; do sleep 0.02; done\n\
                   touch '{mounted}'; echo 'Mounted /dev/dm-9 at {mnt}';;\n\
                 *) exit 1;;\n\
                 esac\n",
                gate = gate.display(),
                refuse = refuse.display(),
                unlocked = unlocked.display(),
                mount_gate = mount_gate.display(),
                mounted = mounted.display(),
                mnt = mnt.display(),
            ),
        );
        let path = std::env::var_os("PATH").unwrap_or_default();
        let mut paths = vec![bin.clone()];
        paths.extend(std::env::split_paths(&path));
        // SAFETY: one test per process under nextest, set before the app
        // starts any thread.
        unsafe { std::env::set_var("PATH", std::env::join_paths(paths).unwrap()) };
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(PlacesPlugin::default()), &app);
        let place = |location: &str| {
            app.registry
                .places
                .all()
                .into_iter()
                .find(|p| p.location.to_string() == location)
        };
        let volume = || place("device:/dev/sdz");
        let opened = || place("device:/dev/dm-9");
        let wait = |done: &dyn Fn() -> bool| {
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            while !done() {
                assert!(std::time::Instant::now() < deadline, "timed out");
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        let unlock = |tab: bool| {
            app.dispatch(&Invocation::with_args(
                "places.unlock",
                &serde_json::json!({
                    "device": "/dev/sdz",
                    "passphrase": "example words",
                    "tab": tab,
                }),
            ))
            .unwrap();
        };
        let pane = app.workspace.read().active_id();
        let before = app.workspace.read().pane(pane).unwrap().shown().cloned();
        wait(&|| volume().is_some());
        assert_eq!(volume().unwrap().busy, None);

        // Refused.
        std::fs::write(&refuse, b"").unwrap();
        unlock(false);
        // At once, before the worker has asked udisks anything.
        assert_eq!(volume().unwrap().busy.as_deref(), Some("Unlocking…"));
        // A rescan meanwhile (a disk plugged) keeps saying so.
        app.dispatch(&Invocation::new("places.refresh")).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(volume().unwrap().busy.as_deref(), Some("Unlocking…"));
        std::fs::write(&gate, b"").unwrap();
        let notice = next_notice(&app);
        assert!(
            notice.text.contains("passphrase was not accepted"),
            "{}",
            notice.text
        );
        assert_eq!(volume().expect("still locked").busy, None, "at once");

        // Accepted. Unlocked and not yet mounted, a rescan lists the
        // container as its cleartext volume: still being attached.
        std::fs::remove_file(&refuse).unwrap();
        unlock(true);
        wait(&|| unlocked.exists());
        app.dispatch(&Invocation::new("places.refresh")).unwrap();
        wait(&|| opened().is_some());
        assert!(volume().is_none(), "listed as its cleartext volume");
        assert_eq!(
            opened().unwrap().busy.as_deref(),
            Some("Unlocking…"),
            "not offered for mounting while it is being mounted"
        );
        // Mounted: neither is one to mount, at once and after `lsblk`
        // has run again.
        std::fs::write(&mount_gate, b"").unwrap();
        let notice = next_notice(&app);
        assert!(notice.text.starts_with("Mounted"), "{}", notice.text);
        {
            let ws = app.workspace.read();
            assert_eq!(ws.active_id(), pane, "the tab shown stays shown");
            assert_eq!(ws.pane(pane).unwrap().shown().cloned(), before);
            let behind = ws.tabs().iter().find(|t| !t.panes.contains(&pane));
            let shown = behind.and_then(|t| ws.pane(t.panes[0])?.shown().cloned());
            assert_eq!(shown, Some(Location::from_path(&mnt)), "in a tab behind");
        }
        assert!(
            volume().is_none() && opened().is_none(),
            "not offered again while lsblk runs"
        );
        std::thread::sleep(Duration::from_millis(300));
        assert!(volume().is_none() && opened().is_none());
    }

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
        let added = run("keep", serde_json::json!({}));
        assert_eq!(added.text, "Bookmarked “it's 100% a folder”");
        let again = run("places.bookmark", serde_json::json!({ "path": path(&a) }));
        assert_eq!(again.text, "“it's 100% a folder” is already bookmarked");
        assert!(again.action.is_none());
        // A folder of the trash is not bookmarked: restored or deleted,
        // the bookmark would lead nowhere.
        let trashed = tmp.path().join(".Trash-1000/files/old project");
        std::fs::create_dir_all(&trashed).unwrap();
        let refused = app.dispatch(&Invocation::with_args(
            "places.bookmark",
            &serde_json::json!({ "path": path(&trashed) }),
        ));
        assert!(format!("{:#}", refused.unwrap_err()).contains("is in the trash"));
        run(
            "places.bookmark",
            serde_json::json!({ "path": path(&b), "title": "Bee" }),
        );
        assert_eq!(titles(&app), ["it's 100% a folder", "Bee"]);
        // The sidebar removes and renames it through `forget` and
        // `rename`, which its own menu does not repeat.
        let first = &bookmarks(&app)[0];
        assert_eq!(
            first.forget.as_ref().map(|f| f.name.to_string()).as_deref(),
            Some("places.unbookmark")
        );
        assert!(first.rename.is_some() && first.menu.is_empty());

        // Dragged before the first, as the sidebar does: the dragged
        // place's move command, with the other's `before`.
        let (first, second) = (bookmarks(&app)[0].clone(), bookmarks(&app)[1].clone());
        let mut drag = second.reorder.unwrap().command;
        drag.args["before"] = first.reorder.unwrap().before;
        let moved = run(&drag.name, drag.args);
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

        // The pane's button, filled while the pane shows a bookmark.
        let forget = bookmarks(&app)[0].forget.clone().unwrap();
        assert_eq!(forget.args["path"], path(&b));
        let removed = run(&forget.name, forget.args);
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

    #[cfg(unix)]
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

    /// Every rescan reads the bookmarks file. notify reports a file opened
    /// too: a watcher counting it rescanned for as long as the file
    /// existed, waking the window at every pass. Once the first scans have
    /// published, the plugin is quiet; another program's edit still counts.
    // GTK's bookmarks (step F, ADR-035).
    #[cfg(target_os = "linux")]
    #[test]
    #[allow(unsafe_code)]
    fn reading_the_bookmarks_rescans_nothing() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        // SAFETY: one test per process under nextest, set before the app
        // starts any thread; the bookmarks file is then this test's own.
        unsafe { std::env::set_var("XDG_CONFIG_HOME", &config) };
        // Names a `file://` URI holds as they are.
        let (a, b) = (tmp.path().join("alpha"), tmp.path().join("beta"));
        let uri = |path: &Path| format!("file://{}", path.display());
        std::fs::create_dir(&a).unwrap();
        std::fs::create_dir(&b).unwrap();
        let file = config.join("gtk-3.0/bookmarks");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, format!("{}\n", uri(&a))).unwrap();

        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        let wakes = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&wakes);
        app.set_waker(Some(Arc::new(move || {
            counted.fetch_add(1, Ordering::SeqCst);
        })));
        app.plugins.add(Arc::new(PlacesPlugin::default()), &app);
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while titles(&app) != ["alpha"] {
            assert!(std::time::Instant::now() < deadline, "{:?}", titles(&app));
            std::thread::sleep(Duration::from_millis(10));
        }
        // Quiet: no wake for three settling times, after the first scans
        // (the quick one and the worker's measured one) end.
        let quiet = SETTLE * 3;
        let mut last = (wakes.load(Ordering::SeqCst), std::time::Instant::now());
        while last.1.elapsed() < quiet {
            assert!(std::time::Instant::now() < deadline, "never quiet");
            std::thread::sleep(Duration::from_millis(20));
            let now = wakes.load(Ordering::SeqCst);
            if now != last.0 {
                last = (now, std::time::Instant::now());
            }
        }
        std::fs::read_to_string(&file).unwrap();
        std::thread::sleep(quiet);
        assert_eq!(wakes.load(Ordering::SeqCst), last.0, "a read rescanned");

        // Another file manager adds a bookmark: it appears.
        std::fs::write(&file, format!("{}\n{}\n", uri(&a), uri(&b))).unwrap();
        while titles(&app) != ["alpha", "beta"] {
            assert!(std::time::Instant::now() < deadline, "{:?}", titles(&app));
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
