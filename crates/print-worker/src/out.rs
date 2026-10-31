//! The PDF written: pages laid out are drawn in turn, those asked only,
//! each with the name above and its number below; what each page draws
//! is marked for a screen reader as it is drawn and its structure put in
//! the document's tag tree in reading order; headings become the
//! outline. A page is finished (and compressed) before the next starts:
//! the drawing of only one is held at a time.

use std::collections::HashMap;
use std::ops::Range;

use anyhow::{Context, Result, bail};
use krilla::action::LinkAction;
use krilla::annotation::{Annotation, LinkAnnotation, Target};
use krilla::destination::XyzDestination;
use krilla::geom::{Point, Rect};
use krilla::metadata::Metadata;
use krilla::outline::{Outline, OutlineNode};
use krilla::page::PageSettings;
use krilla::surface::Surface;
use krilla::tagging::{
    Artifact, ArtifactType, ContentTag, Identifier, Node, SpanTag, TagGroup, TagKind, TagTree,
};
use krilla::{Document, SerializeSettings};
use parley::Layout;
use skwir_core::print::{Job, Made};

use crate::fonts::{Fonts, Role};
use crate::geometry::{self, BAND_SIZE, Frame};
use crate::text::{self, GREY, Ink, Mark, Spec, Wrap};

/// What one part of a job became, laid out: its pages, drawn on demand.
pub trait Part {
    /// Pages laid out.
    fn pages(&self) -> usize;
    /// Its page `page` (from its first) drawn.
    fn draw(&mut self, page: usize, cx: &mut PageCx) -> Result<()>;
    /// What it left open in the tag tree closed, before another part
    /// draws.
    fn finish(&mut self, cx: &mut PageCx);
}

/// A heading: its level, its text, the page it is on (of the whole job)
/// and how far down.
pub struct Heading {
    pub level: u8,
    pub title: String,
    pub page: usize,
    pub y: f32,
}

/// What a link inside the text can lead to: the pages written, by their
/// page laid out, and where each name (`#id`) is.
pub struct Links {
    pub written: Vec<Option<usize>>,
    pub anchors: HashMap<String, (usize, f32)>,
    pub x: f32,
}

/// The schemes a link of a document leads to: places on the web and mail.
const SCHEMES: &[&str] = &["http:", "https:", "mailto:", "ftp:", "ftps:", "tel:"];

impl Links {
    fn target(&self, link: &str) -> Option<Target> {
        if let Some(name) = link.strip_prefix('#') {
            let (page, y) = *self.anchors.get(name)?;
            let page = (*self.written.get(page)?)?;
            return Some(Target::Destination(
                XyzDestination::new(page, Point::from_xy(self.x, y)).into(),
            ));
        }
        let lower = link.trim().to_ascii_lowercase();
        SCHEMES
            .iter()
            .any(|scheme| lower.starts_with(scheme))
            .then(|| Target::Action(LinkAction::new(link.trim().to_owned()).into()))
    }
}

/// What drawing a page says of its structure, played into the tag tree
/// once the page is finished and its links' annotations made.
pub enum Event {
    Open(TagKind),
    Close,
    Leaf(Identifier),
    /// The annotation of the page's link of this index.
    Annot(usize),
}

/// A page being drawn and what it says.
pub struct PageCx<'s, 'p> {
    pub surface: &'s mut Surface<'p>,
    pub fonts: &'s mut Fonts,
    pub frame: Frame,
    pub links: &'s Links,
    events: Vec<Event>,
    annots: Vec<(Target, Rect, String)>,
}

impl PageCx<'_, '_> {
    pub fn open(&mut self, tag: impl Into<TagKind>) {
        self.events.push(Event::Open(tag.into()));
    }

    pub fn close(&mut self) {
        self.events.push(Event::Close);
    }

