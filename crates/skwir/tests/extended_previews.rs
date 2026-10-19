//! Synthetic integration tests exercise provider selection and runtime withdrawal.
use skwir_core::{
    App, CancelToken,
    app::AppConfig,
    inspection::{BlockKind, InspectRequest, Inspection, InspectionKind},
    op::EntryRef,
    plugin::Plugin,
    preview::PreviewRequest,
};
use std::{path::Path, sync::Arc};

mod common;
use common::synthetic_png;
fn app(root: &Path) -> Arc<App> {
    let app = App::new(AppConfig::ephemeral(root.join("state"))).unwrap();
    for plugin in [
        Arc::new(skwir_plugin_markdown::PreviewPlugin) as Arc<dyn Plugin>,
        Arc::new(skwir_plugin_tables::PreviewPlugin),
        Arc::new(skwir_plugin_structured_data::PreviewPlugin),
        Arc::new(skwir_plugin_email::PreviewPlugin),
        Arc::new(skwir_plugin_epub::PreviewPlugin),
        Arc::new(skwir_plugin_hex::PreviewPlugin),
    ] {
        app.plugins.add(plugin, &app);
    }
    app
}
/// A request as the preview worker makes one: the type a fixture's name
/// says. Only bytes proven binary are `application/octet-stream`, which is
/// what the hexadecimal fallback takes, so a text file never says it.
/// A preview's request for `path`, typed as the preview worker types it
/// (`ui/src/preview.rs`): by its name, and its bytes where the name is
/// ambiguous.
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
    }
}
/// What the preview worker shows: the provider's inspection, else, for the
/// source view, the shared code view (`skwir_ui::preview::inspect_shown`,
/// the worker's own step).
fn try_inspect(app: &App, path: &Path, view: &InspectRequest) -> Option<Inspection> {
    skwir_ui::preview::inspect_shown(
        &app.previews,
        &request(path),
        &mut view.clone(),
        path,
        false,
        &CancelToken::new(),
    )
    .unwrap()
}
fn inspect(app: &App, path: &Path, view: &InspectRequest) -> Inspection {
    try_inspect(app, path, view).expect("a provider answers")
}
#[test]
fn asf_media_ownership_precedes_hex_fallback_but_allows_explicit_hex() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("Synthetic container.ASF");
    std::fs::write(&path, b"synthetic container marker").unwrap();
    let app = app(tmp.path());
    app.plugins
        .add(Arc::new(skwir_plugin_media::PreviewPlugin::default()), &app);
    // Windows Media by its name, though its bytes are no film.
    let request = request(&path);
    let default = InspectRequest::default();
    assert!(
        app.previews
            .inspect(&request, &default, &CancelToken::new())
            .unwrap()
            .is_none()
    );
    let explicit = InspectRequest {
        provider: Some("hex".into()),
        ..InspectRequest::default()
    };
    assert!(inspect(&app, &path, &explicit).title.starts_with("Hex"));
    assert!(app.plugins.set_enabled("media", false, &app));
    // Nothing else takes a film; its bytes are still there to ask for.
    assert!(try_inspect(&app, &path, &default).is_none());
    assert!(inspect(&app, &path, &explicit).title.starts_with("Hex"));
    assert!(app.plugins.set_enabled("media", true, &app));
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
    // Said as any text is: the lit Source says it is the source.
    assert_eq!(source.title, "12 lines");
    assert!(source.source_available, "and switches back");
    assert!(app.plugins.set_enabled("markdown", false, &app));
    // Text is never the byte view's: with Markdown off, nothing claims the
    // file and the worker shows it as the plain code view.
    assert!(try_inspect(&app, &path, &InspectRequest::default()).is_none());
    assert!(app.plugins.set_enabled("markdown", true, &app));
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
        assert_eq!(code.kind, skwir_core::inspection::InspectionKind::Code);
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
    assert!(invalid.notice.starts_with("Shown as written:"));
    assert!(!invalid.source_available, "nothing else to switch to");
    // Too long a line for the code view: the source, read where it is
    // shown, the line cut into bounded rows (ADR-051).
    let large = format!("\"{}\"", "a".repeat(40000));
    std::fs::write(&path, &large).unwrap();
    let source = inspect(&app, &path, &InspectRequest::default());
    assert!(source.notice.starts_with("Shown as written:"));
    assert!(source.text.as_ref().is_some_and(|file| file.whole));
    assert!(source.blocks.iter().all(|row| row.text.len() <= 4096));
    assert_eq!(
        source
            .blocks
            .iter()
            .map(|row| row.text.as_str())
            .collect::<String>(),
        large
    );
    assert_eq!((source.next, source.title.as_str()), (None, "1 line"));
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
        provider: Some("hex".into()),
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

    // The panel's way: the attachment becomes a file of its own, in the
    // directory it was given and under a name of ours, and is then previewed
    // as what it is, here by whichever provider reads text files.
    let into = tmp.path().join("members");
    std::fs::create_dir(&into).unwrap();
    let extracted = app
        .previews
        .extract(
            &request(&path),
            &InspectRequest::default(),
            "0",
            &into,
            &CancelToken::new(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(extracted.name, "sample.txt");
    assert_eq!(extracted.path, into.join("member.txt"));
    assert_eq!(std::fs::read(&extracted.path).unwrap(), b"hello");
    assert!(
        app.previews
            .extract(
                &request(&path),
                &InspectRequest::default(),
                "7",
                &into,
                &CancelToken::new()
            )
            .is_err(),
        "no such attachment"
    );
    // A provider whose targets are not files says so; nothing is written.
    let notes = tmp.path().join("notes.md");
    std::fs::write(&notes, "# Title\n").unwrap();
    assert!(
        app.previews
            .extract(
                &request(&notes),
                &InspectRequest::default(),
                "0",
                &into,
                &CancelToken::new()
            )
            .unwrap()
            .is_none()
    );
}
/// A book of two chapters, the first linking to the second.
fn synthetic_book(path: &Path) {
    use std::io::Write;
    let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
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
}
#[test]
fn epub_chapters_source_navigation_and_disable() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let path = tmp.path().join("Synthetic book.epub");
    synthetic_book(&path);
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

/// A type shown as its source (`SourceTypes`) opens on it wherever the file
/// has one: text in the shared code view, a book's chapter as its provider
/// writes it, though a book is no text. A file of the type that is no text
/// has none: the worker asks it again as it shows.
#[test]
fn a_type_shown_as_its_source_opens_on_it_where_the_file_has_one() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let remembered = |path: &Path| {
        let mut view = InspectRequest {
            source: true,
            ..InspectRequest::default()
        };
        let shown = skwir_ui::preview::inspect_shown(
            &app.previews,
            &request(path),
            &mut view,
            path,
            true,
            &CancelToken::new(),
        )
        .unwrap();
        (shown, view.source)
    };
    let notes = tmp.path().join("Synthetic note's draft.md");
    std::fs::write(&notes, "# Title\n\nSome **bold** text.\n").unwrap();
    let (shown, source) = remembered(&notes);
    assert!(source, "text has its source");
    assert!(shown.unwrap().plain_text().contains("**bold**"));
    let book = tmp.path().join("Synthetic book.epub");
    synthetic_book(&book);
    let (shown, source) = remembered(&book);
    assert!(source, "a book's provider writes its source");
    assert!(shown.unwrap().plain_text().contains("<h1>"));
    let bytes = tmp.path().join("Synthetic bytes.md");
    std::fs::write(&bytes, b"\x00\x01\x02 no text \xff\xfe\x00\x7f").unwrap();
    let (shown, source) = remembered(&bytes);
    assert!(shown.is_none() && !source, "bytes have no source");
    // Asked so, it is what it is without: never its bytes as text.
    assert_eq!(
        inspect(&app, &bytes, &InspectRequest::default()).title,
        "Markdown"
    );
    // Asked for by hand, the source is the file's text, whatever it holds.
    let asked = InspectRequest {
        source: true,
        ..InspectRequest::default()
    };
    assert!(try_inspect(&app, &bytes, &asked).unwrap().source_available);
}

