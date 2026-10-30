use super::*;
use crate::{Cover, Engine, Provider};
use skwir_core::op::EntryRef;
use skwir_core::preview::HeldSlot;
use skwir_core::{
    inspection::{InspectRequest, Inspection},
    preview::{PreviewProvider, PreviewRequest},
    thumbs::{ThumbRequest, ThumbnailProvider},
};
use std::sync::Arc;

/// A member the spine names twice is found at its first place, as
/// reading the spine for it found it; a link however spelt finds its
/// member, and one to no member (or no key) finds nothing.
#[test]
fn a_member_is_found_at_its_first_place_in_the_spine() {
    let spine = [
        "Text/one.xhtml",
        "Text/two.xhtml",
        "Text/one.xhtml",
        "Text/caf%C3%A9%20three.xhtml",
    ];
    let keys: Vec<_> = spine.iter().map(|href| member_key(href)).collect();
    let sections = Sections::of(keys.iter().cloned());
    let read = |href: &str| {
        let key = member_key(href);
        keys.iter().position(|k| k.is_some() && *k == key)
    };
    for href in [
        "Text/one.xhtml",
        "Text/../Text/one.xhtml#part",
        "Text/two.xhtml",
        "Text/café three.xhtml",
        "Text/four.xhtml",
    ] {
        assert_eq!(
            sections.of_key(member_key(href).as_deref()),
            read(href),
            "{href}"
        );
    }
    assert_eq!(
        sections.of_key(member_key("Text/one.xhtml").as_deref()),
        Some(0)
    );
    assert_eq!(sections.of_key(None), None);
}

const CONTAINER_XML: &str = r#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="EPUB/book.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#;
const OPF: &str = r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">urn:example:synthetic</dc:identifier><dc:title>Synthetic book</dc:title><dc:language>en</dc:language></metadata><manifest><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/><item id="one" href="one.xhtml" media-type="application/xhtml+xml"/><item id="two" href="two.xhtml" media-type="application/xhtml+xml"/><item id="three" href="sub/caf%C3%A9%20three.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="one"/><itemref idref="two"/><itemref idref="three"/></spine></package>"#;
const NAV: &str = r#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="toc"><ol><li><a href="one.xhtml">Start</a><ol><li><a href="two.xhtml">Middle</a></li></ol></li><li><a href="sub/caf%C3%A9%20three.xhtml">End</a></li></ol></nav></body></html>"#;

/// A 2 × 3 picture.
const PNG: &[u8] = b"\x89\x50\x4e\x47\x0d\x0a\x1a\x0a\x00\x00\x00\x0d\x49\x48\x44\x52\x00\x00\x00\x02\x00\x00\x00\x03\x08\x06\x00\x00\x00\xb9\xea\xde\x81\x00\x00\x00\x11\x49\x44\x41\x54\x78\x9c\x63\xf8\xcf\xc0\xf0\x1f\x84\x19\x30\x18\x00\xa1\x79\x0b\xf5\x4d\xc4\x9a\x07\x00\x00\x00\x00\x49\x45\x4e\x44\xae\x42\x60\x82";

fn write(path: &std::path::Path, entries: &[(&str, &[u8])]) {
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

fn book(path: &std::path::Path) {
    write(
        path,
        &[
            ("mimetype", b"application/epub+zip"),
            ("META-INF/container.xml", CONTAINER_XML.as_bytes()),
            ("EPUB/book.opf", OPF.as_bytes()),
            ("EPUB/nav.xhtml", NAV.as_bytes()),
            (
                "EPUB/one.xhtml",
                br"<html><body><h1>First</h1><p>The needle is here.</p></body></html>",
            ),
            (
                "EPUB/two.xhtml",
                br"<html><body><h1>Second</h1><p>Nothing to see.</p></body></html>",
            ),
            (
                "EPUB/sub/café three.xhtml",
                br"<html><body><h1>Third</h1><p>Another needle.</p></body></html>",
            ),
        ],
    );
}

fn inspect(path: &std::path::Path, view: &InspectRequest) -> anyhow::Result<Inspection> {
    let request = PreviewRequest {
        entry: EntryRef::from_path(path).unwrap(),
        mime: None,
        page: 1,
        seconds: None,
        max_side: 64,
        text_size: None,
        fit: None,
        held: HeldSlot::default(),
    };
    Provider::default()
        .inspect(&request, view, &CancelToken::new())
        .map(Option::unwrap)
}

#[test]
fn each_engine_answers_through_its_own_surface() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("Synthetic book.epub");
    book(&path);
    let request = PreviewRequest {
        entry: EntryRef::from_path(&path).unwrap(),
        mime: None,
        page: 1,
        seconds: None,
        max_side: 64,
        text_size: Some(20.),
        fit: None,
        held: HeldSlot::default(),
    };
    let cancel = CancelToken::new();
    // The built-in engine inspects; it opens no document.
    let builtin = Provider::default();
    assert!(builtin.open_document(&request, &cancel).unwrap().is_none());
    assert!(
        builtin
            .inspect(&request, &InspectRequest::default(), &cancel)
            .unwrap()
            .is_some()
    );
    // MuPDF's books are documents: the reader opens them, never inspect.
    let mupdf = Provider {
        engine: Engine::Mupdf,
        ..Provider::default()
    };
    assert!(
        mupdf
            .inspect(&request, &InspectRequest::default(), &cancel)
            .unwrap()
            .is_none()
    );
}

