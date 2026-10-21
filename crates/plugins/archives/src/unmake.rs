//! The undo of an archive extracted or made where its volume keeps no
//! trash (a network folder a program mounts, ADR-077); elsewhere the trash
//! takes it. What was made is removed for good only while it is as it was
//! made (`op::made_stays`): changed since, it stays, and Undo says so.
//! Redo makes it again.

use skwir_core::App;
use skwir_core::op::{self, Change, EntryRef, Operation, ResolveError, StepContext, StepStatus};

use crate::compress::Compress;
use crate::extract::Extract;

/// The step that made it, which Redo runs again.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Again {
    Extract(Extract),
    Compress(Compress),
}

/// Take back what [`Again`] made at `entry`, as it was then (`made_as`).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct Unmake {
    pub entry: EntryRef,
    /// `op::made_state` once made; `None`: never removed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub made_as: Option<String>,
    pub again: Again,
}

impl Unmake {
    /// The undo of `again`, which made `made` where its volume keeps no
    /// trash.
    pub fn of(made: EntryRef, again: Again) -> Self {
        Unmake {
            made_as: op::made_state(&made.path),
            entry: made,
            again,
        }
    }
}

impl op::Kind for Unmake {
    const KIND: &'static str = "archive_unmake";
}

impl op::Effect for Unmake {
    fn footprint(&self, volumes: &op::footprint::Volumes) -> Option<op::footprint::Footprint> {
        let path = &self.entry.path;
        Some(
            op::footprint::Footprint::default()
                .made(path)
                .heavy_if(volumes.is_folder(path)),
        )
    }

    fn describe(&self) -> Change {
        Change::Remove {
            path: self.entry.path.clone(),
        }
    }

    fn check(&self, _ctx: &StepContext) -> StepStatus {
        match self.entry.resolve() {
            Ok(path) => match op::made_stays(&path, self.made_as.as_deref()) {
                Some(reason) => StepStatus::Conflict { reason },
                None => StepStatus::Pending { reversible: true },
            },
            Err(ResolveError::Gone(_)) => StepStatus::AlreadyDone,
            Err(e) => StepStatus::Conflict {
                reason: e.to_string(),
            },
        }
    }

    fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let path = self.entry.resolve()?;
        if let Some(why) = op::made_stays(&path, self.made_as.as_deref()) {
            anyhow::bail!("{why}");
        }
        op::remove_made(&path, ctx)?;
        Ok(Some(match &self.again {
            Again::Extract(extract) => Box::new(extract.clone()),
            Again::Compress(compress) => Box::new(compress.clone()),
        }))
    }
}

pub fn register(app: &App, owner: &str) {
    app.ops.register::<Unmake>(owner);
}
