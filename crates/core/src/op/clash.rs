//! A name already taken where a copy, a move or a link goes (ADR-054), or
//! where a step makes something new (an archive extracted or made,
//! ADR-059): found by the dry run before anything is done, answered once,
//! and the answers made the plan ([`settle`]): a copy, a move, a link or
//! what is made under a new name, a Trash step before what replaces, the
//! steps of a merge, an `Rmdir` for each folder a move-merge emptied. What
//! is never asked is settled the same way: a copy or a link into its own
//! folder, two items of one name, an item already there whole, a link
//! already made, what is made there already.

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use super::{
    Change, Copy, EntryRef, Link, Made, Move, Operation, Rmdir, StepContext, StepStatus, Trash,
    builtin,
};

/// What becomes of an item whose name a file takes, or of a file whose
/// name a folder takes.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum FileChoice {
    /// Both: what comes is named `name (copy).ext`, a link `name
    /// (link).ext` ([`copy_name`]).
    #[default]
    KeepBoth,
    /// What was there goes to the Trash, its companions with it.
    Replace,
    /// Replaced when what comes changed later than what is there, else
    /// skipped.
    ReplaceIfNewer,
    /// It stays where it is.
    Skip,
}

/// What becomes of a folder whose name a folder takes.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum FolderChoice {
    /// Into the folder there: what is not there goes whole, a folder that
    /// is there is merged in turn, a file that is there follows the files'
    /// choice.
    #[default]
    Merge,
    KeepBoth,
    Skip,
}

/// What becomes of one item.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Choice {
    KeepBoth,
    Replace,
    ReplaceIfNewer,
    Skip,
    Merge,
}

impl From<FileChoice> for Choice {
    fn from(choice: FileChoice) -> Self {
        match choice {
            FileChoice::KeepBoth => Choice::KeepBoth,
            FileChoice::Replace => Choice::Replace,
            FileChoice::ReplaceIfNewer => Choice::ReplaceIfNewer,
            FileChoice::Skip => Choice::Skip,
        }
    }
}

impl From<FolderChoice> for Choice {
    fn from(choice: FolderChoice) -> Self {
        match choice {
            FolderChoice::Merge => Choice::Merge,
            FolderChoice::KeepBoth => Choice::KeepBoth,
            FolderChoice::Skip => Choice::Skip,
        }
    }
}

/// What a plan does with every name taken: said beforehand by an agent or
/// a script (`on_conflict`), or the user's choice for all of a kind.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
pub struct Policy {
    /// For a file, and for a file and a folder of one name.
    #[serde(default)]
    pub files: FileChoice,
    /// For a folder onto a folder.
    #[serde(default)]
    pub folders: FolderChoice,
}

/// The answer to a plan's clashes: for all of a kind, and for some one by
/// one, by their key ([`Clash::key`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Answer {
    pub policy: Policy,
    pub each: HashMap<(usize, PathBuf), Choice>,
}

impl Answer {
    /// The same choice for all of a kind.
    pub fn all(policy: Policy) -> Self {
        Answer {
            policy,
            each: HashMap::new(),
        }
    }

    /// What becomes of `clash`: its own choice, else its kind's.
    pub fn choice(&self, clash: &Clash) -> Choice {
        self.each.get(&clash.key()).copied().unwrap_or_else(|| {
            if clash.folders() {
                self.policy.folders.into()
            } else {
                self.policy.files.into()
            }
        })
    }
}

/// One side of a clash: what comes, or what is there. A link is what it
/// is, not what it leads to: a link to a folder is never merged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Side {
    pub path: PathBuf,
    pub folder: bool,
    /// A file's size; a folder's is not counted, and what a step makes
    /// has one only where the step knows it beforehand (`Made::size`).
    pub size: Option<u64>,
    pub modified: Option<SystemTime>,
}

impl Side {
    fn of(path: &Path) -> Self {
        let meta = std::fs::symlink_metadata(path).ok();
        let folder = meta.as_ref().is_some_and(std::fs::Metadata::is_dir);
        Side {
            path: path.to_path_buf(),
            folder,
            size: meta
                .as_ref()
                .filter(|_| !folder)
                .map(std::fs::Metadata::len),
            modified: meta.and_then(|m| m.modified().ok()),
        }
    }
}

