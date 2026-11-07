//! skwir plugin: print (ADR-111). What a preview shows printed, or saved
//! as a PDF beside it: the file itself when it is a PDF, the PDF a
//! document's reader reads (an office document converted), else one the
//! print helper makes of what the preview shows (`make`).
//!
//! One Print… (the file menu, the preview's, Ctrl+P) asks Skwir's own
//! dialog first (`dialog`): to a printer or a PDF, which pages, as shown,
//! folded as a booklet or as a contact sheet. A printer is then chosen in
//! the system's dialog (`skwir_platform::print`), with the paper, the
//! copies and the sides; Skwir applies the pages, the sides and the order
//! to the PDF it hands over, as an application printing through the portal
//! does. Nothing selected, Print Listing… prints what the pane lists,
//! wherever it is: a folder, a search, a tag, the trash (`listing`).
//! Saved, a PDF goes where the dialog says, beside what it is of unless
//! another folder is chosen (`plan_of`, `dialog::Saved`).
//!
//! Printing waits for the user and for the helpers: it runs on a worker
//! and says how it went in a notification. An agent's print asks first,
//! as any command that acts does; Save as PDF is a plan.
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context as _;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skwir_core::App;
use skwir_core::app::NotifyLevel;
use skwir_core::cancel::CancelToken;
use skwir_core::command::{Invocation, Outcome, Topic};
use skwir_core::location::Location;
use skwir_core::op::{
    self, Change, EntryArg, EntryRef, Made, Operation, Plan, StepContext, StepStatus, Words,
};
use skwir_core::os_path::PathArg;
use skwir_core::plugin::{AppRef, Plugin};
use skwir_core::print::{PageSet, Pages};
use skwir_core::registry::{KeyBinding, MenuItem};
use skwir_core::scheduler::{Lane, Priority};
use skwir_core::shrink::Quality;
use skwir_core::workspace::PaneId;

mod dialog;
mod listing;
mod make;

pub use make::Booklet;
use make::{Layout, Shown, display_name};

const ID: &str = "print";

/// The step's kind in a plan.
const KIND: &str = "print_pdf";

/// What the file menu offers Print… for: files, printed as one job.
const FILES: &str = "file !place";

/// What it offers a pane's listing for: nothing selected, wherever the
/// pane is (a folder, a search, a tag, the trash).
const LISTS: &str = "empty";

/// The menu of the preview's More button and of its own context menu,
/// whose items the window runs with what the preview shows
/// (`ui.preview_invoke`).
const PREVIEW: &str = "preview";

/// How what is printed is laid out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Arrange {
    /// Each page as the preview shows it, on a sheet.
    #[default]
    AsShown,
    /// Two pages to a side, folded (`signature`, `trim`).
    Booklet,
    /// The pictures among the files, several to a page, each named.
    ContactSheet,
    /// The entries a pane shows, as a table (`pane`; no entries).
    Listing,
}

/// What Print… and Save as PDF are asked with, by the window's dialog or
/// a script: what is printed and how.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct PrintArgs {
    /// The files, as one job in their order; the selection's without.
    #[serde(default)]
    pub entries: Vec<EntryArg>,
    #[serde(default)]
    pub pane: Option<PaneId>,
    /// The page the preview shows (from 1): what `current` prints.
    #[serde(default)]
    pub page: Option<u32>,
    /// How many pages the preview shows it with: said in the dialog.
    #[serde(default)]
    pub count: Option<u32>,
    /// What is selected in the preview: what `selection` prints, alone.
    #[serde(default)]
    pub selection: Option<String>,
    /// Which pages: `all` (without), `current`, `selection`, or typed
    /// from 1 (`1-3, 7, 10-`).
    #[serde(default)]
    pub pages: Option<String>,
    #[serde(default)]
    pub layout: Arrange,
    /// A booklet's pages folded together, a multiple of 4 (16: four
    /// sheets); all of them in one booklet without.
    #[serde(default)]
    pub signature: Option<u32>,
    /// A booklet's pages fit by their ink rather than their whole page
    /// (one box for every page, so they scale alike).
    #[serde(default)]
    pub trim: bool,
    /// A booklet's pages to a side of a sheet: 2 (folded once, without), 4
    /// (folded twice, 8 pages a sheet) or 8 (three times, 16 a sheet).
    #[serde(default)]
    pub per_side: Option<u32>,
    /// Chosen already: Print… goes straight to the system's dialog
    /// instead of asking Skwir's first.
    #[serde(default)]
    pub asked: bool,
}

/// Save as PDF: what Print… is asked with, and where.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
pub struct SaveArgs {
    #[serde(flatten)]
    pub print: PrintArgs,
    /// The folder the PDFs are made in, a whole path; each file's own
    /// without (a contact sheet's: its first file's; a listing's: the
    /// folder listed, else the one the place is of, else Documents:
    /// `listing_folder`).
    #[serde(default)]
    pub dest: Option<PathArg>,
    /// Several files saved as one PDF, in their order, named after the
    /// first, as they print together.
    #[serde(default)]
    pub together: bool,
    /// The PDF's name, of one PDF (a file, several `together`, a contact
    /// sheet, a listing); after what it is of without. `.pdf` is added
    /// when it has no such ending.
    #[serde(default)]
    pub name: Option<PathArg>,
    /// Each PDF made smaller as Reduce Size makes a copy (ADR-112): its
    /// images at most this quality's dpi (`print`, `screen`, `small`).
    #[serde(default)]
    pub smaller: Option<Quality>,
    /// The paper it is laid out on: `a4`, `letter`, `a5`, `a3`, `legal`;
    /// the system's without.
    #[serde(default)]
    pub paper: Option<String>,
}

/// The print plugin, one for each app it is added to: what it holds
/// there, shared by what it registers (`Held`).
#[derive(Default)]
pub struct PrintPlugin {
    held: Held,
}

/// What the plugin holds for its app, cloned into its commands and steps:
/// the app, for Save as PDF's step, which reaches previews as the window
/// does (a step is data, not the app's: `AppRef`), and the dialog's last
/// choices. Never a static: two apps in one process hold their own.
#[derive(Clone, Default)]
struct Held {
    app: AppRef,
    last: dialog::Last,
}

