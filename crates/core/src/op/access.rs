//! Who may read, write and run a file: its mode and its group, changed as
//! plan steps (ADR-058). `set_access` takes the mode as `chmod` does
//! (`644`, `u+x`, `go-w`, `a=rX`), the group by number, and with `inside`
//! everything a folder holds; its inverse (`access_back`) puts back each
//! item's mode and group as they were.
//!
//! What a link leads to is changed, as `chmod` changes it; inside a folder
//! a link is left alone, and nothing on another drive mounted inside is
//! reached. A drive whose filesystem keeps no permissions (FAT) takes the
//! call and keeps nothing: the step reads the mode back and says so,
//! rather than guessing from the filesystem's name. The owner is never
//! changed: giving a file away needs root, which skwir never asks for.
//!
//! On Windows a mode is the read-only attribute: the owner's write bit
//! (`u-w` sets it, `u+w` clears it); a group is not yet.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{Change, EntryRef, Operation, ResolveError, StepContext, StepStatus, Words};

// ------------------------------------------------------------------ modes

/// A mode as `chmod` takes it: octal digits, or symbolic clauses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mode {
    /// `644`, `2775`: the bits themselves. `leading_zero` is a fifth
    /// digit (`00755`): only then are a folder's set-ID bits cleared, as
    /// GNU chmod keeps them otherwise.
    Octal { bits: u32, leading_zero: bool },
    /// `u+x,go-w`, applied clause by clause.
    Symbolic(Vec<Clause>),
}

/// One symbolic clause: whom it is about, then what it does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Clause {
    /// `u`, `g`, `o` as bits 4, 2, 1; none said is everyone (the umask
    /// aside, which skwir does not consult).
    who: u8,
    actions: Vec<(char, Perms)>,
}

/// What an action gives or takes.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Perms {
    /// `r`, `w`, `x`, `X` (search for folders and what someone may run
    /// already), `s` (set-ID), `t` (sticky), any of them.
    Letters(String),
    /// What another class has: `g=u`.
    Copy(u8),
}

const USER: u8 = 4;
const GROUP: u8 = 2;
const OTHERS: u8 = 1;
const SETUID: u32 = 0o4000;
const SETGID: u32 = 0o2000;
const STICKY: u32 = 0o1000;

impl Mode {
    /// Read a mode as `chmod` writes it.
    ///
    /// # Errors
    /// What is not a mode, in the user's words.
    pub fn parse(text: &str) -> anyhow::Result<Mode> {
        let text = text.trim();
        let wrong = || {
            anyhow::anyhow!("“{text}” is not a mode: write it as chmod does, as 644, u+x or go-w")
        };
        if text.is_empty() {
            return Err(wrong());
        }
        if text.bytes().all(|b| b.is_ascii_digit()) {
            if text.len() > 5 || text.bytes().any(|b| b > b'7') {
                return Err(wrong());
            }
            let bits = u32::from_str_radix(text, 8).map_err(|_| wrong())?;
            if bits > 0o7777 {
                return Err(wrong());
            }
            return Ok(Mode::Octal {
                bits,
                leading_zero: text.len() == 5,
            });
        }
        let mut clauses = Vec::new();
        for part in text.split(',') {
            let mut chars = part.chars().peekable();
            let mut who = 0;
            while let Some(&c) = chars.peek() {
                who |= match c {
                    'u' => USER,
                    'g' => GROUP,
                    'o' => OTHERS,
                    'a' => USER | GROUP | OTHERS,
                    _ => break,
                };
                chars.next();
            }
            let mut actions = Vec::new();
            while let Some(op) = chars.next() {
                if !matches!(op, '+' | '-' | '=') {
                    return Err(wrong());
                }
                let mut letters = String::new();
                let mut copy = None;
                while let Some(&c) = chars.peek() {
                    match c {
                        'r' | 'w' | 'x' | 'X' | 's' | 't' if copy.is_none() => letters.push(c),
                        'u' | 'g' | 'o' if letters.is_empty() && copy.is_none() => {
                            copy = Some(match c {
                                'u' => USER,
                                'g' => GROUP,
                                _ => OTHERS,
                            });
                        }
                        '+' | '-' | '=' => break,
                        _ => return Err(wrong()),
                    }
                    chars.next();
                }
                actions.push((op, copy.map_or(Perms::Letters(letters), Perms::Copy)));
            }
            if actions.is_empty() {
                return Err(wrong());
            }
            clauses.push(Clause { who, actions });
        }
        Ok(Mode::Symbolic(clauses))
    }

