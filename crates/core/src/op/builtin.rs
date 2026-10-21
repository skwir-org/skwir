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
    Change, EntryRef, OpRegistry, Operation, ResolveError, StepContext, StepStatus, sys,
    valid_file_name,
};
use crate::fs::stat::{id_of, named};
use crate::os_path::PathArg;

fn pending(reversible: bool) -> StepStatus {
    StepStatus::Pending { reversible }
}

fn conflict(reason: impl Into<String>) -> StepStatus {
    StepStatus::Conflict {
        reason: reason.into(),
    }
}

fn exists(p: &Path) -> bool {
    std::fs::symlink_metadata(p).is_ok()
}

fn file_name(p: &Path) -> OsString {
    p.file_name().unwrap_or_default().to_os_string()
}

fn invalid_name(name: &OsStr) -> Option<StepStatus> {
    valid_file_name(name).err().map(|e| conflict(e.to_string()))
}

/// Whether `dir` lists an entry spelled exactly `name`. On a
/// case-insensitive filesystem `a.txt` and `A.txt` stat the same file, and
/// only the listing says which spelling is on disk.
fn name_listed(dir: &Path, name: &OsStr) -> bool {
    std::fs::read_dir(dir).is_ok_and(|rd| rd.flatten().any(|e| e.file_name() == name))
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
    sys::rename_noreplace(from, to).with_context(|| {
        let name = display_name(from);
        let folder = to.parent().map(display_name).unwrap_or_default();
        if from.parent() == to.parent() {
            format!("“{name}” could not be renamed “{}”", display_name(to))
        } else if from.file_name() == to.file_name() {
            format!("“{name}” could not be moved to “{folder}”")
        } else {
            format!(
                "“{name}” could not be moved to “{folder}” as “{}”",
                display_name(to)
            )
        }
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

/// The companions of the file at `src` that go with it to `dest`.
fn along(ctx: &StepContext, src: &Path, dest: &Path) -> Vec<Along> {
    let rules = ctx.companions();
    rules
        .existing(src)
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
        .map(|a| conflict(format!("“{}” is already there", display_name(&a.to))))
}

/// Rename each companion to its place (making a companions' folder, `.ts`,
/// where needed); on a failure, put back those moved and fail.
fn rename_along(along: &[Along]) -> anyhow::Result<()> {
    for (done, a) in along.iter().enumerate() {
        let result =
            a.to.parent()
                .map_or(Ok(()), std::fs::create_dir_all)
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

    fn classify(&self, src: &Path, target: &Path) -> RenameTarget {
        if !exists(target) {
            return RenameTarget::Free;
        }
        if id_of(target) != Some(self.entry.id) {
            return RenameTarget::Taken;
        }
        // The target stats as the entry: either the same file under another
        // spelling (case-insensitive) or a second hard link. The listing
        // tells them apart: a hard link lists both names.
        let dir = target.parent().unwrap_or(Path::new("/"));
        match (
            name_listed(dir, &file_name(src)),
            name_listed(dir, &self.new_name),
        ) {
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
        match self.entry.resolve() {
            Ok(src) if src == target => StepStatus::AlreadyDone,
            // An earlier step of the plan puts something there (two files
            // given one name).
            Ok(_) if ctx.made(&target) => {
                conflict(format!("“{}” is already there", display_name(&target)))
            }
            Ok(src) => match self.classify(&src, &target) {
                RenameTarget::Free | RenameTarget::Respelled => {
                    along_conflict(ctx, &along(ctx, &src, &target)).unwrap_or(pending(true))
                }
                RenameTarget::Spelled => StepStatus::AlreadyDone,
                RenameTarget::Taken => {
                    conflict(format!("“{}” is already there", display_name(&target)))
                }
            },
            Err(_) if id_of(&target) == Some(self.entry.id) => StepStatus::AlreadyDone,
            Err(e) => conflict(e.to_string()),
        }
    }
    fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        valid_file_name(&self.new_name)?;
        let src = self.entry.resolve()?;
        let target = self.target();
        let along = along(ctx, &src, &target);
        match self.classify(&src, &target) {
            // rename(2) between two spellings of one file is a no-op on a
            // case-insensitive filesystem: go through a parked name.
            RenameTarget::Respelled => {
                let parked = parking_name(target.parent().unwrap_or(Path::new("/")), "rename");
                let respelled = || {
                    format!(
                        "“{}” could not be renamed “{}”",
                        display_name(&src),
                        display_name(&target)
                    )
                };
                sys::rename_noreplace(&src, &parked).with_context(respelled)?;
                if let Err(e) = sys::rename_noreplace(&parked, &target) {
                    sys::rename_noreplace(&parked, &src).with_context(|| {
                        format!(
                            "“{}” was left named “{}”",
                            display_name(&src),
                            display_name(&parked)
                        )
                    })?;
                    return Err(anyhow::Error::new(e).context(respelled()));
                }
            }
            RenameTarget::Spelled => {}
            RenameTarget::Free | RenameTarget::Taken => rename_noreplace(&src, &target)?,
        }
        if let Err(e) = rename_along(&along) {
            let _ = rename_noreplace(&target, &src);
            return Err(e);
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

    /// Whether `src` is where it would go.
    pub(super) fn in_place(&self, src: &Path) -> bool {
        src.parent() == Some(self.dest_dir.as_path())
            && self
                .new_name
                .as_ref()
                .is_none_or(|n| src.file_name() == Some(n))
    }
}

impl super::Kind for Move {
    const KIND: &'static str = "move";
}

impl super::Effect for Move {
    fn describe(&self) -> Change {
        Change::Move {
            from: self.entry.path.clone(),
            to: self.target(&self.entry.path),
        }
    }
    /// Its bytes, when it goes to another filesystem: there a move is a
    /// copy (`move_entry`); within one it renames, writing nothing.
    fn writes(&self, ctx: &StepContext) -> Option<(PathBuf, u64)> {
        let src = self.entry.resolve().ok()?;
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
            return conflict(format!(
                "“{}” is not a folder",
                display_name(&self.dest_dir)
            ));
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
        match self.entry.resolve() {
            Ok(src) if self.in_place(&src) => StepStatus::AlreadyDone,
            // A batch placed its copy (ADR-078): its apply removes the
            // original.
            Ok(src) if ctx.placed_by_batch(&src, &self.target(&src)) => pending(true),
            Ok(src) if ctx.exists(&self.target(&src)) => conflict(format!(
                "“{}” is already there",
                display_name(&self.target(&src))
            )),
            Ok(src) if src.is_dir() && self.dest_dir.starts_with(&src) => {
                conflict("a folder cannot be moved into itself")
            }
            Ok(src) => {
                along_conflict(ctx, &along(ctx, &src, &self.target(&src))).unwrap_or(pending(true))
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
            Err(e) => conflict(e.to_string()),
        }
    }
    fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        if let Some(name) = &self.new_name {
            valid_file_name(name)?;
        }
        let src = self.entry.resolve()?;
        if in_trash(&src).is_some() && in_trash(&self.dest_dir).is_none() {
            return super::Effect::apply(&self.restoring(&src), ctx);
        }
        let target = self.target(&src);
        let origin = src.parent().unwrap_or(Path::new("/")).to_path_buf();
        let along = along(ctx, &src, &target);
        move_entry(ctx, &src, &target, &along)?;
        // Moved to another filesystem, it is a new file, of one name.
        let (id, only) = named(&target).unwrap_or((self.entry.id, self.entry.only_name));
        Ok(Some(Box::new(Move {
            entry: EntryRef::new(id, target.clone()).with_only_name(only),
            dest_dir: origin,
            // Back under the name it had.
            new_name: (target.file_name() != src.file_name()).then(|| file_name(&src)),
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

/// Move `src` to `target` with its companions (`along`): renamed, or
/// across volumes copied and then removed. A move must never lose
/// metadata, whatever the copy preference says, and a failed transfer must
/// not leave a partial target. The companion holding the file's attributes
/// is converted with them (ADR-028); the others go as files.
fn move_entry(ctx: &StepContext, src: &Path, target: &Path, along: &[Along]) -> anyhow::Result<()> {
    // Copied by a batch, which read what it was first (ADR-078): on
    // another device, never renamed.
    let batched = ctx.take_batched_state(src);
    let renamed = match batched {
        Some(_) => Err(std::io::Error::from(std::io::ErrorKind::CrossesDevices).into()),
        None => rename_noreplace(src, target),
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
            let through = copy_entry(src, target, dest_dir, true, along, ctx)?;
            let dest_dir = dest_dir.to_path_buf();
            let remove = move || {
                // Edited while it was copied, or since: the copy may not
                // hold it, so nothing of it goes. Gone is nothing to remove.
                let changed = originals.iter().find(|(path, before)| {
                    std::fs::symlink_metadata(path).is_ok() && state_of(path) != *before
                });
                if let Some((changed, _)) = changed {
                    anyhow::bail!(
                        "“{}” changed after it was copied to “{}”: its original stays where it was",
                        display_name(changed),
                        dest_dir.display()
                    );
                }
                let ((src, _), companions) = originals.split_first().expect("the original first");
                // Part of the source may be gone: say that the copy is
                // whole, so that nothing looks lost.
                remove_original(src).with_context(|| {
                    format!(
                        "“{}” was copied whole to “{}”, but its original could not all be removed",
                        display_name(src),
                        dest_dir.display()
                    )
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
            } else {
                remove()?;
            }
        }
        Err(e) => return Err(e),
    }
    Ok(())
}

/// What the tree at `path` is, links not followed: each entry's name,
/// kind, size, change and modification times, folded into one digest, so
/// a move can tell whether anything of what it copied changed before it
/// removes it. Compared with itself, never with the clock: a card's files
/// can be dated by a camera's. `None` where something of it cannot be
/// read, or it is gone.
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
    fn fold(path: &Path, digest: &mut blake3::Hasher) -> Option<()> {
        let meta = std::fs::symlink_metadata(path).ok()?;
        let kind = u8::from(meta.is_dir()) | u8::from(meta.is_symlink()) << 1;
        digest.update(&[kind]);
        digest.update(&meta.len().to_le_bytes());
        digest.update(&time(crate::fs::stat::changed(path, &meta)));
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
                fold(&path.join(name), digest)?;
            }
        }
        Some(())
    }
    let mut digest = blake3::Hasher::new();
    fold(path, &mut digest)?;
    Some(digest.finalize())
}

/// Remove what a move copied: gone already is removed (a folder moved
/// whole after a file of it, its removal waiting for a write-out).
fn remove_original(path: &Path) -> anyhow::Result<()> {
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
    fn describe(&self) -> Change {
        Change::Copy {
            from: self.entry.path.clone(),
            to: self.target(&self.entry.path),
        }
    }
    fn writes(&self, ctx: &StepContext) -> Option<(PathBuf, u64)> {
        let src = self.entry.resolve().ok()?;
        Some((self.dest_dir.clone(), tree_size(&src, ctx).ok()?))
    }
    fn check(&self, ctx: &StepContext) -> StepStatus {
        if let Some(invalid) = self.new_name.as_deref().and_then(invalid_name) {
            return invalid;
        }
        if !ctx.is_folder(&self.dest_dir) {
            return conflict(format!(
                "“{}” is not a folder",
                display_name(&self.dest_dir)
            ));
        }
        let src = match self.entry.resolve() {
            Ok(s) => s,
            Err(e) => return conflict(e.to_string()),
        };
        let target = self.target(&src);
        // A batch placed it there, a moment ago, verified (ADR-078).
        if ctx.placed_by_batch(&src, &target) {
            return pending(true);
        }
        if target == src {
            return conflict("it would be copied onto itself");
        }
        if ctx.made(&target) {
            return conflict(format!(
                "“{}” is already there, by an earlier step",
                display_name(&target)
            ));
        }
        if ctx.exists(&target) {
            // Only a finished copy of this very source is "already done": an
            // unrelated folder that happens to share the name is a conflict.
            return match same_tree(&src, &target, ctx) {
                Ok(true) => StepStatus::AlreadyDone,
                Ok(false) => conflict(format!("“{}” is already there", display_name(&target))),
                Err(e) => conflict(format!("{e:#}")),
            };
        }
        if src.is_dir() && target.starts_with(&src) {
            return conflict("a folder cannot be copied into itself");
        }
        along_conflict(ctx, &along(ctx, &src, &target)).unwrap_or(pending(true))
    }
    fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        if let Some(name) = &self.new_name {
            valid_file_name(name)?;
        }
        let src = self.entry.resolve()?;
        let target = self.target(&src);
        let along = along(ctx, &src, &target);
        copy_entry(
            &src,
            &target,
            &self.dest_dir,
            self.preserve_attributes,
            &along,
            ctx,
        )?;
        let r = EntryRef::from_path(&target).ok_or_else(|| {
            anyhow::anyhow!(
                "the copy “{}” went missing as it was made",
                display_name(&target)
            )
        })?;
        // Undo puts the copy in the trash rather than deleting it: it may
        // have been edited since, and redo then restores it as it was.
        Ok(Some(Box::new(Trash { entry: r })))
    }
}

/// Whether `copy` holds what copying `src` would produce: the same kinds,
/// names, sizes and file times all the way down (a copy keeps file mtimes),
/// and the same link targets. Stops at the first difference.
pub(super) fn same_tree(src: &Path, copy: &Path, ctx: &StepContext) -> anyhow::Result<bool> {
    ctx.check_cancelled()?;
    let (a, b) = (
        std::fs::symlink_metadata(src)?,
        std::fs::symlink_metadata(copy)?,
    );
    if a.file_type() != b.file_type() {
        return Ok(false);
    }
    if a.is_symlink() {
        return Ok(std::fs::read_link(src)? == std::fs::read_link(copy)?);
    }
    if !a.is_dir() {
        return Ok(a.len() == b.len() && a.modified().ok() == b.modified().ok());
    }
    // A companion holding its sibling's attributes may have become them.
    let names = |dir: &Path| -> std::io::Result<Vec<OsString>> {
        let mut names = std::fs::read_dir(dir)?
            .map(|e| e.map(|e| e.file_name()))
            .filter(|n| !n.as_ref().is_ok_and(|n| attributes_of_sibling(ctx, dir, n)))
            .collect::<std::io::Result<Vec<_>>>()?;
        names.sort_unstable();
        Ok(names)
    };
    let children = names(src)?;
    if children != names(copy)? {
        return Ok(false);
    }
    for name in children {
        if !same_tree(&src.join(&name), &copy.join(&name), ctx)? {
            return Ok(false);
        }
    }
    Ok(true)
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
) -> anyhow::Result<bool> {
    let through = crate::fs::volume::is_removable(dest_dir);
    let pace = if through {
        ctx.pace_through(dest_dir)?
    } else {
        std::rc::Rc::new(Pace::at_once())
    };
    copy_staged(
        src,
        target,
        dest_dir,
        preserve_attributes,
        along,
        &pace,
        ctx,
    )?;
    Ok(through)
}

/// Copy `src` to `target` through a private staging directory inside
/// `dest_dir`, publishing with a no-replace rename only after content and
/// attributes are complete. A failed attribute write must not leave a partial destination
/// that `check()` would call already done. Shared by Copy and cross-device Move.
///
/// Its companions (`along`; the ones not holding attributes, which are
/// converted with the file's) are staged with it and placed after it: one
/// that cannot be copied or placed takes the copy back out, so a copy is
/// never left without what goes with it, and without an Undo.
fn copy_staged(
    src: &Path,
    target: &Path,
    dest_dir: &Path,
    preserve_attributes: bool,
    along_src: &[Along],
    pace: &Pace,
    ctx: &StepContext,
) -> anyhow::Result<()> {
    // A batch made it already, or counted it (ADR-078).
    let batched = ctx.take_batched(src);
    if batched.is_none() {
        ctx.add_total(tree_size(src, ctx)?);
    }
    // The plan's staging folder there, shared by its copies, each under a
    // name of its own.
    let (staging, name) = ctx.staging_for(dest_dir)?;
    let (staged, made) = match batched {
        // At its target already: nothing to place.
        Some(super::transfer::Batched::Placed(at, _)) if at == target => return Ok(()),
        Some(super::transfer::Batched::Made(made)) => (made, true),
        _ => (staging.path().join(&name), false),
    };
    let mut staged_along = Vec::new();
    // What the copy put beside the staged entry (the companion that holds
    // its attributes, on a volume that refuses them) follows it out.
    let copied = if made {
        Ok(())
    } else {
        copy_recursive_with_attributes(src, &staged, preserve_attributes, pace, ctx)
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
        sys::rename_noreplace(&staged, target).with_context(|| {
            format!(
                "“{}” could not be placed in “{}”",
                display_name(target),
                display_name(dest_dir)
            )
        })?;
        ctx.placed(&staged, target);
        // The companions placed, or the copy taken back to be removed
        // from the staging folder.
        rename_along(&beside).inspect_err(|_| {
            let _ = sys::rename_noreplace(target, &staged);
        })
    });
    if let Err(error) = result {
        // What this copy staged goes; the folder stays for the plan's
        // other copies, and goes with the plan.
        let made = along(ctx, &staged, target)
            .into_iter()
            .map(|a| a.from)
            .chain([staged])
            .chain(staged_along);
        for path in made {
            if std::fs::symlink_metadata(&path).is_err() {
                continue;
            }
            if let Err(cleanup) = super::staging::remove_tree(&path) {
                return Err(error.context(format!(
                    "a part of the copy is left in “{}” ({})",
                    path.display(),
                    super::io_words(&cleanup)
                )));
            }
        }
        return Err(error);
    }
    Ok(())
}

/// Bytes of regular files under `path`, so progress has a total: a file
/// of several names once in the plan, as it is copied once.
fn tree_size(path: &Path, ctx: &StepContext) -> anyhow::Result<u64> {
    ctx.check_cancelled()?;
    let m = std::fs::symlink_metadata(path)?;
    if !m.is_dir() {
        return Ok(if m.is_file() && ctx.sized_once(&m) {
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

/// Bytes per `copy_file_range` call: large enough to stay in the kernel's
/// fast path, small enough that Cancel is answered within a fraction of a
/// second on a slow disk.
const COPY_CHUNK: usize = 8 << 20;

fn copy_file(
    src: &Path,
    dst: &Path,
    preserve_attributes: bool,
    pace: &Pace,
    ctx: &StepContext,
) -> anyhow::Result<()> {
    // Where the system copies a whole file (Windows), it does: it keeps
    // what a byte copy would lose there.
    let mut written = 0;
    let whole = sys::copy_whole(src, dst, preserve_attributes, &mut |total| {
        // Counted as the system says: nothing is written through off
        // Linux yet (`fs::volume::is_removable`).
        ctx.advance(total.saturating_sub(written));
        written = total;
        ctx.check_cancelled().is_ok()
    });
    if let Some(copied) = whole {
        // Stopped by Cancel, the copy is cancelled, not failed.
        if copied.is_err() {
            ctx.check_cancelled()?;
        }
        copied.with_context(|| format!("“{}” could not be copied", display_name(src)))?;
        // Counted whole, whatever the system said as it went: Wine's copy
        // says nothing, and its bytes were never counted, nor the plan
        // paused between its files.
        if let Ok(meta) = std::fs::symlink_metadata(dst) {
            ctx.advance(meta.len().saturating_sub(written));
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
        .with_context(|| format!("“{}” is no longer a file", display_name(src)))?;
    let df = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dst)?;
    let m = sf.metadata()?;
    // As it was made: on a remote, said by the server's answer to the
    // making, so asked of no one.
    let made = df.metadata()?;
    // What the kernel refused between these devices once, it is not asked
    // for each file again.
    let devices = sys::devices(&m, &made);
    let cloned = !ctx.kernel_refuses(devices, Kernel::Clone) && sys::clone_file(&sf, &df);
    if cloned {
        ctx.advance(m.len());
    } else {
        ctx.kernel_refused(devices, Kernel::Clone);
        copy_contents(&sf, &df, (&m, devices), pace, ctx)
            .with_context(|| format!("“{}” could not be copied", display_name(src)))?;
    }
    // Set attributes before restoring a read-only source's permissions.
    if preserve_attributes {
        transfer_attributes(src, dst, ctx)?;
    }
    // Made with them already (the usual 0644 and 022), they are not asked
    // for again: on a remote each change is a round trip to the server.
    if made.permissions() != m.permissions() {
        sys::mode_kept(df.set_permissions(m.permissions()))?;
    }
    let _ = df.set_modified(m.modified()?);
    Ok(())
}

/// Copy the bytes in chunks, checking for Cancel and reporting progress
/// between them (as `pace` counts it): in the kernel where it can
/// (`copy_file_range`), through a buffer where it cannot. A file with
/// holes keeps them, where the copy is left to the system: a disk image
/// of 100 GB holding 5 copied 100.
fn copy_contents(
    mut sf: &std::fs::File,
    mut df: &std::fs::File,
    (meta, devices): (&std::fs::Metadata, Option<(u64, u64)>),
    pace: &Pace,
    ctx: &StepContext,
) -> anyhow::Result<()> {
    if pace.is_at_once() && sys::is_sparse(meta) {
        return copy_sparse(sf, df, meta.len(), devices, ctx);
    }
    let mut writing = pace.writing(df);
    let mut copied = 0u64;
    // A large file on a FUSE remote, read in ranges at once; what it grew
    // by since, the loops below take from where it was read to.
    #[cfg(unix)]
    if super::ranges::wanted(meta) {
        use std::io::Seek;
        copied = super::ranges::copy(sf, df, meta.len(), &mut writing, ctx)?;
        sf.seek(std::io::SeekFrom::Start(copied))?;
    }
    while !ctx.kernel_refuses(devices, Kernel::Range) {
        ctx.check_cancelled()?;
        match sys::copy_range(sf, df, COPY_CHUNK)? {
            Some(0) if copied > 0 => return writing.close(ctx),
            Some(n @ 1..) => {
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
    loop {
        ctx.check_cancelled()?;
        let n = match sf.read(&mut buf) {
            Ok(0) => return writing.close(ctx),
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        };
        df.write_all(&buf[..n])?;
        writing.wrote(n as u64, ctx)?;
    }
}

/// Copy a file's stretches of data, leaving its holes holes, then give
/// the copy its length: its holes count as done.
fn copy_sparse(
    sf: &std::fs::File,
    df: &std::fs::File,
    len: u64,
    devices: Option<(u64, u64)>,
    ctx: &StepContext,
) -> anyhow::Result<()> {
    #[cfg(unix)]
    use std::os::unix::fs::FileExt;
    let mut at = 0;
    while let Some((data, hole)) = sys::next_data(sf, at)? {
        ctx.advance(data.saturating_sub(at));
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
                    anyhow::bail!("a file with holes is copied on Unix alone")
                }
            };
            from += n as u64;
            ctx.advance(n as u64);
        }
        at = hole.max(from);
    }
    ctx.advance(len.saturating_sub(at));
    df.set_len(len)?;
    Ok(())
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
    if kind.is_symlink() {
        let target = std::fs::read_link(src)?;
        sys::symlink_like(&target, dst, kind)?;
    } else if kind.is_dir() {
        std::fs::create_dir(dst)?;
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
        sys::make_fifo(dst, &m)
            .with_context(|| format!("the pipe “{}” could not be made", display_name(dst)))?;
    } else {
        let what = if sys::is_socket(kind) {
            "a socket"
        } else {
            "a device"
        };
        anyhow::bail!("“{}” is {what}, which cannot be copied", display_name(src));
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
    let mut all = own.clone();
    for (name, value) in beside {
        if !all.iter().any(|(n, _)| *n == name) {
            all.push((name, value));
        }
    }
    let mut refused = false;
    for (name, value) in &all {
        match crate::fs::xattr::set(dst, name, value) {
            Ok(()) => {}
            Err(e) if crate::fs::xattr::refused(&e) => {
                refused = true;
                break;
            }
            Err(e) => {
                return Err(e).context(
                    "keep tags and other attributes while copying; check destination xattr support and permissions, or turn off “Keep tags and attributes when copying” in Settings",
                );
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
    }
    if (aside.is_empty() || !own.is_empty()) && !rules.keep_attributes(dst, &own)? {
        anyhow::bail!(
            "keep tags and other attributes while copying: {} keeps no attributes, and nothing is set to keep them beside it; turn off “Keep tags and attributes when copying” in Settings to copy without them",
            dst.parent().unwrap_or(dst).display()
        );
    }
    Ok(())
}

/// The extended attributes of `path` a copy keeps, none on a volume that
/// keeps none: on Linux the user's (`user.*`); on macOS, whose names have
/// no namespace, every one but those the system sets itself and refuses
/// to anyone else (`com.apple.rootless`, `com.apple.provenance`), Finder's
/// tags and info among those kept.
pub(super) fn user_attributes(path: &Path) -> anyhow::Result<Vec<(String, Vec<u8>)>> {
    let names = match crate::fs::xattr::list(path) {
        Ok(names) => names,
        Err(error) if crate::fs::xattr::refused(&error) => return Ok(Vec::new()),
        Err(error) => {
            return Err(error).with_context(|| {
                format!(
                    "the tags and attributes of “{}” could not be read",
                    display_name(path)
                )
            });
        }
    };
    let mut out = Vec::new();
    for name in names.into_iter().filter(|n| crate::fs::xattr::kept(n)) {
        // One removed since it was listed is not there to keep.
        let Some(value) = crate::fs::xattr::get(path, &name).with_context(|| {
            format!(
                "the tags and attributes of “{}” could not be read",
                display_name(path)
            )
        })?
        else {
            continue;
        };
        out.push((name.to_string_lossy().into_owned(), value));
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
            anyhow::bail!("{why}");
        }
        std::fs::remove_dir_all(p).map_err(|e| {
            let error = anyhow::Error::new(e);
            let Some((first, count)) = left_in(p) else {
                return error;
            };
            let shown = first
                .strip_prefix(p.parent().unwrap_or(Path::new("")))
                .unwrap_or(&first)
                .display();
            error.context(match count {
                1 => format!("“{shown}” is left"),
                n => format!("“{shown}” and {} more are left", n - 1),
            })
        })?;
    } else {
        std::fs::remove_file(p)?;
    }
    Ok(())
}

/// Why `path` may not be removed or trashed whole: it is, or holds, the
/// folder where a drive or a remote is mounted (ADR-077). A removal would
/// reach into it, deleting a drive's files or a server's.
fn mounted_there(path: &Path) -> Option<String> {
    let points = crate::fs::volume::mounted_within(path);
    let point = points.first()?;
    Some(if point == path {
        format!(
            "“{}” is where a drive or a remote is mounted: eject it rather than delete it",
            display_name(path)
        )
    } else {
        format!(
            "“{}” holds “{}”, where a drive or a remote is mounted: eject it first",
            display_name(path),
            point.strip_prefix(path).unwrap_or(point).display()
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
            return conflict("Windows cannot swap two names in one step");
        }
        if let Some(trashed) = trashed(&self.a.path).or_else(|| trashed(&self.b.path)) {
            return trashed;
        }
        match (self.a.resolve(), self.b.resolve()) {
            (Ok(_), Ok(_)) => pending(true),
            (Err(e), _) | (_, Err(e)) => conflict(e.to_string()),
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
        sys::exchange(&pa, &pb).with_context(|| {
            format!(
                "“{}” and “{}” could not swap names",
                display_name(&pa),
                display_name(&pb)
            )
        })?;
        for (from, to) in pairs {
            if exists(&to) {
                sys::exchange(&from, &to).with_context(|| {
                    format!(
                        "“{}” and “{}” could not swap names",
                        display_name(&from),
                        display_name(&to)
                    )
                })?;
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
    fn describe(&self) -> Change {
        Change::Trash {
            path: self.entry.path.clone(),
        }
    }
    fn check(&self, _ctx: &StepContext) -> StepStatus {
        if in_trash(&self.entry.path).is_some() {
            return conflict(format!(
                "“{}” is in the trash already",
                display_name(&self.entry.path)
            ));
        }
        match self.entry.resolve() {
            Ok(path) => mounted_there(&path).map_or_else(|| pending(true), conflict),
            Err(ResolveError::Gone(_)) => StepStatus::AlreadyDone,
            Err(e) => conflict(e.to_string()),
        }
    }
    fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let src = self.entry.resolve()?;
        if let Some(why) = mounted_there(&src) {
            anyhow::bail!("{why}");
        }
        anyhow::ensure!(
            in_trash(&src).is_none(),
            "“{}” is in the trash already",
            display_name(&src)
        );
        ctx.forget_trash();
        let companions: Vec<PathBuf> = ctx
            .companions()
            .existing(&src)
            .into_iter()
            .map(|f| f.path)
            .collect();
        let at = trash_item(ctx, &src)
            .with_context(|| format!("“{}” could not be moved to the trash", display_name(&src)))?;
        // Each an item of the trash of its own, restored with the file:
        // found by when it was trashed where the trash keeps records, by
        // where it went on macOS.
        let mut landed = Vec::new();
        for companion in companions {
            if let Ok(Some(at)) = trash_item(ctx, &companion)
                && cfg!(target_os = "macos")
            {
                landed.push(Landed {
                    at: PathArg(at),
                    from: PathArg(companion),
                });
            }
        }
        Ok(Some(Box::new(Restore {
            path: src,
            item: at.map(PathArg),
            landed,
        })))
    }
}

/// Put `path` in the trash, and say where it went where it can be told:
/// its record on Linux, where the plan puts it there itself, its trash
/// found once (`put_trash`), and one going to a trash on another
/// filesystem is moved there as any move across volumes is; where the
/// system put it on macOS (`sys::trash_item`).
fn trash_item(ctx: &StepContext, path: &Path) -> anyhow::Result<Option<PathBuf>> {
    #[cfg(target_os = "linux")]
    {
        match ctx.trashes().put(path)? {
            super::put_trash::Put::Done { info } => Ok(Some(info)),
            super::put_trash::Put::Across { item, info } => {
                if let Err(error) = move_entry(ctx, path, &item, &[]) {
                    let _ = std::fs::remove_file(&info);
                    return Err(error);
                }
                Ok(Some(info))
            }
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = ctx;
        Ok(sys::trash_item(path)?)
    }
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
        let file = item_of_info(&info)
            .ok_or_else(|| anyhow::anyhow!("“{}” is no trash's record", display_name(&info)))?;
        let to = item.original_path();
        std::fs::create_dir_all(&item.original_parent)?;
        anyhow::ensure!(
            !exists(&to),
            "“{}” is there again: rename or move it first",
            display_name(&to)
        );
        move_entry(ctx, &file, &to, &[])?;
        if let Err(error) = std::fs::remove_file(&info)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            return Err(error.into());
        }
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = ctx;
        Ok(sys::trash_restore(vec![item])?)
    }
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
        let item = item_of_info(info)
            .ok_or_else(|| anyhow::anyhow!("“{}” is no trash's record", display_name(info)))?;
        let info_text = read_record(info).with_context(|| {
            format!(
                "the trash's record “{}” could not be read",
                display_name(info)
            )
        })?;
        if let Some(dir) = to.parent() {
            std::fs::create_dir_all(dir)?;
        }
        move_entry(ctx, &item, to, &[])?;
        let _ = std::fs::remove_file(info);
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

    /// Back in the trash where it was, as the trash said.
    fn put_back(&self, ctx: &StepContext) -> anyhow::Result<()> {
        let item = item_of_info(&self.info).ok_or_else(|| {
            anyhow::anyhow!("“{}” is no trash's record", display_name(&self.info))
        })?;
        if !exists(&item) {
            move_entry(ctx, &self.path, &item, &[])?;
        }
        if !self.info.exists() {
            std::fs::write(&self.info, &self.info_text).with_context(|| {
                format!(
                    "the trash's record “{}” could not be written",
                    display_name(&self.info)
                )
            })?;
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
                "“{}” is there again: rename or move it first",
                display_name(&dest)
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
    fn describe(&self) -> Change {
        Change::Restore {
            path: self.path.clone(),
        }
    }
    fn check(&self, ctx: &StepContext) -> StepStatus {
        if let Some(item) = self.landed_item() {
            return match (exists(&self.path), exists(item)) {
                (true, false) => StepStatus::AlreadyDone,
                (true, true) => conflict(format!(
                    "“{}” is there again: rename or move it first",
                    display_name(&self.path)
                )),
                (false, true) => pending(true),
                (false, false) => conflict(format!(
                    "“{}” is not in the trash",
                    display_name(&self.path)
                )),
            };
        }
        if exists(&self.path) {
            // A folder there again takes the trashed folder's files where
            // their places are free.
            if let Some(item) = self.merging() {
                return match clash(&item, &self.path) {
                    Ok(None) => pending(true),
                    Ok(Some(taken)) => conflict(format!(
                        "“{}” is there again: rename or move it first",
                        display_name(&taken)
                    )),
                    Err(e) => conflict(format!("“{}” could not be read: {e}", display_name(&item))),
                };
            }
            // The item named still in the trash: another file took its
            // place since, which restoring it would not overwrite.
            if self.item.as_deref().is_some_and(Path::exists) {
                return conflict(format!(
                    "“{}” is there again: rename or move it first",
                    display_name(&self.path)
                ));
            }
            return StepStatus::AlreadyDone;
        }
        match ctx.with_trash_near(&self.near(), |listing| self.find(listing)) {
            Ok(Some(_)) => pending(true),
            Ok(None) => conflict(format!(
                "“{}” is not in the trash",
                display_name(&self.path)
            )),
            Err(e) => conflict(format!("the trash could not be read: {e}")),
        }
    }
    fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        if let Some(item) = self.landed_item() {
            rename_noreplace(item, &self.path)
                .with_context(|| format!("“{}” could not be restored", display_name(&self.path)))?;
            for companion in &self.landed {
                if !exists(&companion.from) {
                    let _ = rename_noreplace(&companion.at, &companion.from);
                }
            }
            let r = EntryRef::from_path(&self.path).ok_or_else(|| {
                anyhow::anyhow!(
                    "“{}” went missing as it was restored",
                    display_name(&self.path)
                )
            })?;
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
            .context("the trash could not be read")?
            .ok_or_else(|| anyhow::anyhow!("“{}” is not in the trash", display_name(&self.path)))?;
        let when = item.time_deleted;
        restore_listed(ctx, item)
            .with_context(|| format!("“{}” could not be restored", display_name(&self.path)))?;
        // The companions trashed with it, where their places are free.
        let places = companion_places(ctx, &self.path);
        let found: Vec<trash::TrashItem> =
            take_trashed_with(ctx, &places, when, |n| !exists(&places[n]))
                .into_iter()
                .map(|(_, item)| item)
                .collect();
        for item in found {
            let _ = restore_listed(ctx, item);
        }
        let r = EntryRef::from_path(&self.path).ok_or_else(|| {
            anyhow::anyhow!(
                "“{}” went missing as it was restored",
                display_name(&self.path)
            )
        })?;
        Ok(Some(Box::new(Trash { entry: r })))
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
        return Err(e.context(format!("“{}” could not be restored", display_name(path))));
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
            return conflict(format!("“{}” is no trash's", display_name(&self.info)));
        };
        let back = |inner: &PathArg| exists(&item.join(inner));
        if self.info.exists()
            && self.moved.iter().all(back)
            && self.companions.iter().all(Untrashed::is_back)
        {
            return StepStatus::AlreadyDone;
        }
        if let Some(gone) = self
            .moved
            .iter()
            .find(|inner| !back(inner) && !exists(&self.path.join(inner)))
        {
            return conflict(format!(
                "“{}” is no longer there",
                display_name(&self.path.join(gone))
            ));
        }
        pending(true)
    }
    fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let item = item_of_info(&self.info).ok_or_else(|| {
            anyhow::anyhow!("“{}” is no trash's record", display_name(&self.info))
        })?;
        ctx.forget_trash();
        for inner in self.folders.iter().rev() {
            std::fs::create_dir_all(item.join(inner)).with_context(|| {
                format!(
                    "the folder “{}” could not be made",
                    item.join(inner).display()
                )
            })?;
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
            std::fs::write(&self.info, &self.info_text).with_context(|| {
                format!(
                    "the trash's record “{}” could not be written",
                    display_name(&self.info)
                )
            })?;
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
    for (name, value) in user_attributes(like).unwrap_or_default() {
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
    fn check(&self, ctx: &StepContext) -> StepStatus {
        let src = match self.entry.resolve() {
            Ok(src) => src,
            Err(ResolveError::Gone(_)) if exists(&self.path) => return StepStatus::AlreadyDone,
            Err(ResolveError::Gone(_)) => {
                return conflict(format!(
                    "“{}” is no longer in the trash",
                    display_name(&self.entry.path)
                ));
            }
            Err(e) => return conflict(e.to_string()),
        };
        let Some(inside) = in_trash(&src) else {
            return conflict(format!("“{}” is not in the trash", display_name(&src)));
        };
        if self.path.starts_with(&inside.item) {
            return conflict(format!(
                "“{}” would stay in the trash",
                display_name(&self.path)
            ));
        }
        if exists(&self.path) {
            return conflict(format!(
                "“{}” is there again: rename or move it first",
                display_name(&self.path)
            ));
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
            return conflict(format!("“{}” is not a folder", display_name(there)));
        }
        pending(true)
    }
    fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let src = self.entry.resolve()?;
        let inside = in_trash(&src)
            .ok_or_else(|| anyhow::anyhow!("“{}” is not in the trash", display_name(&src)))?;
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
            Some(read_record(&inside.info).with_context(|| {
                format!(
                    "the trash's record “{}” could not be read",
                    display_name(&inside.info)
                )
            })?)
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
                return Err(e)
                    .with_context(|| format!("the folder “{}” could not be made", dir.display()));
            }
        }
        let companions = if item {
            Vec::new()
        } else {
            along(ctx, &src, &self.path)
        };
        if let Err(e) = move_entry(ctx, &src, &self.path, &companions) {
            undo_folders(&folders);
            return Err(e.context(format!(
                "“{}” could not be restored",
                display_name(&self.path)
            )));
        }
        if item {
            ctx.trash_taken(&inside.info);
            let _ = std::fs::remove_file(&inside.info);
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
            std::fs::write(&info, text).with_context(|| {
                format!(
                    "the trash's record “{}” could not be written",
                    display_name(&info)
                )
            })?;
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
                conflict(format!("“{}” is no longer there", display_name(&self.path)))
            };
        }
        if exists(&self.to) {
            return conflict(format!(
                "“{}” is in its place in the trash",
                display_name(&self.to)
            ));
        }
        if !self.to.parent().is_some_and(Path::is_dir) {
            return conflict(format!(
                "the folder of the trash “{}” was in is gone",
                display_name(&self.path)
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
            .ok_or_else(|| anyhow::anyhow!("“{}” is not in the trash", display_name(&self.to)))?;
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
    fn describe(&self) -> Change {
        Change::Delete {
            path: self.entry.path.clone(),
        }
    }
    fn check(&self, _ctx: &StepContext) -> StepStatus {
        match self.entry.resolve() {
            Ok(path) => mounted_there(&path).map_or_else(|| pending(false), conflict),
            Err(ResolveError::Gone(_)) => StepStatus::AlreadyDone,
            Err(e) => conflict(e.to_string()),
        }
    }
    fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let src = self.entry.resolve()?;
        let companions = ctx.companions().existing(&src);
        remove_recursive(&src)
            .with_context(|| format!("“{}” could not all be deleted", display_name(&src)))?;
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
    trash_info_of(path).map(|_| {
        conflict(format!(
            "“{}” is in the trash: restore it first",
            path.file_name().unwrap_or(path.as_os_str()).display()
        ))
    })
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
        return Some(conflict(format!("“{}” contains “..”", path.display())));
    }
    match path.file_name().map(invalid_name) {
        None => Some(conflict(format!("“{}” has no name", path.display()))),
        Some(invalid) => invalid,
    }
}

impl super::Effect for Mkdir {
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
            Ok(_) => conflict(format!(
                "“{}” is there, and is not a folder",
                display_name(&self.path)
            )),
            Err(_) if self.path.parent().is_some_and(|p| ctx.is_folder(p)) => pending(true),
            Err(_) => conflict("the folder it goes in is not there"),
        }
    }
    fn apply(&self, _ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        std::fs::create_dir(&self.path).with_context(|| {
            format!(
                "the folder “{}” could not be made",
                display_name(&self.path)
            )
        })?;
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
    fn describe(&self) -> Change {
        Change::Remove {
            path: self.path.clone(),
        }
    }
    fn check(&self, ctx: &StepContext) -> StepStatus {
        // Emptied by the plan's earlier steps (a folder a merge moved
        // everything out of, ADR-054) is empty.
        match ctx.empty_after(&self.path) {
            Ok(true) => pending(true),
            Ok(false) => conflict(format!("“{}” is not empty", display_name(&self.path))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => StepStatus::AlreadyDone,
            Err(e) => conflict(e.to_string()),
        }
    }
    fn apply(&self, _ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        std::fs::remove_dir(&self.path).with_context(|| {
            format!(
                "the folder “{}” could not be removed",
                display_name(&self.path)
            )
        })?;
        Ok(Some(Box::new(Mkdir {
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
            Ok(_) => conflict(format!("“{}” is already there", display_name(&self.path))),
            Err(_) if self.path.parent().is_some_and(|p| ctx.is_folder(p)) => pending(true),
            Err(_) => conflict("the folder it goes in is not there"),
        }
    }
    fn apply(&self, _ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&self.path)
            .with_context(|| format!("“{}” could not be made", display_name(&self.path)))?;
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
    fn describe(&self) -> Change {
        Change::Remove {
            path: self.path.clone(),
        }
    }
    fn check(&self, _ctx: &StepContext) -> StepStatus {
        match std::fs::symlink_metadata(&self.path) {
            Ok(m) if m.is_file() && m.len() == 0 => pending(true),
            Ok(m) if m.is_file() => {
                conflict(format!("“{}” is no longer empty", display_name(&self.path)))
            }
            Ok(_) => conflict(format!("“{}” is not a file", display_name(&self.path))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => StepStatus::AlreadyDone,
            Err(e) => conflict(e.to_string()),
        }
    }
    fn apply(&self, _ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        std::fs::remove_file(&self.path)
            .with_context(|| format!("“{}” could not be removed", display_name(&self.path)))?;
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
            return conflict(format!(
                "“{}” is not a folder",
                display_name(&self.dest_dir)
            ));
        }
        let src = match self.entry.resolve() {
            Ok(s) => s,
            Err(e) => return conflict(e.to_string()),
        };
        let link = self.target(&src);
        if ctx.made(&link) {
            return conflict(format!(
                "“{}” is already there, by an earlier step",
                display_name(&link)
            ));
        }
        if !ctx.exists(&link) {
            return pending(true);
        }
        match std::fs::read_link(&link) {
            // What this step makes: applied already.
            Ok(says) if says == src => StepStatus::AlreadyDone,
            _ => conflict(format!("“{}” is already there", display_name(&link))),
        }
    }
    fn apply(&self, _ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        if let Some(name) = &self.new_name {
            valid_file_name(name)?;
        }
        let src = self.entry.resolve()?;
        let link = self.target(&src);
        sys::symlink_to(&src, &link, src.is_dir()).with_context(|| {
            format!(
                "“{}” could not be linked in “{}”",
                display_name(&src),
                display_name(&self.dest_dir)
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
    fn describe(&self) -> Change {
        Change::Remove {
            path: self.path.clone(),
        }
    }
    fn check(&self, _ctx: &StepContext) -> StepStatus {
        match std::fs::read_link(&self.path) {
            Ok(says) if says == self.entry.path => pending(true),
            Ok(_) => conflict(format!(
                "“{}” leads elsewhere now",
                display_name(&self.path)
            )),
            Err(_) if !exists(&self.path) => StepStatus::AlreadyDone,
            Err(_) => conflict(format!(
                "“{}” is no longer a link",
                display_name(&self.path)
            )),
        }
    }
    fn apply(&self, _ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        sys::remove_link(&self.path)
            .with_context(|| format!("“{}” could not be removed", display_name(&self.path)))?;
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
    reg.register::<NewFile>("core");
    reg.register::<RemoveEmptyFile>("core");
    reg.register::<Link>("core");
    reg.register::<Unlink>("core");
    reg.register::<super::access::SetAccess>("core");
    reg.register::<super::access::AccessBack>("core");
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    /// What is, or holds, where something is mounted is never removed or
    /// trashed whole: a removal would reach into a drive's files or a
    /// server's (ADR-077). Read from this machine's own mounts (`/proc`).
    #[cfg(target_os = "linux")]
    #[test]
    fn a_mount_point_is_never_removed_whole() {
        let proc = mounted_there(Path::new("/proc")).expect("/proc is mounted");
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
        let above = mounted_there(holder).expect("it holds a mount");
        assert!(above.contains("holds"), "{above}");
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(mounted_there(tmp.path()), None, "a plain folder");
        let err = remove_recursive(Path::new("/proc")).unwrap_err();
        assert!(err.to_string().contains("eject"), "{err}");
    }

    /// Where a trash keeps what it says of a file, from the file's path:
    /// the home trash's and a volume's two kinds, nothing elsewhere.
    #[test]
    fn a_file_of_a_trash_is_known_by_its_path() {
        let info = |p: &str| trash_info_of(Path::new(p));
        assert_eq!(
            info("/home/u/.local/share/Trash/files/a b.txt"),
            Some(PathBuf::from(
                "/home/u/.local/share/Trash/info/a b.txt.trashinfo"
            ))
        );
        assert_eq!(
            info("/media/stick/.Trash-1000/files/a"),
            Some(PathBuf::from("/media/stick/.Trash-1000/info/a.trashinfo"))
        );
        assert_eq!(
            info("/media/stick/.Trash/1000/files/a"),
            Some(PathBuf::from("/media/stick/.Trash/1000/info/a.trashinfo"))
        );
        // Inside a folder of the trash, or a folder merely named so.
        assert_eq!(info("/home/u/.local/share/Trash/files/dir/a"), None);
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