    /// What `draw` draws, no part of the document's text: a background,
    /// a rule, the name above, a line's number.
    pub fn artifact(&mut self, kind: ArtifactType, draw: impl FnOnce(&mut Surface, &mut Fonts)) {
        self.surface
            .start_tagged(ContentTag::Artifact(Artifact::with_kind(kind)));
        draw(self.surface, self.fonts);
        self.surface.end_tagged();
    }

    /// What `draw` draws, marked as content (a picture, a drawing) and
    /// put where the structure stands.
    pub fn content(&mut self, draw: impl FnOnce(&mut Surface, &mut Fonts)) {
        let id = self.surface.start_tagged(ContentTag::Other);
        draw(self.surface, self.fonts);
        self.surface.end_tagged();
        self.events.push(Event::Leaf(id));
    }

    /// What `draw` draws, marked as text where the structure stands, in
    /// one stretch: a page of code.
    pub fn span(&mut self, draw: impl FnOnce(&mut Surface, &mut Fonts)) {
        let id = self
            .surface
            .start_tagged(ContentTag::Span(SpanTag::empty()));
        draw(self.surface, self.fonts);
        self.surface.end_tagged();
        self.events.push(Event::Leaf(id));
    }

    /// Lines `rows` of `layout` (of `text`, its links `links`) drawn
    /// with the layout's top-left at `origin`, their text marked where
    /// the structure stands, each stretch of a link in a link of its own.
    pub fn lines(
        &mut self,
        layout: &Layout<Ink>,
        text: &str,
        links: &[String],
        rows: Range<usize>,
        origin: (f32, f32),
    ) {
        let PageCx {
            surface,
            fonts,
            links: resolve,
            events,
            annots,
            ..
        } = self;
        let mut link_open: Option<usize> = None;
        for index in rows {
            text::draw_line(
                surface,
                fonts,
                layout,
                text,
                index,
                origin,
                &mut |surface, backs| {
                    surface.start_tagged(ContentTag::Artifact(Artifact::with_kind(
                        ArtifactType::Layout,
                    )));
                    for back in backs {
                        text::rect(surface, *back, text::CODE_BACK);
                    }
                    surface.end_tagged();
                },
                &mut |surface, mark| match mark {
                    Mark::Start(link) => {
                        let target = link
                            .and_then(|link| links.get(link as usize))
                            .and_then(|link| resolve.target(link));
                        if let Some(target) = target {
                            events.push(Event::Open(krilla::tagging::Tag::Link.into()));
                            events.push(Event::Annot(annots.len()));
                            link_open = Some(annots.len());
                            annots.push((
                                target,
                                Rect::from_xywh(0.0, 0.0, 1.0, 1.0).expect("a unit square"),
                                String::new(),
                            ));
                        }
                        let id = surface.start_tagged(ContentTag::Span(SpanTag::empty()));
                        events.push(Event::Leaf(id));
                    }
                    Mark::End(rect, range) => {
                        surface.end_tagged();
                        if let Some(at) = link_open.take() {
                            annots[at].1 = rect;
                            text.get(range)
                                .unwrap_or_default()
                                .clone_into(&mut annots[at].2);
                            events.push(Event::Close);
                        }
                    }
                },
            );
        }
    }
}

/// The tag tree, built as pages are finished.
struct Tags {
    tree: TagTree,
    stack: Vec<TagGroup>,
}

impl Tags {
    fn push(&mut self, node: Node) {
        match self.stack.last_mut() {
            Some(group) => group.push(node),
            None => self.tree.push(node),
        }
    }

    fn play(&mut self, events: Vec<Event>, annots: &[Identifier]) {
        for event in events {
            match event {
                Event::Open(tag) => self.stack.push(TagGroup::new(tag)),
                Event::Close => {
                    if let Some(group) = self.stack.pop() {
                        self.push(group.into());
                    }
                }
                Event::Leaf(id) => self.push(id.into()),
                Event::Annot(at) => {
                    if let Some(id) = annots.get(at) {
                        self.push((*id).into());
                    }
                }
            }
        }
    }

