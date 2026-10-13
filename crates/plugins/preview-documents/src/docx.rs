//! A Word document (`.docx`, `.docm`, `.dotx`, `.dotm`) read as text:
//! its body walked into blocks with the shape its styles and numbering
//! give each paragraph. Transitional and Strict namespaces read alike.
use crate::blocks::{self, Builder, Look, Read, Shape, is_code, is_quote};
use crate::package::{self, Package};
use roxmltree::Node;
use skwir_core::CancelToken;
use skwir_core::inspection::{Align, Script};
use skwir_platform::inspection::{self, Numbering, limits};
use std::collections::{HashMap, HashSet};
use std::path::Path;

const W: [&str; 2] = [
    "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
    "http://purl.oclc.org/ooxml/wordprocessingml/main",
];
const R: [&str; 2] = [
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
    "http://purl.oclc.org/ooxml/officeDocument/relationships",
];
const A: [&str; 2] = [
    "http://schemas.openxmlformats.org/drawingml/2006/main",
    "http://purl.oclc.org/ooxml/drawingml/main",
];
const WP: [&str; 2] = [
    "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing",
    "http://purl.oclc.org/ooxml/drawingml/wordprocessingDrawing",
];
const M: [&str; 2] = [
    "http://schemas.openxmlformats.org/officeDocument/2006/math",
    "http://purl.oclc.org/ooxml/officeDocument/math",
];
const V: [&str; 1] = ["urn:schemas-microsoft-com:vml"];
const MC: [&str; 1] = ["http://schemas.openxmlformats.org/markup-compatibility/2006"];

/// Whether `node` is element `local` of namespace `ns`.
fn is(node: Node<'_, '_>, ns: &[&str], local: &str) -> bool {
    node.is_element()
        && node.tag_name().name() == local
        && node.tag_name().namespace().is_some_and(|n| ns.contains(&n))
}

/// The first child element `local` of namespace `ns`.
fn child<'a, 'i>(node: Node<'a, 'i>, ns: &[&str], local: &str) -> Option<Node<'a, 'i>> {
    node.children().find(|c| is(*c, ns, local))
}

/// Attribute `local` of namespace `ns`.
fn attr<'a>(node: Node<'a, '_>, ns: &[&str], local: &str) -> Option<&'a str> {
    node.attributes()
        .find(|a| a.name() == local && a.namespace().is_some_and(|n| ns.contains(&n)))
        .map(|a| a.value())
}

/// A `w:val`.
fn val<'a>(node: Node<'a, '_>) -> Option<&'a str> {
    attr(node, &W, "val")
}

/// An on/off property: present and not turned off.
fn on(node: Node<'_, '_>) -> bool {
    !matches!(val(node), Some("0" | "false" | "off"))
}

