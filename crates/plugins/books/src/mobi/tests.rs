use super::*;
use crate::{
    Cover, Engine, Provider,
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

/// Every chapter's text, in order.
fn whole(path: &Path) -> String {
    let first = inspect(path, &InspectRequest::default()).unwrap();
    (0..first.sections)
        .map(|offset| {
            text(
                &inspect(
                    path,
                    &InspectRequest {
                        offset,
                        ..InspectRequest::default()
                    },
                )
                .unwrap(),
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The fixtures calibre made of the same book.
const BOOKS: [(&str, &str); 4] = [
    ("synthetic.mobi", "MOBI"),
    ("synthetic-uncompressed.mobi", "MOBI"),
    ("synthetic-both.mobi", "MOBI"),
    ("synthetic.azw3", "AZW3"),
];

/// MOBI 6 (compressed or not), a file holding MOBI 6 and KF8 (read
/// through its KF8 part) and AZW3 read whole, every chapter, in the
/// encoding they are in, with the book's contents and what it says of
/// itself.
#[test]
fn each_kind_of_mobipocket_book_reads_whole_with_its_contents() {
    for (name, format) in BOOKS {
        let path = fixture(name);
        let first = inspect(&path, &InspectRequest::default()).unwrap();
        assert!(
            first.title.starts_with(&format!("{format} · chapter 1 / ")),
            "{name}: {}",
            first.title
        );
        let all = whole(&path);
        for words in [
            "The needle is here",
            "Café, naïve and “quoted” words",
            "w00n00é—",
            // The long chapter's last word: every text record was read.
            "w39n23é—",
            "Another needle",
            "The note's own words.",
        ] {
            assert!(all.contains(words), "{name}: {words} in {all}");
        }
        let rows: Vec<(&str, usize)> = first
            .rows
            .iter()
            .map(|row| (row.cells[0].as_str(), row.depth))
            .collect();
        if format == "AZW3" || name.contains("both") {
            // KF8's contents, listed a level at a time, read as a tree.
            assert_eq!(
                rows,
                [
                    ("Part One", 0),
                    ("The First Chapter", 1),
                    ("The Second Chapter", 1),
                    ("The Long Chapter", 1),
                    ("Part Two", 0),
                    ("The Third Chapter", 1),
                    ("Notes", 0),
                ],
                "{name}"
            );
        } else {
            // calibre's MOBI 6 contents keep one entry for each place.
            assert_eq!(
                rows,
                [
                    ("Part One", 0),
                    ("The Second Chapter", 0),
                    ("The Long Chapter", 0),
                    ("Part Two", 0),
                    ("Notes", 0),
                ],
                "{name}"
            );
        }
        assert!(
            first.rows.iter().all(|row| row.section.is_some()),
            "{name}: {:?}",
            first.rows
        );
        let details: Vec<(&str, &str)> = first
            .details
            .iter()
            .map(|d| (d.label.as_str(), d.value.as_str()))
            .collect();
        for fact in [
            ("Title", "Synthetic Book"),
            ("Author", "Synthetic Author"),
            ("Publisher", "Synthetic Press"),
            ("Language", "en"),
        ] {
            assert!(details.contains(&fact), "{name}: {details:?}");
        }
    }
}

/// A contents entry and a link lead to the chapter and place they name; a
/// note's reference to the note.
#[test]
fn contents_and_links_lead_to_their_place() {
    for (name, _) in BOOKS {
        let path = fixture(name);
        let first = inspect(&path, &InspectRequest::default()).unwrap();
        let at = |target: Option<String>| {
            inspect(
                &path,
                &InspectRequest {
                    target,
                    ..InspectRequest::default()
                },
            )
            .unwrap()
        };
        let third = first
            .rows
            .iter()
            .find(|row| row.cells[0] == "Part Two")
            .unwrap();
        let shown = at(third.target.clone());
        assert_eq!(Some(shown.position), third.section, "{name}");
        assert!(
            text(&shown).contains("Another needle"),
            "{name}: {}",
            text(&shown)
        );
        // The note reference in the first chapter.
        let chapter = (0..first.sections)
            .map(|offset| {
                inspect(
                    &path,
                    &InspectRequest {
                        offset,
                        ..InspectRequest::default()
                    },
                )
                .unwrap()
            })
            .find(|shown| text(shown).contains("The needle is here"))
            .unwrap();
        let links: Vec<String> = chapter
            .blocks
            .iter()
            .flat_map(|block| &block.runs)
            .filter_map(|run| run.link.clone())
            .collect();
        assert!(!links.is_empty(), "{name}");
        let note = links
            .iter()
            .map(|link| at(Some(link.clone())))
            .find(|shown| text(shown).contains("The note's own words."));
        assert!(note.is_some(), "{name}: {links:?}");
        // Back to the first chapter, from the second.
        let second = first
            .rows
            .iter()
            .find(|row| row.cells[0] == "The Second Chapter")
            .unwrap();
        let second = at(second.target.clone());
        let links: Vec<String> = second
            .blocks
            .iter()
            .flat_map(|block| &block.runs)
            .filter_map(|run| run.link.clone())
            .collect();
        let [back] = links.as_slice() else {
            panic!("{name}: {links:?}");
        };
        // In another chapter: named by it.
        assert!(!back.starts_with('#'), "{name}: {back}");
        assert!(
            text(&at(Some(back.clone()))).contains("The needle is here"),
            "{name}: {back}"
        );
    }
}

/// A picture is named by its record with its size, and read when asked;
/// the cover is the grid's thumbnail.
#[test]
fn pictures_and_covers_are_the_book_s_records() {
    for (name, _) in BOOKS {
        let path = fixture(name);
        let first = inspect(&path, &InspectRequest::default()).unwrap();
        let pictures: Vec<(String, u32, u32)> = (0..first.sections)
            .flat_map(|offset| {
                inspect(
                    &path,
                    &InspectRequest {
                        offset,
                        ..InspectRequest::default()
                    },
                )
                .unwrap()
                .blocks
            })
            .filter_map(|block| match block.picture {
                Some(Picture::Later { key, width, height }) => Some((key, width, height)),
                _ => None,
            })
            .collect();
        let [(key, 40, 30)] = pictures.as_slice() else {
            panic!("{name}: {pictures:?}");
        };
        let image = Provider::default()
            .picture(
                &request(&path),
                &InspectRequest::default(),
                key,
                &CancelToken::new(),
            )
            .unwrap()
            .unwrap();
        assert_eq!((image.width, image.height), (40, 30), "{name}");
        // Only a picture's record: never the text's.
        assert!(
            Provider::default()
                .picture(
                    &request(&path),
                    &InspectRequest::default(),
                    "1",
                    &CancelToken::new()
                )
                .is_err(),
            "{name}"
        );
        let req = ThumbRequest {
            entry: EntryRef::from_path(&path).unwrap(),
            mime: None,
            mtime: None,
            size: skwir_core::thumbs::ThumbSize::Normal,
        };
        assert!(Cover.accepts(&req), "{name}");
        let cover = Cover.thumbnail(&req, &CancelToken::new()).unwrap().unwrap();
        assert!(
            cover.height > cover.width,
            "{name}: {} × {}",
            cover.width,
            cover.height
        );
    }
}

/// A `PalmDOC` text reads as its paragraphs, titled by its database's
/// name.
#[test]
fn a_palmdoc_text_reads_as_paragraphs() {
    let mut req = request(&fixture("synthetic.pdb"));
    req.mime = Some("application/vnd.palm".into());
    let shown = Provider::default()
        .inspect(&req, &InspectRequest::default(), &CancelToken::new())
        .unwrap()
        .unwrap();
    assert_eq!(shown.title, "PalmDOC · chapter 1 / 1");
    let read = text(&shown);
    assert!(
        read.contains("The needle is here, in a plain PalmDOC text."),
        "{read}"
    );
    assert!(read.contains("A second paragraph"), "{read}");
    assert!(
        shown
            .details
            .iter()
            .any(|d| d.label == "Title" && d.value == "Synthetic Text")
    );
}

/// A Palm database of `records` named `kind` (type and creator).
fn database(kind: [u8; 8], records: &[Vec<u8>]) -> Vec<u8> {
    let mut out = vec![0u8; 78];
    out[..15].copy_from_slice(b"Synthetic_Book\0");
    out[60..68].copy_from_slice(&kind);
    out[76..78].copy_from_slice(&u16::try_from(records.len()).unwrap().to_be_bytes());
    let mut offset = 78 + 8 * records.len() + 2;
    for (k, record) in records.iter().enumerate() {
        out.extend_from_slice(&u32::try_from(offset).unwrap().to_be_bytes());
        out.extend_from_slice(&u32::try_from(k).unwrap().to_be_bytes());
        offset += record.len();
    }
    out.extend_from_slice(&[0, 0]);
    for record in records {
        out.extend_from_slice(record);
    }
    out
}

/// A first record: `PalmDOC`'s header and a MOBI header (UTF-8, version
/// 6) with `fields` set at their offsets in the record.
fn first_record(compression: u16, text: usize, records: u16, fields: &[(usize, u32)]) -> Vec<u8> {
    let mut record = vec![0u8; 16 + 232];
    record[0..2].copy_from_slice(&compression.to_be_bytes());
    record[4..8].copy_from_slice(&u32::try_from(text).unwrap().to_be_bytes());
    record[8..10].copy_from_slice(&records.to_be_bytes());
    record[10..12].copy_from_slice(&4096u16.to_be_bytes());
    record[16..20].copy_from_slice(b"MOBI");
    record[20..24].copy_from_slice(&232u32.to_be_bytes());
    record[28..32].copy_from_slice(&65001u32.to_be_bytes());
    record[36..40].copy_from_slice(&6u32.to_be_bytes());
    for at in [0x50, 0x6C, 0x70, 0xF4] {
        record[at..at + 4].copy_from_slice(&u32::MAX.to_be_bytes());
    }
    for &(at, value) in fields {
        record[at..at + 4].copy_from_slice(&value.to_be_bytes());
    }
    record
}

/// Huffman-coded text, and text records with what trails them (a
/// character going on in the next record, an indexing entry), read as the
/// book wrote them.
#[test]
fn huffman_coded_and_trailing_bytes_read_as_written() {
    let tmp = tempfile::tempdir().unwrap();
    let (huff, cdic, coded) = huffcdic::encode(
        &[b"<p>The needle", b" is here", b".</p>", b"<mbp:pagebreak/>"],
        &[&[0, 1, 2]],
        &[4, 3, 0, 1, 1, 2],
    );
    let html = "<p>The needle is here.</p><mbp:pagebreak/><p>The needle is here is here.</p>";
    let first = first_record(17480, html.len(), 1, &[(0x70, 2), (0x74, 2)]);
    let path = tmp.path().join("Synthetic Huffman book.mobi");
    std::fs::write(&path, database(*b"BOOKMOBI", &[first, coded, huff, cdic])).unwrap();
    assert_eq!(
        whole(&path),
        "The needle is here.\nThe needle is here is here."
    );
    // Two records, each with a trailing entry (flag 2) of three bytes, the
    // first also ending on the first byte of an `é` the second goes on
    // with (flag 1: one byte).
    let text = "<p>Caf\u{e9} au lait.</p>".as_bytes();
    let split = text.iter().position(|&b| b == 0xC3).unwrap() + 1;
    let (one, two) = text.split_at(split);
    // The multibyte entry: the bytes of a character the next record goes
    // on with, copied, then their count; a record with none says 0.
    let trail = |body: &[u8], overlap: Option<u8>| {
        let mut record = body.to_vec();
        match overlap {
            Some(0) => record.push(0),
            Some(byte) => record.extend_from_slice(&[byte, 1]),
            None => {}
        }
        record.extend_from_slice(&[0xAA, 0xBB, 0x83]);
        record
    };
    let record_one = trail(one, Some(two[0]));
    let mut first = first_record(1, text.len(), 2, &[]);
    first[0xF2..0xF4].copy_from_slice(&3u16.to_be_bytes());
    let path = tmp.path().join("Synthetic trailing book.mobi");
    std::fs::write(
        &path,
        database(*b"BOOKMOBI", &[first, record_one, trail(two, Some(0))]),
    )
    .unwrap();
    assert_eq!(whole(&path), "Café au lait.");
}

/// A book whose text is encrypted, and a KFX book, say what they are; a
/// damaged one fails; nothing panics.
#[test]
fn drm_kfx_and_damage_say_so() {
    let tmp = tempfile::tempdir().unwrap();
    let mut bytes = std::fs::read(fixture("synthetic.mobi")).unwrap();
    let first = u32::from_be_bytes(bytes[78..82].try_into().unwrap()) as usize;
    bytes[first + 12..first + 14].copy_from_slice(&2u16.to_be_bytes());
    let protected = tmp.path().join("Synthetic protected book.mobi");
    std::fs::write(&protected, &bytes).unwrap();
    let error = inspect(&protected, &InspectRequest::default())
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("protected") && error.contains("DRM"),
        "{error}"
    );
    let kfx = tmp.path().join("Synthetic book.kfx");
    std::fs::write(&kfx, b"CONT\x02\x00\x12\x00\x00\x00 synthetic").unwrap();
    let error = inspect(&kfx, &InspectRequest::default())
        .unwrap_err()
        .to_string();
    assert!(error.contains("KFX"), "{error}");
    let short = tmp.path().join("Synthetic short book.mobi");
    std::fs::write(
        &short,
        &std::fs::read(fixture("synthetic.mobi")).unwrap()[..2000],
    )
    .unwrap();
    assert!(inspect(&short, &InspectRequest::default()).is_err());
}

/// With `MuPDF` chosen, what it reads is a document; a KF8 book, which it
/// does not read, is the built-in engine's.
#[test]
fn mupdf_leaves_kf8_to_the_built_in_engine() {
    let mupdf = Provider {
        engine: Engine::Mupdf,
        ..Provider::default()
    };
    let cancel = CancelToken::new();
    let read = |name: &str| {
        mupdf
            .inspect(
                &request(&fixture(name)),
                &InspectRequest::default(),
                &cancel,
            )
            .unwrap()
    };
    assert!(read("synthetic.mobi").is_none());
    let azw3 = read("synthetic.azw3").expect("the built-in engine's");
    assert!(azw3.title.starts_with("AZW3"));
    assert!(read("synthetic.fb2.zip").is_some());
}
