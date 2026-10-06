//! bibli plugin: jump. Frecency jump that reuses what the environment already
//! tracks: zoxide's directory ranking and the desktop's recently used files.
//! Also provides the `recent:` location listing those files.

pub mod xbel;
pub mod zoxide;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use bibli_core::CancelToken;
use bibli_core::app::{App, NotifyLevel};
use bibli_core::command::{Invocation, Outcome};
use bibli_core::entry::EntryId;
use bibli_core::fs::hydrate_paths;
use bibli_core::location::{
    EmptyState, EventSink, ListingEvent, Location, LocationProvider, OpenContext, OpenLocation,
};
use bibli_core::os_path::PathArg;
use bibli_core::plugin::Plugin;
use bibli_core::registry::{KeyBinding, Place};
use bibli_core::scheduler::Priority;
use bibli_core::workspace::PaneId;
use crossbeam_channel::Receiver;
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use parking_lot::Mutex;

const ID: &str = "jump";
/// What `recent:` is called, in the sidebar and wherever it is shown.
const RECENT: &str = "Recent";

/// A ranked jump target.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Candidate {
    #[serde(with = "bibli_core::os_path")]
    pub path: PathBuf,
    pub score: f64,
    /// `dir` or `file`.
    pub kind: &'static str,
    /// `zoxide` or `recent`.
    pub source: &'static str,
}

/// Rank `dirs` (zoxide) and `files` (recent) against `query`. An empty query
/// ranks by source score alone.
pub fn rank(
    query: &str,
    dirs: &[zoxide::ZEntry],
    files: &[xbel::Recent],
    limit: usize,
) -> Vec<Candidate> {
    let now = SystemTime::now();
    let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
    let pattern = Pattern::parse(query, CaseMatching::Smart, Normalization::Smart);
    let mut buf = Vec::new();
    let mut fuzzy = |s: &str| -> Option<f64> {
        if query.trim().is_empty() {
            return Some(1.0);
        }
        pattern
            .score(Utf32Str::new(s, &mut buf), &mut matcher)
            .map(f64::from)
    };
    let mut out: Vec<Candidate> = Vec::new();
    for d in dirs {
        let s = d.path.to_string_lossy();
        if let Some(f) = fuzzy(&s) {
            out.push(Candidate {
                path: d.path.clone(),
                score: f * (1.0 + d.score),
                kind: "dir",
                source: "zoxide",
            });
        }
    }
    for r in files {
        let s = r.path.to_string_lossy();
        if let Some(f) = fuzzy(&s) {
            let days = now
                .duration_since(r.visited)
                .map_or(f64::MAX, |d| d.as_secs_f64() / 86_400.0);
            let recency = 1.0 / (1.0 + days);
            out.push(Candidate {
                path: r.path.clone(),
                score: f * (1.0 + recency),
                kind: "file",
                source: "recent",
            });
        }
    }
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out.truncate(limit);
    out
}

/// What the candidates are ranked from, gathered on a worker: ranking must
/// be quick, and asking zoxide runs a process and checking recent files
/// stats each one.
#[derive(Default)]
struct Index {
    dirs: Vec<zoxide::ZEntry>,
    files: Vec<xbel::Recent>,
    /// When the last refresh started.
    asked: Option<Instant>,
}

impl Index {
    /// How long a refresh answers for before the next query starts another.
    const TTL: Duration = Duration::from_secs(10);
}

/// The jump plugin.
#[derive(Default)]
pub struct JumpPlugin {
    index: Arc<Mutex<Index>>,
    /// Cancelled on disable: a refresh in flight stops and publishes nothing.
    lifetime: Mutex<CancelToken>,
}

