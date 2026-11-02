//! Smaller copies (ADR-112) across plugins: `shrink.reduce`'s plan,
//! applied, makes a smaller copy beside each picture and PDF, named
//! `name (reduced).ext`, and its undo takes it to the trash; a PDF's
//! photograph is brought to the dots per inch asked, its text untouched;
//! a copy that would not be smaller is not made and the plan goes on; a
//! signed PDF is not made smaller; Reduce Size… asks its dialog first; an
//! image whose page looks different in the copy is put back as it was,
//! and a copy whose pages differ otherwise is not kept (a reader standing
//! in for the window's draws the pages).
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

/// A two-page PDF, each page drawing a photograph of 3000 × 2000 pixels
/// 6 by 4 inches (500 dpi), told apart by their grain.
fn pdf_of_two_photos(path: &Path) {
    let mut doc = Document::with_version("1.5");
    let tree_id = doc.new_object_id();
    let mut kids = Vec::new();
    for page in 0..2u32 {
        let mut picture = photo(3000, 2000);
        for pixel in picture.pixels_mut() {
            pixel[2] = pixel[2].wrapping_add(u8::try_from(page * 40).unwrap());
        }
        let mut image = Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => 3000,
                "Height" => 2000,
                "ColorSpace" => "DeviceRGB",
                "BitsPerComponent" => 8,
                "Filter" => "DCTDecode",
            },
            jpeg(&picture, 95),
        );
        image.allows_compression = false;
        let image_id = doc.add_object(image);
        let content = doc.add_object(Stream::new(
            Dictionary::new(),
            b"q 432 0 0 288 90 400 cm /Im0 Do Q".to_vec(),
        ));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => tree_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Contents" => content,
            "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => image_id } },
        });
        kids.push(page_id.into());
    }
    doc.objects.insert(
        tree_id,
        Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => 2 }),
    );
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => tree_id });
    doc.trailer.set("Root", catalog_id);
    doc.save(path).unwrap();
}

/// How the reader standing in for the window's draws a copy.
#[derive(Clone, Copy)]
enum Stand {
    /// The first page as dark as its image is narrow, the others alike:
    /// an image made smaller there looks different.
    FirstPageByWidth,
    /// The copy one page short.
    PageShort,
}

/// A reader of PDFs standing in for the window's, which the copy's check
/// asks: it draws each page in one gray, as `Stand` says.
struct StandIn(Stand);

impl skwir_core::preview::PreviewProvider for StandIn {
    fn id(&self) -> &'static str {
        "stand-in"
    }
    fn priority(&self) -> i32 {
        i32::MAX / 2
    }
    fn accepts(&self, request: &skwir_core::preview::PreviewRequest) -> bool {
        request.mime.as_deref() == Some("application/pdf")
    }
    fn open_document(
        &self,
        request: &skwir_core::preview::PreviewRequest,
        _: &skwir_core::CancelToken,
    ) -> anyhow::Result<Option<Arc<dyn skwir_core::document::DocumentSession>>> {
        // Each page's image's width, as the file or the copy has it.
        let widths = widths_by_page(&request.entry.path);
        let copy = request.entry.path.file_name() == Some(std::ffi::OsStr::new("copy.pdf"));
        let pages = match self.0 {
            Stand::PageShort if copy => 1,
            _ => u32::try_from(widths.len()).unwrap(),
        };
        Ok(Some(Arc::new(Drawn {
            stand: self.0,
            widths,
            info: skwir_core::document::DocumentInfo {
                pages,
                outlines: Vec::new(),
                reflowable: false,
                labels: Vec::new(),
            },
        })))
    }
}

struct Drawn {
    stand: Stand,
    widths: Vec<i64>,
    info: skwir_core::document::DocumentInfo,
}

impl skwir_core::document::DocumentSession for Drawn {
    fn info(&self) -> &skwir_core::document::DocumentInfo {
        &self.info
    }
    fn render(
        &self,
        page: u32,
        side: u32,
        _: &skwir_core::CancelToken,
    ) -> anyhow::Result<skwir_core::document::DocumentPage> {
        let level = match self.stand {
            Stand::FirstPageByWidth if page == 1 => u8::try_from(self.widths[0] / 20).unwrap(),
            _ => 128,
        };
        let (width, height) = (side * 3 / 4, side);
        Ok(skwir_core::document::DocumentPage {
            width,
            height,
            pixels: [level, level, level, 255].repeat((width * height) as usize),
            order: skwir_core::document::PixelOrder::Rgba,
            content: Arc::new(skwir_core::document::PageContent {
                page,
                width: 612.0,
                height: 792.0,
                chars: Vec::new(),
                links: Vec::new(),
            }),
        })
    }
    fn search(
        &self,
        _: &str,
        _: &skwir_core::CancelToken,
    ) -> anyhow::Result<Vec<skwir_core::document::SearchHit>> {
        Ok(Vec::new())
    }
}

