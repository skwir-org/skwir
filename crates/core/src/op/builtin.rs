//! The built-in operations: rename, move, copy, swap, trash, restore, delete,
//! mkdir, rmdir. Each is idempotent (`check` detects an effect already in
//! place) and returns its inverse from `apply`.

use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use super::{Change, EntryRef, OpRegistry, Operation, ResolveError, StepStatus};
use crate::fs::stat::id_of;

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

fn file_name_string(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
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
    pub new_name: String,
}

impl Rename {
    fn target(&self) -> PathBuf {
        self.entry
            .path
            .parent()
            .unwrap_or(Path::new("/"))
            .join(&self.new_name)
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
    fn check(&self) -> StepStatus {
        let target = self.target();
        if self.new_name.is_empty() || self.new_name.contains('/') {
            return conflict("invalid name");
        }
        match self.entry.resolve() {
            Ok(src) if src == target => StepStatus::AlreadyDone,
            Ok(_) if exists(&target) => conflict(format!("{} already exists", target.display())),
            Ok(_) => pending(true),
            Err(_) if id_of(&target) == Some(self.entry.id) => StepStatus::AlreadyDone,
            Err(e) => conflict(e.to_string()),
        }
    }
    fn apply(&self) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let src = self.entry.resolve()?;
        let target = self.target();
        rename_noreplace(&src, &target)?;
        Ok(Some(Box::new(Rename {
            entry: EntryRef::new(self.entry.id, target),
            new_name: file_name_string(&src),
        })))
    }
}

// ------------------------------------------------------------------ move

/// Move into another directory, keeping the name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Move {
    pub entry: EntryRef,
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
    fn check(&self) -> StepStatus {
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
            Err(e) => conflict(e.to_string()),
        }
    }
    fn apply(&self) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let src = self.entry.resolve()?;
        let target = self.target(&src);
        let origin = src.parent().unwrap_or(Path::new("/")).to_path_buf();
        match rename_noreplace(&src, &target) {
            Ok(()) => {}
            Err(e) if is_cross_device(&e) => {
                copy_recursive(&src, &target)?;
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
    pub dest_dir: PathBuf,
    /// Name at the destination; the source name when `None`.
    #[serde(default)]
    pub new_name: Option<String>,
}

impl Copy {
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
    fn check(&self) -> StepStatus {
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
        if let (Ok(a), Ok(b)) = (
            std::fs::symlink_metadata(&src),
            std::fs::symlink_metadata(&target),
        ) {
            let same = a.is_dir() == b.is_dir()
                && (a.is_dir() || (a.len() == b.len() && a.modified().ok() == b.modified().ok()));
            return if same {
                StepStatus::AlreadyDone
            } else {
                conflict(format!("{} already exists", target.display()))
            };
        }
        if src.is_dir() && target.starts_with(&src) {
            return conflict("cannot copy a directory into itself");
        }
        pending(true)
    }
    fn apply(&self) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let src = self.entry.resolve()?;
        let target = self.target(&src);
        copy_recursive(&src, &target)?;
        let r = EntryRef::from_path(&target)
            .ok_or_else(|| anyhow::anyhow!("copy vanished: {}", target.display()))?;
        Ok(Some(Box::new(Delete { entry: r })))
    }
}

fn copy_file(src: &Path, dst: &Path) -> anyhow::Result<()> {
    let sf = std::fs::File::open(src)?;
    let df = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dst)?;
    if rustix::fs::ioctl_ficlone(&df, &sf).is_ok() {
        let m = sf.metadata()?;
        df.set_permissions(m.permissions())?;
        let _ = df.set_modified(m.modified()?);
        return Ok(());
    }
    drop(df);
    std::fs::copy(src, dst)
        .with_context(|| format!("copy {} -> {}", src.display(), dst.display()))?;
    if let Ok(m) = sf.metadata()
        && let Ok(t) = m.modified()
    {
        let _ = std::fs::File::options()
            .write(true)
            .open(dst)
            .and_then(|f| f.set_modified(t));
    }
    Ok(())
}

fn copy_recursive(src: &Path, dst: &Path) -> anyhow::Result<()> {
    let m = std::fs::symlink_metadata(src)?;
    if m.is_symlink() {
        let target = std::fs::read_link(src)?;
        std::os::unix::fs::symlink(target, dst)?;
    } else if m.is_dir() {
        std::fs::create_dir(dst)?;
        for e in std::fs::read_dir(src)? {
            let e = e?;
            copy_recursive(&e.path(), &dst.join(e.file_name()))?;
        }
        std::fs::set_permissions(dst, m.permissions())?;
    } else {
        copy_file(src, dst)?;
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
    fn check(&self) -> StepStatus {
        match (self.a.resolve(), self.b.resolve()) {
            (Ok(_), Ok(_)) => pending(true),
            (Err(e), _) | (_, Err(e)) => conflict(e.to_string()),
        }
    }
    fn apply(&self) -> anyhow::Result<Option<Box<dyn Operation>>> {
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
    fn check(&self) -> StepStatus {
        match self.entry.resolve() {
            Ok(_) => pending(true),
            Err(ResolveError::Gone(_)) => StepStatus::AlreadyDone,
            Err(e) => conflict(e.to_string()),
        }
    }
    fn apply(&self) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let src = self.entry.resolve()?;
        trash::delete(&src).with_context(|| format!("trash {}", src.display()))?;
        Ok(Some(Box::new(Restore { path: src })))
    }
}

/// Restore the most recently trashed item that had this original path.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Restore {
    pub path: PathBuf,
}

impl Restore {
    fn find(&self) -> anyhow::Result<Option<trash::TrashItem>> {
        let items = trash::os_limited::list().context("list trash")?;
        Ok(items
            .into_iter()
            .filter(|i| i.original_path() == self.path)
            .max_by_key(|i| i.time_deleted))
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
    fn check(&self) -> StepStatus {
        if exists(&self.path) {
            return StepStatus::AlreadyDone;
        }
        match self.find() {
            Ok(Some(_)) => pending(true),
            Ok(None) => conflict(format!("{} is not in the trash", self.path.display())),
            Err(e) => conflict(e.to_string()),
        }
    }
    fn apply(&self) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let item = self
            .find()?
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
    fn check(&self) -> StepStatus {
        match self.entry.resolve() {
            Ok(_) => pending(false),
            Err(ResolveError::Gone(_)) => StepStatus::AlreadyDone,
            Err(e) => conflict(e.to_string()),
        }
    }
    fn apply(&self) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let src = self.entry.resolve()?;
        remove_recursive(&src)?;
        Ok(None)
    }
}

// ------------------------------------------------------------------ mkdir / rmdir

/// Create a directory (parent must exist).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Mkdir {
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
    fn check(&self) -> StepStatus {
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
    fn apply(&self) -> anyhow::Result<Option<Box<dyn Operation>>> {
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
    fn check(&self) -> StepStatus {
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
    fn apply(&self) -> anyhow::Result<Option<Box<dyn Operation>>> {
        std::fs::remove_dir(&self.path)
            .with_context(|| format!("rmdir {}", self.path.display()))?;
        Ok(Some(Box::new(Mkdir {
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
}
