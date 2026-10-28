//! Operations, plans, and the executor.
//!
//! An operation is a mutation with an inverse. A plan is an ordered list of
//! operations referencing entries by stable identity. Nothing mutates the
//! filesystem except the executor applying a plan, which journals the
//! inverse so that everything is undoable. Applying a plan twice is a no-op:
//! each step checks whether it is already done before acting.

pub mod access;
mod builtin;
pub mod clash;
mod context;
mod entry_ref;
mod executor;
mod order;
mod pace;
#[cfg(target_os = "linux")]
mod put_trash;
// Unix reads at offsets without moving a file's own (`read_at`).
pub mod footprint;
#[cfg(unix)]
mod ranges;
pub mod records;
pub mod room;
pub mod staging;
mod sys;
pub mod transfer;
#[cfg(target_os = "linux")]
mod trash_list;

use std::ffi::OsStr;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

pub use builtin::{
    Copy, CopyUndo, Delete, InTrash, Landed, Link, MadeBeside, Mkdir, Move, NewFile,
    RemoveEmptyFile, Rename, Restore, RestorePart, ReturnPart, Rmdir, Swap, Trash, Uncopy, Unlink,
    Unmerge, Unmove, Untrash, Untrashed, in_trash, keeps_no_trash, made_state, made_stays,
    parse_trash_info, register_builtin, remove_made, trash_info_of,
};
pub use context::{Cancelled, StepContext, Unanswered};
pub use entry_ref::{EntryArg, EntryRef, ResolveError};
/// The plans, undos and redos that run at once at most (ADR-079).
pub use executor::AT_ONCE as ACTIONS_AT_ONCE;
pub use executor::{
    ChangeObserver, ExecEvent, ExecResult, Executor, JournalEntry, PlanObserver, Progress, RunKind,
    SecretAsker, Transfer, Verdict, Waiting, WaitsFor,
};
pub use order::{RenameOrderError, RenameRequest, order_renames};
pub use staging::Staging;
/// The trash folders of the user's on this machine's own volumes, which
/// the trash place lists: no network mount's (`trash_list`).
#[cfg(target_os = "linux")]
pub use trash_list::{folders_here as trash_folders_here, items_here as trash_items_here};

/// Every step checked as applying the plan would meet it: in order, in one
/// context, a folder an earlier step makes counted there by the steps after
/// it (a plan that makes a folder and copies into it is no conflict), with
/// the rules of files beside a file the executor applies it with: a
/// sidecar's place taken is a conflict as its file's is, found here
/// rather than halfway through the plan.
pub(crate) fn dry_run<'a>(
    steps: &'a [Box<dyn Operation>],
    companions: &Arc<crate::companions::Companions>,
) -> impl Iterator<Item = (Change, StepStatus)> + 'a {
    dry_run_in(
        steps,
        StepContext::detached().with_companions(Arc::clone(companions)),
    )
}

/// [`dry_run`] in `ctx`: a step whose check walks a tree (a change of
/// access with what a folder holds) stops when its plan is cancelled.
pub(crate) fn dry_run_in(
    steps: &[Box<dyn Operation>],
    ctx: StepContext,
) -> impl Iterator<Item = (Change, StepStatus)> + '_ {
    steps.iter().map(move |step| {
        let change = step.describe();
        let status = guarded(|| Ok(step.check(&ctx)))
            .unwrap_or_else(|e| StepStatus::Conflict { reason: said(&e) });
        if matches!(status, StepStatus::Pending { .. }) {
            ctx.foresee(&change);
        }
        (change, status)
    })
}

/// `step` run so that a panic in it is an error of its own, never an unwind
/// through the executor.
pub(crate) fn guarded<T>(step: impl FnOnce() -> anyhow::Result<T>) -> anyhow::Result<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(step)).unwrap_or_else(|panic| {
        let what = panic
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| panic.downcast_ref::<String>().cloned())
            .unwrap_or_default();
        tracing::error!("a step panicked: {what}");
        Err(anyhow::anyhow!("it failed unexpectedly"))
    })
}

/// What a step's failure says to the user: its contexts, outermost first,
/// each written in the user's words where it is raised, then the system's
/// reason in words (`io::ErrorKind`), or its message without the error's
/// number. The whole chain goes to the log.
pub fn said(error: &anyhow::Error) -> String {
    error
        .chain()
        .map(|cause| match cause.downcast_ref::<std::io::Error>() {
            Some(io) => io_words(io),
            None => cause.to_string(),
        })
        .collect::<Vec<_>>()
        .join(": ")
}

/// A system's error in words: by its kind where the kind says it (the
/// same words on every system), else its own message, its number left out.
pub fn io_words(error: &std::io::Error) -> String {
    use std::io::ErrorKind as K;
    let words = match error.kind() {
        K::PermissionDenied => "permission denied",
        K::NotFound => "it is not there any more",
        K::AlreadyExists => "something of that name is already there",
        K::StorageFull => "the drive is full",
        K::QuotaExceeded => "the disk quota is used up",
        K::FileTooLarge => "the file is too large for that drive",
        K::ReadOnlyFilesystem => "the drive is read-only",
        K::InvalidFilename => "the name is too long, or not allowed there",
        K::DirectoryNotEmpty => "the folder is not empty",
        K::ResourceBusy | K::ExecutableFileBusy => "it is in use",
        K::NotADirectory => "a part of its path is not a folder",
        K::IsADirectory => "it is a folder",
        K::StaleNetworkFileHandle => "the network share went away",
        K::TimedOut => "the drive did not answer in time",
        _ => {
            let text = error.to_string();
            return match text.rfind(" (os error ") {
                Some(at) if text.ends_with(')') => text[..at].to_owned(),
                _ => text,
            };
        }
    };
    words.to_owned()
}

