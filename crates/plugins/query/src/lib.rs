//! bibli plugin: query as location.
//!
//! A query (`*.pdf modified<7d size>10M in:~/docs`) is a location. Results
//! stream in as a virtual listing, so every view, selection, and operation
//! works on them unchanged. Saved queries are smart folders in the sidebar.
//! The plan's `search` item (walker, content search, plocate) lives here too.

pub mod lang;
pub mod plocate;
pub mod provider;

use std::sync::Arc;

use bibli_core::app::App;
use bibli_core::cancel::CancelToken;
use bibli_core::command::{Invocation, NoArgs, Outcome};
use bibli_core::location::Location;
use bibli_core::os_path::PathArg;
use bibli_core::plugin::Plugin;
use bibli_core::registry::{KeyBinding, MenuItem, Place};
use bibli_core::settings::SettingSpec;
use bibli_core::workspace::PaneId;
use parking_lot::Mutex;

pub use provider::{QueryProvider, SCHEME};

const ID: &str = "query";
const SAVED_KEY: &str = "query.saved";

/// The query plugin.
#[derive(Debug, Default)]
pub struct QueryPlugin {
    /// Cancelled on disable: every walk and stat the provider started ends.
    lifetime: Mutex<Option<CancelToken>>,
    /// Held across a read-modify-write of the saved queries, so two saves
    /// never drop each other's change.
    saving: Arc<Mutex<()>>,
}

#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema, PartialEq, Eq,
)]
pub struct SavedQuery {
    pub name: String,
    pub expr: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct SearchArgs {
    /// The query expression, e.g. `*.pdf modified<7d`.
    expr: String,
    /// Subtree to search; the pane's current directory when omitted.
    #[serde(default)]
    root: Option<PathArg>,
    #[serde(default)]
    pane: Option<PaneId>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct SaveArgs {
    name: String,
    expr: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct ForgetArgs {
    name: String,
}

/// The location string for `expr` rooted at `root` (unless `expr` names a
/// root with `in:`, as the parser reads it).
pub fn location_for(expr: &str, root: Option<&std::path::Path>) -> Location {
    let has_root = lang::parse(expr).is_ok_and(|q| q.root_given);
    let rest = match root {
        Some(r) if !has_root => format!("{expr} in:{}", lang::quote_os(r.as_os_str())),
        _ => expr.to_owned(),
    };
    Location::new(SCHEME, rest)
}

/// Saved queries from settings.
pub fn saved(app: &App) -> Vec<SavedQuery> {
    app.settings
        .get::<Vec<SavedQuery>>(SAVED_KEY)
        .unwrap_or_default()
}

/// Change the saved queries and persist them, one change at a time.
fn edit_saved(
    app: &App,
    saving: &Mutex<()>,
    edit: impl FnOnce(&mut Vec<SavedQuery>) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let _saving = saving.lock();
    let mut list = saved(app);
    edit(&mut list)?;
    let arr = list
        .iter()
        .map(|s| {
            let mut t = toml::Table::new();
            t.insert("name".into(), toml::Value::String(s.name.clone()));
            t.insert("expr".into(), toml::Value::String(s.expr.clone()));
            toml::Value::Table(t)
        })
        .collect();
    app.settings.set(SAVED_KEY, toml::Value::Array(arr));
    app.settings
        .save()
        .map_err(|e| anyhow::anyhow!("the saved queries were not written: {e:#}"))?;
    register_places(app);
    Ok(())
}

fn register_places(app: &App) {
    app.registry.places.replace_owner(
        ID,
        saved(app).into_iter().map(|s| Place {
            title: s.name,
            location: Location::new(SCHEME, s.expr),
            icon: Some("folder-saved-search".into()),
            group: "queries".into(),
            space: None,
            eject: None,
            activate: None,
            unavailable: None,
            menu: Vec::new(),
        }),
    );
}

impl Plugin for QueryPlugin {
    fn id(&self) -> &'static str {
        ID
    }

    fn title(&self) -> &'static str {
        "Query"
    }

    fn description(&self) -> &'static str {
        "Search as a location: name, size, date, tag, MIME, and content queries; saved queries as smart folders"
    }

