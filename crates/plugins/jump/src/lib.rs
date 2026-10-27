//! skwir plugin: jump. Frecency jump that reuses what the environment already
//! tracks: zoxide's directory ranking and the desktop's recently used files.
//! Also provides the `recent:` location listing those files.

#[cfg(test)]
mod any_input;
pub mod xbel;
pub mod zoxide;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use crossbeam_channel::Receiver;
use nucleo_matcher::pattern::{Atom, AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use parking_lot::Mutex;
use skwir_core::CancelToken;
use skwir_core::app::{App, NotifyLevel};
use skwir_core::command::{Invocation, Outcome, Topic};
use skwir_core::entry::EntryId;
use skwir_core::fs::hydrate_paths;
use skwir_core::location::{
    EmptyState, EventSink, ListingEvent, Location, LocationProvider, OpenContext, OpenLocation,
};
use skwir_core::os_path::PathArg;
use skwir_core::plugin::Plugin;
use skwir_core::registry::{KeyBinding, Place};
use skwir_core::scheduler::Priority;
use skwir_core::workspace::PaneId;

const ID: &str = "jump";
/// What `recent:` is called, in the sidebar and wherever it is shown.
const RECENT: &str = "Recent";

/// A ranked jump target.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Candidate {
    #[serde(with = "skwir_core::os_path")]
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
    let mut out: Vec<Candidate> = used(dirs, files)
        .filter_map(|mut c| {
            c.score *= fuzzy(&c.path.to_string_lossy())?;
            Some(c)
        })
        .collect();
    out.sort_by(|a, b| b.score.total_cmp(&a.score));
    out.truncate(limit);
    out
}

/// Rank for the address field (ADR-098), where words are mostly a search:
/// each word is found unbroken in the path as a row shows it (from `~`
/// under `home`, so the home folder's own path matches nothing), in the
/// name before its folders, at a word's start before within one. Among
/// equals the most used come first, as with nothing typed.
pub fn rank_by_words(
    words: &str,
    home: Option<&Path>,
    dirs: &[zoxide::ZEntry],
    files: &[xbel::Recent],
    limit: usize,
) -> Vec<Candidate> {
    let atoms: Vec<Atom> = words
        .split_whitespace()
        .map(|w| {
            Atom::new(
                w,
                CaseMatching::Smart,
                Normalization::Smart,
                AtomKind::Substring,
                false,
            )
        })
        .collect();
    let mut matcher = Matcher::new(Config::DEFAULT);
    let mut found: Vec<(u32, Candidate)> = used(dirs, files)
        .filter_map(|c| {
            let name = c
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let folders = c
                .path
                .parent()
                .map(|p| shown_from(p, home))
                .unwrap_or_default();
            let mut points = 0;
            for atom in &atoms {
                points += match found_in(atom, &name, &mut matcher) {
                    Some(start) => 2 + u32::from(start),
                    None => u32::from(found_in(atom, &folders, &mut matcher)?),
                };
            }
            Some((points, c))
        })
        .collect();
    found.sort_by(|(a, x), (b, y)| b.cmp(a).then(y.score.total_cmp(&x.score)));
    found.into_iter().take(limit).map(|(_, c)| c).collect()
}

/// Whether `atom` is found in `text`, and found at a word's start: after
/// a character that is no letter or digit, or as a capital after a small
/// letter.
fn found_in(atom: &Atom, text: &str, matcher: &mut Matcher) -> Option<bool> {
    let mut buf = Vec::new();
    let mut at = Vec::new();
    let text = Utf32Str::new(text, &mut buf);
    atom.indices(text, matcher, &mut at)?;
    let first = at.first().copied()?;
    if first == 0 {
        return Some(true);
    }
    let (before, here) = (text.get(first - 1), text.get(first));
    Some(!before.is_alphanumeric() || (before.is_lowercase() && here.is_uppercase()))
}