/// The widths of the images the PDF at `path` holds, by page.
fn widths_by_page(path: &Path) -> Vec<i64> {
    let doc = Document::load(path).unwrap();
    doc.get_pages()
        .values()
        .map(|&page| {
            let resources = doc.get_dictionary(page).unwrap().get(b"Resources").unwrap();
            let image = resources
                .as_dict()
                .unwrap()
                .get(b"XObject")
                .unwrap()
                .as_dict()
                .unwrap()
                .get(b"Im0")
                .unwrap()
                .as_reference()
                .unwrap();
            let Ok(Object::Stream(image)) = doc.get_object(image) else {
                panic!("an image");
            };
            image.dict.get(b"Width").unwrap().as_i64().unwrap()
        })
        .collect()
}

/// The image on a page that looks different in the copy is put back as it
/// was, the copy made again keeping it and checked again; the other page's
/// image is made smaller.
#[test]
fn an_image_whose_page_looks_different_is_put_back() {
    let tmp = tempfile::tempdir().unwrap();
    let Some(app) = app(tmp.path()) else { return };
    app.previews
        .register("stand-in", Arc::new(StandIn(Stand::FirstPageByWidth)));
    let dir = tmp.path().join("files");
    std::fs::create_dir(&dir).unwrap();
    let pdf = dir.join("two photos.pdf");
    pdf_of_two_photos(&pdf);
    let result = reduce(&app, &pdf, "screen");
    assert!(result.error.is_none(), "{:?}", result.error);
    let copy = dir.join("two photos (reduced).pdf");
    assert!(copy.is_file());
    assert!(std::fs::metadata(&copy).unwrap().len() < std::fs::metadata(&pdf).unwrap().len());
    // The first page's image as it was; the second's at 150 dpi.
    assert_eq!(widths_by_page(&copy), [3000, 900]);
}

/// A copy with a page fewer than its file is not kept, and said.
#[test]
fn a_copy_whose_pages_differ_otherwise_is_not_kept() {
    let tmp = tempfile::tempdir().unwrap();
    let Some(app) = app(tmp.path()) else { return };
    app.previews
        .register("stand-in", Arc::new(StandIn(Stand::PageShort)));
    let dir = tmp.path().join("files");
    std::fs::create_dir(&dir).unwrap();
    let pdf = dir.join("two photos.pdf");
    pdf_of_two_photos(&pdf);
    let result = reduce(&app, &pdf, "screen");
    assert!(result.error.is_none(), "{:?}", result.error);
    assert_eq!(result.skipped, 1);
    assert!(!dir.join("two photos (reduced).pdf").exists());
}

/// Lossless brings nothing under a size: asked both, the command says so
/// rather than making a plan.
#[test]
fn lossless_with_a_size_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let Some(app) = app(tmp.path()) else { return };
    let picture = tmp.path().join("walk.png");
    image::RgbImage::from_pixel(64, 64, image::Rgb([200, 100, 50]))
        .save(&picture)
        .unwrap();
    let invocation = Invocation::new("shrink.reduce")
        .with("entries", named(&picture))
        .with("quality", "lossless")
        .with("at_most", "10 KB");
    let refused = app.dispatch(&invocation).expect_err("refused");
    assert!(format!("{refused:#}").contains("Lossless"), "{refused:#}");
}

/// A step of a plan made while the plugin was on is refused once it is
/// off, and makes nothing.
#[test]
fn a_plan_made_before_the_plugin_went_off_makes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let Some(app) = app(tmp.path()) else { return };
    let picture = tmp.path().join("walk.jpg");
    std::fs::write(&picture, jpeg(&photo(2600, 1800), 95)).unwrap();
    let invocation = Invocation::new("shrink.reduce")
        .with("entries", named(&picture))
        .with("quality", "screen");
    let Outcome::Plan(plan) = app.dispatch(&invocation).unwrap() else {
        panic!("a plan");
    };
    assert!(app.plugins.set_enabled("shrink", false, &app));
    let result = app.executor.execute_blocking(&plan, app.lifetime());
    assert!(result.error.is_some(), "the step is refused");
    assert!(!tmp.path().join("walk (reduced).jpg").exists());
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
