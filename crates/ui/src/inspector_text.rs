//! Native rich-text selection. Geometry is read only for pointer hit testing.
use crate::{markdown::MarkdownStyle, shell::Shell};
use bibli_core::{inspection::Block, workspace::PaneId};
use gpui::{
    AnyElement, HighlightStyle, MouseButton, StrikethroughStyle, StyledText, UnderlineStyle,
    WeakEntity, div, prelude::*, px,
};
#[derive(Clone, Copy, Debug, Default)]
pub struct Selection {
    pub start: (usize, usize),
    pub end: (usize, usize),
}
impl Selection {
    pub fn range(self, index: usize, len: usize) -> std::ops::Range<usize> {
        let (start, end) = if self.start <= self.end {
            (self.start, self.end)
        } else {
            (self.end, self.start)
        };
        if index < start.0 || index > end.0 {
            return 0..0;
        }
        (if index == start.0 {
            start.1.min(len)
        } else {
            0
        })..(if index == end.0 { end.1.min(len) } else { len })
    }
    pub fn text(self, blocks: &[Block]) -> Option<String> {
        let parts: Vec<_> = blocks
            .iter()
            .enumerate()
            .filter_map(|(i, b)| {
                let r = self.range(i, b.text.len());
                // Interior empty lines are part of a cross-line selection.
                let (first, last) = (self.start.0.min(self.end.0), self.start.0.max(self.end.0));
                if r.is_empty() && (i < first || i > last || self.start == self.end) {
                    None
                } else {
                    b.text.get(r)
                }
            })
            .collect();
        (!parts.is_empty()).then(|| parts.join("\n"))
    }
}
pub fn render(
    block: &Block,
    index: usize,
    selection: Option<Selection>,
    style: &MarkdownStyle,
    pane: PaneId,
    entity: WeakEntity<Shell>,
) -> AnyElement {
    let whole = 0..block.text.len();
    let id = ("inspection-text", index).into();
    part(block, index, id, whole, selection, style, pane, entity)
}