#[test]
fn contents_and_encoded_names_resolve_through_the_bounded_container() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("Synthetic book.epub");
    book(&path);
    let first = inspect(&path, &InspectRequest::default()).unwrap();
    assert_eq!(
        first
            .rows
            .iter()
            .map(|r| (r.cells[0].as_str(), r.depth, r.section))
            .collect::<Vec<_>>(),
        // Each knows the chapter it opens, its name escaped included.
        [
            ("Start", 0, Some(0)),
            ("Middle", 1, Some(1)),
            ("End", 0, Some(2))
        ]
    );
    // What the package says about the book reaches the panel's Properties;
    // a field the book leaves blank (no author here) is not a row.
    assert_eq!(
        first
            .details
            .iter()
            .map(|detail| (detail.label.as_str(), detail.value.as_str()))
            .collect::<Vec<_>>(),
        [
            ("Title", "Synthetic book"),
            ("Language", "en"),
            ("Chapters", "3")
        ]
    );
    let third = inspect(
        &path,
        &InspectRequest {
            target: first.rows[2].target.clone(),
            ..InspectRequest::default()
        },
    )
    .unwrap();
    assert_eq!(third.position, 2);
    assert!(third.plain_text().contains("Another needle"));
}

/// A part's heading, which leads nowhere, is in the contents and its
/// chapters under it; an entry with neither a link nor a name nests
/// nothing; the contents' own heading is not an entry.
#[test]
fn a_heading_over_chapters_holds_them() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("Synthetic book in parts.epub");
    let nav = r#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="toc"><h1>Contents</h1><ol>
<li><span>Part one</span><ol><li><a href="one.xhtml">Start</a><ol><li><a href="one.xhtml#s">Section</a></li></ol></li></ol></li>
<li><a>Part two</a><ol><li><a href="two.xhtml">Middle</a></li></ol></li>
<li><ol><li><a href="sub/caf%C3%A9%20three.xhtml">End</a></li></ol></li>
</ol></nav></body></html>"#;
    write(
        &path,
        &[
            ("mimetype", b"application/epub+zip"),
            ("META-INF/container.xml", CONTAINER_XML.as_bytes()),
            ("EPUB/book.opf", OPF.as_bytes()),
            ("EPUB/nav.xhtml", nav.as_bytes()),
            (
                "EPUB/one.xhtml",
                br#"<html><body><p>One.</p><h2 id="s">Section</h2></body></html>"#,
            ),
            ("EPUB/two.xhtml", br"<html><body><p>Two.</p></body></html>"),
            (
                "EPUB/sub/café three.xhtml",
                br"<html><body><p>Three.</p></body></html>",
            ),
        ],
    );
    let first = inspect(&path, &InspectRequest::default()).unwrap();
    assert_eq!(
        first
            .rows
            .iter()
            .map(|r| (r.cells[0].as_str(), r.depth, r.target.is_some(), r.section))
            .collect::<Vec<_>>(),
        [
            ("Part one", 0, false, None),
            ("Start", 1, true, Some(0)),
            ("Section", 2, true, Some(0)),
            ("Part two", 0, false, None),
            ("Middle", 1, true, Some(1)),
            ("End", 0, true, Some(2)),
        ]
    );
}

