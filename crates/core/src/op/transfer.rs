//! Copies to and from a remote in batches (ADR-078).
//!
//! Through a mount, each file a copy makes on a server, or reads from one,
//! costs round trips to it: made, written, given its mode and its time,
//! closed. Eight of them, 160 ms a file from an SFTP server 20 ms away,
//! where `rsync` took a few milliseconds a file in one stream. So before
//! the executor checks a copy, it looks at the run of copies that follow
//! into the same folder ([`batch_ahead`]): where one end is a remote, the
//! copies of the run whose sources share a folder are made at once, in a
//! folder of the plan's staging folder there, by a plugin's [`Transfer`]
//! that serves the two folders (the remote plugin's rsync through the
//! connection sshfs holds), else by Skwir's own copy, eight files at a
//! time ([`copy_at_once`]). Each step then runs as any: its check, its
//! apply placing the copy made (`builtin::copy_staged`), its undo. A copy
//! the batch did not take or finish is made by its step. A file alone
//! costs a batch as many round trips as its step: it goes in one within a
//! remote, where a transfer copies it on the server, or by a transfer
//! that takes a file alone faster than the mount
//! ([`Transfer::serves_alone`]).
//!
//! A file or a link copied under its own name, which the destination's
//! listing shows free, the batch places itself ([`Batched::Placed`]),
//! never over anything (`rsync --ignore-existing`, Skwir's own a
//! no-replace rename), its partial bytes in the staging folder: placed
//! by its step, it would cost a look and a rename across the network
//! each, as many round trips as rsync's whole batch. What it placed is
//! verified (the name was free; the file there has the source's size and
//! time) and its step only says so.
//!
//! A move to another device is a copy, then the original removed: a batch
//! makes its copy as a copy's, after reading what its source is
//! (`builtin::state_of`), which its step compares before removing the
//! original. A batch heeds the plan's pause ([`Control`]): Skwir's own
//! copy waits between files, a tool is stopped meanwhile.
//!
//! What a plugin's transfer writes or removes in a remote's folder, it
//! does beside the mount, where no watch of this machine's sees it: the
//! folder is noted (`StepContext::changed_beside`), and the window reads
//! it again where it is shown at the plan's end
//! (`Executor::set_changed_beside`), a transfer whose mount keeps what it
//! read (sshfs) making it read the folder anew just before
//! ([`Transfer::show`]): a look at the folder during the plan may have
//! kept what it held then.

use std::collections::{BTreeMap, HashMap};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use anyhow::Context;
use parking_lot::{Mutex, RwLock};

use super::context::{CopiedAs, Pause};
use super::{Cancelled, Copy, Delete, Move, Operation, StepContext, Uncopy};
use crate::cancel::CancelToken;
use crate::companions::Companions;

/// A way to copy many files at once between a folder here and one on a
/// remote that a plugin knows: an installed tool's (rsync, through the
/// ssh connection a mount holds). Skwir speaks no protocol (ADR-077): the
/// tool does, with what the user set up for it.
pub trait Transfer: Send + Sync {
    /// What it is called in the log: `rsync`.
    fn name(&self) -> &'static str;

    /// Whether it copies from the folder `from` into the folder `into`.
    /// Asked of what is in memory (the mount table, what the plugin
    /// connected), never by reading a remote: on the plan's thread.
    fn serves(&self, from: &Path, into: &Path) -> bool;

    /// Whether it copies a file alone from `from` into `into` faster than
    /// Skwir's own copy through the mount, where a batch of one costs as
    /// many round trips as its step: a mount whose bridge reads and writes
    /// a little at a time (GVfs's AFP and MTP). Measured, never assumed.
    /// Its step would go on from a copy cut short (ADR-080); a tool's
    /// starts again. Asked as [`Transfer::serves`] is.
    fn serves_alone(&self, from: &Path, into: &Path) -> bool {
        let _ = (from, into);
        false
    }

    /// Copy the entries `names` of the folder `from` into the folder
    /// `into`, under the same names: folders whole, links as links, files'
    /// and folders' modes and modification times kept. Without `temp`,
    /// `into` is a folder of the staging folder, empty. With it, `into`
    /// holds other names: nothing there is replaced, a name taken
    /// meanwhile is left as it is, and a file is written in `temp` (a
    /// folder on `into`'s volume) until whole; `placed` is told each name
    /// it placed so (where it cannot tell, a copy is known by its source's
    /// size and time, which a volume keeping no times does not keep).
    /// `copied` is told the bytes copied so far, in all. Blocking, on the
    /// plan's thread; stopped when the plan is cancelled, held while it is
    /// paused ([`Control`]).
    ///
    /// # Errors
    /// When the copy could not be made whole, or was cancelled: what it
    /// made in the staging folder goes with it. [`NotServed`] where it
    /// finds it cannot serve after all, having made nothing.
    fn copy(
        &self,
        from: &Path,
        names: &[OsString],
        into: &Path,
        temp: Option<&Path>,
        told: &Told<'_>,
        control: &Control,
    ) -> anyhow::Result<()>;

    /// Whether it removes entries of `folder` ([`Transfer::remove`]) so
    /// that the mount holding it sees them gone at once: GVfs's daemon
    /// serves its bridge and `gio remove` alike. Not sshfs's: its own
    /// cache would say a file removed beside it is there for 20 s, and an
    /// Undo would find its name taken. Asked of what is in memory.
    fn removes(&self, folder: &Path) -> bool {
        let _ = folder;
        false
    }

    /// Remove the entries `names` of the folder `folder`, trees whole: the
    /// originals of moves a batch copied, which their steps found
    /// unchanged, where a mount removes them one at a time
    /// ([`Transfer::removes`]). Blocking, on the plan's thread, held while
    /// it is paused.
    ///
    /// # Errors
    /// [`NotServed`] where it does not reach `folder`'s server, having
    /// removed nothing; what failed otherwise: what it left is removed one
    /// by one.
    fn remove(&self, folder: &Path, names: &[OsString], control: &Control) -> anyhow::Result<()> {
        let _ = (folder, names, control);
        Err(NotServed.into())
    }

    /// Make the mount show `folder` as the server has it, this plan's
    /// transfers having changed it beside the mount, just before the window
    /// reads it again (`Executor::set_changed_beside`): a mount that keeps
    /// what it read (sshfs keeps a folder's names 20 s) reads it anew.
    /// Whether it did; the first that does is the last asked. Blocking, on
    /// the plan's thread.
    fn show(&self, folder: &Path) -> bool {
        let _ = folder;
        false
    }
}

/// What a transfer tells as it copies ([`Transfer::copy`]).
pub struct Told<'a> {
    /// The bytes copied so far, in all.
    pub copied: &'a (dyn Fn(u64) + Sync),
    /// Each name it placed beside what is there.
    pub placed: &'a (dyn Fn(&OsStr) + Sync),
}

/// What a transfer heeds as it goes: the plan's cancel and its pause.
#[derive(Clone)]
pub struct Control {
    cancel: CancelToken,
    pause: Arc<Pause>,
}

impl Control {
    pub(super) fn new(cancel: CancelToken, pause: Arc<Pause>) -> Self {
        Control { cancel, pause }
    }

    /// What cancels the plan.
    pub fn cancel(&self) -> &CancelToken {
        &self.cancel
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// Whether the plan is paused: a tool is stopped meanwhile, Skwir's
    /// own copy waits between files.
    pub fn is_paused(&self) -> bool {
        self.pause.is_paused()
    }

    /// Return once the plan is not paused, or is cancelled.
    pub fn wait_while_paused(&self) {
        self.pause.wait(&self.cancel);
    }
}

/// The transfers plugins contributed, by owner, the first that serves a
/// pair of folders used.
#[derive(Default)]
pub struct Transfers {
    list: RwLock<Vec<(Arc<str>, Arc<dyn Transfer>)>>,
}

impl Transfers {
    pub fn register(&self, owner: &str, transfer: Arc<dyn Transfer>) {
        self.list.write().push((owner.into(), transfer));
    }

    pub fn unregister_owner(&self, owner: &str) {
        self.list.write().retain(|(o, _)| &**o != owner);
    }

    /// Every one registered, in order.
    pub(super) fn all(&self) -> Vec<Arc<dyn Transfer>> {
        self.list
            .read()
            .iter()
            .map(|(_, t)| Arc::clone(t))
            .collect()
    }

    /// Whether one removes entries of `folder` ([`Transfer::removes`]).
    pub(super) fn remove_in(&self, folder: &Path) -> bool {
        self.list.read().iter().any(|(_, t)| t.removes(folder))
    }

    /// The names of those registered, for a test of plugins withdrawn.
    pub fn names(&self) -> Vec<&'static str> {
        self.list.read().iter().map(|(_, t)| t.name()).collect()
    }

    /// Those that serve the pair of folders, in the order registered: for
    /// a file `alone`, those that take one ([`Transfer::serves_alone`]).
    fn serving(&self, from: &Path, into: &Path, alone: bool) -> Vec<Arc<dyn Transfer>> {
        self.list
            .read()
            .iter()
            .filter(|(_, t)| {
                if alone {
                    t.serves_alone(from, into)
                } else {
                    t.serves(from, into)
                }
            })
            .map(|(_, t)| Arc::clone(t))
            .collect()
    }
}