/// One stretch of a block's text as its own element: the whole block, or a
/// single table cell. Runs, links and the selection are clipped to `range`,
/// and pointer offsets are reported back in the block's own coordinates, so
/// selecting and copying work the same inside a cell as outside one.
pub fn part(
    block: &Block,
    index: usize,
    id: gpui::ElementId,
    range: std::ops::Range<usize>,
    selection: Option<Selection>,
    style: &MarkdownStyle,
    pane: PaneId,
    entity: WeakEntity<Shell>,
) -> AnyElement {
    let selected = selection.map_or(0..0, |s| s.range(index, block.text.len()));
    let selection = selected.start.max(range.start)..selected.end.min(range.end);
    let mut edges = vec![range.start, range.end];
    if selection.start < selection.end {
        edges.extend([selection.start, selection.end]);
    }
    for r in &block.runs {
        edges.extend(
            [r.range.start, r.range.end]
                .into_iter()
                .filter(|edge| range.contains(edge)),
        );
    }
    edges.sort_unstable();
    edges.dedup();
    let highlights = edges
        .windows(2)
        .map(|edge| {
            let start = edge[0];
            let end = edge[1];
            let run = block
                .runs
                .get(block.runs.partition_point(|r| r.range.end <= start))
                .filter(|r| r.range.start <= start);
            let mut highlight = HighlightStyle::default();
            if let Some(r) = run {
                highlight.font_weight = r.bold.then_some(gpui::FontWeight::BOLD);
                highlight.font_style = r.italic.then_some(gpui::FontStyle::Italic);
                highlight.color = r
                    .color
                    .map(|[r, g, b]| {
                        gpui::rgb((u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)).into()
                    })
                    .or_else(|| r.link.as_ref().map(|_| style.link));
                highlight.background_color = r.code.then_some(style.inline_code_bg);
                if r.strikethrough {
                    highlight.strikethrough = Some(StrikethroughStyle {
                        thickness: px(1.),
                        color: Some(style.text_muted),
                    });
                }
                if r.link.is_some() && style.link_underline {
                    highlight.underline = Some(UnderlineStyle {
                        thickness: px(1.),
                        color: Some(style.link),
                        wavy: false,
                    });
                }
            }
            if selection.contains(&start) {
                highlight.background_color = Some(style.selection_bg);
            }
            (start - range.start..end - range.start, highlight)
        })
        .collect::<Vec<_>>();
    let text = StyledText::new(block.text[range.clone()].to_owned()).with_highlights(highlights);
    let down = text.layout().clone();
    let moved = down.clone();
    let links: Vec<_> = block
        .runs
        .iter()
        .filter_map(|r| {
            let clipped = r.range.start.max(range.start)..r.range.end.min(range.end);
            (clipped.start < clipped.end)
                .then(|| r.link.clone())
                .flatten()
                .map(|href| (clipped.start - range.start..clipped.end - range.start, href))
        })
        .collect();
    let ranges = links.iter().map(|(r, _)| r.clone()).collect();
    let weak = entity.clone();
    let offset_of = move |position, layout: &gpui::TextLayout| {
        range.start + layout.index_for_position(position).unwrap_or_else(|i| i)
    };
    let text = gpui::InteractiveText::new(gpui::ElementId::from((id.clone(), "text")), text)
        .on_click(ranges, move |i, window, cx| {
            let _ = weak.update(cx, |s, cx| {
                s.with_pane(pane, |s| {
                    s.run(
                        "ui.inspect",
                        serde_json::json!({"action":"link","target":links[i].1}),
                        window,
                        cx,
                    );
                });
            });
        });
    let press = entity.clone();
    let motion = entity.clone();
    let release = entity.clone();
    div()
        .id(gpui::ElementId::from((id, "select")))
        .cursor_text()
        .child(text)
        .on_mouse_down(MouseButton::Left, move |event, window, cx| {
            let offset = offset_of(event.position, &down);
            let _ = press.update(cx, |s, cx| {
                s.with_pane(pane, |s| {
                    s.run(
                        "ui.inspect",
                        serde_json::json!({"action":"text_down","index":index,"offset":offset}),
                        window,
                        cx,
                    );
                });
            });
        })
        .on_mouse_move(move |event, window, cx| {
            if event.pressed_button == Some(MouseButton::Left) {
                let offset = offset_of(event.position, &moved);
                let _ = motion.update(cx, |s, cx| {
                    s.with_pane(pane, |s| {
                        s.run(
                            "ui.inspect",
                            serde_json::json!({"action":"text_move","index":index,"offset":offset}),
                            window,
                            cx,
                        );
                    });
                });
            }
        })
        .on_mouse_up(MouseButton::Left, move |_, window, cx| {
            let _ = release.update(cx, |s, cx| {
                s.with_pane(pane, |s| {
                    s.run(
                        "ui.inspect",
                        serde_json::json!({"action":"text_up"}),
                        window,
                        cx,
                    );
                });
            });
        })
        .on_mouse_up_out(MouseButton::Left, move |_, window, cx| {
            let _ = entity.update(cx, |s, cx| {
                s.with_pane(pane, |s| {
                    s.run(
                        "ui.inspect",
                        serde_json::json!({"action":"text_up"}),
                        window,
                        cx,
                    );
                });
            });
        })
        .into_any_element()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selection_preserves_blank_lines_and_line_boundaries() {
        let blocks = ["first", "", "last"].map(|text| Block {
            text: text.into(),
            ..Block::default()
        });
        assert_eq!(
            Selection {
                start: (0, 5),
                end: (2, 0)
            }
            .text(&blocks)
            .as_deref(),
            Some("\n\n")
        );
        assert_eq!(
            Selection {
                start: (0, 0),
                end: (2, 4)
            }
            .text(&blocks)
            .as_deref(),
            Some("first\n\nlast")
        );
        assert!(
            Selection {
                start: (0, 0),
                end: (0, 0)
            }
            .text(&blocks)
            .is_none()
        );
    }
    #[test]
    fn a_table_row_selects_like_any_other_line() {
        // Its cells are ranges of one text, so a selection across them needs
        // no special case and copies with the tabs between the columns.
        let blocks = vec![Block {
            kind: bibli_core::inspection::BlockKind::TableRow {
                cells: vec![0..3, 4..8],
                header: false,
            },
            text: "one\ttwo!".into(),
            ..Block::default()
        }];
        let selection = Selection {
            start: (0, 1),
            end: (0, 7),
        };
        assert_eq!(selection.text(&blocks).as_deref(), Some("ne\ttwo"));
    }

    #[test]
    fn reverse_selection_crosses_blocks() {
        let blocks = vec![
            Block {
                text: "First café".into(),
                ..Block::default()
            },
            Block {
                text: "Second".into(),
                ..Block::default()
            },
        ];
        let selection = Selection {
            start: (1, 3),
            end: (0, 6),
        };
        assert_eq!(selection.text(&blocks).as_deref(), Some("café\nSec"));
    }
}
