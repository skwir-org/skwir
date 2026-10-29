//! `DjVu` documents (ADR-061): shown by the reader through the preview
//! registry, with the engine the setting chooses, Automatic giving way to
//! djvu-rs where `DjVuLibre`'s helper is absent; their first page is their
//! thumbnail.
use skwir_core::{
    App, CancelToken,
    app::AppConfig,
    op::EntryRef,
    preview::PreviewRequest,
    scheduler::Priority,
    thumbs::{ThumbRequest, ThumbSize, ThumbState},
};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use djvu_rs::djvu_document::DjVuBookmark;
use djvu_rs::djvu_mut::DjVuDocumentMut;
use djvu_rs::text::{self, TextLayer, TextZone, TextZoneKind};
use djvu_rs::{Bitmap, jb2_encode};

/// The djvu-rs helper as built beside this test.
fn djvu_worker() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("SKWIR_DJVU_WORKER") {
        return Some(path.into());
    }
    let exe = std::env::current_exe().ok()?;
    let built = exe.parent()?.parent()?.join("skwir-djvu-worker");
    built.is_file().then_some(built)
}

/// The djvu-rs helper found, and `DjVuLibre`'s said to be nowhere, so the
/// machine's own is not what answers. A default member, the helper is
/// built with the tests; without it the test says so and passes, unless
/// `SKWIR_REQUIRE_WORKERS` is set.
fn with_djvu_rs_alone(root: &Path) -> bool {
    let Some(worker) = djvu_worker() else {
        assert!(
            std::env::var_os("SKWIR_REQUIRE_WORKERS").is_none(),
            "skwir-djvu-worker is not built"
        );
        eprintln!("skipped: skwir-djvu-worker is not built");
        return false;
    };
    // Tests in this binary run in processes of their own under nextest:
    // the variables cannot leak into another test.
    // SAFETY: no other thread of this test process reads the environment.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("SKWIR_DJVU_WORKER", &worker);
        std::env::set_var("SKWIR_DJVULIBRE_WORKER", root.join("no-such-helper"));
    }
    true
}

/// Two pages of 850 × 1100 pixels at 100 dpi: words on the first, said by
/// its text; an outline leading to the second.
fn document(path: &Path) {
    let mut first = Bitmap::new(850, 1100);
    for y in 150..190 {
        for x in 100..400 {
            first.set(x, y, true);
        }
    }
    let bundle = jb2_encode::encode_djvm_bundle_jb2(&[first, Bitmap::new(850, 1100)], 0, 100);
    let mut edited = DjVuDocumentMut::from_bytes(&bundle).unwrap();
    let word = |text: &str, x: u32, width: u32| TextZone {
        kind: TextZoneKind::Word,
        rect: text::Rect {
            x,
            y: 150,
            width,
            height: 40,
        },
        text: text.into(),
        children: Vec::new(),
    };
    let layer = TextLayer {
        text: "Synthetic words".into(),
        zones: vec![TextZone {
            kind: TextZoneKind::Line,
            rect: text::Rect {
                x: 100,
                y: 150,
                width: 300,
                height: 40,
            },
            text: "Synthetic words".into(),
            children: vec![word("Synthetic", 100, 180), word("words", 290, 110)],
        }],
    };
    edited.page_mut(0).unwrap().set_text_layer(&layer).unwrap();
    edited
        .set_bookmarks(&[DjVuBookmark {
            title: "Second".into(),
            url: "#2".into(),
            children: Vec::new(),
        }])
        .unwrap();
    std::fs::write(path, edited.into_bytes()).unwrap();
}

fn app(root: &Path) -> Arc<App> {
    let app = App::new(AppConfig::ephemeral(root.join("state"))).unwrap();
    app.plugins
        .add(Arc::new(skwir_plugin_djvu::PreviewPlugin), &app);
    app
}

fn request(path: &Path) -> PreviewRequest {
    let mime = skwir_platform::mime::identify(path.file_name().unwrap(), path);
    PreviewRequest {
        entry: EntryRef::from_path(path).unwrap(),
        mime: mime.map(|mime| mime.0),
        page: 1,
        seconds: None,
        max_side: 512,
        text_size: None,
        fit: None,
        held: skwir_core::preview::HeldSlot::default(),
    }
}

#[test]
fn a_djvu_document_is_read_by_djvu_rs_where_djvulibre_is_absent() {
    let tmp = tempfile::tempdir().unwrap();
    if !with_djvu_rs_alone(tmp.path()) {
        return;
    }
    let app = app(tmp.path());
    let path = tmp.path().join("Synthetic scan, “vol. 2” #1.djvu");
    document(&path);
    let cancel = CancelToken::new();
    let session = app
        .previews
        .open_document(&request(&path), &cancel)
        .unwrap()
        .expect("a document of pages");
    assert_eq!(session.info().pages, 2);
    assert_eq!(session.info().outlines[0].page, Some(2));
    let page = session.render(1, 400, &cancel).unwrap();
    assert!(page.height <= 400 && page.height >= 370, "{}", page.height);
    // 8.5 × 11 in, whatever the side drawn.
    assert!((page.content.width - 816.).abs() < 1.);
    let text: String = page.content.chars.iter().map(|c| c.text.as_str()).collect();
    assert_eq!(text, "Synthetic words");
    let hits = session.search("WORDS", &cancel).unwrap();
    assert_eq!(hits.iter().map(|hit| hit.page).collect::<Vec<_>>(), [1]);

    // Its first page is its thumbnail, drawn by the same helper.
    let req = ThumbRequest {
        entry: EntryRef::from_path(&path).unwrap(),
        mime: None,
        mtime: None,
        size: ThumbSize::Normal,
    };
    let deadline = Instant::now() + Duration::from_secs(20);
    let thumbnail = loop {
        match app.thumbnails.get(req.clone(), Priority::Visible) {
            ThumbState::Ready(image) => break image,
            ThumbState::Unavailable => panic!("no thumbnail"),
            ThumbState::Pending => {
                assert!(Instant::now() < deadline, "timed out");
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    };
    assert!(
        thumbnail.height <= 128 && thumbnail.height > 110,
        "{}",
        thumbnail.height
    );
    assert!(thumbnail.width < thumbnail.height);
}

#[test]
fn an_engine_chosen_and_missing_says_what_to_install() {
    let tmp = tempfile::tempdir().unwrap();
    if !with_djvu_rs_alone(tmp.path()) {
        return;
    }
    let app = app(tmp.path());
    let path = tmp.path().join("Synthetic.djvu");
    document(&path);
    let key = "djvu.engine";
    app.settings
        .set(key, toml::Value::String("djvulibre".into()));
    app.plugins.settings_changed(&[key.into()], &app);
    let error = app
        .previews
        .open_document(&request(&path), &CancelToken::new())
        .err()
        .expect("DjVuLibre's helper is missing");
    assert_eq!(
        error.to_string(),
        "Install skwir-djvulibre-worker to read DjVu documents with DjVuLibre"
    );
    // Chosen again, djvu-rs reads it.
    app.settings.set(key, toml::Value::String("djvu-rs".into()));
    app.plugins.settings_changed(&[key.into()], &app);
    let session = app
        .previews
        .open_document(&request(&path), &CancelToken::new())
        .unwrap()
        .expect("read by djvu-rs");
    assert_eq!(session.info().pages, 2);
}
