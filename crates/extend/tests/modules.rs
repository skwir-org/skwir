//! Plugins that run as WebAssembly modules (ADR-046), built from this
//! workspace and installed in a folder of plugins as a user would.
//!
//! Phase 6's gate: the hexadecimal preview again as a module shows, page
//! for page and match for match, what the built-in one shows.
#![cfg(not(windows))] // Built by cargo; the Windows tests run under Wine.

mod common;

use std::path::Path;
use std::sync::Arc;

use skwir_core::inspection::{InspectRequest, Inspection};
use skwir_core::op::EntryRef;
use skwir_core::plugin::Plugin;
use skwir_core::preview::PreviewRequest;
use skwir_core::{App, CancelToken};

fn manifest(crate_folder: &str) -> String {
    std::fs::read_to_string(common::root().join(crate_folder).join("plugin.toml")).unwrap()
}

fn request(path: &Path) -> PreviewRequest {
    PreviewRequest {
        entry: EntryRef::from_path(path).unwrap(),
        mime: Some("application/octet-stream".into()),
        page: 1,
        seconds: None,
        max_side: 64,
        text_size: None,
    }
}

/// What `provider` shows of `path` for `view`: its page, or its error.
fn shown(
    app: &App,
    provider: &str,
    path: &Path,
    view: &InspectRequest,
) -> Result<Inspection, String> {
    let view = InspectRequest {
        provider: Some(provider.into()),
        ..view.clone()
    };
    app.previews
        .inspect(&request(path), &view, &CancelToken::new())
        .map_err(|error| format!("{error:#}"))
        .map(|page| page.expect("a page"))
}

/// What a user sees of a page: everything the hex view draws.
fn seen(page: &Inspection) -> String {
    format!(
        "{:?} {} {:?} {:?} prev={:?} next={:?} at={} {:?}",
        page.kind,
        page.title,
        page.columns,
        page.rows
            .iter()
            .map(|row| (
                row.cells.clone(),
                row.bytes,
                row.depth,
                row.branch,
                row.target.clone()
            ))
            .collect::<Vec<_>>(),
        page.previous,
        page.next,
        page.position,
        page.notice,
    )
}

#[test]
fn the_hex_view_as_a_module_shows_what_the_built_in_one_shows() {
    let module = common::module("skwir-outside-preview-hex");
    let tmp = common::home_tempdir();
    let plugins = tmp.path().join("plugins");
    common::install(
        &plugins,
        "hex-view",
        &manifest("crates/plugins-outside/preview-hex"),
        &module,
        "hex_view.wasm",
    );
    // Built in first, then from outside, as the binary lists them.
    common::module_worker();
    let app = App::new(skwir_core::app::AppConfig::ephemeral(
        tmp.path().join("state"),
    ))
    .unwrap();
    app.plugins
        .add(Arc::new(skwir_plugin_preview_hex::PreviewPlugin), &app);
    for plugin in skwir_extend::discover(std::slice::from_ref(&plugins), &["preview-hex"]) {
        app.plugins.add(plugin, &app);
    }
    assert!(app.plugins.set_enabled("hex-view", true, &app));

    let files = tmp.path().join("files");
    std::fs::create_dir(&files).unwrap();
    let mut big = vec![0u8; 70_000];
    for (i, byte) in big.iter_mut().enumerate() {
        *byte = (i * 7 % 251) as u8;
    }
    for at in [100, 108, 8000, 65_530] {
        big[at..at + 6].copy_from_slice(b"needle");
    }
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("synthetic bytes.bin", big),
        ("tiny.bin", b"0123456789".to_vec()),
        ("empty.bin", Vec::new()),
        ("café [2026].dat", (0..=255u8).collect()),
    ];
    let views = [
        InspectRequest::default(),
        InspectRequest {
            offset: 4096,
            ..InspectRequest::default()
        },
        InspectRequest {
            offset: 69_000,
            ..InspectRequest::default()
        },
        InspectRequest {
            query: Some("needle".into()),
            ..InspectRequest::default()
        },
        InspectRequest {
            query: Some("needle".into()),
            offset: 101,
            ..InspectRequest::default()
        },
        // Across the 64 KiB chunks a search reads at a time.
        InspectRequest {
            query: Some("needle".into()),
            offset: 8001,
            ..InspectRequest::default()
        },
        InspectRequest {
            query: Some("hex: 6e 65 65".into()),
            offset: 9000,
            ..InspectRequest::default()
        },
        InspectRequest {
            query: Some("absent!".into()),
            ..InspectRequest::default()
        },
        InspectRequest {
            query: Some("hex:6".into()),
            ..InspectRequest::default()
        },
    ];
    let mut compared = 0;
    for (name, bytes) in &cases {
        let path = files.join(name);
        std::fs::write(&path, bytes).unwrap();
        for view in &views {
            let built_in = shown(&app, "preview-hex", &path, view);
            let module = shown(&app, "hex-view", &path, view);
            match (&built_in, &module) {
                (Ok(a), Ok(b)) => assert_eq!(seen(a), seen(b), "{name} {view:?}"),
                (Err(a), Err(b)) => assert_eq!(a, b, "{name} {view:?}"),
                _ => panic!("{name} {view:?}: {built_in:?} against {module:?}"),
            }
            compared += 1;
        }
    }
    assert_eq!(compared, cases.len() * views.len());

    // The view every file offers is the module's once the built-in one is off.
    assert_eq!(app.previews.bytes_view(), Some("preview-hex"));
    assert!(app.plugins.set_enabled("preview-hex", false, &app));
    assert_eq!(app.previews.bytes_view(), Some("hex-view"));
}

