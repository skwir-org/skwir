//! What a step can use while it is checked and applied: the plan's
//! cancellation and pause, a meter for work inside one step, and lookups
//! the plan's steps share instead of repeating.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime};

use crossbeam_channel::{Receiver, Sender};
use parking_lot::Mutex;

use super::speed;
use super::staging::{Cut, Kept, KeptBatch};
use crate::cancel::CancelToken;
use crate::location::Waker;

/// The error a step returns when it stopped because its plan was cancelled.
/// The executor reports the plan as cancelled, not failed.
#[derive(Debug, thiserror::Error)]
#[error("cancelled")]
pub struct Cancelled;

/// The error a step returns when it found, as it ran, that it had nothing
/// to make (a smaller copy that would not be smaller), having changed
/// nothing: the executor counts it done already, as a step whose check
/// found it so, and the plan goes on. Its words say why, to the user: the
/// executor says them for the step ([`StepContext::say`]).
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct Unneeded(pub String);

/// What a step's work done on another thread reports through and heeds (a
/// helper program's, followed from a worker): the bytes it got through,
/// as [`StepContext::advance`] counts them, and the plan's pause.
#[derive(Clone)]
pub struct Gauge {
    meter: Arc<Meter>,
    pause: Arc<Pause>,
}

impl Gauge {
    /// Announce `bytes` of work the step is about to do, once known where
    /// it is found out (a file's size, read in its remote's lane).
    pub fn announce(&self, bytes: u64) {
        self.meter.total.fetch_add(bytes, Ordering::Relaxed);
        // The window shows the step's card from here on.
        if let Some(wake) = &self.meter.wake {
            wake();
        }
    }

    /// Report `bytes` of work done.
    pub fn advance(&self, bytes: u64) {
        self.meter.advance(bytes);
    }

    /// Whether the plan is paused: the work elsewhere is held meanwhile.
    pub fn is_paused(&self) -> bool {
        self.pause.is_paused()
    }
}

