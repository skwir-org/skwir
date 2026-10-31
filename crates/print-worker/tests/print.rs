//! The helper run on jobs made here, the PDFs it writes read back: their
//! pages, text, outline, links, pictures, metadata and structure. Fonts
//! are the machine's, found by fontconfig (`DejaVu`, Noto, Liberation);
//! without them the tests say so and pass. Pictures are made here.

use std::path::{Path, PathBuf};
use std::process::Command;

use lopdf::{Dictionary, Document, Object};
use skwir_core::print::{
    Align, Block, BlockKind, Face, Family, Fonts, Job, Line, Made, Paper, Part, Picture, Run,
};

/// The file and index fontconfig gives for `pattern`.
fn matched(pattern: &str) -> Option<Face> {
    let output = Command::new("fc-match")
        .args(["-f", "%{file}\n%{index}", pattern])
        .output()
        .ok()?;
    let text = String::from_utf8(output.stdout).ok()?;
    let mut lines = text.lines();
    let path = PathBuf::from(lines.next()?);
    let index = lines.next().and_then(|i| i.parse().ok()).unwrap_or(0);
    let known = ["dejavu", "noto", "liberation"];
    let name = path.file_name()?.to_string_lossy().to_lowercase();
    (path.is_file() && known.iter().any(|k| name.contains(k))).then_some(Face { path, index })
}

fn fonts() -> Option<Fonts> {
    let family = |generic: &str| Family {
        regular: matched(generic),
        bold: matched(&format!("{generic}:bold")),
        italic: matched(&format!("{generic}:italic")),
        bold_italic: matched(&format!("{generic}:bold:italic")),
    };
    let text = family("sans-serif");
    let code = family("monospace");
    if text.regular.is_none() || code.regular.is_none() {
        eprintln!("no DejaVu, Noto or Liberation font found by fc-match: not tried");
        return None;
    }
    Some(Fonts { text, code })
}

fn job(dir: &Path, fonts: Fonts, parts: Vec<Part>) -> Job {
    Job {
        paper: Paper::a4(),
        output: dir.join("out.pdf"),
        title: "A made-up document".into(),
        author: Some("Someone".into()),
        language: Some("en".into()),
        header: Some("example.txt".into()),
        numbers: true,
        fonts,
        fallback: Vec::new(),
        pages: None,
        parts,
    }
}

/// The helper run on `job`: its answer, or its message and code.
fn run(dir: &Path, job: &Job) -> Result<(Made, Vec<u8>), (String, i32)> {
    let path = dir.join("job.json");
    std::fs::write(&path, serde_json::to_vec(job).unwrap()).unwrap();
    run_file(&path, &job.output)
}

fn run_file(path: &Path, output: &Path) -> Result<(Made, Vec<u8>), (String, i32)> {
    let result = Command::new(env!("CARGO_BIN_EXE_skwir-print-worker"))
        .arg(path)
        .output()
        .unwrap();
    if !result.status.success() {
        return Err((
            String::from_utf8_lossy(&result.stderr).into_owned(),
            result.status.code().unwrap_or(-1),
        ));
    }
    let answer = String::from_utf8(result.stdout).unwrap();
    assert_eq!(answer.lines().count(), 1, "one line: {answer}");
    let made: Made = serde_json::from_str(answer.trim()).unwrap();
    Ok((made, std::fs::read(output).unwrap()))
}

fn ok(dir: &Path, job: &Job) -> (Made, Document, Vec<u8>) {
    let (made, bytes) = run(dir, job).unwrap_or_else(|(message, code)| panic!("{code}: {message}"));
    let document = Document::load_mem(&bytes).unwrap();
    (made, document, bytes)
}

/// The text of page `page` (from 1), by Poppler's `pdftotext` when it is
/// there (it reads what a viewer copies), else by lopdf.
fn text(pdf: &[u8], document: &Document, page: u32, dir: &Path) -> String {
    let path = dir.join("read.pdf");
    std::fs::write(&path, pdf).unwrap();
    if let Ok(output) = Command::new("pdftotext")
        .args(["-f", &page.to_string(), "-l", &page.to_string(), "-layout"])
        .arg(&path)
        .arg("-")
        .output()
        && output.status.success()
    {
        return String::from_utf8_lossy(&output.stdout).into_owned();
    }
    document.extract_text(&[page]).unwrap_or_default()
}

