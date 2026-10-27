//! Compress (ADR-059): an archive made of entries, beside them, by the
//! libarchive helper (`archive::create`), which opens none of the user's
//! files: the step walks them and hands their bytes over. A step of a plan
//! as an extraction is: made in a staging folder and placed whole, Cancel
//! and Pause answered while the helper works, undone to the trash, a name
//! taken asked about as a copy's is.

use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use anyhow::Context;
use skwir_core::App;
use skwir_core::command::{Invocation, Outcome, Topic};
use skwir_core::entry::{EntryKind, FileId};
use skwir_core::fs::{Dir, stat, xattr};
use skwir_core::op::{
    self, Cancelled, Change, EntryRef, Made, Operation, Plan, StepContext, StepStatus, Unanswered,
    Words, clash,
};
use skwir_core::os_path::PathArg;
use skwir_core::registry::MenuItem;
use skwir_core::workspace::PaneId;
use skwir_platform::archive::{self, Format, Item, Part};

/// The step's kind in a plan.
const KIND: &str = "compress";

/// What the menus offer it for: files and folders, not a place, nor what
/// is in the trash.
const COMPRESSES: &str = "selection !place !restores";

/// The context menu's submenu of the ways to compress.
const COMPRESS: &str = "Compress";

/// Make an archive named `name` in `dest_dir` of `entries`, each at its
/// top under its own name, with what is beside it (its companions,
/// ADR-028); its format is what the name ends with ([`Format::of_name`]).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct Compress {
    pub entries: Vec<EntryRef>,
    #[serde(with = "skwir_core::os_path")]
    #[schemars(with = "PathArg")]
    pub dest_dir: PathBuf,
    #[serde(with = "skwir_core::os_path")]
    #[schemars(with = "PathArg")]
    pub name: OsString,
    /// Encrypted (a ZIP's AES-256) by a password the user gives as it is
    /// made: never part of the plan.
    #[serde(default)]
    pub password: bool,
}

impl Compress {
    fn target(&self) -> PathBuf {
        self.dest_dir.join(&self.name)
    }

    /// What it compresses, in words: “photos”, or 3 items.
    fn what(&self) -> String {
        match self.entries.as_slice() {
            [one] => format!("“{}”", display_name(&one.path)),
            many => format!("{} items", many.len()),
        }
    }

    /// The password it is encrypted by, asked of the user, twice to be
    /// sure of it.
    fn password(&self, ctx: &StepContext) -> anyhow::Result<String> {
        let archive = display_name(&self.target());
        let ask = |title: &str, accept: &str| {
            ctx.ask_secret(title, accept).map_err(|why| match why {
                Unanswered::Declined => anyhow::Error::from(Cancelled),
                Unanswered::NoOne => anyhow::anyhow!(
                    "“{archive}” would be encrypted: its password is asked in Skwir's window"
                ),
            })
        };
        let mut title = format!("A password for “{archive}”");
        loop {
            let first = ask(&title, "Next")?;
            if first.is_empty() {
                title = format!("A password for “{archive}”: it cannot be empty");
                continue;
            }
            if ask("The same password again, to be sure of it", "Compress")? == first {
                return Ok(first);
            }
            title = format!("The two passwords differed: a password for “{archive}”");
        }
    }
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

fn conflict(reason: impl Into<String>) -> StepStatus {
    StepStatus::Conflict {
        reason: reason.into(),
    }
}

/// The endings of the formats, in words: ".zip, .7z, … or .tar".
fn endings() -> String {
    let all: Vec<&str> = Format::ALL.iter().map(|f| f.extension()).collect();
    match all.split_last() {
        Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
        None => String::new(),
    }
}

/// The format the archive's name says.
fn format_of(name: &OsStr) -> anyhow::Result<Format> {
    Format::of_name(name)
        .map(|(format, _)| format)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "“{}” says no format: end it with {}",
                name.to_string_lossy(),
                endings()
            )
        })
}

impl op::Kind for Compress {
    const KIND: &'static str = KIND;
}