/// The run properties one level of styling sets; `None` leaves the
/// level below as it was.
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
    /// These, with what `top` sets on top of them.
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

    fn read(rpr: Option<Node<'_, '_>>) -> Props {
        let mut props = Props::default();
        let Some(rpr) = rpr else {
            return props;
        };
        for p in rpr.children().filter(Node::is_element) {
            if !p.tag_name().namespace().is_some_and(|n| W.contains(&n)) {
                continue;
            }
            match p.tag_name().name() {
                "b" => props.bold = Some(on(p)),
                "i" => props.italic = Some(on(p)),
                "u" => props.underline = Some(!matches!(val(p), Some("none") | None)),
                "strike" | "dstrike" => props.strike = Some(on(p)),
                "vertAlign" => {
                    props.script = Some(match val(p) {
                        Some("superscript") => Script::Superscript,
                        Some("subscript") => Script::Subscript,
                        _ => Script::Baseline,
                    });
                }
                "smallCaps" => props.small_caps = Some(on(p)),
                "caps" => props.caps = Some(on(p)),
                "vanish" | "specVanish" => props.hidden = Some(on(p)),
                _ => {}
            }
        }
        props
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

/// The paragraph properties one level sets.
#[derive(Clone, Debug, Default)]
struct Para {
    align: Option<Align>,
    /// Left indent, in twentieths of a point.
    indent: Option<i64>,
    /// 0..=8 a heading's level, 9 body text.
    outline: Option<u8>,
    num_id: Option<String>,
    level: Option<u32>,
}

impl Para {
    fn over(&self, top: &Para) -> Para {
        Para {
            align: top.align.or(self.align),
            indent: top.indent.or(self.indent),
            outline: top.outline.or(self.outline),
            num_id: top.num_id.clone().or_else(|| self.num_id.clone()),
            level: top.level.or(self.level),
        }
    }

    fn read(ppr: Option<Node<'_, '_>>) -> Para {
        let mut para = Para::default();
        let Some(ppr) = ppr else {
            return para;
        };
        if let Some(jc) = child(ppr, &W, "jc") {
            para.align = Some(match val(jc) {
                Some("center") => Align::Center,
                Some("right" | "end") => Align::End,
                _ => Align::Start,
            });
        }
        if let Some(ind) = child(ppr, &W, "ind") {
            para.indent = attr(ind, &W, "left")
                .or_else(|| attr(ind, &W, "start"))
                .and_then(|v| v.parse().ok());
        }
        if let Some(outline) = child(ppr, &W, "outlineLvl") {
            para.outline = val(outline).and_then(|v| v.parse().ok());
        }
        if let Some(num) = child(ppr, &W, "numPr") {
            para.num_id = child(num, &W, "numId").and_then(val).map(str::to_owned);
            para.level = child(num, &W, "ilvl")
                .and_then(val)
                .and_then(|v| v.parse().ok());
        }
        para
    }
}

struct Style {
    /// The style's name, lowercased: Word's built-in styles are known by it
    /// whatever language the document was written in.
    name: String,
    based_on: Option<String>,
    run: Props,
    para: Para,
}

#[derive(Default)]
struct Styles {
    by_id: HashMap<String, Style>,
    default_paragraph: Option<String>,
    run: Props,
    para: Para,
}

impl Styles {
    fn read(xml: Option<&str>) -> Styles {
        let mut styles = Styles::default();
        let Some(doc) = xml.and_then(|xml| package::xml(xml).ok()) else {
            return styles;
        };
        let root = doc.root_element();
        if let Some(defaults) = child(root, &W, "docDefaults") {
            styles.run =
                Props::read(child(defaults, &W, "rPrDefault").and_then(|d| child(d, &W, "rPr")));
            styles.para =
                Para::read(child(defaults, &W, "pPrDefault").and_then(|d| child(d, &W, "pPr")));
        }
        for style in root.children().filter(|c| is(*c, &W, "style")) {
            let Some(id) = attr(style, &W, "styleId") else {
                continue;
            };
            if attr(style, &W, "type") == Some("paragraph")
                && attr(style, &W, "default").is_some_and(|d| matches!(d, "1" | "true" | "on"))
            {
                styles.default_paragraph = Some(id.to_owned());
            }
            styles.by_id.insert(
                id.to_owned(),
                Style {
                    name: child(style, &W, "name")
                        .and_then(val)
                        .unwrap_or(id)
                        .to_lowercase(),
                    based_on: child(style, &W, "basedOn").and_then(val).map(str::to_owned),
                    run: Props::read(child(style, &W, "rPr")),
                    para: Para::read(child(style, &W, "pPr")),
                },
            );
        }
        styles
    }

    /// Style `id` and those it is based on, itself first.
    fn chain(&self, id: Option<&str>) -> Vec<&Style> {
        let mut chain: Vec<&Style> = Vec::new();
        let mut next = id;
        while let Some(id) = next {
            let Some(style) = self.by_id.get(id) else {
                break;
            };
            if chain.len() == limits::NESTING || chain.iter().any(|s| std::ptr::eq(*s, style)) {
                break;
            }
            chain.push(style);
            next = style.based_on.as_deref();
        }
        chain
    }

    fn run_of(&self, id: Option<&str>) -> Props {
        self.chain(id)
            .iter()
            .rev()
            .fold(Props::default(), |props, style| props.over(style.run))
    }

    fn para_of(&self, id: Option<&str>) -> Para {
        self.chain(id)
            .iter()
            .rev()
            .fold(self.para.clone(), |para, style| para.over(&style.para))
    }

    /// Whether style `id`, or one it is based on, is named as `test` says.
    fn named(&self, id: Option<&str>, test: impl Fn(&str) -> bool) -> bool {
        self.chain(id).iter().any(|style| test(&style.name))
    }

    /// A heading's level by its style's name: Word's built-in headings,
    /// nearest first, and its title, which a style based on it (an author
    /// line, a subtitle) is not.
    fn heading(&self, id: Option<&str>) -> Option<u8> {
        let chain = self.chain(id);
        if chain.first().is_some_and(|style| style.name == "title") {
            return Some(1);
        }
        chain.iter().find_map(|style| {
            style
                .name
                .strip_prefix("heading ")
                .and_then(|n| n.parse::<u8>().ok())
                .filter(|n| (1..=9).contains(n))
        })
    }
}

/// How one level of a list numbers its items.
#[derive(Clone, Debug)]
struct Level {
    start: u32,
    format: Option<Numbering>,
    /// What the item says: `%1.` with the counts of the levels named.
    text: String,
    /// Every level counts in decimal (legal numbering, `1.1.1`).
    legal: bool,
}

impl Level {
    fn read(lvl: Node<'_, '_>) -> Level {
        let format = child(lvl, &W, "numFmt").and_then(val).unwrap_or("decimal");
        Level {
            start: child(lvl, &W, "start")
                .and_then(val)
                .and_then(|v| v.parse().ok())
                .unwrap_or(1),
            format: match format {
                "bullet" => None,
                "decimalZero" => Some(Numbering::DecimalZero),
                "lowerLetter" => Some(Numbering::LowerLetter),
                "upperLetter" => Some(Numbering::UpperLetter),
                "lowerRoman" => Some(Numbering::LowerRoman),
                "upperRoman" => Some(Numbering::UpperRoman),
                "none" => Some(Numbering::None),
                _ => Some(Numbering::Decimal),
            },
            text: child(lvl, &W, "lvlText")
                .and_then(val)
                .unwrap_or_default()
                .to_owned(),
            legal: child(lvl, &W, "isLgl").is_some_and(on),
        }
    }
}

const LEVELS: usize = 9;

#[derive(Default)]
struct Abstract {
    levels: Vec<Option<Level>>,
    /// Defined by a numbering style instead (`numStyleLink`).
    style_link: Option<String>,
}

#[derive(Default)]
struct Num {
    abstract_id: String,
    /// Per level: where it starts again, and a level that replaces it.
    starts: HashMap<u32, u32>,
    levels: HashMap<u32, Level>,
}

/// Lists and how far each has counted. Word counts an abstract list's
/// items across every list that uses it, until one says to start again.
#[derive(Default)]
struct Lists {
    abstracts: HashMap<String, Abstract>,
    nums: HashMap<String, Num>,
    counts: HashMap<String, [Option<u32>; LEVELS]>,
    started: HashSet<String>,
}

impl Lists {
    fn read(xml: Option<&str>) -> Lists {
        let mut numbering = Lists::default();
        let Some(doc) = xml.and_then(|xml| package::xml(xml).ok()) else {
            return numbering;
        };
        for node in doc.root_element().children() {
            if is(node, &W, "abstractNum") {
                let Some(id) = attr(node, &W, "abstractNumId") else {
                    continue;
                };
                let mut levels = vec![None; LEVELS];
                for lvl in node.children().filter(|c| is(*c, &W, "lvl")) {
                    if let Some(i) = attr(lvl, &W, "ilvl").and_then(|v| v.parse::<usize>().ok())
                        && i < LEVELS
                    {
                        levels[i] = Some(Level::read(lvl));
                    }
                }
                numbering.abstracts.insert(
                    id.to_owned(),
                    Abstract {
                        levels,
                        style_link: child(node, &W, "numStyleLink")
                            .and_then(val)
                            .map(str::to_owned),
                    },
                );
            } else if is(node, &W, "num") {
                let Some(id) = attr(node, &W, "numId") else {
                    continue;
                };
                let mut num = Num {
                    abstract_id: child(node, &W, "abstractNumId")
                        .and_then(val)
                        .unwrap_or_default()
                        .to_owned(),
                    ..Num::default()
                };
                for over in node.children().filter(|c| is(*c, &W, "lvlOverride")) {
                    let Some(i) = attr(over, &W, "ilvl").and_then(|v| v.parse().ok()) else {
                        continue;
                    };
                    if let Some(start) = child(over, &W, "startOverride")
                        .and_then(val)
                        .and_then(|v| v.parse().ok())
                    {
                        num.starts.insert(i, start);
                    }
                    if let Some(lvl) = child(over, &W, "lvl") {
                        num.levels.insert(i, Level::read(lvl));
                    }
                }
                numbering.nums.insert(id.to_owned(), num);
            }
        }
        numbering
    }

    /// The abstract list `num` counts in, through a numbering style.
    fn abstract_of(&self, num: &Num, styles: &Styles) -> Option<String> {
        let id = &num.abstract_id;
        let link = self.abstracts.get(id)?.style_link.as_deref();
        let Some(link) = link else {
            return Some(id.clone());
        };
        let linked = styles.para_of(Some(link)).num_id?;
        Some(self.nums.get(&linked)?.abstract_id.clone())
    }

    /// The marker of the next item of list `num_id` at `level`, counted.
    fn next(&mut self, num_id: &str, level: u32, styles: &Styles) -> Option<String> {
        let num = self.nums.get(num_id)?;
        let abstract_id = self.abstract_of(num, styles)?;
        let level = (level as usize).min(LEVELS - 1);
        let levels: Vec<Option<Level>> = (0..LEVELS)
            .map(|i| {
                num.levels
                    .get(&(i as u32))
                    .cloned()
                    .or_else(|| self.abstracts.get(&abstract_id)?.levels.get(i)?.clone())
            })
            .collect();
        let definition = levels[level].clone()?;
        let counts = self.counts.entry(abstract_id).or_default();
        if self.started.insert(num_id.to_owned()) {
            for (&at, &start) in &num.starts {
                if let Some(count) = counts.get_mut(at as usize) {
                    *count = Some(start.saturating_sub(1));
                }
            }
        }
        counts[level] = Some(counts[level].map_or(definition.start, |n| n.saturating_add(1)));
        for deeper in &mut counts[level + 1..] {
            *deeper = None;
        }
        let Some(_) = definition.format else {
            return Some(blocks::bullet(&definition.text));
        };
        let mut marker = String::new();
        let mut rest = definition.text.as_str();
        while let Some(at) = rest.find('%') {
            marker.push_str(&rest[..at]);
            let digit = rest[at + 1..].chars().next();
            if let Some(d) = digit
                .and_then(|d| d.to_digit(10))
                .filter(|d| (1..=9).contains(d))
            {
                let i = d as usize - 1;
                let named = levels[i].as_ref();
                let n = counts[i].unwrap_or_else(|| named.map_or(1, |l| l.start));
                let format = if definition.legal {
                    Numbering::Decimal
                } else {
                    named.and_then(|l| l.format).unwrap_or_default()
                };
                marker.push_str(&format.number(n));
                rest = &rest[at + 2..];
            } else {
                marker.push('%');
                rest = &rest[at + 1..];
            }
        }
        marker.push_str(rest);
        Some(marker)
    }
}

/// A part's relationships: id → (target, whether it is outside the package).
type Rels = HashMap<String, (String, bool)>;

/// The relationships of part `name` (`word/document.xml`).
fn rels(package: &mut Package, name: &str, cancel: &CancelToken) -> anyhow::Result<Rels> {
    let (dir, file) = name.rsplit_once('/').unwrap_or(("", name));
    let rels_name = if dir.is_empty() {
        format!("_rels/{file}.rels")
    } else {
        format!("{dir}/_rels/{file}.rels")
    };
    let mut map = Rels::new();
    let Some(xml) = package.text(&rels_name, "Relationships", cancel)? else {
        return Ok(map);
    };
    let doc = package::xml(&xml)?;
    for rel in doc
        .root_element()
        .children()
        .filter(|c| c.is_element() && c.tag_name().name() == "Relationship")
    {
        if let (Some(id), Some(target)) = (rel.attribute("Id"), rel.attribute("Target")) {
            let external = rel.attribute("TargetMode") == Some("External");
            let target = if external {
                target.to_owned()
            } else {
                package::resolve(dir, target).unwrap_or_default()
            };
            map.insert(id.to_owned(), (target, external));
        }
    }
    Ok(map)
}

/// The target of the relationship of type `kind` (its last segment).
fn related(package: &mut Package, name: &str, kind: &str, cancel: &CancelToken) -> Option<String> {
    let (dir, file) = name.rsplit_once('/').unwrap_or(("", name));
    let rels_name = if dir.is_empty() {
        format!("_rels/{file}.rels")
    } else {
        format!("{dir}/_rels/{file}.rels")
    };
    let xml = package.text(&rels_name, "Relationships", cancel).ok()??;
    let doc = package::xml(&xml).ok()?;
    doc.root_element()
        .children()
        .filter(|c| c.is_element() && c.tag_name().name() == "Relationship")
        .find(|rel| {
            rel.attribute("Type")
                .is_some_and(|t| t.rsplit('/').next() == Some(kind))
        })
        .and_then(|rel| package::resolve(dir, rel.attribute("Target")?))
}

/// A part being read: its relationships, for its links and pictures.
struct Part {
    rels: Rels,
}

/// A field (`HYPERLINK`, `PAGE`, a table of contents) between its
/// `fldChar`s: its instruction is skipped, its result read.
struct Field {
    instruction: String,
    result: bool,
    link: Option<String>,
}

/// Where a `HYPERLINK` field leads: `HYPERLINK "url"` or `\l "bookmark"`.
fn field_link(instruction: &str) -> Option<String> {
    let mut words = instruction.split_whitespace();
    if !words.next()?.eq_ignore_ascii_case("HYPERLINK") {
        return None;
    }
    let rest: String = words.collect::<Vec<_>>().join(" ");
    let quoted = |s: &str| -> Option<String> {
        let start = s.find('"')? + 1;
        let end = start + s[start..].find('"')?;
        Some(s[start..end].to_owned())
    };
    if let Some(at) = rest.find("\\l") {
        return quoted(&rest[at..]).map(|anchor| format!("#{anchor}"));
    }
    quoted(&rest).or_else(|| rest.split_whitespace().next().map(str::to_owned))
}

struct Walk<'p> {
    builder: Builder,
    styles: Styles,
    numbering: Lists,
    package: &'p mut Package,
    cancel: &'p CancelToken,
    fields: Vec<Field>,
    /// Footnote and endnote references seen so far, numbered in order.
    footnotes: u32,
    endnotes: u32,
    labels: HashMap<String, String>,
    /// The label of the note being written, which its own mark says.
    note_label: Option<String>,
    visited: usize,
    depth: usize,
    /// How deep the walk is in the XML: see `nested`.
    nest: usize,
}

