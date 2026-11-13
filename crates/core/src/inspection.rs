//! Bounded, toolkit-independent content previews. Providers execute on workers.
use crate::thumbs::Image;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct InspectRequest {
    /// Explicit provider, e.g. `hex`; None chooses by type.
    pub provider: Option<String>,
    pub source: bool,
    pub offset: u64,
    /// Container member or chapter, never a filesystem output path.
    pub target: Option<String>,
    pub query: Option<String>,
    /// Whether the asker draws a 3D mesh itself (a window whose GPU has a
    /// pass for it, ADR-067): a provider may then hand over the mesh it
    /// read (`Inspection::mesh`) rather than a picture of it.
    #[serde(default)]
    pub meshes: bool,
    /// Whether the user wants a model turned in 3D (`Some(true)`) or shown
    /// by its picture (`Some(false)`), when the asker draws meshes; `None`
    /// is the provider's way: a mesh turned, a model that stores a picture
    /// shown by it.
    #[serde(default)]
    pub turned: Option<bool>,
    /// Whether the user asked to see all of this file (Full preview), past
    /// the size at which a preview shows less to stay quick: a file too
    /// large to read whole, a document's first blocks, a mesh too large to
    /// turn unasked. Never past a bound that keeps hostile input out. Kept
    /// for the file shown, never for the next.
    #[serde(default)]
    pub whole: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InspectionKind {
    #[default]
    Text,
    /// Rendered Markdown: reading typography and block layout apply.
    Markdown,
    /// One bounded source line per block, with optional folding ranges.
    Code,
    Hex,
    Table,
    Font,
    Archive,
    Book,
    Comic,
    Mail,
    /// Places on a map (`Inspection::geo`): tracks, routes, waypoints.
    Map,
    /// A 3D model the asker turns (`Inspection::mesh`).
    Model,
    /// A history of commits, newest first (ADR-133): each row a commit
    /// whose `target` opens it (its whole id, and the file's name in it
    /// where it had another: `commit_target`), its cells its summary, its
    /// short id, its author, its date and the names at it (branches,
    /// tags), joined by `REFS_JOIN`; its `graph`, where the history draws
    /// one, its part of the graph of merges.
    History,
}

/// What joins the names at a commit in a history row's fifth cell, each
/// written as its kind says (`RefKind::written`).
pub const REFS_JOIN: &str = "\u{1f}";

/// A history row's `target`: the commit's whole id, then, where the file
/// whose history it is had another name in that commit (before a rename),
/// that name after a space. An id never holds a space; a name may, so the
/// target is split at its first.
pub fn commit_target(id: &str, name: Option<&str>) -> String {
    match name {
        Some(name) => format!("{id} {name}"),
        None => id.to_owned(),
    }
}

/// The commit a history row's target opens, its whole id (`commit_target`).
pub fn commit_of(target: &str) -> &str {
    target.split_once(' ').map_or(target, |(id, _)| id)
}

/// The file's name in the commit a history row's target opens, where it
/// had another than its own (`commit_target`).
pub fn commit_name(target: &str) -> Option<&str> {
    target.split_once(' ').map(|(_, name)| name)
}

/// What a name at a commit is: the branch checked out, another local
/// branch, a remote-tracking one, a tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefKind {
    Current,
    Branch,
    Remote,
    Tag,
}

impl RefKind {
    const PREFIXES: [(RefKind, &'static str); 4] = [
        (RefKind::Current, "head:"),
        (RefKind::Branch, "branch:"),
        (RefKind::Remote, "remote:"),
        (RefKind::Tag, "tag:"),
    ];

    /// `name` written as one of a history row's names.
    pub fn written(self, name: &str) -> String {
        let prefix = Self::PREFIXES
            .iter()
            .find(|(kind, _)| *kind == self)
            .map_or("branch:", |(_, prefix)| prefix);
        format!("{prefix}{name}")
    }

    /// A name as `written` wrote it: what it is, and the name; a name
    /// written otherwise is a branch's.
    pub fn of(written: &str) -> (RefKind, &str) {
        Self::PREFIXES
            .iter()
            .find_map(|(kind, prefix)| Some((*kind, written.strip_prefix(prefix)?)))
            .unwrap_or((RefKind::Branch, written))
    }
}

/// A row's part of a graph drawn down the start of the rows (a history's
/// commits and their parents, `InspectionKind::History`): lanes side by
/// side, the row's node in one, each line in a lane's color, which the
/// asker draws from a few it cycles through.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GraphRow {
    /// The lane the row's node is in.
    pub node: u16,
    /// The color of the node's lane.
    pub color: u8,
    /// The lines through the row.
    pub lines: Vec<GraphLine>,
}

