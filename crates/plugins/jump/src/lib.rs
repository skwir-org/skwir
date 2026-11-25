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

use anyhow::Context as _;
use crossbeam_channel::Receiver;
use nucleo_matcher::pattern::{Atom, AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use parking_lot::Mutex;
use skwir_core::CancelToken;
use skwir_core::app::{App, NotifyLevel};
use skwir_core::command::{Invocation, Outcome, Topic};
use skwir_core::entry::{Entry, EntryId};
use skwir_core::fs::{Newest, Read, Reading, Tickets, read_paths_newest};
use skwir_core::location::{
    EmptyState, EventSink, ListingEvent, Location, LocationProvider, OpenContext, OpenLocation,
};
use skwir_core::os_path::PathArg;
use skwir_core::plugin::Plugin;
use skwir_core::registry::{KeyBinding, Place};
use skwir_core::scheduler::Priority;
use skwir_core::words;
use skwir_core::words::Words;
use skwir_core::workspace::PaneId;

const ID: &str = "jump";

/// What `recent:` is called, in the sidebar and wherever it is shown.
fn recent_title() -> Words {
    words!(
        jump_recent = "Recent",
        noun,
        note = "The place listing the files recently used in any application, in the sidebar and the address."
    )
}

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
/// name before its folders, at a word's start before within one. Words
/// are parted at spaces, and in a script written without them at its
/// words (`segment::terms`: "设置文件" is two). Among equals the most used
/// come first, as with nothing typed.
pub fn rank_by_words(
    words: &str,
    home: Option<&Path>,
    dirs: &[zoxide::ZEntry],
    files: &[xbel::Recent],
    limit: usize,
) -> Vec<Candidate> {
    let atoms: Vec<Atom> = skwir_core::segment::terms(words)
        .into_iter()
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
                .map(|p| skwir_core::os_path::from_home_given(p, home))
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
        words!(
            jump_not_absolute = "{ $path } is not an absolute path",
            path = path.display().to_string(),
            note = "An error of Jump to: the path it was given."
        )
    );
    let pane = app.pane_or_active(pane);
    let visit = app
        .workspace
        .read()
        .pane(pane)
        .map(skwir_core::workspace::Pane::location_lifetime)
        .ok_or_else(|| anyhow::anyhow!(words!(jump_no_pane = "no such pane")))?;
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
                app.notify(NotifyLevel::Error, Words::of_error(e));
            }
        },
    );
    Ok(Outcome::Done)
}