type Boxes<'a, 'i> = Vec<Node<'a, 'i>>;

impl Walk<'_> {
    /// Walk one element deeper: past `limits::XML_DEPTH`, what the file
    /// nests is left out (`sdt` in `sdt`…, ten thousand of them in a
    /// small part would overflow the stack).
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

    /// Paragraphs, tables and what wraps them, in a body, a cell, a note
    /// or a text box. `top`: the body's own, where a part may be cut.
    fn blocks(&mut self, node: Node<'_, '_>, part: &Part, top: bool) -> anyhow::Result<()> {
        self.nested(|walk| walk.blocks_in(node, part, top))
    }

    fn blocks_in(&mut self, node: Node<'_, '_>, part: &Part, top: bool) -> anyhow::Result<()> {
        for n in node.children().filter(Node::is_element) {
            self.check()?;
            if is(n, &W, "p") {
                self.paragraph(n, part)?;
                if top {
                    self.builder.boundary();
                }
            } else if is(n, &W, "tbl") {
                self.table(n, part, top)?;
            } else if is(n, &W, "sdt") {
                if let Some(content) = child(n, &W, "sdtContent") {
                    self.blocks(content, part, top)?;
                }
            } else if is(n, &W, "customXml") {
                self.blocks(n, part, top)?;
            } else if is(n, &W, "bookmarkStart") {
                if let Some(name) = attr(n, &W, "name") {
                    self.builder.anchor(name);
                }
            } else if is(n, &MC, "AlternateContent")
                && let Some(choice) = child(n, &MC, "Choice")
            {
                self.blocks(choice, part, top)?;
            }
        }
        Ok(())
    }

    fn paragraph(&mut self, p: Node<'_, '_>, part: &Part) -> anyhow::Result<()> {
        let ppr = child(p, &W, "pPr");
        let style = ppr
            .and_then(|ppr| child(ppr, &W, "pStyle"))
            .and_then(val)
            .map(str::to_owned)
            .or_else(|| self.styles.default_paragraph.clone());
        let style = style.as_deref();
        let effective = self.styles.para_of(style).over(&Para::read(ppr));
        let heading = match Para::read(ppr).outline {
            Some(level) => (level < 9).then_some(level + 1),
            None => self
                .styles
                .heading(style)
                .or_else(|| effective.outline.filter(|l| *l < 9).map(|l| l + 1)),
        }
        .map_or(0, |level| level.min(6));
        let mut shape = Shape {
            heading,
            align: effective.align.unwrap_or_default(),
            indent: effective.indent.map_or(0., |twips| twips as f32 / 240.),
            quote: self.styles.named(style, is_quote),
            code: self.styles.named(style, is_code),
            ..Shape::default()
        };
        if let Some(num_id) = effective.num_id.as_deref().filter(|id| *id != "0") {
            let level = effective.level.unwrap_or(0);
            if let Some(marker) = self.numbering.next(num_id, level, &self.styles) {
                shape.list_depth = level + 1;
                shape.marker = (!marker.is_empty()).then_some(marker);
            }
        }
        self.builder.paragraph(shape);
        let base = self.styles.run.over(self.styles.run_of(style));
        let mut boxes = Boxes::new();
        self.inline(p, part, base, None, &mut boxes)?;
        self.builder.end_paragraph();
        self.text_boxes(&boxes, part)
    }

    /// Text boxes stand after the paragraph they are anchored in.
    fn text_boxes(&mut self, boxes: &[Node<'_, '_>], part: &Part) -> anyhow::Result<()> {
        if boxes.is_empty() || self.depth >= limits::NESTING {
            return Ok(());
        }
        self.depth += 1;
        for content in boxes {
            self.blocks(*content, part, false)?;
        }
        self.depth -= 1;
        Ok(())
    }

    /// The link the field being read leads to.
    fn field_link(&self) -> Option<String> {
        self.fields
            .iter()
            .rev()
            .find(|f| f.result && f.link.is_some())
            .and_then(|f| f.link.clone())
    }

    /// Whether the text met now is a field's instruction, not shown.
    fn in_instruction(&self) -> bool {
        self.fields.last().is_some_and(|f| !f.result)
    }

    fn inline<'a, 'i>(
        &mut self,
        node: Node<'a, 'i>,
        part: &Part,
        base: Props,
        link: Option<&str>,
        boxes: &mut Boxes<'a, 'i>,
    ) -> anyhow::Result<()> {
        self.nested(|walk| walk.inline_in(node, part, base, link, boxes))
    }

    fn inline_in<'a, 'i>(
        &mut self,
        node: Node<'a, 'i>,
        part: &Part,
        base: Props,
        link: Option<&str>,
        boxes: &mut Boxes<'a, 'i>,
    ) -> anyhow::Result<()> {
        for n in node.children().filter(Node::is_element) {
            self.check()?;
            if is(n, &W, "r") {
                self.run(n, part, base, link, boxes)?;
            } else if is(n, &W, "hyperlink") {
                let target = attr(n, &R, "id")
                    .and_then(|id| part.rels.get(id))
                    .filter(|(_, external)| *external)
                    .map(|(target, _)| target.clone())
                    .or_else(|| attr(n, &W, "anchor").map(|a| format!("#{a}")));
                self.inline(n, part, base, target.as_deref().or(link), boxes)?;
            } else if is(n, &W, "fldSimple") {
                let target = attr(n, &W, "instr").and_then(field_link);
                self.inline(n, part, base, target.as_deref().or(link), boxes)?;
            } else if is(n, &W, "sdt") {
                if let Some(content) = child(n, &W, "sdtContent") {
                    self.inline(content, part, base, link, boxes)?;
                }
            } else if ["ins", "moveTo", "smartTag", "customXml", "dir", "bdo"]
                .iter()
                .any(|name| is(n, &W, name))
            {
                self.inline(n, part, base, link, boxes)?;
            } else if is(n, &W, "bookmarkStart") {
                if let Some(name) = attr(n, &W, "name") {
                    self.builder.anchor(name);
                }
            } else if is(n, &M, "oMath") || is(n, &M, "oMathPara") {
                let text: String = n
                    .descendants()
                    .filter(|n| is(*n, &M, "t"))
                    .filter_map(|n| n.text())
                    .collect();
                if !self.in_instruction() {
                    self.builder.text(&text, &base.look(false), link);
                }
            } else if is(n, &MC, "AlternateContent")
                && let Some(choice) = child(n, &MC, "Choice")
            {
                self.inline(choice, part, base, link, boxes)?;
            }
        }
        Ok(())
    }

    fn run<'a, 'i>(
        &mut self,
        r: Node<'a, 'i>,
        part: &Part,
        base: Props,
        link: Option<&str>,
        boxes: &mut Boxes<'a, 'i>,
    ) -> anyhow::Result<()> {
        let rpr = child(r, &W, "rPr");
        let style = rpr.and_then(|rpr| child(rpr, &W, "rStyle")).and_then(val);
        let props = base.over(self.styles.run_of(style)).over(Props::read(rpr));
        let look = props.look(self.styles.named(style, is_code));
        self.run_content(r, part, &look, link, boxes)
    }

    fn run_content<'a, 'i>(
        &mut self,
        r: Node<'a, 'i>,
        part: &Part,
        look: &Look,
        link: Option<&str>,
        boxes: &mut Boxes<'a, 'i>,
    ) -> anyhow::Result<()> {
        for item in r.children().filter(Node::is_element) {
            if is(item, &W, "fldChar") {
                match attr(item, &W, "fldCharType") {
                    Some("begin") if self.fields.len() < limits::NESTING => {
                        self.fields.push(Field {
                            instruction: String::new(),
                            result: false,
                            link: None,
                        });
                    }
                    Some("separate") => {
                        if let Some(field) = self.fields.last_mut() {
                            field.result = true;
                            field.link = field_link(&field.instruction);
                        }
                    }
                    Some("end") => {
                        self.fields.pop();
                    }
                    _ => {}
                }
                continue;
            }
            if is(item, &W, "instrText") {
                if let Some(field) = self.fields.last_mut()
                    && !field.result
                    && field.instruction.len() < limits::LINE_BYTES
                {
                    field.instruction.push_str(item.text().unwrap_or_default());
                }
                continue;
            }
            if self.in_instruction() {
                continue;
            }
            let field = self.field_link();
            let link = link.or(field.as_deref());
            let name = if item.tag_name().namespace().is_some_and(|n| W.contains(&n)) {
                item.tag_name().name()
            } else if is(item, &MC, "AlternateContent") {
                // Nested as deep as a file says: bounded as every walk is.
                if let Some(choice) = child(item, &MC, "Choice") {
                    self.nested(|this| this.run_content(choice, part, look, link, boxes))?;
                }
                continue;
            } else {
                continue;
            };
            match name {
                "t" => self
                    .builder
                    .text(item.text().unwrap_or_default(), look, link),
                "tab" | "ptab" => self.builder.text("\t", look, link),
                "br" => {
                    if !matches!(attr(item, &W, "type"), Some("page" | "column")) {
                        self.builder.line_break();
                    }
                }
                "cr" => self.builder.line_break(),
                "noBreakHyphen" => self.builder.text("\u{2011}", look, link),
                "sym" => {
                    if let Some(c) = attr(item, &W, "char")
                        .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                        .and_then(char::from_u32)
                    {
                        self.builder
                            .text(&blocks::bullet(&c.to_string()), look, link);
                    }
                }
                "drawing" => self.drawing(item, part, boxes),
                "pict" | "object" => self.vml(item, part, boxes),
                "footnoteReference" | "endnoteReference" => {
                    let foot = name == "footnoteReference";
                    if let Some(id) = attr(item, &W, "id") {
                        // A custom mark is the run that follows.
                        if attr(item, &W, "customMarkFollows").is_some_and(|v| v != "0") {
                            continue;
                        }
                        let (kind, label) = if foot {
                            self.footnotes += 1;
                            ("footnote", Numbering::Decimal.number(self.footnotes))
                        } else {
                            self.endnotes += 1;
                            ("endnote", Numbering::LowerRoman.number(self.endnotes))
                        };
                        let target = format!("{kind}-{id}");
                        self.labels.insert(target.clone(), label.clone());
                        self.builder.reference(&label, &target);
                    }
                }
                "footnoteRef" | "endnoteRef" => {
                    if let Some(label) = self.note_label.clone() {
                        let raised = Look {
                            script: Script::Superscript,
                            ..Look::default()
                        };
                        self.builder.text(&label, &raised, None);
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// A `DrawingML` object: its pictures, and its text boxes for after the
    /// paragraph.
    fn drawing<'a, 'i>(&mut self, node: Node<'a, 'i>, part: &Part, boxes: &mut Boxes<'a, 'i>) {
        let alt = node
            .descendants()
            .find(|n| is(*n, &WP, "docPr"))
            .and_then(|d| d.attribute("descr").or_else(|| d.attribute("title")))
            .unwrap_or_default();
        let mut alt = Some(alt);
        for blip in node.descendants().filter(|n| is(*n, &A, "blip")) {
            let picture = attr(blip, &R, "embed")
                .and_then(|id| part.rels.get(id))
                .filter(|(_, external)| !*external)
                .and_then(|(target, _)| {
                    if self.builder.shows_this_part() {
                        self.package.picture(target, self.cancel)
                    } else {
                        Package::unread_picture(target)
                    }
                });
            self.builder
                .picture(alt.take().unwrap_or_default(), picture);
        }
        boxes.extend(node.descendants().filter(|n| is(*n, &W, "txbxContent")));
    }

    /// A VML picture or shape, as older documents and OLE objects keep them.
    fn vml<'a, 'i>(&mut self, node: Node<'a, 'i>, part: &Part, boxes: &mut Boxes<'a, 'i>) {
        for image in node.descendants().filter(|n| is(*n, &V, "imagedata")) {
            let alt = image.attribute(("urn:schemas-microsoft-com:office:office", "title"));
            let picture = attr(image, &R, "id")
                .and_then(|id| part.rels.get(id))
                .filter(|(_, external)| !*external)
                .and_then(|(target, _)| {
                    if self.builder.shows_this_part() {
                        self.package.picture(target, self.cancel)
                    } else {
                        Package::unread_picture(target)
                    }
                });
            self.builder.picture(alt.unwrap_or_default(), picture);
        }
        boxes.extend(node.descendants().filter(|n| is(*n, &W, "txbxContent")));
    }

    fn table(&mut self, tbl: Node<'_, '_>, part: &Part, top: bool) -> anyhow::Result<()> {
        for row in rows(tbl) {
            self.check()?;
            let header = child(row, &W, "trPr")
                .and_then(|pr| child(pr, &W, "tblHeader"))
                .is_some_and(on);
            self.builder.begin_row(header);
            let mut columns = 0;
            for cell in cells(row) {
                let pr = child(cell, &W, "tcPr");
                let span = pr
                    .and_then(|pr| child(pr, &W, "gridSpan"))
                    .and_then(val)
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(1)
                    .max(1);
                let merged = pr
                    .and_then(|pr| child(pr, &W, "vMerge"))
                    .is_some_and(|m| val(m) != Some("restart"));
                columns += span;
                if columns > limits::TABLE_COLUMNS {
                    break;
                }
                self.builder.cell();
                if !merged {
                    self.cell(cell, part)?;
                }
                for _ in 1..span {
                    self.builder.cell();
                }
            }
            self.builder.end_row();
            if top {
                self.builder.boundary();
            }
        }
        Ok(())
    }

    /// A cell's paragraphs, each a line of it; a table inside is read a
    /// row to a line.
    fn cell(&mut self, cell: Node<'_, '_>, part: &Part) -> anyhow::Result<()> {
        self.nested(|walk| walk.cell_in(cell, part))
    }

    fn cell_in(&mut self, cell: Node<'_, '_>, part: &Part) -> anyhow::Result<()> {
        for n in cell.children().filter(Node::is_element) {
            self.check()?;
            if is(n, &W, "p") {
                self.paragraph(n, part)?;
            } else if is(n, &W, "tbl") && self.depth < limits::NESTING {
                self.depth += 1;
                for row in rows(n) {
                    for (i, cell) in cells(row).into_iter().enumerate() {
                        if i > 0 {
                            self.builder.text(" · ", &Look::default(), None);
                        }
                        self.cell(cell, part)?;
                    }
                }
                self.depth -= 1;
            } else if is(n, &W, "sdt")
                && let Some(content) = child(n, &W, "sdtContent")
            {
                self.cell(content, part)?;
            }
        }
        Ok(())
    }
}

/// A table's rows, through what wraps them.
fn rows<'a, 'i>(tbl: Node<'a, 'i>) -> Vec<Node<'a, 'i>> {
    wrapped(tbl, "tr")
}

/// A row's cells, through what wraps them.
fn cells<'a, 'i>(row: Node<'a, 'i>) -> Vec<Node<'a, 'i>> {
    wrapped(row, "tc")
}

fn wrapped<'a, 'i>(node: Node<'a, 'i>, local: &str) -> Vec<Node<'a, 'i>> {
    let mut found = Vec::new();
    wrapped_in(node, local, 0, &mut found);
    found
}

fn wrapped_in<'a, 'i>(
    node: Node<'a, 'i>,
    local: &str,
    depth: usize,
    found: &mut Vec<Node<'a, 'i>>,
) {
    if depth >= limits::XML_DEPTH {
        return;
    }
    for n in node.children().filter(Node::is_element) {
        if is(n, &W, local) {
            found.push(n);
        } else if is(n, &W, "sdt") {
            if let Some(content) = child(n, &W, "sdtContent") {
                wrapped_in(content, local, depth + 1, found);
            }
        } else if is(n, &W, "customXml") {
            wrapped_in(n, local, depth + 1, found);
        }
    }
}

pub(crate) fn read(path: &Path, wanted: u64, cancel: &CancelToken) -> anyhow::Result<Read> {
    let mut package = Package::open(path, cancel)?;
    let main = related(&mut package, "", "officeDocument", cancel)
        .unwrap_or_else(|| "word/document.xml".into());
    let source = package
        .text(&main, "The document's text", cancel)?
        .ok_or_else(|| anyhow::anyhow!("This is not a Word document: it has no text part"))?;
    let doc = package::xml(&source)?;
    let styles_xml = match related(&mut package, &main, "styles", cancel) {
        Some(name) => package.text(&name, "Styles", cancel)?,
        None => None,
    };
    let numbering_xml = match related(&mut package, &main, "numbering", cancel) {
        Some(name) => package.text(&name, "Numbering", cancel)?,
        None => None,
    };
    let styles = Styles::read(styles_xml.as_deref());
    let numbering = Lists::read(numbering_xml.as_deref());
    let body = child(doc.root_element(), &W, "body")
        .ok_or_else(|| anyhow::anyhow!("This Word document has no body"))?;
    let part = Part {
        rels: rels(&mut package, &main, cancel)?,
    };
    let notes = [
        (
            "footnote",
            related(&mut package, &main, "footnotes", cancel),
        ),
        ("endnote", related(&mut package, &main, "endnotes", cancel)),
    ];
    let details = details(&mut package, cancel);
    let mut walk = Walk {
        builder: Builder::new(wanted),
        styles,
        numbering,
        package: &mut package,
        cancel,
        fields: Vec::new(),
        footnotes: 0,
        endnotes: 0,
        labels: HashMap::new(),
        note_label: None,
        visited: 0,
        depth: 0,
        nest: 0,
    };
    walk.blocks(body, &part, true)?;
    let (referenced, parts) = walk.builder.body_end();
    // Asked past the end (the document shrank): the last part, read again
    // with its pictures read.
    if wanted >= parts && parts > 0 {
        drop(walk);
        return read(path, parts - 1, cancel);
    }
    if !referenced.is_empty() {
        for (kind, name) in notes {
            let Some(name) = name else {
                continue;
            };
            let Some(xml) = walk.package.text(&name, "Notes", cancel)? else {
                continue;
            };
            let doc = package::xml(&xml)?;
            let part = Part {
                rels: rels(walk.package, &name, cancel)?,
            };
            let by_id: HashMap<&str, Node<'_, '_>> = doc
                .root_element()
                .children()
                .filter(|n| is(*n, &W, kind))
                .filter_map(|n| Some((attr(n, &W, "id")?, n)))
                .collect();
            for (ordinal, target) in referenced.iter().enumerate() {
                let Some(id) = target.strip_prefix(kind).and_then(|t| t.strip_prefix('-')) else {
                    continue;
                };
                let Some(note) = by_id.get(id) else {
                    continue;
                };
                walk.note_label = walk.labels.get(target).cloned();
                walk.builder
                    .note(u32::try_from(ordinal).unwrap_or(u32::MAX), target);
                walk.blocks(*note, &part, false)?;
            }
        }
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

/// What a Word document says about itself, for Properties.
pub(crate) fn about(
    path: &Path,
    cancel: &CancelToken,
) -> anyhow::Result<Vec<(&'static str, String)>> {
    Ok(details(&mut Package::open(path, cancel)?, cancel))
}

/// What the package says about the document: its title, author and the
/// pages its application counted.
fn details(package: &mut Package, cancel: &CancelToken) -> Vec<(&'static str, String)> {
    let mut facts = Vec::new();
    let core = related(package, "", "core-properties", cancel)
        .unwrap_or_else(|| "docProps/core.xml".into());
    if let Ok(Some(xml)) = package.text(&core, "Properties", cancel)
        && let Ok(doc) = package::xml(&xml)
    {
        for (label, local) in [
            ("Title", "title"),
            ("Author", "creator"),
            ("Subject", "subject"),
            ("Keywords", "keywords"),
        ] {
            if let Some(text) = doc
                .root_element()
                .children()
                .find(|n| n.is_element() && n.tag_name().name() == local)
                .and_then(|n| n.text())
            {
                facts.push((label, text.trim().to_owned()));
            }
        }
    }
    let app = related(package, "", "extended-properties", cancel)
        .unwrap_or_else(|| "docProps/app.xml".into());
    if let Ok(Some(xml)) = package.text(&app, "Properties", cancel)
        && let Ok(doc) = package::xml(&xml)
    {
        for (label, local) in [("Pages", "Pages"), ("Application", "Application")] {
            if let Some(text) = doc
                .root_element()
                .children()
                .find(|n| n.is_element() && n.tag_name().name() == local)
                .and_then(|n| n.text())
            {
                facts.push((label, text.trim().to_owned()));
            }
        }
    }
    facts
}
