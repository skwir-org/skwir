//! An `OpenDocument` text (`.odt`, `.ott`) read as text: its body walked
//! into blocks, each paragraph shaped by its styles and lists.
use crate::blocks::{self, Builder, Look, Read, Shape, is_code, is_quote};
use crate::package::{self, Package};
use roxmltree::Node;
use skwir_core::CancelToken;
use skwir_core::inspection::{Align, Script};
use skwir_core::preview::FileSource;
use skwir_platform::inspection::{self, Numbering, limits};
use std::collections::HashMap;

const OFFICE: &str = "urn:oasis:names:tc:opendocument:xmlns:office:1.0";
const STYLE: &str = "urn:oasis:names:tc:opendocument:xmlns:style:1.0";
const TEXT: &str = "urn:oasis:names:tc:opendocument:xmlns:text:1.0";
const TABLE: &str = "urn:oasis:names:tc:opendocument:xmlns:table:1.0";
const DRAW: &str = "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0";
const FO: &str = "urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0";
const SVG: &str = "urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0";
const XLINK: &str = "http://www.w3.org/1999/xlink";
const META: &str = "urn:oasis:names:tc:opendocument:xmlns:meta:1.0";
const DC: &str = "http://purl.org/dc/elements/1.1/";

fn is(node: Node<'_, '_>, ns: &str, local: &str) -> bool {
    node.is_element() && node.tag_name().name() == local && node.tag_name().namespace() == Some(ns)
}

fn child<'a, 'i>(node: Node<'a, 'i>, ns: &str, local: &str) -> Option<Node<'a, 'i>> {
    node.children().find(|c| is(*c, ns, local))
}

/// The text properties one style sets; `None` leaves its parent's.
#[derive(Clone, Copy, Debug, Default)]
struct Props {
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    strike: Option<bool>,
    script: Option<Script>,
    small_caps: Option<bool>,
    caps: Option<bool>,
    hidden: Option<bool>,
}

impl Props {
    fn over(self, top: Props) -> Props {
        Props {
            bold: top.bold.or(self.bold),
            italic: top.italic.or(self.italic),
            underline: top.underline.or(self.underline),
            strike: top.strike.or(self.strike),
            script: top.script.or(self.script),
            small_caps: top.small_caps.or(self.small_caps),
            caps: top.caps.or(self.caps),
            hidden: top.hidden.or(self.hidden),
        }
    }

    fn read(node: Option<Node<'_, '_>>) -> Props {
        let Some(node) = node else {
            return Props::default();
        };
        let get = |ns, local| node.attribute((ns, local));
        Props {
            bold: get(FO, "font-weight")
                .map(|w| w == "bold" || w.parse::<u32>().is_ok_and(|w| w >= 600)),
            italic: get(FO, "font-style").map(|s| s == "italic" || s == "oblique"),
            underline: get(STYLE, "text-underline-style").map(|s| s != "none"),
            strike: get(STYLE, "text-line-through-style").map(|s| s != "none"),
            script: get(STYLE, "text-position").map(|p| {
                let first = p.split_whitespace().next().unwrap_or_default();
                match first {
                    "super" => Script::Superscript,
                    "sub" => Script::Subscript,
                    percent => match percent.trim_end_matches('%').parse::<f32>() {
                        Ok(raised) if raised > 0. => Script::Superscript,
                        Ok(lowered) if lowered < 0. => Script::Subscript,
                        _ => Script::Baseline,
                    },
                }
            }),
            small_caps: get(FO, "font-variant").map(|v| v == "small-caps"),
            caps: get(FO, "text-transform").map(|t| t == "uppercase"),
            hidden: get(TEXT, "display").map(|d| d == "none"),
        }
    }

    fn look(self, code: bool) -> Look {
        Look {
            bold: self.bold.unwrap_or(false),
            italic: self.italic.unwrap_or(false),
            underline: self.underline.unwrap_or(false),
            strike: self.strike.unwrap_or(false),
            script: self.script.unwrap_or_default(),
            small_caps: self.small_caps.unwrap_or(false),
            caps: self.caps.unwrap_or(false),
            hidden: self.hidden.unwrap_or(false),
            code,
        }
    }
}

/// A length (`0.5in`, `1.27cm`, `12pt`) in ems of a 12 pt body.
fn ems(length: &str) -> Option<f32> {
    let split = length
        .find(|c: char| c.is_ascii_alphabetic())
        .unwrap_or(length.len());
    let (number, unit) = length.split_at(split);
    let number: f32 = number.trim().parse().ok()?;
    let points = match unit {
        "in" => number * 72.,
        "cm" => number * 72. / 2.54,
        "mm" => number * 72. / 25.4,
        "pt" => number,
        "pc" => number * 12.,
        "px" => number * 0.75,
        _ => return None,
    };
    Some(points / 12.)
}

