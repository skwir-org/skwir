//! bibli plugin: jump. Frecency jump that reuses what the environment already
//! tracks: zoxide's directory ranking and the desktop's recently used files.
//! Also provides the `recent:` location listing those files.

pub mod xbel;
pub mod zoxide;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use bibli_core::app::App;
use bibli_core::command::{Invocation, Outcome};
use bibli_core::entry::{Entry, EntryId};
use bibli_core::location::{
    EventSink, ListingEvent, Location, LocationProvider, OpenContext, OpenLocation,
};
use bibli_core::plugin::Plugin;
use bibli_core::registry::{KeyBinding, Place};
use bibli_core::scheduler::Priority;
use bibli_core::workspace::PaneId;
use crossbeam_channel::Receiver;
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use parking_lot::Mutex;

const ID: &str = "jump";

/// A ranked jump target.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Candidate {
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

/// The jump plugin.
#[derive(Default)]
pub struct JumpPlugin {
    zoxide: Arc<zoxide::ZoxideCache>,
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
    path: String,
    #[serde(default)]
    pane: Option<PaneId>,
}

fn recent_files() -> Vec<xbel::Recent> {
    xbel::default_path()
        .map(|p| xbel::existing(xbel::load(&p)))
        .unwrap_or_default()
}

fn jump_to(path: &Path, pane: Option<PaneId>, app: &App) -> anyhow::Result<Outcome> {
    let pane = pane.unwrap_or_else(|| app.workspace.read().active_id());
    if path.is_dir() {
        app.navigate(pane, &Location::from_path(path))?;
        return Ok(Outcome::Done);
    }
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("{} has no parent", path.display()))?;
    let name = path
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("{} has no name", path.display()))?;
    app.navigate(pane, &Location::from_path(parent))?;
    // Select the file once the listing has it; the listing streams, so wait briefly.
    if app.pump_until_complete(pane, std::time::Duration::from_secs(5)) {
        let inv = Invocation::with_args(
            "select",
            &serde_json::json!({ "names": [name.to_string_lossy()], "pane": pane }),
        );
        let _ = app.dispatch(&inv);
    }
    Ok(Outcome::Done)
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
    fn register(&self, app: &App) {
        let zx = Arc::clone(&self.zoxide);
        app.commands.register::<CandidatesArgs, _>(
            ID,
            "jump.candidates",
            "Jump candidates",
            "Rank jump targets for a query",
            move |a, _| {
                let dirs = zx.get();
                let files = recent_files();
                Ok(Outcome::Value(serde_json::to_value(rank(
                    &a.query,
                    &dirs,
                    &files,
                    a.limit.unwrap_or(20),
                ))?))
            },
        );
        app.commands.register::<ToArgs, _>(
            ID,
            "jump.to",
            "Jump to",
            "Go to a directory, or to a file's directory selecting it",
            |a, app| jump_to(Path::new(&a.path), a.pane, app),
        );
        app.register_provider(ID, Arc::new(RecentProvider));
        app.registry.places.add(
            ID,
            Place {
                title: "Recent".into(),
                location: Location::parse("recent:"),
                icon: Some("document-open-recent".into()),
                group: "places".into(),
            },
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
}

// ---------------------------------------------------------------- recent: location

/// Provider for `recent:`.
#[derive(Debug, Default)]
pub struct RecentProvider;

impl LocationProvider for RecentProvider {
    fn scheme(&self) -> &'static str {
        "recent"
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
                let items = recent_files();
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
        self.ctx.scheduler.spawn(
            priority,
            "recent.stat",
            self.ctx.cancel.child(),
            move || {
                let mut updated: Vec<Entry> = Vec::new();
                let mut gone = Vec::new();
                for (id, path) in wanted {
                    match bibli_core::fs::stat::stat_path(&path) {
                        Some(mut e) => {
                            e.id = id;
                            if let Some(dir) = path.parent() {
                                metadata.fill(dir, std::slice::from_mut(&mut e));
                            }
                            updated.push(e);
                        }
                        None => gone.push(id),
                    }
                }
                if !updated.is_empty() {
                    tx.send(ListingEvent::Updated(updated));
                }
                if !gone.is_empty() {
                    tx.send(ListingEvent::Removed(gone));
                }
            },
        );
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
        let out = app
            .dispatch(&Invocation::with_args(
                "jump.candidates",
                &serde_json::json!({ "query": "alpha" }),
            ))
            .unwrap();
        let Outcome::Value(v) = out else {
            panic!("expected value")
        };
        let arr = v.as_array().unwrap();
        assert!(
            arr.iter()
                .any(|c| c["path"] == "/tmp/fake-alpha" && c["source"] == "zoxide")
        );
        assert!(!arr.iter().any(|c| c["path"] == "/tmp/fake-beta"));
    }

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
        app.navigate(pane, &Location::parse("recent:")).unwrap();
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
        // jump.to a file: parent opened, file selected.
        app.dispatch(&Invocation::with_args(
            "jump.to",
            &serde_json::json!({ "path": f1.to_string_lossy() }),
        ))
        .unwrap();
        let ws = app.workspace.read();
        let p = ws.pane(pane).unwrap();
        assert_eq!(p.location().unwrap().path().unwrap(), tmp.path());
        assert_eq!(p.selection.len(), 1);
        assert!(
            app.registry
                .places
                .all()
                .iter()
                .any(|pl| pl.title == "Recent")
        );
    }
}
