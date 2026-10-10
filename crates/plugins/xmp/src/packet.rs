//! XMP (ISO 16684-1), as sidecars and packets hold it: a file's keywords,
//! rating, description and color label read into the notes model, and an
//! edit written back into the text as it is, so everything else in it
//! (darktable's history, Lightroom's develop settings, another
//! application's properties) stays byte for byte.
//!
//! Keywords are `dc:subject`, a bag; Lightroom's and digiKam's nested
//! keywords (`lr:hierarchicalSubject`, `digiKam:TagsList`, `a|b|c` and
//! `a/b/c`) are tags in categories (`a/b/c`, `metadata::TAG_SEPARATOR`),
//! each while `dc:subject` still has its last part (an application that
//! knows only `dc:subject` took it away there), and the flat keywords that
//! are one of their parts are theirs, as Lightroom writes every part. A
//! tag in a category is written so: every part in `dc:subject`, the path
//! in each nested list the file has, `lr:hierarchicalSubject` begun if it
//! has none. A rating is `xmp:Rating`, 1 to 5 whole
//! stars (−1 is rejected, 0 none), as an attribute of `rdf:Description` or
//! an element; `dc:description` is an alternative of languages, whose
//! default is read and written. `xmp:Label` is Lightroom's color label:
//! read as a tag of that name, and written back there while the tag is
//! kept (ADR-027).

use std::fmt::Write as _;
use std::ops::Range;

use bibli_core::metadata::{TAG_SEPARATOR, clean_tag, same_tag, tag_leaf};
use bibli_core::notes::{Notes, Tag};

const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
const DC: &str = "http://purl.org/dc/elements/1.1/";
const XMP: &str = "http://ns.adobe.com/xap/1.0/";
const LR: &str = "http://ns.adobe.com/lightroom/1.0/";
const DIGIKAM: &str = "http://www.digikam.org/ns/1.0/";
const PHOTOSHOP: &str = "http://ns.adobe.com/photoshop/1.0/";
const XML: &str = "http://www.w3.org/XML/1998/namespace";

/// The nested keyword lists, with the separator each writes.
const NESTED: [(&str, &str, char); 2] =
    [(LR, "hierarchicalSubject", '|'), (DIGIKAM, "TagsList", '/')];

/// The tags a document's keywords are: its nested keywords as tags in
/// categories, while `dc:subject` has their last part, then the flat ones
/// no nested keyword has as a part.
fn keywords_of(descriptions: &[Node<'_, '_>]) -> Vec<String> {
    let flat = element(descriptions, DC, "subject")
        .map(items)
        .unwrap_or_default();
    let mut out: Vec<String> = Vec::new();
    for (ns, name, separator) in NESTED {
        let Some(nested) = element(descriptions, ns, name) else {
            continue;
        };
        for path in items(nested) {
            let tag = clean_tag(&path.split(separator).collect::<Vec<_>>().join("/"));
            let kept = flat.is_empty() || flat.iter().any(|k| same_tag(k, tag_leaf(&tag)));
            if !tag.is_empty() && kept && !out.iter().any(|t| same_tag(t, &tag)) {
                out.push(tag);
            }
        }
    }
    let nested = out.clone();
    for k in flat {
        let part = nested
            .iter()
            .any(|t| t.split(TAG_SEPARATOR).any(|p| same_tag(p, &k)));
        if !part && !out.iter().any(|t| same_tag(t, &k)) {
            out.push(k);
        }
    }
    out
}

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

/// The XMP document `text`, unless it is none, or nests deeper than the
/// parser's recursion can go (a sidecar is a file like any other).
fn parse(text: &str) -> Option<roxmltree::Document<'_>> {
    bibli_platform::inspection::xml_depth(text.as_bytes()).ok()?;
    roxmltree::Document::parse(text).ok()
}

