use super::*;
use crate::{
    Cover, Provider,
    tests::{fixture, request},
};
use skwir_core::{
    inspection::{InspectRequest, Inspection},
    op::EntryRef,
    preview::PreviewProvider,
    thumbs::{ThumbRequest, ThumbnailProvider},
};
use std::path::Path;

fn inspect(path: &Path, view: &InspectRequest) -> anyhow::Result<Inspection> {
    Provider::default()
        .inspect(&request(path), view, &CancelToken::new())
        .map(Option::unwrap)
}

fn text(shown: &Inspection) -> String {
    shown
        .blocks
        .iter()
        .map(|block| block.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn fb2(body: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?><FictionBook xmlns="http://www.gribuser.ru/xml/fictionbook/2.0" xmlns:l="http://www.w3.org/1999/xlink"><description><title-info><book-title>Synthetic</book-title></title-info></description>{body}</FictionBook>"#
    )
}

fn write(dir: &Path, name: &str, text: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path
}

/// A book's parts are its chapters, its notes one more; its contents are
/// its sections' titles, nested as they are, each row knowing its chapter;
/// what it says of itself is in Properties.
#[test]
fn a_fictionbook_reads_as_chapters_with_its_contents() {
    let shown = inspect(&fixture("synthetic.fb2"), &InspectRequest::default()).unwrap();
    assert_eq!(shown.title, "FB2 · chapter 1 / 3");
    assert_eq!(shown.sections, 3);
    let rows: Vec<(&str, usize, Option<u64>)> = shown
        .rows
        .iter()
        .map(|row| (row.cells[0].as_str(), row.depth, row.section))
        .collect();
    assert_eq!(
        rows,
        [
            ("Part One", 0, Some(0)),
            ("The First Chapter", 1, Some(0)),
            ("The Second Chapter", 1, Some(0)),
            ("Part Two", 0, Some(1)),
            ("The Third Chapter", 1, Some(1)),
            ("Notes", 0, Some(2)),
        ]
    );
    let read = text(&shown);
    // The book's title and epigraph open the first chapter.
    assert!(read.starts_with("Synthetic Book"), "{read}");
    assert!(read.contains("An epigraph before it all."), "{read}");
    assert!(read.contains("The needle is here"), "{read}");
    // A title of two lines is one heading.
    let heading = shown
        .blocks
        .iter()
        .find(|block| block.text.starts_with("The First"))
        .unwrap();
    assert!(
        heading.heading > 0 && heading.text.contains('\n'),
        "{heading:?}"
    );
    // Emphasis and the like are runs of their own.
    let second = shown
        .blocks
        .iter()
        .find(|block| block.text.contains("emphasis"))
        .unwrap();
    assert!(second.runs.iter().any(|run| run.italic));
    assert!(second.runs.iter().any(|run| run.bold));
    assert!(second.runs.iter().any(|run| run.strikethrough));
    let details: Vec<(&str, &str)> = shown
        .details
        .iter()
        .map(|d| (d.label.as_str(), d.value.as_str()))
        .collect();
    assert_eq!(
        details,
        [
            ("Title", "Synthetic Book"),
            ("Author", "Synthetic Author"),
            ("Series", "Synthetic Series, 2"),
            ("Genre", "prose_contemporary"),
            ("Language", "en"),
            ("Publisher", "Synthetic Press"),
            ("Year", "2026"),
            ("Chapters", "3"),
        ]
    );
    // A row leads to its section, whichever chapter holds it.
    let third = shown
        .rows
        .iter()
        .find(|row| row.cells[0] == "The Third Chapter")
        .unwrap();
    let shown = inspect(
        &fixture("synthetic.fb2"),
        &InspectRequest {
            target: third.target.clone(),
            ..InspectRequest::default()
        },
    )
    .unwrap();
    assert_eq!(shown.position, 1);
    assert!(text(&shown).contains("Another needle"));
}

/// A note reference opens the note in the body of notes, its own chapter.
#[test]
fn a_note_reference_leads_to_its_note_in_the_notes() {
    let path = fixture("synthetic.fb2");
    let shown = inspect(&path, &InspectRequest::default()).unwrap();
    let reference = shown
        .blocks
        .iter()
        .flat_map(|block| &block.runs)
        .find(|run| run.note)
        .expect("a note reference");
    let target = reference.link.clone().unwrap();
    assert_eq!(target.trim_start_matches('/'), "chapter-3#n1");
    let notes = inspect(
        &path,
        &InspectRequest {
            target: Some(target),
            ..InspectRequest::default()
        },
    )
    .unwrap();
    assert_eq!(notes.position, 2);
    let note = notes
        .blocks
        .iter()
        .find(|block| block.anchors.iter().any(|anchor| anchor == "n1"))
        .expect("the note's anchor");
    assert!(note.note.is_some(), "{note:?}");
    assert!(text(&notes).contains("The note's own words."));
    // A link within a chapter stays in it.
    let back = shown
        .blocks
        .iter()
        .flat_map(|block| &block.runs)
        .find(|run| run.link.as_deref() == Some("#first"));
    assert!(back.is_some(), "{:?}", shown.blocks);
}

/// A picture is named by its binary's id with the size it is shown at,
/// and decoded only when asked for.
#[test]
fn pictures_are_named_by_their_binary_and_decoded_when_asked() {
    let path = fixture("synthetic.fb2");
    let shown = inspect(&path, &InspectRequest::default()).unwrap();
    let pictures: Vec<_> = shown
        .blocks
        .iter()
        .filter_map(|block| block.picture.as_ref())
        .collect();
    let [Picture::Later { key, width, height }] = pictures.as_slice() else {
        panic!("{pictures:?}");
    };
    assert_eq!((key.as_str(), *width, *height), ("picture.png", 40, 30));
    let image = Provider::default()
        .picture(
            &request(&path),
            &InspectRequest::default(),
            key,
            &CancelToken::new(),
        )
        .unwrap()
        .unwrap();
    assert_eq!((image.width, image.height), (40, 30));
    let cover = |path: &Path| {
        let req = ThumbRequest {
            entry: EntryRef::from_path(path).unwrap(),
            mime: None,
            mtime: None,
            size: skwir_core::thumbs::ThumbSize::Normal,
        };
        assert!(Cover.accepts(&req), "{}", path.display());
        Cover
            .thumbnail(&req, &CancelToken::new())
            .unwrap()
            .map(|image| (image.width, image.height))
    };
    let (width, height) = cover(&path).expect("the cover the book names");
    assert!(height > width, "{width} × {height}");
    assert_eq!(cover(&fixture("synthetic.fb2.zip")), Some((width, height)));
}

/// A search starts at the chapter asked and wraps; a chapter's source is
/// the book's own XML.
#[test]
fn search_wraps_and_the_source_is_the_book_s_xml() {
    let path = fixture("synthetic.fb2");
    let find = |offset: u64| {
        inspect(
            &path,
            &InspectRequest {
                query: Some("needle".into()),
                offset,
                ..InspectRequest::default()
            },
        )
        .unwrap()
        .position
    };
    assert_eq!(find(0), 0);
    assert_eq!(find(1), 1);
    assert_eq!(find(2), 0);
    let source = inspect(
        &path,
        &InspectRequest {
            source: true,
            ..InspectRequest::default()
        },
    )
    .unwrap();
    let code = text(&source);
    assert!(code.contains(r#"<section id="part1">"#), "{code}");
    assert!(!code.contains("<binary"), "{code}");
}

/// A book in Windows-1251 reads as its declaration says, and says so; a
/// zipped FB2 reads as the FB2 in it.
#[test]
fn a_declared_encoding_and_a_zipped_book_read_as_the_book() {
    let shown = inspect(&fixture("synthetic-1251.fb2"), &InspectRequest::default()).unwrap();
    assert!(text(&shown).contains("Глава первая"), "{}", text(&shown));
    assert!(
        shown.notice.contains("Read as windows-1251"),
        "{}",
        shown.notice
    );
    assert_eq!(shown.rows[0].cells[0], "Глава первая");
    let zipped = inspect(&fixture("synthetic.fb2.zip"), &InspectRequest::default()).unwrap();
    let plain = inspect(&fixture("synthetic.fb2"), &InspectRequest::default()).unwrap();
    assert_eq!(text(&zipped), text(&plain));
    let cells = |shown: &Inspection| {
        shown
            .rows
            .iter()
            .map(|row| row.cells.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(cells(&zipped), cells(&plain));
    // calibre's FB2 of the synthetic EPUB.
    let calibre = inspect(
        &fixture("synthetic-calibre.fb2"),
        &InspectRequest::default(),
    )
    .unwrap();
    assert!(calibre.sections >= 1);
    let all = (0..calibre.sections)
        .map(|offset| {
            text(
                &inspect(
                    &fixture("synthetic-calibre.fb2"),
                    &InspectRequest {
                        offset,
                        ..InspectRequest::default()
                    },
                )
                .unwrap(),
            )
        })
        .collect::<String>();
    assert!(all.contains("The needle is here"), "{all}");
}

/// A book in one part reads by the part's sections, the part's title
/// opening the first; a body of paragraphs alone is one chapter.
#[test]
fn a_book_in_one_part_reads_by_its_sections() {
    let tmp = tempfile::tempdir().unwrap();
    let part = write(
        tmp.path(),
        "Synthetic part's book.fb2",
        &fb2("<body><section><title><p>The Part</p></title>\
             <section><title><p>One</p></title><p>First words.</p></section>\
             <section><title><p>Two</p></title><p>Second words.</p></section>\
             <section><p>Untitled words.</p><section><title><p>Inner</p></title><p>Inner words.</p></section></section>\
             </section></body>"),
    );
    let shown = inspect(&part, &InspectRequest::default()).unwrap();
    assert_eq!(shown.sections, 3);
    assert!(text(&shown).starts_with("The Part"), "{}", text(&shown));
    let rows: Vec<(&str, usize, Option<u64>)> = shown
        .rows
        .iter()
        .map(|row| (row.cells[0].as_str(), row.depth, row.section))
        .collect();
    // An untitled section adds no row and no depth.
    assert_eq!(
        rows,
        [
            ("The Part", 0, Some(0)),
            ("One", 1, Some(0)),
            ("Two", 1, Some(1)),
            ("Inner", 1, Some(2)),
        ]
    );
    let plain = write(
        tmp.path(),
        "Synthetic plain book.fb2",
        &fb2("<body><p>Only words.</p><p>More words.</p></body>"),
    );
    let shown = inspect(&plain, &InspectRequest::default()).unwrap();
    assert_eq!(shown.sections, 1);
    assert_eq!(text(&shown), "Only words.\nMore words.");
    // Its one row names the chapter it has.
    assert_eq!(shown.rows.len(), 1);
}

/// What is not an FB2, nests deeper than the parser goes or holds a
/// damaged picture fails or shows less; nothing panics.
#[test]
fn a_damaged_book_fails_or_shows_less() {
    let tmp = tempfile::tempdir().unwrap();
    let other = write(tmp.path(), "Synthetic other.fb2", "<html><body/></html>");
    assert!(
        inspect(&other, &InspectRequest::default())
            .unwrap_err()
            .to_string()
            .contains("not an FB2")
    );
    // Deeper than the parser goes: refused before it is parsed.
    let deeper = write(
        tmp.path(),
        "Synthetic deeper.fb2",
        &fb2(&format!(
            "<body>{}{}</body>",
            "<section>".repeat(600),
            "</section>".repeat(600)
        )),
    );
    assert!(inspect(&deeper, &InspectRequest::default()).is_err());
    // Deep, but read: what is deeper than the writer goes is its text.
    let deep = write(
        tmp.path(),
        "Synthetic deep.fb2",
        &fb2(&format!(
            "<body><section><p>Shallow words.</p></section><section>{}<p>Deep <emphasis>words</emphasis>.</p>{}</section></body>",
            "<section><title><p>Level</p></title>".repeat(400),
            "</section>".repeat(400)
        )),
    );
    let shown = inspect(
        &deep,
        &InspectRequest {
            offset: 1,
            ..InspectRequest::default()
        },
    )
    .unwrap();
    assert!(text(&shown).contains("Deep words."), "{}", text(&shown));
    assert!(shown.rows.len() <= limits::BLOCKS);
    let damaged = write(
        tmp.path(),
        "Synthetic damaged picture.fb2",
        &fb2(
            r##"<body><section><p>Words <image l:href="#bad"/></p></section></body><binary id="bad" content-type="image/png">!!!not base64!!!</binary>"##,
        ),
    );
    let shown = inspect(&damaged, &InspectRequest::default()).unwrap();
    assert!(text(&shown).contains("Words"));
    assert!(shown.blocks.iter().all(|block| block.picture.is_none()));
    assert!(
        Provider::default()
            .picture(
                &request(&damaged),
                &InspectRequest::default(),
                "bad",
                &CancelToken::new()
            )
            .is_err()
    );
}

/// The book read is kept while it is shown, and read again once it
/// changes.
#[test]
fn a_changed_book_is_read_again() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write(
        tmp.path(),
        "Synthetic changing book.fb2",
        &fb2("<body><section><p>Before.</p></section></body>"),
    );
    let provider = Provider::default();
    let read = |provider: &Provider| {
        text(
            &provider
                .inspect(
                    &request(&path),
                    &InspectRequest::default(),
                    &CancelToken::new(),
                )
                .unwrap()
                .unwrap(),
        )
    };
    assert_eq!(read(&provider), "Before.");
    std::fs::write(
        &path,
        fb2("<body><section><p>After, longer.</p></section></body>"),
    )
    .unwrap();
    assert_eq!(read(&provider), "After, longer.");
}
