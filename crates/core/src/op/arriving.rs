//! What a plan brings into a folder while it waits or runs (ADR-135): each
//! of its items that puts something there, under the name it will have, and
//! what it will be, so that the folder shows it where it will land before it
//! does. Kept by the executor, from the plan asked until it runs, then from
//! the plan as it runs (its steps unfolded and its names settled), and let
//! go of when its run ends.
//!
//! Nothing here reads the destination: what an item will be is what it is
//! made from (a copy's or a move's source), stat-ed once on a worker as the
//! plan is asked; what a step makes (an archive extracted) is what it says.
//! How far each is is read from its plan's meter as it is asked, never
//! kept: no event of its own, no wake of its own beyond the plan's.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Instant, SystemTime};

use parking_lot::Mutex;

use super::{Change, ItemSaid, Plan, PlanId};
use crate::cancel::CancelToken;
use crate::entry::{Entry, EntryId, EntryKind, Provenance, Value, keys};

/// One item a plan brings into a folder.
#[derive(Debug)]
pub struct Arrival {
    pub plan: PlanId,
    /// Where it lands.
    pub folder: PathBuf,
    /// Its name there, as settled once its plan runs (`photos (copy)`).
    pub name: OsString,
    /// What it is made from: what a copy or a move takes; none for what a
    /// step makes (an archive extracted, a folder made, a file put back
    /// from the trash).
    pub from: Option<PathBuf>,
    /// What it will be, as far as is known before it lands: what it is
    /// made from, stat-ed, under its name there (its kind, its size, its
    /// times, which a copy keeps); what a step makes, by what it says.
    /// What a view sorts it by and draws it as.
    pub like: Entry,
    /// Its steps among those of its plan as it runs; `None` while the plan
    /// waits, or checks itself before its steps.
    steps: Option<Range<usize>>,
    /// When its plan was asked.
    pub asked: Instant,
}

/// How far an item has come (`Executor::arriving_in`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Arrives {
    /// Its plan waits its turn, or its own steps wait for those before
    /// them.
    Waiting,
    /// Its own steps began and have moved nothing yet: a folder being
    /// sized, a plan checking itself.
    Starting,
    /// This far, from 0 to 1: its bytes moved, or its steps ended where
    /// it has several.
    Moving(f32),
    /// Copied by a batch with others (ADR-078) whose transfer says nothing
    /// of each until it is whole: the batch is this far, from 0 to 1.
    Copying(f32),
}

/// An item arriving, and how far it has come.
#[derive(Clone, Debug)]
pub struct Arriving {
    pub arrival: Arc<Arrival>,
    pub state: Arrives,
    /// Its plan is paused.
    pub paused: bool,
}

/// What a plan's run read of its steps for its items (`Meter::steps_read`).
#[derive(Clone, Debug, Default)]
pub(super) struct StepsRead {
    /// The steps ended, the next one under way.
    pub ended: usize,
    /// The bytes of the step under way: moved, and announced.
    pub step_done: u64,
    pub step_total: u64,
    /// The steps after it a batch finished ahead of their turn (ADR-078).
    pub ahead: HashSet<usize>,
    /// The steps a batch under way took: their bytes copied so far, where
    /// its transfer tells them, and in all.
    pub batching: HashMap<usize, Batching>,
}

/// A step a batch under way took (ADR-078), as its meter keeps it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Batching {
    /// Its bytes copied so far, as its transfer tells them.
    pub done: u64,
    /// Its bytes in all.
    pub total: u64,
    /// Whether the transfer copying it tells each name it copies
    /// (`Transfer::tells_each`): then nothing told yet has not begun.
    pub tells: bool,
}