/// What zoxide and the desktop's recent files know, scored by use alone:
/// a folder by zoxide's score, a file by how lately it was opened.
fn used<'a>(
    dirs: &'a [zoxide::ZEntry],
    files: &'a [xbel::Recent],
) -> impl Iterator<Item = Candidate> + 'a {
    let now = SystemTime::now();
    let dirs = dirs.iter().map(|d| Candidate {
        path: d.path.clone(),
        score: 1.0 + d.score,
        kind: "dir",
        source: "zoxide",
    });
    let files = files.iter().map(move |r| {
        let days = now
            .duration_since(r.visited)
            .map_or(f64::MAX, |d| d.as_secs_f64() / 86_400.0);
        Candidate {
            path: r.path.clone(),
            score: 1.0 + 1.0 / (1.0 + days),
            kind: "file",
            source: "recent",
        }
    });
    dirs.chain(files)
}

/// How much the user goes to a name of a folder (`jump.scores`).
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Score {
    #[serde(with = "skwir_core::os_path")]
    pub path: PathBuf,
    pub score: f64,
}

/// What zoxide and the recent files say of the names in `folder`: a
/// folder's score is zoxide's for it and for every folder within it, so
/// the way to where the user goes ranks first; a file's is how recently
/// it was used. Names neither knows are left out.
pub fn scores_in(folder: &Path, dirs: &[zoxide::ZEntry], files: &[xbel::Recent]) -> Vec<Score> {
    let now = SystemTime::now();
    let mut by: HashMap<PathBuf, f64> = HashMap::new();
    for d in dirs {
        if let Ok(within) = d.path.strip_prefix(folder)
            && let Some(std::path::Component::Normal(name)) = within.components().next()
        {
            *by.entry(folder.join(name)).or_default() += d.score;
        }
    }
    for r in files.iter().filter(|r| r.path.parent() == Some(folder)) {
        let days = now
            .duration_since(r.visited)
            .map_or(f64::MAX, |d| d.as_secs_f64() / 86_400.0);
        *by.entry(r.path.clone()).or_default() += 1.0 / (1.0 + days);
    }
    by.into_iter()
        .map(|(path, score)| Score { path, score })
        .collect()
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

/// Whether the index was ever gathered, and the gathering under way.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum Gathered {
    #[default]
    Not,
    Running,
    Done,
}

/// The first gathering of the index: those asked meanwhile wait for it on
/// the condition, never on a lock held across zoxide and the stats.
#[derive(Default)]
struct FirstGathering {
    state: Mutex<Gathered>,
    ended: parking_lot::Condvar,
}

/// The jump plugin.
#[derive(Default)]
pub struct JumpPlugin {
    index: Arc<Mutex<Index>>,
    /// Whether the index was ever gathered: nothing is asked of zoxide or
    /// the recent files until Jump is first used.
    gathered: Arc<FirstGathering>,
    /// Cancelled on disable: a refresh in flight stops and publishes nothing.
    lifetime: Mutex<CancelToken>,
}