    /// The permission bits (`0o7777`) `mode` becomes, for a folder when
    /// `folder`. The file-type bits are kept.
    pub fn apply(&self, mode: u32, folder: bool) -> u32 {
        let kind = mode & !0o7777;
        let bits = match self {
            Mode::Octal { bits, leading_zero } => {
                // GNU chmod: a folder keeps its set-ID bits unless a fifth
                // digit says otherwise.
                if folder && !leading_zero {
                    bits | (mode & (SETUID | SETGID))
                } else {
                    *bits
                }
            }
            Mode::Symbolic(clauses) => clauses
                .iter()
                .fold(mode & 0o7777, |bits, clause| clause.apply(bits, folder)),
        };
        kind | bits
    }
}

impl Clause {
    fn apply(&self, mut bits: u32, folder: bool) -> u32 {
        let who = if self.who == 0 {
            USER | GROUP | OTHERS
        } else {
            self.who
        };
        // The rwx bits of the classes this clause is about.
        let classes: u32 = [(USER, 0o700), (GROUP, 0o070), (OTHERS, 0o007)]
            .iter()
            .filter(|(class, _)| who & class != 0)
            .map(|(_, mask)| mask)
            .sum();
        for (op, perms) in &self.actions {
            let mut given = 0;
            match perms {
                Perms::Copy(from) => {
                    let shift = match *from {
                        USER => 6,
                        GROUP => 3,
                        _ => 0,
                    };
                    let rwx = (bits >> shift) & 0o7;
                    given = (rwx << 6 | rwx << 3 | rwx) & classes;
                }
                Perms::Letters(letters) => {
                    let runs = bits & 0o111 != 0;
                    for letter in letters.chars() {
                        given |= match letter {
                            'r' => 0o444 & classes,
                            'w' => 0o222 & classes,
                            'x' => 0o111 & classes,
                            'X' if folder || runs => 0o111 & classes,
                            's' => {
                                (if who & USER != 0 { SETUID } else { 0 })
                                    | (if who & GROUP != 0 { SETGID } else { 0 })
                            }
                            't' if who & OTHERS != 0 => STICKY,
                            _ => 0,
                        };
                    }
                }
            }
            match op {
                '+' => bits |= given,
                '-' => bits &= !given,
                _ => {
                    // `=`: the classes' rwx as given; their special bits
                    // too, but a folder's set-ID bits only when said.
                    let mut cleared = classes;
                    if !folder || matches!(perms, Perms::Letters(l) if l.contains('s')) {
                        cleared |= if who & USER != 0 { SETUID } else { 0 };
                        cleared |= if who & GROUP != 0 { SETGID } else { 0 };
                    }
                    if who & OTHERS != 0 {
                        cleared |= STICKY;
                    }
                    bits = (bits & !cleared) | given;
                }
            }
        }
        bits
    }
}

// ------------------------------------------------------------------ the system

/// What a stat says that a change of access needs.
#[derive(Clone, Copy, Debug)]
struct Stat {
    mode: u32,
    uid: Option<u32>,
    gid: Option<u32>,
    folder: bool,
    link: bool,
    device: Option<u64>,
}

/// The stat of `path`, through a link when `follow`.
fn stat(path: &Path, follow: bool) -> std::io::Result<Stat> {
    let meta = if follow {
        std::fs::metadata(path)?
    } else {
        std::fs::symlink_metadata(path)?
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(Stat {
            mode: meta.mode(),
            uid: Some(meta.uid()),
            gid: Some(meta.gid()),
            folder: meta.is_dir(),
            link: meta.file_type().is_symlink(),
            device: Some(meta.dev()),
        })
    }
    #[cfg(windows)]
    {
        // The read-only attribute stands for the owner's write bit, and
        // the rest of a mode as Windows lets everyone have it.
        let base = if meta.is_dir() { 0o777 } else { 0o666 };
        Ok(Stat {
            mode: if meta.permissions().readonly() {
                base & !0o222
            } else {
                base
            },
            uid: None,
            gid: None,
            folder: meta.is_dir(),
            link: meta.file_type().is_symlink(),
            device: None,
        })
    }
}

/// Set the permission bits of `path` (through a link) to `mode`.
fn set_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode & 0o7777))
    }
    #[cfg(windows)]
    {
        let mut permissions = std::fs::metadata(path)?.permissions();
        permissions.set_readonly(mode & 0o200 == 0);
        std::fs::set_permissions(path, permissions)
    }
}