/// Why a step was given no secret ([`StepContext::ask_secret`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unanswered {
    /// No one can be asked: no window (a script's plan, an agent's).
    NoOne,
    /// The user cancelled, or the plan was.
    Declined,
}

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

    /// Take the plan's hold off it, `None` when it was not held: the plan
    /// goes on once what this returns is dropped, so whoever resumes it
    /// says so first, and its next step is told after.
    #[must_use]
    pub(super) fn resume(&self) -> Option<(Sender<()>, Receiver<()>)> {
        self.held.lock().take()
    }

    pub(super) fn is_paused(&self) -> bool {
        self.held.lock().is_some()
    }

    /// Return at once unless the plan is held; held, return once it is
    /// resumed or `cancel` is cancelled.
    pub(super) fn wait(&self, cancel: &CancelToken) {
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
/// shows its progress, with how fast it goes and how long it has left
/// (`speed`, ADR-113).
#[derive(Default)]
pub(super) struct Meter {
    pub(super) done: AtomicU64,
    pub(super) total: AtomicU64,
    /// Read with `done` and `total`, under one lock, so a count of steps
    /// and the bytes of the step under way never disagree.
    pub(super) steps: Mutex<Steps>,
    wake: Option<Waker>,
    last_wake: Mutex<Option<Instant>>,
    /// The files a mount that sends later still has to send before the
    /// plan ends (ADR-102), plus one; 0 while none is being sent.
    pub(super) sending: AtomicUsize,
    /// Of `done`, the bytes that moved none now: a clone's (the volume
    /// shares its blocks), a copy's gone on from, which a run before this
    /// one moved. Counted done, never as speed.
    unmeasured: AtomicU64,
    /// Bytes read from a server and not yet written where they go (a large
    /// file's ranges read at once, written in order): not done, but how
    /// fast they come is the speed, and they show as moved (`ranges`).
    arriving: AtomicU64,
    /// The most [`Self::reading`] said was moved: what arrived and was let
    /// go of (the copy stopped) never takes a bar back.
    shown: AtomicU64,
    /// The steps a batch finished before their turn (ADR-078) that have not
    /// ended: their items count as done as each comes whole, not when the
    /// steps place them after the batch. Each leaves as its step ends, so
    /// what a reading counts is how many there are. Said, never acted on.
    finished_ahead: Mutex<HashSet<usize>>,
    /// The steps a batch under way took, with their bytes copied so far
    /// where its transfer tells them (`Told::writing`) and in all: what
    /// arrives says it is copying (ADR-135). A step leaves as it ends, or
    /// as the batch ends without making it.
    batching: Mutex<HashMap<usize, (u64, u64)>>,
    /// The steps of the plan, once it began ([`Self::begin`]).
    items: AtomicUsize,
    /// What its steps were sized at before it began, when they were.
    ahead: Mutex<Option<Ahead>>,
    /// What it moved, sampled ([`speed::Samples`]): what it wrote, or,
    /// while a mount sends, what that mount sent.
    samples: Mutex<speed::Samples>,
    /// How long its steps ran, its pauses left out.
    clock: Mutex<Clock>,
    /// What wakes the window once its bytes stop moving ([`Stall`]).
    stall: StallGuard,
    /// It writes into a mount that sends to its server after (ADR-102),
    /// which its end waits for: no time left is said while it writes.
    sends_after: AtomicBool,
    /// While a mount sends: what it sent so far, and what it still has
    /// (`UNTOLD` while it has not said).
    sent: AtomicU64,
    send_left: AtomicU64,
}

/// The steps of a plan that ended, and `done` and `total` as the next one
/// began.
#[derive(Clone, Copy, Default)]
pub(super) struct Steps {
    pub(super) ended: usize,
    pub(super) done: u64,
    pub(super) total: u64,
    /// Of what `total` grew by since, the bytes a batch counted for the
    /// steps after the one under way (ADR-078): theirs, not its own.
    pub(super) others: u64,
}

/// How long a plan's steps ran, from its start ([`Meter::begin`]), its
/// pauses left out: a pause asked during its checks holds it from there.
#[derive(Default)]
struct Clock {
    ran: speed::Ran,
    begun: bool,
    paused: bool,
}

/// Wakes the window once a plan's bytes stopped moving for
/// [`speed::SPEED_OVER`]: what it said last (a speed, a time left) is no
/// longer said, and nothing else would draw it again. Once a stall, armed
/// again by the next bytes, never a loop. Its thread starts with the first
/// bytes, and ends with the meter ([`StallGuard`]).
#[derive(Default)]
struct Stall {
    state: Mutex<StallState>,
    changed: parking_lot::Condvar,
}

#[derive(Default)]
struct StallState {
    /// When bytes last moved; `None` once the stall was told.
    moved: Option<Instant>,
    started: bool,
    ended: bool,
}

impl Stall {
    /// Bytes moved at `now`: told `wake`, once they stop.
    fn moved(self: &Arc<Self>, now: Instant, wake: &Waker) {
        let mut state = self.state.lock();
        let armed = state.moved.is_none();
        state.moved = Some(now);
        if !state.started {
            state.started = true;
            drop(state);
            let (stall, wake) = (Arc::clone(self), Arc::clone(wake));
            // Without a thread, no wake comes: the reading stays as drawn.
            let _ = std::thread::Builder::new()
                .name("skwir-plan-stall".into())
                .spawn(move || stall.watch(&wake));
        } else if armed {
            self.changed.notify_one();
        }
    }

    /// Wake `wake` once each time no bytes moved for `SPEED_OVER`, until
    /// the meter goes. Woken only when armed again, or due.
    fn watch(&self, wake: &Waker) {
        let mut state = self.state.lock();
        while !state.ended {
            match state.moved {
                None => self.changed.wait(&mut state),
                Some(moved) if Instant::now() < moved + speed::SPEED_OVER => {
                    let _ = self
                        .changed
                        .wait_until(&mut state, moved + speed::SPEED_OVER);
                }
                Some(_) => {
                    state.moved = None;
                    parking_lot::MutexGuard::unlocked(&mut state, || wake());
                }
            }
        }
    }
}

/// A meter's [`Stall`], which ends its thread when the meter goes.
#[derive(Default)]
struct StallGuard(Arc<Stall>);

impl Drop for StallGuard {
    fn drop(&mut self) {
        self.0.state.lock().ended = true;
        self.0.changed.notify_one();
    }
}

/// What a plan's steps were sized at before it began (`room::sized`): the
/// total it will count, and for each step that copies, the size of what it
/// copies, which it takes instead of walking its tree again.
struct Ahead {
    /// Each step's bytes as it will count them; 0 once it ended, or a
    /// batch counted it (ADR-078).
    work: Vec<u64>,
    /// The sum of `work`.
    rest: u64,
    /// The size of what each step copies, as the copy counts it, taken
    /// once ([`StepContext::take_walked`]).
    walked: Vec<Option<u64>>,
}

/// How often byte progress may wake the UI: often enough for a bar to move,
/// rarely enough that a fast copy does not redraw per chunk.
const WAKE_EVERY: Duration = Duration::from_millis(100);

/// What `send_left` holds while a mount has not said what it sends.
const UNTOLD: u64 = u64::MAX;

impl Meter {
    pub(super) fn new(wake: Option<Waker>) -> Self {
        Self {
            wake,
            send_left: AtomicU64::new(UNTOLD),
            ..Self::default()
        }
    }

    /// The plan's `items` steps begin, sized or not: its speed is
    /// measured from here.
    pub(super) fn begin(&self, items: usize) {
        self.items.store(items, Ordering::Relaxed);
        let mut clock = self.clock.lock();
        clock.begun = true;
        // Paused during its checks, it runs from where it goes on.
        if !clock.paused {
            clock.ran.go(Instant::now());
            self.samples.lock().arm();
        }
    }

    /// What its steps were sized at before it began: each one's bytes as
    /// it counts them, and the size of what each that copies copies.
    pub(super) fn set_ahead(&self, work: Vec<u64>, walked: Vec<Option<u64>>) {
        let rest = work
            .iter()
            .fold(0u64, |sum, &bytes| sum.saturating_add(bytes));
        *self.ahead.lock() = Some(Ahead { work, rest, walked });
    }

    /// Whether its steps were sized before it began.
    pub(super) fn is_sized(&self) -> bool {
        self.ahead.lock().is_some()
    }

    /// The step `step`'s `bytes` were counted with another's (a batch's,
    /// ADR-078), the total grown by them: the total it will count no longer
    /// waits for what that step was sized at. A step after the one under
    /// way has them apart from that one's own, which it counts still.
    pub(super) fn counted(&self, step: usize, bytes: u64) {
        self.batching.lock().insert(step, (0, bytes));
        let mut steps = self.steps.lock();
        if step > steps.ended {
            steps.others = steps.others.saturating_add(bytes);
        }
        if let Some(ahead) = self.ahead.lock().as_mut()
            && let Some(work) = ahead.work.get_mut(step)
        {
            ahead.rest = ahead.rest.saturating_sub(std::mem::take(work));
        }
    }

    /// The step `step`'s copy was finished ahead of its turn, by a batch.
    pub(super) fn finished_ahead(&self, step: usize) {
        if let Some((done, total)) = self.batching.lock().get_mut(&step) {
            *done = *total;
        }
        if self.finished_ahead.lock().insert(step) {
            self.moved_now();
        }
    }

    /// The batch copying step `step` has copied `done` bytes of it, as its
    /// transfer tells (ADR-135): said only.
    pub(super) fn writing(&self, step: usize, done: u64) {
        if let Some((was, total)) = self.batching.lock().get_mut(&step) {
            *was = done.min(*total).max(*was);
        }
    }

    /// The batch that took `steps` ended: those it did not finish copy
    /// their own when their turn comes, and are no longer said copying.
    pub(super) fn unbatch(&self, steps: &[usize]) {
        let finished = self.finished_ahead.lock();
        let mut batching = self.batching.lock();
        for step in steps {
            if !finished.contains(step) {
                batching.remove(step);
            }
        }
    }

    /// What a batch counted was not made after all (one that failed, whose
    /// steps copy their own again, ADR-078): `bytes` are no longer done nor
    /// shown, the steps `steps` no longer finished, and its speed is
    /// measured afresh from what comes next.
    pub(super) fn uncount(&self, bytes: u64, steps: &[usize]) {
        if bytes == 0 && steps.is_empty() {
            return;
        }
        let _ = self
            .done
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |done| {
                Some(done.saturating_sub(bytes))
            });
        // What `reading` said no longer holds: the bar goes back.
        self.shown.store(0, Ordering::Relaxed);
        let mut finished = self.finished_ahead.lock();
        for step in steps {
            finished.remove(step);
        }
        drop(finished);
        self.samples.lock().forget();
    }

    /// The size of what the step `step` copies, sized before the plan
    /// began, once.
    fn take_walked(&self, step: usize) -> Option<u64> {
        self.ahead
            .lock()
            .as_mut()
            .and_then(|ahead| ahead.walked.get_mut(step)?.take())
    }

    /// The plan is paused, or goes on: its speed starts again from there,
    /// and the time it ran leaves the pause out.
    /// Before it began, it is only noted: it begins held.
    pub(super) fn hold(&self, paused: bool) {
        let now = Instant::now();
        let mut clock = self.clock.lock();
        clock.paused = paused;
        if paused {
            clock.ran.stop(now);
            self.samples.lock().clear();
        } else if clock.begun {
            clock.ran.go(now);
            self.samples.lock().arm();
        }
    }

    /// How long its steps ran, its pauses left out.
    pub(super) fn ran(&self) -> Duration {
        self.clock.lock().ran.until(Instant::now())
    }

    /// How many files a mount still sends before the plan ends, and how
    /// many bytes when it says; `None` once it is done. The window is woken
    /// to say it. The speed measured while a mount sends is its sending's,
    /// what came before forgotten, and again once it is done: what it sent
    /// is what it had left going down.
    pub(super) fn sending(&self, left: Option<super::transfer::Left>) {
        let files = left.map_or(0, |left| left.files.saturating_add(1));
        let was = self.sending.swap(files, Ordering::Relaxed);
        let (began, ended) = (was == 0 && files > 0, files == 0);
        let bytes = left.and_then(|left| left.bytes);
        if began || ended {
            self.sent.store(0, Ordering::Relaxed);
        }
        let before = self
            .send_left
            .swap(bytes.unwrap_or(UNTOLD), Ordering::Relaxed);
        if let Some(bytes) = bytes
            && before != UNTOLD
            && !began
        {
            self.sent
                .fetch_add(before.saturating_sub(bytes), Ordering::Relaxed);
        }
        if ended {
            self.send_left.store(UNTOLD, Ordering::Relaxed);
        }
        let sample = self.sample(Instant::now());
        let mut samples = self.samples.lock();
        if began || ended {
            samples.arm();
        } else {
            samples.note(sample);
        }
        drop(samples);
        if let Some(wake) = &self.wake {
            wake();
        }
    }

    /// It writes into a mount that sends to its server after: its end
    /// waits for that.
    pub(super) fn sends_after(&self) {
        self.sends_after.store(true, Ordering::Relaxed);
    }

    /// A step ended: the next one's bytes count from here.
    pub(super) fn step_ended(&self) {
        let mut steps = self.steps.lock();
        if let Some(ahead) = self.ahead.lock().as_mut()
            && let Some(work) = ahead.work.get_mut(steps.ended)
        {
            ahead.rest = ahead.rest.saturating_sub(std::mem::take(work));
        }
        // Its item counts as ended now, no longer as finished ahead.
        self.finished_ahead.lock().remove(&steps.ended);
        self.batching.lock().remove(&steps.ended);
        steps.ended += 1;
        steps.done = self.done.load(Ordering::Relaxed);
        steps.total = self.total.load(Ordering::Relaxed);
        steps.others = 0;
        drop(steps);
        let now = Instant::now();
        let sample = self.sample(now);
        self.samples.lock().note(sample);
        // A plan of several steps may be counted by its steps, which go as
        // they end; one of a single step says nothing of them (a rename
        // starts no thread).
        if self.items.load(Ordering::Relaxed) > 1 {
            self.stalls_from(now);
        }
    }

    /// Count `bytes` done and wake, at most every `WAKE_EVERY`; no bytes
    /// wake no one. A copy that says 0 first (from where it went on, on a
    /// volume that clones nothing) took the wake its first bytes were
    /// owed, and with it a pause asked as they came.
    fn advance(&self, bytes: u64) {
        if bytes == 0 {
            return;
        }
        self.done.fetch_add(bytes, Ordering::Relaxed);
        self.moved_now();
    }

    /// What moved now is sampled, and the window woken to show it, at most
    /// every `WAKE_EVERY`.
    fn moved_now(&self) {
        let now = Instant::now();
        let sample = self.sample(now);
        self.samples.lock().note(sample);
        let Some(wake) = &self.wake else {
            return;
        };
        self.stall.0.moved(now, wake);
        let mut last = self.last_wake.lock();
        if last.is_none_or(|t| now.duration_since(t) >= WAKE_EVERY) {
            *last = Some(now);
            drop(last);
            wake();
        }
    }

    /// It went on at `now`: should nothing follow, the window is woken
    /// once to say so ([`Stall`]).
    fn stalls_from(&self, now: Instant) {
        if let Some(wake) = &self.wake {
            self.stall.0.moved(now, wake);
        }
    }

    /// `bytes` came from a server, not yet written where they go: measured
    /// as speed and shown as moved, not done.
    #[cfg(unix)]
    fn arrived(&self, bytes: u64) {
        self.arriving.fetch_add(bytes, Ordering::Relaxed);
        self.moved_now();
    }

    /// `bytes` that arrived are being written, and counted done right
    /// after (told first, so they are never counted twice); or were let
    /// go of, the copy stopped (all of them: `u64::MAX`).
    #[cfg(unix)]
    fn landed(&self, bytes: u64) {
        let _ = self
            .arriving
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |was| {
                Some(was.saturating_sub(bytes))
            });
    }

    /// Count `bytes` done that moved none now (a clone, a copy gone on
    /// from): done, never speed.
    fn advance_unmeasured(&self, bytes: u64) {
        self.unmeasured.fetch_add(bytes, Ordering::Relaxed);
        self.advance(bytes);
    }

    /// What it moved by `now`: what it wrote that moved now, or, while a
    /// mount sends, what that mount sent.
    fn sample(&self, now: Instant) -> speed::Sample {
        let bytes = if self.sending.load(Ordering::Relaxed) > 0 {
            self.sent.load(Ordering::Relaxed)
        } else {
            self.done
                .load(Ordering::Relaxed)
                .saturating_sub(self.unmeasured.load(Ordering::Relaxed))
                .saturating_add(self.arriving.load(Ordering::Relaxed))
        };
        speed::Sample {
            at: now,
            bytes,
            steps: self.steps.lock().ended,
        }
    }

    /// The bytes the whole plan will count, when known: those its steps
    /// ended and the step under way counted, and what those to come were
    /// sized at; for a plan not sized, of one step, what that step said.
    /// What a batch counted for the steps after the one under way is
    /// theirs: the one under way still has its own to count, as sized.
    fn planned(&self, steps: &Steps, total: u64) -> Option<u64> {
        match self.ahead.lock().as_ref() {
            Some(ahead) => {
                let own = ahead.work.get(steps.ended).copied().unwrap_or(0);
                let counted = total.saturating_sub(steps.total);
                let others = steps.others.min(counted);
                Some(
                    steps
                        .total
                        .saturating_add(others)
                        .saturating_add((counted - others).max(own))
                        .saturating_add(ahead.rest.saturating_sub(own)),
                )
            }
            None if self.items.load(Ordering::Relaxed) <= 1 => (total > 0).then_some(total),
            None => None,
        }
    }

    /// How far its steps are, for what it brings into a folder (ADR-135):
    /// the steps ended, the bytes of the one under way, as `reading` counts
    /// them (what a batch counted for the steps after it left out), and
    /// the steps a batch finished ahead of their turn. Reads no clock and
    /// moves nothing a reading keeps.
    pub(super) fn steps_read(&self) -> super::arriving::StepsRead {
        let steps = *self.steps.lock();
        let done = self
            .done
            .load(Ordering::Relaxed)
            .saturating_add(self.arriving.load(Ordering::Relaxed));
        let total = self.total.load(Ordering::Relaxed);
        let counted = total.saturating_sub(steps.total);
        super::arriving::StepsRead {
            ended: steps.ended,
            step_done: done.saturating_sub(steps.done),
            step_total: counted.saturating_sub(steps.others.min(counted)),
            ahead: self.finished_ahead.lock().iter().copied().collect(),
            batching: self.batching.lock().clone(),
        }
    }

    /// What the window and the `running` command show of the plan: how far
    /// it went, how fast it goes, and how long it has left (ADR-113).
    pub(super) fn reading(&self, paused: bool) -> Reading {
        let steps = *self.steps.lock();
        // What came from a server and waits to be written in its order (a
        // large file's ranges, `ranges`) shows as moved: it is here, and
        // a bar waiting for the range written next would stand still while
        // the others come, then jump. Never less than shown before.
        let done = self.done.load(Ordering::Relaxed);
        let done = done.saturating_add(self.arriving.load(Ordering::Relaxed));
        let done = self.shown.fetch_max(done, Ordering::Relaxed).max(done);
        let total = self.total.load(Ordering::Relaxed);
        let planned = self.planned(&steps, total);
        let items = self.items.load(Ordering::Relaxed);
        let sending = self.sending.load(Ordering::Relaxed).checked_sub(1);
        let send_left = self.send_left.load(Ordering::Relaxed);
        let then_sends = sending.is_none() && self.sends_after.load(Ordering::Relaxed);
        let left = match (sending, planned) {
            (Some(_), _) if send_left != UNTOLD => speed::Left::Bytes(send_left),
            // Its end waits for a mount to send what it wrote, untold.
            (Some(_), _) | (None, None) => speed::Left::Unknown,
            (None, _) if then_sends => speed::Left::Unknown,
            (None, Some(0)) if items > 1 => speed::Left::Steps(items.saturating_sub(steps.ended)),
            (None, Some(planned)) => speed::Left::Bytes(planned.saturating_sub(done)),
        };
        let measured = if paused {
            speed::Reading::default()
        } else {
            let now = self.sample(Instant::now());
            self.samples.lock().reading(now, left)
        };
        // Its items done: the steps ended, and those after them a batch
        // finished already.
        let finished = steps.ended + self.finished_ahead.lock().len();
        Reading {
            steps,
            finished,
            done,
            total,
            planned,
            sending,
            send_left: (sending.is_some() && send_left != UNTOLD).then_some(send_left),
            then_sends,
            measured,
        }
    }
}

