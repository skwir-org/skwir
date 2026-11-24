//! The built-in operations: rename, move, copy, swap, trash, restore, delete,
//! mkdir, rmdir, new file and its removal, a link and its removal. Each is idempotent (`check` detects an effect already in
//! place) and returns its inverse from `apply`.
//!
//! Names are `OsString`s and paths `PathBuf`s from end to end, serialized
//! through `crate::os_path`: a name that is not UTF-8 must survive a rename
//! and its undo byte for byte.

use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::context::Kernel;
use super::pace::Pace;
use super::{
    Change, Couldnt, CouldntExt, EntryRef, OpRegistry, Operation, ResolveError, StepContext,
    StepStatus, sys, valid_file_name,
};
use crate::fs::stat::{id_of, named};
use crate::os_path::PathArg;
use crate::words;
use crate::words::{Name, Words};

fn pending(reversible: bool) -> StepStatus {
    StepStatus::Pending { reversible }
}

/// A step's conflict: why, in words.
fn conflict(reason: impl Into<Words>) -> StepStatus {
    StepStatus::Conflict {
        reason: reason.into(),
    }
}

/// A step's conflict that is an error: said link by link.
fn conflict_of(error: impl Into<anyhow::Error>) -> StepStatus {
    conflict(Words::of_error(error.into()))
}

/// `path` by its name, for a message (`display_name`).
fn name_of(path: &Path) -> Name {
    Name(display_name(path))
}

/// Why a step does not go on: its name is taken where it goes.
fn already_there(path: &Path) -> Words {
    words!(
        op_already_there = "{ $name } is already there",
        name = name_of(path),
        note = "Why a step of an action does not go on: a file of that name is where it goes."
    )
}

/// Why a step does not go on: what it goes into is no folder.
fn not_a_folder(path: &Path) -> Words {
    words!(
        op_not_a_folder = "{ $name } is not a folder",
        name = name_of(path),
        note = "Why a step of an action does not go on: what it goes into, or acts on as a folder, is a file."
    )
}

/// Why a restore does not go on: another file took the place it goes back
/// to.
fn there_again(path: &Path) -> Words {
    words!(
        op_there_again = "{ $name } is there again: rename or move it first",
        name = name_of(path),
        note = "Why a file is not put back from the trash: a file of its name is where it came from now."
    )
}

/// Why a step on the trash does not go on: the file is not there.
fn not_in_trash(path: &Path) -> Words {
    words!(
        op_not_in_trash = "{ $name } is not in the trash",
        name = name_of(path),
        note =
            "Why a step on a file of the trash does not go on: it is not in the trash (any more)."
    )
}

/// What a copy of `src` could not do, its why after it (`Couldnt`).
fn not_copied(src: &Path) -> Couldnt {
    Couldnt::new(
        src,
        words!(
            op_not_copied = "{ $name } could not be copied",
            name = name_of(src),
            note = "What a step could not do; why follows."
        ),
    )
}

/// What a restore of `path` could not do, its why after it (`Couldnt`).
fn not_restored(path: &Path) -> Couldnt {
    Couldnt::new(
        path,
        words!(
            op_not_restored = "{ $name } could not be restored",
            name = name_of(path),
            note = "What a step putting a file back from the trash could not do; why follows."
        ),
    )
}

/// Why a step on a trash's record does not go on: the file is none.
fn no_record(info: &Path) -> Words {
    words!(
        op_no_record = "{ $name } is not a trash's record of an item",
        name = name_of(info),
        note = "Why a step on the trash does not go on: the file named as what the trash keeps of an item (its .trashinfo) is not one."
    )
}

/// What could not be read of a trash's record, its why after it.
fn record_unread(info: &Path) -> Words {
    words!(
        op_record_unread = "the trash's record { $name } could not be read",
        name = name_of(info),
        note = "What a step on the trash could not do: read what the trash keeps of an item (its .trashinfo); why follows."
    )
}

/// What could not be written of a trash's record, its why after it.
fn record_unwritten(info: &Path) -> Words {
    words!(
        op_record_unwritten = "the trash's record { $name } could not be written",
        name = name_of(info),
        note = "What a step on the trash could not do: write what the trash keeps of an item (its .trashinfo); why follows."
    )
}

/// What a folder a step makes could not be, its why after it.
fn folder_not_made(path: &Path) -> Words {
    words!(
        op_folder_not_made = "the folder { $name } could not be made",
        name = name_of(path),
        note = "What a step could not do; why follows."
    )
}

/// What a folder a step makes, at `path`, could not be, its why after it.
fn folder_path_not_made(path: &Path) -> Words {
    words!(
        op_folder_path_not_made = "the folder { $path } could not be made",
        path = crate::words::Arg::Path(path.display().to_string()),
        note = "What a step could not do: make a folder, by its path; why follows."
    )
}

/// Why a folder is not made, or put back: something else has its name.
fn there_not_a_folder(path: &Path) -> Words {
    words!(
        op_there_not_a_folder = "{ $name } is there, and is not a folder",
        name = name_of(path),
        note = "Why a folder is not made, or put back: a file has its name."
    )
}

/// Why a step does not go on: the folder it puts something in is gone.
fn no_folder_to_go_in() -> Words {
    words!(
        op_no_folder_to_go_in = "the folder it goes in is not there",
        note = "Why a step of an action does not go on: the folder it makes or puts something in is gone."
    )
}

/// Why a step does not go on: what it acts on is gone.
fn no_longer_there(path: &Path) -> Words {
    words!(
        op_no_longer_there = "{ $name } is no longer there",
        name = name_of(path),
        note = "Why a step of an action does not go on: the file it acts on is gone."
    )
}

/// What removing `path` could not do, its why after it (`Couldnt`).
fn not_removed(path: &Path) -> Couldnt {
    Couldnt::new(
        path,
        words!(
            op_not_removed = "{ $name } could not be removed",
            name = name_of(path),
            note = "What a step could not do; why follows."
        ),
    )
}

/// Why a folder is not removed, or taken back: Undo could not put back
/// what it holds.
fn foreign_folder() -> Words {
    words!(
        op_foreign_folder = "this folder belongs to another user and its permissions would prevent Undo from restoring its contents",
        note = "Why a step does not remove a folder: it is another user's, and Undo could not put back what it holds."
    )
}

/// Why a folder is not moved or copied: it would go inside itself.
fn into_itself(moved: bool) -> Words {
    if moved {
        words!(
            op_moved_into_itself = "a folder cannot be moved into itself",
            note = "Why a move does not go on: the folder it goes to is inside the one moved."
        )
    } else {
        words!(
            op_copied_into_itself = "a folder cannot be copied into itself",
            note = "Why a copy does not go on: the folder it goes to is inside the one copied."
        )
    }
}

/// Why a copy does not go on: it would be made onto what it copies.
fn onto_itself() -> Words {
    words!(
        op_copied_onto_itself = "it would be copied onto itself",
        note = "Why a copy does not go on: where it goes is the file copied."
    )
}

/// Why a step does not go on: an earlier step of its plan puts something
/// where it goes.
fn there_by_earlier_step(path: &Path) -> Words {
    words!(
        op_there_by_earlier_step = "{ $name } is already there, by an earlier step",
        name = name_of(path),
        note = "Why a step of an action does not go on: a step before it in the same action puts a file of that name where it goes."
    )
}

/// Why a step on the trash does not go on: the trash could not be read
/// (`error`, said link by link where it is shown).
fn trash_unread(error: anyhow::Error) -> Words {
    words!(
        op_trash_unread = "the trash could not be read: { $error }",
        error = error,
        note = "Why a step on the trash does not go on. error: why, as the system's trash said it."
    )
}

/// What could not be read of the trash, its why after it.
fn trash_unread_here() -> Words {
    words!(
        op_trash_unread_context = "the trash could not be read",
        note = "What a step on the trash could not do; why follows."
    )
}

/// Why a restore failed: what it put back was gone as soon as it was.
fn went_missing_restored(path: &Path) -> Words {
    words!(
        op_went_missing_restored = "{ $name } went missing as it was restored",
        name = name_of(path),
        note = "Why a restore failed: the file put back was gone as soon as it was."
    )
}

/// Why a file is not put in the trash: it is there already.
fn in_trash_already(path: &Path) -> Words {
    words!(
        op_in_trash_already = "{ $name } is in the trash already",
        name = name_of(path),
        note = "Why a file is not put in the trash: it is in the trash."
    )
}

/// Why a step does not go on: the file is in the trash, where a step that
/// is no restore does not reach.
fn in_the_trash_first(path: &Path) -> Words {
    words!(
        op_in_trash_restore_first = "{ $name } is in the trash: restore it first",
        name = name_of(path),
        note = "Why a step does not act on a file: it is in the trash."
    )
}

fn exists(p: &Path) -> bool {
    std::fs::symlink_metadata(p).is_ok()
}

fn file_name(p: &Path) -> OsString {
    p.file_name().unwrap_or_default().to_os_string()
}

fn invalid_name(name: &OsStr) -> Option<StepStatus> {
    valid_file_name(name).err().map(conflict_of)
}

/// How the destination relates to the entry being transferred. A file's
/// identity alone cannot answer: two hard-link names are distinct entries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TransferTarget {
    SameEntry,
    InsideSource,
    Separate,
}

/// Resolve folder aliases, keeping the leaf itself unfollowed. In a dry
/// run a destination folder can be made by an earlier step: resolve its
/// existing ancestor, then retain the names the plan would make below it.
fn canonical_entry(path: &Path) -> std::io::Result<PathBuf> {
    fn folder(path: &Path) -> std::io::Result<PathBuf> {
        match crate::os_path::canonical(path) {
            Ok(path) => Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
                    return Err(error);
                };
                Ok(folder(parent)?.join(name))
            }
            Err(error) => Err(error),
        }
    }
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => Ok(folder(parent)?.join(name)),
        _ => crate::os_path::canonical(path),
    }
}

/// The entry's identity, preserving errors so an unavailable identity can
/// never turn an uncertain alias into an unrelated destination.
fn entry_identity(path: &Path) -> std::io::Result<crate::entry::FileId> {
    let meta = std::fs::symlink_metadata(path)?;
    crate::fs::stat::identity(path, &meta)
        .ok_or_else(|| {
            std::io::Error::other(words!(
                op_identity_unread = "the entry's identity could not be read",
                note = "Why a step does not go on: what tells a file apart from others (its device and inode) could not be read."
            ))
        })
}

/// Whether two spellings in one directory name one entry, given that both
/// resolve to the same file. Two names actually listed are distinct hard
/// links. An alternate spelling is proven only when that file has one
/// listed name; otherwise the filesystem's case rules would have to guess
/// which hard link it means, so leave the transfer refused.
fn same_spelling(source: &OsStr, target: &OsStr, names: &[OsString]) -> std::io::Result<bool> {
    if source == target {
        return Ok(true);
    }
    if names.iter().any(|name| name == source) && names.iter().any(|name| name == target) {
        return Ok(false);
    }
    if names.len() == 1 {
        return Ok(true);
    }
    Err(std::io::Error::other(words!(
        op_spellings_undistinguished =
            "the spellings of this file's hard links could not be distinguished",
        note = "Why a step does not go on: on a drive that ignores case, two names of one file could not be told apart."
    )))
}

fn same_entry(source: &Path, destination: &Path) -> std::io::Result<bool> {
    if source == destination {
        return Ok(true);
    }
    let (Some(from), Some(into), Some(source_name), Some(target_name)) = (
        source.parent(),
        destination.parent(),
        source.file_name(),
        destination.file_name(),
    ) else {
        return Ok(false);
    };
    // Directory identity also recognizes two paths through bind mounts.
    if from != into {
        let there = match entry_identity(into) {
            Ok(id) => id,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error),
        };
        if entry_identity(from)? != there {
            return Ok(false);
        }
    }
    let there = match entry_identity(destination) {
        Ok(id) => id,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    let original = entry_identity(source)?;
    if original != there {
        return Ok(false);
    }
    if source_name == target_name {
        return Ok(true);
    }
    let mut names = Vec::new();
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        if entry_identity(&entry.path())? == original {
            names.push(entry.file_name());
        }
    }
    same_spelling(source_name, target_name, &names)
}

/// What two paths name within their mounted filesystem. Bind mounts can
/// start below the source, so walking the destination's ancestors alone
/// need never meet the source directory's identity.
#[cfg(target_os = "linux")]
fn inside_mounts(source: &Path, destination: &Path, mounts: &[crate::fs::volume::Mounted]) -> bool {
    let within = |path: &Path| {
        let mount = mounts
            .iter()
            .filter(|mount| path.starts_with(&mount.point))
            .max_by_key(|mount| mount.point.as_os_str().len())?;
        Some((
            mount.device?,
            mount.root.join(path.strip_prefix(&mount.point).ok()?),
        ))
    };
    matches!((within(source), within(destination)), (Some((a, source)), Some((b, destination)))
        if a == b && destination.starts_with(&source))
}

fn inside_source(source: &Path, destination: &Path) -> std::io::Result<bool> {
    if destination.starts_with(source) {
        return Ok(true);
    }
    let source_id = entry_identity(source)?;
    for ancestor in destination.parent().into_iter().flat_map(Path::ancestors) {
        match entry_identity(ancestor) {
            Ok(id) if id == source_id => return Ok(true),
            Ok(_) => {}
            // A folder foreseen by an earlier step does not exist yet.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    #[cfg(target_os = "linux")]
    {
        let table = std::fs::read("/proc/self/mountinfo")?;
        Ok(inside_mounts(
            source,
            destination,
            &crate::fs::volume::mountinfo(&table),
        ))
    }
    // Other systems resolve folder aliases canonically and by ancestor
    // identity above; Linux's bind-root mapping is specific to mountinfo.
    #[cfg(not(target_os = "linux"))]
    Ok(false)
}

/// Compare entry locations after resolving parent aliases, never following
/// a symlink at the leaf. A folder's destination is checked against its real
/// location before collision policies, batches or staging can change anything.
pub(super) fn transfer_target(src: &Path, target: &Path) -> std::io::Result<TransferTarget> {
    let source = canonical_entry(src)?;
    let destination = canonical_entry(target)?;
    if same_entry(&source, &destination)? {
        return Ok(TransferTarget::SameEntry);
    }
    if std::fs::symlink_metadata(src)?.is_dir()
        && inside_source(&crate::os_path::canonical(src)?, &destination)?
    {
        return Ok(TransferTarget::InsideSource);
    }
    Ok(TransferTarget::Separate)
}

/// A free name in `dir` to park an entry under while it changes names.
fn parking_name(dir: &Path, purpose: &str) -> PathBuf {
    let pid = std::process::id();
    let mut n = 0u64;
    loop {
        let candidate = dir.join(format!(".skwir-{purpose}-{pid}-{n}"));
        if !exists(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// Rename `from` to `to`, never over what is there. A failure says, in the
/// user's words, what could not be done: a rename within a folder, or a
/// move to another.
fn rename_noreplace(from: &Path, to: &Path) -> anyhow::Result<()> {
    sys::rename_noreplace(from, to).couldnt(|| {
        let name = name_of(from);
        let folder = || Name(to.parent().map(display_name).unwrap_or_default());
        let words = if from.parent() == to.parent() {
            words!(
                op_not_renamed = "{ $name } could not be renamed { $new }",
                name = name,
                new = name_of(to),
                note = "What a step could not do: a file's name and the new name it was to take; why follows."
            )
        } else if from.file_name() == to.file_name() {
            words!(
                op_not_moved = "{ $name } could not be moved to { $folder }",
                name = name,
                folder = folder(),
                note = "What a step could not do: a file and the folder it was to go to, by their names; why follows."
            )
        } else {
            words!(
                op_not_moved_as = "{ $name } could not be moved to { $folder } as { $new }",
                name = name,
                folder = folder(),
                new = name_of(to),
                note = "What a step could not do: a file, the folder it was to go to and the name it was to take there; why follows."
            )
        };
        Couldnt::new(from, words)
    })
}

// ------------------------------------------------------------------ companions

/// A companion of a file (ADR-028) and where it goes with its file.
struct Along {
    from: PathBuf,
    to: PathBuf,
    /// It holds the file's attributes (a Mac's `._x`): a copy across
    /// volumes converts it instead of copying it.
    attributes: bool,
}

/// The companions of the file at `src` that go with it to `dest`: none of
/// one a batch took, which read its folder (ADR-078); each looked for
/// through the plan's context, which a dry run answers from a remote
/// folder's names.
fn along(ctx: &StepContext, src: &Path, dest: &Path) -> Vec<Along> {
    if ctx.batch_took(src) {
        return Vec::new();
    }
    let rules = ctx.companions();
    rules
        .existing_as(src, &|at| ctx.exists(at))
        .into_iter()
        .filter_map(|found| {
            Some(Along {
                to: rules.place(&found, dest)?,
                attributes: rules.holds_attributes(&found),
                from: found.path,
            })
        })
        .filter(|a| a.from != a.to)
        .collect()
}

/// The first companion whose place is taken, as the plan's earlier steps
/// leave it: the step conflicts as it does when the file's own place is.
fn along_conflict(ctx: &StepContext, along: &[Along]) -> Option<StepStatus> {
    along
        .iter()
        .find(|a| ctx.exists(&a.to))
        .map(|a| conflict(already_there(&a.to)))
}

/// Rename each companion to its place (making a companions' folder, `.ts`,
/// where needed, hidden as on Unix: `fs::hidden::create_dir_all`); on a
/// failure, put back those moved and fail.
fn rename_along(along: &[Along]) -> anyhow::Result<()> {
    for (done, a) in along.iter().enumerate() {
        let result =
            a.to.parent()
                .map_or(Ok(()), crate::fs::hidden::create_dir_all)
                .map_err(anyhow::Error::from)
                .and_then(|()| rename_noreplace(&a.from, &a.to));
        if let Err(e) = result {
            for back in &along[..done] {
                let _ = rename_noreplace(&back.to, &back.from);
            }
            return Err(e);
        }
    }
    Ok(())
}

/// The file owning `name` in `dir`, when `name` is a companion that holds
/// its attributes and the file is there: a copy carries it with the file.
fn attributes_of_sibling(ctx: &StepContext, dir: &Path, name: &OsStr) -> bool {
    let rules = ctx.companions();
    let Some(owner) = rules.owner(name) else {
        return false;
    };
    let owner = dir.join(owner);
    exists(&owner)
        && rules
            .existing(&owner)
            .iter()
            .any(|f| rules.holds_attributes(f) && f.path == dir.join(name))
}

// ------------------------------------------------------------------ rename

/// Rename within the same directory.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Rename {
    pub entry: EntryRef,
    #[serde(with = "crate::os_path")]
    #[schemars(with = "PathArg")]
    pub new_name: OsString,
}

/// How a rename relates to what is on disk at its target.
enum RenameTarget {
    /// Nothing there.
    Free,
    /// The entry itself under a spelling that differs only in case, on a
    /// case-insensitive filesystem: the rename changes the spelling.
    Respelled,
    /// The entry is already spelled as asked.
    Spelled,
    /// Something else, or another hard link to the entry.
    Taken,
}

impl Rename {
    fn target(&self) -> PathBuf {
        self.entry
            .path
            .parent()
            .unwrap_or(Path::new("/"))
            .join(&self.new_name)
    }

    fn classify(&self, src: &Path, target: &Path, ctx: &StepContext) -> RenameTarget {
        if !exists(target) {
            return RenameTarget::Free;
        }
        if id_of(target) != Some(self.entry.id) {
            return RenameTarget::Taken;
        }
        // The target stats as the entry: either the same file under another
        // spelling (case-insensitive) or a second hard link. The listing
        // tells them apart: a hard link lists both names. On a
        // case-insensitive filesystem `a.txt` and `A.txt` stat the same
        // file, and only the listing says which spelling is on disk: read
        // once for the plan's renames in the folder (`StepContext::listed`).
        let dir = target.parent().unwrap_or(Path::new("/"));
        let listed = ctx.listed(dir);
        let listed = |name: &OsStr| listed.as_ref().is_some_and(|l| l.holds(name));
        match (listed(&file_name(src)), listed(&self.new_name)) {
            (true, false) => RenameTarget::Respelled,
            (false, true) => RenameTarget::Spelled,
            _ => RenameTarget::Taken,
        }
    }
}

impl super::Kind for Rename {
    const KIND: &'static str = "rename";
}

impl super::Effect for Rename {
    fn footprint(
        &self,
        volumes: &super::footprint::Volumes,
    ) -> Option<super::footprint::Footprint> {
        Some(
            super::footprint::Footprint::default()
                .made_with(&self.entry.path, volumes)
                .made_with(&self.target(), volumes),
        )
    }
    fn describe(&self) -> Change {
        Change::Rename {
            from: self.entry.path.clone(),
            to: self.target(),
        }
    }
    fn check(&self, ctx: &StepContext) -> StepStatus {
        if let Some(invalid) = invalid_name(&self.new_name) {
            return invalid;
        }
        if let Some(trashed) = trashed(&self.entry.path) {
            return trashed;
        }
        let target = self.target();
        match ctx.resolve(&self.entry) {
            Ok(src) if src == target => StepStatus::AlreadyDone,
            // An earlier step of the plan puts something there (two files
            // given one name).
            Ok(_) if ctx.made(&target) => conflict(already_there(&target)),
            Ok(src) => match self.classify(&src, &target, ctx) {
                RenameTarget::Free | RenameTarget::Respelled => {
                    along_conflict(ctx, &along(ctx, &src, &target)).unwrap_or(pending(true))
                }
                RenameTarget::Spelled => StepStatus::AlreadyDone,
                RenameTarget::Taken => conflict(already_there(&target)),
            },
            Err(_) if id_of(&target) == Some(self.entry.id) => StepStatus::AlreadyDone,
            Err(e) => conflict_of(e),
        }
    }
    fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        valid_file_name(&self.new_name)?;
        let src = ctx.resolve(&self.entry)?;
        let target = self.target();
        let along = along(ctx, &src, &target);
        let dir = target.parent().unwrap_or(Path::new("/"));
        match self.classify(&src, &target, ctx) {
            // rename(2) between two spellings of one file is a no-op on a
            // case-insensitive filesystem: go through a parked name.
            RenameTarget::Respelled => {
                let parked = parking_name(target.parent().unwrap_or(Path::new("/")), "rename");
                let respelled = || {
                    Couldnt::new(
                        &src,
                        words!(
                            op_not_renamed = "{ $name } could not be renamed { $new }",
                            name = name_of(&src),
                            new = name_of(&target),
                            note = "What a step could not do: a file's name and the new name it was to take; why follows."
                        ),
                    )
                };
                sys::rename_noreplace(&src, &parked).couldnt(respelled)?;
                if let Err(e) = sys::rename_noreplace(&parked, &target) {
                    sys::rename_noreplace(&parked, &src).with_context(|| {
                        words!(
                            op_left_parked = "{ $name } was left named { $parked }",
                            name = name_of(&src),
                            parked = name_of(&parked),
                            note = "What a rename that changes only the case of a name left behind when it failed halfway: the file under a name of its own making; why follows."
                        )
                    })?;
                    ctx.forget_listed(dir);
                    return Err(respelled().because(e));
                }
            }
            RenameTarget::Spelled => {}
            RenameTarget::Free | RenameTarget::Taken => rename_noreplace(&src, &target)?,
        }
        if let Err(e) = rename_along(&along) {
            let _ = rename_noreplace(&target, &src);
            ctx.forget_listed(dir);
            return Err(e);
        }
        // What the plan keeps of the folder's names follows the rename; a
        // companion renamed too, it is read again when next asked.
        if along.is_empty() {
            ctx.renamed_in(dir, &file_name(&src), &self.new_name);
        } else {
            ctx.forget_listed(dir);
        }
        Ok(Some(Box::new(Rename {
            entry: EntryRef::new(self.entry.id, target).with_only_name(self.entry.only_name),
            new_name: file_name(&src),
        })))
    }
}

// ------------------------------------------------------------------ move

/// Move into another directory, keeping the name or under a new one (a
/// name taken there kept, ADR-054).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Move {
    pub entry: EntryRef,
    #[serde(with = "crate::os_path")]
    #[schemars(with = "PathArg")]
    pub dest_dir: PathBuf,
    /// Name at the destination; the source's when `None`.
    #[serde(
        default,
        with = "optional_name",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Option<PathArg>")]
    pub new_name: Option<OsString>,
}

impl Move {
    /// Move `entry` into `dest_dir` under its name.
    pub fn new(entry: EntryRef, dest_dir: PathBuf) -> Self {
        Move {
            entry,
            dest_dir,
            new_name: None,
        }
    }

    /// Where `src` goes: into the destination under its new name, else
    /// its own, or for an item of the trash the name it had
    /// (`trashed_name`).
    pub(super) fn target(&self, src: &Path) -> PathBuf {
        self.dest_dir.join(
            self.new_name
                .clone()
                .unwrap_or_else(|| trashed_name(src).unwrap_or_else(|| file_name(src))),
        )
    }
}

impl super::Kind for Move {
    const KIND: &'static str = "move";
}

impl super::Effect for Move {
    fn footprint(
        &self,
        volumes: &super::footprint::Volumes,
    ) -> Option<super::footprint::Footprint> {
        let src = &self.entry.path;
        let target = self.target(src);
        let trash = in_trash(src).is_some() || in_trash(&self.dest_dir).is_some();
        let across = !src
            .parent()
            .is_some_and(|from| volumes.same(from, &self.dest_dir));
        Some(
            super::footprint::Footprint::default()
                .made_with(src, volumes)
                .made_with(&target, volumes)
                // Kept beside a name taken there; across volumes, a copy.
                .settles_name(&target, across)
                .heavy_if(across)
                .store_if(trash, super::footprint::TRASH),
        )
    }
    fn describe(&self) -> Change {
        Change::Move {
            from: self.entry.path.clone(),
            to: self.target(&self.entry.path),
        }
    }
    /// Its bytes, when it goes to another filesystem: there a move is a
    /// copy (`move_entry`); within one it renames, writing nothing.
    fn writes(&self, ctx: &StepContext) -> Option<(PathBuf, u64)> {
        let src = ctx.resolve(&self.entry).ok()?;
        if id_of(&src)?.dev == id_of(&self.dest_dir)?.dev {
            return None;
        }
        Some((self.dest_dir.clone(), tree_size(&src, ctx).ok()?))
    }
    fn check(&self, ctx: &StepContext) -> StepStatus {
        if let Some(invalid) = self.new_name.as_deref().and_then(invalid_name) {
            return invalid;
        }
        if !ctx.is_folder(&self.dest_dir) {
            return conflict(not_a_folder(&self.dest_dir));
        }
        // A file of the trash leaves it as a restore does; within the
        // trash, only a trashed folder's files move.
        if in_trash(&self.entry.path).is_some() {
            if in_trash(&self.dest_dir).is_none() {
                return super::Effect::check(&self.restoring(&self.entry.path), ctx);
            }
            if let Some(trashed) = trashed(&self.entry.path) {
                return trashed;
            }
        }
        match ctx.resolve(&self.entry) {
            Ok(src) => {
                let target = self.target(&src);
                match transfer_target(&src, &target) {
                    Ok(TransferTarget::SameEntry) => return StepStatus::AlreadyDone,
                    Ok(TransferTarget::InsideSource) => {
                        return conflict(into_itself(true));
                    }
                    Err(error) => return conflict_of(error),
                    Ok(TransferTarget::Separate) => {}
                }
                // A batch placed its copy (ADR-078): apply removes the original.
                if ctx.placed_by_batch(&src, &target) {
                    return pending(true);
                }
                // Larger than the volume there keeps, it is on another: a
                // copy, which cannot be made.
                if let Some(refused) = too_large(&src, &self.dest_dir, ctx) {
                    return conflict(refused);
                }
                if ctx.exists(&target) {
                    return conflict(already_there(&target));
                }
                along_conflict(ctx, &along(ctx, &src, &target)).unwrap_or(pending(true))
            }
            Err(_) if id_of(&self.target(&self.entry.path)) == Some(self.entry.id) => {
                StepStatus::AlreadyDone
            }
            // Across devices the moved entry is a copy with a new identity.
            // The source being gone and its name standing in a destination
            // on another device is the most a re-run can know of it.
            Err(ResolveError::Gone(_))
                if exists(&self.target(&self.entry.path))
                    && id_of(&self.dest_dir).is_some_and(|d| d.dev != self.entry.id.dev) =>
            {
                StepStatus::AlreadyDone
            }
            Err(e) => conflict_of(e),
        }
    }
    fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        if let Some(name) = &self.new_name {
            valid_file_name(name)?;
        }
        let src = ctx.resolve(&self.entry)?;
        if in_trash(&src).is_some() && in_trash(&self.dest_dir).is_none() {
            return super::Effect::apply(&self.restoring(&src), ctx);
        }
        let target = self.target(&src);
        match transfer_target(&src, &target)? {
            TransferTarget::SameEntry => return Ok(None),
            TransferTarget::InsideSource => anyhow::bail!(into_itself(true)),
            TransferTarget::Separate => {}
        }
        let origin = src.parent().unwrap_or(Path::new("/")).to_path_buf();
        let along = along(ctx, &src, &target);
        let moved = move_entry(ctx, &src, &target, &along)?;
        // Moved to another filesystem, it is a new file, of one name.
        let (id, only) = named(&target).unwrap_or((self.entry.id, self.entry.only_name));
        let back = Move {
            entry: EntryRef::new(id, target.clone()).with_only_name(only),
            dest_dir: origin,
            // Back under the name it had.
            new_name: (target.file_name() != src.file_name()).then(|| file_name(&src)),
        };
        let Some(copied_as) = moved.waits else {
            return Ok(Some(Box::new(back)));
        };
        // Its original's removal waits, and may not come: its undo tells
        // then whether the move left a copy.
        let original =
            EntryRef::new(self.entry.id, src.clone()).with_only_name(self.entry.only_name);
        let copy = back.entry.clone();
        let copy_undo = if keeps_no_trash(&target) {
            CopyUndo::Uncopy(Uncopy {
                entry: copy,
                source: original.clone(),
                made_as: made_state(&target),
                beside: MadeBeside::now(moved.beside),
                preserve_attributes: true,
            })
        } else {
            CopyUndo::Trash(Trash { entry: copy })
        };
        Ok(Some(Box::new(Unmove {
            back,
            original,
            copied_as: copied_as.0.map(|state| state.to_hex().to_string()),
            copy_undo,
        })))
    }
}

impl Move {
    /// A file of the trash, at `src`, moved: taken out of the trash to
    /// its place in the destination, as a restore takes it to where it
    /// was (`RestorePart`: what the trash says of it goes, and comes back
    /// with it undone).
    fn restoring(&self, src: &Path) -> RestorePart {
        RestorePart {
            entry: EntryRef::new(self.entry.id, src.to_path_buf())
                .with_only_name(self.entry.only_name),
            path: self.target(src),
        }
    }
}

/// What [`move_entry`] did with the original.
struct Moved {
    /// Across volumes, its removal waits (a write-out onto a drive that
    /// can be unplugged, the plan's end after a batch), and may not come:
    /// what it was as it was copied (`state_of`).
    waits: Option<super::context::CopiedAs>,
    /// Across volumes, the companions placed beside the copy.
    beside: Vec<PathBuf>,
}

/// Move `src` to `target` with its companions (`along`): renamed, or
/// across volumes copied and then removed. A move must never lose
/// metadata, whatever the copy preference says, and a failed transfer must
/// not leave a partial target. The companion holding the file's attributes
/// is converted with them (ADR-028); the others go as files.
fn move_entry(
    ctx: &StepContext,
    src: &Path,
    target: &Path,
    along: &[Along],
) -> anyhow::Result<Moved> {
    // Copied by a batch, which read what it was first (ADR-078): on
    // another device, never renamed.
    let batched = ctx.take_batched_state(src);
    let renamed = match batched {
        Some(_) => Err(std::io::Error::from(std::io::ErrorKind::CrossesDevices).into()),
        None => rename_noreplace(src, target),
    };
    let mut moved = Moved {
        waits: None,
        beside: Vec::new(),
    };
    match renamed {
        Ok(()) => {
            if let Err(e) = rename_along(along) {
                let _ = rename_noreplace(target, src);
                return Err(e);
            }
        }
        Err(e) if is_cross_device(&e) => {
            let dest_dir = target.parent().unwrap_or(Path::new("/"));
            // The original and its companions, each with what it is as it
            // is copied.
            let originals: Vec<(PathBuf, Option<blake3::Hash>)> = std::iter::once(src)
                .chain(along.iter().map(|a| a.from.as_path()))
                .map(|p| match batched {
                    Some(copied_as) if p == src => (p.to_path_buf(), copied_as.0),
                    _ => (p.to_path_buf(), state_of(p)),
                })
                .collect();
            let made = copy_entry(src, target, dest_dir, true, along, ctx)?;
            let through = made.through;
            moved.beside = made.beside;
            let dest_dir = dest_dir.to_path_buf();
            if let Some(copied_as) = batched
                && !through
                && src
                    .parent()
                    .is_some_and(|folder| ctx.transfers().is_some_and(|t| t.remove_in(folder)))
            {
                // Copied by a batch from a remote whose mount removes its
                // originals one at a time, where a transfer removes them
                // together: with the plan's others at its end, once found
                // unchanged there, all looked at at once
                // (`transfer::remove_deferred`).
                ctx.remove_later(src.to_path_buf(), copied_as, dest_dir);
                moved.waits = Some(copied_as);
                return Ok(moved);
            }
            let copied_as = originals[0].1;
            let remove = move || {
                // Edited while it was copied, or since: the copy may not
                // hold it, so nothing of it goes. Gone is nothing to remove.
                let changed = originals.iter().find(|(path, before)| {
                    std::fs::symlink_metadata(path).is_ok() && state_of(path) != *before
                });
                if let Some((changed, _)) = changed {
                    return Err(changed_since(changed, &dest_dir));
                }
                let ((src, before), companions) =
                    originals.split_first().expect("the original first");
                // Part of the source may be gone: say that the copy is
                // whole, so that nothing looks lost.
                remove_original(src).with_context(|| OriginalStays {
                    whole: state_of(src) == *before,
                    said: words!(
                        op_original_stays = "{ $name } was copied whole to { $folder }, but its original could not all be removed",
                        name = name_of(src),
                        folder = crate::words::Arg::Path(dest_dir.display().to_string()),
                        note = "Why a move across drives ended with its original still there: the copy is whole. folder: the path of the folder it was moved to."
                    ),
                })?;
                for (companion, _) in companions {
                    let _ = remove_original(companion);
                }
                Ok(())
            };
            if through {
                // Its original goes once the copy is on the drive, names
                // and all: with the others the plan moves there, at their
                // next write-out.
                ctx.after_write_out(remove);
                moved.waits = Some(super::context::CopiedAs(copied_as));
            } else {
                remove()?;
            }
        }
        Err(e) => return Err(e),
    }
    Ok(moved)
}

/// What a move across volumes says once its copy is whole and its
/// original stays: `whole`, nothing of it removed (it changed after it was
/// copied, or none of it could be removed), or in part, the copy alone
/// holding what went. Its words are the error's, told as any.
#[derive(Debug)]
pub(super) struct OriginalStays {
    // Read by a failed move into the trash on another volume, which only
    // Linux's trash makes (`failed_across`).
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(super) whole: bool,
    said: Words,
}

impl std::fmt::Display for OriginalStays {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.said, f)
    }
}

