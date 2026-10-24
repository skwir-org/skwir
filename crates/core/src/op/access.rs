//! Who may read, write and run a file: its mode and its group, changed as
//! plan steps (ADR-058). `set_access` takes the mode as `chmod` does
//! (`644`, `u+x`, `go-w`, `a=rX`; a clause naming no class leaves out
//! what the umask does), the group by number, and with `inside`
//! everything a folder holds; its inverse (`access_back`) puts back each
//! item's mode and group as they were.
//!
//! What a link given leads to is changed, as `chmod` changes it. Inside a
//! folder no link is followed, nothing on another drive mounted inside is
//! reached, and a trash's folder is left alone. Each folder is listed
//! through its own descriptor, opened without following a link and
//! checked to be the folder stat-ed; each item is changed through a hold
//! on it (`sys::access::Held`), never through a link or another file put
//! in its place since.
//!
//! A folder whose owner can still list and open it once changed is
//! changed before what it holds; any other after it: `a=rX`, `u=r` and
//! `u=` alike reach every item. Putting back follows the same rule with
//! the modes put back, so Undo and Redo reach what the change reached.
//!
//! A new group is given first, and the mode is worked out from what the
//! system left: giving a file to a group clears its set-ID bits, which
//! stay cleared unless the mode asked sets them (as `chgrp`, then
//! `chmod`). The owner is never changed: giving a file away needs root,
//! which skwir never asks for.
//!
//! A drive whose filesystem keeps no permissions (FAT) takes the call and
//! keeps nothing: the step reads the mode back and says so, rather than
//! guessing from the filesystem's name.
//!
//! On Windows a mode is the read-only attribute: the owner's write bit
//! (`u-w` sets it, `u+w` clears it); a group is not yet.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use serde::{Deserialize, Serialize};

use super::sys::access::{self as sys, Held, Me, Meta};
use super::{Change, EntryRef, Operation, ResolveError, StepContext, StepStatus, Words};
use crate::entry::FileId;
use crate::fs::Dir;

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
    /// `u`, `g`, `o` as bits 4, 2, 1; none said is everyone, but for what
    /// the umask leaves out (`umask`).
    who: u8,
    actions: Vec<(char, Perms)>,
    /// What a clause naming no class leaves out, as `chmod` does: the
    /// umask's bits. Zero for a clause that names its classes.
    umask: u32,
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

impl Perms {
    /// Whether it gives read, write or run bits when added or set.
    fn gives_rwx(&self) -> bool {
        match self {
            Perms::Copy(_) => true,
            Perms::Letters(letters) => letters.chars().any(|c| matches!(c, 'r' | 'w' | 'x' | 'X')),
        }
    }
}

const USER: u8 = 4;
const GROUP: u8 = 2;
const OTHERS: u8 = 1;
const SETUID: u32 = 0o4000;
const SETGID: u32 = 0o2000;
const STICKY: u32 = 0o1000;

impl Mode {
    /// Read a mode as `chmod` writes it, a clause naming no class leaving
    /// out what the system's umask does ([`Mode::parse_with`]).
    ///
    /// # Errors
    /// What is not a mode, in the user's words.
    pub fn parse(text: &str) -> anyhow::Result<Mode> {
        Self::parse_with(text, sys::umask())
    }