/// Give `path` (through a link) to the group `gid`.
fn set_group(path: &Path, gid: u32) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        rustix::fs::chown(path, None, Some(rustix::fs::Gid::from_raw(gid)))?;
        Ok(())
    }
    #[cfg(windows)]
    {
        let _ = (path, gid);
        Err(crate::system::not_yet("Changing a file's group"))
    }
}

/// Why the user cannot change the access of a file owned by `uid` (with
/// a group `gid` asked), or nothing when they can: its owner may (and
/// root), giving it only to a group they are in.
fn refused(uid: Option<u32>, gid: Option<u32>) -> Option<String> {
    #[cfg(unix)]
    {
        let me = rustix::process::geteuid();
        if me.is_root() {
            return None;
        }
        if uid.is_some_and(|uid| uid != me.as_raw()) {
            return Some("only its owner can change who may use it".into());
        }
        if let Some(gid) = gid
            && gid != rustix::process::getegid().as_raw()
            && !rustix::process::getgroups()
                .unwrap_or_default()
                .iter()
                .any(|g| g.as_raw() == gid)
        {
            return Some(format!("you are not in the group {gid}"));
        }
        None
    }
    #[cfg(windows)]
    {
        let _ = uid;
        gid.map(|_| crate::system::not_yet("Changing a file's group").to_string())
    }
}

fn name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

fn gone(e: &ResolveError) -> StepStatus {
    StepStatus::Conflict {
        reason: match e {
            ResolveError::Gone(p) => format!("“{}” is not there any more", name(p)),
            other @ ResolveError::Replaced { .. } => other.to_string(),
        },
    }
}

// ------------------------------------------------------------------ the step

/// Change who may use an entry: its mode as `chmod` takes it, its group,
/// and with `inside` those of everything a folder holds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SetAccess {
    pub entry: EntryRef,
    /// The mode, as `chmod` takes it: `644`, `u+x`, `go-w`, `a=rX`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// The group to give it, by number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<u32>,
    /// Everything a folder holds too: not through a link, nor onto
    /// another drive mounted inside.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub inside: bool,
}

/// What an item was, for putting it back.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AccessWas {
    pub entry: EntryRef,
    /// Its permission bits.
    pub mode: u32,
    /// Its group, when the step changed it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<u32>,
}

impl SetAccess {
    fn parsed(&self) -> anyhow::Result<Option<Mode>> {
        self.mode.as_deref().map(Mode::parse).transpose()
    }

    /// What it makes of an item as it is: its mode and its group.
    fn target(&self, mode: Option<&Mode>, st: &Stat) -> (u32, Option<u32>) {
        (
            mode.map_or(st.mode & 0o7777, |m| m.apply(st.mode, st.folder) & 0o7777),
            self.group.or(st.gid),
        )
    }
}

impl super::Kind for SetAccess {
    const KIND: &'static str = "set_access";
}

impl super::Effect for SetAccess {
    fn describe(&self) -> Change {
        let name = name(&self.entry.path);
        let (inside, of_inside) = if self.inside {
            (" and everything inside", " and of everything inside")
        } else {
            ("", "")
        };
        let (will, did) = match (&self.mode, self.group) {
            (Some(mode), None) => (
                format!("change who may use “{name}”{inside} ({mode})"),
                format!("changed who may use “{name}”{inside}"),
            ),
            (None, Some(_)) => (
                format!("change the group of “{name}”{of_inside}"),
                format!("changed the group of “{name}”{of_inside}"),
            ),
            _ => (
                format!("change the access and group of “{name}”{of_inside}"),
                format!("changed the access and group of “{name}”{of_inside}"),
            ),
        };
        Change::Metadata {
            path: self.entry.path.clone(),
            field: "access".into(),
            words: Some(Words { will, did }),
        }
    }

    fn check(&self, _ctx: &StepContext) -> StepStatus {
        let conflict = |reason: String| StepStatus::Conflict { reason };
        if self.mode.is_none() && self.group.is_none() {
            return conflict("nothing to change: give a mode or a group".into());
        }
        let mode = match self.parsed() {
            Ok(mode) => mode,
            Err(e) => return conflict(e.to_string()),
        };
        if super::in_trash(&self.entry.path).is_some() {
            return conflict(format!(
                "“{}” is in the trash: restore it first",
                name(&self.entry.path)
            ));
        }
        let path = match self.entry.resolve() {
            Ok(path) => path,
            Err(e) => return gone(&e),
        };
        let st = match stat(&path, true) {
            Ok(st) => st,
            Err(e) => return conflict(super::io_words(&e)),
        };
        if let Some(why) = refused(st.uid, self.group.filter(|g| Some(*g) != st.gid)) {
            return conflict(why);
        }
        if crate::fs::volume::read_only(&path) == Some(true) {
            return conflict("the drive is read-only".into());
        }
        if !self.inside && self.target(mode.as_ref(), &st) == (st.mode & 0o7777, st.gid) {
            return StepStatus::AlreadyDone;
        }
        StepStatus::Pending { reversible: true }
    }

    fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let mode = self.parsed()?;
        let path = self.entry.resolve()?;
        let root = stat(&path, true)?;
        if let Some(why) = refused(root.uid, self.group.filter(|g| Some(*g) != root.gid)) {
            anyhow::bail!("“{}”: {why}", name(&path));
        }
        let mut was: Vec<AccessWas> = Vec::new();
        let result = self.change_all(ctx, mode.as_ref(), &path, &root, &mut was);
        if let Err(e) = result {
            // Half a change is none: what was changed goes back.
            put_back(&was);
            return Err(e);
        }
        Ok(Some(Box::new(AccessBack { items: was })))
    }
}

impl SetAccess {
    /// Change `path` (`root`, its stat through a link), then with
    /// `inside` what it holds, recording in `was` what each was.
    fn change_all(
        &self,
        ctx: &StepContext,
        mode: Option<&Mode>,
        path: &Path,
        root: &Stat,
        was: &mut Vec<AccessWas>,
    ) -> anyhow::Result<()> {
        self.change(mode, path, root, was)?;
        // The drive kept what it was told, or says it keeps nothing.
        if let Some(asked) = mode.map(|m| m.apply(root.mode, root.folder) & 0o7777) {
            let now = stat(path, true)?.mode & 0o7777;
            if now != asked && was.iter().any(|w| w.entry.path == path) {
                anyhow::bail!(
                    "the drive holding “{}” did not keep the change: its filesystem keeps no permissions",
                    name(path)
                );
            }
        }
        if !(self.inside && root.folder) {
            return Ok(());
        }
        let mut folders: Vec<PathBuf> = vec![path.to_path_buf()];
        while let Some(folder) = folders.pop() {
            ctx.check_cancelled()?;
            for entry in std::fs::read_dir(&folder)? {
                ctx.check_cancelled()?;
                let item = entry?.path();
                let st = stat(&item, false)?;
                if st.link || (root.device.is_some() && st.device != root.device) {
                    continue;
                }
                self.change(mode, &item, &st, was)?;
                if st.folder {
                    folders.push(item);
                }
            }
        }
        Ok(())
    }

    /// Change one item, recording what it was when it changes.
    fn change(
        &self,
        mode: Option<&Mode>,
        path: &Path,
        st: &Stat,
        was: &mut Vec<AccessWas>,
    ) -> anyhow::Result<()> {
        let (to_mode, to_group) = self.target(mode, st);
        let mode_changes = to_mode != st.mode & 0o7777;
        let group_changes = to_group != st.gid;
        if !mode_changes && !group_changes {
            return Ok(());
        }
        let id = crate::fs::stat::id_of(path)
            .ok_or_else(|| anyhow::anyhow!("“{}” is not there any more", name(path)))?;
        let record = AccessWas {
            entry: EntryRef::new(id, path),
            mode: st.mode & 0o7777,
            group: st.gid.filter(|_| group_changes),
        };
        // The group first: giving a file to a group clears its set-ID bits.
        if let (true, Some(gid)) = (group_changes, to_group) {
            set_group(path, gid).map_err(|e| {
                anyhow::Error::new(e).context(format!("“{}” could not change group", name(path)))
            })?;
        }
        was.push(record);
        // Set after a new group too, which may have cleared bits.
        set_mode(path, to_mode).map_err(|e| {
            anyhow::Error::new(e)
                .context(format!("who may use “{}” could not be changed", name(path)))
        })?;
        Ok(())
    }
}

/// Put back what `was` says, last changed first; what fails stays.
fn put_back(was: &[AccessWas]) -> Vec<AccessWas> {
    let mut now = Vec::new();
    for item in was.iter().rev() {
        let Ok(path) = item.entry.resolve() else {
            continue;
        };
        let Ok(st) = stat(&path, true) else {
            continue;
        };
        if let Some(gid) = item.group
            && Some(gid) != st.gid
            && set_group(&path, gid).is_err()
        {
            continue;
        }
        if set_mode(&path, item.mode).is_ok() {
            now.push(AccessWas {
                entry: item.entry.clone(),
                mode: st.mode & 0o7777,
                group: item.group.and(st.gid),
            });
        }
    }
    now.reverse();
    now
}

