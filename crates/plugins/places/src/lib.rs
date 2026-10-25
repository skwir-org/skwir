//! skwir plugin: places. Fills the sidebar from what the desktop already
//! knows: home, XDG user dirs, GTK bookmarks, mounted volumes.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use skwir_core::app::{App, NotifyLevel};
use skwir_core::cancel::CancelToken;
use skwir_core::command::{Invocation, NoArgs, Outcome};
use skwir_core::location::Location;
use skwir_core::os_path::PathArg;
use skwir_core::plugin::Plugin;
use skwir_core::registry::{Keeper, MenuItem, Place, Reorder};
use skwir_core::scheduler::Priority;
use skwir_core::workspace::PaneId;
use skwir_platform::places::{BookmarkEdit, Bookmarked, Marked, address_name};

mod passphrases;

pub use passphrases::Keyring;

const ID: &str = "places";
/// A disk brings several partitions: events this close together are one change.
const SETTLE: Duration = Duration::from_millis(400);
/// How long an unmount waits for what skwir let go of to close: its
/// listings close their directories on a worker, and a preview's reader or
/// player, and the terminal's shell after its `cd`, are processes of their
/// own. Past this, what still uses the volume is not ours.
const RELEASE: Duration = Duration::from_secs(3);

/// The places plugin.
pub struct PlacesPlugin {
    places: Places,
    /// Watches for volumes being plugged, unplugged, mounted or unmounted,
    /// and for the bookmarks changing, whoever changes them.
    watcher: Watcher,
    /// Where the passphrases the user asked to be remembered are (ADR-088).
    keyring: Arc<dyn Keyring>,
}

impl Default for PlacesPlugin {
    fn default() -> Self {
        Self::with_keyring(Arc::new(passphrases::Desktop))
    }
}

impl PlacesPlugin {
    /// The plugin with `keyring` for volumes' passphrases: a test's.
    pub fn with_keyring(keyring: Arc<dyn Keyring>) -> Self {
        PlacesPlugin {
            places: Places::default(),
            watcher: Watcher::default(),
            keyring,
        }
    }
}

/// The plugin's watcher, shared with the bookmark commands: the first
/// bookmark can create the folder that is then to be watched.
#[derive(Clone, Default)]
struct Watcher(Arc<Mutex<Option<skwir_core::fs::Settled>>>);

