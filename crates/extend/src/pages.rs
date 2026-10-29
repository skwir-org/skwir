//! A plugin's pages as a document skwir reads (ADR-047, protocol version
//! 3): the plugin draws the page asked and says its text and its links,
//! where they lie, and the file's outline; skwir's reader turns, zooms,
//! selects, copies, searches and follows it as any document. What the
//! plugin answered is kept in its slot of the cache: how many pages and
//! the outline, a page's picture by the side drawn, its text and links
//! apart, so a search reads the text of pages never drawn, and nothing
//! is asked twice.

use std::sync::Arc;

use serde_json::json;
use skwir_core::cache::CacheSlot;
use skwir_core::document::{
    Destination, DocumentInfo, DocumentPage, DocumentSession, Link, MAX_PAGE_SIZE, MAX_PAGES,
    Outline, PageContent, PixelOrder, Rect, SearchHit, TextChar, words,
};
use skwir_core::permissions::Capability;
use skwir_core::preview::Held;
use skwir_core::{CancelToken, FileId};
use skwir_plugin_api::protocol::{OutlineEntry, PageAnswer, PageLink, PageParams, TextRun, method};

use crate::call::Call;
use crate::external::Runner;

/// What the pages a plugin drew may take of the cache.
const BUDGET: u64 = 256 * 1024 * 1024;
/// The most matches a search answers.
const HITS: usize = 10_000;
/// The most links a page keeps, and headings an outline.
const LINKS: usize = 10_000;
/// The longest heading kept, in characters.
const TITLE: usize = 200;
/// The deepest heading kept.
const DEPTH: u32 = 16;

/// What a page says beside its picture, kept apart from it: its text and
/// its links.
#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Words {
    text: Vec<TextRun>,
    links: Vec<PageLink>,
}

/// What the file says once: how many pages, its outline.
#[derive(serde::Serialize, serde::Deserialize)]
struct Contents {
    pages: u32,
    outline: Vec<OutlineEntry>,
}

/// A file a plugin shows a page at a time, open in the panel.
pub(crate) struct Pages {
    runner: Arc<Runner>,
    kept: CacheSlot,
    /// What the preview holds of the file (ADR-101), which every page's
    /// call reads.
    held: Arc<Held>,
    id: FileId,
    mime: Option<String>,
    info: DocumentInfo,
    /// Why its links are not followed: the plugin may not use the network.
    closed_links: Option<String>,
}

/// A page's picture as kept: its own size, then its PNG.
fn picture_kept(bytes: &[u8]) -> Option<((f32, f32), &[u8])> {
    let (width, rest) = bytes.split_first_chunk::<4>()?;
    let (height, png) = rest.split_first_chunk::<4>()?;
    Some((
        (f32::from_le_bytes(*width), f32::from_le_bytes(*height)),
        png,
    ))
}

