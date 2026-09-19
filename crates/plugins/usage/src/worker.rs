//! All filesystem access, recursive monitoring and cache eviction happen here.
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::{collections::HashMap, os::unix::fs::MetadataExt};

use bibli_core::cancel::CancelToken;
use bibli_core::workspace::PaneId;
use crossbeam_channel::{Receiver, Sender};
use notify::{EventKind, RecursiveMode, Watcher};
use parking_lot::Mutex;
use std::collections::{BTreeMap, HashSet};

use super::{Message, Node, Request, Shared, SnapshotKind, Status};

const MAX_CHILDREN: usize = 64;
const MAX_CACHE_BYTES: usize = 16 * 1024 * 1024;
const MAX_DIRECTORIES: usize = 4096;
const MAX_WATCH_ROOTS: usize = 4;
const REVALIDATE: Duration = Duration::from_secs(2);
const PROGRESS_INTERVAL: Duration = Duration::from_millis(200);

#[derive(Default)]
struct Cache {
    dirs: HashMap<PathBuf, (Node, u64)>,
    order: BTreeMap<u64, PathBuf>,
    clock: u64,
    bytes: usize,
}
fn cost(n: &Node) -> usize {
    std::mem::size_of::<Node>()
        + n.path.as_os_str().len() * 3
        + n.name.len()
        + n.children.iter().map(cost).sum::<usize>()
}
impl Cache {
    fn get(&mut self, path: &Path) -> Option<Node> {
        let (node, stamp) = self.dirs.get_mut(path)?;
        self.order.remove(stamp);
        self.clock += 1;
        *stamp = self.clock;
        self.order.insert(*stamp, path.to_path_buf());
        Some(node.clone())
    }
    fn insert(&mut self, node: Node) {
        if let Some((old, stamp)) = self.dirs.remove(&node.path) {
            self.bytes -= cost(&old);
            self.order.remove(&stamp);
        }
        self.clock += 1;
        self.bytes += cost(&node);
        self.order.insert(self.clock, node.path.clone());
        self.dirs.insert(node.path.clone(), (node, self.clock));
        while self.bytes > MAX_CACHE_BYTES || self.dirs.len() > MAX_DIRECTORIES {
            if let Some((_, path)) = self.order.pop_first() {
                if let Some((node, _)) = self.dirs.remove(&path) {
                    self.bytes -= cost(&node);
                }
            }
        }
    }
    fn invalidate(&mut self, path: &Path) {
        self.dirs.retain(|p, (node, stamp)| {
            let remove = p.starts_with(path) || path.starts_with(p);
            if remove {
                self.bytes -= cost(node);
                self.order.remove(stamp);
            }
            !remove
        });
    }
    fn clear(&mut self) {
        self.dirs.clear();
        self.order.clear();
        self.bytes = 0;
    }
}

/// Bound notifications even when a long initial scan overlaps a large copy.
#[derive(Default)]
struct Changes {
    paths: HashSet<PathBuf>,
    lost: bool,
    pending: bool,
}
impl Changes {
    fn push(&mut self, event: notify::Result<notify::Event>) -> bool {
        match event {
            Ok(event) if matches!(event.kind, EventKind::Access(_)) => return false,
            Ok(event) if !event.need_rescan() && !event.paths.is_empty() => {
                if !self.lost {
                    self.paths.extend(event.paths);
                    if self.paths.len() > 4096 {
                        self.paths.clear();
                        self.lost = true;
                    }
                }
            }
            _ => {
                self.paths.clear();
                self.lost = true;
            }
        }
        let notify = !self.pending;
        self.pending = true;
        notify
    }
}

struct Scanner<'a> {
    cache: &'a mut Cache,
    request: &'a Request,
    shared: &'a Shared,
    dirs: u64,
    entries: u64,
    errors: u64,
    last_progress: Instant,
    preview: Option<Progress>,
}

/// Only the visible level and the branch being counted are retained. Partial
/// nodes never enter Cache; descendants still cache only completed breakdowns.
struct Progress {
    tree: Node,
    branch: Option<Node>,
}
impl Progress {
    fn snapshot(&self) -> Node {
        let mut tree = self.tree.clone();
        tree.incomplete = true;
        if let Some(branch) = &self.branch {
            add_contents(&mut tree, branch);
            replace_pending(&mut tree, branch.clone());
        }
        trim(&mut tree);
        tree
    }
}

fn add_contents(total: &mut Node, child: &Node) {
    total.size += child.size;
    total.alloc += child.alloc;
    total.files += child.files;
    total.dirs += child.dirs;
}

fn replace_pending(tree: &mut Node, mut child: Node) {
    if let Some(index) = tree.children.iter().position(|n| n.path == child.path) {
        tree.children.swap_remove(index);
    } else {
        // Pending children have no known bytes. They may be in the bounded
        // aggregate rather than individually displayed. Changes between the two
        // directory passes are reconciled by the queued filesystem events.
        tree.other_count = tree.other_count.saturating_sub(1);
        tree.other_incomplete = tree.other_incomplete.saturating_sub(1);
    }
    child.children.clear();
    tree.children.push(child);
    trim(tree);
}

