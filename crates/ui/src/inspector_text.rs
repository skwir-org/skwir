//! Native rich-text selection. Geometry is read only for pointer hit testing.
use crate::elements::rows::{Seen, Spot};
use crate::{markdown::MarkdownStyle, shell::Shell};
use gpui::{
    AnyElement, HighlightStyle, MouseButton, RunPosition, SharedString, StrikethroughStyle,
    StyledText, UnderlineStyle, WeakEntity, div, prelude::*, px,
};
use skwir_core::{
    inspection::{Block, Script},
    workspace::PaneId,
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
/// The text of `range` as drawn, and where it is set in small capitals (in
/// the block's coordinates): a lowercase letter of a small-caps run is drawn
/// as its capital, reduced. Only a letter whose capital is one character of
/// the same length changes, so every offset the view reports (selection,
/// links, the pointer) is still the block's, and copy reads the text itself.
fn small_capitals(
    block: &Block,
    range: std::ops::Range<usize>,
) -> (String, Vec<std::ops::Range<usize>>) {
    let mut shown = block.text[range.clone()].to_owned();
    let mut reduced: Vec<std::ops::Range<usize>> = Vec::new();
    for run in block.runs.iter().filter(|run| run.small_caps) {
        let from = run.range.start.max(range.start);
        let to = run.range.end.min(range.end);
        if from >= to {
            continue;
        }
        for (offset, letter) in block.text[from..to].char_indices() {
            let mut capitals = letter.to_uppercase();
            let (Some(capital), None) = (capitals.next(), capitals.next()) else {
                continue;
            };
            if capital == letter || capital.len_utf8() != letter.len_utf8() {
                continue;
            }
            let at = from + offset;
            let end = at + letter.len_utf8();
            let mut buffer = [0; 4];
            shown.replace_range(
                at - range.start..end - range.start,
                capital.encode_utf8(&mut buffer),
            );
            match reduced.last_mut() {
                Some(last) if last.end == at => last.end = end,
                _ => reduced.push(at..end),
            }
        }
    }
    (shown, reduced)
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

/// What a frame draws over a block's text besides its own style
/// (`links.rs`): the found link Ctrl is held over, in the block's
/// coordinates, and the labels the links carry while they are labelled,
/// each at the start of its link.
#[derive(Clone, Copy, Default)]
pub struct Marks<'a> {
    pub armed: Option<&'a std::ops::Range<usize>>,
    pub labels: &'a [(usize, SharedString)],
    /// What a find found (`Sought::marks`), and whether each is the match
    /// it is on.
    pub found: &'a [(std::ops::Range<usize>, bool)],
}

/// What a find marks in the text a frame draws, while its field is open:
/// every match of what it looks for (`Theme::find_match`), and the one it
/// is on more strongly (`find_current`).
#[derive(Clone)]
pub struct Sought {
    /// What it looks for, as typed: matched as `text_rows::matches` does.
    pub query: String,
    /// The match it is on: its block (or row) and where it is there.
    pub current: Option<(usize, std::ops::Range<usize>)>,
    /// Told where that one was drawn, when the view brings it into sight.
    pub seen: Option<Seen>,
}

impl Sought {
    /// The matches in `range` of block `index`, in the block's
    /// coordinates, and whether each is the one the find is on.
    pub fn marks(
        &self,
        block: &Block,
        index: usize,
        range: std::ops::Range<usize>,
    ) -> Vec<(std::ops::Range<usize>, bool)> {
        let Some(text) = block.text.get(range.clone()) else {
            return Vec::new();
        };
        let current = self
            .current
            .as_ref()
            .filter(|(at, _)| *at == index)
            .map(|(_, found)| found);
        skwir_platform::text_rows::matches(text, &self.query)
            .into_iter()
            .map(|found| {
                let found = found.start + range.start..found.end + range.start;
                let on = current == Some(&found);
                (found, on)
            })
            .collect()
    }
}

/// Where `range` of a text laid out and placed is drawn: from its start,
/// one line high, to its end when that is on the same line.
fn spot_of(layout: &gpui::TextLayout, range: std::ops::Range<usize>) -> Option<Spot> {
    let start = layout.position_for_index(range.start)?;
    let end = layout
        .position_for_index(range.end)
        .filter(|end| end.y == start.y)
        .unwrap_or(start);
    let line = layout.line_height();
    Some(Spot {
        area: gpui::Bounds::from_corners(start, gpui::point(end.x.max(start.x), start.y + line)),
        text_left: layout.bounds().left(),
    })
}

/// The text of `range` as drawn and its highlights (relative to `range`):
/// emphasis, links, code, scripts, small capitals and the `selection`, which
/// is in the block's coordinates. A link found in raw text is drawn as one
/// only while armed (`marks`); a label is set before its link. Shared by the
/// reader's rows and the note popover, so a note reads as it does in the
/// text.
pub fn styled(
    block: &Block,
    range: std::ops::Range<usize>,
    selection: std::ops::Range<usize>,
    style: &MarkdownStyle,
    marks: Marks<'_>,
) -> (String, Vec<(std::ops::Range<usize>, HighlightStyle)>) {
    let armed = marks.armed;
    let (shown, reduced) = small_capitals(block, range.clone());
    let mut edges = vec![range.start, range.end];
    edges.extend(reduced.iter().flat_map(|r| [r.start, r.end]));
    if selection.start < selection.end {
        edges.extend([selection.start, selection.end]);
    }
    for (found, _) in marks.found {
        edges.extend(
            [found.start, found.end]
                .into_iter()
                .filter(|edge| (range.start..=range.end).contains(edge)),
        );
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
                let link =
                    r.link.is_some() && (!r.found || armed.is_some_and(|a| a.contains(&start)));
                highlight.font_weight = r.bold.then_some(gpui::FontWeight::BOLD);
                highlight.font_style = r.italic.then_some(gpui::FontStyle::Italic);
                highlight.color = r
                    .color
                    .map(|[r, g, b]| {
                        gpui::rgb((u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)).into()
                    })
                    .or_else(|| link.then_some(style.link));
                // Inline code is set apart from the prose around it; a code
                // block's box has its own background (its theme's, if any).
                highlight.background_color =
                    (r.code && !block.code).then_some(style.inline_code_bg);
                if r.strikethrough {
                    highlight.strikethrough = Some(StrikethroughStyle {
                        thickness: px(1.),
                        color: Some(style.text_muted),
                    });
                }
                highlight.position = match r.script {
                    Script::Baseline => None,
                    Script::Superscript => Some(RunPosition::Superscript),
                    Script::Subscript => Some(RunPosition::Subscript),
                };
                if highlight.position.is_none() && reduced.iter().any(|r| r.contains(&start)) {
                    highlight.position = Some(RunPosition::Reduced);
                }
                if link && (style.link_underline || r.found) {
                    highlight.underline = Some(UnderlineStyle {
                        thickness: px(1.),
                        color: Some(style.link),
                        wavy: false,
                    });
                } else if r.underline {
                    highlight.underline = Some(UnderlineStyle {
                        thickness: px(1.),
                        color: None,
                        wavy: false,
                    });
                }
            }
            if let Some((_, on)) = marks.found.iter().find(|(found, _)| found.contains(&start)) {
                highlight.background_color = Some(if *on {
                    style.find_current
                } else {
                    style.find_match
                });
                if *on {
                    highlight.color = Some(style.find_current_text);
                }
            }
            if selection.contains(&start) {
                highlight.background_color = Some(style.selection_bg);
            }
            (start - range.start..end - range.start, highlight)
        })
        .collect::<Vec<_>>();
    labelled(shown, highlights, range, marks.labels, style)
}

