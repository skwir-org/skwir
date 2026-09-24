//! XHTML read as XML, into the same tree the HTML parser builds.
//!
//! An EPUB chapter is XHTML, where `<title/>` or `<script/>` closes itself.
//! The HTML parser reads either as an open tag and takes the rest of the file
//! as its text, so a chapter written that way would show nothing. XHTML is
//! read as XML first, and only what is not well-formed goes to the HTML
//! parser. HTML's named entities (`&nbsp;`), which the XHTML DTDs declare but
//! a parser that does not fetch DTDs cannot know, become the characters they
//! stand for before parsing.
//!
//! Names are given the shape html5ever gives them, so everything after
//! parsing reads one kind of tree: elements keep their namespace, `xlink:` and
//! `xml:` attributes keep theirs, and any other prefixed attribute is a plain
//! one named with its prefix (`epub:type`), as the HTML parser leaves it.
use markup5ever::{Attribute, LocalName, Namespace, Prefix, QualName, ns, tendril::StrTendril};
use scraper::{
    Html, Node,
    node::{Element, Text},
};
use std::{borrow::Cow, fmt::Write};

/// Nodes one document may have, entity expansions included.
const NODES: u32 = 1 << 20;
const XLINK: &str = "http://www.w3.org/1999/xlink";
const XML: &str = "http://www.w3.org/XML/1998/namespace";
const OPS: &str = "http://www.idpf.org/2007/ops";

/// The document, or `None` when it is not well-formed XML.
pub fn parse(source: &str) -> Option<Html> {
    let source = entities(source);
    let options = roxmltree::ParsingOptions {
        allow_dtd: true,
        nodes_limit: NODES,
        ..roxmltree::ParsingOptions::default()
    };
    let doc = roxmltree::Document::parse_with_options(&source, options).ok()?;
    let mut html = Html::new_document();
    let root = html.tree.root().id();
    // Depth first, children pushed last to first: each node is appended to
    // its parent after the whole subtree of the sibling before it.
    let mut stack = vec![(doc.root_element(), root)];
    while let Some((node, parent)) = stack.pop() {
        let value = if node.is_element() {
            Node::Element(element(node))
        } else if node.is_text() {
            Node::Text(Text {
                text: node.text().unwrap_or_default().into(),
            })
        } else {
            continue;
        };
        let mut parent = html.tree.get_mut(parent)?;
        let id = parent.append(value).id();
        if node.is_element() {
            let children: Vec<_> = node.children().collect();
            stack.extend(children.into_iter().rev().map(|child| (child, id)));
        }
    }
    Some(html)
}

fn element(node: roxmltree::Node<'_, '_>) -> Element {
    let tag = node.tag_name();
    let name = QualName::new(
        None,
        tag.namespace().map_or(ns!(), Namespace::from),
        LocalName::from(tag.name()),
    );
    let attributes = node
        .attributes()
        .map(|attribute| {
            let local = attribute.name();
            let name = match attribute.namespace() {
                None => QualName::new(None, ns!(), LocalName::from(local)),
                Some(XLINK) => QualName::new(Some(Prefix::from("xlink")), ns!(xlink), local.into()),
                Some(XML) => QualName::new(Some(Prefix::from("xml")), ns!(xml), local.into()),
                // EPUB's vocabulary is read as `epub:type` whatever prefix a
                // book declared for it.
                Some(OPS) => QualName::new(None, ns!(), LocalName::from(format!("epub:{local}"))),
                Some(uri) => {
                    let prefix = node.lookup_prefix(uri).unwrap_or_default();
                    QualName::new(None, ns!(), LocalName::from(format!("{prefix}:{local}")))
                }
            };
            Attribute {
                name,
                value: StrTendril::from_slice(attribute.value()),
            }
        })
        .collect();
    Element::new(name, attributes)
}

/// Replace HTML's named character references, other than XML's own five, by
/// numeric ones. A name HTML does not know is left for the XML parser to
/// refuse, and the document then goes to the HTML parser.
fn entities(source: &str) -> Cow<'_, str> {
    if !source.contains('&') {
        return Cow::Borrowed(source);
    }
    let mut out = String::with_capacity(source.len());
    let mut rest = source;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let length = rest[1..]
            .bytes()
            .take_while(u8::is_ascii_alphanumeric)
            .count();
        let name = &rest[1..=length];
        if length > 0
            && rest.as_bytes().get(length + 1) == Some(&b';')
            && !matches!(name, "lt" | "gt" | "amp" | "quot" | "apos")
            && let Some(&(first, second)) =
                markup5ever::data::NAMED_ENTITIES.get(&rest[1..length + 2])
        {
            let _ = write!(out, "&#{first};");
            if second != 0 {
                let _ = write!(out, "&#{second};");
            }
            rest = &rest[length + 2..];
            continue;
        }
        out.push('&');
        rest = &rest[1..];
    }
    out.push_str(rest);
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_closing_tags_close_and_named_entities_read() {
        let doc = parse(
            r#"<?xml version="1.0" encoding="utf-8"?>
            <!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
            <html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"
                  xmlns:xlink="http://www.w3.org/1999/xlink">
            <head><title/><script src="a.js"/></head>
            <body><p epub:type="bodymatter">One&nbsp;&amp;&#160;two &hellip;</p>
            <svg xmlns="http://www.w3.org/2000/svg"><image xlink:href="c.jpg"/></svg></body></html>"#,
        )
        .unwrap();
        let p = doc
            .select(&scraper::Selector::parse("p").unwrap())
            .next()
            .unwrap();
        assert_eq!(p.text().collect::<String>(), "One\u{a0}&\u{a0}two \u{2026}");
        assert_eq!(p.value().attr("epub:type"), Some("bodymatter"));
        let image = doc
            .tree
            .nodes()
            .filter_map(|n| n.value().as_element())
            .find(|e| e.name() == "image")
            .unwrap();
        assert_eq!(image.name.ns, ns!(svg));
        assert_eq!(
            image.attrs().find(|(name, _)| *name == "href"),
            Some(("href", "c.jpg"))
        );
    }

    #[test]
    fn what_is_not_well_formed_is_left_to_the_html_parser() {
        assert!(parse("<html><body><p>open<br></p></body></html>").is_none());
        assert!(parse("<html><body>&unknownentity;</body></html>").is_none());
        assert_eq!(entities("a & b &amp; &nbsp"), "a & b &amp; &nbsp");
    }
}