impl op::Effect for Compress {
    fn footprint(&self, _: &op::footprint::Volumes) -> Option<op::footprint::Footprint> {
        let mut footprint = op::footprint::Footprint::default()
            .made(&self.target())
            .heavy_if(true);
        for entry in &self.entries {
            footprint = footprint.read(&entry.path);
        }
        Some(footprint)
    }

    fn describe(&self) -> Change {
        let (what, archive) = (self.what(), display_name(&self.target()));
        Change::Create {
            path: self.target(),
            directory: false,
            words: Some(Words {
                will: format!("compress {what} as “{archive}”"),
                did: format!("compressed {what} as “{archive}”"),
            }),
        }
    }

    fn check(&self, ctx: &StepContext) -> StepStatus {
        if let Err(invalid) = op::valid_file_name(&self.name) {
            return conflict(invalid.to_string());
        }
        let format = match format_of(&self.name) {
            Ok(format) => format,
            Err(e) => return conflict(e.to_string()),
        };
        if self.password && !format.encrypts() {
            return conflict("Only a ZIP archive can have a password");
        }
        if self.entries.is_empty() {
            return conflict("Nothing to compress");
        }
        if !ctx.is_folder(&self.dest_dir) {
            return conflict(format!(
                "“{}” is not a folder",
                display_name(&self.dest_dir)
            ));
        }
        if let Some(Err(e)) = self
            .entries
            .iter()
            .map(EntryRef::resolve)
            .find(Result::is_err)
        {
            return conflict(e.to_string());
        }
        let target = self.target();
        // As the plan's earlier steps leave it: one may put what is there
        // in the trash (Replace).
        if !ctx.exists(&target) {
            return StepStatus::Pending { reversible: true };
        }
        // Applied already, or another file under the name.
        if holds_compressed(self, format, &target, ctx) {
            StepStatus::AlreadyDone
        } else if let Some(input) = self.entries.iter().find(|e| e.path.starts_with(&target)) {
            // Never replaced: it would take what goes in with it.
            conflict(if input.path == target {
                format!("“{}” is among what it compresses", display_name(&target))
            } else {
                format!(
                    "“{}” holds “{}”, which it compresses",
                    display_name(&target),
                    display_name(&input.path)
                )
            })
        } else {
            conflict(format!("“{}” is already there", display_name(&target)))
        }
    }

    fn makes(&self, _: &StepContext) -> Option<Made> {
        Some(Made {
            target: self.target(),
            from: self
                .entries
                .first()
                .map(|e| e.path.clone())
                .unwrap_or_default(),
            inputs: self.entries.iter().map(|e| e.path.clone()).collect(),
            folder: false,
            // Not known until it is written.
            size: None,
            words: ("Compress", "compressed"),
        })
    }

    /// What goes in, as stored whole (a tar's other names once): what the
    /// archive takes at most, compressed or not; a 7z twice, written whole
    /// to a temporary file beside it first.
    fn writes(&self, ctx: &StepContext) -> Option<(PathBuf, u64)> {
        let format = format_of(&self.name).ok()?;
        let mut once = HashSet::new();
        let mut bytes = 0u64;
        let mut left: Vec<PathBuf> = self
            .entries
            .iter()
            .filter_map(|e| e.resolve().ok())
            .collect();
        while let Some(path) = left.pop() {
            if ctx.cancel_token().is_cancelled() {
                return None;
            }
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if meta.is_dir() {
                if let Ok(read) = std::fs::read_dir(&path) {
                    left.extend(read.flatten().map(|entry| entry.path()));
                }
            } else if meta.is_file()
                && (!format.keeps_hard_links()
                    || stat::identity(&path, &meta).is_none_or(|id| once.insert(id)))
            {
                bytes = bytes.saturating_add(meta.len());
            }
        }
        if format == Format::SevenZip {
            bytes = bytes.saturating_mul(2);
        }
        Some((self.dest_dir.clone(), bytes))
    }

    fn made_as(&self, name: OsString) -> Option<Box<dyn Operation>> {
        Some(Box::new(Compress {
            name,
            ..self.clone()
        }))
    }

    fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let format = format_of(&self.name)?;
        let parts = walk(&self.entries, format, ctx)?;
        let total: u64 = parts
            .iter()
            .map(|part| match part.item {
                Item::File { size, .. } => size,
                _ => 0,
            })
            .sum();
        ctx.add_total(total);
        let passphrase = if self.password {
            Some(self.password(ctx)?)
        } else {
            None
        };
        let target = self.target();
        let staging = ctx.stage(&self.dest_dir)?;
        let made = staging.path().join("archive");
        let mut done = 0;
        let result = archive::create(
            &parts,
            format,
            passphrase.as_deref(),
            &made,
            ctx.cancel_token(),
            |sent| {
                ctx.advance(sent);
                done += sent;
            },
            || ctx.is_paused(),
        )
        // Paused once it is all there, it waits before it is placed.
        .and_then(|()| Ok(ctx.check_cancelled()?))
        .and_then(|()| {
            staging.place(&made, &target).with_context(|| {
                format!(
                    "“{}” could not be placed in “{}”",
                    display_name(&target),
                    display_name(&self.dest_dir)
                )
            })
        });
        // Whatever happened, the staging folder goes now, with what it
        // holds. Should it stay, a later run sweeps it.
        if let Err(e) = staging.remove() {
            tracing::warn!("a staging folder stays: {e}");
        }
        if ctx.cancel_token().is_cancelled() && result.is_err() {
            return Err(Cancelled.into());
        }
        result?;
        ctx.advance(total.saturating_sub(done));
        let made = EntryRef::from_path(&target)
            .with_context(|| format!("“{}” went missing as it was made", display_name(&target)))?;
        // Where its volume keeps no trash, Undo removes it while it is as
        // made, and Redo makes it again.
        if op::keeps_no_trash(&target) {
            return Ok(Some(Box::new(crate::unmake::Unmake::of(
                made,
                crate::unmake::Again::Compress(self.clone()),
            ))));
        }
        // Undone to the trash, as a copy is: it may have been changed since.
        Ok(Some(Box::new(op::Trash { entry: made })))
    }
}

/// Whether the archive at `target` holds what `step` would put in it, and
/// nothing else, as a copy is told done (`same_tree`): each part under its
/// name, a folder as a folder, a file of its size and time, a link saying
/// the same, a second name of the same first. Times are compared to the
/// second, the format's (a tar's and a ZIP's extended time keep seconds);
/// a ZIP without its extended time keeps a local time to two seconds,
/// read in this machine's zone: one made elsewhere is not told done. A
/// listing cut short (`archive::list`'s bounds) says no.
fn holds_compressed(step: &Compress, format: Format, target: &Path, ctx: &StepContext) -> bool {
    let Ok(listing) = archive::list(target, ctx.cancel_token()) else {
        return false;
    };
    if !listing.complete {
        return false;
    }
    let Ok(parts) = walk(&step.entries, format, ctx) else {
        return false;
    };
    let listed: HashMap<&str, &archive::Member> = listing
        .members
        .iter()
        .map(|m| (m.name.trim_end_matches('/'), m))
        .collect();
    let lossy = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
    parts.len() == listed.len()
        && parts.iter().all(|part| {
            let Some(member) = listed.get(lossy(&part.name).as_str()) else {
                return false;
            };
            match &part.item {
                Item::Folder => member.directory,
                Item::File { size, .. } => {
                    member.regular
                        && member.hardlink.is_none()
                        && u64::try_from(member.size).is_ok_and(|s| s == *size)
                        && same_second(part.modified, member.mtime, format)
                }
                Item::Link { target } => member.link.as_deref() == Some(lossy(target).as_str()),
                Item::SecondName { first } => {
                    member.hardlink.as_deref() == Some(lossy(first).as_str())
                }
            }
        })
}

/// Whether a file's time `walked` is the one an archive of `format` keeps
/// as `listed` (seconds, nanoseconds): the same second, or for a ZIP, the
/// same two seconds (MS-DOS's, where the extended time is missing).
fn same_second(
    walked: Option<std::time::SystemTime>,
    listed: Option<[i64; 2]>,
    format: Format,
) -> bool {
    let (Some(walked), Some([listed, _])) = (walked, listed) else {
        return walked.is_none() && listed.is_none();
    };
    // Whole seconds, rounded down, as the archive's header was written.
    let walked = match walked.duration_since(std::time::UNIX_EPOCH) {
        Ok(after) => after.as_secs().cast_signed(),
        Err(before) => {
            let before = before.duration();
            -before.as_secs().cast_signed() - i64::from(before.subsec_nanos() > 0)
        }
    };
    let apart = walked - listed;
    apart == 0 || format == Format::Zip && apart == 1
}