/// Contents are the table of contents: not the landmarks, not the list
/// of printed pages, not the NCX's second copy of the chapters.
#[test]
fn contents_are_the_table_of_contents_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("Synthetic paginated book.epub");
    let opf = OPF
            .replace(
                r#"<item id="nav""#,
                r#"<item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/><item id="nav""#,
            )
            .replace("<spine>", r#"<spine toc="ncx">"#);
    let nav = r#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body>
<nav epub:type="toc"><ol><li><a href="one.xhtml">Start</a></li><li><a href="two.xhtml">Middle</a></li></ol></nav>
<nav epub:type="landmarks"><ol><li><a epub:type="bodymatter" href="one.xhtml">Begin reading</a></li></ol></nav>
<nav epub:type="page-list"><ol><li><a href="one.xhtml#p1">1</a></li><li><a href="two.xhtml#p2">2</a></li></ol></nav>
</body></html>"#;
    let ncx = r#"<?xml version="1.0"?><ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1"><head/><docTitle><text>Synthetic</text></docTitle>
<navMap><navPoint id="n1" playOrder="1"><navLabel><text>Start</text></navLabel><content src="one.xhtml"/></navPoint><navPoint id="n2" playOrder="2"><navLabel><text>Middle</text></navLabel><content src="two.xhtml"/></navPoint></navMap>
<pageList><pageTarget id="p1" type="normal" value="1" playOrder="3"><navLabel><text>1</text></navLabel><content src="one.xhtml#p1"/></pageTarget></pageList></ncx>"#;
    write(
        &path,
        &[
            ("mimetype", b"application/epub+zip"),
            ("META-INF/container.xml", CONTAINER_XML.as_bytes()),
            ("EPUB/book.opf", opf.as_bytes()),
            ("EPUB/nav.xhtml", nav.as_bytes()),
            ("EPUB/toc.ncx", ncx.as_bytes()),
            ("EPUB/one.xhtml", br"<html><body><p>One.</p></body></html>"),
            ("EPUB/two.xhtml", br"<html><body><p>Two.</p></body></html>"),
        ],
    );
    let first = inspect(&path, &InspectRequest::default()).unwrap();
    let titles: Vec<_> = first.rows.iter().map(|r| r.cells[0].as_str()).collect();
    assert_eq!(titles, ["Start", "Middle"]);
    // The printed pages are the page list, each with its chapter.
    let pages = |inspection: &Inspection| {
        inspection
            .printed_pages
            .iter()
            .map(|p| (p.label.clone(), p.section, p.target.clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        pages(&first),
        [
            ("1".to_owned(), 0, "/EPUB/one.xhtml#p1".to_owned()),
            ("2".to_owned(), 1, "/EPUB/two.xhtml#p2".to_owned())
        ]
    );
    // A book with only the EPUB 2 navigation reads its NCX `navMap`.
    let nav_free = opf.replace(
        r#"<item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>"#,
        "",
    );
    write(
        &path,
        &[
            ("mimetype", b"application/epub+zip"),
            ("META-INF/container.xml", CONTAINER_XML.as_bytes()),
            ("EPUB/book.opf", nav_free.as_bytes()),
            ("EPUB/toc.ncx", ncx.as_bytes()),
            ("EPUB/one.xhtml", br"<html><body><p>One.</p></body></html>"),
            ("EPUB/two.xhtml", br"<html><body><p>Two.</p></body></html>"),
        ],
    );
    let first = inspect(&path, &InspectRequest::default()).unwrap();
    let titles: Vec<_> = first.rows.iter().map(|r| r.cells[0].as_str()).collect();
    assert_eq!(titles, ["Start", "Middle"]);
    // ... and its `pageList`.
    assert_eq!(
        pages(&first),
        [("1".to_owned(), 0, "/EPUB/one.xhtml#p1".to_owned())]
    );
}

#[test]
fn repeated_search_advances_to_the_next_chapter_and_wraps() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("Synthetic book.epub");
    book(&path);
    let search = |offset: u64| {
        inspect(
            &path,
            &InspectRequest {
                query: Some("NEEDLE".into()),
                offset,
                ..InspectRequest::default()
            },
        )
        .unwrap()
    };
    let first = search(0);
    assert_eq!(first.position, 0);
    assert_eq!(first.notice, "Search match in this chapter");
    let second = search(first.position + 1);
    assert_eq!(second.position, 2, "chapter two has no match");
    let wrapped = search(second.position + 1);
    assert_eq!(wrapped.position, 0);
    let none = inspect(
        &path,
        &InspectRequest {
            query: Some("absent".into()),
            offset: 1,
            ..InspectRequest::default()
        },
    )
    .unwrap();
    assert_eq!(none.notice, "No matching chapter");
    assert_eq!(none.position, 1);
}

/// A container entry whose deflate stream inflates far beyond the size its
/// headers declare must fail on the bytes produced, before rbook parses it.
#[test]
fn declared_sizes_do_not_bound_decompression() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("Synthetic bomb.epub");
    let inflated = vec![0u8; limits::BINARY_INPUT + 1024];
    write(
        &path,
        &[
            ("META-INF/container.xml", &inflated),
            ("EPUB/book.opf", OPF.as_bytes()),
        ],
    );
    // Declare a tiny uncompressed size in the local header and central directory.
    let mut bytes = std::fs::read(&path).unwrap();
    let lying = 100u32.to_le_bytes();
    let local = bytes.windows(4).position(|w| w == b"PK\x03\x04").unwrap();
    bytes[local + 22..local + 26].copy_from_slice(&lying);
    let central = bytes.windows(4).position(|w| w == b"PK\x01\x02").unwrap();
    bytes[central + 24..central + 28].copy_from_slice(&lying);
    std::fs::write(&path, &bytes).unwrap();
    let mut archive = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
    assert_eq!(
        archive.by_index_raw(0).unwrap().size(),
        100,
        "the lie is in place"
    );
    // zip 9 refuses a member past its declared size; were it to trust the
    // lie further, the bound on what is read would stop it.
    let error = inspect(&path, &InspectRequest::default())
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("larger than its declared uncompressed size")
            || error.contains("EPUB container exceeds 16 MiB"),
        "{error}"
    );
}

