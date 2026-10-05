//! The built-in operations: rename, move, copy, swap, trash, restore, delete,
//! mkdir, rmdir, new file and its removal. Each is idempotent (`check` detects an effect already in
//! place) and returns its inverse from `apply`.
//!
//! Names are `OsString`s and paths `PathBuf`s from end to end, serialized
//! through `crate::os_path`: a name that is not UTF-8 must survive a rename
//! and its undo byte for byte.

use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::{
    Change, EntryRef, OpRegistry, Operation, ResolveError, StepContext, StepStatus, valid_file_name,
};
use crate::fs::stat::id_of;
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
        let candidate = dir.join(format!(".bibli-{purpose}-{pid}-{n}"));
        if !exists(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

fn rename_noreplace(from: &Path, to: &Path) -> anyhow::Result<()> {
    match rustix::fs::renameat_with(
        rustix::fs::CWD,
        from,
        rustix::fs::CWD,
        to,
        rustix::fs::RenameFlags::NOREPLACE,
    ) {
        // Many FUSE filesystems (sshfs, bindfs, exfat-fuse) and some
        // network ones take no flags: look, then rename. What appears in
        // between is replaced; nothing better is offered there.
        Err(rustix::io::Errno::INVAL | rustix::io::Errno::NOSYS) if !exists(to) => {
            // As an `Errno`, so a move still tells a cross-device rename.
            std::fs::rename(from, to).map_err(|e| {
                rustix::io::Errno::from_io_error(&e)
                    .map_or_else(|| anyhow::Error::from(e), anyhow::Error::from)
            })
        }
        Err(rustix::io::Errno::INVAL | rustix::io::Errno::NOSYS) => {
            Err(anyhow::Error::from(rustix::io::Errno::EXIST))
        }
        r => r.map_err(anyhow::Error::from),
    }
    .with_context(|| format!("rename {} -> {}", from.display(), to.display()))
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

/// The first companion whose place is taken: the step conflicts as it does
/// when the file's own place is.
fn along_conflict(along: &[Along]) -> Option<StepStatus> {
    along
        .iter()
        .find(|a| exists(&a.to))
        .map(|a| conflict(format!("{} already exists", a.to.display())))
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

fn unsupported(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::Unsupported
        || rustix::io::Errno::from_io_error(error) == Some(rustix::io::Errno::NOTSUP)
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
            Ok(src) => match self.classify(&src, &target) {
                RenameTarget::Free | RenameTarget::Respelled => {
                    along_conflict(&along(ctx, &src, &target)).unwrap_or(pending(true))
                }
                RenameTarget::Spelled => StepStatus::AlreadyDone,
                RenameTarget::Taken => conflict(format!("{} already exists", target.display())),
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
                rename_noreplace(&src, &parked)?;
                if let Err(e) = rename_noreplace(&parked, &target) {
                    rename_noreplace(&parked, &src).context("put the entry back")?;
                    return Err(e);
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
            entry: EntryRef::new(self.entry.id, target),
            new_name: file_name(&src),
        })))
    }
}

// ------------------------------------------------------------------ move

/// Move into another directory, keeping the name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Move {
    pub entry: EntryRef,
    #[serde(with = "crate::os_path")]
    #[schemars(with = "PathArg")]
    pub dest_dir: PathBuf,
}

