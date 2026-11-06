//! skwir plugin: smaller copies (ADR-112). Reduce Size… (the Compress
//! submenu of a selection of pictures or PDFs, the preview's menu) asks a
//! dialog (`dialog`): how small, gray, a picture's location and camera
//! details left out, at most a size. Each file then gets a copy beside it,
//! `name (reduced).ext`, made by the print helper's `--shrink` mode
//! (`skwir_platform::shrink_job`), which links the engine (`skwir-shrink`):
//! a picture resized and written again, a PDF's images replaced where they
//! are stored, what it stores twice kept once. A copy that would not be
//! smaller is not made, and said.
//!
//! A PDF's copy is checked before it is kept
//! (`skwir_platform::shrink_job::made`): the pages showing an image
//! written anew are drawn from the file and from the copy by the reader
//! the settings choose, and an image on a page that looks different is
//! put back as it was.
//!
//! Each copy is a step of a plan, undone to the trash, as a copy is.
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use anyhow::Context as _;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skwir_core::App;
use skwir_core::app::NotifyLevel;
use skwir_core::cancel::CancelToken;
use skwir_core::command::{Invocation, Outcome, Topic};
use skwir_core::op::{
    self, Change, EntryArg, EntryRef, Made, Operation, Plan, StepContext, StepStatus, Words,
};
use skwir_core::os_path::PathArg;
use skwir_core::plugin::{AppRef, Plugin};
use skwir_core::registry::MenuItem;
use skwir_core::scheduler::Priority;
use skwir_core::shrink::{Job, Kind, Quality};
use skwir_core::workspace::PaneId;
use skwir_platform::process::Metered;

mod dialog;

const ID: &str = "shrink";

/// The step's kind in a plan.
const KIND: &str = "shrink_copy";

/// The types a copy is made of, as a menu's `kind:` names them: every
/// entry one of them.
const TYPES: &str = "kind:application/pdf,image/jpeg,image/png";

/// The file menu's submenu it joins, the archives' Compress.
const COMPRESS: &str = "Compress";

/// The menu of the preview's More button and of its own context menu,
/// whose items the window runs with what the preview shows
/// (`ui.preview_invoke`).
const PREVIEW: &str = "preview";

/// What Reduce Size… is asked with, by its dialog or a script.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct ReduceArgs {
    /// The pictures (JPEG, PNG) and PDFs; the selection's without.
    #[serde(default)]
    pub entries: Vec<EntryArg>,
    #[serde(default)]
    pub pane: Option<PaneId>,
    /// How small: `lossless`, `print`, `screen` (without), `small`.
    #[serde(default)]
    pub quality: Quality,
    /// Pictures, and a PDF's images, made gray.
    #[serde(default)]
    pub gray: bool,
    /// A picture's location and camera details left out; its date stays.
    #[serde(default)]
    pub strip: bool,
    /// At most this size, each copy (`2 MB`, `500 KB`, bytes): smaller
    /// steps are tried until it fits, the smallest kept when none does.
    #[serde(default)]
    pub at_most: Option<String>,
    /// The folder the copies are made in; each file's own without.
    #[serde(default)]
    pub dest: Option<PathArg>,
}

/// The shrink plugin, one for each app it is added to: what it holds
/// there, shared by what it registers (`Held`).
#[derive(Default)]
pub struct ShrinkPlugin {
    held: Held,
}

/// What the plugin holds for its app, cloned into its commands and steps:
/// the app, for the step, which checks a PDF's copy with the readers the
/// window reads it with (a step is data, not the app's: `AppRef`), and
/// the dialog's last choices. Never a static: two apps in one process
/// hold their own.
#[derive(Clone, Default)]
struct Held {
    app: AppRef,
    last: dialog::Last,
}