struct Style {
    parent: Option<String>,
    /// What the user sees it called, lowercased: built-in styles are known
    /// by it (`Heading 1`, `Title`, `Quotations`).
    name: String,
    text: Props,
    align: Option<Align>,
    indent: Option<f32>,
    /// The list style its paragraphs are items of.
    list: Option<String>,
    /// Written for one place in the content (`P1`), not named by the
    /// user: it stands for the named style it is based on.
    automatic: bool,
}

/// How one level of a list style marks its items.
#[derive(Clone, Debug, Default)]
struct Level {
    /// A bullet, or a number format (`None`: no number).
    bullet: Option<String>,
    format: Option<Numbering>,
    prefix: String,
    suffix: String,
    /// How many levels' numbers the marker shows (`1.2.`).
    display: usize,
    start: u32,
}

#[derive(Default)]
struct Styles {
    /// By family and name.
    by_name: HashMap<(String, String), Style>,
    default_text: Props,
    lists: HashMap<String, Vec<Option<Level>>>,
    /// How headings are numbered, by level.
    outline: Vec<Option<Level>>,
}

const LEVELS: usize = 10;

fn levels(list: Node<'_, '_>) -> Vec<Option<Level>> {
    let mut levels = vec![None; LEVELS];
    for level in list.children().filter(Node::is_element) {
        let Some(index) = level
            .attribute((TEXT, "level"))
            .and_then(|l| l.parse::<usize>().ok())
            .filter(|l| (1..=LEVELS).contains(l))
        else {
            continue;
        };
        let affix = |local| {
            level
                .attribute((STYLE, local))
                .unwrap_or_default()
                .to_owned()
        };
        let defined = match level.tag_name().name() {
            // A bullet is its character: `LibreOffice` also writes it as
            // the level's suffix.
            "list-level-style-bullet" => Level {
                bullet: Some(blocks::bullet(
                    level.attribute((TEXT, "bullet-char")).unwrap_or("•"),
                )),
                ..Level::default()
            },
            "list-level-style-image" => Level {
                bullet: Some("•".into()),
                ..Level::default()
            },
            "list-level-style-number" | "outline-level-style" => Level {
                format: match level.attribute((STYLE, "num-format")).unwrap_or_default() {
                    "" => None,
                    "a" => Some(Numbering::LowerLetter),
                    "A" => Some(Numbering::UpperLetter),
                    "i" => Some(Numbering::LowerRoman),
                    "I" => Some(Numbering::UpperRoman),
                    _ => Some(Numbering::Decimal),
                },
                prefix: affix("num-prefix"),
                suffix: affix("num-suffix"),
                display: level
                    .attribute((TEXT, "display-levels"))
                    .and_then(|d| d.parse().ok())
                    .unwrap_or(1),
                start: level
                    .attribute((TEXT, "start-value"))
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(1),
                ..Level::default()
            },
            _ => continue,
        };
        levels[index - 1] = Some(defined);
    }
    levels
}

impl Styles {
    /// The styles a part declares (`office:styles` of styles.xml, the
    /// automatic styles of content.xml) added to these.
    fn add(&mut self, container: Node<'_, '_>, automatic: bool) {
        for node in container.children().filter(Node::is_element) {
            if is(node, STYLE, "style") {
                let (Some(name), Some(family)) = (
                    node.attribute((STYLE, "name")),
                    node.attribute((STYLE, "family")),
                ) else {
                    continue;
                };
                let paragraph = child(node, STYLE, "paragraph-properties");
                self.by_name.insert(
                    (family.to_owned(), name.to_owned()),
                    Style {
                        parent: node
                            .attribute((STYLE, "parent-style-name"))
                            .map(str::to_owned),
                        name: node
                            .attribute((STYLE, "display-name"))
                            .unwrap_or(name)
                            .to_lowercase(),
                        text: Props::read(child(node, STYLE, "text-properties")),
                        align: paragraph
                            .and_then(|p| p.attribute((FO, "text-align")))
                            .map(|a| match a {
                                "center" => Align::Center,
                                "end" | "right" => Align::End,
                                _ => Align::Start,
                            }),
                        indent: paragraph
                            .and_then(|p| p.attribute((FO, "margin-left")))
                            .and_then(ems),
                        list: node
                            .attribute((STYLE, "list-style-name"))
                            .map(str::to_owned),
                        automatic,
                    },
                );
            } else if is(node, STYLE, "default-style")
                && node.attribute((STYLE, "family")) == Some("paragraph")
            {
                self.default_text = Props::read(child(node, STYLE, "text-properties"));
            } else if is(node, TEXT, "list-style") {
                if let Some(name) = node.attribute((STYLE, "name")) {
                    self.lists.insert(name.to_owned(), levels(node));
                }
            } else if is(node, TEXT, "outline-style") {
                self.outline = levels(node);
            }
        }
    }

