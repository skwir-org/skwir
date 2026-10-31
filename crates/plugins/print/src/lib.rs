//! skwir plugin: print (ADR-111). What a preview shows printed through the
//! system's own dialog (`skwir_platform::print`), or saved as a PDF beside
//! the file: the file itself when it is a PDF, the PDF a document's reader
//! reads (an office document converted), else one the print helper makes
//! of what the preview shows (`make`). The dialog chooses the pages, the
//! sides and the order; this plugin applies them to the PDF it hands over,
//! as an application printing through the portal does.
//!
//! Printing waits for the user and for the helpers: it runs on a worker
//! and says how it went in a notification. An agent's print asks first,
//! as any command that acts does; Save as PDF is a plan.
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock, Weak};

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
use skwir_core::plugin::Plugin;
use skwir_core::print::{PageSet, Pages};
use skwir_core::registry::{KeyBinding, MenuItem};
use skwir_core::scheduler::Priority;
use skwir_core::workspace::PaneId;

mod make;

pub use make::Booklet;
use make::{Layout, Shown, display_name};

const ID: &str = "print";

/// The step's kind in a plan.
const KIND: &str = "print_pdf";

/// What the file menu offers it for: one file.
const PRINTS: &str = "single file !place";

/// The context menu's submenu of the ways to print.
const PRINT: &str = "Print";

/// The menu of the preview's More button and of its own context menu,
/// whose items the window runs with what the preview shows
/// (`ui.preview_invoke`).
const PREVIEW: &str = "preview";

/// The app the plugin registered with, for Save as PDF's step, which
/// reaches previews as the window does; a step is data, not the app's.
static APP: RwLock<Option<Weak<App>>> = RwLock::new(None);

/// What a print is asked with, by the window or a script: the file, and
/// what its preview shows of it.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct PrintArgs {
    /// The file to print; the selection's one without.
    #[serde(default)]
    pub entries: Vec<EntryArg>,
    #[serde(default)]
    pub pane: Option<PaneId>,
    /// The page the preview shows (from 1): what "Current page" prints.
    #[serde(default)]
    pub page: Option<u32>,
    /// What is selected in the preview: what "Selection" prints, alone.
    #[serde(default)]
    pub selection: Option<String>,
}

/// Save as PDF: the file, the pages (`1-3, 7, 10-`, all without), where.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
pub struct SaveArgs {
    #[serde(flatten)]
    pub print: PrintArgs,
    /// Pages typed, from 1 (`1-3, 7, 10-`); every page without.
    #[serde(default)]
    pub pages: Option<String>,
    /// The folder the PDF is made in; the file's own without.
    #[serde(default)]
    pub dest: Option<PathArg>,
}

/// Print as Booklet: what Print is asked with, and how it folds. Asked
/// from the window without `asked`, it asks first (`ui.choose`).
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
pub struct BookletArgs {
    #[serde(flatten)]
    pub print: PrintArgs,
    /// Pages folded together, a multiple of 4 (16: four sheets); all of
    /// them in one booklet without.
    #[serde(default)]
    pub signature: Option<u32>,
    /// Fit the pages' ink rather than their whole page (one box for every
    /// page, so they scale alike).
    #[serde(default)]
    pub trim: bool,
    /// Its signature and trim chosen: print without asking.
    #[serde(default)]
    pub asked: bool,
}

/// Save as Booklet: what Save as PDF is asked with, and how it folds.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
pub struct SaveBookletArgs {
    #[serde(flatten)]
    pub save: SaveArgs,
    #[serde(default)]
    pub signature: Option<u32>,
    #[serde(default)]
    pub trim: bool,
}

#[derive(Default)]
pub struct PrintPlugin;

