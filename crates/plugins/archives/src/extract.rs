//! Extract Here (ADR-048): an archive's one folder or file put beside it,
//! or in the folder it is dropped in, else a folder of what it holds, by
//! the libarchive helper in its sandbox (`archive::extract_all`). A step
//! of a plan like a copy: made in a staging folder and put in place whole
//! (`StepContext::stage`), Cancel and Pause answered while the helper
//! works, undone to the trash. A name taken is asked about as a copy's
//! is (ADR-059): a folder there under the folder's name is merged into,
//! as a copy's folder is, the archive's listing saying what it holds
//! (amended 2026-11-14), else a file's choices; an encrypted member asks
//! the user its password.

use std::ffi::{OsStr, OsString};
use std::path::{Component, Path, PathBuf};

use anyhow::Context;
use skwir_core::App;
use skwir_core::command::{Invocation, Outcome, Topic};
use skwir_core::op::{
    self, Cancelled, Change, CouldntExt as _, EntryRef, Made, MadeItem, MadeTree, Operation,
    Placing, Plan, StepContext, StepStatus, Tenses, Unanswered,
};
use skwir_core::os_path::PathArg;
use skwir_core::registry::MenuItem;
use skwir_core::words;
use skwir_core::words::{Folder, Name, Words};
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
    /// Merged into the folder there under its name: what it places there,
    /// each item of what it makes where settling the names found it goes
    /// (`op::Placing`), what is there the same left as it is. Not said, it
    /// makes its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge: Option<Vec<Placing>>,
}

