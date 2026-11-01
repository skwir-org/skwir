//! Smaller copies (ADR-112) across plugins: `shrink.reduce`'s plan,
//! applied, makes a smaller copy beside each picture and PDF, named
//! `name (reduced).ext`, and its undo takes it to the trash; a PDF's
//! photograph is brought to the dots per inch asked, its text untouched;
//! a copy that would not be smaller is not made and the plan goes on; a
//! signed PDF is not made smaller; Reduce Size… asks its dialog first.
//! The print helper, which makes the copies, is a default member built
//! with the tests; without it the test says so and passes, unless
//! `SKWIR_REQUIRE_WORKERS` is set.
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use lopdf::{Dictionary, Document, Object, Stream, dictionary};
use skwir_core::app::{App, AppConfig};
use skwir_core::command::{Invocation, Outcome};
use skwir_core::form::{Answer, Asked, Values};
use skwir_core::op::{EntryArg, EntryRef};
use skwir_core::plugin::Plugin;

/// A helper built beside the tests (a default member, built with them).
fn built(name: &str) -> Option<PathBuf> {
    let name = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.parent()?.join(&name)))
        .filter(|path| path.is_file())
}

fn app(root: &Path) -> Option<Arc<App>> {
    let Some(helper) = built("skwir-print-worker") else {
        assert!(
            std::env::var_os("SKWIR_REQUIRE_WORKERS").is_none(),
            "skwir-print-worker is not built"
        );
        eprintln!("skipped: skwir-print-worker is not built");
        return None;
    };
    // Tests in this binary run in processes of their own under nextest:
    // the variable cannot leak into another test.
    // SAFETY: no other thread of this test process reads the environment.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("SKWIR_PRINT_WORKER", &helper);
        // A PDF's copy is checked with PDFium's reader where it is built.
        if let Some(pdf) = built("skwir-pdf-worker") {
            std::env::set_var("SKWIR_PDF_WORKER", pdf);
        }
    }
    let app = App::new(AppConfig::ephemeral(root.join("state"))).unwrap();
    for plugin in [
        Arc::new(skwir_plugin_documents::PreviewPlugin::default()) as Arc<dyn Plugin>,
        Arc::new(skwir_plugin_shrink::ShrinkPlugin),
    ] {
        app.plugins.add(plugin, &app);
    }
    Some(app)
}

fn named(path: &Path) -> [EntryArg; 1] {
    [EntryArg::from(EntryRef::from_path(path).unwrap())]
}

/// `shrink.reduce` of `path` at `quality`, the plan applied: what it did.
fn reduce(app: &Arc<App>, path: &Path, quality: &str) -> skwir_core::op::ExecResult {
    let invocation = Invocation::new("shrink.reduce")
        .with("entries", named(path))
        .with("quality", quality);
    let Outcome::Plan(plan) = app.dispatch(&invocation).unwrap() else {
        panic!("Reduce Size makes a plan");
    };
    app.executor.execute_blocking(&plan, app.lifetime())
}

/// A photograph's pixels: smooth gradients and grain, as a camera's are.
fn photo(width: u32, height: u32) -> image::RgbImage {
    let mut seed = 0x2545_f491_u32;
    image::RgbImage::from_fn(width, height, |x, y| {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let grain = u8::try_from(seed % 24).unwrap();
        let shade = |n: u32, of: u32| u8::try_from(n * 200 / of.max(1)).unwrap();
        image::Rgb([
            shade(x, width).saturating_add(grain),
            shade(y, height).saturating_add(grain),
            shade(x + y, width + height).saturating_add(grain),
        ])
    })
}

fn jpeg(picture: &image::RgbImage, quality: u8) -> Vec<u8> {
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality)
        .encode_image(picture)
        .unwrap();
    out
}

/// A one-page PDF drawing a photograph of `width` × `height` pixels 6 by
/// 4 inches, and a line of text.
fn pdf_with_photo(path: &Path, width: u32, height: u32, signed: bool) {
    let mut doc = Document::with_version("1.5");
    let tree_id = doc.new_object_id();
    let mut image = Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => i64::from(width),
            "Height" => i64::from(height),
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8,
            "Filter" => "DCTDecode",
        },
        jpeg(&photo(width, height), 95),
    );
    image.allows_compression = false;
    let image_id = doc.add_object(image);
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let content = b"q 432 0 0 288 90 400 cm /Im0 Do Q BT /F1 12 Tf 72 72 Td (Smaller but the same words) Tj ET".to_vec();
    let content_id = doc.add_object(Stream::new(Dictionary::new(), content));
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => tree_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Contents" => content_id,
        "Resources" => dictionary! {
            "XObject" => dictionary! { "Im0" => image_id },
            "Font" => dictionary! { "F1" => font_id },
        },
    });
    doc.objects.insert(
        tree_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page_id.into()],
            "Count" => 1,
        }),
    );
    let mut catalog = dictionary! { "Type" => "Catalog", "Pages" => tree_id };
    if signed {
        catalog.set(
            "AcroForm",
            dictionary! { "Fields" => Vec::<Object>::new(), "SigFlags" => 3 },
        );
    }
    let catalog_id = doc.add_object(catalog);
    doc.trailer.set("Root", catalog_id);
    doc.save(path).unwrap();
}

/// The width of each image the PDF at `path` holds, and its pages' text
/// as their drawing writes it.
fn read_back(path: &Path) -> (Vec<i64>, String) {
    let doc = Document::load(path).unwrap();
    let widths = doc
        .objects
        .values()
        .filter_map(|object| match object {
            Object::Stream(stream)
                if stream.dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Image") =>
            {
                stream.dict.get(b"Width").and_then(Object::as_i64).ok()
            }
            _ => None,
        })
        .collect();
    let page = *doc.get_pages().values().next().unwrap();
    let text = String::from_utf8_lossy(&doc.get_page_content(page)).into_owned();
    (widths, text)
}

