//! Extract Here (ADR-048): an archive's one folder or file put beside it,
//! or in the folder it is dropped in, else a folder of what it holds, by
//! the libarchive helper in its sandbox (`archive::extract_all`). A step
//! of a plan like a copy: made in a staging folder and put in place whole
//! (`StepContext::stage`), Cancel and Pause answered while the helper
//! works, undone to the trash. A name taken is asked about as a copy's
//! is, with a file's choices (ADR-059); an encrypted member asks the user
//! its password.

use std::ffi::{OsStr, OsString};
use std::path::{Component, Path, PathBuf};

use anyhow::Context;
use skwir_core::App;
use skwir_core::command::{Invocation, Outcome};
use skwir_core::op::{
    self, Cancelled, Change, EntryRef, Made, Operation, Plan, StepContext, StepStatus, Unanswered,
    Words,
};
use skwir_core::os_path::PathArg;
use skwir_core::registry::MenuItem;
use skwir_core::workspace::PaneId;
use skwir_platform::{archive, mime};

/// The step's kind in a plan.
const KIND: &str = "extract";

/// What `when` says of an archive: what the preview lists, as the menus
/// read names (`kind:archive`, a disc image's type).
const ARCHIVES: &str = "kind:archive|kind:application/x-cd-image";

/// Extract the archive into `dest_dir`: its one member where it holds one
/// (a folder and what is in it, `project-1.0` of `v1.0.tar.gz`; the file
/// of `notes.txt.gz` or `report.pdf.zip`), never a folder around it; else
/// a folder of what it holds. A hidden member alone (`.config`) is put in
/// a folder all the same, rather than out of sight in the folder it is
/// extracted into.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct Extract {
    /// The archive.
    pub entry: EntryRef,
    #[serde(with = "skwir_core::os_path")]
    #[schemars(with = "PathArg")]
    pub dest_dir: PathBuf,
    /// What it is called there. Not said, as the archive says
    /// (`Extract::named`): its one member's own name, else the archive's
    /// less its extensions. Settling a plan says it (`made_as`), as a name
    /// kept beside one taken does (`photos (2)`), under which the one
    /// member is placed all the same.
    #[serde(
        default,
        with = "skwir_core::os_path::opt",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Option<PathArg>")]
    pub name: Option<OsString>,
}

impl Extract {
    /// The archive at `entry` extracted into `dest_dir`, named as it says.
    pub fn new(entry: EntryRef, dest_dir: PathBuf) -> Self {
        Self {
            entry,
            dest_dir,
            name: None,
        }
    }

    /// What it is called in `dest_dir`, and whether it is a folder, by the
    /// archive's listing: `None`, not read, as the archive's name says.
    fn named(&self, listing: Option<&archive::Listing>) -> (OsString, bool) {
        let one = listing.and_then(one_member);
        let folder = match (&one, listing) {
            (Some(one), _) => one.folder,
            (None, Some(_)) => true,
            (None, None) => !only_compressed(&self.entry.path),
        };
        let name = self
            .name
            .clone()
            .or(one.map(|one| one.name))
            .unwrap_or_else(|| extracted_name(&self.entry.path));
        (name, folder)
    }

    /// What it is called in `dest_dir`: as said, else as the archive at
    /// `src` says, its listing read.
    fn name_in(&self, src: &Path, ctx: &StepContext) -> OsString {
        self.name.clone().unwrap_or_else(|| {
            let listing = archive::list(src, ctx.cancel_token()).ok();
            self.named(listing.as_deref()).0
        })
    }
}

/// What an archive at `path` of several members is extracted as, or one
/// whose listing cannot be read: its name less its extensions
/// (`photos.zip`, `photos.tar.gz` and `photos.tgz` give `photos`;
/// `notes.txt.gz` gives `notes.txt`), or with "contents" after it where
/// that is its own name.
fn extracted_name(path: &Path) -> OsString {
    let stem = path.file_stem().unwrap_or_default();
    let stem = Path::new(stem)
        .extension()
        .filter(|tar| tar.eq_ignore_ascii_case("tar"))
        .and_then(|_| Path::new(stem).file_stem())
        .unwrap_or(stem);
    if Some(stem) == path.file_name() || stem.is_empty() {
        let mut name = path.file_name().unwrap_or_default().to_owned();
        name.push(" contents");
        return name;
    }
    stem.to_owned()
}

