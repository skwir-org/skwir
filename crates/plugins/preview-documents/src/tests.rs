//! Synthetic Word and `OpenDocument` files, written by the tests themselves.
use super::*;
use bibli_core::inspection::{Align, Block, BlockKind, Picture, Script};
use bibli_core::op::EntryRef;
use std::fmt::Write as _;
use std::io::Write;

/// A 2 × 3 picture.
const PNG: &[u8] = b"\x89\x50\x4e\x47\x0d\x0a\x1a\x0a\x00\x00\x00\x0d\x49\x48\x44\x52\x00\x00\x00\x02\x00\x00\x00\x03\x08\x06\x00\x00\x00\xb9\xea\xde\x81\x00\x00\x00\x11\x49\x44\x41\x54\x78\x9c\x63\xf8\xcf\xc0\xf0\x1f\x84\x19\x30\x18\x00\xa1\x79\x0b\xf5\x4d\xc4\x9a\x07\x00\x00\x00\x00\x49\x45\x4e\x44\xae\x42\x60\x82";

fn zip(path: &Path, entries: &[(&str, &[u8])]) {
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

const W_NS: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture" xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006""#;
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

fn package_rels(thumbnail: bool) -> String {
    format!(
        r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="r1" Type="{REL}/officeDocument" Target="word/document.xml"/><Relationship Id="r2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/>{}</Relationships>"#,
        if thumbnail {
            r#"<Relationship Id="r3" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/thumbnail" Target="docProps/thumbnail.png"/>"#
        } else {
            ""
        }
    )
}

fn document_rels() -> String {
    format!(
        r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdStyles" Type="{REL}/styles" Target="styles.xml"/><Relationship Id="rIdNumbering" Type="{REL}/numbering" Target="numbering.xml"/><Relationship Id="rIdNotes" Type="{REL}/footnotes" Target="footnotes.xml"/><Relationship Id="rIdLink" Type="{REL}/hyperlink" Target="https://example.invalid/linked" TargetMode="External"/><Relationship Id="rIdImage" Type="{REL}/image" Target="media/a.png"/></Relationships>"#
    )
}

fn styles() -> String {
    format!(
        r#"<?xml version="1.0"?><w:styles {W_NS}><w:docDefaults><w:rPrDefault><w:rPr/></w:rPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style><w:style w:type="paragraph" w:styleId="Title"><w:name w:val="Title"/><w:basedOn w:val="Normal"/></w:style><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:pPr><w:outlineLvl w:val="0"/></w:pPr></w:style><w:style w:type="paragraph" w:styleId="Quote"><w:name w:val="Quote"/></w:style><w:style w:type="paragraph" w:styleId="Author"><w:name w:val="Author"/><w:basedOn w:val="Title"/></w:style><w:style w:type="character" w:styleId="Emphasis"><w:name w:val="Emphasis"/><w:rPr><w:i/></w:rPr></w:style></w:styles>"#
    )
}

fn numbering() -> String {
    format!(
        r#"<?xml version="1.0"?><w:numbering {W_NS}><w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/></w:lvl><w:lvl w:ilvl="1"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1.%2."/></w:lvl></w:abstractNum><w:abstractNum w:abstractNumId="1"><w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/><w:lvlText w:val="&#xF0B7;"/></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num><w:num w:numId="2"><w:abstractNumId w:val="1"/></w:num></w:numbering>"#
    )
}

fn footnotes(notes: &[(u32, &str)]) -> String {
    let mut out = format!(
        r#"<?xml version="1.0"?><w:footnotes {W_NS}><w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:footnote>"#
    );
    for (id, text) in notes {
        write!(
            out,
            r#"<w:footnote w:id="{id}"><w:p><w:r><w:rPr><w:vertAlign w:val="superscript"/></w:rPr><w:footnoteRef/></w:r><w:r><w:t xml:space="preserve"> {text}</w:t></w:r></w:p></w:footnote>"#
        )
        .unwrap();
    }
    out.push_str("</w:footnotes>");
    out
}

fn word(path: &Path, body: &str, notes: &[(u32, &str)], thumbnail: bool) {
    let document = format!(
        r#"<?xml version="1.0"?><w:document {W_NS}><w:body>{body}<w:sectPr/></w:body></w:document>"#
    );
    let core = r#"<?xml version="1.0"?><cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Synthetic report</dc:title><dc:creator>A. Writer</dc:creator></cp:coreProperties>"#;
    let mut entries: Vec<(&str, Vec<u8>)> = vec![
        ("_rels/.rels", package_rels(thumbnail).into_bytes()),
        ("word/document.xml", document.into_bytes()),
        ("word/_rels/document.xml.rels", document_rels().into_bytes()),
        ("word/styles.xml", styles().into_bytes()),
        ("word/numbering.xml", numbering().into_bytes()),
        ("word/footnotes.xml", footnotes(notes).into_bytes()),
        ("word/media/a.png", PNG.to_vec()),
        ("docProps/core.xml", core.as_bytes().to_vec()),
    ];
    if thumbnail {
        entries.push(("docProps/thumbnail.png", PNG.to_vec()));
    }
    let borrowed: Vec<(&str, &[u8])> = entries.iter().map(|(n, b)| (*n, b.as_slice())).collect();
    zip(path, &borrowed);
}

const BODY: &str = r#"
<w:p><w:pPr><w:pStyle w:val="Title"/></w:pPr><w:r><w:t>Café report</w:t></w:r></w:p>
<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:bookmarkStart w:id="0" w:name="scope"/><w:r><w:t>Scope</w:t></w:r><w:bookmarkEnd w:id="0"/></w:p>
<w:p><w:pPr><w:jc w:val="center"/></w:pPr><w:r><w:t xml:space="preserve">Plain, </w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>bold</w:t></w:r><w:r><w:rPr><w:rStyle w:val="Emphasis"/></w:rPr><w:t xml:space="preserve"> styled</w:t></w:r><w:hyperlink r:id="rIdLink"><w:r><w:t xml:space="preserve"> a link</w:t></w:r></w:hyperlink><w:del w:id="1" w:author="x"><w:r><w:delText>gone</w:delText></w:r></w:del><w:ins w:id="2" w:author="x"><w:r><w:t xml:space="preserve"> added</w:t></w:r></w:ins><w:r><w:rPr><w:vanish/></w:rPr><w:t>hidden</w:t></w:r><w:r><w:rPr><w:rStyle w:val="FootnoteReference"/></w:rPr><w:footnoteReference w:id="1"/></w:r></w:p>
<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>First</w:t></w:r></w:p>
<w:p><w:pPr><w:numPr><w:ilvl w:val="1"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>Inner</w:t></w:r></w:p>
<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>Second</w:t></w:r></w:p>
<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="2"/></w:numPr></w:pPr><w:r><w:t>Dot</w:t></w:r></w:p>
<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> HYPERLINK "https://example.invalid/field" </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>field link</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r><w:r><w:t xml:space="preserve"> and </w:t></w:r><w:hyperlink w:anchor="scope"><w:r><w:t>back</w:t></w:r></w:hyperlink></w:p>
<w:tbl>
<w:tr><w:trPr><w:tblHeader/></w:trPr><w:tc><w:p><w:r><w:t>Name</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>Value</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>Note</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:tc><w:tcPr><w:gridSpan w:val="2"/></w:tcPr><w:p><w:r><w:t>wide</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:vMerge w:val="restart"/></w:tcPr><w:p><w:r><w:t>tall</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:tc><w:p><w:r><w:t>a</w:t></w:r></w:p><w:p><w:r><w:t>b</w:t></w:r></w:p></w:tc><w:tc><w:p/></w:tc><w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p/></w:tc></w:tr>
</w:tbl>
<w:p><w:r><w:t xml:space="preserve">Picture: </w:t></w:r><w:r><w:drawing><wp:inline><wp:docPr id="1" name="Picture 1" descr="A red box"/><a:graphic><a:graphicData><pic:pic><pic:blipFill><a:blip r:embed="rIdImage"/></pic:blipFill></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r><w:r><mc:AlternateContent><mc:Choice Requires="wps"><w:drawing><wp:anchor><wp:docPr id="2" name="Text Box 2"/><a:graphic><a:graphicData><wps:wsp><wps:txbx><w:txbxContent><w:p><w:r><w:t>In a box</w:t></w:r></w:p></w:txbxContent></wps:txbx></wps:wsp></a:graphicData></a:graphic></wp:anchor></w:drawing></mc:Choice><mc:Fallback><w:pict/></mc:Fallback></mc:AlternateContent></w:r></w:p>
<w:p><w:pPr><w:pStyle w:val="Quote"/></w:pPr><w:r><w:t>Quoted</w:t></w:r></w:p>
"#;

fn texts(blocks: &[Block]) -> Vec<&str> {
    blocks.iter().map(|b| b.text.as_str()).collect()
}

/// The run of `block` covering `text`.
fn run<'b>(block: &'b Block, text: &str) -> &'b bibli_core::inspection::Run {
    let start = block.text.find(text).expect("text in block");
    block
        .runs
        .iter()
        .find(|r| r.range.start <= start && start < r.range.end)
        .expect("a run")
}

