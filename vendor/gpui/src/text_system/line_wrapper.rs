use crate::{FontId, FontRun, Pixels, PlatformTextSystem, SharedString, TextRun, px};
use collections::HashMap;
use std::{iter, sync::Arc};

/// The GPUI line wrapper, used to wrap lines of text to a given width.
pub struct LineWrapper {
    platform_text_system: Arc<dyn PlatformTextSystem>,
    pub(crate) font_id: FontId,
    pub(crate) font_size: Pixels,
    cached_ascii_char_widths: [Option<Pixels>; 128],
    cached_other_char_widths: HashMap<char, Pixels>,
}

impl LineWrapper {
    /// The maximum indent that can be applied to a line.
    pub const MAX_INDENT: u32 = 256;

    pub(crate) fn new(
        font_id: FontId,
        font_size: Pixels,
        text_system: Arc<dyn PlatformTextSystem>,
    ) -> Self {
        Self {
            platform_text_system: text_system,
            font_id,
            font_size,
            cached_ascii_char_widths: [None; 128],
            cached_other_char_widths: HashMap::default(),
        }
    }

    /// Wrap a line of text to the given width with this wrapper's font and font size.
    pub fn wrap_line<'a>(
        &'a mut self,
        fragments: &'a [LineFragment],
        wrap_width: Pixels,
    ) -> impl Iterator<Item = Boundary> + 'a {
        let mut width = px(0.);
        let mut first_non_whitespace_ix = None;
        let mut indent = None;
        let mut last_candidate_ix = 0;
        let mut last_candidate_width = px(0.);
        let mut last_wrap_ix = 0;
        let mut prev_c = '\0';
        let mut index = 0;
        let mut candidates = fragments
            .iter()
            .flat_map(move |fragment| fragment.wrap_boundary_candidates())
            .peekable();
        iter::from_fn(move || {
            for candidate in candidates.by_ref() {
                let ix = index;
                index += candidate.len_utf8();
                let mut new_prev_c = prev_c;
                let item_width = match candidate {
                    WrapBoundaryCandidate::Char { character: c } => {
                        if c == '\n' {
                            continue;
                        }

                        if Self::is_word_char(c) {
                            if prev_c == ' ' && c != ' ' && first_non_whitespace_ix.is_some() {
                                last_candidate_ix = ix;
                                last_candidate_width = width;
                            }
                        } else {
                            // CJK may not be space separated, e.g.: `Hello world你好世界`
                            if c != ' ' && first_non_whitespace_ix.is_some() {
                                last_candidate_ix = ix;
                                last_candidate_width = width;
                            }
                        }

                        if c != ' ' && first_non_whitespace_ix.is_none() {
                            first_non_whitespace_ix = Some(ix);
                        }

                        new_prev_c = c;

                        self.width_for_char(c)
                    }
                    WrapBoundaryCandidate::Element {
                        width: element_width,
                        ..
                    } => {
                        if prev_c == ' ' && first_non_whitespace_ix.is_some() {
                            last_candidate_ix = ix;
                            last_candidate_width = width;
                        }

                        if first_non_whitespace_ix.is_none() {
                            first_non_whitespace_ix = Some(ix);
                        }

                        element_width
                    }
                };

                width += item_width;
                // The candidate is on the line whether or not it wraps it: a word after a
                // space that overflowed must still see that space, or it is no candidate
                // and the next line breaks inside it (as the shaped layout never does).
                prev_c = new_prev_c;
                if width > wrap_width && ix > last_wrap_ix {
                    if let (None, Some(first_non_whitespace_ix)) = (indent, first_non_whitespace_ix)
                    {
                        indent = Some(
                            Self::MAX_INDENT.min((first_non_whitespace_ix - last_wrap_ix) as u32),
                        );
                    }

                    if last_candidate_ix > 0 {
                        last_wrap_ix = last_candidate_ix;
                        width -= last_candidate_width;
                        last_candidate_ix = 0;
                    } else {
                        last_wrap_ix = ix;
                        width = item_width;
                    }

                    if let Some(indent) = indent {
                        width += self.width_for_char(' ') * indent as f32;
                    }

                    return Some(Boundary::new(last_wrap_ix, indent.unwrap_or(0)));
                }
            }

            None
        })
    }

    /// Truncate a line of text to the given width with this wrapper's font and font size.
    pub fn truncate_line(
        &mut self,
        line: SharedString,
        truncate_width: Pixels,
        truncation_suffix: &str,
        runs: &mut Vec<TextRun>,
    ) -> SharedString {
        self.truncate_line_from(line, 0, truncate_width, truncation_suffix, runs)
    }

    /// Replace the middle of a line that does not fit with `ellipsis`, keeping both ends.
    pub fn truncate_line_middle(
        &mut self,
        line: SharedString,
        truncate_width: Pixels,
        ellipsis: &str,
        runs: &mut Vec<TextRun>,
    ) -> SharedString {
        self.truncate_middle_from(line, 0, truncate_width, ellipsis, runs)
    }

    /// Truncate the last of `max_lines` wrapped lines, so that a clamped text ends with the
    /// suffix inside its width. Truncating the whole text to `max_lines` widths instead lets
    /// the last line overflow by whatever the earlier lines left unused at a word boundary.
    /// With `keep_end` the suffix replaces the middle of that last line instead of its end.
    pub fn truncate_clamped_line(
        &mut self,
        line: SharedString,
        wrap_width: Pixels,
        max_lines: usize,
        truncation_suffix: &str,
        keep_end: bool,
        runs: &mut Vec<TextRun>,
    ) -> SharedString {
        let last_line_start = if max_lines <= 1 {
            Some(0)
        } else {
            self.wrap_line(&[LineFragment::text(&line)], wrap_width)
                .nth(max_lines - 2)
                .map(|boundary| boundary.ix)
        };
        match last_line_start {
            Some(start) if keep_end => {
                self.truncate_middle_from(line, start, wrap_width, truncation_suffix, runs)
            }
            Some(start) => {
                self.truncate_line_from(line, start, wrap_width, truncation_suffix, runs)
            }
            // Fewer lines than allowed: nothing to cut.
            None => line,
        }
    }

    /// Elide the middle of `line[start..]`: the space left by the ellipsis is shared evenly
    /// between the two ends, and neither end keeps whitespace next to the ellipsis.
    fn truncate_middle_from(
        &mut self,
        line: SharedString,
        start: usize,
        truncate_width: Pixels,
        ellipsis: &str,
        runs: &mut Vec<TextRun>,
    ) -> SharedString {
        let widths: Vec<(usize, char, Pixels)> = line[start..]
            .char_indices()
            .map(|(ix, c)| (start + ix, c, self.width_for_char(c)))
            .collect();
        let total = widths.iter().fold(px(0.), |sum, (.., width)| sum + *width);
        if total.floor() <= truncate_width {
            return line;
        }
        let ellipsis_width = ellipsis
            .chars()
            .map(|c| self.width_for_char(c))
            .fold(px(0.), |a, x| a + x);
        let budget = (truncate_width - ellipsis_width).max(px(0.));

        let mut tail_width = px(0.);
        let mut tail_start = line.len();
        for (ix, _, width) in widths.iter().rev() {
            if tail_width + *width > budget / 2. {
                break;
            }
            tail_width += *width;
            tail_start = *ix;
        }
        let mut head_width = px(0.);
        let mut head_end = start;
        for (ix, c, width) in &widths {
            if *ix >= tail_start || head_width + *width > budget - tail_width {
                break;
            }
            head_width += *width;
            head_end = ix + c.len_utf8();
        }

        let head = line[..head_end].trim_end();
        let tail = line[tail_start..].trim_start();
        let removed = head.len()..line.len() - tail.len();
        let result = SharedString::from(format!("{head}{ellipsis}{tail}"));
        update_runs_after_elision(removed, ellipsis.len(), runs);
        result
    }

    fn truncate_line_from(
        &mut self,
        line: SharedString,
        start: usize,
        truncate_width: Pixels,
        truncation_suffix: &str,
        runs: &mut Vec<TextRun>,
    ) -> SharedString {
        let mut width = px(0.);
        let mut suffix_width = truncation_suffix
            .chars()
            .map(|c| self.width_for_char(c))
            .fold(px(0.0), |a, x| a + x);
        let mut char_indices = line.char_indices();
        let mut truncate_ix = start;
        for (ix, c) in char_indices {
            if ix < start {
                continue;
            }
            if width + suffix_width < truncate_width {
                truncate_ix = ix;
            }

            let char_width = self.width_for_char(c);
            width += char_width;

            if width.floor() > truncate_width {
                // No space before the suffix: "three …" reads as three full stops.
                let kept = line[..truncate_ix].trim_end();
                let result = SharedString::from(format!("{kept}{truncation_suffix}"));
                update_runs_after_truncation(&result, truncation_suffix, runs);

                return result;
            }
        }

        line
    }

    /// Any character in this list should be treated as a word character,
    /// meaning it can be part of a word that should not be wrapped.
    pub(crate) fn is_word_char(c: char) -> bool {
        // ASCII alphanumeric characters, for English, numbers: `Hello123`, etc.
        c.is_ascii_alphanumeric() ||
        // Latin script in Unicode for French, German, Spanish, etc.
        // Latin-1 Supplement
        // https://en.wikipedia.org/wiki/Latin-1_Supplement
        matches!(c, '\u{00C0}'..='\u{00FF}') ||
        // Latin Extended-A
        // https://en.wikipedia.org/wiki/Latin_Extended-A
        matches!(c, '\u{0100}'..='\u{017F}') ||
        // Latin Extended-B
        // https://en.wikipedia.org/wiki/Latin_Extended-B
        matches!(c, '\u{0180}'..='\u{024F}') ||
        // Cyrillic for Russian, Ukrainian, etc.
        // https://en.wikipedia.org/wiki/Cyrillic_script_in_Unicode
        matches!(c, '\u{0400}'..='\u{04FF}') ||
        // Some other known special characters that should be treated as word characters,
        // e.g. `a-b`, `var_name`, `I'm`, '@mention`, `#hashtag`, `100%`, `3.1415`,
        // `2^3`, `a~b`, `a=1`, `Self::new`, etc.
        matches!(c, '-' | '_' | '.' | '\'' | '$' | '%' | '@' | '#' | '^' | '~' | ',' | '=' | ':') ||
        // `⋯` character is special used in Zed, to keep this at the end of the line.
        // `…` is the ellipsis GPUI truncates with (`Styled::truncate`): it stands for
        // letters taken out, so it is never a place to break either.
        matches!(c, '⋯' | '…')
    }

    #[inline(always)]
    fn width_for_char(&mut self, c: char) -> Pixels {
        if (c as u32) < 128 {
            if let Some(cached_width) = self.cached_ascii_char_widths[c as usize] {
                cached_width
            } else {
                let width = self.compute_width_for_char(c);
                self.cached_ascii_char_widths[c as usize] = Some(width);
                width
            }
        } else if let Some(cached_width) = self.cached_other_char_widths.get(&c) {
            *cached_width
        } else {
            let width = self.compute_width_for_char(c);
            self.cached_other_char_widths.insert(c, width);
            width
        }
    }

    fn compute_width_for_char(&self, c: char) -> Pixels {
        let mut buffer = [0; 4];
        let buffer = c.encode_utf8(&mut buffer);
        self.platform_text_system
            .layout_line(
                buffer,
                self.font_size,
                &[FontRun {
                    len: buffer.len(),
                    font_id: self.font_id,
                    position: crate::RunPosition::Baseline,
                }],
            )
            .width
    }
}

