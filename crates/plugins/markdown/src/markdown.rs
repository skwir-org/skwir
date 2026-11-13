//! Event-based Markdown reader: pulldown-cmark's events become semantic
//! blocks directly, so headings, list depth, list markers, quotes, rules and
//! table rows keep their structure instead of being flattened to HTML and
//! scraped back. Inline emphasis, code and links survive as runs.
//!
//! Raw HTML blocks are the exception: Markdown allows arbitrary HTML, and
//! READMEs put logos and centered headers in it, so a run of HTML events is
//! handed to the shared reading-mode HTML reader rather than dropped. Its
//! images go through the same document-local loader, which names each
//! picture for the view to decode when it shows it.
use pulldown_cmark::{
    CodeBlockKind, CowStr, Event, HeadingLevel, LinkType, Options, Parser, Tag, TagEnd,
};
use skwir_core::{
    CancelToken,
    inspection::{Align, Block, BlockKind, Flow, Picture, Run},
};
use skwir_platform::{
    inspection::{check, limits, limits::Bounds},
    math, rich_preview,
};

/// Where a note's Obsidian links (`[[Note]]`, `[[Note#Heading|label]]`)
/// lead, found without a search: the note named, as `Note.md` or as
/// written, next to the note, else from the vault's top (the folder that
/// holds `.obsidian`). Obsidian looks through the whole vault; that waits
/// for an index of names that makes it free. Each becomes an ordinary
/// relative link (`wiki_href`), so it is followed as any other.
pub struct Wiki {
    dir: std::path::PathBuf,
    /// The vault's top, when the note is in one, and the way up to it from
    /// the note's folder (`../..`).
    top: Option<(std::path::PathBuf, String)>,
    here: skwir_core::fs::volume::Here,
    /// What each name was found as, from the note's folder.
    known: std::collections::HashMap<String, Option<String>>,
}

impl Wiki {
    /// For the note at `note`. Looks for the vault's top in the folders
    /// above it: on a worker.
    pub fn new(note: &std::path::Path) -> Option<Self> {
        let dir = note.parent().filter(|dir| dir.is_absolute())?.to_path_buf();
        // Looked up as `find` looks, only where `Here` holds it
        // (previews.md).
        let here = skwir_core::fs::volume::Here::now();
        let top = dir
            .ancestors()
            .enumerate()
            .find(|(_, folder)| {
                let marker = folder.join(".obsidian");
                here.holds(&marker, &dir) && marker.is_dir()
            })
            .map(|(up, folder)| (folder.to_path_buf(), "../".repeat(up)));
        Some(Self {
            dir,
            top,
            here,
            known: std::collections::HashMap::new(),
        })
    }

    /// Where the note named `name` is, from the note's folder, if it is
    /// next to it or under the vault's top. A name that climbs (`..`) or is
    /// absolute is looked up nowhere.
    fn find(&mut self, name: &str) -> Option<String> {
        if let Some(known) = self.known.get(name) {
            return known.clone();
        }
        let plain = !name.starts_with('/')
            && !name.contains('\\')
            && name.split('/').all(|part| part != ".." && part != ".");
        let names = [
            (!name.to_lowercase().ends_with(".md")).then(|| format!("{name}.md")),
            Some(name.to_owned()),
        ];
        let places = std::iter::once((self.dir.clone(), String::new())).chain(self.top.clone());
        let found = plain
            .then(|| {
                places.into_iter().find_map(|(folder, up)| {
                    names.iter().flatten().find_map(|name| {
                        let path = folder.join(name);
                        (self.here.holds(&path, &self.dir) && path.is_file())
                            .then(|| format!("{up}{name}"))
                    })
                })
            })
            .flatten();
        self.known.insert(name.to_owned(), found.clone());
        found
    }
}

/// The relative link `[[target]]` writes: the note `find` finds, else
/// `Note.md` beside the note (where Obsidian makes a note), its heading a
/// slug as the headings here are named (a block's `^id` as it is). Escaped
/// where a link's text would be cut: `%`, `#` and `?`.
fn wiki_href(target: &str, find: impl FnOnce(&str) -> Option<String>) -> String {
    let escape = |text: &str| {
        text.replace('%', "%25")
            .replace('#', "%23")
            .replace('?', "%3F")
    };
    let (name, heading) = target.split_once('#').unwrap_or((target, ""));
    let name = name.trim();
    let mut href = if name.is_empty() {
        String::new()
    } else {
        escape(&find(name).unwrap_or_else(|| {
            if name.to_lowercase().ends_with(".md") {
                name.to_owned()
            } else {
                format!("{name}.md")
            }
        }))
    };
    let heading = heading.trim();
    if !heading.is_empty() {
        href.push('#');
        href.push_str(&escape(&if heading.starts_with('^') {
            heading.to_owned()
        } else {
            slug(heading)
        }));
    }
    href
}

/// Bullet glyphs by nesting depth, cycling when the list goes deeper.
const BULLETS: [&str; 5] = ["•", "◦", "▪", "‣", "⁃"];

#[derive(Clone, Default)]
struct Inline {
    bold: usize,
    italic: usize,
    strikethrough: usize,
    code: usize,
    link: Option<String>,
}

impl Inline {
    fn run(&self, range: std::ops::Range<usize>) -> Run {
        Run {
            range,
            bold: self.bold > 0,
            italic: self.italic > 0,
            strikethrough: self.strikethrough > 0,
            code: self.code > 0,
            link: self.link.clone(),
            ..Run::default()
        }
    }
}

fn push(current: &mut Block, text: &str, inline: &Inline) {
    if text.is_empty() {
        return;
    }
    let start = current.text.len();
    current.text.push_str(text);
    current.runs.push(inline.run(start..current.text.len()));
}

/// Push `current` if it holds anything, and start the next block in the same
/// place in the document. The list marker belongs to the first block of its
/// item, so the block that takes it is also the one that spends it: a loose
/// item's later paragraphs must not repeat the bullet.
fn flush(blocks: &mut Vec<Block>, current: &mut Block, marker: &mut Option<String>) {
    if current.text.is_empty() && current.picture.is_none() && current.kind == BlockKind::Text {
        // A paragraph begun is the next block's to begin: one that opens
        // with a picture or a formula set apart flushes nothing first.
        *current = Block {
            flow: current.flow,
            ..Block::default()
        };
        return;
    }
    if current.marker.is_some() {
        *marker = None;
    }
    // An address written bare is a link, as GitHub reads Markdown.
    if !current.code {
        skwir_platform::links::link_block(current, false);
    }
    blocks.push(std::mem::take(current));
}

/// Start a block in the ambient context: inside the open quotes, the open
/// lists, carrying the list marker that no block has taken yet, and
/// beginning the paragraph no block has begun yet (`Flow::Paragraph`).
fn begin(current: &mut Block, quote_depth: u32, lists: &[Option<u64>], marker: Option<&str>) {
    *current = Block {
        quote: quote_depth > 0,
        list_depth: lists.len() as u32,
        marker: marker.map(str::to_owned),
        flow: current.flow,
        ..Block::default()
    };
}

