//! XMP (ISO 16684-1), as sidecars and packets hold it: a file's keywords,
//! rating, description and color label read into the notes model, and an
//! edit written back into the text as it is, so everything else in it
//! (darktable's history, Lightroom's develop settings, another
//! application's properties) stays byte for byte.
//!
//! Keywords are `dc:subject`, a bag; Lightroom's and digiKam's nested
//! keywords (`lr:hierarchicalSubject`, `digiKam:TagsList`, `a|b|c` and
//! `a/b/c`) are read for their last part when `dc:subject` says nothing,
//! and kept in step when present. A rating is `xmp:Rating`, 1 to 5 whole
//! stars (−1 is rejected, 0 none), as an attribute of `rdf:Description` or
//! an element; `dc:description` is an alternative of languages, whose
//! default is read and written. `xmp:Label` is Lightroom's color label:
//! read as a tag of that name, and written back there while the tag is
//! kept (ADR-027).

use std::fmt::Write as _;
use std::ops::Range;

use bibli_core::metadata::same_tag;
use bibli_core::notes::{Notes, Tag};

const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
const DC: &str = "http://purl.org/dc/elements/1.1/";
const XMP: &str = "http://ns.adobe.com/xap/1.0/";
const LR: &str = "http://ns.adobe.com/lightroom/1.0/";
const DIGIKAM: &str = "http://www.digikam.org/ns/1.0/";
const PHOTOSHOP: &str = "http://ns.adobe.com/photoshop/1.0/";
const XML: &str = "http://www.w3.org/XML/1998/namespace";

/// Lightroom's color labels, by the palette color they wear.
const LABELS: [(&str, &str); 5] = [
    ("Red", "red"),
    ("Yellow", "yellow"),
    ("Green", "green"),
    ("Blue", "blue"),
    ("Purple", "purple"),
];

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

/// A rating as XMP stars, from half stars: rounded, half up.
pub fn stars(rating: u64) -> u64 {
    rating.div_ceil(2)
}

type Node<'a, 'input> = roxmltree::Node<'a, 'input>;

/// The `rdf:Description` elements of a document.
fn descriptions<'a, 'input>(doc: &'a roxmltree::Document<'input>) -> Vec<Node<'a, 'input>> {
    doc.descendants()
        .filter(|n| n.has_tag_name((RDF, "Description")))
        .collect()
}

/// The items of an RDF container property (`dc:subject/rdf:Bag/rdf:li`).
fn items(property: Node<'_, '_>) -> Vec<String> {
    property
        .descendants()
        .filter(|n| n.has_tag_name((RDF, "li")))
        .filter_map(|n| n.text())
        .map(|t| t.trim().to_owned())
        .filter(|t| !t.is_empty())
        .collect()
}

/// A property of the descriptions: its element, if one.
fn element<'a, 'input>(
    descriptions: &[Node<'a, 'input>],
    ns: &str,
    name: &str,
) -> Option<Node<'a, 'input>> {
    descriptions
        .iter()
        .flat_map(Node::children)
        .find(|n| n.has_tag_name((ns, name)))
}

/// A simple property: an attribute of a description, or an element's text.
fn simple(descriptions: &[Node<'_, '_>], ns: &str, name: &str) -> Option<String> {
    descriptions
        .iter()
        .find_map(|d| d.attribute((ns, name)).map(str::to_owned))
        .or_else(|| element(descriptions, ns, name)?.text().map(str::to_owned))
        .map(|t| t.trim().to_owned())
        .filter(|t| !t.is_empty())
}

