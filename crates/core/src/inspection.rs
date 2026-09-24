//! Bounded, toolkit-independent content previews. Providers execute on workers.
use crate::thumbs::Image;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct InspectRequest {
    /// Explicit provider, e.g. `preview-hex`; None chooses by type.
    pub provider: Option<String>,
    pub source: bool,
    pub offset: u64,
    /// Container member or chapter, never a filesystem output path.
    pub target: Option<String>,
    pub query: Option<String>,
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
    pub source_available: bool,
    pub notice: String,
    /// What the format knows about the file, for the panel's Details: a
    /// book's author, a font's family, an archive's unpacked size. Facts read
    /// from the content, in the order they should be shown, bounded like the
    /// rest: a provider already parsed the file to build this inspection, so
    /// they cost nothing more.
    pub details: Vec<Detail>,
}
/// One fact of [`Inspection::details`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Detail {
    pub label: String,
    pub value: String,
}
impl Detail {
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Detail {
            label: label.into(),
            value: value.into(),
        }
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
    pub image: Option<Image>,
}
#[derive(Clone, Debug, Default)]
#[allow(clippy::struct_excessive_bools)]
pub struct Run {
    pub range: std::ops::Range<usize>,
    pub color: Option<[u8; 3]>,
    pub bold: bool,
    pub italic: bool,
    pub strikethrough: bool,
    pub underline: bool,
    pub code: bool,
    pub link: Option<String>,
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
        while start < block.text.len() {
            let mut end = (start + LINE_BYTES).min(block.text.len());
            while !block.text.is_char_boundary(end) {
                end -= 1;
            }
            if end < block.text.len()
                && let Some(newline) = block.text[start..end].rfind('\n')
                && newline > 0
            {
                end = start + newline + 1;
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
                image: if start == 0 { block.image.take() } else { None },
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
                align: block.align,
                indent: block.indent,
                size: block.size,
                // Only text can exceed the limit, so the kind carries over.
                kind: block.kind.clone(),
                language: block.language.clone(),
                heading: block.heading,
                code: block.code,
                quote: block.quote,
                list_depth: block.list_depth,
            });
            start = end;
        }
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
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