/// Start gathering the index again unless a refresh started less than
/// [`Index::TTL`] ago (or is still running).
fn refresh(index: &Arc<Mutex<Index>>, lifetime: &CancelToken, app: &App) {
    {
        let mut ix = index.lock();
        if ix.asked.is_some_and(|at| at.elapsed() < Index::TTL) {
            return;
        }
        ix.asked = Some(Instant::now());
    }
    let index = Arc::clone(index);
    let cancel = lifetime.child();
    app.scheduler.spawn(
        Priority::Background,
        "jump.index",
        cancel.clone(),
        move || {
            let dirs = zoxide::query(&cancel);
            let files = recent_files();
            if cancel.is_cancelled() {
                return;
            }
            let mut ix = index.lock();
            ix.dirs = dirs;
            ix.files = files;
        },
    );
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct CandidatesArgs {
    #[serde(default)]
    query: String,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct ToArgs {
    path: PathArg,
    #[serde(default)]
    pane: Option<PaneId>,
}

/// Recently used files that still exist. Worker-only.
fn recent_files() -> Vec<xbel::Recent> {
    xbel::default_path()
        .map(|p| xbel::existing(xbel::load(&p)))
        .unwrap_or_default()
}

/// Open a folder, or a file's folder with the file selected. Whether the
/// path is a folder is asked on a worker (a sleeping disk can take a
/// while); the name is selected when the listing brings it.
fn jump_to(path: PathBuf, pane: Option<PaneId>, app: &App) -> anyhow::Result<Outcome> {
    anyhow::ensure!(
        path.is_absolute(),
        "{} is not an absolute path",
        path.display()
    );
    let pane = app.pane_or_active(pane);
    let weak = app.weak();
    app.scheduler.spawn(
        Priority::Normal,
        "jump.to",
        app.lifetime().child(),
        move || {
            let Some(app) = weak.upgrade() else { return };
            if let Err(e) = go(&app, pane, &path) {
                app.notify(NotifyLevel::Error, format!("{e:#}"));
            }
        },
    );
    Ok(Outcome::Done)
}

fn go(app: &App, pane: PaneId, path: &Path) -> anyhow::Result<()> {
    let is_dir = std::fs::metadata(path)
        .map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?
        .is_dir();
    if is_dir {
        return app.navigate(pane, &Location::from_path(path));
    }
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        anyhow::bail!("{} has no folder", path.display());
    };
    app.navigate(pane, &Location::from_path(parent))?;
    if let Some(p) = app.workspace.write().pane_mut(pane) {
        p.reveal(name.to_owned());
    }
    app.wake();
    Ok(())
}

impl Plugin for JumpPlugin {
    fn id(&self) -> &'static str {
        ID
    }
    fn title(&self) -> &'static str {
        "Jump"
    }
    fn description(&self) -> &'static str {
        "Frecency jump from zoxide and recently used files; the recent: location"
    }
    fn category(&self) -> bibli_core::plugin::Category {
        bibli_core::plugin::Category::Browsing
    }
    fn register(&self, app: &App) {
        let lifetime = app.lifetime().child();
        *self.lifetime.lock() = lifetime.clone();
        refresh(&self.index, &lifetime, app);
        let index = Arc::clone(&self.index);
        app.commands.register::<CandidatesArgs, _>(
            ID,
            "jump.candidates",
            "Jump candidates",
            "Rank jump targets for a query, from what zoxide and the recent files list said last",
            move |a, app| {
                let ranked = {
                    let ix = index.lock();
                    rank(&a.query, &ix.dirs, &ix.files, a.limit.unwrap_or(20))
                };
                refresh(&index, &lifetime, app);
                Ok(Outcome::Value(serde_json::to_value(ranked)?))
            },
        );
        app.commands.register::<ToArgs, _>(
            ID,
            "jump.to",
            "Jump to",
            "Go to a directory, or to a file's directory selecting it",
            |a, app| jump_to(a.path.into_inner(), a.pane, app),
        );
        app.register_provider(ID, Arc::new(RecentProvider));
        app.registry.places.add(
            ID,
            Place::new(
                "places",
                RECENT,
                "document-open-recent",
                Location::parse("recent:"),
            ),
        );
        app.registry.keymap.add(
            ID,
            KeyBinding {
                keys: "ctrl-j".into(),
                command: Invocation::new("ui.jump"),
                when: None,
            },
        );
    }

    fn on_disable(&self, _app: &App) {
        self.lifetime.lock().cancel();
        *self.index.lock() = Index::default();
    }
}

// ---------------------------------------------------------------- recent: location

/// Provider for `recent:`.
#[derive(Debug, Default)]
pub struct RecentProvider;

impl LocationProvider for RecentProvider {
    fn scheme(&self) -> &'static str {
        "recent"
    }

    fn title(&self, _location: &Location) -> Option<String> {
        Some(RECENT.into())
    }

    fn icon(&self, _location: &Location) -> Option<&'static str> {
        Some("recent")
    }

    fn empty_state(&self, _location: &Location) -> Option<EmptyState> {
        Some(EmptyState::new(
            "No recent files",
            "The files you open in your applications are listed here, as the desktop records them.",
        ))
    }
    fn open(&self, location: &Location, ctx: OpenContext) -> anyhow::Result<Arc<dyn OpenLocation>> {
        Ok(RecentLocation::open(location.clone(), ctx))
    }
}

