//! Printing (ADR-111) across plugins: Save as PDF's plan, applied, makes a
//! PDF of what a file's preview shows (a Markdown document's reading view,
//! a source file's code, a picture, a drawing), the pages chosen kept and
//! no other, beside the file, and its undo takes it to the trash; a
//! booklet, a contact sheet, a folder's listing, a search's and the
//! trash's. Print… asks Skwir's own dialog first, answered here as the
//! window would, saying where a PDF goes. The print helper is a
//! default member, built with the tests; without it the test says so and
//! passes, unless `SKWIR_REQUIRE_WORKERS` is set.
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use skwir_core::app::{App, AppConfig};
use skwir_core::command::{Invocation, Outcome};
use skwir_core::form::{Answer, Asked, Values};
use skwir_core::op::{EntryArg, EntryRef};
use skwir_core::plugin::Plugin;

mod common;

/// A helper built beside the tests (a default member, built with them).
fn built(name: &str) -> Option<PathBuf> {
    let name = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.parent()?.join(&name)))
        .filter(|path| path.is_file())
}

fn helper() -> Option<PathBuf> {
    let helper = built("skwir-print-worker");
    if helper.is_none() {
        assert!(
            std::env::var_os("SKWIR_REQUIRE_WORKERS").is_none(),
            "skwir-print-worker is not built"
        );
        eprintln!("skipped: skwir-print-worker is not built");
    }
    helper
}

fn app(root: &Path) -> Option<Arc<App>> {
    with(root, Vec::new())
}

/// The app with the print plugin, the Markdown preview and `more`.
fn with(root: &Path, more: Vec<Arc<dyn Plugin>>) -> Option<Arc<App>> {
    let helper = helper()?;
    // Tests in this binary run in processes of their own under nextest:
    // the variable cannot leak into another test.
    // SAFETY: no other thread of this test process reads the environment.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("SKWIR_PRINT_WORKER", &helper);
        // Pages picked are composed by PDFium's helper (`PDFium` itself the
        // dev shell names).
        if let Some(pdf) = built("skwir-pdf-worker") {
            std::env::set_var("SKWIR_PDF_WORKER", pdf);
        }
    }
    let app = App::new(AppConfig::ephemeral(root.join("state"))).unwrap();
    for plugin in [
        Arc::new(skwir_plugin_markdown::PreviewPlugin) as Arc<dyn Plugin>,
        Arc::new(skwir_plugin_print::PrintPlugin::default()),
    ]
    .into_iter()
    .chain(more)
    {
        app.plugins.add(plugin, &app);
    }
    Some(app)
}

/// Save `path` as PDF with `pages` typed, the plan applied: the PDF made.
fn save(app: &Arc<App>, path: &Path, pages: Option<&str>) -> PathBuf {
    let mut invocation = Invocation::new("print.save_pdf").with(
        "entries",
        [EntryArg::from(EntryRef::from_path(path).unwrap())],
    );
    if let Some(pages) = pages {
        invocation = invocation.with("pages", pages);
    }
    let Outcome::Plan(plan) = app.dispatch(&invocation).unwrap() else {
        panic!("Save as PDF makes a plan");
    };
    let result = app.executor.execute_blocking(&plan, app.lifetime());
    assert!(result.error.is_none(), "{:?}", result.error);
    let made = path.with_extension("pdf");
    assert!(made.is_file(), "{} made", made.display());
    made
}

/// The pages of a PDF, read back; its text, as Poppler's `pdftotext` reads
/// it when the dev shell has it.
fn pages(pdf: &Path) -> usize {
    let bytes = std::fs::read(pdf).unwrap();
    assert!(bytes.starts_with(b"%PDF-"), "{} is a PDF", pdf.display());
    lopdf::Document::load_mem(&bytes).unwrap().get_pages().len()
}

