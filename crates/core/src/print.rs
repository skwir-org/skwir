//! Printing (ADR-111): which pages, the paper they go on, the order a
//! booklet folds in, and what the print helper (`skwir-print-worker`) is
//! handed to make a PDF of what a preview shows. Pure: the dialogs are
//! the platform's (`skwir_platform::print`), the commands the `print`
//! plugin's.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Points in a millimetre: PDF measures in points, 72 to the inch.
pub const POINTS_PER_MM: f32 = 72.0 / 25.4;

/// Which pages to print, as a dialog or a prompt chose them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Pages {
    #[default]
    All,
    /// The page the preview shows.
    Current,
    /// What the preview has selected: its lines, its rows.
    Selection,
    /// Pages typed, from 1 (`parse`).
    Ranges(Vec<Range>),
}

/// Which sides of what is printed: every page, or every other (the first,
/// third…: `Odd`), so a printer that prints one side prints both in two
/// runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PageSet {
    #[default]
    All,
    Even,
    Odd,
}

impl PageSet {
    /// Of `pages`, in order, those this keeps: by their place (the first
    /// is odd), not their number.
    pub fn keep<T: Copy>(self, pages: &[T]) -> Vec<T> {
        pages
            .iter()
            .enumerate()
            .filter(|(at, _)| match self {
                PageSet::All => true,
                PageSet::Odd => at % 2 == 0,
                PageSet::Even => at % 2 == 1,
            })
            .map(|(_, page)| *page)
            .collect()
    }
}

/// Pages `first` to `last`, from 1; to the end without `last`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Range {
    pub first: u32,
    pub last: Option<u32>,
}

/// The pages of a document picked, from 0, in the document's order, and
/// whether some asked were past its end.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Picked {
    pub pages: Vec<u32>,
    pub clipped: bool,
}

/// Pages as people type them: `1-3, 7, 10-`, `-5`, commas or spaces
/// between. A range backwards, a page 0 or a word is refused, saying
/// which.
pub fn parse(text: &str) -> Result<Vec<Range>, String> {
    let mut ranges = Vec::new();
    for part in text
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|part| !part.is_empty())
    {
        let number = |text: &str| -> Result<Option<u32>, String> {
            let text = text.trim();
            if text.is_empty() {
                return Ok(None);
            }
            match text.parse::<u32>() {
                Ok(0) => Err("pages are counted from 1".to_owned()),
                Ok(page) => Ok(Some(page)),
                Err(_) => Err(format!("“{text}” is not a page")),
            }
        };
        let range = if let Some((first, last)) = part.split_once(['-', '–']) {
            Range {
                first: number(first)?.unwrap_or(1),
                last: number(last)?,
            }
        } else {
            let page = number(part)?.unwrap_or(1);
            Range {
                first: page,
                last: Some(page),
            }
        };
        if range.last.is_some_and(|last| last < range.first) {
            return Err(format!("{part} goes backwards"));
        }
        ranges.push(range);
    }
    if ranges.is_empty() {
        return Err("no page named".to_owned());
    }
    Ok(ranges)
}

/// Ranges as the print portal writes them: from 0 (`0-2,4,9-11`).
pub fn from_zero_based(text: &str) -> Result<Vec<Range>, String> {
    let mut ranges = parse(&shift(text, 1)?)?;
    ranges.retain(|range| range.first > 0);
    Ok(ranges)
}