/// Whether the archive at `path` is a file only compressed (`notes.txt.gz`),
/// as its name says: extracted, it is a file, not a folder.
fn only_compressed(path: &Path) -> bool {
    let squeezed = ["gz", "bz2", "xz", "zst", "lz", "lzma", "lz4", "z"];
    let is = |ext: Option<&OsStr>, of: &str| ext.is_some_and(|e| e.eq_ignore_ascii_case(of));
    squeezed.iter().any(|&end| is(path.extension(), end))
        && path
            .file_stem()
            .is_some_and(|stem| !is(Path::new(stem).extension(), "tar"))
}

/// What is said when a password is needed and no one can be asked.
pub(crate) fn unasked(archive: &str) -> anyhow::Error {
    anyhow::anyhow!("“{archive}” is encrypted: its password is asked in Skwir's window")
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

impl op::Kind for Extract {
    const KIND: &'static str = KIND;
}

impl op::Effect for Extract {
    fn describe(&self) -> Change {
        let (archive, into) = (display_name(&self.entry.path), display_name(&self.dest_dir));
        // As the listing last read says, if one was (`makes` reads it):
        // only the archive's metadata is read here.
        let (name, folder) = self.named(archive::listed(&self.entry.path).as_deref());
        Change::Create {
            path: self.dest_dir.join(name),
            directory: folder,
            words: Some(Words {
                will: format!("extract “{archive}” to {into}"),
                did: format!("extracted “{archive}” to {into}"),
            }),
        }
    }

    fn check(&self, ctx: &StepContext) -> StepStatus {
        if !ctx.is_folder(&self.dest_dir) {
            return conflict(format!(
                "“{}” is not a folder",
                display_name(&self.dest_dir)
            ));
        }
        let src = match self.entry.resolve() {
            Ok(src) => src,
            Err(e) => return conflict(e.to_string()),
        };
        // Read where the archive says its name, or something is there.
        let listed = std::cell::OnceCell::new();
        let listing = || {
            listed
                .get_or_init(|| archive::list(&src, ctx.cancel_token()).ok())
                .as_deref()
        };
        let name = match &self.name {
            Some(name) => name.clone(),
            None => self.named(listing()).0,
        };
        if let Err(invalid) = op::valid_file_name(&name) {
            return conflict(invalid.to_string());
        }
        let target = self.dest_dir.join(name);
        // As the plan's earlier steps leave it: one may put what is there
        // in the trash (Replace).
        if !ctx.exists(&target) {
            return StepStatus::Pending { reversible: true };
        }
        // Applied already, or another file under the name.
        if listing().is_some_and(|listing| holds_extracted(listing, &target, ctx)) {
            StepStatus::AlreadyDone
        } else if src.starts_with(&target) {
            // Never replaced: it would take the archive with it.
            conflict(format!(
                "“{}” holds “{}”, which it would be extracted from",
                display_name(&target),
                display_name(&src)
            ))
        } else {
            conflict(format!("“{}” is already there", display_name(&target)))
        }
    }

    /// Its name and kind as the archive's listing says, read here and
    /// kept for the check, the room and the extraction after: a file,
    /// whose name kept beside another keeps its extension last
    /// (`report.pdf.zip` gives `report (2).pdf`), of the size listed.
    fn makes(&self, ctx: &StepContext) -> Option<Made> {
        let listing = self
            .entry
            .resolve()
            .ok()
            .and_then(|src| archive::list(&src, ctx.cancel_token()).ok());
        let (name, folder) = self.named(listing.as_deref());
        let size = listing
            .as_deref()
            .and_then(one_member)
            .and_then(|one| one.size);
        Some(Made {
            target: self.dest_dir.join(name),
            from: self.entry.path.clone(),
            inputs: vec![self.entry.path.clone()],
            folder,
            size,
            words: ("Extract", "extracted"),
        })
    }

    /// What it extracts, by the archive's listing: its members' sizes,
    /// those listed when the listing was cut short (a lower bound). A
    /// compressed tar not listed before is read once to be counted.
    fn writes(&self, ctx: &StepContext) -> Option<(PathBuf, u64)> {
        let src = self.entry.resolve().ok()?;
        let listing = archive::list(&src, ctx.cancel_token()).ok()?;
        let bytes = listing
            .members
            .iter()
            .filter(|m| m.regular && m.hardlink.is_none())
            .filter_map(|m| u64::try_from(m.size).ok())
            .fold(0u64, u64::saturating_add);
        Some((self.dest_dir.clone(), bytes))
    }

    fn made_as(&self, name: OsString) -> Option<Box<dyn Operation>> {
        Some(Box::new(Extract {
            name: Some(name),
            ..self.clone()
        }))
    }

    fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let src = self.entry.resolve()?;
        let target = self.dest_dir.join(self.name_in(&src, ctx));
        let size = std::fs::metadata(&src)?.len();
        ctx.add_total(size);
        let staging = ctx.stage(&self.dest_dir)?;
        // A folder of its own: the staging folder holds its lock.
        let made = staging.path().join("archive");
        let mut done = 0;
        let archive_name = display_name(&self.entry.path);
        let unanswered = std::cell::Cell::new(None);
        let result = std::fs::create_dir(&made)
            .map_err(anyhow::Error::from)
            .and_then(|()| {
                archive::extract_all(
                    &src,
                    &made,
                    ctx.cancel_token(),
                    |read| {
                        let read = read.min(size);
                        if read > done {
                            ctx.advance(read - done);
                            done = read;
                        }
                    },
                    || ctx.is_paused(),
                    |attempt| {
                        // Each member's attempts count from 1: a second
                        // says the first was wrong for it.
                        let mut title = if attempt == 1 {
                            format!("Password for “{archive_name}”")
                        } else {
                            format!("Wrong password for “{archive_name}”: try again")
                        };
                        // Nothing typed is asked again, as making one
                        // asks; Cancel gives up.
                        loop {
                            match ctx.ask_secret(&title, "Extract") {
                                Ok(given) if given.is_empty() => {
                                    title = format!(
                                        "Password for “{archive_name}”: it cannot be empty"
                                    );
                                }
                                Ok(given) => return Some(given),
                                Err(why) => {
                                    unanswered.set(Some(why));
                                    return None;
                                }
                            }
                        }
                    },
                )
            })
            // Paused once it is all there, it waits before it is placed.
            .and_then(|()| Ok(ctx.check_cancelled()?))
            .and_then(|()| {
                // Its one member, under the step's name, which may be one
                // kept beside; else a folder of all it holds.
                let placed = only_member(&made).unwrap_or_else(|| made.clone());
                staging.place(&placed, &target).with_context(|| {
                    format!(
                        "“{}” could not be placed in “{}”",
                        display_name(&target),
                        display_name(&self.dest_dir)
                    )
                })
            });
        // Whatever happened, the staging folder goes now, with what it
        // holds: an archive's folders may be read-only, which only
        // `Staging::remove` opens. Should it stay, a later run sweeps it.
        if let Err(e) = staging.remove() {
            tracing::warn!("a staging folder stays: {e}");
        }
        if result.is_err() {
            match unanswered.get() {
                _ if ctx.cancel_token().is_cancelled() => return Err(Cancelled.into()),
                Some(Unanswered::Declined) => return Err(Cancelled.into()),
                Some(Unanswered::NoOne) => return Err(unasked(&archive_name)),
                None => {}
            }
        }
        result?;
        ctx.advance(size.saturating_sub(done));
        let made = EntryRef::from_path(&target)
            .with_context(|| format!("“{}” went missing as it was made", display_name(&target)))?;
        // Where its volume keeps no trash, Undo removes it while it is as
        // made, and Redo extracts it again under the same name.
        if op::keeps_no_trash(&target) {
            let again = Extract {
                name: target.file_name().map(ToOwned::to_owned),
                ..self.clone()
            };
            return Ok(Some(Box::new(crate::unmake::Unmake::of(
                made,
                crate::unmake::Again::Extract(again),
            ))));
        }
        // Undone to the trash, as a copy is: it may have been changed since.
        Ok(Some(Box::new(op::Trash { entry: made })))
    }
}