/// A plan's meter, read ([`Meter::reading`]).
pub(super) struct Reading {
    pub(super) steps: Steps,
    /// The plan's items done: its steps ended, and those a batch finished
    /// before their turn.
    pub(super) finished: usize,
    pub(super) done: u64,
    pub(super) total: u64,
    pub(super) planned: Option<u64>,
    pub(super) sending: Option<usize>,
    pub(super) send_left: Option<u64>,
    pub(super) then_sends: bool,
    pub(super) measured: speed::Reading,
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
    /// Which volumes' trashes the listing holds (`trash_list`).
    #[cfg(target_os = "linux")]
    trash_listed: RefCell<Option<super::trash_list::Listed>>,
    /// The files beside a file that go where it goes (ADR-028).
    companions: Arc<crate::companions::Companions>,
    /// What a dry run's earlier steps do to the names the steps after
    /// them meet (`super::dry_run`): a later step finds them as it does
    /// when the plan is applied step by step.
    foreseen: RefCell<Foreseen>,
    /// Where the folders copies are under way in are noted, and those this
    /// plan stages in, each swept once and noted while the plan runs.
    under_way: Option<super::staging::UnderWay>,
    staged_in: RefCell<HashMap<PathBuf, Option<super::staging::Mark>>>,
    /// How this plan's copies are written through to each volume of a
    /// drive the user can unplug (`pace::Pace`), by its device: one per
    /// volume, so the drive's queue runs on from one item to the next, and
    /// [`Self::write_out`] writes each out.
    paces: RefCell<Vec<(Option<u64>, Rc<super::pace::Pace>)>>,
    /// The mounts this plan wrote into that send it to their server only
    /// after it is closed ([`Transfer::sends_later`](super::transfer::Transfer::sends_later),
    /// ADR-102), by their point, with what it wrote there since the last
    /// write-out ([`Self::written_later`]): each written out with the
    /// drives ([`Self::write_out`]), its server having what was written.
    sends: RefCell<Vec<Sending>>,
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
    /// The staging folder this plan's copies into a folder share
    /// ([`Self::staging_for`]), by folder, and how many things were staged.
    stagings: RefCell<HashMap<PathBuf, Rc<super::staging::Staging>>>,
    staged: std::cell::Cell<u64>,
    /// The file the step under way copies byte by byte, from its start,
    /// into its staging folder: what [`Self::keep_partial`] may keep.
    copying: RefCell<Option<Cut>>,
    /// What a copy cut short left, kept for Retry ([`Self::keep_partial`]),
    /// or handed to this run to go on from ([`Self::with_partial`]).
    partial: RefCell<Option<KeptPartial>>,
    /// What the copy under way goes on in, taken from [`Self::partial`]
    /// ([`Self::go_on_from`]).
    going_on: RefCell<Option<GoingOn>>,
    /// The devices the kernel would not clone or copy between, by the
    /// pair: not asked again for each file (copied onto a stick, every
    /// file is refused both).
    refused: RefCell<HashSet<(u64, u64, Kernel)>>,
    /// What a copy the kernel does not make goes through, made once.
    buffer: RefCell<Vec<u8>>,
    /// Where this plan puts files in the trash, found once.
    #[cfg(target_os = "linux")]
    trashes: super::put_trash::Trashes,
    /// What a step's check read that its apply needs too (a file's
    /// notes): the executor applies a step right after it checks it, so
    /// what the check read is what is there. Each step starts without
    /// ([`Self::step_begins`]); a dry run never applies.
    checked: RefCell<Option<Box<dyn std::any::Any>>>,
    /// What the step under way said for its plan's answer to carry
    /// ([`Self::say`]), taken by the executor when it ends.
    said: RefCell<Vec<String>>,
    /// The copies this plan made of files with several names, by the file
    /// copied: a later name of one is linked to its copy (ADR-049).
    copies: RefCell<HashMap<crate::FileId, Copied>>,
    /// The files of `copies` in the order they were copied: a step's own
    /// are those after its mark ([`Self::copied_mark`]).
    copied_order: RefCell<Vec<crate::FileId>>,
    /// Folders listed for the steps (`Self::listed`), each with its times
    /// then, the last asked at the end: at most [`LISTINGS_KEPT`].
    listings: RefCell<Vec<KeptListing>>,
    /// The states of folders compared (`builtin::state_in`), each read
    /// once while nothing changes them: within one comparison, and all
    /// through the settling of a plan's names ([`Self::keeping_states`]),
    /// which changes nothing.
    states: RefCell<HashMap<PathBuf, blake3::Hash>>,
    /// Whether `states` are kept past a comparison ([`Self::keeping_states`]).
    keeps_states: std::cell::Cell<bool>,
    /// The files of several names whose bytes this plan counted already:
    /// copied once, they are counted once.
    sized: RefCell<HashSet<crate::FileId>>,
    /// Who asks the user for a step's secret ([`Self::ask_secret`]).
    secret_asker: Option<super::SecretAsker>,
    /// The transfers plugins contributed (`transfer`, ADR-078).
    transfers: Option<Arc<super::transfer::Transfers>>,
    /// The steps before this one were looked at for a batch
    /// (`transfer::batch_ahead`).
    batched_until: std::cell::Cell<usize>,
    /// What a batch did for each copy's source, until its step takes it.
    batched: RefCell<HashMap<PathBuf, super::transfer::Batched>>,
    /// The steps before this one were looked at for a removal ahead
    /// (`transfer::remove_ahead`).
    removed_until: std::cell::Cell<usize>,
    /// What a removal ahead removed, until its step says so.
    removed: RefCell<HashSet<PathBuf>>,
    /// What a move's source was as a batch copied it.
    batched_states: RefCell<HashMap<PathBuf, CopiedAs>>,
    /// Where each reference was found ([`StepContext::resolve`]): by a
    /// batch, or ahead of a dry run, on a remote where each look is a
    /// round trip. Only for the steps looked at ([`Self::looking_ahead`]),
    /// and, found for one step (a batch's source), for that step alone.
    resolved: RefCell<HashMap<(crate::FileId, PathBuf), (PathBuf, Option<usize>)>>,
    /// The step past those [`Self::resolved`] was found for.
    resolved_until: std::cell::Cell<usize>,
    /// The step under way ([`Self::step_begins`]).
    step: std::cell::Cell<usize>,
    /// A dry run's: whether a name is in a remote's folder is read from
    /// the folder's names, listed once ([`StepContext::listing_remotes`]).
    lists_remotes: bool,
    /// Those names, by folder; `None` where the folder could not be read.
    listed: RefCell<HashMap<PathBuf, Option<HashSet<std::ffi::OsString>>>>,
    /// The names of a folder read a moment ago, which the checks run
    /// meanwhile answer from ([`StepContext::knowing`]).
    known: RefCell<Option<(PathBuf, HashSet<std::ffi::OsString>)>>,
    /// Whether each folder asked tells no case apart (`folds_case`).
    folds: RefCell<HashMap<PathBuf, bool>>,
    /// The originals of moves a batch copied from a remote, removed at the
    /// plan's end (`transfer::remove_deferred`), each with what it was as
    /// copied and where its copy went.
    removing: RefCell<Vec<(PathBuf, CopiedAs, PathBuf)>>,
    /// The folders a transfer wrote in or removed from beside the mount
    /// showing them (`transfer`): nothing watching them sees it.
    beside: RefCell<std::collections::BTreeSet<PathBuf>>,
    /// What the plan found comparing trees, shared by its dry runs and
    /// its run ([`Compared`]).
    compared: Arc<Compared>,
}

/// What a plan found comparing a source with what is there under its name
/// (`builtin::same_tree`), by the two and whether attributes count, with
/// what they and their companions were then, and whether bytes were
/// proven (`builtin::compared_state`): settling its names, its dry run
/// and its run read the bytes once while nothing of them changed, and a
/// merge's items found while their folder was compared are not compared
/// again. One a plan's run (`Executor::show`).
#[derive(Default)]
pub(super) struct Compared(
    parking_lot::Mutex<HashMap<(PathBuf, PathBuf, bool), (blake3::Hash, bool)>>,
);

impl Compared {
    /// What was found comparing `key`'s two while they were `state`.
    pub(super) fn found(
        &self,
        key: &(PathBuf, PathBuf, bool),
        state: blake3::Hash,
    ) -> Option<bool> {
        self.0
            .lock()
            .get(key)
            .and_then(|&(then, same)| (then == state).then_some(same))
    }

    /// `key`'s two, `state`, were found `same` or not.
    pub(super) fn note(&self, key: (PathBuf, PathBuf, bool), state: blake3::Hash, same: bool) {
        self.0.lock().insert(key, (state, same));
    }
}

/// How many folders' listings a plan keeps for its steps
/// (`StepContext::listed`): those of the folders its steps work in, one
/// after another.
const LISTINGS_KEPT: usize = 8;

/// The modification and change times of the folder `dir`, what tells a
/// listing of it kept (`StepContext::listed`) from one gone stale; `None`
/// when it cannot be stat-ed.
fn folder_times(dir: &Path) -> Option<(Option<SystemTime>, Option<SystemTime>)> {
    let meta = std::fs::metadata(dir).ok()?;
    Some((meta.modified().ok(), crate::fs::stat::changed(dir, &meta)))
}

/// A mount this plan wrote into that sends to its server only after a file
/// is closed (`StepContext::sends_later`): what sends there, and what the
/// plan wrote there since the last write-out.
struct Sending {
    point: PathBuf,
    transfer: Arc<dyn super::transfer::Transfer>,
    written: Vec<PathBuf>,
}

/// A folder listed for a plan's steps, and its times then
/// (`StepContext::listed`).
struct KeptListing {
    dir: PathBuf,
    times: (Option<SystemTime>, Option<SystemTime>),
    listed: Rc<super::listed::Listed>,
}

/// How many copies [`StepContext::placed`] looked at, for a test to count.
#[cfg(test)]
pub(super) static PLACED_LOOKS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// What a move's source was as a batch copied it (`builtin::state_of`:
/// `None` where it could not all be read), which its step compares before
/// removing it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct CopiedAs(pub(super) Option<blake3::Hash>);

/// What a dry run's earlier steps make and take away, by path. A path
/// made after it went is made; one that went after it was made is gone.
#[derive(Default)]
struct Foreseen {
    /// Made: a folder made, a copy, where a move or a rename goes.
    made: HashSet<PathBuf>,
    /// Of those, the folders.
    folders: HashSet<PathBuf>,
    /// How many of `made` each folder holds.
    made_in: HashMap<PathBuf, usize>,
    /// How many of `made` each folder holds under each name, its case
    /// left out (`folded`): a folder that tells no case apart meets them.
    made_folded: HashMap<(PathBuf, String), usize>,
    /// Taken away, with what is in it: moved, renamed, put in the trash,
    /// removed, and the places of its companions.
    gone: HashSet<PathBuf>,
}

impl Foreseen {
    fn make(&mut self, path: &Path, folder: bool) {
        self.gone.remove(path);
        if self.made.insert(path.to_path_buf())
            && let Some(dir) = path.parent()
        {
            *self.made_in.entry(dir.to_path_buf()).or_default() += 1;
            if let Some(key) = folded_key(path) {
                *self.made_folded.entry(key).or_default() += 1;
            }
        }
        if folder {
            self.folders.insert(path.to_path_buf());
        }
    }

    /// `path` goes, and where its companions would be (no disk read: a
    /// place that holds none is gone all the same).
    fn take(&mut self, path: &Path, companions: &crate::companions::Companions) {
        if self.made.remove(path) {
            if let Some(held) = path.parent().and_then(|dir| self.made_in.get_mut(dir)) {
                *held -= 1;
            }
            if let Some(held) = folded_key(path).and_then(|key| self.made_folded.get_mut(&key)) {
                *held -= 1;
            }
        }
        self.folders.remove(path);
        self.gone.insert(path.to_path_buf());
        if let (Some(dir), Some(name)) = (path.parent(), path.file_name()) {
            for place in companions.of(name) {
                self.gone.insert(dir.join(place));
            }
        }
    }

    /// Whether `path`, or a folder it is in, went and was not made again.
    fn went(&self, path: &Path) -> bool {
        path.ancestors()
            .take_while(|p| !self.made.contains(*p))
            .any(|p| self.gone.contains(p))
    }
}

/// `path`'s folder and its name, its case left out ([`folded`]).
fn folded_key(path: &Path) -> Option<(PathBuf, String)> {
    Some((path.parent()?.to_path_buf(), folded(path.file_name()?)))
}

/// A name with its case left out, as a volume that tells no case apart
/// (FAT, exFAT, NTFS, SMB, APFS as it comes) compares it, near enough.
pub(super) fn folded(name: &std::ffi::OsStr) -> String {
    name.to_str().map_or_else(
        || {
            name.as_encoded_bytes()
                .to_ascii_lowercase()
                .escape_ascii()
                .to_string()
        },
        str::to_lowercase,
    )
}

/// Whether the folder `dir` tells no case apart: an entry there whose
/// name has a letter of case is found under its other case, else the
/// folder itself in its own (on the same volume). A folder that cannot
/// tell (made earlier in the plan, empty, or holding no name of case, as
/// `2024`) is as the nearest folder holding it that can, on its volume: a
/// folder made takes its parent's way. Where nothing tells, taken to: a
/// dry run never lets through two names that may be one. Asked only when
/// two spellings meet.
fn folds_case(dir: &Path) -> bool {
    let Some(base) = dir
        .ancestors()
        .find(|folder| crate::fs::stat::id_of(folder).is_some())
    else {
        return true;
    };
    let volume = crate::fs::stat::id_of(base).map(|id| id.dev);
    for folder in base.ancestors() {
        if crate::fs::stat::id_of(folder).map(|id| id.dev) != volume {
            break;
        }
        if let Some(folds) = tells_case(folder) {
            return folds;
        }
    }
    true
}