#[test]
fn a_word_document_reads_as_its_structure() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("l’été report.docx");
    word(&path, BODY, &[(1, "The note.")], false);
    let read = docx::read(&path, 0, &CancelToken::new()).unwrap();
    let blocks = &read.blocks;
    assert_eq!(
        texts(blocks),
        [
            "Café report",
            "Scope",
            "Plain, bold styled a link added1",
            "First",
            "Inner",
            "Second",
            "Dot",
            "field link and back",
            "Name\tValue\tNote",
            "wide\t\ttall",
            "a\nb\t\t",
            "Picture:",
            "",
            "In a box",
            "Quoted",
            "",
            "1 The note.",
        ]
    );
    assert_eq!((blocks[0].heading, blocks[1].heading), (1, 1));
    assert_eq!(blocks[1].anchors, ["scope"]);
    let plain = &blocks[2];
    assert_eq!(plain.align, Align::Center);
    assert!(run(plain, "bold").bold && !run(plain, "Plain").bold);
    assert!(run(plain, "styled").italic);
    assert_eq!(
        run(plain, "a link").link.as_deref(),
        Some("https://example.invalid/linked")
    );
    let reference = plain.runs.last().unwrap();
    assert!(reference.note && reference.script == Script::Superscript);
    assert_eq!(reference.link.as_deref(), Some("#footnote-1"));
    let markers: Vec<_> = blocks[3..7]
        .iter()
        .map(|b| (b.marker.as_deref(), b.list_depth))
        .collect();
    assert_eq!(
        markers,
        [
            (Some("1."), 1),
            (Some("1.1."), 2),
            (Some("2."), 1),
            (Some("•"), 1)
        ]
    );
    assert_eq!(
        run(&blocks[7], "field").link.as_deref(),
        Some("https://example.invalid/field")
    );
    assert_eq!(run(&blocks[7], "back").link.as_deref(), Some("#scope"));
    assert!(matches!(
        blocks[8].kind,
        BlockKind::TableRow { header: true, .. }
    ));
    let BlockKind::TableRow { cells, header } = &blocks[9].kind else {
        panic!("a row");
    };
    assert_eq!((cells.len(), *header), (3, false));
    assert!(matches!(
        blocks[12].picture,
        Some(Picture::Later { ref key, width: 2, height: 3 }) if key == "word/media/a.png"
    ));
    assert!(blocks[14].quote);
    assert_eq!(blocks[15].kind, BlockKind::Rule);
    assert_eq!(blocks[16].note, Some(0));
    assert_eq!(blocks[16].anchors, ["footnote-1"]);
    assert_eq!((read.part, read.parts), (0, 1));
    assert!(read.details.contains(&("Title", "Synthetic report".into())));
    assert!(read.details.contains(&("Author", "A. Writer".into())));
}

