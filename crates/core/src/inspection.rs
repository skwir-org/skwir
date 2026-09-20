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
    /// One bounded source line per block, with optional folding ranges.
    Code,
    Hex,
    Table,
    Tree,
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
}
#[derive(Clone, Debug, Default)]
pub struct Block {
    pub anchor: Option<String>,
    pub language: Option<String>,
    pub text: String,
    /// 0 paragraph; 1..=6 heading.
    pub heading: u8,
    pub code: bool,
    pub quote: bool,
    pub runs: Vec<Run>,
    pub image: Option<Image>,
}
#[derive(Clone, Debug)]
pub struct Run {
    pub range: std::ops::Range<usize>,
    pub color: Option<[u8; 3]>,
    pub bold: bool,
    pub italic: bool,
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
        if block.text.len() <= LINE_BYTES {
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
                anchor: if start == 0 {
                    block.anchor.take()
                } else {
                    None
                },
                language: block.language.clone(),
                heading: block.heading,
                code: block.code,
                quote: block.quote,
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
                italic: false,
                code: false,
                link: None,
                color: None,
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
}
