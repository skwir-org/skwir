//! `FictionBook` 2 (ADR-106): one XML document, plain or alone in a ZIP; its
//! bodies of nested sections, a body of notes, its pictures in `binary`
//! elements as base64.
//!
//! The document is read whole, at most `limits::BINARY_INPUT` (a book is a
//! container, not a text read whole), parsed once and kept while the
//! preview shows it (`book::Kept`). Each chapter is translated to XHTML once
//! for reading mode: FB2's elements become the HTML that means the same,
//! what has no HTML element of its own a class the book's stylesheet
//! (`STYLESHEET`) sets, so the reader's cascade decides how it reads.
use std::{borrow::Cow, collections::HashMap, fmt::Write, ops::Range, sync::Arc};

use base64::Engine as _;
use roxmltree::{Node, NodeId};
use skwir_core::{
    CancelToken,
    inspection::{Picture, Row},
    preview::FileSource,
};
use skwir_platform::{
    inspection::{self, limits},
    rich_preview::{self, Markup},
};

use crate::book::{Book, ChapterRef, Sheets, Sizes, member_key};

/// The stylesheet every chapter links: what FB2 sets apart that HTML has no
/// element for.
const STYLESHEET: &str = "fb2.css";
const CSS: &str = "\
.subtitle, .title { text-align: center; font-weight: bold }
.text-author { text-align: right; font-style: italic }
.epigraph { margin-left: 30%; font-style: italic }
.stanza { margin-left: 2em }
.date { text-align: right; font-style: italic }
.image { text-align: center }
";

/// The `XLink` namespace an FB2 link's `href` is in.
const XLINK: &str = "http://www.w3.org/1999/xlink";

/// A chapter's name: what links and the contents call it.
fn href(index: usize) -> String {
    format!("chapter-{}", index + 1)
}

/// A book in FB2, read and translated.
pub(crate) struct Fb2 {
    /// The document, decoded: binaries and chapters' sources are ranges of
    /// it.
    text: String,
    /// How it was decoded, when not as UTF-8.
    decoded: Option<String>,
    refs: Vec<ChapterRef>,
    chapters: Vec<Chapter>,
    /// The base64 text of each picture, by its id.
    binaries: HashMap<String, Range<usize>>,
    contents: Vec<Row>,
    details: Vec<(&'static str, Option<String>)>,
    /// The size each picture is shown at, read once.
    sizes: Sizes,
}

struct Chapter {
    xhtml: String,
    /// Whether it holds more than reading mode reads, which is left out.
    cut: bool,
    /// Where it is in the document, for its source.
    source: Range<usize>,
}

/// The document in `file`, or the first FB2 in it when it is `zipped`
/// (in the archive's order), decoded: with how, when not as UTF-8.
fn read<S: FileSource + ?Sized>(
    file: &S,
    zipped: bool,
    cancel: &CancelToken,
) -> anyhow::Result<(String, Option<String>)> {
    let bytes = if zipped {
        let mut package =
            skwir_platform::package::Package::open(file, "This zipped FB2", false, cancel)?;
        let names: Vec<String> = package.names().map(str::to_owned).collect();
        let name = names
            .iter()
            .find(|name| name.to_lowercase().ends_with(".fb2"))
            .or_else(|| names.first().filter(|_| names.len() == 1))
            .ok_or_else(|| anyhow::anyhow!("This zipped FB2 holds no FB2"))?
            .clone();
        Cow::Owned(package.read(&name, limits::BINARY_INPUT, "FB2 document", cancel)?)
    } else {
        file.read_whole(limits::BINARY_INPUT as u64, cancel)?
            .ok_or_else(|| anyhow::anyhow!("This FB2 is too large to read"))?
    };
    inspection::check(cancel)?;
    Ok(decode(&bytes))
}

/// The cover of the FB2 in `file` (the one in it when it is `zipped`), if
/// it names one it holds: its description and that binary read, its
/// chapters left as they are, as a thumbnail needs.
pub(crate) fn cover<S: FileSource + ?Sized>(
    file: &S,
    zipped: bool,
    cancel: &CancelToken,
) -> anyhow::Result<Option<Vec<u8>>> {
    let (text, _) = read(file, zipped, cancel)?;
    let doc = inspection::xml_document(&text)?;
    let root = doc.root_element();
    anyhow::ensure!(is(root, "FictionBook"), "This is not an FB2 document");
    let Some(id) = child(root, "description").and_then(cover_id) else {
        return Ok(None);
    };
    Ok(root
        .children()
        .find_map(|node| binary_text(node).filter(|(of, _)| *of == id))
        .and_then(|(_, data)| from_base64(&text[data], limits::BINARY_INPUT).ok()))
}

/// A `binary` element's id and where its base64 text is.
fn binary_text<'a>(node: Node<'a, '_>) -> Option<(&'a str, Range<usize>)> {
    let data = node
        .first_child()
        .filter(|data| is(node, "binary") && data.is_text())?;
    Some((node.attribute("id")?, data.range()))
}