impl Plugin for PrintPlugin {
    fn id(&self) -> &'static str {
        ID
    }

    fn title(&self) -> &'static str {
        "Printing"
    }

    fn description(&self) -> &'static str {
        "Print what a preview shows, as it is, as a booklet or as a contact sheet, or save it as a PDF"
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
        self.held.app.hold(app);
        // A step rebuilt from its data (a plan kept, redone) reaches the
        // app as one made here does.
        let rebuilt = self.held.app.clone();
        app.ops.register_with(ID, KIND, move |data| {
            let step = MakePdf {
                app: rebuilt.clone(),
                ..serde_json::from_value(data)?
            };
            Ok(Box::new(step))
        });
        let held = self.held.clone();
        let print = app.commands.register::<PrintArgs, _>(
            ID,
            Topic::Files,
            "print.print",
            "Print…",
            "Print files as their previews show them, one job in their order (`layout`: as shown, a booklet folded in signatures of `signature` pages with `trim`, a contact sheet of the pictures, a pane's listing), the `pages` chosen; the window asks first in Skwir's dialog without `asked`, then the system's dialog chooses the printer, the paper and the sides",
            move |a, app| {
                if a.asked || !app.window_shown() {
                    match request(app, &a) {
                        Ok(request) => start(app, request),
                        Err(error) => app.notify(NotifyLevel::Error, format!("{error:#}")),
                    }
                } else {
                    match resolved(app, a) {
                        Ok(args) => ask_first(app, args, held.clone()),
                        Err(error) => app.notify(NotifyLevel::Error, format!("{error:#}")),
                    }
                }
                Ok(Outcome::Done)
            },
        );
        // Ctrl+P and Print Listing… with nothing selected.
        print.forms([skwir_core::command::Form::new(
            serde_json::json!({ "layout": "listing" }),
            "Print Listing…",
        )]);
        let saving = self.held.app.clone();
        app.commands
            .register::<SaveArgs, _>(
                ID,
                Topic::Files,
                "print.save_pdf",
                "Save as PDF",
                "Make a PDF of each file as its preview shows it, beside it or in `dest` (one of them all with `together`), on the paper of the system's locale, laid out and of the pages Print… takes (plan)",
                move |a, app| {
                    let request = request(app, &a.print)?;
                    let dest = a
                        .dest
                        .as_ref()
                        .map(|PathArg(dest)| skwir_core::os_path::absolute(dest))
                        .transpose()?;
                    if let Some(paper) = a.paper.as_deref() {
                        anyhow::ensure!(
                            skwir_core::print::Paper::named(paper).is_some(),
                            "“{paper}” is no paper: a4, letter, a5, a3 or legal"
                        );
                    }
                    let name = a
                        .name
                        .as_ref()
                        .map(|PathArg(name)| pdf_named(name.as_os_str()))
                        .transpose()?;
                    Ok(Outcome::Plan(plan_of(
                        app,
                        &request,
                        &Saving {
                            dest,
                            name: name.as_deref(),
                            together: a.together,
                            smaller: a.smaller,
                            paper: a.paper.as_deref(),
                            app: &saving,
                        },
                    )?))
                },
            )
            .plans();
        menus(app);
        // Ctrl+P prints the files selected, the pane's listing with none,
        // or what the preview shows.
        for (when, command) in [
            ("pane file !place", Invocation::new("print.print")),
            (
                "pane empty",
                Invocation::new("print.print").with("layout", "listing"),
            ),
            (
                "preview",
                Invocation::new("ui.preview_invoke").with("command", "print.print"),
            ),
        ] {
            app.registry.keymap.add(
                ID,
                KeyBinding {
                    keys: "ctrl-p".into(),
                    command,
                    when: Some(when.into()),
                },
            );
        }
    }

    fn on_disable(&self, _app: &App) {
        self.held.last.forget();
        // A step of a plan made before is refused from here on.
        self.held.app.release();
    }
}

/// Print… in the file menu's group of what is made from it (3xx), for
/// files and in the preview's menu; Print Listing… for the pane's
/// listing, named as the palette names it, so what is printed is said.
fn menus(app: &App) {
    for (menu, when, title, invocation) in [
        ("context", FILES, "Print…", Invocation::new("print.print")),
        (
            "context",
            LISTS,
            "Print Listing…",
            Invocation::new("print.print").with("layout", "listing"),
        ),
        (
            PREVIEW,
            "pages|picture|text",
            "Print…",
            Invocation::new("ui.preview_invoke").with("command", "print.print"),
        ),
    ] {
        app.registry.menus.add(
            ID,
            MenuItem::new(menu, title, "printer", 340, invocation).when(when),
        );
    }
}

/// `args` with what they name taken now, as the dialog starts: the pane,
/// and the files selected there (a listing names none), so the print is
/// of what was asked whatever the window shows by the time it is chosen.
fn resolved(app: &App, args: PrintArgs) -> anyhow::Result<PrintArgs> {
    let pane = app.pane_or_active(args.pane);
    let entries = if args.layout == Arrange::Listing {
        Vec::new()
    } else {
        app.targets(Some(pane), args.entries)?
            .into_iter()
            .map(EntryArg::from)
            .collect()
    };
    Ok(PrintArgs {
        entries,
        pane: Some(pane),
        ..args
    })
}

/// What is printed or saved, resolved.
struct Request {
    what: What,
    pages: Pages,
    booklet: Option<Booklet>,
}

/// What a request prints.
enum What {
    /// Files as their previews show them, one job in their order; of one,
    /// the page shown and what is selected.
    Files {
        entries: Vec<EntryRef>,
        shown: Shown,
    },
    /// A contact sheet of the pictures among these files.
    Sheet(Vec<EntryRef>),
    /// The listing a pane shows, as it stands when made.
    Listing(PaneId),
}

/// What `args` ask printed: their files (the selection's without) or
/// their pane's listing, the pages read, the booklet's folds.
fn request(app: &App, args: &PrintArgs) -> anyhow::Result<Request> {
    let (what, shown) = match args.layout {
        Arrange::Listing => (
            What::Listing(app.pane_or_active(args.pane)),
            Shown::default(),
        ),
        Arrange::ContactSheet => (
            What::Sheet(app.targets(args.pane, args.entries.clone())?),
            Shown::default(),
        ),
        Arrange::AsShown | Arrange::Booklet => {
            let entries = app.targets(args.pane, args.entries.clone())?;
            anyhow::ensure!(!entries.is_empty(), "Nothing to print");
            // What the preview shows is of the one file it shows.
            let shown = if entries.len() == 1 {
                Shown {
                    page: args.page,
                    selection: args.selection.clone().filter(|text| !text.is_empty()),
                }
            } else {
                Shown::default()
            };
            (
                What::Files {
                    entries,
                    shown: shown.clone(),
                },
                shown,
            )
        }
    };
    let pages = match args
        .pages
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        None | Some("all") => Pages::All,
        Some("current") => {
            anyhow::ensure!(shown.page.is_some(), "No page of it is shown");
            Pages::Current
        }
        Some("selection") => {
            anyhow::ensure!(shown.selection.is_some(), "Nothing of it is selected");
            Pages::Selection
        }
        Some(typed) => Pages::Ranges(skwir_core::print::parse(typed).map_err(anyhow::Error::msg)?),
    };
    // What is selected is printed only when it is asked, alone.
    let what = match what {
        What::Files { entries, shown } => What::Files {
            entries,
            shown: Shown {
                selection: shown.selection.filter(|_| pages == Pages::Selection),
                ..shown
            },
        },
        other => other,
    };
    let per_side = match args.per_side {
        None => skwir_core::print::booklet::PerSide::default(),
        Some(count) => skwir_core::print::booklet::PerSide::of_count(count)
            .with_context(|| format!("{count} pages a side: a booklet has 2, 4 or 8"))?,
    };
    let booklet = (args.layout == Arrange::Booklet).then_some(Booklet {
        per_side,
        signature: args.signature,
        trim: args.trim,
        turn_backs: false,
    });
    Ok(Request {
        what,
        pages,
        booklet,
    })
}

