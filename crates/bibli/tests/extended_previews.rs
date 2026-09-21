//! Synthetic integration tests exercise provider selection and runtime withdrawal.
use bibli_core::{
    App, CancelToken,
    app::AppConfig,
    inspection::{BlockKind, InspectRequest, Inspection, InspectionKind},
    op::EntryRef,
    plugin::Plugin,
    preview::PreviewRequest,
};
use std::{path::Path, sync::Arc};
fn app(root: &Path) -> Arc<App> {
    let app = App::new(AppConfig::ephemeral(root.join("state"))).unwrap();
    for plugin in [
        Arc::new(bibli_plugin_preview_markdown::PreviewPlugin) as Arc<dyn Plugin>,
        Arc::new(bibli_plugin_preview_tables::PreviewPlugin),
        Arc::new(bibli_plugin_preview_structured::PreviewPlugin),
        Arc::new(bibli_plugin_preview_email::PreviewPlugin),
        Arc::new(bibli_plugin_preview_epub::PreviewPlugin),
        Arc::new(bibli_plugin_preview_hex::PreviewPlugin),
    ] {
        app.plugins.add(plugin, &app);
    }
    app
}
fn request(path: &Path) -> PreviewRequest {
    PreviewRequest {
        entry: EntryRef::from_path(path).unwrap(),
        mime: Some("application/octet-stream".into()),
        page: 1,
        seconds: None,
        max_side: 512,
    }
}
fn inspect(app: &App, path: &Path, view: &InspectRequest) -> Inspection {
    app.previews
        .inspect(&request(path), view, &CancelToken::new())
        .unwrap()
        .unwrap()
}
#[test]
fn asf_media_ownership_precedes_hex_fallback_but_allows_explicit_hex() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("Synthetic container.ASF");
    std::fs::write(&path, b"synthetic container marker").unwrap();
    let app = app(tmp.path());
    app.plugins
        .add(Arc::new(bibli_plugin_preview_media::PreviewPlugin), &app);
    let request = request(&path); // Deliberately generic application/octet-stream.
    let default = InspectRequest::default();
    assert!(
        app.previews
            .inspect(&request, &default, &CancelToken::new())
            .unwrap()
            .is_none()
    );
    let explicit = InspectRequest {
        provider: Some("preview-hex".into()),
        ..InspectRequest::default()
    };
    assert!(inspect(&app, &path, &explicit).title.starts_with("Hex"));
    assert!(app.plugins.set_enabled("preview-media", false, &app));
    assert!(inspect(&app, &path, &default).title.starts_with("Hex"));
    assert!(app.plugins.set_enabled("preview-media", true, &app));
    assert!(
        app.previews
            .inspect(&request, &default, &CancelToken::new())
            .unwrap()
            .is_none()
    );
}