    fn requirements(&self) -> Vec<bibli_core::plugin::Requirement> {
        vec![bibli_core::plugin::Requirement::program(
            "plocate",
            "Instant name searches over whole disks (a directory walk is used otherwise)",
        )]
    }

    fn on_disable(&self, _app: &App) {
        if let Some(lifetime) = self.lifetime.lock().take() {
            lifetime.cancel();
        }
    }

    fn register(&self, app: &App) {
        let lifetime = app.lifetime().child();
        if let Some(previous) = self.lifetime.lock().replace(lifetime.clone()) {
            previous.cancel();
        }
        app.register_provider(ID, Arc::new(QueryProvider::new(lifetime)));
        app.settings.declare(SettingSpec::data(
            SAVED_KEY,
            "Saved queries shown in the sidebar: [{ name, expr }]",
            toml::Value::Array(Vec::new()),
        ));
        register_places(app);

        app.commands.register::<SearchArgs, _>(
            ID,
            "query.search",
            "Search",
            "Open a query as the pane's location (`*.pdf modified<7d size>10M`; `in:` picks the root)",
            |a, app| {
                let pane = app.pane_or_active(a.pane);
                lang::parse(&a.expr).map_err(|e| anyhow::anyhow!("query: {e}"))?;
                let root = match a.root {
                    Some(r) => Some(
                        Location::parse_os(r.as_os_str())
                            .path()
                            .ok_or_else(|| anyhow::anyhow!("root must be a path"))?,
                    ),
                    // A pane showing another query searches from home.
                    None => app.pane_dir(pane).ok(),
                };
                app.navigate(pane, &location_for(&a.expr, root.as_deref()))?;
                Ok(Outcome::Done)
            },
        );

        let saving = Arc::clone(&self.saving);
        app.commands.register::<SaveArgs, _>(
            ID,
            "query.save",
            "Save query",
            "Keep a query as a smart folder in the sidebar",
            move |a, app| {
                lang::parse(&a.expr).map_err(|e| anyhow::anyhow!("query: {e}"))?;
                edit_saved(app, &saving, |list| {
                    list.retain(|s| s.name != a.name);
                    list.push(SavedQuery {
                        name: a.name,
                        expr: a.expr,
                    });
                    Ok(())
                })?;
                Ok(Outcome::Done)
            },
        );

        let saving = Arc::clone(&self.saving);
        app.commands.register::<ForgetArgs, _>(
            ID,
            "query.forget",
            "Forget saved query",
            "Remove a saved query from the sidebar",
            move |a, app| {
                edit_saved(app, &saving, |list| {
                    let before = list.len();
                    list.retain(|s| s.name != a.name);
                    anyhow::ensure!(list.len() != before, "no saved query named `{}`", a.name);
                    Ok(())
                })?;
                Ok(Outcome::Done)
            },
        );

        app.commands.register::<NoArgs, _>(
            ID,
            "query.list",
            "List saved queries",
            "Return the saved queries",
            |_, app| Ok(Outcome::Value(serde_json::to_value(saved(app))?)),
        );

        app.registry.keymap.add(
            ID,
            KeyBinding {
                keys: "ctrl-f".into(),
                command: Invocation::new("ui.search"),
                when: None,
            },
        );
        app.registry.menus.add(
            ID,
            MenuItem {
                menu: "context".into(),
                title: "Search here…".into(),
                command: Invocation::new("ui.search"),
                when: Some("empty".into()),
                order: 420,
                icon: Some("search".into()),
                submenu: None,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bibli_core::app::AppConfig;
    use bibli_core::location::ListingEvent;
    use std::collections::BTreeSet;
    use std::path::Path;
    use std::time::{Duration, SystemTime};

    #[test]
    fn generated_locations_preserve_literal_roots() {
        for root in [
            "/images/2026-08 Trip d'Example - Summit",
            "/images/d'Example",
            "/images/two words",
            "/images/(trip)",
            "/images/\"trip\"",
            "/images/back\\slash",
            "/images/back\\slash with space\\",
            "/images/tab\there",
            "/images/new\nline",
            "/images/unicode\u{2003}space",
        ] {
            let location = location_for("type:file", Some(Path::new(root)));
            let parsed = lang::parse(&location.rest).unwrap();
            assert_eq!(parsed.root, Path::new(root));
            // The canonical query printer must preserve the same root too.
            assert_eq!(lang::parse(&parsed.to_string()).unwrap().root, parsed.root);
        }
    }

    /// A tree with varied names, sizes, mtimes, contents, and a hidden dir.
    fn tree(root: &Path) {
        std::fs::create_dir_all(root.join("docs/deep/deeper")).unwrap();
        std::fs::create_dir_all(root.join(".hidden")).unwrap();
        std::fs::create_dir_all(root.join("pics")).unwrap();
        std::fs::write(root.join("docs/report.pdf"), vec![0u8; 3000]).unwrap();
        std::fs::write(root.join("docs/notes.txt"), "hello world\nneedle here\n").unwrap();
        std::fs::write(root.join("docs/deep/old.txt"), "ancient").unwrap();
        std::fs::write(root.join("docs/deep/deeper/bottom.md"), "x").unwrap();
        std::fs::write(root.join("pics/cat.png"), vec![1u8; 20_000]).unwrap();
        std::fs::write(root.join("pics/dog.JPG"), vec![1u8; 10]).unwrap();
        std::fs::write(root.join(".hidden/secret.txt"), "needle").unwrap();
        std::fs::write(root.join("binary.dat"), b"nee\x00dle").unwrap();
        let old = SystemTime::now() - Duration::from_secs(30 * 86_400);
        std::fs::File::options()
            .write(true)
            .open(root.join("docs/deep/old.txt"))
            .unwrap()
            .set_modified(old)
            .unwrap();
    }

    fn run(app: &App, expr: &str, root: &Path) -> BTreeSet<String> {
        let loc = location_for(expr, Some(root));
        let open = app
            .open_location(&loc)
            .unwrap_or_else(|e| panic!("{expr}: {e:#}"));
        let mut names = BTreeSet::new();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            match open
                .events()
                .recv_deadline(deadline)
                .expect("query completes")
            {
                ListingEvent::Added(batch) | ListingEvent::Updated(batch) => {
                    for e in batch {
                        let p = open.path_of(e.id).expect("path_of");
                        names.insert(p.strip_prefix(root).unwrap().to_string_lossy().into_owned());
                    }
                }
                ListingEvent::Complete => break,
                ListingEvent::Error(e) => panic!("{expr}: {e}"),
                ListingEvent::Removed(_) => {}
            }
        }
        names
    }

    fn set(v: &[&str]) -> BTreeSet<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn queries_over_a_tree() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("t");
        std::fs::create_dir(&root).unwrap();
        tree(&root);
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(QueryPlugin::default()), &app);

        assert_eq!(
            run(&app, "*.txt", &root),
            set(&["docs/notes.txt", "docs/deep/old.txt"])
        );
        assert_eq!(
            run(&app, "*.txt hidden:true", &root),
            set(&["docs/notes.txt", "docs/deep/old.txt", ".hidden/secret.txt"])
        );
        assert_eq!(
            run(&app, "ext:png,jpg", &root),
            set(&["pics/cat.png", "pics/dog.JPG"])
        );
        assert_eq!(
            run(&app, "type:dir", &root),
            set(&["docs", "docs/deep", "docs/deep/deeper", "pics"])
        );
        assert_eq!(run(&app, "type:dir depth:1", &root), set(&["docs", "pics"]));
        assert_eq!(
            run(&app, "size>1k", &root),
            set(&["docs/report.pdf", "pics/cat.png"])
        );
        assert_eq!(
            run(&app, "size<100 type:file", &root),
            set(&[
                "docs/notes.txt",
                "docs/deep/old.txt",
                "docs/deep/deeper/bottom.md",
                "pics/dog.JPG",
                "binary.dat"
            ])
        );
        assert_eq!(run(&app, "modified>7d", &root), set(&["docs/deep/old.txt"]));
        assert_eq!(
            run(&app, "*.txt modified<7d", &root),
            set(&["docs/notes.txt"])
        );
        assert_eq!(run(&app, "content:needle", &root), set(&["docs/notes.txt"]));
        assert_eq!(
            run(&app, "content:/^hello/", &root),
            set(&["docs/notes.txt"])
        );
        assert_eq!(
            run(&app, "NOT type:dir -ext:txt depth:2", &root),
            set(&[
                "docs/report.pdf",
                "pics/cat.png",
                "pics/dog.JPG",
                "binary.dat"
            ])
        );
        assert_eq!(
            run(&app, "cat OR dog", &root),
            set(&["pics/cat.png", "pics/dog.JPG"])
        );
        assert_eq!(
            run(&app, "mime:image/*", &root),
            set(&["pics/cat.png", "pics/dog.JPG"])
        );
        assert_eq!(
            run(&app, "(report OR notes) ext:txt", &root),
            set(&["docs/notes.txt"])
        );
        // `in:` inside the expression wins over the root argument; a relative
        // one cannot be resolved and the location refuses to open.
        assert!(
            app.open_location(&location_for("in:docs/deep", Some(&root)))
                .is_err()
        );
        assert_eq!(
            run(
                &app,
                &format!("in:{} ext:md", root.join("docs").display()),
                &root
            ),
            set(&["docs/deep/deeper/bottom.md"])
        );
    }

