//! bibli plugin: open-with. Opens entries with the default application,
//! enters directories, and opens a terminal in the current directory.

use bibli_core::app::App;
use bibli_core::command::{Invocation, Outcome};
use bibli_core::entry::{EntryId, EntryKind};
use bibli_core::location::Location;
use bibli_core::plugin::Plugin;
use bibli_core::registry::MenuItem;
use bibli_core::workspace::PaneId;

const ID: &str = "open-with";

/// The open-with plugin.
#[derive(Debug, Default)]
pub struct OpenWithPlugin;

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
struct OpenArgs {
    #[serde(default)]
    pane: Option<PaneId>,
    /// Entries to open; the selection when omitted.
    #[serde(default)]
    ids: Vec<EntryId>,
}

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
struct PaneArgs {
    #[serde(default)]
    pane: Option<PaneId>,
}

fn pane_of(app: &App, pane: Option<PaneId>) -> PaneId {
    pane.unwrap_or_else(|| app.workspace.read().active_id())
}

fn open(args: OpenArgs, app: &App) -> anyhow::Result<Outcome> {
    let pane = pane_of(app, args.pane);
    // Resolve (kind, path) for every target while holding the read lock, then release it.
    let targets: Vec<(EntryKind, std::path::PathBuf)> = {
        let ws = app.workspace.read();
        let p = ws
            .pane(pane)
            .ok_or_else(|| anyhow::anyhow!("no such pane"))?;
        let open = p
            .open
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("pane has no location"))?;
        let ids: Vec<EntryId> = if args.ids.is_empty() {
            p.selection.iter().collect()
        } else {
            args.ids
        };
        ids.into_iter()
            .filter_map(|id| {
                let kind = p.listing.get(id)?.kind;
                let path = open.path_of(id)?;
                Some((kind, path))
            })
            .collect()
    };
    if targets.is_empty() {
        anyhow::bail!("nothing to open");
    }
    if let [(EntryKind::Directory, dir)] = targets.as_slice() {
        app.navigate(pane, &Location::from_path(dir))?;
        return Ok(Outcome::Done);
    }
    for (_, path) in &targets {
        bibli_platform::open::open_path(path)?;
    }
    Ok(Outcome::Done)
}

fn open_terminal(pane: Option<PaneId>, app: &App) -> anyhow::Result<Outcome> {
    let pane = pane_of(app, pane);
    let dir = {
        let ws = app.workspace.read();
        let p = ws
            .pane(pane)
            .ok_or_else(|| anyhow::anyhow!("no such pane"))?;
        p.location()
            .and_then(Location::path)
            .ok_or_else(|| anyhow::anyhow!("pane is not showing a directory"))?
    };
    bibli_platform::open::open_terminal(&dir)?;
    Ok(Outcome::Done)
}

impl Plugin for OpenWithPlugin {
    fn id(&self) -> &'static str {
        ID
    }

    fn title(&self) -> &'static str {
        "Open with"
    }

    fn description(&self) -> &'static str {
        "Open files with their default application and open a terminal here"
    }

    fn register(&self, app: &App) {
        app.commands.register::<OpenArgs, _>(ID, "open", "Open", "Enter the selected directory, or open the selected files with their default application", open);
        app.commands.register::<PaneArgs, _>(
            ID,
            "open_terminal",
            "Open terminal here",
            "Open a terminal in the current directory",
            |a, app| open_terminal(a.pane, app),
        );
        app.registry.menus.add(
            ID,
            MenuItem {
                menu: "context".into(),
                title: "Open".into(),
                command: Invocation::new("open"),
                when: Some("selection".into()),
                order: 0,
            },
        );
        app.registry.menus.add(
            ID,
            MenuItem {
                menu: "context".into(),
                title: "Open Terminal Here".into(),
                command: Invocation::new("open_terminal"),
                when: None,
                order: 10,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bibli_core::app::AppConfig;
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn open_enters_a_single_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(OpenWithPlugin), &app);
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

    #[test]
    fn withdraws_on_disable() {
        let tmp = tempfile::tempdir().unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(OpenWithPlugin), &app);
        assert!(app.commands.get("open").is_some());
        assert_eq!(app.registry.menus.all().len(), 2);
        app.plugins.set_enabled(ID, false, &app);
        assert!(app.commands.get("open").is_none());
        assert!(app.registry.menus.all().is_empty());
    }
}