#[test]
fn a_presentation_that_fails_on_text_shows_its_source_instead() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    // Too long to be read whole and laid out: its text, read where it is
    // shown, saying why, and nothing to switch back to.
    let page = tmp.path().join("Synthetic page's long copy.html");
    let paragraph = "<p>Some <b>bold</b> text, over and over.</p>\n";
    let large = paragraph.repeat(2 * 1024 * 1024 / paragraph.len() + 1);
    std::fs::write(&page, &large).unwrap();
    let shown = inspect(&app, &page, &InspectRequest::default());
    assert_eq!(shown.kind, InspectionKind::Code);
    assert_eq!(
        shown.notice,
        "Shown as written: Text exceeds 2 MiB preview limit"
    );
    assert!(!shown.source_available);
    assert!(shown.plain_text().starts_with("<p>Some <b>bold</b>"));
    assert!(shown.text.is_some(), "read where it is shown (ADR-051)");
    // Only its size kept it from being presented: Full preview presents it,
    // all of it.
    assert!(shown.whole_available);
    let whole = InspectRequest {
        whole: true,
        ..InspectRequest::default()
    };
    let all = inspect(&app, &page, &whole);
    assert_eq!(all.kind, InspectionKind::Markdown);
    assert_eq!(all.blocks.len(), large.len() / paragraph.len());
    assert!(!all.whole_available);
    // Its Source asked for: the same text, and still a way back.
    let asked = InspectRequest {
        source: true,
        ..InspectRequest::default()
    };
    let source = inspect(&app, &page, &asked);
    assert!(source.notice.is_empty() && source.source_available);
    // Bytes that are not text keep the failure: never shown as text.
    let bytes = tmp.path().join("Synthetic bytes.html");
    let mut binary = vec![0u8; 2 * 1024 * 1024 + 1];
    binary[..4].copy_from_slice(b"<p>\x00");
    std::fs::write(&bytes, &binary).unwrap();
    let error = skwir_ui::preview::inspect_shown(
        &app.previews,
        &request(&bytes),
        &mut InspectRequest::default(),
        &bytes,
        false,
        &CancelToken::new(),
    )
    .unwrap_err();
    assert_eq!(error.to_string(), "Text exceeds 2 MiB preview limit");
    assert!(
        skwir_core::inspection::TooLarge::is(&error),
        "the panel offers Full preview"
    );
    // Another failure is no size's: its text, and no Full preview.
    let failed = || anyhow::anyhow!("Synthetic failure");
    let instead = skwir_ui::preview::source_instead(
        &InspectRequest::default(),
        &page,
        failed(),
        &CancelToken::new(),
    )
    .unwrap();
    assert!(!instead.whole_available);
    // Nor when another way of looking was chosen, or the wait was given up.
    let hex = InspectRequest {
        provider: Some("hex".into()),
        ..InspectRequest::default()
    };
    assert!(skwir_ui::preview::source_instead(&hex, &page, failed(), &CancelToken::new()).is_err());
    let cancelled = CancelToken::new();
    cancelled.cancel();
    assert!(
        skwir_ui::preview::source_instead(&InspectRequest::default(), &page, failed(), &cancelled)
            .is_err()
    );
}

