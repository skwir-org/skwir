//! A fixed-layout page drawn as its book lays it out.
//!
//! A fixed-layout book (`rendition:layout pre-paginated`) places every box
//! of a page itself. A comic converted from PDF is a picture per page with
//! its lettering as lines of text in the book's fonts at exact positions over
//! it, and that text alone reads as nothing: lettering fonts map their glyph
//! variants to private code points. Such a page is not read but drawn: its
//! boxes are laid out here from the cascade (`style::PageStyle`) and written
//! as one SVG document, which resvg rasterizes with the book's own fonts
//! (renamed to the families the page's CSS gives them, since a converter
//! subsets one font into several files with one internal name).
//!
//! What this layout knows: absolutely positioned, relative and static boxes
//! with explicit or content sizes, margins and padding, 2D transforms with
//! their origin, backgrounds and opacity, pictures (`<img>`, SVG frames),
//! and lines of text in the page's fonts, sizes, line heights, letter and
//! word spacing, with alignment and wrapping, and inline spacers (an empty
//! `inline-block`'s width, an inline margin) that converters use to kern.
//! Floats, tables, columns and CSS background images are not laid out. A
//! page that states no viewport (`<meta name="viewport">`) is drawn at the
//! size the caller gives for it, or not at all.
//!
//! Like reading mode it is inert: fonts and pictures come only through the
//! caller's `Resources`, and resvg is refused every other reference.
use super::{
    Markup, Resources, document, escape,
    style::{IDENTITY, LineHeight, PageDisplay, PageStyle, Position, Sheets, multiply},
    stylesheets,
};
use crate::inspection::{check, limits};
use ego_tree::NodeRef;
use resvg::{
    tiny_skia,
    usvg::{self, fontdb},
};
use scraper::{ElementRef, Node};
use skwir_core::{CancelToken, inspection::Align};
use std::{collections::HashMap, fmt::Write, rc::Rc, sync::Arc};

/// Characters of text one page may set.
const TEXT: usize = 100_000;
/// Pictures one page may draw.
const PICTURES: usize = 64;
/// The largest a page's own size may be, in CSS pixels.
const VIEWPORT: f32 = 8192.;

/// A page drawn: its pixels, and its own size in CSS pixels, what 100% is.
pub struct Drawing {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub own: (f32, f32),
}

/// The page drawn with `side` pixels on its longer side (bounded by
/// `document::MAX_SIDE`), at the size its viewport states, else at
/// `fallback`; `None` when it states none and there is no fallback.
pub fn page(
    source: &str,
    markup: Markup,
    cancel: &CancelToken,
    mut resources: impl Resources,
    side: u32,
    fallback: Option<(f32, f32)>,
) -> anyhow::Result<Option<Drawing>> {
    let Some((canvas, drawn)) = layout(source, markup, cancel, &mut resources, fallback)? else {
        return Ok(None);
    };
    let own = (drawn.width, drawn.height);
    let side = side.clamp(64, skwir_core::document::MAX_SIDE);
    render(canvas, drawn, side, cancel).map(|(width, height, rgba)| {
        Some(Drawing {
            width,
            height,
            rgba,
            own,
        })
    })
}

/// What a page draws, in paint order: SVG fragments, which resvg draws with
/// the page's fonts, and the pictures between them, which are decoded here
/// (resvg is built without its own decoders, as gpui links it).
/// The pictures a page draws at most, however many times each is used.
const PICTURE_USES: usize = 1024;

