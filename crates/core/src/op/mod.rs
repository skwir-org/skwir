//! Operations, plans, and the executor.
//!
//! An operation is a mutation with an inverse. A plan is an ordered list of
//! operations referencing entries by stable identity. Nothing mutates the
//! filesystem except the executor applying a plan, which journals the
//! inverse so that everything is undoable. Applying a plan twice is a no-op:
//! each step checks whether it is already done before acting.

mod builtin;
mod entry_ref;
mod executor;

use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

pub use builtin::{
    Copy, Delete, Mkdir, Move, Rename, Restore, Rmdir, Swap, Trash, register_builtin,
};
pub use entry_ref::{EntryRef, ResolveError};
pub use executor::{ExecEvent, ExecResult, Executor, JournalEntry, Progress};

/// What a step will do, for the diff shown before apply.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", tag = "change")]
pub enum Change {
    Rename {
        from: PathBuf,
        to: PathBuf,
    },
    Move {
        from: PathBuf,
        to: PathBuf,
    },
    Copy {
        from: PathBuf,
        to: PathBuf,
    },
    Swap {
        a: PathBuf,
        b: PathBuf,
    },
    Trash {
        path: PathBuf,
    },
    Restore {
        path: PathBuf,
    },
    Delete {
        path: PathBuf,
    },
    Create {
        path: PathBuf,
        directory: bool,
    },
    Remove {
        path: PathBuf,
    },
    /// A metadata-only change (tags, attributes), naming the field.
    Metadata {
        path: PathBuf,
        field: String,
    },
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
            Change::Remove { path } => write!(f, "rmdir {}", path.display()),
            Change::Metadata { path, field } => write!(f, "set {field} on {}", path.display()),
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
    /// Dry run.
    fn check(&self) -> StepStatus;
    /// Do it. Returns the inverse, or `None` when irreversible.
    fn apply(&self) -> anyhow::Result<Option<Box<dyn Operation>>>;
    /// Clone into a box (plans are cloned into the journal).
    fn clone_box(&self) -> Box<dyn Operation>;
}

impl Clone for Box<dyn Operation> {
    fn clone(&self) -> Self {
        self.clone_box()
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
    pub fn register<O>(&self, owner: &str, kind: &'static str)
    where
        O: Operation + serde::de::DeserializeOwned + 'static,
    {
        let d: OpDeserializer = Arc::new(|v| Ok(Box::new(serde_json::from_value::<O>(v)?)));
        self.kinds.write().insert(kind, (owner.into(), d));
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
    pub steps: Vec<Box<dyn Operation>>,
}

/// Serialized plan.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PlanData {
    pub title: String,
    pub steps: Vec<OpData>,
}

/// The dry-run view of a plan: what each step will do and whether it can.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PlanDiff {
    pub title: String,
    pub steps: Vec<(Change, StepStatus)>,
}

impl Plan {
    pub fn new(title: impl Into<String>) -> Self {
        Plan {
            id: PlanId(NEXT_PLAN.fetch_add(1, Ordering::Relaxed)),
            title: title.into(),
            steps: Vec::new(),
        }
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

    pub fn len(&self) -> usize {
        self.steps.len()
    }

    /// Dry run every step.
    pub fn diff(&self) -> PlanDiff {
        PlanDiff {
            title: self.title.clone(),
            steps: self
                .steps
                .iter()
                .map(|s| (s.describe(), s.check()))
                .collect(),
        }
    }

    /// Whether every step is pending or already done (no conflicts).
    pub fn is_applicable(&self) -> bool {
        self.steps
            .iter()
            .all(|s| !matches!(s.check(), StepStatus::Conflict { .. }))
    }

    pub fn to_data(&self) -> PlanData {
        PlanData {
            title: self.title.clone(),
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
        Ok(Plan::with_steps(data.title.clone(), steps))
    }
}