/// What goes into the archive, in order (a folder before what it holds):
/// each entry at the top under its name, with the files beside it (its
/// companions, at their places from its folder: `.ts/x.json`), folders
/// whole. A link is kept as a link; a file's other names, where the format
/// keeps them, as links to the first; a pipe, a socket or a device is
/// left out, never opened. The attributes a copy keeps go where the format
/// keeps them. Two entries of one name are both kept, the second
/// numbered (`notes (2).txt`). Reads the disk.
fn walk(entries: &[EntryRef], format: Format, ctx: &StepContext) -> anyhow::Result<Vec<Part>> {
    let rules = ctx.companions();
    // Each one at the top, by its path and its name there.
    let mut tops: Vec<(PathBuf, OsString)> = Vec::new();
    for entry in entries {
        let path = entry.resolve()?;
        let Some(name) = path.file_name().map(OsStr::to_os_string) else {
            continue;
        };
        if !tops.iter().any(|(p, _)| *p == path) {
            tops.push((path.clone(), name));
        }
        let folder = path.parent().unwrap_or(Path::new(""));
        for found in rules.existing(&path) {
            let Ok(beside) = found.path.strip_prefix(folder) else {
                continue;
            };
            if !tops.iter().any(|(p, _)| *p == found.path) {
                tops.push((found.path.clone(), beside.as_os_str().to_os_string()));
            }
        }
    }
    let mut parts = Vec::new();
    let mut firsts: HashMap<FileId, Vec<u8>> = HashMap::new();
    let mut taken: HashSet<OsString> = HashSet::new();
    for (path, name) in tops {
        let (Some(parent), Some(file)) = (path.parent(), path.file_name()) else {
            continue;
        };
        let dir = Dir::open(parent)
            .with_context(|| format!("“{}” could not be read", display_name(parent)))?;
        let own = stat::own(&dir, file)
            .with_context(|| format!("“{}” is no longer there", display_name(&path)))?;
        let name = if taken.contains(&name) {
            let folder = own.kind == EntryKind::Directory;
            clash::kept_name(&name, "", folder, |n| taken.contains(n))
        } else {
            name
        };
        taken.insert(name.clone());
        add(
            &mut parts,
            (path.clone(), member_name(&name), own),
            format,
            &mut firsts,
            ctx,
        )?;
    }
    Ok(parts)
}

/// A name's bytes in an archive: `/` between its folders, whatever the
/// system's separator.
fn member_name(name: &OsStr) -> Vec<u8> {
    let bytes = name.as_encoded_bytes();
    if std::path::MAIN_SEPARATOR == '/' {
        bytes.to_vec()
    } else {
        Path::new(name)
            .components()
            .map(|c| c.as_os_str().as_encoded_bytes().to_vec())
            .collect::<Vec<_>>()
            .join(&b'/')
    }
}