#[derive(Default)]
struct Canvas {
    layers: Vec<Layer>,
    /// The pictures drawn so far (`PICTURE_USES`).
    pictures: usize,
    /// The opacity groups open in the fragment being written: a picture
    /// closes them and the next fragment opens them again.
    groups: Vec<f32>,
}
enum Layer {
    Svg(String),
    Picture {
        key: String,
        /// `x y width height` in page pixels, under `transform`.
        rect: [f32; 4],
        transform: [f32; 6],
        opacity: f32,
    },
}
impl Canvas {
    /// The fragment being written.
    fn svg(&mut self) -> &mut String {
        if !matches!(self.layers.last(), Some(Layer::Svg(_))) {
            self.layers.push(Layer::Svg(String::new()));
        }
        match self.layers.last_mut() {
            Some(Layer::Svg(svg)) => svg,
            _ => unreachable!("a fragment was just pushed"),
        }
    }
    /// Where the next write goes, to insert something there later.
    fn mark(&mut self) -> (usize, usize) {
        let at = self.svg().len();
        (self.layers.len() - 1, at)
    }
    fn insert(&mut self, (layer, at): (usize, usize), text: &str) {
        if let Some(Layer::Svg(svg)) = self.layers.get_mut(layer) {
            svg.insert_str(at, text);
        }
    }
    fn open(&mut self, opacity: f32) {
        let _ = write!(self.svg(), r#"<g opacity="{opacity}">"#);
        self.groups.push(opacity);
    }
    fn close(&mut self) {
        self.svg().push_str("</g>");
        self.groups.pop();
    }
    fn picture(&mut self, key: String, rect: [f32; 4], transform: [f32; 6]) {
        // A page's pictures drawn at most: each a layer of its own.
        if self.pictures >= PICTURE_USES {
            return;
        }
        self.pictures += 1;
        let open = self.groups.len();
        for _ in 0..open {
            self.svg().push_str("</g>");
        }
        self.layers.push(Layer::Picture {
            key,
            rect,
            transform,
            opacity: self.groups.iter().product(),
        });
        let reopened = self.groups.iter().fold(String::new(), |mut out, opacity| {
            let _ = write!(out, r#"<g opacity="{opacity}">"#);
            out
        });
        self.layers.push(Layer::Svg(reopened));
    }
}

/// What the page's SVG needs to be drawn: its size, fonts and pictures.
struct Drawn {
    width: f32,
    height: f32,
    fonts: Arc<fontdb::Database>,
    pictures: HashMap<String, Arc<Vec<u8>>>,
}

/// Lay the page out as what it draws.
fn layout(
    source: &str,
    markup: Markup,
    cancel: &CancelToken,
    resources: &mut impl Resources,
    fallback: Option<(f32, f32)>,
) -> anyhow::Result<Option<(Canvas, Drawn)>> {
    let doc = document(source, markup);
    let Some((width, height)) = viewport(&doc).or(fallback) else {
        return Ok(None);
    };
    let sheets = stylesheets(&doc, resources);
    check(cancel)?;
    let root = doc.root_element();
    let mut page = Layout {
        fonts: Fonts::new(sheets.faces.clone()),
        sheets,
        resources,
        canvas: Canvas::default(),
        pictures: HashMap::new(),
        measuring: 0,
        measured: HashMap::new(),
        text: 0,
        cancel,
    };
    let style = page.sheets.compute_page(root, &PageStyle::default());
    let whole = Rect {
        x: 0.,
        y: 0.,
        w: width,
        h: height,
    };
    page.block(
        *root,
        &style,
        Flow {
            x: 0.,
            y: 0.,
            width,
        },
        whole,
        IDENTITY,
        0,
    )?;
    Ok(Some((
        page.canvas,
        Drawn {
            width,
            height,
            fonts: page.fonts.db,
            pictures: page.pictures,
        },
    )))
}

/// The size a page states for itself, in CSS pixels, if it does.
pub fn stated_size(source: &str, markup: Markup) -> Option<(f32, f32)> {
    viewport(&document(source, markup))
}

/// The page's size, from `<meta name="viewport" content="width=…,height=…">`.
fn viewport(doc: &scraper::Html) -> Option<(f32, f32)> {
    let content = doc.tree.nodes().find_map(|node| {
        let e = node.value().as_element()?;
        (e.name() == "meta" && e.attr("name")?.eq_ignore_ascii_case("viewport"))
            .then(|| e.attr("content"))
            .flatten()
    })?;
    let (mut width, mut height) = (None, None);
    for pair in content.split([',', ';']) {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_end_matches("px").parse::<f32>().ok();
        match key.trim() {
            "width" => width = value,
            "height" => height = value,
            _ => {}
        }
    }
    let (width, height) = (width?, height?);
    (width >= 1. && height >= 1. && width <= VIEWPORT && height <= VIEWPORT)
        .then_some((width, height))
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Rect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

/// Where a box in the flow starts, and the width it has.
#[derive(Clone, Copy, Debug)]
struct Flow {
    x: f32,
    y: f32,
    width: f32,
}

/// What a box takes: its height in the flow of its parent (nothing for one
/// out of it) and the widest line it set, for a box sized to its content.
#[derive(Clone, Copy, Debug, Default)]
struct Extent {
    height: f32,
    width: f32,
}

struct Layout<'a, R> {
    sheets: Sheets,
    fonts: Fonts,
    resources: &'a mut R,
    canvas: Canvas,
    pictures: HashMap<String, Arc<Vec<u8>>>,
    /// Nonzero while boxes are measured rather than drawn.
    measuring: u32,
    /// What each box measured, by its node, its width and its containing
    /// block: a box measured before it is laid out, inside one measured
    /// too, was measured again at every level, two to the power of the
    /// depth.
    measured: HashMap<(ego_tree::NodeId, [u32; 5]), Extent>,
    text: usize,
    cancel: &'a CancelToken,
}

impl<R: Resources> Layout<'_, R> {
    fn drawing(&self) -> bool {
        self.measuring == 0
    }

    /// Lay out a box that is not inline: a block, or anything positioned
    /// absolutely. `at` is where it would stand in its parent's flow, `cb`
    /// the containing block of absolutely positioned boxes, `transform` the
    /// transform of its ancestors.
    fn block(
        &mut self,
        node: NodeRef<'_, Node>,
        style: &PageStyle,
        at: Flow,
        cb: Rect,
        transform: [f32; 6],
        depth: usize,
    ) -> anyhow::Result<Extent> {
        check(self.cancel)?;
        if depth > limits::HTML_DEPTH {
            return Ok(Extent::default());
        }
        let font = style.font_size;
        let absolute = style.position == Position::Absolute;
        let base = if absolute { cb.w } else { at.width };
        let px = |length: super::style::Length, base: f32| length.px(font, base);
        let margin = style.margin.map(|m| px(m, base).unwrap_or(0.));
        let padding = style.padding.map(|p| px(p, base).unwrap_or(0.));
        // Horizontal offsets are of the containing block's width, vertical
        // ones of its height.
        let left = px(style.offsets[3], base);
        let right = px(style.offsets[1], base);
        let top = style.offsets[0].px(font, cb.h);
        let bottom = style.offsets[2].px(font, cb.h);
        let horizontal = margin[1] + margin[3] + padding[1] + padding[3];
        let vertical = margin[0] + margin[2] + padding[0] + padding[2];
        let width = px(style.width, base);
        let height = style
            .height
            .px(font, if absolute { cb.h } else { f32::NAN });
        let height = height.filter(|h| h.is_finite());

        // The content width: stated, stretched between both offsets, the
        // flow's, or (an absolute box with neither) as wide as its content.
        let content_width = match (width, absolute, left, right) {
            (Some(width), ..) => width,
            (None, true, Some(left), Some(right)) => (cb.w - left - right - horizontal).max(0.),
            (None, true, ..) => {
                let inner = Flow {
                    x: 0.,
                    y: 0.,
                    width: f32::INFINITY,
                };
                self.measure(node, style, inner, cb, depth)?.width
            }
            (None, false, ..) => (at.width - horizontal).max(0.),
        };
        let border_width = content_width + padding[1] + padding[3];
        let x = if absolute {
            match (left, right) {
                (Some(left), _) => cb.x + left + margin[3],
                (None, Some(right)) => cb.x + cb.w - right - margin[1] - border_width,
                (None, None) => at.x + margin[3],
            }
        } else if matches!(style.margin[1], super::style::Length::Auto)
            && matches!(style.margin[3], super::style::Length::Auto)
        {
            at.x + (at.width - border_width) / 2.
        } else {
            at.x + margin[3]
        };
        // The content height: stated, stretched, or what the content takes,
        // which a box placed by its bottom needs before it is drawn.
        let stretched = match (absolute, height, top, bottom) {
            (true, None, Some(top), Some(bottom)) => Some((cb.h - top - bottom - vertical).max(0.)),
            _ => height,
        };
        let content_height = match stretched {
            Some(height) => Some(height),
            None if absolute && top.is_none() && bottom.is_some() || style.transform.is_some() => {
                let inner = Flow {
                    x: 0.,
                    y: 0.,
                    width: content_width,
                };
                Some(self.measure(node, style, inner, cb, depth)?.height)
            }
            None => None,
        };
        let y = if absolute {
            match (top, bottom, content_height) {
                (Some(top), ..) => cb.y + top + margin[0],
                (None, Some(bottom), Some(height)) => {
                    cb.y + cb.h - bottom - margin[2] - height - padding[0] - padding[2]
                }
                _ => at.y + margin[0],
            }
        } else {
            at.y + margin[0]
        };
        // A relative box moves with its content, and takes its place still.
        let (x, y) = if style.position == Position::Relative {
            (
                x + left.or(right.map(|r| -r)).unwrap_or(0.),
                y + top.or(bottom.map(|b| -b)).unwrap_or(0.),
            )
        } else {
            (x, y)
        };

        let transform = match style.transform {
            Some(matrix) => {
                let size = (
                    border_width,
                    content_height.unwrap_or(0.) + padding[0] + padding[2],
                );
                let ox = x + style.origin.0.px(font, size.0).unwrap_or(0.);
                let oy = y + style.origin.1.px(font, size.1).unwrap_or(0.);
                multiply(
                    transform,
                    multiply(
                        [1., 0., 0., 1., ox, oy],
                        multiply(matrix, [1., 0., 0., 1., -ox, -oy]),
                    ),
                )
            }
            None => transform,
        };
        let opened = self.open_group(style);
        // The background is drawn under the content, once its height is
        // known: its place in the document is kept for it.
        let background = self.canvas.mark();
        let inner_cb = if style.position == Position::Static {
            cb
        } else {
            Rect {
                x,
                y,
                w: border_width,
                h: content_height.unwrap_or(cb.h) + padding[0] + padding[2],
            }
        };
        let content = Flow {
            x: x + padding[3],
            y: y + padding[0],
            width: content_width,
        };
        let inside = self.children(node, style, content, inner_cb, transform, depth)?;
        let content_height = content_height.unwrap_or(inside.height);
        let border_height = content_height + padding[0] + padding[2];
        if let Some(color) = style.background.filter(|c| c[3] > 0)
            && self.drawing()
        {
            let [r, g, b, a] = color;
            let rect = format!(
                r#"<rect x="{x}" y="{y}" width="{border_width}" height="{border_height}" fill="rgb({r},{g},{b})" fill-opacity="{}"{}/>"#,
                f32::from(a) / 255.,
                matrix(transform)
            );
            self.canvas.insert(background, &rect);
        }
        if opened {
            self.canvas.close();
        }
        Ok(Extent {
            height: if absolute {
                0.
            } else {
                border_height + margin[0] + margin[2]
            },
            width: if width.is_some() {
                border_width
            } else {
                inside.width + padding[1] + padding[3]
            },
        })
    }

    /// Lay out `node`'s content without drawing it: what it takes.
    fn measure(
        &mut self,
        node: NodeRef<'_, Node>,
        style: &PageStyle,
        at: Flow,
        cb: Rect,
        depth: usize,
    ) -> anyhow::Result<Extent> {
        let key = (
            node.id(),
            [at.width, cb.x, cb.y, cb.w, cb.h].map(f32::to_bits),
        );
        if let Some(extent) = self.measured.get(&key) {
            return Ok(*extent);
        }
        self.measuring += 1;
        let extent = self.children(node, style, at, cb, IDENTITY, depth);
        self.measuring -= 1;
        let extent = extent?;
        self.measured.insert(key, extent);
        Ok(extent)
    }

    fn open_group(&mut self, style: &PageStyle) -> bool {
        if style.opacity < 1. && self.drawing() {
            self.canvas.open(style.opacity);
            true
        } else {
            false
        }
    }

    /// The content of a box: blocks stacked in its flow, lines of inline
    /// content between them, and what is positioned out of the flow.
    fn children(
        &mut self,
        node: NodeRef<'_, Node>,
        style: &PageStyle,
        at: Flow,
        cb: Rect,
        transform: [f32; 6],
        depth: usize,
    ) -> anyhow::Result<Extent> {
        let mut cursor = at.y;
        let mut widest: f32 = 0.;
        let mut inline = Vec::new();
        let shared = Rc::new(style.clone());
        for child in node.children() {
            match child.value() {
                Node::Text(text) => inline.push(Item::Text(text.to_string(), shared.clone())),
                Node::Element(e) => {
                    let element = ElementRef::wrap(child).expect("an element");
                    let own = self.sheets.compute_page(element, style);
                    if own.display == PageDisplay::None {
                        continue;
                    }
                    let replaced = matches!(e.name(), "img" | "svg" | "image");
                    let here = Flow {
                        x: at.x,
                        y: cursor,
                        width: at.width,
                    };
                    if own.position == Position::Absolute {
                        if replaced {
                            self.replaced(child, &own, here, cb, transform);
                        } else {
                            self.block(child, &own, here, cb, transform, depth + 1)?;
                        }
                    } else if own.display == PageDisplay::Block || replaced {
                        let lines = self.lines(&mut inline, style, here, transform);
                        cursor += lines.height;
                        widest = widest.max(lines.width);
                        let here = Flow { y: cursor, ..here };
                        let extent = if replaced {
                            self.replaced(child, &own, here, cb, transform)
                        } else {
                            self.block(child, &own, here, cb, transform, depth + 1)?
                        };
                        cursor += extent.height;
                        widest = widest.max(extent.width);
                    } else {
                        self.inline(child, &own, &mut inline, cb, transform, depth + 1)?;
                    }
                }
                _ => {}
            }
        }
        let lines = self.lines(
            &mut inline,
            style,
            Flow {
                x: at.x,
                y: cursor,
                width: at.width,
            },
            transform,
        );
        cursor += lines.height;
        widest = widest.max(lines.width);
        Ok(Extent {
            height: cursor - at.y,
            width: widest,
        })
    }

    /// Gather an inline element's content into the current lines: its text,
    /// its margins and an empty inline block's width as spacers, and line
    /// breaks. What is positioned inside it is laid out on its own.
    fn inline(
        &mut self,
        node: NodeRef<'_, Node>,
        style: &PageStyle,
        items: &mut Vec<Item>,
        cb: Rect,
        transform: [f32; 6],
        depth: usize,
    ) -> anyhow::Result<()> {
        if depth > limits::HTML_DEPTH {
            return Ok(());
        }
        let font = style.font_size;
        let shared = Rc::new(style.clone());
        let e = node.value().as_element().expect("an element");
        if e.name() == "br" {
            items.push(Item::Break);
            return Ok(());
        }
        let margin = style.margin.map(|m| m.px(font, 0.).unwrap_or(0.));
        let padding = style.padding.map(|p| p.px(font, 0.).unwrap_or(0.));
        items.push(Item::Space(margin[3] + padding[3]));
        let before = items.len();
        for child in node.children() {
            match child.value() {
                Node::Text(text) => items.push(Item::Text(text.to_string(), shared.clone())),
                Node::Element(_) => {
                    let element = ElementRef::wrap(child).expect("an element");
                    let own = self.sheets.compute_page(element, style);
                    if own.display == PageDisplay::None {
                        continue;
                    }
                    if own.position == Position::Absolute {
                        let here = Flow {
                            x: cb.x,
                            y: cb.y,
                            width: cb.w,
                        };
                        self.block(child, &own, here, cb, transform, depth + 1)?;
                    } else {
                        self.inline(child, &own, items, cb, transform, depth + 1)?;
                    }
                }
                _ => {}
            }
        }
        // An inline block with nothing in it is its width: the spacer a
        // converter puts between two words.
        if style.display == PageDisplay::InlineBlock
            && items.len() == before
            && let Some(width) = style.width.px(font, 0.)
        {
            items.push(Item::Space(width));
        }
        items.push(Item::Space(margin[1] + padding[1]));
        Ok(())
    }

    /// Set the gathered inline content in lines from `at`, and draw them.
    fn lines(
        &mut self,
        items: &mut Vec<Item>,
        block: &PageStyle,
        at: Flow,
        transform: [f32; 6],
    ) -> Extent {
        if items.is_empty() {
            return Extent::default();
        }
        let items = std::mem::take(items);
        // Pieces: text in one style and one word, spaces, spacers, breaks.
        let mut pieces: Vec<Piece> = Vec::new();
        let mut space = true;
        for item in items {
            match item {
                Item::Break => {
                    pieces.push(Piece::Break);
                    space = true;
                }
                Item::Space(width) => {
                    if width != 0. {
                        pieces.push(Piece::Spacer(width));
                    }
                }
                Item::Text(text, style) => {
                    if style.pre {
                        for (i, line) in text.split('\n').enumerate() {
                            if i > 0 {
                                pieces.push(Piece::Break);
                            }
                            if !line.is_empty() {
                                pieces.push(Piece::Word(line.replace('\t', "    "), style.clone()));
                            }
                        }
                        space = text.ends_with([' ', '\n']);
                        continue;
                    }
                    let mut word = String::new();
                    for c in text.chars() {
                        if matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{c}') {
                            if !word.is_empty() {
                                pieces.push(Piece::Word(std::mem::take(&mut word), style.clone()));
                            }
                            if !space {
                                pieces.push(Piece::Space(style.clone()));
                                space = true;
                            }
                        } else {
                            word.push(c);
                            space = false;
                        }
                    }
                    if !word.is_empty() {
                        pieces.push(Piece::Word(word, style.clone()));
                    }
                }
            }
        }
        // Lines, greedily, where a word would pass the width of a box whose
        // text wraps.
        // A line counts when it holds something, or ends at a line break:
        // white space alone between blocks makes none.
        let mut lines: Vec<Vec<(f32, Piece)>> = vec![Vec::new()];
        let mut broken = vec![false];
        let mut x = 0.;
        for piece in pieces {
            let width = match &piece {
                Piece::Break => {
                    *broken.last_mut().expect("a line") = true;
                    lines.push(Vec::new());
                    broken.push(false);
                    x = 0.;
                    continue;
                }
                Piece::Spacer(width) => *width,
                Piece::Space(style) => self.advance(" ", style),
                Piece::Word(word, style) => self.advance(word, style),
            };
            let line = lines.last_mut().expect("a line");
            let wraps = match &piece {
                Piece::Word(_, style) => style.wrap,
                _ => false,
            };
            if wraps && x + width > at.width && !line.is_empty() {
                lines.push(Vec::new());
                broken.push(false);
                x = 0.;
            }
            let line = lines.last_mut().expect("a line");
            if matches!(piece, Piece::Space(_)) && line.is_empty() {
                continue;
            }
            line.push((x, piece));
            x += width;
        }
        let mut y = at.y;
        let mut widest: f32 = 0.;
        let strut = self.vertical(block);
        for (line, broken) in lines.into_iter().zip(broken) {
            // A line ends on its last word: trailing spaces take no room.
            let mut line = line;
            while matches!(line.last(), Some((_, Piece::Space(_)))) {
                line.pop();
            }
            if line.is_empty() && !broken {
                continue;
            }
            let width = line.last().map_or(0., |(x, piece)| {
                x + match piece {
                    Piece::Word(word, style) => self.advance(word, style),
                    Piece::Spacer(width) => *width,
                    _ => 0.,
                }
            });
            widest = widest.max(width);
            let (mut above, mut below) = strut;
            for (_, piece) in &line {
                if let Piece::Word(_, style) = piece {
                    let (a, b) = self.vertical(style);
                    above = above.max(a);
                    below = below.max(b);
                }
            }
            let baseline = y + above;
            let offset = if at.width.is_finite() {
                match block.align {
                    Align::Start => 0.,
                    Align::Center => (at.width - width) / 2.,
                    Align::End => at.width - width,
                }
            } else {
                0.
            };
            if self.drawing() {
                for (x, piece) in &line {
                    if let Piece::Word(word, style) = piece {
                        self.word(word, style, at.x + offset + x, baseline, transform);
                    }
                }
            }
            y += above + below;
        }
        Extent {
            height: y - at.y,
            width: widest,
        }
    }

