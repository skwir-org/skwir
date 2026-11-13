//! What the files under a folder are to the repository (ADR-134), as
//! `git status` tells them: in conflict, changed (the file differs from
//! the index), staged (the index differs from `HEAD`), new (untracked),
//! ignored. Computed here rather than by gix's status, which brings its
//! filters and its command runner: the index is read, each file's stat
//! compared with what the index kept of it, and only a file whose stat
//! differs (or is racy, as git says: written in the second the index was)
//! is hashed, its bytes as they are, no filter run and no end of line
//! converted. Nothing is written: the index is not refreshed, no lock is
//! taken.
//!
//! Each path is said once, at its strongest; a folder new or ignored as a
//! whole is said as one path, never what it holds. What a folder holds
//! below is not summed here: the caller sums it from the paths.
use crate::{Deadline, Result};
use gix::ObjectId;
use gix::bstr::{BStr, BString, ByteSlice};
use gix::index::entry::{Flags, Mode, Stage, Stat};
use gix::objs::tree::EntryKind;
use serde::Serialize;
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{self, AtomicBool};

/// Entries looked at between two looks at the clock.
const CLOCK: usize = 256;
/// Index entries a thread looks at at least: fewer are looked at on one.
const PER_THREAD: usize = 4096;
/// Threads the files are stat-ed on at most.
const THREADS: usize = 8;

/// What a path is to the repository, the strongest first: a path both
/// staged and changed again is said changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum State {
    Conflict,
    Changed,
    Staged,
    New,
    Ignored,
}

#[derive(Serialize)]
pub(crate) struct Status {
    /// The folder itself, new or ignored as a whole: what it holds is too,
    /// and no path is said.
    folder: Option<State>,
    /// Each path that is not as `HEAD` has it, `/`-separated from the
    /// folder asked, by name: a file, or a folder new or ignored whole. A
    /// file gone from the disk or from the index is said too, though no
    /// listing holds it: the folders above it hold a change.
    states: Vec<(String, State)>,
    /// More paths were found than `limit`: those past it are not said.
    cut: bool,
    /// The deadline passed: what was found is said, the rest not looked at.
    stopped: bool,
}

/// How the repository's files are compared with what the index kept of
/// them, and what is told of what it does not track: as its configuration
/// says, else as the user's says (`user`, which skwir reads for the
/// helper, kept from it: `git::User`), else as git does by default. The
/// repository's word wins over the user's, as in git.
// Each flag is one of git's settings, named as git names it.
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct Settings {
    /// `core.fileMode`: an executable bit not as the index has it is a
    /// change. Off where the drive keeps no such bit (exFAT, NTFS), which
    /// says every file executable.
    file_mode: bool,
    /// `core.symlinks`: a link is checked out as one; off, as a file that
    /// holds its target.
    symlinks: bool,
    /// `core.ignoreCase`: a name on the disk is the index's in any case.
    ignore_case: bool,
    /// `status.showUntrackedFiles` other than `no`: what the index does
    /// not track is told, new or ignored. A repository of one's home files
    /// says `no`, which leaves the rest of the home folder unsaid.
    untracked: bool,
    /// `core.trustCTime` and `core.checkStat`: what of a stat says a file
    /// is as it was.
    stat: gix::index::entry::stat::Options,
}

impl Settings {
    pub(crate) fn of(repo: &gix::Repository, user: &BTreeMap<String, String>) -> Self {
        let config = repo.config_snapshot();
        let users = |key: &str| user.get(&key.to_ascii_lowercase());
        let flag = |key: &str, default: bool| {
            config
                .boolean(key)
                .or_else(|| users(key).and_then(|value| truth(value)))
                .unwrap_or(default)
        };
        let word = |key: &str| {
            config
                .string(key)
                .map(|value| value.to_str_lossy().to_ascii_lowercase())
                .or_else(|| users(key).map(|value| value.to_ascii_lowercase()))
        };
        Settings {
            file_mode: flag("core.fileMode", true),
            symlinks: flag("core.symlinks", true),
            ignore_case: flag("core.ignoreCase", false),
            untracked: !matches!(
                word("status.showUntrackedFiles").as_deref(),
                Some("no" | "false" | "off" | "0")
            ),
            stat: gix::index::entry::stat::Options {
                trust_ctime: flag("core.trustCTime", true),
                check_stat: word("core.checkStat").as_deref() != Some("minimal"),
                use_nsec: false,
                use_stdev: false,
            },
        }
    }
}

