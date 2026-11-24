//! Whether a plan has room where it writes (ADR-049, point 6): what its
//! steps will write (`Operation::writes`: a copy, a move to another
//! filesystem), by the volume they write to, against what each volume
//! says is free. Checked when a plan the user asked for as one action
//! begins (`Executor::execute_whole`), which asks whether to go on
//! anyway rather than refuse: a network share may say it has less than
//! it has. A drive filled meanwhile still fails the copy, in words.

use std::collections::HashMap;
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
    pub fn said(&self) -> crate::words::Words {
        use crate::words::{Name, Size};
        crate::words!(
            op_no_room = "Not enough room in { $folder }: { $needs } needed, { $free } free",
            folder = Name(name(&self.dest)),
            needs = Size(self.needs),
            free = Size(self.free),
            note = "Why an action does not go on: the drive it writes to has too little free. folder: the folder written into, by its name; needs, free: sizes."
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
    /// Each step's largest file larger than some volume keeps, and its
    /// length (`StepContext::sized_large`): `None` for most.
    pub large: Vec<Option<(PathBuf, u64)>>,
}

/// A file a plan would copy onto a volume that keeps no file so large (a
/// FAT drive's 4 GiB): refused before anything is copied, as it cannot be
/// written there whatever the user answers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TooLarge {
    /// The file, where it is read.
    pub file: PathBuf,
    /// The folder it would be copied into.
    pub dest: PathBuf,
    /// The largest file that volume keeps.
    pub most: u64,
}

impl TooLarge {
    /// What a refusal says: "“film.mkv” is too large for “STICK”: a drive
    /// formatted as FAT keeps no file of 4.3 GB or more".
    pub fn said(&self) -> crate::words::Words {
        super::builtin::too_large_for(&self.file, &self.dest, self.most)
    }
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
        ctx.take_large();
        let writes = taken.then(|| step.writes(ctx)).flatten();
        sized
            .large
            .push(writes.as_ref().and_then(|_| ctx.take_large()));
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
    /// The first file these steps would copy onto a volume that keeps no
    /// file so large, if one: `largest` saying what each folder's volume
    /// keeps (`StepContext::largest_in`, asked once a folder), only for
    /// the steps that copy one larger than some volume keeps.
    pub fn too_large(&self, largest: &dyn Fn(&Path) -> Option<u64>) -> Option<TooLarge> {
        self.large
            .iter()
            .zip(&self.writes)
            .find_map(|(large, writes)| {
                let ((file, len), (dest, _)) = (large.as_ref()?, writes.as_ref()?);
                let most = largest(dest)?;
                (*len > most).then(|| TooLarge {
                    file: file.clone(),
                    dest: dest.clone(),
                    most,
                })
            })
    }

    /// The first volume these steps would write more to than it has free,
    /// if one: volumes told apart by their device, `free` saying what one
    /// has free. A folder the plan makes itself (a merge's, a new
    /// folder's) is on the volume of the nearest folder above it that is
    /// there. Each folder is asked its device once a plan, however many
    /// steps write into it (100,000 copies into one folder: one look).
    pub fn short(&self, free: &dyn Fn(&Path) -> Option<u64>) -> Option<Short> {
        self.short_with(free, &|folder| Some(crate::fs::stat::id_of(folder)?.dev))
    }

