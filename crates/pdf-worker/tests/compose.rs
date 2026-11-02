//! The helper composes a new document of a PDF's pages for printing
//! (ADR-111): pages in the order asked, repeated, left out, blanks the
//! size of the page before. Needs `PDFium` (`SKWIR_PDFIUM_LIBRARY`, which the
//! dev shell sets): skipped, saying so, without.

use pdfium_render::prelude::*;
use skwir_core::document::{COMPOSED_FILE, Compose, Tagged, WorkerReply, WorkerRequest};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};

fn pdfium() -> Option<Pdfium> {
    let library = std::env::var_os("SKWIR_PDFIUM_LIBRARY")?;
    Some(Pdfium::new(Pdfium::bind_to_library(library).ok()?))
}

/// A PDF of three pages of three sizes, each a size of its own to be told
/// apart by.
fn three_pages(pdfium: &Pdfium, path: &Path) {
    let mut document = pdfium.create_new_pdf().unwrap();
    for (width, height) in [(200.0, 300.0), (400.0, 300.0), (250.0, 250.0)] {
        document
            .pages_mut()
            .create_page_at_end(PdfPagePaperSize::Custom(
                PdfPoints::new(width),
                PdfPoints::new(height),
            ))
            .unwrap();
    }
    document.save_to_file(path).unwrap();
}

fn sizes(pdfium: &Pdfium, path: &Path) -> Vec<(f32, f32)> {
    let document = pdfium.load_pdf_from_file(path, None).unwrap();
    document
        .pages()
        .iter()
        .map(|page| (page.width().value, page.height().value))
        .collect()
}