    /// Read a mode as `chmod` writes it, with `umask` as the umask: a
    /// clause naming no class (`+x`, `=r`) leaves out its bits. `None` is
    /// a umask the system does not say (macOS): such a clause is then
    /// refused where it would give something (`+`, `=` with `r`, `w`,
    /// `x`, `X` or a copy), since `chmod` would have left some of it out;
    /// one that takes away (`-w`) takes it from everyone.
    ///
    /// # Errors
    /// What is not a mode, in the user's words.
    pub fn parse_with(text: &str, umask: Option<u32>) -> anyhow::Result<Mode> {
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
            let umask = if who != 0 {
                0
            } else if let Some(umask) = umask {
                umask & 0o777
            } else if actions
                .iter()
                .any(|(op, perms)| *op != '-' && perms.gives_rwx())
            {
                anyhow::bail!(
                    "“{part}” names no class, and this system does not say what its umask leaves \
                     out, as chmod would: name the classes, as u{part} for the owner or a{part} \
                     for everyone"
                );
            } else {
                0
            };
            clauses.push(Clause {
                who,
                actions,
                umask,
            });
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
            // No class named: what the umask holds is left out, whichever
            // the action (the umask holds no special bit).
            given &= !self.umask;
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

// ------------------------------------------------------------------ words

/// Why the user cannot change the access of a file owned by `uid` (with
/// a group `gid` asked), or nothing when they can: its owner may (and
/// root), giving it only to a group they are in. `me` is `None` where
/// users are not numbers (Windows): a mode may be asked, a group not yet.
fn refused(me: Option<&Me>, uid: Option<u32>, gid: Option<u32>) -> Option<String> {
    let Some(me) = me else {
        return gid.map(|_| crate::system::not_yet("Changing a file's group").to_string());
    };
    if me.root {
        return None;
    }
    if uid.is_some_and(|uid| uid != me.uid) {
        return Some("only its owner can change who may use it".into());
    }
    if let Some(gid) = gid
        && !me.may_give(gid)
    {
        return Some(format!("you are not in the group {gid}"));
    }
    None
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

/// What reading a folder or an item inside one failed on, by its path.
fn unreadable(path: &Path) -> String {
    format!("“{}” could not be read", path.display())
}

/// Whether a folder whose owner has `mode` can be listed and looked into
/// by them: a walk changes such a folder before what it holds.
fn opens(mode: u32) -> bool {
    mode & 0o500 == 0o500
}

/// Whether the folder at `path` is a trash, which an `inside` walk leaves
/// alone: one `in_trash` knows (its `files` are trashed items: a home's
/// `Trash`, a drive's `.Trash-<uid>`, `.Trash/<uid>`), a drive's shared
/// `.Trash`, or Windows' Recycle Bin.
fn is_a_trash(path: &Path) -> bool {
    super::trash_info_of(&path.join("files").join("item")).is_some()
        || super::trash_info_of(&path.join("$Ritem")).is_some()
        || path.file_name().is_some_and(|name| {
            name == OsStr::new(".Trash") || name.eq_ignore_ascii_case("$Recycle.Bin")
        })
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
    /// another drive mounted inside, nor into a trash.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub inside: bool,
}

/// What an item was, for putting it back.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AccessWas {
    /// The item: its path, and the identity of the file changed.
    pub entry: EntryRef,
    /// Its permission bits.
    pub mode: u32,
    /// Its group, when the step changed it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<u32>,
    /// Its path is followed to what it leads to: the entry given, which
    /// may be a link (its identity is then what the link leads to's).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub follow: bool,
}

impl SetAccess {
    fn parsed(&self) -> anyhow::Result<Option<Mode>> {
        self.mode.as_deref().map(Mode::parse).transpose()
    }

    /// What it makes of an item as it is: its mode and its group.
    fn target(&self, mode: Option<&Mode>, meta: &Meta) -> (u32, Option<u32>) {
        (
            mode.map_or(meta.mode, |m| m.apply(meta.mode, meta.folder) & 0o7777),
            self.group.or(meta.gid),
        )
    }

    /// Whether it changes an item as it is.
    fn changes(&self, mode: Option<&Mode>, meta: &Meta) -> bool {
        self.target(mode, meta) != (meta.mode, meta.gid)
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

    /// The dry run walks what the step would change (with `inside`, all a
    /// folder holds, as far as it can be read without changing anything,
    /// stopping when the plan is): nothing to change is already done; an
    /// item inside of someone else's that would change is a conflict,
    /// named; a group given to an item whose group the user is not in is
    /// pending but not undoable (its group could not be given back).
    fn check(&self, ctx: &StepContext) -> StepStatus {
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
        let top = match sys::stat(&path, true) {
            Ok(top) => top,
            Err(e) => return conflict(super::io_words(&e)),
        };
        let me = sys::me();
        if let Some(why) = refused(
            me.as_ref(),
            top.uid,
            self.group.filter(|g| Some(*g) != top.gid),
        ) {
            return conflict(why);
        }
        if crate::fs::volume::read_only(&path) == Some(true) {
            return conflict("the drive is read-only".into());
        }
        self.foresee(ctx, mode.as_ref(), &path, &top, me.as_ref())
    }

    fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let mode = self.parsed()?;
        let path = self.entry.resolve()?;
        let top = sys::stat(&path, true).with_context(|| unreadable(&path))?;
        let me = sys::me();
        if let Some(why) = refused(
            me.as_ref(),
            top.uid,
            self.group.filter(|g| Some(*g) != top.gid),
        ) {
            anyhow::bail!("“{}”: {why}", name(&path));
        }
        let mut changer = Changer {
            step: self,
            mode: mode.as_ref(),
            me: me.as_ref(),
            was: Vec::new(),
        };
        let walked = self.walk(ctx, mode.as_ref(), &path, &top, &mut changer);
        let was = changer.was;
        if let Err(e) = walked {
            // Half a change is none: what was changed goes back, and what
            // could not is said.
            let (_, failed) = put_back(&was);
            if failed.is_empty() {
                return Err(e);
            }
            let what = if e.is::<super::Cancelled>() {
                "stopped".to_owned()
            } else {
                super::said(&e)
            };
            anyhow::bail!("{what}, and {}", not_put_back(&failed));
        }
        Ok(Some(Box::new(AccessBack { items: was })))
    }
}

// ------------------------------------------------------------------ the walk

/// Where an item is, for its hold: the entry given, by its path (a link
/// followed), or a name in a folder held open (no link followed).
#[derive(Clone, Copy)]
enum At<'a> {
    Top(&'a Path),
    In(&'a Dir, &'a OsStr),
}

impl At<'_> {
    fn hold(self, id: FileId) -> std::io::Result<Held> {
        match self {
            At::Top(path) => Held::path(path, id, true),
            At::In(dir, name) => Held::at(dir, name, id),
        }
    }
}

/// What a walk does with each item it reaches: changes it ([`Changer`]),
/// or says what changing it would do ([`Foresee`]).
trait Visit {
    fn visit(&mut self, at: At<'_>, path: &Path, meta: &Meta) -> anyhow::Result<()>;
}

/// A folder the walk is in: held open, its listing as far as read, and
/// its own change when that waits for what the folder holds.
struct Frame {
    dir: Dir,
    id: FileId,
    path: PathBuf,
    /// Its name in the folder above (empty for the entry given).
    name: OsString,
    listing: Box<dyn Iterator<Item = std::io::Result<crate::fs::dir::Listed>>>,
    after: Option<Meta>,
}

impl Frame {
    /// `dir`, opened as the folder `meta` names (`sys::open_folder`), to
    /// be walked.
    fn new(
        dir: std::io::Result<Dir>,
        meta: &Meta,
        path: PathBuf,
        name: OsString,
        after: Option<Meta>,
    ) -> anyhow::Result<Frame> {
        let dir = dir.with_context(|| unreadable(&path))?;
        let listing = Box::new(dir.read().with_context(|| unreadable(&path))?);
        Ok(Frame {
            dir,
            id: meta.id,
            path,
            name,
            listing,
            after,
        })
    }
}

impl SetAccess {
    /// Visit the entry given (`top`, stat-ed through a link, at `root`),
    /// then with `inside` what it holds, in the order its change needs: a
    /// folder its owner can still open once changed first, then what it
    /// holds; any other folder after what it holds.
    fn walk(
        &self,
        ctx: &StepContext,
        mode: Option<&Mode>,
        root: &Path,
        top: &Meta,
        visit: &mut dyn Visit,
    ) -> anyhow::Result<()> {
        if !(self.inside && top.folder) {
            return visit.visit(At::Top(root), root, top);
        }
        let first = |meta: &Meta| opens(self.target(mode, meta).0);
        let early = first(top);
        if early {
            visit.visit(At::Top(root), root, top)?;
        }
        let mut stack = vec![Frame::new(
            sys::open_top(root, top.id),
            top,
            root.to_path_buf(),
            OsString::new(),
            (!early).then_some(*top),
        )?];
        loop {
            let Some(frame) = stack.last_mut() else {
                return Ok(());
            };
            let Some(listed) = frame.listing.next() else {
                let done = stack.pop().expect("the folder just read");
                if let Some(meta) = done.after {
                    let at = stack
                        .last()
                        .map_or(At::Top(root), |up| At::In(&up.dir, &done.name));
                    visit.visit(at, &done.path, &meta)?;
                }
                continue;
            };
            ctx.check_cancelled()?;
            let frame = stack.last().expect("the folder just read");
            let name = listed.with_context(|| unreadable(&frame.path))?.name;
            let path = frame.path.join(&name);
            let meta = sys::stat_at(&frame.dir, &name).with_context(|| unreadable(&path))?;
            #[cfg(test)]
            tests::between(&path);
            // No link is followed, no other drive reached.
            if meta.link || meta.id.dev != top.id.dev {
                continue;
            }
            if !meta.folder {
                visit.visit(At::In(&frame.dir, &name), &path, &meta)?;
                continue;
            }
            // No trash is changed, and a folder met above (a bind mount
            // of it inside) is not walked again.
            if is_a_trash(&path) || stack.iter().any(|f| f.id == meta.id) {
                continue;
            }
            let early = first(&meta);
            if early {
                visit.visit(At::In(&frame.dir, &name), &path, &meta)?;
            }
            let child = Frame::new(
                sys::open_folder(&frame.dir, &name, meta.id),
                &meta,
                path,
                name,
                (!early).then_some(meta),
            )?;
            stack.push(child);
        }
    }

