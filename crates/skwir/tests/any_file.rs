//! Every preview and thumbnail skwir makes in its own process, given
//! files no application writes: a sample of each type it reads, mutated
//! (bytes changed, characters of several bytes put in, cut short), asked
//! of every provider as the preview panel and the views ask. A provider
//! may refuse a file or say it is damaged; none panics, on the caller's
//! thread or on a worker's.

use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use proptest::prelude::*;
use skwir_core::{
    App, CancelToken,
    app::AppConfig,
    inspection::InspectRequest,
    op::EntryRef,
    plugin::{Category, Plugin},
    preview::PreviewRequest,
    scheduler::Priority,
    testing::mutated,
    thumbs::{ThumbRequest, ThumbSize, ThumbState},
};

mod common;

/// The plugins that read files in skwir's process to show them.
fn app(root: &Path) -> Arc<App> {
    let app = App::new(AppConfig::ephemeral(root.join("state"))).unwrap();
    for plugin in skwir::plugins() {
        let reads = plugin.category() == Category::Previews
            || ["thumbnails", "geo", "xmp"].contains(&plugin.id());
        if reads {
            app.plugins.add(plugin as Arc<dyn Plugin>, &app);
        }
    }
    app
}

fn zip(members: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut out);
    for (name, bytes) in members {
        let stored = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file(*name, stored).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
    out.into_inner()
}

const XHTML: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>One</title></head>
<body><h1>Caf&#233;</h1><p>Text <em>and</em> <a href="two.xhtml">a link</a>.</p>
<img src="cover.png" alt="cover"/></body></html>"#;

fn epub() -> Vec<u8> {
    zip(&[
        ("mimetype", b"application/epub+zip"),
        (
            "META-INF/container.xml",
            br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
        ),
        (
            "OEBPS/content.opf",
            br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Book</dc:title><dc:creator>Someone</dc:creator></metadata><manifest><item id="one" href="one.xhtml" media-type="application/xhtml+xml"/><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/></manifest><spine><itemref idref="one"/></spine></package>"#,
        ),
        ("OEBPS/one.xhtml", XHTML),
        (
            "OEBPS/nav.xhtml",
            br#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="toc"><ol><li><a href="one.xhtml">One</a></li></ol></nav></body></html>"#,
        ),
    ])
}

fn docx() -> Vec<u8> {
    zip(&[
        (
            "[Content_Types].xml",
            br#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#,
        ),
        (
            "word/document.xml",
            br#"<?xml version="1.0"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Title</w:t></w:r></w:p><w:p><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">Bold </w:t></w:r><w:r><w:t>caf&#233;</w:t></w:r></w:p><w:tbl><w:tr><w:tc><w:p><w:r><w:t>a</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:body></w:document>"#,
        ),
    ])
}

fn odt() -> Vec<u8> {
    zip(&[
        ("mimetype", b"application/vnd.oasis.opendocument.text"),
        (
            "content.xml",
            br#"<?xml version="1.0"?><office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0"><office:body><office:text><text:h text:outline-level="1">Title</text:h><text:p>Some <text:span>text</text:span><text:s text:c="3"/>caf&#233;</text:p><text:list><text:list-item><text:p>one</text:p></text:list-item></text:list><table:table><table:table-row><table:table-cell><text:p>a</text:p></table:table-cell></table:table-row></table:table></office:text></office:body></office:document-content>"#,
        ),
    ])
}

fn binary_stl() -> Vec<u8> {
    let mut out = vec![0u8; 80];
    out.extend(1u32.to_le_bytes());
    for value in [0f32, 0., 1., 0., 0., 0., 1., 0., 0., 0., 1., 0.] {
        out.extend(value.to_le_bytes());
    }
    out.extend([0, 0]);
    out
}