impl Plugin for PrintPlugin {
    fn id(&self) -> &'static str {
        ID
    }

    fn title(&self) -> &'static str {
        "Printing"
    }

    fn description(&self) -> &'static str {
        "Print what a preview shows through the system's dialog, as it is or as a booklet, or save it as a PDF"
    }

    fn category(&self) -> skwir_core::plugin::Category {
        skwir_core::plugin::Category::Files
    }

    fn capabilities(&self) -> skwir_core::permissions::Capabilities {
        use skwir_core::permissions::Capability;
        // Reads what it prints, starts its helpers, writes the PDFs saved.
        skwir_core::permissions::Capabilities::of(&[
            Capability::Read,
            Capability::Write,
            Capability::Programs,
        ])
    }

    fn register(&self, app: &App) {
        if let Ok(mut held) = APP.write() {
            *held = Some(app.weak());
        }
        app.ops.register::<MakePdf>(ID);
        app.commands.register::<PrintArgs, _>(
            ID,
            Topic::Files,
            "print.print",
            "Print…",
            "Print a file as its preview shows it, through the system's print dialog: the pages, sides and order chosen there; `page` is the page shown (Current page), `selection` what is selected (Selection)",
            |a, app| {
                start(app, &a, None);
                Ok(Outcome::Done)
            },
        );
        app.commands.register::<BookletArgs, _>(
            ID,
            Topic::Files,
            "print.booklet",
            "Print as Booklet…",
            "Print a file as its preview shows it, folded as a booklet: two pages to a side of the sheet chosen in the system's dialog, in the order they fold in, in signatures of `signature` pages (one booklet without), white margins trimmed with `trim`; without `asked`, the window asks first",
            |a, app| {
                if a.asked {
                    let booklet = Booklet {
                        signature: a.signature,
                        trim: a.trim,
                        turn_backs: false,
                    };
                    start(app, &a.print, Some(booklet));
                } else {
                    ask_booklet(app, &a.print);
                }
                Ok(Outcome::Done)
            },
        );
        app.commands
            .register::<SaveArgs, _>(
                ID,
                Topic::Files,
                "print.save_pdf",
                "Save as PDF",
                "Make a PDF of a file as its preview shows it, beside it or in `dest`: `pages` typed (1-3, 7, 10-), else every page (plan)",
                |a, app| Ok(Outcome::Plan(save_plan(a, app, None)?)),
            )
            .plans();
        app.commands
            .register::<SaveBookletArgs, _>(
                ID,
                Topic::Files,
                "print.save_booklet",
                "Save as Booklet",
                "Make a PDF of a file as its preview shows it folded as a booklet, beside it or in `dest`, on the paper of the system's locale: `signature` pages folded together (one booklet without), white margins trimmed with `trim` (plan)",
                |a, app| {
                    let booklet = Booklet {
                        signature: a.signature,
                        trim: a.trim,
                        turn_backs: false,
                    };
                    Ok(Outcome::Plan(save_plan(a.save, app, Some(booklet))?))
                },
            )
            .plans();
        menus(app);
        // Ctrl+P prints the file selected, or what the preview shows.
        app.registry.keymap.add(
            ID,
            KeyBinding {
                keys: "ctrl-p".into(),
                command: Invocation::new("print.print"),
                when: Some("pane single file".into()),
            },
        );
        app.registry.keymap.add(
            ID,
            KeyBinding {
                keys: "ctrl-p".into(),
                command: Invocation::new("ui.preview_invoke").with("command", "print.print"),
                when: Some("preview".into()),
            },
        );
    }
}

/// The context menu's Print submenu (3xx, made from it), and the
/// preview's menu.
fn menus(app: &App) {
    for (menu, when, submenu) in [
        ("context", Some(PRINTS), Some(PRINT)),
        (PREVIEW, Some("pages|picture|text"), None),
    ] {
        let item = |title: &str, icon: &str, order: i32, command: &str| {
            let invocation = if menu == PREVIEW {
                Invocation::new("ui.preview_invoke").with("command", command)
            } else {
                Invocation::new(command)
            };
            let mut item = MenuItem::new(menu, title, icon, order, invocation);
            if let Some(when) = when {
                item = item.when(when);
            }
            if let Some(submenu) = submenu {
                item = item.submenu(submenu);
            }
            item
        };
        app.registry
            .menus
            .add(ID, item("Print…", "printer", 340, "print.print"));
        app.registry.menus.add(
            ID,
            item("Print as Booklet…", "reading", 341, "print.booklet"),
        );
        app.registry
            .menus
            .add(ID, item("Save as PDF", "file-plus", 342, "print.save_pdf"));
        app.registry.menus.add(
            ID,
            item("Save as Booklet", "reading", 343, "print.save_booklet"),
        );
    }
}

/// The file a call names, and what its preview shows.
fn target(app: &App, args: &PrintArgs) -> anyhow::Result<(EntryRef, Shown)> {
    let entries = app.targets(args.pane, args.entries.clone())?;
    let [entry] = entries.as_slice() else {
        anyhow::bail!("Print one file at a time");
    };
    Ok((
        entry.clone(),
        Shown {
            page: args.page,
            selection: args.selection.clone().filter(|text| !text.is_empty()),
        },
    ))
}

