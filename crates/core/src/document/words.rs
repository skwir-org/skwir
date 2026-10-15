//! A page's text as an engine that knows no glyph says it: runs of text
//! (words, lines), each in a box of the page. Made into the reader's
//! characters, and searched there, alike for every such engine (a plugin
//! from outside skwir, `DjVu`'s).

use super::{MAX_PAGE_CHARS, Rect, TextChar};

/// Text an engine places as a whole, in a box of the page (fractions of
/// its width and height from its top left, as every `Rect` of the
/// protocol).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Run {
    pub text: String,
    pub bounds: Rect,
    /// Where each of its characters starts, as a fraction of the page's
    /// width, one a character in order: a selection then follows each
    /// glyph. Without them, each character is an equal part of the run.
    pub edges: Vec<f32>,
}

/// A fraction of the page, kept on it.
fn unit(v: f32) -> f32 {
    if v.is_finite() { v.clamp(0., 1.) } else { 0. }
}

/// A page's runs as the reader's characters: each where its run's edges
/// say it starts, else an equal column of the run's box; between two
/// runs, a space when they share a line, else a line break, drawn
/// nowhere. Boxes are kept on the page, and a page's characters within
/// `MAX_PAGE_CHARS`.
pub fn chars(runs: &[Run]) -> Vec<TextChar> {
    let mut chars = Vec::new();
    for (i, run) in runs.iter().enumerate() {
        if chars.len() >= MAX_PAGE_CHARS {
            break;
        }
        let (x, y) = (unit(run.bounds.x), unit(run.bounds.y));
        let (width, height) = (
            unit(run.bounds.width).min(1. - x),
            unit(run.bounds.height).min(1. - y),
        );
        let letters: Vec<char> = run.text.chars().collect();
        #[allow(clippy::cast_precision_loss)] // A run's length.
        let each = width / letters.len().max(1) as f32;
        // Each character's own start, when the run gives one a character
        // in order and within it.
        let edges = (run.edges.len() == letters.len()
            && run.edges.windows(2).all(|pair| pair[0] <= pair[1]))
        .then(|| {
            run.edges
                .iter()
                .map(|edge| unit(*edge).clamp(x, x + width))
                .collect::<Vec<f32>>()
        });
        for (k, letter) in letters.iter().enumerate() {
            #[allow(clippy::cast_precision_loss)]
            let (left, right) = match &edges {
                Some(edges) => (edges[k], edges.get(k + 1).copied().unwrap_or(x + width)),
                None => (x + each * k as f32, x + each * (k + 1) as f32),
            };
            chars.push(TextChar {
                text: letter.to_string(),
                bounds: Rect {
                    x: left,
                    y,
                    width: right - left,
                    height,
                },
            });
        }
        if let Some(next) = runs.get(i + 1) {
            let (next_y, next_height) = (unit(next.bounds.y), unit(next.bounds.height));
            let same_line = next_y < y + height && y < next_y + next_height;
            chars.push(TextChar {
                text: if same_line { " " } else { "\n" }.into(),
                bounds: Rect {
                    x: x + width,
                    y,
                    width: 0.,
                    height: 0.,
                },
            });
        }
    }
    chars.truncate(MAX_PAGE_CHARS);
    chars
}

/// A character as a search compares it: lowercase, any white space a space.
pub fn folded(c: char) -> char {
    if c.is_whitespace() {
        ' '
    } else {
        c.to_lowercase().next().unwrap_or(c)
    }
}

/// Where `needle` (folded) starts in `chars`, each match after the last.
pub fn found(chars: &[TextChar], needle: &[char]) -> Vec<usize> {
    let hay: Vec<char> = chars
        .iter()
        .map(|c| c.text.chars().next().map_or(' ', folded))
        .collect();
    let mut starts = Vec::new();
    let mut at = 0;
    while at + needle.len() <= hay.len() {
        if hay[at..at + needle.len()] == *needle {
            starts.push(at);
            at += needle.len();
        } else {
            at += 1;
        }
    }
    starts
}

