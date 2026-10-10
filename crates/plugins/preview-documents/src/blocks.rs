//! What a Word or `OpenDocument` reader writes into: semantic blocks, as the
//! reading mode draws them (`inspection::Block`), built straight from the
//! document's structure, never through HTML, so a list keeps the marker
//! the document numbers it with.
//!
//! A long document is cut into parts between paragraphs or table rows
//! once a part holds `PART_BLOCKS`; only the part asked for is kept, the
//! others are counted. Its notes follow it under a rule.
use skwir_core::inspection::{Align, Block, BlockKind, Picture, Run, Script};
use skwir_platform::inspection::limits;

/// Blocks of text a part holds before it is cut: what is left of
/// `limits::BLOCKS` is for its notes.
pub(crate) const PART_BLOCKS: usize = limits::BLOCKS * 3 / 4;

/// How a run of text looks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct Look {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub script: Script,
    pub small_caps: bool,
    /// Set in capitals whatever was typed.
    pub caps: bool,
    /// Not shown: the document hides it.
    pub hidden: bool,
    /// Set as code: a character style for it.
    pub code: bool,
}

/// What a paragraph is.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Shape {
    /// 0 a paragraph, 1..=6 a heading.
    pub heading: u8,
    pub align: Align,
    /// Left indent in ems, outside lists and quotes.
    pub indent: f32,
    pub quote: bool,
    pub code: bool,
    /// List nesting, 0 outside a list.
    pub list_depth: u32,
    /// The number or bullet the document gives the paragraph.
    pub marker: Option<String>,
}

/// The largest indent kept, in ems: a margin, not a layout.
const INDENT: f32 = 8.;

/// A table row being written: its cells are ranges of one block's text.
struct Row {
    header: bool,
    cells: usize,
    /// Where the current cell's text begins.
    cell_start: usize,
}

#[derive(Default)]
pub(crate) struct Builder {
    /// The blocks of the part being written.
    blocks: Vec<Block>,
    current: Block,
    shape: Shape,
    /// The marker is drawn beside the paragraph's first block only.
    marked: bool,
    /// Names for the next block (a bookmark before any text).
    anchors: Vec<String>,
    row: Option<Row>,
    /// Pictures met in a row, which stand after it.
    after_row: Vec<Block>,
    /// The note the blocks being written belong to.
    note: Option<u32>,
    /// Which part is kept, and which part is being written.
    wanted: u64,
    part: u64,
    /// The part asked for, once written: its blocks and the notes it
    /// refers to, in order.
    kept: Option<(Vec<Block>, Vec<String>)>,
    /// Until then, the last part written, in case the one asked for is past
    /// the end: its number, blocks and notes.
    last: Option<(u64, Vec<Block>, Vec<String>)>,
    /// The notes the part being written refers to.
    notes: Vec<String>,
    /// Words of running text in the whole document.
    pub words: usize,
    /// The body ended: what is written now (its notes) is the part kept.
    ended: bool,
}

impl Builder {
    /// A builder keeping part `wanted` (0-based) of the document.
    pub(crate) fn new(wanted: u64) -> Self {
        Self {
            wanted,
            ..Self::default()
        }
    }

    fn filled(&self) -> bool {
        !self.current.text.is_empty() || self.current.picture.is_some()
    }

    /// Whether what is written now is shown: the part kept, or its notes.
    /// What another part holds is only counted (its words, its blocks,
    /// where it ends), so its pictures need not be read for their size.
    pub(crate) fn shows_this_part(&self) -> bool {
        self.ended || self.part == self.wanted
    }

    /// A paragraph begins. In a table cell it is a new line of the cell's
    /// text, its marker written in it.
    pub(crate) fn paragraph(&mut self, mut shape: Shape) {
        if let Some(row) = &self.row {
            if self.current.text.len() > row.cell_start {
                self.current.text.push('\n');
            }
            if let Some(marker) = shape.marker {
                self.current.text.push_str(&marker);
                self.current.text.push(' ');
            }
            return;
        }
        if self.filled() {
            self.finish();
        }
        // A numbered heading says its number in its text: it is not a list.
        if shape.heading > 0
            && let Some(marker) = shape.marker.take()
        {
            self.current.text = format!("{marker} ");
            shape.list_depth = 0;
        }
        if shape.list_depth > 0 || shape.quote {
            shape.indent = 0.;
        }
        shape.indent = shape.indent.clamp(0., INDENT);
        self.shape = shape;
        self.marked = false;
    }

