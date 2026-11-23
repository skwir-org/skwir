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