/// The one member `made` holds, when it holds one that is not hidden:
/// what the archive is extracted as, as `one_member` foresaw it.
fn only_member(made: &Path) -> Option<PathBuf> {
    let mut members = std::fs::read_dir(made).ok()?;
    let first = members.next()?.ok()?;
    (members.next().is_none() && !hidden(&first.file_name())).then(|| first.path())
}

/// Whether a name is hidden, as a listing hides it: it starts with a dot.
fn hidden(name: &OsStr) -> bool {
    name.as_encoded_bytes().starts_with(b".")
}

/// The one member an archive is extracted as (`one_member`).
struct One {
    /// Its own name.
    name: OsString,
    folder: bool,
    /// A file's size, as listed; `None` for a folder, a link, or a size
    /// not said.
    size: Option<u64>,
}

/// The one member the listed archive is extracted as: what extracting it
/// leaves at its top (`left_by`) is one entry, not hidden. `None` for
/// several, none, a hidden one, or an archive refused whole. A listing cut
/// short tells a folder by what it listed (a source tree's thousands of
/// files under one folder), a file only when it lists every member.
fn one_member(listing: &archive::Listing) -> Option<One> {
    let left = left_by(listing)?;
    let mut top = left
        .iter()
        .filter(|(path, _)| path.components().count() == 1);
    let (path, what) = top.next()?;
    let folder = *what == Left::Folder;
    let size = match what {
        Left::File { size, .. } => *size,
        _ => None,
    };
    (top.next().is_none() && !hidden(path.as_os_str()) && (folder || listing.complete)).then(|| {
        One {
            name: path.as_os_str().to_owned(),
            folder,
            size,
        }
    })
}