/// What a plan does with an item whose name is taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Copy,
    Move,
    Link,
    /// Makes something new there ([`Made`]): the verb on the question's
    /// button and what nothing is until it is answered ("Extract",
    /// "extracted").
    Make {
        verb: &'static str,
        done: &'static str,
    },
}

/// A name taken where an item goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Clash {
    /// The plan's step it is in: a merge's are in its folder's.
    pub step: usize,
    /// The clash of the folder it is in, merged: its index in the list.
    pub inside: Option<usize>,
    /// Copied, moved or linked.
    pub act: Act,
    /// What comes: for a link, what it leads to.
    pub source: Side,
    pub there: Side,
}

impl Clash {
    /// What an answer knows it by: its step and the path taken.
    pub fn key(&self) -> (usize, PathBuf) {
        (self.step, self.there.path.clone())
    }

    /// A folder onto a folder: merged, both kept, or skipped. Any other
    /// takes a file's choices, and so do a link to a folder (a link is
    /// what it is, never merged) and what a step makes (new, never merged
    /// into what is there).
    pub fn folders(&self) -> bool {
        matches!(self.act, Act::Copy | Act::Move) && self.source.folder && self.there.folder
    }

    /// What it says where no one is asked: the dry run's words.
    pub fn said(&self) -> String {
        format!(
            "“{}” is already there",
            self.there
                .path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
        )
    }
}

/// A plan's steps with its clashes settled.
pub struct Settled {
    pub steps: Vec<Box<dyn Operation>>,
    /// The names taken that were met, in plan order, a merged folder's
    /// after it.
    pub clashes: Vec<Clash>,
    /// Whether the steps differ from the plan's.
    pub changed: bool,
    /// The plan's items that do something (not skipped), the first one by
    /// its own step where it has one (under its new name): `None` for none.
    pub items: Option<super::Items>,
}

/// `steps` with every name taken settled as `answer` says, and what is
/// never asked as ADR-054 says; and the clashes met on the way. With
/// [`Answer::default`], the clashes a dialog lists: what the defaults
/// meet (a folder merged, its own clashes after it). Reads the disk: on
/// the plan's thread.
pub fn settle(steps: &[Box<dyn Operation>], answer: &Answer, ctx: &StepContext) -> Settled {
    let mut walk = Walk {
        ctx,
        answer,
        out: Vec::with_capacity(steps.len()),
        clashes: Vec::new(),
        changed: false,
    };
    let (mut count, mut first) = (0, None);
    for (i, step) in steps.iter().enumerate() {
        let asked = step.describe();
        let before = walk.out.len();
        match (Transfer::of(step.as_ref()), step.makes(ctx)) {
            (Some(transfer), _) => walk.place(i, transfer, None),
            (None, Some(made)) => walk.make(i, step.as_ref(), &made),
            (None, None) => walk.push(step.clone_box()),
        }
        let done = &walk.out[before..];
        if done.is_empty() {
            continue;
        }
        count += 1;
        if first.is_none() {
            let from = match &asked {
                Change::Copy { from, .. }
                | Change::Move { from, .. }
                | Change::Link { from, .. } => Some(from.clone()),
                _ => None,
            };
            let made = matches!(asked, Change::Create { .. });
            let own = done.iter().map(|s| s.describe()).find(|c| {
                matches!(c, Change::Copy { from: f, .. } | Change::Move { from: f, .. }
                    | Change::Link { from: f, .. } if Some(f) == from.as_ref())
                    || made && matches!(c, Change::Create { .. })
            });
            first = Some(own.unwrap_or(asked));
        }
    }
    Settled {
        steps: walk.out,
        clashes: walk.clashes,
        changed: walk.changed,
        items: first.map(|first| super::Items { first, count }),
    }
}

/// Whether a step of `steps` copies, moves, links or makes something
/// under a name: a plan with none has no name to settle. May read what a
/// step makes of (`Operation::makes`): on the plan's thread.
pub fn takes_names(steps: &[Box<dyn Operation>], ctx: &StepContext) -> bool {
    steps
        .iter()
        .any(|step| Transfer::of(step.as_ref()).is_some() || step.makes(ctx).is_some())
}