/// Every preview plugin the binary wires, in the order it wires them.
fn preview_plugins() -> Vec<Arc<dyn Plugin>> {
    skwir::plugins()
        .into_iter()
        .filter(|p| p.category() == skwir_core::plugin::Category::Previews)
        .collect()
}

/// Providers of one priority are ordered by registration, so no two of
/// them may accept the same file: one that settles a shared name by its
/// content outranks one that takes the name whole (a Litchi mission's
/// `.csv` before the table reader). Fallbacks (below 0) take what nothing
/// else does.
#[test]
fn format_providers_never_compete_for_the_same_file() {
    let tmp = tempfile::tempdir().unwrap();
    let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
    for plugin in preview_plugins() {
        app.plugins.add(plugin, &app);
    }
    let actions: Vec<String> = (1..=15)
        .map(|i| format!("actiontype{i},actionparam{i}"))
        .collect();
    let mission = format!(
        "latitude,longitude,altitude(m),heading(deg),curvesize(m),rotationdir,gimbalmode,\
         gimbalpitchangle,{},altitudemode,speed(m/s),poi_latitude,poi_longitude,poi_altitude(m),\
         poi_altitudemode,photo_timeinterval,photo_distinterval\r\n",
        actions.join(",")
    );
    let fixtures: Vec<(&str, Option<&str>, &[u8])> = vec![
        ("notes.md", Some("text/markdown"), b"synthetic"),
        ("table.csv", Some("text/csv"), b"synthetic"),
        ("mission.csv", Some("text/csv"), mission.as_bytes()),
        ("table.tsv", Some("text/tab-separated-values"), b"synthetic"),
        ("data.json", Some("application/json"), b"synthetic"),
        ("data.yaml", Some("application/yaml"), b"synthetic"),
        ("data.yml", None, b"synthetic"),
        ("data.toml", Some("application/toml"), b"synthetic"),
        ("message.eml", Some("message/rfc822"), b"synthetic"),
        ("book.epub", Some("application/epub+zip"), b"synthetic"),
        ("face.ttf", Some("font/ttf"), b"synthetic"),
        ("face.otf", Some("font/otf"), b"synthetic"),
        ("bundle.zip", Some("application/zip"), b"synthetic"),
        ("bundle.tar.gz", Some("application/gzip"), b"synthetic"),
        (
            "bundle.7z",
            Some("application/x-7z-compressed"),
            b"synthetic",
        ),
        (
            "pages.cbz",
            Some("application/vnd.comicbook+zip"),
            b"synthetic",
        ),
        ("pages.cbr", None, b"synthetic"),
        ("clip.asf", Some("application/vnd.ms-asf"), b"synthetic"),
        ("clip.mp4", Some("video/mp4"), b"synthetic"),
        ("song.mp3", Some("audio/mpeg"), b"synthetic"),
        ("mix.m3u", Some("audio/x-mpegurl"), b"synthetic"),
        ("mix.m3u8", None, b"synthetic"),
        ("mix.pls", Some("audio/x-scpls"), b"synthetic"),
        ("mix.xspf", Some("application/xspf+xml"), b"synthetic"),
        ("album.cue", Some("application/x-cue"), b"synthetic"),
        ("paper.pdf", Some("application/pdf"), b"synthetic"),
        (
            "text.odt",
            Some("application/vnd.oasis.opendocument.text"),
            b"synthetic",
        ),
        ("blob.bin", Some("application/octet-stream"), b"synthetic"),
        ("photo.jpg", Some("image/jpeg"), b"synthetic"),
    ];
    let mut shown: Vec<(&str, Option<String>)> = Vec::new();
    for (name, mime, bytes) in &fixtures {
        let path = tmp.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        let request = PreviewRequest {
            entry: EntryRef::from_path(&path).unwrap(),
            mime: mime.map(str::to_owned),
            page: 1,
            seconds: None,
            max_side: 64,
            text_size: None,
            fit: None,
        };
        let accepting = app.previews.accepting(&request);
        for (i, (owner, id, priority)) in accepting.iter().enumerate() {
            let tied = accepting[i + 1..]
                .iter()
                .find(|(_, _, other)| other == priority && *priority >= 0);
            assert!(
                tied.is_none(),
                "{name}: {owner}'s {id} and {tied:?} accept it at one priority, {priority}"
            );
        }
        shown.push((name, accepting.first().map(|(owner, _, _)| owner.clone())));
    }
    let owner = |name: &str| shown.iter().find(|(n, _)| *n == name).unwrap().1.clone();
    assert_eq!(owner("notes.md").as_deref(), Some("markdown"));
    assert_eq!(owner("table.csv").as_deref(), Some("tables"));
    assert_eq!(owner("mission.csv").as_deref(), Some("geo"));
    assert_eq!(owner("data.yml").as_deref(), Some("structured-data"));
    assert_eq!(owner("bundle.tar.gz").as_deref(), Some("archives"));
    assert_eq!(owner("pages.cbr").as_deref(), Some("comics"));
    assert_eq!(owner("clip.asf").as_deref(), Some("media"));
    assert_eq!(owner("song.mp3").as_deref(), Some("media"));
    assert_eq!(owner("paper.pdf").as_deref(), Some("documents"));
    assert_eq!(
        owner("blob.bin").as_deref(),
        Some("hex"),
        "only the fallback owns generic bytes"
    );
    assert_eq!(
        owner("photo.jpg"),
        None,
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
        .add(Arc::new(skwir_plugin_fonts::PreviewPlugin), &app);
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
/// The libarchive helper: `SKWIR_ARCHIVE_WORKER`, else the one built into
/// the target directory this test runs from (wherever `CARGO_TARGET_DIR`
/// put it: the test binary sits in its `deps`).
fn archive_worker() -> Option<std::path::PathBuf> {
    if let Some(path) = std::env::var_os("SKWIR_ARCHIVE_WORKER") {
        return Some(path.into());
    }
    let exe = std::env::current_exe().ok()?;
    let built = exe.parent()?.parent()?.join("skwir-archive-worker");
    built.is_file().then_some(built)
}

/// Needs the libarchive helper (`just workers` builds it). Without it the
/// test says so and passes, unless `SKWIR_REQUIRE_WORKERS` is set, as it
/// should be wherever the workers are built: there a missing helper is a
/// failure, not a skip.
fn with_archive_worker() -> bool {
    let Some(worker) = archive_worker() else {
        assert!(
            std::env::var_os("SKWIR_REQUIRE_WORKERS").is_none(),
            "skwir-archive-worker is not built"
        );
        eprintln!("skipped: skwir-archive-worker is not built");
        return false;
    };
    // The helper path is process-wide; tests in this binary run in separate
    // processes under nextest, so the variable cannot leak into another test.
    // SAFETY: no other thread of this test process reads the environment.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("SKWIR_ARCHIVE_WORKER", &worker);
    }
    true
}

fn archives_app(root: &Path) -> Arc<App> {
    let app = App::new(AppConfig::ephemeral(root.join("state"))).unwrap();
    app.plugins
        .add(Arc::new(skwir_plugin_archives::PreviewPlugin), &app);
    app
}

fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
    use std::io::Write;
    let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    for (name, body) in entries {
        zip.start_file(
            *name,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated),
        )
        .unwrap();
        zip.write_all(body).unwrap();
    }
    zip.finish().unwrap();
}