/// `ranges` written as the print portal reads them, from 0 (a page 0,
/// which no range read has, as the first).
pub fn to_zero_based(ranges: &[Range]) -> String {
    ranges
        .iter()
        .map(|range| {
            let first = range.first.saturating_sub(1);
            match range.last {
                Some(last) if last == range.first => format!("{first}"),
                Some(last) => format!("{first}-{}", last.saturating_sub(1)),
                None => format!("{first}-"),
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// Every number of `text` moved by `by`: one that would pass the largest
/// page number is refused, as one past it typed is.
fn shift(text: &str, by: u32) -> Result<String, String> {
    let mut out = String::new();
    let mut digits = String::new();
    let flush = |digits: &mut String, out: &mut String| -> Result<(), String> {
        if !digits.is_empty() {
            let n = digits
                .parse::<u32>()
                .ok()
                .and_then(|n| n.checked_add(by))
                .ok_or_else(|| format!("“{digits}” is not a page"))?;
            out.push_str(&n.to_string());
            digits.clear();
        }
        Ok(())
    };
    for c in text.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
        } else {
            flush(&mut digits, &mut out)?;
            out.push(c);
        }
    }
    flush(&mut digits, &mut out)?;
    Ok(out)
}

impl Pages {
    /// The pages of a document of `count` this picks, `current` the page
    /// shown (from 0). Selection picks every page: what is selected is
    /// made into its own document before pages are picked.
    pub fn pick(&self, count: u32, current: Option<u32>) -> Picked {
        match self {
            Pages::All | Pages::Selection => Picked {
                pages: (0..count).collect(),
                clipped: false,
            },
            Pages::Current => match current.filter(|&page| page < count) {
                Some(page) => Picked {
                    pages: vec![page],
                    clipped: false,
                },
                None => Picked {
                    pages: Vec::new(),
                    clipped: true,
                },
            },
            Pages::Ranges(ranges) => {
                let mut wanted = vec![false; count as usize];
                let mut clipped = false;
                for range in ranges {
                    let last = range.last.unwrap_or(count);
                    if range.first > count || last > count {
                        clipped = true;
                    }
                    for page in range.first.max(1)..=last.min(count) {
                        if let Some(slot) = wanted.get_mut(page as usize - 1) {
                            *slot = true;
                        }
                    }
                }
                Picked {
                    pages: (0..count).filter(|&page| wanted[page as usize]).collect(),
                    clipped,
                }
            }
        }
    }
}

/// The sheet a page is printed on, and its margins, in points.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Paper {
    pub width: f32,
    pub height: f32,
    pub margins: Margins,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Margins {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl Paper {
    /// A sheet of `width` by `height` millimetres, 15 mm all round.
    pub fn mm(width: f32, height: f32) -> Paper {
        let margin = 15.0 * POINTS_PER_MM;
        Paper {
            width: width * POINTS_PER_MM,
            height: height * POINTS_PER_MM,
            margins: Margins {
                top: margin,
                right: margin,
                bottom: margin,
                left: margin,
            },
        }
    }

    pub const fn a4() -> Paper {
        // 210 × 297 mm, 15 mm margins: written out, as `mm` is not const.
        Paper {
            width: 595.276,
            height: 841.89,
            margins: Margins {
                top: 42.52,
                right: 42.52,
                bottom: 42.52,
                left: 42.52,
            },
        }
    }

    pub const fn letter() -> Paper {
        Paper {
            width: 612.0,
            height: 792.0,
            margins: Margins {
                top: 42.52,
                right: 42.52,
                bottom: 42.52,
                left: 42.52,
            },
        }
    }

    /// The same sheet turned on its side.
    #[must_use]
    pub fn landscape(self) -> Paper {
        if self.width >= self.height {
            return self;
        }
        Paper {
            width: self.height,
            height: self.width,
            margins: Margins {
                top: self.margins.left,
                right: self.margins.top,
                bottom: self.margins.right,
                left: self.margins.bottom,
            },
        }
    }

    /// Half of this sheet turned landscape: a booklet's page, its margins
    /// kept.
    #[must_use]
    pub fn half(self) -> Paper {
        let sheet = self.landscape();
        Paper {
            width: sheet.width / 2.0,
            height: sheet.height,
            margins: self.margins,
        }
    }
}

/// How a sheet's back is printed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Duplex {
    /// One side.
    #[default]
    Simplex,
    /// Both, turned over its long edge (a portrait book's way).
    LongEdge,
    /// Both, turned over its short edge (a landscape sheet's, a booklet's).
    ShortEdge,
}

/// The order a booklet folds in.
pub mod booklet {
    use super::{Deserialize, JsonSchema, Serialize};

    /// One sheet, folded in the middle: its front's left and right pages,
    /// its back's left and right, each a page of the document (from 0) or
    /// a blank (`None`).
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
    pub struct Sheet {
        pub front: [Option<u32>; 2],
        pub back: [Option<u32>; 2],
    }

    /// The sheets of `pages` pages (from 0, in the order they read) folded
    /// as a booklet, in signatures of `signature` pages (rounded up to a
    /// multiple of 4; all of them in one without), blanks at the end. Laid
    /// in this order, folded, each signature put after the one before,
    /// they read in order.
    pub fn sheets(pages: &[u32], signature: Option<u32>) -> Vec<Sheet> {
        let total = pages.len().div_ceil(4) * 4;
        let size = signature
            .map_or(total, |size| (size.max(4) as usize).div_ceil(4) * 4)
            .max(4);
        let page = |at: usize| pages.get(at).copied();
        let mut sheets = Vec::new();
        let mut start = 0;
        while start < total {
            let n = size.min(total - start);
            for i in 0..n / 4 {
                sheets.push(Sheet {
                    front: [page(start + n - 1 - 2 * i), page(start + 2 * i)],
                    back: [page(start + 2 * i + 1), page(start + n - 2 - 2 * i)],
                });
            }
            start += n;
        }
        sheets
    }
}

/// The most lines of code a job prints: a file longer is cut there, and
/// the caller says so. A million colored lines are about 90 seconds of
/// the helper's work and a 56 MB PDF of 15,625 pages.
pub const MAX_CODE_LINES: usize = 1_000_000;

// What a job leaves out when it is the default, so a long document or a
// file of colored code writes only what differs (a run is its range and
// its color), and a job written either way reads the same. serde hands
// each a reference.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(value: &bool) -> bool {
    !*value
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero_u8(value: &u8) -> bool {
    *value == 0
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero_u32(value: &u32) -> bool {
    *value == 0
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero_i8(value: &i8) -> bool {
    *value == 0
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero_f32(value: &f32) -> bool {
    *value == 0.0
}

fn is_text(kind: &BlockKind) -> bool {
    matches!(kind, BlockKind::Text)
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_start(align: &Align) -> bool {
    *align == Align::Start
}

/// What the print helper makes a PDF of (`skwir-print-worker <job.json>`):
/// the content, laid out on `paper`, and written to `output`. Everything
/// it reads is named here (pictures written beside the job, fonts the
/// system's): it opens nothing else.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Job {
    /// The page laid out: a sheet, or half of one for a booklet.
    pub paper: Paper,
    /// Where the PDF is written.
    pub output: PathBuf,
    /// Said in the PDF, for a screen reader and a viewer's title bar.
    pub title: String,
    pub author: Option<String>,
    /// The text's language (`fr`, `en-GB`), if known.
    pub language: Option<String>,
    /// Above every page (a file's name); nothing without.
    pub header: Option<String>,
    /// "n / N" below every page.
    pub numbers: bool,
    /// The fonts by role: each a regular face, then bold, italic and bold
    /// italic where the family has them.
    pub fonts: Fonts,
    /// Folders a character no role font has may be found in.
    pub fallback: Vec<PathBuf>,
    /// The pages laid out to write (from 0), all without.
    pub pages: Option<Vec<u32>>,
    /// What it shows, each part from a new page.
    pub parts: Vec<Part>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Fonts {
    pub text: Family,
    pub code: Family,
}

/// A family's faces, as files.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Family {
    pub regular: Option<Face>,
    pub bold: Option<Face>,
    pub italic: Option<Face>,
    pub bold_italic: Option<Face>,
}

/// A face: a font file, and its index in a collection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Face {
    pub path: PathBuf,
    pub index: u32,
}

/// One file's content, as printed.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Part {
    /// Text laid out in the reading font: a document, a page, a message.
    Flow { blocks: Vec<Block> },
    /// Lines of code in the code font, numbered from `first` when
    /// `numbers`, a long line wrapped and its rows after the first marked.
    Code {
        lines: Vec<Line>,
        numbers: bool,
        first: u32,
    },
    /// Rows under their columns' names, repeated on each page.
    Table {
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    /// Pictures, each fitted to a page; several to a page, with their
    /// names, as a contact sheet.
    Pictures { pictures: Vec<Picture>, sheet: bool },
    /// An SVG drawing, kept vector, fitted to a page.
    Drawing { svg: PathBuf, alt: String },
    /// A document's pages drawn as pictures (a `DjVu`, a comic): each on a
    /// page of its own, fitted.
    Pages { pages: Vec<Picture> },
    /// A font shown: its name, then the sample at each size, in it.
    Specimen {
        face: Face,
        name: String,
        sample: String,
        sizes: Vec<f32>,
    },
}

/// A block of text, as a reading view shows it. A field at its default is
/// left out of a job, and read as its default where it is missing.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Block {
    #[serde(default, skip_serializing_if = "is_text")]
    pub kind: BlockKind,
    #[serde(default)]
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runs: Vec<Run>,
    /// 1 to 6 for a heading (in the PDF's outline), else 0.
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub heading: u8,
    #[serde(default, skip_serializing_if = "is_false")]
    pub quote: bool,
    /// How deep in lists, and the marker it starts with (`•`, `3.`).
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub list_depth: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub marker: Option<String>,
    /// Left, center, right, justified.
    #[serde(default, skip_serializing_if = "is_start")]
    pub align: Align,
    /// Ems in.
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub indent: f32,
    /// Size against the body's; 0 is the body's own.
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub size: f32,
    /// A footnote's number, for a note's own block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<u32>,
    /// What a link inside the text leads to by name (`#id`): where this
    /// block is, for one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub anchors: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockKind {
    #[default]
    Text,
    /// Code, monospaced, its runs colored.
    Code,
    Rule,
    /// A table's row: its cells' text ranges, a header's bold.
    Row {
        cells: Vec<(u32, u32)>,
        header: bool,
    },
    Picture(Picture),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
    Justify,
}

/// How part of a text is written: `start..end` of its bytes. A field at
/// its default is left out of a job, and read as its default where it is
/// missing: a run of colored code is its range and its color.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)] // A run's styles, each on or off alone.
pub struct Run {
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub start: u32,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub end: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<[u8; 3]>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub italic: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub underline: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub strike: bool,
    /// In the code font.
    #[serde(default, skip_serializing_if = "is_false")]
    pub code: bool,
    /// Raised (1) or lowered (-1).
    #[serde(default, skip_serializing_if = "is_zero_i8")]
    pub script: i8,
    #[serde(default, skip_serializing_if = "is_false")]
    pub small_caps: bool,
    /// An address (`https://`, `mailto:`) or a name in this text (`#id`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
}

/// A line of code and its colors.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Line {
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runs: Vec<Run>,
}

/// A picture skwir wrote beside the job (PNG or JPEG as it was), its size
/// in pixels, what it shows for a screen reader, and the name a contact
/// sheet writes under it.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Picture {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub alt: String,
    pub caption: Option<String>,
}

/// The helper's answer, a line on its standard output.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Made {
    /// Pages written.
    pub pages: u32,
    /// Pages laid out, those not written included.
    pub laid_out: u32,
}

