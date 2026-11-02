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

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant, SystemTime};

use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
use parking_lot::{Condvar, Mutex};
use serde::Serialize;

use super::context::{Compared, Meter, Pause};
use super::footprint::{Footprint, Kept};
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
        /// What the user asked for, as it was done, where the steps are
        /// not its items (`Plan::items`: unfolded, names taken settled):
        /// `Started` is told before the plan is checked, and says what
        /// was asked.
        items: Option<super::Items>,
        /// What its steps said beside what they did (`StepContext::say`,
        /// an `Unneeded`'s why), in order, each once: its answer says it
        /// after what was done.
        said: Vec<String>,
        /// The bytes it moved, and how long its steps ran, its pauses left
        /// out: what its answer says of a long copy (ADR-113).
        moved: u64,
        ran: Duration,
    },
    /// The server the plan `plan` stopped on does not answer: it goes on
    /// by itself once it does (ADR-080, 9).
    WaitsForServer {
        plan: PlanId,
    },
    /// The server the plan `plan` stopped on answers again: what it had
    /// left goes on by itself, as the run `next` (its `Started` follows).
    GoesOn {
        plan: PlanId,
        next: PlanId,
        title: String,
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
        /// What its steps said beside what they did, each once.
        #[serde(skip_serializing_if = "Vec::is_empty")]
        said: Vec<String>,
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

/// How far a running plan is, how fast it goes and how long it has left
/// (ADR-113). Its bytes are those its steps move (copies, cross-device
/// moves), `total` growing as each such step starts; `planned`, what it
/// will move in all, is known from its start where its steps were sized
/// before it began (a plan the user asked for, its retries, the heavy
/// steps of an undo).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct Transfer {
    pub done: u64,
    pub total: u64,
    /// Steps ended (applied, found done, or failed).
    pub ended: usize,
    /// Its items done: the steps ended, and those whose copy a batch
    /// finished before their turn (ADR-078), which their steps place
    /// after it. What a card counts.
    pub finished: usize,
    /// The bytes of the step under way: moved, and announced.
    pub step_done: u64,
    pub step_total: u64,
    /// The files a mount that sends later still sends to its server before
    /// the plan ends (ADR-102), while it does, and their bytes where it
    /// says.
    pub sending: Option<usize>,
    pub sending_bytes: Option<u64>,
    /// What the plan will move in all, when known.
    pub planned: Option<u64>,
    /// Bytes a second, over the last seconds: what it writes, or, while a
    /// mount sends, what that mount sends. `None` before two seconds went
    /// by, while paused, when nothing moved of late, and for a plan that
    /// moves no byte.
    pub speed: Option<f64>,
    /// How long it has left: by its bytes, or by its steps for a plan that
    /// moves none. `None` while that cannot be told (not sized, nothing
    /// moved of late, writing into a mount that sends after). In seconds
    /// for a script.
    #[serde(rename = "left_seconds", serialize_with = "seconds")]
    pub left: Option<Duration>,
    /// It writes into a mount that sends to its server after, which its
    /// end waits for (ADR-102): why no time left is said meanwhile.
    pub then_sends: bool,
}

/// `left` in whole seconds, as a script reads it.
#[allow(clippy::ref_option)] // serde's `serialize_with` hands a reference.
fn seconds<S: serde::Serializer>(left: &Option<Duration>, to: S) -> Result<S::Ok, S::Error> {
    match left {
        Some(left) => to.serialize_some(&left.as_secs()),
        None => to.serialize_none(),
    }
}

/// A plan running, as the `running` command answers it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RunningPlan {
    pub plan: PlanId,
    pub title: String,
    pub paused: bool,
    #[serde(flatten)]
    pub transfer: Transfer,
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

/// A [`ChangeObserver`] told also which plan made the changes: the id of
/// the plan asked (`Plan::id`, as `apply`, a proposal and the toast keep
/// it), its retries' too, those of a plan going on by itself included,
/// which continue it; an undo's or a redo's run is no plan asked, and is
/// told by its own id, which names none (`ExecEvent::Started`). So a
/// plugin that dispatched a plan knows when that plan, and no other of
/// the same changes, applied.
pub type PlanObserver = Arc<dyn Fn(PlanId, &[Change]) + Send + Sync>;

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
    /// The runs that made it, as their events named them (a plan, its
    /// retries, a redo of it): a toast's Undo takes back the one it
    /// reports ([`Executor::undo_of`]).
    asked: Vec<PlanId>,
    /// What they touched, bounded (`Kept`): what a plan asked after it
    /// must not have touched for it to be taken back alone.
    touched: Arc<Kept>,
    /// The entries after it on its stack that touched what it did, by
    /// their `seq`: it is taken back alone once each of them is (ADR-079,
    /// 13). Worked out as each entry joins the stack (`Journal::put`),
    /// never as the history is read.
    blocked_by: BTreeSet<u64>,
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
    /// Counts each change of either stack, which the history's readers
    /// compare (`Executor::history_version`).
    version: u64,
}

/// What a footprint meets on a stack, worked out before it joins, out of
/// the journal's lock (`Executor::meets_on`): each entry's `seq`, the
/// footprint it was compared with, and whether they meet.
#[derive(Default)]
struct Met {
    /// The footprint compared.
    with: Option<Arc<Kept>>,
    entries: HashMap<u64, (Arc<Kept>, bool)>,
}

impl Met {
    /// Whether `entry` and `touched` meet: as worked out, when both are
    /// still what was compared, else compared now.
    fn meets(&self, entry: &JournalEntry, touched: &Arc<Kept>) -> bool {
        let compared = self
            .with
            .as_ref()
            .is_some_and(|with| Arc::ptr_eq(with, touched));
        match self.entries.get(&entry.seq) {
            Some((kept, meets)) if compared && Arc::ptr_eq(kept, &entry.touched) => *meets,
            _ => entry.touched.conflicts(touched),
        }
    }
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

    fn stack_of(&self, direction: Direction) -> &[JournalEntry] {
        match direction {
            Direction::Undo => &self.undo,
            Direction::Redo => &self.redo,
        }
    }

    /// Put `entry` on the stack `direction` takes from, in its place,
    /// noting which entries there it blocks and which block it, as `met`
    /// worked out (ADR-079, 13). One that holds nothing to take back
    /// (deleted for good) blocks those before it too: it took for good
    /// what they did, until Undo, reaching it, lets go of it.
    fn put(&mut self, direction: Direction, mut entry: JournalEntry, met: &Met) {
        self.version += 1;
        let stack = self.stack(direction);
        entry.blocked_by.clear();
        for other in stack.iter_mut() {
            if !met.meets(other, &entry.touched) {
                continue;
            }
            if other.stacked < entry.stacked {
                other.blocked_by.insert(entry.seq);
            } else {
                entry.blocked_by.insert(other.seq);
            }
        }
        let at = stack.partition_point(|e| e.stacked < entry.stacked);
        stack.insert(at, entry);
    }

    /// Take the entry at `stacked` off the stack `direction` takes from:
    /// it blocks none there any more.
    fn take(&mut self, direction: Direction, stacked: u64) -> Option<JournalEntry> {
        self.version += 1;
        let stack = self.stack(direction);
        let at = stack.iter().position(|e| e.stacked == stacked)?;
        let entry = stack.remove(at);
        for other in stack.iter_mut() {
            other.blocked_by.remove(&entry.seq);
        }
        Some(entry)
    }

    /// Let go of what Redo would apply again: a plan was asked.
    fn clear_redo(&mut self) {
        if !self.redo.is_empty() {
            self.version += 1;
            self.redo.clear();
        }
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
            if drop == 0 {
                continue;
            }
            self.version += 1;
            // The oldest go: only those before them could name them, and
            // none is.
            let gone: Vec<u64> = stack.drain(..drop).map(|e| e.seq).collect();
            for entry in stack.iter_mut() {
                for seq in &gone {
                    entry.blocked_by.remove(seq);
                }
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Direction {
    Undo,
    Redo,
}

impl Direction {
    fn other(self) -> Direction {
        match self {
            Direction::Undo => Direction::Redo,
            Direction::Redo => Direction::Undo,
        }
    }
}

enum Work {
    /// `whole`: dry-run every step first and apply nothing if one conflicts.
    Execute {
        plan: Arc<Plan>,
        whole: bool,
        /// A retry: what it continues.
        continues: Option<Box<Continues>>,
    },
    /// An undo or a redo: of the latest entry, or of the one named
    /// ([`Executor::undo_of`], [`Executor::undo_entry`]).
    Replay(Direction, Option<Of>),
}

/// The entry an undo or a redo names: by the plan that made it (a toast's
/// Undo), or by its place in the history (the History panel's).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Of {
    Plan(PlanId),
    Entry(u64),
}

impl Of {
    /// Whether `entry` is the one named.
    fn names(self, entry: &JournalEntry) -> bool {
        match self {
            Of::Plan(plan) => entry.asked.contains(&plan),
            Of::Entry(seq) => entry.seq == seq,
        }
    }
}

/// What a retry continues (`Executor::retry`): the action that stopped,
/// by where its entry is in the history, and what it left of a large
/// file cut short, to go on from.
struct Continues {
    seq: u64,
    /// The plan asked, which it continues: what observers are told.
    origin: PlanId,
    partial: Option<super::staging::Partial>,
    waits: Waits,
    /// It goes on by itself, its server answering again (ADR-080, 9):
    /// the action asked already, going on, not one asked now.
    by_itself: bool,
}

/// A plan a step's error stopped, what Retry runs of it.
struct Stopped {
    /// The run that stopped, as its events named it.
    plan: PlanId,
    /// Where its entry is, or would be, in the history.
    seq: u64,
    /// The plan asked, which a retry continues (`Continues::origin`).
    origin: PlanId,
    /// Its steps from the one that failed on, unfolded and settled.
    rest: Plan,
    /// What it left of a large file cut short, kept until it is retried
    /// or let go.
    partial: Option<super::staging::Partial>,
    /// The server it waits for, to go on by itself (ADR-080, 9).
    waits: Waits,
}

impl Stopped {
    /// Whether it waits for the server whose mount is at `point`.
    fn waits_for(&self, point: &Path) -> bool {
        self.waits.server.as_deref() == Some(point) && self.waits.tries < ON_ITS_OWN
    }

    /// Whether it waits for its server, seen not answering, to answer
    /// again and go on by itself.
    fn goes_on_by_itself(&self) -> bool {
        self.waits.down && self.waits.tries < ON_ITS_OWN
    }
}

/// A stopped plan taken out to be retried, counted as handed over until
/// it is let go of, once its work is queued (`Executor::busy_or_waiting`).
struct HandOver<'a>(&'a std::sync::atomic::AtomicUsize);

impl<'a> HandOver<'a> {
    fn new(handing: &'a std::sync::atomic::AtomicUsize) -> Self {
        handing.fetch_add(1, Ordering::SeqCst);
        HandOver(handing)
    }
}

impl Drop for HandOver<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// What a stopped plan waits for to go on by itself (ADR-080, 9): the
/// mount of the server its failing step reached, whether that server was
/// seen not answering since, and how often it went on so already.
#[derive(Clone, Debug, Default)]
struct Waits {
    server: Option<PathBuf>,
    down: bool,
    tries: u32,
}

/// A plan's run: when it was asked, the entry a retry joins (its
/// `seq`), and what it touches.
struct Run {
    seq: u64,
    first: Option<u64>,
    /// The plan asked, which a retry continues: what observers are told.
    origin: PlanId,
    touched: Arc<Footprint>,
}

/// Where a step's error stopped a plan (`Executor::apply_plan`): its index,
/// what a copy it cut short left, and the mount of the server the step
/// reached, if it reached one.
struct Stop {
    at: usize,
    partial: Option<super::staging::Partial>,
    server: Option<PathBuf>,
}

/// How often the server a stopped plan waits for is asked whether it
/// answers again ([`Executor::set_server_look`] for a test's).
const LOOK_EVERY: Duration = Duration::from_secs(15);

/// An answer slower than this says the server did not answer when asked:
/// it came once the server was reached again.
const PROMPT: Duration = Duration::from_secs(2);

/// How often a plan goes on by itself when its server answers again,
/// before it waits for Retry alone.
const ON_ITS_OWN: u32 = 3;

/// The stopped plans kept for Retry, the latest: older ones are let go.
const STOPPED_KEPT: usize = 8;

impl Work {
    /// The stack its end may put an entry on: an applied plan's or a
    /// redo's inverse on the undo stack, an undo's on the redo stack.
    fn adds(&self) -> Direction {
        match self {
            Work::Execute { .. } | Work::Replay(Direction::Redo, _) => Direction::Undo,
            Work::Replay(Direction::Undo, _) => Direction::Redo,
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
    /// A plan's, as the history keeps it, worked out with it: what the
    /// history compares with the entries before it (`Executor::history`).
    kept: Option<Arc<Kept>>,
    /// An undo's or a redo's entry (its `stacked`), once known.
    target: Option<u64>,
    /// Why an undo of a plan named cannot be, said by its run, with the
    /// title of the plan it names where known.
    refused: Option<(Option<String>, String)>,
}

/// A job started, until it ends.
struct Begun {
    seq: u64,
    /// A plan's id, as its events name it.
    plan: Option<PlanId>,
    /// The entry a retry joins (`Continues::seq`).
    joins: Option<u64>,
    /// A plan's title, which Undo names while it runs.
    title: Option<String>,
    /// What it is called, an undo's or a redo's too (`Executor::waiting`).
    named: String,
    footprint: Arc<Footprint>,
    /// A plan's, as the history keeps it (`Job::kept`).
    kept: Option<Arc<Kept>>,
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
    /// Counts each change of what waits and runs, which the history's
    /// readers compare (`Executor::history_version`).
    version: u64,
}

/// Plans run at once at most (ADR-079).
pub const AT_ONCE: usize = 4;

/// The history as the History panel lists it (ADR-079, 13), newest
/// first, and the version it was read at ([`Executor::history_version`]).
#[derive(Clone, Debug, Default, Serialize)]
pub struct History {
    pub version: u64,
    pub rows: Vec<HistoryRow>,
}

/// An action of the history.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HistoryRow {
    /// Its place in the order asked: what an undo or a redo of an entry
    /// names ([`Executor::undo_entry`], [`Executor::redo_entry`]).
    pub entry: u64,
    pub title: String,
    /// When it was applied, or undone; now, for what has not ended.
    #[serde(serialize_with = "unix_seconds")]
    pub at: SystemTime,
    pub state: Done,
    /// What its Undo, an undone one's Redo, can do now.
    pub can: Can,
    /// What Undo with what came after takes with it, newest first: the
    /// entries after it that touched what it did, followed through; empty
    /// when it is taken back alone, or one of those is under way.
    pub with: Vec<u64>,
}

/// How an action of the history stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Done {
    Running,
    Waiting,
    Done,
    Undone,
}

/// What an action's Undo (an undone one's Redo) can do now.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "can")]
pub enum Can {
    /// Take it back (put it back) alone, at once.
    Now,
    /// Take it back once it ends: it runs or waits.
    OnceEnded,
    /// Not alone: the latest action that touched what it did since, by its
    /// entry and title, and how that one stands.
    After {
        entry: u64,
        title: String,
        since: Since,
    },
    /// It is being undone or redone.
    Going,
    /// Nothing it did can be taken back (deleted for good).
    Never,
}

/// How the action that blocks another's Undo stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Since {
    /// Done, asked after it: undo that first.
    Done,
    Running,
    Waiting,
    /// Undone after it: redo that first.
    Undone,
    /// Done after it, and cannot be undone (deleted for good): it took
    /// what this did for good.
    ForGood,
}