/// A boolean as git writes one.
fn truth(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => Some(true),
        "false" | "no" | "off" | "0" | "" => Some(false),
        _ => None,
    }
}

/// The index asked of a name found on the disk: as it is, or in any case
/// where the repository says its names are (`Settings::ignore_case`).
struct Names<'s> {
    state: &'s gix::index::State,
    icase: Option<gix::index::AccelerateLookup<'s>>,
}

impl<'s> Names<'s> {
    fn new(state: &'s gix::index::State, ignore_case: bool) -> Self {
        Names {
            state,
            icase: ignore_case.then(|| state.prepare_icase_backing()),
        }
    }

    /// Whether it tracks `path` itself: a file, or a submodule.
    fn tracks(&self, path: &BStr) -> bool {
        match &self.icase {
            Some(lookup) => self.state.entry_by_path_icase(path, true, lookup).is_some(),
            None => self.state.entry_index_by_path(path).is_ok(),
        }
    }

    /// Whether it tracks files below `path`.
    fn holds(&self, path: &BStr) -> bool {
        if let Some(lookup) = &self.icase {
            return self.state.path_is_directory_icase(path, true, lookup);
        }
        let mut inside = path.to_owned();
        inside.push(b'/');
        self.state
            .prefixed_entries_range(inside.as_bstr())
            .is_some()
    }
}

/// What was found, each path at its strongest.
struct Found {
    states: HashMap<BString, State>,
    limit: usize,
    cut: bool,
    stopped: bool,
    looked: usize,
}

impl Found {
    fn add(&mut self, path: &BStr, state: State) {
        if let Some(known) = self.states.get_mut(path) {
            *known = (*known).min(state);
            return;
        }
        if self.states.len() >= self.limit {
            self.cut = true;
            return;
        }
        self.states.insert(path.to_owned(), state);
    }

    /// Whether to stop: the deadline passed, looked at every `CLOCK`
    /// entries.
    fn late(&mut self, deadline: &Deadline) -> bool {
        self.looked += 1;
        if !self.stopped && self.looked.is_multiple_of(CLOCK) && deadline.passed() {
            self.stopped = true;
        }
        self.stopped
    }
}