#[test]
fn a_module_reads_only_what_it_may_and_never_a_private_file() {
    let module = common::module("skwir-outside-preview-hex");
    let tmp = common::home_tempdir();
    let plugins = tmp.path().join("plugins");
    // Declared without `read`: it may not read what files hold.
    let blind = manifest("crates/plugins-outside/preview-hex")
        .replace("capabilities = [\"read\"]", "capabilities = []");
    common::install(&plugins, "hex-view", &blind, &module, "hex_view.wasm");
    let app = common::app_with(&tmp.path().join("state"), &plugins, &[]);
    let file = tmp.path().join("bytes.bin");
    std::fs::write(&file, b"0123456789").unwrap();
    let refused = shown(&app, "hex-view", &file, &InspectRequest::default()).unwrap_err();
    assert!(refused.contains("may not read"), "{refused}");
    assert_eq!(
        app.plugins
            .list()
            .into_iter()
            .find(|p| p.id == "hex-view")
            .map(|p| p.capabilities.words()),
        Some(None),
        "and the Plugins page says it may do nothing"
    );

    // With `read`, a private file is never handed to it.
    let readable = tmp.path().join("readable");
    common::install(
        &readable,
        "hex-view",
        &manifest("crates/plugins-outside/preview-hex"),
        &module,
        "hex_view.wasm",
    );
    let app = common::app_with(&tmp.path().join("state2"), &readable, &[]);
    let private = tmp.path().join("diary");
    std::fs::create_dir(&private).unwrap();
    std::fs::write(private.join(skwir_core::permissions::MARKER), "").unwrap();
    std::fs::write(private.join("entry.bin"), b"secret").unwrap();
    let withheld = shown(
        &app,
        "hex-view",
        &private.join("entry.bin"),
        &InspectRequest::default(),
    )
    .unwrap_err();
    assert!(withheld.contains("private"), "{withheld}");
    assert!(shown(&app, "hex-view", &file, &InspectRequest::default()).is_ok());
}