/// How long the dialog waits for a PDF's pages to be counted before it
/// asks without saying them.
const COUNTING: std::time::Duration = std::time::Duration::from_millis(400);

/// What the dialog may offer of what `args` name (resolved: their files,
/// or their pane's listing), and where a PDF of it goes. On a worker: it
/// reads the user's folders (Documents).
fn offers(
    app: &App,
    args: &PrintArgs,
    last: &dialog::Last,
    cancel: &CancelToken,
) -> anyhow::Result<dialog::Offers> {
    let pane = app.pane_or_active(args.pane);
    let documents = documents();
    let other = other_folder(app, pane);
    // The folders chosen before, those still folders (a server that
    // answers slowly keeps its own).
    let (opens, recent) = dialog::kept_folders(app, last);
    let there = |folder: &PathBuf| is_folder(app, folder, cancel) != Some(false);
    let opens = opens.filter(there);
    let recent: Vec<PathBuf> = recent.into_iter().filter(there).collect();
    let places = sidebar_folders(app);
    if args.layout == Arrange::Listing {
        let (location, filter) = shown(app, pane)?;
        let own = own_folder(app, &location);
        let said = match (location.is_path(), own.is_some()) {
            (true, _) => "The folder listed",
            (false, true) => "The folder it is of",
            (false, false) => "It has no folder of its own",
        };
        let folder = own.or_else(|| documents.clone());
        let named = listing_named(app, &location, filter.as_deref());
        let file = listing_name(&location, &named)
            .to_string_lossy()
            .into_owned();
        return Ok(dialog::Offers {
            what: format!("the listing of “{named}”"),
            subject: dialog::Subject::Listing,
            saved: dialog::Saved {
                beside: folder.as_deref().map_or_else(
                    || "In the folder listed".to_owned(),
                    |folder| format!("In “{}”", display_name(folder)),
                ),
                folder,
                said: said.to_owned(),
                other,
                documents,
                opens,
                recent,
                places,
                named: dialog::Names {
                    shown: file,
                    ..dialog::Names::default()
                },
            },
            ..dialog::Offers::default()
        });
    }
    let typed = app.targets_typed(args.pane, args.entries.clone())?;
    // Beside them, where they are in one folder; else each beside its own.
    let mut folders = typed.iter().map(|(entry, _)| entry.path.parent());
    let first = folders.next().flatten();
    let folder = first
        .filter(|first| folders.all(|folder| folder == Some(*first)))
        .map(Path::to_owned);
    // Named after the first file, as one PDF of several is.
    let first_named = |booklet: bool| {
        typed.first().map_or_else(String::new, |(entry, _)| {
            pdf_name(&entry.path, booklet)
                .to_string_lossy()
                .into_owned()
        })
    };
    let saved = dialog::Saved {
        said: match typed.len() {
            1 => "Beside it",
            _ => "Beside them",
        }
        .to_owned(),
        beside: match (&folder, typed.len()) {
            (Some(_), 1) => "Beside it",
            (Some(_), _) => "Beside them",
            (None, _) => "Beside each file; one PDF of them all beside the first",
        }
        .to_owned(),
        folder,
        other,
        documents,
        opens,
        recent,
        places,
        named: dialog::Names {
            shown: first_named(false),
            booklet: first_named(true),
            sheet: SHEET.to_owned(),
        },
    };
    let mime = |name: &Option<OsString>| name.as_deref().and_then(skwir_platform::mime::by_name);
    let what = match typed.as_slice() {
        [(entry, _)] => format!("“{}”", display_name(&entry.path)),
        many => format!("{} files", many.len()),
    };
    let several = typed.len() > 1;
    let sheet = several
        && typed.iter().any(|(_, name)| {
            mime(name).is_some_and(|mime| {
                skwir_platform::mime::is_kind(&mime, skwir_platform::mime::Kind::Image)
            })
        });
    let count = match typed.as_slice() {
        [(entry, name)] => args.count.or_else(|| {
            let pdf = mime(name).is_some_and(|mime| mime.as_str() == "application/pdf");
            pdf.then(|| counted(app, entry, cancel)).flatten()
        }),
        _ => None,
    };
    Ok(dialog::Offers {
        what,
        saved,
        page: args.page.filter(|_| !several),
        selection: !several
            && args
                .selection
                .as_deref()
                .is_some_and(|text| !text.is_empty()),
        count,
        subject: if several {
            dialog::Subject::Several { pictures: sheet }
        } else {
            dialog::Subject::One
        },
    })
}

/// How many pages the PDF `entry` has, if its reader says so within
/// `COUNTING` (a file on a server that answers slowly is not waited for):
/// found and counted in its remote's lane, which the wait bounds.
fn counted(app: &App, entry: &EntryRef, cancel: &CancelToken) -> Option<u32> {
    let (entry, weak) = (entry.clone(), app.weak());
    soon(
        app,
        &entry.path.clone(),
        "print-count",
        cancel,
        move |token| {
            let app = weak.upgrade()?;
            let path = entry.resolve().ok()?;
            make::page_count(&app, &path, token).ok()
        },
    )
}

/// Whether `path` is a folder, if it is told within `COUNTING`: a folder
/// typed in the dialog is refused there when it is none, and one on a
/// server that answers slowly is taken as typed, the plan checking it
/// again.
fn is_folder(app: &App, path: &Path, cancel: &CancelToken) -> Option<bool> {
    let asked = path.to_owned();
    soon(app, path, "print-folder", cancel, move |_| {
        Some(std::fs::metadata(&asked).is_ok_and(|meta| meta.is_dir()))
    })
}

/// What `work` answers, run in the lane of `path`, if it answers within
/// `COUNTING`; cancelled after.
fn soon<T: Send + 'static>(
    app: &App,
    path: &Path,
    label: &'static str,
    cancel: &CancelToken,
    work: impl FnOnce(&CancelToken) -> Option<T> + Send + 'static,
) -> Option<T> {
    let (said, heard) = std::sync::mpsc::sync_channel(1);
    let working = cancel.child();
    let token = working.clone();
    app.scheduler.spawn_in(
        skwir_core::fs::reach::lane(path),
        Priority::Normal,
        label,
        working.clone(),
        move || {
            let _ = said.send(work(&token));
        },
    );
    let answer = heard.recv_timeout(COUNTING).ok().flatten();
    working.cancel();
    answer
}