/// A line of a graph through one row, from lane `from` to lane `to`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GraphLine {
    pub from: u16,
    pub to: u16,
    pub color: u8,
    pub part: GraphPart,
}

/// Where a line of a row's graph runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphPart {
    /// From the row's top to its node (`to` is the node's lane): a child
    /// above coming down to its parent.
    Above,
    /// From the node to the row's bottom (`from` is the node's lane): to a
    /// parent below.
    Below,
    /// From the row's top to its bottom: a line going by.
    Through,
}
#[derive(Clone, Debug, Default)]
pub struct Inspection {
    pub kind: InspectionKind,
    pub title: String,
    pub blocks: Vec<Block>,
    pub rows: Vec<Row>,
    pub columns: Vec<String>,
    /// Code block ranges: the first line remains visible, the rest can fold.
    /// Sorted by start; ranges must be nested or disjoint, never crossing.
    pub folds: Vec<std::ops::Range<usize>>,
    pub image: Option<Image>,
    pub previous: Option<u64>,
    pub next: Option<u64>,
    pub position: u64,
    /// How many sections the provider numbers, whose offsets are their
    /// indices (a book's chapters): the toolbar's field goes to one by its
    /// number, as a document's goes to a page. 0: not numbered.
    pub sections: u64,
    /// The pages of the printed book, in order, when it lists them (an
    /// EPUB's `page-list`): where the toolbar counts by printed page.
    pub printed_pages: Vec<PrintedPage>,
    pub source_available: bool,
    pub notice: String,
    /// What the format knows about the file, for the panel's Properties: a
    /// book's author, a font's family, an archive's unpacked size. Facts read
    /// from the content, in the order they should be shown, bounded like the
    /// rest: a provider already parsed the file to build this inspection, so
    /// they cost nothing more.
    pub details: Vec<Detail>,
    /// What an [`InspectionKind::Map`] places on the map.
    pub geo: Option<std::sync::Arc<crate::geo::GeoDoc>>,
    /// What it draws over the background: the tiles a tile archive holds.
    pub tiles: Option<crate::tiles::Layer>,
    /// The file it is the text of, read where it is shown (ADR-051): its
    /// blocks are only its first rows, and the view reads the rest.
    pub text: Option<TextFile>,
    /// What an [`InspectionKind::Model`] shows.
    pub mesh: Option<std::sync::Arc<Mesh>>,
    /// Whether the file is a model whose mesh can be turned in 3D, asked
    /// with `InspectRequest::meshes` and `turned`.
    pub mesh_available: bool,
    /// Whether this is less than the file holds, cut short to stay quick:
    /// asked again with `InspectRequest::whole`, the provider shows all of
    /// it. Never set when it was asked so: what a bound that keeps hostile
    /// input out leaves out stays out.
    pub whole_available: bool,
}

/// A file a preview refused only because it is large: past a bound that
/// keeps previews quick, not one that keeps hostile input out. Asked again
/// with `InspectRequest::whole`, the provider reads it; the panel offers
/// that (Full preview). Never made when the file was asked whole.
#[derive(Debug)]
pub struct TooLarge(pub String);

impl std::fmt::Display for TooLarge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for TooLarge {}

impl TooLarge {
    /// Whether `error` is a [`TooLarge`], or was caused by one.
    pub fn is(error: &anyhow::Error) -> bool {
        error.chain().any(<dyn std::error::Error>::is::<TooLarge>)
    }
}

/// A 3D mesh as a provider read it (ADR-067): positions, in the file's
/// unit with Z up, and the triangles between them, three indices each, and
/// each position's colour when the file gives them (sRGB bytes, red, green,
/// blue, alpha). Checked when made, so a view hands it to the GPU as it is:
/// it has triangles, every index names a position, every coordinate is
/// finite, there is a colour a position or none.
#[derive(Clone, PartialEq)]
pub struct Mesh {
    positions: Vec<[f32; 3]>,
    indices: Vec<u32>,
    colours: Option<Vec<[u8; 4]>>,
}