fn decode(object: &Object) -> String {
    let Ok(bytes) = object.as_str() else {
        return String::new();
    };
    if bytes.starts_with(&[0xfe, 0xff]) {
        let units: Vec<u16> = bytes[2..]
            .chunks(2)
            .map(|pair| u16::from_be_bytes([pair[0], *pair.get(1).unwrap_or(&0)]))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        bytes.iter().map(|&b| char::from(b)).collect()
    }
}

fn deref<'a>(document: &'a Document, object: &'a Object) -> &'a Object {
    document
        .dereference(object)
        .map_or(object, |(_, object)| object)
}

fn dict<'a>(document: &'a Document, object: &'a Object) -> Option<&'a Dictionary> {
    deref(document, object).as_dict().ok()
}

/// The outline's entries, depth first: their depth and title.
fn outline(document: &Document) -> Vec<(usize, String)> {
    fn walk(document: &Document, node: &Dictionary, depth: usize, out: &mut Vec<(usize, String)>) {
        let mut next = node.get(b"First").ok();
        while let Some(object) = next {
            let Some(item) = dict(document, object) else {
                break;
            };
            out.push((depth, item.get(b"Title").map(decode).unwrap_or_default()));
            walk(document, item, depth + 1, out);
            next = item.get(b"Next").ok();
        }
    }
    let mut out = Vec::new();
    let catalog = document.catalog().unwrap();
    if let Some(root) = catalog
        .get(b"Outlines")
        .ok()
        .and_then(|o| dict(document, o))
    {
        walk(document, root, 0, &mut out);
    }
    out
}

/// The links of every page: their address, or `None` for one within.
fn links(document: &Document) -> Vec<(u32, Option<String>)> {
    let mut out = Vec::new();
    for (number, id) in document.get_pages() {
        for annot in document.get_page_annotations(id).unwrap_or_default() {
            if annot.get(b"Subtype").and_then(Object::as_name).ok() != Some(b"Link".as_slice()) {
                continue;
            }
            let uri = annot
                .get(b"A")
                .ok()
                .and_then(|a| dict(document, a))
                .and_then(|a| a.get(b"URI").ok())
                .map(decode);
            out.push((number, uri));
        }
    }
    out
}

/// The pictures embedded: each one's filter.
fn images(document: &Document) -> Vec<String> {
    document
        .objects
        .values()
        .filter_map(|object| object.as_stream().ok())
        .filter(|stream| {
            stream.dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Image".as_slice())
        })
        .map(|stream| {
            stream
                .dict
                .get(b"Filter")
                .and_then(Object::as_name)
                .map(|name| String::from_utf8_lossy(name).into_owned())
                .unwrap_or_default()
        })
        .collect()
}

fn info(document: &Document, key: &[u8]) -> String {
    document
        .trailer
        .get(b"Info")
        .ok()
        .and_then(|info| dict(document, info))
        .and_then(|info| info.get(key).ok())
        .map(decode)
        .unwrap_or_default()
}

fn tagged(document: &Document) -> bool {
    let catalog = document.catalog().unwrap();
    catalog.get(b"StructTreeRoot").is_ok()
        && catalog
            .get(b"MarkInfo")
            .ok()
            .and_then(|m| dict(document, m))
            .and_then(|m| m.get(b"Marked").ok())
            .and_then(|m| m.as_bool().ok())
            == Some(true)
}

/// The structure types under the tree, by name, counted.
fn structure(document: &Document) -> std::collections::HashMap<String, usize> {
    let mut counts = std::collections::HashMap::new();
    let catalog = document.catalog().unwrap();
    let mut stack: Vec<&Object> = catalog.get(b"StructTreeRoot").into_iter().collect();
    let mut seen = 0;
    while let Some(object) = stack.pop() {
        seen += 1;
        if seen > 1_000_000 {
            break;
        }
        match deref(document, object) {
            Object::Dictionary(node) => {
                if let Ok(kind) = node.get(b"S").and_then(Object::as_name) {
                    *counts
                        .entry(String::from_utf8_lossy(kind).into_owned())
                        .or_insert(0) += 1;
                }
                if let Ok(kids) = node.get(b"K") {
                    stack.push(kids);
                }
            }
            Object::Array(items) => stack.extend(items.iter()),
            _ => {}
        }
    }
    counts
}

