//! What every book read in reading mode shares, whatever its format
//! (ADR-106): its chapters read one at a time, a search across them that
//! starts at the chapter asked and wraps, a link or a contents entry
//! leading to its chapter however it is spelt, and the chapter's source.
//! A format says how a chapter is read and what it names (`Book`).
use std::{collections::HashMap, path::PathBuf, sync::Arc, time::SystemTime};

use skwir_core::{
    CancelToken,
    inspection::{Inspection, InspectionKind, PrintedPage, Row},
    preview::Held,
};
use skwir_platform::{
    inspection::{self, limits::Bounds},
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
    /// What the title calls the format: "EPUB".
    const FORMAT: &'static str;
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
        B::FORMAT
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
        title: format!("{} · chapter {} / {}", B::FORMAT, index + 1, chapters.len()),
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

/// What a format works out once from a book (its index, its text
/// decompressed), kept while the preview shows that book and that version
/// of it (`PreviewRequest::shown`, as ADR-105 keeps an archive's helper):
/// a chapter turned, a search, a picture shown read the book once.
pub(crate) struct Kept<T>(std::sync::Mutex<Option<KeptBook<T>>>);

struct KeptBook<T> {
    path: PathBuf,
    version: (u64, Option<SystemTime>),
    shown: CancelToken,
    book: Arc<T>,
}

impl<T> Default for Kept<T> {
    fn default() -> Self {
        Self(std::sync::Mutex::new(None))
    }
}

impl<T> Kept<T> {
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
        if let Some(book) = kept(&self.lock()) {
            return Ok(book);
        }
        let book = Arc::new(open()?);
        let mut slot = self.lock();
        if let Some(other) = kept(&slot) {
            return Ok(other);
        }
        // Nothing is kept for a preview already gone.
        if !shown.is_cancelled() {
            *slot = Some(KeptBook {
                path: held.path().to_owned(),
                version,
                shown: shown.clone(),
                book: Arc::clone(&book),
            });
        }
        Ok(book)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<KeptBook<T>>> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}