#[cfg(test)]
mod tests {
    use super::booklet::{Sheet, sheets};
    use super::*;

    fn range(first: u32, last: Option<u32>) -> Range {
        Range { first, last }
    }

    #[test]
    fn pages_are_read_as_people_type_them() {
        assert_eq!(
            parse("1-3, 7 10-").unwrap(),
            [range(1, Some(3)), range(7, Some(7)), range(10, None)]
        );
        assert_eq!(parse("-5").unwrap(), [range(1, Some(5))]);
        assert_eq!(parse("2–4").unwrap(), [range(2, Some(4))]);
        assert!(parse("").is_err());
        assert!(parse("0").is_err());
        assert!(parse("5-3").is_err());
        assert!(parse("three").is_err());
    }

    #[test]
    fn pages_picked_keep_the_document_s_order_and_say_what_was_past_its_end() {
        let ranges = Pages::Ranges(parse("7, 1-3, 2, 9-").unwrap());
        assert_eq!(
            ranges.pick(10, None),
            Picked {
                pages: vec![0, 1, 2, 6, 8, 9],
                clipped: false
            }
        );
        let past = Pages::Ranges(parse("4-12").unwrap()).pick(5, None);
        assert_eq!(past.pages, [3, 4]);
        assert!(past.clipped);
        assert_eq!(Pages::Current.pick(5, Some(2)).pages, [2]);
        assert!(Pages::Current.pick(5, None).clipped);
        assert_eq!(Pages::All.pick(3, None).pages, [0, 1, 2]);
    }