/// A copy, a move or a link, as the walk takes it.
#[derive(Clone)]
enum Transfer {
    Copy(Copy),
    Move(Move),
    Link(Link),
}

impl Transfer {
    fn of(step: &dyn Operation) -> Option<Self> {
        let any = step.as_any()?;
        any.downcast_ref::<Copy>()
            .cloned()
            .map(Transfer::Copy)
            .or_else(|| any.downcast_ref::<Move>().cloned().map(Transfer::Move))
            .or_else(|| any.downcast_ref::<Link>().cloned().map(Transfer::Link))
    }

    fn entry(&self) -> &EntryRef {
        match self {
            Transfer::Copy(c) => &c.entry,
            Transfer::Move(m) => &m.entry,
            Transfer::Link(l) => &l.entry,
        }
    }

    fn act(&self) -> Act {
        match self {
            Transfer::Copy(_) => Act::Copy,
            Transfer::Move(_) => Act::Move,
            Transfer::Link(_) => Act::Link,
        }
    }

    fn moving(&self) -> bool {
        matches!(self, Transfer::Move(_))
    }

    fn target(&self, src: &Path) -> PathBuf {
        match self {
            Transfer::Copy(c) => c.target(src),
            Transfer::Move(m) => m.target(src),
            Transfer::Link(l) => l.target(src),
        }
    }

    /// The same, under `name`.
    fn named(self, name: OsString) -> Self {
        match self {
            Transfer::Copy(c) => Transfer::Copy(Copy {
                new_name: Some(name),
                ..c
            }),
            Transfer::Move(m) => Transfer::Move(Move {
                new_name: Some(name),
                ..m
            }),
            Transfer::Link(l) => Transfer::Link(Link {
                new_name: Some(name),
                ..l
            }),
        }
    }

    /// The same done to `entry`, into `dest_dir` under its name: an item of
    /// a folder merged.
    fn child(&self, entry: EntryRef, dest_dir: PathBuf) -> Self {
        match self {
            Transfer::Copy(c) => Transfer::Copy(Copy {
                entry,
                dest_dir,
                new_name: None,
                preserve_attributes: c.preserve_attributes,
            }),
            Transfer::Move(_) => Transfer::Move(Move::new(entry, dest_dir)),
            Transfer::Link(_) => Transfer::Link(Link::new(entry, dest_dir)),
        }
    }

    fn boxed(self) -> Box<dyn Operation> {
        match self {
            Transfer::Copy(c) => Box::new(c),
            Transfer::Move(m) => Box::new(m),
            Transfer::Link(l) => Box::new(l),
        }
    }
}

struct Walk<'a> {
    ctx: &'a StepContext,
    answer: &'a Answer,
    out: Vec<Box<dyn Operation>>,
    clashes: Vec<Clash>,
    changed: bool,
}