/// The location `pane` shows and the words filtering it.
fn shown(app: &App, pane: PaneId) -> anyhow::Result<(Location, Option<String>)> {
    let ws = app.workspace.read();
    let shown = ws.pane(pane).context("No such pane")?;
    let location = shown
        .location()
        .cloned()
        .context("The pane shows nothing")?;
    Ok((location, shown.filter().map(str::to_owned)))
}

/// What the listing of `location` is called in words: its name (a
/// folder's own, a search's, a tag's), with what filters it.
fn listing_named(app: &App, location: &Location, filter: Option<&str>) -> String {
    listing::filtered(app.location_title(location), filter)
}

/// Where the PDF of what `location` lists is saved unless another folder
/// is chosen: its own folder (`own_folder`), else the user's Documents
/// (`documents`). Reads the user's folders: on a worker, or a script's
/// thread.
fn listing_folder(app: &App, location: &Location) -> Option<PathBuf> {
    own_folder(app, location).or_else(documents)
}

/// The folder `location` lists, else the folder the place is of (a
/// search's, a tag's: its trail's `base`, else the first of its places
/// that is a folder), as its trail says, never its scheme; `None` for a
/// place of no folder (the trash, Recent).
fn own_folder(app: &App, location: &Location) -> Option<PathBuf> {
    if let Some(path) = location.path() {
        return Some(path);
    }
    app.trail(location).and_then(|trail| {
        trail
            .base
            .or_else(|| trail.crumbs.iter().find_map(|crumb| crumb.location.path()))
    })
}

/// The user's Documents folder, else their home: where what is of no
/// folder is saved. Reads the user's folders' list.
fn documents() -> Option<PathBuf> {
    skwir_platform::places::user_dirs()
        .into_iter()
        .find(|dir| dir.kind == "documents")
        .map(|dir| dir.path)
        .or_else(skwir_core::os_path::home)
}

/// The sidebar's folders, offered where a PDF goes: its places and its
/// bookmarks that are folders here and open now (no volume, no place of
/// a plugin's, none marked unavailable), each by its title, in its order.
/// Read from the places core holds (`registry.places`), whoever made
/// them.
fn sidebar_folders(app: &App) -> Vec<(String, PathBuf)> {
    app.registry
        .places
        .all()
        .into_iter()
        .filter(|place| matches!(place.group.as_str(), "places" | "bookmarks"))
        .filter(|place| place.unavailable.is_none() && place.activate.is_none())
        .filter_map(|place| Some((place.title, place.location.path()?)))
        .collect()
}

/// The folder another pane of `pane`'s tab shows (the split's other
/// side), offered as where a PDF goes, as the split's transfers go there.
fn other_folder(app: &App, pane: PaneId) -> Option<PathBuf> {
    let ws = app.workspace.read();
    let tab = ws.tab(ws.tab_of(pane)?)?;
    tab.panes
        .iter()
        .filter(|&&other| other != pane)
        .find_map(|&other| ws.pane(other)?.location()?.path())
}

/// `work` run in `lane` (a remote's, or the shared one: `fs::reach::lane`
/// of what it reads) and waited for here: what reads files runs there, so
/// a server that stopped answering holds its own lane, and what waits for
/// the user (a dialog) holds none (AGENTS.md, remote paths). An error
/// when `cancel` fires first. Never called from a job of `lane` itself,
/// which would wait for its own turn.
fn in_lane<T: Send + 'static>(
    app: &App,
    lane: Lane,
    label: &'static str,
    cancel: &CancelToken,
    work: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> anyhow::Result<T> {
    let (said, heard) = std::sync::mpsc::sync_channel(1);
    app.scheduler
        .spawn_in(lane, Priority::Normal, label, cancel.clone(), move || {
            let _ = said.send(work());
        });
    loop {
        match heard.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(done) => return done,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) if !cancel.is_cancelled() => {}
            // Cancelled, or skipped as it was before it started.
            Err(_) => return Err(op::Cancelled.into()),
        }
    }
}

/// Ask Skwir's dialog on a worker, then print what it chose, or save it
/// as a PDF: a plan applied as the user's own action is (undone to the
/// trash).
fn ask_first(app: &App, args: PrintArgs, held: Held) {
    let weak = app.weak();
    app.scheduler.spawn(
        Priority::Normal,
        "print-dialog",
        app.lifetime().child(),
        move || {
            let Some(app) = weak.upgrade() else { return };
            let cancel = app.lifetime().child();
            match chosen_then(&app, args, &held, &cancel) {
                Ok(Some(said)) => app.notify(NotifyLevel::Info, said),
                Ok(None) => {}
                Err(error) => app.notify(NotifyLevel::Error, format!("{error:#}")),
            }
            app.wake();
        },
    );
}

/// The dialog, then what it chose done: what to say, `None` when it was
/// closed (or a plan says it).
fn chosen_then(
    app: &Arc<App>,
    args: PrintArgs,
    held: &Held,
    cancel: &CancelToken,
) -> anyhow::Result<Option<String>> {
    let offers = offers(app, &args, &held.last, cancel)?;
    let is_folder = |path: &Path| is_folder(app, path, cancel);
    let Some(chosen) = dialog::ask(app, &offers, &held.last, &is_folder, cancel) else {
        return Ok(None);
    };
    let args = PrintArgs {
        pages: Some(chosen.pages),
        layout: chosen.layout,
        signature: chosen.signature,
        trim: chosen.trim,
        per_side: Some(chosen.per_side.count()),
        asked: true,
        ..args
    };
    let request = request(app, &args)?;
    match chosen.to {
        dialog::To::Printer => print(app, request, cancel),
        dialog::To::Pdf => {
            let smaller = chosen.smaller.then_some(Quality::Screen);
            app.apply_whole(plan_of(
                app,
                &request,
                &Saving {
                    dest: chosen.dest.as_deref(),
                    name: chosen.name.as_deref(),
                    together: chosen.together,
                    smaller,
                    paper: Some(chosen.paper.as_str()),
                    app: &held.app,
                },
            )?);
            Ok(None)
        }
    }
}

/// Print on a worker: the system's dialog, the PDF made, the pages
/// applied, handed to the system; how it went said in a notification.
fn start(app: &App, request: Request) {
    let weak = app.weak();
    app.scheduler.spawn(
        Priority::Normal,
        "print",
        app.lifetime().child(),
        move || {
            let Some(app) = weak.upgrade() else { return };
            let cancel = app.lifetime().child();
            match print(&app, request, &cancel) {
                Ok(Some(said)) => app.notify(NotifyLevel::Info, said),
                Ok(None) => {}
                Err(error) => app.notify(NotifyLevel::Error, format!("{error:#}")),
            }
            app.wake();
        },
    );
}

