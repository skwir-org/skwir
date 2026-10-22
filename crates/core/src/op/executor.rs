//! Runs plans in the background with progress, pause and cancellation, and
//! keeps the undo/redo journal of inverse plans.
//!
//! Plans, undos and redos are asked in an order, and run at once where
//! they touch nothing in common (ADR-079): each says what it touches (its
//! steps' `footprint`), and starts when nothing asked before it that has
//! not ended conflicts with it, when it is heavy no other heavy one on one
//! of its disks or servers runs or waits before it, and fewer than
//! [`AT_ONCE`] run. A step that cannot
//! say what it touches makes its plan run alone, after everything asked
//! before it and before everything asked after. So any two plans that
//! touch something in common run in the order asked, and the journal keeps
//! that order: undoing in its reverse puts back every state, whatever
//! order independent plans ended in. An undo takes the last entry asked
//! for, and waits for it while it runs; two quick undos take two entries.
//! A plan a step's error stopped keeps the steps it had left
//! ([`Executor::retry`]): retried, they run as a plan asked now, and what
//! they apply joins the stopped plan's entry, one action, unless a plan
//! asked since put an entry after it.
//! Each plan runs on a thread of its own, which ends with it, so an idle
//! app keeps no executor thread, and a copy that runs for minutes, or
//! waits paused for as long as the user likes, never holds a scheduler
//! worker: those draw what is on screen.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant, SystemTime};

use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
use parking_lot::{Condvar, Mutex};
use serde::Serialize;

use super::context::{Meter, Pause};
use super::footprint::Footprint;
use super::{Cancelled, Change, Operation, Plan, PlanId, StepContext, StepStatus};
use crate::cancel::CancelToken;
use crate::registry::Contributions;
use crate::scheduler::{Priority, Scheduler};

/// Progress of one step.
#[derive(Clone, Debug, Serialize)]
pub struct Progress {
    pub plan: PlanId,
    pub step: usize,
    pub total: usize,
    pub change: Change,
    /// The name the user knows what the change acts on by, where its path
    /// says another: an item of a trash, by the name it had where it came
    /// from, not the trash's (`a.txt.2`, `$R…`). Read for the first step and
    /// for one that fails, what a toast names.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub named: Option<String>,
    pub status: StepStatus,
    pub error: Option<String>,
}

/// What `change` acts on, by the name the user knows it by where its path
/// says another (`Progress::named`). Reads its trash's record, before the
/// step takes it away; nothing for a file that is no item of a trash,
/// which its path alone says.
fn named(change: &Change) -> Option<String> {
    let name = super::builtin::trashed_name(change.subject()?)?;
    Some(name.to_string_lossy().into_owned())
}

/// Executor events, consumed by the UI (and tests) without blocking.
#[derive(Clone, Debug, Serialize)]
pub enum ExecEvent {
    Started {
        plan: PlanId,
        title: String,
        total: usize,
        /// Whether this run applies a plan or takes one back (or again).
        run: RunKind,
        /// What the user asked for, where the steps are not its items
        /// (`Plan::items`).
        items: Option<super::Items>,
    },
    Step(Progress),
    /// The plan waits where it would answer Cancel, until resumed
    /// (`Executor::pause`).
    Paused {
        plan: PlanId,
    },
    Resumed {
        plan: PlanId,
    },
    Finished {
        plan: PlanId,
        applied: usize,
        skipped: usize,
        error: Option<String>,
        cancelled: bool,
        undoable: bool,
    },
}

/// What became of a plan proposed for review (`App::propose_awaiting`):
/// applied when the user said so, with what its run did, or dismissed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "verdict")]
pub enum Verdict {
    Applied {
        /// Steps applied, and steps found already done.
        applied: usize,
        skipped: usize,
        /// What stopped it, or made it apply nothing (a conflict).
        error: Option<String>,
        cancelled: bool,
    },
    Dismissed,
}

/// What a run does with the journal, so a surface can word its result
/// ("Undid …", a Redo button) without reading it from the title.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunKind {
    Apply,
    Undo,
    Redo,
}

impl From<Direction> for RunKind {
    fn from(direction: Direction) -> Self {
        match direction {
            Direction::Undo => RunKind::Undo,
            Direction::Redo => RunKind::Redo,
        }
    }
}

/// How far a running plan is. Its bytes are those its steps move (copies,
/// cross-device moves), `total` growing as each such step starts: a plan
/// of several steps knows its bytes only as it goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Transfer {
    pub done: u64,
    pub total: u64,
    /// Steps ended (applied, found done, or failed).
    pub ended: usize,
    /// The bytes of the step under way: moved, and announced.
    pub step_done: u64,
    pub step_total: u64,
}

/// Told, once per finished run of a plan (undo and redo runs included),
/// the changes that hold afterward: the steps applied and the steps found
/// already done, in plan order. A plugin that keeps paths of its own (the
/// shelf, a clipboard) follows moves, renames and removals with it instead
/// of stat-ing what it holds.
///
/// It runs on the plan's thread after the `Finished` event, before any plan
/// that touches what it did starts; plans that share nothing with it may
/// run, and tell their own, meanwhile (ADR-079): keep it brief (update a
/// map, wake), and never wait for the executor.
pub type ChangeObserver = Arc<dyn Fn(&[Change]) + Send + Sync>;

/// One journaled change and the plan that takes it back.
#[derive(Debug)]
pub struct JournalEntry {
    /// The title of the plan as the user knows it.
    pub title: String,
    pub applied_at: SystemTime,
    /// On the undo stack, the inverse of what was applied; on the redo
    /// stack, the inverse of the undo. Built from what really happened, so
    /// its ids are current: a cross-device move gets a new inode each way.
    /// `None` when nothing applied had an inverse.
    pub plan: Option<Plan>,
    /// Inverses from an earlier attempt at `plan` that stopped partway.
    /// Retrying skips those steps as already done, so their inverses are
    /// kept here to become part of the entry on the other stack.
    partial: Vec<Box<dyn Operation>>,
    /// When the plan it holds was asked for, in the order asked: its place
    /// in the history.
    seq: u64,
    /// Its place on its stack: on the undo stack its own `seq`, on the
    /// redo stack when the undo that put it there was asked.
    stacked: u64,
}

impl JournalEntry {
    /// What it holds, as the journal's bound counts it: its steps' bytes
    /// (`Operation::held`), and each step as a few hundred for its paths.
    fn weight(&self) -> usize {
        const STEP: usize = 256;
        self.plan
            .iter()
            .flat_map(|plan| &plan.steps)
            .chain(&self.partial)
            .map(|step| STEP + step.held())
            .sum()
    }
}

/// Result of applying a plan synchronously.
#[derive(Debug)]
pub struct ExecResult {
    pub applied: usize,
    pub skipped: usize,
    pub error: Option<String>,
    pub cancelled: bool,
    /// Whether an inverse was journaled.
    pub undoable: bool,
    /// False if some applied step had no inverse.
    pub complete: bool,
}

/// The undo and redo stacks, each in its entries' `stacked` order.
#[derive(Default)]
struct Journal {
    undo: Vec<JournalEntry>,
    redo: Vec<JournalEntry>,
    /// When the latest plan was asked for: an undo asked before it puts
    /// nothing on the redo stack (ADR-079, 10).
    redo_floor: u64,
}

/// The actions the undo journal keeps at most, the latest always: past it,
/// the oldest go first.
pub const JOURNAL_ENTRIES: usize = 200;
/// What the journal's entries may hold together (`JournalEntry::weight`):
/// tagging ten thousand photos with XMP sidecars kept every sidecar's text
/// twice, some 600 MB, for as long as skwir ran.
pub const JOURNAL_BYTES: usize = 64 << 20;

impl Journal {
    fn stack(&mut self, direction: Direction) -> &mut Vec<JournalEntry> {
        match direction {
            Direction::Undo => &mut self.undo,
            Direction::Redo => &mut self.redo,
        }
    }

    /// Put `entry` on the stack `direction` takes from, in its place.
    fn put(&mut self, direction: Direction, entry: JournalEntry) {
        let stack = self.stack(direction);
        let at = stack.partition_point(|e| e.stacked < entry.stacked);
        stack.insert(at, entry);
    }

    /// Take the entry at `stacked` off the stack `direction` takes from.
    fn take(&mut self, direction: Direction, stacked: u64) -> Option<JournalEntry> {
        let stack = self.stack(direction);
        let at = stack.iter().position(|e| e.stacked == stacked)?;
        Some(stack.remove(at))
    }