impl Walk<'_> {
    /// A step of the settled plan: the steps after it find what it does.
    fn push(&mut self, step: Box<dyn Operation>) {
        self.ctx.foresee(&step.describe());
        self.out.push(step);
    }

    /// A step the plan had, kept as it was or changed.
    fn keep(&mut self, transfer: Transfer, changed: bool) {
        self.changed |= changed;
        self.push(transfer.boxed());
    }

    fn place(&mut self, step: usize, transfer: Transfer, inside: Option<usize>) {
        let child = inside.is_some();
        // What cannot be resolved, or leaves the trash (a restore, with
        // rules of its own), is the dry run's to judge.
        let Ok(src) = transfer.entry().resolve() else {
            return self.keep(transfer, child);
        };
        if transfer.moving() && builtin::in_trash(&src).is_some() {
            return self.keep(transfer, child);
        }
        let target = transfer.target(&src);
        if let Transfer::Move(m) = &transfer
            && m.in_place(&src)
        {
            return self.keep(transfer, child);
        }
        let linking = transfer.act() == Act::Link;
        // Onto itself (a copy or a link into its own folder), or onto what
        // an earlier step puts there (two items of one name): both kept.
        if target == src || self.ctx.made(&target) {
            return self.keep_both(transfer, &src, &target);
        }
        if !self.ctx.exists(&target) || !linking && src.is_dir() && target.starts_with(&src) {
            return self.keep(transfer, child);
        }
        // There already: the link made, or the item whole (the same kinds,
        // sizes and times all the way down). A copy or a link has nothing
        // to do, and a move leaves what was moved in the trash.
        let there = if linking {
            std::fs::read_link(&target).is_ok_and(|says| says == src)
        } else {
            builtin::same_tree(&src, &target, self.ctx).unwrap_or(false)
        };
        if there {
            if transfer.moving() {
                self.changed = true;
                return self.push(Box::new(Trash {
                    entry: transfer.entry().clone(),
                }));
            }
            return self.keep(transfer, child);
        }
        let clash = Clash {
            step,
            inside,
            act: transfer.act(),
            source: Side::of(&src),
            there: Side::of(&target),
        };
        let choice = self.answer.choice(&clash);
        let index = self.clashes.len();
        let newer = matches!((clash.source.modified, clash.there.modified),
            (Some(source), Some(there)) if source > there);
        let folders = clash.folders();
        self.clashes.push(clash);
        self.changed = true;
        match choice {
            Choice::Skip => {}
            Choice::ReplaceIfNewer if !newer => {}
            Choice::Replace | Choice::ReplaceIfNewer => self.replace(transfer, &src, &target),
            Choice::Merge if folders => self.merge(step, &transfer, &src, &target, index),
            Choice::Merge | Choice::KeepBoth => self.keep_both(transfer, &src, &target),
        }
    }

    fn keep_both(&mut self, transfer: Transfer, src: &Path, target: &Path) {
        let dir = target.parent().unwrap_or(Path::new("/"));
        let folder = std::fs::symlink_metadata(src).is_ok_and(|m| m.is_dir());
        let word = if transfer.act() == Act::Link {
            "link"
        } else {
            "copy"
        };
        let name = kept_name(target.file_name().unwrap_or_default(), word, folder, |n| {
            self.ctx.exists(&dir.join(n))
        });
        self.keep(transfer.named(name), true);
    }

    /// What `op` makes ([`Made`]), its name settled: fixed in the step
    /// where the step found it (an archive's one member), made beside
    /// what an earlier step makes there (two archives of one name
    /// extracted into one folder), or as the answer says where something
    /// is there, with a file's choices. Made there already, it is done.
    /// What is there is never put in the trash when it is, or holds, what
    /// the step reads (`Made::inputs`): the step's check says so instead.
    fn make(&mut self, step: usize, op: &dyn Operation, made: &Made) {
        let target = &made.target;
        let named = target
            .file_name()
            .and_then(|name| op.made_as(name.to_owned()))
            .filter(|named| named.args() != op.args());
        self.changed |= named.is_some();
        let op = named.as_deref().unwrap_or(op);
        let earlier = self.ctx.made(target);
        let status = (earlier || self.ctx.exists(target)).then(|| op.check(self.ctx));
        if earlier {
            return self.make_beside(op, made);
        }
        if status.is_none_or(|status| status == StepStatus::AlreadyDone) {
            return self.push(op.clone_box());
        }
        let (verb, done) = made.words;
        // What comes is what the step makes, not what it reads (the
        // archive extracted, the first file compressed): a folder or a
        // file, of the size the step foresees if it does, as new as the
        // latest change among what it reads, a file deep in a folder as
        // well as the folder.
        let clash = Clash {
            step,
            inside: None,
            act: Act::Make { verb, done },
            source: Side {
                path: made.from.clone(),
                folder: made.folder,
                size: made.size,
                modified: newest(&made.inputs, self.ctx),
            },
            there: Side::of(target),
        };
        let choice = self.answer.choice(&clash);
        let newer = matches!((clash.source.modified, clash.there.modified),
            (Some(source), Some(there)) if source > there);
        self.clashes.push(clash);
        self.changed = true;
        match choice {
            Choice::Skip => {}
            Choice::ReplaceIfNewer if !newer => {}
            Choice::Replace | Choice::ReplaceIfNewer => {
                let holds_input = made.inputs.iter().any(|input| input.starts_with(target));
                if !holds_input && let Some(there) = EntryRef::from_path(target) {
                    self.push(Box::new(Trash { entry: there }));
                }
                self.push(op.clone_box());
            }
            Choice::Merge | Choice::KeepBoth => self.make_beside(op, made),
        }
    }

    /// What `op` makes, under the first name free beside its own:
    /// `photos (2).zip`.
    fn make_beside(&mut self, op: &dyn Operation, made: &Made) {
        let dir = made.target.parent().unwrap_or(Path::new("/"));
        let name = kept_name(
            made.target.file_name().unwrap_or_default(),
            "",
            made.folder,
            |n| self.ctx.exists(&dir.join(n)),
        );
        self.changed = true;
        self.push(op.made_as(name).unwrap_or_else(|| op.clone_box()));
    }

    /// What is there to the Trash, then the item in its place. Not a
    /// folder holding what comes: the dry run says its name is taken.
    fn replace(&mut self, transfer: Transfer, src: &Path, target: &Path) {
        if !src.starts_with(target)
            && let Some(there) = EntryRef::from_path(target)
        {
            self.push(Box::new(Trash { entry: there }));
        }
        self.keep(transfer, true);
    }

    /// Each item of the folder `src` into the folder `target`, as the
    /// answer says of each; then, for a move that took everything out of
    /// `src`, `src` removed. A companion goes with its file.
    fn merge(&mut self, step: usize, transfer: &Transfer, src: &Path, target: &Path, index: usize) {
        let Ok(read) = std::fs::read_dir(src) else {
            return self.keep(transfer.clone(), true);
        };
        let mut names: Vec<OsString> = read.flatten().map(|e| e.file_name()).collect();
        names.sort_unstable();
        let rules = self.ctx.companions();
        for name in names {
            let owned = rules
                .owner(&name)
                .is_some_and(|owner| std::fs::symlink_metadata(src.join(owner)).is_ok());
            if owned {
                continue;
            }
            let Some(entry) = EntryRef::from_path(src.join(&name)) else {
                continue;
            };
            self.place(
                step,
                transfer.child(entry, target.to_path_buf()),
                Some(index),
            );
        }
        if transfer.moving() && self.ctx.empty_after(src).unwrap_or(false) {
            self.push(Box::new(Rmdir {
                path: src.to_path_buf(),
            }));
        }
    }
}

