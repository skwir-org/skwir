//! The arithmetic of a page, apart from what is drawn on it: where the
//! text goes on a sheet, how a picture fits a box, how many rows a page
//! holds, how columns share a width. Pure, so each decision is tested
//! alone.

use skwir_core::print::Paper;

/// Room for the name above the text, or the page's number below it.
pub const BAND: f32 = 16.0;
/// The size the name and the page's number are written at.
pub const BAND_SIZE: f32 = 7.5;

/// A rectangle, `y` downwards from the page's top, in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Area {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Area {
    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }
}

/// Where things go on every page of a job: the text, the name above it
/// and the number below, as baselines.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub width: f32,
    pub height: f32,
    pub text: Area,
    /// The baseline of the name, when there is one.
    pub header: Option<f32>,
    /// The baseline of "n / N", when pages are numbered.
    pub footer: Option<f32>,
}

/// The least room the text keeps, whatever the margins say: a sheet with
/// margins wider than itself still prints something.
const LEAST: f32 = 72.0;

impl Frame {
    /// The frame of `paper`. The name and the number sit in the margins
    /// when those have room for them, else they take it from the text.
    pub fn new(paper: &Paper, header: bool, numbers: bool) -> Frame {
        let width = paper.width.max(LEAST);
        let height = paper.height.max(LEAST);
        let margin = |value: f32| {
            if value.is_finite() {
                value.max(0.0)
            } else {
                0.0
            }
        };
        let (mut left, mut right) = (margin(paper.margins.left), margin(paper.margins.right));
        let (mut top, mut bottom) = (margin(paper.margins.top), margin(paper.margins.bottom));
        if width - left - right < LEAST {
            let scale = (width - LEAST).max(0.0) / (left + right).max(1.0);
            left *= scale;
            right *= scale;
        }
        if height - top - bottom < LEAST {
            let scale = (height - LEAST).max(0.0) / (top + bottom).max(1.0);
            top *= scale;
            bottom *= scale;
        }
        let mut text = Area {
            x: left,
            y: top,
            width: width - left - right,
            height: height - top - bottom,
        };
        let header = header.then(|| {
            if top >= BAND + 4.0 {
                top - BAND * 0.5
            } else {
                let baseline = text.y + BAND_SIZE;
                text.y += BAND;
                text.height -= BAND;
                baseline
            }
        });
        let footer = numbers.then(|| {
            if bottom >= BAND + 4.0 {
                height - bottom + BAND
            } else {
                text.height -= BAND;
                text.bottom() + BAND * 0.75
            }
        });
        text.height = text.height.max(BAND);
        Frame {
            width,
            height,
            text,
            header,
            footer,
        }
    }
}

/// `width` by `height` scaled to fit in `area`, never larger than `most`
/// times its own size, centred: the rectangle it is drawn in.
pub fn fit(width: f32, height: f32, area: Area, most: f32) -> Area {
    if width <= 0.0 || height <= 0.0 || !width.is_finite() || !height.is_finite() {
        return Area {
            x: area.x,
            y: area.y,
            width: 0.0,
            height: 0.0,
        };
    }
    let scale = (area.width / width).min(area.height / height).min(most);
    let (w, h) = (width * scale, height * scale);
    Area {
        x: area.x + (area.width - w) / 2.0,
        y: area.y + (area.height - h) / 2.0,
        width: w,
        height: h,
    }
}

/// A contact sheet's grid on a page's text area: how many pictures across
/// and down, and the box each has, its name under it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    pub columns: usize,
    pub rows: usize,
    /// A cell's width and height, its name included.
    pub cell_width: f32,
    pub cell_height: f32,
    /// The height left for the picture in a cell.
    pub picture: f32,
    pub gap: f32,
}

/// The space under a picture of a contact sheet for its name.
pub const CAPTION: f32 = 12.0;