#[test]
fn a_style_based_on_the_title_is_not_a_title() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("author.docx");
    word(
        &path,
        r#"<w:p><w:pPr><w:pStyle w:val="Title"/></w:pPr><w:r><w:t>Report</w:t></w:r></w:p><w:p><w:pPr><w:pStyle w:val="Author"/></w:pPr><w:r><w:t>A. Writer</w:t></w:r></w:p>"#,
        &[],
        false,
    );
    let read = docx::read(&path, 0, &CancelToken::new()).unwrap();
    let headings: Vec<_> = read
        .blocks
        .iter()
        .map(|b| (b.text.as_str(), b.heading))
        .collect();
    assert_eq!(headings, [("Report", 1), ("A. Writer", 0)]);
}

#[test]
fn a_long_word_document_is_read_in_parts_numbered_from_its_start() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("long.docx");
    let total = blocks::PART_BLOCKS * 2 + 10;
    let mut body = String::new();
    for i in 0..total {
        let note = match i {
            0 => r#"<w:r><w:footnoteReference w:id="1"/></w:r>"#,
            i if i == blocks::PART_BLOCKS + 3 => r#"<w:r><w:footnoteReference w:id="2"/></w:r>"#,
            _ => "",
        };
        write!(
            body,
            r#"<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>Item {i}</w:t></w:r>{note}</w:p>"#
        )
        .unwrap();
    }
    word(
        &path,
        &body,
        &[(1, "First note."), (2, "Second note.")],
        false,
    );
    let read = docx::read(&path, 1, &CancelToken::new()).unwrap();
    assert_eq!((read.part, read.parts), (1, 3));
    let first = &read.blocks[0];
    assert_eq!(first.text, format!("Item {}", blocks::PART_BLOCKS));
    assert_eq!(
        first.marker.as_deref(),
        Some(format!("{}.", blocks::PART_BLOCKS + 1).as_str()),
        "numbering counts from the document's start"
    );
    let noted = &read.blocks[3];
    assert_eq!(
        noted.runs.last().unwrap().link.as_deref(),
        Some("#footnote-2")
    );
    assert!(noted.text.ends_with('2'), "the second note of the document");
    let last = read.blocks.last().unwrap();
    assert_eq!((last.text.as_str(), last.note), ("2 Second note.", Some(0)));
    assert!(!read.blocks.iter().any(|b| b.text.contains("First note")));
    // Past the end: the last part.
    let read = docx::read(&path, 7, &CancelToken::new()).unwrap();
    assert_eq!((read.part, read.parts), (2, 3));
    assert_eq!(read.words, total * 2);
}