#[test]
fn the_template_counts_words_as_a_module_a_field_and_a_command() {
    let module = common::module("skwir-outside-template");
    let tmp = common::home_tempdir();
    let plugins = tmp.path().join("plugins");
    common::install(
        &plugins,
        "word-count",
        &manifest("crates/plugins-outside/template"),
        &module,
        "word_count.wasm",
    );
    let app = common::app_with(&tmp.path().join("state"), &plugins, &[]);
    let note = tmp.path().join("a note.txt");
    std::fs::write(&note, "four words in here\nand three more").unwrap();
    let words = app
        .fields
        .get("word-count.words", &note, &CancelToken::new());
    assert_eq!(words, Some(skwir_core::Value::UInt(7)));
    assert_eq!(
        app.fields.shown_for(std::ffi::OsStr::new("a note.txt")),
        [("word-count.words", "Words")],
        "Properties shows it, labelled"
    );
    let other = tmp.path().join("other.md");
    std::fs::write(&other, "two words").unwrap();
    let outcome = app
        .dispatch(&skwir_core::command::Invocation::with_args(
            "word-count.count",
            &serde_json::json!({ "entries": [note, other] }),
        ))
        .unwrap();
    let skwir_core::command::Outcome::Value(value) = outcome else {
        panic!("{outcome:?}")
    };
    assert_eq!(value["total"], 9);
    // What the plugin says it does, an agent reads.
    assert_eq!(
        app.commands.get("word-count.count").unwrap().effect,
        skwir_core::command::Effect::Reads
    );
    // Its helper kept the module compiled in the plugin's folder of the
    // cache; started again, it loads it rather than compiling it.
    let compiled = app.cache.namespace("word-count").folder("module");
    let kept = || -> Vec<(std::path::PathBuf, std::time::SystemTime)> {
        std::fs::read_dir(&compiled)
            .unwrap()
            .map(|e| {
                let path = e.unwrap().path();
                let written = std::fs::metadata(&path).unwrap().modified().unwrap();
                (path, written)
            })
            .collect()
    };
    let first = kept();
    assert_eq!(first.len(), 1, "{first:?}");
    assert!(app.plugins.set_enabled("word-count", false, &app));
    assert!(app.plugins.set_enabled("word-count", true, &app));
    assert_eq!(
        app.fields
            .get("word-count.words", &other, &CancelToken::new()),
        Some(skwir_core::Value::UInt(2))
    );
    assert_eq!(kept(), first);
    // Turned off, it leaves nothing.
    assert!(app.plugins.set_enabled("word-count", false, &app));
    assert!(app.commands.get("word-count.count").is_none());
    assert_eq!(
        app.fields
            .get("word-count.words", &note, &CancelToken::new()),
        None
    );
    let _: &dyn Plugin = &*skwir_extend::discover(&[plugins], &[])[0];
}

