//! bibli plugin: places. Fills the sidebar from what the desktop already
//! knows: home, XDG user dirs, GTK bookmarks, mounted volumes.

use bibli_core::app::App;
use bibli_core::command::Outcome;
use bibli_core::location::Location;
use bibli_core::plugin::Plugin;
use bibli_core::registry::Place;

const ID: &str = "places";

/// The places plugin.
#[derive(Debug, Default)]
pub struct PlacesPlugin;

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
            let measured: Vec<Place> = places
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
                        let (program, arguments) =
                            bibli_platform::places::unmount_command(&mount.source);
                        let outcome = std::process::Command::new(program).args(arguments).output();
                        match outcome {
                            Ok(output) if output.status.success() => {
                                populate(&app);
                                app.notify(
                                    NotifyLevel::Info,
                                    format!("“{}” can be removed", mount.label),
                                );
                            }
                            Ok(output) => app.notify(
                                NotifyLevel::Error,
                                format!(
                                    "“{}” was not unmounted: {}",
                                    mount.label,
                                    String::from_utf8_lossy(&output.stderr).trim()
                                ),
                            ),
                            Err(error) => app.notify(
                                NotifyLevel::Error,
                                format!("udisksctl could not be run: {error}"),
                            ),
                        }
                    },
                );
                Ok(Outcome::Done)
            },
        );
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
        app.plugins.add(Arc::new(PlacesPlugin), &app);
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