/// When anything among `paths`, or held in them, last changed: a link's
/// own time, never followed. What Replace if newer compares a step's
/// inputs with (`Made::inputs`). Reads them all: on the plan's thread.
fn newest(paths: &[PathBuf], ctx: &StepContext) -> Option<SystemTime> {
    let mut newest = None;
    let mut left = paths.to_vec();
    while let Some(path) = left.pop() {
        if ctx.cancel_token().is_cancelled() {
            break;
        }
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        newest = newest.max(meta.modified().ok());
        if meta.is_dir()
            && let Ok(read) = std::fs::read_dir(&path)
        {
            left.extend(read.flatten().map(|entry| entry.path()));
        }
    }
    newest
}

/// The longest name a folder takes, in bytes.
const NAME_MAX: usize = 255;

/// The name GNOME gives a copy kept beside `name`: `name (copy).ext`, then
/// `name (copy 2).ext`, `(copy 3)`… the first that `taken` does not hold.
/// A name that already ends so counts on (`a (copy).txt` gives
/// `a (copy 2).txt`). The extension is the last one (`.tar.gz`, `.json.gz`
/// whole), short and without a space; a folder's name and a dotfile's have
/// none. Too long for a folder, the name before the extension is cut. The
/// words are not translated: a file's name does not change with the
/// window's language.
pub fn copy_name(name: &OsStr, folder: bool, taken: impl Fn(&OsStr) -> bool) -> OsString {
    kept_name(name, "copy", folder, taken)
}