    /// Let go of the oldest entries of each stack past `JOURNAL_ENTRIES`
    /// or `JOURNAL_BYTES`, keeping the latest whatever it holds.
    fn bound(&mut self) {
        for stack in [&mut self.undo, &mut self.redo] {
            let mut weight: usize = stack.iter().map(JournalEntry::weight).sum();
            let mut drop = 0;
            while stack.len() - drop > 1
                && (stack.len() - drop > JOURNAL_ENTRIES || weight > JOURNAL_BYTES)
            {
                weight -= stack[drop].weight();
                drop += 1;
            }
            stack.drain(..drop);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Direction {
    Undo,
    Redo,
}

enum Work {
    /// `whole`: dry-run every step first and apply nothing if one conflicts.
    Execute {
        plan: Arc<Plan>,
        whole: bool,
        /// A retry: what it continues.
        continues: Option<Box<Continues>>,
    },
    Replay(Direction),
}

/// What a retry continues (`Executor::retry`): the action that stopped,
/// by where its entry is in the history, and what it left of a large
/// file cut short, to go on from.
struct Continues {
    seq: u64,
    partial: Option<super::staging::Partial>,
}

/// A plan a step's error stopped, what Retry runs of it.
struct Stopped {
    /// The run that stopped, as its events named it.
    plan: PlanId,
    /// Where its entry is, or would be, in the history.
    seq: u64,
    /// Its steps from the one that failed on, unfolded and settled.
    rest: Plan,
    /// What it left of a large file cut short, kept until it is retried
    /// or let go.
    partial: Option<super::staging::Partial>,
}

/// Where a step's error stopped a plan (`Executor::apply_plan`): its index,
/// and what a copy it cut short left.
struct Stop {
    at: usize,
    partial: Option<super::staging::Partial>,
}

/// The stopped plans kept for Retry, the latest: older ones are let go.
const STOPPED_KEPT: usize = 8;

impl Work {
    /// The stack its end may put an entry on: an applied plan's or a
    /// redo's inverse on the undo stack, an undo's on the redo stack.
    fn adds(&self) -> Direction {
        match self {
            Work::Execute { .. } | Work::Replay(Direction::Redo) => Direction::Undo,
            Work::Replay(Direction::Undo) => Direction::Redo,
        }
    }
}

struct Job {
    /// When it was asked for, in the order asked.
    seq: u64,
    work: Work,
    cancel: CancelToken,
    reply: Option<Sender<Option<ExecResult>>>,
    /// What it touches, once worked out (`Executor::dispatch`).
    footprint: Option<Arc<Footprint>>,
    /// An undo's or a redo's entry (its `stacked`), once known.
    target: Option<u64>,
}

/// A job started, until it ends.
struct Begun {
    seq: u64,
    /// A plan's title, which Undo names while it runs.
    title: Option<String>,
    /// What it is called, an undo's or a redo's too (`Executor::waiting`).
    named: String,
    footprint: Arc<Footprint>,
    adds: Direction,
    /// Its entry, an undo's or a redo's: no other takes it.
    target: Option<(Direction, u64)>,
    cancel: CancelToken,
}

/// What waits and what runs (module documentation).
#[derive(Default)]
struct Lane {
    /// Asked and not started, in the order asked.
    queue: VecDeque<Job>,
    /// Started and not ended.
    started: Vec<Begun>,
    /// Wound down (`Executor::wind_down`): what is queued now is cancelled
    /// as it arrives.
    closed: bool,
    /// The number the last job asked took.
    next: u64,
    /// A footprint is being worked out: by one caller at a time.
    measuring: bool,
    /// The entries undos and redos waiting to start will take.
    targeted: Vec<(Direction, u64)>,
}

/// Plans run at once at most (ADR-079).
pub const AT_ONCE: usize = 4;

/// An action waiting its turn, and what for (ADR-079, 12).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Waiting {
    /// Its title: a plan's, or "undo …" and "redo …".
    pub named: String,
    pub waits_for: WaitsFor,
}

/// What an action waiting its turn waits for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WaitsFor {
    /// An action asked before it that touches what it does, by its title.
    Action(String),
    /// A heavy action on one of its disks.
    Disk,
    /// A heavy action on its server.
    Server,
    /// One of the [`AT_ONCE`] running to end.
    Room,
    /// It cannot say what it touches: every action asked before it.
    Everything,
    /// Nothing more: what it touches is being worked out, or it starts.
    Starting,
}

/// How often steps' events may wake the window: as often as bytes do.
const STEP_WAKE: Duration = Duration::from_millis(100);

/// The executor. One per app.
pub struct Executor {
    /// Itself, for the threads it starts.
    me: std::sync::Weak<Executor>,
    scheduler: Scheduler,
    journal: Mutex<Journal>,
    lane: Mutex<Lane>,
    /// Told when the lane has drained.
    idle: Condvar,
    tx: Sender<ExecEvent>,
    rx: Receiver<ExecEvent>,
    waker: Mutex<Option<crate::location::Waker>>,
    /// When a step's event last woke the window (`emit_step`).
    step_woken: Mutex<Option<Instant>>,
    /// Plans being applied, so a surface can pause or cancel one by id and
    /// read its byte progress.
    running: Mutex<Vec<Active>>,
    observers: Contributions<ChangeObserver>,
    companions: Mutex<Arc<crate::companions::Companions>>,
    /// The transfers plugins contributed: a batch of copies to or from a
    /// remote is made by one that serves it (ADR-078).
    transfers: Mutex<Arc<super::transfer::Transfers>>,
    /// Where the folders copies are under way in are noted.
    under_way: Mutex<Option<super::staging::UnderWay>>,
    /// Plans someone waits for (`watch`): answered when their run
    /// finishes, or when they are dismissed.
    watched: Mutex<std::collections::HashMap<PlanId, Sender<Verdict>>>,
    /// What a volume has free (`fs::volume::free`; a test's own).
    free: Mutex<Free>,
    /// Asks the user whether a plan short of room goes on anyway
    /// (`set_room_asker`): its answer comes on the channel. None asks:
    /// such a plan is refused.
    room_asker: Mutex<Option<RoomAsker>>,
    /// Told the folders a plan's transfers changed beside the mounts
    /// showing them (`set_changed_beside`).
    changed_beside: Mutex<Option<ChangedBeside>>,
    /// Asks the user for a step's secret (`StepContext::ask_secret`).
    secret_asker: Mutex<Option<SecretAsker>>,
    /// Asks the user what becomes of the names a plan finds taken
    /// (`set_clash_asker`). None asks: such a plan is refused.
    clash_asker: Mutex<Option<ClashAsker>>,
    /// The plans a step's error stopped, oldest first, for Retry.
    stopped: Mutex<VecDeque<Stopped>>,
}

/// What a volume has free, by a folder on it.
pub type Free = Arc<dyn Fn(&Path) -> Option<u64> + Send + Sync>;

/// Told the folders a plan's transfers wrote in or removed from beside the
/// mounts showing them (ADR-078): a tool's copies onto a server, an
/// original it removed there, which no watch of this machine's sees.
pub type ChangedBeside = Arc<dyn Fn(&[PathBuf]) + Send + Sync>;

/// Asks whether the plan titled so, short of room (`room::Short`), goes
/// on anyway; answers on the channel, true to go on.
pub type RoomAsker =
    Arc<dyn Fn(&str, &super::room::Short) -> crossbeam_channel::Receiver<bool> + Send + Sync>;

/// Asks the user for what a step needs that only they know (an archive's
/// password), titled with the question and its button's verb, and waits
/// for the answer until the token given is cancelled.
pub type SecretAsker = Arc<
    dyn Fn(&str, &str, &crate::cancel::CancelToken) -> Result<String, super::Unanswered>
        + Send
        + Sync,
>;

/// Asks what becomes of the names the plan titled so finds taken
/// (ADR-054), all at once: the answer comes on the channel, `None` to
/// cancel. `None` when no one can be asked: the plan applies nothing.
pub type ClashAsker = Arc<
    dyn Fn(
            &str,
            &[super::clash::Clash],
        ) -> Option<crossbeam_channel::Receiver<Option<super::clash::Answer>>>
        + Send
        + Sync,
>;

/// What becomes of a plan with names taken where it copies or moves
/// (`Executor::settle`).
enum Settling {
    /// It goes on, these steps in place of its own where they differ.
    Steps(Option<super::clash::Settled>),
    /// A name taken, and no one to ask: what it says.
    Refused(String),
    /// The user cancelled.
    Cancelled,
}

/// A plan being applied: what stops it, what holds it, what it moved.
struct Active {
    plan: PlanId,
    cancel: CancelToken,
    pause: Arc<Pause>,
    meter: Arc<Meter>,
}

/// Removes a plan from `running` however `apply_plan` leaves.
struct Running<'a>(&'a Executor, PlanId);

impl Drop for Running<'_> {
    fn drop(&mut self) {
        self.0.running.lock().retain(|active| active.plan != self.1);
    }
}

/// Ends a started job however it leaves, a panic included: what waited
/// for it may start.
struct Ended<'a>(&'a Executor, u64);

impl Drop for Ended<'_> {
    fn drop(&mut self) {
        {
            let mut lane = self.0.lane.lock();
            if let Some(at) = lane.started.iter().position(|b| b.seq == self.1) {
                let begun = lane.started.remove(at);
                if let Some(target) = begun.target {
                    lane.targeted.retain(|t| *t != target);
                }
            }
            if lane.started.is_empty() && lane.queue.is_empty() {
                self.0.idle.notify_all();
            }
        }
        self.0.dispatch();
    }
}

impl Executor {
    pub fn new(scheduler: Scheduler) -> Arc<Self> {
        let (tx, rx) = unbounded();
        Arc::new_cyclic(|me| Executor {
            me: me.clone(),
            scheduler,
            journal: Mutex::new(Journal::default()),
            lane: Mutex::new(Lane::default()),
            idle: Condvar::new(),
            tx,
            rx,
            waker: Mutex::new(None),
            step_woken: Mutex::new(None),
            running: Mutex::new(Vec::new()),
            observers: Contributions::default(),
            companions: Mutex::new(Arc::default()),
            transfers: Mutex::new(Arc::default()),
            under_way: Mutex::new(None),
            watched: Mutex::new(std::collections::HashMap::new()),
            free: Mutex::new(Arc::new(crate::fs::volume::free)),
            room_asker: Mutex::new(None),
            changed_beside: Mutex::new(None),
            secret_asker: Mutex::new(None),
            clash_asker: Mutex::new(None),
            stopped: Mutex::default(),
        })
    }

    /// Ask the user, through `asker`, whether a plan short of room where it
    /// writes goes on anyway (ADR-049): the window's question.
    pub fn set_room_asker(&self, asker: Option<RoomAsker>) {
        *self.room_asker.lock() = asker;
    }

    /// Tell `told` the folders a plan's transfers changed beside the mounts
    /// showing them, at its end: the window reads them again.
    pub fn set_changed_beside(&self, told: Option<ChangedBeside>) {
        *self.changed_beside.lock() = told;
    }

    /// Ask the user, through `asker`, for what a step needs that only they
    /// know (an archive's password): the window's prompt.
    pub fn set_secret_asker(&self, asker: Option<SecretAsker>) {
        *self.secret_asker.lock() = asker;
    }

    /// Ask the user, through `asker`, what becomes of the names a plan
    /// asked for as one action finds taken (ADR-054): the window's dialog.
    pub fn set_clash_asker(&self, asker: Option<ClashAsker>) {
        *self.clash_asker.lock() = asker;
    }