fn text(pdf: &Path) -> Option<String> {
    let output = std::process::Command::new("pdftotext")
        .arg(pdf)
        .arg("-")
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

#[test]
fn what_a_preview_shows_is_saved_as_a_pdf_beside_it() {
    let tmp = tempfile::tempdir().unwrap();
    let Some(app) = app(tmp.path()) else { return };
    let dir = tmp.path().join("files");
    std::fs::create_dir(&dir).unwrap();

    // A Markdown document, as its reading view shows it.
    let markdown = dir.join("notes.md");
    std::fs::write(
        &markdown,
        "# Groceries for Sunday\n\nApples, *pears* and **plums**.\n\n- bread\n- cheese\n\n| What | How many |\n| --- | --- |\n| eggs | 6 |\n",
    )
    .unwrap();
    let pdf = save(&app, &markdown, None);
    assert!(pages(&pdf) >= 1);
    if let Some(text) = text(&pdf) {
        assert!(text.contains("Groceries for Sunday"), "{text}");
        assert!(text.contains("cheese"), "{text}");
    }

    // A source file, as the code view shows it: numbered lines.
    let code = dir.join("main.rs");
    let source = (1..=300).fold(String::new(), |mut source, n| {
        use std::fmt::Write as _;
        let _ = writeln!(source, "let line_{n} = {n}; // the {n}th");
        source
    });
    std::fs::write(&code, source).unwrap();
    let pdf = save(&app, &code, None);
    let all = pages(&pdf);
    assert!(all > 2, "300 lines take several pages: {all}");
    if let Some(text) = text(&pdf) {
        assert!(text.contains("let line_1 = 1;"), "{text}");
        assert!(text.contains("let line_300 = 300;"), "{text}");
    }
    // Its pages typed: the second alone.
    std::fs::remove_file(&pdf).unwrap();
    let pdf = save(&app, &code, Some("2"));
    assert_eq!(pages(&pdf), 1);
    if let Some(text) = text(&pdf) {
        assert!(!text.contains("let line_1 = 1;"), "{text}");
    }

    // A picture and a drawing, as the window shows them itself.
    let picture = dir.join("square.png");
    std::fs::write(&picture, common::synthetic_png(64, 48)).unwrap();
    assert_eq!(pages(&save(&app, &picture, None)), 1);
    let drawing = dir.join("shape.svg");
    std::fs::write(
        &drawing,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="60"><rect width="80" height="40" fill="teal"/></svg>"#,
    )
    .unwrap();
    assert_eq!(pages(&save(&app, &drawing, None)), 1);
}

#[test]
fn a_pdf_saved_is_undone_to_the_trash() {
    let tmp = tempfile::tempdir().unwrap();
    let Some(app) = app(tmp.path()) else { return };
    let dir = tmp.path().join("files");
    std::fs::create_dir(&dir).unwrap();
    let text = dir.join("letter.txt");
    std::fs::write(&text, "Dear reader,\nthis is printed.\n").unwrap();
    let invocation = Invocation::new("print.save_pdf").with(
        "entries",
        [EntryArg::from(EntryRef::from_path(&text).unwrap())],
    );
    let Outcome::Plan(plan) = app.dispatch(&invocation).unwrap() else {
        panic!("Save as PDF makes a plan");
    };
    let result = app.executor.execute_blocking(&plan, app.lifetime());
    assert!(result.error.is_none(), "{:?}", result.error);
    let pdf = dir.join("letter.pdf");
    assert!(pdf.is_file());
    assert!(result.undoable);
    let undone = app.executor.undo_blocking(app.lifetime()).expect("an undo");
    assert!(undone.error.is_none(), "{:?}", undone.error);
    assert!(!pdf.exists(), "taken to the trash");
    // A name taken is said so, never written over.
    std::fs::write(&pdf, b"kept").unwrap();
    let Outcome::Plan(plan) = app.dispatch(&invocation).unwrap() else {
        panic!("Save as PDF makes a plan");
    };
    let result = app.executor.execute_blocking(&plan, app.lifetime());
    assert!(result.error.is_some() || std::fs::read(&pdf).unwrap() == b"kept");
}

#[test]
fn a_booklet_saved_folds_two_pages_to_a_landscape_side() {
    let tmp = tempfile::tempdir().unwrap();
    let Some(app) = app(tmp.path()) else { return };
    let dir = tmp.path().join("files");
    std::fs::create_dir(&dir).unwrap();
    let code = dir.join("long.rs");
    let source = (1..=400).fold(String::new(), |mut source, n| {
        use std::fmt::Write as _;
        let _ = writeln!(source, "let value_{n} = {n};");
        source
    });
    std::fs::write(&code, source).unwrap();
    let invocation = Invocation::new("print.save_pdf")
        .with(
            "entries",
            [EntryArg::from(EntryRef::from_path(&code).unwrap())],
        )
        .with("layout", "booklet");
    let Outcome::Plan(plan) = app.dispatch(&invocation).unwrap() else {
        panic!("Save as PDF makes a plan");
    };
    let result = app.executor.execute_blocking(&plan, app.lifetime());
    if result
        .error
        .as_deref()
        .is_some_and(|error| error.contains("needs PDFium"))
    {
        eprintln!("skipped: no PDF helper or PDFium to fold with");
        return;
    }
    assert!(result.error.is_none(), "{:?}", result.error);
    let booklet = dir.join("long (booklet).pdf");
    let document = lopdf::Document::load(&booklet).unwrap();
    let pages = document.get_pages();
    assert!(
        pages.len() >= 2 && pages.len().is_multiple_of(2),
        "{} sides",
        pages.len()
    );
    for (_, page) in pages {
        let media = document
            .get_dictionary(page)
            .unwrap()
            .get(b"MediaBox")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_float().unwrap_or_else(|_| n.as_i64().unwrap() as f32))
            .collect::<Vec<f32>>();
        assert!(
            media[2] - media[0] > media[3] - media[1],
            "a landscape side"
        );
    }
}

