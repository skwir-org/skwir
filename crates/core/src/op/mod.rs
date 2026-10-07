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
pub mod records;
mod sys;

use std::ffi::OsStr;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

pub use builtin::{
    Copy, Delete, InTrash, Landed, Mkdir, Move, NewFile, RemoveEmptyFile, Rename, Restore,
    RestorePart, ReturnPart, Rmdir, Swap, Trash, Unmerge, Untrashed, in_trash, parse_trash_info,
    register_builtin, trash_info_of,
};
pub use context::{Cancelled, StepContext};
pub use entry_ref::{EntryRef, ResolveError};
pub use executor::{
    ChangeObserver, ExecEvent, ExecResult, Executor, JournalEntry, Progress, RunKind, Transfer,
    Verdict,
};
pub use order::{RenameOrderError, RenameRequest, order_renames};

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
            Change::Swap { a, b } => write!(f, "swap {} <-> {}", a.display(), b.display()),
            Change::Trash { path } => write!(f, "trash {}", path.display()),
            Change::Restore { path } => write!(f, "restore {}", path.display()),
            Change::Delete { path } => write!(f, "delete {}", path.display()),
            Change::Create {
                path,
                directory: true,
            } => write!(f, "mkdir {}", path.display()),
            Change::Create {
                path,
                directory: false,
            } => write!(f, "create {}", path.display()),
            // An empty folder, or a file made empty and left so.
            Change::Remove { path } => write!(f, "remove {}", path.display()),
            Change::Metadata {
                words: Some(words), ..
            } => f.write_str(&words.will),
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
            | Change::Restore { .. }
            | Change::Create { .. }
            | Change::Metadata { .. } => Followed::Same,
        }
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
        }
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

    /// Dry run every step.
    pub fn diff(&self) -> PlanDiff {
        let ctx = StepContext::detached();
        PlanDiff {
            title: self.title.clone(),
            note: self.note.clone(),
            steps: self
                .steps
                .iter()
                .map(|s| (s.describe(), s.check(&ctx)))
                .collect(),
        }
    }

    /// Whether every step is pending or already done (no conflicts).
    pub fn is_applicable(&self) -> bool {
        let ctx = StepContext::detached();
        self.steps
            .iter()
            .all(|s| !matches!(s.check(&ctx), StepStatus::Conflict { .. }))
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
    use super::{Change, Followed};
    use std::path::{Path, PathBuf};

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
