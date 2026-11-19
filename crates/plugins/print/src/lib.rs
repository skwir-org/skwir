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
//! another folder is chosen (`plan_of`, `dialog::Saved`). A booklet is
//! bound on the edge chosen (`bound`), else as the document reads
//! (`make`): a Hebrew or an Arabic book, a manga, on the right.
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
use skwir_core::op::CouldntExt as _;
use skwir_core::op::{
    self, Change, EntryArg, EntryRef, Made, Operation, Plan, StepContext, StepStatus, Tenses,
};
use skwir_core::os_path::PathArg;
use skwir_core::plugin::{AppRef, Plugin};
use skwir_core::print::{PageSet, Pages};
use skwir_core::registry::{KeyBinding, MenuItem};
use skwir_core::scheduler::{Lane, Priority};
use skwir_core::shrink::Quality;
use skwir_core::words;
use skwir_core::words::{Name, Words};
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

/// What is printed, as the words that say it name it: a message chooses
/// its words by `kind` and places `name` and `count` (`print_title`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum Printed {
    /// A file, by its name.
    File(String),
    /// Several files, how many.
    Files(usize),
    /// A contact sheet.
    #[default]
    Sheet,
    /// The listing of a place, by its name as shown.
    Listing(String),
}

impl Printed {
    /// The word its messages choose by.
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Printed::File(_) => "file",
            Printed::Files(_) => "files",
            Printed::Sheet => "sheet",
            Printed::Listing(_) => "listing",
        }
    }

    /// The file's or the place's name; none for the others.
    pub(crate) fn name(&self) -> Name {
        match self {
            Printed::File(name) | Printed::Listing(name) => Name(name.clone()),
            Printed::Files(_) | Printed::Sheet => Name(String::new()),
        }
    }

    /// How many files; none for the others.
    pub(crate) fn count(&self) -> usize {
        match self {
            Printed::Files(count) => *count,
            _ => 0,
        }
    }
}

/// Print and what is printed: the dialog's title and the card of a print
/// under way.
pub(crate) fn print_title(printed: &Printed) -> Words {
    words!(
        print_title = "{ $what ->
            [file] Print { $name }
            [files] { $count ->
                [one] Print { $count } file
               *[other] Print { $count } files
            }
            [sheet] Print a contact sheet
           *[listing] Print the listing of { $name }
        }",
        what = printed.kind(),
        name = printed.name(),
        count = printed.count(),
        note = "The title of the Print dialog, and of a print under way in Activity: a file by its name, several by how many, a contact sheet, or the listing of a folder or a place by its name."
    )
}

/// `said`, then `then`: two sentences of one notice, the first without
/// its full stop.
pub(crate) fn then(said: Words, then: Words) -> Words {
    words!(
        print_then = "{ $said }. { $then }",
        said = said,
        then = then,
        note = "Two sentences said one after the other in a notice; the first has no full stop of its own."
    )
}

/// Sentences one after the other, as `then` puts them; none for none.
pub(crate) fn sentences(said: Vec<Words>) -> Option<Words> {
    said.into_iter().reduce(then)
}

/// The error of a pane asked that is not there (closed meanwhile).
pub(crate) fn no_such_pane() -> Words {
    words!(
        print_no_such_pane = "No such pane",
        note = "An error: the pane whose listing is printed is not there any more."
    )
}

/// The error of a pane that shows nothing yet.
pub(crate) fn shows_nothing() -> Words {
    words!(
        print_shows_nothing = "The pane shows nothing",
        note = "An error: the pane whose listing is printed shows no folder or place."
    )
}

/// Why nothing was made: the program is closing.
pub(crate) fn closing() -> Words {
    words!(
        print_closing = "Skwir is closing",
        note = "Why a print or a PDF was not made. Skwir is the program's name."
    )
}

/// Why nothing was printed: it was cancelled.
pub(crate) fn cancelled() -> Words {
    words!(print_cancelled = "Printing was cancelled")
}

/// Why nothing was printed: nothing was named.
fn nothing_to_print() -> Words {
    words!(print_nothing_to_print = "Nothing to print")
}

/// What could not be made, before why: the folder the PDF is made in.
fn work_folder() -> Words {
    words!(
        print_work_folder = "A folder to make the document in",
        note = "What could not be made, before why: a temporary folder the PDF is made in."
    )
}

/// The error of a file named that is a root.
fn in_no_folder() -> Words {
    words!(
        print_in_no_folder = "The file is in no folder",
        note = "An error an agent or a script reads: the file named is a root."
    )
}

/// Print…: the command, the menus' items.
fn print_menu() -> Words {
    words!(
        print_print = "Print…",
        note = "A menu item and the action's name: asks how to print, then the system's dialog chooses the printer."
    )
}

