//! Native reading-mode HTML: semantic blocks, inline styles and local images.
//! Scripts, frames and remote resources are never executed or fetched.
//!
//! A page is read for its structure (headings, paragraphs, lists, quotes,
//! code, tables, rules, pictures) and for the part of its own styling that
//! says how text reads (`style`): a publisher's classes carry the italics of
//! most books, and `display: none` hides what the page hides. XHTML is read
//! as XML (`xml`), so a chapter that closes `<title/>` itself is not lost.
//!
//! What a book puts beside its text keeps its place in the reading: a
//! footnote (`epub:type="footnote"`, `role="doc-footnote"`) moves to the end
//! of the page, under a rule, where its link still reaches it; a printed
//! page number (`pagebreak`) is not read into the sentence, but stays a place
//! to link to. Every `id` is a link target of the block it lands in.
mod style;
mod xml;

use crate::inspection::{check, limits};
use bibli_core::{
    CancelToken,
    inspection::{Block, BlockKind, Run},
    thumbs::Image,
};
use ego_tree::{NodeId, NodeRef, iter::Edge};
use scraper::{ElementRef, Html, Node, node::Element};
use std::{collections::HashSet, fmt::Write};
use style::{Display, Sheets, Style};

/// A page read for its structure.
#[derive(Debug, Default)]
pub struct Page {
    pub blocks: Vec<Block>,
    /// The page went on past `limits::BLOCKS` and the rest was left out: the
    /// caller says so in its notice (`inspection::blocks_notice`), and the
    /// page's source shows it whole.
    pub truncated: bool,
}

/// How a page's markup is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Markup {
    Html,
    /// Read as XML first, as HTML only when it is not well-formed.
    Xhtml,
}

/// What a page names outside itself, resolved by the caller within the
/// page's own bounds: a Markdown file's folder, a book's container, a
/// message's parts.
pub trait Resources {
    fn image(&mut self, src: &str) -> Option<Image>;
    /// A linked stylesheet's text.
    fn stylesheet(&mut self, _href: &str) -> Option<String> {
        None
    }
}
/// Resources that are pictures alone, resolved by `image`.
pub fn pictures<F: FnMut(&str) -> Option<Image>>(image: F) -> Pictures<F> {
    Pictures(image)
}
pub struct Pictures<F>(F);
impl<F: FnMut(&str) -> Option<Image>> Resources for Pictures<F> {
    fn image(&mut self, src: &str) -> Option<Image> {
        (self.0)(src)
    }
}

/// A page on disk: its pictures and stylesheets are read only from its own
/// folder, like `local_image`.
pub struct Local<'a> {
    pub base: &'a std::path::Path,
    pub cancel: &'a CancelToken,
}
impl Resources for Local<'_> {
    fn image(&mut self, src: &str) -> Option<Image> {
        local_image(self.base, src, self.cancel)
    }
    fn stylesheet(&mut self, href: &str) -> Option<String> {
        let bytes = local_file(
            self.base,
            href,
            limits::TEXT_INPUT,
            "Stylesheet",
            self.cancel,
        )?;
        Some(crate::inspection::decode(&bytes).0.into_owned())
    }
}