/// What extracting an archive leaves at a path, by its listing.
#[derive(Debug, PartialEq)]
enum Left {
    Folder,
    /// A file of this size (`None`: not said) and time.
    File {
        size: Option<u64>,
        modified: Option<std::time::SystemTime>,
    },
    /// A link saying this.
    Link(String),
    /// Another name of the file at this path.
    Hard(PathBuf),
}

/// What extracting the listed archive leaves, by path in the folder it is
/// extracted into: each member where the extraction puts it, the folders
/// above it, a later member of a name over an earlier one; pipes, devices
/// and sockets left out, as they are. `None` when a member would leave the
/// folder (`..`, absolute): the extraction refuses the whole archive.
fn left_by(listing: &archive::Listing) -> Option<std::collections::BTreeMap<PathBuf, Left>> {
    // Its names, `.` left out (`./a/`, `.`).
    let relative = |name: &str| -> Option<PathBuf> {
        let mut path = PathBuf::new();
        for part in Path::new(name).components() {
            match part {
                Component::Normal(part) => path.push(part),
                Component::CurDir => {}
                _ => return None,
            }
        }
        Some(path)
    };
    let mut left = std::collections::BTreeMap::new();
    for member in &listing.members {
        let path = relative(&member.name)?;
        if path.as_os_str().is_empty() {
            continue;
        }
        let what = if let Some(first) = &member.hardlink {
            Left::Hard(relative(first)?)
        } else if member.directory {
            Left::Folder
        } else if let Some(link) = &member.link {
            Left::Link(link.clone())
        } else if member.regular {
            Left::File {
                size: u64::try_from(member.size).ok(),
                modified: member.modified(),
            }
        } else {
            continue;
        };
        for above in path.ancestors().skip(1) {
            if above.as_os_str().is_empty() {
                break;
            }
            left.entry(above.to_path_buf()).or_insert(Left::Folder);
        }
        left.insert(path, what);
    }
    Some(left)
}

/// How far a file's time on the disk may be from the archive's: a FAT
/// drive keeps two-second times.
const TIME_KEPT: std::time::Duration = std::time::Duration::from_secs(2);