/// `PDFium` as the settings show it: installed only where the helper can
/// open the library the settings name (`process::probe`), not merely
/// where the helper is. Needs the helper built in the target folder the
/// test runs from; without it the test says so and passes, unless
/// `BIBLI_REQUIRE_WORKERS` is set.
#[test]
fn pdfium_is_installed_only_with_a_library_it_can_open() {
    let built = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.parent()?.join("bibli-pdf-worker")))
        .filter(|path| path.is_file());
    let Some(worker) = built else {
        assert!(
            std::env::var_os("BIBLI_REQUIRE_WORKERS").is_none(),
            "bibli-pdf-worker is not built"
        );
        eprintln!("skipped: bibli-pdf-worker is not built");
        return;
    };
    // The shell's own library, if it names one (the `pdf` dev shell).
    let real = std::env::var_os("BIBLI_PDFIUM_LIBRARY");
    // SAFETY: nextest runs each test in a process of its own, and nothing
    // else reads the environment before this.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("BIBLI_PDF_WORKER", &worker);
        std::env::remove_var("BIBLI_PDFIUM_LIBRARY");
    }
    let dir = tempfile::tempdir().unwrap();
    let app = bibli_core::App::new(bibli_core::app::AppConfig::ephemeral(
        dir.path().join("state"),
    ))
    .unwrap();
    let plugin = Arc::new(PreviewPlugin::default());
    app.plugins
        .add(Arc::clone(&plugin) as Arc<dyn Plugin>, &app);
    let pdfium = |library: &std::path::Path| {
        app.settings.set(
            PDFIUM_LIBRARY,
            toml::Value::String(library.to_string_lossy().into_owned()),
        );
        plugin.on_settings_changed(&app, &[PDFIUM_LIBRARY.to_owned()]);
        app.plugins
            .probe_requirements()
            .into_iter()
            .find(|status| status.name == "bibli-pdf-worker")
            .expect("the PDFium requirement")
            .found
    };
    assert!(
        !pdfium(&dir.path().join("libpdfium.so")),
        "the helper alone is not PDFium"
    );
    if let Some(real) = real {
        assert!(
            pdfium(std::path::Path::new(&real)),
            "the library named works"
        );
    }
}