fn update_runs_after_truncation(result: &str, ellipsis: &str, runs: &mut Vec<TextRun>) {
    let mut truncate_at = result.len() - ellipsis.len();
    for (run_index, run) in runs.iter_mut().enumerate() {
        if run.len <= truncate_at {
            truncate_at -= run.len;
        } else {
            run.len = truncate_at + ellipsis.len();
            runs.truncate(run_index + 1);
            break;
        }
    }
}

/// `removed` bytes of the text became `inserted` bytes: shrink the runs over them, and give the
/// inserted bytes to the run the removal began in.
fn update_runs_after_elision(
    removed: std::ops::Range<usize>,
    inserted: usize,
    runs: &mut Vec<TextRun>,
) {
    let mut offset = 0;
    let mut inserted = Some(inserted);
    for run in runs.iter_mut() {
        let end = offset + run.len;
        let overlap = end.min(removed.end).saturating_sub(offset.max(removed.start));
        if removed.start < end || removed.start == offset {
            run.len += inserted.take().unwrap_or(0);
        }
        run.len -= overlap;
        offset = end;
    }
    if let (Some(inserted), Some(last)) = (inserted, runs.last_mut()) {
        last.len += inserted;
    }
    runs.retain(|run| run.len > 0);
}

/// A fragment of a line that can be wrapped.
pub enum LineFragment<'a> {
    /// A text fragment consisting of characters.
    Text {
        /// The text content of the fragment.
        text: &'a str,
    },
    /// A non-text element with a fixed width.
    Element {
        /// The width of the element in pixels.
        width: Pixels,
        /// The UTF-8 encoded length of the element.
        len_utf8: usize,
    },
}