/// The layout Sigil writes: package in `OEBPS/`, chapters in `Text/`,
/// linked to each other through `../Text/`.
fn sigil(path: &std::path::Path, extra: &[(&str, &[u8])]) {
    let opf = OPF
        .replace(
            "nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"",
            "Text/nav.xhtml\" media-type=\"application/xhtml+xml\"",
        )
        .replace("href=\"one.xhtml\"", "href=\"Text/one.xhtml\"")
        .replace("href=\"two.xhtml\"", "href=\"Text/two.xhtml\"")
        .replace(
            "href=\"sub/caf%C3%A9%20three.xhtml\"",
            "href=\"Text/three.xhtml\"",
        );
    let container = CONTAINER_XML.replace("EPUB/book.opf", "OEBPS/content.opf");
    let mut entries: Vec<(&str, &[u8])> = vec![
            ("mimetype", b"application/epub+zip"),
            ("META-INF/container.xml", container.as_bytes()),
            ("OEBPS/content.opf", opf.as_bytes()),
            (
                "OEBPS/Text/one.xhtml",
                br#"<html><body><h1>First</h1><p><a href="../Text/two.xhtml#note5">Note</a></p></body></html>"#,
            ),
            (
                "OEBPS/Text/two.xhtml",
                br#"<html><body><h1>Second</h1><p id="note5">The note.</p></body></html>"#,
            ),
            (
                "OEBPS/Text/three.xhtml",
                b"<html><body><h1>Th\xe9</h1><p>caf\xe9</p></body></html>",
            ),
        ];
    // An extra entry replaces the chapter of the same name.
    entries.retain(|(name, _)| !extra.iter().any(|(extra, _)| extra == name));
    entries.extend_from_slice(extra);
    write(path, &entries);
}

#[test]
fn a_link_through_parent_folders_lands_on_its_chapter() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("Synthetic sigil book.epub");
    sigil(&path, &[]);
    let first = inspect(&path, &InspectRequest::default()).unwrap();
    let link = first
        .blocks
        .iter()
        .flat_map(|b| &b.runs)
        .find_map(|r| r.link.clone())
        .unwrap();
    assert!(link.ends_with("../Text/two.xhtml#note5"), "{link}");
    let linked = inspect(
        &path,
        &InspectRequest {
            target: Some(link),
            ..InspectRequest::default()
        },
    )
    .unwrap();
    assert_eq!(linked.position, 1);
    assert_eq!(linked.title, "EPUB · chapter 2 / 3");
    assert_eq!((linked.previous, linked.next), (Some(0), Some(2)));
    assert!(linked.plain_text().contains("The note."));
    // A chapter in a legacy encoding reads, and says how.
    let third = inspect(
        &path,
        &InspectRequest {
            offset: 2,
            ..InspectRequest::default()
        },
    )
    .unwrap();
    assert!(third.plain_text().contains("caf\u{e9}"));
    assert!(
        third.notice.contains("Read as Windows-1252"),
        "{}",
        third.notice
    );
}

#[test]
fn resources_never_read_do_not_refuse_the_book() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("Synthetic illustrated book.epub");
    let video = vec![0u8; limits::BINARY_INPUT + 1024];
    sigil(&path, &[("OEBPS/Video/film.mp4", &video)]);
    let first = inspect(&path, &InspectRequest::default()).unwrap();
    assert!(first.plain_text().contains("First"));
}

#[test]
fn a_chapter_source_is_the_code_view_and_search_passes_over_bad_chapters() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("Synthetic book.epub");
    book(&path);
    let source = inspect(
        &path,
        &InspectRequest {
            source: true,
            offset: 1,
            ..InspectRequest::default()
        },
    )
    .unwrap();
    assert_eq!(source.kind, skwir_core::inspection::InspectionKind::Code);
    assert!(source.blocks.iter().all(|b| b.code));
    assert_eq!(source.blocks[0].language.as_deref(), Some("xhtml"));
    assert_eq!(source.position, 1);
    assert!(source.source_available);

    // The spine names a chapter the container lacks: the search reads on.
    let broken = tmp.path().join("Synthetic broken book.epub");
    let opf = OPF.replace("href=\"two.xhtml\"", "href=\"absent.xhtml\"");
    write(
        &broken,
        &[
            ("META-INF/container.xml", CONTAINER_XML.as_bytes()),
            ("EPUB/book.opf", opf.as_bytes()),
            ("EPUB/nav.xhtml", NAV.as_bytes()),
            (
                "EPUB/one.xhtml",
                b"<html><body><p>Nothing.</p></body></html>",
            ),
            (
                "EPUB/sub/caf\u{e9} three.xhtml",
                b"<html><body><p>The needle.</p></body></html>",
            ),
        ],
    );
    let found = inspect(
        &broken,
        &InspectRequest {
            query: Some("needle".into()),
            ..InspectRequest::default()
        },
    )
    .unwrap();
    assert_eq!(found.position, 2);
}

