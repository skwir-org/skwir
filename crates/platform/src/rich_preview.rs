//! Native reading-mode HTML: semantic blocks, inline styles and local images.
//! Scripts, styles, frames and remote resources are never executed or fetched.
use crate::inspection::check;
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
    for node in doc.tree.root().descendants() {
        check(cancel)?;
        anyhow::ensure!(blocks.len() < 2000, "Document exceeds 2000 preview blocks");
        let ancestors: Vec<_> = node
            .ancestors()
            .filter_map(|n| n.value().as_element().map(|e| (n.id(), e)))
            .take(129)
            .collect();
        if ancestors.len() > 128
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
            blocks.push(std::mem::take(&mut current));
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
            if e.name() == "li" && current.text.is_empty() {
                current.text.push_str("• ");
            }
        }
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
                    bold: ancestors
                        .iter()
                        .any(|(_, e)| matches!(e.name(), "b" | "strong" | "th")),
                    italic: ancestors
                        .iter()
                        .any(|(_, e)| matches!(e.name(), "i" | "em")),
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
            Node::Element(e) if matches!(e.name(), "td" | "th") && !current.text.is_empty() => {
                current.text.push_str("    │    ");
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
                    blocks.push(std::mem::take(&mut current));
                }
                let bitmap = if images < 8 {
                    e.attr("src").and_then(&mut image)
                } else {
                    None
                };
                images += 1;
                blocks.push(Block {
                    text: e.attr("alt").unwrap_or_default().into(),
                    image: bitmap,
                    ..Block::default()
                });
            }
            _ => {}
        }
    }
    if !current.text.is_empty() {
        blocks.push(current);
    }
    Ok(blocks)
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
        crate::inspection::read(&path, 0, crate::inspection::IMAGE_LIMIT + 1, cancel).ok()?;
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
}