fn entry_node(entry: &std::fs::DirEntry) -> std::io::Result<Node> {
    let metadata = std::fs::symlink_metadata(entry.path())?;
    let is_dir = metadata.is_dir();
    Ok(Node {
        id: Some(bibli_core::EntryId {
            dev: metadata.dev(),
            ino: metadata.ino(),
        }),
        path: entry.path(),
        name: entry.file_name().to_string_lossy().into_owned(),
        is_dir,
        incomplete: is_dir,
        size: if is_dir { 0 } else { metadata.len() },
        alloc: if is_dir { 0 } else { metadata.blocks() * 512 },
        files: u64::from(!is_dir),
        ..Node::default()
    })
}

impl Scanner<'_> {
    fn progress(&mut self, now: Instant) {
        if now.duration_since(self.last_progress) < PROGRESS_INTERVAL {
            return;
        }
        self.last_progress = now;
        self.publish(true);
    }

    fn publish(&self, updating: bool) {
        let mut statuses = self.shared.statuses.lock();
        let Some(s) = statuses.get_mut(&self.request.pane).filter(|s| {
            s.generation == self.request.generation && !self.request.cancel.is_cancelled()
        }) else {
            return;
        };
        // Progressive results are for the first scan of this location only.
        // Background revalidation must not turn a finished chart into unknown
        // blocks again. Explicit refresh can report progress over the old chart.
        if s.snapshot == SnapshotKind::Finished && !self.request.force {
            return;
        }
        s.updating = updating;
        s.scanned_dirs = self.dirs;
        if s.snapshot == SnapshotKind::Progressive
            && let Some(preview) = &self.preview
        {
            let tree = preview.snapshot();
            s.size = tree.size;
            s.alloc = tree.alloc;
            s.files = tree.files;
            s.dirs = tree.dirs;
            s.tree = Some(tree);
            s.error = None;
        }
        drop(statuses);
        self.shared.changed();
    }

    fn root(&mut self) -> anyhow::Result<Node> {
        let path = &self.request.path;
        anyhow::ensure!(!self.request.cancel.is_cancelled(), "Scan cancelled");
        if let Some(node) = self.cache.get(path) {
            return Ok(node);
        }
        self.list_root()?;
        self.scan_children()?;
        anyhow::ensure!(!self.request.cancel.is_cancelled(), "Scan cancelled");
        let mut node = self.preview.take().expect("root preview").tree;
        trim(&mut node);
        node.incomplete = self.errors > 0
            || node.other_incomplete > 0
            || node.children.iter().any(|child| child.incomplete);
        if !node.incomplete {
            self.cache.insert(node.clone());
        }
        Ok(node)
    }

    fn list_root(&mut self) -> anyhow::Result<()> {
        let path = &self.request.path;
        let metadata = std::fs::metadata(path)?;
        anyhow::ensure!(metadata.is_dir(), "Disk usage needs a directory");
        self.preview = Some(Progress {
            tree: Node {
                id: Some(bibli_core::EntryId {
                    dev: metadata.dev(),
                    ino: metadata.ino(),
                }),
                path: path.clone(),
                name: path
                    .file_name()
                    .unwrap_or(path.as_os_str())
                    .to_string_lossy()
                    .into_owned(),
                is_dir: true,
                ..Node::default()
            },
            branch: None,
        });
        // List files and unknown folders before descending into any subtree.
        // A second streaming pass finds directories without retaining an
        // unbounded queue of paths or stat-ing every file twice.
        self.dirs += 1;
        for entry in std::fs::read_dir(path)? {
            anyhow::ensure!(!self.request.cancel.is_cancelled(), "Scan cancelled");
            match entry.and_then(|entry| entry_node(&entry)) {
                Ok(child) => {
                    let tree = &mut self.preview.as_mut().expect("root preview").tree;
                    add_contents(tree, &child);
                    tree.dirs += u64::from(child.is_dir);
                    tree.children.push(child);
                    if tree.children.len() > MAX_CHILDREN * 2 {
                        trim(tree);
                    }
                }
                Err(_) => self.errors += 1,
            }
            self.entries += 1;
            if self.entries % 256 == 0 {
                self.progress(Instant::now());
            }
        }
        self.progress(Instant::now());
        Ok(())
    }

    fn scan_children(&mut self) -> anyhow::Result<()> {
        let path = &self.request.path;
        for entry in if self.preview.as_ref().expect("root preview").tree.dirs > 0 {
            Some(std::fs::read_dir(path)?)
        } else {
            None
        }
        .into_iter()
        .flatten()
        {
            anyhow::ensure!(!self.request.cancel.is_cancelled(), "Scan cancelled");
            let entry = match entry.and_then(|entry| entry.file_type().map(|kind| (entry, kind))) {
                Ok((entry, kind)) if kind.is_dir() => entry,
                Ok(_) => continue,
                Err(_) => {
                    self.errors += 1;
                    continue;
                }
            };
            let pending = Node {
                path: entry.path(),
                name: entry.file_name().to_string_lossy().into_owned(),
                is_dir: true,
                incomplete: true,
                ..Node::default()
            };
            self.preview.as_mut().expect("root preview").branch = Some(pending.clone());
            let child = match self.directory(&pending.path, 1) {
                Ok(child) => child,
                Err(_) if self.request.cancel.is_cancelled() => anyhow::bail!("Scan cancelled"),
                Err(_) => {
                    self.errors += 1;
                    pending
                }
            };
            let preview = self.preview.as_mut().expect("root preview");
            preview.branch = None;
            add_contents(&mut preview.tree, &child);
            replace_pending(&mut preview.tree, child);
            self.progress(Instant::now());
        }
        Ok(())
    }

    fn directory(&mut self, path: &Path, depth: usize) -> anyhow::Result<Node> {
        anyhow::ensure!(!self.request.cancel.is_cancelled(), "Scan cancelled");
        if let Some(node) = self.cache.get(path) {
            if let Some(branch) = self.preview.as_mut().and_then(|p| p.branch.as_mut()) {
                if depth == 1 {
                    branch.id = node.id;
                }
                add_contents(branch, &node);
                branch.dirs += u64::from(depth > 1);
            }
            return Ok(node);
        }
        if let Some(branch) = self.preview.as_mut().and_then(|p| p.branch.as_mut()) {
            branch.dirs += u64::from(depth > 1);
        }
        anyhow::ensure!(depth < 256, "Directory nesting exceeds the scan limit");
        let mut node = Node {
            name: path.file_name().map_or_else(
                || path.to_string_lossy().into_owned(),
                |n| n.to_string_lossy().into_owned(),
            ),
            path: path.to_path_buf(),
            is_dir: true,
            ..Node::default()
        };
        // Explicitly navigated roots may be directory aliases. Interior links
        // remain leaves, so a link back to an ancestor never creates a cycle.
        let metadata = if depth == 0 {
            std::fs::metadata(path)?
        } else {
            std::fs::symlink_metadata(path)?
        };
        anyhow::ensure!(metadata.is_dir(), "Disk usage needs a directory");
        node.id = Some(bibli_core::EntryId {
            dev: metadata.dev(),
            ino: metadata.ino(),
        });
        if depth == 1
            && let Some(branch) = self.preview.as_mut().and_then(|p| p.branch.as_mut())
        {
            branch.id = node.id;
        }
        self.progress(Instant::now());
        let entries = std::fs::read_dir(path)?;
        let initial_errors = self.errors;
        self.dirs += 1;
        for entry in entries {
            anyhow::ensure!(!self.request.cancel.is_cancelled(), "Scan cancelled");
            let Ok(entry) = entry else {
                self.errors += 1;
                continue;
            };
            let path = entry.path();
            let Ok(metadata) = std::fs::symlink_metadata(&path) else {
                self.errors += 1;
                continue;
            };
            let mut child = if metadata.is_dir() {
                match self.directory(&path, depth + 1) {
                    Ok(node) => node,
                    Err(_) if self.request.cancel.is_cancelled() => anyhow::bail!("Scan cancelled"),
                    Err(_) => {
                        self.errors += 1;
                        Node {
                            path,
                            name: entry.file_name().to_string_lossy().into_owned(),
                            is_dir: true,
                            incomplete: true,
                            ..Node::default()
                        }
                    }
                }
            } else {
                Node {
                    path,
                    name: entry.file_name().to_string_lossy().into_owned(),
                    size: metadata.len(),
                    alloc: metadata.blocks() * 512,
                    files: 1,
                    ..Node::default()
                }
            };
            child.id = Some(bibli_core::EntryId {
                dev: metadata.dev(),
                ino: metadata.ino(),
            });
            if !child.is_dir
                && let Some(branch) = self.preview.as_mut().and_then(|p| p.branch.as_mut())
            {
                add_contents(branch, &child);
            }
            node.size += child.size;
            node.alloc += child.alloc;
            node.files += child.files;
            node.dirs += child.dirs + u64::from(child.is_dir);
            child.children.clear();
            node.children.push(child);
            // Keep transient memory bounded even in directories with millions of entries.
            if node.children.len() > MAX_CHILDREN * 2 {
                trim(&mut node);
            }
            self.entries += 1;
            if self.entries % 256 == 0 {
                self.progress(Instant::now());
            }
        }
        trim(&mut node);
        node.incomplete = self.errors != initial_errors;
        if !node.incomplete {
            self.cache.insert(node.clone());
        }
        Ok(node)
    }
}
fn trim(node: &mut Node) {
    node.children
        .sort_by(|a, b| b.alloc.cmp(&a.alloc).then_with(|| a.name.cmp(&b.name)));
    if node.children.len() > MAX_CHILDREN {
        for child in node.children.drain(MAX_CHILDREN..) {
            node.other_alloc += child.alloc;
            node.other_size += child.size;
            node.other_count += 1;
            node.other_incomplete += u64::from(child.incomplete);
        }
    }
}