/// A member is written out and previewed as the file it is; the archive
/// provider only lists.
#[test]
fn archive_members_are_written_out_under_our_names() {
    if !with_archive_worker() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let app = archives_app(tmp.path());
    let path = tmp.path().join("Synthetic bundle's.zip");
    let png = synthetic_png(3, 2);
    let text = "caf\u{e9} member\n".repeat(20);
    let binary: Vec<u8> = (0..=255u8).cycle().take(5000).collect();
    write_zip(
        &path,
        &[
            ("folder/cover.png", png.as_slice()),
            ("folder/notes.txt", text.as_bytes()),
            ("blob.bin", &binary),
            ("../outside.txt", b"a name, never a path"),
            // libarchive converts names through the locale's charset: in the C
            // locale a name like this one fails the whole listing.
            (
                "Cr\u{e8}me br\u{fb}l\u{e9}e/r\u{e9}sum\u{e9} \u{e9}t\u{e9}.txt",
                "caf\u{e9}".as_bytes(),
            ),
        ],
    );
    let listing = inspect(&app, &path, &InspectRequest::default());
    assert_eq!(
        listing.kind,
        skwir_core::inspection::InspectionKind::Archive
    );
    assert!(listing.notice.is_empty(), "{}", listing.notice);
    assert!(
        listing
            .rows
            .iter()
            .any(|r| r.cells[0] == "Cr\u{e8}me br\u{fb}l\u{e9}e" && r.branch)
    );
    // A member written out for the panel lands under our name in the
    // directory given, whatever its own name tries to say.
    let into = tmp.path().join("members");
    std::fs::create_dir(&into).unwrap();
    let accented = "r\u{e9}sum\u{e9} \u{e9}t\u{e9}.txt";
    for (name, file, body) in [
        ("cover.png", "member.png", png.as_slice()),
        ("notes.txt", "member.txt", text.as_bytes()),
        ("blob.bin", "member.bin", &binary),
        ("outside.txt", "member.txt", b"a name, never a path"),
        (accented, "member.txt", "caf\u{e9}".as_bytes()),
    ] {
        let row = listing.rows.iter().find(|r| r.cells[0] == name).unwrap();
        let member = app
            .previews
            .extract(
                &request(&path),
                &InspectRequest::default(),
                row.target.as_deref().unwrap(),
                &into,
                &CancelToken::new(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            (member.name.as_str(), &member.path),
            (name, &into.join(file))
        );
        assert_eq!(std::fs::read(&member.path).unwrap(), body);
    }
    assert!(!tmp.path().join("outside.txt").exists());
    assert!(!tmp.path().parent().unwrap().join("outside.txt").exists());
}

/// A file that is only compressed is an archive of one member, named after
/// it; a listing past its bound shows the start and says so.
#[test]
fn compressed_files_and_long_archives_are_listed() {
    use std::io::Write;
    if !with_archive_worker() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let app = archives_app(tmp.path());
    let log = tmp.path().join("access.log.1.gz");
    let mut gzip = flate2::write::GzEncoder::new(
        std::fs::File::create(&log).unwrap(),
        flate2::Compression::default(),
    );
    gzip.write_all(b"GET / 200\n").unwrap();
    gzip.finish().unwrap();
    let listing = inspect(&app, &log, &InspectRequest::default());
    assert_eq!(listing.rows.len(), 1);
    assert_eq!(listing.rows[0].cells[0], "access.log.1");
    let into = tmp.path().join("members");
    std::fs::create_dir(&into).unwrap();
    let member = app
        .previews
        .extract(
            &request(&log),
            &InspectRequest::default(),
            listing.rows[0].target.as_deref().unwrap(),
            &into,
            &CancelToken::new(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(std::fs::read(&member.path).unwrap(), b"GET / 200\n");

    let many = tmp.path().join("Synthetic many.zip");
    let names: Vec<String> = (0..=skwir_platform::inspection::limits::CONTAINER_MEMBERS)
        .map(|i| format!("f{i}.txt"))
        .collect();
    let entries: Vec<(&str, &[u8])> = names.iter().map(|n| (n.as_str(), &b""[..])).collect();
    write_zip(&many, &entries);
    let listing = inspect(&app, &many, &InspectRequest::default());
    assert_eq!(
        listing.rows.len(),
        skwir_platform::inspection::limits::CONTAINER_MEMBERS
    );
    assert!(
        listing.notice.starts_with("First 4000 members"),
        "{}",
        listing.notice
    );
}

/// A comic is a document of pages in the reader: in the order its names
/// read, each decoded at the side asked and never enlarged, its own size
/// what 100% is.
#[test]
fn a_comic_is_a_document_of_pages() {
    if !with_archive_worker() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
    app.plugins
        .add(Arc::new(skwir_plugin_comics::PreviewPlugin), &app);
    let path = tmp.path().join("Synthetic comic's pages.cbz");
    // Within one stored deflate block, as `synthetic_png` writes them.
    let (big, small) = (synthetic_png(120, 180), synthetic_png(30, 40));
    write_zip(
        &path,
        &[
            ("pages/10.png", big.as_slice()),
            ("pages/2.png", small.as_slice()),
            ("pages/1.png", big.as_slice()),
            ("pages/notes.txt", b"not a page"),
        ],
    );
    let cancel = CancelToken::new();
    let request = request(&path);
    assert!(
        app.previews
            .inspect(&request, &InspectRequest::default(), &cancel)
            .unwrap()
            .is_none(),
        "a comic is not an inspection"
    );
    let comic = app
        .previews
        .open_document(&request, &cancel)
        .unwrap()
        .expect("a comic of pages");
    assert_eq!(comic.info().pages, 3);
    let page = comic.render(1, 90, &cancel).unwrap();
    assert_eq!((page.width, page.height), (60, 90));
    assert_eq!((page.content.width, page.content.height), (120., 180.));
    assert_eq!(page.pixels.len(), 60 * 90 * 4);
    let second = comic.render(2, 300, &cancel).unwrap();
    assert_eq!((second.width, second.height), (30, 40), "never enlarged");
    assert_eq!(comic.render(3, 4096, &cancel).unwrap().height, 180);
    assert!(comic.search("page", &cancel).unwrap().is_empty());
}

/// A comic's cover is its first page in reading order: numbers compare as
/// numbers, and what a Mac left beside the pages is no page.
#[test]
fn a_comic_cover_is_its_first_page() {
    use skwir_core::scheduler::Priority;
    use skwir_core::thumbs::{ThumbRequest, ThumbSize, ThumbState};
    if !with_archive_worker() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
    app.plugins
        .add(Arc::new(skwir_plugin_comics::PreviewPlugin), &app);
    let path = tmp.path().join("Synthetic issue n\u{b0} 2.cbz");
    let (first, later) = (synthetic_png(4, 6), synthetic_png(6, 4));
    write_zip(
        &path,
        &[
            ("__MACOSX/._page 1.png", b"not a picture"),
            ("page 10.png", later.as_slice()),
            ("page 2.png", first.as_slice()),
        ],
    );
    let req = ThumbRequest {
        entry: EntryRef::from_path(&path).unwrap(),
        mime: None,
        mtime: None,
        size: ThumbSize::Normal,
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let cover = loop {
        match app.thumbnails.get(req.clone(), Priority::Visible) {
            ThumbState::Ready(image) => break image,
            ThumbState::Unavailable => panic!("no cover"),
            ThumbState::Pending => {
                assert!(std::time::Instant::now() < deadline, "timed out");
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
    };
    assert_eq!((cover.width, cover.height), (4, 6), "page 2, not page 10");
}