/// Four to a side on A5 asked: the sides are upright A5 sheets, a
/// gathering of eight pages each, the text laid out at a quarter of one.
#[test]
fn a_booklet_of_four_a_side_is_saved_on_the_paper_asked() {
    let tmp = tempfile::tempdir().unwrap();
    let Some(app) = app(tmp.path()) else { return };
    let dir = tmp.path().join("files");
    std::fs::create_dir(&dir).unwrap();
    let code = dir.join("short.rs");
    let source = (1..=60).fold(String::new(), |mut source, n| {
        use std::fmt::Write as _;
        let _ = writeln!(source, "let value_{n} = {n};");
        source
    });
    std::fs::write(&code, source).unwrap();
    let invocation = Invocation::new("print.save_pdf")
        .with(
            "entries",
            [EntryArg::from(EntryRef::from_path(&code).unwrap())],
        )
        .with("layout", "booklet")
        .with("per_side", 4)
        .with("paper", "a5");
    let Outcome::Plan(plan) = app.dispatch(&invocation).unwrap() else {
        panic!("Save as PDF makes a plan");
    };
    let result = app.executor.execute_blocking(&plan, app.lifetime());
    if result
        .error
        .as_deref()
        .is_some_and(|error| error.contains("needs PDFium"))
    {
        eprintln!("skipped: no PDF helper or PDFium to fold with");
        return;
    }
    assert!(result.error.is_none(), "{:?}", result.error);
    // How to fold it is said with what was made: the plan's end carries
    // it, for its toast and a script's answer.
    let said = app
        .executor
        .events()
        .try_iter()
        .find_map(|event| match event {
            skwir_core::op::ExecEvent::Finished { plan: id, said, .. } if id == plan.id => {
                Some(said)
            }
            _ => None,
        })
        .expect("the plan's end was told");
    assert!(
        said.iter()
            .any(|note| note.starts_with("Fold each sheet in half twice")),
        "{said:?}"
    );
    let booklet = dir.join("short (booklet).pdf");
    let document = lopdf::Document::load(&booklet).unwrap();
    let pages = document.get_pages();
    assert!(pages.len().is_multiple_of(2), "{} sides", pages.len());
    for (_, page) in pages {
        let media: Vec<f32> = document
            .get_dictionary(page)
            .unwrap()
            .get(b"MediaBox")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_float().unwrap_or_else(|_| n.as_i64().unwrap() as f32))
            .collect();
        let (width, height) = (media[2] - media[0], media[3] - media[1]);
        // A5 is 148 × 210 mm: 419.5 × 595.3 points, upright.
        assert!((width - 419.53).abs() < 1.0, "{width} wide");
        assert!((height - 595.28).abs() < 1.0, "{height} high");
    }
    // An unknown paper, or pages a side a booklet has not, are refused.
    let refused = app.dispatch(
        &Invocation::new("print.save_pdf")
            .with(
                "entries",
                [EntryArg::from(EntryRef::from_path(&code).unwrap())],
            )
            .with("paper", "tabloid"),
    );
    assert!(refused.is_err());
    let refused = app.dispatch(
        &Invocation::new("print.save_pdf")
            .with(
                "entries",
                [EntryArg::from(EntryRef::from_path(&code).unwrap())],
            )
            .with("layout", "booklet")
            .with("per_side", 6),
    );
    assert!(refused.is_err());
}

#[test]
fn several_files_are_saved_each_as_a_pdf_or_together_as_one() {
    let tmp = tempfile::tempdir().unwrap();
    let Some(app) = app(tmp.path()) else { return };
    let dir = tmp.path().join("files");
    std::fs::create_dir(&dir).unwrap();
    let letter = dir.join("letter 'one'.txt");
    std::fs::write(&letter, "Dear reader,\nthis is printed.\n").unwrap();
    let notes = dir.join("nötes.md");
    std::fs::write(&notes, "# Shopping list\n\n- bread\n").unwrap();
    let noise = dir.join("noise.bin");
    std::fs::write(&noise, [0u8, 159, 146, 150, 0, 1, 2, 3].repeat(64)).unwrap();
    let entries = |paths: &[&Path]| {
        paths
            .iter()
            .map(|path| EntryArg::from(EntryRef::from_path(path).unwrap()))
            .collect::<Vec<_>>()
    };
    let apply = |invocation: &Invocation| {
        let Outcome::Plan(plan) = app.dispatch(invocation).unwrap() else {
            panic!("Save as PDF makes a plan");
        };
        let steps = plan.steps.len();
        let result = app.executor.execute_blocking(&plan, app.lifetime());
        (steps, result)
    };

    // Each its own PDF, beside it.
    let each = Invocation::new("print.save_pdf").with("entries", entries(&[&letter, &notes]));
    let (steps, result) = apply(&each);
    assert_eq!(steps, 2);
    assert!(result.error.is_none(), "{:?}", result.error);
    let (letter_pdf, notes_pdf) = (dir.join("letter 'one'.pdf"), dir.join("nötes.pdf"));
    assert_eq!(pages(&letter_pdf), 1);
    assert_eq!(pages(&notes_pdf), 1);
    std::fs::remove_file(&letter_pdf).unwrap();
    std::fs::remove_file(&notes_pdf).unwrap();

    // Together: one PDF named after the first, their pages in order, what
    // cannot be printed left out.
    let together = Invocation::new("print.save_pdf")
        .with("entries", entries(&[&letter, &noise, &notes]))
        .with("together", true);
    let (steps, result) = apply(&together);
    assert_eq!(steps, 1);
    if result
        .error
        .as_deref()
        .is_some_and(|error| error.contains("needs PDFium"))
    {
        eprintln!("skipped: no PDF helper or PDFium to join with");
        return;
    }
    assert!(result.error.is_none(), "{:?}", result.error);
    assert_eq!(pages(&letter_pdf), 2);
    assert!(!notes_pdf.exists(), "one PDF of them all");
    if let Some(text) = text(&letter_pdf) {
        let (dear, shopping) = (
            text.find("Dear reader").expect("the letter"),
            text.find("Shopping list").expect("the notes"),
        );
        assert!(dear < shopping, "in their order: {text}");
    }
}