fn go(app: &App, pane: PaneId, path: &Path, cancel: &CancelToken) -> anyhow::Result<()> {
    let is_dir = std::fs::metadata(path)
        .with_context(|| Words::as_written(path.display().to_string()))?
        .is_dir();
    if cancel.is_cancelled() {
        return Ok(());
    }
    if is_dir {
        return app.navigate(pane, &Location::from_path(path));
    }
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        anyhow::bail!(words!(
            jump_no_folder = "{ $path } has no folder",
            path = path.display().to_string(),
            note = "An error of Jump to: the path it was given is in no folder."
        ));
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

/// Where a row's file or folder is, as its detail says it: the folder it
/// is in, as a person reads a path (`os_path::from_home`), drawn alone,
/// its controls shown.
fn where_it_is(path: &Path, home: Option<&Path>) -> Option<Words> {
    let parent = path.parent()?;
    Some(Words::name_alone(skwir_core::os_path::from_home_given(
        parent, home,
    )))
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
                // A name drawn alone, its controls shown (ADR-139, 5).
                let mut row = Suggestion::new(Section::Go, Words::name_alone(name))
                    .icon(if dir { "folder" } else { "open" })
                    .rank(i32::try_from(i).unwrap_or(i32::MAX) + 20)
                    .runs(Invocation::with_args(
                        "jump.to",
                        &serde_json::json!({ "path": PathArg::from(c.path.clone()), "pane": ask.pane }),
                    ));
                row.detail = where_it_is(&c.path, home.as_deref());
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
    fn title(&self) -> skwir_core::words::Words {
        skwir_core::words!(jump_plugin_title = "Jump")
    }
    fn description(&self) -> skwir_core::words::Words {
        skwir_core::words!(
            jump_plugin_description =
                "Frecency jump from zoxide and recently used files; the recent: location"
        )
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
            words!(jump_candidates = "Jump candidates"),
            words!(
                jump_candidates_description = "Rank jump targets for a query, from what zoxide and the recent files list said last",
                note = "Read by agents and in F1's list of actions."
            ),
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
            words!(jump_scores = "Jump scores in a folder"),
            words!(
                jump_scores_description = "How much the user goes to each name of a folder, from what zoxide and the recent files list said last: a folder counts the folders within it"
            ),
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
                words!(jump_to = "Jump to"),
                words!(
                    jump_to_description =
                        "Go to a directory, or to a file's directory selecting it"
                ),
                |a, app| jump_to(a.path.into_inner(), a.pane, app),
            )
            .shows();
        app.register_provider(ID, Arc::new(RecentProvider));
        app.registry.places.add(
            ID,
            Place::new(
                skwir_core::registry::groups::PLACES.id,
                recent_title(),
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

    fn title(&self, _location: &Location) -> Option<skwir_core::words::Words> {
        Some(recent_title())
    }

    /// The files used lately: what a dialog opening files offers first
    /// (ADR-144, 7).
    fn lists_recent(&self, _location: &Location) -> bool {
        true
    }

    fn icon(&self, _location: &Location) -> Option<&'static str> {
        Some("recent")
    }

    fn empty_state(&self, _location: &Location) -> Option<EmptyState> {
        Some(EmptyState::new(
            words!(jump_recent_empty = "No recent files"),
            words!(
                jump_recent_empty_detail = "The files you open in your applications are listed here, as the desktop records them."
            ),
        ))
    }

    /// Words search the recent files, as they are listed.
    fn searchable(&self, _location: &Location) -> bool {
        true
    }

    fn open(&self, location: &Location, ctx: OpenContext) -> anyhow::Result<Arc<dyn OpenLocation>> {
        Ok(RecentLocation::open(location.clone(), ctx))
    }
}

struct RecentLocation {
    location: Location,
    rx: Receiver<ListingEvent>,
    recent: Arc<Recent>,
}

impl RecentLocation {
    fn open(location: Location, ctx: OpenContext) -> Arc<dyn OpenLocation> {
        let (tx, rx) = ctx.channel();
        let recent = Arc::new(Recent {
            ctx,
            tx,
            known: Mutex::default(),
            tickets: Tickets::default(),
            listing: Mutex::new(CancelToken::new()),
        });
        recent.list();
        Arc::new(RecentLocation {
            location,
            rx,
            recent,
        })
    }
}

/// What hydration's answers are told at least this many at a time, as its
/// folders are read: the window applies them in slices.
const SENT_EVERY: usize = 256;

/// An open `recent:`, shared by its jobs: a listing's (F5's), a
/// hydration's. It has no watcher: what it lists is as its last reads
/// found it, and of two reads of a file, the one begun later stands,
/// whichever is told last (`Known::newest`).
struct Recent {
    ctx: OpenContext,
    tx: EventSink,
    /// What it lists, changed under this lock, its events sent under it:
    /// they leave in the order decided.
    known: Mutex<Known>,
    /// Taken by each read of a file just before it reads (a listing's
    /// stat, a hydration's).
    tickets: Tickets,
    /// The listing under way, which the next supersedes (`list`).
    listing: Mutex<CancelToken>,
}

#[derive(Default)]
struct Known {
    /// The files listed, by id.
    rows: HashMap<EntryId, Row>,
    /// Which read of each file was told last, while another read that may
    /// hold an older one is under way (`fs::Newest`).
    newest: Newest<EntryId>,
}

/// A file listed.
struct Row {
    path: PathBuf,
    /// Whether its row was told what the metadata providers say of it,
    /// which it keeps under a stat alone (`Listing::upsert`): a hydration
    /// left out behind a read that told it less (a stat alone, F5's) is
    /// read again where it was not, or the row would stay unfilled for
    /// good, its asker waiting on that answer.
    filled: bool,
}

/// The row of `id` in `rows`, if it is listed still where it was looked for
/// (`asked`).
fn listed<'a>(
    rows: &'a mut HashMap<EntryId, Row>,
    asked: &HashMap<EntryId, &Path>,
    id: EntryId,
) -> Option<&'a mut Row> {
    let asked = asked.get(&id)?;
    rows.get_mut(&id).filter(|row| row.path.as_path() == *asked)
}