/// What XMP says of a file, from a document or a packet.
pub fn notes_of(text: &str) -> Option<Notes> {
    let doc = parse(text)?;
    let descriptions = descriptions(&doc);
    if descriptions.is_empty() {
        return None;
    }
    let names = keywords_of(&descriptions);
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
    let doc = parse(text)?;
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
    let doc = parse(text)?;
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
    let old_keywords = keywords_of(&descriptions);
    let same_keywords = old_keywords.len() == keywords.len()
        && old_keywords
            .iter()
            .all(|k| keywords.iter().any(|x| same_tag(x, k)));
    if !same_keywords {
        // Every part of every tag, as Lightroom writes them.
        let mut flat: Vec<String> = Vec::new();
        for part in keywords.iter().flat_map(|k| k.split(TAG_SEPARATOR)) {
            if !flat.iter().any(|f| same_tag(f, part)) {
                flat.push(part.to_owned());
            }
        }
        wanted.push((DC, "subject", Value::Bag(flat)));
        // The paths, in each nested list the file has; Lightroom's begun
        // for a tag in a category where it has none.
        let has_nested = NESTED
            .iter()
            .any(|(ns, name, _)| element(&descriptions, ns, name).is_some());
        let in_category = keywords.iter().any(|k| k.contains(TAG_SEPARATOR));
        for (ns, name, separator) in NESTED {
            let present = element(&descriptions, ns, name).is_some();
            if present || (ns == LR && in_category && !has_nested) {
                let paths = keywords
                    .iter()
                    .map(|k| {
                        k.split(TAG_SEPARATOR)
                            .collect::<Vec<_>>()
                            .join(&separator.to_string())
                    })
                    .collect();
                wanted.push((ns, name, Value::Bag(paths)));
            }
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
pub(crate) mod tests {
    use super::*;

    /// A sidecar as darktable writes one: attributes, a keyword bag, and a
    /// history bibli knows nothing of.
    pub(crate) const DARKTABLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
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
        assert_eq!(
            notes.tag_names(),
            ["Places/Alps/Holidays", "it's & more", "Red"],
            "the nested keyword in its categories"
        );
        assert_eq!(notes.tag("red").unwrap().color.as_deref(), Some("red"));
        assert_eq!(notes.rating, Some(6));
        assert_eq!(notes.comment, None);
    }

    #[test]
    fn an_edit_changes_what_it_says_and_keeps_everything_else() {
        let mut notes = notes_of(DARKTABLE).unwrap();
        notes.tags.retain(|t| t.name != "Places/Alps/Holidays");
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
        // The nested keyword taken away is gone from both lists; the new
        // one is at the top.
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
    fn a_tag_in_a_category_is_written_as_lightroom_and_digikam_read_it() {
        let notes = Notes {
            tags: vec![Tag::named("Places/France/Paris"), Tag::named("work")],
            ..Notes::default()
        };
        let text = new_document(&notes);
        for part in ["Places", "France", "Paris", "work"] {
            assert!(text.contains(&format!("<rdf:li>{part}</rdf:li>")), "{text}");
        }
        assert!(
            text.contains("<rdf:li>Places|France|Paris</rdf:li>"),
            "{text}"
        );
        assert_eq!(
            notes_of(&text).unwrap().tag_names(),
            ["Places/France/Paris", "work"]
        );
        // digiKam's list, where the file has it, is kept in step.
        let digikam = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:digiKam="http://www.digikam.org/ns/1.0/"><dc:subject><rdf:Bag><rdf:li>Paris</rdf:li><rdf:li>Lyon</rdf:li></rdf:Bag></dc:subject><digiKam:TagsList><rdf:Seq><rdf:li>Places/France/Paris</rdf:li><rdf:li>Places/France/Lyon</rdf:li></rdf:Seq></digiKam:TagsList></rdf:Description></rdf:RDF></x:xmpmeta>"#;
        let mut read = notes_of(digikam).unwrap();
        assert_eq!(
            read.tag_names(),
            ["Places/France/Paris", "Places/France/Lyon"]
        );
        read.tags.retain(|t| t.name.ends_with("Lyon"));
        let text = edit(digikam, &read).unwrap();
        assert!(
            text.contains("<rdf:li>Places/France/Lyon</rdf:li>"),
            "{text}"
        );
        assert!(!text.contains("Paris"), "{text}");
        assert!(
            !text.contains("hierarchicalSubject"),
            "a list of its own only"
        );
        // A nested keyword whose last part another application took out of
        // `dc:subject` is no tag of the file's.
        let stale = digikam.replace("<rdf:li>Paris</rdf:li>", "");
        assert_eq!(
            notes_of(&stale).unwrap().tag_names(),
            ["Places/France/Lyon"]
        );
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