pub fn html(
    source: &str,
    markup: Markup,
    cancel: &CancelToken,
    mut resources: impl Resources,
) -> anyhow::Result<Page> {
    let doc = match markup {
        Markup::Xhtml => xml::parse(source),
        Markup::Html => None,
    }
    .unwrap_or_else(|| Html::parse_document(source));
    let mut sheets = Sheets::new();
    for node in doc.tree.root().descendants() {
        let Some(element) = ElementRef::wrap(node) else {
            continue;
        };
        let e = element.value();
        if e.name.ns == markup5ever::ns!(svg) {
            continue;
        }
        match e.name() {
            "style" => sheets.add(&element.text().collect::<String>()),
            "link"
                if e.attr("rel").is_some_and(|rel| {
                    let mut kinds = rel.split_ascii_whitespace();
                    kinds.clone().any(|k| k.eq_ignore_ascii_case("stylesheet"))
                        && !kinds.any(|k| k.eq_ignore_ascii_case("alternate"))
                }) =>
            {
                if let Some(css) = e.attr("href").and_then(|href| resources.stylesheet(href)) {
                    sheets.add(&css);
                }
            }
            _ => {}
        }
    }
    check(cancel)?;
    let mut walk = Walk::default();
    let mut stack: Vec<Context> = vec![Context::default()];
    // A subtree that is not read: hidden, too deep, or a picture.
    let mut skip: Option<NodeId> = None;
    for (visited, edge) in doc.tree.root().traverse().enumerate() {
        if visited % 256 == 0 {
            check(cancel)?;
        }
        if walk.blocks.len() + walk.notes.len() > limits::BLOCKS {
            return Ok(walk.page());
        }
        match edge {
            Edge::Open(node) => {
                if skip.is_some() {
                    continue;
                }
                let parent = stack.last().expect("the root context").clone();
                match node.value() {
                    Node::Text(text) => walk.text(&parent, text),
                    Node::Element(e) => {
                        let element = ElementRef::wrap(node).expect("an element");
                        let style = sheets.compute(element, &parent.style);
                        if style.display == Display::None
                            || stack.len() > limits::HTML_DEPTH
                            || matches!(e.name(), "annotation" | "annotation-xml")
                        {
                            skip = Some(node.id());
                            continue;
                        }
                        let context = parent.enter(node, e, style);
                        walk.anchor(&context, e, node.id());
                        if walk.open(&context, node, e, &mut resources) {
                            skip = Some(node.id());
                            continue;
                        }
                        stack.push(context);
                    }
                    _ => {}
                }
            }
            Edge::Close(node) => {
                if skip == Some(node.id()) {
                    skip = None;
                    continue;
                }
                if skip.is_some() || !node.value().is_element() {
                    continue;
                }
                let context = stack.pop().expect("an open element");
                if node.value().as_element().is_some_and(|e| e.name() == "rt")
                    && !has_child(node.parent(), "rp")
                {
                    walk.push(&context, ")");
                }
            }
        }
    }
    Ok(walk.page())
}

/// What an open element passes to its content.
// Each flag is an independent piece of the context an element is in (inside
// a `pre`, a `code`, a quote, a note, a page number), not states of one thing.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone)]
struct Context {
    style: Style,
    /// The nearest block, and its style.
    block: Option<NodeId>,
    block_style: Style,
    /// Left indent of the page's own margins, summed over the blocks.
    indent: f32,
    heading: u8,
    /// Inside `pre`: a code box.
    pre: bool,
    code: bool,
    quote: bool,
    lists: u32,
    /// Whether the nearest list is numbered, and where it starts.
    list: Option<(bool, usize)>,
    /// The nearest list item and its marker.
    item: Option<(NodeId, String)>,
    link: Option<String>,
    language: Option<String>,
    row: Option<NodeId>,
    note: bool,
    /// A printed page number: kept as a place to link to, never read.
    silent: bool,
}
impl Default for Context {
    fn default() -> Self {
        Self {
            style: Style::default(),
            block: None,
            block_style: Style::default(),
            indent: 0.,
            heading: 0,
            pre: false,
            code: false,
            quote: false,
            lists: 0,
            list: None,
            item: None,
            link: None,
            language: None,
            row: None,
            note: false,
            silent: false,
        }
    }
}
impl Context {
    fn enter(&self, node: NodeRef<'_, Node>, e: &Element, style: Style) -> Self {
        let mut context = self.clone();
        context.style = style;
        let name = e.name();
        if style.display == Display::Block {
            context.block = Some(node.id());
            context.block_style = style;
            // Quotes, lists and tables have a shape of their own; a page's
            // margins on them would indent them twice. The page's own
            // margins around everything are not the text's.
            if !matches!(
                name,
                "html" | "body" | "blockquote" | "ul" | "ol" | "li" | "pre" | "table" | "tr"
            ) {
                context.indent = (context.indent + style.indent).clamp(0., INDENT);
            }
        }
        match name {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                context.heading = name.as_bytes()[1] - b'0';
            }
            "pre" => context.pre = true,
            "code" => context.code = true,
            "blockquote" => context.quote = true,
            "ul" | "ol" => {
                context.lists += 1;
                let start = e
                    .attr("start")
                    .and_then(|start| start.parse::<usize>().ok())
                    .unwrap_or(1);
                context.list = Some((name == "ol", start));
            }
            "li" => {
                let before = node
                    .prev_siblings()
                    .filter(|n| n.value().as_element().is_some_and(|e| e.name() == "li"))
                    .count();
                let marker = match context.list {
                    Some((true, start)) => format!("{}.", start + before),
                    _ => "•".into(),
                };
                context.item = Some((node.id(), marker));
            }
            "a" => {
                if let Some(href) = e.attr("href") {
                    context.link = Some(href.to_owned());
                }
            }
            "tr" => context.row = Some(node.id()),
            _ => {}
        }
        if let Some(language) = e.attr("class").and_then(|c| {
            c.split_whitespace()
                .find_map(|c| c.strip_prefix("language-"))
        }) {
            context.language = Some(language.to_owned());
        }
        let mut kinds = e
            .attr("epub:type")
            .into_iter()
            .chain(e.attr("role"))
            .flat_map(str::split_ascii_whitespace);
        if kinds
            .clone()
            .any(|kind| matches!(kind, "footnote" | "note" | "rearnote" | "doc-footnote"))
        {
            context.note = true;
        }
        if kinds.any(|kind| matches!(kind, "pagebreak" | "doc-pagebreak")) {
            context.silent = true;
        }
        context
    }
}

