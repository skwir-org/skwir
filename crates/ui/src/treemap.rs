//! Squarified treemap layout (Bruls, Huizing, van Wijk 2000).

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Rect { x, y, w, h }
    }

    pub fn area(&self) -> f64 {
        self.w * self.h
    }
}

fn worst(row: &[f64], side: f64) -> f64 {
    let sum: f64 = row.iter().sum();
    if sum <= 0.0 || side <= 0.0 {
        return f64::INFINITY;
    }
    let max = row.iter().copied().fold(0.0, f64::max);
    let min = row.iter().copied().fold(f64::INFINITY, f64::min);
    let s2 = sum * sum;
    let side2 = side * side;
    (side2 * max / s2).max(s2 / (side2 * min))
}

/// Lay out one row along the shorter side of `free`, returning the row's
/// rectangles and the remaining free rectangle.
fn layout_row(row: &[f64], free: Rect) -> (Vec<Rect>, Rect) {
    let sum: f64 = row.iter().sum();
    let mut out = Vec::with_capacity(row.len());
    if free.w >= free.h {
        // Row is a vertical strip on the left.
        let strip_w = if free.h > 0.0 { sum / free.h } else { 0.0 };
        let mut y = free.y;
        for a in row {
            let h = if sum > 0.0 { a / sum * free.h } else { 0.0 };
            out.push(Rect::new(free.x, y, strip_w, h));
            y += h;
        }
        (
            out,
            Rect::new(free.x + strip_w, free.y, free.w - strip_w, free.h),
        )
    } else {
        let strip_h = if free.w > 0.0 { sum / free.w } else { 0.0 };
        let mut x = free.x;
        for a in row {
            let w = if sum > 0.0 { a / sum * free.w } else { 0.0 };
            out.push(Rect::new(x, free.y, w, strip_h));
            x += w;
        }
        (
            out,
            Rect::new(free.x, free.y + strip_h, free.w, free.h - strip_h),
        )
    }
}

/// Rectangles for `weights` (any non-negative numbers, in order) filling
/// `bounds`. Zero-weight items get empty rectangles.
pub fn squarify(weights: &[f64], bounds: Rect) -> Vec<Rect> {
    let total: f64 = weights.iter().filter(|w| **w > 0.0).sum();
    let mut out = vec![Rect::default(); weights.len()];
    if total <= 0.0 || bounds.area() <= 0.0 {
        return out;
    }
    let scale = bounds.area() / total;
    let mut items: Vec<(usize, f64)> = weights
        .iter()
        .enumerate()
        .filter(|(_, w)| **w > 0.0)
        .map(|(i, w)| (i, w * scale))
        .collect();
    // Descending areas give the best aspect ratios.
    items.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let mut free = bounds;
    let mut row: Vec<(usize, f64)> = Vec::new();
    for item in items {
        let side = free.w.min(free.h);
        let mut candidate: Vec<f64> = row.iter().map(|(_, a)| *a).collect();
        candidate.push(item.1);
        let current: Vec<f64> = row.iter().map(|(_, a)| *a).collect();
        if row.is_empty() || worst(&candidate, side) <= worst(&current, side) {
            row.push(item);
        } else {
            let (rects, rest) = layout_row(&current, free);
            for ((i, _), r) in row.iter().zip(rects) {
                out[*i] = r;
            }
            free = rest;
            row = vec![item];
        }
    }
    if !row.is_empty() {
        let areas: Vec<f64> = row.iter().map(|(_, a)| *a).collect();
        let (rects, _) = layout_row(&areas, free);
        for ((i, _), r) in row.iter().zip(rects) {
            out[*i] = r;
        }
    }
    out
}

/// A way an arrow key moves among blocks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toward {
    Up,
    Down,
    Left,
    Right,
}