/// A chapter reads with the book's own stylesheet, found inside the
/// container through the chapter's relative link, and as XML: a
/// `<title/>` closes itself instead of swallowing the chapter.
#[test]
fn a_chapter_reads_with_its_stylesheet_and_as_xhtml() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("Synthetic styled book.epub");
    let chapter = br#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><head><title/><link rel="stylesheet" type="text/css" href="../Styles/style.css"/></head>
<body><p class="c">CHAPTER ONE</p><p>She said <span class="i">never</span>.</p><p class="kf8">Kindle only needle</p></body></html>"#;
    sigil(
        &path,
        &[
            (
                "OEBPS/Styles/style.css",
                b".i { font-style: italic } .c { text-align: center } .kf8 { display: none }",
            ),
            ("OEBPS/Text/one.xhtml", chapter),
        ],
    );
    let first = inspect(&path, &InspectRequest::default()).unwrap();
    let texts: Vec<_> = first.blocks.iter().map(|b| b.text.as_str()).collect();
    assert_eq!(texts, ["CHAPTER ONE", "She said never."]);
    assert_eq!(first.blocks[0].align, skwir_core::inspection::Align::Center);
    let never = &first.blocks[1].runs[1];
    assert!(never.italic && first.blocks[1].text[never.range.clone()] == *"never");
    // What the book hides is not a match either.
    let search = inspect(
        &path,
        &InspectRequest {
            query: Some("needle".into()),
            ..InspectRequest::default()
        },
    )
    .unwrap();
    assert_eq!(search.notice, "No matching chapter");
}

/// A fixed-layout book whose pages are pictures (a comic) shows a page
/// as a page: the picture, the width of the panel.
fn fixed(path: &std::path::Path, pages: &[(&str, &[u8])]) {
    fixed_with(path, NAV, pages);
}

fn fixed_with(path: &std::path::Path, nav: &str, pages: &[(&str, &[u8])]) {
    let opf = OPF.replace(
        "<dc:language>en</dc:language>",
        r#"<dc:language>en</dc:language><meta property="rendition:layout">pre-paginated</meta>"#,
    );
    let mut entries: Vec<(&str, &[u8])> = vec![
        ("mimetype", b"application/epub+zip"),
        ("META-INF/container.xml", CONTAINER_XML.as_bytes()),
        ("EPUB/book.opf", opf.as_bytes()),
        ("EPUB/nav.xhtml", nav.as_bytes()),
        ("EPUB/images/p1.png", PNG),
    ];
    entries.extend_from_slice(pages);
    write(path, &entries);
}

fn session(path: &std::path::Path) -> Arc<dyn skwir_core::document::DocumentSession> {
    let request = PreviewRequest {
        entry: EntryRef::from_path(path).unwrap(),
        mime: None,
        page: 1,
        seconds: None,
        max_side: 64,
        text_size: None,
        fit: None,
        held: HeldSlot::default(),
    };
    let provider = Provider::default();
    let cancel = CancelToken::new();
    assert!(
        provider
            .inspect(&request, &InspectRequest::default(), &cancel)
            .unwrap()
            .is_none(),
        "a book of pages is not read as chapters"
    );
    provider
        .open_document(&request, &cancel)
        .unwrap()
        .expect("a book of pages")
}

/// A chapter names its pictures, as many as it has, with the size they
/// are shown at; each is decoded only when asked for by its member.
#[test]
fn pictures_are_named_for_later_and_decoded_by_their_member() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("Synthetic illustrated book.epub");
    let count = limits::EMBEDDED_IMAGES + 4;
    let chapter = (0..count).fold(String::from("<html><body>"), |mut html, i| {
        use std::fmt::Write;
        let _ = write!(
            html,
            r#"<p>Plate {i}</p><img src="../Images/plate%20{i}.png" alt="Plate"/>"#
        );
        html
    }) + "</body></html>";
    let names: Vec<String> = (0..count)
        .map(|i| format!("OEBPS/Images/plate {i}.png"))
        .collect();
    let mut extra: Vec<(&str, &[u8])> = vec![("OEBPS/Text/one.xhtml", chapter.as_bytes())];
    extra.extend(names.iter().map(|name| (name.as_str(), PNG)));
    sigil(&path, &extra);
    let first = inspect(&path, &InspectRequest::default()).unwrap();
    let later: Vec<_> = first
        .blocks
        .iter()
        .filter_map(|b| match &b.picture {
            Some(Picture::Later { key, width, height }) => Some((key.clone(), *width, *height)),
            _ => None,
        })
        .collect();
    assert_eq!(later.len(), count, "no picture is left out");
    assert_eq!(later[3], ("OEBPS/Images/plate 3.png".to_owned(), 2, 3));
    let request = PreviewRequest {
        entry: EntryRef::from_path(&path).unwrap(),
        mime: None,
        page: 1,
        seconds: None,
        max_side: 64,
        text_size: None,
        fit: None,
        held: HeldSlot::default(),
    };
    let cancel = CancelToken::new();
    let view = InspectRequest::default();
    let decoded = Provider::default()
        .picture(&request, &view, &later[3].0, &cancel)
        .unwrap()
        .unwrap();
    assert_eq!((decoded.width, decoded.height), (2, 3));
    assert!(
        Provider::default()
            .picture(&request, &view, "../outside.png", &cancel)
            .is_err(),
        "a key is a member, never a path out of the container"
    );
}