impl Recent {
    /// List the recent files: stat-ed to be listed by their identity, each
    /// remote's in its lane (ADR-077), so a server that stops answering
    /// holds its own files, never the listing of the others: what is on
    /// this machine is listed, then complete; a remote's come as its
    /// server answers. What is gone is left out.
    ///
    /// A listing supersedes the one before it, as a search's F5 does: the
    /// older is cancelled, under `known`'s lock, before the newer reads
    /// the list, so nothing the older read is told after (a stat of a file
    /// its list named, which a stat's later ticket alone would let add
    /// back what the newer list no longer names); what it told before,
    /// the newer decides over (`listed_at`).
    fn list(self: &Arc<Self>) {
        let recent = Arc::clone(self);
        let cancel = self.ctx.cancel.child();
        {
            let _known = self.known.lock();
            std::mem::replace(&mut *self.listing.lock(), cancel.clone()).cancel();
        }
        let scheduler = self.ctx.scheduler.clone();
        self.ctx.scheduler.spawn(
            Priority::Visible,
            "recent.list",
            cancel.clone(),
            move || {
                // Dropped last, after what it takes out is told.
                let _reading = Reading::begin(&recent.known, |k| &mut k.newest, &recent.tickets);
                // The list as read now: a file a read begun later told (a
                // later F5's, the list grown since) stays, whatever this
                // one says of it.
                let listed_at = recent.tickets.take();
                let items = xbel::default_path()
                    .map(|p| xbel::load(&p))
                    .unwrap_or_default();
                #[cfg(test)]
                {
                    let hook = BEFORE_STATS.lock().take();
                    if let Some(hook) = hook {
                        hook();
                    }
                }
                let wanted: std::collections::HashSet<PathBuf> =
                    items.iter().map(|r| r.path.clone()).collect();
                let listing = Listed {
                    recent: Arc::clone(&recent),
                    seen: Arc::default(),
                    cancel: cancel.clone(),
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
                #[cfg(test)]
                {
                    let hook = BEFORE_PREV.lock().take();
                    if let Some(hook) = hook {
                        hook();
                    }
                }
                // What was listed before and no longer is recent, unless
                // a read begun after the list was read told it.
                let mut known = recent.known.lock();
                if cancel.is_cancelled() {
                    return;
                }
                let Known { rows, newest } = &mut *known;
                let prev: Vec<EntryId> = rows
                    .iter()
                    .filter(|(_, row)| !wanted.contains(&row.path))
                    .map(|(id, _)| *id)
                    .collect();
                let prev: Vec<EntryId> = prev
                    .into_iter()
                    .filter(|id| newest.applies(id, listed_at))
                    .collect();
                for id in &prev {
                    rows.remove(id);
                }
                if !prev.is_empty() {
                    recent.tx.send(ListingEvent::Removed(prev));
                }
                recent.tx.send(ListingEvent::Complete);
            },
        );
    }

    fn hydrate(self: &Arc<Self>, ids: &[EntryId], priority: Priority) {
        let wanted: Vec<(EntryId, PathBuf)> = {
            let known = self.known.lock();
            ids.iter()
                .filter_map(|id| known.rows.get(id).map(|row| (*id, row.path.clone())))
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
                let recent = Arc::clone(self);
                let cancel = self.ctx.cancel.child();
                self.ctx.scheduler.spawn_in(
                    lane,
                    priority,
                    "recent.stat",
                    cancel.clone(),
                    move || recent.hydrate_now(&wanted, priority, &cancel),
                );
            }
        }
    }

    /// Stat `items` and fill them, one folder at a time, each answer told
    /// under the lock where no read of it begun later was; what is left
    /// out behind a read that told its row less (a stat alone, F5's) is
    /// asked again. Worker-only: this is I/O, in the lane of the paths.
    fn hydrate_now(
        self: &Arc<Self>,
        items: &[(EntryId, PathBuf)],
        priority: Priority,
        cancel: &CancelToken,
    ) {
        let again = {
            // Dropped once its answers are told, before what is read
            // again is asked.
            let _reading = Reading::begin(&self.known, |k| &mut k.newest, &self.tickets);
            let asked: HashMap<EntryId, &Path> = items
                .iter()
                .map(|(id, path)| (*id, path.as_path()))
                .collect();
            let (mut reads, mut held, mut again) = (Vec::new(), 0, Vec::new());
            read_paths_newest(items, &self.ctx.metadata, cancel, &self.tickets, |read| {
                held += read.found.len() + read.gone.len() + read.unread.len();
                reads.push(read);
                if held >= SENT_EVERY {
                    again.extend(self.answer(std::mem::take(&mut reads), &asked, cancel));
                    held = 0;
                }
            });
            again.extend(self.answer(reads, &asked, cancel));
            again
        };
        if !again.is_empty() && !cancel.is_cancelled() {
            self.hydrate(&again, priority);
        }
    }