    /// The dry run of [`Effect::check`](super::Effect::check), as `me`.
    fn foresee(
        &self,
        ctx: &StepContext,
        mode: Option<&Mode>,
        root: &Path,
        top: &Meta,
        me: Option<&Me>,
    ) -> StepStatus {
        let mut seen = Foresee {
            step: self,
            mode,
            me,
            changes: false,
            lost: false,
        };
        let walked = self.walk(ctx, mode, root, top, &mut seen);
        let theirs = walked
            .as_ref()
            .err()
            .and_then(|e| e.downcast_ref::<Theirs>());
        if let Some(Theirs(item)) = theirs {
            return StepStatus::Conflict {
                reason: format!(
                    "“{}” inside is not yours: only its owner can change who may use it",
                    item.display()
                ),
            };
        }
        match walked {
            Ok(()) if !seen.changes => StepStatus::AlreadyDone,
            // Changes, or a walk cut short (cancelled, or a folder that
            // only the change itself opens to its owner): apply finds the
            // rest, and says what it could not do.
            _ => StepStatus::Pending {
                reversible: !seen.lost,
            },
        }
    }
}

/// A dry run's visit: what the change would do, without doing it.
struct Foresee<'a> {
    step: &'a SetAccess,
    mode: Option<&'a Mode>,
    me: Option<&'a Me>,
    /// Something would change.
    changes: bool,
    /// A group would be given to an item whose group the user is not in:
    /// Undo could not give it back.
    lost: bool,
}

/// An item of someone else's that the change would reach.
#[derive(Debug)]
struct Theirs(PathBuf);

impl std::fmt::Display for Theirs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "“{}” is not yours", self.0.display())
    }
}