/// Each match of `query` in a page's `chars`, whatever its case, a line
/// break matching a space: the boxes of the characters it covers that are
/// drawn somewhere. None for a query of white space alone.
pub fn matches(chars: &[TextChar], query: &str) -> Vec<Vec<Rect>> {
    let needle: Vec<char> = query.chars().map(folded).collect();
    if needle.iter().all(|c| *c == ' ') {
        return Vec::new();
    }
    found(chars, &needle)
        .into_iter()
        .map(|at| {
            chars[at..at + needle.len()]
                .iter()
                .map(|c| c.bounds)
                .filter(|b| b.height > 0.)
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str, x: f32, y: f32, width: f32) -> Run {
        Run {
            text: text.into(),
            bounds: Rect {
                x,
                y,
                width,
                height: 0.1,
            },
            edges: Vec::new(),
        }
    }

    #[test]
    fn a_run_that_says_where_its_characters_start_is_selected_glyph_by_glyph() {
        let wide = Run {
            edges: vec![0.1, 0.4, 0.5],
            ..run("Wil", 0.1, 0.1, 0.5)
        };
        let chars = chars(&[wide]);
        let spans: Vec<(f32, f32)> = chars
            .iter()
            .map(|c| (c.bounds.x, c.bounds.x + c.bounds.width))
            .collect();
        for ((left, right), (l, r)) in spans.iter().zip([(0.1, 0.4), (0.4, 0.5), (0.5, 0.6)]) {
            assert!(
                (left - l).abs() < 1e-6 && (right - r).abs() < 1e-6,
                "{spans:?}"
            );
        }
        // Edges that do not hold together (too few, backwards) are not
        // trusted: equal columns.
        for edges in [vec![0.1, 0.2], vec![0.5, 0.4, 0.3]] {
            let chars = super::chars(&[Run {
                edges,
                ..run("Wil", 0.1, 0.1, 0.6)
            }]);
            assert!((chars[1].bounds.x - 0.3).abs() < 1e-6);
        }
    }

    #[test]
    fn runs_are_characters_spaced_on_a_line_and_broken_between_lines() {
        let chars = chars(&[
            run("Hello", 0.1, 0.1, 0.5),
            run("you", 0.7, 0.1, 0.3),
            run("café", 0.1, 0.3, 0.4),
        ]);
        let text: String = chars.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(text, "Hello you\ncafé");
        // Each letter a column of its run's box.
        assert!((chars[1].bounds.x - 0.2).abs() < 1e-6);
        assert!((chars[1].bounds.width - 0.1).abs() < 1e-6);
        // What joins them is drawn nowhere.
        assert!(chars[5].bounds.height.abs() < f32::EPSILON);
        // A box off the page is kept on it.
        let off = super::chars(&[run("x", 0.9, -1., 5.)]);
        assert_eq!(
            (off[0].bounds.y, off[0].bounds.x + off[0].bounds.width),
            (0., 1.)
        );
    }

    #[test]
    fn a_match_is_found_whatever_its_case_and_across_lines() {
        let chars = chars(&[
            run("Hello", 0.1, 0.1, 0.5),
            run("you", 0.7, 0.1, 0.3),
            run("Café", 0.1, 0.3, 0.4),
            run("you", 0.1, 0.5, 0.3),
            run("CAFÉ", 0.5, 0.5, 0.4),
        ]);
        let needle: Vec<char> = "YOU café".chars().map(folded).collect();
        assert_eq!(found(&chars, &needle), [6, 15]);
        assert!(found(&chars, &['z']).is_empty());
        // A match's boxes are its characters drawn: the line break that
        // joins "you" and "Café" is not one.
        let hits = matches(&chars, "you café");
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].len(), 7);
        assert!(matches(&chars, " \n").is_empty());
    }
}