impl OriginalStays {
    /// Whether `error` says the original stayed only in part (anyhow
    /// finds a context among an error's contexts).
    #[cfg(target_os = "linux")]
    fn in_part(error: &anyhow::Error) -> bool {
        error.downcast_ref::<Self>().is_some_and(|s| !s.whole)
    }
}

/// What a move says of its original `path`, changed after it was copied
/// into `dest_dir`: it stays, whole.
pub(super) fn changed_since(path: &Path, dest_dir: &Path) -> anyhow::Error {
    anyhow::Error::msg(OriginalStays {
        whole: true,
        said: words!(
            op_changed_after_copy = "{ $name } changed after it was copied to { $folder }: its original stays where it was",
            name = name_of(path),
            folder = crate::words::Arg::Path(dest_dir.display().to_string()),
            note = "Why a move across drives ended with its original still there: it was changed while it was copied. folder: the path of the folder it was moved to."
        ),
    })
}

/// What the tree at `path` is, links not followed: each entry's name,
/// kind, size, change and modification times, folded into one digest, so
/// a move can tell whether anything of what it copied changed before it
/// removes it. Compared with itself, never with the clock: a card's files
/// can be dated by a camera's. A mount that makes change times up gives
/// none (`Remote::makes_up_change_times`): kio-fuse's would say every file
/// changed each time it read them again. `None` where something of it
/// cannot be read, or it is gone.
pub(super) fn state_of(path: &Path) -> Option<blake3::Hash> {
    fn time(t: Option<std::time::SystemTime>) -> [u8; 17] {
        let (after, d) = match t.map(|t| t.duration_since(std::time::UNIX_EPOCH)) {
            None => (2, std::time::Duration::ZERO),
            Some(Ok(d)) => (1, d),
            Some(Err(e)) => (0, e.duration()),
        };
        let mut bytes = [after; 17];
        bytes[1..].copy_from_slice(&d.as_nanos().to_le_bytes());
        bytes
    }
    fn fold(path: &Path, real_changes: bool, digest: &mut blake3::Hasher) -> Option<()> {
        let meta = std::fs::symlink_metadata(path).ok()?;
        let kind = u8::from(meta.is_dir()) | u8::from(meta.is_symlink()) << 1;
        digest.update(&[kind]);
        digest.update(&meta.len().to_le_bytes());
        let changed = real_changes
            .then(|| crate::fs::stat::changed(path, &meta))
            .flatten();
        digest.update(&time(changed));
        digest.update(&time(meta.modified().ok()));
        if meta.is_dir() {
            let mut names = std::fs::read_dir(path)
                .ok()?
                .map(|e| e.map(|e| e.file_name()))
                .collect::<Result<Vec<_>, _>>()
                .ok()?;
            names.sort();
            for name in names {
                digest.update(name.as_encoded_bytes());
                digest.update(&[0]);
                fold(&path.join(name), real_changes, digest)?;
            }
        }
        Some(())
    }
    let real_changes =
        crate::fs::reach::of(path).is_none_or(|remote| !remote.makes_up_change_times());
    let mut digest = blake3::Hasher::new();
    fold(path, real_changes, &mut digest)?;
    Some(digest.finalize())
}

/// Remove what a move copied: gone already is removed (a folder moved
/// whole after a file of it, its removal waiting for a write-out).
pub(super) fn remove_original(path: &Path) -> anyhow::Result<()> {
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => remove_recursive(path),
    }
}

fn is_cross_device(e: &anyhow::Error) -> bool {
    e.downcast_ref::<std::io::Error>()
        .is_some_and(|e| e.kind() == std::io::ErrorKind::CrossesDevices)
}

// ------------------------------------------------------------------ copy

/// Copy into another directory, reflinking when the filesystem allows it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Copy {
    pub entry: EntryRef,
    #[serde(with = "crate::os_path")]
    #[schemars(with = "PathArg")]
    pub dest_dir: PathBuf,
    /// Name at the destination; the source name when `None`.
    #[serde(default, with = "optional_name")]
    #[schemars(with = "Option<PathArg>")]
    pub new_name: Option<OsString>,
    /// Whether the copy keeps the source's `user.*` extended attributes
    /// (tags, rating, comment, where it was downloaded from). Captured when
    /// the plan is created, so redo does not consult live settings; a plan
    /// that does not say (an agent's, a script's) keeps them.
    #[serde(default = "preserve_attributes_default")]
    pub preserve_attributes: bool,
}

fn preserve_attributes_default() -> bool {
    true
}

/// `Option<OsString>` through `crate::os_path`.
mod optional_name {
    use super::{Deserialize, Deserializer, OsString, PathArg, Serialize, Serializer};

    #[allow(clippy::ref_option, reason = "the signature serde's `with` expects")]
    pub fn serialize<S: Serializer>(name: &Option<OsString>, s: S) -> Result<S::Ok, S::Error> {
        name.as_ref().map(|n| PathArg(n.into())).serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<OsString>, D::Error> {
        Ok(Option::<PathArg>::deserialize(d)?.map(|p| p.0.into_os_string()))
    }
}

impl Copy {
    pub fn from_settings(
        entry: EntryRef,
        dest_dir: PathBuf,
        settings: &crate::settings::Settings,
    ) -> Self {
        Self {
            entry,
            dest_dir,
            new_name: None,
            preserve_attributes: settings
                .get(crate::settings::PRESERVE_ATTRIBUTES_ON_COPY)
                .unwrap_or(true),
        }
    }
    pub(super) fn target(&self, src: &Path) -> PathBuf {
        match &self.new_name {
            Some(n) => self.dest_dir.join(n),
            None => self.dest_dir.join(src.file_name().unwrap_or_default()),
        }
    }
}

impl super::Kind for Copy {
    const KIND: &'static str = "copy";
}

impl super::Effect for Copy {
    fn footprint(
        &self,
        volumes: &super::footprint::Volumes,
    ) -> Option<super::footprint::Footprint> {
        let target = self.target(&self.entry.path);
        Some(
            super::footprint::Footprint::default()
                .read_with(&self.entry.path, volumes)
                .made_with(&target, volumes)
                .settles_name(&target, true)
                .heavy_if(true),
        )
    }
    fn describe(&self) -> Change {
        Change::Copy {
            from: self.entry.path.clone(),
            to: self.target(&self.entry.path),
        }
    }
    fn writes(&self, ctx: &StepContext) -> Option<(PathBuf, u64)> {
        let src = ctx.resolve(&self.entry).ok()?;
        Some((self.dest_dir.clone(), tree_size(&src, ctx).ok()?))
    }
    fn check(&self, ctx: &StepContext) -> StepStatus {
        if let Some(invalid) = self.new_name.as_deref().and_then(invalid_name) {
            return invalid;
        }
        if !ctx.is_folder(&self.dest_dir) {
            return conflict(not_a_folder(&self.dest_dir));
        }
        let src = match ctx.resolve(&self.entry) {
            Ok(s) => s,
            Err(e) => return conflict_of(e),
        };
        let target = self.target(&src);
        match transfer_target(&src, &target) {
            Ok(TransferTarget::SameEntry) => return conflict(onto_itself()),
            Ok(TransferTarget::InsideSource) => {
                return conflict(into_itself(false));
            }
            Err(error) => return conflict_of(error),
            Ok(TransferTarget::Separate) => {}
        }
        // A batch placed it there, a moment ago, verified (ADR-078).
        if ctx.placed_by_batch(&src, &target) {
            return pending(true);
        }
        if ctx.made(&target) {
            return conflict(there_by_earlier_step(&target));
        }
        if let Some(refused) = too_large(&src, &self.dest_dir, ctx) {
            return conflict(refused);
        }
        if ctx.exists(&target) {
            // Only a finished copy of this very source is "already done": an
            // unrelated folder that happens to share the name is a conflict.
            return match same_tree(&src, &target, self.preserve_attributes, false, ctx) {
                Ok(true) => StepStatus::AlreadyDone,
                Ok(false) => conflict(already_there(&target)),
                Err(e) => conflict_of(e),
            };
        }
        along_conflict(ctx, &along(ctx, &src, &target)).unwrap_or(pending(true))
    }
    fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        if let Some(name) = &self.new_name {
            valid_file_name(name)?;
        }
        let src = ctx.resolve(&self.entry)?;
        let target = self.target(&src);
        let along = along(ctx, &src, &target);
        let made = copy_entry(
            &src,
            &target,
            &self.dest_dir,
            self.preserve_attributes,
            &along,
            ctx,
        )?;
        let r = EntryRef::from_path(&target).ok_or_else(|| {
            anyhow::Error::new(words!(
                op_copy_went_missing = "the copy { $name } went missing as it was made",
                name = name_of(&target),
                note = "Why a copy failed: what it made was gone as soon as it was made."
            ))
        })?;
        // Where its volume keeps no trash, Undo removes the copy itself,
        // and what it placed beside it, only while all is as it was made;
        // its look is the one just made, kept by the kernel a moment.
        if keeps_no_trash(&target) {
            return Ok(Some(Box::new(Uncopy {
                entry: r,
                source: EntryRef::new(self.entry.id, src).with_only_name(self.entry.only_name),
                made_as: made_state(&target),
                beside: MadeBeside::now(made.beside),
                preserve_attributes: self.preserve_attributes,
            })));
        }
        // Undo puts the copy in the trash rather than deleting it: it may
        // have been edited since, and redo then restores it as it was.
        Ok(Some(Box::new(Trash { entry: r })))
    }
}

/// Whether the volume `path` is on keeps no trash Skwir may put it in: a
/// network folder a program mounts (sshfs, GVfs, rclone), as the mount
/// table says, nothing read (ADR-077). A share the kernel mounts keeps a
/// `.Trash-<uid>` of its own.
pub fn keeps_no_trash(path: &Path) -> bool {
    crate::fs::reach::of(path).is_some_and(|remote| !remote.by_kernel())
}

/// What a step made at `path` is now (`state_of`, in hexadecimal), for
/// its undo to tell later whether it changed: where its volume keeps no
/// trash ([`keeps_no_trash`]), that undo removes it for good, only while
/// it is as it was made. `None` where it cannot all be read.
pub fn made_state(path: &Path) -> Option<String> {
    state_of(path).map(|state| state.to_hex().to_string())
}

/// A companion (ADR-028) a step placed beside what it made where its volume
/// keeps no trash, and what it was then ([`made_state`]): its undo removes
/// it with its file, only while it is so ([`made_stays`]). Only what the
/// step placed is one: a sidecar there before, or written since (an
/// editor's), is not the step's to remove.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MadeBeside {
    #[serde(with = "crate::os_path")]
    #[schemars(with = "PathArg")]
    pub path: PathBuf,
    /// `None` where it could not all be read: then nothing is removed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub made_as: Option<String>,
}

impl MadeBeside {
    /// Each of `paths`, as it is now.
    pub fn now(paths: impl IntoIterator<Item = PathBuf>) -> Vec<Self> {
        paths
            .into_iter()
            .map(|path| MadeBeside {
                made_as: made_state(&path),
                path,
            })
            .collect()
    }
}

/// Where each of `beside`, placed beside what a step made at `was_at`, is
/// for it now at `path`: renamed with it, as a companion goes with its file.
fn beside_now<'a>(
    path: &Path,
    was_at: &Path,
    beside: &'a [MadeBeside],
    rules: &crate::companions::Companions,
) -> Vec<(PathBuf, &'a MadeBeside)> {
    if path == was_at {
        return beside.iter().map(|b| (b.path.clone(), b)).collect();
    }
    let found = rules.existing_as(was_at, &|at| beside.iter().any(|b| b.path == at));
    beside
        .iter()
        .filter_map(|b| {
            let found = found.iter().find(|f| f.path == b.path)?;
            Some((rules.place(found, path)?, b))
        })
        .collect()
}

/// Why what a step made at `was_at`, `made_as` then ([`made_state`]) with
/// `beside` placed beside it, stays rather than its undo removing it from
/// `path`, where it is now: changed since (or never read whole), holding
/// where a drive or a remote is mounted, or a companion beside it that is
/// not as the step placed it: changed, or not the step's (there before it,
/// written since). The step's own and the others go together or not at
/// all: a sidecar is never left without its file.
pub fn made_stays(
    path: &Path,
    was_at: &Path,
    made_as: Option<&str>,
    beside: &[MadeBeside],
    ctx: &StepContext,
) -> Option<Words> {
    if let Some(why) = mounted_there(path) {
        return Some(why);
    }
    let now = made_state(path);
    if made_as.is_none() || now.as_deref() != made_as {
        return Some(words!(
            op_made_changed = "{ $name } changed since it was made: Undo leaves it",
            name = name_of(path),
            note = "Why Undo does not remove what an action made (where no trash is kept): it was changed since."
        ));
    }
    let rules = ctx.companions();
    let made = beside_now(path, was_at, beside, rules);
    rules
        .existing_as(path, &|at| ctx.exists(at))
        .into_iter()
        .find_map(|found| {
            let Some((at, b)) = made.iter().find(|(at, _)| *at == found.path) else {
                return Some(words!(
                    op_made_beside_other = "{ $companion } is beside { $name } and was not made with it: Undo leaves them",
                    companion = name_of(&found.path),
                    name = name_of(path),
                    note = "Why Undo does not remove what an action made (where no trash is kept): a file that goes with it (its tags, its sidecar) is there and was not made by the action."
                ));
            };
            (b.made_as.is_none() || made_state(at) != b.made_as).then(|| {
                words!(
                    op_made_companion_changed = "{ $companion } changed since it was made: Undo leaves it and { $name }",
                    companion = name_of(at),
                    name = name_of(path),
                    note = "Why Undo does not remove what an action made (where no trash is kept): a file made with it (its tags, its sidecar) was changed since."
                )
            })
        })
}

/// Remove for good what a step made at `was_at`, now at `path`, and the
/// companions it placed beside it (`beside`), none other: the undo of a
/// step that made it where its volume keeps no trash, once [`made_stays`]
/// said nothing.
///
/// # Errors
/// What could not be removed.
pub fn remove_made(
    path: &Path,
    was_at: &Path,
    beside: &[MadeBeside],
    ctx: &StepContext,
) -> anyhow::Result<()> {
    let companions = beside_now(path, was_at, beside, ctx.companions());
    remove_recursive(path).couldnt(|| {
        Couldnt::new(
            path,
            words!(
                op_not_all_removed = "{ $name } could not all be removed",
                name = name_of(path),
                note = "What a step could not do: remove a folder with all it holds; why follows."
            ),
        )
    })?;
    for (companion, _) in companions {
        if exists(&companion) {
            let _ = remove_recursive(&companion);
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ uncopy

/// Take back a copy made where its volume keeps no trash (a network folder
/// a program mounts): the undo of [`Copy`] there, which elsewhere puts the
/// copy in the trash. The copy is removed for good, with the companions
/// it placed beside it, only while all of it is as it was made
/// (`state_of`, [`made_stays`]): one changed since stays, and its step
/// says so. Redo copies its original again.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Uncopy {
    /// The copy.
    pub entry: EntryRef,
    /// What it was copied from, which Redo copies again.
    pub source: EntryRef,
    /// The copy as it was made (`state_of`, in hexadecimal); `None` where
    /// it could not all be read, and then it is never removed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub made_as: Option<String>,
    /// The companions the copy placed beside it, as they were then.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub beside: Vec<MadeBeside>,
    #[serde(default = "preserve_attributes_default")]
    pub preserve_attributes: bool,
}

impl Uncopy {
    /// Why the copy, now at `path`, stays ([`made_stays`]).
    pub(super) fn stays(&self, path: &Path, ctx: &StepContext) -> Option<Words> {
        made_stays(
            path,
            &self.entry.path,
            self.made_as.as_deref(),
            &self.beside,
            ctx,
        )
    }

    /// Where the companions it removes with the copy, now at `path`, are.
    pub(super) fn beside_at(&self, path: &Path, ctx: &StepContext) -> Vec<PathBuf> {
        beside_now(path, &self.entry.path, &self.beside, ctx.companions())
            .into_iter()
            .map(|(at, _)| at)
            .collect()
    }
}

impl super::Kind for Uncopy {
    const KIND: &'static str = "uncopy";
}

impl super::Effect for Uncopy {
    fn footprint(
        &self,
        volumes: &super::footprint::Volumes,
    ) -> Option<super::footprint::Footprint> {
        let path = &self.entry.path;
        Some(
            self.beside
                .iter()
                .fold(
                    super::footprint::Footprint::default().made(path),
                    |all, b| all.made(&b.path),
                )
                .heavy_if(volumes.is_folder(path)),
        )
    }
    fn describe(&self) -> Change {
        Change::Remove {
            path: self.entry.path.clone(),
        }
    }
    fn check(&self, ctx: &StepContext) -> StepStatus {
        match ctx.resolve(&self.entry) {
            Ok(path) if ctx.was_removed_ahead(&path) => pending(true),
            Ok(path) => self
                .stays(&path, ctx)
                .map_or_else(|| pending(true), conflict),
            Err(ResolveError::Gone(_)) => StepStatus::AlreadyDone,
            Err(e) => conflict_of(e),
        }
    }
    fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let path = ctx.resolve(&self.entry)?;
        if !ctx.take_removed_ahead(&path) {
            if let Some(why) = self.stays(&path, ctx) {
                anyhow::bail!(why);
            }
            remove_made(&path, &self.entry.path, &self.beside, ctx)?;
        }
        let (Some(dest_dir), Some(name)) = (path.parent(), path.file_name()) else {
            return Ok(None);
        };
        Ok(Some(Box::new(Copy {
            entry: self.source.clone(),
            dest_dir: dest_dir.to_path_buf(),
            new_name: (self.source.path.file_name() != Some(name)).then(|| name.to_owned()),
            preserve_attributes: self.preserve_attributes,
        })))
    }
}

// ------------------------------------------------------------------ unmove

/// Take back a move to another volume whose original's removal waited (a
/// write-out onto a drive that can be unplugged, the plan's end after a
/// batch from a remote): the undo of [`Move`] then, which a move back
/// alone would not be. That removal may never have come (the write-out
/// failed, the original changed meanwhile, it could not be removed): the
/// original stayed where it was, and the move left a copy, which this
/// takes back as a copy's undo does ([`CopyUndo`]): put in the trash, or
/// where none is kept removed for good, then only while the original is
/// whole as it was copied (else the copy may hold what the original lost:
/// both stay, and the undo says so). Where the original went, it moves the
/// entry back (`back`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Unmove {
    /// The move back: the undo where the original went.
    pub back: Move,
    /// The original, where it was.
    pub original: EntryRef,
    /// What the original was as it was copied (`state_of`, in
    /// hexadecimal); `None` where it could not all be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copied_as: Option<String>,
    /// What takes the copy back, should the original have stayed.
    pub copy_undo: CopyUndo,
}

/// What takes back a copy ([`Unmove`]): as [`Copy`]'s undo does.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CopyUndo {
    Trash(Trash),
    Uncopy(Uncopy),
}

impl Unmove {
    /// The step it takes: the copy's undo where the original stayed where
    /// it was, else the move back. The original is told there by its
    /// identity (where a mount makes numbers up, by something there);
    /// where the copy would go for good, it must be whole as it was
    /// copied too. Moved back by a batch already (ADR-078), what is there
    /// is the move back's.
    fn step(&self, ctx: &StepContext) -> Result<&dyn Operation, Words> {
        if let Ok(src) = ctx.resolve(&self.back.entry)
            && ctx.placed_by_batch(&src, &self.back.target(&src))
        {
            return Ok(&self.back);
        }
        let at = &self.original.path;
        let whole = || self.copied_as.is_some() && made_state(at) == self.copied_as;
        if !stayed(&self.original) {
            return Ok(&self.back);
        }
        match &self.copy_undo {
            CopyUndo::Trash(trash) => Ok(trash),
            CopyUndo::Uncopy(uncopy) if whole() => Ok(uncopy),
            CopyUndo::Uncopy(_) => Err(words!(
                op_move_original_changed = "{ $name } stayed where it was, changed since it was moved: Undo leaves it and its copy",
                name = name_of(at),
                note = "Why Undo does not take back a move across drives whose original could not be removed: the original was changed since."
            )),
        }
    }
}

/// Whether the original of a move across volumes whose removal waited
/// stayed where it was: the same file there, told by its identity; where
/// a mount makes numbers up, anything there, which is all it says.
fn stayed(original: &EntryRef) -> bool {
    let at = &original.path;
    match crate::fs::stat::id_of(at) {
        None => false,
        Some(id) if id.lasts() && original.id.lasts() && !keeps_no_trash(at) => id == original.id,
        Some(_) => true,
    }
}

impl super::Kind for Unmove {
    const KIND: &'static str = "unmove";
}

impl super::Effect for Unmove {
    fn footprint(
        &self,
        volumes: &super::footprint::Volumes,
    ) -> Option<super::footprint::Footprint> {
        let undo: &dyn Operation = match &self.copy_undo {
            CopyUndo::Trash(trash) => trash,
            CopyUndo::Uncopy(uncopy) => uncopy,
        };
        let mut both = super::Effect::footprint(&self.back, volumes)?;
        both.merge(undo.footprint(volumes)?);
        Some(both.read(&self.original.path))
    }
    fn describe(&self) -> Change {
        super::Effect::describe(&self.back)
    }
    fn writes(&self, ctx: &StepContext) -> Option<(PathBuf, u64)> {
        super::Effect::writes(&self.back, ctx)
    }
    fn check(&self, ctx: &StepContext) -> StepStatus {
        match self.step(ctx) {
            Ok(step) => step.check(ctx),
            Err(why) => conflict(why),
        }
    }
    fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        self.step(ctx).map_err(anyhow::Error::new)?.apply(ctx)
    }
}

/// Whether an existing destination already holds the source, including
/// what travels beside it. Size and time reject obvious differences; the
/// bytes are read where [`proves_bytes`] says they must be, the only proof
/// of content equality for unrelated files.
///
/// A plan asks it as it settles names, in its dry run and as it runs: its
/// bytes are read once a plan while nothing of either tree, nor of what
/// goes with them, changed (`StepContext::compared`, by `compared_state`);
/// a folder found different keeps what it found of its items, which a
/// merge asks of next, and each tree's state is read once a comparison,
/// or once a settling of the plan's names (`state_in`,
/// `StepContext::keeping_states`).
pub(super) fn same_tree(
    src: &Path,
    copy: &Path,
    preserve_attributes: bool,
    moving: bool,
    ctx: &StepContext,
) -> anyhow::Result<bool> {
    let along = along(ctx, src, copy);
    let bytes = proves_bytes(src, copy, moving);
    // Read before they are compared: a change meanwhile is a change since.
    let state = compared_state(ctx, src, copy, &along, bytes);
    let key = (src.to_path_buf(), copy.to_path_buf(), preserve_attributes);
    if let Some(state) = state
        && let Some(same) = ctx.compared().found(&key, state)
    {
        ctx.compared_ended();
        return Ok(same);
    }
    let same = same_trees(src, copy, preserve_attributes, bytes, &along, ctx);
    ctx.compared_ended();
    if let (Some(state), Ok(same)) = (state, &same) {
        ctx.compared().note(key, state, *same);
    }
    same
}

/// Whether "already there" must be proven by reading the bytes of both
/// sides, or is settled by kind, permissions, attributes, size and time
/// (`finished_as`). Proven for a move, whose verdict puts the original
/// in the trash, and for a copy between this machine's own folders.
/// Not for a copy from or to a remote (`fs::reach`): proving it would
/// bring every byte of both sides across the network, for a verdict that
/// only leaves a copy undone and removes nothing.
fn proves_bytes(src: &Path, copy: &Path, moving: bool) -> bool {
    moving || (crate::fs::reach::of(src).is_none() && crate::fs::reach::of(copy).is_none())
}

/// What `src` and `copy` are, and each companion that goes with `src` and
/// its place beside `copy` (`state_in`, whether there or not), with
/// whether bytes are proven, in one digest: `None` where something of
/// them cannot be read.
fn compared_state(
    ctx: &StepContext,
    src: &Path,
    copy: &Path,
    along: &[Along],
    bytes: bool,
) -> Option<blake3::Hash> {
    let besides = along
        .iter()
        .flat_map(|a| [&a.from, &a.to])
        .map(|path| state_in(ctx, path));
    Some(pair_state(
        state_in(ctx, src)?,
        state_in(ctx, copy)?,
        besides,
        bytes,
    ))
}