struct Active {
    request: Request,
    // Aliases need fallback even when their spelling lies beneath a watched root.
    watchable: bool,
    last_scan: Instant,
}

struct Worker {
    shared: Arc<Shared>,
    cache: Cache,
    active: HashMap<PaneId, Active>,
    watcher: Option<notify::RecommendedWatcher>,
    roots: Vec<PathBuf>,
    registrations: HashSet<PathBuf>,
}
impl Worker {
    fn prepare(&mut self, request: &Request) {
        if request.cancel.is_cancelled()
            || self.cache.dirs.contains_key(&request.path)
            || self.roots.iter().any(|root| request.path.starts_with(root))
            || self
                .shared
                .statuses
                .lock()
                .get(&request.pane)
                .is_none_or(|s| s.tree.is_some())
        {
            return;
        }
        // Native recursive watch setup itself walks the tree. Show the visible
        // level before that work; these unmonitored observations are never
        // cached and root() reads them again once coverage is established.
        let mut scanner = Scanner {
            cache: &mut self.cache,
            request,
            shared: &self.shared,
            dirs: 0,
            entries: 0,
            errors: 0,
            last_progress: Instant::now(),
            preview: None,
        };
        if scanner.list_root().is_ok() {
            scanner.publish(false);
        }
    }

    fn watch(&mut self, path: &Path) -> bool {
        // Watching the resolved inode cannot detect replacement of an alias in
        // this path's ancestors. Revalidate aliases periodically instead.
        if std::fs::canonicalize(path).is_ok_and(|resolved| resolved != path) {
            return false;
        }
        if self.roots.iter().any(|root| path.starts_with(root)) {
            return true;
        }
        let Some(watcher) = self.watcher.as_mut() else {
            return false;
        };
        if watcher.watch(path, RecursiveMode::Recursive).is_err() {
            // Recursive registration can fail after adding part of a subtree.
            // Removing that partial parent can also remove shared child watches.
            let _ = watcher.unwatch(path);
            self.roots.retain(|root| !root.starts_with(path));
            self.unwatch(path);
            self.cache.invalidate(path);
            return false;
        }
        // A backend can share watch descriptors across overlapping requests.
        // Unwatching a covered child here would punch a hole in the new parent.
        self.registrations.insert(path.to_path_buf());
        self.roots.retain(|root| !root.starts_with(path));
        self.roots.push(path.to_path_buf());
        if self.roots.len() > MAX_WATCH_ROOTS {
            let old = self.roots.remove(0);
            self.unwatch(&old);
            self.cache.invalidate(&old);
        }
        true
    }
    fn unwatch(&mut self, path: &Path) {
        self.registrations.retain(|root| {
            if root.starts_with(path) {
                if let Some(watcher) = self.watcher.as_mut() {
                    let _ = watcher.unwatch(root);
                }
                false
            } else {
                true
            }
        });
    }
    fn monitored(&self, active: &Active) -> bool {
        active.watchable
            && self
                .roots
                .iter()
                .any(|root| active.request.path.starts_with(root))
    }
    fn scan(&mut self, request: &Request, watched: bool) {
        if request.cancel.is_cancelled() {
            return;
        }
        if request.force || !watched {
            self.cache.invalidate(&request.path);
        }
        {
            let mut statuses = self.shared.statuses.lock();
            if let Some(s) = statuses
                .get_mut(&request.pane)
                .filter(|s| s.generation == request.generation)
            {
                s.scanning = true;
                s.updating = false;
            }
        }
        // Do not publish a busy state for every cache hit or short update.
        // Scanner progress wakes the UI only once real work lasts long enough.
        let started = Instant::now();
        let mut scanner = Scanner {
            cache: &mut self.cache,
            request,
            shared: &self.shared,
            dirs: 0,
            entries: 0,
            errors: 0,
            last_progress: started,
            preview: None,
        };
        let tree = scanner.root();
        let scanned_dirs = scanner.dirs;
        let errors = scanner.errors;
        let mut statuses = self.shared.statuses.lock();
        if !request.cancel.is_cancelled()
            && let Some(s) = statuses
                .get_mut(&request.pane)
                .filter(|s| s.generation == request.generation)
        {
            let incomplete = tree.as_ref().is_ok_and(|n| n.incomplete);
            let error = tree
                .as_ref()
                .err()
                .map(|_| "Unable to read this directory".to_owned())
                .or_else(|| {
                    (errors > 0).then(|| {
                        format!("{errors} entries could not be read; totals are incomplete")
                    })
                })
                .or_else(|| {
                    incomplete
                        .then(|| "Directory changed during scan; totals are incomplete".into())
                })
                .or_else(|| {
                    (!watched)
                        .then(|| "Live monitoring unavailable; rechecking every 2 seconds".into())
                });
            let node = tree.ok();
            *s = Status {
                root: Some(request.path.clone()),
                scanning: false,
                updating: false,
                elapsed_ms: started.elapsed().as_millis() as u64,
                files: node.as_ref().map_or(0, |n| n.files),
                dirs: node.as_ref().map_or(0, |n| n.dirs),
                size: node.as_ref().map_or(0, |n| n.size),
                alloc: node.as_ref().map_or(0, |n| n.alloc),
                snapshot: if node.is_some() {
                    SnapshotKind::Finished
                } else {
                    SnapshotKind::Progressive
                },
                tree: node,
                error,
                stale: !watched || incomplete,
                scanned_dirs,
                generation: request.generation,
            };
        }
        drop(statuses);
        self.shared.changed();
    }
    fn changes(&mut self, changes: Changes) -> HashSet<PaneId> {
        let affected = self
            .active
            .iter()
            .filter_map(|(pane, active)| {
                (changes.lost
                    || changes.paths.iter().any(|path| {
                        path.starts_with(&active.request.path)
                            || active.request.path.starts_with(path)
                    }))
                .then_some(*pane)
            })
            .collect();
        if changes.lost {
            self.cache.clear();
            // A lost batch can hide replacement of a root. Rebuild coverage;
            // a failed registration then falls back only for that active path.
            for root in std::mem::take(&mut self.roots) {
                self.unwatch(&root);
            }
        }
        for path in changes.paths {
            self.cache.invalidate(&path);
            // Also handle removal/replacement of an ancestor of a watched root.
            if self.roots.iter().any(|root| root.starts_with(&path)) {
                self.roots.retain(|root| !root.starts_with(&path));
                self.unwatch(&path);
            }
        }
        affected
    }
    fn update(&mut self, dirty: &HashSet<PaneId>, now: Instant) {
        let mut active = std::mem::take(&mut self.active);
        for (pane, state) in &mut active {
            let watched = self.monitored(state);
            let fallback_due = !watched && now.duration_since(state.last_scan) >= REVALIDATE;
            if !state.request.cancel.is_cancelled() && (dirty.contains(pane) || fallback_due) {
                if !watched {
                    // Never reuse data collected during a gap in coverage, even
                    // when re-registering succeeds on this attempt.
                    self.cache.invalidate(&state.request.path);
                    state.watchable = self.watch(&state.request.path);
                }
                self.scan(&state.request, self.monitored(state));
                state.last_scan = Instant::now();
            }
        }
        self.active = active;
    }
}

