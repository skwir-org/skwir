//! What every book read in reading mode shares, whatever its format
//! (ADR-106): its chapters read one at a time, a search across them that
//! starts at the chapter asked and wraps, a link or a contents entry
//! leading to its chapter however it is spelt, and the chapter's source.
//! A format says how a chapter is read and what it names (`Book`).
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::SystemTime,
};

use skwir_core::{
    CancelToken,
    inspection::{Inspection, InspectionKind, Picture, PrintedPage, Row},
    preview::Held,
};
use skwir_platform::{
    inspection::{
        self,
        limits::{self, Bounds},
    },
    package::normalize,
    rich_preview::{self, Markup, Resources},
};

/// A chapter in reading order: what links name it by, and how it is
/// written.
#[derive(Clone, Debug)]
pub(crate) struct ChapterRef {
    pub href: String,
    pub markup: Markup,
}

/// The parsed stylesheets of the chapters read for one preview, by the
/// sheet and the folder their URLs are found at: a search parses each
/// once, not once a chapter.
pub(crate) type Sheets = HashMap<(String, String), Option<Arc<rich_preview::Parsed>>>;

/// A book as reading mode reads it.
pub(crate) trait Book {
    /// What a chapter names outside itself, resolved within the book.
    type Resources<'a>: Resources
    where
        Self: 'a;
    /// What the title calls the book's format: "EPUB".
    fn format(&self) -> &'static str;
    /// The language of a chapter's source, for the code view.
    const SOURCE: &'static str;
    /// Its chapters, in reading order.
    fn chapters(&self) -> &[ChapterRef];
    /// A chapter's text, and how it was decoded when not as UTF-8.
    fn text(
        &mut self,
        href: &str,
        cancel: &CancelToken,
    ) -> anyhow::Result<(String, Option<String>)>;
    /// A chapter's source, as the book holds it: its text, unless the
    /// format translates it for reading mode.
    fn source(
        &mut self,
        href: &str,
        cancel: &CancelToken,
    ) -> anyhow::Result<(String, Option<String>)> {
        self.text(href, cancel)
    }
    /// What the chapter whose links start at `base` names: pictures
    /// (unless only the text is wanted) and stylesheets.
    fn resources<'a>(
        &'a mut self,
        base: &'a str,
        images: bool,
        sheets: &'a mut Sheets,
        cancel: &'a CancelToken,
    ) -> Self::Resources<'a>;
    /// Its table of contents.
    fn contents(&mut self) -> Vec<Row>;
    /// The printed book's pages, each in its chapter.
    fn printed_pages(&mut self, _sections: &Sections) -> Vec<PrintedPage> {
        Vec::new()
    }
    /// What it says about itself, for Properties: the number of chapters is
    /// added after.
    fn details(&mut self) -> Vec<(&'static str, Option<String>)>;
}

