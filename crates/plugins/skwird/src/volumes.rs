//! Removable drives (ADR-150, 18): skwird 0.2 indexes the file names on a
//! removable drive once its user said yes for that drive, by the identity
//! the engine spells (`SeenVolume::id`; Skwir never derives one), and keeps
//! it for this client across runs. How Skwir asks is the user's choice
//! (2026-11-25):
//!
//! - **Once a drive, as it is plugged in.** A drive mounted while Skwir runs
//!   (`reach::follow`: skwird sees it at a mount point made since the check
//!   before), or mounted before and found at the first look of a window
//!   (`Plugin::on_looked_at`), is offered where an engine already runs and
//!   says `volumes`, the drive has an identity, this client keeps it not and
//!   its user was never asked about it: a message that stays
//!   (`App::ask_quietly`), Yes or No. Never an engine started to ask, and
//!   never a drive skwird came to see otherwise (an engine started since,
//!   another mount made): its menu offers it. Its identity is counted asked
//!   as it is asked, so a question closed unanswered is a No. One question
//!   stands at a time: the next waits for its answer, and a drive plugged in
//!   meanwhile takes its place, the one it replaces counted asked. The
//!   question waits for a window of the user's own (ADR-141, 8), and asks
//!   only of what skwird said last: none while no engine answers, a drive
//!   gone from the mount table then let go of.
//! - **A drive's menu in the sidebar**, Index Its Names, checked while it is
//!   kept: from memory, for a mount point skwird's last answer saw with an
//!   identity.
//! - **The Index page lists the drives kept**, each with Forget, which takes
//!   it from what is kept and has skwird remove its index.
//!
//! A drive kept or forgotten by any way (its question, its menu, the Index
//! page, a command) is decided: counted asked, waiting no more, its
//! question taken back. What is counted asked, what waits and the question
//! the window shows change in one place alone (`Volumes::settle`), under one
//! lock held until the window is told, so none of them is left behind and no
//! two askers come between each other.
//!
//! What Skwir keeps of it is state (ADR-056, `skwird.volumes`): the
//! identities asked, each kept drive's label, which a drive away has not in
//! skwird's answer, and when each label was taken or let go of, so that of
//! two skwirs on one state file the later word on a drive stands
//! (`Memory::merge`). Nothing of it goes to skwird but keep and forget,
//! each a command (`skwird.keep_volume`, the user's alone, and
//! `skwird.forget_volume`), which start an engine where none runs as a
//! search does (`skwird.start`): the user asked. skwird is asked what it
//! sees (`volumes` with nothing to keep or forget) as mounts change, at the
//! first look, and as the list or a drive's menu is shown (at most every
//! few seconds), never starting one; what it said last is what the menu and
//! the list answer from, of two answers the one asked later.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant, SystemTime};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use skwir_core::app::{App, NotifyAction, NotifyLevel};
use skwir_core::cancel::CancelToken;
use skwir_core::command::{Effect, Invocation, NoArgs, Outcome, Topic};
use skwir_core::location::Location;
use skwir_core::os_path::PathArg;
use skwir_core::registry::{MenuItem, MenuSource};
use skwir_core::scheduler::Priority;
use skwir_core::settings::{RecordField, RecordSource, SettingSpec, page, section};
use skwir_core::words::{Moment, Name, Shape, Words};
use skwird_client::{Client, protocol};

use crate::{Engines, ID};

/// What Skwir keeps of the drives, in the state file.
pub(crate) const STATE: &str = "skwird.volumes";
/// The list on the Index page, which this plugin keeps.
pub(crate) const LIST: &str = "skwird.volumes_kept";

/// How long a drive's question waits for an engine that sees a mount a
/// moment after the system makes it (macOS': `DiskArbitration` describes a
/// volume a while after it is mounted) before it is asked once more.
const AGAIN: Duration = Duration::from_secs(2);

/// How often a listing of the page or a drive's menu asks skwird again.
const LISTED_EVERY: Duration = Duration::from_secs(5);

/// How long skwird may take to answer `volumes`: it keeps, forgets and
/// says what it sees at once, its reads of a drive coming after.
const VOLUMES_SILENCE: Duration = Duration::from_secs(8);

/// Where filesystems of files kept are mounted now: the system's table
/// ([`table`]), or a test's.
pub(crate) type Mounted = Arc<dyn Fn() -> HashSet<PathBuf> + Send + Sync>;

/// The mount points of filesystems of files kept, from the table in memory
/// (`reach::points`): no network's, no kernel's, no automount point.
pub(crate) fn table() -> HashSet<PathBuf> {
    skwir_core::fs::reach::points()
        .into_iter()
        .filter(|m| !m.remote && !m.kernel && !m.on_demand)
        .map(|m| m.point)
        .collect()
}

/// What Skwir keeps from one run to the next: the drives asked about, by
/// their identity, and each kept drive's label. Another skwir's, on the
/// same state file, is merged in before a save (`Memory::merge`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Remembered {
    pub asked: BTreeSet<String>,
    pub labels: BTreeMap<String, String>,
    /// When each drive's label was last taken or let go of (kept, or
    /// forgotten), in milliseconds since 1970: of two skwirs' words on a
    /// drive, the later stands, a drive forgotten in one never brought
    /// back by the label another still holds.
    pub since: BTreeMap<String, i64>,
    /// What a later skwir keeps that this one does not know, kept as it is.
    #[serde(flatten)]
    pub later: toml::Table,
}

impl Remembered {
    fn of(value: toml::Value) -> Option<Remembered> {
        value.try_into().ok()
    }
}

/// Now, as [`Remembered::since`] counts it.
fn now() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |since| {
            i64::try_from(since.as_millis()).unwrap_or(i64::MAX)
        })
}

/// A drive to ask about, as skwird last saw it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Offer {
    id: String,
    label: String,
    /// Where it was mounted then: a point the table lists no more, it went.
    at: PathBuf,
}

/// The drives waiting to be asked about and the one asked: changed only in
/// [`Volumes::settle`], under its lock.
#[derive(Default)]
struct Questions {
    /// The drives to ask about, in the order found, none counted asked.
    waiting: VecDeque<Offer>,
    /// The drive whose question stands, as far as this run knows: no
    /// answer heard for it (a question closed is never told). The window's
    /// question is always its.
    up: Option<Offer>,
}