/// What a [`Transfer::copy`] answers when it finds it cannot serve after
/// all (the server runs no rsync), having made nothing: the next transfer
/// that serves the folders is asked, else Skwir's own copy.
#[derive(Debug)]
pub struct NotServed;

impl std::fmt::Display for NotServed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the transfer does not serve these folders")
    }
}

impl std::error::Error for NotServed {}

/// What a batch did for a copy's source, which its step takes
/// (`StepContext::take_batched`).
#[derive(Debug, Clone)]
pub(super) enum Batched {
    /// Its copy, whole, in the staging folder.
    Made(PathBuf),
    /// Its copy, whole, at its target, and what it is there: its step
    /// only says so.
    Placed(PathBuf, Stamp),
    /// Counted in the progress's total, not made: its step copies it
    /// without counting it again.
    Counted,
}

/// The batches this test's process made.
#[cfg(test)]
static BATCHES: AtomicUsize = AtomicUsize::new(0);

/// How many files Skwir's own batch copy makes at once: twice as fast as
/// one by one into a folder of an SFTP server 20 ms away (2026-10-21),
/// where the folder's lock, taken for each name made, holds the rest.
const AT_ONCE: usize = 8;

/// How many it copies at once from a remote onto this machine: reading
/// takes no lock of the folder's, and a file costs two round trips
/// through sshfs (opened with its attributes, read), which serves ten
/// requests at once (libfuse's default, which it keeps and offers no
/// option to change). 100 files of 16 KiB from an SFTP-only server 20 ms
/// away: 0.72 s eight at once, 0.63 sixteen, 0.86 thirty-two, 1.01–1.04
/// sixty-four; ten files of 4 MiB 1.08–1.11, 0.85–0.87, 0.90–0.97,
/// 0.87–0.92 (2026-10-21).
const DOWN_AT_ONCE: usize = 16;

/// Look, before the step `i` of `steps` is checked, at the run of copies
/// from it into one folder, and make at once those a batch takes (module
/// documentation). Once for a run: its later steps find it done.
///
/// # Errors
/// Only when the plan was cancelled during a batch; one that failed
/// otherwise leaves its copies to their steps.
pub(super) fn batch_ahead(
    steps: &[Box<dyn Operation>],
    i: usize,
    ctx: &StepContext,
) -> Result<(), Cancelled> {
    if i < ctx.batched_until() {
        return Ok(());
    }
    let Some(first) = steps.get(i).and_then(|s| batchable(s.as_ref())) else {
        return Ok(());
    };
    let dest = first.dest_dir.to_path_buf();
    let run: Vec<Batchable<'_>> = steps[i..]
        .iter()
        .map_while(|s| batchable(s.as_ref()).filter(|b| b.dest_dir == dest))
        .collect();
    ctx.batched_up_to(i + run.len());
    // A drive written through keeps its own pace (`pace`).
    if crate::fs::volume::is_removable(&dest) {
        return Ok(());
    }
    let dest_remote = crate::fs::reach::of(&dest).is_some();
    let mut groups: BTreeMap<PathBuf, Vec<Source>> = BTreeMap::new();
    for step in run {
        // One that does not resolve says why in its own check.
        if let Ok(src) = ctx.resolve(step.entry)
            && let Some(parent) = src.parent()
        {
            ctx.resolved_as(step.entry, src.clone());
            groups
                .entry(parent.to_path_buf())
                .or_default()
                .push(Source {
                    src,
                    new_name: step.new_name.cloned(),
                    moving: step.moving,
                });
        }
    }
    for (from, sources) in groups {
        if !dest_remote && crate::fs::reach::of(&from).is_none() {
            continue;
        }
        let taken = takes(&from, &sources, ctx);
        // One file alone: as many round trips in a batch as by its step;
        // but within one remote, a transfer may copy it there, where its
        // step would bring its bytes here and send them back; and one may
        // take a file alone faster than a mount does.
        let alone = is_alone(&taken);
        let within = crate::fs::reach::of(&from)
            .zip(crate::fs::reach::of(&dest))
            .is_some_and(|(a, b)| a.point == b.point)
            && ctx
                .transfers()
                .is_some_and(|t| !t.serving(&from, &dest, false).is_empty());
        let faster = alone
            && ctx
                .transfers()
                .is_some_and(|t| !t.serving(&from, &dest, true).is_empty());
        let worth = !alone || within || faster;
        if taken.is_empty() || !worth {
            continue;
        }
        batch(&from, &dest, &taken, ctx)?;
    }
    Ok(())
}

/// Whether a batch takes a file (or a link) alone: no folder, one entry.
fn is_alone(taken: &[Taken]) -> bool {
    matches!(taken, [one] if !one.folder)
}

/// Resolve ahead, [`AT_ONCE`] at a time, the sources of `steps` (copies
/// and moves) and what they remove (`removal`) that are on a remote, where
/// each look is a round trip: a dry run then checks each step from what
/// was found (`StepContext::resolve`).
pub(super) fn resolve_ahead(steps: &[Box<dyn Operation>], ctx: &StepContext) {
    let entries: Vec<&super::EntryRef> = steps
        .iter()
        .filter_map(|step| {
            let any = step.as_any()?;
            any.downcast_ref::<Copy>()
                .map(|c| &c.entry)
                .or_else(|| any.downcast_ref::<Move>().map(|m| &m.entry))
                .or_else(|| removal(step.as_ref()))
        })
        .filter(|entry| crate::fs::reach::of(&entry.path).is_some())
        .collect();
    resolve_all(&entries, ctx);
}

/// Resolve `entries` at once, [`AT_ONCE`] at a time, each found kept for
/// its step (`StepContext::resolved_as`). One alone: by its step.
fn resolve_all(entries: &[&super::EntryRef], ctx: &StepContext) {
    if entries.len() < 2 {
        return;
    }
    let next = AtomicUsize::new(0);
    let found: Mutex<Vec<(usize, PathBuf)>> = Mutex::default();
    std::thread::scope(|s| {
        for _ in 0..AT_ONCE.min(entries.len()) {
            s.spawn(|| {
                loop {
                    let n = next.fetch_add(1, Ordering::Relaxed);
                    let Some(entry) = entries.get(n) else {
                        return;
                    };
                    if let Ok(path) = entry.resolve() {
                        found.lock().push((n, path));
                    }
                }
            });
        }
    });
    for (n, path) in found.into_inner() {
        ctx.resolved_as(entries[n], path);
    }
}

/// Whether a removal ahead removed what `step` removes and its step has
/// not said so: the plan stopped before it.
pub(super) fn removed_unclaimed(step: &dyn Operation, ctx: &StepContext) -> bool {
    removal(step)
        .and_then(|entry| ctx.resolve(entry).ok())
        .is_some_and(|path| ctx.was_removed_ahead(&path))
}

/// What a step removes, where a removal ahead may take it: a deletion for
/// good, or a copy taken back where its volume keeps no trash.
fn removal(step: &dyn Operation) -> Option<&super::EntryRef> {
    let any = step.as_any()?;
    any.downcast_ref::<Delete>()
        .map(|d| &d.entry)
        .or_else(|| any.downcast_ref::<Uncopy>().map(|u| &u.entry))
}

