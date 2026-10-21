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
//! the batch did not take or finish is made by its step.

use std::collections::{BTreeMap, HashMap};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use anyhow::Context;
use parking_lot::{Mutex, RwLock};

use super::{Cancelled, Copy, Operation, StepContext};
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

    /// Copy the entries `names` of the folder `from` into the folder
    /// `into`, under the same names, which `into` does not hold: folders
    /// whole, links as links, files' and folders' modes and modification
    /// times kept. `copied` is told the bytes copied so far, in all.
    /// Blocking, on the plan's thread; stopped when `cancel` is.
    ///
    /// # Errors
    /// When the copy could not be made whole, or was cancelled: what it
    /// made is removed with the staging folder.
    fn copy(
        &self,
        from: &Path,
        names: &[OsString],
        into: &Path,
        copied: &(dyn Fn(u64) + Sync),
        cancel: &CancelToken,
    ) -> anyhow::Result<()>;
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

    /// The names of those registered, for a test of plugins withdrawn.
    pub fn names(&self) -> Vec<&'static str> {
        self.list.read().iter().map(|(_, t)| t.name()).collect()
    }

    fn serving(&self, from: &Path, into: &Path) -> Option<Arc<dyn Transfer>> {
        self.list
            .read()
            .iter()
            .find(|(_, t)| t.serves(from, into))
            .map(|(_, t)| Arc::clone(t))
    }
}

/// What a batch did for a copy's source, which its step takes
/// (`StepContext::take_batched`).
#[derive(Debug, Clone)]
pub(super) enum Batched {
    /// Its copy, whole, in the staging folder.
    Made(PathBuf),
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
    let Some(first) = steps.get(i).and_then(|s| copy_of(s.as_ref())) else {
        return Ok(());
    };
    let dest = first.dest_dir.clone();
    let run: Vec<&Copy> = steps[i..]
        .iter()
        .map_while(|s| copy_of(s.as_ref()).filter(|c| c.dest_dir == dest))
        .collect();
    ctx.batched_up_to(i + run.len());
    // A drive written through keeps its own pace (`pace`).
    if crate::fs::volume::is_removable(&dest) {
        return Ok(());
    }
    let dest_remote = crate::fs::reach::of(&dest).is_some();
    let mut groups: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();
    for copy in run {
        // One that does not resolve says why in its own check.
        if let Ok(src) = copy.entry.resolve()
            && let Some(parent) = src.parent()
        {
            groups.entry(parent.to_path_buf()).or_default().push(src);
        }
    }
    for (from, sources) in groups {
        if !dest_remote && crate::fs::reach::of(&from).is_none() {
            continue;
        }
        let taken = takes(&from, &sources, ctx);
        // One file alone: as many round trips in a batch as by its step.
        let worth = taken.len() > 1 || taken.iter().any(|t| t.folder);
        if taken.is_empty() || !worth {
            continue;
        }
        batch(&from, &dest, &taken, ctx)?;
    }
    Ok(())
}

/// The step as a copy a batch may make: one that keeps attributes.
fn copy_of(step: &dyn Operation) -> Option<&Copy> {
    step.as_any()?
        .downcast_ref::<Copy>()
        .filter(|c| c.preserve_attributes)
}

/// An entry a batch takes: its source, its name, its bytes.
struct Taken {
    src: PathBuf,
    name: OsString,
    bytes: u64,
    folder: bool,
}