/// Print Listing…: what the pane lists, printed.
fn print_listing_menu() -> Words {
    words!(
        print_print_listing = "Print Listing…",
        note = "A menu item, with nothing selected: prints what the pane lists, as a table."
    )
}

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
    /// The edge a booklet is bound on: `left`, or `right` (a Hebrew or an
    /// Arabic book, a manga), its pages laid for folds made the other way.
    /// Without, as the document reads: `reads`, else the way it says it
    /// reads (a PDF's `/ViewerPreferences /Direction`, a book's page
    /// progression, HTML's `dir`), else, a text Skwir lays out, its first
    /// letter that has a direction; else `left`. Skwir's dialog (Print…
    /// without `asked`, a window shown) opens on that edge, an agent's
    /// call included; Save as PDF and `asked` ask no dialog.
    #[serde(default)]
    pub bound: Option<skwir_core::print::booklet::Bound>,
    /// The way the document reads (`left_to_right`, `right_to_left`), a
    /// fact its caller knows, as `count` is: the preview passes what the
    /// document it shows says of itself. Binds a booklet without `bound`.
    #[serde(default)]
    pub reads: Option<skwir_core::direction::Direction>,
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

    fn title(&self) -> skwir_core::words::Words {
        skwir_core::words!(print_plugin_title = "Printing")
    }

    fn description(&self) -> skwir_core::words::Words {
        skwir_core::words!(
            print_plugin_description = "Print what a preview shows, as it is, as a booklet or as a contact sheet, or save it as a PDF"
        )
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
            print_menu(),
            words!(
                print_print_description = "Print files as their previews show them, one job in their order (`layout`: as shown, a booklet folded in signatures of `signature` pages with `trim`, a contact sheet of the pictures, a pane's listing), the `pages` chosen; the window asks first in Skwir's dialog without `asked`, then the system's dialog chooses the printer, the paper and the sides",
                note = "Read by agents and in F1's list of actions: the words in backticks are the command's arguments, as written."
            ),
            move |a, app| {
                if a.asked || !app.window_shown() {
                    match request(app, &a) {
                        Ok(request) => start(app, request),
                        Err(error) => app.notify(NotifyLevel::Error, Words::of_error(error)),
                    }
                } else {
                    match resolved(app, a) {
                        Ok(args) => ask_first(app, args, held.clone()),
                        Err(error) => app.notify(NotifyLevel::Error, Words::of_error(error)),
                    }
                }
                Ok(Outcome::Done)
            },
        );
        // Ctrl+P and Print Listing… with nothing selected.
        print.forms([skwir_core::command::Form::new(
            serde_json::json!({ "layout": "listing" }),
            print_listing_menu(),
        )]);
        let saving = self.held.app.clone();
        app.commands
            .register::<SaveArgs, _>(
                ID,
                Topic::Files,
                "print.save_pdf",
                words!(
                    print_save_pdf = "Save as PDF",
                    note = "The action's name, in F1's list of actions."
                ),
                words!(
                    print_save_pdf_description = "Make a PDF of each file as its preview shows it, beside it or in `dest` (one of them all with `together`), on the paper of the system's locale, laid out and of the pages Print… takes (plan)",
                    note = "Read by agents and in F1's list of actions: the words in backticks are the command's arguments, as written."
                ),
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
                            words!(
                                print_no_paper = "{ $paper } is no paper: a4, letter, a5, a3 or legal",
                                paper = skwir_core::words::Name(paper.to_owned()),
                                note = "An error an agent or a script reads: the paper it gave, and those the command takes, as written."
                            )
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
        (
            "context",
            FILES,
            print_menu(),
            Invocation::new("print.print"),
        ),
        (
            "context",
            LISTS,
            print_listing_menu(),
            Invocation::new("print.print").with("layout", "listing"),
        ),
        (
            PREVIEW,
            "pages|picture|text",
            print_menu(),
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
            anyhow::ensure!(!entries.is_empty(), nothing_to_print());
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
            anyhow::ensure!(
                shown.page.is_some(),
                words!(
                    print_no_page_shown = "No page of it is shown",
                    note = "An error an agent or a script reads: it asked the page shown, and the preview shows none."
                )
            );
            Pages::Current
        }
        Some("selection") => {
            anyhow::ensure!(
                shown.selection.is_some(),
                words!(
                    print_nothing_selected = "Nothing of it is selected",
                    note = "An error an agent or a script reads: it asked what is selected in the preview, and nothing is."
                )
            );
            Pages::Selection
        }
        Some(typed) => Pages::Ranges(skwir_core::print::parse(typed).map_err(anyhow::Error::new)?),
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
            .with_context(|| {
                words!(
                    print_per_side = "{ $count ->
                        [one] { $count } page a side: a booklet has 2, 4 or 8
                       *[other] { $count } pages a side: a booklet has 2, 4 or 8
                    }",
                    count = count,
                    note = "An error an agent or a script reads: the pages to a side of a booklet's sheet it gave."
                )
            })?,
    };
    // Bound on the edge asked, else as the caller says the document
    // reads; else as it reads, told as it is made.
    let booklet = (args.layout == Arrange::Booklet).then_some(Booklet {
        per_side,
        signature: args.signature,
        trim: args.trim,
        turn_backs: false,
        bound: args
            .bound
            .or_else(|| args.reads.map(skwir_core::print::booklet::Bound::reading)),
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
    // answers slowly keeps its own), asked together so that the dialog
    // waits for one of them at most.
    let (opens, recent) = dialog::kept_folders(app, last);
    let mut asked: Vec<PathBuf> = opens.iter().cloned().collect();
    asked.extend(recent.iter().cloned());
    let mut answers = asked_together(app, &asked, cancel, holds_a_folder).into_iter();
    let mut there = |_: &PathBuf| answers.next() != Some(Some(false));
    let opens = opens.filter(&mut there);
    let recent: Vec<PathBuf> = recent.into_iter().filter(&mut there).collect();
    let places = sidebar_folders(app);
    if args.layout == Arrange::Listing {
        let (location, filter) = shown(app, pane)?;
        let own = own_folder(app, &location);
        let said = match (location.is_path(), own.is_some()) {
            (true, _) => words!(
                print_folder_listed = "The folder listed",
                note = "Under a folder offered where a listing's PDF is saved: the folder whose listing it is."
            ),
            (false, true) => words!(
                print_folder_of = "The folder it is of",
                note = "Under a folder offered where a listing's PDF is saved: the folder a search or a tag is of."
            ),
            (false, false) => words!(
                print_no_folder_of_its_own = "It has no folder of its own",
                note = "Under the folder offered where a listing's PDF is saved, for a place of no folder (the trash): the user's Documents."
            ),
        };
        let folder = own.or_else(|| documents.clone());
        let named = listing_named(app, &location, filter.as_deref());
        let file = listing_name(&location, &named)
            .to_string_lossy()
            .into_owned();
        return Ok(dialog::Offers {
            what: Printed::Listing(named),
            subject: dialog::Subject::Listing,
            saved: dialog::Saved {
                beside: folder.as_deref().map_or_else(
                    || {
                        words!(
                            print_in_folder_listed = "In the folder listed",
                            note = "Under Save as, where a name typed alone goes."
                        )
                    },
                    |folder| {
                        words!(
                            print_in_folder = "In { $folder }",
                            folder = Name::of(folder),
                            note = "Under Save as, where a name typed alone goes: this folder."
                        )
                    },
                ),
                folder,
                said,
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
    let beside_them = || {
        words!(
            print_beside = "{ $count ->
                [1] Beside it
               *[other] Beside them
            }",
            count = typed.len(),
            note = "Under the folder offered where a PDF is saved, and under Save as: the folder of the file printed, or of the files."
        )
    };
    let saved = dialog::Saved {
        said: beside_them(),
        beside: match &folder {
            Some(_) => beside_them(),
            None => words!(
                print_beside_each = "Beside each file; one PDF of them all beside the first",
                note = "Under Save as, where files of several folders are saved."
            ),
        },
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
        [(entry, _)] => Printed::File(display_name(&entry.path)),
        many => Printed::Files(many.len()),
    };
    let several = typed.len() > 1;
    let sheet = several
        && typed.iter().any(|(_, name)| {
            mime(name).is_some_and(|mime| {
                skwir_platform::mime::is_kind(&mime, skwir_platform::mime::Kind::Image)
            })
        });
    // What the first file says of itself, asked once, where the caller
    // did not say it: its pages (of one file read as pages: a PDF, a
    // comic, a `DjVu`), and the way it reads, which a booklet is bound by.
    let told = match typed.as_slice() {
        [(entry, _), rest @ ..] => {
            let count = rest.is_empty() && args.count.is_none();
            let reads = args.bound.is_none() && args.reads.is_none();
            if count || reads {
                told(app, entry, cancel)
            } else {
                make::Told::default()
            }
        }
        [] => make::Told::default(),
    };
    let count = if several {
        None
    } else {
        args.count.or(told.pages)
    };
    // Unknown where nothing tells it in time: bound as it reads once made.
    let bound = args.bound.or_else(|| {
        args.reads
            .or(told.reads)
            .map(skwir_core::print::booklet::Bound::reading)
    });
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
        bound,
        subject: if several {
            dialog::Subject::Several { pictures: sheet }
        } else {
            dialog::Subject::One
        },
    })
}

/// What `entry` says of itself before the dialog asks (`make::looked_at`:
/// a PDF's pages, the way it reads), what is told within `COUNTING` (a
/// file on a server that answers slowly is not waited for, nor one whose
/// provider is slow): found and read in its remote's lane, which the wait
/// bounds. Nothing where it is not told in time.
fn told(app: &App, entry: &EntryRef, cancel: &CancelToken) -> make::Told {
    let (entry, weak) = (entry.clone(), app.weak());
    soon(
        app,
        &entry.path.clone(),
        "print-told",
        cancel,
        move |token| {
            let app = weak.upgrade()?;
            let path = entry.resolve().ok()?;
            make::looked_at(&app, &path, token).ok()
        },
    )
    .unwrap_or_default()
}

/// Whether `path` is a folder, if it is told within `COUNTING`: a folder
/// typed in the dialog is refused there when it is none, and one on a
/// server that answers slowly is taken as typed, the plan checking it
/// again.
fn is_folder(app: &App, path: &Path, cancel: &CancelToken) -> Option<bool> {
    let asked = path.to_owned();
    soon(app, path, "print-folder", cancel, move |_| {
        Some(holds_a_folder(&asked))
    })
}

/// Whether `path` is a folder, the wait the system makes. On a worker, in
/// its lane.
fn holds_a_folder(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|meta| meta.is_dir())
}

/// What `work` tells of each of `paths`, all asked at once and waited for
/// together, `COUNTING` at most in all: those on one server wait for each
/// other in its lane, never the dialog for each in turn. `None` where it
/// was not told in time. In the order of `paths`.
fn asked_together(
    app: &App,
    paths: &[PathBuf],
    cancel: &CancelToken,
    work: impl Fn(&Path) -> bool + Clone + Send + 'static,
) -> Vec<Option<bool>> {
    let (said, heard) = std::sync::mpsc::channel();
    let working = cancel.child();
    for (at, path) in paths.iter().enumerate() {
        let (said, asked, work) = (said.clone(), path.clone(), work.clone());
        app.scheduler.spawn_in(
            skwir_core::fs::reach::lane(path),
            Priority::Normal,
            "print-folder",
            working.clone(),
            move || {
                let _ = said.send((at, work(&asked)));
            },
        );
    }
    drop(said);
    let mut answers = vec![None; paths.len()];
    let until = std::time::Instant::now() + COUNTING;
    for _ in paths {
        let left = until.saturating_duration_since(std::time::Instant::now());
        let Ok((at, told)) = heard.recv_timeout(left) else {
            break;
        };
        answers[at] = Some(told);
    }
    working.cancel();
    answers
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
    let shown = ws.pane(pane).with_context(no_such_pane)?;
    let location = shown.location().cloned().with_context(shows_nothing)?;
    Ok((location, shown.filter().map(str::to_owned)))
}

/// What the listing of `location` is called in words: its name (a
/// folder's own, a search's, a tag's), with what filters it, said as the
/// window says it: the dialog, the plan and the file's name show it.
/// Plainly (`Words::say_plain`): a file's name, and the PDF's text, hold no
/// mark isolating a name, as everything printed is said.
fn listing_named(app: &App, location: &Location, filter: Option<&str>) -> String {
    listing::filtered(app.location_title(location).say_plain(), filter)
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
/// a plugin's, none marked unavailable), each by its title as the dialog
/// says it, in its order. Read from the places core holds
/// (`registry.places`), whoever made them.
fn sidebar_folders(app: &App) -> Vec<(String, PathBuf)> {
    app.registry
        .places
        .all()
        .into_iter()
        .filter(|place| matches!(place.group.as_str(), "places" | "bookmarks"))
        .filter(|place| place.unavailable.is_none() && place.activate.is_none())
        .filter_map(|place| Some((place.title.say_plain(), place.location.path()?)))
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
                Err(error) => app.notify(NotifyLevel::Error, Words::of_error(error)),
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
) -> anyhow::Result<Option<Words>> {
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
        bound: chosen.bound,
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
                Err(error) => app.notify(NotifyLevel::Error, Words::of_error(error)),
            }
            app.wake();
        },
    );
}