/// At most `limit` bytes of what `base64` encodes, white space aside: what
/// is past them is not decoded.
fn from_base64(base64: &str, limit: usize) -> anyhow::Result<Vec<u8>> {
    // Four characters of base64 for each three bytes.
    let wanted = limit.div_ceil(3).saturating_mul(4);
    let base64: Vec<u8> = base64
        .bytes()
        .filter(|b| !b.is_ascii_whitespace())
        .take(wanted)
        .collect();
    let engine = base64::engine::GeneralPurpose::new(
        &base64::alphabet::STANDARD,
        base64::engine::GeneralPurposeConfig::new()
            .with_decode_padding_mode(base64::engine::DecodePaddingMode::Indifferent)
            .with_decode_allow_trailing_bits(true),
    );
    let mut bytes = engine
        .decode(&base64)
        .map_err(|_| anyhow::anyhow!("damaged"))?;
    bytes.truncate(limit);
    Ok(bytes)
}

impl Fb2 {
    /// The FB2 in `file`, or the one FB2 in it when it is `zipped`.
    pub(crate) fn open<S: FileSource + ?Sized>(
        file: &S,
        zipped: bool,
        cancel: &CancelToken,
    ) -> anyhow::Result<Self> {
        let (text, decoded) = read(file, zipped, cancel)?;
        Self::parse(text, decoded, cancel)
    }

    fn parse(text: String, decoded: Option<String>, cancel: &CancelToken) -> anyhow::Result<Self> {
        let doc = inspection::xml_document(&text)?;
        let root = doc.root_element();
        anyhow::ensure!(is(root, "FictionBook"), "This is not an FB2 document");
        let mut groups = Vec::new();
        let mut binaries = HashMap::new();
        let mut first = true;
        for child in root.children().filter(Node::is_element) {
            if is(child, "body") {
                // The first body is the book; others are its notes, or
                // comments, each a chapter.
                if first && child.attribute("name").is_none_or(|name| name != "notes") {
                    groups.extend(chapters_of(child));
                } else {
                    groups.push(Group::Notes(child));
                }
                first = false;
            } else if let Some((id, data)) = binary_text(child) {
                // The first of an id is the one, as the cover finds it.
                binaries.entry(id.to_owned()).or_insert(data);
            }
        }
        anyhow::ensure!(!groups.is_empty(), "This FB2 has no body");
        anyhow::ensure!(
            groups.len() <= limits::BLOCKS,
            "This FB2 has more than {} chapters",
            limits::BLOCKS
        );
        let description = child(root, "description");
        let details = description.map(describe).unwrap_or_default();
        // Every id by the chapter that holds it, then an anchor for each
        // section that has none, named apart from every id: a link leads to
        // its chapter wherever it points, before or after it.
        let mut ids = HashMap::new();
        for (index, group) in groups.iter().enumerate() {
            for node in group.written() {
                if let Some(id) = node.attribute("id") {
                    ids.entry(id.to_owned()).or_insert(index);
                }
            }
        }
        let mut anchors = HashMap::new();
        let mut number = 0;
        for (index, group) in groups.iter().enumerate() {
            for node in group.written() {
                if is(node, "section") && node.attribute("id").is_none() {
                    let anchor = loop {
                        number += 1;
                        let anchor = format!("section-{number}");
                        if !ids.contains_key(&anchor) {
                            break anchor;
                        }
                    };
                    ids.insert(anchor.clone(), index);
                    anchors.insert(node.id(), anchor);
                }
            }
        }
        let mut contents = Vec::new();
        let mut chapters = Vec::with_capacity(groups.len());
        for (index, group) in groups.iter().enumerate() {
            inspection::check(cancel)?;
            let mut writer = Writer {
                ids: &ids,
                anchors: &anchors,
                chapter: index,
                out: String::new(),
                rows: &mut contents,
                nest: 0,
                // The frame's: `html`, `head`, its `link`, `body`.
                nodes: 4,
                cut: false,
            };
            writer.out.push_str(concat!(
                r#"<?xml version="1.0" encoding="utf-8"?>"#,
                r#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">"#,
                r#"<head><link rel="stylesheet" href="fb2.css"/></head><body>"#
            ));
            match group {
                Group::Main(items) => {
                    for item in items {
                        if item.head {
                            writer.head(item.node, item.level, item.depth);
                        } else {
                            writer.node(item.node, item.level, item.depth);
                        }
                    }
                }
                Group::Notes(body) => writer.notes(*body),
            }
            writer.out.push_str("</body></html>");
            chapters.push(Chapter {
                cut: writer.cut,
                xhtml: writer.out,
                source: group.source(),
            });
        }
        contents.truncate(limits::BLOCKS);
        let refs = (0..chapters.len())
            .map(|index| ChapterRef {
                href: href(index),
                markup: Markup::Xhtml,
            })
            .collect();
        drop(doc);
        Ok(Self {
            text,
            decoded,
            refs,
            chapters,
            binaries,
            contents,
            details,
            sizes: Sizes::default(),
        })
    }