impl Plugin for ShrinkPlugin {
    fn id(&self) -> &'static str {
        ID
    }

    fn title(&self) -> &'static str {
        "Smaller copies"
    }

    fn description(&self) -> &'static str {
        "Make smaller copies of pictures and PDFs beside them: Reduce Size…, in Compress and in a preview's menu"
    }

    fn category(&self) -> skwir_core::plugin::Category {
        skwir_core::plugin::Category::Files
    }

    fn capabilities(&self) -> skwir_core::permissions::Capabilities {
        use skwir_core::permissions::Capability;
        // Reads what it shrinks, starts the print helper, writes the copies.
        skwir_core::permissions::Capabilities::of(&[
            Capability::Read,
            Capability::Write,
            Capability::Programs,
        ])
    }

    fn requirements(&self) -> Vec<skwir_core::plugin::Requirement> {
        vec![
            skwir_platform::print_job::WORKER
                .requirement("Making smaller copies")
                .required(),
        ]
    }

    fn register(&self, app: &App) {
        self.held.app.hold(app);
        // A step rebuilt from its data (a plan kept, redone) reaches the
        // app as one made here does.
        let rebuilt = self.held.app.clone();
        app.ops.register_with(ID, KIND, move |data| {
            let step = ShrinkCopy {
                app: rebuilt.clone(),
                ..serde_json::from_value(data)?
            };
            Ok(Box::new(step))
        });
        let made = self.held.app.clone();
        app.commands
            .register::<ReduceArgs, _>(
                ID,
                Topic::Files,
                "shrink.reduce",
                "Make Smaller Copies",
                "Make a smaller copy of each picture (JPEG, PNG) and PDF beside it, or in `dest`, named `name (reduced).ext`: `quality` lossless, print (pictures 3840 pixels across, a PDF's images 300 dpi), screen (1920, 150 dpi) or small (1280, 96 dpi); `gray`; `strip`, a picture's location and camera details left out; `at_most` a size each copy fits, smaller steps tried. A copy that would not be smaller is not made (plan)",
                move |a, app| Ok(Outcome::Plan(plan_of(app, &a, &made)?)),
            )
            .plans();
        let held = self.held.clone();
        app.commands
            .register::<ReduceArgs, _>(
                ID,
                Topic::Files,
                "shrink.reduce_ask",
                "Reduce Size…",
                "Ask how small, then make a smaller copy of each picture and PDF beside it, as `shrink.reduce` does",
                move |a, app| {
                    match resolved(app, a) {
                        Ok(args) => ask_first(app, args, held.clone()),
                        Err(error) => app.notify(NotifyLevel::Error, format!("{error:#}")),
                    }
                    Ok(Outcome::Done)
                },
            )
            // A dialog for the user, which runs what they answer as their
            // own: never an agent's.
            .user_only();
        menus(app);
    }

    fn on_disable(&self, _app: &App) {
        self.held.last.forget();
        // A step of a plan made before is refused from here on.
        self.held.app.release();
    }
}

fn menus(app: &App) {
    app.registry.menus.add(
        ID,
        MenuItem::new(
            "context",
            "Reduce Size…",
            "archive",
            325,
            Invocation::new("shrink.reduce_ask"),
        )
        .when(&format!("selection !place !restores {TYPES}"))
        .submenu(COMPRESS),
    );
    app.registry.menus.add(
        ID,
        MenuItem::new(
            PREVIEW,
            "Reduce Size…",
            "archive",
            345,
            Invocation::new("ui.preview_invoke").with("command", "shrink.reduce_ask"),
        )
        .when(TYPES),
    );
}

/// `args` with what they name taken now, as the dialog starts: the pane
/// and the files selected there, so the copies are of what was asked
/// whatever the window shows by the time it is answered.
fn resolved(app: &App, args: ReduceArgs) -> anyhow::Result<ReduceArgs> {
    let pane = app.pane_or_active(args.pane);
    let entries = app
        .targets(Some(pane), args.entries)?
        .into_iter()
        .map(EntryArg::from)
        .collect();
    Ok(ReduceArgs {
        entries,
        pane: Some(pane),
        ..args
    })
}

