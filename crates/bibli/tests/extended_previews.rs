//! Synthetic integration tests exercise provider selection and runtime withdrawal.
use bibli_core::{
    App, CancelToken,
    app::AppConfig,
    inspection::{InspectRequest, Inspection},
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
        "# Heading\n\nText with **bold** and [link](https://example.invalid).\n\n- [x] Checked\n",
    )
    .unwrap();
    let app = app(tmp.path());
    let rendered = inspect(&app, &path, &InspectRequest::default());
    assert_eq!(rendered.title, "Markdown");
    assert!(rendered.blocks.iter().any(|b| b.heading == 1));
    assert!(
        rendered
            .blocks
            .iter()
            .any(|b| b.runs.iter().any(|r| r.bold))
    );
    assert!(rendered.plain_text().contains('☑'));
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
    assert_eq!(found.position, 4080);
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