// ------------------------------------------------------------------ the inverse

/// Put back the mode and group each item had: `set_access` undone.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AccessBack {
    pub items: Vec<AccessWas>,
}

impl super::Kind for AccessBack {
    const KIND: &'static str = "access_back";
}

impl super::Effect for AccessBack {
    fn describe(&self) -> Change {
        let path = self
            .items
            .first()
            .map(|w| w.entry.path.clone())
            .unwrap_or_default();
        let name = name(&path);
        let more = match self.items.len() {
            0 | 1 => String::new(),
            n => format!(" and {} more", n - 1),
        };
        Change::Metadata {
            path,
            field: "access".into(),
            words: Some(Words {
                will: format!("put back who may use “{name}”{more}"),
                did: format!("put back who may use “{name}”{more}"),
            }),
        }
    }

    fn check(&self, _ctx: &StepContext) -> StepStatus {
        // What is gone since is skipped; the first is what it is about.
        if let Some(first) = self.items.first()
            && let Err(e) = first.entry.resolve()
        {
            return gone(&e);
        }
        let done = self.items.iter().all(|item| {
            item.entry
                .resolve()
                .ok()
                .and_then(|p| stat(&p, true).ok())
                .is_some_and(|st| {
                    st.mode & 0o7777 == item.mode && item.group.is_none_or(|g| Some(g) == st.gid)
                })
        });
        if done {
            StepStatus::AlreadyDone
        } else {
            StepStatus::Pending { reversible: true }
        }
    }

    fn apply(&self, _ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        if let Some(first) = self.items.first() {
            first.entry.resolve()?;
        }
        Ok(Some(Box::new(AccessBack {
            items: put_back(&self.items),
        })))
    }