/// `top` (its path, its name in the archive, what it is) and, for a
/// folder, what it holds, as parts of the archive: a folder's before its
/// own, sorted by name.
fn add(
    parts: &mut Vec<Part>,
    top: (PathBuf, Vec<u8>, stat::Own),
    format: Format,
    firsts: &mut HashMap<FileId, Vec<u8>>,
    ctx: &StepContext,
) -> anyhow::Result<()> {
    let mut stack = vec![top];
    while let Some((path, name, own)) = stack.pop() {
        ctx.check_cancelled()?;
        let item = match own.kind {
            EntryKind::Directory => Item::Folder,
            EntryKind::File => match firsts.get(&own.id) {
                Some(first) if format.keeps_hard_links() => Item::SecondName {
                    first: first.clone(),
                },
                _ => {
                    if format.keeps_hard_links() && own.names > 1 {
                        firsts.insert(own.id, name.clone());
                    }
                    Item::File {
                        path: path.clone(),
                        size: own.size,
                        id: Some(own.id),
                    }
                }
            },
            EntryKind::Symlink => Item::Link {
                target: own
                    .link
                    .as_ref()
                    .map(|t| t.as_encoded_bytes().to_vec())
                    .unwrap_or_default(),
            },
            // A pipe, a socket, a device: never opened, left out.
            _ => continue,
        };
        let mode = own.mode.unwrap_or(match own.kind {
            EntryKind::Directory => 0o755,
            EntryKind::Symlink => 0o777,
            _ => 0o644,
        });
        let attributes = if format.keeps_attributes() {
            attributes_of(&path)?
        } else {
            Vec::new()
        };
        let folder = matches!(item, Item::Folder);
        parts.push(Part {
            name: name.clone(),
            item,
            mode,
            modified: own.modified,
            attributes,
        });
        if !folder {
            continue;
        }
        let dir = Dir::open(&path)
            .with_context(|| format!("“{}” could not be read", display_name(&path)))?;
        let mut names: Vec<OsString> = dir
            .read()
            .with_context(|| format!("“{}” could not be read", display_name(&path)))?
            .filter_map(|listed| listed.ok().map(|l| l.name))
            .collect();
        names.sort();
        // Pushed last first: taken in order.
        for child in names.into_iter().rev() {
            // Gone since it was listed: not there to keep.
            let Some(child_own) = stat::own(&dir, &child) else {
                continue;
            };
            let mut child_name = name.clone();
            child_name.push(b'/');
            child_name.extend_from_slice(child.as_encoded_bytes());
            stack.push((path.join(&child), child_name, child_own));
        }
    }
    Ok(())
}

/// The extended attributes of `path` (a link's own) an archive keeps: those
/// a copy keeps (`xattr::kept`), none on a volume that keeps none.
fn attributes_of(path: &Path) -> anyhow::Result<Vec<(Vec<u8>, Vec<u8>)>> {
    let unread = || {
        format!(
            "the tags and attributes of “{}” could not be read",
            display_name(path)
        )
    };
    let names = match xattr::list(path) {
        Ok(names) => names,
        Err(error) if xattr::refused(&error) => return Ok(Vec::new()),
        Err(error) => return Err(error).with_context(unread),
    };
    let mut out = Vec::new();
    for name in names.into_iter().filter(|n| xattr::kept(n)) {
        // One removed since it was listed is not there to keep.
        let Some(value) = xattr::get(path, &name).with_context(unread)? else {
            continue;
        };
        out.push((name.as_encoded_bytes().to_vec(), value));
    }
    Ok(out)
}

/// Where an archive of `entries` is made unless told: the folder that holds
/// them all, else the pane's (`App::work_dir`).
fn dest_of(app: &App, pane: Option<PaneId>, entries: &[EntryRef]) -> anyhow::Result<PathBuf> {
    let mut parents = entries.iter().map(|e| e.path.parent());
    if let Some(Some(first)) = parents.next()
        && parents.all(|p| p == Some(first))
    {
        return Ok(first.to_path_buf());
    }
    app.work_dir(app.pane_or_active(pane))
}

/// The name an archive of `entries` gets in `format` unless told: one
/// entry's own name (`photos.zip`; `report.pdf.zip`, which extracts as
/// the file itself), else that of the folder holding them all
/// (`Holiday.zip`), else `Archive`.
fn default_name(entries: &[EntryRef], format: Format) -> OsString {
    let mut name = match entries {
        [one] => one.path.file_name().map(OsStr::to_os_string),
        many => {
            let mut parents = many.iter().map(|e| e.path.parent());
            match parents.next() {
                Some(Some(first)) if parents.all(|p| p == Some(first)) => {
                    first.file_name().map(OsStr::to_os_string)
                }
                _ => None,
            }
        }
    }
    .unwrap_or_else(|| OsString::from("Archive"));
    name.push(format.extension());
    name
}