#[test]
fn a_text_document_is_read_by_default_whatever_is_installed() {
    let dir = tempfile::tempdir().unwrap();
    let app = bibli_core::App::new(bibli_core::app::AppConfig::ephemeral(
        dir.path().join("state"),
    ))
    .unwrap();
    app.plugins.add(Arc::new(PreviewPlugin::default()), &app);
    let path = dir.path().join("report.docx");
    word(&path, BODY, &[(1, "The note.")], false);
    let data = app
        .previews
        .inspect(
            &request(&path),
            &InspectRequest::default(),
            &CancelToken::new(),
        )
        .unwrap()
        .expect("the reading provider answers");
    assert!(data.notice.starts_with("Reading mode"), "{}", data.notice);
    assert_eq!(data.blocks[0].text, "Café report");
}

fn request(path: &Path) -> PreviewRequest {
    PreviewRequest {
        entry: EntryRef::from_path(path).unwrap(),
        mime: None,
        page: 1,
        seconds: None,
        max_side: 256,
        text_size: None,
    }
}

/// The reading provider, its cache in `dir`.
fn reading(choice: Choice, dir: &Path) -> Reading {
    Reading {
        choice,
        cache: bibli_core::cache::Cache::new(dir.join("cache")).namespace(ID),
    }
}

#[test]
fn reading_takes_what_bibli_reads_and_names_its_pictures_for_later() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("report.docx");
    word(&path, BODY, &[(1, "The note.")], false);
    let provider = reading(Choice::Reading, dir.path());
    let req = request(&path);
    assert!(provider.accepts(&req));
    let view = InspectRequest::default();
    let data = provider
        .inspect(&req, &view, &CancelToken::new())
        .unwrap()
        .unwrap();
    assert_eq!(data.kind, InspectionKind::Markdown);
    assert!(data.title.is_empty() && data.next.is_none());
    assert!(data.notice.starts_with("Reading mode"));
    let labels: Vec<_> = data.details.iter().map(|d| d.label.as_str()).collect();
    assert_eq!(labels, ["Title", "Author", "Words"]);
    let picture = provider
        .picture(&req, &view, "word/media/a.png", &CancelToken::new())
        .unwrap()
        .unwrap();
    assert_eq!((picture.width, picture.height), (2, 3));
    assert!(
        provider
            .picture(&req, &view, "../outside.png", &CancelToken::new())
            .is_err()
    );
    // A Word file in name only (an encrypted one is an OLE container) is
    // not read, nor given to Pandoc.
    let fake = dir.path().join("locked.docx");
    std::fs::write(&fake, b"\xd0\xcf\x11\xe0 not a zip").unwrap();
    assert!(!provider.accepts(&request(&fake)));
    let legacy = dir.path().join("old.doc");
    zip(&legacy, &[("x", b"y")]);
    assert!(!provider.accepts(&request(&legacy)));
}