/// Whether `dir` tells no case apart, as `folds_case` reads it there:
/// `None` when nothing there tells.
fn tells_case(dir: &Path) -> Option<bool> {
    let swapped = |name: &std::ffi::OsStr| {
        let name = name.to_str()?;
        let other: String = name
            .chars()
            .map(|c| {
                if c.is_lowercase() {
                    c.to_uppercase().next().unwrap_or(c)
                } else {
                    c.to_lowercase().next().unwrap_or(c)
                }
            })
            .collect();
        (other != name).then_some(other)
    };
    let found_as = |path: &Path, other: &Path| {
        let id = crate::fs::stat::id_of(path)?;
        Some(crate::fs::stat::id_of(other) == Some(id))
    };
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten().take(64) {
            if let Some(other) = swapped(&entry.file_name())
                && let Some(folds) = found_as(&entry.path(), &dir.join(other))
            {
                return Some(folds);
            }
        }
    }
    if let (Some(parent), Some(name)) = (dir.parent(), dir.file_name())
        && let Some(other) = swapped(name)
        && crate::fs::stat::id_of(parent).map(|id| id.dev)
            == crate::fs::stat::id_of(dir).map(|id| id.dev)
        && let Some(folds) = found_as(dir, &parent.join(other))
    {
        return Some(folds);
    }
    None
}

/// A copy a plan made of a file with several names: where it is now, the
/// file it is, and how many of the file's names are still to be copied,
/// so that it is forgotten once they all are.
struct Copied {
    at: PathBuf,
    copy: crate::FileId,
    left: u64,
}

/// What a copy cut short left in its staging folder in `dest_dir`
/// (`StepContext::keep_partial`, `StepContext::keep_batch`).
struct KeptPartial {
    dest_dir: PathBuf,
    kept: Kept,
}

/// A copy going on in what a run cut short left (`StepContext::go_on_from`):
/// what is under `root` may be that run's, and `cut` the file it was
/// writing.
struct GoingOn {
    root: PathBuf,
    cut: Option<Cut>,
}

/// The bytes a copy cut short must have moved to be kept for Retry: below
/// it, starting again costs little.
pub(super) const KEEP_FROM: u64 = 8 << 20;

/// The file `meta` is of, and how many names it has, when it has several:
/// Unix says, `std` on Windows does not (there each name is copied).
fn several_names(meta: &std::fs::Metadata) -> Option<(crate::FileId, u64)> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        (meta.nlink() > 1).then(|| (crate::fs::stat::file(meta.dev(), meta.ino()), meta.nlink()))
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        None
    }
}