impl std::error::Error for Theirs {}

impl Visit for Foresee<'_> {
    fn visit(&mut self, _at: At<'_>, path: &Path, meta: &Meta) -> anyhow::Result<()> {
        if !self.step.changes(self.mode, meta) {
            return Ok(());
        }
        self.changes = true;
        if let Some(me) = self.me.filter(|me| !me.root) {
            if meta.uid.is_some_and(|uid| uid != me.uid) {
                return Err(anyhow::Error::new(Theirs(path.to_path_buf())));
            }
            let (_, to_group) = self.step.target(self.mode, meta);
            if to_group != meta.gid && meta.gid.is_some_and(|gid| !me.may_give(gid)) {
                self.lost = true;
            }
        }
        Ok(())
    }
}

/// The change's visit: each item changed, what it was recorded first.
struct Changer<'a> {
    step: &'a SetAccess,
    mode: Option<&'a Mode>,
    me: Option<&'a Me>,
    was: Vec<AccessWas>,
}

impl Visit for Changer<'_> {
    fn visit(&mut self, at: At<'_>, path: &Path, meta: &Meta) -> anyhow::Result<()> {
        if !self.step.changes(self.mode, meta) {
            return Ok(());
        }
        let failed = || format!("who may use “{}” could not be changed", path.display());
        let held = at.hold(meta.id).with_context(failed)?;
        let (_, to_group) = self.step.target(self.mode, meta);
        let regroup = to_group.filter(|gid| Some(*gid) != meta.gid);
        // Recorded before anything changes: a change that fails halfway
        // is put back too.
        self.was.push(AccessWas {
            entry: EntryRef::new(meta.id, path),
            mode: meta.mode,
            group: meta.gid.filter(|_| regroup.is_some()),
            follow: matches!(at, At::Top(_)),
        });
        let mut now = *meta;
        if let Some(gid) = regroup {
            held.set_group(gid).with_context(|| {
                format!("“{}” could not be given to the group {gid}", path.display())
            })?;
            // What the system left of its mode: a file given to a group
            // loses its set-ID bits.
            now = held.stat().with_context(failed)?;
        }
        let Some(mode) = self.mode else {
            return Ok(());
        };
        let asked = mode.apply(now.mode, now.folder) & 0o7777;
        if asked == now.mode {
            return Ok(());
        }
        held.set_mode(asked).with_context(failed)?;
        // The drive kept what it was told, or says it keeps nothing.
        if matches!(at, At::Top(_)) {
            let kept = held.stat().with_context(failed)?;
            if kept.mode != asked {
                let not_in_group = kept
                    .gid
                    .zip(self.me)
                    .is_some_and(|(gid, me)| !me.may_give(gid));
                if kept.mode ^ asked == SETGID && not_in_group {
                    anyhow::bail!(
                        "the system cleared the set-group-ID bit of “{}”: only a member of its group may set it",
                        name(path)
                    );
                }
                anyhow::bail!(
                    "the drive holding “{}” did not keep the change: its filesystem keeps no permissions",
                    name(path)
                );
            }
        }
        Ok(())
    }
}

// ------------------------------------------------------------------ putting back

/// The order `items` are put back in: by their paths, a folder before
/// what it holds when its owner can look into it once put back, after it
/// otherwise, so every item is reached whichever way the change went.
fn back_order(items: &[AccessWas]) -> Vec<usize> {
    let mut sorted: Vec<usize> = (0..items.len()).collect();
    sorted.sort_by(|&a, &b| items[a].entry.path.cmp(&items[b].entry.path));
    let mut order = Vec::with_capacity(items.len());
    // The items above the one at hand, each with whether it waits for
    // what it holds.
    let mut above: Vec<(usize, bool)> = Vec::new();
    for i in sorted {
        let path = &items[i].entry.path;
        while let Some(&(up, waits)) = above.last() {
            let up_path = &items[up].entry.path;
            if path != up_path && path.starts_with(up_path) {
                break;
            }
            above.pop();
            if waits {
                order.push(up);
            }
        }
        let waits = items[i].mode & 0o100 == 0;
        if !waits {
            order.push(i);
        }
        above.push((i, waits));
    }
    while let Some((up, waits)) = above.pop() {
        if waits {
            order.push(up);
        }
    }
    order
}

/// Put back what `was` says, in [`back_order`]: what each now is, for
/// putting it back again, and what could not be put back, with why. An
/// item gone since, or another file in its place, is left alone.
fn put_back(was: &[AccessWas]) -> (Vec<AccessWas>, Vec<(PathBuf, anyhow::Error)>) {
    let mut now = vec![None; was.len()];
    let mut failed = Vec::new();
    for i in back_order(was) {
        match put_back_one(&was[i]) {
            Ok(item) => now[i] = item,
            Err(e) => failed.push((was[i].entry.path.clone(), e)),
        }
    }
    (now.into_iter().flatten().collect(), failed)
}