/// The chapter `view` asks for, read in reading mode: the one it names
/// (`target`, `offset`), or the next to hold its search; with the book's
/// contents, each row knowing its chapter.
pub(crate) fn inspect<B: Book>(
    book: &mut B,
    view: &skwir_core::inspection::InspectRequest,
    cancel: &CancelToken,
) -> anyhow::Result<Inspection> {
    let chapters = book.chapters().to_vec();
    anyhow::ensure!(
        !chapters.is_empty(),
        "{} has no readable chapters",
        book.format()
    );
    // A chapter is found by the member it names, however a link spells
    // it (`Text/../Text/ch2.xhtml`, escaped, with a fragment).
    let sections = Sections::of(chapters.iter().map(|chapter| member_key(&chapter.href)));
    let mut index = view.offset.min(chapters.len().saturating_sub(1) as u64) as usize;
    let mut sheets = Sheets::new();
    let mut notice = vec![String::from(
        "Reading mode · the book's fonts, colours and page layout are not applied",
    )];
    if let Some(query) = view.query.as_deref().filter(|q| !q.is_empty()) {
        // A search starts at the requested chapter and wraps, so a caller
        // resuming at `position + 1` reaches the next matching chapter.
        // A chapter that cannot be read is passed over, not the search.
        let start = (view.offset % chapters.len() as u64) as usize;
        let query = query.to_lowercase();
        // A chapter is searched as it reads, with its stylesheets: what
        // the book hides is not a match.
        let found = (start..chapters.len()).chain(0..start).find(|&candidate| {
            let chapter = &chapters[candidate];
            !cancel.is_cancelled()
                && book
                    .text(&chapter.href, cancel)
                    .and_then(|(text, _)| {
                        let resources =
                            book.resources(base(&chapter.href), false, &mut sheets, cancel);
                        rich_preview::html(
                            &text,
                            chapter.markup,
                            Bounds::of(view),
                            cancel,
                            resources,
                        )
                    })
                    .is_ok_and(|page| {
                        page.blocks
                            .iter()
                            .any(|b| b.text.to_lowercase().contains(&query))
                    })
        });
        inspection::check(cancel)?;
        if let Some(found) = found {
            index = found;
        }
        notice = vec![if found.is_some() {
            "Search match in this chapter".into()
        } else {
            "No matching chapter".into()
        }];
    }
    let href = view
        .target
        .clone()
        .filter(|_| view.query.is_none())
        .unwrap_or_else(|| chapters[index].href.clone());
    let target = member_key(&href);
    let index = sections.of_key(target.as_deref()).unwrap_or(index);
    let mut rows = book.contents();
    if rows.is_empty() {
        rows = chapters
            .iter()
            .enumerate()
            .map(|(i, chapter)| Row {
                cells: vec![format!("Chapter {}", i + 1)],
                target: Some(chapter.href.clone()),
                ..Row::default()
            })
            .collect();
    }
    // Each row knows the chapter it opens, so the contents show where
    // the reader is: a row naming no chapter of the spine knows none.
    for row in &mut rows {
        let key = row.target.as_deref().and_then(member_key);
        row.section = sections
            .of_key(key.as_deref())
            .map(|section| section as u64);
    }
    let mut details = book.details();
    details.push(("Chapters", inspection::count(chapters.len())));
    let frame = Inspection {
        details: inspection::details(details),
        title: format!(
            "{} · chapter {} / {}",
            book.format(),
            index + 1,
            chapters.len()
        ),
        previous: (index > 0).then(|| index as u64 - 1),
        next: (index + 1 < chapters.len()).then_some(index as u64 + 1),
        position: index as u64,
        sections: chapters.len() as u64,
        printed_pages: book.printed_pages(&sections),
        source_available: true,
        ..Inspection::default()
    };
    // A chapter's source is the shared code view, a line per block.
    if view.source {
        let (source, decoded) = book.source(&href, cancel)?;
        let code = inspection::code_lines(&source, Some(B::SOURCE.into()));
        return Ok(Inspection {
            kind: code.kind,
            blocks: code.blocks,
            notice: decoded.unwrap_or_default(),
            ..frame
        });
    }
    let (text, decoded) = book.text(&href, cancel)?;
    notice.extend(decoded);
    let base = base(&href);
    let bounds = Bounds::of(view);
    let resources = book.resources(base, true, &mut sheets, cancel);
    let page = rich_preview::html(&text, chapters[index].markup, bounds, cancel, resources)?;
    if page.truncated {
        notice.push(inspection::blocks_notice(bounds));
    }
    let mut blocks = page.blocks;
    for block in &mut blocks {
        for run in &mut block.runs {
            if let Some(link) = &mut run.link
                && !link.contains(':')
                && !link.starts_with('#')
            {
                *link = resolve(base, link);
            }
        }
    }
    Ok(Inspection {
        kind: InspectionKind::Book,
        blocks,
        rows,
        notice: notice.join(" · "),
        whole_available: page.truncated && bounds.cut(),
        ..frame
    })
}

/// A chapter's folder in the book, which its links start from.
pub(crate) fn base(href: &str) -> &str {
    href.rsplit_once('/').map_or("", |(base, _)| base)
}

/// `href` as a chapter whose links start at `base` names it.
pub(crate) fn resolve(base: &str, href: &str) -> String {
    if href.starts_with('/') {
        href.into()
    } else {
        format!("{base}/{href}")
    }
}

/// The member an href names: without its fragment or query, unescaped and
/// normalized. Chapters, links and resources are all compared by it.
pub(crate) fn member_key(href: &str) -> Option<String> {
    let href = href.split(['#', '?']).next().unwrap_or(href);
    let decoded = percent_encoding::percent_decode_str(href)
        .decode_utf8()
        .ok()?;
    normalize(&decoded)
}

/// The spine's members by their key (`member_key`), each at its first
/// place in the spine, as reading the spine for it would find: a link's
/// chapter found at once, where each of a thousand contents' entries read
/// a spine of a thousand chapters.
pub(crate) struct Sections(HashMap<String, usize>);

