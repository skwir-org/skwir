//! bibli plugin: places. Fills the sidebar from what the desktop already
//! knows: home, XDG user dirs, GTK bookmarks, mounted volumes.

use bibli_core::app::App;
use bibli_core::command::Outcome;
use bibli_core::location::Location;
use bibli_core::plugin::Plugin;
use bibli_core::registry::Place;

const ID: &str = "places";

/// The places plugin.
#[derive(Default)]
pub struct PlacesPlugin {
    /// Watches for volumes being plugged, unplugged, mounted or unmounted.
    watcher: parking_lot::Mutex<Option<notify::RecommendedWatcher>>,
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
    }
}

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
struct NoArgs {}

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

/// Scan the environment and return every place, in sidebar order.
pub fn scan() -> Vec<Place> {
    let mut out = Vec::new();
    if let Some(home) = bibli_platform::places::home() {
        out.push(Place {
            title: "Home".into(),
            location: Location::from_path(home),
            icon: Some("user-home".into()),
            group: "places".into(),
            space: None,
            eject: None,
            activate: None,
        });
    }
    for d in bibli_platform::places::user_dirs() {
        out.push(Place {
            title: d.name,
            location: Location::from_path(d.path),
            icon: Some(icon_for_user_dir(&d.kind).into()),
            group: "places".into(),
            space: None,
            eject: None,
            activate: None,
        });
    }
    for b in bibli_platform::places::bookmarks() {
        out.push(Place {
            title: b.title,
            location: Location::from_path(b.path),
            icon: Some("folder".into()),
            group: "bookmarks".into(),
            space: None,
            eject: None,
            activate: None,
        });
    }
    for m in bibli_platform::places::mounts() {
        let icon = if m.removable {
            "drive-removable-media"
        } else {
            "drive-harddisk"
        };
        out.push(Place {
            title: m.label,
            location: Location::from_path(&m.path),
            icon: Some(icon.into()),
            group: "devices".into(),
            space: None,
            // Only what the user mounted: system volumes are not ours to detach.
            eject: m.removable.then(|| {
                bibli_core::command::Invocation::with_args(
                    "places.unmount",
                    &serde_json::json!({ "path": m.path }),
                )
            }),
            activate: None,
        });
    }
    out
}

/// Publish the places now; ask each volume for its free space on a worker
/// (a sleeping or unreachable volume can block that call) and publish again
/// with the answers.
fn populate(app: &App) {
    let places = scan();
    app.registry.places.remove_owner(ID);
    for place in places.clone() {
        app.registry.places.add(ID, place);
    }
    let weak = app.weak();
    app.scheduler.spawn(
        bibli_core::scheduler::Priority::Background,
        "places-space",
        app.lifetime().child(),
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
            let Some(app) = weak.upgrade() else { return };
            // Disabled meanwhile: a late answer must not bring the places back.
            if !app.plugins.is_enabled(ID) {
                return;
            }
            app.registry.places.remove_owner(ID);
            for place in measured {
                app.registry.places.add(ID, place);
            }
            app.wake();
        },
    );
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
    app: &App,
    device: String,
    passphrase: Option<String>,
    pane: Option<bibli_core::workspace::PaneId>,
) {
    let pane = pane.unwrap_or_else(|| app.workspace.read().active_id());
    let weak = app.weak();
    app.scheduler.spawn(
        bibli_core::scheduler::Priority::Normal,
        "places-attach",
        app.lifetime().child(),
        move || {
            use bibli_core::app::NotifyLevel;
            let Some(app) = weak.upgrade() else { return };
            let mounted = passphrase
                .map_or_else(
                    || Ok(device.clone()),
                    |passphrase| bibli_platform::places::unlock(&device, &passphrase),
                )
                .and_then(|cleartext| bibli_platform::places::mount(&cleartext));
            // Whatever happened, the set of volumes may have changed (a
            // container can unlock and its filesystem still fail to mount).
            populate(&app);
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
        },
    );
}