    fn finish(mut self) -> TagTree {
        while let Some(group) = self.stack.pop() {
            self.push(group.into());
        }
        self.tree
    }
}

/// The structure a part stands in, as a path of groups from the root:
/// moving to another closes the groups it leaves and opens those it
/// enters. Each group has a key, telling two of a kind apart.
#[derive(Default)]
pub struct Structure {
    open: Vec<u64>,
}

impl Structure {
    pub fn goto(&mut self, cx: &mut PageCx, path: &[(u64, TagKind)]) {
        let common = self
            .open
            .iter()
            .zip(path)
            .take_while(|(open, (key, _))| *open == key)
            .count();
        while self.open.len() > common {
            self.open.pop();
            cx.close();
        }
        for (key, tag) in &path[common..] {
            self.open.push(*key);
            cx.open(tag.clone());
        }
    }

    pub fn close_all(&mut self, cx: &mut PageCx) {
        self.goto(cx, &[]);
    }
}

/// A key for a group of `kind` (any number naming what it is) for `id`.
pub fn key(kind: u8, id: usize) -> u64 {
    (u64::from(kind) << 56) | (id as u64 & ((1 << 56) - 1))
}

/// The job's parts, laid out, written to its output: the answer.
pub fn write(
    job: &Job,
    fonts: &mut Fonts,
    frame: Frame,
    mut parts: Vec<Box<dyn Part + '_>>,
    anchors: HashMap<String, (usize, f32)>,
    headings: &[Heading],
) -> Result<Made> {
    let firsts: Vec<usize> = parts
        .iter()
        .scan(0, |at, part| {
            let first = *at;
            *at += part.pages();
            Some(first)
        })
        .collect();
    let laid_out: usize = parts.iter().map(|part| part.pages()).sum();
    let laid_out = u32::try_from(laid_out).context("too many pages")?;
    let written = geometry::written(laid_out, job.pages.as_deref());
    if written.is_empty() {
        bail!("none of the pages asked is there: {laid_out} laid out");
    }
    let mut index = vec![None; laid_out as usize];
    for (at, &page) in written.iter().enumerate() {
        index[page as usize] = Some(at);
    }
    let links = Links {
        written: index,
        anchors,
        x: frame.text.x,
    };
    let settings = SerializeSettings {
        render_svg_glyph_fn: krilla_svg::render_svg_glyph,
        ..SerializeSettings::default()
    };
    let mut document = Document::new_with(settings);
    let mut tags = Tags {
        tree: TagTree::new().with_lang(job.language.clone()),
        stack: Vec::new(),
    };
    let header = job.header.as_deref().map(|header| {
        let mut spec = Spec::new(Role::Text, BAND_SIZE);
        spec.color = GREY;
        spec.width = Some(frame.text.width);
        spec.wrap = Wrap::Anywhere;
        text::fitted(fonts, header, &spec, 1)
    });
    let page_settings =
        PageSettings::from_wh(frame.width, frame.height).context("the paper has no size")?;
    let mut last: Option<usize> = None;
    for &page in &written {
        let page = page as usize;
        let part = firsts.partition_point(|&first| first <= page) - 1;
        let mut sheet = document.start_page_with(page_settings.clone());
        let mut surface = sheet.surface();
        let mut cx = PageCx {
            surface: &mut surface,
            fonts,
            frame,
            links: &links,
            events: Vec::new(),
            annots: Vec::new(),
        };
        if let Some(previous) = last.filter(|&previous| previous != part) {
            parts[previous].finish(&mut cx);
        }
        last = Some(part);
        draw_bands(&mut cx, header.as_ref(), page, laid_out as usize);
        parts[part].draw(page - firsts[part], &mut cx)?;
        let PageCx { events, annots, .. } = cx;
        surface.finish();
        let ids: Vec<Identifier> = annots
            .into_iter()
            .map(|(target, rect, alt)| {
                let alt = (!alt.trim().is_empty()).then(|| alt.trim().to_owned());
                sheet.add_tagged_annotation(Annotation::new_link(
                    LinkAnnotation::new(rect, target),
                    alt,
                ))
            })
            .collect();
        sheet.finish();
        tags.play(events, &ids);
    }
    // What the last part left open is closed with the tree.
    document.set_tag_tree(tags.finish());
    document.set_outline(outline(headings, &links));
    let mut metadata = Metadata::new()
        .title(job.title.clone())
        .creator(format!("skwir {}", env!("SKWIR_BUILD")));
    if let Some(author) = &job.author {
        metadata = metadata.authors(vec![author.clone()]);
    }
    if let Some(language) = &job.language {
        metadata = metadata.language(language.clone());
    }
    document.set_metadata(metadata);
    let pdf = document
        .finish()
        .map_err(|error| anyhow::anyhow!("the PDF could not be made: {error:?}"))?;
    std::fs::write(&job.output, pdf)
        .with_context(|| format!("{} could not be written", job.output.display()))?;
    Ok(Made {
        pages: written.len() as u32,
        laid_out,
    })
}