impl Move {
    fn target(&self, src: &Path) -> PathBuf {
        self.dest_dir.join(src.file_name().unwrap_or_default())
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
    fn check(&self, ctx: &StepContext) -> StepStatus {
        if !self.dest_dir.is_dir() {
            return conflict(format!("{} is not a directory", self.dest_dir.display()));
        }
        if let Some(trashed) = trashed(&self.entry.path) {
            return trashed;
        }
        match self.entry.resolve() {
            Ok(src) if src.parent() == Some(self.dest_dir.as_path()) => StepStatus::AlreadyDone,
            Ok(src) if exists(&self.target(&src)) => {
                conflict(format!("{} already exists", self.target(&src).display()))
            }
            Ok(src) if src.is_dir() && self.dest_dir.starts_with(&src) => {
                conflict("cannot move a directory into itself")
            }
            Ok(src) => {
                along_conflict(&along(ctx, &src, &self.target(&src))).unwrap_or(pending(true))
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
        let src = self.entry.resolve()?;
        let target = self.target(&src);
        let origin = src.parent().unwrap_or(Path::new("/")).to_path_buf();
        let along = along(ctx, &src, &target);
        match rename_noreplace(&src, &target) {
            Ok(()) => {
                if let Err(e) = rename_along(&along) {
                    let _ = rename_noreplace(&target, &src);
                    return Err(e);
                }
            }
            Err(e) if is_cross_device(&e) => {
                // A move must never lose metadata, whatever the copy preference
                // says, and a failed transfer must not leave a partial target.
                // The companion holding the file's attributes is converted
                // with them (ADR-028); the others go as files.
                copy_entry(&src, &target, &self.dest_dir, true, &along, ctx)?;
                remove_recursive(&src)?;
                for a in &along {
                    let _ = remove_recursive(&a.from);
                }
            }
            Err(e) => return Err(e),
        }
        let id = id_of(&target).unwrap_or(self.entry.id);
        Ok(Some(Box::new(Move {
            entry: EntryRef::new(id, target),
            dest_dir: origin,
        })))
    }
}

fn is_cross_device(e: &anyhow::Error) -> bool {
    e.downcast_ref::<rustix::io::Errno>()
        .is_some_and(|e| *e == rustix::io::Errno::XDEV)
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
    /// the plan is created, so redo does not consult live settings; plans
    /// written before attributes other than tags were kept say
    /// `preserve_tags`, which means the same now.
    #[serde(default = "preserve_attributes_default", alias = "preserve_tags")]
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
    fn target(&self, src: &Path) -> PathBuf {
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
    fn check(&self, ctx: &StepContext) -> StepStatus {
        if let Some(invalid) = self.new_name.as_deref().and_then(invalid_name) {
            return invalid;
        }
        if !self.dest_dir.is_dir() {
            return conflict(format!("{} is not a directory", self.dest_dir.display()));
        }
        let src = match self.entry.resolve() {
            Ok(s) => s,
            Err(e) => return conflict(e.to_string()),
        };
        let target = self.target(&src);
        if target == src {
            return conflict("source and destination are the same");
        }
        if exists(&target) {
            // Only a finished copy of this very source is "already done": an
            // unrelated folder that happens to share the name is a conflict.
            return match same_tree(&src, &target, ctx) {
                Ok(true) => StepStatus::AlreadyDone,
                Ok(false) => conflict(format!("{} already exists", target.display())),
                Err(e) => conflict(format!("{e:#}")),
            };
        }
        if src.is_dir() && target.starts_with(&src) {
            return conflict("cannot copy a directory into itself");
        }
        along_conflict(&along(ctx, &src, &target)).unwrap_or(pending(true))
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
        let r = EntryRef::from_path(&target)
            .ok_or_else(|| anyhow::anyhow!("copy vanished: {}", target.display()))?;
        // Undo puts the copy in the trash rather than deleting it: it may
        // have been edited since, and redo then restores it as it was.
        Ok(Some(Box::new(Trash { entry: r })))
    }
}

/// Whether `copy` holds what copying `src` would produce: the same kinds,
/// names, sizes and file times all the way down (a copy keeps file mtimes),
/// and the same link targets. Stops at the first difference.
fn same_tree(src: &Path, copy: &Path, ctx: &StepContext) -> anyhow::Result<bool> {
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
/// written through ([`Pace`]): progress and Cancel follow the drive however
/// slow it is, and the whole copy, names included, is on it when the step
/// ends. Written only when unmounted, it could be unplugged, or reset
/// itself (a device taking its firmware), before the writes end, and lose
/// them.
fn copy_entry(
    src: &Path,
    target: &Path,
    dest_dir: &Path,
    preserve_attributes: bool,
    along: &[Along],
    ctx: &StepContext,
) -> anyhow::Result<()> {
    let pace = if crate::fs::volume::is_removable(dest_dir) {
        Pace::through(dest_dir)?
    } else {
        Pace::at_once()
    };
    copy_staged(src, target, dest_dir, preserve_attributes, &pace, ctx)?;
    copy_along(along, &pace, ctx)?;
    pace.settle(ctx)
}

/// Copy `src` to `target` through a private staging directory inside
/// `dest_dir`, publishing with a no-replace rename only after content and
/// attributes are complete. A failed attribute write must not leave a partial destination
/// that `check()` would call already done. Shared by Copy and cross-device Move.
fn copy_staged(
    src: &Path,
    target: &Path,
    dest_dir: &Path,
    preserve_attributes: bool,
    pace: &Pace,
    ctx: &StepContext,
) -> anyhow::Result<()> {
    ctx.add_total(tree_size(src, ctx)?);
    let staging = tempfile::Builder::new()
        .prefix(".bibli-copy-")
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir_in(dest_dir)?;
    let staged = staging.path().join("entry");
    // What the copy put beside the staged entry (the companion that holds
    // its attributes, on a volume that refuses them) follows it out.
    let result = copy_recursive_with_attributes(src, &staged, preserve_attributes, pace, ctx)
        .and_then(|()| {
            let beside = along(ctx, &staged, target);
            rename_noreplace(&staged, target)?;
            rename_along(&beside)
        });
    if let Err(error) = result {
        // Completed subdirectories may already have read-only permissions.
        // Only relax permissions inside our private, newly-created staging tree.
        if let Err(cleanup) = remove_staged_copy(staging.path()) {
            return Err(error).context(format!("copy staging cleanup failed: {cleanup}"));
        }
        return Err(error);
    }
    Ok(())
}

/// Bytes of regular files under `path`, so progress has a total.
fn tree_size(path: &Path, ctx: &StepContext) -> anyhow::Result<u64> {
    ctx.check_cancelled()?;
    let m = std::fs::symlink_metadata(path)?;
    if !m.is_dir() {
        return Ok(if m.is_file() { m.len() } else { 0 });
    }
    let mut total = 0;
    for e in std::fs::read_dir(path)? {
        total += tree_size(&e?.path(), ctx)?;
    }
    Ok(total)
}

fn remove_staged_copy(path: &Path) -> std::io::Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.is_dir() {
        std::fs::set_permissions(
            path,
            std::fs::Permissions::from_mode(metadata.permissions().mode() | 0o700),
        )?;
        for entry in std::fs::read_dir(path)? {
            remove_staged_copy(&entry?.path())?;
        }
        std::fs::remove_dir(path)
    } else {
        std::fs::remove_file(path)
    }
}

/// Bytes per `copy_file_range` call: large enough to stay in the kernel's
/// fast path, small enough that Cancel is answered within a fraction of a
/// second on a slow disk.
const COPY_CHUNK: usize = 8 << 20;

/// Written through, the first chunk, and the least: a chunk then takes
/// about `THROUGH_STEP` on the drive, so a slow one still shows progress
/// and answers Cancel, and a fast one is not stalled by a wait every few
/// bytes. Measured on the drive as it writes, never assumed of one.
const THROUGH_FIRST: u64 = 1 << 20;
const THROUGH_MIN: u64 = 128 << 10;
const THROUGH_STEP: std::time::Duration = std::time::Duration::from_millis(250);

/// How a step's copy counts what it wrote: at once, or, written through,
/// once it is on the drive. Through, every chunk's worth of bytes, whatever
/// files they belong to, waits for the drive to have everything written
/// (`syncfs`): a folder of small files waits once per chunk, not once per
/// file.
struct Pace {
    /// The destination's filesystem, written through.
    drive: Option<rustix::fd::OwnedFd>,
    /// Bytes to write before the next wait.
    chunk: std::cell::Cell<u64>,
    /// Bytes written since the last wait, not yet counted.
    unsaid: std::cell::Cell<u64>,
    since: std::cell::Cell<std::time::Instant>,
}

impl Pace {
    fn at_once() -> Self {
        Self {
            drive: None,
            chunk: std::cell::Cell::new(COPY_CHUNK as u64),
            unsaid: std::cell::Cell::new(0),
            since: std::cell::Cell::new(std::time::Instant::now()),
        }
    }

    fn through(dir: &Path) -> anyhow::Result<Self> {
        use rustix::fs::{Mode, OFlags};
        // Opened before anything is written: `syncfs` reports the errors
        // since.
        let drive = rustix::fs::open(
            dir,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
        )?;
        Ok(Self {
            drive: Some(drive),
            chunk: std::cell::Cell::new(THROUGH_FIRST),
            ..Self::at_once()
        })
    }

    /// How much to write at once.
    fn chunk(&self) -> usize {
        usize::try_from(self.chunk.get()).unwrap_or(COPY_CHUNK)
    }

    /// `n` more bytes were written.
    fn wrote(&self, n: u64, ctx: &StepContext) -> anyhow::Result<()> {
        if self.drive.is_none() {
            ctx.advance(n);
            return Ok(());
        }
        self.unsaid.set(self.unsaid.get() + n);
        if self.unsaid.get() >= self.chunk.get() {
            self.settle(ctx)?;
        }
        Ok(())
    }

    /// Written through, wait until the drive has everything written so far
    /// (names, folders and allocation tables with the bytes), count it, and
    /// size the next chunk by how long it took.
    fn settle(&self, ctx: &StepContext) -> anyhow::Result<()> {
        let Some(drive) = &self.drive else {
            return Ok(());
        };
        rustix::fs::syncfs(drive).context("write the copy to the drive")?;
        let n = self.unsaid.take();
        ctx.advance(n);
        if n > 0 {
            self.chunk.set(next_chunk(n, self.since.get().elapsed()));
        }
        self.since.set(std::time::Instant::now());
        Ok(())
    }
}

/// The chunk that takes about `THROUGH_STEP` on a drive that wrote `n`
/// bytes in `took`.
fn next_chunk(n: u64, took: std::time::Duration) -> u64 {
    let next = u128::from(n) * THROUGH_STEP.as_micros() / took.as_micros().max(1);
    u64::try_from(next)
        .unwrap_or(u64::MAX)
        .clamp(THROUGH_MIN, COPY_CHUNK as u64)
}

fn copy_file(
    src: &Path,
    dst: &Path,
    preserve_attributes: bool,
    pace: &Pace,
    ctx: &StepContext,
) -> anyhow::Result<()> {
    // Non-blocking, then checked: a file swapped for a FIFO since it was
    // listed must not block the executor in open(2).
    let mut sf = crate::fs::file::open_regular(src)
        .with_context(|| format!("{} is no longer a regular file", src.display()))?;
    let mut df = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dst)?;
    let m = sf.metadata()?;
    if rustix::fs::ioctl_ficlone(&df, &sf).is_ok() {
        ctx.advance(m.len());
    } else {
        copy_contents(&mut sf, &mut df, pace, ctx)
            .with_context(|| format!("copy {} -> {}", src.display(), dst.display()))?;
    }
    // Set attributes before restoring a read-only source's permissions.
    if preserve_attributes {
        transfer_attributes(src, dst, ctx)?;
    }
    df.set_permissions(m.permissions())?;
    let _ = df.set_modified(m.modified()?);
    Ok(())
}

/// Copy the bytes in chunks, checking for Cancel and reporting progress
/// between them (as `pace` counts it): in the kernel where it can
/// (`copy_file_range`), through a buffer where it cannot.
fn copy_contents(
    sf: &mut std::fs::File,
    df: &mut std::fs::File,
    pace: &Pace,
    ctx: &StepContext,
) -> anyhow::Result<()> {
    let mut copied = 0u64;
    loop {
        ctx.check_cancelled()?;
        match rustix::fs::copy_file_range(&*sf, None, &*df, None, pace.chunk()) {
            Ok(0) if copied > 0 => return Ok(()),
            Ok(n @ 1..) => {
                copied += n as u64;
                pace.wrote(n as u64, ctx)?;
            }
            // Some filesystems (procfs, a few FUSE) report 0 before the end,
            // so a first 0 is not trusted; others cannot copy in the kernel.
            // Either way the buffer takes over.
            Ok(0)
            | Err(
                rustix::io::Errno::XDEV
                | rustix::io::Errno::NOSYS
                | rustix::io::Errno::INVAL
                | rustix::io::Errno::OPNOTSUPP
                | rustix::io::Errno::PERM,
            ) => break,
            Err(e) => return Err(e.into()),
        }
    }
    // Both offsets advanced together, so the buffered copy resumes in place.
    let mut buf = vec![0; 1 << 20];
    loop {
        ctx.check_cancelled()?;
        let n = match sf.read(&mut buf) {
            Ok(0) => return Ok(()),
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        };
        df.write_all(&buf[..n])?;
        pace.wrote(n as u64, ctx)?;
    }
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
        std::os::unix::fs::symlink(target, dst)?;
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
        std::fs::set_permissions(dst, m.permissions())?;
    } else if kind.is_file() {
        copy_file(src, dst, preserve_attributes, pace, ctx)?;
    } else if kind.is_fifo() {
        // A named pipe has no content to copy; reading one would wait for a
        // writer forever. Make a new pipe, as `cp -R` does.
        rustix::fs::mknodat(
            rustix::fs::CWD,
            dst,
            rustix::fs::FileType::Fifo,
            rustix::fs::Mode::from_raw_mode(m.permissions().mode() & 0o7777),
            0,
        )
        .with_context(|| format!("create pipe {}", dst.display()))?;
    } else {
        let what = if kind.is_socket() {
            "a socket"
        } else {
            "a device"
        };
        anyhow::bail!("{} is {what}, which cannot be copied", src.display());
    }
    Ok(())
}

/// Copy companions (the ones not holding attributes: those are converted
/// with the file's) beside the copy.
fn copy_along(along: &[Along], pace: &Pace, ctx: &StepContext) -> anyhow::Result<()> {
    for a in along.iter().filter(|a| !a.attributes) {
        if let Some(dir) = a.to.parent() {
            std::fs::create_dir_all(dir)?;
        }
        copy_recursive_with_attributes(&a.from, &a.to, true, pace, ctx)?;
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
    let aside: Vec<Along> = along(ctx, src, dst)
        .into_iter()
        .filter(|a| a.attributes)
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
        match xattr::set(dst, name, value) {
            Ok(()) => {}
            Err(e) if unsupported(&e) => {
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
        // Small: the step settles it with the rest (`copy_entry`).
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

/// The user's extended attributes of `path` (`user.*`), none on a volume
/// that keeps none.
fn user_attributes(path: &Path) -> anyhow::Result<Vec<(String, Vec<u8>)>> {
    let names = match xattr::list(path) {
        Ok(names) => names,
        Err(error) if unsupported(&error) => return Ok(Vec::new()),
        Err(error) => return Err(error).context("read source attributes while copying"),
    };
    let mut out = Vec::new();
    for name in names.filter(|n| n.as_encoded_bytes().starts_with(b"user.")) {
        // One removed since it was listed is not there to keep.
        let Some(value) =
            xattr::get(path, &name).context("read a source attribute while copying")?
        else {
            continue;
        };
        out.push((name.to_string_lossy().into_owned(), value));
    }
    Ok(out)
}

fn remove_recursive(p: &Path) -> anyhow::Result<()> {
    let m = std::fs::symlink_metadata(p)?;
    if m.is_dir() {
        std::fs::remove_dir_all(p)?;
    } else {
        std::fs::remove_file(p)?;
    }
    Ok(())
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
        rustix::fs::renameat_with(
            rustix::fs::CWD,
            &pa,
            rustix::fs::CWD,
            &pb,
            rustix::fs::RenameFlags::EXCHANGE,
        )
        .with_context(|| format!("swap {} <-> {}", pa.display(), pb.display()))?;
        for (from, to) in pairs {
            if exists(&to) {
                rustix::fs::renameat_with(
                    rustix::fs::CWD,
                    &from,
                    rustix::fs::CWD,
                    &to,
                    rustix::fs::RenameFlags::EXCHANGE,
                )
                .with_context(|| format!("swap {} <-> {}", from.display(), to.display()))?;
            } else {
                rename_noreplace(&from, &to)?;
            }
        }
        Ok(Some(Box::new(Swap {
            a: EntryRef::new(self.a.id, pb),
            b: EntryRef::new(self.b.id, pa),
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
                "{} is in the trash already",
                self.entry.path.display()
            ));
        }
        match self.entry.resolve() {
            Ok(_) => pending(true),
            Err(ResolveError::Gone(_)) => StepStatus::AlreadyDone,
            Err(e) => conflict(e.to_string()),
        }
    }
    fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let src = self.entry.resolve()?;
        anyhow::ensure!(
            in_trash(&src).is_none(),
            "{} is in the trash already",
            src.display()
        );
        ctx.forget_trash();
        let companions: Vec<PathBuf> = ctx
            .companions()
            .existing(&src)
            .into_iter()
            .map(|f| f.path)
            .collect();
        trash::delete(&src).with_context(|| format!("trash {}", src.display()))?;
        // Each an item of the trash of its own, restored with the file.
        for companion in companions {
            let _ = trash::delete(&companion);
        }
        Ok(Some(Box::new(Restore {
            path: src,
            item: None,
        })))
    }
}

/// How far apart, in seconds, a file and its companions are trashed.
const COMPANION_TRASHED_WITHIN: i64 = 5;

/// Where the trash keeps what it says of the file at `path` (where it came
/// from, when), for a file of a trash's `files` folder: its
/// `info/<name>.trashinfo`, beside. A trash is `$XDG_DATA_HOME/Trash`, or
/// on another volume `.Trash/<uid>` or `.Trash-<uid>` at its top
/// (freedesktop.org's trash specification). Read from the path alone.
pub fn trash_info_of(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?;
    let files = path.parent()?;
    if files.file_name()? != "files" {
        return None;
    }
    let trash = files.parent()?;
    let named = trash.file_name()?.to_str()?;
    let is_trash = named == "Trash"
        || named
            .strip_prefix(".Trash-")
            .is_some_and(|uid| uid.parse::<u32>().is_ok())
        || (named.parse::<u32>().is_ok()
            && trash.parent().and_then(Path::file_name) == Some(".Trash".as_ref()));
    if !is_trash {
        return None;
    }
    let mut info = name.to_owned();
    info.push(".trashinfo");
    Some(trash.join("info").join(info))
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

/// The item a `.trashinfo` speaks of, in its trash's `files` folder.
fn item_of_info(info: &Path) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    let name = info.file_name()?.as_bytes().strip_suffix(b".trashinfo")?;
    Some(
        info.parent()?
            .parent()?
            .join("files")
            .join(OsStr::from_bytes(name)),
    )
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
    /// The item's `.trashinfo`, when one item is meant: undoing a trash
    /// leaves it out, and restores what was trashed last from `path`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<PathArg>,
}

impl Restore {
    /// Index of the item to restore in the plan's trash listing: the one
    /// named, else (it was restored and trashed again since, under
    /// another name) the most recent from the path.
    fn find(&self, items: &[trash::TrashItem]) -> Option<usize> {
        if let Some(item) = self.item.as_deref()
            && let Some(n) = items.iter().position(|i| Path::new(&i.id) == item)
        {
            return Some(n);
        }
        items
            .iter()
            .enumerate()
            .filter(|(_, i)| i.original_path() == self.path)
            .max_by_key(|(_, i)| i.time_deleted)
            .map(|(n, _)| n)
    }
}

impl Restore {
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
fn merge(from: &Path, into: &Path, rel: &Path, merged: &mut Merged) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name();
        let (dest, inner) = (into.join(&name), rel.join(&name));
        if exists(&dest) {
            anyhow::ensure!(
                entry.file_type()?.is_dir() && dest.is_dir(),
                "{} is there again: rename or move it first",
                dest.display()
            );
            merge(&entry.path(), &dest, &inner, merged)?;
        } else {
            rename_noreplace(&entry.path(), &dest)?;
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
        if exists(&self.path) {
            // A folder there again takes the trashed folder's files where
            // their places are free.
            if let Some(item) = self.merging() {
                return match clash(&item, &self.path) {
                    Ok(None) => pending(true),
                    Ok(Some(taken)) => conflict(format!(
                        "{} is there again: rename or move it first",
                        taken.display()
                    )),
                    Err(e) => conflict(format!("read {}: {e}", item.display())),
                };
            }
            // The item named still in the trash: another file took its
            // place since, which restoring it would not overwrite.
            if self.item.as_deref().is_some_and(Path::exists) {
                return conflict(format!(
                    "{} is there again: rename or move it first",
                    self.path.display()
                ));
            }
            return StepStatus::AlreadyDone;
        }
        match ctx.with_trash(|items| self.find(items)) {
            Ok(Some(_)) => pending(true),
            Ok(None) => conflict(format!("{} is not in the trash", self.path.display())),
            Err(e) => conflict(format!("list trash: {e}")),
        }
    }
    fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        if let (Some(item), Some(info)) = (self.merging(), self.item.as_deref()) {
            ctx.forget_trash();
            return Ok(Some(Box::new(merge_restore(&item, info, &self.path)?)));
        }
        // Taken out of the listing: once restored it is no longer there.
        let item = ctx
            .with_trash(|items| self.find(items).map(|n| items.swap_remove(n)))
            .context("list trash")?
            .ok_or_else(|| anyhow::anyhow!("{} is not in the trash", self.path.display()))?;
        let when = item.time_deleted;
        trash::os_limited::restore_all([item])
            .with_context(|| format!("restore {}", self.path.display()))?;
        // The companions trashed with it: those trashed from their places
        // as it was, then or just after.
        if let (Some(dir), Some(name)) = (self.path.parent(), self.path.file_name()) {
            let places: Vec<PathBuf> = ctx
                .companions()
                .of(name)
                .into_iter()
                .map(|p| dir.join(p))
                .filter(|p| !exists(p))
                .collect();
            let found = ctx
                .with_trash(|items| {
                    let mut found = Vec::new();
                    for place in &places {
                        let at = items
                            .iter()
                            .enumerate()
                            .filter(|(_, i)| {
                                i.original_path() == *place
                                    && (i.time_deleted - when).abs() <= COMPANION_TRASHED_WITHIN
                            })
                            .max_by_key(|(_, i)| i.time_deleted)
                            .map(|(n, _)| n);
                        if let Some(n) = at {
                            found.push(items.swap_remove(n));
                        }
                    }
                    found
                })
                .unwrap_or_default();
            if !found.is_empty() {
                let _ = trash::os_limited::restore_all(found);
            }
        }
        let r = EntryRef::from_path(&self.path)
            .ok_or_else(|| anyhow::anyhow!("restored file missing: {}", self.path.display()))?;
        Ok(Some(Box::new(Trash { entry: r })))
    }
}

/// Restore the trashed folder `item` (of which `info` speaks) into the
/// folder `path`, there again: merged, then the trashed folder's empty
/// folders and its `.trashinfo` go, as a restore takes them. Undone, the
/// files that moved go back into it (`Unmerge`); nothing else of the
/// folder there leaves it. Companions trashed beside the folder stay in
/// the trash: they are restored with it only into a free place.
fn merge_restore(item: &Path, info: &Path, path: &Path) -> anyhow::Result<Unmerge> {
    let mut merged = Merged::default();
    if let Err(e) = merge(item, path, Path::new(""), &mut merged) {
        // Back as it was: the trash keeps the folder whole.
        for inner in merged.moved.iter().rev() {
            let _ = rename_noreplace(&path.join(inner), &item.join(inner));
        }
        return Err(e.context(format!("restore {}", path.display())));
    }
    let info_text = std::fs::read_to_string(info).unwrap_or_default();
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
    /// The trashed folder's `.trashinfo`.
    pub info: PathArg,
    /// What it said, written again if it went.
    pub info_text: String,
    /// What moved, relative to both folders.
    pub moved: Vec<PathArg>,
    /// The trashed folder's folders that were left empty and went,
    /// relative to it, deepest first.
    pub folders: Vec<PathArg>,
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
            return conflict(format!("{} is no trash's", self.info.display()));
        };
        let back = |inner: &PathArg| exists(&item.join(inner));
        if self.info.exists() && self.moved.iter().all(back) {
            return StepStatus::AlreadyDone;
        }
        if let Some(gone) = self
            .moved
            .iter()
            .find(|inner| !back(inner) && !exists(&self.path.join(inner)))
        {
            return conflict(format!(
                "{} is no longer there",
                self.path.join(gone).display()
            ));
        }
        pending(true)
    }
    fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let item = item_of_info(&self.info)
            .ok_or_else(|| anyhow::anyhow!("{} is no trash's", self.info.display()))?;
        ctx.forget_trash();
        for inner in self.folders.iter().rev() {
            std::fs::create_dir_all(item.join(inner))
                .with_context(|| format!("mkdir {}", item.join(inner).display()))?;
        }
        for inner in &self.moved {
            let (now, back) = (self.path.join(inner), item.join(inner));
            if !exists(&back) {
                if let Some(dir) = back.parent() {
                    std::fs::create_dir_all(dir)?;
                }
                rename_noreplace(&now, &back)?;
            }
        }
        if !self.info.exists() {
            std::fs::write(&self.info, &self.info_text)
                .with_context(|| format!("write {}", self.info.display()))?;
        }
        Ok(Some(Box::new(Restore {
            path: self.path.clone(),
            item: Some(self.info.clone()),
        })))
    }
}

// ------------------------------------------------------------------ inside a trashed folder

/// Restore, alone, a file or folder from inside a trashed folder
/// (`Restore` takes back a whole item): the folders it was in are made
/// again where they are missing, as their counterparts in the trash are
/// (their mode, `user.*` attributes and times), and it moves back to where
/// it was, with its companions. The trashed folder stays in the trash with
/// the rest, under what the trash says of it; restored later, it merges
/// into the folders made again.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RestorePart {
    /// It, inside the trashed folder.
    pub entry: EntryRef,
    /// Where it was: where the trashed folder was, then its path in it.
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
        let _ = xattr::set(dir, name, &value);
    }
    if let (Ok(file), Ok(accessed), Ok(modified)) =
        (std::fs::File::open(dir), meta.accessed(), meta.modified())
    {
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
                    "{} is no longer in the trash",
                    self.entry.path.display()
                ));
            }
            Err(e) => return conflict(e.to_string()),
        };
        if in_trash(&src).is_none_or(|t| t.inner.as_os_str().is_empty()) {
            return conflict(format!(
                "{} is not inside a folder of the trash",
                src.display()
            ));
        }
        if exists(&self.path) {
            return conflict(format!(
                "{} is there again: rename or move it first",
                self.path.display()
            ));
        }
        if let Some(taken) = along_conflict(&along(ctx, &src, &self.path)) {
            return taken;
        }
        // The first folder above that is there must be one.
        if let Some(there) = self.path.ancestors().skip(1).find(|dir| exists(dir))
            && !there.is_dir()
        {
            return conflict(format!("{} is not a folder", there.display()));
        }
        pending(true)
    }
    fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let src = self.entry.resolve()?;
        let inside = in_trash(&src)
            .filter(|t| !t.inner.as_os_str().is_empty())
            .ok_or_else(|| {
                anyhow::anyhow!("{} is not inside a folder of the trash", src.display())
            })?;
        let folders = self.folders(&src, inside.inner.components().count());
        let undo_folders = |made: &[(PathBuf, Option<PathBuf>)]| {
            for (dir, _) in made.iter().rev() {
                let _ = std::fs::remove_dir(dir);
            }
        };
        for (n, (dir, _)) in folders.iter().enumerate() {
            if let Err(e) = std::fs::create_dir(dir) {
                undo_folders(&folders[..n]);
                return Err(e).with_context(|| format!("mkdir {}", dir.display()));
            }
        }
        let companions = along(ctx, &src, &self.path);
        if let Err(e) = rename_noreplace(&src, &self.path) {
            undo_folders(&folders);
            return Err(e.context(format!("restore {}", self.path.display())));
        }
        if let Err(e) = rename_along(&companions) {
            let _ = rename_noreplace(&self.path, &src);
            undo_folders(&folders);
            return Err(e);
        }
        ctx.forget_trash();
        for (dir, like) in folders.iter().rev() {
            if let Some(like) = like {
                made_like(dir, like);
            }
        }
        Ok(Some(Box::new(ReturnPart {
            path: self.path.clone(),
            to: src,
            made: folders.into_iter().map(|(dir, _)| PathArg(dir)).collect(),
        })))
    }
}