impl Mesh {
    pub fn new(positions: Vec<[f32; 3]>, indices: Vec<u32>) -> anyhow::Result<Self> {
        anyhow::ensure!(!indices.is_empty(), "the mesh has no triangles");
        anyhow::ensure!(
            indices.len().is_multiple_of(3),
            "the mesh's indices are not whole triangles"
        );
        let count = positions.len();
        if let Some(index) = indices.iter().find(|&&i| i as usize >= count) {
            anyhow::bail!("the mesh's index {index} names no position (it has {count})");
        }
        anyhow::ensure!(
            positions.iter().flatten().all(|v| v.is_finite()),
            "the mesh has a position that is not finite"
        );
        Ok(Mesh {
            positions,
            indices,
            colours: None,
        })
    }

    /// The same mesh with a colour for each position.
    pub fn with_colours(self, colours: Vec<[u8; 4]>) -> anyhow::Result<Self> {
        anyhow::ensure!(
            colours.len() == self.positions.len(),
            "the mesh has {} colours for {} positions",
            colours.len(),
            self.positions.len()
        );
        Ok(Mesh {
            colours: Some(colours),
            ..self
        })
    }

    pub fn colours(&self) -> Option<&[[u8; 4]]> {
        self.colours.as_deref()
    }

    pub fn positions(&self) -> &[[f32; 3]] {
        &self.positions
    }

    /// Three indices into the positions a triangle.
    pub fn indices(&self) -> &[u32] {
        &self.indices
    }

    pub fn triangles(&self) -> usize {
        self.indices.len() / 3
    }

    /// Its positions, indices and colours, for a view to hand on.
    pub fn into_parts(self) -> (Vec<[f32; 3]>, Vec<u32>, Option<Vec<[u8; 4]>>) {
        (self.positions, self.indices, self.colours)
    }
}

impl std::fmt::Debug for Mesh {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mesh")
            .field("positions", &self.positions.len())
            .field("triangles", &self.triangles())
            .finish_non_exhaustive()
    }
}

/// A text file a view reads where it shows it, a few rows at a time
/// (ADR-051): from what the preview holds of it (ADR-101), whole up to
/// `preview::WHOLE`, else from the file.
#[derive(Clone, Debug)]
pub struct TextFile {
    pub path: std::path::PathBuf,
    /// What it is colored as: an extension or a name, as the highlighter
    /// takes it.
    pub language: Option<String>,
    /// Whether its first rows are all of it: nothing is left to count.
    pub whole: bool,
    /// What the preview holds of it, read instead of the file: `None` for
    /// a text read by its path (a member written out, a test).
    pub held: Option<std::sync::Arc<crate::preview::Held>>,
}

impl PartialEq for TextFile {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
            && self.language == other.language
            && self.whole == other.whole
            && match (&self.held, &other.held) {
                (Some(a), Some(b)) => std::sync::Arc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            }
    }
}

impl Eq for TextFile {}

/// A text file is read from what the preview holds of it, else from its
/// path.
impl crate::preview::FileSource for TextFile {
    fn path(&self) -> &std::path::Path {
        &self.path
    }
    fn length(&self) -> std::io::Result<u64> {
        match &self.held {
            Some(held) => held.length(),
            None => self.path.length(),
        }
    }
    fn first_bytes(&self) -> std::io::Result<std::borrow::Cow<'_, [u8]>> {
        match &self.held {
            Some(held) => held.first_bytes(),
            None => self.path.first_bytes(),
        }
    }
    fn read_range(
        &self,
        offset: u64,
        limit: usize,
        cancel: &crate::CancelToken,
    ) -> std::io::Result<std::borrow::Cow<'_, [u8]>> {
        match &self.held {
            Some(held) => held.read_range(offset, limit, cancel),
            None => self.path.read_range(offset, limit, cancel),
        }
    }
    fn read_whole(
        &self,
        limit: u64,
        cancel: &crate::CancelToken,
    ) -> std::io::Result<Option<std::borrow::Cow<'_, [u8]>>> {
        match &self.held {
            Some(held) => held.read_whole(limit, cancel),
            None => self.path.read_whole(limit, cancel),
        }
    }
    fn open_reader(&self) -> std::io::Result<Box<dyn crate::preview::ReadSeek + Send>> {
        match &self.held {
            Some(held) => held.open_reader(),
            None => self.path.open_reader(),
        }
    }
}
/// A page of the printed book (`Inspection::printed_pages`): its number as
/// printed ("xii", "47"), the section that holds it, and the link to where
/// it starts in it (the section's target, with the fragment of its mark).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrintedPage {
    pub label: String,
    pub section: u64,
    pub target: String,
}