    /// What `plan`, asked for as one action, does with the names it finds
    /// taken (ADR-054): as it says beforehand, else as the user answers;
    /// what is never asked, as ADR-054 says. Waits for the answer, Cancel
    /// answering none.
    fn settle(
        &self,
        plan: &Plan,
        companions: &Arc<crate::companions::Companions>,
        cancel: &CancelToken,
    ) -> Settling {
        use super::clash::{Answer, settle};
        let ctx = || dry_context(&plan.steps, companions, cancel);
        let said = plan.on_conflict.map(Answer::all);
        let first = settle(
            &plan.steps,
            said.as_ref().unwrap_or(&Answer::default()),
            &ctx(),
        );
        let settled = if first.clashes.is_empty() || said.is_some() {
            first
        } else {
            let asker = self.clash_asker.lock().clone();
            let Some(answer) = asker.and_then(|asker| asker(&plan.title, &first.clashes)) else {
                return Settling::Refused(first.clashes[0].said());
            };
            let answer = loop {
                match answer.recv_timeout(Duration::from_millis(100)) {
                    Ok(Some(answer)) => break answer,
                    Ok(None) | Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                        return Settling::Cancelled;
                    }
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) if cancel.is_cancelled() => {
                        return Settling::Cancelled;
                    }
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                }
            };
            settle(&plan.steps, &answer, &ctx())
        };
        Settling::Steps(settled.changed.then_some(settled))
    }

    /// What a volume has free, as `free` says: for a test.
    pub fn set_free(&self, free: Free) {
        *self.free.lock() = free;
    }

    /// Why `plan`, asked for as one action, does not go on for want of
    /// room: `None` when it has room, or the user said to go on. Waits for
    /// the answer, Cancel answering no.
    fn short_of_room(&self, plan: &Plan, cancel: &CancelToken) -> Option<String> {
        let free = Arc::clone(&self.free.lock());
        let ctx = dry_context(&plan.steps, &Arc::default(), cancel);
        let short = super::room::short(&plan.steps, &ctx, &*free, cancel)?;
        let asker = self.room_asker.lock().clone();
        let Some(asker) = asker else {
            return Some(short.said());
        };
        let answer = asker(&plan.title, &short);
        loop {
            match answer.recv_timeout(Duration::from_millis(100)) {
                Ok(true) => return None,
                Ok(false) | Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                    return Some(short.said());
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) if cancel.is_cancelled() => {
                    return Some(short.said());
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            }
        }
    }

    /// Answer `reply` once the plan `id` has run: what it did. Its first
    /// run finished answers, whoever started it.
    pub fn watch(&self, id: PlanId, reply: Sender<Verdict>) {
        self.watched.lock().insert(id, reply);
    }

    /// Answer the one waiting for the plan `id`, if anyone is, that it was
    /// dismissed. Whether someone was.
    pub fn dismiss(&self, id: PlanId) -> bool {
        let reply = self.watched.lock().remove(&id);
        reply.is_some_and(|reply| reply.send(Verdict::Dismissed).is_ok())
    }

    /// The companion rules every step carries files beside a file by
    /// (ADR-028).
    pub fn set_companions(&self, companions: Arc<crate::companions::Companions>) {
        *self.companions.lock() = companions;
    }

    /// The transfers a batch of copies to or from a remote may be made by
    /// (ADR-078).
    pub fn set_transfers(&self, transfers: Arc<super::transfer::Transfers>) {
        *self.transfers.lock() = transfers;
    }

    /// Note the folders copies are under way in there, for the next run to
    /// sweep should this one end mid-copy (`staging::UnderWay`).
    pub fn set_under_way(&self, under_way: super::staging::UnderWay) {
        *self.under_way.lock() = Some(under_way);
    }

    /// Where the folders copies are under way in are noted, if anywhere.
    pub fn under_way(&self) -> Option<super::staging::UnderWay> {
        self.under_way.lock().clone()
    }

    /// Wake the UI after each event.
    pub fn set_waker(&self, waker: Option<crate::location::Waker>) {
        *self.waker.lock() = waker;
    }

    fn emit(&self, ev: ExecEvent) {
        if let ExecEvent::Finished {
            plan,
            applied,
            skipped,
            error,
            cancelled,
            ..
        } = &ev
            && let Some(reply) = self.watched.lock().remove(plan)
        {
            let _ = reply.send(Verdict::Applied {
                applied: *applied,
                skipped: *skipped,
                error: error.clone(),
                cancelled: *cancelled,
            });
        }
        let _ = self.tx.send(ev);
        self.wake();
    }

    fn wake(&self) {
        if let Some(w) = self.waker.lock().as_ref() {
            w();
        }
    }

    /// A step's event, sent at once, the window woken for it at most
    /// every `STEP_WAKE`: a plan of many small steps (a hundred thousand
    /// files put in the trash) woke it, and it drew, for each one. The
    /// events not woken for are read at the next wake, the plan's end's
    /// at the latest.
    fn emit_step(&self, ev: ExecEvent) {
        let _ = self.tx.send(ev);
        let now = Instant::now();
        let mut last = self.step_woken.lock();
        if last.is_none_or(|t| now.duration_since(t) >= STEP_WAKE) {
            *last = Some(now);
            drop(last);
            self.wake();
        }
    }

    /// Stop a plan after its current file, or before it starts if it is
    /// still queued; a paused one stops where it waits. Steps already
    /// applied stay applied and are journaled, so the partial result can be
    /// undone.
    pub fn cancel(&self, plan: PlanId) -> bool {
        if let Some(active) = self.running.lock().iter().find(|a| a.plan == plan) {
            active.cancel.cancel();
            return true;
        }
        self.lane
            .lock()
            .queue
            .iter()
            .find(|job| matches!(&job.work, Work::Execute { plan: p, .. } if p.id == plan))
            .map(|job| job.cancel.cancel())
            .is_some()
    }

    /// Hold a running plan where it would next answer Cancel: between its
    /// steps, or inside one between files and chunks of a file, keeping
    /// what it has open, what it touches and its heavy slot (ADR-079): the
    /// plans that wait for it wait on. Whether the plan runs; `Paused` is
    /// told when it was running freely.
    pub fn pause(&self, plan: PlanId) -> bool {
        self.hold(plan, true)
    }

    /// Let a paused plan go on from where it waits. Whether the plan runs;
    /// `Resumed` is told when it was paused.
    pub fn resume(&self, plan: PlanId) -> bool {
        self.hold(plan, false)
    }

    fn hold(&self, plan: PlanId, paused: bool) -> bool {
        let (held, released) = {
            let running = self.running.lock();
            let Some(active) = running.iter().find(|a| a.plan == plan) else {
                return false;
            };
            if paused {
                (active.pause.pause(), None)
            } else {
                (false, active.pause.resume())
            }
        };
        if held {
            self.emit(ExecEvent::Paused { plan });
        }
        // Resumed is told before the plan goes on: its next step, told by
        // the plans' thread once the hold is dropped, comes after.
        if let Some(hold) = released {
            self.emit(ExecEvent::Resumed { plan });
            drop(hold);
        }
        true
    }

    /// Whether a running plan is paused.
    pub fn is_paused(&self, plan: PlanId) -> bool {
        self.running
            .lock()
            .iter()
            .any(|a| a.plan == plan && a.pause.is_paused())
    }

    /// Plans, undos and redos asked and not started: waiting for what they
    /// meet, or for a heavy one, to end (ADR-079).
    pub fn queued(&self) -> usize {
        self.lane.lock().queue.len()
    }

    /// Cancel every plan, undo and redo, whether it runs, waits paused or
    /// waits its turn, and take no more; then wait up to `timeout` for all
    /// to end. A cancelled step takes back its own partial work as
    /// it stops (a copy removes its staging folder), so a process that
    /// ends after this leaves no half-made copy behind. Whether all
    /// drained in time. It blocks: call it off the window's thread.
    pub fn wind_down(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        {
            let mut lane = self.lane.lock();
            lane.closed = true;
            for token in lane
                .queue
                .iter()
                .map(|job| &job.cancel)
                .chain(lane.started.iter().map(|b| &b.cancel))
            {
                token.cancel();
            }
        }
        // Cancelled, what waited starts at once and ends.
        self.dispatch();
        let mut lane = self.lane.lock();
        while !lane.started.is_empty() || !lane.queue.is_empty() {
            if self.idle.wait_until(&mut lane, deadline).timed_out() {
                return lane.started.is_empty() && lane.queue.is_empty();
            }
        }
        true
    }

    /// How far a running plan is, in steps and bytes, for a progress bar
    /// finer than its steps. `None` once the plan finished.
    pub fn transfer(&self, plan: PlanId) -> Option<Transfer> {
        self.running
            .lock()
            .iter()
            .find(|a| a.plan == plan)
            .map(|a| {
                let steps = *a.meter.steps.lock();
                let done = a.meter.done.load(Ordering::Relaxed);
                let total = a.meter.total.load(Ordering::Relaxed);
                Transfer {
                    done,
                    total,
                    ended: steps.ended,
                    step_done: done.saturating_sub(steps.done),
                    step_total: total.saturating_sub(steps.total),
                }
            })
    }

    /// Tell `observer` what each finished plan changed, until `owner`'s
    /// observers are removed (`App::unregister_owner` does).
    pub fn observe(&self, owner: &str, observer: ChangeObserver) {
        self.observers.add(owner, observer);
    }

    pub fn remove_observers(&self, owner: &str) {
        self.observers.remove_owner(owner);
    }

    /// Progress and completion events.
    pub fn events(&self) -> &Receiver<ExecEvent> {
        &self.rx
    }

    /// Queue `plan`; journal the inverse when it finishes.
    pub fn execute(self: &Arc<Self>, plan: Plan, cancel: CancelToken) {
        self.submit(
            Work::Execute {
                plan: Arc::new(plan),
                whole: false,
                continues: None,
            },
            cancel,
        );
    }

    /// Like [`Executor::execute`], but all or nothing: when its turn comes,
    /// every step is checked first and a conflict anywhere applies nothing
    /// and finishes with that conflict as the error. For a plan the user
    /// asked for as one action, so it never stops halfway on a conflict
    /// that was there from the start. The checks stat the filesystem, so
    /// they run on the plan's thread, never on the caller's.
    pub fn execute_whole(self: &Arc<Self>, plan: Plan, cancel: CancelToken) {
        self.submit(
            Work::Execute {
                plan: Arc::new(plan),
                whole: true,
                continues: None,
            },
            cancel,
        );
    }

    /// Apply `plan` when its turn comes, waiting for it, and journal it.
    /// Returns what happened. Never call it from inside an operation.
    pub fn execute_blocking(&self, plan: &Plan, cancel: &CancelToken) -> ExecResult {
        self.submit_blocking(
            Work::Execute {
                plan: Arc::new(plan.clone()),
                whole: false,
                continues: None,
            },
            cancel,
        )
        .expect("an executed plan has a result")
    }

    /// Whether the plan `plan`, which a step's error stopped, can be
    /// retried ([`Executor::retry`]).
    pub fn can_retry(&self, plan: PlanId) -> bool {
        self.stopped.lock().iter().any(|s| s.plan == plan)
    }

    /// The title of the plan `plan`, which a step's error stopped.
    pub fn stopped_title(&self, plan: PlanId) -> Option<String> {
        let stopped = self.stopped.lock();
        let found = stopped.iter().find(|s| s.plan == plan)?;
        Some(found.rest.title.clone())
    }

    /// The plan a step's error stopped last, which Retry takes when told
    /// no other.
    pub fn last_stopped(&self) -> Option<PlanId> {
        self.stopped.lock().back().map(|s| s.plan)
    }

    /// Run again what the plan `plan` had left when a step's error stopped
    /// it, as a plan asked now, checked whole: what it applies joins that
    /// plan's entry in the history (module documentation). False when
    /// there is no such plan (retried already, or let go).
    pub fn retry(self: &Arc<Self>, plan: PlanId, cancel: CancelToken) -> bool {
        let Some(work) = self.retrying(plan) else {
            return false;
        };
        self.submit(work, cancel);
        true
    }

    /// Retry when its turn comes, waiting for it. `None` when there is no
    /// such plan.
    pub fn retry_blocking(&self, plan: PlanId, cancel: &CancelToken) -> Option<ExecResult> {
        let work = self.retrying(plan)?;
        self.submit_blocking(work, cancel)
    }

    /// The work that retries `plan`, taken out of the stopped plans.
    fn retrying(&self, plan: PlanId) -> Option<Work> {
        let mut stopped = self.stopped.lock();
        let at = stopped.iter().position(|s| s.plan == plan)?;
        let Stopped {
            seq, rest, partial, ..
        } = stopped.remove(at)?;
        Some(Work::Execute {
            plan: Arc::new(rest),
            whole: true,
            continues: Some(Box::new(Continues { seq, partial })),
        })
    }

    /// Undo the last plan asked (ADR-079): once it ends where it runs,
    /// beside what it shares nothing with. Returns false if there is
    /// nothing to undo and nothing that could add to the journal is queued
    /// or running.
    pub fn undo(self: &Arc<Self>, cancel: CancelToken) -> bool {
        let available = self.is_busy() || self.can_undo();
        if available {
            self.submit(Work::Replay(Direction::Undo), cancel);
        }
        available
    }

    /// Undo when its turn comes (module documentation), waiting for it.
    /// Returns `None` if nothing to undo.
    pub fn undo_blocking(&self, cancel: &CancelToken) -> Option<ExecResult> {
        self.submit_blocking(Work::Replay(Direction::Undo), cancel)
    }

    /// Redo the most recently undone plan when its turn comes (module
    /// documentation).
    pub fn redo(self: &Arc<Self>, cancel: CancelToken) -> bool {
        let available = self.is_busy() || self.can_redo();
        if available {
            self.submit(Work::Replay(Direction::Redo), cancel);
        }
        available
    }

    /// Redo when its turn comes, waiting for it.
    pub fn redo_blocking(&self, cancel: &CancelToken) -> Option<ExecResult> {
        self.submit_blocking(Work::Replay(Direction::Redo), cancel)
    }

    pub fn can_undo(&self) -> bool {
        !self.journal.lock().undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.journal.lock().redo.is_empty()
    }

    /// The title of what Undo would take back, without copying the stack.
    pub fn last_undo_title(&self) -> Option<String> {
        self.journal.lock().undo.last().map(|e| e.title.clone())
    }

    /// The title of what Redo would apply again.
    pub fn last_redo_title(&self) -> Option<String> {
        self.journal.lock().redo.last().map(|e| e.title.clone())
    }

    /// What Undo takes now (ADR-079, 9): the latest plan asked that may be
    /// in the history, by its title, and whether it has not ended (Undo
    /// waits for it then); `None` with nothing to undo.
    pub fn undo_target(&self) -> Option<(String, bool)> {
        let lane = self.lane.lock();
        let pending = lane
            .queue
            .iter()
            .filter_map(|job| match &job.work {
                Work::Execute { plan, .. } => Some((job.seq, plan.title.clone())),
                Work::Replay(_) => None,
            })
            .chain(
                lane.started
                    .iter()
                    .filter_map(|b| b.title.clone().map(|title| (b.seq, title))),
            )
            .max_by_key(|(seq, _)| *seq);
        let entry = self
            .journal
            .lock()
            .undo
            .last()
            .map(|e| (e.stacked, e.title.clone()));
        match (pending, entry) {
            (Some((seq, title)), entry) if entry.as_ref().is_none_or(|(at, _)| seq > *at) => {
                Some((title, true))
            }
            (_, entry) => entry.map(|(_, title)| (title, false)),
        }
    }

    /// Titles of the undo stack, most recent last.
    pub fn undo_titles(&self) -> Vec<String> {
        self.journal
            .lock()
            .undo
            .iter()
            .map(|e| e.title.clone())
            .collect()
    }

    /// The actions waiting their turn, in the order asked, each with what
    /// it waits for (ADR-079, 12), as `admit` would find it now.
    pub fn waiting(&self) -> Vec<Waiting> {
        let lane = self.lane.lock();
        let mut out = Vec::new();
        for (i, job) in lane.queue.iter().enumerate() {
            let waits_for = match job.footprint.as_deref() {
                _ if job.cancel.is_cancelled() => WaitsFor::Starting,
                None => match &job.work {
                    // An undo or a redo waits for the plan asked before it
                    // that may put its entry there.
                    Work::Replay(_) => lane
                        .queue
                        .iter()
                        .take(i)
                        .map(|j| (j.seq, self.named(j)))
                        .chain(lane.started.iter().map(|b| (b.seq, b.named.clone())))
                        .max_by_key(|(seq, _)| *seq)
                        .map_or(WaitsFor::Starting, |(_, named)| WaitsFor::Action(named)),
                    Work::Execute { .. } => WaitsFor::Starting,
                },
                Some(footprint) if footprint.is_every() => WaitsFor::Everything,
                Some(footprint) => {
                    // The earliest asked it meets, running or waiting.
                    let met = lane
                        .started
                        .iter()
                        .map(|b| (b.seq, Some(&*b.footprint), b.named.clone()))
                        .chain(
                            lane.queue
                                .iter()
                                .take(i)
                                .map(|j| (j.seq, j.footprint.as_deref(), self.named(j))),
                        )
                        .filter(|(_, other, _)| other.is_none_or(|o| o.conflicts(footprint)))
                        .min_by_key(|(seq, _, _)| *seq);
                    let heavy = footprint.is_heavy().then(|| {
                        lane.started
                            .iter()
                            .map(|b| &*b.footprint)
                            .chain(
                                lane.queue
                                    .iter()
                                    .take(i)
                                    .filter_map(|j| j.footprint.as_deref()),
                            )
                            .find(|o| o.is_heavy() && o.shares_channel(footprint))
                    });
                    match (met, heavy.flatten()) {
                        (Some((_, _, named)), _) => WaitsFor::Action(named),
                        (None, Some(other)) if other.shares_a_server(footprint) => WaitsFor::Server,
                        (None, Some(_)) => WaitsFor::Disk,
                        (None, None) if lane.started.len() >= AT_ONCE => WaitsFor::Room,
                        (None, None) => WaitsFor::Starting,
                    }
                }
            };
            out.push(Waiting {
                named: self.named(job),
                waits_for,
            });
        }
        out
    }

    /// What `job` is called: its plan's title, or "undo …" and "redo …"
    /// with its entry's, once known.
    fn named(&self, job: &Job) -> String {
        let direction = match &job.work {
            Work::Execute { plan, .. } => return plan.title.clone(),
            Work::Replay(direction) => *direction,
        };
        let verb = match direction {
            Direction::Undo => "undo",
            Direction::Redo => "redo",
        };
        let entry = job.target.and_then(|stacked| {
            let mut journal = self.journal.lock();
            let stack = journal.stack(direction);
            let entry = stack.iter().find(|e| e.stacked == stacked)?;
            Some(entry.title.clone())
        });
        entry.map_or_else(|| verb.to_owned(), |title| format!("{verb} {title}"))
    }

    /// Whether a plan, an undo or a redo runs or waits its turn.
    pub fn is_busy(&self) -> bool {
        let lane = self.lane.lock();
        !lane.started.is_empty() || !lane.queue.is_empty()
    }

    fn submit(&self, work: Work, cancel: CancelToken) {
        self.enqueue(work, cancel, None);
        // Behind what it waits for, whoever shows the plans says it waits.
        self.wake();
        // Working out what it touches may read (the mount table, whether
        // what is deleted is a folder): never on the caller's thread, the
        // window's.
        let Some(this) = self.me.upgrade() else {
            return;
        };
        let dispatch = {
            let this = Arc::clone(&this);
            move || this.dispatch()
        };
        let thread = std::thread::Builder::new()
            .name("skwir-plans".into())
            .spawn(dispatch);
        if let Err(error) = thread {
            tracing::warn!("no thread to start plans ({error}); a worker does");
            self.scheduler.spawn(
                Priority::Normal,
                "op.dispatch",
                CancelToken::new(),
                move || {
                    this.dispatch();
                },
            );
        }
    }

    /// Queue the work and wait for its result, working out what it
    /// touches on the caller's thread (never the window's).
    fn submit_blocking(&self, work: Work, cancel: &CancelToken) -> Option<ExecResult> {
        let (tx, rx) = bounded(1);
        self.enqueue(work, cancel.clone(), Some(tx));
        self.dispatch();
        rx.recv().ok().flatten()
    }

    fn enqueue(&self, work: Work, cancel: CancelToken, reply: Option<Sender<Option<ExecResult>>>) {
        let mut lane = self.lane.lock();
        if lane.closed {
            cancel.cancel();
        }
        lane.next += 1;
        let seq = lane.next;
        if matches!(work, Work::Execute { .. }) {
            // A plan asked after an undo drops that undo's redo, even one
            // that ends after it (ADR-079, 10).
            let mut journal = self.journal.lock();
            journal.redo_floor = seq;
            journal.redo.clear();
        }
        lane.queue.push_back(Job {
            seq,
            work,
            cancel,
            reply,
            footprint: None,
            target: None,
        });
    }

    /// Work out what the queued jobs touch, in the order asked, and start
    /// each that may start (module documentation): whenever a job is asked
    /// or ends. One caller at a time works out footprints, which may read
    /// (a move's volumes, whether what is deleted is a folder), never
    /// under the lock; the others start what may start meanwhile.
    fn dispatch(&self) {
        while let Some((seq, steps)) = self.next_to_measure() {
            let volumes = super::footprint::Volumes::now();
            let footprint = Footprint::of_steps(steps.iter().map(|s| s.footprint(&volumes)))
                .canonical()
                .with_channels(&volumes);
            let mut lane = self.lane.lock();
            lane.measuring = false;
            if let Some(job) = lane.queue.iter_mut().find(|j| j.seq == seq) {
                job.footprint = Some(Arc::new(footprint));
            }
        }
        for job in self.admit() {
            self.start(job);
        }
    }

    /// The first queued job whose footprint is not known, and the steps it
    /// is worked out from, unless another caller works one out. An undo or
    /// a redo first finds its entry: the latest on its stack asked before
    /// it, unless a plan asked before it, which may put a later one there,
    /// has not ended (it waits for it); no entry and none to come, it
    /// touches nothing.
    fn next_to_measure(&self) -> Option<(u64, Vec<Box<dyn Operation>>)> {
        let mut lane = self.lane.lock();
        if lane.measuring {
            return None;
        }
        let lane = &mut *lane;
        for i in 0..lane.queue.len() {
            if lane.queue[i].footprint.is_some() {
                continue;
            }
            let seq = lane.queue[i].seq;
            let direction = match &lane.queue[i].work {
                Work::Execute { plan, .. } => {
                    lane.measuring = true;
                    return Some((seq, plan.steps.clone()));
                }
                Work::Replay(direction) => *direction,
            };
            let coming = lane
                .queue
                .iter()
                .take(i)
                .map(|j| (j.seq, j.work.adds()))
                .chain(lane.started.iter().map(|b| (b.seq, b.adds)))
                .filter(|&(asked, adds)| asked < seq && adds == direction)
                .map(|(asked, _)| asked)
                .max();
            let journal = self.journal.lock();
            let stack = match direction {
                Direction::Undo => &journal.undo,
                Direction::Redo => &journal.redo,
            };
            let entry = stack
                .iter()
                .rev()
                .find(|e| e.stacked < seq && !lane.targeted.contains(&(direction, e.stacked)));
            match (entry, coming) {
                // A plan asked before it may put a later entry there.
                (entry, Some(coming)) if entry.is_none_or(|e| e.stacked < coming) => {}
                (Some(entry), _) => {
                    let steps = entry
                        .plan
                        .as_ref()
                        .map(|p| p.steps.clone())
                        .unwrap_or_default();
                    let stacked = entry.stacked;
                    drop(journal);
                    lane.targeted.push((direction, stacked));
                    lane.queue[i].target = Some(stacked);
                    lane.measuring = true;
                    return Some((seq, steps));
                }
                (None, _) => {
                    drop(journal);
                    lane.queue[i].footprint = Some(Arc::new(Footprint::default()));
                }
            }
        }
        None
    }

    /// Take out of the queue each job that may start now (module
    /// documentation), in the order asked; a cancelled one at once, which
    /// does nothing.
    fn admit(&self) -> Vec<Job> {
        let mut lane = self.lane.lock();
        let lane = &mut *lane;
        let mut begun = Vec::new();
        let mut i = 0;
        while i < lane.queue.len() {
            let job = &lane.queue[i];
            let may = if job.cancel.is_cancelled() {
                true
            } else if let Some(footprint) = job.footprint.as_deref()
                && lane.started.len() < AT_ONCE
            {
                let before = || lane.queue.iter().take(i).map(|j| j.footprint.as_deref());
                let conflicts = before().any(|f| f.is_none_or(|f| f.conflicts(footprint)))
                    || lane
                        .started
                        .iter()
                        .any(|b| b.footprint.conflicts(footprint));
                // One heavy one at a time on each disk or server, in the
                // order asked there.
                let shares =
                    |other: &Footprint| other.is_heavy() && other.shares_channel(footprint);
                let heavy = footprint.is_heavy()
                    && (lane.started.iter().any(|b| shares(&b.footprint))
                        || before().any(|f| f.is_none_or(shares)));
                !conflicts && !heavy
            } else {
                false
            };
            if !may {
                i += 1;
                continue;
            }
            let job = lane.queue.remove(i).expect("in the queue");
            let (direction, title) = match &job.work {
                Work::Replay(direction) => (Some(*direction), None),
                Work::Execute { plan, .. } => (None, Some(plan.title.clone())),
            };
            let named = self.named(&job);
            lane.started.push(Begun {
                seq: job.seq,
                title,
                named,
                footprint: job.footprint.clone().unwrap_or_default(),
                adds: job.work.adds(),
                target: direction.zip(job.target),
                cancel: job.cancel.clone(),
            });
            begun.push(job);
        }
        begun
    }

    /// Run `job` on a thread of its own, a worker's where none is had.
    fn start(&self, job: Job) {
        let Some(this) = self.me.upgrade() else {
            return;
        };
        let job = Arc::new(Mutex::new(Some(job)));
        let run = {
            let (this, job) = (Arc::clone(&this), Arc::clone(&job));
            move || {
                if let Some(job) = job.lock().take() {
                    this.run_job(job);
                }
            }
        };
        let thread = std::thread::Builder::new()
            .name("skwir-plan".into())
            .spawn(run);
        if let Err(error) = thread {
            tracing::warn!("no thread for a plan ({error}); a worker applies it");
            self.scheduler
                .spawn(Priority::Normal, "op.plan", CancelToken::new(), move || {
                    if let Some(job) = job.lock().take() {
                        this.run_job(job);
                    }
                });
        }
    }

    fn run_job(&self, job: Job) {
        let _ended = Ended(self, job.seq);
        // A cancelled plan is not checked whole: it stops at once.
        let result = match job.work {
            Work::Execute {
                plan,
                whole,
                continues,
            } => {
                let plan = Arc::try_unwrap(plan).unwrap_or_else(|plan| (*plan).clone());
                Some(self.execute_now(plan, whole, continues, &job.cancel, job.seq))
            }
            Work::Replay(direction) => self.replay(direction, job.target, job.seq, &job.cancel),
        };
        if let Some(reply) = job.reply {
            let _ = reply.send(result);
        }
    }

    /// Apply `plan`, asked as `seq`: unfolded, its names taken settled, and
    /// `whole`, checked first; a retry, joining what it `continues`.
    fn execute_now(
        &self,
        plan: Plan,
        whole: bool,
        continues: Option<Box<Continues>>,
        cancel: &CancelToken,
        seq: u64,
    ) -> ExecResult {
        let whole = whole && !cancel.is_cancelled();
        let (first, partial) = continues.map_or((None, None), |c| (Some(c.seq), c.partial));
        let companions = Arc::clone(&self.companions.lock());
        // Each step as it will be done, before anything is settled or
        // checked.
        let plan = if cancel.is_cancelled() {
            plan
        } else {
            plan.unfold(
                &StepContext::with_cancel(cancel.clone()).with_companions(Arc::clone(&companions)),
            )
        };
        // Names taken are settled first: what the dry run checks is what
        // will be done.
        let settled = if whole
            && super::clash::takes_names(&plan.steps, &StepContext::with_cancel(cancel.clone()))
        {
            self.settle(&plan, &companions, cancel)
        } else {
            Settling::Steps(None)
        };
        // A retry that applies nothing (its server not answering yet, the
        // question of a name taken cancelled) stays to be retried, under
        // the id it is told by now, with what it would go on from.
        let kept = |plan: &Plan, partial| {
            if let Some(first) = first {
                self.stash(plan.id, first, plan.clone(), partial);
            }
        };
        match settled {
            Settling::Refused(reason) => {
                kept(&plan, partial);
                self.refuse(&plan, reason)
            }
            Settling::Cancelled => {
                kept(&plan, partial);
                self.cancelled(&plan)
            }
            Settling::Steps(settled) => {
                let plan = match settled {
                    Some(settled) => Plan {
                        steps: settled.steps,
                        items: settled.items.map(Box::new),
                        ..plan
                    },
                    None => plan,
                };
                // A conflict anywhere, or too little room where it writes
                // and the user not going on, applies nothing.
                let refused = whole
                    .then(|| {
                        first_conflict(&plan, &companions, cancel)
                            .or_else(|| self.short_of_room(&plan, cancel))
                    })
                    .flatten();
                match refused {
                    Some(reason) => {
                        kept(&plan, partial);
                        self.refuse(&plan, reason)
                    }
                    None => self.run_execute(&plan, cancel, seq, first, partial),
                }
            }
        }
    }

    /// A plan that will not be applied at all: say so with the usual events.
    fn refuse(&self, plan: &Plan, error: String) -> ExecResult {
        self.emit(ExecEvent::Started {
            plan: plan.id,
            title: plan.title.clone(),
            total: plan.steps.len(),
            run: RunKind::Apply,
            items: None,
        });
        self.emit(ExecEvent::Finished {
            plan: plan.id,
            applied: 0,
            skipped: 0,
            error: Some(error.clone()),
            cancelled: false,
            undoable: false,
        });
        ExecResult {
            applied: 0,
            skipped: 0,
            error: Some(error),
            cancelled: false,
            undoable: false,
            complete: false,
        }
    }

    /// A plan the user cancelled before it began (the names it found
    /// taken, unanswered): whoever waits for it hears so; nothing started,
    /// so nothing is shown.
    fn cancelled(&self, plan: &Plan) -> ExecResult {
        self.emit(ExecEvent::Finished {
            plan: plan.id,
            applied: 0,
            skipped: 0,
            error: None,
            cancelled: true,
            undoable: false,
        });
        ExecResult {
            applied: 0,
            skipped: 0,
            error: None,
            cancelled: true,
            undoable: false,
            complete: false,
        }
    }

    /// Apply `plan`, asked as `seq`, and journal it; a retry of the action
    /// whose entry is at `first`, going on from `partial`.
    fn run_execute(
        &self,
        plan: &Plan,
        cancel: &CancelToken,
        seq: u64,
        first: Option<u64>,
        partial: Option<super::staging::Partial>,
    ) -> ExecResult {
        self.apply_plan(plan, cancel, RunKind::Apply, partial, |r, inverse, stop| {
            let mut j = self.journal.lock();
            // In its place in the order asked: plans that ran beside it
            // touched nothing it did (ADR-079). A retry in the place of
            // what it continues, one action, unless a plan asked since put
            // an entry after it, which may have touched what it then did:
            // undoing that first, in the order asked, puts back each state.
            let place = match first {
                Some(first) if !j.undo.iter().any(|e| e.stacked > first && e.stacked < seq) => {
                    first
                }
                _ => seq,
            };
            if r.applied > 0 {
                match j.undo.iter_mut().find(|e| e.stacked == place) {
                    Some(entry) => {
                        // What the retry applied is taken back first: it
                        // came after.
                        if let Some(mut inverse) = inverse {
                            if let Some(earlier) = entry.plan.take() {
                                inverse.title = earlier.title;
                                inverse.steps.extend(earlier.steps);
                            }
                            entry.plan = Some(inverse);
                        }
                        entry.applied_at = SystemTime::now();
                    }
                    None => j.put(
                        Direction::Undo,
                        JournalEntry {
                            title: plan.title.clone(),
                            applied_at: SystemTime::now(),
                            plan: inverse,
                            partial: Vec::new(),
                            seq: place,
                            stacked: place,
                        },
                    ),
                }
                j.bound();
            }
            drop(j);
            // Stopped by a step's error, what it had left waits for Retry;
            // what stopped it otherwise (Cancel, a write-out) is not.
            if let Some(Stop { at, partial }) = stop {
                let mut rest = Plan::with_steps(plan.title.clone(), plan.steps[at..].to_vec());
                rest.on_conflict = plan.on_conflict;
                rest.items.clone_from(&plan.items);
                self.stash(plan.id, place, rest, partial);
            }
        })
    }

    /// Keep `rest`, what the plan `plan` had left when it stopped, for
    /// Retry, its entry at `seq`, with what it left of a file cut short;
    /// the oldest let go past [`STOPPED_KEPT`] (what it left removed).
    fn stash(&self, plan: PlanId, seq: u64, rest: Plan, partial: Option<super::staging::Partial>) {
        let let_go = {
            let mut stopped = self.stopped.lock();
            stopped.push_back(Stopped {
                plan,
                seq,
                rest,
                partial,
            });
            let over = stopped.len().saturating_sub(STOPPED_KEPT);
            stopped.drain(..over).collect::<Vec<_>>()
        };
        // Removed out of the lock: on a remote, a removal may wait.
        drop(let_go);
    }

    /// Take back the top entry of one stack and journal what takes that
    /// back on the other. A run that stops partway stays on its stack to be
    /// retried; its steps are idempotent.
    fn replay(
        &self,
        direction: Direction,
        target: Option<u64>,
        seq: u64,
        cancel: &CancelToken,
    ) -> Option<ExecResult> {
        if cancel.is_cancelled() {
            return None;
        }
        let mut entry = self.journal.lock().take(direction, target?)?;
        let title = match direction {
            Direction::Undo => format!("undo {}", entry.title),
            Direction::Redo => entry.title.clone(),
        };
        let Some(plan) = &entry.plan else {
            // Nothing applied had an inverse (a permanent delete). Say so in
            // the usual way and drop the entry, so the next undo reaches
            // what came before it.
            let error = format!("“{}” cannot be undone", entry.title);
            let id = Plan::new(title.clone()).id;
            self.emit(ExecEvent::Started {
                plan: id,
                title,
                total: 0,
                run: direction.into(),
                items: None,
            });
            self.emit(ExecEvent::Finished {
                plan: id,
                applied: 0,
                skipped: 0,
                error: Some(error.clone()),
                cancelled: false,
                undoable: false,
            });
            return Some(ExecResult {
                applied: 0,
                skipped: 0,
                error: Some(error),
                cancelled: false,
                undoable: false,
                complete: false,
            });
        };
        let mut run = plan.clone();
        run.title = title;
        let r = self.apply_plan(&run, cancel, direction.into(), None, |r, back, _| {
            // This run's inverses come first: its steps were applied after
            // the ones an earlier, interrupted attempt already took back.
            let mut steps = back.map(|p| p.steps).unwrap_or_default();
            steps.append(&mut entry.partial);
            let mut j = self.journal.lock();
            if r.error.is_none() && !r.cancelled {
                // Undone, it is redone from the redo stack, unless a plan
                // was asked after this undo; redone, it takes its place in
                // the history again.
                let (other, stacked) = match direction {
                    Direction::Undo => (Direction::Redo, seq),
                    Direction::Redo => (Direction::Undo, entry.seq),
                };
                let kept = direction == Direction::Redo || seq > j.redo_floor;
                if !steps.is_empty() && kept {
                    let other_entry = JournalEntry {
                        plan: Some(Plan::with_steps(entry.title.clone(), steps)),
                        title: entry.title,
                        applied_at: SystemTime::now(),
                        partial: Vec::new(),
                        seq: entry.seq,
                        stacked,
                    };
                    j.put(other, other_entry);
                }
            } else {
                entry.partial = steps;
                j.put(direction, entry);
            }
            j.bound();
        });
        Some(r)
    }

    /// The core loop: check, skip if done, apply, collect inverses in reverse order.
    /// `record` journals what was applied, and its inverse, before the plan
    /// says it finished: whoever hears `Finished` (the toast's Undo, Redo
    /// and Retry) finds the journal as the plan left it. It is told where
    /// a step's error stopped the plan, if one did. A retry's copies go on
    /// from `partial`, what the run it retries left of a file.
    fn apply_plan(
        &self,
        plan: &Plan,
        cancel: &CancelToken,
        run: RunKind,
        partial: Option<super::staging::Partial>,
        record: impl FnOnce(&ExecResult, Option<Plan>, Option<Stop>),
    ) -> ExecResult {
        let total = plan.steps.len();
        let meter = Arc::new(Meter::new(self.waker.lock().clone()));
        let pause = Arc::new(Pause::default());
        self.running.lock().push(Active {
            plan: plan.id,
            cancel: cancel.clone(),
            pause: Arc::clone(&pause),
            meter: Arc::clone(&meter),
        });
        let _running = Running(self, plan.id);
        let ctx = StepContext::new(cancel.clone(), pause, Arc::clone(&meter))
            .with_companions(Arc::clone(&self.companions.lock()))
            .with_transfers(Arc::clone(&self.transfers.lock()))
            .with_under_way(self.under_way.lock().clone())
            .with_secret_asker(self.secret_asker.lock().clone())
            .with_partial(partial);
        self.emit(ExecEvent::Started {
            plan: plan.id,
            title: plan.title.clone(),
            total,
            run,
            items: plan.items.as_deref().cloned(),
        });
        let mut inverses: Vec<Box<dyn Operation>> = Vec::new();
        let mut held = Vec::new();
        let mut applied = 0;
        let mut skipped = 0;
        let mut complete = true;
        let mut error = None;
        let mut failed = None;
        let mut cancelled = false;
        for (i, step) in plan.steps.iter().enumerate() {
            // Paused, the next step waits here.
            if ctx.check_cancelled().is_err() {
                cancelled = true;
                break;
            }
            // The copies from here into one folder, made at once where one
            // end is a remote (ADR-078): each step then places its own.
            if super::transfer::batch_ahead(&plan.steps, i, &ctx).is_err() {
                cancelled = true;
                break;
            }
            // The removals from here in a remote's folder, made at once.
            if super::transfer::remove_ahead(&plan.steps, i, &ctx).is_err() {
                cancelled = true;
                break;
            }
            let change = step.describe();
            // What a toast names: read before the step takes the record.
            let mut named = if i == 0 { self::named(&change) } else { None };
            ctx.step_begins();
            // A step that panics fails, as one that errs: the plan ends
            // and says so, and what was applied before it stays undoable.
            // Unwound, it would take the plans' thread with it.
            let status =
                super::guarded(|| Ok(step.check(&ctx))).unwrap_or_else(|e| StepStatus::Conflict {
                    reason: super::said(&e),
                });
            let mut err = None;
            let mut holds = false;
            match &status {
                StepStatus::AlreadyDone => {
                    skipped += 1;
                    holds = true;
                }
                StepStatus::Conflict { reason } => err = Some(reason.clone()),
                StepStatus::Pending { .. } => match super::guarded(|| step.apply(&ctx)) {
                    Ok(Some(inv)) => {
                        applied += 1;
                        holds = true;
                        inverses.push(inv);
                    }
                    Ok(None) => {
                        applied += 1;
                        holds = true;
                        complete = false;
                    }
                    // The step stopped midway and undid its own partial work:
                    // the plan is cancelled, not failed.
                    Err(e) if e.is::<Cancelled>() => {
                        cancelled = true;
                        break;
                    }
                    Err(e) => {
                        tracing::warn!(plan = %plan.title, "a step failed: {e:#}");
                        err = Some(super::said(&e));
                    }
                },
            }
            if holds {
                held.push(change.clone());
            }
            // Failed, its record is still there: the toast names it too.
            if err.is_some() && named.is_none() {
                named = self::named(&change);
            }
            meter.step_ended();
            self.emit_step(ExecEvent::Step(Progress {
                plan: plan.id,
                step: i,
                total,
                change,
                named,
                status,
                error: err.clone(),
            }));
            if let Some(e) = err {
                error = Some(e);
                failed = Some(i);
                break;
            }
        }
        // Cut short, the removals a removal ahead made for steps that never
        // ran are done all the same: said, and taken back by Undo as theirs.
        for step in &plan.steps {
            if !super::transfer::removed_unclaimed(step.as_ref(), &ctx) {
                continue;
            }
            match super::guarded(|| step.apply(&ctx)) {
                Ok(inverse) => {
                    applied += 1;
                    held.push(step.describe());
                    match inverse {
                        Some(inverse) => inverses.push(inverse),
                        None => complete = false,
                    }
                }
                Err(e) => tracing::warn!(plan = %plan.title, "a step removed ahead: {e:#}"),
            }
        }
        // Cut short, what a batch placed for steps that never ran goes:
        // no Undo holds it (ADR-078).
        super::transfer::take_back_unclaimed(&ctx);
        // The originals of moves a batch copied from a remote, together;
        // cancelled too: their steps are done.
        if let Err(e) = super::transfer::remove_deferred(&ctx) {
            tracing::warn!(plan = %plan.title, "{e:#}");
            error.get_or_insert_with(|| super::said(&e));
        }
        // What the plan wrote through to a drive is on it, names and all,
        // before it says it ended: cut short too, for what it did.
        if let Err(e) = ctx.write_out() {
            tracing::warn!(plan = %plan.title, "{e:#}");
            error.get_or_insert_with(|| super::said(&e));
        }
        // What transfers changed beside a mount, no watch saw: read again
        // where it is shown.
        let beside = ctx.take_changed_beside();
        if !beside.is_empty() {
            // Shown anew by the mount just before it is read: a look at
            // it during the plan may have kept what it held then.
            let transfers = ctx.transfers().map(super::transfer::Transfers::all);
            for folder in &beside {
                for transfer in transfers.iter().flatten() {
                    if transfer.show(folder) {
                        break;
                    }
                }
            }
            if let Some(told) = self.changed_beside.lock().clone() {
                told(&beside);
            }
        }
        inverses.reverse();
        let inverse = if inverses.is_empty() {
            None
        } else {
            Some(Plan::with_steps(format!("undo {}", plan.title), inverses))
        };
        let result = ExecResult {
            applied,
            skipped,
            error,
            cancelled,
            undoable: inverse.is_some(),
            complete,
        };
        // Stopped by a step's error, what it had left waits for Retry,
        // with what a copy cut short left; what stopped it otherwise
        // (Cancel, a write-out) is not retried.
        let stop = match (failed, &result.error, cancelled) {
            (Some(at), Some(_), false) => Some(Stop {
                at,
                partial: ctx.take_partial(),
            }),
            _ => None,
        };
        record(&result, inverse, stop);
        self.emit(ExecEvent::Finished {
            plan: plan.id,
            applied,
            skipped,
            error: result.error.clone(),
            cancelled,
            undoable: result.undoable,
        });
        if !held.is_empty() {
            for observer in self.observers.all() {
                observer(&held);
            }
        }
        result
    }
}