/// The status of the files under `path` (the whole work tree for none) of
/// `repo`, checked out in `work_tree`, compared as `settings` says: `limit`
/// paths at most.
pub(crate) fn status(
    repo: &gix::Repository,
    work_tree: &Path,
    path: &[&[u8]],
    settings: &Settings,
    limit: usize,
    deadline: &Deadline,
) -> Result<Status> {
    let index = if repo.index_path().is_file() {
        repo.open_index()?
    } else {
        gix::index::File::from_state(
            gix::index::State::new(repo.object_hash()),
            repo.index_path(),
        )
    };
    let state: &gix::index::State = &index;
    let names = Names::new(state, settings.ignore_case);
    let prefix: BString = path.join(&b'/').into();
    let under = |prefix: &BStr| -> BString {
        let mut under = prefix.to_owned();
        if !under.is_empty() {
            under.push(b'/');
        }
        under
    };
    let mut excludes = repo.excludes(
        state,
        None,
        gix::worktree::stack::state::ignore::Source::WorktreeThenIdMappingIfNotSkipped,
    )?;
    // A folder above it ignored: what it holds that is not tracked is too.
    let mut above = false;
    for depth in 1..path.len() {
        let folder: BString = path[..depth].join(&b'/').into();
        if excluded(&mut excludes, folder.as_bstr(), true)? {
            above = true;
            break;
        }
    }
    let range = state.prefixed_entries_range(under(prefix.as_bstr()).as_bstr());
    let Some(range) = range else {
        // Nothing tracked under it: the folder is new or ignored whole.
        let ignored =
            above || (!prefix.is_empty() && excluded(&mut excludes, prefix.as_bstr(), true)?);
        // What is not tracked is not told: nothing is said of it.
        let folder = if prefix.is_empty() || !settings.untracked {
            None
        } else if ignored {
            Some(State::Ignored)
        } else {
            Some(State::New)
        };
        let mut found = Found {
            states: HashMap::new(),
            limit,
            cut: false,
            stopped: false,
            looked: 0,
        };
        if prefix.is_empty() && settings.untracked {
            // A repository with nothing tracked: its files are new.
            walk(
                &names,
                &mut excludes,
                work_tree,
                &prefix,
                false,
                &mut found,
                deadline,
            )?;
        }
        return Ok(answered(found, prefix.as_bstr(), folder));
    };
    let entries = &state.entries()[range];
    let mut found = Found {
        states: HashMap::new(),
        limit,
        cut: false,
        stopped: false,
        looked: 0,
    };
    // The index against `HEAD`: what is staged.
    let head_tree = match repo.head()?.id() {
        Some(id) => Some(repo.find_commit(id.detach())?.tree_id()?.detach()),
        None => None,
    };
    let head = match head_tree {
        Some(root) => crate::tree::entry_at(repo, root, path)?
            .filter(|entry| entry.is_tree())
            .map(|entry| entry.id),
        None => None,
    };
    let mut cached = state.tree();
    for component in path {
        cached = cached.and_then(|node| {
            node.children
                .iter()
                .find(|child| child.name.as_slice() == *component)
        });
    }
    staged(
        repo, state, &prefix, head, cached, entries, &mut found, deadline,
    )?;
    // The files against the index: what is changed, in conflict.
    changed(
        repo, state, work_tree, entries, settings, &mut found, deadline,
    )?;
    // What is on the disk and not in the index: new, or ignored.
    if settings.untracked {
        let folder_ignored =
            above || (!prefix.is_empty() && excluded(&mut excludes, prefix.as_bstr(), true)?);
        walk(
            &names,
            &mut excludes,
            work_tree,
            &prefix,
            folder_ignored,
            &mut found,
            deadline,
        )?;
    }
    Ok(answered(found, prefix.as_bstr(), None))
}

/// What was found, named from the folder asked, in order; a name not
/// written in UTF-8 is not said (no listing would be matched with it).
fn answered(found: Found, prefix: &BStr, folder: Option<State>) -> Status {
    let skip = if prefix.is_empty() {
        0
    } else {
        prefix.len() + 1
    };
    let mut states: Vec<(String, State)> = found
        .states
        .into_iter()
        .filter_map(|(path, state)| {
            let relative = path.get(skip..)?;
            Some((relative.to_str().ok()?.to_owned(), state))
        })
        .collect();
    states.sort();
    Status {
        folder,
        states,
        cut: found.cut,
        stopped: found.stopped,
    }
}

/// Whether `relative` (from the work tree's top) is ignored, a folder's
/// patterns matching it as one when `folder`.
fn excluded(excludes: &mut gix::AttributeStack<'_>, relative: &BStr, folder: bool) -> Result<bool> {
    let platform = excludes.at_entry(relative, folder.then_some(Mode::DIR))?;
    Ok(platform.is_excluded())
}

/// The kind of tree entry an index entry's mode is.
fn kind_of(mode: Mode) -> Option<EntryKind> {
    if mode == Mode::FILE {
        Some(EntryKind::Blob)
    } else if mode == Mode::FILE_EXECUTABLE {
        Some(EntryKind::BlobExecutable)
    } else if mode == Mode::SYMLINK {
        Some(EntryKind::Link)
    } else if mode == Mode::COMMIT {
        Some(EntryKind::Commit)
    } else {
        None
    }
}

/// One name in a folder of the index: a file (its entries, a conflict's
/// stages), or a folder (the entries under it).
enum Named {
    File(std::ops::Range<usize>),
    Folder(std::ops::Range<usize>),
}