impl Watcher {
    /// Watch the bookmarks' folders that exist; watching one again is
    /// harmless.
    fn watch_bookmarks(&self) {
        if let Some(watcher) = self.0.lock().as_mut() {
            for folder in skwir_platform::places::bookmarks_folders() {
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

/// Forget Passphrase on the place of each volume whose passphrase Skwir
/// keeps (`remembered`, by UUID), locked or mounted, and on no other
/// (ADR-088).
fn offer_forget(places: &mut [Place], remembered: &[String]) {
    const FORGET: &str = "places.forget_passphrase";
    for place in places {
        place
            .menu
            .retain(|item| item.command.name.as_ref() != FORGET);
        let Some(uuid) = place.key.as_deref().and_then(|k| k.strip_prefix("volume:")) else {
            continue;
        };
        // `volume:<uuid>/@home`: a subvolume of it.
        let uuid = uuid.split('/').next().unwrap_or(uuid);
        if remembered.iter().any(|r| r == uuid) {
            place.menu.push(MenuItem::new(
                "place",
                "Forget Passphrase",
                "",
                530,
                Invocation::new(FORGET).with("volume", uuid),
            ));
        }
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
    // What only the worker lists (`lsblk`'s), not what both do (a drive
    // BitLocker locked, from the quick scan, which also activates).
    let listed: Vec<(String, Location)> = out
        .iter()
        .map(|p| (p.group.clone(), p.location.clone()))
        .collect();
    out.extend(
        measured
            .iter()
            .filter(|m| {
                m.activate.is_some()
                    && !listed
                        .iter()
                        .any(|(group, location)| *group == m.group && *location == m.location)
            })
            .cloned(),
    );
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
fn volume_key(identity: &skwir_platform::places::Identity, root: &Path) -> Option<String> {
    let uuid = identity.uuid.as_deref()?;
    Some(if root == Path::new("/") {
        format!("volume:{uuid}")
    } else {
        format!("volume:{uuid}{}", root.display())
    })
}

/// A present but unmounted volume as a sidebar entry: activating it mounts
/// it, unlocking it first when it is locked (`places.open_locked`). One
/// udisks says not to show (an EFI partition) is offered hidden, as is a
/// small FAT or NTFS partition where udev says nothing.
fn volume_place(volume: &skwir_platform::places::Volume) -> Place {
    use skwir_core::command::Invocation;
    let identity = match &volume.uuid {
        Some(uuid) => skwir_platform::places::Identity {
            uuid: Some(uuid.clone()),
            ignored: None,
        },
        None => skwir_platform::places::identity(&volume.device),
    };
    let title = match (volume.label.is_empty(), volume.locked) {
        (false, _) => volume.label.clone(),
        (true, true) => format!("{} encrypted", size_label(volume.size)),
        (true, false) => format!("{} volume", size_label(volume.size)),
    };
    let activate = if volume.locked {
        Invocation::with_args(
            "places.open_locked",
            &serde_json::json!({
                "device": volume.device,
                "title": title,
                "uuid": identity.uuid,
            }),
        )
    } else {
        Invocation::with_args(
            "places.mount",
            &serde_json::json!({ "device": volume.device }),
        )
    };
    let unavailable = if volume.locked {
        "Locked. Click to unlock and open it."
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

/// Asking for a bookmark's new name: its menu's Rename…, and F2. `which`
/// names it as the command takes it (`path` or `address`).
fn bookmark_rename(title: &str, which: &serde_json::Value, own: &str) -> Invocation {
    Invocation::with_args(
        "ui.ask",
        &serde_json::json!({
            "title": "Rename bookmark",
            "accept": "Rename",
            "placeholder": format!("Name (empty: {own})"),
            "text": title,
            "command": "places.rename_bookmark",
            "args": which,
            "field": "title",
        }),
    )
}

/// What moves a bookmark: one command for folders and addresses, so they
/// move among each other (`sidebar::mover`).
fn bookmark_reorder(mark: &Marked) -> Reorder {
    let which = match mark {
        Marked::Folder(path) => serde_json::json!({ "path": PathArg::from(path.as_path()) }),
        Marked::Address(address) => serde_json::json!({ "address": address }),
    };
    Reorder {
        command: Invocation::with_args("places.move_bookmark", &which),
        before: mark_json(mark),
    }
}

/// Scan the environment and return every place, in sidebar order.
pub fn scan() -> Vec<Place> {
    let folder = |title: String, path: PathBuf, icon: &str, group: &str| Place {
        menu: folder_menu(&path),
        ..Place::new(group, title, icon, Location::from_path(path))
    };
    let mut out = Vec::new();
    if let Some(home) = skwir_platform::places::home() {
        out.push(folder("Home".into(), home, "user-home", "places"));
    }
    for d in skwir_platform::places::user_dirs() {
        out.push(folder(d.name, d.path, icon_for_user_dir(&d.kind), "places"));
    }
    // In the file's order, a remote folder's beside a folder's.
    for b in skwir_platform::places::every_bookmark() {
        match b {
            Bookmarked::Folder(b) => {
                let path = serde_json::json!(PathArg::from(b.path.as_path()));
                // Rename… and the item that removes it are the sidebar's,
                // from `rename` and `forget`.
                out.push(Place {
                    menu: Vec::new(),
                    rename: Some(bookmark_rename(
                        &b.title,
                        &serde_json::json!({ "path": path }),
                        "the folder's own",
                    )),
                    forget: Some(Invocation::with_args(
                        "places.unbookmark",
                        &serde_json::json!({ "path": path }),
                    )),
                    reorder: Some(bookmark_reorder(&Marked::Folder(b.path.clone()))),
                    ..folder(b.title, b.path, "folder", "bookmarks")
                });
            }
            // A remote folder another file manager bookmarked opens by its
            // address (ADR-077), when a plugin opens its scheme.
            Bookmarked::Address(b) => {
                let which = serde_json::json!({ "address": b.address });
                out.push(Place {
                    rename: Some(bookmark_rename(&b.title, &which, "its address's last part")),
                    forget: Some(Invocation::with_args("places.unbookmark_address", &which)),
                    reorder: Some(bookmark_reorder(&Marked::Address(b.address.clone()))),
                    ..Place::new(
                        "bookmarks",
                        b.title,
                        "folder-remote",
                        Location::parse(&b.address),
                    )
                });
            }
        }
    }
    for m in skwir_platform::places::mounts() {
        // A network's mount is the Network section's (the remote plugin).
        if skwir_core::fs::reach::network(&m.fstype, std::ffi::OsStr::new(&m.source)) {
            continue;
        }
        // A drive BitLocker keeps locked (Windows): opening it unlocks it
        // through Windows' own dialog (ADR-088).
        if m.locked {
            out.push(Place {
                activate: Some(Invocation::with_args(
                    "places.open_locked",
                    &serde_json::json!({ "device": m.source, "title": m.label }),
                )),
                unavailable: Some("Locked. Click to unlock and open it.".into()),
                ..Place::new(
                    "devices",
                    m.label,
                    "drive-harddisk-encrypted",
                    device_location(&m.source),
                )
            });
            continue;
        }
        let icon = if m.removable {
            "drive-removable-media"
        } else {
            "drive-harddisk"
        };
        // What udisks says of it: an EFI partition mounted at /boot is
        // offered hidden. Read from memory (sysfs, udev's database).
        let identity = skwir_platform::places::identity(&m.source);
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
        let mut places = places;
        // Read before the lock: the settings have their own.
        offer_forget(&mut places, &passphrases::remembered(app));
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
                                .and_then(|path| skwir_platform::places::space(&path));
                        }
                        // A bookmark outlives its folder: it may have moved,
                        // or live on a volume that is not mounted. A remote's
                        // is not asked: its server may not answer (ADR-077).
                        if place.group == "bookmarks"
                            && place.location.path().is_some_and(|path| {
                                skwir_core::fs::reach::of(&path).is_none() && !path.is_dir()
                            })
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
                let volumes = skwir_platform::places::unmounted_volumes();
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
    pane: Option<skwir_core::workspace::PaneId>,
    /// Open the volume in a tab behind the one shown, not in the pane.
    #[serde(default)]
    tab: bool,
}

// No `Debug`: the passphrase must not reach a log through a formatter.
#[derive(serde::Deserialize, schemars::JsonSchema)]
struct UnlockArgs {
    device: String,
    /// Secret. Handed to the system on standard input; never kept or logged.
    passphrase: String,
    #[serde(default)]
    pane: Option<skwir_core::workspace::PaneId>,
    /// Open the volume in a tab behind the one shown, not in the pane.
    #[serde(default)]
    tab: bool,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct OpenLockedArgs {
    /// The locked volume's device, as listed in the sidebar (`/dev/sdb2`).
    device: String,
    /// What the question for its passphrase calls it.
    #[serde(default)]
    title: Option<String>,
    /// What its passphrase is kept by in the keyring: its container's UUID.
    #[serde(default)]
    uuid: Option<String>,
    #[serde(default)]
    pane: Option<skwir_core::workspace::PaneId>,
    /// Open the volume in a tab behind the one shown, not in the pane.
    #[serde(default)]
    tab: bool,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ForgetArgs {
    /// The volume, by its container's UUID.
    volume: String,
}

/// What a volume is unlocked with before it is mounted.
enum Unlock {
    /// Nothing: it is not locked.
    Not,
    /// The passphrase given (`places.unlock`).
    With(String),
    /// The one the keyring keeps, else the user's answer, or the system's
    /// own dialog where it asks itself (`places.open_locked`, ADR-088).
    Asking {
        title: String,
        uuid: Option<String>,
        keyring: Arc<dyn Keyring>,
    },
}

/// The volume at `device`, unlocked as `unlock` says and mounted: where
/// it is mounted, `None` when the user cancelled. Blocks: on a worker.
fn unlocked_and_mounted(
    app: &App,
    device: &str,
    unlock: Unlock,
    cancel: &CancelToken,
) -> anyhow::Result<Option<PathBuf>> {
    use skwir_platform::places;
    let cleartext = match unlock {
        Unlock::Not => device.to_owned(),
        Unlock::With(passphrase) => places::unlock(device, &passphrase)?,
        Unlock::Asking { .. } if places::SYSTEM_ASKS => {
            return places::unlock_by_system(device, cancel);
        }
        Unlock::Asking {
            title,
            uuid,
            keyring,
        } => {
            let volume = passphrases::Locked {
                device,
                title: &title,
                uuid: uuid.as_deref(),
            };
            match passphrases::unlock_kept_or_asked(app, &keyring, &volume, cancel, places::unlock)?
            {
                Some(cleartext) => cleartext,
                None => return Ok(None),
            }
        }
    };
    places::mount(&cleartext).map(Some)
}

/// Unlock as `unlock` says, mount, republish the places and open the
/// volume in the pane that asked, or with `tab` in a tab behind the one
/// shown. Everything blocks, so it runs on a worker; the pane is resolved
/// now, not when the work ends.
fn attach(
    places: &Places,
    app: &App,
    device: String,
    unlock: Unlock,
    pane: Option<skwir_core::workspace::PaneId>,
    tab: bool,
) {
    let pane = app.pane_or_active(pane);
    let this = places.clone();
    // The place `volume_place` published for it.
    let location = Location::parse(&format!("device:{device}"));
    places.start_busy(
        app,
        location.clone(),
        if matches!(unlock, Unlock::Not) {
            "Mounting…"
        } else {
            "Unlocking…"
        },
    );
    let Some(cancel) = places.lifetime() else {
        return;
    };
    places.spawn(app, "places-attach", move |app| {
        use skwir_core::app::NotifyLevel;
        let mounted = unlocked_and_mounted(app, &device, unlock, &cancel);
        // Whatever happened, the set of volumes may have changed (a
        // container can unlock and its filesystem still fail to mount).
        this.end_busy(&location, matches!(mounted, Ok(Some(_))));
        this.populate(app);
        match mounted {
            Ok(None) => {}
            Ok(Some(path)) => {
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
        let settings = skwir_platform::places::bookmarks_folders();
        let relevant = move |path: &std::path::Path| {
            path.file_name().is_some_and(|name| name == "bookmarks")
                || !settings.iter().any(|folder| path.parent() == Some(folder))
        };
        let (places, weak) = (self.places.clone(), app.weak());
        let Some(mut watcher) =
            skwir_core::fs::Settled::start("skwir-places", SETTLE, relevant, move || {
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
    before: Option<MarkArg>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct AddressArgs {
    /// The address as the bookmarks file writes it
    /// (`sftp://me@example.com/home/me`).
    address: String,
    /// Its label; its last part when omitted.
    #[serde(default)]
    title: Option<String>,
    /// The bookmark it goes before; last when omitted.
    #[serde(default)]
    before: Option<MarkArg>,
    #[serde(default)]
    undo: bool,
}

/// A bookmark named in a command's arguments: a folder's path, or
/// `{"address": "sftp://…"}` for a remote folder's.
#[derive(Clone, Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
enum MarkArg {
    Address { address: String },
    Folder(PathArg),
}

impl MarkArg {
    fn into_marked(self) -> Marked {
        match self {
            Self::Address { address } => Marked::Address(address),
            Self::Folder(path) => Marked::Folder(path.into_inner()),
        }
    }
}

/// `mark` as a command's argument names it ([`MarkArg`]).
fn mark_json(mark: &Marked) -> serde_json::Value {
    match mark {
        Marked::Folder(path) => serde_json::json!(PathArg::from(path.as_path())),
        Marked::Address(address) => serde_json::json!({ "address": address }),
    }
}

/// The bookmark a rename or a move is about: `path`'s, else `address`'s.
fn named(path: Option<PathArg>, address: Option<String>) -> anyhow::Result<Marked> {
    match (path, address) {
        (Some(path), _) => Ok(Marked::Folder(path.into_inner())),
        (None, Some(address)) => Ok(Marked::Address(address)),
        (None, None) => anyhow::bail!("which bookmark? pass its path or its address"),
    }
}

/// `address` as the bookmarks file writes it: a URI, escaped.
fn address_uri(address: &Location) -> String {
    skwir_platform::uri::address_uri(
        &address.scheme,
        &skwir_core::location::unescape(&address.rest),
    )
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct RenameArgs {
    /// The bookmarked folder.
    #[serde(default)]
    path: Option<PathArg>,
    /// The bookmarked address, for a remote folder's bookmark.
    #[serde(default)]
    address: Option<String>,
    /// Its new label; empty for the folder's own name.
    title: String,
    #[serde(default)]
    undo: bool,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct MoveArgs {
    /// The bookmarked folder.
    #[serde(default)]
    path: Option<PathArg>,
    /// The bookmarked address, for a remote folder's bookmark.
    #[serde(default)]
    address: Option<String>,
    /// The bookmark it goes before (a folder's path, or
    /// `{"address": …}`); last when omitted.
    #[serde(default)]
    before: Option<MarkArg>,
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
    let before = |before: &Option<Marked>| before.as_ref().map(mark_json);
    // A rename's or a move's bookmark, as its command names it.
    let which = |mark: &Marked| match mark {
        Marked::Folder(path) => serde_json::json!({ "path": arg(path) }),
        Marked::Address(address) => serde_json::json!({ "address": address }),
    };
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
        BookmarkEdit::Rename { mark, title } => {
            let mut args = which(mark);
            args["title"] = serde_json::json!(title.clone().unwrap_or_default());
            ("places.rename_bookmark", args)
        }
        BookmarkEdit::Move { mark, before: b } => {
            let mut args = which(mark);
            args["before"] = serde_json::json!(before(b));
            ("places.move_bookmark", args)
        }
        BookmarkEdit::AddAddress {
            address,
            title,
            before: b,
        } => (
            "places.bookmark_address",
            serde_json::json!({ "address": address, "title": title, "before": before(b) }),
        ),
        BookmarkEdit::RemoveAddress { address } => (
            "places.unbookmark_address",
            serde_json::json!({ "address": address }),
        ),
    };
    let mut args = args;
    args["undo"] = serde_json::json!(undo);
    Invocation::with_args(name, &args)
}

/// A name as the bookmarks file keeps it: one line, none when empty.
fn one_line(title: Option<&str>) -> Option<String> {
    let title = title?.split(['\n', '\r']).collect::<Vec<_>>().join(" ");
    Some(title.trim().to_owned()).filter(|t| !t.is_empty())
}

/// What an edit did, in a sentence, or what it found when it did nothing.
/// `shown` is the bookmark's label before the edit, when it had one.
fn describe(edit: &BookmarkEdit, shown: Option<&str>, done: bool) -> Option<String> {
    let own = || {
        edit.path().map_or_else(
            || edit.address().map(address_name).unwrap_or_default(),
            name_of,
        )
    };
    let before = || shown.map_or_else(own, str::to_owned);
    Some(match (edit, done) {
        (BookmarkEdit::Add { title, .. } | BookmarkEdit::AddAddress { title, .. }, true) => {
            format!(
                "Bookmarked “{}”",
                one_line(title.as_deref()).unwrap_or_else(own)
            )
        }
        (BookmarkEdit::Add { .. } | BookmarkEdit::AddAddress { .. }, false) => {
            format!("“{}” is already bookmarked", before())
        }
        (BookmarkEdit::Remove { .. } | BookmarkEdit::RemoveAddress { .. }, true) => {
            format!("Removed “{}” from bookmarks", before())
        }
        (BookmarkEdit::Remove { .. } | BookmarkEdit::RemoveAddress { .. }, false) => {
            format!("“{}” is not bookmarked", own())
        }
        (BookmarkEdit::Rename { title, .. }, true) => format!(
            "Renamed bookmark “{}” to “{}”",
            before(),
            one_line(title.as_deref()).unwrap_or_else(own)
        ),
        (BookmarkEdit::Move { .. }, true) => format!("Moved bookmark “{}”", before()),
        // Renamed to its own name, dropped where it was: nothing to say.
        (BookmarkEdit::Rename { .. } | BookmarkEdit::Move { .. }, false) => return None,
    })
}

impl Places {
    /// Apply `edit` to the bookmarks, republish, and say what happened
    /// with the way back: Undo, or Redo when this was an undo. Asked on
    /// the window's thread, it runs on a worker and says its failure;
    /// anywhere else (an agent's call, a script's, another plugin's
    /// worker) it runs where it is, and its failure is the caller's, who
    /// then knows the bookmark was not kept.
    fn edit(
        &self,
        app: &App,
        edit: BookmarkEdit,
        undo: bool,
        watcher: Watcher,
    ) -> anyhow::Result<()> {
        if !skwir_core::scheduler::on_window_thread() {
            return self.apply(app, &edit, undo, &watcher);
        }
        let this = self.clone();
        self.spawn(app, "places-bookmarks", move |app| {
            if let Err(error) = this.apply(app, &edit, undo, &watcher) {
                app.notify(
                    NotifyLevel::Error,
                    format!("The bookmarks were not changed: {error:#}"),
                );
            }
        });
        Ok(())
    }

    fn apply(
        &self,
        app: &App,
        edit: &BookmarkEdit,
        undo: bool,
        watcher: &Watcher,
    ) -> anyhow::Result<()> {
        // A new bookmark is a folder; one coming back (an undone removal)
        // may be missing, and was a bookmark all the same.
        if let BookmarkEdit::Add { path, .. } = edit
            && !undo
            && !path.is_dir()
        {
            anyhow::bail!(
                "“{}” is not a folder: only folders can be bookmarked",
                name_of(path)
            );
        }
        let shown = match edit.address() {
            Some(address) => skwir_platform::places::remote_bookmarks()
                .into_iter()
                .find(|b| b.address == address)
                .map(|b| b.title),
            None => skwir_platform::places::bookmarks()
                .into_iter()
                .find(|b| Some(b.path.as_path()) == edit.path())
                .map(|b| b.title),
        };
        if let Some(inverse) = skwir_platform::places::edit_bookmarks(edit)? {
            watcher.watch_bookmarks();
            self.populate(app);
            if let Some(text) = describe(edit, shown.as_deref(), true) {
                app.notify_undoable(text, undo, invocation(&inverse, !undo));
            }
        } else if let Some(text) = describe(edit, shown.as_deref(), false) {
            app.notify(NotifyLevel::Info, text);
        }
        Ok(())
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

    fn category(&self) -> skwir_core::plugin::Category {
        skwir_core::plugin::Category::Browsing
    }

    fn requirements(&self) -> Vec<skwir_core::plugin::Requirement> {
        vec![skwir_core::plugin::Requirement::program(
            "udisksctl",
            "Unmounting removable volumes from the sidebar",
        )]
    }

    fn on_settings_changed(&self, app: &App, keys: &[String]) {
        // Another Skwir remembered or forgot a passphrase.
        if keys.iter().any(|k| k == passphrases::REMEMBERED) {
            self.places.populate(app);
        }
    }

    fn on_disable(&self, _app: &App) {
        *self.watcher.0.lock() = None;
        if let Some(lifetime) = self.places.0.lock().lifetime.take() {
            lifetime.cancel();
        }
    }

    fn capabilities(&self) -> skwir_core::permissions::Capabilities {
        use skwir_core::permissions::Capability;
        skwir_core::permissions::Capabilities::of(&[Capability::Programs])
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
        app.settings
            .declare(skwir_core::settings::SettingSpec::data(
                passphrases::REMEMBERED,
                "The encrypted volumes whose passphrase is kept in the keyring, by their UUID",
                toml::Value::Array(Vec::new()),
            ));
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
                // A remote folder is kept by its address, which connects
                // again once its mount is gone (ADR-077).
                if let Some(address) = app.address_of(&Location::from_path(&path)) {
                    let edit = BookmarkEdit::AddAddress {
                        address: address_uri(&address),
                        title: args.title,
                        before: args.before.map(MarkArg::into_marked),
                    };
                    places.edit(app, edit, undo, watcher.clone())?;
                    return Ok(Outcome::Done);
                }
                // Restored or deleted, a folder of the trash leaves the
                // bookmark leading nowhere.
                if skwir_core::op::in_trash(&path).is_some() {
                    anyhow::bail!(
                        "“{}” is in the trash: restore it first",
                        path.file_name().unwrap_or(path.as_os_str()).display()
                    );
                }
                let edit = BookmarkEdit::Add {
                    path,
                    title: args.title,
                    before: args.before.map(MarkArg::into_marked),
                };
                places.edit(app, edit, undo, watcher.clone())?;
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
                places.edit(app, BookmarkEdit::Remove { path }, undo, watcher.clone())?;
                Ok(Outcome::Done)
            },
        );
        let (places, watcher) = (self.places.clone(), self.watcher.clone());
        app.commands.register::<AddressArgs, _>(
            ID,
            "places.bookmark_address",
            "Bookmark an address",
            "Add a remote folder's address (sftp://, smb://…) to the desktop's shared bookmarks",
            move |args, app| {
                let edit = BookmarkEdit::AddAddress {
                    address: args.address,
                    title: args.title,
                    before: args.before.map(MarkArg::into_marked),
                };
                places.edit(app, edit, args.undo, watcher.clone())?;
                Ok(Outcome::Done)
            },
        );
        let (places, watcher) = (self.places.clone(), self.watcher.clone());
        app.commands.register::<AddressArgs, _>(
            ID,
            "places.unbookmark_address",
            "Remove an address from bookmarks",
            "Remove a remote folder's address from the desktop's shared bookmarks",
            move |args, app| {
                let edit = BookmarkEdit::RemoveAddress {
                    address: args.address,
                };
                places.edit(app, edit, args.undo, watcher.clone())?;
                Ok(Outcome::Done)
            },
        );
        let (places, watcher) = (self.places.clone(), self.watcher.clone());
        app.commands.register::<RenameArgs, _>(
            ID,
            "places.rename_bookmark",
            "Rename bookmark",
            "Change the name a bookmark shows in the sidebar (a folder's, or a remote folder's by its address); the folder keeps its own",
            move |args, app| {
                let edit = BookmarkEdit::Rename {
                    mark: named(args.path, args.address)?,
                    title: Some(args.title),
                };
                places.edit(app, edit, args.undo, watcher.clone())?;
                Ok(Outcome::Done)
            },
        );
        let (places, watcher) = (self.places.clone(), self.watcher.clone());
        app.commands.register::<MoveArgs, _>(
            ID,
            "places.move_bookmark",
            "Move bookmark",
            "Put a bookmark (a folder's, or a remote folder's by its address) before another in the sidebar, or last",
            move |args, app| {
                let edit = BookmarkEdit::Move {
                    mark: named(args.path, args.address)?,
                    before: args.before.map(MarkArg::into_marked),
                };
                places.edit(app, edit, args.undo, watcher.clone())?;
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
                let mount = skwir_platform::places::mounts()
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
                // Nothing of skwir's may hold the volume: a pane showing it
                // goes home, the folders kept there close.
                let home = skwir_platform::places::home().unwrap_or_else(|| PathBuf::from("/"));
                app.release(&mount.path, &Location::from_path(home))?;
                let lifetime = places.lifetime();
                places.spawn(app, "places-unmount", move |app| {
                    use skwir_core::app::NotifyLevel;
                    // Written first, while still mounted: the row stays busy
                    // until the drive has it all, and a write that fails is
                    // reported instead of lost.
                    if let Err(error) = skwir_platform::places::flush(&mount.path) {
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
                        match skwir_platform::places::unmount(&mount.source) {
                            Err(error)
                                if skwir_platform::places::is_busy(&error)
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
                        // skwir let go of it: another program has it.
                        Err(error) if skwir_platform::places::is_busy(&error) => app.notify(
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
                attach(&places, app, args.device, Unlock::Not, args.pane, args.tab);
                Ok(Outcome::Done)
            },
        );
        let places = self.places.clone();
        app.commands.register::<UnlockArgs, _>(
            ID,
            "places.unlock",
            "Unlock volume",
            "Unlock an encrypted volume with the passphrase given, mount it and open it. The passphrase is a secret: it is handed to the system on standard input, never logged, and never kept",
            move |args, app| {
                attach(
                    &places,
                    app,
                    args.device,
                    Unlock::With(args.passphrase),
                    args.pane,
                    args.tab,
                );
                Ok(Outcome::Done)
            },
        );
        // The user's alone (ADR-088): the passphrase the keyring keeps
        // opens the volume only when the user opens it.
        let (places, keyring) = (self.places.clone(), Arc::clone(&self.keyring));
        app.commands
            .register::<OpenLockedArgs, _>(
                ID,
                "places.open_locked",
                "Unlock and open volume",
                "Unlock an encrypted volume with the passphrase the keyring keeps for it, else the one the user types (offering to remember it), or through the system's own dialog where it asks itself; then mount it and open it",
                move |args, app| {
                    let title = args.title.unwrap_or_else(|| "this volume".to_owned());
                    let unlock = Unlock::Asking {
                        title,
                        uuid: args.uuid,
                        keyring: Arc::clone(&keyring),
                    };
                    attach(&places, app, args.device, unlock, args.pane, args.tab);
                    Ok(Outcome::Done)
                },
            )
            .user_only();
        let (places, keyring) = (self.places.clone(), Arc::clone(&self.keyring));
        app.commands.register::<ForgetArgs, _>(
            ID,
            "places.forget_passphrase",
            "Forget passphrase",
            "Remove from the keyring the passphrase Skwir keeps for an encrypted volume (never one another program kept)",
            move |args, app| {
                anyhow::ensure!(
                    passphrases::remembered(app).contains(&args.volume),
                    "Skwir keeps no passphrase for this volume"
                );
                let keyring = Arc::clone(&keyring);
                let this = places.clone();
                // The keyring may wait on its own window: on a worker.
                places.spawn(app, "places-forget", move |app| {
                    match keyring.forget(&args.volume) {
                        Ok(()) => {
                            passphrases::note(app, &args.volume, false);
                            app.notify(NotifyLevel::Info, "The passphrase is forgotten");
                        }
                        Err(error) => app.notify(
                            NotifyLevel::Error,
                            format!("The passphrase was not forgotten: {error:#}"),
                        ),
                    }
                    this.populate(app);
                });
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
                scheme: skwir_core::location::PATH_SCHEME.into(),
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
    use skwir_core::app::AppConfig;

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
        let volume = skwir_platform::places::Volume {
            device: "/dev/sdz1".into(),
            label: "STICK".into(),
            size: 16 << 20,
            locked: false,
            hotplug: true,
            container: None,
            system: false,
            uuid: None,
        };
        let mut places = vec![volume_place(&volume)];
        mark_busy(&mut places, &busy, &[]);
        assert_eq!(places[0].busy.as_deref(), Some("Unmounting…"));
        let mut other = vec![volume_place(&skwir_platform::places::Volume {
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
        // A drive BitLocker locked: the quick scan lists it too.
        let mut locked = place("devices", "device:E:");
        locked.activate = Some(Invocation::new("places.open_locked"));
        let measured = [
            disk,
            gone,
            unmounted,
            place("bookmarks", "/example/renamed"),
            locked.clone(),
        ];
        let fresh = [
            place("devices", "/run/media/example/disk"),
            place("bookmarks", "/example/gone"),
            place("bookmarks", "/example/new"),
            locked,
        ];
        let quick = carry_over(&fresh, &measured);
        assert_eq!(quick[0].space, Some((1, 2)));
        assert!(quick[1].unavailable.is_some());
        assert!(quick[2].unavailable.is_none() && quick[2].space.is_none());
        assert!(quick[4].activate.is_some(), "volumes not mounted stay");
        assert_eq!(
            quick.len(),
            5,
            "what the scan no longer finds goes, what both list is there once"
        );
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
    #[cfg(target_os = "linux")]
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

    /// A keyring in memory: Skwir's own passphrases, and the desktop's
    /// (GVfs's) the test puts there.
    #[cfg(target_os = "linux")]
    #[derive(Default)]
    struct Memory {
        own: Mutex<std::collections::HashMap<String, String>>,
        desktop: Mutex<std::collections::HashMap<String, String>>,
    }

    #[cfg(target_os = "linux")]
    impl Keyring for Memory {
        fn own(&self, uuid: &str) -> Option<String> {
            self.own.lock().get(uuid).cloned()
        }
        fn desktop(&self, uuid: &str) -> Option<String> {
            self.desktop.lock().get(uuid).cloned()
        }
        fn keep(&self, uuid: &str, _label: &str, passphrase: &str) -> anyhow::Result<()> {
            self.own
                .lock()
                .insert(uuid.to_owned(), passphrase.to_owned());
            Ok(())
        }
        fn forget(&self, uuid: &str) -> anyhow::Result<()> {
            self.own.lock().remove(uuid);
            Ok(())
        }
    }

    /// Opening a locked volume (ADR-088): the passphrase typed and asked
    /// to be remembered is kept once it unlocked the volume, and opens it
    /// next time without a question; once refused (changed since) the
    /// question says so, and the one that served without Remember makes
    /// Skwir forget its own. GVfs's opens it, read only. Cancel leaves it
    /// locked, unsaid; three refusals leave it locked, said. udisks and
    /// lsblk are scripts that take one passphrase, read from their input.
    #[cfg(target_os = "linux")]
    #[test]
    #[allow(unsafe_code)]
    fn a_remembered_passphrase_opens_the_volume_until_it_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("bin");
        let mnt = tmp.path().join("mnt");
        std::fs::create_dir(&bin).unwrap();
        std::fs::create_dir(&mnt).unwrap();
        let (right, unlocked, mounted) = (
            tmp.path().join("right"),
            tmp.path().join("unlocked"),
            tmp.path().join("mounted"),
        );
        std::fs::write(&right, "example words").unwrap();
        let script = |name: &str, body: String| {
            let path = bin.join(name);
            std::fs::write(&path, format!("#!/bin/sh\n{body}")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        };
        let container = r#"{"path":"/dev/sdz","label":null,"size":2000000000000,"fstype":"crypto_LUKS","mountpoint":null,"hotplug":true,"pkname":null}"#;
        let opened = format!(
            r#"{{"path":"/dev/dm-9","label":"Backup","size":1999000000000,"fstype":"ext4","mountpoint":"{}","hotplug":false,"pkname":"sdz"}}"#,
            mnt.display()
        );
        script(
            "lsblk",
            format!(
                "if [ -e '{mounted}' ]; then echo '{{\"blockdevices\": [{container},{opened}]}}'\n\
                 else echo '{{\"blockdevices\": [{container}]}}'; fi\n",
                mounted = mounted.display(),
            ),
        );
        script(
            "udisksctl",
            format!(
                "case \"$1\" in\n\
                 unlock) given=$(cat)\n\
                   if [ \"$given\" != \"$(cat '{right}')\" ]; then echo 'No key available with this passphrase' >&2; exit 1; fi\n\
                   touch '{unlocked}'; echo 'Unlocked /dev/sdz as /dev/dm-9.';;\n\
                 mount) touch '{mounted}'; echo 'Mounted /dev/dm-9 at {mnt}';;\n\
                 *) exit 1;;\n\
                 esac\n",
                right = right.display(),
                unlocked = unlocked.display(),
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
        app.set_window_shown();
        let keyring = Arc::new(Memory::default());
        app.plugins
            .add(Arc::new(PlacesPlugin::with_keyring(keyring.clone())), &app);
        let uuid = "1111-luks";
        let wait = |done: &dyn Fn() -> bool| {
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            while !done() {
                assert!(std::time::Instant::now() < deadline, "timed out");
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        let volume = || {
            app.registry
                .places
                .all()
                .into_iter()
                .find(|p| p.location.to_string() == "device:/dev/sdz")
        };
        let open = || {
            app.dispatch(&Invocation::with_args(
                "places.open_locked",
                &serde_json::json!({ "device": "/dev/sdz", "title": "Backup", "uuid": uuid }),
            ))
            .unwrap();
        };
        let asked = || {
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            loop {
                if let Some(question) = app.take_secret_questions().into_iter().next() {
                    return question;
                }
                assert!(std::time::Instant::now() < deadline, "nothing asked");
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        let answer = |id: u64, text: &str, remember: bool| {
            let text = text.to_owned();
            app.answer_secret(id, Some(skwir_core::app::SecretAnswer { text, remember }));
        };
        // Locked again: unmounted, its container closed.
        let lock = || {
            for marker in [&unlocked, &mounted] {
                if marker.exists() {
                    std::fs::remove_file(marker).unwrap();
                }
            }
            app.dispatch(&Invocation::new("places.refresh")).unwrap();
            wait(&|| volume().is_some_and(|v| v.busy.is_none()));
        };
        let remembered = || passphrases::remembered(&app);
        assert_eq!(
            app.commands
                .get("places.open_locked")
                .map(|spec| spec.effect),
            Some(skwir_core::command::Effect::UserOnly),
            "a kept passphrase opens a volume only for the user, never an agent"
        );
        wait(&|| volume().is_some());

        // Typed, Remember on: kept once it served.
        open();
        let question = asked();
        assert_eq!(question.title, "Unlock “Backup”");
        assert_eq!(question.placeholder, "Passphrase");
        assert!(
            question.remember && question.keeping.is_some(),
            "offered, with what it means"
        );
        answer(question.id, "example words", true);
        assert!(next_notice(&app).text.starts_with("Mounted"));
        assert_eq!(keyring.own(uuid).as_deref(), Some("example words"));
        assert_eq!(remembered(), [uuid], "its UUID noted, never the passphrase");

        // Kept: opened without a question.
        lock();
        open();
        assert!(next_notice(&app).text.starts_with("Mounted"));
        assert!(app.take_secret_questions().is_empty(), "nothing asked");

        // Changed since: refused, asked saying so; the one typed without
        // Remember served, so Skwir's own is forgotten.
        lock();
        std::fs::write(&right, "other words").unwrap();
        open();
        let question = asked();
        assert!(
            question.title.contains("not accepted"),
            "{}",
            question.title
        );
        answer(question.id, "other words", false);
        assert!(next_notice(&app).text.starts_with("Mounted"));
        assert_eq!(keyring.own(uuid), None, "the refused one forgotten");
        assert!(remembered().is_empty());

        // GVfs's: opened without a question, never copied or changed.
        lock();
        keyring
            .desktop
            .lock()
            .insert(uuid.into(), "other words".into());
        open();
        assert!(next_notice(&app).text.starts_with("Mounted"));
        assert!(app.take_secret_questions().is_empty(), "nothing asked");
        assert_eq!(keyring.own(uuid), None);
        assert_eq!(
            keyring.desktop.lock().get(uuid).map(String::as_str),
            Some("other words")
        );

        // Cancel: left locked, nothing said.
        lock();
        keyring.desktop.lock().clear();
        open();
        let question = asked();
        app.answer_secret(question.id, None);
        wait(&|| volume().is_some_and(|v| v.busy.is_none()));
        assert!(!unlocked.exists());
        std::thread::sleep(Duration::from_millis(100));
        assert!(app.take_notifications().is_empty(), "nothing to say");

        // Refused three times: left locked, said.
        open();
        for _ in 0..3 {
            let question = asked();
            answer(question.id, "wrong words", true);
        }
        let notice = next_notice(&app);
        assert!(
            notice.text.contains("passphrase was not accepted"),
            "{}",
            notice.text
        );
        assert!(
            app.take_secret_questions().is_empty(),
            "not asked a fourth time"
        );
        assert!(keyring.own(uuid).is_none(), "a refused one never kept");
    }

    /// Forget Passphrase is on the place of a volume whose passphrase
    /// Skwir keeps, mounted or locked, a subvolume's included, and on no
    /// other; it comes off once forgotten.
    #[test]
    fn forget_passphrase_is_offered_where_one_is_kept() {
        let place = |key: &str| Place {
            key: Some(key.to_owned()),
            ..Place::new("devices", "Backup", "", Location::parse("device:/dev/sdz"))
        };
        let mut places = vec![
            place("volume:1111-luks"),
            place("volume:1111-luks/@home"),
            place("volume:2222"),
        ];
        let forgets = |places: &[Place]| {
            places
                .iter()
                .map(|p| {
                    p.menu
                        .iter()
                        .filter(|m| m.command.name.as_ref() == "places.forget_passphrase")
                        .count()
                })
                .collect::<Vec<_>>()
        };
        let kept = vec!["1111-luks".to_owned()];
        offer_forget(&mut places, &kept);
        offer_forget(&mut places, &kept);
        assert_eq!(forgets(&places), [1, 1, 0], "once each");
        assert_eq!(
            places[1].menu[0].command.args,
            serde_json::json!({ "volume": "1111-luks" })
        );
        offer_forget(&mut places, &[]);
        assert_eq!(forgets(&places), [0, 0, 0]);
    }

    fn next_notice(app: &App) -> skwir_core::app::Notification {
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
        let undo = |notice: &skwir_core::app::Notification, label: &str| {
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
                skwir_platform::uri::file_uri(&b),
                skwir_platform::uri::file_uri(&a)
            )
        );

        // Only folders; and a bookmark whose folder went is shown as such.
        let note = tmp.path().join("note.txt");
        std::fs::write(&note, b"x").unwrap();
        // Asked from no window's thread, refused to its caller.
        let refused = app
            .dispatch(&Invocation::with_args(
                "places.bookmark",
                &serde_json::json!({ "path": path(&note) }),
            ))
            .unwrap_err();
        assert!(
            format!("{refused:#}").contains("only folders can be bookmarked"),
            "{refused:#}"
        );
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

    /// A remote folder's bookmark is renamed, moved among the folders'
    /// and taken out as theirs are, by its address, each edit undone: the
    /// sidebar shows the file's order, and the file keeps it.
    #[test]
    #[allow(unsafe_code)]
    fn a_remote_bookmark_is_renamed_and_moved_among_folders() {
        let tmp = tempfile::tempdir().unwrap();
        // SAFETY: one test per process under nextest, set before the app
        // starts any thread; the bookmarks file is then this test's own.
        unsafe { std::env::set_var("XDG_CONFIG_HOME", tmp.path().join("config")) };
        let a = tmp.path().join("a");
        std::fs::create_dir(&a).unwrap();
        let address = "sftp://me@example.com/home/me/Two%20Words";
        let file = tmp.path().join("config/gtk-3.0/bookmarks");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let first = format!("{}\n{address}\n", skwir_platform::uri::file_uri(&a));
        std::fs::write(&file, &first).unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(PlacesPlugin::default()), &app);
        let run = |name: &str, args: serde_json::Value| {
            app.dispatch(&Invocation::with_args(name, &args)).unwrap();
            next_notice(&app)
        };
        let undo = |notice: &skwir_core::app::Notification| {
            let action = notice.action.as_ref().expect("a way back");
            app.dispatch(&action.command).unwrap();
            next_notice(&app)
        };
        assert_eq!(titles(&app), ["a", "Two Words"]);
        let remote = bookmarks(&app)[1].clone();
        assert_eq!(remote.location, Location::parse(address));

        // F2: the question asks the rename command with the address.
        let ask = remote.rename.clone().expect("renamed in the sidebar");
        assert_eq!(ask.args["command"], "places.rename_bookmark");
        let mut args = ask.args["args"].clone();
        args["title"] = serde_json::json!("Work\nfiles");
        let renamed = run("places.rename_bookmark", args);
        assert_eq!(renamed.text, "Renamed bookmark “Two Words” to “Work files”");
        assert_eq!(titles(&app), ["a", "Work files"]);
        undo(&renamed);
        assert_eq!(titles(&app), ["a", "Two Words"]);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), first);

        // Dragged before the folder's, as the sidebar does; then back.
        let folder = bookmarks(&app)[0].clone();
        let mut drag = remote
            .reorder
            .clone()
            .expect("moved in the sidebar")
            .command;
        assert_eq!(drag.name, folder.reorder.clone().unwrap().command.name);
        drag.args["before"] = folder.reorder.unwrap().before;
        let moved = run(&drag.name, drag.args);
        assert_eq!(moved.text, "Moved bookmark “Two Words”");
        assert_eq!(titles(&app), ["Two Words", "a"]);
        undo(&moved);
        assert_eq!(titles(&app), ["a", "Two Words"]);

        // Taken out and put back where it stood, before a later folder.
        let b = tmp.path().join("b");
        std::fs::create_dir(&b).unwrap();
        run(
            "places.bookmark",
            serde_json::json!({ "path": PathArg::from(b.as_path()) }),
        );
        assert_eq!(titles(&app), ["a", "Two Words", "b"]);
        let forget = bookmarks(&app)[1].forget.clone().unwrap();
        let removed = run(&forget.name, forget.args);
        assert_eq!(titles(&app), ["a", "b"]);
        undo(&removed);
        assert_eq!(titles(&app), ["a", "Two Words", "b"], "back in its place");
    }

    #[cfg(unix)]
    #[test]
    fn unmount_takes_a_path_that_is_not_utf8() {
        use std::os::unix::ffi::OsStrExt;
        let tmp = tempfile::tempdir().unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(PlacesPlugin::default()), &app);
        let path = std::path::Path::new(std::ffi::OsStr::from_bytes(b"/run/media/x/caf\xe9"));
        let invocation = skwir_core::command::Invocation::try_with_args(
            "places.unmount",
            &serde_json::json!({ "path": PathArg::from(path) }),
        )
        .unwrap();
        let error = app.dispatch(&invocation).unwrap_err().to_string();
        assert!(error.contains("not a removable volume"), "{error}");
    }

    /// A bookmark asked anywhere but on the window's thread (another
    /// plugin's worker, an agent's call) is written before the command
    /// returns, and a refusal is the caller's: Add Server says nothing it
    /// has not seen done. From the window's thread, it is written on a
    /// worker, and a refusal is said.
    #[test]
    #[allow(unsafe_code)]
    fn a_bookmark_is_kept_when_its_command_returns_off_the_window() {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        // SAFETY: one test per process under nextest, set before the app
        // starts any thread; the bookmarks file is then this test's own.
        unsafe { std::env::set_var("XDG_CONFIG_HOME", &config) };
        let file = tmp.path().join("notes, not a folder.txt");
        std::fs::write(&file, "x").unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(PlacesPlugin::default()), &app);
        let kept = || std::fs::read_to_string(config.join("gtk-3.0/bookmarks")).unwrap_or_default();

        let address = "sftp://nas.example/srv/it's shared";
        let asked = std::thread::spawn({
            let app = app.clone();
            move || {
                app.dispatch(&Invocation::with_args(
                    "places.bookmark_address",
                    &serde_json::json!({ "address": address, "title": "Shared" }),
                ))
            }
        });
        asked.join().unwrap().unwrap();
        assert!(
            kept().contains(" Shared"),
            "written once it returned: {}",
            kept()
        );
        assert_eq!(next_notice(&app).text, "Bookmarked “Shared”");
        let refused = app
            .dispatch(&Invocation::with_args(
                "places.bookmark",
                &serde_json::json!({ "path": PathArg::from(file.as_path()) }),
            ))
            .unwrap_err();
        assert!(
            format!("{refused:#}").contains("is not a folder"),
            "{refused:#}"
        );
        assert!(app.take_notifications().is_empty(), "the caller's to say");

        skwir_core::scheduler::mark_window_thread(&app.scheduler);
        app.dispatch(&Invocation::with_args(
            "places.bookmark",
            &serde_json::json!({ "path": PathArg::from(file.as_path()) }),
        ))
        .unwrap();
        let said = next_notice(&app);
        assert_eq!(said.level, NotifyLevel::Error);
        assert!(said.text.contains("is not a folder"), "{}", said.text);
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
