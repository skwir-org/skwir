//! A column of labels beside their values (Properties, About, the bulk
//! rename's fields): as wide as the widest word of its labels, so a label
//! wraps between its words and never inside one, and the values stand in
//! one column. Never a length written for it: a word wider than its
//! column, in another language or at another text size, is broken
//! between its letters ("Languag", then "e").

use gpui::{App, Rems};

use crate::sizes::{REM, Text, sized};
use crate::theme::Theme;

/// The width the `labels`, drawn at `text` in the interface's font, share,
/// in the rem in force: the widest of their words, kept between `least`
/// and `most` (lengths at 100 %); past `most`, a word is cut as before.
///
/// A text's width follows from its words and its font, known before
/// layout (no bounds read, no frame late, as `nav_menu` sizes a popup by
/// its widest label): each word measured as gpui's line wrapper measures
/// it (`name_lines::width`), at the size at 100 %. That size and the width
/// are both in rems, so the text size chosen leaves their ratio as it is.
/// A word ends at a space alone: where gpui may also break (between two
/// Chinese characters) the column is wider than it must be, never
/// narrower.
pub fn column<'a>(
    labels: impl IntoIterator<Item = &'a str>,
    text: Text,
    least: f32,
    most: f32,
    theme: &Theme,
    cx: &App,
) -> Rems {
    let font = gpui::font(theme.font.clone());
    let size = gpui::px(text.rems() * REM);
    let widest = labels
        .into_iter()
        .flat_map(|label| label.split(' '))
        .map(|word| f32::from(crate::name_lines::width(word, &font, size, cx)))
        .fold(0., f32::max);
    // A pixel of slack: the line wrapper's widths summed, against the
    // shaped line's.
    sized((widest + 1.).ceil().clamp(least, most))
}