/// The state a comparison of two trees is kept under (`Compared`): theirs,
/// their companions' (`None` for one not there), and whether bytes were
/// proven.
fn pair_state(
    src: blake3::Hash,
    copy: blake3::Hash,
    besides: impl IntoIterator<Item = Option<blake3::Hash>>,
    bytes: bool,
) -> blake3::Hash {
    let mut digest = blake3::Hasher::new();
    digest.update(&[u8::from(bytes)]);
    digest.update(src.as_bytes());
    digest.update(copy.as_bytes());
    for state in besides {
        match state {
            Some(state) => digest.update(state.as_bytes()),
            None => digest.update(&[0]),
        };
    }
    digest.finalize()
}

/// How many entries `state_in` stat-ed, for a test to count.
#[cfg(test)]
static STATED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// What the tree at `path` is, links not followed, as [`state_of`] tells
/// it (each entry's name, kind, size, change and modification times), in
/// a digest of its own made from its folders' up: each folder's state read
/// whole is kept for the comparison, or the settling of a plan's names
/// (`StepContext::keeping_states`), so the trees a merge compares level by
/// level are read once, not once a level. A file is
/// stat-ed when asked. `None` where something of it cannot be read, it
/// is gone, or the plan was cancelled.
fn state_in(ctx: &StepContext, path: &Path) -> Option<blake3::Hash> {
    if let Some(known) = ctx.state_found(path) {
        return Some(known);
    }
    if ctx.cancel_token().is_cancelled() {
        return None;
    }
    #[cfg(test)]
    STATED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let meta = std::fs::symlink_metadata(path).ok()?;
    node_state(ctx, path, &meta)
}

/// [`state_in`] of the entry at `path`, whose metadata is `meta`: a
/// folder's from its entries', kept (`state_in`); another's from `meta`.
fn node_state(ctx: &StepContext, path: &Path, meta: &std::fs::Metadata) -> Option<blake3::Hash> {
    if meta.is_dir()
        && let Some(known) = ctx.state_found(path)
    {
        return Some(known);
    }
    let real_changes =
        crate::fs::reach::of(path).is_none_or(|remote| !remote.makes_up_change_times());
    let mut digest = blake3::Hasher::new();
    let kind = u8::from(meta.is_dir()) | u8::from(meta.is_symlink()) << 1;
    digest.update(&[kind]);
    digest.update(&meta.len().to_le_bytes());
    for time in [
        real_changes
            .then(|| crate::fs::stat::changed(path, meta))
            .flatten(),
        meta.modified().ok(),
    ] {
        let (after, since) = match time.map(|t| t.duration_since(std::time::UNIX_EPOCH)) {
            None => (2u8, std::time::Duration::ZERO),
            Some(Ok(d)) => (1, d),
            Some(Err(e)) => (0, e.duration()),
        };
        digest.update(&[after]);
        digest.update(&since.as_nanos().to_le_bytes());
    }
    if !meta.is_dir() {
        return Some(digest.finalize());
    }
    let state = (|| {
        let mut names = std::fs::read_dir(path)
            .ok()?
            .map(|e| e.map(|e| e.file_name()))
            .collect::<Result<Vec<_>, _>>()
            .ok()?;
        names.sort();
        for name in names {
            digest.update(name.as_encoded_bytes());
            digest.update(&[0]);
            digest.update(state_in(ctx, &path.join(name))?.as_bytes());
        }
        Some(digest.finalize())
    })();
    if let Some(state) = state {
        ctx.state_seen(path, state);
    }
    state
}

/// [`same_tree`], read.
fn same_trees(
    src: &Path,
    copy: &Path,
    preserve_attributes: bool,
    bytes: bool,
    along: &[Along],
    ctx: &StepContext,
) -> anyhow::Result<bool> {
    if !same_node(src, copy, preserve_attributes, bytes, ctx)?.0 {
        return Ok(false);
    }
    for companion in along {
        // Attribute containers are compared through their effective attributes,
        // as a copy can turn a container into native attributes or vice versa.
        if companion.attributes {
            continue;
        }
        if !exists(&companion.to) || !same_node(&companion.from, &companion.to, true, bytes, ctx)?.0
        {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Whether `copy` is `src` as a copy keeps it, bytes read where `bytes`
/// says so ([`proves_bytes`]), and the two as they were stat-ed for it. A
/// folder found different keeps what it found of each item compared
/// (`Compared`, under the items' states): a merge asks of those next.
fn same_node(
    src: &Path,
    copy: &Path,
    preserve_attributes: bool,
    bytes: bool,
    ctx: &StepContext,
) -> anyhow::Result<(bool, [std::fs::Metadata; 2])> {
    ctx.check_cancelled()?;
    let (a, b) = (
        std::fs::symlink_metadata(src)?,
        std::fs::symlink_metadata(copy)?,
    );
    let same = same_entries(src, copy, (&a, &b), preserve_attributes, bytes, ctx)?;
    Ok((same, [a, b]))
}

/// [`same_node`] of `src` and `copy`, stat-ed as `a` and `b`.
fn same_entries(
    src: &Path,
    copy: &Path,
    (a, b): (&std::fs::Metadata, &std::fs::Metadata),
    preserve_attributes: bool,
    bytes: bool,
    ctx: &StepContext,
) -> anyhow::Result<bool> {
    if a.file_type() != b.file_type() {
        return Ok(false);
    }
    if a.is_symlink() {
        return Ok(std::fs::read_link(src)? == std::fs::read_link(copy)?);
    }
    // A volume may not keep modes. Without proof that the difference is
    // harmless, leave this to the normal conflict policy, never discard it.
    if a.permissions() != b.permissions() {
        return Ok(false);
    }
    if preserve_attributes && !same_attributes(src, copy, ctx)? {
        return Ok(false);
    }
    if a.is_file() {
        return Ok(finished_as(b, a) && (!bytes || same_bytes(src, copy, a.len(), ctx)?));
    }
    if !a.is_dir() {
        // Never open a pipe, socket or device to compare it.
        return Ok(false);
    }
    // A companion holding its sibling's attributes may have become them.
    let names = |dir: &Path| -> std::io::Result<Vec<OsString>> {
        let mut names = std::fs::read_dir(dir)?
            .map(|e| e.map(|e| e.file_name()))
            .filter(|n| {
                !preserve_attributes
                    || !n.as_ref().is_ok_and(|n| attributes_of_sibling(ctx, dir, n))
            })
            .collect::<std::io::Result<Vec<_>>>()?;
        names.sort_unstable();
        Ok(names)
    };
    let children = names(src)?;
    if children != names(copy)? {
        return Ok(false);
    }
    let mut found = Vec::new();
    for name in children {
        let (from, to) = (src.join(&name), copy.join(&name));
        let (same, stated) = same_node(&from, &to, preserve_attributes, bytes, ctx)?;
        found.push((from, to, same, stated));
        if same {
            continue;
        }
        // Different: each item compared is kept for the merge that asks
        // of them next, as `same_tree` keeps it (with no companion: in
        // the folder, they were compared as its items).
        for (from, to, same, [a, b]) in found {
            let state = node_state(ctx, &from, &a)
                .zip(node_state(ctx, &to, &b))
                .map(|(a, b)| pair_state(a, b, [], bytes));
            if let Some(state) = state {
                ctx.compared()
                    .note((from, to, preserve_attributes), state, same);
            }
        }
        return Ok(false);
    }
    Ok(true)
}

/// Compare the attributes a copy transfers, through the same native and
/// companion seams as `transfer_attributes`. Native attributes take precedence.
/// An unreadable container is unknown, never evidence of an empty set.
fn same_attributes(src: &Path, copy: &Path, ctx: &StepContext) -> anyhow::Result<bool> {
    let attributes = |path: &Path| -> anyhow::Result<Option<Vec<_>>> {
        let mut all = kept_attributes(path)?;
        let rules = ctx.companions();
        if !rules.existing_holding_attributes(path).is_empty() {
            let Some(beside) = rules.attributes(path) else {
                return Ok(None);
            };
            all = merge_attributes(
                all,
                beside
                    .into_iter()
                    .map(|(name, value)| (OsString::from(name), value))
                    .collect(),
            );
        }
        Ok(Some(all))
    };
    let (Some(source), Some(there)) = (attributes(src)?, attributes(copy)?) else {
        return Ok(false);
    };
    Ok(source.iter().all(|attribute| there.contains(attribute)))
}

/// How many files `same_bytes` compared, for a test to count.
#[cfg(test)]
static BYTES_COMPARED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Read a bounded amount at a time, checking cancellation between reads.
/// Reading exactly the expected length plus EOF also refuses truncated or
/// growing files; a mount returning EOF early is not evidence of equality.
fn same_bytes(src: &Path, copy: &Path, len: u64, ctx: &StepContext) -> anyhow::Result<bool> {
    #[cfg(test)]
    BYTES_COMPARED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let before = (state_of(src), state_of(copy));
    if before.0.is_none() || before.1.is_none() {
        return Ok(false);
    }
    let mut a = crate::fs::file::open_regular_unfollowed(src)?;
    let mut b = crate::fs::file::open_regular_unfollowed(copy)?;
    let mut left = len;
    let mut source = vec![0; 64 << 10];
    let mut there = vec![0; source.len()];
    while left > 0 {
        ctx.check_cancelled()?;
        let n = left.min(source.len() as u64) as usize;
        a.read_exact(&mut source[..n])?;
        ctx.check_cancelled()?;
        b.read_exact(&mut there[..n])?;
        if source[..n] != there[..n] {
            return Ok(false);
        }
        left -= n as u64;
    }
    ctx.check_cancelled()?;
    Ok(a.read(&mut source[..1])? == 0
        && b.read(&mut there[..1])? == 0
        && before == (state_of(src), state_of(copy)))
}

/// Copy `src` to `target`, and its companions (`along`) beside it. On a
/// drive the user can unplug (`fs::volume::is_removable`) the copy is
/// written through (`pace::Pace`), and this says so: progress and Cancel
/// follow the drive however slow it is, and the plan ends once the drive
/// has everything, names included (`StepContext::write_out`). Written only
/// when unmounted, it could be unplugged, or reset itself (a device taking
/// its firmware), before the writes end, and lose them.
fn copy_entry(
    src: &Path,
    target: &Path,
    dest_dir: &Path,
    preserve_attributes: bool,
    along: &[Along],
    ctx: &StepContext,
) -> anyhow::Result<CopyMade> {
    match transfer_target(src, target)? {
        TransferTarget::SameEntry => anyhow::bail!(onto_itself()),
        TransferTarget::InsideSource => anyhow::bail!(into_itself(false)),
        TransferTarget::Separate => {}
    }
    let removable = sys::written_through(dest_dir);
    let pace = if removable {
        ctx.pace_through(dest_dir)?
    } else {
        std::rc::Rc::new(Pace::at_once())
    };
    // Written out before the plan ends, as onto a drive: onto one the user
    // can unplug, and onto a mount that sends to its server only after a
    // file is closed (ADR-102), told what it is to send.
    let sends_later = !removable && ctx.sends_later(dest_dir);
    let beside = copy_staged(
        src,
        target,
        dest_dir,
        preserve_attributes,
        along,
        &pace,
        ctx,
    )?;
    if sends_later {
        ctx.written_later(
            dest_dir,
            std::iter::once(target.to_path_buf()).chain(beside.iter().cloned()),
        );
    }
    Ok(CopyMade {
        through: removable || sends_later,
        beside,
    })
}

/// What [`copy_entry`] did.
struct CopyMade {
    /// It wrote through to a drive the user can unplug (`op::pace`).
    through: bool,
    /// The companions it placed beside the copy, and none other: the
    /// undo of a copy where no trash is kept removes those alone.
    beside: Vec<PathBuf>,
}

/// Copy `src` to `target` through a private staging directory inside
/// `dest_dir`, publishing with a no-replace rename only after content and
/// attributes are complete. A failed attribute write must not leave a partial destination
/// that `check()` would call already done. Shared by Copy and cross-device Move.
///
/// Its companions (`along`; the ones not holding attributes, which are
/// converted with the file's) are staged with it and placed after it: one
/// that cannot be copied or placed takes the copy back out, so a copy is
/// never left without what goes with it, and without an Undo. What it
/// placed beside the copy is returned: a batch places none (it takes no
/// file with a companion, ADR-078).
fn copy_staged(
    src: &Path,
    target: &Path,
    dest_dir: &Path,
    preserve_attributes: bool,
    along_src: &[Along],
    pace: &Pace,
    ctx: &StepContext,
) -> anyhow::Result<Vec<PathBuf>> {
    // A batch made it already, or counted it (ADR-078). Sized before the
    // plan began, its tree is not walked again (ADR-113).
    let batched = ctx.take_batched(src);
    if batched.is_none() {
        let size = match ctx.take_walked() {
            Some(size) => size,
            None => tree_size(src, ctx)?,
        };
        ctx.add_total(size);
    }
    // A destination that moves nothing between its folders (a camera)
    // takes the copy in place, never staged and moved (ADR-083).
    if crate::fs::reach::writes_in_place(dest_dir) {
        if let Some(super::transfer::Batched::Placed(at, _)) = &batched
            && at == target
        {
            return Ok(Vec::new());
        }
        return copy_in_place(
            src,
            target,
            dest_dir,
            preserve_attributes,
            along_src,
            pace,
            ctx,
        );
    }
    // The plan's staging folder there, shared by its copies, each under a
    // name of its own.
    let (staging, name) = ctx.staging_for(dest_dir)?;
    ctx.copy_begins();
    let mark = ctx.copied_mark();
    let (staged, made) = match batched {
        // At its target already: nothing to place.
        Some(super::transfer::Batched::Placed(at, _)) if at == target => return Ok(Vec::new()),
        Some(super::transfer::Batched::Made(made)) => (made, true),
        // Cut short by the run this one retries: it goes on from there.
        _ => (
            ctx.go_on_from(src, dest_dir)
                .unwrap_or_else(|| staging.path().join(&name)),
            false,
        ),
    };
    let mut staged_along = Vec::new();
    // What the copy put beside the staged entry (the companion that holds
    // its attributes, on a volume that refuses them) follows it out.
    let copied = if made {
        Ok(())
    } else {
        let copied = copy_recursive_with_attributes(src, &staged, preserve_attributes, pace, ctx);
        ctx.gone_on();
        copied
    };
    let result = copied.and_then(|()| {
        let mut beside = along(ctx, &staged, target);
        for (n, a) in along_src.iter().filter(|a| !a.attributes).enumerate() {
            let at = staging.path().join(format!("{name}-along-{n}"));
            staged_along.push(at.clone());
            copy_recursive_with_attributes(&a.from, &at, true, pace, ctx)?;
            beside.push(Along {
                from: at,
                to: a.to.clone(),
                attributes: false,
            });
        }
        sys::rename_noreplace(&staged, target).couldnt(|| not_placed(target, dest_dir))?;
        ctx.placed(&staged, target, mark);
        // The companions placed, or the copy taken back to be removed
        // from the staging folder.
        rename_along(&beside).inspect_err(|_| {
            let _ = sys::rename_noreplace(target, &staged);
        })?;
        Ok(beside.into_iter().map(|a| a.to).collect())
    });
    let error = match result {
        Ok(placed) => return Ok(placed),
        Err(error) => error,
    };
    // What this copy staged goes; the folder stays for the plan's other
    // copies, and goes with the plan. A large file a failure cut short
    // (not Cancel: a server gone, a drive pulled out) stays for Retry to go
    // on from, and the folder with it.
    let kept = !error.is::<super::Cancelled>() && ctx.keep_partial(src, dest_dir, &staged);
    let made = along(ctx, &staged, target)
        .into_iter()
        .map(|a| a.from)
        .chain((!kept).then_some(staged))
        .chain(staged_along);
    for path in made {
        if std::fs::symlink_metadata(&path).is_err() {
            continue;
        }
        if let Err(cleanup) = super::staging::remove_tree(&path) {
            return Err(error.context(part_left(&path, &cleanup)));
        }
    }
    Err(error)
}

/// What a copy could not place at `target` in `dest_dir`, its why after
/// it (`Couldnt`).
fn not_placed(target: &Path, dest_dir: &Path) -> Couldnt {
    Couldnt::new(
        target,
        words!(
            op_not_placed = "{ $name } could not be placed in { $folder }",
            name = name_of(target),
            folder = name_of(dest_dir),
            note = "What a step could not do: put a copy made in its place; why follows."
        ),
    )
}

/// Why `file` is not copied into `dest`, whose volume keeps no file of
/// more than `most` bytes (`sys::largest_file`: a FAT drive's): said alone,
/// as a check or the plan's sizing refuses it before anything is copied.
pub(super) fn too_large_for(file: &Path, dest: &Path, most: u64) -> Words {
    words!(
        op_too_large_for_fat = "{ $name } is too large for { $folder }: a drive formatted as FAT keeps no file of { $size } or more",
        name = name_of(file),
        folder = name_of(dest),
        size = crate::words::Size(most.saturating_add(1)),
        note = "Why a copy is refused before it begins: the drive it goes to is formatted as FAT (FAT32, as USB sticks and memory cards often are), which keeps no file of 4 GiB or more. name: the file; folder: where it would go; size: 4 GiB, in the user's units (4.3 GB)."
    )
}

/// Why a file was not copied where its volume keeps no file of more than
/// `most` bytes: said after its name, as a copy's error is.
fn too_large_there(most: u64) -> Words {
    words!(
        op_too_large_fat = "a drive formatted as FAT keeps no file of { $size } or more",
        size = crate::words::Size(most.saturating_add(1)),
        note = "Why a copy failed, said after the file's name: the drive it went to is formatted as FAT (FAT32, as USB sticks and memory cards often are), which keeps no file of 4 GiB or more. size: 4 GiB, in the user's units (4.3 GB). Begins in lower case."
    )
}

/// Why `src` cannot be copied into `dest_dir`: a file larger than the
/// volume there keeps (a FAT drive's 4 GiB). The volume is asked once a
/// plan (`StepContext::largest_in`), and the source looked at only where
/// it keeps less than any file asks.
fn too_large(src: &Path, dest_dir: &Path, ctx: &StepContext) -> Option<Words> {
    let most = ctx.largest_in(dest_dir)?;
    let meta = std::fs::symlink_metadata(src).ok()?;
    (meta.is_file() && meta.len() > most).then(|| too_large_for(src, dest_dir, most))
}

/// What a copy that failed left at `path`, which could not be removed
/// (`cleanup`).
fn part_left(path: &Path, cleanup: &std::io::Error) -> Words {
    words!(
        op_part_left = "a part of the copy is left in { $path } ({ $why })",
        path = crate::words::Arg::Path(path.display().to_string()),
        why = super::io_words(cleanup),
        note = "Said after why a copy failed: what it had made could not all be removed. path: where it is; why: why it stayed."
    )
}

/// [`copy_staged`] where `dest_dir` moves nothing between its folders (a
/// camera, ADR-083): the copy written at `target` itself, never over what
/// is there (each file and folder made new), its companions beside it,
/// all taken back where it fails: a retry makes it again. What it placed
/// beside the copy is returned.
fn copy_in_place(
    src: &Path,
    target: &Path,
    dest_dir: &Path,
    preserve_attributes: bool,
    along_src: &[Along],
    pace: &Pace,
    ctx: &StepContext,
) -> anyhow::Result<Vec<PathBuf>> {
    let placed_in = || not_placed(target, dest_dir);
    if std::fs::symlink_metadata(target).is_ok() {
        return Err(placed_in().because(std::io::Error::from(std::io::ErrorKind::AlreadyExists)));
    }
    // The companion that holds attributes, written beside the copy where
    // the volume keeps none: one there before it (left on the card by
    // another) is never the copy's.
    let holding = || {
        ctx.companions()
            .existing_holding_attributes(target)
            .into_iter()
            .map(|found| found.path)
    };
    let before: Vec<PathBuf> = holding().collect();
    ctx.copy_begins();
    let mut made = vec![target.to_path_buf()];
    let result = copy_recursive_with_attributes(src, target, preserve_attributes, pace, ctx)
        .and_then(|()| {
            for a in along_src.iter().filter(|a| !a.attributes) {
                anyhow::ensure!(
                    std::fs::symlink_metadata(&a.to).is_err(),
                    already_there(&a.to)
                );
                made.push(a.to.clone());
                copy_recursive_with_attributes(&a.from, &a.to, true, pace, ctx)?;
            }
            Ok(())
        });
    made.extend(holding().filter(|at| !before.contains(at)));
    let Err(error) = result else {
        return Ok(made.split_off(1));
    };
    // What it made goes, and nothing else.
    for path in made {
        if std::fs::symlink_metadata(&path).is_err() {
            continue;
        }
        if let Err(cleanup) = super::staging::remove_tree(&path) {
            return Err(error.context(part_left(&path, &cleanup)));
        }
    }
    Err(error)
}

/// Bytes of regular files under `path`, so progress has a total: a file
/// of several names once in the plan, as it is copied once.
fn tree_size(path: &Path, ctx: &StepContext) -> anyhow::Result<u64> {
    ctx.check_cancelled()?;
    let m = std::fs::symlink_metadata(path)?;
    if !m.is_dir() {
        return Ok(if m.is_file() && ctx.sized_once(&m) {
            ctx.sized_large(path, m.len());
            m.len()
        } else {
            0
        });
    }
    let mut total = 0;
    for e in std::fs::read_dir(path)? {
        total += tree_size(&e?.path(), ctx)?;
    }
    Ok(total)
}

/// Bytes per `copy_file_range` call at most: large enough to stay in the
/// kernel's fast path on a fast disk.
const COPY_CHUNK: usize = 8 << 20;

/// The bytes a copy asks for at once (`copy_range`, a read): about a
/// quarter of a second's worth, so that its progress, its speed and Cancel
/// come that often from a slow server or onto a slow drive, and at most
/// `most` on a fast disk (ADR-113). It starts at a mebibyte, doubles while
/// a call takes under a tenth of a second, halves while one takes over
/// four tenths, never under `CHUNK_LEAST`. A batch's own copy asks so too
/// (`transfer::copy_one`).
pub(super) struct Chunk {
    pub(super) size: usize,
    most: usize,
}

/// The least a copy asks for at once: a slow server still sends a few
/// round trips' worth.
const CHUNK_LEAST: usize = 256 << 10;

impl Chunk {
    pub(super) fn new(most: usize) -> Self {
        Chunk {
            size: (1 << 20).min(most),
            most,
        }
    }

    /// One that starts at the least: a reader of one of several ranges
    /// read at once (`ranges`), which shares the server's pace with the
    /// others.
    #[cfg(unix)]
    pub(super) fn from_least(most: usize) -> Self {
        Chunk {
            size: CHUNK_LEAST.min(most),
            most,
        }
    }

    /// The last call took `took`.
    pub(super) fn took(&mut self, took: std::time::Duration) {
        if took < std::time::Duration::from_millis(100) {
            self.size = (self.size * 2).min(self.most);
        } else if took > std::time::Duration::from_millis(400) {
            self.size = (self.size / 2).max(CHUNK_LEAST.min(self.most));
        }
    }
}

fn copy_file(
    src: &Path,
    dst: &Path,
    preserve_attributes: bool,
    pace: &Pace,
    ctx: &StepContext,
) -> anyhow::Result<()> {
    // Where the system copies a whole file (Windows), it does: it keeps
    // what a byte copy would lose there. Its place must be free (macOS's
    // `clonefile` refuses one taken): what a run this one retries cut
    // short there is gone on from, or replaced, byte by byte.
    let mut written = 0;
    let whole = if ctx.is_cut(dst) {
        None
    } else {
        sys::copy_whole(src, dst, preserve_attributes, &mut |total| {
            // Counted as the system says: nothing is written through off
            // Linux yet (`fs::volume::is_removable`).
            ctx.advance(total.saturating_sub(written));
            written = total;
            ctx.check_cancelled().is_ok()
        })
    };
    if let Some(copied) = whole {
        // Stopped by Cancel, the copy is cancelled, not failed.
        if copied.is_err() {
            ctx.check_cancelled()?;
        }
        let made = copied.couldnt(|| not_copied(src))?;
        // Counted whole, whatever the system said as it went: Wine's copy
        // says nothing, and its bytes were never counted, nor the plan
        // paused between its files. A clone's moved none (ADR-113).
        if let Ok(meta) = std::fs::symlink_metadata(dst) {
            let rest = meta.len().saturating_sub(written);
            if made == sys::Whole::Cloned {
                ctx.advance_unmeasured(rest);
            } else {
                ctx.advance(rest);
            }
        }
        ctx.check_cancelled()?;
        if preserve_attributes {
            transfer_attributes(src, dst, ctx)?;
        }
        return Ok(());
    }
    // Non-blocking, then checked: a file swapped for a FIFO since it was
    // listed must not block the executor in open(2).
    let sf = crate::fs::file::open_regular(src)
        .with_context(|| {
            words!(
                op_no_longer_a_file = "{ $name } is no longer a file",
                name = name_of(src),
                note = "Why a copy failed: what was listed as a file is something else now (a pipe, a folder)."
            )
        })?;
    let m = sf.metadata()?;
    // Larger than its drive keeps (a FAT drive's 4 GiB): refused before
    // its first byte, never written up to where the drive refuses it.
    if m.len() > sys::FAT_MOST
        && let Some(most) = dst.parent().and_then(sys::largest_file)
        && m.len() > most
    {
        return Err(not_copied(src).because(std::io::Error::new(
            std::io::ErrorKind::FileTooLarge,
            too_large_there(most),
        )));
    }
    // Cut short by the run this one retries: gone on from where its last
    // part reads as the source's, when the source is as it was then.
    let partial = ctx.partial_at(dst, &m);
    let resumed = match partial {
        Some(true) => std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(dst)
            .ok(),
        _ => None,
    };
    if partial.is_some() && resumed.is_none() {
        match std::fs::remove_file(dst) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
    }
    // Read in ranges at once, its last ranges may hold a mount's zeroes.
    #[cfg(unix)]
    let in_flight = if super::ranges::wanted(&m) {
        super::ranges::IN_FLIGHT
    } else {
        0
    };
    #[cfg(not(unix))]
    let in_flight = 0;
    let from = match &resumed {
        Some(df) => goes_on_from(&sf, df, m.len(), in_flight)?,
        None => 0,
    };
    let df = match resumed {
        Some(df) => {
            df.set_len(from)?;
            df
        }
        None => std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(dst)?,
    };
    // As it was made: on a remote, said by the server's answer to the
    // making, so asked of no one.
    let made = df.metadata()?;
    // What the kernel refused between these devices once, it is not asked
    // for each file again.
    let devices = sys::devices(&m, &made);
    let cloned = from == 0 && !ctx.kernel_refuses(devices, Kernel::Clone) && {
        let cloned = sys::clone_file(&sf, &df);
        if !cloned {
            ctx.kernel_refused(devices, Kernel::Clone);
        }
        cloned
    };
    // A clone moves no byte now, nor what a run before this one copied:
    // counted done, never as speed (ADR-113).
    if cloned {
        ctx.advance_unmeasured(m.len());
    } else {
        ctx.advance_unmeasured(from);
        // Should it fail partway, what it wrote may be gone on from.
        ctx.copying(dst, &m);
        let began = std::time::Instant::now();
        let end =
            copy_contents(&sf, &df, (&m, devices), from, pace, ctx).couldnt(|| not_copied(src))?;
        // From a server, how fast it reads (its thumbnails' Automatic).
        crate::fs::reach::note_read(src, dst, end.saturating_sub(from), began.elapsed());
        // A mount may answer the end of a file its server stopped serving,
        // and then say the file is that short: short of the source as it
        // was, the copy was cut (or the source changed), not done. Retry
        // goes on, or starts again on a source changed.
        if end < m.len() {
            return Err(not_copied(src).because(anyhow::Error::new(words!(
                op_read_short = "{ $read ->
                    [one] only { $read } byte of { $bytes } could be read
                   *[other] only { $read } bytes of { $bytes } could be read
                }",
                read = end,
                bytes = m.len(),
                note = "Why a copy failed: its source ended early (a server stopped serving it). read: how many bytes were read; bytes: how many it holds. Begins in lower case."
            ))));
        }
    }
    // Whole once closing it says so: a FUSE remote sends there what it
    // held, and says there what its server refused. Its mode and time are
    // given after, by its name: kio-fuse sends a file up as it is closed,
    // which would take a time given before.
    finish(df, src)?;
    // Set attributes and the time before restoring a read-only source's
    // permissions (Windows opens the file to give it its time).
    if preserve_attributes {
        transfer_attributes(src, dst, ctx)?;
    }
    let _ = sys::set_modified(dst, m.modified()?);
    // Made with them already (the usual 0644 and 022), they are not asked
    // for again: on a remote each change is a round trip to the server.
    if made.permissions() != m.permissions() {
        sys::mode_kept(std::fs::set_permissions(dst, m.permissions()))?;
    }
    Ok(())
}

/// Close `df`, the copy of `src` just written (`sys::close`), saying what
/// refused it in words that fit: what closing says is its drive's or
/// server's answer to the whole file, and kio-fuse calls a server's
/// refusal "no such file", which is no source gone.
pub(super) fn finish(df: std::fs::File, src: &Path) -> anyhow::Result<()> {
    sys::close(df).map_err(|error| {
        let refused = || {
            words!(
                op_refused_on_close = "its drive or server refused it as it was closed",
                note = "Why a copy failed: the drive or the server it was written to refused it whole once written."
            )
        };
        let refused = if error.kind() == std::io::ErrorKind::NotFound {
            anyhow::Error::new(refused())
        } else {
            anyhow::Error::new(error).context(refused())
        };
        not_copied(src).because(refused)
    })
}

/// The bytes of the copy `df` cut short it goes on from: up to the end of
/// its last part (of [`CHECKED_TAIL`]) that reads as the source's there,
/// trying [`TAIL_TRIES`] back from its end (a mount cut short may leave
/// the last bytes it was asked for zeroed), none otherwise (not this
/// source's start, or a server that wrote it out of order). Its last
/// `in_flight` bytes are not gone on from: what was read at once as it was
/// cut (`ranges::IN_FLIGHT`), whose zeroes may lie under whole ranges. A
/// copy cut short is read back where the system says what it wrote
/// (Unix); elsewhere it starts again.
pub(super) fn goes_on_from(
    sf: &std::fs::File,
    df: &std::fs::File,
    len: u64,
    in_flight: u64,
) -> std::io::Result<u64> {
    let have = df.metadata()?.len();
    if have == 0 || have > len {
        return Ok(0);
    }
    let have = have.saturating_sub(in_flight);
    if have == 0 {
        return Ok(0);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        let mut end = have;
        for _ in 0..TAIL_TRIES {
            let check = usize::try_from(end.min(CHECKED_TAIL)).unwrap_or(0);
            let at = end - check as u64;
            let (mut theirs, mut ours) = (vec![0; check], vec![0; check]);
            sf.read_exact_at(&mut theirs, at)?;
            df.read_exact_at(&mut ours, at)?;
            if theirs == ours {
                return Ok(end);
            }
            if at == 0 {
                break;
            }
            end = at;
        }
        Ok(0)
    }
    #[cfg(not(unix))]
    {
        let _ = sf;
        Ok(0)
    }
}

/// How much of a copy cut short is read back before it is gone on from.
#[cfg(unix)]
const CHECKED_TAIL: u64 = 1 << 20;

/// How many parts of [`CHECKED_TAIL`] back from a copy cut short's end
/// are read for one that is the source's.
#[cfg(unix)]
const TAIL_TRIES: usize = 4;

/// Copy the bytes in chunks from `from` (the start, or where a copy cut
/// short went to), checking for Cancel and reporting progress between
/// them (as `pace` counts it): in the kernel where it can
/// (`copy_file_range`), through a buffer where it cannot. A file with
/// holes keeps them, where the copy is left to the system: a disk image
/// of 100 GB holding 5 copied 100. The copy's length at its end: where
/// the source's reading ended.
fn copy_contents(
    mut sf: &std::fs::File,
    mut df: &std::fs::File,
    (meta, devices): (&std::fs::Metadata, Option<(u64, u64)>),
    from: u64,
    pace: &Pace,
    ctx: &StepContext,
) -> anyhow::Result<u64> {
    if pace.is_at_once() && sys::is_sparse(meta) {
        copy_sparse(sf, df, (from, meta.len()), devices, ctx)?;
        return Ok(meta.len());
    }
    if from > 0 {
        use std::io::Seek;
        sf.seek(std::io::SeekFrom::Start(from))?;
        df.seek(std::io::SeekFrom::Start(from))?;
    }
    let mut writing = pace.writing(df, from);
    // What this run copied: a 0 the kernel says before any is not trusted.
    let mut copied = 0u64;
    // A large file on a FUSE remote, read in ranges at once; what it grew
    // by since, the loops below take from where it was read to.
    #[cfg(unix)]
    if super::ranges::wanted(meta) {
        use std::io::Seek;
        copied = super::ranges::copy(sf, df, from, meta.len(), &mut writing, ctx)?;
        sf.seek(std::io::SeekFrom::Start(from + copied))?;
    }
    let mut chunk = Chunk::new(COPY_CHUNK);
    while !ctx.kernel_refuses(devices, Kernel::Range) {
        ctx.check_cancelled()?;
        let began = std::time::Instant::now();
        match sys::copy_range(sf, df, chunk.size)? {
            Some(0) if copied > 0 => return writing.close(ctx).map(|()| from + copied),
            Some(n @ 1..) => {
                chunk.took(began.elapsed());
                copied += n as u64;
                writing.wrote(n as u64, ctx)?;
            }
            // Some filesystems (procfs, a few FUSE) report 0 before the end,
            // so a first 0 is not trusted; others cannot copy in the kernel.
            // Either way the buffer takes over.
            Some(0) => break,
            None => {
                ctx.kernel_refused(devices, Kernel::Range);
                break;
            }
        }
    }
    // Both offsets advanced together, so the buffered copy resumes in place.
    let mut buf = ctx.copy_buffer();
    copied += copy_buffered(&mut sf, &mut df, &mut buf, ctx, &mut |n| {
        writing.wrote(n, ctx)
    })?;
    writing.close(ctx)?;
    Ok(from + copied)
}

/// Copy what `sf` reads to `df` through `buf`, a chunk at a time
/// (`Chunk`), until `sf` ends, telling `wrote` each chunk once written
/// whole and answering Cancel between two: the bytes copied. A write that
/// takes part of a chunk is written on until it took all of it, one that
/// takes none fails (`WriteZero`), and what the drive refuses (`EFBIG`
/// past what it keeps, `ENOSPC` once full) fails the copy: never an end,
/// which only the source's reading says.
fn copy_buffered(
    sf: &mut impl Read,
    df: &mut impl Write,
    buf: &mut [u8],
    ctx: &StepContext,
    wrote: &mut dyn FnMut(u64) -> anyhow::Result<()>,
) -> anyhow::Result<u64> {
    let mut chunk = Chunk::new(buf.len());
    let mut copied = 0u64;
    loop {
        ctx.check_cancelled()?;
        let began = std::time::Instant::now();
        let n = match sf.read(&mut buf[..chunk.size]) {
            Ok(0) => return Ok(copied),
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        };
        df.write_all(&buf[..n])?;
        // Read and written: a slow destination is as slow as a slow source.
        chunk.took(began.elapsed());
        copied += n as u64;
        wrote(n as u64)?;
    }
}

/// Copy a file's stretches of data from `start` (0, or where a
/// copy cut short went to: written in order, it holds the source's start,
/// its holes read as the source's), leaving its holes holes, then give
/// the copy its length `len`: its holes count as done.
fn copy_sparse(
    sf: &std::fs::File,
    df: &std::fs::File,
    (start, len): (u64, u64),
    devices: Option<(u64, u64)>,
    ctx: &StepContext,
) -> anyhow::Result<()> {
    #[cfg(unix)]
    use std::os::unix::fs::FileExt;
    let mut at = start;
    // A hole moves no byte: done, never speed (ADR-113).
    while let Some((data, hole)) = sys::next_data(sf, at)? {
        ctx.advance_unmeasured(data.saturating_sub(at));
        let mut from = data;
        while from < hole {
            ctx.check_cancelled()?;
            let want = usize::try_from(hole - from)
                .unwrap_or(usize::MAX)
                .min(COPY_CHUNK);
            let kernel = if ctx.kernel_refuses(devices, Kernel::Range) {
                None
            } else {
                sys::copy_range_at(sf, df, from, want)?
            };
            // Without Unix the buffer's branch only ends the copy, which
            // reads as a let-else there, and as none here.
            #[cfg_attr(not(unix), allow(clippy::manual_let_else))]
            let n = if let Some(n @ 1..) = kernel {
                n
            } else {
                if kernel.is_none() {
                    ctx.kernel_refused(devices, Kernel::Range);
                }
                #[cfg(unix)]
                {
                    let mut buf = ctx.copy_buffer();
                    let take = want.min(buf.len());
                    let n = sf.read_at(&mut buf[..take], from)?;
                    if n == 0 {
                        break;
                    }
                    df.write_all_at(&buf[..n], from)?;
                    n
                }
                #[cfg(not(unix))]
                {
                    anyhow::bail!(words!(
                        op_holes_not_yet = "Copying a file with holes (a sparse file) is not available on { $system } yet",
                        system = crate::system::named(),
                        note = "Why a copy failed: a file some parts of which were never written (a sparse file) is copied as such only on Unix. system: the system's name (Windows)."
                    ))
                }
            };
            from += n as u64;
            ctx.advance(n as u64);
        }
        at = hole.max(from);
    }
    ctx.advance_unmeasured(len.saturating_sub(at));
    df.set_len(len)?;
    Ok(())
}

/// Whether output owned by an interrupted copy looks finished. This is
/// only a resume hint: unrelated files also need `same_bytes`. Its
/// length, and its time (given last), to the second where either keeps
/// no more (SFTP's), else exactly: two files of one length changed in
/// one second are told apart where both volumes can.
pub(super) fn finished_as(there: &std::fs::Metadata, source: &std::fs::Metadata) -> bool {
    let since = |meta: &std::fs::Metadata| {
        meta.modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
    };
    let (Some(copy), Some(original)) = (since(there), since(source)) else {
        return false;
    };
    there.len() == source.len()
        && (copy == original
            || ((copy.subsec_nanos() == 0 || original.subsec_nanos() == 0)
                && copy.as_secs() == original.as_secs()))
}

fn copy_recursive_with_attributes(
    src: &Path,
    dst: &Path,
    preserve_attributes: bool,
    pace: &Pace,
    ctx: &StepContext,
) -> anyhow::Result<()> {
    ctx.check_cancelled()?;
    let m = std::fs::symlink_metadata(src)?;
    let kind = m.file_type();
    // Going on in what a run cut short made: what is there is that run's.
    let there = if ctx.going_on_in(dst) {
        std::fs::symlink_metadata(dst).ok()
    } else {
        None
    };
    if let Some(there) = &there
        && !(kind.is_dir() && there.is_dir())
    {
        if kind.is_file() && there.is_file() && finished_as(there, &m) {
            // Copied whole by that run: counted, and known for a later
            // name of the same file.
            ctx.advance(m.len());
            ctx.copied(&m, dst);
            return Ok(());
        }
        // The file it was writing goes on (`copy_file`); anything else
        // that run made is made again.
        if !(kind.is_file() && there.is_file() && ctx.is_cut(dst)) {
            super::staging::remove_tree(dst)?;
        }
    }
    if kind.is_symlink() {
        let target = std::fs::read_link(src)?;
        sys::symlink_like(&target, dst, kind)?;
    } else if kind.is_dir() {
        match &there {
            // Made by that run, its mode maybe given already.
            Some(there) if there.is_dir() => {
                let mode = there.permissions();
                if !sys::open_to_owner(&mode) {
                    sys::mode_kept(std::fs::set_permissions(dst, sys::opened_to_owner(&mode)))?;
                }
            }
            _ => std::fs::create_dir(dst)?,
        }
        for e in std::fs::read_dir(src)? {
            let e = e?;
            // A companion holding its sibling's attributes goes with them.
            if preserve_attributes && attributes_of_sibling(ctx, src, &e.file_name()) {
                continue;
            }
            copy_recursive_with_attributes(
                &e.path(),
                &dst.join(e.file_name()),
                preserve_attributes,
                pace,
                ctx,
            )?;
        }
        if preserve_attributes {
            transfer_attributes(src, dst, ctx)?;
        }
        sys::mode_kept(std::fs::set_permissions(dst, m.permissions()))?;
    } else if kind.is_file() {
        // A later name of a file of several names this plan copied is a
        // link to that copy (ADR-049).
        if !ctx.link_copied(&m, dst) {
            copy_file(src, dst, preserve_attributes, pace, ctx)?;
            ctx.copied(&m, dst);
        }
    } else if sys::is_fifo(kind) {
        // A named pipe has no content to copy; reading one would wait for a
        // writer forever. Make a new pipe, as `cp -R` does.
        sys::make_fifo(dst, &m).with_context(|| {
            words!(
                op_pipe_not_made = "the pipe { $name } could not be made",
                name = name_of(dst),
                note = "What a copy of a named pipe could not do: make a new pipe in its place; why follows."
            )
        })?;
    } else if sys::is_socket(kind) {
        anyhow::bail!(words!(
            op_socket_not_copied = "{ $name } is a socket, which cannot be copied",
            name = name_of(src),
            note = "Why a copy failed: a socket is a door to a running program, not a file."
        ));
    } else {
        anyhow::bail!(words!(
            op_device_not_copied = "{ $name } is a device, which cannot be copied",
            name = name_of(src),
            note = "Why a copy failed: a device file is a door to hardware, not a file."
        ));
    }
    Ok(())
}

/// Give `dst` the user's attributes of `src` (ADR-028): its own `user.*`
/// and those a companion holds for it (a Mac's `._x`), as attributes where
/// the destination keeps them; where it refuses them, beside it, as a Mac
/// does: the companion copied whole (its resource fork and Finder info
/// with it), or started from the attributes.
fn transfer_attributes(src: &Path, dst: &Path, ctx: &StepContext) -> anyhow::Result<()> {
    let own = user_attributes(src)?;
    let rules = ctx.companions();
    // The companion holding attributes alone (`._x`): asked for every file
    // and folder of a tree copied, looking for every other rule's too
    // stat-ed four names for each, a scan of the folder each on a FAT
    // card.
    let aside: Vec<Along> = rules
        .existing_holding_attributes(src)
        .into_iter()
        .filter_map(|found| {
            Some(Along {
                to: rules.place(&found, dst)?,
                attributes: true,
                from: found.path,
            })
        })
        .filter(|a| a.from != a.to)
        .collect();
    let beside = if aside.is_empty() {
        Vec::new()
    } else {
        rules.attributes(src).unwrap_or_default()
    };
    let all = merge_attributes(own.clone(), beside);
    let mut refused = false;
    for (name, value) in &all {
        match crate::fs::xattr::set(dst, name, value) {
            Ok(()) => {}
            Err(e) if crate::fs::xattr::refused(&e) => {
                refused = true;
                break;
            }
            Err(e) => {
                return Err(e).context(words!(
                    op_attributes_refused = "keep tags and other attributes while copying: check that the destination keeps extended attributes and lets you write them, or turn off { $setting } in Settings",
                    setting = crate::words::Named(crate::settings::keep_attributes_title()),
                    note = "What a copy could not do, its why after it: write a file's tags and other extended attributes where it goes. setting: the setting's title. Begins in lower case."
                ));
            }
        }
    }
    if !refused {
        return Ok(());
    }
    for a in &aside {
        // Small: counted at once, and written out with the rest
        // (`StepContext::write_out`).
        copy_recursive_with_attributes(&a.from, &a.to, false, &Pace::at_once(), ctx)?;
        // Made here for the copy: hidden as its dot hides it on Unix.
        crate::fs::hidden::made(&a.to);
    }
    if (aside.is_empty() || !own.is_empty()) && !rules.keep_attributes(dst, &own)? {
        anyhow::bail!(words!(
            op_attributes_not_kept = "keep tags and other attributes while copying: { $folder } keeps no attributes, and nothing is set to keep them beside it; turn off { $setting } in Settings to copy without them",
            folder = dst.parent().unwrap_or(dst).display().to_string(),
            setting = crate::words::Named(crate::settings::keep_attributes_title()),
            note = "Why a copy failed: the drive it goes to keeps no tags or attributes, and no companion file is set to keep them. folder: the path copied into; setting: the setting's title. Begins in lower case."
        ));
    }
    Ok(())
}

/// Native attributes take precedence over a companion's, both when a
/// copy carries them and when an existing destination is compared with it.
fn merge_attributes<N: PartialEq>(
    mut own: Vec<(N, Vec<u8>)>,
    beside: Vec<(N, Vec<u8>)>,
) -> Vec<(N, Vec<u8>)> {
    for (name, value) in beside {
        if !own.iter().any(|(n, _)| *n == name) {
            own.push((name, value));
        }
    }
    own
}

/// The extended attributes of `path` a copy keeps, none on a volume that
/// keeps none: on Linux the user's (`user.*`); on macOS, whose names have
/// no namespace, every one but those the system sets itself and refuses
/// to anyone else (`com.apple.rootless`, `com.apple.provenance`), Finder's
/// tags and info among those kept.
pub(super) fn user_attributes(path: &Path) -> anyhow::Result<Vec<(String, Vec<u8>)>> {
    kept_attributes(path)?.into_iter().map(|(name, value)| {
        name.into_string().map(|name| (name, value)).map_err(|_| anyhow::Error::new(words!(
            op_attribute_name_not_utf8 = "an attribute of { $name } has a non-UTF-8 name, which the copy's attribute interface cannot preserve",
            name = name_of(path),
            note = "Why a copy failed: an extended attribute of the file has a name that is not text."
        )))
    }).collect()
}

/// What could not be read of `path`'s attributes, its why after it.
fn attributes_unread(path: &Path) -> Words {
    words!(
        op_attributes_unread = "the tags and attributes of { $name } could not be read",
        name = name_of(path),
        note = "What a step could not do: read a file's extended attributes (its tags among them); why follows."
    )
}

/// Kept attribute names are bytes. A directory removal's inverse retains
/// them exactly even where a legacy companion interface uses text names.
fn kept_attributes(path: &Path) -> anyhow::Result<Vec<(OsString, Vec<u8>)>> {
    let names = match crate::fs::xattr::list(path) {
        Ok(names) => names,
        Err(error) if crate::fs::xattr::refused(&error) => return Ok(Vec::new()),
        Err(error) => {
            return Err(error).with_context(|| attributes_unread(path));
        }
    };
    let mut out = Vec::new();
    for name in names.into_iter().filter(|n| crate::fs::xattr::kept(n)) {
        // One removed since it was listed is not there to keep.
        let Some(value) =
            crate::fs::xattr::get(path, &name).with_context(|| attributes_unread(path))?
        else {
            continue;
        };
        out.push((name, value));
    }
    Ok(out)
}

/// Remove `p`, a folder with all it holds. A folder removed in part says
/// what is left: the first entry found still there, and how many more.
fn remove_recursive(p: &Path) -> anyhow::Result<()> {
    let m = std::fs::symlink_metadata(p)?;
    if m.is_dir() {
        // Never into what is mounted there: a drive's files, a server's.
        if let Some(why) = mounted_there(p) {
            anyhow::bail!(why);
        }
        std::fs::remove_dir_all(p).map_err(|e| {
            let error = anyhow::Error::new(e);
            let Some((first, count)) = left_in(p) else {
                return error;
            };
            let shown = first
                .strip_prefix(p.parent().unwrap_or(Path::new("")))
                .unwrap_or(&first)
                .display()
                .to_string();
            error.context(words!(
                op_left_after_removal = "{ $more ->
                    [0] { $path } is left
                   *[other] { $path } and { $more } more are left
                }",
                path = crate::words::Arg::Path(shown),
                more = count.saturating_sub(1),
                note = "What a folder removed in part left: the first file still there (its path from the folder above) and how many more besides it."
            ))
        })?;
    } else {
        std::fs::remove_file(p)?;
    }
    Ok(())
}

/// Why `path` may not be removed or trashed whole: it is, or holds, the
/// folder where a drive or a remote is mounted (ADR-077). A removal would
/// reach into it, deleting a drive's files or a server's.
fn mounted_there(path: &Path) -> Option<Words> {
    let points = crate::fs::volume::mounted_within(path);
    let point = points.first()?;
    Some(if point == path {
        words!(
            op_mount_point =
                "{ $name } is where a drive or a remote is mounted: eject it rather than delete it",
            name = name_of(path),
            note = "Why a folder is not deleted or put in the trash: a drive or a server's files are shown there."
        )
    } else {
        words!(
            op_holds_mount_point =
                "{ $name } holds { $point }, where a drive or a remote is mounted: eject it first",
            name = name_of(path),
            point = crate::words::Arg::Path(
                point
                    .strip_prefix(path)
                    .unwrap_or(point)
                    .display()
                    .to_string()
            ),
            note = "Why a folder is not deleted or put in the trash: a drive or a server's files are shown in a folder inside it. point: that folder's path from it."
        )
    })
}

/// What a removal of `dir` that stopped left behind, links not followed:
/// the first entry found that is no folder, or a folder that holds
/// nothing, and how many there are. A walk of bounded length; `None`
/// past it, or when `dir` cannot be read.
fn left_in(dir: &Path) -> Option<(PathBuf, usize)> {
    const BOUND: usize = 100_000;
    let mut stack = vec![dir.to_path_buf()];
    let mut first = None;
    let mut count = 0usize;
    let mut seen = 0usize;
    while let Some(folder) = stack.pop() {
        let mut held = false;
        for entry in std::fs::read_dir(&folder).ok()?.flatten() {
            seen += 1;
            if seen > BOUND {
                return None;
            }
            held = true;
            let path = entry.path();
            if std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_dir()) {
                stack.push(path);
            } else {
                count += 1;
                first.get_or_insert(path);
            }
        }
        if !held && folder != dir {
            count += 1;
            first.get_or_insert(folder);
        }
    }
    first.map(|first| (first, count))
}

