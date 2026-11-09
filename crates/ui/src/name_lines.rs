//! Where a name that wraps breaks: into lines about as long as each other,
//! at the points a reader expects, never leaving a character or two alone
//! on the last line (`photograph_final.jpe` over `g`).
//!
//! gpui fills each line as far as it goes, so a long name without spaces
//! spills what is left onto the next. Here the lines a name takes stay as
//! many as gpui gives it (the grid counts them, `GridView::measure`, and
//! its rows keep their heights), and where they break is chosen: the
//! lines balanced (each one's distance from their mean, squared, as a
//! share of the width), each break costing what it reads like: nothing at
//! a space or before the extension, little after a separator (any
//! punctuation or symbol, `_ - . ,`, or before an opening bracket) or at a
//! change from letters to digits or into a capital, more in the middle of
//! a word, most before a separator or an apostrophe, or after an opening
//! bracket (a line starting "-06", `l` over `'été`). The separators and
//! the apostrophe are Dolphin's (`KStringHandler::preProcessWrap` in
//! `KCoreAddons`, which gives Qt's greedy wrapping those chances to
//! break); the balance is ours. The extension is never split. The name is
//! then drawn with those breaks as newlines, so gpui wraps nothing more.
//!
//! A line ends only between two grapheme clusters, and a break is priced by
//! the clusters on either side, each as its first character says: an accent
//! typed apart (NFD), a virama, a joiner or a Hangul jamo is part of what
//! it follows, never a place to break nor a separator.
//!
//! Widths are each character's, as gpui's line wrapper measures them
//! (`TextSystem::layout_width`), kept per font and size, so a line kept
//! here fits where gpui's wrapper and its middle truncation measure it.
//! A name broken is kept by what it depends on (`KEPT`); a frame only
//! looks it up. The search is a small dynamic one over the places a line
//! may end: linear for two lines, quadratic in the name's length for
//! three, a name being at most 255 bytes.

use std::cell::RefCell;
use std::collections::HashMap;

use gpui::{App, Font, FontId, LineWrapper, Pixels, SharedString};
use unicode_segmentation::UnicodeSegmentation;

/// What a break costs, in the units of a line's distance from the mean:
/// a share of the width, squared.
const AT_SPACE: f32 = 0.;
const BEFORE_EXTENSION: f32 = 0.;
const AFTER_SEPARATOR: f32 = 0.02;
const AT_CHANGE: f32 = 0.04;
const MID_WORD: f32 = 0.15;
/// A line that would start with a separator or a closing bracket, or end
/// on an opening one ("-06-at…"): worse than the middle of a word, as
/// typography never breaks there.
const BEFORE_SEPARATOR: f32 = 0.25;
/// The longest extension kept whole, its dot included (`.jpeg`, `.tar.gz`'s
/// `.gz`, `.markdown` not).
const EXTENSION: usize = 8;
/// Room kept on each line, in pixels: laid out, a box's edges land on
/// whole device pixels, and a line a hair too wide would wrap again.
const SLACK: f32 = 1.;
/// Names kept broken; past it, all are forgotten at once.
const KEPT: usize = 4096;

/// Each character's width in one font at one size.
struct Widths {
    font: FontId,
    size: u32,
    ascii: [f32; 128],
    other: HashMap<char, f32>,
}

/// What a name broken depends on.
#[derive(Clone, PartialEq, Eq, Hash)]
struct Key {
    name: SharedString,
    lines: usize,
    width: u32,
    font: FontId,
    size: u32,
}

thread_local! {
    /// The fonts and sizes names are measured in: the grid's, a drag's.
    static WIDTHS: RefCell<Vec<Widths>> = const { RefCell::new(Vec::new()) };
    static BROKEN: RefCell<HashMap<Key, Option<SharedString>>> = RefCell::new(HashMap::new());
}