const ODF_NS: &str = r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:xlink="http://www.w3.org/1999/xlink" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" xmlns:meta="urn:oasis:names:tc:opendocument:xmlns:meta:1.0" xmlns:dc="http://purl.org/dc/elements/1.1/""#;

fn open_document(path: &Path, body: &str) {
    let content = format!(
        r#"<?xml version="1.0"?><office:document-content {ODF_NS} office:version="1.3"><office:automatic-styles><style:style style:name="T1" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style><style:style style:name="T2" style:family="text"><style:text-properties style:text-position="super 58%"/></style:style><style:style style:name="P1" style:family="paragraph" style:parent-style-name="Standard"><style:paragraph-properties fo:text-align="center"/></style:style><style:style style:name="P2" style:family="paragraph" style:parent-style-name="Title"/><text:list-style style:name="L1"><text:list-level-style-number text:level="1" style:num-suffix="." style:num-format="1"/><text:list-level-style-number text:level="2" style:num-suffix=")" style:num-format="a"/></text:list-style></office:automatic-styles><office:body><office:text>{body}</office:text></office:body></office:document-content>"#
    );
    let styles = format!(
        r#"<?xml version="1.0"?><office:document-styles {ODF_NS}><office:styles><style:style style:name="Standard" style:family="paragraph"/><style:style style:name="Title" style:family="paragraph" style:parent-style-name="Standard"/><style:style style:name="Author" style:family="paragraph" style:parent-style-name="Title"/><style:style style:name="Quotations" style:family="paragraph" style:parent-style-name="Standard"/></office:styles><office:automatic-styles><style:style style:name="P1" style:family="paragraph"><style:paragraph-properties fo:text-align="end"/></style:style></office:automatic-styles></office:document-styles>"#
    );
    let meta = format!(
        r#"<?xml version="1.0"?><office:document-meta {ODF_NS}><office:meta><dc:title>Synthetic report</dc:title><meta:initial-creator>A. Writer</meta:initial-creator><meta:document-statistic meta:page-count="3"/><meta:generator>LibreOffice/24.2.1.2$Linux_X86_64 LibreOffice_project/x</meta:generator></office:meta></office:document-meta>"#
    );
    zip(
        path,
        &[
            ("mimetype", b"application/vnd.oasis.opendocument.text"),
            ("content.xml", content.as_bytes()),
            ("styles.xml", styles.as_bytes()),
            ("meta.xml", meta.as_bytes()),
            ("Pictures/a.png", PNG),
            ("Thumbnails/thumbnail.png", PNG),
        ],
    );
}

const ODF_BODY: &str = r#"
<text:tracked-changes><text:changed-region text:id="c1"><text:deletion><text:p>gone</text:p></text:deletion></text:changed-region></text:tracked-changes>
<text:h text:outline-level="1">Café report</text:h>
<text:p text:style-name="P1">Plain,   <text:span text:style-name="T1">bold</text:span> <text:a xlink:href="https://example.invalid/linked">a link</text:a><text:s text:c="2"/>end<text:note text:id="ftn1" text:note-class="footnote"><text:note-citation>1</text:note-citation><text:note-body><text:p>The note.</text:p></text:note-body></text:note></text:p>
<text:list text:style-name="L1">
<text:list-item><text:p>First</text:p><text:list><text:list-item><text:p>Inner</text:p></text:list-item></text:list></text:list-item>
<text:list-item><text:p>Second</text:p></text:list-item>
</text:list>
<text:p>Between<text:span text:style-name="T2">2</text:span></text:p>
<text:list text:style-name="L1" text:continue-numbering="true"><text:list-item><text:p>Third</text:p></text:list-item></text:list>
<table:table><table:table-column table:number-columns-repeated="2"/>
<table:table-header-rows><table:table-row><table:table-cell><text:p>Name</text:p></table:table-cell><table:table-cell><text:p>Value</text:p></table:table-cell></table:table-row></table:table-header-rows>
<table:table-row><table:table-cell table:number-columns-spanned="2"><text:p>wide</text:p></table:table-cell><table:covered-table-cell/></table:table-row>
</table:table>
<text:p>Picture: <draw:frame draw:name="Image1"><draw:image xlink:href="Pictures/a.png"/><svg:desc>A red box</svg:desc></draw:frame></text:p>
<text:p text:style-name="Quotations">Quoted<office:annotation><text:p>a comment</text:p></office:annotation></text:p>
<text:p text:style-name="P2">Synthetic title</text:p>
<text:p text:style-name="Author">A. Writer</text:p>
"#;