    /// The width a word takes in its style.
    fn advance(&mut self, word: &str, style: &PageStyle) -> f32 {
        let Some(face) = self.fonts.face(style, self.resources) else {
            return 0.;
        };
        let size = style.font_size;
        let mut width = 0.;
        for c in word.chars() {
            width += self.fonts.advance(face, c) * size + style.letter_spacing;
            if c == ' ' {
                width += style.word_spacing;
            }
        }
        width
    }

    /// How far a line in `style` reaches above and below its baseline: the
    /// font's ascent and descent, with half the leading on each side.
    fn vertical(&mut self, style: &PageStyle) -> (f32, f32) {
        let size = style.font_size;
        let (ascent, descent, gap) =
            self.fonts
                .face(style, self.resources)
                .map_or((0.8, 0.2, 0.), |face| {
                    let metrics = self.fonts.metrics(face);
                    (metrics.ascent, metrics.descent, metrics.gap)
                });
        let height = match style.line_height {
            LineHeight::Normal => (ascent + descent + gap) * size,
            LineHeight::Number(number) => number * size,
            LineHeight::Length(length) => length.px(size, size).unwrap_or(size * 1.2),
        };
        let half = (height - (ascent + descent) * size) / 2.;
        (half + ascent * size, half + descent * size)
    }