/// Whether `target` holds what extracting the listed archive made, and
/// nothing else, as a copy is told done (`same_tree`): the folders and
/// their names alike, each file of its size and time (what the extraction
/// gave it), each link saying the same, each hard link to its first name;
/// of the archive's one member (`one_member`) where it holds one. A
/// listing cut short (`archive::list`'s bounds), or a file of no size or
/// time said, says no: then nobody can tell.
fn holds_extracted(listing: &archive::Listing, target: &Path, ctx: &StepContext) -> bool {
    if !listing.complete || listing.members.is_empty() {
        return false;
    }
    let Some(mut left) = left_by(listing) else {
        return false;
    };
    // Its one member is the target. Else the target is the folder holding
    // them all.
    let mut expected: std::collections::BTreeMap<PathBuf, Left> = if let Some(one) =
        one_member(listing)
    {
        let own = Path::new(&one.name);
        std::mem::take(&mut left)
            .into_iter()
            .filter_map(|(path, what)| {
                let path = path.strip_prefix(own).ok()?.to_path_buf();
                let what = match what {
                    Left::Hard(first) => Left::Hard(first.strip_prefix(own).ok()?.to_path_buf()),
                    other => other,
                };
                Some((path, what))
            })
            .collect()
    } else {
        let mut all = std::mem::take(&mut left);
        all.insert(PathBuf::new(), Left::Folder);
        all
    };
    // The target itself for the empty path: never `target/`, which follows
    // a link.
    let at = |relative: &Path| {
        if relative.as_os_str().is_empty() {
            target.to_path_buf()
        } else {
            target.join(relative)
        }
    };
    // What is there, a link never followed; no more than what is expected
    // and one, so a folder of other things is not read whole.
    let mut there: Vec<(PathBuf, std::fs::Metadata)> = Vec::new();
    let mut walk = vec![PathBuf::new()];
    while let Some(relative) = walk.pop() {
        if there.len() > expected.len() || ctx.cancel_token().is_cancelled() {
            return false;
        }
        let path = at(&relative);
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            return false;
        };
        if meta.is_dir() {
            let Ok(read) = std::fs::read_dir(&path) else {
                return false;
            };
            walk.extend(read.flatten().map(|entry| relative.join(entry.file_name())));
        }
        there.push((relative, meta));
    }
    there.len() == expected.len()
        && there.iter().all(|(relative, meta)| {
            let Some(what) = expected.remove(relative) else {
                return false;
            };
            let path = at(relative);
            match what {
                Left::Folder => meta.is_dir(),
                Left::File { size, modified } => {
                    let near = |a: std::time::SystemTime, b: std::time::SystemTime| {
                        a.duration_since(b)
                            .or_else(|_| b.duration_since(a))
                            .is_ok_and(|apart| apart < TIME_KEPT)
                    };
                    meta.is_file()
                        && size == Some(meta.len())
                        && modified
                            .zip(meta.modified().ok())
                            .is_some_and(|(a, b)| near(a, b))
                }
                Left::Link(says) => {
                    meta.is_symlink()
                        && std::fs::read_link(&path)
                            .is_ok_and(|read| read.as_os_str().to_string_lossy() == says)
                }
                Left::Hard(first) => {
                    meta.is_file()
                        && skwir_core::fs::stat::id_of(&path)
                            .is_some_and(|id| skwir_core::fs::stat::id_of(&at(&first)) == Some(id))
                }
            }
        })
}

/// `archive.extract`: what the menus run.
#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
struct ExtractArgs {
    /// The archives, by absolute path; the pane's selection when omitted.
    #[serde(default)]
    entries: Vec<op::EntryArg>,
    #[serde(default)]
    pane: Option<PaneId>,
    /// The folder to extract into, an absolute path; each archive's own
    /// folder when omitted.
    #[serde(default)]
    dest: Option<PathArg>,
}

/// Whether the file named `type_name` is one this plugin opens: an
/// archive, or a disc image.
fn is_archive(type_name: &OsStr) -> bool {
    mime::by_names([type_name])
        .into_iter()
        .flatten()
        .any(|m| mime::is_kind(&m, mime::Kind::Archive) || crate::DISC_IMAGES.contains(m.as_str()))
}

