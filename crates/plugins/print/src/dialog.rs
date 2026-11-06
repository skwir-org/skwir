//! Print…'s own dialog (ADR-111, revised 2026-11-01): where it goes (a
//! printer, the system's dialog asked next, or a PDF, in the folder said:
//! beside what it is of unless another is chosen, `Saved`),
//! which pages, and how they are laid out (as shown, folded as a booklet,
//! a contact sheet), asked as a form the window draws
//! (`skwir_core::form`). The system's dialog then chooses only the
//! printer, the paper, the copies and the sides: desktops' dialogs differ
//! in what they offer of pages (KDE's portal folds its range away and
//! offers neither the current page nor the selection), so the pages are
//! chosen here, the same on every desktop.
//!
//! It opens on what was chosen last, but the pages, which are a
//! document's own: kept in memory until Skwir quits or the plugin is
//! turned off, never a setting. A folder chosen for PDFs is kept so too;
//! the one beside what is printed is no choice kept, but where each PDF
//! goes by itself.
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use skwir_core::App;
use skwir_core::cancel::CancelToken;
use skwir_core::form::{Answer, Choice, Field, Form, Kind, Mark, Picture, Values};
use skwir_core::print::booklet::{self, PerSide};

use crate::Arrange;

/// What the dialog may offer, known before it asks.
#[derive(Clone, Debug, Default)]
pub(crate) struct Offers {
    /// What is printed, as its title says it: “report.md”, 3 files, the
    /// listing of “letters”.
    pub what: String,
    /// The page the preview shows (from 1): Current page is offered.
    pub page: Option<u32>,
    /// Something is selected in the preview: Selection is offered.
    pub selection: bool,
    /// How many pages it has, when known before asking (a PDF's, a
    /// document's the preview reads): said, and drawn on the booklets.
    pub count: Option<u32>,
    pub subject: Subject,
    /// Where a PDF of it goes, and what else is offered.
    pub saved: Saved,
}

/// Where a PDF is saved unless another folder is chosen, and the folders
/// offered beside it; any other is typed, a whole path or from `~`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Saved {
    /// The folder it goes in: `None` for files in several folders, each
    /// PDF beside its own (one of them all, a contact sheet, beside the
    /// first).
    pub folder: Option<PathBuf>,
    /// What that folder is, said under it: "Beside it", "The folder
    /// listed".
    pub said: String,
    /// The folder another pane shows.
    pub other: Option<PathBuf>,
    /// The user's Documents.
    pub documents: Option<PathBuf>,
    /// What the PDF is named unless another name is typed.
    pub named: Named,
}

/// What a PDF is named by itself, by how it is laid out: offered of one
/// PDF only (a file, several as one, a contact sheet, a listing), a PDF
/// each being named after its file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Named {
    /// As shown, and a listing.
    pub shown: String,
    /// Folded as a booklet.
    pub booklet: String,
    /// A contact sheet.
    pub sheet: String,
}

/// What the Save in field holds for files in several folders, each PDF
/// beside its own: what it reads, as an open choice's value is.
const EACH: &str = "Beside each file";

impl Saved {
    /// What the field holds for where it goes by itself.
    fn value(&self) -> String {
        self.folder
            .as_deref()
            .map_or_else(|| EACH.to_owned(), skwir_core::os_path::from_home)
    }
}

/// What is printed, as the dialog tells them apart.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Subject {
    /// A file.
    #[default]
    One,
    /// Several files, saved as one PDF or a PDF each; with `pictures`
    /// among them, a contact sheet is offered.
    Several { pictures: bool },
    /// A folder's listing: a table, neither folded nor a contact sheet.
    Listing,
}

impl Offers {
    fn several(&self) -> bool {
        matches!(self.subject, Subject::Several { .. })
    }

    fn sheet(&self) -> bool {
        self.subject == Subject::Several { pictures: true }
    }

    fn listing(&self) -> bool {
        self.subject == Subject::Listing
    }
}

/// Where what is printed goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum To {
    /// A printer, which the system's dialog chooses next.
    #[default]
    Printer,
    /// A PDF beside the file (a plan, undone to the trash).
    Pdf,
}

/// What the dialog chose.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Chosen {
    pub to: To,
    /// As `PrintArgs::pages` takes them: `all`, `current`, `selection`,
    /// or pages typed (`1-3, 7`), checked.
    pub pages: String,
    pub layout: Arrange,
    /// Pages folded together; one booklet without.
    pub signature: Option<u32>,
    pub trim: bool,
    /// Several files saved as one PDF.
    pub together: bool,
    /// The PDF saved made smaller, as Reduce Size's Screen makes it
    /// (ADR-112): its images at most 150 dpi.
    pub smaller: bool,
    /// A booklet's pages to a side of a sheet.
    pub per_side: PerSide,
    /// The paper a PDF is laid out on, by name (`Paper::NAMED`).
    pub paper: String,
    /// The folder a PDF is saved in, chosen; where it goes by itself
    /// (`Saved::folder`) without.
    pub dest: Option<PathBuf>,
    /// The PDF's name, typed; its own (`Named`) without.
    pub name: Option<OsString>,
}

/// What the next dialog opens on.
#[derive(Clone, Debug, Default)]
struct Remembered {
    to: To,
    layout: Arrange,
    signature: Option<u32>,
    trim: bool,
    together: bool,
    smaller: bool,
    per_side: PerSide,
    paper: Option<String>,
    /// A folder chosen for PDFs: offered first while it is kept.
    dest: Option<PathBuf>,
}

/// The last dialog's choices, but its pages.
static LAST: Mutex<Option<Remembered>> = Mutex::new(None);

