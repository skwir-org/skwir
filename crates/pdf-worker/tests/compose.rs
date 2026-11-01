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
        sheets: skwir_core::print::booklet::sheets(&pages, None),
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
        sheets: skwir_core::print::booklet::sheets(&[0, 1, 2, 9], None),
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