impl Grid {
    /// Three across on a page taller than wide, four on one wider; as many
    /// rows of square pictures as fit, one at least.
    pub fn new(text: Area) -> Grid {
        let columns = if text.width > text.height { 4 } else { 3 };
        let gap = 8.0;
        let cell_width = (text.width - gap * (columns as f32 - 1.0)) / columns as f32;
        let cell_height = cell_width + CAPTION;
        let rows = (((text.height + gap) / (cell_height + gap)).floor() as usize).max(1);
        // A page that holds no full row shrinks the picture to the row.
        let cell_height = cell_height.min(text.height);
        Grid {
            columns,
            rows,
            cell_width,
            cell_height,
            picture: (cell_height - CAPTION).max(1.0),
            gap,
        }
    }

    pub fn per_page(&self) -> usize {
        self.columns * self.rows
    }

    /// The cell of the `index`th picture of a page.
    pub fn cell(&self, text: Area, index: usize) -> Area {
        let (row, column) = (index / self.columns, index % self.columns);
        Area {
            x: text.x + column as f32 * (self.cell_width + self.gap),
            y: text.y + row as f32 * (self.cell_height + self.gap),
            width: self.cell_width,
            height: self.cell_height,
        }
    }
}

/// Where each page of rows starts: units (a line, or a piece of a long
/// one) of `rows` rows each, named by their keys, `per_page` rows to a
/// page. A page starts at a unit's key and the row of it, so a unit
/// longer than a page goes on over the next. One page at least, at the
/// default key when there are no units.
pub fn units_pages<K: Copy + Default>(
    units: impl IntoIterator<Item = (K, u32)>,
    per_page: u32,
) -> Vec<(K, u32)> {
    let per_page = per_page.max(1);
    let mut starts = Vec::new();
    let mut used = per_page;
    for (key, rows) in units {
        let mut row = 0;
        let mut left = rows.max(1);
        while left > 0 {
            if used == per_page {
                starts.push((key, row));
                used = 0;
            }
            let take = left.min(per_page - used);
            used += take;
            row += take;
            left -= take;
        }
    }
    if starts.is_empty() {
        starts.push((K::default(), 0));
    }
    starts
}

/// Rows of `heights` stacked on pages of `room`, the first of each page
/// lower by `head` (a header repeated): the rows of each page, one at
/// least, however tall.
pub fn stack(heights: &[f32], room: f32, head: f32) -> Vec<std::ops::Range<usize>> {
    let mut pages = Vec::new();
    let mut start = 0;
    let mut used = head;
    for (index, &height) in heights.iter().enumerate() {
        if index > start && used + height > room {
            pages.push(start..index);
            start = index;
            used = head;
        }
        used += height;
    }
    pages.push(start..heights.len());
    pages
}

/// How a table's columns share a page's width.
#[derive(Clone, Debug, PartialEq)]
pub struct Columns {
    /// The size the cells are written at.
    pub size: f32,
    /// The columns of each page across, each with its width: one group
    /// when they fit, else the first column and those that follow it.
    pub groups: Vec<Vec<(usize, f32)>>,
}