fn last() -> MutexGuard<'static, Option<Remembered>> {
    LAST.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Forget the last choices: the plugin turned off leaves no trace.
pub(crate) fn forget() {
    *last() = None;
}

/// The form's fields, by key.
const TO: &str = "to";
const DEST: &str = "dest";
const NAME: &str = "name";
const NAME_BOOKLET: &str = "name_booklet";
const NAME_SHEET: &str = "name_sheet";
const PAGES: &str = "pages";
const RANGES: &str = "ranges";
const LAYOUT: &str = "layout";
const FOLD: &str = "fold";
const TRIM: &str = "trim";
const TOGETHER: &str = "together";
const SMALLER: &str = "smaller";
const PER_SIDE: &str = "per_side";
const PAPER: &str = "paper";

/// Ask the dialog until it is answered with choices that hold: `None`
/// when it was cancelled, or no window can show it. A folder chosen for a
/// PDF must be one, as `is_folder` tells (`None`: not told in time, taken
/// as typed).
pub(crate) fn ask(
    app: &App,
    offers: &Offers,
    is_folder: &dyn Fn(&Path) -> Option<bool>,
    cancel: &CancelToken,
) -> Option<Chosen> {
    let mut form = form(offers);
    loop {
        let Some(Answer::Accepted(values)) = app.fill(form.clone(), cancel) else {
            return None;
        };
        let held = chosen(&values, offers).and_then(|chosen| match chosen.dest.as_deref() {
            Some(dest) if is_folder(dest) == Some(false) => Err(format!(
                "Save in: “{}” is no folder",
                skwir_core::os_path::from_home(dest)
            )),
            _ => Ok(chosen),
        });
        match held {
            Ok(chosen) => {
                let mut last = last();
                let before = last.take().unwrap_or_default();
                *last = Some(remembered(&values, &chosen, &before));
                return Some(chosen);
            }
            Err(refused) => form = form.again(&values, refused),
        }
    }
}

/// What the next dialog opens on, of `values` answered as `chosen`, the
/// dialog before having opened on `before`. A booklet's own choices (its
/// pages a side, its fold, its trim) are kept as they were set, a booklet
/// chosen or not, and as they were before where the dialog had none (a
/// folder's listing): printing a file as it is in between does not
/// forget them.
fn remembered(values: &Values, chosen: &Chosen, before: &Remembered) -> Remembered {
    let set = |key: &str| values.0.contains_key(key);
    Remembered {
        to: chosen.to,
        layout: chosen.layout,
        signature: if set(FOLD) {
            match values.get(FOLD) {
                "16" => Some(16),
                "32" => Some(32),
                _ => None,
            }
        } else {
            before.signature
        },
        trim: if set(TRIM) {
            values.on(TRIM)
        } else {
            before.trim
        },
        together: chosen.together,
        smaller: chosen.smaller,
        per_side: values
            .get(PER_SIDE)
            .parse()
            .ok()
            .and_then(PerSide::of_count)
            .unwrap_or(before.per_side),
        paper: Some(chosen.paper.clone()),
        // Printed, the folder for PDFs is kept as it was.
        dest: match chosen.to {
            To::Pdf => chosen.dest.clone(),
            To::Printer => before.dest.clone(),
        },
    }
}

