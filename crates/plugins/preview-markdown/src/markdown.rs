//! Event-based Markdown reader: pulldown-cmark's events become semantic
//! blocks directly, so headings, list depth, list markers, quotes, rules and
//! table rows keep their structure instead of being flattened to HTML and
//! scraped back. Inline emphasis, code and links survive as runs.
//!
//! Raw HTML blocks are the exception: Markdown allows arbitrary HTML, and
//! READMEs put logos and centered headers in it, so a run of HTML events is
//! handed to the shared reading-mode HTML reader rather than dropped. Its
//! images go through the same document-local loader, which names each
//! picture for the view to decode when it shows it.
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use skwir_core::{
    CancelToken,
    inspection::{Block, BlockKind, Picture, Run},
};
use skwir_platform::{
    inspection::{check, limits},
    rich_preview,
};

/// Bullet glyphs by nesting depth, cycling when the list goes deeper.
const BULLETS: [&str; 5] = ["•", "◦", "▪", "‣", "⁃"];

#[derive(Clone, Default)]
struct Inline {
    bold: usize,
    italic: usize,
    strikethrough: usize,
    code: usize,
    link: Option<String>,
}

impl Inline {
    fn run(&self, range: std::ops::Range<usize>) -> Run {
        Run {
            range,
            bold: self.bold > 0,
            italic: self.italic > 0,
            strikethrough: self.strikethrough > 0,
            code: self.code > 0,
            link: self.link.clone(),
            ..Run::default()
        }
    }
}

fn push(current: &mut Block, text: &str, inline: &Inline) {
    if text.is_empty() {
        return;
    }
    let start = current.text.len();
    current.text.push_str(text);
    current.runs.push(inline.run(start..current.text.len()));
}

/// Push `current` if it holds anything, and start the next block in the same
/// place in the document. The list marker belongs to the first block of its
/// item, so the block that takes it is also the one that spends it: a loose
/// item's later paragraphs must not repeat the bullet.
fn flush(blocks: &mut Vec<Block>, current: &mut Block, marker: &mut Option<String>) {
    if current.text.is_empty() && current.picture.is_none() && current.kind == BlockKind::Text {
        *current = Block::default();
        return;
    }
    if current.marker.is_some() {
        *marker = None;
    }
    // An address written bare is a link, as GitHub reads Markdown.
    if !current.code {
        skwir_platform::links::link_block(current, false);
    }
    blocks.push(std::mem::take(current));
}

/// Start a block in the ambient context: inside the open quotes, the open
/// lists, and carrying the list marker that no block has taken yet.
fn begin(current: &mut Block, quote_depth: u32, lists: &[Option<u64>], marker: Option<&str>) {
    *current = Block {
        quote: quote_depth > 0,
        list_depth: lists.len() as u32,
        marker: marker.map(str::to_owned),
        ..Block::default()
    };
}

/// The inline state an emphasis, link or strikethrough tag changes. A table
/// cell is text like any other, so both the block path and the table path
/// read these through here and keep their runs.
fn inline_event(inline: &mut Inline, event: &Event) -> bool {
    match event {
        Event::Start(Tag::Emphasis) => inline.italic += 1,
        Event::End(TagEnd::Emphasis) => inline.italic = inline.italic.saturating_sub(1),
        Event::Start(Tag::Strong) => inline.bold += 1,
        Event::End(TagEnd::Strong) => inline.bold = inline.bold.saturating_sub(1),
        Event::Start(Tag::Strikethrough) => inline.strikethrough += 1,
        Event::End(TagEnd::Strikethrough) => {
            inline.strikethrough = inline.strikethrough.saturating_sub(1);
        }
        Event::Start(Tag::Link { dest_url, .. }) => inline.link = Some(dest_url.to_string()),
        Event::End(TagEnd::Link) => inline.link = None,
        _ => return false,
    }
    true
}