#[test]
fn markdown_styles_source_and_plugin_fallback() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("Synthetic d'Example.md");
    std::fs::write(
        &path,
        "# Heading\n\nText with **bold** and [link](https://example.invalid).\n\n- [x] Checked\n- [ ] Todo\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n---\n",
    )
    .unwrap();
    let app = app(tmp.path());
    let rendered = inspect(&app, &path, &InspectRequest::default());
    assert_eq!(rendered.title, "Markdown");
    assert_eq!(rendered.kind, InspectionKind::Markdown);
    assert!(rendered.blocks.iter().any(|b| b.heading == 1));
    assert!(
        rendered
            .blocks
            .iter()
            .any(|b| b.runs.iter().any(|r| r.bold))
    );
    assert!(rendered.plain_text().contains('☑'));
    let item = rendered
        .blocks
        .iter()
        .find(|b| b.marker.as_deref() == Some("•"))
        .expect("bulleted item");
    assert_eq!(item.list_depth, 1);
    let header = rendered
        .blocks
        .iter()
        .find(|b| matches!(&b.kind, BlockKind::TableRow { header, .. } if *header))
        .expect("table head");
    let BlockKind::TableRow { cells, .. } = &header.kind else {
        unreachable!()
    };
    assert_eq!(
        cells
            .iter()
            .map(|r| &header.text[r.clone()])
            .collect::<Vec<_>>(),
        ["a", "b"]
    );
    assert!(rendered.blocks.iter().any(|b| b.kind == BlockKind::Rule));
    // A table row copies as one line, not as one line per cell.
    assert!(rendered.plain_text().contains("a\tb\n1\t2"));
    let source = inspect(
        &app,
        &path,
        &InspectRequest {
            source: true,
            ..InspectRequest::default()
        },
    );
    assert!(source.plain_text().contains("**bold**"));
    assert!(app.plugins.set_enabled("preview-markdown", false, &app));
    assert!(
        inspect(&app, &path, &InspectRequest::default())
            .title
            .starts_with("Hex")
    );
    assert!(app.plugins.set_enabled("preview-markdown", true, &app));
    // Registry order cannot let the fallback claim a known format on re-enable.
    assert_eq!(
        inspect(&app, &path, &InspectRequest::default()).title,
        "Markdown"
    );
}
#[test]
fn csv_quoted_newlines_and_structured_code() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let csv = tmp.path().join("Synthetic table.csv");
    std::fs::write(
        &csv,
        "name,note\nexample,\"line one\nline two\"\nsecond,\"a,b\"\n",
    )
    .unwrap();
    let table = inspect(&app, &csv, &InspectRequest::default());
    assert_eq!(table.columns, ["name", "note"]);
    assert_eq!(table.rows[0].cells[1], "line one\nline two");
    assert_eq!(table.rows[1].cells[1], "a,b");
    for (name, source) in [
        ("data.json", r#"{"group":{"items":[1,2]}}"#),
        ("data.yaml", "group:\n  items: [1, 2]\n"),
        ("data.toml", "[group]\nitems = [1, 2]\n"),
    ] {
        let path = tmp.path().join(name);
        std::fs::write(&path, source).unwrap();
        let code = inspect(&app, &path, &InspectRequest::default());
        assert_eq!(code.kind, bibli_core::inspection::InspectionKind::Code);
        assert!(code.blocks.iter().all(|block| block.code));
        assert!(!code.folds.is_empty());
        assert!(code.plain_text().contains("items"));
        if name != "data.json" {
            assert_eq!(code.plain_text(), source);
        }
    }
}
#[test]
fn structured_source_toggle_and_invalid_input_remain_readable() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let path = tmp.path().join("Synthetic document.json");
    let source = r#"{"nested":{"enabled":true}}"#;
    std::fs::write(&path, source).unwrap();
    let formatted = inspect(&app, &path, &InspectRequest::default());
    assert!(formatted.blocks.len() > 1);
    let original = inspect(
        &app,
        &path,
        &InspectRequest {
            source: true,
            ..InspectRequest::default()
        },
    );
    assert_eq!(original.plain_text(), source);
    std::fs::write(&path, "{incomplete").unwrap();
    let invalid = inspect(&app, &path, &InspectRequest::default());
    assert_eq!(invalid.plain_text(), "{incomplete");
    assert!(invalid.notice.starts_with("Showing source:"));
    // Bounded fallback remains pageable even for a single huge source line.
    let large = format!("\"{}\"", "a".repeat(40000));
    std::fs::write(&path, &large).unwrap();
    let first = inspect(&app, &path, &InspectRequest::default());
    let next = first.next.unwrap();
    let second = inspect(
        &app,
        &path,
        &InspectRequest {
            offset: next,
            ..InspectRequest::default()
        },
    );
    assert_eq!(
        format!("{}{}", first.plain_text(), second.plain_text()),
        large
    );
}

