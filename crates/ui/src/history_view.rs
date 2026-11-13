//! A history of commits' rows (`InspectionKind::History`, ADR-133): its
//! graph of merges down the rows' start, each row's part drawn in its own
//! bounds so the lines meet from row to row, then its summary after the
//! branches and tags at it, and its short id, author and date under it;
//! and a commit's menu, which copies its id.
use gpui::{Bounds, Div, PathBuilder, Pixels, Window, div, point, prelude::*, px};
use skwir_core::inspection::{GraphPart, GraphRow, REFS_JOIN, RefKind, Row};

use crate::sizes::{TextSizes, sized};
use crate::theme::Theme;

/// A lane's width; a graph's width is its widest row's lanes, `DRAWN` at
/// most (`Display::lanes`). A picture's: it keeps its size as text grows.
pub const LANE: f32 = 12.;
/// Lanes drawn at most; past them a line goes on unseen.
pub const DRAWN: u16 = 16;
/// Room before the graph's first lane and after its last, so its lines
/// stand apart from the panel's edge and from the summaries.
const BEFORE: f32 = 10.;
const AFTER: f32 = 12.;
/// Room before a row's text where it draws no graph.
const PLAIN: f32 = 8.;
const NODE: f32 = 3.5;
const STROKE: f32 = 1.5;
/// Where the node stands down the row: level with the summary's line.
const NODE_AT: f32 = 0.34;

/// How many lanes a history's graph takes: its widest row's, `DRAWN` at
/// most; 0 without a graph. On the worker, with the inspection.
pub fn lanes(rows: &[Row]) -> u16 {
    rows.iter()
        .filter_map(|row| row.graph.as_deref())
        .flat_map(|graph| {
            std::iter::once(graph.node).chain(graph.lines.iter().flat_map(|l| [l.from, l.to]))
        })
        .max()
        .map_or(0, |widest| widest.saturating_add(1).min(DRAWN))
}

/// A commit's row: the graph's part, its names and summary, its short id
/// (in the code font: what is read a character at a time), its author and
/// date; drawn in what is chosen's ink while chosen (`chosen`).
pub fn row(row: &Row, lanes: u16, chosen: bool, theme: &Theme) -> Div {
    let cell = |i: usize| row.cells.get(i).map_or("", String::as_str);
    // The lane the commit's branches are drawn in: its node's, else the
    // first, which a history without a graph would draw its line in.
    let lane = row
        .graph
        .as_ref()
        .map_or(0, |graph| usize::from(graph.color));
    let names: Vec<Div> = cell(4)
        .split(REFS_JOIN)
        .filter(|name| !name.is_empty())
        .map(|name| {
            let (kind, name) = RefKind::of(name);
            name_chip(kind, name, lane, chosen, theme)
        })
        .collect();
    let width = f32::from(lanes) * LANE;
    let byline = [cell(2), cell(3)]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
    let lead = if row.graph.is_some() {
        BEFORE + width + AFTER
    } else {
        PLAIN
    };
    div()
        .relative()
        .w_full()
        .flex()
        .py_1()
        .pr_2()
        .pl(px(lead))
        .when_some(row.graph.clone(), |d, graph| {
            let colors = theme.lanes;
            let ink = chosen.then_some(theme.selection_text);
            d.child(
                gpui::canvas(
                    |_, _, _| {},
                    move |bounds, (), window, _| paint(&graph, bounds, colors, ink, window),
                )
                .absolute()
                .top_0()
                .bottom_0()
                .left_0()
                .w(px(BEFORE + width)),
            )
        })
        .child(
            div()
                .min_w_0()
                .flex_1()
                .flex()
                .flex_col()
                .child(
                    div()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap_1()
                        .children(names)
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .text_body()
                                .truncate()
                                .child(cell(0).to_owned()),
                        ),
                )
                .child(
                    div()
                        .min_w_0()
                        .min_h(sized(16.))
                        .flex()
                        .gap_1()
                        .text_small()
                        .text_color(theme.text_muted)
                        .whitespace_nowrap()
                        .child(
                            div()
                                .flex_none()
                                .font_family(theme.code_font.clone())
                                .child(cell(1).to_owned()),
                        )
                        .when(!byline.is_empty(), |d| d.child("·"))
                        .child(div().min_w_0().truncate().child(byline)),
                ),
        )
}