    /// Style `name` of `family` and its parents, itself first.
    fn chain(&self, family: &str, name: Option<&str>) -> Vec<&Style> {
        let mut chain: Vec<&Style> = Vec::new();
        let mut next = name;
        while let Some(name) = next {
            let Some(style) = self.by_name.get(&(family.to_owned(), name.to_owned())) else {
                break;
            };
            if chain.len() == limits::NESTING || chain.iter().any(|s| std::ptr::eq(*s, style)) {
                break;
            }
            chain.push(style);
            next = style.parent.as_deref();
        }
        chain
    }

    fn text_of(&self, family: &str, name: Option<&str>) -> Props {
        self.chain(family, name)
            .iter()
            .rev()
            .fold(Props::default(), |props, style| props.over(style.text))
    }

    fn named(&self, family: &str, name: Option<&str>, test: impl Fn(&str) -> bool) -> bool {
        self.chain(family, name)
            .iter()
            .any(|style| test(&style.name))
    }
}

/// A list being read: its style, and how far each level has counted.
struct List {
    style: Option<String>,
    counts: [u32; LEVELS],
}

struct Walk<'p> {
    builder: Builder,
    styles: Styles,
    package: &'p mut Package,
    cancel: &'p CancelToken,
    /// The lists the walk is inside, outermost first, and the counts each
    /// list style ended on, for a list that continues it.
    lists: Vec<List>,
    ended: HashMap<String, [u32; LEVELS]>,
    /// Whether the item being read has given its marker yet.
    item_marked: bool,
    /// The item's number as it says it (`text:start-value`).
    item_start: Option<u32>,
    headings: [u32; LEVELS],
    /// Notes met in the body, numbered in order, and their bodies.
    footnotes: u32,
    endnotes: u32,
    visited: usize,
    depth: usize,
    /// How deep the walk is in the XML: see `nested`.
    nest: usize,
}

type Boxes<'a, 'i> = Vec<Node<'a, 'i>>;

/// Notes of the body by target: their label and body.
type Notes<'a, 'i> = HashMap<String, (String, Node<'a, 'i>)>;