#[test]
fn hex_ranges_search_and_truncation() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let path = tmp.path().join("Synthetic bytes.bin");
    let mut bytes = vec![0; 8200];
    bytes[4094..4100].copy_from_slice(b"needle");
    std::fs::write(&path, &bytes).unwrap();
    let view = InspectRequest {
        provider: Some("preview-hex".into()),
        ..InspectRequest::default()
    };
    let first = inspect(&app, &path, &view);
    assert_eq!(first.rows.len(), 256);
    assert_eq!(first.next, Some(4096));
    let found = inspect(
        &app,
        &path,
        &InspectRequest {
            query: Some("hex: 6e 65 65 64 6c 65".into()),
            ..view.clone()
        },
    );
    assert_eq!(found.position, 4094, "the exact match byte, for find-next");
    assert_eq!(found.rows[0].cells[0], "0000000000000FF0");
    assert!(found.notice.contains("FFE"));
    std::fs::write(&path, [1, 2]).unwrap();
    let truncated = inspect(
        &app,
        &path,
        &InspectRequest {
            offset: u64::MAX,
            ..view
        },
    );
    assert_eq!(truncated.position, 0);
    assert_eq!(truncated.rows.len(), 1);
}
#[test]
fn email_decodes_headers_body_and_attachment_without_fetching() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let path = tmp.path().join("Synthetic message.eml");
    std::fs::write(&path,"From: Example <sender@example.invalid>\r\nTo: Reader <reader@example.invalid>\r\nSubject: =?UTF-8?Q?Synthetic_caf=C3=A9?=\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=sample\r\n\r\n--sample\r\nContent-Type: text/html; charset=UTF-8\r\n\r\n<h1>Hello</h1><p>A message</p><img src='https://example.invalid/remote.png'><script>hidden</script>\r\n--sample\r\nContent-Type: text/plain\r\nContent-Disposition: attachment; filename=sample.txt\r\nContent-Transfer-Encoding: base64\r\n\r\naGVsbG8=\r\n--sample--\r\n").unwrap();
    let mail = inspect(&app, &path, &InspectRequest::default());
    assert!(mail.plain_text().contains("Synthetic café"));
    assert!(!mail.plain_text().contains("hidden"));
    assert_eq!(mail.rows.len(), 1);
    let member = inspect(
        &app,
        &path,
        &InspectRequest {
            target: Some("0".into()),
            ..InspectRequest::default()
        },
    );
    assert!(member.plain_text().contains("hello"));

    // The panel's way: the attachment becomes a file of its own, in the
    // directory it was given and under a name of ours, and is then previewed
    // as what it is, here by whichever provider reads text files.
    let into = tmp.path().join("members");
    std::fs::create_dir(&into).unwrap();
    let extracted = app
        .previews
        .extract(&request(&path), "0", &into, &CancelToken::new())
        .unwrap()
        .unwrap();
    assert_eq!(extracted.name, "sample.txt");
    assert_eq!(extracted.path, into.join("member.txt"));
    assert_eq!(std::fs::read(&extracted.path).unwrap(), b"hello");
    assert!(
        app.previews
            .extract(&request(&path), "7", &into, &CancelToken::new())
            .is_err(),
        "no such attachment"
    );
    // A provider whose targets are not files says so; nothing is written.
    let notes = tmp.path().join("notes.md");
    std::fs::write(&notes, "# Title\n").unwrap();
    assert!(
        app.previews
            .extract(&request(&notes), "0", &into, &CancelToken::new())
            .unwrap()
            .is_none()
    );
}
#[test]
fn epub_chapters_source_navigation_and_disable() {
    use std::io::Write;
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let path = tmp.path().join("Synthetic book.epub");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    for (name, body) in [
        ("mimetype", "application/epub+zip"),
        (
            "META-INF/container.xml",
            r#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="EPUB/book.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
        ),
        (
            "EPUB/book.opf",
            r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">urn:example:synthetic</dc:identifier><dc:title>Synthetic book</dc:title><dc:language>en</dc:language></metadata><manifest><item id="one" href="one.xhtml" media-type="application/xhtml+xml"/><item id="two" href="two.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="one"/><itemref idref="two"/></spine></package>"#,
        ),
        (
            "EPUB/one.xhtml",
            r#"<html xmlns="http://www.w3.org/1999/xhtml"><body><h1>First chapter</h1><p>First text.</p><a href="two.xhtml">Next</a></body></html>"#,
        ),
        (
            "EPUB/two.xhtml",
            r#"<html xmlns="http://www.w3.org/1999/xhtml"><body><h1>Second chapter</h1><p>Second text.</p></body></html>"#,
        ),
    ] {
        zip.start_file(
            name,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored),
        )
        .unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    }
    zip.finish().unwrap();
    let first = inspect(&app, &path, &InspectRequest::default());
    assert!(first.plain_text().contains("First chapter"));
    assert_eq!(first.next, Some(1));
    let second = inspect(
        &app,
        &path,
        &InspectRequest {
            offset: 1,
            ..InspectRequest::default()
        },
    );
    assert!(second.plain_text().contains("Second chapter"));
    assert_eq!(second.previous, Some(0));
    let searched = inspect(
        &app,
        &path,
        &InspectRequest {
            query: Some("Second text".into()),
            ..InspectRequest::default()
        },
    );
    assert_eq!(searched.position, 1);
    assert_eq!(
        first.rows.len(),
        2,
        "Books without a TOC still expose chapters"
    );
    let linked = inspect(
        &app,
        &path,
        &InspectRequest {
            target: Some("/EPUB/two.xhtml".into()),
            ..InspectRequest::default()
        },
    );
    assert_eq!(linked.position, 1);
    let source = inspect(
        &app,
        &path,
        &InspectRequest {
            source: true,
            ..InspectRequest::default()
        },
    );
    assert!(source.plain_text().contains("<h1>"));
}

