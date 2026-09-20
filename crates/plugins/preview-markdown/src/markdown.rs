//! Event-based Markdown reader: pulldown-cmark's events become semantic
//! blocks directly, so headings, list depth, list markers, quotes, rules and
//! table rows keep their structure instead of being flattened to HTML and
//! scraped back. Inline emphasis, code and links survive as runs.
//!
//! Raw HTML blocks are the exception: Markdown allows arbitrary HTML, and
//! READMEs put logos and centered headers in it, so a run of HTML events is
//! handed to the shared reading-mode HTML reader rather than dropped. Its
//! images go through the same bounded, document-local loader.
use bibli_core::{
    CancelToken,
    inspection::{Block, BlockKind, Run},
    thumbs::Image,
};
use bibli_platform::{
    inspection::{check, limits},
    rich_preview,
};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

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
    if current.text.is_empty() && current.image.is_none() && current.kind == BlockKind::Text {
        *current = Block::default();
        return;
    }
    if current.marker.is_some() {
        *marker = None;
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

/// One finished table row, taking the cells collected for it.
fn row(cells: &mut Vec<String>, header: bool, quote_depth: u32) -> Block {
    Block {
        kind: BlockKind::TableRow {
            cells: std::mem::take(cells),
            header,
        },
        quote: quote_depth > 0,
        ..Block::default()
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
    image: &mut impl FnMut(&str) -> Option<Image>,
    quote_depth: u32,
) -> anyhow::Result<()> {
    let source = std::mem::take(html);
    if source.trim().is_empty() {
        return Ok(());
    }
    for mut block in rich_preview::html(&source, cancel, image)? {
        block.quote |= quote_depth > 0;
        blocks.push(block);
    }
    Ok(())
}

pub fn parse(
    source: &str,
    cancel: &CancelToken,
    mut image: impl FnMut(&str) -> Option<Image>,
) -> anyhow::Result<Vec<Block>> {
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS | Options::ENABLE_STRIKETHROUGH;
    let mut blocks = Vec::new();
    let mut current = Block::default();
    let mut inline = Inline::default();
    let mut quote_depth = 0u32;
    let mut lists: Vec<Option<u64>> = Vec::new();
    let mut marker: Option<String> = None;
    let mut alt = String::new();
    let mut pending: Option<Image> = None;
    let mut in_image = false;
    let mut in_table = false;
    let mut table_header = false;
    let mut cells: Vec<String> = Vec::new();
    let mut cell = String::new();
    let mut html = String::new();
    // One budget for every image in the document, Markdown's and raw HTML's.
    let mut images_seen = 0usize;
    let mut load = |href: &str| -> Option<Image> {
        let within = images_seen < limits::EMBEDDED_IMAGES;
        images_seen += 1;
        within.then(|| image(href)).flatten()
    };

    for event in Parser::new_ext(source, options) {
        check(cancel)?;
        anyhow::ensure!(
            blocks.len() < limits::BLOCKS,
            "Document exceeds {} preview blocks",
            limits::BLOCKS
        );
        if in_table {
            match event {
                Event::Start(Tag::TableHead) => table_header = true,
                Event::End(TagEnd::TableHead) => {
                    table_header = false;
                    // The head row may close without its own `TableRow` event.
                    if !cells.is_empty() {
                        blocks.push(row(&mut cells, true, quote_depth));
                    }
                }
                Event::Start(Tag::TableRow) => cells.clear(),
                Event::Start(Tag::TableCell) => cell.clear(),
                Event::End(TagEnd::TableCell) => cells.push(std::mem::take(&mut cell)),
                Event::End(TagEnd::TableRow) => {
                    blocks.push(row(&mut cells, table_header, quote_depth));
                }
                Event::End(TagEnd::Table) => in_table = false,
                Event::Text(text) | Event::Code(text) => cell.push_str(text.as_ref()),
                Event::SoftBreak => cell.push(' '),
                Event::HardBreak => cell.push('\n'),
                _ => {}
            }
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
            Event::End(TagEnd::Paragraph | TagEnd::Heading(_)) => {
                flush(&mut blocks, &mut current, &mut marker);
            }
            Event::Start(Tag::Heading { level: tag, .. }) => {
                flush(&mut blocks, &mut current, &mut marker);
                begin(&mut current, quote_depth, &lists, marker.as_deref());
                current.heading = level(tag);
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
                flush(&mut blocks, &mut current, &mut marker);
            }
            Event::Start(Tag::Table(_)) => {
                flush(&mut blocks, &mut current, &mut marker);
                in_table = true;
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
                let bitmap = pending.take();
                let text = std::mem::take(&mut alt);
                begin(&mut current, quote_depth, &lists, marker.as_deref());
                if bitmap.is_some() || !text.is_empty() {
                    current.text = text;
                    current.image = bitmap;
                    flush(&mut blocks, &mut current, &mut marker);
                    begin(&mut current, quote_depth, &lists, marker.as_deref());
                }
            }
            Event::Start(Tag::Emphasis) => inline.italic += 1,
            Event::End(TagEnd::Emphasis) => inline.italic = inline.italic.saturating_sub(1),
            Event::Start(Tag::Strong) => inline.bold += 1,
            Event::End(TagEnd::Strong) => inline.bold = inline.bold.saturating_sub(1),
            Event::Start(Tag::Strikethrough) => inline.strikethrough += 1,
            Event::End(TagEnd::Strikethrough) => {
                inline.strikethrough = inline.strikethrough.saturating_sub(1);
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                inline.link = Some(dest_url.into_string());
            }
            Event::End(TagEnd::Link) => inline.link = None,
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
    Ok(blocks)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> Vec<Block> {
        super::parse(source, &CancelToken::new(), |_| None).unwrap()
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
        .unwrap();
        assert_eq!(asked, ["logo.png"], "its images use the bounded loader");
        assert!(blocks.iter().any(|b| b.text == "Logo"));
        assert!(blocks.iter().any(|b| b.text == "Text."));
    }

    #[test]
    fn every_image_in_the_document_shares_one_budget() {
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
        assert_eq!(asked, limits::EMBEDDED_IMAGES);
    }

    #[test]
    fn table_rows_become_cells() {
        let blocks = parse("| name | note |\n|---|---|\n| one | two |\n");
        let rows: Vec<_> = blocks.iter().map(|b| &b.kind).collect();
        assert_eq!(
            rows,
            [
                &BlockKind::TableRow {
                    cells: vec!["name".into(), "note".into()],
                    header: true
                },
                &BlockKind::TableRow {
                    cells: vec!["one".into(), "two".into()],
                    header: false
                },
            ]
        );
        assert!(blocks.iter().all(|b| b.text.is_empty()));
        assert_eq!(blocks[1].plain(), "one\ttwo");
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
