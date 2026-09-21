//! Native reading-mode HTML: semantic blocks, inline styles and local images.
//! Scripts, styles, frames and remote resources are never executed or fetched.
use crate::inspection::{check, limits};
use bibli_core::{
    CancelToken,
    inspection::{Block, Run},
    thumbs::Image,
};
use scraper::{Html, Node};

pub fn html(
    source: &str,
    cancel: &CancelToken,
    mut image: impl FnMut(&str) -> Option<Image>,
) -> anyhow::Result<Vec<Block>> {
    let doc = Html::parse_document(source);
    let mut blocks = Vec::new();
    let mut current = Block::default();
    let mut group = None;
    let mut images = 0;
    // Whether the block being filled is a table row, and a header row.
    let mut row: Option<bool> = None;
    // List items that already got their bullet: it belongs to the first block
    // of an item, not to every paragraph inside it.
    let mut marked = std::collections::HashSet::new();
    for node in doc.tree.root().descendants() {
        check(cancel)?;
        anyhow::ensure!(
            blocks.len() < limits::BLOCKS,
            "Document exceeds {} preview blocks",
            limits::BLOCKS
        );
        let ancestors: Vec<_> = node
            .ancestors()
            .filter_map(|n| n.value().as_element().map(|e| (n.id(), e)))
            .take(limits::HTML_DEPTH + 1)
            .collect();
        if ancestors.len() > limits::HTML_DEPTH
            || ancestors.iter().any(|(_, e)| {
                matches!(
                    e.name(),
                    "script" | "style" | "head" | "iframe" | "object" | "noscript"
                )
            })
        {
            continue;
        }
        let block = ancestors.iter().find(|(_, e)| {
            matches!(
                e.name(),
                "p" | "h1"
                    | "h2"
                    | "h3"
                    | "h4"
                    | "h5"
                    | "h6"
                    | "li"
                    | "pre"
                    | "tr"
                    | "blockquote"
                    | "div"
            )
        });
        let id = block.map(|(id, _)| *id);
        if group != id && (!current.text.is_empty() || current.image.is_some()) {
            blocks.push(finish(std::mem::take(&mut current), row.take()));
        }
        if group != id {
            row = block.filter(|(_, e)| e.name() == "tr").map(|_| false);
        }
        group = id;
        if current.anchor.is_none() {
            current.anchor = block.and_then(|(_, e)| e.attr("id").map(str::to_owned));
        }
        current.language = ancestors.iter().find_map(|(_, e)| {
            e.attr("class").and_then(|c| {
                c.split_whitespace()
                    .find_map(|c| c.strip_prefix("language-").map(str::to_owned))
            })
        });
        current.code = ancestors.iter().any(|(_, e)| e.name() == "pre");
        current.quote = ancestors.iter().any(|(_, e)| e.name() == "blockquote");
        if let Some((_, e)) = block {
            current.heading = match e.name() {
                "h1" => 1,
                "h2" => 2,
                "h3" => 3,
                "h4" => 4,
                "h5" => 5,
                "h6" => 6,
                _ => 0,
            };
        }
        // A list item is structure, as in Markdown: a depth and a marker drawn
        // beside the text, not a bullet typed into it.
        let lists = ancestors
            .iter()
            .filter(|(_, e)| matches!(e.name(), "ul" | "ol"))
            .count();
        if let Some((item, _)) = ancestors.iter().find(|(_, e)| e.name() == "li") {
            current.list_depth = lists.max(1) as u32;
            if current.text.is_empty() && current.marker.is_none() && marked.insert(*item) {
                let ordered = ancestors
                    .iter()
                    .find(|(_, e)| matches!(e.name(), "ul" | "ol"))
                    .filter(|(_, e)| e.name() == "ol");
                current.marker = Some(match (ordered, doc.tree.get(*item)) {
                    (Some((_, list)), Some(node)) => {
                        let before = node
                            .prev_siblings()
                            .filter(|n| n.value().as_element().is_some_and(|e| e.name() == "li"))
                            .count();
                        let start = list
                            .attr("start")
                            .and_then(|start| start.parse::<usize>().ok())
                            .unwrap_or(1);
                        format!("{}.", start + before)
                    }
                    _ => "•".into(),
                });
            }
        } else {
            current.list_depth = 0;
        }
        // The one piece of a page's own styling kept: what is centred stays so.
        current.centered = ancestors.iter().any(|(_, e)| {
            e.name() == "center"
                || e.attr("align")
                    .is_some_and(|a| a.eq_ignore_ascii_case("center"))
                || styled(e, "text-align", &["center"])
        });
        match node.value() {
            Node::Text(text) => {
                let text = if current.code {
                    text.to_string()
                } else {
                    let mut out = String::new();
                    for c in text.chars() {
                        if c.is_whitespace() {
                            if !out.ends_with(' ') {
                                out.push(' ');
                            }
                        } else {
                            out.push(c);
                        }
                    }
                    out
                };
                if current.text.is_empty() && text.trim().is_empty() {
                    continue;
                }
                let start = current.text.len();
                current.text.push_str(&text);
                current.runs.push(Run {
                    color: None,
                    range: start..current.text.len(),
                    bold: ancestors.iter().any(|(_, e)| {
                        matches!(e.name(), "b" | "strong" | "th")
                            || styled(
                                e,
                                "font-weight",
                                &["bold", "bolder", "600", "700", "800", "900"],
                            )
                    }),
                    italic: ancestors.iter().any(|(_, e)| {
                        matches!(e.name(), "i" | "em" | "cite" | "figcaption")
                            || styled(e, "font-style", &["italic", "oblique"])
                    }),
                    strikethrough: ancestors
                        .iter()
                        .any(|(_, e)| matches!(e.name(), "del" | "s" | "strike")),
                    code: ancestors.iter().any(|(_, e)| e.name() == "code"),
                    link: ancestors.iter().find_map(|(_, e)| {
                        if e.name() == "a" {
                            e.attr("href").map(str::to_owned)
                        } else {
                            None
                        }
                    }),
                });
            }
            Node::Element(e) if e.name() == "br" => current.text.push('\n'),
            // Cells are ranges of the row's text, joined by tabs, as Markdown's
            // are: emphasis and links inside a cell stay ordinary runs.
            Node::Element(e) if matches!(e.name(), "td" | "th") => {
                if let Some(header) = row.as_mut() {
                    *header |= e.name() == "th";
                }
                if !current.text.is_empty() {
                    current.text.push('\t');
                }
            }
            Node::Element(e) if e.name() == "hr" => {
                if !current.text.is_empty() {
                    blocks.push(finish(std::mem::take(&mut current), row.take()));
                }
                blocks.push(Block {
                    kind: bibli_core::inspection::BlockKind::Rule,
                    ..Block::default()
                });
            }
            Node::Element(e) if e.name() == "input" && e.attr("type") == Some("checkbox") => {
                current.text.push_str(if e.attr("checked").is_some() {
                    "☑ "
                } else {
                    "☐ "
                });
            }
            Node::Element(e) if e.name() == "img" => {
                if !current.text.is_empty() {
                    blocks.push(finish(std::mem::take(&mut current), row.take()));
                }
                let bitmap = if images < limits::EMBEDDED_IMAGES {
                    e.attr("src").and_then(&mut image)
                } else {
                    None
                };
                images += 1;
                blocks.push(Block {
                    text: e.attr("alt").unwrap_or_default().into(),
                    image: bitmap,
                    centered: current.centered,
                    ..Block::default()
                });
            }
            _ => {}
        }
    }
    if !current.text.is_empty() {
        blocks.push(finish(current, row));
    }
    Ok(blocks)
}