    /// Tell what `reads` found, `asked` where each was looked for: only
    /// where it is listed still at that path and no read of it begun later
    /// was told (a refresh's stat of a file grown, of one named back), gone
    /// included. Returns what is to be read again, as asked: left out, for
    /// a row still listed there, behind a read that told it less than this
    /// one would (a stat alone, its row never filled), found, gone or
    /// unread. Each read again takes later tickets: this ends.
    fn answer(
        &self,
        reads: Vec<Read>,
        asked: &HashMap<EntryId, &Path>,
        cancel: &CancelToken,
    ) -> Vec<EntryId> {
        let mut known = self.known.lock();
        if cancel.is_cancelled() {
            return Vec::new();
        }
        let Known { rows, newest } = &mut *known;
        let (mut updated, mut gone, mut unread, mut again) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for read in reads {
            for (e, ticket) in read.found {
                let Some(row) = listed(rows, asked, e.id) else {
                    continue;
                };
                if newest.applies(&e.id, ticket) {
                    row.filled |= e.metadata.is_filled();
                    updated.push(e);
                } else if !row.filled {
                    again.push(e.id);
                }
            }
            for (id, ticket) in read.gone {
                let Some(row) = listed(rows, asked, id) else {
                    continue;
                };
                if newest.applies(&id, ticket) {
                    gone.push(id);
                } else if !row.filled {
                    again.push(id);
                }
            }
            for (id, ticket) in read.unread {
                let Some(row) = listed(rows, asked, id) else {
                    continue;
                };
                if newest.applies(&id, ticket) {
                    unread.push(id);
                } else if !row.filled {
                    again.push(id);
                }
            }
        }
        for id in &gone {
            rows.remove(id);
        }
        if !updated.is_empty() {
            self.tx.send(ListingEvent::Updated(updated));
        }
        if !gone.is_empty() {
            self.tx.send(ListingEvent::Removed(gone));
        }
        if !unread.is_empty() {
            self.tx.send(ListingEvent::Unreadable(unread));
        }
        again
    }
}

/// What a test runs where a listing of the recent files has stat-ed them
/// and is about to take out what is no longer recent: it may hold it there.
#[cfg(test)]
static BEFORE_PREV: Mutex<Option<Box<dyn FnOnce() + Send>>> = Mutex::new(None);

/// What a test runs where a listing of the recent files has read the list
/// and is about to stat its files: it may hold it there.
#[cfg(test)]
static BEFORE_STATS: Mutex<Option<Box<dyn FnOnce() + Send>>> = Mutex::new(None);

/// One listing of the recent files, shared by its lanes' jobs.
#[derive(Clone)]
struct Listed {
    recent: Arc<Recent>,
    /// The files listed, once each: two names of one file are one row.
    seen: Arc<Mutex<std::collections::HashSet<EntryId>>>,
    cancel: CancelToken,
}

