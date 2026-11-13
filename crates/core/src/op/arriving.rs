//! What a plan brings into a folder while it waits or runs (ADR-135): each
//! of its items that puts something there, under the name it will have, and
//! what it will be, so that the folder shows it where it will land before it
//! does. Kept by the executor, from the plan asked until it runs, then from
//! the plan as it runs (its steps unfolded and its names settled), and let
//! go of when its run ends.
//!
//! Nothing here reads the destination: what an item will be is what it is
//! made from (a copy's or a move's source), read once on a worker as the
//! plan is asked, or as it runs what was not read by then (never on the
//! plan's thread, which runs at once), told as it is read; whether it is
//! made anew (a move to another volume) is the mount table's to say, in
//! memory (`fs::reach::volume_of`). What a step makes (an archive
//! extracted) is what it says. How far each is is read from its plan's
//! meter as it is asked, never kept: no event of its own, no wake of its
//! own beyond the plan's and its reading's.

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
#[derive(Clone, Debug)]
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
    /// made from, read, under its name there (its kind, its size, its
    /// times, which a copy keeps); what a step makes, by what it says;
    /// until what it is made from is read, a file of its name alone. What
    /// a view sorts it by and draws it as. Shared with what its plan was
    /// asked as, where it lands as asked: one of each kept, never a copy.
    pub like: Arc<Entry>,
    /// Whether `like` says what it is made from (read, or a step's), not
    /// its name alone.
    read: bool,
    /// Made anew where it lands, dated as made when asked once read: a
    /// copy; a move to another volume.
    anew: bool,
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
    /// Its steps ended: it is in place, while its plan runs on. Drawn whole
    /// until the folder lists it, so that its row and its entry's follow
    /// each other with no gap; the entry listed, nothing is drawn of it.
    Landed,
}

/// An item arriving, and how far it has come.
#[derive(Clone, Debug)]
pub struct Arriving {
    pub arrival: Arc<Arrival>,
    pub state: Arrives,
    /// Its plan is paused.
    pub paused: bool,
}

/// What a plan's run read of its steps for its items (`Meter::steps_read`),
/// borrowed from its meter while its items are told how far they are: no
/// copy of what a batch of 100,000 steps keeps, at each look.
#[derive(Clone, Copy, Debug)]
pub(super) struct StepsRead<'a> {
    /// The steps ended, the next one under way.
    pub ended: usize,
    /// The bytes of the step under way: moved, and announced.
    pub step_done: u64,
    pub step_total: u64,
    /// The steps after it a batch finished ahead of their turn (ADR-078).
    pub ahead: &'a HashSet<usize>,
    /// The steps a batch under way took: their bytes copied so far, where
    /// its transfer tells them, and in all.
    pub batching: &'a HashMap<usize, Batching>,
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

