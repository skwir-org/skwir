//! Print…'s own dialog (ADR-111, revised 2026-11-01): where it goes (a
//! printer, the system's dialog asked next, or a PDF saved as the one
//! line of its folder and name says: beside what it is of unless another
//! folder is typed or chosen, `Saved`, `Kind::Save`),
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
//! turned off, never a setting. A folder chosen for PDFs is kept so too,
//! with the few chosen before it; the one beside what is printed is no
//! choice kept, but where each PDF goes by itself. A name is a document's
//! own, never kept, and so is the edge a booklet is bound on: preset every
//! time as the document reads (`Offers::bound`), never as the last
//! dialog's, nor as the window's language reads.
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use skwir_core::App;
use skwir_core::cancel::CancelToken;
use skwir_core::form::path::{save_folder, save_split};
use skwir_core::form::{Answer, Choice, Field, Form, Kind, Mark, Picture, Values};
use skwir_core::print::booklet::{self, Bound, PerSide};
use skwir_core::words;
use skwir_core::words::Words;

use crate::{Arrange, Printed};

/// What the dialog may offer, known before it asks.
#[derive(Clone, Debug, Default)]
pub(crate) struct Offers {
    /// What is printed, as its title says it: “report.md”, 3 files, the
    /// listing of “letters”.
    pub what: Printed,
    /// The page the preview shows (from 1): Current page is offered.
    pub page: Option<u32>,
    /// Something is selected in the preview: Selection is offered.
    pub selection: bool,
    /// How many pages it has, when known before asking (a PDF's, a
    /// document's the preview reads): said, and drawn on the booklets.
    pub count: Option<u32>,
    /// The edge a booklet is bound on as the dialog opens: the one asked
    /// (`PrintArgs::bound`), else as the document reads (the caller's
    /// `reads`, else what the file says of itself or its text tells, as
    /// it is told before the dialog asks: `crate::told`), else the left.
    /// Drawn so on the booklets' pictures.
    pub bound: Bound,
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
    /// What that folder is, said under it in the list: "Beside it", "The
    /// folder listed".
    pub said: Words,
    /// Where a name typed with no folder goes, said under the field:
    /// "Beside it", "Beside each file…", "In “Documents”".
    pub beside: Words,
    /// The folder another pane shows.
    pub other: Option<PathBuf>,
    /// The user's Documents.
    pub documents: Option<PathBuf>,
    /// The folder chosen for the last PDF, still one: Save as opens on
    /// it.
    pub opens: Option<PathBuf>,
    /// The folders chosen before, still folders, the last first.
    pub recent: Vec<PathBuf>,
    /// The sidebar's folders (its places and bookmarks), each by its
    /// title, in its order.
    pub places: Vec<(String, PathBuf)>,
    /// What the PDF is named unless another name is typed.
    pub named: Names,
}

/// What a PDF is named by itself, by how it is laid out: offered of one
/// PDF only (a file, several as one, a contact sheet, a listing), a PDF
/// each being named after its file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Names {
    /// As shown, and a listing.
    pub shown: String,
    /// Folded as a booklet.
    pub booklet: String,
    /// A contact sheet.
    pub sheet: String,
}

/// What Save as says while it holds a folder alone: several files saved a
/// PDF each.
fn alone() -> Words {
    words!(
        print_alone = "Each PDF named after its file, in this folder",
        note = "Under Save as, while it holds a folder alone: several files saved a PDF each."
    )
}

/// What a name typed without it is given.
const ENDING: &str = ".pdf";

/// The folders chosen before that are offered, at most.
const RECENT: usize = 4;

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
    /// The edge a booklet is bound on.
    pub bound: Bound,
    /// The paper a PDF is laid out on, by name (`Paper::NAMED`).
    pub paper: String,
    /// The folder a PDF is saved in, typed or chosen; where it goes by
    /// itself (`Saved::folder`) without.
    pub dest: Option<PathBuf>,
    /// The PDF's name, typed (its ending added); its own (`Names`)
    /// without.
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
    /// The folder chosen for the last PDF: Save as opens on it; where it
    /// goes by itself without.
    dest: Option<PathBuf>,
}

/// The last dialog's choices, but its pages: the plugin's own, for the
/// app it is added to (`PrintPlugin`), shared by what it registers there.
#[derive(Clone, Default)]
pub(crate) struct Last(Arc<Mutex<Option<Remembered>>>);

impl Last {
    fn held(&self) -> MutexGuard<'_, Option<Remembered>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// What the next dialog opens on: the last choices, else the first
    /// dialog's.
    fn get(&self) -> Remembered {
        self.held().clone().unwrap_or_default()
    }

    /// Forget the last choices: the plugin turned off leaves no trace.
    pub(crate) fn forget(&self) {
        *self.held() = None;
    }
}

/// The folder chosen for the last PDF, kept until Skwir quits (`last`),
/// and those chosen before, kept in the cache (`chosen_folders`): what
/// Save as opens on and offers once each is looked at (`Saved`). Reads
/// the cache: on a worker.
pub(crate) fn kept_folders(app: &App, last: &Last) -> (Option<PathBuf>, Vec<PathBuf>) {
    (last.get().dest, chosen_folders(app))
}

/// Where the folders chosen for PDFs are kept: the cache's, the user's
/// data that may go (`skwir_core::cache`), never a setting.
const FOLDERS_KEY: &[u8] = b"save-folders";

/// The folders chosen for PDFs, the last first, as the cache keeps them
/// across runs: none when it holds none, or none it can read.
pub(crate) fn chosen_folders(app: &App) -> Vec<PathBuf> {
    app.cache
        .namespace(crate::ID)
        .get(FOLDERS_KEY)
        .and_then(|bytes| serde_json::from_slice::<Vec<skwir_core::os_path::PathArg>>(&bytes).ok())
        .map(|kept| {
            kept.into_iter()
                .map(skwir_core::os_path::PathArg::into_inner)
                .collect()
        })
        .unwrap_or_default()
}

/// `chosen` first among the folders chosen before (`kept`), each once,
/// `RECENT` at most.
fn with_chosen(kept: &[PathBuf], chosen: &Path) -> Vec<PathBuf> {
    std::iter::once(chosen.to_owned())
        .chain(kept.iter().filter(|folder| *folder != chosen).cloned())
        .take(RECENT)
        .collect()
}