/// Look, before the step `i` of `steps` is checked, at the run of
/// removals from it (`removal`), and remove at once those in a remote's
/// folder where a plugin's transfer removes (`Transfer::removes`), with
/// the files beside them that go with them (their companions, as the
/// folder's names listed once say): through the mount, each name is a
/// round trip to the server under its folder's lock. Only those whose
/// checks pass now, as their steps would check them: a removal is not
/// taken back. Each step then says it removed its own
/// (`StepContext::take_removed_ahead`). Once for a run.
///
/// # Errors
/// Only when the plan was cancelled; a transfer that failed leaves what
/// it did not remove to the steps.
pub(super) fn remove_ahead(
    steps: &[Box<dyn Operation>],
    i: usize,
    ctx: &StepContext,
) -> Result<(), Cancelled> {
    if i < ctx.removed_until() || steps.get(i).and_then(|s| removal(s.as_ref())).is_none() {
        return Ok(());
    }
    let run: Vec<&dyn Operation> = steps[i..]
        .iter()
        .map(AsRef::as_ref)
        .take_while(|step| removal(*step).is_some())
        .collect();
    ctx.removed_up_to(i + run.len());
    let Some(transfers) = ctx.transfers() else {
        return Ok(());
    };
    let remote: Vec<&dyn Operation> = run
        .into_iter()
        .filter(|step| removal(*step).is_some_and(|e| crate::fs::reach::of(&e.path).is_some()))
        .collect();
    if remote.is_empty() {
        return Ok(());
    }
    let entries: Vec<&super::EntryRef> = remote.iter().filter_map(|s| removal(*s)).collect();
    resolve_all(&entries, ctx);
    let mut folders: BTreeMap<PathBuf, Vec<(&super::EntryRef, &dyn Operation, PathBuf)>> =
        BTreeMap::new();
    for step in remote {
        let Some(entry) = removal(step) else { continue };
        if let Ok(path) = ctx.resolve(entry)
            && let Some(folder) = path.parent()
            && transfers.remove_in(folder)
        {
            folders
                .entry(folder.to_path_buf())
                .or_default()
                .push((entry, step, path));
        }
    }
    let control = ctx.control();
    for (folder, steps) in folders {
        // One file alone: as many round trips by its step. A folder is a
        // round trip a name it holds.
        let worth = steps.len() > 1
            || steps
                .iter()
                .any(|(_, _, path)| std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir()));
        if !worth {
            continue;
        }
        // Their names, and their companions there, as the folder's names
        // say: a look for a name not there is a round trip each.
        let Ok(listed) = std::fs::read_dir(&folder).map(|entries| {
            entries
                .flatten()
                .map(|e| e.file_name())
                .collect::<std::collections::HashSet<_>>()
        }) else {
            continue;
        };
        let mut names: Vec<OsString> = Vec::new();
        let mut taken: Vec<(&super::EntryRef, PathBuf)> = Vec::new();
        for (entry, step, path) in steps {
            if !matches!(step.check(ctx), super::StepStatus::Pending { .. }) {
                continue;
            }
            let companions = ctx.companions().existing_as(&path, &|at| {
                at.parent() == Some(folder.as_path())
                    && at.file_name().is_some_and(|name| listed.contains(name))
            });
            // One beside elsewhere is left to its step.
            if companions
                .iter()
                .any(|c| c.path.parent() != Some(folder.as_path()))
            {
                continue;
            }
            let Some(name) = path.file_name() else {
                continue;
            };
            names.push(name.to_owned());
            names.extend(
                companions
                    .iter()
                    .filter_map(|c| c.path.file_name().map(ToOwned::to_owned)),
            );
            taken.push((entry, path));
        }
        if taken.is_empty() {
            continue;
        }
        let mut removed = false;
        for transfer in transfers.all() {
            if !transfer.removes(&folder) {
                continue;
            }
            ctx.changed_beside(&folder);
            match transfer.remove(&folder, &names, &control) {
                Err(error) if error.is::<NotServed>() => {}
                Ok(()) => {
                    removed = true;
                    break;
                }
                Err(error) => {
                    tracing::warn!(
                        "{} removing {} entries failed, the rest removed one by one: {error:#}",
                        transfer.name(),
                        taken.len()
                    );
                    break;
                }
            }
        }
        for (entry, path) in taken {
            if removed {
                ctx.removed_ahead(&path);
            } else {
                // Cut short, each is looked at again by its step.
                ctx.forget_resolved(entry);
            }
        }
        ctx.check_cancelled()?;
    }
    Ok(())
}

/// A step whose copy a batch may make: a copy that keeps attributes, or a
/// move to another device (from or to a remote, or between two), which is
/// a copy and the original removed. Not a move out of the trash or into
/// it: a restore, or the trash's own.
struct Batchable<'a> {
    entry: &'a super::EntryRef,
    dest_dir: &'a Path,
    new_name: Option<&'a OsString>,
    moving: bool,
}

fn batchable(step: &dyn Operation) -> Option<Batchable<'_>> {
    let any = step.as_any()?;
    if let Some(copy) = any.downcast_ref::<Copy>() {
        return copy.preserve_attributes.then_some(Batchable {
            entry: &copy.entry,
            dest_dir: &copy.dest_dir,
            new_name: copy.new_name.as_ref(),
            moving: false,
        });
    }
    let moving = any.downcast_ref::<Move>()?;
    let across = match (
        crate::fs::reach::of(&moving.entry.path),
        crate::fs::reach::of(&moving.dest_dir),
    ) {
        (Some(from), Some(to)) => from.point != to.point,
        (from, to) => from.is_some() != to.is_some(),
    };
    let trash = super::in_trash(&moving.entry.path).is_some()
        || super::in_trash(&moving.dest_dir).is_some();
    (across && !trash).then_some(Batchable {
        entry: &moving.entry,
        dest_dir: &moving.dest_dir,
        new_name: moving.new_name.as_ref(),
        moving: true,
    })
}

/// A step's source, as a batch looks at it.
struct Source {
    src: PathBuf,
    new_name: Option<OsString>,
    moving: bool,
}

/// An entry a batch takes: its source, its name, its bytes, and whether
/// it is a folder, or copied under another name (a copy's "(copy)").
struct Taken {
    src: PathBuf,
    name: OsString,
    bytes: u64,
    folder: bool,
    renamed: bool,
    /// A move's: what its source is is read before it is copied.
    moving: bool,
    /// What a file or a link was as it was taken, which its copy placed
    /// must be.
    stamp: Option<Stamp>,
}

/// A file's size and modification time to the second (SFTP keeps no
/// more), or a link's target: what a copy a batch placed has of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Stamp {
    File { len: u64, modified: u64 },
    Link(PathBuf),
}

impl Stamp {
    /// What the file or link at `path` is; `None` for anything else, or
    /// a file whose time cannot be read.
    fn of(path: &Path) -> Option<Self> {
        let meta = std::fs::symlink_metadata(path).ok()?;
        if meta.file_type().is_symlink() {
            return std::fs::read_link(path).ok().map(Stamp::Link);
        }
        let modified = meta
            .modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_secs();
        meta.is_file().then_some(Stamp::File {
            len: meta.len(),
            modified,
        })
    }
}

/// What of `sources` (entries of the folder `from`) a batch takes
/// (ADR-078, 2): a name no other of them has, no companion beside it, a
/// tree of files, folders and links alone. Reads the folder's names once
/// and walks each tree.
fn takes(from: &Path, sources: &[Source], ctx: &StepContext) -> Vec<Taken> {
    let rules = ctx.companions();
    let Ok(listed) = std::fs::read_dir(from) else {
        return Vec::new();
    };
    let listed: Vec<OsString> = listed.flatten().map(|e| e.file_name()).collect();
    let mut names: HashMap<&OsStr, usize> = HashMap::new();
    for Source { src, .. } in sources {
        if let Some(name) = src.file_name() {
            *names.entry(name).or_default() += 1;
        }
    }
    let mut taken = Vec::new();
    for Source {
        src,
        new_name,
        moving,
    } in sources
    {
        if ctx.cancel_token().is_cancelled() {
            return Vec::new();
        }
        let Some(name) = src.file_name() else {
            continue;
        };
        let alone = names.get(name) == Some(&1)
            && rules.owner(name).is_none()
            && !listed
                .iter()
                .any(|other| rules.owner(other).as_deref() == Some(name));
        if !alone {
            continue;
        }
        let folder = std::fs::symlink_metadata(src).is_ok_and(|m| m.is_dir());
        let stamp = (!folder).then(|| Stamp::of(src)).flatten();
        if let Some(bytes) = plain_tree(src, rules, ctx.cancel_token()) {
            taken.push(Taken {
                src: src.clone(),
                name: name.to_os_string(),
                bytes,
                folder,
                renamed: new_name.as_deref().is_some_and(|n| n != name),
                moving: *moving,
                stamp,
            });
        }
    }
    taken
}

/// The bytes of the tree at `path` when a batch copies it as its step
/// would: files of one name, folders and links (on Unix), nothing a
/// companion rule claims, no attribute a copy keeps. `None` otherwise.
fn plain_tree(path: &Path, rules: &Companions, cancel: &CancelToken) -> Option<u64> {
    if cancel.is_cancelled() {
        return None;
    }
    let meta = std::fs::symlink_metadata(path).ok()?;
    let kind = meta.file_type();
    if kind.is_symlink() {
        return cfg!(unix).then_some(0);
    }
    if !kind.is_file() && !kind.is_dir() {
        return None;
    }
    if !super::builtin::user_attributes(path).ok()?.is_empty() {
        return None;
    }
    if kind.is_file() {
        return (names_of(&meta) == 1).then_some(meta.len());
    }
    let mut total = 0;
    for entry in std::fs::read_dir(path).ok()? {
        let entry = entry.ok()?;
        let name = entry.file_name();
        if rules.owner(&name).is_some() || rules.is_folder(&name) {
            return None;
        }
        total += plain_tree(&entry.path(), rules, cancel)?;
    }
    Some(total)
}

/// How many names the file `meta` is of has: one where the system does
/// not say.
fn names_of(meta: &std::fs::Metadata) -> u64 {
    #[cfg(unix)]
    {
        std::os::unix::fs::MetadataExt::nlink(meta)
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        1
    }
}