/// `archive.compress`: what the menus run, and scripts.
#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
struct CompressArgs {
    /// What goes in, by absolute path; the pane's selection when omitted.
    #[serde(default)]
    entries: Vec<op::EntryArg>,
    #[serde(default)]
    pane: Option<PaneId>,
    /// The folder the archive is made in, an absolute path; the one holding
    /// the entries when omitted (the pane's, when they are in several).
    #[serde(default)]
    dest: Option<PathArg>,
    /// The archive's name, whose ending says its format
    /// (`photos.tar.zst`); after what it holds when omitted.
    #[serde(default)]
    name: Option<PathArg>,
    /// Its format, where the name says none: `zip` (when neither says),
    /// `7z`, `tar.gz`, `tar.xz`, `tar.zst`, `tar.bz2` or `tar`.
    #[serde(default)]
    format: Option<String>,
    /// Encrypted by a password asked in the window as it is made (ZIP
    /// only); never an argument.
    #[serde(default)]
    password: bool,
    /// What becomes of a name taken there (`files`: `keep_both`,
    /// `replace`, `replace_if_newer`, `skip`); asked when omitted.
    #[serde(default)]
    on_conflict: Option<clash::Policy>,
}

/// `archive.compress_ask`: the menu's Compress…, asking the name.
#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
struct AskArgs {
    /// What goes in, by absolute path; the pane's selection when omitted.
    #[serde(default)]
    entries: Vec<op::EntryArg>,
    #[serde(default)]
    pane: Option<PaneId>,
    /// The folder the archive is made in; as `archive.compress` says when
    /// omitted.
    #[serde(default)]
    dest: Option<PathArg>,
    /// Encrypted by a password asked next (a ZIP).
    #[serde(default)]
    password: bool,
}

fn absolute(dest: PathArg) -> anyhow::Result<PathBuf> {
    let dest = dest.into_inner();
    skwir_core::os_path::absolute(&dest)?;
    Ok(dest)
}

/// The plan `archive.compress` answers.
fn plan(a: CompressArgs, app: &App) -> anyhow::Result<Plan> {
    let entries = app.targets(a.pane, a.entries)?;
    let dest = match a.dest {
        Some(dest) => absolute(dest)?,
        None => dest_of(app, a.pane, &entries)?,
    };
    let asked = a
        .format
        .as_deref()
        .map(|f| {
            Format::named(f)
                .ok_or_else(|| anyhow::anyhow!("“{f}” is no format here: say one of {}", endings()))
        })
        .transpose()?;
    let name = match a.name {
        Some(name) => {
            let name = name.into_inner().into_os_string();
            match (Format::of_name(&name), asked) {
                (Some((said, _)), Some(asked)) if said != asked => anyhow::bail!(
                    "“{}” says {}, and the format {}",
                    name.to_string_lossy(),
                    said.name(),
                    asked.name()
                ),
                (None, Some(asked)) => {
                    let mut name = name;
                    name.push(asked.extension());
                    name
                }
                _ => name,
            }
        }
        None => default_name(&entries, asked.unwrap_or(Format::Zip)),
    };
    op::valid_file_name(&name)?;
    let format = format_of(&name)?;
    anyhow::ensure!(
        !a.password || format.encrypts(),
        "Only a ZIP archive can have a password: end its name with .zip"
    );
    let step = Compress {
        entries,
        dest_dir: dest,
        name,
        password: a.password,
    };
    let title = format!("compress {}", step.what());
    Ok(Plan::with_steps(title, vec![Box::new(step)]).on_conflict(a.on_conflict))
}

