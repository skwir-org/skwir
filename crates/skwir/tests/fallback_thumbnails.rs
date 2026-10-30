//! Thumbnails skwir makes when the desktop cannot (PDF and Office pages,
//! the pictures Office documents keep, audio, book and comic covers, `DjVu`
//! pages): tried only after the desktop's cache and thumbnailers,
//! whichever plugin registers first, and shared through the desktop's
//! cache.
use skwir_core::{
    App, CancelToken,
    app::AppConfig,
    op::EntryRef,
    plugin::Plugin,
    preview::PreviewRequest,
    scheduler::Priority,
    thumbs::{Image, ThumbRequest, ThumbSize, ThumbState},
};
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

const THUMBNAILS: &str = "thumbnails";
const FALLBACK_PLUGINS: [&str; 5] = ["documents", "media", "epub", "comics", "djvu"];

/// The plugins with fallbacks first: the reverse of the binary's order.
fn app(root: &Path) -> Arc<App> {
    let app = App::new(AppConfig::ephemeral(root.join("state"))).unwrap();
    for plugin in [
        Arc::new(skwir_plugin_documents::PreviewPlugin::default()) as Arc<dyn Plugin>,
        Arc::new(skwir_plugin_media::PreviewPlugin::default()),
        Arc::new(skwir_plugin_epub::PreviewPlugin),
        Arc::new(skwir_plugin_comics::PreviewPlugin),
        Arc::new(skwir_plugin_djvu::PreviewPlugin),
        Arc::new(skwir_plugin_thumbnails::ThumbnailsPlugin::default()),
    ] {
        app.plugins.add(plugin, &app);
    }
    app
}

/// The desktop's providers first, in their order, then every fallback once,
/// then the last resort.
#[track_caller]
fn assert_order(app: &App, after: &str) {
    let ids = app.thumbnails.provider_ids();
    // The system's own (the Windows shell, Quick Look) where a Linux
    // desktop's thumbnailers are, before them (ADR-041).
    let desktop: &[&str] = if skwir_platform::system_thumbs::AVAILABLE {
        &[
            "freedesktop-cache",
            "image-decoder",
            "system",
            "thumbnailers",
        ]
    } else {
        &["freedesktop-cache", "image-decoder", "thumbnailers"]
    };
    assert_eq!(ids[..desktop.len()], *desktop, "{after}: {ids:?}");
    assert_eq!(ids.last(), Some(&"imagemagick"), "{after}: {ids:?}");
    let mut fallbacks = ids[desktop.len()..ids.len() - 1].to_vec();
    fallbacks.sort_unstable();
    assert_eq!(
        fallbacks,
        [
            "audio-covers",
            "book-covers",
            "comic-covers",
            "djvu-pages",
            "office-pages",
            "office-stored-pictures",
            "pdf-pages"
        ],
        "{after}: {ids:?}"
    );
    // The picture a document keeps costs a read; a converted page needs a
    // preview made first.
    let at = |id: &str| ids.iter().position(|i| *i == id);
    assert!(
        at("office-stored-pictures") < at("office-pages"),
        "{after}: {ids:?}"
    );
}

#[test]
fn fallbacks_come_after_the_desktop_whoever_registers_first() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    assert_order(&app, "registration");
    // Re-registered by a toggle or a settings change: same order, no copy.
    for id in FALLBACK_PLUGINS.into_iter().chain([THUMBNAILS]) {
        assert!(app.plugins.set_enabled(id, false, &app));
        assert!(app.plugins.set_enabled(id, true, &app));
        assert_order(&app, id);
    }
    for (key, value) in [
        ("documents.backend", "poppler"),
        ("epub.backend", "mupdf"),
        ("media.backend", "stills"),
        ("djvu.engine", "djvu-rs"),
        ("thumbnails.write_cache", "false"),
    ] {
        let value = if value == "false" {
            toml::Value::Boolean(false)
        } else {
            toml::Value::String(value.into())
        };
        app.settings.set(key, value);
        app.plugins.settings_changed(&[key.into()], &app);
        assert_order(&app, key);
    }
    // Turned off, ImageMagick is asked nothing; turned on, it is last again.
    let key = "thumbnails.imagemagick";
    for on in [false, true] {
        app.settings.set(key, toml::Value::Boolean(on));
        app.plugins.settings_changed(&[key.into()], &app);
        let ids = app.thumbnails.provider_ids();
        assert_eq!(ids.contains(&"imagemagick"), on, "{ids:?}");
    }
    assert_order(&app, key);
}