/// `shown` with each label set before the link it names: what follows a
/// label moves along by its length, and the label is drawn as a key cap.
/// Labels stand at the start of a link, which is an edge of every
/// highlight, so none is cut in two.
fn labelled(
    shown: String,
    highlights: Vec<(std::ops::Range<usize>, HighlightStyle)>,
    range: std::ops::Range<usize>,
    labels: &[(usize, SharedString)],
    style: &MarkdownStyle,
) -> (String, Vec<(std::ops::Range<usize>, HighlightStyle)>) {
    let labels: Vec<(usize, &SharedString)> = labels
        .iter()
        .filter(|(at, _)| range.contains(at) && shown.is_char_boundary(at - range.start))
        .map(|(at, label)| (at - range.start, label))
        .collect();
    if labels.is_empty() {
        return (shown, highlights);
    }
    let before = |offset: usize, inclusive: bool| -> usize {
        labels
            .iter()
            .filter(|(at, _)| *at < offset || inclusive && *at == offset)
            .map(|(_, label)| label.len())
            .sum()
    };
    let mut highlights: Vec<_> = highlights
        .into_iter()
        .map(|(r, h)| {
            (
                r.start + before(r.start, true)..r.end + before(r.end, false),
                h,
            )
        })
        .collect();
    let mut text = shown;
    for (at, label) in labels.iter().rev() {
        text.insert_str(*at, label);
    }
    let cap = HighlightStyle {
        color: Some(style.label_text),
        background_color: Some(style.link),
        font_weight: Some(gpui::FontWeight::BOLD),
        ..HighlightStyle::default()
    };
    for (at, label) in &labels {
        let start = at + before(*at, false);
        highlights.push((start..start + label.len(), cap));
    }
    highlights.sort_by_key(|(r, _)| r.start);
    (text, highlights)
}

