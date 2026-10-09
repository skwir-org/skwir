//! What a step can use while it is checked and applied: the plan's
//! cancellation and pause, a meter for work inside one step, and lookups
//! the plan's steps share instead of repeating.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};
use parking_lot::Mutex;

use crate::cancel::CancelToken;
use crate::location::Waker;

/// The error a step returns when it stopped because its plan was cancelled.
/// The executor reports the plan as cancelled, not failed.
#[derive(Debug, thiserror::Error)]
#[error("cancelled")]
pub struct Cancelled;

/// Whether a running plan is held. A step waits where it would answer
/// Cancel ([`StepContext::check_cancelled`]), holding what it has open,
/// until the plan is resumed or cancelled.
#[derive(Default)]
pub(super) struct Pause {
    /// While paused, a channel nobody sends on: resuming drops its only
    /// sender, which wakes every step waiting on a receiver.
    held: Mutex<Option<(Sender<()>, Receiver<()>)>>,
}

impl Pause {
    /// Hold the plan. Whether it was running freely.
    pub(super) fn pause(&self) -> bool {
        let mut held = self.held.lock();
        if held.is_some() {
            return false;
        }
        *held = Some(crossbeam_channel::bounded(0));
        true
    }

    /// Let the plan go on. Whether it was held.
    pub(super) fn resume(&self) -> bool {
        self.held.lock().take().is_some()
    }

    pub(super) fn is_paused(&self) -> bool {
        self.held.lock().is_some()
    }

    /// Return at once unless the plan is held; held, return once it is
    /// resumed or `cancel` is cancelled.
    fn wait(&self, cancel: &CancelToken) {
        let Some(resumed) = self.held.lock().as_ref().map(|(_, rx)| rx.clone()) else {
            return;
        };
        let cancelled = cancel.subscribe();
        crossbeam_channel::select! {
            recv(resumed) -> _ => {}
            recv(cancelled.receiver()) -> _ => {}
        }
    }
}

/// Bytes a running plan has moved, and its steps ended, read by whoever
/// shows its progress.
#[derive(Default)]
pub(super) struct Meter {
    pub(super) done: AtomicU64,
    pub(super) total: AtomicU64,
    /// Read with `done` and `total`, under one lock, so a count of steps
    /// and the bytes of the step under way never disagree.
    pub(super) steps: Mutex<Steps>,
    wake: Option<Waker>,
    last_wake: Mutex<Option<Instant>>,
}

/// The steps of a plan that ended, and `done` and `total` as the next one
/// began.
#[derive(Clone, Copy, Default)]
pub(super) struct Steps {
    pub(super) ended: usize,
    pub(super) done: u64,
    pub(super) total: u64,
}

/// How often byte progress may wake the UI: often enough for a bar to move,
/// rarely enough that a fast copy does not redraw per chunk.
const WAKE_EVERY: Duration = Duration::from_millis(100);

impl Meter {
    pub(super) fn new(wake: Option<Waker>) -> Self {
        Self {
            wake,
            ..Self::default()
        }
    }

    /// A step ended: the next one's bytes count from here.
    pub(super) fn step_ended(&self) {
        let mut steps = self.steps.lock();
        steps.ended += 1;
        steps.done = self.done.load(Ordering::Relaxed);
        steps.total = self.total.load(Ordering::Relaxed);
    }

    fn advance(&self, bytes: u64) {
        self.done.fetch_add(bytes, Ordering::Relaxed);
        let Some(wake) = &self.wake else {
            return;
        };
        let now = Instant::now();
        let mut last = self.last_wake.lock();
        if last.is_none_or(|t| now.duration_since(t) >= WAKE_EVERY) {
            *last = Some(now);
            drop(last);
            wake();
        }
    }
}