/// The inline state an emphasis, link or strikethrough tag changes. A table
/// cell is text like any other, so both the block path and the table path
/// read these through here and keep their runs. An Obsidian link leads
/// where `wiki` finds it (`wiki_href`).
fn inline_event(inline: &mut Inline, event: &Event, wiki: Option<&mut Wiki>) -> bool {
    match event {
        Event::Start(Tag::Emphasis) => inline.italic += 1,
        Event::End(TagEnd::Emphasis) => inline.italic = inline.italic.saturating_sub(1),
        Event::Start(Tag::Strong) => inline.bold += 1,
        Event::End(TagEnd::Strong) => inline.bold = inline.bold.saturating_sub(1),
        Event::Start(Tag::Strikethrough) => inline.strikethrough += 1,
        Event::End(TagEnd::Strikethrough) => {
            inline.strikethrough = inline.strikethrough.saturating_sub(1);
        }
        Event::Start(Tag::Link {
            link_type: LinkType::WikiLink { .. },
            dest_url,
            ..
        }) => {
            inline.link = Some(wiki_href(dest_url, |name| wiki?.find(name)));
        }
        Event::Start(Tag::Link { dest_url, .. }) => inline.link = Some(dest_url.to_string()),
        Event::End(TagEnd::Link) => inline.link = None,
        _ => return false,
    }
    true
}

/// A cell's events with the math Jupyter took out put back
/// (`delimited::take`), as JupyterLab puts it back: a placeholder in prose
/// is the formula Markdown's math would have given; one in code (a span, a
/// fenced or indented block), HTML or a link's address or title is what was
/// written there, which MathJax does not typeset.
struct Placed<'t> {
    taken: &'t [math::delimited::Taken],
    /// Inside a code block: its text is code, never a formula.
    code: bool,
}

impl<'t> Placed<'t> {
    fn new(taken: &'t [math::delimited::Taken]) -> Self {
        Self { taken, code: false }
    }

    fn restored(&self, text: &str) -> CowStr<'static> {
        CowStr::from(math::delimited::restore(text, self.taken))
    }

    /// `text`, its placeholders what was written there, where it holds any.
    fn written<'a>(&self, text: CowStr<'a>) -> CowStr<'a> {
        if text.contains(math::delimited::MARK) {
            self.restored(&text)
        } else {
            text
        }
    }

    fn event<'a>(&mut self, event: Event<'a>) -> Vec<Event<'a>> {
        use math::delimited::{self, Taken};
        let taken = self.taken;
        match event {
            Event::Start(Tag::CodeBlock(kind)) => {
                self.code = true;
                let kind = match kind {
                    CodeBlockKind::Fenced(info) => CodeBlockKind::Fenced(self.written(info)),
                    indented @ CodeBlockKind::Indented => indented,
                };
                vec![Event::Start(Tag::CodeBlock(kind))]
            }
            Event::End(TagEnd::CodeBlock) => {
                self.code = false;
                vec![event]
            }
            Event::Text(text) if self.code => vec![Event::Text(self.written(text))],
            Event::Text(text) if text.contains(delimited::MARK) => delimited::split(&text)
                .into_iter()
                .map(|part| match part.map_err(|number| taken.get(number)) {
                    Ok(text) => Event::Text(CowStr::from(text.to_owned())),
                    Err(Some(Taken::Formula { tex, display, .. })) if *display => {
                        Event::DisplayMath(CowStr::from(tex.clone()))
                    }
                    Err(Some(Taken::Formula { tex, .. })) => {
                        Event::InlineMath(CowStr::from(tex.clone()))
                    }
                    Err(Some(Taken::Written(written))) => {
                        Event::Text(CowStr::from(written.clone()))
                    }
                    Err(None) => Event::Text(CowStr::from("")),
                })
                .collect(),
            Event::Code(text) => vec![Event::Code(self.written(text))],
            Event::Html(text) => vec![Event::Html(self.written(text))],
            Event::InlineHtml(text) => vec![Event::InlineHtml(self.written(text))],
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            }) => vec![Event::Start(Tag::Link {
                link_type,
                dest_url: self.written(dest_url),
                title: self.written(title),
                id,
            })],
            Event::Start(Tag::Image {
                link_type,
                dest_url,
                title,
                id,
            }) => vec![Event::Start(Tag::Image {
                link_type,
                dest_url: self.written(dest_url),
                title: self.written(title),
                id,
            })],
            other => vec![other],
        }
    }
}

/// A formula in a sentence: a drawing to come in its run's styles, or its
/// TeX as code.
fn inline_formula(
    current: &mut Block,
    tex: &str,
    inline: &mut Inline,
    formulas: &mut math::Pending,
    github: bool,
) {
    // GitHub's own form, `` $`…`$ ``, where Markdown's math is GitHub's:
    // pulldown-cmark reads it as a formula whose TeX is in backticks.
    let (tex, written) = match github.then(|| backticked(tex)).flatten() {
        Some(inner) => (inner, format!("$`{inner}`$")),
        None => (tex, written(tex.trim(), false)),
    };
    if let Some(drawing) = formulas.ask(tex, false, written.clone()) {
        current.push_drawing(drawing, &inline.run(0..0));
    } else {
        inline.code += 1;
        push(current, &written, inline);
        inline.code -= 1;
    }
}

/// What `tex` holds between the backticks around it, if it is in them as a
/// code span is (one or more on each side, as many).
fn backticked(tex: &str) -> Option<&str> {
    let ticks = tex.len() - tex.trim_start_matches('`').len();
    let inner = tex
        .get(ticks..tex.len().checked_sub(ticks)?)
        .filter(|_| ticks > 0 && tex.ends_with(&tex[..ticks]))?;
    (!inner.is_empty() && !inner.starts_with('`') && !inner.ends_with('`')).then_some(inner)
}

/// Heading link targets, named the way GitHub names them, since that is
/// where READMEs' `[see](#the-plan)` links were written against: lowercase,
/// a space becomes a hyphen, and everything but letters, digits, `-` and
/// `_` is dropped; a repeated target gets `-1`, `-2` and so on. An explicit
/// `{#id}` is kept as written and only made unique.
#[derive(Default)]
struct Anchors {
    /// Every target handed out, with the next suffix to try for it.
    used: std::collections::HashMap<String, usize>,
}
/// A heading's link target as GitHub names it (`Anchors`), before it is
/// made unique.
fn slug(text: &str) -> String {
    text.trim()
        .chars()
        .filter_map(|c| match c {
            ' ' => Some('-'),
            c if c.is_alphanumeric() || c == '-' || c == '_' => Some(c),
            _ => None,
        })
        .flat_map(char::to_lowercase)
        .collect()
}

impl Anchors {
    fn slug(&mut self, text: &str) -> String {
        self.unique(slug(text))
    }
    fn unique(&mut self, base: String) -> String {
        let Some(next) = self.used.get(&base).copied() else {
            self.used.insert(base.clone(), 1);
            return base;
        };
        // Each base remembers where its count stopped, so a document that
        // repeats one heading a thousand times costs a thousand lookups.
        let mut seen = next;
        let candidate = loop {
            let candidate = format!("{base}-{seen}");
            seen += 1;
            if !self.used.contains_key(&candidate) {
                break candidate;
            }
        };
        self.used.insert(base, seen);
        self.used.insert(candidate.clone(), 1);
        candidate
    }
}