/// Make the copies `taken` of the folder `from` at once in `dest`, and
/// tell their steps: those a batch places itself at their targets
/// (module documentation), the rest in a folder of the plan's staging
/// folder there.
fn batch(from: &Path, dest: &Path, taken: &[Taken], ctx: &StepContext) -> Result<(), Cancelled> {
    #[cfg(test)]
    BATCHES.fetch_add(1, Ordering::Relaxed);
    let total: u64 = taken.iter().map(|t| t.bytes).sum();
    ctx.add_total(total);
    for t in taken {
        ctx.batched(&t.src, Batched::Counted);
    }
    // What a move's source is as it is copied: changed after, its step
    // leaves it (`builtin::move_entry`).
    let states: HashMap<&Path, CopiedAs> = taken
        .iter()
        .filter(|t| t.moving)
        .map(|t| (t.src.as_path(), CopiedAs(super::builtin::state_of(&t.src))))
        .collect();
    let made_by_batch = |t: &Taken, what: Batched| {
        ctx.batched(&t.src, what);
        if let Some(state) = states.get(t.src.as_path()) {
            ctx.batched_state(&t.src, *state);
        }
    };
    // What the destination holds now: a name free there may be placed.
    let held: std::collections::HashSet<OsString> = if taken.iter().any(|t| !t.folder) {
        match std::fs::read_dir(dest) {
            Ok(listed) => listed.flatten().map(|e| e.file_name()).collect(),
            Err(_) => return Ok(()),
        }
    } else {
        std::collections::HashSet::new()
    };
    let (placed, staged): (Vec<&Taken>, Vec<&Taken>) = taken
        .iter()
        .partition(|t| t.stamp.is_some() && !t.renamed && !held.contains(&t.name));
    // A file alone goes by a transfer that takes one (`batch_ahead`).
    let alone = is_alone(taken);
    let advance = ctx.progress();
    let told = AtomicU64::new(0);
    let copied = |so_far: u64| {
        let before = told.fetch_max(so_far, Ordering::Relaxed);
        if so_far > before {
            advance(so_far - before);
        }
    };
    let made = (|| {
        let (staging, name) = ctx.staging_for(dest)?;
        if !staged.is_empty() {
            let into = staging.path().join(format!("batch-{name}"));
            std::fs::create_dir(&into)?;
            run(from, (&staged, alone), &into, None, &copied, &|_| {}, ctx)?;
            for t in &staged {
                made_by_batch(t, Batched::Made(into.join(&t.name)));
            }
        }
        if !placed.is_empty() {
            let temp = staging.path().join(format!("temp-{name}"));
            std::fs::create_dir(&temp)?;
            let said: Mutex<std::collections::HashSet<OsString>> = Mutex::default();
            let ran = run(
                from,
                (&placed, alone),
                dest,
                Some(&temp),
                &copied,
                &|name: &OsStr| {
                    said.lock().insert(name.to_owned());
                },
                ctx,
            );
            let said = said.into_inner();
            // What it placed, finished or not: what the transfer says it
            // placed; else, the name being free, a file there that is the
            // source as it was taken or as it is now, by its size and time
            // (changed meanwhile, a move's step finds it so and leaves it).
            // Listing the folder first answers each look of an sshfs mount
            // from what sshfs read with it.
            let _ = std::fs::read_dir(dest).map(Iterator::count);
            let cancelled = ctx.cancel_token().is_cancelled();
            // What is there, each looked at at once: through a mount, a look
            // may be a round trip.
            let there = looked_at(&placed, |t| dest.join(&t.name));
            for (t, there) in placed.iter().zip(there) {
                let target = dest.join(&t.name);
                let Some(stamp) = there.filter(|there| {
                    said.contains(&t.name)
                        || t.stamp.as_ref() == Some(there)
                        || Stamp::of(&t.src).as_ref() == Some(there)
                }) else {
                    continue;
                };
                if cancelled {
                    // Cancelled, a copy leaves nothing it made: its own,
                    // made a moment ago.
                    let _ = std::fs::remove_file(&target);
                } else {
                    made_by_batch(t, Batched::Placed(target, stamp));
                }
            }
            ran?;
        }
        anyhow::Ok(())
    })();
    match made {
        Ok(()) => Ok(()),
        Err(_) if ctx.cancel_token().is_cancelled() => Err(Cancelled),
        // Each step copies its own, as without a batch.
        Err(error) => {
            tracing::warn!("a batch of copies failed, the rest copied one by one: {error:#}");
            Ok(())
        }
    }
}

/// The stamp of the file or link at `path_of(item)` for each of `items`,
/// [`AT_ONCE`] looked at at once, in their order.
fn looked_at<T: Sync>(items: &[T], path_of: impl Fn(&T) -> PathBuf + Sync) -> Vec<Option<Stamp>> {
    let next = AtomicUsize::new(0);
    let found: Mutex<Vec<Option<Stamp>>> = Mutex::new(vec![None; items.len()]);
    std::thread::scope(|s| {
        for _ in 0..AT_ONCE.min(items.len()) {
            s.spawn(|| {
                loop {
                    let n = next.fetch_add(1, Ordering::Relaxed);
                    let Some(item) = items.get(n) else {
                        return;
                    };
                    let stamp = Stamp::of(&path_of(item));
                    found.lock()[n] = stamp;
                }
            });
        }
    });
    found.into_inner()
}

/// Take back what a batch placed for steps that never ran, the plan cut
/// short before them (a step failed, Cancel): no Undo holds it. A move's
/// original stays where it was. What it staged goes with the plan's
/// staging folder.
pub(super) fn take_back_unclaimed(ctx: &StepContext) {
    for (_, what) in ctx.unclaimed() {
        // Still what it placed.
        if let Batched::Placed(at, stamp) = what
            && Stamp::of(&at) == Some(stamp)
        {
            tracing::debug!(at = %at.display(), "a placed copy whose step never ran taken back");
            let _ = std::fs::remove_file(&at);
        }
    }
}

/// Remove the originals of moves the plan's batches copied from a remote,
/// which their steps left for now (`StepContext::remove_later`): each
/// looked at again, [`AT_ONCE`] at a time; those still as copied removed
/// together where a transfer removes in their folder ([`Transfer::remove`]),
/// else, and whatever it left, one by one. Each is done, whatever the
/// others say: the first failure is told.
///
/// # Errors
/// An original changed since it was copied (it stays), or what removing
/// one met.
pub(super) fn remove_deferred(ctx: &StepContext) -> anyhow::Result<()> {
    let removing = ctx.take_removing();
    if removing.is_empty() {
        return Ok(());
    }
    // A last look, its stats at once: a moved original may have changed
    // since its step found it unchanged.
    let next = AtomicUsize::new(0);
    let changed: Mutex<Vec<usize>> = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..AT_ONCE.min(removing.len()) {
            s.spawn(|| {
                loop {
                    let n = next.fetch_add(1, Ordering::Relaxed);
                    let Some((src, copied_as, _)) = removing.get(n) else {
                        return;
                    };
                    if std::fs::symlink_metadata(src).is_ok()
                        && super::builtin::state_of(src) != copied_as.0
                    {
                        changed.lock().push(n);
                    }
                }
            });
        }
    });
    let changed = changed.into_inner();
    let mut failed = changed.first().map(|&n| {
        let (src, _, dest_dir) = &removing[n];
        super::builtin::changed_since(src, dest_dir)
    });
    let mut by_folder: BTreeMap<PathBuf, Vec<(OsString, PathBuf)>> = BTreeMap::new();
    for (n, (src, _, dest_dir)) in removing.iter().enumerate() {
        if let (Some(folder), Some(name), false) =
            (src.parent(), src.file_name(), changed.contains(&n))
        {
            by_folder
                .entry(folder.to_path_buf())
                .or_default()
                .push((name.to_os_string(), dest_dir.clone()));
        }
    }
    let control = ctx.control();
    let transfers = ctx.transfers().map(Transfers::all).unwrap_or_default();
    for (folder, entries) in by_folder {
        let names: Vec<OsString> = entries.iter().map(|(name, _)| name.clone()).collect();
        let mut removed = false;
        for transfer in &transfers {
            if !transfer.removes(&folder) {
                continue;
            }
            match transfer.remove(&folder, &names, &control) {
                Err(error) if error.is::<NotServed>() => {}
                Ok(()) => {
                    ctx.changed_beside(&folder);
                    removed = true;
                    break;
                }
                Err(error) => {
                    ctx.changed_beside(&folder);
                    tracing::warn!(
                        "{} removing {} entries failed, the rest removed one by one: {error:#}",
                        transfer.name(),
                        names.len()
                    );
                    break;
                }
            }
        }
        // Removed by a transfer: each look through the mount would be a
        // round trip, and it may say for a moment what it removed is there.
        if removed {
            continue;
        }
        for (name, dest_dir) in &entries {
            let src = folder.join(name);
            // Gone meanwhile is removed.
            let gone = |error: &anyhow::Error| {
                error.chain().any(|cause| {
                    cause
                        .downcast_ref::<std::io::Error>()
                        .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound)
                })
            };
            match super::builtin::remove_original(&src) {
                Err(error) if !gone(&error) => {
                    failed.get_or_insert(error.context(format!(
                        "“{}” was copied whole to “{}”, but its original could not all be removed",
                        src.display(),
                        dest_dir.display()
                    )));
                }
                _ => {}
            }
        }
    }
    failed.map_or(Ok(()), Err)
}