/// An entry's own name, for a message.
fn display_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

// ------------------------------------------------------------------ swap

/// What a swap of `a` and `b` could not do, its why after it.
fn not_swapped(a: &Path, b: &Path) -> Words {
    words!(
        op_not_swapped = "{ $a } and { $b } could not swap names",
        a = name_of(a),
        b = name_of(b),
        note = "What a step could not do: exchange two files' names; why follows."
    )
}

/// Atomically exchange two entries (bulk rename cycles).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Swap {
    pub a: EntryRef,
    pub b: EntryRef,
}

impl super::Kind for Swap {
    const KIND: &'static str = "swap";
}

impl super::Effect for Swap {
    fn footprint(&self, _: &super::footprint::Volumes) -> Option<super::footprint::Footprint> {
        Some(
            super::footprint::Footprint::default()
                .made(&self.a.path)
                .made(&self.b.path),
        )
    }
    fn describe(&self) -> Change {
        Change::Swap {
            a: self.a.path.clone(),
            b: self.b.path.clone(),
        }
    }
    fn check(&self, _ctx: &StepContext) -> StepStatus {
        // Each stands where the other was: the exchange is in place.
        if id_of(&self.a.path) == Some(self.b.id) && id_of(&self.b.path) == Some(self.a.id) {
            return StepStatus::AlreadyDone;
        }
        if cfg!(windows) {
            return conflict(words!(
                op_swap_windows = "Windows cannot swap two names in one step",
                note = "Why a step that exchanges two files' names does not go on, on Windows."
            ));
        }
        if let Some(trashed) = trashed(&self.a.path).or_else(|| trashed(&self.b.path)) {
            return trashed;
        }
        match (self.a.resolve(), self.b.resolve()) {
            (Ok(_), Ok(_)) => pending(true),
            (Err(e), _) | (_, Err(e)) => conflict_of(e),
        }
    }
    fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let (pa, pb) = (self.a.resolve()?, self.b.resolve()?);
        // Each one's companions take the other's place with it.
        let mut pairs: Vec<(PathBuf, PathBuf)> = Vec::new();
        for a in along(ctx, &pa, &pb).into_iter().chain(along(ctx, &pb, &pa)) {
            if !pairs
                .iter()
                .any(|(x, y)| (*x == a.from && *y == a.to) || (*x == a.to && *y == a.from))
            {
                pairs.push((a.from, a.to));
            }
        }
        sys::exchange(&pa, &pb).with_context(|| not_swapped(&pa, &pb))?;
        for (from, to) in pairs {
            if exists(&to) {
                sys::exchange(&from, &to).with_context(|| not_swapped(&from, &to))?;
            } else {
                rename_noreplace(&from, &to)?;
            }
        }
        Ok(Some(Box::new(Swap {
            a: EntryRef::new(self.a.id, pb).with_only_name(self.a.only_name),
            b: EntryRef::new(self.b.id, pa).with_only_name(self.b.only_name),
        })))
    }
}

// ------------------------------------------------------------------ trash / restore

/// Move to the system trash.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Trash {
    pub entry: EntryRef,
}

impl super::Kind for Trash {
    const KIND: &'static str = "trash";
}

impl super::Effect for Trash {
    fn footprint(
        &self,
        volumes: &super::footprint::Volumes,
    ) -> Option<super::footprint::Footprint> {
        // An item of its own: its record is made new, under the first
        // name free (`put_trash`), the system's trash naming one too.
        Some(
            super::footprint::Footprint::default()
                .made_with(&self.entry.path, volumes)
                .record_in(super::footprint::TRASH),
        )
    }
    fn describe(&self) -> Change {
        Change::Trash {
            path: self.entry.path.clone(),
        }
    }
    fn check(&self, ctx: &StepContext) -> StepStatus {
        if in_trash(&self.entry.path).is_some() {
            return conflict(in_trash_already(&self.entry.path));
        }
        if keeps_no_trash(&self.entry.path) {
            return conflict(words!(
                op_no_trash_there =
                    "{ $name } is in a network folder a program mounts, which keeps no trash",
                name = name_of(&self.entry.path),
                note = "Why a file is not put in the trash: its folder is a server's, mounted by a program (sshfs, rclone, GVfs), where no trash is kept."
            ));
        }
        match ctx.resolve(&self.entry) {
            Ok(path) => mounted_there(&path).map_or_else(|| pending(true), conflict),
            Err(ResolveError::Gone(_)) => StepStatus::AlreadyDone,
            Err(e) => conflict_of(e),
        }
    }
    fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let src = ctx.resolve(&self.entry)?;
        if let Some(why) = mounted_there(&src) {
            anyhow::bail!(why);
        }
        anyhow::ensure!(in_trash(&src).is_none(), in_trash_already(&src));
        ctx.forget_trash();
        let companions: Vec<PathBuf> = ctx
            .companions()
            .existing(&src)
            .into_iter()
            .map(|f| f.path)
            .collect();
        let put = trash_item(ctx, &src).couldnt(|| {
            Couldnt::new(
                &src,
                words!(
                    op_not_trashed = "{ $name } could not be moved to the trash",
                    name = name_of(&src),
                    note = "What a step could not do; why follows."
                ),
            )
        })?;
        // Each an item of the trash of its own, restored with the file:
        // found by when it was trashed where the trash keeps records, by
        // where it went on macOS.
        let mut landed = Vec::new();
        for companion in companions {
            if let Ok(TrashedAt { at: Some(at), .. }) = trash_item(ctx, &companion)
                && cfg!(target_os = "macos")
            {
                landed.push(Landed {
                    at: PathArg(at),
                    from: PathArg(companion),
                });
            }
        }
        let restore = Restore {
            path: src.clone(),
            item: put.at.map(PathArg),
            landed,
        };
        let Some(copied_as) = put.waits else {
            return Ok(Some(Box::new(restore)));
        };
        // Its original's removal waits, and may not come: its undo tells
        // then whether the trash holds a copy.
        let made_as = restore
            .item
            .as_deref()
            .and_then(item_of_info)
            .and_then(|item| made_state(&item));
        Ok(Some(Box::new(Untrash {
            restore,
            original: EntryRef::new(self.entry.id, src).with_only_name(self.entry.only_name),
            copied_as: copied_as.0.map(|state| state.to_hex().to_string()),
            made_as,
        })))
    }
}

/// Where [`trash_item`] put a file: its record on Linux, the item on
/// macOS, where that can be told; and, put in a trash on another
/// filesystem whose removal of the original waits (`Moved::waits`: a
/// write-out onto a drive that can be unplugged), what the original was
/// as it was copied there.
struct TrashedAt {
    at: Option<PathBuf>,
    waits: Option<super::context::CopiedAs>,
}

/// Put `path` in the trash, and say where it went where it can be told
/// ([`TrashedAt`]): its record on Linux, where the plan puts it there
/// itself, its trash found once (`put_trash`), and one going to a trash
/// on another filesystem is moved there as any move across volumes is,
/// its original's removal waiting as a move's does; where the system put
/// it on macOS (`sys::trash_item`).
fn trash_item(ctx: &StepContext, path: &Path) -> anyhow::Result<TrashedAt> {
    #[cfg(target_os = "linux")]
    {
        match ctx.trashes().put(path)? {
            super::put_trash::Put::Done { info } => Ok(TrashedAt {
                at: Some(info),
                waits: None,
            }),
            super::put_trash::Put::Across { item, info } => match move_entry(ctx, path, &item, &[])
            {
                Ok(moved) => Ok(TrashedAt {
                    at: Some(info),
                    waits: moved.waits,
                }),
                Err(error) => Err(failed_across(error, &item, &info)),
            },
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = ctx;
        Ok(TrashedAt {
            at: sys::trash_item(path)?,
            waits: None,
        })
    }
}

/// A move into the trash on another volume that failed (`trash_item`),
/// its copy `item` in the trash's `files` and its record `info` made: the
/// trash holds nothing it does not list. While the original is whole, the
/// copy goes with its record; once part of the original went, the copy
/// alone holds it, and stays with its record, listed in the trash to be
/// restored from there.
#[cfg(target_os = "linux")]
fn failed_across(error: anyhow::Error, item: &Path, info: &Path) -> anyhow::Error {
    if exists(item) && OriginalStays::in_part(&error) {
        return error.context(words!(
            op_all_in_trash = "all of it is in the trash",
            note = "Said after why a move into the trash on another drive stopped: part of the original went, and the trash holds all of it."
        ));
    }
    if exists(item)
        && let Err(cleanup) = super::staging::remove_tree(item)
    {
        return error.context(words!(
            op_trash_copy_left = "a copy of it is left in { $path } ({ $why })",
            path = crate::words::Arg::Path(item.display().to_string()),
            why = super::io_words(&cleanup),
            note = "Said after why a move into the trash on another drive failed: its copy in the trash could not be removed. path: where it is; why: why it stayed."
        ));
    }
    let _ = std::fs::remove_file(info);
    error
}

/// Put `item` of the trash back where it came from, its record gone. On
/// Linux moved as any move is, across filesystems too (an item of a
/// subvolume's file is in the home trash): the crate only renames, and
/// left an empty file where its rename failed.
// Handed to the crate whole where it restores it.
#[cfg_attr(target_os = "linux", allow(clippy::needless_pass_by_value))]
fn restore_listed(ctx: &StepContext, item: trash::TrashItem) -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    {
        let info = PathBuf::from(&item.id);
        let file = item_of_info(&info).ok_or_else(|| anyhow::Error::new(no_record(&info)))?;
        let to = item.original_path();
        std::fs::create_dir_all(&item.original_parent)?;
        anyhow::ensure!(!exists(&to), there_again(&to));
        let moved = move_entry(ctx, &file, &to, &[])?;
        record_goes(ctx, &moved, &file, &info)?;
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = ctx;
        sys::trash_restore(vec![item])
    }
}