/// Heading link targets, named the way GitHub names them, since that is
/// where READMEs' `[see](#the-plan)` links were written against: lowercase,
/// a space becomes a hyphen, and everything but letters, digits, `-` and
/// `_` is dropped; a repeated target gets `-1`, `-2` and so on. An explicit
/// `{#id}` is kept as written and only made unique.
#[derive(Default)]
struct Anchors {
    /// Every target handed out, with the next suffix to try for it.
    used: std::collections::HashMap<String, usize>,
}
impl Anchors {
    fn slug(&mut self, text: &str) -> String {
        let base: String = text
            .trim()
            .chars()
            .filter_map(|c| match c {
                ' ' => Some('-'),
                c if c.is_alphanumeric() || c == '-' || c == '_' => Some(c),
                _ => None,
            })
            .flat_map(char::to_lowercase)
            .collect();
        self.unique(base)
    }
    fn unique(&mut self, base: String) -> String {
        let Some(next) = self.used.get(&base).copied() else {
            self.used.insert(base.clone(), 1);
            return base;
        };
        // Each base remembers where its count stopped, so a document that
        // repeats one heading a thousand times costs a thousand lookups.
        let mut seen = next;
        let candidate = loop {
            let candidate = format!("{base}-{seen}");
            seen += 1;
            if !self.used.contains_key(&candidate) {
                break candidate;
            }
        };
        self.used.insert(base, seen);
        self.used.insert(candidate.clone(), 1);
        candidate
    }
}

fn level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// Read a run of raw HTML with the shared reading-mode parser and append what
/// it found. Markdown allows arbitrary HTML blocks, so dropping them loses a
/// README's logo, badges and centered headers.
fn emit_html(
    blocks: &mut Vec<Block>,
    html: &mut String,
    cancel: &CancelToken,
    image: &mut impl FnMut(&str) -> Option<Picture>,
    quote_depth: u32,
) -> anyhow::Result<()> {
    let source = std::mem::take(html);
    if source.trim().is_empty() {
        return Ok(());
    }
    let page = rich_preview::html(
        &source,
        rich_preview::Markup::Html,
        cancel,
        rich_preview::named(image),
    )?;
    for mut block in page.blocks {
        block.quote |= quote_depth > 0;
        blocks.push(block);
    }
    Ok(())
}