/// Keep `chosen` first among the folders chosen for PDFs, in the cache:
/// one change at a time (each reads and writes the cache's file), on the
/// dialog's worker. Nothing is said of it in the logs: where the user
/// saves is theirs.
fn keep_chosen(app: &App, chosen: &Path) {
    static KEEPING: Mutex<()> = Mutex::new(());
    let _one = KEEPING.lock().unwrap_or_else(PoisonError::into_inner);
    let kept: Vec<skwir_core::os_path::PathArg> = with_chosen(&chosen_folders(app), chosen)
        .into_iter()
        .map(skwir_core::os_path::PathArg)
        .collect();
    let written = serde_json::to_vec(&kept)
        .map_err(std::io::Error::other)
        .and_then(|bytes| app.cache.namespace(crate::ID).put(FOLDERS_KEY, &bytes));
    if let Err(error) = written {
        tracing::warn!("the folders chosen for PDFs: {error}");
    }
}

/// The form's fields, by key.
const TO: &str = "to";
const SAVE_AS: &str = "save_as";
const PAGES: &str = "pages";
const RANGES: &str = "ranges";
const LAYOUT: &str = "layout";
const FOLD: &str = "fold";
const TRIM: &str = "trim";
const TOGETHER: &str = "together";
const SMALLER: &str = "smaller";
const PER_SIDE: &str = "per_side";
const BOUND: &str = "bound";
const PAPER: &str = "paper";