    #[test]
    fn hydrate_updates_and_removed_on_vanish() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("t");
        std::fs::create_dir(&root).unwrap();
        tree(&root);
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(QueryPlugin::default()), &app);
        let open = app
            .open_location(&location_for("*.png", Some(&root)))
            .unwrap();
        let mut listing = bibli_core::Listing::default();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !listing.is_complete() {
            listing.apply(open.events().recv_deadline(deadline).unwrap());
        }
        assert_eq!(listing.len(), 1);
        let id = listing.ids().next().unwrap();
        assert!(
            listing.get(id).unwrap().metadata.is_hydrated(),
            "matches are stat-ed for identity"
        );
        std::fs::remove_file(root.join("pics/cat.png")).unwrap();
        open.hydrate(&[id], bibli_core::scheduler::Priority::Visible);
        loop {
            let ev = open.events().recv_deadline(deadline).unwrap();
            let removed = matches!(ev, ListingEvent::Removed(_));
            listing.apply(ev);
            if removed {
                break;
            }
        }
        assert!(listing.is_empty());
    }

    #[test]
    fn saved_queries_become_places_and_disable_withdraws_everything() {
        let tmp = tempfile::tempdir().unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(QueryPlugin::default()), &app);
        assert!(app.provider(SCHEME).is_some());
        app.dispatch(&Invocation::with_args(
            "query.save",
            &serde_json::json!({"name": "Big PDFs", "expr": "ext:pdf size>10M"}),
        ))
        .unwrap();
        let places = app.registry.places.all();
        assert_eq!(places.len(), 1);
        assert_eq!(places[0].group, "queries");
        assert_eq!(places[0].location.to_string(), "query:ext:pdf size>10M");
        assert!(
            app.dispatch(&Invocation::with_args(
                "query.save",
                &serde_json::json!({"name": "bad", "expr": "(x"})
            ))
            .is_err()
        );
        app.dispatch(&Invocation::with_args(
            "query.forget",
            &serde_json::json!({"name": "Big PDFs"}),
        ))
        .unwrap();
        assert!(app.registry.places.all().is_empty());
        assert!(
            app.dispatch(&Invocation::with_args(
                "query.forget",
                &serde_json::json!({"name": "Big PDFs"})
            ))
            .is_err()
        );
        app.dispatch(&Invocation::with_args(
            "query.save",
            &serde_json::json!({"name": "Recent", "expr": "modified<7d"}),
        ))
        .unwrap();
        assert!(app.registry.keymap.all().iter().any(|b| b.keys == "ctrl-f"));
        assert!(app.plugins.set_enabled(ID, false, &app));
        assert!(app.provider(SCHEME).is_none());
        assert!(app.commands.get("query.search").is_none());
        assert!(app.registry.places.all().is_empty());
        assert!(app.registry.keymap.all().is_empty());
        assert!(app.registry.menus.all().is_empty());
        assert!(
            app.settings
                .schema()
                .iter()
                .all(|s| !s.key.starts_with("query."))
        );
    }

    #[test]
    fn search_command_navigates_the_pane() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("t");
        std::fs::create_dir(&root).unwrap();
        tree(&root);
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(QueryPlugin::default()), &app);
        let pane = app.workspace.read().active_id();
        app.navigate(pane, &Location::from_path(&root)).unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
        app.dispatch(&Invocation::with_args(
            "query.search",
            &serde_json::json!({"expr": "ext:pdf"}),
        ))
        .unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(10)));
        let ws = app.workspace.read();
        let p = ws.pane(pane).unwrap();
        assert_eq!(p.location().unwrap().scheme.as_ref(), SCHEME);
        assert_eq!(p.listing.len(), 1);
        assert_eq!(p.listing.iter().next().unwrap().name_str(), "report.pdf");
    }

    #[test]
    fn plocate_is_used_for_name_only_queries_over_root() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let hit = tmp.path().join("hit-report.txt");
        std::fs::write(&hit, "x").unwrap();
        let script = format!(
            "#!/bin/sh\nprintf '%s\\0%s\\0' '{}' '/nonexistent/report.txt'\n",
            hit.display()
        );
        std::fs::write(bin.join("plocate"), script).unwrap();
        std::fs::set_permissions(bin.join("plocate"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
        let q = lang::parse("report in:/").unwrap();
        let paths =
            plocate::locate_with(&bin.join("plocate"), &q.name_substrings().unwrap(), false)
                .unwrap();
        assert_eq!(
            paths,
            vec![
                hit.clone(),
                std::path::PathBuf::from("/nonexistent/report.txt")
            ]
        );
        assert!(
            plocate::try_locate(&lang::parse("report in:/tmp").unwrap()).is_none(),
            "only over /"
        );
        assert!(
            plocate::try_locate(&lang::parse("report size>1k in:/").unwrap()).is_none(),
            "only name-only"
        );
        // A database that cannot answer (it says why) is not "no matches".
        std::fs::write(
            bin.join("broken"),
            "#!/bin/sh\necho 'no database' >&2\nexit 1\n",
        )
        .unwrap();
        std::fs::set_permissions(bin.join("broken"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
        assert!(plocate::locate_with(&bin.join("broken"), &["x".into()], false).is_none());
        std::fs::write(bin.join("empty"), "#!/bin/sh\nexit 1\n").unwrap();
        std::fs::set_permissions(bin.join("empty"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
        assert_eq!(
            plocate::locate_with(&bin.join("empty"), &["x".into()], false),
            Some(Vec::new())
        );
    }

    #[test]
    fn a_folder_whose_name_is_not_utf8_can_be_searched() {
        use std::os::unix::ffi::OsStrExt;
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join(std::ffi::OsStr::from_bytes(b"caf\xe9 (x)"));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("a.pdf"), b"x").unwrap();
        std::fs::write(root.join("b.txt"), b"x").unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
        app.plugins.add(Arc::new(QueryPlugin::default()), &app);
        let pane = app.workspace.read().active_id();
        app.dispatch(&Invocation::with_args(
            "query.search",
            &serde_json::json!({"expr": "ext:pdf", "root": PathArg::from(root.as_path())}),
        ))
        .unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(10)));
        let ws = app.workspace.read();
        let p = ws.pane(pane).unwrap();
        assert_eq!(lang::parse(&p.location().unwrap().rest).unwrap().root, root);
        assert_eq!(p.listing.len(), 1);
        assert_eq!(p.listing.iter().next().unwrap().name_str(), "a.pdf");
    }

    #[test]
    fn a_root_that_is_not_a_folder_is_a_listing_error() {
        let tmp = tempfile::tempdir().unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(QueryPlugin::default()), &app);
        let open = app
            .open_location(&location_for("x", Some(&tmp.path().join("missing"))))
            .expect("opening does no I/O");
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        match open.events().recv_deadline(deadline).unwrap() {
            ListingEvent::Error(e) => assert!(e.contains("not a folder"), "{e}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn location_for_reads_in_like_the_parser() {
        let root = Path::new("/r");
        // A quoted `in:` is part of a name: the root is still added.
        let l = location_for("name:\"a in:b\"", Some(root));
        assert_eq!(lang::parse(&l.rest).unwrap().root, root);
        let l = location_for("x in:/other", Some(root));
        assert_eq!(lang::parse(&l.rest).unwrap().root, Path::new("/other"));
    }

    #[test]
    fn tags_are_matched_in_batches_per_folder() {
        use bibli_core::entry::{Entry, Provenance, Value, keys};
        use bibli_core::metadata::MetadataProvider;
        /// Tags every `*.tagged` file, counting calls and entries.
        #[derive(Default)]
        struct Fake(parking_lot::Mutex<(usize, usize)>);
        impl MetadataProvider for Fake {
            fn id(&self) -> &'static str {
                "fake"
            }
            fn fill(&self, _: &Path, entries: &mut [Entry]) {
                let mut n = self.0.lock();
                n.0 += 1;
                n.1 += entries.len();
                for e in entries {
                    if e.name_str().ends_with(".tagged") {
                        e.metadata.set(
                            keys::TAGS,
                            Value::List(vec![Value::Text("Work".into())]),
                            Provenance::User,
                        );
                    }
                }
            }
        }
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("t");
        std::fs::create_dir(&root).unwrap();
        for i in 0..100 {
            let ext = if i % 10 == 0 { "tagged" } else { "plain" };
            std::fs::write(root.join(format!("f{i}.{ext}")), b"x").unwrap();
        }
        let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
        let fake = Arc::new(Fake::default());
        app.metadata.register("fake", fake.clone());
        app.plugins.add(Arc::new(QueryPlugin::default()), &app);
        assert_eq!(run(&app, "tag:work", &root).len(), 10);
        let (calls, entries) = *fake.0.lock();
        assert_eq!(entries, 100);
        assert!(calls <= 10, "{calls} provider calls for 100 files");
    }

    #[test]
    fn disabling_the_plugin_stops_its_walks() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("t");
        std::fs::create_dir(&root).unwrap();
        tree(&root);
        let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
        app.plugins.add(Arc::new(QueryPlugin::default()), &app);
        let open = app
            .open_location(&location_for("txt", Some(&root)))
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !matches!(
            open.events().recv_deadline(deadline).unwrap(),
            ListingEvent::Complete
        ) {}
        assert!(app.plugins.set_enabled(ID, false, &app));
        // A location that outlives the plugin walks no more.
        open.refresh();
        let deadline = std::time::Instant::now() + Duration::from_millis(300);
        while let Ok(ev) = open.events().recv_deadline(deadline) {
            assert!(
                !matches!(ev, ListingEvent::Complete | ListingEvent::Added(_)),
                "walked after disable: {ev:?}"
            );
        }
    }
}