/// Print `request`: `None` when the system's dialog was closed, else what
/// to say. What reads files (a listing, the files, the helpers) runs in
/// their remote's lane; the dialog waits on this worker, holding none.
fn print(app: &Arc<App>, request: Request, cancel: &CancelToken) -> anyhow::Result<Option<String>> {
    let Request {
        what,
        pages,
        booklet,
    } = request;
    // What it is printed as, named before the dialog asks: the files by
    // their names when asked, a listing as it stands when asked.
    let (what, title, lane, made) = match what {
        What::Files { entries, shown } => {
            let (what, title) = match entries.as_slice() {
                [] => anyhow::bail!("Nothing to print"),
                [entry] => (
                    format!("“{}”", display_name(&entry.path)),
                    display_name(&entry.path),
                ),
                [first, rest @ ..] => (
                    format!("{} files", entries.len()),
                    format!("{} and {} more", display_name(&first.path), rest.len()),
                ),
            };
            let lane = skwir_core::fs::reach::lane(&entries[0].path);
            (what, title, lane, Make::Files(entries, shown))
        }
        What::Sheet(entries) => {
            let lane =
                skwir_core::fs::reach::lane(&entries.first().context("Nothing to print")?.path);
            (
                "a contact sheet".to_owned(),
                "Contact sheet".to_owned(),
                lane,
                Make::Sheet(entries),
            )
        }
        What::Listing(pane) => {
            // Its files read in their lanes; the PDF made of the table
            // reads none, in the shared one.
            let listed = listing::listing(app, pane, cancel)?;
            (
                format!("the listing of “{}”", listed.title),
                listed.title.clone(),
                Lane::SHARED,
                Make::Listing(listed),
            )
        }
    };
    let ask = skwir_platform::print::Ask {
        title: title.clone(),
    };
    let Some(chosen) = skwir_platform::print::ask(&ask, cancel)? else {
        return Ok(None);
    };
    // A booklet's backs are laid for a sheet turned over its vertical
    // edge: the short one of a landscape sheet, the long one of an upright
    // one. A printer turning it over the other turns them upside down:
    // they are turned back.
    let booklet = booklet.map(|booklet| Booklet {
        turn_backs: if booklet.per_side.landscape() {
            chosen.duplex == skwir_core::print::Duplex::LongEdge
        } else {
            chosen.duplex == skwir_core::print::Duplex::ShortEdge
        },
        ..booklet
    });
    let layout = Layout {
        paper: chosen.paper,
        pages,
        sent: chosen.pages.clone(),
        page_set: chosen.page_set,
        reverse: chosen.reverse,
        scale: chosen.scale,
        booklet,
    };
    // While it is made and sent, the window shows it, with Cancel: a
    // print is no plan, and nothing else would show it.
    let working = cancel.child();
    let task = Arc::new(app.tasks.start(format!("Print {what}"), working.clone()));
    let (weak, token, sent_as, held) = (
        app.weak(),
        working.clone(),
        chosen.clone(),
        Arc::clone(&task),
    );
    let printed = in_lane(app, lane, "print", &working, move || {
        let app = weak.upgrade().context("Skwir is closing")?;
        let work = tempfile::Builder::new()
            .prefix("skwir-print-")
            .tempdir()
            .context("A folder to make the document in")?;
        // Made, then sent: the sending its last tenth.
        let went = |fraction: f64| held.went(fraction * 0.9);
        let never = || false;
        let follow = skwir_platform::process::Follow::new(&went, &never);
        let ready = made_ready(&app, made, &title, &layout, work.path(), &token, &follow)?;
        // What a helper made is sent only as the file it is.
        make::open_made(&ready, work.path())?;
        held.went(0.9);
        let answered = skwir_platform::print::send(&sent_as, &title, &ready.pdf, &token)?;
        let notice = [ready.notice, ready.folding.map(str::to_owned)];
        Ok((
            answered,
            ready.pages,
            notice.into_iter().flatten().collect::<Vec<_>>(),
        ))
    });
    drop(task);
    // Cancelled by its card, not by Skwir closing: said, as no error.
    if working.is_cancelled() && !cancel.is_cancelled() {
        return Ok(Some(format!("{} was not printed", capitalized(&what))));
    }
    let (answered, pages, notice) =
        printed.with_context(|| format!("{} was not printed", capitalized(&what)))?;
    // Cancelled in a dialog the desktop's printing showed again: as the
    // first one closed, nothing is said.
    if answered == skwir_platform::print::Sent::Cancelled {
        return Ok(None);
    }
    let sent = match pages {
        Some(1) => "1 page of ".to_owned(),
        Some(pages) => format!("{pages} pages of "),
        None => String::new(),
    };
    let to = match (&chosen.to_file, &chosen.printer) {
        (Some(file), _) => format!("to “{}”", display_name(file)),
        (None, Some(named)) => format!("to {named}"),
        (None, None) => "to the printer".to_owned(),
    };
    let mut said = match answered {
        // What the portal answers when its back end failed, too.
        skwir_platform::print::Sent::Unconfirmed => format!(
            "Handed {sent}{what} to the desktop's printing for {}, which did not confirm it took it",
            to.strip_prefix("to ").unwrap_or(&to)
        ),
        _ => format!("Sent {sent}{what} {to}"),
    };
    if booklet.is_some() && chosen.duplex == skwir_core::print::Duplex::Simplex {
        said = format!(
            "{said}. A booklet is printed on both sides: on one side, print the odd sides, turn the sheets over, then the even"
        );
    }
    for notice in notice {
        said = format!("{said}. {notice}");
    }
    Ok(Some(said))
}

/// The paths `entries` name now.
fn resolve(entries: &[EntryRef]) -> anyhow::Result<Vec<PathBuf>> {
    Ok(entries
        .iter()
        .map(EntryRef::resolve)
        .collect::<Result<Vec<PathBuf>, _>>()?)
}

/// What a print makes its PDF of, once the dialog chose: the files found
/// as it is made.
enum Make {
    Files(Vec<EntryRef>, Shown),
    Sheet(Vec<EntryRef>),
    Listing(listing::Listed),
}