/// The index's entries in `entries` (all under `dir`), by name in `dir`,
/// in the tree's order (by name, a folder's as if it ended in `/`), which
/// is the index's order of paths.
fn named<'s>(
    state: &'s gix::index::State,
    dir: &BStr,
    entries: &[gix::index::Entry],
) -> Vec<(&'s BStr, Named)> {
    let skip = if dir.is_empty() { 0 } else { dir.len() + 1 };
    let mut out: Vec<(&BStr, Named)> = Vec::new();
    let mut at = 0;
    while at < entries.len() {
        let path = entries[at].path(state);
        let rest = &path[skip.min(path.len())..];
        let mut end = at + 1;
        if let Some(slash) = rest.find_byte(b'/') {
            let name = &rest[..slash];
            let mut folder = path[..skip + slash].to_owned();
            folder.push(b'/');
            while end < entries.len() && entries[end].path(state).starts_with(folder.as_slice()) {
                end += 1;
            }
            out.push((name.as_bstr(), Named::Folder(at..end)));
        } else {
            while end < entries.len() && entries[end].path(state) == path {
                end += 1;
            }
            out.push((rest.as_bstr(), Named::File(at..end)));
        }
        at = end;
    }
    out
}

/// The order of tree entries: by name, a folder's as if it ended in `/`.
fn order(a: (&[u8], bool), b: (&[u8], bool)) -> Ordering {
    fn key((name, folder): (&[u8], bool)) -> impl Iterator<Item = u8> + '_ {
        name.iter().copied().chain(folder.then_some(b'/'))
    }
    key(a).cmp(key(b))
}

/// What is staged under `dir`: its index entries (`entries`) against its
/// tree in `HEAD` (`head`). A folder whose tree the index keeps whole
/// (its cache tree, valid) as `HEAD` has it holds nothing staged and is
/// not read, so the work is the folders that differ, not the files.
#[allow(clippy::too_many_arguments)]
fn staged(
    repo: &gix::Repository,
    state: &gix::index::State,
    dir: &BString,
    head: Option<ObjectId>,
    cached: Option<&gix::index::extension::Tree>,
    entries: &[gix::index::Entry],
    found: &mut Found,
    deadline: &Deadline,
) -> Result<()> {
    if let (Some(node), Some(head)) = (cached, head)
        && node.num_entries.is_some()
        && node.id == head
    {
        return Ok(());
    }
    if found.late(deadline) {
        return Ok(());
    }
    let tree = match head {
        Some(id) => crate::tree::entries(repo, id)?,
        None => Vec::new(),
    };
    let names = named(state, dir.as_bstr(), entries);
    let join = |name: &[u8]| -> BString {
        let mut path = dir.clone();
        if !path.is_empty() {
            path.push(b'/');
        }
        path.extend_from_slice(name);
        path
    };
    let (mut t, mut n) = (0, 0);
    while t < tree.len() || n < names.len() {
        if found.stopped {
            return Ok(());
        }
        let pick = match (tree.get(t), names.get(n)) {
            (Some((a, ea)), Some((b, nb))) => order(
                (a.as_slice(), ea.is_tree()),
                (b.as_bytes(), matches!(nb, Named::Folder(_))),
            ),
            (Some(_), None) => Ordering::Less,
            _ => Ordering::Greater,
        };
        match pick {
            // In `HEAD` alone: taken out of the index, a folder's files
            // each (`git rm -r --cached`), as git says them: the files on
            // the disk are its, not the folder's.
            Ordering::Less => {
                let (name, entry) = &tree[t];
                let path = join(name);
                if entry.is_tree() {
                    let mut gone = Vec::new();
                    if !crate::tree::changed(repo, &path, Some(*entry), None, deadline, &mut gone)?
                    {
                        found.stopped = true;
                    }
                    for change in gone {
                        found.add(change.path.as_bstr(), State::Staged);
                    }
                } else {
                    found.add(path.as_bstr(), State::Staged);
                }
                t += 1;
            }
            Ordering::Greater => {
                let (name, at) = &names[n];
                match at {
                    Named::Folder(range) => staged(
                        repo,
                        state,
                        &join(name),
                        None,
                        None,
                        &entries[range.clone()],
                        found,
                        deadline,
                    )?,
                    Named::File(range) => {
                        added(&join(name), &entries[range.clone()], found);
                    }
                }
                n += 1;
            }
            Ordering::Equal => {
                let (name, at) = &names[n];
                let tree_entry = tree[t].1;
                match at {
                    Named::Folder(range) => {
                        let child = cached.and_then(|node| {
                            node.children
                                .iter()
                                .find(|child| child.name.as_slice() == name.as_bytes())
                        });
                        staged(
                            repo,
                            state,
                            &join(name),
                            Some(tree_entry.id),
                            child,
                            &entries[range.clone()],
                            found,
                            deadline,
                        )?;
                    }
                    Named::File(range) => {
                        let path = join(name);
                        let stages = &entries[range.clone()];
                        if stages
                            .iter()
                            .any(|entry| entry.stage() != Stage::Unconflicted)
                        {
                            found.add(path.as_bstr(), State::Conflict);
                        } else if let Some(entry) = stages.first()
                            && (entry.id != tree_entry.id
                                || kind_of(entry.mode) != Some(tree_entry.mode.kind()))
                        {
                            found.add(path.as_bstr(), State::Staged);
                        }
                    }
                }
                t += 1;
                n += 1;
            }
        }
    }
    Ok(())
}