impl Walk<'_> {
    /// Walk one element deeper: past `limits::XML_DEPTH`, what the file
    /// nests is left out (spans in spans, sections in sections: ten
    /// thousand of them in a small part would overflow the stack).
    fn nested(&mut self, walk: impl FnOnce(&mut Self) -> anyhow::Result<()>) -> anyhow::Result<()> {
        if self.nest >= limits::XML_DEPTH {
            return Ok(());
        }
        self.nest += 1;
        let done = walk(self);
        self.nest -= 1;
        done
    }

    fn check(&mut self) -> anyhow::Result<()> {
        self.visited += 1;
        if self.visited.is_multiple_of(256) {
            inspection::check(self.cancel)?;
        }
        Ok(())
    }

    fn blocks<'a, 'i>(
        &mut self,
        node: Node<'a, 'i>,
        top: bool,
        notes: &mut Notes<'a, 'i>,
    ) -> anyhow::Result<()> {
        self.nested(|walk| walk.blocks_in(node, top, notes))
    }

    fn blocks_in<'a, 'i>(
        &mut self,
        node: Node<'a, 'i>,
        top: bool,
        notes: &mut Notes<'a, 'i>,
    ) -> anyhow::Result<()> {
        for n in node.children().filter(Node::is_element) {
            self.check()?;
            if is(n, TEXT, "p") || is(n, TEXT, "h") {
                self.paragraph(n, notes)?;
                if top {
                    self.builder.boundary();
                }
            } else if is(n, TEXT, "list") {
                self.list(n, top, notes)?;
            } else if is(n, TABLE, "table") {
                self.table(n, top, notes)?;
            } else if is(n, TEXT, "section")
                || is(n, TEXT, "index-body")
                || is(n, TEXT, "index-title")
                || is(n, TEXT, "numbered-paragraph")
            {
                self.blocks(n, top, notes)?;
            } else if n.tag_name().namespace() == Some(TEXT)
                && (n.tag_name().name().ends_with("index")
                    || n.tag_name().name() == "table-of-content"
                    || n.tag_name().name() == "bibliography")
            {
                if let Some(body) = child(n, TEXT, "index-body") {
                    self.blocks(body, top, notes)?;
                }
            } else if is(n, DRAW, "frame") || is(n, DRAW, "a") {
                let mut boxes = Boxes::new();
                self.frame(n, &mut boxes);
                self.text_boxes(&boxes, notes)?;
            } else if (is(n, TEXT, "bookmark") || is(n, TEXT, "bookmark-start"))
                && let Some(name) = n.attribute((TEXT, "name"))
            {
                self.builder.anchor(name);
            }
        }
        Ok(())
    }

    fn list<'a, 'i>(
        &mut self,
        list: Node<'a, 'i>,
        top: bool,
        notes: &mut Notes<'a, 'i>,
    ) -> anyhow::Result<()> {
        if self.lists.len() >= LEVELS {
            return self.blocks(list, top, notes);
        }
        let style = list
            .attribute((TEXT, "style-name"))
            .map(str::to_owned)
            .or_else(|| self.lists.last().and_then(|l| l.style.clone()));
        let continues = list.attribute((TEXT, "continue-numbering")) == Some("true")
            || list.attribute((TEXT, "continue-list")).is_some();
        let counts = if self.lists.is_empty() && continues {
            style
                .as_ref()
                .and_then(|s| self.ended.get(s).copied())
                .unwrap_or_default()
        } else if let Some(outer) = self.lists.last() {
            outer.counts
        } else {
            [0; LEVELS]
        };
        self.lists.push(List { style, counts });
        for item in list.children().filter(Node::is_element) {
            let header = is(item, TEXT, "list-header");
            if !header && !is(item, TEXT, "list-item") {
                continue;
            }
            self.item_marked = header;
            self.item_start = item
                .attribute((TEXT, "start-value"))
                .and_then(|s| s.parse().ok());
            self.blocks(item, top, notes)?;
        }
        let ended = self.lists.pop().expect("pushed above");
        if let Some(outer) = self.lists.last_mut() {
            outer.counts = ended.counts;
        } else if let Some(style) = ended.style {
            self.ended.insert(style, ended.counts);
        }
        self.item_marked = true;
        Ok(())
    }

    /// The marker of the list item a paragraph begins, counted; `None`
    /// for one that goes on an item.
    fn list_marker(&mut self, paragraph_list_style: Option<&str>) -> Option<(u32, Option<String>)> {
        let depth = self.lists.len();
        if depth == 0 {
            return None;
        }
        let level = depth - 1;
        if self.item_marked {
            return Some((depth as u32, None));
        }
        self.item_marked = true;
        let list = self.lists.last_mut()?;
        let style = list
            .style
            .clone()
            .or_else(|| paragraph_list_style.map(str::to_owned));
        let definition = style
            .as_deref()
            .and_then(|s| self.styles.lists.get(s))
            .and_then(|levels| levels.get(level).cloned().flatten())
            .unwrap_or(Level {
                bullet: Some("•".into()),
                ..Level::default()
            });
        list.counts[level] = match self.item_start.take() {
            Some(start) => start,
            None if list.counts[level] == 0 => definition.start.max(1),
            None => list.counts[level].saturating_add(1),
        };
        for deeper in &mut list.counts[level + 1..] {
            *deeper = 0;
        }
        let counts = list.counts;
        let levels = style.as_deref().and_then(|s| self.styles.lists.get(s));
        Some((
            depth as u32,
            Some(marker(&definition, &counts, level, levels)),
        ))
    }

    fn paragraph<'a, 'i>(
        &mut self,
        p: Node<'a, 'i>,
        notes: &mut Notes<'a, 'i>,
    ) -> anyhow::Result<()> {
        let style = p.attribute((TEXT, "style-name"));
        let chain = self.styles.chain("paragraph", style);
        let align = chain.iter().find_map(|s| s.align).unwrap_or_default();
        let indent = chain.iter().find_map(|s| s.indent).unwrap_or(0.);
        let list_style = chain.iter().find_map(|s| s.list.clone());
        // What the user calls its style, nearest first; a style only named,
        // which the document never defines, by its name (`Block_20_Text`).
        let mut names: Vec<String> = chain
            .iter()
            .filter(|s| !s.automatic)
            .map(|s| s.name.clone())
            .collect();
        if chain.is_empty()
            && let Some(style) = style
        {
            names.push(style.replace("_20_", " ").to_lowercase());
        }
        let heading = if is(p, TEXT, "h") {
            p.attribute((TEXT, "outline-level"))
                .and_then(|l| l.parse::<u8>().ok())
                .unwrap_or(1)
                .clamp(1, 6)
        } else {
            // Its title, not a style based on it (an author line).
            u8::from(names.first().is_some_and(|name| name == "title"))
        };
        let mut shape = Shape {
            heading,
            align,
            indent,
            quote: names.iter().any(|name| is_quote(name)),
            code: names.iter().any(|name| is_code(name)),
            ..Shape::default()
        };
        if let Some((depth, marker)) = self.list_marker(list_style.as_deref()) {
            shape.list_depth = depth;
            shape.marker = marker;
        } else if heading > 0 && p.attribute((TEXT, "is-list-header")) != Some("true") {
            shape.marker = self.heading_number(usize::from(heading));
        }
        self.builder.paragraph(shape);
        let base = self
            .styles
            .default_text
            .over(self.styles.text_of("paragraph", style));
        let mut boxes = Boxes::new();
        let mut space = true;
        self.inline(p, base, None, &mut boxes, notes, &mut space)?;
        self.builder.end_paragraph();
        self.text_boxes(&boxes, notes)
    }

    /// The number the outline style gives a heading of `level`, counted.
    fn heading_number(&mut self, level: usize) -> Option<String> {
        let index = level - 1;
        let definition = self.styles.outline.get(index).cloned().flatten()?;
        definition.format?;
        self.headings[index] = if self.headings[index] == 0 {
            definition.start.max(1)
        } else {
            self.headings[index].saturating_add(1)
        };
        for deeper in &mut self.headings[level..] {
            *deeper = 0;
        }
        let marker = marker(
            &definition,
            &self.headings,
            index,
            Some(&self.styles.outline),
        );
        (!marker.trim().is_empty()).then_some(marker)
    }

    fn text_boxes<'a, 'i>(
        &mut self,
        boxes: &[Node<'a, 'i>],
        notes: &mut Notes<'a, 'i>,
    ) -> anyhow::Result<()> {
        if boxes.is_empty() || self.depth >= limits::NESTING {
            return Ok(());
        }
        self.depth += 1;
        for content in boxes {
            self.blocks(*content, false, notes)?;
        }
        self.depth -= 1;
        Ok(())
    }

    /// Text as ODF lays it out: runs of spaces, tabs and line ends are
    /// one space, and none starts a paragraph (`space`: the last character
    /// written was one).
    fn collapsed(text: &str, space: &mut bool) -> String {
        let mut out = String::new();
        for c in text.chars() {
            if matches!(c, ' ' | '\t' | '\n' | '\r') {
                if !*space {
                    out.push(' ');
                    *space = true;
                }
            } else {
                out.push(c);
                *space = false;
            }
        }
        out
    }

    fn inline<'a, 'i>(
        &mut self,
        node: Node<'a, 'i>,
        props: Props,
        link: Option<&str>,
        boxes: &mut Boxes<'a, 'i>,
        notes: &mut Notes<'a, 'i>,
        space: &mut bool,
    ) -> anyhow::Result<()> {
        self.nested(|walk| walk.inline_in(node, props, link, boxes, notes, space))
    }

    fn inline_in<'a, 'i>(
        &mut self,
        node: Node<'a, 'i>,
        props: Props,
        link: Option<&str>,
        boxes: &mut Boxes<'a, 'i>,
        notes: &mut Notes<'a, 'i>,
        space: &mut bool,
    ) -> anyhow::Result<()> {
        for n in node.children() {
            self.check()?;
            if n.is_text() {
                let text = Self::collapsed(n.text().unwrap_or_default(), space);
                self.builder.text(&text, &props.look(false), link);
                continue;
            }
            if !n.is_element() {
                continue;
            }
            let ns = n.tag_name().namespace();
            let local = n.tag_name().name();
            match (ns, local) {
                (Some(TEXT), "span") => {
                    let style = n.attribute((TEXT, "style-name"));
                    let props = props.over(self.styles.text_of("text", style));
                    let code = self.styles.named("text", style, is_code);
                    if code {
                        let mut look = props.look(true);
                        let text: String = n
                            .descendants()
                            .filter(Node::is_text)
                            .filter_map(|d| d.text())
                            .collect();
                        look.hidden |= text.is_empty();
                        self.builder
                            .text(&Self::collapsed(&text, space), &look, link);
                    } else {
                        self.inline(n, props, link, boxes, notes, space)?;
                    }
                }
                (Some(TEXT), "a") => {
                    let target = n.attribute((XLINK, "href"));
                    self.inline(n, props, target.or(link), boxes, notes, space)?;
                }
                (Some(TEXT), "s") => {
                    let count = n
                        .attribute((TEXT, "c"))
                        .and_then(|c| c.parse::<usize>().ok())
                        .unwrap_or(1)
                        .min(64);
                    self.builder
                        .text(&" ".repeat(count), &props.look(false), link);
                    *space = true;
                }
                (Some(TEXT), "tab") => {
                    self.builder.text("\t", &props.look(false), link);
                    *space = true;
                }
                (Some(TEXT), "line-break") => {
                    self.builder.line_break();
                    *space = true;
                }
                (Some(TEXT), "note") => self.note(n, notes),
                (
                    Some(TEXT),
                    "bookmark" | "bookmark-start" | "reference-mark" | "reference-mark-start",
                ) => {
                    if let Some(name) = n.attribute((TEXT, "name")) {
                        self.builder.anchor(name);
                    }
                }
                (Some(TEXT), "ruby") => {
                    if let Some(base) = child(n, TEXT, "ruby-base") {
                        self.inline(base, props, link, boxes, notes, space)?;
                    }
                }
                (Some(DRAW), "frame" | "a" | "g") => self.frame(n, boxes),
                (
                    Some(TEXT),
                    "change"
                    | "change-start"
                    | "change-end"
                    | "soft-page-break"
                    | "tracked-changes"
                    | "alphabetical-index-mark"
                    | "toc-mark"
                    | "user-index-mark",
                )
                | (Some(OFFICE), "annotation" | "annotation-end") => {}
                (Some(TEXT), "hidden-text") if n.attribute((TEXT, "is-hidden")) == Some("true") => {
                }
                _ => self.inline(n, props, link, boxes, notes, space)?,
            }
        }
        Ok(())
    }

    /// A note's reference in the text: labelled by its citation, or by its
    /// number in the document.
    fn note<'a, 'i>(&mut self, note: Node<'a, 'i>, notes: &mut Notes<'a, 'i>) {
        let endnote = note.attribute((TEXT, "note-class")) == Some("endnote");
        let number = if endnote {
            self.endnotes += 1;
            Numbering::LowerRoman.number(self.endnotes)
        } else {
            self.footnotes += 1;
            Numbering::Decimal.number(self.footnotes)
        };
        let label = child(note, TEXT, "note-citation")
            .and_then(|c| c.attribute((TEXT, "label")).or_else(|| c.text()))
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map_or(number, str::to_owned);
        let target = note.attribute((TEXT, "id")).map_or_else(
            || format!("note-{}", self.footnotes + self.endnotes),
            str::to_owned,
        );
        if let Some(body) = child(note, TEXT, "note-body") {
            notes.insert(target.clone(), (label.clone(), body));
        }
        self.builder.reference(&label, &target);
    }

    /// A frame: its picture, or its text box for after the paragraph.
    fn frame<'a, 'i>(&mut self, frame: Node<'a, 'i>, boxes: &mut Boxes<'a, 'i>) {
        if !is(frame, DRAW, "frame") {
            for inner in frame.children().filter(|n| is(*n, DRAW, "frame")) {
                self.frame(inner, boxes);
            }
            return;
        }
        let text = |local| {
            child(frame, SVG, local)
                .and_then(|n| n.text())
                .map(str::trim)
                .filter(|t| !t.is_empty())
        };
        let alt = text("desc").or_else(|| text("title")).unwrap_or_default();
        let picture = frame
            .children()
            .filter(|n| is(*n, DRAW, "image"))
            .filter_map(|image| image.attribute((XLINK, "href")))
            .filter(|href| !href.contains(':'))
            .find_map(|href| {
                let href = href.trim_start_matches("./");
                if self.builder.shows_this_part() {
                    self.package.picture(href, self.cancel)
                } else {
                    Package::unread_picture(href)
                }
            });
        if picture.is_some() || frame.children().any(|n| is(n, DRAW, "image")) {
            self.builder.picture(alt, picture);
        }
        boxes.extend(frame.children().filter(|n| is(*n, DRAW, "text-box")));
    }

    fn table<'a, 'i>(
        &mut self,
        table: Node<'a, 'i>,
        top: bool,
        notes: &mut Notes<'a, 'i>,
    ) -> anyhow::Result<()> {
        for (row, header) in rows(table, false) {
            self.check()?;
            self.builder.begin_row(header);
            let mut columns = 0;
            'cells: for cell in row
                .children()
                .filter(|n| is(*n, TABLE, "table-cell") || is(*n, TABLE, "covered-table-cell"))
            {
                let repeated = cell
                    .attribute((TABLE, "number-columns-repeated"))
                    .and_then(|r| r.parse::<usize>().ok())
                    .unwrap_or(1)
                    .max(1);
                for _ in 0..repeated {
                    columns += 1;
                    if columns > limits::TABLE_COLUMNS {
                        break 'cells;
                    }
                    self.builder.cell();
                    if is(cell, TABLE, "table-cell") {
                        self.cell(cell, notes)?;
                    }
                }
            }
            self.builder.end_row();
            if top {
                self.builder.boundary();
            }
        }
        Ok(())
    }

    /// A cell's paragraphs and lists, each a line of it; a table inside is
    /// read a row to a line.
    fn cell<'a, 'i>(
        &mut self,
        cell: Node<'a, 'i>,
        notes: &mut Notes<'a, 'i>,
    ) -> anyhow::Result<()> {
        self.nested(|walk| walk.cell_in(cell, notes))
    }

    fn cell_in<'a, 'i>(
        &mut self,
        cell: Node<'a, 'i>,
        notes: &mut Notes<'a, 'i>,
    ) -> anyhow::Result<()> {
        for n in cell.children().filter(Node::is_element) {
            self.check()?;
            if is(n, TABLE, "table") && self.depth < limits::NESTING {
                self.depth += 1;
                for (row, _) in rows(n, false) {
                    for (i, inner) in row
                        .children()
                        .filter(|c| is(*c, TABLE, "table-cell"))
                        .enumerate()
                    {
                        if i > 0 {
                            self.builder.text(" · ", &Look::default(), None);
                        }
                        self.cell(inner, notes)?;
                    }
                }
                self.depth -= 1;
            } else if is(n, TEXT, "p") || is(n, TEXT, "h") {
                self.paragraph(n, notes)?;
            } else if is(n, TEXT, "list") {
                self.list(n, false, notes)?;
            }
        }
        Ok(())
    }
}

