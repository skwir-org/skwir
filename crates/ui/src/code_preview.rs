//! Virtual source lines. Fold coordinates stay in the provider's logical lines.
use crate::{inspector_text::Selection, shell::Shell, theme::Theme};
use bibli_core::{
    inspection::{Inspection, Run},
    workspace::PaneId,
};
use gpui::{AnyElement, WeakEntity, div, prelude::*, px};
use std::collections::BTreeSet;
use std::sync::OnceLock;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;

/// bat's grammars, syntect's defaults among them: read once, each
/// grammar compiled when a file first needs it.
pub(crate) fn syntaxes() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(two_face::syntax::extra_newlines)
}

fn themes() -> &'static ThemeSet {
    static T: OnceLock<ThemeSet> = OnceLock::new();
    T.get_or_init(ThemeSet::load_defaults)
}

/// The one syntax highlighter, for the code view and for a document's code
/// blocks alike. It runs on the worker that builds the `Display` and keeps
/// the grammar's state from line to line (a multiline string, a comment).
pub(crate) struct Highlighter {
    syntaxes: &'static SyntaxSet,
    lines: syntect::easy::HighlightLines<'static>,
}

impl Highlighter {
    pub fn new(language: Option<&str>, dark: bool) -> Self {
        let syntaxes = syntaxes();
        let syntax = language
            .and_then(|language| syntaxes.find_syntax_by_token(language))
            .unwrap_or_else(|| syntaxes.find_syntax_plain_text());
        let theme = &themes().themes[if dark {
            "base16-ocean.dark"
        } else {
            "base16-ocean.light"
        }];
        Self {
            syntaxes,
            lines: syntect::easy::HighlightLines::new(syntax, theme),
        }
    }

    /// The coloured runs of the next line, `text` without its line ending,
    /// in the line's own coordinates. The grammar needs the ending, but
    /// selection and copy use the text as it is: the extra byte never shows.
    pub fn line(&mut self, text: &str) -> Vec<Run> {
        let mut runs = Vec::new();
        if let Ok(ranges) = self
            .lines
            .highlight_line(&format!("{text}\n"), self.syntaxes)
        {
            let mut offset = 0;
            for (style, part) in ranges {
                let end = (offset + part.len()).min(text.len());
                if offset < end {
                    runs.push(Run {
                        range: offset..end,
                        color: Some([style.foreground.r, style.foreground.g, style.foreground.b]),
                        ..Run::default()
                    });
                }
                offset += part.len();
            }
        }
        runs
    }

    /// The runs of a block of several lines, in the block's coordinates.
    pub fn text(&mut self, text: &str) -> Vec<Run> {
        let mut runs = Vec::new();
        let mut offset = 0;
        for line in text.split('\n') {
            runs.extend(self.line(line).into_iter().map(|mut run| {
                run.range = run.range.start + offset..run.range.end + offset;
                run
            }));
            offset += line.len() + 1;
        }
        runs
    }
}

/// Highlight the code view: one block per line, parser state kept between
/// them, and the addresses each line writes found as links, followed with
/// Ctrl (`Run::found`). Runs on the preview conversion worker, before any UI
/// rows exist.
pub fn highlight(data: &mut Inspection, dark: bool) {
    let language = data.blocks.first().and_then(|b| b.language.clone());
    let mut highlighter = Highlighter::new(language.as_deref(), dark);
    for block in &mut data.blocks {
        block.runs = highlighter.line(&block.text);
        bibli_platform::links::link_block(block, true);
    }
}

pub fn visible(data: &Inspection, collapsed: &BTreeSet<usize>) -> Vec<usize> {
    let mut hidden_until = 0;
    let mut folds = data.folds.iter().peekable();
    (0..data.blocks.len())
        .filter(|&line| {
            while folds.peek().is_some_and(|fold| fold.start < line) {
                folds.next();
            }
            if line < hidden_until {
                return false;
            }
            if let Some(fold) = folds.peek().filter(|fold| fold.start == line)
                && collapsed.contains(&line)
            {
                hidden_until = fold.end.min(data.blocks.len());
            }
            true
        })
        .collect()
}