/// A time as seconds since 1970, UTC, as the commands answer times.
fn unix_seconds<S: serde::Serializer>(at: &SystemTime, s: S) -> Result<S::Ok, S::Error> {
    let secs = at
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    s.serialize_u64(secs)
}

/// A plan asked that has not ended, as the history reads it.
struct Ahead {
    seq: u64,
    /// The entry it joins, a retry.
    joins: Option<u64>,
    title: String,
    kept: Option<Arc<Kept>>,
    running: bool,
}

/// An entry, as the history reads it under the journal's lock.
struct Read {
    seq: u64,
    stacked: u64,
    title: String,
    at: SystemTime,
    undone: bool,
    /// It holds something to take back.
    holds: bool,
    blocked_by: BTreeSet<u64>,
    touched: Arc<Kept>,
}

impl Read {
    fn of(e: &JournalEntry, undone: bool) -> Read {
        Read {
            seq: e.seq,
            stacked: e.stacked,
            title: e.title.clone(),
            at: e.applied_at,
            undone,
            holds: e.plan.is_some(),
            blocked_by: e.blocked_by.clone(),
            touched: Arc::clone(&e.touched),
        }
    }
}

/// The history's rows from the entries read and what is `ahead` (asked
/// and not ended), newest first ([`Executor::history`]); `targeted`, the
/// entries undos and redos under way take, `true` for the redo stack.
fn rows_of(rows: &[Read], ahead: &[Ahead], targeted: &HashSet<(bool, u64)>) -> Vec<HistoryRow> {
    // Each entry by its place, on its stack: its title, where it stands.
    let by_seq: HashMap<(bool, u64), &Read> = rows.iter().map(|r| ((r.undone, r.seq), r)).collect();
    // A retry under way is its entry's, which runs or waits.
    let retried: HashMap<u64, bool> = ahead
        .iter()
        .filter_map(|a| Some((a.joins?, a.running)))
        .collect();
    // What runs or waits after an entry and touched what it did: the
    // latest such.
    let under_way_after = |row: &Read| {
        ahead
            .iter()
            .filter(|a| a.seq > row.seq && a.joins != Some(row.seq))
            .filter(|a| {
                a.kept
                    .as_ref()
                    .is_none_or(|kept| kept.conflicts(&row.touched))
            })
            .max_by_key(|a| a.seq)
    };
    let mut blocked_under_way: HashSet<u64> = HashSet::new();
    let mut out: Vec<HistoryRow> = Vec::with_capacity(rows.len() + ahead.len());
    for row in rows {
        let state = match (row.undone, retried.get(&row.seq)) {
            (true, _) => Done::Undone,
            (false, Some(true)) => Done::Running,
            (false, Some(false)) => Done::Waiting,
            (false, None) => Done::Done,
        };
        // One that cannot be undone first: undoing the others would not do.
        let latest = row
            .blocked_by
            .iter()
            .filter_map(|seq| by_seq.get(&(row.undone, *seq)))
            .max_by_key(|r| (!r.holds, r.stacked))
            .map(|r| (r.seq, r.title.clone(), r.holds));
        let can = if !row.holds {
            Can::Never
        } else if targeted.contains(&(row.undone, row.stacked)) {
            Can::Going
        } else if matches!(state, Done::Running | Done::Waiting) {
            Can::OnceEnded
        } else if row.undone {
            match latest {
                Some((entry, title, _)) => Can::After {
                    entry,
                    title,
                    since: Since::Undone,
                },
                None => Can::Now,
            }
        } else {
            match (under_way_after(row), latest) {
                (Some(a), _) => {
                    blocked_under_way.insert(row.seq);
                    Can::After {
                        entry: a.joins.unwrap_or(a.seq),
                        title: a.title.clone(),
                        since: if a.running {
                            Since::Running
                        } else {
                            Since::Waiting
                        },
                    }
                }
                (None, Some((entry, title, holds))) => Can::After {
                    entry,
                    title,
                    since: if holds { Since::Done } else { Since::ForGood },
                },
                (None, None) => Can::Now,
            }
        };
        out.push(HistoryRow {
            entry: row.seq,
            title: row.title.clone(),
            at: row.at,
            state,
            can,
            with: Vec::new(),
        });
    }
    // Undo with what came after: the entries that block it, followed
    // through, when none of them is blocked by what runs or waits, runs
    // again or is being undone.
    for row in &mut out {
        if !matches!(
            row.can,
            Can::After {
                since: Since::Done,
                ..
            }
        ) {
            continue;
        }
        let mut taken: BTreeSet<u64> = BTreeSet::new();
        let mut next = vec![row.entry];
        let mut clear = true;
        while let Some(seq) = next.pop() {
            if !taken.insert(seq) {
                continue;
            }
            let Some(read) = by_seq.get(&(false, seq)) else {
                continue;
            };
            if blocked_under_way.contains(&seq)
                || retried.contains_key(&seq)
                || !read.holds
                || targeted.contains(&(false, read.stacked))
            {
                clear = false;
                break;
            }
            next.extend(read.blocked_by.iter().copied());
        }
        if clear {
            taken.remove(&row.entry);
            row.with = taken.into_iter().rev().collect();
        }
    }
    // What has not ended and joins no entry, at its place.
    let entries: HashSet<u64> = out.iter().map(|r| r.entry).collect();
    for a in ahead {
        if a.joins.is_some_and(|joins| entries.contains(&joins)) {
            continue;
        }
        out.push(HistoryRow {
            entry: a.joins.unwrap_or(a.seq),
            title: a.title.clone(),
            at: SystemTime::now(),
            state: if a.running {
                Done::Running
            } else {
                Done::Waiting
            },
            can: Can::OnceEnded,
            with: Vec::new(),
        });
    }
    out.sort_by_key(|row| std::cmp::Reverse(row.entry));
    out
}

/// An action waiting its turn, and what for (ADR-079, 12).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Waiting {
    /// Its place in the order asked: what [`Executor::cancel_waiting`]
    /// takes.
    pub asked: u64,
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
    observers: Contributions<PlanObserver>,
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
    /// Stopped plans taken out to be retried and not queued yet
    /// ([`HandOver`]): from one to the other, a plan is never out of both
    /// ([`Executor::busy_or_waiting`]).
    handing: std::sync::atomic::AtomicUsize,
    /// The mounts of the servers stopped plans wait for, each asked by a
    /// thread of its own whether it answers again (`look_for`).
    looking: Mutex<std::collections::HashSet<PathBuf>>,
    /// How often they are asked ([`LOOK_EVERY`]).
    look_every: Mutex<Duration>,
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

/// The entry an undo of a plan named takes (`Executor::entry_of`).
enum Named {
    /// That plan has not ended.
    Waits,
    /// It cannot be: the title of the plan named, where known, and why.
    Refused(Option<String>, String),
    /// Its place on the undo stack, and the steps that take it back.
    Entry(u64, Vec<Box<dyn Operation>>),
}

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
    title: String,
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

/// A run told started and listed among those applied, so that Pause and
/// Cancel reach it from its first check on ([`Executor::show`]); let go
/// of before it is told finished.
struct Shown<'a> {
    pause: Arc<Pause>,
    meter: Arc<Meter>,
    /// What its checks and its steps found comparing trees, read once.
    compared: Arc<Compared>,
    _listed: Running<'a>,
}

/// Ends a started job however it leaves, a panic included: what waited
/// for it may start.
struct Ended<'a>(&'a Executor, u64);