impl<'a> LineFragment<'a> {
    /// Creates a new text fragment from the given text.
    pub fn text(text: &'a str) -> Self {
        LineFragment::Text { text }
    }

    /// Creates a new non-text element with the given width and UTF-8 encoded length.
    pub fn element(width: Pixels, len_utf8: usize) -> Self {
        LineFragment::Element { width, len_utf8 }
    }

    fn wrap_boundary_candidates(&self) -> impl Iterator<Item = WrapBoundaryCandidate> {
        let text = match self {
            LineFragment::Text { text } => text,
            LineFragment::Element { .. } => "\0",
        };
        text.chars().map(move |character| {
            if let LineFragment::Element { width, len_utf8 } = self {
                WrapBoundaryCandidate::Element {
                    width: *width,
                    len_utf8: *len_utf8,
                }
            } else {
                WrapBoundaryCandidate::Char { character }
            }
        })
    }
}

enum WrapBoundaryCandidate {
    Char { character: char },
    Element { width: Pixels, len_utf8: usize },
}

impl WrapBoundaryCandidate {
    pub fn len_utf8(&self) -> usize {
        match self {
            WrapBoundaryCandidate::Char { character } => character.len_utf8(),
            WrapBoundaryCandidate::Element { len_utf8: len, .. } => *len,
        }
    }
}