impl Questions {
    /// The user decided of `id`: kept or forgotten, by its question, its
    /// menu, the Index page or a command. Counted asked, so never asked
    /// again nor left waiting; its question, standing, taken back.
    fn decided(&mut self, asked: &mut BTreeSet<String>, id: &str) {
        asked.insert(id.to_owned());
        self.up.take_if(|up| up.id == id);
    }

    /// Only the drives `still` holds stay, waiting or asked: the others
    /// went, or are kept since.
    fn moot(&mut self, still: impl Fn(&Offer) -> bool) {
        self.waiting.retain(&still);
        self.up.take_if(|up| !still(up));
    }

    /// The drives it holds, waiting or asked.
    fn held(&self) -> HashSet<String> {
        self.waiting
            .iter()
            .chain(&self.up)
            .map(|offer| offer.id.clone())
            .collect()
    }
}

#[derive(Default)]
struct Memory {
    /// What a running engine that says `volumes` last answered; none where
    /// none did.
    said: Option<protocol::Volumes>,
    /// The ticket of the ask `said` answers (`Volumes::asks`): an answer to
    /// one asked before it is let go of, whenever it comes.
    said_at: u64,
    remembered: Remembered,
    /// The mount points last seen, each a filesystem of files kept.
    points: Option<HashSet<PathBuf>>,
}

impl Memory {
    /// Take in what `theirs` keeps (another skwir's): what it asked, and of
    /// each drive's label the one taken or let go of later, theirs where
    /// they said so later than this one did.
    fn merge(&mut self, theirs: &Remembered) {
        let ours = &mut self.remembered;
        ours.asked.extend(theirs.asked.iter().cloned());
        for (id, &at) in &theirs.since {
            if ours.since.get(id).is_some_and(|&ours_at| ours_at >= at) {
                continue;
            }
            ours.since.insert(id.clone(), at);
            match theirs.labels.get(id) {
                Some(label) => ours.labels.insert(id.clone(), label.clone()),
                None => ours.labels.remove(id),
            };
        }
        // A label written with no time (a build before them): taken where
        // nothing is known of its drive here.
        for (id, label) in &theirs.labels {
            if !theirs.since.contains_key(id) && !ours.since.contains_key(id) {
                ours.labels
                    .entry(id.clone())
                    .or_insert_with(|| label.clone());
            }
        }
        for (key, value) in &theirs.later {
            if !ours.later.contains_key(key) {
                ours.later.insert(key.clone(), value.clone());
            }
        }
    }

    /// Remember `label` for the drive `id`, kept: whether it changed.
    fn labelled(&mut self, id: &str, label: String) -> bool {
        let ours = &mut self.remembered;
        if ours.labels.get(id) == Some(&label) {
            return false;
        }
        ours.labels.insert(id.to_owned(), label);
        ours.since.insert(id.to_owned(), now());
        true
    }

    /// Let go of the label of `id`, no longer kept: whether it held one.
    fn unlabelled(&mut self, id: &str) -> bool {
        let ours = &mut self.remembered;
        if ours.labels.remove(id).is_none() {
            return false;
        }
        ours.since.insert(id.to_owned(), now());
        true
    }

    /// `id` forgotten here: its label let go of, and the word kept that it
    /// was then, which another skwir's older label does not undo.
    fn forgot(&mut self, id: &str) {
        let ours = &mut self.remembered;
        ours.labels.remove(id);
        ours.since.insert(id.to_owned(), now());
    }

    /// What `id` is called: its label as mounted now, else as remembered,
    /// else its identity.
    fn label(&self, id: &str) -> String {
        self.said
            .as_ref()
            .and_then(|said| said.seen.iter().find(|s| s.id.as_deref() == Some(id)))
            .map(label_of)
            .or_else(|| self.remembered.labels.get(id).cloned())
            .unwrap_or_else(|| id.to_owned())
    }
}

/// What asks skwird: whether it is the first look of this run, and whether
/// a mount was made or let go of.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Asking {
    first_look: bool,
    mounts: bool,
}

impl Asking {
    fn with(self, other: Asking) -> Asking {
        Asking {
            first_look: self.first_look || other.first_look,
            mounts: self.mounts || other.mounts,
        }
    }
}

/// Which drives a check finds to ask about (ADR-150, 19).
enum Finds {
    /// None: a listing's check.
    Nothing,
    /// Every one skwird sees: at the first look, for the drives mounted
    /// before, or at a mount before any point was seen.
    Every,
    /// Those skwird sees at these points, mounted since the check before:
    /// plugged in, each asked at once.
    At(HashSet<PathBuf>),
}

impl Finds {
    fn finds(&self, at: &Path) -> bool {
        match self {
            Finds::Nothing => false,
            Finds::Every => true,
            Finds::At(points) => points.contains(at),
        }
    }

    fn plugs(&self, at: &Path) -> bool {
        matches!(self, Finds::At(points) if points.contains(at))
    }
}

/// Whether a filesystem just mounted at `point` may be a drive skwird sees:
/// where the system mounts what the user plugs in (udisks', a Mac's
/// `/Volumes`), or on a drive that can be unplugged. A FUSE program's, a
/// container's layers, a loop or bind mount elsewhere, are not waited for.
fn may_be_drive(point: &Path) -> bool {
    skwir_core::fs::volume::user_mounted(point) || skwir_core::fs::volume::is_removable(point)
}

/// Whether a check runs, and what was asked while it did.
#[derive(Default)]
struct Checking {
    running: bool,
    again: Option<Asking>,
    /// When a listing (the page, a drive's menu) last asked one.
    listed: Option<Instant>,
}

/// skwird did not answer as asked.
enum Unanswered {
    /// No engine that indexes drives answered.
    NoEngine,
    /// The engine refused, saying why, in its own words.
    Refused(String),
}

/// What a check's answer did.
struct Took {
    /// Taken: no answer to a later ask stood already.
    applied: bool,
    /// The labels changed, for the caller to save.
    relabelled: bool,
}