/// The block an arrow reaches from `rects[from]`: of those lying past its
/// edge that way, one beside it (sharing some of its span across the way)
/// before one only diagonal from it, then the nearest, then the one whose
/// middle is closest to its middle; the first of equals.
pub fn neighbour(rects: &[Rect], from: usize, toward: Toward) -> Option<usize> {
    const EPS: f64 = 0.5;
    let f = rects.get(from)?;
    let score = |c: &Rect| {
        let (gap, lo, hi, c_lo, c_hi) = match toward {
            Toward::Right => (c.x - (f.x + f.w), f.y, f.y + f.h, c.y, c.y + c.h),
            Toward::Left => (f.x - (c.x + c.w), f.y, f.y + f.h, c.y, c.y + c.h),
            Toward::Down => (c.y - (f.y + f.h), f.x, f.x + f.w, c.x, c.x + c.w),
            Toward::Up => (f.y - (c.y + c.h), f.x, f.x + f.w, c.x, c.x + c.w),
        };
        let beside = hi.min(c_hi) - lo.max(c_lo) > EPS;
        let middles = ((lo + hi) - (c_lo + c_hi)).abs() / 2.0;
        (gap >= -EPS).then_some((!beside, gap.max(0.0).round(), middles))
    };
    rects
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != from)
        .filter_map(|(i, c)| score(c).map(|s| (i, s)))
        .min_by(|(_, a), (_, b)| {
            a.0.cmp(&b.0)
                .then(a.1.total_cmp(&b.1))
                .then(a.2.total_cmp(&b.2))
        })
        .map(|(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::{Rect, Toward, neighbour, squarify};

    #[test]
    fn areas_are_proportional_and_inside_bounds() {
        let b = Rect::new(10.0, 20.0, 600.0, 400.0);
        let w = [6.0, 6.0, 4.0, 3.0, 2.0, 2.0, 1.0];
        let rects = squarify(&w, b);
        let total: f64 = w.iter().sum();
        for (weight, r) in w.iter().zip(&rects) {
            let expected = weight / total * b.area();
            assert!((r.area() - expected).abs() < 1e-6, "{r:?} vs {expected}");
            assert!(r.x >= b.x - 1e-9 && r.y >= b.y - 1e-9);
            assert!(r.x + r.w <= b.x + b.w + 1e-6 && r.y + r.h <= b.y + b.h + 1e-6);
        }
        let sum: f64 = rects.iter().map(Rect::area).sum();
        assert!((sum - b.area()).abs() < 1e-6);
    }

    #[test]
    fn classic_example_has_good_aspect_ratios() {
        // The paper's example: 6,6,4,3,2,2,1 in a 6x4 box. Our tail differs from the
        // figure (a lone 1 in a 0.6 x 1.67 slot, aspect 2.78) but stays squarish.
        let rects = squarify(
            &[6.0, 6.0, 4.0, 3.0, 2.0, 2.0, 1.0],
            Rect::new(0.0, 0.0, 6.0, 4.0),
        );
        let worst = rects
            .iter()
            .map(|r| (r.w / r.h).max(r.h / r.w))
            .fold(0.0, f64::max);
        assert!(worst <= 3.0, "worst aspect {worst}");
    }

    #[test]
    fn zero_weights_and_empty_bounds() {
        assert_eq!(
            squarify(&[0.0, 0.0], Rect::new(0.0, 0.0, 10.0, 10.0)),
            vec![Rect::default(); 2]
        );
        assert_eq!(
            squarify(&[1.0], Rect::new(0.0, 0.0, 0.0, 10.0)),
            vec![Rect::default()]
        );
        let r = squarify(&[0.0, 5.0], Rect::new(0.0, 0.0, 10.0, 10.0));
        assert_eq!(r[0], Rect::default());
        assert!((r[1].area() - 100.0).abs() < 1e-9);
    }

    #[test]
    fn arrows_reach_the_block_beside_before_a_diagonal_one() {
        // A tall block on the left, two stacked on the right.
        let rects = [
            Rect::new(0.0, 0.0, 60.0, 100.0),
            Rect::new(60.0, 0.0, 40.0, 50.0),
            Rect::new(60.0, 50.0, 40.0, 50.0),
        ];
        assert_eq!(neighbour(&rects, 0, Toward::Right), Some(1));
        assert_eq!(neighbour(&rects, 1, Toward::Down), Some(2));
        assert_eq!(neighbour(&rects, 2, Toward::Up), Some(1));
        assert_eq!(neighbour(&rects, 2, Toward::Left), Some(0));
        assert_eq!(neighbour(&rects, 1, Toward::Right), None);
        assert_eq!(neighbour(&rects, 0, Toward::Up), None);
        // Nothing beside it that way: the diagonal one.
        let rects = [
            Rect::new(0.0, 0.0, 10.0, 10.0),
            Rect::new(20.0, 20.0, 10.0, 10.0),
        ];
        assert_eq!(neighbour(&rects, 0, Toward::Right), Some(1));
        assert_eq!(neighbour(&rects, 1, Toward::Up), Some(0));
        assert_eq!(neighbour(&rects, 9, Toward::Up), None);
    }
}