    /// The picture `id` names, decoded: at most `limit` bytes of it.
    pub(crate) fn binary(&self, id: &str, limit: usize) -> anyhow::Result<Vec<u8>> {
        let range = self
            .binaries
            .get(id.strip_prefix('#').unwrap_or(id))
            .ok_or_else(|| anyhow::anyhow!("Missing FB2 picture {id}"))?;
        from_base64(&self.text[range.clone()], limit)
            .map_err(|_| anyhow::anyhow!("This FB2's picture {id} is damaged"))
    }

    fn chapter(&self, href: &str) -> anyhow::Result<&Chapter> {
        member_key(href)
            .and_then(|key| key.strip_prefix("chapter-")?.parse::<usize>().ok())
            .and_then(|number| self.chapters.get(number.checked_sub(1)?))
            .ok_or_else(|| anyhow::anyhow!("Missing FB2 chapter {href}"))
    }
}

/// The text of `bytes`: a byte order mark decides, else the encoding the
/// XML declaration names (Russian books are often in Windows-1251), else
/// UTF-8, else Windows-1252 (`inspection::decode`). With how it was
/// decoded, when not as UTF-8.
fn decode(bytes: &[u8]) -> (String, Option<String>) {
    let declared = (encoding_rs::Encoding::for_bom(bytes).is_none())
        .then(|| declared_encoding(bytes))
        .flatten()
        .filter(|encoding| *encoding != encoding_rs::UTF_8);
    if let Some(encoding) = declared {
        let (text, _) = encoding.decode_without_bom_handling(bytes);
        return (
            text.into_owned(),
            Some(format!("Read as {}", encoding.name())),
        );
    }
    let (text, encoding) = inspection::decode(bytes);
    (text.into_owned(), encoding.notice())
}

/// The encoding an XML declaration at the start of `bytes` names.
fn declared_encoding(bytes: &[u8]) -> Option<&'static encoding_rs::Encoding> {
    let head = &bytes[..bytes.len().min(256)];
    let declaration = head.strip_prefix(b"<?xml")?;
    let declaration = &declaration[..declaration.windows(2).position(|w| w == b"?>")?];
    let at = declaration.windows(8).position(|w| w == b"encoding")?;
    let rest = declaration[at + 8..]
        .trim_ascii_start()
        .strip_prefix(b"=")?
        .trim_ascii_start();
    let quote = *rest.first().filter(|q| matches!(q, b'"' | b'\''))?;
    let label = &rest[1..=rest[1..].iter().position(|&b| b == quote)?];
    encoding_rs::Encoding::for_label(label)
}

/// Whether `node` is the FB2 element `name`, in FB2's namespace or none.
fn is(node: Node<'_, '_>, name: &str) -> bool {
    node.is_element() && node.tag_name().name() == name
}