/// What XMP says of a file, from a document or a packet.
pub fn notes_of(text: &str) -> Option<Notes> {
    let doc = roxmltree::Document::parse(text).ok()?;
    let descriptions = descriptions(&doc);
    if descriptions.is_empty() {
        return None;
    }
    let mut names = element(&descriptions, DC, "subject")
        .map(items)
        .unwrap_or_default();
    if names.is_empty() {
        for (ns, name, separator) in [(LR, "hierarchicalSubject", '|'), (DIGIKAM, "TagsList", '/')]
        {
            if let Some(nested) = element(&descriptions, ns, name) {
                names.extend(items(nested).into_iter().filter_map(|path| {
                    path.rsplit(separator)
                        .next()
                        .map(|leaf| leaf.trim().to_owned())
                }));
            }
        }
    }
    let mut notes = Notes::default();
    for name in names {
        if notes.tag(&name).is_none() {
            notes.tags.push(Tag::named(name));
        }
    }
    if let Some(label) = simple(&descriptions, XMP, "Label") {
        let color = LABELS
            .iter()
            .find(|(l, _)| l.eq_ignore_ascii_case(&label))
            .map(|(_, c)| (*c).to_owned());
        match notes.tags.iter_mut().find(|t| same_tag(&t.name, &label)) {
            Some(tag) => tag.color = tag.color.take().or(color),
            None => notes.tags.push(Tag { name: label, color }),
        }
    }
    notes.rating = simple(&descriptions, XMP, "Rating")
        .and_then(|r| r.parse::<f64>().ok())
        .filter(|r| *r > 0.0)
        .map(|r| {
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a rating of 1 to 5 stars, rounded, as half stars"
            )]
            let half = (r.min(5.0) * 2.0).round() as u64;
            half.max(1)
        });
    notes.comment = element(&descriptions, DC, "description").and_then(|d| {
        let lis: Vec<Node<'_, '_>> = d
            .descendants()
            .filter(|n| n.has_tag_name((RDF, "li")))
            .collect();
        lis.iter()
            .find(|n| n.attribute((XML, "lang")) == Some("x-default"))
            .or(lis.first())
            .and_then(roxmltree::Node::text)
            .map(|t| t.trim().to_owned())
            .filter(|t| !t.is_empty())
    });
    Some(notes)
}

/// The extension a sidecar says it is for (`photoshop:SidecarForExtension`).
pub fn sidecar_for(text: &str) -> Option<String> {
    let doc = roxmltree::Document::parse(text).ok()?;
    simple(&descriptions(&doc), PHOTOSHOP, "SidecarForExtension")
}

/// One change to the text: its range replaced.
struct Splice {
    at: Range<usize>,
    with: String,
}

/// The prefix `ns` is declared with in `node`'s scope.
fn prefix(node: Node<'_, '_>, ns: &str) -> Option<String> {
    node.lookup_prefix(ns).map(str::to_owned)
}

/// A property's new element, `None` to have none.
fn property_xml(prefix: &str, name: &str, rdf: &str, value: &Value) -> Option<String> {
    match value {
        Value::Bag(items) if items.is_empty() => None,
        Value::Bag(items) => {
            let mut li = String::new();
            for i in items {
                let _ = write!(li, "<{rdf}:li>{}</{rdf}:li>", escape(i));
            }
            Some(format!(
                "<{prefix}:{name}><{rdf}:Bag>{li}</{rdf}:Bag></{prefix}:{name}>"
            ))
        }
        Value::Default(None) | Value::Simple(None) => None,
        Value::Default(Some(text)) => Some(format!(
            "<{prefix}:{name}><{rdf}:Alt><{rdf}:li xml:lang=\"x-default\">{}</{rdf}:li></{rdf}:Alt></{prefix}:{name}>",
            escape(text)
        )),
        Value::Simple(Some(text)) => Some(format!(
            "<{prefix}:{name}>{}</{prefix}:{name}>",
            escape(text)
        )),
    }
}

/// What a property is set to.
enum Value {
    Bag(Vec<String>),
    /// A language alternative's default.
    Default(Option<String>),
    Simple(Option<String>),
}

