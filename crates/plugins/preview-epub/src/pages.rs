//! A fixed-layout book in the document reader (ADR-013): a book of pages,
//! like a PDF, read continuously or a page at a time. Each page is drawn by
//! `rich_preview::page` at the side the reader asks, on the reader's worker,
//! from the container read again for it; nothing is kept but the spine.
//! A drawn page has no text layer: converted comics set their lettering in
//! private-use glyphs, which read as nothing.
use std::{path::PathBuf, sync::Arc};

use bibli_core::{
    CancelToken,
    document::{
        DocumentInfo, DocumentPage, DocumentSession, MAX_HITS, Outline, PageContent, SearchHit,
    },
    inspection::Picture,
};
use bibli_platform::{
    inspection,
    rich_preview::{self, Drawing, Markup},
};

use crate::{Chapter, Container, base, contents, member_key, package};

/// Pages read to find the size a page stating none is drawn at.
const SIZE_PAGES: usize = 8;
/// The size such a page is drawn at when no page states one: a page of a
/// book, in CSS pixels.
const PAGE_SIZE: (f32, f32) = (600., 800.);

pub(crate) struct FixedBook {
    path: PathBuf,
    pages: Vec<Page>,
    /// The size a page that states none is drawn at: the book's own.
    size: (f32, f32),
    info: DocumentInfo,
}

struct Page {
    href: String,
    /// `None`: the spine names a picture itself.
    markup: Option<Markup>,
}

impl FixedBook {
    /// The book at `path` as a document of pages, when it is laid out in
    /// pages (`rendition:layout pre-paginated`); `None` for a book that
    /// reflows, which reading mode shows. Called on a scheduler worker.
    pub(crate) fn open(path: PathBuf, cancel: &CancelToken) -> anyhow::Result<Option<Self>> {
        let mut container = Container::open(&path, cancel)?;
        let (book, fixed) = package(&mut container, cancel)?;
        if !fixed {
            return Ok(None);
        }
        let pages: Vec<Page> = book
            .spine()
            .iter()
            .filter_map(|s| s.manifest_entry())
            .map(|item| {
                let kind = item.media_type();
                Page {
                    href: item.href().as_str().to_owned(),
                    markup: if kind.starts_with("image/") {
                        None
                    } else if kind == "text/html" {
                        Some(Markup::Html)
                    } else {
                        Some(Markup::Xhtml)
                    },
                }
            })
            .collect();
        anyhow::ensure!(!pages.is_empty(), "EPUB has no pages");
        anyhow::ensure!(
            u32::try_from(pages.len()).is_ok(),
            "EPUB has too many pages"
        );
        // The contents lead to the page each entry names, however its link
        // spells it.
        let keys: Vec<_> = pages.iter().map(|page| member_key(&page.href)).collect();
        let mut rows = Vec::new();
        for entry in book.toc() {
            contents(entry, 0, &mut rows);
        }
        let outlines = rows
            .into_iter()
            .map(|row| {
                let target = row.target.as_deref().map(member_key);
                Outline {
                    title: row.cells.into_iter().next().unwrap_or_default(),
                    depth: u32::try_from(row.depth).unwrap_or(0),
                    page: target
                        .and_then(|key| keys.iter().position(|k| k.is_some() && *k == key))
                        .map(|index| index as u32 + 1),
                }
            })
            .collect();
        let size = pages
            .iter()
            .take(SIZE_PAGES)
            .find_map(|page| {
                let markup = page.markup?;
                let bytes = container.read(&page.href, "Page", cancel).ok()?;
                rich_preview::stated_size(&inspection::decode(&bytes).0, markup)
            })
            .unwrap_or(PAGE_SIZE);
        inspection::check(cancel)?;
        Ok(Some(Self {
            info: DocumentInfo {
                pages: pages.len() as u32,
                outlines,
                reflowable: false,
            },
            path,
            pages,
            size,
        }))
    }