/// The document's blocks, up to `limits::BLOCKS`: a longer one (a changelog
/// is a list item per line) is cut there and says so (`Page::truncated`),
/// its source one toggle away.
pub fn parse(
    source: &str,
    cancel: &CancelToken,
    mut load: impl FnMut(&str) -> Option<Picture>,
) -> anyhow::Result<rich_preview::Page> {
    // Metadata blocks are enabled so that a note's front matter is recognized
    // and skipped, rather than read as a rule and a large heading.
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_HEADING_ATTRIBUTES
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
        | Options::ENABLE_PLUSES_DELIMITED_METADATA_BLOCKS;
    let mut blocks = Vec::new();
    let mut current = Block::default();
    let mut inline = Inline::default();
    let mut quote_depth = 0u32;
    let mut lists: Vec<Option<u64>> = Vec::new();
    let mut marker: Option<String> = None;
    let mut alt = String::new();
    let mut pending: Option<Picture> = None;
    let mut in_image = false;
    let mut in_table = false;
    let mut in_metadata = false;
    let mut table_header = false;
    let mut cells: Vec<std::ops::Range<usize>> = Vec::new();
    let mut cell_start = 0usize;
    let mut html = String::new();
    let mut anchors = Anchors::default();
    let mut diagrams = 0;

    for event in Parser::new_ext(source, options) {
        check(cancel)?;
        if blocks.len() >= limits::BLOCKS {
            return Ok(rich_preview::Page {
                blocks: cut(blocks),
                truncated: true,
            });
        }
        if in_metadata {
            if matches!(event, Event::End(TagEnd::MetadataBlock(_))) {
                in_metadata = false;
            }
            continue;
        }
        // A table row is collected into `current` like any other text, so a
        // cell keeps its emphasis, code and links; the cells are the ranges
        // of that text the view draws in columns.
        if in_table {
            if inline_event(&mut inline, &event) {
                continue;
            }
            let mut finish = |current: &mut Block, cells: &mut Vec<_>, header| {
                current.kind = BlockKind::TableRow {
                    cells: std::mem::take(cells),
                    header,
                };
                skwir_platform::links::link_block(current, false);
                blocks.push(std::mem::take(current));
            };
            match event {
                Event::Start(Tag::TableHead | Tag::TableRow) => {
                    table_header = matches!(event, Event::Start(Tag::TableHead));
                    cells.clear();
                    begin(&mut current, quote_depth, &lists, None);
                }
                // The head row may close without its own `TableRow` event.
                Event::End(TagEnd::TableHead) => {
                    if !cells.is_empty() {
                        finish(&mut current, &mut cells, true);
                    }
                    table_header = false;
                }
                Event::End(TagEnd::TableRow) => finish(&mut current, &mut cells, table_header),
                Event::Start(Tag::TableCell) => {
                    if !cells.is_empty() {
                        current.text.push('\t');
                    }
                    cell_start = current.text.len();
                }
                Event::End(TagEnd::TableCell) => cells.push(cell_start..current.text.len()),
                Event::End(TagEnd::Table) => {
                    in_table = false;
                    begin(&mut current, quote_depth, &lists, marker.as_deref());
                }
                Event::Text(text) => push(&mut current, text.as_ref(), &inline),
                Event::Code(text) => {
                    inline.code += 1;
                    push(&mut current, text.as_ref(), &inline);
                    inline.code -= 1;
                }
                Event::SoftBreak => push(&mut current, " ", &inline),
                Event::HardBreak => push(&mut current, "\n", &inline),
                _ => {}
            }
            continue;
        }
        if inline_event(&mut inline, &event) {
            continue;
        }
        // A run of raw HTML ends here: read it where it stood.
        if !html.is_empty() && !matches!(event, Event::Html(_)) {
            flush(&mut blocks, &mut current, &mut marker);
            emit_html(&mut blocks, &mut html, cancel, &mut load, quote_depth)?;
            begin(&mut current, quote_depth, &lists, marker.as_deref());
        }
        match event {
            Event::Start(Tag::Paragraph) => {
                flush(&mut blocks, &mut current, &mut marker);
                begin(&mut current, quote_depth, &lists, marker.as_deref());
            }
            Event::End(TagEnd::Paragraph) => {
                flush(&mut blocks, &mut current, &mut marker);
            }
            Event::End(TagEnd::Heading(_)) => {
                if current.anchors.is_empty() {
                    current.anchors.push(anchors.slug(&current.text));
                }
                flush(&mut blocks, &mut current, &mut marker);
            }
            Event::Start(Tag::Heading { level: tag, id, .. }) => {
                flush(&mut blocks, &mut current, &mut marker);
                begin(&mut current, quote_depth, &lists, marker.as_deref());
                current.heading = level(tag);
                current.anchors = id
                    .map(|id| anchors.unique(id.to_string()))
                    .into_iter()
                    .collect();
            }
            Event::Start(Tag::BlockQuote(_)) => {
                flush(&mut blocks, &mut current, &mut marker);
                quote_depth += 1;
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                flush(&mut blocks, &mut current, &mut marker);
                quote_depth = quote_depth.saturating_sub(1);
            }
            Event::Start(Tag::List(start)) => {
                flush(&mut blocks, &mut current, &mut marker);
                lists.push(start);
            }
            Event::End(TagEnd::List(_)) => {
                flush(&mut blocks, &mut current, &mut marker);
                lists.pop();
            }
            Event::Start(Tag::Item) => {
                flush(&mut blocks, &mut current, &mut marker);
                marker = Some(if let Some(Some(next)) = lists.last_mut() {
                    let number = *next;
                    *next += 1;
                    format!("{number}.")
                } else {
                    let depth = lists.len().saturating_sub(1);
                    BULLETS[depth % BULLETS.len()].to_string()
                });
                begin(&mut current, quote_depth, &lists, marker.as_deref());
            }
            Event::End(TagEnd::Item) => {
                flush(&mut blocks, &mut current, &mut marker);
                marker = None;
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                flush(&mut blocks, &mut current, &mut marker);
                begin(&mut current, quote_depth, &lists, marker.as_deref());
                current.code = true;
                current.language = match kind {
                    CodeBlockKind::Fenced(info) => info
                        .split_whitespace()
                        .next()
                        .filter(|token| !token.is_empty())
                        .map(str::to_owned),
                    CodeBlockKind::Indented => None,
                };
            }
            Event::End(TagEnd::CodeBlock) => {
                if current.text.ends_with('\n') {
                    current.text.pop();
                }
                // A diagram is part of the page, like a drawing in HTML:
                // drawn with it, a few per page; past them, or when it
                // cannot be drawn, it stays the code it is written in.
                // Every one tried counts, drawn or not: a page of diagrams
                // that fail ran the helper for each, its deadline each time.
                if current.language.as_deref() == Some("mermaid")
                    && diagrams < limits::EMBEDDED_IMAGES
                    && let Some(image) = {
                        diagrams += 1;
                        crate::diagram::mermaid(&current.text, cancel)
                    }
                {
                    current.text.clear();
                    current.code = false;
                    current.language = None;
                    current.picture = Some(Picture::Ready(image));
                }
                flush(&mut blocks, &mut current, &mut marker);
            }
            Event::Start(Tag::Table(_)) => {
                flush(&mut blocks, &mut current, &mut marker);
                in_table = true;
            }
            Event::Start(Tag::MetadataBlock(_)) => {
                flush(&mut blocks, &mut current, &mut marker);
                in_metadata = true;
            }
            Event::Start(Tag::Image { dest_url, .. }) => {
                flush(&mut blocks, &mut current, &mut marker);
                in_image = true;
                alt.clear();
                pending = load(dest_url.as_ref());
            }
            // An image is a block of its own, so the paragraph around it is
            // cut in two; both halves stay in the quote or list item they
            // were written in.
            Event::End(TagEnd::Image) => {
                in_image = false;
                let picture = pending.take();
                let text = std::mem::take(&mut alt);
                begin(&mut current, quote_depth, &lists, marker.as_deref());
                if picture.is_some() || !text.is_empty() {
                    current.text = text;
                    current.picture = picture;
                    flush(&mut blocks, &mut current, &mut marker);
                    begin(&mut current, quote_depth, &lists, marker.as_deref());
                }
            }
            Event::Text(text) => {
                if in_image {
                    alt.push_str(text.as_ref());
                } else {
                    push(&mut current, text.as_ref(), &inline);
                }
            }
            Event::Code(text) => {
                inline.code += 1;
                push(&mut current, text.as_ref(), &inline);
                inline.code -= 1;
            }
            Event::SoftBreak => {
                if !in_image {
                    push(&mut current, " ", &inline);
                }
            }
            Event::HardBreak => {
                if !in_image {
                    push(&mut current, "\n", &inline);
                }
            }
            Event::Rule => {
                flush(&mut blocks, &mut current, &mut marker);
                blocks.push(Block {
                    kind: BlockKind::Rule,
                    quote: quote_depth > 0,
                    list_depth: lists.len() as u32,
                    ..Block::default()
                });
            }
            Event::TaskListMarker(checked) => {
                push(&mut current, if checked { "☑ " } else { "☐ " }, &inline);
            }
            Event::InlineHtml(text) => {
                if matches!(text.trim(), "<br>" | "<br/>" | "<br />") {
                    push(&mut current, "\n", &inline);
                }
            }
            Event::Html(text) => html.push_str(text.as_ref()),
            _ => {}
        }
    }
    flush(&mut blocks, &mut current, &mut marker);
    emit_html(&mut blocks, &mut html, cancel, &mut load, quote_depth)?;
    Ok(rich_preview::Page {
        truncated: blocks.len() > limits::BLOCKS,
        blocks: cut(blocks),
    })
}