    /// [`Sized::short`], each folder's device as `device` says.
    fn short_with(
        &self,
        free: &dyn Fn(&Path) -> Option<u64>,
        device: &dyn Fn(&Path) -> Option<u64>,
    ) -> Option<Short> {
        // The bytes written into each folder, in the order first written.
        let mut into: Vec<(&Path, u64)> = Vec::new();
        let mut at: HashMap<&Path, usize> = HashMap::new();
        for (dest, bytes) in self.writes.iter().flatten() {
            if let Some(&i) = at.get(dest.as_path()) {
                into[i].1 = into[i].1.saturating_add(*bytes);
            } else {
                at.insert(dest, into.len());
                into.push((dest, *bytes));
            }
        }
        // Each folder's device, asked once: a folder the plan makes shares
        // the look at the folders above it with its siblings.
        let mut devices: HashMap<&Path, Option<u64>> = HashMap::new();
        // (device, the first folder written into there, the folder there it
        // is on, bytes)
        let mut volumes: Vec<(u64, PathBuf, PathBuf, u64)> = Vec::new();
        for (dest, bytes) in into {
            let Some((on, dev)) = dest.ancestors().find_map(|folder| {
                let known = *devices.entry(folder).or_insert_with(|| device(folder));
                Some((folder, known?))
            }) else {
                continue;
            };
            match volumes.iter_mut().find(|(d, ..)| *d == dev) {
                Some((.., sum)) => *sum = sum.saturating_add(bytes),
                None => volumes.push((dev, dest.to_path_buf(), on.to_path_buf(), bytes)),
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

    /// Each folder written into is asked its device once a plan, however
    /// many steps write there (AGENTS.md, Scale), the folders above a
    /// folder the plan makes too, shared with its siblings.
    #[test]
    fn each_folder_written_into_is_looked_at_once() {
        let into = |dest: &str, bytes: u64| Some((PathBuf::from(dest), bytes));
        let mut writes: Vec<Option<(PathBuf, u64)>> = (0..10_000)
            .map(|_| into("/example/stick/photos", 10))
            .collect();
        writes.push(None);
        writes.extend((0..10).map(|i| into(&format!("/example/stick/new/it's {i}"), 1)));
        let sized = Sized {
            work: vec![0; writes.len()],
            large: vec![None; writes.len()],
            writes,
        };
        let asked = std::sync::Mutex::new(Vec::new());
        // Only the stick is there: what the plan makes inside it is not.
        let device = |folder: &Path| {
            asked.lock().unwrap().push(folder.to_path_buf());
            (folder == Path::new("/example/stick")).then_some(7)
        };
        let short = sized.short_with(&|_| Some(50_000), &device);
        assert_eq!(
            short,
            Some(Short {
                dest: PathBuf::from("/example/stick/photos"),
                needs: 100_010,
                free: 50_000,
            })
        );
        let mut asked = asked.into_inner().unwrap();
        let looks = asked.len();
        asked.sort();
        asked.dedup();
        assert_eq!(looks, asked.len(), "no folder asked twice: {asked:?}");
        assert_eq!(looks, 13, "{asked:?}");
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
            with(1000).unwrap().said().english(),
            "Not enough room in “dest”: 1.1\u{a0}kB needed, 1.0\u{a0}kB free"
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

    /// A step copying a file larger than the volume it goes to keeps (a
    /// FAT drive's 4 GiB) is found from what the sizing kept of it, its
    /// volume asked once; one that fits, or goes where any fits, is not.
    #[test]
    fn a_file_larger_than_its_volume_keeps_is_found_before_anything_is_copied() {
        let fat = crate::op::sys::FAT_MOST;
        let at = |dest: &str| Some((PathBuf::from(dest), 0));
        let film = |len: u64| Some((PathBuf::from("/example/films/it's a film.mkv"), len));
        let sized = Sized {
            writes: vec![
                at("/example/stick"),
                at("/example/disk"),
                at("/example/stick"),
                None,
            ],
            work: vec![0; 4],
            large: vec![film(fat), film(fat + 1), film(fat + 1), None],
        };
        let asked = std::sync::Mutex::new(Vec::new());
        let found = sized.too_large(&|dest| {
            asked.lock().unwrap().push(dest.to_path_buf());
            (dest == Path::new("/example/stick")).then_some(fat)
        });
        assert_eq!(
            found,
            Some(TooLarge {
                file: PathBuf::from("/example/films/it's a film.mkv"),
                dest: PathBuf::from("/example/stick"),
                most: fat,
            })
        );
        assert_eq!(
            *asked.lock().unwrap(),
            [
                Path::new("/example/stick"),
                Path::new("/example/disk"),
                Path::new("/example/stick")
            ],
            "only where a file larger than some volume keeps goes"
        );
        assert_eq!(sized.too_large(&|_| None), None, "volumes that keep any");
    }

    /// The sizing keeps the largest file of a step that some volume may
    /// not keep, and nothing of smaller ones: one comparison a file.
    #[test]
    fn the_sizing_keeps_a_step_s_largest_file_that_some_volume_may_not_keep() {
        let fat = crate::op::sys::FAT_MOST;
        let ctx = StepContext::detached();
        ctx.sized_large(Path::new("/example/a.bin"), fat);
        assert_eq!(ctx.take_large(), None, "kept anywhere");
        for (name, len) in [("b.bin", fat + 5), ("c.bin", fat + 9), ("d.bin", fat + 1)] {
            ctx.sized_large(&Path::new("/example").join(name), len);
        }
        assert_eq!(
            ctx.take_large(),
            Some((PathBuf::from("/example/c.bin"), fat + 9))
        );
        assert_eq!(ctx.take_large(), None, "taken");
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