impl Sections {
    /// The spine's members' `keys`, in its order.
    pub(crate) fn of(keys: impl IntoIterator<Item = Option<String>>) -> Self {
        let mut first = HashMap::new();
        for (index, key) in keys.into_iter().enumerate() {
            if let Some(key) = key {
                first.entry(key).or_insert(index);
            }
        }
        Sections(first)
    }

    /// The first place in the spine of the member `key` names.
    pub(crate) fn of_key(&self, key: Option<&str>) -> Option<usize> {
        self.0.get(key?).copied()
    }
}

/// A part of a book's XML, parsed once it is known not to nest deeper
/// than the parser's recursion can go (`inspection::xml_depth`).
pub(crate) fn parse_xml(text: &str) -> anyhow::Result<roxmltree::Document<'_>> {
    inspection::xml_depth(text.as_bytes())?;
    Ok(roxmltree::Document::parse(text)?)
}

/// The size each picture of a book is shown at, read once: a chapter
/// naming one picture a thousand times reads it once, not a thousand times,
/// and all of it only when its first bytes do not say.
#[derive(Default)]
pub(crate) struct Sizes(Mutex<HashMap<String, Option<(u32, u32)>>>);

impl Sizes {
    /// The picture `key` names, for the view to decode when it shows it,
    /// with the size it is shown at: read from its first bytes
    /// (`read(key, limits::PICTURE_HEADER)`), else, when there is more of
    /// it, from all of it (`limits::BINARY_INPUT`). Each picture is read
    /// once however often the book shows it; one whose read was cancelled
    /// is read again.
    pub(crate) fn picture(
        &self,
        key: String,
        cancel: &CancelToken,
        mut read: impl FnMut(&str, usize) -> Option<Vec<u8>>,
    ) -> Option<Picture> {
        let known = lock(&self.0).get(&key).copied();
        let size = known.unwrap_or_else(|| {
            let size = read(&key, limits::PICTURE_HEADER).and_then(|head| {
                inspection::picture_size(&head).or_else(|| {
                    // Shorter, its first bytes were all of it.
                    let whole = (head.len() >= limits::PICTURE_HEADER)
                        .then(|| read(&key, limits::BINARY_INPUT))??;
                    inspection::picture_size(&whole)
                })
            });
            if !cancel.is_cancelled() {
                lock(&self.0).insert(key.clone(), size);
            }
            size
        });
        let (width, height) = size?;
        Some(Picture::Later { key, width, height })
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What a format works out once from a book (its index, its text
/// decompressed), kept while the preview shows that book and that version
/// of it (`PreviewRequest::shown`, as ADR-105 keeps an archive's helper):
/// a chapter turned, a search, a picture shown read the book once. Once
/// the preview ends, a thread that waited for it lets go of the book.
pub(crate) struct Kept<T>(Arc<Mutex<Option<KeptBook<T>>>>);

struct KeptBook<T> {
    path: PathBuf,
    version: (u64, Option<SystemTime>),
    shown: CancelToken,
    book: Arc<T>,
}

impl<T> Default for Kept<T> {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(None)))
    }
}