    /// Page `index` drawn with `side` pixels on its longer side: as its
    /// book lays it out; one stating no size of its own that is a single
    /// picture (a comic's page) is that picture, any other is drawn at the
    /// book's page size.
    fn draw(&self, index: usize, side: u32, cancel: &CancelToken) -> anyhow::Result<Drawing> {
        let page = &self.pages[index];
        let mut container = Container::open(&self.path, cancel)?;
        let bytes = container.read(&page.href, "Page", cancel)?;
        let Some(markup) = page.markup else {
            return picture(&bytes, side);
        };
        let source = inspection::decode(&bytes).0;
        let base = base(&page.href);
        if let Some(drawing) = rich_preview::page(
            &source,
            markup,
            cancel,
            chapter(&mut container, base, cancel),
            side,
            None,
        )? {
            return Ok(drawing);
        }
        let read = rich_preview::html(
            &source,
            markup,
            cancel,
            chapter(&mut container, base, cancel),
        )?;
        if let [only] = read.blocks.as_slice()
            && let Some(Picture::Later { key, .. }) = &only.picture
        {
            let bytes = container.read(key, "Embedded image", cancel)?;
            return picture(&bytes, side);
        }
        rich_preview::page(
            &source,
            markup,
            cancel,
            chapter(&mut container, base, cancel),
            side,
            Some(self.size),
        )?
        .ok_or_else(|| anyhow::anyhow!("the page could not be drawn"))
    }
}

fn chapter<'a>(
    container: &'a mut Container,
    base: &'a str,
    cancel: &'a CancelToken,
) -> Chapter<'a> {
    Chapter {
        container,
        base,
        cancel,
        images: true,
    }
}

/// A picture as a page: decoded at the side asked, its own size what 100%
/// is.
fn picture(bytes: &[u8], side: u32) -> anyhow::Result<Drawing> {
    let decoded = inspection::picture(bytes, side)?;
    Ok(Drawing {
        width: decoded.width,
        height: decoded.height,
        rgba: decoded.rgba,
        own: (decoded.own.0 as f32, decoded.own.1 as f32),
    })
}

impl DocumentSession for FixedBook {
    fn info(&self) -> &DocumentInfo {
        &self.info
    }

    fn render(&self, page: u32, side: u32, cancel: &CancelToken) -> anyhow::Result<DocumentPage> {
        let number = page.clamp(1, self.info.pages);
        let drawing = self.draw(number as usize - 1, side, cancel)?;
        Ok(DocumentPage {
            width: drawing.width,
            height: drawing.height,
            rgba: drawing.rgba,
            content: Arc::new(PageContent {
                page: number,
                width: drawing.own.0,
                height: drawing.own.1,
                chars: Vec::new(),
                links: Vec::new(),
            }),
        })
    }

    /// The pages whose text holds `query`, as reading mode reads it: what
    /// the book hides is not a match. A drawn page has no text layer, so a
    /// match is a page, not a place on it.
    fn search(&self, query: &str, cancel: &CancelToken) -> anyhow::Result<Vec<SearchHit>> {
        anyhow::ensure!(query.len() <= 4096, "search query exceeds limit");
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let mut container = Container::open(&self.path, cancel)?;
        let mut hits = Vec::new();
        for (index, page) in self.pages.iter().enumerate() {
            inspection::check(cancel)?;
            let Some(markup) = page.markup else {
                continue;
            };
            // A page that cannot be read is passed over, not the search.
            let Ok(bytes) = container.read(&page.href, "Page", cancel) else {
                continue;
            };
            let resources = Chapter {
                container: &mut container,
                base: base(&page.href),
                cancel,
                images: false,
            };
            let found =
                rich_preview::html(&inspection::decode(&bytes).0, markup, cancel, resources)
                    .is_ok_and(|read| {
                        read.blocks
                            .iter()
                            .any(|block| block.text.to_lowercase().contains(&query))
                    });
            if found {
                hits.push(SearchHit {
                    page: index as u32 + 1,
                    bounds: Vec::new(),
                });
                if hits.len() >= MAX_HITS {
                    break;
                }
            }
        }
        Ok(hits)
    }
}