/// A font is restored whatever opened the book: a fixed-layout page's
/// drawing and a search open the container afresh, without the
/// package being read first, and found no identifier to key it by.
#[test]
fn an_obfuscated_font_is_restored_in_a_container_opened_afresh() {
    let font: Vec<u8> = (0..1500u32).map(|i| (i % 241) as u8).collect();
    let mut stored = font.clone();
    deobfuscate(&mut stored, IDPF_OBFUSCATION, "urn:example:synthetic").unwrap();
    let encryption = format!(
        r#"<?xml version="1.0"?><encryption xmlns="urn:oasis:names:tc:opendocument:xmlns:container" xmlns:enc="http://www.w3.org/2001/04/xmlenc#"><enc:EncryptedData><enc:EncryptionMethod Algorithm="{IDPF_OBFUSCATION}"/><enc:CipherData><enc:CipherReference URI="EPUB/fonts/Libre%20Été.otf"/></enc:CipherData></enc:EncryptedData></encryption>"#
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fonts.epub");
    write(
        &path,
        &[
            ("mimetype", b"application/epub+zip"),
            ("META-INF/container.xml", CONTAINER_XML.as_bytes()),
            ("META-INF/encryption.xml", encryption.as_bytes()),
            ("EPUB/book.opf", OPF.as_bytes()),
            ("EPUB/fonts/Libre Été.otf", &stored),
        ],
    );
    let cancel = CancelToken::new();
    let mut container = Container::open(&path, &cancel).unwrap();
    assert_eq!(
        container.file("EPUB/fonts/Libre%20Été.otf", &cancel),
        Some(font)
    );
}

/// A protected book's chapter says it is encrypted, rather than showing
/// its bytes; its obfuscated font is still read.
#[test]
fn an_encrypted_chapter_says_so() {
    let encryption = format!(
        r#"<?xml version="1.0"?><encryption xmlns="urn:oasis:names:tc:opendocument:xmlns:container" xmlns:enc="http://www.w3.org/2001/04/xmlenc#"><enc:EncryptedData><enc:EncryptionMethod Algorithm="http://www.w3.org/2001/04/xmlenc#aes128-cbc"/><enc:CipherData><enc:CipherReference URI="EPUB/one.xhtml"/></enc:CipherData></enc:EncryptedData><enc:EncryptedData><enc:EncryptionMethod Algorithm="{IDPF_OBFUSCATION}"/><enc:CipherData><enc:CipherReference URI="EPUB/a.otf"/></enc:CipherData></enc:EncryptedData></encryption>"#
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("protected.epub");
    write(
        &path,
        &[
            ("mimetype", b"application/epub+zip"),
            ("META-INF/container.xml", CONTAINER_XML.as_bytes()),
            ("META-INF/encryption.xml", encryption.as_bytes()),
            ("EPUB/book.opf", OPF.as_bytes()),
            ("EPUB/one.xhtml", b"\x8f\x02 ciphered bytes"),
            ("EPUB/a.otf", b"font"),
        ],
    );
    let cancel = CancelToken::new();
    let mut container = Container::open(&path, &cancel).unwrap();
    let error = container
        .read("EPUB/one.xhtml", "Chapter", &cancel)
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "This EPUB is protected: its chapter is encrypted"
    );
    assert!(container.read("EPUB/a.otf", "Font", &cancel).is_ok());
}

/// Obfuscation is its own inverse, keyed by the package's identifier:
/// IDPF's by its SHA-1 over 1040 bytes, Adobe's by its UUID over 1024.
#[test]
fn obfuscated_fonts_are_restored() {
    let font: Vec<u8> = (0..2000u32).map(|i| (i % 251) as u8).collect();
    for (algorithm, identifier, length) in [
        (IDPF_OBFUSCATION, " urn:isbn:9780000000000 \n", 1040),
        (
            ADOBE_OBFUSCATION,
            "urn:uuid:0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0",
            1024,
        ),
    ] {
        let mut bytes = font.clone();
        deobfuscate(&mut bytes, algorithm, identifier).unwrap();
        assert_ne!(bytes[..length], font[..length]);
        assert_eq!(bytes[length..], font[length..], "only the head is keyed");
        deobfuscate(&mut bytes, algorithm, identifier).unwrap();
        assert_eq!(bytes, font);
    }
    let mut drm = font.clone();
    assert!(deobfuscate(&mut drm, "http://www.w3.org/2001/04/xmlenc#aes128-cbc", "x").is_none());
}

/// A fixed-layout book is a document of pages: each drawn at the side
/// the reader asks, its own size what 100% is, its contents leading to
/// its pages, its text found by page.
#[test]
fn a_fixed_layout_book_is_a_document_of_pages() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("Synthetic drawn book.epub");
    let page = br#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><head><title/><meta name="viewport" content="width=300,height=400"/>
<style>body { margin: 0 } img { position: absolute; left: 0; top: 0; width: 300px; height: 400px }
.t { position: absolute; left: 20px; top: 20px; color: #123456 }</style></head>
<body><img src="images/p1.png" alt=""/><div class="t">Lettering</div></body></html>"#;
    fixed(
        &path,
        &[
            ("EPUB/one.xhtml", page),
            (
                "EPUB/two.xhtml",
                br"<html><body><p>No size stated.</p></body></html>",
            ),
        ],
    );
    let book = session(&path);
    let cancel = CancelToken::new();
    assert_eq!(book.info().pages, 3);
    assert_eq!(
        book.info()
            .outlines
            .iter()
            .map(|o| (o.title.as_str(), o.depth, o.page))
            .collect::<Vec<_>>(),
        [
            ("Start", 0, Some(1)),
            ("Middle", 1, Some(2)),
            ("End", 0, Some(3))
        ]
    );
    for side in [800, 1600] {
        let drawn = book.render(1, side, &cancel).unwrap();
        assert_eq!(drawn.height, side, "drawn at the side asked");
        assert_eq!(drawn.width * 4, side * 3);
        assert_eq!(drawn.pixels.len() as u32, drawn.width * drawn.height * 4);
        assert_eq!((drawn.content.width, drawn.content.height), (300., 400.));
        assert!(
            drawn.content.chars.is_empty(),
            "a drawn page has no text layer"
        );
    }
    // A page that states no size is drawn at the book's.
    let second = book.render(2, 800, &cancel).unwrap();
    assert_eq!((second.content.width, second.content.height), (300., 400.));
    // The third page is missing from the archive: an error of its own.
    assert!(book.render(3, 800, &cancel).is_err());
    let pages = |query| {
        book.search(query, &cancel)
            .unwrap()
            .into_iter()
            .map(|hit| hit.page)
            .collect::<Vec<_>>()
    };
    assert_eq!(pages("LETTERING"), [1]);
    assert_eq!(pages("no size"), [2]);
    assert!(pages("absent").is_empty());
}

/// A book of pages names them as its page list does: a page without a
/// printed number has none.
#[test]
fn a_fixed_layout_book_names_its_pages_as_printed() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("Synthetic numbered book.epub");
    let nav = NAV.replace(
            "</body>",
            r#"<nav epub:type="page-list"><ol><li><a href="one.xhtml">i</a></li><li><a href="two.xhtml">ii</a></li></ol></nav></body>"#,
        );
    let page = br#"<html><head><meta name="viewport" content="width=30,height=40"/></head><body><p>Page</p></body></html>"#;
    fixed_with(
        &path,
        &nav,
        &[("EPUB/one.xhtml", page), ("EPUB/two.xhtml", page)],
    );
    assert_eq!(session(&path).info().labels, ["i", "ii", ""]);
}