/// Put back one item: what it was before, `None` when it is gone.
fn put_back_one(item: &AccessWas) -> anyhow::Result<Option<AccessWas>> {
    let held = match Held::path(&item.entry.path, item.entry.id, item.follow) {
        Ok(held) => held,
        Err(e) if sys::is_gone(&e) => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let st = held.stat()?;
    let mut fresh = st;
    if let Some(gid) = item.group
        && Some(gid) != st.gid
    {
        held.set_group(gid)
            .with_context(|| format!("it could not be given back to the group {gid}"))?;
        fresh = held.stat()?;
    }
    if fresh.mode != item.mode {
        held.set_mode(item.mode)?;
    }
    Ok(Some(AccessWas {
        entry: item.entry.clone(),
        mode: st.mode,
        group: item.group.and(st.gid),
        follow: item.follow,
    }))
}

/// What could not be put back, in words: the first, and how many more.
fn not_put_back(failed: &[(PathBuf, anyhow::Error)]) -> String {
    let Some((path, e)) = failed.first() else {
        return String::new();
    };
    let more = match failed.len() {
        1 => String::new(),
        n => format!(" (and {} more)", n - 1),
    };
    format!(
        "who may use “{}” could not be put back{more}: {}",
        path.display(),
        super::said(e)
    )
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
        let now = |item: &AccessWas| sys::stat(&item.entry.path, item.follow);
        // What is gone since is skipped; the first is what it is about.
        if let Some(first) = self.items.first() {
            match now(first) {
                Err(_) => {
                    return StepStatus::Conflict {
                        reason: format!("“{}” is not there any more", name(&first.entry.path)),
                    };
                }
                Ok(meta) if meta.id != first.entry.id => {
                    return StepStatus::Conflict {
                        reason: format!("“{}” is now another file", name(&first.entry.path)),
                    };
                }
                Ok(_) => {}
            }
        }
        let done = self.items.iter().all(|item| {
            now(item)
                .ok()
                .filter(|meta| meta.id == item.entry.id)
                .is_none_or(|meta| {
                    meta.mode == item.mode && item.group.is_none_or(|g| Some(g) == meta.gid)
                })
        });
        if done {
            StepStatus::AlreadyDone
        } else {
            StepStatus::Pending { reversible: true }
        }
    }

    /// Puts back what it can, then fails naming what it could not: the
    /// action stays to be undone again.
    fn apply(&self, _ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let (now, failed) = put_back(&self.items);
        if !failed.is_empty() {
            anyhow::bail!("{}", not_put_back(&failed));
        }
        Ok(Some(Box::new(AccessBack { items: now })))
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
    #[cfg(unix)]
    use crate::op::Effect;

    thread_local! {
        /// What a test does between the walk's stat of an item and its
        /// hold on it, or its opening of a folder.
        static BETWEEN: std::cell::RefCell<Option<Box<dyn FnMut(&Path)>>> =
            const { std::cell::RefCell::new(None) };
    }

    /// The walk stat-ed the item at `path` and is about to act on it.
    pub(super) fn between(path: &Path) {
        BETWEEN.with(|hook| {
            if let Some(hook) = hook.borrow_mut().as_mut() {
                hook(path);
            }
        });
    }

    fn mode(text: &str, from: u32, folder: bool) -> u32 {
        Mode::parse_with(text, Some(0o022))
            .unwrap()
            .apply(from, folder)
            & 0o7777
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

    /// A clause naming no class leaves out what the umask does, as chmod
    /// (finding #8): `+w` under 022 is the owner's alone. Where the umask
    /// is not said, one that would give something is refused in words.
    #[test]
    fn a_clause_naming_no_class_leaves_out_the_umask() {
        let with = |text: &str, from: u32, umask: u32| {
            Mode::parse_with(text, Some(umask))
                .unwrap()
                .apply(from, false)
                & 0o7777
        };
        assert_eq!(with("+w", 0o444, 0o022), 0o644);
        assert_eq!(with("+w", 0o444, 0o002), 0o664);
        assert_eq!(with("-w", 0o666, 0o022), 0o466, "taken as chmod takes it");
        assert_eq!(with("=rw", 0o777, 0o077), 0o600);
        assert_eq!(with("=rwx", 0o4777, 0o022), 0o755, "= clears the rest");
        assert_eq!(with("+s", 0o755, 0o777), 0o6755, "no umask bit is special");
        assert_eq!(with("a+w", 0o444, 0o022), 0o666, "a class named: all of it");
        assert_eq!(with("ug+w", 0o444, 0o022), 0o664);
        // A umask not said: what gives is refused, naming the classes.
        let refused = Mode::parse_with("+x", None).unwrap_err().to_string();
        assert!(
            refused.contains("names no class")
                && refused.contains("u+x")
                && refused.contains("a+x"),
            "{refused}"
        );
        assert!(Mode::parse_with("u+x,=r", None).is_err());
        let taken = Mode::parse_with("-w", None).unwrap().apply(0o666, false);
        assert_eq!(taken & 0o7777, 0o444, "taking away needs no umask");
        for given in ["u+x", "a=r", "go-w", "+s", "+t", "=", "644"] {
            assert!(Mode::parse_with(given, None).is_ok(), "{given}");
        }
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn linux_says_its_umask() {
        assert!(sys::umask().is_some());
    }

    #[cfg(unix)]
    fn stat_mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::symlink_metadata(path)
            .unwrap()
            .permissions()
            .mode()
            & 0o7777
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

    /// A folder on the disk the tests run on, never tmpfs; what a test
    /// left unreadable is opened again before it is removed.
    #[cfg(unix)]
    struct Scratch(tempfile::TempDir);

    #[cfg(unix)]
    impl Scratch {
        fn new() -> Scratch {
            let base = dirs::home_dir().unwrap().join(".cache/skwir-tests");
            std::fs::create_dir_all(&base).unwrap();
            Scratch(tempfile::tempdir_in(base).unwrap())
        }

        fn path(&self) -> &Path {
            self.0.path()
        }
    }

    #[cfg(unix)]
    impl Drop for Scratch {
        fn drop(&mut self) {
            fn open_up(path: &Path) {
                let Ok(meta) = std::fs::symlink_metadata(path) else {
                    return;
                };
                if meta.is_dir() {
                    set(path, 0o700);
                    for entry in std::fs::read_dir(path).into_iter().flatten().flatten() {
                        open_up(&entry.path());
                    }
                }
            }
            open_up(self.0.path());
        }
    }

    #[cfg(unix)]
    fn apply(step: &SetAccess) -> anyhow::Result<Box<dyn Operation>> {
        Effect::apply(step, &StepContext::detached()).map(|undo| undo.expect("an inverse"))
    }

    #[test]
    #[cfg(unix)]
    fn a_change_of_access_is_undone_and_redone() {
        let tmp = Scratch::new();
        let script = tmp.path().join("it's a script.sh");
        std::fs::write(&script, b"#!/bin/sh\n").unwrap();
        set(&script, 0o644);
        let ctx = StepContext::detached();
        let run = step(&script, "u+x", false);
        assert_eq!(
            Effect::check(&run, &ctx),
            StepStatus::Pending { reversible: true }
        );
        let undo = apply(&run).unwrap();
        assert_eq!(stat_mode(&script), 0o744);
        assert_eq!(
            Effect::check(&run, &ctx),
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
        } = Effect::describe(&run)
        else {
            panic!()
        };
        assert_eq!(words.will, "change who may use “it's a script.sh” (u+x)");
    }

    #[test]
    #[cfg(unix)]
    fn inside_a_folder_x_gives_folders_search_and_leaves_links() {
        let tmp = Scratch::new();
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
        let undo = apply(&all).unwrap();
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

    /// Finding #10: a recursive change applied again is already done, and
    /// a trash inside the folder is left alone.
    #[test]
    #[cfg(unix)]
    fn a_recursive_change_applied_again_is_done_and_leaves_the_trash() {
        let tmp = Scratch::new();
        let root = tmp.path().join("drive");
        let trash = root.join(".Trash-1000");
        std::fs::create_dir_all(trash.join("files")).unwrap();
        std::fs::create_dir_all(trash.join("info")).unwrap();
        let trashed = trash.join("files/old draft.txt");
        std::fs::write(&trashed, b"x").unwrap();
        let doc = root.join("doc.txt");
        std::fs::write(&doc, b"x").unwrap();
        set(&trashed, 0o600);
        set(&doc, 0o600);
        let ctx = StepContext::detached();
        let all = step(&root, "go+r", true);
        assert_eq!(
            Effect::check(&all, &ctx),
            StepStatus::Pending { reversible: true }
        );
        apply(&all).unwrap();
        assert_eq!(stat_mode(&doc), 0o644);
        assert_eq!(stat_mode(&trashed), 0o600, "the trash is left alone");
        assert_eq!(
            Effect::check(&all, &ctx),
            StepStatus::AlreadyDone,
            "applied again, nothing changes"
        );
    }

    /// Finding #10: an item inside of someone else's that the change
    /// would reach is a conflict, named, before anything changes. (As a
    /// user whose files the fixture's are not: a test cannot make a file
    /// of another user's.)
    #[test]
    #[cfg(unix)]
    fn an_item_inside_of_someone_elses_is_foreseen() {
        let tmp = Scratch::new();
        let root = tmp.path().join("shared");
        std::fs::create_dir_all(&root).unwrap();
        let theirs = root.join("their notes.txt");
        std::fs::write(&theirs, b"x").unwrap();
        set(&root, 0o755);
        set(&theirs, 0o666);
        let all = step(&root, "go-w", true);
        let top = sys::stat(&root, true).unwrap();
        let someone = Me {
            uid: top.uid.unwrap() + 1,
            root: false,
            groups: Vec::new(),
        };
        let status = all.foresee(
            &StepContext::detached(),
            all.parsed().unwrap().as_ref(),
            &root,
            &top,
            Some(&someone),
        );
        assert!(
            matches!(&status, StepStatus::Conflict { reason } if reason.contains("their notes.txt") && reason.contains("not yours")),
            "{status:?}"
        );
        // A group given where the user is not in the item's: not undoable.
        let regroup = SetAccess {
            group: Some(top.gid.unwrap() + 1),
            mode: None,
            ..all
        };
        let outsider = Me {
            uid: top.uid.unwrap(),
            root: false,
            groups: Vec::new(),
        };
        assert_eq!(
            regroup.foresee(&StepContext::detached(), None, &root, &top, Some(&outsider)),
            StepStatus::Pending { reversible: false }
        );
    }

    /// Findings #2 and #5: undone and redone, a folder its owner could
    /// not look into before is reached each way, and a level that leaves
    /// its owner unable to open a folder reaches what it holds.
    #[test]
    #[cfg(unix)]
    fn a_folder_its_owner_cannot_look_into_is_undone_and_redone() {
        if rustix::process::geteuid().is_root() {
            return; // Root looks into any folder: nothing to show.
        }
        let tmp = Scratch::new();
        let root = tmp.path().join("photos");
        let sub = root.join("no search");
        std::fs::create_dir_all(&sub).unwrap();
        let photo = sub.join("it's a photo.jpg");
        std::fs::write(&photo, b"x").unwrap();
        set(&photo, 0o444);
        set(&sub, 0o644);
        set(&root, 0o755);
        let ctx = StepContext::detached();
        // What a folder its owner cannot look into holds is read by
        // opening the folder a moment.
        let inside = |folder: &Path, item: &Path| {
            let shut = stat_mode(folder);
            set(folder, 0o700);
            let mode = stat_mode(item);
            set(folder, shut);
            mode
        };
        let undo = apply(&step(&root, "u+rwX", true)).unwrap();
        assert_eq!([stat_mode(&sub), stat_mode(&photo)], [0o744, 0o644]);
        let redo = undo.apply(&ctx).unwrap().unwrap();
        assert_eq!([stat_mode(&sub), inside(&sub, &photo)], [0o644, 0o444]);
        redo.apply(&ctx).unwrap();
        assert_eq!(
            [stat_mode(&sub), stat_mode(&photo)],
            [0o744, 0o644],
            "redone, what the folder holds is reached"
        );

        // The owner's "List items only" (`u=r`) and "None" (`u=`), with
        // everything inside, then back.
        set(&root, 0o700);
        set(&sub, 0o700);
        set(&photo, 0o600);
        let modes = || {
            let shut = stat_mode(&root);
            set(&root, 0o700);
            let modes = [shut, stat_mode(&sub), inside(&sub, &photo)];
            set(&root, shut);
            modes
        };
        for level in ["u=r", "u="] {
            let undo = apply(&step(&root, level, true)).unwrap();
            let want = if level == "u=r" { 0o400 } else { 0o000 };
            assert_eq!(modes(), [want; 3], "{level}");
            let redo = undo.apply(&ctx).unwrap().unwrap();
            assert_eq!(modes(), [0o700, 0o700, 0o600], "{level} undone");
            redo.apply(&ctx).unwrap();
            assert_eq!(modes(), [want; 3], "{level} redone");
            undo.apply(&ctx).unwrap();
            assert_eq!(modes(), [0o700, 0o700, 0o600]);
        }
    }

    /// Finding #1: a link put where a folder or a file was, between the
    /// walk's stat and its act, is not followed: the change fails, says
    /// so, and puts back what it changed.
    #[test]
    #[cfg(unix)]
    fn a_link_put_in_place_mid_walk_is_not_followed() {
        let tmp = Scratch::new();
        let outside = tmp.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        let secret = outside.join("secret.txt");
        std::fs::write(&secret, b"x").unwrap();
        set(&secret, 0o600);
        set(&outside, 0o700);
        // A folder changed before it is walked (`go+rX`), one walked
        // before it is changed (`u=`), a file.
        for (swapped, level) in [("sub", "go+rX"), ("sub", "u="), ("file.txt", "go+r")] {
            let root = tmp.path().join(format!("shared {level}"));
            std::fs::create_dir_all(root.join("sub")).unwrap();
            std::fs::write(root.join("file.txt"), b"x").unwrap();
            std::fs::write(root.join("sub/inner.txt"), b"x").unwrap();
            for item in ["file.txt", "sub/inner.txt"] {
                set(&root.join(item), 0o600);
            }
            set(&root.join("sub"), 0o711);
            set(&root, 0o711);
            let target = if swapped == "sub" { &outside } else { &secret };
            let at = root.join(swapped);
            let link_to = target.clone();
            BETWEEN.with(|hook| {
                *hook.borrow_mut() = Some(Box::new(move |path: &Path| {
                    if path == at {
                        if path.is_dir() {
                            std::fs::rename(path, path.with_extension("moved")).unwrap();
                        } else {
                            std::fs::remove_file(path).unwrap();
                        }
                        std::os::unix::fs::symlink(&link_to, path).unwrap();
                    }
                }));
            });
            let result = apply(&step(&root, level, true));
            BETWEEN.with(|hook| *hook.borrow_mut() = None);
            let error = super::super::said(&result.unwrap_err());
            assert!(
                error.contains(swapped) && error.contains("link"),
                "{level}: {error}"
            );
            assert_eq!(
                [stat_mode(&outside), stat_mode(&secret)],
                [0o700, 0o600],
                "{level}: what the link leads to is untouched"
            );
            assert_eq!(stat_mode(&root), 0o711, "{level}: what changed is put back");
        }
    }

    /// Finding #3: what cannot be put back is said, the rest put back;
    /// an item gone since is left alone.
    #[test]
    #[cfg(unix)]
    fn what_cannot_be_put_back_is_said() {
        if rustix::process::geteuid().is_root() {
            return; // Root reaches any item: nothing fails.
        }
        let tmp = Scratch::new();
        let root = tmp.path().join("notes");
        let sub = root.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        let kept = sub.join("kept.txt");
        let gone = root.join("gone.txt");
        let first = root.join("first.txt");
        for file in [&kept, &gone, &first] {
            std::fs::write(file, b"x").unwrap();
            set(file, 0o600);
        }
        set(&sub, 0o755);
        set(&root, 0o755);
        let ctx = StepContext::detached();
        let undo = apply(&step(&root, "go+r", true)).unwrap();
        std::fs::remove_file(&gone).unwrap();
        // Another program closes the folder to its owner meanwhile.
        set(&sub, 0o600);
        let error = super::super::said(&undo.apply(&ctx).unwrap_err());
        set(&sub, 0o755);
        assert!(
            error.contains("kept.txt") && error.contains("could not be put back"),
            "{error}"
        );
        assert!(!error.contains("gone.txt"), "{error}");
        assert_eq!(stat_mode(&first), 0o600, "the rest is put back");
    }

    /// Finding #4: a file given to another group keeps the set-ID bits
    /// the system cleared, as chgrp leaves them; undone, all comes back.
    #[test]
    #[cfg(target_os = "linux")]
    fn a_group_given_leaves_the_set_id_bits_the_system_cleared() {
        use std::os::unix::fs::MetadataExt;
        let me = sys::me().unwrap();
        let tmp = Scratch::new();
        let tool = tmp.path().join("it's a tool");
        std::fs::write(&tool, b"#!/bin/sh\n").unwrap();
        let mine = std::fs::metadata(&tool).unwrap().gid();
        let Some(other) = me.groups.iter().copied().find(|g| *g != mine) else {
            return; // A user of one group: no other to give it to.
        };
        set(&tool, 0o2755);
        let ctx = StepContext::detached();
        let regroup = SetAccess {
            entry: EntryRef::from_path(&tool).unwrap(),
            mode: None,
            group: Some(other),
            inside: false,
        };
        let undo = apply(&regroup).unwrap();
        let now = std::fs::metadata(&tool).unwrap();
        assert_eq!(now.gid(), other);
        assert_eq!(
            now.mode() & 0o7777,
            0o755,
            "the set-group-ID bit stays cleared"
        );
        undo.apply(&ctx).unwrap();
        let back = std::fs::metadata(&tool).unwrap();
        assert_eq!((back.gid(), back.mode() & 0o7777), (mine, 0o2755));
        // A mode asked with the group is applied to what the system left.
        let both = SetAccess {
            mode: Some("g+s".into()),
            ..regroup
        };
        apply(&both).unwrap();
        assert_eq!(std::fs::metadata(&tool).unwrap().mode() & 0o7777, 0o2755);
    }

    #[test]
    #[cfg(unix)]
    fn a_file_of_the_trash_or_of_someone_else_is_not_changed() {
        let tmp = Scratch::new();
        let ctx = StepContext::detached();
        // A folder merely named so is no trash.
        let named = tmp.path().join("Trash");
        std::fs::create_dir_all(named.join("files")).unwrap();
        std::fs::write(named.join("files/mine.txt"), b"x").unwrap();
        assert!(matches!(
            Effect::check(&step(&named.join("files/mine.txt"), "u+x", false), &ctx),
            StepStatus::Pending { .. }
        ));
        assert!(!is_a_trash(&named));
        let trash = tmp.path().join(".Trash-1000");
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
            Effect::check(&step(&item, "u+x", false), &ctx),
            StepStatus::Conflict { reason } if reason.contains("in the trash")
        ));
        // A file of root's, for a user who is not root.
        if !rustix::process::geteuid().is_root() {
            let theirs = Path::new("/");
            let status = Effect::check(&step(theirs, "o+w", false), &ctx);
            assert!(
                matches!(&status, StepStatus::Conflict { reason } if reason.contains("only its owner")),
                "{status:?}"
            );
        }
    }

    /// The order things are put back in: a folder that its owner can look
    /// into once put back before what it holds, any other after.
    #[test]
    fn put_back_reaches_inside_before_closing() {
        let was = |path: &str, mode: u32| AccessWas {
            entry: EntryRef::new(FileId::new(1, 1), path),
            mode,
            group: None,
            follow: false,
        };
        let items = [
            was("/d", 0o700),
            was("/d/closed", 0o600),
            was("/d/closed/f", 0o600),
            was("/d/open", 0o755),
            was("/d/open/g", 0o644),
            was("/d/open b", 0o755),
        ];
        let order: Vec<&str> = back_order(&items)
            .into_iter()
            .map(|i| items[i].entry.path.to_str().unwrap())
            .collect();
        assert_eq!(
            order,
            [
                "/d",
                "/d/closed/f",
                "/d/closed",
                "/d/open",
                "/d/open/g",
                "/d/open b"
            ]
        );
    }
}