/// Passed to [`super::Operation::check`] and [`super::Operation::apply`].
/// One context lives for one run of a plan (or one dry run), so what it
/// caches is shared by that plan's steps and no longer.
pub struct StepContext {
    cancel: CancelToken,
    pause: Arc<Pause>,
    meter: Arc<Meter>,
    /// The trash listing, read once per plan: parsing every `.trashinfo`
    /// per restored item made undoing a large trash quadratic.
    trash: RefCell<Option<super::builtin::TrashListing>>,
    /// The files beside a file that go where it goes (ADR-028).
    companions: Arc<crate::companions::Companions>,
    /// The folders a dry run's earlier steps make (`super::dry_run`): a
    /// later step finds them, as it does when the plan is applied step by
    /// step.
    foreseen: RefCell<HashSet<PathBuf>>,
    /// Where the folders copies are under way in are noted, and those this
    /// plan stages in, each swept once and noted while the plan runs.
    under_way: Option<super::staging::UnderWay>,
    staged_in: RefCell<HashMap<PathBuf, Option<super::staging::Mark>>>,
    /// How this plan's copies are written through to each volume of a
    /// drive the user can unplug (`pace::Pace`), by its device: one per
    /// volume, so the drive's queue runs on from one item to the next, and
    /// [`Self::write_out`] writes each out.
    paces: RefCell<Vec<(Option<u64>, Rc<super::pace::Pace>)>>,
    /// What waits until a write-out has put on the drive what this plan
    /// wrote through (a move's removal of what it copied), done at the
    /// next one ([`Self::after_write_out`]).
    waiting: RefCell<Vec<Box<dyn FnOnce() -> anyhow::Result<()>>>>,
    /// When the last write-out ended (or, before one, when the first
    /// thing began to wait for it), and how long it took.
    written_out: std::cell::Cell<(Option<Instant>, Duration)>,
    /// What failed of a write-out on the way, or of what waited for it:
    /// told at the plan's end, never by the step it fell on.
    failed: RefCell<Option<anyhow::Error>>,
}

/// A plan's write-outs before its end are at least this far apart, and
/// apart by ten times the last one's length: on a hard drive one takes a
/// third of a second however little changed, so they stay a tenth of the
/// plan's time at most.
const WRITE_OUTS_APART: Duration = Duration::from_secs(1);
const WRITE_OUT_SHARE: u32 = 10;

impl StepContext {
    pub(super) fn new(cancel: CancelToken, pause: Arc<Pause>, meter: Arc<Meter>) -> Self {
        Self {
            cancel,
            pause,
            meter,
            trash: RefCell::new(None),
            companions: Arc::default(),
            foreseen: RefCell::default(),
            under_way: None,
            staged_in: RefCell::default(),
            paces: RefCell::default(),
            waiting: RefCell::default(),
            written_out: std::cell::Cell::new((None, Duration::ZERO)),
            failed: RefCell::default(),
        }
    }

    /// How this plan's copies into `dest_dir`, a folder of a drive the user
    /// can unplug, are written through: the pace of its volume, shared by
    /// every copy of the plan onto it.
    ///
    /// # Errors
    /// When the folder cannot be opened.
    pub(super) fn pace_through(&self, dest_dir: &Path) -> std::io::Result<Rc<super::pace::Pace>> {
        let drive = crate::fs::Dir::open(dest_dir)?;
        let device = drive.device().ok();
        let mut paces = self.paces.borrow_mut();
        if let Some((_, pace)) = paces.iter().find(|(d, _)| d.is_some() && *d == device) {
            return Ok(Rc::clone(pace));
        }
        let pace = Rc::new(super::pace::Pace::through(drive));
        paces.push((device, Rc::clone(&pace)));
        Ok(pace)
    }

    /// Wait until the drives this plan's copies were written through to
    /// have every byte, then write their volumes out (`syncfs`), names and
    /// folders included, and do what waited for it
    /// ([`Self::after_write_out`]). At the plan's end, and on the way when
    /// moves wait: on a journaled or copy-on-write filesystem it commits
    /// the whole volume, which a hard drive takes a third of a second to do
    /// however little changed.
    ///
    /// # Errors
    /// When a drive could not write it all (then what waited for it is not
    /// done: a move's original stays), or what waited failed, now or at a
    /// write-out on the way: the first failure.
    pub(super) fn write_out(&self) -> anyhow::Result<()> {
        let now = self.write_out_now();
        match self.failed.take() {
            Some(earlier) => Err(earlier),
            None => now,
        }
    }