#[test]
fn an_open_document_text_reads_as_its_structure() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("l’été report.odt");
    open_document(&path, ODF_BODY);
    let read = odt::read(&path, 0, &CancelToken::new()).unwrap();
    let blocks = &read.blocks;
    assert_eq!(
        texts(blocks),
        [
            "Café report",
            "Plain, bold a link  end1",
            "First",
            "Inner",
            "Second",
            "Between2",
            "Third",
            "Name\tValue",
            "wide\t",
            "Picture:",
            "",
            "Quoted",
            "Synthetic title",
            "A. Writer",
            "",
            "1 The note.",
        ]
    );
    assert_eq!(blocks[0].heading, 1);
    assert_eq!(
        (blocks[12].heading, blocks[13].heading),
        (1, 0),
        "a title through its automatic style; a style based on it is no title"
    );
    let plain = &blocks[1];
    assert_eq!(plain.align, Align::Center, "content's automatic style wins");
    assert!(run(plain, "bold").bold);
    assert_eq!(
        run(plain, "a link").link.as_deref(),
        Some("https://example.invalid/linked")
    );
    let reference = plain.runs.last().unwrap();
    assert!(reference.note);
    assert_eq!(reference.link.as_deref(), Some("#ftn1"));
    let markers: Vec<_> = [&blocks[2], &blocks[3], &blocks[4], &blocks[6]]
        .iter()
        .map(|b| (b.marker.as_deref(), b.list_depth))
        .collect();
    assert_eq!(
        markers,
        [
            (Some("1."), 1),
            (Some("a)"), 2),
            (Some("2."), 1),
            (Some("3."), 1)
        ],
        "a list that continues numbering goes on counting"
    );
    assert_eq!(run(&blocks[5], "2").script, Script::Superscript);
    assert!(matches!(
        blocks[7].kind,
        BlockKind::TableRow { header: true, .. }
    ));
    assert!(matches!(
        blocks[10].picture,
        Some(Picture::Later { ref key, .. }) if key == "Pictures/a.png"
    ));
    assert!(blocks[11].quote);
    assert_eq!(blocks[15].note, Some(0));
    assert_eq!(blocks[15].anchors, ["ftn1"]);
    assert!(read.details.contains(&("Pages", "3".into())));
    let facts = details(&read.details, read.words);
    let application = facts.iter().find(|d| d.label == "Application").unwrap();
    assert_eq!(application.value, "LibreOffice 24.2.1.2");
}

#[test]
fn a_password_protected_open_document_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("locked.odt");
    zip(
        &path,
        &[
            ("mimetype", b"application/vnd.oasis.opendocument.text"),
            (
                "META-INF/manifest.xml",
                br#"<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0"><manifest:file-entry manifest:full-path="content.xml"><manifest:encryption-data/></manifest:file-entry></manifest:manifest>"#,
            ),
            ("content.xml", b"\x00\x01 ciphered"),
        ],
    );
    let error = odt::read(&path, 0, &CancelToken::new()).err().unwrap();
    assert!(error.to_string().contains("password"), "{error}");
}