fn level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// Read a run of raw HTML with the shared reading-mode parser and append what
/// it found. Markdown allows arbitrary HTML blocks, so dropping them loses a
/// README's logo, badges and centered headers.
/// HTML in the document, read by the shared reading-mode reader, its
/// formulas joining the document's batch (`formulas`), its text's TeX found
/// by `tex` (a notebook's output, by JupyterLab's MathJax), else by the
/// page's own scripts alone: Markdown's HTML is no Markdown, whose `$`
/// GitHub leaves there.
#[allow(clippy::too_many_arguments)] // The reader's state, borrowed apart.
fn emit_html(
    blocks: &mut Vec<Block>,
    html: &mut String,
    bounds: Bounds,
    cancel: &CancelToken,
    image: &mut impl FnMut(&str) -> Option<Picture>,
    quote_depth: u32,
    formulas: &mut math::Pending,
    tex: Option<math::delimited::Rules>,
) -> anyhow::Result<()> {
    let source = std::mem::take(html);
    if source.trim().is_empty() {
        return Ok(());
    }
    let page = rich_preview::html_with(
        &source,
        rich_preview::Markup::Html,
        bounds,
        cancel,
        rich_preview::named(image),
        rich_preview::Math {
            batch: Some(formulas),
            tex,
        },
    )?;
    for mut block in page.blocks {
        block.quote |= quote_depth > 0;
        blocks.push(block);
    }
    Ok(())
}

/// The document's blocks, up to `bounds`' `blocks`: a longer one (a
/// changelog is a list item per line) is cut there and says so
/// (`Page::truncated`), its source one toggle away.
pub fn parse(
    source: &str,
    bounds: Bounds,
    cancel: &CancelToken,
    load: impl FnMut(&str) -> Option<Picture>,
    wiki: Option<&mut Wiki>,
) -> anyhow::Result<rich_preview::Page> {
    let mut document = Document::new(bounds);
    document.markdown(source, cancel, load, wiki)?;
    Ok(document.finish(cancel))
}

/// A page read in parts, each a document of its own: a notebook's cells,
/// which Jupyter draws apart, so a list or a fence one leaves open never
/// takes the next, and what a cell printed is never read as Markdown. The
/// parts share the page's heading targets, its diagrams and its bound on
/// the blocks.
pub struct Document {
    blocks: Vec<Block>,
    anchors: Anchors,
    diagrams: usize,
    formulas: math::Pending,
    bounds: Bounds,
    truncated: bool,
}

/// A formula as it is written in Markdown: what it is copied as, and shown
/// as where it is not drawn.
fn written(tex: &str, display: bool) -> String {
    if display {
        format!("$${tex}$$")
    } else {
        format!("${tex}$")
    }
}

impl Document {
    pub fn new(bounds: Bounds) -> Self {
        Self {
            blocks: Vec::new(),
            anchors: Anchors::default(),
            diagrams: 0,
            formulas: math::Pending::default(),
            bounds,
            truncated: false,
        }
    }

    pub fn bounds(&self) -> Bounds {
        self.bounds
    }

    /// Whether its formulas can be typeset: the helper is installed.
    pub fn typesets(&mut self) -> bool {
        self.formulas.typesets()
    }

    /// Whether the page holds all it may: what comes after is left out.
    pub fn full(&self) -> bool {
        self.truncated || self.blocks.len() >= self.bounds.blocks()
    }

    /// A part written in Markdown, its pictures named by `load`.
    pub fn markdown(
        &mut self,
        source: &str,
        cancel: &CancelToken,
        load: impl FnMut(&str) -> Option<Picture>,
        wiki: Option<&mut Wiki>,
    ) -> anyhow::Result<()> {
        read(self, source, cancel, load, wiki, None)
    }

    /// A notebook's Markdown cell, its pictures named by `load`: its math
    /// is what Jupyter takes out of it before reading its Markdown
    /// (`delimited::JUPYTER`), not Markdown's `$…$`.
    pub fn cell(
        &mut self,
        source: &str,
        cancel: &CancelToken,
        load: impl FnMut(&str) -> Option<Picture>,
    ) -> anyhow::Result<()> {
        let (text, taken) = math::delimited::take(source, math::delimited::JUPYTER);
        read(self, &text, cancel, load, None, Some(&taken))
    }

    /// A notebook's HTML result, read by the shared reading-mode reader, its
    /// pictures named by `load`, its TeX found as JupyterLab's MathJax finds
    /// it (`$` included, `\$` a dollar, nothing marked `mathjax_ignore`),
    /// its formulas typeset with the notebook's; whether it showed anything
    /// (a chart a script draws shows nothing inert).
    pub fn html(
        &mut self,
        source: &str,
        cancel: &CancelToken,
        mut load: impl FnMut(&str) -> Option<Picture>,
    ) -> anyhow::Result<bool> {
        let before = self.blocks.len();
        emit_html(
            &mut self.blocks,
            &mut source.to_owned(),
            self.bounds,
            cancel,
            &mut load,
            0,
            &mut self.formulas,
            Some(math::delimited::mathjax(true)),
        )?;
        Ok(self.blocks.len() > before)
    }

    /// A block made elsewhere: a notebook's code, what a cell printed.
    pub fn push(&mut self, block: Block) {
        if self.full() {
            self.truncated = true;
        } else {
            self.blocks.push(block);
        }
    }

    /// The page read, its formulas typeset.
    pub fn finish(self, cancel: &CancelToken) -> rich_preview::Page {
        let truncated = self.truncated || self.blocks.len() > self.bounds.blocks();
        let mut blocks = cut(self.blocks, self.bounds);
        self.formulas.typeset(&mut blocks, cancel);
        rich_preview::Page { blocks, truncated }
    }
}