/// Where `arrival` stands as its plan's run read it, `None` once it landed
/// (its steps ended) or where its run ended.
pub(super) fn state_of(arrival: &Arrival, read: Option<&StepsRead>) -> Option<Arrives> {
    let Some(read) = read else {
        // Its plan not running: waiting, unless it ran (its steps known),
        // and its run is over.
        return arrival.steps.is_none().then_some(Arrives::Waiting);
    };
    let Some(steps) = arrival.steps.clone() else {
        // Its plan checks itself before its steps.
        return Some(Arrives::Starting);
    };
    if steps.end <= read.ended || steps.is_empty() {
        return None;
    }
    if steps.clone().all(|step| read.ahead.contains(&step)) {
        return Some(Arrives::Moving(1.));
    }
    // Taken by a batch: by its own bytes where its transfer tells them;
    // nothing told yet by one that tells each, not begun; else by the
    // batch's, which the step under way runs.
    let batched: Vec<Batching> = steps
        .clone()
        .filter_map(|step| read.batching.get(&step).copied())
        .collect();
    if !batched.is_empty() {
        let (done, total) = batched.iter().fold((0u64, 0u64), |(d, t), b| {
            (d.saturating_add(b.done), t.saturating_add(b.total))
        });
        if done > 0 && total > 0 {
            return Some(Arrives::Moving(
                (done as f64 / total as f64).clamp(0., 1.) as f32
            ));
        }
        if batched.iter().any(|b| b.tells) {
            return Some(Arrives::Waiting);
        }
        let batch = if read.step_total > 0 {
            read.step_done.min(read.step_total) as f64 / read.step_total as f64
        } else {
            0.
        };
        return Some(Arrives::Copying(batch as f32));
    }
    if steps.start > read.ended {
        return Some(Arrives::Waiting);
    }
    let within = if read.step_total > 0 {
        read.step_done.min(read.step_total) as f64 / read.step_total as f64
    } else {
        0.
    };
    let ended = (read.ended - steps.start) as f64;
    if ended == 0. && read.step_done == 0 {
        return Some(Arrives::Starting);
    }
    let len = steps.len() as f64;
    Some(Arrives::Moving(
        ((ended + within) / len).clamp(0., 1.) as f32
    ))
}

/// An item a plan's steps put somewhere: its steps, where it lands, what it
/// is made from, and what is made where nothing is (a `Create`'s).
struct Item {
    steps: Range<usize>,
    placed: PathBuf,
    from: Option<PathBuf>,
    made: Option<Made>,
    /// A copy: a file made anew, dated as it is made where it was
    /// created and read (a move to another volume is one too, told by
    /// what it is made from).
    copied: bool,
}

/// What a step makes new, as it says it.
#[derive(Clone, Copy)]
enum Made {
    Folder,
    File,
}

/// What a change brings ([`brought`]): what it is made from, what is made
/// where nothing is, whether it is copied, and where it lands.
struct Brings<'a> {
    from: Option<PathBuf>,
    made: Option<Made>,
    copied: bool,
    to: &'a Path,
}

/// What `change` brings, if it brings something. What a link or a rename
/// makes is not brought: it is there at once, as a move within its folder
/// is.
fn brought(change: &Change) -> Option<Brings<'_>> {
    let brings = |from: &Path, copied, to| Brings {
        from: Some(from.to_path_buf()),
        made: None,
        copied,
        to,
    };
    match change {
        Change::Copy { from, to } => Some(brings(from, true, to)),
        Change::Move { from, to } => {
            (from.parent() != to.parent()).then(|| brings(from, false, to))
        }
        Change::Create {
            path, directory, ..
        } => Some(Brings {
            from: None,
            made: Some(if *directory { Made::Folder } else { Made::File }),
            copied: false,
            to: path,
        }),
        Change::Restore { path } => Some(Brings {
            from: None,
            made: None,
            copied: false,
            to: path,
        }),
        _ => None,
    }
}

/// What `plan` puts somewhere, item by item: by its items where they say
/// where each was put (`Items::placed`), else each step that brings
/// something ([`brought`]).
fn items_of(plan: &Plan) -> Vec<Item> {
    let changes: Vec<Change> = plan.steps.iter().map(|step| step.describe()).collect();
    let item = |steps, brings: Brings<'_>, placed: &Path| Item {
        steps,
        placed: placed.to_path_buf(),
        from: brings.from,
        made: brings.made,
        copied: brings.copied,
    };
    if let Some(items) = plan.items.as_deref()
        && items.placed.len() == items.count
        && let Some(each) = items.each(changes.len())
    {
        return each
            .into_iter()
            .zip(&items.placed)
            .filter_map(|((steps, said), placed)| {
                let change = match said {
                    ItemSaid::Step(at) => changes.get(*at)?,
                    ItemSaid::Asked(change) => change,
                };
                Some(item(steps, brought(change)?, placed))
            })
            .collect();
    }
    changes
        .iter()
        .enumerate()
        .filter_map(|(at, change)| {
            let brings = brought(change)?;
            let to = brings.to;
            Some(item(at..at + 1, brings, to))
        })
        .collect()
}