    /// Text in the paragraph, as it looks, a link when it is one.
    pub(crate) fn text(&mut self, text: &str, look: &Look, link: Option<&str>) {
        if look.hidden || text.is_empty() {
            return;
        }
        let keep_tabs = self.shape.code && self.row.is_none();
        let mut text: String = text
            .chars()
            .map(|c| if c == '\t' && !keep_tabs { ' ' } else { c })
            .collect();
        if look.caps {
            text = text.to_uppercase();
        }
        let start = self.current.text.len();
        self.current.text.push_str(&text);
        self.current.runs.push(Run {
            range: start..self.current.text.len(),
            bold: look.bold,
            italic: look.italic,
            underline: look.underline,
            strikethrough: look.strike,
            script: look.script,
            small_caps: look.small_caps,
            link: link.map(str::to_owned),
            code: look.code && !self.shape.code,
            ..Run::default()
        });
    }

    /// A reference to note `target`, labelled as the document numbers it.
    pub(crate) fn reference(&mut self, label: &str, target: &str) {
        let start = self.current.text.len();
        self.current.text.push_str(label);
        self.current.runs.push(Run {
            range: start..self.current.text.len(),
            script: Script::Superscript,
            note: true,
            link: Some(format!("#{target}")),
            ..Run::default()
        });
        if self.note.is_none() && !self.notes.iter().any(|n| n == target) {
            self.notes.push(target.to_owned());
        }
    }

    pub(crate) fn line_break(&mut self) {
        if !self.current.text.is_empty() {
            self.current.text.push('\n');
        }
    }

    /// A name a link can reach this place by.
    pub(crate) fn anchor(&mut self, name: &str) {
        if self.row.is_none() && self.filled() {
            self.current.anchors.push(name.to_owned());
        } else {
            self.anchors.push(name.to_owned());
        }
    }

    /// A picture ends the block it sits in; the paragraph goes on after it
    /// in the same list or quote. In a table it stands after the row. One
    /// that cannot be shown says its description, when it has one; one that
    /// is shown does not, since a word processor writes descriptions nobody
    /// typed ("Description automatically generated").
    pub(crate) fn picture(&mut self, alt: &str, picture: Option<Picture>) {
        if picture.is_none() && alt.trim().is_empty() {
            return;
        }
        let mut block = Block {
            text: if picture.is_some() {
                String::new()
            } else {
                alt.trim().to_owned()
            },
            picture,
            note: self.note,
            ..Block::default()
        };
        if self.row.is_some() {
            self.after_row.push(block);
            return;
        }
        if self.filled() {
            self.finish();
        }
        block.align = self.shape.align;
        block.quote = self.shape.quote;
        block.list_depth = self.shape.list_depth;
        // A list item that begins with its picture gives it the marker.
        if !self.marked {
            block.marker.clone_from(&self.shape.marker);
            self.marked = true;
        }
        block.anchors.append(&mut self.anchors);
        self.blocks.push(block);
    }

    /// The paragraph ends.
    pub(crate) fn end_paragraph(&mut self) {
        if self.row.is_none() && self.filled() {
            self.finish();
        }
    }

    /// A rule across the page.
    pub(crate) fn rule(&mut self) {
        if self.filled() {
            self.finish();
        }
        self.blocks.push(Block {
            kind: BlockKind::Rule,
            ..Block::default()
        });
    }

    pub(crate) fn begin_row(&mut self, header: bool) {
        if self.filled() {
            self.finish();
        }
        self.shape = Shape::default();
        self.row = Some(Row {
            header,
            cells: 0,
            cell_start: 0,
        });
    }

    pub(crate) fn cell(&mut self) {
        if let Some(row) = &mut self.row {
            if row.cells > 0 {
                self.current.text.push('\t');
            }
            row.cells += 1;
            row.cell_start = self.current.text.len();
        }
    }

    pub(crate) fn end_row(&mut self) {
        let Some(row) = self.row.take() else {
            return;
        };
        let mut block = std::mem::take(&mut self.current);
        let mut cells = Vec::new();
        let mut start = 0;
        for (index, _) in block.text.match_indices('\t') {
            cells.push(start..index);
            start = index + 1;
        }
        cells.push(start..block.text.len());
        block.kind = BlockKind::TableRow {
            cells,
            header: row.header,
        };
        block.anchors.append(&mut self.anchors);
        block.note = self.note;
        if row.cells > 0 {
            self.blocks.push(block);
        }
        self.blocks.append(&mut self.after_row);
    }

