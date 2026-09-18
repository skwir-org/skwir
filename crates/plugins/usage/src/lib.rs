//! bibli plugin: disk usage. Scans a subtree in parallel and keeps a size
//! tree the UI renders as a treemap. Reports both apparent size and
//! allocated size (`st_blocks`), so compressed or reflinked files on btrfs
//! show what they really occupy.
//!
//! Commands: `usage.scan { path?, pane? }` starts (or restarts) a scan;
//! `usage.status {}` returns the current tree, partial while scanning;
//! `usage.cancel {}` stops it.

use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use bibli_core::app::App;
use bibli_core::cancel::CancelToken;
use bibli_core::command::Outcome;
use bibli_core::location::Location;
use bibli_core::plugin::Plugin;
use bibli_core::scheduler::Priority;
use bibli_core::workspace::PaneId;
use parking_lot::Mutex;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const ID: &str = "usage";
/// Children kept per node (largest first); the rest is summed into `other`.
const MAX_CHILDREN: usize = 64;
/// Depth of the tree kept for display; deeper sizes are folded into their ancestor.
const MAX_DEPTH: usize = 4;

/// One node of the size tree.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct Node {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    /// Sum of file sizes as reported by stat.
    pub size: u64,
    /// Sum of allocated bytes (`st_blocks * 512`).
    pub alloc: u64,
    pub files: u64,
    pub dirs: u64,
    /// Largest children, descending by `alloc`.
    pub children: Vec<Node>,
    /// Bytes (alloc) of children not listed individually.
    pub other_alloc: u64,
    pub other_size: u64,
}

/// What `usage.status` returns.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Status {
    pub root: Option<PathBuf>,
    pub scanning: bool,
    pub files: u64,
    pub dirs: u64,
    pub alloc: u64,
    pub size: u64,
    pub elapsed_ms: u64,
    pub tree: Option<Node>,
    pub error: Option<String>,
}

#[derive(Default)]
struct State {
    root: Option<PathBuf>,
    tree: Option<Node>,
    scanning: bool,
    error: Option<String>,
    cancel: Option<CancelToken>,
    started: Option<Instant>,
    finished_ms: u64,
}

/// Running totals, updated while scanning.
pub struct Counters {
    files: AtomicU64,
    dirs: AtomicU64,
    alloc: AtomicU64,
    size: AtomicU64,
}

/// The plugin. Holds the last scan.
pub struct UsagePlugin {
    state: Arc<Mutex<State>>,
    counters: Arc<Counters>,
}