impl Listed {
    /// Stat `items` and send them, in batches; what was listed at their
    /// paths before and is not now (gone, or another file there) is taken
    /// out. Each stat takes a ticket just before it reads, and what it
    /// found is told only where no read of the file begun later was (a
    /// hydration's, asked meanwhile).
    fn stat(&self, items: &[PathBuf]) {
        let recent = &*self.recent;
        // Dropped last, after what it found is told.
        let _reading = Reading::begin(&recent.known, |k| &mut k.newest, &recent.tickets);
        let batch_size = recent.ctx.batch_size.max(1);
        // Each path asked, with its stat's ticket.
        let mut asked: HashMap<&Path, u64> = HashMap::with_capacity(items.len());
        let mut batch = Vec::new();
        for path in items {
            if self.cancel.is_cancelled() {
                return;
            }
            let ticket = recent.tickets.take();
            asked.insert(path.as_path(), ticket);
            let Some(e) = skwir_core::fs::stat::stat_path(path) else {
                continue;
            };
            if !self.seen.lock().insert(e.id) {
                continue;
            }
            batch.push((e, path.clone(), ticket));
            if batch.len() >= batch_size {
                let mut known = recent.known.lock();
                if self.cancel.is_cancelled() {
                    return;
                }
                self.tell(&mut known, std::mem::take(&mut batch));
            }
        }
        let mut known = recent.known.lock();
        if self.cancel.is_cancelled() {
            return;
        }
        self.tell(&mut known, batch);
        let Known { rows, newest } = &mut *known;
        let left: Vec<(EntryId, u64)> = {
            let seen = self.seen.lock();
            rows.iter()
                .filter(|(id, _)| !seen.contains(*id))
                .filter_map(|(id, row)| Some((*id, *asked.get(row.path.as_path())?)))
                .collect()
        };
        // Unless a read begun later found it there.
        let removed: Vec<EntryId> = left
            .into_iter()
            .filter(|(id, ticket)| newest.applies(id, *ticket))
            .map(|(id, _)| id)
            .collect();
        for id in &removed {
            rows.remove(id);
        }
        if !removed.is_empty() {
            recent.tx.send(ListingEvent::Removed(removed));
        }
    }