fn page_size(document: &Document, page: u32) -> (f32, f32) {
    let id = document.get_pages()[&page];
    let page = document.get_dictionary(id).unwrap();
    let mut media = page
        .get(b"MediaBox")
        .ok()
        .and_then(|m| deref(document, m).as_array().ok());
    if media.is_none() {
        // Inherited from the tree.
        let parent = page
            .get(b"Parent")
            .ok()
            .and_then(|p| dict(document, p))
            .unwrap();
        media = parent
            .get(b"MediaBox")
            .ok()
            .and_then(|m| deref(document, m).as_array().ok());
    }
    let number = |o: &Object| {
        o.as_float()
            .or_else(|_| o.as_i64().map(|i| i as f32))
            .unwrap()
    };
    let media = media.unwrap();
    (
        number(&media[2]) - number(&media[0]),
        number(&media[3]) - number(&media[1]),
    )
}

fn line(text: &str) -> Line {
    Line {
        text: text.into(),
        runs: Vec::new(),
    }
}

fn scratch() -> tempfile::TempDir {
    let base = std::env::var_os("HOME").map_or_else(std::env::temp_dir, |home| {
        PathBuf::from(home).join(".cache/skwir-tests")
    });
    std::fs::create_dir_all(&base).unwrap();
    tempfile::tempdir_in(base).unwrap()
}

/// The rows of code a page of A4 holds: its text's height over 9 × 1.3.
const A4_ROWS: usize = 64;

#[test]
fn a_thousand_lines_of_code_are_numbered_and_paged() {
    let Some(fonts) = fonts() else { return };
    let dir = scratch();
    let mut lines: Vec<Line> = (0..1000)
        .map(|i| line(&format!("let value_{i} = compute({i});")))
        .collect();
    // A line wider than the page, wrapped on three rows.
    lines[10] = line(&format!("let long = \"{}\";", "abcdefghij ".repeat(20)));
    lines[20].runs.push(Run {
        start: 0,
        end: 3,
        color: Some([0xcf, 0x22, 0x2e]),
        bold: true,
        ..Run::default()
    });
    let job = job(
        dir.path(),
        fonts,
        vec![Part::Code {
            lines,
            numbers: true,
            first: 1,
        }],
    );
    let (made, document, pdf) = ok(dir.path(), &job);
    let rows: usize = 1000 + 2;
    assert_eq!(made.laid_out as usize, rows.div_ceil(A4_ROWS));
    assert_eq!(made.pages, made.laid_out);
    assert_eq!(document.get_pages().len(), made.pages as usize);
    let first = text(&pdf, &document, 1, dir.path());
    assert!(first.contains("let value_0 = compute(0);"), "{first}");
    assert!(first.contains("example.txt"), "the name above");
    assert!(
        first.contains(&format!("1 / {}", made.laid_out)),
        "the page below"
    );
    // Line 11 is the long one: its number, then its rows marked.
    let numbered = first.lines().find(|l| l.contains("let long")).unwrap();
    assert!(numbered.trim_start().starts_with("11"), "{numbered}");
    assert!(first.contains('↪'), "continued rows marked");
    let last = text(&pdf, &document, made.pages, dir.path());
    assert!(last.contains("1000"), "the last line's number");
    assert!(last.contains("let value_999 = compute(999);"));
    assert!(tagged(&document));
    assert!(structure(&document).contains_key("Code"));
    assert_eq!(info(&document, b"Title"), "A made-up document");
    assert_eq!(info(&document, b"Author"), "Someone");
}

#[test]
fn only_the_pages_asked_are_written_numbered_as_laid_out() {
    let Some(fonts) = fonts() else { return };
    let dir = scratch();
    let lines: Vec<Line> = (0..300).map(|i| line(&format!("line {i}"))).collect();
    let mut job = job(
        dir.path(),
        fonts,
        vec![Part::Code {
            lines,
            numbers: false,
            first: 1,
        }],
    );
    job.pages = Some(vec![3, 1, 99]);
    let (made, document, pdf) = ok(dir.path(), &job);
    assert_eq!(made.laid_out, 5);
    assert_eq!(made.pages, 2);
    assert_eq!(document.get_pages().len(), 2);
    let first = text(&pdf, &document, 1, dir.path());
    // pdftotext lays out narrow spaces as it likes: "2/5" or "2 / 5".
    assert!(first.replace(' ', "").contains("2/5"), "{first}");
    assert!(first.contains(&format!("line {A4_ROWS}")));
    job.pages = Some(vec![7]);
    let (message, code) = run(dir.path(), &job).unwrap_err();
    assert_eq!(code, 1);
    assert_eq!(message.trim().lines().count(), 1, "{message}");
}

