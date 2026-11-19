//! Operations, plans, and the executor.
//!
//! An operation is a mutation with an inverse. A plan is an ordered list of
//! operations referencing entries by stable identity. Nothing mutates the
//! filesystem except the executor applying a plan, which journals the
//! inverse so that everything is undoable. Applying a plan twice is a no-op:
//! each step checks whether it is already done before acting.

pub mod access;
pub mod arriving;
mod builtin;
pub mod clash;
mod context;
mod entry_ref;
mod executor;
mod listed;
pub mod made;
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
mod speed;
pub mod staging;
mod steps;
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

use crate::words::Words;

pub use builtin::{
    Copy, CopyUndo, Delete, InTrash, Landed, Link, MadeBeside, Mkdir, Move, NewFile,
    RemoveEmptyFile, Rename, Restore, RestorePart, ReturnPart, Rmdir, Swap, Trash, Uncopy, Unlink,
    Unmerge, Unmove, Untrash, Untrashed, in_trash, keeps_no_trash, made_state, made_stays,
    parse_trash_info, register_builtin, remove_made, trash_info_of,
};
pub use context::{Cancelled, Gauge, Partly, StepContext, Unanswered, Unneeded};
pub use entry_ref::{EntryArg, EntryRef, ResolveError};
/// The plans, undos and redos that run at once at most (ADR-079).
pub use executor::AT_ONCE as ACTIONS_AT_ONCE;
/// The actions the undo history keeps at most (ADR-079, 11).
pub use executor::JOURNAL_ENTRIES as HISTORY_ENTRIES;
pub use executor::{
    Can, ChangeObserver, Done, ExecEvent, ExecResult, Executor, History, HistoryRow, JournalEntry,
    PlanObserver, Progress, Reader, RunKind, RunningPlan, SecretAsker, Since, Transfer, Verdict,
    Waiting, WaitsFor, counts_as_action,
};
pub use made::{MadeItem, MadeTree, Placing};
pub use order::{RenameOrderError, RenameRequest, order_renames};
pub use staging::Staging;
pub use steps::{Steps, each as each_step};
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
        let status = guarded(|| Ok(step.check(&ctx))).unwrap_or_else(|e| StepStatus::Conflict {
            reason: Words::of_error(e),
        });
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
        Err(crate::words!(
            op_panicked = "it failed unexpectedly",
            note = "Why a step of an action stopped: it failed in a way Skwir did not foresee. Begins in lower case: said after the step's own words."
        )
        .into())
    })
}

/// What a step's failure says to the user: its contexts, outermost first,
/// each written in the user's words where it is raised, then the system's
/// reason in words (`io::ErrorKind`), or its message without the error's
/// number. The whole chain goes to the log.
pub fn said(error: &anyhow::Error) -> String {
    said_of(error.chain())
}

/// The links of an error's chain, said as [`said`] says them, in English.
fn said_of<'a>(links: impl Iterator<Item = &'a (dyn std::error::Error + 'static)>) -> String {
    links
        .map(|cause| match cause.downcast_ref::<std::io::Error>() {
            Some(io) => io_words(io).english(),
            None => cause.to_string(),
        })
        .collect::<Vec<_>>()
        .join(": ")
}

/// What a step could not do to the file it acts on, in its own words
/// ("“a.txt” could not be copied"), put around the error that stopped it
/// (`.couldnt(|| …)`, [`Couldnt::because`]): the error's outermost link, its
/// kind still found inside by a downcast (`Cancelled`, an `io::Error`).
/// What tells of the failure in words of its own (the toast's "Couldn't
/// copy “a.txt” to Docs") says the reason after them alone
/// ([`reason_after`]), told by the link's make, where it stands and the
/// file it names, never by its words (ADR-139, 8). It is put around an
/// error only so: it is no context of its own (no `Display`), so that
/// where it stands is always known.
#[derive(Debug)]
pub struct Couldnt {
    /// The file its words name.
    subject: PathBuf,
    words: Words,
}

impl Couldnt {
    /// `subject`'s step could not do what `words` say.
    pub fn new(subject: &Path, words: impl Into<Words>) -> Self {
        Couldnt {
            subject: subject.to_owned(),
            words: words.into(),
        }
    }

    /// `error`, these words put around it: its outermost link.
    pub fn because(self, error: impl Into<anyhow::Error>) -> anyhow::Error {
        let error = error.into();
        let below = error.chain().count();
        error.context(CouldntLink {
            subject: self.subject,
            words: self.words,
            below,
        })
    }
}

/// A [`Couldnt`] put around an error ([`Couldnt::because`]): a link of its
/// chain, the links it was put around counted.
#[derive(Debug)]
struct CouldntLink {
    subject: PathBuf,
    words: Words,
    below: usize,
}

impl fmt::Display for CouldntLink {
    /// Its words: English, or the reader's language while an error's chain
    /// is said (`Words`' `Display`).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.words, f)
    }
}

