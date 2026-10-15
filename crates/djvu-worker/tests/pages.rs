//! The worker read through its protocol, as skwir reads it, on a document
//! djvu-rs itself wrote: pages drawn where their words and links are, the
//! outline, search and the first page alone. `just djvu-check` runs the
//! same on documents `DjVuLibre`'s tools make, for both engines (ADR-061).

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdout, Command, Stdio};

use djvu_rs::annotation::{self, Annotation, MapArea, Shape};
use djvu_rs::djvu_document::DjVuBookmark;
use djvu_rs::djvu_mut::DjVuDocumentMut;
use djvu_rs::text::{self, TextLayer, TextZone, TextZoneKind};
use djvu_rs::{Bitmap, jb2_encode};
use skwir_core::document::{
    Destination, PIXELS_FILE, PixelOrder, Tagged, WorkerReply, WorkerRequest,
};

/// A page as stored, in pixels at 100 dpi (US letter).
const WIDTH: u32 = 850;
const HEIGHT: u32 = 1100;
/// Page 1's words, from its top left: drawn black, and said by its text.
const WORDS: [(&str, [u32; 4]); 2] = [
    ("Alpha", [100, 150, 250, 190]),
    ("Beta", [280, 150, 400, 190]),
];

fn zone(
    kind: TextZoneKind,
    text: &str,
    [l, t, r, b]: [u32; 4],
    children: Vec<TextZone>,
) -> TextZone {
    TextZone {
        kind,
        rect: text::Rect {
            x: l,
            y: t,
            width: r - l,
            height: b - t,
        },
        text: text.into(),
        children,
    }
}

/// Two pages: the first with its words and a link to the second, the
/// second with a box alone; an outline that leads to the second.
fn document(path: &Path) {
    let mut first = Bitmap::new(WIDTH, HEIGHT);
    for (_, [l, t, r, b]) in WORDS {
        for y in t..b {
            for x in l..r {
                first.set(x, y, true);
            }
        }
    }
    let mut second = Bitmap::new(WIDTH, HEIGHT);
    for y in 500..600 {
        for x in 300..500 {
            second.set(x, y, true);
        }
    }
    let bundle = jb2_encode::encode_djvm_bundle_jb2(&[first, second], 0, 100);
    let mut edited = DjVuDocumentMut::from_bytes(&bundle).unwrap();
    let words = WORDS
        .iter()
        .map(|(text, at)| zone(TextZoneKind::Word, text, *at, Vec::new()))
        .collect();
    let line = zone(
        TextZoneKind::Line,
        "Alpha Beta",
        [100, 150, 400, 190],
        words,
    );
    let layer = TextLayer {
        text: "Alpha Beta".into(),
        zones: vec![zone(
            TextZoneKind::Page,
            "Alpha Beta",
            [0, 0, WIDTH, HEIGHT],
            vec![line],
        )],
    };
    let mut page = edited.page_mut(0).unwrap();
    page.set_text_layer(&layer).unwrap();
    // A link over "Alpha", from the page's bottom left.
    page.set_annotations(
        &Annotation::default(),
        &[MapArea {
            url: "#2".into(),
            target: None,
            description: String::new(),
            shape: Shape::Rect(annotation::Rect {
                x: 100,
                y: HEIGHT - 190,
                width: 150,
                height: 40,
            }),
            border: None,
            highlight: None,
            extra: Vec::new(),
        }],
    );
    edited
        .set_bookmarks(&[DjVuBookmark {
            title: "The box".into(),
            url: "#2".into(),
            children: Vec::new(),
        }])
        .unwrap();
    std::fs::write(path, edited.into_bytes()).unwrap();
}

struct Worker {
    child: Child,
    replies: BufReader<ChildStdout>,
    sent: u64,
}

