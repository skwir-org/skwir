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
        });
    }
    for d in bibli_platform::places::user_dirs() {
        out.push(Place {
            title: d.name,
            location: Location::from_path(d.path),
            icon: Some(icon_for_user_dir(&d.kind).into()),
            group: "places".into(),
        });
    }
    for b in bibli_platform::places::bookmarks() {
        out.push(Place {
            title: b.title,
            location: Location::from_path(b.path),
            icon: Some("folder".into()),
            group: "bookmarks".into(),
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
            location: Location::from_path(m.path),
            icon: Some(icon.into()),
            group: "devices".into(),
        });
    }
    out
}

fn populate(app: &App) {
    app.registry.places.remove_owner(ID);
    for p in scan() {
        app.registry.places.add(ID, p);
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