/// The drives' keeper for one run of the plugin.
pub(crate) struct Volumes {
    pub(crate) source: RecordSource,
    app: Weak<App>,
    engines: Engines,
    /// Where filesystems are mounted now.
    mounted: Mounted,
    /// This run's: cancelled once the plugin is turned off.
    lifetime: CancelToken,
    memory: Mutex<Memory>,
    /// Taken before memory, held until the window is told: see `settle`.
    questions: Mutex<Questions>,
    /// One saves at a time, what it writes read under it.
    saving: Mutex<()>,
    checking: Mutex<Checking>,
    /// The tickets of skwird's asks, one taken just before each.
    asks: AtomicU64,
    /// Whether a window was looked at this run.
    looked: AtomicBool,
    /// How many looks were told: what a test waits on, with `idle`.
    pub(crate) looks: AtomicU64,
    /// The jobs given and not ended.
    working: AtomicUsize,
}

/// Counts a job down whether it runs or is skipped, cancelled.
struct Working(Arc<Volumes>);

impl Drop for Working {
    fn drop(&mut self) {
        self.0.working.fetch_sub(1, Ordering::AcqRel);
    }
}

impl Volumes {
    /// Let go of what this run follows and does.
    pub(crate) fn stop(&self) {
        self.lifetime.cancel();
    }

    /// Whether no job of it is under way: what a test waits on (its
    /// tests run where the fake engine's Unix socket does).
    #[cfg(all(test, unix))]
    pub(crate) fn idle(&self) -> bool {
        self.working.load(Ordering::Acquire) == 0
    }

    /// `job` on a worker of this run.
    fn spawn(
        self: &Arc<Self>,
        priority: Priority,
        label: &'static str,
        job: impl FnOnce(&Arc<Volumes>, &App) + Send + 'static,
    ) {
        let Some(app) = self.app.upgrade() else {
            return;
        };
        self.working.fetch_add(1, Ordering::AcqRel);
        let working = Working(Arc::clone(self));
        app.scheduler
            .spawn(priority, label, self.lifetime.child(), move || {
                let this = Arc::clone(&working.0);
                if let Some(app) = this.app.upgrade() {
                    job(&this, &app);
                }
                drop(working);
            });
    }

    /// A window came to the front: at the first look of this run, skwird
    /// is asked what it sees, which is offered; at a later one, a drive
    /// found while no window of the user's own was looked at is asked.
    pub(crate) fn looked_at(self: &Arc<Self>) {
        if self.looked.swap(true, Ordering::AcqRel) {
            self.spawn(Priority::Background, "skwird.volumes_look", |this, app| {
                this.ask_next(app, &[]);
            });
        } else {
            self.request(Asking {
                first_look: true,
                mounts: false,
            });
        }
        self.looks.fetch_add(1, Ordering::AcqRel);
    }

    /// A mount was made or let go of, by any program (`reach::follow`).
    pub(crate) fn mounts_changed(self: &Arc<Self>) {
        self.request(Asking {
            first_look: false,
            mounts: true,
        });
    }

    /// [`request`](Self::request) for a listing: once every
    /// `LISTED_EVERY` at most.
    fn refresh_listed(self: &Arc<Self>) {
        {
            let mut checking = self.checking.lock();
            if checking
                .listed
                .is_some_and(|at| at.elapsed() < LISTED_EVERY)
            {
                return;
            }
            checking.listed = Some(Instant::now());
        }
        self.request(Asking::default());
    }

    /// Ask skwird what it sees, on a worker, one at a time: one asked while
    /// another runs makes it go once more, as both asked.
    fn request(self: &Arc<Self>, asking: Asking) {
        {
            let mut checking = self.checking.lock();
            if checking.running {
                checking.again = Some(checking.again.unwrap_or_default().with(asking));
                return;
            }
            checking.running = true;
        }
        self.spawn(Priority::Background, "skwird.volumes", move |this, app| {
            let mut asking = asking;
            loop {
                this.check(app, asking);
                let mut checking = this.checking.lock();
                match checking.again.take() {
                    Some(next) if !this.lifetime.is_cancelled() => asking = next,
                    _ => {
                        checking.running = false;
                        return;
                    }
                }
            }
        });
    }

    /// What skwird sees, its drives found as `asking` says; asked once more
    /// a moment later where a mount just made, which may be a drive, is one
    /// skwird does not see yet.
    fn check(self: &Arc<Self>, app: &App, asking: Asking) {
        let finds = if asking.first_look || asking.mounts {
            match self.points_since() {
                Some(fresh) if !asking.first_look => Finds::At(fresh),
                _ => Finds::Every,
            }
        } else {
            Finds::Nothing
        };
        let answer = self.check_once(app, &finds);
        let late = match (&finds, &answer) {
            (Finds::At(fresh), Some(answer)) => {
                let seen: HashSet<PathBuf> = answer.seen.iter().map(|s| s.at.to_path()).collect();
                fresh
                    .iter()
                    .any(|point| !seen.contains(point) && may_be_drive(point))
            }
            _ => false,
        };
        if late
            && self
                .lifetime
                .subscribe()
                .receiver()
                .recv_timeout(AGAIN)
                .is_err()
        {
            self.check_once(app, &finds);
        }
    }

    fn check_once(self: &Arc<Self>, app: &App, finds: &Finds) -> Option<protocol::Volumes> {
        let (ticket, answer) = self.with_engine(false, |client| client.volumes(&[], &[], &[]));
        let answer = answer.ok();
        let took = self.took(app, ticket, answer.clone());
        if took.relabelled {
            self.save(app);
        }
        if !took.applied {
            return answer;
        }
        if let Some(answer) = &answer {
            if !matches!(finds, Finds::Nothing) {
                let plugged = self.offers(app, answer, finds);
                self.ask_next(app, &plugged);
            }
        } else {
            // No engine to say which drives went: the table says it.
            let mounted = (self.mounted)();
            self.settle(app, |questions, _| {
                questions.moot(|offer| mounted.contains(&offer.at));
            });
        }
        answer
    }

    /// The mount points of files kept made since the last look at them:
    /// none where none was seen before.
    fn points_since(&self) -> Option<HashSet<PathBuf>> {
        let now = (self.mounted)();
        let mut memory = self.memory.lock();
        let fresh = memory
            .points
            .as_ref()
            .map(|before| now.difference(before).cloned().collect());
        memory.points = Some(now);
        fresh
    }