/// Put back in its trashed folder what `RestorePart` took out, with its
/// companions: the folders the restore made go while empty.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ReturnPart {
    /// Where it is, restored.
    #[serde(with = "crate::os_path")]
    #[schemars(with = "PathArg")]
    pub path: PathBuf,
    /// Its place in the trashed folder.
    #[serde(with = "crate::os_path")]
    #[schemars(with = "PathArg")]
    pub to: PathBuf,
    /// The folders the restore made, outermost first.
    pub made: Vec<PathArg>,
}

impl super::Kind for ReturnPart {
    const KIND: &'static str = "return_part";
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
                conflict(format!("{} is no longer there", self.path.display()))
            };
        }
        if exists(&self.to) {
            return conflict(format!(
                "{} is in its place in the trash",
                self.to.display()
            ));
        }
        if !self.to.parent().is_some_and(Path::is_dir) {
            return conflict(format!(
                "the folder of the trash {} was in is gone",
                self.path.display()
            ));
        }
        along_conflict(&along(ctx, &self.path, &self.to)).unwrap_or(pending(true))
    }
    fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let companions = along(ctx, &self.path, &self.to);
        rename_noreplace(&self.path, &self.to)?;
        if let Err(e) = rename_along(&companions) {
            let _ = rename_noreplace(&self.to, &self.path);
            return Err(e);
        }
        ctx.forget_trash();
        for dir in self.made.iter().rev() {
            // Only while empty: what was put there since stays.
            let _ = std::fs::remove_dir(dir);
        }
        let entry = EntryRef::from_path(&self.to)
            .ok_or_else(|| anyhow::anyhow!("{} is not in the trash", self.to.display()))?;
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
            Ok(_) => pending(false),
            Err(ResolveError::Gone(_)) => StepStatus::AlreadyDone,
            Err(e) => conflict(e.to_string()),
        }
    }
    fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let src = self.entry.resolve()?;
        let companions = ctx.companions().existing(&src);
        remove_recursive(&src)?;
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