/// A sample of each type read in skwir's process, by its name.
fn samples() -> Vec<(&'static str, Vec<u8>)> {
    let text = |s: &str| s.as_bytes().to_vec();
    vec![
        (
            "notes.md",
            text(
                "# Title\n\nSome *emphasis*, `code` and a [link](https://example.org).\n\n- one\n- [ ] two\n\n```rust\nfn main() {}\n```\n\n| a | b |\n|---|:-:|\n| 1 | 2 |\n\n> quote\n",
            ),
        ),
        (
            "table.csv",
            text("name,size,\"quoted, field\"\ncafé,12,\"a\nb\"\n,,\n"),
        ),
        ("table.tsv", text("a\tb\tc\n1\t2\n")),
        (
            "lines.jsonl",
            text("{\"a\":1,\"b\":[1,2]}\n{\"a\":\"x\"}\nnot json\n"),
        ),
        (
            "data.json",
            text("{\"a\":[1,2,{\"b\":null}],\"c\":\"caf\\u00e9\",\"d\":1e400}"),
        ),
        (
            "data.yaml",
            text("a: 1\nb:\n  - x\n  - y: z\nc: &anchor {d: [1, 2]}\ne: *anchor\n"),
        ),
        (
            "data.toml",
            text("[a]\nb = 1\nc = [\"x\", 2026-03-12T08:00:00Z]\n[[d]]\ne = 'f'\n"),
        ),
        (
            "data.xml",
            text("<?xml version=\"1.0\"?><a b=\"c\"><d>e &amp; f</d><!-- x --><g/></a>"),
        ),
        (
            "page.html",
            text(
                "<!doctype html><html><head><style>p{color:red}</style></head><body><h1>T</h1><p>One <b>two</b><table><tr><td>a</td></tr></table></body></html>",
            ),
        ),
        (
            "mail.eml",
            text(
                "From: A <a@example.invalid>\r\nTo: B <b@example.invalid>\r\nSubject: =?UTF-8?Q?caf=C3=A9?=\r\nDate: Thu, 12 Mar 2026 08:00:00 +0100\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"b\"\r\n\r\n--b\r\nContent-Type: text/html; charset=utf-8\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\n<p>Hello =C3=A9</p>\r\n--b\r\nContent-Type: application/octet-stream; name=\"x.bin\"\r\nContent-Transfer-Encoding: base64\r\n\r\nAAEC\r\n--b--\r\n",
            ),
        ),
        ("book.epub", epub()),
        (
            "book.fb2",
            include_bytes!("../../plugins/epub/testdata/synthetic.fb2").to_vec(),
        ),
        (
            "book.fb2.zip",
            include_bytes!("../../plugins/epub/testdata/synthetic.fb2.zip").to_vec(),
        ),
        ("letter.docx", docx()),
        ("letter.odt", odt()),
        (
            "note.ipynb",
            text(
                r##"{"cells":[{"cell_type":"markdown","source":["# T\n","text"],"metadata":{}},{"cell_type":"code","source":"print(1)","outputs":[{"output_type":"stream","text":["1\n"]},{"output_type":"display_data","data":{"image/png":"iVBORw0KGgo="}}],"metadata":{},"execution_count":1}],"metadata":{"language_info":{"name":"python"}},"nbformat":4,"nbformat_minor":5}"##,
            ),
        ),
        (
            "board.gbr",
            text(
                "%FSLAX26Y26*%\n%MOMM*%\n%ADD10C,0.1*%\n%ADD11R,1X0.5*%\nD10*\nX0Y0D02*\nX1000000Y0D01*\nG75*\nG03X0Y1000000I-1000000J0D01*\nG36*\nX0Y0D02*\nX100Y100D01*\nX0Y100D01*\nG37*\nD11*\nX5Y5D03*\nM02*\n",
            ),
        ),
        (
            "holes.drl",
            text(
                "M48\nMETRIC,TZ\nT1C0.8\nT2C1.2\n%\nG90\nT1\nX1.0Y2.0\nT2\nX3.5Y-1.0\nG85X5.0Y5.0\nM30\n",
            ),
        ),
        (
            "board.kicad_pcb",
            text(
                "(kicad_pcb (version 20221018) (generator pcbnew) (general (thickness 1.6)) (layers (0 \"F.Cu\" signal) (31 \"B.Cu\" signal)) (gr_line (start 0 0) (end 10 10) (layer \"F.Cu\") (width 0.2)) (footprint \"R\" (at 5 5 90) (pad \"1\" smd rect (at 0 0) (size 1 1) (layers \"F.Cu\"))))",
            ),
        ),
        (
            "part.stl",
            text(
                "solid part\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid part\n",
            ),
        ),
        ("part-binary.stl", binary_stl()),
        (
            "part.obj",
            text("# part\nv 0 0 0\nv 1 0 0\nv 0 1 0\nvn 0 0 1\nf 1//1 2//1 3//1\nf 1 2 3 4\n"),
        ),
        (
            "part.ply",
            text(
                "ply\nformat ascii 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n0 0 0\n1 0 0\n0 1 0\n3 0 1 2\n",
            ),
        ),
        (
            "part.off",
            text("OFF\n3 1 0\n0 0 0\n1 0 0\n0 1 0\n3 0 1 2\n"),
        ),
        (
            "print.gcode",
            text(
                "; generated by a slicer\nG21\nG90\nM82\nG1 X0 Y0 Z0.2 F1200\nG1 X10 Y0 E1\nG2 X10 Y10 I0 J5 E2\nG1 Z0.4\n;LAYER:1\nG1 X0 Y10 E3\n",
            ),
        ),
        (
            "plan.dxf",
            text(
                "0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n10\n0.0\n20\n0.0\n11\n10.0\n21\n10.0\n0\nCIRCLE\n8\n0\n10\n5.0\n20\n5.0\n40\n2.0\n0\nENDSEC\n0\nEOF\n",
            ),
        ),
        (
            "list.m3u",
            text(
                "#EXTM3U\n#EXTINF:123,Artist - Title\nsong.mp3\n#EXTINF:-1,Radio\nhttps://example.org/x.mp3\n../other/café.flac\n",
            ),
        ),
        (
            "list.pls",
            text(
                "[playlist]\nFile1=song.mp3\nTitle1=Song\nLength1=123\nNumberOfEntries=1\nVersion=2\n",
            ),
        ),
        (
            "list.xspf",
            text(
                r#"<?xml version="1.0"?><playlist version="1" xmlns="http://xspf.org/ns/0/"><trackList><track><location>file:///a/b.mp3</location><title>T</title><duration>1000</duration></track></trackList></playlist>"#,
            ),
        ),
        (
            "album.cue",
            text(
                "PERFORMER \"Someone\"\nTITLE \"Album\"\nFILE \"a.flac\" WAVE\n  TRACK 01 AUDIO\n    TITLE \"One\"\n    INDEX 01 00:00:00\n  TRACK 02 AUDIO\n    INDEX 01 03:10:20\n",
            ),
        ),
        (
            "font.woff2",
            include_bytes!("../../plugins/fonts/src/testdata/synthetic.woff2").to_vec(),
        ),
        (
            "comic.cbz",
            zip(&[
                ("01.png", &common::synthetic_png(4, 6)),
                ("02.png", &common::synthetic_png(6, 4)),
            ]),
        ),
        ("picture.png", common::synthetic_png(8, 6)),
        (
            "drawing.svg",
            text(
                "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10\" height=\"10\" viewBox=\"0 0 10 10\"><rect x=\"1\" y=\"1\" width=\"8\" height=\"8\" fill=\"red\"/><text x=\"0\" y=\"5\">café</text></svg>",
            ),
        ),
        (
            "track.gpx",
            text(
                "<?xml version=\"1.0\"?><gpx version=\"1.1\" creator=\"x\" xmlns=\"http://www.topografix.com/GPX/1/1\"><trk><name>T</name><trkseg><trkpt lat=\"45.0\" lon=\"6.0\"><ele>1500</ele><time>2026-03-12T08:00:00Z</time></trkpt><trkpt lat=\"45.01\" lon=\"6.01\"/></trkseg></trk></gpx>",
            ),
        ),
    ]
}