/// The widths of columns whose content wants `natural` (at `size`, each
/// with `padding` that does not shrink), on a width of `width`: as they
/// are when they fit; else shrunk with the text, down to `least` points;
/// else the widest wrapped, each column keeping `narrowest` at least;
/// else, when even that does not fit and `split` allows, the columns
/// spread over pages across, the first one on each.
#[allow(clippy::too_many_arguments)] // The columns and each bound on them.
pub fn columns(
    natural: &[f32],
    width: f32,
    size: f32,
    least: f32,
    narrowest: f32,
    padding: f32,
    split: bool,
) -> Columns {
    let all: Vec<usize> = (0..natural.len()).collect();
    let total: f32 = natural.iter().sum();
    if natural.is_empty() || total <= width {
        return Columns {
            size,
            groups: vec![all.iter().map(|&c| (c, natural[c])).collect()],
        };
    }
    let fixed = padding * natural.len() as f32;
    let text = (total - fixed).max(1.0);
    let shrunk = (size * (width - fixed).max(0.0) / text).max(least.min(size));
    let scale = shrunk / size;
    let natural: Vec<f32> = natural
        .iter()
        .map(|w| (w - padding).max(0.0) * scale + padding)
        .collect();
    let total: f32 = natural.iter().sum();
    if total <= width + 0.01 {
        return Columns {
            size: shrunk,
            groups: vec![all.iter().map(|&c| (c, natural[c])).collect()],
        };
    }
    if !split || natural.len() as f32 * narrowest <= width || natural.len() < 3 {
        return Columns {
            size: shrunk,
            groups: vec![share(&natural, &all, width, narrowest)],
        };
    }
    // Pages across: the first column on each, then as many of the rest
    // as fit beside it at their narrowest or their natural width.
    let first = natural[0].min(width / 3.0).max(narrowest.min(width / 3.0));
    let mut groups = Vec::new();
    let mut group = vec![0];
    let mut used = first;
    for column in 1..natural.len() {
        let wants = natural[column].min(width - first).max(narrowest);
        if group.len() > 1 && used + wants > width {
            groups.push(share(&natural, &group, width, narrowest));
            group = vec![0];
            used = first;
        }
        group.push(column);
        used += wants;
    }
    groups.push(share(&natural, &group, width, narrowest));
    Columns {
        size: shrunk,
        groups,
    }
}

/// `width` shared among `columns` wanting `natural`: each as much as it
/// wants up to a common cap, the cap set so they fill the width, none
/// under `narrowest` unless the width cannot hold that.
fn share(natural: &[f32], columns: &[usize], width: f32, narrowest: f32) -> Vec<(usize, f32)> {
    let wants: Vec<f32> = columns.iter().map(|&c| natural[c]).collect();
    let total: f32 = wants.iter().sum();
    if total <= width {
        return columns.iter().zip(wants).map(|(&c, w)| (c, w)).collect();
    }
    let floor = narrowest.min(width / columns.len() as f32);
    // The cap: raise it from the floor until the capped widths fill the
    // width, the columns sorted by what they want.
    let mut sorted = wants.clone();
    sorted.sort_by(f32::total_cmp);
    let mut cap = floor;
    let mut below = 0.0;
    for (index, want) in sorted.iter().enumerate() {
        let want = want.max(floor);
        let above = (sorted.len() - index) as f32;
        if below + want * above >= width {
            cap = ((width - below) / above).max(floor);
            break;
        }
        below += want;
        cap = want;
    }
    columns
        .iter()
        .zip(wants)
        .map(|(&c, w)| (c, w.max(floor).min(cap)))
        .collect()
}