/// `path` named in a command's arguments.
fn named(path: &Path) -> [EntryArg; 1] {
    [EntryArg::from(EntryRef::from_path(path).unwrap())]
}

/// A PDF saved whole is the file as it is: neither opened nor counted,
/// so it needs no document reader, even one that cannot read it.
#[test]
fn a_pdf_saved_whole_is_copied_unread() {
    let tmp = common::home_tempdir();
    let Some(app) = app(tmp.path()) else { return };
    let dir = tmp.path().join("files");
    std::fs::create_dir(&dir).unwrap();
    let pdf = dir.join("report.pdf");
    // No reader opens this: anything that read it would fail.
    std::fs::write(&pdf, b"%PDF-1.7 not a document any reader opens").unwrap();
    applied(
        &app,
        &Invocation::new("print.save_pdf").with("entries", named(&pdf)),
    );
    let printed = dir.join("report (printed).pdf");
    assert_eq!(
        std::fs::read(&printed).unwrap(),
        std::fs::read(&pdf).unwrap()
    );
}

/// A link is saved beside itself, named after itself, as what it opens
/// shows: its target's type, read through it.
#[cfg(unix)]
#[test]
fn a_link_is_saved_beside_itself_as_its_target_shows() {
    let tmp = common::home_tempdir();
    let Some(app) = app(tmp.path()) else { return };
    let (here, there) = (tmp.path().join("here"), tmp.path().join("there"));
    std::fs::create_dir(&here).unwrap();
    std::fs::create_dir(&there).unwrap();
    let target = there.join("notes.md");
    std::fs::write(&target, "# Planted bulbs\n\nTulips *and* crocuses.\n").unwrap();
    // Named with no ending: typed by what it opens, never its own name.
    let link = here.join("garden notes");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    applied(
        &app,
        &Invocation::new("print.save_pdf").with("entries", named(&link)),
    );
    let pdf = here.join("garden notes.pdf");
    assert!(pdf.is_file(), "beside the link");
    assert!(!there.join("notes.pdf").exists(), "not beside its target");
    if let Some(text) = text(&pdf) {
        // Read as Markdown: the heading without its mark.
        assert!(text.contains("Planted bulbs"), "{text}");
        assert!(!text.contains("# Planted"), "{text}");
    }
}

/// Dispatch `invocation`, a Save as PDF, and apply its plan.
fn applied(app: &Arc<App>, invocation: &Invocation) {
    let Outcome::Plan(plan) = app.dispatch(invocation).unwrap() else {
        panic!("Save as PDF makes a plan");
    };
    let result = app.executor.execute_blocking(&plan, app.lifetime());
    assert!(result.error.is_none(), "{:?}", result.error);
}

/// A source file of `lines` numbered lines, several pages long.
fn source(path: &Path, lines: u32) {
    let text = (1..=lines).fold(String::new(), |mut text, n| {
        use std::fmt::Write as _;
        let _ = writeln!(text, "let line_{n} = {n}; // the {n}th");
        text
    });
    std::fs::write(path, text).unwrap();
}