/// The record `info` of the item of a trash at `item`, moved out
/// (`moved`): removed once the item left the trash, at once, or where its
/// removal waits (`Moved::waits`: a write-out onto a drive that can be
/// unplugged) once that came, a failure then told at the plan's end.
/// Should it not come (the item changed meanwhile, the write-out failed),
/// the record stays with it: the trash lists what it holds.
fn record_goes(ctx: &StepContext, moved: &Moved, item: &Path, info: &Path) -> std::io::Result<()> {
    let (item, info) = (item.to_path_buf(), info.to_path_buf());
    let gone = move || {
        if std::fs::symlink_metadata(&item).is_ok() {
            return Ok(());
        }
        match std::fs::remove_file(&info) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        }
    };
    if moved.waits.is_none() {
        return gone();
    }
    // After the removal it waits for, asked before it.
    ctx.after_write_out(move || Ok(gone()?));
    Ok(())
}

/// How far apart, in seconds, a file and its companions are trashed.
const COMPANION_TRASHED_WITHIN: i64 = 5;

/// Where the trash keeps what it says of the file at `path` (where it came
/// from, when), for an item of a trash: freedesktop's
/// `info/<name>.trashinfo` beside its `files` folder (a trash is
/// `$XDG_DATA_HOME/Trash`, or on another volume `.Trash/<uid>` or
/// `.Trash-<uid>` at its top), Windows' `$I` record beside a `$R` item
/// (`records`). Read from the path alone.
pub fn trash_info_of(path: &Path) -> Option<PathBuf> {
    super::records::record_of(path)
}

/// What a `.trashinfo` says: `records::parse_trash_info`.
pub use super::records::parse_trash_info;

/// The name the item of a trash at `item` had where it came from: a trash
/// holding a file of that name already gives it another (`a.txt.2`), and
/// taken out, it goes back to its own. `None` for a file that is no item
/// of a trash. Reads its `.trashinfo`.
pub(super) fn trashed_name(item: &Path) -> Option<OsString> {
    let info = trash_info_of(item)?;
    let bytes = read_record(&info).ok()?;
    let said = super::records::read(&info, &bytes, Some(Path::new("/")))?;
    said.original.file_name().map(OsStr::to_os_string)
}

/// The bytes of the record at `info`, whole: a trash is anyone's to
/// write in on a shared volume, so a pipe or a giant named like a record
/// is refused.
fn read_record(info: &Path) -> std::io::Result<Vec<u8>> {
    crate::fs::file::read_regular(info, crate::fs::file::SIDE_FILE_LIMIT)
}

/// A file of a trash at any depth (`in_trash`): the item of the trash it
/// is, or is inside (a trashed folder's file), what the trash says of the
/// item, and where the file is in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InTrash {
    /// The item, in the trash's `files` folder.
    pub item: PathBuf,
    /// The item's `.trashinfo`.
    pub info: PathBuf,
    /// The file's path inside the item; empty for the item itself.
    pub inner: PathBuf,
}

/// Whether `path` is in a trash, at any depth, and where: read from the
/// path alone, as `trash_info_of`. A file inside a trashed folder is in the
/// trash too: put there again, it would be an item that came from the
/// trash.
pub fn in_trash(path: &Path) -> Option<InTrash> {
    path.ancestors().find_map(|item| {
        Some(InTrash {
            info: trash_info_of(item)?,
            inner: path.strip_prefix(item).ok()?.to_path_buf(),
            item: item.to_path_buf(),
        })
    })
}

/// The record of an item the `trash` crate lists: its id on freedesktop
/// systems (the `.trashinfo`), the record beside its id on Windows (the
/// `$R` item, as the shell names it).
fn record_of_listed(item: &trash::TrashItem) -> PathBuf {
    let id = Path::new(&item.id);
    trash_info_of(id).unwrap_or_else(|| id.to_path_buf())
}

/// The trash as a plan read it (`StepContext::with_trash_near`), each item
/// found by its record and by where it was put there from: a step looks
/// up its item, never the whole listing, and one that takes an item out
/// takes it out of the listing too, never reading the trash again (a
/// thousand files moved out of a trash of ten thousand read ten million
/// records).
pub(super) struct TrashListing {
    items: Vec<Option<trash::TrashItem>>,
    by_record: std::collections::HashMap<PathBuf, usize>,
    by_original: std::collections::HashMap<PathBuf, Vec<usize>>,
}

impl TrashListing {
    /// More items of the trash, read since: another volume's (Linux's own
    /// listing, `trash_list`).
    #[cfg(target_os = "linux")]
    pub(super) fn add(&mut self, items: Vec<trash::TrashItem>) {
        for item in items {
            let n = self.items.len();
            self.by_record.insert(record_of_listed(&item), n);
            self.by_original
                .entry(item.original_path())
                .or_default()
                .push(n);
            self.items.push(Some(item));
        }
    }

    pub(super) fn new(items: Vec<trash::TrashItem>) -> Self {
        let mut by_record = std::collections::HashMap::with_capacity(items.len());
        let mut by_original: std::collections::HashMap<PathBuf, Vec<usize>> =
            std::collections::HashMap::with_capacity(items.len());
        for (n, item) in items.iter().enumerate() {
            by_record.insert(record_of_listed(item), n);
            by_original.entry(item.original_path()).or_default().push(n);
        }
        TrashListing {
            items: items.into_iter().map(Some).collect(),
            by_record,
            by_original,
        }
    }

    /// The item whose record is `info`, still listed.
    fn record(&self, info: &Path) -> Option<&trash::TrashItem> {
        self.items.get(*self.by_record.get(info)?)?.as_ref()
    }

    /// The item listed last put there from `original`: the latest, or the
    /// latest within `COMPANION_TRASHED_WITHIN` of `near`.
    fn latest_from(&self, original: &Path, near: Option<i64>) -> Option<usize> {
        self.by_original
            .get(original)?
            .iter()
            .filter_map(|&n| Some((n, self.items[n].as_ref()?)))
            .filter(|(_, item)| {
                near.is_none_or(|when| (item.time_deleted - when).abs() <= COMPANION_TRASHED_WITHIN)
            })
            .max_by_key(|(_, item)| item.time_deleted)
            .map(|(n, _)| n)
    }

    /// The item at `n`, out of the listing.
    fn take(&mut self, n: usize) -> Option<trash::TrashItem> {
        let item = self.items.get_mut(n)?.take()?;
        self.by_record.remove(&record_of_listed(&item));
        Some(item)
    }

    /// The item whose record is `info` is no longer in the trash.
    pub(super) fn taken(&mut self, info: &Path) {
        if let Some(n) = self.by_record.remove(info) {
            self.items[n] = None;
        }
    }
}

/// The item a trash's record speaks of: `records::item_of`.
fn item_of_info(info: &Path) -> Option<PathBuf> {
    super::records::item_of(info)
}

/// Where the companions of a file at `path` are, by every rule
/// (`Companions::of`): the same places, in the same order, for every
/// name, so the one at an index is that one wherever its file goes.
fn companion_places(ctx: &StepContext, path: &Path) -> Vec<PathBuf> {
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
        return Vec::new();
    };
    ctx.companions()
        .of(name)
        .into_iter()
        .map(|p| dir.join(p))
        .collect()
}

/// Where the item of the trash whose `.trashinfo` is `info` was put there
/// from, and when, as the plan's trash listing says.
fn trashed_from(ctx: &StepContext, info: &Path) -> Option<(PathBuf, i64)> {
    ctx.with_trash_near(&[info], |listing| {
        listing
            .record(info)
            .map(|i| (i.original_path(), i.time_deleted))
    })
    .ok()
    .flatten()
}

/// The companions trashed with a file put in the trash at `when`, each an
/// item of its own (`Trash`): for each of `places` (its companions' places
/// when it was put there, `companion_places`) that `wanted` says, the item
/// put there from it then or just after, the latest, taken out of the
/// plan's trash listing. Each with its index in `places`.
fn take_trashed_with(
    ctx: &StepContext,
    places: &[PathBuf],
    when: i64,
    wanted: impl Fn(usize) -> bool,
) -> Vec<(usize, trash::TrashItem)> {
    if places.is_empty() {
        return Vec::new();
    }
    let near: Vec<&Path> = places.iter().map(PathBuf::as_path).collect();
    ctx.with_trash_near(&near, |listing| {
        places
            .iter()
            .enumerate()
            .filter(|(n, _)| wanted(*n))
            .filter_map(|(index, place)| {
                let n = listing.latest_from(place, Some(when))?;
                Some((index, listing.take(n)?))
            })
            .collect()
    })
    .unwrap_or_default()
}

/// A file taken out of the trash to a place of its own (a companion of a
/// file restored or moved out of the trash), and what puts it back as it
/// was: where it was in the trash and what the trash said of it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Untrashed {
    /// Where it is now.
    #[serde(with = "crate::os_path")]
    #[schemars(with = "PathArg")]
    pub path: PathBuf,
    /// Its `.trashinfo`, which names its place in the trash.
    pub info: PathArg,
    /// What the trash said of it, written again when it goes back: a
    /// text on Linux, bytes on Windows.
    #[serde(with = "crate::os_path::bytes")]
    #[schemars(with = "String")]
    pub info_text: Vec<u8>,
}

impl Untrashed {
    /// Take the item of the trash whose `.trashinfo` is `info` out to `to`
    /// (into a companions' folder made where needed, `.ts`), what the
    /// trash said of it with it.
    fn take(ctx: &StepContext, info: &Path, to: &Path) -> anyhow::Result<Self> {
        let item = item_of_info(info).ok_or_else(|| anyhow::Error::new(no_record(info)))?;
        let info_text = read_record(info).with_context(|| record_unread(info))?;
        if let Some(dir) = to.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let moved = move_entry(ctx, &item, to, &[])?;
        let _ = record_goes(ctx, &moved, &item, info);
        Ok(Untrashed {
            path: to.to_path_buf(),
            info: PathArg::from(info),
            info_text,
        })
    }

    /// Whether it is in the trash as it was.
    fn is_back(&self) -> bool {
        self.info.exists() && item_of_info(&self.info).is_some_and(|item| exists(&item))
    }

    /// Whether nothing is left to put back: it is in the trash as it was,
    /// or gone from where it came back to (deleted since: its file goes
    /// with nothing, and an undo of what it came back with goes on).
    fn is_done(&self) -> bool {
        self.is_back()
            || !exists(&self.path) && item_of_info(&self.info).is_none_or(|item| !exists(&item))
    }

    /// Back in the trash where it was, as the trash said; gone since it
    /// came back, there is nothing to put back, nor a record of nothing.
    fn put_back(&self, ctx: &StepContext) -> anyhow::Result<()> {
        let item =
            item_of_info(&self.info).ok_or_else(|| anyhow::Error::new(no_record(&self.info)))?;
        if !exists(&item) {
            if !exists(&self.path) {
                return Ok(());
            }
            move_entry(ctx, &self.path, &item, &[])?;
        }
        if !self.info.exists() {
            std::fs::write(&self.info, &self.info_text)
                .with_context(|| record_unwritten(&self.info))?;
        }
        Ok(())
    }
}

/// Restore the trashed item that had this original path: the one whose
/// `.trashinfo` is `item` (the trash's own listing names it), else the
/// most recently trashed. A folder whose place holds a folder again (made
/// again by restoring one of its files alone, `RestorePart`, or by hand)
/// merges into it: what it holds moves in where its place is free.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Restore {
    #[serde(with = "crate::os_path")]
    #[schemars(with = "PathArg")]
    pub path: PathBuf,
    /// The item's record (`.trashinfo`, `$I`), when one item is meant:
    /// undoing a trash names it where it is known (Linux: a plan that
    /// trashes a path twice, a replace undone, restores each its own),
    /// and without it restores what was trashed last from `path`. On
    /// macOS, whose trash keeps no record, the item itself, where
    /// `trashItemAtURL` put it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<PathArg>,
    /// On macOS, the companions trashed with it and where they went.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub landed: Vec<Landed>,
}

/// A file put in a trash that keeps no record of it (macOS's): where it
/// is there, and where it came from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Landed {
    pub at: PathArg,
    pub from: PathArg,
}

impl Restore {
    /// The item itself, on macOS, where its trash keeps no record: what a
    /// restore moves back. `None` where `item` is a record.
    fn landed_item(&self) -> Option<&Path> {
        let item = self.item.as_deref()?;
        (cfg!(target_os = "macos") && super::records::item_of(item).is_none()).then_some(item)
    }
}

impl Restore {
    /// Index of the item to restore in the plan's trash listing: the one
    /// named, else (it was restored and trashed again since, under
    /// another name) the most recent from the path.
    fn find(&self, listing: &TrashListing) -> Option<usize> {
        if let Some(item) = self.item.as_deref()
            && let Some(&n) = listing.by_record.get(item)
        {
            return Some(n);
        }
        listing.latest_from(&self.path, None)
    }
}

impl Restore {
    /// Where its item's trash may be: the volume of where it goes, and of
    /// its record when named.
    fn near(&self) -> Vec<&Path> {
        std::iter::once(self.path.as_path())
            .chain(self.item.as_deref())
            .collect()
    }

    /// The trashed folder to merge into `path`, a folder there again: the
    /// item named, still in the trash, a folder too.
    fn merging(&self) -> Option<PathBuf> {
        let info = self.item.as_deref().filter(|info| info.exists())?;
        let item = item_of_info(info)?;
        let is_dir = |p: &Path| std::fs::symlink_metadata(p).is_ok_and(|m| m.is_dir());
        (is_dir(&item) && is_dir(&self.path)).then_some(item)
    }
}

/// The first place in `into` that a file of the trashed folder `from` would
/// take and cannot: something is there, and they are not both folders,
/// which merge.
fn clash(from: &Path, into: &Path) -> std::io::Result<Option<PathBuf>> {
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let dest = into.join(entry.file_name());
        let Ok(there) = std::fs::symlink_metadata(&dest) else {
            continue;
        };
        if !(entry.file_type()?.is_dir() && there.is_dir()) {
            return Ok(Some(dest));
        }
        if let Some(found) = clash(&entry.path(), &dest)? {
            return Ok(Some(found));
        }
    }
    Ok(None)
}

/// What a merge did: what moved, relative to both folders, and the
/// trashed folder's own folders it left empty, deepest first.
#[derive(Default)]
struct Merged {
    moved: Vec<PathBuf>,
    left: Vec<PathBuf>,
}

/// Move what the trashed folder `from` holds into the folder `into`: each
/// file or folder whose place is free, whole; into a folder that is there,
/// what it holds. `rel` is where both are in the folders merged.
fn merge(
    ctx: &StepContext,
    from: &Path,
    into: &Path,
    rel: &Path,
    merged: &mut Merged,
) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name();
        let (dest, inner) = (into.join(&name), rel.join(&name));
        if exists(&dest) {
            anyhow::ensure!(
                entry.file_type()?.is_dir() && dest.is_dir(),
                there_again(&dest)
            );
            merge(ctx, &entry.path(), &dest, &inner, merged)?;
        } else {
            move_entry(ctx, &entry.path(), &dest, &[])?;
            merged.moved.push(inner);
        }
    }
    merged.left.push(rel.to_path_buf());
    Ok(())
}

impl super::Kind for Restore {
    const KIND: &'static str = "restore";
}

impl super::Effect for Restore {
    fn footprint(&self, _: &super::footprint::Volumes) -> Option<super::footprint::Footprint> {
        Some(
            super::footprint::Footprint::default()
                .made(&self.path)
                .store(super::footprint::TRASH),
        )
    }
    fn describe(&self) -> Change {
        Change::Restore {
            path: self.path.clone(),
        }
    }
    /// Its item in the trash: the item itself where the trash keeps no
    /// record (macOS), else the one its record names; none where it names
    /// no record (what was trashed last from its path).
    fn brings_from(&self) -> Option<PathBuf> {
        self.landed_item()
            .map(Path::to_path_buf)
            .or_else(|| super::records::item_of(self.item.as_deref()?))
    }
    fn check(&self, ctx: &StepContext) -> StepStatus {
        if let Some(item) = self.landed_item() {
            return match (exists(&self.path), exists(item)) {
                (true, false) => StepStatus::AlreadyDone,
                (true, true) => conflict(there_again(&self.path)),
                (false, true) => pending(true),
                (false, false) => conflict(not_in_trash(&self.path)),
            };
        }
        if exists(&self.path) {
            // A folder there again takes the trashed folder's files where
            // their places are free.
            if let Some(item) = self.merging() {
                return match clash(&item, &self.path) {
                    Ok(None) => pending(true),
                    Ok(Some(taken)) => conflict(there_again(&taken)),
                    Err(e) => conflict(words!(
                        op_unreadable = "{ $name } could not be read: { $error }",
                        name = name_of(&item),
                        error = anyhow::Error::new(e),
                        note = "Why a step does not go on: what it acts on could not be read. error: why."
                    )),
                };
            }
            // The item named still in the trash: another file took its
            // place since, which restoring it would not overwrite.
            if self.item.as_deref().is_some_and(Path::exists) {
                return conflict(there_again(&self.path));
            }
            return StepStatus::AlreadyDone;
        }
        match ctx.with_trash_near(&self.near(), |listing| self.find(listing)) {
            Ok(Some(_)) => pending(true),
            Ok(None) => conflict(not_in_trash(&self.path)),
            Err(e) => conflict(trash_unread(e)),
        }
    }
    fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        if let Some(item) = self.landed_item() {
            rename_noreplace(item, &self.path).couldnt(|| not_restored(&self.path))?;
            for companion in &self.landed {
                if !exists(&companion.from) {
                    let _ = rename_noreplace(&companion.at, &companion.from);
                }
            }
            let r = EntryRef::from_path(&self.path)
                .ok_or_else(|| anyhow::Error::new(went_missing_restored(&self.path)))?;
            return Ok(Some(Box::new(Trash { entry: r })));
        }
        if let (Some(item), Some(info)) = (self.merging(), self.item.as_deref()) {
            // The companions trashed with it, found while the trash still
            // says when it was put there, go back where their places are
            // free, as they do with a folder restored whole.
            let places = companion_places(ctx, &self.path);
            let with = if places.is_empty() {
                Vec::new()
            } else {
                trashed_from(ctx, info).map_or_else(Vec::new, |(_, when)| {
                    take_trashed_with(ctx, &places, when, |n| !exists(&places[n]))
                })
            };
            ctx.trash_taken(info);
            let mut unmerge = merge_restore(ctx, &item, info, &self.path)?;
            for (n, found) in with {
                if let Ok(back) = Untrashed::take(ctx, &record_of_listed(&found), &places[n]) {
                    unmerge.companions.push(back);
                }
            }
            return Ok(Some(Box::new(unmerge)));
        }
        // Taken out of the listing: once restored it is no longer there.
        let item = ctx
            .with_trash_near(&self.near(), |listing| {
                self.find(listing).and_then(|n| listing.take(n))
            })
            .with_context(trash_unread_here)?
            .ok_or_else(|| anyhow::Error::new(not_in_trash(&self.path)))?;
        let when = item.time_deleted;
        restore_listed(ctx, item).couldnt(|| not_restored(&self.path))?;
        restore_trashed_with(ctx, &self.path, when);
        let r = EntryRef::from_path(&self.path)
            .ok_or_else(|| anyhow::Error::new(went_missing_restored(&self.path)))?;
        Ok(Some(Box::new(Trash { entry: r })))
    }
}

/// Whether a companion trashed with the file at `path`, put in the trash
/// at `when`, is there to come back, its place free
/// (`restore_trashed_with`).
fn trashed_with_any(ctx: &StepContext, path: &Path, when: i64) -> bool {
    let places = companion_places(ctx, path);
    if places.is_empty() {
        return false;
    }
    let near: Vec<&Path> = places.iter().map(PathBuf::as_path).collect();
    ctx.with_trash_near(&near, |listing| {
        places
            .iter()
            .any(|place| !exists(place) && listing.latest_from(place, Some(when)).is_some())
    })
    .unwrap_or(false)
}

/// Restore the companions trashed with the file at `path`, put in the
/// trash at `when`, where their places are free.
fn restore_trashed_with(ctx: &StepContext, path: &Path, when: i64) {
    let places = companion_places(ctx, path);
    let found: Vec<trash::TrashItem> =
        take_trashed_with(ctx, &places, when, |n| !exists(&places[n]))
            .into_iter()
            .map(|(_, item)| item)
            .collect();
    for item in found {
        let _ = restore_listed(ctx, item);
    }
}

// ------------------------------------------------------------------ untrash

/// Take back a trash onto another filesystem whose original's removal
/// waited (a write-out onto a drive that can be unplugged): the undo of
/// [`Trash`] then, which a restore alone would not be. That removal may
/// never have come (the write-out failed, the original changed
/// meanwhile, it could not be removed): the original stayed where it
/// was, told as [`Unmove`] tells it, and the trash holds a copy of it,
/// listed. That copy goes for good with its record only while the
/// original is whole as it was copied and the copy as it was made; else
/// both stay, the trash keeping what the original was. The companions
/// trashed with the file come back where their places are free, as a
/// restore brings them. Where the original went, the item is restored
/// (`restore`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Untrash {
    /// The restore: the undo where the original went.
    pub restore: Restore,
    /// The original, where it was.
    pub original: EntryRef,
    /// What the original was as it was copied (`state_of`, in
    /// hexadecimal); `None` where it could not all be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copied_as: Option<String>,
    /// The copy in the trash as it was made (`made_state`, in
    /// hexadecimal); `None` where it could not all be read, and then it
    /// is never removed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub made_as: Option<String>,
}

impl Untrash {
    /// The trash's copy and its record, where the original stayed where
    /// it was; `None` where it went, for the restore to take back.
    fn copy(&self) -> Option<(PathBuf, PathBuf)> {
        let info = self.restore.item.as_deref()?;
        if !stayed(&self.original) {
            return None;
        }
        Some((item_of_info(info)?, info.to_path_buf()))
    }

    /// Whether the copy at `item` goes for good: the original whole as it
    /// was copied, and the copy as it was made.
    fn takes(&self, item: &Path) -> bool {
        let whole = self.copied_as.is_some() && made_state(&self.original.path) == self.copied_as;
        whole && self.made_as.is_some() && made_state(item) == self.made_as
    }
}

impl super::Kind for Untrash {
    const KIND: &'static str = "untrash";
}

impl super::Effect for Untrash {
    fn footprint(
        &self,
        volumes: &super::footprint::Volumes,
    ) -> Option<super::footprint::Footprint> {
        super::Effect::footprint(&self.restore, volumes)
    }
    fn describe(&self) -> Change {
        super::Effect::describe(&self.restore)
    }
    fn check(&self, ctx: &StepContext) -> StepStatus {
        let Some((item, info)) = self.copy() else {
            return super::Effect::check(&self.restore, ctx);
        };
        let with = trashed_from(ctx, &info)
            .is_some_and(|(_, when)| trashed_with_any(ctx, &self.original.path, when));
        if with || exists(&item) && self.takes(&item) {
            pending(true)
        } else {
            // Gone from the trash since, or kept there, and its companions
            // back: nothing to take back.
            StepStatus::AlreadyDone
        }
    }
    fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let Some((item, info)) = self.copy() else {
            return super::Effect::apply(&self.restore, ctx);
        };
        // When it was put there, for its companions: read while the trash
        // still says.
        let when = trashed_from(ctx, &info).map(|(_, when)| when);
        if exists(&item) && self.takes(&item) {
            remove_recursive(&item).with_context(|| {
                words!(
                    op_trash_copy_not_removed = "the copy of { $name } in the trash could not all be removed",
                    name = name_of(&self.original.path),
                    note = "What the undo of a move into the trash on another drive could not do: remove the copy the trash held; why follows."
                )
            })?;
            ctx.trash_taken(&info);
            if let Err(error) = std::fs::remove_file(&info)
                && error.kind() != std::io::ErrorKind::NotFound
            {
                return Err(error.into());
            }
        }
        if let Some(when) = when {
            restore_trashed_with(ctx, &self.original.path, when);
        }
        Ok(Some(Box::new(Trash {
            entry: self.original.clone(),
        })))
    }
}

/// Restore the trashed folder `item` (of which `info` speaks) into the
/// folder `path`, there again: merged, then the trashed folder's empty
/// folders and its `.trashinfo` go, as a restore takes them. Undone, the
/// files that moved go back into it (`Unmerge`); nothing else of the
/// folder there leaves it. Its companions are the caller's.
fn merge_restore(
    ctx: &StepContext,
    item: &Path,
    info: &Path,
    path: &Path,
) -> anyhow::Result<Unmerge> {
    let mut merged = Merged::default();
    if let Err(e) = merge(ctx, item, path, Path::new(""), &mut merged) {
        // Back as it was: the trash keeps the folder whole.
        for inner in merged.moved.iter().rev() {
            let _ = move_entry(ctx, &path.join(inner), &item.join(inner), &[]);
        }
        return Err(not_restored(path).because(e));
    }
    let info_text = read_record(info).unwrap_or_default();
    let mut folders = Vec::new();
    for inner in merged.left {
        // One a file was put in since stays, and the trash with it.
        if std::fs::remove_dir(item.join(&inner)).is_ok() {
            folders.push(PathArg(inner));
        }
    }
    if !exists(item) {
        let _ = std::fs::remove_file(info);
    }
    Ok(Unmerge {
        path: path.to_path_buf(),
        info: PathArg::from(info),
        info_text,
        moved: merged.moved.into_iter().map(PathArg).collect(),
        folders,
        companions: Vec::new(),
    })
}

/// Undo a trashed folder's restore that merged into a folder there again
/// (`Restore`): what moved goes back into the trashed folder, made again
/// with its folders and what the trash said of it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Unmerge {
    /// The folder it merged into.
    #[serde(with = "crate::os_path")]
    #[schemars(with = "PathArg")]
    pub path: PathBuf,
    /// The trashed folder's record (`.trashinfo`, `$I`).
    pub info: PathArg,
    /// What it said, written again if it went.
    #[serde(with = "crate::os_path::bytes")]
    #[schemars(with = "String")]
    pub info_text: Vec<u8>,
    /// What moved, relative to both folders.
    pub moved: Vec<PathArg>,
    /// The trashed folder's folders that were left empty and went,
    /// relative to it, deepest first.
    pub folders: Vec<PathArg>,
    /// The companions trashed with it that came back with it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub companions: Vec<Untrashed>,
}

impl super::Kind for Unmerge {
    const KIND: &'static str = "unmerge";
}

impl super::Effect for Unmerge {
    fn describe(&self) -> Change {
        Change::Trash {
            path: self.path.clone(),
        }
    }
    fn check(&self, _ctx: &StepContext) -> StepStatus {
        let Some(item) = item_of_info(&self.info) else {
            return conflict(words!(
                op_no_trash_s = "{ $name } names no item of a trash",
                name = name_of(&self.info),
                note = "Why the undo of a folder restored into one there does not go on: the trash's record of it names no item of a trash."
            ));
        };
        let back = |inner: &PathArg| exists(&item.join(inner));
        if self.info.exists()
            && self.moved.iter().all(back)
            && self.companions.iter().all(Untrashed::is_done)
        {
            return StepStatus::AlreadyDone;
        }
        if let Some(gone) = self
            .moved
            .iter()
            .find(|inner| !back(inner) && !exists(&self.path.join(inner)))
        {
            return conflict(no_longer_there(&self.path.join(gone)));
        }
        pending(true)
    }
    fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let item =
            item_of_info(&self.info).ok_or_else(|| anyhow::Error::new(no_record(&self.info)))?;
        ctx.forget_trash();
        for inner in self.folders.iter().rev() {
            std::fs::create_dir_all(item.join(inner))
                .with_context(|| folder_path_not_made(&item.join(inner)))?;
        }
        for inner in &self.moved {
            let (now, back) = (self.path.join(inner), item.join(inner));
            if !exists(&back) {
                if let Some(dir) = back.parent() {
                    std::fs::create_dir_all(dir)?;
                }
                move_entry(ctx, &now, &back, &[])?;
            }
        }
        if !self.info.exists() {
            std::fs::write(&self.info, &self.info_text)
                .with_context(|| record_unwritten(&self.info))?;
        }
        for companion in &self.companions {
            companion.put_back(ctx)?;
        }
        Ok(Some(Box::new(Restore {
            path: self.path.clone(),
            item: Some(self.info.clone()),
            landed: Vec::new(),
        })))
    }
}

// ------------------------------------------------------------------ out of the trash, one file

/// Take a file out of the trash to `path`, alone: a file or folder from
/// inside a trashed folder restored (`Restore` takes back a whole item),
/// or any file of the trash moved elsewhere (`Move`). The folders it was
/// in are made again where they are missing, as their counterparts in the
/// trash are (their mode, `user.*` attributes and times), and it moves
/// there with its companions. A trashed folder it leaves stays in the
/// trash with the rest, under what the trash says of it; restored later,
/// it merges into the folders made again. An item of the trash takes
/// what the trash says of it along, and the companions trashed with it go
/// beside it where their places are free; undone, all of it is in the
/// trash again as it was (`ReturnPart`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RestorePart {
    /// It, in the trash.
    pub entry: EntryRef,
    /// Where it goes: restored, where it was (where the trashed folder
    /// was, then its path in it).
    #[serde(with = "crate::os_path")]
    #[schemars(with = "PathArg")]
    pub path: PathBuf,
}

impl RestorePart {
    /// The folders to make for it, outermost first, each with its
    /// counterpart in the trash while there is one: `src` is where it is
    /// in the trashed folder, `depth` how deep there.
    fn folders(&self, src: &Path, depth: usize) -> Vec<(PathBuf, Option<PathBuf>)> {
        let mut missing: Vec<(PathBuf, Option<PathBuf>)> = self
            .path
            .ancestors()
            .skip(1)
            .zip(1..)
            .take_while(|(dir, _)| !exists(dir))
            .map(|(dir, level)| {
                let like = (level <= depth)
                    .then(|| src.ancestors().nth(level))
                    .flatten();
                (dir.to_path_buf(), like.map(Path::to_path_buf))
            })
            .collect();
        missing.reverse();
        missing
    }
}

