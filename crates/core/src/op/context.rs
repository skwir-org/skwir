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

use super::staging::{Cut, Kept, KeptBatch};
use crate::cancel::CancelToken;
use crate::location::Waker;

/// The error a step returns when it stopped because its plan was cancelled.
/// The executor reports the plan as cancelled, not failed.
#[derive(Debug, thiserror::Error)]
#[error("cancelled")]
pub struct Cancelled;

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
    /// The copies this plan made of files with several names, by the file
    /// copied: a later name of one is linked to its copy (ADR-049).
    copies: RefCell<HashMap<crate::FileId, Copied>>,
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
    /// round trip.
    resolved: RefCell<HashMap<(crate::FileId, PathBuf), PathBuf>>,
    /// A dry run's: whether a name is in a remote's folder is read from
    /// the folder's names, listed once ([`StepContext::listing_remotes`]).
    lists_remotes: bool,
    /// Those names, by folder; `None` where the folder could not be read.
    listed: RefCell<HashMap<PathBuf, Option<HashSet<std::ffi::OsString>>>>,
    /// The originals of moves a batch copied from a remote, removed at the
    /// plan's end (`transfer::remove_deferred`), each with what it was as
    /// copied and where its copy went.
    removing: RefCell<Vec<(PathBuf, CopiedAs, PathBuf)>>,
    /// The folders a transfer wrote in or removed from beside the mount
    /// showing them (`transfer`): nothing watching them sees it.
    beside: RefCell<std::collections::BTreeSet<PathBuf>>,
}

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
        }
        if folder {
            self.folders.insert(path.to_path_buf());
        }
    }

    /// `path` goes, and where its companions would be (no disk read: a
    /// place that holds none is gone all the same).
    fn take(&mut self, path: &Path, companions: &crate::companions::Companions) {
        if self.made.remove(path)
            && let Some(held) = path.parent().and_then(|dir| self.made_in.get_mut(dir))
        {
            *held -= 1;
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
            copies: RefCell::default(),
            sized: RefCell::default(),
            secret_asker: None,
            transfers: None,
            batched_until: std::cell::Cell::new(0),
            removed_until: std::cell::Cell::new(0),
            removed: RefCell::default(),
            batched: RefCell::default(),
            batched_states: RefCell::default(),
            resolved: RefCell::default(),
            lists_remotes: false,
            listed: RefCell::default(),
            removing: RefCell::default(),
            beside: RefCell::default(),
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
    }

    /// What was staged at `staged` is at `target` now: the copies made
    /// under it are found there.
    pub(super) fn placed(&self, staged: &Path, target: &Path) {
        for copied in self.copies.borrow_mut().values_mut() {
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

    /// Another step begins: what the last one's check kept is not its own.
    pub(super) fn step_begins(&self) {
        self.checked.borrow_mut().take();
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
        // What is staged is moved into place, which a camera does not do
        // between its folders: only copies go there, in place (ADR-083).
        if crate::fs::reach::writes_in_place(dest_dir) {
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

    /// Whether a removal ahead removed `path` (its step's check asks).
    pub(super) fn was_removed_ahead(&self, path: &Path) -> bool {
        self.removed.borrow().contains(path)
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

    /// The plan's cancel and pause, for a batch's transfer.
    pub(super) fn control(&self) -> super::transfer::Control {
        super::transfer::Control::new(self.cancel.clone(), Arc::clone(&self.pause))
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

    /// The bytes reported done.
    #[cfg(all(test, target_os = "linux"))]
    pub(super) fn done(&self) -> u64 {
        self.meter.done.load(Ordering::Relaxed)
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
        foreseen.made.contains(path) || !foreseen.went(path) && self.there(path)
    }

    /// Whether something is at `path` now: for a dry run, in a remote's
    /// folder, as its names listed once say ([`Self::listing_remotes`]).
    fn there(&self, path: &Path) -> bool {
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

    /// Where `entry` is: as a batch or a look ahead found it in this plan
    /// ([`Self::resolved_as`]), else as it resolves now.
    ///
    /// # Errors
    /// What resolving it says.
    pub(super) fn resolve(&self, entry: &super::EntryRef) -> Result<PathBuf, super::ResolveError> {
        if let Some(found) = self.resolved.borrow().get(&(entry.id, entry.path.clone())) {
            return Ok(found.clone());
        }
        entry.resolve()
    }

    /// `entry` was found at `path`, a moment ago.
    pub(super) fn resolved_as(&self, entry: &super::EntryRef, path: PathBuf) {
        self.resolved
            .borrow_mut()
            .insert((entry.id, entry.path.clone()), path);
    }

    /// Whether a batch took the copy of `src` (made it, placed it, or
    /// counted it): it holds no companion, as the batch read its folder.
    pub(super) fn batch_took(&self, src: &Path) -> bool {
        self.batched.borrow().contains_key(src)
    }

    /// Whether an earlier step of this dry run puts something at `path`.
    pub fn made(&self, path: &Path) -> bool {
        self.foreseen.borrow().made.contains(path)
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