/// A marker as a list style writes it: the prefix, the numbers of the
/// levels it displays, the suffix; or the bullet.
fn marker(
    definition: &Level,
    counts: &[u32; LEVELS],
    level: usize,
    levels: Option<&Vec<Option<Level>>>,
) -> String {
    if let Some(bullet) = &definition.bullet {
        return bullet.clone();
    }
    let Some(format) = definition.format else {
        return format!("{}{}", definition.prefix, definition.suffix);
    };
    let first = (level + 1).saturating_sub(definition.display.max(1));
    let numbers: Vec<String> = (first..=level)
        .map(|i| {
            let format = if i == level {
                format
            } else {
                levels
                    .and_then(|l| l.get(i).cloned().flatten())
                    .and_then(|l| l.format)
                    .unwrap_or_default()
            };
            format.number(counts[i].max(1))
        })
        .collect();
    format!(
        "{}{}{}",
        definition.prefix,
        numbers.join("."),
        definition.suffix
    )
}

/// A table's rows, through their groups, and whether each is a header.
fn rows<'a, 'i>(table: Node<'a, 'i>, header: bool) -> Vec<(Node<'a, 'i>, bool)> {
    let mut found = Vec::new();
    rows_in(table, header, 0, &mut found);
    found
}

fn rows_in<'a, 'i>(
    table: Node<'a, 'i>,
    header: bool,
    depth: usize,
    found: &mut Vec<(Node<'a, 'i>, bool)>,
) {
    if depth >= limits::XML_DEPTH {
        return;
    }
    for n in table.children().filter(Node::is_element) {
        if is(n, TABLE, "table-row") {
            found.push((n, header));
        } else if is(n, TABLE, "table-header-rows") {
            rows_in(n, true, depth + 1, found);
        } else if is(n, TABLE, "table-rows") || is(n, TABLE, "table-row-group") {
            rows_in(n, header, depth + 1, found);
        }
    }
}