#[test]
fn a_booklet_s_half_page_holds_less_and_is_its_size() {
    let Some(fonts) = fonts() else { return };
    let dir = scratch();
    let lines: Vec<Line> = (0..200)
        .map(|i| line(&format!("{i}: {}", "word ".repeat(16))))
        .collect();
    let mut job = job(
        dir.path(),
        fonts,
        vec![Part::Code {
            lines,
            numbers: true,
            first: 1,
        }],
    );
    let (whole, _, _) = ok(dir.path(), &job);
    job.paper = Paper::a4().half();
    let (half, document, _) = ok(dir.path(), &job);
    assert!(half.laid_out > whole.laid_out, "{half:?} {whole:?}");
    let (width, height) = page_size(&document, 1);
    assert!((width - 420.95).abs() < 0.5, "{width}");
    assert!((height - 595.28).abs() < 0.5, "{height}");
}

fn block(text: &str) -> Block {
    Block {
        text: text.into(),
        ..Block::default()
    }
}

fn write_png(path: &Path, width: u32, height: u32, tint: u8) {
    image::RgbImage::from_fn(width, height, |x, y| {
        image::Rgb([(x % 256) as u8, (y % 256) as u8, tint])
    })
    .save(path)
    .unwrap();
}

fn write_jpeg(path: &Path, width: u32, height: u32) {
    image::RgbImage::from_fn(width, height, |x, y| {
        image::Rgb([((x * 3) % 256) as u8, 128, ((y * 5) % 256) as u8])
    })
    .save(path)
    .unwrap();
}

/// A row of a table: its cells' text joined, each cell's range.
fn row(cells: &[&str], header: bool) -> Block {
    let mut text = String::new();
    let mut ranges = Vec::new();
    for cell in cells {
        let start = text.len() as u32;
        text.push_str(cell);
        ranges.push((start, text.len() as u32));
        text.push('\t');
    }
    Block {
        text,
        kind: BlockKind::Row {
            cells: ranges,
            header,
        },
        ..Block::default()
    }
}