/// A page that states no size and holds one picture (a comic's page) is
/// that picture, at its own size.
#[test]
fn a_fixed_layout_picture_page_is_its_picture() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("Synthetic comic book.epub");
    fixed(
        &path,
        &[(
            "EPUB/one.xhtml",
            br#"<html><body><img src="images/p1.png" alt=""/></body></html>"#,
        )],
    );
    let page = session(&path).render(1, 800, &CancelToken::new()).unwrap();
    assert_eq!((page.content.width, page.content.height), (2., 3.));
    assert_eq!(
        (page.width, page.height),
        (2, 3),
        "a picture is never enlarged"
    );
}

/// A preview reads its file once (ADR-101): a book's chapters, its
/// pictures and the pages drawn later come from what the request
/// holds, named where no file is, so a read of the path would fail.
#[test]
fn a_book_and_its_pages_come_from_what_is_held() {
    let tmp = tempfile::tempdir().unwrap();
    let held = |written: &std::path::Path| {
        PreviewRequest::holding(
            tmp.path().join("gone").join("l’été book.epub"),
            std::fs::read(written).unwrap(),
            Some("application/epub+zip"),
        )
    };
    let cancel = CancelToken::new();
    let view = InspectRequest::default();
    let provider = Provider::default();
    let written = tmp.path().join("Synthetic book.epub");
    book(&written);
    let request = held(&written);
    assert!(provider.accepts(&request));
    let first = provider.inspect(&request, &view, &cancel).unwrap().unwrap();
    assert_eq!(first.title, "EPUB · chapter 1 / 3");
    assert!(first.blocks.iter().any(|b| b.text == "First"));
    let written = tmp.path().join("Synthetic drawn book.epub");
    fixed(
        &written,
        &[(
            "EPUB/one.xhtml",
            br#"<html><body><img src="images/p1.png" alt=""/><p>Lettering</p></body></html>"#,
        )],
    );
    let request = held(&written);
    let pages = provider
        .open_document(&request, &cancel)
        .unwrap()
        .expect("a book of pages");
    assert_eq!(pages.info().pages, 3);
    assert!(pages.render(1, 800, &cancel).is_ok());
    let found: Vec<_> = pages
        .search("lettering", &cancel)
        .unwrap()
        .into_iter()
        .map(|hit| hit.page)
        .collect();
    assert_eq!(found, [1]);
    let picture = provider
        .picture(&request, &view, "EPUB/images/p1.png", &cancel)
        .unwrap()
        .unwrap();
    assert_eq!((picture.width, picture.height), (2, 3));
}