/// Every preview plugin the binary wires, each constructed the way main.rs does.
fn all_preview_plugins() -> Vec<(&'static str, Arc<dyn Plugin>)> {
    vec![
        (
            "preview-media",
            Arc::new(bibli_plugin_preview_media::PreviewPlugin),
        ),
        (
            "preview-documents",
            Arc::new(bibli_plugin_preview_documents::PreviewPlugin),
        ),
        (
            "preview-audio",
            Arc::new(bibli_plugin_preview_audio::PreviewPlugin),
        ),
        (
            "preview-markdown",
            Arc::new(bibli_plugin_preview_markdown::PreviewPlugin),
        ),
        (
            "preview-archives",
            Arc::new(bibli_plugin_preview_archives::PreviewPlugin),
        ),
        (
            "preview-epub",
            Arc::new(bibli_plugin_preview_epub::PreviewPlugin),
        ),
        (
            "preview-tables",
            Arc::new(bibli_plugin_preview_tables::PreviewPlugin),
        ),
        (
            "preview-structured",
            Arc::new(bibli_plugin_preview_structured::PreviewPlugin),
        ),
        (
            "preview-fonts",
            Arc::new(bibli_plugin_preview_fonts::PreviewPlugin),
        ),
        (
            "preview-email",
            Arc::new(bibli_plugin_preview_email::PreviewPlugin),
        ),
        (
            "preview-comics",
            Arc::new(bibli_plugin_preview_comics::PreviewPlugin),
        ),
        (
            "preview-hex",
            Arc::new(bibli_plugin_preview_hex::PreviewPlugin),
        ),
    ]
}