/// The PDF of `made`, titled `title`, laid out as `layout` says, in
/// `work`. Reads the files: in their lane.
fn made_ready(
    app: &App,
    made: Make,
    title: &str,
    layout: &Layout,
    work: &Path,
    cancel: &CancelToken,
    follow: &skwir_platform::process::Follow<'_>,
) -> anyhow::Result<make::Ready> {
    match made {
        Make::Files(entries, shown) => make::ready(
            app,
            &resolve(&entries)?,
            &shown,
            layout,
            work,
            cancel,
            follow,
        ),
        Make::Sheet(entries) => {
            make::sheet(&resolve(&entries)?, work, cancel).and_then(|(part, notice)| {
                let ready = make::made(app, title, vec![part], layout, work, cancel, follow)?;
                Ok(make::Ready {
                    notice: ready.notice.or(notice),
                    ..ready
                })
            })
        }
        Make::Listing(listing::Listed { title, table }) => {
            make::made(app, &title, vec![table], layout, work, cancel, follow)
        }
    }
}

/// `text` with its first letter a capital.
fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// How Save as PDF saves, besides what: where, named how, one PDF of
/// several files or one each, made smaller, on what paper.
struct Saving<'a> {
    /// The folder; where each goes by itself without.
    dest: Option<&'a Path>,
    /// The name of the one PDF made; after what it is of without.
    name: Option<&'a std::ffi::OsStr>,
    together: bool,
    smaller: Option<Quality>,
    paper: Option<&'a str>,
    /// The app its steps reach, as the plugin holds it.
    app: &'a AppRef,
}

/// What a contact sheet's PDF is named by itself.
const SHEET: &str = "Contact sheet.pdf";

/// `name`, typed for a PDF, as it is saved: `.pdf` added where it has no
/// such ending, refused where no file can be named so.
fn pdf_named(name: &std::ffi::OsStr) -> anyhow::Result<OsString> {
    let mut named = name.to_owned();
    let pdf = Path::new(name)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"));
    if !pdf {
        named.push(".pdf");
    }
    op::valid_file_name(&named)?;
    Ok(named)
}

/// Save as PDF's plan for `request`: a step making each file's PDF beside
/// it (or in `dest`), one of them all with `together`; a contact sheet's
/// beside its first file, a listing's in the folder listed, else the one
/// the place is of, else Documents (`listing_folder`); the one PDF named
/// `name` where it is given.
fn plan_of(app: &App, request: &Request, saving: &Saving<'_>) -> anyhow::Result<Plan> {
    let Saving {
        dest,
        name: given,
        together,
        smaller,
        paper,
        app: reach,
    } = *saving;
    let booklet = request.booklet;
    let what = if booklet.is_some() { "booklet" } else { "PDF" };
    let folder_of = |path: &Path| -> anyhow::Result<PathBuf> {
        match dest {
            Some(dest) => Ok(dest.to_owned()),
            None => Ok(path
                .parent()
                .context("The file is in no folder")?
                .to_owned()),
        }
    };
    let step = |of: Of, dest_dir: PathBuf, name: OsString| MakePdf {
        of,
        dest_dir,
        name,
        pages: request.pages.clone(),
        booklet,
        smaller,
        paper: paper.map(str::to_owned),
        app: reach.clone(),
    };
    let plan = match &request.what {
        What::Files { entries, shown } => {
            let mut plan = Plan::new(match entries.as_slice() {
                [entry] => format!("save “{}” as {what}", display_name(&entry.path)),
                many if together => format!("save {} files as one {what}", many.len()),
                many => format!("save {} files as {what}s", many.len()),
            });
            // Together: one step, the others' pages after the first's.
            let groups: Vec<Vec<EntryRef>> = if together {
                vec![entries.clone()]
            } else {
                entries.iter().map(|entry| vec![entry.clone()]).collect()
            };
            anyhow::ensure!(
                given.is_none() || groups.len() == 1,
                "A name is a PDF's: these files make one each (`together` makes one of them all)"
            );
            for group in groups {
                let Some(first) = group.first() else { continue };
                let (dest_dir, name) = (
                    folder_of(&first.path)?,
                    given.map_or_else(
                        || pdf_name(&first.path, booklet.is_some()),
                        ToOwned::to_owned,
                    ),
                );
                plan.push(step(
                    Of::Files {
                        entries: group,
                        page: shown.page,
                        selection: shown.selection.clone(),
                    },
                    dest_dir,
                    name,
                ));
            }
            plan
        }
        What::Sheet(entries) => {
            let first = entries.first().context("Nothing to print")?;
            let mut plan = Plan::new(format!(
                "save a contact sheet of {} files as PDF",
                entries.len()
            ));
            plan.push(step(
                Of::Sheet {
                    entries: entries.clone(),
                },
                folder_of(&first.path)?,
                given.map_or_else(|| OsString::from(SHEET), ToOwned::to_owned),
            ));
            plan
        }
        What::Listing(pane) => {
            let (location, filter) = shown(app, *pane)?;
            let named = listing_named(app, &location, filter.as_deref());
            let dest_dir = match dest {
                Some(dest) => dest.to_owned(),
                None => listing_folder(app, &location)
                    .context("There is no folder to save the listing in: choose one")?,
            };
            let mut plan = Plan::new(format!("save the listing of “{named}” as PDF"));
            plan.push(step(
                Of::Listing {
                    pane: *pane,
                    location: location.clone(),
                    named: named.clone(),
                },
                dest_dir,
                given.map_or_else(|| listing_name(&location, &named), ToOwned::to_owned),
            ));
            plan
        }
    };
    Ok(plan)
}

/// What the PDF of `location`'s listing is named: after the folder, a
/// folder without a name of its own (the root, a drive) after what it is,
/// never its path; any other place after its name (`named`), a separator
/// a dash and control characters left out, else, a name this system
/// refuses, after what it is. Cut to what a folder takes, as a copy's
/// name is (`kept_name`).
fn listing_name(location: &Location, named: &str) -> OsString {
    let plain = || OsString::from("Listing.pdf");
    let own = if let Some(folder) = location.path() {
        let Some(own) = folder.file_name() else {
            return plain();
        };
        own.to_owned()
    } else {
        let kept: String = named
            .chars()
            .filter(|c| !c.is_control())
            .map(|c| {
                if c == '/' || std::path::is_separator(c) {
                    '-'
                } else {
                    c
                }
            })
            .collect();
        OsString::from(kept.trim())
    };
    if own.is_empty() {
        return plain();
    }
    let mut pdf = own;
    pdf.push(".pdf");
    let name = op::clash::kept_name(&pdf, "listing", false, |_| false);
    if op::valid_file_name(&name).is_err() {
        return plain();
    }
    name
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

/// Make the PDF of what `of` names, the `pages` of it, in `dest_dir` as
/// `name`, on the paper of this system's locale.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MakePdf {
    pub of: Of,
    #[serde(with = "skwir_core::os_path")]
    #[schemars(with = "PathArg")]
    pub dest_dir: PathBuf,
    #[serde(with = "skwir_core::os_path")]
    #[schemars(with = "PathArg")]
    pub name: OsString,
    #[serde(default)]
    pub pages: Pages,
    /// Folded as a booklet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub booklet: Option<Booklet>,
    /// Made smaller as Reduce Size makes a copy (ADR-112), at this
    /// quality, before it is kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub smaller: Option<Quality>,
    /// The paper it is laid out on, by name (`Paper::NAMED`); the system's
    /// without.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paper: Option<String>,
    /// The app it is made in, as the plugin holds it: no part of its data
    /// (set as it is made, and as it is rebuilt: `register_with`).
    #[serde(skip)]
    #[schemars(skip)]
    pub app: AppRef,
}