/// How far a page's margins may indent its text, in ems.
const INDENT: f32 = 8.;

#[derive(Default)]
struct Walk {
    blocks: Vec<Block>,
    /// Footnotes, moved to the end of the page.
    notes: Vec<Block>,
    current: Block,
    /// Whether `current` is part of a note, and keeps its spaces.
    current_note: bool,
    current_pre: bool,
    /// The element whose content `current` holds.
    group: Option<NodeId>,
    /// Whether `current` is a table row, and a header row.
    row: Option<bool>,
    /// Cells opened in the current row.
    cells: usize,
    /// List items that already got their marker: it belongs to the first
    /// block of an item, not to every paragraph inside it.
    marked: HashSet<NodeId>,
    /// Link targets waiting for the next block that has content.
    pending: Vec<String>,
    images: usize,
    /// The size of each block's text relative to the root, for `page`.
    sizes: Vec<(f32, bool)>,
    note_sizes: Vec<(f32, bool)>,
    current_size: f32,
}
impl Walk {
    /// Whether `current` has anything in it.
    fn filled(&self) -> bool {
        !self.current.text.is_empty() || self.current.image.is_some()
    }

    /// Content from `context` is about to be added: close the block before
    /// it if it belongs to another one, and describe the block it goes in.
    fn begin(&mut self, context: &Context) {
        if self.group != context.block {
            if self.filled() {
                self.finish();
            }
            self.group = context.block;
            self.row = (context.row.is_some() && context.row == context.block).then_some(false);
            self.cells = 0;
        }
        if !self.filled() {
            self.current.anchors.append(&mut self.pending);
            self.current_note = context.note;
            self.current_pre = context.pre || context.block_style.pre;
            self.current_size = context.block_style.size;
        }
        let block = &mut self.current;
        block.heading = context.heading;
        block.code = context.pre;
        block.quote = context.quote;
        block.language.clone_from(&context.language);
        block.align = context.block_style.align;
        block.indent = context.indent;
        if let Some((item, marker)) = &context.item {
            block.list_depth = context.lists.max(1);
            if block.text.is_empty() && block.marker.is_none() && self.marked.insert(*item) {
                block.marker = Some(marker.clone());
            }
        } else {
            block.list_depth = 0;
        }
    }