/// A run of raw HTML may add several blocks at once: keep to the bound.
fn cut(mut blocks: Vec<Block>) -> Vec<Block> {
    blocks.truncate(limits::BLOCKS);
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> Vec<Block> {
        super::parse(source, &CancelToken::new(), |_| None)
            .unwrap()
            .blocks
    }

    #[test]
    fn headings_and_inline_runs_keep_their_structure() {
        let blocks = parse(
            "# Title\n\nA **bold** *italic* ~~gone~~ `code` [link](https://example.invalid).\n",
        );
        assert_eq!(blocks[0].heading, 1);
        assert_eq!(blocks[0].text, "Title");
        let text = &blocks[1].text;
        let runs = &blocks[1].runs;
        let run = |pick: &dyn Fn(&Run) -> bool| {
            let r = runs.iter().find(|r| pick(r)).expect("run");
            &text[r.range.clone()]
        };
        assert_eq!(run(&|r| r.bold), "bold");
        assert_eq!(run(&|r| r.italic), "italic");
        assert_eq!(run(&|r| r.strikethrough), "gone");
        assert_eq!(run(&|r| r.code), "code");
        assert_eq!(
            runs.iter()
                .find(|r| r.link.as_deref() == Some("https://example.invalid"))
                .map(|r| &text[r.range.clone()]),
            Some("link")
        );
    }

    /// Point `SKWIR_MERMAID_WORKER` at `worker`. Tests run in processes of
    /// their own under nextest, so the variable reaches no other test.
    fn use_worker(worker: &std::path::Path) {
        // SAFETY: no other thread of this test process reads the environment.
        #[allow(unsafe_code)]
        unsafe {
            std::env::set_var("SKWIR_MERMAID_WORKER", worker);
        }
    }

    /// The helper built into the target directory this test runs from
    /// (the test binary sits in its `deps`). Without it the test says so
    /// and passes, unless `SKWIR_REQUIRE_WORKERS` is set.
    fn with_mermaid_worker() -> bool {
        let built = std::env::current_exe()
            .ok()
            .and_then(|exe| Some(exe.parent()?.parent()?.join("skwir-mermaid-worker")))
            .filter(|path| path.is_file());
        let Some(worker) = built else {
            assert!(
                std::env::var_os("SKWIR_REQUIRE_WORKERS").is_none(),
                "skwir-mermaid-worker is not built"
            );
            eprintln!("skipped: skwir-mermaid-worker is not built");
            return false;
        };
        use_worker(&worker);
        true
    }

    const DIAGRAMS: &str = "```mermaid\nflowchart LR\n  A[Read] --> B[Show]\n```\n\n```mermaid\nnot a diagram at all\n```\n";

    #[test]
    fn a_mermaid_block_is_drawn_and_a_broken_one_stays_code() {
        if !with_mermaid_worker() {
            return;
        }
        let blocks = parse(DIAGRAMS);
        let Some(Picture::Ready(image)) = &blocks[0].picture else {
            panic!("the diagram is a picture: {:?}", blocks[0].text);
        };
        assert!(!blocks[0].code);
        assert!(image.width > image.height, "laid out left to right");
        // On white, and something drawn on it.
        assert_eq!(&image.rgba[..4], &[255, 255, 255, 255]);
        assert!(image.rgba.chunks(4).any(|pixel| pixel[0] < 128));
        assert!(blocks[1].picture.is_none());
        assert!(blocks[1].code);
        assert_eq!(blocks[1].language.as_deref(), Some("mermaid"));
        assert_eq!(blocks[1].text, "not a diagram at all");
    }

    #[test]
    fn without_its_helper_a_mermaid_block_stays_code() {
        use_worker(std::path::Path::new("/nonexistent/skwir-mermaid-worker"));
        let blocks = parse(DIAGRAMS);
        assert!(
            blocks
                .iter()
                .all(|block| block.picture.is_none() && block.code)
        );
        assert_eq!(blocks[0].text, "flowchart LR\n  A[Read] --> B[Show]");
    }

    #[test]
    fn list_items_carry_depth_and_a_marker() {
        let blocks = parse("- one\n  - nested\n\n1. first\n2. second\n");
        assert_eq!(blocks[0].marker.as_deref(), Some("•"));
        assert_eq!(blocks[0].list_depth, 1);
        assert_eq!(blocks[1].marker.as_deref(), Some("◦"));
        assert_eq!(blocks[1].list_depth, 2);
        assert_eq!(blocks[2].marker.as_deref(), Some("1."));
        assert_eq!(blocks[3].marker.as_deref(), Some("2."));
    }

    #[test]
    fn a_marker_belongs_to_the_first_block_of_its_item() {
        let blocks =
            parse("- first para\n\n  second para\n\n  ```rs\n  fn a() {}\n  ```\n\n- next\n");
        let markers: Vec<_> = blocks.iter().map(|b| b.marker.as_deref()).collect();
        assert_eq!(markers, [Some("•"), None, None, Some("•")]);
        assert!(
            blocks.iter().all(|b| b.list_depth == 1),
            "every block of the item stays in the list"
        );
    }

    #[test]
    fn an_image_splits_a_paragraph_without_leaving_its_context() {
        let blocks = parse("> Before ![alt](x.png) after.\n");
        assert_eq!(
            blocks.iter().map(|b| b.text.as_str()).collect::<Vec<_>>(),
            ["Before ", "alt", " after."]
        );
        assert!(
            blocks.iter().all(|b| b.quote),
            "the halves stay inside the quote"
        );
        let blocks = parse("- Before ![alt](x.png) after.\n");
        assert!(blocks.iter().all(|b| b.list_depth == 1));
        assert_eq!(
            blocks.iter().filter(|b| b.marker.is_some()).count(),
            1,
            "one bullet for the item"
        );
    }

    #[test]
    fn raw_html_blocks_are_read_rather_than_dropped() {
        let mut asked = Vec::new();
        let blocks = super::parse(
            "<p align=\"center\"><img src=\"logo.png\" alt=\"Logo\"></p>\n\nText.\n",
            &CancelToken::new(),
            |href| {
                asked.push(href.to_owned());
                None
            },
        )
        .unwrap()
        .blocks;
        assert_eq!(asked, ["logo.png"], "its images use the document's loader");
        assert!(blocks.iter().any(|b| b.text == "Logo"));
        assert!(blocks.iter().any(|b| b.text == "Text."));
    }

    #[test]
    fn every_image_in_the_document_goes_through_one_loader() {
        use std::fmt::Write;
        let mut source = String::new();
        for i in 0..limits::EMBEDDED_IMAGES + 4 {
            let _ = if i % 2 == 0 {
                write!(source, "![a{i}](a{i}.png)\n\n")
            } else {
                write!(source, "<img src=\"b{i}.png\" alt=\"b{i}\">\n\n")
            };
        }
        let mut asked = 0usize;
        super::parse(&source, &CancelToken::new(), |_| {
            asked += 1;
            None
        })
        .unwrap();
        // Naming a picture costs its header: none is left out.
        assert_eq!(asked, limits::EMBEDDED_IMAGES + 4);
    }

    /// The cells of a row, read back through the ranges the view draws.
    fn cells_of(block: &Block) -> (Vec<&str>, bool) {
        let BlockKind::TableRow { cells, header } = &block.kind else {
            panic!("not a table row");
        };
        (
            cells.iter().map(|r| &block.text[r.clone()]).collect(),
            *header,
        )
    }

    #[test]
    fn table_rows_are_text_with_the_cells_as_ranges() {
        let blocks = parse("| name | note |\n|---|---|\n| one | two |\n");
        assert_eq!(cells_of(&blocks[0]), (vec!["name", "note"], true));
        assert_eq!(cells_of(&blocks[1]), (vec!["one", "two"], false));
        // The row reads as one line, so copying and searching need no case.
        assert_eq!(blocks[1].text, "one\ttwo");
    }

    /// Each link of `block`: its text, where it leads, whether found.
    fn links(block: &Block) -> Vec<(&str, &str, bool)> {
        block
            .runs
            .iter()
            .filter_map(|r| Some((&block.text[r.range.clone()], r.link.as_deref()?, r.found)))
            .collect()
    }

    #[test]
    fn a_bare_address_is_a_link_as_github_reads_it() {
        let blocks = parse(
            "See https://example.org/a_b_c. Not `https://code.example`, \
             [named](https://named.example) or *https://em.example/x*.\n\n\
             | where |\n|---|\n| mailto:a@b.example |\n",
        );
        assert_eq!(
            links(&blocks[0]),
            [
                (
                    "https://example.org/a_b_c",
                    "https://example.org/a_b_c",
                    false
                ),
                ("named", "https://named.example", false),
                ("https://em.example/x", "https://em.example/x", false),
            ]
        );
        assert_eq!(
            links(&blocks[2]),
            [("mailto:a@b.example", "mailto:a@b.example", false)]
        );
    }

    #[test]
    fn a_cell_keeps_its_emphasis_and_links() {
        let blocks = parse("| what | where |\n|---|---|\n| **bold** `code` | [doc](d.md) |\n");
        let row = &blocks[1];
        assert_eq!(cells_of(row).0, vec!["bold code", "doc"]);
        let run = |pick: &dyn Fn(&Run) -> bool| {
            let r = row.runs.iter().find(|r| pick(r)).expect("run");
            &row.text[r.range.clone()]
        };
        assert_eq!(run(&|r| r.bold), "bold");
        assert_eq!(run(&|r| r.code), "code");
        assert_eq!(run(&|r| r.link.as_deref() == Some("d.md")), "doc");
    }

    #[test]
    fn headings_carry_a_link_target() {
        let blocks = parse(
            "# The Plan!\n\n## The Plan\n\n### Explicit {#Named_Part}\n\n## my_func\n\n## a -- b\n\n## The Plan-1\n",
        );
        let anchors: Vec<_> = blocks
            .iter()
            .map(|b| b.anchors.first().map(String::as_str))
            .collect();
        assert_eq!(
            anchors,
            [
                Some("the-plan"),
                Some("the-plan-1"),
                Some("Named_Part"),
                Some("my_func"),
                Some("a----b"),
                Some("the-plan-1-1"),
            ],
            "GitHub's names, explicit ids verbatim, repeats made distinct"
        );
    }

    #[test]
    fn a_repeated_heading_costs_a_lookup_not_a_scan() {
        let source = "# a\n".repeat(limits::BLOCKS - 1);
        let started = std::time::Instant::now();
        let blocks = parse(&source);
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        assert_eq!(blocks[1].anchors, ["a-1"]);
        let last = format!("a-{}", limits::BLOCKS - 2);
        assert_eq!(blocks.last().unwrap().anchors, [last]);
    }

    #[test]
    fn a_long_document_is_cut_at_the_bound_not_refused() {
        let source = "- item\n".repeat(limits::BLOCKS + 500);
        let page = super::parse(&source, &CancelToken::new(), |_| None).unwrap();
        assert!(page.truncated);
        assert_eq!(page.blocks.len(), limits::BLOCKS);
        let short = super::parse("- one\n- two\n", &CancelToken::new(), |_| None).unwrap();
        assert!(!short.truncated);
    }

    #[test]
    fn front_matter_is_not_read_as_a_heading() {
        let blocks = parse("---\ntitle: Note\ntags: [a, b]\n---\n\n# Real heading\n\nBody.\n");
        assert_eq!(
            blocks.iter().map(|b| b.text.as_str()).collect::<Vec<_>>(),
            ["Real heading", "Body."]
        );
        assert!(blocks.iter().all(|b| b.kind == BlockKind::Text));
    }

    #[test]
    fn rules_quotes_and_code_are_marked() {
        let blocks = parse("---\n\n> quoted\n\n```rust\nfn main() {}\n```\n");
        assert_eq!(blocks[0].kind, BlockKind::Rule);
        assert!(blocks[1].quote);
        assert_eq!(blocks[1].text, "quoted");
        assert!(blocks[2].code);
        assert_eq!(blocks[2].language.as_deref(), Some("rust"));
        assert_eq!(blocks[2].text, "fn main() {}");
    }

    #[test]
    fn task_list_markers_are_kept_in_text() {
        let blocks = parse("- [x] done\n- [ ] todo\n");
        assert_eq!(blocks[0].text, "☑ done");
        assert_eq!(blocks[1].text, "☐ todo");
    }
}