/// Whether `name` can name an entry inside a directory: not empty, not `.`
/// or `..`, and free of `/` and NUL; on Windows also what Windows refuses
/// (`windows_name_problem`). Every operation that takes a name from a
/// command (rename, copy under a new name, a new folder) checks it here,
/// so no surface can reach outside the directory it meant.
///
/// # Errors
/// Why the name is not usable, in the user's words.
pub fn valid_file_name(name: &OsStr) -> anyhow::Result<()> {
    let bytes = name.as_encoded_bytes();
    if bytes.is_empty() {
        anyhow::bail!("a name cannot be empty");
    }
    if bytes == b"." || bytes == b".." {
        anyhow::bail!("“{}” cannot be used as a name", name.display());
    }
    if bytes.contains(&b'/') {
        anyhow::bail!("a name cannot contain “/”");
    }
    if bytes.contains(&0) {
        anyhow::bail!("a name cannot contain a NUL character");
    }
    #[cfg(windows)]
    if let Some(problem) = windows_name_problem(&name.to_string_lossy()) {
        anyhow::bail!(problem);
    }
    Ok(())
}

/// What Windows refuses in a name, in the user's words: `\ : * ? " < >
/// |` and control characters, a name ending in a dot or a space, and the
/// names of devices (`CON`, `NUL`, `COM1`, `LPT¹`…), with an extension
/// or without. `None` for a name Windows takes. Tested everywhere, asked
/// on Windows.
#[cfg(any(windows, test))]
pub(crate) fn windows_name_problem(name: &str) -> Option<String> {
    if let Some(c) = name
        .chars()
        .find(|c| matches!(c, '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
    {
        return Some(format!("a name cannot contain “{c}”"));
    }
    if name.chars().any(char::is_control) {
        return Some("a name cannot contain a control character".into());
    }
    if name.ends_with(['.', ' ']) {
        return Some("a name cannot end with a dot or a space".into());
    }
    // A device is its name before the first dot, spaces at its end aside.
    let stem = name.split('.').next().unwrap_or(name).trim_end_matches(' ');
    let device = ["CON", "PRN", "AUX", "NUL"]
        .iter()
        .any(|d| stem.eq_ignore_ascii_case(d))
        || ["COM", "LPT"].iter().any(|d| {
            stem.get(..3).is_some_and(|s| s.eq_ignore_ascii_case(d))
                && matches!(
                    stem.get(3..),
                    Some("1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³")
                )
        });
    device.then(|| format!("“{stem}” is a device's name on Windows"))
}

/// What a step will do, for the diff shown before apply.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", tag = "change")]
pub enum Change {
    Rename {
        #[serde(with = "crate::os_path")]
        #[schemars(with = "crate::os_path::PathArg")]
        from: PathBuf,
        #[serde(with = "crate::os_path")]
        #[schemars(with = "crate::os_path::PathArg")]
        to: PathBuf,
    },
    Move {
        #[serde(with = "crate::os_path")]
        #[schemars(with = "crate::os_path::PathArg")]
        from: PathBuf,
        #[serde(with = "crate::os_path")]
        #[schemars(with = "crate::os_path::PathArg")]
        to: PathBuf,
    },
    Copy {
        #[serde(with = "crate::os_path")]
        #[schemars(with = "crate::os_path::PathArg")]
        from: PathBuf,
        #[serde(with = "crate::os_path")]
        #[schemars(with = "crate::os_path::PathArg")]
        to: PathBuf,
    },
    /// A link made at `to` that leads to `from`.
    Link {
        #[serde(with = "crate::os_path")]
        #[schemars(with = "crate::os_path::PathArg")]
        from: PathBuf,
        #[serde(with = "crate::os_path")]
        #[schemars(with = "crate::os_path::PathArg")]
        to: PathBuf,
    },
    Swap {
        #[serde(with = "crate::os_path")]
        #[schemars(with = "crate::os_path::PathArg")]
        a: PathBuf,
        #[serde(with = "crate::os_path")]
        #[schemars(with = "crate::os_path::PathArg")]
        b: PathBuf,
    },
    Trash {
        #[serde(with = "crate::os_path")]
        #[schemars(with = "crate::os_path::PathArg")]
        path: PathBuf,
    },
    Restore {
        #[serde(with = "crate::os_path")]
        #[schemars(with = "crate::os_path::PathArg")]
        path: PathBuf,
    },
    Delete {
        #[serde(with = "crate::os_path")]
        #[schemars(with = "crate::os_path::PathArg")]
        path: PathBuf,
    },
    Create {
        #[serde(with = "crate::os_path")]
        #[schemars(with = "crate::os_path::PathArg")]
        path: PathBuf,
        directory: bool,
        /// What the step does, in the user's words, when making it is not
        /// all it says (an archive extracted as this folder).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        words: Option<Words>,
    },
    Remove {
        #[serde(with = "crate::os_path")]
        #[schemars(with = "crate::os_path::PathArg")]
        path: PathBuf,
    },
    /// A metadata-only change (tags, attributes), naming the field.
    Metadata {
        #[serde(with = "crate::os_path")]
        #[schemars(with = "crate::os_path::PathArg")]
        path: PathBuf,
        field: String,
        /// What the step does, in the user's words, for the review and the
        /// toast; without it, the field's name is all they can say.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        words: Option<Words>,
    },
}

/// A step in the user's words: what it will do, as the review lists it
/// ("place “a.jpg” at 10.12000, 20.12000"), and what it did, as the toast
/// says it ("Placed “a.jpg” at 10.12000, 20.12000").
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Words {
    pub will: String,
    pub did: String,
}

impl fmt::Display for Change {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.said(f, None)
    }
}

/// A [`Change`] said as it says itself, its paths under a folder written
/// from there ([`Change::within`]).
pub struct Within<'a> {
    change: &'a Change,
    folder: &'a Path,
}