    fn write_out_now(&self) -> anyhow::Result<()> {
        let began = Instant::now();
        for (_, pace) in self.paces.borrow().iter() {
            if let Err(e) = pace.write_out() {
                return Err(if self.waiting.borrow().is_empty() {
                    e
                } else {
                    e.context("what was moved stays where it was")
                });
            }
        }
        let ended = Instant::now();
        self.written_out.set((Some(ended), ended - began));
        // Each is done, whatever the others say: the first failure is told.
        let mut failed = None;
        for then in self.waiting.take() {
            if let Err(e) = then() {
                failed.get_or_insert(e);
            }
        }
        failed.map_or(Ok(()), Err)
    }

    /// Do `then` once what this plan wrote through is on the drive, names
    /// and all: at the next write-out ([`Self::write_out`]), which comes
    /// once write-outs stay a small share of the plan's time
    /// (`WRITE_OUT_SHARE`), and at its end. Many moves share one. What
    /// fails of it is told at the plan's end: the step that asks has done
    /// what it said (a copy whose original stays is still a copy, and its
    /// undo still takes it back), and the plan goes on.
    pub(super) fn after_write_out(&self, then: impl FnOnce() -> anyhow::Result<()> + 'static) {
        self.waiting.borrow_mut().push(Box::new(then));
        let (since, took) = self.written_out.get();
        let since = since.unwrap_or_else(|| {
            let now = Instant::now();
            self.written_out.set((Some(now), took));
            now
        });
        if since.elapsed() >= WRITE_OUTS_APART.max(took * WRITE_OUT_SHARE)
            && let Err(e) = self.write_out_now()
        {
            self.failed.borrow_mut().get_or_insert(e);
        }
    }

    /// The same context, noting in `under_way` the folders its copies are
    /// made in, for a later run to sweep should this one end mid-copy.
    #[must_use]
    pub(super) fn with_under_way(mut self, under_way: Option<super::staging::UnderWay>) -> Self {
        self.under_way = under_way;
        self
    }

    /// Before a copy stages in `dest_dir`: the first time this plan does,
    /// note the folder as under way and remove what copies cut short left
    /// there (`staging::sweep`).
    pub(super) fn staging_in(&self, dest_dir: &Path) {
        if self.staged_in.borrow().contains_key(dest_dir) {
            return;
        }
        let mark = self.under_way.as_ref().and_then(|u| u.mark(dest_dir));
        super::staging::sweep(dest_dir);
        self.staged_in
            .borrow_mut()
            .insert(dest_dir.to_path_buf(), mark);
    }

    /// A staging folder in `dest_dir` for a step to make something in and
    /// put in place whole (`Staging::place`), so what it leaves cut short
    /// never looks finished: removed when dropped, and swept by a later
    /// run should this one end mid-way, as a copy's is.
    ///
    /// # Errors
    /// When the folder cannot be made in `dest_dir`.
    pub fn stage(&self, dest_dir: &Path) -> std::io::Result<super::staging::Staging> {
        self.staging_in(dest_dir);
        super::staging::Staging::new(dest_dir)
    }

    /// Whether the plan is paused: a step whose work goes on elsewhere (a
    /// helper program) holds it meanwhile, and waits in
    /// [`Self::check_cancelled`].
    pub fn is_paused(&self) -> bool {
        self.pause.is_paused()
    }

    /// The same context, carrying files beside a file by `companions`.
    #[must_use]
    pub fn with_companions(mut self, companions: Arc<crate::companions::Companions>) -> Self {
        self.companions = companions;
        self
    }

    /// The rules of the files beside a file.
    pub fn companions(&self) -> &crate::companions::Companions {
        &self.companions
    }

    /// A context for a dry run or a test: never cancelled, progress unread.
    pub fn detached() -> Self {
        Self::with_cancel(CancelToken::new())
    }

    /// A context outside the executor that stops when `cancel` does.
    pub fn with_cancel(cancel: CancelToken) -> Self {
        Self::new(cancel, Arc::default(), Arc::default())
    }

    pub fn cancel_token(&self) -> &CancelToken {
        &self.cancel
    }