    /// Close `current`: a table row gets its cells, the ranges between its
    /// tabs, and running text loses the spaces it ended on.
    fn finish(&mut self) {
        let mut block = std::mem::take(&mut self.current);
        if !self.current_pre && block.kind == BlockKind::Text && block.image.is_none() {
            let end = block.text.trim_end_matches([' ', '\n']).len();
            block.text.truncate(end);
            block.runs.retain_mut(|run| {
                run.range.end = run.range.end.min(end);
                run.range.start < run.range.end
            });
        }
        if let Some(header) = self.row.take() {
            let mut cells = Vec::new();
            let mut start = 0;
            for (index, _) in block.text.match_indices('\t') {
                cells.push(start..index);
                start = index + 1;
            }
            cells.push(start..block.text.len());
            block.kind = BlockKind::TableRow { cells, header };
        }
        let size = (self.current_size, block.heading == 0 && !block.code);
        if self.current_note {
            self.notes.push(block);
            self.note_sizes.push(size);
        } else {
            self.blocks.push(block);
            self.sizes.push(size);
        }
        self.current_note = false;
    }

    /// Record the link targets an element names.
    fn anchor(&mut self, context: &Context, e: &Element, id: NodeId) {
        let names = e
            .id()
            .into_iter()
            .chain(e.attr("name").filter(|_| e.name() == "a"));
        for name in names {
            // Inside a block that has begun, a name is a place in it; one on
            // a block, or before any text, is the block that comes next.
            if context.block != Some(id) && self.group == context.block && self.filled() {
                self.current.anchors.push(name.to_owned());
            } else {
                self.pending.push(name.to_owned());
            }
        }
    }

