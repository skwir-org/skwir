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

/// What each step of a plan writes, and where (`Operation::writes`), and
/// the bytes it counts as it goes (`Operation::work`), sized once before
/// the plan begins: what its room is checked against, and the total its
/// progress counts toward, which says how long it has left (ADR-113).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sized {
    /// Each step's folder written into, and the bytes it writes there,
    /// counted as the copy counts them; `None` for one that writes none,
    /// cannot say, or was not sized.
    pub writes: Vec<Option<(PathBuf, u64)>>,
    /// Each step's bytes as it counts them; 0 for one not sized.
    pub work: Vec<u64>,
}

/// What `steps` write and count (`Sized`), each step `only` takes sized,
/// the others left at nothing: in one context for the plan, a file of
/// several names counted once. Reads what would be copied: on the plan's
/// thread. `None` once cancelled.
pub fn sized(
    steps: &[Box<dyn Operation>],
    ctx: &StepContext,
    cancel: &CancelToken,
    only: &dyn Fn(&dyn Operation) -> bool,
) -> Option<Sized> {
    let mut sized = Sized::default();
    for step in steps {
        if cancel.is_cancelled() {
            return None;
        }
        let taken = only(step.as_ref());
        let writes = taken.then(|| step.writes(ctx)).flatten();
        let work = if taken {
            step.work(ctx, writes.as_ref().map(|(_, bytes)| *bytes))
        } else {
            0
        };
        sized.writes.push(writes);
        sized.work.push(work);
    }
    (!cancel.is_cancelled()).then_some(sized)
}

impl Sized {
    /// The first volume these steps would write more to than it has free,
    /// if one: volumes told apart by their device, `free` saying what one
    /// has free. A folder the plan makes itself (a merge's, a new
    /// folder's) is on the volume of the nearest folder above it that is
    /// there.
    pub fn short(&self, free: &dyn Fn(&Path) -> Option<u64>) -> Option<Short> {
        // (device, the first folder written into there, the folder there it
        // is on, bytes)
        let mut volumes: Vec<(u64, PathBuf, PathBuf, u64)> = Vec::new();
        for (dest, bytes) in self.writes.iter().flatten() {
            let Some((on, device)) = dest.ancestors().find_map(|folder| {
                Some((folder.to_path_buf(), crate::fs::stat::id_of(folder)?.dev))
            }) else {
                continue;
            };
            match volumes.iter_mut().find(|(d, ..)| *d == device) {
                Some((.., sum)) => *sum = sum.saturating_add(*bytes),
                None => volumes.push((device, dest.clone(), on, *bytes)),
            }
        }
        volumes.into_iter().find_map(|(_, dest, on, needs)| {
            let free = free(&on)?;
            (needs > free).then_some(Short { dest, needs, free })
        })
    }
}

/// The first volume `steps` would write more to than it has free, if one
/// (`sized`, then `Sized::short`). `None` once cancelled.
pub fn short(
    steps: &[Box<dyn Operation>],
    ctx: &StepContext,
    free: &dyn Fn(&Path) -> Option<u64>,
    cancel: &CancelToken,
) -> Option<Short> {
    sized(steps, ctx, cancel, &|_| true)?.short(free)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::op::{Copy, EntryRef};
    use std::cell::Cell;

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

    /// What a plan copies into a folder it makes itself (a merge's) needs
    /// room on the volume of the folder above it.
    #[test]
    fn a_folder_the_plan_makes_has_its_volume_s_room() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("it’s a.bin"), [0u8; 900]).unwrap();
        let made = tmp.path().join("dest/new  folder");
        std::fs::create_dir(tmp.path().join("dest")).unwrap();
        let asked = std::sync::Mutex::new(Vec::new());
        let found = short(
            &[copy(&src.join("it’s a.bin"), &made)],
            &StepContext::detached(),
            &|at| {
                asked.lock().unwrap().push(at.to_path_buf());
                Some(100)
            },
            &CancelToken::new(),
        );
        assert_eq!(found.map(|s| (s.dest, s.needs)), Some((made, 900)));
        assert_eq!(*asked.lock().unwrap(), [tmp.path().join("dest")]);
    }

    /// What each step writes and counts is kept step by step, for the run
    /// to take (ADR-113); a step left out is sized at nothing, and not read.
    #[test]
    fn each_step_is_sized_and_one_left_out_at_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let (src, dest) = (tmp.path().join("src"), tmp.path().join("dest"));
        std::fs::create_dir_all(src.join("a folder")).unwrap();
        std::fs::create_dir(&dest).unwrap();
        std::fs::write(src.join("it's a.bin"), [0u8; 300]).unwrap();
        std::fs::write(src.join("a folder/b.bin"), [0u8; 500]).unwrap();
        let steps = [
            copy(&src.join("it's a.bin"), &dest),
            copy(&src.join("a folder"), &dest),
        ];
        let ctx = StepContext::detached();
        let all = sized(&steps, &ctx, &CancelToken::new(), &|_| true).unwrap();
        assert_eq!(all.work, [300, 500]);
        assert_eq!(
            all.writes,
            [Some((dest.clone(), 300)), Some((dest.clone(), 500))]
        );
        let first = Cell::new(true);
        let one = sized(&steps, &ctx, &CancelToken::new(), &|_| first.replace(false)).unwrap();
        assert_eq!(one.work, [300, 0]);
        assert_eq!(one.writes[1], None);
        let cancel = CancelToken::new();
        cancel.cancel();
        assert_eq!(sized(&steps, &ctx, &cancel, &|_| true), None);
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
