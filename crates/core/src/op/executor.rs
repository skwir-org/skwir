//! Runs plans in the background with progress and cancellation, and keeps
//! the undo/redo journal of inverse plans.

use std::sync::Arc;
use std::time::SystemTime;

use crossbeam_channel::{Receiver, Sender, unbounded};
use parking_lot::Mutex;
use serde::Serialize;

use super::{Change, Operation, Plan, PlanId, StepStatus};
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

/// One journaled plan: what was applied and how to undo it.
#[derive(Debug)]
pub struct JournalEntry {
    pub title: String,
    pub applied_at: SystemTime,
    /// The plan as applied; re-applied on redo (it is idempotent).
    pub forward: Plan,
    /// The inverse plan; `None` if nothing reversible was applied.
    pub inverse: Option<Plan>,
    /// False if some applied step had no inverse.
    pub complete: bool,
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

/// The executor. One per app.
pub struct Executor {
    scheduler: Scheduler,
    journal: Mutex<Journal>,
    tx: Sender<ExecEvent>,
    rx: Receiver<ExecEvent>,
}

impl Executor {
    pub fn new(scheduler: Scheduler) -> Arc<Self> {
        let (tx, rx) = unbounded();
        Arc::new(Executor {
            scheduler,
            journal: Mutex::new(Journal::default()),
            tx,
            rx,
        })
    }

    /// Progress and completion events.
    pub fn events(&self) -> &Receiver<ExecEvent> {
        &self.rx
    }

    /// Apply `plan` on a worker thread; journal the inverse when it finishes.
    pub fn execute(self: &Arc<Self>, plan: Plan, cancel: CancelToken) {
        let this = Arc::clone(self);
        self.scheduler
            .spawn(Priority::Normal, "op.execute", cancel.clone(), move || {
                this.execute_blocking(&plan, &cancel);
            });
    }

    /// Apply `plan` on the calling thread and journal it. Returns what happened.
    pub fn execute_blocking(&self, plan: &Plan, cancel: &CancelToken) -> ExecResult {
        let (r, inverse) = self.apply_plan(plan, cancel);
        if r.applied > 0 {
            let mut j = self.journal.lock();
            j.undo.push(JournalEntry {
                title: plan.title.clone(),
                applied_at: SystemTime::now(),
                forward: plan.clone(),
                inverse,
                complete: r.complete,
            });
            j.redo.clear();
        }
        r
    }

    /// Undo the most recent plan, in the background. Returns false if nothing to undo.
    pub fn undo(self: &Arc<Self>, cancel: CancelToken) -> bool {
        let Some(entry) = self.journal.lock().undo.pop() else {
            return false;
        };
        let this = Arc::clone(self);
        self.scheduler
            .spawn(Priority::Normal, "op.undo", cancel.clone(), move || {
                this.undo_entry(entry, &cancel);
            });
        true
    }

    /// Undo synchronously. Returns `None` if nothing to undo.
    pub fn undo_blocking(&self, cancel: &CancelToken) -> Option<ExecResult> {
        let entry = self.journal.lock().undo.pop()?;
        Some(self.undo_entry(entry, cancel))
    }

    /// Redo the most recently undone plan, in the background.
    pub fn redo(self: &Arc<Self>, cancel: CancelToken) -> bool {
        let Some(entry) = self.journal.lock().redo.pop() else {
            return false;
        };
        let this = Arc::clone(self);
        self.scheduler
            .spawn(Priority::Normal, "op.redo", cancel.clone(), move || {
                this.redo_entry(entry, &cancel);
            });
        true
    }

    /// Redo synchronously.
    pub fn redo_blocking(&self, cancel: &CancelToken) -> Option<ExecResult> {
        let entry = self.journal.lock().redo.pop()?;
        Some(self.redo_entry(entry, cancel))
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

    fn undo_entry(&self, entry: JournalEntry, cancel: &CancelToken) -> ExecResult {
        let Some(inverse) = &entry.inverse else {
            return ExecResult {
                applied: 0,
                skipped: 0,
                error: Some("nothing reversible to undo".into()),
                cancelled: false,
                undoable: false,
                complete: false,
            };
        };
        let mut inverse = inverse.clone();
        inverse.title = format!("undo {}", entry.title);
        let (r, _) = self.apply_plan(&inverse, cancel);
        if r.error.is_none() && !r.cancelled {
            self.journal.lock().redo.push(entry);
        } else {
            // Partial undo: keep the entry so the user can retry.
            self.journal.lock().undo.push(entry);
        }
        r
    }

    fn redo_entry(&self, entry: JournalEntry, cancel: &CancelToken) -> ExecResult {
        let (r, inverse) = self.apply_plan(&entry.forward, cancel);
        if r.applied > 0 {
            self.journal.lock().undo.push(JournalEntry {
                title: entry.title,
                applied_at: SystemTime::now(),
                forward: entry.forward,
                inverse,
                complete: r.complete,
            });
        } else {
            self.journal.lock().redo.push(entry);
        }
        r
    }

    /// The core loop: check, skip if done, apply, collect inverses in reverse order.
    fn apply_plan(&self, plan: &Plan, cancel: &CancelToken) -> (ExecResult, Option<Plan>) {
        let total = plan.steps.len();
        let _ = self.tx.send(ExecEvent::Started {
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
            let status = step.check();
            let mut err = None;
            match &status {
                StepStatus::AlreadyDone => skipped += 1,
                StepStatus::Conflict { reason } => err = Some(reason.clone()),
                StepStatus::Pending { .. } => match step.apply() {
                    Ok(Some(inv)) => {
                        applied += 1;
                        inverses.push(inv);
                    }
                    Ok(None) => {
                        applied += 1;
                        complete = false;
                    }
                    Err(e) => err = Some(format!("{e:#}")),
                },
            }
            let _ = self.tx.send(ExecEvent::Step(Progress {
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
        let _ = self.tx.send(ExecEvent::Finished {
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