    /// `ask` of an engine that says `volumes`, started where none runs if
    /// `start` and the Index page's switch allows it; given up on once this
    /// run is. With the ticket taken just before it asked (`took`).
    fn with_engine<T>(
        &self,
        start: bool,
        ask: impl FnOnce(&mut Client) -> Result<T, skwird_client::Error>,
    ) -> (u64, Result<T, Unanswered>) {
        let ticket = || self.asks.fetch_add(1, Ordering::AcqRel) + 1;
        let start = start
            && self
                .app
                .upgrade()
                .is_none_or(|app| app.settings.get::<bool>(crate::kept::START).unwrap_or(true));
        let Some(mut find) = crate::find(&self.engines, start) else {
            return (ticket(), Err(Unanswered::NoEngine));
        };
        find.silence = Some(VOLUMES_SILENCE);
        if self.lifetime.is_cancelled() {
            return (ticket(), Err(Unanswered::NoEngine));
        }
        let (mut client, _stop) = match crate::connect(&find, &self.lifetime, crate::HELLO) {
            Ok(connected) => connected,
            Err(error) => {
                crate::said("connect", &error);
                return (ticket(), Err(Unanswered::NoEngine));
            }
        };
        if !says_volumes(&client) {
            return (ticket(), Err(Unanswered::NoEngine));
        }
        let asked = ticket();
        let answer = ask(&mut client).map_err(|error| {
            crate::said("volumes", &error);
            match error {
                skwird_client::Error::Engine(failure) => Unanswered::Refused(failure.message),
                _ => Unanswered::NoEngine,
            }
        });
        (asked, answer)
    }

    /// Keep what skwird said, asked with `ticket`, unless an answer to a
    /// later ask stands already; the labels of the drives it keeps with it
    /// (as mounted now, else as remembered), and say so where it changed.
    fn took(&self, app: &App, ticket: u64, answer: Option<protocol::Volumes>) -> Took {
        let (changed, relabelled) = {
            let mut memory = self.memory.lock();
            // Of two answers, the one asked later stands, whichever came
            // last.
            if ticket < memory.said_at {
                return Took {
                    applied: false,
                    relabelled: false,
                };
            }
            memory.said_at = ticket;
            let mut relabelled = false;
            if let Some(answer) = &answer {
                let seen: HashMap<&str, &protocol::SeenVolume> = answer
                    .seen
                    .iter()
                    .filter_map(|s| Some((s.id.as_deref()?, s)))
                    .collect();
                let kept: HashSet<&str> = answer.kept.iter().map(|k| k.id.as_str()).collect();
                let gone: Vec<String> = memory
                    .remembered
                    .labels
                    .keys()
                    .filter(|id| !kept.contains(id.as_str()))
                    .cloned()
                    .collect();
                for id in &gone {
                    relabelled |= memory.unlabelled(id);
                }
                for id in &kept {
                    if let Some(seen) = seen.get(id) {
                        relabelled |= memory.labelled(id, label_of(seen));
                    }
                }
            }
            let changed = memory.said != answer;
            memory.said = answer;
            (changed, relabelled)
        };
        if changed || relabelled {
            self.source.changed();
            app.wake();
        }
        Took {
            applied: true,
            relabelled,
        }
    }

    /// The one way what is counted asked, the drives waiting and the
    /// question change: `change` edits them, under `questions` held until
    /// the window is told, so no other asker comes between. Whatever it did,
    /// after it the drive up is counted asked, no drive counted asked
    /// waits, and the window's question is the one up: asked where another
    /// is up, taken back where none is. Saved where what is counted asked
    /// changed. On a worker: it may write the state file.
    fn settle<T>(&self, app: &App, change: impl FnOnce(&mut Questions, &mut Memory) -> T) -> T {
        let mut questions = self.questions.lock();
        let before = questions.up.as_ref().map(|up| up.id.clone());
        let (out, counted) = {
            let mut memory = self.memory.lock();
            let counted = memory.remembered.asked.clone();
            questions
                .waiting
                .retain(|offer| !memory.remembered.asked.contains(&offer.id));
            let out = change(&mut questions, &mut memory);
            let asked = &mut memory.remembered.asked;
            if let Some(up) = &questions.up {
                asked.insert(up.id.clone());
            }
            questions.waiting.retain(|offer| !asked.contains(&offer.id));
            (out, counted)
        };
        if questions.up.as_ref().map(|up| &up.id) != before.as_ref() {
            match &questions.up {
                Some(offer) => {
                    app.ask_quietly(ID, question(&offer.label), answers(&offer.id));
                    // Refused (the plugin turned off meanwhile): not asked.
                    if self.lifetime.is_cancelled() || !app.registry.gate().is_open(ID) {
                        let offer = questions.up.take().map(|offer| offer.id);
                        let mut memory = self.memory.lock();
                        if let Some(id) = offer.filter(|id| !counted.contains(id)) {
                            memory.remembered.asked.remove(&id);
                        }
                    }
                }
                None => app.take_back_question(ID),
            }
        }
        drop(questions);
        let grew = self.memory.lock().remembered.asked != counted;
        if grew {
            self.save(app);
        }
        out
    }

    /// Note the drives `answer` offers to ask about, those `finds` finds
    /// (each seen with an identity, not kept, never asked), letting go of
    /// those it no longer offers, and take back a question about a drive
    /// gone or kept since: the drives found plugged in, which are asked
    /// first.
    fn offers(&self, app: &App, answer: &protocol::Volumes, finds: &Finds) -> Vec<String> {
        let kept: HashSet<&str> = answer.kept.iter().map(|k| k.id.as_str()).collect();
        let seen: HashMap<&str, &protocol::SeenVolume> = answer
            .seen
            .iter()
            .filter_map(|s| Some((s.id.as_deref().filter(|id| !id.is_empty())?, s)))
            .collect();
        self.settle(app, |questions, memory| {
            questions.moot(|offer| {
                seen.contains_key(offer.id.as_str()) && !kept.contains(offer.id.as_str())
            });
            // Where each is mounted now.
            for offer in questions.waiting.iter_mut().chain(questions.up.iter_mut()) {
                if let Some(seen) = seen.get(offer.id.as_str()) {
                    offer.at = seen.at.to_path();
                }
            }
            let held = questions.held();
            let asked = &memory.remembered.asked;
            let mut plugged = Vec::new();
            for volume in &answer.seen {
                let Some(id) = volume.id.as_deref().filter(|id| !id.is_empty()) else {
                    continue;
                };
                let at = volume.at.to_path();
                if kept.contains(id) || asked.contains(id) || held.contains(id) || !finds.finds(&at)
                {
                    continue;
                }
                if finds.plugs(&at) {
                    plugged.push(id.to_owned());
                }
                questions.waiting.push_back(Offer {
                    id: id.to_owned(),
                    label: label_of(volume),
                    at,
                });
            }
            plugged
        })
    }

