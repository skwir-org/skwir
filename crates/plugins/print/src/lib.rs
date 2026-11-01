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
//! does. Nothing selected in a folder, Print… prints its listing.
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

/// What it offers the folder's listing for: the folder, nothing selected.
const LISTS: &str = "empty folder";

/// The menu of the preview's More button and of its own context menu,
/// whose items the window runs with what the preview shows
/// (`ui.preview_invoke`).
const PREVIEW: &str = "preview";

/// The app the plugin registered with, for Save as PDF's step, which
/// reaches previews as the window does; a step is data, not the app's.
static APP: RwLock<Option<Weak<App>>> = RwLock::new(None);

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
    /// The folder the PDFs are made in; each file's own without (a
    /// contact sheet's: its first file's; a listing's: its folder).
    #[serde(default)]
    pub dest: Option<PathArg>,
    /// Several files saved as one PDF, in their order, named after the
    /// first, as they print together.
    #[serde(default)]
    pub together: bool,
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
        if let Ok(mut held) = APP.write() {
            *held = Some(app.weak());
        }
        app.ops.register::<MakePdf>(ID);
        let print = app.commands.register::<PrintArgs, _>(
            ID,
            Topic::Files,
            "print.print",
            "Print…",
            "Print files as their previews show them, one job in their order (`layout`: as shown, a booklet folded in signatures of `signature` pages with `trim`, a contact sheet of the pictures, a pane's listing), the `pages` chosen; the window asks first in Skwir's dialog without `asked`, then the system's dialog chooses the printer, the paper and the sides",
            |a, app| {
                if a.asked || !app.window_shown() {
                    match request(app, &a) {
                        Ok(request) => start(app, request),
                        Err(error) => app.notify(NotifyLevel::Error, format!("{error:#}")),
                    }
                } else {
                    match resolved(app, a) {
                        Ok(args) => ask_first(app, args),
                        Err(error) => app.notify(NotifyLevel::Error, format!("{error:#}")),
                    }
                }
                Ok(Outcome::Done)
            },
        );
        // Ctrl+P and Print… on a folder with nothing selected.
        print.forms([skwir_core::command::Form::new(
            serde_json::json!({ "layout": "listing" }),
            "Print Listing…",
        )]);
        app.commands
            .register::<SaveArgs, _>(
                ID,
                Topic::Files,
                "print.save_pdf",
                "Save as PDF",
                "Make a PDF of each file as its preview shows it, beside it or in `dest` (one of them all with `together`), on the paper of the system's locale, laid out and of the pages Print… takes (plan)",
                |a, app| {
                    let request = request(app, &a.print)?;
                    let dest = a.dest.as_ref().map(|PathArg(dest)| dest.as_path());
                    Ok(Outcome::Plan(plan_of(app, &request, dest, a.together)?))
                },
            )
            .plans();
        menus(app);
        // Ctrl+P prints the files selected, the folder's listing with none,
        // or what the preview shows.
        for (when, command) in [
            ("pane file !place", Invocation::new("print.print")),
            (
                "pane empty folder",
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
        dialog::forget();
    }
}