    #[test]
    fn the_portal_s_ranges_count_from_zero() {
        let ranges = from_zero_based("0-2,4,9-").unwrap();
        assert_eq!(
            ranges,
            [range(1, Some(3)), range(5, Some(5)), range(10, None)]
        );
        assert_eq!(to_zero_based(&ranges), "0-2,4,9-");
        // The last page there is, moved to count from 1, is past any: a
        // range refused, never one that wraps to page 0.
        assert!(from_zero_based(&u32::MAX.to_string()).is_err());
        assert!(from_zero_based("0-4294967295").is_err());
        assert_eq!(
            from_zero_based(&(u32::MAX - 1).to_string()).unwrap(),
            [range(u32::MAX, Some(u32::MAX))]
        );
        assert_eq!(to_zero_based(&[range(0, Some(0))]), "0");
        // A range from 0, which no text reads as, picks from the first page.
        let zero = Pages::Ranges(vec![range(0, Some(2))]);
        assert_eq!(zero.pick(5, None).pages, [0, 1]);
    }

    #[test]
    fn a_booklet_of_eight_pages_folds_two_sheets() {
        let pages: Vec<u32> = (0..8).collect();
        assert_eq!(
            sheets(&pages, None),
            [
                Sheet {
                    front: [Some(7), Some(0)],
                    back: [Some(1), Some(6)]
                },
                Sheet {
                    front: [Some(5), Some(2)],
                    back: [Some(3), Some(4)]
                },
            ]
        );
        // Five pages: three blanks at the end.
        assert_eq!(
            sheets(&[0, 1, 2, 3, 4], None),
            [
                Sheet {
                    front: [None, Some(0)],
                    back: [Some(1), None]
                },
                Sheet {
                    front: [None, Some(2)],
                    back: [Some(3), Some(4)]
                },
            ]
        );
    }

