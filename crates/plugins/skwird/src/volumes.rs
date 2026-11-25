//! Removable drives (ADR-150, 18): skwird 0.2 indexes the file names on a
//! removable drive once its user said yes for that drive, by the identity
//! the engine spells (`SeenVolume::id`; Skwir never derives one), and keeps
//! it for this client across runs. How Skwir asks is the user's choice
//! (2026-11-25):
//!
//! - **Once a drive, as it is plugged in.** A drive mounted while Skwir runs
//!   (`reach::follow`), or mounted before and found at the first look of a
//!   window (`Plugin::on_looked_at`), is offered where an engine already
//!   runs and says `volumes`, the drive has an identity, this client keeps
//!   it not and its user was never asked about it: a message that stays
//!   (`App::ask_quietly`), Yes or No. Never an engine started to ask. Its
//!   identity is counted asked as it is asked, so a question closed
//!   unanswered is a No. One question stands at a time: the next waits for
//!   its answer, and a drive plugged in meanwhile takes its place, the one
//!   it replaces counted asked. The question waits for a window of the
//!   user's own (ADR-141, 8).
//! - **A drive's menu in the sidebar**, Index Its Names, checked while it is
//!   kept: from memory, for a mount point skwird's last answer saw with an
//!   identity.
//! - **The Index page lists the drives kept**, each with Forget, which takes
//!   it from what is kept and has skwird remove its index.
//!
//! What Skwir keeps of it is state (ADR-056, `skwird.volumes`): the
//! identities asked, and each kept drive's label, which a drive away has
//! not in skwird's answer. Nothing of it goes to skwird but keep and forget,
//! each a command (`skwird.keep_volume`, the user's alone, and
//! `skwird.forget_volume`), which start an engine where none runs as a
//! search does (`skwird.start`): the user asked. skwird is asked what it
//! sees (`volumes` with nothing to keep or forget) as mounts change, at the
//! first look, and as the list or a drive's menu is shown (at most every
//! few seconds), never starting one; what it said last is what the menu and
//! the list answer from.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

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

/// What Skwir keeps from one run to the next: the drives asked about, by
/// their identity, and each kept drive's label. Another skwir's, on the
/// same state file, is merged in before a save (`Memory::merge`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Remembered {
    pub asked: BTreeSet<String>,
    pub labels: BTreeMap<String, String>,
    /// What a later skwir keeps that this one does not know, kept as it is.
    #[serde(flatten)]
    pub later: toml::Table,
}

impl Remembered {
    fn of(value: toml::Value) -> Option<Remembered> {
        value.try_into().ok()
    }
}

/// A drive to ask about.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Offer {
    id: String,
    label: String,
}

#[derive(Default)]
struct Memory {
    /// What a running engine that says `volumes` last answered; none where
    /// none did.
    said: Option<protocol::Volumes>,
    remembered: Remembered,
    /// The labels let go of this run (a drive forgotten, or no longer
    /// kept): a merge with another skwir's state brings none of them back.
    dropped: HashSet<String>,
    /// The drives to ask about, in the order found, none asked yet.
    pending: VecDeque<Offer>,
    /// The drive whose question stands, as far as this run knows: no
    /// answer heard for it (a question closed is never told).
    up: Option<String>,
    /// The mount points last seen, each a filesystem of files kept.
    points: Option<HashSet<PathBuf>>,
}