/// Close a block: a table row gets its cells, the ranges between its tabs.
fn finish(mut block: Block, row: Option<bool>) -> Block {
    if let Some(header) = row {
        let mut cells = Vec::new();
        let mut start = 0;
        for (index, _) in block.text.match_indices('\t') {
            cells.push(start..index);
            start = index + 1;
        }
        cells.push(start..block.text.len());
        block.kind = bibli_core::inspection::BlockKind::TableRow { cells, header };
    }
    block
}

/// Whether the element's own `style` sets `property` to one of `values`. Only
/// the inline attribute: a publisher's stylesheet is not applied.
fn styled(element: &scraper::node::Element, property: &str, values: &[&str]) -> bool {
    element.attr("style").is_some_and(|style| {
        style.split(';').any(|declaration| {
            declaration.split_once(':').is_some_and(|(name, value)| {
                name.trim().eq_ignore_ascii_case(property)
                    && values
                        .iter()
                        .any(|wanted| value.trim().eq_ignore_ascii_case(wanted))
            })
        })
    })
}
/// Resolve only local descendants of the document directory, including symlinks.
pub fn local_image(base: &std::path::Path, href: &str, cancel: &CancelToken) -> Option<Image> {
    if href.contains(':') || href.starts_with('/') {
        return None;
    }
    let href = href.split(['#', '?']).next()?;
    let href = percent_encoding::percent_decode_str(href)
        .decode_utf8()
        .ok()?;
    let base = base.canonicalize().ok()?;
    let path = base.join(href.as_ref()).canonicalize().ok()?;
    if !path.starts_with(&base) {
        return None;
    }
    let bytes =
        crate::inspection::read_bounded(&path, limits::BINARY_INPUT, "Embedded image", cancel)
            .ok()?;
    crate::inspection::image(&bytes).ok()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn semantic_text_does_not_include_active_content() {
        let blocks=html("<h1>Title</h1><p>A <strong>bold</strong> <a href='https://example.invalid'>link</a></p><script>secret()</script>",&CancelToken::new(), |_|None).unwrap();
        assert_eq!(blocks[0].heading, 1);
        assert!(blocks.iter().any(|b| b.runs.iter().any(|r| r.bold)));
        assert!(!blocks.iter().any(|b| b.text.contains("secret")));
    }

    #[test]
    fn lists_tables_rules_and_centring_are_structure_not_typed_characters() {
        use bibli_core::inspection::BlockKind;
        let source = r#"<h1 style="text-align: center">A Title</h1>
            <p><span style="font-style:italic">said</span> and <span style="font-weight: 700">done</span></p>
            <ol start="3"><li>three</li><li><p>four</p><p>still four</p><ul><li>inner</li></ul></li></ol>
            <hr/>
            <table><tr><th>Name</th><th>Qty</th></tr><tr><td><em>pear</em></td><td>12</td></tr></table>
            <center><img src="cover.png" alt="Cover"/></center>"#;
        let blocks = html(source, &CancelToken::new(), |_| None).unwrap();
        let text: Vec<_> = blocks.iter().map(|b| b.text.as_str()).collect();
        assert_eq!(
            text,
            [
                "A Title",
                "said and done",
                "three",
                "four",
                "still four",
                "inner",
                "",
                "Name\tQty",
                "pear\t12",
                "Cover"
            ]
        );
        assert!(blocks[0].centered && !blocks[1].centered && blocks[9].centered);
        assert!(blocks[1].runs[0].italic && blocks[1].runs.iter().any(|run| run.bold));
        let list: Vec<_> = blocks[2..6]
            .iter()
            .map(|b| (b.list_depth, b.marker.as_deref()))
            .collect();
        assert_eq!(
            list,
            [(1, Some("3.")), (1, Some("4.")), (1, None), (2, Some("•"))],
            "numbered from `start`; a second paragraph of an item has no marker"
        );
        assert_eq!(blocks[6].kind, BlockKind::Rule);
        assert_eq!(
            blocks[7].kind,
            BlockKind::TableRow {
                cells: vec![0..4, 5..8],
                header: true
            }
        );
        assert_eq!(
            blocks[8].kind,
            BlockKind::TableRow {
                cells: vec![0..4, 5..7],
                header: false
            }
        );
        assert!(blocks[8].runs[0].italic, "a cell keeps its emphasis");
    }

    #[test]
    fn embedded_images_stay_bounded() {
        let source = (0..limits::EMBEDDED_IMAGES + 3).fold(String::new(), |mut s, i| {
            use std::fmt::Write;
            let _ = write!(s, "<img src='{i}.png' alt='image {i}'>");
            s
        });
        let mut requested = Vec::new();
        let blocks = html(&source, &CancelToken::new(), |src| {
            requested.push(src.to_owned());
            None
        })
        .unwrap();
        assert_eq!(requested.len(), limits::EMBEDDED_IMAGES);
        assert_eq!(blocks.len(), limits::EMBEDDED_IMAGES + 3);
    }

    fn png(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
        let path = dir.join(name);
        image::RgbaImage::from_pixel(1, 1, image::Rgba([9, 9, 9, 255]))
            .save_with_format(&path, image::ImageFormat::Png)
            .unwrap();
        path
    }

    #[test]
    #[cfg(unix)]
    fn local_images_never_leave_the_document_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let docs = tmp.path().join("doc's dir");
        let outside = tmp.path().join("outside");
        std::fs::create_dir_all(docs.join("nested")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        png(&docs, "in side.png");
        png(&docs.join("nested"), "deep.png");
        let secret = png(&outside, "secret.png");
        std::os::unix::fs::symlink(&secret, docs.join("escape.png")).unwrap();
        std::os::unix::fs::symlink(&outside, docs.join("dir link")).unwrap();
        std::os::unix::fs::symlink(docs.join("nested/deep.png"), docs.join("alias.png")).unwrap();
        let cancel = CancelToken::new();
        let load = |href: &str| local_image(&docs, href, &cancel).is_some();
        assert!(load("in%20side.png"));
        assert!(load("nested/deep.png?x=1#frag"));
        assert!(load("alias.png"), "symlink inside the directory is fine");
        assert!(load("nested/../in side.png"));
        assert!(!load("escape.png"), "symlink to a file outside");
        assert!(
            !load("dir link/secret.png"),
            "symlink to a directory outside"
        );
        assert!(!load("../outside/secret.png"));
        assert!(
            !load(&secret.to_string_lossy()),
            "absolute paths are rejected"
        );
        assert!(!load("file:///etc/hostname"));
        assert!(!load("https://example.invalid/x.png"));
        assert!(!load("missing.png"));
    }
}