/// A step's result, what it could not do put around its error
/// ([`Couldnt::because`]).
pub trait CouldntExt<T> {
    /// `Err(couldnt().because(error))` for an error.
    ///
    /// # Errors
    /// The error, its step's words put around it.
    fn couldnt(self, couldnt: impl FnOnce() -> Couldnt) -> anyhow::Result<T>;
}

impl<T, E: Into<anyhow::Error>> CouldntExt<T> for Result<T, E> {
    fn couldnt(self, couldnt: impl FnOnce() -> Couldnt) -> anyhow::Result<T> {
        self.map_err(|error| couldnt().because(error))
    }
}

/// What stopped `change`'s step, said alone, where its error begins with
/// the step's own words naming the file it acts on ([`Couldnt`]): what
/// follows them in its chain, in the reader's language where it is words.
/// `None` where the error begins otherwise.
pub fn reason_after(error: &Arc<anyhow::Error>, change: &Change) -> Option<Words> {
    // A downcast finds the outermost of its kind wherever it stands in the
    // chain; it is the error's first link where nothing was put around it
    // since: the chain is the links it was put around, and it.
    let own = error.downcast_ref::<CouldntLink>()?;
    if error.chain().count() != own.below + 1 {
        return None;
    }
    // By name: a copy's words name it where it lands, an item of a trash
    // by the trash's name, the same as the step's.
    let named = change.subject().and_then(Path::file_name);
    (named.is_some() && own.subject.file_name() == named).then(|| Words::of_error_from(error, 1))
}

/// A system's error in words: by its kind where the kind says it (the
/// same words on every system, said in the reader's language), else its
/// own message, as the system wrote it, its number left out.
pub fn io_words(error: &std::io::Error) -> Words {
    use crate::words;
    use std::io::ErrorKind as K;
    // Words of Skwir's own inside (`system::not_yet_words`): said as they
    // are, whatever the kind.
    if let Some(words) = error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<Words>())
    {
        return words.clone();
    }
    match error.kind() {
        K::PermissionDenied => words!(io_denied = "permission denied"),
        K::NotFound => words!(io_gone = "it is not there any more"),
        K::AlreadyExists => words!(io_taken = "something of that name is already there"),
        K::StorageFull => words!(io_full = "the drive is full"),
        K::QuotaExceeded => words!(io_quota = "the disk quota is used up"),
        K::FileTooLarge => words!(io_too_large = "the file is too large for that drive"),
        K::ReadOnlyFilesystem => words!(io_read_only = "the drive is read-only"),
        K::InvalidFilename => words!(io_bad_name = "the name is too long, or not allowed there"),
        K::DirectoryNotEmpty => words!(io_not_empty = "the folder is not empty"),
        K::ResourceBusy | K::ExecutableFileBusy => words!(io_busy = "it is in use"),
        K::NotADirectory => words!(io_not_folder = "a part of its path is not a folder"),
        K::IsADirectory => words!(io_is_folder = "it is a folder"),
        K::StaleNetworkFileHandle => words!(io_share_gone = "the network share went away"),
        K::TimedOut => words!(io_timed_out = "the drive did not answer in time"),
        _ => {
            let text = error.to_string();
            Words::as_written(match text.rfind(" (os error ") {
                Some(at) if text.ends_with(')') => text[..at].to_owned(),
                _ => text,
            })
        }
    }
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
    use crate::words;
    let bytes = name.as_encoded_bytes();
    if bytes.is_empty() {
        anyhow::bail!(words!(
            op_name_empty = "a name cannot be empty",
            note = "Why a name typed for a file is refused. Begins in lower case: said after other words too."
        ));
    }
    if bytes == b"." || bytes == b".." {
        anyhow::bail!(words!(
            op_name_dots = "{ $name } cannot be used as a name",
            name = words::Name(name.display().to_string()),
            note = "Why a name typed for a file is refused: it is . or .., which name folders themselves."
        ));
    }
    if bytes.contains(&b'/') {
        anyhow::bail!(words!(
            op_name_slash = "a name cannot contain “/”",
            note = "Why a name typed for a file is refused: / separates folders."
        ));
    }
    if bytes.contains(&0) {
        anyhow::bail!(words!(
            op_name_nul = "a name cannot contain a NUL character",
            note = "Why a name typed for a file is refused: it holds the character of code 0."
        ));
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
pub(crate) fn windows_name_problem(name: &str) -> Option<Words> {
    use crate::words;
    if let Some(c) = name
        .chars()
        .find(|c| matches!(c, '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
    {
        return Some(words!(
            op_name_character = "a name cannot contain { $character }",
            character = crate::words::Name(c.to_string()),
            note = "Why a name typed for a file is refused on Windows: it holds a character Windows keeps for itself (\\ : * ? \" < > |)."
        ));
    }
    if name.chars().any(char::is_control) {
        return Some(words!(
            op_name_control = "a name cannot contain a control character",
            note = "Why a name typed for a file is refused on Windows: it holds an invisible control character."
        ));
    }
    if name.ends_with(['.', ' ']) {
        return Some(words!(
            op_name_end = "a name cannot end with a dot or a space",
            note = "Why a name typed for a file is refused on Windows."
        ));
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
    device.then(|| {
        words!(
            op_name_device = "{ $name } is a device's name on Windows",
            name = words::Name(stem.to_owned()),
            note = "Why a name typed for a file is refused on Windows: CON, NUL, COM1… name devices there, with an extension or without."
        )
    })
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
        words: Option<Tenses>,
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
        words: Option<Tenses>,
    },
}

/// A step in the user's words: what it will do, as the review lists it
/// ("place “a.jpg” at 10.12000, 20.12000"), what it did, as the toast
/// says it ("Placed “a.jpg” at 10.12000, 20.12000"), and what its check
/// found when it was done already (`StepStatus::AlreadyDone`), which the
/// toast says instead ("“a.jpg” was already at 10.12000, 20.12000"):
/// never that it did what it found done. Its words in three tenses, each
/// said where it is shown.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Tenses {
    pub will: Words,
    pub did: Words,
    /// `None` in words written before it was said (absent, or empty as an
    /// older plugin from outside wrote it): then the toast says it as it
    /// says a change without words.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "unless_empty"
    )]
    pub already: Option<Words>,
}