impl fmt::Display for Within<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.change.said(f, Some(self.folder))
    }
}

/// `path` as a change says it: from `folder` when it is inside it, whole
/// otherwise.
fn shown<'a>(path: &'a Path, folder: Option<&Path>) -> std::path::Display<'a> {
    folder
        .and_then(|folder| path.strip_prefix(folder).ok())
        .filter(|rest| !rest.as_os_str().is_empty())
        .unwrap_or(path)
        .display()
}

impl Change {
    /// Said as it says itself, each path under `folder` written from
    /// there: a review that says the folder once lists `rename a.txt ->
    /// b.txt`, not the folder on every line.
    pub fn within<'a>(&'a self, folder: &'a Path) -> Within<'a> {
        Within {
            change: self,
            folder,
        }
    }

    /// Every path it says, in the order it says them.
    pub fn paths(&self) -> impl Iterator<Item = &Path> {
        let (first, second) = match self {
            Change::Rename { from, to }
            | Change::Move { from, to }
            | Change::Copy { from, to }
            | Change::Link { from, to } => (from, Some(to)),
            Change::Swap { a, b } => (a, Some(b)),
            Change::Trash { path }
            | Change::Restore { path }
            | Change::Delete { path }
            | Change::Create { path, .. }
            | Change::Remove { path }
            | Change::Metadata { path, .. } => (path, None),
        };
        std::iter::once(first.as_path()).chain(second.map(PathBuf::as_path))
    }

    fn said(&self, f: &mut fmt::Formatter<'_>, folder: Option<&Path>) -> fmt::Result {
        let p = |path| shown(path, folder);
        match self {
            Change::Rename { from, to } => {
                write!(f, "rename {} -> {}", p(from), p(to))
            }
            Change::Move { from, to } => write!(f, "move {} -> {}", p(from), p(to)),
            Change::Copy { from, to } => write!(f, "copy {} -> {}", p(from), p(to)),
            // As `ls -l` shows a link: where it is, then where it leads.
            Change::Link { from, to } => write!(f, "link {} -> {}", p(to), p(from)),
            Change::Swap { a, b } => write!(f, "swap {} <-> {}", p(a), p(b)),
            Change::Trash { path } => write!(f, "trash {}", p(path)),
            Change::Restore { path } => write!(f, "restore {}", p(path)),
            Change::Delete { path } => write!(f, "delete {}", p(path)),
            // A step that says what it does says it best.
            Change::Create {
                words: Some(words), ..
            }
            | Change::Metadata {
                words: Some(words), ..
            } => f.write_str(&words.will),
            Change::Create {
                path,
                directory: true,
                ..
            } => write!(f, "mkdir {}", p(path)),
            Change::Create {
                path,
                directory: false,
                ..
            } => write!(f, "create {}", p(path)),
            // An empty folder, or a file made empty and left so.
            Change::Remove { path } => write!(f, "remove {}", p(path)),
            Change::Metadata { path, field, .. } => {
                write!(f, "set {field} on {}", p(path))
            }
        }
    }
}

/// Where a path is after a [`Change`] that holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Followed {
    /// The change did not touch it.
    Same,
    /// It, or a folder above it, was renamed, moved or swapped.
    Moved(PathBuf),
    /// It, or a folder above it, was trashed or deleted.
    Gone,
}

impl Change {
    /// The file it acts on, as it was asked: what is copied, moved, linked,
    /// renamed, trashed, restored (where it goes back), deleted or made.
    pub fn subject(&self) -> Option<&Path> {
        match self {
            Change::Copy { from, .. }
            | Change::Move { from, .. }
            | Change::Link { from, .. }
            | Change::Rename { from, .. } => Some(from),
            Change::Trash { path }
            | Change::Restore { path }
            | Change::Delete { path }
            | Change::Create { path, .. } => Some(path),
            _ => None,
        }
    }

    /// Where it puts something that was not there: what a copy, a move or
    /// a link places, what is made, where a restore puts a file back. What
    /// the window selects once listed (ADR-054's names settled first). A
    /// rename leaves its entry where it was, under another name: the
    /// listing follows it (`ListingEvent::Renamed`).
    pub fn placed(&self) -> Option<&Path> {
        match self {
            Change::Copy { to, .. } | Change::Move { to, .. } | Change::Link { to, .. } => Some(to),
            Change::Create { path, .. } | Change::Restore { path } => Some(path),
            _ => None,
        }
    }

    /// Where `path` is once this change holds: what a holder of paths (the
    /// shelf, a clipboard) does with each change a
    /// [`ChangeObserver`](executor::ChangeObserver) hears.
    pub fn follow(&self, path: &Path) -> Followed {
        let under = |root: &Path| path.strip_prefix(root).ok();
        let moved = |from: &Path, to: &Path| {
            under(from).map_or(Followed::Same, |rest| {
                Followed::Moved(if rest.as_os_str().is_empty() {
                    to.to_path_buf()
                } else {
                    to.join(rest)
                })
            })
        };
        match self {
            Change::Rename { from, to } | Change::Move { from, to } => moved(from, to),
            Change::Swap { a, b } => match moved(a, b) {
                Followed::Same => moved(b, a),
                followed => followed,
            },
            Change::Trash { path: gone }
            | Change::Delete { path: gone }
            | Change::Remove { path: gone } => {
                if under(gone).is_some() {
                    Followed::Gone
                } else {
                    Followed::Same
                }
            }
            Change::Copy { .. }
            | Change::Link { .. }
            | Change::Restore { .. }
            | Change::Create { .. }
            | Change::Metadata { .. } => Followed::Same,
        }
    }
}