fn wait_ready(app: &App, req: &ThumbRequest) -> Arc<Image> {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        match app.thumbnails.get(req.clone(), Priority::Visible) {
            ThumbState::Ready(img) => return img,
            ThumbState::Unavailable => panic!("no thumbnail for the PDF"),
            ThumbState::Pending => {
                assert!(Instant::now() < deadline, "timed out");
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
}

fn request(path: &Path) -> ThumbRequest {
    ThumbRequest {
        entry: EntryRef::from_path(path).unwrap(),
        mime: skwir_platform::mime::by_name(path.file_name().unwrap()).map(|m| m.0.into()),
        mtime: Some(std::fs::metadata(path).unwrap().modified().unwrap()),
        size: ThumbSize::Normal,
    }
}

/// The colour at the middle of `image`.
fn middle(image: &Image) -> [u8; 4] {
    let at = ((image.height / 2 * image.width + image.width / 2) * 4) as usize;
    image.rgba[at..at + 4].try_into().unwrap()
}

#[test]
#[ignore = "requires LibreOffice, bubblewrap and Poppler; run explicitly for preview changes"]
fn an_office_document_has_a_thumbnail_once_previewed() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let path = skwir_core::os_path::canonical(tmp.path())
        .unwrap()
        .join("Synthetic minutes \u{2019}24.fodt");
    std::fs::write(
        &path,
        r#"<?xml version="1.0"?><office:document xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" office:version="1.2" office:mimetype="application/vnd.oasis.opendocument.text"><office:body><office:text><text:p>Synthetic minutes</text:p></office:text></office:body></office:document>"#,
    )
    .unwrap();
    let req = request(&path);
    // Never previewed: no LibreOffice for a tile.
    let deadline = Instant::now() + Duration::from_secs(10);
    while !matches!(
        app.thumbnails.get(req.clone(), Priority::Visible),
        ThumbState::Unavailable
    ) {
        assert!(Instant::now() < deadline, "timed out");
        std::thread::sleep(Duration::from_millis(5));
    }
    // Previewed: the conversion makes the tile ask again, and it has one.
    let preview = PreviewRequest {
        entry: req.entry.clone(),
        mime: req.mime.as_deref().map(str::to_owned),
        page: 1,
        seconds: None,
        max_side: 512,
        text_size: None,
        fit: None,
        held: skwir_core::preview::HeldSlot::default(),
    };
    app.previews
        .render(&preview, &CancelToken::new())
        .unwrap()
        .expect("a preview");
    let page = wait_ready(&app, &req);
    assert!(page.height == 128 && page.width < 128, "a portrait page");
    assert_eq!(middle(&page), [255, 255, 255, 255], "a white page");
}

#[test]
#[ignore = "requires installed FFmpeg; run explicitly for preview changes"]
fn a_recording_shows_its_cover() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let dir = skwir_core::os_path::canonical(tmp.path()).unwrap();
    let path = dir.join("Synthetic track n\u{b0} 1.flac");
    let bare = dir.join("Synthetic track n\u{b0} 2.flac");
    let ffmpeg = |args: &[&str], out: &Path| {
        let status = std::process::Command::new("ffmpeg")
            .args(["-nostdin", "-v", "error", "-y"])
            .args(args)
            .arg(out)
            .status()
            .unwrap();
        assert!(status.success());
    };
    let tone = ["-f", "lavfi", "-i", "sine=frequency=440:duration=0.2"];
    ffmpeg(
        &[
            &tone[..],
            &["-f", "lavfi", "-i", "color=c=0x00ff00:s=40x60:d=1"],
            &["-map", "0:a", "-map", "1:v", "-frames:v", "1"],
            &["-c:v", "png", "-disposition:v", "attached_pic"],
        ]
        .concat(),
        &path,
    );
    ffmpeg(&tone, &bare);
    // From the desktop's audio thumbnailer where one is installed (its
    // pictures may be square), else from `AudioCovers`.
    let cover = wait_ready(&app, &request(&path));
    // Through YUV and back: green, give or take rounding.
    let [r, g, b, _] = middle(&cover);
    assert!(r < 8 && g > 247 && b < 8, "{:?}", middle(&cover));
    // A recording without a cover has none.
    let req = request(&bare);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !matches!(
        app.thumbnails.get(req.clone(), Priority::Visible),
        ThumbState::Unavailable
    ) {
        assert!(Instant::now() < deadline, "timed out");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
#[ignore = "requires installed Poppler tools; run explicitly for preview changes"]
fn a_new_pdf_gets_a_thumbnail_that_other_applications_reuse() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let dir = tmp.path().join("Papers \u{2019}24");
    std::fs::create_dir(&dir).unwrap();
    let path = skwir_core::os_path::canonical(&dir)
        .unwrap()
        .join("Synthetic \u{201c}report\u{201d}.pdf");
    red_page_pdf(&path);
    let img = wait_ready(&app, &request(&path));
    assert_eq!(img.width, 128, "the page is wider than tall");
    assert_eq!(middle(&img), [255, 0, 0, 255]);

    // Written back where every other application looks, for this version.
    let cache = skwir_platform::thumbnails::ThumbCache::desktop().unwrap();
    let cached = cache
        .lookup(&path, skwir_platform::thumbnails::ThumbSize::Normal)
        .expect("the thumbnail was shared through the desktop's cache");
    assert_eq!(
        skwir_platform::thumbnails::png_text_chunk(&cached, "Software").as_deref(),
        Some("skwir")
    );
    std::fs::remove_file(cached).unwrap();
}

/// A valid one-page PDF, 300 by 200 points, filled red.
fn red_page_pdf(path: &Path) {
    let stream = "1 0 0 rg 0 0 300 200 re f\n";
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Contents 4 0 R >>".to_owned(),
        format!("<< /Length {} >>\nstream\n{stream}endstream", stream.len()),
    ];
    let mut bytes = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        write!(bytes, "{} 0 obj\n{object}\nendobj\n", i + 1).unwrap();
    }
    let xref = bytes.len();
    writeln!(bytes, "xref\n0 {}\n0000000000 65535 f ", offsets.len() + 1).unwrap();
    for offset in &offsets {
        writeln!(bytes, "{offset:010} 00000 n ").unwrap();
    }
    write!(
        bytes,
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        offsets.len() + 1
    )
    .unwrap();
    std::fs::write(path, bytes).unwrap();
}