/// The name above the text and "n / N" below it.
fn draw_bands(cx: &mut PageCx, header: Option<&(Layout<Ink>, String)>, page: usize, total: usize) {
    let frame = cx.frame;
    if let (Some(baseline), Some((layout, text))) = (frame.header, header) {
        let ascent = layout
            .get(0)
            .map_or(BAND_SIZE, |line| line.metrics().baseline);
        cx.artifact(ArtifactType::Header, |surface, fonts| {
            text::draw_line(
                surface,
                fonts,
                layout,
                text,
                0,
                (frame.text.x, baseline - ascent),
                &mut |_, _| {},
                &mut |_, _| {},
            );
        });
    }
    if let Some(baseline) = frame.footer {
        let number = format!("{} / {}", page + 1, total);
        let mut spec = Spec::new(Role::Text, BAND_SIZE);
        spec.color = GREY;
        let layout = text::plain(cx.fonts, &number, &spec);
        let ascent = layout
            .get(0)
            .map_or(BAND_SIZE, |line| line.metrics().baseline);
        let x = frame.text.x + (frame.text.width - layout.width()) / 2.0;
        cx.artifact(ArtifactType::Footer, |surface, fonts| {
            text::draw_line(
                surface,
                fonts,
                &layout,
                &number,
                0,
                (x, baseline - ascent),
                &mut |_, _| {},
                &mut |_, _| {},
            );
        });
    }
}

/// The outline of the headings on pages written, nested by level.
fn outline(headings: &[Heading], links: &Links) -> Outline {
    let kept: Vec<(u8, OutlineNode)> = headings
        .iter()
        .filter_map(|heading| {
            let page = (*links.written.get(heading.page)?)?;
            let title = heading.title.trim();
            (!title.is_empty()).then(|| {
                (
                    heading.level.clamp(1, 6),
                    OutlineNode::new(
                        title.to_owned(),
                        XyzDestination::new(page, Point::from_xy(links.x, heading.y)),
                    ),
                )
            })
        })
        .collect();
    let mut outline = Outline::new();
    let mut nodes = kept.into_iter().peekable();
    while nodes.peek().is_some() {
        if let Some(node) = nest(&mut nodes) {
            outline.push_child(node);
        }
    }
    outline
}

/// The next heading with those under it (deeper levels) as its children.
fn nest(
    nodes: &mut std::iter::Peekable<impl Iterator<Item = (u8, OutlineNode)>>,
) -> Option<OutlineNode> {
    let (level, mut node) = nodes.next()?;
    while nodes.peek().is_some_and(|(next, _)| *next > level) {
        if let Some(child) = nest(nodes) {
            node.push_child(child);
        }
    }
    Some(node)
}