#[test]
fn pages_are_composed_in_the_order_asked_with_blanks() {
    let Some(pdfium) = pdfium() else {
        eprintln!("skipped: no PDFium (SKWIR_PDFIUM_LIBRARY)");
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let (pdf, scratch) = (tmp.path().join("three.pdf"), tmp.path().join("work"));
    std::fs::create_dir(&scratch).unwrap();
    three_pages(&pdfium, &pdf);
    let mut worker = Command::new(env!("CARGO_BIN_EXE_skwir-pdf-worker"))
        .arg(&pdf)
        .arg(&scratch)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = worker.stdin.take().unwrap();
    let mut output = BufReader::new(worker.stdout.take().unwrap());
    let mut read = || -> Tagged<WorkerReply> {
        let mut line = String::new();
        output.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    };
    assert!(matches!(read().body, WorkerReply::Ready(_)));
    let mut ask = |id: u64, request: WorkerRequest| {
        let line = serde_json::to_string(&Tagged { id, body: request }).unwrap();
        writeln!(input, "{line}").unwrap();
    };
    // The third page, a blank of its size, the first twice.
    ask(
        1,
        WorkerRequest::Compose(Compose::Pages {
            pages: vec![Some(2), None, Some(0), Some(0)],
            scale: 1.0,
        }),
    );
    let reply = read();
    assert_eq!(reply.id, 1);
    assert!(
        matches!(reply.body, WorkerReply::Composed { pages: 4 }),
        "{:?}",
        reply.body
    );
    let composed = scratch.join(COMPOSED_FILE);
    assert_eq!(
        sizes(&pdfium, &composed),
        [
            (250.0, 250.0),
            (250.0, 250.0),
            (200.0, 300.0),
            (200.0, 300.0)
        ]
    );
    // A page past the end is that request's error; the helper goes on.
    ask(
        2,
        WorkerRequest::Compose(Compose::Pages {
            pages: vec![Some(3)],
            scale: 1.0,
        }),
    );
    let reply = read();
    assert!(
        matches!(&reply.body, WorkerReply::Error(message) if message.contains("past the end")),
        "{:?}",
        reply.body
    );
    ask(
        3,
        WorkerRequest::Compose(Compose::Pages {
            pages: vec![Some(1)],
            scale: 1.0,
        }),
    );
    assert!(matches!(read().body, WorkerReply::Composed { pages: 1 }));
    assert_eq!(sizes(&pdfium, &composed), [(400.0, 300.0)]);
    // Printing on a printer is Windows' alone.
    ask(
        4,
        WorkerRequest::Print {
            printer: "Office".into(),
            devmode: Vec::new(),
            title: "three".into(),
        },
    );
    assert!(matches!(read().body, WorkerReply::Error(_)));
    // Several documents printed as one: its pages, then the others' in
    // their order; one that cannot be read is that request's error.
    let other = tmp.path().join("other.pdf");
    let mut document = pdfium.create_new_pdf().unwrap();
    document
        .pages_mut()
        .create_page_at_end(PdfPagePaperSize::Custom(
            PdfPoints::new(100.0),
            PdfPoints::new(120.0),
        ))
        .unwrap();
    document.save_to_file(&other).unwrap();
    ask(
        5,
        WorkerRequest::Compose(Compose::Append(vec![other.clone(), pdf.clone()])),
    );
    assert!(matches!(read().body, WorkerReply::Composed { pages: 7 }));
    assert_eq!(
        sizes(&pdfium, &composed),
        [
            (200.0, 300.0),
            (400.0, 300.0),
            (250.0, 250.0),
            (100.0, 120.0),
            (200.0, 300.0),
            (400.0, 300.0),
            (250.0, 250.0)
        ]
    );
    ask(
        6,
        WorkerRequest::Compose(Compose::Append(vec![tmp.path().join("missing.pdf")])),
    );
    assert!(matches!(read().body, WorkerReply::Error(_)));
    drop(input);
    worker.wait().unwrap();
}

/// A worker on `pdf`, its scratch folder `scratch`, greeted.
struct Worker {
    child: std::process::Child,
    input: std::process::ChildStdin,
    output: BufReader<std::process::ChildStdout>,
    next: u64,
}

impl Worker {
    fn start(pdf: &Path, scratch: &Path) -> Worker {
        let mut child = Command::new(env!("CARGO_BIN_EXE_skwir-pdf-worker"))
            .arg(pdf)
            .arg(scratch)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        let mut worker = Worker {
            child,
            input,
            output,
            next: 1,
        };
        assert!(matches!(worker.read().body, WorkerReply::Ready(_)));
        worker
    }

    fn read(&mut self) -> Tagged<WorkerReply> {
        let mut line = String::new();
        self.output.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    }

    fn ask(&mut self, request: WorkerRequest) -> WorkerReply {
        let id = self.next;
        self.next += 1;
        let line = serde_json::to_string(&Tagged { id, body: request }).unwrap();
        writeln!(self.input, "{line}").unwrap();
        let reply = self.read();
        assert_eq!(reply.id, id);
        reply.body
    }

    fn end(mut self) {
        drop(self.input);
        self.child.wait().unwrap();
    }
}

/// A PDF of `count` A5 pages, each its number written large in Helvetica;
/// the pages `turned` set to show a quarter turn round, and a dark square
/// in the bottom left corner of each page when `corner`, the number left
/// out.
fn numbered(pdfium: &Pdfium, path: &Path, count: usize, turned: &[usize], corner: bool) {
    let mut document = pdfium.create_new_pdf().unwrap();
    let font = document.fonts_mut().helvetica();
    for number in 1..=count {
        let mut page = document
            .pages_mut()
            .create_page_at_end(PdfPagePaperSize::Custom(
                PdfPoints::new(420.0),
                PdfPoints::new(595.0),
            ))
            .unwrap();
        if corner {
            let square = PdfPagePathObject::new_rect(
                &document,
                PdfRect::new_from_values(20.0, 20.0, 60.0, 60.0),
                None,
                None,
                Some(PdfColor::BLACK),
            )
            .unwrap();
            page.objects_mut().add_path_object(square).unwrap();
        } else {
            let mut text =
                PdfPageTextObject::new(&document, number.to_string(), font, PdfPoints::new(160.0))
                    .unwrap();
            text.translate(PdfPoints::new(160.0), PdfPoints::new(240.0))
                .unwrap();
            page.objects_mut().add_text_object(text).unwrap();
        }
        if turned.contains(&number) {
            page.set_rotation(PdfPageRenderRotation::Degrees90);
        }
    }
    document.save_to_file(path).unwrap();
}

/// A character read back: what it is, its box (left, bottom, right, top)
/// and its angle.
#[derive(Debug, Clone, PartialEq)]
struct Read {
    text: String,
    bounds: (f32, f32, f32, f32),
    angle: f32,
}

/// Each side of `path`: what its left half and its right half write.
fn sides(pdfium: &Pdfium, path: &Path) -> Vec<(Vec<Read>, Vec<Read>)> {
    let document = pdfium.load_pdf_from_file(path, None).unwrap();
    document
        .pages()
        .iter()
        .map(|page| {
            let middle = page.width().value / 2.0;
            let text = page.text().unwrap();
            let (mut left, mut right) = (Vec::new(), Vec::new());
            for ch in text.chars().iter() {
                let Some(written) = ch.unicode_string().filter(|t| !t.trim().is_empty()) else {
                    continue;
                };
                let b = ch.tight_bounds().unwrap();
                let read = Read {
                    text: written,
                    bounds: (
                        b.left().value,
                        b.bottom().value,
                        b.right().value,
                        b.top().value,
                    ),
                    angle: ch.angle_degrees().unwrap_or_default(),
                };
                if f32::midpoint(read.bounds.0, read.bounds.2) < middle {
                    left.push(read);
                } else {
                    right.push(read);
                }
            }
            (left, right)
        })
        .collect()
}

fn written(half: &[Read]) -> String {
    half.iter().map(|read| read.text.as_str()).collect()
}

/// A booklet of the document's `count` pages in the core's order, laid
/// out on landscape A4.
fn booklet(count: u32, trim: bool, turn_backs: bool) -> WorkerRequest {
    let pages: Vec<u32> = (0..count).collect();
    WorkerRequest::Compose(Compose::Booklet {
        sheets: skwir_core::print::booklet::sheets(
            &pages,
            None,
            skwir_core::print::booklet::PerSide::Two,
        ),
        per_side: skwir_core::print::booklet::PerSide::Two,
        width: 842.0,
        height: 595.0,
        fold: 12.0,
        edge: 18.0,
        trim,
        turn_backs,
    })
}

fn pages_and_size(pdfium: &Pdfium, path: &Path) -> (usize, (f32, f32)) {
    let sizes = sizes(pdfium, path);
    (sizes.len(), sizes[0])
}

#[test]
fn a_booklet_lays_two_pages_a_side_in_the_folding_order() {
    let Some(pdfium) = pdfium() else {
        eprintln!("skipped: no PDFium (SKWIR_PDFIUM_LIBRARY)");
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let (pdf, scratch) = (tmp.path().join("eight.pdf"), tmp.path().join("work"));
    std::fs::create_dir(&scratch).unwrap();
    numbered(&pdfium, &pdf, 8, &[], false);
    let composed = scratch.join(COMPOSED_FILE);
    let mut worker = Worker::start(&pdf, &scratch);
    let reply = worker.ask(booklet(8, false, false));
    assert!(
        matches!(reply, WorkerReply::Composed { pages: 4 }),
        "{reply:?}"
    );
    assert_eq!(pages_and_size(&pdfium, &composed), (4, (842.0, 595.0)));
    let read = sides(&pdfium, &composed);
    let halves: Vec<(String, String)> = read
        .iter()
        .map(|(left, right)| (written(left), written(right)))
        .collect();
    // Two sheets, each its front then its back: 8|1, 2|7, 6|3, 4|5.
    let expected = [("8", "1"), ("2", "7"), ("6", "3"), ("4", "5")];
    assert_eq!(halves, expected.map(|(l, r)| (l.to_owned(), r.to_owned())));
    // Turned for a printer turning sheets over their long edge, each back
    // shows its halves swapped and upside down; the fronts are as they were.
    let reply = worker.ask(booklet(8, false, true));
    assert!(
        matches!(reply, WorkerReply::Composed { pages: 4 }),
        "{reply:?}"
    );
    let turned = sides(&pdfium, &composed);
    let halves: Vec<(String, String)> = turned
        .iter()
        .map(|(left, right)| (written(left), written(right)))
        .collect();
    let expected = [("8", "1"), ("7", "2"), ("6", "3"), ("5", "4")];
    assert_eq!(halves, expected.map(|(l, r)| (l.to_owned(), r.to_owned())));
    assert_eq!(turned[0], read[0], "a front is not turned");
    // The 2, on the back's left, is now on its right, its box where the
    // sheet turned about its middle puts it, its writing upside down.
    let (before, after) = (&read[1].0[0], &turned[1].1[0]);
    assert_eq!(after.text, "2");
    let (l, b, r, t) = before.bounds;
    let (tl, tb, tr, tt) = after.bounds;
    for (got, want) in [
        (tl, 842.0 - r),
        (tr, 842.0 - l),
        (tb, 595.0 - t),
        (tt, 595.0 - b),
    ] {
        assert!(
            (got - want).abs() < 1.0,
            "{:?} turned is {:?}",
            before.bounds,
            after.bounds
        );
    }
    assert!((before.angle - 0.0).abs() < 1.0, "{}", before.angle);
    assert!((after.angle - 180.0).abs() < 1.0, "{}", after.angle);
    worker.end();
}

#[test]
fn a_booklet_of_five_pages_leaves_three_halves_blank_and_honours_a_page_s_turn() {
    let Some(pdfium) = pdfium() else {
        eprintln!("skipped: no PDFium (SKWIR_PDFIUM_LIBRARY)");
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let (pdf, scratch) = (tmp.path().join("five.pdf"), tmp.path().join("work"));
    std::fs::create_dir(&scratch).unwrap();
    // The third page shows a quarter turn round, as a landscape page does.
    numbered(&pdfium, &pdf, 5, &[3], false);
    let composed = scratch.join(COMPOSED_FILE);
    let mut worker = Worker::start(&pdf, &scratch);
    let reply = worker.ask(booklet(5, false, false));
    assert!(
        matches!(reply, WorkerReply::Composed { pages: 4 }),
        "{reply:?}"
    );
    let read = sides(&pdfium, &composed);
    let halves: Vec<(String, String)> = read
        .iter()
        .map(|(left, right)| (written(left), written(right)))
        .collect();
    let expected = [("", "1"), ("2", ""), ("", "3"), ("4", "5")];
    assert_eq!(halves, expected.map(|(l, r)| (l.to_owned(), r.to_owned())));
    // The turned page's 3 is written across, as the page shows it; the
    // others stand.
    let three = &read[2].1[0];
    assert!(
        (three.angle - 90.0).abs() < 1.0 || (three.angle - 270.0).abs() < 1.0,
        "{three:?}"
    );
    let one = &read[0].1[0];
    assert!(one.angle.abs() < 1.0, "{one:?}");
    // A page past the end is refused, and the helper goes on.
    let past = WorkerRequest::Compose(Compose::Booklet {
        sheets: skwir_core::print::booklet::sheets(
            &[0, 1, 2, 9],
            None,
            skwir_core::print::booklet::PerSide::Two,
        ),
        per_side: skwir_core::print::booklet::PerSide::Two,
        width: 842.0,
        height: 595.0,
        fold: 0.0,
        edge: 0.0,
        trim: false,
        turn_backs: false,
    });
    let reply = worker.ask(past);
    assert!(
        matches!(&reply, WorkerReply::Error(message) if message.contains("past the end")),
        "{reply:?}"
    );
    assert!(matches!(
        worker.ask(booklet(5, false, true)),
        WorkerReply::Composed { pages: 4 }
    ));
    worker.end();
}

/// The width each page's drawing takes on the sides of `path`.
fn drawn_widths(pdfium: &Pdfium, path: &Path) -> Vec<f32> {
    let document = pdfium.load_pdf_from_file(path, None).unwrap();
    document
        .pages()
        .iter()
        .flat_map(|page| {
            page.objects()
                .iter()
                .filter_map(|object| object.bounds().ok())
                .map(|bounds| bounds.width().value)
                .collect::<Vec<_>>()
        })
        .collect()
}

#[test]
fn a_trimmed_booklet_fits_the_pages_ink_larger() {
    let Some(pdfium) = pdfium() else {
        eprintln!("skipped: no PDFium (SKWIR_PDFIUM_LIBRARY)");
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let (pdf, scratch) = (tmp.path().join("corner.pdf"), tmp.path().join("work"));
    std::fs::create_dir(&scratch).unwrap();
    // Each page's ink a 40-point square in its corner.
    numbered(&pdfium, &pdf, 4, &[], true);
    let composed = scratch.join(COMPOSED_FILE);
    let mut worker = Worker::start(&pdf, &scratch);
    assert!(matches!(
        worker.ask(booklet(4, false, false)),
        WorkerReply::Composed { pages: 2 }
    ));
    let whole = drawn_widths(&pdfium, &composed);
    assert!(matches!(
        worker.ask(booklet(4, true, false)),
        WorkerReply::Composed { pages: 2 }
    ));
    let trimmed = drawn_widths(&pdfium, &composed);
    assert_eq!(whole.len(), 4);
    assert_eq!(trimmed.len(), 4);
    // Whole, the page's 420 points take a half's 391: the square some 37.
    // Trimmed, the square (and the white kept round it) fills the half.
    for (whole, trimmed) in whole.iter().zip(&trimmed) {
        assert!(*whole > 30.0 && *whole < 45.0, "{whole}");
        assert!(*trimmed > 300.0, "{trimmed}");
    }
    // The source's pages are as they were: drawn again, uncut.
    assert!(matches!(
        worker.ask(WorkerRequest::Compose(Compose::Pages {
            pages: vec![Some(0)],
            scale: 1.0
        })),
        WorkerReply::Composed { pages: 1 }
    ));
    assert_eq!(sizes(&pdfium, &composed), [(420.0, 595.0)]);
    worker.end();
}

/// A page's stamps, notes and form fields scale with it: drawn into it
/// first, as they print, never left at their own size over the page drawn
/// smaller. At its own size a page keeps them as they are.
#[test]
fn a_page_s_annotations_scale_with_it() {
    let Some(pdfium) = pdfium() else {
        eprintln!("skipped: no PDFium (SKWIR_PDFIUM_LIBRARY)");
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let (pdf, scratch) = (tmp.path().join("stamped.pdf"), tmp.path().join("work"));
    std::fs::create_dir(&scratch).unwrap();
    // A stamp, a 40-point black square printed at 20 points from the
    // bottom left of 420 × 595, and nothing else on the page.
    {
        let mut document = pdfium.create_new_pdf().unwrap();
        let mut page = document
            .pages_mut()
            .create_page_at_end(PdfPagePaperSize::Custom(
                PdfPoints::new(420.0),
                PdfPoints::new(595.0),
            ))
            .unwrap();
        let square = PdfRect::new_from_values(20.0, 20.0, 60.0, 60.0);
        let mut stamp = page.annotations_mut().create_stamp_annotation().unwrap();
        stamp.set_bounds(square).unwrap();
        stamp.set_is_printed(true).unwrap();
        stamp
            .objects_mut()
            .create_path_object_rect(square, None, None, Some(PdfColor::BLACK))
            .unwrap();
        document.save_to_file(&pdf).unwrap();
    }
    let composed = scratch.join(COMPOSED_FILE);
    let mut worker = Worker::start(&pdf, &scratch);
    let at = |scale: f32| {
        WorkerRequest::Compose(Compose::Pages {
            pages: vec![Some(0)],
            scale,
        })
    };
    assert!(matches!(
        worker.ask(at(1.0)),
        WorkerReply::Composed { pages: 1 }
    ));
    {
        let document = pdfium.load_pdf_from_file(&composed, None).unwrap();
        let page = document.pages().get(0).unwrap();
        assert_eq!(page.annotations().len(), 1, "a stamp still");
    }
    assert!(matches!(
        worker.ask(at(0.5)),
        WorkerReply::Composed { pages: 1 }
    ));
    let document = pdfium.load_pdf_from_file(&composed, None).unwrap();
    let page = document.pages().get(0).unwrap();
    assert_eq!(page.annotations().len(), 0, "drawn into the page");
    let bounds: Vec<_> = page
        .objects()
        .iter()
        .filter_map(|object| object.bounds().ok())
        .collect();
    assert_eq!(bounds.len(), 1, "{bounds:?}");
    // Halfway to the middle (210, 297.5), as the page's own drawing is.
    let (left, bottom, width) = (
        bounds[0].left().value,
        bounds[0].bottom().value,
        bounds[0].width().value,
    );
    assert!((left - 115.0).abs() < 1.0, "{left}");
    assert!((bottom - 158.75).abs() < 1.0, "{bottom}");
    assert!((width - 20.0).abs() < 1.0, "{width}");
    worker.end();
}

/// The print dialog's scale: each page drawn at that of its size about
/// its middle, on a page of its own size; a scale that is none refused.
#[test]
fn pages_are_drawn_at_the_scale_asked_about_their_middle() {
    let Some(pdfium) = pdfium() else {
        eprintln!("skipped: no PDFium (SKWIR_PDFIUM_LIBRARY)");
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let (pdf, scratch) = (tmp.path().join("corner.pdf"), tmp.path().join("work"));
    std::fs::create_dir(&scratch).unwrap();
    // A 40-point square at 20 points from the bottom left of 420 × 595.
    numbered(&pdfium, &pdf, 2, &[], true);
    let composed = scratch.join(COMPOSED_FILE);
    let mut worker = Worker::start(&pdf, &scratch);
    assert!(matches!(
        worker.ask(WorkerRequest::Compose(Compose::Pages {
            pages: vec![Some(1), Some(0)],
            scale: 0.5
        })),
        WorkerReply::Composed { pages: 2 }
    ));
    assert_eq!(sizes(&pdfium, &composed), [(420.0, 595.0), (420.0, 595.0)]);
    let document = pdfium.load_pdf_from_file(&composed, None).unwrap();
    for page in document.pages().iter() {
        let bounds: Vec<_> = page
            .objects()
            .iter()
            .filter_map(|object| object.bounds().ok())
            .collect();
        assert_eq!(bounds.len(), 1);
        // Halfway to the middle (210, 297.5): from 115, 158.75, 20 wide.
        let (left, bottom, width) = (
            bounds[0].left().value,
            bounds[0].bottom().value,
            bounds[0].width().value,
        );
        assert!((left - 115.0).abs() < 0.5, "{left}");
        assert!((bottom - 158.75).abs() < 0.5, "{bottom}");
        assert!((width - 20.0).abs() < 0.5, "{width}");
    }
    for scale in [0.0, -1.0, 11.0] {
        let reply = worker.ask(WorkerRequest::Compose(Compose::Pages {
            pages: vec![Some(0)],
            scale,
        }));
        assert!(
            matches!(&reply, WorkerReply::Error(message) if message.contains("scale")),
            "{scale}: {reply:?}"
        );
    }
    worker.end();
}

/// What each place of a side writes, row by row from the top left, and
/// whether its writing is upside down: the side's characters by where
/// they are, in a grid of `columns` × `rows`.
fn places(pdfium: &Pdfium, path: &Path, columns: usize, rows: usize) -> Vec<Vec<(String, bool)>> {
    let document = pdfium.load_pdf_from_file(path, None).unwrap();
    document
        .pages()
        .iter()
        .map(|page| {
            let (width, height) = (page.width().value, page.height().value);
            let mut cells = vec![(String::new(), false); columns * rows];
            let text = page.text().unwrap();
            for ch in text.chars().iter() {
                let Some(written) = ch.unicode_string().filter(|t| !t.trim().is_empty()) else {
                    continue;
                };
                let b = ch.tight_bounds().unwrap();
                let x = f32::midpoint(b.left().value, b.right().value);
                let y = f32::midpoint(b.bottom().value, b.top().value);
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let column = ((x / width) * columns as f32) as usize;
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let row = (((height - y) / height) * rows as f32) as usize;
                let cell = &mut cells[row.min(rows - 1) * columns + column.min(columns - 1)];
                cell.0.push_str(&written);
                cell.1 = (ch.angle_degrees().unwrap_or_default() - 180.0).abs() < 1.0;
            }
            cells
        })
        .collect()
}

/// Four to a side, on upright A4: a printer's 8-page signature, the top
/// row upside down, page 1 on the front's bottom right; the folds marked
/// in the front's margins and not on the back.
#[test]
fn a_booklet_of_four_a_side_lays_a_printer_s_signature() {
    let Some(pdfium) = pdfium() else {
        eprintln!("skipped: no PDFium (SKWIR_PDFIUM_LIBRARY)");
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let (pdf, scratch) = (tmp.path().join("eight.pdf"), tmp.path().join("work"));
    std::fs::create_dir(&scratch).unwrap();
    numbered(&pdfium, &pdf, 8, &[], false);
    let composed = scratch.join(COMPOSED_FILE);
    let mut worker = Worker::start(&pdf, &scratch);
    let pages: Vec<u32> = (0..8).collect();
    let four = skwir_core::print::booklet::PerSide::Four;
    let reply = worker.ask(WorkerRequest::Compose(Compose::Booklet {
        sheets: skwir_core::print::booklet::sheets(&pages, None, four),
        per_side: four,
        width: 595.0,
        height: 842.0,
        fold: 12.0,
        edge: 18.0,
        trim: false,
        turn_backs: false,
    }));
    assert!(
        matches!(reply, WorkerReply::Composed { pages: 2 }),
        "{reply:?}"
    );
    assert_eq!(pages_and_size(&pdfium, &composed), (2, (595.0, 842.0)));
    let read = places(&pdfium, &composed, 2, 2);
    let said = |side: &[(String, bool)]| -> Vec<String> {
        side.iter()
            .map(|(text, turned)| {
                if *turned {
                    format!("{text}T")
                } else {
                    text.clone()
                }
            })
            .collect()
    };
    assert_eq!(said(&read[0]), ["5T", "4T", "8", "1"]);
    assert_eq!(said(&read[1]), ["3T", "6T", "2", "7"]);
    let document = pdfium.load_pdf_from_file(&composed, None).unwrap();
    let lines = |at: PdfPageIndex| {
        document
            .pages()
            .get(at)
            .unwrap()
            .objects()
            .iter()
            .filter(|object| object.object_type() == PdfPageObjectType::Path)
            .count()
    };
    assert_eq!(lines(0), 4, "each fold marked at both ends on the front");
    assert_eq!(lines(1), 0, "nothing marked on the back");
    worker.end();
}

/// Eight to a side, on landscape A4, of ten pages: a printer's 16-page
/// signature, the places past the last page blank; its back turned half a
/// turn for a printer turning the sheet over the other edge, each page of
/// it in the place across the sheet's middle and turned once more. The
/// folds are marked on the front alone.
#[test]
fn a_booklet_of_eight_a_side_leaves_its_last_places_blank_and_turns_its_back() {
    let Some(pdfium) = pdfium() else {
        eprintln!("skipped: no PDFium (SKWIR_PDFIUM_LIBRARY)");
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let (pdf, scratch) = (tmp.path().join("ten.pdf"), tmp.path().join("work"));
    std::fs::create_dir(&scratch).unwrap();
    numbered(&pdfium, &pdf, 10, &[], false);
    let composed = scratch.join(COMPOSED_FILE);
    let mut worker = Worker::start(&pdf, &scratch);
    let pages: Vec<u32> = (0..10).collect();
    let eight = skwir_core::print::booklet::PerSide::Eight;
    let reply = worker.ask(WorkerRequest::Compose(Compose::Booklet {
        sheets: skwir_core::print::booklet::sheets(&pages, None, eight),
        per_side: eight,
        width: 842.0,
        height: 595.0,
        fold: 12.0,
        edge: 18.0,
        trim: false,
        turn_backs: true,
    }));
    assert!(
        matches!(reply, WorkerReply::Composed { pages: 2 }),
        "{reply:?}"
    );
    assert_eq!(pages_and_size(&pdfium, &composed), (2, (842.0, 595.0)));
    let read = places(&pdfium, &composed, 4, 2);
    let said = |side: &[(String, bool)]| -> Vec<String> {
        side.iter()
            .map(|(text, turned)| {
                if *turned {
                    format!("{text}T")
                } else {
                    text.clone()
                }
            })
            .collect()
    };
    // The front as the core lays it (5T 12T 9T 8T / 4 13 16 1), 11 to 16
    // blank.
    assert_eq!(said(&read[0]), ["5T", "", "9T", "8T", "4", "", "", "1"]);
    // The back as the core lays it (7T 10T 11T 6T / 2 15 14 3), turned
    // half a turn with its sheet.
    assert_eq!(said(&read[1]), ["3T", "", "", "2T", "6", "", "10", "7"]);
    let document = pdfium.load_pdf_from_file(&composed, None).unwrap();
    let lines = |at: PdfPageIndex| {
        document
            .pages()
            .get(at)
            .unwrap()
            .objects()
            .iter()
            .filter(|object| object.object_type() == PdfPageObjectType::Path)
            .count()
    };
    assert_eq!(lines(0), 8, "three folds across and one down, at both ends");
    assert_eq!(lines(1), 0, "nothing marked on the back");
    worker.end();
}