/// Where `arrival` stands as its plan's run read it: landed once its steps
/// ended; `None` where it has none, or where its run ended.
pub(super) fn state_of(arrival: &Arrival, read: Option<&StepsRead<'_>>) -> Option<Arrives> {
    let Some(read) = read else {
        // Its plan not running: waiting, unless it ran (its steps known),
        // and its run is over.
        return arrival.steps.is_none().then_some(Arrives::Waiting);
    };
    let Some(steps) = arrival.steps.clone() else {
        // Its plan checks itself before its steps.
        return Some(Arrives::Starting);
    };
    if steps.is_empty() {
        return None;
    }
    if steps.end <= read.ended {
        return Some(Arrives::Landed);
    }
    if steps.clone().all(|step| read.ahead.contains(&step)) {
        return Some(Arrives::Moving(1.));
    }
    // Taken by a batch: by its own bytes where its transfer tells them;
    // nothing told yet by one that tells each, not begun; else by the
    // batch's, which the step under way runs. Read in one pass, nothing
    // kept: asked of every item arriving at each look.
    let (mut taken, mut done, mut total, mut tells) = (false, 0u64, 0u64, false);
    for b in steps.clone().filter_map(|step| read.batching.get(&step)) {
        taken = true;
        done = done.saturating_add(b.done);
        total = total.saturating_add(b.total);
        tells |= b.tells;
    }
    if taken {
        if done > 0 && total > 0 {
            return Some(Arrives::Moving(
                (done as f64 / total as f64).clamp(0., 1.) as f32
            ));
        }
        if tells {
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

/// What `item` will be before what it is made from is read: what a step
/// makes, a folder or a file, dated now; else a file of its name alone.
fn made_like(item: &Item, name: &OsString) -> Entry {
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

/// The time a plan asked `asked` ago was asked at, by the clock.
pub(super) fn asked_at(asked: Instant) -> SystemTime {
    SystemTime::now()
        .checked_sub(asked.elapsed())
        .unwrap_or_else(SystemTime::now)
}

/// The arrivals of `plan`, asked at `asked`: their steps among its own when
/// `runs` (the plan as it runs), else none yet. Nothing is read: what each
/// is made from is taken from `known` (what its plan was asked as, read),
/// sharing its entry where it lands as asked, else it is its name alone
/// until read ([`Index::read`]). A copy, and a move to another volume (as
/// the mount table in memory says the volumes of what it is made from and
/// of its folder), are made anew.
pub(super) fn arrivals_of(
    plan: &Plan,
    runs: bool,
    asked: Instant,
    known: &HashMap<PathBuf, Arc<Arrival>>,
) -> Vec<Arrival> {
    let at = asked_at(asked);
    let mut volumes: HashMap<PathBuf, Option<PathBuf>> = HashMap::new();
    items_of(plan)
        .into_iter()
        .filter_map(|item| {
            let folder = item.placed.parent()?.to_path_buf();
            let name = item.placed.file_name()?.to_os_string();
            let anew = item.copied
                || item.from.as_deref().is_some_and(|from| {
                    let into = volumes
                        .entry(folder.clone())
                        .or_insert_with(|| crate::fs::reach::volume_of(&folder))
                        .clone();
                    crate::fs::reach::volume_of(from) != into
                });
            let was = item
                .from
                .as_deref()
                .and_then(|from| known.get(from))
                .filter(|was| was.read);
            let (like, read) = match was {
                Some(was) if was.folder == folder && was.name == name && was.anew == anew => {
                    (Arc::clone(&was.like), true)
                }
                Some(was) => (
                    Arc::new(moved_to(
                        Entry::clone(&was.like),
                        &item.placed,
                        &name,
                        anew.then_some(at),
                    )),
                    true,
                ),
                None => (Arc::new(made_like(&item, &name)), item.from.is_none()),
            };
            Some(Arrival {
                plan: plan.id,
                folder,
                name,
                from: item.from,
                like,
                read,
                anew,
                steps: runs.then_some(item.steps),
                asked,
            })
        })
        .collect()
}

/// `arrivals`, each not read whose source `read` holds made from it, as it
/// will be where it lands, those made anew dated as made at `at`.
pub(super) fn with_read(
    arrivals: impl IntoIterator<Item = Arrival>,
    read: &HashMap<PathBuf, Entry>,
    at: SystemTime,
) -> Vec<Arc<Arrival>> {
    arrivals
        .into_iter()
        .map(|mut arrival| {
            if !arrival.read
                && let Some(entry) = arrival.from.as_deref().and_then(|from| read.get(from))
            {
                let placed = arrival.folder.join(&arrival.name);
                arrival.like = Arc::new(moved_to(
                    entry.clone(),
                    &placed,
                    &arrival.name,
                    arrival.anew.then_some(at),
                ));
                arrival.read = true;
            }
            Arc::new(arrival)
        })
        .collect()
}

/// The sources of `arrivals` not read yet, each once.
pub(super) fn unread(arrivals: &[Arrival]) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    arrivals
        .iter()
        .filter(|a| !a.read)
        .filter_map(|a| a.from.clone())
        .filter(|from| seen.insert(from.clone()))
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
/// those of its run, and what they are, by the folder each lands in, those
/// folders in its order: a folder asked of is answered with its own, never
/// a walk of all a plan brings (a restore of 100,000 files into as many
/// folders).
struct Record {
    cancel: CancelToken,
    asked: Instant,
    /// Ends its sources' reading, once its run ended however it ended.
    reads: CancelToken,
    /// The sources a reading took, so that none is read twice: the asked
    /// plan's, then what its run brings that they were not.
    claimed: HashSet<PathBuf>,
    runs: bool,
    /// Its arrivals by folder, in its order.
    by_folder: HashMap<PathBuf, Arc<[Arc<Arrival>]>>,
    /// The folders it brings something into, each once, in its order.
    folders: Arc<[PathBuf]>,
}

impl Record {
    fn arrivals(&self) -> impl Iterator<Item = &Arc<Arrival>> {
        self.folders
            .iter()
            .filter_map(|folder| self.by_folder.get(folder))
            .flat_map(|those| those.iter())
    }
}

impl Index {
    /// `plan` was asked, cancelled by `cancel`: its arrivals follow
    /// ([`Index::fill`]). When it was asked.
    pub(super) fn asked(&self, plan: PlanId, cancel: &CancelToken) -> Instant {
        let mut inner = self.inner.lock();
        let record = inner.plans.entry(plan).or_insert_with(|| Record {
            cancel: cancel.clone(),
            asked: Instant::now(),
            reads: cancel.child(),
            claimed: HashSet::new(),
            runs: false,
            by_folder: HashMap::new(),
            folders: Arc::from([]),
        });
        record.asked
    }

    /// When `plan` was asked, while it is kept; and its arrivals read, by
    /// what each is made from (shared, never copied).
    pub(super) fn known(&self, plan: PlanId) -> Option<(Instant, HashMap<PathBuf, Arc<Arrival>>)> {
        let inner = self.inner.lock();
        let record = inner.plans.get(&plan)?;
        let known = record
            .arrivals()
            .filter(|a| a.read)
            .filter_map(|a| Some((a.from.clone()?, Arc::clone(a))))
            .collect();
        Some((record.asked, known))
    }

    /// Of `sources`, those no reading of `plan`'s took yet, now taken, and
    /// what ends their reading; `None` once `plan` is not kept.
    pub(super) fn claim(
        &self,
        plan: PlanId,
        sources: Vec<PathBuf>,
    ) -> Option<(Vec<PathBuf>, CancelToken)> {
        let mut inner = self.inner.lock();
        let record = inner.plans.get_mut(&plan)?;
        let sources = sources
            .into_iter()
            .filter(|from| record.claimed.insert(from.clone()))
            .collect();
        Some((sources, record.reads.clone()))
    }

    /// What `plan`'s sources were read as (`read`): each of its arrivals
    /// not read yet made from its own, as it will be where it lands.
    /// Whether any was.
    pub(super) fn read(&self, plan: PlanId, read: &HashMap<PathBuf, Entry>) -> bool {
        let mut inner = self.inner.lock();
        let Some(record) = inner.plans.get_mut(&plan) else {
            return false;
        };
        let at = asked_at(record.asked);
        let mut any = false;
        for those in record.by_folder.values_mut() {
            let takes = |a: &Arc<Arrival>| {
                !a.read && a.from.as_ref().is_some_and(|from| read.contains_key(from))
            };
            if those.iter().any(takes) {
                *those = with_read(those.iter().map(|a| Arrival::clone(a)), read, at).into();
                any = true;
            }
        }
        any
    }

    /// `plan` brings `arrivals`, those of its run where `runs`: kept only
    /// while it is (asked and not ended), and never the asked plan's over
    /// its run's. Whether they were kept.
    pub(super) fn fill(&self, plan: PlanId, arrivals: Vec<Arc<Arrival>>, runs: bool) -> bool {
        let mut by_folder: HashMap<PathBuf, Vec<Arc<Arrival>>> = HashMap::new();
        let mut order: Vec<PathBuf> = Vec::new();
        for arrival in arrivals {
            if let Some(those) = by_folder.get_mut(&arrival.folder) {
                those.push(arrival);
            } else {
                order.push(arrival.folder.clone());
                by_folder.insert(arrival.folder.clone(), vec![arrival]);
            }
        }
        let mut inner = self.inner.lock();
        let Inner { plans, folders } = &mut *inner;
        let Some(record) = plans.get_mut(&plan) else {
            return false;
        };
        if record.runs && !runs {
            return false;
        }
        unlist(folders, plan, &record.folders);
        for folder in &order {
            folders.entry(folder.clone()).or_default().push(plan);
        }
        record.runs = runs;
        record.by_folder = by_folder
            .into_iter()
            .map(|(folder, those)| (folder, Arc::from(those)))
            .collect();
        record.folders = order.into();
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
        record.reads.cancel();
        unlist(folders, plan, &record.folders);
        !record.folders.is_empty()
    }

    /// The folders `plan` brings something into, each once, in its order.
    pub(super) fn folders_of(&self, plan: PlanId) -> Arc<[PathBuf]> {
        let inner = self.inner.lock();
        inner
            .plans
            .get(&plan)
            .map_or_else(|| Arc::from([]), |record| Arc::clone(&record.folders))
    }

    /// What the plans not cancelled bring into `folder`, each plan's
    /// together, in the order they were asked and their items in theirs.
    pub(super) fn in_folder(&self, folder: &Path) -> Vec<Arc<[Arc<Arrival>]>> {
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
            .filter_map(|record| record.by_folder.get(folder).map(Arc::clone))
            .collect()
    }
}

/// `plan` no longer brings anything into `its` folders.
fn unlist(folders: &mut HashMap<PathBuf, Vec<PlanId>>, plan: PlanId, its: &[PathBuf]) {
    for folder in its {
        if let Some(those) = folders.get_mut(folder) {
            those.retain(|p| *p != plan);
            if those.is_empty() {
                folders.remove(folder);
            }
        }
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
            like: Arc::new(Entry::new(EntryId::UNKNOWN, "a", EntryKind::File)),
            read: true,
            anew: false,
            steps,
            asked: Instant::now(),
        }
    }

    /// A reading of a plan's steps, kept for the tests to lend.
    struct Read {
        ended: usize,
        done: u64,
        total: u64,
        ahead: HashSet<usize>,
        batching: HashMap<usize, Batching>,
    }

    impl Read {
        fn lent(&self) -> StepsRead<'_> {
            StepsRead {
                ended: self.ended,
                step_done: self.done,
                step_total: self.total,
                ahead: &self.ahead,
                batching: &self.batching,
            }
        }
    }

    fn read(ended: usize, done: u64, total: u64, ahead: &[usize]) -> Read {
        Read {
            ended,
            done,
            total,
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
        assert_eq!(
            state_of(&item, Some(&batch.lent())),
            Some(Arrives::Copying(0.4))
        );
        batch.batching.insert(3, taken(25, false));
        assert_eq!(
            state_of(&item, Some(&batch.lent())),
            Some(Arrives::Moving(0.5))
        );
        // One that tells each: not told yet, not begun (a later session).
        batch.batching.insert(3, taken(0, true));
        assert_eq!(state_of(&item, Some(&batch.lent())), Some(Arrives::Waiting));
        batch.batching.insert(3, taken(10, true));
        assert_eq!(
            state_of(&item, Some(&batch.lent())),
            Some(Arrives::Moving(0.2))
        );
        batch.batching.clear();
        assert_eq!(state_of(&item, Some(&batch.lent())), Some(Arrives::Waiting));
    }

    #[test]
    fn an_item_waits_starts_moves_and_lands_by_its_steps() {
        // Its plan waiting, or checking itself before its steps.
        assert_eq!(state_of(&arrival(None), None), Some(Arrives::Waiting));
        let checking = read(0, 0, 0, &[]);
        assert_eq!(
            state_of(&arrival(None), Some(&checking.lent())),
            Some(Arrives::Starting)
        );
        // Its steps 2 to 4 of a plan: those before it under way.
        let item = arrival(Some(2..4));
        assert_eq!(
            state_of(&item, Some(&read(1, 5, 10, &[]).lent())),
            Some(Arrives::Waiting)
        );
        // Its first step under way, nothing moved yet, then half of it.
        assert_eq!(
            state_of(&item, Some(&read(2, 0, 0, &[]).lent())),
            Some(Arrives::Starting)
        );
        assert_eq!(
            state_of(&item, Some(&read(2, 5, 10, &[]).lent())),
            Some(Arrives::Moving(0.25))
        );
        // Its first step ended: half way, by its steps.
        assert_eq!(
            state_of(&item, Some(&read(3, 0, 10, &[]).lent())),
            Some(Arrives::Moving(0.5))
        );
        // Its steps ended: landed.
        assert_eq!(
            state_of(&item, Some(&read(4, 0, 0, &[]).lent())),
            Some(Arrives::Landed)
        );
        // A batch finished its steps ahead of their turn: whole.
        assert_eq!(
            state_of(&item, Some(&read(1, 0, 0, &[2, 3]).lent())),
            Some(Arrives::Moving(1.))
        );
        // Its run over, not landed: nothing said.
        assert_eq!(state_of(&item, None), None);
    }
}
