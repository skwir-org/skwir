//! Virtual source lines. Fold coordinates stay in the provider's logical lines.
use crate::{inspector_text::Selection, shell::Shell, theme::Theme};
use bibli_core::{
    inspection::{Inspection, Run},
    workspace::PaneId,
};
use gpui::{AnyElement, WeakEntity, div, prelude::*, px};
use std::collections::BTreeSet;

/// Keep the highlighter's parser state across lines (e.g. multiline strings).
/// Runs on the preview conversion worker, before any UI rows are created.
pub fn highlight(data: &mut Inspection, dark: bool) {
    let language = data.blocks.first().and_then(|b| b.language.as_deref());
    let syntaxes = crate::preview::syntaxes(language);
    let syntax = language
        .and_then(|language| syntaxes.find_syntax_by_token(language))
        .unwrap_or_else(|| syntaxes.find_syntax_plain_text());
    let theme = &crate::preview::themes().themes[if dark {
        "base16-ocean.dark"
    } else {
        "base16-ocean.light"
    }];
    let mut highlight = syntect::easy::HighlightLines::new(syntax, theme);
    for block in &mut data.blocks {
        block.runs.clear();
        // The grammar needs line endings, but selection and copy use the line's
        // original text and the block separator. Never expose this extra byte.
        if let Ok(ranges) = highlight.highlight_line(&format!("{}\n", block.text), syntaxes) {
            let mut offset = 0;
            for (style, text) in ranges {
                let end = (offset + text.len()).min(block.text.len());
                if offset < end {
                    block.runs.push(Run {
                        range: offset..end,
                        color: Some([style.foreground.r, style.foreground.g, style.foreground.b]),
                        ..Run::default()
                    });
                }
                offset += text.len();
            }
        }
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
            pane,
            entity,
        } = self;
        let style = crate::markdown::MarkdownStyle::new(theme, font_size, false);
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
            .when(selected == Some(line), |d| {
                d.bg(theme.selection_bg.opacity(0.5))
            })
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
                        &style,
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