/// What a PDF is made of.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Of {
    /// Files as their previews show them, one PDF in their order; of one,
    /// the page its preview showed (from 1, for `Pages::Current`) and what
    /// was selected (`Pages::Selection`).
    Files {
        entries: Vec<EntryRef>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        page: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        selection: Option<String>,
    },
    /// A contact sheet of the pictures among these files.
    Sheet { entries: Vec<EntryRef> },
    /// The listing a pane shows of `location` (a folder, a search, a
    /// tag, the trash), as it stands when made; `named` as it was asked.
    Listing {
        pane: PaneId,
        location: Location,
        named: String,
    },
}

impl Of {
    /// The files it reads.
    fn entries(&self) -> &[EntryRef] {
        match self {
            Of::Files { entries, .. } | Of::Sheet { entries } => entries,
            Of::Listing { .. } => &[],
        }
    }

    /// The path it is made from, first: its first file, the folder
    /// listed; none for a place that is no folder.
    fn from(&self) -> PathBuf {
        match self {
            Of::Files { entries, .. } | Of::Sheet { entries } => entries
                .first()
                .map(|entry| entry.path.clone())
                .unwrap_or_default(),
            Of::Listing { location, .. } => location.path().unwrap_or_default(),
        }
    }
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
    /// What it is made of, as its step counts it as the helper goes: the
    /// plan's time left told from its start (ADR-113). A listing reads no
    /// file.
    fn work(&self, _: &StepContext, _: Option<u64>) -> u64 {
        self.of
            .entries()
            .iter()
            .filter_map(|entry| std::fs::metadata(&entry.path).ok())
            .fold(0, |sum, meta| sum.saturating_add(meta.len()))
    }

    fn footprint(&self, _: &op::footprint::Volumes) -> Option<op::footprint::Footprint> {
        let read = match &self.of {
            Of::Listing { location, .. } => match location.path() {
                Some(folder) => op::footprint::Footprint::default().read(&folder),
                // Its files' metadata, where the pane has not read it: as
                // a search reads them, nothing held.
                None => op::footprint::Footprint::default(),
            },
            of => of
                .entries()
                .iter()
                .fold(op::footprint::Footprint::default(), |footprint, entry| {
                    footprint.read(&entry.path)
                }),
        };
        // A name taken there is kept beside when its turn comes.
        Some(read.settles_name(&self.target(), true).heavy_if(true))
    }

    fn describe(&self) -> Change {
        let what = match &self.of {
            Of::Files { entries, .. } => match entries.as_slice() {
                [first, rest @ ..] if !rest.is_empty() => {
                    format!("“{}” and {} more", display_name(&first.path), rest.len())
                }
                _ => format!("“{}”", display_name(&self.of.from())),
            },
            Of::Sheet { entries } => format!("a contact sheet of {} files", entries.len()),
            Of::Listing { named, .. } => format!("the listing of “{named}”"),
        };
        // Where it goes said too: Save as may put it anywhere.
        let (pdf, folder) = (display_name(&self.target()), display_name(&self.dest_dir));
        Change::Create {
            path: self.target(),
            directory: false,
            words: Some(Words {
                will: format!("save {what} as “{pdf}” in “{folder}”"),
                did: format!("saved {what} as “{pdf}” in “{folder}”"),
                already: format!("{what} was already saved as “{pdf}” in “{folder}”"),
            }),
        }
    }

