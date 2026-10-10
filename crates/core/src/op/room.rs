//! Whether a plan has room where it writes (ADR-049, point 6): what its
//! steps will write (`Operation::writes`: a copy, a move to another
//! filesystem), by the volume they write to, against what each volume
//! says is free. Checked when a plan the user asked for as one action
//! begins (`Executor::execute_whole`), which asks whether to go on
//! anyway rather than refuse: a network share may say it has less than
//! it has. A drive filled meanwhile still fails the copy, in words.

use std::path::{Path, PathBuf};

use crate::cancel::CancelToken;

use super::{Operation, StepContext};

/// A volume a plan would write more to than it has free.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Short {
    /// The folder written into, the first on that volume.
    pub dest: PathBuf,
    /// What the plan writes there.
    pub needs: u64,
    /// What the volume says is free.
    pub free: u64,
}

impl Short {
    /// What a refusal says: "Not enough room in “Stick”: 12.0 GB needed,
    /// 8.0 GB free".
    pub fn said(&self) -> String {
        format!(
            "Not enough room in “{}”: {} needed, {} free",
            name(&self.dest),
            crate::fs::size_said(self.needs),
            crate::fs::size_said(self.free)
        )
    }
}

/// A folder's own name, or its path where it has none (`/`).
fn name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// The first volume `steps` would write more to than it has free, if one.
/// Each step says what it writes, counted as the copy counts it (a file of
/// several names once, in one context for the plan); volumes are told apart
/// by their device, and `free` says what one has free. Reads what would be
/// copied: on the plan's thread. `None` once cancelled.
pub fn short(
    steps: &[Box<dyn Operation>],
    ctx: &StepContext,
    free: &dyn Fn(&Path) -> Option<u64>,
    cancel: &CancelToken,
) -> Option<Short> {
    // (device, the first folder written into there, bytes)
    let mut volumes: Vec<(u64, PathBuf, u64)> = Vec::new();
    for step in steps {
        if cancel.is_cancelled() {
            return None;
        }
        let Some((dest, bytes)) = step.writes(ctx) else {
            continue;
        };
        let Some(device) = crate::fs::stat::id_of(&dest).map(|f| f.dev) else {
            continue;
        };
        match volumes.iter_mut().find(|(d, ..)| *d == device) {
            Some((_, _, sum)) => *sum = sum.saturating_add(bytes),
            None => volumes.push((device, dest, bytes)),
        }
    }
    volumes.into_iter().find_map(|(_, dest, needs)| {
        let free = free(&dest)?;
        (needs > free).then_some(Short { dest, needs, free })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::op::{Copy, EntryRef};

    fn copy(src: &Path, dest: &Path) -> Box<dyn Operation> {
        Box::new(Copy {
            entry: EntryRef::from_path(src).unwrap(),
            dest_dir: dest.to_path_buf(),
            new_name: None,
            preserve_attributes: true,
        })
    }

    /// A plan's copies onto one volume are added up against what it has
    /// free; what writes nothing needs no room.
    #[test]
    fn a_plan_is_short_of_room_where_its_copies_need_more_than_is_free() {
        let tmp = tempfile::tempdir().unwrap();
        let (src, dest) = (tmp.path().join("src"), tmp.path().join("dest"));
        std::fs::create_dir_all(src.join("folder")).unwrap();
        std::fs::create_dir(&dest).unwrap();
        std::fs::write(src.join("a.bin"), [0u8; 600]).unwrap();
        std::fs::write(src.join("folder/b.bin"), [0u8; 500]).unwrap();
        let steps = [
            copy(&src.join("a.bin"), &dest),
            copy(&src.join("folder"), &dest),
        ];
        let cancel = CancelToken::new();
        let with = |free: u64| {
            short(
                &steps,
                &StepContext::detached(),
                &move |_| Some(free),
                &cancel,
            )
        };
        assert_eq!(
            with(1000),
            Some(Short {
                dest: dest.clone(),
                needs: 1100,
                free: 1000
            })
        );
        assert_eq!(with(1100), None, "just enough");
        assert_eq!(
            short(&steps, &StepContext::detached(), &|_| None, &cancel),
            None,
            "a volume that does not say is not refused"
        );
        assert_eq!(
            with(1000).unwrap().said(),
            "Not enough room in “dest”: 1.1 KB needed, 1000 B free"
        );
    }

    /// Two names of one file copied together need its bytes once
    /// (ADR-049).
    #[cfg(unix)]
    #[test]
    fn a_file_of_two_names_needs_its_room_once() {
        let tmp = tempfile::tempdir().unwrap();
        let (src, dest) = (tmp.path().join("src"), tmp.path().join("dest"));
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir(&dest).unwrap();
        std::fs::write(src.join("a.bin"), [0u8; 700]).unwrap();
        std::fs::hard_link(src.join("a.bin"), src.join("b.bin")).unwrap();
        let steps = [
            copy(&src.join("a.bin"), &dest),
            copy(&src.join("b.bin"), &dest),
        ];
        let found = short(
            &steps,
            &StepContext::detached(),
            &|_| Some(0),
            &CancelToken::new(),
        );
        assert_eq!(found.map(|s| s.needs), Some(700));
    }
}