/// A file in the index that `HEAD` lacks: staged as added, in conflict
/// while merged, new where only its intent to be added is (`git add -N`).
fn added(path: &BString, stages: &[gix::index::Entry], found: &mut Found) {
    if stages
        .iter()
        .any(|entry| entry.stage() != Stage::Unconflicted)
    {
        found.add(path.as_bstr(), State::Conflict);
    } else if stages
        .first()
        .is_some_and(|entry| entry.flags.contains(Flags::INTENT_TO_ADD))
    {
        found.add(path.as_bstr(), State::New);
    } else {
        found.add(path.as_bstr(), State::Staged);
    }
}

/// What the files in the index are on the disk: changed, gone, in
/// conflict. A stat as the index kept it, not racy, says a file is as it
/// was; a size that differs says it changed; else its bytes are hashed.
/// A stat a file, on as many threads as the machine has (eight at most,
/// as git's `core.preloadIndex` does), each a share of the entries, a
/// path's entries never split.
fn changed(
    repo: &gix::Repository,
    state: &gix::index::State,
    work_tree: &Path,
    entries: &[gix::index::Entry],
    settings: &Settings,
    found: &mut Found,
    deadline: &Deadline,
) -> Result<()> {
    let kind = repo.object_hash();
    let threads = std::thread::available_parallelism()
        .map_or(1, std::num::NonZero::get)
        .min(THREADS)
        .min(entries.len().div_ceil(PER_THREAD))
        .max(1);
    let mut bounds = vec![0];
    for share in 1..threads {
        let mut at = entries.len() * share / threads;
        while at < entries.len() && entries[at].path(state) == entries[at - 1].path(state) {
            at += 1;
        }
        if at > *bounds.last().unwrap_or(&0) && at < entries.len() {
            bounds.push(at);
        }
    }
    bounds.push(entries.len());
    let stopped = AtomicBool::new(found.stopped);
    let shares: Vec<std::ops::Range<usize>> = bounds.windows(2).map(|w| w[0]..w[1]).collect();
    let said: Vec<Result<Vec<(BString, State)>>> = if shares.len() == 1 {
        vec![looked_at(
            kind, state, work_tree, entries, settings, &stopped, deadline,
        )]
    } else {
        std::thread::scope(|scope| {
            let looking: Vec<_> = shares
                .iter()
                .map(|share| {
                    let entries = &entries[share.clone()];
                    let stopped = &stopped;
                    scope.spawn(move || {
                        looked_at(kind, state, work_tree, entries, settings, stopped, deadline)
                    })
                })
                .collect();
            looking
                .into_iter()
                .map(|thread| {
                    thread
                        .join()
                        .unwrap_or_else(|_| Err("a thread looking at files failed".into()))
                })
                .collect()
        })
    };
    for share in said {
        for (path, state) in share? {
            found.add(path.as_bstr(), state);
        }
    }
    if stopped.load(atomic::Ordering::Acquire) {
        found.stopped = true;
    }
    Ok(())
}

