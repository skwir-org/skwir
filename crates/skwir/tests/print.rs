//! Printing (ADR-111) across plugins: Save as PDF's plan, applied, makes a
//! PDF of what a file's preview shows (a Markdown document's reading view,
//! a source file's code, a picture, a drawing), the pages typed kept and
//! no other, beside the file, and its undo takes it to the trash. The
//! print helper is a default member, built with the tests; without it the
//! test says so and passes, unless `SKWIR_REQUIRE_WORKERS` is set.
use std::path::{Path, PathBuf};
use std::sync::Arc;

use skwir_core::app::{App, AppConfig};
use skwir_core::command::{Invocation, Outcome};
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