/// The text of the document with `notes` written into it: keywords,
/// rating, description and label changed where they differ, everything
/// else as it was. `None` when it is not XMP bibli can edit.
pub fn edit(text: &str, notes: &Notes) -> Option<String> {
    let doc = roxmltree::Document::parse(text).ok()?;
    let descriptions = descriptions(&doc);
    let first = *descriptions.first()?;
    let before = notes_of(text).unwrap_or_default();
    let rdf = prefix(first, RDF)?;

    // The label stays a label while its tag is kept; the other tags are
    // keywords.
    let label = simple(&descriptions, XMP, "Label");
    let keep_label = label.as_ref().filter(|l| notes.tag(l).is_some());
    let keywords: Vec<String> = notes
        .tags
        .iter()
        .map(|t| t.name.clone())
        .filter(|t| keep_label.is_none_or(|l| !same_tag(l, t)))
        .collect();
    let mut wanted: Vec<(&str, &str, Value)> = Vec::new();
    let old_keywords = element(&descriptions, DC, "subject")
        .map(items)
        .unwrap_or_default();
    let same_keywords = old_keywords.len() == keywords.len()
        && old_keywords
            .iter()
            .all(|k| keywords.iter().any(|x| same_tag(x, k)));
    if !same_keywords {
        wanted.push((DC, "subject", Value::Bag(keywords.clone())));
    }
    // Nested keywords, where present: those whose last part is kept, and
    // the new tags at the top.
    for (ns, name, separator) in [(LR, "hierarchicalSubject", '|'), (DIGIKAM, "TagsList", '/')] {
        let Some(nested) = element(&descriptions, ns, name) else {
            continue;
        };
        let paths = items(nested);
        let leaf = |p: &str| p.rsplit(separator).next().unwrap_or(p).trim().to_owned();
        let mut kept: Vec<String> = paths
            .iter()
            .filter(|p| keywords.iter().any(|k| same_tag(k, &leaf(p))))
            .cloned()
            .collect();
        for k in &keywords {
            if !kept.iter().any(|p| same_tag(&leaf(p), k)) {
                kept.push(k.clone());
            }
        }
        if kept != paths {
            wanted.push((ns, name, Value::Bag(kept)));
        }
    }
    // In whole stars: a half star that rounds to what is there is no
    // change, and a rejection (−1) stays while no rating is given.
    if notes.rating.map(stars) != before.rating.map(stars) {
        let to = notes.rating.map(|r| stars(r).to_string());
        wanted.push((XMP, "Rating", Value::Simple(to)));
    }
    if notes.comment != before.comment {
        wanted.push((DC, "description", Value::Default(notes.comment.clone())));
    }
    if label.is_some() && keep_label.is_none() {
        wanted.push((XMP, "Label", Value::Simple(None)));
    }
    if wanted.is_empty() {
        return Some(text.to_owned());
    }

    let mut splices: Vec<Splice> = Vec::new();
    let mut inserted = String::new();
    let mut declare: Vec<(String, &str)> = Vec::new();
    let known_prefix = |ns: &str| -> String {
        match (ns, prefix(first, ns)) {
            (_, Some(p)) => p,
            (DC, None) => "dc".into(),
            (XMP, None) => "xmp".into(),
            (LR, None) => "lr".into(),
            (_, None) => "digiKam".into(),
        }
    };
    for (ns, name, value) in wanted {
        // As an attribute of a description: changed in place or removed.
        let attribute = descriptions.iter().find_map(|d| {
            d.attributes()
                .find(|a| a.namespace() == Some(ns) && a.name() == name)
        });
        if let Some(a) = attribute {
            if let Value::Simple(Some(text)) = &value {
                splices.push(Splice {
                    at: a.range_value(),
                    with: escape(text),
                });
            } else {
                // The attribute and the space before it.
                let range = a.range();
                let start = text[..range.start].trim_end().len();
                splices.push(Splice {
                    at: start..range.end,
                    with: String::new(),
                });
            }
            continue;
        }
        if let Some(e) = element(&descriptions, ns, name) {
            let prefix = prefix(e, ns).unwrap_or_else(|| known_prefix(ns));
            splices.push(Splice {
                at: e.range(),
                with: property_xml(&prefix, name, &rdf, &value).unwrap_or_default(),
            });
        } else {
            let p = known_prefix(ns);
            if let Some(xml) = property_xml(&p, name, &rdf, &value) {
                if prefix(first, ns).is_none() && !declare.iter().any(|(q, _)| *q == p) {
                    declare.push((p, ns));
                }
                inserted.push_str(&xml);
            }
        }
    }
    let range = first.range();
    let tag = &text[range.clone()];
    // The start tag ends at the first `>` after its last attribute: a value
    // may hold one.
    let after = first
        .attributes()
        .map(|a| a.range().end)
        .max()
        .unwrap_or(range.start);
    let start_end = after + text[after..].find('>')?;
    let self_closing = text[..start_end].ends_with('/');
    // The element's name as written, `rdf:Description`.
    let name_len = tag[1..].find(|c: char| c.is_whitespace() || c == '>' || c == '/')?;
    let qname = &tag[1..=name_len];
    if !declare.is_empty() {
        let name_end = range.start + 1 + name_len;
        let mut with = String::new();
        for (p, ns) in &declare {
            let _ = write!(with, " xmlns:{p}=\"{ns}\"");
        }
        splices.push(Splice {
            at: name_end..name_end,
            with,
        });
    }
    if !inserted.is_empty() {
        if self_closing {
            let end = start_end + 1;
            splices.push(Splice {
                at: start_end - 1..end,
                with: format!(">{inserted}</{qname}>"),
            });
        } else {
            let close = range.start + tag.rfind("</")?;
            splices.push(Splice {
                at: close..close,
                with: inserted,
            });
        }
    }
    splices.sort_by_key(|s| std::cmp::Reverse(s.at.start));
    let mut out = text.to_owned();
    for s in splices {
        out.replace_range(s.at, &s.with);
    }
    // What was written must read back as XMP.
    roxmltree::Document::parse(&out).ok()?;
    Some(out)
}

