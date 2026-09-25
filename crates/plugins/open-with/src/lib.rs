//! bibli plugin: open-with. Opens entries with the default application,
//! enters directories, opens a terminal in the current directory, and opens
//! files or folders with an application the user chooses.
//!
//! Choosing is the shell's `ui.choose` picker: `open_with.choices` lists
//! the applications for the selection, each choice an `open_with` that
//! names its application, pane and entries. The desktop's catalog of
//! applications is read on a worker and kept until an install or a changed
//! default makes it stale (`bibli_platform::apps::Stamp`).

use std::path::PathBuf;
use std::sync::Arc;

use bibli_core::app::{App, NotifyLevel, PaneArg};
use bibli_core::command::{Invocation, Outcome};
use bibli_core::entry::{EntryId, EntryKind};
use bibli_core::location::Location;
use bibli_core::plugin::Plugin;
use bibli_core::registry::{KeyBinding, MenuItem};
use bibli_core::scheduler::Priority;
use bibli_core::workspace::PaneId;
use bibli_platform::apps::{Application, Catalog, Stamp};
use bibli_platform::mime::{self, Mime};
use parking_lot::Mutex;

const ID: &str = "open-with";

/// The catalog of applications last read, with the stamp it was read at.
type Cached = Arc<Mutex<Option<(Stamp, Arc<Catalog>)>>>;

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
}

fn open(args: OpenArgs, app: &App) -> anyhow::Result<Outcome> {
    let pane = app.pane_or_active(args.pane);
    // Resolve (kind, path) for every target while holding the read lock, then release it.
    let (targets, lifetime): (Vec<(EntryKind, std::path::PathBuf)>, _) = {
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
        let targets = ids
            .into_iter()
            .filter_map(|id| {
                let kind = p.listing.get(id)?.kind;
                let path = open.path_of(id)?;
                Some((kind, path))
            })
            .collect();
        (targets, p.location_lifetime())
    };
    if targets.is_empty() {
        anyhow::bail!("nothing to open");
    }
    match targets.as_slice() {
        [(EntryKind::Directory, dir)] => {
            app.navigate(pane, &Location::from_path(dir))?;
            return Ok(Outcome::Done);
        }
        // A link may lead to a folder, which opens here like one; asking
        // follows the link, which can wait on a sleeping disk, so a worker
        // asks. Navigating away first cancels it.
        [(EntryKind::Symlink | EntryKind::Unknown, path)] => {
            let path = path.clone();
            let weak = app.weak();
            let cancel = lifetime.clone();
            app.scheduler
                .spawn(Priority::Normal, "open-with.resolve", lifetime, move || {
                    let Some(app) = weak.upgrade() else { return };
                    let result = if path.is_dir() {
                        if cancel.is_cancelled() {
                            return;
                        }
                        app.navigate(pane, &Location::from_path(&path))
                    } else {
                        bibli_platform::open::open_path(&path)
                    };
                    match result {
                        Ok(()) => app.wake(),
                        Err(e) => app.notify(NotifyLevel::Error, format!("{e:#}")),
                    }
                });
            return Ok(Outcome::Done);
        }
        _ => {}
    }
    for (_, path) in &targets {
        bibli_platform::open::open_path(path)?;
    }
    Ok(Outcome::Done)
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
        && let Some(e) = p.listing.get(id)
        && e.is_dir()
        && let Some(open) = &p.open
        && let Some(path) = open.path_of(id)
    {
        return Ok(path);
    }
    drop(ws);
    app.pane_dir(pane)
}

fn open_terminal(pane: Option<PaneId>, app: &App) -> anyhow::Result<Outcome> {
    let dir = target_dir(app.pane_or_active(pane), app)?;
    bibli_platform::open::open_terminal(&dir)?;
    Ok(Outcome::Done)
}

/// The applications, read again only when the stamp moved. Stats the
/// applications directories and lists, and reads them when they changed:
/// call from a worker.
fn catalog(cache: &Cached) -> Arc<Catalog> {
    let stamp = Stamp::current();
    if let Some((at, catalog)) = &*cache.lock()
        && *at == stamp
    {
        return Arc::clone(catalog);
    }
    let catalog = Arc::new(Catalog::load());
    *cache.lock() = Some((stamp, Arc::clone(&catalog)));
    catalog
}

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
struct TargetArgs {
    #[serde(default)]
    pane: Option<PaneId>,
    /// Entries to open; the selection when omitted.
    #[serde(default)]
    ids: Vec<EntryId>,
    /// The pane's folder itself, not entries in it.
    #[serde(default)]
    here: bool,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct OpenWithArgs {
    /// The application's desktop file ID (`org.gnome.Loupe.desktop`), as
    /// `open_with.choices` lists them.
    app: String,
    #[serde(flatten)]
    target: TargetArgs,
}

/// What is opened: a path, and whether it is known to be a folder (a link
/// or an entry of unknown kind is asked when its type is).
struct Target {
    path: PathBuf,
    kind: EntryKind,
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
        };
        return Ok((pane, Vec::new(), vec![target]));
    }
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
    let mut named = Vec::new();
    let mut targets = Vec::new();
    for id in ids {
        let (Some(entry), Some(path)) = (p.listing.get(id), open.path_of(id)) else {
            continue;
        };
        named.push(id);
        targets.push(Target {
            path,
            kind: entry.kind,
        });
    }
    drop(ws);
    anyhow::ensure!(!targets.is_empty(), "nothing to open");
    Ok((pane, named, targets))
}

