//! Several steps as one ([`Steps`]): what a step that placed several
//! things gives back as its undo (an archive merged into a folder: each
//! item it placed, put in the trash). Never a step of a plan: the executor
//! journals each as a step of its own ([`each`]), so Undo checks and takes
//! back each with its own inverse, as it does a copy's merge; a plan
//! written as data lists each ([`each_ref`], `Plan::to_data`), and no plan
//! read from data names them (`OpRegistry::build` refuses [`KIND`]): a
//! review lists every change, a dry run foresees each.

use std::path::PathBuf;

use super::{Change, OpData, Operation, StepContext, StepStatus, footprint};

/// The kind they would be said under, which no plan read from data names.
pub const KIND: &str = "steps";

/// Steps applied in order, as one. Applied as one, what applied is taken
/// back when one after fails, and their undo is theirs, in the order back.
#[derive(Clone, Debug)]
pub struct Steps(Vec<Box<dyn Operation>>);

impl Steps {
    /// `steps` as one step: `None` for none, the one itself for one.
    pub fn of(mut steps: Vec<Box<dyn Operation>>) -> Option<Box<dyn Operation>> {
        match steps.len() {
            0 => None,
            1 => steps.pop(),
            _ => Some(Box::new(Steps(steps))),
        }
    }

    pub fn steps(&self) -> &[Box<dyn Operation>] {
        &self.0
    }
}

/// [`each`] of a step borrowed: the steps `step` is, in order, put after
/// what `out` holds.
pub fn each_ref<'a>(step: &'a dyn Operation, out: &mut Vec<&'a dyn Operation>) {
    match step.as_any().and_then(|any| any.downcast_ref::<Steps>()) {
        Some(steps) => {
            for step in &steps.0 {
                each_ref(step.as_ref(), out);
            }
        }
        None => out.push(step),
    }
}

/// The steps `step` is, in order: those it holds where it is [`Steps`]
/// (theirs too, all the way down), else itself.
pub fn each(step: Box<dyn Operation>) -> Vec<Box<dyn Operation>> {
    let mut out = Vec::new();
    let mut left = vec![step];
    while let Some(step) = left.pop() {
        match step.as_any().and_then(|any| any.downcast_ref::<Steps>()) {
            Some(steps) => left.extend(steps.0.iter().rev().cloned()),
            None => out.push(step),
        }
    }
    out
}

impl Operation for Steps {
    fn kind(&self) -> &'static str {
        KIND
    }

    fn args(&self) -> serde_json::Value {
        let steps: Vec<OpData> = self
            .0
            .iter()
            .map(|step| OpData {
                kind: step.kind().to_owned(),
                args: step.args(),
            })
            .collect();
        serde_json::to_value(steps).expect("serializable")
    }

    /// As the first says: what the others do is said by each where it is
    /// journaled.
    fn describe(&self) -> Change {
        self.0[0].describe()
    }

    fn check(&self, ctx: &StepContext) -> StepStatus {
        let mut done = true;
        let mut reversible = true;
        for step in &self.0 {
            match step.check(ctx) {
                StepStatus::AlreadyDone => {}
                StepStatus::Pending { reversible: r } => {
                    done = false;
                    reversible &= r;
                }
                conflict @ StepStatus::Conflict { .. } => return conflict,
            }
        }
        if done {
            StepStatus::AlreadyDone
        } else {
            StepStatus::Pending { reversible }
        }
    }

    fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let mut back: Vec<Box<dyn Operation>> = Vec::new();
        for step in &self.0 {
            let applied = match step.check(ctx) {
                StepStatus::AlreadyDone => continue,
                StepStatus::Conflict { reason } => Err(anyhow::anyhow!(reason)),
                StepStatus::Pending { .. } => step.apply(ctx),
            };
            match applied {
                Ok(inverse) => back.extend(inverse),
                Err(error) => {
                    // What applied before it is taken back, the last first.
                    for undo in back.iter().rev() {
                        if let Err(e) = undo.apply(ctx) {
                            tracing::warn!("a step could not be taken back: {e:#}");
                        }
                    }
                    return Err(error);
                }
            }
        }
        back.reverse();
        Ok(Steps::of(back))
    }

    fn clone_box(&self) -> Box<dyn Operation> {
        Box::new(self.clone())
    }

    fn undoable(&self) -> bool {
        self.0.iter().all(|step| step.undoable())
    }

    fn held(&self) -> usize {
        self.0.iter().map(|step| step.held()).sum()
    }

    fn work(&self, ctx: &StepContext, _: Option<u64>) -> u64 {
        self.0
            .iter()
            .map(|step| step.work(ctx, step.writes(ctx).map(|(_, bytes)| bytes)))
            .fold(0, u64::saturating_add)
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn footprint(&self, volumes: &footprint::Volumes) -> Option<footprint::Footprint> {
        let mut all = footprint::Footprint::default();
        for step in &self.0 {
            all.merge(step.footprint(volumes)?);
        }
        Some(all)
    }

    fn brings_from(&self) -> Option<PathBuf> {
        self.0[0].brings_from()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::op::{Mkdir, OpRegistry, Rmdir};

    #[test]
    fn several_steps_are_one_and_each_again() {
        let tmp = tempfile::tempdir().unwrap();
        let (a, b) = (tmp.path().join("a"), tmp.path().join("it's b"));
        let made = |path: &std::path::Path| -> Box<dyn Operation> {
            Box::new(Mkdir {
                path: path.to_path_buf(),
            })
        };
        assert!(Steps::of(Vec::new()).is_none());
        assert_eq!(Steps::of(vec![made(&a)]).unwrap().kind(), "mkdir");
        let both = Steps::of(vec![made(&a), made(&b)]).unwrap();
        assert_eq!(each(both.clone()).len(), 2);

        // Written as data, each is a step of the plan, built back so; none
        // is read as several.
        let registry = OpRegistry::default();
        registry.register::<Mkdir>("test");
        registry.register::<Rmdir>("test");
        let plan = crate::op::Plan::with_steps("both", vec![made(&a), both.clone()]);
        let data = plan.to_data();
        let kinds: Vec<_> = data.steps.iter().map(|s| s.kind.as_str()).collect();
        assert_eq!(kinds, ["mkdir", "mkdir", "mkdir"]);
        let back = crate::op::Plan::from_data(&data, &registry).unwrap();
        assert_eq!(back.steps.len(), 3);
        assert!(
            registry
                .build(&OpData {
                    kind: both.kind().into(),
                    args: both.args(),
                })
                .is_err()
        );

        let rebuilt = both;
        let ctx = StepContext::detached();
        assert!(matches!(rebuilt.check(&ctx), StepStatus::Pending { .. }));
        let undo = rebuilt.apply(&ctx).unwrap().unwrap();
        assert!(a.is_dir() && b.is_dir());
        assert_eq!(rebuilt.check(&ctx), StepStatus::AlreadyDone);
        // Its undo is theirs, the last first.
        let back = each(undo.clone_box());
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].describe(), Change::Remove { path: b.clone() });
        undo.apply(&ctx).unwrap();
        assert!(!a.exists() && !b.exists());

        // One that fails takes back those applied before it.
        std::fs::write(&b, "a file").unwrap();
        let blocked = Steps::of(vec![made(&a), made(&b)]).unwrap();
        assert!(blocked.apply(&ctx).is_err());
        assert!(!a.exists(), "taken back");
    }
}