/// A sidecar holding `notes`, as the XMP specification asks one written:
/// a complete document with its XML declaration.
pub fn new_document(notes: &Notes) -> String {
    let empty = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
        <x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n \
        <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n  \
        <rdf:Description rdf:about=\"\"\n    \
        xmlns:dc=\"http://purl.org/dc/elements/1.1/\"\n    \
        xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\">\n  \
        </rdf:Description>\n \
        </rdf:RDF>\n\
        </x:xmpmeta>\n";
    edit(empty, notes).unwrap_or_else(|| empty.to_owned())
}

/// The first XMP packet inside `bytes` (a picture's, a PDF's), as its
/// specification lets any file be scanned for one.
pub fn packet_in(bytes: &[u8]) -> Option<&str> {
    let start = find(bytes, b"<x:xmpmeta").or_else(|| find(bytes, b"<rdf:RDF"))?;
    let end_tag: &[u8] = if bytes[start..].starts_with(b"<x:xmpmeta") {
        b"</x:xmpmeta>"
    } else {
        b"</rdf:RDF>"
    };
    let end = start + find(&bytes[start..], end_tag)? + end_tag.len();
    std::str::from_utf8(&bytes[start..end]).ok()
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sidecar as darktable writes one: attributes, a keyword bag, and a
    /// history bibli knows nothing of.
    const DARKTABLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="XMP Core 4.4.0-Exiv2">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:xmp="http://ns.adobe.com/xap/1.0/"
    xmlns:dc="http://purl.org/dc/elements/1.1/"
    xmlns:darktable="http://darktable.sf.net/"
    xmlns:lr="http://ns.adobe.com/lightroom/1.0/"
    xmp:Rating="3"
    xmp:Label="Red"
    darktable:history_end="2">
   <dc:subject>
    <rdf:Bag>
     <rdf:li>Holidays</rdf:li>
     <rdf:li>it's &amp; more</rdf:li>
    </rdf:Bag>
   </dc:subject>
   <lr:hierarchicalSubject>
    <rdf:Bag>
     <rdf:li>Places|Alps|Holidays</rdf:li>
     <rdf:li>it's &amp; more</rdf:li>
    </rdf:Bag>
   </lr:hierarchicalSubject>
   <darktable:history>
    <rdf:Seq>
     <rdf:li darktable:operation="exposure" darktable:enabled="1"/>
    </rdf:Seq>
   </darktable:history>
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>
"#;

    #[test]
    fn a_sidecar_reads_as_notes_its_label_a_tag_of_its_color() {
        let notes = notes_of(DARKTABLE).unwrap();
        assert_eq!(notes.tag_names(), ["Holidays", "it's & more", "Red"]);
        assert_eq!(notes.tag("red").unwrap().color.as_deref(), Some("red"));
        assert_eq!(notes.rating, Some(6));
        assert_eq!(notes.comment, None);
    }

    #[test]
    fn an_edit_changes_what_it_says_and_keeps_everything_else() {
        let mut notes = notes_of(DARKTABLE).unwrap();
        notes.tags.retain(|t| t.name != "Holidays");
        notes.tags.push(Tag::named("Été <new>"));
        notes.rating = Some(7);
        notes.comment = Some("from the \"trip\"".into());
        let text = edit(DARKTABLE, &notes).unwrap();
        assert!(text.contains(r#"darktable:operation="exposure""#), "{text}");
        assert!(text.contains(r#"darktable:history_end="2""#));
        let read = notes_of(&text).unwrap();
        assert_eq!(read.tag_names(), ["it's & more", "Été <new>", "Red"]);
        assert_eq!(read.rating, Some(8), "3½ stars written as 4");
        assert_eq!(read.comment.as_deref(), Some("from the \"trip\""));
        // The nested keyword whose last part went is gone; the new one is
        // at the top.
        assert!(!text.contains("Places|Alps|Holidays"));
        assert!(text.contains("<rdf:li>Été &lt;new&gt;</rdf:li>"));
        // The label's tag taken away takes the label.
        notes.tags.retain(|t| t.name != "Red");
        let text = edit(&text, &notes).unwrap();
        assert!(!text.contains("xmp:Label"));
        // Nothing to change: the text as it was.
        assert_eq!(edit(&text, &notes_of(&text).unwrap()).unwrap(), text);
    }

    #[test]
    fn a_new_sidecar_is_a_whole_document_of_the_notes() {
        let notes = Notes {
            tags: vec![Tag::named("a"), Tag::named("b, c")],
            rating: Some(10),
            comment: Some("note".into()),
            ..Notes::default()
        };
        let text = new_document(&notes);
        assert!(text.starts_with("<?xml version=\"1.0\""));
        let read = notes_of(&text).unwrap();
        assert_eq!(read.tag_names(), ["a", "b, c"]);
        assert_eq!(
            (read.rating, read.comment.as_deref()),
            (Some(10), Some("note"))
        );
        // A description written as an empty element gains children.
        let bare = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about=""/></rdf:RDF></x:xmpmeta>"#;
        let text = edit(bare, &notes).unwrap();
        assert_eq!(notes_of(&text).unwrap().tag_names(), ["a", "b, c"]);
        assert_eq!(edit("not xml", &notes), None);
    }

    #[test]
    fn a_packet_is_found_inside_a_file() {
        let mut jpeg = b"\xff\xd8\xff\xe1\x00\x10http://ns.adobe.com/xap/1.0/\0".to_vec();
        jpeg.extend(DARKTABLE.split_once('\n').unwrap().1.as_bytes());
        jpeg.extend(b"\xff\xd9");
        let packet = packet_in(&jpeg).unwrap();
        assert_eq!(notes_of(packet).unwrap().rating, Some(6));
        assert!(packet_in(b"no packet here").is_none());
    }
}