/// Print… in the file menu's group of what is made from it (3xx), for
/// files and for the folder's listing, and in the preview's menu.
fn menus(app: &App) {
    for (menu, when, invocation) in [
        ("context", FILES, Invocation::new("print.print")),
        (
            "context",
            LISTS,
            Invocation::new("print.print").with("layout", "listing"),
        ),
        (
            PREVIEW,
            "pages|picture|text",
            Invocation::new("ui.preview_invoke").with("command", "print.print"),
        ),
    ] {
        app.registry.menus.add(
            ID,
            MenuItem::new(menu, "Print…", "printer", 340, invocation).when(when),
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
    let booklet = (args.layout == Arrange::Booklet).then_some(Booklet {
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
/// or their pane's listing).
fn offers(app: &App, args: &PrintArgs, cancel: &CancelToken) -> anyhow::Result<dialog::Offers> {
    if args.layout == Arrange::Listing {
        let pane = app.pane_or_active(args.pane);
        let folder = app
            .pane_dir(pane)
            .map_or_else(|_| "this folder".to_owned(), |dir| display_name(&dir));
        return Ok(dialog::Offers {
            what: format!("the listing of “{folder}”"),
            subject: dialog::Subject::Listing,
            ..dialog::Offers::default()
        });
    }
    let typed = app.targets_typed(args.pane, args.entries.clone())?;
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
    let (said, heard) = std::sync::mpsc::sync_channel(1);
    let counting = cancel.child();
    let (token, entry, weak) = (counting.clone(), entry.clone(), app.weak());
    app.scheduler.spawn_in(
        skwir_core::fs::reach::lane(&entry.path),
        Priority::Normal,
        "print-count",
        counting.clone(),
        move || {
            let count = weak.upgrade().and_then(|app| {
                let path = entry.resolve().ok()?;
                make::page_count(&app, &path, &token).ok()
            });
            let _ = said.send(count);
        },
    );
    let count = heard.recv_timeout(COUNTING).ok().flatten();
    counting.cancel();
    count
}

/// `work` run in the lane of `path` (its remote's, the shared one
/// elsewhere: `fs::reach::lane`) and waited for here: what reads files
/// runs there, so a server that stopped answering holds its own lane, and
/// what waits for the user (a dialog) holds none (AGENTS.md, remote
/// paths). An error when `cancel` fires first.
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
fn ask_first(app: &App, args: PrintArgs) {
    let weak = app.weak();
    app.scheduler.spawn(
        Priority::Normal,
        "print-dialog",
        app.lifetime().child(),
        move || {
            let Some(app) = weak.upgrade() else { return };
            let cancel = app.lifetime().child();
            match chosen_then(&app, args, &cancel) {
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
    cancel: &CancelToken,
) -> anyhow::Result<Option<String>> {
    let offers = offers(app, &args, cancel)?;
    let Some(chosen) = dialog::ask(app, &offers, cancel) else {
        return Ok(None);
    };
    let args = PrintArgs {
        pages: Some(chosen.pages),
        layout: chosen.layout,
        signature: chosen.signature,
        trim: chosen.trim,
        asked: true,
        ..args
    };
    let request = request(app, &args)?;
    match chosen.to {
        dialog::To::Printer => print(app, request, cancel),
        dialog::To::Pdf => {
            app.apply_whole(plan_of(app, &request, None, chosen.together)?);
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
            let lane = entries[0].path.clone();
            (what, title, lane, Make::Files(entries, shown))
        }
        What::Sheet(entries) => {
            let lane = entries.first().context("Nothing to print")?.path.clone();
            (
                "a contact sheet".to_owned(),
                "Contact sheet".to_owned(),
                lane,
                Make::Sheet(entries),
            )
        }
        What::Listing(pane) => {
            let folder = app.pane_dir(pane).unwrap_or_default();
            let (weak, token) = (app.weak(), cancel.clone());
            let (title, table) = in_lane(app, &folder, "print-listing", cancel, move || {
                let app = weak.upgrade().context("Skwir is closing")?;
                listing::listing(&app, pane, &token)
            })?;
            (
                format!("the listing of “{title}”"),
                title.clone(),
                folder,
                Make::Listing(title, table),
            )
        }
    };
    let ask = skwir_platform::print::Ask {
        title: title.clone(),
    };
    let Some(chosen) = skwir_platform::print::ask(&ask, cancel)? else {
        return Ok(None);
    };
    // A printer turning the sheet over its long edge turns a booklet's
    // backs upside down: they are turned back.
    let booklet = booklet.map(|booklet| Booklet {
        turn_backs: chosen.duplex == skwir_core::print::Duplex::LongEdge,
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
    let (weak, token, sent_as) = (app.weak(), cancel.clone(), chosen.clone());
    let (pages, notice) = in_lane(app, &lane, "print", cancel, move || {
        let app = weak.upgrade().context("Skwir is closing")?;
        let work = tempfile::Builder::new()
            .prefix("skwir-print-")
            .tempdir()
            .context("A folder to make the document in")?;
        let ready = made_ready(&app, made, &title, &layout, work.path(), &token)?;
        skwir_platform::print::send(&sent_as, &title, &ready.pdf, &token)?;
        Ok((ready.pages, ready.notice))
    })
    .with_context(|| format!("{} was not printed", capitalized(&what)))?;
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
    let mut said = format!("Sent {sent}{what} {to}");
    if booklet.is_some() && chosen.duplex == skwir_core::print::Duplex::Simplex {
        said = format!(
            "{said}. A booklet is printed on both sides: on one side, print the odd sides, turn the sheets over, then the even"
        );
    }
    if let Some(notice) = notice {
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
    Listing(String, skwir_core::print::Part),
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
) -> anyhow::Result<make::Ready> {
    match made {
        Make::Files(entries, shown) => {
            make::ready(app, &resolve(&entries)?, &shown, layout, work, cancel)
        }
        Make::Sheet(entries) => {
            make::sheet(&resolve(&entries)?, work, cancel).and_then(|(part, notice)| {
                let ready = make::made(app, title, vec![part], layout, work, cancel)?;
                Ok(make::Ready {
                    notice: ready.notice.or(notice),
                    ..ready
                })
            })
        }
        Make::Listing(title, table) => make::made(app, &title, vec![table], layout, work, cancel),
    }
}

/// `text` with its first letter a capital.
fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// Save as PDF's plan for `request`: a step making each file's PDF beside
/// it (or in `dest`), one of them all with `together`; a contact sheet's
/// beside its first file, a listing's in its folder.
fn plan_of(
    app: &App,
    request: &Request,
    dest: Option<&Path>,
    together: bool,
) -> anyhow::Result<Plan> {
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
            for group in groups {
                let Some(first) = group.first() else { continue };
                let (dest_dir, name) = (
                    folder_of(&first.path)?,
                    pdf_name(&first.path, booklet.is_some()),
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
                OsString::from("Contact sheet.pdf"),
            ));
            plan
        }
        What::Listing(pane) => {
            let folder = app
                .pane_dir(*pane)
                .context("Only a folder's listing is saved, in it: this one is of no folder")?;
            let name = listing_name(&folder);
            let mut plan = Plan::new(format!(
                "save the listing of “{}” as PDF",
                display_name(&folder)
            ));
            let dest_dir = dest.map_or_else(|| folder.clone(), Path::to_owned);
            plan.push(step(
                Of::Listing {
                    pane: *pane,
                    folder,
                },
                dest_dir,
                name,
            ));
            plan
        }
    };
    Ok(plan)
}

/// What the PDF of `folder`'s listing is named: after the folder, a
/// folder without a name of its own (the root, a drive) after what it is,
/// never its path.
fn listing_name(folder: &Path) -> OsString {
    let Some(named) = folder.file_name() else {
        return OsString::from("Listing.pdf");
    };
    let mut name = named.to_owned();
    name.push(" (listing).pdf");
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
    /// The listing a pane shows of `folder`, as it stands when made.
    Listing {
        pane: PaneId,
        #[serde(with = "skwir_core::os_path")]
        #[schemars(with = "PathArg")]
        folder: PathBuf,
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

    /// The path it is made from, first: its first file, the folder listed.
    fn from(&self) -> PathBuf {
        match self {
            Of::Files { entries, .. } | Of::Sheet { entries } => entries
                .first()
                .map(|entry| entry.path.clone())
                .unwrap_or_default(),
            Of::Listing { folder, .. } => folder.clone(),
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
    fn footprint(&self, _: &op::footprint::Volumes) -> Option<op::footprint::Footprint> {
        let read = match &self.of {
            Of::Listing { folder, .. } => op::footprint::Footprint::default().read(folder),
            of => of
                .entries()
                .iter()
                .fold(op::footprint::Footprint::default(), |footprint, entry| {
                    footprint.read(&entry.path)
                }),
        };
        Some(read.made(&self.target()).heavy_if(true))
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
            Of::Listing { folder, .. } => format!("the listing of “{}”", display_name(folder)),
        };
        let pdf = display_name(&self.target());
        Change::Create {
            path: self.target(),
            directory: false,
            words: Some(Words {
                will: format!("save {what} as “{pdf}”"),
                did: format!("saved {what} as “{pdf}”"),
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
        if let Of::Listing { folder, .. } = &self.of
            && !ctx.is_folder(folder)
        {
            return conflict(format!("“{}” is not a folder", display_name(folder)));
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
            Of::Listing { folder, .. } => vec![folder.clone()],
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
        let app = APP
            .read()
            .ok()
            .and_then(|held| held.as_ref().and_then(Weak::upgrade))
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
            Of::Listing { pane, folder } => {
                // The pane's listing: it must show that folder still.
                anyhow::ensure!(
                    app.pane_dir(*pane).is_ok_and(|shown| shown == *folder),
                    "“{}” is no longer shown, so its listing is not known",
                    display_name(folder)
                );
                let (pane, weak, token) = (*pane, app.weak(), cancel.clone());
                let (title, table) = in_lane(&app, folder, "print-listing", cancel, move || {
                    let app = weak.upgrade().context("Skwir is closing")?;
                    listing::listing(&app, pane, &token)
                })?;
                Make::Listing(title, table)
            }
        };
        let title = match &self.of {
            Of::Sheet { .. } => "Contact sheet".to_owned(),
            of => display_name(&of.from()),
        };
        let layout = Layout {
            paper: skwir_platform::print::paper(),
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
        let ready = in_lane(&app, &self.of.from(), "print-save", cancel, move || {
            let app = weak.upgrade().context("Skwir is closing")?;
            made_ready(&app, made, &title, &layout, &at, &token)
        });
        if cancel.is_cancelled() && ready.is_err() {
            return Err(op::Cancelled.into());
        }
        let ready = ready?;
        ctx.check_cancelled()?;
        let staging = ctx.stage(&self.dest_dir)?;
        let made = staging.path().join("made.pdf");
        // What was made here moves where it can; the file itself, printed
        // whole, is copied, never moved.
        let kept = if ready.pdf.starts_with(work.path()) {
            std::fs::rename(&ready.pdf, &made)
                .or_else(|_| std::fs::copy(&ready.pdf, &made).map(drop))
        } else {
            std::fs::copy(&ready.pdf, &made).map(drop)
        };
        let result = kept.map_err(anyhow::Error::from).and_then(|()| {
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

    /// A listing's PDF is named after its folder; the root's, which has no
    /// name, after what it is, a name a file can have.
    #[test]
    fn a_listing_is_named_after_its_folder_never_its_path() {
        assert_eq!(
            listing_name(Path::new("/example/letters")),
            "letters (listing).pdf"
        );
        let root = listing_name(Path::new("/"));
        assert_eq!(root, "Listing.pdf");
        assert!(op::valid_file_name(&root).is_ok());
    }
}
