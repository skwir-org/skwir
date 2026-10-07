//! Runs plans in the background with progress and cancellation, and keeps
//! the undo/redo journal of inverse plans.
//!
//! Every plan, undo and redo runs on one first-in-first-out lane, so the
//! journal's order is the order things happened on disk. An undo pressed
//! while a copy runs waits for the copy and then takes it back; two quick
//! undos never race each other. The lane is a queue drained by one
//! scheduler job at a time, not a thread of its own, so an idle app keeps
//! no executor thread.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::SystemTime;

use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
use parking_lot::Mutex;
use serde::Serialize;

use super::context::Meter;
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
    pub status: StepStatus,
    pub error: Option<String>,
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
    },
    Step(Progress),
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

/// Bytes moved so far by a running plan's steps (copies, cross-device
/// moves). `total` grows as each such step starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Transfer {
    pub done: u64,
    pub total: u64,
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
    /// A drainer is running (on a worker or a blocking caller's thread).
    busy: bool,
}

/// The executor. One per app.
pub struct Executor {
    scheduler: Scheduler,
    journal: Mutex<Journal>,
    lane: Mutex<Lane>,
    tx: Sender<ExecEvent>,
    rx: Receiver<ExecEvent>,
    waker: Mutex<Option<crate::location::Waker>>,
    /// Plans being applied, so a surface can cancel one by id and read its
    /// byte progress.
    running: Mutex<Vec<(PlanId, CancelToken, Arc<Meter>)>>,
    observers: Contributions<ChangeObserver>,
    companions: Mutex<Arc<crate::companions::Companions>>,
    /// Plans someone waits for (`watch`): answered when their run
    /// finishes, or when they are dismissed.
    watched: Mutex<std::collections::HashMap<PlanId, Sender<Verdict>>>,
}

/// Removes a plan from `running` however `apply_plan` leaves.
struct Running<'a>(&'a Executor, PlanId);

impl Drop for Running<'_> {
    fn drop(&mut self) {
        self.0.running.lock().retain(|(id, _, _)| *id != self.1);
    }
}

/// Frees the lane if a job panics, so later jobs are not stuck behind it.
struct Draining<'a>(&'a Executor);

impl Drop for Draining<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.0.lane.lock().busy = false;
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
            tx,
            rx,
            waker: Mutex::new(None),
            running: Mutex::new(Vec::new()),
            observers: Contributions::default(),
            companions: Mutex::new(Arc::default()),
            watched: Mutex::new(std::collections::HashMap::new()),
        })
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
        if let Some(w) = self.waker.lock().as_ref() {
            w();
        }
    }

    /// Stop a plan after its current file, or before it starts if it is
    /// still queued. Steps already applied stay applied and are journaled,
    /// so the partial result can be undone.
    pub fn cancel(&self, plan: PlanId) -> bool {
        if let Some((_, token, _)) = self.running.lock().iter().find(|(id, _, _)| *id == plan) {
            token.cancel();
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

    /// Bytes moved so far by a running plan, for a progress bar finer than
    /// its steps. `None` once the plan finished.
    pub fn transfer(&self, plan: PlanId) -> Option<Transfer> {
        self.running
            .lock()
            .iter()
            .find(|(id, _, _)| *id == plan)
            .map(|(_, _, meter)| Transfer {
                done: meter.done.load(Ordering::Relaxed),
                total: meter.total.load(Ordering::Relaxed),
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
        let available = self.lane_active() || self.can_undo();
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
        let available = self.lane_active() || self.can_redo();
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

    fn lane_active(&self) -> bool {
        let lane = self.lane.lock();
        lane.busy || !lane.queue.is_empty()
    }

    fn submit(self: &Arc<Self>, work: Work, cancel: CancelToken) {
        if self.enqueue(Job {
            work,
            cancel,
            reply: None,
        }) {
            let this = Arc::clone(self);
            // The drainer must run even if the work it drains was cancelled:
            // each job checks its own token.
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
                    return;
                };
                job
            };
            let result = match job.work {
                Work::Execute { plan, whole } => {
                    Some(match whole.then(|| first_conflict(&plan)).flatten() {
                        Some(reason) => self.refuse(&plan, reason),
                        None => self.run_execute(&plan, &job.cancel),
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

    fn run_execute(&self, plan: &Plan, cancel: &CancelToken) -> ExecResult {
        let (r, inverse) = self.apply_plan(plan, cancel, RunKind::Apply);
        if r.applied > 0 {
            let mut j = self.journal.lock();
            j.undo.push(JournalEntry {
                title: plan.title.clone(),
                applied_at: SystemTime::now(),
                plan: inverse,
                partial: Vec::new(),
            });
            j.redo.clear();
        }
        r
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
        let (r, back) = self.apply_plan(&run, cancel, direction.into());
        // This run's inverses come first: its steps were applied after the
        // ones an earlier, interrupted attempt already took back.
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
        Some(r)
    }

    /// The core loop: check, skip if done, apply, collect inverses in reverse order.
    fn apply_plan(
        &self,
        plan: &Plan,
        cancel: &CancelToken,
        run: RunKind,
    ) -> (ExecResult, Option<Plan>) {
        let total = plan.steps.len();
        let meter = Arc::new(Meter::new(self.waker.lock().clone()));
        self.running
            .lock()
            .push((plan.id, cancel.clone(), Arc::clone(&meter)));
        let _running = Running(self, plan.id);
        let ctx = StepContext::new(cancel.clone(), meter)
            .with_companions(Arc::clone(&self.companions.lock()));
        self.emit(ExecEvent::Started {
            plan: plan.id,
            title: plan.title.clone(),
            total,
            run,
        });
        let mut inverses: Vec<Box<dyn Operation>> = Vec::new();
        let mut held = Vec::new();
        let mut applied = 0;
        let mut skipped = 0;
        let mut complete = true;
        let mut error = None;
        let mut cancelled = false;
        for (i, step) in plan.steps.iter().enumerate() {
            if cancel.is_cancelled() {
                cancelled = true;
                break;
            }
            let change = step.describe();
            let status = step.check(&ctx);
            let mut err = None;
            let mut holds = false;
            match &status {
                StepStatus::AlreadyDone => {
                    skipped += 1;
                    holds = true;
                }
                StepStatus::Conflict { reason } => err = Some(reason.clone()),
                StepStatus::Pending { .. } => match step.apply(&ctx) {
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
            self.emit(ExecEvent::Step(Progress {
                plan: plan.id,
                step: i,
                total,
                change,
                status,
                error: err.clone(),
            }));
            if let Some(e) = err {
                error = Some(e);
                break;
            }
        }
        inverses.reverse();
        let inverse = if inverses.is_empty() {
            None
        } else {
            Some(Plan::with_steps(format!("undo {}", plan.title), inverses))
        };
        self.emit(ExecEvent::Finished {
            plan: plan.id,
            applied,
            skipped,
            error: error.clone(),
            cancelled,
            undoable: inverse.is_some(),
        });
        if !held.is_empty() {
            for observer in self.observers.all() {
                observer(&held);
            }
        }
        (
            ExecResult {
                applied,
                skipped,
                error,
                cancelled,
                undoable: inverse.is_some(),
                complete,
            },
            inverse,
        )
    }
}

/// The reason the first conflicting step gives, after a dry run of every
/// step with one shared context.
fn first_conflict(plan: &Plan) -> Option<String> {
    super::dry_run(&plan.steps).find_map(|(_, status)| match status {
        StepStatus::Conflict { reason } => Some(reason),
        _ => None,
    })
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