    /// Ask about the next drive waiting, where a window of the user's own is
    /// looked at and skwird's last answer still offers it: one of `plugged`
    /// first, taking the place of a question up, else the first found while
    /// none is up. Counted asked as it is asked, and kept so. None while no
    /// engine answers. On a worker: it reads and writes the state file.
    fn ask_next(&self, app: &App, plugged: &[String]) {
        if self.lifetime.is_cancelled()
            || !app.looked_at_own()
            || self.questions.lock().waiting.is_empty()
        {
            return;
        }
        // Another skwir may have asked meanwhile.
        if let Some(theirs) = on_disk(app) {
            self.memory.lock().merge(&theirs);
        }
        let plugged: HashSet<&str> = plugged.iter().map(String::as_str).collect();
        self.settle(app, |questions, memory| {
            let Some(said) = &memory.said else {
                return;
            };
            let seen: HashSet<&str> = said.seen.iter().filter_map(|s| s.id.as_deref()).collect();
            let kept: HashSet<&str> = said.kept.iter().map(|k| k.id.as_str()).collect();
            questions.waiting.retain(|offer| {
                seen.contains(offer.id.as_str()) && !kept.contains(offer.id.as_str())
            });
            let at = questions
                .waiting
                .iter()
                .position(|offer| plugged.contains(offer.id.as_str()))
                .or_else(|| (questions.up.is_none() && !questions.waiting.is_empty()).then_some(0));
            if let Some(offer) = at.and_then(|at| questions.waiting.remove(at)) {
                questions.up = Some(offer);
            }
        });
    }

    /// The answer to a drive's question (its buttons): kept if Yes, then
    /// the next drive waiting asked.
    fn answer(&self, app: &App, id: &str, keep: bool) -> anyhow::Result<()> {
        let kept = if keep {
            self.keep(app, id)
        } else {
            self.settle(app, |questions, memory| {
                questions.decided(&mut memory.remembered.asked, id);
            });
            Ok(())
        };
        self.ask_next(app, &[]);
        kept
    }

    /// Have skwird index the drive `id`, its label remembered: decided,
    /// whatever skwird answers.
    fn keep(&self, app: &App, id: &str) -> anyhow::Result<()> {
        let label = self.memory.lock().label(id);
        self.settle(app, |questions, memory| {
            questions.decided(&mut memory.remembered.asked, id);
        });
        let (ticket, answer) = self.with_engine(true, |client| client.volumes(&[id], &[], &[]));
        let answer = match answer {
            Ok(answer) => answer,
            Err(Unanswered::NoEngine) => anyhow::bail!(skwir_core::words!(
                skwird_volume_not_kept = "The names on { $name } are not indexed: no engine of skwird's that indexes drives answered.",
                name = Name(label),
                note = "An error, once the user said yes to indexing a drive's names: name is the drive's label. skwird is a program's name, as written."
            )),
            Err(Unanswered::Refused(why)) => {
                return Err(anyhow::Error::msg(why).context(skwir_core::words!(
                    skwird_volume_keep_refused = "skwird refused to index the names on { $name }",
                    name = Name(label),
                    note = "An error, once the user said yes to indexing a drive's names, followed by what skwird said, as it said it: name is the drive's label. skwird is a program's name, as written."
                )));
            }
        };
        let taken = answer.kept.iter().any(|kept| kept.id == id);
        if taken {
            self.memory.lock().labelled(id, label.clone());
        }
        self.took(app, ticket, Some(answer));
        self.save(app);
        if !taken {
            anyhow::bail!(skwir_core::words!(
                skwird_volume_not_taken =
                    "The names on { $name } are not indexed: skwird did not take the drive.",
                name = Name(label),
                note = "An error, once the user said yes to indexing a drive's names, where skwird answered without keeping it (the drive gone meanwhile): name is the drive's label. skwird is a program's name, as written."
            ));
        }
        Ok(())
    }

    /// Have skwird index the drive `id` no more and remove its index:
    /// decided, whatever skwird answers.
    fn forget(&self, app: &App, id: &str) -> anyhow::Result<()> {
        let label = self.memory.lock().label(id);
        self.settle(app, |questions, memory| {
            questions.decided(&mut memory.remembered.asked, id);
        });
        let (ticket, answer) = self.with_engine(true, |client| client.volumes(&[], &[id], &[]));
        let answer = match answer {
            Ok(answer) => answer,
            Err(Unanswered::NoEngine) => anyhow::bail!(skwir_core::words!(
                skwird_volume_not_forgotten = "The index of { $name } is not removed: no engine of skwird's that indexes drives answered.",
                name = Name(label),
                note = "An error, once the user asked to forget a drive's index: name is the drive's label. skwird is a program's name, as written."
            )),
            Err(Unanswered::Refused(why)) => {
                return Err(anyhow::Error::msg(why).context(skwir_core::words!(
                    skwird_volume_forget_refused = "skwird refused to remove the index of { $name }",
                    name = Name(label),
                    note = "An error, once the user asked to forget a drive's index, followed by what skwird said, as it said it: name is the drive's label. skwird is a program's name, as written."
                )));
            }
        };
        let still = answer.kept.iter().any(|kept| kept.id == id);
        if !still {
            self.memory.lock().forgot(id);
        }
        self.took(app, ticket, Some(answer));
        self.save(app);
        if still {
            anyhow::bail!(skwir_core::words!(
                skwird_volume_still_kept =
                    "The index of { $name } is not removed: skwird still keeps the drive.",
                name = Name(label),
                note = "An error, once the user asked to forget a drive's index, where skwird answered still keeping it: name is the drive's label. skwird is a program's name, as written."
            ));
        }
        Ok(())
    }