/// Ask how the booklet folds, then print it: one booklet or signatures,
/// and whether white margins are trimmed (the choice's switch).
fn ask_booklet(app: &App, args: &PrintArgs) {
    let mut given = serde_json::to_value(args).unwrap_or_else(|_| serde_json::json!({}));
    given["asked"] = serde_json::json!(true);
    let choice = |title: &str, detail: &str, signature: Option<u32>| {
        let mut args = given.clone();
        if let Some(signature) = signature {
            args["signature"] = serde_json::json!(signature);
        }
        serde_json::json!({
            "title": title,
            "detail": detail,
            "command": { "name": "print.booklet", "args": args },
        })
    };
    app.request_front(Invocation::with_args(
        "ui.choose",
        &serde_json::json!({
            "title": "Fold as",
            "choices": [
                choice("One booklet", "Every sheet folded together, one inside the other", None),
                choice("Signatures of 16 pages", "Four sheets folded together, then the next four", Some(16)),
                choice("Signatures of 32 pages", "Eight sheets folded together, then the next eight", Some(32)),
            ],
            "option": { "title": "Trim white margins", "field": "trim" },
        }),
    ));
}

/// Print on a worker: the dialog, the PDF made, the pages applied, handed
/// to the system; how it went said in a notification.
fn start(app: &App, args: &PrintArgs, booklet: Option<Booklet>) {
    let (entry, shown) = match target(app, args) {
        Ok(target) => target,
        Err(error) => {
            app.notify(NotifyLevel::Error, format!("{error:#}"));
            return;
        }
    };
    let weak = app.weak();
    app.scheduler.spawn(
        Priority::Normal,
        "print",
        app.lifetime().child(),
        move || {
            let Some(app) = weak.upgrade() else { return };
            let cancel = app.lifetime().child();
            let name = display_name(&entry.path);
            match print(&app, &entry, &shown, booklet, &cancel) {
                Ok(Some(said)) => app.notify(NotifyLevel::Info, said),
                Ok(None) => {}
                Err(error) => app.notify(
                    NotifyLevel::Error,
                    format!("“{name}” was not printed: {error:#}"),
                ),
            }
            app.wake();
        },
    );
}

/// Ask, make, send: what to say, `None` when the dialog was cancelled.
fn print(
    app: &Arc<App>,
    entry: &EntryRef,
    shown: &Shown,
    booklet: Option<Booklet>,
    cancel: &CancelToken,
) -> anyhow::Result<Option<String>> {
    let path = entry.resolve()?;
    let name = display_name(&path);
    let ask = skwir_platform::print::Ask {
        title: name.clone(),
        current_page: shown.page.is_some(),
        selection: shown.selection.is_some(),
    };
    let Some(chosen) = skwir_platform::print::ask(&ask, cancel)? else {
        return Ok(None);
    };
    let work = tempfile::Builder::new()
        .prefix("skwir-print-")
        .tempdir()
        .context("A folder to make the document in")?;
    // Selection asked: what is selected, alone; else nothing of it.
    let shown = Shown {
        page: shown.page,
        selection: shown
            .selection
            .clone()
            .filter(|_| chosen.pages == Pages::Selection),
    };
    // A printer turning the sheet over its long edge turns a booklet's
    // backs upside down: they are turned back.
    let booklet = booklet.map(|booklet| Booklet {
        turn_backs: chosen.duplex == skwir_core::print::Duplex::LongEdge,
        ..booklet
    });
    let layout = Layout {
        paper: chosen.paper,
        pages: chosen.pages.clone(),
        page_set: chosen.page_set,
        reverse: chosen.reverse,
        booklet,
    };
    let ready = make::ready(app, &path, &shown, &layout, work.path(), cancel)?;
    skwir_platform::print::send(&chosen, &name, &ready.pdf, cancel)?;
    let pages = if ready.pages == 1 {
        "1 page".to_owned()
    } else {
        format!("{} pages", ready.pages)
    };
    let to = match (&chosen.to_file, &chosen.printer) {
        (Some(file), _) => format!("to “{}”", display_name(file)),
        (None, Some(printer)) => format!("to {printer}"),
        (None, None) => "to the printer".to_owned(),
    };
    let mut said = format!("Sent {pages} of “{name}” {to}");
    if booklet.is_some() && chosen.duplex == skwir_core::print::Duplex::Simplex {
        said = format!(
            "{said}. A booklet is printed on both sides: on one side, print the odd sides, turn the sheets over, then the even"
        );
    }
    if let Some(notice) = ready.notice {
        said = format!("{said}. {notice}");
    }
    Ok(Some(said))
}

/// Save as PDF's plan: one step making the PDF beside the file.
fn save_plan(args: SaveArgs, app: &App, booklet: Option<Booklet>) -> anyhow::Result<Plan> {
    let (entry, shown) = target(app, &args.print)?;
    let pages = match args
        .pages
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        Some(text) => Pages::Ranges(skwir_core::print::parse(text).map_err(anyhow::Error::msg)?),
        None if shown.selection.is_some() => Pages::Selection,
        None => Pages::All,
    };
    let dest_dir = match args.dest {
        Some(PathArg(dest)) => dest,
        None => entry
            .path
            .parent()
            .context("The file is in no folder")?
            .to_owned(),
    };
    let name = pdf_name(&entry.path, booklet.is_some());
    let step = MakePdf {
        entry,
        dest_dir,
        name,
        pages,
        page: shown.page,
        selection: shown.selection,
        booklet,
    };
    let what = if booklet.is_some() { "booklet" } else { "PDF" };
    let title = format!("save “{}” as {what}", display_name(&step.entry.path));
    let mut plan = Plan::new(title);
    plan.push(step);
    Ok(plan)
}