/// The dialog on a worker, then the plan its answer makes applied as the
/// user's action.
fn ask_first(app: &App, args: ReduceArgs, held: Held) {
    let weak = app.weak();
    app.scheduler.spawn(
        Priority::Normal,
        "shrink-dialog",
        app.lifetime().child(),
        move || {
            let Some(app) = weak.upgrade() else { return };
            let cancel = app.lifetime().child();
            if let Err(error) = asked_then(&app, args, &held, &cancel) {
                app.notify(NotifyLevel::Error, format!("{error:#}"));
            }
            app.wake();
        },
    );
}

fn asked_then(
    app: &Arc<App>,
    args: ReduceArgs,
    held: &Held,
    cancel: &CancelToken,
) -> anyhow::Result<()> {
    let files = typed(app, &args)?;
    let offers = dialog::Offers {
        what: match files.as_slice() {
            [(entry, _)] => format!("“{}”", display_name(&entry.path)),
            many => format!("{} files", many.len()),
        },
        pictures: files.iter().any(|(_, kind)| *kind == Kind::Picture),
        pdfs: files.iter().any(|(_, kind)| *kind == Kind::Pdf),
        only_jpegs: files.iter().all(|(entry, _)| is_jpeg(&entry.path)),
    };
    let Some(chosen) = dialog::ask(app, &offers, &held.last, cancel) else {
        return Ok(());
    };
    let args = ReduceArgs {
        quality: chosen.quality,
        gray: chosen.gray,
        strip: chosen.strip,
        at_most: chosen.at_most.map(|bytes| bytes.to_string()),
        ..args
    };
    app.apply_whole(plan_of(app, &args, &held.app)?);
    Ok(())
}

/// The files `args` name (the selection's without), each with what it is
/// made a copy of as: a picture or a PDF, by its name's type; one that is
/// neither is refused, named.
fn typed(app: &App, args: &ReduceArgs) -> anyhow::Result<Vec<(EntryRef, Kind)>> {
    let typed = app.targets_typed(args.pane, args.entries.clone())?;
    anyhow::ensure!(!typed.is_empty(), "Nothing to make smaller");
    typed
        .into_iter()
        .map(|(entry, name)| {
            let mime = name.as_deref().and_then(skwir_platform::mime::by_name);
            let kind = match mime.as_ref().map(skwir_platform::mime::Mime::as_str) {
                Some("application/pdf") => Kind::Pdf,
                Some("image/jpeg" | "image/png") => Kind::Picture,
                _ => anyhow::bail!(
                    "“{}” is neither a JPEG or PNG picture nor a PDF",
                    display_name(&entry.path)
                ),
            };
            Ok((entry, kind))
        })
        .collect()
}

/// Whether `path` is named as a JPEG.
fn is_jpeg(path: &Path) -> bool {
    path.extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("jpg") || extension.eq_ignore_ascii_case("jpeg")
    })
}

/// The plan of `args`: a step a file, each copy beside its file (in
/// `dest` when named), each reaching the app as `reach` holds it.
fn plan_of(app: &App, args: &ReduceArgs, reach: &AppRef) -> anyhow::Result<Plan> {
    let files = typed(app, args)?;
    let at_most = args
        .at_most
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(|text| dialog::size(text).map_err(anyhow::Error::msg))
        .transpose()?;
    anyhow::ensure!(
        at_most.is_none() || args.quality != Quality::Lossless,
        "Lossless re-encodes nothing, so it cannot bring a copy under a size: ask `at_most` with print, screen or small"
    );
    let mut plan = Plan::new(match files.as_slice() {
        [(entry, _)] => format!("make a smaller copy of “{}”", display_name(&entry.path)),
        many => format!("make smaller copies of {} files", many.len()),
    });
    for (entry, kind) in files {
        let dest_dir = match &args.dest {
            Some(PathArg(dest)) => dest.clone(),
            None => entry
                .path
                .parent()
                .context("The file is in no folder")?
                .to_owned(),
        };
        let name = copy_name(&entry.path, kind);
        plan.push(ShrinkCopy {
            entry,
            kind: kind.into(),
            dest_dir,
            name,
            quality: args.quality,
            gray: args.gray,
            strip: args.strip,
            at_most,
            app: reach.clone(),
        });
    }
    Ok(plan)
}