pub struct CodeRow<'a> {
    pub data: &'a Inspection,
    pub line: usize,
    pub collapsed: &'a BTreeSet<usize>,
    pub selected: Option<usize>,
    pub selection: Option<Selection>,
    pub font_size: f32,
    pub mono: &'a gpui::SharedString,
    pub theme: &'a Theme,
    /// The inspector's style for this frame, built once, not per row.
    pub style: &'a crate::markdown::MarkdownStyle,
    pub pane: PaneId,
    pub entity: WeakEntity<Shell>,
}
impl CodeRow<'_> {
    pub fn render(self) -> AnyElement {
        let Self {
            data,
            line,
            collapsed,
            selected,
            selection,
            font_size,
            mono,
            theme,
            style,
            pane,
            entity,
        } = self;
        let fold = data
            .folds
            .binary_search_by_key(&line, |fold| fold.start)
            .ok()
            .map(|i| &data.folds[i]);
        let folded = collapsed.contains(&line);
        let mut gutter = div()
            .id(("code-fold", line))
            .flex_none()
            .flex()
            .items_center()
            .w(px(68.))
            .pr_2()
            .gap_1()
            .text_color(theme.text_muted)
            .child(div().w(px(42.)).text_right().child((line + 1).to_string()))
            .child(div().w(px(14.)).child(if fold.is_some() {
                if folded { "▸" } else { "▾" }
            } else {
                ""
            }));
        if fold.is_some() {
            let weak = entity.clone();
            gutter = gutter
                .cursor_pointer()
                .hover(|d| d.bg(theme.hover))
                .on_click(move |_, window, cx| {
                    let _ = weak.update(cx, |s, cx| {
                        s.with_pane(pane, |s| {
                            s.run(
                                "ui.inspect",
                                serde_json::json!({"action":"collapse", "index":line}),
                                window,
                                cx,
                            );
                        });
                    });
                });
        }
        div()
            .id(("code-line", line))
            .w_full()
            .flex()
            .items_start()
            .font_family(mono.clone())
            .text_size(px(font_size - 2.))
            .line_height(px(font_size + 4.))
            .when(selected == Some(line), |d| d.bg(theme.highlight))
            .child(gutter)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .pr_2()
                    .child(crate::inspector_text::render(
                        &data.blocks[line],
                        line,
                        selection,
                        style,
                        pane,
                        entity,
                    )),
            )
            .when(folded && fold.is_some(), |d| {
                d.child(
                    div()
                        .flex_none()
                        .px_2()
                        .rounded_sm()
                        .bg(theme.code_bg)
                        .text_color(theme.text_muted)
                        .child(format!(
                            "… {} lines",
                            fold.map_or(0, |r| r.end - r.start - 1)
                        )),
                )
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bibli_core::inspection::{Block, InspectionKind};

    #[test]
    fn nested_folds_retain_siblings_and_remember_child_state() {
        let data = Inspection {
            kind: InspectionKind::Code,
            blocks: vec![Block::default(); 10],
            folds: vec![0..7, 2..5, 7..9],
            ..Inspection::default()
        };
        assert_eq!(
            visible(&data, &BTreeSet::from([2])),
            [0, 1, 2, 5, 6, 7, 8, 9]
        );
        assert_eq!(visible(&data, &BTreeSet::from([0, 2])), [0, 7, 8, 9]);
        assert_eq!(visible(&data, &BTreeSet::from([0, 2, 7])), [0, 7, 9]);
        assert_eq!(
            visible(&data, &BTreeSet::new()),
            (0..10).collect::<Vec<_>>()
        );
    }

    #[test]
    fn everyday_languages_are_highlighted_by_extension_or_name() {
        for (language, line) in [
            ("ts", "const answer: number = 42;"),
            ("tsx", "const answer: number = 42;"),
            ("nix", "{ pkgs ? import <nixpkgs> {} }: pkgs.hello"),
            ("Dockerfile", "FROM alpine:3 AS build"),
            ("kt", "val answer: Int = 42"),
            ("ini", "name = value ; a note"),
            ("typescript", "const answer: number = 42;"),
        ] {
            let runs = Highlighter::new(Some(language), false).line(line);
            let colors: BTreeSet<_> = runs.iter().filter_map(|run| run.color).collect();
            assert!(colors.len() > 1, "{language} is highlighted: {runs:?}");
        }
    }

    #[test]
    fn toml_highlight_keeps_multiline_state_in_both_themes() {
        for dark in [false, true] {
            let mut data = Inspection {
                blocks: [
                    "text = \"\"\"",
                    "# still a string",
                    "\"\"\"",
                    "# a comment",
                    "number = 42",
                ]
                .into_iter()
                .map(|text| Block {
                    text: text.into(),
                    language: Some("toml".into()),
                    ..Block::default()
                })
                .collect(),
                ..Inspection::default()
            };
            highlight(&mut data, dark);
            assert!(data.blocks.iter().all(|b| !b.runs.is_empty()));
            assert_ne!(data.blocks[1].runs[0].color, data.blocks[3].runs[0].color);
            assert!(
                data.blocks[4]
                    .runs
                    .windows(2)
                    .any(|runs| runs[0].color != runs[1].color)
            );
            for block in data.blocks {
                assert!(
                    block
                        .runs
                        .iter()
                        .all(|run| run.range.end <= block.text.len())
                );
            }
        }
    }
}