    /// A folder changed whole holds a record per item: the undo journal
    /// counts them against its bound.
    fn held(&self) -> usize {
        self.items
            .iter()
            .map(|w| w.entry.path.as_os_str().len() + 48)
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode(text: &str, from: u32, folder: bool) -> u32 {
        Mode::parse(text).unwrap().apply(from, folder) & 0o7777
    }

    #[test]
    fn modes_read_as_chmod_reads_them() {
        assert_eq!(mode("644", 0o100_755, false), 0o644);
        assert_eq!(mode("u+x", 0o644, false), 0o744);
        assert_eq!(mode("+x", 0o644, false), 0o755, "no class said: everyone");
        assert_eq!(mode("go-w", 0o666, false), 0o644);
        assert_eq!(mode("a=r", 0o755, false), 0o444);
        assert_eq!(mode("u=rw,go=r", 0o777, false), 0o644);
        assert_eq!(mode("g=u", 0o750, false), 0o770);
        assert_eq!(mode("o=", 0o777, false), 0o770);
        assert_eq!(
            mode("u+x-w", 0o644, false),
            0o544,
            "two actions in a clause"
        );
        // X: search for folders, run only for what someone runs already.
        assert_eq!(mode("a=rX", 0o600, true), 0o555);
        assert_eq!(mode("a=rX", 0o600, false), 0o444);
        assert_eq!(mode("a=rX", 0o700, false), 0o555);
        assert_eq!(mode("go+rX", 0o700, true), 0o755);
        // Special bits.
        assert_eq!(mode("g+s", 0o755, true), 0o2755);
        assert_eq!(mode("u+s", 0o755, false), 0o4755);
        assert_eq!(mode("+t", 0o777, true), 0o1777);
        assert_eq!(mode("o-t", 0o1777, true), 0o777);
        // A folder keeps its set-ID bits through digits, unless five.
        assert_eq!(mode("755", 0o2775, true), 0o2755);
        assert_eq!(mode("00755", 0o2775, true), 0o755);
        assert_eq!(mode("755", 0o2775, false), 0o755);
        assert_eq!(
            mode("g=rx", 0o2775, true),
            0o2755,
            "= keeps a folder's set-ID"
        );
        // The file's type bits stay.
        assert_eq!(
            Mode::parse("600").unwrap().apply(0o100_644, false),
            0o100_600
        );
        for wrong in ["", "8", "999999", "u+q", "z+x", "u", "u+x,", "+uX", "77777"] {
            assert!(Mode::parse(wrong).is_err(), "{wrong:?} is no mode");
        }
    }

    #[cfg(unix)]
    fn stat_mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).unwrap().permissions().mode() & 0o7777
    }

    #[cfg(unix)]
    fn set(path: &Path, mode: u32) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    #[cfg(unix)]
    fn step(path: &Path, mode: &str, inside: bool) -> SetAccess {
        SetAccess {
            entry: EntryRef::from_path(path).unwrap(),
            mode: Some(mode.into()),
            group: None,
            inside,
        }
    }

    #[test]
    #[cfg(unix)]
    fn a_change_of_access_is_undone_and_redone() {
        let tmp = tempfile::tempdir().unwrap();
        let script = tmp.path().join("it's a script.sh");
        std::fs::write(&script, b"#!/bin/sh\n").unwrap();
        set(&script, 0o644);
        let ctx = StepContext::detached();
        let run = step(&script, "u+x", false);
        assert_eq!(
            super::super::Effect::check(&run, &ctx),
            StepStatus::Pending { reversible: true }
        );
        let undo = super::super::Effect::apply(&run, &ctx).unwrap().unwrap();
        assert_eq!(stat_mode(&script), 0o744);
        assert_eq!(
            super::super::Effect::check(&run, &ctx),
            StepStatus::AlreadyDone,
            "applied twice is once"
        );
        let redo = undo.apply(&ctx).unwrap().unwrap();
        assert_eq!(stat_mode(&script), 0o644);
        assert_eq!(undo.check(&ctx), StepStatus::AlreadyDone);
        redo.apply(&ctx).unwrap();
        assert_eq!(stat_mode(&script), 0o744);
        // Said in words, for the review and the toast.
        let Change::Metadata {
            words: Some(words), ..
        } = super::super::Effect::describe(&run)
        else {
            panic!()
        };
        assert_eq!(words.will, "change who may use “it's a script.sh” (u+x)");
    }

    #[test]
    #[cfg(unix)]
    fn inside_a_folder_x_gives_folders_search_and_leaves_links() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("Été");
        let sub = root.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        let doc = sub.join("doc.txt");
        let tool = root.join("tool");
        std::fs::write(&doc, b"x").unwrap();
        std::fs::write(&tool, b"x").unwrap();
        let outside = tmp.path().join("outside.txt");
        std::fs::write(&outside, b"x").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
        set(&root, 0o700);
        set(&sub, 0o700);
        set(&doc, 0o600);
        set(&tool, 0o700);
        set(&outside, 0o600);
        let ctx = StepContext::detached();
        let all = step(&root, "go+rX", true);
        let undo = super::super::Effect::apply(&all, &ctx).unwrap().unwrap();
        assert_eq!(
            [&root, &sub, &doc, &tool].map(|p| stat_mode(p)),
            [0o755, 0o755, 0o644, 0o755]
        );
        assert_eq!(stat_mode(&outside), 0o600, "a link inside is not followed");
        assert!(undo.held() > 0, "the journal counts its records");
        undo.apply(&ctx).unwrap();
        assert_eq!(
            [&root, &sub, &doc, &tool].map(|p| stat_mode(p)),
            [0o700, 0o700, 0o600, 0o700]
        );
    }

    #[test]
    #[cfg(unix)]
    fn a_file_of_the_trash_or_of_someone_else_is_not_changed() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = StepContext::detached();
        let trash = tmp.path().join("Trash");
        std::fs::create_dir_all(trash.join("files")).unwrap();
        std::fs::create_dir_all(trash.join("info")).unwrap();
        let item = trash.join("files/old.txt");
        std::fs::write(&item, b"x").unwrap();
        std::fs::write(
            trash.join("info/old.txt.trashinfo"),
            "[Trash Info]\nPath=/somewhere/old.txt\nDeletionDate=2026-10-13T10:00:00\n",
        )
        .unwrap();
        assert!(matches!(
            super::super::Effect::check(&step(&item, "u+x", false), &ctx),
            StepStatus::Conflict { reason } if reason.contains("in the trash")
        ));
        // A file of root's, for a user who is not root.
        if !rustix::process::geteuid().is_root() {
            let theirs = Path::new("/");
            let status = super::super::Effect::check(&step(theirs, "o+w", false), &ctx);
            assert!(
                matches!(&status, StepStatus::Conflict { reason } if reason.contains("only its owner")),
                "{status:?}"
            );
        }
    }
}