/// A link of a stretch of text: where it is drawn (relative to the
/// stretch), where it is in the block, where it leads, and how it is
/// followed.
#[derive(Clone)]
pub(crate) struct Link {
    shown: std::ops::Range<usize>,
    pub(crate) block: std::ops::Range<usize>,
    pub(crate) target: String,
    pub(crate) note: bool,
    /// The link's own name, which a note may link back to (`Run::name`).
    pub(crate) name: Option<String>,
    found: bool,
}

/// The links of `range` of `block`, in order: the runs one link was cut
/// into (a found address across a source line's colours) are one link.
pub(crate) fn links_in(block: &Block, range: std::ops::Range<usize>) -> Vec<Link> {
    let mut links: Vec<Link> = Vec::new();
    for run in &block.runs {
        let clipped = run.range.start.max(range.start)..run.range.end.min(range.end);
        let Some(target) = run.link.as_ref().filter(|_| clipped.start < clipped.end) else {
            continue;
        };
        if let Some(last) = links.last_mut()
            && last.block.end == clipped.start
            && last.target == *target
            && last.found == run.found
        {
            last.block.end = clipped.end;
            last.shown.end = clipped.end - range.start;
            continue;
        }
        links.push(Link {
            shown: clipped.start - range.start..clipped.end - range.start,
            block: clipped,
            target: target.clone(),
            note: run.note,
            name: run.name.clone(),
            found: run.found,
        });
    }
    links
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
    let element = gpui::ElementId::from((id.clone(), "text"));
    // The found link Ctrl is held over, if it is drawn here.
    let armed = style
        .armed_link
        .as_ref()
        .filter(|(at, _)| *at == element)
        .map(|(_, armed)| armed.clone());
    let labels = style
        .labels
        .as_ref()
        .and_then(|labels| labels.get(&index))
        .map_or(&[][..], Vec::as_slice);
    let found = style
        .sought
        .as_deref()
        .map(|sought| sought.marks(block, index, range.clone()))
        .unwrap_or_default();
    // The match the find is on, where the view is told it was drawn: not
    // while labels move the text along.
    let spot = style
        .sought
        .as_ref()
        .and_then(|sought| sought.seen.clone())
        .zip(
            found
                .iter()
                .find(|(_, on)| *on)
                .map(|(found, _)| found.clone()),
        )
        .filter(|_| labels.is_empty());
    let marks = Marks {
        armed: armed.as_ref(),
        labels,
        found: &found,
    };
    let (shown, highlights) = styled(block, range.clone(), selection, style, marks);
    let text = StyledText::new(shown).with_highlights(highlights);
    let down = text.layout().clone();
    let placed = text.layout().clone();
    let start = range.start;
    let moved = down.clone();
    // While labelled the text moved along by its labels: the keys follow
    // links then, not the pointer.
    let links = if style.labels.is_some() {
        Vec::new()
    } else {
        links_in(block, range.clone())
    };
    // What answers a click: written links, and the found one armed.
    let followed: Vec<Link> = links
        .iter()
        .filter(|link| !link.found || armed.as_ref() == Some(&link.block))
        .cloned()
        .collect();
    let ranges = followed.iter().map(|link| link.shown.clone()).collect();
    let found: Vec<Link> = links.into_iter().filter(|link| link.found).collect();
    let clicked = down.clone();
    let weak = entity.clone();
    let offset_of = move |position, layout: &gpui::TextLayout| {
        range.start + layout.index_for_position(position).unwrap_or_else(|i| i)
    };
    let text =
        gpui::InteractiveText::new(element.clone(), text).on_click(ranges, move |i, window, cx| {
            let link = &followed[i];
            let held = window.modifiers();
            if link.found && !held.secondary() {
                return;
            }
            // A note opens under its reference: where the reference is
            // drawn now, below its line.
            let under = clicked
                .position_for_index(link.shown.start)
                .map(|at| at + gpui::point(px(0.), clicked.line_height()));
            let _ = weak.update(cx, |s, cx| {
                // The keys reach links too (Ctrl+Shift+O): said once.
                s.note_pointer_use("ui.inspect", &serde_json::json!({"action": "links"}), cx);
                s.with_pane(pane, |s| {
                    s.run(
                        "ui.inspect",
                        serde_json::json!({
                            "action": "link",
                            "target": link.target,
                            "note": link.note,
                            "name": link.name,
                            "tab": held.shift,
                            "x": under.map(|at| f32::from(at.x)),
                            "y": under.map(|at| f32::from(at.y)),
                        }),
                        window,
                        cx,
                    );
                });
            });
        });
    // A found link under the pointer becomes one while Ctrl is held: the
    // shell keeps which, and draws it so (`Shell::armed_link`). Every move
    // is read, as the text's own hover says nothing when the pointer comes
    // back to the character it left from.
    let hovered = (!found.is_empty()).then(|| (element.clone(), entity.clone()));
    let pointed = hovered.clone().map(|(element, weak)| {
        let layout = moved.clone();
        move |event: &gpui::MouseMoveEvent, cx: &mut gpui::App| {
            let under = layout
                .index_for_position(event.position)
                .ok()
                .and_then(|index| {
                    found
                        .iter()
                        .find(|link| link.shown.contains(&index))
                        .map(|link| link.block.clone())
                });
            let held = event.modifiers.secondary();
            let _ = weak.update(cx, |s, cx| s.hover_link(&element, under, held, cx));
        }
    });
    let press = entity.clone();
    let motion = entity.clone();
    let release = entity.clone();
    div()
        .id(gpui::ElementId::from((id, "select")))
        .cursor_text()
        .child(text)
        // Prepainted after the text: where the match is drawn is known.
        .when_some(spot, |d, (seen, found)| {
            d.child(
                gpui::canvas(
                    move |_, _, _| {
                        seen.set(spot_of(&placed, found.start - start..found.end - start));
                    },
                    |_, (), _, _| {},
                )
                .absolute()
                .size_0(),
            )
        })
        .when_some(hovered, |d, (element, weak)| {
            d.on_hover(move |inside, _, cx| {
                if !*inside {
                    let _ = weak.update(cx, |s, cx| s.leave_link(&element, cx));
                }
            })
        })
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
            if let Some(pointed) = &pointed {
                pointed(event, cx);
            }
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
    fn small_capitals_keep_every_offset() {
        use skwir_core::inspection::Run;
        let text = "The Straße, éclair";
        let block = Block {
            text: text.into(),
            runs: vec![
                Run {
                    range: 0..4,
                    ..Run::default()
                },
                Run {
                    range: 4..text.len(),
                    small_caps: true,
                    ..Run::default()
                },
            ],
            ..Block::default()
        };
        let (shown, reduced) = small_capitals(&block, 0..text.len());
        assert_eq!(shown.len(), text.len());
        // "ß" has no one-letter capital of its length: it stays as written.
        assert_eq!(shown, "The STRAßE, ÉCLAIR");
        let parts: Vec<_> = reduced.iter().map(|r| &text[r.clone()]).collect();
        assert_eq!(parts, ["tra", "e", "éclair"]);
        // A part of the block (a table cell) keeps the block's coordinates.
        let (shown, reduced) = small_capitals(&block, 5..10);
        assert_eq!(shown, "TRAß");
        assert_eq!(reduced, vec![std::ops::Range { start: 5, end: 8 }]);
    }
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
            kind: skwir_core::inspection::BlockKind::TableRow {
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