/// One fact of [`Inspection::details`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Detail {
    pub label: String,
    pub value: String,
    /// What the value leads to, drawn as a link after it: a picture's
    /// position shows it on the map. A command like any other, so it
    /// fails as one when its owner is disabled.
    pub action: Option<(String, crate::command::Invocation)>,
}
impl Detail {
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Detail {
            label: label.into(),
            value: value.into(),
            action: None,
        }
    }

    /// The same fact with a link, `words` running `action`.
    #[must_use]
    pub fn with_action(
        mut self,
        words: impl Into<String>,
        action: crate::command::Invocation,
    ) -> Self {
        self.action = Some((words.into(), action));
        self
    }
}
/// What a block draws. A block is text, a thematic break or one row of a
/// table, never two of those at once: the shape decides which fields mean
/// anything, so it is one value rather than a set of flags that can disagree.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum BlockKind {
    /// `text` with its `runs`, plus the ambient heading/code/quote/list state.
    #[default]
    Text,
    /// A thematic break, drawn as a rule; `text` is empty.
    Rule,
    /// One row of a table. Its cells are ranges of `text`, which holds them
    /// joined by tabs: a row is text like any other block, so its emphasis,
    /// code and links are ordinary `runs`, and search, selection and copy
    /// need no special case. The view draws each range in its own column.
    TableRow {
        cells: Vec<std::ops::Range<usize>>,
        header: bool,
    },
}
/// Where a block's lines sit in the column.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Start,
    /// A book's title page, a README's logo line.
    Center,
    End,
}
/// How a block follows the one before it as its text runs: what sets it
/// apart from it, or keeps it on it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Flow {
    /// It comes next, as the document's blocks do: the words of an item of
    /// a tight list, the half of a paragraph after what cut it (a picture,
    /// a formula set apart).
    #[default]
    Next,
    /// It begins a paragraph, as the document wrote one (Markdown's, HTML's
    /// `p`). An item that does is one of a loose list, set apart from the
    /// item before it in reading mode.
    Paragraph,
    /// The rest of the block before it, cut from a line too long for one
    /// (`split_blocks`): it goes on that block, nothing between them.
    Rest,
}

#[derive(Clone, Debug, Default)]
pub struct Block {
    /// The names a link can reach this block by (`#name`): a heading's slug,
    /// or every `id` in a page that lands here, the block's own and those of
    /// the sections it opens and the words inside it.
    pub anchors: Vec<String>,
    pub language: Option<String>,
    pub kind: BlockKind,
    pub text: String,
    /// 0 paragraph; 1..=6 heading.
    pub heading: u8,
    pub code: bool,
    pub quote: bool,
    /// List nesting level: 0 outside a list, 1 for a top-level item.
    pub list_depth: u32,
    /// How it follows the block before it as the text runs (`Flow`).
    pub flow: Flow,
    /// The bullet or number that introduces a list item, drawn beside it.
    pub marker: Option<String>,
    /// Where the lines sit, as the document's own styling says.
    pub align: Align,
    /// Extra left indent, in ems, from the document's own margins (a letter,
    /// a verse, a definition): bounded, and apart from quotes and lists,
    /// which are structure.
    pub indent: f32,
    /// Text size relative to the body, when the document sets a paragraph
    /// apart by it (a title page line, small print). Headings and code keep
    /// the reading typography's sizes.
    pub size: Option<f32>,
    pub runs: Vec<Run>,
    pub picture: Option<Picture>,
    /// The words `picture` stands for, drawn in its box only if it cannot be
    /// decoded (`set_picture`).
    pub alt: String,
    /// The note this block is part of (a footnote, an endnote), numbered in
    /// its page: the blocks of one note share it, and a reference to any of
    /// its names opens them together.
    pub note: Option<u32>,
    /// Words said where they stand in the text and not shown: what a page
    /// writes for screen readers alone (a visually hidden label), at the
    /// byte of `text` before which they are said (`Block::said`).
    pub unseen: Vec<(usize, String)>,
}
impl Block {
    /// The character a drawing stands in for in a block's text (`Drawing`):
    /// the object replacement character, which nothing else writes.
    pub const DRAWN: char = '\u{fffc}';