/// Copy `taken` of `from` into `into` (with `temp`, beside what it
/// holds) by a plugin's transfer that serves the two folders (`alone`, a
/// file alone: one that takes it), else by Skwir's own.
fn run(
    from: &Path,
    (taken, alone): (&[&Taken], bool),
    into: &Path,
    temp: Option<&Path>,
    copied: &(dyn Fn(u64) + Sync),
    placed: &(dyn Fn(&OsStr) + Sync),
    ctx: &StepContext,
) -> anyhow::Result<()> {
    let names: Vec<OsString> = taken.iter().map(|t| t.name.clone()).collect();
    let control = ctx.control();
    control.wait_while_paused();
    // A file alone within one remote goes as a batch does.
    let within = crate::fs::reach::of(from)
        .zip(crate::fs::reach::of(into))
        .is_some_and(|(a, b)| a.point == b.point);
    let serving = ctx
        .transfers()
        .map(|t| t.serving(from, into, alone && !within))
        .unwrap_or_default();
    for transfer in serving {
        tracing::debug!(
            copies = names.len(),
            beside = temp.is_some(),
            by = transfer.name(),
            "a batch of copies"
        );
        let told = Told { copied, placed };
        match transfer.copy(from, &names, into, temp, &told, &control) {
            // It made nothing: the next.
            Err(error) if error.is::<NotServed>() => {}
            ran => {
                // Placed where they go on a remote, beside its mount (here
                // a watch sees them), failed or not.
                if temp.is_some() && crate::fs::reach::of(into).is_some() {
                    ctx.changed_beside(into);
                }
                return ran.with_context(|| {
                    format!("{} copying {} entries", transfer.name(), names.len())
                });
            }
        }
    }
    tracing::debug!(
        copies = names.len(),
        beside = temp.is_some(),
        by = "skwir",
        "a batch of copies"
    );
    copy_at_once(from, &names, into, temp, copied, placed, &control)
        .with_context(|| format!("skwir copying {} entries", names.len()))
}

/// Skwir's own batch copy (the [`Transfer::copy`] it does where no
/// plugin's serves): the trees' folders made first, in order, their files
/// [`AT_ONCE`] at a time ([`DOWN_AT_ONCE`] from a remote onto this
/// machine), each given its mode where it was made otherwise
/// and its time, then the folders theirs, deepest first. While the plan
/// is paused, each waits before its next file. Beside what is there, each
/// makes its files in a folder of its own in `temp`: the kernel takes a
/// folder's lock to make a name in it, and to rename one out of it, so
/// one folder for all would make them one at a time.
///
/// # Errors
/// The first copy that failed, or the cancel.
pub(super) fn copy_at_once(
    from: &Path,
    names: &[OsString],
    into: &Path,
    temp: Option<&Path>,
    copied: &(dyn Fn(u64) + Sync),
    placed: &(dyn Fn(&OsStr) + Sync),
    control: &Control,
) -> anyhow::Result<()> {
    let cancel = control.cancel();
    let layout = match temp {
        // Files and links alone are placed beside what is there.
        Some(_) => Layout::files(from, names, into),
        None => Layout::made(from, names, into)?,
    };
    let files = &layout.files;
    let next = AtomicUsize::new(0);
    let total = AtomicU64::new(0);
    let failed: Mutex<Option<anyhow::Error>> = Mutex::new(None);
    let stop = AtomicBool::new(false);
    let at_once = if crate::fs::reach::of(from).is_some() && crate::fs::reach::of(into).is_none() {
        DOWN_AT_ONCE
    } else {
        AT_ONCE
    };
    let workers = at_once.min(files.len());
    // A folder of this call's in `temp`, which other calls share, and one
    // in it for each worker.
    let asides = match temp {
        Some(temp) => {
            let call = tempfile::Builder::new()
                .prefix("at-once-")
                .tempdir_in(temp)?
                .keep();
            (0..workers)
                .map(|k| {
                    let aside = call.join(k.to_string());
                    std::fs::create_dir(&aside).map(|()| aside)
                })
                .collect::<std::io::Result<Vec<_>>>()?
        }
        None => Vec::new(),
    };
    let (next, total, failed, stop) = (&next, &total, &failed, &stop);
    std::thread::scope(|s| {
        for k in 0..workers {
            let aside = asides.get(k);
            s.spawn(move || {
                loop {
                    control.wait_while_paused();
                    if stop.load(Ordering::Relaxed) || cancel.is_cancelled() {
                        return;
                    }
                    let Some((src, dst)) = files.get(next.fetch_add(1, Ordering::Relaxed)) else {
                        return;
                    };
                    let wrote = |n: u64| copied(total.fetch_add(n, Ordering::Relaxed) + n);
                    let made = match aside {
                        // Made aside, then placed, never over anything.
                        Some(aside) => {
                            let aside = aside.join(dst.file_name().unwrap_or_default());
                            copy_one(src, &aside, &wrote).and_then(|()| {
                                match place(&aside, dst) {
                                    // Taken meanwhile: left to its step.
                                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                                        Ok(())
                                    }
                                    Ok(()) => {
                                        placed(dst.file_name().unwrap_or_default());
                                        Ok(())
                                    }
                                    Err(e) => Err(e.into()),
                                }
                            })
                        }
                        None => copy_one(src, dst, &wrote),
                    };
                    if let Err(error) = made {
                        stop.store(true, Ordering::Relaxed);
                        failed.lock().get_or_insert(error);
                    }
                }
            });
        }
    });
    if let Some(error) = failed.lock().take() {
        return Err(error);
    }
    anyhow::ensure!(!cancel.is_cancelled(), "cancelled");
    layout.finish()
}

/// What a batch's copy makes of the entries `names` of a folder in
/// another ([`Layout::made`]): their folders, made, and their files and
/// links, to copy. For a plugin's [`Transfer`] that copies files alone
/// (`gio copy`).
pub struct Layout {
    /// Each file or link to copy, and where to.
    pub files: Vec<(PathBuf, PathBuf)>,
    /// Each folder made, and the one it is a copy of, in order.
    folders: Vec<(PathBuf, PathBuf)>,
}

impl Layout {
    /// The trees `names` of `from` laid out in `into`: their folders made
    /// now, in order, and their files and links listed.
    ///
    /// # Errors
    /// When a folder cannot be read or made.
    pub fn made(from: &Path, names: &[OsString], into: &Path) -> anyhow::Result<Layout> {
        let mut layout = Layout {
            files: Vec::new(),
            folders: Vec::new(),
        };
        for name in names {
            layout.lay_out(&from.join(name), &into.join(name))?;
        }
        Ok(layout)
    }

    /// The files and links `names` of `from`, to copy into `into` (what a
    /// batch places beside what is there, never a folder).
    pub fn files(from: &Path, names: &[OsString], into: &Path) -> Layout {
        Layout {
            files: names
                .iter()
                .map(|name| (from.join(name), into.join(name)))
                .collect(),
            folders: Vec::new(),
        }
    }

    fn lay_out(&mut self, src: &Path, dst: &Path) -> anyhow::Result<()> {
        let meta = std::fs::symlink_metadata(src)?;
        if meta.is_dir() {
            std::fs::create_dir(dst)?;
            self.folders.push((src.to_path_buf(), dst.to_path_buf()));
            for entry in std::fs::read_dir(src)? {
                let name = entry?.file_name();
                self.lay_out(&src.join(&name), &dst.join(&name))?;
            }
        } else {
            self.files.push((src.to_path_buf(), dst.to_path_buf()));
        }
        Ok(())
    }

    /// Give the folders made their sources' modes and times, deepest
    /// first, once their files are in: each name made in a folder changes
    /// its time.
    ///
    /// # Errors
    /// When a mode cannot be set.
    pub fn finish(&self) -> anyhow::Result<()> {
        for (src, dst) in self.folders.iter().rev() {
            let meta = std::fs::symlink_metadata(src)?;
            if let Ok(modified) = meta.modified()
                && let Ok(folder) = super::sys::open_to_set_times(dst)
            {
                let _ = folder.set_modified(modified);
            }
            super::sys::mode_kept(std::fs::set_permissions(dst, meta.permissions()))?;
        }
        Ok(())
    }
}

/// Give each copy at `.1` the modification time of its source at `.0`,
/// sixteen at once: for a transfer whose copies keep none (`sftp`'s `cp`,
/// on the server), each a round trip through a mount, which takes no lock
/// of the folder. Links keep their own.
///
/// # Errors
/// The first that could not be read or given.
pub fn give_times(pairs: &[(PathBuf, PathBuf)]) -> anyhow::Result<()> {
    let next = AtomicUsize::new(0);
    let failed: Mutex<Option<anyhow::Error>> = Mutex::new(None);
    std::thread::scope(|s| {
        for _ in 0..DOWN_AT_ONCE.min(pairs.len()) {
            s.spawn(|| {
                while let Some((src, dst)) = pairs.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let given = std::fs::symlink_metadata(src)
                        .and_then(|meta| {
                            if meta.is_symlink() {
                                return Ok(());
                            }
                            super::sys::set_modified(dst, meta.modified()?)
                        })
                        .with_context(|| {
                            format!("“{}” could not be given its time", dst.display())
                        });
                    if let Err(error) = given {
                        failed.lock().get_or_insert(error);
                    }
                }
            });
        }
    });
    failed.into_inner().map_or(Ok(()), Err)
}