#[test]
fn a_document_has_its_outline_links_lists_tables_and_pictures() {
    let Some(fonts) = fonts() else { return };
    let dir = scratch();
    let png = dir.path().join("figure.png");
    write_png(&png, 320, 200, 40);
    let lorem = "Words go on and on so that a paragraph takes several lines of the page, \
                 justified between its margins as a book sets them. ";
    let mut blocks = vec![
        Block {
            heading: 1,
            ..block("The first chapter")
        },
        Block {
            align: Align::Justify,
            runs: vec![
                Run {
                    start: 0,
                    end: 5,
                    bold: true,
                    ..Run::default()
                },
                Run {
                    start: 6,
                    end: 8,
                    italic: true,
                    ..Run::default()
                },
            ],
            ..block(&lorem.repeat(3))
        },
    ];
    let linked = "Read the site or the second section.";
    blocks.push(Block {
        runs: vec![
            Run {
                start: 9,
                end: 13,
                link: Some("https://example.org/".into()),
                ..Run::default()
            },
            Run {
                start: 21,
                end: 35,
                link: Some("#second".into()),
                ..Run::default()
            },
        ],
        ..block(linked)
    });
    for (n, item) in ["First point", "Second point"].iter().enumerate() {
        blocks.push(Block {
            list_depth: 1,
            marker: Some(format!("{}.", n + 1)),
            ..block(item)
        });
    }
    blocks.push(Block {
        list_depth: 2,
        marker: Some("•".into()),
        ..block("A point within")
    });
    blocks.push(Block {
        quote: true,
        ..block("Something someone said once.")
    });
    blocks.push(Block {
        kind: BlockKind::Code,
        runs: vec![Run {
            start: 0,
            end: 2,
            color: Some([0xcf, 0x22, 0x2e]),
            ..Run::default()
        }],
        ..block(&format!(
            "fn main() {{\n    println!(\"{}\");\n}}",
            "x".repeat(150)
        ))
    });
    blocks.push(Block {
        kind: BlockKind::Rule,
        ..Block::default()
    });
    blocks.push(Block {
        kind: BlockKind::Picture(Picture {
            path: png.clone(),
            width: 320,
            height: 200,
            alt: "A gradient".into(),
            caption: Some("A figure".into()),
        }),
        ..Block::default()
    });
    blocks.push(Block {
        heading: 2,
        anchors: vec!["second".into()],
        ..block("The second section")
    });
    blocks.push(row(&["Name", "Amount", "Unit"], true));
    for i in 0..70 {
        blocks.push(row(
            &[&format!("item {i}"), &format!("{}", i * 7), "kg"],
            false,
        ));
    }
    blocks.push(Block {
        note: Some(1),
        ..block("1. A note at the end.")
    });
    let job = job(dir.path(), fonts, vec![Part::Flow { blocks }]);
    let (made, document, pdf) = ok(dir.path(), &job);
    assert!(made.pages >= 2, "{made:?}");
    let entries = outline(&document);
    assert_eq!(
        entries,
        [
            (0, "The first chapter".to_owned()),
            (1, "The second section".to_owned())
        ]
    );
    let links = links(&document);
    assert!(
        links
            .iter()
            .any(|(_, uri)| uri.as_deref() == Some("https://example.org/"))
    );
    assert!(
        links.iter().any(|(_, uri)| uri.is_none()),
        "a link within: {links:?}"
    );
    assert_eq!(images(&document).len(), 1);
    let all: String = (1..=made.pages)
        .map(|page| text(&pdf, &document, page, dir.path()))
        .collect();
    for expected in [
        "The first chapter",
        "First point",
        "A point within",
        "Something someone said once.",
        "println!",
        "A figure",
        "The second section",
        "item 69",
        "A note at the end.",
    ] {
        assert!(all.contains(expected), "{expected} in {all}");
    }
    // The table's header again over its rows on the next page.
    let last = text(&pdf, &document, made.pages, dir.path());
    assert!(last.contains("Amount"), "{last}");
    let kinds = structure(&document);
    for kind in [
        "H1",
        "H2",
        "P",
        "L",
        "LI",
        "Lbl",
        "LBody",
        "Table",
        "TR",
        "TH",
        "TD",
        "Figure",
        "Code",
        "BlockQuote",
        "Link",
    ] {
        assert!(kinds.contains_key(kind), "{kind} in {kinds:?}");
    }
}

#[test]
fn a_table_repeats_its_header_cuts_long_cells_and_goes_across_when_too_wide() {
    let Some(fonts) = fonts() else { return };
    let dir = scratch();
    let columns = vec!["Name".to_owned(), "Size".to_owned(), "Modified".to_owned()];
    let mut rows: Vec<Vec<String>> = (0..300)
        .map(|i| {
            vec![
                format!("file-{i}.txt"),
                format!("{} KiB", i * 3),
                "2026-10-31".into(),
            ]
        })
        .collect();
    rows[5][0] = "a name that goes on ".repeat(40);
    let job1 = job(
        dir.path(),
        fonts.clone(),
        vec![Part::Table { columns, rows }],
    );
    let (made, document, pdf) = ok(dir.path(), &job1);
    assert!(made.pages >= 2);
    let second = text(&pdf, &document, 2, dir.path());
    assert!(second.contains("Modified"), "{second}");
    let first = text(&pdf, &document, 1, dir.path());
    assert!(first.contains('…'), "a long cell cut");
    let kinds = structure(&document);
    assert!(
        kinds.contains_key("TH") && kinds.contains_key("TD"),
        "{kinds:?}"
    );
    // Thirty wide columns go across pages, the first on each.
    let columns: Vec<String> = (0..30).map(|i| format!("Column number {i}")).collect();
    let rows: Vec<Vec<String>> = (0..10)
        .map(|r| {
            (0..30)
                .map(|c| format!("value {r}-{c} of some width"))
                .collect()
        })
        .collect();
    let wide = job(dir.path(), fonts, vec![Part::Table { columns, rows }]);
    let (made, document, pdf) = ok(dir.path(), &wide);
    assert!(made.pages >= 2, "{made:?}");
    for page in 1..=made.pages {
        let text = text(&pdf, &document, page, dir.path());
        assert!(text.contains("Column number 0"), "page {page}: {text}");
    }
    let last = text(&pdf, &document, made.pages, dir.path());
    assert!(last.contains("Column number 29"));
}

