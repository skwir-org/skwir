//! The undo of an archive extracted or made where its volume keeps no
//! trash (a network folder a program mounts, ADR-077); elsewhere the trash
//! takes it. What was made is removed for good only while it is as it was
//! made (`op::made_stays`): changed since, or with a file beside it that
//! goes with it (a sidecar written since), it stays, and Undo says so. An
//! archive places nothing beside what it makes, so nothing beside it is
//! removed. Redo makes it again. What an archive merged into a folder
//! placed is taken back together, each item as it was made, and placed
//! again together.

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

/// Take back what [`Again`] made at `entry`, as it was then (`made_as`),
/// and what it made with it (`with`).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct Unmake {
    pub entry: EntryRef,
    /// `op::made_state` once made; `None`: never removed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub made_as: Option<String>,
    /// The others it made, each as it was then: the items an archive
    /// merged into a folder placed beside the first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub with: Vec<Also>,
    pub again: Again,
}

/// One more item made, as it was then.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct Also {
    pub entry: EntryRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub made_as: Option<String>,
}

impl Also {
    fn of(made: EntryRef) -> Self {
        Also {
            made_as: op::made_state(&made.path),
            entry: made,
        }
    }
}

impl Unmake {
    /// The undo of `again`, which made `made` where its volume keeps no
    /// trash.
    pub fn of(made: EntryRef, again: Again) -> Self {
        Unmake::of_each(vec![made], again)
    }

    /// The undo of `again`, which made each of `made` (one at least) where
    /// its volume keeps no trash.
    pub fn of_each(made: Vec<EntryRef>, again: Again) -> Self {
        let mut each = made.into_iter().map(Also::of);
        let first = each.next().expect("one made at least");
        Unmake {
            entry: first.entry,
            made_as: first.made_as,
            with: each.collect(),
            again,
        }
    }

    /// Each item it takes back, as made.
    fn each(&self) -> impl Iterator<Item = (&EntryRef, Option<&str>)> {
        std::iter::once((&self.entry, self.made_as.as_deref())).chain(
            self.with
                .iter()
                .map(|also| (&also.entry, also.made_as.as_deref())),
        )
    }

    /// Where each item it takes back is now; `None` for one gone already.
    fn found(&self) -> Result<Vec<Option<std::path::PathBuf>>, String> {
        self.each()
            .map(|(entry, _)| match entry.resolve() {
                Ok(path) => Ok(Some(path)),
                Err(ResolveError::Gone(_)) => Ok(None),
                Err(e) => Err(e.to_string()),
            })
            .collect()
    }

    /// Why one of what was made, now where `found` says, stays
    /// (`op::made_stays`).
    fn stays(&self, found: &[Option<std::path::PathBuf>], ctx: &StepContext) -> Option<String> {
        self.each()
            .zip(found)
            .filter_map(|((entry, made_as), path)| Some((entry, made_as, path.as_deref()?)))
            .find_map(|(entry, made_as, path)| op::made_stays(path, &entry.path, made_as, &[], ctx))
    }
}

impl op::Kind for Unmake {
    const KIND: &'static str = "archive_unmake";
}

impl op::Effect for Unmake {
    fn footprint(&self, volumes: &op::footprint::Volumes) -> Option<op::footprint::Footprint> {
        let mut footprint = op::footprint::Footprint::default();
        let mut heavy = false;
        for (entry, _) in self.each() {
            footprint = footprint.made(&entry.path);
            heavy |= volumes.is_folder(&entry.path);
        }
        Some(footprint.heavy_if(heavy))
    }

    fn describe(&self) -> Change {
        Change::Remove {
            path: self.entry.path.clone(),
        }
    }

    fn check(&self, ctx: &StepContext) -> StepStatus {
        let found = match self.found() {
            Ok(found) => found,
            Err(reason) => return StepStatus::Conflict { reason },
        };
        if let Some(reason) = self.stays(&found, ctx) {
            return StepStatus::Conflict { reason };
        }
        if found.iter().all(Option::is_none) {
            StepStatus::AlreadyDone
        } else {
            StepStatus::Pending { reversible: true }
        }
    }

    fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let found = self.found().map_err(anyhow::Error::msg)?;
        if let Some(why) = self.stays(&found, ctx) {
            anyhow::bail!("{why}");
        }
        for ((entry, _), path) in self.each().zip(&found) {
            if let Some(path) = path {
                op::remove_made(path, &entry.path, &[], ctx)?;
            }
        }
        Ok(Some(match &self.again {
            Again::Extract(extract) => Box::new(extract.clone()),
            Again::Compress(compress) => Box::new(compress.clone()),
        }))
    }
}

pub fn register(app: &App, owner: &str) {
    app.ops.register::<Unmake>(owner);
}