pub(super) fn run(
    rx: &Receiver<Message>,
    tx: Sender<Message>,
    shared: Arc<Shared>,
    cancel: &CancelToken,
) {
    let changes = Arc::new(Mutex::new(Changes::default()));
    let watcher_changes = Arc::clone(&changes);
    let watcher = notify::RecommendedWatcher::new(
        move |event| {
            if watcher_changes.lock().push(event) {
                let _ = tx.send(Message::Changed);
            }
        },
        notify::Config::default().with_follow_symlinks(false),
    )
    .ok();
    let mut worker = Worker {
        shared,
        cache: Cache::default(),
        active: HashMap::new(),
        watcher,
        roots: Vec::new(),
        registrations: HashSet::new(),
    };
    while !cancel.is_cancelled() {
        let message = match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(message) => Some(message),
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => None,
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        };
        let mut dirty = HashSet::new();
        let mut messages: Vec<_> = message.into_iter().collect();
        // Coalesce a burst without delaying a new visible request.
        if matches!(messages.first(), Some(Message::Changed)) {
            while let Ok(message) = rx.recv_timeout(Duration::from_millis(40)) {
                let command = !matches!(message, Message::Changed);
                messages.push(message);
                if command || messages.len() >= 1024 {
                    break;
                }
            }
        }
        messages.extend(rx.try_iter().take(1024));
        for message in messages {
            match message {
                Message::Scan(request) => {
                    if request.cancel.is_cancelled() {
                        continue;
                    }
                    worker.prepare(&request);
                    if request.cancel.is_cancelled() {
                        continue;
                    }
                    let covered = worker.watch(&request.path);
                    worker.scan(&request, covered);
                    worker.active.insert(
                        request.pane,
                        Active {
                            request: Request {
                                force: false,
                                ..request
                            },
                            watchable: covered,
                            last_scan: Instant::now(),
                        },
                    );
                }
                Message::Release(pane) => {
                    worker.active.remove(&pane);
                }
                Message::Changed => {
                    let batch = std::mem::take(&mut *changes.lock());
                    dirty.extend(worker.changes(batch));
                }
            }
        }
        worker.update(&dirty, Instant::now());
    }
    // Watcher/cache destruction stays on this thread, including plugin disable and app shutdown.
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{CreateKind, ModifyKind};

    fn request(pane: u32, path: PathBuf) -> Request {
        Request {
            pane: PaneId(pane),
            path,
            force: false,
            generation: 1,
            cancel: CancelToken::new(),
        }
    }

    fn shared() -> Arc<Shared> {
        Arc::new(Shared {
            statuses: Mutex::default(),
            revision: Arc::default(),
            wake: Arc::new(|| {}),
        })
    }

    fn scanner<'a>(cache: &'a mut Cache, request: &'a Request, shared: &'a Shared) -> Scanner<'a> {
        Scanner {
            cache,
            request,
            shared,
            dirs: 0,
            entries: 0,
            errors: 0,
            // Deterministic first publication, without depending on disk speed.
            last_progress: Instant::now().checked_sub(PROGRESS_INTERVAL).unwrap(),
            preview: None,
        }
    }

    fn observed(request: &Request) -> (Arc<Shared>, Receiver<Status>) {
        let (tx, rx) = crossbeam_channel::unbounded();
        let pane = request.pane;
        let shared = Arc::new_cyclic(|weak: &std::sync::Weak<Shared>| {
            let weak = weak.clone();
            Shared {
                statuses: Mutex::new(HashMap::from([(
                    pane,
                    Status {
                        root: Some(request.path.clone()),
                        scanning: true,
                        generation: request.generation,
                        ..Status::default()
                    },
                )])),
                revision: Arc::default(),
                wake: Arc::new(move || {
                    if let Some(shared) = weak.upgrade() {
                        tx.send(shared.statuses.lock()[&pane].clone()).unwrap();
                    }
                }),
            }
        });
        (shared, rx)
    }

    #[test]
    fn refresh_progress_retains_finished_results_until_replacement_is_ready() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("child")).unwrap();
        let file = tmp.path().join("child/payload");
        std::fs::write(&file, b"old").unwrap();
        let mut request = request(0, tmp.path().into());
        let (shared, updates) = observed(&request);
        let mut worker = Worker {
            shared: Arc::clone(&shared),
            cache: Cache::default(),
            active: HashMap::new(),
            watcher: None,
            roots: Vec::new(),
            registrations: HashSet::new(),
        };
        worker.scan(&request, false);
        assert_eq!(
            shared.statuses.lock()[&request.pane].snapshot,
            SnapshotKind::Finished
        );
        // Repeated fallback passes, explicit refresh, and a finished result
        // with read errors all retain the displayed breakdown during work.
        for (index, (force, incomplete)) in [
            (false, false),
            (false, false),
            (true, false),
            (true, true),
            (false, true),
        ]
        .into_iter()
        .enumerate()
        {
            request.force = force;
            {
                let mut statuses = shared.statuses.lock();
                let s = statuses.get_mut(&request.pane).unwrap();
                s.scanning = true;
                s.tree.as_mut().unwrap().incomplete = incomplete;
                s.error = incomplete.then(|| "Some entries could not be read".into());
            }
            let before = shared.statuses.lock()[&request.pane].clone();
            updates.try_iter().for_each(drop);
            std::fs::write(&file, vec![0; 10 + index]).unwrap();
            worker.cache.clear();
            // Force a progress checkpoint independently of filesystem speed.
            let fresh = scanner(&mut worker.cache, &request, &shared)
                .root()
                .unwrap();
            assert_eq!(fresh.size, (10 + index) as u64);
            let during = shared.statuses.lock()[&request.pane].clone();
            assert_eq!(
                serde_json::to_value(&during.tree).unwrap(),
                serde_json::to_value(&before.tree).unwrap()
            );
            assert_eq!(
                (during.size, during.alloc, during.files, during.dirs),
                (before.size, before.alloc, before.files, before.dirs)
            );
            assert_eq!(during.error, before.error);
            assert_eq!(during.updating, force);
            let progress: Vec<_> = updates.try_iter().collect();
            assert_eq!(
                !progress.is_empty(),
                force,
                "automatic refresh must stay silent"
            );
            worker.scan(&request, false);
            let finished = shared.statuses.lock()[&request.pane].clone();
            assert_eq!(finished.size, fresh.size);
            assert!(!finished.scanning && !finished.updating);
            assert!(!finished.tree.unwrap().incomplete);
        }
    }

    #[test]
    fn watch_setup_preview_is_uncached_and_revalidated_before_completion() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("direct");
        std::fs::write(&file, b"old").unwrap();
        std::fs::create_dir(tmp.path().join("child")).unwrap();
        std::fs::write(tmp.path().join("child/payload"), b"data").unwrap();
        let request = request(0, tmp.path().into());
        let (shared, updates) = observed(&request);
        let mut worker = Worker {
            shared,
            cache: Cache::default(),
            active: HashMap::new(),
            watcher: None,
            roots: Vec::new(),
            registrations: HashSet::new(),
        };
        worker.prepare(&request);
        let initial = updates.try_recv().unwrap();
        assert_eq!(initial.size, 3);
        assert!(initial.tree.unwrap().incomplete);
        assert!(worker.cache.dirs.is_empty());
        std::fs::write(&file, b"new content").unwrap();
        worker.scan(&request, false);
        let final_status = worker.shared.statuses.lock()[&request.pane].clone();
        assert!(!final_status.scanning);
        assert_eq!(final_status.size, 15);
        assert!(!final_status.tree.unwrap().incomplete);
    }

    #[test]
    fn progressive_root_shows_files_before_subtrees_and_finishes_with_exact_totals() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("summary.txt"), b"hello").unwrap();
        for index in 0..80 {
            let dir = tmp.path().join(format!("folder-{index:02}"));
            std::fs::create_dir(&dir).unwrap();
            std::fs::write(dir.join("payload"), [0; 37]).unwrap();
        }
        let request = request(0, tmp.path().into());
        let (shared, updates) = observed(&request);
        let mut cache = Cache::default();
        let result = scanner(&mut cache, &request, &shared).root().unwrap();
        let first = updates.try_recv().unwrap();
        assert!(first.scanning && first.tree.as_ref().unwrap().incomplete);
        assert_eq!((first.size, first.files, first.dirs), (5, 1, 80));
        let tree = first.tree.unwrap();
        assert_eq!(tree.children.len(), MAX_CHILDREN);
        assert!(
            tree.children
                .iter()
                .any(|n| n.name == "summary.txt" && !n.incomplete)
        );
        assert!(
            tree.children
                .iter()
                .filter(|n| n.is_dir)
                .all(|n| n.incomplete && n.id.is_some())
        );
        assert!(tree.other_incomplete > 0);
        assert!(!result.incomplete);
        assert_eq!(
            (result.size, result.files, result.dirs),
            (5 + 80 * 37, 81, 80)
        );
        assert_eq!(result.other_count, 17);
        assert_eq!(result.other_incomplete, 0);
        assert_eq!(
            result.children.iter().map(|n| n.size).sum::<u64>() + result.other_size,
            result.size
        );
        assert_eq!(
            result.children.iter().map(|n| n.alloc).sum::<u64>() + result.other_alloc,
            result.alloc
        );
        assert!(cache.dirs.values().all(|(node, _)| !node.incomplete));
        let revision = shared.revision.load(std::sync::atomic::Ordering::Acquire);
        assert_eq!(
            scanner(&mut cache, &request, &shared).root().unwrap().size,
            result.size
        );
        assert_eq!(
            shared.revision.load(std::sync::atomic::Ordering::Acquire),
            revision,
            "a cached root publishes no partial updates"
        );
    }

    #[test]
    fn progressive_branch_counts_nested_and_cached_contents_once() {
        let tmp = tempfile::tempdir().unwrap();
        let child = tmp.path().join("child");
        std::fs::create_dir_all(child.join("nested")).unwrap();
        std::fs::write(child.join("direct"), b"one").unwrap();
        std::fs::write(child.join("nested/file"), b"two!").unwrap();
        let request = request(0, tmp.path().into());
        let shared = shared();
        let mut cache = Cache::default();
        for pass in 0..2 {
            // On the second pass reuse the nested cache but recalculate its parent.
            let pending = Node {
                path: child.clone(),
                is_dir: true,
                incomplete: true,
                ..Node::default()
            };
            let mut scan = scanner(&mut cache, &request, &shared);
            scan.preview = Some(Progress {
                tree: Node {
                    dirs: 1,
                    children: vec![pending.clone()],
                    ..Node::default()
                },
                branch: Some(pending),
            });
            let result = scan.directory(&child, 1).unwrap();
            let partial = scan.preview.as_ref().unwrap().snapshot();
            assert_eq!((partial.size, partial.files, partial.dirs), (7, 2, 2));
            assert!(partial.children[0].incomplete && partial.children[0].id.is_some());
            assert_eq!(partial.children[0].alloc, result.alloc);
            assert_eq!(scan.dirs, if pass == 0 { 2 } else { 1 });
            if let Some((node, stamp)) = cache.dirs.remove(&child) {
                cache.bytes -= cost(&node);
                cache.order.remove(&stamp);
            }
        }
    }

    #[test]
    fn cancelling_a_partial_scan_never_caches_the_unfinished_root() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("child")).unwrap();
        std::fs::write(tmp.path().join("child/payload"), b"data").unwrap();
        let request = request(0, tmp.path().into());
        let cancel = request.cancel.clone();
        let shared = Shared {
            statuses: Mutex::new(HashMap::from([(
                request.pane,
                Status {
                    generation: 1,
                    ..Status::default()
                },
            )])),
            revision: Arc::default(),
            wake: Arc::new(move || cancel.cancel()),
        };
        let mut cache = Cache::default();
        assert!(scanner(&mut cache, &request, &shared).root().is_err());
        assert!(
            shared.statuses.lock()[&request.pane]
                .tree
                .as_ref()
                .unwrap()
                .incomplete
        );
        assert!(cache.get(tmp.path()).is_none());
        let fresh = self::request(1, tmp.path().into());
        let result = scanner(&mut cache, &fresh, &shared).root().unwrap();
        assert_eq!(result.size, 4);
        assert!(!result.incomplete);
    }

    #[test]
    fn updates_only_touch_intersecting_panes_and_due_fallbacks() {
        let tmp = tempfile::tempdir().unwrap();
        let left = tmp.path().join("left");
        let right = tmp.path().join("right");
        let fallback = tmp.path().join("fallback");
        for path in [&left, &right, &fallback] {
            std::fs::create_dir(path).unwrap();
            std::fs::write(path.join("file"), b"original").unwrap();
        }
        let shared = shared();
        let mut worker = Worker {
            shared: Arc::clone(&shared),
            cache: Cache::default(),
            active: HashMap::new(),
            watcher: None,
            roots: vec![tmp.path().into()],
            registrations: HashSet::new(),
        };
        for (pane, path) in [left.clone(), right.clone(), fallback.clone()]
            .into_iter()
            .enumerate()
        {
            let request = request(pane as u32, path);
            shared.statuses.lock().insert(
                request.pane,
                Status {
                    generation: 1,
                    ..Status::default()
                },
            );
            worker.scan(&request, pane != 2);
            worker.active.insert(
                request.pane,
                Active {
                    request,
                    watchable: pane != 2,
                    last_scan: Instant::now(),
                },
            );
        }
        let right_before = serde_json::to_value(&shared.statuses.lock()[&PaneId(1)]).unwrap();
        let fallback_deadline = worker.active[&PaneId(2)].last_scan + REVALIDATE;
        std::fs::write(left.join("file"), b"changed").unwrap();
        let dirty = worker.changes(Changes {
            paths: HashSet::from([left.join("file")]),
            ..Changes::default()
        });
        assert_eq!(dirty, HashSet::from([PaneId(0)]));
        worker.update(&dirty, Instant::now());
        assert_eq!(shared.statuses.lock()[&PaneId(0)].size, 7);
        // Unrelated changes cannot postpone the fallback deadline.
        std::fs::write(fallback.join("file"), b"updated fallback").unwrap();
        worker.update(&HashSet::new(), fallback_deadline);
        assert_eq!(shared.statuses.lock()[&PaneId(2)].size, 16);
        assert_eq!(
            serde_json::to_value(&shared.statuses.lock()[&PaneId(1)]).unwrap(),
            right_before
        );

        let revision = shared.revision.load(std::sync::atomic::Ordering::Acquire);
        let dirty = worker.changes(Changes {
            paths: HashSet::from([tmp.path().join("previously-visited/file")]),
            ..Changes::default()
        });
        assert!(dirty.is_empty());
        worker.update(&dirty, Instant::now());
        assert_eq!(
            shared.revision.load(std::sync::atomic::Ordering::Acquire),
            revision
        );
        let dirty = worker.changes(Changes {
            paths: HashSet::from([tmp.path().to_path_buf()]),
            ..Changes::default()
        });
        assert_eq!(
            dirty.len(),
            3,
            "an ancestor replacement affects every descendant pane"
        );
    }

    #[test]
    fn progress_is_delayed_and_cannot_outlive_its_request() {
        let shared = shared();
        let request = request(0, PathBuf::from("/synthetic"));
        shared.statuses.lock().insert(
            request.pane,
            Status {
                scanning: true,
                generation: 1,
                ..Status::default()
            },
        );
        let mut cache = Cache::default();
        cache.insert(Node {
            path: request.path.clone(),
            ..Node::default()
        });
        let started = Instant::now();
        let mut scanner = Scanner {
            cache: &mut cache,
            request: &request,
            shared: &shared,
            dirs: 2,
            entries: 0,
            errors: 0,
            last_progress: started,
            preview: None,
        };
        scanner.directory(&request.path, 0).unwrap();
        scanner.progress(started + PROGRESS_INTERVAL / 2);
        assert!(!shared.statuses.lock()[&request.pane].updating);
        assert_eq!(
            shared.revision.load(std::sync::atomic::Ordering::Acquire),
            0
        );
        scanner.progress(started + PROGRESS_INTERVAL);
        assert!(shared.statuses.lock()[&request.pane].updating);
        assert_eq!(shared.statuses.lock()[&request.pane].scanned_dirs, 2);
        shared
            .statuses
            .lock()
            .get_mut(&request.pane)
            .unwrap()
            .generation = 2;
        scanner.progress(started + PROGRESS_INTERVAL * 2);
        assert_eq!(
            shared.revision.load(std::sync::atomic::Ordering::Acquire),
            1
        );
        shared
            .statuses
            .lock()
            .get_mut(&request.pane)
            .unwrap()
            .generation = 1;
        request.cancel.cancel();
        scanner.progress(started + PROGRESS_INTERVAL * 3);
        assert_eq!(
            shared.revision.load(std::sync::atomic::Ordering::Acquire),
            1
        );
    }

    #[test]
    fn eviction_preserves_recent_breakdowns_and_bounds_memory() {
        let mut cache = Cache::default();
        for index in 0..MAX_DIRECTORIES {
            cache.insert(Node {
                path: PathBuf::from(format!("/synthetic/{index}")),
                ..Node::default()
            });
        }
        assert!(cache.get(Path::new("/synthetic/0")).is_some());
        cache.insert(Node {
            path: PathBuf::from("/synthetic/new"),
            ..Node::default()
        });
        assert!(cache.get(Path::new("/synthetic/1")).is_none());
        assert!(cache.get(Path::new("/synthetic/0")).is_some());
        cache.insert(Node {
            path: PathBuf::from("/synthetic/oversized"),
            name: "x".repeat(MAX_CACHE_BYTES),
            ..Node::default()
        });
        assert!(cache.bytes <= MAX_CACHE_BYTES);
        assert_eq!(cache.dirs.len(), cache.order.len());
    }

    #[test]
    fn lost_batches_keep_revalidating_instead_of_trusting_old_coverage() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("file");
        std::fs::write(&file, b"a").unwrap();
        let pane = PaneId(0);
        let shared = Arc::new(Shared {
            statuses: Mutex::new(HashMap::from([(
                pane,
                Status {
                    generation: 1,
                    ..Status::default()
                },
            )])),
            revision: Arc::default(),
            wake: Arc::new(|| {}),
        });
        let request = Request {
            pane,
            path: tmp.path().into(),
            force: false,
            generation: 1,
            cancel: CancelToken::new(),
        };
        let mut worker = Worker {
            shared: Arc::clone(&shared),
            cache: Cache::default(),
            active: HashMap::new(),
            watcher: None,
            roots: vec![tmp.path().into()],
            registrations: HashSet::new(),
        };
        worker.scan(&request, true);
        worker.changes(Changes {
            lost: true,
            ..Changes::default()
        });
        for data in [b"new".as_slice(), b"newer".as_slice()] {
            std::fs::write(&file, data).unwrap();
            assert!(worker.roots.is_empty());
            worker.scan(&request, false);
            let statuses = shared.statuses.lock();
            assert_eq!(statuses[&pane].size, data.len() as u64);
            assert!(statuses[&pane].stale);
        }
    }

    #[test]
    fn event_bursts_are_bounded_and_lost_events_require_revalidation() {
        let mut changes = Changes::default();
        for index in 0..10_000 {
            let notify = changes.push(Ok(notify::Event::new(EventKind::Create(CreateKind::File))
                .add_path(PathBuf::from(format!("/synthetic/{index}")))));
            assert_eq!(notify, index == 0, "only one wake can be queued per batch");
        }
        assert!(changes.lost);
        assert!(changes.paths.is_empty());
        let mut fresh = Changes::default();
        fresh.push(Err(notify::Error::generic("synthetic overflow")));
        assert!(fresh.lost);
        fresh.push(Ok(notify::Event::new(EventKind::Modify(ModifyKind::Any))
            .add_path(PathBuf::from("/synthetic/file"))));
        assert!(fresh.paths.is_empty());
    }
}