/// The dialog for `offers`, on the last choices where they are offered.
pub(crate) fn form(offers: &Offers) -> Form {
    let last = last().clone().unwrap_or_default();
    let mut form = Form::new(format!("Print {}", offers.what), "Print…");
    if let Some(count) = offers.count {
        form = form.intro(pages(count));
    }
    let to = match last.to {
        To::Printer => "printer",
        To::Pdf => "pdf",
    };
    form = form.field(
        Field::new(
            TO,
            "To",
            Kind::Row {
                choices: vec![
                    Choice::new("printer", "Printer").accept("Print…"),
                    Choice::new("pdf", "PDF").accept("Save"),
                ],
            },
        )
        .value(to),
    );
    form = form.field(save_in(&offers.saved, last.dest.as_deref()));
    for field in names(offers) {
        form = form.field(field);
    }
    let mut pages_offered = vec![Choice::new("all", "All")];
    if let Some(page) = offers.page {
        pages_offered.push(Choice::new("current", format!("Page {page}")));
    }
    if offers.selection {
        pages_offered.push(Choice::new("selection", "Selection"));
    }
    pages_offered.push(Choice::new("ranges", "Pages…"));
    form = form
        .field(
            Field::new(
                PAGES,
                "Pages",
                Kind::Row {
                    choices: pages_offered,
                },
            )
            .value("all"),
        )
        .field(
            Field::text(RANGES, "Pages to print")
                .placeholder("From 1: 1-3, 7, 10- (to the end)")
                .required()
                .when(PAGES, &["ranges"]),
        );
    let paper = last
        .paper
        .clone()
        .unwrap_or_else(|| skwir_platform::print::paper_name().to_owned());
    if offers.listing() {
        return form.field(papers(&paper)).field(smaller(last.smaller));
    }
    let mut layouts = vec![
        Choice::new("as_shown", "As shown")
            .detail("Each page on a sheet")
            .picture(as_shown()),
        Choice::new("booklet", "Booklet")
            .detail("Pages on both sides, folded")
            .picture(folded()),
    ];
    if offers.sheet() {
        layouts.push(
            Choice::new("contact_sheet", "Contact sheet")
                .detail("The pictures, several to a page")
                .picture(contact_sheet()),
        );
    }
    let layout = match last.layout {
        Arrange::Booklet => "booklet",
        Arrange::ContactSheet if offers.sheet() => "contact_sheet",
        _ => "as_shown",
    };
    let fold = match last.signature {
        Some(16) => "16",
        Some(32) => "32",
        _ => "whole",
    };
    let count = offers.count;
    let per_side = last.per_side.count().to_string();
    form = form
        .field(Field::new(LAYOUT, "Layout", Kind::Row { choices: layouts }).value(layout))
        .field(
            Field::new(
                PER_SIDE,
                "Pages a side",
                Kind::Row {
                    choices: PerSide::ALL
                        .into_iter()
                        .map(|side| {
                            Choice::new(
                                side.count().to_string(),
                                format!("{} a side", side.count()),
                            )
                            .detail(match side {
                                PerSide::Two => "Folded once",
                                PerSide::Four => "Folded twice, its top cut",
                                PerSide::Eight => "Folded three times, cut",
                            })
                            .picture(per_side_picture(side))
                        })
                        .collect(),
                },
            )
            .value(per_side)
            .when(LAYOUT, &["booklet"]),
        )
        .field(
            Field::new(
                FOLD,
                "Folded as",
                Kind::Row {
                    choices: vec![
                        Choice::new("whole", "One booklet")
                            .detail(count.map_or_else(
                                || "Every sheet in one fold".to_owned(),
                                |count| sheets_said(count.div_ceil(4)),
                            ))
                            .picture(fold_picture(count, None)),
                        Choice::new("16", "Sets of 16 pages")
                            .detail("4 sheets to a fold")
                            .picture(fold_picture(count, Some(16))),
                        Choice::new("32", "Sets of 32 pages")
                            .detail("8 sheets to a fold")
                            .picture(fold_picture(count, Some(32))),
                    ],
                },
            )
            .value(fold)
            .when(LAYOUT, &["booklet"])
            .when(PER_SIDE, &["2"]),
        )
        .field(
            Field::new(TRIM, "Trim white margins", Kind::Toggle)
                .help("Pages fit by their ink, all alike")
                .value(last.trim.to_string())
                .when(LAYOUT, &["booklet"]),
        );
    if offers.several() {
        form = form.field(
            Field::new(
                TOGETHER,
                "PDFs",
                Kind::Row {
                    choices: vec![
                        Choice::new("each", "A PDF each"),
                        Choice::new("one", "One PDF of them all"),
                    ],
                },
            )
            .value(if last.together { "one" } else { "each" })
            // A printer prints them as one job, a contact sheet is one.
            .when(TO, &["pdf"])
            .when(LAYOUT, &["as_shown", "booklet"]),
        );
    }
    form.field(papers(&paper)).field(smaller(last.smaller))
}

/// Where a PDF is saved, said when one is chosen: the folder it goes in
/// by itself first, then the one chosen last, the other pane's and
/// Documents, each once; any other typed. It opens on the one chosen last
/// while it is kept.
fn save_in(saved: &Saved, last: Option<&Path>) -> Field {
    let mut choices = vec![match &saved.folder {
        Some(folder) => Choice::new(saved.value(), folder_title(folder)).detail(saved.said.clone()),
        None => Choice::new(EACH, EACH).detail("One PDF of them all beside the first"),
    }];
    let mut offered: Vec<&Path> = saved.folder.as_deref().into_iter().collect();
    for (folder, said) in [
        (last, "Chosen before"),
        (saved.other.as_deref(), "The other pane"),
        (saved.documents.as_deref(), "Your documents"),
    ] {
        let Some(folder) = folder.filter(|folder| !offered.contains(folder)) else {
            continue;
        };
        offered.push(folder);
        choices.push(
            Choice::new(skwir_core::os_path::from_home(folder), folder_title(folder)).detail(said),
        );
    }
    let value = last.map_or_else(|| saved.value(), skwir_core::os_path::from_home);
    Field::new(
        DEST,
        "Save in",
        Kind::Choice {
            choices,
            open: true,
        },
    )
    // Left empty, it goes where it goes by itself: nothing is needed.
    .help("Choose a folder, or type one")
    .value(value)
    .when(TO, &["pdf"])
}

/// The PDF's name, offered of one PDF: one field for each layout, shown
/// with it, each opening on what that layout names it (`notes.pdf`,
/// `notes (booklet).pdf`, `Contact sheet.pdf`); none for several files
/// saved a PDF each. Never kept: a name is a document's own.
fn names(offers: &Offers) -> Vec<Field> {
    let named = &offers.saved.named;
    let field = |key: &str, value: &str| {
        Field::text(key, "Name")
            .help(".pdf is added when it is left out")
            .value(value)
            .when(TO, &["pdf"])
    };
    if offers.listing() {
        return vec![field(NAME, &named.shown)];
    }
    // Several files make one PDF of them all only when asked.
    let one = |field: Field| {
        if offers.several() {
            field.when(TOGETHER, &["one"])
        } else {
            field
        }
    };
    let mut fields = vec![
        one(field(NAME, &named.shown).when(LAYOUT, &["as_shown"])),
        one(field(NAME_BOOKLET, &named.booklet).when(LAYOUT, &["booklet"])),
    ];
    if offers.sheet() {
        fields.push(field(NAME_SHEET, &named.sheet).when(LAYOUT, &["contact_sheet"]));
    }
    fields
}

/// The name typed in the field of `key`, as `chosen` reads it: `None`
/// for the PDF's own (`given`, or nothing typed); else a name a file can
/// have here, `.pdf` added when it has no such ending.
fn typed_name(values: &Values, key: &str, given: &str) -> Result<Option<OsString>, String> {
    let typed = values.get(key).trim();
    if typed.is_empty() || typed == given {
        return Ok(None);
    }
    if typed.chars().any(std::path::is_separator) || typed.contains('/') {
        return Err(format!(
            "Name: “{typed}” names a folder too: choose the folder in Save in"
        ));
    }
    crate::pdf_named(OsStr::new(typed))
        .map(Some)
        .map_err(|refused| format!("Name: {refused}"))
}