impl Worker {
    fn start(document: &Path, scratch: &Path, first_page: bool) -> (Self, WorkerReply) {
        let mut command = Command::new(env!("CARGO_BIN_EXE_skwir-djvu-worker"));
        command.arg(document).arg(scratch);
        if first_page {
            command.arg(skwir_core::document::FIRST_PAGE);
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let replies = BufReader::new(child.stdout.take().unwrap());
        let mut worker = Self {
            child,
            replies,
            sent: 0,
        };
        let greeting = worker.reply(0);
        (worker, greeting)
    }

    fn reply(&mut self, id: u64) -> WorkerReply {
        let mut line = String::new();
        self.replies.read_line(&mut line).unwrap();
        let reply: Tagged<WorkerReply> = serde_json::from_str(&line).unwrap();
        assert_eq!(reply.id, id, "{line}");
        reply.body
    }

    fn ask(&mut self, request: &WorkerRequest) -> WorkerReply {
        self.sent += 1;
        let line = serde_json::to_string(&Tagged {
            id: self.sent,
            body: request,
        })
        .unwrap();
        let input = self.child.stdin.as_mut().unwrap();
        writeln!(input, "{line}").unwrap();
        input.flush().unwrap();
        self.reply(self.sent)
    }

    fn close(mut self) {
        drop(self.child.stdin.take());
        assert!(self.child.wait().unwrap().success());
    }
}

/// Whether a pixel inside `bounds` (fractions of the page) is dark.
fn inked(pixels: &[u8], width: u32, height: u32, bounds: skwir_core::document::Rect) -> bool {
    let (w, h) = (width as f32, height as f32);
    let (x0, y0) = ((bounds.x * w) as u32, (bounds.y * h) as u32);
    let x1 = (((bounds.x + bounds.width) * w) as u32 + 1).min(width);
    let y1 = (((bounds.y + bounds.height) * h) as u32 + 1).min(height);
    (y0..y1).any(|y| {
        (x0..x1).any(|x| {
            let at = ((y * width + x) * 4) as usize;
            pixels[at..at + 3].iter().all(|channel| *channel < 128)
        })
    })
}

#[test]
fn a_document_is_drawn_read_and_searched_through_the_protocol() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("Synthetic “scan” #1.djvu");
    document(&path);
    let scratch = folder.path().join("scratch");
    std::fs::create_dir(&scratch).unwrap();
    let (mut worker, greeting) = Worker::start(&path, &scratch, false);
    let WorkerReply::Ready(info) = greeting else {
        panic!("{greeting:?}");
    };
    assert_eq!(info.pages, 2);
    let outline: Vec<_> = info
        .outlines
        .iter()
        .map(|o| (o.title.as_str(), o.depth, o.page))
        .collect();
    assert_eq!(outline, [("The box", 0, Some(2))]);
    for side in [300, 700] {
        let WorkerReply::Page {
            content,
            width,
            height,
            order,
        } = worker.ask(&WorkerRequest::Render { page: 1, side })
        else {
            panic!("no page");
        };
        // At most the side asked, the page's own proportions kept.
        assert!(
            height <= side && height * 100 >= side * 93,
            "{width}×{height}"
        );
        assert!((width as f32 / height as f32 - 850. / 1100.).abs() < 0.01);
        assert_eq!(order, PixelOrder::Rgba);
        // Its own size is what 100% shows: 8.5 × 11 in.
        assert!((content.width - 816.).abs() < 1. && (content.height - 1056.).abs() < 1.);
        let pixels = std::fs::read(scratch.join(PIXELS_FILE)).unwrap();
        assert_eq!(pixels.len(), (width * height * 4) as usize);
        let text: String = content.chars.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(text, "Alpha Beta");
        for char in content.chars.iter().filter(|c| !c.text.trim().is_empty()) {
            assert!(inked(&pixels, width, height, char.bounds), "{char:?}");
        }
        let [link] = content.links.as_slice() else {
            panic!("{:?}", content.links);
        };
        assert_eq!(link.destination, Destination::Page(2));
        assert!(inked(&pixels, width, height, link.bounds), "{link:?}");
        assert!(link.bounds.x < 0.2 && link.bounds.y < 0.2, "{link:?}");
    }
    let WorkerReply::Matches(hits) = worker.ask(&WorkerRequest::Search {
        query: "BETA".into(),
    }) else {
        panic!("no matches");
    };
    assert_eq!(hits.iter().map(|hit| hit.page).collect::<Vec<_>>(), [1]);
    // A page past the last is the last.
    let WorkerReply::Page { content, .. } = worker.ask(&WorkerRequest::Render {
        page: 99,
        side: 200,
    }) else {
        panic!("no page");
    };
    assert_eq!(content.page, 2);
    assert!(content.chars.is_empty() && content.links.is_empty());
    worker.close();

    // For a thumbnail, its first page alone: no outline read.
    let (worker, greeting) = Worker::start(&path, &scratch, true);
    let WorkerReply::Ready(info) = greeting else {
        panic!("{greeting:?}");
    };
    assert_eq!((info.pages, info.outlines.len()), (2, 0));
    worker.close();

    // What is no DjVu document says so before anything is asked.
    std::fs::write(&path, b"no DjVu document").unwrap();
    let (worker, greeting) = Worker::start(&path, &scratch, false);
    assert!(matches!(greeting, WorkerReply::Error(_)), "{greeting:?}");
    drop(worker.child.stdin);
}