struct RecentLocation {
    location: Location,
    ctx: OpenContext,
    tx: EventSink,
    rx: Receiver<ListingEvent>,
    paths: Arc<Mutex<HashMap<EntryId, PathBuf>>>,
}

impl RecentLocation {
    fn open(location: Location, ctx: OpenContext) -> Arc<dyn OpenLocation> {
        let (tx, rx) = ctx.channel();
        let this = Arc::new(RecentLocation {
            location,
            ctx,
            tx,
            rx,
            paths: Arc::new(Mutex::new(HashMap::new())),
        });
        this.list();
        this
    }

    fn list(&self) {
        let tx = self.tx.clone();
        let paths = Arc::clone(&self.paths);
        let cancel = self.ctx.cancel.child();
        let batch_size = self.ctx.batch_size.max(1);
        self.ctx.scheduler.spawn(
            Priority::Visible,
            "recent.list",
            cancel.clone(),
            move || {
                // Each is stat-ed below: what is gone is left out there.
                let items = xbel::default_path()
                    .map(|p| xbel::load(&p))
                    .unwrap_or_default();
                let mut batch = Vec::new();
                let mut seen = std::collections::HashSet::new();
                let mut map = HashMap::new();
                for r in items {
                    if cancel.is_cancelled() {
                        return;
                    }
                    let Some(e) = bibli_core::fs::stat::stat_path(&r.path) else {
                        continue;
                    };
                    if !seen.insert(e.id) {
                        continue;
                    }
                    map.insert(e.id, r.path.clone());
                    batch.push(e);
                    if batch.len() >= batch_size {
                        tx.send(ListingEvent::Added(std::mem::take(&mut batch)));
                    }
                }
                let prev: Vec<EntryId> = {
                    let mut p = paths.lock();
                    let prev = p
                        .keys()
                        .filter(|k| !map.contains_key(*k))
                        .copied()
                        .collect();
                    *p = map;
                    prev
                };
                if !batch.is_empty() {
                    tx.send(ListingEvent::Added(batch));
                }
                if !prev.is_empty() {
                    tx.send(ListingEvent::Removed(prev));
                }
                tx.send(ListingEvent::Complete);
            },
        );
    }
}

impl OpenLocation for RecentLocation {
    fn location(&self) -> &Location {
        &self.location
    }
    fn events(&self) -> &Receiver<ListingEvent> {
        &self.rx
    }
    fn hydrate(&self, ids: &[EntryId], priority: Priority) {
        let wanted: Vec<(EntryId, PathBuf)> = {
            let p = self.paths.lock();
            ids.iter()
                .filter_map(|id| p.get(id).map(|path| (*id, path.clone())))
                .collect()
        };
        if wanted.is_empty() {
            return;
        }
        let tx = self.tx.clone();
        let metadata = Arc::clone(&self.ctx.metadata);
        let paths = Arc::clone(&self.paths);
        let cancel = self.ctx.cancel.child();
        self.ctx
            .scheduler
            .spawn(priority, "recent.stat", cancel.clone(), move || {
                let gone = hydrate_paths(&wanted, &metadata, &tx, &cancel);
                let mut paths = paths.lock();
                for id in gone {
                    paths.remove(&id);
                }
            });
    }
    fn path_of(&self, id: EntryId) -> Option<PathBuf> {
        self.paths.lock().get(&id).cloned()
    }
    fn refresh(&self) {
        self.list();
    }
    fn close(&self) {
        self.ctx.cancel.cancel();
    }
}

#[cfg(test)]
// Helpers of the tests that run on Linux alone (ADR-035).
#[cfg_attr(not(target_os = "linux"), allow(dead_code, unused_imports))]
mod tests {
    use super::*;
    use bibli_core::app::AppConfig;
    use std::time::Duration;

    fn app(root: &Path) -> Arc<App> {
        App::new(AppConfig::ephemeral(root)).unwrap()
    }

    #[test]
    fn ranking_combines_fuzzy_match_and_source_score() {
        let dirs = vec![
            zoxide::ZEntry {
                score: 50.0,
                path: "/home/x/projects/bibli".into(),
            },
            zoxide::ZEntry {
                score: 5.0,
                path: "/home/x/bibliography".into(),
            },
        ];
        let files = vec![xbel::Recent {
            path: "/home/x/Downloads/bibli-notes.txt".into(),
            mime: None,
            visited: SystemTime::now(),
        }];
        let r = rank("bibli", &dirs, &files, 10);
        assert_eq!(r.len(), 3);
        assert_eq!(
            r[0].path,
            PathBuf::from("/home/x/projects/bibli"),
            "highest zoxide score wins among matches"
        );
        assert!(rank("zzzzqq", &dirs, &files, 10).is_empty());
        let all = rank("", &dirs, &files, 2);
        assert_eq!(all.len(), 2, "limit applies");
    }