/// Text that comes in as words, none where it is empty.
fn unless_empty<'de, D: serde::Deserializer<'de>>(to: D) -> Result<Option<Words>, D::Error> {
    let text = Option::<String>::deserialize(to)?;
    Ok(text.filter(|text| !text.is_empty()).map(Words::as_written))
}

impl fmt::Display for Change {
    /// In English, its paths whole: what a log reads ([`Change::words`]).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.words(None), f)
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

    /// What it does, as a review lists it, each path under `folder`
    /// written from there (paths as they are, in every language): a review
    /// that says the folder once lists `rename a.txt -> b.txt`, not the
    /// folder on every line. What its `Display` says in English, said in
    /// the reader's language where it is shown.
    pub fn words(&self, folder: Option<&Path>) -> Words {
        use crate::words;
        let p = |path| shown(path, folder).to_string();
        match self {
            Change::Rename { from, to } => words!(
                op_change_rename = "rename { $from } -> { $to }",
                from = p(from),
                to = p(to),
                note = "A line of a plan's review, terse as a listing: a file renamed, its path then its new path. Begins in lower case."
            ),
            Change::Move { from, to } => words!(
                op_change_move = "move { $from } -> { $to }",
                from = p(from),
                to = p(to),
                note = "A line of a plan's review, terse as a listing: a file moved, its path then where it goes. Begins in lower case."
            ),
            Change::Copy { from, to } => words!(
                op_change_copy = "copy { $from } -> { $to }",
                from = p(from),
                to = p(to),
                note = "A line of a plan's review, terse as a listing: a file copied, its path then its copy's. Begins in lower case."
            ),
            // As `ls -l` shows a link: where it is, then where it leads.
            Change::Link { from, to } => words!(
                op_change_link = "link { $link } -> { $target }",
                link = p(to),
                target = p(from),
                note = "A line of a plan's review, terse as a listing: a link made, where it is then where it leads, as ls -l shows it. Begins in lower case."
            ),
            Change::Swap { a, b } => words!(
                op_change_swap = "swap { $a } <-> { $b }",
                a = p(a),
                b = p(b),
                note = "A line of a plan's review, terse as a listing: two files' names swapped. Begins in lower case."
            ),
            Change::Trash { path } => words!(
                op_change_trash = "trash { $path }",
                path = p(path),
                note = "A line of a plan's review, terse as a listing: a file put in the trash. Begins in lower case."
            ),
            Change::Restore { path } => words!(
                op_change_restore = "restore { $path }",
                path = p(path),
                note = "A line of a plan's review, terse as a listing: a file put back from the trash. Begins in lower case."
            ),
            Change::Delete { path } => words!(
                op_change_delete = "delete { $path }",
                path = p(path),
                note = "A line of a plan's review, terse as a listing: a file deleted for good. Begins in lower case."
            ),
            // A step that says what it does says it best.
            Change::Create {
                words: Some(words), ..
            }
            | Change::Metadata {
                words: Some(words), ..
            } => words.will.clone(),
            Change::Create {
                path,
                directory: true,
                ..
            } => words!(
                op_change_mkdir = "mkdir { $path }",
                path = p(path),
                note = "A line of a plan's review, terse as a listing: a folder made, as the command that makes one is named. Begins in lower case."
            ),
            Change::Create {
                path,
                directory: false,
                ..
            } => words!(
                op_change_create = "create { $path }",
                path = p(path),
                note = "A line of a plan's review, terse as a listing: an empty file made. Begins in lower case."
            ),
            // An empty folder, or a file made empty and left so.
            Change::Remove { path } => words!(
                op_change_remove = "remove { $path }",
                path = p(path),
                note = "A line of a plan's review, terse as a listing: an empty folder, or a file made empty, removed. Begins in lower case."
            ),
            Change::Metadata { path, field, .. } => words!(
                op_change_metadata = "set { $field } on { $path }",
                field = field.clone(),
                path = p(path),
                note = "A line of a plan's review, terse as a listing: a field of a file written; field is its key (position, tags). Begins in lower case."
            ),
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
    /// Its own words, where it says what it does in words of its own (an
    /// archive extracted, a picture placed): what its `Display` says of it
    /// in English, said in the reader's language where it is shown.
    pub fn tenses(&self) -> Option<&Tenses> {
        match self {
            Change::Create { words, .. } | Change::Metadata { words, .. } => words.as_ref(),
            _ => None,
        }
    }

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

    /// Where it acts: what it places, renames, takes away, makes or
    /// writes. A copy's or a link's source is only read, and not said here.
    pub fn acts_at(&self) -> impl Iterator<Item = &Path> {
        let (first, second) = match self {
            Change::Rename { from, to } | Change::Move { from, to } => (from, Some(to)),
            Change::Copy { to, .. } | Change::Link { to, .. } => (to, None),
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
    /// Cannot be applied as-is (target exists, source moved, ...): why,
    /// said where it is shown, English to agents.
    Conflict { reason: Words },
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
    /// The file it writes, as a metadata change says it
    /// (`Change::Metadata`), where it knows it: what the run counts apart
    /// from what it found done already, under whichever name a file of
    /// several names was asked. None for what is known by its path alone.
    fn file(&self) -> Option<crate::entry::FileId> {
        None
    }
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
    /// The bytes it counts as it goes (`StepContext::add_total`, a
    /// `Gauge`'s), sized before the plan begins so that its total, and the
    /// time it has left, are known from its start (`room::sized`,
    /// ADR-113): what it writes (`writes`, given as read once), unless it
    /// counts other bytes (an archive extracted counts the archive's, a
    /// smaller copy its file's). A step that counts bytes says them here,
    /// or its plan's time left is told only once it begins. Reads: on the
    /// plan's thread.
    fn work(&self, ctx: &StepContext, writes: Option<u64>) -> u64 {
        let _ = ctx;
        writes.unwrap_or(0)
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
    /// (`clash`): with a file's choices, or a folder's onto a folder there
    /// where the step says what it makes item by item (`made_tree`), which
    /// it is then merged into (`merged`). `None` for any other step. May read what it is made of, where that
    /// says its name (an archive's one member): on the plan's thread.
    fn makes(&self, ctx: &StepContext) -> Option<Made> {
        let _ = ctx;
        None
    }
    /// What what it brings into a folder is made from, where its change
    /// does not say it (`Change::Restore` says where it goes alone): a
    /// restore's item in the trash, which what arrives there is read from
    /// as a copy's source is (ADR-135). Reads nothing.
    fn brings_from(&self) -> Option<PathBuf> {
        None
    }
    /// The same step making what it makes under `name`: the name kept
    /// beside one taken, or the one `makes` found, which settling fixes
    /// in the step so that what is checked is what is applied.
    fn made_as(&self, name: std::ffi::OsString) -> Option<Box<dyn Operation>> {
        let _ = name;
        None
    }
    /// What it makes, item by item, where it makes a folder that a folder
    /// there under its name may be merged into ([`MadeTree`]: an archive's
    /// members, as its listing says them). `None` where it cannot say all
    /// of it (a listing cut short): the name taken is then asked about
    /// with a file's choices. May read: on the plan's thread.
    fn made_tree(&self, ctx: &StepContext) -> Option<MadeTree> {
        let _ = ctx;
        None
    }
    /// The same step merged into the folder there under its name: it puts
    /// there only what `placing` says, each item where it says
    /// ([`Placing`]), as settling the names found them (ADR-054). `None`
    /// for a step that cannot be merged.
    fn merged(&self, placing: Vec<Placing>) -> Option<Box<dyn Operation>> {
        let _ = placing;
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
/// with its own inverse. `None` when none unfolds; else the steps, and
/// where each of `steps` begins among them. A step that panics stays
/// itself, for its check to fail.
pub(crate) fn unfold(
    steps: &[Box<dyn Operation>],
    ctx: &StepContext,
) -> Option<(Vec<Box<dyn Operation>>, Vec<usize>)> {
    let mut out: Option<(Vec<Box<dyn Operation>>, Vec<usize>)> = None;
    for (i, step) in steps.iter().enumerate() {
        match guarded(|| Ok(step.unfolds(ctx))).ok().flatten() {
            Some(more) => {
                let (out, starts) =
                    out.get_or_insert_with(|| (steps[..i].to_vec(), (0..i).collect()));
                starts.push(out.len());
                out.extend(more);
            }
            None => {
                if let Some((out, starts)) = &mut out {
                    starts.push(out.len());
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
    /// What the question of names taken says of it.
    pub words: MakeWords,
}

/// What the question of the names a step finds taken says of what it
/// makes ([`Made::words`], `clash::Act::Make`), each said where it is
/// shown: three messages, never one verb placed in the others' sentences.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MakeWords {
    /// The verb on its button: "Extract".
    pub verb: Words,
    /// What nothing is until it is answered, under its title: "Nothing is
    /// extracted until you choose."
    pub waiting: Words,
    /// What the window says once the question is cancelled: "Cancelled:
    /// nothing was extracted".
    pub cancelled: Words,
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
    /// `Operation::work`.
    fn work(&self, ctx: &StepContext, writes: Option<u64>) -> u64 {
        let _ = ctx;
        writes.unwrap_or(0)
    }
    /// `Operation::held`.
    fn held(&self) -> usize {
        0
    }
    /// `Operation::file`.
    fn file(&self) -> Option<crate::entry::FileId> {
        None
    }
    /// `Operation::makes`.
    fn makes(&self, ctx: &StepContext) -> Option<Made> {
        let _ = ctx;
        None
    }
    /// `Operation::brings_from`.
    fn brings_from(&self) -> Option<PathBuf> {
        None
    }
    /// `Operation::made_as`.
    fn made_as(&self, name: std::ffi::OsString) -> Option<Box<dyn Operation>> {
        let _ = name;
        None
    }
    /// `Operation::made_tree`.
    fn made_tree(&self, ctx: &StepContext) -> Option<MadeTree> {
        let _ = ctx;
        None
    }
    /// `Operation::merged`.
    fn merged(&self, placing: Vec<Placing>) -> Option<Box<dyn Operation>> {
        let _ = placing;
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
    fn work(&self, ctx: &StepContext, writes: Option<u64>) -> u64 {
        Effect::work(self, ctx, writes)
    }
    fn held(&self) -> usize {
        Effect::held(self)
    }
    fn file(&self) -> Option<crate::entry::FileId> {
        Effect::file(self)
    }
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn makes(&self, ctx: &StepContext) -> Option<Made> {
        Effect::makes(self, ctx)
    }
    fn brings_from(&self) -> Option<PathBuf> {
        Effect::brings_from(self)
    }
    fn made_as(&self, name: std::ffi::OsString) -> Option<Box<dyn Operation>> {
        Effect::made_as(self, name)
    }
    fn made_tree(&self, ctx: &StepContext) -> Option<MadeTree> {
        Effect::made_tree(self, ctx)
    }
    fn merged(&self, placing: Vec<Placing>) -> Option<Box<dyn Operation>> {
        Effect::merged(self, placing)
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

    /// The step `data` says. Never several as one ([`Steps`]): a plan
    /// lists each step it takes, which its review shows and its run
    /// foresees and journals one by one (`Plan::to_data` writes a
    /// [`Steps`] so).
    pub fn build(&self, data: &OpData) -> anyhow::Result<Box<dyn Operation>> {
        anyhow::ensure!(
            data.kind != steps::KIND,
            crate::words!(
                op_plan_steps_kind = "a plan lists each of its steps: `{ $kind }` is none",
                kind = steps::KIND,
                note = "Why a plan given as data is refused: one of its steps is of a kind that holds several. kind: that kind's identifier."
            )
        );
        let d = self
            .kinds
            .read()
            .get(data.kind.as_str())
            .map(|(_, d)| Arc::clone(d));
        let d = d.ok_or_else(|| {
            anyhow::Error::new(crate::words!(
                op_unknown_kind = "unknown operation kind `{ $kind }`",
                kind = data.kind.clone(),
                note = "Why a plan given as data is refused: no step of that kind is known (its plugin is off, or it is misspelt). kind: the identifier given."
            ))
        })?;
        d(data.args.clone())
    }
}

/// Plan identifier, unique within a process.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PlanId(pub u64);

static NEXT_PLAN: AtomicU64 = AtomicU64::new(1);

impl PlanId {
    /// An id no plan of this process has: a new plan's, or the one an
    /// undo's or a redo's run will go by, made as it is asked
    /// (`Executor::undo`).
    pub fn fresh() -> Self {
        PlanId(NEXT_PLAN.fetch_add(1, Ordering::Relaxed))
    }
}

/// An ordered list of operations. The unit of apply, review, and undo.
#[derive(Clone, Debug)]
pub struct Plan {
    pub id: PlanId,
    /// What the user knows it by, said where it is shown: a card, a
    /// toast, the History, the review.
    pub title: Words,
    /// What the review says under the title: what the plan left out and
    /// why ("3 pictures outside the track's times"). The toast does not
    /// repeat it.
    pub note: Option<Words>,
    pub steps: Vec<Box<dyn Operation>>,
    /// Who proposes it when the user did not make it: "An agent", a
    /// plugin from outside by its title. The review says so.
    pub by: Option<Words>,
    /// What becomes of a name taken where it copies or moves, said
    /// beforehand (ADR-054); `None` asks whoever applies it, and applies
    /// nothing where no one can be asked.
    pub on_conflict: Option<clash::Policy>,
    /// What the user asked for, where its steps are not its items one by
    /// one (names taken settled, ADR-054: a merge, a replace): what the
    /// toast says it did.
    pub items: Option<Box<Items>>,
    /// For an undo's run, the title of the action it takes back, as its
    /// history keeps it: what tells of the run says it apart from the
    /// run's own `title`, never by taking that apart (ADR-139, 8).
    pub undoes: Option<Words>,
}

/// A folder as a title names it (ADR-135): by its name, "/" by itself.
/// Its whole path is said where its action is pointed at
/// (`Executor::brings_into`), never in the title, which a card, a toast
/// and the History draw on one line.
pub fn folder_said(folder: &Path) -> String {
    folder.file_name().map_or_else(
        || folder.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// What a transfer does to what it takes, for its title
/// ([`transfer_words`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Carried {
    Move,
    Copy,
    Link,
}

/// A transfer's title (ADR-135) in words: what it does, what it takes
/// (one by its name, several by their count), then the folder it goes to
/// by its name ([`folder_said`]): "copy 3 items to Projects", "link “a.txt”
/// in Projects".
pub fn transfer_words(carried: Carried, taken: &[&Path], dest: &Path) -> Words {
    use crate::words;
    use crate::words::{Folder, Name};
    let count = taken.len();
    let name = Name(taken.first().map_or_else(String::new, |one| {
        one.file_name()
            .map_or_else(|| one.to_string_lossy(), |name| name.to_string_lossy())
            .into_owned()
    }));
    let folder = Folder(folder_said(dest));
    match carried {
        Carried::Move => words!(
            op_move_title = "{ $count ->
                [1] move { $name } to { $folder }
               *[other] move { $count } items to { $folder }
            }",
            count = count,
            name = name,
            folder = folder,
            note = "An action's title, in its review, the list of actions and Undo: the file moved (or how many), then the folder it goes to, by its name. Begins in lower case: said inside other words too."
        ),
        Carried::Copy => words!(
            op_copy_title = "{ $count ->
                [1] copy { $name } to { $folder }
               *[other] copy { $count } items to { $folder }
            }",
            count = count,
            name = name,
            folder = folder,
            note = "An action's title: the file copied (or how many), then the folder it goes to, by its name. Begins in lower case: said inside other words too."
        ),
        Carried::Link => words!(
            op_link_title = "{ $count ->
                [1] link { $name } in { $folder }
               *[other] link { $count } items in { $folder }
            }",
            count = count,
            name = name,
            folder = folder,
            note = "An action's title: the file a link is made to (or how many), then the folder the links are made in, by its name. Begins in lower case: said inside other words too."
        ),
    }
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
    /// Which of the plan's steps are each item's, in order (`count` of
    /// them): what a run counts by item, done or found done already
    /// ([`Tally`]). Empty where the plan's steps are not the ones they
    /// were made for (what a retry has left): then nothing is counted.
    #[serde(skip)]
    pub steps: Vec<ItemSteps>,
}

impl Items {
    /// Each item's steps among the `steps` of its plan, as a range, and
    /// what says it; `None` where they are not known for them.
    fn each(&self, steps: usize) -> Option<Vec<(std::ops::Range<usize>, &ItemSaid)>> {
        let fits = self.steps.len() == self.count
            && self
                .steps
                .windows(2)
                .all(|two| two[0].start <= two[1].start)
            && self.steps.last().is_none_or(|last| last.start <= steps);
        fits.then(|| {
            let ends = self.steps.iter().skip(1).map(|item| item.start);
            self.steps
                .iter()
                .zip(ends.chain(std::iter::once(steps)))
                .map(|(item, end)| (item.start..end, &item.said))
                .collect()
        })
    }
}

/// Where an item's steps are among its plan's ([`Items::steps`]): from
/// `start` to the next item's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemSteps {
    pub start: usize,
    pub said: ItemSaid,
}

/// What says an item, done or found done ([`Tally`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ItemSaid {
    /// Its own step, by its index among the plan's: the copy of a
    /// replace, after the trash of what it replaces.
    Step(usize),
    /// The change asked, where no step of its own says it: a folder
    /// merged into one there, said by the steps that fill it.
    Asked(Change),
}

/// What a run came to, by its plan's items (`Plan::items`, else each step
/// an item): those it did (a step of theirs applied), and those it found
/// done already (every step of theirs, `StepStatus::AlreadyDone`). What
/// its answer says ("Copied 3 items to Docs; 2 were already there", "“a”
/// was already extracted to Downloads"), never that it did what it found
/// done. Counted for a run of the user's action from its start, not for
/// what a retry had left.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Tally {
    pub done: Counted,
    pub already: Counted,
}

/// Items of a [`Tally`]: how many, and the first, as its step says it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Counted {
    pub count: usize,
    pub first: Option<Change>,
    /// The name the user knows the first one's file by, where its path
    /// says another (an item of a trash, `Progress::named`).
    pub named: Option<String>,
}

/// Serialized plan.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PlanData {
    /// English where it leaves the process; as written where it comes in.
    pub title: Words,
    /// As its title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<Words>,
    pub steps: Vec<OpData>,
    /// What becomes of a name taken where a step copies or moves: asked
    /// when applied in the window when omitted, else nothing applied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_conflict: Option<clash::Policy>,
}

/// The dry-run view of a plan: what each step will do and whether it can.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PlanDiff {
    pub title: Words,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<Words>,
    pub steps: Vec<(Change, StepStatus)>,
}

impl Plan {
    pub fn new(title: impl Into<Words>) -> Self {
        Plan {
            id: PlanId::fresh(),
            title: title.into(),
            note: None,
            steps: Vec::new(),
            by: None,
            on_conflict: None,
            items: None,
            undoes: None,
        }
    }

    /// The same plan, settling the names it finds taken as `policy` says.
    #[must_use]
    pub fn on_conflict(mut self, policy: Option<clash::Policy>) -> Self {
        self.on_conflict = policy;
        self
    }

    /// The same plan, proposed by `who` ("An agent"; a plugin from outside
    /// by its title, as written).
    #[must_use]
    pub fn proposed_by(mut self, who: impl Into<Words>) -> Self {
        self.by = Some(who.into());
        self
    }

    /// The same plan, with what the review says under its title.
    #[must_use]
    pub fn with_note(mut self, note: impl Into<Words>) -> Self {
        self.note = Some(note.into());
        self
    }

    pub fn with_steps(title: impl Into<Words>, steps: Vec<Box<dyn Operation>>) -> Self {
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
        unfold(&self.steps, &ctx)
            .map_or(std::borrow::Cow::Borrowed(&self.steps[..]), |(steps, _)| {
                steps.into()
            })
    }

    /// The same plan with its steps unfolded ([`unfold`]), when one is:
    /// then its items are the steps it was asked as, which the toast
    /// counts, each said by the first step it unfolds to, unless settling
    /// names its own.
    pub(crate) fn unfold(self, ctx: &StepContext) -> Plan {
        let Some((steps, starts)) = unfold(&self.steps, ctx) else {
            return self;
        };
        // Its items said already (what a retry has left), its steps are not
        // theirs any more.
        let items = if let Some(mut items) = self.items {
            items.steps.clear();
            Some(items)
        } else {
            let asked: Vec<Change> = self.steps.iter().map(|step| step.describe()).collect();
            let placed = asked
                .iter()
                .filter_map(Change::placed)
                .map(Path::to_path_buf);
            let each = starts
                .into_iter()
                .map(|start| ItemSteps {
                    start,
                    said: ItemSaid::Step(start),
                })
                .collect();
            asked.first().cloned().map(|first| {
                Box::new(Items {
                    placed: placed.collect(),
                    count: asked.len(),
                    first,
                    steps: each,
                })
            })
        };
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

    /// The plan as data, each step it takes listed on its own: several as
    /// one ([`Steps`]) as each of them, which `from_data` builds back.
    pub fn to_data(&self) -> PlanData {
        let mut each = Vec::with_capacity(self.steps.len());
        for step in &self.steps {
            steps::each_ref(step.as_ref(), &mut each);
        }
        PlanData {
            title: self.title.clone(),
            note: self.note.clone(),
            on_conflict: self.on_conflict,
            steps: each
                .into_iter()
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
    use super::{Change, Follow, Followed, Tenses, Through, io_words, said};
    use crate::words::Words;
    use std::path::{Path, PathBuf};

    /// A step's own words naming its file are told apart by their make, not
    /// their words: what follows them is the reason said alone, and the kind
    /// of what stopped the step is still found inside them (ADR-139, 8).
    #[test]
    fn a_step_s_reason_is_told_apart_from_its_own_words() {
        use super::{Cancelled, Couldnt, reason_after};
        let step = Change::Rename {
            from: PathBuf::from("/a/it's a.txt"),
            to: PathBuf::from("/a/b.txt"),
        };
        let own = |path: &str, words: &str, error: anyhow::Error| {
            Couldnt::new(Path::new(path), Words::as_written(words.to_owned())).because(error)
        };
        let denied =
            || anyhow::Error::new(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        let reason = |error: anyhow::Error, step: &Change| {
            reason_after(&std::sync::Arc::new(error), step).map(|why| why.english())
        };
        let failed = own(
            "/a/it's a.txt",
            "“it's a.txt” could not be renamed “b.txt”",
            denied(),
        );
        assert_eq!(
            said(&failed),
            "“it's a.txt” could not be renamed “b.txt”: permission denied"
        );
        let failed = std::sync::Arc::new(failed);
        assert_eq!(
            reason_after(&failed, &step)
                .map(|why| why.english())
                .as_deref(),
            Some("permission denied")
        );
        // Words of another file's: said whole.
        let other = own("/a/other.txt", "“other.txt” could not be read", denied());
        assert_eq!(reason(other, &step), None);
        // The step's words inside another's: they are not its first.
        let wrapped = own(
            "/a/it's a.txt",
            "“it's a.txt” could not be renamed “b.txt”",
            denied(),
        )
        .context("the folder could not be read");
        assert_eq!(reason(wrapped, &step), None);
        // Where it lands, by its name: a copy's words name its target.
        let copy = Change::Copy {
            from: PathBuf::from("/a/it's a.txt"),
            to: PathBuf::from("/b/it's a.txt"),
        };
        let placed = own(
            "/b/it's a.txt",
            "“it's a.txt” could not be placed in “b”",
            denied(),
        );
        assert_eq!(reason(placed, &copy).as_deref(), Some("permission denied"));
        // A cancelled step is still told cancelled through its words.
        let cancelled = own(
            "/a/it's a.txt",
            "“it's a.txt” could not be copied",
            anyhow::Error::new(Cancelled),
        );
        assert!(cancelled.is::<Cancelled>());
        // Its words around another step's: the outer ones are the first.
        let twice = own(
            "/a/it's a.txt",
            "“it's a.txt” could not be renamed “b.txt”",
            own("/a/other.txt", "“other.txt” could not be read", denied()),
        );
        assert_eq!(
            reason(twice, &step).as_deref(),
            Some("“other.txt” could not be read: permission denied")
        );
        // A result's error, its words put around it.
        let failed: std::io::Result<()> = Err(std::io::Error::from(std::io::ErrorKind::NotFound));
        let failed = super::CouldntExt::couldnt(failed, || {
            Couldnt::new(
                Path::new("/a/it's a.txt"),
                Words::as_written("gone".to_owned()),
            )
        })
        .unwrap_err();
        assert!(failed.is::<std::io::Error>());
        assert_eq!(
            reason(failed, &step).as_deref(),
            Some("it is not there any more")
        );
    }

    /// A transfer's title names what it takes and the folder by its name,
    /// never its whole path (ADR-135).
    #[test]
    fn a_transfer_is_titled_by_what_it_takes_and_its_folder_s_name() {
        let dest = Path::new("/home/someone/l'été 2026");
        let one = [Path::new("/elsewhere/it's a.txt")];
        assert_eq!(
            super::transfer_words(super::Carried::Copy, &one, dest).english(),
            "copy “it's a.txt” to l'été 2026"
        );
        let two = [Path::new("/a/one"), Path::new("/a/two")];
        assert_eq!(
            super::transfer_words(super::Carried::Link, &two, dest).english(),
            "link 2 items in l'été 2026"
        );
        assert_eq!(super::folder_said(Path::new("/")), "/");
    }

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
            rename.words(Some(dir)).english(),
            "rename old  name.txt -> sub/new name.txt"
        );
        // Where it leads, outside the folder, stays whole.
        let link = Change::Link {
            from: PathBuf::from("/elsewhere/target"),
            to: dir.join("link"),
        };
        assert_eq!(
            link.words(Some(dir)).english(),
            "link link -> /elsewhere/target"
        );
        // The folder itself is not written as nothing.
        let made = Change::Create {
            path: dir.to_path_buf(),
            directory: true,
            words: None,
        };
        assert_eq!(made.words(Some(dir)).english(), "mkdir /data/it's a café");
        // A step's own words are what it says.
        let placed = Change::Metadata {
            path: dir.join("a.jpg"),
            field: "position".into(),
            words: Some(Tenses {
                will: Words::as_written("place “a.jpg” at 10, 20"),
                did: Words::as_written("Placed “a.jpg” at 10, 20"),
                already: Some(Words::as_written("“a.jpg” was already at 10, 20")),
            }),
        };
        assert_eq!(placed.words(Some(dir)).english(), "place “a.jpg” at 10, 20");
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
        assert_eq!(io_words(&full).english(), "the drive is full");
        let other = std::io::Error::other("the helper stopped");
        assert_eq!(io_words(&other).english(), "the helper stopped");
        #[cfg(target_os = "linux")]
        assert_eq!(
            io_words(&std::io::Error::from_raw_os_error(71)).english(),
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
            let said = problem(name)
                .unwrap_or_else(|| panic!("{name} taken"))
                .english();
            assert!(said.contains(says), "{name}: {said}");
        }
    }
}