    fn text(&mut self, context: &Context, text: &str) {
        if context.silent {
            return;
        }
        let pre = context.pre || context.style.pre;
        let text = if pre {
            text.to_owned()
        } else {
            let mut out = String::new();
            let mut space = self.group != context.block
                || self.current.text.is_empty()
                || self.current.text.ends_with([' ', '\n', '\t']);
            // CSS collapses spaces, tabs and line breaks, never a
            // non-breaking space, which is there to be kept.
            for c in text.chars() {
                if matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{c}') {
                    if !space {
                        out.push(' ');
                        space = true;
                    }
                } else {
                    out.push(c);
                    space = false;
                }
            }
            out
        };
        if text.is_empty() {
            return;
        }
        self.begin(context);
        let start = self.current.text.len();
        self.current.text.push_str(&text);
        self.current.runs.push(Run {
            color: None,
            range: start..self.current.text.len(),
            bold: context.style.bold,
            italic: context.style.italic,
            strikethrough: context.style.strike,
            underline: context.style.underline,
            code: context.code,
            link: context.link.clone(),
        });
    }

    /// Text the reader adds around the page's own (a ruby reading's
    /// brackets), in the page's style but outside its runs.
    fn push(&mut self, context: &Context, text: &str) {
        if context.silent {
            return;
        }
        self.begin(context);
        self.current.text.push_str(text);
    }

    /// An element opens. Returns whether its content is already read: a
    /// picture, which is one thing however it is drawn.
    fn open(
        &mut self,
        context: &Context,
        node: NodeRef<'_, Node>,
        e: &Element,
        resources: &mut impl Resources,
    ) -> bool {
        if e.name.ns == markup5ever::ns!(svg) && e.name() == "svg" {
            if !context.silent {
                self.svg(context, node, resources);
            }
            return true;
        }
        if e.name() == "math"
            && let Some(alt) = e.attr("alttext")
        {
            self.text(context, alt);
            return true;
        }
        match e.name() {
            "br" => {
                if !self.current.text.is_empty() && !context.silent {
                    self.begin(context);
                    self.current.text.push('\n');
                }
            }
            // Cells are ranges of the row's text, joined by tabs, as Markdown's
            // are: emphasis and links inside a cell stay ordinary runs. An
            // empty cell is a column too.
            "td" | "th" => {
                if context.row.is_some() && context.row == context.block {
                    self.begin(context);
                    if let Some(header) = self.row.as_mut() {
                        *header |= e.name() == "th";
                    }
                    if self.cells > 0 {
                        self.current.text.push('\t');
                    }
                    self.cells += 1;
                }
            }
            "hr" => {
                if self.filled() {
                    self.finish();
                }
                self.blocks.push(Block {
                    kind: BlockKind::Rule,
                    ..Block::default()
                });
                self.sizes.push((1., false));
            }
            "input" if e.attr("type") == Some("checkbox") => {
                let mark = if e.attr("checked").is_some() {
                    "☑ "
                } else {
                    "☐ "
                };
                self.push(context, mark);
            }
            "img" if !context.silent => {
                let alt = e.attr("alt").unwrap_or_default();
                let bitmap = if self.images < limits::EMBEDDED_IMAGES {
                    e.attr("src").and_then(|src| resources.image(src))
                } else {
                    None
                };
                self.picture(context, alt, bitmap);
            }
            "rt" if !has_child(node.parent(), "rp") => self.push(context, "("),
            _ => {}
        }
        false
    }

    /// An SVG picture: the image it frames (a cover page), or the drawing
    /// itself.
    fn svg(&mut self, context: &Context, node: NodeRef<'_, Node>, resources: &mut impl Resources) {
        let framed = node.descendants().find_map(|n| {
            let e = n.value().as_element()?;
            (e.name() == "image")
                .then(|| e.attrs().find(|(name, _)| *name == "href").map(|(_, v)| v))
                .flatten()
        });
        let title = node
            .children()
            .find(|n| n.value().as_element().is_some_and(|e| e.name() == "title"))
            .and_then(ElementRef::wrap)
            .map(|title| title.text().collect::<String>())
            .unwrap_or_default();
        let bitmap = if self.images >= limits::EMBEDDED_IMAGES {
            None
        } else if let Some(href) = framed {
            resources.image(href)
        } else {
            crate::inspection::image(svg_source(node).as_bytes()).ok()
        };
        self.picture(context, title.trim(), bitmap);
    }

    /// A picture ends the block it sits in and keeps its quote and list.
    /// In a table row it stands between the cells before and after it.
    fn picture(&mut self, context: &Context, alt: &str, bitmap: Option<Image>) {
        self.begin(context);
        let row = self.row;
        if !self.current.text.is_empty() {
            self.finish();
            self.begin(context);
        }
        self.images += 1;
        alt.clone_into(&mut self.current.text);
        self.current.image = bitmap;
        self.row = None;
        self.finish();
        self.row = row;
    }

    fn page(mut self) -> Page {
        if self.filled() {
            self.finish();
        }
        if !self.pending.is_empty()
            && let Some(last) = self.blocks.last_mut()
        {
            last.anchors.append(&mut self.pending);
        }
        if !self.notes.is_empty() {
            self.blocks.push(Block {
                kind: BlockKind::Rule,
                ..Block::default()
            });
            self.sizes.push((1., false));
            self.blocks.append(&mut self.notes);
            self.sizes.append(&mut self.note_sizes);
        }
        sizes(&mut self.blocks, &self.sizes);
        let truncated = self.blocks.len() > limits::BLOCKS;
        self.blocks.truncate(limits::BLOCKS);
        Page {
            blocks: self.blocks,
            truncated,
        }
    }
}

/// Set each block's `size` relative to the page's body text: the size most of
/// its running text is set in, whatever the page calls it. A book that sets
/// every paragraph at `1.1em` reads at the reader's size; its title page and
/// its small print stay larger and smaller than that.
fn sizes(blocks: &mut [Block], sizes: &[(f32, bool)]) {
    let mut weights: Vec<(f32, usize)> = Vec::new();
    for (block, &(size, running)) in blocks.iter().zip(sizes) {
        if !running {
            continue;
        }
        match weights.iter_mut().find(|(s, _)| (s - size).abs() < 0.01) {
            Some((_, weight)) => *weight += block.text.len(),
            None => weights.push((size, block.text.len())),
        }
    }
    let body = weights
        .iter()
        .max_by_key(|(_, weight)| *weight)
        .map_or(1., |(size, _)| *size);
    for (block, &(size, running)) in blocks.iter_mut().zip(sizes) {
        let scale = (size / body).clamp(0.75, 2.);
        block.size = (running && (scale - 1.).abs() > 0.08).then_some(scale);
    }
}