pub(crate) fn read<S: FileSource + ?Sized>(
    file: &S,
    wanted: u64,
    cancel: &CancelToken,
) -> anyhow::Result<Read> {
    let mut package = Package::open(file, cancel)?;
    if let Some(manifest) = package.text("META-INF/manifest.xml", "Manifest", cancel)?
        && manifest.contains("encryption-data")
    {
        anyhow::bail!("This document is protected by a password");
    }
    let content = package
        .text("content.xml", "The document's text", cancel)?
        .ok_or_else(|| anyhow::anyhow!("This is not an OpenDocument text: it has no content"))?;
    let doc = package::xml(&content)?;
    let mut styles = Styles::default();
    let styles_xml = package.text("styles.xml", "Styles", cancel)?;
    let styles_doc = styles_xml.as_deref().map(package::xml).transpose()?;
    if let Some(named) = styles_doc
        .as_ref()
        .and_then(|d| child(d.root_element(), OFFICE, "styles"))
    {
        styles.add(named, false);
    }
    if let Some(automatic) = child(doc.root_element(), OFFICE, "automatic-styles") {
        styles.add(automatic, true);
    }
    let body = child(doc.root_element(), OFFICE, "body")
        .and_then(|b| child(b, OFFICE, "text"))
        .ok_or_else(|| anyhow::anyhow!("This OpenDocument file is not a text document"))?;
    let details = details(&mut package, cancel);
    let mut notes = Notes::new();
    let mut walk = Walk {
        builder: Builder::new(wanted),
        styles,
        package: &mut package,
        cancel,
        lists: Vec::new(),
        ended: HashMap::new(),
        item_marked: true,
        item_start: None,
        headings: [0; LEVELS],
        footnotes: 0,
        endnotes: 0,
        visited: 0,
        depth: 0,
        nest: 0,
    };
    walk.blocks(body, true, &mut notes)?;
    let (referenced, parts) = walk.builder.body_end();
    // Asked past the end (the document shrank): the last part, read again
    // with its pictures read.
    if wanted >= parts && parts > 0 {
        drop(walk);
        return read(file, parts - 1, cancel);
    }
    for (ordinal, target) in referenced.iter().enumerate() {
        let Some((label, body)) = notes
            .get(target)
            .map(|(label, body)| (label.clone(), *body))
        else {
            continue;
        };
        walk.builder
            .note(u32::try_from(ordinal).unwrap_or(u32::MAX), target);
        walk.builder.paragraph(Shape::default());
        walk.builder.text(
            &label,
            &Look {
                script: Script::Superscript,
                ..Look::default()
            },
            None,
        );
        // A space after it, unless the note begins with one.
        let spaced = child(body, TEXT, "p")
            .and_then(|p| p.first_child())
            .is_some_and(|first| {
                first
                    .text()
                    .is_some_and(|t| t.starts_with(char::is_whitespace))
                    || is(first, TEXT, "s")
                    || is(first, TEXT, "tab")
            });
        if !spaced {
            walk.builder.text(" ", &Look::default(), None);
        }
        walk.item_marked = true;
        let mut first = true;
        for n in body.children().filter(Node::is_element) {
            // The label begins the note's first paragraph.
            if first && is(n, TEXT, "p") {
                first = false;
                let style = n.attribute((TEXT, "style-name"));
                let base = walk
                    .styles
                    .default_text
                    .over(walk.styles.text_of("paragraph", style));
                let mut boxes = Boxes::new();
                let mut space = !spaced;
                let mut inner = Notes::new();
                walk.inline(n, base, None, &mut boxes, &mut inner, &mut space)?;
                walk.builder.end_paragraph();
                continue;
            }
            first = false;
            let mut inner = Notes::new();
            walk.blocks_of(n, &mut inner)?;
        }
        walk.builder.end_paragraph();
    }
    let part = walk.builder.kept_part();
    let words = walk.builder.words;
    let (blocks, truncated) = std::mem::take(&mut walk.builder).into_blocks();
    Ok(Read {
        blocks,
        truncated,
        part,
        parts,
        words,
        details,
    })
}