    /// Write what is kept into the state file, another skwir's merged in
    /// first. Reads and writes the disk: on a worker.
    fn save(&self, app: &App) {
        if self.lifetime.is_cancelled() {
            return;
        }
        let _saving = self.saving.lock();
        let theirs = on_disk(app);
        let value = {
            let mut memory = self.memory.lock();
            if let Some(theirs) = theirs {
                memory.merge(&theirs);
            }
            toml::Value::try_from(&memory.remembered)
        };
        match value {
            Ok(value) => {
                app.settings.set(STATE, value);
                if let Err(error) = app.settings.save() {
                    tracing::warn!("what skwird's plugin keeps of drives is not saved: {error:#}");
                }
            }
            Err(error) => {
                tracing::warn!(%error, "what skwird's plugin keeps of drives is not written");
            }
        }
    }

    /// The list as the Index page draws it (`RecordSource`): from memory,
    /// skwird asked again for the next listing.
    fn list(self: &Arc<Self>) -> serde_json::Value {
        self.refresh_listed();
        let memory = self.memory.lock();
        let record = |id: &str, label: &str, state: Words| {
            serde_json::json!({
                "id": id,
                "name": Words::name_alone(label).to_data(),
                "state": state.to_data(),
                "actions": [{
                    "label": skwir_core::words!(
                        skwird_volume_forget = "Forget",
                        note = "A button on a drive's line in Settings › Index: skwird indexes its names no more and removes its index."
                    ).to_data(),
                    "command": "skwird.forget_volume",
                    "args": { "id": id },
                }],
            })
        };
        let records: Vec<serde_json::Value> = match &memory.said {
            Some(said) => said
                .kept
                .iter()
                .map(|kept| record(&kept.id, &memory.label(&kept.id), standing(kept)))
                .collect(),
            None => memory
                .remembered
                .labels
                .iter()
                .map(|(id, label)| {
                    record(
                        id,
                        label,
                        skwir_core::words!(
                            skwird_volume_unknown = "Indexed when skwird runs",
                            note = "Under a drive's name in the list of drives indexed, while no engine of skwird's answers. skwird is a program's name, as written."
                        ),
                    )
                })
                .collect(),
        };
        serde_json::json!({ "records": records })
    }

    /// A drive's entry in its place's menu: from memory, for a mount point
    /// skwird's last answer saw with an identity, checked while it is kept.
    fn menu(self: &Arc<Self>, location: &PathArg) -> serde_json::Value {
        self.refresh_listed();
        let point = Location::parse_os(location.as_os_str()).path();
        let item = point.and_then(|point| {
            let memory = self.memory.lock();
            let said = memory.said.as_ref()?;
            let id = said
                .seen
                .iter()
                .find(|s| s.at.to_path() == point)?
                .id
                .clone()
                .filter(|id| !id.is_empty())?;
            let kept = said.kept.iter().any(|k| k.id == id);
            let command = if kept {
                "skwird.forget_volume"
            } else {
                "skwird.keep_volume"
            };
            Some(
                MenuItem::new(
                    "place",
                    skwir_core::words!(
                        skwird_volume_item = "Index Its Names",
                        note = "An entry of a drive's menu in the sidebar: skwird indexes the names of the files on it, so that searches find them at once. Checked while it does."
                    ),
                    "search",
                    165,
                    Invocation::new(command).with("id", &id),
                )
                .checked(kept),
            )
        });
        let items: Vec<MenuItem> = item.into_iter().collect();
        skwir_core::registry::menu_answer(&items)
    }
}

/// What the state file holds of the drives now: another skwir's writes
/// too. Reads the disk: on a worker.
fn on_disk(app: &App) -> Option<Remembered> {
    app.settings.kept_on_disk(STATE).and_then(Remembered::of)
}

/// Whether the engine `client` talks to said, in this connection's hello,
/// that it indexes removable drives (`protocol::VOLUMES`).
fn says_volumes(client: &Client) -> bool {
    client
        .welcome()
        .features
        .iter()
        .any(|word| word == protocol::VOLUMES)
}

/// What a drive is called: its label, else its mount point's name.
fn label_of(volume: &protocol::SeenVolume) -> String {
    volume
        .label
        .clone()
        .filter(|label| !label.is_empty())
        .unwrap_or_else(|| Name::of(&volume.at.to_path()).0)
}

/// The question about the drive `label`.
fn question(label: &str) -> Words {
    skwir_core::words!(
        skwird_volume_ask = "Index the names on { $label } so searches find them at once? The index stays on this computer, never on the drive.",
        label = Name(label.to_owned()),
        note = "Asked once, in a message that stays, as a removable drive (a USB stick, a card) is plugged in: whether skwird indexes the names of its files. label: the drive's label, else the name of the folder it is mounted on."
    )
}

/// The question's buttons.
fn answers(id: &str) -> Vec<NotifyAction> {
    let answer = |keep: bool| {
        Invocation::with_args(
            "skwird.volume_answer",
            &serde_json::json!({ "id": id, "keep": keep }),
        )
    };
    vec![
        NotifyAction {
            label: skwir_core::words!(
                skwird_volume_yes = "Yes",
                note = "The button saying yes to indexing the names on a drive just plugged in."
            ),
            command: answer(true),
            answers: true,
        },
        NotifyAction {
            label: skwir_core::words!(
                skwird_volume_no = "No",
                note = "The button saying no to indexing the names on a drive just plugged in."
            ),
            command: answer(false),
            answers: true,
        },
    ]
}