fn child<'a, 'input>(node: Node<'a, 'input>, name: &str) -> Option<Node<'a, 'input>> {
    node.children().find(|child| is(*child, name))
}

/// What a chapter is made of.
enum Group<'a, 'input> {
    /// Parts of the book's body, in order.
    Main(Vec<Item<'a, 'input>>),
    /// A body of notes.
    Notes(Node<'a, 'input>),
}

/// A part of a chapter: a node written whole, or the head of a part over
/// every chapter (its title and what is not one of its sections, which are
/// chapters of their own). Its titles are headings of `level`, its
/// sections rows of the contents at `depth`.
#[derive(Clone, Copy)]
struct Item<'a, 'input> {
    node: Node<'a, 'input>,
    head: bool,
    level: usize,
    depth: usize,
}

impl<'a, 'input> Group<'a, 'input> {
    /// The nodes the chapter writes, those inside them included.
    fn written(&self) -> Vec<Node<'a, 'input>> {
        match self {
            Self::Main(items) => items
                .iter()
                .flat_map(|item| {
                    if item.head {
                        std::iter::once(item.node)
                            .chain(
                                item.node
                                    .children()
                                    .filter(|c| !is(*c, "section"))
                                    .flat_map(|c| c.descendants()),
                            )
                            .collect::<Vec<_>>()
                    } else {
                        item.node.descendants().collect()
                    }
                })
                .collect(),
            Self::Notes(body) => body.descendants().collect(),
        }
    }

    /// Where the chapter is in the document, for its source.
    fn source(&self) -> Range<usize> {
        match self {
            Self::Main(items) => {
                items.first().map_or(0, |item| item.node.range().start)
                    ..items.last().map_or(0, |item| item.node.range().end)
            }
            Self::Notes(body) => body.range(),
        }
    }
}

/// The chapters of the book's `body`: its sections, or the sections of the
/// one section that holds them all (a book in one part), each with what
/// stands before it (the book's title, an epigraph, the part's head) or
/// after the last.
fn chapters_of<'a, 'input>(body: Node<'a, 'input>) -> Vec<Group<'a, 'input>> {
    let item = |node, head, level, depth| Item {
        node,
        head,
        level,
        depth,
    };
    let mut pending = Vec::new();
    let mut container = body;
    let (mut level, mut depth) = (1, 0);
    loop {
        let sections: Vec<_> = container.children().filter(|c| is(*c, "section")).collect();
        let [only] = sections.as_slice() else {
            break;
        };
        if !only.children().any(|c| is(c, "section")) || level >= limits::NESTING {
            break;
        }
        if container == body {
            pending.extend(
                body.children()
                    .filter(|c| c.id() != only.id() && is_content(*c))
                    .map(|c| item(c, false, level, depth)),
            );
        }
        pending.push(item(*only, true, level, depth));
        if child(*only, "title").is_some() {
            depth += 1;
        }
        container = *only;
        level += 1;
    }
    // Of a part, only its sections: the rest is its head's.
    let part = container != body;
    let mut groups: Vec<Group<'a, 'input>> = Vec::new();
    for node in container.children().filter(|c| is_content(*c)) {
        if is(node, "section") {
            let mut items = std::mem::take(&mut pending);
            items.push(item(node, false, level, depth));
            groups.push(Group::Main(items));
        } else if !part {
            pending.push(item(node, false, level, depth));
        }
    }
    if !pending.is_empty() {
        match groups.last_mut() {
            Some(Group::Main(items)) => items.extend(pending),
            _ => groups.push(Group::Main(pending)),
        }
    }
    groups
}

/// Whether `node` is read: an element, or text that is not only space.
fn is_content(node: Node<'_, '_>) -> bool {
    node.is_element() || node.text().is_some_and(|text| !text.trim().is_empty())
}

/// XHTML written for one chapter, with the contents' rows its sections
/// give.
struct Writer<'w> {
    ids: &'w HashMap<String, usize>,
    anchors: &'w HashMap<NodeId, String>,
    chapter: usize,
    out: String,
    rows: &'w mut Vec<Row>,
    /// How deep the elements written nest: past `limits::NESTING`, what is
    /// deeper is its text alone, where reading mode still reads it (it
    /// passes over elements deeper than `limits::HTML_DEPTH`), and no
    /// writer's stack goes as deep as the parser does.
    nest: usize,
    /// The nodes written: elements, and texts (each counted, though
    /// reading mode's parser joins texts that follow each other).
    nodes: usize,
    /// Whether the chapter holds more than reading mode reads of it.
    cut: bool,
}