/// Place the file or link `made` at `dst`, never over anything. A file is
/// linked there, then its first name removed: the kernel takes the
/// destination folder's lock for that link alone, where a rename takes
/// it to look the name up, and again, through a FUSE mount that refuses
/// the no-replace flag (sshfs), to look and rename (`sys::rename_noreplace`):
/// two round trips a file held one at a time, not four. Where links are
/// refused (most FUSE file systems but sshfs), renamed. A plugin's
/// [`Transfer`] places with it what it made beside what is there.
///
/// # Errors
/// `AlreadyExists` when `dst` is taken; what the system said otherwise.
pub fn place(made: &Path, dst: &Path) -> std::io::Result<()> {
    let file = std::fs::symlink_metadata(made).is_ok_and(|m| m.is_file());
    if file {
        match std::fs::hard_link(made, dst) {
            Ok(()) => {
                // Left, it goes with the staging folder.
                let _ = std::fs::remove_file(made);
                return Ok(());
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Err(e),
            Err(_) => {}
        }
    }
    super::sys::rename_noreplace(made, dst)
}

/// Copy the file or link `src` to `dst`, which is not there: its bytes
/// told to `wrote` as they go, its mode where it was made otherwise, its
/// time.
fn copy_one(src: &Path, dst: &Path, wrote: &dyn Fn(u64)) -> anyhow::Result<()> {
    use std::io::{Read, Write};
    let meta = std::fs::symlink_metadata(src)?;
    if meta.file_type().is_symlink() {
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(std::fs::read_link(src)?, dst)?;
            return Ok(());
        }
        #[cfg(not(unix))]
        anyhow::bail!("“{}” is a link", src.display());
    }
    let mut sf = crate::fs::file::open_regular(src)?;
    let mut df = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dst)?;
    let made = df.metadata()?;
    let mut buf = vec![0; 1 << 20];
    loop {
        let n = match sf.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        };
        df.write_all(&buf[..n])?;
        wrote(n as u64);
    }
    let meta = sf.metadata()?;
    if made.permissions() != meta.permissions() {
        super::sys::mode_kept(df.set_permissions(meta.permissions()))?;
    }
    let _ = df.set_modified(meta.modified()?);
    Ok(())
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::time::{Duration, SystemTime};

    use super::*;
    use crate::op::{EntryRef, Executor, Plan};
    use crate::scheduler::Scheduler;

    /// A folder of the test's on the home's file system, which keeps
    /// links, modes and attributes as a disk does.
    fn home_tempdir() -> tempfile::TempDir {
        let base = dirs::home_dir().unwrap().join(".cache/skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        tempfile::Builder::new()
            .prefix("t-")
            .tempdir_in(base)
            .unwrap()
    }

    fn when() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000)
    }

    fn file(path: &Path, text: &str, mode: u32) {
        std::fs::write(path, text).unwrap();
        let f = std::fs::File::options().write(true).open(path).unwrap();
        f.set_modified(when()).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    /// A folder of what a paste holds: files of several modes, a folder of
    /// folders, a link.
    fn served(root: &Path) -> Vec<PathBuf> {
        let src = root.join("served");
        std::fs::create_dir_all(src.join("sub folder/deeper")).unwrap();
        file(&src.join("it's a.txt"), "a", 0o644);
        file(&src.join("run.sh"), "#!/bin/sh", 0o755);
        file(&src.join("kept.txt"), "kept", 0o444);
        file(&src.join("sub folder/c.txt"), "c", 0o600);
        file(&src.join("sub folder/deeper/d.txt"), "d", 0o644);
        std::os::unix::fs::symlink("it's a.txt", src.join("to a")).unwrap();
        ["it's a.txt", "run.sh", "kept.txt", "sub folder", "to a"]
            .into_iter()
            .map(|n| src.join(n))
            .collect()
    }

    fn copies(entries: &[PathBuf], dest: &Path) -> Plan {
        Plan::with_steps(
            "copy",
            entries
                .iter()
                .map(|e| {
                    Box::new(Copy {
                        entry: EntryRef::from_path(e).unwrap(),
                        dest_dir: dest.to_path_buf(),
                        new_name: None,
                        preserve_attributes: true,
                    }) as Box<dyn Operation>
                })
                .collect(),
        )
    }

    /// `copy` is `src` as a copy keeps it: names, kinds, bytes, modes and
    /// files' times, all the way down.
    fn same(src: &Path, copy: &Path) {
        let (a, b) = (
            std::fs::symlink_metadata(src).unwrap(),
            std::fs::symlink_metadata(copy).unwrap(),
        );
        assert_eq!(a.file_type(), b.file_type(), "{}", copy.display());
        if a.is_symlink() {
            assert_eq!(
                std::fs::read_link(src).unwrap(),
                std::fs::read_link(copy).unwrap()
            );
            return;
        }
        assert_eq!(a.mode(), b.mode(), "{}", copy.display());
        if a.is_file() {
            assert_eq!(std::fs::read(src).unwrap(), std::fs::read(copy).unwrap());
            assert_eq!(a.modified().unwrap(), b.modified().unwrap());
            return;
        }
        let mut names: Vec<_> = std::fs::read_dir(src)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        names.sort();
        let mut theirs: Vec<_> = std::fs::read_dir(copy)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        theirs.sort();
        assert_eq!(names, theirs, "{}", copy.display());
        for name in names {
            same(&src.join(&name), &copy.join(&name));
        }
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn copies_from_a_remote_go_in_one_batch_and_each_step_places_its_own() {
        let tmp = home_tempdir();
        let entries = served(tmp.path());
        let dest = tmp.path().join("here");
        std::fs::create_dir(&dest).unwrap();
        crate::fs::reach::pretend_remote(&tmp.path().join("served"));
        let executor = Executor::new(Scheduler::new(2));
        let r = executor.execute_blocking(&copies(&entries, &dest), &CancelToken::new());
        assert!(r.error.is_none(), "{:?}", r.error);
        assert_eq!(r.applied, entries.len());
        assert_eq!(BATCHES.load(Ordering::Relaxed), 1);
        for e in &entries {
            same(e, &dest.join(e.file_name().unwrap()));
        }
        assert_eq!(
            names(&dest),
            ["it's a.txt", "kept.txt", "run.sh", "sub folder", "to a"],
            "nothing staged left"
        );
        let undone = executor.undo_blocking(&CancelToken::new()).unwrap();
        assert!(undone.error.is_none(), "{:?}", undone.error);
        assert!(names(&dest).is_empty(), "each step's undo takes its copy");
    }

    /// A transfer that copies as Skwir's own does, counting its calls, and
    /// fails after copying when told to.
    struct Fake {
        calls: Arc<Mutex<Vec<Vec<OsString>>>>,
        fails: bool,
        cancels: Option<CancelToken>,
        /// Cancels once it placed files beside (its second call).
        cancels_after: Option<CancelToken>,
        /// A file it changes once it copied, once, as someone else might.
        touches: Mutex<Option<PathBuf>>,
        /// Removes moved originals ([`Transfer::removes`]), each batch's
        /// names said here.
        removals: Option<Arc<Mutex<Vec<Vec<OsString>>>>>,
        /// Takes a file alone ([`Transfer::serves_alone`]).
        alone: bool,
    }

    impl Transfer for Fake {
        fn name(&self) -> &'static str {
            "fake"
        }
        fn serves(&self, _: &Path, _: &Path) -> bool {
            true
        }
        fn serves_alone(&self, _: &Path, _: &Path) -> bool {
            self.alone
        }
        fn removes(&self, _: &Path) -> bool {
            self.removals.is_some()
        }
        fn remove(&self, folder: &Path, names: &[OsString], _: &Control) -> anyhow::Result<()> {
            let removals = self.removals.as_ref().ok_or(NotServed)?;
            removals.lock().push(names.to_vec());
            for name in names {
                super::super::builtin::remove_original(&folder.join(name))?;
            }
            Ok(())
        }
        fn copy(
            &self,
            from: &Path,
            names: &[OsString],
            into: &Path,
            temp: Option<&Path>,
            told: &Told<'_>,
            control: &Control,
        ) -> anyhow::Result<()> {
            let (copied, placed) = (told.copied, told.placed);
            self.calls.lock().push(names.to_vec());
            if let Some(cancel) = &self.cancels {
                cancel.cancel();
                anyhow::bail!("cancelled");
            }
            copy_at_once(
                from,
                &names[..names.len() / 2],
                into,
                temp,
                copied,
                placed,
                control,
            )?;
            anyhow::ensure!(!self.fails, "the server went away");
            copy_at_once(
                from,
                &names[names.len() / 2..],
                into,
                temp,
                copied,
                placed,
                control,
            )?;
            if let Some(cancel) = self.cancels_after.as_ref().filter(|_| temp.is_some()) {
                cancel.cancel();
                anyhow::bail!("cancelled");
            }
            if let Some(touched) = self.touches.lock().take() {
                use std::io::Write;
                std::fs::OpenOptions::new()
                    .append(true)
                    .open(&touched)?
                    .write_all(b" and more")?;
            }
            Ok(())
        }
    }

    fn with_fake(
        fails: bool,
        cancels: Option<CancelToken>,
    ) -> (Arc<Executor>, Arc<Mutex<Vec<Vec<OsString>>>>) {
        with_fake_cancelling(fails, cancels, None)
    }

    fn with_fake_cancelling(
        fails: bool,
        cancels: Option<CancelToken>,
        cancels_after: Option<CancelToken>,
    ) -> (Arc<Executor>, Arc<Mutex<Vec<Vec<OsString>>>>) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let executor = executor_with(Fake {
            calls: Arc::clone(&calls),
            fails,
            cancels,
            cancels_after,
            touches: Mutex::default(),
            removals: None,
            alone: false,
        });
        (executor, calls)
    }

    fn executor_with(fake: Fake) -> Arc<Executor> {
        let transfers = Arc::new(Transfers::default());
        transfers.register("test", Arc::new(fake));
        let executor = Executor::new(Scheduler::new(2));
        executor.set_transfers(transfers);
        executor
    }

    fn moves(entries: &[PathBuf], dest: &Path) -> Plan {
        Plan::with_steps(
            "move",
            entries
                .iter()
                .map(|e| {
                    Box::new(Move::new(
                        EntryRef::from_path(e).unwrap(),
                        dest.to_path_buf(),
                    )) as Box<dyn Operation>
                })
                .collect(),
        )
    }

    #[test]
    fn moves_from_a_remote_go_in_one_batch_and_their_originals_go_after() {
        let tmp = home_tempdir();
        let entries = served(&tmp.path().join("there"));
        let kept = served(&tmp.path().join("as it was"));
        let from = tmp.path().join("there/served");
        let dest = tmp.path().join("here");
        std::fs::create_dir(&dest).unwrap();
        crate::fs::reach::pretend_remote(&from);
        let executor = Executor::new(Scheduler::new(2));
        let r = executor.execute_blocking(&moves(&entries, &dest), &CancelToken::new());
        assert!(r.error.is_none(), "{:?}", r.error);
        assert_eq!(r.applied, entries.len());
        assert_eq!(BATCHES.load(Ordering::Relaxed), 1);
        for e in &kept {
            same(e, &dest.join(e.file_name().unwrap()));
        }
        assert!(names(&from).is_empty(), "the originals removed");
        let undone = executor.undo_blocking(&CancelToken::new()).unwrap();
        assert!(undone.error.is_none(), "{:?}", undone.error);
        assert_eq!(BATCHES.load(Ordering::Relaxed), 2, "back in a batch too");
        assert!(names(&dest).is_empty());
        for e in &kept {
            same(e, &from.join(e.file_name().unwrap()));
        }
    }

    /// Moved from a remote where a transfer removes together, the
    /// originals go at the plan's end, in one call for their folder.
    #[test]
    fn moved_originals_go_together_where_a_transfer_removes_them() {
        let tmp = home_tempdir();
        let entries = served(&tmp.path().join("there"));
        let kept = served(&tmp.path().join("as it was"));
        let from = tmp.path().join("there/served");
        let dest = tmp.path().join("here");
        std::fs::create_dir(&dest).unwrap();
        crate::fs::reach::pretend_remote(&from);
        let removals = Arc::new(Mutex::new(Vec::new()));
        let executor = executor_with(Fake {
            calls: Arc::default(),
            fails: false,
            cancels: None,
            cancels_after: None,
            touches: Mutex::default(),
            removals: Some(Arc::clone(&removals)),
            alone: false,
        });
        let r = executor.execute_blocking(&moves(&entries, &dest), &CancelToken::new());
        assert!(r.error.is_none(), "{:?}", r.error);
        assert_eq!(r.applied, entries.len());
        let mut removed: Vec<OsString> = removals.lock().concat();
        removed.sort();
        let mut all: Vec<OsString> = entries
            .iter()
            .map(|e| e.file_name().unwrap().into())
            .collect();
        all.sort();
        assert_eq!(removals.lock().len(), 1, "one call for the folder");
        assert_eq!(removed, all);
        assert!(names(&from).is_empty(), "the originals removed");
        for e in &kept {
            same(e, &dest.join(e.file_name().unwrap()));
        }
    }

    /// An original changed after its step found it unchanged, before the
    /// plan's end, stays, and the plan says so; the others go.
    #[test]
    fn an_original_changed_before_the_plan_s_end_stays() {
        let tmp = home_tempdir();
        let entries = served(tmp.path());
        let ctx = StepContext::detached();
        for e in &entries {
            let state = super::super::builtin::state_of(e);
            ctx.remove_later(e.clone(), CopiedAs(state), tmp.path().join("elsewhere"));
        }
        file(&entries[0], "changed since", 0o644);
        let error = remove_deferred(&ctx).unwrap_err().to_string();
        assert!(error.contains("changed after it was copied"), "{error}");
        assert!(entries[0].exists(), "the changed one stays");
        assert!(entries[1..].iter().all(|e| !e.exists()), "the others go");
    }

    fn fake_removing(removals: &Arc<Mutex<Vec<Vec<OsString>>>>) -> Arc<Executor> {
        executor_with(Fake {
            calls: Arc::default(),
            fails: false,
            cancels: None,
            cancels_after: None,
            touches: Mutex::default(),
            removals: Some(Arc::clone(removals)),
            alone: false,
        })
    }

    /// Deletions for good in a remote's folder go together, by the
    /// transfer that removes there, and each step says it deleted its own.
    #[test]
    fn deletions_in_a_remote_folder_go_together() {
        let tmp = home_tempdir();
        let entries = served(tmp.path());
        let from = tmp.path().join("served");
        crate::fs::reach::pretend_remote(&from);
        let removals = Arc::new(Mutex::new(Vec::new()));
        let executor = fake_removing(&removals);
        let plan = Plan::with_steps(
            "delete",
            entries
                .iter()
                .map(|e| {
                    Box::new(Delete {
                        entry: EntryRef::from_path(e).unwrap(),
                    }) as Box<dyn Operation>
                })
                .collect(),
        );
        let r = executor.execute_blocking(&plan, &CancelToken::new());
        assert!(r.error.is_none(), "{:?}", r.error);
        assert_eq!(r.applied, entries.len(), "each said deleted, none skipped");
        assert_eq!(removals.lock().len(), 1, "one call for the folder");
        assert!(names(&from).is_empty());
    }

    /// A copy onto a volume that keeps no trash is taken back by Undo for
    /// good, together, and copied again by Redo; a copy changed since
    /// stays, and Undo says so.
    #[test]
    fn copies_onto_a_remote_are_taken_back_by_undo_and_made_again_by_redo() {
        let tmp = home_tempdir();
        let entries = served(&tmp.path().join("here"));
        let dest = tmp.path().join("there");
        std::fs::create_dir(&dest).unwrap();
        crate::fs::reach::pretend_remote(&dest);
        let removals = Arc::new(Mutex::new(Vec::new()));
        let executor = fake_removing(&removals);
        let r = executor.execute_blocking(&copies(&entries, &dest), &CancelToken::new());
        assert!(r.error.is_none(), "{:?}", r.error);
        let undone = executor.undo_blocking(&CancelToken::new()).unwrap();
        assert!(undone.error.is_none(), "{:?}", undone.error);
        assert_eq!(removals.lock().len(), 1, "taken back in one call");
        assert!(names(&dest).is_empty(), "the copies gone");
        let redone = executor.redo_blocking(&CancelToken::new()).unwrap();
        assert!(redone.error.is_none(), "{:?}", redone.error);
        for e in &entries {
            same(e, &dest.join(e.file_name().unwrap()));
        }
        // Changed since it was copied again: it stays.
        let changed = dest.join("kept.txt");
        std::fs::set_permissions(&changed, std::fs::Permissions::from_mode(0o644)).unwrap();
        file(&changed, "edited there", 0o644);
        let undone = executor.undo_blocking(&CancelToken::new()).unwrap();
        let error = undone.error.unwrap_or_default();
        assert!(error.contains("changed since it was made"), "{error}");
        assert_eq!(names(&dest), ["kept.txt"], "the changed one stays");
    }

    /// The trash of a network folder a program mounts is none: a step
    /// that would put something there says so before anything is done.
    #[test]
    fn a_trash_where_none_is_kept_is_refused_before_anything_is_done() {
        let tmp = home_tempdir();
        let entries = served(tmp.path());
        crate::fs::reach::pretend_remote(&tmp.path().join("served"));
        let step = super::super::Trash {
            entry: EntryRef::from_path(&entries[0]).unwrap(),
        };
        let status = super::super::Operation::check(&step, &StepContext::detached());
        assert!(
            matches!(&status, super::super::StepStatus::Conflict { reason } if reason.contains("keeps no trash")),
            "{status:?}"
        );
    }

    /// What a move's source was as its batch took it is what its step
    /// compares: changed since (here once its batch's first call copied
    /// the folder, before its second copied the file), it stays, and the
    /// plan stops there.
    #[test]
    fn a_moved_source_changed_after_its_batch_stays() {
        let tmp = home_tempdir();
        let entries = served(tmp.path());
        let from = tmp.path().join("served");
        let dest = tmp.path().join("here");
        std::fs::create_dir(&dest).unwrap();
        crate::fs::reach::pretend_remote(&from);
        let executor = executor_with(Fake {
            calls: Arc::default(),
            fails: false,
            cancels: None,
            cancels_after: None,
            touches: Mutex::new(Some(from.join("run.sh"))),
            removals: None,
            alone: false,
        });
        let r = executor.execute_blocking(&moves(&entries, &dest), &CancelToken::new());
        let error = r.error.expect("the changed one stops the plan");
        assert!(error.contains("changed after it was copied"), "{error}");
        assert_eq!(r.applied, 1, "the one before it moved");
        assert_eq!(
            std::fs::read_to_string(from.join("run.sh")).unwrap(),
            "#!/bin/sh and more",
            "kept as changed"
        );
        assert_eq!(names(&from), ["kept.txt", "run.sh", "sub folder", "to a"]);
        assert_eq!(
            names(&dest),
            ["it's a.txt", "run.sh"],
            "what the batch placed for the steps after taken back"
        );
    }

    /// A plan cut short before the steps whose copies a batch placed
    /// leaves none of them: no Undo would hold them.
    #[test]
    fn a_plan_stopped_before_their_steps_takes_back_what_its_batch_placed() {
        let tmp = home_tempdir();
        let entries = served(tmp.path());
        let dest = tmp.path().join("on the server");
        std::fs::create_dir(&dest).unwrap();
        file(&dest.join("it's a.txt"), "already there", 0o644);
        crate::fs::reach::pretend_remote(&dest);
        let (executor, calls) = with_fake(false, None);
        let r = executor.execute_blocking(&copies(&entries, &dest), &CancelToken::new());
        assert!(r.error.is_some_and(|e| e.contains("already there")));
        assert_eq!(r.applied, 0);
        assert!(!calls.lock().is_empty(), "the batch ran");
        assert_eq!(names(&dest), ["it's a.txt"]);
        assert_eq!(
            std::fs::read_to_string(dest.join("it's a.txt")).unwrap(),
            "already there"
        );
    }

    #[test]
    fn a_batch_waits_while_the_plan_is_paused_and_stops_when_cancelled() {
        let tmp = home_tempdir();
        let entries = served(tmp.path());
        let from = tmp.path().join("served");
        let listed: Vec<OsString> = entries
            .iter()
            .map(|e| e.file_name().unwrap().into())
            .collect();
        let pause = Arc::new(Pause::default());
        // Paused for a while, then let go, or cancelled: whether it was
        // still copying then, and had copied a file.
        let held = |into: &Path, cancel: &CancelToken, resume: bool| {
            std::fs::create_dir(into).unwrap();
            assert!(pause.pause());
            let control = Control::new(cancel.clone(), Arc::clone(&pause));
            std::thread::scope(|s| {
                let copying = s
                    .spawn(|| copy_at_once(&from, &listed, into, None, &|_| {}, &|_| {}, &control));
                std::thread::sleep(Duration::from_millis(300));
                let waiting = !copying.is_finished() && !into.join("run.sh").exists();
                if resume {
                    drop(pause.resume());
                } else {
                    cancel.cancel();
                }
                (waiting, copying.join().unwrap())
            })
        };
        let resumed = tmp.path().join("resumed");
        let (waiting, copied) = held(&resumed, &CancelToken::new(), true);
        assert!(waiting, "nothing copied while paused");
        assert!(copied.is_ok(), "{copied:?}");
        for e in &entries {
            same(e, &resumed.join(e.file_name().unwrap()));
        }
        let cancelled = tmp.path().join("cancelled");
        let (waiting, copied) = held(&cancelled, &CancelToken::new(), false);
        assert!(waiting);
        assert!(copied.is_err(), "cancelled while paused, it stops");
        assert!(!cancelled.join("run.sh").exists());
        drop(pause.resume());
    }

    #[test]
    fn a_plugins_transfer_makes_the_batch_it_serves() {
        let tmp = home_tempdir();
        let entries = served(tmp.path());
        let dest = tmp.path().join("on the server");
        std::fs::create_dir(&dest).unwrap();
        crate::fs::reach::pretend_remote(&dest);
        let (executor, calls) = with_fake(false, None);
        let r = executor.execute_blocking(&copies(&entries, &dest), &CancelToken::new());
        assert!(r.error.is_none(), "{:?}", r.error);
        let calls = calls.lock();
        // The folder staged, the files and the link placed beside.
        assert_eq!(calls.len(), 2);
        let mut taken: Vec<OsString> = calls.iter().flatten().cloned().collect();
        taken.sort();
        let mut all: Vec<OsString> = entries
            .iter()
            .map(|e| e.file_name().unwrap().into())
            .collect();
        all.sort();
        assert_eq!(taken, all, "each once");
        for e in &entries {
            same(e, &dest.join(e.file_name().unwrap()));
        }
        assert_eq!(names(&dest).len(), entries.len(), "nothing staged left");
    }

    /// A file alone goes by its step, its bytes through the mount; but by
    /// a transfer that takes a file alone faster than the mount does.
    #[test]
    fn a_file_alone_goes_by_a_transfer_that_takes_one() {
        let tmp = home_tempdir();
        let entries = served(tmp.path());
        let file = entries
            .iter()
            .find(|e| e.file_name().is_some_and(|n| n == "kept.txt"))
            .unwrap();
        crate::fs::reach::pretend_remote(&tmp.path().join("served"));
        for alone in [false, true] {
            let dest = tmp.path().join(format!("here {alone}"));
            std::fs::create_dir(&dest).unwrap();
            let calls = Arc::new(Mutex::new(Vec::new()));
            let executor = executor_with(Fake {
                calls: Arc::clone(&calls),
                fails: false,
                cancels: None,
                cancels_after: None,
                touches: Mutex::default(),
                removals: None,
                alone,
            });
            let plan = copies(std::slice::from_ref(file), &dest);
            let r = executor.execute_blocking(&plan, &CancelToken::new());
            assert!(r.error.is_none(), "{:?}", r.error);
            assert_eq!(calls.lock().len(), usize::from(alone));
            same(file, &dest.join("kept.txt"));
            assert_eq!(names(&dest), ["kept.txt"], "nothing staged left");
        }
    }

    #[test]
    fn what_a_batch_cannot_take_is_copied_by_its_step() {
        let tmp = home_tempdir();
        let mut entries = served(tmp.path());
        let src = tmp.path().join("served");
        file(&src.join("tagged.txt"), "tagged", 0o644);
        let tagged = crate::fs::xattr::set(src.join("tagged.txt"), "user.xdg.tags", b"blue");
        file(&src.join("twice.txt"), "twice", 0o644);
        std::fs::hard_link(src.join("twice.txt"), src.join("twice again.txt")).unwrap();
        entries.extend(["tagged.txt", "twice.txt", "twice again.txt"].map(|n| src.join(n)));
        let dest = tmp.path().join("on the server");
        std::fs::create_dir(&dest).unwrap();
        crate::fs::reach::pretend_remote(&dest);
        let (executor, calls) = with_fake(false, None);
        let r = executor.execute_blocking(&copies(&entries, &dest), &CancelToken::new());
        assert!(r.error.is_none(), "{:?}", r.error);
        let taken: Vec<OsString> = calls.lock().iter().flatten().cloned().collect();
        assert!(
            !taken
                .iter()
                .any(|n| n == "twice.txt" || n == "twice again.txt")
        );
        if tagged.is_ok() {
            assert!(!taken.iter().any(|n| n == "tagged.txt"));
            assert_eq!(
                crate::fs::xattr::get(dest.join("tagged.txt"), "user.xdg.tags").unwrap(),
                Some(b"blue".to_vec()),
                "its step kept its tags"
            );
        }
        let (one, two) = (
            std::fs::metadata(dest.join("twice.txt")).unwrap(),
            std::fs::metadata(dest.join("twice again.txt")).unwrap(),
        );
        assert_eq!(one.ino(), two.ino(), "its step kept its two names one file");
        for e in &entries {
            assert!(dest.join(e.file_name().unwrap()).exists());
        }
    }

    #[test]
    fn a_failed_transfer_leaves_the_copies_to_their_steps() {
        let tmp = home_tempdir();
        let entries = served(tmp.path());
        let dest = tmp.path().join("on the server");
        std::fs::create_dir(&dest).unwrap();
        crate::fs::reach::pretend_remote(&dest);
        let (executor, calls) = with_fake(true, None);
        let r = executor.execute_blocking(&copies(&entries, &dest), &CancelToken::new());
        assert!(r.error.is_none(), "{:?}", r.error);
        assert!(!calls.lock().is_empty());
        for e in &entries {
            same(e, &dest.join(e.file_name().unwrap()));
        }
        assert_eq!(names(&dest).len(), entries.len(), "nothing staged left");
    }

    #[test]
    fn a_batch_cancelled_cancels_the_plan_and_leaves_nothing() {
        let tmp = home_tempdir();
        let entries = served(tmp.path());
        let dest = tmp.path().join("on the server");
        std::fs::create_dir(&dest).unwrap();
        crate::fs::reach::pretend_remote(&dest);
        let cancel = CancelToken::new();
        let (executor, _) = with_fake(false, Some(cancel.clone()));
        let r = executor.execute_blocking(&copies(&entries, &dest), &cancel);
        assert!(r.cancelled);
        assert_eq!(r.applied, 0);
        assert!(
            names(&dest).is_empty(),
            "nothing placed, nothing staged left"
        );
    }

    #[test]
    fn a_batch_cancelled_once_it_placed_files_takes_them_back() {
        let tmp = home_tempdir();
        let entries = served(tmp.path());
        let dest = tmp.path().join("on the server");
        std::fs::create_dir(&dest).unwrap();
        crate::fs::reach::pretend_remote(&dest);
        let cancel = CancelToken::new();
        let (executor, calls) = with_fake_cancelling(false, None, Some(cancel.clone()));
        let r = executor.execute_blocking(&copies(&entries, &dest), &cancel);
        assert!(r.cancelled);
        assert_eq!(r.applied, 0);
        assert_eq!(
            calls.lock().len(),
            2,
            "it placed its files, then was cancelled"
        );
        assert!(names(&dest).is_empty(), "what it placed taken back");
    }
}