/// `dir`, made for a file restored alone, made as `like`, its counterpart
/// in the trash, is: its mode, its `user.*` attributes, its times. After
/// the file moved in: its mode may forbid that.
fn made_like(dir: &Path, like: &Path) {
    let Ok(meta) = std::fs::metadata(like) else {
        return;
    };
    for (name, value) in kept_attributes(like).unwrap_or_default() {
        let _ = crate::fs::xattr::set(dir, name, &value);
    }
    if let (Ok(file), Ok(accessed), Ok(modified)) = (
        sys::open_to_set_times(dir),
        meta.accessed(),
        meta.modified(),
    ) {
        let _ = file.set_times(
            std::fs::FileTimes::new()
                .set_accessed(accessed)
                .set_modified(modified),
        );
    }
    let _ = std::fs::set_permissions(dir, meta.permissions());
}

impl super::Kind for RestorePart {
    const KIND: &'static str = "restore_part";
}

impl super::Effect for RestorePart {
    fn describe(&self) -> Change {
        Change::Restore {
            path: self.path.clone(),
        }
    }
    /// It, in the trashed folder.
    fn brings_from(&self) -> Option<PathBuf> {
        Some(self.entry.path.clone())
    }
    fn check(&self, ctx: &StepContext) -> StepStatus {
        let src = match self.entry.resolve() {
            Ok(src) => src,
            Err(ResolveError::Gone(_)) if exists(&self.path) => return StepStatus::AlreadyDone,
            Err(ResolveError::Gone(_)) => {
                return conflict(words!(
                    op_no_longer_in_trash = "{ $name } is no longer in the trash",
                    name = name_of(&self.entry.path),
                    note = "Why a file of a trashed folder is not restored: it left the trash."
                ));
            }
            Err(e) => return conflict_of(e),
        };
        let Some(inside) = in_trash(&src) else {
            return conflict(not_in_trash(&src));
        };
        if self.path.starts_with(&inside.item) {
            return conflict(words!(
                op_would_stay_in_trash = "{ $name } would stay in the trash",
                name = name_of(&self.path),
                note = "Why a file of a trashed folder is not restored: where it would go is in the trash."
            ));
        }
        if exists(&self.path) {
            return conflict(there_again(&self.path));
        }
        // An item's companions are items of their own, which go only where
        // their places are free.
        if !inside.inner.as_os_str().is_empty()
            && let Some(taken) = along_conflict(ctx, &along(ctx, &src, &self.path))
        {
            return taken;
        }
        // The first folder above that is there must be one.
        if let Some(there) = self.path.ancestors().skip(1).find(|dir| exists(dir))
            && !there.is_dir()
        {
            return conflict(not_a_folder(there));
        }
        pending(true)
    }
    fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let src = self.entry.resolve()?;
        let inside = in_trash(&src).ok_or_else(|| anyhow::Error::new(not_in_trash(&src)))?;
        let item = inside.inner.as_os_str().is_empty();
        // An item of the trash: the companions trashed with it, found
        // while the trash still says where it came from and when.
        let places = companion_places(ctx, &self.path);
        let with = if item && !places.is_empty() {
            trashed_from(ctx, &inside.info).map_or_else(Vec::new, |(original, when)| {
                take_trashed_with(ctx, &companion_places(ctx, &original), when, |n| {
                    places.get(n).is_some_and(|p| !exists(p))
                })
            })
        } else {
            Vec::new()
        };
        let info_text = if item {
            Some(read_record(&inside.info).with_context(|| record_unread(&inside.info))?)
        } else {
            None
        };
        let folders = self.folders(&src, inside.inner.components().count());
        let undo_folders = |made: &[(PathBuf, Option<PathBuf>)]| {
            for (dir, _) in made.iter().rev() {
                let _ = std::fs::remove_dir(dir);
            }
        };
        for (n, (dir, _)) in folders.iter().enumerate() {
            if let Err(e) = std::fs::create_dir(dir) {
                undo_folders(&folders[..n]);
                return Err(e).with_context(|| folder_path_not_made(dir));
            }
        }
        let companions = if item {
            Vec::new()
        } else {
            along(ctx, &src, &self.path)
        };
        let moved = match move_entry(ctx, &src, &self.path, &companions) {
            Ok(moved) => moved,
            Err(e) => {
                undo_folders(&folders);
                return Err(not_restored(&self.path).because(e));
            }
        };
        if item {
            ctx.trash_taken(&inside.info);
            let _ = record_goes(ctx, &moved, &src, &inside.info);
        }
        let mut returned = Vec::new();
        for (n, found) in with {
            if let Ok(back) = Untrashed::take(ctx, &record_of_listed(&found), &places[n]) {
                returned.push(back);
            }
        }
        for (dir, like) in folders.iter().rev() {
            if let Some(like) = like {
                made_like(dir, like);
            }
        }
        Ok(Some(Box::new(ReturnPart {
            path: self.path.clone(),
            to: src,
            made: folders.into_iter().map(|(dir, _)| PathArg(dir)).collect(),
            info_text,
            companions: returned,
        })))
    }
}

/// Put back in the trash what `RestorePart` took out, with its companions:
/// an item of the trash with what the trash said of it, and the folders
/// the restore made go while empty.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ReturnPart {
    /// Where it is, out of the trash.
    #[serde(with = "crate::os_path")]
    #[schemars(with = "PathArg")]
    pub path: PathBuf,
    /// Its place in the trash.
    #[serde(with = "crate::os_path")]
    #[schemars(with = "PathArg")]
    pub to: PathBuf,
    /// The folders the restore made, outermost first.
    pub made: Vec<PathArg>,
    /// What the trash said of it, when it was an item of the trash (not
    /// inside a trashed folder): its record, written again.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::os_path::bytes_opt"
    )]
    #[schemars(with = "Option<String>")]
    pub info_text: Option<Vec<u8>>,
    /// The companions trashed with it that went beside it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub companions: Vec<Untrashed>,
}

impl super::Kind for ReturnPart {
    const KIND: &'static str = "return_part";
}

impl ReturnPart {
    /// The folders made that are there, opened to their owner while the
    /// file leaves them and they go: made as their counterparts in the
    /// trash are, they may be read-only. Each with the mode to give back
    /// to it if it stays.
    fn open_made(&self) -> Vec<(PathBuf, std::fs::Permissions)> {
        self.made
            .iter()
            .filter_map(|dir| {
                let meta = std::fs::symlink_metadata(dir)
                    .ok()
                    .filter(std::fs::Metadata::is_dir)?;
                let mode = meta.permissions();
                if !sys::open_to_owner(&mode) {
                    std::fs::set_permissions(dir, sys::opened_to_owner(&mode)).ok()?;
                }
                Some((dir.to_path_buf(), mode))
            })
            .collect()
    }

    fn put_back(&self, ctx: &StepContext) -> anyhow::Result<()> {
        let companions = if self.info_text.is_some() {
            Vec::new()
        } else {
            along(ctx, &self.path, &self.to)
        };
        move_entry(ctx, &self.path, &self.to, &companions)?;
        ctx.forget_trash();
        if let (Some(text), Some(info)) = (&self.info_text, trash_info_of(&self.to))
            && !exists(&info)
        {
            std::fs::write(&info, text).with_context(|| record_unwritten(&info))?;
        }
        for companion in &self.companions {
            companion.put_back(ctx)?;
        }
        Ok(())
    }
}

impl super::Effect for ReturnPart {
    fn describe(&self) -> Change {
        Change::Trash {
            path: self.path.clone(),
        }
    }
    fn check(&self, ctx: &StepContext) -> StepStatus {
        if !exists(&self.path) {
            return if exists(&self.to) {
                StepStatus::AlreadyDone
            } else {
                conflict(no_longer_there(&self.path))
            };
        }
        if exists(&self.to) {
            return conflict(words!(
                op_place_taken_in_trash = "another { $name } has taken its place in the trash",
                name = name_of(&self.to),
                note = "Why a file restored from a trashed folder is not put back in it: another file took its place there."
            ));
        }
        if !self.to.parent().is_some_and(Path::is_dir) {
            return conflict(words!(
                op_trash_folder_gone = "the folder of the trash { $name } was in is gone",
                name = name_of(&self.path),
                note = "Why a file restored from a trashed folder is not put back in it: that folder left the trash."
            ));
        }
        if self.info_text.is_some() {
            return pending(true);
        }
        along_conflict(ctx, &along(ctx, &self.path, &self.to)).unwrap_or(pending(true))
    }
    fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let opened = self.open_made();
        let result = self.put_back(ctx);
        if result.is_ok() {
            for dir in self.made.iter().rev() {
                // Only while empty: what was put there since stays.
                let _ = std::fs::remove_dir(dir);
            }
        }
        for (dir, mode) in opened {
            if exists(&dir) {
                let _ = std::fs::set_permissions(&dir, mode);
            }
        }
        result?;
        let entry = EntryRef::from_path(&self.to)
            .ok_or_else(|| anyhow::Error::new(not_in_trash(&self.to)))?;
        Ok(Some(Box::new(RestorePart {
            entry,
            path: self.path.clone(),
        })))
    }
}

// ------------------------------------------------------------------ delete

/// Permanent deletion. Irreversible.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Delete {
    pub entry: EntryRef,
}

impl super::Kind for Delete {
    const KIND: &'static str = "delete";
}

impl super::Effect for Delete {
    fn footprint(
        &self,
        volumes: &super::footprint::Volumes,
    ) -> Option<super::footprint::Footprint> {
        let path = &self.entry.path;
        Some(
            super::footprint::Footprint::default()
                .made_with(path, volumes)
                .heavy_if(volumes.is_folder(path))
                .store_if(in_trash(path).is_some(), super::footprint::TRASH),
        )
    }
    fn describe(&self) -> Change {
        Change::Delete {
            path: self.entry.path.clone(),
        }
    }
    fn check(&self, ctx: &StepContext) -> StepStatus {
        match ctx.resolve(&self.entry) {
            Ok(path) if ctx.was_removed_ahead(&path) => pending(false),
            Ok(path) => mounted_there(&path).map_or_else(|| pending(false), conflict),
            Err(ResolveError::Gone(_)) => StepStatus::AlreadyDone,
            Err(e) => conflict_of(e),
        }
    }
    fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let src = ctx.resolve(&self.entry)?;
        // Removed with others of its folder ahead (`transfer::remove_ahead`).
        if ctx.take_removed_ahead(&src) {
            forget_trashed(&src);
            return Ok(None);
        }
        let companions = ctx.companions().existing(&src);
        remove_recursive(&src).couldnt(|| {
            Couldnt::new(
                &src,
                words!(
                    op_not_all_deleted = "{ $name } could not all be deleted",
                    name = name_of(&src),
                    note = "What a step deleting for good could not do: remove a folder with all it holds; why follows."
                ),
            )
        })?;
        forget_trashed(&src);
        for companion in companions {
            if remove_recursive(&companion.path).is_ok() {
                forget_trashed(&companion.path);
            }
        }
        Ok(None)
    }
    fn undoable(&self) -> bool {
        false
    }
}

/// A file of a trash is renamed or moved within the trash only by being
/// restored: under another name, or elsewhere, its `.trashinfo` would
/// speak of nothing and the trash would hold a file it says nothing of.
/// Moved out of the trash, it takes the `.trashinfo` along
/// (`Move::restoring`).
fn trashed(path: &Path) -> Option<StepStatus> {
    trash_info_of(path).map(|_| conflict(in_the_trash_first(path)))
}

/// A file deleted from the trash takes what the trash said of it along:
/// left behind, its `.trashinfo` would list a file that is not there.
fn forget_trashed(path: &Path) {
    if let Some(info) = trash_info_of(path) {
        let _ = std::fs::remove_file(info);
    }
}

// ------------------------------------------------------------------ mkdir / rmdir

/// Create a directory (parent must exist).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Mkdir {
    #[serde(with = "crate::os_path")]
    #[schemars(with = "PathArg")]
    pub path: PathBuf,
}

impl super::Kind for Mkdir {
    const KIND: &'static str = "mkdir";
}

/// Why a new folder or file cannot be made at `path`, whatever is there:
/// a name climbing out of its folder, none, or one not valid.
fn refuse_new(path: &Path) -> Option<StepStatus> {
    if path.components().any(|c| c == Component::ParentDir) {
        return Some(conflict(words!(
            op_path_climbs = "{ $path } contains “..”",
            path = crate::words::Arg::Path(path.display().to_string()),
            note = "Why a new folder or file is not made: its path climbs out of the folder it is made in."
        )));
    }
    match path.file_name().map(invalid_name) {
        None => Some(conflict(words!(
            op_path_no_name = "{ $path } has no name",
            path = crate::words::Arg::Path(path.display().to_string()),
            note = "Why a new folder or file is not made: its path ends without a name."
        ))),
        Some(invalid) => invalid,
    }
}

impl super::Effect for Mkdir {
    fn footprint(&self, _: &super::footprint::Volumes) -> Option<super::footprint::Footprint> {
        Some(super::footprint::Footprint::default().made(&self.path))
    }
    fn describe(&self) -> Change {
        Change::Create {
            path: self.path.clone(),
            directory: true,
            words: None,
        }
    }
    fn check(&self, ctx: &StepContext) -> StepStatus {
        if let Some(refused) = refuse_new(&self.path) {
            return refused;
        }
        match std::fs::symlink_metadata(&self.path) {
            Ok(m) if m.is_dir() => StepStatus::AlreadyDone,
            Ok(_) => conflict(there_not_a_folder(&self.path)),
            Err(_) if self.path.parent().is_some_and(|p| ctx.is_folder(p)) => pending(true),
            Err(_) => conflict(no_folder_to_go_in()),
        }
    }
    fn apply(&self, _ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        std::fs::create_dir(&self.path).with_context(|| folder_not_made(&self.path))?;
        Ok(Some(Box::new(Rmdir {
            path: self.path.clone(),
        })))
    }
}

/// Remove an empty directory.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Rmdir {
    #[serde(with = "crate::os_path")]
    #[schemars(with = "PathArg")]
    pub path: PathBuf,
}

impl super::Kind for Rmdir {
    const KIND: &'static str = "rmdir";
}

impl super::Effect for Rmdir {
    fn footprint(&self, _: &super::footprint::Volumes) -> Option<super::footprint::Footprint> {
        Some(super::footprint::Footprint::default().made(&self.path))
    }
    fn describe(&self) -> Change {
        Change::Remove {
            path: self.path.clone(),
        }
    }
    fn check(&self, ctx: &StepContext) -> StepStatus {
        if let Ok(meta) = std::fs::symlink_metadata(&self.path)
            && meta.is_dir()
            && !sys::directory_can_be_recreated(&meta)
        {
            return conflict(foreign_folder());
        }
        // Emptied by the plan's earlier steps (a folder a merge moved
        // everything out of, ADR-054) is empty.
        match ctx.empty_after(&self.path) {
            Ok(true) => pending(true),
            Ok(false) => conflict(words!(
                op_not_empty = "{ $name } is not empty",
                name = name_of(&self.path),
                note = "Why a folder is not removed: it holds something (a step removes only an empty one)."
            )),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => StepStatus::AlreadyDone,
            Err(e) => conflict_of(e),
        }
    }
    fn apply(&self, _ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        // Snapshot before removal: an unreadable attribute is not an absent
        // attribute, and must leave the original available for Undo/retry.
        let meta = std::fs::symlink_metadata(&self.path)?;
        anyhow::ensure!(meta.is_dir(), not_a_folder(&self.path));
        anyhow::ensure!(sys::directory_can_be_recreated(&meta), foreign_folder());
        let inverse = RestoreDirectory {
            path: self.path.clone(),
            permissions: sys::DirectoryPermissions::read(&meta.permissions()),
            attributes: DirectoryAttribute::read(&self.path)?,
        };
        std::fs::remove_dir(&self.path).with_context(|| {
            words!(
                op_folder_not_removed = "the folder { $name } could not be removed",
                name = name_of(&self.path),
                note = "What a step could not do: remove an empty folder; why follows."
            )
        })?;
        Ok(Some(Box::new(inverse)))
    }
}

/// A native attribute retained without converting its name to text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
struct DirectoryAttribute {
    #[serde(with = "crate::os_path")]
    #[schemars(with = "PathArg")]
    name: OsString,
    value: Vec<u8>,
}

impl DirectoryAttribute {
    fn read(path: &Path) -> anyhow::Result<Vec<Self>> {
        Ok(kept_attributes(path)?
            .into_iter()
            .map(|(name, value)| Self { name, value })
            .collect())
    }
}

/// The inverse of removing an empty directory. This is a separate operation
/// so old `mkdir`/`rmdir` plans keep their schema and ordinary creation keeps
/// the platform's defaults. Every successful redo captures a fresh inverse.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
struct RestoreDirectory {
    #[serde(with = "crate::os_path")]
    #[schemars(with = "PathArg")]
    path: PathBuf,
    permissions: sys::DirectoryPermissions,
    attributes: Vec<DirectoryAttribute>,
}

impl super::Kind for RestoreDirectory {
    const KIND: &'static str = "restore_directory";
}

impl RestoreDirectory {
    fn metadata(&self, path: &Path) -> anyhow::Result<()> {
        // A newly created folder may inherit attributes. Restore absence too,
        // keeping the exact user-attribute snapshot, before restricting access.
        for (name, _) in kept_attributes(path)? {
            if !self.attributes.iter().any(|saved| saved.name == name) {
                match crate::fs::xattr::remove(path, &name) {
                    Ok(()) => {}
                    Err(error) if crate::fs::xattr::missing(&error) => {}
                    Err(error) => return Err(error.into()),
                }
            }
        }
        for attribute in &self.attributes {
            crate::fs::xattr::set(path, &attribute.name, &attribute.value).with_context(|| {
                words!(
                    op_folder_attribute_not_restored = "the folder's attribute { $attribute } could not be restored",
                    attribute = crate::words::Name(attribute.name.display().to_string()),
                    note = "What the undo of a folder's removal could not do: give it back an extended attribute it had. attribute: the attribute's name (user.xdg.tags); why follows."
                )
            })?;
        }
        self.permissions.apply(path)?;
        Ok(())
    }

    fn matches(&self, meta: &std::fs::Metadata) -> anyhow::Result<bool> {
        if self.permissions != sys::DirectoryPermissions::read(&meta.permissions()) {
            return Ok(false);
        }
        let attributes = DirectoryAttribute::read(&self.path)?;
        Ok(attributes.len() == self.attributes.len()
            && self
                .attributes
                .iter()
                .all(|attribute| attributes.contains(attribute)))
    }
}

impl super::Effect for RestoreDirectory {
    fn footprint(&self, _: &super::footprint::Volumes) -> Option<super::footprint::Footprint> {
        Some(super::footprint::Footprint::default().made(&self.path))
    }
    fn describe(&self) -> Change {
        Change::Create {
            path: self.path.clone(),
            directory: true,
            words: None,
        }
    }
    fn held(&self) -> usize {
        self.attributes
            .iter()
            .map(|attribute| attribute.name.as_encoded_bytes().len() + attribute.value.len())
            .sum()
    }
    fn check(&self, ctx: &StepContext) -> StepStatus {
        if let Some(refused) = refuse_new(&self.path) {
            return refused;
        }
        match std::fs::symlink_metadata(&self.path) {
            Ok(meta) if meta.is_dir() => match self.matches(&meta) {
                Ok(true) => StepStatus::AlreadyDone,
                Ok(false) => conflict(words!(
                    op_folder_attributes_differ =
                        "{ $name } is there with different folder attributes",
                    name = name_of(&self.path),
                    note = "Why the undo of a folder's removal does not go on: a folder of its name is there again, with other permissions or attributes."
                )),
                Err(error) => conflict_of(error),
            },
            Ok(_) => conflict(there_not_a_folder(&self.path)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if self
                    .path
                    .parent()
                    .is_some_and(|parent| ctx.is_folder(parent))
                {
                    pending(true)
                } else {
                    conflict(no_folder_to_go_in())
                }
            }
            Err(error) => conflict_of(error),
        }
    }
    fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let parent = self.path.parent().with_context(no_folder_to_go_in)?;
        if crate::fs::reach::writes_in_place(parent) {
            // Cameras cannot rename a staged directory. As with in-place
            // copies, roll back only what this step created, while empty.
            std::fs::create_dir(&self.path)?;
            let made = id_of(&self.path);
            if let Err(error) = self.metadata(&self.path) {
                let removed = made.is_some()
                    && id_of(&self.path) == made
                    && std::fs::remove_dir(&self.path).is_ok();
                return Err(if removed {
                    error
                } else {
                    error.context(words!(
                        op_partly_restored_folder = "the partially restored folder remains at { $path }",
                        path = crate::words::Arg::Path(self.path.display().to_string()),
                        note = "Said after why the undo of a folder's removal failed: the folder it began to make could not be removed. path: where it is."
                    ))
                });
            }
        } else {
            let staging = ctx.stage(parent)?;
            #[cfg(unix)]
            staging.place_empty_directory(&self.path, |made| {
                self.metadata(made)?;
                ctx.check_cancelled()?;
                Ok(())
            })?;
            // Windows directory read-only attributes do not restrict moving
            // between parents; keep the staging lock outside the new folder.
            #[cfg(not(unix))]
            {
                let made = staging.path().join("folder");
                std::fs::create_dir(&made)?;
                self.metadata(&made)?;
                ctx.check_cancelled()?;
                staging.place(&made, &self.path)?;
            }
        }
        Ok(Some(Box::new(Rmdir {
            path: self.path.clone(),
        })))
    }
}

// ------------------------------------------------------------------ new file

/// Create an empty file (parent must exist), never over an existing one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct NewFile {
    #[serde(with = "crate::os_path")]
    #[schemars(with = "PathArg")]
    pub path: PathBuf,
}

impl super::Kind for NewFile {
    const KIND: &'static str = "new_file";
}

impl super::Effect for NewFile {
    fn footprint(&self, _: &super::footprint::Volumes) -> Option<super::footprint::Footprint> {
        Some(super::footprint::Footprint::default().made(&self.path))
    }
    fn describe(&self) -> Change {
        Change::Create {
            path: self.path.clone(),
            directory: false,
            words: None,
        }
    }
    fn check(&self, ctx: &StepContext) -> StepStatus {
        if let Some(refused) = refuse_new(&self.path) {
            return refused;
        }
        match std::fs::symlink_metadata(&self.path) {
            // What this step makes: applied already.
            Ok(m) if m.is_file() && m.len() == 0 => StepStatus::AlreadyDone,
            Ok(_) => conflict(already_there(&self.path)),
            Err(_) if self.path.parent().is_some_and(|p| ctx.is_folder(p)) => pending(true),
            Err(_) => conflict(no_folder_to_go_in()),
        }
    }
    fn apply(&self, _ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&self.path)
            .couldnt(|| {
                Couldnt::new(
                    &self.path,
                    words!(
                        op_not_made = "{ $name } could not be made",
                        name = name_of(&self.path),
                        note = "What a step could not do: make a new empty file; why follows."
                    ),
                )
            })?;
        Ok(Some(Box::new(RemoveEmptyFile {
            path: self.path.clone(),
        })))
    }
}

/// Remove a file that is still empty: the undo of [`NewFile`], which never
/// takes what was written into the file since.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RemoveEmptyFile {
    #[serde(with = "crate::os_path")]
    #[schemars(with = "PathArg")]
    pub path: PathBuf,
}

impl super::Kind for RemoveEmptyFile {
    const KIND: &'static str = "remove_empty_file";
}

impl super::Effect for RemoveEmptyFile {
    fn footprint(&self, _: &super::footprint::Volumes) -> Option<super::footprint::Footprint> {
        Some(super::footprint::Footprint::default().made(&self.path))
    }
    fn describe(&self) -> Change {
        Change::Remove {
            path: self.path.clone(),
        }
    }
    fn check(&self, _ctx: &StepContext) -> StepStatus {
        match std::fs::symlink_metadata(&self.path) {
            Ok(m) if m.is_file() && m.len() == 0 => pending(true),
            Ok(m) if m.is_file() => conflict(words!(
                op_no_longer_empty = "{ $name } is no longer empty",
                name = name_of(&self.path),
                note = "Why the undo of a new file does not remove it: something was written in it since."
            )),
            Ok(_) => conflict(words!(
                op_not_a_file = "{ $name } is not a file",
                name = name_of(&self.path),
                note =
                    "Why the undo of a new file does not go on: something else has its name now."
            )),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => StepStatus::AlreadyDone,
            Err(e) => conflict_of(e),
        }
    }
    fn apply(&self, _ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        std::fs::remove_file(&self.path).couldnt(|| not_removed(&self.path))?;
        Ok(Some(Box::new(NewFile {
            path: self.path.clone(),
        })))
    }
}

// ------------------------------------------------------------------ link

/// Make a link in `dest_dir` to the entry, under its name or `new_name`:
/// what a drop's Link Here makes. The link says the entry's whole path, so
/// it leads there wherever the link itself is moved.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Link {
    pub entry: EntryRef,
    #[serde(with = "crate::os_path")]
    #[schemars(with = "PathArg")]
    pub dest_dir: PathBuf,
    /// Name of the link; the entry's name when `None` (a name kept beside
    /// one taken, ADR-054).
    #[serde(default, with = "optional_name")]
    #[schemars(with = "Option<PathArg>")]
    pub new_name: Option<OsString>,
}

impl Link {
    pub fn new(entry: EntryRef, dest_dir: PathBuf) -> Self {
        Self {
            entry,
            dest_dir,
            new_name: None,
        }
    }

    pub(super) fn target(&self, src: &Path) -> PathBuf {
        match &self.new_name {
            Some(n) => self.dest_dir.join(n),
            None => self.dest_dir.join(file_name(src)),
        }
    }
}

impl super::Kind for Link {
    const KIND: &'static str = "link";
}

impl super::Effect for Link {
    fn footprint(&self, _: &super::footprint::Volumes) -> Option<super::footprint::Footprint> {
        Some(
            super::footprint::Footprint::default()
                .settles_name(&self.target(&self.entry.path), false),
        )
    }
    fn describe(&self) -> Change {
        Change::Link {
            from: self.entry.path.clone(),
            to: self.target(&self.entry.path),
        }
    }
    fn check(&self, ctx: &StepContext) -> StepStatus {
        if let Some(invalid) = self.new_name.as_deref().and_then(invalid_name) {
            return invalid;
        }
        if !ctx.is_folder(&self.dest_dir) {
            return conflict(not_a_folder(&self.dest_dir));
        }
        let src = match self.entry.resolve() {
            Ok(s) => s,
            Err(e) => return conflict_of(e),
        };
        let link = self.target(&src);
        if ctx.made(&link) {
            return conflict(there_by_earlier_step(&link));
        }
        if !ctx.exists(&link) {
            return pending(true);
        }
        match std::fs::read_link(&link) {
            // What this step makes: applied already.
            Ok(says) if says == src => StepStatus::AlreadyDone,
            _ => conflict(already_there(&link)),
        }
    }
    fn apply(&self, _ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        if let Some(name) = &self.new_name {
            valid_file_name(name)?;
        }
        let src = self.entry.resolve()?;
        let link = self.target(&src);
        sys::symlink_to(&src, &link, src.is_dir()).couldnt(|| {
            Couldnt::new(
                &src,
                words!(
                    op_not_linked = "{ $name } could not be linked in { $folder }",
                    name = name_of(&src),
                    folder = name_of(&self.dest_dir),
                    note = "What a step could not do: make a link to a file in a folder, by their names; why follows."
                ),
            )
        })?;
        Ok(Some(Box::new(Unlink {
            path: link,
            entry: EntryRef::new(self.entry.id, src).with_only_name(self.entry.only_name),
        })))
    }
}

/// Take away a link [`Link`] made, while it still says what it was made
/// to say: the undo of a Link Here, which never takes a link changed
/// since, nor what it leads to.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Unlink {
    #[serde(with = "crate::os_path")]
    #[schemars(with = "PathArg")]
    pub path: PathBuf,
    /// What the link leads to, at the path it says.
    pub entry: EntryRef,
}

impl super::Kind for Unlink {
    const KIND: &'static str = "unlink";
}

impl super::Effect for Unlink {
    fn footprint(&self, _: &super::footprint::Volumes) -> Option<super::footprint::Footprint> {
        Some(super::footprint::Footprint::default().made(&self.path))
    }
    fn describe(&self) -> Change {
        Change::Remove {
            path: self.path.clone(),
        }
    }
    fn check(&self, _ctx: &StepContext) -> StepStatus {
        match std::fs::read_link(&self.path) {
            Ok(says) if says == self.entry.path => pending(true),
            Ok(_) => conflict(words!(
                op_link_elsewhere = "{ $name } leads elsewhere now",
                name = name_of(&self.path),
                note = "Why the undo of a link does not remove it: it was changed to lead elsewhere since."
            )),
            Err(_) if !exists(&self.path) => StepStatus::AlreadyDone,
            Err(_) => conflict(words!(
                op_no_longer_a_link = "{ $name } is no longer a link",
                name = name_of(&self.path),
                note = "Why the undo of a link does not go on: something else has its name now."
            )),
        }
    }
    fn apply(&self, _ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        sys::remove_link(&self.path).couldnt(|| not_removed(&self.path))?;
        let name = file_name(&self.path);
        Ok(Some(Box::new(Link {
            entry: self.entry.clone(),
            dest_dir: self.path.parent().unwrap_or(Path::new("/")).to_path_buf(),
            new_name: (name != file_name(&self.entry.path)).then_some(name),
        })))
    }
}