pub fn register(app: &App, owner: &str) {
    app.ops.register::<Extract>(owner);
    app.commands
        .register::<ExtractArgs, _>(
            owner,
            "archive.extract",
            "Extract here",
            "Extract each archive of the selection, or the entries given, beside it or in `dest`: an archive of one folder or file gives that folder or file, one of several a folder named after it (plan)",
            |a, app| {
                let dest = a
                    .dest
                    .map(|d| -> anyhow::Result<PathBuf> {
                        let d = d.into_inner();
                        skwir_core::os_path::absolute(&d)?;
                        Ok(d)
                    })
                    .transpose()?;
                let archives = app.targets_typed(a.pane, a.entries)?;
                let mut extracts = Vec::new();
                for (entry, type_name) in archives {
                    // No name to type it by: a folder, a device.
                    anyhow::ensure!(
                        type_name.as_deref().is_some_and(is_archive),
                        "“{}” is not an archive",
                        display_name(&entry.path)
                    );
                    let dest_dir = match &dest {
                        Some(dest) => dest.clone(),
                        None => entry
                            .path
                            .parent()
                            .context("an archive is in a folder")?
                            .to_path_buf(),
                    };
                    extracts.push(Extract::new(entry, dest_dir));
                }
                let title = match extracts.as_slice() {
                    [one] => format!("extract “{}”", display_name(&one.entry.path)),
                    many => format!("extract {} archives", many.len()),
                };
                let steps = extracts
                    .into_iter()
                    .map(|extract| Box::new(extract) as Box<dyn Operation>)
                    .collect();
                Ok(Outcome::Plan(Plan::with_steps(title, steps)))
            },
        )
        .plans();
    // Beside it, from its own menu; in the folder it is dropped in, from
    // the drop's.
    app.registry.menus.add(
        owner,
        MenuItem::new(
            "context",
            "Extract Here",
            "folder",
            210,
            Invocation::new("archive.extract"),
        )
        .when(&format!("selection {ARCHIVES} !place")),
    );
    app.registry.menus.add(
        owner,
        MenuItem::new(
            "drop",
            "Extract Here",
            "folder",
            100,
            Invocation::new("archive.extract"),
        )
        .when(ARCHIVES),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A listing of `names`: one ending in `/` a folder, one ending in `|`
    /// a pipe, any other a file.
    fn listing(names: &[&str], complete: bool) -> archive::Listing {
        let members = names
            .iter()
            .map(|name| {
                let (name, directory, regular) = if let Some(folder) = name.strip_suffix('/') {
                    (folder, true, false)
                } else if let Some(pipe) = name.strip_suffix('|') {
                    (pipe, false, false)
                } else {
                    (*name, false, true)
                };
                archive::Member {
                    name: name.to_owned(),
                    directory,
                    regular,
                    ..Default::default()
                }
            })
            .collect();
        archive::Listing { members, complete }
    }

    fn one(names: &[&str], complete: bool) -> Option<(String, bool)> {
        one_member(&listing(names, complete))
            .map(|one| (one.name.to_string_lossy().into_owned(), one.folder))
    }

    #[test]
    fn an_archive_of_one_member_is_extracted_as_that_member() {
        let folder = Some(("project-1.0".to_owned(), true));
        assert_eq!(one(&["project-1.0/", "project-1.0/README"], true), folder);
        // Its folder not listed of its own, or under `./`.
        assert_eq!(one(&["project-1.0/src/main.rs"], true), folder);
        assert_eq!(one(&["./", "./project-1.0/README"], true), folder);
        // A pipe beside it is never extracted.
        assert_eq!(one(&["project-1.0/README", "fifo|"], true), folder);
        assert_eq!(
            one(&["report l'été.pdf"], true),
            Some(("report l'été.pdf".to_owned(), false))
        );
    }

    #[test]
    fn an_archive_of_several_or_a_hidden_one_is_a_folder() {
        assert_eq!(one(&["a/README", "b.txt"], true), None);
        assert_eq!(one(&[".config/", ".config/app.conf"], true), None);
        assert_eq!(one(&[".profile"], true), None);
        assert_eq!(one(&[], true), None);
        // Refused whole, it makes nothing.
        assert_eq!(one(&["project/README", "../outside.txt"], true), None);
    }

    #[test]
    fn a_listing_cut_short_tells_a_folder_and_never_a_file() {
        assert_eq!(
            one(&["linux-6.0/", "linux-6.0/Makefile"], false),
            Some(("linux-6.0".to_owned(), true))
        );
        assert_eq!(one(&["huge.img"], false), None);
    }

    #[test]
    fn a_name_said_is_kept_and_none_said_is_the_members() {
        let entry = EntryRef::new(
            skwir_core::FileId::new(1, 1),
            PathBuf::from("/example/v1.0.tar.gz"),
        );
        let step = Extract::new(entry, PathBuf::from("/example"));
        let of = |names: &[&str]| listing(names, true);
        let named = |step: &Extract, listing: Option<&archive::Listing>| {
            let (name, folder) = step.named(listing);
            (name.to_string_lossy().into_owned(), folder)
        };
        let release = of(&["project-1.0/", "project-1.0/README"]);
        assert_eq!(named(&step, Some(&release)), ("project-1.0".into(), true));
        assert_eq!(
            named(&step, Some(&of(&["a.txt", "b.txt"]))),
            ("v1.0".into(), true)
        );
        // Not listed: as its name says.
        assert_eq!(named(&step, None), ("v1.0".into(), true));
        // Kept beside one taken: the member under the name said.
        let Some(kept) = step.made_as("project-1.0 (2)".into()) else {
            panic!("an extraction can be named");
        };
        let kept = kept.as_any().and_then(|any| any.downcast_ref::<Extract>());
        assert_eq!(
            named(kept.unwrap(), Some(&release)),
            ("project-1.0 (2)".into(), true)
        );
    }
}