impl Pages {
    /// The file `held` (entry `id`) as pages: asked how many there are
    /// and its outline. `None` when the plugin shows nothing of it.
    pub fn open(
        runner: Arc<Runner>,
        kept: CacheSlot,
        held: Arc<Held>,
        id: FileId,
        mime: Option<String>,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<Pages>> {
        let closed_links = (!runner.granted.has(Capability::Network)).then(|| {
            format!(
                "Links are not followed in pages “{}” drew: it may not use the network",
                runner.title
            )
        });
        let mut pages = Pages {
            runner,
            closed_links,
            kept,
            held,
            id,
            mime,
            info: DocumentInfo {
                pages: 0,
                outlines: Vec::new(),
                reflowable: false,
                labels: Vec::new(),
            },
        };
        let key = pages.key(&json!(["contents"]))?;
        let kept: Option<Contents> = pages
            .kept
            .get(&key)
            .and_then(|bytes| serde_json::from_slice(&bytes).ok());
        let contents = if let Some(contents) = kept {
            contents
        } else {
            let Some(answer) = pages.ask(1, 0, false, cancel)? else {
                return Ok(None);
            };
            let contents = Contents {
                pages: answer.pages,
                outline: answer.outline,
            };
            pages.keep(&key, &serde_json::to_vec(&contents)?);
            contents
        };
        if contents.pages == 0 {
            return Ok(None);
        }
        // Whatever the plugin says, the reader reads at most `MAX_PAGES`.
        let count = contents.pages.min(MAX_PAGES);
        pages.info.pages = count;
        pages.info.outlines = outline(contents.outline, count);
        Ok(Some(pages))
    }

    /// Where what the plugin made of this version of the file is kept:
    /// the file's version, the plugin's, and `what`.
    fn key(&self, what: &serde_json::Value) -> anyhow::Result<Vec<u8>> {
        skwir_platform::preview::made_key(self.id, &self.held, self.runner.made_by(), what)
    }

    fn keep(&self, key: &[u8], value: &[u8]) {
        // A page not kept is asked again: never an error.
        if self.kept.put(key, value).is_ok() {
            self.kept.trim_soon(value.len() as u64, BUDGET);
        }
    }

    /// Ask the plugin `page` within `side` (0: no picture), with its text
    /// or not.
    fn ask(
        &self,
        page: u32,
        side: u32,
        text: bool,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<PageAnswer>> {
        let call = Call::holding(Arc::clone(&self.held), self.runner.granted, cancel.clone());
        let params = PageParams {
            file: call.describe(0, self.mime.clone()),
            page,
            side,
            text,
        };
        let answer = self
            .runner
            .call(method::PAGE, serde_json::to_value(params)?, &call)?;
        Ok(serde_json::from_value(answer)?)
    }

    /// The text and links of `page`, kept or asked (with no picture).
    fn words(&self, page: u32, cancel: &CancelToken) -> anyhow::Result<Words> {
        let key = self.key(&json!(["words", page]))?;
        if let Some(words) = self
            .kept
            .get(&key)
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        {
            return Ok(words);
        }
        let words = self
            .ask(page, 0, true, cancel)?
            .map(|answer| Words {
                text: answer.text,
                links: answer.links,
            })
            .unwrap_or_default();
        self.keep(&key, &serde_json::to_vec(&words)?);
        Ok(words)
    }

    /// A page's links as the reader's: on the page, to a page it has or
    /// an address.
    fn links(&self, links: Vec<PageLink>) -> Vec<Link> {
        links
            .into_iter()
            .take(LINKS)
            .filter_map(|link| {
                let destination = match (link.page, link.uri) {
                    (Some(page), _) if (1..=self.info.pages).contains(&page) => {
                        Destination::Page(page)
                    }
                    (None, Some(uri)) if !uri.trim().is_empty() => Destination::Uri(uri),
                    _ => return None,
                };
                let (x, y) = (unit(link.x), unit(link.y));
                Some(Link {
                    bounds: Rect {
                        x,
                        y,
                        width: unit(link.width).min(1. - x),
                        height: unit(link.height).min(1. - y),
                    },
                    destination,
                })
            })
            .collect()
    }
}

/// A plugin's outline as the reader's: headings bounded, a page it does
/// not have dropped.
fn outline(entries: Vec<OutlineEntry>, pages: u32) -> Vec<Outline> {
    entries
        .into_iter()
        .take(LINKS)
        .filter_map(|entry| {
            let title: String = entry.title.trim().chars().take(TITLE).collect();
            (!title.is_empty()).then(|| Outline {
                title,
                depth: entry.depth.min(DEPTH),
                page: entry.page.filter(|page| (1..=pages).contains(page)),
            })
        })
        .collect()
}

/// A fraction of the page, kept on it.
fn unit(v: f32) -> f32 {
    if v.is_finite() { v.clamp(0., 1.) } else { 0. }
}

impl DocumentSession for Pages {
    fn info(&self) -> &DocumentInfo {
        &self.info
    }

    fn render(&self, page: u32, side: u32, cancel: &CancelToken) -> anyhow::Result<DocumentPage> {
        anyhow::ensure!(
            (1..=self.info.pages).contains(&page),
            "There is no page {page}: it has {}",
            self.info.pages
        );
        let side = side.clamp(64, 8192);
        let picture_key = self.key(&json!(["picture", page, side]))?;
        let words_key = self.key(&json!(["words", page]))?;
        let mut words: Option<Words> = self
            .kept
            .get(&words_key)
            .and_then(|bytes| serde_json::from_slice(&bytes).ok());
        let kept = self.kept.get(&picture_key);
        let (own, png) = if let Some((own, png)) = kept.as_deref().and_then(picture_kept) {
            (Some(own), png.to_vec())
        } else {
            let answer = self
                .ask(page, side, words.is_none(), cancel)?
                .ok_or_else(|| {
                    anyhow::anyhow!("“{}” shows nothing of this file", self.runner.title)
                })?;
            let png = answer.png.ok_or_else(|| {
                anyhow::anyhow!("There is no page {page}: it has {}", answer.pages)
            })?;
            let png = skwir_plugin_api::base64::decode(&png)
                .ok_or_else(|| anyhow::anyhow!("the plugin's page is no base64"))?;
            let own = answer.width.zip(answer.height);
            if words.is_none() {
                let said = Words {
                    text: answer.text,
                    links: answer.links,
                };
                self.keep(&words_key, &serde_json::to_vec(&said)?);
                words = Some(said);
            }
            let (width, height) = own.unwrap_or((f32::NAN, f32::NAN));
            let mut value = width.to_le_bytes().to_vec();
            value.extend_from_slice(&height.to_le_bytes());
            value.extend_from_slice(&png);
            self.keep(&picture_key, &value);
            (own, png)
        };
        let decoded = skwir_platform::inspection::picture(&png, side)?;
        let words = words.unwrap_or_default();
        // Its own size, else its picture's.
        #[allow(clippy::cast_precision_loss)] // A picture's sides.
        let (width, height) = own
            .filter(|(w, h)| w.is_finite() && h.is_finite() && *w > 0. && *h > 0.)
            .unwrap_or((decoded.width as f32, decoded.height as f32));
        Ok(DocumentPage {
            width: decoded.width,
            height: decoded.height,
            pixels: decoded.rgba,
            order: PixelOrder::Rgba,
            content: Arc::new(PageContent {
                page,
                width: width.min(MAX_PAGE_SIZE),
                height: height.min(MAX_PAGE_SIZE),
                chars: chars(&words.text),
                links: self.links(words.links),
            }),
        })
    }

    /// Every page's text read (kept, or asked without its picture) and
    /// matched whatever the case, a line break matching a space.
    fn search(&self, query: &str, cancel: &CancelToken) -> anyhow::Result<Vec<SearchHit>> {
        if query.chars().all(char::is_whitespace) {
            return Ok(Vec::new());
        }
        let mut hits = Vec::new();
        for page in 1..=self.info.pages {
            anyhow::ensure!(!cancel.is_cancelled(), "search cancelled");
            let chars = chars(&self.words(page, cancel)?.text);
            for bounds in words::matches(&chars, query) {
                hits.push(SearchHit { page, bounds });
                if hits.len() == HITS {
                    return Ok(hits);
                }
            }
        }
        Ok(hits)
    }

    fn closed_links(&self) -> Option<&str> {
        self.closed_links.as_deref()
    }
}

/// A page's runs as the reader's characters (`words::chars`).
fn chars(runs: &[TextRun]) -> Vec<TextChar> {
    let runs: Vec<words::Run> = runs
        .iter()
        .map(|run| words::Run {
            text: run.text.clone(),
            bounds: Rect {
                x: run.x,
                y: run.y,
                width: run.width,
                height: run.height,
            },
            edges: run.edges.clone(),
        })
        .collect();
    words::chars(&runs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str, x: f32, y: f32, width: f32) -> TextRun {
        TextRun {
            text: text.into(),
            x,
            y,
            width,
            height: 0.1,
            edges: Vec::new(),
        }
    }

    /// A plugin of three pages: page `n` reads `page n` across its top,
    /// its picture one pixel; it counts what it is asked.
    struct Three(parking_lot::Mutex<Vec<(u32, u32, bool)>>);

    /// A PNG of one transparent pixel.
    const PIXEL: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x06\0\0\0\x1f\x15\xc4\x89\0\0\0\nIDATx\x9cc\0\x01\0\0\x05\0\x01\r\n-\xb4\0\0\0\0IEND\xaeB`\x82";

    impl crate::call::Transport for Three {
        fn call(
            &self,
            method: &str,
            params: serde_json::Value,
            _call: &Call,
        ) -> anyhow::Result<serde_json::Value> {
            assert_eq!(method, method::PAGE);
            let params: PageParams = serde_json::from_value(params)?;
            self.0.lock().push((params.page, params.side, params.text));
            let link = |page: Option<u32>, uri: Option<&str>| PageLink {
                x: 0.1,
                y: 0.9,
                width: 0.2,
                height: 0.05,
                page,
                uri: uri.map(str::to_owned),
            };
            let answer = PageAnswer {
                pages: 3,
                png: (params.side > 0 && params.page <= 3)
                    .then(|| skwir_plugin_api::base64::encode(PIXEL)),
                width: Some(612.),
                height: Some(792.),
                text: if params.text {
                    vec![run(&format!("page {}", params.page), 0.1, 0.05, 0.8)]
                } else {
                    Vec::new()
                },
                links: if params.text {
                    vec![
                        link(Some(params.page % 3 + 1), None),
                        link(None, Some("https://example.com/")),
                        link(Some(9), None),
                    ]
                } else {
                    Vec::new()
                },
                outline: vec![
                    OutlineEntry {
                        title: " Start ".into(),
                        depth: 0,
                        page: Some(1),
                    },
                    OutlineEntry {
                        title: "Beyond".into(),
                        depth: 99,
                        page: Some(7),
                    },
                ],
            };
            Ok(serde_json::to_value(answer)?)
        }
        fn stop(&self) {}
    }

    #[test]
    fn a_plugins_pages_are_read_selected_and_searched_and_asked_once() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("three pages.doc");
        std::fs::write(&file, "three").unwrap();
        let module = tmp.path().join("plugin.wasm");
        std::fs::write(&module, "module").unwrap();
        let three = Arc::new(Three(parking_lot::Mutex::default()));
        let runner = Arc::new(Runner::answered_by(&module, three.clone()));
        let cache = skwir_core::cache::Cache::new(tmp.path().join("cache"));
        let id = skwir_core::op::EntryRef::from_path(&file).unwrap().id;
        let open = || {
            Pages::open(
                Arc::clone(&runner),
                cache.namespace("three").within("pages"),
                Arc::new(Held::open(&file, skwir_core::preview::WHOLE).unwrap()),
                id,
                None,
                &CancelToken::new(),
            )
            .unwrap()
            .unwrap()
        };
        let pages = open();
        assert_eq!(pages.info().pages, 3);
        // Its outline, read with the count: a page it has not, none.
        let outline: Vec<(&str, u32, Option<u32>)> = pages
            .info()
            .outlines
            .iter()
            .map(|o| (o.title.as_str(), o.depth, o.page))
            .collect();
        assert_eq!(outline, [("Start", 0, Some(1)), ("Beyond", 16, None)]);
        // A plugin that may not use the network: its links closed.
        assert!(pages.closed_links().unwrap().contains("network"));
        let cancel = CancelToken::new();
        let first = pages.render(1, 256, &cancel).unwrap();
        assert_eq!((first.width, first.height), (1, 1));
        assert_eq!((first.content.width, first.content.height), (612., 792.));
        let text: String = first
            .content
            .chars
            .iter()
            .map(|c| c.text.as_str())
            .collect();
        assert_eq!(text, "page 1");
        // Its links: to the next page, to an address; one to a page it
        // does not have is dropped.
        let links: Vec<&Destination> = first.content.links.iter().map(|l| &l.destination).collect();
        assert_eq!(
            links,
            [
                &Destination::Page(2),
                &Destination::Uri("https://example.com/".into())
            ]
        );
        // Its text is kept: drawn larger, it is not asked again.
        pages.render(1, 1024, &cancel).unwrap();
        // A search reads every page's text, never drawing one.
        let hits = pages.search("PAGE 3", &cancel).unwrap();
        assert_eq!(hits.len(), 1);
        // Six boxes: the space is the run's own, drawn there.
        assert_eq!((hits[0].page, hits[0].bounds.len()), (3, 6));
        assert_eq!(pages.search("page", &cancel).unwrap().len(), 3);
        assert_eq!(
            *three.0.lock(),
            [
                (1, 0, false),
                (1, 256, true),
                (1, 1024, false),
                (2, 0, true),
                (3, 0, true),
            ]
        );
        // Opened again, all of it is kept: the plugin is asked nothing.
        let again = open();
        again.render(1, 256, &cancel).unwrap();
        again.search("page", &cancel).unwrap();
        assert_eq!(three.0.lock().len(), 5);
        assert!(again.render(4, 256, &cancel).is_err(), "no page 4");
    }

    /// A plugin that says a file has every page there can be.
    struct Endless;
    impl crate::call::Transport for Endless {
        fn call(
            &self,
            _method: &str,
            _params: serde_json::Value,
            _call: &Call,
        ) -> anyhow::Result<serde_json::Value> {
            Ok(serde_json::to_value(PageAnswer {
                pages: u32::MAX,
                png: None,
                width: None,
                height: None,
                text: Vec::new(),
                links: Vec::new(),
                outline: vec![OutlineEntry {
                    title: "The last".into(),
                    depth: 0,
                    page: Some(u32::MAX),
                }],
            })?)
        }
        fn stop(&self) {}
    }

    #[test]
    fn a_plugins_page_count_is_held_to_what_the_reader_reads() {
        let base = std::env::home_dir().unwrap().join(".cache/skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        let tmp = tempfile::Builder::new()
            .prefix("pages-")
            .tempdir_in(base)
            .unwrap();
        let file = tmp.path().join("it's endless.doc");
        std::fs::write(&file, "endless").unwrap();
        let module = tmp.path().join("plugin.wasm");
        std::fs::write(&module, "module").unwrap();
        let runner = Arc::new(Runner::answered_by(&module, Arc::new(Endless)));
        let cache = skwir_core::cache::Cache::new(tmp.path().join("cache"));
        let id = skwir_core::op::EntryRef::from_path(&file).unwrap().id;
        let pages = Pages::open(
            runner,
            cache.namespace("endless").within("pages"),
            Arc::new(Held::open(&file, skwir_core::preview::WHOLE).unwrap()),
            id,
            None,
            &CancelToken::new(),
        )
        .unwrap()
        .unwrap();
        // Four billion items laid out on the window's thread: the reader's
        // `ListState` hung, or ran out of memory.
        assert_eq!(pages.info().pages, MAX_PAGES);
        assert_eq!(pages.info().outlines[0].page, None, "past what is read");
        let past = pages.render(MAX_PAGES + 1, 64, &CancelToken::new());
        assert!(format!("{:#}", past.unwrap_err()).contains("no page"));
    }
}