/// Changes indexed by the paths they move or take away, so a holder of
/// many paths (a search's results, the clipboard, the shelf) follows each
/// through the changes that reach it or a folder above it, in order,
/// never through all of them: following every path through every change
/// was a plan's size times the holder's, tens of seconds for a move of
/// 20 000 files a search showed.
pub struct Follow<'a> {
    changes: &'a [Change],
    /// For each path a change moves, takes away or changes the metadata
    /// of, the changes that do, in order.
    by_path: std::collections::HashMap<&'a Path, Vec<usize>>,
}

/// What became of a path through changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Through {
    /// Where it is now; `None` once it, or a folder above it, was trashed
    /// or deleted.
    pub at: Option<PathBuf>,
    /// It, or a folder above it, was renamed, moved or swapped (perhaps
    /// back where it was).
    pub moved: bool,
    /// Its metadata was changed where it was then.
    pub changed: bool,
}

impl<'a> Follow<'a> {
    pub fn new(changes: &'a [Change]) -> Self {
        let mut by_path: std::collections::HashMap<&'a Path, Vec<usize>> =
            std::collections::HashMap::new();
        for (i, change) in changes.iter().enumerate() {
            let (first, second): (Option<&Path>, Option<&Path>) = match change {
                Change::Rename { from, .. } | Change::Move { from, .. } => (Some(from), None),
                Change::Swap { a, b } => (Some(a), Some(b)),
                Change::Trash { path } | Change::Delete { path } | Change::Remove { path } => {
                    (Some(path), None)
                }
                Change::Metadata { path, .. } => (Some(path), None),
                Change::Copy { .. }
                | Change::Link { .. }
                | Change::Restore { .. }
                | Change::Create { .. } => (None, None),
            };
            for path in first.into_iter().chain(second) {
                by_path.entry(path).or_default().push(i);
            }
        }
        Follow { changes, by_path }
    }

    /// `path` through the changes, in the order they were applied.
    pub fn through(&self, path: &Path) -> Through {
        let mut through = Through {
            at: Some(path.to_path_buf()),
            moved: false,
            changed: false,
        };
        let mut next = 0;
        while let Some(at) = through.at.as_deref() {
            // The first change not taken yet that names this path or a
            // folder above it.
            let Some(i) = at
                .ancestors()
                .filter_map(|above| {
                    let indexes = self.by_path.get(above)?;
                    indexes.get(indexes.partition_point(|&i| i < next)).copied()
                })
                .min()
            else {
                break;
            };
            let change = &self.changes[i];
            match change.follow(at) {
                Followed::Same => {
                    through.changed |=
                        matches!(change, Change::Metadata { path, .. } if path == at);
                }
                Followed::Moved(to) => {
                    through.at = Some(to);
                    through.moved = true;
                }
                Followed::Gone => through.at = None,
            }
            next = i + 1;
        }
        through
    }
}

/// Result of a dry run of one step.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum StepStatus {
    /// Will be applied. `reversible: false` means the journal cannot undo it.
    Pending { reversible: bool },
    /// The effect is already in place; applying is a no-op.
    AlreadyDone,
    /// Cannot be applied as-is (target exists, source moved, ...).
    Conflict { reason: String },
}

/// A mutation with an inverse.
pub trait Operation: Send + Sync + fmt::Debug {
    /// Registered kind name, e.g. `rename`.
    fn kind(&self) -> &'static str;
    /// Arguments as JSON, so plans serialize and travel.
    fn args(&self) -> serde_json::Value;
    /// What this will do.
    fn describe(&self) -> Change;
    /// Dry run. `ctx` holds lookups shared by the plan's steps.
    fn check(&self, ctx: &StepContext) -> StepStatus;
    /// Do it. Returns the inverse, or `None` when irreversible. A step that
    /// takes long (a copy) checks `ctx` between files and reports its bytes;
    /// cancelled, it undoes its own partial work and returns [`Cancelled`].
    fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>>;
    /// Clone into a box (plans are cloned into the journal).
    fn clone_box(&self) -> Box<dyn Operation>;
    /// Whether its inverse can undo it: false for what `apply` never
    /// returns an inverse for (deleting permanently). A plan with such a
    /// step is asked about before the user's gesture applies it
    /// ([`Plan::undoable`]).
    fn undoable(&self) -> bool {
        true
    }
    /// The bytes it holds beyond its paths, which the undo journal counts
    /// against its bound (`executor::JOURNAL_BYTES`): the notes a restore
    /// puts back, a sidecar's text whole.
    fn held(&self) -> usize {
        0
    }
    /// The folder it will write into, and how many bytes it will write
    /// there, as it will count them (`ctx`'s files of several names once):
    /// what the plan's room is checked against (`room::short`). `None` for
    /// what writes no file's bytes. Reads what it would copy: on the plan's
    /// thread.
    fn writes(&self, ctx: &StepContext) -> Option<(PathBuf, u64)> {
        let _ = ctx;
        None
    }
    /// The step as its own type, for code that knows the kind (`clash`
    /// reads a copy's and a move's). `None` for one that is not its own
    /// data.
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        None
    }
    /// What it touches, for the executor to tell which actions may run at
    /// once (ADR-079, `footprint`): the trees it reads, those it makes,
    /// writes or removes, the folders whose names it changes, the stores
    /// it keeps with others, and whether it is heavy. `None` where it
    /// cannot be told: its action runs alone. From what the step says,
    /// `volumes` telling a move within a volume apart; on the executor's
    /// thread, never the window's.
    fn footprint(&self, volumes: &footprint::Volumes) -> Option<footprint::Footprint> {
        let _ = volumes;
        None
    }
    /// What it makes, new, under a name of its own ([`Made`]): an archive
    /// extracted, one made. A name taken there is settled as a copy's is
    /// (`clash`), with a file's choices: what is made is never merged.
    /// `None` for any other step. May read what it is made of, where that
    /// says its name (an archive's one member): on the plan's thread.
    fn makes(&self, ctx: &StepContext) -> Option<Made> {
        let _ = ctx;
        None
    }
    /// The same step making what it makes under `name`: the name kept
    /// beside one taken, or the one `makes` found, which settling fixes
    /// in the step so that what is checked is what is applied.
    fn made_as(&self, name: std::ffi::OsString) -> Option<Box<dyn Operation>> {
        let _ = name;
        None
    }
    /// The steps it is once what it reads is read ([`unfold`]): a notes
    /// edit, and the rename of a name that holds the file's tags. `None`
    /// for a step that stays itself, as every step it returns does. May
    /// read: on the plan's thread, or the review's worker.
    fn unfolds(&self, ctx: &StepContext) -> Option<Vec<Box<dyn Operation>>> {
        let _ = ctx;
        None
    }
}