    /// Every page is printed once, blanks only where the count was
    /// rounded up, and each signature folds on its own.
    #[test]
    fn every_page_lands_once_whatever_the_count_and_signature() {
        for count in 0..60u32 {
            for signature in [None, Some(4), Some(8), Some(10), Some(16)] {
                let pages: Vec<u32> = (100..100 + count).collect();
                let sheets = sheets(&pages, signature);
                let slots: Vec<Option<u32>> = sheets
                    .iter()
                    .flat_map(|sheet| sheet.front.into_iter().chain(sheet.back))
                    .collect();
                let mut printed: Vec<u32> = slots.iter().flatten().copied().collect();
                printed.sort_unstable();
                assert_eq!(printed, pages, "{count} {signature:?}");
                assert_eq!(slots.len(), (count as usize).div_ceil(4) * 4);
                // Within a signature, reading the folded sheets in order:
                // front right, back left, back right of the inner half…
                let size = signature
                    .map_or(slots.len(), |s| (s as usize).div_ceil(4) * 4)
                    .max(4);
                let mut read = Vec::new();
                for group in sheets.chunks(size / 4) {
                    let n = group.len();
                    let mut order = vec![None; n * 4];
                    for (i, sheet) in group.iter().enumerate() {
                        order[2 * i] = sheet.front[1];
                        order[2 * i + 1] = sheet.back[0];
                        order[n * 4 - 2 - 2 * i] = sheet.back[1];
                        order[n * 4 - 1 - 2 * i] = sheet.front[0];
                    }
                    read.extend(order);
                }
                let read: Vec<u32> = read.into_iter().flatten().collect();
                assert_eq!(read, pages, "{count} {signature:?}");
            }
        }
    }

    /// A run, a line and a block at their defaults write nothing of them,
    /// and a job written with every field, as before, reads the same.
    #[test]
    fn defaults_are_left_out_and_read_back_either_way() {
        let run = Run {
            start: 4,
            end: 9,
            color: Some([1, 2, 3]),
            ..Run::default()
        };
        assert_eq!(
            serde_json::to_string(&run).unwrap(),
            r#"{"start":4,"end":9,"color":[1,2,3]}"#
        );
        let line = Line {
            text: "fn main() {}".into(),
            runs: Vec::new(),
        };
        assert_eq!(
            serde_json::to_string(&line).unwrap(),
            r#"{"text":"fn main() {}"}"#
        );
        assert_eq!(
            serde_json::to_string(&Block::default()).unwrap(),
            r#"{"text":""}"#
        );
        let full = r#"{"start":0,"end":3,"color":null,"bold":true,"italic":false,
            "underline":false,"strike":false,"code":false,"script":-1,
            "small_caps":false,"link":"https://example.org"}"#;
        let read: Run = serde_json::from_str(full).unwrap();
        assert!(read.bold && read.script == -1 && read.end == 3);
        assert_eq!(read.link.as_deref(), Some("https://example.org"));
        let block: Block = serde_json::from_str(
            r#"{"kind":"rule","text":"","runs":[],"heading":0,"quote":true,
            "list_depth":2,"marker":null,"align":"center","indent":0.0,
            "size":1.5,"note":null,"anchors":["x"]}"#,
        )
        .unwrap();
        assert!(matches!(block.kind, BlockKind::Rule) && block.quote);
        assert_eq!(
            (block.list_depth, block.align, block.size),
            (2, Align::Center, 1.5)
        );
        let short: Block = serde_json::from_str(r#"{"heading":2}"#).unwrap();
        assert_eq!(short.heading, 2);
        assert!(short.text.is_empty() && matches!(short.kind, BlockKind::Text));
    }

    #[test]
    fn half_a_sheet_is_a_booklet_s_page() {
        let half = Paper::a4().half();
        assert!((half.width - 841.89 / 2.0).abs() < 0.01);
        assert!((half.height - 595.276).abs() < 0.01);
        assert_eq!(Paper::a4().landscape().landscape(), Paper::a4().landscape());
    }
}