/// What a view reads of what is brought: what it sorts, groups and filters
/// by, and what it says of it; its notes (tags, rating, comment), which a
/// copy keeps, where they were read (`Executor::set_reader`). The rest is
/// not kept: a paste of ten thousand files keeps ten thousand of these
/// while it runs.
const KEPT: [&str; 11] = [
    keys::SIZE,
    keys::MODIFIED,
    keys::CREATED,
    keys::ACCESSED,
    keys::MODE,
    keys::UID,
    keys::GID,
    keys::TAGS,
    keys::TAG_COLORS,
    keys::RATING,
    keys::COMMENT,
];

/// `entry` as it will be at `placed`, under `name`, with only what a view
/// reads of it ([`KEPT`]), read as it was (`Metadata::is_filled`). Made
/// anew (`made_at`: a copy, a move to another volume), it was created and
/// last read then.
fn moved_to(entry: Entry, placed: &Path, name: &OsString, made_at: Option<SystemTime>) -> Entry {
    let mut like = Entry::new(
        EntryId::at_path(entry.id.file, placed),
        name.clone(),
        entry.kind,
    );
    like.target = entry.target;
    like.hidden = entry.hidden;
    for key in KEPT {
        if let Some(field) = entry.metadata.get(key) {
            like.metadata
                .set(key, field.value.clone(), field.provenance.clone());
        }
    }
    if let Some(at) = made_at {
        for key in [keys::CREATED, keys::ACCESSED] {
            like.metadata
                .set(key, Value::Time(at), Provenance::Filesystem);
        }
    }
    if entry.metadata.is_filled() {
        like.metadata.mark_filled();
    }
    like
}

/// What `placed` will be, made from `from` (read by `read`), or made new:
/// a folder or a file, dated now. What cannot be read is a file, nothing
/// else known of it.
fn like_of(
    item: &Item,
    name: &OsString,
    made_at: impl Fn(&Entry) -> Option<SystemTime>,
    read: &mut impl FnMut(&Path) -> Option<Entry>,
) -> Entry {
    if let Some(from) = item.from.as_deref().and_then(&mut *read) {
        let at = made_at(&from);
        return moved_to(from, &item.placed, name, at);
    }
    let kind = match item.made {
        Some(Made::Folder) => EntryKind::Directory,
        Some(Made::File) | None => EntryKind::File,
    };
    let mut like = Entry::new(EntryId::UNKNOWN, name.clone(), kind);
    if item.made.is_some() {
        let now = Value::Time(SystemTime::now());
        for key in [keys::MODIFIED, keys::CREATED, keys::ACCESSED] {
            like.metadata.set(key, now.clone(), Provenance::Filesystem);
        }
    }
    like
}

/// The arrivals of `plan`, asked at `asked`: their steps among its own when
/// `runs` (the plan as it runs), else none yet. What each is made from is
/// taken from `known` where it was read before, else `read`. A copy, and a
/// move to another volume (what it is made from on another device than
/// its folder, each folder stat-ed once), are dated as made when asked.
pub(super) fn arrivals_of(
    plan: &Plan,
    runs: bool,
    asked: Instant,
    known: &HashMap<PathBuf, Entry>,
    mut read: impl FnMut(&Path) -> Option<Entry>,
) -> Vec<Arc<Arrival>> {
    let asked_at = SystemTime::now()
        .checked_sub(asked.elapsed())
        .unwrap_or_else(SystemTime::now);
    let mut devices: HashMap<PathBuf, Option<u64>> = HashMap::new();
    items_of(plan)
        .into_iter()
        .filter_map(|item| {
            let folder = item.placed.parent()?.to_path_buf();
            let name = item.placed.file_name()?.to_os_string();
            let device = devices
                .entry(folder.clone())
                .or_insert_with(|| crate::fs::stat::id_of(&folder).map(|id| id.dev));
            let device = *device;
            let made_at = |from: &Entry| {
                let anew = item.copied || device.is_some_and(|dev| dev != from.id.file.dev);
                anew.then_some(asked_at)
            };
            let like = match item.from.as_deref().and_then(|from| known.get(from)) {
                Some(known) => {
                    let at = made_at(known);
                    moved_to(known.clone(), &item.placed, &name, at)
                }
                None => like_of(&item, &name, made_at, &mut read),
            };
            Some(Arc::new(Arrival {
                plan: plan.id,
                folder,
                name,
                from: item.from,
                like,
                steps: runs.then_some(item.steps),
                asked,
            }))
        })
        .collect()
}