/// `changed`, for a share of the index's entries: what each path is, where
/// it is not as the index has it. A file that cannot be read is not said.
#[allow(clippy::too_many_arguments)]
fn looked_at(
    kind: gix::hash::Kind,
    state: &gix::index::State,
    work_tree: &Path,
    entries: &[gix::index::Entry],
    settings: &Settings,
    stopped: &AtomicBool,
    deadline: &Deadline,
) -> Result<Vec<(BString, State)>> {
    let options = settings.stat;
    let written = state.timestamp();
    let mut said = Vec::new();
    let mut at = 0;
    let mut looked = 0usize;
    while at < entries.len() {
        looked += 1;
        if looked.is_multiple_of(CLOCK)
            && (stopped.load(atomic::Ordering::Acquire) || deadline.passed())
        {
            stopped.store(true, atomic::Ordering::Release);
            break;
        }
        let entry = &entries[at];
        let path = entry.path(state);
        let mut end = at + 1;
        while end < entries.len() && entries[end].path(state) == path {
            end += 1;
        }
        let conflict = entries[at..end]
            .iter()
            .any(|entry| entry.stage() != Stage::Unconflicted);
        at = end;
        if conflict {
            said.push((path.to_owned(), State::Conflict));
            continue;
        }
        // Kept out of the work tree, or said unchanged by whoever added it:
        // as git takes them. A submodule says its own state.
        if entry
            .flags
            .intersects(Flags::SKIP_WORKTREE | Flags::ASSUME_VALID)
            || entry.flags.contains(Flags::INTENT_TO_ADD)
            || entry.mode == Mode::COMMIT
        {
            continue;
        }
        let Ok(relative) = gix::path::from_bstr(path) else {
            continue;
        };
        let full = work_tree.join(relative);
        let meta = match gix::index::fs::Metadata::from_path_no_follow(&full) {
            Ok(meta) => meta,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) =>
            {
                said.push((path.to_owned(), State::Changed));
                continue;
            }
            Err(_) => continue,
        };
        // A link checked out as a file holding its target where the
        // repository keeps no links (`core.symlinks`): compared as a file.
        let link = entry.mode == Mode::SYMLINK && settings.symlinks;
        if (link && !meta.is_symlink()) || (!link && !meta.is_file()) {
            said.push((path.to_owned(), State::Changed));
            continue;
        }
        if entry.mode != Mode::SYMLINK
            && settings.file_mode
            && executable_differs(entry.mode, &meta)
        {
            said.push((path.to_owned(), State::Changed));
            continue;
        }
        let Ok(now) = Stat::from_fs(&meta) else {
            continue;
        };
        if now.size != entry.stat.size {
            said.push((path.to_owned(), State::Changed));
            continue;
        }
        if entry.stat.matches(&now, options) && !entry.stat.is_racy(written, options) {
            continue;
        }
        let id = if link {
            let Ok(target) = std::fs::read_link(&full) else {
                continue;
            };
            let target = gix::path::os_str_into_bstr(target.as_os_str())?;
            hashed(kind, &mut target.as_bytes(), target.len() as u64)?
        } else {
            let Ok(mut file) = std::fs::File::open(&full) else {
                continue;
            };
            hashed(kind, &mut file, meta.len())?
        };
        if id != entry.id {
            said.push((path.to_owned(), State::Changed));
        }
    }
    Ok(said)
}

/// Whether the file's executable bit is not as the index has it, where
/// the system keeps one (git's `core.fileMode`, on by default there).
#[cfg(unix)]
fn executable_differs(mode: Mode, meta: &gix::index::fs::Metadata) -> bool {
    (mode == Mode::FILE_EXECUTABLE) != meta.is_executable()
}

#[cfg(not(unix))]
fn executable_differs(_mode: Mode, _meta: &gix::index::fs::Metadata) -> bool {
    false
}