/// What the copy of `path` is named: its name with ` (reduced)` before its
/// ending, which it keeps (a picture stays the format it is); one without
/// an ending gets its kind's.
fn copy_name(path: &Path, kind: Kind) -> OsString {
    let mut name = path.file_stem().unwrap_or(path.as_os_str()).to_owned();
    name.push(" (reduced).");
    match path.extension() {
        Some(extension) => name.push(extension),
        None => name.push(match kind {
            Kind::Pdf => "pdf",
            Kind::Picture => "jpg",
        }),
    }
    name
}

/// A name as shown: its last part, as it reads.
fn display_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

/// A smaller copy of `entry` made in `dest_dir` as `name`, or none when it
/// would not be smaller (`op::Unneeded`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ShrinkCopy {
    pub entry: EntryRef,
    pub kind: KindArg,
    #[serde(with = "skwir_core::os_path")]
    #[schemars(with = "PathArg")]
    pub dest_dir: PathBuf,
    #[serde(with = "skwir_core::os_path")]
    #[schemars(with = "PathArg")]
    pub name: OsString,
    #[serde(default)]
    pub quality: Quality,
    #[serde(default)]
    pub gray: bool,
    #[serde(default)]
    pub strip: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at_most: Option<u64>,
    /// The app it is made in, as the plugin holds it: no part of its data
    /// (set as it is made, and as it is rebuilt: `register_with`).
    #[serde(skip)]
    #[schemars(skip)]
    pub app: AppRef,
}

/// What a copy is made of, as a plan's step says it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum KindArg {
    Picture,
    Pdf,
}

impl From<Kind> for KindArg {
    fn from(kind: Kind) -> KindArg {
        match kind {
            Kind::Picture => KindArg::Picture,
            Kind::Pdf => KindArg::Pdf,
        }
    }
}

impl From<KindArg> for Kind {
    fn from(kind: KindArg) -> Kind {
        match kind {
            KindArg::Picture => Kind::Picture,
            KindArg::Pdf => Kind::Pdf,
        }
    }
}

impl ShrinkCopy {
    fn target(&self) -> PathBuf {
        self.dest_dir.join(&self.name)
    }
}

impl op::Kind for ShrinkCopy {
    const KIND: &'static str = KIND;
}

impl op::Effect for ShrinkCopy {
    /// Its file's bytes, as the helper counts them going through it: the
    /// plan's time left told from its start (ADR-113).
    fn work(&self, _: &StepContext, _: Option<u64>) -> u64 {
        std::fs::metadata(&self.entry.path).map_or(0, |meta| meta.len())
    }

    fn footprint(&self, _: &op::footprint::Volumes) -> Option<op::footprint::Footprint> {
        // A name taken there is kept beside when its turn comes.
        Some(
            op::footprint::Footprint::default()
                .read(&self.entry.path)
                .settles_name(&self.target(), true)
                .heavy_if(true),
        )
    }

    fn describe(&self) -> Change {
        let what = display_name(&self.entry.path);
        let copy = display_name(&self.target());
        Change::Create {
            path: self.target(),
            directory: false,
            words: Some(Words {
                will: format!("make “{copy}”, a smaller copy of “{what}”"),
                did: format!("made “{copy}”, a smaller copy of “{what}”"),
            }),
        }
    }

    fn check(&self, ctx: &StepContext) -> StepStatus {
        let conflict = |reason: String| StepStatus::Conflict { reason };
        if let Err(invalid) = op::valid_file_name(&self.name) {
            return conflict(invalid.to_string());
        }
        if let Err(error) = self.entry.resolve() {
            return conflict(error.to_string());
        }
        if !ctx.is_folder(&self.dest_dir) {
            return conflict(format!(
                "“{}” is not a folder",
                display_name(&self.dest_dir)
            ));
        }
        // A copy is made aside and moved into place, which a folder that
        // takes only copies written in place (a camera) does not do.
        if !StepContext::stages_in(&self.dest_dir) {
            return conflict(format!(
                "“{}” takes only copies written in place: no smaller copy is made there",
                display_name(&self.dest_dir)
            ));
        }
        if ctx.exists(&self.target()) {
            return conflict(format!(
                "“{}” is already there",
                display_name(&self.target())
            ));
        }
        StepStatus::Pending { reversible: true }
    }