#[test]
fn pictures_go_a_page_each_or_on_a_contact_sheet() {
    let Some(fonts) = fonts() else { return };
    let dir = scratch();
    let mut pictures = Vec::new();
    for i in 0..10u8 {
        let path = if i % 2 == 0 {
            let path = dir.path().join(format!("p{i}.png"));
            write_png(&path, 64 + u32::from(i), 48, i * 20);
            path
        } else {
            let path = dir.path().join(format!("p{i}.jpg"));
            write_jpeg(&path, 80 + u32::from(i), 60);
            path
        };
        pictures.push(Picture {
            path,
            width: 64 + u32::from(i),
            height: 48,
            alt: format!("Picture {i}"),
            caption: Some(format!("picture-{i}.jpg")),
        });
    }
    let sheet = job(
        dir.path(),
        fonts.clone(),
        vec![Part::Pictures {
            pictures: pictures.clone(),
            sheet: true,
        }],
    );
    let (made, document, pdf) = ok(dir.path(), &sheet);
    assert_eq!(made.pages, 1, "twelve to an upright page");
    let filters = images(&document);
    assert_eq!(filters.len(), 10, "{filters:?}");
    assert!(filters.iter().any(|f| f == "DCTDecode"), "a JPEG as it is");
    let page = text(&pdf, &document, 1, dir.path());
    assert!(page.contains("picture-9.jpg"), "{page}");
    assert_eq!(structure(&document).get("Figure"), Some(&10));
    let each = job(
        dir.path(),
        fonts.clone(),
        vec![Part::Pictures {
            pictures: pictures[..3].to_vec(),
            sheet: false,
        }],
    );
    let (made, _, _) = ok(dir.path(), &each);
    assert_eq!(made.pages, 3);
    let pages = job(
        dir.path(),
        fonts,
        vec![Part::Pages {
            pages: pictures[..2].to_vec(),
        }],
    );
    let (made, document, _) = ok(dir.path(), &pages);
    assert_eq!(made.pages, 2);
    assert_eq!(images(&document).len(), 2);
}

#[test]
fn a_drawing_stays_vector_and_reads_nothing_beside_it() {
    let Some(fonts) = fonts() else { return };
    let dir = scratch();
    // A picture beside the drawing, which the drawing names: not read.
    write_png(&dir.path().join("beside.png"), 10, 10, 0);
    let svg = dir.path().join("drawing.svg");
    std::fs::write(
        &svg,
        r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="200" height="100" viewBox="0 0 200 100">
  <rect x="10" y="10" width="180" height="80" fill="#0969da"/>
  <text x="20" y="60" font-family="sans-serif" font-size="20" fill="white">Hello drawing</text>
  <image x="0" y="0" width="10" height="10" xlink:href="beside.png"/>
  <image x="0" y="0" width="10" height="10" href="file:///etc/hostname"/>
</svg>"##,
    )
    .unwrap();
    let job = job(
        dir.path(),
        fonts,
        vec![Part::Drawing {
            svg,
            alt: "A blue box".into(),
        }],
    );
    let (made, document, pdf) = ok(dir.path(), &job);
    assert_eq!(made.pages, 1);
    assert!(images(&document).is_empty(), "nothing read beside it");
    let page = text(&pdf, &document, 1, dir.path());
    assert!(
        page.contains("Hello drawing"),
        "its text kept as text: {page}"
    );
    assert_eq!(structure(&document).get("Figure"), Some(&1));
}