impl PlacesPlugin {
    /// Republish when a volume appears, disappears, or is mounted by anyone:
    /// udev keeps `/dev/disk/by-uuid` and udisks keeps `/run/media/<user>`.
    /// Bursts (a disk brings several partitions) collapse into one refresh.
    fn watch(&self, app: &App) {
        use notify::Watcher as _;
        let weak = app.weak();
        let scheduled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let handler = move |event: notify::Result<notify::Event>| {
            if event.is_err() || scheduled.swap(true, std::sync::atomic::Ordering::AcqRel) {
                return;
            }
            let Some(app) = weak.upgrade() else { return };
            let (weak, scheduled) = (weak.clone(), std::sync::Arc::clone(&scheduled));
            app.scheduler.spawn(
                bibli_core::scheduler::Priority::Background,
                "places-changed",
                app.lifetime().child(),
                move || {
                    std::thread::sleep(std::time::Duration::from_millis(400));
                    scheduled.store(false, std::sync::atomic::Ordering::Release);
                    if let Some(app) = weak.upgrade()
                        && app.plugins.is_enabled(ID)
                    {
                        populate(&app);
                    }
                },
            );
        };
        let Ok(mut watcher) = notify::recommended_watcher(handler) else {
            return;
        };
        let mut folders = vec![std::path::PathBuf::from("/dev/disk/by-uuid")];
        if let Some(user) = std::env::var_os("USER") {
            folders.push(std::path::Path::new("/run/media").join(user));
        }
        for folder in folders {
            // A folder that does not exist yet is simply not watched.
            let _ = watcher.watch(&folder, notify::RecursiveMode::NonRecursive);
        }
        *self.watcher.lock() = Some(watcher);
    }
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct PathArgs {
    /// A folder; the single selected folder of the active pane when omitted.
    #[serde(default)]
    path: Option<std::path::PathBuf>,
}

fn target(app: &App, path: Option<std::path::PathBuf>) -> anyhow::Result<std::path::PathBuf> {
    if let Some(path) = path {
        return Ok(path);
    }
    let pane = app.workspace.read().active_id();
    let selected = app.selected_refs(pane);
    anyhow::ensure!(selected.len() == 1, "select one folder");
    let path = selected[0].resolve()?;
    anyhow::ensure!(path.is_dir(), "only folders can be bookmarked");
    Ok(path)
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

    fn requirements(&self) -> Vec<bibli_core::plugin::Requirement> {
        vec![bibli_core::plugin::Requirement::program(
            "udisksctl",
            "Unmounting removable volumes from the sidebar",
        )]
    }

    fn on_disable(&self, _app: &App) {
        *self.watcher.lock() = None;
    }

    fn register(&self, app: &App) {
        populate(app);
        app.commands.register::<NoArgs, _>(
            ID,
            "places.refresh",
            "Refresh places",
            "Re-scan user folders, bookmarks, and mounts",
            |_, app| {
                populate(app);
                Ok(Outcome::Done)
            },
        );
        app.commands.register::<PathArgs, _>(
            ID,
            "places.bookmark",
            "Add to bookmarks",
            "Add a folder to the desktop's shared bookmarks (the GTK bookmarks file)",
            |args, app| {
                let path = target(app, args.path)?;
                let weak = app.weak();
                app.scheduler.spawn(
                    bibli_core::scheduler::Priority::Normal,
                    "places-bookmark",
                    app.lifetime().child(),
                    move || {
                        use bibli_core::app::NotifyLevel;
                        let Some(app) = weak.upgrade() else { return };
                        let name = path.file_name().map_or_else(
                            || path.display().to_string(),
                            |name| name.to_string_lossy().into_owned(),
                        );
                        match bibli_platform::places::add_bookmark(&path) {
                            Ok(true) => {
                                populate(&app);
                                app.notify(NotifyLevel::Info, format!("Bookmarked “{name}”"));
                            }
                            Ok(false) => app.notify(
                                NotifyLevel::Info,
                                format!("“{name}” is already bookmarked"),
                            ),
                            Err(error) => app.notify(
                                NotifyLevel::Error,
                                format!("The bookmark was not saved: {error:#}"),
                            ),
                        }
                    },
                );
                Ok(Outcome::Done)
            },
        );
        app.commands.register::<PathArgs, _>(
            ID,
            "places.unmount",
            "Unmount volume",
            "Detach a removable volume through udisks so it can be unplugged",
            |args, app| {
                let path = args
                    .path
                    .ok_or_else(|| anyhow::anyhow!("which volume? pass its mount point"))?;
                let mount = bibli_platform::places::mounts()
                    .into_iter()
                    .find(|mount| mount.path == path && mount.removable)
                    .ok_or_else(|| anyhow::anyhow!("not a removable volume"))?;
                let weak = app.weak();
                app.scheduler.spawn(
                    bibli_core::scheduler::Priority::Normal,
                    "places-unmount",
                    app.lifetime().child(),
                    move || {
                        use bibli_core::app::NotifyLevel;
                        let Some(app) = weak.upgrade() else { return };
                        match bibli_platform::places::unmount(&mount.source) {
                            Ok(()) => {
                                populate(&app);
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
                    },
                );
                Ok(Outcome::Done)
            },
        );
        app.commands.register::<MountArgs, _>(
            ID,
            "places.mount",
            "Mount volume",
            "Mount a present volume through udisks and open it",
            |args, app| {
                attach(app, args.device, None, args.pane);
                Ok(Outcome::Done)
            },
        );
        app.commands.register::<UnlockArgs, _>(
            ID,
            "places.unlock",
            "Unlock volume",
            "Unlock an encrypted volume with its passphrase, mount it and open it. The passphrase is a secret: it is handed to udisks on standard input and never stored or logged",
            |args, app| {
                attach(app, args.device, Some(args.passphrase), args.pane);
                Ok(Outcome::Done)
            },
        );
        self.watch(app);
        app.registry.menus.add(
            ID,
            bibli_core::registry::MenuItem {
                menu: "context".into(),
                title: "Add to Bookmarks".into(),
                command: bibli_core::command::Invocation::new("places.bookmark"),
                when: Some("single dir".into()),
                order: 145,
                icon: Some("tag".into()),
                submenu: Some("More".into()),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bibli_core::app::AppConfig;
    use std::sync::Arc;

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
}