    fn makes(&self, _: &StepContext) -> Option<Made> {
        Some(Made {
            target: self.target(),
            from: self.entry.path.clone(),
            inputs: vec![self.entry.path.clone()],
            folder: false,
            size: None,
            words: ("Make Smaller Copy", "made smaller"),
        })
    }

    fn made_as(&self, name: OsString) -> Option<Box<dyn Operation>> {
        Some(Box::new(ShrinkCopy {
            name,
            ..self.clone()
        }))
    }

    fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let target = self.target();
        let app = self
            .app
            .get()
            .context("Smaller copies need the plugin on")?;
        let cancel = ctx.cancel_token();
        let source = self.entry.resolve()?;
        // Made in a folder of this machine's, in the lane of what it reads:
        // only the copy goes where it is kept, staged there and put in place.
        // Skwir's cache, on a disk, holds it while it is made and checked
        // (a copy of a PDF may be gigabytes, which a temporary folder in
        // memory would hold in memory); the folder goes when it is done.
        let work = work_folder(&app).context("A folder to make the copy in")?;
        let job = Job {
            kind: self.kind.into(),
            input: source.clone(),
            output: PathBuf::new(),
            quality: self.quality,
            gray: self.gray,
            strip: self.strip,
            at_most: self.at_most,
            keep: Vec::new(),
            rule: None,
        };
        let name = format!(
            "copy.{}",
            source
                .extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or("bin")
        );
        let (weak, token, at, gauge) = (
            app.weak(),
            cancel.clone(),
            work.path().to_owned(),
            ctx.gauge(),
        );
        let made = in_lane(&app, &source, "shrink", cancel, move || {
            let app = weak.upgrade().context("Skwir is closing")?;
            // Its size announced, read here in its lane, so the window
            // shows the copy being made, with Cancel, however long it takes:
            // one file is one step, which shows nothing otherwise. The
            // helper says how far it went, held while the plan is paused.
            let size = std::fs::metadata(&job.input).map_or(1, |meta| meta.len().max(1));
            gauge.announce(size);
            let metered = Mutex::new(Metered::new(gauge.clone(), size));
            let meter = || metered.lock().unwrap_or_else(PoisonError::into_inner);
            let went = |fraction: f64| meter().went(fraction);
            let held = || gauge.is_paused();
            // A PDF made again after images were put back goes through the
            // whole file again: that much more work, announced.
            let again = || {
                let mut meter = meter();
                meter.went(1.0);
                gauge.announce(size);
                *meter = Metered::new(gauge.clone(), size);
            };
            let follow = skwir_platform::process::Follow::new(&went, &held);
            let made =
                skwir_platform::shrink_job::made(&app, job, &at, &name, &token, &follow, &again);
            meter().went(1.0);
            made
        });
        if cancel.is_cancelled() && made.is_err() {
            return Err(op::Cancelled.into());
        }
        let (copy, made) = made?;
        let what = display_name(&self.entry.path);
        let Some(copy) = copy else {
            let why = match made.notes.first() {
                Some(note) => format!("No smaller copy of “{what}” was made: {note}"),
                // Location and camera details go with a copy: none made,
                // they stay, which is said, never left to be assumed.
                None if self.strip && self.kind == KindArg::Picture => format!(
                    "No smaller copy of “{what}” was made, so its location and camera details were not removed either"
                ),
                None => format!("“{what}” is as small as it gets already: no copy was made"),
            };
            return Err(op::Unneeded(why).into());
        };
        if made.met == Some(false) {
            ctx.say(format!(
                "“{what}” could not be brought under the size asked: its smallest copy is {}",
                sized(made.after)
            ));
        }
        for note in made.notes {
            ctx.say(format!("“{what}”: {note}"));
        }
        ctx.check_cancelled()?;
        let staging = ctx.stage(&self.dest_dir)?;
        let staged = staging.path().join("made");
        // Read from the file as it was checked, never again by its name,
        // which the helper could have left a link at.
        let kept = copied(copy.file, &staged);
        let result = kept.map_err(anyhow::Error::from).and_then(|()| {
            ctx.check_cancelled()?;
            staging.place(&staged, &target).with_context(|| {
                format!(
                    "“{}” could not be placed in “{}”",
                    display_name(&target),
                    display_name(&self.dest_dir)
                )
            })
        });
        if let Err(e) = staging.remove() {
            tracing::warn!("a staging folder stays: {e}");
        }
        if cancel.is_cancelled() && result.is_err() {
            return Err(op::Cancelled.into());
        }
        result?;
        let made = EntryRef::from_path(&target)
            .with_context(|| format!("“{}” went missing as it was made", display_name(&target)))?;
        // Undone to the trash, as a copy is: it may have been changed since.
        Ok(Some(Box::new(op::Trash { entry: made })))
    }
}