impl Target {
    /// Reads the head of a file, and follows a link: call from a worker.
    fn mime(&self) -> Mime {
        let is_dir = match self.kind {
            EntryKind::Directory => true,
            EntryKind::Symlink | EntryKind::Unknown => self.path.is_dir(),
            _ => false,
        };
        bibli_platform::apps::type_of(&self.path, is_dir)
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

fn choices(args: TargetArgs, cache: &Cached, app: &App) -> anyhow::Result<Outcome> {
    let here = args.here;
    let (pane, ids, targets) = targets(args, app)?;
    let mut types: Vec<Mime> = Vec::new();
    for t in &targets {
        let m = t.mime();
        if !types.contains(&m) {
            types.push(m);
        }
    }
    let catalog = catalog(cache);
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
    Ok(Outcome::Value(serde_json::json!({
        "title": title,
        "empty": "No applications found",
        "choices": choices,
    })))
}

fn open_with(args: OpenWithArgs, cache: &Cached, app: &App) -> anyhow::Result<Outcome> {
    let (_, _, targets) = targets(args.target, app)?;
    let paths: Vec<PathBuf> = targets.into_iter().map(|t| t.path).collect();
    let id = args.app;
    // Reading the catalog and starting the program touch the disk.
    let cache = Arc::clone(cache);
    let weak = app.weak();
    app.scheduler.spawn(
        Priority::Visible,
        "open-with.launch",
        app.lifetime().child(),
        move || {
            let Some(app) = weak.upgrade() else { return };
            let catalog = catalog(&cache);
            let result = match catalog.get(&id) {
                Some(application) => application.launch(&paths),
                None => Err(anyhow::anyhow!("no application “{id}” is installed")),
            };
            if let Err(e) = result {
                app.notify(NotifyLevel::Error, format!("{e:#}"));
            }
        },
    );
    Ok(Outcome::Done)
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

    fn category(&self) -> bibli_core::plugin::Category {
        bibli_core::plugin::Category::Desktop
    }

    fn register(&self, app: &App) {
        app.commands.register::<OpenArgs, _>(ID, "open", "Open", "Enter the selected directory, or open the selected files with their default application", open);
        app.commands.register::<PaneArg, _>(
            ID,
            "open_terminal",
            "Open terminal here",
            "Open a terminal in the selected folder, or in the current directory",
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
                icon: Some("open".into()),
                submenu: None,
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
                icon: Some("command".into()),
                submenu: None,
            },
        );
        let cache = Arc::clone(&self.catalog);
        app.commands.register::<TargetArgs, _>(
            ID,
            "open_with.choices",
            "List applications for the selection",
            "List the applications that open the selected entries (or the pane's folder with `here`), recommended first, each with the `open_with` invocation that opens them",
            move |a, app| choices(a, &cache, app),
        );
        let cache = Arc::clone(&self.catalog);
        app.commands.register::<OpenWithArgs, _>(
            ID,
            "open_with",
            "Open with an application by desktop ID",
            "Open the selected entries (or the pane's folder with `here`) with the application whose desktop file ID is `app`",
            move |a, app| open_with(a, &cache, app),
        );
        app.registry.menus.add(
            ID,
            MenuItem {
                menu: "context".into(),
                title: "Open With…".into(),
                command: choose(&serde_json::json!({})),
                when: Some("selection".into()),
                order: 1,
                icon: Some("open".into()),
                submenu: None,
            },
        );
        app.registry.menus.add(
            ID,
            MenuItem {
                menu: "context".into(),
                title: "Open Folder With…".into(),
                command: choose(&serde_json::json!({ "here": true })),
                when: Some("empty".into()),
                order: 11,
                icon: Some("open".into()),
                submenu: None,
            },
        );
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
        self.catalog.lock().take();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bibli_core::app::AppConfig;
    use std::time::Duration;

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
        app.plugins.set_enabled(ID, false, &app);
        assert!(app.commands.get("open").is_none());
        assert!(app.registry.menus.all().is_empty());
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

    fn cached(catalog: Catalog) -> Cached {
        Arc::new(Mutex::new(Some((Stamp::current(), Arc::new(catalog)))))
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
            own("browser.desktop", Some(Fit::Exact), false),
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

    #[test]
    fn choices_name_the_application_pane_and_entries_to_open() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        let photo = "it's a “photo” .png";
        std::fs::write(root.join(photo), b"").unwrap();
        let plugin = OpenWithPlugin {
            catalog: cached(fake_catalog(tmp.path(), std::path::Path::new("record"))),
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
        let record = tmp.path().join("record");
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
        let plugin = OpenWithPlugin {
            catalog: cached(fake_catalog(tmp.path(), &record)),
        };
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(plugin), &app);
        let pane = app.workspace.read().active_id();
        app.navigate(pane, &Location::from_path(&root)).unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
        select(&app, pane, &names);
        app.dispatch(&Invocation::with_args(
            "open_with",
            &serde_json::json!({ "app": "viewer.desktop" }),
        ))
        .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !out.exists() {
            assert!(std::time::Instant::now() < deadline, "never launched");
            std::thread::sleep(Duration::from_millis(5));
        }
        let mut got: Vec<String> = std::fs::read_to_string(&out)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect();
        got.sort();
        let want: Vec<String> = names
            .iter()
            .map(|n| root.join(n).display().to_string())
            .collect();
        assert_eq!(got, want, "one run with every file, as %F says");
    }
}