/// Ask the dialog until it is answered with choices that hold: `None`
/// when it was cancelled, or no window can show it. A folder typed for a
/// PDF must be one, as `is_folder` tells (`None`: not told in time, taken
/// as typed); a name typed that is a folder's there is that folder, the
/// PDF named by itself.
pub(crate) fn ask(
    app: &App,
    offers: &Offers,
    last: &Last,
    is_folder: &dyn Fn(&Path) -> Option<bool>,
    cancel: &CancelToken,
) -> Option<Chosen> {
    let mut form = form(offers, &last.get());
    loop {
        let Some(Answer::Accepted(values)) = app.fill(form.clone(), cancel) else {
            return None;
        };
        let held = chosen(&values, offers).and_then(|chosen| {
            if chosen.to == To::Printer {
                return Ok(chosen);
            }
            into_folder(chosen, values.get(SAVE_AS), &offers.saved, is_folder)
        });
        match held {
            Ok(chosen) => {
                {
                    let mut held = last.held();
                    let before = held.take().unwrap_or_default();
                    *held = Some(remembered(&values, &chosen, &before));
                }
                if chosen.to == To::Pdf
                    && let Some(dest) = &chosen.dest
                {
                    keep_chosen(app, dest);
                }
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
        // Printed, the folders for PDFs are kept as they were.
        dest: match chosen.to {
            To::Pdf => chosen.dest.clone(),
            To::Printer => before.dest.clone(),
        },
    }
}

/// `chosen` once what its Save as typed (`text`) is looked at: a name
/// that is a folder's in the folder it goes in is that folder, the PDF
/// named by itself; a folder typed that is none is refused.
fn into_folder(
    mut chosen: Chosen,
    text: &str,
    saved: &Saved,
    is_folder: &dyn Fn(&Path) -> Option<bool>,
) -> Result<Chosen, Words> {
    let typed = save_split(text.trim()).1;
    let folder = chosen.dest.clone().or_else(|| saved.folder.clone());
    if !typed.is_empty()
        && chosen.name.is_some()
        && let Some(folder) = folder
        && is_folder(&folder.join(typed)) == Some(true)
    {
        let within = folder.join(typed);
        chosen.dest = (saved.folder.as_deref() != Some(within.as_path())).then_some(within);
        chosen.name = None;
    }
    if let Some(dest) = &chosen.dest
        && is_folder(dest) == Some(false)
    {
        return Err(words!(
            print_no_such_folder = "Save as: there is no folder “{ $path }”",
            path = skwir_core::os_path::from_home(dest),
            note = "Said in the dialog: the folder typed in the Save as field is none. The path is as typed, from ~ where it is the home's."
        ));
    }
    Ok(chosen)
}

/// The dialog for `offers`, on the `last` choices where they are offered.
fn form(offers: &Offers, last: &Remembered) -> Form {
    let print = || {
        words!(
            print_dialog_accept = "Print…",
            note = "The dialog's button, a printer chosen: the system's dialog asks next."
        )
    };
    let mut form = Form::new(crate::print_title(&offers.what), print());
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
            words!(
                print_to = "To",
                note =
                    "A field of the Print dialog: where what is printed goes, a printer or a PDF."
            ),
            Kind::Row {
                choices: vec![
                    Choice::new(
                        "printer",
                        words!(
                            print_to_printer = "Printer",
                            note = "A choice of the To field."
                        ),
                    )
                    .accept(print()),
                    Choice::new(
                        "pdf",
                        words!(
                            print_to_pdf = "PDF",
                            note = "A choice of the To field: saved as a PDF file."
                        ),
                    )
                    .accept(words!(
                        print_save = "Save",
                        note = "The dialog's button, a PDF chosen: saves it."
                    )),
                ],
            },
        )
        .value(to),
    );
    form = form.field(save_as(offers));
    let mut pages_offered = vec![Choice::new(
        "all",
        words!(
            print_pages_all = "All",
            note = "A choice of the Pages field: every page."
        ),
    )];
    if let Some(page) = offers.page {
        pages_offered.push(Choice::new(
            "current",
            words!(
                print_pages_current = "Page { $page }",
                page = page,
                note = "A choice of the Pages field: the page the preview shows, by its number."
            ),
        ));
    }
    if offers.selection {
        pages_offered.push(Choice::new(
            "selection",
            words!(
                print_pages_selection = "Selection",
                note = "A choice of the Pages field: what is selected in the preview."
            ),
        ));
    }
    pages_offered.push(Choice::new(
        "ranges",
        words!(
            print_pages_ranges = "Pages…",
            note = "A choice of the Pages field: the pages typed in the field it shows."
        ),
    ));
    form = form
        .field(
            Field::new(
                PAGES,
                pages_title(),
                Kind::Row {
                    choices: pages_offered,
                },
            )
            .value("all"),
        )
        .field(
            Field::text(RANGES, pages_to_print())
                .placeholder(words!(
                    print_ranges_placeholder = "From 1: 1-3, 7, 10- (to the end)",
                    note =
                        "In the empty field of the pages to print: what may be typed, as written."
                ))
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
        Choice::new(
            "as_shown",
            words!(
                print_as_shown = "As shown",
                note = "A card of the Layout field: each page as the preview shows it."
            ),
        )
        .detail(words!(print_as_shown_detail = "Each page on a sheet"))
        .picture(as_shown()),
        Choice::new(
            "booklet",
            words!(
                print_booklet = "Booklet",
                note = "A card of the Layout field: pages laid out to be folded."
            ),
        )
        .detail(words!(print_booklet_detail = "Pages on both sides, folded"))
        .picture(folded(offers.bound)),
    ];
    if offers.sheet() {
        layouts.push(
            Choice::new(
                "contact_sheet",
                words!(
                    print_contact_sheet = "Contact sheet",
                    note = "A card of the Layout field: the pictures among the files, small, several to a page."
                ),
            )
            .detail(words!(print_contact_sheet_detail = "The pictures, several to a page"))
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
    // The document's own, as the pages are: never the last dialog's.
    let bound = offers.bound;
    form = form
        .field(
            Field::new(
                LAYOUT,
                words!(
                    print_layout = "Layout",
                    note = "A field of the Print dialog: how the pages are laid out."
                ),
                Kind::Row { choices: layouts },
            )
            .value(layout),
        )
        .field(
            Field::new(
                PER_SIDE,
                words!(
                    print_per_side_field = "Pages a side",
                    note = "A field of the Print dialog: how many pages a booklet puts on each side of a sheet."
                ),
                Kind::Row {
                    choices: PerSide::ALL
                        .into_iter()
                        .map(|side| {
                            Choice::new(
                                side.count().to_string(),
                                words!(
                                    print_per_side_choice = "{ $count } a side",
                                    count = side.count(),
                                    note = "A card of the Pages a side field: this many pages on each side of a sheet."
                                ),
                            )
                            .detail(match side {
                                PerSide::Two => words!(print_folded_once = "Folded once"),
                                PerSide::Four => {
                                    words!(print_folded_twice = "Folded twice, its top cut")
                                }
                                PerSide::Eight => {
                                    words!(print_folded_three_times = "Folded three times, cut")
                                }
                            })
                            .picture(per_side_picture(side, bound))
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
                words!(
                    print_folded_as = "Folded as",
                    note = "A field of the Print dialog: a booklet's sheets folded all together, or in sets."
                ),
                Kind::Row {
                    choices: vec![
                        Choice::new(
                            "whole",
                            words!(
                                print_one_booklet = "One booklet",
                                note = "A card of the Folded as field: every sheet folded together."
                            ),
                        )
                        .detail(count.map_or_else(
                            || words!(print_every_sheet = "Every sheet in one fold"),
                            |count| sheets_said(count.div_ceil(4)),
                        ))
                        .picture(fold_picture(count, None, bound)),
                        Choice::new(
                            "16",
                            words!(print_sets_of_16 = "Sets of 16 pages"),
                        )
                        .detail(words!(print_four_sheets = "4 sheets to a fold"))
                        .picture(fold_picture(count, Some(16), bound)),
                        Choice::new(
                            "32",
                            words!(print_sets_of_32 = "Sets of 32 pages"),
                        )
                        .detail(words!(print_eight_sheets = "8 sheets to a fold"))
                        .picture(fold_picture(count, Some(32), bound)),
                    ],
                },
            )
            .value(fold)
            .when(LAYOUT, &["booklet"])
            .when(PER_SIDE, &["2"]),
        )
        .field(
            Field::new(
                TRIM,
                words!(
                    print_trim = "Trim white margins",
                    note = "A switch of the Print dialog, for a booklet."
                ),
                Kind::Toggle,
            )
            .help(words!(
                print_trim_help = "Pages fit by their ink, all alike",
                note = "Under the switch trimming a booklet's white margins: every page scaled alike, to fit what is printed on it."
            ))
            .value(last.trim.to_string())
            .when(LAYOUT, &["booklet"]),
        )
        .field(
            Field::new(
                BOUND,
                words!(
                    print_bound_right = "Bound on the right",
                    note = "A switch of the Print dialog, for a booklet: its spine on the right, as a Hebrew or an Arabic book, or a manga, is bound."
                ),
                Kind::Toggle,
            )
            .help(words!(
                print_bound_right_help = "For a book read from right to left",
                note = "Under the switch binding a booklet on the right: its pages are read and turned from right to left, as in Hebrew, Arabic or a manga."
            ))
            // The document's edge every time, never the last dialog's.
            .value((bound == Bound::Right).to_string())
            .when(LAYOUT, &["booklet"]),
        );
    if offers.several() {
        form = form.field(
            Field::new(
                TOGETHER,
                words!(
                    print_pdfs = "PDFs",
                    note = "A field of the Print dialog, several files saved as PDF: one each, or one of them all."
                ),
                Kind::Row {
                    choices: vec![
                        Choice::new("each", words!(print_pdf_each = "A PDF each")),
                        Choice::new("one", words!(print_pdf_one = "One PDF of them all")),
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

/// Save as, said when a PDF is chosen: one line of the folder and the
/// name (`Kind::Save`, which the window completes), opening on the folder
/// chosen last, else where it goes by itself, and on the name what is
/// printed gives (`names`). It offers, each once: where it goes by itself
/// and the other pane's folder; then under their headings the folders
/// chosen before and the sidebar's (Documents where it lists none), each
/// said by its path; those still folders as the dialog was offered
/// (`Saved`).
fn save_as(offers: &Offers) -> Field {
    let saved = &offers.saved;
    let mut folders = Vec::new();
    let mut offered: Vec<PathBuf> = Vec::new();
    let mut offer = |folder: &Path, title: String, said: Words, section: Words| {
        if offered.iter().any(|kept| kept == folder) {
            return;
        }
        offered.push(folder.to_owned());
        folders.push(
            Choice::new(
                skwir_core::os_path::from_home(folder),
                Words::as_written(title),
            )
            .detail(said)
            .section(section),
        );
    };
    // A folder's path is said as written; its title is its name, or a
    // place's as the sidebar says it.
    let path = |folder: &Path| Words::as_written(skwir_core::os_path::from_home(folder));
    let chosen_before = || {
        words!(
            print_chosen_before = "Chosen before",
            note = "A heading of the folders Save as offers: those chosen for earlier PDFs."
        )
    };
    let places = || {
        words!(
            print_places = "Places",
            note = "A heading of the folders Save as offers: the sidebar's places and bookmarks."
        )
    };
    if let Some(folder) = &saved.folder {
        offer(
            folder,
            folder_title(folder),
            saved.said.clone(),
            Words::default(),
        );
    }
    if let Some(folder) = &saved.other {
        offer(
            folder,
            folder_title(folder),
            words!(
                print_other_pane = "The other pane",
                note = "Under a folder Save as offers: the one the split's other pane shows."
            ),
            Words::default(),
        );
    }
    for folder in &saved.recent {
        offer(folder, folder_title(folder), path(folder), chosen_before());
    }
    for (title, folder) in &saved.places {
        offer(folder, title.clone(), path(folder), places());
    }
    if saved.places.is_empty()
        && let Some(folder) = &saved.documents
    {
        offer(folder, folder_title(folder), path(folder), places());
    }
    // The folder alone: the window adds the name the fields give.
    let value = saved
        .opens
        .as_deref()
        .or(saved.folder.as_deref())
        .map_or_else(String::new, |folder| {
            let mut written = skwir_core::os_path::from_home(folder);
            if !written.ends_with(std::path::is_separator) {
                written.push(std::path::MAIN_SEPARATOR);
            }
            written
        });
    Field::new(
        SAVE_AS,
        save_as_title(),
        Kind::Save {
            folders,
            names: names(offers),
            ending: ENDING.to_owned(),
            alone: alone(),
            beside: saved.beside.clone(),
        },
    )
    .help(words!(
        print_save_as_help = "A folder from ~ or the top, then the name: Tab completes a folder",
        note = "Under the Save as field: a folder typed from the home folder (~) or from the top, then the PDF's name. Tab is the key."
    ))
    .value(value)
    .when(TO, &["pdf"])
}

/// The Save as field, and what its refusals begin with.
fn save_as_title() -> Words {
    words!(
        print_save_as = "Save as",
        note = "A field of the Print dialog, a PDF chosen: its folder and name, in one line."
    )
}

/// The Pages field.
fn pages_title() -> Words {
    words!(
        print_pages = "Pages",
        note = "A field of the Print dialog: which pages are printed."
    )
}

/// The field of the pages typed, and what its refusals begin with.
fn pages_to_print() -> Words {
    words!(
        print_pages_to_print = "Pages to print",
        note = "A field of the Print dialog: the pages typed, from 1."
    )
}

/// What the PDF is named by itself, by the layout chosen: of one PDF (a
/// file, several as one, a contact sheet, a listing); several files saved
/// a PDF each hold a folder alone, each named after its file.
fn names(offers: &Offers) -> Vec<skwir_core::form::Named> {
    use skwir_core::form::Named as Held;
    let named = &offers.saved.named;
    if offers.listing() {
        return vec![Held::new(&named.shown)];
    }
    if !offers.several() {
        return vec![
            Held::new(&named.booklet).when(LAYOUT, &["booklet"]),
            Held::new(&named.shown),
        ];
    }
    vec![
        Held::new(&named.sheet).when(LAYOUT, &["contact_sheet"]),
        Held::new(&named.booklet)
            .when(LAYOUT, &["booklet"])
            .when(TOGETHER, &["one"]),
        Held::new(&named.shown)
            .when(LAYOUT, &["as_shown"])
            .when(TOGETHER, &["one"]),
    ]
}

/// What the PDF is named by itself as `layout` lays it out: empty for a
/// PDF each.
fn own_name(offers: &Offers, layout: Arrange, together: bool) -> &str {
    let named = &offers.saved.named;
    match layout {
        Arrange::Listing | Arrange::AsShown if !offers.several() || together => &named.shown,
        Arrange::ContactSheet => &named.sheet,
        Arrange::Booklet if !offers.several() || together => &named.booklet,
        _ => "",
    }
}

/// Where Save as's `text` saves, as `chosen` reads it: the folder typed
/// (`None` for where it goes by itself: none typed, or that one) and the
/// name (`None` for its own, `own`: none typed, or that one; a name typed
/// given its ending). Holding a folder alone (`own` empty: a PDF each),
/// the whole text is the folder.
fn target(
    text: &str,
    saved: &Saved,
    own: &str,
) -> Result<(Option<PathBuf>, Option<OsString>), Words> {
    let text = text.trim();
    let (dir, name) = if own.is_empty() {
        (text, "")
    } else {
        save_split(text)
    };
    let folder = if dir.is_empty() {
        None
    } else {
        Some(folder_typed(dir)?)
    };
    let name = match name {
        "" => None,
        typed if typed == own => None,
        typed => Some(crate::pdf_named(OsStr::new(typed)).map_err(|refused| {
            words!(
                print_save_as_refused = "Save as: { $why }",
                why = refused,
                note = "Said in the dialog: why the name typed in the Save as field is no file's."
            )
        })?),
    };
    let dest = folder.filter(|folder| saved.folder.as_deref() != Some(folder.as_path()));
    Ok((dest, name))
}

/// A folder's name, as a choice titles it; a folder without one (the
/// root, a drive) by its path.
fn folder_title(folder: &Path) -> String {
    folder.file_name().map_or_else(
        || folder.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// The folder `typed` names, as the window that completes it reads it
/// (`form::path`): a whole path, or from `~`.
fn folder_typed(typed: &str) -> Result<PathBuf, Words> {
    let home = skwir_core::os_path::home();
    if home.is_none() && typed.starts_with('~') {
        return Err(words!(
            print_no_home = "Save as: there is no home folder for “~”",
            note = "Said in the dialog: a folder typed from ~ where the user has no home folder."
        ));
    }
    save_folder(typed, home.as_deref()).ok_or_else(|| {
        words!(
            print_no_whole_path = "Save as: “{ $typed }” is no whole path: type it from the top, or from ~",
            typed = typed.to_owned(),
            note = "Said in the dialog: the folder typed, as typed, is neither from the top nor from the home folder (~)."
        )
    })
}

/// The paper a PDF is laid out on, offered when it is saved: a printer's
/// is chosen in the system's dialog.
fn papers(chosen: &str) -> Field {
    Field::new(
        PAPER,
        words!(
            print_paper = "Paper",
            note = "A field of the Print dialog, a PDF chosen: the paper it is laid out on (A4, Letter…)."
        ),
        Kind::Row {
            choices: skwir_core::print::Paper::named_words()
                .into_iter()
                .map(|(name, title)| Choice::new(name, title))
                .collect(),
        },
    )
    .value(chosen)
    .when(TO, &["pdf"])
}

/// The PDF saved made smaller, offered when it is saved.
fn smaller(last: bool) -> Field {
    Field::new(
        SMALLER,
        words!(
            print_smaller = "Smaller file",
            note = "A switch of the Print dialog, a PDF chosen: its pictures made smaller."
        ),
        Kind::Toggle,
    )
    .help(words!(
        print_smaller_help = "Pictures in it at most 150\u{a0}dpi, as Reduce Size's Screen",
        note = "Under the Smaller file switch. Reduce Size is a command's name and Screen its card's, as their own words say them."
    ))
        .value(last.to_string())
        .when(TO, &["pdf"])
}

/// What the dialog's `values` chose of `offers`, or why they are no
/// answer (pages typed that are none).
fn chosen(values: &Values, offers: &Offers) -> Result<Chosen, Words> {
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
            skwir_core::print::parse(typed).map_err(|refused| {
                words!(
                    print_pages_refused = "Pages to print: { $why }",
                    why = refused,
                    note = "Said in the dialog: why the pages typed are none, as the reader of pages explains."
                )
            })?;
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
    let together = offers.several() && to == To::Pdf && values.get(TOGETHER) == "one";
    let (dest, name) = match to {
        To::Pdf => target(
            values.get(SAVE_AS),
            &offers.saved,
            own_name(offers, layout, together),
        )?,
        To::Printer => (None, None),
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
        // A booklet's; the edge the dialog opened on where none is held.
        bound: if values.0.contains_key(BOUND) {
            if values.on(BOUND) {
                Bound::Right
            } else {
                Bound::Left
            }
        } else {
            offers.bound
        },
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
fn pages(count: u32) -> Words {
    words!(
        print_page_count = "{ $count ->
            [one] { $count } page
           *[other] { $count } pages
        }",
        count = count,
        note = "Under the Print dialog's title: how many pages the document has."
    )
}

/// `count` sheets in one fold, said.
fn sheets_said(count: u32) -> Words {
    words!(
        print_sheets_in_fold = "{ $count ->
            [one] { $count } sheet, one fold
           *[other] { $count } sheets in one fold
        }",
        count = count,
        note = "Under the One booklet card: how many sheets its one fold holds."
    )
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

/// A booklet's sheet bound on `bound`: two pages side by side, the fold
/// between, page 1 where it prints (on the right of the sheet's front
/// bound on the left, on its left bound on the right).
fn folded(bound: Bound) -> Picture {
    let left = Picture {
        width: 64,
        height: 36,
        marks: vec![
            boxed(8, 5, 24, 26, false),
            boxed(32, 5, 24, 26, false),
            text(8, 5, 24, 26, "4"),
            text(32, 5, 24, 26, "1"),
        ],
    };
    match bound {
        Bound::Left => left,
        Bound::Right => mirrored(&left),
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

/// `picture` seen in a mirror, left for right.
fn mirrored(picture: &Picture) -> Picture {
    let across = |x: u16, width: u16| picture.width.saturating_sub(x + width);
    let marks = picture
        .marks
        .iter()
        .map(|mark| match mark {
            Mark::Box {
                x,
                y,
                width,
                height,
                shaded,
            } => boxed(across(*x, *width), *y, *width, *height, *shaded),
            Mark::Text {
                x,
                y,
                width,
                height,
                text: written,
            } => text(across(*x, *width), *y, *width, *height, written.clone()),
        })
        .collect();
    Picture {
        width: picture.width,
        height: picture.height,
        marks,
    }
}

/// The most sheets a fold's stack is drawn with, and folds drawn.
/// A sheet of `per_side` pages a side bound on `bound`: its front, the
/// first sheet's pages where they print, those printed upside down
/// shaded.
fn per_side_picture(per_side: PerSide, bound: Bound) -> Picture {
    let pages: Vec<u32> = (0..per_side.per_sheet()).collect();
    let first = booklet::sheets_bound(&pages, None, per_side, bound)
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

/// How a booklet of `count` pages (unknown: none) bound on `bound` folds
/// in signatures of `signature` pages (one booklet without): its first
/// sheet's front over its back, each page's number where it lands (`n`
/// the last page where the count is unknown), and beside them the folds,
/// each a stack of its sheets.
fn fold_picture(count: Option<u32>, signature: Option<u32>, bound: Bound) -> Picture {
    let mut marks = Vec::new();
    // The first sheet: front above, back below, as the order lays them.
    let named = |page: Option<u32>| page.map_or_else(String::new, |page| (page + 1).to_string());
    let (mut front, mut back, folds): ([String; 2], [String; 2], Vec<u16>) = match count {
        Some(count) => {
            let order: Vec<u32> = (0..count.max(1)).collect();
            let sheets = booklet::sheets_bound(&order, signature, PerSide::Two, bound);
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
    // Unknown, its pages said bound on the left: on the right, each side
    // mirrored, as the sheets are.
    if count.is_none() && bound == Bound::Right {
        front.reverse();
        back.reverse();
    }
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

    /// The dialog for `offers`, as a first dialog opens.
    fn opened(offers: &Offers) -> Form {
        super::form(offers, &Remembered::default())
    }

    fn offers() -> Offers {
        Offers {
            what: Printed::File("notes.md".to_owned()),
            page: Some(4),
            selection: true,
            count: Some(12),
            bound: Bound::Left,
            subject: Subject::One,
            saved: Saved {
                folder: Some(PathBuf::from("/example/notes")),
                said: Words::as_written("Beside it"),
                beside: Words::as_written("Beside it"),
                other: Some(PathBuf::from("/example/other")),
                documents: Some(PathBuf::from("/example/Documents")),
                opens: None,
                recent: Vec::new(),
                places: Vec::new(),
                named: Names {
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
        let form = opened(&offers());
        let keys: Vec<&str> = form.fields.iter().map(|f| f.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                TO, SAVE_AS, PAGES, RANGES, LAYOUT, PER_SIDE, FOLD, TRIM, BOUND, PAPER, SMALLER
            ]
        );
        assert_eq!(form.title.english(), "Print “notes.md”");
        assert_eq!(form.intro.english(), "12 pages");
        let titles = |key: &str| -> Vec<String> {
            form.fields
                .iter()
                .find(|f| f.key == key)
                .unwrap()
                .choices()
                .iter()
                .map(|c| c.title.english())
                .collect()
        };
        assert_eq!(titles(PAGES), ["All", "Page 4", "Selection", "Pages…"]);
        assert_eq!(titles(LAYOUT), ["As shown", "Booklet"]);
        let several = opened(&Offers {
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
                TO, SAVE_AS, PAGES, RANGES, LAYOUT, PER_SIDE, FOLD, TRIM, BOUND, TOGETHER, PAPER,
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
        let listing = opened(&Offers {
            subject: Subject::Listing,
            ..offers()
        });
        let keys: Vec<&str> = listing.fields.iter().map(|f| f.key.as_str()).collect();
        assert_eq!(keys, [TO, SAVE_AS, PAGES, RANGES, PAPER, SMALLER]);
    }

    /// The answer read: pages typed checked, what is hidden ignored (a
    /// fold of no booklet, one PDF of a print), the button's verb by
    /// where it goes.
    #[test]
    fn the_answer_is_read_as_shown() {
        let offers = offers();
        let form = opened(&offers);
        let printed = values(&[
            (TO, "printer"),
            (PAGES, "ranges"),
            (RANGES, "1-3, 7"),
            (LAYOUT, "as_shown"),
            (FOLD, "16"),
            (TRIM, "true"),
        ]);
        assert_eq!(form.accept_with(&printed).english(), "Print…");
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
                bound: Bound::Left,
                paper: skwir_platform::print::paper_name().to_owned(),
                dest: None,
                name: None,
            }
        );
        let wrong = values(&[(PAGES, "ranges"), (RANGES, "3-1")]);
        assert!(
            super::chosen(&wrong, &offers)
                .unwrap_err()
                .english()
                .starts_with("Pages to print")
        );
        let saved = values(&[
            (TO, "pdf"),
            (PAGES, "current"),
            (LAYOUT, "booklet"),
            (FOLD, "32"),
            (TRIM, "true"),
        ]);
        assert_eq!(form.accept_with(&saved).english(), "Save");
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
        let left = Bound::Left;
        // 10 pages: 12 sides, the last two blank.
        assert_eq!(
            written(&fold_picture(Some(10), None, left)),
            ["", "1", "2", ""]
        );
        assert_eq!(
            written(&fold_picture(Some(12), None, left)),
            ["12", "1", "2", "11"]
        );
        assert_eq!(
            written(&fold_picture(Some(40), Some(16), left)),
            ["16", "1", "2", "15"]
        );
        assert_eq!(
            written(&fold_picture(None, None, left)),
            ["n", "1", "2", "n−1"]
        );
        // Bound on the right, each side mirrored: page 1 on the left.
        let right = Bound::Right;
        assert_eq!(
            written(&fold_picture(Some(12), None, right)),
            ["1", "12", "11", "2"]
        );
        assert_eq!(
            written(&fold_picture(None, Some(16), right)),
            ["1", "16", "15", "2", "…"],
            "more folds than drawn"
        );
        assert_eq!(
            written(&fold_picture(None, None, right)),
            ["1", "n", "n−1", "2"]
        );
        assert_eq!(
            written(&per_side_picture(PerSide::Four, right)),
            ["4", "5", "1", "8"]
        );
        // Sets of 16 of 40 pages: three folds, the last of 2 sheets.
        let stacks = |picture: &Picture| {
            picture
                .marks
                .iter()
                .filter(|mark| matches!(mark, Mark::Box { x, .. } if *x >= 60))
                .count()
        };
        assert_eq!(stacks(&fold_picture(Some(40), Some(16), left)), 4 + 4 + 2);
        assert_eq!(stacks(&fold_picture(Some(12), None, left)), 3);
        assert_eq!(stacks(&fold_picture(Some(12), None, right)), 3);
    }

    /// Bound on the right is a booklet's switch, shown only while Booklet
    /// is chosen, after Trim white margins, as a file's or several's dialog
    /// offers it, never a listing's.
    #[test]
    fn the_binding_is_offered_for_a_booklet_alone() {
        let form = opened(&offers());
        let field = form.fields.iter().find(|f| f.key == BOUND).unwrap();
        assert!(field.shown_with(&values(&[(LAYOUT, "booklet")])));
        assert!(!field.shown_with(&values(&[(LAYOUT, "as_shown")])));
        assert!(!field.shown_with(&values(&[(LAYOUT, "contact_sheet")])));
        assert_eq!(field.title.english(), "Bound on the right");
        assert_eq!(field.help.english(), "For a book read from right to left");
        assert!(matches!(field.kind, Kind::Toggle));
        let several = opened(&Offers {
            subject: Subject::Several { pictures: true },
            ..offers()
        });
        assert!(several.fields.iter().any(|f| f.key == BOUND));
        let listing = opened(&Offers {
            subject: Subject::Listing,
            ..offers()
        });
        assert!(listing.fields.iter().all(|f| f.key != BOUND));
    }

    /// The Bound on the right switch opens on the edge the document reads by, every
    /// time: never the last dialog's, whatever was chosen there; what is
    /// chosen is answered.
    #[test]
    fn the_binding_opens_as_the_document_reads_never_as_the_last() {
        let value = |form: &Form| {
            form.fields
                .iter()
                .find(|f| f.key == BOUND)
                .unwrap()
                .value
                .clone()
        };
        let hebrew = Offers {
            bound: Bound::Right,
            ..offers()
        };
        assert_eq!(value(&opened(&hebrew)), "true");
        assert_eq!(value(&opened(&offers())), "false");
        // The Layout card's booklet drawn as it is bound: page 1 on the
        // left of the sheet's front bound on the right, else on its right.
        let page_one = |form: &Form| {
            let layout = form.fields.iter().find(|f| f.key == LAYOUT).unwrap();
            let booklet = layout
                .choices()
                .iter()
                .find(|c| c.value == "booklet")
                .unwrap();
            booklet
                .picture
                .as_ref()
                .unwrap()
                .marks
                .iter()
                .find_map(|mark| match mark {
                    Mark::Text { x, text, .. } if text == "1" => Some(*x),
                    _ => None,
                })
                .unwrap()
        };
        let (right, left) = (page_one(&opened(&hebrew)), page_one(&opened(&offers())));
        assert!(right < 32 && left >= 32, "{right} {left}");
        // Chosen on the left for the Hebrew book, then a book read left to
        // right opens on the left, and the Hebrew one on the right again.
        let answered = values(&[(TO, "printer"), (LAYOUT, "booklet"), (BOUND, "false")]);
        let chosen = super::chosen(&answered, &hebrew).unwrap();
        assert_eq!(chosen.bound, Bound::Left);
        let last = remembered(&answered, &chosen, &Remembered::default());
        assert_eq!(value(&super::form(&offers(), &last)), "false");
        assert_eq!(value(&super::form(&hebrew, &last)), "true");
        let answered = values(&[(TO, "pdf"), (LAYOUT, "booklet"), (BOUND, "true")]);
        let chosen = super::chosen(&answered, &offers()).unwrap();
        assert_eq!(chosen.bound, Bound::Right);
        let last = remembered(&answered, &chosen, &last);
        assert_eq!(value(&super::form(&offers(), &last)), "false");
        // Held by no field (no booklet asked): the edge it opened on.
        let printed = values(&[(TO, "printer"), (LAYOUT, "as_shown")]);
        assert_eq!(
            super::chosen(&printed, &hebrew).unwrap().bound,
            Bound::Right
        );
    }

    /// The next dialog opens on the last choices, but its pages.
    #[test]
    fn the_dialog_opens_on_the_last_choices() {
        let last = Remembered {
            to: To::Pdf,
            layout: Arrange::Booklet,
            signature: Some(16),
            trim: true,
            together: false,
            smaller: true,
            per_side: PerSide::Four,
            paper: Some("a5".to_owned()),
            dest: None,
        };
        let form = super::form(&offers(), &last);
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
        assert_eq!(
            opened(&offers()).fields[0].value,
            "printer",
            "a first dialog"
        );
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

    /// Save as is said once a PDF is chosen: the folder it goes in by
    /// itself and the name the layout gives, with the folders it offers;
    /// what is typed is read as a folder and a name (`.pdf` added), a
    /// name alone saving where it goes by itself.
    #[test]
    fn save_as_holds_the_folder_and_the_name() {
        let offers = offers();
        let form = opened(&offers);
        let field = form.fields.iter().find(|f| f.key == SAVE_AS).unwrap();
        assert!(field.shown_with(&values(&[(TO, "pdf")])));
        assert!(!field.shown_with(&values(&[(TO, "printer")])));
        assert_eq!(field.value, "/example/notes/", "the folder alone");
        let Kind::Save {
            folders, ending, ..
        } = &field.kind
        else {
            panic!("a file to write");
        };
        assert_eq!(ending, ".pdf");
        let owned: Vec<(String, String, String)> = folders
            .iter()
            .map(|c| (c.title.english(), c.detail.english(), c.section.english()))
            .collect();
        let titled: Vec<(&str, &str, &str)> = owned
            .iter()
            .map(|(title, detail, section)| (title.as_str(), detail.as_str(), section.as_str()))
            .collect();
        assert_eq!(
            titled,
            [
                ("notes", "Beside it", ""),
                ("other", "The other pane", ""),
                ("Documents", "/example/Documents", "Places")
            ],
            "Documents, the sidebar listing none"
        );
        let held = |layout: &str| field.save_name(&values(&[(LAYOUT, layout)])).to_owned();
        assert_eq!(held("as_shown"), "notes.pdf");
        assert_eq!(held("booklet"), "notes (booklet).pdf");

        let read = |layout: &str, text: &str, offers: &Offers| {
            super::chosen(
                &values(&[(TO, "pdf"), (LAYOUT, layout), (SAVE_AS, text)]),
                offers,
            )
            .map(|c| (c.dest, c.name))
        };
        let shown = |text: &str| read("as_shown", text, &offers);
        assert_eq!(shown("/example/notes/notes.pdf"), Ok((None, None)));
        assert_eq!(shown("/example/notes/"), Ok((None, None)));
        assert_eq!(
            shown("/example/other/ma liste"),
            Ok((
                Some(PathBuf::from("/example/other")),
                Some(OsString::from("ma liste.pdf"))
            ))
        );
        assert_eq!(
            shown("l'été.PDF"),
            Ok((None, Some(OsString::from("l'été.PDF")))),
            "a name alone, where it goes by itself"
        );
        if let Some(home) = skwir_core::os_path::home() {
            assert_eq!(shown("~/x/notes.pdf"), Ok((Some(home.join("x")), None)));
            assert_eq!(shown("~"), Ok((Some(home.clone()), None)));
            // What the window answers for `~` alone, its name added.
            assert_eq!(shown("~/notes.pdf"), Ok((Some(home.clone()), None)));
            assert_eq!(
                shown("~/link/../b/x"),
                Ok((Some(home.join("b")), Some(OsString::from("x.pdf"))))
            );
        }
        // `..` read as the window reads it, by the names written.
        assert_eq!(
            shown("/example/gone/../other/x.pdf"),
            Ok((
                Some(PathBuf::from("/example/other")),
                Some(OsString::from("x.pdf"))
            ))
        );
        assert!(
            shown("letters/x")
                .unwrap_err()
                .english()
                .starts_with("Save as")
        );
        assert_eq!(
            read("booklet", "/example/notes/notes (booklet).pdf", &offers),
            Ok((None, None))
        );
        // Printed, nothing is read.
        let printed = values(&[(TO, "printer"), (SAVE_AS, "letters/x")]);
        assert_eq!(super::chosen(&printed, &offers).map(|c| c.dest), Ok(None));

        // Several files of several folders saved a PDF each: a folder
        // alone; as one, or a contact sheet, named.
        let several = Offers {
            subject: Subject::Several { pictures: true },
            saved: Saved {
                folder: None,
                ..offers.saved.clone()
            },
            ..offers.clone()
        };
        let form = opened(&several);
        let field = form.fields.iter().find(|f| f.key == SAVE_AS).unwrap();
        assert_eq!(field.value, "", "beside each file");
        let each = [(LAYOUT, "as_shown"), (TOGETHER, "each")];
        assert_eq!(field.save_name(&values(&each)), "");
        let one = [(LAYOUT, "as_shown"), (TOGETHER, "one")];
        assert_eq!(field.save_name(&values(&one)), "notes.pdf");
        let sheet = [(LAYOUT, "contact_sheet"), (TOGETHER, "each")];
        assert_eq!(field.save_name(&values(&sheet)), "Contact sheet.pdf");
        let each_in = |text: &str| {
            super::chosen(
                &values(&[
                    (TO, "pdf"),
                    (LAYOUT, "as_shown"),
                    (TOGETHER, "each"),
                    (SAVE_AS, text),
                ]),
                &several,
            )
            .map(|c| (c.dest, c.name))
        };
        assert_eq!(
            each_in("/example/a b"),
            Ok((Some(PathBuf::from("/example/a b")), None))
        );
        assert_eq!(each_in(""), Ok((None, None)));
        assert_eq!(
            read("contact_sheet", "faces", &several),
            Ok((None, Some(OsString::from("faces.pdf"))))
        );
    }

    /// A name typed that is a folder's in the folder it goes in is that
    /// folder, the PDF named by itself; a folder typed that is none is
    /// refused; one not told in time is taken as typed.
    #[test]
    fn a_folder_typed_is_one() {
        let offers = offers();
        let chosen = |text: &str| {
            super::chosen(
                &values(&[(TO, "pdf"), (LAYOUT, "as_shown"), (SAVE_AS, text)]),
                &offers,
            )
            .unwrap()
        };
        let folders = |path: &Path| match path.to_str() {
            Some("/example/notes/drafts" | "/example/other") => Some(true),
            Some("/example/slow") => None,
            _ => Some(false),
        };
        let into = |text: &str| {
            into_folder(chosen(text), text, &offers.saved, &folders).map(|c| (c.dest, c.name))
        };
        assert_eq!(
            into("/example/notes/drafts"),
            Ok((Some(PathBuf::from("/example/notes/drafts")), None))
        );
        assert_eq!(
            into("/example/other/x"),
            Ok((
                Some(PathBuf::from("/example/other")),
                Some(OsString::from("x.pdf"))
            ))
        );
        assert!(
            into("/example/gone/x")
                .unwrap_err()
                .english()
                .starts_with("Save as: there is no folder")
        );
        assert_eq!(
            into("/example/slow/x"),
            Ok((
                Some(PathBuf::from("/example/slow")),
                Some(OsString::from("x.pdf"))
            ))
        );
    }

    /// A folder chosen opens the next Save as and is offered among the few
    /// chosen before, the last first, each once; printing in between keeps
    /// them; where it goes by itself chosen again opens the next there.
    #[test]
    fn the_folders_chosen_before_are_offered() {
        // Kept in the cache, the last first, each once, four at most.
        let tmp = tempfile::tempdir().unwrap();
        let app = App::new(skwir_core::app::AppConfig::ephemeral(tmp.path())).unwrap();
        assert!(chosen_folders(&app).is_empty());
        for folder in [
            "/example/a",
            "/example/b b",
            "/example/a",
            "/example/c",
            "/example/d",
            "/example/l'été",
        ] {
            keep_chosen(&app, Path::new(folder));
        }
        let kept = chosen_folders(&app);
        let recent: Vec<&str> = kept.iter().filter_map(|p| p.to_str()).collect();
        assert_eq!(
            recent,
            ["/example/l'été", "/example/d", "/example/c", "/example/a"]
        );
        assert_eq!(
            with_chosen(&kept, Path::new("/example/c"))[..2],
            [PathBuf::from("/example/c"), PathBuf::from("/example/l'été")]
        );

        // The folder chosen last opens the next Save as, printing in
        // between keeping it; where it goes by itself chosen again opens
        // the next there.
        let offers = offers();
        let pdf = |text: &str| values(&[(TO, "pdf"), (LAYOUT, "as_shown"), (SAVE_AS, text)]);
        let chosen = super::chosen(&pdf("/example/e/x.pdf"), &offers).unwrap();
        let mut last = remembered(&pdf("/example/e/x.pdf"), &chosen, &Remembered::default());
        let printed = values(&[(TO, "printer")]);
        let chosen = super::chosen(&printed, &offers).unwrap();
        last = remembered(&printed, &chosen, &last);
        assert_eq!(last.dest, Some(PathBuf::from("/example/e")));

        // Offered under their headings, said by their paths, each once:
        // the sidebar's places after those chosen before.
        let offered = |opens: Option<PathBuf>| Offers {
            saved: Saved {
                opens,
                recent: kept.clone(),
                places: vec![
                    ("Home".to_owned(), PathBuf::from("/example")),
                    ("Mine".to_owned(), PathBuf::from("/example/c")),
                ],
                ..offers.saved.clone()
            },
            ..offers.clone()
        };
        let field = super::save_as(&offered(last.dest.clone()));
        assert_eq!(field.value, "/example/e/");
        let Kind::Save { folders, .. } = &field.kind else {
            panic!("a file to write");
        };
        let owned: Vec<(String, String, String)> = folders
            .iter()
            .map(|c| (c.title.english(), c.detail.english(), c.section.english()))
            .collect();
        let said: Vec<(&str, &str, &str)> = owned
            .iter()
            .map(|(title, detail, section)| (title.as_str(), detail.as_str(), section.as_str()))
            .collect();
        assert_eq!(
            said,
            [
                ("notes", "Beside it", ""),
                ("other", "The other pane", ""),
                ("l'été", "/example/l'été", "Chosen before"),
                ("d", "/example/d", "Chosen before"),
                ("c", "/example/c", "Chosen before"),
                ("a", "/example/a", "Chosen before"),
                ("Home", "/example", "Places"),
            ]
        );
        let back = super::chosen(&pdf("/example/notes/notes.pdf"), &offers).unwrap();
        let last = remembered(&pdf(""), &back, &last);
        assert_eq!(last.dest, None);
        assert_eq!(super::save_as(&offered(last.dest)).value, "/example/notes/");
    }
}