/// What the executor keeps of what each plan brings (module documentation).
#[derive(Default)]
pub(super) struct Index {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    plans: HashMap<PlanId, Record>,
    /// The plans bringing something into each folder.
    folders: HashMap<PathBuf, Vec<PlanId>>,
}

/// A plan asked: its token, when it was asked, whether its arrivals are
/// those of its run, and what they are.
struct Record {
    cancel: CancelToken,
    asked: Instant,
    runs: bool,
    arrivals: Arc<[Arc<Arrival>]>,
}

impl Index {
    /// `plan` was asked, cancelled by `cancel`: its arrivals follow
    /// ([`Index::fill`]). When it was asked.
    pub(super) fn asked(&self, plan: PlanId, cancel: &CancelToken) -> Instant {
        let mut inner = self.inner.lock();
        let record = inner.plans.entry(plan).or_insert_with(|| Record {
            cancel: cancel.clone(),
            asked: Instant::now(),
            runs: false,
            arrivals: Arc::from([]),
        });
        record.asked
    }

    /// When `plan` was asked, while it is kept; and what its arrivals
    /// were read as, by what each is made from.
    pub(super) fn known(&self, plan: PlanId) -> Option<(Instant, HashMap<PathBuf, Entry>)> {
        let inner = self.inner.lock();
        let record = inner.plans.get(&plan)?;
        let known = record
            .arrivals
            .iter()
            .filter_map(|a| Some((a.from.clone()?, a.like.clone())))
            .collect();
        Some((record.asked, known))
    }

    /// `plan` brings `arrivals`, those of its run where `runs`: kept only
    /// while it is (asked and not ended), and never the asked plan's over
    /// its run's. Whether they were kept.
    pub(super) fn fill(&self, plan: PlanId, arrivals: Vec<Arc<Arrival>>, runs: bool) -> bool {
        let mut inner = self.inner.lock();
        let Inner { plans, folders } = &mut *inner;
        let Some(record) = plans.get_mut(&plan) else {
            return false;
        };
        if record.runs && !runs {
            return false;
        }
        for folder in record.arrivals.iter().map(|a| &a.folder) {
            if let Some(those) = folders.get_mut(folder) {
                those.retain(|p| *p != plan);
                if those.is_empty() {
                    folders.remove(folder);
                }
            }
        }
        let mut told = HashSet::new();
        for arrival in &arrivals {
            if told.insert(&arrival.folder) {
                folders
                    .entry(arrival.folder.clone())
                    .or_default()
                    .push(plan);
            }
        }
        record.runs = runs;
        record.arrivals = arrivals.into();
        true
    }

    /// `plan`'s run ended: what it brought is let go of. Whether it
    /// brought anything.
    pub(super) fn forget(&self, plan: PlanId) -> bool {
        let mut inner = self.inner.lock();
        let Inner { plans, folders } = &mut *inner;
        let Some(record) = plans.remove(&plan) else {
            return false;
        };
        for arrival in record.arrivals.iter() {
            if let Some(those) = folders.get_mut(&arrival.folder) {
                those.retain(|p| *p != plan);
                if those.is_empty() {
                    folders.remove(&arrival.folder);
                }
            }
        }
        !record.arrivals.is_empty()
    }

    /// The folders `plan` brings something into, each once, in its order.
    pub(super) fn folders_of(&self, plan: PlanId) -> Vec<PathBuf> {
        let inner = self.inner.lock();
        let mut seen = HashSet::new();
        inner.plans.get(&plan).map_or_else(Vec::new, |record| {
            record
                .arrivals
                .iter()
                .filter(|a| seen.insert(&a.folder))
                .map(|a| a.folder.clone())
                .collect()
        })
    }