    /// Append `drawing` to the text, in a run of its own carrying `run`'s
    /// styles (its link, its emphasis).
    pub fn push_drawing(&mut self, drawing: std::sync::Arc<Drawing>, run: &Run) {
        let start = self.text.len();
        self.text.push(Self::DRAWN);
        self.runs.push(Run {
            range: start..self.text.len(),
            drawing: Some(drawing),
            ..run.clone()
        });
    }

    /// Write `text` in place of the drawing run `run` holds, as code: a
    /// formula that cannot be drawn shows the TeX it is written in. What
    /// follows it in the text (runs, a row's cells) moves along.
    pub fn undraw(&mut self, run: usize, text: &str) {
        let Some(at) = self.runs.get(run).map(|run| run.range.clone()) else {
            return;
        };
        if self.text.get(at.clone()).is_none() {
            return;
        }
        self.text.replace_range(at.clone(), text);
        let moved = |offset: usize| {
            if offset >= at.end {
                offset - at.len() + text.len()
            } else {
                offset
            }
        };
        for (index, each) in self.runs.iter_mut().enumerate() {
            if index == run {
                each.range = at.start..at.start + text.len();
                each.drawing = None;
                each.code = true;
            } else {
                each.range = moved(each.range.start)..moved(each.range.end);
            }
        }
        if let BlockKind::TableRow { cells, .. } = &mut self.kind {
            for cell in cells {
                *cell = moved(cell.start)..moved(cell.end);
            }
        }
    }