/// `steps` as each says it is once what it reads is read
/// ([`Operation::unfolds`]), before anything checks them: what the dry run
/// and the review show is what is applied, and Undo takes back each step
/// with its own inverse. `None` when none unfolds. A step that panics
/// stays itself, for its check to fail.
pub(crate) fn unfold(
    steps: &[Box<dyn Operation>],
    ctx: &StepContext,
) -> Option<Vec<Box<dyn Operation>>> {
    let mut out: Option<Vec<Box<dyn Operation>>> = None;
    for (i, step) in steps.iter().enumerate() {
        match guarded(|| Ok(step.unfolds(ctx))).ok().flatten() {
            Some(more) => out.get_or_insert_with(|| steps[..i].to_vec()).extend(more),
            None => {
                if let Some(out) = &mut out {
                    out.push(step.clone());
                }
            }
        }
    }
    out
}

/// What a step makes, new, under a name of its own in a folder
/// ([`Operation::makes`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Made {
    /// Where it lands.
    pub target: PathBuf,
    /// What it is made from (the archive extracted), or, for what is made
    /// of several, the first: the path of what comes in its clash, which
    /// says what is made (`clash::Side`).
    pub from: PathBuf,
    /// Everything it reads to make it (the archive; what an archive is
    /// made of): never put in the trash to make room for it, and what
    /// Replace if newer compares with what is there.
    pub inputs: Vec<PathBuf>,
    /// Whether it is a folder: a name kept beside a file's keeps its
    /// extension last.
    pub folder: bool,
    /// A file's size where the step knows it before making it (an
    /// archive's one file, as listed): said beside what is there.
    pub size: Option<u64>,
    /// What the question says of it: the verb on its button and what
    /// nothing is until it is answered ("Extract", "extracted").
    pub words: (&'static str, &'static str),
}

impl Clone for Box<dyn Operation> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

/// What an operation does, for one that is its own data: with [`Kind`]
/// beside it, it is an [`Operation`], whose kind, arguments and clone it
/// need not write.
pub trait Effect: Send + Sync + fmt::Debug {
    /// What this will do.
    fn describe(&self) -> Change;
    /// Dry run (`Operation::check`).
    fn check(&self, ctx: &StepContext) -> StepStatus;
    /// Do it (`Operation::apply`).
    fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>>;
    /// `Operation::undoable`.
    fn undoable(&self) -> bool {
        true
    }
    /// `Operation::writes`.
    fn writes(&self, ctx: &StepContext) -> Option<(PathBuf, u64)> {
        let _ = ctx;
        None
    }
    /// `Operation::held`.
    fn held(&self) -> usize {
        0
    }
    /// `Operation::makes`.
    fn makes(&self, ctx: &StepContext) -> Option<Made> {
        let _ = ctx;
        None
    }
    /// `Operation::made_as`.
    fn made_as(&self, name: std::ffi::OsString) -> Option<Box<dyn Operation>> {
        let _ = name;
        None
    }
    /// `Operation::footprint`.
    fn footprint(&self, volumes: &footprint::Volumes) -> Option<footprint::Footprint> {
        let _ = volumes;
        None
    }
}

/// An operation that is its own data: the kind it is registered and
/// serialized under, said once (`OpRegistry::register`), its arguments
/// itself.
pub trait Kind: Effect + Clone + Serialize + serde::de::DeserializeOwned + 'static {
    const KIND: &'static str;
}

impl<T: Kind> Operation for T {
    fn kind(&self) -> &'static str {
        T::KIND
    }
    fn args(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("serializable")
    }
    fn describe(&self) -> Change {
        Effect::describe(self)
    }
    fn check(&self, ctx: &StepContext) -> StepStatus {
        Effect::check(self, ctx)
    }
    fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        Effect::apply(self, ctx)
    }
    fn clone_box(&self) -> Box<dyn Operation> {
        Box::new(self.clone())
    }
    fn undoable(&self) -> bool {
        Effect::undoable(self)
    }
    fn writes(&self, ctx: &StepContext) -> Option<(PathBuf, u64)> {
        Effect::writes(self, ctx)
    }
    fn held(&self) -> usize {
        Effect::held(self)
    }
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn makes(&self, ctx: &StepContext) -> Option<Made> {
        Effect::makes(self, ctx)
    }
    fn made_as(&self, name: std::ffi::OsString) -> Option<Box<dyn Operation>> {
        Effect::made_as(self, name)
    }
    fn footprint(&self, volumes: &footprint::Volumes) -> Option<footprint::Footprint> {
        Effect::footprint(self, volumes)
    }
}