/// What a copy asks the kernel to do between two devices.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Kernel {
    /// Share the blocks (`FICLONE`).
    Clone,
    /// Copy the bytes in the kernel (`copy_file_range`).
    Range,
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
            #[cfg(target_os = "linux")]
            trash_listed: RefCell::new(None),
            companions: Arc::default(),
            foreseen: RefCell::default(),
            under_way: None,
            staged_in: RefCell::default(),
            paces: RefCell::default(),
            waiting: RefCell::default(),
            written_out: std::cell::Cell::new((None, Duration::ZERO)),
            failed: RefCell::default(),
            stagings: RefCell::default(),
            staged: std::cell::Cell::new(0),
            copying: RefCell::default(),
            partial: RefCell::default(),
            going_on: RefCell::default(),
            refused: RefCell::default(),
            buffer: RefCell::default(),
            #[cfg(target_os = "linux")]
            trashes: super::put_trash::Trashes::default(),
            checked: RefCell::default(),
            said: RefCell::default(),
            copies: RefCell::default(),
            copied_order: RefCell::default(),
            listings: RefCell::default(),
            states: RefCell::default(),
            keeps_states: std::cell::Cell::new(false),
            sized: RefCell::default(),
            secret_asker: None,
            transfers: None,
            sends: RefCell::default(),
            batched_until: std::cell::Cell::new(0),
            removed_until: std::cell::Cell::new(0),
            removed: RefCell::default(),
            batched: RefCell::default(),
            batched_states: RefCell::default(),
            resolved: RefCell::default(),
            resolved_until: std::cell::Cell::new(0),
            step: std::cell::Cell::new(0),
            lists_remotes: false,
            listed: RefCell::default(),
            known: RefCell::default(),
            folds: RefCell::default(),
            removing: RefCell::default(),
            beside: RefCell::default(),
            compared: Arc::default(),
        }
    }

    /// Make `dst` a link to the copy this plan made already of the file
    /// `meta` is of, when it has several names (ADR-049): a copy keeps the
    /// links among what it copies, as `cp -a` does. False when there is
    /// none, or no link can be made (to another volume, on one that keeps
    /// none): the file is copied then.
    pub(super) fn link_copied(&self, meta: &std::fs::Metadata, dst: &Path) -> bool {
        let Some((file, _)) = several_names(meta) else {
            return false;
        };
        let mut made = self.copies.borrow_mut();
        let Some(copy) = made.get_mut(&file) else {
            return false;
        };
        // Still the copy made, not a file put in its place since.
        if crate::fs::stat::id_of(&copy.at) != Some(copy.copy)
            || std::fs::hard_link(&copy.at, dst).is_err()
        {
            return false;
        }
        copy.left = copy.left.saturating_sub(1);
        if copy.left == 0 {
            made.remove(&file);
        }
        true
    }

    /// The file `meta` is of was copied to `dst`: a later name of it in
    /// this plan is linked to that copy ([`Self::link_copied`]).
    pub(super) fn copied(&self, meta: &std::fs::Metadata, dst: &Path) {
        let Some((file, names)) = several_names(meta) else {
            return;
        };
        let Some(copy) = crate::fs::stat::id_of(dst) else {
            return;
        };
        self.copies.borrow_mut().insert(
            file,
            Copied {
                at: dst.to_path_buf(),
                copy,
                left: names - 1,
            },
        );
        self.copied_order.borrow_mut().push(file);
    }

    /// Where the copies a step is about to make begin among those of the
    /// plan ([`Self::placed`]).
    pub(super) fn copied_mark(&self) -> usize {
        self.copied_order.borrow().len()
    }

    /// What was staged at `staged` is at `target` now: the copies made
    /// under it, made since `mark` (a staging folder is a step's own), are
    /// found there.
    pub(super) fn placed(&self, staged: &Path, target: &Path, mark: usize) {
        let mut order = self.copied_order.borrow_mut();
        let mut made = self.copies.borrow_mut();
        let since = mark.min(order.len());
        for file in order.drain(since..) {
            #[cfg(test)]
            PLACED_LOOKS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let Some(copied) = made.get_mut(&file) else {
                continue;
            };
            if let Ok(rest) = copied.at.strip_prefix(staged) {
                // The file staged itself, or one in the folder staged.
                copied.at = if rest.as_os_str().is_empty() {
                    target.to_path_buf()
                } else {
                    target.join(rest)
                };
            }
        }
    }

    /// Whether the bytes of the file `meta` is of are to be counted: a
    /// file of several names once in this plan, as it is copied once.
    pub(super) fn sized_once(&self, meta: &std::fs::Metadata) -> bool {
        several_names(meta).is_none_or(|(file, _)| self.sized.borrow_mut().insert(file))
    }

    /// Keep what this step's check read, for its apply to take
    /// ([`Self::take_checked`]) instead of reading it again.
    pub fn keep_checked(&self, read: impl std::any::Any) {
        *self.checked.borrow_mut() = Some(Box::new(read));
    }

    /// What this step's check kept, if it kept one of this type.
    pub fn take_checked<T: std::any::Any>(&self) -> Option<T> {
        let kept = self.checked.borrow_mut().take()?;
        kept.downcast::<T>().ok().map(|kept| *kept)
    }

    /// Say `note` with what the step did, for the user: the end of its
    /// plan carries it, each note once (`ExecEvent::Finished`'s `said`:
    /// the toast after the sentence of what was done, a script's
    /// `Verdict`), never a notification of its own, which that answer
    /// would cover or follow. What it left out, how to use what it made.
    pub fn say(&self, note: impl Into<String>) {
        self.said.borrow_mut().push(note.into());
    }

    /// What the step under way said ([`Self::say`]), taken.
    pub(super) fn take_said(&self) -> Vec<String> {
        std::mem::take(&mut *self.said.borrow_mut())
    }

    /// The step `step` begins: what the last one's check kept is not its
    /// own, and the trees the last one applied to may have changed. Past
    /// the steps a look ahead found references for, where it found them is
    /// let go of: the steps since may have put another file there (a swap).
    pub(super) fn step_begins(&self, step: usize) {
        self.step.set(step);
        self.checked.borrow_mut().take();
        self.said.borrow_mut().clear();
        self.states.borrow_mut().clear();
        if step >= self.resolved_until.get() {
            self.resolved.borrow_mut().clear();
        }
    }

    /// The state of the folder `path` found since the last step began
    /// (`builtin::state_in`), if it was read whole.
    pub(super) fn state_found(&self, path: &Path) -> Option<blake3::Hash> {
        self.states.borrow().get(path).copied()
    }

    /// The folder `path` was found to be `state` (`builtin::state_in`).
    pub(super) fn state_seen(&self, path: &Path, state: blake3::Hash) {
        self.states.borrow_mut().insert(path.to_path_buf(), state);
    }

    /// A comparison ended (`builtin::same_tree`): the folders' states it
    /// read are let go of, unless they are kept ([`Self::keeping_states`]).
    /// A context asked again later (a step checked, the disk changed, the
    /// step checked again) reads them afresh.
    pub(super) fn compared_ended(&self) {
        if !self.keeps_states.get() {
            self.states.borrow_mut().clear();
        }
    }

    /// Run `f`, which changes nothing on the disk (the settling of a
    /// plan's names: a merge comparing its folder, then each of its items,
    /// level by level), keeping each folder's state read for the whole of
    /// it: each tree is read once, not once a level.
    pub(super) fn keeping_states<T>(&self, f: impl FnOnce() -> T) -> T {
        let kept = self.keeps_states.replace(true);
        let out = f();
        self.keeps_states.set(kept);
        self.compared_ended();
        out
    }

    /// The same context, going on from `partial`, what a copy this plan
    /// retries left (`Executor::retry`): its staging folder is the one
    /// this plan's copies into its destination share, and the copy of its
    /// source goes on from it ([`Self::partial_at`]).
    #[must_use]
    pub(super) fn with_partial(self, partial: Option<super::staging::Partial>) -> Self {
        let Some(partial) = partial else {
            return self;
        };
        let super::staging::Partial {
            staging,
            mark,
            dest_dir,
            kept,
        } = partial;
        // Names staged next come after its own (`n`, `batch-n`, `partial-n`).
        let staged = match &kept {
            Kept::Copy { at, .. } => at,
            Kept::Batch(batch) => &batch.partial,
        };
        let after = staged
            .file_name()
            .and_then(|n| n.to_str()?.rsplit('-').next()?.parse::<u64>().ok())
            .map_or(0, |n| n + 1);
        self.staged.set(self.staged.get().max(after));
        self.stagings
            .borrow_mut()
            .insert(dest_dir.clone(), Rc::new(staging));
        self.staged_in.borrow_mut().insert(dest_dir.clone(), mark);
        *self.partial.borrow_mut() = Some(KeptPartial { dest_dir, kept });
        self
    }

    /// Where the copy of `src` into `dest_dir` goes on from, what a run
    /// cut short left (`with_partial`): its path in the staging folder.
    /// Taken: the copy goes on in it ([`Self::going_on_in`],
    /// [`Self::partial_at`]) until it ends ([`Self::gone_on`]).
    pub(super) fn go_on_from(&self, src: &Path, dest_dir: &Path) -> Option<PathBuf> {
        let mut partial = self.partial.borrow_mut();
        let its = partial.as_ref().is_some_and(|kept| {
            kept.dest_dir == dest_dir && matches!(&kept.kept, Kept::Copy { src: s, .. } if s == src)
        });
        if !its {
            return None;
        }
        let Kept::Copy { at, cut, .. } = partial.take()?.kept else {
            return None;
        };
        *self.going_on.borrow_mut() = Some(GoingOn {
            root: at.clone(),
            cut,
        });
        Some(at)
    }

    /// What a batch of copies from `from` into `dest_dir` that a run cut
    /// short made, which this run's goes on in (`with_partial`): taken.
    pub(super) fn kept_batch(&self, from: &Path, dest_dir: &Path) -> Option<KeptBatch> {
        let mut partial = self.partial.borrow_mut();
        let its = partial.as_ref().is_some_and(|kept| {
            kept.dest_dir == dest_dir && matches!(&kept.kept, Kept::Batch(b) if b.from == from)
        });
        if !its {
            return None;
        }
        match partial.take()?.kept {
            Kept::Batch(batch) => Some(batch),
            Kept::Copy { .. } => None,
        }
    }

    /// Keep what a batch of copies into `dest_dir` that failed made, for
    /// Retry to go on in, should the plan stop (it stops at the first step
    /// whose own copy fails then: its server gone, say). What this plan's
    /// copy that fails later keeps takes its place.
    pub(super) fn keep_batch(&self, dest_dir: &Path, batch: KeptBatch) {
        *self.partial.borrow_mut() = Some(KeptPartial {
            dest_dir: dest_dir.to_path_buf(),
            kept: Kept::Batch(batch),
        });
    }

    /// The copy that went on in what a run cut short left ended: placed,
    /// or failed (and kept again, maybe: [`Self::keep_partial`]).
    pub(super) fn gone_on(&self) {
        self.going_on.borrow_mut().take();
    }

    /// Whether `dst` is in what the copy under way goes on in: what is
    /// there may be the copy a run cut short made.
    pub(super) fn going_on_in(&self, dst: &Path) -> bool {
        self.going_on
            .borrow()
            .as_ref()
            .is_some_and(|going_on| dst.starts_with(&going_on.root))
    }

    /// Whether `dst` is the file a run cut short was writing, which the
    /// copy under way goes on from ([`Self::partial_at`]).
    pub(super) fn is_cut(&self, dst: &Path) -> bool {
        self.going_on
            .borrow()
            .as_ref()
            .and_then(|going_on| going_on.cut.as_ref())
            .is_some_and(|cut| cut.at == dst)
    }

    /// The copy at `dst` of a source now `meta`, cut short earlier: how
    /// far it went if it is the start of that source as it is (its length
    /// and time unchanged); taken, so a copy goes on from it once.
    pub(super) fn partial_at(&self, dst: &Path, meta: &std::fs::Metadata) -> Option<bool> {
        let mut going_on = self.going_on.borrow_mut();
        let going_on = going_on.as_mut()?;
        if going_on.cut.as_ref().is_none_or(|cut| cut.at != dst) {
            return None;
        }
        let cut = going_on.cut.take()?;
        Some(cut.len == meta.len() && cut.modified == meta.modified().ok())
    }

    /// The step under way copies a source now `meta` byte by byte into
    /// `dst`, from its start or from what it holds: kept should it fail
    /// partway ([`Self::keep_partial`]).
    pub(super) fn copying(&self, dst: &Path, meta: &std::fs::Metadata) {
        *self.copying.borrow_mut() = Some(Cut {
            at: dst.to_path_buf(),
            len: meta.len(),
            modified: meta.modified().ok(),
        });
    }

    /// Before a step copies into its staging folder: nothing of an
    /// earlier step's is kept for it.
    pub(super) fn copy_begins(&self) {
        self.copying.borrow_mut().take();
    }

    /// Whether what the copy of `src` that failed staged at `staged` in
    /// `dest_dir` is kept for Retry to go on from: a file copied byte by
    /// byte from its start, or a folder's tree whose files were (the file
    /// it was writing, the last, gone on from), of [`KEEP_FROM`] bytes at
    /// least by what this step counted (a server gone may not say what it
    /// holds). Told by what it was copying, never by looking: the server
    /// may not answer. Kept, it stays in the staging folder, which the
    /// plan's end leaves.
    pub(super) fn keep_partial(&self, src: &Path, dest_dir: &Path, staged: &Path) -> bool {
        let moved = {
            let steps = self.meter.steps.lock();
            self.meter
                .done
                .load(Ordering::Relaxed)
                .saturating_sub(steps.done)
        };
        let Some(copying) = self.copying.borrow_mut().take() else {
            return false;
        };
        if !copying.at.starts_with(staged) || moved < KEEP_FROM {
            return false;
        }
        *self.partial.borrow_mut() = Some(KeptPartial {
            dest_dir: dest_dir.to_path_buf(),
            kept: Kept::Copy {
                at: staged.to_path_buf(),
                src: src.to_path_buf(),
                cut: Some(copying),
            },
        });
        true
    }

    /// At the plan's end, what a copy cut short left for Retry: kept by a
    /// step that failed, or handed to this run and not gone on from. Its
    /// staging folder leaves the plan's context, its lock held.
    pub(super) fn take_partial(&self) -> Option<super::staging::Partial> {
        let kept = self.partial.borrow_mut().take()?;
        let staging = self.stagings.borrow_mut().remove(&kept.dest_dir)?;
        let staging = match Rc::try_unwrap(staging) {
            Ok(staging) => staging,
            Err(shared) => {
                // Still in use: it goes with the plan, as usual.
                self.stagings
                    .borrow_mut()
                    .insert(kept.dest_dir.clone(), shared);
                return None;
            }
        };
        let mark = self
            .staged_in
            .borrow_mut()
            .get_mut(&kept.dest_dir)
            .and_then(Option::take);
        Some(super::staging::Partial {
            staging,
            mark,
            dest_dir: kept.dest_dir,
            kept: kept.kept,
        })
    }

    /// The staging folder in `dest_dir` this plan's copies there share,
    /// and a name for one more thing staged in it: made (and the folder
    /// swept) the first time, removed with the plan's context. A folder
    /// of each copy's own changed the destination six times more for
    /// every file copied (made, locked, named, unlocked, removed).
    ///
    /// # Errors
    /// When the folder cannot be made in `dest_dir`.
    pub(super) fn staging_for(
        &self,
        dest_dir: &Path,
    ) -> std::io::Result<(Rc<super::staging::Staging>, String)> {
        self.staging_in(dest_dir);
        let known = self.stagings.borrow().get(dest_dir).cloned();
        let staging = if let Some(staging) = known {
            staging
        } else {
            let made = Rc::new(super::staging::Staging::new(
                dest_dir,
                self.under_way.as_ref(),
            )?);
            self.stagings
                .borrow_mut()
                .insert(dest_dir.to_path_buf(), Rc::clone(&made));
            made
        };
        let n = self.staged.get();
        self.staged.set(n + 1);
        Ok((staging, n.to_string()))
    }

    /// Whether the kernel refused `kernel` between `devices` before.
    pub(super) fn kernel_refuses(&self, devices: Option<(u64, u64)>, kernel: Kernel) -> bool {
        devices.is_some_and(|(from, to)| self.refused.borrow().contains(&(from, to, kernel)))
    }

    /// The kernel refused `kernel` between `devices`: not asked again in
    /// this plan.
    pub(super) fn kernel_refused(&self, devices: Option<(u64, u64)>, kernel: Kernel) {
        if let Some((from, to)) = devices {
            self.refused.borrow_mut().insert((from, to, kernel));
        }
    }

    /// The buffer a copy the kernel does not make goes through.
    pub(super) fn copy_buffer(&self) -> std::cell::RefMut<'_, Vec<u8>> {
        let mut buffer = self.buffer.borrow_mut();
        if buffer.is_empty() {
            buffer.resize(1 << 20, 0);
        }
        buffer
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

    /// Whether what this plan writes into `dest_dir` reaches its server
    /// only after it is closed, its mount then noted to be written out
    /// with the drives ([`Self::write_out`], ADR-102).
    pub(super) fn sends_later(&self, dest_dir: &Path) -> bool {
        let Some(point) = crate::fs::reach::of(dest_dir).map(|remote| remote.point.clone()) else {
            return false;
        };
        if self.sends.borrow().iter().any(|s| s.point == point) {
            return true;
        }
        let Some(transfer) = self
            .transfers()
            .and_then(|transfers| transfers.sending_later(dest_dir))
        else {
            return false;
        };
        self.sends.borrow_mut().push(Sending {
            point,
            transfer,
            written: Vec::new(),
        });
        // Its end waits for that mount to send: no time left is said
        // while it writes there.
        self.meter.sends_after();
        true
    }

    /// `written` (a copy made in `dest_dir`, and what it placed beside
    /// it) is for the server of the mount there to take before the plan
    /// ends ([`Self::sends_later`], [`Self::write_out`]).
    pub(super) fn written_later(
        &self,
        dest_dir: &Path,
        written: impl IntoIterator<Item = PathBuf>,
    ) {
        let Some(point) = crate::fs::reach::of(dest_dir).map(|remote| remote.point.clone()) else {
            return;
        };
        if let Some(sending) = self
            .sends
            .borrow_mut()
            .iter_mut()
            .find(|s| s.point == point)
        {
            sending.written.extend(written);
        }
    }

    /// Wait until the drives this plan's copies were written through to
    /// have every byte, then write their volumes out (`syncfs`), names and
    /// folders included, and have the mounts that send later send what
    /// waits ([`Self::sends_later`]), and do what waited for it
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
        let stays = |e: anyhow::Error| {
            if self.waiting.borrow().is_empty() {
                e
            } else {
                e.context("what was moved stays where it was")
            }
        };
        for (_, pace) in self.paces.borrow().iter() {
            pace.write_out().map_err(stays)?;
        }
        // What each mount was given since the last write-out: a failure
        // fails this one, and the plan's end says it again.
        let sends: Vec<_> = self
            .sends
            .borrow_mut()
            .iter_mut()
            .filter(|s| !s.written.is_empty())
            .map(|s| {
                let written = std::mem::take(&mut s.written);
                (s.point.clone(), Arc::clone(&s.transfer), written)
            })
            .collect();
        for (point, transfer, written) in sends {
            let mut left = |left| self.meter.sending(Some(left));
            let sent = transfer.send(&point, &written, &self.control(), &mut left);
            self.meter.sending(None);
            sent.map_err(stays)?;
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
    /// undo, `builtin::Unmove`, takes the copy back then), and the plan
    /// goes on.
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
    /// there (`staging::sweep`). On a remote, where each staging folder is
    /// noted itself, what this machine noted there is removed instead
    /// (`UnderWay::sweep_noted`).
    pub(super) fn staging_in(&self, dest_dir: &Path) {
        if self.staged_in.borrow().contains_key(dest_dir) {
            return;
        }
        if crate::fs::reach::of(dest_dir).is_some() {
            if let Some(under_way) = &self.under_way {
                under_way.sweep_noted(dest_dir);
            }
            self.staged_in
                .borrow_mut()
                .insert(dest_dir.to_path_buf(), None);
            return;
        }
        let mark = self.under_way.as_ref().and_then(|u| u.mark(dest_dir));
        super::staging::sweep(dest_dir);
        self.staged_in
            .borrow_mut()
            .insert(dest_dir.to_path_buf(), mark);
    }

    /// Whether something can be staged in `dest_dir` (`stage`) and moved
    /// into place: not where only copies go, written in place (a camera,
    /// whose folders move nothing between them, ADR-083). What a step that
    /// stages refuses in its `check`, so it is refused before it runs.
    pub fn stages_in(dest_dir: &Path) -> bool {
        !crate::fs::reach::writes_in_place(dest_dir)
    }

    /// A staging folder in `dest_dir` for a step to make something in and
    /// put in place whole (`Staging::place`), so what it leaves cut short
    /// never looks finished: removed when dropped, and swept by a later
    /// run should this one end mid-way, as a copy's is.
    ///
    /// # Errors
    /// When the folder cannot be made in `dest_dir`, or `dest_dir` moves
    /// nothing between its folders (`fs::reach::writes_in_place`: a
    /// camera, where only copies go, written in place).
    pub fn stage(&self, dest_dir: &Path) -> std::io::Result<super::staging::Staging> {
        if !Self::stages_in(dest_dir) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "it moves nothing between its folders (a camera): only copies are made there",
            ));
        }
        self.staging_in(dest_dir);
        super::staging::Staging::new(dest_dir, self.under_way.as_ref())
    }

    /// The same context, asking the user for a step's secret through
    /// `asker`.
    #[must_use]
    pub(super) fn with_secret_asker(mut self, asker: Option<super::SecretAsker>) -> Self {
        self.secret_asker = asker;
        self
    }

    /// Ask the user for what the step needs that only they know (an
    /// archive's password): `title` the question, `accept` its button's
    /// verb. Waits for the answer, or for the plan to be cancelled.
    ///
    /// # Errors
    /// [`Unanswered`]: no one can be asked (no window: a script's plan, an
    /// agent's), or the user cancelled, or the plan was.
    pub fn ask_secret(&self, title: &str, accept: &str) -> Result<String, Unanswered> {
        let asker = self.secret_asker.as_ref().ok_or(Unanswered::NoOne)?;
        asker(title, accept, &self.cancel)
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

    pub(super) fn with_transfers(mut self, transfers: Arc<super::transfer::Transfers>) -> Self {
        self.transfers = Some(transfers);
        self
    }

    /// The transfers plugins contributed, where the executor gave them.
    pub(super) fn transfers(&self) -> Option<&super::transfer::Transfers> {
        self.transfers.as_deref()
    }

    /// The steps before this index were looked at for a batch.
    pub(super) fn batched_until(&self) -> usize {
        self.batched_until.get()
    }

    pub(super) fn batched_up_to(&self, end: usize) {
        self.batched_until.set(end);
    }

    /// The steps before this index were looked at for a removal ahead.
    pub(super) fn removed_until(&self) -> usize {
        self.removed_until.get()
    }

    pub(super) fn removed_up_to(&self, end: usize) {
        self.removed_until.set(end);
    }

    /// A removal ahead removed `path`, for its step.
    pub(super) fn removed_ahead(&self, path: &Path) {
        self.removed.borrow_mut().insert(path.to_path_buf());
    }

    /// A step renamed `from` to `to` in the folder `dir`: what the plan
    /// keeps of its names ([`Self::listed`]) follows, as of the folder's
    /// times now, so the plan's next rename there reads no listing again.
    /// Forgotten where it cannot follow.
    pub(super) fn renamed_in(&self, dir: &Path, from: &std::ffi::OsStr, to: &std::ffi::OsStr) {
        let mut kept = self.listings.borrow_mut();
        let Some(at) = kept.iter().position(|k| k.dir == dir) else {
            return;
        };
        let times = folder_times(dir);
        let found = &mut kept[at];
        let followed = match (times, Rc::get_mut(&mut found.listed)) {
            (Some(times), Some(listed)) => {
                let followed = listed.renamed(from, to);
                found.times = times;
                followed
            }
            _ => false,
        };
        if !followed {
            kept.remove(at);
        }
    }

    /// What the plan keeps of the names of `dir` ([`Self::listed`]) is
    /// not to be trusted: read again when next asked.
    pub(super) fn forget_listed(&self, dir: &Path) {
        self.listings.borrow_mut().retain(|k| k.dir != dir);
    }

    /// Whether a removal ahead removed `path` (its step's check asks).
    pub(super) fn was_removed_ahead(&self, path: &Path) -> bool {
        self.removed.borrow().contains(path)
    }

    /// Whether a removal ahead removed something its step has not taken
    /// ([`Self::take_removed_ahead`]): after the plan's steps, what the
    /// plan stopped before.
    pub(super) fn any_removed_ahead(&self) -> bool {
        !self.removed.borrow().is_empty()
    }

    /// Whether a removal ahead removed `path`, taken by its step.
    pub(super) fn take_removed_ahead(&self, path: &Path) -> bool {
        self.removed.borrow_mut().remove(path)
    }

    /// Forget where `entry` was found: what was looked at ahead may have
    /// changed since (a removal ahead cut short).
    pub(super) fn forget_resolved(&self, entry: &super::EntryRef) {
        self.resolved
            .borrow_mut()
            .remove(&(entry.id, entry.path.clone()));
    }

    /// A batch did `what` for the copy of `src`.
    pub(super) fn batched(&self, src: &Path, what: super::transfer::Batched) {
        self.batched.borrow_mut().insert(src.to_path_buf(), what);
    }

    /// Whether a batch placed the copy of `src` at `target` itself.
    pub(super) fn placed_by_batch(&self, src: &Path, target: &Path) -> bool {
        matches!(
            self.batched.borrow().get(src),
            Some(super::transfer::Batched::Placed(at, _)) if at == target
        )
    }

    /// What a batch did for the copy of `src`, taken by its step.
    pub(super) fn take_batched(&self, src: &Path) -> Option<super::transfer::Batched> {
        self.batched.borrow_mut().remove(src)
    }

    /// What batches did that no step took: the plan ended before them.
    pub(super) fn unclaimed(&self) -> Vec<(PathBuf, super::transfer::Batched)> {
        self.batched.borrow_mut().drain().collect()
    }

    /// A batch copied the source `src` of a move, which was `state` then.
    pub(super) fn batched_state(&self, src: &Path, state: CopiedAs) {
        self.batched_states
            .borrow_mut()
            .insert(src.to_path_buf(), state);
    }

    /// What the source `src` of a move was as a batch copied it, taken by
    /// its step; `None` when no batch copied it.
    pub(super) fn take_batched_state(&self, src: &Path) -> Option<CopiedAs> {
        self.batched_states.borrow_mut().remove(src)
    }

    /// The original `src` of a move, its copy whole in `dest_dir`, goes at
    /// the plan's end, if still `copied_as` then.
    pub(super) fn remove_later(&self, src: PathBuf, copied_as: CopiedAs, dest_dir: PathBuf) {
        self.removing.borrow_mut().push((src, copied_as, dest_dir));
    }

    /// The originals to remove now.
    pub(super) fn take_removing(&self) -> Vec<(PathBuf, CopiedAs, PathBuf)> {
        self.removing.take()
    }

    /// A transfer changed `folder` beside its mount: what shows it reads
    /// it again at the plan's end (`Executor::set_changed_beside`).
    pub(super) fn changed_beside(&self, folder: &Path) {
        self.beside.borrow_mut().insert(folder.to_path_buf());
    }

    /// The folders transfers changed beside their mounts.
    pub(super) fn take_changed_beside(&self) -> Vec<PathBuf> {
        self.beside.take().into_iter().collect()
    }

    /// The plan's cancel and pause, for work delegated to a transfer.
    /// Operations and transfers share this control so a delegated tree
    /// obeys the executor's pause and cancellation between its children.
    pub fn control(&self) -> super::transfer::Control {
        super::transfer::Control::new(self.cancel.clone(), Arc::clone(&self.pause))
    }

    /// What reports work done, and heeds the pause, from another thread (a
    /// helper followed from a worker).
    pub fn gauge(&self) -> Gauge {
        Gauge {
            meter: Arc::clone(&self.meter),
            pause: Arc::clone(&self.pause),
        }
    }

    /// What says, from another thread, bytes came from a server that are
    /// not yet written where they go (a large file's ranges): measured as
    /// speed and shown as moved, not done, until [`Self::landed`].
    #[cfg(unix)]
    pub(super) fn arrivals(&self) -> impl Fn(u64) + Send + Sync + Clone + use<> {
        let meter = Arc::clone(&self.meter);
        move |bytes| meter.arrived(bytes)
    }

    /// `bytes` that arrived ([`Self::arrivals`]) were written, counted done
    /// by then; `u64::MAX` lets go of all of them (the copy stopped).
    #[cfg(unix)]
    pub(super) fn landed(&self, bytes: u64) {
        self.meter.landed(bytes);
    }

    /// What reports work done from another thread (a batch's).
    pub(super) fn progress(&self) -> impl Fn(u64) + Sync + use<> {
        let meter = Arc::clone(&self.meter);
        move |bytes| meter.advance(bytes)
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

    /// Report `bytes` done that moved none now: a clone's, whose blocks the
    /// volume shares, a copy's gone on from. Counted done, never as speed.
    pub(super) fn advance_unmeasured(&self, bytes: u64) {
        self.meter.advance_unmeasured(bytes);
    }

    /// The step `step`'s `bytes` were counted with this one's (a batch's,
    /// ADR-078), announced already: what the plan was sized at no longer
    /// waits for them.
    pub(super) fn counted_ahead(&self, step: usize, bytes: u64) {
        self.meter.counted(step, bytes);
    }

    /// What says, from a batch's threads, how far the copy of a step it
    /// took has come (ADR-135, `Told::writing`): said only.
    pub(super) fn writer(&self) -> impl Fn(usize, u64) + Send + Sync + use<> {
        let meter = Arc::clone(&self.meter);
        move |step, done| meter.writing(step, done)
    }

    /// The batch that took `steps` ended: those it did not make are no
    /// longer said copying.
    pub(super) fn unbatch(&self, steps: &[usize]) {
        self.meter.unbatch(steps);
    }

    /// What a batch that failed counted is taken back (ADR-078): `bytes`
    /// done, and the steps `steps` finished, which copy their own again.
    pub(super) fn uncount(&self, bytes: u64, steps: &[usize]) {
        self.meter.uncount(bytes, steps);
    }

    /// What says, from a batch's threads, that the copy of a step after
    /// the one under way is whole: its item counts as done from then
    /// (ADR-078). Said only, never acted on.
    pub(super) fn finishing(&self) -> impl Fn(usize) + Send + Sync + use<> {
        let meter = Arc::clone(&self.meter);
        move |step| meter.finished_ahead(step)
    }

    /// The size of what the step under way copies, as the copy counts it,
    /// when the plan was sized before it began (`room::sized`): taken once,
    /// so that its tree is not walked again (on a server, a round trip a
    /// file).
    pub(super) fn take_walked(&self) -> Option<u64> {
        self.meter.take_walked(self.step.get())
    }

    /// The bytes reported done.
    #[cfg(all(test, target_os = "linux"))]
    pub(super) fn done(&self) -> u64 {
        self.meter.done.load(Ordering::Relaxed)
    }

    /// The bytes measured as moved now, what a speed is told by.
    #[cfg(all(test, target_os = "linux"))]
    pub(super) fn measured(&self) -> u64 {
        self.meter.sample(Instant::now()).bytes
    }

    /// Run `f` over the trash listing, reading it on first use in this
    /// plan, with the trashes of the volumes holding `near` (on Linux, a
    /// network mount's trash is read only where a step names a path
    /// there: `trash_list`).
    // Linux's own listing never fails: what cannot be read lists nothing.
    #[cfg_attr(target_os = "linux", allow(clippy::unnecessary_wraps))]
    pub(super) fn with_trash_near<T>(
        &self,
        near: &[&Path],
        f: impl FnOnce(&mut super::builtin::TrashListing) -> T,
    ) -> Result<T, trash::Error> {
        let mut cached = self.trash.borrow_mut();
        #[cfg(target_os = "linux")]
        {
            let mut listed = self.trash_listed.borrow_mut();
            if cached.is_none() || listed.is_none() {
                let (read, items) = super::trash_list::Listed::here();
                *listed = Some(read);
                *cached = Some(super::builtin::TrashListing::new(items));
            }
            let (listing, listed) = (
                cached.as_mut().expect("just filled"),
                listed.as_mut().expect("just filled"),
            );
            for path in near {
                listing.add(listed.near(path));
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = near;
            if cached.is_none() {
                *cached = Some(super::builtin::TrashListing::new(super::sys::trash_list()?));
            }
        }
        Ok(f(cached.as_mut().expect("just filled")))
    }

    /// The trash changed under the cached listing (an item was trashed, or
    /// put back): read again when next asked.
    /// Where this plan puts files in the trash.
    #[cfg(target_os = "linux")]
    pub(super) fn trashes(&self) -> &super::put_trash::Trashes {
        &self.trashes
    }

    pub(super) fn forget_trash(&self) {
        self.trash.borrow_mut().take();
        #[cfg(target_os = "linux")]
        self.trash_listed.borrow_mut().take();
    }

    /// The item whose record is `info` left the trash: out of the cached
    /// listing, which stays.
    pub(super) fn trash_taken(&self, info: &std::path::Path) {
        if let Some(listing) = self.trash.borrow_mut().as_mut() {
            listing.taken(info);
        }
    }

    /// A dry run's step checked as pending will do what `change` says:
    /// the steps after it find what it makes there, and what it moves,
    /// renames, puts in the trash or removes gone.
    pub(super) fn foresee(&self, change: &super::Change) {
        use super::Change;
        match change {
            Change::Create {
                path, directory, ..
            } => self.foreseen.borrow_mut().make(path, *directory),
            Change::Copy { from, to } => {
                let folder = self.is_folder(from);
                self.foreseen.borrow_mut().make(to, folder);
            }
            Change::Move { from, to } | Change::Rename { from, to } => {
                let folder = self.is_folder(from);
                let mut foreseen = self.foreseen.borrow_mut();
                foreseen.take(from, &self.companions);
                foreseen.make(to, folder);
            }
            Change::Link { to, .. } => self.foreseen.borrow_mut().make(to, false),
            Change::Restore { path } => {
                let folder = path.is_dir();
                self.foreseen.borrow_mut().make(path, folder);
            }
            Change::Trash { path } | Change::Delete { path } | Change::Remove { path } => {
                self.foreseen.borrow_mut().take(path, &self.companions);
            }
            Change::Swap { .. } | Change::Metadata { .. } => {}
        }
    }

    /// Whether `path` is a folder, or one an earlier step of this dry run
    /// makes, and no earlier step takes it away.
    pub fn is_folder(&self, path: &Path) -> bool {
        let foreseen = self.foreseen.borrow();
        foreseen.folders.contains(path) || !foreseen.went(path) && path.is_dir()
    }

    /// Whether something is at `path` (a link counts, whatever it leads
    /// to), as this dry run's earlier steps leave it.
    pub fn exists(&self, path: &Path) -> bool {
        let foreseen = self.foreseen.borrow();
        foreseen.made.contains(path)
            || self.made_respelled(&foreseen, path)
            || !foreseen.went(path) && self.there(path)
    }

    /// Whether something is at `path` now: in a folder whose names were
    /// read a moment ago ([`Self::knowing`]), or for a dry run in a
    /// remote's folder, as its names listed once say
    /// ([`Self::listing_remotes`]).
    fn there(&self, path: &Path) -> bool {
        if let Some((folder, names)) = &*self.known.borrow()
            && let Ok(rest) = path.strip_prefix(folder)
            && let Some(first) = rest.components().next()
        {
            // Below a folder of it, looked at only where that folder is.
            if !names.contains(first.as_os_str()) {
                return false;
            }
            if rest.components().nth(1).is_none() {
                return true;
            }
        }
        if self.lists_remotes
            && let (Some(folder), Some(name)) = (path.parent(), path.file_name())
            && crate::fs::reach::of(folder).is_some()
        {
            let mut listed = self.listed.borrow_mut();
            let names = listed.entry(folder.to_path_buf()).or_insert_with(|| {
                std::fs::read_dir(folder)
                    .ok()
                    .map(|entries| entries.flatten().map(|e| e.file_name()).collect())
            });
            if let Some(names) = names {
                return names.contains(name);
            }
        }
        std::fs::symlink_metadata(path).is_ok()
    }

    /// Run `f` (checks, which change nothing) answering whether a name is
    /// in `folder` from its `names`, read a moment ago: through a remote's
    /// mount, each look for a name that is not there is a round trip to
    /// the server (`transfer::remove_ahead`).
    pub(super) fn knowing<T>(
        &self,
        folder: &Path,
        names: &HashSet<std::ffi::OsString>,
        f: impl FnOnce() -> T,
    ) -> T {
        let before = self
            .known
            .replace(Some((folder.to_path_buf(), names.clone())));
        let out = f();
        self.known.replace(before);
        out
    }

    /// The same context, comparing trees as `compared` found them: the
    /// plan's dry runs and its run share one.
    #[must_use]
    pub(super) fn with_compared(mut self, compared: Arc<Compared>) -> Self {
        self.compared = compared;
        self
    }

    /// What this plan found comparing trees ([`Compared`]).
    pub(super) fn compared(&self) -> &Compared {
        &self.compared
    }

    /// The same context, for a dry run: whether a name is in a remote's
    /// folder is read from the folder's names, listed once, where each look
    /// is a round trip to the server (a look for a name that is not there
    /// is never kept: sshfs asks again each time). What the dry run's own
    /// steps make and take is foreseen as ever.
    #[must_use]
    pub fn listing_remotes(mut self) -> Self {
        self.lists_remotes = true;
        self
    }

    /// Where `entry` is: as a batch or a look ahead found it for the steps
    /// it looks at, or for this step ([`Self::resolved_as`]), else as it
    /// resolves now.
    ///
    /// # Errors
    /// What resolving it says.
    pub(super) fn resolve(&self, entry: &super::EntryRef) -> Result<PathBuf, super::ResolveError> {
        if let Some((found, only)) = self.resolved.borrow().get(&(entry.id, entry.path.clone()))
            && only.is_none_or(|step| step == self.step.get())
        {
            return Ok(found.clone());
        }
        entry.resolve_with(&|dir| self.listed(dir))
    }

    /// The names of the folder `dir`, listed once for this plan's steps
    /// while it stays as it was then (its modification and change times):
    /// a step looking for where a file gone from its name went, or how a
    /// name is spelled, reads the listing an earlier step read, where
    /// each would list the folder again. A change the folder's times do
    /// not show (two in one tick of a coarse clock, a FAT volume's) leaves
    /// a name unfound, never another file found: what is found is stat-ed.
    /// `None` when it cannot be listed. Blocking.
    pub(super) fn listed(&self, dir: &Path) -> Option<Rc<super::listed::Listed>> {
        let times = folder_times(dir)?;
        // A folder that says no time is never taken to be as it was.
        let stamped = times.0.is_some() || times.1.is_some();
        let mut kept = self.listings.borrow_mut();
        if let Some(at) = kept.iter().position(|k| k.dir == dir) {
            let found = kept.remove(at);
            if stamped && found.times == times {
                let listed = Rc::clone(&found.listed);
                kept.push(found);
                return Some(listed);
            }
        }
        drop(kept);
        let listed = Rc::new(super::listed::Listed::read(dir)?);
        if stamped {
            let mut kept = self.listings.borrow_mut();
            if kept.len() >= LISTINGS_KEPT {
                kept.remove(0);
            }
            kept.push(KeptListing {
                dir: dir.to_path_buf(),
                times,
                listed: Rc::clone(&listed),
            });
        }
        Some(listed)
    }

    /// A look ahead at the steps before `end` begins (a batch, a removal
    /// ahead, a dry run's sources): where an earlier one found references
    /// is let go of, and what this one finds ([`Self::resolved_as`]) is
    /// kept for its steps, until the step `end` begins
    /// ([`Self::step_begins`]). A later step naming the same reference
    /// resolves it afresh.
    pub(super) fn looking_ahead(&self, end: usize) {
        self.resolved.borrow_mut().clear();
        self.resolved_until.set(end);
    }

    /// `entry` was found at `path`, a moment ago, for the steps the look
    /// ahead looks at ([`Self::looking_ahead`]), or, with `only`, for that
    /// step alone: a batch's source, which its own step may move, a later
    /// step naming it then finding it where it went.
    pub(super) fn resolved_as(&self, entry: &super::EntryRef, path: PathBuf, only: Option<usize>) {
        self.resolved
            .borrow_mut()
            .insert((entry.id, entry.path.clone()), (path, only));
    }

    /// Whether a batch took the copy of `src` (made it, placed it, or
    /// counted it): it holds no companion, as the batch read its folder.
    pub(super) fn batch_took(&self, src: &Path) -> bool {
        self.batched.borrow().contains_key(src)
    }

    /// Whether an earlier step of this dry run puts something at `path`:
    /// under another case of its name too, in a folder that tells no case
    /// apart (`Photo.jpg` and `photo.jpg` copied onto a stick).
    pub fn made(&self, path: &Path) -> bool {
        let foreseen = self.foreseen.borrow();
        foreseen.made.contains(path) || self.made_respelled(&foreseen, path)
    }

    /// Whether an earlier step puts something at another case of `path`'s
    /// name, and its folder tells no case apart (asked once a folder).
    fn made_respelled(&self, foreseen: &Foreseen, path: &Path) -> bool {
        let Some(key) = folded_key(path) else {
            return false;
        };
        if foreseen.made_folded.get(&key).is_none_or(|&held| held == 0) {
            return false;
        }
        *self
            .folds
            .borrow_mut()
            .entry(key.0.clone())
            .or_insert_with(|| folds_case(&key.0))
    }

    /// The same context, the folder `dir` taken to tell no case apart
    /// or to, as a test's volume does not.
    #[cfg(test)]
    #[must_use]
    pub(super) fn folding(self, dir: &Path, folds: bool) -> Self {
        self.folds.borrow_mut().insert(dir.to_path_buf(), folds);
        self
    }

    /// Whether the folder `dir` holds nothing once this dry run's earlier
    /// steps have taken what they take from it.
    ///
    /// # Errors
    /// When the folder cannot be read (`NotFound` when it is not there).
    pub fn empty_after(&self, dir: &Path) -> std::io::Result<bool> {
        let foreseen = self.foreseen.borrow();
        if foreseen.went(dir) {
            return Err(std::io::ErrorKind::NotFound.into());
        }
        if foreseen.made_in.get(dir).is_some_and(|&held| held > 0) {
            return Ok(false);
        }
        for entry in std::fs::read_dir(dir)? {
            if !foreseen.gone.contains(&entry?.path()) {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    /// A plan sized before it began counts toward what it was sized at from
    /// its start; a batch that counts the steps after the one under way
    /// takes them out of what waits, so nothing is counted twice; a step
    /// takes the size of what it copies once (ADR-113).
    #[test]
    fn a_plan_sized_ahead_counts_toward_its_total_from_its_start() {
        let meter = Meter::new(None);
        meter.set_ahead(vec![100, 200, 300], vec![Some(100), Some(200), None]);
        meter.begin(3);
        let planned = || meter.reading(false).planned;
        assert_eq!(planned(), Some(600));
        assert_eq!(meter.take_walked(0), Some(100));
        assert_eq!(meter.take_walked(0), None, "taken once");
        assert_eq!(meter.take_walked(2), None, "not sized");
        meter.total.fetch_add(100, Ordering::Relaxed);
        meter.advance(100);
        assert_eq!(planned(), Some(600));
        meter.step_ended();
        // A batch at the second step counts the second and the third.
        meter.total.fetch_add(500, Ordering::Relaxed);
        meter.counted(1, 200);
        meter.counted(2, 300);
        assert_eq!(planned(), Some(600), "the batch's bytes counted once");
        meter.advance(500);
        meter.step_ended();
        meter.step_ended();
        assert_eq!(planned(), Some(600));
        // A batch at the first step that leaves it to itself (a file alone
        // in its folder) counts those after it: the first's own bytes still
        // wait, and are counted once it counts them.
        let left = Meter::new(None);
        left.set_ahead(vec![400, 100, 100], vec![Some(400), None, None]);
        left.begin(3);
        left.total.fetch_add(200, Ordering::Relaxed);
        left.counted(1, 100);
        left.counted(2, 100);
        assert_eq!(left.reading(false).planned, Some(600), "during the batch");
        left.advance(200);
        left.total.fetch_add(400, Ordering::Relaxed);
        assert_eq!(left.reading(false).planned, Some(600), "its own copy");
        left.advance(400);
        left.step_ended();
        assert_eq!(left.reading(false).planned, Some(600));
        // A step that moved more than it was sized at: what it moved.
        let grown = Meter::new(None);
        grown.set_ahead(vec![100, 50], vec![None, None]);
        grown.begin(2);
        grown.total.fetch_add(180, Ordering::Relaxed);
        assert_eq!(grown.reading(false).planned, Some(230));
        // Not sized, a plan of several steps knows no total; one of one
        // step, what that step said.
        let several = Meter::new(None);
        several.begin(2);
        several.total.fetch_add(10, Ordering::Relaxed);
        assert_eq!(several.reading(false).planned, None);
        let one = Meter::new(None);
        one.begin(1);
        one.total.fetch_add(10, Ordering::Relaxed);
        assert_eq!(one.reading(false).planned, Some(10));
    }

    /// What moved no byte now (a clone, a copy gone on from) is counted
    /// done, never as speed; while a mount sends, what is measured is what
    /// it sent, its bytes left going down, more to send never undoing what
    /// it sent.
    #[test]
    fn a_clone_is_done_but_no_speed_and_a_sending_is_measured_by_what_it_sent() {
        use super::super::transfer::Left;
        let meter = Meter::new(None);
        meter.begin(1);
        let moved = || meter.sample(Instant::now()).bytes;
        meter.advance_unmeasured(1 << 30);
        assert_eq!(meter.done.load(Ordering::Relaxed), 1 << 30);
        assert_eq!(moved(), 0);
        meter.advance(10);
        assert_eq!(moved(), 10);
        let left = |files, bytes| Some(Left { files, bytes });
        meter.sending(left(2, Some(1000)));
        assert_eq!(moved(), 0, "the sending measured from its start");
        meter.sending(left(1, Some(400)));
        assert_eq!(moved(), 600);
        meter.sending(left(2, Some(900)));
        assert_eq!(moved(), 600, "more to send");
        meter.sending(left(1, Some(100)));
        assert_eq!(moved(), 1400);
        let read = meter.reading(false);
        assert_eq!((read.sending, read.send_left), (Some(1), Some(100)));
        meter.sending(left(1, None));
        assert_eq!(meter.reading(false).send_left, None, "untold");
        meter.sending(None);
        assert_eq!(moved(), 10, "back to what it wrote");
        assert_eq!(meter.reading(false).sending, None);
    }

    /// What came from a server and waits to be written in its order shows
    /// as moved, never counted twice as it is written; let go of (the copy
    /// stopped), it takes nothing back from what was shown. Done is what
    /// was written.
    #[cfg(unix)]
    #[test]
    fn what_arrived_shows_as_moved_and_never_goes_back() {
        let meter = Meter::new(None);
        meter.begin(1);
        let shown = || meter.reading(false).done;
        meter.arrived(300);
        meter.arrived(200);
        assert_eq!(shown(), 500, "all that arrived");
        assert_eq!(meter.done.load(Ordering::Relaxed), 0, "none written");
        meter.landed(300);
        meter.advance(300);
        assert_eq!(shown(), 500, "written, counted once");
        meter.landed(u64::MAX);
        assert_eq!(shown(), 500, "let go of, not taken back");
        assert_eq!(meter.done.load(Ordering::Relaxed), 300);
        meter.advance(400);
        assert_eq!(shown(), 700);
    }

    /// A batch's copies count their items done as each comes whole, before
    /// their steps place them; each counted once, as its step ends too.
    #[test]
    fn a_batch_s_items_count_as_they_come_whole() {
        let meter = Meter::new(None);
        meter.begin(3);
        let finished = || meter.reading(false).finished;
        assert_eq!(finished(), 0);
        meter.finished_ahead(2);
        meter.finished_ahead(1);
        meter.finished_ahead(1);
        assert_eq!(finished(), 2, "the second and the third, once each");
        meter.step_ended();
        assert_eq!(finished(), 3);
        meter.step_ended();
        meter.step_ended();
        assert_eq!(finished(), 3, "their steps ended: counted once");
    }

    /// A batch that failed takes back what it counted: its bytes no longer
    /// done nor shown, its items no longer finished, and its speed measured
    /// afresh, as its steps copy their own again (ADR-078).
    #[test]
    fn a_batch_that_failed_takes_back_what_it_counted() {
        let meter = Meter::new(None);
        meter.set_ahead(vec![500, 500], vec![None, None]);
        meter.begin(2);
        meter.total.fetch_add(1000, Ordering::Relaxed);
        meter.counted(0, 500);
        meter.counted(1, 500);
        meter.advance(800);
        meter.finished_ahead(1);
        assert_eq!(meter.reading(false).done, 800);
        assert_eq!(meter.reading(false).finished, 1);
        meter.uncount(800, &[1]);
        let read = meter.reading(false);
        assert_eq!((read.done, read.finished), (0, 0));
        assert_eq!(read.planned, Some(1000), "its steps copy the same");
        assert_eq!(meter.sample(Instant::now()).bytes, 0);
        meter.advance(1000);
        assert_eq!(meter.reading(false).done, 1000, "counted once");
    }

    /// Paused during its checks, a plan's time runs from where it goes on;
    /// a pause after counts no more.
    #[test]
    fn a_plan_paused_before_it_began_runs_from_where_it_goes_on() {
        let meter = Meter::new(None);
        meter.hold(true);
        meter.begin(1);
        std::thread::sleep(Duration::from_millis(60));
        assert_eq!(meter.ran(), Duration::ZERO, "held from its start");
        meter.advance(10);
        assert_eq!(meter.sample(Instant::now()).bytes, 10);
        assert!(
            meter
                .samples
                .lock()
                .reading(meter.sample(Instant::now()), speed::Left::Unknown)
                == speed::Reading::default(),
            "nothing measured while held"
        );
        let before = Instant::now();
        meter.hold(false);
        std::thread::sleep(Duration::from_millis(30));
        meter.hold(true);
        let between = before.elapsed();
        std::thread::sleep(Duration::from_millis(30));
        let ran = meter.ran();
        assert!(
            ran >= Duration::from_millis(30) && ran <= between,
            "{ran:?}"
        );
        // Resumed before it began, its time starts with it.
        let early = Meter::new(None);
        early.hold(true);
        early.hold(false);
        std::thread::sleep(Duration::from_millis(40));
        assert_eq!(early.ran(), Duration::ZERO, "not begun");
        let began = Instant::now();
        early.begin(1);
        assert!(early.ran() <= began.elapsed());
    }

    /// Bytes that stop moving wake the window once, so that the speed it
    /// shows goes, and no more until more bytes come: never a loop.
    #[test]
    fn a_stall_wakes_the_window_once() {
        let woken = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&woken);
        let meter = Meter::new(Some(Arc::new(move || {
            counted.fetch_add(1, Ordering::Relaxed);
        })));
        meter.begin(1);
        let moved = Instant::now();
        meter.advance(10);
        let after_bytes = woken.load(Ordering::Relaxed);
        let deadline = moved + speed::SPEED_OVER + Duration::from_secs(5);
        while woken.load(Ordering::Relaxed) == after_bytes && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(moved.elapsed() >= speed::SPEED_OVER, "not before");
        assert_eq!(woken.load(Ordering::Relaxed), after_bytes + 1, "woken once");
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(
            woken.load(Ordering::Relaxed),
            after_bytes + 1,
            "and no more"
        );
        drop(meter);
        // A step that ends moves nothing: a plan of one step, counted by
        // its bytes alone, arms nothing; one of several is counted by its
        // steps too.
        let woken = || Some(Arc::new(|| {}) as Waker);
        let one = Meter::new(woken());
        one.begin(1);
        one.step_ended();
        assert!(!one.stall.0.state.lock().started, "no thread");
        let several = Meter::new(woken());
        several.begin(3);
        several.step_ended();
        assert!(several.stall.0.state.lock().started);
    }

    /// Writing into a mount that sends to its server after, its end waits
    /// for that: said, and no time left told meanwhile.
    #[test]
    fn writing_into_a_mount_that_sends_after_says_so() {
        let meter = Meter::new(None);
        meter.set_ahead(vec![1000], vec![None]);
        meter.begin(1);
        assert!(!meter.reading(false).then_sends);
        meter.sends_after();
        let read = meter.reading(false);
        assert!(read.then_sends);
        assert_eq!(read.measured.left, None);
    }

    /// What an earlier step makes is met under another case of its name
    /// where the folder tells no case apart, and only there.
    #[test]
    fn a_name_made_meets_its_other_case_where_case_is_not_told_apart() {
        let stick = Path::new("/media/stick");
        let made = || {
            let ctx = StepContext::detached();
            ctx.foresee(&crate::op::Change::Copy {
                from: "/disk/Été Photo.jpg".into(),
                to: stick.join("Été Photo.jpg"),
            });
            ctx
        };
        let ctx = made().folding(stick, true);
        assert!(ctx.made(&stick.join("été photo.jpg")) && ctx.exists(&stick.join("ÉTÉ PHOTO.JPG")));
        assert!(!ctx.made(&stick.join("other.jpg")));
        let ctx = made().folding(stick, false);
        assert!(!ctx.made(&stick.join("été photo.jpg")));
        assert!(ctx.made(&stick.join("Été Photo.jpg")));
    }

    /// A folder's case rule is read from what it holds: here, a disk's
    /// that tells case apart.
    #[test]
    fn a_folder_that_tells_case_apart_says_so() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("Mixed Case.txt"), "x").unwrap();
        // Linux's tmp folders tell case apart; other systems' may not.
        if cfg!(target_os = "linux") {
            assert!(!folds_case(tmp.path()));
            // An empty folder holding no name of case, and one still to be
            // made in it, are as the folder holding them.
            let empty = tmp.path().join("2024");
            std::fs::create_dir(&empty).unwrap();
            assert!(!folds_case(&empty));
            assert!(!folds_case(&empty.join("made in the plan")));
        }
    }

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

    /// Entries gone from a folder are looked for in one listing of it
    /// while it stays as it was; a rename in place since changes its
    /// times, and the file renamed is found under its new name.
    #[cfg(unix)]
    #[test]
    fn gone_entries_are_looked_for_in_one_listing_a_folder() {
        use std::sync::atomic::Ordering;

        use super::super::listed::READ;
        let base = dirs::home_dir().unwrap().join(".cache/skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        let tmp = tempfile::tempdir_in(base).unwrap();
        let name = |i: usize| tmp.path().join(format!("it's {i}.txt"));
        let refs: Vec<super::super::EntryRef> = (0..100)
            .map(|i| {
                std::fs::write(name(i), b"").unwrap();
                super::super::EntryRef::from_path(name(i)).unwrap()
            })
            .collect();
        std::fs::write(tmp.path().join("kept.txt"), b"").unwrap();
        for i in 0..99 {
            std::fs::remove_file(name(i)).unwrap();
        }
        let ctx = StepContext::detached();
        let before = READ.load(Ordering::Relaxed);
        for entry in &refs[..99] {
            assert!(matches!(
                ctx.resolve(entry),
                Err(super::super::ResolveError::Gone(_))
            ));
        }
        assert_eq!(READ.load(Ordering::Relaxed) - before, 1, "one listing");
        let renamed = tmp.path().join("it's renamed.txt");
        std::fs::rename(name(99), &renamed).unwrap();
        // A coarse clock may give the rename the listing's tick: the
        // folder's time moved on, as a later tick gives it.
        std::fs::File::open(tmp.path())
            .unwrap()
            .set_modified(SystemTime::now() + Duration::from_secs(1))
            .unwrap();
        assert_eq!(ctx.resolve(&refs[99]).unwrap(), renamed);
        assert_eq!(READ.load(Ordering::Relaxed) - before, 2, "listed again");
    }

    /// Where a look ahead found a reference holds for the steps it looked
    /// at, and no further: a step after them finds the file where the
    /// steps between put it (two names swapped), never the other file now
    /// at its name.
    #[cfg(unix)]
    #[test]
    fn a_reference_found_ahead_holds_only_for_the_steps_looked_at() {
        let base = dirs::home_dir().unwrap().join(".cache/skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        let tmp = tempfile::tempdir_in(base).unwrap();
        let a = tmp.path().join("it's a.txt");
        let b = tmp.path().join("it's b.txt");
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();
        let entry = super::super::EntryRef::from_path(&a).unwrap();
        let ctx = StepContext::detached();
        ctx.looking_ahead(1);
        ctx.resolved_as(&entry, a.clone(), None);
        ctx.step_begins(0);
        assert_eq!(
            ctx.resolve(&entry).unwrap(),
            a,
            "as the look ahead found it"
        );
        let parked = tmp.path().join("parked");
        std::fs::rename(&a, &parked).unwrap();
        std::fs::rename(&b, &a).unwrap();
        std::fs::rename(&parked, &b).unwrap();
        ctx.step_begins(1);
        assert_eq!(ctx.resolve(&entry).unwrap(), b, "where the swap put it");
    }

    /// Where a batch found a step's source holds for that step alone: a
    /// later step of the same run naming the same file, which the first
    /// moved, finds it where it went, never at the name it left.
    #[cfg(unix)]
    #[test]
    fn a_source_found_for_one_step_is_found_afresh_by_the_next() {
        let base = dirs::home_dir().unwrap().join(".cache/skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        let tmp = tempfile::tempdir_in(base).unwrap();
        let a = tmp.path().join("it's a.txt");
        std::fs::write(&a, b"a").unwrap();
        let entry = super::super::EntryRef::from_path(&a).unwrap();
        let ctx = StepContext::detached();
        ctx.looking_ahead(2);
        ctx.resolved_as(&entry, a.clone(), Some(0));
        ctx.step_begins(0);
        assert_eq!(ctx.resolve(&entry).unwrap(), a, "as the batch found it");
        // The first step moves it, under another name.
        let moved = tmp.path().join("l'été.txt");
        std::fs::rename(&a, &moved).unwrap();
        ctx.step_begins(1);
        assert_eq!(ctx.resolve(&entry).unwrap(), moved, "where it went");
    }
}
