//! Extract Here (ADR-048): an archive made a folder beside it, or in the
//! folder it is dropped in, by the libarchive helper in its sandbox
//! (`archive::extract_all`). A step of a plan like a copy: made in a
//! staging folder and put in place whole (`StepContext::stage`), Cancel
//! and Pause answered while the helper works, undone to the trash. A name
//! taken is asked about as a copy's is, with a file's choices (ADR-059);
//! an encrypted member asks the user its password.

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

/// Extract the archive into `dest_dir` as `name`: a folder of what it
/// holds, or, when all it holds is one member named as the archive says
/// (a folder `project-1.0` in `project-1.0.tar.gz`, the one file of
/// `notes.txt.gz`), that member. `name` is the archive's (`extracted_name`)
/// unless one beside a name taken was kept (`photos (2)`).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct Extract {
    /// The archive.
    pub entry: EntryRef,
    #[serde(with = "skwir_core::os_path")]
    #[schemars(with = "PathArg")]
    pub dest_dir: PathBuf,
    #[serde(with = "skwir_core::os_path")]
    #[schemars(with = "PathArg")]
    pub name: OsString,
}

impl Extract {
    /// The archive at `entry` extracted into `dest_dir`, named after it.
    pub fn new(entry: EntryRef, dest_dir: PathBuf) -> Self {
        let name = extracted_name(&entry.path);
        Self {
            entry,
            dest_dir,
            name,
        }
    }

    fn target(&self) -> PathBuf {
        self.dest_dir.join(&self.name)
    }
}

/// What an archive at `path` is extracted as: its name less its
/// extensions (`photos.zip`, `photos.tar.gz` and `photos.tgz` give
/// `photos`; `notes.txt.gz` gives `notes.txt`), or with "contents" after
/// it where that is its own name.
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
        let (archive, folder) = (display_name(&self.entry.path), display_name(&self.dest_dir));
        Change::Create {
            path: self.target(),
            directory: true,
            words: Some(Words {
                will: format!("extract “{archive}” to {folder}"),
                did: format!("extracted “{archive}” to {folder}"),
            }),
        }
    }

    fn check(&self, ctx: &StepContext) -> StepStatus {
        if let Err(invalid) = op::valid_file_name(&self.name) {
            return conflict(invalid.to_string());
        }
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
        let target = self.target();
        // As the plan's earlier steps leave it: one may put what is there
        // in the trash (Replace).
        if !ctx.exists(&target) {
            return StepStatus::Pending { reversible: true };
        }
        // Applied already, or another file under the name.
        if holds_extracted(&src, &target, &extracted_name(&self.entry.path), ctx) {
            StepStatus::AlreadyDone
        } else {
            conflict(format!("“{}” is already there", display_name(&target)))
        }
    }

    fn makes(&self) -> Option<Made> {
        Some(Made {
            target: self.target(),
            from: self.entry.path.clone(),
            folder: !only_compressed(&self.entry.path),
            words: ("Extract", "extracted"),
        })
    }

    fn made_as(&self, name: OsString) -> Option<Box<dyn Operation>> {
        Some(Box::new(Extract {
            name,
            ..self.clone()
        }))
    }

    fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let src = self.entry.resolve()?;
        let target = self.target();
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
                        let title = if attempt == 1 {
                            format!("Password for “{archive_name}”")
                        } else {
                            format!("Wrong password for “{archive_name}”: try again")
                        };
                        ctx.ask_secret(&title, "Extract")
                            .map_err(|why| unanswered.set(Some(why)))
                            .ok()
                    },
                )
            })
            // Paused once it is all there, it waits before it is placed.
            .and_then(|()| Ok(ctx.check_cancelled()?))
            .and_then(|()| {
                // What the archive holds is unwrapped by its own name, and
                // placed under the step's, which may be one kept beside.
                let own = extracted_name(&self.entry.path);
                let placed = only_member(&made, &own).unwrap_or_else(|| made.clone());
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
        // Undone to the trash, as a copy is: it may have been changed since.
        Ok(Some(Box::new(op::Trash { entry: made })))
    }
}

/// The one member `made` holds, when it is called `name`: what the
/// archive is extracted as.
fn only_member(made: &Path, name: &OsStr) -> Option<PathBuf> {
    let mut members = std::fs::read_dir(made).ok()?;
    let first = members.next()?.ok()?;
    (members.next().is_none() && first.file_name() == name).then(|| first.path())
}

/// Whether `target` holds what extracting `src` made: every member its
/// listing names, at its place, a file of its size; all of them under one
/// member called `name`, the archive's own (`extracted_name`), that
/// member's. A listing cut short (`archive::list`'s bounds) says no: then
/// nobody can tell.
fn holds_extracted(src: &Path, target: &Path, name: &OsStr, ctx: &StepContext) -> bool {
    let Ok(listing) = archive::list(src, ctx.cancel_token()) else {
        return false;
    };
    if !listing.complete || listing.members.is_empty() {
        return false;
    }
    // A name the extraction refused (`..`, absolute) was never made.
    let inside = |member: &str| {
        Path::new(member)
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
    };
    if !listing.members.iter().all(|m| inside(&m.name)) {
        return false;
    }
    let name = name.to_string_lossy();
    let place = |member: &str| -> Option<PathBuf> {
        let member = member.trim_start_matches("./").trim_end_matches('/');
        let rest = member.strip_prefix(name.as_ref())?;
        rest.strip_prefix('/')
            .or(rest.is_empty().then_some(""))
            .map(|rest| target.join(rest))
    };
    // All under one folder of its name, or one file of it: that member is
    // the target; else the target holds them all.
    let unwrapped = listing.members.iter().all(|m| place(&m.name).is_some());
    listing.members.iter().all(|member| {
        let path = if unwrapped {
            place(&member.name)
        } else {
            Some(target.join(member.name.trim_start_matches("./")))
        };
        let Some(path) = path else {
            return false;
        };
        match std::fs::symlink_metadata(&path) {
            Ok(m) if member.directory => m.is_dir(),
            Ok(m) if member.regular && member.size >= 0 => {
                m.is_file() && i64::try_from(m.len()).is_ok_and(|len| len == member.size)
            }
            Ok(_) => true,
            Err(_) => false,
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
            "Extract each archive of the selection, or the entries given, into a folder named after it, beside it or in `dest` (plan)",
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
