//! Cached disk usage, owned by a lazy worker. Commands never perform filesystem I/O.
//! Directory breakdowns are cached separately from the treemap presentation;
//! per-directory watches invalidate changed branches and their ancestors.
//! Statuses live only as long as their request: navigation, pane close and
//! `usage.release` drop them.

mod worker;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use bibli_core::app::{App, PaneArg};
use bibli_core::cancel::CancelToken;
use bibli_core::command::{Invocation, Outcome};
use bibli_core::location::Location;
use bibli_core::os_path::PathArg;
use bibli_core::plugin::Plugin;
use bibli_core::registry::ViewSpec;
use bibli_core::workspace::PaneId;
use parking_lot::Mutex;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const ID: &str = "usage";

/// One directory breakdown. Children are shallow; their own breakdowns are cached separately.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct Node {
    pub id: Option<bibli_core::EntryId>,
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    /// Totals are lower bounds while scanning or when entries could not be read.
    #[serde(default)]
    pub incomplete: bool,
    pub size: u64,
    /// Allocated bytes reported by stat (not exclusive physical ownership of shared extents).
    pub alloc: u64,
    pub files: u64,
    pub dirs: u64,
    pub children: Vec<Node>,
    pub other_alloc: u64,
    pub other_size: u64,
    pub other_count: u64,
    #[serde(default)]
    pub other_incomplete: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct Status {
    pub root: Option<PathBuf>,
    pub scanning: bool,
    /// Delayed progress indication; quick cache lookups and scans stay quiet.
    pub updating: bool,
    pub files: u64,
    pub dirs: u64,
    pub alloc: u64,
    pub size: u64,
    pub elapsed_ms: u64,
    pub tree: Option<Node>,
    pub error: Option<String>,
    /// True when recursive monitoring is unavailable; periodic revalidation is used.
    pub stale: bool,
    /// Directories actually read for this request (zero on a cache hit).
    pub scanned_dirs: u64,
    pub generation: u64,
    /// A finished scan supplied the displayed tree. This is separate from
    /// `Node::incomplete`: an unreadable subtree can leave a finished result partial.
    #[serde(skip)]
    snapshot: SnapshotKind,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum SnapshotKind {
    #[default]
    Progressive,
    Finished,
}

struct Request {
    pane: PaneId,
    path: PathBuf,
    force: bool,
    generation: u64,
    cancel: CancelToken,
}

enum Message {
    Scan(Request),
    Release(PaneId),
    Changed,
}

struct Shared {
    statuses: Mutex<HashMap<PaneId, Status>>,
    revision: Arc<AtomicU64>,
    wake: Arc<dyn Fn() + Send + Sync>,
}
impl Shared {
    fn changed(&self) {
        self.revision.fetch_add(1, Ordering::Release);
        (self.wake)();
    }
}

struct Runtime {
    tx: crossbeam_channel::Sender<Message>,
    shared: Arc<Shared>,
    cancel: CancelToken,
    requests: HashMap<PaneId, CancelToken>,
    generation: u64,
}
impl Drop for Runtime {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

#[derive(Default)]
pub struct UsagePlugin {
    runtime: Arc<Mutex<Option<Runtime>>>,
    revision: Arc<AtomicU64>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct ScanArgs {
    /// The folder; the pane's own when omitted.
    pub path: Option<PathArg>,
    pub pane: Option<PaneId>,
    /// Ignore cached sizes in this subtree (manual Refresh).
    #[serde(default)]
    pub force: bool,
}

impl UsagePlugin {
    fn start(&self, app: &App, pane: PaneId, path: PathBuf, force: bool) -> anyhow::Result<()> {
        let (location_lifetime, open_panes) = {
            let ws = app.workspace.read();
            let lifetime = ws
                .pane(pane)
                .ok_or_else(|| anyhow::anyhow!("no such pane"))?
                .location_lifetime();
            let open: HashSet<PaneId> = ws.panes().iter().map(|p| p.id).collect();
            (lifetime, open)
        };
        let mut runtime = self.runtime.lock();
        if runtime.is_none() {
            let (tx, rx) = crossbeam_channel::unbounded();
            let shared = Arc::new(Shared {
                statuses: Mutex::new(HashMap::new()),
                revision: Arc::clone(&self.revision),
                wake: app.waker_handle(),
            });
            let cancel = app.lifetime().child();
            let worker_shared = Arc::clone(&shared);
            let worker_cancel = cancel.clone();
            let worker_tx = tx.clone();
            std::thread::Builder::new()
                .name("bibli-usage".into())
                .spawn(move || {
                    worker::run(&rx, worker_tx, worker_shared, &worker_cancel);
                })?;
            *runtime = Some(Runtime {
                tx,
                shared,
                cancel,
                requests: HashMap::new(),
                generation: 0,
            });
        }
        let r = runtime.as_mut().expect("runtime started");
        let cancel = r.cancel.linked(&location_lifetime);
        r.requests.retain(|_, token| !token.is_cancelled());
        if let Some(previous) = r.requests.insert(pane, cancel.clone()) {
            previous.cancel();
        }
        r.generation += 1;
        let generation = r.generation;
        {
            let mut statuses = r.shared.statuses.lock();
            // A pane closed after `usage.cancel` left its status behind.
            statuses.retain(|p, _| open_panes.contains(p));
            let s = statuses.entry(pane).or_default();
            if s.root.as_ref() != Some(&path) {
                *s = Status::default();
            }
            s.root = Some(path.clone());
            s.scanning = true;
            s.updating = false;
            s.generation = generation;
        }
        r.tx.send(Message::Scan(Request {
            pane,
            path,
            force,
            generation,
            cancel,
        }))?;
        let shared = Arc::clone(&r.shared);
        drop(runtime);
        shared.changed();
        Ok(())
    }
}

impl Plugin for UsagePlugin {
    fn id(&self) -> &'static str {
        ID
    }
    fn title(&self) -> &'static str {
        "Disk usage"
    }
    fn description(&self) -> &'static str {
        "Browse folder sizes in a treemap with live updates"
    }
    fn category(&self) -> bibli_core::plugin::Category {
        bibli_core::plugin::Category::Browsing
    }
    fn register(&self, app: &App) {
        app.registry.views.add(
            ID,
            ViewSpec::named(ID, "Usage", "usage", Arc::clone(&self.revision)),
        );
        let this = Arc::new(Self {
            runtime: Arc::clone(&self.runtime),
            revision: Arc::clone(&self.revision),
        });
        let t = Arc::clone(&this);
        app.commands.register::<ScanArgs, _>(
            ID,
            "usage.scan",
            "Scan disk usage",
            "Show cached directory sizes, computing missing or changed branches in the background",
            move |a, app| {
                let pane = app.pane_or_active(a.pane);
                let path = match a.path {
                    Some(path) => Location::parse_os(path.as_os_str()).path(),
                    None => app.pane_dir(pane).ok(),
                }
                .ok_or_else(|| anyhow::anyhow!("disk usage needs a filesystem directory"))?;
                t.start(app, pane, path, a.force)?;
                Ok(Outcome::Done)
            },
        );
        let t = Arc::clone(&this);
        app.commands.register::<PaneArg, _>(
            ID,
            "usage.status",
            "Disk usage status",
            "The selected pane's latest size breakdown",
            move |a, app| {
                let pane = app.pane_or_active(a.pane);
                let status = t
                    .runtime
                    .lock()
                    .as_ref()
                    .filter(|r| !r.requests.get(&pane).is_some_and(CancelToken::is_cancelled))
                    .and_then(|r| r.shared.statuses.lock().get(&pane).cloned())
                    .unwrap_or_default();
                Ok(Outcome::Value(serde_json::to_value(status)?))
            },
        );
        for (name, title, release) in [
            ("usage.cancel", "Cancel disk usage scan", false),
            ("usage.release", "Release disk usage view", true),
        ] {
            let t = Arc::clone(&this);
            app.commands
                .register::<PaneArg, _>(ID, name, title, title, move |a, app| {
                    let pane = app.pane_or_active(a.pane);
                    let shared = {
                        let mut runtime = t.runtime.lock();
                        if let Some(r) = runtime.as_mut() {
                            if let Some(cancel) = r.requests.remove(&pane) {
                                cancel.cancel();
                            }
                            if release {
                                r.shared.statuses.lock().remove(&pane);
                            } else if let Some(s) = r.shared.statuses.lock().get_mut(&pane) {
                                s.scanning = false;
                                s.updating = false;
                                s.error = Some("Scan cancelled".into());
                            }
                            let _ = r.tx.send(Message::Release(pane));
                            Some(Arc::clone(&r.shared))
                        } else {
                            None
                        }
                    };
                    if let Some(shared) = shared {
                        shared.changed();
                    }
                    Ok(Outcome::Done)
                });
        }
        app.registry.keymap.add(
            ID,
            bibli_core::registry::KeyBinding {
                keys: "ctrl-shift-u".into(),
                command: Invocation::with_args("ui.view", &serde_json::json!({"id": ID})),
                when: None,
            },
        );
        app.registry.keymap.add(
            ID,
            bibli_core::registry::KeyBinding {
                keys: "ctrl-r".into(),
                command: Invocation::new("ui.usage_refresh"),
                when: Some("pane usage".into()),
            },
        );
        app.registry.menus.add(
            ID,
            bibli_core::registry::MenuItem {
                menu: "context".into(),
                title: "Usage view".into(),
                command: Invocation::with_args("ui.view", &serde_json::json!({"id": ID})),
                when: Some("empty folder".into()),
                order: 430,
                icon: Some("usage".into()),
                submenu: None,
            },
        );
    }
    fn on_disable(&self, app: &App) {
        self.runtime.lock().take();
        self.revision.fetch_add(1, Ordering::Release);
        app.wake();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bibli_core::app::AppConfig;
    use std::path::Path;
    use std::time::{Duration, Instant};

    fn status(app: &App, pane: PaneId) -> Status {
        let Outcome::Value(value) = app
            .dispatch(&Invocation::with_args(
                "usage.status",
                &serde_json::json!({"pane":pane}),
            ))
            .unwrap()
        else {
            panic!()
        };
        serde_json::from_value(value).unwrap()
    }
    fn wait(app: &App, pane: PaneId, accept: impl Fn(&Status) -> bool) -> Status {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let s = status(app, pane);
            if !s.scanning && accept(&s) {
                return s;
            }
            assert!(Instant::now() < deadline, "usage did not converge: {s:?}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn scan(app: &App, pane: PaneId, path: &Path, force: bool) -> Status {
        app.dispatch(&Invocation::with_args(
            "usage.scan",
            &serde_json::json!({"pane":pane,"path":path,"force":force}),
        ))
        .unwrap();
        wait(app, pane, |s| s.root.as_deref() == Some(path))
    }
    fn setup() -> (tempfile::TempDir, Arc<App>, Arc<UsagePlugin>, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("Example's été");
        std::fs::create_dir(&root).unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path().join("config"))).unwrap();
        let plugin = Arc::new(UsagePlugin::default());
        app.plugins.add(plugin.clone(), &app);
        (tmp, app, plugin, root)
    }
    #[test]
    fn navigation_back_and_pane_close_cancel_only_their_location_requests() {
        let (_tmp, app, plugin, root) = setup();
        let next = root.join("Next folder");
        std::fs::create_dir(&next).unwrap();
        let pane = app.workspace.read().active_id();
        let other = app.workspace.write().add_pane();
        let scan_token = |pane| {
            app.dispatch(&Invocation::with_args(
                "usage.scan",
                &serde_json::json!({"path":root,"pane":pane}),
            ))
            .unwrap();
            plugin.runtime.lock().as_ref().unwrap().requests[&pane].clone()
        };
        app.navigate(pane, &Location::from_path(&root)).unwrap();
        let first = scan_token(pane);
        let sibling = scan_token(other);
        app.dispatch(&Invocation::with_args(
            "navigate",
            &serde_json::json!({"location":next,"pane":pane}),
        ))
        .unwrap();
        assert!(first.is_cancelled());
        assert!(!sibling.is_cancelled());
        assert!(!status(&app, pane).scanning);
        let second = scan_token(pane);
        app.dispatch(&Invocation::with_args(
            "back",
            &serde_json::json!({"pane":pane}),
        ))
        .unwrap();
        assert!(second.is_cancelled());
        let third = scan_token(pane);
        assert!(app.workspace.write().close_pane(pane));
        assert!(third.is_cancelled());
        assert!(!sibling.is_cancelled());
    }

    #[test]
    fn navigation_reuses_deep_breakdowns_and_isolates_panes() {
        let (_tmp, app, _, root) = setup();
        let deep = root.join("one/two/three/four/five/six");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(deep.join("file"), b"hello").unwrap();
        for i in 0..70 {
            std::fs::write(root.join(format!("empty-{i}")), []).unwrap();
        }
        let a = app.workspace.read().active_id();
        let b = app.workspace.write().add_pane();
        let first = scan(&app, a, &root, false);
        assert_eq!(first.size, 5);
        assert_eq!(first.tree.as_ref().unwrap().children.len(), 64);
        assert_eq!(first.tree.as_ref().unwrap().other_count, 7);
        assert!(first.scanned_dirs >= 7);
        let child = scan(&app, a, &deep, false);
        assert_eq!(child.size, 5);
        assert_eq!(child.scanned_dirs, 0);
        let sibling = scan(&app, b, &root, false);
        assert_eq!(sibling.scanned_dirs, 0);
        assert_eq!(status(&app, a).root, Some(deep));
        assert_eq!(scan(&app, a, &root, false).scanned_dirs, 0);
        assert!(scan(&app, a, &root, true).scanned_dirs >= 7);
    }
    #[test]
    fn recursive_changes_refresh_ancestors_without_rescanning_unchanged_siblings() {
        let (_tmp, app, _, root) = setup();
        std::fs::create_dir_all(root.join("left/deep")).unwrap();
        std::fs::create_dir_all(root.join("right/deep")).unwrap();
        std::fs::write(root.join("left/deep/file"), b"old").unwrap();
        std::fs::write(root.join("right/deep/file"), b"unchanged").unwrap();
        let pane = app.workspace.read().active_id();
        assert_eq!(scan(&app, pane, &root, false).size, 12);
        std::fs::write(root.join("left/deep/file"), b"new content").unwrap();
        let updated = wait(&app, pane, |s| s.size == 20);
        assert!(!updated.stale);
        assert!(updated.scanned_dirs <= 3);
        assert_eq!(
            scan(&app, pane, &root.join("right/deep"), false).scanned_dirs,
            0
        );
        scan(&app, pane, &root, false);
        std::fs::create_dir_all(root.join("new/nested")).unwrap();
        std::fs::write(root.join("new/nested/file"), b"addition").unwrap();
        wait(&app, pane, |s| s.size == 28);
        std::fs::rename(root.join("new"), root.join("renamed")).unwrap();
        wait(&app, pane, |s| {
            s.tree
                .as_ref()
                .is_some_and(|n| n.children.iter().any(|c| c.name == "renamed"))
        });
        std::fs::remove_dir_all(root.join("renamed")).unwrap();
        wait(&app, pane, |s| s.size == 20);
    }
    #[test]
    fn expanding_watch_coverage_keeps_child_updates_and_recovers_from_root_removal() {
        let (_tmp, app, _, root) = setup();
        let child = root.join("child");
        std::fs::create_dir(&child).unwrap();
        std::fs::write(child.join("file"), b"old").unwrap();
        let pane = app.workspace.read().active_id();
        assert_eq!(scan(&app, pane, &child, false).size, 3);
        assert_eq!(scan(&app, pane, &root, false).scanned_dirs, 1);
        std::fs::write(child.join("file"), b"updated").unwrap();
        wait(&app, pane, |s| s.size == 7);
        std::fs::remove_dir_all(&root).unwrap();
        wait(&app, pane, |s| s.error.is_some() && s.tree.is_none());
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("replacement"), b"replacement").unwrap();
        wait(&app, pane, |s| s.size == 11);
    }

    #[test]
    fn directory_aliases_revalidate_when_their_target_changes() {
        let (tmp, app, _, root) = setup();
        std::fs::write(root.join("file"), b"original").unwrap();
        let replacement = tmp.path().join("replacement");
        std::fs::create_dir(&replacement).unwrap();
        std::fs::write(replacement.join("file"), b"new").unwrap();
        let alias = tmp.path().join("alias");
        std::os::unix::fs::symlink(&root, &alias).unwrap();
        let pane = app.workspace.read().active_id();
        let healthy = app.workspace.write().add_pane();
        let original = scan(&app, healthy, &root, false);
        assert!(!original.stale);
        let first = scan(&app, pane, &alias, false);
        assert_eq!(first.size, 8);
        assert!(!first.stale, "an alias is watched through its parent");
        assert!(first.error.is_none());
        std::fs::remove_file(&alias).unwrap();
        std::os::unix::fs::symlink(&replacement, &alias).unwrap();
        wait(&app, pane, |s| s.size == 3);
        assert_eq!(
            serde_json::to_value(status(&app, healthy)).unwrap(),
            serde_json::to_value(original).unwrap(),
            "an alias's fallback must not rescan or degrade another pane"
        );
    }

    #[test]
    fn symlinks_do_not_recurse_and_missing_directories_report_errors() {
        let (_tmp, app, _, root) = setup();
        std::os::unix::fs::symlink(&root, root.join("cycle")).unwrap();
        std::fs::write(root.join("file"), b"hello").unwrap();
        let pane = app.workspace.read().active_id();
        let s = scan(&app, pane, &root, false);
        assert_eq!(s.dirs, 0);
        assert_eq!(s.files, 2);
        let missing = scan(&app, pane, &root.join("missing"), false);
        assert!(missing.error.is_some());
        assert!(missing.tree.is_none());
    }
    #[test]
    fn cancelled_requests_and_plugin_lifetime_cannot_publish_stale_results() {
        let (_tmp, app, plugin, root) = setup();
        for i in 0..100 {
            std::fs::create_dir(root.join(format!("dir-{i}"))).unwrap();
        }
        std::fs::write(root.join("dir-0/file"), b"x").unwrap();
        let pane = app.workspace.read().active_id();
        app.dispatch(&Invocation::with_args(
            "usage.scan",
            &serde_json::json!({"path":root}),
        ))
        .unwrap();
        app.dispatch(&Invocation::new("usage.cancel")).unwrap();
        assert!(!status(&app, pane).scanning);
        assert_eq!(scan(&app, pane, &root.join("dir-0"), false).size, 1);
        let weak = Arc::downgrade(&plugin.runtime.lock().as_ref().unwrap().shared);
        app.dispatch(&Invocation::with_args(
            "usage.scan",
            &serde_json::json!({"path":root,"force":true}),
        ))
        .unwrap();
        app.plugins.set_enabled(ID, false, &app);
        assert!(plugin.runtime.lock().is_none());
        assert!(app.registry.views.all().is_empty());
        let deadline = Instant::now() + Duration::from_secs(10);
        while weak.upgrade().is_some() {
            assert!(
                Instant::now() < deadline,
                "worker did not release its state"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        app.plugins.set_enabled(ID, true, &app);
        assert!(status(&app, pane).tree.is_none());
        assert_eq!(scan(&app, pane, &root.join("dir-0"), false).size, 1);
        app.dispatch(&Invocation::new("usage.release")).unwrap();
        assert!(status(&app, pane).root.is_none());
    }
}