pub fn register(app: &App, owner: &str) {
    app.ops.register::<Compress>(owner);
    app.commands
        .register::<CompressArgs, _>(
            owner, Topic::Files,
            "archive.compress",
            "Compress",
            "Make an archive of the selection, or the entries given, beside them or in `dest`: its format is what `name` ends with (.zip, .7z, .tar.gz, .tar.xz, .tar.zst, .tar.bz2, .tar), else `format`, else ZIP (plan)",
            |a, app| Ok(Outcome::Plan(plan(a, app)?)),
        )
        .plans();
    app.commands.register::<AskArgs, _>(
        owner, Topic::Files,
        "archive.compress_ask",
        "Compress…",
        "Ask the name of an archive of the selection, its ending saying its format, and make it",
        |a, app| {
            let entries = app.targets(a.pane, a.entries)?;
            let dest = match a.dest {
                Some(dest) => absolute(dest)?,
                None => dest_of(app, a.pane, &entries)?,
            };
            let name = default_name(&entries, Format::Zip);
            let text = name.to_string_lossy().into_owned();
            let stem = text.len() - Format::Zip.extension().len();
            let what = match entries.as_slice() {
                [one] => format!("“{}”", display_name(&one.path)),
                many => format!("{} items", many.len()),
            };
            let (title, note) = if a.password {
                (
                    format!("Compress {what} with a password, as"),
                    "A ZIP archive, encrypted: its password is asked next".to_owned(),
                )
            } else {
                (
                    format!("Compress {what} as"),
                    format!("Its ending says the format: {}", endings()),
                )
            };
            // Named by identity as well as path: accepted, the dialog's
            // command resolves them without a stat each (on the window's
            // thread), and a file renamed meanwhile is not another's.
            let named: Vec<op::EntryArg> = entries.iter().cloned().map(op::EntryArg::from).collect();
            let args = Invocation::new("archive.compress")
                .with("entries", named)
                .with("dest", PathArg(dest))
                .with("password", a.password)
                .args;
            app.request_front(
                Invocation::new("ui.ask")
                    .with("title", title)
                    .with("text", text)
                    .with("select", [0, stem])
                    .with("accept", "Compress")
                    .with("note", note)
                    .with("command", "archive.compress")
                    .with("args", args)
                    .with("field", "name"),
            );
            Ok(Outcome::Done)
        },
    )
    // A prompt for the user, which runs what they answer as their own:
    // never an agent's, whose `dest` the prompt would not show.
    .user_only();
    // Beside Extract Here, one submenu: the way that asks nothing first, a
    // name kept beside one taken; then the one that asks the name and the
    // format; then with a password.
    app.registry.menus.add(
        owner,
        MenuItem::new(
            "context",
            "To ZIP",
            "archive",
            320,
            Invocation::new("archive.compress").with(
                "on_conflict",
                clash::Policy {
                    files: clash::FileChoice::KeepBoth,
                    folders: clash::FolderChoice::KeepBoth,
                },
            ),
        )
        .when(COMPRESSES)
        .submenu(COMPRESS),
    );
    app.registry.menus.add(
        owner,
        MenuItem::new(
            "context",
            "Choose Format…",
            "archive",
            321,
            Invocation::new("archive.compress_ask"),
        )
        .when(COMPRESSES)
        .submenu(COMPRESS),
    );
    app.registry.menus.add(
        owner,
        MenuItem::new(
            "context",
            "With a Password…",
            "lock",
            322,
            Invocation::new("archive.compress_ask").with("password", true),
        )
        .when(COMPRESSES)
        .submenu(COMPRESS),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refs(paths: &[&str]) -> Vec<EntryRef> {
        paths
            .iter()
            .map(|p| EntryRef::new(FileId::new(1, 1), PathBuf::from(p)))
            .collect()
    }

    fn named(paths: &[&str], format: Format) -> String {
        default_name(&refs(paths), format)
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn an_archive_is_named_after_what_it_holds() {
        assert_eq!(named(&["/example/photos"], Format::Zip), "photos.zip");
        assert_eq!(
            named(&["/example/report.pdf"], Format::Zip),
            "report.pdf.zip",
            "extracted, it is the file itself"
        );
        assert_eq!(
            named(
                &["/example/Holiday/a.jpg", "/example/Holiday/b"],
                Format::TarZst
            ),
            "Holiday.tar.zst"
        );
        assert_eq!(
            named(&["/example/one/a.jpg", "/example/two/b.jpg"], Format::Zip),
            "Archive.zip"
        );
        assert_eq!(named(&["/a", "/b"], Format::SevenZip), "Archive.7z");
    }

    #[test]
    fn the_formats_are_said_in_words() {
        assert_eq!(
            endings(),
            ".zip, .7z, .tar.gz, .tar.xz, .tar.zst, .tar.bz2 or .tar"
        );
        let error = format_of(OsStr::new("photos.rar")).unwrap_err();
        assert!(error.to_string().contains("photos.rar"), "{error}");
    }
}