/// A boundary between two lines of text.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Boundary {
    /// The index of the last character in a line
    pub ix: usize,
    /// The indent of the next line.
    pub next_indent: u32,
}

impl Boundary {
    fn new(ix: usize, next_indent: u32) -> Self {
        Self { ix, next_indent }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Font, FontFeatures, FontStyle, FontWeight, Hsla, TestAppContext, TestDispatcher, font,
    };
    #[cfg(target_os = "macos")]
    use crate::{TextRun, WindowTextSystem, WrapBoundary};
    use rand::prelude::*;

    fn build_wrapper() -> LineWrapper {
        let dispatcher = TestDispatcher::new(StdRng::seed_from_u64(0));
        let cx = TestAppContext::build(dispatcher, None);
        let id = cx.text_system().resolve_font(&font(".ZedMono"));
        LineWrapper::new(id, px(16.), cx.text_system().platform_text_system.clone())
    }

    fn generate_test_runs(input_run_len: &[usize]) -> Vec<TextRun> {
        input_run_len
            .iter()
            .map(|run_len| TextRun {
                len: *run_len,
                font: Font {
                    family: "Dummy".into(),
                    features: FontFeatures::default(),
                    fallbacks: None,
                    weight: FontWeight::default(),
                    style: FontStyle::Normal,
                },
                color: Hsla::default(),
                background_color: None,
                underline: None,
                strikethrough: None,
                position: Default::default(),
            })
            .collect()
    }

