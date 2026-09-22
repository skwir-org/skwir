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

/// Bytes moved so far by a running plan's steps (copies, cross-device
/// moves). `total` grows as each such step starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Transfer {
    pub done: u64,
    pub total: u64,
}

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
    Execute(Plan),
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
        })
    }

    /// Wake the UI after each event.
    pub fn set_waker(&self, waker: Option<crate::location::Waker>) {
        *self.waker.lock() = waker;
    }

    fn emit(&self, ev: ExecEvent) {
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
            .find(|job| matches!(&job.work, Work::Execute(p) if p.id == plan))
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

    /// Progress and completion events.
    pub fn events(&self) -> &Receiver<ExecEvent> {
        &self.rx
    }

    /// Queue `plan` on the lane; journal the inverse when it finishes.
    pub fn execute(self: &Arc<Self>, plan: Plan, cancel: CancelToken) {
        self.submit(Work::Execute(plan), cancel);
    }

    /// Apply `plan` in lane order, waiting for it, and journal it. Returns
    /// what happened. Never call it from inside an operation.
    pub fn execute_blocking(&self, plan: &Plan, cancel: &CancelToken) -> ExecResult {
        self.submit_blocking(Work::Execute(plan.clone()), cancel)
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
                Work::Execute(plan) => Some(self.run_execute(&plan, &job.cancel)),
                Work::Replay(direction) => self.replay(direction, &job.cancel),
            };
            if let Some(reply) = job.reply {
                let _ = reply.send(result);
            }
        }
    }

    fn run_execute(&self, plan: &Plan, cancel: &CancelToken) -> ExecResult {
        let (r, inverse) = self.apply_plan(plan, cancel);
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
        let (r, back) = self.apply_plan(&run, cancel);
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
    fn apply_plan(&self, plan: &Plan, cancel: &CancelToken) -> (ExecResult, Option<Plan>) {
        let total = plan.steps.len();
        let meter = Arc::new(Meter::new(self.waker.lock().clone()));
        self.running
            .lock()
            .push((plan.id, cancel.clone(), Arc::clone(&meter)));
        let _running = Running(self, plan.id);
        let ctx = StepContext::new(cancel.clone(), meter);
        self.emit(ExecEvent::Started {
            plan: plan.id,
            title: plan.title.clone(),
            total,
        });
        let mut inverses: Vec<Box<dyn Operation>> = Vec::new();
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
            match &status {
                StepStatus::AlreadyDone => skipped += 1,
                StepStatus::Conflict { reason } => err = Some(reason.clone()),
                StepStatus::Pending { .. } => match step.apply(&ctx) {
                    Ok(Some(inv)) => {
                        applied += 1;
                        inverses.push(inv);
                    }
                    Ok(None) => {
                        applied += 1;
                        complete = false;
                    }
                    // The step stopped midway and undid its own partial work:
                    // the plan is cancelled, not failed.
                    Err(e) if e.is::<Cancelled>() => {
                        cancelled = true;
                        break;
                    }
                    Err(e) => err = Some(format!("{e:#}")),
                },
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