    fn word(&mut self, word: &str, style: &PageStyle, x: f32, baseline: f32, transform: [f32; 6]) {
        if !style.visible || style.color[3] == 0 || self.text >= TEXT {
            return;
        }
        let Some(face) = self.fonts.face(style, self.resources) else {
            return;
        };
        self.text += word.len();
        let family = self.fonts.metrics(face).family.clone();
        let [r, g, b, a] = style.color;
        let _ = write!(
            self.canvas.svg(),
            r#"<text x="{x}" y="{baseline}" font-family="{}" font-size="{}" font-weight="{}" font-style="{}" fill="rgb({r},{g},{b})" fill-opacity="{}" letter-spacing="{}" word-spacing="{}" font-kerning="none" xml:space="preserve"{}>{}</text>"#,
            attribute(&family),
            style.font_size,
            if style.bold { 700 } else { 400 },
            if style.italic { "italic" } else { "normal" },
            f32::from(a) / 255.,
            style.letter_spacing,
            style.word_spacing,
            matrix(transform),
            escape(word),
        );
    }

    /// A picture: `<img>`, or an inline SVG (a cover page's frame).
    fn replaced(
        &mut self,
        node: NodeRef<'_, Node>,
        style: &PageStyle,
        at: Flow,
        cb: Rect,
        transform: [f32; 6],
    ) -> Extent {
        let e = node.value().as_element().expect("an element");
        let font = style.font_size;
        let absolute = style.position == Position::Absolute;
        let base = if absolute {
            cb
        } else {
            Rect {
                x: at.x,
                y: at.y,
                w: at.width,
                h: cb.h,
            }
        };
        let attribute_length = |name: &str, base: f32| {
            e.attr(name).and_then(|value| {
                let value = value.trim();
                match value.strip_suffix('%') {
                    Some(percent) => percent.parse::<f32>().ok().map(|p| p / 100. * base),
                    None => value.trim_end_matches("px").parse::<f32>().ok(),
                }
            })
        };
        let mut width = style
            .width
            .px(font, base.w)
            .or_else(|| attribute_length("width", base.w));
        let mut height = style
            .height
            .px(font, base.h)
            .or_else(|| attribute_length("height", base.h));
        let (source, natural) = if e.name() == "svg" {
            let natural = e.attr("viewBox").and_then(|view| {
                let numbers: Vec<f32> = view
                    .split([' ', ','])
                    .filter_map(|n| n.parse().ok())
                    .collect();
                (numbers.len() == 4).then(|| (numbers[2], numbers[3]))
            });
            (None, natural)
        } else {
            let href = e
                .attr("src")
                .or_else(|| e.attrs().find(|(name, _)| *name == "href").map(|(_, v)| v));
            let key = href.and_then(|href| self.picture(href));
            let natural = key
                .as_ref()
                .and_then(|key| self.pictures.get(key))
                .and_then(|bytes| dimensions(bytes));
            (key, natural)
        };
        if let Some((nw, nh)) = natural.filter(|(w, h)| *w > 0. && *h > 0.) {
            match (width, height) {
                (None, None) => (width, height) = (Some(nw), Some(nh)),
                (Some(w), None) => height = Some(w * nh / nw),
                (None, Some(h)) => width = Some(h * nw / nh),
                _ => {}
            }
        }
        let width = width.unwrap_or(base.w);
        let height = height.unwrap_or(base.h);
        let left = style.offsets[3].px(font, base.w);
        let right = style.offsets[1].px(font, base.w);
        let top = style.offsets[0].px(font, cb.h);
        let bottom = style.offsets[2].px(font, cb.h);
        let margin = style.margin.map(|m| m.px(font, base.w).unwrap_or(0.));
        let x = match (absolute, left, right) {
            (true, Some(left), _) => cb.x + left + margin[3],
            (true, None, Some(right)) => cb.x + cb.w - right - width - margin[1],
            _ => at.x + margin[3],
        };
        let y = match (absolute, top, bottom) {
            (true, Some(top), _) => cb.y + top + margin[0],
            (true, None, Some(bottom)) => cb.y + cb.h - bottom - height - margin[2],
            _ => at.y + margin[0],
        };
        if self.drawing() && style.visible {
            let opened = self.open_group(style);
            if e.name() == "svg" {
                self.svg_frame(node, [x, y, width, height], transform);
            } else if let Some(key) = source {
                self.canvas.picture(key, [x, y, width, height], transform);
            }
            if opened {
                self.canvas.close();
            }
        }
        Extent {
            height: if absolute {
                0.
            } else {
                height + margin[0] + margin[2]
            },
            width,
        }
    }