fn has_child(node: Option<NodeRef<'_, Node>>, name: &str) -> bool {
    node.is_some_and(|node| {
        node.children()
            .any(|n| n.value().as_element().is_some_and(|e| e.name() == name))
    })
}

/// An SVG element written out on its own, for the rasterizer: the namespace
/// its page implied declared, and nothing but local names, which SVG 2 reads
/// (`href` as well as `xlink:href`).
fn svg_source(node: NodeRef<'_, Node>) -> String {
    let mut out = String::new();
    for edge in node.traverse() {
        match edge {
            Edge::Open(n) => match n.value() {
                Node::Element(e) => {
                    let _ = write!(out, "<{}", e.name());
                    if n.id() == node.id() {
                        out.push_str(r#" xmlns="http://www.w3.org/2000/svg""#);
                    }
                    for (name, value) in e.attrs() {
                        if name.contains(':') || name.starts_with("xmlns") {
                            continue;
                        }
                        let _ =
                            write!(out, r#" {name}="{}""#, escape(value).replace('"', "&quot;"));
                    }
                    out.push('>');
                }
                Node::Text(text) => out.push_str(&escape(text)),
                _ => {}
            },
            Edge::Close(n) => {
                if let Some(e) = n.value().as_element() {
                    let _ = write!(out, "</{}>", e.name());
                }
            }
        }
    }
    out
}
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Read a file named by a document, only if it resolves (symlinks included)
/// inside the document's directory.
fn local_file(
    base: &std::path::Path,
    href: &str,
    limit: usize,
    what: &str,
    cancel: &CancelToken,
) -> Option<Vec<u8>> {
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
    crate::inspection::read_bounded(&path, limit, what, cancel).ok()
}
/// Resolve only local descendants of the document directory, including symlinks.
pub fn local_image(base: &std::path::Path, href: &str, cancel: &CancelToken) -> Option<Image> {
    let bytes = local_file(base, href, limits::BINARY_INPUT, "Embedded image", cancel)?;
    crate::inspection::image(&bytes).ok()
}
#[cfg(test)]
mod tests {
    use super::*;
    use bibli_core::inspection::Align;
    #[test]
    fn semantic_text_does_not_include_active_content() {
        let blocks=html("<h1>Title</h1><p>A <strong>bold</strong> <a href='https://example.invalid'>link</a></p><script>secret()</script>", Markup::Html, &CancelToken::new(), pictures(|_|None)).unwrap().blocks;
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
        let blocks = html(
            source,
            Markup::Html,
            &CancelToken::new(),
            pictures(|_| None),
        )
        .unwrap()
        .blocks;
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
        assert_eq!(
            [blocks[0].align, blocks[1].align, blocks[9].align],
            [Align::Center, Align::Start, Align::Center]
        );
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
        let blocks = html(
            &source,
            Markup::Html,
            &CancelToken::new(),
            pictures(|src| {
                requested.push(src.to_owned());
                None
            }),
        )
        .unwrap()
        .blocks;
        assert_eq!(requested.len(), limits::EMBEDDED_IMAGES);
        assert_eq!(blocks.len(), limits::EMBEDDED_IMAGES + 3);
    }

    #[test]
    fn a_long_page_is_cut_short_not_refused() {
        let paragraphs = |n: usize| "<p>line</p>".repeat(n);
        let whole = html(
            &paragraphs(limits::BLOCKS),
            Markup::Html,
            &CancelToken::new(),
            pictures(|_| None),
        )
        .unwrap();
        assert_eq!(
            (whole.blocks.len(), whole.truncated),
            (limits::BLOCKS, false)
        );
        let long = html(
            &paragraphs(limits::BLOCKS + 5),
            Markup::Html,
            &CancelToken::new(),
            pictures(|_| None),
        )
        .unwrap();
        assert_eq!((long.blocks.len(), long.truncated), (limits::BLOCKS, true));
    }

    fn read(source: &str, markup: Markup) -> Vec<Block> {
        html(source, markup, &CancelToken::new(), pictures(|_| None))
            .unwrap()
            .blocks
    }
    fn texts(blocks: &[Block]) -> Vec<&str> {
        blocks.iter().map(|b| b.text.as_str()).collect()
    }

    /// A book's emphasis and alignment live in its stylesheet's classes, as
    /// Calibre and `InDesign` write them; linked sheets come from the caller.
    #[test]
    fn a_publisher_stylesheet_carries_emphasis_alignment_and_hiding() {
        struct Book;
        impl Resources for Book {
            fn image(&mut self, _: &str) -> Option<Image> {
                None
            }
            fn stylesheet(&mut self, href: &str) -> Option<String> {
                (href == "../Styles/style.css").then(|| ".calibre7 { font-weight: bold }".into())
            }
        }
        let source = r#"<html><head><link rel="stylesheet" type="text/css" href="../Styles/style.css"/>
            <style>.calibre5 { font-style: italic } .c { text-align: center } .hide { display: none }
            .u { text-decoration: underline }</style></head>
            <body><p class="c">CHAPTER ONE</p>
            <p class="calibre3">She said <span class="calibre5">never</span> and <span class="calibre7">meant</span> <span class="u">it</span>.</p>
            <p class="hide">hidden</p><p style="display:none">also hidden</p></body></html>"#;
        let blocks = html(source, Markup::Html, &CancelToken::new(), Book)
            .unwrap()
            .blocks;
        assert_eq!(
            texts(&blocks),
            ["CHAPTER ONE", "She said never and meant it."]
        );
        assert_eq!(blocks[0].align, Align::Center);
        let styled: Vec<_> = blocks[1]
            .runs
            .iter()
            .map(|r| {
                (
                    &blocks[1].text[r.range.clone()],
                    r.italic,
                    r.bold,
                    r.underline,
                )
            })
            .collect();
        assert_eq!(
            styled,
            [
                ("She said ", false, false, false),
                ("never", true, false, false),
                (" and ", false, false, false),
                ("meant", false, true, false),
                (" ", false, false, false),
                ("it", false, false, true),
                (".", false, false, false),
            ]
        );
    }

    /// XHTML closes `<title/>` itself; read as HTML it would swallow the page.
    #[test]
    fn xhtml_is_read_as_xml_and_falls_back_to_html() {
        let xhtml = r#"<?xml version="1.0" encoding="utf-8"?>
            <html xmlns="http://www.w3.org/1999/xhtml"><head><title/><script src="a.js"/></head>
            <body><p>First&nbsp;paragraph.</p><div class="spacer"/><p>Second.</p></body></html>"#;
        assert_eq!(
            texts(&read(xhtml, Markup::Xhtml)),
            ["First\u{a0}paragraph.", "Second."]
        );
        let broken = "<html><body><p>Unclosed<br><p>Still read.</body></html>";
        assert_eq!(
            texts(&read(broken, Markup::Xhtml)),
            ["Unclosed", "Still read."]
        );
    }

    /// Every `id` is somewhere to land: a section's is its first block, a
    /// word's is the block around it, an old `<a name>` too.
    #[test]
    fn every_id_is_a_link_target() {
        let source = r#"<section id="ch3"><h2>Three</h2><p>Para <span id="p12"></span>with <a name="old"></a>anchors.</p></section>
            <p id="own">Own.</p><span id="end"></span>"#;
        let blocks = read(source, Markup::Html);
        let anchors: Vec<_> = blocks.iter().map(|b| b.anchors.join(" ")).collect();
        assert_eq!(anchors, ["ch3", "p12 old", "own end"]);
    }

    /// A footnote leaves the running text for the end of the page, where its
    /// link reaches it; a printed page number is a target, not a word.
    #[test]
    fn footnotes_move_to_the_end_and_page_numbers_are_not_read() {
        let source = r##"<p>Text with a note<a epub:type="noteref" id="r1" href="#n1">1</a> continues.</p>
            <aside epub:type="footnote" id="n1"><p><a href="#r1">1.</a> The note body.</p></aside>
            <p>Before<span epub:type="pagebreak" id="page_12" title="12">12</span> after.</p>
            <div role="doc-footnote" id="n2">Second note.</div>"##;
        let blocks = read(source, Markup::Html);
        assert_eq!(
            texts(&blocks),
            [
                "Text with a note1 continues.",
                "Before after.",
                "",
                "1. The note body.",
                "Second note."
            ]
        );
        assert_eq!(blocks[1].anchors, ["page_12"]);
        assert_eq!(blocks[2].kind, BlockKind::Rule);
        assert_eq!(blocks[3].anchors, ["n1"]);
        assert_eq!(blocks[0].anchors, ["r1"], "the note's way back");
        assert_eq!(blocks[4].anchors, ["n2"]);
    }

    #[test]
    fn ruby_definitions_and_whitespace_read_as_text() {
        let source = r"<p><ruby>漢字<rt>かんじ</rt></ruby>を読む</p>
            <p><ruby>字<rp>(</rp><rt>じ</rt><rp>)</rp></ruby></p>
            <dl><dt>Term</dt><dd>Definition</dd></dl>
            <p>  one
              <i>two</i>
              three  </p>";
        let blocks = read(source, Markup::Html);
        assert_eq!(
            texts(&blocks),
            [
                "漢字(かんじ)を読む",
                "字(じ)",
                "Term",
                "Definition",
                "one two three"
            ]
        );
        assert!(
            (blocks[3].indent - 2.).abs() < 1e-4,
            "a definition is indented"
        );
    }

    /// A title page set larger than the text stays larger; the size a book
    /// sets all its paragraphs in is the reader's size.
    #[test]
    fn sizes_are_relative_to_the_body_text() {
        let source = r#"<style>body { font-size: 1.1em } p { font-size: 1.1em } p.title { font-size: 2.2em }
            p.small { font-size: 0.88em }</style>
            <p class="title">Title</p><p>Body text that is long enough to be the body.</p>
            <p>More body text in the same size.</p><p class="small">Small print.</p>"#;
        let sizes: Vec<_> = read(source, Markup::Html).iter().map(|b| b.size).collect();
        assert_eq!(sizes[1..3], [None, None]);
        assert!((sizes[0].unwrap() - 2.).abs() < 0.01, "{sizes:?}");
        assert!((sizes[3].unwrap() - 0.8).abs() < 0.01, "{sizes:?}");
    }

    #[test]
    fn empty_cells_keep_their_column_and_pictures_leave_the_row() {
        let source = r#"<table><tr><td></td><td>b</td></tr><tr><td><img src="a.png" alt="A"/></td><td>c</td></tr></table>"#;
        let blocks = read(source, Markup::Html);
        assert_eq!(
            blocks[0].kind,
            BlockKind::TableRow {
                cells: vec![0..0, 1..2],
                header: false
            }
        );
        assert_eq!(blocks[1].kind, BlockKind::Text, "a picture is not a row");
        assert_eq!(blocks[1].text, "A");
    }

    /// A cover page frames its picture in SVG; a drawing is rasterized.
    #[test]
    fn svg_frames_a_picture_or_is_one() {
        let source = r#"<div><svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 600 800"><title>Cover</title><image width="600" height="800" xlink:href="../Images/cover.jpg"/></svg></div>
            <p>Text <svg width="20" height="10" viewBox="0 0 20 10"><rect width="20" height="10" fill="red"/><text>not read</text></svg> after.</p>"#;
        for markup in [Markup::Html, Markup::Xhtml] {
            let mut asked = Vec::new();
            let blocks = html(
                source,
                markup,
                &CancelToken::new(),
                pictures(|src| {
                    asked.push(src.to_owned());
                    Some(Image {
                        width: 1,
                        height: 1,
                        rgba: vec![0; 4].into(),
                    })
                }),
            )
            .unwrap()
            .blocks;
            assert_eq!(asked, ["../Images/cover.jpg"]);
            assert_eq!(texts(&blocks), ["Cover", "Text", "", "after."]);
            let drawing = blocks[2].image.as_ref().expect("the drawing");
            assert_eq!((drawing.width, drawing.height), (20, 10));
            assert_eq!(&drawing.rgba[..4], &[255, 0, 0, 255]);
        }
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