/// Serialized form of an operation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct OpData {
    pub kind: String,
    pub args: serde_json::Value,
}

type OpDeserializer =
    Arc<dyn Fn(serde_json::Value) -> anyhow::Result<Box<dyn Operation>> + Send + Sync>;

/// Registry of operation kinds, so serialized plans can be rebuilt.
#[derive(Default)]
pub struct OpRegistry {
    kinds: RwLock<indexmap::IndexMap<&'static str, (Arc<str>, OpDeserializer)>>,
}

impl OpRegistry {
    /// Register an operation that is its own data, under its kind.
    pub fn register<O: Kind>(&self, owner: &str) {
        let d: OpDeserializer = Arc::new(|v| Ok(Box::new(serde_json::from_value::<O>(v)?)));
        self.kinds.write().insert(O::KIND, (owner.into(), d));
    }

    /// Register `kind` with a builder of its own: for an operation that
    /// needs what the plugin holds (a registry) besides its data.
    pub fn register_with(
        &self,
        owner: &str,
        kind: &'static str,
        build: impl Fn(serde_json::Value) -> anyhow::Result<Box<dyn Operation>> + Send + Sync + 'static,
    ) {
        self.kinds
            .write()
            .insert(kind, (owner.into(), Arc::new(build)));
    }

    /// The kinds registered, in registration order.
    pub fn kinds(&self) -> Vec<&'static str> {
        self.kinds.read().keys().copied().collect()
    }

    pub fn unregister_owner(&self, owner: &str) {
        self.kinds.write().retain(|_, (o, _)| &**o != owner);
    }

    pub fn build(&self, data: &OpData) -> anyhow::Result<Box<dyn Operation>> {
        let d = self
            .kinds
            .read()
            .get(data.kind.as_str())
            .map(|(_, d)| Arc::clone(d));
        let d = d.ok_or_else(|| anyhow::anyhow!("unknown operation kind `{}`", data.kind))?;
        d(data.args.clone())
    }
}

/// Plan identifier, unique within a process.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PlanId(pub u64);

static NEXT_PLAN: AtomicU64 = AtomicU64::new(1);

/// An ordered list of operations. The unit of apply, review, and undo.
#[derive(Clone, Debug)]
pub struct Plan {
    pub id: PlanId,
    pub title: String,
    /// What the review says under the title: what the plan left out and
    /// why ("3 pictures outside the track's times"). The toast does not
    /// repeat it.
    pub note: Option<String>,
    pub steps: Vec<Box<dyn Operation>>,
    /// Who proposes it when the user did not make it: "An agent", a
    /// plugin from outside by its title. The review says so.
    pub by: Option<String>,
    /// What becomes of a name taken where it copies or moves, said
    /// beforehand (ADR-054); `None` asks whoever applies it, and applies
    /// nothing where no one can be asked.
    pub on_conflict: Option<clash::Policy>,
    /// What the user asked for, where its steps are not its items one by
    /// one (names taken settled, ADR-054: a merge, a replace): what the
    /// toast says it did.
    pub items: Option<Box<Items>>,
}

/// A plan's items as the user asked for them: the first one's change and
/// how many there are, and where each was put.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Items {
    pub first: Change,
    pub count: usize,
    /// Where each item that puts something was put, in plan order
    /// (`Change::placed`): under the name it was settled to (`photos
    /// (copy)`), a folder merged into where it is, though the steps that
    /// merged it put files inside it. What the window selects once the
    /// plan is done; not said with the plan's events (the toast counts).
    #[serde(skip)]
    pub placed: Vec<PathBuf>,
}

/// Serialized plan.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PlanData {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub steps: Vec<OpData>,
    /// What becomes of a name taken where a step copies or moves: asked
    /// when applied in the window when omitted, else nothing applied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_conflict: Option<clash::Policy>,
}

/// The dry-run view of a plan: what each step will do and whether it can.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PlanDiff {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub steps: Vec<(Change, StepStatus)>,
}

impl Plan {
    pub fn new(title: impl Into<String>) -> Self {
        Plan {
            id: PlanId(NEXT_PLAN.fetch_add(1, Ordering::Relaxed)),
            title: title.into(),
            note: None,
            steps: Vec::new(),
            by: None,
            on_conflict: None,
            items: None,
        }
    }

    /// The same plan, settling the names it finds taken as `policy` says.
    #[must_use]
    pub fn on_conflict(mut self, policy: Option<clash::Policy>) -> Self {
        self.on_conflict = policy;
        self
    }

    /// The same plan, proposed by `who` ("An agent").
    #[must_use]
    pub fn proposed_by(mut self, who: impl Into<String>) -> Self {
        self.by = Some(who.into());
        self
    }