/// Where a kept drive stands, as a row of the list says it.
fn standing(kept: &protocol::KeptVolume) -> Words {
    let indexed = || {
        skwir_core::words!(
            skwird_volume_indexed = "Indexed",
            note = "Under a drive's name in the list of drives indexed: plugged in, a root of skwird's index."
        )
    };
    let when = |at: protocol::Instant| Moment::of(crate::time_of(at), Shape::DateTime);
    match kept.standing {
        protocol::Standing::Away => skwir_core::words!(
            skwird_volume_away = "Not plugged in",
            note = "Under a drive's name in the list of drives indexed: not mounted now; its index is kept."
        ),
        protocol::Standing::Released => skwir_core::words!(
            skwird_volume_released = "Let go of for an eject",
            note = "Under a drive's name in the list of drives indexed: skwird closed all it held on it before it was ejected, and reads nothing there until it is plugged in again."
        ),
        protocol::Standing::Twin => skwir_core::words!(
            skwird_volume_twin = "Refused: another drive of the same identity is indexed",
            note = "Under a drive's name in the list of drives indexed: a copy of a drive, or two drives whose serial numbers are alike; the one indexed first is kept."
        ),
        protocol::Standing::Indexed => match kept.coverage.as_ref().map(|c| c.names) {
            Some(protocol::Proof::Checked(at)) => when(at).map_or_else(indexed, |when| {
                skwir_core::words!(
                    skwird_volume_checked = "Indexed, checked { $when }",
                    when = when,
                    note = "Under a drive's name in the list of drives indexed: its index was compared with its folders then. when: the date and time."
                )
            }),
            Some(protocol::Proof::Live(since)) => when(since).map_or_else(indexed, |when| {
                skwir_core::words!(
                    skwird_volume_live = "Indexed, live since { $when }",
                    when = when,
                    note = "Under a drive's name in the list of drives indexed: every change on it heard as it comes since then. when: the date and time."
                )
            }),
            Some(protocol::Proof::AsOf(at)) => when(at).map_or_else(indexed, |when| {
                skwir_core::words!(
                    skwird_volume_as_of = "Indexed as of { $when }, being checked",
                    when = when,
                    note = "Under a drive's name in the list of drives indexed: its index was read back as it was then, and is being compared with its folders. when: the date and time."
                )
            }),
            Some(protocol::Proof::NotCovered(_)) => skwir_core::words!(
                skwird_volume_reading = "Indexed, being read",
                note = "Under a drive's name in the list of drives indexed: plugged in, skwird reads its names for the first time or checks them."
            ),
            None => indexed(),
        },
    }
}