/// The reason the first conflicting step gives, after a dry run of every
/// step with one shared context, which stops when the plan is cancelled
/// (a check walking a tree says then what it has seen).
fn first_conflict(
    plan: &Plan,
    companions: &Arc<crate::companions::Companions>,
    cancel: &CancelToken,
) -> Option<String> {
    let ctx = dry_context(&plan.steps, companions, cancel);
    super::dry_run_in(&plan.steps, ctx).find_map(|(_, status)| match status {
        StepStatus::Conflict { reason } => Some(reason),
        _ => None,
    })
}

/// A context for a dry run of `steps` before a plan the user asked for as
/// one action runs: where a step names a remote, its folders' names are
/// read once and its sources found ahead, at once (ADR-078), where a look
/// at each would be a round trip to the server.
fn dry_context(
    steps: &[Box<dyn Operation>],
    companions: &Arc<crate::companions::Companions>,
    cancel: &CancelToken,
) -> StepContext {
    let ctx = StepContext::with_cancel(cancel.clone())
        .with_companions(Arc::clone(companions))
        .listing_remotes();
    super::transfer::resolve_ahead(steps, &ctx);
    ctx
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use super::*;

    /// A step that counts its applications and conflicts when told to.
    #[derive(Debug, Clone)]
    struct Probe {
        applied: Arc<AtomicUsize>,
        conflict: bool,
    }

    impl Operation for Probe {
        fn kind(&self) -> &'static str {
            "probe"
        }
        fn args(&self) -> serde_json::Value {
            serde_json::json!({})
        }
        fn describe(&self) -> Change {
            Change::Trash {
                path: "/example/probe".into(),
            }
        }
        fn check(&self, _: &StepContext) -> StepStatus {
            if self.conflict {
                StepStatus::Conflict {
                    reason: "in the way".into(),
                }
            } else {
                StepStatus::Pending { reversible: false }
            }
        }
        fn apply(&self, _: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
            self.applied.fetch_add(1, Ordering::SeqCst);
            Ok(None)
        }
        fn clone_box(&self) -> Box<dyn Operation> {
            Box::new(self.clone())
        }
    }

    /// A step that makes `path` (heavy where said), waits for its gate to
    /// open, and says so in `log`; its inverse, never held, says it undid.
    #[derive(Debug, Clone)]
    struct Gate {
        path: PathBuf,
        heavy: bool,
        undoing: bool,
        open: Arc<(parking_lot::Mutex<bool>, parking_lot::Condvar)>,
        log: Arc<parking_lot::Mutex<Vec<String>>>,
    }

    impl Gate {
        fn new(
            path: &str,
            heavy: bool,
            open: bool,
            log: &Arc<parking_lot::Mutex<Vec<String>>>,
        ) -> Self {
            Gate {
                path: PathBuf::from(path),
                heavy,
                undoing: false,
                open: Arc::new((parking_lot::Mutex::new(open), parking_lot::Condvar::new())),
                log: Arc::clone(log),
            }
        }

        fn opened(&self) {
            *self.open.0.lock() = true;
            self.open.1.notify_all();
        }

        fn plan(&self) -> Plan {
            Plan::with_steps(
                self.path.display().to_string(),
                vec![Box::new(self.clone())],
            )
        }
    }

    impl Operation for Gate {
        fn kind(&self) -> &'static str {
            "gate"
        }
        fn args(&self) -> serde_json::Value {
            serde_json::json!({})
        }
        fn describe(&self) -> Change {
            Change::Create {
                path: self.path.clone(),
                directory: false,
                words: None,
            }
        }
        fn check(&self, _: &StepContext) -> StepStatus {
            StepStatus::Pending { reversible: true }
        }
        fn apply(&self, _: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
            let verb = if self.undoing { "undid" } else { "began" };
            self.log
                .lock()
                .push(format!("{verb} {}", self.path.display()));
            let mut open = self.open.0.lock();
            while !*open {
                self.open.1.wait(&mut open);
            }
            drop(open);
            if !self.undoing {
                self.log
                    .lock()
                    .push(format!("ended {}", self.path.display()));
            }
            Ok(Some(Box::new(Gate {
                undoing: true,
                open: Arc::new((parking_lot::Mutex::new(true), parking_lot::Condvar::new())),
                ..self.clone()
            })))
        }
        fn clone_box(&self) -> Box<dyn Operation> {
            Box::new(self.clone())
        }
        fn footprint(&self, _: &super::super::footprint::Volumes) -> Option<Footprint> {
            Some(Footprint::default().made(&self.path).heavy_if(self.heavy))
        }
    }

    /// A step that makes `path`, failing while its server is `down`, and
    /// conflicting while it is `blocked`; it and its inverse say so in
    /// `log`.
    #[derive(Debug, Clone)]
    struct Flaky {
        path: PathBuf,
        undoing: bool,
        down: Arc<std::sync::atomic::AtomicBool>,
        blocked: Arc<std::sync::atomic::AtomicBool>,
        log: Arc<parking_lot::Mutex<Vec<String>>>,
    }

    impl Operation for Flaky {
        fn kind(&self) -> &'static str {
            "flaky"
        }
        fn args(&self) -> serde_json::Value {
            serde_json::json!({})
        }
        fn describe(&self) -> Change {
            Change::Create {
                path: self.path.clone(),
                directory: false,
                words: None,
            }
        }
        fn check(&self, _: &StepContext) -> StepStatus {
            if self.blocked.load(Ordering::SeqCst) {
                StepStatus::Conflict {
                    reason: "not answering".into(),
                }
            } else {
                StepStatus::Pending { reversible: true }
            }
        }
        fn apply(&self, _: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
            anyhow::ensure!(!self.down.load(Ordering::SeqCst), "the server went away");
            let verb = if self.undoing { "unmade" } else { "made" };
            self.log
                .lock()
                .push(format!("{verb} {}", self.path.display()));
            Ok(Some(Box::new(Flaky {
                undoing: !self.undoing,
                ..self.clone()
            })))
        }
        fn clone_box(&self) -> Box<dyn Operation> {
            Box::new(self.clone())
        }
        fn footprint(&self, _: &super::super::footprint::Volumes) -> Option<Footprint> {
            Some(Footprint::default().made(&self.path))
        }
    }

    /// Steps making `paths`, the one named `fails` failing while `down`.
    struct Server {
        down: Arc<std::sync::atomic::AtomicBool>,
        blocked: Arc<std::sync::atomic::AtomicBool>,
        log: Arc<parking_lot::Mutex<Vec<String>>>,
    }

    impl Server {
        fn new() -> Self {
            Server {
                down: Arc::default(),
                blocked: Arc::default(),
                log: Arc::default(),
            }
        }

        fn plan(&self, title: &str, paths: &[&str], fails: &str) -> Plan {
            let steps = paths
                .iter()
                .map(|path| {
                    let fine = Arc::new(std::sync::atomic::AtomicBool::new(false));
                    Box::new(Flaky {
                        path: PathBuf::from(path),
                        undoing: false,
                        down: if *path == fails {
                            Arc::clone(&self.down)
                        } else {
                            Arc::clone(&fine)
                        },
                        blocked: if *path == fails {
                            Arc::clone(&self.blocked)
                        } else {
                            fine
                        },
                        log: Arc::clone(&self.log),
                    }) as Box<dyn Operation>
                })
                .collect();
            Plan::with_steps(title, steps)
        }

        fn said(&self) -> Vec<String> {
            std::mem::take(&mut *self.log.lock())
        }
    }

    /// A plan an error stopped is retried from the step that failed, and
    /// what the retry applies is undone with it, one action; a retry
    /// refused before it applies anything stays to be retried.
    #[test]
    fn a_stopped_plan_is_retried_from_where_it_stopped_and_undone_whole() {
        let executor = Executor::new(Scheduler::new(2));
        let server = Server::new();
        server.down.store(true, Ordering::SeqCst);
        let plan = server.plan("upload", &["/srv/a", "/srv/b", "/srv/c"], "/srv/b");
        let id = plan.id;
        let r = executor.execute_blocking(&plan, &CancelToken::new());
        assert_eq!(
            (r.applied, r.error.as_deref()),
            (1, Some("the server went away"))
        );
        assert!(executor.can_retry(id) && executor.last_stopped() == Some(id));
        assert_eq!(server.said(), ["made /srv/a"]);

        // Still not answering: the whole retry is checked first, refused,
        // and kept under the id its run was told by.
        server.blocked.store(true, Ordering::SeqCst);
        let r = executor.retry_blocking(id, &CancelToken::new()).unwrap();
        assert_eq!((r.applied, r.error.as_deref()), (0, Some("not answering")));
        assert!(!executor.can_retry(id));
        let again = executor.last_stopped().expect("still to retry");

        server.blocked.store(false, Ordering::SeqCst);
        server.down.store(false, Ordering::SeqCst);
        let r = executor.retry_blocking(again, &CancelToken::new()).unwrap();
        assert_eq!((r.applied, r.error), (2, None));
        assert_eq!(server.said(), ["made /srv/b", "made /srv/c"]);
        assert_eq!(executor.last_stopped(), None);
        assert_eq!(executor.undo_titles(), ["upload"], "one action");

        executor.undo_blocking(&CancelToken::new()).unwrap();
        assert_eq!(
            server.said(),
            ["unmade /srv/c", "unmade /srv/b", "unmade /srv/a"]
        );
        assert!(!executor.can_undo());
    }

    /// A plan asked after a stopped one put an entry after it: the retry's
    /// own entry is then the latest, undone first.
    #[test]
    fn a_retry_after_another_action_is_an_action_of_its_own() {
        let executor = Executor::new(Scheduler::new(2));
        let server = Server::new();
        server.down.store(true, Ordering::SeqCst);
        let plan = server.plan("upload", &["/srv/a", "/srv/b"], "/srv/b");
        executor.execute_blocking(&plan, &CancelToken::new());
        let other = server.plan("rename", &["/srv/z"], "");
        executor.execute_blocking(&other, &CancelToken::new());
        server.down.store(false, Ordering::SeqCst);
        let r = executor
            .retry_blocking(plan.id, &CancelToken::new())
            .unwrap();
        assert_eq!(r.applied, 1);
        assert_eq!(executor.undo_titles(), ["upload", "rename", "upload"]);
        server.said();
        executor.undo_blocking(&CancelToken::new()).unwrap();
        assert_eq!(server.said(), ["unmade /srv/b"]);
    }

    /// Wait until `log` says `line`, at most five seconds.
    fn logged(log: &Arc<parking_lot::Mutex<Vec<String>>>, line: &str) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if log.lock().iter().any(|l| l == line) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }

    /// A light plan runs beside a heavy one it shares nothing with; one
    /// within the heavy one's tree, and another heavy one, wait for it
    /// (ADR-079). Two that make names in one folder meet: their folders
    /// differ here.
    #[test]
    fn a_light_plan_runs_beside_a_heavy_one_and_what_meets_it_waits() {
        let executor = Executor::new(Scheduler::new(2));
        let log = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let copy = Gate::new("/stick/photos", true, false, &log);
        executor.execute(copy.plan(), CancelToken::new());
        assert!(logged(&log, "began /stick/photos"));
        let rename = Gate::new("/disk/notes/a.txt", false, true, &log);
        let r = executor.execute_blocking(&rename.plan(), &CancelToken::new());
        assert_eq!(r.applied, 1, "beside the copy");
        let inside = Gate::new("/stick/photos/2026", false, true, &log);
        executor.execute(inside.plan(), CancelToken::new());
        let other = Gate::new("/card/music", true, true, &log);
        executor.execute(other.plan(), CancelToken::new());
        std::thread::sleep(Duration::from_millis(100));
        assert!(
            !log.lock()
                .iter()
                .any(|l| l.contains("/stick/photos/2026") || l.contains("/card"))
        );
        assert_eq!(executor.queued(), 2);
        copy.opened();
        assert!(logged(&log, "ended /stick/photos/2026") && logged(&log, "ended /card/music"));
        let order: Vec<String> = log.lock().clone();
        let at = |line: &str| order.iter().position(|l| l == line).unwrap();
        assert!(at("ended /stick/photos") < at("began /stick/photos/2026"));
        assert!(at("ended /stick/photos") < at("began /card/music"));
    }

    /// Heavy plans on two servers run at once; on one, they take turns
    /// (ADR-079: one heavy action a channel).
    #[cfg(target_os = "linux")]
    #[test]
    fn heavy_plans_on_two_channels_run_at_once_and_on_one_take_turns() {
        crate::fs::reach::pretend_remote(Path::new("/servers/one"));
        crate::fs::reach::pretend_remote(Path::new("/servers/two"));
        let executor = Executor::new(Scheduler::new(2));
        let log = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let up = Gate::new("/servers/one/photos", true, false, &log);
        executor.execute(up.plan(), CancelToken::new());
        assert!(logged(&log, "began /servers/one/photos"));
        let other = Gate::new("/servers/two/music", true, true, &log);
        let r = executor.execute_blocking(&other.plan(), &CancelToken::new());
        assert_eq!(r.applied, 1, "beside, on another server");
        let same = Gate::new("/servers/one/videos", true, true, &log);
        executor.execute(same.plan(), CancelToken::new());
        std::thread::sleep(Duration::from_millis(100));
        assert!(!log.lock().iter().any(|l| l == "began /servers/one/videos"));
        up.opened();
        assert!(logged(&log, "ended /servers/one/videos"));
    }

    /// What waits says what for: one that meets a running plan names it;
    /// a heavy one on the same server says the server (ADR-079, 12).
    #[cfg(target_os = "linux")]
    #[test]
    fn what_waits_says_what_it_waits_for() {
        crate::fs::reach::pretend_remote(Path::new("/servers/one"));
        let executor = Executor::new(Scheduler::new(2));
        let log = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let up = Gate::new("/servers/one/a/photos", true, false, &log);
        executor.execute(up.plan(), CancelToken::new());
        assert!(logged(&log, "began /servers/one/a/photos"));
        let inside = Gate::new("/servers/one/a/photos/it's 1.jpg", false, true, &log);
        executor.execute(inside.plan(), CancelToken::new());
        let other = Gate::new("/servers/one/b/videos", true, true, &log);
        executor.execute(other.plan(), CancelToken::new());
        let deadline = Instant::now() + Duration::from_secs(5);
        let waiting = loop {
            let waiting = executor.waiting();
            if waiting.iter().all(|w| w.waits_for != WaitsFor::Starting)
                || Instant::now() > deadline
            {
                break waiting;
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(
            waiting,
            [
                Waiting {
                    named: "/servers/one/a/photos/it's 1.jpg".into(),
                    waits_for: WaitsFor::Action("/servers/one/a/photos".into()),
                },
                Waiting {
                    named: "/servers/one/b/videos".into(),
                    waits_for: WaitsFor::Server,
                },
            ]
        );
        up.opened();
        assert!(logged(&log, "ended /servers/one/b/videos"));
    }

    /// Undo takes the last plan asked: one ended, beside a heavy plan it
    /// shares nothing with; then the heavy one, once it ends.
    #[test]
    fn undo_takes_the_last_asked_and_waits_for_it_while_it_runs() {
        let executor = Executor::new(Scheduler::new(2));
        let log = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let copy = Gate::new("/stick/photos", true, false, &log);
        executor.execute(copy.plan(), CancelToken::new());
        assert!(logged(&log, "began /stick/photos"));
        let rename = Gate::new("/disk/notes/a.txt", false, true, &log);
        executor.execute_blocking(&rename.plan(), &CancelToken::new());
        assert_eq!(
            executor.undo_target(),
            Some(("/disk/notes/a.txt".to_owned(), false)),
            "the last asked, ended"
        );
        let undone = executor.undo_blocking(&CancelToken::new()).unwrap();
        assert_eq!(undone.applied, 1);
        assert!(logged(&log, "undid /disk/notes/a.txt"));
        assert!(
            !log.lock().iter().any(|l| l == "ended /stick/photos"),
            "beside the copy"
        );
        // The copy is the latest now: Undo waits for it.
        assert_eq!(
            executor.undo_target(),
            Some(("/stick/photos".to_owned(), true))
        );
        assert!(executor.undo(CancelToken::new()));
        std::thread::sleep(Duration::from_millis(100));
        assert!(!log.lock().iter().any(|l| l == "undid /stick/photos"));
        copy.opened();
        assert!(logged(&log, "undid /stick/photos"));
    }

    /// A step that cannot say what it touches runs alone (ADR-079).
    #[test]
    fn a_plan_that_cannot_say_what_it_touches_runs_alone() {
        let executor = Executor::new(Scheduler::new(2));
        let log = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let copy = Gate::new("/stick/photos", true, false, &log);
        executor.execute(copy.plan(), CancelToken::new());
        assert!(logged(&log, "began /stick/photos"));
        let applied = Arc::new(AtomicUsize::new(0));
        let unknown = Plan::with_steps(
            "unknown",
            vec![Box::new(Probe {
                applied: Arc::clone(&applied),
                conflict: false,
            }) as Box<dyn Operation>],
        );
        executor.execute(unknown, CancelToken::new());
        let later = Gate::new("/elsewhere", false, true, &log);
        executor.execute(later.plan(), CancelToken::new());
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(applied.load(Ordering::SeqCst), 0);
        assert!(
            !log.lock().iter().any(|l| l == "began /elsewhere"),
            "after it, not beside"
        );
        copy.opened();
        assert!(logged(&log, "ended /elsewhere"));
        assert_eq!(applied.load(Ordering::SeqCst), 1);
    }

    /// A plan asked after an undo drops its redo (ADR-079, 10).
    #[test]
    fn a_plan_asked_after_an_undo_drops_its_redo() {
        let executor = Executor::new(Scheduler::new(2));
        let log = Arc::new(parking_lot::Mutex::new(Vec::new()));
        executor.execute_blocking(
            &Gate::new("/a", false, true, &log).plan(),
            &CancelToken::new(),
        );
        executor.undo_blocking(&CancelToken::new()).unwrap();
        assert!(executor.can_redo());
        executor.execute_blocking(
            &Gate::new("/b", false, true, &log).plan(),
            &CancelToken::new(),
        );
        assert!(!executor.can_redo());
    }

    /// A step that panics when applied.
    #[derive(Debug, Clone)]
    struct Panics;

    impl Operation for Panics {
        fn kind(&self) -> &'static str {
            "panics"
        }
        fn args(&self) -> serde_json::Value {
            serde_json::json!({})
        }
        fn describe(&self) -> Change {
            Change::Trash {
                path: "/example/panics".into(),
            }
        }
        fn check(&self, _: &StepContext) -> StepStatus {
            StepStatus::Pending { reversible: false }
        }
        fn apply(&self, _: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
            panic!("a bug in a step");
        }
        fn clone_box(&self) -> Box<dyn Operation> {
            Box::new(self.clone())
        }
    }

    /// The journal keeps the latest actions, never more than its bound,
    /// and the latest whatever it holds.
    #[test]
    fn the_journal_keeps_the_latest_actions_within_its_bound() {
        let executor = Executor::new(Scheduler::new(1));
        let applied = Arc::new(AtomicUsize::new(0));
        for n in 0..JOURNAL_ENTRIES + 5 {
            let plan = Plan::with_steps(
                format!("plan {n}"),
                vec![Box::new(Probe {
                    applied: Arc::clone(&applied),
                    conflict: false,
                }) as Box<dyn Operation>],
            );
            executor.execute_blocking(&plan, &CancelToken::new());
        }
        let j = executor.journal.lock();
        assert_eq!(j.undo.len(), JOURNAL_ENTRIES);
        assert_eq!(j.undo.first().map(|e| e.title.as_str()), Some("plan 5"));
        let mut big = Journal::default();
        for n in 0..3 {
            big.undo.push(JournalEntry {
                title: n.to_string(),
                applied_at: SystemTime::now(),
                plan: Some(Plan::with_steps(
                    "x",
                    (0..=JOURNAL_BYTES / 256)
                        .map(|_| Box::new(Panics) as Box<dyn Operation>)
                        .collect(),
                )),
                partial: Vec::new(),
                seq: n,
                stacked: n,
            });
        }
        big.bound();
        assert_eq!(big.undo.len(), 1, "the latest stays, however much it holds");
        assert_eq!(big.undo[0].title, "2");
    }

    /// A step that panics fails its plan as an error would, after what was
    /// applied before it, and the executor runs the next plan.
    #[test]
    fn a_step_that_panics_fails_its_plan_and_the_executor_goes_on() {
        let executor = Executor::new(Scheduler::new(1));
        let applied = Arc::new(AtomicUsize::new(0));
        let probe = || {
            Box::new(Probe {
                applied: Arc::clone(&applied),
                conflict: false,
            }) as Box<dyn Operation>
        };
        let plan = Plan::with_steps("panics", vec![probe(), Box::new(Panics), probe()]);
        let r = executor.execute_blocking(&plan, &CancelToken::new());
        assert_eq!(r.applied, 1);
        assert_eq!(r.error.as_deref(), Some("it failed unexpectedly"));
        let next = Plan::with_steps("next", vec![probe()]);
        let r = executor.execute_blocking(&next, &CancelToken::new());
        assert_eq!((r.applied, r.error), (1, None));
        assert_eq!(applied.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_whole_plan_with_a_conflict_applies_nothing_and_says_why() {
        let executor = Executor::new(Scheduler::new(1));
        let applied = Arc::new(AtomicUsize::new(0));
        let probe = |conflict| {
            Box::new(Probe {
                applied: Arc::clone(&applied),
                conflict,
            }) as Box<dyn Operation>
        };
        let done = Arc::new(Mutex::new(None));
        let (tx, rx) = crossbeam_channel::bounded(1);
        let finished = Arc::clone(&done);
        executor.set_waker(Some(Arc::new(move || {
            let _ = tx.try_send(());
        })));
        let plan = Plan::with_steps("two", vec![probe(false), probe(true)]);
        let id = plan.id;
        executor.execute_whole(plan, CancelToken::new());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while finished.lock().unwrap().is_none() {
            let _ = rx.recv_timeout(deadline - std::time::Instant::now());
            for event in executor.events().try_iter() {
                if let ExecEvent::Finished {
                    plan,
                    error,
                    applied,
                    ..
                } = event
                {
                    assert_eq!(plan, id);
                    *finished.lock().unwrap() = Some((applied, error));
                }
            }
            assert!(std::time::Instant::now() < deadline, "the plan finished");
        }
        assert_eq!(
            done.lock().unwrap().clone(),
            Some((0, Some("in the way".to_owned())))
        );
        assert_eq!(applied.load(Ordering::SeqCst), 0, "not even the first step");
        assert!(!executor.can_undo());
    }
}