#[test]
fn a_thumbnailer_from_outside_draws_through_the_thumbnail_service() {
    use skwir_core::thumbs::{ThumbRequest, ThumbSize, ThumbState};
    let module = common::module("skwir-outside-netpbm");
    let tmp = common::home_tempdir();
    let plugins = tmp.path().join("plugins");
    common::install(
        &plugins,
        "netpbm",
        &manifest("crates/plugins-outside/netpbm"),
        &module,
        "netpbm.wasm",
    );
    let app = common::app_with(&tmp.path().join("state"), &plugins, &[]);
    assert_eq!(app.thumbnails.provider_ids(), ["netpbm"]);
    // A picture of 400 by 100, its left half red, its right half blue.
    let picture = tmp.path().join("bands.ppm");
    let mut bytes = b"P6\n400 100\n255\n".to_vec();
    for _ in 0..100 {
        for x in 0..400 {
            bytes.extend(if x < 200 { [255, 0, 0] } else { [0, 0, 255] });
        }
    }
    std::fs::write(&picture, bytes).unwrap();
    let request = ThumbRequest {
        entry: EntryRef::from_path(&picture).unwrap(),
        mime: Some("image/x-portable-pixmap".into()),
        mtime: None,
        size: ThumbSize::Normal,
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let image = loop {
        match app
            .thumbnails
            .get(request.clone(), skwir_core::scheduler::Priority::Visible)
        {
            ThumbState::Ready(image) => break image,
            ThumbState::Pending => {
                assert!(std::time::Instant::now() < deadline, "drawn in time");
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            ThumbState::Unavailable => panic!("no thumbnail"),
        }
    };
    assert_eq!((image.width, image.height), (128, 32));
    let pixel = |x: usize, y: usize| {
        let at = (y * image.width as usize + x) * 4;
        image.rgba[at..at + 4].to_vec()
    };
    assert_eq!(pixel(10, 10), [255, 0, 0, 255]);
    assert_eq!(pixel(120, 20), [0, 0, 255, 255]);
}

/// How many processes this test started that still run: a plugin's
/// helper, or the sandbox it runs in.
#[cfg(target_os = "linux")]
fn children() -> usize {
    let me = std::process::id().to_string();
    std::fs::read_dir("/proc")
        .unwrap()
        .flatten()
        .filter(|process| {
            let stat = std::fs::read_to_string(process.path().join("stat")).unwrap_or_default();
            // `pid (name) state parent …`, the name being anything.
            stat.rsplit_once(") ")
                .and_then(|(_, rest)| rest.split_whitespace().nth(1))
                == Some(me.as_str())
        })
        .count()
}

#[test]
fn a_picture_from_outside_is_previewed_as_the_png_it_makes() {
    let module = common::module("skwir-outside-netpbm");
    let tmp = common::home_tempdir();
    let plugins = tmp.path().join("plugins");
    let installed = common::install(
        &plugins,
        "netpbm",
        &manifest("crates/plugins-outside/netpbm"),
        &module,
        "netpbm.wasm",
    );
    let app = common::app_with(&tmp.path().join("state"), &plugins, &[]);
    // A picture of 40 by 10, its left half red, its right half blue.
    let picture = tmp.path().join("bands it's.ppm");
    let mut bytes = b"P6\n40 10\n255\n".to_vec();
    for _ in 0..10 {
        for x in 0..40 {
            bytes.extend(if x < 20 { [255, 0, 0] } else { [0, 0, 255] });
        }
    }
    std::fs::write(&picture, bytes).unwrap();
    let asked = PreviewRequest {
        mime: Some("image/x-portable-pixmap".into()),
        ..request(&picture)
    };
    let made = app
        .previews
        .converted(&asked, &CancelToken::new())
        .unwrap()
        .expect("shown as another file");
    // A PNG of the whole picture, typed by its name and bytes, kept in the
    // plugin's folder of the cache.
    assert_eq!(made.mime.as_deref(), Some("image/png"));
    assert_eq!(made.path.file_name().unwrap(), "bands it's.png");
    assert!(
        made.path
            .starts_with(app.cache.namespace("netpbm").folder("converted"))
    );
    let png = std::fs::read(&made.path).unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(&png[16..24], &[0, 0, 0, 40, 0, 0, 0, 10], "its size");
    // A module may not use the network: what it made keeps its links.
    assert!(made.closed_links.unwrap().contains("network"));
    // Shown again, even with its helper ended: the file kept, the plugin
    // not asked, no helper started.
    assert!(app.plugins.set_enabled("netpbm", false, &app));
    assert!(app.plugins.set_enabled("netpbm", true, &app));
    #[cfg(target_os = "linux")]
    {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while children() > 0 {
            assert!(std::time::Instant::now() < deadline, "the helper ended");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
    let again = app
        .previews
        .converted(&asked, &CancelToken::new())
        .unwrap()
        .unwrap();
    assert_eq!(again.path, made.path);
    #[cfg(target_os = "linux")]
    assert_eq!(children(), 0, "nothing was started");
    // The picture changed: made again.
    std::fs::write(&picture, b"P5\n2 1\n255\n\x00\xff").unwrap();
    let changed = app
        .previews
        .converted(&asked, &CancelToken::new())
        .unwrap()
        .unwrap();
    assert_ne!(changed.path, made.path);
    // A new version of the plugin makes it again too.
    let wasm = installed.join("netpbm.wasm");
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(&wasm, std::fs::read(&wasm).unwrap()).unwrap();
    let updated = app
        .previews
        .converted(&asked, &CancelToken::new())
        .unwrap()
        .unwrap();
    assert_ne!(updated.path, changed.path);
    assert_eq!(
        &std::fs::read(&changed.path).unwrap()[16..24],
        &[0, 0, 0, 2, 0, 0, 0, 1]
    );
    // A private file is never handed to it.
    std::fs::write(tmp.path().join(".private"), "").unwrap();
    let private = tmp.path().join("kept.ppm");
    std::fs::write(&private, b"P5\n1 1\n255\n\x00").unwrap();
    let refused = app.previews.converted(
        &PreviewRequest {
            mime: Some("image/x-portable-pixmap".into()),
            ..request(&private)
        },
        &CancelToken::new(),
    );
    assert!(refused.is_err());
}

#[test]
fn a_file_of_several_pictures_is_previewed_a_page_at_a_time() {
    let module = common::module("skwir-outside-netpbm");
    let tmp = common::home_tempdir();
    let plugins = tmp.path().join("plugins");
    common::install(
        &plugins,
        "netpbm",
        &manifest("crates/plugins-outside/netpbm"),
        &module,
        "netpbm.wasm",
    );
    let app = common::app_with(&tmp.path().join("state"), &plugins, &[]);
    // Two pictures one after another: 40 by 10 in red, 20 by 20 in grey.
    let file = tmp.path().join("two it's.ppm");
    let mut bytes = b"P6\n40 10\n255\n".to_vec();
    bytes.extend(std::iter::repeat_n([255u8, 0, 0], 400).flatten());
    bytes.extend(b"P5\n20 20\n255\n");
    bytes.extend([128u8; 400]);
    std::fs::write(&file, bytes).unwrap();
    let asked = |page| PreviewRequest {
        mime: Some("image/x-portable-pixmap".into()),
        page,
        max_side: 64,
        ..request(&file)
    };
    let cancel = CancelToken::new();
    // Not one picture: no file to show instead, asked once.
    assert!(
        app.previews
            .converted(&asked(1), &cancel)
            .unwrap()
            .is_none()
    );
    assert!(
        app.previews
            .converted(&asked(2), &cancel)
            .unwrap()
            .is_none()
    );
    // Read as a document: a page a picture, its own size the picture's.
    let open = || {
        app.previews
            .open_document(&asked(1), &cancel)
            .unwrap()
            .expect("shown a page at a time")
    };
    let pages = open();
    assert_eq!(pages.info().pages, 2);
    let first = pages.render(1, 64, &cancel).unwrap();
    assert_eq!((first.width, first.height), (40, 10));
    assert_eq!((first.content.width, first.content.height), (40., 10.));
    assert_eq!(&first.pixels[..4], &[255, 0, 0, 255]);
    assert!(first.content.chars.is_empty(), "a picture has no text");
    let second = pages.render(2, 64, &cancel).unwrap();
    assert_eq!((second.width, second.height), (20, 20));
    assert_eq!(&second.pixels[..4], &[128, 128, 128, 255]);
    assert!(format!("{:#}", pages.render(3, 64, &cancel).unwrap_err()).contains("no page 3"));
    assert!(pages.search("anything", &cancel).unwrap().is_empty());
    // A file of one picture is shown as its PNG, not as pages.
    let one = tmp.path().join("one.ppm");
    std::fs::write(&one, b"P5\n1 1\n255\n\x00").unwrap();
    let single = PreviewRequest {
        mime: Some("image/x-portable-pixmap".into()),
        ..request(&one)
    };
    assert!(app.previews.converted(&single, &cancel).unwrap().is_some());
    // Turned to again with its helper ended: the page kept, nothing
    // started.
    assert!(app.plugins.set_enabled("netpbm", false, &app));
    assert!(app.plugins.set_enabled("netpbm", true, &app));
    #[cfg(target_os = "linux")]
    {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while children() > 0 {
            assert!(std::time::Instant::now() < deadline, "the helper ended");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
    assert!(
        app.previews
            .converted(&asked(2), &cancel)
            .unwrap()
            .is_none()
    );
    let again = open().render(2, 64, &cancel).unwrap();
    assert_eq!((again.width, again.height), (20, 20));
    #[cfg(target_os = "linux")]
    assert_eq!(children(), 0, "nothing was started");
}