/// A file of a trash is moved or renamed only by being restored: under
/// another name, or elsewhere, its `.trashinfo` would speak of nothing and
/// the trash would hold a file it says nothing of.
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
        return Some(conflict(format!("{} contains “..”", path.display())));
    }
    match path.file_name().map(invalid_name) {
        None => Some(conflict(format!("{} has no name", path.display()))),
        Some(invalid) => invalid,
    }
}

impl super::Effect for Mkdir {
    fn describe(&self) -> Change {
        Change::Create {
            path: self.path.clone(),
            directory: true,
        }
    }
    fn check(&self, _ctx: &StepContext) -> StepStatus {
        if let Some(refused) = refuse_new(&self.path) {
            return refused;
        }
        match std::fs::symlink_metadata(&self.path) {
            Ok(m) if m.is_dir() => StepStatus::AlreadyDone,
            Ok(_) => conflict(format!(
                "{} exists and is not a directory",
                self.path.display()
            )),
            Err(_) if self.path.parent().is_some_and(Path::is_dir) => pending(true),
            Err(_) => conflict("parent directory does not exist"),
        }
    }
    fn apply(&self, _ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        std::fs::create_dir(&self.path)
            .with_context(|| format!("mkdir {}", self.path.display()))?;
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
    fn check(&self, _ctx: &StepContext) -> StepStatus {
        match std::fs::read_dir(&self.path) {
            Ok(mut rd) => {
                if rd.next().is_none() {
                    pending(true)
                } else {
                    conflict(format!("{} is not empty", self.path.display()))
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => StepStatus::AlreadyDone,
            Err(e) => conflict(e.to_string()),
        }
    }
    fn apply(&self, _ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        std::fs::remove_dir(&self.path)
            .with_context(|| format!("rmdir {}", self.path.display()))?;
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
        }
    }
    fn check(&self, _ctx: &StepContext) -> StepStatus {
        if let Some(refused) = refuse_new(&self.path) {
            return refused;
        }
        match std::fs::symlink_metadata(&self.path) {
            // What this step makes: applied already.
            Ok(m) if m.is_file() && m.len() == 0 => StepStatus::AlreadyDone,
            Ok(_) => conflict(format!("{} already exists", self.path.display())),
            Err(_) if self.path.parent().is_some_and(Path::is_dir) => pending(true),
            Err(_) => conflict("parent directory does not exist"),
        }
    }
    fn apply(&self, _ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&self.path)
            .with_context(|| format!("create {}", self.path.display()))?;
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
            Ok(m) if m.is_file() => conflict(format!("{} is no longer empty", self.path.display())),
            Ok(_) => conflict(format!("{} is not a file", self.path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => StepStatus::AlreadyDone,
            Err(e) => conflict(e.to_string()),
        }
    }
    fn apply(&self, _ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        std::fs::remove_file(&self.path)
            .with_context(|| format!("remove {}", self.path.display()))?;
        Ok(Some(Box::new(NewFile {
            path: self.path.clone(),
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
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

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
    /// in a trash is not put there again.
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
        let moved = Move {
            entry: entry.clone(),
            dest_dir: tmp.path().to_path_buf(),
        };
        assert!(matches!(moved.check(&ctx), StepStatus::Conflict { .. }));
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
                crate::EntryId { dev: 1, ino: 2 },
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

    #[test]
    fn a_chunk_written_through_takes_about_a_step() {
        // 1 MiB in a second: a quarter of it next.
        assert_eq!(next_chunk(1 << 20, Duration::from_secs(1)), 256 << 10);
        // A very slow drive still writes the least chunk at a time.
        assert_eq!(next_chunk(1 << 20, Duration::from_secs(60)), THROUGH_MIN);
        // A fast one, no more than an unsynced copy would.
        assert_eq!(next_chunk(1 << 20, Duration::ZERO), COPY_CHUNK as u64);
    }

    /// The wait written through syncs a whole filesystem: the tests' own
    /// would make every test writing to it wait. procfs has nothing to
    /// write; this checks the bytes and what is counted, not the drive.
    const NOTHING_TO_WRITE: &str = "/proc";

    #[test]
    fn a_copy_written_through_is_whole() {
        let base = dirs::home_dir().unwrap().join(".cache/bibli-tests");
        std::fs::create_dir_all(&base).unwrap();
        let dir = tempfile::tempdir_in(base).unwrap();
        let (src, dst) = (dir.path().join("firmware.bin"), dir.path().join("copy.bin"));
        // Several chunks, and an end that is not a whole one.
        let bytes: Vec<u8> = (0..(3 << 20) + 12_345u32)
            .map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8)
            .collect();
        std::fs::write(&src, &bytes).unwrap();
        let ctx = StepContext::detached();
        for through in [true, false] {
            let pace = if through {
                Pace::through(Path::new(NOTHING_TO_WRITE)).unwrap()
            } else {
                Pace::at_once()
            };
            let _ = std::fs::remove_file(&dst);
            copy_file(&src, &dst, false, &pace, &ctx).unwrap();
            pace.settle(&ctx).unwrap();
            assert_eq!(std::fs::read(&dst).unwrap(), bytes, "through: {through}");
            assert_eq!(pace.unsaid.get(), 0, "everything counted");
        }
        std::fs::write(&src, b"").unwrap();
        std::fs::remove_file(&dst).unwrap();
        let pace = Pace::through(Path::new(NOTHING_TO_WRITE)).unwrap();
        copy_file(&src, &dst, false, &pace, &ctx).unwrap();
        pace.settle(&ctx).unwrap();
        assert!(std::fs::read(&dst).unwrap().is_empty());
    }
}