    /// The same plan, with what the review says under its title.
    #[must_use]
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }

    pub fn with_steps(title: impl Into<String>, steps: Vec<Box<dyn Operation>>) -> Self {
        let mut p = Plan::new(title);
        p.steps = steps;
        p
    }

    pub fn push(&mut self, op: impl Operation + 'static) -> &mut Self {
        self.steps.push(Box::new(op));
        self
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// Whether Undo can take it back: no step is irreversible
    /// ([`Operation::undoable`]).
    pub fn undoable(&self) -> bool {
        self.steps.iter().all(|s| s.undoable())
    }

    pub fn len(&self) -> usize {
        self.steps.len()
    }

    /// Dry run every step, with the companion rules it will be applied
    /// with (`App::companions`): as it will be done, the names it finds
    /// taken settled as it says (ADR-054); saying nothing, those are its
    /// conflicts.
    pub fn diff(&self, companions: &Arc<crate::companions::Companions>) -> PlanDiff {
        let steps = self.unfolded(companions);
        let settled = self.on_conflict.map(|policy| {
            clash::settle(
                &steps,
                &clash::Answer::all(policy),
                &StepContext::detached().with_companions(Arc::clone(companions)),
            )
        });
        PlanDiff {
            title: self.title.clone(),
            note: self.note.clone(),
            steps: dry_run(
                settled.as_ref().map_or(&steps, |settled| &settled.steps),
                companions,
            )
            .collect(),
        }
    }

    /// Its steps unfolded ([`unfold`]), as they will be applied.
    fn unfolded(
        &self,
        companions: &Arc<crate::companions::Companions>,
    ) -> std::borrow::Cow<'_, [Box<dyn Operation>]> {
        let ctx = StepContext::detached().with_companions(Arc::clone(companions));
        unfold(&self.steps, &ctx).map_or(std::borrow::Cow::Borrowed(&self.steps[..]), Into::into)
    }

    /// The same plan with its steps unfolded ([`unfold`]), when one is:
    /// then its items are the steps it was asked as, which the toast
    /// counts, unless settling names its own.
    pub(crate) fn unfold(self, ctx: &StepContext) -> Plan {
        let Some(steps) = unfold(&self.steps, ctx) else {
            return self;
        };
        let items = self.items.or_else(|| {
            let asked: Vec<Change> = self.steps.iter().map(|step| step.describe()).collect();
            let placed = asked
                .iter()
                .filter_map(Change::placed)
                .map(Path::to_path_buf);
            Some(Box::new(Items {
                placed: placed.collect(),
                count: asked.len(),
                first: asked.into_iter().next()?,
            }))
        });
        Plan {
            steps,
            items,
            ..self
        }
    }

    /// What a review shows of the plan before the user applies it: its dry
    /// run as it will be done, the names it finds taken settled as it says,
    /// else as the defaults would (both kept, folders merged); and how
    /// many of those the user is asked about as it is applied.
    pub fn review(&self, companions: &Arc<crate::companions::Companions>) -> (PlanDiff, usize) {
        let settled = clash::settle(
            &self.unfolded(companions),
            &clash::Answer::all(self.on_conflict.unwrap_or_default()),
            &StepContext::detached().with_companions(Arc::clone(companions)),
        );
        let asked = if self.on_conflict.is_some() {
            0
        } else {
            settled
                .clashes
                .iter()
                .filter(|c| c.inside.is_none())
                .count()
        };
        let diff = PlanDiff {
            title: self.title.clone(),
            note: self.note.clone(),
            steps: dry_run(&settled.steps, companions).collect(),
        };
        (diff, asked)
    }

    /// Whether every step is pending or already done (no conflicts).
    pub fn is_applicable(&self, companions: &Arc<crate::companions::Companions>) -> bool {
        dry_run(&self.unfolded(companions), companions)
            .all(|(_, status)| !matches!(status, StepStatus::Conflict { .. }))
    }

    pub fn to_data(&self) -> PlanData {
        PlanData {
            title: self.title.clone(),
            note: self.note.clone(),
            on_conflict: self.on_conflict,
            steps: self
                .steps
                .iter()
                .map(|s| OpData {
                    kind: s.kind().into(),
                    args: s.args(),
                })
                .collect(),
        }
    }

    pub fn from_data(data: &PlanData, registry: &OpRegistry) -> anyhow::Result<Self> {
        let steps = data
            .steps
            .iter()
            .map(|d| registry.build(d))
            .collect::<anyhow::Result<Vec<_>>>()?;
        let mut plan = Plan::with_steps(data.title.clone(), steps);
        plan.note.clone_from(&data.note);
        plan.on_conflict = data.on_conflict;
        Ok(plan)
    }
}

#[cfg(test)]
mod tests {
    use super::{Change, Follow, Followed, Through, Words, io_words, said};
    use std::path::{Path, PathBuf};

    /// Said from a folder, a change writes what is inside it from there,
    /// anything else whole; said alone, every path whole.
    #[test]
    fn a_change_is_said_from_the_folder_its_paths_share() {
        let dir = Path::new("/data/it's a café");
        let rename = Change::Rename {
            from: dir.join("old  name.txt"),
            to: dir.join("sub/new name.txt"),
        };
        assert_eq!(
            rename.to_string(),
            format!(
                "rename {} -> {}",
                dir.join("old  name.txt").display(),
                dir.join("sub/new name.txt").display()
            )
        );
        assert_eq!(
            rename.within(dir).to_string(),
            "rename old  name.txt -> sub/new name.txt"
        );
        // Where it leads, outside the folder, stays whole.
        let link = Change::Link {
            from: PathBuf::from("/elsewhere/target"),
            to: dir.join("link"),
        };
        assert_eq!(
            link.within(dir).to_string(),
            "link link -> /elsewhere/target"
        );
        // The folder itself is not written as nothing.
        let made = Change::Create {
            path: dir.to_path_buf(),
            directory: true,
            words: None,
        };
        assert_eq!(made.within(dir).to_string(), "mkdir /data/it's a café");
        // A step's own words are what it says.
        let placed = Change::Metadata {
            path: dir.join("a.jpg"),
            field: "position".into(),
            words: Some(Words {
                will: "place “a.jpg” at 10, 20".into(),
                did: "Placed “a.jpg” at 10, 20".into(),
            }),
        };
        assert_eq!(placed.within(dir).to_string(), "place “a.jpg” at 10, 20");
        let paths: Vec<_> = rename.paths().collect();
        assert_eq!(
            paths,
            [dir.join("old  name.txt"), dir.join("sub/new name.txt")]
        );
    }