/// Gather the index now, the first time it is needed, unless it was: a
/// first query waits for it (on a worker, as the window asks) rather than
/// ranking nothing, and those asked meanwhile wait for the same gathering,
/// as long as zoxide may take ([`zoxide::TIMEOUT`]), then rank what there
/// is. On the window's thread it is left to a worker, and what there is
/// ranked.
fn first_need(
    gathered: &FirstGathering,
    index: &Arc<Mutex<Index>>,
    lifetime: &CancelToken,
    app: &App,
) {
    if skwir_core::scheduler::on_window_thread() {
        if *gathered.state.lock() == Gathered::Not {
            refresh(index, lifetime, app);
        }
        return;
    }
    {
        let mut state = gathered.state.lock();
        let deadline = Instant::now() + zoxide::TIMEOUT;
        while *state == Gathered::Running {
            if gathered.ended.wait_until(&mut state, deadline).timed_out() {
                return;
            }
        }
        if *state == Gathered::Done {
            return;
        }
        *state = Gathered::Running;
    }
    let cancel = lifetime.child();
    let dirs = zoxide::query(&cancel);
    let files = recent_files();
    let done = !cancel.is_cancelled();
    if done {
        let mut ix = index.lock();
        ix.dirs = dirs;
        ix.files = files;
        ix.asked = Some(Instant::now());
    }
    *gathered.state.lock() = if done { Gathered::Done } else { Gathered::Not };
    gathered.ended.notify_all();
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
struct ScoresArgs {
    /// The folder whose names are scored.
    folder: PathArg,
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
/// path is a folder is asked on a worker, in its lane (a sleeping disk or
/// a server can take a while); the name is selected when the listing
/// brings it. The pane's: going elsewhere meanwhile, Back or closing it
/// lets the jump go.
fn jump_to(path: PathBuf, pane: Option<PaneId>, app: &App) -> anyhow::Result<Outcome> {
    anyhow::ensure!(
        path.is_absolute(),
        "{} is not an absolute path",
        path.display()
    );
    let pane = app.pane_or_active(pane);
    let visit = app
        .workspace
        .read()
        .pane(pane)
        .map(skwir_core::workspace::Pane::location_lifetime)
        .ok_or_else(|| anyhow::anyhow!("no such pane"))?;
    let cancel = app.lifetime().linked(&visit);
    let weak = app.weak();
    app.scheduler.spawn_in(
        skwir_core::fs::reach::lane(&path),
        Priority::Normal,
        "jump.to",
        cancel.clone(),
        move || {
            let Some(app) = weak.upgrade() else { return };
            if let Err(e) = go(&app, pane, &path, &cancel) {
                app.notify(NotifyLevel::Error, format!("{e:#}"));
            }
        },
    );
    Ok(Outcome::Done)
}

fn go(app: &App, pane: PaneId, path: &Path, cancel: &CancelToken) -> anyhow::Result<()> {
    let is_dir = std::fs::metadata(path)
        .map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?
        .is_dir();
    if cancel.is_cancelled() {
        return Ok(());
    }
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

/// What the address field offers of what Jump knows (ADR-098): the folders
/// and files one goes to whose paths hold every word (`rank_by_words`);
/// before anything is typed with no scope, those one goes to most.
struct Places {
    index: Arc<Mutex<Index>>,
    gathered: Arc<FirstGathering>,
    lifetime: CancelToken,
}

/// How many places the address field shows.
const FIELD_PLACES: usize = 5;

/// A folder as a row says it: from `~` under `home`.
fn shown_from(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

impl skwir_core::suggest::SuggestSource for Places {
    fn id(&self) -> &'static str {
        ID
    }

    fn answers(&self, ask: &skwir_core::suggest::Ask) -> bool {
        use skwir_core::suggest::Typed;
        match ask.typed {
            Typed::Words => ask.words().chars().count() >= 2,
            Typed::Nothing => ask.scope.is_none(),
            _ => false,
        }
    }

    fn suggest(
        &self,
        app: &App,
        ask: &skwir_core::suggest::Ask,
        _: &CancelToken,
    ) -> skwir_core::suggest::Answer {
        use skwir_core::suggest::{Answer, Section, Suggestion, Typed};
        first_need(&self.gathered, &self.index, &self.lifetime, app);
        let home = skwir_core::os_path::home();
        let ranked = {
            let ix = self.index.lock();
            if ask.typed == Typed::Words {
                let (dirs, files) = (&ix.dirs, &ix.files);
                rank_by_words(ask.words(), home.as_deref(), dirs, files, FIELD_PLACES + 1)
            } else {
                rank("", &ix.dirs, &ix.files, FIELD_PLACES + 1)
            }
        };
        refresh(&self.index, &self.lifetime, app);
        let shown = ask.dir.as_deref();
        let rows = ranked
            .into_iter()
            .filter(|c| Some(c.path.as_path()) != shown)
            .take(FIELD_PLACES)
            .enumerate()
            .map(|(i, c)| {
                let dir = c.kind == "dir";
                let name = c.path.file_name().map_or_else(
                    || c.path.display().to_string(),
                    |n| n.to_string_lossy().into_owned(),
                );
                let mut row = Suggestion::new(Section::Go, name)
                    .icon(if dir { "folder" } else { "open" })
                    .rank(i32::try_from(i).unwrap_or(i32::MAX) + 20)
                    .runs(Invocation::with_args(
                        "jump.to",
                        &serde_json::json!({ "path": PathArg::from(c.path.clone()), "pane": ask.pane }),
                    ));
                if let Some(parent) = c.path.parent() {
                    row.detail = Some(shown_from(parent, home.as_deref()));
                }
                if dir {
                    row.scope = Some(Location::from_path(&c.path));
                } else {
                    row.open = Some(Invocation::with_args(
                        "open",
                        &serde_json::json!({ "entries": [PathArg::from(c.path.clone())], "pane": ask.pane }),
                    ));
                }
                row
            })
            .collect();
        Answer {
            rows,
            spans: Vec::new(),
        }
    }
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
    fn category(&self) -> skwir_core::plugin::Category {
        skwir_core::plugin::Category::Browsing
    }
    fn capabilities(&self) -> skwir_core::permissions::Capabilities {
        use skwir_core::permissions::Capability;
        skwir_core::permissions::Capabilities::of(&[Capability::Programs])
    }
    fn register(&self, app: &App) {
        let lifetime = app.lifetime().child();
        *self.lifetime.lock() = lifetime.clone();
        // The places one goes to, offered by the address field (ADR-098).
        app.registry.suggesters.add(
            ID,
            skwir_core::suggest::Suggester::new(Places {
                index: Arc::clone(&self.index),
                gathered: Arc::clone(&self.gathered),
                lifetime: lifetime.clone(),
            }),
        );
        let index = Arc::clone(&self.index);
        let gathered = Arc::clone(&self.gathered);
        app.commands.register::<CandidatesArgs, _>(
            ID, Topic::Go,
            "jump.candidates",
            "Jump candidates",
            "Rank jump targets for a query, from what zoxide and the recent files list said last",
            move |a, app| {
                first_need(&gathered, &index, &lifetime, app);
                let ranked = {
                    let ix = index.lock();
                    rank(&a.query, &ix.dirs, &ix.files, a.limit.unwrap_or(20))
                };
                refresh(&index, &lifetime, app);
                Ok(Outcome::Value(serde_json::to_value(ranked)?))
            },
        )
        .data();
        let index = Arc::clone(&self.index);
        let gathered = Arc::clone(&self.gathered);
        let lifetime = self.lifetime.lock().clone();
        app.commands.register::<ScoresArgs, _>(
            ID, Topic::Go,
            "jump.scores",
            "Jump scores in a folder",
            "How much the user goes to each name of a folder, from what zoxide and the recent files list said last: a folder counts the folders within it",
            move |a, app| {
                first_need(&gathered, &index, &lifetime, app);
                let scores = {
                    let ix = index.lock();
                    scores_in(&a.folder.into_inner(), &ix.dirs, &ix.files)
                };
                refresh(&index, &lifetime, app);
                Ok(Outcome::Value(serde_json::to_value(scores)?))
            },
        )
        .data();
        app.commands
            .register::<ToArgs, _>(
                ID,
                Topic::Go,
                "jump.to",
                "Jump to",
                "Go to a directory, or to a file's directory selecting it",
                |a, app| jump_to(a.path.into_inner(), a.pane, app),
            )
            .shows();
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
        *self.gathered.state.lock() = Gathered::Not;
        self.gathered.ended.notify_all();
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

    /// List the recent files: stat-ed to be listed by their identity, each
    /// remote's in its lane (ADR-077), so a server that stops answering
    /// holds its own files, never the listing of the others: what is on
    /// this machine is listed, then complete; a remote's come as its
    /// server answers. What is gone is left out.
    fn list(&self) {
        let tx = self.tx.clone();
        let paths = Arc::clone(&self.paths);
        let cancel = self.ctx.cancel.child();
        let batch_size = self.ctx.batch_size.max(1);
        let scheduler = self.ctx.scheduler.clone();
        self.ctx.scheduler.spawn(
            Priority::Visible,
            "recent.list",
            cancel.clone(),
            move || {
                let items = xbel::default_path()
                    .map(|p| xbel::load(&p))
                    .unwrap_or_default();
                let wanted: std::collections::HashSet<PathBuf> =
                    items.iter().map(|r| r.path.clone()).collect();
                let listing = Listed {
                    tx,
                    paths,
                    seen: Arc::default(),
                    cancel: cancel.clone(),
                    batch_size,
                };
                let mut here = Vec::new();
                for (lane, items) in skwir_core::fs::reach::by_lane(items, |r| &r.path) {
                    let items: Vec<PathBuf> = items.into_iter().map(|r| r.path).collect();
                    if lane.is_shared() {
                        here = items;
                        continue;
                    }
                    let listing = listing.clone();
                    scheduler.spawn_in(
                        lane,
                        Priority::Visible,
                        "recent.list",
                        cancel.clone(),
                        move || listing.stat(&items),
                    );
                }
                listing.stat(&here);
                // What was listed before and no longer is recent.
                let prev: Vec<EntryId> = {
                    let mut p = listing.paths.lock();
                    let prev: Vec<EntryId> = p
                        .iter()
                        .filter(|(_, path)| !wanted.contains(*path))
                        .map(|(id, _)| *id)
                        .collect();
                    for id in &prev {
                        p.remove(id);
                    }
                    prev
                };
                if cancel.is_cancelled() {
                    return;
                }
                if !prev.is_empty() {
                    listing.tx.send(ListingEvent::Removed(prev));
                }
                listing.tx.send(ListingEvent::Complete);
            },
        );
    }
}

/// One listing of the recent files, shared by its lanes' jobs.
#[derive(Clone)]
struct Listed {
    tx: EventSink,
    paths: Arc<Mutex<HashMap<EntryId, PathBuf>>>,
    /// The files listed, once each: two names of one file are one row.
    seen: Arc<Mutex<std::collections::HashSet<EntryId>>>,
    cancel: CancelToken,
    batch_size: usize,
}

impl Listed {
    /// Stat `items` and send them, in batches; what was listed at their
    /// paths before and is not now (gone, or another file there) is taken
    /// out.
    fn stat(&self, items: &[PathBuf]) {
        let mut batch = Vec::new();
        for path in items {
            if self.cancel.is_cancelled() {
                return;
            }
            let Some(e) = skwir_core::fs::stat::stat_path(path) else {
                continue;
            };
            if !self.seen.lock().insert(e.id) {
                continue;
            }
            self.paths.lock().insert(e.id, path.clone());
            batch.push(e);
            if batch.len() >= self.batch_size {
                self.tx
                    .send(ListingEvent::Added(std::mem::take(&mut batch)));
            }
        }
        let asked: std::collections::HashSet<&PathBuf> = items.iter().collect();
        let removed: Vec<EntryId> = {
            let seen = self.seen.lock();
            let mut p = self.paths.lock();
            let removed: Vec<EntryId> = p
                .iter()
                .filter(|(id, path)| asked.contains(path) && !seen.contains(*id))
                .map(|(id, _)| *id)
                .collect();
            for id in &removed {
                p.remove(id);
            }
            removed
        };
        if self.cancel.is_cancelled() {
            return;
        }
        if !batch.is_empty() {
            self.tx.send(ListingEvent::Added(batch));
        }
        if !removed.is_empty() {
            self.tx.send(ListingEvent::Removed(removed));
        }
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
        // A job for each slice, side by side on the workers, each remote's
        // in its lane.
        for (lane, wanted) in skwir_core::fs::reach::by_lane(wanted, |(_, path)| path) {
            for wanted in wanted.chunks(skwir_core::fs::HYDRATE_JOB) {
                let wanted = wanted.to_vec();
                let tx = self.tx.clone();
                let metadata = Arc::clone(&self.ctx.metadata);
                let paths = Arc::clone(&self.paths);
                let cancel = self.ctx.cancel.child();
                self.ctx.scheduler.spawn_in(
                    lane,
                    priority,
                    "recent.stat",
                    cancel.clone(),
                    move || {
                        let gone = hydrate_paths(&wanted, &metadata, &tx, &cancel);
                        let mut paths = paths.lock();
                        for id in gone {
                            paths.remove(&id);
                        }
                    },
                );
            }
        }
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
    use skwir_core::app::AppConfig;
    use std::time::Duration;

    fn app(root: &Path) -> Arc<App> {
        App::new(AppConfig::ephemeral(root)).unwrap()
    }

    #[test]
    fn a_folder_scores_what_zoxide_knows_within_it_and_a_file_its_recency() {
        let z = |score, path: &str| zoxide::ZEntry {
            score,
            path: path.into(),
        };
        let dirs = vec![
            z(40.0, "/w/proj/app"),
            z(2.0, "/w/proj"),
            z(10.0, "/w/proj/lib/deep"),
            z(7.0, "/w/notes"),
            z(99.0, "/elsewhere"),
            z(5.0, "/w"),
        ];
        let files = vec![
            xbel::Recent {
                path: "/w/todo.txt".into(),
                mime: None,
                visited: SystemTime::now(),
            },
            xbel::Recent {
                path: "/w/proj/readme.md".into(),
                mime: None,
                visited: SystemTime::now(),
            },
        ];
        let mut scores = scores_in(Path::new("/w"), &dirs, &files);
        scores.sort_by(|a, b| a.path.cmp(&b.path));
        // As paths: Windows joins a name with `\\`.
        let paths: Vec<&Path> = scores.iter().map(|s| s.path.as_path()).collect();
        assert_eq!(paths, ["/w/notes", "/w/proj", "/w/todo.txt"].map(Path::new));
        assert!((scores[1].score - 52.0).abs() < 1e-9, "its own and within");
        assert!((scores[2].score - 1.0).abs() < 0.01, "used just now");
    }

    #[test]
    fn ranking_combines_fuzzy_match_and_source_score() {
        let dirs = vec![
            zoxide::ZEntry {
                score: 50.0,
                path: "/home/x/projects/skwir".into(),
            },
            zoxide::ZEntry {
                score: 5.0,
                path: "/home/x/skwirrels".into(),
            },
        ];
        let files = vec![xbel::Recent {
            path: "/home/x/Downloads/skwir-notes.txt".into(),
            mime: None,
            visited: SystemTime::now(),
        }];
        let r = rank("skwir", &dirs, &files, 10);
        assert_eq!(r.len(), 3);
        assert_eq!(
            r[0].path,
            PathBuf::from("/home/x/projects/skwir"),
            "highest zoxide score wins among matches"
        );
        assert!(rank("zzzzqq", &dirs, &files, 10).is_empty());
        let all = rank("", &dirs, &files, 2);
        assert_eq!(all.len(), 2, "limit applies");
    }

    #[test]
    fn the_field_finds_each_word_unbroken_the_name_first() {
        let z = |score, path: &str| zoxide::ZEntry {
            score,
            path: path.into(),
        };
        let dirs = vec![
            z(80.0, "/home/kim/Drafts"),
            z(3.0, "/home/kim/Drafts/Field Guide"),
            z(1.0, "/home/kim/wip/fieldwork"),
            z(5.0, "/home/kim/a/notes"),
            z(50.0, "/home/kim/b/notes"),
        ];
        let files = vec![xbel::Recent {
            path: "/home/kim/Drafts/Field Guide/packing-list.txt".into(),
            mime: None,
            visited: SystemTime::now(),
        }];
        let home = Some(Path::new("/home/kim"));
        let paths = |words| -> Vec<PathBuf> {
            rank_by_words(words, home, &dirs, &files, 10)
                .into_iter()
                .map(|c| c.path)
                .collect()
        };
        let guide = "/home/kim/Drafts/Field Guide";
        let list = "/home/kim/Drafts/Field Guide/packing-list.txt";
        assert!(
            paths("pdf last week").is_empty(),
            "letters strewn over a path are no match"
        );
        assert!(
            paths("kim").is_empty(),
            "the home folder's path matches nothing"
        );
        assert_eq!(
            paths("field"),
            [guide, "/home/kim/wip/fieldwork", list].map(PathBuf::from),
            "in the name before the folders, then the most used"
        );
        assert_eq!(paths("drafts guide"), [guide, list].map(PathBuf::from));
        assert_eq!(
            paths("notes"),
            ["/home/kim/b/notes", "/home/kim/a/notes"].map(PathBuf::from),
            "among equals, the most used"
        );
        assert_eq!(
            paths("ld gui"),
            [guide, list].map(PathBuf::from),
            "within a word too"
        );
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
        let plugin = Arc::new(JumpPlugin::default());
        app.plugins
            .add(Arc::clone(&plugin) as Arc<dyn Plugin>, &app);
        // Nothing is asked of zoxide until Jump is used.
        assert!(plugin.index.lock().asked.is_none());
        // The first query, off the window's thread as the window asks it,
        // gathers what it ranks: it is never answered empty.
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
        // SAFETY: nextest runs each test in a process of its own, and no
        // thread has started yet that reads the environment.
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
        assert_eq!(crumbs, [skwir_core::Crumb::new("Recent", recent.clone())]);
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