/// Which laid-out pages are written: those asked (from 0), in order, once
/// each, past the end left out; all without.
pub fn written(laid_out: u32, asked: Option<&[u32]>) -> Vec<u32> {
    match asked {
        None => (0..laid_out).collect(),
        Some(asked) => {
            let mut wanted = vec![false; laid_out as usize];
            for &page in asked {
                if let Some(slot) = wanted.get_mut(page as usize) {
                    *slot = true;
                }
            }
            (0..laid_out).filter(|&p| wanted[p as usize]).collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pages of lines of `rows` rows each, keyed by their index.
    fn row_pages(rows: impl IntoIterator<Item = u32>, per_page: u32) -> Vec<(u32, u32)> {
        units_pages(
            rows.into_iter()
                .enumerate()
                .map(|(line, rows)| (line as u32, rows)),
            per_page,
        )
    }

    #[test]
    fn a4_keeps_its_margins_and_the_bands_sit_in_them() {
        let frame = Frame::new(&Paper::a4(), true, true);
        assert!((frame.text.x - 42.52).abs() < 0.01);
        assert!((frame.text.width - (595.276 - 2.0 * 42.52)).abs() < 0.01);
        assert!((frame.text.height - (841.89 - 2.0 * 42.52)).abs() < 0.01);
        let header = frame.header.unwrap();
        assert!(header < frame.text.y, "the name above the text");
        let footer = frame.footer.unwrap();
        assert!(footer > frame.text.bottom() && footer < frame.height);
    }

    #[test]
    fn narrow_margins_give_the_bands_room_from_the_text() {
        let mut paper = Paper::a4();
        paper.margins.top = 4.0;
        paper.margins.bottom = 4.0;
        let bare = Frame::new(&paper, false, false);
        let framed = Frame::new(&paper, true, true);
        assert!((bare.text.height - framed.text.height - 2.0 * BAND).abs() < 0.01);
        assert!(framed.header.unwrap() > 4.0 && framed.header.unwrap() < framed.text.y);
        assert!(framed.footer.unwrap() > framed.text.bottom());
        assert!(framed.footer.unwrap() <= framed.height);
    }

    #[test]
    fn a_booklet_s_half_sheet_is_a_narrow_page_with_the_same_margins() {
        let half = Paper::a4().half();
        let frame = Frame::new(&half, true, true);
        // A5 by another name: 420.9 wide, its 15 mm margins kept.
        assert!((frame.width - 420.945).abs() < 0.01);
        assert!((frame.text.width - (420.945 - 2.0 * 42.52)).abs() < 0.01);
        assert!((frame.text.height - (595.276 - 2.0 * 42.52)).abs() < 0.01);
        // Fewer columns of a contact sheet do not follow: it stays taller
        // than wide, three across.
        assert_eq!(Grid::new(frame.text).columns, 3);
        // Code at 9 points, 11.7 to a row: 43 rows a page where A4 holds
        // 64.
        let rows = (frame.text.height / (9.0 * 1.3)).floor() as u32;
        assert_eq!(rows, 43);
        assert_eq!(row_pages(std::iter::repeat_n(1, 100), rows).len(), 3);
    }

    #[test]
    fn margins_wider_than_the_sheet_still_leave_room() {
        let mut paper = Paper::mm(50.0, 50.0);
        paper.margins.left = 200.0;
        paper.margins.right = 200.0;
        let frame = Frame::new(&paper, false, false);
        assert!(frame.text.width >= LEAST - 0.01);
        assert!(frame.text.x >= 0.0 && frame.text.x + frame.text.width <= frame.width + 0.01);
    }

    #[test]
    fn a_picture_fits_centred_never_enlarged_past_what_is_asked() {
        let area = Area {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 200.0,
        };
        let wide = fit(400.0, 100.0, area, f32::INFINITY);
        assert_eq!((wide.width, wide.height), (100.0, 25.0));
        assert_eq!((wide.x, wide.y), (10.0, 20.0 + 87.5));
        let small = fit(10.0, 10.0, area, 1.0);
        assert_eq!((small.width, small.height), (10.0, 10.0));
        assert_eq!(fit(0.0, 10.0, area, 1.0).width, 0.0);
    }

    #[test]
    fn code_pages_count_rows_and_a_long_line_goes_on_over_pages() {
        assert_eq!(row_pages(std::iter::repeat_n(1, 1000), 60).len(), 17);
        assert_eq!(row_pages(std::iter::repeat_n(1, 60), 60), [(0, 0)]);
        assert_eq!(row_pages(std::iter::repeat_n(1, 61), 60), [(0, 0), (60, 0)]);
        assert_eq!(row_pages(Vec::new(), 60), [(0, 0)]);
        // A line of 130 rows among short ones: its first 59 rows end the
        // first page, 60 the second, the last 11 and the next line the
        // third.
        let starts = row_pages([1, 130, 1], 60);
        assert_eq!(starts, [(0, 0), (1, 59), (1, 119)]);
    }

    #[test]
    fn a_contact_sheet_is_three_across_upright_four_on_its_side() {
        let a4 = Frame::new(&Paper::a4(), true, true);
        let grid = Grid::new(a4.text);
        assert_eq!(grid.columns, 3);
        assert!(grid.rows >= 3);
        let last = grid.cell(a4.text, grid.per_page() - 1);
        assert!(last.bottom() <= a4.text.bottom() + 0.01);
        assert!(last.x + last.width <= a4.text.x + a4.text.width + 0.01);
        let landscape = Frame::new(&Paper::a4().landscape(), true, true);
        assert_eq!(Grid::new(landscape.text).columns, 4);
    }

    #[test]
    fn columns_that_fit_keep_their_width() {
        let fitted = columns(&[100.0, 50.0], 300.0, 9.0, 6.0, 30.0, 0.0, true);
        assert_eq!(fitted.size, 9.0);
        assert_eq!(fitted.groups, [vec![(0, 100.0), (1, 50.0)]]);
    }

    #[test]
    fn columns_a_little_too_wide_shrink_with_their_text() {
        let fitted = columns(&[300.0, 300.0], 500.0, 9.0, 6.0, 30.0, 0.0, true);
        assert!((fitted.size - 7.5).abs() < 0.01);
        assert_eq!(fitted.groups.len(), 1);
        let total: f32 = fitted.groups[0].iter().map(|(_, w)| w).sum();
        assert!((total - 500.0).abs() < 0.1);
    }

    #[test]
    fn padding_stays_as_the_text_shrinks() {
        // Two columns of 300, 10 of it padding each, on 500: their text
        // shrinks from 580 to 480, the padding stays.
        let fitted = columns(&[300.0, 300.0], 500.0, 9.0, 6.0, 30.0, 10.0, true);
        assert!((fitted.size - 9.0 * 480.0 / 580.0).abs() < 0.01);
        let widths: Vec<f32> = fitted.groups[0].iter().map(|(_, w)| *w).collect();
        assert!((widths[0] - 250.0).abs() < 0.01, "{widths:?}");
    }

    #[test]
    fn columns_far_too_wide_wrap_the_widest_at_the_least_size() {
        let fitted = columns(&[90.0, 900.0, 60.0], 500.0, 9.0, 6.0, 30.0, 0.0, true);
        assert_eq!(fitted.size, 6.0);
        let group = &fitted.groups[0];
        assert_eq!(group.len(), 3);
        // The narrow ones as they are at 6 points, the wide one the rest.
        assert!((group[0].1 - 60.0).abs() < 0.01);
        assert!((group[2].1 - 40.0).abs() < 0.01);
        let total: f32 = group.iter().map(|(_, w)| w).sum();
        assert!((total - 500.0).abs() < 0.1);
    }

    #[test]
    fn too_many_columns_go_across_pages_the_first_on_each() {
        let natural = vec![120.0; 30];
        let fitted = columns(&natural, 500.0, 9.0, 6.0, 40.0, 0.0, true);
        assert!(fitted.groups.len() > 1);
        let mut seen = Vec::new();
        for group in &fitted.groups {
            assert_eq!(group[0].0, 0, "the first column on every page");
            let total: f32 = group.iter().map(|(_, w)| w).sum();
            assert!(total <= 500.1, "{total}");
            seen.extend(group.iter().skip(1).map(|(c, _)| *c));
        }
        assert_eq!(seen, (1..30).collect::<Vec<_>>());
        // Not split, they share the width, each at its narrowest at least.
        let shared = columns(&natural, 500.0, 9.0, 6.0, 40.0, 0.0, false);
        assert_eq!(shared.groups.len(), 1);
    }

    #[test]
    fn rows_stack_on_pages_under_their_header() {
        assert_eq!(stack(&[10.0; 10], 50.0, 10.0), [0..4, 4..8, 8..10]);
        assert_eq!(stack(&[], 50.0, 10.0), vec![0..0]);
        // A row taller than a page has one to itself.
        assert_eq!(stack(&[10.0, 80.0, 10.0], 50.0, 0.0), [0..1, 1..2, 2..3]);
    }

    #[test]
    fn pages_written_are_those_asked_in_order_past_the_end_left_out() {
        assert_eq!(written(4, None), [0, 1, 2, 3]);
        assert_eq!(written(4, Some(&[3, 1, 1, 9])), [1, 3]);
        assert!(written(0, Some(&[0])).is_empty());
    }
}
