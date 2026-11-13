//! The graph of a history's merges, laid out down its rows (ADR-133): each
//! commit a node in a lane, a line from it to each of its parents, lanes
//! side by side. Commits come newest first, never a parent before its
//! children: a lane waits for the commit it leads to, and the node of that
//! commit takes the first lane waiting for it, the others joining it.
use std::collections::HashMap;

use skwir_core::inspection::{GraphLine, GraphPart, GraphRow};

/// Colors a graph cycles through, as the asker draws them.
pub const COLORS: u8 = 8;
/// Lanes drawn at most: a line past them goes on unseen.
pub const LANES: usize = 64;

/// Each commit's row of the graph, in the order given: `(id, parents)`.
/// Linear in the commits times the lanes a row crosses.
pub fn lay<'a>(commits: impl IntoIterator<Item = (&'a str, &'a [String])>) -> Vec<GraphRow> {
    // Each lane's commit waited for and its color; `None` a free lane.
    let mut lanes: Vec<Option<(&'a str, u8)>> = Vec::new();
    // The lanes waiting for each commit, in the order they began.
    let mut waiting: HashMap<&'a str, Vec<usize>> = HashMap::new();
    let mut next_color = 0u8;
    let mut color = || {
        let given = next_color;
        next_color = (next_color + 1) % COLORS;
        given
    };
    let mut rows = Vec::new();
    for (id, parents) in commits {
        let mut hits = waiting.remove(id).unwrap_or_default();
        hits.sort_unstable();
        let node = hits.first().copied().unwrap_or_else(|| free(&mut lanes));
        let node_color = match lanes.get(node).copied().flatten() {
            Some((_, kept)) if hits.first() == Some(&node) => kept,
            _ => color(),
        };
        let mut drawn = Vec::new();
        for (lane, held) in lanes.iter().enumerate().take(LANES) {
            if let Some((_, color)) = held
                && hits.binary_search(&lane).is_err()
            {
                drawn.push(line(lane, lane, *color, GraphPart::Through));
            }
        }
        for &hit in &hits {
            if let Some(Some((_, color))) = lanes.get(hit)
                && hit < LANES
            {
                drawn.push(line(hit, node, *color, GraphPart::Above));
            }
            lanes[hit] = None;
        }
        if node == lanes.len() {
            lanes.push(None);
        }
        for (k, parent) in parents.iter().enumerate() {
            let parent = parent.as_str();
            // A lane already leads to it: this one joins it there.
            let (lane, color) = if let Some(&lane) = waiting.get(parent).and_then(|at| at.first()) {
                (lane, lanes[lane].map_or(node_color, |(_, color)| color))
            } else {
                let lane = if k == 0 && lanes[node].is_none() {
                    node
                } else {
                    free(&mut lanes)
                };
                let color = if lane == node { node_color } else { color() };
                lanes[lane] = Some((parent, color));
                waiting.entry(parent).or_default().push(lane);
                (lane, color)
            };
            if lane < LANES && node < LANES {
                drawn.push(line(node, lane, color, GraphPart::Below));
            }
        }
        while lanes.last().is_some_and(Option::is_none) {
            lanes.pop();
        }
        rows.push(GraphRow {
            node: lane_of(node),
            color: node_color,
            lines: drawn,
        });
    }
    rows
}

/// The first free lane, made where none is.
fn free(lanes: &mut Vec<Option<(&str, u8)>>) -> usize {
    lanes.iter().position(Option::is_none).unwrap_or_else(|| {
        lanes.push(None);
        lanes.len() - 1
    })
}

fn lane_of(lane: usize) -> u16 {
    u16::try_from(lane).unwrap_or(u16::MAX)
}

fn line(from: usize, to: usize, color: u8, part: GraphPart) -> GraphLine {
    GraphLine {
        from: lane_of(from),
        to: lane_of(to),
        color,
        part,
    }
}

#[cfg(test)]
mod tests {
    use super::lay;
    use skwir_core::inspection::{GraphLine, GraphPart};

    fn owned(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|id| (*id).to_owned()).collect()
    }

    fn parts(row: &skwir_core::inspection::GraphRow) -> Vec<(u16, u16, GraphPart)> {
        let mut parts: Vec<_> = row
            .lines
            .iter()
            .map(|GraphLine { from, to, part, .. }| (*from, *to, *part))
            .collect();
        parts.sort_by_key(|(from, to, part)| (*part as u8, *from, *to));
        parts
    }

    /// A line of commits stays in one lane, from the newest down to the
    /// first, which has nothing below it.
    #[test]
    fn a_line_of_commits_keeps_one_lane() {
        use GraphPart::{Above, Below};
        let history = [
            ("c", owned(&["b"])),
            ("b", owned(&["a"])),
            ("a", owned(&[])),
        ];
        let rows = lay(history.iter().map(|(id, p)| (*id, p.as_slice())));
        assert_eq!(parts(&rows[0]), [(0, 0, Below)]);
        assert_eq!(parts(&rows[1]), [(0, 0, Above), (0, 0, Below)]);
        assert_eq!(parts(&rows[2]), [(0, 0, Above)]);
        assert!(
            rows.iter()
                .all(|row| row.node == 0 && row.color == rows[0].color)
        );
    }

    /// A merge opens a lane for its second parent, in a color of its own;
    /// the side's commit comes down it, and its line goes back into the
    /// first lane where both sides meet.
    #[test]
    fn a_merge_opens_a_lane_that_joins_where_both_sides_meet() {
        use GraphPart::{Above, Below, Through};
        let history = [
            ("m", owned(&["a", "b"])),
            ("a", owned(&["c"])),
            ("b", owned(&["c"])),
            ("c", owned(&[])),
        ];
        let rows = lay(history.iter().map(|(id, p)| (*id, p.as_slice())));
        assert_eq!(parts(&rows[0]), [(0, 0, Below), (0, 1, Below)]);
        assert_eq!(
            parts(&rows[1]),
            [(0, 0, Above), (0, 0, Below), (1, 1, Through)]
        );
        assert_eq!(rows[2].node, 1);
        assert_eq!(
            parts(&rows[2]),
            [(1, 1, Above), (1, 0, Below), (0, 0, Through)]
        );
        assert_eq!(parts(&rows[3]), [(0, 0, Above)]);
        assert_ne!(
            rows[0].lines[1].color, rows[0].color,
            "the side's own color"
        );
        assert_eq!(rows[2].color, rows[0].lines[1].color, "kept down its lane");
    }

    /// Two branch tips, neither the other's parent, each take a lane; a lane
    /// freed is taken again rather than the graph growing wider.
    #[test]
    fn tips_take_free_lanes_and_freed_ones_are_taken_again() {
        let history = [
            ("x", owned(&["a"])),
            ("y", owned(&["b"])),
            ("a", owned(&["r"])),
            ("b", owned(&["r"])),
            ("z", owned(&["r"])),
            ("r", owned(&[])),
        ];
        let rows = lay(history.iter().map(|(id, p)| (*id, p.as_slice())));
        let nodes: Vec<u16> = rows.iter().map(|row| row.node).collect();
        // y beside x; b in y's lane, meeting r in a's; z in the lane b left.
        assert_eq!(nodes, [0, 1, 0, 1, 1, 0]);
        assert!(
            rows.iter()
                .all(|row| row.lines.iter().all(|l| l.from < 3 && l.to < 3))
        );
    }
}