#[derive(Deserialize, schemars::JsonSchema)]
struct VolumeArgs {
    /// A removable drive, by the identity skwird spells (`id` in
    /// `skwird.volumes_list`).
    id: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct AnswerArgs {
    /// The drive asked about, by the identity skwird spells.
    id: String,
    /// Whether to index its names.
    keep: bool,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct PlaceArgs {
    /// The place's location, as the sidebar has it.
    location: PathArg,
    /// The pane the menu opens in, which a menu gives every source.
    #[serde(default, rename = "pane")]
    _pane: Option<skwir_core::workspace::PaneId>,
}

/// Run `job` for a command: on a worker when asked from the window, what
/// fails said there; where asked otherwise (an agent's call, a script's),
/// there, what fails its error.
fn carried(
    volumes: &Arc<Volumes>,
    label: &'static str,
    job: impl FnOnce(&Volumes, &App) -> anyhow::Result<()> + Send + 'static,
    app: &App,
) -> anyhow::Result<Outcome> {
    if skwir_core::scheduler::on_window_thread() {
        volumes.spawn(Priority::Normal, label, move |this, app| {
            if let Err(error) = job(this.as_ref(), app) {
                app.notify_from(ID, NotifyLevel::Error, Words::of_error(error), Vec::new());
            }
        });
    } else {
        job(volumes.as_ref(), app)?;
    }
    Ok(Outcome::Done)
}

/// Declare what Skwir keeps of the drives, the list, the commands and the
/// menu's entry, and follow the mounts where an engine may be asked: the
/// drives' keeper of this run, reading where filesystems are mounted from
/// `mounted`.
pub(crate) fn register(app: &App, engines: &Engines, mounted: Mounted) -> Arc<Volumes> {
    app.settings.declare(SettingSpec::state(
        STATE,
        skwir_core::words!(
            skwird_volumes_state_description = "What skwird's plugin keeps of removable drives: those asked about once, and the labels of those indexed."
        ),
        toml::Value::Table(toml::Table::new()),
    ));
    let remembered = app
        .settings
        .get::<toml::Value>(STATE)
        .and_then(Remembered::of)
        .unwrap_or_default();
    let volumes = Arc::new(Volumes {
        source: RecordSource::new("skwird.volumes_list", "", "", ""),
        app: app.weak(),
        engines: engines.clone(),
        mounted,
        lifetime: app.lifetime().child(),
        memory: Mutex::new(Memory {
            remembered,
            ..Memory::default()
        }),
        questions: Mutex::default(),
        saving: Mutex::default(),
        checking: Mutex::default(),
        asks: AtomicU64::new(0),
        looked: AtomicBool::new(false),
        looks: AtomicU64::new(0),
        working: AtomicUsize::new(0),
    });
    app.settings.declare(
        SettingSpec::records(
            LIST,
            skwir_core::words!(
                skwird_volumes_title = "Drives indexed",
                note = "A setting's title on the Index page: the removable drives (USB sticks, cards) whose file names skwird indexes."
            ),
            (
                skwir_core::words!(
                    skwird_volumes_description = "The removable drives whose file names skwird indexes, each known by its own identity; their index stays on this computer, never on the drive.",
                    note = "What the list of drives indexed says. skwird is a program's name, as written."
                ),
                skwir_core::words!(
                    skwird_volumes_description_more = "Skwir asks once as a drive is plugged in while skwird runs; a drive's menu in the sidebar also indexes it, or forgets its index.",
                    note = "What a setting's description says after its first sentence, in its tooltip."
                ),
            ),
            Words::default(),
            vec![
                RecordField::text(
                    "name",
                    skwir_core::words!(
                        skwird_volumes_name = "Drive",
                        note = "A field of a line in the list of drives indexed: the drive's label."
                    ),
                    Words::default(),
                    Words::default(),
                ),
                RecordField::text(
                    "state",
                    skwir_core::words!(skwird_kept_state = "State"),
                    Words::default(),
                    Words::default(),
                ),
            ],
        )
        .records_kept_by(volumes.source.clone())
        .page(page::INDEX)
        .section(section::NAMES_INDEX),
    );
    register_commands(&volumes, app);
    // Asked as a place's menu opens: from memory.
    app.registry.menu_sources.add(
        ID,
        MenuSource {
            menu: "place".into(),
            when: None,
            command: Invocation::new("skwird.volume_menu"),
            replaces: Vec::new(),
        },
    );
    if !matches!(engines, Engines::None) {
        let weak = Arc::downgrade(&volumes);
        // On the table's own thread: a check is only started there.
        skwir_core::fs::reach::follow(&volumes.lifetime, move || {
            if let Some(volumes) = weak.upgrade() {
                volumes.mounts_changed();
            }
        });
    }
    volumes
}

fn register_commands(volumes: &Arc<Volumes>, app: &App) {
    let v = Arc::clone(volumes);
    app.commands
        .register::<NoArgs, _>(
            ID,
            Topic::Find,
            "skwird.volumes_list",
            skwir_core::words!(skwird_volumes_list = "Drives Indexed"),
            skwir_core::words!(
                skwird_volumes_list_description = "The removable drives whose names skwird indexes, as the Index page lists them: each one's identity, label and standing, from memory"
            ),
            move |_, _| Ok(Outcome::Value(v.list())),
        )
        .data();
    // The user's alone: an agent never has a drive indexed (ADR-150, 18).
    let v = Arc::clone(volumes);
    let keep = app.commands.register::<VolumeArgs, _>(
        ID,
        Topic::Find,
        "skwird.keep_volume",
        skwir_core::words!(skwird_keep_volume = "Index a Drive's Names"),
        skwir_core::words!(
            skwird_keep_volume_description = "Have skwird index the file names on a removable drive, by the identity skwird spells, from now on whenever it is plugged in"
        ),
        move |args, app| {
            carried(
                &v,
                "skwird.keep_volume",
                move |this, app| this.keep(app, &args.id),
                app,
            )
        },
    );
    keep.asked_only();
    keep.effect(Effect::UserOnly);
    // An agent's call asks the user first: an index removed is read again.
    let v = Arc::clone(volumes);
    app.commands
        .register::<VolumeArgs, _>(
            ID,
            Topic::Find,
            "skwird.forget_volume",
            skwir_core::words!(skwird_forget_volume = "Forget a Drive's Index"),
            skwir_core::words!(
                skwird_forget_volume_description = "Have skwird index a removable drive no more, by the identity skwird spells, and remove its index"
            ),
            move |args, app| {
                carried(
                    &v,
                    "skwird.forget_volume",
                    move |this, app| this.forget(app, &args.id),
                    app,
                )
            },
        )
        .asked_only();
    let v = Arc::clone(volumes);
    let answer = app.commands.register::<AnswerArgs, _>(
        ID,
        Topic::Find,
        "skwird.volume_answer",
        skwir_core::words!(skwird_volume_answer = "Answer Whether to Index a Drive"),
        skwir_core::words!(
            skwird_volume_answer_description = "The answer to the question asked once as a removable drive is plugged in: whether skwird indexes its names"
        ),
        move |args, app| {
            carried(
                &v,
                "skwird.volume_answer",
                move |this, app| this.answer(app, &args.id, args.keep),
                app,
            )
        },
    );
    answer.asked_only();
    answer.effect(Effect::UserOnly);
    let v = Arc::clone(volumes);
    app.commands
        .register::<PlaceArgs, _>(
            ID,
            Topic::Find,
            "skwird.volume_menu",
            skwir_core::words!(skwird_volume_menu = "Index Its Names in a Drive's Menu"),
            skwir_core::words!(
                skwird_volume_menu_description = "A sidebar drive's entry indexing its names, or forgetting its index, where skwird last saw it mounted with an identity. Answers from memory"
            ),
            move |args, _| Ok(Outcome::Value(v.menu(&args.location))),
        )
        .data();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remembered(labels: &[(&str, &str)], since: &[(&str, i64)]) -> Remembered {
        Remembered {
            labels: labels
                .iter()
                .map(|(id, label)| ((*id).to_owned(), (*label).to_owned()))
                .collect(),
            since: since
                .iter()
                .map(|(id, at)| ((*id).to_owned(), *at))
                .collect(),
            ..Remembered::default()
        }
    }

    /// Of two skwirs on one state file, the later word on a drive stands: a
    /// drive one forgot is not brought back by the label the other still
    /// holds, kept again later it is; a label written with no time is taken
    /// only where nothing is known of its drive.
    #[test]
    fn of_two_skwirs_the_later_word_on_a_drive_stands() {
        // Forgotten here at 20, kept there at 10.
        let mut ours = Memory {
            remembered: remembered(&[], &[("P", 20)]),
            ..Memory::default()
        };
        ours.merge(&remembered(&[("P", "PHOTOS")], &[("P", 10)]));
        assert!(!ours.remembered.labels.contains_key("P"), "forgotten later");
        // Kept there again at 30.
        ours.merge(&remembered(&[("P", "PHOTOS")], &[("P", 30)]));
        assert_eq!(
            ours.remembered.labels.get("P").map(String::as_str),
            Some("PHOTOS")
        );
        assert_eq!(ours.remembered.since.get("P"), Some(&30));
        // Forgotten there at 40: let go of here too.
        ours.merge(&remembered(&[], &[("P", 40)]));
        assert!(!ours.remembered.labels.contains_key("P"));
        // A label of no time: taken only for a drive unknown here.
        ours.merge(&remembered(&[("P", "OLD"), ("Q", "CARD")], &[]));
        assert!(!ours.remembered.labels.contains_key("P"), "known here");
        assert_eq!(
            ours.remembered.labels.get("Q").map(String::as_str),
            Some("CARD")
        );
    }

    /// Of two answers, the one asked later stands, whichever comes last.
    #[test]
    fn an_answer_asked_before_another_taken_is_let_go_of() {
        let tmp = tempfile::tempdir().unwrap();
        let app = App::new(skwir_core::app::AppConfig::ephemeral(tmp.path())).unwrap();
        let volumes = register(&app, &Engines::None, Arc::new(HashSet::new));
        let kept = |ids: &[&str]| protocol::Volumes {
            kept: ids
                .iter()
                .map(|id| protocol::KeptVolume {
                    id: (*id).to_owned(),
                    standing: protocol::Standing::Away,
                    at: None,
                    coverage: None,
                })
                .collect(),
            seen: Vec::new(),
        };
        assert!(volumes.took(&app, 2, Some(kept(&["P"]))).applied);
        assert!(
            !volumes.took(&app, 1, Some(kept(&[]))).applied,
            "asked before"
        );
        assert_eq!(volumes.memory.lock().said, Some(kept(&["P"])));
        assert!(volumes.took(&app, 3, None).applied, "asked later");
        assert_eq!(volumes.memory.lock().said, None);
    }
}
