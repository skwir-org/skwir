//! Runs plans in the background with progress, pause and cancellation, and
//! keeps the undo/redo journal of inverse plans.
//!
//! Every plan, undo and redo runs on one first-in-first-out lane, so the
//! journal's order is the order things happened on disk. An undo pressed
//! while a copy runs, or waits paused, waits for the copy and then takes it
//! back; two quick undos never race each other. The lane is a queue drained
//! by a thread of its own that ends when the queue is empty, so an idle app
//! keeps no executor thread, and a copy that runs for minutes, or waits
//! paused for as long as the user likes, never holds a scheduler worker:
//! those draw what is on screen.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant, SystemTime};

use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
use parking_lot::{Condvar, Mutex};
use serde::Serialize;

use super::context::{Meter, Pause};
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
/// It runs on the lane after the `Finished` event, before the next plan:
/// keep it brief (update a map, wake), and never wait for the executor.
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

#[derive(Default)]
struct Journal {
    undo: Vec<JournalEntry>,
    redo: Vec<JournalEntry>,
}

/// The actions the undo journal keeps at most, the latest always: past it,
/// the oldest go first.
pub const JOURNAL_ENTRIES: usize = 200;
/// What the journal's entries may hold together (`JournalEntry::weight`):
/// tagging ten thousand photos with XMP sidecars kept every sidecar's text
/// twice, some 600 MB, for as long as skwir ran.
pub const JOURNAL_BYTES: usize = 64 << 20;

impl Journal {
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

#[derive(Clone, Copy)]
enum Direction {
    Undo,
    Redo,
}

enum Work {
    /// `whole`: dry-run every step first and apply nothing if one conflicts.
    Execute {
        plan: Plan,
        whole: bool,
    },
    Replay(Direction),
}

struct Job {
    work: Work,
    cancel: CancelToken,
    reply: Option<Sender<Option<ExecResult>>>,
}

#[derive(Default)]
struct Lane {
    queue: VecDeque<Job>,
    /// A drainer is running (on its thread or a blocking caller's).
    busy: bool,
    /// What stops the job being drained, from the moment it leaves the
    /// queue: it is in neither the queue nor `running` until its plan
    /// starts.
    current: Option<CancelToken>,
    /// Wound down (`Executor::wind_down`): what is queued now is cancelled
    /// as it arrives.
    closed: bool,
}

/// How often steps' events may wake the window: as often as bytes do.
const STEP_WAKE: Duration = Duration::from_millis(100);

/// The executor. One per app.
pub struct Executor {
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
    /// Asks the user for a step's secret (`StepContext::ask_secret`).
    secret_asker: Mutex<Option<SecretAsker>>,
    /// Asks the user what becomes of the names a plan finds taken
    /// (`set_clash_asker`). None asks: such a plan is refused.
    clash_asker: Mutex<Option<ClashAsker>>,
}

/// What a volume has free, by a folder on it.
pub type Free = Arc<dyn Fn(&Path) -> Option<u64> + Send + Sync>;

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

/// Frees the lane if a job panics, so later jobs are not stuck behind it.
struct Draining<'a>(&'a Executor);

impl Drop for Draining<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            let mut lane = self.0.lane.lock();
            lane.busy = false;
            lane.current = None;
            self.0.idle.notify_all();
        }
    }
}

impl Executor {
    pub fn new(scheduler: Scheduler) -> Arc<Self> {
        let (tx, rx) = unbounded();
        Arc::new(Executor {
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
            secret_asker: Mutex::new(None),
            clash_asker: Mutex::new(None),
        })
    }