    fn check(&self, ctx: &StepContext) -> StepStatus {
        let conflict = |reason: String| StepStatus::Conflict { reason };
        if let Err(invalid) = op::valid_file_name(&self.name) {
            return conflict(invalid.to_string());
        }
        if self.of.entries().is_empty() && !matches!(self.of, Of::Listing { .. }) {
            return conflict("There is nothing to print".to_owned());
        }
        for entry in self.of.entries() {
            if let Err(error) = entry.resolve() {
                return conflict(error.to_string());
            }
        }
        if let Of::Listing { location, .. } = &self.of
            && let Some(folder) = location.path()
            && !ctx.is_folder(&folder)
        {
            return conflict(format!("“{}” is not a folder", display_name(&folder)));
        }
        if !ctx.is_folder(&self.dest_dir) {
            return conflict(format!(
                "“{}” is not a folder",
                display_name(&self.dest_dir)
            ));
        }
        // A PDF is made aside and moved into place, which a folder that
        // takes only copies written in place (a camera) does not do.
        if !StepContext::stages_in(&self.dest_dir) {
            return conflict(format!(
                "“{}” takes only copies: no PDF is made there",
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
        let inputs = match &self.of {
            Of::Listing { location, .. } => location.path().into_iter().collect(),
            of => of
                .entries()
                .iter()
                .map(|entry| entry.path.clone())
                .collect(),
        };
        Some(Made {
            target: self.target(),
            from: self.of.from(),
            inputs,
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
        let target = self.target();
        let app = self
            .app
            .get()
            .context("Saving as PDF needs the print plugin on")?;
        let cancel = ctx.cancel_token();
        // What it is made of, named now: a listing as its pane shows it.
        let made = match &self.of {
            Of::Files {
                entries,
                page,
                selection,
            } => Make::Files(
                entries.clone(),
                Shown {
                    page: *page,
                    selection: selection.clone(),
                },
            ),
            Of::Sheet { entries } => Make::Sheet(entries.clone()),
            Of::Listing {
                pane,
                location,
                named,
            } => {
                // The pane's listing: it must show that place still.
                anyhow::ensure!(
                    app.pane_location(*pane).as_ref() == Some(location),
                    "“{named}” is no longer shown, so its listing is not known",
                );
                // Read in its files' lanes, from this thread.
                Make::Listing(listing::listing(&app, *pane, cancel)?)
            }
        };
        let title = match &self.of {
            Of::Sheet { .. } => "Contact sheet".to_owned(),
            of => display_name(&of.from()),
        };
        let layout = Layout {
            paper: self
                .paper
                .as_deref()
                .and_then(skwir_core::print::Paper::named)
                .unwrap_or_else(skwir_platform::print::paper),
            pages: self.pages.clone(),
            sent: Pages::All,
            page_set: PageSet::All,
            reverse: false,
            scale: 1.0,
            booklet: self.booklet,
        };
        // Made in a folder of this machine's, in the lane of what it
        // reads: only the PDF made goes where it is kept (a server's
        // folder too), staged there and put in place.
        let work = tempfile::Builder::new()
            .prefix("skwir-print-")
            .tempdir()
            .context("A folder to make the document in")?;
        let (weak, token, at) = (app.weak(), cancel.clone(), work.path().to_owned());
        let smaller = self.smaller;
        let (gauge, inputs) = (ctx.gauge(), self.of.entries().to_vec());
        let lane = skwir_core::fs::reach::lane(&self.of.from());
        let ready = in_lane(&app, lane, "print-save", cancel, move || {
            let app = weak.upgrade().context("Skwir is closing")?;
            // What it is made of announced, read here in its lane, so the
            // window shows it being made, with Cancel: one PDF is one step,
            // which shows nothing otherwise. A listing reads no file.
            let size: u64 = inputs
                .iter()
                .filter_map(|entry| std::fs::metadata(&entry.path).ok())
                .map(|meta| meta.len())
                .sum();
            gauge.announce(size);
            let metered = skwir_platform::process::Metered::new(gauge, size);
            // Made, then made smaller when asked: each a share of the bar.
            let making = if smaller.is_some() { 0.6 } else { 1.0 };
            let went = |fraction: f64| metered.went(fraction);
            let held = || metered.held();
            let follow = skwir_platform::process::Follow::new(&went, &held);
            let mut ready = made_ready(
                &app,
                made,
                &title,
                &layout,
                &at,
                &token,
                &follow.part(0.0, making),
            )?;
            if let Some(quality) = smaller {
                ready.pdf = smaller_pdf(
                    &app,
                    ready.pdf,
                    quality,
                    &at,
                    &token,
                    &follow.part(making, 1.0),
                )?;
            }
            metered.went(1.0);
            Ok(ready)
        });
        if cancel.is_cancelled() && ready.is_err() {
            return Err(op::Cancelled.into());
        }
        let ready = ready?;
        ctx.check_cancelled()?;
        let staging = ctx.stage(&self.dest_dir)?;
        let made = staging.path().join("made.pdf");
        // What a helper made here is copied from the file it opened as
        // what it is, never moved nor read through a link it left; the
        // file itself, printed whole, is copied.
        let kept = make::open_made(&ready, work.path()).and_then(|opened| {
            match opened {
                Some(mut from) => {
                    let mut to = std::fs::File::create_new(&made)?;
                    std::io::copy(&mut from, &mut to)?;
                }
                None => {
                    std::fs::copy(&ready.pdf, &made)?;
                }
            }
            Ok(())
        });
        let result = kept.and_then(|()| {
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
        if cancel.is_cancelled() && result.is_err() {
            return Err(op::Cancelled.into());
        }
        result?;
        // What was left out, and how to fold it, said with what was made:
        // the folding once for several booklets alike.
        if let Some(notice) = &ready.notice {
            ctx.say(format!("“{}”: {notice}", display_name(&target)));
        }
        if let Some(folding) = ready.folding {
            ctx.say(folding);
        }
        let made = EntryRef::from_path(&target)
            .with_context(|| format!("“{}” went missing as it was made", display_name(&target)))?;
        // Undone to the trash, as a copy is: it may have been changed since.
        Ok(Some(Box::new(op::Trash { entry: made })))
    }
}

/// The PDF at `pdf` made smaller at `quality` (ADR-112), by the print
/// helper in a folder of its own in `work`, checked as Reduce Size checks
/// a copy: the smaller copy, or `pdf` itself when it would not be smaller.
fn smaller_pdf(
    app: &App,
    pdf: PathBuf,
    quality: Quality,
    work: &Path,
    cancel: &CancelToken,
    follow: &skwir_platform::process::Follow<'_>,
) -> anyhow::Result<PathBuf> {
    let folder = work.join("smaller");
    std::fs::create_dir(&folder).context("A folder to make the PDF smaller in")?;
    let job = skwir_core::shrink::Job {
        kind: skwir_core::shrink::Kind::Pdf,
        input: pdf.clone(),
        output: PathBuf::new(),
        quality,
        gray: false,
        strip: false,
        at_most: None,
        keep: Vec::new(),
        rule: None,
    };
    // A pass after the first is not announced: the bar holds where the
    // first left it while it runs.
    let (copy, _) =
        skwir_platform::shrink_job::made(app, job, &folder, "smaller.pdf", cancel, follow, &|| {})
            .context("The PDF could not be made smaller")?;
    Ok(copy.map_or(pdf, |copy| copy.path))
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

    /// The folder's menu names its listing as the palette does; files and
    /// the preview keep Print….
    #[test]
    fn the_listing_is_offered_by_its_name() {
        let tmp = tempfile::tempdir().unwrap();
        let app = App::new(skwir_core::app::AppConfig::ephemeral(
            tmp.path().join("state"),
        ))
        .unwrap();
        menus(&app);
        let title = |when: &str| {
            app.registry
                .menus
                .all()
                .into_iter()
                .find(|item| item.when.as_deref() == Some(when))
                .map(|item| item.title)
        };
        assert_eq!(title(LISTS).as_deref(), Some("Print Listing…"));
        assert_eq!(title(FILES).as_deref(), Some("Print…"));
        assert_eq!(title("pages|picture|text").as_deref(), Some("Print…"));
    }

    /// A listing's PDF is named after its folder; the root's, which has no
    /// name, after what it is, a name a file can have; any other place's
    /// after its name, what no name holds left out.
    #[test]
    fn a_listing_is_named_after_its_folder_never_its_path() {
        let folder = |path: &str| Location::from_path(Path::new(path));
        assert_eq!(
            listing_name(&folder("/example/letters"), "letters"),
            "letters (listing).pdf"
        );
        let root = listing_name(&folder("/"), "/");
        assert_eq!(root, "Listing.pdf");
        assert!(op::valid_file_name(&root).is_ok());
        let place = Location::new("query", "report in:/example/Papers");
        assert_eq!(
            listing_name(&place, "report in Papers"),
            "report in Papers (listing).pdf"
        );
        let tag = Location::new("tags", "/Places/France");
        let named = listing_name(&tag, "Places/France, filtered by kind:image");
        assert_eq!(named, "Places-France, filtered by kind:image (listing).pdf");
        assert!(op::valid_file_name(&named).is_ok());
        assert_eq!(listing_name(&tag, " / "), "- (listing).pdf");
        assert_eq!(listing_name(&tag, "two\nlines"), "twolines (listing).pdf");
        let long = listing_name(&tag, &"x".repeat(300));
        assert!(long.len() <= 255, "{}", long.len());
        assert!(long.to_string_lossy().ends_with("x (listing).pdf"));
    }
}