impl Default for UsagePlugin {
    fn default() -> Self {
        UsagePlugin {
            state: Arc::new(Mutex::new(State::default())),
            counters: Arc::new(Counters {
                files: AtomicU64::new(0),
                dirs: AtomicU64::new(0),
                alloc: AtomicU64::new(0),
                size: AtomicU64::new(0),
            }),
        }
    }
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct ScanArgs {
    /// Directory to scan; the pane's directory when omitted.
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub pane: Option<PaneId>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct NoArgs {}

/// Scan `dir` recursively. `progress` is called every few thousand entries.
pub fn scan(
    dir: &Path,
    cancel: &CancelToken,
    counters: &Counters,
    progress: &(dyn Fn() + Sync),
) -> Node {
    scan_dir(dir, 0, cancel, counters, progress)
}

fn scan_dir(
    dir: &Path,
    depth: usize,
    cancel: &CancelToken,
    counters: &Counters,
    progress: &(dyn Fn() + Sync),
) -> Node {
    use rayon::prelude::*;
    let mut node = Node {
        name: dir.file_name().map_or_else(
            || dir.to_string_lossy().into_owned(),
            |n| n.to_string_lossy().into_owned(),
        ),
        path: dir.to_path_buf(),
        is_dir: true,
        ..Node::default()
    };
    if cancel.is_cancelled() {
        return node;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return node;
    };
    let mut files: Vec<Node> = Vec::new();
    let mut subdirs: Vec<PathBuf> = Vec::new();
    for e in rd.flatten() {
        let Ok(m) = e.metadata() else { continue };
        if m.is_dir() {
            subdirs.push(e.path());
        } else {
            let alloc = m.blocks() * 512;
            counters.files.fetch_add(1, Ordering::Relaxed);
            counters.alloc.fetch_add(alloc, Ordering::Relaxed);
            counters.size.fetch_add(m.len(), Ordering::Relaxed);
            files.push(Node {
                name: e.file_name().to_string_lossy().into_owned(),
                path: e.path(),
                is_dir: false,
                size: m.len(),
                alloc,
                files: 1,
                ..Node::default()
            });
        }
    }
    counters
        .dirs
        .fetch_add(subdirs.len() as u64, Ordering::Relaxed);
    if counters.files.load(Ordering::Relaxed) % 4096 < files.len() as u64 {
        progress();
    }
    let mut children: Vec<Node> = subdirs
        .par_iter()
        .map(|d| scan_dir(d, depth + 1, cancel, counters, progress))
        .collect();
    children.extend(files);
    for c in &children {
        node.size += c.size;
        node.alloc += c.alloc;
        node.files += c.files;
        node.dirs += c.dirs + u64::from(c.is_dir);
    }
    if depth >= MAX_DEPTH {
        // Keep only totals below the display depth.
        node.children.clear();
        node.other_alloc = node.alloc;
        node.other_size = node.size;
        return node;
    }
    children.sort_by_key(|c| std::cmp::Reverse(c.alloc));
    if children.len() > MAX_CHILDREN {
        for c in children.drain(MAX_CHILDREN..) {
            node.other_alloc += c.alloc;
            node.other_size += c.size;
        }
    }
    node.children = children;
    node
}

impl UsagePlugin {
    fn status(&self) -> Status {
        let s = self.state.lock();
        let elapsed_ms = match (s.started, s.scanning) {
            (Some(t), true) => t.elapsed().as_millis() as u64,
            _ => s.finished_ms,
        };
        Status {
            root: s.root.clone(),
            scanning: s.scanning,
            files: self.counters.files.load(Ordering::Relaxed),
            dirs: self.counters.dirs.load(Ordering::Relaxed),
            alloc: self.counters.alloc.load(Ordering::Relaxed),
            size: self.counters.size.load(Ordering::Relaxed),
            elapsed_ms,
            tree: s.tree.clone(),
            error: s.error.clone(),
        }
    }

    fn start(&self, app: &App, dir: PathBuf) {
        let cancel = app.lifetime().child();
        {
            let mut s = self.state.lock();
            if let Some(c) = s.cancel.take() {
                c.cancel();
            }
            s.root = Some(dir.clone());
            s.tree = None;
            s.error = None;
            s.scanning = true;
            s.started = Some(Instant::now());
            s.cancel = Some(cancel.clone());
        }
        for c in [
            &self.counters.files,
            &self.counters.dirs,
            &self.counters.alloc,
            &self.counters.size,
        ] {
            c.store(0, Ordering::Relaxed);
        }
        let state = Arc::clone(&self.state);
        let counters = Arc::clone(&self.counters);
        let waker: Arc<dyn Fn() + Send + Sync> = {
            let app_wake = app.waker_handle();
            Arc::new(move || app_wake())
        };
        let task_cancel = cancel.clone();
        app.scheduler
            .spawn(Priority::Background, "usage.scan", cancel, move || {
                let progress = {
                    let waker = Arc::clone(&waker);
                    move || waker()
                };
                let tree = scan(&dir, &task_cancel, &counters, &progress);
                let mut s = state.lock();
                if s.root.as_deref() == Some(dir.as_path()) {
                    s.finished_ms = s.started.map_or(0, |t| t.elapsed().as_millis() as u64);
                    s.scanning = false;
                    if task_cancel.is_cancelled() {
                        s.error = Some("cancelled".into());
                    } else {
                        s.tree = Some(tree);
                    }
                }
                drop(s);
                waker();
            });
    }
}

fn pane_dir(app: &App, pane: Option<PaneId>) -> anyhow::Result<PathBuf> {
    let ws = app.workspace.read();
    let pane = pane.unwrap_or_else(|| ws.active_id());
    let p = ws
        .pane(pane)
        .ok_or_else(|| anyhow::anyhow!("no such pane"))?;
    p.location()
        .and_then(Location::path)
        .ok_or_else(|| anyhow::anyhow!("pane is not showing a directory"))
}

impl Plugin for UsagePlugin {
    fn id(&self) -> &'static str {
        ID
    }
    fn title(&self) -> &'static str {
        "Disk usage"
    }
    fn description(&self) -> &'static str {
        "Scan a folder and show what takes space, as a treemap"
    }
    fn register(&self, app: &App) {
        let this = Arc::new(UsagePlugin {
            state: Arc::clone(&self.state),
            counters: Arc::clone(&self.counters),
        });
        let t = Arc::clone(&this);
        app.commands.register::<ScanArgs, _>(
            ID,
            "usage.scan",
            "Scan disk usage",
            "Scan a folder (default: the current one) and compute what takes space",
            move |a, app| {
                let dir = match a.path {
                    Some(p) => Location::parse(&p)
                        .path()
                        .ok_or_else(|| anyhow::anyhow!("not a path"))?,
                    None => pane_dir(app, a.pane)?,
                };
                anyhow::ensure!(dir.is_dir(), "{} is not a directory", dir.display());
                t.start(app, dir);
                Ok(Outcome::Done)
            },
        );
        let t = Arc::clone(&this);
        app.commands.register::<NoArgs, _>(
            ID,
            "usage.status",
            "Disk usage status",
            "The current size tree, partial while scanning",
            move |_, _| Ok(Outcome::Value(serde_json::to_value(t.status())?)),
        );
        let t = Arc::clone(&this);
        app.commands.register::<NoArgs, _>(
            ID,
            "usage.cancel",
            "Cancel disk usage scan",
            "Stop the running scan",
            move |_, _| {
                if let Some(c) = t.state.lock().cancel.take() {
                    c.cancel();
                }
                Ok(Outcome::Done)
            },
        );
        app.registry.keymap.add(
            ID,
            bibli_core::registry::KeyBinding {
                keys: "ctrl-shift-u".into(),
                command: bibli_core::command::Invocation::new("ui.usage"),
                when: None,
            },
        );
        app.registry.menus.add(
            ID,
            bibli_core::registry::MenuItem {
                menu: "context".into(),
                title: "Disk Usage…".into(),
                command: bibli_core::command::Invocation::new("ui.usage"),
                when: None,
                order: 430,
            },
        );
    }
    fn on_disable(&self, _app: &App) {
        if let Some(c) = self.state.lock().cancel.take() {
            c.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bibli_core::app::AppConfig;
    use bibli_core::command::Invocation;
    use std::time::Duration;

    #[test]
    fn scan_sums_sizes_and_keeps_largest_children() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("r");
        std::fs::create_dir_all(root.join("a/deep/deeper/deepest/beyond")).unwrap();
        std::fs::write(root.join("a/big"), vec![0u8; 100_000]).unwrap();
        std::fs::write(root.join("small"), b"x").unwrap();
        std::fs::write(
            root.join("a/deep/deeper/deepest/beyond/hidden"),
            vec![1u8; 5000],
        )
        .unwrap();
        let counters = Counters {
            files: AtomicU64::new(0),
            dirs: AtomicU64::new(0),
            alloc: AtomicU64::new(0),
            size: AtomicU64::new(0),
        };
        let tree = scan(&root, &CancelToken::new(), &counters, &|| {});
        assert_eq!(tree.size, 105_001);
        assert_eq!(tree.files, 3);
        assert_eq!(tree.children[0].name, "a", "largest child first");
        assert!(tree.alloc >= tree.size / 2, "allocation is tracked");
        let beyond = tree.children[0]
            .children
            .iter()
            .find(|c| c.name == "deep")
            .unwrap();
        assert_eq!(beyond.size, 5000, "deep sizes fold into their ancestor");
    }

    #[test]
    fn scan_command_reports_status_and_finishes() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("f"), b"hello").unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path().join("app"))).unwrap();
        app.plugins.add(Arc::new(UsagePlugin::default()), &app);
        app.dispatch(&Invocation::with_args(
            "usage.scan",
            &serde_json::json!({ "path": tmp.path().to_string_lossy() }),
        ))
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let Outcome::Value(v) = app.dispatch(&Invocation::new("usage.status")).unwrap() else {
                panic!()
            };
            let st: Status = serde_json::from_value(v).unwrap();
            if !st.scanning {
                assert!(st.tree.is_some());
                assert_eq!(st.size, 5);
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        app.plugins.set_enabled(ID, false, &app);
        assert!(app.commands.get("usage.scan").is_none());
    }
}
