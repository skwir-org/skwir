//! Print…'s own dialog (ADR-111, revised 2026-11-01): where it goes (a
//! printer, the system's dialog asked next, or a PDF beside the file),
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
//! turned off, never a setting.
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
/// when it was cancelled, or no window can show it.
pub(crate) fn ask(app: &App, offers: &Offers, cancel: &CancelToken) -> Option<Chosen> {
    let mut form = form(offers);
    loop {
        let Some(Answer::Accepted(values)) = app.fill(form.clone(), cancel) else {
            return None;
        };
        match chosen(&values, offers) {
            Ok(chosen) => {
                *last() = Some(Remembered {
                    to: chosen.to,
                    layout: chosen.layout,
                    signature: chosen.signature,
                    trim: chosen.trim,
                    together: chosen.together,
                    smaller: chosen.smaller,
                    per_side: chosen.per_side,
                    paper: Some(chosen.paper.clone()),
                });
                return Some(chosen);
            }
            Err(refused) => form = form.again(&values, refused),
        }
    }
}

/// The dialog for `offers`, on the last choices where they are offered.
pub(crate) fn form(offers: &Offers) -> Form {
    let last = last().clone().unwrap_or_default();
    let mut form = Form::new(format!("Print {}", offers.what), "Print…");
    if let Some(count) = offers.count {
        form = form.intro(pages(count));
    }
    let beside = if offers.listing() {
        "PDF in the folder"
    } else if offers.several() {
        "PDF beside them"
    } else {
        "PDF beside it"
    };
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
                    Choice::new("pdf", beside).accept("Save"),
                ],
            },
        )
        .value(to),
    );
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
        together: offers.several() && to == To::Pdf && values.get(TOGETHER) == "one",
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
                TO, PAGES, RANGES, LAYOUT, PER_SIDE, FOLD, TRIM, PAPER, SMALLER
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
                TO, PAGES, RANGES, LAYOUT, PER_SIDE, FOLD, TRIM, TOGETHER, PAPER, SMALLER
            ]
        );
        assert_eq!(
            several.fields[3].choices().len(),
            3,
            "a contact sheet of several pictures"
        );
        assert_eq!(several.fields[1].choices().len(), 2, "All and Pages…");
        let listing = super::form(&Offers {
            subject: Subject::Listing,
            ..offers()
        });
        let keys: Vec<&str> = listing.fields.iter().map(|f| f.key.as_str()).collect();
        assert_eq!(keys, [TO, PAGES, RANGES, PAPER, SMALLER]);
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
}