    #[test]
    fn test_wrap_line() {
        let mut wrapper = build_wrapper();

        assert_eq!(
            wrapper
                .wrap_line(&[LineFragment::text("aa bbb cccc ddddd eeee")], px(72.))
                .collect::<Vec<_>>(),
            &[
                Boundary::new(7, 0),
                Boundary::new(12, 0),
                Boundary::new(18, 0)
            ],
        );
        // The space after "bbbb" overflows the first line: "ccccc" still follows a space,
        // so the second line breaks before it, not inside it.
        assert_eq!(
            wrapper
                .wrap_line(&[LineFragment::text("aa bbbb ccccc")], px(72.))
                .collect::<Vec<_>>(),
            &[Boundary::new(3, 0), Boundary::new(8, 0)],
        );
        // An ellipsis that overflows the line is no place to break: the word it is in
        // goes to the next line whole, as a clamped text's truncation placed it.
        assert_eq!(
            wrapper
                .wrap_line(&[LineFragment::text("aa bbbb cc…zip")], px(72.))
                .collect::<Vec<_>>(),
            &[Boundary::new(3, 0), Boundary::new(8, 0)],
        );
        assert_eq!(
            wrapper
                .wrap_line(&[LineFragment::text("aaa aaaaaaaaaaaaaaaaaa")], px(72.0))
                .collect::<Vec<_>>(),
            &[
                Boundary::new(4, 0),
                Boundary::new(11, 0),
                Boundary::new(18, 0)
            ],
        );
        assert_eq!(
            wrapper
                .wrap_line(&[LineFragment::text("     aaaaaaa")], px(72.))
                .collect::<Vec<_>>(),
            &[
                Boundary::new(7, 5),
                Boundary::new(9, 5),
                Boundary::new(11, 5),
            ]
        );
        assert_eq!(
            wrapper
                .wrap_line(
                    &[LineFragment::text("                            ")],
                    px(72.)
                )
                .collect::<Vec<_>>(),
            &[
                Boundary::new(7, 0),
                Boundary::new(14, 0),
                Boundary::new(21, 0)
            ]
        );
        assert_eq!(
            wrapper
                .wrap_line(&[LineFragment::text("          aaaaaaaaaaaaaa")], px(72.))
                .collect::<Vec<_>>(),
            &[
                Boundary::new(7, 0),
                Boundary::new(14, 3),
                Boundary::new(18, 3),
                Boundary::new(22, 3),
            ]
        );

        // Test wrapping multiple text fragments
        assert_eq!(
            wrapper
                .wrap_line(
                    &[
                        LineFragment::text("aa bbb "),
                        LineFragment::text("cccc ddddd eeee")
                    ],
                    px(72.)
                )
                .collect::<Vec<_>>(),
            &[
                Boundary::new(7, 0),
                Boundary::new(12, 0),
                Boundary::new(18, 0)
            ],
        );

        // Test wrapping with a mix of text and element fragments
        assert_eq!(
            wrapper
                .wrap_line(
                    &[
                        LineFragment::text("aa "),
                        LineFragment::element(px(20.), 1),
                        LineFragment::text(" bbb "),
                        LineFragment::element(px(30.), 1),
                        LineFragment::text(" cccc")
                    ],
                    px(72.)
                )
                .collect::<Vec<_>>(),
            &[
                Boundary::new(5, 0),
                Boundary::new(9, 0),
                Boundary::new(11, 0)
            ],
        );

        // Test with element at the beginning and text afterward
        assert_eq!(
            wrapper
                .wrap_line(
                    &[
                        LineFragment::element(px(50.), 1),
                        LineFragment::text(" aaaa bbbb cccc dddd")
                    ],
                    px(72.)
                )
                .collect::<Vec<_>>(),
            &[
                Boundary::new(2, 0),
                Boundary::new(7, 0),
                Boundary::new(12, 0),
                Boundary::new(17, 0)
            ],
        );

        // Test with a large element that forces wrapping by itself
        assert_eq!(
            wrapper
                .wrap_line(
                    &[
                        LineFragment::text("short text "),
                        LineFragment::element(px(100.), 1),
                        LineFragment::text(" more text")
                    ],
                    px(72.)
                )
                .collect::<Vec<_>>(),
            &[
                Boundary::new(6, 0),
                Boundary::new(11, 0),
                Boundary::new(12, 0),
                Boundary::new(18, 0)
            ],
        );
    }