#[test]
fn a_document_keeps_a_picture_of_its_first_page() {
    let dir = tempfile::tempdir().unwrap();
    let side = |path: &Path| {
        stored::picture(path, 256, &CancelToken::new())
            .unwrap()
            .map(|image| (image.width, image.height))
    };
    let text = dir.path().join("a.odt");
    open_document(&text, "<text:p>x</text:p>");
    assert_eq!(side(&text), Some((2, 3)));
    let kept = dir.path().join("kept.docx");
    word(&kept, BODY, &[], true);
    assert_eq!(side(&kept), Some((2, 3)));
    let none = dir.path().join("none.docx");
    word(&none, BODY, &[], false);
    assert_eq!(side(&none), None);
    let fake = dir.path().join("fake.pptx");
    std::fs::write(&fake, b"not a zip").unwrap();
    assert_eq!(side(&fake), None);
}

/// Pandoc through the sandbox, end to end: needs Pandoc and bubblewrap,
/// which the dev shell has and a CI runner may not let run.
#[test]
#[ignore = "runs Pandoc in bubblewrap"]
fn pandoc_reads_rtf_and_keeps_its_pictures() {
    let dir = tempfile::tempdir().unwrap();
    let rtf = dir.path().join("notes été.rtf");
    std::fs::write(
        &rtf,
        br"{\rtf1\ansi{\b Bold} and {\i italic}\par Second paragraph\par}",
    )
    .unwrap();
    let provider = reading(Choice::Reading, dir.path());
    let req = request(&rtf);
    assert!(provider.accepts(&req), "Pandoc reads RTF");
    let data = provider
        .inspect(&req, &InspectRequest::default(), &CancelToken::new())
        .unwrap()
        .unwrap();
    assert_eq!(texts(&data.blocks), ["Bold and italic", "Second paragraph"]);
    assert!(run(&data.blocks[0], "Bold").bold);
    assert!(data.notice.starts_with("Read by Pandoc"));
    // A Word file read by Pandoc when asked: its picture extracted, kept,
    // and read back once its folder is gone.
    let docx = dir.path().join("report.docx");
    word(&docx, BODY, &[(1, "The note.")], false);
    let provider = reading(Choice::Pandoc, dir.path());
    let req = request(&docx);
    let view = InspectRequest::default();
    let data = provider
        .inspect(&req, &view, &CancelToken::new())
        .unwrap()
        .unwrap();
    let key = data
        .blocks
        .iter()
        .find_map(|b| match &b.picture {
            Some(Picture::Later { key, .. }) => Some(key.clone()),
            _ => None,
        })
        .expect("the picture, named for later");
    let picture = provider
        .picture(&req, &view, &key, &CancelToken::new())
        .unwrap()
        .unwrap();
    assert_eq!((picture.width, picture.height), (2, 3));
    assert!(data.blocks.iter().any(|b| b.text == "First"));
    // A cache that let it go: Pandoc extracts it again.
    let elsewhere = tempfile::tempdir().unwrap();
    let provider = reading(Choice::Pandoc, elsewhere.path());
    let picture = provider
        .picture(&req, &view, &key, &CancelToken::new())
        .unwrap()
        .unwrap();
    assert_eq!((picture.width, picture.height), (2, 3));
}

/// What Pandoc made is kept with its pictures' sizes, and read back as it
/// was; anything else kept there is not taken for it.
#[test]
fn a_conversion_kept_reads_back_as_it_was() {
    let sizes: std::collections::HashMap<String, (u32, u32)> = [
        ("media/image1.png".to_owned(), (640, 480)),
        ("media/sub/é.jpg".to_owned(), (3, 4)),
    ]
    .into_iter()
    .collect();
    let html = "<p>one</p>\n\n<p>two</p>";
    let kept = super::converted_to(html, true, &sizes);
    let (back, cut, back_sizes) = super::converted_from(&kept).unwrap();
    assert_eq!((back.as_str(), cut), (html, true));
    assert_eq!(back_sizes, sizes);
    let (_, cut, none) = super::converted_from(&super::converted_to(
        "x",
        false,
        &std::collections::HashMap::default(),
    ))
    .unwrap();
    assert!(!cut && none.is_empty());
    assert!(super::converted_from(b"\x89PNG\r\n").is_none());
}