    /// Close the block being written: running text loses the spaces it
    /// ended on, and takes the paragraph's shape.
    fn finish(&mut self) {
        let mut block = std::mem::take(&mut self.current);
        if !self.shape.code && block.picture.is_none() {
            let end = block.text.trim_end().len();
            block.text.truncate(end);
            block.runs.retain_mut(|run| {
                run.range.end = run.range.end.min(end);
                run.range.start < run.range.end
            });
            if block.text.is_empty() {
                return;
            }
        }
        let mut anchors = std::mem::take(&mut self.anchors);
        anchors.append(&mut block.anchors);
        block.anchors = anchors;
        block.heading = self.shape.heading;
        block.align = self.shape.align;
        block.indent = self.shape.indent;
        block.quote = self.shape.quote;
        block.code = self.shape.code;
        block.list_depth = self.shape.list_depth;
        if !self.marked {
            block.marker.clone_from(&self.shape.marker);
            self.marked = true;
        }
        block.note = self.note;
        self.blocks.push(block);
    }

    /// A paragraph or a table row of the body has ended: the part is cut
    /// here once it is full.
    pub(crate) fn boundary(&mut self) {
        if self.blocks.len() >= PART_BLOCKS {
            self.cut();
        }
    }

    fn cut(&mut self) {
        if self.filled() {
            self.finish();
        }
        let blocks = std::mem::take(&mut self.blocks);
        let notes = std::mem::take(&mut self.notes);
        self.words += words(&blocks);
        if self.part == self.wanted {
            self.kept = Some((blocks, notes));
        } else if self.kept.is_none() {
            self.last = Some((self.part, blocks, notes));
        }
        self.part += 1;
    }

    /// The body has ended: the part asked for (the last, when it asked
    /// past the end) is the one written on, followed by its notes, which
    /// this returns with how many parts there are.
    pub(crate) fn body_end(&mut self) -> (Vec<String>, u64) {
        if self.filled() || !self.blocks.is_empty() || self.part == 0 {
            self.cut();
        }
        self.ended = true;
        let parts = self.part;
        let (blocks, notes) = if let Some(kept) = self.kept.take() {
            kept
        } else {
            let (part, blocks, notes) = self.last.take().unwrap_or_default();
            self.wanted = part;
            (blocks, notes)
        };
        self.blocks = blocks;
        self.last = None;
        (notes, parts)
    }

    /// What follows is note `ordinal` of the part (from 0), reached by
    /// `target`; the first is set under a rule.
    pub(crate) fn note(&mut self, ordinal: u32, target: &str) {
        if self.filled() {
            self.finish();
        }
        if ordinal == 0 {
            self.rule();
        }
        self.note = Some(ordinal);
        self.anchors.push(target.to_owned());
    }

    /// The blocks kept, within `limits::BLOCKS`, and whether some were left
    /// out.
    pub(crate) fn into_blocks(mut self) -> (Vec<Block>, bool) {
        if self.filled() {
            self.finish();
        }
        let truncated = self.blocks.len() > limits::BLOCKS;
        self.blocks.truncate(limits::BLOCKS);
        (self.blocks, truncated)
    }

    /// The part kept: the one asked for, or the last when it asked past
    /// the end.
    pub(crate) fn kept_part(&self) -> u64 {
        self.wanted
    }
}

