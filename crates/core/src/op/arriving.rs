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
}

/// What a step makes new, as it says it.
#[derive(Clone, Copy)]
enum Made {
    Folder,
    File,
}

/// What `change` brings, if it brings something: what it is made from,
/// what is made where nothing is, and where it lands. What a link or a
/// rename makes is not brought: it is there at once, as a move within its
/// folder is.
fn brought(change: &Change) -> Option<(Option<PathBuf>, Option<Made>, &PathBuf)> {
    match change {
        Change::Copy { from, to } => Some((Some(from.clone()), None, to)),
        Change::Move { from, to } => {
            (from.parent() != to.parent()).then(|| (Some(from.clone()), None, to))
        }
        Change::Create {
            path, directory, ..
        } => Some((
            None,
            Some(if *directory { Made::Folder } else { Made::File }),
            path,
        )),
        Change::Restore { path } => Some((None, None, path)),
        _ => None,
    }
}

/// What `plan` puts somewhere, item by item: by its items where they say
/// where each was put (`Items::placed`), else each step that brings
/// something ([`brought`]).
fn items_of(plan: &Plan) -> Vec<Item> {
    let changes: Vec<Change> = plan.steps.iter().map(|step| step.describe()).collect();
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
                let (from, made, _) = brought(change)?;
                Some(Item {
                    steps,
                    placed: placed.clone(),
                    from,
                    made,
                })
            })
            .collect();
    }
    changes
        .iter()
        .enumerate()
        .filter_map(|(at, change)| {
            let (from, made, to) = brought(change)?;
            Some(Item {
                steps: at..at + 1,
                placed: to.clone(),
                from,
                made,
            })
        })
        .collect()
}

/// What a view reads of what is brought: what it sorts and groups by, and
/// what it says of it. The rest of a stat is not kept: a paste of ten
/// thousand files keeps ten thousand of these while it runs.
const KEPT: [&str; 5] = [
    keys::SIZE,
    keys::MODIFIED,
    keys::CREATED,
    keys::ACCESSED,
    keys::MODE,
];

/// `entry` as it will be at `placed`, under `name`, with only what a view
/// reads of it ([`KEPT`]).
fn moved_to(entry: Entry, placed: &Path, name: &OsString) -> Entry {
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
    like
}

/// What `placed` will be, made from `from` (read by `read`, a stat), or
/// made new: a folder or a file, dated now. What cannot be read is a
/// file, nothing else known of it.
fn like_of(
    placed: &Path,
    name: &OsString,
    from: Option<&Path>,
    made: Option<Made>,
    read: &mut impl FnMut(&Path) -> Option<Entry>,
) -> Entry {
    if let Some(like) = from.and_then(&mut *read) {
        return moved_to(like, placed, name);
    }
    let kind = match made {
        Some(Made::Folder) => EntryKind::Directory,
        Some(Made::File) | None => EntryKind::File,
    };
    let mut like = Entry::new(EntryId::UNKNOWN, name.clone(), kind);
    if made.is_some() {
        let now = Value::Time(SystemTime::now());
        like.metadata
            .set(keys::MODIFIED, now.clone(), Provenance::Filesystem);
        like.metadata
            .set(keys::CREATED, now, Provenance::Filesystem);
    }
    like
}

/// The arrivals of `plan`, asked at `asked`: their steps among its own when
/// `runs` (the plan as it runs), else none yet. What each is made from is
/// taken from `known` where it was read before, else `read`.
pub(super) fn arrivals_of(
    plan: &Plan,
    runs: bool,
    asked: Instant,
    known: &HashMap<PathBuf, Entry>,
    mut read: impl FnMut(&Path) -> Option<Entry>,
) -> Vec<Arc<Arrival>> {
    items_of(plan)
        .into_iter()
        .filter_map(|item| {
            let folder = item.placed.parent()?.to_path_buf();
            let name = item.placed.file_name()?.to_os_string();
            let like = match item.from.as_deref().and_then(|from| known.get(from)) {
                Some(known) => moved_to(known.clone(), &item.placed, &name),
                None => like_of(
                    &item.placed,
                    &name,
                    item.from.as_deref(),
                    item.made,
                    &mut read,
                ),
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
        }
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