impl<T: Send + Sync + 'static> Kept<T> {
    /// The book `held` as `open` reads it: the one kept for it, or read now
    /// (outside the lock: a remote's file may take long) and kept in its
    /// place while `shown` lasts.
    pub(crate) fn get(
        &self,
        held: &Held,
        shown: &CancelToken,
        open: impl FnOnce() -> anyhow::Result<T>,
    ) -> anyhow::Result<Arc<T>> {
        let version = held.version();
        let kept = |slot: &Option<KeptBook<T>>| {
            slot.as_ref()
                .filter(|kept| {
                    kept.path == held.path()
                        && kept.version == version
                        && !kept.shown.is_cancelled()
                })
                .map(|kept| Arc::clone(&kept.book))
        };
        if let Some(book) = kept(&lock(&self.0)) {
            return Ok(book);
        }
        let book = Arc::new(open()?);
        let mut slot = lock(&self.0);
        if let Some(other) = kept(&slot) {
            return Ok(other);
        }
        // Nothing is kept for a preview already gone, nor without the
        // thread that lets go of it.
        if !shown.is_cancelled() && self.let_go_with(shown, &book) {
            let before = slot.replace(KeptBook {
                path: held.path().to_owned(),
                version,
                shown: shown.clone(),
                book: Arc::clone(&book),
            });
            drop(slot);
            drop(before);
        }
        Ok(book)
    }

    /// Let go of `book` once `shown` ends, if it is still the one kept: on
    /// a thread that lives as long as the preview, as `held_before`'s lives
    /// while a file is kept. What it frees was built on a worker, which is
    /// asked to give it back.
    fn let_go_with(&self, shown: &CancelToken, book: &Arc<T>) -> bool {
        let ended = shown.subscribe();
        let slot = Arc::downgrade(&self.0);
        let book = Arc::downgrade(book);
        std::thread::Builder::new()
            .name("skwir-book-kept".into())
            .spawn(move || {
                let _ = ended.receiver().recv();
                let Some(slot) = slot.upgrade() else {
                    return;
                };
                let mut kept = lock(&slot);
                let gone = kept
                    .as_ref()
                    .is_some_and(|kept| std::ptr::eq(Arc::as_ptr(&kept.book), book.as_ptr()))
                    .then(|| kept.take())
                    .flatten();
                drop(kept);
                if gone.is_some() {
                    drop(gone);
                    skwir_core::scheduler::release_on_workers();
                }
            })
            .is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// A 2 × 3 picture.
    const PNG: &[u8] = b"\x89\x50\x4e\x47\x0d\x0a\x1a\x0a\x00\x00\x00\x0d\x49\x48\x44\x52\x00\x00\x00\x02\x00\x00\x00\x03\x08\x06\x00\x00\x00\xb9\xea\xde\x81\x00\x00\x00\x11\x49\x44\x41\x54\x78\x9c\x63\xf8\xcf\xc0\xf0\x1f\x84\x19\x30\x18\x00\xa1\x79\x0b\xf5\x4d\xc4\x9a\x07\x00\x00\x00\x00\x49\x45\x4e\x44\xae\x42\x60\x82";

    /// A book kept while its preview shows it is let go of once the preview
    /// ends, though nothing asks for a book again; nothing is kept for a
    /// preview already gone.
    #[test]
    fn a_kept_book_goes_with_its_preview() {
        let kept = Kept::<String>::default();
        let held = Held::of_bytes("/nowhere/Synthetic book.fb2", b"words".to_vec());
        let shown = CancelToken::new();
        let book = kept.get(&held, &shown, || Ok("read".to_owned())).unwrap();
        let again = kept
            .get(&held, &shown, || anyhow::bail!("read again"))
            .unwrap();
        assert!(Arc::ptr_eq(&book, &again));
        let gone = Arc::downgrade(&book);
        drop((book, again));
        assert!(gone.upgrade().is_some(), "kept while it is shown");
        shown.cancel();
        let deadline = Instant::now() + Duration::from_secs(10);
        while gone.upgrade().is_some() {
            assert!(Instant::now() < deadline, "kept after its preview ended");
            std::thread::sleep(Duration::from_millis(10));
        }
        let late = Arc::downgrade(&kept.get(&held, &shown, || Ok("late".to_owned())).unwrap());
        assert!(late.upgrade().is_none(), "kept for a preview gone");
    }

    /// A picture a book names again and again is read once; all of it only
    /// when its first bytes do not say its size and there is more of it.
    #[test]
    fn a_picture_is_sized_once() {
        let sizes = Sizes::default();
        let cancel = CancelToken::new();
        let mut reads = Vec::new();
        for _ in 0..1000 {
            let picture = sizes.picture("cover".into(), &cancel, |key, limit| {
                reads.push((key.to_owned(), limit));
                Some(PNG.to_vec())
            });
            assert!(matches!(
                picture,
                Some(Picture::Later {
                    width: 2,
                    height: 3,
                    ..
                })
            ));
        }
        assert_eq!(reads, [("cover".to_owned(), limits::PICTURE_HEADER)]);
        // Unreadable: short, its first bytes were all of it; long, all of it
        // is read once.
        let mut reads = Vec::new();
        for _ in 0..1000 {
            for (key, length) in [("short", 16), ("long", limits::PICTURE_HEADER)] {
                let picture = sizes.picture(key.into(), &cancel, |_, limit| {
                    reads.push((key, limit));
                    Some(vec![0; length.min(limit)])
                });
                assert!(picture.is_none());
            }
        }
        assert_eq!(
            reads,
            [
                ("short", limits::PICTURE_HEADER),
                ("long", limits::PICTURE_HEADER),
                ("long", limits::BINARY_INPUT)
            ]
        );
        // A read cancelled is not one the book lacks.
        let cancelled = CancelToken::new();
        cancelled.cancel();
        assert!(
            sizes
                .picture("later".into(), &cancelled, |_, _| None)
                .is_none()
        );
        assert!(
            sizes
                .picture("later".into(), &cancel, |_, _| Some(PNG.to_vec()))
                .is_some()
        );
    }
}