/// Read `source` into `document`, up to its bound.
fn read(
    document: &mut Document,
    source: &str,
    cancel: &CancelToken,
    mut load: impl FnMut(&str) -> Option<Picture>,
    mut wiki: Option<&mut Wiki>,
    taken: Option<&[math::delimited::Taken]>,
) -> anyhow::Result<()> {
    let Document {
        blocks,
        anchors,
        diagrams,
        formulas,
        bounds,
        truncated,
    } = document;
    let bounds = *bounds;
    // Metadata blocks are enabled so that a note's front matter is recognized
    // and skipped, rather than read as a rule and a large heading.
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_HEADING_ATTRIBUTES
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
        | Options::ENABLE_PLUSES_DELIMITED_METADATA_BLOCKS
        | Options::ENABLE_WIKILINKS;
    // Markdown's math, unless Jupyter's was taken out before: its
    // placeholders are formulas again here (`placed`). Markdown's math is
    // GitHub's: a fenced `math` block is a formula set apart, and
    // `` $`…`$ `` one in a sentence, which JupyterLab has neither of.
    let github = taken.is_none();
    let events: Box<dyn Iterator<Item = Event<'_>>> = match taken {
        None => Box::new(Parser::new_ext(source, options | Options::ENABLE_MATH)),
        Some(taken) => {
            let mut placed = Placed::new(taken);
            Box::new(Parser::new_ext(source, options).flat_map(move |event| placed.event(event)))
        }
    };
    let mut current = Block::default();
    let mut inline = Inline::default();
    let mut quote_depth = 0u32;
    let mut lists: Vec<Option<u64>> = Vec::new();
    let mut marker: Option<String> = None;
    let mut alt = String::new();
    let mut pending: Option<Picture> = None;
    // Images open inside the one being read: its description may hold one.
    let mut images = 0u32;
    let mut in_table = false;
    let mut in_metadata = false;
    let mut table_header = false;
    let mut cells: Vec<std::ops::Range<usize>> = Vec::new();
    let mut cell_start = 0usize;
    let mut html = String::new();

    for event in events {
        check(cancel)?;
        if blocks.len() >= bounds.blocks() {
            *truncated = true;
            return Ok(());
        }
        if in_metadata {
            if matches!(event, Event::End(TagEnd::MetadataBlock(_))) {
                in_metadata = false;
            }
            continue;
        }
        // A table row is collected into `current` like any other text, so a
        // cell keeps its emphasis, code and links; the cells are the ranges
        // of that text the view draws in columns.
        if in_table {
            if inline_event(&mut inline, &event, wiki.as_deref_mut()) {
                continue;
            }
            let mut finish = |current: &mut Block, cells: &mut Vec<_>, header| {
                current.kind = BlockKind::TableRow {
                    cells: std::mem::take(cells),
                    header,
                };
                skwir_platform::links::link_block(current, false);
                blocks.push(std::mem::take(current));
            };
            match event {
                Event::Start(Tag::TableHead | Tag::TableRow) => {
                    table_header = matches!(event, Event::Start(Tag::TableHead));
                    cells.clear();
                    begin(&mut current, quote_depth, &lists, None);
                }
                // The head row may close without its own `TableRow` event.
                Event::End(TagEnd::TableHead) => {
                    if !cells.is_empty() {
                        finish(&mut current, &mut cells, true);
                    }
                    table_header = false;
                }
                Event::End(TagEnd::TableRow) => finish(&mut current, &mut cells, table_header),
                Event::Start(Tag::TableCell) => {
                    if !cells.is_empty() {
                        current.text.push('\t');
                    }
                    cell_start = current.text.len();
                }
                Event::End(TagEnd::TableCell) => cells.push(cell_start..current.text.len()),
                Event::End(TagEnd::Table) => {
                    in_table = false;
                    begin(&mut current, quote_depth, &lists, marker.as_deref());
                }
                Event::Text(text) => push(&mut current, text.as_ref(), &inline),
                Event::Code(text) => {
                    inline.code += 1;
                    push(&mut current, text.as_ref(), &inline);
                    inline.code -= 1;
                }
                Event::SoftBreak => push(&mut current, " ", &inline),
                Event::HardBreak => push(&mut current, "\n", &inline),
                // A formula in a cell stands in its line, whichever way it
                // was written.
                Event::InlineMath(tex) | Event::DisplayMath(tex) => {
                    inline_formula(&mut current, &tex, &mut inline, formulas, github);
                }
                _ => {}
            }
            continue;
        }
        // An image's description is its `alt`, read as CommonMark and
        // pulldown-cmark's own HTML writer read it: its plain content, code
        // and inline HTML as written, an image inside it by its own
        // description, a line break as a space; emphasis and links leave
        // their words.
        if images > 0 {
            match &event {
                Event::Start(Tag::Image { .. }) => images += 1,
                Event::End(TagEnd::Image) => images -= 1,
                Event::Text(text)
                | Event::Code(text)
                | Event::InlineHtml(text)
                | Event::InlineMath(text)
                | Event::DisplayMath(text) => {
                    alt.push_str(text);
                }
                Event::SoftBreak | Event::HardBreak => alt.push(' '),
                _ => {}
            }
            if images > 0 {
                continue;
            }
        }
        if inline_event(&mut inline, &event, wiki.as_deref_mut()) {
            continue;
        }
        // A run of raw HTML ends here: read it where it stood.
        if !html.is_empty() && !matches!(event, Event::Html(_)) {
            flush(blocks, &mut current, &mut marker);
            emit_html(
                blocks,
                &mut html,
                bounds,
                cancel,
                &mut load,
                quote_depth,
                formulas,
                None,
            )?;
            begin(&mut current, quote_depth, &lists, marker.as_deref());
        }
        match event {
            Event::Start(Tag::Paragraph) => {
                flush(blocks, &mut current, &mut marker);
                begin(&mut current, quote_depth, &lists, marker.as_deref());
                current.flow = Flow::Paragraph;
            }
            Event::End(TagEnd::Paragraph) => {
                flush(blocks, &mut current, &mut marker);
                current.flow = Flow::Next;
            }
            Event::End(TagEnd::Heading(_)) => {
                if current.anchors.is_empty() {
                    current.anchors.push(anchors.slug(&current.text));
                }
                flush(blocks, &mut current, &mut marker);
            }
            Event::Start(Tag::Heading { level: tag, id, .. }) => {
                flush(blocks, &mut current, &mut marker);
                begin(&mut current, quote_depth, &lists, marker.as_deref());
                current.heading = level(tag);
                current.anchors = id
                    .map(|id| anchors.unique(id.to_string()))
                    .into_iter()
                    .collect();
            }
            Event::Start(Tag::BlockQuote(_)) => {
                flush(blocks, &mut current, &mut marker);
                quote_depth += 1;
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                flush(blocks, &mut current, &mut marker);
                quote_depth = quote_depth.saturating_sub(1);
            }
            Event::Start(Tag::List(start)) => {
                flush(blocks, &mut current, &mut marker);
                lists.push(start);
            }
            Event::End(TagEnd::List(_)) => {
                flush(blocks, &mut current, &mut marker);
                lists.pop();
            }
            Event::Start(Tag::Item) => {
                flush(blocks, &mut current, &mut marker);
                marker = Some(if let Some(Some(next)) = lists.last_mut() {
                    let number = *next;
                    *next += 1;
                    format!("{number}.")
                } else {
                    let depth = lists.len().saturating_sub(1);
                    BULLETS[depth % BULLETS.len()].to_string()
                });
                begin(&mut current, quote_depth, &lists, marker.as_deref());
            }
            Event::End(TagEnd::Item) => {
                flush(blocks, &mut current, &mut marker);
                marker = None;
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                flush(blocks, &mut current, &mut marker);
                begin(&mut current, quote_depth, &lists, marker.as_deref());
                current.code = true;
                current.language = match kind {
                    CodeBlockKind::Fenced(info) => info
                        .split_whitespace()
                        .next()
                        .filter(|token| !token.is_empty())
                        .map(str::to_owned),
                    CodeBlockKind::Indented => None,
                };
            }
            Event::End(TagEnd::CodeBlock) => {
                if current.text.ends_with('\n') {
                    current.text.pop();
                }
                // A diagram is part of the page, like a drawing in HTML:
                // drawn with it, a few per page; past them, or when it
                // cannot be drawn, it stays the code it is written in.
                // Every one tried counts, drawn or not: a page of diagrams
                // that fail ran the helper for each, its deadline each time.
                if current.language.as_deref() == Some("mermaid")
                    && *diagrams < limits::EMBEDDED_IMAGES
                    && let Some(image) = {
                        *diagrams += 1;
                        crate::diagram::mermaid(&current.text, cancel)
                    }
                {
                    current.text.clear();
                    current.runs.clear();
                    current.code = false;
                    current.language = None;
                    current.picture = Some(Picture::Ready(image));
                }
                if github
                    && current.language.as_deref() == Some("math")
                    && let Some(drawing) = {
                        let tex = current.text.trim();
                        formulas.ask(tex, true, written(tex, true))
                    }
                {
                    // The drawing is the block's text: the code's run goes
                    // with the code.
                    current.text.clear();
                    current.runs.clear();
                    current.code = false;
                    current.language = None;
                    current.align = Align::Center;
                    current.push_drawing(drawing, &Run::default());
                }
                flush(blocks, &mut current, &mut marker);
            }
            Event::Start(Tag::Table(_)) => {
                flush(blocks, &mut current, &mut marker);
                in_table = true;
            }
            Event::Start(Tag::MetadataBlock(_)) => {
                flush(blocks, &mut current, &mut marker);
                in_metadata = true;
            }
            Event::Start(Tag::Image { dest_url, .. }) => {
                flush(blocks, &mut current, &mut marker);
                images = 1;
                alt.clear();
                pending = load(dest_url.as_ref());
            }
            // An image is a block of its own, so the paragraph around it is
            // cut in two; both halves stay in the quote or list item they
            // were written in.
            Event::End(TagEnd::Image) => {
                let picture = pending.take();
                let text = std::mem::take(&mut alt);
                begin(&mut current, quote_depth, &lists, marker.as_deref());
                if picture.is_some() || !text.trim().is_empty() {
                    current.set_picture(picture, &text);
                    flush(blocks, &mut current, &mut marker);
                    begin(&mut current, quote_depth, &lists, marker.as_deref());
                }
            }
            // The half of a paragraph after what cut it (an image, a
            // formula set apart) starts at its words, not at the space
            // before them; code keeps its indent.
            Event::Text(text) if current.text.is_empty() && !current.code => {
                push(&mut current, text.trim_start(), &inline);
            }
            Event::Text(text) => push(&mut current, text.as_ref(), &inline),
            Event::Code(text) => {
                inline.code += 1;
                push(&mut current, text.as_ref(), &inline);
                inline.code -= 1;
            }
            Event::SoftBreak if current.text.is_empty() => {}
            Event::SoftBreak => push(&mut current, " ", &inline),
            Event::HardBreak => push(&mut current, "\n", &inline),
            Event::Rule => {
                flush(blocks, &mut current, &mut marker);
                blocks.push(Block {
                    kind: BlockKind::Rule,
                    quote: quote_depth > 0,
                    list_depth: lists.len() as u32,
                    ..Block::default()
                });
            }
            Event::InlineMath(tex) => {
                inline_formula(&mut current, &tex, &mut inline, formulas, github);
            }
            // A formula set apart is a block of its own, centered, so the
            // paragraph around it is cut in two, as an image cuts it; both
            // halves stay in the quote or list item they were written in.
            Event::DisplayMath(tex) => {
                flush(blocks, &mut current, &mut marker);
                begin(&mut current, quote_depth, &lists, marker.as_deref());
                if let Some(drawing) = formulas.ask(&tex, true, written(tex.trim(), true)) {
                    current.align = Align::Center;
                    current.push_drawing(drawing, &Run::default());
                } else {
                    current.code = true;
                    current.language = Some("latex".into());
                    tex.trim().clone_into(&mut current.text);
                }
                flush(blocks, &mut current, &mut marker);
                begin(&mut current, quote_depth, &lists, marker.as_deref());
            }
            Event::TaskListMarker(checked) => {
                push(&mut current, if checked { "☑ " } else { "☐ " }, &inline);
            }
            Event::InlineHtml(text) => {
                if matches!(text.trim(), "<br>" | "<br/>" | "<br />") {
                    push(&mut current, "\n", &inline);
                }
            }
            Event::Html(text) => html.push_str(text.as_ref()),
            _ => {}
        }
    }
    flush(blocks, &mut current, &mut marker);
    emit_html(
        blocks,
        &mut html,
        bounds,
        cancel,
        &mut load,
        quote_depth,
        formulas,
        None,
    )?;
    Ok(())
}