    /// `Err(Cancelled)` once the plan is cancelled; call between units of
    /// work. Where a step answers Cancel it answers Pause too: while the
    /// plan is paused this waits, until it is resumed or cancelled.
    ///
    /// # Errors
    /// [`Cancelled`] when the plan's token is cancelled.
    pub fn check_cancelled(&self) -> Result<(), Cancelled> {
        self.pause.wait(&self.cancel);
        if self.cancel.is_cancelled() {
            Err(Cancelled)
        } else {
            Ok(())
        }
    }

    /// Announce `bytes` of work this step is about to do.
    pub fn add_total(&self, bytes: u64) {
        self.meter.total.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Report `bytes` of work done.
    pub fn advance(&self, bytes: u64) {
        self.meter.advance(bytes);
    }

    /// The bytes reported done.
    #[cfg(all(test, target_os = "linux"))]
    pub(super) fn done(&self) -> u64 {
        self.meter.done.load(Ordering::Relaxed)
    }

    /// Run `f` over the trash listing, reading it on first use in this plan.
    pub(super) fn with_trash<T>(
        &self,
        f: impl FnOnce(&mut super::builtin::TrashListing) -> T,
    ) -> Result<T, trash::Error> {
        let mut cached = self.trash.borrow_mut();
        if cached.is_none() {
            *cached = Some(super::builtin::TrashListing::new(super::sys::trash_list()?));
        }
        Ok(f(cached.as_mut().expect("just filled")))
    }

    /// The trash changed under the cached listing (an item was trashed, or
    /// put back): read again when next asked.
    pub(super) fn forget_trash(&self) {
        self.trash.borrow_mut().take();
    }

    /// The item whose record is `info` left the trash: out of the cached
    /// listing, which stays.
    pub(super) fn trash_taken(&self, info: &std::path::Path) {
        if let Some(listing) = self.trash.borrow_mut().as_mut() {
            listing.taken(info);
        }
    }

    /// A dry run's step checked as pending will make what `change` says:
    /// the steps after it count a folder it makes as there.
    pub(super) fn foresee(&self, change: &super::Change) {
        if let super::Change::Create {
            path,
            directory: true,
            ..
        } = change
        {
            self.foreseen.borrow_mut().insert(path.clone());
        }
    }

    /// Whether `path` is a folder, or one an earlier step of this dry run
    /// makes.
    pub fn is_folder(&self, path: &Path) -> bool {
        path.is_dir() || self.foreseen.borrow().contains(path)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn what_waits_for_a_write_out_is_done_at_it_all_of_it() {
        let ctx = StepContext::detached();
        let done = Rc::new(Cell::new(0));
        for fails in [false, true, false] {
            let done = Rc::clone(&done);
            ctx.after_write_out(move || {
                done.set(done.get() + 1);
                if fails {
                    anyhow::bail!("its original could not all be removed");
                }
                Ok(())
            });
        }
        assert_eq!(done.get(), 0, "a write-out is not due yet");
        let failed = ctx.write_out().unwrap_err();
        assert!(failed.to_string().contains("could not all be removed"));
        assert_eq!(done.get(), 3, "one failing leaves none of the others");
        ctx.write_out().unwrap();
        assert_eq!(done.get(), 3, "each done once");
    }

    #[test]
    fn a_write_out_comes_once_write_outs_stay_a_small_share() {
        let ctx = StepContext::detached();
        let done = Rc::new(Cell::new(false));
        // The last one took a second: the next is ten seconds after it.
        let ended = Instant::now().checked_sub(Duration::from_secs(5)).unwrap();
        ctx.written_out.set((Some(ended), Duration::from_secs(1)));
        let waited = Rc::clone(&done);
        ctx.after_write_out(move || {
            waited.set(true);
            Ok(())
        });
        assert!(!done.get());
        ctx.written_out.set((
            ended.checked_sub(Duration::from_secs(6)),
            Duration::from_secs(1),
        ));
        // What fails on the way is told at the end, not by the step.
        ctx.after_write_out(|| anyhow::bail!("“a.jpg” changed after it was copied"));
        assert!(done.get(), "due: everything waiting is done");
        let told = ctx.write_out().unwrap_err();
        assert!(told.to_string().contains("changed after it was copied"));
        ctx.write_out().unwrap();
    }
}