    #[test]
    fn test_truncate_middle() {
        let mut wrapper = build_wrapper();
        let mut elide = |text: &'static str, run_lens: &[usize], lines: usize| {
            let mut runs = generate_test_runs(run_lens);
            let result =
                wrapper.truncate_clamped_line(text.into(), px(72.), lines, "…", true, &mut runs);
            assert_eq!(runs.iter().map(|run| run.len).sum::<usize>(), result.len());
            (result, runs.iter().map(|run| run.len).collect::<Vec<_>>())
        };
        // One line of 72 px holds seven characters: three, the ellipsis, three.
        assert_eq!(elide("aaaaaaaaaaaa.zip", &[16], 1).0, "aaa…zip");
        // Two lines: the first is untouched and the second keeps the end.
        assert_eq!(elide("aa bbb cccc ddddd.zip", &[21], 2).0, "aa bbb ccc…zip");
        // No whitespace beside the ellipsis.
        assert_eq!(elide("aa bbb cc  ddddd e.zip", &[22], 2).0, "aa bbb cc…zip");
        // The last line starts where the layout starts it, after a space that overflowed
        // the line above: elided from inside "ccccc", it was wider than its line.
        assert_eq!(
            elide("aa bbbb ccccc dddddddd.zip", &[26], 3).0,
            "aa bbbb ccc…zip"
        );
        // It fits: untouched, and so are its runs.
        assert_eq!(elide("aa bbb cccc", &[3, 8], 2), ("aa bbb cccc".into(), vec![3, 8]));
        // The runs lose what was removed; the ellipsis joins the run it began in.
        assert_eq!(
            elide("aa bbb cccc ddddd.zip", &[7, 6, 8], 2),
            ("aa bbb ccc…zip".into(), vec![7, 3 + "…".len(), 3])
        );
        assert_eq!(
            wrapper.truncate_line_middle("aaaaaaaaaaaa.zip".into(), px(72.), "…", &mut vec![]),
            "aaa…zip"
        );
    }

    #[test]
    fn test_truncate_clamped_line() {
        let mut wrapper = build_wrapper();
        let mut clamp = |text: &'static str, lines: usize| {
            let mut runs = generate_test_runs(&[text.len()]);
            let result = wrapper
                .truncate_clamped_line(text.into(), px(72.), lines, "…", false, &mut runs);
            assert_eq!(runs.first().unwrap().len, result.len());
            result
        };
        // Wrapped at 72 px this is "aa bbb ", "cccc ", "ddddd ", "eeee": the first line
        // leaves room unused, which the last line must not inherit.
        assert_eq!(clamp("aa bbb cccc ddddd eeee", 2), "aa bbb cccc d…");
        assert_eq!(clamp("aa bbb cccc ddddd eeee", 1), "aa bbb…");
        // Cut right after a space: the suffix follows the word, not the space.
        assert_eq!(clamp("aa bbb ccccc dd", 2), "aa bbb ccccc…");
        // It fits in the lines it is allowed: untouched.
        assert_eq!(clamp("aa bbb cccc ddddd eeee", 4), "aa bbb cccc ddddd eeee");
        assert_eq!(clamp("aa bbb cccc", 2), "aa bbb cccc");
        assert_eq!(clamp("aa", 2), "aa");
    }

    #[test]
    fn test_truncate_line() {
        let mut wrapper = build_wrapper();

        fn perform_test(
            wrapper: &mut LineWrapper,
            text: &'static str,
            result: &'static str,
            ellipsis: &str,
        ) {
            let dummy_run_lens = vec![text.len()];
            let mut dummy_runs = generate_test_runs(&dummy_run_lens);
            assert_eq!(
                wrapper.truncate_line(text.into(), px(220.), ellipsis, &mut dummy_runs),
                result
            );
            assert_eq!(dummy_runs.first().unwrap().len, result.len());
        }

        perform_test(
            &mut wrapper,
            "aa bbb cccc ddddd eeee ffff gggg",
            "aa bbb cccc ddddd eeee",
            "",
        );
        perform_test(
            &mut wrapper,
            "aa bbb cccc ddddd eeee ffff gggg",
            "aa bbb cccc ddddd eee…",
            "…",
        );
        perform_test(
            &mut wrapper,
            "aa bbb cccc ddddd eeee ffff gggg",
            "aa bbb cccc dddd......",
            "......",
        );
    }

    #[test]
    fn test_truncate_multiple_runs() {
        let mut wrapper = build_wrapper();

        fn perform_test(
            wrapper: &mut LineWrapper,
            text: &'static str,
            result: &str,
            run_lens: &[usize],
            result_run_len: &[usize],
            line_width: Pixels,
        ) {
            let mut dummy_runs = generate_test_runs(run_lens);
            assert_eq!(
                wrapper.truncate_line(text.into(), line_width, "…", &mut dummy_runs),
                result
            );
            for (run, result_len) in dummy_runs.iter().zip(result_run_len) {
                assert_eq!(run.len, *result_len);
            }
        }
        // Case 0: Normal
        // Text: abcdefghijkl
        // Runs: Run0 { len: 12, ... }
        //
        // Truncate res: abcd… (truncate_at = 4)
        // Run res: Run0 { string: abcd…, len: 7, ... }
        perform_test(&mut wrapper, "abcdefghijkl", "abcd…", &[12], &[7], px(50.));
        // Case 1: Drop some runs
        // Text: abcdefghijkl
        // Runs: Run0 { len: 4, ... }, Run1 { len: 4, ... }, Run2 { len: 4, ... }
        //
        // Truncate res: abcdef… (truncate_at = 6)
        // Runs res: Run0 { string: abcd, len: 4, ... }, Run1 { string: ef…, len:
        // 5, ... }
        perform_test(
            &mut wrapper,
            "abcdefghijkl",
            "abcdef…",
            &[4, 4, 4],
            &[4, 5],
            px(70.),
        );
        // Case 2: Truncate at start of some run
        // Text: abcdefghijkl
        // Runs: Run0 { len: 4, ... }, Run1 { len: 4, ... }, Run2 { len: 4, ... }
        //
        // Truncate res: abcdefgh… (truncate_at = 8)
        // Runs res: Run0 { string: abcd, len: 4, ... }, Run1 { string: efgh, len:
        // 4, ... }, Run2 { string: …, len: 3, ... }
        perform_test(
            &mut wrapper,
            "abcdefghijkl",
            "abcdefgh…",
            &[4, 4, 4],
            &[4, 4, 3],
            px(90.),
        );
    }

    #[test]
    fn test_update_run_after_truncation() {
        fn perform_test(result: &str, run_lens: &[usize], result_run_lens: &[usize]) {
            let mut dummy_runs = generate_test_runs(run_lens);
            update_runs_after_truncation(result, "…", &mut dummy_runs);
            for (run, result_len) in dummy_runs.iter().zip(result_run_lens) {
                assert_eq!(run.len, *result_len);
            }
        }
        // Case 0: Normal
        // Text: abcdefghijkl
        // Runs: Run0 { len: 12, ... }
        //
        // Truncate res: abcd… (truncate_at = 4)
        // Run res: Run0 { string: abcd…, len: 7, ... }
        perform_test("abcd…", &[12], &[7]);
        // Case 1: Drop some runs
        // Text: abcdefghijkl
        // Runs: Run0 { len: 4, ... }, Run1 { len: 4, ... }, Run2 { len: 4, ... }
        //
        // Truncate res: abcdef… (truncate_at = 6)
        // Runs res: Run0 { string: abcd, len: 4, ... }, Run1 { string: ef…, len:
        // 5, ... }
        perform_test("abcdef…", &[4, 4, 4], &[4, 5]);
        // Case 2: Truncate at start of some run
        // Text: abcdefghijkl
        // Runs: Run0 { len: 4, ... }, Run1 { len: 4, ... }, Run2 { len: 4, ... }
        //
        // Truncate res: abcdefgh… (truncate_at = 8)
        // Runs res: Run0 { string: abcd, len: 4, ... }, Run1 { string: efgh, len:
        // 4, ... }, Run2 { string: …, len: 3, ... }
        perform_test("abcdefgh…", &[4, 4, 4], &[4, 4, 3]);
    }

    #[test]
    fn test_is_word_char() {
        #[track_caller]
        fn assert_word(word: &str) {
            for c in word.chars() {
                assert!(LineWrapper::is_word_char(c), "assertion failed for '{}'", c);
            }
        }

        #[track_caller]
        fn assert_not_word(word: &str) {
            let found = word.chars().any(|c| !LineWrapper::is_word_char(c));
            assert!(found, "assertion failed for '{}'", word);
        }

        assert_word("Hello123");
        assert_word("non-English");
        assert_word("var_name");
        assert_word("123456");
        assert_word("3.1415");
        assert_word("10^2");
        assert_word("1~2");
        assert_word("100%");
        assert_word("@mention");
        assert_word("#hashtag");
        assert_word("$variable");
        assert_word("a=1");
        assert_word("Self::is_word_char");
        assert_word("more⋯");
        assert_word("aa…zip");

        // Space
        assert_not_word("foo bar");

        // URL case
        assert_word("github.com");
        assert_not_word("zed-industries/zed");
        assert_not_word("zed-industries\\zed");
        assert_not_word("a=1&b=2");
        assert_not_word("foo?b=2");

        // Latin-1 Supplement
        assert_word("ÀÁÂÃÄÅÆÇÈÉÊËÌÍÎÏ");
        // Latin Extended-A
        assert_word("ĀāĂăĄąĆćĈĉĊċČčĎď");
        // Latin Extended-B
        assert_word("ƀƁƂƃƄƅƆƇƈƉƊƋƌƍƎƏ");
        // Cyrillic
        assert_word("АБВГДЕЖЗИЙКЛМНОП");

        // non-word characters
        assert_not_word("你好");
        assert_not_word("안녕하세요");
        assert_not_word("こんにちは");
        assert_not_word("😀😁😂");
        assert_not_word("()[]{}<>");
    }

    // For compatibility with the test macro
    #[cfg(target_os = "macos")]
    use crate as gpui;

    // These seem to vary wildly based on the text system.
    #[cfg(target_os = "macos")]
    #[crate::test]
    fn test_wrap_shaped_line(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let text_system = WindowTextSystem::new(cx.text_system().clone());

            let normal = TextRun {
                len: 0,
                font: font("Helvetica"),
                color: Default::default(),
                underline: Default::default(),
                strikethrough: None,
                background_color: None,
                position: Default::default(),
            };
            let bold = TextRun {
                len: 0,
                font: font("Helvetica").bold(),
                color: Default::default(),
                underline: Default::default(),
                strikethrough: None,
                background_color: None,
                position: Default::default(),
            };

            let text = "aa bbb cccc ddddd eeee".into();
            let lines = text_system
                .shape_text(
                    text,
                    px(16.),
                    &[
                        normal.with_len(4),
                        bold.with_len(5),
                        normal.with_len(6),
                        bold.with_len(1),
                        normal.with_len(7),
                    ],
                    Some(px(72.)),
                    None,
                )
                .unwrap();

            assert_eq!(
                lines[0].layout.wrap_boundaries(),
                &[
                    WrapBoundary {
                        run_ix: 0,
                        glyph_ix: 7
                    },
                    WrapBoundary {
                        run_ix: 0,
                        glyph_ix: 12
                    },
                    WrapBoundary {
                        run_ix: 0,
                        glyph_ix: 18
                    }
                ],
            );
        });
    }
}