impl Memory {
    /// Take in what `theirs` keeps (another skwir's): what it asked, and
    /// the labels of what it kept, but those let go of here.
    fn merge(&mut self, theirs: &Remembered) {
        self.remembered.asked.extend(theirs.asked.iter().cloned());
        for (id, label) in &theirs.labels {
            if !self.dropped.contains(id) && !self.remembered.labels.contains_key(id) {
                self.remembered.labels.insert(id.clone(), label.clone());
            }
        }
        for (key, value) in &theirs.later {
            if !self.remembered.later.contains_key(key) {
                self.remembered.later.insert(key.clone(), value.clone());
            }
        }
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

/// What asks skwird: whether what it finds is offered (a mount, the first
/// look) and whether a mount was made or let go of.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Asking {
    offer: bool,
    plugged: bool,
}

impl Asking {
    fn with(self, other: Asking) -> Asking {
        Asking {
            offer: self.offer || other.offer,
            plugged: self.plugged || other.plugged,
        }
    }
}

/// Whether a check runs, and what was asked while it did.
#[derive(Default)]
struct Checking {
    running: bool,
    again: Option<Asking>,
    /// When a listing (the page, a drive's menu) last asked one.
    listed: Option<Instant>,
}

/// No engine that indexes drives answered.
struct Unanswered;

/// The drives' keeper for one run of the plugin.
pub(crate) struct Volumes {
    pub(crate) source: RecordSource,
    app: Weak<App>,
    engines: Engines,
    /// This run's: cancelled once the plugin is turned off.
    lifetime: CancelToken,
    memory: Mutex<Memory>,
    /// One saves at a time, what it writes read under it.
    saving: Mutex<()>,
    checking: Mutex<Checking>,
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
                offer: true,
                plugged: false,
            });
        }
        self.looks.fetch_add(1, Ordering::AcqRel);
    }

    /// A mount was made or let go of, by any program (`reach::follow`).
    pub(crate) fn mounts_changed(self: &Arc<Self>) {
        self.request(Asking {
            offer: true,
            plugged: true,
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

    /// What skwird sees, offered where `asking` says; asked once more a
    /// moment later where a mount just made is a drive it does not see yet.
    fn check(self: &Arc<Self>, app: &App, asking: Asking) {
        let fresh = if asking.offer {
            self.points_since()
        } else {
            Vec::new()
        };
        let answer = self.check_once(app, asking);
        let late = asking.plugged
            && answer.as_ref().is_some_and(|answer| {
                let seen: HashSet<PathBuf> = answer.seen.iter().map(|s| s.at.to_path()).collect();
                fresh.iter().any(|point| !seen.contains(point))
            });
        if late
            && self
                .lifetime
                .subscribe()
                .receiver()
                .recv_timeout(AGAIN)
                .is_err()
        {
            self.check_once(app, asking);
        }
    }

    fn check_once(self: &Arc<Self>, app: &App, asking: Asking) -> Option<protocol::Volumes> {
        let answer = self
            .with_engine(false, |client| client.volumes(&[], &[], &[]))
            .ok();
        if self.took(app, answer.clone()) {
            self.save(app);
        }
        if asking.offer
            && let Some(answer) = &answer
        {
            let found = self.offer(app, answer, asking.plugged);
            self.ask_next(app, &found);
        }
        answer
    }

    /// The mount points of files kept made since the last look at them:
    /// none at the first.
    fn points_since(&self) -> Vec<PathBuf> {
        let now: HashSet<PathBuf> = skwir_core::fs::reach::points()
            .into_iter()
            .filter(|m| !m.remote && !m.kernel && !m.on_demand)
            .map(|m| m.point)
            .collect();
        let mut memory = self.memory.lock();
        let fresh = memory
            .points
            .as_ref()
            .map_or_else(Vec::new, |before| now.difference(before).cloned().collect());
        memory.points = Some(now);
        fresh
    }

    /// `ask` of an engine that says `volumes`, started where none runs if
    /// `start` and the Index page's switch allows it; given up on once this
    /// run is.
    fn with_engine<T>(
        &self,
        start: bool,
        ask: impl FnOnce(&mut Client) -> Result<T, skwird_client::Error>,
    ) -> Result<T, Unanswered> {
        let start = start
            && self
                .app
                .upgrade()
                .is_none_or(|app| app.settings.get::<bool>(crate::kept::START).unwrap_or(true));
        let mut find = crate::find(&self.engines, start).ok_or(Unanswered)?;
        find.silence = Some(VOLUMES_SILENCE);
        if self.lifetime.is_cancelled() {
            return Err(Unanswered);
        }
        let (mut client, _stop) = crate::connect(&find, &self.lifetime, crate::HELLO)
            .map_err(|e| crate::said("connect", &e))
            .map_err(|()| Unanswered)?;
        if !says_volumes(&client) {
            return Err(Unanswered);
        }
        ask(&mut client)
            .map_err(|e| crate::said("volumes", &e))
            .map_err(|()| Unanswered)
    }

    /// Keep what skwird said, the labels of the drives it keeps with it
    /// (as mounted now, else as remembered), and say so where it changed:
    /// whether the labels changed, for the caller to save.
    fn took(&self, app: &App, answer: Option<protocol::Volumes>) -> bool {
        let (changed, relabelled) = {
            let mut memory = self.memory.lock();
            let mut relabelled = false;
            if let Some(answer) = &answer {
                let seen: HashMap<&str, &protocol::SeenVolume> = answer
                    .seen
                    .iter()
                    .filter_map(|s| Some((s.id.as_deref()?, s)))
                    .collect();
                let kept: HashSet<&str> = answer.kept.iter().map(|k| k.id.as_str()).collect();
                let Memory {
                    remembered,
                    dropped,
                    ..
                } = &mut *memory;
                let before = remembered.labels.clone();
                remembered.labels.retain(|id, _| {
                    let keep = kept.contains(id.as_str());
                    if !keep {
                        dropped.insert(id.clone());
                    }
                    keep
                });
                for id in &kept {
                    if let Some(seen) = seen.get(id) {
                        remembered.labels.insert((*id).to_owned(), label_of(seen));
                        dropped.remove(*id);
                    }
                }
                relabelled = remembered.labels != before;
            }
            let changed = memory.said != answer;
            memory.said = answer;
            (changed, relabelled)
        };
        if changed || relabelled {
            self.source.changed();
            app.wake();
        }
        relabelled
    }

    /// Note the drives `answer` offers to ask about (each seen with an
    /// identity, not kept, never asked), letting go of those it no longer
    /// offers, and take back a question about a drive gone or kept since:
    /// the identities noted now, which a mount made asks first.
    fn offer(&self, app: &App, answer: &protocol::Volumes, plugged: bool) -> Vec<String> {
        let kept: HashSet<&str> = answer.kept.iter().map(|k| k.id.as_str()).collect();
        let seen: HashMap<&str, &protocol::SeenVolume> = answer
            .seen
            .iter()
            .filter_map(|s| Some((s.id.as_deref().filter(|id| !id.is_empty())?, s)))
            .collect();
        let (found, moot) = {
            let mut memory = self.memory.lock();
            let Memory {
                remembered,
                pending,
                up,
                ..
            } = &mut *memory;
            let offers = |id: &str| !kept.contains(id) && !remembered.asked.contains(id);
            pending.retain(|offer| seen.contains_key(offer.id.as_str()) && offers(&offer.id));
            let moot = up
                .take_if(|id| !seen.contains_key(id.as_str()) || kept.contains(id.as_str()))
                .is_some();
            let waiting: HashSet<String> = pending.iter().map(|o| o.id.clone()).collect();
            let mut found = Vec::new();
            for volume in &answer.seen {
                let Some(id) = volume.id.as_deref().filter(|id| !id.is_empty()) else {
                    continue;
                };
                if offers(id) && !waiting.contains(id) && up.as_deref() != Some(id) {
                    pending.push_back(Offer {
                        id: id.to_owned(),
                        label: label_of(volume),
                    });
                    found.push(id.to_owned());
                }
            }
            (found, moot)
        };
        if moot {
            app.take_back_question(ID);
        }
        if plugged { found } else { Vec::new() }
    }

    /// Ask about the next drive waiting, where a window of the user's own is
    /// looked at: one of `plugged` first, taking the place of a question
    /// up, else the first found while none is up. Counted asked as it is
    /// asked, and kept so. On a worker: it reads and writes the state file.
    fn ask_next(&self, app: &App, plugged: &[String]) {
        if self.lifetime.is_cancelled()
            || !app.looked_at_own()
            || self.memory.lock().pending.is_empty()
        {
            return;
        }
        // Another skwir may have asked meanwhile.
        if let Some(theirs) = on_disk(app) {
            self.memory.lock().merge(&theirs);
        }
        let offer = {
            let mut memory = self.memory.lock();
            let Memory {
                remembered,
                pending,
                up,
                ..
            } = &mut *memory;
            pending.retain(|offer| !remembered.asked.contains(&offer.id));
            let plugged: HashSet<&str> = plugged.iter().map(String::as_str).collect();
            let at = pending
                .iter()
                .position(|offer| plugged.contains(offer.id.as_str()))
                .or_else(|| (up.is_none() && !pending.is_empty()).then_some(0));
            let Some(offer) = at.and_then(|at| pending.remove(at)) else {
                return;
            };
            remembered.asked.insert(offer.id.clone());
            *up = Some(offer.id.clone());
            offer
        };
        app.ask_quietly(ID, question(&offer.label), answers(&offer.id));
        self.save(app);
    }

    /// The answer to a drive's question (its buttons): kept if Yes, then
    /// the next drive waiting asked.
    fn answer(&self, app: &App, id: &str, keep: bool) -> anyhow::Result<()> {
        self.memory.lock().up.take_if(|up| up == id);
        let kept = if keep { self.keep(app, id) } else { Ok(()) };
        self.ask_next(app, &[]);
        kept
    }

    /// Have skwird index the drive `id`, its label remembered.
    fn keep(&self, app: &App, id: &str) -> anyhow::Result<()> {
        let label = self.memory.lock().label(id);
        let Ok(answer) = self.with_engine(true, |client| client.volumes(&[id], &[], &[])) else {
            anyhow::bail!(skwir_core::words!(
                skwird_volume_not_kept = "The names on { $name } are not indexed: no engine of skwird's that indexes drives answered.",
                name = Name(label),
                note = "An error, once the user said yes to indexing a drive's names: name is the drive's label. skwird is a program's name, as written."
            ));
        };
        {
            let mut memory = self.memory.lock();
            memory.remembered.labels.insert(id.to_owned(), label);
            memory.dropped.remove(id);
        }
        self.took(app, Some(answer));
        self.save(app);
        Ok(())
    }

    /// Have skwird index the drive `id` no more and remove its index.
    fn forget(&self, app: &App, id: &str) -> anyhow::Result<()> {
        let label = self.memory.lock().label(id);
        let Ok(answer) = self.with_engine(true, |client| client.volumes(&[], &[id], &[])) else {
            anyhow::bail!(skwir_core::words!(
                skwird_volume_not_forgotten = "The index of { $name } is not removed: no engine of skwird's that indexes drives answered.",
                name = Name(label),
                note = "An error, once the user asked to forget a drive's index: name is the drive's label. skwird is a program's name, as written."
            ));
        };
        {
            let mut memory = self.memory.lock();
            memory.remembered.labels.remove(id);
            memory.dropped.insert(id.to_owned());
        }
        self.took(app, Some(answer));
        self.save(app);
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
/// drives' keeper of this run.
pub(crate) fn register(app: &App, engines: &Engines) -> Arc<Volumes> {
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
        lifetime: app.lifetime().child(),
        memory: Mutex::new(Memory {
            remembered,
            ..Memory::default()
        }),
        saving: Mutex::default(),
        checking: Mutex::default(),
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