#[test]
fn a_photograph_s_copy_is_smaller_beside_it_and_undone_to_the_trash() {
    let tmp = tempfile::tempdir().unwrap();
    let Some(app) = app(tmp.path()) else { return };
    let dir = tmp.path().join("files");
    std::fs::create_dir(&dir).unwrap();
    let picture = dir.join("IMG 0001 l'été.jpg");
    std::fs::write(&picture, jpeg(&photo(3000, 2000), 95)).unwrap();
    let result = reduce(&app, &picture, "screen");
    assert!(result.error.is_none(), "{:?}", result.error);
    let copy = dir.join("IMG 0001 l'été (reduced).jpg");
    assert!(copy.is_file(), "the copy is beside it");
    let shrunk = image::open(&copy).unwrap();
    assert_eq!((shrunk.width(), shrunk.height()), (1920, 1280));
    assert!(std::fs::metadata(&copy).unwrap().len() < std::fs::metadata(&picture).unwrap().len());
    let undone = app.executor.undo_blocking(app.lifetime()).expect("an undo");
    assert!(undone.error.is_none(), "{:?}", undone.error);
    assert!(!copy.exists(), "taken to the trash");
    assert!(picture.is_file(), "the picture stays");
}

#[test]
fn a_pdf_s_photograph_is_brought_to_the_dpi_asked_its_text_untouched() {
    let tmp = tempfile::tempdir().unwrap();
    let Some(app) = app(tmp.path()) else { return };
    let dir = tmp.path().join("files");
    std::fs::create_dir(&dir).unwrap();
    let pdf = dir.join("report.pdf");
    // 3000 pixels over 6 inches: 500 dpi.
    pdf_with_photo(&pdf, 3000, 2000, false);
    let result = reduce(&app, &pdf, "screen");
    assert!(result.error.is_none(), "{:?}", result.error);
    let copy = dir.join("report (reduced).pdf");
    assert!(copy.is_file());
    assert!(std::fs::metadata(&copy).unwrap().len() < std::fs::metadata(&pdf).unwrap().len());
    let (widths, text) = read_back(&copy);
    // 150 dpi over 6 inches.
    assert_eq!(widths, [900]);
    assert!(text.contains("(Smaller but the same words) Tj"), "{text}");
}

#[test]
fn a_copy_that_would_not_be_smaller_is_not_made_and_the_plan_goes_on() {
    let tmp = tempfile::tempdir().unwrap();
    let Some(app) = app(tmp.path()) else { return };
    let dir = tmp.path().join("files");
    std::fs::create_dir(&dir).unwrap();
    let signed = dir.join("signed.pdf");
    pdf_with_photo(&signed, 3000, 2000, true);
    let large = dir.join("large.jpg");
    std::fs::write(&large, jpeg(&photo(2600, 1800), 95)).unwrap();
    let invocation = Invocation::new("shrink.reduce")
        .with(
            "entries",
            [&signed, &large]
                .into_iter()
                .map(|path| EntryArg::from(EntryRef::from_path(path).unwrap()))
                .collect::<Vec<_>>(),
        )
        .with("quality", "small");
    let Outcome::Plan(plan) = app.dispatch(&invocation).unwrap() else {
        panic!("a plan");
    };
    let result = app.executor.execute_blocking(&plan, app.lifetime());
    assert!(result.error.is_none(), "{:?}", result.error);
    assert_eq!((result.applied, result.skipped), (1, 1));
    assert!(
        !dir.join("signed (reduced).pdf").exists(),
        "a signed PDF is left"
    );
    assert!(
        dir.join("large (reduced).jpg").is_file(),
        "the plan went on"
    );
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

/// Reduce Size… asks how small first, offering what the files are; a
/// size that is none is asked again; the copy is made as the user's
/// action, at most the size asked.
#[test]
fn reduce_size_asks_first_and_makes_the_copy_chosen() {
    let tmp = tempfile::tempdir().unwrap();
    let Some(app) = app(tmp.path()) else { return };
    app.set_window_shown();
    let dir = tmp.path().join("files");
    std::fs::create_dir(&dir).unwrap();
    let picture = dir.join("walk.jpg");
    std::fs::write(&picture, jpeg(&photo(3200, 2400), 95)).unwrap();
    let ask = Invocation::new("shrink.reduce_ask").with("entries", named(&picture));
    app.dispatch(&ask).unwrap();
    let asked = form_asked(&app);
    assert_eq!(asked.form.title, "Reduce Size of “walk.jpg”");
    let keys: Vec<&str> = asked.form.fields.iter().map(|f| f.key.as_str()).collect();
    assert_eq!(keys, ["quality", "gray", "strip", "at_most"]);
    answer(&app, &asked, &[("quality", "screen"), ("at_most", "lots")]);
    let again = form_asked(&app);
    assert!(
        again
            .form
            .error
            .as_deref()
            .is_some_and(|error| error.starts_with("At most")),
        "{:?}",
        again.form.error
    );
    answer(
        &app,
        &again,
        &[("quality", "screen"), ("at_most", "150 KB")],
    );
    let copy = dir.join("walk (reduced).jpg");
    let deadline = Instant::now() + Duration::from_secs(60);
    while !copy.is_file() || app.executor.is_busy() {
        assert!(Instant::now() < deadline, "no copy was made");
        app.pump();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(std::fs::metadata(&copy).unwrap().len() <= 150_000);
}