/// A name at a commit, as a chip at the text's size, told apart by what it
/// is: a branch in its lane's color, its tint under the name and its line
/// around it, as the graph draws the lane; the branch checked out in
/// semibold; a remote-tracking one outlined alone, not one of yours to
/// work on; a tag in the chrome's tint, its glyph before its name in
/// semibold, a name no lane wears. Chosen, an outline is drawn in what is
/// chosen's ink; a tinted chip keeps its colors, a ground of its own.
fn name_chip(kind: RefKind, name: &str, lane: usize, chosen: bool, theme: &Theme) -> Div {
    let color = theme.lanes[lane % theme.lanes.len()];
    let outline = if chosen { theme.selection_text } else { color };
    let chip = div()
        .flex_none()
        .h(sized(crate::tags::CHIP_HEIGHT))
        .flex()
        .items_center()
        .gap_1()
        .px(sized(6.))
        .rounded_sm()
        .border_1()
        .text_body()
        .whitespace_nowrap();
    match kind {
        RefKind::Current | RefKind::Branch => chip
            .bg(theme.tag_fill(lane))
            .border_color(color)
            .text_color(theme.text)
            .when(kind == RefKind::Current, |d| {
                d.font_weight(gpui::FontWeight::SEMIBOLD)
            })
            .child(name.to_owned()),
        RefKind::Remote => chip.border_color(outline).child(name.to_owned()),
        RefKind::Tag => chip
            .bg(theme.on_bg(theme.surface))
            .border_color(theme.border)
            .text_color(theme.text)
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .child(
                gpui::svg()
                    .path("toolbar/tag.svg")
                    .flex_none()
                    .size(px(12.))
                    .text_color(theme.text_muted),
            )
            .child(name.to_owned()),
    }
}

/// The tags at a commit's row, as its names cell writes them.
pub fn tagged(row: &Row) -> impl Iterator<Item = &str> {
    // Most commits have no names: their empty cell is passed at once.
    row.cells
        .get(4)
        .filter(|names| !names.is_empty())
        .into_iter()
        .flat_map(|names| names.split(REFS_JOIN))
        .filter_map(|name| match RefKind::of(name) {
            (RefKind::Tag, tag) => Some(tag),
            _ => None,
        })
}