/// `name` drawn on `lines` lines `width` wide, in `font` at `size`: its
/// breaks chosen (see the module), as newlines. `None` where it is drawn
/// as it is: it takes one line, holds a newline of its own, or takes more
/// lines than `lines` (cut in the middle by gpui).
pub fn broken(
    name: &SharedString,
    lines: usize,
    width: Pixels,
    font: &Font,
    size: Pixels,
    cx: &App,
) -> Option<SharedString> {
    if lines < 2 || name.contains('\n') {
        return None;
    }
    let font = cx.text_system().resolve_font(font);
    let key = Key {
        name: name.clone(),
        lines,
        width: f32::from(width).to_bits(),
        font,
        size: f32::from(size).to_bits(),
    };
    if let Some(kept) = BROKEN.with(|kept| kept.borrow().get(&key).cloned()) {
        return kept;
    }
    let chars: Vec<char> = name.chars().collect();
    let widths = measured(&chars, font, size, cx);
    let text = lines_of(&chars, &widths, lines, f32::from(width) - SLACK).map(|ranges| {
        let lines: Vec<String> = ranges
            .into_iter()
            .map(|(start, end)| chars[start..end].iter().collect())
            .collect();
        SharedString::from(lines.join("\n"))
    });
    BROKEN.with(|kept| {
        let mut kept = kept.borrow_mut();
        if kept.len() >= KEPT {
            kept.clear();
        }
        kept.insert(key, text.clone());
    });
    text
}

/// The lines `text` takes where `wrapper` wraps it at `width`, at most
/// `most`: a newline ends a line, and no line that goes on is indented, as
/// gpui draws it. What the grid's rows are counted by
/// (`GridView::measure`), and a drag's name: `broken` keeps to as many.
pub fn count(text: &str, width: Pixels, most: usize, wrapper: &mut LineWrapper) -> usize {
    text.split('\n')
        .map(|line| {
            wrapper
                .wrap_line(
                    &[gpui::LineFragment::text(line)],
                    width,
                    gpui::IndentAdjustment::NoIndent,
                )
                .take(most - 1)
                .count()
                + 1
        })
        .sum::<usize>()
        .min(most)
}

/// `text`'s width as gpui's line wrapper measures it, in `font` at
/// `size`: each character's own, summed.
pub fn width(text: &str, font: &Font, size: Pixels, cx: &App) -> Pixels {
    let font = cx.text_system().resolve_font(font);
    let chars: Vec<char> = text.chars().collect();
    gpui::px(measured(&chars, font, size, cx).iter().sum())
}

/// Each of `chars`' widths in `font` at `size`.
fn measured(chars: &[char], font: FontId, size: Pixels, cx: &App) -> Vec<f32> {
    WIDTHS.with(|all| {
        let mut all = all.borrow_mut();
        let bits = f32::from(size).to_bits();
        let found = all.iter().position(|w| w.font == font && w.size == bits);
        let at = found.unwrap_or_else(|| {
            // A few fonts and sizes at once: the grid's and a drag's.
            if all.len() >= 4 {
                all.remove(0);
            }
            all.push(Widths {
                font,
                size: bits,
                ascii: [f32::NAN; 128],
                other: HashMap::new(),
            });
            all.len() - 1
        });
        let widths = &mut all[at];
        let text = cx.text_system();
        chars
            .iter()
            .map(|&c| {
                let slot = match usize::try_from(u32::from(c)) {
                    Ok(code) if code < 128 => Some(code),
                    _ => None,
                };
                if let Some(code) = slot
                    && !widths.ascii[code].is_nan()
                {
                    return widths.ascii[code];
                }
                if let Some(width) = widths.other.get(&c) {
                    return *width;
                }
                let width = f32::from(text.layout_width(font, size, c));
                match slot {
                    Some(code) => widths.ascii[code] = width,
                    None => {
                        widths.other.insert(c, width);
                    }
                }
                width
            })
            .collect()
    })
}