impl Writer<'_> {
    /// Whether one more node of the book is written: what it writes
    /// before what it holds is two nodes at most (a picture's paragraph and
    /// the picture), and a chapter is at most what reading mode parses as
    /// XML (`limits::XML_NODES`, its document's root one of them). Past it,
    /// the rest is left out, or the chapter would go to the HTML parser
    /// whole.
    fn room(&mut self) -> bool {
        let room = self.nodes + 2 < limits::XML_NODES as usize;
        self.cut |= !room;
        room
    }

    /// `node`, its titles headings of `level`, its sections rows of the
    /// contents at `depth`.
    fn node(&mut self, node: Node<'_, '_>, level: usize, depth: usize) {
        if !self.room() {
            return;
        }
        if node.is_text() {
            self.text(node.text().unwrap_or_default());
            return;
        }
        if !node.is_element() {
            return;
        }
        if self.nest >= limits::NESTING {
            self.flat(node);
            return;
        }
        self.nest += 1;
        self.element(node, level, depth);
        self.nest -= 1;
    }

    fn text(&mut self, text: &str) {
        self.nodes += 1;
        escape(&mut self.out, text);
    }

    /// What `node` holds, as text alone.
    fn flat(&mut self, node: Node<'_, '_>) {
        for text in node
            .descendants()
            .filter_map(|n| n.is_text().then(|| n.text()).flatten())
        {
            if !self.room() {
                return;
            }
            self.text(text);
        }
    }

    fn element(&mut self, node: Node<'_, '_>, level: usize, depth: usize) {
        let name = node.tag_name().name();
        match name {
            "section" => self.section(node, level, depth),
            "title" => {
                let heading = level.clamp(1, 6);
                self.open(&format!("h{heading}"), node, "");
                self.lines(node);
                let _ = write!(self.out, "</h{heading}>");
            }
            "p" | "v" => self.wrap("p", node, "", level, depth),
            "subtitle" => self.wrap("p", node, "subtitle", level, depth),
            "text-author" => self.wrap("p", node, "text-author", level, depth),
            "date" => self.wrap("p", node, "date", level, depth),
            "epigraph" => self.wrap("blockquote", node, "epigraph", level + 1, depth),
            "cite" | "annotation" => self.wrap("blockquote", node, "", level + 1, depth),
            "poem" => self.wrap("div", node, "poem", level + 1, depth),
            "stanza" => self.wrap("div", node, "stanza", level + 1, depth),
            "empty-line" => {
                self.nodes += 2;
                self.out.push_str("<p>\u{a0}</p>");
            }
            "image" => self.image(node),
            "emphasis" => self.wrap("em", node, "", level, depth),
            "strong" => self.wrap("strong", node, "", level, depth),
            "strikethrough" => self.wrap("s", node, "", level, depth),
            "sub" => self.wrap("sub", node, "", level, depth),
            "sup" => self.wrap("sup", node, "", level, depth),
            "code" => self.wrap("code", node, "", level, depth),
            "a" => self.link(node, level, depth),
            "table" | "tr" | "th" | "td" => {
                let mut extra = String::new();
                for attribute in ["colspan", "rowspan", "align"] {
                    if let Some(value) = node.attribute(attribute) {
                        let _ = write!(extra, r#" {attribute}=""#);
                        escape(&mut extra, value);
                        extra.push('"');
                    }
                }
                self.open(name, node, "");
                self.out.push_str(&extra);
                self.children(node, level, depth);
                let _ = write!(self.out, "</{name}>");
            }
            // An element of its own (`style`, an unknown one): its text,
            // and its id as an anchor.
            _ => {
                if let Some(id) = node.attribute("id") {
                    self.span(id);
                }
                self.children(node, level, depth);
            }
        }
    }

    fn children(&mut self, node: Node<'_, '_>, level: usize, depth: usize) {
        for child in node.children() {
            self.node(child, level, depth);
        }
    }

    /// `node` as the element `tag`, of `class`, its id kept: the start tag
    /// left open for attributes.
    fn open(&mut self, tag: &str, node: Node<'_, '_>, class: &str) {
        self.nodes += 1;
        let _ = write!(self.out, "<{tag}");
        if !class.is_empty() {
            let _ = write!(self.out, r#" class="{class}""#);
        }
        if let Some(id) = node.attribute("id") {
            self.id(id);
        }
    }

    fn id(&mut self, id: &str) {
        self.out.push_str(r#" id=""#);
        escape(&mut self.out, id);
        self.out.push('"');
    }

    /// An empty element that is a place, `id`.
    fn span(&mut self, id: &str) {
        self.nodes += 1;
        self.out.push_str("<span");
        self.id(id);
        self.out.push_str("></span>");
    }

    fn wrap(&mut self, tag: &str, node: Node<'_, '_>, class: &str, level: usize, depth: usize) {
        self.open(tag, node, class);
        self.out.push('>');
        self.children(node, level, depth);
        let _ = write!(self.out, "</{tag}>");
    }

    /// A title's paragraphs as the lines of one heading.
    fn lines(&mut self, title: Node<'_, '_>) {
        self.out.push('>');
        let mut first = true;
        for line in title.children().filter(Node::is_element) {
            if is(line, "p") {
                if !self.room() {
                    return;
                }
                if !first {
                    self.nodes += 1;
                    self.out.push_str("<br/>");
                }
                first = false;
                self.children(line, 0, 0);
            }
        }
    }

    /// A section, a row of the contents when it has a title; its own
    /// sections' titles a level down.
    fn section(&mut self, node: Node<'_, '_>, level: usize, depth: usize) {
        let (anchor, inner) = self.row(node, depth);
        self.nodes += 1;
        self.out.push_str("<section");
        if let Some(anchor) = &anchor {
            self.id(anchor);
        }
        self.out.push('>');
        for child in node.children() {
            if is(child, "section") {
                self.node_section(child, level + 1, inner);
            } else {
                self.node(child, level, inner);
            }
        }
        self.out.push_str("</section>");
    }

    fn node_section(&mut self, node: Node<'_, '_>, level: usize, depth: usize) {
        if !self.room() {
            return;
        }
        if self.nest >= limits::NESTING {
            self.flat(node);
            return;
        }
        self.nest += 1;
        self.section(node, level, depth);
        self.nest -= 1;
    }

    /// The head of a part over every chapter: its row, its title and what
    /// is not one of its sections, which are chapters of their own.
    fn head(&mut self, part: Node<'_, '_>, level: usize, depth: usize) {
        let (anchor, inner) = self.row(part, depth);
        if let Some(anchor) = &anchor {
            self.span(anchor);
        }
        for child in part.children().filter(|c| !is(*c, "section")) {
            self.node(child, level, inner);
        }
    }

    /// The row a section with a title gives the contents, at `depth`: its
    /// anchor, and the depth of the rows its sections give.
    fn row(&mut self, section: Node<'_, '_>, depth: usize) -> (Option<String>, usize) {
        let anchor = section
            .attribute("id")
            .map(str::to_owned)
            .or_else(|| self.anchors.get(&section.id()).cloned());
        let title = child(section, "title").map(plain).filter(|t| !t.is_empty());
        let Some(title) = title else {
            return (anchor, depth);
        };
        self.rows.push(Row {
            cells: vec![title],
            target: Some(match &anchor {
                Some(anchor) => format!("{}#{anchor}", href(self.chapter)),
                None => href(self.chapter),
            }),
            depth: depth.min(limits::NESTING),
            ..Row::default()
        });
        (anchor, depth + 1)
    }

    /// A body of notes, each of its sections a note an endnote reference
    /// opens.
    fn notes(&mut self, body: Node<'_, '_>) {
        let title = child(body, "title")
            .map(|title| plain(title))
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| "Notes".into());
        self.rows.push(Row {
            cells: vec![title.clone()],
            target: Some(href(self.chapter)),
            ..Row::default()
        });
        self.nodes += 1;
        self.out.push_str("<h1>");
        self.text(&title);
        self.out.push_str("</h1>");
        for child in body.children() {
            if !self.room() {
                return;
            }
            if is(child, "section") {
                self.nodes += 1;
                self.out.push_str(r#"<div epub:type="endnote""#);
                if let Some(id) = child.attribute("id") {
                    self.id(id);
                }
                self.out.push('>');
                for part in child.children() {
                    if is(part, "title") && self.room() {
                        self.nodes += 1;
                        self.out.push_str(r#"<p class="title""#);
                        self.lines(part);
                        self.out.push_str("</p>");
                    } else {
                        self.node(part, 2, 0);
                    }
                }
                self.out.push_str("</div>");
            } else if !is(child, "title") {
                self.node(child, 2, 0);
            }
        }
    }

    /// A link: to a place in the book, by the chapter that holds it.
    fn link(&mut self, node: Node<'_, '_>, level: usize, depth: usize) {
        let target = node
            .attribute((XLINK, "href"))
            .or_else(|| node.attribute("href"))
            .unwrap_or_default();
        let target = match target.strip_prefix('#') {
            Some(id) => match self.ids.get(id) {
                Some(&chapter) if chapter == self.chapter => format!("#{id}"),
                Some(&chapter) => format!("{}#{id}", href(chapter)),
                None => format!("#{id}"),
            },
            None => target.to_owned(),
        };
        self.nodes += 1;
        self.out.push_str("<a");
        if node.attribute("type") == Some("note") {
            self.out.push_str(r#" epub:type="noteref""#);
        }
        if let Some(id) = node.attribute("id") {
            self.id(id);
        }
        self.out.push_str(r#" href=""#);
        escape(&mut self.out, &target);
        self.out.push_str(r#"">"#);
        self.children(node, level, depth);
        self.out.push_str("</a>");
    }

    /// A picture the book holds, by its binary's id: a block of its own
    /// unless it stands in a paragraph.
    fn image(&mut self, node: Node<'_, '_>) {
        let source = node
            .attribute((XLINK, "href"))
            .or_else(|| node.attribute("href"))
            .unwrap_or_default();
        let inline = node.parent().is_some_and(|parent| {
            ["p", "v", "subtitle", "text-author", "td", "th", "a"]
                .iter()
                .any(|name| is(parent, name))
        });
        if !inline {
            self.nodes += 1;
            self.out.push_str(r#"<p class="image""#);
            if let Some(id) = node.attribute("id") {
                self.id(id);
            }
            self.out.push('>');
        }
        self.nodes += 1;
        self.out.push_str(r#"<img src=""#);
        escape(&mut self.out, source);
        self.out.push_str(r#"" alt=""#);
        escape(&mut self.out, node.attribute("alt").unwrap_or_default());
        self.out.push_str(r#""/>"#);
        if !inline {
            self.out.push_str("</p>");
        }
    }
}

/// A title's text: its paragraphs, a space between.
fn plain(node: Node<'_, '_>) -> String {
    let mut text = String::new();
    for line in node.children().filter(|c| is(*c, "p")) {
        let words: String = line
            .descendants()
            .filter(Node::is_text)
            .filter_map(|n| n.text())
            .collect();
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(words.trim());
    }
    if text.is_empty() {
        text = node
            .descendants()
            .filter(Node::is_text)
            .filter_map(|n| n.text())
            .collect::<String>();
    }
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `text` escaped for XML text and attribute values.
fn escape(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
}

/// What the description says of the book, for Properties.
fn describe(description: Node<'_, '_>) -> Vec<(&'static str, Option<String>)> {
    let text = |node: Option<Node<'_, '_>>| {
        node.map(|n| {
            n.descendants()
                .filter(Node::is_text)
                .filter_map(|t| t.text())
                .collect::<String>()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|t| !t.is_empty())
    };
    let title = child(description, "title-info");
    let publish = child(description, "publish-info");
    let authors: Vec<String> = title
        .iter()
        .flat_map(|info| info.children().filter(|c| is(*c, "author")))
        .filter_map(|author| {
            let name: Vec<String> = ["first-name", "middle-name", "last-name"]
                .iter()
                .filter_map(|part| text(child(author, part)))
                .collect();
            if name.is_empty() {
                text(child(author, "nickname"))
            } else {
                Some(name.join(" "))
            }
        })
        .collect();
    let genres: Vec<String> = title
        .iter()
        .flat_map(|info| info.children().filter(|c| is(*c, "genre")))
        .filter_map(|genre| text(Some(genre)))
        .collect();
    let series = title
        .and_then(|info| child(info, "sequence"))
        .and_then(|sequence| {
            let name = sequence.attribute("name")?.trim();
            Some(match sequence.attribute("number") {
                Some(number) => format!("{name}, {}", number.trim()),
                None => name.to_owned(),
            })
        });
    let field = |info: Option<Node<'_, '_>>, name: &str| text(info.and_then(|i| child(i, name)));
    vec![
        ("Title", field(title, "book-title")),
        ("Author", (!authors.is_empty()).then(|| authors.join(", "))),
        ("Series", series),
        ("Genre", (!genres.is_empty()).then(|| genres.join(", "))),
        ("Language", field(title, "lang")),
        ("Publisher", field(publish, "publisher")),
        ("Year", field(publish, "year")),
    ]
}

/// The binary the description names as the book's cover.
fn cover_id<'a>(description: Node<'a, '_>) -> Option<&'a str> {
    let image = child(
        child(child(description, "title-info")?, "coverpage")?,
        "image",
    )?;
    let href = image
        .attribute((XLINK, "href"))
        .or_else(|| image.attribute("href"))?;
    Some(href.trim_start_matches('#'))
}

/// An FB2 as reading mode reads it.
pub(crate) struct Reading<'a>(pub &'a Fb2);

impl Book for Reading<'_> {
    type Resources<'r>
        = Resources<'r>
    where
        Self: 'r;
    fn format(&self) -> &'static str {
        "FB2"
    }
    const SOURCE: &'static str = "xml";
    fn chapters(&self) -> &[ChapterRef] {
        &self.0.refs
    }
    fn text(
        &mut self,
        href: &str,
        _cancel: &CancelToken,
    ) -> anyhow::Result<(String, Option<String>)> {
        let chapter = self.0.chapter(href)?;
        let cut = chapter
            .cut
            .then(|| String::from("The chapter is cut: it holds more than reading mode reads"));
        let notice: Vec<String> = self.0.decoded.clone().into_iter().chain(cut).collect();
        Ok((
            chapter.xhtml.clone(),
            (!notice.is_empty()).then(|| notice.join(" · ")),
        ))
    }
    fn source(
        &mut self,
        href: &str,
        _cancel: &CancelToken,
    ) -> anyhow::Result<(String, Option<String>)> {
        let range = self.0.chapter(href)?.source.clone();
        Ok((
            self.0.text.get(range).unwrap_or_default().to_owned(),
            self.0.decoded.clone(),
        ))
    }
    fn resources<'r>(
        &'r mut self,
        _base: &'r str,
        images: bool,
        _sheets: &'r mut Sheets,
        cancel: &'r CancelToken,
    ) -> Resources<'r> {
        Resources {
            book: self.0,
            images,
            cancel,
        }
    }
    fn contents(&mut self) -> Vec<Row> {
        self.0.contents.clone()
    }
    fn details(&mut self) -> Vec<(&'static str, Option<String>)> {
        self.0.details.clone()
    }
}

/// What a chapter names: the book's pictures, by id, and its stylesheet.
pub(crate) struct Resources<'a> {
    book: &'a Fb2,
    images: bool,
    cancel: &'a CancelToken,
}

impl rich_preview::Resources for Resources<'_> {
    /// Named by its binary's id for the view to decode when it shows it,
    /// with the size it is shown at, read once.
    fn picture(&mut self, src: &str) -> Option<Picture> {
        if !self.images {
            return None;
        }
        let key = src.strip_prefix('#').unwrap_or(src).to_owned();
        let book = self.book;
        book.sizes
            .picture(key, self.cancel, |key, limit| book.binary(key, limit).ok())
    }
    fn stylesheet(&mut self, href: &str) -> Option<String> {
        (href == STYLESHEET).then(|| CSS.to_owned())
    }
    fn parsed_stylesheet(&mut self, href: &str, base: &str) -> Option<Arc<rich_preview::Parsed>> {
        (href == STYLESHEET).then(|| Arc::new(rich_preview::Parsed::new(CSS, base)))
    }
    fn file(&mut self, href: &str) -> Option<Vec<u8>> {
        self.book.binary(href, limits::BINARY_INPUT).ok()
    }
}

#[cfg(test)]
mod tests;