/// What a screen reader hears of a row's names: each with what it is.
pub fn said_names(cell: &str) -> String {
    cell.split(REFS_JOIN)
        .filter(|name| !name.is_empty())
        .map(|name| match RefKind::of(name) {
            (RefKind::Current, name) => format!("{name} (checked out)"),
            (RefKind::Branch, name) => name.to_owned(),
            (RefKind::Remote, name) => format!("{name} (remote)"),
            (RefKind::Tag, name) => format!("tag {name}"),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// A row's part of the graph in `bounds`: the lines going by, those from
/// the children above to the node, those from it to the parents below,
/// each in its lane's color, then the node; in `ink` while chosen.
fn paint(
    graph: &GraphRow,
    bounds: Bounds<Pixels>,
    colors: [gpui::Hsla; 8],
    ink: Option<gpui::Hsla>,
    window: &mut Window,
) {
    let x = |lane: u16| bounds.left() + px(BEFORE + LANE * (f32::from(lane) + 0.5));
    let (top, bottom) = (bounds.top(), bottom_of(bounds));
    let node_y = top + bounds.size.height * NODE_AT;
    let color = |index: u8| ink.unwrap_or(colors[usize::from(index) % colors.len()]);
    for line in &graph.lines {
        if line.from >= DRAWN || line.to >= DRAWN {
            continue;
        }
        let (from, to) = (x(line.from), x(line.to));
        let mut path = PathBuilder::stroke(px(STROKE));
        match line.part {
            GraphPart::Through => {
                path.move_to(point(from, top));
                path.line_to(point(to, bottom));
            }
            // Down from the child's lane, bending into the node.
            GraphPart::Above => {
                path.move_to(point(from, top));
                if from == to {
                    path.line_to(point(to, node_y));
                } else {
                    path.curve_to(point(to, node_y), point(from, node_y));
                }
            }
            // Out of the node, bending down into the parent's lane.
            GraphPart::Below => {
                path.move_to(point(from, node_y));
                if from == to {
                    path.line_to(point(to, bottom));
                } else {
                    path.curve_to(point(to, bottom), point(to, node_y));
                }
            }
        }
        if let Ok(path) = path.build() {
            window.paint_path(path, color(line.color));
        }
    }
    if graph.node < DRAWN {
        let center = point(x(graph.node), node_y);
        let radius = px(NODE);
        window.paint_quad(
            gpui::fill(
                Bounds::new(
                    point(center.x - radius, center.y - radius),
                    gpui::size(radius * 2., radius * 2.),
                ),
                color(graph.color),
            )
            .corner_radii(radius),
        );
    }
}

fn bottom_of(bounds: Bounds<Pixels>) -> Pixels {
    bounds.top() + bounds.size.height
}

impl crate::shell::Shell {
    /// A commit's menu, its row pressed with the right button (`index`, in
    /// the history the panel shows or the one kept above a patch,
    /// `parked`): the row chosen, then what copies it: its id whole,
    /// whatever the row shows of it, or as the row shows it, and its
    /// summary.
    pub(crate) fn open_commit_menu(
        &mut self,
        parked: bool,
        index: usize,
        position: gpui::Point<Pixels>,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let choose = |shell: &mut Self| {
            shell.preview.inspection.selected = Some(index);
            let Some(crate::preview::PreviewContent::Inspection(display)) = &shell.preview.content
            else {
                return None;
            };
            let row = crate::inspector::listed(&display.data)
                .get(index.checked_sub(display.data.blocks.len())?)?;
            let cell = |i: usize| row.cells.get(i).cloned().unwrap_or_default();
            Some((row.target.clone()?, cell(1), cell(0)))
        };
        let found = if parked {
            self.with_parked(choose).flatten()
        } else {
            choose(self)
        };
        let Some((id, short, summary)) = found else {
            return;
        };
        let item = |order, title: &str, what: &str, text: String| {
            skwir_core::registry::MenuItem::new(
                "preview",
                title,
                "copy",
                order,
                skwir_core::command::Invocation::with_args(
                    "ui.copy_text",
                    &serde_json::json!({ "text": text, "what": what }),
                ),
            )
        };
        let items = vec![
            item(100, "Copy hash", "the commit's hash", id),
            item(101, "Copy short hash", "the commit's short hash", short),
            item(102, "Copy summary", "the commit's summary", summary),
        ];
        self.menu = Some(crate::menu::MenuState::open(position, items, window, cx));
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::{DRAWN, lanes, said_names};
    use skwir_core::inspection::{REFS_JOIN, RefKind};

    /// A screen reader hears each name at a commit with what it is.
    #[test]
    fn a_commit_s_names_are_said_with_what_they_are() {
        let cell = [
            RefKind::Current.written("main"),
            RefKind::Branch.written("feature/x"),
            RefKind::Remote.written("origin/main"),
            RefKind::Tag.written("v1"),
        ]
        .join(REFS_JOIN);
        assert_eq!(
            said_names(&cell),
            "main (checked out), feature/x, origin/main (remote), tag v1"
        );
        assert_eq!(RefKind::of("tag:v1"), (RefKind::Tag, "v1"));
        assert_eq!(RefKind::of("plain"), (RefKind::Branch, "plain"));
    }
    use skwir_core::inspection::{GraphLine, GraphPart, GraphRow, Row};

    /// A graph is as wide as its widest row, a line's far lane included,
    /// `DRAWN` lanes at most; none without a graph.
    #[test]
    fn a_graph_is_as_wide_as_its_widest_row() {
        let with = |node: u16, to: u16| Row {
            graph: Some(Box::new(GraphRow {
                node,
                color: 0,
                lines: vec![GraphLine {
                    from: node,
                    to,
                    color: 0,
                    part: GraphPart::Below,
                }],
            })),
            ..Row::default()
        };
        assert_eq!(lanes(&[with(0, 0), with(0, 2)]), 3);
        assert_eq!(lanes(&[with(40, 41)]), DRAWN);
        assert_eq!(lanes(&[Row::default()]), 0);
    }
}
