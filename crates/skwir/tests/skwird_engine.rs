//! skwird's plugin against a real engine (ADR-150, 11): `just
//! skwird-check` runs these, `SKWIRD` naming skwird's program; ignored
//! otherwise, as no engine is installed with Skwir. Each test starts an
//! engine of its own on a fixture (`skwird serve --root`), its five folders
//! (runtime, cache, state, config, home) the test's, under
//! `~/.cache/skwir-tests`, and asks it through the plugin and the query
//! plugin, as a search does. Sockets: Unix's.
#![cfg(unix)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use skwir_core::app::{App, AppConfig};
use skwir_core::coverage::Proof;
use skwir_core::location::{ListingEvent, RestsOn};

/// An engine of skwird's, serving `root` alone, stopped when dropped.
struct Engine {
    child: Child,
    dir: tempfile::TempDir,
}

impl Engine {
    /// Started with `exclude` in its user's exclusions file.
    fn start(root: &Path, exclude: &[&Path]) -> Engine {
        let skwird = std::env::var_os("SKWIRD")
            .expect("SKWIRD names skwird's program: `just skwird-check` sets it");
        let base = dirs::home_dir().unwrap().join(".cache").join("skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        // Short: a socket's path has room for 108 bytes.
        let dir = tempfile::Builder::new()
            .prefix("skwird-")
            .tempdir_in(base)
            .unwrap();
        let at = |name: &str| dir.path().join(name);
        for name in ["home", "cache", "state", "config/skwird"] {
            std::fs::create_dir_all(at(name)).unwrap();
        }
        std::fs::create_dir(at("rt")).unwrap();
        std::fs::set_permissions(
            at("rt"),
            std::os::unix::fs::PermissionsExt::from_mode(0o700),
        )
        .unwrap();
        let excluded: Vec<String> = exclude.iter().map(|p| p.display().to_string()).collect();
        std::fs::write(at("config/skwird/exclude"), excluded.join("\n")).unwrap();
        // The test's own program, never one Skwir starts.
        #[allow(clippy::disallowed_methods, reason = "a test's own program")]
        let child = std::process::Command::new(skwird)
            .args(["serve", "--root"])
            .arg(root)
            .args(["--idle", "120"])
            .env("HOME", at("home"))
            .env("XDG_RUNTIME_DIR", at("rt"))
            .env("XDG_CACHE_HOME", at("cache"))
            .env("XDG_STATE_HOME", at("state"))
            .env("XDG_CONFIG_HOME", at("config"))
            .env("SKWIRD_NO_START", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("skwird starts");
        let engine = Engine { child, dir };
        let deadline = Instant::now() + Duration::from_secs(30);
        // Until its root is read: a root being read is answered "not
        // covered: unread", which a search walks.
        loop {
            if let Ok(mut client) = skwird_client::Client::at(&engine.socket(), "skwir-test")
                && client.coverage(root).is_ok_and(|c| {
                    !matches!(
                        c.names,
                        skwird_client::protocol::Proof::NotCovered(
                            skwird_client::protocol::Why::Unread
                        )
                    )
                })
            {
                return engine;
            }
            assert!(Instant::now() < deadline, "the engine read its root");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn socket(&self) -> PathBuf {
        self.dir.path().join("rt/skwird/socket")
    }

    /// Keep `folder` live, as a client of skwird's own would, until skwird
    /// says it is.
    fn watch(&self, folder: &Path) {
        let mut client = skwird_client::Client::at(&self.socket(), "skwir-test").unwrap();
        client.watch(&[folder], &[]).unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        while !client
            .coverage(folder)
            .is_ok_and(|c| matches!(c.names, skwird_client::protocol::Proof::Live(_)))
        {
            assert!(Instant::now() < deadline, "skwird kept it live");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// An app searching with the query plugin and skwird's, asking `engine`.
fn app(state: &Path, engine: &Engine) -> Arc<App> {
    let app = App::new(AppConfig::ephemeral(state)).unwrap();
    app.plugins
        .add(Arc::new(skwir_plugin_query::QueryPlugin::default()), &app);
    app.plugins.add(
        Arc::new(skwir_plugin_skwird::SkwirdPlugin::at(engine.socket())),
        &app,
    );
    app
}

/// What a search found, relative to `root`, and what its answer rests on.
fn search(app: &App, expr: &str, root: &Path) -> (BTreeSet<String>, Option<RestsOn>) {
    let location = skwir_plugin_query::location_for(expr, Some(root));
    let open = app.open_location(&location).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let (mut found, mut rests_on) = (BTreeSet::new(), None);
    loop {
        match open
            .events()
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("the search completes")
        {
            ListingEvent::Added(batch) => {
                for e in batch {
                    let path = open.path_of(e.id).unwrap();
                    found.insert(path.strip_prefix(root).unwrap().display().to_string());
                }
            }
            ListingEvent::RestsOn(said) => rests_on = said,
            ListingEvent::Complete => break,
            ListingEvent::Error(e) => panic!("{e}"),
            _ => {}
        }
    }
    open.close();
    (found, rests_on)
}

fn fixture(names: &[&str]) -> (tempfile::TempDir, PathBuf) {
    let base = dirs::home_dir().unwrap().join(".cache").join("skwir-tests");
    std::fs::create_dir_all(&base).unwrap();
    let dir = tempfile::Builder::new()
        .prefix("skwird-fixture-")
        .tempdir_in(base)
        .unwrap();
    // The real path: skwird knows no other.
    let root = std::fs::canonicalize(dir.path()).unwrap().join("files");
    for name in names {
        let path = root.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "fixture").unwrap();
    }
    (dir, root)
}

fn set(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|s| (*s).to_owned()).collect()
}

/// A search answered from the engine's index, checked; a file made since
/// found once a sync proves the index current; then, kept live, live.
#[test]
#[ignore = "needs a real skwird: `just skwird-check`"]
fn a_search_is_answered_from_a_real_engine_and_proven() {
    let (_dir, root) = fixture(&["report one.txt", "docs/report two.pdf", "notes.txt"]);
    let engine = Engine::start(&root, &[]);
    let state = tempfile::tempdir().unwrap();
    let app = app(state.path(), &engine);

    let (found, rests_on) = search(&app, "report", &root);
    assert_eq!(found, set(&["report one.txt", "docs/report two.pdf"]));
    let rests_on = rests_on.expect("skwird answered");
    assert_eq!(rests_on.by, skwir_plugin_skwird::by());
    assert!(
        matches!(rests_on.proof, Proof::Checked { .. }),
        "{rests_on:?}"
    );

    std::fs::write(root.join("docs/report three.txt"), "fixture").unwrap();
    let (found, _) = search(&app, "report", &root);
    assert!(
        found.contains("docs/report three.txt"),
        "the sync proved it: {found:?}"
    );

    engine.watch(&root);
    let (found, rests_on) = search(&app, "report", &root);
    assert_eq!(found.len(), 3, "{found:?}");
    assert!(
        matches!(rests_on.map(|r| r.proof), Some(Proof::Live { .. })),
        "kept live"
    );
}

/// A folder skwird keeps out is a hole of its answer: the search walks it.
#[test]
#[ignore = "needs a real skwird: `just skwird-check`"]
fn a_folder_skwird_keeps_out_is_walked() {
    let (_dir, root) = fixture(&["report.txt", "kept out/report inside.txt"]);
    let engine = Engine::start(&root, &[&root.join("kept out")]);
    let mut client = skwird_client::Client::at(&engine.socket(), "skwir-test").unwrap();
    let holes = client.coverage(&root).unwrap().holes;
    assert!(
        holes
            .iter()
            .any(|h| h.why == skwird_client::protocol::Why::Excluded),
        "skwird says it keeps it out: {holes:?}"
    );
    let state = tempfile::tempdir().unwrap();
    let app = app(state.path(), &engine);
    let (found, rests_on) = search(&app, "report", &root);
    assert_eq!(found, set(&["report.txt", "kept out/report inside.txt"]));
    assert!(rests_on.is_some(), "answered by skwird, its hole walked");
}

/// Sizes and times are asked of a real engine where it holds them live
/// (ADR-150, 14): a dated search with no name, the root kept live, is
/// answered from its index, the date tested on what it names.
#[test]
#[ignore = "needs a real skwird: `just skwird-check`"]
fn a_dated_search_is_answered_where_a_real_engine_holds_times_live() {
    let (_dir, root) = fixture(&["new report.txt", "old report.txt", "docs/new notes.txt"]);
    for (name, secs) in [
        ("new report.txt", 1_900_000_000),
        ("old report.txt", 1_000_000_000),
        ("docs/new notes.txt", 1_900_000_000),
    ] {
        std::fs::File::options()
            .write(true)
            .open(root.join(name))
            .unwrap()
            .set_modified(std::time::UNIX_EPOCH + Duration::from_secs(secs))
            .unwrap();
    }
    let engine = Engine::start(&root, &[]);
    engine.watch(&root);
    let state = tempfile::tempdir().unwrap();
    let app = app(state.path(), &engine);
    let (found, rests_on) = search(&app, "modified>=2030-01-01 type:file", &root);
    assert_eq!(found, set(&["new report.txt", "docs/new notes.txt"]));
    assert!(
        matches!(rests_on.map(|r| r.proof), Some(Proof::Live { .. })),
        "answered from the index"
    );
}

/// A real engine's journal, followed through the plugin (ADR-150, 15),
/// tells a tag set as an attribute alone where the folder is live, says
/// it is every change, and names a folder removed by the device and inode
/// a stat says: what the tags index keys its folders by.
#[cfg(target_os = "linux")]
#[test]
#[ignore = "needs a real skwird: `just skwird-check`"]
fn a_real_engines_journal_tells_a_tag_set_where_the_folder_is_live() {
    use skwir_core::names::{Change, Since};
    use std::os::unix::fs::MetadataExt;
    let (_dir, root) = fixture(&["tagged.txt", "gone/inside.txt"]);
    let engine = Engine::start(&root, &[]);
    engine.watch(&root);
    let state = tempfile::tempdir().unwrap();
    let app = app(state.path(), &engine);
    let cancel = skwir_core::cancel::CancelToken::new();
    let (index, opened) = app.names.follow(&root, None, &cancel).expect("followed");
    assert_eq!(index.id(), skwir_plugin_skwird::ID);
    let mut session = opened.session;
    let Some(Since::Journal(now)) = session.changes(None) else {
        panic!("a mark from now");
    };
    assert!(now.changes.is_empty());
    let gone = std::fs::metadata(root.join("gone")).unwrap();
    skwir_core::fs::xattr::set(root.join("tagged.txt"), "user.xdg.tags", b"work").unwrap();
    std::fs::remove_dir_all(root.join("gone")).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let Some(Since::Journal(journal)) = session.changes(Some(&now.mark)) else {
            panic!("a journal from the mark");
        };
        let tagged = journal
            .changes
            .iter()
            .any(|c| matches!(c, Change::Changed(n) if n.path == root.join("tagged.txt")));
        let removed = journal.changes.iter().find_map(|c| match c {
            Change::Removed(n) if n.path == root.join("gone") => Some(n.id),
            _ => None,
        });
        if tagged && let Some(id) = removed {
            assert!(journal.complete(), "live all along: every change");
            assert_eq!(id, Some((gone.dev(), gone.ino())));
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the journal told the tag and the removal: {:?}",
            journal.changes
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// A real engine sums a folder through the plugin (ADR-150, D3): each
/// entry's totals, a folder's subtree's, the folder's own, dated by its
/// newest file.
#[test]
#[ignore = "needs a real skwird: `just skwird-check`"]
fn a_real_engine_sums_a_folder() {
    let (_dir, root) = fixture(&["a.txt", "docs/b.pdf", "docs/deeper/c.pdf", "empty/.keep"]);
    std::fs::write(root.join("a.txt"), vec![b'x'; 10_000]).unwrap();
    std::fs::remove_file(root.join("empty/.keep")).unwrap();
    let engine = Engine::start(&root, &[]);
    engine.watch(&root);
    let state = tempfile::tempdir().unwrap();
    let app = app(state.path(), &engine);
    let cancel = skwir_core::cancel::CancelToken::new();
    let (_, opened) = app.names.follow(&root, None, &cancel).expect("followed");
    let mut session = opened.session;
    let mut children = Vec::new();
    let summed = session
        .totals(&root, None, &mut |batch| {
            children.extend(batch);
            true
        })
        .expect("summed");
    assert_eq!((summed.totals.files, summed.totals.folders), (3, 3));
    assert_eq!(summed.totals.size, 10_000 + 2 * "fixture".len() as u64);
    let named = |name: &str| {
        children
            .iter()
            .find(|c| c.path == root.join(name))
            .unwrap_or_else(|| panic!("{name} among {children:?}"))
    };
    assert_eq!(named("a.txt").totals.size, 10_000);
    let docs = named("docs");
    assert_eq!(docs.kind, skwir_core::entry::EntryKind::Directory);
    assert_eq!((docs.totals.files, docs.totals.folders), (2, 1));
    assert!(docs.totals.newest_file.is_some());
    let empty = named("empty");
    assert!(empty.totals.newest_file.is_none() && empty.mtime.is_some());
    assert!(
        docs.totals
            .by_extension
            .iter()
            .any(|e| e.ext.as_deref() == Some(std::ffi::OsStr::new("pdf")) && e.files == 2)
    );
}

/// Usage measures a folder from a real engine's totals as its own scan of
/// the disk does (ADR-150, D3): the same bytes, files, folders and dates,
/// kinds, names and caches, folder by folder down the levels the map opens:
/// a file of two names once, one with a name outside said one of two and
/// its bytes not freed with its folders, an empty folder dated by its own
/// time, a cache inside a cache, and what is scanned inside a cache room
/// given back with it; what the engine keeps out scanned, and nothing else
/// read.
#[cfg(target_os = "linux")]
#[test]
#[ignore = "needs a real skwird: `just skwird-check`"]
fn usage_measures_from_a_real_engine_as_its_scan_does() {
    use skwir_core::command::{Invocation, Outcome};
    /// What a block says, then each of its blocks' by name, a level deeper.
    fn facts(node: &serde_json::Value, depth: usize, rows: &mut Vec<String>) {
        const SAID: [&str; 18] = [
            "size",
            "alloc",
            "files",
            "dirs",
            "modified",
            "kind",
            "entry_kind",
            "kinds",
            "reclaim",
            "reclaimable",
            "nlink",
            "shared",
            "other_count",
            "other_alloc",
            "other_size",
            "other_files",
            "other_modified",
            "incomplete",
        ];
        let said: Vec<String> = SAID
            .iter()
            .map(|key| {
                format!(
                    "{key} {}",
                    node.get(*key).unwrap_or(&serde_json::Value::Null)
                )
            })
            .collect();
        rows.push(format!(
            "{:width$}{} {}",
            "",
            node["name"],
            said.join(" "),
            width = depth * 2
        ));
        let mut children: Vec<&serde_json::Value> = node["children"]
            .as_array()
            .map(|c| c.iter().collect())
            .unwrap_or_default();
        children.sort_by_key(|c| c["name"].as_str().unwrap_or_default().to_owned());
        for child in children {
            facts(child, depth + 1, rows);
        }
    }
    let (_dir, root) = fixture(&[
        "a.txt",
        "docs/b.pdf",
        "docs/deeper/c.pdf",
        "kept out/secret.txt",
        "media/clip.mkv",
        "build/CACHEDIR.TAG",
        "build/obj/data.o",
        "build/inner/CACHEDIR.TAG",
        "build/inner/blob.bin",
        "build/private/p.bin",
    ]);
    std::fs::write(root.join("a.txt"), vec![b'x'; 10_000]).unwrap();
    std::fs::write(root.join("media/clip.mkv"), vec![b'y'; 300_000]).unwrap();
    std::fs::write(root.join("build/obj/data.o"), vec![b'o'; 40_000]).unwrap();
    std::fs::write(root.join("build/inner/blob.bin"), vec![b'i'; 30_000]).unwrap();
    std::fs::write(root.join("build/private/p.bin"), vec![b'p'; 20_000]).unwrap();
    for cache in ["build", "build/inner"] {
        std::fs::write(
            root.join(cache).join("CACHEDIR.TAG"),
            "Signature: 8a477f597d28d172789f06886806bc55\n",
        )
        .unwrap();
    }
    std::fs::hard_link(root.join("docs/b.pdf"), root.join("docs/b again.pdf")).unwrap();
    // Its other name outside the folder measured.
    std::fs::hard_link(
        root.join("media/clip.mkv"),
        root.parent().unwrap().join("clip elsewhere.mkv"),
    )
    .unwrap();
    std::fs::create_dir(root.join("empty")).unwrap();
    let engine = Engine::start(
        &root,
        &[&root.join("kept out"), &root.join("build/private")],
    );
    engine.watch(&root);
    let measured = |with_index: bool| {
        let state = tempfile::tempdir().unwrap();
        let app = if with_index {
            app(state.path(), &engine)
        } else {
            App::new(AppConfig::ephemeral(state.path())).unwrap()
        };
        app.plugins
            .add(Arc::new(skwir_plugin_usage::UsagePlugin::default()), &app);
        let pane = app.workspace.read().active_id();
        app.dispatch(&Invocation::with_args(
            "usage.scan",
            &serde_json::json!({"pane": pane, "path": root}),
        ))
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let Outcome::Value(status) = app
                .dispatch(&Invocation::with_args(
                    "usage.status",
                    &serde_json::json!({"pane": pane}),
                ))
                .unwrap()
            else {
                panic!("usage.status answers a value");
            };
            if status["scanning"] == false && status["tree"].is_object() {
                return status;
            }
            assert!(Instant::now() < deadline, "measured: {status}");
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    let said = |status: &serde_json::Value| {
        let mut rows = Vec::new();
        facts(&status["tree"], 0, &mut rows);
        rows
    };
    let scanned = measured(false);
    let indexed = measured(true);
    assert_eq!(said(&indexed), said(&scanned));
    let tree = &indexed["tree"];
    let named = |node: &serde_json::Value, name: &str| -> serde_json::Value {
        node["children"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == name)
            .unwrap_or_else(|| panic!("{name} in {node}"))
            .clone()
    };
    // What the comparison holds, said of the index's.
    let media = named(tree, "media");
    assert_eq!(named(&media, "clip.mkv")["nlink"], 2);
    assert_eq!(media["shared"], named(&media, "clip.mkv")["alloc"]);
    let build = named(tree, "build");
    assert_eq!(build["reclaim"], "cache");
    assert_eq!(build["reclaimable"], build["alloc"], "private's room too");
    assert_eq!(named(&build, "inner")["reclaim"], "cache");
    assert_eq!(named(&build, "private")["size"], 20_000);
    assert_eq!(
        indexed["scanned_dirs"], 2,
        "the folders kept out alone, one inside the cache"
    );
    assert!(scanned["scanned_dirs"].as_u64().unwrap() >= 10);
    assert_eq!(indexed["stale"], false);
}