    /// Tell what stats found, under the lock it is decided by: what a read
    /// begun later told already is left out.
    fn tell(&self, known: &mut Known, batch: Vec<(Entry, PathBuf, u64)>) {
        let mut added = Vec::with_capacity(batch.len());
        for (e, path, ticket) in batch {
            if !known.newest.applies(&e.id, ticket) {
                continue;
            }
            known
                .rows
                .entry(e.id)
                .or_insert_with(|| Row {
                    path: PathBuf::new(),
                    filled: false,
                })
                .path = path;
            added.push(e);
        }
        if !added.is_empty() {
            self.recent.tx.send(ListingEvent::Added(added));
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
        self.recent.hydrate(ids, priority);
    }
    fn path_of(&self, id: EntryId) -> Option<PathBuf> {
        let known = self.recent.known.lock();
        known.rows.get(&id).map(|row| row.path.clone())
    }
    fn refresh(&self) {
        self.recent.list();
    }
    fn close(&self) {
        self.recent.ctx.cancel.cancel();
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
            z(80.0, "/home/x/Drafts"),
            z(3.0, "/home/x/Drafts/Field Guide"),
            z(1.0, "/home/x/wip/fieldwork"),
            z(5.0, "/home/x/a/notes"),
            z(50.0, "/home/x/b/notes"),
        ];
        let files = vec![xbel::Recent {
            path: "/home/x/Drafts/Field Guide/packing-list.txt".into(),
            mime: None,
            visited: SystemTime::now(),
        }];
        let home = Some(Path::new("/home/x"));
        let paths = |words| -> Vec<PathBuf> {
            rank_by_words(words, home, &dirs, &files, 10)
                .into_iter()
                .map(|c| c.path)
                .collect()
        };
        let guide = "/home/x/Drafts/Field Guide";
        let list = "/home/x/Drafts/Field Guide/packing-list.txt";
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
            [guide, "/home/x/wip/fieldwork", list].map(PathBuf::from),
            "in the name before the folders, then the most used"
        );
        assert_eq!(paths("drafts guide"), [guide, list].map(PathBuf::from));
        assert_eq!(
            paths("notes"),
            ["/home/x/b/notes", "/home/x/a/notes"].map(PathBuf::from),
            "among equals, the most used"
        );
        assert_eq!(
            paths("ld gui"),
            [guide, list].map(PathBuf::from),
            "within a word too"
        );
    }

    /// Words typed without spaces, in a script written without them, are
    /// found apart as words typed with spaces are: "项目文档" (project,
    /// documents) finds a project kept in documents.
    #[test]
    fn the_field_finds_words_typed_without_spaces_apart() {
        let dirs = vec![
            zoxide::ZEntry {
                score: 1.0,
                path: "/home/x/文档/项目".into(),
            },
            zoxide::ZEntry {
                score: 1.0,
                path: "/home/x/文件".into(),
            },
        ];
        let found: Vec<PathBuf> =
            rank_by_words("项目文档", Some(Path::new("/home/x")), &dirs, &[], 10)
                .into_iter()
                .map(|c| c.path)
                .collect();
        assert_eq!(found, [PathBuf::from("/home/x/文档/项目")]);
    }

    /// A row says where its file is from `~`, the system's separator
    /// between all its names (`~\wip\one` on Windows, never `~/wip\one`),
    /// its controls shown; a file in home itself is in `~`.
    #[test]
    fn a_row_says_its_folder_from_home_as_the_system_writes_it() {
        let home = PathBuf::from(if cfg!(windows) {
            r"C:\Users\u"
        } else {
            "/home/u"
        });
        let sep = std::path::MAIN_SEPARATOR;
        let said = |path: &Path| where_it_is(path, Some(&home)).map(|w| w.say_plain());
        let nested = home.join("wip").join("one").join("l'été.txt");
        assert_eq!(said(&nested), Some(format!("~{sep}wip{sep}one")));
        let spoof = home.join("a\u{202e}gpj.exe").join("x");
        assert_eq!(said(&spoof), Some(format!("~{sep}a⟨RLO⟩gpj.exe")));
        assert_eq!(said(&home.join("notes.txt")).as_deref(), Some("~"));
        // What is typed finds a folder by the words its row says.
        let dirs = vec![zoxide::ZEntry {
            score: 1.0,
            path: nested.parent().unwrap().to_owned(),
        }];
        let found = rank_by_words("wip one", Some(&home), &dirs, &[], 10);
        assert_eq!(found.len(), 1);
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

    /// A metadata provider that holds the first batch naming `name` until
    /// let go: a hydration held after its stats, before it is told.
    struct Hold {
        name: std::ffi::OsString,
        once: Mutex<Option<(crossbeam_channel::Sender<()>, Receiver<()>)>>,
    }

    impl skwir_core::metadata::MetadataProvider for Hold {
        fn id(&self) -> &'static str {
            "test.hold"
        }

        fn fill(&self, _dir: &Path, entries: &mut [Entry]) {
            if !entries.iter().any(|e| e.name == self.name) {
                return;
            }
            let Some((held, go)) = self.once.lock().take() else {
                return;
            };
            let _ = held.send(());
            // Let go, or the test gone.
            let _ = go.recv_timeout(Duration::from_secs(30));
        }
    }

    /// Apply what `open` sends to `listing` until `done` holds of it.
    fn until(
        open: &dyn OpenLocation,
        listing: &mut skwir_core::Listing,
        what: &str,
        done: impl Fn(&skwir_core::Listing) -> bool,
    ) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done(listing) {
            let Ok(event) = open.events().recv_deadline(deadline) else {
                panic!("never {what}");
            };
            listing.apply(event);
        }
    }

    fn size_of(listing: &skwir_core::Listing, id: EntryId) -> Option<u64> {
        let e = listing.get(id)?;
        e.metadata
            .value(skwir_core::entry::keys::SIZE)
            .and_then(skwir_core::entry::Value::as_u64)
    }

    /// F5 while a hydration is held: one file grew and another was named
    /// away and back since the hydration read them. What the refresh
    /// found, begun later, stands over what the hydration found, told
    /// after: the new size (filled once read again), and the file named
    /// back still listed.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_refresh_while_a_hydration_is_held_stands_over_what_it_found() {
        let base = std::env::home_dir().unwrap().join(".cache/skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        let tmp = tempfile::tempdir_in(base).unwrap();
        let data = tmp.path().join("data");
        let dir = tmp.path().join("l'été, à l'ombre");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir(&dir).unwrap();
        let (grown, named) = ("it's ☂ grown.txt", "named back, s'il vous plaît.txt");
        let (grown_at, named_at, away) = (
            dir.join(grown),
            dir.join(named),
            dir.join("named away ☂.txt"),
        );
        std::fs::write(&grown_at, b"ten bytes!").unwrap();
        std::fs::write(&named_at, b"named").unwrap();
        let xbel = format!(
            r#"<?xml version="1.0"?><xbel version="1.0"><bookmark href="{}" visited="2026-01-01T00:00:00Z"/><bookmark href="{}" visited="2026-01-02T00:00:00Z"/></xbel>"#,
            skwir_platform::uri::file_uri(&grown_at),
            skwir_platform::uri::file_uri(&named_at),
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
        let open = app.open_location(&Location::parse("recent:")).unwrap();
        let mut listing = skwir_core::Listing::default();
        until(&*open, &mut listing, "listed", |l| {
            l.is_complete() && l.len() == 2
        });
        let id_of = |l: &skwir_core::Listing, name: &str| {
            l.find_by_name(std::ffi::OsStr::new(name)).unwrap().id
        };
        let (g, n) = (id_of(&listing, grown), id_of(&listing, named));
        assert_eq!(size_of(&listing, g), Some(10));
        let (held, go) = (crossbeam_channel::bounded(1), crossbeam_channel::bounded(1));
        app.metadata.register(
            "test",
            Arc::new(Hold {
                name: grown.into(),
                once: Mutex::new(Some((held.0, go.1))),
            }),
        );
        // Named away: the hydration finds it gone, and the other as it was,
        // and is held.
        std::fs::rename(&named_at, &away).unwrap();
        open.hydrate(&[g, n], Priority::Visible);
        held.1.recv_timeout(Duration::from_secs(10)).expect("held");
        // Meanwhile one grows, the other is named back, and F5 lists them
        // again.
        std::fs::write(&grown_at, b"twenty bytes, grown!").unwrap();
        std::fs::rename(&away, &named_at).unwrap();
        open.refresh();
        until(&*open, &mut listing, "listed again", |l| {
            size_of(l, g) == Some(20)
        });
        // The hydration let go: what it found is told after F5's.
        go.0.send(()).unwrap();
        until(&*open, &mut listing, "filled", |l| {
            l.get(g).is_some_and(|e| e.metadata.is_filled())
        });
        assert!(app.scheduler.wait_idle_here(Duration::from_secs(10)));
        for event in open.events().try_iter() {
            listing.apply(event);
        }
        // The newer size, and the file named back still listed, each
        // filled: what the hydration found was left out behind F5's stats
        // alone, and read again.
        let filled = |id| listing.get(id).is_some_and(|e| e.metadata.is_filled());
        assert_eq!(
            (
                size_of(&listing, g),
                listing.get(n).is_some(),
                filled(g),
                filled(n)
            ),
            (Some(20), true, true, true)
        );
        assert_eq!(open.path_of(n), Some(named_at));
    }

    /// Two F5s overlap, and a file is used in between: the older listing,
    /// ending last, read the list before the newer told that file, which
    /// stays listed.
    #[cfg(target_os = "linux")]
    #[test]
    fn an_older_refresh_ending_last_keeps_what_a_newer_one_listed() {
        let base = std::env::home_dir().unwrap().join(".cache/skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        let tmp = tempfile::tempdir_in(base).unwrap();
        let data = tmp.path().join("data");
        let dir = tmp.path().join("l'été, à l'ombre");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir(&dir).unwrap();
        let (first, since) = (
            dir.join("it's ☂ first.txt"),
            dir.join("used since, s'il vous plaît.txt"),
        );
        std::fs::write(&first, b"first").unwrap();
        std::fs::write(&since, b"since").unwrap();
        let xbel = |paths: &[&Path]| {
            let marks: Vec<String> = paths
                .iter()
                .map(|p| skwir_platform::uri::file_uri(p))
                .map(|href| format!(r#"<bookmark href="{href}" visited="2026-01-01T00:00:00Z"/>"#))
                .collect();
            let marks = marks.concat();
            std::fs::write(
                data.join("recently-used.xbel"),
                format!(r#"<?xml version="1.0"?><xbel version="1.0">{marks}</xbel>"#),
            )
            .unwrap();
        };
        xbel(&[&first]);
        // SAFETY: nextest runs each test in a process of its own, and no
        // thread has started yet that reads the environment.
        #[allow(unsafe_code)]
        unsafe {
            std::env::set_var("XDG_DATA_HOME", &data);
        }
        let app = app(tmp.path());
        app.plugins.add(Arc::new(JumpPlugin::default()), &app);
        let open = app.open_location(&Location::parse("recent:")).unwrap();
        let mut listing = skwir_core::Listing::default();
        until(&*open, &mut listing, "listed", |l| {
            l.is_complete() && l.len() == 1
        });
        // F5, held once it read the list and its files.
        let (held, is_held) = crossbeam_channel::bounded(1);
        let (go, gone) = crossbeam_channel::bounded::<()>(1);
        *BEFORE_PREV.lock() = Some(Box::new(move || {
            let _ = held.send(());
            let _ = gone.recv_timeout(Duration::from_secs(30));
        }));
        open.refresh();
        is_held.recv_timeout(Duration::from_secs(10)).expect("held");
        // A file used meanwhile, and F5 again.
        xbel(&[&first, &since]);
        open.refresh();
        let named = std::ffi::OsStr::new("used since, s'il vous plaît.txt");
        until(&*open, &mut listing, "listed again", |l| l.is_named(named));
        // The older let go: it takes out what it did not read in the list.
        go.send(()).unwrap();
        assert!(app.scheduler.wait_idle_here(Duration::from_secs(10)));
        for event in open.events().try_iter() {
            listing.apply(event);
        }
        let id = listing.find_by_name(named).map(|e| e.id);
        assert!(id.is_some(), "used since, still listed");
        assert_eq!(id.and_then(|id| open.path_of(id)), Some(since));
        assert_eq!(listing.len(), 2);
    }

    /// Two F5s overlap, and a file is no longer recent in between: the
    /// older listing, which read the list before, stats that file only
    /// once the newer took it out, and never adds it back.
    #[cfg(target_os = "linux")]
    #[test]
    fn an_older_refresh_never_adds_back_what_a_newer_one_took_out() {
        let base = std::env::home_dir().unwrap().join(".cache/skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        let tmp = tempfile::tempdir_in(base).unwrap();
        let data = tmp.path().join("data");
        let dir = tmp.path().join("l'été, à l'ombre");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir(&dir).unwrap();
        let (kept, dropped) = (
            dir.join("still used, s'il vous plaît.txt"),
            dir.join("it's ☂ no longer.txt"),
        );
        std::fs::write(&kept, b"kept").unwrap();
        std::fs::write(&dropped, b"dropped").unwrap();
        let xbel = |paths: &[&Path]| {
            let marks: Vec<String> = paths
                .iter()
                .map(|p| skwir_platform::uri::file_uri(p))
                .map(|href| format!(r#"<bookmark href="{href}" visited="2026-01-01T00:00:00Z"/>"#))
                .collect();
            let marks = marks.concat();
            std::fs::write(
                data.join("recently-used.xbel"),
                format!(r#"<?xml version="1.0"?><xbel version="1.0">{marks}</xbel>"#),
            )
            .unwrap();
        };
        xbel(&[&kept, &dropped]);
        // SAFETY: nextest runs each test in a process of its own, and no
        // thread has started yet that reads the environment.
        #[allow(unsafe_code)]
        unsafe {
            std::env::set_var("XDG_DATA_HOME", &data);
        }
        let app = app(tmp.path());
        app.plugins.add(Arc::new(JumpPlugin::default()), &app);
        let open = app.open_location(&Location::parse("recent:")).unwrap();
        let mut listing = skwir_core::Listing::default();
        until(&*open, &mut listing, "listed", |l| {
            l.is_complete() && l.len() == 2
        });
        // F5, held once it read the list, before it stats its files.
        let (held, is_held) = crossbeam_channel::bounded(1);
        let (go, gone) = crossbeam_channel::bounded::<()>(1);
        *BEFORE_STATS.lock() = Some(Box::new(move || {
            let _ = held.send(());
            let _ = gone.recv_timeout(Duration::from_secs(30));
        }));
        open.refresh();
        is_held.recv_timeout(Duration::from_secs(10)).expect("held");
        // A file no longer recent, and F5 again: it is taken out.
        xbel(&[&kept]);
        open.refresh();
        let named = std::ffi::OsStr::new("it's ☂ no longer.txt");
        until(&*open, &mut listing, "taken out", |l| {
            l.is_complete() && !l.is_named(named)
        });
        // The older let go: its stats come after the newer's list.
        go.send(()).unwrap();
        assert!(app.scheduler.wait_idle_here(Duration::from_secs(10)));
        for event in open.events().try_iter() {
            listing.apply(event);
        }
        assert!(!listing.is_named(named), "no longer recent, not listed");
        assert_eq!(listing.len(), 1);
        let kept_name = std::ffi::OsStr::new("still used, s'il vous plaît.txt");
        assert!(listing.is_named(kept_name));
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
        assert_eq!(
            crumbs,
            [skwir_core::Crumb::new(recent_title(), recent.clone())]
        );
        assert_eq!(crumbs[0].label.english(), "Recent");
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
                .any(|pl| pl.title.english() == "Recent")
        );
    }
}