/// Register every built-in operation kind under the `core` owner.
pub fn register_builtin(reg: &OpRegistry) {
    reg.register::<Rename>("core");
    reg.register::<Move>("core");
    reg.register::<Copy>("core");
    reg.register::<Swap>("core");
    reg.register::<Trash>("core");
    reg.register::<Restore>("core");
    reg.register::<Unmerge>("core");
    reg.register::<RestorePart>("core");
    reg.register::<ReturnPart>("core");
    reg.register::<Delete>("core");
    reg.register::<Mkdir>("core");
    reg.register::<Rmdir>("core");
    reg.register::<RestoreDirectory>("core");
    reg.register::<NewFile>("core");
    reg.register::<RemoveEmptyFile>("core");
    reg.register::<Uncopy>("core");
    reg.register::<Unmove>("core");
    reg.register::<Untrash>("core");
    reg.register::<Link>("core");
    reg.register::<Unlink>("core");
    reg.register::<super::access::SetAccess>("core");
    reg.register::<super::access::AccessBack>("core");
}

#[cfg(test)]
mod tests {
    /// A copy cut short goes on from before what was read at once as it
    /// was cut: a mount's zeroes there, under a whole range after them
    /// whose end reads as the source's, are never kept (sshfs, run under
    /// load: 32 KiB zeroed at the end of the seventh range of eight).
    #[cfg(unix)]
    #[test]
    fn a_copy_cut_short_goes_on_from_before_what_was_read_at_once() {
        use std::os::unix::fs::FileExt;
        let tmp = tempfile::tempdir().unwrap();
        let source: Vec<u8> = (0..48u32 << 20)
            .map(|i| u8::try_from(i % 251).unwrap())
            .collect();
        let (src, dst) = (tmp.path().join("it's big.bin"), tmp.path().join("partial"));
        std::fs::write(&src, &source).unwrap();
        // 40 MiB came, 32 KiB of them zeroes just before 36 MiB.
        let mut came = source[..40 << 20].to_vec();
        came[(36 << 20) - (32 << 10)..36 << 20].fill(0);
        std::fs::write(&dst, &came).unwrap();
        let sf = std::fs::File::open(&src).unwrap();
        let df = std::fs::File::open(&dst).unwrap();
        let len = source.len() as u64;
        // Its end reads as the source's: gone on from its end, the zeroes
        // kept, were nothing read at once.
        assert_eq!(goes_on_from(&sf, &df, len, 0).unwrap(), 40 << 20);
        // Read at once, its last 32 MiB are set aside: gone on from 8 MiB.
        let from = goes_on_from(&sf, &df, len, super::super::ranges::IN_FLIGHT).unwrap();
        assert_eq!(from, 8 << 20);
        let mut kept = vec![0; usize::try_from(from).unwrap()];
        df.read_exact_at(&mut kept, 0).unwrap();
        assert!(
            kept == source[..kept.len()],
            "what it goes on from is the source's"
        );
        // Kept no further than what was read at once: started again.
        std::fs::write(&dst, &came[..32 << 20]).unwrap();
        let df = std::fs::File::open(&dst).unwrap();
        assert_eq!(goes_on_from(&sf, &df, len, 32 << 20).unwrap(), 0);
    }