impl Extract {
    /// The archive at `entry` extracted into `dest_dir`, named as it says.
    pub fn new(entry: EntryRef, dest_dir: PathBuf) -> Self {
        Self {
            entry,
            dest_dir,
            name: None,
            merge: None,
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
    words!(
        archives_unasked = "{ $name } is encrypted: its password is asked in Skwir's window",
        name = Name(archive.to_owned()),
        note = "Why an archive was not read or made where no window can ask its password (a script's, an agent's). Skwir is the program's name."
    )
    .into()
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

/// A step's conflict: why, in words.
fn conflict(reason: impl Into<Words>) -> StepStatus {
    StepStatus::Conflict {
        reason: reason.into(),
    }
}

/// An archive whose listing could not be read to its end.
fn cannot_list_whole(archive: &Path) -> Words {
    words!(
        archives_cannot_list_whole = "{ $name } cannot be listed whole",
        name = Name::of(archive),
        note = "Why an archive is not extracted into a folder already there: what it holds must be known, member by member."
    )
}

/// What an extraction into a folder already there says when it finds it
/// done: as its step says, and as its run finds nothing to do.
fn already_extracted_into(archive: &Path, into: String) -> Words {
    words!(
        archives_already_extracted_into = "{ $archive } was already extracted into { $into }",
        archive = Name::of(archive),
        into = Folder(into),
        note = "Said after extracting, when everything the archive holds was in the folder it is extracted into already."
    )
}

impl op::Kind for Extract {
    const KIND: &'static str = KIND;
}

impl op::Effect for Extract {
    /// The archive's bytes, which it counts as it reads them: the plan's
    /// time left told from its start (ADR-113).
    fn work(&self, _: &StepContext, _: Option<u64>) -> u64 {
        self.entry
            .resolve()
            .ok()
            .and_then(|src| std::fs::metadata(src).ok())
            .map_or(0, |meta| meta.len())
    }

    /// The archive read; what it makes in its folder, whose name may be
    /// known only once the archive is read: the folder, whole.
    fn footprint(&self, _: &op::footprint::Volumes) -> Option<op::footprint::Footprint> {
        Some(
            op::footprint::Footprint::default()
                .read(&self.entry.path)
                .made(&self.dest_dir)
                .heavy_if(true),
        )
    }

    fn describe(&self) -> Change {
        let (archive, into) = (display_name(&self.entry.path), display_name(&self.dest_dir));
        // As the listing last read says, if one was (`makes` reads it):
        // only the archive's metadata is read here.
        let (name, folder) = self.named(archive::listed(&self.entry.path).as_deref());
        let words = if self.merge.is_some() {
            // Into the folder there, as merged.
            let into = name.to_string_lossy().into_owned();
            Tenses {
                will: words!(
                    archives_extract_into = "extract { $archive } into { $into }",
                    archive = Name(archive.clone()),
                    into = Folder(into.clone()),
                    note = "A step of a plan, in its review: the archive, and the folder already there it is extracted into. Begins in lower case."
                ),
                did: words!(
                    archives_extracted_into = "extracted { $archive } into { $into }",
                    archive = Name(archive),
                    into = Folder(into.clone()),
                    note = "What a step did, said once its plan ran. Begins in lower case."
                ),
                already: Some(already_extracted_into(&self.entry.path, into)),
            }
        } else {
            Tenses {
                will: words!(
                    archives_extract_to = "extract { $archive } to { $into }",
                    archive = Name(archive.clone()),
                    into = Folder(into.clone()),
                    note = "A step of a plan, in its review: the archive, and the folder it is extracted to. Begins in lower case."
                ),
                did: words!(
                    archives_extracted_to = "extracted { $archive } to { $into }",
                    archive = Name(archive.clone()),
                    into = Folder(into.clone()),
                    note = "What a step did, said once its plan ran. Begins in lower case."
                ),
                already: Some(words!(
                    archives_already_extracted_to =
                        "{ $archive } was already extracted to { $into }",
                    archive = Name(archive),
                    into = Folder(into),
                    note = "Said after extracting, when what the archive holds was where it is extracted to already."
                )),
            }
        };
        Change::Create {
            path: self.dest_dir.join(name),
            directory: folder || self.merge.is_some(),
            words: Some(words),
        }
    }

    fn check(&self, ctx: &StepContext) -> StepStatus {
        if !ctx.is_folder(&self.dest_dir) {
            return conflict(crate::not_a_folder(&self.dest_dir));
        }
        let src = match self.entry.resolve() {
            Ok(src) => src,
            Err(e) => return conflict(Words::as_written(e.to_string())),
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
            return conflict(Words::of_error(invalid));
        }
        let target = self.dest_dir.join(name);
        if let Some(placing) = &self.merge {
            return check_merged(&src, &target, placing, ctx);
        }
        // As the plan's earlier steps leave it: one may put what is there
        // in the trash (Replace).
        if !ctx.exists(&target) {
            return StepStatus::Pending { reversible: true };
        }
        // Applied already, or another file under the name: as the listing
        // says, read whole only where a folder there may hold the folder
        // it makes, as a merge would find it (`made_tree`); a file there
        // is told by the quick listing, which says a file it makes whole.
        let held = |listing: &archive::Listing| {
            tree_of(listing).is_some_and(|tree| tree.held_at(&target, ctx.cancel_token()))
        };
        let folder_there = std::fs::symlink_metadata(&target).is_ok_and(|meta| meta.is_dir());
        let already = if folder_there {
            whole_listing(&src, ctx).as_deref().is_some_and(held)
        } else {
            listing().is_some_and(held)
        };
        if already {
            StepStatus::AlreadyDone
        } else if src.starts_with(&target) {
            // Never replaced: it would take the archive with it.
            conflict(words!(
                archives_holds_archive = "{ $there } holds { $archive }, the archive to extract",
                there = Name::of(&target),
                archive = Name::of(&src),
                note = "Why an archive is not extracted: the folder of its name holds the archive itself."
            ))
        } else {
            conflict(crate::already_there(&target))
        }
    }

    /// Its name and kind as the archive's listing says, read here and
    /// kept for the check, the room and the extraction after: a file,
    /// whose name kept beside another keeps its extension last
    /// (`report.pdf.zip` gives `report (2).pdf`), of the size listed.
    fn makes(&self, ctx: &StepContext) -> Option<Made> {
        // Merged, its name is the folder's there: it makes none.
        if self.merge.is_some() {
            return None;
        }
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
            words: op::MakeWords {
                verb: words!(
                    archives_extract_verb = "Extract",
                    note = "The button of the question asking what becomes of the names an extraction finds taken: extract, once they are answered."
                ),
                waiting: words!(
                    archives_nothing_extracted = "Nothing is extracted until you choose.",
                    note = "Under the title of the question asking what becomes of the names an extraction finds taken."
                ),
                cancelled: words!(
                    archives_cancelled_extract = "Cancelled: nothing was extracted",
                    note = "An extraction that found names taken asked what becomes of them, and the user cancelled it."
                ),
            },
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

    /// What it makes, member by member, as the archive's whole listing
    /// says: none where it is cut short (more members than
    /// `Bounds::WHOLE` lists), or a member would leave the folder.
    fn made_tree(&self, ctx: &StepContext) -> Option<MadeTree> {
        if self.merge.is_some() {
            return None;
        }
        let src = self.entry.resolve().ok()?;
        let listing = whole_listing(&src, ctx)?;
        tree_of(&listing)
    }

    fn merged(&self, placing: Vec<Placing>) -> Option<Box<dyn Operation>> {
        Some(Box::new(Extract {
            merge: Some(placing),
            ..self.clone()
        }))
    }

    fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let src = self.entry.resolve()?;
        let target = self.dest_dir.join(self.name_in(&src, ctx));
        // Merged, what each item is, as its check read it: what a name
        // found taken is told the same as.
        let merging = match &self.merge {
            Some(placing) => Some((
                placing,
                whole_listing(&src, ctx)
                    .as_deref()
                    .and_then(tree_of)
                    .with_context(|| cannot_list_whole(&src))?,
            )),
            None => None,
        };
        let size = std::fs::metadata(&src)?.len();
        ctx.add_total(size);
        let staging = ctx.stage(&self.dest_dir)?;
        // A folder of its own: the staging folder holds its lock.
        let made = staging.path().join("archive");
        let mut done = 0;
        let archive_name = display_name(&self.entry.path);
        let unanswered = std::cell::Cell::new(None);
        // What a merge placed, which its undo takes back.
        let mut placed = Vec::new();
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
                            words!(
                                archives_password_for = "Password for { $name }",
                                name = Name(archive_name.clone()),
                                note = "The title of the question asking an encrypted archive's password."
                            )
                        } else {
                            words!(
                                archives_wrong_password = "Wrong password for { $name }: try again",
                                name = Name(archive_name.clone()),
                                note = "The title of the question asking an archive's password again, the one given being wrong."
                            )
                        };
                        let accept = words!(
                            archives_extract_button = "Extract",
                            note = "The button that gives an archive's password and extracts it."
                        );
                        // Nothing typed is asked again, as making one
                        // asks; Cancel gives up.
                        loop {
                            match ctx.ask_secret(&title, &accept) {
                                Ok(given) if given.is_empty() => {
                                    title = words!(
                                        archives_password_empty = "Password for { $name }: it cannot be empty",
                                        name = Name(archive_name.clone()),
                                        note = "The title of the question asking an archive's password again, nothing having been typed."
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
                let root = only_member(&made).unwrap_or_else(|| made.clone());
                match &merging {
                    Some((placing, tree)) => {
                        place_merged(&staging, &root, &target, placing, tree, ctx, &mut placed)
                    }
                    None => staging
                        .place_within(&root, &target)
                        .couldnt(|| crate::not_placed(&target, &self.dest_dir)),
                }
            });
        // Whatever happened, the staging folder goes now, with what it
        // holds: an archive's folders may be read-only, which only
        // `Staging::remove` opens. Should it stay, a later run sweeps it.
        if let Err(e) = staging.remove() {
            tracing::warn!("a staging folder stays: {e}");
        }
        if let Err(error) = result {
            // Failed partway, what it placed and could not take back is
            // its undo's to take back.
            if !placed.is_empty()
                && let Ok(Some(inverse)) = self.merged_undo(&target, &placed)
            {
                return Err(op::Partly { inverse, error }.into());
            }
            match unanswered.get() {
                _ if ctx.cancel_token().is_cancelled() => return Err(Cancelled.into()),
                Some(Unanswered::Declined) => return Err(Cancelled.into()),
                Some(Unanswered::NoOne) => return Err(unasked(&archive_name)),
                None => return Err(error),
            }
        }
        ctx.advance(size.saturating_sub(done));
        if self.merge.is_some() {
            return self.merged_undo(&target, &placed);
        }
        let made = EntryRef::from_path(&target).with_context(|| crate::went_missing(&target))?;
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

impl Extract {
    /// The undo of a merge that placed `placed` in `target`: each taken
    /// back on its own, to the trash as a copy is; where the volume keeps
    /// no trash, together, while each is as made, Redo placing them again
    /// from one extraction.
    fn merged_undo(
        &self,
        target: &Path,
        placed: &[Placing],
    ) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let made: Vec<EntryRef> = placed
            .iter()
            .filter_map(|placing| EntryRef::from_path(target.join(&placing.to)))
            .collect();
        if made.is_empty() {
            // Placed already, as its check found: nothing done.
            return Err(op::Unneeded(already_extracted_into(
                &self.entry.path,
                display_name(target),
            ))
            .into());
        }
        if op::keeps_no_trash(target) {
            let again = Extract {
                merge: Some(placed.to_vec()),
                ..self.clone()
            };
            return Ok(Some(Box::new(crate::unmake::Unmake::of_each(
                made,
                crate::unmake::Again::Extract(again),
            ))));
        }
        Ok(op::Steps::of(
            made.into_iter()
                .map(|entry| Box::new(op::Trash { entry }) as Box<dyn Operation>)
                .collect(),
        ))
    }
}

/// The archive's listing, all of it where the quick bounds cut it short:
/// what a merge settles against, member by member.
fn whole_listing(src: &Path, ctx: &StepContext) -> Option<std::sync::Arc<archive::Listing>> {
    let quick = archive::list(src, ctx.cancel_token()).ok()?;
    if quick.complete {
        return Some(quick);
    }
    archive::list_within(
        src,
        skwir_platform::inspection::limits::Bounds::WHOLE,
        ctx.cancel_token(),
    )
    .ok()
    .filter(|whole| whole.complete)
}

/// The merged extraction's check: the folder there is still one, never a
/// link; each item it places is in the archive, goes in a folder there
/// that is one, reached through no link (`Within`), and finds its name
/// free (as the plan's earlier steps leave it: one may put what is there
/// in the trash) in a folder this process may write in, or there the same
/// already. All of them there, it is done.
fn check_merged(src: &Path, target: &Path, placing: &[Placing], ctx: &StepContext) -> StepStatus {
    if !ctx.is_folder(target) || is_link(target) {
        return conflict(words!(
            archives_not_a_folder_to_extract_into = "{ $name } is not a folder to extract into",
            name = Name::of(target)
        ));
    }
    // A plan from elsewhere says its own: never a name out of the folder.
    if !placing.iter().all(Placing::stays_below) {
        return conflict(words!(
            archives_would_leave = "what is extracted into { $name } would leave it",
            name = Name::of(target),
            note = "Why an archive is not extracted: a member's path leads out of the folder it is extracted into."
        ));
    }
    let Some(tree) = whole_listing(src, ctx).as_deref().and_then(tree_of) else {
        return conflict(cannot_list_whole(src));
    };
    let mut done = true;
    // Never two items to one name: the second would find it taken.
    let mut going = std::collections::HashSet::with_capacity(placing.len());
    let mut within = Within::new(target);
    for placing in placing {
        if !going.insert(placing.to.as_path()) {
            return conflict(words!(
                archives_two_as_one = "two items would be extracted as { $name }",
                name = Name(placing.to.display().to_string()),
                note = "The name is a path within the folder extracted into."
            ));
        }
        let Some(item) = tree.below(&placing.from) else {
            return conflict(words!(
                archives_not_in_archive = "{ $member } is not in { $archive } any more",
                member = Name(placing.from.display().to_string()),
                archive = Name::of(src),
                note = "The member is named by its path within the archive."
            ));
        };
        let to = target.join(&placing.to);
        let folder = match within.reach(&to) {
            Ok(folder) => folder,
            Err(why) => return conflict(why),
        };
        if ctx.exists(&to) {
            if item.held_at(&to, ctx.cancel_token()) {
                continue;
            }
            return conflict(crate::already_there(&to));
        }
        if !within.writable(folder, ctx) {
            return conflict(words!(
                archives_not_writable =
                    "{ $name } cannot be written in: nothing is extracted into it",
                name = Name::of(folder)
            ));
        }
        done = false;
    }
    if done {
        StepStatus::AlreadyDone
    } else {
        StepStatus::Pending { reversible: true }
    }
}

/// Whether `path` is a link (a Windows junction too), never followed.
fn is_link(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink())
}

/// The folders below a merged extraction's folder that its items go in,
/// each read once: a folder, never a link, every part of the way from the
/// folder merged into (a plan from elsewhere naming one, or a folder made
/// a link since the question, would lead out of it); and, where asked,
/// one this process may write in.
struct Within<'a> {
    target: &'a Path,
    folders: std::collections::HashSet<PathBuf>,
    writable: std::collections::HashMap<PathBuf, bool>,
}

impl<'a> Within<'a> {
    fn new(target: &'a Path) -> Self {
        Within {
            target,
            folders: std::collections::HashSet::new(),
            writable: std::collections::HashMap::new(),
        }
    }

    /// The folder `to` goes in, each part of the way from the folder
    /// merged into a folder, none a link; else why not.
    fn reach<'p>(&mut self, to: &'p Path) -> Result<&'p Path, Words> {
        let not_there = || {
            words!(
                archives_folder_not_there = "the folder { $name } goes in is not there",
                name = Name::of(to),
                note = "Why a member of an archive is not extracted into a folder already there: the folder it goes in went."
            )
        };
        let folder = to.parent().ok_or_else(not_there)?;
        let below = folder.strip_prefix(self.target).map_err(|_| not_there())?;
        let mut at = self.target.to_path_buf();
        for part in below.components() {
            at.push(part);
            if self.folders.contains(&at) {
                continue;
            }
            match std::fs::symlink_metadata(&at) {
                Ok(meta) if meta.is_dir() => {
                    self.folders.insert(at.clone());
                }
                Ok(meta) if meta.file_type().is_symlink() => {
                    return Err(words!(
                        archives_through_link =
                            "{ $name } is a link: nothing is extracted through it",
                        name = Name::of(&at)
                    ));
                }
                _ => return Err(not_there()),
            }
        }
        Ok(folder)
    }

    /// Whether this process may make names in `folder`, asked once.
    fn writable(&mut self, folder: &Path, ctx: &StepContext) -> bool {
        *self
            .writable
            .entry(folder.to_path_buf())
            .or_insert_with(|| ctx.may_write_in(folder))
    }
}

/// Each item `placing` says, from `root` (what the archive was extracted
/// as, in `staging`) into `target`, the folder there merged into, noted in
/// `placed`; one there already the same (`tree` says what each item is,
/// as the check found it) left as it is. Anything else under its name, or
/// a link on its way (`Within`), come since the check, is never placed
/// over or through: the step fails, and those placed before are taken
/// back where they were made (`Staging::take_back`, a read-only folder
/// opened for it), which goes with the staging folder; any that cannot be
/// stays in `placed`, for the step's undo.
fn place_merged(
    staging: &op::Staging,
    root: &Path,
    target: &Path,
    placing: &[Placing],
    tree: &MadeTree,
    ctx: &StepContext,
    placed: &mut Vec<Placing>,
) -> anyhow::Result<()> {
    let mut within = Within::new(target);
    for item in placing {
        let to = target.join(&item.to);
        let failed = if let Err(why) = within.reach(&to) {
            Some(anyhow::Error::from(why))
        } else if std::fs::symlink_metadata(&to).is_ok() {
            let same = tree
                .below(&item.from)
                .is_some_and(|made| made.held_at(&to, ctx.cancel_token()));
            if same {
                continue;
            }
            Some(anyhow::Error::from(crate::already_there(&to)))
        } else {
            staging
                .place_within(&root.join(&item.from), &to)
                .err()
                .map(|error| crate::not_placed(&to, target).because(error))
        };
        if let Some(error) = failed {
            // What cannot be taken back stays placed, and in `placed`:
            // the step's undo takes it back (`op::Partly`).
            let mut stays = Vec::new();
            for back in placed.drain(..).rev() {
                let (at, made) = (target.join(&back.to), root.join(&back.from));
                if let Err(e) = staging.take_back(&at, &made) {
                    tracing::warn!("“{}” stays where it was placed: {e}", back.to.display());
                    stays.push(back);
                }
            }
            stays.reverse();
            *placed = stays;
            return Err(error);
        }
        placed.push(item.clone());
    }
    Ok(())
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
    let folder = *what == MadeItem::Folder;
    let size = match what {
        MadeItem::File { size, .. } => *size,
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

/// What extracting the listed archive leaves, by path in the folder it is
/// extracted into: each member where the extraction puts it, the folders
/// above it, a later member of a name over an earlier one; pipes, devices
/// and sockets left out, as they are. `None` when a member would leave the
/// folder (`..`, absolute): the extraction refuses the whole archive.
fn left_by(listing: &archive::Listing) -> Option<std::collections::BTreeMap<PathBuf, MadeItem>> {
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
            MadeItem::Hard(relative(first)?)
        } else if member.directory {
            MadeItem::Folder
        } else if let Some(link) = &member.link {
            MadeItem::Link(link.clone())
        } else if member.regular {
            MadeItem::File {
                size: u64::try_from(member.size).ok(),
                modified: member.modified(),
            }
        } else {
            continue;
        };
        // The folders above it not met yet: above one met, all are.
        for above in path.ancestors().skip(1) {
            if above.as_os_str().is_empty() || left.contains_key(above) {
                break;
            }
            left.insert(above.to_path_buf(), MadeItem::Folder);
        }
        left.insert(path, what);
    }
    Some(left)
}

/// What extracting the listed archive makes, as the folder or file it is
/// extracted as: its one member's (`one_member`) where it holds one, its
/// paths below that member; else the folder holding them all. `None` for a
/// listing cut short (`archive::list`'s bounds), an empty one, or one the
/// extraction refuses whole: nobody can tell what it makes. A file of no
/// size or time said (a file only compressed) is never told the same as
/// what is there (`op::MadeTree`).
fn tree_of(listing: &archive::Listing) -> Option<MadeTree> {
    if !listing.complete || listing.members.is_empty() {
        return None;
    }
    let mut left = left_by(listing)?;
    let Some(one) = one_member(listing) else {
        left.insert(PathBuf::new(), MadeItem::Folder);
        return Some(MadeTree::new(left));
    };
    let own = Path::new(&one.name);
    let items = left
        .into_iter()
        .filter_map(|(path, what)| {
            let path = path.strip_prefix(own).ok()?.to_path_buf();
            let what = match what {
                MadeItem::Hard(first) => {
                    MadeItem::Hard(first.strip_prefix(own).ok()?.to_path_buf())
                }
                other => other,
            };
            Some((path, what))
        })
        .collect();
    Some(MadeTree::new(items))
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

/// The menu item extracting an archive: beside it, or where it is dropped.
fn extract_here() -> Words {
    words!(
        archives_extract_here = "Extract Here",
        note = "A menu item, in an archive's menu and in what a drop of one offers."
    )
}

pub fn register(app: &App, owner: &str) {
    app.ops.register::<Extract>(owner);
    app.commands
        .register::<ExtractArgs, _>(
            owner, Topic::Files,
            "archive.extract",
            words!(archives_extract = "Extract here"),
            words!(
                archives_extract_description = "Extract each archive of the selection, or the entries given, beside it or in `dest`: an archive of one folder or file gives that folder or file, one of several a folder named after it (plan)",
                note = "Read by agents and in F1's list of actions; `dest` is the command's argument, as written."
            ),
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
                        words!(
                            archives_not_an_archive = "{ $name } is not an archive",
                            name = Name::of(&entry.path)
                        )
                    );
                    let dest_dir = match &dest {
                        Some(dest) => dest.clone(),
                        None => entry
                            .path
                            .parent()
                            .with_context(|| {
                                words!(
                                    archives_in_a_folder = "an archive is in a folder",
                                    note = "An error an agent or a script reads: the archive named has no folder (a root)."
                                )
                            })?
                            .to_path_buf(),
                    };
                    extracts.push(Extract::new(entry, dest_dir));
                }
                let title = words!(
                    archives_extract_plan = "{ $count ->
                        [1] extract { $name }
                       *[other] extract { $count } archives
                    }",
                    count = extracts.len(),
                    name = extracts
                        .first()
                        .map_or_else(|| Name(String::new()), |one| Name::of(&one.entry.path)),
                    note = "A plan's title, in its review, Activity and the History: the archive extracted, or how many. Begins in lower case: said inside other words too (undo extract 3 archives)."
                );
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
            extract_here(),
            "folder",
            310,
            Invocation::new("archive.extract"),
        )
        .when(&format!("selection {ARCHIVES} !place")),
    );
    app.registry.menus.add(
        owner,
        MenuItem::new(
            "drop",
            extract_here(),
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