/// What of `sources` (entries of the folder `from`) a batch takes
/// (ADR-078, 2): a name no other of them has, no companion beside it, a
/// tree of files, folders and links alone. Reads the folder's names once
/// and walks each tree.
fn takes(from: &Path, sources: &[PathBuf], ctx: &StepContext) -> Vec<Taken> {
    let rules = ctx.companions();
    let Ok(listed) = std::fs::read_dir(from) else {
        return Vec::new();
    };
    let listed: Vec<OsString> = listed.flatten().map(|e| e.file_name()).collect();
    let mut names: HashMap<&OsStr, usize> = HashMap::new();
    for src in sources {
        if let Some(name) = src.file_name() {
            *names.entry(name).or_default() += 1;
        }
    }
    let mut taken = Vec::new();
    for src in sources {
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
        if let Some(bytes) = plain_tree(src, rules, ctx.cancel_token()) {
            taken.push(Taken {
                src: src.clone(),
                name: name.to_os_string(),
                bytes,
                folder,
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

/// Make the copies `taken` of the folder `from` at once, in a folder of
/// the plan's staging folder in `dest`, and tell their steps.
fn batch(from: &Path, dest: &Path, taken: &[Taken], ctx: &StepContext) -> Result<(), Cancelled> {
    #[cfg(test)]
    BATCHES.fetch_add(1, Ordering::Relaxed);
    let total: u64 = taken.iter().map(|t| t.bytes).sum();
    ctx.add_total(total);
    for t in taken {
        ctx.batched(&t.src, Batched::Counted);
    }
    let names: Vec<OsString> = taken.iter().map(|t| t.name.clone()).collect();
    let made = (|| {
        let (staging, name) = ctx.staging_for(dest)?;
        let into = staging.path().join(format!("batch-{name}"));
        std::fs::create_dir(&into)?;
        let advance = ctx.progress();
        let told = AtomicU64::new(0);
        let copied = |so_far: u64| {
            let before = told.fetch_max(so_far, Ordering::Relaxed);
            if so_far > before {
                advance(so_far - before);
            }
        };
        let cancel = ctx.cancel_token();
        let by = ctx.transfers().and_then(|t| t.serving(from, &into));
        let doer = by.as_ref().map_or("skwir", |t| t.name());
        tracing::debug!(copies = taken.len(), total, by = doer, "a batch of copies");
        match by {
            Some(transfer) => transfer.copy(from, &names, &into, &copied, cancel),
            None => copy_at_once(from, &names, &into, &copied, cancel),
        }
        .with_context(|| format!("{doer} copying {} entries", names.len()))?;
        anyhow::Ok(into)
    })();
    match made {
        Ok(into) => {
            for t in taken {
                ctx.batched(&t.src, Batched::Made(into.join(&t.name)));
            }
            Ok(())
        }
        Err(_) if ctx.cancel_token().is_cancelled() => Err(Cancelled),
        // Each step copies its own, as without a batch.
        Err(error) => {
            tracing::warn!("a batch of copies failed, copied one by one: {error:#}");
            Ok(())
        }
    }
}

/// Skwir's own batch copy (the [`Transfer::copy`] it does where no
/// plugin's serves): the trees' folders made first, in order, their files
/// [`AT_ONCE`] at a time, each given its mode where it was made otherwise
/// and its time, then the folders theirs, deepest first.
///
/// # Errors
/// The first copy that failed, or the cancel.
pub(super) fn copy_at_once(
    from: &Path,
    names: &[OsString],
    into: &Path,
    copied: &(dyn Fn(u64) + Sync),
    cancel: &CancelToken,
) -> anyhow::Result<()> {
    let (mut folders, mut files) = (Vec::new(), Vec::new());
    for name in names {
        lay_out(&from.join(name), &into.join(name), &mut folders, &mut files)?;
    }
    let next = AtomicUsize::new(0);
    let total = AtomicU64::new(0);
    let failed: Mutex<Option<anyhow::Error>> = Mutex::new(None);
    let stop = AtomicBool::new(false);
    std::thread::scope(|s| {
        for _ in 0..AT_ONCE.min(files.len()) {
            s.spawn(|| {
                while !stop.load(Ordering::Relaxed) && !cancel.is_cancelled() {
                    let Some((src, dst)) = files.get(next.fetch_add(1, Ordering::Relaxed)) else {
                        return;
                    };
                    let wrote = |n: u64| copied(total.fetch_add(n, Ordering::Relaxed) + n);
                    if let Err(error) = copy_one(src, dst, &wrote) {
                        stop.store(true, Ordering::Relaxed);
                        failed.lock().get_or_insert(error);
                    }
                }
            });
        }
    });
    if let Some(error) = failed.into_inner() {
        return Err(error);
    }
    anyhow::ensure!(!cancel.is_cancelled(), "cancelled");
    for (src, dst) in folders.iter().rev() {
        let meta = std::fs::symlink_metadata(src)?;
        if let Ok(modified) = meta.modified()
            && let Ok(folder) = super::sys::open_to_set_times(dst)
        {
            let _ = folder.set_modified(modified);
        }
        std::fs::set_permissions(dst, meta.permissions())?;
    }
    Ok(())
}

/// Make the folders of the tree at `src` under `dst` now, and list its
/// files and links to copy.
fn lay_out(
    src: &Path,
    dst: &Path,
    folders: &mut Vec<(PathBuf, PathBuf)>,
    files: &mut Vec<(PathBuf, PathBuf)>,
) -> anyhow::Result<()> {
    let meta = std::fs::symlink_metadata(src)?;
    if meta.is_dir() {
        std::fs::create_dir(dst)?;
        folders.push((src.to_path_buf(), dst.to_path_buf()));
        for entry in std::fs::read_dir(src)? {
            let name = entry?.file_name();
            lay_out(&src.join(&name), &dst.join(&name), folders, files)?;
        }
    } else {
        files.push((src.to_path_buf(), dst.to_path_buf()));
    }
    Ok(())
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
        df.set_permissions(meta.permissions())?;
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
    }

    impl Transfer for Fake {
        fn name(&self) -> &'static str {
            "fake"
        }
        fn serves(&self, _: &Path, _: &Path) -> bool {
            true
        }
        fn copy(
            &self,
            from: &Path,
            names: &[OsString],
            into: &Path,
            copied: &(dyn Fn(u64) + Sync),
            cancel: &CancelToken,
        ) -> anyhow::Result<()> {
            self.calls.lock().push(names.to_vec());
            if let Some(cancel) = &self.cancels {
                cancel.cancel();
                anyhow::bail!("cancelled");
            }
            copy_at_once(from, &names[..names.len() / 2], into, copied, cancel)?;
            anyhow::ensure!(!self.fails, "the server went away");
            copy_at_once(from, &names[names.len() / 2..], into, copied, cancel)
        }
    }

    fn with_fake(
        fails: bool,
        cancels: Option<CancelToken>,
    ) -> (Arc<Executor>, Arc<Mutex<Vec<Vec<OsString>>>>) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let transfers = Arc::new(Transfers::default());
        transfers.register(
            "test",
            Arc::new(Fake {
                calls: Arc::clone(&calls),
                fails,
                cancels,
            }),
        );
        let executor = Executor::new(Scheduler::new(2));
        executor.set_transfers(transfers);
        (executor, calls)
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
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].len(), entries.len());
        for e in &entries {
            same(e, &dest.join(e.file_name().unwrap()));
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
        let taken = calls.lock()[0].clone();
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
        assert_eq!(calls.lock().len(), 1);
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
}