    #[test]
    fn a_failure_is_said_in_words_after_its_contexts() {
        let denied = anyhow::Error::new(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
            .context("“b.txt” could not be moved to “locked”");
        assert_eq!(
            said(&denied),
            "“b.txt” could not be moved to “locked”: permission denied"
        );
        let full = std::io::Error::from(std::io::ErrorKind::StorageFull);
        assert_eq!(io_words(&full), "the drive is full");
        let other = std::io::Error::other("the helper stopped");
        assert_eq!(io_words(&other), "the helper stopped");
        #[cfg(target_os = "linux")]
        assert_eq!(
            io_words(&std::io::Error::from_raw_os_error(71)),
            "Protocol error",
            "its number left out"
        );
    }

    #[test]
    fn a_path_follows_changes_to_itself_and_its_folders() {
        let p = |s: &str| PathBuf::from(s);
        let mv = Change::Move {
            from: p("/a/d"),
            to: p("/b/d"),
        };
        assert_eq!(mv.follow(Path::new("/a/d")), Followed::Moved(p("/b/d")));
        assert_eq!(mv.follow(Path::new("/a/d/x")), Followed::Moved(p("/b/d/x")));
        assert_eq!(
            mv.follow(Path::new("/a/dx")),
            Followed::Same,
            "components, not bytes"
        );
        let swap = Change::Swap {
            a: p("/a"),
            b: p("/b"),
        };
        assert_eq!(swap.follow(Path::new("/b/x")), Followed::Moved(p("/a/x")));
        let trash = Change::Trash { path: p("/a") };
        assert_eq!(trash.follow(Path::new("/a/x")), Followed::Gone);
        assert_eq!(trash.follow(Path::new("/ab")), Followed::Same);
        let copy = Change::Copy {
            from: p("/a"),
            to: p("/b"),
        };
        assert_eq!(copy.follow(Path::new("/a")), Followed::Same);
    }

    /// Following each path through only the changes that reach it says
    /// what following it through every change, in order, does.
    #[test]
    fn many_paths_follow_the_changes_that_reach_them_in_order() {
        let p = |s: &str| PathBuf::from(s);
        let changes = vec![
            Change::Move {
                from: p("/a/d"),
                to: p("/b/d"),
            },
            // The folder moved, then a file in it renamed where it is now.
            Change::Rename {
                from: p("/b/d/x"),
                to: p("/b/d/y"),
            },
            // A file arriving where another left: the first is not it.
            Change::Move {
                from: p("/c/z"),
                to: p("/a/d"),
            },
            Change::Metadata {
                path: p("/b/d/y"),
                field: "tags".into(),
                words: None,
            },
            Change::Swap {
                a: p("/e"),
                b: p("/f"),
            },
            Change::Trash { path: p("/f/gone") },
            Change::Move {
                from: p("/b/d"),
                to: p("/a/d2"),
            },
            Change::Copy {
                from: p("/q"),
                to: p("/r"),
            },
        ];
        let naive = |path: &Path| {
            let mut through = Through {
                at: Some(path.to_path_buf()),
                moved: false,
                changed: false,
            };
            for change in &changes {
                let Some(at) = through.at.clone() else { break };
                match change.follow(&at) {
                    Followed::Same => {
                        through.changed |=
                            matches!(change, Change::Metadata { path, .. } if *path == at);
                    }
                    Followed::Moved(to) => {
                        through.at = Some(to);
                        through.moved = true;
                    }
                    Followed::Gone => through.at = None,
                }
            }
            through
        };
        let follow = Follow::new(&changes);
        for path in [
            "/a/d", "/a/d/x", "/a/d/w", "/c/z", "/c/zz", "/e/gone", "/e/kept", "/f", "/q", "/",
            "/b/d/y",
        ] {
            assert_eq!(
                follow.through(Path::new(path)),
                naive(Path::new(path)),
                "{path}"
            );
        }
        assert_eq!(
            follow.through(Path::new("/a/d/x")),
            Through {
                at: Some(p("/a/d2/y")),
                moved: true,
                changed: true,
            }
        );
        assert_eq!(follow.through(Path::new("/c/z")).at, Some(p("/a/d")));
        assert_eq!(follow.through(Path::new("/e/gone")).at, None);
    }

    #[test]
    fn windows_refuses_its_characters_its_endings_and_its_devices() {
        use super::windows_name_problem as problem;
        for fine in [
            "notes.txt",
            "it's here",
            "CONSOLE",
            "con-fig",
            "COM10",
            "COM0",
            "lpt",
            "été",
            ".hidden",
            "a.b.c",
        ] {
            assert_eq!(problem(fine), None, "{fine}");
        }
        for (name, says) in [
            ("a:b", "“:”"),
            ("what?", "“?”"),
            ("back\\slash", "“\\”"),
            ("\"quoted\"", "“\"”"),
            ("a|b", "“|”"),
            ("tab\there", "control"),
            ("ends.", "end with"),
            ("ends ", "end with"),
            ("CON", "device"),
            ("con.txt", "device"),
            ("Nul.tar.gz", "device"),
            ("aux ", "end with"),
            ("PRN .txt", "device"),
            ("COM1", "device"),
            ("lpt9.log", "device"),
            ("COM¹", "device"),
        ] {
            let said = problem(name).unwrap_or_else(|| panic!("{name} taken"));
            assert!(said.contains(says), "{name}: {said}");
        }
    }
}