/// A document read: the part asked for, and what the document says about
/// itself.
pub(crate) struct Read {
    pub blocks: Vec<Block>,
    /// Some blocks of the part were left out (`limits::BLOCKS`).
    pub truncated: bool,
    /// The part shown (from 0), of how many.
    pub part: u64,
    pub parts: u64,
    pub words: usize,
    /// Facts for Details, as the document states them.
    pub details: Vec<(&'static str, String)>,
}

/// Words of running text: what Details counts.
fn words(blocks: &[Block]) -> usize {
    blocks
        .iter()
        .filter(|block| block.note.is_none())
        .map(|block| block.text.split_whitespace().count())
        .sum()
}

/// A paragraph style a word processor names for a quotation, in any of
/// the names Word, `LibreOffice` and Pandoc give it (lowercased).
pub(crate) fn is_quote(name: &str) -> bool {
    matches!(
        name,
        "quote" | "quotations" | "intense quote" | "block text" | "block quotation"
    )
}

/// A paragraph or character style named for code or preformatted text.
pub(crate) fn is_code(name: &str) -> bool {
    name.contains("code")
        || matches!(
            name,
            "html preformatted"
                | "plain text"
                | "macro text"
                | "verbatim char"
                | "html typewriter"
                | "preformatted text"
                | "source text"
                | "teletype"
                | "example"
        )
}

/// A symbol font's private-use bullet, as the character it draws: Word
/// writes its bullets in Symbol and Wingdings.
pub(crate) fn bullet(text: &str) -> String {
    text.chars()
        .map(|c| match u32::from(c) {
            0xF0A7 | 0xF06E => '▪',
            0xF0D8 | 0xF0E0 => '➢',
            0xF0FC => '✓',
            0xF076 => '❖',
            0xF0A8 => '□',
            // The bullets (0xF0B7 and others), and what no font here draws.
            0xE000..=0xF8FF => '•',
            _ => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_bullets_are_the_characters_they_draw() {
        assert_eq!(bullet("\u{f0b7}"), "•");
        assert_eq!(bullet("o"), "o");
    }

    fn paragraph(builder: &mut Builder, text: &str) {
        builder.paragraph(Shape::default());
        builder.text(text, &Look::default(), None);
        builder.end_paragraph();
        builder.boundary();
    }

    #[test]
    fn a_long_body_is_cut_into_parts_between_paragraphs() {
        let total = PART_BLOCKS * 2 + 10;
        for wanted in [0, 2, 9] {
            let mut builder = Builder::new(wanted);
            for i in 0..total {
                paragraph(&mut builder, &format!("p{i} word"));
            }
            let (_, parts) = builder.body_end();
            assert_eq!(parts, 3);
            assert_eq!(builder.words, total * 2);
            let kept = builder.kept_part();
            let (blocks, truncated) = builder.into_blocks();
            assert!(!truncated);
            let first = usize::try_from(kept).unwrap() * PART_BLOCKS;
            assert_eq!(blocks[0].text, format!("p{first} word"), "part {wanted}");
            assert_eq!(kept, wanted.min(2));
        }
    }

    #[test]
    fn rows_keep_their_columns_and_pictures_stand_after_them() {
        let mut builder = Builder::new(0);
        builder.begin_row(true);
        builder.cell();
        builder.paragraph(Shape::default());
        builder.text("a\tb", &Look::default(), None);
        builder.paragraph(Shape {
            marker: Some("1.".into()),
            list_depth: 1,
            ..Shape::default()
        });
        builder.text("item", &Look::default(), None);
        builder.picture("chart", None);
        builder.cell();
        builder.cell();
        builder.text("c", &Look::default(), None);
        builder.end_row();
        builder.body_end();
        let (blocks, _) = builder.into_blocks();
        assert_eq!(blocks[0].text, "a b\n1. item\t\tc");
        assert_eq!(
            blocks[0].kind,
            BlockKind::TableRow {
                cells: vec![0..11, 12..12, 13..14],
                header: true
            }
        );
        assert_eq!(blocks[1].text, "chart");
    }

    #[test]
    fn a_marker_is_drawn_beside_the_first_block_and_a_heading_says_its_number() {
        let mut builder = Builder::new(0);
        builder.paragraph(Shape {
            heading: 2,
            marker: Some("1.2".into()),
            list_depth: 2,
            ..Shape::default()
        });
        builder.text("Scope", &Look::default(), None);
        builder.end_paragraph();
        builder.paragraph(Shape {
            marker: Some("•".into()),
            list_depth: 1,
            ..Shape::default()
        });
        builder.text("before", &Look::default(), None);
        builder.picture(
            "",
            Some(Picture::Later {
                key: "word/media/a.png".into(),
                width: 2,
                height: 3,
            }),
        );
        builder.text("after", &Look::default(), None);
        builder.end_paragraph();
        builder.body_end();
        let (blocks, _) = builder.into_blocks();
        assert_eq!(blocks[0].text, "1.2 Scope");
        assert_eq!((blocks[0].heading, blocks[0].list_depth), (2, 0));
        assert_eq!(blocks[1].marker.as_deref(), Some("•"));
        assert!(blocks[2].picture.is_some());
        assert_eq!(blocks[2].list_depth, 1);
        assert_eq!(
            (blocks[3].text.as_str(), blocks[3].marker.as_deref()),
            ("after", None)
        );
    }
}