impl Drop for Ended<'_> {
    fn drop(&mut self) {
        let idle = {
            let mut lane = self.0.lane.lock();
            lane.version += 1;
            if let Some(at) = lane.started.iter().position(|b| b.seq == self.1) {
                let begun = lane.started.remove(at);
                if let Some(target) = begun.target {
                    lane.targeted.retain(|t| *t != target);
                }
            }
            let idle = lane.started.is_empty() && lane.queue.is_empty();
            if idle {
                self.0.idle.notify_all();
            }
            idle
        };
        // Its end was told (`Finished`) while it was still among those
        // started: whoever waits for nothing to run (a program without a
        // window, ADR-115) hears it now, once, with no lock held.
        if idle {
            self.0.wake();
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
            handing: std::sync::atomic::AtomicUsize::new(0),
            looking: Mutex::default(),
            look_every: Mutex::new(LOOK_EVERY),
        })
    }

    /// Ask the server a stopped plan waits for whether it answers again
    /// every `every` ([`LOOK_EVERY`]): a test's, which cannot wait so long.
    pub fn set_server_look(&self, every: Duration) {
        *self.look_every.lock() = every;
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
        shown: &Shown<'_>,
    ) -> Settling {
        use super::clash::{Answer, settle};
        let ctx = || dry_context(&plan.steps, companions, cancel, shown);
        let said = plan.on_conflict.map(Answer::all);
        let first = settle(
            &plan.steps,
            said.as_ref().unwrap_or(&Answer::default()),
            &ctx(),
        );
        // Cancelled as it looked, what it found is not asked about.
        if cancel.is_cancelled() {
            return Settling::Cancelled;
        }
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
    fn short_of_room(
        &self,
        plan: &Plan,
        cancel: &CancelToken,
        shown: &Shown<'_>,
    ) -> Option<String> {
        let free = Arc::clone(&self.free.lock());
        let ctx = dry_context(&plan.steps, &Arc::default(), cancel, shown);
        let sized = super::room::sized(&plan.steps, &ctx, cancel, &|_| true)?;
        // What it will move, and what each copy copies, kept for its run:
        // its time left told from its start, no tree walked twice.
        shown.meter.set_ahead(sized.work.clone(), walked(&sized));
        let short = sized.short(&*free)?;
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
            said,
            ..
        } = &ev
            && let Some(reply) = self.watched.lock().remove(plan)
        {
            let _ = reply.send(Verdict::Applied {
                applied: *applied,
                skipped: *skipped,
                error: error.clone(),
                cancelled: *cancelled,
                said: said.clone(),
            });
        }
        let _ = self.tx.send(ev);
        self.wake();
    }

    /// Called with no lock held: a waker may call the executor back (pause
    /// a plan), which wakes again.
    fn wake(&self) {
        let waker = self.waker.lock().clone();
        if let Some(w) = waker {
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
        let queued = self
            .lane
            .lock()
            .queue
            .iter()
            .find(|job| matches!(&job.work, Work::Execute { plan: p, .. } if p.id == plan))
            .map(|job| job.cancel.cancel())
            .is_some();
        if queued {
            self.dispatch_elsewhere();
        }
        queued
    }

    /// Cancel the plan, undo or redo that waits its turn as `asked`
    /// ([`Waiting::asked`]): it ends at once, having done nothing, without
    /// waiting for what it waited for. A plan says it was cancelled; an
    /// undo or a redo leaves its entry where it was. Whether it was
    /// waiting (one that started meanwhile is cancelled as a running one
    /// is: [`Executor::cancel`]).
    pub fn cancel_waiting(&self, asked: u64) -> bool {
        self.cancel_waiting_all(&[asked]) == 1
    }

    /// [`Executor::cancel_waiting`] for each of `asked` (Cancel all), at
    /// once: one look at what waits, one start of what may start. How many
    /// were waiting.
    pub fn cancel_waiting_all(&self, asked: &[u64]) -> usize {
        let asked: std::collections::HashSet<u64> = asked.iter().copied().collect();
        let cancelled = self
            .lane
            .lock()
            .queue
            .iter()
            .filter(|job| asked.contains(&job.seq) && !job.cancel.is_cancelled())
            .map(|job| job.cancel.cancel())
            .count();
        if cancelled > 0 {
            self.wake();
            self.dispatch_elsewhere();
        }
        cancelled
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
                let held = active.pause.pause();
                if held {
                    active.meter.hold(true);
                }
                (held, None)
            } else {
                let released = active.pause.resume();
                if released.is_some() {
                    active.meter.hold(false);
                }
                (false, released)
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
    /// meet, or for a heavy one, to end (ADR-079). One cancelled is not:
    /// it ends at once.
    pub fn queued(&self) -> usize {
        let lane = self.lane.lock();
        lane.queue
            .iter()
            .filter(|job| !job.cancel.is_cancelled())
            .count()
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
                let read = a.meter.reading(a.pause.is_paused());
                Transfer {
                    done: read.done,
                    total: read.total,
                    ended: read.steps.ended,
                    finished: read.finished,
                    step_done: read.done.saturating_sub(read.steps.done),
                    step_total: read.total.saturating_sub(read.steps.total),
                    sending: read.sending,
                    sending_bytes: read.send_left,
                    planned: read.planned,
                    speed: read.measured.speed,
                    left: read.measured.left,
                    then_sends: read.then_sends,
                }
            })
    }

    /// The plans, undos and redos running, in the order they started: what
    /// each is, how far it went, how fast it goes and how long it has left
    /// (the `running` command).
    pub fn running(&self) -> Vec<RunningPlan> {
        let running: Vec<(PlanId, String, bool)> = self
            .running
            .lock()
            .iter()
            .map(|a| (a.plan, a.title.clone(), a.pause.is_paused()))
            .collect();
        running
            .into_iter()
            .filter_map(|(plan, title, paused)| {
                Some(RunningPlan {
                    plan,
                    title,
                    paused,
                    transfer: self.transfer(plan)?,
                })
            })
            .collect()
    }

    /// Tell `observer` what each finished plan changed, until `owner`'s
    /// observers are removed (`App::unregister_owner` does).
    pub fn observe(&self, owner: &str, observer: ChangeObserver) {
        self.observe_plans(owner, Arc::new(move |_, changes| observer(changes)));
    }

    /// Tell `observer` what each finished plan changed and which plan it
    /// was ([`PlanObserver`]), until `owner`'s observers are removed.
    pub fn observe_plans(&self, owner: &str, observer: PlanObserver) {
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
        let Some((work, handing)) = self.retrying(plan) else {
            return false;
        };
        self.submit(work, cancel);
        drop(handing);
        true
    }

    /// Retry when its turn comes, waiting for it. `None` when there is no
    /// such plan.
    pub fn retry_blocking(&self, plan: PlanId, cancel: &CancelToken) -> Option<ExecResult> {
        let (work, handing) = self.retrying(plan)?;
        let (tx, rx) = bounded(1);
        self.enqueue(work, cancel.clone(), Some(tx));
        drop(handing);
        self.dispatch();
        rx.recv().ok().flatten()
    }

    /// The work that retries `plan`, taken out of the stopped plans: asked
    /// for, its server's count of going on by itself starts again.
    fn retrying(&self, plan: PlanId) -> Option<(Work, HandOver<'_>)> {
        self.taken_to_retry(plan, false, |waits| Waits {
            server: waits.server,
            ..Waits::default()
        })
    }

    /// The work that retries `plan`, taken out of the stopped plans, what it
    /// waits for made `waits`; going on `by_itself`, or asked. Held until it
    /// is queued: meanwhile it is busy (`busy_or_waiting`).
    fn taken_to_retry(
        &self,
        plan: PlanId,
        by_itself: bool,
        waits: impl FnOnce(Waits) -> Waits,
    ) -> Option<(Work, HandOver<'_>)> {
        let mut stopped = self.stopped.lock();
        let at = stopped.iter().position(|s| s.plan == plan)?;
        let Stopped {
            seq,
            origin,
            rest,
            partial,
            waits: was,
            ..
        } = stopped.remove(at)?;
        // Counted before the lock is let go of: whoever looks finds it
        // stopped, handed over, or queued.
        let handing = HandOver::new(&self.handing);
        drop(stopped);
        let work = Work::Execute {
            plan: Arc::new(rest),
            whole: true,
            continues: Some(Box::new(Continues {
                seq,
                origin,
                partial,
                waits: waits(was),
                by_itself,
            })),
        };
        Some((work, handing))
    }

    /// Whether the plan `plan`, which a step's error stopped, waits for its
    /// server to answer again, to go on by itself (ADR-080, 9).
    pub fn waits_for_server(&self, plan: PlanId) -> bool {
        self.stopped
            .lock()
            .iter()
            .find(|s| s.plan == plan)
            .is_some_and(Stopped::goes_on_by_itself)
    }

    /// While a plan its server stopped waits (`Stopped::waits`), ask that
    /// server whether it answers, in its lane, one question at a time: at
    /// once, then every [`LOOK_EVERY`] (`answered`). Ends once nothing
    /// waits for it, no more is taken, or the executor is gone.
    fn look_for(&self, point: &Path) {
        if !self.looking.lock().insert(point.to_path_buf()) {
            return;
        }
        let me = self.me.clone();
        let looked = point.to_path_buf();
        let started = std::thread::Builder::new()
            .name("skwir-plans-look".into())
            .spawn(move || {
                let point = looked;
                let mut first = true;
                loop {
                    let Some(this) = me.upgrade() else { return };
                    if !first {
                        let every = *this.look_every.lock();
                        drop(this);
                        std::thread::sleep(every);
                    }
                    first = false;
                    let Some(this) = me.upgrade() else { return };
                    let closed = this.lane.lock().closed;
                    {
                        // Under the lock a stopped plan is kept by: one kept
                        // after finds the looking ended, and starts it again.
                        let stopped = this.stopped.lock();
                        if closed || !stopped.iter().any(|s| s.waits_for(&point)) {
                            this.looking.lock().remove(&point);
                            return;
                        }
                    }
                    let (tx, rx) = bounded(1);
                    let asked = point.clone();
                    this.scheduler.spawn_in(
                        crate::fs::reach::lane(&point),
                        Priority::Background,
                        "op.answers",
                        CancelToken::new(),
                        move || {
                            let began = Instant::now();
                            let answers = crate::fs::reach::answers(&asked);
                            let _ = tx.send((answers, began.elapsed()));
                        },
                    );
                    drop(this);
                    // Held by its server, the question waits: the only one.
                    let Ok((answers, took)) = rx.recv() else {
                        if let Some(this) = me.upgrade() {
                            this.looking.lock().remove(&point);
                        }
                        return;
                    };
                    let Some(this) = me.upgrade() else { return };
                    this.answered(&point, answers, took >= PROMPT);
                }
            });
        if started.is_err() {
            self.looking.lock().remove(point);
        }
    }

    /// What the server at `point` answered: nothing, or only `slow`ly, the
    /// plans that wait for it saw it gone; answering, those go on by
    /// themselves, and those that never saw it gone, answered at once,
    /// were stopped by something else: they wait for Retry alone.
    fn answered(&self, point: &Path, answers: bool, slow: bool) {
        let (mut down, mut going) = (Vec::new(), Vec::new());
        {
            let mut stopped = self.stopped.lock();
            for s in stopped.iter_mut().filter(|s| s.waits_for(point)) {
                if (!answers || slow) && !s.waits.down {
                    s.waits.down = true;
                    down.push(s.plan);
                }
                if answers {
                    if s.waits.down {
                        going.push((s.plan, s.rest.id, s.rest.title.clone()));
                    } else {
                        s.waits.server = None;
                    }
                }
            }
        }
        for plan in down {
            self.emit(ExecEvent::WaitsForServer { plan });
        }
        for (plan, next, title) in going {
            let Some((work, handing)) = self.taken_to_retry(plan, true, |waits| Waits {
                down: false,
                tries: waits.tries + 1,
                ..waits
            }) else {
                continue;
            };
            tracing::info!(%title, "its server answers again: it goes on");
            self.emit(ExecEvent::GoesOn { plan, next, title });
            self.submit(work, CancelToken::new());
            drop(handing);
        }
    }

    /// Undo the last plan asked (ADR-079): once it ends where it runs,
    /// beside what it shares nothing with. Returns false if there is
    /// nothing to undo and nothing that could add to the journal is queued
    /// or running.
    pub fn undo(self: &Arc<Self>, cancel: CancelToken) -> bool {
        let available = self.is_busy() || self.can_undo();
        if available {
            self.submit(Work::Replay(Direction::Undo, None), cancel);
        }
        available
    }

    /// Undo when its turn comes (module documentation), waiting for it.
    /// Returns `None` if nothing to undo.
    pub fn undo_blocking(&self, cancel: &CancelToken) -> Option<ExecResult> {
        self.submit_blocking(Work::Replay(Direction::Undo, None), cancel)
    }

    /// Undo the plan `plan`, as its events named it (a toast's Undo, of
    /// the action it reports), rather than the last asked: when no plan
    /// asked after it, its entry still in the history or under way,
    /// touched what it did, since those share nothing and their inverses
    /// give the same result in either order (ADR-079, 8); otherwise it
    /// does nothing and says why, as an undo's error. A plan that has not
    /// ended is waited for, as Undo does.
    pub fn undo_of(self: &Arc<Self>, plan: PlanId, cancel: CancelToken) {
        self.submit(Work::Replay(Direction::Undo, Some(Of::Plan(plan))), cancel);
    }

    /// [`Executor::undo_of`] when its turn comes, waiting for it. `None`
    /// when cancelled before it began.
    pub fn undo_of_blocking(&self, plan: PlanId, cancel: &CancelToken) -> Option<ExecResult> {
        self.submit_blocking(Work::Replay(Direction::Undo, Some(Of::Plan(plan))), cancel)
    }

    /// Undo the entry at `entry` in the history (the History panel's, its
    /// place in the order asked, ADR-079, 13), as [`Executor::undo_of`]
    /// takes a plan's: alone when nothing asked after it touched what it
    /// did, else refused, saying what did; an action that has not ended
    /// waited for.
    pub fn undo_entry(self: &Arc<Self>, entry: u64, cancel: CancelToken) {
        self.submit(
            Work::Replay(Direction::Undo, Some(Of::Entry(entry))),
            cancel,
        );
    }

    /// [`Executor::undo_entry`] when its turn comes, waiting for it.
    pub fn undo_entry_blocking(&self, entry: u64, cancel: &CancelToken) -> Option<ExecResult> {
        self.submit_blocking(
            Work::Replay(Direction::Undo, Some(Of::Entry(entry))),
            cancel,
        )
    }

    /// Redo the undone entry at `entry` (ADR-079, 13): alone when nothing
    /// undone after it touched what it did, else refused, saying what did.
    pub fn redo_entry(self: &Arc<Self>, entry: u64, cancel: CancelToken) {
        self.submit(
            Work::Replay(Direction::Redo, Some(Of::Entry(entry))),
            cancel,
        );
    }

    /// [`Executor::redo_entry`] when its turn comes, waiting for it.
    pub fn redo_entry_blocking(&self, entry: u64, cancel: &CancelToken) -> Option<ExecResult> {
        self.submit_blocking(
            Work::Replay(Direction::Redo, Some(Of::Entry(entry))),
            cancel,
        )
    }

    /// The entries an undo of `entry` with what came after takes, newest
    /// first, `entry` last: it and the entries after it that touched what
    /// it did, followed through (`JournalEntry::blocked_by`); just `entry`
    /// when it is not in the history to undo, or one of those cannot be
    /// undone (deleted for good): its undo then says why.
    pub fn with_after(&self, entry: u64) -> Vec<u64> {
        let journal = self.journal.lock();
        let by_seq: HashMap<u64, &JournalEntry> = journal.undo.iter().map(|e| (e.seq, e)).collect();
        let mut taken: BTreeSet<u64> = BTreeSet::new();
        let mut next = vec![entry];
        while let Some(seq) = next.pop() {
            let Some(found) = by_seq.get(&seq) else {
                continue;
            };
            // Not through one that cannot be undone: its own undo says why.
            if found.plan.is_none() {
                return vec![entry];
            }
            if taken.insert(seq) {
                next.extend(found.blocked_by.iter().copied());
            }
        }
        if taken.is_empty() {
            return vec![entry];
        }
        // On the undo stack an entry's place is its `seq`: newest first.
        taken.into_iter().rev().collect()
    }

    /// Undo `entry` with the entries after it that touched what it did,
    /// followed through, newest first ([`Executor::with_after`]), each
    /// when the one before it ended (`entry_of`). How many were asked.
    pub fn undo_with_after(self: &Arc<Self>, entry: u64, cancel: &CancelToken) -> usize {
        let entries = self.with_after(entry);
        for &seq in &entries {
            self.undo_entry(seq, cancel.child());
        }
        entries.len()
    }

    /// The history as the History panel lists it (ADR-079, 13): every
    /// entry of either stack, and the plans asked that have not ended,
    /// newest first, each with what its Undo (an undone one's Redo) can
    /// do now. Read from what each entry keeps (`JournalEntry::blocked_by`)
    /// and, for what runs or waits, compared with each entry asked before
    /// it out of every lock: a reader keeps it while
    /// [`Executor::history_version`] stays.
    pub fn history(&self) -> History {
        let (lane_version, ahead, targeted) = {
            let lane = self.lane.lock();
            let started = lane.started.iter().filter_map(|b| {
                Some(Ahead {
                    seq: b.seq,
                    joins: b.joins,
                    title: b.title.clone()?,
                    kept: b.kept.clone(),
                    running: true,
                })
            });
            let queued = lane.queue.iter().filter_map(|job| match &job.work {
                Work::Execute {
                    plan, continues, ..
                } if !job.cancel.is_cancelled() => Some(Ahead {
                    seq: job.seq,
                    joins: continues.as_ref().map(|c| c.seq),
                    title: plan.title.clone(),
                    kept: job.kept.clone(),
                    running: false,
                }),
                _ => None,
            });
            let ahead: Vec<Ahead> = started.chain(queued).collect();
            let targeted: HashSet<(bool, u64)> = lane
                .targeted
                .iter()
                .map(|&(d, stacked)| (d == Direction::Redo, stacked))
                .collect();
            (lane.version, ahead, targeted)
        };
        let (journal_version, rows) = {
            let journal = self.journal.lock();
            let rows: Vec<Read> = journal
                .undo
                .iter()
                .map(|e| Read::of(e, false))
                .chain(journal.redo.iter().map(|e| Read::of(e, true)))
                .collect();
            (journal.version, rows)
        };
        let out = rows_of(&rows, &ahead, &targeted);
        History {
            version: lane_version + journal_version,
            rows: out,
        }
    }

    /// Moves whenever [`Executor::history`] may answer otherwise: an entry
    /// put, taken back or let go of, or a plan asked, started or ended.
    pub fn history_version(&self) -> u64 {
        let lane = self.lane.lock().version;
        lane + self.journal.lock().version
    }

    /// What `touched` meets on `direction`'s stack, each entry but the one
    /// at `skip` compared, out of the journal's lock: the entry then joins
    /// under it without comparing there what was compared here (`Met`).
    fn meets_on(&self, direction: Direction, touched: &Arc<Kept>, skip: Option<u64>) -> Met {
        let entries: Vec<(u64, Arc<Kept>)> = {
            let journal = self.journal.lock();
            journal
                .stack_of(direction)
                .iter()
                .filter(|e| Some(e.stacked) != skip)
                .map(|e| (e.seq, Arc::clone(&e.touched)))
                .collect()
        };
        let entries = entries
            .into_iter()
            .map(|(seq, kept)| {
                let meets = kept.conflicts(touched);
                (seq, (kept, meets))
            })
            .collect();
        Met {
            with: Some(Arc::clone(touched)),
            entries,
        }
    }

    /// Redo the most recently undone plan when its turn comes (module
    /// documentation).
    pub fn redo(self: &Arc<Self>, cancel: CancelToken) -> bool {
        let available = self.is_busy() || self.can_redo();
        if available {
            self.submit(Work::Replay(Direction::Redo, None), cancel);
        }
        available
    }

    /// Redo when its turn comes, waiting for it.
    pub fn redo_blocking(&self, cancel: &CancelToken) -> Option<ExecResult> {
        self.submit_blocking(Work::Replay(Direction::Redo, None), cancel)
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
                Work::Replay(..) => None,
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
            // Cancelled, it ends at once: it waits for nothing.
            if job.cancel.is_cancelled() {
                continue;
            }
            let waits_for = match job.footprint.as_deref() {
                None => match &job.work {
                    // An undo or a redo waits for the plan asked before it
                    // that may put its entry there.
                    Work::Replay(..) => lane
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
                asked: job.seq,
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
            Work::Replay(direction, _) => *direction,
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

    /// The actions under way that touch something at or below `tree`, by
    /// what they are called: what letting go of a mount there would cut
    /// (Eject). Those running, and those waiting their turn (for the
    /// server, the disk, a slot), their footprints worked out; a plan not
    /// measured yet by the paths its steps name. One cancelled is not: what
    /// it still waits for there, it gave up on.
    pub fn under_way_in(&self, tree: &Path) -> Vec<String> {
        let lane = self.lane.lock();
        let running = lane
            .started
            .iter()
            .filter(|begun| !begun.cancel.is_cancelled() && begun.footprint.touches(tree))
            .map(|begun| begun.named.clone());
        let waiting = lane
            .queue
            .iter()
            .filter(|job| !job.cancel.is_cancelled())
            .filter(|job| match (&job.footprint, &job.work) {
                (Some(footprint), _) => footprint.touches(tree),
                (None, Work::Execute { plan, .. }) => plan.steps.iter().any(|step| {
                    let change = step.describe();
                    change.paths().any(|path| path.starts_with(tree))
                }),
                (None, Work::Replay(..)) => false,
            })
            .map(|job| self.named(job));
        running.chain(waiting).collect()
    }

    /// Whether a plan, an undo or a redo runs or waits its turn, a stopped
    /// one being handed over to be retried included.
    pub fn is_busy(&self) -> bool {
        let lane = self.lane.lock();
        !lane.started.is_empty()
            || !lane.queue.is_empty()
            || self.handing.load(Ordering::SeqCst) > 0
    }

    /// Whether a plan, an undo or a redo runs or waits its turn, or a plan
    /// a step's error stopped waits for its server to answer, to go on by
    /// itself (ADR-080, 9): what a program without a window waits for
    /// before it ends (ADR-115). Asked at once, under the lock the stopped
    /// plans are kept by: one going on by itself, taken out of them and
    /// queued, is found in one or the other, never in neither, as separate
    /// questions ([`Executor::is_busy`], [`Executor::waits_for_server`])
    /// may find it.
    pub fn busy_or_waiting(&self) -> bool {
        let stopped = self.stopped.lock();
        if stopped.iter().any(Stopped::goes_on_by_itself) {
            return true;
        }
        // Taken out under that lock and counted, it stays counted until
        // it is queued.
        let handing = self.handing.load(Ordering::SeqCst) > 0;
        let lane = self.lane.lock();
        handing || !lane.started.is_empty() || !lane.queue.is_empty()
    }

    fn submit(&self, work: Work, cancel: CancelToken) {
        self.enqueue(work, cancel, None);
        // Behind what it waits for, whoever shows the plans says it waits.
        self.wake();
        self.dispatch_elsewhere();
    }

    /// Work out what the queued jobs touch and start what may start, on a
    /// thread of its own: working out what a job touches may read (the
    /// mount table, whether what is deleted is a folder), never on the
    /// caller's thread, the window's.
    fn dispatch_elsewhere(&self) {
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
        if let Work::Execute { continues, .. } = &work
            && !continues.as_ref().is_some_and(|c| c.by_itself)
        {
            // A plan asked after an undo drops that undo's redo, even one
            // that ends after it (ADR-079, 10); one going on by itself was
            // asked before, and drops nothing (ADR-080, 9).
            let mut journal = self.journal.lock();
            journal.redo_floor = seq;
            journal.clear_redo();
        }
        lane.version += 1;
        lane.queue.push_back(Job {
            seq,
            work,
            cancel,
            reply,
            footprint: None,
            kept: None,
            target: None,
            refused: None,
        });
    }

    /// Work out what the queued jobs touch, in the order asked, and start
    /// each that may start (module documentation): whenever a job is asked
    /// or ends. One caller at a time works out footprints, which may read
    /// (a move's volumes, whether what is deleted is a folder), never
    /// under the lock; the others start what may start meanwhile.
    fn dispatch(&self) {
        while let Some((seq, steps, plan)) = self.next_to_measure() {
            // A step that panics saying what it touches cannot say: its
            // plan runs alone, and the next is measured.
            let companions = Arc::clone(&self.companions.lock());
            let footprint = super::guarded(|| {
                let volumes = super::footprint::Volumes::now().with_companions(companions);
                Ok(
                    Footprint::of_steps(steps.iter().map(|s| s.footprint(&volumes)))
                        .canonical()
                        .with_channels(&volumes),
                )
            })
            .unwrap_or_else(|_| Footprint::every());
            // What the history compares a plan with, worked out once.
            let kept = plan.then(|| Arc::new(Kept::new(&footprint)));
            let mut lane = self.lane.lock();
            lane.measuring = false;
            lane.version += 1;
            if let Some(job) = lane.queue.iter_mut().find(|j| j.seq == seq) {
                job.kept = kept;
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
    /// touches nothing. An undo of a plan named takes that plan's entry
    /// ([`Executor::undo_of`]).
    fn next_to_measure(&self) -> Option<(u64, Vec<Box<dyn Operation>>, bool)> {
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
                    return Some((seq, plan.steps.clone(), true));
                }
                Work::Replay(direction, Some(of)) => {
                    let (direction, of) = (*direction, *of);
                    match self.entry_of(lane, i, direction, of) {
                        // What it names has not ended, or what may
                        // change whether it can be has not: it waits.
                        Named::Waits => {}
                        Named::Refused(title, reason) => {
                            let job = &mut lane.queue[i];
                            job.refused = Some((title, reason));
                            job.footprint = Some(Arc::new(Footprint::default()));
                            lane.version += 1;
                        }
                        Named::Entry(stacked, steps) => {
                            lane.targeted.push((direction, stacked));
                            lane.queue[i].target = Some(stacked);
                            lane.measuring = true;
                            return Some((seq, steps, false));
                        }
                    }
                    continue;
                }
                Work::Replay(direction, _) => *direction,
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
                    return Some((seq, steps, false));
                }
                (None, _) => {
                    drop(journal);
                    lane.queue[i].footprint = Some(Arc::new(Footprint::default()));
                }
            }
        }
        None
    }

    /// The entry the undo or the redo queued at `i` takes, as `of` names
    /// it, and its steps (ADR-079, 9 and 13). It waits while what may
    /// change the answer has not ended: for an undo, the plan that makes
    /// its entry (as Undo waits, ADR-079, 9) and a redo asked before it,
    /// which may put back an entry asked after this one; for a redo, an
    /// undo asked before it, which may put one undone after it; for
    /// either, one of its kind asked before it and not yet measured, or
    /// one taking an entry after it on its stack (Undo with what came
    /// after, newest first: whether that one still blocks this is decided
    /// once it ended). Then it takes its entry unless an entry after it on
    /// its stack touched what it did (`JournalEntry::blocked_by`), or, for
    /// an undo, a plan asked after it, running or waiting, did
    /// (`Footprint::conflicts`): those taken back first, in the order
    /// asked, put back each state; this one alone may not. Then, or with
    /// no such entry, what it says.
    fn entry_of(&self, lane: &Lane, i: usize, direction: Direction, of: Of) -> Named {
        let journal = self.journal.lock();
        let (stack, other) = match direction {
            Direction::Undo => (&journal.undo, &journal.redo),
            Direction::Redo => (&journal.redo, &journal.undo),
        };
        let entry = stack.iter().find(|e| of.names(e));
        let at = entry.map(|e| e.stacked);
        let before = || lane.queue.iter().take(i);
        let waits = match direction {
            Direction::Undo => {
                // The plan that makes its entry, a retry joining it too.
                let unended = |plan: Option<PlanId>, seq: u64, joins: Option<u64>| match of {
                    Of::Plan(of) => {
                        plan == Some(of) || joins.is_some_and(|joins| Some(joins) == at)
                    }
                    Of::Entry(entry) => (plan.is_some() && seq == entry) || joins == Some(entry),
                };
                let queued = before().any(|job| match &job.work {
                    Work::Execute {
                        plan, continues, ..
                    } => unended(Some(plan.id), job.seq, continues.as_ref().map(|c| c.seq)),
                    Work::Replay(..) => false,
                });
                // A redo asked before it may put back an entry asked after
                // this one, which then must not have touched what it did:
                // decided once the redo ended.
                let redoing = before()
                    .any(|job| matches!(job.work, Work::Replay(Direction::Redo, _)))
                    || lane
                        .started
                        .iter()
                        .any(|b| b.plan.is_none() && b.adds == Direction::Undo);
                queued || redoing || lane.started.iter().any(|b| unended(b.plan, b.seq, b.joins))
            }
            // An undo asked before it may put on the redo stack an entry
            // undone after this one.
            Direction::Redo => {
                before().any(|job| matches!(job.work, Work::Replay(Direction::Undo, _)))
                    || lane
                        .started
                        .iter()
                        .any(|b| b.plan.is_none() && b.adds == Direction::Redo)
            }
        };
        let alike = |work: &Work| matches!(work, Work::Replay(d, _) if *d == direction);
        let unmeasured = before().any(|job| alike(&job.work) && job.footprint.is_none());
        let later_going = at.is_some_and(|at| {
            before()
                .filter(|job| alike(&job.work))
                .filter_map(|job| job.target)
                .chain(
                    lane.started
                        .iter()
                        .filter_map(|b| b.target.filter(|(d, _)| *d == direction))
                        .map(|(_, stacked)| stacked),
                )
                .any(|stacked| stacked > at)
        });
        if waits || unmeasured || later_going {
            return Named::Waits;
        }
        let (verb, done, after) = match direction {
            Direction::Undo => ("undo", "undone", "asked after it"),
            Direction::Redo => ("redo", "redone", "undone after it"),
        };
        let Some(entry) = entry else {
            return match other.iter().find(|e| of.names(e)) {
                Some(found) => {
                    let reason = match direction {
                        Direction::Undo => format!("“{}” is undone already", found.title),
                        Direction::Redo => {
                            format!("“{}” is not undone: nothing to redo", found.title)
                        }
                    };
                    Named::Refused(Some(found.title.clone()), reason)
                }
                None => Named::Refused(
                    None,
                    format!("that action is no longer in the history: nothing to {verb}"),
                ),
            };
        };
        let refused = |reason: String| Named::Refused(Some(entry.title.clone()), reason);
        if lane.targeted.contains(&(direction, entry.stacked)) {
            return refused(format!("“{}” is being {done} already", entry.title));
        }
        let alone = |other: &str, how: &str| {
            format!(
                "“{}” cannot be {done} alone: “{other}”, {how}, touched what it did; {verb} that first",
                entry.title
            )
        };
        // One that cannot be undone says it first: undoing the others
        // would not do.
        if let Some(later) = stack
            .iter()
            .filter(|e| entry.blocked_by.contains(&e.seq))
            .max_by_key(|e| (e.plan.is_none(), e.stacked))
        {
            if later.plan.is_none() {
                return refused(format!(
                    "“{}” cannot be {done}: “{}”, {after}, touched what it did and cannot be undone",
                    entry.title, later.title
                ));
            }
            return refused(alone(&later.title, after));
        }
        if direction == Direction::Undo {
            // Compared as the history compares them (`Executor::history`),
            // bounded: what it shows held is held.
            let meets = |kept: Option<&Arc<Kept>>, footprint: &Footprint| match kept {
                Some(kept) => kept.conflicts(&entry.touched),
                None => footprint.conflicts(entry.touched.footprint()),
            };
            let under_way = lane
                .started
                .iter()
                .filter(|b| b.seq > entry.seq && b.title.is_some())
                .find(|b| meets(b.kept.as_ref(), &b.footprint))
                .map(|b| (b.named.as_str(), "running"))
                .or_else(|| {
                    before().find_map(|job| match &job.work {
                        Work::Execute { plan, .. }
                            if job.seq > entry.seq
                                && job
                                    .footprint
                                    .as_deref()
                                    .is_some_and(|f| meets(job.kept.as_ref(), f)) =>
                        {
                            Some((plan.title.as_str(), "waiting"))
                        }
                        _ => None,
                    })
                });
            if let Some((title, how)) = under_way {
                return refused(alone(title, &format!("asked after it, {how}")));
            }
        }
        let steps = entry
            .plan
            .as_ref()
            .map(|p| p.steps.clone())
            .unwrap_or_default();
        Named::Entry(entry.stacked, steps)
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
            let (direction, plan, joins) = match &job.work {
                Work::Replay(direction, _) => (Some(*direction), None, None),
                Work::Execute {
                    plan, continues, ..
                } => (None, Some(plan), continues.as_ref().map(|c| c.seq)),
            };
            let named = self.named(&job);
            lane.version += 1;
            lane.started.push(Begun {
                seq: job.seq,
                plan: plan.map(|p| p.id),
                joins,
                title: plan.map(|p| p.title.clone()),
                named,
                footprint: job.footprint.clone().unwrap_or_default(),
                kept: job.kept.clone(),
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
        // What it touches, which its entry keeps.
        let touched = job.footprint.clone().unwrap_or_default();
        // A cancelled plan is not checked whole: it stops at once.
        let result = match job.work {
            Work::Execute {
                plan,
                whole,
                continues,
            } => {
                let plan = Arc::try_unwrap(plan).unwrap_or_else(|plan| (*plan).clone());
                Some(self.execute_now(plan, whole, continues, &job.cancel, job.seq, touched))
            }
            Work::Replay(direction, _) => self.replay(
                direction,
                job.target,
                job.refused,
                job.seq,
                &touched,
                &job.cancel,
            ),
        };
        if let Some(reply) = job.reply {
            let _ = reply.send(result);
        }
    }

    /// Apply `plan`, asked as `seq`, which `touched` what it says:
    /// unfolded, its names taken settled, and `whole`, checked first; a
    /// retry, joining what it `continues`.
    fn execute_now(
        &self,
        plan: Plan,
        whole: bool,
        continues: Option<Box<Continues>>,
        cancel: &CancelToken,
        seq: u64,
        touched: Arc<Footprint>,
    ) -> ExecResult {
        let whole = whole && !cancel.is_cancelled();
        let (first, origin, partial, waits) = continues
            .map_or((None, plan.id, None, Waits::default()), |c| {
                (Some(c.seq), c.origin, c.partial, c.waits)
            });
        let companions = Arc::clone(&self.companions.lock());
        // Shown from here, as asked, Pause and Cancel reaching it: its
        // checks may read whole trees (a paste onto what is there whole).
        let shown = self.show(&plan, cancel, RunKind::Apply);
        let checking =
            || StepContext::new(cancel.clone(), Arc::clone(&shown.pause), Arc::default());
        // Each step as it will be done, before anything is settled or
        // checked.
        let plan = if cancel.is_cancelled() {
            plan
        } else {
            plan.unfold(&checking().with_companions(Arc::clone(&companions)))
        };
        // Names taken are settled first: what the dry run checks is what
        // will be done. A step that panics as it is asked what it makes
        // refuses its plan, as a conflict does: before anything applies.
        let settled = super::guarded(|| {
            Ok(
                if whole && super::clash::takes_names(&plan.steps, &checking()) {
                    self.settle(&plan, &companions, cancel, &shown)
                } else {
                    Settling::Steps(None)
                },
            )
        })
        .unwrap_or_else(|e| Settling::Refused(super::said(&e)));
        // A retry that applies nothing (its server not answering yet, the
        // question of a name taken cancelled) stays to be retried, under
        // the id it is told by now, with what it would go on from.
        let kept = |plan: &Plan, partial| {
            if let Some(first) = first {
                self.stash(plan.id, first, origin, plan.clone(), partial, waits.clone());
            }
        };
        match settled {
            Settling::Refused(reason) => {
                kept(&plan, partial);
                self.refuse(&plan, RunKind::Apply, reason, Some(shown))
            }
            Settling::Cancelled => {
                kept(&plan, partial);
                self.cancelled(&plan, shown)
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
                        super::guarded(|| {
                            Ok(first_conflict(&plan, &companions, cancel, &shown)
                                .or_else(|| self.short_of_room(&plan, cancel, &shown)))
                        })
                        .unwrap_or_else(|e| Some(super::said(&e)))
                    })
                    .flatten();
                // Cancelled in its checks, it applies nothing, and says so.
                if whole && cancel.is_cancelled() {
                    kept(&plan, partial);
                    return self.cancelled(&plan, shown);
                }
                if let Some(reason) = refused {
                    kept(&plan, partial);
                    return self.refuse(&plan, RunKind::Apply, reason, Some(shown));
                }
                let run = Run {
                    seq,
                    first,
                    origin,
                    touched,
                };
                self.run_execute(&plan, cancel, &run, partial, &waits, shown)
            }
        }
    }

    /// A plan that will not be applied at all, or taken back: say so with
    /// the usual events, told started unless it was `shown` already.
    fn refuse(
        &self,
        plan: &Plan,
        run: RunKind,
        error: String,
        shown: Option<Shown<'_>>,
    ) -> ExecResult {
        if shown.is_none() {
            self.emit(ExecEvent::Started {
                plan: plan.id,
                title: plan.title.clone(),
                total: plan.steps.len(),
                run,
                items: None,
            });
        }
        drop(shown);
        self.emit(ExecEvent::Finished {
            plan: plan.id,
            applied: 0,
            skipped: 0,
            error: Some(error.clone()),
            cancelled: false,
            undoable: false,
            items: plan.items.as_deref().cloned(),
            said: Vec::new(),
            moved: 0,
            ran: Duration::ZERO,
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

    /// A plan the user cancelled before it applied anything (the names it
    /// found taken, unanswered): told finished, cancelled.
    fn cancelled(&self, plan: &Plan, shown: Shown<'_>) -> ExecResult {
        drop(shown);
        self.emit(ExecEvent::Finished {
            plan: plan.id,
            applied: 0,
            skipped: 0,
            error: None,
            cancelled: true,
            undoable: false,
            items: None,
            said: Vec::new(),
            moved: 0,
            ran: Duration::ZERO,
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

    /// Apply `plan` and journal it, as `run` says; a retry going on from
    /// `partial`.
    fn run_execute(
        &self,
        plan: &Plan,
        cancel: &CancelToken,
        run: &Run,
        partial: Option<super::staging::Partial>,
        waits: &Waits,
        shown: Shown<'_>,
    ) -> ExecResult {
        let Run {
            seq,
            first,
            origin,
            ref touched,
        } = *run;
        // In its place in the order asked: plans that ran beside it
        // touched nothing it did (ADR-079). A retry in the place of what it
        // continues, one action, unless a plan asked since put an entry
        // after it, which may have touched what it then did: undoing that
        // first, in the order asked, puts back each state.
        let place_in = |j: &Journal| match first {
            Some(first) if !j.undo.iter().any(|e| e.stacked > first && e.stacked < seq) => first,
            _ => seq,
        };
        // What the entry holds then: this run's footprint, with the one of
        // the entry it joins.
        let joined = |earlier: Option<&Arc<Kept>>| match earlier {
            Some(earlier) => {
                let mut both = earlier.footprint().clone();
                both.merge((**touched).clone());
                Arc::new(Kept::new(&both))
            }
            None => Arc::new(Kept::new(touched)),
        };
        self.apply_plan(plan, origin, cancel, shown, partial, |r, inverse, stop| {
            let place = if r.applied > 0 {
                // What it meets in the history, worked out before the
                // journal is locked, and looked at again under it.
                let (place, earlier) = {
                    let j = self.journal.lock();
                    let place = place_in(&j);
                    let earlier = j.undo.iter().find(|e| e.stacked == place);
                    (place, earlier.map(|e| Arc::clone(&e.touched)))
                };
                let whole = joined(earlier.as_ref());
                let met = self.meets_on(Direction::Undo, &whole, Some(place));
                let mut j = self.journal.lock();
                let place = place_in(&j);
                match j.take(Direction::Undo, place) {
                    Some(mut entry) => {
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
                        entry.asked.push(plan.id);
                        entry.touched = match &earlier {
                            Some(earlier) if Arc::ptr_eq(earlier, &entry.touched) => whole,
                            _ => joined(Some(&entry.touched)),
                        };
                        j.put(Direction::Undo, entry, &met);
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
                            asked: vec![plan.id],
                            touched: if earlier.is_none() {
                                whole
                            } else {
                                joined(None)
                            },
                            blocked_by: BTreeSet::new(),
                        },
                        &met,
                    ),
                }
                j.bound();
                place
            } else {
                place_in(&self.journal.lock())
            };
            // Stopped by a step's error, what it had left waits for Retry;
            // what stopped it otherwise (Cancel, a write-out) is not.
            if let Some(Stop {
                at,
                partial,
                server,
            }) = stop
            {
                let mut rest = Plan::with_steps(plan.title.clone(), plan.steps[at..].to_vec());
                rest.on_conflict = plan.on_conflict;
                rest.items.clone_from(&plan.items);
                // Stopped on the same server again, it counts on.
                let tries = if server.is_some() && server == waits.server {
                    waits.tries
                } else {
                    0
                };
                let waits = Waits {
                    server,
                    down: false,
                    tries,
                };
                self.stash(plan.id, place, origin, rest, partial, waits);
            }
        })
    }

    /// Keep `rest`, what the plan `plan` had left when it stopped, for
    /// Retry, its entry at `seq`, continuing the plan asked `origin`, with
    /// what it left of a file cut short;
    /// the oldest let go past [`STOPPED_KEPT`] (what it left removed).
    /// Waiting for its server (`waits`), it is asked whether it answers
    /// again (`look_for`).
    fn stash(
        &self,
        plan: PlanId,
        seq: u64,
        origin: PlanId,
        rest: Plan,
        partial: Option<super::staging::Partial>,
        waits: Waits,
    ) {
        let server = waits.server.clone().filter(|_| waits.tries < ON_ITS_OWN);
        let let_go = {
            let mut stopped = self.stopped.lock();
            stopped.push_back(Stopped {
                plan,
                seq,
                origin,
                rest,
                partial,
                waits,
            });
            let over = stopped.len().saturating_sub(STOPPED_KEPT);
            stopped.drain(..over).collect::<Vec<_>>()
        };
        // Removed out of the lock: on a remote, a removal may wait.
        drop(let_go);
        if let Some(server) = server {
            self.look_for(&server);
        }
    }

    /// What the plan whose entry is at `seq` had left when a step's error
    /// stopped it goes, the action being undone: it never goes on by
    /// itself once its server answers, nor is retried (what it left of a
    /// file cut short removed, out of the lock).
    fn let_go_of_stopped(&self, seq: u64) {
        let gone: VecDeque<Stopped> = {
            let mut stopped = self.stopped.lock();
            let (gone, kept) = std::mem::take(&mut *stopped)
                .into_iter()
                .partition(|s| s.seq == seq);
            *stopped = kept;
            gone
        };
        drop(gone);
    }

    /// Take back the entry `target` of one stack and journal what takes
    /// that back on the other, the run `touched` what it says. A run that
    /// stops partway stays on its stack to be retried; its steps are
    /// idempotent. An undo of a plan named that cannot be says why it is
    /// `refused`.
    fn replay(
        &self,
        direction: Direction,
        target: Option<u64>,
        refused: Option<(Option<String>, String)>,
        seq: u64,
        touched: &Footprint,
        cancel: &CancelToken,
    ) -> Option<ExecResult> {
        if cancel.is_cancelled() {
            return None;
        }
        if let Some((title, reason)) = refused {
            let (title, run) = match direction {
                Direction::Undo => (
                    title.map_or_else(|| "undo".to_owned(), |t| format!("undo {t}")),
                    RunKind::Undo,
                ),
                Direction::Redo => (title.unwrap_or_else(|| "redo".to_owned()), RunKind::Redo),
            };
            return Some(self.refuse(&Plan::new(title), run, reason, None));
        }
        let mut entry = self.journal.lock().take(direction, target?)?;
        if direction == Direction::Undo {
            self.let_go_of_stopped(entry.seq);
        }
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
                items: None,
                said: Vec::new(),
                moved: 0,
                ran: Duration::ZERO,
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
        let id = run.id;
        let shown = self.show(&run, cancel, direction.into());
        let r = self.apply_plan(&run, id, cancel, shown, None, |r, back, _| {
            // This run's inverses come first: its steps were applied after
            // the ones an earlier, interrupted attempt already took back.
            let mut steps = back.map(|p| p.steps).unwrap_or_default();
            steps.append(&mut entry.partial);
            let went = r.error.is_none() && !r.cancelled;
            // What the entry it leaves on the other stack meets there,
            // worked out before the journal is locked.
            let left = Arc::new(Kept::new(touched));
            let met = if went {
                self.meets_on(direction.other(), &left, None)
            } else {
                Met::default()
            };
            let mut j = self.journal.lock();
            if went {
                // Undone, it is redone from the redo stack, unless a plan
                // was asked after this undo; redone, it takes its place in
                // the history again.
                let (other, stacked) = match direction {
                    Direction::Undo => (Direction::Redo, seq),
                    Direction::Redo => (Direction::Undo, entry.seq),
                };
                let kept = direction == Direction::Redo || seq > j.redo_floor;
                if !steps.is_empty() && kept {
                    let mut asked = entry.asked;
                    asked.push(id);
                    let other_entry = JournalEntry {
                        plan: Some(Plan::with_steps(entry.title.clone(), steps)),
                        title: entry.title,
                        applied_at: SystemTime::now(),
                        partial: Vec::new(),
                        seq: entry.seq,
                        stacked,
                        asked,
                        touched: left,
                        blocked_by: BTreeSet::new(),
                    };
                    j.put(other, other_entry, &met);
                }
            } else {
                entry.partial = steps;
                j.put(direction, entry, &Met::default());
            }
            j.bound();
        });
        Some(r)
    }

    /// Tell `plan` started, as it stands (`run`), and list it among the
    /// plans applied, so Pause and Cancel reach it from its first check:
    /// until the returned [`Shown`] is let go of.
    fn show(&self, plan: &Plan, cancel: &CancelToken, run: RunKind) -> Shown<'_> {
        let meter = Arc::new(Meter::new(self.waker.lock().clone()));
        let pause = Arc::new(Pause::default());
        self.running.lock().push(Active {
            plan: plan.id,
            title: plan.title.clone(),
            cancel: cancel.clone(),
            pause: Arc::clone(&pause),
            meter: Arc::clone(&meter),
        });
        self.emit(ExecEvent::Started {
            plan: plan.id,
            title: plan.title.clone(),
            total: plan.steps.len(),
            run,
            items: plan.items.as_deref().cloned(),
        });
        Shown {
            pause,
            meter,
            compared: Arc::default(),
            _listed: Running(self, plan.id),
        }
    }

    /// The core loop: check, skip if done, apply, collect inverses in reverse order.
    /// `record` journals what was applied, and its inverse, before the plan
    /// says it finished: whoever hears `Finished` (the toast's Undo, Redo
    /// and Retry) finds the journal as the plan left it. It is told where
    /// a step's error stopped the plan, if one did. A retry's copies go on
    /// from `partial`, what the run it retries left of a file. Observers
    /// hear what held as the plan `origin`'s ([`PlanObserver`]).
    fn apply_plan(
        &self,
        plan: &Plan,
        origin: PlanId,
        cancel: &CancelToken,
        shown: Shown<'_>,
        partial: Option<super::staging::Partial>,
        record: impl FnOnce(&ExecResult, Option<Plan>, Option<Stop>),
    ) -> ExecResult {
        let total = plan.steps.len();
        // Sized before it begins where the room check did not size it (an
        // undo, a redo, a plan not asked as one action): its heavy steps
        // alone, and those that cannot say what they touch. A light one (a
        // rename) moves no byte, and is not read for it (ADR-113).
        // A step that panics as it is asked leaves the plan unsized: its
        // own check says what it does.
        if !shown.meter.is_sized() && !cancel.is_cancelled() {
            let companions = Arc::clone(&self.companions.lock());
            let sized = super::guarded(|| {
                let volumes = super::footprint::Volumes::now();
                let heavy =
                    |step: &dyn Operation| step.footprint(&volumes).is_none_or(|f| f.is_heavy());
                let dry = dry_context(&plan.steps, &companions, cancel, &shown);
                Ok(super::room::sized(&plan.steps, &dry, cancel, &heavy))
            });
            if let Ok(Some(sized)) = sized {
                shown.meter.set_ahead(sized.work.clone(), walked(&sized));
            }
        }
        let Shown {
            pause,
            meter,
            compared,
            _listed: listed,
        } = shown;
        let ctx = StepContext::new(cancel.clone(), pause, Arc::clone(&meter))
            .with_compared(compared)
            .with_companions(Arc::clone(&self.companions.lock()))
            .with_transfers(Arc::clone(&self.transfers.lock()))
            .with_under_way(self.under_way.lock().clone())
            .with_secret_asker(self.secret_asker.lock().clone())
            .with_partial(partial);
        let mut inverses: Vec<Box<dyn Operation>> = Vec::new();
        let mut held = Vec::new();
        let mut applied = 0;
        let mut skipped = 0;
        let mut complete = true;
        let mut error = None;
        let mut failed = None;
        let mut cancelled = false;
        // What the steps said, each once (several booklets fold alike).
        let (mut said, mut heard) = (Vec::new(), std::collections::HashSet::new());
        // Its speed is measured from here, its sizing and checks done.
        meter.begin(total);
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
            ctx.step_begins(i);
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
                    // It found it had nothing to make, and changed nothing:
                    // done already, as a step whose check found it so.
                    Err(e) if e.is::<super::Unneeded>() => {
                        skipped += 1;
                        ctx.say(e.to_string());
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
            for note in ctx.take_said() {
                if heard.insert(note.clone()) {
                    said.push(note);
                }
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
        // None unclaimed (every plan but one cut short in a remote's
        // folder), no step is looked at again.
        let unclaimed = if ctx.any_removed_ahead() {
            plan.steps.as_slice()
        } else {
            &[]
        };
        for step in unclaimed {
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
                server: super::guarded(|| Ok(server_of(plan.steps[at].as_ref())))
                    .ok()
                    .flatten(),
            }),
            _ => None,
        };
        record(&result, inverse, stop);
        // What it staged goes before it is told finished (its staging
        // folders, removed with its context): on a server, a round trip a
        // name, while its card still says it stops. A card gone, the plan
        // let go of what it held.
        drop(ctx);
        // Told finished, it runs no more: Pause, Resume and Cancel asked
        // as its end is read find nothing.
        drop(listed);
        self.emit(ExecEvent::Finished {
            plan: plan.id,
            applied,
            skipped,
            error: result.error.clone(),
            cancelled,
            undoable: result.undoable,
            items: plan.items.as_deref().cloned(),
            said,
            moved: meter.done.load(Ordering::Relaxed),
            ran: meter.ran(),
        });
        if !held.is_empty() {
            for observer in self.observers.all() {
                observer(origin, &held);
            }
        }
        result
    }
}

/// The mount of the server `step` reaches, if it reaches one: the first
/// path it touches on a remote, told from the mount table in memory.
fn server_of(step: &dyn Operation) -> Option<PathBuf> {
    let footprint = step.footprint(&super::footprint::Volumes::now())?;
    footprint
        .paths()
        .find_map(|path| crate::fs::reach::of(path).map(|remote| remote.point.clone()))
}

/// The reason the first conflicting step gives, after a dry run of every
/// step with one shared context, which stops when the plan is cancelled
/// (a check walking a tree says then what it has seen).
fn first_conflict(
    plan: &Plan,
    companions: &Arc<crate::companions::Companions>,
    cancel: &CancelToken,
    shown: &Shown<'_>,
) -> Option<String> {
    let ctx = dry_context(&plan.steps, companions, cancel, shown);
    super::dry_run_in(&plan.steps, ctx).find_map(|(_, status)| match status {
        StepStatus::Conflict { reason } => Some(reason),
        _ => None,
    })
}

/// A context for a dry run of `steps` before a plan the user asked for as
/// one action runs, held by its Pause and comparing trees as its run will
/// (`shown`): where a step names a remote, its folders' names are read
/// once and its sources found ahead, at once (ADR-078), where a look at
/// each would be a round trip to the server.
/// What each step of `sized` copies, as its copy counts it, for it to take
/// instead of walking its tree again (`StepContext::take_walked`).
fn walked(sized: &super::room::Sized) -> Vec<Option<u64>> {
    sized
        .writes
        .iter()
        .map(|writes| writes.as_ref().map(|(_, bytes)| *bytes))
        .collect()
}

fn dry_context(
    steps: &[Box<dyn Operation>],
    companions: &Arc<crate::companions::Companions>,
    cancel: &CancelToken,
    shown: &Shown<'_>,
) -> StepContext {
    let ctx = StepContext::new(cancel.clone(), Arc::clone(&shown.pause), Arc::default())
        .with_companions(Arc::clone(companions))
        .with_compared(Arc::clone(&shown.compared))
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
        /// Its name settled when its turn comes, as a copy's.
        settles: bool,
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
                settles: false,
                undoing: false,
                open: Arc::new((parking_lot::Mutex::new(open), parking_lot::Condvar::new())),
                log: Arc::clone(log),
            }
        }

        fn opened(&self) {
            *self.open.0.lock() = true;
            self.open.1.notify_all();
        }

        /// The same, its name settled when its turn comes, as a copy's.
        fn settling(mut self) -> Self {
            self.settles = true;
            self
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
            let made = if self.settles {
                Footprint::default().settles_name(&self.path, true)
            } else {
                Footprint::default().made(&self.path)
            };
            Some(made.heavy_if(self.heavy))
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

    /// An observer of plans hears what held as the plan asked's, its
    /// retries' too (the second continuing the first), and an undo and a
    /// redo, which make the very same changes, by ids that name no plan
    /// asked; withdrawn with its owner.
    #[test]
    fn observers_of_plans_hear_the_plan_asked_and_its_retries_by_its_id() {
        let executor = Executor::new(Scheduler::new(2));
        let heard = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let sink = Arc::clone(&heard);
        executor.observe_plans(
            "test",
            Arc::new(move |plan, changes| sink.lock().push((plan, changes.len()))),
        );
        let server = Server::new();
        server.down.store(true, Ordering::SeqCst);
        let plan = server.plan("upload", &["/srv/a", "/srv/b", "/srv/c"], "/srv/b");
        let r = executor.execute_blocking(&plan, &CancelToken::new());
        assert_eq!(r.applied, 1);
        server.blocked.store(true, Ordering::SeqCst);
        executor.retry_blocking(plan.id, &CancelToken::new());
        let again = executor.last_stopped().expect("still to retry");
        assert_ne!(again, plan.id, "its run told by an id of its own");
        server.blocked.store(false, Ordering::SeqCst);
        server.down.store(false, Ordering::SeqCst);
        let r = executor.retry_blocking(again, &CancelToken::new()).unwrap();
        assert_eq!((r.applied, r.error), (2, None));
        assert_eq!(*heard.lock(), [(plan.id, 1), (plan.id, 2)]);
        heard.lock().clear();

        executor.undo_blocking(&CancelToken::new()).unwrap();
        executor.redo_blocking(&CancelToken::new()).unwrap();
        let replays = std::mem::take(&mut *heard.lock());
        assert_eq!(replays.len(), 2, "{replays:?}");
        assert!(
            replays
                .iter()
                .all(|&(id, changes)| id != plan.id && id != again && changes == 3),
            "{replays:?}"
        );
        executor.remove_observers("test");
        executor.undo_blocking(&CancelToken::new()).unwrap();
        assert!(heard.lock().is_empty(), "withdrawn with its owner");
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

    /// A plan the user asked for is sized before it begins (ADR-113): what
    /// it will copy is known from its start, before its first copy; the
    /// `running` listing says it, its pause too; its end says what it
    /// moved.
    #[test]
    fn a_plan_asked_knows_what_it_will_copy_from_its_start() {
        let tmp = tempfile::tempdir().unwrap();
        let (src, dest) = (tmp.path().join("src"), tmp.path().join("dest"));
        std::fs::create_dir_all(src.join("a folder")).unwrap();
        std::fs::create_dir(&dest).unwrap();
        std::fs::write(src.join("it's a.bin"), [0u8; 300]).unwrap();
        std::fs::write(src.join("a folder/b.bin"), [0u8; 500]).unwrap();
        let copy = |from: &Path| -> Box<dyn Operation> {
            Box::new(super::super::Copy {
                entry: super::super::EntryRef::from_path(from).unwrap(),
                dest_dir: dest.clone(),
                new_name: None,
                preserve_attributes: true,
            })
        };
        let log = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let gate = Gate::new(
            &tmp.path().join("gate").display().to_string(),
            true,
            false,
            &log,
        );
        let plan = Plan::with_steps(
            "copy 2 items to dest",
            vec![
                Box::new(gate.clone()),
                copy(&src.join("it's a.bin")),
                copy(&src.join("a folder")),
            ],
        );
        let id = plan.id;
        let executor = Arc::new(Executor::new(Scheduler::new(1)));
        executor.execute_whole(plan, CancelToken::new());
        assert!(logged(&log, &format!("began {}", gate.path.display())));
        let transfer = executor.transfer(id).expect("running");
        assert_eq!((transfer.done, transfer.planned), (0, Some(800)));
        let running = executor.running();
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].title, "copy 2 items to dest");
        assert!(!running[0].paused);
        assert_eq!(running[0].transfer.planned, Some(800));
        assert!(executor.pause(id));
        let paused = &executor.running()[0];
        assert!(paused.paused);
        assert_eq!((paused.transfer.speed, paused.transfer.left), (None, None));
        let json = serde_json::to_value(executor.running()).unwrap();
        assert_eq!(json[0]["planned"], 800);
        assert!(json[0]["left_seconds"].is_null());
        assert!(executor.resume(id));
        gate.opened();
        let ended = events_until(&executor, |e| matches!(e, ExecEvent::Finished { .. }));
        match ended.last() {
            Some(ExecEvent::Finished {
                error: None, moved, ..
            }) => assert_eq!(*moved, 800),
            other => panic!("{other:?}"),
        }
        assert!(dest.join("a folder/b.bin").is_file());
        assert!(executor.running().is_empty());
    }

    /// A light plan runs beside a heavy one it shares nothing with; one
    /// within the heavy one's tree, and another heavy one, wait for it
    /// (ADR-079).
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

    /// A new folder beside what a copy makes, in the folder it copies
    /// into, starts at once, whether the copy runs or waits its turn; one
    /// of the same name, its case aside, or of a name the copy may be kept
    /// as beside one taken, waits for it (ADR-079, 1).
    #[test]
    fn a_name_made_beside_a_copy_starts_at_once_and_the_same_name_waits() {
        let executor = Executor::new(Scheduler::new(2));
        let log = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let copy = Gate::new("/stick/dest/photos", true, false, &log).settling();
        executor.execute(copy.plan(), CancelToken::new());
        assert!(logged(&log, "began /stick/dest/photos"));
        let beside = Gate::new("/stick/dest/it's new", false, true, &log);
        let r = executor.execute_blocking(&beside.plan(), &CancelToken::new());
        assert_eq!(r.applied, 1, "beside the copy");
        let same = Gate::new("/stick/dest/Photos", false, true, &log);
        executor.execute(same.plan(), CancelToken::new());
        let kept = Gate::new("/stick/dest/photos (copy)", false, true, &log);
        executor.execute(kept.plan(), CancelToken::new());
        let deadline = Instant::now() + Duration::from_secs(5);
        let waiting = loop {
            let waiting = executor.waiting();
            if waiting.len() == 2 && waiting.iter().all(|w| w.waits_for != WaitsFor::Starting)
                || Instant::now() > deadline
            {
                break waiting;
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        let said: Vec<(&str, &WaitsFor)> = waiting
            .iter()
            .map(|w| (w.named.as_str(), &w.waits_for))
            .collect();
        let copying = WaitsFor::Action("/stick/dest/photos".into());
        assert_eq!(
            said,
            [
                ("/stick/dest/Photos", &copying),
                ("/stick/dest/photos (copy)", &copying),
            ]
        );
        copy.opened();
        assert!(logged(&log, "ended /stick/dest/Photos"));
        assert!(logged(&log, "ended /stick/dest/photos (copy)"));

        // Behind a copy into the folder that waits its turn (for the
        // disk), a new folder beside it starts at once all the same.
        let first = Gate::new("/stick/a/one", true, false, &log);
        executor.execute(first.plan(), CancelToken::new());
        assert!(logged(&log, "began /stick/a/one"));
        let second = Gate::new("/stick/dest/videos", true, false, &log).settling();
        executor.execute(second.plan(), CancelToken::new());
        let new = Gate::new("/stick/dest/new", false, true, &log);
        let r = executor.execute_blocking(&new.plan(), &CancelToken::new());
        assert_eq!(r.applied, 1, "beside the copy waiting");
        assert!(!log.lock().iter().any(|l| l.contains("/stick/dest/videos")));
        first.opened();
        assert!(logged(&log, "began /stick/dest/videos"));
        second.opened();
        assert!(logged(&log, "ended /stick/dest/videos"));
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
        let said: Vec<(&str, &WaitsFor)> = waiting
            .iter()
            .map(|w| (w.named.as_str(), &w.waits_for))
            .collect();
        assert_eq!(
            said,
            [
                (
                    "/servers/one/a/photos/it's 1.jpg",
                    &WaitsFor::Action("/servers/one/a/photos".into()),
                ),
                ("/servers/one/b/videos", &WaitsFor::Server),
            ]
        );
        assert!(waiting[0].asked < waiting[1].asked, "in the order asked");
        up.opened();
        assert!(logged(&log, "ended /servers/one/b/videos"));
    }

    /// An action waiting its turn, cancelled, ends at once without waiting
    /// for what it waited for: a plan says it was cancelled, having done
    /// nothing; an undo leaves its entry, which a later Undo takes. What
    /// else waits goes on waiting, then runs.
    #[test]
    fn an_action_waiting_its_turn_is_cancelled_at_once() {
        let executor = Executor::new(Scheduler::new(2));
        let log = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let copy = Gate::new("/stick/photos", true, false, &log);
        executor.execute(copy.plan(), CancelToken::new());
        assert!(logged(&log, "began /stick/photos"));
        let first = Gate::new("/stick/photos/it's 2026", false, true, &log);
        let first = first.plan();
        let first_id = first.id;
        executor.execute(first, CancelToken::new());
        let second = Gate::new("/stick/photos/2027", false, true, &log);
        executor.execute(second.plan(), CancelToken::new());
        assert!(
            executor.undo(CancelToken::new()),
            "an undo waits for the copy"
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        let waiting = loop {
            let waiting = executor.waiting();
            if waiting.len() == 3 && waiting.iter().all(|w| w.waits_for != WaitsFor::Starting)
                || Instant::now() > deadline
            {
                break waiting;
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        let named: Vec<&str> = waiting.iter().map(|w| w.named.as_str()).collect();
        assert_eq!(
            named,
            ["/stick/photos/it's 2026", "/stick/photos/2027", "undo"]
        );
        executor.events().try_iter().for_each(drop);

        // The first: it says it was cancelled, while the copy still runs.
        assert!(executor.cancel_waiting(waiting[0].asked));
        assert!(!executor.cancel_waiting(waiting[0].asked), "once");
        let seen = events_until(
            &executor,
            |e| matches!(e, ExecEvent::Finished { plan, .. } if *plan == first_id),
        );
        assert!(seen.iter().any(|e| matches!(
            e,
            ExecEvent::Finished { plan, applied: 0, cancelled: true, .. } if *plan == first_id
        )));
        // The undo: it ends without a word.
        assert!(executor.cancel_waiting(waiting[2].asked));
        let deadline = Instant::now() + Duration::from_secs(5);
        while executor.queued() != 1 || executor.waiting().len() != 1 {
            assert!(Instant::now() < deadline, "{:?}", executor.waiting());
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(executor.waiting()[0].named, "/stick/photos/2027");
        assert!(!log.lock().iter().any(|l| l.contains("2026")));

        copy.opened();
        assert!(logged(&log, "ended /stick/photos/2027"));
        assert!(!log.lock().iter().any(|l| l.contains("2026")), "never ran");
        assert!(
            !log.lock().iter().any(|l| l.starts_with("undid")),
            "never ran"
        );
        // The copy's entry is still there for Undo, under the latest.
        let undone = executor.undo_blocking(&CancelToken::new()).unwrap();
        assert_eq!(undone.applied, 1);
        let undone = executor.undo_blocking(&CancelToken::new()).unwrap();
        assert_eq!(undone.applied, 1);
        assert!(logged(&log, "undid /stick/photos"));
    }

    /// Once the last plan running is let go of, whoever waits for nothing
    /// to run is woken, and finds nothing running: its end, told while it
    /// was still among those started, is never the last word (ADR-115).
    #[test]
    fn the_executor_wakes_once_nothing_runs() {
        let executor = Executor::new(Scheduler::new(1));
        let seen: Arc<parking_lot::Mutex<Vec<bool>>> = Arc::default();
        let me = Arc::downgrade(&executor);
        let heard = Arc::clone(&seen);
        executor.set_waker(Some(Arc::new(move || {
            if let Some(executor) = me.upgrade() {
                heard.lock().push(executor.busy_or_waiting());
            }
        })));
        let applied = Arc::new(AtomicUsize::new(0));
        let probe = Probe {
            applied: Arc::clone(&applied),
            conflict: false,
        };
        executor.execute(
            Plan::with_steps("probe", vec![Box::new(probe)]),
            CancelToken::new(),
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while seen.lock().last() != Some(&false) {
            assert!(Instant::now() < deadline, "{:?}", seen.lock());
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(applied.load(Ordering::SeqCst), 1);
        assert!(!executor.busy_or_waiting());
    }

    /// Cancel all cancels at once what waits its turn when asked, each
    /// once, nothing that runs, and says how many it cancelled.
    #[test]
    fn what_waits_is_cancelled_all_at_once() {
        let executor = Executor::new(Scheduler::new(2));
        let log = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let copy = Gate::new("/stick/photos", true, false, &log);
        let plan = copy.plan();
        let copy_id = plan.id;
        executor.execute(plan, CancelToken::new());
        assert!(logged(&log, "began /stick/photos"));
        let waiting: Vec<Gate> = (0..3)
            .map(|i| Gate::new(&format!("/stick/photos/it's {i}"), false, true, &log))
            .collect();
        for gate in &waiting {
            executor.execute(gate.plan(), CancelToken::new());
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while executor.waiting().len() < 3 {
            assert!(Instant::now() < deadline, "{:?}", executor.waiting());
            std::thread::sleep(Duration::from_millis(5));
        }
        let asked: Vec<u64> = executor.waiting().iter().map(|w| w.asked).collect();
        assert_eq!(executor.cancel_waiting_all(&asked), 3);
        assert_eq!(executor.cancel_waiting_all(&asked), 0, "once");
        while executor.queued() > 0 || !executor.waiting().is_empty() {
            assert!(Instant::now() < deadline, "{:?}", executor.waiting());
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(executor.transfer(copy_id).is_some(), "what runs runs on");
        copy.opened();
        assert!(logged(&log, "ended /stick/photos"));
        assert!(!log.lock().iter().any(|l| l.contains("it's")), "never ran");
    }

    /// A stopped plan that goes on by itself is under way throughout: while
    /// it waits for its server, taken out to be retried, and queued, never
    /// out of all three, as `is_busy` and `waits_for_server` asked apart
    /// may find it (ADR-115).
    #[test]
    fn a_plan_going_on_by_itself_is_under_way_as_it_is_handed_over() {
        let executor = Executor::new(Scheduler::new(1));
        assert!(!executor.busy_or_waiting());
        let rest = Plan::new("copy “it's a.bin” to the server");
        let plan = rest.id;
        let waits = Waits {
            server: None,
            down: true,
            tries: 0,
        };
        executor.stash(plan, 1, plan, rest, None, waits);
        assert!(executor.waits_for_server(plan) && executor.busy_or_waiting());
        let (work, handing) = executor
            .taken_to_retry(plan, true, |waits| Waits {
                down: false,
                ..waits
            })
            .unwrap();
        assert!(!executor.waits_for_server(plan));
        assert!(
            executor.busy_or_waiting() && executor.is_busy(),
            "handed over"
        );
        let cancel = CancelToken::new();
        executor.enqueue(work, cancel.clone(), None);
        drop(handing);
        assert!(executor.busy_or_waiting(), "queued");
        cancel.cancel();
        executor.dispatch();
        let deadline = Instant::now() + Duration::from_secs(5);
        while executor.busy_or_waiting() {
            assert!(Instant::now() < deadline, "it ends");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// What a mount's Eject would cut: the actions under way there, by
    /// what they are called; not one elsewhere, nor one cancelled that
    /// still waits there.
    #[test]
    fn the_actions_under_way_in_a_tree_are_named() {
        let executor = Executor::new(Scheduler::new(2));
        let log = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let up = Gate::new("/servers/one/photos", false, false, &log);
        let cancel = CancelToken::new();
        executor.execute(up.plan(), cancel.clone());
        assert!(logged(&log, "began /servers/one/photos"));
        let one = Path::new("/servers/one");
        assert_eq!(executor.under_way_in(one), ["/servers/one/photos"]);
        assert_eq!(
            executor
                .under_way_in(Path::new("/servers/one/photos"))
                .len(),
            1
        );
        assert!(executor.under_way_in(Path::new("/servers/two")).is_empty());
        assert!(executor.under_way_in(Path::new("/servers/on")).is_empty());
        cancel.cancel();
        assert!(executor.under_way_in(one).is_empty(), "given up on");
        up.opened();
        assert!(logged(&log, "ended /servers/one/photos"));
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

    /// Undo of a plan named (a toast's) takes back that plan, not the last
    /// asked, when nothing asked after it touched what it did, and waits
    /// for it while it runs; else it takes nothing back and says what did,
    /// asked after it in the history or running.
    #[test]
    fn undo_of_a_plan_named_takes_it_back_alone_or_says_why_not() {
        let executor = Executor::new(Scheduler::new(2));
        let log = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let copy = Gate::new("/stick/photos", true, true, &log).plan();
        executor.execute_blocking(&copy, &CancelToken::new());
        let rename = Gate::new("/disk/notes/it’s a.txt", false, true, &log).plan();
        executor.execute_blocking(&rename, &CancelToken::new());
        let r = executor
            .undo_of_blocking(copy.id, &CancelToken::new())
            .unwrap();
        assert_eq!((r.applied, r.error), (1, None));
        assert!(logged(&log, "undid /stick/photos"));
        assert_eq!(executor.undo_titles(), ["/disk/notes/it’s a.txt"]);
        let r = executor
            .undo_of_blocking(copy.id, &CancelToken::new())
            .unwrap();
        let error = r.error.unwrap_or_default();
        assert!(error.contains("undone already"), "{error}");

        // The folder it renamed in, made again after it.
        let folder = Gate::new("/disk/notes", false, true, &log).plan();
        executor.execute_blocking(&folder, &CancelToken::new());
        let r = executor
            .undo_of_blocking(rename.id, &CancelToken::new())
            .unwrap();
        let error = r.error.unwrap_or_default();
        assert_eq!(r.applied, 0);
        assert!(
            error.contains("“/disk/notes”, asked after it, touched what it did"),
            "{error}"
        );
        // Running after it, in what it made.
        let inside = Gate::new("/disk/notes/later", true, false, &log);
        let later = inside.plan();
        let later_id = later.id;
        executor.execute(later, CancelToken::new());
        assert!(logged(&log, "began /disk/notes/later"));
        let r = executor
            .undo_of_blocking(folder.id, &CancelToken::new())
            .unwrap();
        let error = r.error.unwrap_or_default();
        assert!(error.contains("asked after it, running"), "{error}");
        assert!(!log.lock().iter().any(|l| l.starts_with("undid /disk")));
        // Itself running: taken back once it ends.
        executor.undo_of(later_id, CancelToken::new());
        std::thread::sleep(Duration::from_millis(100));
        assert!(!log.lock().iter().any(|l| l == "undid /disk/notes/later"));
        inside.opened();
        assert!(logged(&log, "undid /disk/notes/later"));
    }

    /// The history says what each Undo can do (ADR-079, 13): alone, or
    /// after the action that touched what it did, which it names; Undo
    /// with what came after takes that one first, then it; an undone
    /// one's Redo waits for the one undone after it that touched the
    /// same; what runs after an entry and touches what it did holds its
    /// Undo, and its chain.
    #[test]
    fn the_history_says_what_each_undo_can_do_and_takes_what_came_after() {
        let executor = Executor::new(Scheduler::new(2));
        let log = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let run = |path: &str| {
            let plan = Gate::new(path, false, true, &log).plan();
            let r = executor.execute_blocking(&plan, &CancelToken::new());
            assert_eq!((r.applied, r.error), (1, None), "{path}");
        };
        run("/stick/photos");
        run("/disk/notes/it’s a.txt");
        // The folder the rename was in, made again after it.
        run("/disk/notes");
        let rows = |executor: &Executor| executor.history().rows;
        let h = rows(&executor);
        let titles: Vec<&str> = h.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(
            titles,
            ["/disk/notes", "/disk/notes/it’s a.txt", "/stick/photos"]
        );
        let (folder, rename, copy) = (h[0].entry, h[1].entry, h[2].entry);
        assert_eq!((h[0].state, &h[0].can), (Done::Done, &Can::Now));
        assert_eq!(
            h[1].can,
            Can::After {
                entry: folder,
                title: "/disk/notes".into(),
                since: Since::Done
            }
        );
        assert_eq!(h[1].with, [folder]);
        assert_eq!((&h[2].can, h[2].with.is_empty()), (&Can::Now, true));
        let r = executor
            .undo_entry_blocking(rename, &CancelToken::new())
            .unwrap();
        let error = r.error.unwrap_or_default();
        assert!(
            error.contains("cannot be undone alone: “/disk/notes”, asked after it"),
            "{error}"
        );

        // With what came after: the folder first, then the rename.
        let version = executor.history_version();
        assert_eq!(executor.with_after(rename), [folder, rename]);
        assert_eq!(executor.undo_with_after(rename, &CancelToken::new()), 2);
        assert!(logged(&log, "undid /disk/notes/it’s a.txt"));
        let undid: Vec<String> = log
            .lock()
            .iter()
            .filter(|l| l.starts_with("undid"))
            .cloned()
            .collect();
        assert_eq!(undid, ["undid /disk/notes", "undid /disk/notes/it’s a.txt"]);
        let deadline = Instant::now() + Duration::from_secs(5);
        while executor.is_busy() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_ne!(executor.history_version(), version);
        let h = rows(&executor);
        assert_eq!(
            h.iter().map(|r| r.state).collect::<Vec<_>>(),
            [Done::Undone, Done::Undone, Done::Done]
        );
        // Undone first, the folder is put back once the rename is.
        assert_eq!(
            h[0].can,
            Can::After {
                entry: rename,
                title: "/disk/notes/it’s a.txt".into(),
                since: Since::Undone
            }
        );
        assert_eq!(h[1].can, Can::Now);
        let r = executor
            .redo_entry_blocking(folder, &CancelToken::new())
            .unwrap();
        let error = r.error.unwrap_or_default();
        assert!(error.contains("cannot be redone alone"), "{error}");
        for entry in [rename, folder] {
            let r = executor
                .redo_entry_blocking(entry, &CancelToken::new())
                .unwrap();
            assert_eq!(r.error, None);
        }
        let h = rows(&executor);
        assert_eq!(
            h.iter().map(|r| r.entry).collect::<Vec<_>>(),
            [folder, rename, copy]
        );
        assert!(h.iter().all(|r| r.state == Done::Done));
        assert_eq!(h[1].with, [folder]);

        // Running after the folder, in it: its Undo waits, and the
        // rename's chain with it.
        let inside = Gate::new("/disk/notes/later", false, false, &log);
        executor.execute(inside.plan(), CancelToken::new());
        assert!(logged(&log, "began /disk/notes/later"));
        let h = rows(&executor);
        assert_eq!(h[0].title, "/disk/notes/later");
        assert_eq!((h[0].state, &h[0].can), (Done::Running, &Can::OnceEnded));
        assert!(matches!(
            &h[1].can,
            Can::After { since: Since::Running, title, .. } if title == "/disk/notes/later"
        ));
        assert!(h[2].with.is_empty(), "a chain through what runs waits");
        assert_eq!(h[3].can, Can::Now, "the copy shares nothing with it");
        inside.opened();
        assert!(logged(&log, "ended /disk/notes/later"));
    }

    /// An entry joins the history having been compared with each other
    /// once, and leaves it named by none: undone alone, the entries
    /// before it that it blocked are free.
    #[test]
    fn an_entry_undone_no_longer_blocks_the_ones_before_it() {
        let executor = Executor::new(Scheduler::new(2));
        let log = Arc::new(parking_lot::Mutex::new(Vec::new()));
        for path in ["/disk/a/it’s 1", "/disk/a", "/disk/b"] {
            let plan = Gate::new(path, false, true, &log).plan();
            executor.execute_blocking(&plan, &CancelToken::new());
        }
        let h = executor.history().rows;
        let (b, a, one) = (h[0].entry, h[1].entry, h[2].entry);
        assert_eq!(h[0].can, Can::Now);
        assert_eq!(h[1].can, Can::Now);
        assert!(matches!(h[2].can, Can::After { entry, .. } if entry == a));
        let r = executor
            .undo_entry_blocking(a, &CancelToken::new())
            .unwrap();
        assert_eq!(r.error, None);
        let h = executor.history().rows;
        let can = |entry: u64| h.iter().find(|r| r.entry == entry).map(|r| r.can.clone());
        assert_eq!(can(one), Some(Can::Now));
        assert_eq!(can(b), Some(Can::Now));
        let r = executor
            .undo_entry_blocking(one, &CancelToken::new())
            .unwrap();
        assert_eq!(r.error, None);
        assert_eq!(executor.undo_titles(), ["/disk/b"]);
    }

    /// A step that panics when asked what it touches, or what it makes.
    #[derive(Debug, Clone)]
    struct Unsaid {
        makes: bool,
    }

    impl Operation for Unsaid {
        fn kind(&self) -> &'static str {
            "unsaid"
        }
        fn args(&self) -> serde_json::Value {
            serde_json::json!({})
        }
        fn describe(&self) -> Change {
            Change::Create {
                path: "/example/unsaid".into(),
                directory: false,
                words: None,
            }
        }
        fn check(&self, _: &StepContext) -> StepStatus {
            StepStatus::Pending { reversible: false }
        }
        fn apply(&self, _: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
            Ok(None)
        }
        fn clone_box(&self) -> Box<dyn Operation> {
            Box::new(self.clone())
        }
        fn footprint(&self, _: &super::super::footprint::Volumes) -> Option<Footprint> {
            assert!(self.makes, "what it touches is beyond it");
            Some(Footprint::default().made(Path::new("/example/unsaid")))
        }
        fn makes(&self, _: &StepContext) -> Option<super::super::Made> {
            assert!(!self.makes, "what it makes is beyond it");
            None
        }
    }

    /// A step that panics saying what it touches runs alone, and the plans
    /// after it still start; one that panics saying what it makes refuses
    /// its plan before anything applies, which says so.
    #[test]
    fn a_step_that_panics_before_it_runs_never_stops_the_executor() {
        let executor = Executor::new(Scheduler::new(1));
        let unsaid = |makes| {
            Plan::with_steps(
                "unsaid",
                vec![Box::new(Unsaid { makes }) as Box<dyn Operation>],
            )
        };
        let r = executor.execute_blocking(&unsaid(false), &CancelToken::new());
        assert_eq!((r.applied, r.error), (1, None), "alone");
        let log = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let after = Gate::new("/elsewhere/it’s after", false, true, &log);
        assert_eq!(
            executor
                .execute_blocking(&after.plan(), &CancelToken::new())
                .applied,
            1
        );
        let r = executor
            .submit_blocking(
                Work::Execute {
                    plan: Arc::new(unsaid(true)),
                    whole: true,
                    continues: None,
                },
                &CancelToken::new(),
            )
            .expect("a result");
        assert_eq!(r.applied, 0);
        let error = r.error.unwrap_or_default();
        assert!(error.contains("failed unexpectedly"), "{error}");
    }

    /// A step whose check reads until its plan is cancelled, as one
    /// comparing a large tree with what is there does.
    #[derive(Debug, Clone)]
    struct Reading;

    impl Operation for Reading {
        fn kind(&self) -> &'static str {
            "reading"
        }
        fn args(&self) -> serde_json::Value {
            serde_json::json!({})
        }
        fn describe(&self) -> Change {
            Change::Create {
                path: "/example/it’s read".into(),
                directory: false,
                words: None,
            }
        }
        fn check(&self, ctx: &StepContext) -> StepStatus {
            let deadline = Instant::now() + Duration::from_secs(5);
            while Instant::now() < deadline {
                if ctx.check_cancelled().is_err() {
                    return StepStatus::Conflict {
                        reason: "cancelled as it read".into(),
                    };
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            StepStatus::Pending { reversible: false }
        }
        fn apply(&self, _: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
            Ok(None)
        }
        fn clone_box(&self) -> Box<dyn Operation> {
            Box::new(self.clone())
        }
        fn footprint(&self, _: &super::super::footprint::Volumes) -> Option<Footprint> {
            Some(Footprint::default().made(Path::new("/example/it’s read")))
        }
    }

    /// A plan is told started before its checks, which may read whole
    /// trees: Cancel reaches it there, and it applies nothing.
    #[test]
    fn a_plan_is_shown_and_cancelled_while_it_is_checked() {
        let executor = Executor::new(Scheduler::new(1));
        let plan = Plan::with_steps("read", vec![Box::new(Reading) as Box<dyn Operation>]);
        let id = plan.id;
        executor.execute_whole(plan, CancelToken::new());
        events_until(
            &executor,
            |e| matches!(e, ExecEvent::Started { plan, .. } if *plan == id),
        );
        assert!(executor.cancel(id), "Cancel reaches it in its checks");
        let ended = events_until(&executor, |e| matches!(e, ExecEvent::Finished { .. }));
        assert!(
            matches!(
                ended.last(),
                Some(ExecEvent::Finished { plan, applied: 0, cancelled: true, .. }) if *plan == id
            ),
            "{ended:?}"
        );
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

    /// A step that finds, as it runs, it has nothing to make.
    #[derive(Debug, Clone)]
    struct Needless;

    impl Operation for Needless {
        fn kind(&self) -> &'static str {
            "needless"
        }
        fn args(&self) -> serde_json::Value {
            serde_json::json!({})
        }
        fn describe(&self) -> Change {
            Change::Trash {
                path: "/example/needless".into(),
            }
        }
        fn check(&self, _: &StepContext) -> StepStatus {
            StepStatus::Pending { reversible: true }
        }
        fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
            ctx.say("“needless” was read twice");
            Err(super::super::Unneeded("as small as it gets already".to_owned()).into())
        }
        fn clone_box(&self) -> Box<dyn Operation> {
            Box::new(self.clone())
        }
    }

    /// A step with nothing to make is done already: the plan goes on, the
    /// steps after it applied, and nothing failed or was cancelled. What it
    /// said, and why it made nothing, the plan's end carries, for its
    /// answer.
    #[test]
    fn a_step_with_nothing_to_make_lets_the_plan_go_on() {
        let executor = Executor::new(Scheduler::new(1));
        let applied = Arc::new(AtomicUsize::new(0));
        let probe = || {
            Box::new(Probe {
                applied: Arc::clone(&applied),
                conflict: false,
            }) as Box<dyn Operation>
        };
        let plan = Plan::with_steps("copies", vec![probe(), Box::new(Needless), probe()]);
        let r = executor.execute_blocking(&plan, &CancelToken::new());
        assert_eq!((r.applied, r.skipped, r.error), (2, 1, None));
        assert!(!r.cancelled);
        assert_eq!(applied.load(Ordering::SeqCst), 2);
        let ended = events_until(&executor, |e| matches!(e, ExecEvent::Finished { .. }));
        assert!(
            matches!(
                ended.last(),
                Some(ExecEvent::Finished { said, .. })
                    if *said == ["“needless” was read twice", "as small as it gets already"]
            ),
            "{ended:?}"
        );
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
                asked: Vec::new(),
                touched: Arc::default(),
                blocked_by: BTreeSet::new(),
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

    /// The executor's events until one `last` accepts, ten seconds at most.
    fn events_until(executor: &Executor, last: impl Fn(&ExecEvent) -> bool) -> Vec<ExecEvent> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut seen = Vec::new();
        loop {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            let event = executor
                .events()
                .recv_timeout(left)
                .unwrap_or_else(|_| panic!("no such event in {seen:?}"));
            let done = last(&event);
            seen.push(event);
            if done {
                return seen;
            }
        }
    }

    /// A plan stopped on a server that does not answer goes on by itself
    /// once it answers again, one action in the history (ADR-080, 9); one
    /// stopped by something else on a server that answers at once waits
    /// for Retry alone.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_plan_its_server_stopped_goes_on_once_the_server_answers() {
        use std::os::unix::fs::PermissionsExt;
        let base = dirs::home_dir().unwrap().join(".cache/skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        let tmp = tempfile::Builder::new()
            .prefix("t-")
            .tempdir_in(base)
            .unwrap();
        let served = tmp.path().join("served");
        std::fs::create_dir(&served).unwrap();
        crate::fs::reach::pretend_remote(&served);
        let executor = Executor::new(Scheduler::new(2));
        executor.set_server_look(Duration::from_millis(100));
        let mkdir = |name: &str| {
            Box::new(super::super::Mkdir {
                path: served.join(name),
            }) as Box<dyn Operation>
        };
        let mode = |mode: u32| {
            std::fs::set_permissions(&served, std::fs::Permissions::from_mode(mode)).unwrap();
        };
        // The server gone: its top read by no one.
        mode(0o000);
        let plan = Plan::with_steps("make folders", vec![mkdir("a"), mkdir("b")]);
        let r = executor.execute_blocking(&plan, &CancelToken::new());
        assert!(r.error.is_some() && r.applied == 0, "{r:?}");
        events_until(
            &executor,
            |e| matches!(e, ExecEvent::WaitsForServer { plan: p } if *p == plan.id),
        );
        assert!(executor.waits_for_server(plan.id));
        // Meanwhile an action elsewhere, undone: its Redo stays once the
        // plan goes on by itself, which was asked before it.
        let elsewhere = tmp.path().join("elsewhere");
        executor.execute_blocking(
            &Plan::with_steps(
                "make one here",
                vec![Box::new(super::super::Mkdir { path: elsewhere })],
            ),
            &CancelToken::new(),
        );
        executor.undo_blocking(&CancelToken::new()).unwrap();
        assert!(executor.can_redo());
        executor.events().try_iter().for_each(drop);
        // Back: it goes on by itself.
        mode(0o755);
        let seen = events_until(&executor, |e| matches!(e, ExecEvent::Finished { .. }));
        let next = seen
            .iter()
            .find_map(|e| match e {
                ExecEvent::GoesOn { plan: p, next, .. } if *p == plan.id => Some(*next),
                _ => None,
            })
            .expect("it went on by itself");
        assert!(matches!(
            seen.last(),
            Some(ExecEvent::Finished { plan: p, applied: 2, error: None, .. }) if *p == next
        ));
        assert!(served.join("a").is_dir() && served.join("b").is_dir());
        assert!(!executor.can_retry(plan.id) && !executor.can_retry(next));
        assert!(executor.can_redo(), "going on dropped no Redo");
        let undone = executor.undo_blocking(&CancelToken::new()).unwrap();
        assert_eq!(undone.applied, 2, "one action");

        // In the way, on a server that answers: Retry alone.
        std::fs::write(served.join("a file"), "x").unwrap();
        let blocked = Box::new(super::super::Mkdir {
            path: served.join("a file").join("below"),
        }) as Box<dyn Operation>;
        let plan = Plan::with_steps("make below", vec![blocked]);
        let r = executor.execute_blocking(&plan, &CancelToken::new());
        assert!(r.error.is_some(), "{r:?}");
        std::thread::sleep(Duration::from_millis(600));
        assert!(executor.can_retry(plan.id) && !executor.waits_for_server(plan.id));
        assert!(
            executor.events().try_iter().all(|e| !matches!(
                e,
                ExecEvent::GoesOn { .. } | ExecEvent::WaitsForServer { .. }
            )),
            "never waited for its server"
        );
    }

    /// An action undone while it waits for its server never goes on by
    /// itself once the server answers, nor is retried: what it had left
    /// goes with what it did.
    #[cfg(target_os = "linux")]
    #[test]
    fn an_action_undone_while_it_waits_for_its_server_does_not_go_on() {
        use std::os::unix::fs::PermissionsExt;
        let base = dirs::home_dir().unwrap().join(".cache/skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        let tmp = tempfile::Builder::new()
            .prefix("t-")
            .tempdir_in(base)
            .unwrap();
        let served = tmp.path().join("served share");
        std::fs::create_dir(&served).unwrap();
        crate::fs::reach::pretend_remote(&served);
        let executor = Executor::new(Scheduler::new(2));
        executor.set_server_look(Duration::from_millis(100));
        let mode = |mode: u32| {
            std::fs::set_permissions(&served, std::fs::Permissions::from_mode(mode)).unwrap();
        };
        let here = tmp.path().join("it's here");
        let plan = Plan::with_steps(
            "make folders",
            vec![
                Box::new(super::super::Mkdir { path: here.clone() }) as Box<dyn Operation>,
                Box::new(super::super::Mkdir {
                    path: served.join("there"),
                }),
            ],
        );
        mode(0o000);
        let r = executor.execute_blocking(&plan, &CancelToken::new());
        assert!(r.error.is_some() && r.applied == 1, "{r:?}");
        events_until(
            &executor,
            |e| matches!(e, ExecEvent::WaitsForServer { plan: p } if *p == plan.id),
        );
        let undone = executor
            .undo_of_blocking(plan.id, &CancelToken::new())
            .unwrap();
        assert!(undone.error.is_none() && !here.exists(), "{undone:?}");
        assert!(!executor.can_retry(plan.id) && executor.last_stopped().is_none());
        executor.events().try_iter().for_each(drop);
        mode(0o755);
        // Several looks at the server, answering.
        std::thread::sleep(Duration::from_millis(600));
        assert!(!served.join("there").exists() && !here.exists());
        assert!(
            executor
                .events()
                .try_iter()
                .all(|e| !matches!(e, ExecEvent::GoesOn { .. })),
            "it did not go on"
        );
    }

    #[test]
    fn a_waker_may_call_the_executor_back() {
        // Pausing a plan from the waker as it starts tells it, which wakes
        // again from inside the waker: called under its own lock, it held
        // the plans' thread for good (a test's waker did, on CI).
        let executor = Executor::new(Scheduler::new(1));
        let applied = Arc::new(AtomicUsize::new(0));
        let plan = Plan::with_steps(
            "one",
            vec![Box::new(Probe {
                applied: Arc::clone(&applied),
                conflict: false,
            }) as Box<dyn Operation>],
        );
        let id = plan.id;
        let weak = Arc::downgrade(&executor);
        executor.set_waker(Some(Arc::new(move || {
            if let Some(executor) = weak.upgrade() {
                executor.pause(id);
            }
        })));
        executor.execute(plan, CancelToken::new());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            let event = executor
                .events()
                .recv_timeout(left)
                .expect("the plan was paused");
            if matches!(event, ExecEvent::Paused { plan } if plan == id) {
                break;
            }
        }
        assert!(executor.wind_down(std::time::Duration::from_secs(10)));
        assert_eq!(applied.load(Ordering::SeqCst), 0, "held before its step");
    }

    /// Files deleted for good from a folder that keeps others are not
    /// looked for again once the plan has run: none was removed ahead,
    /// so no folder is listed for a step done. Applied again, the plan
    /// finds them gone in one listing of their folder.
    #[cfg(unix)]
    #[test]
    fn a_plan_s_deletions_are_not_looked_for_again() {
        use super::super::listed::READ;
        let base = dirs::home_dir().unwrap().join(".cache/skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        let tmp = tempfile::tempdir_in(base).unwrap();
        let name = |i: usize| tmp.path().join(format!("it's {i}.txt"));
        for i in 0..400 {
            std::fs::write(name(i), b"").unwrap();
        }
        let steps: Vec<Box<dyn Operation>> = (0..400)
            .step_by(2)
            .map(|i| -> Box<dyn Operation> {
                Box::new(super::super::Delete {
                    entry: super::super::EntryRef::from_path(name(i)).unwrap(),
                })
            })
            .collect();
        let plan = Plan::with_steps("Delete", steps);
        let executor = Executor::new(Scheduler::new(2));
        let before = READ.load(Ordering::Relaxed);
        let done = executor.execute_blocking(&plan, &CancelToken::new());
        assert_eq!((done.applied, done.error), (200, None));
        assert_eq!(READ.load(Ordering::Relaxed) - before, 0, "nothing listed");
        assert!(!name(0).exists() && name(1).exists());
        let before = READ.load(Ordering::Relaxed);
        let again = executor.execute_blocking(&plan, &CancelToken::new());
        assert_eq!((again.applied, again.skipped), (0, 200), "already done");
        assert_eq!(READ.load(Ordering::Relaxed) - before, 1, "one listing");
    }
}