/// [`copy_name`] with another word: `name (link).ext` for a link kept
/// beside `name`. With none, a number from 2: `name (2).ext` for what a
/// step makes beside `name` (an archive, what one holds).
pub fn kept_name(
    name: &OsStr,
    word: &str,
    folder: bool,
    taken: impl Fn(&OsStr) -> bool,
) -> OsString {
    let bytes = name.as_encoded_bytes();
    let split = if folder {
        bytes.len()
    } else {
        extension_at(bytes)
    };
    let (stem, extension) = bytes.split_at(split);
    let (base, mut n) = if word.is_empty() {
        let (base, n) = numbered(stem);
        (base, n.max(1))
    } else {
        counted(stem, word)
    };
    loop {
        n += 1;
        let suffix = if word.is_empty() {
            format!(" ({n})")
        } else if n == 1 {
            format!(" ({word})")
        } else {
            format!(" ({word} {n})")
        };
        let room = NAME_MAX.saturating_sub(suffix.len() + extension.len());
        let mut candidate = cut(base, room).to_vec();
        candidate.extend_from_slice(suffix.as_bytes());
        candidate.extend_from_slice(extension);
        let candidate = crate::os_path::from_bytes(candidate);
        if !taken(&candidate) {
            return candidate;
        }
    }
}

/// The extensions of what a compressor made of a file, whose own
/// extension goes with theirs (`.tar.gz`).
const SQUEEZED: [&[u8]; 9] = [
    b".gz", b".bz2", b".xz", b".zst", b".Z", b".lz", b".lzma", b".lz4", b".br",
];

/// Where the extension of a file's name begins: its length when it has
/// none.
fn extension_at(name: &[u8]) -> usize {
    let short = |ext: &[u8]| ext.len() <= 10 && !ext.contains(&b' ');
    let Some(dot) = name.iter().rposition(|&b| b == b'.').filter(|&d| d > 0) else {
        return name.len();
    };
    if !short(&name[dot..]) {
        return name.len();
    }
    if SQUEEZED.contains(&&name[dot..])
        && let Some(inner) = name[..dot]
            .iter()
            .rposition(|&b| b == b'.')
            .filter(|&d| d > 0 && short(&name[d..dot]))
    {
        return inner;
    }
    dot
}

/// `stem` without a copy's ending (with `word` "copy"), and the copy it
/// says it is: 0 for none, 1 for ` (copy)`, n for ` (copy n)`.
fn counted<'a>(stem: &'a [u8], word: &str) -> (&'a [u8], u32) {
    if let Some(base) = stem.strip_suffix(format!(" ({word})").as_bytes()) {
        return (base, 1);
    }
    let numbered = stem.strip_suffix(b")").and_then(|rest| {
        let at = rest.iter().rposition(|&b| b == b' ')?;
        let n: u32 = std::str::from_utf8(&rest[at + 1..]).ok()?.parse().ok()?;
        let base = rest[..at].strip_suffix(format!(" ({word}").as_bytes())?;
        (n >= 2 && !rest[at + 1..].starts_with(b"0")).then_some((base, n))
    });
    numbered.unwrap_or((stem, 0))
}

/// `stem` without a number kept beside a name (` (2)`), and that number:
/// 0 for none.
fn numbered(stem: &[u8]) -> (&[u8], u32) {
    let counted = stem.strip_suffix(b")").and_then(|rest| {
        let at = rest.iter().rposition(|&b| b == b'(')?;
        let base = rest[..at].strip_suffix(b" ")?;
        let digits = &rest[at + 1..];
        let n: u32 = std::str::from_utf8(digits).ok()?.parse().ok()?;
        (n >= 2 && !digits.starts_with(b"0") && digits.iter().all(u8::is_ascii_digit))
            .then_some((base, n))
    });
    counted.unwrap_or((stem, 0))
}