/// A folder's name, as a choice titles it; a folder without one (the
/// root, a drive) by its path.
fn folder_title(folder: &Path) -> String {
    folder.file_name().map_or_else(
        || folder.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// The folder `typed` in Save in names, as `chosen` reads it: `None` for
/// where the PDF goes by itself (`Saved::value`); else a whole path, or
/// from `~`.
fn typed_folder(typed: &str, saved: &Saved) -> Result<Option<PathBuf>, String> {
    let typed = typed.trim();
    if typed.is_empty() || typed == saved.value() {
        return Ok(None);
    }
    let home = typed
        .strip_prefix('~')
        .filter(|rest| rest.is_empty() || rest.starts_with(std::path::is_separator));
    let path = match home {
        Some(rest) => skwir_core::os_path::home()
            .ok_or("Save in: there is no home folder for “~”")?
            .join(rest.trim_start_matches(std::path::is_separator)),
        None => PathBuf::from(typed),
    };
    if !path.is_absolute() {
        return Err(format!(
            "Save in: “{typed}” is no whole path: type it from the top, or from ~"
        ));
    }
    // What the folder offered first holds, typed otherwise (a separator
    // more): it is where the PDF goes by itself still.
    Ok(Some(path).filter(|path| saved.folder.as_deref() != Some(path.as_path())))
}

/// The paper a PDF is laid out on, offered when it is saved: a printer's
/// is chosen in the system's dialog.
fn papers(chosen: &str) -> Field {
    Field::new(
        PAPER,
        "Paper",
        Kind::Row {
            choices: skwir_core::print::Paper::NAMED
                .iter()
                .map(|(name, title)| Choice::new(*name, *title))
                .collect(),
        },
    )
    .value(chosen)
    .when(TO, &["pdf"])
}

/// The PDF saved made smaller, offered when it is saved.
fn smaller(last: bool) -> Field {
    Field::new(SMALLER, "Smaller file", Kind::Toggle)
        .help("Pictures in it at most 150 dpi, as Reduce Size's Screen")
        .value(last.to_string())
        .when(TO, &["pdf"])
}

/// What the dialog's `values` chose of `offers`, or why they are no
/// answer (pages typed that are none).
fn chosen(values: &Values, offers: &Offers) -> Result<Chosen, String> {
    let to = if values.get(TO) == "pdf" {
        To::Pdf
    } else {
        To::Printer
    };
    let pages = match values.get(PAGES) {
        "current" if offers.page.is_some() => "current".to_owned(),
        "selection" if offers.selection => "selection".to_owned(),
        "ranges" => {
            let typed = values.get(RANGES).trim();
            skwir_core::print::parse(typed)
                .map_err(|refused| format!("Pages to print: {refused}"))?;
            typed.to_owned()
        }
        _ => "all".to_owned(),
    };
    let layout = if offers.listing() {
        Arrange::Listing
    } else {
        match values.get(LAYOUT) {
            "booklet" => Arrange::Booklet,
            "contact_sheet" if offers.sheet() => Arrange::ContactSheet,
            _ => Arrange::AsShown,
        }
    };
    let dest = match to {
        To::Pdf => typed_folder(values.get(DEST), &offers.saved)?,
        To::Printer => None,
    };
    let together = offers.several() && to == To::Pdf && values.get(TOGETHER) == "one";
    let named = &offers.saved.named;
    let name = match (to, layout) {
        (To::Printer, _) => None,
        (To::Pdf, Arrange::Listing) => typed_name(values, NAME, &named.shown)?,
        (To::Pdf, Arrange::ContactSheet) => typed_name(values, NAME_SHEET, &named.sheet)?,
        // A PDF each is named after its file.
        (To::Pdf, _) if offers.several() && !together => None,
        (To::Pdf, Arrange::Booklet) => typed_name(values, NAME_BOOKLET, &named.booklet)?,
        (To::Pdf, Arrange::AsShown) => typed_name(values, NAME, &named.shown)?,
    };
    let booklet = layout == Arrange::Booklet;
    // Sets of pages folded together are a booklet's of two a side.
    let two = values.get(PER_SIDE).is_empty() || values.get(PER_SIDE) == "2";
    let signature = match values.get(FOLD) {
        "16" if booklet && two => Some(16),
        "32" if booklet && two => Some(32),
        _ => None,
    };
    Ok(Chosen {
        to,
        pages,
        layout,
        signature,
        trim: booklet && values.on(TRIM),
        together,
        smaller: to == To::Pdf && values.on(SMALLER),
        per_side: values
            .get(PER_SIDE)
            .parse()
            .ok()
            .and_then(PerSide::of_count)
            .filter(|_| booklet)
            .unwrap_or_default(),
        paper: Some(values.get(PAPER))
            .filter(|name| skwir_core::print::Paper::named(name).is_some())
            .map_or_else(
                || skwir_platform::print::paper_name().to_owned(),
                str::to_owned,
            ),
        dest,
        name,
    })
}

/// `count` pages, said.
fn pages(count: u32) -> String {
    if count == 1 {
        "1 page".to_owned()
    } else {
        format!("{count} pages")
    }
}

/// `count` sheets in one fold, said.
fn sheets_said(count: u32) -> String {
    if count == 1 {
        "1 sheet, one fold".to_owned()
    } else {
        format!("{count} sheets in one fold")
    }
}

fn boxed(x: u16, y: u16, width: u16, height: u16, shaded: bool) -> Mark {
    Mark::Box {
        x,
        y,
        width,
        height,
        shaded,
    }
}

fn text(x: u16, y: u16, width: u16, height: u16, text: impl Into<String>) -> Mark {
    Mark::Text {
        x,
        y,
        width,
        height,
        text: text.into(),
    }
}

/// A page as it is shown: a portrait page and its lines.
fn as_shown() -> Picture {
    let mut marks = vec![boxed(19, 1, 26, 34, false)];
    for line in 0..4 {
        let width = if line == 3 { 10 } else { 18 };
        marks.push(boxed(23, 7 + line * 7, width, 3, true));
    }
    Picture {
        width: 64,
        height: 36,
        marks,
    }
}

/// A booklet's sheet: two pages side by side, the fold between.
fn folded() -> Picture {
    Picture {
        width: 64,
        height: 36,
        marks: vec![
            boxed(8, 5, 24, 26, false),
            boxed(32, 5, 24, 26, false),
            text(8, 5, 24, 26, "4"),
            text(32, 5, 24, 26, "1"),
        ],
    }
}

/// A contact sheet: a page of small pictures.
fn contact_sheet() -> Picture {
    let mut marks = vec![boxed(19, 1, 26, 34, false)];
    for row in 0..3 {
        for column in 0..2 {
            marks.push(boxed(22 + column * 11, 4 + row * 10, 9, 8, true));
        }
    }
    Picture {
        width: 64,
        height: 36,
        marks,
    }
}

/// The most sheets a fold's stack is drawn with, and folds drawn.
/// A sheet of `per_side` pages a side: its front, the first sheet's
/// pages where they print, those printed upside down shaded.
fn per_side_picture(per_side: PerSide) -> Picture {
    let pages: Vec<u32> = (0..per_side.per_sheet()).collect();
    let first = booklet::sheets(&pages, None, per_side)
        .into_iter()
        .next()
        .map(|sheet| sheet.front)
        .unwrap_or_default();
    let (columns, rows) = per_side.grid();
    // The sheet in the picture's 64 × 36, as it lies.
    let (cell_w, cell_h): (u16, u16) = match per_side {
        PerSide::Two => (22, 32),
        PerSide::Four => (15, 17),
        PerSide::Eight => (14, 16),
    };
    let width = cell_w * u16::try_from(columns).unwrap_or(1);
    let height = cell_h * u16::try_from(rows).unwrap_or(1);
    let (left, top) = ((64 - width) / 2, (36 - height) / 2);
    let mut marks = Vec::new();
    for (at, cell) in first.iter().enumerate() {
        let at = u16::try_from(at).unwrap_or(0);
        let columns = u16::try_from(columns).unwrap_or(1);
        let x = left + (at % columns) * cell_w;
        let y = top + (at / columns) * cell_h;
        marks.push(boxed(x, y, cell_w, cell_h, cell.turned));
        let page = cell
            .page
            .map_or_else(String::new, |page| (page + 1).to_string());
        marks.push(text(x, y, cell_w, cell_h, page));
    }
    Picture {
        width: 64,
        height: 36,
        marks,
    }
}

const STACK: u16 = 6;
const FOLDS: usize = 3;

/// How a booklet of `count` pages (unknown: none) folds in signatures of
/// `signature` pages (one booklet without): its first sheet's front over
/// its back, each page's number where it lands (`n` the last page where
/// the count is unknown), and beside them the folds, each a stack of its
/// sheets.
fn fold_picture(count: Option<u32>, signature: Option<u32>) -> Picture {
    let mut marks = Vec::new();
    // The first sheet: front above, back below, as the order lays them.
    let named = |page: Option<u32>| page.map_or_else(String::new, |page| (page + 1).to_string());
    let (front, back, folds): ([String; 2], [String; 2], Vec<u16>) = match count {
        Some(count) => {
            let order: Vec<u32> = (0..count.max(1)).collect();
            let sheets = booklet::sheets(&order, signature, PerSide::Two);
            let pages = |cells: Option<&Vec<booklet::Cell>>| -> [Option<u32>; 2] {
                let page = |at: usize| {
                    cells
                        .and_then(|cells| cells.get(at))
                        .and_then(|cell| cell.page)
                };
                [page(0), page(1)]
            };
            let front = pages(sheets.first().map(|sheet| &sheet.front));
            let back = pages(sheets.first().map(|sheet| &sheet.back));
            let per = signature.map_or(sheets.len(), |size| (size as usize).div_ceil(4));
            let folds = sheets
                .chunks(per.max(1))
                .map(|fold| u16::try_from(fold.len()).unwrap_or(u16::MAX))
                .collect();
            (
                [named(front[0]), named(front[1])],
                [named(back[0]), named(back[1])],
                folds,
            )
        }
        None => match signature {
            Some(size) => {
                let sheets = u16::try_from(size / 4).unwrap_or(STACK);
                (
                    [size.to_string(), "1".to_owned()],
                    ["2".to_owned(), (size - 1).to_string()],
                    vec![sheets; FOLDS + 1],
                )
            }
            None => (
                ["n".to_owned(), "1".to_owned()],
                ["2".to_owned(), "n−1".to_owned()],
                vec![STACK + 1],
            ),
        },
    };
    for (y, sides) in [(1u16, &front), (29, &back)] {
        for (at, page) in sides.iter().enumerate() {
            let x = 2 + 24 * u16::try_from(at).unwrap_or(0);
            marks.push(boxed(x, y, 24, 26, false));
            marks.push(text(x, y, 24, 26, page.clone()));
        }
    }
    // The folds, side by side, each its sheets stacked (as many as
    // `STACK` drawn: the card says how many).
    let mut x = 60;
    for (shown, &sheets) in folds.iter().enumerate() {
        if shown == FOLDS {
            marks.push(text(x, 18, 12, 20, "…"));
            break;
        }
        let drawn = sheets.clamp(1, STACK);
        for sheet in 0..drawn {
            let front = sheet + 1 == drawn;
            marks.push(boxed(x + 2 * sheet, 16 + 2 * sheet, 14, 20, !front));
        }
        x += 14 + 2 * drawn + 6;
    }
    Picture {
        width: 146,
        height: 56,
        marks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(pairs: &[(&str, &str)]) -> Values {
        let mut values = Values::default();
        for (key, value) in pairs {
            values.set(*key, *value);
        }
        values
    }

    fn offers() -> Offers {
        Offers {
            what: "“notes.md”".to_owned(),
            page: Some(4),
            selection: true,
            count: Some(12),
            subject: Subject::One,
            saved: Saved {
                folder: Some(PathBuf::from("/example/notes")),
                said: "Beside it".to_owned(),
                other: Some(PathBuf::from("/example/other")),
                documents: Some(PathBuf::from("/example/Documents")),
                named: Named {
                    shown: "notes.pdf".to_owned(),
                    booklet: "notes (booklet).pdf".to_owned(),
                    sheet: "Contact sheet.pdf".to_owned(),
                },
            },
        }
    }

    /// The dialog offers what the file allows: the page shown, what is
    /// selected, a contact sheet only of several files with pictures, how
    /// they are saved only of several, no layout for a listing.
    #[test]
    fn the_dialog_offers_what_is_there() {
        forget();
        let form = form(&offers());
        let keys: Vec<&str> = form.fields.iter().map(|f| f.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                TO,
                DEST,
                NAME,
                NAME_BOOKLET,
                PAGES,
                RANGES,
                LAYOUT,
                PER_SIDE,
                FOLD,
                TRIM,
                PAPER,
                SMALLER
            ]
        );
        assert_eq!(form.title, "Print “notes.md”");
        assert_eq!(form.intro, "12 pages");
        let titles = |key: &str| -> Vec<String> {
            form.fields
                .iter()
                .find(|f| f.key == key)
                .unwrap()
                .choices()
                .iter()
                .map(|c| c.title.clone())
                .collect()
        };
        assert_eq!(titles(PAGES), ["All", "Page 4", "Selection", "Pages…"]);
        assert_eq!(titles(LAYOUT), ["As shown", "Booklet"]);
        let several = super::form(&Offers {
            subject: Subject::Several { pictures: true },
            page: None,
            selection: false,
            count: None,
            ..offers()
        });
        let keys: Vec<&str> = several.fields.iter().map(|f| f.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                TO,
                DEST,
                NAME,
                NAME_BOOKLET,
                NAME_SHEET,
                PAGES,
                RANGES,
                LAYOUT,
                PER_SIDE,
                FOLD,
                TRIM,
                TOGETHER,
                PAPER,
                SMALLER
            ]
        );
        let field = |form: &Form, key: &str| form.fields.iter().find(|f| f.key == key).cloned();
        assert_eq!(
            field(&several, LAYOUT).unwrap().choices().len(),
            3,
            "a contact sheet of several pictures"
        );
        assert_eq!(
            field(&several, PAGES).unwrap().choices().len(),
            2,
            "All and Pages…"
        );
        let listing = super::form(&Offers {
            subject: Subject::Listing,
            ..offers()
        });
        let keys: Vec<&str> = listing.fields.iter().map(|f| f.key.as_str()).collect();
        assert_eq!(keys, [TO, DEST, NAME, PAGES, RANGES, PAPER, SMALLER]);
    }

    /// The answer read: pages typed checked, what is hidden ignored (a
    /// fold of no booklet, one PDF of a print), the button's verb by
    /// where it goes.
    #[test]
    fn the_answer_is_read_as_shown() {
        let offers = offers();
        let form = form(&offers);
        let printed = values(&[
            (TO, "printer"),
            (PAGES, "ranges"),
            (RANGES, "1-3, 7"),
            (LAYOUT, "as_shown"),
            (FOLD, "16"),
            (TRIM, "true"),
        ]);
        assert_eq!(form.accept_with(&printed), "Print…");
        let chosen = chosen(&printed, &offers).unwrap();
        assert_eq!(
            chosen,
            Chosen {
                to: To::Printer,
                pages: "1-3, 7".to_owned(),
                layout: Arrange::AsShown,
                signature: None,
                trim: false,
                together: false,
                smaller: false,
                per_side: PerSide::Two,
                paper: skwir_platform::print::paper_name().to_owned(),
                dest: None,
                name: None,
            }
        );
        let wrong = values(&[(PAGES, "ranges"), (RANGES, "3-1")]);
        assert!(
            super::chosen(&wrong, &offers)
                .unwrap_err()
                .starts_with("Pages to print")
        );
        let saved = values(&[
            (TO, "pdf"),
            (PAGES, "current"),
            (LAYOUT, "booklet"),
            (FOLD, "32"),
            (TRIM, "true"),
        ]);
        assert_eq!(form.accept_with(&saved), "Save");
        let chosen = super::chosen(&saved, &offers).unwrap();
        assert_eq!(chosen.pages, "current");
        assert_eq!(
            (chosen.layout, chosen.signature, chosen.trim),
            (Arrange::Booklet, Some(32), true)
        );
        // Not offered: never chosen.
        let no_page = Offers {
            page: None,
            ..offers.clone()
        };
        assert_eq!(super::chosen(&saved, &no_page).unwrap().pages, "all");
    }

    /// A booklet's picture puts each page where it lands on the first
    /// sheet, and draws a stack for each fold.
    #[test]
    fn a_booklet_s_picture_shows_where_its_pages_land() {
        let written = |picture: &Picture| -> Vec<String> {
            picture
                .marks
                .iter()
                .filter_map(|mark| match mark {
                    Mark::Text { text, .. } => Some(text.clone()),
                    Mark::Box { .. } => None,
                })
                .collect()
        };
        // 10 pages: 12 sides, the last two blank.
        assert_eq!(written(&fold_picture(Some(10), None)), ["", "1", "2", ""]);
        assert_eq!(
            written(&fold_picture(Some(12), None)),
            ["12", "1", "2", "11"]
        );
        assert_eq!(
            written(&fold_picture(Some(40), Some(16))),
            ["16", "1", "2", "15"]
        );
        assert_eq!(written(&fold_picture(None, None)), ["n", "1", "2", "n−1"]);
        // Sets of 16 of 40 pages: three folds, the last of 2 sheets.
        let stacks = |picture: &Picture| {
            picture
                .marks
                .iter()
                .filter(|mark| matches!(mark, Mark::Box { x, .. } if *x >= 60))
                .count()
        };
        assert_eq!(stacks(&fold_picture(Some(40), Some(16))), 4 + 4 + 2);
        assert_eq!(stacks(&fold_picture(Some(12), None)), 3);
    }

    /// The next dialog opens on the last choices, but its pages.
    #[test]
    fn the_dialog_opens_on_the_last_choices() {
        forget();
        *last() = Some(Remembered {
            to: To::Pdf,
            layout: Arrange::Booklet,
            signature: Some(16),
            trim: true,
            together: false,
            smaller: true,
            per_side: PerSide::Four,
            paper: Some("a5".to_owned()),
            dest: None,
        });
        let form = form(&offers());
        let value = |key: &str| {
            form.fields
                .iter()
                .find(|f| f.key == key)
                .unwrap()
                .value
                .clone()
        };
        assert_eq!(value(TO), "pdf");
        assert_eq!(value(PAGES), "all");
        assert_eq!(value(LAYOUT), "booklet");
        assert_eq!(value(FOLD), "16");
        assert_eq!(value(TRIM), "true");
        assert_eq!(value(SMALLER), "true");
        assert_eq!(value(PER_SIDE), "4");
        assert_eq!(value(PAPER), "a5");
        forget();
        assert_eq!(super::form(&offers()).fields[0].value, "printer");
    }

    /// A file printed as it is between two booklets leaves the booklet's
    /// choices as they were set: the next booklet opens on eight a side,
    /// its fold and its trim.
    #[test]
    fn a_booklet_s_choices_outlast_a_print_as_it_is() {
        let answered = values(&[
            (TO, "printer"),
            (PAGES, "all"),
            (LAYOUT, "as_shown"),
            (PER_SIDE, "8"),
            (FOLD, "16"),
            (TRIM, "true"),
        ]);
        let asked = chosen(&answered, &offers()).unwrap();
        assert_eq!(asked.layout, Arrange::AsShown);
        assert_eq!(asked.per_side, PerSide::Two, "what is done is as shown");
        let kept = remembered(&answered, &asked, &Remembered::default());
        assert_eq!(kept.per_side, PerSide::Eight);
        assert_eq!(kept.signature, Some(16));
        assert!(kept.trim);
        // A folder's listing, whose dialog asks no layout: kept as before.
        let listing = values(&[(TO, "printer"), (PAGES, "all")]);
        let listed = chosen(&listing, &offers()).unwrap();
        let kept = remembered(&listing, &listed, &kept);
        assert_eq!(kept.per_side, PerSide::Eight);
        assert_eq!(kept.signature, Some(16));
        assert!(kept.trim);
    }

    /// Where a PDF goes is said once one is chosen: where it goes by
    /// itself first, then the other pane's folder and Documents, any other
    /// typed (a whole path, or from `~`); a folder chosen is the next
    /// dialog's, printing in between keeping it, and the folder offered
    /// first chosen again is no choice kept.
    #[test]
    fn where_a_pdf_goes_is_said_and_chosen() {
        forget();
        let offers = offers();
        let form = form(&offers);
        let field = form.fields.iter().find(|f| f.key == DEST).unwrap();
        assert!(field.shown_with(&values(&[(TO, "pdf")])));
        assert!(!field.shown_with(&values(&[(TO, "printer")])));
        let titled: Vec<(&str, &str)> = field
            .choices()
            .iter()
            .map(|c| (c.title.as_str(), c.detail.as_str()))
            .collect();
        assert_eq!(
            titled,
            [
                ("notes", "Beside it"),
                ("other", "The other pane"),
                ("Documents", "Your documents")
            ]
        );
        assert_eq!(field.value, offers.saved.value(), "where it goes by itself");
        let pdf = |dest: &str| values(&[(TO, "pdf"), (PAGES, "all"), (DEST, dest)]);
        let to = |dest: &str| super::chosen(&pdf(dest), &offers).map(|c| c.dest);
        assert_eq!(to(&offers.saved.value()), Ok(None));
        assert_eq!(to("/example/notes/"), Ok(None), "the same folder");
        assert_eq!(
            to("/example/other"),
            Ok(Some(PathBuf::from("/example/other")))
        );
        if let Some(home) = skwir_core::os_path::home() {
            assert_eq!(to("~/l'été"), Ok(Some(home.join("l'été"))));
        }
        assert!(to("letters").unwrap_err().starts_with("Save in"));
        // Printed, no folder is read.
        let printed = values(&[(TO, "printer"), (DEST, "letters")]);
        assert_eq!(super::chosen(&printed, &offers).unwrap().dest, None);

        // Several files of several folders: each beside its own.
        let each = Offers {
            subject: Subject::Several { pictures: false },
            saved: Saved {
                folder: None,
                said: "Beside them".to_owned(),
                ..offers.saved.clone()
            },
            ..offers.clone()
        };
        let field = super::save_in(&each.saved, None);
        assert_eq!(field.value, EACH);
        assert_eq!(field.choices()[0].title, EACH);
        assert_eq!(
            super::chosen(&pdf(EACH), &each).unwrap().dest,
            None,
            "each beside its own"
        );

        // A folder chosen opens the next dialog, offered first after where
        // it goes by itself; printing in between keeps it.
        let chosen = super::chosen(&pdf("/example/kept"), &offers).unwrap();
        let kept = remembered(&pdf("/example/kept"), &chosen, &Remembered::default());
        assert_eq!(kept.dest, Some(PathBuf::from("/example/kept")));
        let printed = super::chosen(&values(&[(TO, "printer")]), &offers).unwrap();
        let kept = remembered(&values(&[(TO, "printer")]), &printed, &kept);
        assert_eq!(kept.dest, Some(PathBuf::from("/example/kept")));
        let field = super::save_in(&offers.saved, kept.dest.as_deref());
        assert_eq!(
            field.value,
            skwir_core::os_path::from_home(Path::new("/example/kept"))
        );
        assert_eq!(field.choices()[1].detail, "Chosen before");
        // The one offered first, chosen again: where it goes by itself.
        let back = super::chosen(&pdf(&offers.saved.value()), &offers).unwrap();
        assert_eq!(remembered(&pdf(""), &back, &kept).dest, None);
    }

    /// The PDF's name is offered of one PDF, each layout's opening on what
    /// it names it and shown with it; a PDF each is named after its file;
    /// a name typed is taken with `.pdf` added where left out, never one
    /// naming a folder or none a file can have.
    #[test]
    fn a_pdf_s_name_is_typed() {
        forget();
        let offers = offers();
        let form = form(&offers);
        let shown = |key: &str, pairs: &[(&str, &str)]| {
            form.fields
                .iter()
                .find(|f| f.key == key)
                .unwrap()
                .shown_with(&values(pairs))
        };
        let pdf = [(TO, "pdf"), (LAYOUT, "as_shown")];
        assert!(shown(NAME, &pdf));
        assert!(!shown(NAME_BOOKLET, &pdf));
        assert!(shown(NAME_BOOKLET, &[(TO, "pdf"), (LAYOUT, "booklet")]));
        assert!(!shown(NAME, &[(TO, "printer"), (LAYOUT, "as_shown")]));
        let value = |key: &str| {
            form.fields
                .iter()
                .find(|f| f.key == key)
                .unwrap()
                .value
                .clone()
        };
        assert_eq!(value(NAME), "notes.pdf");
        assert_eq!(value(NAME_BOOKLET), "notes (booklet).pdf");

        let named = |pairs: &[(&str, &str)], offers: &Offers| {
            super::chosen(&values(pairs), offers).map(|c| c.name)
        };
        let as_shown =
            |name: &str| named(&[(TO, "pdf"), (LAYOUT, "as_shown"), (NAME, name)], &offers);
        assert_eq!(as_shown("notes.pdf"), Ok(None), "its own");
        assert_eq!(as_shown(""), Ok(None));
        assert_eq!(as_shown("l'été"), Ok(Some(OsString::from("l'été.pdf"))));
        assert_eq!(as_shown("Final.PDF"), Ok(Some(OsString::from("Final.PDF"))));
        assert_eq!(
            as_shown("notes.md"),
            Ok(Some(OsString::from("notes.md.pdf")))
        );
        assert!(as_shown("a/b").unwrap_err().starts_with("Name"));
        // The booklet's own field, the other one's text left aside.
        let booklet = named(
            &[
                (TO, "pdf"),
                (LAYOUT, "booklet"),
                (NAME, "ignored"),
                (NAME_BOOKLET, "folded"),
            ],
            &offers,
        );
        assert_eq!(booklet, Ok(Some(OsString::from("folded.pdf"))));
        // Printed, no name.
        assert_eq!(named(&[(TO, "printer"), (NAME, "x")], &offers), Ok(None));
        // Several: named only as one PDF, or a contact sheet.
        let several = Offers {
            subject: Subject::Several { pictures: true },
            ..offers.clone()
        };
        let each = [
            (TO, "pdf"),
            (LAYOUT, "as_shown"),
            (TOGETHER, "each"),
            (NAME, "x"),
        ];
        assert_eq!(named(&each, &several), Ok(None));
        let one = [
            (TO, "pdf"),
            (LAYOUT, "as_shown"),
            (TOGETHER, "one"),
            (NAME, "x"),
        ];
        assert_eq!(named(&one, &several), Ok(Some(OsString::from("x.pdf"))));
        let sheet = [
            (TO, "pdf"),
            (LAYOUT, "contact_sheet"),
            (TOGETHER, "each"),
            (NAME_SHEET, "faces"),
        ];
        assert_eq!(
            named(&sheet, &several),
            Ok(Some(OsString::from("faces.pdf")))
        );
        let form = super::form(&several);
        let name = form.fields.iter().find(|f| f.key == NAME).unwrap();
        assert!(!name.shown_with(&values(&each)), "a PDF each");
        assert!(name.shown_with(&values(&one)));
    }
}