/// A run of raw HTML may add several blocks at once: keep to the bound.
fn cut(mut blocks: Vec<Block>, bounds: Bounds) -> Vec<Block> {
    blocks.truncate(bounds.blocks());
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> Vec<Block> {
        super::parse(source, Bounds::QUICK, &CancelToken::new(), |_| None, None)
            .unwrap()
            .blocks
    }

    #[test]
    fn headings_and_inline_runs_keep_their_structure() {
        let blocks = parse(
            "# Title\n\nA **bold** *italic* ~~gone~~ `code` [link](https://example.invalid).\n",
        );
        assert_eq!(blocks[0].heading, 1);
        assert_eq!(blocks[0].text, "Title");
        let text = &blocks[1].text;
        let runs = &blocks[1].runs;
        let run = |pick: &dyn Fn(&Run) -> bool| {
            let r = runs.iter().find(|r| pick(r)).expect("run");
            &text[r.range.clone()]
        };
        assert_eq!(run(&|r| r.bold), "bold");
        assert_eq!(run(&|r| r.italic), "italic");
        assert_eq!(run(&|r| r.strikethrough), "gone");
        assert_eq!(run(&|r| r.code), "code");
        assert_eq!(
            runs.iter()
                .find(|r| r.link.as_deref() == Some("https://example.invalid"))
                .map(|r| &text[r.range.clone()]),
            Some("link")
        );
    }

    /// Point `SKWIR_MERMAID_WORKER` at `worker`.
    fn use_worker(worker: &std::path::Path) {
        use_helper(&skwir_platform::diagram::WORKER, worker);
    }

    /// Point `helper`'s variable at `worker`. Tests run in processes of
    /// their own under nextest, so the variable reaches no other test.
    fn use_helper(helper: &skwir_platform::process::Worker, worker: &std::path::Path) {
        // SAFETY: no other thread of this test process reads the environment.
        #[allow(unsafe_code)]
        unsafe {
            std::env::set_var(helper.variable, worker);
        }
    }

    /// `helper` built into the target directory this test runs from (the
    /// test binary sits in its `deps`), its variable pointed at it. Without
    /// it the test says so and passes, unless `SKWIR_REQUIRE_WORKERS` is
    /// set.
    fn with_built(helper: &skwir_platform::process::Worker) -> bool {
        let built = std::env::current_exe()
            .ok()
            .and_then(|exe| Some(exe.parent()?.parent()?.join(helper.executable)))
            .filter(|path| path.is_file());
        let Some(worker) = built else {
            assert!(
                std::env::var_os("SKWIR_REQUIRE_WORKERS").is_none(),
                "{} is not built",
                helper.executable
            );
            eprintln!("skipped: {} is not built", helper.executable);
            return false;
        };
        use_helper(helper, &worker);
        true
    }

    fn with_mermaid_worker() -> bool {
        with_built(&skwir_platform::diagram::WORKER)
    }

    /// A paragraph with a formula drawn and one that cannot be, one set
    /// apart, and a row of a table holding both kinds.
    /// GitHub's `` $`…`$ `` is the formula between its backticks, copied as
    /// it is written.
    #[test]
    fn github_s_inline_form_is_the_formula_between_its_backticks() {
        assert_eq!(backticked("`\\sqrt{3}`"), Some("\\sqrt{3}"));
        assert_eq!(backticked("``a`b``"), Some("a`b"));
        assert_eq!(backticked("x^2"), None);
        assert_eq!(backticked("`x"), None);
        assert_eq!(backticked("``"), None);
        if !with_built(&skwir_platform::math::WORKER) {
            return;
        }
        let blocks = parse("A $`\\sqrt{3}`$ and $x$.\n");
        let sentence = &blocks[0];
        assert_eq!(
            sentence.said(0..sentence.text.len()),
            "A $`\\sqrt{3}`$ and $x$."
        );
        let drawn = drawings(sentence);
        assert_eq!(drawn.len(), 2);
        assert!(
            drawn[0].width > 1.,
            "a root, not quotes: {}",
            drawn[0].width
        );
    }

    /// A paragraph's first block begins it, a picture first included; a
    /// tight list's items and the half after a picture do not.
    #[test]
    fn a_block_says_whether_it_begins_a_paragraph() {
        let source = "> One.\n>\n> Two.\n\n- tight\n- items\n\nBetween.\n\n\
                      * loose\n\n* items\n\n![a picture](missing.png) and after it.\n";
        let blocks = super::parse(
            source,
            Bounds::QUICK,
            &CancelToken::new(),
            |_| {
                Some(Picture::Ready(skwir_core::thumbs::Image {
                    width: 1,
                    height: 1,
                    rgba: vec![0; 4].into(),
                }))
            },
            None,
        )
        .unwrap()
        .blocks;
        let begun: Vec<(&str, bool, bool)> = blocks
            .iter()
            .map(|block| {
                (
                    block.text.as_str(),
                    block.picture.is_some(),
                    block.flow == Flow::Paragraph,
                )
            })
            .collect();
        assert_eq!(
            begun,
            [
                ("One.", false, true),
                ("Two.", false, true),
                ("tight", false, false),
                ("items", false, false),
                ("Between.", false, true),
                ("loose", false, true),
                ("items", false, true),
                ("", true, true),
                ("and after it.", false, false),
            ]
        );
    }

    /// TeX a notebook's cell writes in code (a fenced block, an indented
    /// one, a span) or a link's address is what is written there, as
    /// JupyterLab shows it: never a formula, and the block stays whole.
    #[test]
    fn a_notebook_cell_s_code_keeps_the_tex_it_writes() {
        let fenced =
            "```latex\n\\begin{align} x &= 1 \\end{align}\nthen \\\\(y\\\\) and $$z$$\n```";
        let source = format!(
            "Before.\n\n{fenced}\n\n    \\\\[w\\\\] indented\n\nA span `\\\\(v\\\\)` and [a link](#\\\\(u\\\\)).\n"
        );
        let mut notebook = Document::new(Bounds::QUICK);
        notebook
            .cell(&source, &CancelToken::new(), |_| None)
            .unwrap();
        let blocks = &notebook.blocks;
        let shown: Vec<(&str, bool)> = blocks
            .iter()
            .map(|block| (block.text.as_str(), block.code))
            .collect();
        assert_eq!(
            shown,
            [
                ("Before.", false),
                (
                    "\\begin{align} x &= 1 \\end{align}\nthen \\\\(y\\\\) and $$z$$",
                    true
                ),
                ("\\\\[w\\\\] indented", true),
                ("A span \\\\(v\\\\) and a link.", false),
            ]
        );
        assert!(
            blocks
                .iter()
                .flat_map(|block| &block.runs)
                .all(|run| run.drawing.is_none()),
            "no formula in code"
        );
        let link = blocks[3].runs.iter().find_map(|run| run.link.as_deref());
        assert_eq!(link, Some("#\\\\(u\\\\)"));
    }

    /// GitHub's fenced `math` is a formula set apart in Markdown, as `$$`
    /// is; in a notebook's cell it is code, as JupyterLab shows it.
    #[test]
    fn a_fenced_math_block_is_a_formula_set_apart_but_in_a_notebook() {
        let source = "Before.\n\n```math\n\\frac{a}{b}\n```\n\nAfter.\n";
        let mut notebook = Document::new(Bounds::QUICK);
        notebook
            .cell(source, &CancelToken::new(), |_| None)
            .unwrap();
        let code = &notebook.blocks[1];
        assert!(code.code);
        assert_eq!(code.language.as_deref(), Some("math"));
        assert_eq!(code.text, "\\frac{a}{b}");
        if !with_built(&skwir_platform::math::WORKER) {
            return;
        }
        let blocks = parse(source);
        assert_eq!(blocks.len(), 3, "{blocks:?}");
        let apart = &blocks[1];
        assert!(!apart.code);
        assert_eq!(apart.align, Align::Center);
        assert_eq!(apart.text, Block::DRAWN.to_string());
        assert_eq!(
            apart.runs.len(),
            1,
            "the drawing's run alone: {:?}",
            apart.runs
        );
        assert_eq!(apart.said(0..apart.text.len()), "$$\\frac{a}{b}$$");
        assert!(
            drawings(apart)[0].descent > 0.2,
            "the fraction's denominator"
        );
    }

    const FORMULAS: &str = "Einstein: $E = mc^2$, and *so* $\\foo{x}$.\n\n$$\n\\sum_{i=1}^n x_i\n$$\n\n| a | b |\n|---|---|\n| $\\foo{x}$ | $x^2$ |\n";

    fn drawings(block: &Block) -> Vec<&skwir_core::inspection::Drawing> {
        block
            .runs
            .iter()
            .filter_map(|run| run.drawing.as_deref())
            .collect()
    }

    #[test]
    fn formulas_stand_in_their_lines_and_a_broken_one_is_its_tex() {
        if !with_built(&skwir_platform::math::WORKER) {
            return;
        }
        let blocks = parse(FORMULAS);
        let sentence = &blocks[0];
        assert_eq!(
            sentence.text,
            format!("Einstein: {}, and so $\\foo{{x}}$.", Block::DRAWN)
        );
        assert_eq!(
            sentence.said(0..sentence.text.len()),
            "Einstein: $E = mc^2$, and so $\\foo{x}$.",
            "a drawing reads as its TeX"
        );
        let drawn = drawings(sentence);
        assert_eq!(drawn.len(), 1);
        assert!(drawn[0].svg.starts_with(b"<svg"));
        assert!(drawn[0].width > 2. && drawn[0].ascent > 0.5 && drawn[0].margin > 0.);
        assert!(
            sentence
                .runs
                .iter()
                .any(|run| run.code && &sentence.text[run.range.clone()] == "$\\foo{x}$"),
            "what cannot be typeset is its TeX, as code"
        );
        let apart = &blocks[1];
        assert_eq!(apart.align, Align::Center);
        assert_eq!(apart.text, Block::DRAWN.to_string());
        assert!(drawings(apart)[0].descent > 0.5, "the sum's limit below");
        let row = blocks
            .iter()
            .find(|block| matches!(block.kind, BlockKind::TableRow { header: false, .. }))
            .unwrap();
        let BlockKind::TableRow { cells, .. } = &row.kind else {
            unreachable!()
        };
        let cells: Vec<_> = cells.iter().map(|cell| &row.text[cell.clone()]).collect();
        assert_eq!(
            cells,
            ["$\\foo{x}$".to_owned(), Block::DRAWN.to_string()],
            "the cells after a formula written out move along"
        );
    }

    #[test]
    fn without_its_helper_a_formula_is_its_tex() {
        use_helper(
            &skwir_platform::math::WORKER,
            std::path::Path::new("/nonexistent/skwir-math-worker"),
        );
        let blocks = parse(FORMULAS);
        assert!(blocks.iter().all(|block| drawings(block).is_empty()));
        assert_eq!(blocks[0].text, "Einstein: $E = mc^2$, and so $\\foo{x}$.");
        assert!(blocks[0].runs.iter().any(|run| run.code));
        assert!(blocks[1].code);
        assert_eq!(blocks[1].language.as_deref(), Some("latex"));
        assert_eq!(blocks[1].text, "\\sum_{i=1}^n x_i");
    }

    const DIAGRAMS: &str = "```mermaid\nflowchart LR\n  A[Read] --> B[Show]\n```\n\n```mermaid\nnot a diagram at all\n```\n";

    #[test]
    fn a_mermaid_block_is_drawn_and_a_broken_one_stays_code() {
        if !with_mermaid_worker() {
            return;
        }
        let blocks = parse(DIAGRAMS);
        let Some(Picture::Ready(image)) = &blocks[0].picture else {
            panic!("the diagram is a picture: {:?}", blocks[0].text);
        };
        assert!(!blocks[0].code);
        assert!(image.width > image.height, "laid out left to right");
        // On white, and something drawn on it.
        assert_eq!(&image.rgba[..4], &[255, 255, 255, 255]);
        assert!(image.rgba.chunks(4).any(|pixel| pixel[0] < 128));
        assert!(blocks[1].picture.is_none());
        assert!(blocks[1].code);
        assert_eq!(blocks[1].language.as_deref(), Some("mermaid"));
        assert_eq!(blocks[1].text, "not a diagram at all");
    }

    #[test]
    fn without_its_helper_a_mermaid_block_stays_code() {
        use_worker(std::path::Path::new("/nonexistent/skwir-mermaid-worker"));
        let blocks = parse(DIAGRAMS);
        assert!(
            blocks
                .iter()
                .all(|block| block.picture.is_none() && block.code)
        );
        assert_eq!(blocks[0].text, "flowchart LR\n  A[Read] --> B[Show]");
    }

    #[test]
    fn list_items_carry_depth_and_a_marker() {
        let blocks = parse("- one\n  - nested\n\n1. first\n2. second\n");
        assert_eq!(blocks[0].marker.as_deref(), Some("•"));
        assert_eq!(blocks[0].list_depth, 1);
        assert_eq!(blocks[1].marker.as_deref(), Some("◦"));
        assert_eq!(blocks[1].list_depth, 2);
        assert_eq!(blocks[2].marker.as_deref(), Some("1."));
        assert_eq!(blocks[3].marker.as_deref(), Some("2."));
    }

    #[test]
    fn a_marker_belongs_to_the_first_block_of_its_item() {
        let blocks =
            parse("- first para\n\n  second para\n\n  ```rs\n  fn a() {}\n  ```\n\n- next\n");
        let markers: Vec<_> = blocks.iter().map(|b| b.marker.as_deref()).collect();
        assert_eq!(markers, [Some("•"), None, None, Some("•")]);
        assert!(
            blocks.iter().all(|b| b.list_depth == 1),
            "every block of the item stays in the list"
        );
    }

    #[test]
    fn an_image_splits_a_paragraph_without_leaving_its_context() {
        let blocks = parse("> Before ![alt](x.png) after.\n");
        assert_eq!(
            blocks.iter().map(|b| b.text.as_str()).collect::<Vec<_>>(),
            ["Before ", "alt", "after."],
            "the half after it starts at its words"
        );
        assert!(
            blocks.iter().all(|b| b.quote),
            "the halves stay inside the quote"
        );
        let blocks = parse("- Before ![alt](x.png) after.\n");
        assert!(blocks.iter().all(|b| b.list_depth == 1));
        assert_eq!(
            blocks.iter().filter(|b| b.marker.is_some()).count(),
            1,
            "one bullet for the item"
        );
    }

    /// A description stands in for its picture: kept aside while there is
    /// one to show, the text where there is none, in Markdown and in HTML.
    #[test]
    fn a_picture_shown_is_not_described_beside_itself() {
        let source = "![A grid of *pictures*](shown.png)\n\n\
            <img src=\"logo.png\" alt=\"The logo\">\n\n\
            ![Not there](missing.png)\n";
        let blocks = super::parse(
            source,
            Bounds::QUICK,
            &CancelToken::new(),
            |href| {
                (href != "missing.png").then(|| Picture::Later {
                    key: href.to_owned(),
                    width: 4,
                    height: 3,
                })
            },
            None,
        )
        .unwrap()
        .blocks;
        let read: Vec<_> = blocks
            .iter()
            .map(|b| (b.text.as_str(), b.alt.as_str(), b.picture.is_some()))
            .collect();
        assert_eq!(
            read,
            [
                ("", "A grid of pictures", true),
                ("", "The logo", true),
                ("Not there", "", false),
            ]
        );
    }

    /// What the Markdown specification and `pulldown_cmark::html` put in
    /// `alt`, with the image inside the description named, never loaded.
    #[test]
    fn a_description_is_the_plain_content_of_its_brackets() {
        let mut asked = Vec::new();
        let blocks = super::parse(
            "![Run `ls -a` *here*\nthen ![the inner](inner.png) [a link](u) <kbd>K</kbd>](x.png)\n",
            Bounds::QUICK,
            &CancelToken::new(),
            |href| {
                asked.push(href.to_owned());
                None
            },
            None,
        )
        .unwrap()
        .blocks;
        assert_eq!(asked, ["x.png"]);
        assert_eq!(
            blocks.iter().map(|b| b.text.as_str()).collect::<Vec<_>>(),
            ["Run ls -a here then the inner a link <kbd>K</kbd>"]
        );
    }

    #[test]
    fn raw_html_blocks_are_read_rather_than_dropped() {
        let mut asked = Vec::new();
        let blocks = super::parse(
            "<p align=\"center\"><img src=\"logo.png\" alt=\"Logo\"></p>\n\nText.\n",
            Bounds::QUICK,
            &CancelToken::new(),
            |href| {
                asked.push(href.to_owned());
                None
            },
            None,
        )
        .unwrap()
        .blocks;
        assert_eq!(asked, ["logo.png"], "its images use the document's loader");
        assert!(blocks.iter().any(|b| b.text == "Logo"));
        assert!(blocks.iter().any(|b| b.text == "Text."));
    }

    #[test]
    fn every_image_in_the_document_goes_through_one_loader() {
        use std::fmt::Write;
        let mut source = String::new();
        for i in 0..limits::EMBEDDED_IMAGES + 4 {
            let _ = if i % 2 == 0 {
                write!(source, "![a{i}](a{i}.png)\n\n")
            } else {
                write!(source, "<img src=\"b{i}.png\" alt=\"b{i}\">\n\n")
            };
        }
        let mut asked = 0usize;
        super::parse(
            &source,
            Bounds::QUICK,
            &CancelToken::new(),
            |_| {
                asked += 1;
                None
            },
            None,
        )
        .unwrap();
        // Naming a picture costs its header: none is left out.
        assert_eq!(asked, limits::EMBEDDED_IMAGES + 4);
    }

    /// The cells of a row, read back through the ranges the view draws.
    fn cells_of(block: &Block) -> (Vec<&str>, bool) {
        let BlockKind::TableRow { cells, header } = &block.kind else {
            panic!("not a table row");
        };
        (
            cells.iter().map(|r| &block.text[r.clone()]).collect(),
            *header,
        )
    }

    #[test]
    fn table_rows_are_text_with_the_cells_as_ranges() {
        let blocks = parse("| name | note |\n|---|---|\n| one | two |\n");
        assert_eq!(cells_of(&blocks[0]), (vec!["name", "note"], true));
        assert_eq!(cells_of(&blocks[1]), (vec!["one", "two"], false));
        // The row reads as one line, so copying and searching need no case.
        assert_eq!(blocks[1].text, "one\ttwo");
    }

    /// Each link of `block`: its text, where it leads, whether found.
    fn links(block: &Block) -> Vec<(&str, &str, bool)> {
        block
            .runs
            .iter()
            .filter_map(|r| Some((&block.text[r.range.clone()], r.link.as_deref()?, r.found)))
            .collect()
    }

    #[test]
    fn a_bare_address_is_a_link_as_github_reads_it() {
        let blocks = parse(
            "See https://example.org/a_b_c. Not `https://code.example`, \
             [named](https://named.example) or *https://em.example/x*.\n\n\
             | where |\n|---|\n| mailto:a@b.example |\n",
        );
        assert_eq!(
            links(&blocks[0]),
            [
                (
                    "https://example.org/a_b_c",
                    "https://example.org/a_b_c",
                    false
                ),
                ("named", "https://named.example", false),
                ("https://em.example/x", "https://em.example/x", false),
            ]
        );
        assert_eq!(
            links(&blocks[2]),
            [("mailto:a@b.example", "mailto:a@b.example", false)]
        );
    }

    #[test]
    fn a_cell_keeps_its_emphasis_and_links() {
        let blocks = parse("| what | where |\n|---|---|\n| **bold** `code` | [doc](d.md) |\n");
        let row = &blocks[1];
        assert_eq!(cells_of(row).0, vec!["bold code", "doc"]);
        let run = |pick: &dyn Fn(&Run) -> bool| {
            let r = row.runs.iter().find(|r| pick(r)).expect("run");
            &row.text[r.range.clone()]
        };
        assert_eq!(run(&|r| r.bold), "bold");
        assert_eq!(run(&|r| r.code), "code");
        assert_eq!(run(&|r| r.link.as_deref() == Some("d.md")), "doc");
    }

    #[test]
    fn headings_carry_a_link_target() {
        let blocks = parse(
            "# The Plan!\n\n## The Plan\n\n### Explicit {#Named_Part}\n\n## my_func\n\n## a -- b\n\n## The Plan-1\n",
        );
        let anchors: Vec<_> = blocks
            .iter()
            .map(|b| b.anchors.first().map(String::as_str))
            .collect();
        assert_eq!(
            anchors,
            [
                Some("the-plan"),
                Some("the-plan-1"),
                Some("Named_Part"),
                Some("my_func"),
                Some("a----b"),
                Some("the-plan-1-1"),
            ],
            "GitHub's names, explicit ids verbatim, repeats made distinct"
        );
    }

    #[test]
    fn a_repeated_heading_costs_a_lookup_not_a_scan() {
        let source = "# a\n".repeat(limits::BLOCKS - 1);
        let started = std::time::Instant::now();
        let blocks = parse(&source);
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        assert_eq!(blocks[1].anchors, ["a-1"]);
        let last = format!("a-{}", limits::BLOCKS - 2);
        assert_eq!(blocks.last().unwrap().anchors, [last]);
    }

    #[test]
    fn a_long_document_is_cut_at_the_bound_not_refused() {
        let source = "- item\n".repeat(limits::BLOCKS + 500);
        let page =
            super::parse(&source, Bounds::QUICK, &CancelToken::new(), |_| None, None).unwrap();
        assert!(page.truncated);
        assert_eq!(page.blocks.len(), limits::BLOCKS);
        let short = super::parse(
            "- one\n- two\n",
            Bounds::QUICK,
            &CancelToken::new(),
            |_| None,
            None,
        )
        .unwrap();
        assert!(!short.truncated);
        let all =
            super::parse(&source, Bounds::WHOLE, &CancelToken::new(), |_| None, None).unwrap();
        assert!(!all.truncated);
        assert_eq!(all.blocks.len(), limits::BLOCKS + 500);
    }

    #[test]
    fn front_matter_is_not_read_as_a_heading() {
        let blocks = parse("---\ntitle: Note\ntags: [a, b]\n---\n\n# Real heading\n\nBody.\n");
        assert_eq!(
            blocks.iter().map(|b| b.text.as_str()).collect::<Vec<_>>(),
            ["Real heading", "Body."]
        );
        assert!(blocks.iter().all(|b| b.kind == BlockKind::Text));
    }

    #[test]
    fn rules_quotes_and_code_are_marked() {
        let blocks = parse("---\n\n> quoted\n\n```rust\nfn main() {}\n```\n");
        assert_eq!(blocks[0].kind, BlockKind::Rule);
        assert!(blocks[1].quote);
        assert_eq!(blocks[1].text, "quoted");
        assert!(blocks[2].code);
        assert_eq!(blocks[2].language.as_deref(), Some("rust"));
        assert_eq!(blocks[2].text, "fn main() {}");
    }

    #[test]
    fn task_list_markers_are_kept_in_text() {
        let blocks = parse("- [x] done\n- [ ] todo\n");
        assert_eq!(blocks[0].text, "☑ done");
        assert_eq!(blocks[1].text, "☐ todo");
    }

    /// Obsidian's links lead where the note they name is: beside the note,
    /// else from the vault's top, else where Obsidian would make it. A
    /// heading is a slug, a label is the link's text.
    #[test]
    fn obsidian_links_lead_to_the_notes_they_name() {
        let vault = tempfile::tempdir().unwrap();
        let projects = vault.path().join("Projects");
        std::fs::create_dir_all(vault.path().join(".obsidian")).unwrap();
        std::fs::create_dir_all(&projects).unwrap();
        for note in [
            "Projects/Next steps.md",
            "Projects/l'été.md",
            "Inbox.md",
            "Projects/plan.txt",
        ] {
            std::fs::write(vault.path().join(note), "").unwrap();
        }
        let source = "[[Next steps]] [[Inbox#Read later|later]] [[l'été]] [[plan.txt]] \
                      [[Nowhere]] [[#Plan of attack]] [[Next steps#^abc]] [[100% sure?]]";
        let mut wiki = Wiki::new(&projects.join("plan.md")).unwrap();
        let page = super::parse(
            source,
            Bounds::QUICK,
            &CancelToken::new(),
            |_| None,
            Some(&mut wiki),
        )
        .unwrap();
        let links: Vec<(&str, &str)> =
            page.blocks
                .iter()
                .flat_map(|block| {
                    block.runs.iter().filter_map(|run| {
                        Some((&block.text[run.range.clone()], run.link.as_deref()?))
                    })
                })
                .collect();
        assert_eq!(
            links,
            [
                ("Next steps", "Next steps.md"),
                ("later", "../Inbox.md#read-later"),
                ("l'été", "l'été.md"),
                ("plan.txt", "plan.txt"),
                ("Nowhere", "Nowhere.md"),
                ("#Plan of attack", "#plan-of-attack"),
                ("Next steps#^abc", "Next steps.md#^abc"),
                ("100% sure?", "100%25 sure%3F.md"),
            ]
        );
    }
}