#[test]
fn a_specimen_writes_the_sample_in_its_face_at_each_size() {
    let Some(fonts) = fonts() else { return };
    let dir = scratch();
    let face = fonts.code.regular.clone().unwrap();
    let job = job(
        dir.path(),
        fonts,
        vec![Part::Specimen {
            face,
            name: "A monospaced face".into(),
            sample: "The quick brown fox jumps over the lazy dog".into(),
            sizes: vec![9.0, 12.0, 24.0, 48.0, 72.0],
        }],
    );
    let (made, document, pdf) = ok(dir.path(), &job);
    assert!(made.pages >= 1);
    let page = text(&pdf, &document, 1, dir.path());
    assert!(page.contains("A monospaced face"));
    assert!(page.contains("12 pt"));
    assert!(page.contains("quick brown fox"));
    assert_eq!(outline(&document), [(0, "A monospaced face".to_owned())]);
}

#[test]
fn a_character_no_role_font_has_is_found_in_the_fallback_folders() {
    let Some(fonts) = fonts() else { return };
    let Some(cjk) = matched(":lang=ja") else {
        eprintln!("no Noto CJK font found by fc-match: not tried");
        return;
    };
    let dir = scratch();
    let mut job = job(
        dir.path(),
        fonts,
        vec![Part::Flow {
            blocks: vec![block("Kanji: 漢字 and more")],
        }],
    );
    job.fallback = vec![cjk.path.parent().unwrap().to_owned()];
    let (_, document, pdf) = ok(dir.path(), &job);
    let page = text(&pdf, &document, 1, dir.path());
    assert!(page.contains("漢字"), "{page}");
}

/// The names of the fonts embedded.
fn font_names(document: &Document) -> Vec<String> {
    let mut names: Vec<String> = document
        .objects
        .values()
        .filter_map(|object| object.as_dict().ok())
        .filter(|dict| dict.get(b"Type").and_then(Object::as_name).ok() == Some(b"Font".as_slice()))
        .filter(|dict| {
            dict.get(b"Subtype").and_then(Object::as_name).ok() != Some(b"Type0".as_slice())
        })
        .filter_map(|dict| dict.get(b"BaseFont").and_then(Object::as_name).ok())
        .map(|name| {
            let name = String::from_utf8_lossy(name);
            // A subset's tag, `ABCDEF+`, left out.
            name.split_once('+')
                .map_or(name.to_string(), |(_, rest)| rest.to_owned())
        })
        .collect();
    names.sort();
    names.dedup();
    names
}

#[test]
fn a_face_the_family_has_is_used_and_one_it_lacks_is_made_up() {
    let Some(fonts) = fonts() else { return };
    let dir = scratch();
    let styled = Block {
        runs: vec![
            Run {
                start: 0,
                end: 4,
                italic: true,
                ..Run::default()
            },
            Run {
                start: 5,
                end: 9,
                bold: true,
                ..Run::default()
            },
        ],
        ..block("Some text, styled")
    };
    let whole = job(
        dir.path(),
        fonts.clone(),
        vec![Part::Flow {
            blocks: vec![styled.clone()],
        }],
    );
    let (_, document, _) = ok(dir.path(), &whole);
    let names = font_names(&document);
    assert_eq!(names.len(), 3, "regular, bold and italic faces: {names:?}");
    // Its regular face alone: bold and italic made from it.
    let mut alone = fonts;
    alone.text.bold = None;
    alone.text.italic = None;
    alone.text.bold_italic = None;
    let lacking = job(
        dir.path(),
        alone,
        vec![Part::Flow {
            blocks: vec![styled],
        }],
    );
    let (_, document, pdf) = ok(dir.path(), &lacking);
    let names = font_names(&document);
    assert_eq!(names.len(), 1, "{names:?}");
    let page = text(&pdf, &document, 1, dir.path());
    assert!(page.contains("Some text, styled"), "{page}");
}

#[test]
fn a_job_too_large_or_unreadable_is_refused_in_a_line() {
    let dir = scratch();
    let path = dir.path().join("job.json");
    std::fs::write(&path, b"{ not a job").unwrap();
    let (message, code) = run_file(&path, &dir.path().join("out.pdf")).unwrap_err();
    assert_eq!(code, 1);
    assert_eq!(message.trim_end().lines().count(), 1, "{message}");
    // Sparse: nothing written but its length.
    let file = std::fs::File::create(&path).unwrap();
    file.set_len(512 * 1024 * 1024 + 1).unwrap();
    let (message, code) = run_file(&path, &dir.path().join("out.pdf")).unwrap_err();
    assert_eq!(code, 1);
    assert!(message.contains("more than"), "{message}");
}