    /// Ask the user, through `asker`, whether a plan short of room where it
    /// writes goes on anyway (ADR-049): the window's question.
    pub fn set_room_asker(&self, asker: Option<RoomAsker>) {
        *self.room_asker.lock() = asker;
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
    /// what it has open. Plans queued after it wait too, as they wait for
    /// it to finish. Whether the plan runs; `Paused` is told when it was
    /// running freely.
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

    /// Plans, undos and redos waiting for their turn on the lane.
    pub fn queued(&self) -> usize {
        self.lane.lock().queue.len()
    }

    /// Cancel every plan, undo and redo, whether it runs, waits paused or
    /// waits its turn, and take no more; then wait up to `timeout` for the
    /// lane to drain. A cancelled step takes back its own partial work as
    /// it stops (a copy removes its staging folder), so a process that
    /// ends after this leaves no half-made copy behind. Whether the lane
    /// drained in time. It blocks: call it off the window's thread.
    pub fn wind_down(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut lane = self.lane.lock();
        lane.closed = true;
        for token in lane
            .queue
            .iter()
            .map(|job| &job.cancel)
            .chain(&lane.current)
        {
            token.cancel();
        }
        while lane.busy {
            if self.idle.wait_until(&mut lane, deadline).timed_out() {
                return !lane.busy;
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

    /// Queue `plan` on the lane; journal the inverse when it finishes.
    pub fn execute(self: &Arc<Self>, plan: Plan, cancel: CancelToken) {
        self.submit(Work::Execute { plan, whole: false }, cancel);
    }

    /// Like [`Executor::execute`], but all or nothing: when its turn comes,
    /// every step is checked first and a conflict anywhere applies nothing
    /// and finishes with that conflict as the error. For a plan the user
    /// asked for as one action, so it never stops halfway on a conflict
    /// that was there from the start. The checks stat the filesystem, so
    /// they run on the lane, never on the caller's thread.
    pub fn execute_whole(self: &Arc<Self>, plan: Plan, cancel: CancelToken) {
        self.submit(Work::Execute { plan, whole: true }, cancel);
    }

    /// Apply `plan` in lane order, waiting for it, and journal it. Returns
    /// what happened. Never call it from inside an operation.
    pub fn execute_blocking(&self, plan: &Plan, cancel: &CancelToken) -> ExecResult {
        self.submit_blocking(
            Work::Execute {
                plan: plan.clone(),
                whole: false,
            },
            cancel,
        )
        .expect("an executed plan has a result")
    }

    /// Undo the most recent plan once everything queued before has run.
    /// Returns false if there is nothing to undo and nothing that could
    /// add to the journal is queued or running.
    pub fn undo(self: &Arc<Self>, cancel: CancelToken) -> bool {
        let available = self.is_busy() || self.can_undo();
        if available {
            self.submit(Work::Replay(Direction::Undo), cancel);
        }
        available
    }

    /// Undo in lane order, waiting for it. Returns `None` if nothing to undo.
    pub fn undo_blocking(&self, cancel: &CancelToken) -> Option<ExecResult> {
        self.submit_blocking(Work::Replay(Direction::Undo), cancel)
    }

    /// Redo the most recently undone plan once everything queued before has run.
    pub fn redo(self: &Arc<Self>, cancel: CancelToken) -> bool {
        let available = self.is_busy() || self.can_redo();
        if available {
            self.submit(Work::Replay(Direction::Redo), cancel);
        }
        available
    }

    /// Redo in lane order, waiting for it.
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

    /// Titles of the undo stack, most recent last.
    pub fn undo_titles(&self) -> Vec<String> {
        self.journal
            .lock()
            .undo
            .iter()
            .map(|e| e.title.clone())
            .collect()
    }

    /// Whether a plan, an undo or a redo runs or waits its turn.
    pub fn is_busy(&self) -> bool {
        let lane = self.lane.lock();
        lane.busy || !lane.queue.is_empty()
    }

    fn submit(self: &Arc<Self>, work: Work, cancel: CancelToken) {
        if !self.enqueue(Job {
            work,
            cancel,
            reply: None,
        }) {
            // Behind a plan that runs or waits paused: whoever shows the
            // lane says it waits.
            self.wake();
            return;
        }
        // The drainer must run even if the work it drains was cancelled:
        // each job checks its own token.
        let this = Arc::clone(self);
        let thread = std::thread::Builder::new()
            .name("skwir-plans".into())
            .spawn(move || this.drain());
        if let Err(error) = thread {
            tracing::warn!("no thread for the plans ({error}); a worker applies them");
            let this = Arc::clone(self);
            self.scheduler
                .spawn(Priority::Normal, "op.lane", CancelToken::new(), move || {
                    this.drain();
                });
        }
    }

    /// Queue the work and wait for its result. When the lane is idle the
    /// caller drains it itself, so a headless caller needs no free worker.
    fn submit_blocking(&self, work: Work, cancel: &CancelToken) -> Option<ExecResult> {
        let (tx, rx) = bounded(1);
        if self.enqueue(Job {
            work,
            cancel: cancel.clone(),
            reply: Some(tx),
        }) {
            self.drain();
        }
        rx.recv().ok().flatten()
    }

    /// Returns true when the caller must start draining.
    fn enqueue(&self, job: Job) -> bool {
        let mut lane = self.lane.lock();
        if lane.closed {
            job.cancel.cancel();
        }
        lane.queue.push_back(job);
        !std::mem::replace(&mut lane.busy, true)
    }

    fn drain(&self) {
        let _guard = Draining(self);
        loop {
            let job = {
                let mut lane = self.lane.lock();
                let Some(job) = lane.queue.pop_front() else {
                    lane.busy = false;
                    lane.current = None;
                    self.idle.notify_all();
                    return;
                };
                lane.current = Some(job.cancel.clone());
                job
            };
            // A cancelled plan is not checked whole: it stops at once.
            let result = match job.work {
                Work::Execute { plan, whole } => {
                    let whole = whole && !job.cancel.is_cancelled();
                    let companions = Arc::clone(&self.companions.lock());
                    // Each step as it will be done, before anything is
                    // settled or checked.
                    let plan = if job.cancel.is_cancelled() {
                        plan
                    } else {
                        plan.unfold(
                            &StepContext::with_cancel(job.cancel.clone())
                                .with_companions(Arc::clone(&companions)),
                        )
                    };
                    // Names taken are settled first: what the dry run
                    // checks is what will be done.
                    let settled = if whole
                        && super::clash::takes_names(
                            &plan.steps,
                            &StepContext::with_cancel(job.cancel.clone()),
                        ) {
                        self.settle(&plan, &companions, &job.cancel)
                    } else {
                        Settling::Steps(None)
                    };
                    Some(match settled {
                        Settling::Refused(reason) => self.refuse(&plan, reason),
                        Settling::Cancelled => self.cancelled(&plan),
                        Settling::Steps(settled) => {
                            let plan = match settled {
                                Some(settled) => Plan {
                                    steps: settled.steps,
                                    items: settled.items.map(Box::new),
                                    ..plan
                                },
                                None => plan,
                            };
                            // A conflict anywhere, or too little room where
                            // it writes and the user not going on, applies
                            // nothing.
                            let refused = whole
                                .then(|| {
                                    first_conflict(&plan, &companions, &job.cancel)
                                        .or_else(|| self.short_of_room(&plan, &job.cancel))
                                })
                                .flatten();
                            match refused {
                                Some(reason) => self.refuse(&plan, reason),
                                None => self.run_execute(&plan, &job.cancel),
                            }
                        }
                    })
                }
                Work::Replay(direction) => self.replay(direction, &job.cancel),
            };
            if let Some(reply) = job.reply {
                let _ = reply.send(result);
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

    fn run_execute(&self, plan: &Plan, cancel: &CancelToken) -> ExecResult {
        self.apply_plan(plan, cancel, RunKind::Apply, |r, inverse| {
            if r.applied > 0 {
                let mut j = self.journal.lock();
                j.undo.push(JournalEntry {
                    title: plan.title.clone(),
                    applied_at: SystemTime::now(),
                    plan: inverse,
                    partial: Vec::new(),
                });
                j.redo.clear();
                j.bound();
            }
        })
    }

    /// Take back the top entry of one stack and journal what takes that
    /// back on the other. A run that stops partway stays on its stack to be
    /// retried; its steps are idempotent.
    fn replay(&self, direction: Direction, cancel: &CancelToken) -> Option<ExecResult> {
        if cancel.is_cancelled() {
            return None;
        }
        let mut entry = {
            let mut j = self.journal.lock();
            match direction {
                Direction::Undo => j.undo.pop(),
                Direction::Redo => j.redo.pop(),
            }
        }?;
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
        let r = self.apply_plan(&run, cancel, direction.into(), |r, back| {
            // This run's inverses come first: its steps were applied after
            // the ones an earlier, interrupted attempt already took back.
            let mut steps = back.map(|p| p.steps).unwrap_or_default();
            steps.append(&mut entry.partial);
            let mut j = self.journal.lock();
            if r.error.is_none() && !r.cancelled {
                if !steps.is_empty() {
                    let other = JournalEntry {
                        plan: Some(Plan::with_steps(entry.title.clone(), steps)),
                        title: entry.title,
                        applied_at: SystemTime::now(),
                        partial: Vec::new(),
                    };
                    match direction {
                        Direction::Undo => j.redo.push(other),
                        Direction::Redo => j.undo.push(other),
                    }
                }
            } else {
                entry.partial = steps;
                match direction {
                    Direction::Undo => j.undo.push(entry),
                    Direction::Redo => j.redo.push(entry),
                }
            }
            j.bound();
        });
        Some(r)
    }

    /// The core loop: check, skip if done, apply, collect inverses in reverse order.
    /// `record` journals what was applied, and its inverse, before the plan
    /// says it finished: whoever hears `Finished` (the toast's Undo and
    /// Redo) finds the journal as the plan left it.
    fn apply_plan(
        &self,
        plan: &Plan,
        cancel: &CancelToken,
        run: RunKind,
        record: impl FnOnce(&ExecResult, Option<Plan>),
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
            .with_secret_asker(self.secret_asker.lock().clone());
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
                break;
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
        record(&result, inverse);
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