/// Equal-priority providers are ordered by registration, so their accept sets
/// must stay disjoint: at most one non-fallback plugin may claim a file. The
/// registry has no accept query, so each plugin is probed alone through
/// `render`, which returns `Ok(None)` only when nothing accepted the request.
#[test]
fn format_providers_never_compete_for_the_same_file() {
    let tmp = tempfile::tempdir().unwrap();
    let fixtures: Vec<(&str, Option<&str>)> = vec![
        ("notes.md", Some("text/markdown")),
        ("table.csv", Some("text/csv")),
        ("table.tsv", Some("text/tab-separated-values")),
        ("data.json", Some("application/json")),
        ("data.yaml", Some("application/yaml")),
        ("data.yml", None),
        ("data.toml", Some("application/toml")),
        ("message.eml", Some("message/rfc822")),
        ("book.epub", Some("application/epub+zip")),
        ("face.ttf", Some("font/ttf")),
        ("face.otf", Some("font/otf")),
        ("bundle.zip", Some("application/zip")),
        ("bundle.tar.gz", Some("application/gzip")),
        ("bundle.7z", Some("application/x-7z-compressed")),
        ("pages.cbz", Some("application/vnd.comicbook+zip")),
        ("pages.cbr", None),
        ("clip.asf", Some("application/vnd.ms-asf")),
        ("clip.mp4", Some("video/mp4")),
        ("song.mp3", Some("audio/mpeg")),
        ("paper.pdf", Some("application/pdf")),
        ("text.odt", Some("application/vnd.oasis.opendocument.text")),
        ("blob.bin", Some("application/octet-stream")),
        ("photo.jpg", Some("image/jpeg")),
    ];
    let mut claims: Vec<(String, Vec<&str>)> = Vec::new();
    for (name, mime) in &fixtures {
        let path = tmp.path().join(name);
        std::fs::write(&path, b"synthetic").unwrap();
        let mut accepted = Vec::new();
        for (id, plugin) in all_preview_plugins() {
            let app = App::new(AppConfig::ephemeral(tmp.path().join("state").join(id))).unwrap();
            app.plugins.add(plugin, &app);
            let request = PreviewRequest {
                entry: EntryRef::from_path(&path).unwrap(),
                mime: mime.map(str::to_owned),
                page: 1,
                seconds: None,
                max_side: 64,
            };
            if !matches!(app.previews.render(&request, &CancelToken::new()), Ok(None)) {
                accepted.push(id);
            }
        }
        claims.push(((*name).to_owned(), accepted));
    }
    for (name, accepted) in &claims {
        let format_owners: Vec<_> = accepted.iter().filter(|id| **id != "preview-hex").collect();
        assert!(
            format_owners.len() <= 1,
            "{name} is claimed by several format providers: {format_owners:?}"
        );
    }
    let owner = |name: &str| claims.iter().find(|(n, _)| n == name).unwrap().1.clone();
    assert_eq!(owner("notes.md"), ["preview-markdown"]);
    assert_eq!(owner("data.yml"), ["preview-structured"]);
    assert_eq!(owner("bundle.tar.gz"), ["preview-archives"]);
    assert_eq!(owner("pages.cbr"), ["preview-comics"]);
    assert_eq!(owner("clip.asf"), ["preview-media"]);
    assert_eq!(owner("song.mp3"), ["preview-audio"]);
    assert_eq!(owner("paper.pdf"), ["preview-documents"]);
    assert_eq!(
        owner("blob.bin"),
        ["preview-hex"],
        "only the fallback owns generic bytes"
    );
    assert!(
        owner("photo.jpg").is_empty(),
        "bitmaps are the shell's own preview"
    );
}

/// Fonts flow through the registry like any other format; the specimen is a
/// synthetic TrueType built by the plugin's own test helper shape.
#[test]
fn font_specimens_render_and_search_sets_the_sample() {
    let tmp = tempfile::tempdir().unwrap();
    let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
    app.plugins
        .add(Arc::new(bibli_plugin_preview_fonts::PreviewPlugin), &app);
    let path = tmp.path().join("Synthetic face.ttf");
    std::fs::write(&path, b"not a font at all").unwrap();
    let request = request(&path);
    let error = app
        .previews
        .inspect(&request, &InspectRequest::default(), &CancelToken::new())
        .unwrap_err();
    assert!(!error.to_string().is_empty());
    let view = InspectRequest {
        query: Some("Sample".into()),
        ..InspectRequest::default()
    };
    assert!(
        app.previews
            .inspect(&request, &view, &CancelToken::new())
            .is_err(),
        "a malformed font is an error, never a silent fallback"
    );
}

/// An uncompressed RGB PNG built without an encoder dependency.
fn synthetic_png(width: u32, height: u32) -> Vec<u8> {
    fn crc(bytes: &[u8]) -> u32 {
        !bytes.iter().fold(u32::MAX, |crc, &b| {
            (0..8).fold(crc ^ u32::from(b), |c, _| {
                if c & 1 == 1 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                }
            })
        })
    }
    fn chunk(out: &mut Vec<u8>, kind: [u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc(&body).to_be_bytes());
    }
    let mut raw = Vec::new();
    for _ in 0..height {
        raw.push(0);
        raw.extend((0..width).flat_map(|_| [1u8, 2, 3]));
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in &raw {
        a = (a + u32::from(byte)) % 65521;
        b = (b + a) % 65521;
    }
    let mut idat = vec![0x78, 0x01, 0x01];
    idat.extend_from_slice(&(raw.len() as u16).to_le_bytes());
    idat.extend_from_slice(&(!(raw.len() as u16)).to_le_bytes());
    idat.extend_from_slice(&raw);
    idat.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut out, *b"IHDR", &ihdr);
    chunk(&mut out, *b"IDAT", &idat);
    chunk(&mut out, *b"IEND", &[]);
    out
}