    /// A copy asks for about a quarter second's worth at once: more on a
    /// fast disk, up to the kernel's fast path; less from a slow server,
    /// never under a few round trips' worth (ADR-113).
    #[test]
    fn a_copy_asks_for_what_comes_in_a_quarter_second() {
        use std::time::Duration;
        let mut chunk = Chunk::new(COPY_CHUNK);
        assert_eq!(chunk.size, 1 << 20);
        for _ in 0..5 {
            chunk.took(Duration::from_millis(2));
        }
        assert_eq!(chunk.size, COPY_CHUNK, "a fast disk: the most");
        for _ in 0..10 {
            chunk.took(Duration::from_secs(2));
        }
        assert_eq!(chunk.size, CHUNK_LEAST, "a slow server: the least");
        chunk.took(Duration::from_millis(250));
        assert_eq!(chunk.size, CHUNK_LEAST, "a quarter second: kept");
        let mut small = Chunk::new(64 << 10);
        small.took(Duration::from_secs(1));
        assert_eq!(small.size, 64 << 10, "never past its buffer");
    }

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    #[test]
    fn alternate_spellings_require_one_entry_not_just_one_file() {
        let one = [OsString::from("Case.txt")];
        assert!(same_spelling(OsStr::new("Case.txt"), OsStr::new("CASE.TXT"), &one).unwrap());
        assert!(same_spelling(OsStr::new("case.txt"), OsStr::new("CASE.TXT"), &one).unwrap());
        let two = [OsString::from("Case.txt"), OsString::from("Other.txt")];
        assert!(!same_spelling(OsStr::new("Case.txt"), OsStr::new("Other.txt"), &two).unwrap());
        assert!(same_spelling(OsStr::new("Case.txt"), OsStr::new("OTHER.TXT"), &two).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn bind_mount_roots_reveal_descendants_outside_the_visible_source_path() {
        let mounts = crate::fs::volume::mountinfo(
            "\
1 0 8:1 / / rw - ext4 /dev/test rw\n\
2 1 8:1 /tree /alias rw - ext4 /dev/test rw\n\
3 1 8:1 /tree/child /nested-alias rw - ext4 /dev/test rw\n\
4 1 8:2 /tree/child /other-volume rw - ext4 /dev/other rw\n\
5 1 8:1 /outside /tree/elsewhere rw - ext4 /dev/test rw\n",
        );
        let inside = |source: &str, target: &str| {
            inside_mounts(Path::new(source), Path::new(target), &mounts)
        };
        assert!(inside("/tree", "/alias/new"));
        assert!(inside("/tree", "/nested-alias/deeper/new"));
        assert!(inside("/alias", "/tree/child/new"));
        assert!(!inside("/tree", "/other-volume/new"));
        assert!(!inside("/tree", "/tree/elsewhere/new"));
        assert!(!inside("/tree/child", "/alias/sibling/new"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_directory_restore_parent_preserves_native_attribute_names() {
        let base = dirs::home_dir().unwrap().join(".cache/skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        let tmp = tempfile::tempdir_in(base).unwrap();
        let trash = tmp.path().join(".Trash-1000");
        let item = trash.join("files/old");
        std::fs::create_dir_all(&item).unwrap();
        std::fs::create_dir_all(trash.join("info")).unwrap();
        std::fs::write(item.join("a.txt"), "a").unwrap();
        std::fs::write(
            trash.join("info/old.trashinfo"),
            "[Trash Info]\nPath=docs/old\n",
        )
        .unwrap();
        let name = crate::os_path::from_bytes(b"user.test-\xff".to_vec());
        for name in [name.as_os_str(), OsStr::new("user.test")] {
            crate::fs::xattr::set(&item, name, b"kept").unwrap();
        }
        let docs = tmp.path().join("docs");
        std::fs::create_dir(&docs).unwrap();
        let part = RestorePart {
            entry: EntryRef::from_path(item.join("a.txt")).unwrap(),
            path: docs.join("old/a.txt"),
        };
        super::super::Effect::apply(&part, &StepContext::detached()).unwrap();
        for name in [name.as_os_str(), OsStr::new("user.test")] {
            assert_eq!(
                crate::fs::xattr::get(docs.join("old"), name).unwrap(),
                Some(b"kept".to_vec())
            );
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_directory_restore_with_unwritable_attributes_leaves_no_partial_folder() {
        let base = dirs::home_dir().unwrap().join(".cache/skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        let tmp = tempfile::tempdir_in(base).unwrap();
        let restore = RestoreDirectory {
            path: tmp.path().join("restore"),
            permissions: sys::DirectoryPermissions::Unix { mode: 0o500 },
            attributes: vec![DirectoryAttribute {
                name: "invalid-attribute-namespace".into(),
                value: b"value".to_vec(),
            }],
        };
        assert!(super::super::Effect::apply(&restore, &StepContext::detached()).is_err());
        assert!(!restore.path.exists());
        assert_eq!(
            std::fs::read_dir(tmp.path()).unwrap().count(),
            0,
            "staging removed too"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_camera_directory_restore_rolls_back_failed_metadata_in_place() {
        let base = dirs::home_dir().unwrap().join(".cache/skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        let tmp = tempfile::tempdir_in(base).unwrap();
        let camera = tmp.path().join("gphoto2:synthetic");
        std::fs::create_dir(&camera).unwrap();
        crate::fs::reach::pretend_camera(&camera);
        let restore = RestoreDirectory {
            path: camera.join("restore"),
            permissions: sys::DirectoryPermissions::Unix { mode: 0o500 },
            attributes: vec![DirectoryAttribute {
                name: "invalid-attribute-namespace".into(),
                value: b"value".to_vec(),
            }],
        };
        assert!(super::super::Effect::apply(&restore, &StepContext::detached()).is_err());
        assert!(!restore.path.exists());
        assert_eq!(std::fs::read_dir(camera).unwrap().count(), 0);
    }

    /// The undo of a restore that merged into a folder there goes on when
    /// a companion that came back with it was deleted since: nothing of it
    /// is put back, and what moved goes back to the trash.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_merged_restore_is_undone_without_a_companion_deleted_since() {
        let base = dirs::home_dir().unwrap().join(".cache/skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        let tmp = tempfile::tempdir_in(base).unwrap();
        let trash = tmp.path().join(".Trash-1000");
        std::fs::create_dir_all(trash.join("files/old  folder")).unwrap();
        std::fs::create_dir_all(trash.join("info")).unwrap();
        let docs = tmp.path().join("docs/old  folder");
        std::fs::create_dir_all(&docs).unwrap();
        std::fs::write(docs.join("it’s back.txt"), "back").unwrap();
        let said = b"[Trash Info]\nPath=docs/old%20%20folder\n".to_vec();
        let undo = Unmerge {
            path: docs.clone(),
            info: PathArg(trash.join("info/old  folder.trashinfo")),
            info_text: said.clone(),
            moved: vec![PathArg("it’s back.txt".into())],
            folders: Vec::new(),
            companions: vec![Untrashed {
                // Deleted since it came back.
                path: tmp.path().join("docs/._old  folder"),
                info: PathArg(trash.join("info/._old  folder.trashinfo")),
                info_text: said,
            }],
        };
        let ctx = StepContext::detached();
        assert_eq!(
            super::super::Effect::check(&undo, &ctx),
            StepStatus::Pending { reversible: true }
        );
        super::super::Effect::apply(&undo, &ctx).unwrap();
        assert!(trash.join("files/old  folder/it’s back.txt").is_file());
        assert!(trash.join("info/old  folder.trashinfo").is_file());
        assert!(
            !trash.join("info/._old  folder.trashinfo").exists(),
            "no record of nothing"
        );
        assert_eq!(
            super::super::Effect::check(&undo, &ctx),
            StepStatus::AlreadyDone
        );
    }

    /// A tree compared with what is there is read once a plan, while
    /// nothing of either changed: changed since (a byte, its time given
    /// back), it is read again.
    #[cfg(unix)]
    #[test]
    fn a_tree_compared_once_is_read_again_only_once_changed() {
        let tmp = tempfile::tempdir().unwrap();
        let (src, copy) = (
            tmp.path().join("it’s a.bin"),
            tmp.path().join("copy  of it.bin"),
        );
        std::fs::write(&src, vec![7u8; 1 << 20]).unwrap();
        std::fs::copy(&src, &copy).unwrap();
        let when = std::fs::metadata(&src).unwrap().modified().unwrap();
        let file = std::fs::File::options().write(true).open(&copy).unwrap();
        file.set_modified(when).unwrap();
        drop(file);
        // The dry run and the run: a context each, what they compared shared.
        let compared = std::sync::Arc::new(super::super::context::Compared::default());
        let pass = || StepContext::detached().with_compared(std::sync::Arc::clone(&compared));
        let ctx = pass();
        assert!(same_tree(&src, &copy, true, false, &ctx).unwrap());
        let key = (src.clone(), copy.clone(), true);
        let state = compared_state(&ctx, &src, &copy, &[], true).unwrap();
        assert_eq!(ctx.compared().found(&key, state), Some(true), "kept");
        let file = std::fs::File::options().write(true).open(&copy).unwrap();
        std::os::unix::fs::FileExt::write_all_at(&file, b"X", 0).unwrap();
        file.set_modified(when).unwrap();
        drop(file);
        assert!(
            !same_tree(&src, &copy, true, false, &pass()).unwrap(),
            "read again"
        );
    }

    /// A tree, and a copy of it there whose last file differs in its
    /// bytes alone (its length and time kept).
    #[cfg(unix)]
    fn tree_and_copy(tmp: &Path) -> (PathBuf, PathBuf) {
        let (src, dest) = (tmp.join("src"), tmp.join("dest"));
        for folder in ["a", "b", "z"] {
            for n in 0..10 {
                let rel = format!("tree/{folder}/it's {n}.bin");
                let bytes = format!("{folder}{n}").repeat(1000).into_bytes();
                for side in [&src, &dest] {
                    let path = side.join(&rel);
                    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                    std::fs::write(&path, &bytes).unwrap();
                }
                let when = std::fs::metadata(src.join(&rel))
                    .unwrap()
                    .modified()
                    .unwrap();
                let there = std::fs::File::options()
                    .write(true)
                    .open(dest.join(&rel))
                    .unwrap();
                if folder == "z" && n == 9 {
                    std::os::unix::fs::FileExt::write_all_at(&there, b"X", 0).unwrap();
                }
                there.set_modified(when).unwrap();
            }
        }
        (src, dest)
    }

    /// A folder merged into its copy, level by level, reads each file's
    /// bytes once and stats each entry at most twice a side: what a
    /// folder found of its items is what the merge asks of them next. It
    /// settles as ever: what is there whole is kept to be found done, the
    /// file that differs kept beside its namesake.
    #[cfg(unix)]
    #[test]
    fn a_merge_reads_each_file_once() {
        use std::sync::atomic::Ordering;
        let base = dirs::home_dir().unwrap().join(".cache/skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        let tmp = tempfile::tempdir_in(base).unwrap();
        let (src, dest) = tree_and_copy(tmp.path());
        let steps: Vec<Box<dyn Operation>> = vec![Box::new(Copy {
            entry: EntryRef::from_path(src.join("tree")).unwrap(),
            dest_dir: dest.clone(),
            new_name: None,
            preserve_attributes: false,
        })];
        let ctx = StepContext::detached();
        let (read, stated) = (
            BYTES_COMPARED.load(Ordering::Relaxed),
            STATED.load(Ordering::Relaxed),
        );
        let settled =
            super::super::clash::settle(&steps, &super::super::clash::Answer::default(), &ctx);
        let read = BYTES_COMPARED.load(Ordering::Relaxed) - read;
        let stated = STATED.load(Ordering::Relaxed) - stated;
        assert_eq!(read, 30, "each file's bytes once");
        // A side: the tree, its three folders and their 30 files.
        let entries = 2 * 34;
        assert!(stated <= 2 * entries, "{stated} stats of {entries} entries");
        let copies: Vec<(PathBuf, PathBuf)> = settled
            .steps
            .iter()
            .map(|step| match step.describe() {
                Change::Copy { from, to } => (from, to),
                other => panic!("{other:?}"),
            })
            .collect();
        let item = |rel: &str, to: &str| (src.join(rel), dest.join(to));
        let mut expected = vec![item("tree/a", "tree/a"), item("tree/b", "tree/b")];
        expected.extend((0..9).map(|n| {
            item(
                &format!("tree/z/it's {n}.bin"),
                &format!("tree/z/it's {n}.bin"),
            )
        }));
        expected.push(item("tree/z/it's 9.bin", "tree/z/it's 9 (copy).bin"));
        assert_eq!(copies, expected);
        assert_eq!(settled.clashes.len(), 3, "the tree, z, and its last file");
    }

    /// A copy from or to a remote finds what is there whole by its
    /// length, time, kind and permissions, its bytes unread; a move, and
    /// a copy between this machine's folders, reads them.
    #[cfg(target_os = "linux")]
    #[test]
    fn only_a_remote_copy_trusts_length_and_time() {
        use std::sync::atomic::Ordering;
        let base = dirs::home_dir().unwrap().join(".cache/skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        let tmp = tempfile::tempdir_in(base).unwrap();
        let server = tmp.path().join("server");
        crate::fs::reach::pretend_remote(&server);
        let src = tmp.path().join("here/it's a.bin");
        let (remote, local) = (
            server.join("it's a.bin"),
            tmp.path().join("there/it's a.bin"),
        );
        for path in [&src, &remote, &local] {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"0123456789").unwrap();
        }
        let when = std::fs::metadata(&src).unwrap().modified().unwrap();
        for path in [&remote, &local] {
            let file = std::fs::File::options().write(true).open(path).unwrap();
            std::os::unix::fs::FileExt::write_all_at(&file, b"X", 0).unwrap();
            file.set_modified(when).unwrap();
        }
        let ctx = StepContext::detached();
        let read = || BYTES_COMPARED.load(Ordering::Relaxed);
        let before = read();
        assert!(
            same_tree(&src, &remote, false, false, &ctx).unwrap(),
            "a copy"
        );
        assert_eq!(read() - before, 0, "its bytes unread");
        assert!(
            !same_tree(&src, &remote, false, true, &ctx).unwrap(),
            "a move"
        );
        assert_eq!(read() - before, 1);
        assert!(
            !same_tree(&src, &local, false, false, &ctx).unwrap(),
            "a copy here"
        );
        assert_eq!(read() - before, 2);
    }

    /// An item of a trash on another filesystem, a copy of `name` in
    /// `dir`, and its record, made as a trash there does.
    #[cfg(target_os = "linux")]
    fn trashed_copy(trash: &Path, dir: &Path, name: &str, text: &str) -> (PathBuf, PathBuf) {
        let (item, info) = (
            trash.join("files").join(name),
            trash.join("info").join(format!("{name}.trashinfo")),
        );
        std::fs::create_dir_all(trash.join("files")).unwrap();
        std::fs::create_dir_all(trash.join("info")).unwrap();
        std::fs::write(&item, text).unwrap();
        let path = dir.join(name);
        std::fs::write(
            &info,
            format!(
                "[Trash Info]\nPath={}\nDeletionDate=2026-10-28T12:00:00\n",
                path.display()
            ),
        )
        .unwrap();
        (item, info)
    }

    /// A trash onto another filesystem whose original's removal waited,
    /// and never came (the write-out failed): the original stayed, the
    /// trash holds a copy, listed. A restore alone, what its undo was,
    /// refuses to put the copy where the original is; its undo takes the
    /// copy and its record back, and Redo trashes the original. One whose
    /// original changed meanwhile keeps both, nothing to take back; one
    /// whose original went is a restore.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_trash_whose_original_stayed_is_undone_by_taking_its_copy_back() {
        let base = dirs::home_dir().unwrap().join(".cache/skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        let tmp = tempfile::tempdir_in(base).unwrap();
        let (dir, trash) = (tmp.path().join("l’été"), tmp.path().join(".Trash-1000"));
        std::fs::create_dir_all(&dir).unwrap();
        let name = "it’s a  file.txt";
        let original = dir.join(name);
        std::fs::write(&original, "words").unwrap();
        let (item, info) = trashed_copy(&trash, &dir, name, "words");
        let undo = Untrash {
            restore: Restore {
                path: original.clone(),
                item: Some(PathArg(info.clone())),
                landed: Vec::new(),
            },
            original: EntryRef::from_path(&original).unwrap(),
            copied_as: made_state(&original),
            made_as: made_state(&item),
        };
        let ctx = StepContext::detached();
        assert!(
            matches!(undo.restore.check(&ctx), StepStatus::Conflict { .. }),
            "a restore alone is refused"
        );
        assert!(matches!(undo.check(&ctx), StepStatus::Pending { .. }));
        let redo = undo.apply(&ctx).unwrap().expect("its redo");
        assert!(!item.exists() && !info.exists(), "the copy and its record");
        assert_eq!(std::fs::read_to_string(&original).unwrap(), "words");
        assert!(matches!(undo.check(&ctx), StepStatus::AlreadyDone));
        let trash_again = redo.as_any().and_then(|any| any.downcast_ref::<Trash>());
        assert_eq!(trash_again.map(|t| &t.entry.path), Some(&original));

        // Changed since it was copied: the trash keeps what it was.
        let (item, info) = trashed_copy(&trash, &dir, name, "words");
        std::fs::write(&original, "more words").unwrap();
        assert!(matches!(undo.check(&ctx), StepStatus::AlreadyDone));
        assert!(item.exists() && info.exists());
        // Gone, as the removal it waited for came: the restore's.
        std::fs::remove_file(&original).unwrap();
        assert!(undo.copy().is_none());
    }

    /// An item moved out of a trash takes its record along once it left:
    /// at once, or where its removal waits for a write-out, once that
    /// came; should it not come, the record stays, and the trash lists
    /// what it holds.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_record_goes_once_its_item_left_the_trash() {
        let base = dirs::home_dir().unwrap().join(".cache/skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        let tmp = tempfile::tempdir_in(base).unwrap();
        let trash = tmp.path().join(".Trash-1000");
        let waits = || Moved {
            waits: Some(super::super::context::CopiedAs(None)),
            beside: Vec::new(),
        };
        // Its removal waits, and does not come.
        let ctx = StepContext::detached();
        let (item, info) = trashed_copy(&trash, tmp.path(), "a’s.txt", "a");
        ctx.after_write_out(|| Ok(()));
        record_goes(&ctx, &waits(), &item, &info).unwrap();
        assert!(info.exists(), "not before the write-out");
        ctx.write_out().unwrap();
        assert!(item.exists() && info.exists(), "listed, as it stayed");
        // It comes.
        let ctx = StepContext::detached();
        let (item, info) = trashed_copy(&trash, tmp.path(), "b’s.txt", "b");
        let removed = item.clone();
        ctx.after_write_out(move || Ok(std::fs::remove_file(&removed)?));
        record_goes(&ctx, &waits(), &item, &info).unwrap();
        assert!(info.exists(), "not before the write-out");
        ctx.write_out().unwrap();
        assert!(!item.exists() && !info.exists(), "gone with it");
        // Moved out at once.
        let (item, info) = trashed_copy(&trash, tmp.path(), "c’s.txt", "c");
        std::fs::remove_file(&item).unwrap();
        let moved = Moved {
            waits: None,
            beside: Vec::new(),
        };
        record_goes(&StepContext::detached(), &moved, &item, &info).unwrap();
        assert!(!info.exists());
    }

    /// A move into the trash on another volume that failed leaves nothing
    /// the trash does not list: while the original is whole its copy goes
    /// with its record; once part of the original went, both stay, listed.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_failed_trash_across_volumes_leaves_nothing_unlisted() {
        let base = dirs::home_dir().unwrap().join(".cache/skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        let tmp = tempfile::tempdir_in(base).unwrap();
        let trash = tmp.path().join(".Trash-1000");
        let (item, info) = (
            trash.join("files/it’s a  folder"),
            trash.join("info/it’s a  folder.trashinfo"),
        );
        let made = || {
            std::fs::create_dir_all(&item).unwrap();
            std::fs::write(item.join("notes.txt"), "notes").unwrap();
            std::fs::create_dir_all(trash.join("info")).unwrap();
            std::fs::write(&info, "[Trash Info]\nPath=/example/it’s a  folder\n").unwrap();
        };
        made();
        let whole = changed_since(Path::new("/example/it’s a  folder"), &trash);
        let _ = failed_across(whole, &item, &info);
        assert!(!item.exists() && !info.exists(), "nothing unlisted left");
        made();
        let in_part =
            anyhow::Error::from(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
                .context(OriginalStays {
                    whole: false,
                    said: Words::as_written("its original could not all be removed"),
                });
        let said = super::super::said(&failed_across(in_part, &item, &info));
        assert!(item.join("notes.txt").is_file() && info.is_file(), "{said}");
        assert!(said.contains("all of it is in the trash"), "{said}");
    }

    /// A move across volumes whose original cannot all be removed says
    /// that part of it went (`OriginalStays`), which a failed trash reads.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_move_across_volumes_says_whether_its_original_stayed_whole() {
        if rustix::process::geteuid().is_root() {
            return;
        }
        let base = dirs::home_dir().unwrap().join(".cache/skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        let tmp = tempfile::tempdir_in(base).unwrap();
        let shm = Path::new("/dev/shm");
        if crate::fs::stat::id_of(shm).map(|id| id.dev)
            == crate::fs::stat::id_of(tmp.path()).map(|id| id.dev)
            || !shm.is_dir()
        {
            eprintln!("skipped: no second filesystem available");
            return;
        }
        let there = tempfile::tempdir_in(shm).unwrap();
        // Its one file removed, the folder holding it is not: `kept`
        // keeps its names.
        let src = tmp.path().join("project  ’25");
        std::fs::create_dir_all(src.join("kept/sub")).unwrap();
        std::fs::write(src.join("kept/sub/data.txt"), "data").unwrap();
        std::fs::set_permissions(src.join("kept"), std::fs::Permissions::from_mode(0o555)).unwrap();
        let ctx = StepContext::detached();
        let error = move_entry(&ctx, &src, &there.path().join("project  ’25"), &[]).err();
        std::fs::set_permissions(src.join("kept"), std::fs::Permissions::from_mode(0o755)).unwrap();
        let error = error.expect("its original could not all be removed");
        assert!(OriginalStays::in_part(&error), "{error:#}");
        assert!(
            !OriginalStays::in_part(&changed_since(&src, there.path())),
            "a changed one stays whole"
        );
    }

    /// A copy onto a camera that fails takes back what it made, never the
    /// sidecars that were there before it: one where its own was to go
    /// (why it failed), an orphan beside its name.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_failed_copy_onto_a_camera_leaves_what_was_there() {
        let base = dirs::home_dir().unwrap().join(".cache/skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        let tmp = tempfile::tempdir_in(base).unwrap();
        let src = tmp.path().join("here");
        std::fs::create_dir_all(src.join(".notes")).unwrap();
        std::fs::write(src.join("it’s a.jpg"), "picture").unwrap();
        std::fs::write(src.join(".notes/it’s a.jpg.note"), "mine").unwrap();
        let camera = tmp.path().join("gphoto2:synthetic");
        std::fs::create_dir_all(camera.join(".notes")).unwrap();
        std::fs::write(camera.join("._it’s a.jpg"), "orphan").unwrap();
        std::fs::write(camera.join(".notes/it’s a.jpg.note"), "theirs").unwrap();
        crate::fs::reach::pretend_camera(&camera);
        let companions = crate::companions::Companions::default();
        companions.register("test", std::sync::Arc::new(crate::companions::tests::Rules));
        let ctx = StepContext::detached().with_companions(std::sync::Arc::new(companions));
        let copy = Copy {
            entry: EntryRef::from_path(src.join("it’s a.jpg")).unwrap(),
            dest_dir: camera.clone(),
            new_name: None,
            preserve_attributes: false,
        };
        let error = super::super::Effect::apply(&copy, &ctx).unwrap_err();
        assert!(format!("{error:#}").contains("already there"), "{error:#}");
        assert!(
            !camera.join("it’s a.jpg").exists(),
            "what it made taken back"
        );
        assert_eq!(
            std::fs::read(camera.join("._it’s a.jpg")).unwrap(),
            b"orphan"
        );
        assert_eq!(
            std::fs::read(camera.join(".notes/it’s a.jpg.note")).unwrap(),
            b"theirs"
        );
    }

    /// Each copy step placing its staged copy looks at its own copies of
    /// files with several names, never at every earlier step's: files
    /// whose other names the plan never copies stay known all plan long.
    #[cfg(unix)]
    #[test]
    fn a_copy_placed_looks_at_its_own_copies_alone() {
        use std::os::unix::fs::MetadataExt;
        use std::sync::atomic::Ordering;
        const FILES: usize = 200;
        let base = dirs::home_dir().unwrap().join(".cache/skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        let tmp = tempfile::tempdir_in(base).unwrap();
        let (src, outside, dest) = (
            tmp.path().join("src"),
            tmp.path().join("outside"),
            tmp.path().join("dest"),
        );
        for dir in [&src, &outside, &dest] {
            std::fs::create_dir(dir).unwrap();
        }
        let ctx = StepContext::detached();
        let before = super::super::context::PLACED_LOOKS.load(Ordering::Relaxed);
        for n in 0..FILES {
            let name = format!("it's {n}.jpg");
            std::fs::write(src.join(&name), n.to_string()).unwrap();
            std::fs::hard_link(src.join(&name), outside.join(&name)).unwrap();
            let copy = Copy {
                entry: EntryRef::from_path(src.join(&name)).unwrap(),
                dest_dir: dest.clone(),
                new_name: None,
                preserve_attributes: false,
            };
            super::super::Effect::apply(&copy, &ctx).unwrap();
        }
        let looked = super::super::context::PLACED_LOOKS.load(Ordering::Relaxed) - before;
        assert_eq!(looked, FILES, "one look a step, at its own copy");
        let ino = |path: PathBuf| std::fs::metadata(path).unwrap().ino();
        assert_ne!(ino(dest.join("it's 7.jpg")), ino(src.join("it's 7.jpg")));
        assert_eq!(std::fs::read(dest.join("it's 7.jpg")).unwrap(), b"7");
    }

    /// A rename onto a name the file already has (another spelling of it,
    /// or a link to it) reads the folder's listing once for the plan:
    /// the plan's own renames there are followed in what it keeps, and a
    /// change from elsewhere has it read again.
    #[cfg(unix)]
    #[test]
    fn renames_onto_a_name_of_their_file_read_the_folder_once() {
        use std::sync::atomic::Ordering;

        use super::super::listed::READ;
        let base = dirs::home_dir().unwrap().join(".cache/skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        let tmp = tempfile::tempdir_in(base).unwrap();
        let dir = tmp.path();
        for name in ["it's a.txt", "b.txt"] {
            std::fs::write(dir.join(name), name).unwrap();
        }
        std::fs::hard_link(dir.join("it's a.txt"), dir.join("it's a link.txt")).unwrap();
        let rename = |name: &str, to: &str| Rename {
            entry: EntryRef::from_path(dir.join(name)).unwrap(),
            new_name: to.into(),
        };
        let onto_link = rename("it's a.txt", "it's a link.txt");
        let taken = |status: StepStatus| matches!(status, StepStatus::Conflict { .. });
        let ctx = StepContext::detached();
        let before = READ.load(Ordering::Relaxed);
        for _ in 0..10 {
            assert!(taken(onto_link.check(&ctx)), "a link is another name");
        }
        assert_eq!(READ.load(Ordering::Relaxed) - before, 1, "one listing");
        super::super::Effect::apply(&rename("b.txt", "b2.txt"), &ctx).unwrap();
        assert!(taken(onto_link.check(&ctx)));
        assert_eq!(
            READ.load(Ordering::Relaxed) - before,
            1,
            "the plan's own rename followed"
        );
        // From elsewhere: a link made, the folder's time moved on as a
        // later tick of a coarse clock gives it.
        std::fs::hard_link(dir.join("b2.txt"), dir.join("b2 link.txt")).unwrap();
        std::fs::File::open(dir)
            .unwrap()
            .set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(1))
            .unwrap();
        assert!(taken(rename("b2.txt", "b2 link.txt").check(&ctx)));
        assert_eq!(READ.load(Ordering::Relaxed) - before, 2, "read again");
    }

    /// A copy of a file is finished when it has its length and time:
    /// exactly, or to the second where the copy keeps no more (SFTP's).
    #[test]
    fn a_copy_is_finished_by_its_length_and_time_as_its_volume_keeps_it() {
        let tmp = tempfile::tempdir().unwrap();
        let at = |name: &str, nanos: u32| {
            let path = tmp.path().join(name);
            std::fs::write(&path, b"same length").unwrap();
            let when = std::time::UNIX_EPOCH + std::time::Duration::new(1_700_000_000, nanos);
            std::fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(when)
                .unwrap();
            std::fs::metadata(&path).unwrap()
        };
        let source = at("source", 250_000_000);
        assert!(finished_as(&at("exact", 250_000_000), &source));
        assert!(finished_as(&at("seconds kept", 0), &source));
        assert!(
            finished_as(&source, &at("source of seconds", 0)),
            "a source read through a server keeping seconds"
        );
        assert!(
            !finished_as(&at("another", 750_000_000), &source),
            "another file of that second"
        );
    }

    /// What is, or holds, where something is mounted is never removed or
    /// trashed whole: a removal would reach into a drive's files or a
    /// server's (ADR-077). Read from this machine's own mounts (`/proc`).
    #[cfg(target_os = "linux")]
    #[test]
    fn a_mount_point_is_never_removed_whole() {
        let proc = mounted_there(Path::new("/proc"))
            .expect("/proc is mounted")
            .english();
        assert!(
            proc.contains("is where a drive or a remote is mounted"),
            "{proc}"
        );
        // A folder that is no mount point holding one: found in the table.
        let points: Vec<PathBuf> =
            crate::fs::volume::mountinfo(&std::fs::read("/proc/self/mountinfo").unwrap())
                .into_iter()
                .map(|m| m.point)
                .collect();
        let holder = points
            .iter()
            .filter_map(|p| p.parent())
            .find(|q| !points.iter().any(|p| p == q))
            .expect("a mount below a plain folder");
        let above = mounted_there(holder).expect("it holds a mount").english();
        assert!(above.contains("holds"), "{above}");
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(mounted_there(tmp.path()), None, "a plain folder");
        let err = remove_recursive(Path::new("/proc")).unwrap_err();
        assert!(err.to_string().contains("eject"), "{err}");
    }

    /// Where a trash keeps what it says of a file, from the file's path:
    /// the home trash's and a volume's two kinds, nothing elsewhere, not
    /// a folder merely named `Trash`.
    #[test]
    fn a_file_of_a_trash_is_known_by_its_path() {
        let info = |p: &str| trash_info_of(Path::new(p));
        let home = super::super::records::home_trash().unwrap();
        assert_eq!(
            trash_info_of(&home.join("files/a b.txt")),
            Some(home.join("info/a b.txt.trashinfo"))
        );
        assert_eq!(info("/home/u/projects/Trash/files/a b.txt"), None);
        assert_eq!(
            info("/media/stick/.Trash-1000/files/a"),
            Some(PathBuf::from("/media/stick/.Trash-1000/info/a.trashinfo"))
        );
        assert_eq!(
            info("/media/stick/.Trash/1000/files/a"),
            Some(PathBuf::from("/media/stick/.Trash/1000/info/a.trashinfo"))
        );
        // Inside a folder of the trash, or a folder merely named so.
        assert_eq!(trash_info_of(&home.join("files/dir/a")), None);
        assert_eq!(info("/home/u/projects/files/a"), None);
        assert_eq!(info("/media/stick/.Trash-me/files/a"), None);
        assert_eq!(info("/media/stick/1000/files/a"), None);
    }

    /// A file deleted from a trash takes its `.trashinfo` along, and one
    /// in a trash is neither put there again nor renamed there.
    #[test]
    fn deleting_from_the_trash_forgets_what_it_said() {
        let tmp = tempfile::tempdir().unwrap();
        let trash = tmp.path().join(".Trash-1000");
        std::fs::create_dir_all(trash.join("files")).unwrap();
        std::fs::create_dir_all(trash.join("info")).unwrap();
        let file = trash.join("files/a.txt");
        std::fs::write(&file, "a").unwrap();
        std::fs::write(
            trash.join("info/a.txt.trashinfo"),
            "[Trash Info]\nPath=a.txt\n",
        )
        .unwrap();
        let entry = EntryRef::from_path(&file).unwrap();
        let ctx = StepContext::detached();
        assert!(matches!(
            Trash {
                entry: entry.clone()
            }
            .check(&ctx),
            StepStatus::Conflict { .. }
        ));
        let renamed = Rename {
            entry: entry.clone(),
            new_name: "b.txt".into(),
        };
        assert!(matches!(renamed.check(&ctx), StepStatus::Conflict { .. }));
        // Moved out, it leaves the trash (as a restore does elsewhere).
        let moved = Move {
            entry: entry.clone(),
            dest_dir: tmp.path().to_path_buf(),
            new_name: None,
        };
        assert_eq!(moved.check(&ctx), StepStatus::Pending { reversible: true });
        let delete = Delete { entry };
        assert!(!delete.undoable());
        assert!(delete.apply(&ctx).unwrap().is_none());
        assert!(!file.exists());
        assert!(!trash.join("info/a.txt.trashinfo").exists());
    }

    /// A file inside a trashed folder is in the trash: it is not put there
    /// again, and it knows the item it is in.
    #[test]
    fn a_file_inside_a_trashed_folder_is_in_the_trash() {
        let at = in_trash(Path::new("/m/.Trash-1000/files/old project/sub/a.txt")).unwrap();
        assert_eq!(at.item, Path::new("/m/.Trash-1000/files/old project"));
        assert_eq!(
            at.info,
            Path::new("/m/.Trash-1000/info/old project.trashinfo")
        );
        assert_eq!(at.inner, Path::new("sub/a.txt"));
        let item = in_trash(Path::new("/m/.Trash-1000/files/a")).unwrap();
        assert!(item.inner.as_os_str().is_empty());
        assert_eq!(in_trash(Path::new("/m/.Trash-1000/files")), None);
        assert_eq!(in_trash(Path::new("/m/projects/files/a/b")), None);
        let ctx = StepContext::detached();
        let inside = Trash {
            entry: EntryRef::new(
                crate::FileId::new(1, 2),
                "/m/.Trash-1000/files/old project/sub/a.txt",
            ),
        };
        assert!(matches!(inside.check(&ctx), StepStatus::Conflict { .. }));
    }

    /// A file restored alone from a trashed folder makes its folders again,
    /// as they are in the trash, and goes back undone; the folder restored
    /// later merges into what was made, and undone takes back only what it
    /// brought.
    #[test]
    #[cfg(unix)]
    fn a_trashed_folder_is_restored_in_parts_and_merges_into_what_came_back() {
        let tmp = tempfile::tempdir().unwrap();
        let trash = tmp.path().join(".Trash-1000");
        let item = trash.join("files/old project");
        std::fs::create_dir_all(item.join("sub")).unwrap();
        std::fs::create_dir_all(trash.join("info")).unwrap();
        std::fs::write(item.join("sub/a.txt"), "a").unwrap();
        std::fs::write(item.join("b.txt"), "b").unwrap();
        std::fs::set_permissions(item.join("sub"), std::fs::Permissions::from_mode(0o750)).unwrap();
        let info = trash.join("info/old project.trashinfo");
        std::fs::write(&info, "[Trash Info]\nPath=docs/old%20project\n").unwrap();
        let docs = tmp.path().join("docs");
        std::fs::create_dir(&docs).unwrap();
        let folder = docs.join("old project");
        let ctx = StepContext::detached();

        let part = RestorePart {
            entry: EntryRef::from_path(item.join("sub/a.txt")).unwrap(),
            path: folder.join("sub/a.txt"),
        };
        assert_eq!(part.check(&ctx), StepStatus::Pending { reversible: true });
        let back = part.apply(&ctx).unwrap().unwrap();
        assert_eq!(
            std::fs::read_to_string(folder.join("sub/a.txt")).unwrap(),
            "a"
        );
        assert_eq!(
            std::fs::metadata(folder.join("sub"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o750,
            "made as it is in the trash"
        );
        assert!(
            item.join("b.txt").exists() && info.exists(),
            "the rest stays"
        );
        assert_eq!(part.check(&ctx), StepStatus::AlreadyDone);
        // Undone: back in the trash, and the folders made go.
        let again = back.apply(&ctx).unwrap().unwrap();
        assert!(item.join("sub/a.txt").exists());
        assert!(!folder.exists());
        again.apply(&ctx).unwrap();
        assert!(folder.join("sub/a.txt").exists());

        // The folder now merges into the one made again.
        let whole = Restore {
            path: folder.clone(),
            item: Some(PathArg::from(info.as_path())),
            landed: Vec::new(),
        };
        assert_eq!(whole.check(&ctx), StepStatus::Pending { reversible: true });
        let unmerge = whole.apply(&ctx).unwrap().unwrap();
        assert_eq!(std::fs::read_to_string(folder.join("b.txt")).unwrap(), "b");
        assert!(folder.join("sub/a.txt").exists());
        assert!(
            !item.exists() && !info.exists(),
            "restored, it leaves the trash"
        );
        // Undone, only what it brought goes back.
        let redo = unmerge.apply(&ctx).unwrap().unwrap();
        assert!(item.join("b.txt").exists() && info.exists());
        assert!(!folder.join("b.txt").exists());
        assert!(
            folder.join("sub/a.txt").exists(),
            "restored before, it stays"
        );
        // A file there again in its place: not merged over.
        std::fs::write(folder.join("b.txt"), "mine").unwrap();
        assert!(matches!(redo.check(&ctx), StepStatus::Conflict { .. }));
        assert!(matches!(whole.check(&ctx), StepStatus::Conflict { .. }));
    }

    /// An item of the trash moved elsewhere leaves the trash with what it
    /// said of it, and comes back with it undone; one inside a trashed
    /// folder leaves the folder there.
    // The freedesktop trash's records.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_file_moved_out_of_the_trash_takes_what_the_trash_said_of_it() {
        let tmp = tempfile::tempdir().unwrap();
        let trash = tmp.path().join(".Trash-1000");
        std::fs::create_dir_all(trash.join("files/old project")).unwrap();
        std::fs::create_dir_all(trash.join("info")).unwrap();
        std::fs::write(trash.join("files/it's a.txt"), "a").unwrap();
        std::fs::write(trash.join("files/old project/b.txt"), "b").unwrap();
        let said = "[Trash Info]\nPath=docs/it%27s%20a.txt\nDeletionDate=2026-10-04T09:30:00\n";
        let info = trash.join("info/it's a.txt.trashinfo");
        std::fs::write(&info, said).unwrap();
        let folder_info = trash.join("info/old project.trashinfo");
        std::fs::write(&folder_info, "[Trash Info]\nPath=docs/old%20project\n").unwrap();
        let desk = tmp.path().join("desk");
        std::fs::create_dir(&desk).unwrap();
        let ctx = StepContext::detached();

        let out = Move {
            entry: EntryRef::from_path(trash.join("files/it's a.txt")).unwrap(),
            dest_dir: desk.clone(),
            new_name: None,
        };
        assert_eq!(out.check(&ctx), StepStatus::Pending { reversible: true });
        let back = out.apply(&ctx).unwrap().unwrap();
        assert_eq!(
            std::fs::read_to_string(desk.join("it's a.txt")).unwrap(),
            "a"
        );
        assert!(!info.exists(), "out of the trash, nothing says it is there");
        assert_eq!(out.check(&ctx), StepStatus::AlreadyDone);
        let again = back.apply(&ctx).unwrap().unwrap();
        assert!(!desk.join("it's a.txt").exists());
        assert_eq!(std::fs::read_to_string(&info).unwrap(), said, "as it was");
        again.apply(&ctx).unwrap();
        assert!(desk.join("it's a.txt").exists() && !info.exists());

        let inner = Move {
            entry: EntryRef::from_path(trash.join("files/old project/b.txt")).unwrap(),
            dest_dir: desk.clone(),
            new_name: None,
        };
        let back = inner.apply(&ctx).unwrap().unwrap();
        assert!(desk.join("b.txt").exists());
        assert!(
            trash.join("files/old project").is_dir() && folder_info.exists(),
            "the folder it was in stays in the trash"
        );
        back.apply(&ctx).unwrap();
        assert!(trash.join("files/old project/b.txt").exists());

        // One the trash named otherwise, holding a file of its name
        // already, goes back to its own name.
        std::fs::write(trash.join("files/it's a.txt.2"), "two").unwrap();
        let info_two = trash.join("info/it's a.txt.2.trashinfo");
        std::fs::write(&info_two, said).unwrap();
        let other = tmp.path().join("other");
        std::fs::create_dir(&other).unwrap();
        let named = Move {
            entry: EntryRef::from_path(trash.join("files/it's a.txt.2")).unwrap(),
            dest_dir: other.clone(),
            new_name: None,
        };
        assert_eq!(
            named.describe(),
            Change::Move {
                from: trash.join("files/it's a.txt.2"),
                to: other.join("it's a.txt"),
            }
        );
        let back = named.apply(&ctx).unwrap().unwrap();
        assert_eq!(
            std::fs::read_to_string(other.join("it's a.txt")).unwrap(),
            "two"
        );
        back.apply(&ctx).unwrap();
        assert!(trash.join("files/it's a.txt.2").exists() && info_two.exists());

        // Within the trash, an item is not moved: the trash would hold a
        // file it says nothing of.
        std::fs::write(trash.join("files/c.txt"), "c").unwrap();
        std::fs::write(trash.join("info/c.txt.trashinfo"), said).unwrap();
        let within = Move {
            entry: EntryRef::from_path(trash.join("files/c.txt")).unwrap(),
            dest_dir: trash.join("files/old project"),
            new_name: None,
        };
        assert!(matches!(within.check(&ctx), StepStatus::Conflict { .. }));
    }

    /// The folders a file restored alone made are made as their
    /// counterparts in the trash are, read-only maybe: undone, they go all
    /// the same, and one something was put in since keeps its mode.
    #[test]
    #[cfg(unix)]
    fn read_only_folders_made_for_a_restore_go_when_it_is_undone() {
        let tmp = tempfile::tempdir().unwrap();
        let trash = tmp.path().join(".Trash-1000");
        let item = trash.join("files/old");
        std::fs::create_dir_all(item.join("sub")).unwrap();
        std::fs::create_dir_all(trash.join("info")).unwrap();
        std::fs::write(item.join("sub/a.txt"), "a").unwrap();
        std::fs::write(
            trash.join("info/old.trashinfo"),
            "[Trash Info]\nPath=docs/old\n",
        )
        .unwrap();
        std::fs::set_permissions(&item, std::fs::Permissions::from_mode(0o555)).unwrap();
        let docs = tmp.path().join("docs");
        std::fs::create_dir(&docs).unwrap();
        let ctx = StepContext::detached();
        let part = RestorePart {
            entry: EntryRef::from_path(item.join("sub/a.txt")).unwrap(),
            path: docs.join("old/sub/a.txt"),
        };
        let back = part.apply(&ctx).unwrap().unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&docs.join("old")), 0o555, "made as it is in the trash");
        let again = back.apply(&ctx).unwrap().unwrap();
        assert!(item.join("sub/a.txt").exists());
        assert!(!docs.join("old").exists(), "the folders made go");

        again.apply(&ctx).unwrap();
        std::fs::set_permissions(docs.join("old"), std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(docs.join("old/mine.txt"), "mine").unwrap();
        std::fs::set_permissions(docs.join("old"), std::fs::Permissions::from_mode(0o555)).unwrap();
        back.apply(&ctx).unwrap();
        assert!(docs.join("old/mine.txt").exists() && !docs.join("old/sub").exists());
        assert_eq!(mode(&docs.join("old")), 0o555, "what stays keeps its mode");
        for dir in [docs.join("old"), item] {
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    /// The ways a copy is counted: at once, by ranges handed to the drive,
    /// or by write-outs of the volume (where a program writes it). A
    /// volume's write-out syncs a whole filesystem: the tests' own would
    /// make every test writing to it wait. procfs has nothing to write;
    /// this checks the bytes and what is counted, not the drive.
    #[cfg(target_os = "linux")]
    fn paces() -> [(&'static str, Pace); 3] {
        let proc = || crate::fs::Dir::open(Path::new("/proc")).unwrap();
        [
            ("at once", Pace::at_once()),
            ("ranges", Pace::testing(proc(), false)),
            ("volume", Pace::testing(proc(), true)),
        ]
    }

    /// A drive as the copy loop meets it (`copy_buffered`): each write
    /// takes at most `each` bytes, the one after each is interrupted
    /// (`EINTR`) where `interrupts`, and once it holds `holds` bytes it
    /// refuses with `refuses`, or takes nothing (`Ok(0)`) without one.
    struct StandIn {
        took: Vec<u8>,
        each: usize,
        holds: usize,
        refuses: Option<std::io::ErrorKind>,
        interrupts: bool,
        interrupted: bool,
    }

    impl StandIn {
        fn new(each: usize, holds: usize, refuses: Option<std::io::ErrorKind>) -> Self {
            StandIn {
                took: Vec::new(),
                each,
                holds,
                refuses,
                interrupts: false,
                interrupted: false,
            }
        }
    }

    impl Write for StandIn {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if self.interrupts && !std::mem::replace(&mut self.interrupted, true) {
                return Err(std::io::ErrorKind::Interrupted.into());
            }
            self.interrupted = false;
            let room = self.holds - self.took.len();
            if room == 0 {
                return self.refuses.map_or(Ok(0), |kind| Err(kind.into()));
            }
            let n = buf.len().min(self.each).min(room);
            self.took.extend_from_slice(&buf[..n]);
            Ok(n)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// Bytes that differ along their length.
    fn varied(len: usize) -> Vec<u8> {
        (0..len)
            .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
            .collect()
    }

    /// A drive that takes a little of each write, and is interrupted
    /// between two, is written on until it holds every byte: told once
    /// each chunk is whole, the copy ends where its source does.
    #[test]
    fn the_copy_loop_writes_on_after_short_and_interrupted_writes() {
        let bytes = varied((3 << 20) + 12_345);
        let mut drive = StandIn::new(4097, usize::MAX, None);
        drive.interrupts = true;
        let mut told = 0u64;
        let copied = copy_buffered(
            &mut bytes.as_slice(),
            &mut drive,
            &mut vec![0; 1 << 20],
            &StepContext::detached(),
            &mut |n| {
                told += n;
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(copied, bytes.len() as u64);
        assert_eq!(told, copied, "every byte told, once");
        assert!(drive.took == bytes, "the drive holds the source");
    }

    /// What the drive refuses partway through a chunk (a FAT volume past
    /// 4 GiB, a full one) fails the copy, in the drive's words, with what
    /// it took never told as copied; one that takes nothing fails too.
    /// Never an end: only the source's reading says one.
    #[test]
    fn the_copy_loop_fails_where_the_drive_refuses() {
        use std::io::ErrorKind;
        let bytes = varied(3 << 20);
        let holds = (2 << 20) + 7;
        for (refuses, said) in [
            (
                Some(ErrorKind::FileTooLarge),
                "the file is too large for that drive",
            ),
            (Some(ErrorKind::StorageFull), "the drive is full"),
            (None, "failed to write whole buffer"),
        ] {
            let mut drive = StandIn::new(1 << 16, holds, refuses);
            let mut told = 0u64;
            let error = copy_buffered(
                &mut bytes.as_slice(),
                &mut drive,
                &mut vec![0; 1 << 20],
                &StepContext::detached(),
                &mut |n| {
                    told += n;
                    Ok(())
                },
            )
            .expect_err("refused");
            let io = error.downcast_ref::<std::io::Error>().expect("the drive's");
            assert_eq!(io.kind(), refuses.unwrap_or(ErrorKind::WriteZero));
            assert_eq!(super::super::io_words(io).english(), said);
            assert_eq!(drive.took.len(), holds, "{said}");
            assert!(
                told < holds as u64,
                "{said}: only whole chunks told, {told}"
            );
        }
        // As the system says them.
        #[cfg(unix)]
        for (errno, kind) in [
            (libc::EFBIG, ErrorKind::FileTooLarge),
            (libc::ENOSPC, ErrorKind::StorageFull),
        ] {
            assert_eq!(std::io::Error::from_raw_os_error(errno).kind(), kind);
        }
    }

    /// A file larger than the volume it goes to keeps (a FAT drive's
    /// 4 GiB) is refused by the check of its copy and of its move, in
    /// words: before anything is copied, where it could never be written.
    /// One that fits, and a folder (the plan's sizing looks inside it), are
    /// not.
    #[test]
    fn a_file_larger_than_its_drive_keeps_is_refused_by_its_check() {
        let base = dirs::home_dir().unwrap().join(".cache/skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        let dir = tempfile::tempdir_in(base).unwrap();
        let (src, dest) = (dir.path().join("it's a film.mkv"), dir.path().join("stick"));
        std::fs::create_dir(&dest).unwrap();
        std::fs::write(&src, [7u8; 100]).unwrap();
        std::fs::create_dir(dir.path().join("films")).unwrap();
        std::fs::write(dir.path().join("films/a.mkv"), [7u8; 100]).unwrap();
        let entry = |path: &Path| EntryRef::from_path(path).unwrap();
        let copy = |path: &Path| Copy {
            entry: entry(path),
            dest_dir: dest.clone(),
            new_name: None,
            preserve_attributes: true,
        };
        let moving = Move {
            entry: entry(&src),
            dest_dir: dest.clone(),
            new_name: None,
        };
        let refused = |status: StepStatus| match status {
            StepStatus::Conflict { reason } => Some(reason.english()),
            _ => None,
        };
        let ctx = StepContext::detached();
        ctx.assume_largest(&dest, Some(99));
        let said = "“it's a film.mkv” is too large for “stick”: a drive formatted as FAT keeps no file of 100\u{a0}B or more";
        assert_eq!(refused(copy(&src).check(&ctx)).as_deref(), Some(said));
        assert_eq!(refused(moving.check(&ctx)).as_deref(), Some(said));
        assert_eq!(refused(copy(&dir.path().join("films")).check(&ctx)), None);
        ctx.assume_largest(&dest, Some(100));
        assert_eq!(refused(copy(&src).check(&ctx)), None, "just fits");
        ctx.assume_largest(&dest, None);
        assert_eq!(refused(copy(&src).check(&ctx)), None, "keeps any");
    }

    /// The home's volume keeps any file: only a FAT one is said to keep
    /// less, and a remote is never asked.
    #[test]
    fn only_a_fat_volume_keeps_less_than_any_file() {
        let base = dirs::home_dir().unwrap().join(".cache/skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        let dir = tempfile::tempdir_in(base).unwrap();
        assert_eq!(sys::largest_file(dir.path()), None);
        let ctx = StepContext::detached();
        assert_eq!(
            ctx.largest_in(&dir.path().join("not there yet/below")),
            None
        );
    }

    // Writing through is Linux's alone so far (ADR-035).
    #[cfg(target_os = "linux")]
    #[test]
    fn a_copy_written_through_is_whole_and_counted() {
        let base = dirs::home_dir().unwrap().join(".cache/skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        let dir = tempfile::tempdir_in(base).unwrap();
        let (src, dst) = (dir.path().join("firmware.bin"), dir.path().join("copy.bin"));
        // Several windows, and an end that is not a whole one.
        let bytes: Vec<u8> = (0..(3 << 20) + 12_345u32)
            .map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8)
            .collect();
        std::fs::write(&src, &bytes).unwrap();
        for (how, pace) in paces() {
            let ctx = StepContext::detached();
            let _ = std::fs::remove_file(&dst);
            copy_file(&src, &dst, false, &pace, &ctx).unwrap();
            assert_eq!(ctx.done(), bytes.len() as u64, "{how}: everything counted");
            pace.write_out().unwrap();
            assert_eq!(std::fs::read(&dst).unwrap(), bytes, "{how}");
        }
        std::fs::write(&src, b"").unwrap();
        for (how, pace) in paces() {
            let ctx = StepContext::detached();
            std::fs::remove_file(&dst).unwrap();
            copy_file(&src, &dst, false, &pace, &ctx).unwrap();
            pace.write_out().unwrap();
            assert!(std::fs::read(&dst).unwrap().is_empty(), "{how}");
        }
    }

    /// A file's holes count as done, never as speed: they move no byte
    /// (ADR-113).
    #[cfg(target_os = "linux")]
    #[test]
    fn a_file_s_holes_are_done_but_no_speed() {
        use std::os::unix::fs::{FileExt, MetadataExt};
        let base = dirs::home_dir().unwrap().join(".cache/skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        let dir = tempfile::tempdir_in(base).unwrap();
        let (src, dst) = (
            dir.path().join("disk image.raw"),
            dir.path().join("copy.raw"),
        );
        let len = 64u64 << 20;
        let data = vec![7u8; 1 << 20];
        let file = std::fs::File::create(&src).unwrap();
        file.write_all_at(&data, 0).unwrap();
        file.write_all_at(&data, len - data.len() as u64).unwrap();
        drop(file);
        let meta = std::fs::metadata(&src).unwrap();
        if meta.blocks() * 512 >= len {
            eprintln!("skipped: this volume keeps no holes");
            return;
        }
        let ctx = StepContext::detached();
        let (sf, df) = (
            std::fs::File::open(&src).unwrap(),
            std::fs::File::create(&dst).unwrap(),
        );
        copy_sparse(&sf, &df, (0, len), None, &ctx).unwrap();
        assert_eq!(ctx.done(), len, "the holes counted done");
        assert!(
            ctx.measured() <= 2 * data.len() as u64,
            "only the data measured: {}",
            ctx.measured()
        );
        assert_eq!(std::fs::read(&dst).unwrap(), std::fs::read(&src).unwrap());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_folder_of_small_files_written_through_is_whole_and_counted() {
        let base = dirs::home_dir().unwrap().join(".cache/skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        let dir = tempfile::tempdir_in(base).unwrap();
        let album = dir.path().join("album");
        std::fs::create_dir(&album).unwrap();
        // More files than the drive is handed at once.
        let files = 200;
        for i in 0..files {
            std::fs::write(
                album.join(format!("photo {i}.jpg")),
                vec![i as u8; 1000 + i],
            )
            .unwrap();
        }
        let total: usize = (0..files).map(|i| 1000 + i).sum();
        for (how, pace) in paces() {
            let ctx = StepContext::detached();
            let copy = dir.path().join(format!("copy {how}"));
            copy_recursive_with_attributes(&album, &copy, false, &pace, &ctx).unwrap();
            assert_eq!(ctx.done(), total as u64, "{how}: everything counted");
            pace.write_out().unwrap();
            for i in 0..files {
                let file = std::fs::read(copy.join(format!("photo {i}.jpg"))).unwrap();
                assert_eq!(file, vec![i as u8; 1000 + i], "{how}");
            }
        }
    }

    #[test]
    fn a_trees_state_changes_with_anything_of_it_and_only_then() {
        let base = dirs::home_dir().unwrap().join(".cache/skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        let dir = tempfile::tempdir_in(base).unwrap();
        let album = dir.path().join("album");
        std::fs::create_dir_all(album.join("2026")).unwrap();
        std::fs::write(album.join("2026/photo.jpg"), b"one").unwrap();
        // Dated by a camera whose clock runs a year ahead.
        let ahead = std::time::SystemTime::now() + std::time::Duration::from_hours(365 * 24);
        let card = std::fs::File::create(album.join("card.jpg")).unwrap();
        card.set_modified(ahead).unwrap();
        drop(card);
        let state = state_of(&album).expect("readable");
        assert_eq!(state_of(&album), Some(state), "nothing changed");

        // Past the clock tick change times are read from.
        std::thread::sleep(std::time::Duration::from_millis(50));
        std::fs::write(album.join("2026/photo.jpg"), b"two").unwrap();
        let edited = state_of(&album).unwrap();
        assert_ne!(edited, state, "deep in it");
        std::fs::write(album.join("2026/new.jpg"), b"").unwrap();
        assert_ne!(state_of(&album), Some(edited), "a file more");
        assert_eq!(state_of(&dir.path().join("gone")), None);
    }

    #[test]
    fn a_plans_copies_onto_one_volume_share_its_pace() {
        let base = dirs::home_dir().unwrap().join(".cache/skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        let dir = tempfile::tempdir_in(base).unwrap();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        std::fs::create_dir(&a).unwrap();
        std::fs::create_dir(&b).unwrap();
        let ctx = StepContext::detached();
        let first = ctx.pace_through(&a).unwrap();
        assert!(std::rc::Rc::ptr_eq(&first, &ctx.pace_through(&b).unwrap()));
        assert!(!std::rc::Rc::ptr_eq(
            &first,
            &StepContext::detached().pace_through(&a).unwrap()
        ));
    }
}