    /// Read a picture the page names, once, under a key the SVG uses.
    fn picture(&mut self, href: &str) -> Option<String> {
        let key = format!("picture:{href}");
        if self.pictures.contains_key(&key) {
            return Some(key);
        }
        if self.pictures.len() >= PICTURES || href.contains(':') {
            return None;
        }
        let bytes = self.resources.file(href)?;
        image::guess_format(&bytes).ok()?;
        self.pictures.insert(key.clone(), Arc::new(bytes));
        Some(key)
    }

    /// An SVG the page draws with, nested at its box. Its pictures (a cover
    /// page's frame holds one) are placed on the page through the frame's
    /// `viewBox` and drawn after the rest of it.
    fn svg_frame(&mut self, node: NodeRef<'_, Node>, frame: [f32; 4], transform: [f32; 6]) {
        let e = node.value().as_element().expect("an element");
        let [x, y, width, height] = frame;
        let view = e.attr("viewBox").and_then(|view| {
            let numbers: Vec<f32> = view
                .split([' ', ','])
                .filter_map(|n| n.trim().parse().ok())
                .collect();
            (numbers.len() == 4 && numbers[2] > 0. && numbers[3] > 0.)
                .then(|| [numbers[0], numbers[1], numbers[2], numbers[3]])
        });
        // The frame's user space on the page: `sx sy tx ty`.
        let (sx, sy, tx, ty) = match view {
            None => (1., 1., x, y),
            Some([vx, vy, vw, vh]) => {
                let aspect = e.attr("preserveAspectRatio").unwrap_or("xMidYMid meet");
                if aspect.trim() == "none" {
                    let (sx, sy) = (width / vw, height / vh);
                    (sx, sy, x - vx * sx, y - vy * sy)
                } else {
                    let (fit, align) = (aspect.contains("slice"), aspect.trim());
                    let scale = if fit {
                        (width / vw).max(height / vh)
                    } else {
                        (width / vw).min(height / vh)
                    };
                    let spare = (width - vw * scale, height - vh * scale);
                    let dx = if align.starts_with("xMin") {
                        0.
                    } else if align.starts_with("xMax") {
                        spare.0
                    } else {
                        spare.0 / 2.
                    };
                    let dy = if align.contains("YMin") {
                        0.
                    } else if align.contains("YMax") {
                        spare.1
                    } else {
                        spare.1 / 2.
                    };
                    (scale, scale, x + dx - vx * scale, y + dy - vy * scale)
                }
            }
        };
        let mut pictures = Vec::new();
        let svg = self.canvas.svg();
        let _ = write!(svg, "<g{}>", matrix(transform));
        let _ = write!(
            svg,
            r#"<svg x="{x}" y="{y}" width="{width}" height="{height}""#
        );
        for name in ["viewBox", "preserveAspectRatio"] {
            if let Some(value) = e.attr(name) {
                let _ = write!(svg, r#" {name}="{}""#, attribute(value));
            }
        }
        svg.push('>');
        let mut inside = String::new();
        for child in node.children() {
            svg_node(child, &mut inside, &mut pictures);
        }
        let svg = self.canvas.svg();
        svg.push_str(&inside);
        svg.push_str("</svg></g>");
        for (href, [ix, iy, iw, ih]) in pictures {
            if let Some(key) = self.picture(&href) {
                let rect = [tx + ix * sx, ty + iy * sy, iw * sx, ih * sy];
                self.canvas.picture(key, rect, transform);
            }
        }
    }
}

/// Write an SVG frame's element as it is, but for its pictures, which are
/// set aside with their place (`x y width height` in the frame's units).
fn svg_node(node: NodeRef<'_, Node>, out: &mut String, pictures: &mut Vec<(String, [f32; 4])>) {
    match node.value() {
        Node::Text(text) => out.push_str(&escape(text)),
        Node::Element(e) if e.name() == "image" => {
            let number = |name: &str| {
                e.attr(name)
                    .and_then(|value| value.trim().trim_end_matches("px").parse::<f32>().ok())
            };
            if let Some(href) = e.attrs().find(|(name, _)| *name == "href").map(|(_, v)| v) {
                pictures.push((
                    href.to_owned(),
                    [
                        number("x").unwrap_or(0.),
                        number("y").unwrap_or(0.),
                        number("width").unwrap_or(0.),
                        number("height").unwrap_or(0.),
                    ],
                ));
            }
        }
        Node::Element(e) => {
            let _ = write!(out, "<{}", e.name());
            for (name, value) in e.attrs() {
                if name.contains(':') || name.starts_with("xmlns") || name == "href" {
                    continue;
                }
                let _ = write!(out, r#" {name}="{}""#, attribute(value));
            }
            out.push('>');
            for child in node.children() {
                svg_node(child, out, pictures);
            }
            let _ = write!(out, "</{}>", e.name());
        }
        _ => {}
    }
}

/// Inline content as gathered: text in a style, a spacer, a line break.
enum Item {
    Text(String, Rc<PageStyle>),
    Space(f32),
    Break,
}

/// Inline content as set: a word, a collapsible space, a spacer, a break.
enum Piece {
    Word(String, Rc<PageStyle>),
    Space(Rc<PageStyle>),
    Spacer(f32),
    Break,
}

/// The fonts a page sets text in: the book's, loaded when a style first
/// names them and renamed to the family the CSS gives them, and the
/// system's for the rest.
struct Fonts {
    /// The system's, shared, made a page's own only when the page brings
    /// fonts of its own (`load`): copied for every page drawn, the whole
    /// list of the system's faces was.
    db: Arc<fontdb::Database>,
    faces: Vec<super::style::FontFace>,
    loaded: Vec<String>,
    metrics: HashMap<fontdb::ID, Metrics>,
    chosen: HashMap<(Rc<str>, bool, bool), Option<fontdb::ID>>,
}

/// What laying text out needs of a face, in ems.
struct Metrics {
    family: String,
    ascent: f32,
    descent: f32,
    gap: f32,
    units: f32,
    advances: HashMap<char, f32>,
}
impl Metrics {
    fn advance(&mut self, db: &fontdb::Database, face: fontdb::ID, c: char) -> f32 {
        if let Some(advance) = self.advances.get(&c) {
            return *advance;
        }
        let units = self.units;
        let advance = db
            .with_face_data(face, |data, index| {
                use skrifa::MetadataProvider;
                let font = skrifa::FontRef::from_index(data, index).ok()?;
                let glyph = font.charmap().map(c)?;
                let unscaled = skrifa::instance::Size::unscaled();
                let metrics =
                    font.glyph_metrics(unscaled, skrifa::instance::LocationRef::default());
                Some(metrics.advance_width(glyph)? / units)
            })
            .flatten()
            .unwrap_or(0.5);
        self.advances.insert(c, advance);
        advance
    }
}

impl Fonts {
    fn new(faces: Vec<super::style::FontFace>) -> Self {
        Self {
            // The system's, for text whose book brings no font.
            db: Arc::clone(crate::inspection::system_fonts()),
            faces,
            loaded: Vec::new(),
            metrics: HashMap::new(),
            chosen: HashMap::new(),
        }
    }

    /// The face a style's text is set in: its first family the book or the
    /// system has, else any sans-serif face.
    fn face(&mut self, style: &PageStyle, resources: &mut impl Resources) -> Option<fontdb::ID> {
        let key: Rc<str> = style.families.join(",").into();
        if let Some(chosen) = self.chosen.get(&(key.clone(), style.bold, style.italic)) {
            return *chosen;
        }
        let weight = if style.bold {
            fontdb::Weight::BOLD
        } else {
            fontdb::Weight::NORMAL
        };
        let slant = if style.italic {
            fontdb::Style::Italic
        } else {
            fontdb::Style::Normal
        };
        let mut chosen = None;
        for family in style.families.iter() {
            self.load(family, resources);
            let family = match family.to_ascii_lowercase().as_str() {
                "serif" => fontdb::Family::Serif,
                "sans-serif" | "system-ui" => fontdb::Family::SansSerif,
                "monospace" => fontdb::Family::Monospace,
                "cursive" => fontdb::Family::Cursive,
                "fantasy" => fontdb::Family::Fantasy,
                _ => fontdb::Family::Name(family),
            };
            chosen = self.db.query(&fontdb::Query {
                families: &[family],
                weight,
                style: slant,
                stretch: fontdb::Stretch::Normal,
            });
            if chosen.is_some() {
                break;
            }
        }
        let chosen = chosen.or_else(|| {
            ["DejaVu Sans", "Liberation Sans", "Noto Sans", "Cantarell"]
                .iter()
                .find_map(|name| {
                    self.db.query(&fontdb::Query {
                        families: &[fontdb::Family::Name(name)],
                        weight,
                        style: slant,
                        stretch: fontdb::Stretch::Normal,
                    })
                })
                .or_else(|| self.db.faces().next().map(|face| face.id))
        });
        self.chosen.insert((key, style.bold, style.italic), chosen);
        chosen
    }

    /// Load the book's fonts of `family`, if it brings any, under that name.
    fn load(&mut self, family: &str, resources: &mut impl Resources) {
        if self
            .loaded
            .iter()
            .any(|loaded| loaded.eq_ignore_ascii_case(family))
        {
            return;
        }
        self.loaded.push(family.to_owned());
        let faces: Vec<_> = self
            .faces
            .iter()
            .filter(|face| face.family.eq_ignore_ascii_case(family))
            .cloned()
            .collect();
        for face in faces {
            let Some(bytes) = face.src.iter().find_map(|src| resources.file(src)) else {
                continue;
            };
            let db = Arc::make_mut(&mut self.db);
            let ids = db.load_font_source(fontdb::Source::Binary(Arc::new(bytes)));
            for id in ids {
                let Some(mut info) = db.face(id).cloned() else {
                    continue;
                };
                db.remove_face(id);
                info.families = vec![(family.to_owned(), fontdb::Language::English_UnitedStates)];
                info.weight = if face.bold {
                    fontdb::Weight::BOLD
                } else {
                    fontdb::Weight::NORMAL
                };
                info.style = if face.italic {
                    fontdb::Style::Italic
                } else {
                    fontdb::Style::Normal
                };
                db.push_face_info(info);
            }
        }
    }

    /// The advance of `c` in `face`, in ems.
    fn advance(&mut self, face: fontdb::ID, c: char) -> f32 {
        self.metrics(face);
        let db = &self.db;
        self.metrics
            .get_mut(&face)
            .map_or(0.5, |metrics| metrics.advance(db, face, c))
    }

    fn metrics(&mut self, face: fontdb::ID) -> &mut Metrics {
        let db = &self.db;
        self.metrics.entry(face).or_insert_with(|| {
            let family = db
                .face(face)
                .and_then(|info| info.families.first())
                .map_or_else(String::new, |(family, _)| family.clone());
            let (units, ascent, descent, gap) = db
                .with_face_data(face, |data, index| {
                    use skrifa::MetadataProvider;
                    let font = skrifa::FontRef::from_index(data, index).ok()?;
                    let metrics = font.metrics(
                        skrifa::instance::Size::unscaled(),
                        skrifa::instance::LocationRef::default(),
                    );
                    let units = f32::from(metrics.units_per_em).max(1.);
                    Some((
                        units,
                        metrics.ascent / units,
                        -metrics.descent / units,
                        metrics.leading / units,
                    ))
                })
                .flatten()
                .unwrap_or((1000., 0.8, 0.2, 0.));
            Metrics {
                family,
                ascent,
                descent,
                gap,
                units,
                advances: HashMap::new(),
            }
        })
    }
}

/// A picture's own size, read from its header, turned as it is drawn.
fn dimensions(bytes: &[u8]) -> Option<(f32, f32)> {
    let (width, height) = crate::picture::size(
        image::ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()
            .ok()?,
    )
    .ok()?;
    Some((width as f32, height as f32))
}

fn matrix(transform: [f32; 6]) -> String {
    if transform == IDENTITY {
        String::new()
    } else {
        let [a, b, c, d, e, f] = transform;
        format!(r#" transform="matrix({a} {b} {c} {d} {e} {f})""#)
    }
}
fn attribute(value: &str) -> String {
    escape(value).replace('"', "&quot;")
}

/// Draw the page with `side` pixels on its longer side, on white paper,
/// layer by layer: its width, height and pixels.
/// Its layers are drawn until `cancel`; a picture used several times is
/// decoded once (within `DECODED_BYTES`).
fn render(
    canvas: Canvas,
    drawn: Drawn,
    side: u32,
    cancel: &CancelToken,
) -> anyhow::Result<(u32, u32, Vec<u8>)> {
    let scale = side as f32 / drawn.width.max(drawn.height);
    let width = (drawn.width * scale).round().max(1.) as u32;
    let height = (drawn.height * scale).round().max(1.) as u32;
    let mut options = usvg::Options {
        fontdb: drawn.fonts,
        ..usvg::Options::default()
    };
    options.image_href_resolver.resolve_string = std::boxed::Box::new(|_, _| None);
    let mut pixmap =
        tiny_skia::Pixmap::new(width, height).ok_or_else(|| anyhow::anyhow!("page has no size"))?;
    pixmap.fill(tiny_skia::Color::WHITE);
    let view = tiny_skia::Transform::from_scale(scale, scale);
    let mut decoded: HashMap<String, Option<image::DynamicImage>> = HashMap::new();
    let mut decoded_bytes = 0usize;
    for layer in canvas.layers {
        check(cancel)?;
        match layer {
            Layer::Svg(fragment) => {
                if fragment.trim().is_empty() {
                    continue;
                }
                let document = format!(
                    r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="{}" height="{}" viewBox="0 0 {width} {height}">{fragment}</svg>"#,
                    drawn.width,
                    drawn.height,
                    width = drawn.width,
                    height = drawn.height,
                );
                crate::inspection::xml_depth(document.as_bytes())?;
                let tree = usvg::Tree::from_str(&document, &options)?;
                resvg::render(&tree, view, &mut pixmap.as_mut());
            }
            Layer::Picture {
                key,
                rect,
                transform,
                opacity,
            } => {
                let Some(bytes) = drawn.pictures.get(&key) else {
                    continue;
                };
                if let Some(picture) = decoded.get(&key) {
                    if let Some(picture) = picture {
                        draw(&mut pixmap, picture.clone(), rect, transform, opacity, view);
                    }
                    continue;
                }
                let picture = decode_picture(bytes);
                if let Some(picture) = &picture {
                    draw(&mut pixmap, picture.clone(), rect, transform, opacity, view);
                }
                // Kept for its next use, within the bound.
                let held = picture.as_ref().map_or(0, |p| p.as_bytes().len());
                if decoded_bytes + held <= DECODED_BYTES {
                    decoded_bytes += held;
                    decoded.insert(key, picture);
                }
            }
        }
    }
    let rgba: Vec<u8> = pixmap
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let color = pixel.demultiply();
            [color.red(), color.green(), color.blue(), color.alpha()]
        })
        .collect();
    Ok((width, height, rgba))
}

/// Pictures decoded kept for their next use on a page, at most.
const DECODED_BYTES: usize = 64 << 20;

/// A picture decoded within the bounds of every embedded picture.
fn decode_picture(bytes: &[u8]) -> Option<image::DynamicImage> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut bounds = image::Limits::default();
    bounds.max_image_width = Some(limits::IMAGE_SIDE);
    bounds.max_image_height = Some(limits::IMAGE_SIDE);
    bounds.max_alloc = Some(limits::IMAGE_ALLOC);
    reader.limits(bounds);
    crate::picture::decode(reader).ok()
}