/// The page shown and what is selected, as the preview hands them, pick
/// what is saved; asked without them, the command says so.
#[test]
fn the_page_shown_or_what_is_selected_is_saved_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let Some(app) = app(tmp.path()) else { return };
    let dir = tmp.path().join("files");
    std::fs::create_dir(&dir).unwrap();
    let code = dir.join("main.rs");
    source(&code, 300);
    let pdf = dir.join("main.pdf");
    let current = Invocation::new("print.save_pdf")
        .with("entries", named(&code))
        .with("page", 2)
        .with("pages", "current");
    applied(&app, &current);
    assert_eq!(pages(&pdf), 1);
    if let Some(text) = text(&pdf) {
        assert!(!text.contains("let line_1 = 1;"), "{text}");
    }
    std::fs::remove_file(&pdf).unwrap();
    let selected = Invocation::new("print.save_pdf")
        .with("entries", named(&code))
        .with("selection", "let picked = 7;\nlet also = 8;")
        .with("pages", "selection");
    applied(&app, &selected);
    assert_eq!(pages(&pdf), 1);
    if let Some(text) = text(&pdf) {
        assert!(text.contains("let also = 8;"), "{text}");
        assert!(!text.contains("let line_1 = 1;"), "{text}");
    }
    let unshown = Invocation::new("print.save_pdf")
        .with("entries", named(&code))
        .with("pages", "current");
    let refused = app.dispatch(&unshown).unwrap_err();
    assert!(refused.to_string().contains("No page"), "{refused:#}");
}

/// A contact sheet of the pictures among files is saved beside the first,
/// what is no picture left out; a folder's listing in the folder.
#[test]
fn a_contact_sheet_and_a_listing_are_saved_as_pdfs() {
    let tmp = tempfile::tempdir().unwrap();
    let Some(app) = app(tmp.path()) else { return };
    let dir = tmp.path().join("letters 'n' photos");
    std::fs::create_dir_all(dir.join("drafts")).unwrap();
    let (wide, tall, note) = (
        dir.join("wïde.png"),
        dir.join("tall.png"),
        dir.join("a note.txt"),
    );
    std::fs::write(&wide, common::synthetic_png(64, 48)).unwrap();
    std::fs::write(&tall, common::synthetic_png(30, 90)).unwrap();
    std::fs::write(&note, "not a picture").unwrap();
    let entries: Vec<EntryArg> = [&wide, &note, &tall]
        .iter()
        .map(|path| EntryArg::from(EntryRef::from_path(path).unwrap()))
        .collect();
    applied(
        &app,
        &Invocation::new("print.save_pdf")
            .with("entries", entries)
            .with("layout", "contact_sheet"),
    );
    assert_eq!(pages(&dir.join("Contact sheet.pdf")), 1);

    app.dispatch(&Invocation::with_args(
        "navigate",
        &serde_json::json!({ "location": dir.to_string_lossy() }),
    ))
    .unwrap();
    let pane = app.workspace.read().active_id();
    assert!(app.pump_until_complete(pane, Duration::from_secs(10)));
    applied(
        &app,
        &Invocation::new("print.save_pdf")
            .with("layout", "listing")
            .with("pane", pane.0),
    );
    let listing = dir.join("letters 'n' photos (listing).pdf");
    assert_eq!(pages(&listing), 1);
    if let Some(text) = text(&listing) {
        assert!(text.contains("a note.txt"), "{text}");
        assert!(text.contains("drafts/"), "{text}");
    }
}

/// The next form the plugin asks, within 30 seconds.
fn form_asked(app: &App) -> Asked {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        app.pump();
        if let Some(asked) = app.take_forms().pop() {
            return asked;
        }
        assert!(Instant::now() < deadline, "no dialog was asked");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Answer `asked` with `values` over what its fields hold.
fn answer(app: &App, asked: &Asked, values: &[(&str, &str)]) {
    let mut filled = Values::default();
    for field in &asked.form.fields {
        filled.set(field.key.clone(), field.value.clone());
    }
    for (key, value) in values {
        filled.set(*key, *value);
    }
    assert!(app.answer_form(asked.id, Some(Answer::Accepted(filled))));
}

/// Print… asks Skwir's dialog first, offering what the preview shows;
/// pages typed that are none are asked again; a PDF chosen is saved beside
/// the file as the user's own action is; the next dialog opens on it.
#[test]
fn print_asks_skwir_s_dialog_and_saves_the_pdf_it_chose() {
    let tmp = tempfile::tempdir().unwrap();
    let Some(app) = app(tmp.path()) else { return };
    app.set_window_shown();
    let dir = tmp.path().join("files");
    std::fs::create_dir(&dir).unwrap();
    let code = dir.join("main.rs");
    source(&code, 300);
    let print = Invocation::new("print.print")
        .with("entries", named(&code))
        .with("page", 3)
        .with("count", 7);
    app.dispatch(&print).unwrap();
    let asked = form_asked(&app);
    assert_eq!(asked.form.title, "Print “main.rs”");
    assert_eq!(asked.form.intro, "7 pages");
    let keys: Vec<&str> = asked.form.fields.iter().map(|f| f.key.as_str()).collect();
    assert_eq!(
        keys,
        [
            "to", "save_as", "pages", "ranges", "layout", "per_side", "fold", "trim", "paper",
            "smaller"
        ]
    );
    // A PDF goes beside the file unless another folder is chosen, named
    // after it: said, in one line.
    let save_as = field(&asked, "save_as");
    assert_eq!(save_as.value, in_folder(&dir));
    assert_eq!(offered(save_as)[0].detail, "Beside it");
    assert_eq!(held(&asked, "save_as"), "main.pdf");
    let pages_offered: Vec<&str> = field(&asked, "pages")
        .choices()
        .iter()
        .map(|c| c.value.as_str())
        .collect();
    assert_eq!(pages_offered, ["all", "current", "ranges"]);
    answer(
        &app,
        &asked,
        &[("to", "pdf"), ("pages", "ranges"), ("ranges", "5-2")],
    );
    let again = form_asked(&app);
    assert!(
        again
            .form
            .error
            .as_deref()
            .is_some_and(|error| error.starts_with("Pages to print")),
        "{:?}",
        again.form.error
    );
    assert_eq!(field(&again, "ranges").value, "5-2", "kept as typed");
    answer(
        &app,
        &again,
        &[("to", "pdf"), ("pages", "ranges"), ("ranges", "2")],
    );
    let pdf = dir.join("main.pdf");
    let deadline = Instant::now() + Duration::from_secs(60);
    while !pdf.is_file() || app.executor.is_busy() {
        assert!(Instant::now() < deadline, "no PDF was saved");
        app.pump();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(pages(&pdf), 1);
    // Undone as the user's action is.
    let undone = app.executor.undo_blocking(app.lifetime()).expect("an undo");
    assert!(undone.error.is_none(), "{:?}", undone.error);
    assert!(!pdf.exists());
    // The next dialog opens on a PDF; closed, nothing is done.
    app.dispatch(&print).unwrap();
    let next = form_asked(&app);
    assert_eq!(field(&next, "to").value, "pdf");
    assert_eq!(
        field(&next, "pages").value,
        "all",
        "pages are a document's own"
    );
    assert!(app.answer_form(next.id, None));
}

/// The field of `asked` named `key`.
fn field<'a>(asked: &'a Asked, key: &str) -> &'a skwir_core::form::Field {
    asked
        .form
        .fields
        .iter()
        .find(|field| field.key == key)
        .unwrap_or_else(|| panic!("a field {key}"))
}

