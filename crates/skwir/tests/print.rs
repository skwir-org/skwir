//! Printing (ADR-111) across plugins: Save as PDF's plan, applied, makes a
//! PDF of what a file's preview shows (a Markdown document's reading view,
//! a source file's code, a picture, a drawing), the pages chosen kept and
//! no other, beside the file, and its undo takes it to the trash; a
//! booklet, a contact sheet, a folder's listing. Print… asks Skwir's own
//! dialog first, answered here as the window would. The print helper is a
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
        Arc::new(skwir_plugin_print::PrintPlugin),
    ] {
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
    assert_eq!(keys, ["to", "pages", "ranges", "layout", "fold", "trim"]);
    let pages_offered: Vec<&str> = asked.form.fields[1]
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
    assert_eq!(again.form.fields[2].value, "5-2", "kept as typed");
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
    assert_eq!(next.form.fields[0].value, "pdf");
    assert_eq!(
        next.form.fields[1].value, "all",
        "pages are a document's own"
    );
    assert!(app.answer_form(next.id, None));
}