/// What the PDF of `path` is named: its name with `.pdf` for its own
/// ending, a PDF's own name marked as printed, a booklet's as one.
fn pdf_name(path: &Path, booklet: bool) -> OsString {
    let stem = path.file_stem().unwrap_or(path.as_os_str()).to_owned();
    let is_pdf = path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"));
    let mut name = stem;
    if booklet {
        name.push(" (booklet)");
    } else if is_pdf {
        name.push(" (printed)");
    }
    name.push(".pdf");
    name
}

/// Make the PDF of `entry` as its preview shows it, the `pages` of it, in
/// `dest_dir` as `name`, on the paper of this system's locale.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MakePdf {
    pub entry: EntryRef,
    #[serde(with = "skwir_core::os_path")]
    #[schemars(with = "PathArg")]
    pub dest_dir: PathBuf,
    #[serde(with = "skwir_core::os_path")]
    #[schemars(with = "PathArg")]
    pub name: OsString,
    #[serde(default)]
    pub pages: Pages,
    /// The page the preview showed (from 1), for `Pages::Current`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<u32>,
    /// What was selected, for `Pages::Selection`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection: Option<String>,
    /// Folded as a booklet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub booklet: Option<Booklet>,
}

impl MakePdf {
    fn target(&self) -> PathBuf {
        self.dest_dir.join(&self.name)
    }
}

impl op::Kind for MakePdf {
    const KIND: &'static str = KIND;
}

impl op::Effect for MakePdf {
    fn footprint(&self, _: &op::footprint::Volumes) -> Option<op::footprint::Footprint> {
        Some(
            op::footprint::Footprint::default()
                .read(&self.entry.path)
                .made(&self.target())
                .heavy_if(true),
        )
    }

    fn describe(&self) -> Change {
        let (what, pdf) = (display_name(&self.entry.path), display_name(&self.target()));
        Change::Create {
            path: self.target(),
            directory: false,
            words: Some(Words {
                will: format!("save “{what}” as “{pdf}”"),
                did: format!("saved “{what}” as “{pdf}”"),
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
            words: ("Save as PDF", "saved as PDF"),
        })
    }

    fn made_as(&self, name: OsString) -> Option<Box<dyn Operation>> {
        Some(Box::new(MakePdf {
            name,
            ..self.clone()
        }))
    }

    fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let path = self.entry.resolve()?;
        let target = self.target();
        let app = APP
            .read()
            .ok()
            .and_then(|held| held.as_ref().and_then(Weak::upgrade))
            .context("Saving as PDF needs the print plugin on")?;
        let staging = ctx.stage(&self.dest_dir)?;
        let work = staging.path().join("work");
        std::fs::create_dir_all(&work)?;
        let layout = Layout {
            paper: skwir_platform::print::paper(),
            pages: self.pages.clone(),
            page_set: PageSet::All,
            reverse: false,
            booklet: self.booklet,
        };
        let shown = Shown {
            page: self.page,
            selection: self.selection.clone(),
        };
        let result = make::ready(&app, &path, &shown, &layout, &work, ctx.cancel_token())
            .and_then(|ready| {
                // The file itself, printed whole: copied, never moved.
                let made = staging.path().join("made.pdf");
                std::fs::copy(&ready.pdf, &made)?;
                Ok(made)
            })
            .and_then(|made| {
                ctx.check_cancelled()?;
                staging.place(&made, &target).with_context(|| {
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
        if ctx.cancel_token().is_cancelled() && result.is_err() {
            return Err(op::Cancelled.into());
        }
        result?;
        let made = EntryRef::from_path(&target)
            .with_context(|| format!("“{}” went missing as it was made", display_name(&target)))?;
        // Undone to the trash, as a copy is: it may have been changed since.
        Ok(Some(Box::new(op::Trash { entry: made })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pdf_is_named_after_its_file() {
        assert_eq!(pdf_name(Path::new("/example/notes.md"), false), "notes.pdf");
        assert_eq!(
            pdf_name(Path::new("/example/Makefile"), false),
            "Makefile.pdf"
        );
        assert_eq!(
            pdf_name(Path::new("/example/notes.md"), true),
            "notes (booklet).pdf"
        );
        assert_eq!(
            pdf_name(Path::new("/example/report.PDF"), false),
            "report (printed).pdf"
        );
    }
}