    #[cfg(unix)]
    #[test]
    fn candidates_command_uses_a_fake_zoxide_on_path() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let script = bin.join("zoxide");
        std::fs::write(
            &script,
            "#!/bin/sh\nprintf '  9.5 /tmp/fake-alpha\\n  1.0 /tmp/fake-beta\\n'\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let old = std::env::var_os("PATH").unwrap_or_default();
        let mut paths = vec![bin.clone()];
        paths.extend(std::env::split_paths(&old));
        // Tests run one per process under nextest; setting the environment is sound here.
        #[allow(unsafe_code)]
        unsafe {
            std::env::set_var("PATH", std::env::join_paths(paths).unwrap());
        }
        let app = app(tmp.path());
        app.plugins.add(Arc::new(JumpPlugin::default()), &app);
        // The index is gathered on a worker; the command only ranks it.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let arr = loop {
            let Outcome::Value(v) = app
                .dispatch(&Invocation::with_args(
                    "jump.candidates",
                    &serde_json::json!({ "query": "alpha" }),
                ))
                .unwrap()
            else {
                panic!("expected value")
            };
            let arr = v.as_array().unwrap().clone();
            if !arr.is_empty() {
                break arr;
            }
            assert!(std::time::Instant::now() < deadline, "no candidates");
            std::thread::sleep(Duration::from_millis(10));
        };
        assert!(
            arr.iter()
                .any(|c| c["path"] == "/tmp/fake-alpha" && c["source"] == "zoxide")
        );
        assert!(!arr.iter().any(|c| c["path"] == "/tmp/fake-beta"));
        app.plugins.set_enabled(ID, false, &app);
        app.plugins.set_enabled(ID, true, &app);
        assert!(app.commands.get("jump.candidates").is_some());
    }

    // The free desktops' recent files (step E, ADR-035).
    #[cfg(target_os = "linux")]
    #[test]
    fn recent_location_streams_existing_files_and_jump_to_selects() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path().join("data");
        std::fs::create_dir_all(&data).unwrap();
        let f1 = tmp.path().join("one.txt");
        std::fs::write(&f1, b"1").unwrap();
        let xbel = format!(
            r#"<?xml version="1.0"?><xbel version="1.0"><bookmark href="file://{}" visited="2026-01-01T00:00:00Z"/><bookmark href="file:///nonexistent/zz" visited="2026-01-02T00:00:00Z"/></xbel>"#,
            f1.display()
        );
        std::fs::write(data.join("recently-used.xbel"), xbel).unwrap();
        #[allow(unsafe_code)]
        unsafe {
            std::env::set_var("XDG_DATA_HOME", &data);
        }
        let app = app(tmp.path());
        app.plugins.add(Arc::new(JumpPlugin::default()), &app);
        let pane = app.workspace.read().active_id();
        let recent = Location::parse("recent:");
        // It reads as one place in the address, under its own glyph.
        let crumbs = app.trail(&recent).unwrap().crumbs;
        assert_eq!(crumbs, [bibli_core::Crumb::new("Recent", recent.clone())]);
        assert_eq!(app.location_icon(&recent), Some("recent"));
        app.navigate(pane, &recent).unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
        {
            let ws = app.workspace.read();
            let l = &ws.pane(pane).unwrap().listing;
            assert_eq!(l.len(), 1);
            assert_eq!(l.iter().next().unwrap().name, "one.txt");
            let id = l.ids().next().unwrap();
            assert_eq!(
                ws.pane(pane).unwrap().open.as_ref().unwrap().path_of(id),
                Some(f1.clone())
            );
        }
        // jump.to a file: parent opened, file selected once listed.
        app.dispatch(&Invocation::with_args(
            "jump.to",
            &serde_json::json!({ "path": PathArg::from(f1.as_path()) }),
        ))
        .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            app.pump();
            let ws = app.workspace.read();
            let p = ws.pane(pane).unwrap();
            if p.location().and_then(Location::path).as_deref() == Some(tmp.path())
                && p.selection.len() == 1
            {
                let id = p.selection.iter().next().unwrap();
                assert_eq!(p.listing.get(id).unwrap().name, "one.txt");
                break;
            }
            drop(ws);
            assert!(std::time::Instant::now() < deadline, "never selected");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            app.registry
                .places
                .all()
                .iter()
                .any(|pl| pl.title == "Recent")
        );
    }
}