/// Where the extension starts: its dot, where it is short and has no
/// space, and is not the whole name (`.bashrc`).
fn extension(chars: &[char]) -> Option<usize> {
    let dot = chars.iter().rposition(|&c| c == '.')?;
    let length = chars.len() - dot;
    (dot > 0 && (2..=EXTENSION).contains(&length) && !chars[dot..].contains(&' ')).then_some(dot)
}

/// Where each grapheme cluster of `chars` starts, as an index in `chars`:
/// the only places a line may begin.
fn clusters(chars: &[char]) -> Vec<usize> {
    let text: String = chars.iter().collect();
    let mut at = 0;
    text.graphemes(true)
        .map(|cluster| {
            let start = at;
            at += cluster.chars().count();
            start
        })
        .collect()
}

/// What a line costs ending between a cluster starting with `before` and
/// one starting with `after`, the latter being the extension's dot or not.
fn cost_of_break(before: char, after: char, at_extension: bool) -> f32 {
    if before == ' ' {
        AT_SPACE
    } else if at_extension {
        BEFORE_EXTENSION
    } else if apostrophe(after) || opening(before) {
        // Never inside `l'été` or `don't`, nor after an opening bracket.
        BEFORE_SEPARATOR
    } else if opening(after) {
        AFTER_SEPARATOR
    } else if separator(after) {
        // A line never starts with punctuation.
        BEFORE_SEPARATOR
    } else if apostrophe(before) {
        MID_WORD
    } else if separator(before) {
        AFTER_SEPARATOR
    } else if (before.is_alphabetic() && after.is_numeric())
        || (before.is_numeric() && after.is_alphabetic())
        || (before.is_lowercase() && after.is_uppercase())
    {
        AT_CHANGE
    } else {
        MID_WORD
    }
}

/// Punctuation or a symbol: neither a letter, a digit, a space nor a
/// control (Dolphin's `isPunct() || isSymbol()`).
fn separator(c: char) -> bool {
    !c.is_alphanumeric() && !c.is_whitespace() && !c.is_control()
}

/// An opening bracket or quote: a line may break before it, never after.
fn opening(c: char) -> bool {
    matches!(
        c,
        '(' | '['
            | '{'
            | '<'
            | '«'
            | '‹'
            | '“'
            | '‘'
            | '„'
            | '‚'
            | '（'
            | '［'
            | '｛'
            | '「'
            | '『'
    )
}

/// An apostrophe, which joins what is on either side (`l'été`, `don't`).
fn apostrophe(c: char) -> bool {
    matches!(c, '\'' | '’' | 'ʼ')
}

/// `chars` in `lines` lines at most `width` wide, each character
/// `widths[i]` wide: each line's start and end (spaces where it breaks
/// left out), chosen as the module says. `None` where no `lines` lines
/// hold it.
fn lines_of(
    chars: &[char],
    widths: &[f32],
    lines: usize,
    width: f32,
) -> Option<Vec<(usize, usize)>> {
    let extension = extension(chars);
    lines_within(chars, widths, lines, width, extension)
        // An extension that leaves no way to break: split it after all.
        .or_else(|| extension.and_then(|_| lines_within(chars, widths, lines, width, None)))
}