/// A request as the preview panel makes one: the type the name says, and
/// the bytes where the name is ambiguous.
fn request(path: &Path) -> PreviewRequest {
    let mime = skwir_platform::mime::identify(path.file_name().unwrap(), path);
    PreviewRequest {
        entry: EntryRef::from_path(path).unwrap(),
        mime: mime.map(|mime| mime.0),
        page: 1,
        seconds: None,
        max_side: 256,
        text_size: None,
        fit: None,
        held: skwir_core::preview::HeldSlot::default(),
    }
}

/// Everything shown of `path` in skwir's process, as the preview panel
/// and the views ask it.
fn show(app: &App, path: &Path) {
    let cancel = CancelToken::new();
    let req = request(path);
    let _ = app.previews.render(&req, &cancel);
    let _ = app
        .previews
        .inspect(&req, &InspectRequest::default(), &cancel);
    let source = InspectRequest {
        source: true,
        ..InspectRequest::default()
    };
    let _ = app.previews.inspect(&req, &source, &cancel);
    let search = InspectRequest {
        query: Some("é".into()),
        ..InspectRequest::default()
    };
    let _ = app.previews.inspect(&req, &search, &cancel);
    if let Ok(Some(document)) = app.previews.open_document(&req, &cancel) {
        let _ = document.render(1, 128, &cancel);
        let _ = document.search("é", &cancel);
    }
    let thumb = ThumbRequest {
        entry: req.entry.clone(),
        mime: req.mime.as_deref().map(Arc::from),
        mtime: std::fs::metadata(path).and_then(|m| m.modified()).ok(),
        size: ThumbSize::Normal,
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    while matches!(
        app.thumbnails.get(thumb.clone(), Priority::Visible),
        ThumbState::Pending
    ) && Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// What the scheduler's workers caught: a panic there is a thumbnail or a
/// field that failed unseen.
fn panics(app: &App) -> Vec<(&'static str, u64)> {
    app.scheduler
        .stats()
        .into_iter()
        .filter(|(_, stats)| stats.panics > 0)
        .map(|(label, stats)| (label, stats.panics))
        .collect()
}

#[test]
fn every_sample_is_shown_as_it_is() {
    let tmp = common::home_tempdir();
    let app = app(tmp.path());
    for (name, bytes) in samples() {
        let path = tmp.path().join(name);
        std::fs::write(&path, &bytes).unwrap();
        show(&app, &path);
    }
    assert_eq!(panics(&app), [], "no job panicked");
}

/// Each case shows a file every way skwir does, a thumbnail included: a
/// few of them in every run, more when `PROPTEST_CASES` asks.
fn cases() -> u32 {
    std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|cases| cases.parse().ok())
        .unwrap_or(128)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases()))]
    #[test]
    fn every_sample_mutated_is_shown_without_a_panic(
        (index, bytes) in (0..samples().len()).prop_flat_map(|index| {
            (Just(index), mutated(samples().swap_remove(index).1))
        })
    ) {
        thread_local! {
            static HERE: (tempfile::TempDir, Arc<App>) = {
                let tmp = common::home_tempdir();
                let app = app(tmp.path());
                (tmp, app)
            };
        }
        HERE.with(|(tmp, app)| {
            use std::sync::atomic::{AtomicU64, Ordering};
            static CASE: AtomicU64 = AtomicU64::new(0);
            let name = samples().swap_remove(index).0;
            // A file of its own each time: nothing answered from a cache.
            let case = CASE.fetch_add(1, Ordering::Relaxed);
            let path = tmp.path().join(format!("{case}-{name}"));
            std::fs::write(&path, &bytes).unwrap();
            show(app, &path);
            let _ = std::fs::remove_file(&path);
            prop_assert_eq!(panics(app), Vec::<(&str, u64)>::new(), "{}", name);
            Ok(())
        })?;
    }
}