/// A folder of its own in the plugin's corner of Skwir's cache, for a
/// copy being made, removed when dropped.
fn work_folder(app: &App) -> std::io::Result<tempfile::TempDir> {
    let folder = app.cache.namespace(ID).folder("work");
    std::fs::create_dir_all(&folder)?;
    tempfile::Builder::new().prefix("copy-").tempdir_in(folder)
}

/// What `from` holds written at `to`, a name nothing has.
fn copied(mut from: std::fs::File, to: &Path) -> std::io::Result<()> {
    let mut out = std::fs::File::create_new(to)?;
    std::io::copy(&mut from, &mut out)?;
    out.sync_all()
}

/// `bytes` said as a size: 1.2 MB, 640 KB.
fn sized(bytes: u64) -> String {
    #[allow(clippy::cast_precision_loss)]
    let value = bytes as f64;
    if bytes >= 1_000_000 {
        format!("{:.1} MB", value / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{:.0} KB", value / 1_000.0)
    } else {
        format!("{bytes} bytes")
    }
}

/// `work` run in the lane of `path` (its remote's, the shared one
/// elsewhere: `fs::reach::lane`) and waited for here: what reads files
/// runs there, so a server that stopped answering holds its own lane
/// (AGENTS.md, remote paths). An error when `cancel` fires first.
fn in_lane<T: Send + 'static>(
    app: &App,
    path: &Path,
    label: &'static str,
    cancel: &CancelToken,
    work: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> anyhow::Result<T> {
    let (said, heard) = std::sync::mpsc::sync_channel(1);
    app.scheduler.spawn_in(
        skwir_core::fs::reach::lane(path),
        Priority::Normal,
        label,
        cancel.clone(),
        move || {
            let _ = said.send(work());
        },
    );
    loop {
        match heard.recv_timeout(Duration::from_millis(100)) {
            Ok(result) => return result,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) if !cancel.is_cancelled() => {}
            Err(_) => anyhow::bail!("Making the copy was cancelled"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_copy_is_named_after_its_file_and_keeps_its_ending() {
        assert_eq!(
            copy_name(Path::new("/example/report.pdf"), Kind::Pdf),
            "report (reduced).pdf"
        );
        assert_eq!(
            copy_name(Path::new("/example/IMG 0001.JPG"), Kind::Picture),
            "IMG 0001 (reduced).JPG"
        );
        assert_eq!(
            copy_name(Path::new("/example/scan"), Kind::Pdf),
            "scan (reduced).pdf"
        );
        assert!(
            op::valid_file_name(&copy_name(Path::new("/example/l'été.png"), Kind::Picture)).is_ok()
        );
    }

    #[test]
    fn sizes_are_said_as_people_read_them() {
        assert_eq!(sized(1_234_567), "1.2 MB");
        assert_eq!(sized(640_400), "640 KB");
        assert_eq!(sized(512), "512 bytes");
    }
}