fn lines_within(
    chars: &[char],
    widths: &[f32],
    lines: usize,
    width: f32,
    extension: Option<usize>,
) -> Option<Vec<(usize, usize)>> {
    let length = chars.len();
    if lines < 2 || length < lines || width <= 0. {
        return None;
    }
    let mut prefix = Vec::with_capacity(length + 1);
    prefix.push(0.);
    for w in widths {
        prefix.push(prefix.last().copied().unwrap_or(0.) + w);
    }
    // More than its lines hold, whatever the spaces they break at: left
    // to gpui, which cuts it.
    let spaces: f32 = chars
        .iter()
        .zip(widths)
        .filter(|(c, _)| **c == ' ')
        .map(|(_, w)| w)
        .sum();
    if prefix[length] - spaces > width * lines as f32 {
        return None;
    }
    let mean = prefix[length] / lines as f32;
    // Where a line may end: between two clusters, after the last space of
    // a run, never inside the extension; each with what the break costs,
    // and where the line ending there stops being drawn (the spaces it
    // breaks at left out).
    let mut ends: Vec<(usize, f32, usize)> = vec![(0, 0., 0)];
    let shown = |end: usize| {
        let mut shown = end;
        while shown > 0 && chars[shown - 1] == ' ' {
            shown -= 1;
        }
        shown
    };
    for pair in clusters(chars).windows(2) {
        let (before, at) = (pair[0], pair[1]);
        if chars[at] == ' ' || extension.is_some_and(|dot| at > dot) {
            continue;
        }
        let cost = cost_of_break(chars[before], chars[at], extension == Some(at));
        ends.push((at, cost, shown(at)));
    }
    ends.push((length, 0., shown(length)));
    // best[k][j]: the least the first k lines, the last ending at ends[j],
    // cost, and where that line began. A line from ends[i] to ends[j] is
    // wider the earlier it begins: the search for its start stops at the
    // first too wide, so it costs what a line holds, not the name.
    let count = ends.len();
    let mut best = vec![vec![(f32::INFINITY, usize::MAX); count]; lines + 1];
    best[0][0] = (0., usize::MAX);
    for k in 1..=lines {
        for j in 1..count {
            // The last line ends at the name's end, and none before it does.
            if (k == lines) != (j == count - 1) {
                continue;
            }
            let (_, broken, stop) = ends[j];
            for i in (0..j).rev() {
                let start = ends[i].0;
                if stop <= start {
                    continue;
                }
                let w = prefix[stop] - prefix[start];
                if w > width {
                    break;
                }
                let (before, _) = best[k - 1][i];
                if !before.is_finite() {
                    continue;
                }
                let cost = ((w - mean) / width).powi(2);
                let total = before + cost + if k < lines { broken } else { 0. };
                if total < best[k][j].0 {
                    best[k][j] = (total, i);
                }
            }
        }
    }
    if !best[lines][count - 1].0.is_finite() {
        return None;
    }
    let mut out = Vec::with_capacity(lines);
    let mut j = count - 1;
    for k in (1..=lines).rev() {
        let i = best[k][j].1;
        out.push((ends[i].0, ends[j].2));
        j = i;
    }
    out.reverse();
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::lines_of;

    /// The lines of `name` as text, each character one wide.
    fn broken(name: &str, lines: usize, width: f32) -> Option<Vec<String>> {
        broken_in(name, lines, width, |_| 1.)
    }

    /// The lines of `name` as text, each character `wide(c)` wide.
    fn broken_in(
        name: &str,
        lines: usize,
        width: f32,
        wide: impl Fn(char) -> f32,
    ) -> Option<Vec<String>> {
        let chars: Vec<char> = name.chars().collect();
        let widths: Vec<f32> = chars.iter().map(|&c| wide(c)).collect();
        lines_of(&chars, &widths, lines, width).map(|ranges| {
            ranges
                .into_iter()
                .map(|(a, b)| chars[a..b].iter().collect())
                .collect()
        })
    }

    #[test]
    fn no_character_is_left_alone_on_the_last_line() {
        // gpui: "photograph_final.jpe" over "g".
        assert_eq!(
            broken("photograph_final.jpeg", 2, 20.).unwrap(),
            ["photograph_", "final.jpeg"]
        );
        // A long run of digits: balanced, the extension whole.
        let lines = broken("IMG20260512183045.jpg", 2, 20.).unwrap();
        assert_eq!(lines[1].rsplit('.').next(), Some("jpg"), "{lines:?}");
        assert!(lines[1].len() >= 8, "{lines:?}");
        // Into a capital.
        assert_eq!(
            broken("ProjectBudgetDraft.ods", 2, 20.).unwrap(),
            ["ProjectBudget", "Draft.ods"]
        );
    }

    #[test]
    fn a_space_is_where_a_line_breaks_and_is_not_drawn() {
        assert_eq!(
            broken("Annual report 2026.pdf", 2, 16.).unwrap(),
            ["Annual report", "2026.pdf"]
        );
        // Spaces in a run: the next line starts at the word.
        assert_eq!(broken("aaaa   bbbb", 2, 6.).unwrap(), ["aaaa", "bbbb"]);
    }

    #[test]
    fn no_line_starts_with_a_separator() {
        // As a proportional font draws them: digits and separators narrow.
        let wide = |c: char| match c {
            '0'..='9' => 0.6,
            '-' | '.' => 0.4,
            _ => 1.,
        };
        let name = "screenshot-2026-11-06-at-10.42.17.png";
        // The two halves balanced, after a hyphen.
        assert_eq!(
            broken_in(name, 2, 15., wide).unwrap(),
            ["screenshot-2026-", "11-06-at-10.42.17.png"]
        );
        // Before a separator costs more than the middle of a word, after
        // one less; before the extension's dot, nothing.
        let chars: Vec<char> = "ab-cd.txt".chars().collect();
        let dot = super::extension(&chars);
        let cost = |at: usize| super::cost_of_break(chars[at - 1], chars[at], dot == Some(at));
        assert!(cost(2) > super::MID_WORD, "before the hyphen");
        assert!(cost(3) < super::MID_WORD, "after it");
        assert!(cost(1) == super::MID_WORD, "inside a word");
        assert!(cost(5) == 0., "before the extension");
        // Any punctuation or symbol, as Dolphin has them.
        let chars: Vec<char> = "a–b~c(d)e".chars().collect();
        let cost = |at: usize| super::cost_of_break(chars[at - 1], chars[at], false);
        assert!(cost(1) > super::MID_WORD, "before the dash");
        assert!(cost(2) < super::MID_WORD, "after it");
        assert!(cost(4) < super::MID_WORD, "after the tilde");
        assert!(cost(5) < super::MID_WORD, "before an opening bracket");
        assert!(cost(6) > super::MID_WORD, "not after it");
        assert!(cost(8) < super::MID_WORD, "after a closing one");
    }

    #[test]
    fn an_apostrophe_holds_what_is_on_either_side() {
        let chars: Vec<char> = "l'été".chars().collect();
        let cost = |at: usize| super::cost_of_break(chars[at - 1], chars[at], false);
        assert!(cost(1) > super::MID_WORD, "not before it");
        assert!(cost(2) >= super::MID_WORD, "nor cheaply after it");
        // Spaces elsewhere: the lines break at one, the apostrophe kept.
        let lines = broken("don't stop the music.mp3", 2, 14.).unwrap();
        assert_eq!(lines, ["don't stop", "the music.mp3"]);
        // None: the whole name is one word, cut anywhere but there.
        let lines = broken("don'tstopthemusic.mp3", 2, 12.).unwrap();
        assert!(
            !lines[0].ends_with("don") && !lines[1].starts_with('\''),
            "{lines:?}"
        );
    }

    /// `lines`, as `name` drawn on them: they hold it, cluster by cluster.
    fn starts_on_a_cluster(name: &str, lines: &[String]) {
        let clusters: Vec<String> =
            unicode_segmentation::UnicodeSegmentation::graphemes(name, true)
                .map(str::to_owned)
                .collect();
        let mut at = 0;
        for line in lines {
            let mut left = line.as_str();
            while !left.is_empty() {
                let cluster = &clusters[at];
                assert!(
                    left.starts_with(cluster.as_str()),
                    "{left:?} is not at a cluster of {name:?}: {cluster:?}"
                );
                left = &left[cluster.len()..];
                at += 1;
            }
        }
        assert_eq!(at, clusters.len(), "{lines:?} of {name:?}");
    }

    #[test]
    fn a_line_ends_between_clusters_only() {
        // An accent typed apart (NFD) is its letter's, however cheap a
        // separator would be: the line does not stop just after it.
        let name = "re\u{301}sume\u{301}rapport.pdf";
        let lines = broken(name, 2, 11.).unwrap();
        starts_on_a_cluster(name, &lines);
        assert!(!lines[0].ends_with('\u{301}'), "{lines:?}");
        // Nor just before it, the mark alone at a line's start.
        let name = "re\u{301}sume\u{301}_final_draft.pdf";
        for width in [12., 16., 20.] {
            let lines = broken(name, 2, width).unwrap();
            starts_on_a_cluster(name, &lines);
        }
        // Devanagari: a conjunct stays whole, never cut after the virama
        // that joins it, nor before a vowel sign.
        let name = "क्षत्रियक्षत्रियक्षत्रिय.txt";
        for width in [15., 16., 18., 20.] {
            let lines = broken(name, 2, width).unwrap();
            starts_on_a_cluster(name, &lines);
            assert!(!lines[0].ends_with('\u{94d}'), "{lines:?}");
        }
        // Hangul in jamo (NFD): each jamo is a letter to `char`, a syllable
        // one cluster, and a balanced end may not land inside it.
        let name = "\u{1112}\u{1161}\u{11ab}\u{1100}\u{1173}\u{11af}\u{1100}\u{1173}\u{11af}\u{110c}\u{1161}.txt";
        for width in [9., 10., 12.] {
            let lines = broken(name, 2, width).unwrap();
            starts_on_a_cluster(name, &lines);
        }
        // Emoji joined by a ZWJ, a flag: one cluster each.
        let name = "\u{1f469}\u{200d}\u{1f4bb}\u{1f1eb}\u{1f1f7}report_final.txt";
        starts_on_a_cluster(name, &broken(name, 2, 12.).unwrap());
    }

    /// No two clusters a line could end between leaves it at a width where
    /// the characters alone would have fitted: left to gpui, which cuts it.
    #[test]
    fn a_cluster_wider_than_a_line_is_left_to_gpui() {
        let name = "\u{1112}\u{1161}\u{11ab}\u{1100}\u{1173}\u{11af}\u{1100}\u{1173}\u{11af}\u{110c}\u{1161}.txt";
        assert_eq!(broken(name, 2, 8.), None);
    }

    #[test]
    fn a_cluster_is_priced_as_what_it_starts_with() {
        // An accented lowercase letter into a capital is still a change:
        // the mark does not hide it.
        let chars: Vec<char> = "e\u{301}M".chars().collect();
        let clusters = super::clusters(&chars);
        assert_eq!(clusters, [0, 2]);
        let cost = super::cost_of_break(chars[clusters[0]], chars[clusters[1]], false);
        assert!(cost == super::AT_CHANGE, "into a capital, past the accent");
    }

    #[test]
    fn three_lines_come_out_even() {
        let lines = broken("expedition-notes-from-the-high-plateau.txt", 3, 16.).unwrap();
        assert_eq!(lines.len(), 3);
        assert!(lines.iter().all(|line| line.len() <= 16), "{lines:?}");
        assert!(lines.iter().all(|line| line.len() >= 10), "{lines:?}");
        assert_eq!(lines.concat(), "expedition-notes-from-the-high-plateau.txt");
    }

    #[test]
    fn what_does_not_fit_is_left_to_gpui() {
        // Longer than three lines hold: cut in the middle by gpui.
        assert_eq!(broken(&"x".repeat(40), 3, 10.), None);
        // One line is not broken here.
        assert_eq!(broken("short.txt", 1, 20.), None);
        // An extension wider than a line is split after all.
        assert_eq!(broken("ab.abcdefg", 2, 5.).unwrap().concat(), "ab.abcdefg");
    }

    #[test]
    fn a_dot_file_has_no_extension_to_keep() {
        assert_eq!(
            super::extension(&".bashrc".chars().collect::<Vec<_>>()),
            None
        );
        assert_eq!(
            super::extension(&"a.tar.gz".chars().collect::<Vec<_>>()),
            Some(5)
        );
    }
}