/// Print `request`: `None` when the system's dialog was closed, else what
/// to say. What reads files (a listing, the files, the helpers) runs in
/// their remote's lane; the dialog waits on this worker, holding none.
fn print(app: &Arc<App>, request: Request, cancel: &CancelToken) -> anyhow::Result<Option<Words>> {
    let Request {
        what,
        pages,
        booklet,
    } = request;
    // What it is printed as, named before the dialog asks: the files by
    // their names when asked, a listing as it stands when asked.
    // The job's title is said here: the system's dialog and the PDF's
    // header take text.
    let (what, title, lane, made) = match what {
        What::Files { entries, shown } => {
            let (what, title) = match entries.as_slice() {
                [] => anyhow::bail!(nothing_to_print()),
                [entry] => (
                    Printed::File(display_name(&entry.path)),
                    display_name(&entry.path),
                ),
                [first, rest @ ..] => (
                    Printed::Files(entries.len()),
                    words!(
                        print_job_files = "{ $first } and { $more } more",
                        first = display_name(&first.path),
                        more = rest.len(),
                        note = "The title of a print of several files, in the system's print dialog and the queue: the first's name, then how many more."
                    )
                    .say_plain(),
                ),
            };
            let lane = skwir_core::fs::reach::lane(&entries[0].path);
            (what, title, lane, Make::Files(entries, shown))
        }
        What::Sheet(entries) => {
            let lane =
                skwir_core::fs::reach::lane(&entries.first().with_context(nothing_to_print)?.path);
            (
                Printed::Sheet,
                contact_sheet_title().say_plain(),
                lane,
                Make::Sheet(entries),
            )
        }
        What::Listing(pane) => {
            // Its files read in their lanes; the PDF made of the table
            // reads none, in the shared one.
            let listed = listing::listing(app, pane, cancel)?;
            (
                Printed::Listing(listed.title.clone()),
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
    let task = Arc::new(app.tasks.start(print_title(&what), working.clone()));
    let (weak, token, sent_as, held) = (
        app.weak(),
        working.clone(),
        chosen.clone(),
        Arc::clone(&task),
    );
    let printed = in_lane(app, lane, "print", &working, move || {
        let app = weak.upgrade().with_context(closing)?;
        let work = tempfile::Builder::new()
            .prefix("skwir-print-")
            .tempdir()
            .with_context(work_folder)?;
        // Made, then sent: the sending its last tenth.
        let went = |fraction: f64| held.went(fraction * 0.9);
        let never = || false;
        let follow = skwir_platform::process::Follow::new(&went, &never);
        let ready = made_ready(&app, made, &title, &layout, work.path(), &token, &follow)?;
        // What a helper made is sent only as the file it is.
        make::open_made(&ready, work.path())?;
        held.went(0.9);
        let answered = skwir_platform::print::send(&sent_as, &title, &ready.pdf, &token)?;
        let notice = [ready.notice, ready.folding];
        Ok((
            answered,
            ready.pages,
            notice.into_iter().flatten().collect::<Vec<_>>(),
        ))
    });
    drop(task);
    // Cancelled by its card, not by Skwir closing: said, as no error.
    if working.is_cancelled() && !cancel.is_cancelled() {
        return Ok(Some(not_printed(&what)));
    }
    let (answered, pages, notice) = printed.with_context(|| not_printed(&what))?;
    // Cancelled in a dialog the desktop's printing showed again: as the
    // first one closed, nothing is said.
    if answered == skwir_platform::print::Sent::Cancelled {
        return Ok(None);
    }
    // Where it went: a file, a printer by its name, else the printer.
    let (to, target, printer_name) = match (&chosen.to_file, &chosen.printer) {
        (Some(file), _) => ("file", Name::of(file), String::new()),
        (None, Some(named)) => ("printer", Name(String::new()), named.clone()),
        (None, None) => ("system", Name(String::new()), String::new()),
    };
    let pages = pages.unwrap_or(0);
    let mut said = match answered {
        // What the portal answers when its back end failed, too.
        skwir_platform::print::Sent::Unconfirmed => words!(
            print_handed = "Handed { $pages ->
                    [0] {\"\"}
                    [one] { $pages } page of{\" \"}
                   *[other] { $pages } pages of{\" \"}
                }{ $what ->
                    [file] { $name }
                    [files] { $count ->
                        [one] { $count } file
                       *[other] { $count } files
                    }
                    [sheet] a contact sheet
                   *[listing] the listing of { $name }
                } to the desktop's printing for { $to ->
                    [file] { $target }
                    [printer] { $printer }
                   *[system] the printer
                }, which did not confirm it took it",
            pages = pages,
            what = what.kind(),
            name = what.name(),
            count = what.count(),
            to = to,
            target = target,
            printer = printer_name,
            note = "Said once a print was handed to the desktop's printing, which did not say it took it: how many pages (0 when not counted), what (a file, several, a contact sheet, a listing), and where (a file, a printer by its name, or the printer)."
        ),
        _ => words!(
            print_sent = "Sent { $pages ->
                    [0] {\"\"}
                    [one] { $pages } page of{\" \"}
                   *[other] { $pages } pages of{\" \"}
                }{ $what ->
                    [file] { $name }
                    [files] { $count ->
                        [one] { $count } file
                       *[other] { $count } files
                    }
                    [sheet] a contact sheet
                   *[listing] the listing of { $name }
                } { $to ->
                    [file] to { $target }
                    [printer] to { $printer }
                   *[system] to the printer
                }",
            pages = pages,
            what = what.kind(),
            name = what.name(),
            count = what.count(),
            to = to,
            target = target,
            printer = printer_name,
            note = "Said once a print was sent: how many pages (0 when not counted), what (a file, several, a contact sheet, a listing), and where (a file, a printer by its name, or the printer)."
        ),
    };
    if booklet.is_some() && chosen.duplex == skwir_core::print::Duplex::Simplex {
        said = then(
            said,
            words!(
                print_booklet_one_side = "A booklet is printed on both sides: on one side, print the odd sides, turn the sheets over, then the even",
                note = "Said after a booklet was sent to a printer printing on one side."
            ),
        );
    }
    for notice in notice {
        said = then(said, notice);
    }
    Ok(Some(said))
}

/// What is said when `what` was not printed.
fn not_printed(what: &Printed) -> Words {
    words!(
        print_not_printed = "{ $what ->
            [file] { $name } was not printed
            [files] { $count ->
                [one] { $count } file was not printed
               *[other] { $count } files were not printed
            }
            [sheet] A contact sheet was not printed
           *[listing] The listing of { $name } was not printed
        }",
        what = what.kind(),
        name = what.name(),
        count = what.count(),
        note = "Said when a print was cancelled or failed (why follows): a file by its name, several by how many, a contact sheet, or the listing of a place by its name."
    )
}

/// A contact sheet's title, as its PDF and the system's print dialog say.
fn contact_sheet_title() -> Words {
    words!(
        print_contact_sheet_title = "Contact sheet",
        note =
            "The title of a contact sheet's document, in its header and the system's print dialog."
    )
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
    // What a plan's title says it is saved as.
    let form = if booklet.is_some() { "booklet" } else { "pdf" };
    let folder_of = |path: &Path| -> anyhow::Result<PathBuf> {
        match dest {
            Some(dest) => Ok(dest.to_owned()),
            None => Ok(path.parent().with_context(in_no_folder)?.to_owned()),
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
                [entry] => words!(
                    print_save_one = "{ $form ->
                        [booklet] save { $name } as booklet
                       *[pdf] save { $name } as PDF
                    }",
                    form = form,
                    name = Name::of(&entry.path),
                    note = "A plan's title, in its review, Activity and the History: a file saved as a PDF, or as a booklet's PDF. Begins in lower case: said inside other words too."
                ),
                many if together => words!(
                    print_save_together = "{ $count ->
                        [one] { $form ->
                            [booklet] save { $count } file as one booklet
                           *[pdf] save { $count } file as one PDF
                        }
                       *[other] { $form ->
                            [booklet] save { $count } files as one booklet
                           *[pdf] save { $count } files as one PDF
                        }
                    }",
                    count = many.len(),
                    form = form,
                    note = "A plan's title: several files saved as one PDF, or one booklet's. Begins in lower case."
                ),
                many => words!(
                    print_save_each = "{ $count ->
                        [one] { $form ->
                            [booklet] save { $count } file as booklets
                           *[pdf] save { $count } file as PDFs
                        }
                       *[other] { $form ->
                            [booklet] save { $count } files as booklets
                           *[pdf] save { $count } files as PDFs
                        }
                    }",
                    count = many.len(),
                    form = form,
                    note = "A plan's title: several files saved as a PDF each, or a booklet's each. Begins in lower case."
                ),
            });
            // Together: one step, the others' pages after the first's.
            let groups: Vec<Vec<EntryRef>> = if together {
                vec![entries.clone()]
            } else {
                entries.iter().map(|entry| vec![entry.clone()]).collect()
            };
            anyhow::ensure!(
                given.is_none() || groups.len() == 1,
                words!(
                    print_name_of_one = "A name is a PDF's: these files make one each (`together` makes one of them all)",
                    note = "An error an agent or a script reads: `together` is the command's argument, as written."
                )
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
            let first = entries.first().with_context(nothing_to_print)?;
            let mut plan = Plan::new(words!(
                print_save_sheet = "{ $count ->
                    [one] save a contact sheet of { $count } file as PDF
                   *[other] save a contact sheet of { $count } files as PDF
                }",
                count = entries.len(),
                note = "A plan's title: a contact sheet of the pictures among these files saved as a PDF. Begins in lower case."
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
                None => listing_folder(app, &location).with_context(|| {
                    words!(
                        print_no_listing_folder = "There is no folder to save the listing in: choose one",
                        note = "An error: the place listed has no folder, and the user has no Documents nor home folder."
                    )
                })?,
            };
            let mut plan = Plan::new(words!(
                print_save_listing = "save the listing of { $name } as PDF",
                name = Name(named.clone()),
                note = "A plan's title: what a folder or a place lists saved as a PDF table. Begins in lower case."
            ));
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
        // What it is of, for its words' selectors: a file, a file and how
        // many more, a contact sheet of how many, a place's listing.
        let (what, subject, more, count) = match &self.of {
            Of::Files { entries, .. } => match entries.as_slice() {
                [first, rest @ ..] if !rest.is_empty() => {
                    ("files", display_name(&first.path), rest.len(), 0)
                }
                _ => ("file", display_name(&self.of.from()), 0, 0),
            },
            Of::Sheet { entries } => ("sheet", String::new(), 0, entries.len()),
            Of::Listing { named, .. } => ("listing", named.clone(), 0, 0),
        };
        // Where it goes said too: Save as may put it anywhere.
        let (pdf, folder) = (|| Name::of(&self.target()), || Name::of(&self.dest_dir));
        let name = || Name(subject.clone());
        Change::Create {
            path: self.target(),
            directory: false,
            words: Some(Tenses {
                will: words!(
                    print_save_step = "{ $what ->
                        [file] save { $name } as { $pdf } in { $folder }
                        [files] save { $name } and { $more } more as { $pdf } in { $folder }
                        [sheet] { $count ->
                            [one] save a contact sheet of { $count } file as { $pdf } in { $folder }
                           *[other] save a contact sheet of { $count } files as { $pdf } in { $folder }
                        }
                       *[listing] save the listing of { $name } as { $pdf } in { $folder }
                    }",
                    what = what,
                    name = name(),
                    more = more,
                    count = count,
                    pdf = pdf(),
                    folder = folder(),
                    note = "A step of a plan, in its review: what is saved (a file; a file and how many more; a contact sheet of how many files; a place's listing), the PDF's name and its folder. Begins in lower case."
                ),
                did: words!(
                    print_saved_step = "{ $what ->
                        [file] saved { $name } as { $pdf } in { $folder }
                        [files] saved { $name } and { $more } more as { $pdf } in { $folder }
                        [sheet] { $count ->
                            [one] saved a contact sheet of { $count } file as { $pdf } in { $folder }
                           *[other] saved a contact sheet of { $count } files as { $pdf } in { $folder }
                        }
                       *[listing] saved the listing of { $name } as { $pdf } in { $folder }
                    }",
                    what = what,
                    name = name(),
                    more = more,
                    count = count,
                    pdf = pdf(),
                    folder = folder(),
                    note = "What a step did, said once its plan ran. Begins in lower case."
                ),
                already: Some(words!(
                    print_already_saved = "{ $what ->
                        [file] { $name } was already saved as { $pdf } in { $folder }
                        [files] { $name } and { $more } more were already saved as { $pdf } in { $folder }
                        [sheet] { $count ->
                            [one] a contact sheet of { $count } file was already saved as { $pdf } in { $folder }
                           *[other] a contact sheet of { $count } files was already saved as { $pdf } in { $folder }
                        }
                       *[listing] the listing of { $name } was already saved as { $pdf } in { $folder }
                    }",
                    what = what,
                    name = name(),
                    more = more,
                    count = count,
                    pdf = pdf(),
                    folder = folder(),
                    note = "What a step found done already, said once its plan ran: the PDF of that name in that folder is this one already. Begins in lower case."
                )),
            }),
        }
    }

    fn check(&self, ctx: &StepContext) -> StepStatus {
        let conflict = |reason: Words| StepStatus::Conflict { reason };
        let not_a_folder = |folder: &Path| {
            words!(
                print_not_a_folder = "{ $name } is not a folder",
                name = Name::of(folder),
                note = "Why a PDF is not saved: the folder listed, or the one it goes in, is no folder."
            )
        };
        if let Err(invalid) = op::valid_file_name(&self.name) {
            return conflict(Words::of_error(invalid));
        }
        if self.of.entries().is_empty() && !matches!(self.of, Of::Listing { .. }) {
            return conflict(words!(print_nothing_there = "There is nothing to print"));
        }
        for entry in self.of.entries() {
            if let Err(error) = entry.resolve() {
                return conflict(Words::as_written(error.to_string()));
            }
        }
        if let Of::Listing { location, .. } = &self.of
            && let Some(folder) = location.path()
            && !ctx.is_folder(&folder)
        {
            return conflict(not_a_folder(&folder));
        }
        if !ctx.is_folder(&self.dest_dir) {
            return conflict(not_a_folder(&self.dest_dir));
        }
        // A PDF is made aside and moved into place, which a folder that
        // takes only copies written in place (a camera) does not do.
        if !StepContext::stages_in(&self.dest_dir) {
            return conflict(words!(
                print_copies_only = "{ $name } takes only copies: no PDF is made there",
                name = Name::of(&self.dest_dir),
                note = "Why a PDF is not saved: the folder (a camera's) cannot take a file made aside and moved in."
            ));
        }
        if ctx.exists(&self.target()) {
            return conflict(words!(
                print_already_there = "{ $name } is already there",
                name = Name::of(&self.target())
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
            words: op::MakeWords {
                verb: words!(
                    print_save_verb = "Save as PDF",
                    note = "The button of the question asking what becomes of the name a PDF being saved finds taken: save it, once that is answered."
                ),
                waiting: words!(
                    print_nothing_saved = "Nothing is saved as PDF until you choose.",
                    note = "Under the title of the question asking what becomes of the name a PDF being saved finds taken."
                ),
                cancelled: words!(
                    print_cancelled_save = "Cancelled: nothing was saved as PDF",
                    note = "A PDF being saved found its name taken, asked what becomes of it, and the user cancelled it."
                ),
            },
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
        let app = self.app.get().with_context(|| {
            words!(
                print_plugin_off = "Saving as PDF needs the print plugin on",
                note = "Why a step of a plan made before was refused: the Printing plugin was turned off since."
            )
        })?;
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
                    words!(
                        print_no_longer_shown =
                            "{ $name } is no longer shown, so its listing is not known",
                        name = Name(named.clone()),
                        note = "Why a listing's PDF was not saved: the pane whose listing it is shows another place now."
                    )
                );
                // Read in its files' lanes, from this thread.
                Make::Listing(listing::listing(&app, *pane, cancel)?)
            }
        };
        // Said here: the PDF's title is text.
        let title = match &self.of {
            Of::Sheet { .. } => contact_sheet_title().say_plain(),
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
            .with_context(work_folder)?;
        let (weak, token, at) = (app.weak(), cancel.clone(), work.path().to_owned());
        let smaller = self.smaller;
        let (gauge, inputs) = (ctx.gauge(), self.of.entries().to_vec());
        let lane = skwir_core::fs::reach::lane(&self.of.from());
        let ready = in_lane(&app, lane, "print-save", cancel, move || {
            let app = weak.upgrade().with_context(closing)?;
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
            staging.place(&made, &target).couldnt(|| {
                skwir_core::op::Couldnt::new(
                    &target,
                    words!(
                        print_not_placed = "{ $name } could not be placed in { $folder }",
                        name = Name::of(&target),
                        folder = Name::of(&self.dest_dir),
                        note = "A PDF made could not be put in place; why follows."
                    ),
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
        if let Some(notice) = ready.notice {
            ctx.say(words!(
                print_notice_of = "{ $name }: { $notice }",
                name = Name::of(&target),
                notice = notice,
                note = "Said once PDFs were saved: what was left out of this one."
            ));
        }
        if let Some(folding) = ready.folding {
            ctx.say(folding);
        }
        let made = EntryRef::from_path(&target).with_context(|| {
            words!(
                print_went_missing = "{ $name } went missing as it was made",
                name = Name::of(&target)
            )
        })?;
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
    std::fs::create_dir(&folder).with_context(|| {
        words!(
            print_smaller_folder = "A folder to make the PDF smaller in",
            note = "What could not be made, before why: a temporary folder."
        )
    })?;
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
            .with_context(|| words!(print_not_smaller = "The PDF could not be made smaller"))?;
    Ok(copy.map_or(pdf, |copy| copy.path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use skwir_core::direction::Direction;
    use skwir_core::print::booklet::Bound;

    /// What stands in for the readers and the Markdown preview: a PDF
    /// named `manga.pdf` says it reads right to left, another says
    /// nothing; a Markdown file's one block is its text.
    struct StandIn;

    impl skwir_core::preview::PreviewProvider for StandIn {
        fn id(&self) -> &'static str {
            "stand-in"
        }
        fn priority(&self) -> i32 {
            i32::MAX / 2
        }
        fn accepts(&self, request: &skwir_core::preview::PreviewRequest) -> bool {
            matches!(
                request.mime.as_deref(),
                Some("application/pdf" | "text/markdown" | COMIC | WORD)
            )
        }
        fn inspect(
            &self,
            request: &skwir_core::preview::PreviewRequest,
            _: &skwir_core::inspection::InspectRequest,
            _: &CancelToken,
        ) -> anyhow::Result<Option<skwir_core::inspection::Inspection>> {
            if request.mime.as_deref() != Some("text/markdown") {
                return Ok(None);
            }
            let text = std::fs::read_to_string(&request.entry.path)?;
            let blocks = text
                .lines()
                .map(|line| skwir_core::inspection::Block {
                    text: line.to_owned(),
                    code: line.starts_with("    "),
                    ..skwir_core::inspection::Block::default()
                })
                .collect();
            Ok(Some(skwir_core::inspection::Inspection {
                kind: skwir_core::inspection::InspectionKind::Markdown,
                blocks,
                ..skwir_core::inspection::Inspection::default()
            }))
        }
        fn open_document(
            &self,
            request: &skwir_core::preview::PreviewRequest,
            _: &CancelToken,
        ) -> anyhow::Result<Option<Arc<dyn skwir_core::document::DocumentSession>>> {
            let pages = match request.mime.as_deref() {
                Some("application/pdf") => 3,
                Some(COMIC) => 24,
                // An office document's reader converts it first: never
                // asked before the dialog.
                Some(WORD) => panic!("an office document opened before the dialog"),
                _ => return Ok(None),
            };
            let name = request
                .entry
                .path
                .file_name()
                .and_then(std::ffi::OsStr::to_str);
            let manga = matches!(name, Some("manga.pdf" | "manga.cbz"));
            Ok(Some(Arc::new(Read(skwir_core::document::DocumentInfo {
                pages,
                outlines: Vec::new(),
                reflowable: false,
                labels: Vec::new(),
                direction: manga.then_some(Direction::RightToLeft),
            }))))
        }
    }

    /// A document read, its pages never drawn.
    struct Read(skwir_core::document::DocumentInfo);

    /// A comic's type and a word processor's, as the MIME database names
    /// them by their names.
    const COMIC: &str = "application/vnd.comicbook+zip";
    const WORD: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";

    impl skwir_core::document::DocumentSession for Read {
        fn info(&self) -> &skwir_core::document::DocumentInfo {
            &self.0
        }
        fn render(
            &self,
            _: u32,
            _: u32,
            _: &CancelToken,
        ) -> anyhow::Result<skwir_core::document::DocumentPage> {
            anyhow::bail!("not drawn here")
        }
        fn search(
            &self,
            _: &str,
            _: &CancelToken,
        ) -> anyhow::Result<Vec<skwir_core::document::SearchHit>> {
            Ok(Vec::new())
        }
    }

    /// An app whose previews the stand-in answers, and files of `tmp`
    /// written as named.
    fn standing_in(tmp: &Path) -> (Arc<App>, impl Fn(&str, &[u8]) -> PathBuf + '_) {
        let app = App::new(skwir_core::app::AppConfig::ephemeral(tmp.join("state"))).unwrap();
        app.previews.register("test", Arc::new(StandIn));
        let file = move |name: &str, bytes: &[u8]| {
            let path = tmp.join(name);
            std::fs::write(&path, bytes).unwrap();
            path
        };
        (app, file)
    }

    const PDF: &[u8] = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n";

    /// What the dialog is told of a file before it asks: a PDF's pages
    /// and the way it says it reads, read together, and a comic's alike
    /// from its reader; a text's way, as its first text block with a
    /// letter tells (code passed over); nothing of what says nothing, nor
    /// of an office document, which its reader would convert first.
    #[test]
    fn a_file_is_told_before_the_dialog_asks() {
        let tmp = tempfile::tempdir().unwrap();
        let (app, file) = standing_in(tmp.path());
        let told = |path: &Path| make::looked_at(&app, path, &CancelToken::new()).unwrap();
        let manga = file("manga.pdf", PDF);
        assert_eq!(
            told(&manga),
            make::Told {
                pages: Some(3),
                reads: Some(Direction::RightToLeft)
            }
        );
        let report = file("report.pdf", PDF);
        assert_eq!(told(&report).reads, None);
        let hebrew = file("שלום.md", "2026\n    let code = 1;\nשלום עולם\n".as_bytes());
        assert_eq!(told(&hebrew).reads, Some(Direction::RightToLeft));
        let english = file("l'été.md", "    שלום\nNotes\n".as_bytes());
        assert_eq!(
            told(&english).reads,
            Some(Direction::LeftToRight),
            "its code passed over"
        );
        let digits = file("digits.md", b"2026\n");
        assert_eq!(told(&digits).reads, None);
        // Read as pages (a comic): its pages and its way from its reader,
        // as a PDF's.
        let comic = file("manga.cbz", b"PK\x03\x04");
        assert_eq!(
            told(&comic),
            make::Told {
                pages: Some(24),
                reads: Some(Direction::RightToLeft)
            }
        );
        let western = file("western.cbz", b"PK\x03\x04");
        assert_eq!(
            told(&western),
            make::Told {
                pages: Some(24),
                reads: None
            }
        );
        // An office document is converted to be read: told nothing.
        let letter = file("letter.docx", b"PK\x03\x04");
        assert_eq!(told(&letter), make::Told::default());
    }

    /// The edge a booklet's dialog opens on: the one asked, else the way
    /// the caller says the document reads, else the way the file says it
    /// reads (a PDF's, read with its count) or its text tells, else the
    /// left; never the window's language.
    #[test]
    fn the_dialog_binds_a_booklet_as_the_document_reads() {
        let tmp = tempfile::tempdir().unwrap();
        let (app, file) = standing_in(tmp.path());
        let manga = file("manga.pdf", PDF);
        let report = file("report.pdf", PDF);
        let hebrew = file("שלום.md", "שלום עולם\n".as_bytes());
        let english = file("notes.md", b"Notes\n");
        // Read once before, so that what the dialog tells within its
        // wait is not the type database's first reading.
        for path in [&manga, &hebrew] {
            make::looked_at(&app, path, &CancelToken::new()).unwrap();
        }
        let offered = |path: &Path, args: PrintArgs| {
            let args = PrintArgs {
                entries: vec![EntryArg::from(EntryRef::from_path(path).unwrap())],
                ..args
            };
            offers(&app, &args, &dialog::Last::default(), &CancelToken::new()).unwrap()
        };
        let none = PrintArgs::default;
        let manga_offered = offered(&manga, none());
        assert_eq!(manga_offered.bound, Some(Bound::Right), "a PDF's own");
        assert_eq!(manga_offered.count, Some(3), "read with its count");
        assert_eq!(
            offered(&report, none()).bound,
            None,
            "told nothing: bound as it reads once made"
        );
        assert_eq!(
            offered(&hebrew, none()).bound,
            Some(Bound::Right),
            "its text"
        );
        assert_eq!(offered(&english, none()).bound, Some(Bound::Left));
        // Asked, or told by the caller: over what the file says.
        let asked = |bound| PrintArgs {
            bound: Some(bound),
            ..none()
        };
        let reads = |reads| PrintArgs {
            reads: Some(reads),
            ..none()
        };
        assert_eq!(offered(&manga, asked(Bound::Left)).bound, Some(Bound::Left));
        assert_eq!(
            offered(&english, asked(Bound::Right)).bound,
            Some(Bound::Right)
        );
        assert_eq!(
            offered(&manga, reads(Direction::LeftToRight)).bound,
            Some(Bound::Left)
        );
        assert_eq!(
            offered(&report, reads(Direction::RightToLeft)).bound,
            Some(Bound::Right)
        );
        // Several files: as the first reads.
        let several = PrintArgs {
            entries: [&hebrew, &english]
                .map(|path| EntryArg::from(EntryRef::from_path(path).unwrap()))
                .to_vec(),
            ..none()
        };
        let several = offers(
            &app,
            &several,
            &dialog::Last::default(),
            &CancelToken::new(),
        )
        .unwrap();
        assert_eq!((several.bound, several.count), (Some(Bound::Right), None));
    }

    /// Without a dialog (Save as PDF, `asked`), a booklet is bound on the
    /// edge asked, else as the caller says the document reads, else as it
    /// reads, told as it is made.
    #[test]
    fn a_booklet_asked_without_a_dialog_is_bound_as_asked_or_as_it_reads() {
        let tmp = tempfile::tempdir().unwrap();
        let (app, file) = standing_in(tmp.path());
        let notes = file("notes.md", b"Notes\n");
        let bound = |args: PrintArgs| {
            let args = PrintArgs {
                entries: vec![EntryArg::from(EntryRef::from_path(&notes).unwrap())],
                layout: Arrange::Booklet,
                ..args
            };
            request(&app, &args).unwrap().booklet.unwrap().bound
        };
        assert_eq!(bound(PrintArgs::default()), None, "told as it is made");
        assert_eq!(
            bound(PrintArgs {
                bound: Some(Bound::Right),
                reads: Some(Direction::LeftToRight),
                ..PrintArgs::default()
            }),
            Some(Bound::Right)
        );
        assert_eq!(
            bound(PrintArgs {
                reads: Some(Direction::RightToLeft),
                ..PrintArgs::default()
            }),
            Some(Bound::Right)
        );
        // An agent's or a script's arguments, as JSON.
        let args: PrintArgs =
            serde_json::from_value(serde_json::json!({"bound": "right", "reads": "right_to_left"}))
                .unwrap();
        assert_eq!(
            (args.bound, args.reads),
            (Some(Bound::Right), Some(Direction::RightToLeft))
        );
    }

    /// Remembered folders on a server that stopped answering cost the
    /// dialog one wait, however many they are; the others are told, each
    /// in its place.
    #[test]
    fn folders_that_do_not_answer_cost_one_wait_together() {
        let tmp = tempfile::tempdir().unwrap();
        let app = App::new(skwir_core::app::AppConfig::ephemeral(tmp.path())).unwrap();
        let named = |name: &str| PathBuf::from(format!("/example/{name}"));
        let hung = ["hung a", "hung b", "hung c", "hung d", "hung e"];
        let mut asked: Vec<PathBuf> = vec![named("a folder"), named("no folder")];
        asked.extend(hung.iter().map(|name| named(name)));

        let release = CancelToken::new();
        let released = release.clone();
        let started = std::time::Instant::now();
        let answers = asked_together(&app, &asked, &CancelToken::new(), move |path| {
            while path.to_string_lossy().contains("hung") && !released.is_cancelled() {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            path.ends_with("a folder")
        });
        let waited = started.elapsed();
        release.cancel();

        assert_eq!(answers[..2], [Some(true), Some(false)]);
        assert!(answers[2..].iter().all(Option::is_none));
        assert!(
            waited < COUNTING * 4,
            "{waited:?} for {} folders that do not answer",
            hung.len()
        );
    }

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
                .map(|item| item.title.english())
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