    /// The text of `range` as a screen reader says it: as copied
    /// (`copied`), without what is shown only (`Run::unsaid`), with what is
    /// said only (`Block::unseen`) where it stands.
    pub fn said(&self, range: std::ops::Range<usize>) -> std::borrow::Cow<'_, str> {
        let at_end = range.end == self.text.len();
        let unseen: Vec<&(usize, String)> = self
            .unseen
            .iter()
            .filter(|(at, _)| range.contains(at) || (at_end && *at == range.end))
            .collect();
        let unsaid = self
            .runs
            .iter()
            .any(|run| run.unsaid && run.range.start < range.end && range.start < run.range.end);
        if unseen.is_empty() && !unsaid {
            return self.copied(range);
        }
        // Cut where a run or a word said only begins or ends; runs are in
        // their text's order.
        let mut cuts: Vec<usize> = self
            .runs
            .iter()
            .flat_map(|run| [run.range.start, run.range.end])
            .chain(unseen.iter().map(|(at, _)| *at))
            .filter(|at| range.contains(at))
            .chain([range.start, range.end])
            .collect();
        cuts.sort_unstable();
        cuts.dedup();
        let mut unseen = unseen.into_iter().peekable();
        let mut said = String::new();
        for cut in cuts.windows(2) {
            let (from, to) = (cut[0], cut[1]);
            while let Some((_, words)) = unseen.next_if(|(at, _)| *at <= from) {
                said.push_str(words);
            }
            let run = self
                .runs
                .get(self.runs.partition_point(|run| run.range.end <= from))
                .filter(|run| run.range.start <= from);
            if !run.is_some_and(|run| run.unsaid) {
                said.push_str(&self.copied(from..to));
            }
        }
        for (_, words) in unseen {
            said.push_str(words);
        }
        std::borrow::Cow::Owned(said)
    }

    /// The text of `range` as it is copied: each drawing as what it says
    /// (`Drawing::text`), the rest as it is shown.
    pub fn copied(&self, range: std::ops::Range<usize>) -> std::borrow::Cow<'_, str> {
        let Some(text) = self.text.get(range.clone()) else {
            return std::borrow::Cow::Borrowed("");
        };
        let mut drawn: Vec<(std::ops::Range<usize>, &Drawing)> = self
            .runs
            .iter()
            .filter_map(|run| {
                let at = run.range.start.max(range.start)..run.range.end.min(range.end);
                Some((at, run.drawing.as_deref()?)).filter(|(at, _)| !at.is_empty())
            })
            .collect();
        if drawn.is_empty() {
            return std::borrow::Cow::Borrowed(text);
        }
        drawn.sort_by_key(|(at, _)| at.start);
        let mut said = String::with_capacity(text.len());
        let mut from = range.start;
        for (at, drawing) in drawn {
            let Some(drawn) = self.text.get(at.clone()).filter(|_| at.start >= from) else {
                continue;
            };
            said.push_str(&self.text[from..at.start]);
            for letter in drawn.chars() {
                if letter == Self::DRAWN {
                    said.push_str(&drawing.text);
                } else {
                    said.push(letter);
                }
            }
            from = at.end;
        }
        said.push_str(&self.text[from..range.end]);
        std::borrow::Cow::Owned(said)
    }

    /// Show `picture`, described by `alt` (an image's `alt`, a drawing's
    /// title). A description stands in for its picture, as a browser draws
    /// it: never beside a picture shown, in the picture's box if it cannot be
    /// decoded, and as the block's text when there is no picture to show.
    pub fn set_picture(&mut self, picture: Option<Picture>, alt: &str) {
        let alt = alt.trim().to_owned();
        if picture.is_some() {
            self.alt = alt;
        } else {
            self.text = alt;
        }
        self.picture = picture;
    }
}
/// A picture a block shows.
#[derive(Clone, Debug)]
pub enum Picture {
    /// Decoded with the page: a message's embedded image, a drawing.
    Ready(Image),
    /// Named by the page and decoded only when the view shows it: the
    /// provider reads it again by `key` (`PreviewProvider::picture`). Its size
    /// as it will be shown, read from the file's header, holds its place
    /// until then, so the page does not move when it arrives. A book with a
    /// hundred illustrations costs what is on screen.
    Later {
        key: String,
        width: u32,
        height: u32,
    },
}
impl Picture {
    /// The size the picture is shown at.
    pub fn size(&self) -> (u32, u32) {
        match self {
            Self::Ready(image) => (image.width, image.height),
            Self::Later { width, height, .. } => (*width, *height),
        }
    }
}
/// A drawing set in a line of text, in place of one character of it
/// (`Block::DRAWN`), whose run holds it: a formula. It is drawn in the
/// text's ink (its shapes are a mask, whatever colors they name), at the
/// text's size: every length is in ems of the text around it.
#[derive(Clone, Debug)]
pub struct Drawing {
    /// An SVG of outlines, nothing it could fetch.
    pub svg: std::sync::Arc<[u8]>,
    /// What tells this SVG from another (a hash of it): what a view keeps
    /// its raster by.
    pub id: u64,
    /// The box the line holds for it: as wide as `width`, standing `ascent`
    /// above the baseline and reaching `descent` below it.
    pub width: f32,
    pub ascent: f32,
    pub descent: f32,
    /// How far the SVG reaches past each side of the box: what overhangs it
    /// (an italic letter, a radical's hook) is drawn, not laid out.
    pub margin: f32,
    /// What the drawing says, in its place where the text is copied, read
    /// aloud or searched: a formula's TeX, as written.
    pub text: String,
    /// A formula's number (`\tag`), set at the end of the line the formula
    /// stands on, or `Tag::gap` past it where that line has no room.
    pub tag: Option<std::sync::Arc<Tag>>,
}
/// A formula's number, drawn apart from it (`Drawing::tag`): on its
/// baseline, its own box and SVG; `gap` ems clear of the formula at least.
#[derive(Clone, Debug)]
pub struct Tag {
    pub drawing: Drawing,
    pub gap: f32,
}
/// Where a run sits in its line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Script {
    #[default]
    Baseline,
    Superscript,
    Subscript,
}
#[derive(Clone, Debug, Default)]
#[allow(clippy::struct_excessive_bools)] // A run's styles, each on or off alone.
pub struct Run {
    pub range: std::ops::Range<usize>,
    pub color: Option<[u8; 3]>,
    pub bold: bool,
    pub italic: bool,
    pub strikethrough: bool,
    pub underline: bool,
    /// Raised or lowered, and smaller: a note reference, an exponent.
    pub script: Script,
    /// Lowercase letters set as small capitals.
    pub small_caps: bool,
    /// The link is a reference to a note, which the view opens beside the
    /// text rather than going to it.
    pub note: bool,
    /// The link's own name (its `id`, an `a`'s `name`), which a note links
    /// back to: a reference set as plain text (`[1]`) is known by that way
    /// back from what it leads to (`note_popover`).
    pub name: Option<String>,
    pub code: bool,
    pub link: Option<String>,
    /// The link was found in raw text rather than written as one (source,
    /// a log), where a click selects: it is drawn and followed only while
    /// Ctrl (Cmd) is held over it (`skwir_platform::links::link_block`).
    pub found: bool,
    /// Each character of the run is this drawing (`Block::DRAWN`), drawn
    /// where the line holds it.
    pub drawing: Option<std::sync::Arc<Drawing>>,
    /// Shown, and not said: what a page tells screen readers to skip
    /// (`aria-hidden`), a decoration or a copy of what is said otherwise.
    pub unsaid: bool,
}
#[derive(Clone, Debug, Default)]
pub struct Row {
    pub cells: Vec<String>,
    /// Uncompressed byte size of a file/member/attachment, when known.
    pub bytes: Option<u64>,
    pub depth: usize,
    pub branch: bool,
    /// Provider-local member key; only activated by a user command.
    pub target: Option<String>,
    /// For a book's contents, the section the row opens (its offset, among
    /// `Inspection::sections`): how the contents tell where the reader is.
    pub section: Option<u64>,
    /// Its part of the graph drawn down the rows' start, in a history that
    /// draws one (`InspectionKind::History`).
    pub graph: Option<Box<GraphRow>>,
}
impl Inspection {
    pub fn text(title: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            blocks: vec![Block {
                text: text.into(),
                ..Block::default()
            }],
            ..Self::default()
        }
    }
    pub fn plain_text(&self) -> String {
        self.blocks
            .iter()
            .map(|b| b.text.clone())
            .chain(self.rows.iter().map(|r| r.cells.join("\t")))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Longest logical line (in bytes) a block may hold; longer text is split
/// so a single huge line cannot stall layout. Platform's `limits` re-exports it.
pub const LINE_BYTES: usize = 4096;

/// Bound text shaping per virtualized row while preserving UTF-8 and inline spans.
pub fn split_blocks(blocks: Vec<Block>) -> Vec<Block> {
    let mut out = Vec::new();
    for mut block in blocks {
        // A rule has no text and a table row's cells are ranges of its own,
        // so only running text is ever long enough to split.
        if block.text.len() <= LINE_BYTES || block.kind != BlockKind::Text {
            out.push(block);
            continue;
        }
        let mut start = 0;
        // The words said only, in their text's order, each to the piece it
        // stands in (one at the very end to the last).
        let mut unseen = std::mem::take(&mut block.unseen).into_iter().peekable();
        while start < block.text.len() {
            let mut end = (start + LINE_BYTES).min(block.text.len());
            while !block.text.is_char_boundary(end) {
                end -= 1;
            }
            // At a line's end where one ends in it, else after a space: a
            // word is never cut in two. The rest goes on this piece
            // (`Flow::Rest`), on a line of its own.
            if end < block.text.len()
                && let Some(cut) = block.text[start..end]
                    .rfind('\n')
                    .filter(|&newline| newline > 0)
                    .or_else(|| block.text[start..end].rfind(' ').filter(|&space| space > 0))
            {
                end = start + cut + 1;
            }
            let runs = block
                .runs
                .iter()
                .filter_map(|r| {
                    let from = r.range.start.max(start);
                    let to = r.range.end.min(end);
                    if from >= to {
                        return None;
                    }
                    let mut r = r.clone();
                    r.range = from - start..to - start;
                    Some(r)
                })
                .collect();
            out.push(Block {
                text: block.text[start..end].into(),
                runs,
                picture: if start == 0 {
                    block.picture.take()
                } else {
                    None
                },
                alt: if start == 0 {
                    std::mem::take(&mut block.alt)
                } else {
                    String::new()
                },
                anchors: if start == 0 {
                    std::mem::take(&mut block.anchors)
                } else {
                    Vec::new()
                },
                marker: if start == 0 {
                    block.marker.take()
                } else {
                    None
                },
                flow: if start == 0 { block.flow } else { Flow::Rest },
                align: block.align,
                indent: block.indent,
                size: block.size,
                note: block.note,
                // Only text can exceed the limit, so the kind carries over.
                kind: block.kind.clone(),
                language: block.language.clone(),
                heading: block.heading,
                code: block.code,
                quote: block.quote,
                list_depth: block.list_depth,
                unseen: std::iter::from_fn(|| {
                    unseen.next_if(|(at, _)| *at < end || end == block.text.len())
                })
                .map(|(at, words)| (at.saturating_sub(start), words))
                .collect(),
            });
            start = end;
        }
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;

    /// A history row opens its commit, under the name the file had in it
    /// where it had another, a name with spaces whole.
    #[test]
    fn a_commit_s_target_holds_its_id_and_the_file_s_name_in_it() {
        let id = "c9cb00b4025ca661f0182a64ff7f09ce00004a9c";
        assert_eq!(commit_target(id, None), id);
        assert_eq!(commit_of(id), id);
        assert_eq!(commit_name(id), None);
        let target = commit_target(id, Some("old docs/a b.md"));
        assert_eq!(target, format!("{id} old docs/a b.md"));
        assert_eq!(commit_of(&target), id);
        assert_eq!(commit_name(&target), Some("old docs/a b.md"));
    }

    /// What is copied is what is shown, a drawing as its words; what a
    /// screen reader is told leaves out what is shown only and says what is
    /// said only, where it stands.
    #[test]
    fn a_block_is_copied_as_shown_and_said_as_written_for_the_ear() {
        let stars = "★★★★☆";
        let mut block = Block {
            text: format!("Rated {stars}."),
            ..Block::default()
        };
        let at = "Rated ".len();
        block.runs = vec![
            Run {
                range: 0..at,
                ..Run::default()
            },
            Run {
                range: at..at + stars.len(),
                unsaid: true,
                ..Run::default()
            },
            Run {
                range: at + stars.len()..block.text.len(),
                ..Run::default()
            },
        ];
        block.unseen = vec![(at + stars.len(), "4 out of 5".into())];
        let whole = 0..block.text.len();
        assert_eq!(block.copied(whole.clone()), "Rated ★★★★☆.");
        assert_eq!(block.said(whole), "Rated 4 out of 5.");
        assert_eq!(block.said(0..at), "Rated ", "a part says its own words");
        let plain = Block {
            text: "Plain".into(),
            ..Block::default()
        };
        assert!(matches!(
            plain.said(0..5),
            std::borrow::Cow::Borrowed("Plain")
        ));
    }
    #[test]
    fn split_preserves_unicode_and_ranges() {
        let text = "é".repeat(6000);
        let blocks = split_blocks(vec![Block {
            text: text.clone(),
            runs: vec![Run {
                range: 0..text.len(),
                bold: true,
                ..Run::default()
            }],
            ..Block::default()
        }]);
        assert_eq!(
            blocks.iter().map(|b| b.text.as_str()).collect::<String>(),
            text
        );
        assert!(
            blocks
                .iter()
                .all(|b| b.text.len() <= LINE_BYTES && b.runs[0].range.end == b.text.len())
        );
    }

    /// A line too long for one block is cut after a space, never in a word,
    /// and its rest goes on the piece before it.
    #[test]
    fn a_long_line_is_cut_between_words_and_goes_on() {
        let text = "word ".repeat(LINE_BYTES / 5 + 100);
        let blocks = split_blocks(vec![Block {
            flow: Flow::Paragraph,
            runs: vec![Run {
                range: 0..text.len(),
                ..Run::default()
            }],
            text: text.clone(),
            ..Block::default()
        }]);
        assert_eq!(blocks.len(), 2);
        assert!(
            blocks[0].text.ends_with("word "),
            "{:?}",
            &blocks[0].text[4080..]
        );
        assert!(blocks[1].text.starts_with("word"));
        assert_eq!(
            blocks.iter().map(|b| b.text.as_str()).collect::<String>(),
            text
        );
        assert_eq!(
            blocks.iter().map(|b| b.flow).collect::<Vec<_>>(),
            [Flow::Paragraph, Flow::Rest]
        );
    }

    #[test]
    fn a_table_row_copies_as_one_tab_separated_line() {
        let row = |cells: &[&str], header| {
            let mut text = String::new();
            let mut ranges = Vec::new();
            for cell in cells {
                if !ranges.is_empty() {
                    text.push('\t');
                }
                let start = text.len();
                text.push_str(cell);
                ranges.push(start..text.len());
            }
            Block {
                kind: BlockKind::TableRow {
                    cells: ranges,
                    header,
                },
                text,
                ..Block::default()
            }
        };
        let data = Inspection {
            blocks: vec![
                Block {
                    text: "Before".into(),
                    ..Block::default()
                },
                row(&["a", "b"], true),
                row(&["1", "2"], false),
            ],
            ..Inspection::default()
        };
        assert_eq!(data.plain_text(), "Before\na\tb\n1\t2");
    }
}
