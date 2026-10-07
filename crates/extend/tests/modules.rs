//! Plugins that run as WebAssembly modules (ADR-046), built from this
//! workspace and installed in a folder of plugins as a user would.
//!
//! Phase 6's gate: the hexadecimal preview again as a module shows, page
//! for page and match for match, what the built-in one shows.
#![cfg(not(windows))] // Built by cargo; the Windows tests run under Wine.

mod common;

use std::path::Path;
use std::sync::Arc;

use bibli_core::inspection::{InspectRequest, Inspection};
use bibli_core::op::EntryRef;
use bibli_core::plugin::Plugin;
use bibli_core::preview::PreviewRequest;
use bibli_core::{App, CancelToken};

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
    let module = common::module("bibli-outside-preview-hex");
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
    let app = App::new(bibli_core::app::AppConfig::ephemeral(
        tmp.path().join("state"),
    ))
    .unwrap();
    app.plugins
        .add(Arc::new(bibli_plugin_preview_hex::PreviewPlugin), &app);
    for plugin in bibli_extend::discover(std::slice::from_ref(&plugins), &["preview-hex"]) {
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
    let module = common::module("bibli-outside-preview-hex");
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
    std::fs::write(private.join(bibli_core::permissions::MARKER), "").unwrap();
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
    let module = common::module("bibli-outside-template");
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
    assert_eq!(words, Some(bibli_core::Value::UInt(7)));
    assert_eq!(
        app.fields.shown_for(std::ffi::OsStr::new("a note.txt")),
        [("word-count.words", "Words")],
        "Details shows it, labelled"
    );
    let other = tmp.path().join("other.md");
    std::fs::write(&other, "two words").unwrap();
    let outcome = app
        .dispatch(&bibli_core::command::Invocation::with_args(
            "word-count.count",
            &serde_json::json!({ "entries": [note, other] }),
        ))
        .unwrap();
    let bibli_core::command::Outcome::Value(value) = outcome else {
        panic!("{outcome:?}")
    };
    assert_eq!(value["total"], 9);
    // What the plugin says it does, an agent reads.
    assert_eq!(
        app.commands.get("word-count.count").unwrap().effect,
        bibli_core::command::Effect::Reads
    );
    // Turned off, it leaves nothing.
    assert!(app.plugins.set_enabled("word-count", false, &app));
    assert!(app.commands.get("word-count.count").is_none());
    assert_eq!(
        app.fields
            .get("word-count.words", &note, &CancelToken::new()),
        None
    );
    let _: &dyn Plugin = &*bibli_extend::discover(&[plugins], &[])[0];
}

#[test]
fn a_thumbnailer_from_outside_draws_through_the_thumbnail_service() {
    use bibli_core::thumbs::{ThumbRequest, ThumbSize, ThumbState};
    let module = common::module("bibli-outside-netpbm");
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
            .get(request.clone(), bibli_core::scheduler::Priority::Visible)
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