/// Wait until `path` is made and the executor rests, within a minute.
fn made(app: &App, path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while !path.is_file() || app.executor.is_busy() {
        assert!(Instant::now() < deadline, "{} was not made", path.display());
        app.pump();
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A search's listing is printed as any place's: Print Listing… asks the
/// dialog, which says its PDF goes in the folder searched and takes
/// another typed; its files are said with the folder each is in.
#[test]
fn a_search_s_listing_is_saved_where_the_dialog_says() {
    let tmp = tempfile::tempdir().unwrap();
    let Some(app) = with(
        tmp.path(),
        vec![Arc::new(skwir_plugin_query::QueryPlugin::default())],
    ) else {
        return;
    };
    app.set_window_shown();
    let root = tmp.path().join("l'été notes");
    let (one, two) = (root.join("one"), root.join("two words"));
    std::fs::create_dir_all(&one).unwrap();
    std::fs::create_dir_all(&two).unwrap();
    std::fs::write(one.join("a report.txt"), "hello").unwrap();
    std::fs::write(two.join("ünïcode report.md"), "# Title\n").unwrap();
    std::fs::write(two.join("other.txt"), "not found").unwrap();
    let pane = app.workspace.read().active_id();
    app.dispatch(&Invocation::with_args(
        "query.search",
        &serde_json::json!({ "expr": "report", "root": root, "pane": pane }),
    ))
    .unwrap();
    assert!(app.pump_until_complete(pane, Duration::from_secs(10)));
    app.dispatch(
        &Invocation::new("print.print")
            .with("layout", "listing")
            .with("pane", pane.0),
    )
    .unwrap();
    let asked = form_asked(&app);
    assert!(
        asked.form.title.starts_with("Print the listing of “report"),
        "{}",
        asked.form.title
    );
    let save_as = field(&asked, "save_as");
    assert_eq!(save_as.value, in_folder(&root));
    assert_eq!(offered(save_as)[0].detail, "The folder it is of");
    assert_eq!(
        held(&asked, "save_as"),
        "report in l'été notes (listing).pdf"
    );
    // A folder typed that is none is asked again; one that is, taken.
    answer(
        &app,
        &asked,
        &[("to", "pdf"), ("save_as", "/nowhere/at all/x.pdf")],
    );
    let again = form_asked(&app);
    assert!(
        again
            .form
            .error
            .as_deref()
            .is_some_and(|error| error.starts_with("Save as")),
        "{:?}",
        again.form.error
    );
    let elsewhere = tmp.path().join("kept here");
    std::fs::create_dir(&elsewhere).unwrap();
    answer(
        &app,
        &again,
        &[
            ("to", "pdf"),
            (
                "save_as",
                &elsewhere.join("rapports d'été").to_string_lossy(),
            ),
        ],
    );
    let saved = std::fs::read_dir(&elsewhere)
        .ok()
        .and_then(|mut read| read.next());
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut saved = saved;
    while saved.is_none() || app.executor.is_busy() {
        assert!(Instant::now() < deadline, "no listing was saved");
        app.pump();
        std::thread::sleep(Duration::from_millis(20));
        saved = std::fs::read_dir(&elsewhere).unwrap().next();
    }
    let pdf = saved.unwrap().unwrap().path();
    assert_eq!(pdf, elsewhere.join("rapports d'été.pdf"), "named as typed");
    assert_eq!(pages(&pdf), 1);
    if let Some(text) = text(&pdf) {
        assert!(text.contains("Folder"), "{text}");
        assert!(text.contains("a report.txt"), "{text}");
        assert!(!text.contains("other.txt"), "{text}");
    }
    // The next dialog opens on the folder chosen.
    app.dispatch(
        &Invocation::new("print.print")
            .with("layout", "listing")
            .with("pane", pane.0),
    )
    .unwrap();
    let next = form_asked(&app);
    assert_eq!(field(&next, "save_as").value, in_folder(&elsewhere));
    // A name that is a folder there is that folder, the PDF named by
    // itself.
    let inside = elsewhere.join("ïnside");
    std::fs::create_dir(&inside).unwrap();
    answer(
        &app,
        &next,
        &[("to", "pdf"), ("save_as", &inside.to_string_lossy())],
    );
    made(&app, &inside.join("report in l'été notes (listing).pdf"));
    // A folder chosen before that is gone is neither opened on nor
    // offered; one still there is.
    std::fs::remove_dir_all(&inside).unwrap();
    app.dispatch(
        &Invocation::new("print.print")
            .with("layout", "listing")
            .with("pane", pane.0),
    )
    .unwrap();
    let after = form_asked(&app);
    let save_as = field(&after, "save_as");
    assert_eq!(save_as.value, in_folder(&root), "where it goes by itself");
    let offered: Vec<&str> = offered(save_as)
        .iter()
        .map(|choice| choice.value.as_str())
        .collect();
    assert!(
        !offered.contains(&skwir_core::os_path::from_home(&inside).as_str()),
        "{offered:?}"
    );
    assert!(
        offered.contains(&skwir_core::os_path::from_home(&elsewhere).as_str()),
        "{offered:?}"
    );
    assert!(app.answer_form(after.id, None));
}

/// `folder` as Save as holds it alone: from `~`, ending with a separator.
fn in_folder(folder: &Path) -> String {
    let mut written = skwir_core::os_path::from_home(folder);
    written.push(std::path::MAIN_SEPARATOR);
    written
}

/// The folders a Save as field offers.
fn offered(field: &skwir_core::form::Field) -> &[skwir_core::form::Choice] {
    match &field.kind {
        skwir_core::form::Kind::Save { folders, .. } => folders,
        _ => panic!("{} is no file to write", field.key),
    }
}

/// The name the Save as field `key` of `asked` holds by itself, as its
/// fields stand.
fn held(asked: &Asked, key: &str) -> String {
    let mut values = Values::default();
    for field in &asked.form.fields {
        values.set(field.key.clone(), field.value.clone());
    }
    field(asked, key).save_name(&values).to_owned()
}

/// The trash's listing, filtered, is saved where a script says: its
/// files with where each was put there from and when.
#[test]
fn the_trash_s_listing_is_saved_where_a_script_says() {
    // A trash of the user's files' volume, never a tmpfs's.
    let tmp = common::home_tempdir();
    let Some(app) = with(
        tmp.path(),
        vec![
            Arc::new(skwir_plugin_ops_basic::OpsBasicPlugin::default()),
            Arc::new(skwir_plugin_trash::TrashPlugin::default()),
        ],
    ) else {
        return;
    };
    let root = tmp.path().join("it's a folder");
    std::fs::create_dir_all(&root).unwrap();
    // A name of its own: the tests share one trash.
    let mark = tmp
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let name = format!("{mark} ünïcode.txt");
    std::fs::write(root.join(&name), "hello").unwrap();
    let Outcome::Plan(plan) = app
        .dispatch(&Invocation::new("trash").with("entries", named(&root.join(&name))))
        .unwrap()
    else {
        panic!("a plan that trashes it");
    };
    let trashed = app.executor.execute_blocking(&plan, app.lifetime());
    assert!(trashed.error.is_none(), "{:?}", trashed.error);
    let pane = app.workspace.read().active_id();
    app.navigate(pane, &skwir_core::location::Location::parse("trash:"))
        .unwrap();
    app.dispatch(&Invocation::with_args(
        "filter",
        &serde_json::json!({ "text": mark, "pane": pane }),
    ))
    .unwrap();
    // Every volume's trash streams in: what is ours is waited for.
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        app.pump();
        let shown = {
            let ws = app.workspace.read();
            let p = ws.pane(pane).unwrap();
            p.order()
                .iter()
                .filter_map(|id| p.entry(*id))
                .any(|entry| entry.name_str() == name.as_str())
        };
        if shown {
            break;
        }
        assert!(Instant::now() < deadline, "the trash lists {name}");
        std::thread::sleep(Duration::from_millis(20));
    }
    let out = tmp.path().join("listings");
    std::fs::create_dir(&out).unwrap();
    let Outcome::Plan(plan) = app
        .dispatch(
            &Invocation::new("print.save_pdf")
                .with("layout", "listing")
                .with("pane", pane.0)
                .with("dest", out.to_string_lossy()),
        )
        .unwrap()
    else {
        panic!("Save as PDF makes a plan");
    };
    let listing = out.join(format!("Trash, filtered by {mark} (listing).pdf"));
    let result = app.executor.execute_blocking(&plan, app.lifetime());
    assert!(result.error.is_none(), "{:?}", result.error);
    made(&app, &listing);
    assert_eq!(pages(&listing), 1);
    if let Some(text) = text(&listing) {
        assert!(text.contains("Deleted"), "{text}");
        assert!(text.contains(&mark), "{text}");
    }
    // A script names the PDF.
    let Outcome::Plan(plan) = app
        .dispatch(
            &Invocation::new("print.save_pdf")
                .with("layout", "listing")
                .with("pane", pane.0)
                .with("dest", out.to_string_lossy())
                .with("name", "deleted"),
        )
        .unwrap()
    else {
        panic!("Save as PDF makes a plan");
    };
    let result = app.executor.execute_blocking(&plan, app.lifetime());
    assert!(result.error.is_none(), "{:?}", result.error);
    made(&app, &out.join("deleted.pdf"));
    // A script's folder is a whole path.
    let relative = app.dispatch(
        &Invocation::new("print.save_pdf")
            .with("layout", "listing")
            .with("pane", pane.0)
            .with("dest", "listings"),
    );
    assert!(relative.is_err());
}

/// A folder chosen for a PDF is offered again the next time Skwir runs,
/// under "Chosen before", kept in the cache.
#[test]
fn the_folders_chosen_are_offered_after_a_restart() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("files");
    let out = tmp.path().join("l'été out");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::create_dir_all(&out).unwrap();
    let code = dir.join("main.rs");
    source(&code, 20);
    let print = Invocation::new("print.print").with("entries", named(&code));
    {
        let Some(app) = app(tmp.path()) else { return };
        app.set_window_shown();
        app.dispatch(&print).unwrap();
        let asked = form_asked(&app);
        answer(
            &app,
            &asked,
            &[("to", "pdf"), ("save_as", &out.to_string_lossy())],
        );
        made(&app, &out.join("main.pdf"));
    }
    // Another run, an app of its own in this process: what the first held
    // in memory is not its (the dialog's choices are the plugin's, one for
    // each app), what the cache keeps is.
    let Some(app) = app(tmp.path()) else { return };
    app.set_window_shown();
    app.dispatch(&print).unwrap();
    let asked = form_asked(&app);
    let save_as = field(&asked, "save_as");
    assert_eq!(save_as.value, in_folder(&dir), "where it goes by itself");
    let before: Vec<(&str, &str)> = offered(save_as)
        .iter()
        .map(|choice| (choice.value.as_str(), choice.section.as_str()))
        .collect();
    let written = skwir_core::os_path::from_home(&out);
    assert!(
        before.contains(&(written.as_str(), "Chosen before")),
        "{before:?}"
    );
    assert!(app.answer_form(asked.id, None));
}