/// Draw a picture into `rect` under `transform`, first shrunk to about the
/// size it covers.
fn draw(
    pixmap: &mut tiny_skia::Pixmap,
    mut picture: image::DynamicImage,
    [x, y, width, height]: [f32; 4],
    transform: [f32; 6],
    opacity: f32,
    view: tiny_skia::Transform,
) -> Option<()> {
    let [a, b, c, d, e, f] = transform;
    let place = view
        .pre_concat(tiny_skia::Transform::from_row(a, b, c, d, e, f))
        .pre_translate(x, y);
    let (sx, sy) = place.get_scale();
    let covered = (
        (width * sx).ceil().max(1.) as u32,
        (height * sy).ceil().max(1.) as u32,
    );
    if picture.width() > covered.0.saturating_mul(2)
        && picture.height() > covered.1.saturating_mul(2)
    {
        picture = picture.thumbnail_exact(covered.0.saturating_mul(2), covered.1.saturating_mul(2));
    }
    let (w, h) = (picture.width(), picture.height());
    let mut rgba = picture.into_rgba8().into_raw();
    // tiny-skia draws premultiplied colour.
    for pixel in rgba.as_chunks_mut::<4>().0 {
        let alpha = u16::from(pixel[3]);
        for channel in &mut pixel[..3] {
            *channel = ((u16::from(*channel) * alpha + 127) / 255) as u8;
        }
    }
    let source = tiny_skia::Pixmap::from_vec(rgba, tiny_skia::IntSize::from_wh(w, h)?)?;
    let paint = tiny_skia::PixmapPaint {
        opacity,
        quality: tiny_skia::FilterQuality::Bilinear,
        ..tiny_skia::PixmapPaint::default()
    };
    let transform = place.pre_scale(width / w as f32, height / h as f32);
    pixmap.draw_pixmap(0, 0, source.as_ref(), &paint, transform, None);
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A font whose space, `A` and `B` advance half an em, `A` being a
    /// square 0.5 em wide and 0.7 em tall on the baseline; ascent 0.8 em,
    /// descent 0.2 em. Named `Synthetic` inside, whatever the CSS calls it.
    fn font() -> Vec<u8> {
        fn be16(out: &mut Vec<u8>, v: i32) {
            out.extend_from_slice(&(v as u16).to_be_bytes());
        }
        fn be32(out: &mut Vec<u8>, v: u32) {
            out.extend_from_slice(&v.to_be_bytes());
        }
        let mut head = Vec::new();
        be32(&mut head, 0x0001_0000);
        be32(&mut head, 0x0001_0000);
        be32(&mut head, 0);
        be32(&mut head, 0x5F0F_3CF5);
        be16(&mut head, 0);
        be16(&mut head, 1000);
        head.extend_from_slice(&[0; 16]);
        for v in [0, -200, 500, 800, 0, 8, 2, 0, 0] {
            be16(&mut head, v);
        }
        let mut hhea = Vec::new();
        be32(&mut hhea, 0x0001_0000);
        for v in [800, -200, 0, 500, 0, 0, 500, 1, 0, 0, 0, 0, 0, 0, 0, 1] {
            be16(&mut hhea, v);
        }
        let mut maxp = Vec::new();
        be32(&mut maxp, 0x0000_5000);
        be16(&mut maxp, 4);
        let mut hmtx = Vec::new();
        for v in [500, 0, 0, 0, 0] {
            be16(&mut hmtx, v);
        }
        // cmap: space → 1, `A` → 2, `B` → 3.
        let mut cmap = Vec::new();
        for v in [0, 1, 3, 1] {
            be16(&mut cmap, v);
        }
        be32(&mut cmap, 12);
        let segments = [
            (0x20, 0x20, 1 - 0x20),
            (0x41, 0x42, 2 - 0x41),
            (0xFFFF, 0xFFFF, 1),
        ];
        let mut sub = Vec::new();
        for v in [4, 0, 0, 6, 4, 1, 2] {
            be16(&mut sub, v);
        }
        for (_, end, _) in segments {
            be16(&mut sub, end);
        }
        be16(&mut sub, 0);
        for (start, _, _) in segments {
            be16(&mut sub, start);
        }
        for (_, _, delta) in segments {
            be16(&mut sub, delta);
        }
        for _ in segments {
            be16(&mut sub, 0);
        }
        let length = sub.len() as u16;
        sub[2..4].copy_from_slice(&length.to_be_bytes());
        cmap.extend(sub);
        let mut glyf = Vec::new();
        for v in [1, 0, 0, 500, 700, 3, 0] {
            be16(&mut glyf, v);
        }
        glyf.extend_from_slice(&[1, 1, 1, 1]);
        for v in [0, 0, 500, 0, 0, 700, 0, -700] {
            be16(&mut glyf, v);
        }
        let mut loca = Vec::new();
        for v in [0, 0, 0, 17, 17] {
            be16(&mut loca, v);
        }
        let names = ["Synthetic", "Synthetic-Regular"];
        let mut name = Vec::new();
        for v in [0, 2, 6 + 12 * 2] {
            be16(&mut name, v);
        }
        let mut strings = Vec::new();
        for (id, text) in [1, 6].into_iter().zip(names) {
            let utf16: Vec<u8> = text.encode_utf16().flat_map(u16::to_be_bytes).collect();
            let (length, offset) = (
                i32::try_from(utf16.len()).unwrap(),
                i32::try_from(strings.len()).unwrap(),
            );
            for v in [3, 1, 0x409, id, length, offset] {
                be16(&mut name, v);
            }
            strings.extend(utf16);
        }
        name.extend(strings);
        let tables = [
            (*b"cmap", cmap),
            (*b"glyf", glyf),
            (*b"head", head),
            (*b"hhea", hhea),
            (*b"hmtx", hmtx),
            (*b"loca", loca),
            (*b"maxp", maxp),
            (*b"name", name),
        ];
        let mut out = Vec::new();
        be32(&mut out, 0x0001_0000);
        for v in [8, 0, 0, 0] {
            be16(&mut out, v);
        }
        let mut offset = 12 + 16 * tables.len();
        let mut body = Vec::new();
        for (tag, data) in &tables {
            out.extend_from_slice(tag);
            be32(&mut out, 0);
            be32(&mut out, offset as u32);
            be32(&mut out, data.len() as u32);
            let mut data = data.clone();
            while data.len() % 4 != 0 {
                data.push(0);
            }
            offset += data.len();
            body.extend(data);
        }
        out.extend(body);
        out
    }

    fn png(width: u32, height: u32, color: [u8; 4]) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(width, height, image::Rgba(color))
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
        out.into_inner()
    }

    /// A book's files by name.
    struct Files(HashMap<&'static str, Vec<u8>>);
    impl Resources for Files {
        fn picture(&mut self, _: &str) -> Option<skwir_core::inspection::Picture> {
            None
        }
        fn stylesheet(&mut self, href: &str) -> Option<String> {
            String::from_utf8(self.0.get(href)?.clone()).ok()
        }
        fn file(&mut self, href: &str) -> Option<Vec<u8>> {
            self.0.get(href).cloned()
        }
    }
    fn files() -> Files {
        Files(HashMap::from([
            ("fonts/f.ttf", font()),
            ("bg.png", png(20, 10, [200, 30, 30, 255])),
            (
                "style.css",
                br#"@font-face { font-family: ff1; src: url(fonts/f.ttf) format("truetype") }
                .pf { position: relative; width: 200px; height: 100px }
                .bi { position: absolute; left: 0; bottom: 0; width: 200px; height: 100px }
                .t { position: absolute; white-space: pre; font-size: 1px;
                     transform-origin: 0 100%; font-family: ff1; line-height: 1 }
                .m { transform: matrix(0.5, 0, 0, 0.5, 0, 0) } .fs { font-size: 20px }
                .x { left: 10px } .y { bottom: 20px } .h { height: 10px }
                ._ { display: inline-block } ._7 { width: 7px } .gone { visibility: hidden }"#
                    .to_vec(),
            ),
        ]))
    }
    /// A page as a PDF converter writes it: its picture, and a line of
    /// lettering placed by its bottom, scaled from its bottom-left corner,
    /// with a spacer between two words.
    const CONVERTED: &str = r#"<?xml version="1.0" encoding="utf-8"?>
        <html xmlns="http://www.w3.org/1999/xhtml"><head><meta name="viewport" content="width=200,height=100"/>
        <link rel="stylesheet" type="text/css" href="style.css"/><title/></head>
        <body><div class="pf"><img class="bi" alt="" src="bg.png"/>
        <div class="t m fs x y h">A<span class="_ _7"></span>B<span class="gone">hidden</span></div></div></body></html>"#;

    fn laid_out(source: &str, resources: &mut Files) -> (Canvas, Drawn) {
        layout(source, Markup::Xhtml, &CancelToken::new(), resources, None)
            .unwrap()
            .expect("a page with a viewport")
    }
    fn fragments(canvas: &Canvas) -> String {
        canvas
            .layers
            .iter()
            .filter_map(|layer| match layer {
                Layer::Svg(svg) => Some(svg.as_str()),
                Layer::Picture { .. } => None,
            })
            .collect()
    }

    #[test]
    fn a_converted_page_sets_its_lettering_where_the_page_says() {
        let (canvas, drawn) = laid_out(CONVERTED, &mut files());
        assert_eq!((drawn.width, drawn.height), (200., 100.));
        // The picture fills the page, under the text.
        let pictures: Vec<_> = canvas
            .layers
            .iter()
            .filter_map(|layer| match layer {
                Layer::Picture { rect, .. } => Some(*rect),
                Layer::Svg(_) => None,
            })
            .collect();
        assert_eq!(pictures, [[0., 0., 200., 100.]]);
        let svg = fragments(&canvas);
        // The box stands 20 px from the bottom and is 10 px tall: its top is
        // at 70, its baseline 0.8 em (16 px) under it, the line being 1 em.
        // The words are half an em apart plus the 7 px spacer; the scale
        // turns about the box's bottom-left corner (10, 80).
        assert!(
            svg.contains(r#"<text x="10" y="86" font-family="ff1""#),
            "{svg}"
        );
        assert!(
            svg.contains(r#"<text x="27" y="86" font-family="ff1""#),
            "{svg}"
        );
        assert!(
            svg.contains(r#"transform="matrix(0.5 0 0 0.5 5 40)""#),
            "{svg}"
        );
        assert!(
            !svg.contains("hidden"),
            "hidden text takes its place, unseen"
        );
    }

    #[test]
    fn the_page_is_drawn_with_the_books_font_over_its_picture() {
        let (canvas, drawn) = laid_out(CONVERTED, &mut files());
        // The 200 × 100 page drawn with 800 pixels on its longer side.
        let (width, height, rgba) = render(canvas, drawn, 800, &CancelToken::new()).unwrap();
        assert_eq!((width, height), (800, 400));
        let at = |x: u32, y: u32| {
            let i = ((y * width + x) * 4) as usize;
            [rgba[i], rgba[i + 1], rgba[i + 2]]
        };
        // The picture, away from the text.
        assert_eq!(at(600, 100), [200, 30, 30]);
        // `A`'s square: 10..20 × 72..86 before the scale, 10..15 × 76..83
        // after it, in page pixels; its middle, four times over.
        let [r, g, b] = at(50, 318);
        assert!(
            r < 60 && g < 60 && b < 60,
            "the letter is drawn: {:?}",
            [r, g, b]
        );
    }

    #[test]
    fn a_page_is_drawn_at_the_side_asked_and_keeps_its_own_size() {
        // Twice the side: twice the pixels, the same letter, the same page.
        let drawing = page(
            CONVERTED,
            Markup::Xhtml,
            &CancelToken::new(),
            files(),
            1600,
            None,
        )
        .unwrap()
        .expect("a page with a viewport");
        assert_eq!((drawing.width, drawing.height), (1600, 800));
        assert_eq!(drawing.own, (200., 100.));
        let i = ((636 * drawing.width + 100) * 4) as usize;
        assert!(drawing.rgba[i] < 60, "the letter is drawn at the new scale");
    }

    #[test]
    fn a_cover_frame_places_its_picture_through_its_view_box() {
        let page = r#"<html xmlns="http://www.w3.org/1999/xhtml"><head>
            <meta name="viewport" content="width=300, height=400"/></head>
            <body style="margin: 0"><div><svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink"
              width="100%" height="100%" viewBox="0 0 600 800" preserveAspectRatio="xMidYMid meet">
              <image width="600" height="800" xlink:href="bg.png"/></svg></div></body></html>"#;
        let (canvas, _) = laid_out(page, &mut files());
        let pictures: Vec<_> = canvas
            .layers
            .iter()
            .filter_map(|layer| match layer {
                Layer::Picture { rect, .. } => Some(*rect),
                Layer::Svg(_) => None,
            })
            .collect();
        assert_eq!(pictures, [[0., 0., 300., 400.]]);
    }

    #[test]
    fn a_page_without_a_size_takes_the_one_given_for_it() {
        let page = "<html><head></head><body><p>Text</p></body></html>";
        let cancel = CancelToken::new();
        assert!(
            layout(page, Markup::Html, &cancel, &mut files(), None)
                .unwrap()
                .is_none()
        );
        let (_, drawn) = layout(
            page,
            Markup::Html,
            &cancel,
            &mut files(),
            Some((300., 400.)),
        )
        .unwrap()
        .expect("drawn at the size given");
        assert_eq!((drawn.width, drawn.height), (300., 400.));
    }
}
