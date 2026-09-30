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
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        from,
        rustix::fs::CWD,
        to,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .with_context(|| format!("rename {} -> {}", from.display(), to.display()))
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
    fn check(&self, _ctx: &StepContext) -> StepStatus {
        if let Some(invalid) = invalid_name(&self.new_name) {
            return invalid;
        }
        let target = self.target();
        match self.entry.resolve() {
            Ok(src) if src == target => StepStatus::AlreadyDone,
            Ok(src) => match self.classify(&src, &target) {
                RenameTarget::Free | RenameTarget::Respelled => pending(true),
                RenameTarget::Spelled => StepStatus::AlreadyDone,
                RenameTarget::Taken => conflict(format!("{} already exists", target.display())),
            },
            Err(_) if id_of(&target) == Some(self.entry.id) => StepStatus::AlreadyDone,
            Err(e) => conflict(e.to_string()),
        }
    }
    fn apply(&self, _ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        valid_file_name(&self.new_name)?;
        let src = self.entry.resolve()?;
        let target = self.target();
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
    fn check(&self, _ctx: &StepContext) -> StepStatus {
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
            Ok(_) => pending(true),
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
        match rename_noreplace(&src, &target) {
            Ok(()) => {}
            Err(e) if is_cross_device(&e) => {
                // A move must never lose metadata, whatever the copy preference
                // says, and a failed transfer must not leave a partial target.
                copy_staged(&src, &target, &self.dest_dir, true, ctx)?;
                remove_recursive(&src)?;
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
        pending(true)
    }
    fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        if let Some(name) = &self.new_name {
            valid_file_name(name)?;
        }
        let src = self.entry.resolve()?;
        let target = self.target(&src);
        copy_staged(&src, &target, &self.dest_dir, self.preserve_attributes, ctx)?;
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
    let names = |dir: &Path| -> std::io::Result<Vec<OsString>> {
        let mut names = std::fs::read_dir(dir)?
            .map(|e| e.map(|e| e.file_name()))
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

/// Copy `src` to `target` through a private staging directory inside
/// `dest_dir`, publishing with a no-replace rename only after content and
/// attributes are complete. A failed attribute write must not leave a partial destination
/// that `check()` would call already done. Shared by Copy and cross-device Move.
fn copy_staged(
    src: &Path,
    target: &Path,
    dest_dir: &Path,
    preserve_attributes: bool,
    ctx: &StepContext,
) -> anyhow::Result<()> {
    ctx.add_total(tree_size(src, ctx)?);
    let staging = tempfile::Builder::new()
        .prefix(".bibli-copy-")
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir_in(dest_dir)?;
    let staged = staging.path().join("entry");
    let result = copy_recursive_with_attributes(src, &staged, preserve_attributes, ctx)
        .and_then(|()| rename_noreplace(&staged, target));
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

fn copy_file(
    src: &Path,
    dst: &Path,
    preserve_attributes: bool,
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
        copy_contents(&mut sf, &mut df, ctx)
            .with_context(|| format!("copy {} -> {}", src.display(), dst.display()))?;
    }
    // Set attributes before restoring a read-only source's permissions.
    if preserve_attributes {
        copy_attributes(src, dst)?;
    }
    df.set_permissions(m.permissions())?;
    let _ = df.set_modified(m.modified()?);
    Ok(())
}

/// Copy the bytes in chunks, checking for Cancel and reporting progress
/// between them: in the kernel where it can (`copy_file_range`), through a
/// buffer where it cannot.
fn copy_contents(
    sf: &mut std::fs::File,
    df: &mut std::fs::File,
    ctx: &StepContext,
) -> anyhow::Result<()> {
    let mut copied = 0u64;
    loop {
        ctx.check_cancelled()?;
        match rustix::fs::copy_file_range(&*sf, None, &*df, None, COPY_CHUNK) {
            Ok(0) if copied > 0 => return Ok(()),
            Ok(n @ 1..) => {
                copied += n as u64;
                ctx.advance(n as u64);
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
        ctx.advance(n as u64);
    }
}

fn copy_recursive_with_attributes(
    src: &Path,
    dst: &Path,
    preserve_attributes: bool,
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
            copy_recursive_with_attributes(
                &e.path(),
                &dst.join(e.file_name()),
                preserve_attributes,
                ctx,
            )?;
        }
        if preserve_attributes {
            copy_attributes(src, dst)?;
        }
        std::fs::set_permissions(dst, m.permissions())?;
    } else if kind.is_file() {
        copy_file(src, dst, preserve_attributes, ctx)?;
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

/// Copy the user's extended attributes (`user.*`: tags, rating, comment,
/// where a download came from), as `cp --preserve=xattr` does. The other
/// namespaces are the system's (ACLs, security labels, capabilities) and
/// stay as the destination makes them.
fn copy_attributes(src: &Path, dst: &Path) -> anyhow::Result<()> {
    let names = match xattr::list(src) {
        Ok(names) => names,
        // A source filesystem without user attributes has none to keep.
        Err(error) if error.kind() == std::io::ErrorKind::Unsupported => return Ok(()),
        Err(error) => return Err(error).context("read source attributes while copying"),
    };
    for name in names.filter(|n| n.as_encoded_bytes().starts_with(b"user.")) {
        // One removed since it was listed is not there to keep.
        let Some(value) =
            xattr::get(src, &name).context("read a source attribute while copying")?
        else {
            continue;
        };
        xattr::set(dst, &name, &value).context(
            "keep tags and other attributes while copying; check destination xattr support and permissions, or turn off “Keep tags and attributes when copying” in Settings",
        )?;
    }
    Ok(())
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
    fn apply(&self, _ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let (pa, pb) = (self.a.resolve()?, self.b.resolve()?);
        rustix::fs::renameat_with(
            rustix::fs::CWD,
            &pa,
            rustix::fs::CWD,
            &pb,
            rustix::fs::RenameFlags::EXCHANGE,
        )
        .with_context(|| format!("swap {} <-> {}", pa.display(), pb.display()))?;
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
        trash::delete(&src).with_context(|| format!("trash {}", src.display()))?;
        Ok(Some(Box::new(Restore { path: src })))
    }
}

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
        trash::os_limited::restore_all([item])
            .with_context(|| format!("restore {}", self.path.display()))?;
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
    fn apply(&self, _ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let src = self.entry.resolve()?;
        remove_recursive(&src)?;
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