#[test]
fn member_paths_never_escape_the_container_root() {
    assert_eq!(
        normalize("/EPUB/./a/../b.xhtml"),
        Some("EPUB/b.xhtml".into())
    );
    assert_eq!(normalize("EPUB//x.xhtml"), Some("EPUB/x.xhtml".into()));
    assert_eq!(normalize("../outside.xhtml"), None);
    assert_eq!(normalize("EPUB/../../x"), None);
    assert_eq!(normalize("/"), None);
}

/// A book whose package lists `manifest` (with a decoy picture first)
/// and `metadata`, with the pictures it names, and `extra` members.
fn covered(path: &std::path::Path, metadata: &str, manifest: &str, extra: &[(&str, &[u8])]) {
    const DECOY: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="30" height="10"/>"#;
    let opf = OPF
            .replace(
                "<dc:language>en</dc:language>",
                &format!("<dc:language>en</dc:language>{metadata}"),
            )
            .replace(
                "<manifest>",
                &format!(
                    r#"<manifest><item id="decoy" href="Images/decoy.svg" media-type="image/svg+xml"/>{manifest}"#
                ),
            );
    let mut entries: Vec<(&str, &[u8])> = vec![
        ("mimetype", b"application/epub+zip"),
        ("META-INF/container.xml", CONTAINER_XML.as_bytes()),
        ("EPUB/book.opf", opf.as_bytes()),
        ("EPUB/nav.xhtml", NAV.as_bytes()),
        ("EPUB/one.xhtml", b"<html><body><p>One</p></body></html>"),
        ("EPUB/Images/decoy.svg", DECOY),
        ("EPUB/Images/c\u{f4}ver art.png", PNG),
    ];
    entries.extend_from_slice(extra);
    write(path, &entries);
}

fn cover_of(path: &std::path::Path) -> Option<(u32, u32)> {
    let req = ThumbRequest {
        entry: EntryRef::from_path(path).unwrap(),
        mime: None,
        mtime: None,
        size: skwir_core::thumbs::ThumbSize::Normal,
    };
    assert!(Cover.accepts(&req));
    Cover
        .thumbnail(&req, &CancelToken::new())
        .unwrap()
        .map(|image| (image.width, image.height))
}

#[test]
fn a_cover_is_the_picture_the_package_names() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("Synthetic \u{2019}book\u{2019}.epub");
    let cover = r#"<item id="art" href="Images/c%C3%B4ver%20art.png" media-type="image/png""#;
    // EPUB 3: the manifest item whose properties say so.
    covered(
        &path,
        "",
        &format!(r#"{cover} properties="cover-image"/>"#),
        &[],
    );
    assert_eq!(cover_of(&path), Some((2, 3)));
    // EPUB 2: the item `<meta name="cover">` names, by id or by href.
    covered(
        &path,
        r#"<meta name="cover" content="art"/>"#,
        &format!("{cover}/>"),
        &[],
    );
    assert_eq!(cover_of(&path), Some((2, 3)));
    covered(
        &path,
        r#"<meta name="cover" content="Images/c%C3%B4ver%20art.png"/>"#,
        &format!("{cover}/>"),
        &[],
    );
    assert_eq!(cover_of(&path), Some((2, 3)));
    // Nothing named: no guess among the book's pictures.
    covered(&path, "", &format!("{cover}/>"), &[]);
    assert_eq!(cover_of(&path), None);
    // A cover page is a chapter, not a picture.
    covered(
        &path,
        "",
        r#"<item id="page" href="one.xhtml" media-type="application/xhtml+xml" properties="cover-image"/>"#,
        &[],
    );
    assert_eq!(cover_of(&path), None);
}

#[test]
fn an_encrypted_cover_is_not_shown() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("locked.epub");
    let encryption = br#"<encryption xmlns="urn:oasis:names:tc:opendocument:xmlns:container" xmlns:enc="http://www.w3.org/2001/04/xmlenc#"><enc:EncryptedData><enc:EncryptionMethod Algorithm="http://www.w3.org/2001/04/xmlenc#aes128-cbc"/><enc:CipherData><enc:CipherReference URI="EPUB/Images/c%C3%B4ver%20art.png"/></enc:CipherData></enc:EncryptedData></encryption>"#;
    covered(
        &path,
        "",
        r#"<item id="art" href="Images/c%C3%B4ver%20art.png" media-type="image/png" properties="cover-image"/>"#,
        &[("META-INF/encryption.xml", encryption)],
    );
    assert_eq!(cover_of(&path), None);
}