    /// What the plans not cancelled bring into `folder`, in the order they
    /// were asked and their items in theirs.
    pub(super) fn in_folder(&self, folder: &Path) -> Vec<Arc<Arrival>> {
        let inner = self.inner.lock();
        let Some(plans) = inner.folders.get(folder) else {
            return Vec::new();
        };
        let mut records: Vec<&Record> = plans
            .iter()
            .filter_map(|plan| inner.plans.get(plan))
            .filter(|record| !record.cancel.is_cancelled())
            .collect();
        records.sort_by_key(|record| record.asked);
        records
            .into_iter()
            .flat_map(|record| record.arrivals.iter())
            .filter(|arrival| arrival.folder == folder)
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arrival(steps: Option<Range<usize>>) -> Arrival {
        Arrival {
            plan: PlanId(1),
            folder: PathBuf::from("/dest"),
            name: "a".into(),
            from: None,
            like: Entry::new(EntryId::UNKNOWN, "a", EntryKind::File),
            steps,
            asked: Instant::now(),
        }
    }

    fn read(ended: usize, done: u64, total: u64, ahead: &[usize]) -> StepsRead {
        StepsRead {
            ended,
            step_done: done,
            step_total: total,
            ahead: ahead.iter().copied().collect(),
            batching: HashMap::new(),
        }
    }

    /// Taken by a batch, an item copies with it: by its own bytes where
    /// the transfer tells them, else at the batch's share; a transfer that
    /// tells each not having told it yet, it waits; once the batch let it
    /// go unmade, it waits for its turn again.
    #[test]
    fn an_item_a_batch_took_copies_with_it() {
        let item = arrival(Some(3..4));
        let mut batch = read(1, 40, 100, &[]);
        let taken = |done, tells| Batching {
            done,
            total: 50,
            tells,
        };
        batch.batching.insert(3, taken(0, false));
        assert_eq!(state_of(&item, Some(&batch)), Some(Arrives::Copying(0.4)));
        batch.batching.insert(3, taken(25, false));
        assert_eq!(state_of(&item, Some(&batch)), Some(Arrives::Moving(0.5)));
        // One that tells each: not told yet, not begun (a later session).
        batch.batching.insert(3, taken(0, true));
        assert_eq!(state_of(&item, Some(&batch)), Some(Arrives::Waiting));
        batch.batching.insert(3, taken(10, true));
        assert_eq!(state_of(&item, Some(&batch)), Some(Arrives::Moving(0.2)));
        batch.batching.clear();
        assert_eq!(state_of(&item, Some(&batch)), Some(Arrives::Waiting));
    }

    #[test]
    fn an_item_waits_starts_moves_and_lands_by_its_steps() {
        // Its plan waiting, or checking itself before its steps.
        assert_eq!(state_of(&arrival(None), None), Some(Arrives::Waiting));
        let checking = read(0, 0, 0, &[]);
        assert_eq!(
            state_of(&arrival(None), Some(&checking)),
            Some(Arrives::Starting)
        );
        // Its steps 2 to 4 of a plan: those before it under way.
        let item = arrival(Some(2..4));
        assert_eq!(
            state_of(&item, Some(&read(1, 5, 10, &[]))),
            Some(Arrives::Waiting)
        );
        // Its first step under way, nothing moved yet, then half of it.
        assert_eq!(
            state_of(&item, Some(&read(2, 0, 0, &[]))),
            Some(Arrives::Starting)
        );
        assert_eq!(
            state_of(&item, Some(&read(2, 5, 10, &[]))),
            Some(Arrives::Moving(0.25))
        );
        // Its first step ended: half way, by its steps.
        assert_eq!(
            state_of(&item, Some(&read(3, 0, 10, &[]))),
            Some(Arrives::Moving(0.5))
        );
        // Its steps ended: landed.
        assert_eq!(state_of(&item, Some(&read(4, 0, 0, &[]))), None);
        // A batch finished its steps ahead of their turn: whole.
        assert_eq!(
            state_of(&item, Some(&read(1, 0, 0, &[2, 3]))),
            Some(Arrives::Moving(1.))
        );
        // Its run over, not landed: nothing said.
        assert_eq!(state_of(&item, None), None);
    }
}
