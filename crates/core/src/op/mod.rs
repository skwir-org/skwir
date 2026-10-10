//! Operations, plans, and the executor.
//!
//! An operation is a mutation with an inverse. A plan is an ordered list of
//! operations referencing entries by stable identity. Nothing mutates the
//! filesystem except the executor applying a plan, which journals the
//! inverse so that everything is undoable. Applying a plan twice is a no-op:
//! each step checks whether it is already done before acting.

mod builtin;
mod context;
mod entry_ref;
mod executor;
mod order;
mod pace;
#[cfg(target_os = "linux")]
mod put_trash;
pub mod records;
pub mod room;
pub mod staging;
mod sys;

use std::ffi::OsStr;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

pub use builtin::{
    Copy, Delete, InTrash, Landed, Link, Mkdir, Move, NewFile, RemoveEmptyFile, Rename, Restore,
    RestorePart, ReturnPart, Rmdir, Swap, Trash, Unlink, Unmerge, Untrashed, in_trash,
    parse_trash_info, register_builtin, trash_info_of,
};
pub use context::{Cancelled, StepContext};
pub use entry_ref::{EntryArg, EntryRef, ResolveError};
pub use executor::{
    ChangeObserver, ExecEvent, ExecResult, Executor, JournalEntry, Progress, RunKind, Transfer,
    Verdict,
};
pub use order::{RenameOrderError, RenameRequest, order_renames};
pub use staging::Staging;

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
    let ctx = StepContext::detached().with_companions(Arc::clone(companions));
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
        match self {
            Change::Rename { from, to } => {
                write!(f, "rename {} -> {}", from.display(), to.display())
            }
            Change::Move { from, to } => write!(f, "move {} -> {}", from.display(), to.display()),
            Change::Copy { from, to } => write!(f, "copy {} -> {}", from.display(), to.display()),
            // As `ls -l` shows a link: where it is, then where it leads.
            Change::Link { from, to } => write!(f, "link {} -> {}", to.display(), from.display()),
            Change::Swap { a, b } => write!(f, "swap {} <-> {}", a.display(), b.display()),
            Change::Trash { path } => write!(f, "trash {}", path.display()),
            Change::Restore { path } => write!(f, "restore {}", path.display()),
            Change::Delete { path } => write!(f, "delete {}", path.display()),
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
            } => write!(f, "mkdir {}", path.display()),
            Change::Create {
                path,
                directory: false,
                ..
            } => write!(f, "create {}", path.display()),
            // An empty folder, or a file made empty and left so.
            Change::Remove { path } => write!(f, "remove {}", path.display()),
            Change::Metadata { path, field, .. } => {
                write!(f, "set {field} on {}", path.display())
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
}

/// Serialized plan.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PlanData {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub steps: Vec<OpData>,
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
        }
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
    /// with (`App::companions`).
    pub fn diff(&self, companions: &Arc<crate::companions::Companions>) -> PlanDiff {
        PlanDiff {
            title: self.title.clone(),
            note: self.note.clone(),
            steps: dry_run(&self.steps, companions).collect(),
        }
    }

    /// Whether every step is pending or already done (no conflicts).
    pub fn is_applicable(&self, companions: &Arc<crate::companions::Companions>) -> bool {
        dry_run(&self.steps, companions)
            .all(|(_, status)| !matches!(status, StepStatus::Conflict { .. }))
    }

    pub fn to_data(&self) -> PlanData {
        PlanData {
            title: self.title.clone(),
            note: self.note.clone(),
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
        Ok(plan)
    }
}

#[cfg(test)]
mod tests {
    use super::{Change, Follow, Followed, Through, io_words, said};
    use std::path::{Path, PathBuf};

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
