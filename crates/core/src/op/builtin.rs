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

impl Operation for Rename {
    fn kind(&self) -> &'static str {
        "rename"
    }
    fn clone_box(&self) -> Box<dyn Operation> {
        Box::new(self.clone())
    }
    fn args(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("serializable")
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

impl Operation for Move {
    fn kind(&self) -> &'static str {
        "move"
    }
    fn clone_box(&self) -> Box<dyn Operation> {
        Box::new(self.clone())
    }
    fn args(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("serializable")
    }
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

impl Operation for Copy {
    fn kind(&self) -> &'static str {
        "copy"
    }
    fn clone_box(&self) -> Box<dyn Operation> {
        Box::new(self.clone())
    }
    fn args(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("serializable")
    }
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
    let mut sf = {
        use std::os::unix::fs::OpenOptionsExt;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(rustix::fs::OFlags::NONBLOCK.bits().cast_signed())
            .open(src)?;
        anyhow::ensure!(
            file.metadata()?.is_file(),
            "{} is no longer a regular file",
            src.display()
        );
        file
    };
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

impl Operation for Swap {
    fn kind(&self) -> &'static str {
        "swap"
    }
    fn clone_box(&self) -> Box<dyn Operation> {
        Box::new(self.clone())
    }
    fn args(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("serializable")
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

impl Operation for Trash {
    fn kind(&self) -> &'static str {
        "trash"
    }
    fn clone_box(&self) -> Box<dyn Operation> {
        Box::new(self.clone())
    }
    fn args(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("serializable")
    }
    fn describe(&self) -> Change {
        Change::Trash {
            path: self.entry.path.clone(),
        }
    }
    fn check(&self, _ctx: &StepContext) -> StepStatus {
        match self.entry.resolve() {
            Ok(_) => pending(true),
            Err(ResolveError::Gone(_)) => StepStatus::AlreadyDone,
            Err(e) => conflict(e.to_string()),
        }
    }
    fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let src = self.entry.resolve()?;
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
        Ok(Some(Box::new(Restore { path: src })))
    }
}

/// How far apart, in seconds, a file and its companions are trashed.
const COMPANION_TRASHED_WITHIN: i64 = 5;

/// Restore the most recently trashed item that had this original path.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Restore {
    #[serde(with = "crate::os_path")]
    #[schemars(with = "PathArg")]
    pub path: PathBuf,
}

impl Restore {
    /// Index of the item to restore in the plan's trash listing.
    fn find(&self, items: &[trash::TrashItem]) -> Option<usize> {
        items
            .iter()
            .enumerate()
            .filter(|(_, i)| i.original_path() == self.path)
            .max_by_key(|(_, i)| i.time_deleted)
            .map(|(n, _)| n)
    }
}

impl Operation for Restore {
    fn kind(&self) -> &'static str {
        "restore"
    }
    fn clone_box(&self) -> Box<dyn Operation> {
        Box::new(self.clone())
    }
    fn args(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("serializable")
    }
    fn describe(&self) -> Change {
        Change::Restore {
            path: self.path.clone(),
        }
    }
    fn check(&self, ctx: &StepContext) -> StepStatus {
        if exists(&self.path) {
            return StepStatus::AlreadyDone;
        }
        match ctx.with_trash(|items| self.find(items)) {
            Ok(Some(_)) => pending(true),
            Ok(None) => conflict(format!("{} is not in the trash", self.path.display())),
            Err(e) => conflict(format!("list trash: {e}")),
        }
    }
    fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
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

// ------------------------------------------------------------------ delete

/// Permanent deletion. Irreversible.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Delete {
    pub entry: EntryRef,
}

impl Operation for Delete {
    fn kind(&self) -> &'static str {
        "delete"
    }
    fn clone_box(&self) -> Box<dyn Operation> {
        Box::new(self.clone())
    }
    fn args(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("serializable")
    }
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
        for companion in companions {
            let _ = remove_recursive(&companion.path);
        }
        Ok(None)
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

impl Operation for Mkdir {
    fn kind(&self) -> &'static str {
        "mkdir"
    }
    fn clone_box(&self) -> Box<dyn Operation> {
        Box::new(self.clone())
    }
    fn args(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("serializable")
    }
    fn describe(&self) -> Change {
        Change::Create {
            path: self.path.clone(),
            directory: true,
        }
    }
    fn check(&self, _ctx: &StepContext) -> StepStatus {
        if self.path.components().any(|c| c == Component::ParentDir) {
            return conflict(format!("{} contains “..”", self.path.display()));
        }
        match self.path.file_name().map(invalid_name) {
            None => return conflict(format!("{} has no name", self.path.display())),
            Some(Some(invalid)) => return invalid,
            Some(None) => {}
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

impl Operation for Rmdir {
    fn kind(&self) -> &'static str {
        "rmdir"
    }
    fn clone_box(&self) -> Box<dyn Operation> {
        Box::new(self.clone())
    }
    fn args(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("serializable")
    }
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

impl Operation for NewFile {
    fn kind(&self) -> &'static str {
        "new_file"
    }
    fn clone_box(&self) -> Box<dyn Operation> {
        Box::new(self.clone())
    }
    fn args(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("serializable")
    }
    fn describe(&self) -> Change {
        Change::Create {
            path: self.path.clone(),
            directory: false,
        }
    }
    fn check(&self, _ctx: &StepContext) -> StepStatus {
        if self.path.components().any(|c| c == Component::ParentDir) {
            return conflict(format!("{} contains “..”", self.path.display()));
        }
        match self.path.file_name().map(invalid_name) {
            None => return conflict(format!("{} has no name", self.path.display())),
            Some(Some(invalid)) => return invalid,
            Some(None) => {}
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

impl Operation for RemoveEmptyFile {
    fn kind(&self) -> &'static str {
        "remove_empty_file"
    }
    fn clone_box(&self) -> Box<dyn Operation> {
        Box::new(self.clone())
    }
    fn args(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("serializable")
    }
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
    reg.register::<Rename>("core", "rename");
    reg.register::<Move>("core", "move");
    reg.register::<Copy>("core", "copy");
    reg.register::<Swap>("core", "swap");
    reg.register::<Trash>("core", "trash");
    reg.register::<Restore>("core", "restore");
    reg.register::<Delete>("core", "delete");
    reg.register::<Mkdir>("core", "mkdir");
    reg.register::<Rmdir>("core", "rmdir");
    reg.register::<NewFile>("core", "new_file");
    reg.register::<RemoveEmptyFile>("core", "remove_empty_file");
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

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