/// A Save as PDF step rebuilt from its data (a plan kept, then read back)
/// reaches the app it is rebuilt in, as one made there does; the plugin
/// turned off, it is refused.
#[test]
fn a_step_rebuilt_from_its_data_reaches_its_app() {
    let tmp = tempfile::tempdir().unwrap();
    let Some(app) = app(tmp.path()) else { return };
    let dir = tmp.path().join("files");
    std::fs::create_dir(&dir).unwrap();
    let code = dir.join("l'été.rs");
    source(&code, 10);
    let Outcome::Plan(plan) = app
        .dispatch(&Invocation::new("print.save_pdf").with("entries", named(&code)))
        .unwrap()
    else {
        panic!("Save as PDF makes a plan");
    };
    let data = plan.to_data();
    let rebuilt = skwir_core::op::Plan::from_data(&data, &app.ops).unwrap();
    let result = app.executor.execute_blocking(&rebuilt, app.lifetime());
    assert!(result.error.is_none(), "{:?}", result.error);
    made(&app, &dir.join("l'été.pdf"));

    // Turned off, the step its data rebuilds before is refused.
    let again = skwir_core::op::Plan::from_data(&data, &app.ops).unwrap();
    std::fs::remove_file(dir.join("l'été.pdf")).unwrap();
    assert!(app.plugins.set_enabled("print", false, &app));
    let refused = app.executor.execute_blocking(&again, app.lifetime());
    assert!(
        refused
            .error
            .as_deref()
            .is_some_and(|error| error.contains("print plugin on")),
        "{:?}",
        refused.error
    );
    assert!(!dir.join("l'été.pdf").exists());
}