/// The id `bytes` (`size` of them) have as a blob.
fn hashed(kind: gix::hash::Kind, bytes: &mut dyn Read, size: u64) -> Result<ObjectId> {
    let mut hasher = gix::hash::hasher(kind);
    hasher.update(&gix::objs::encode::loose_header(
        gix::objs::Kind::Blob,
        size,
    ));
    let mut buffer = vec![0u8; 64 * 1024];
    let mut read = 0u64;
    loop {
        let count = bytes.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        read += count as u64;
        hasher.update(&buffer[..count]);
    }
    if read != size {
        // Written while read: as changed as can be.
        return Ok(ObjectId::null(kind));
    }
    Ok(hasher.try_finalize()?)
}

/// What `dir` (from the work tree's top) holds that the index does not:
/// new, or ignored (`ignored`: a folder above it is). A folder the index
/// tracks files in is walked; one it does not is said whole, new where it
/// holds a file not ignored, ignored where all it holds is, nothing where
/// it holds nothing, as git says it. A repository's `.git` is never said.
fn walk(
    names: &Names<'_>,
    excludes: &mut gix::AttributeStack<'_>,
    work_tree: &Path,
    dir: &BString,
    ignored: bool,
    found: &mut Found,
    deadline: &Deadline,
) -> Result<()> {
    let Ok(read) = std::fs::read_dir(work_tree.join(gix::path::from_bstr(dir.as_bstr())?)) else {
        return Ok(());
    };
    for item in read {
        if found.late(deadline) {
            return Ok(());
        }
        let Ok(item) = item else {
            continue;
        };
        let name = item.file_name();
        let Ok(name) = gix::path::os_str_into_bstr(&name) else {
            continue;
        };
        if name == ".git" {
            continue;
        }
        let mut path = dir.clone();
        if !path.is_empty() {
            path.push(b'/');
        }
        path.extend_from_slice(name);
        let folder = item.file_type().is_ok_and(|kind| kind.is_dir());
        // A file the index tracks, or a submodule checked out there (its
        // own repository says what it holds).
        if names.tracks(path.as_bstr()) {
            continue;
        }
        if folder && names.holds(path.as_bstr()) {
            let below = ignored || excluded(excludes, path.as_bstr(), true)?;
            walk(names, excludes, work_tree, &path, below, found, deadline)?;
            continue;
        }
        if ignored || excluded(excludes, path.as_bstr(), folder)? {
            found.add(path.as_bstr(), State::Ignored);
        } else if !folder {
            found.add(path.as_bstr(), State::New);
        } else if let Some(held) = holds(excludes, work_tree, &path, found, deadline)? {
            found.add(path.as_bstr(), held);
        }
    }
    Ok(())
}

/// What an untracked folder holds, as git says the folder: new at its
/// first file not ignored, ignored when all it holds is, `None` for
/// nothing at all. A repository inside it (its `.git`) is something new.
fn holds(
    excludes: &mut gix::AttributeStack<'_>,
    work_tree: &Path,
    dir: &BString,
    found: &mut Found,
    deadline: &Deadline,
) -> Result<Option<State>> {
    let Ok(read) = std::fs::read_dir(work_tree.join(gix::path::from_bstr(dir.as_bstr())?)) else {
        return Ok(None);
    };
    let mut ignored = false;
    for item in read {
        if found.late(deadline) {
            return Ok(None);
        }
        let Ok(item) = item else {
            continue;
        };
        let name = item.file_name();
        let Ok(name) = gix::path::os_str_into_bstr(&name) else {
            continue;
        };
        if name == ".git" {
            return Ok(Some(State::New));
        }
        let mut path = dir.clone();
        path.push(b'/');
        path.extend_from_slice(name);
        let folder = item.file_type().is_ok_and(|kind| kind.is_dir());
        if excluded(excludes, path.as_bstr(), folder)? {
            ignored = true;
            continue;
        }
        if !folder {
            return Ok(Some(State::New));
        }
        match holds(excludes, work_tree, &path, found, deadline)? {
            Some(State::New) => return Ok(Some(State::New)),
            Some(_) => ignored = true,
            None => {}
        }
    }
    Ok(ignored.then_some(State::Ignored))
}