fn archive_worker() -> Option<std::path::PathBuf> {
    if let Some(path) = std::env::var_os("BIBLI_ARCHIVE_WORKER") {
        return Some(path.into());
    }
    let built =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/bibli-archive-worker");
    built.is_file().then_some(built)
}

/// Needs the libarchive helper built beside the workspace target (`just
/// previews-check` builds it); skips with a note otherwise.
#[test]
fn archive_members_preview_as_image_text_or_paged_bytes() {
    use std::io::Write;
    let Some(worker) = archive_worker() else {
        eprintln!("skipped: bibli-archive-worker is not built");
        return;
    };
    // The helper path is process-wide; tests in this binary run in separate
    // processes under nextest, so the variable cannot leak into another test.
    // SAFETY: no other thread of this test process reads the environment.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("BIBLI_ARCHIVE_WORKER", &worker);
    }
    let tmp = tempfile::tempdir().unwrap();
    let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
    app.plugins
        .add(Arc::new(bibli_plugin_preview_archives::PreviewPlugin), &app);
    let path = tmp.path().join("Synthetic bundle's.zip");
    let png = synthetic_png(3, 2);
    let text = "caf\u{e9} member\n".repeat(20);
    let binary: Vec<u8> = (0..=255u8).cycle().take(5000).collect();
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    for (name, body) in [
        ("folder/cover.png", png.as_slice()),
        ("folder/notes.txt", text.as_bytes()),
        ("blob.bin", &binary),
        ("../outside.txt", b"a name, never a path"),
    ] {
        zip.start_file(
            name,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated),
        )
        .unwrap();
        zip.write_all(body).unwrap();
    }
    zip.finish().unwrap();
    let listing = inspect(&app, &path, &InspectRequest::default());
    assert_eq!(
        listing.kind,
        bibli_core::inspection::InspectionKind::Archive
    );
    let target = |name: &str| {
        let row = listing.rows.iter().find(|r| r.cells[0] == name).unwrap();
        InspectRequest {
            target: row.target.clone(),
            ..InspectRequest::default()
        }
    };
    // A member written out for the panel lands under our name in the
    // directory given, whatever its own name tries to say.
    let into = tmp.path().join("members");
    std::fs::create_dir(&into).unwrap();
    for (name, file) in [("cover.png", "member.png"), ("outside.txt", "member.txt")] {
        let member = app
            .previews
            .extract(
                &request(&path),
                target(name).target.as_deref().unwrap(),
                &into,
                &CancelToken::new(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            (member.name.as_str(), &member.path),
            (name, &into.join(file))
        );
    }
    assert_eq!(std::fs::read(into.join("member.png")).unwrap(), png);
    assert!(!tmp.path().join("outside.txt").exists());
    let image = inspect(&app, &path, &target("cover.png"));
    assert_eq!(image.image.map(|i| (i.width, i.height)), Some((3, 2)));
    let notes = inspect(&app, &path, &target("notes.txt"));
    assert_eq!(notes.plain_text(), text);
    let bytes = inspect(&app, &path, &target("blob.bin"));
    assert_eq!(bytes.kind, bibli_core::inspection::InspectionKind::Hex);
    assert_eq!(bytes.rows.len(), 256);
    assert_eq!(bytes.next, Some(4096));
    let second = inspect(
        &app,
        &path,
        &InspectRequest {
            offset: 4096,
            ..target("blob.bin")
        },
    );
    assert_eq!(second.position, 4096);
    assert_eq!(second.rows[0].cells[0], "0000000000001000");
    let outside = inspect(&app, &path, &target("outside.txt"));
    assert_eq!(outside.plain_text(), "a name, never a path");
    assert!(!tmp.path().parent().unwrap().join("outside.txt").exists());
}