/// The first `room` bytes of `base`, not cutting a character in two.
fn cut(base: &[u8], room: usize) -> &[u8] {
    if base.len() <= room {
        return base;
    }
    let mut end = room;
    // A UTF-8 continuation byte is no character's start.
    while end > 0 && base[end] & 0xC0 == 0x80 {
        end -= 1;
    }
    &base[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(name: &str, folder: bool, taken: &[&str]) -> String {
        copy_name(OsStr::new(name), folder, |n| {
            taken.iter().any(|t| OsStr::new(t) == n)
        })
        .to_string_lossy()
        .into_owned()
    }

    #[test]
    fn a_copy_is_named_as_gnome_names_it() {
        assert_eq!(named("notes.txt", false, &[]), "notes (copy).txt");
        assert_eq!(
            named("notes.txt", false, &["notes (copy).txt"]),
            "notes (copy 2).txt"
        );
        assert_eq!(
            named(
                "notes.txt",
                false,
                &["notes (copy).txt", "notes (copy 2).txt"]
            ),
            "notes (copy 3).txt"
        );
        assert_eq!(named("notes (copy).txt", false, &[]), "notes (copy 2).txt");
        assert_eq!(
            named("notes (copy 9).txt", false, &[]),
            "notes (copy 10).txt"
        );
        assert_eq!(
            named("it's (copy 07).txt", false, &[]),
            "it's (copy 07) (copy).txt",
            "not a count Skwir writes"
        );
        assert_eq!(named("backup.tar.gz", false, &[]), "backup (copy).tar.gz");
        assert_eq!(named("data.json.zst", false, &[]), "data (copy).json.zst");
        assert_eq!(named("archive.gz", false, &[]), "archive (copy).gz");
        assert_eq!(named(".bashrc", false, &[]), ".bashrc (copy)");
        assert_eq!(named(".config.json", false, &[]), ".config (copy).json");
        assert_eq!(named("v1.2 final", false, &[]), "v1.2 final (copy)");
        assert_eq!(named("Photos 2024.d", true, &[]), "Photos 2024.d (copy)");
        assert_eq!(
            named("été à Brest.jpg", false, &[]),
            "été à Brest (copy).jpg"
        );
        assert_eq!(named("README", false, &[]), "README (copy)");
    }

    #[test]
    fn a_link_kept_beside_is_named_so() {
        let link = |name: &str, folder: bool, taken: &[&str]| {
            kept_name(OsStr::new(name), "link", folder, |n| {
                taken.iter().any(|t| OsStr::new(t) == n)
            })
            .to_string_lossy()
            .into_owned()
        };
        assert_eq!(link("notes.txt", false, &[]), "notes (link).txt");
        assert_eq!(
            link("notes.txt", false, &["notes (link).txt"]),
            "notes (link 2).txt"
        );
        assert_eq!(link("notes (link).txt", false, &[]), "notes (link 2).txt");
        assert_eq!(
            link("notes (copy).txt", false, &[]),
            "notes (copy) (link).txt"
        );
        assert_eq!(link("Photos 2024.d", true, &[]), "Photos 2024.d (link)");
    }

    #[test]
    fn what_is_made_beside_a_name_is_numbered() {
        let made = |name: &str, folder: bool, taken: &[&str]| {
            kept_name(OsStr::new(name), "", folder, |n| {
                taken.iter().any(|t| OsStr::new(t) == n)
            })
            .to_string_lossy()
            .into_owned()
        };
        assert_eq!(made("photos.zip", false, &[]), "photos (2).zip");
        assert_eq!(
            made("photos.zip", false, &["photos (2).zip"]),
            "photos (3).zip"
        );
        assert_eq!(made("photos (2).zip", false, &[]), "photos (3).zip");
        assert_eq!(made("backup.tar.gz", false, &[]), "backup (2).tar.gz");
        assert_eq!(made("project-1.0", true, &[]), "project-1.0 (2)");
        assert_eq!(made("notes (copy).txt", false, &[]), "notes (copy) (2).txt");
        assert_eq!(made("v (02).txt", false, &[]), "v (02) (2).txt");
        assert_eq!(made("a(3).txt", false, &[]), "a(3) (2).txt");
    }

    #[test]
    fn a_copy_too_long_is_cut_before_its_extension_on_a_character() {
        // One byte, then two-byte characters: the cut falls inside one.
        let long = format!("a{}.txt", "é".repeat(124));
        assert_eq!(long.len(), 253);
        let copy = named(&long, false, &[]);
        assert!(copy.len() <= NAME_MAX, "{}", copy.len());
        assert!(copy.ends_with("é (copy).txt"), "{copy}");
    }

    #[test]
    fn a_name_not_utf8_keeps_its_bytes() {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let name = OsStr::from_bytes(b"caf\xe9.txt");
            let copy = copy_name(name, false, |_| false);
            assert_eq!(copy.as_bytes(), b"caf\xe9 (copy).txt");
        }
    }
}