impl Walk<'_> {
    /// One element of a note's body, read as the body's own would be.
    fn blocks_of<'a, 'i>(
        &mut self,
        node: Node<'a, 'i>,
        notes: &mut Notes<'a, 'i>,
    ) -> anyhow::Result<()> {
        if is(node, TEXT, "p") || is(node, TEXT, "h") {
            self.paragraph(node, notes)
        } else if is(node, TEXT, "list") {
            self.list(node, false, notes)
        } else if is(node, TABLE, "table") {
            self.table(node, false, notes)
        } else {
            Ok(())
        }
    }
}

/// What an `OpenDocument` file says about itself, for Properties.
pub(crate) fn about<S: FileSource + ?Sized>(
    file: &S,
    cancel: &CancelToken,
) -> anyhow::Result<Vec<(&'static str, String)>> {
    Ok(details(&mut Package::open(file, cancel)?, cancel))
}

/// What the document says about itself: its title, author and the pages
/// its application counted.
fn details(package: &mut Package, cancel: &CancelToken) -> Vec<(&'static str, String)> {
    let mut facts = Vec::new();
    let Ok(Some(xml)) = package.text("meta.xml", "Properties", cancel) else {
        return facts;
    };
    let Ok(doc) = package::xml(&xml) else {
        return facts;
    };
    let Some(meta) = child(doc.root_element(), OFFICE, "meta") else {
        return facts;
    };
    let text = |ns, local| {
        child(meta, ns, local)
            .and_then(|n| n.text())
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_owned)
    };
    if let Some(title) = text(DC, "title") {
        facts.push(("Title", title));
    }
    if let Some(author) = text(META, "initial-creator").or_else(|| text(DC, "creator")) {
        facts.push(("Author", author));
    }
    if let Some(subject) = text(DC, "subject") {
        facts.push(("Subject", subject));
    }
    let keywords: Vec<&str> = meta
        .children()
        .filter(|n| is(*n, META, "keyword"))
        .filter_map(|n| n.text())
        .collect();
    if !keywords.is_empty() {
        facts.push(("Keywords", keywords.join(", ")));
    }
    if let Some(pages) =
        child(meta, META, "document-statistic").and_then(|s| s.attribute((META, "page-count")))
    {
        facts.push(("Pages", pages.to_owned()));
    }
    if let Some(generator) = text(META, "generator") {
        facts.push(("Application", generator));
    }
    facts
}
