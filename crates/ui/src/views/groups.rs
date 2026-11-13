//! A pane's groups as a view of rows draws them (ADR-130, 9): a heading
//! is a row, never an entry. `RowMap` says where each row of the view is
//! in the pane's order, made once per order from the pane's groups
//! (`Pane::groups`). Drawing, scrolling and the pointer work in rows;
//! everything else (the cursor, ranges, Select All, the rubber band's
//! selection, a screen reader's position and count) in positions, as
//! when nothing is grouped.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use gpui::{
    AnyElement, App, Bounds, Div, Pixels, Point, UniformListDecoration, Window, div, prelude::*, px,
};
use skwir_core::grouping::Span;

use crate::sizes::{Scale, TextSizes};
use crate::theme::Theme;

/// What a row of a view is: a group's heading, an entry, or what a plan
/// brings that has not landed (ADR-135). Only an entry has a position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    /// The heading of the `n`th group.
    Heading(usize),
    /// The heading of the `n`th group no file is in yet, which only what
    /// arrives is in (`RowMap::fresh`).
    Fresh(usize),
    /// The entry at this position of the order.
    Entry(usize),
    /// The `n`th of what arrives, as the view was given them.
    Arriving(usize),
}

/// A group no file is in yet, which what arrives will be in (ADR-135):
/// drawn before the `before`th of the pane's groups (after them all at
/// their count), never one of them, so their numbers stay the core's.
#[derive(Clone, Debug)]
pub struct Fresh {
    pub before: usize,
    pub group: Arc<skwir_core::grouping::Group>,
}

/// A group as a grid draws its lines, in the order drawn: one of the
/// pane's, or one no file is in yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Drawn {
    Group(usize),
    Fresh(usize),
}

/// What a tile of a grid is: an entry, or what arrives (a grid's headings
/// are lines of their own, no tile).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tile {
    Entry(usize),
    Arriving(usize),
}

/// Where what a plan brings is drawn (ADR-135): before the entry at `at`
/// in the order (after the last at its length), in group `group` where
/// the view groups, or under the heading of `fresh`, a group no file is
/// in yet; in no group shown, after every group.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Placed {
    pub at: usize,
    pub group: Option<usize>,
    pub fresh: Option<usize>,
}

/// Where each row of a view is in the order it draws: the entries of each
/// group under its heading, and what arrives among them where it will
/// land. Neither grouped nor arriving, row and position are one. Every
/// question is a search among the rows that are no entry, never a walk of
/// the rows.
#[derive(Clone, Debug, Default)]
pub struct RowMap {
    /// Empty while not grouped.
    spans: Arc<[Span]>,
    entries: usize,
    /// The rows that are no entry, in the order drawn, each with its row.
    extras: Arc<[(usize, Row)]>,
    /// The row of each group's heading.
    headings: Arc<[usize]>,
    /// The row of each heading drawn, in the order drawn (`drawn`): the
    /// pane's groups' and those of groups no file is in yet.
    heads: Arc<[usize]>,
    /// The tiles of a grid that are no entry, in order: each tile, and
    /// which of what arrives it is.
    tiles: Arc<[(usize, usize)]>,
    /// How many tiles each group drawn holds, what arrives in it among
    /// them, in the order drawn (`drawn`).
    lens: Arc<[usize]>,
    /// The groups no file is in yet, and every group in the order drawn.
    fresh: Arc<[Fresh]>,
    drawn: Arc<[Drawn]>,
    /// Where each of the pane's groups is among `drawn`.
    drawn_at: Arc<[usize]>,
}

impl RowMap {
    /// The rows of an order of `entries` grouped as `spans` say. Spans that
    /// do not cover the order from its start, one after the other (an
    /// order and groups read apart), group nothing. A folded group's span
    /// holds none of them (ADR-131, 2): its heading, with its count.
    pub fn new(spans: Arc<[Span]>, entries: usize) -> Self {
        Self::arriving(spans, entries, &[], &[])
    }

    /// `new`, with what arrives placed as `arriving` says (ADR-135): in
    /// its group before the entry it goes before, or at the group's end
    /// for one past it; under the heading of a group no file is in yet
    /// (`fresh`), drawn where that group stands among the others; after
    /// every group, in none shown; in a folded group, not drawn. Those that
    /// go before the same entry in the order given.
    pub fn arriving(
        spans: Arc<[Span]>,
        entries: usize,
        arriving: &[Placed],
        fresh: &[Fresh],
    ) -> Self {
        let mut at = 0;
        let whole = spans.iter().all(|span| {
            let follows = span.start == at && (span.len > 0 || span.count > 0);
            at += span.len;
            follows
        }) && at == entries;
        let spans: Arc<[Span]> = if whole { spans } else { Arc::from([]) };
        let grouped = !spans.is_empty();
        // The groups in the order drawn: each group no file is in yet
        // before the one it stands before.
        let mut fresh_order: Vec<usize> = if grouped {
            (0..fresh.len()).collect()
        } else {
            Vec::new()
        };
        fresh_order.sort_by_key(|&k| (fresh[k].before.min(spans.len()), k));
        let mut drawn: Vec<Drawn> = Vec::with_capacity(spans.len() + fresh_order.len());
        let mut drawn_at = Vec::with_capacity(spans.len());
        let mut f = 0;
        for n in 0..spans.len() {
            while let Some(&k) = fresh_order.get(f)
                && fresh[k].before.min(spans.len()) <= n
            {
                drawn.push(Drawn::Fresh(k));
                f += 1;
            }
            drawn_at.push(drawn.len());
            drawn.push(Drawn::Group(n));
        }
        drawn.extend(fresh_order[f..].iter().map(|&k| Drawn::Fresh(k)));
        let place_of: HashMap<Drawn, usize> =
            drawn.iter().enumerate().map(|(d, g)| (*g, d)).collect();
        // Each in the group drawn it is in, or none (`usize::MAX`, after
        // them all).
        let drawn_of = |placed: &Placed| -> Option<usize> {
            if !grouped {
                return None;
            }
            let group = match (placed.fresh, placed.group) {
                (Some(k), _) if k < fresh.len() => Drawn::Fresh(k),
                (_, Some(n)) if n < spans.len() => Drawn::Group(n),
                _ => return None,
            };
            place_of.get(&group).copied()
        };
        let mut order: Vec<usize> = (0..arriving.len())
            .filter(|&i| {
                !grouped
                    || !matches!(
                        drawn_of(&arriving[i]).map(|d| drawn[d]),
                        Some(Drawn::Group(n)) if spans[n].len == 0
                    )
            })
            .collect();
        order.sort_by_key(|&i| {
            let placed = &arriving[i];
            let group = if grouped {
                drawn_of(placed).unwrap_or(usize::MAX)
            } else {
                0
            };
            (group, placed.at.min(entries), i)
        });
        let mut extras = Vec::with_capacity(drawn.len() + order.len());
        let mut headings = Vec::with_capacity(spans.len());
        let mut heads = Vec::with_capacity(drawn.len());
        let mut tiles = Vec::with_capacity(order.len());
        let mut lens = Vec::with_capacity(drawn.len());
        // The rows, and the tiles, before the next that is no entry.
        let (mut rows_before, mut tiles_before) = (0, 0);
        let mut next = 0;
        for (d, group) in drawn.iter().enumerate() {
            // Where its entries are: a group no file is in yet stands where
            // the group after it begins.
            let (heading, from, to) = match *group {
                Drawn::Group(n) => (
                    Row::Heading(n),
                    spans[n].start,
                    spans[n].start + spans[n].len,
                ),
                Drawn::Fresh(k) => {
                    let start = spans.get(fresh[k].before).map_or(entries, |s| s.start);
                    (Row::Fresh(k), start, start)
                }
            };
            if let Drawn::Group(_) = group {
                headings.push(from + rows_before);
            }
            heads.push(from + rows_before);
            extras.push((from + rows_before, heading));
            rows_before += 1;
            let mut len = to - from;
            while let Some(&i) = order.get(next)
                && drawn_of(&arriving[i]) == Some(d)
            {
                let at = arriving[i].at.clamp(from, to);
                extras.push((at + rows_before, Row::Arriving(i)));
                tiles.push((at + tiles_before, i));
                rows_before += 1;
                tiles_before += 1;
                len += 1;
                next += 1;
            }
            lens.push(len);
        }
        // Not grouped, each where it lands; grouped, those in no group
        // shown, after them all, in the last group's tiles.
        for &i in &order[next..] {
            let at = if grouped {
                entries
            } else {
                arriving[i].at.min(entries)
            };
            extras.push((at + rows_before, Row::Arriving(i)));
            tiles.push((at + tiles_before, i));
            rows_before += 1;
            tiles_before += 1;
            if let Some(last) = lens.last_mut() {
                *last += 1;
            }
        }
        RowMap {
            spans,
            entries,
            extras: extras.into(),
            headings: headings.into(),
            heads: heads.into(),
            tiles: tiles.into(),
            lens: lens.into(),
            fresh: if grouped {
                fresh.to_vec().into()
            } else {
                Arc::from([])
            },
            drawn: drawn.into(),
            drawn_at: drawn_at.into(),
        }
    }

    /// Where the pane's group `n` is among the groups drawn (`drawn`): the
    /// grid's line of headings it heads.
    pub fn drawn_of(&self, n: usize) -> Option<usize> {
        self.drawn_at.get(n).copied()
    }

    /// The groups no file is in yet, whose headings only what arrives
    /// follows (`Row::Fresh`).
    pub fn fresh(&self) -> &[Fresh] {
        &self.fresh
    }

    /// Every group in the order drawn: what a grid's `n`th group of lines
    /// is (`tile_lens`).
    pub fn drawn(&self) -> &[Drawn] {
        &self.drawn
    }

    /// An order of `entries` not grouped.
    pub fn flat(entries: usize) -> Self {
        Self::new(Arc::from([]), entries)
    }

    pub fn grouped(&self) -> bool {
        !self.spans.is_empty()
    }

    /// Whether each row is its entry: nothing grouped, nothing arriving.
    pub fn plain(&self) -> bool {
        self.extras.is_empty()
    }

    /// Whether something arrives among the rows.
    pub fn arrives(&self) -> bool {
        !self.tiles.is_empty()
    }

    /// Which rows these are, for what is laid out from them to be kept
    /// while they are the same: the rows of another order are another
    /// list of them.
    pub fn key(&self) -> (usize, usize, usize) {
        (
            Arc::as_ptr(&self.spans).cast::<()>().addr(),
            Arc::as_ptr(&self.extras).cast::<()>().addr(),
            self.extras.len(),
        )
    }

    /// How many rows there are: the entries, the headings and what
    /// arrives.
    pub fn len(&self) -> usize {
        self.entries + self.extras.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn spans(&self) -> &[Span] {
        &self.spans
    }

    /// The row of group `n`'s heading.
    pub fn heading_row(&self, n: usize) -> usize {
        self.headings[n]
    }

    /// The group drawn (`drawn`) whose heading or rows `row` is, the last
    /// whose heading is not below it: one of the pane's or one no file is
    /// in yet.
    pub fn drawn_at(&self, row: usize) -> Option<usize> {
        if self.spans.is_empty() || row >= self.len() {
            return None;
        }
        Some(
            self.heads
                .partition_point(|heading| *heading <= row)
                .saturating_sub(1),
        )
    }

    /// The row of the heading of the `d`th group drawn.
    pub fn drawn_row(&self, d: usize) -> Option<usize> {
        self.heads.get(d).copied()
    }

    /// The row in these rows that shows what row `row` of `old` (its order
    /// `old_order`) showed, after a fold (ADR-131, 2): the same heading,
    /// the same entry (where it is now, `position_of`), or, an entry folded
    /// away, its group's heading. None for what arrives, or what is gone.
    pub fn kept_row(
        &self,
        old: &RowMap,
        old_order: &[skwir_core::EntryId],
        row: usize,
        position_of: impl Fn(skwir_core::EntryId) -> Option<usize>,
    ) -> Option<usize> {
        let heading_of = |n: usize| {
            let group = old.spans.get(n)?.group.as_ref();
            let m = self
                .spans
                .iter()
                .position(|span| skwir_core::grouping::same(span.group.as_ref(), group))?;
            self.headings.get(m).copied()
        };
        match old.at(row)? {
            Row::Heading(n) => heading_of(n),
            Row::Entry(p) => match position_of(*old_order.get(p)?) {
                Some(q) => Some(self.row_of(q)),
                None => heading_of(old.group_of(p)?),
            },
            Row::Fresh(_) | Row::Arriving(_) => None,
        }
    }

    /// Where `row` is among the rows that are no entry: `Ok` the one it
    /// is, else how many are above it.
    fn extra_at(&self, row: usize) -> Result<usize, usize> {
        self.extras.binary_search_by_key(&row, |(at, _)| *at)
    }

    /// How many entries are above the `k`th row that is no entry.
    fn entries_above(&self, k: usize) -> usize {
        self.extras[k].0 - k
    }

    /// What `row` is.
    pub fn at(&self, row: usize) -> Option<Row> {
        if row >= self.len() {
            return None;
        }
        Some(match self.extra_at(row) {
            Ok(k) => self.extras[k].1,
            Err(above) => Row::Entry(row - above),
        })
    }

    /// The group the entry at `position` is in.
    pub fn group_of(&self, position: usize) -> Option<usize> {
        if self.spans.is_empty() || position >= self.entries {
            return None;
        }
        Some(
            self.spans
                .partition_point(|span| span.start <= position)
                .saturating_sub(1),
        )
    }

    /// The row of the entry at `position`: after every row that is no
    /// entry with no more entries above it.
    pub fn row_of(&self, position: usize) -> usize {
        let (mut lo, mut hi) = (0, self.extras.len());
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if self.entries_above(mid) <= position {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        position + lo
    }

    /// The first entry at `row` or below it (a heading's, its group's
    /// first, or a folded group's the next shown; what arrives, the entry
    /// it goes before), none past the last.
    pub fn entry_from(&self, row: usize) -> Option<usize> {
        if row >= self.len() {
            return None;
        }
        match self.extra_at(row) {
            Ok(k) => Some(self.entries_above(k)).filter(|at| *at < self.entries),
            Err(above) => Some(row - above),
        }
    }

    /// Where a page landing on `row` goes: the first entry at it or below
    /// it, else, below the last entry (a folded group's heading, what
    /// arrives after every entry), the last; none with no entry at all.
    pub fn entry_near(&self, row: usize) -> Option<usize> {
        self.entry_from(row.min(self.len().saturating_sub(1)))
            .or_else(|| self.entries.checked_sub(1))
    }

    /// Whether group `n` is folded: its span holds none of the entries.
    pub fn folded(&self, n: usize) -> bool {
        self.spans.get(n).is_some_and(|span| span.len == 0)
    }

    /// The positions of the entries among `rows`.
    pub fn entries_in(&self, rows: Range<usize>) -> Range<usize> {
        let rows = rows.start.min(self.len())..rows.end.min(self.len());
        if rows.is_empty() {
            return 0..0;
        }
        let start = self.entry_from(rows.start).unwrap_or(self.entries);
        let above_end = self.extras.partition_point(|(at, _)| *at < rows.end);
        let end = rows.end - above_end;
        start..end.max(start)
    }

    /// The entry at the top of a view scrolled to `offset` (pixels down)
    /// over rows `height` tall, and how far above the view begins from
    /// that entry's row: a heading, or what arrives, at the top is told by
    /// a negative distance, so that the view comes back to it.
    pub fn anchor(&self, offset: f32, height: f32) -> Option<(usize, f32)> {
        let row = (offset.max(0.) / height).floor() as usize;
        let within = offset.max(0.) - row as f32 * height;
        if row >= self.len() {
            return None;
        }
        match self.extra_at(row) {
            Err(above) => Some((row - above, within)),
            Ok(k) => Some((
                self.entries_above(k).min(self.entries.saturating_sub(1)),
                within - height,
            )),
        }
    }

    /// The offset (pixels down) that puts `anchor` back at the top.
    pub fn offset_of(&self, (position, within): (usize, f32), height: f32) -> f32 {
        let position = position.min(self.entries.saturating_sub(1));
        (self.row_of(position) as f32 * height + within.clamp(-height, height - 1.)).max(0.)
    }

    /// How many tiles a grid draws: the entries and what arrives.
    pub fn tiles(&self) -> usize {
        self.entries + self.tiles.len()
    }

    /// How many tiles each group holds, in a grid that groups.
    pub fn tile_lens(&self) -> &[usize] {
        &self.lens
    }

    /// What tile `tile` is.
    pub fn at_tile(&self, tile: usize) -> Option<Tile> {
        if tile >= self.tiles() {
            return None;
        }
        Some(
            match self.tiles.binary_search_by_key(&tile, |(at, _)| *at) {
                Ok(k) => Tile::Arriving(self.tiles[k].1),
                Err(above) => Tile::Entry(tile - above),
            },
        )
    }

    /// The tile of the entry at `position`.
    pub fn tile_of(&self, position: usize) -> usize {
        let (mut lo, mut hi) = (0, self.tiles.len());
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if self.tiles[mid].0 - mid <= position {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        position + lo
    }

    /// The positions of the entries among `tiles`.
    pub fn entries_in_tiles(&self, tiles: Range<usize>) -> Range<usize> {
        let tiles = tiles.start.min(self.tiles())..tiles.end.min(self.tiles());
        if tiles.is_empty() {
            return 0..0;
        }
        let before = |tile: usize| tile - self.tiles.partition_point(|(at, _)| *at < tile);
        before(tiles.start)..before(tiles.end)
    }

    /// The entry at tile `tile`, or the first after it where it is what
    /// arrives (`None` past the last).
    pub fn entry_of_tile(&self, tile: usize) -> Option<usize> {
        match self.at_tile(tile)? {
            Tile::Entry(position) => Some(position),
            Tile::Arriving(_) => {
                let above = self.tiles.partition_point(|(at, _)| *at < tile);
                Some(tile - above).filter(|at| *at < self.entries)
            }
        }
    }
}

/// What a group's heading says: its label, what it holds and what not
/// read yet holds.
pub fn label(span: &Span) -> Arc<str> {
    span.group
        .as_ref()
        .map_or_else(|| Arc::from("Unknown"), |group| Arc::clone(&group.label))
}

/// What a screen reader hears of a group: "Today, 3 items".
pub fn said(span: &Span) -> String {
    format!(
        "{}, {}",
        label(span),
        crate::format::count(span.count, "item")
    )
}

/// The heading of a group as a row `height` tall: its label in Body text,
/// as the names it heads, its lines theirs, 1.2 times the text; the count
/// beside it a note, Small in the muted ink (ADR-117); a line above it
/// unless it is the first, set on the files' ground as rows are, after its
/// `chevron` (`groups::chevron`). Its words begin `left` from its left
/// edge, where a grid's tiles do; none, where a list's rows begin theirs.
pub fn heading(
    span: &Span,
    first: bool,
    height: f32,
    left: Option<f32>,
    scale: Scale,
    theme: &Theme,
    chevron: Option<AnyElement>,
) -> Div {
    let words = (label(span), span.count.to_string());
    heading_saying(words, first, height, left, scale, theme, chevron)
}

/// The heading of a group no file is in yet (ADR-135): as a group's, its
/// label, "arriving" where a count would be, dimmed as what it heads,
/// with nothing to fold.
pub fn fresh_heading(
    fresh: &Fresh,
    first: bool,
    height: f32,
    left: Option<f32>,
    scale: Scale,
    theme: &Theme,
) -> Div {
    let words = (Arc::clone(&fresh.group.label), "arriving".to_owned());
    heading_saying(words, first, height, left, scale, theme, None)
        .opacity(crate::views::arriving::OPACITY)
}

/// What a screen reader hears of a group no file is in yet.
pub fn fresh_said(fresh: &Fresh) -> String {
    format!("{}, arriving", fresh.group.label)
}

/// A heading's row saying `(label, count)`: `heading`'s, a group's or one
/// no file is in yet's.
fn heading_saying(
    (label, count): (Arc<str>, String),
    first: bool,
    height: f32,
    left: Option<f32>,
    scale: Scale,
    theme: &Theme,
    chevron: Option<AnyElement>,
) -> Div {
    let words = div()
        .min_w_0()
        .flex()
        .items_baseline()
        .gap(scale.px(8.))
        .text_body()
        .line_height(gpui::relative(1.2))
        .children(chevron)
        .child(
            div()
                .min_w_0()
                .truncate()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(theme.text)
                .child(label.to_string()),
        )
        .child(
            div()
                .flex_none()
                .text_small()
                .text_color(theme.text_muted)
                .child(count),
        );
    div()
        .h(px(height))
        .w_full()
        .flex_none()
        .flex()
        .items_end()
        .pb(scale.px(5.))
        .when(!first, |d| d.border_t_1().border_color(theme.border))
        .map(|d| match left {
            Some(left) => d.pl(px(left)).pr_2().child(words),
            // In line with the rows' content, inside their 1 px ring.
            None => d.px_2().child(words.mx(px(1.))),
        })
}

/// The chevron at a heading's start that folds its group or opens it
/// (ADR-131, 2: `group_fold`), pointing right while it is folded. Pressed,
/// it does only that: the group is not selected.
pub fn chevron(
    group: usize,
    folded: bool,
    pane: skwir_core::workspace::PaneId,
    shell: gpui::WeakEntity<crate::shell::Shell>,
    scale: Scale,
    theme: &Theme,
) -> AnyElement {
    let side = scale.f(12.);
    div()
        .id(("fold", group))
        .flex_none()
        .self_center()
        .cursor_pointer()
        .on_mouse_down(
            gpui::MouseButton::Left,
            move |_: &gpui::MouseDownEvent, window, cx| {
                cx.stop_propagation();
                shell
                    .update(cx, |this, cx| {
                        this.with_pane(pane, |this| {
                            this.run(
                                "group_fold",
                                serde_json::json!({ "group": group, "pane": pane }),
                                window,
                                cx,
                            );
                        });
                    })
                    .ok();
            },
        )
        .child(
            gpui::svg()
                .path(if folded {
                    "toolbar/right.svg"
                } else {
                    "toolbar/chevron.svg"
                })
                .size(px(side))
                .text_color(theme.text_muted),
        )
        .into_any_element()
}

/// What a heading does (ADR-130, 12): lit where the pointer is, a press
/// selects its group (`ui.select_group`; with Ctrl, beside what is
/// selected), the right button opens its menu (Select these, Show only
/// these). Its pane is made the active one as a press on an entry makes
/// it; a band never begins on a heading (`marquee::Geometry::hits`).
pub fn acting(
    el: gpui::Stateful<Div>,
    group: usize,
    pane: skwir_core::workspace::PaneId,
    shell: gpui::WeakEntity<crate::shell::Shell>,
    theme: &Theme,
) -> gpui::Stateful<Div> {
    let hover = theme.hover;
    let menu = shell.clone();
    el.cursor_pointer()
        .hover(move |s| s.bg(hover))
        .on_mouse_down(
            gpui::MouseButton::Left,
            move |ev: &gpui::MouseDownEvent, window, cx| {
                let add = crate::keymap::secondary(&ev.modifiers);
                shell
                    .update(cx, |this, cx| {
                        this.with_pane(pane, |this| {
                            this.run(
                                "ui.select_group",
                                serde_json::json!({ "group": group, "add": add }),
                                window,
                                cx,
                            );
                            // The keys go where the press was, as on an entry.
                            this.stop_on_heading(group, cx);
                        });
                    })
                    .ok();
            },
        )
        .on_mouse_down(
            gpui::MouseButton::Right,
            move |ev: &gpui::MouseDownEvent, window, cx| {
                // Its menu, not the folder's the pane opens under it.
                cx.stop_propagation();
                menu.update(cx, |this, cx| {
                    this.activate_pane(pane, cx);
                    window.focus(&this.ui().focus, cx);
                    this.with_pane(pane, |this| {
                        this.open_group_menu(group, ev.position, window, cx);
                    });
                })
                .ok();
            },
        )
}

/// A heading as a screen reader hears it, where one listens: a heading
/// among the options, named by its group and count. It is never an
/// option: the entries' positions and count are their own.
pub fn expose(
    el: gpui::Stateful<Div>,
    span: &Span,
    stopped: bool,
    a11y: bool,
) -> gpui::Stateful<Div> {
    if !a11y {
        return el;
    }
    // Folded, its span holds none of its entries; the one the keys are on
    // is where they are, as the cursor's entry is otherwise.
    el.role(gpui::Role::Heading)
        .aria_level(2)
        .aria_label(said(span))
        .aria_expanded(span.len > 0)
        .when(
            stopped,
            gpui::StatefulInteractiveElement::aria_active_descendant,
        )
}

/// The cursor's ring around a heading the keys are on, drawn over it as
/// an entry's is (`entry::highlight`): in the accent, a mark.
pub fn ring(el: gpui::Stateful<Div>, on: bool, theme: &Theme) -> gpui::Stateful<Div> {
    if !on {
        return el;
    }
    el.relative().child(
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .border_1()
            .border_color(theme.accent),
    )
}

/// The heading pinned at the top of a view, `height` tall: on the files'
/// `ground`, a line under it, over what scrolls beneath, which it hides
/// from the pointer.
pub fn pinned(
    span: &Span,
    height: f32,
    left: Option<f32>,
    scale: Scale,
    theme: &Theme,
    ground: gpui::Hsla,
    chevron: Option<AnyElement>,
) -> Div {
    let heading = heading(span, true, height, left, scale, theme, chevron);
    pinned_frame(heading, height, theme, ground)
}

/// The heading of a group no file is in yet (`fresh_heading`) pinned at
/// the top of a view, as `pinned` pins one of the pane's.
pub fn pinned_fresh(
    fresh: &Fresh,
    height: f32,
    left: Option<f32>,
    scale: Scale,
    theme: &Theme,
    ground: gpui::Hsla,
) -> Div {
    let heading = fresh_heading(fresh, true, height, left, scale, theme);
    pinned_frame(heading, height, theme, ground)
}

/// A heading `height` tall on the files' `ground`, a line under it, over
/// what scrolls beneath, which it hides from the pointer.
fn pinned_frame(heading: Div, height: f32, theme: &Theme, ground: gpui::Hsla) -> Div {
    div()
        .relative()
        .w_full()
        .h(px(height))
        .bg(ground)
        .occlude()
        .child(heading)
        .child(
            div()
                .absolute()
                .bottom_0()
                .left_0()
                .w_full()
                .h(px(1.))
                .bg(theme.border),
        )
}

/// The heading of the group at the top of a view of rows, kept there while
/// its entries scroll under it and pushed up by the next as it arrives,
/// on the files' ground with a line under it. Computed where the list
/// lays its rows out, after it scrolled: never a frame late.
pub struct Pinned {
    pub rows: RowMap,
    pub scale: Scale,
    pub theme: Theme,
    pub ground: gpui::Hsla,
    /// The group whose heading the keys are on: ringed.
    pub stopped: Option<usize>,
    /// Where the keyboard's menu hangs (`Shell::host_row_anchor`): from
    /// the heading the keys are on, pinned.
    pub anchor: Option<crate::elements::popover::PopoverAnchor>,
    /// Whether a screen reader listens: the pinned heading is heard where
    /// its row is not drawn, the one the keys are on said so.
    pub a11y: bool,
    /// Whose heading it is, for what it does (`acting`).
    pub pane: skwir_core::workspace::PaneId,
    pub shell: gpui::WeakEntity<crate::shell::Shell>,
}

impl UniformListDecoration for Pinned {
    fn compute(
        &self,
        visible_range: Range<usize>,
        _bounds: Bounds<Pixels>,
        scroll_offset: Point<Pixels>,
        item_height: Pixels,
        _item_count: usize,
        _window: &mut Window,
        _cx: &mut App,
    ) -> AnyElement {
        let top = visible_range.start;
        // The group drawn at the top: one of the pane's, or one no file is
        // in yet, which is pinned and pushed up as theirs are.
        let Some(d) = self.rows.drawn_at(top) else {
            return div().into_any_element();
        };
        let into = -scroll_offset.y - item_height * top as f32;
        // A heading wholly in its place is drawn there already.
        let own_row = self.rows.drawn_row(d);
        if own_row == Some(top) && into < px(0.5) {
            return div().into_any_element();
        }
        // The next heading, arriving, pushes this one up.
        let push = if self.rows.drawn_row(d + 1) == Some(top + 1) {
            into.min(item_height)
        } else {
            Pixels::ZERO
        };
        let height = f32::from(item_height);
        // Laid out as a root, an element's own offset is not kept: the
        // heading hangs in a frame the size of the list, where the view's
        // top is.
        let hang = |heading: gpui::Stateful<Div>| {
            div()
                .size_full()
                .child(heading.absolute().top(-scroll_offset.y - push).left_0())
                .into_any_element()
        };
        let n = match self.rows.drawn().get(d) {
            Some(Drawn::Group(n)) => *n,
            Some(Drawn::Fresh(k)) => {
                let Some(fresh) = self.rows.fresh().get(*k) else {
                    return div().into_any_element();
                };
                let heading =
                    pinned_fresh(fresh, height, None, self.scale, &self.theme, self.ground)
                        .id(("pinned-fresh", *k));
                return hang(heading);
            }
            None => return div().into_any_element(),
        };
        // Its own row not drawn (scrolled above), it is the heading heard,
        // and the keyboard's menu hangs from it.
        let alone = own_row != Some(top);
        let stopped = self.stopped == Some(n);
        let chevron = chevron(
            n,
            self.rows.folded(n),
            self.pane,
            self.shell.clone(),
            self.scale,
            &self.theme,
        );
        let heading = pinned(
            &self.rows.spans()[n],
            height,
            None,
            self.scale,
            &self.theme,
            self.ground,
            Some(chevron),
        )
        .id(("pinned-heading", n))
        .children(
            self.anchor
                .as_ref()
                .filter(|_| stopped)
                .map(crate::elements::popover::PopoverAnchor::marker),
        );
        let heading = ring(heading, stopped, &self.theme);
        let heading = acting(heading, n, self.pane, self.shell.clone(), &self.theme);
        let span = &self.rows.spans()[n];
        hang(expose(heading, span, stopped, self.a11y && alone))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(start: usize, len: usize) -> Span {
        Span {
            start,
            len,
            count: len,
            group: None,
        }
    }

    /// Groups of 2, 1 and 3 entries: rows 0 H, 1 e0, 2 e1, 3 H, 4 e2,
    /// 5 H, 6 e3, 7 e4, 8 e5.
    fn three() -> RowMap {
        RowMap::new(Arc::from([span(0, 2), span(2, 1), span(3, 3)]), 6)
    }

    #[test]
    fn each_row_is_a_heading_or_its_entry() {
        let rows = three();
        assert_eq!(rows.len(), 9);
        let all: Vec<Row> = (0..9).map(|r| rows.at(r).unwrap()).collect();
        assert_eq!(
            all,
            [
                Row::Heading(0),
                Row::Entry(0),
                Row::Entry(1),
                Row::Heading(1),
                Row::Entry(2),
                Row::Heading(2),
                Row::Entry(3),
                Row::Entry(4),
                Row::Entry(5),
            ]
        );
        assert_eq!(rows.at(9), None);
        for position in 0..6 {
            assert_eq!(rows.at(rows.row_of(position)), Some(Row::Entry(position)));
        }
        assert_eq!(rows.drawn_at(4), Some(1));
        assert_eq!(rows.group_of(4), Some(2));
    }

    #[test]
    fn not_grouped_a_row_is_its_position() {
        let rows = RowMap::flat(4);
        assert!(!rows.grouped());
        assert_eq!(rows.len(), 4);
        assert_eq!(rows.at(2), Some(Row::Entry(2)));
        assert_eq!(rows.row_of(3), 3);
        assert_eq!(rows.entries_in(1..3), 1..3);
        assert_eq!(rows.drawn_at(1), None);
        // Groups read apart from the order, which they do not cover:
        // nothing grouped.
        assert!(!RowMap::new(Arc::from([span(0, 2)]), 3).grouped());
        assert!(!RowMap::new(Arc::from([span(1, 2)]), 3).grouped());
        assert!(RowMap::new(Arc::from([span(0, 3)]), 3).grouped());
    }

    /// A folded group's heading is a row with no entries after it, its
    /// count kept: rows 0 H, 1 e0, 2 e1, 3 H folded, 4 H, 5 e2.
    #[test]
    fn a_folded_group_is_its_heading_alone() {
        let folded = Span {
            start: 2,
            len: 0,
            count: 3,
            group: None,
        };
        let rows = RowMap::new(Arc::from([span(0, 2), folded.clone(), span(2, 1)]), 3);
        assert!(rows.grouped());
        assert_eq!(rows.len(), 6);
        assert_eq!(rows.at(3), Some(Row::Heading(1)));
        assert_eq!(rows.at(4), Some(Row::Heading(2)));
        assert_eq!(rows.at(5), Some(Row::Entry(2)));
        assert_eq!(rows.row_of(2), 5);
        assert_eq!(rows.group_of(2), Some(2));
        assert_eq!(rows.entry_from(3), Some(2), "the next shown");
        assert_eq!(rows.entries_in(3..5), 2..2);
        assert!(rows.folded(1) && !rows.folded(0));
        // Folded last: nothing below it; a page landing there goes to the
        // last entry.
        let last = RowMap::new(Arc::from([span(0, 2), folded.clone()]), 2);
        assert_eq!(last.entry_from(3), None);
        assert_eq!(last.entry_near(3), Some(1));
        assert_eq!(last.entry_near(9), Some(1), "past the rows");
        assert_eq!(last.entry_near(0), Some(0));
        assert_eq!(last.anchor(3. * 20., 20.).map(|a| a.0), Some(1));
        // Every group folded: their headings still, the order empty.
        let shut = Span { start: 0, ..folded };
        let all = RowMap::new(Arc::from([shut.clone(), shut]), 0);
        assert!(all.grouped(), "drawn, not an empty folder");
        assert_eq!(all.len(), 2);
        assert_eq!(all.at(1), Some(Row::Heading(1)));
        assert_eq!(all.entry_near(1), None);
        // A span of nothing that counts nothing is no group.
        assert!(!RowMap::new(Arc::from([span(0, 2), span(2, 0)]), 2).grouped());
    }

    fn placed(at: usize, group: Option<usize>) -> Placed {
        Placed {
            at,
            group,
            fresh: None,
        }
    }

    /// What arrives is a row before the entry it goes before, those before
    /// the same entry in the order given, one past the last at the end; a
    /// tile alike: rows 0 e0, 1 A0, 2 A2, 3 e1, 4 e2, 5 e3, 6 A1.
    #[test]
    fn what_arrives_is_a_row_and_a_tile_where_it_lands() {
        let rows = RowMap::arriving(
            Arc::from([]),
            4,
            &[placed(1, None), placed(4, None), placed(1, None)],
            &[],
        );
        assert!(!rows.plain() && rows.arrives() && !rows.grouped());
        assert_eq!(rows.len(), 7);
        let all: Vec<Row> = (0..7).map(|r| rows.at(r).unwrap()).collect();
        assert_eq!(
            all,
            [
                Row::Entry(0),
                Row::Arriving(0),
                Row::Arriving(2),
                Row::Entry(1),
                Row::Entry(2),
                Row::Entry(3),
                Row::Arriving(1),
            ]
        );
        for position in 0..4 {
            assert_eq!(rows.at(rows.row_of(position)), Some(Row::Entry(position)));
            assert_eq!(
                rows.at_tile(rows.tile_of(position)),
                Some(Tile::Entry(position))
            );
        }
        assert_eq!(rows.entry_from(1), Some(1), "the entry it goes before");
        assert_eq!(rows.entry_from(6), None, "none after the last");
        assert_eq!(rows.entry_near(6), Some(3), "a page there: the last");
        assert_eq!(rows.entries_in(0..3), 0..1);
        assert_eq!(rows.entries_in(2..6), 1..4);
        assert_eq!(rows.tiles(), 7);
        assert_eq!(rows.at_tile(2), Some(Tile::Arriving(2)));
        assert_eq!(rows.entries_in_tiles(1..4), 1..2);
        assert_eq!(rows.entry_of_tile(1), Some(1));
        // Nothing arriving: as before.
        assert!(RowMap::arriving(Arc::from([]), 4, &[], &[]).plain());
    }

    /// Grouped, what arrives is in its group, before its entry or at its
    /// end, and in none shown after them all; a grid's tiles count it in
    /// its group: rows 0 H, 1 e0, 2 A0, 3 e1, 4 H, 5 e2, 6 A1, 7 H, 8 e3,
    /// 9 e4, 10 e5, 11 A2.
    #[test]
    fn grouped_what_arrives_is_in_its_group() {
        let rows = RowMap::arriving(
            Arc::from([span(0, 2), span(2, 1), span(3, 3)]),
            6,
            &[placed(1, Some(0)), placed(3, Some(1)), placed(0, None)],
            &[],
        );
        assert_eq!(rows.len(), 12);
        assert_eq!(rows.at(2), Some(Row::Arriving(0)));
        assert_eq!(rows.at(4), Some(Row::Heading(1)));
        assert_eq!(rows.at(6), Some(Row::Arriving(1)), "its group's end");
        assert_eq!(rows.at(7), Some(Row::Heading(2)));
        assert_eq!(rows.at(11), Some(Row::Arriving(2)), "after every group");
        assert_eq!(rows.heading_row(2), 7);
        assert_eq!(rows.row_of(2), 5);
        assert_eq!(rows.row_of(5), 10);
        assert_eq!(rows.drawn_at(6), Some(1));
        assert_eq!(rows.entry_from(6), Some(3));
        assert_eq!(rows.entries_in(4..8), 2..3);
        assert_eq!(rows.tile_lens(), [3, 2, 4]);
        assert_eq!(rows.at_tile(4), Some(Tile::Arriving(1)));
        assert_eq!(rows.tile_of(3), 5);
        assert_eq!(rows.entries_in_tiles(0..5), 0..3);
        // A view scrolled to what arrives comes back to the entry after it.
        let h = 20.;
        assert_eq!(rows.anchor(2. * h + 3., h).map(|a| a.0), Some(1));
    }

    /// What arrives in a group no file is in yet has its own heading,
    /// drawn where that group stands among the others, which keep their
    /// numbers: rows 0 H0, 1 e0, 2 e1, 3 F0, 4 A0, 5 H1, 6 e2.
    #[test]
    fn a_group_no_file_is_in_yet_has_its_own_heading() {
        let fresh = Fresh {
            before: 1,
            group: Arc::new(skwir_core::grouping::Group::new(5, "Documents")),
        };
        let arrives = Placed {
            at: 2,
            group: None,
            fresh: Some(0),
        };
        let rows = RowMap::arriving(
            Arc::from([span(0, 2), span(2, 1)]),
            3,
            &[arrives],
            std::slice::from_ref(&fresh),
        );
        assert_eq!(rows.len(), 7);
        let all: Vec<Row> = (0..7).map(|r| rows.at(r).unwrap()).collect();
        assert_eq!(
            all,
            [
                Row::Heading(0),
                Row::Entry(0),
                Row::Entry(1),
                Row::Fresh(0),
                Row::Arriving(0),
                Row::Heading(1),
                Row::Entry(2),
            ]
        );
        assert_eq!(
            rows.heading_row(1),
            5,
            "the pane's groups keep their numbers"
        );
        assert_eq!(rows.row_of(2), 6);
        assert_eq!(
            rows.drawn(),
            [Drawn::Group(0), Drawn::Fresh(0), Drawn::Group(1)]
        );
        assert_eq!(
            (rows.drawn_of(0), rows.drawn_of(1), rows.drawn_of(2)),
            (Some(0), Some(2), None)
        );
        assert_eq!(rows.tile_lens(), [2, 1, 1]);
        assert_eq!(rows.at_tile(2), Some(Tile::Arriving(0)));
        assert_eq!(rows.fresh()[0].group.label.as_ref(), "Documents");
    }

    /// What arrives in a folded group is not drawn: its heading alone.
    #[test]
    fn what_arrives_in_a_folded_group_is_not_drawn() {
        let folded = Span {
            start: 2,
            len: 0,
            count: 3,
            group: None,
        };
        let rows = RowMap::arriving(
            Arc::from([span(0, 2), folded]),
            2,
            &[placed(2, Some(1)), placed(1, Some(0))],
            &[],
        );
        assert_eq!(rows.len(), 5);
        assert_eq!(rows.at(2), Some(Row::Arriving(1)));
        assert_eq!(rows.at(4), Some(Row::Heading(1)));
        assert_eq!(rows.tile_lens(), [3, 0]);
    }

    /// A fold keeps the view on what was at its top: an entry where it is
    /// now, a heading where it is now, an entry folded away its group's
    /// heading. A, B and C of 2, 1 and 3, then B folded: rows 0 A, 1 e0,
    /// 2 e1, 3 B, 4 C, 5 e3, 6 e4, 7 e5.
    #[test]
    fn a_fold_keeps_what_was_at_the_top() {
        use skwir_core::{EntryId, FileId, grouping::Group};
        let id = |ino| EntryId {
            file: FileId::new(1, ino),
            at: ino,
        };
        let named = |start, len, count, rank, label| Span {
            start,
            len,
            count,
            group: Some(Arc::new(Group::new(rank, label))),
        };
        let old = RowMap::new(
            Arc::from([
                named(0, 2, 2, 1, "A"),
                named(2, 1, 1, 2, "B"),
                named(3, 3, 3, 3, "C"),
            ]),
            6,
        );
        let old_order: Vec<EntryId> = (0..6).map(id).collect();
        let new = RowMap::new(
            Arc::from([
                named(0, 2, 2, 1, "A"),
                named(2, 0, 1, 2, "B"),
                named(2, 3, 3, 3, "C"),
            ]),
            5,
        );
        let new_order: Vec<EntryId> = [0, 1, 3, 4, 5].into_iter().map(id).collect();
        let position_of = |want: EntryId| new_order.iter().position(|id| *id == want);
        let kept = |row| new.kept_row(&old, &old_order, row, position_of);
        assert_eq!(kept(4), Some(3), "e2 folded away: B's heading");
        assert_eq!(kept(6), Some(5), "e3 where it is now");
        assert_eq!(kept(5), Some(4), "C's heading");
        assert_eq!(kept(1), Some(1));
        assert_eq!(kept(9), None, "past the rows");
    }

    #[test]
    fn rows_drawn_hold_the_entries_between_their_headings() {
        let rows = three();
        assert_eq!(rows.entries_in(0..9), 0..6);
        assert_eq!(rows.entries_in(0..1), 0..0, "a heading alone");
        assert_eq!(rows.entries_in(3..6), 2..3, "heading, entry, heading");
        assert_eq!(rows.entries_in(2..5), 1..3);
        assert_eq!(rows.entries_in(7..20), 4..6);
        assert_eq!(rows.entries_in(9..12), 0..0);
        assert_eq!(rows.entry_from(3), Some(2), "a heading's first entry");
        assert_eq!(rows.entry_from(8), Some(5));
        assert_eq!(rows.entry_from(9), None);
    }

    #[test]
    fn a_view_scrolled_comes_back_to_its_entry_or_heading() {
        let rows = three();
        let h = 20.;
        // Into entry 2's row (row 4).
        let anchor = rows.anchor(4. * h + 7., h).unwrap();
        assert_eq!(anchor, (2, 7.));
        assert!((rows.offset_of(anchor, h) - (4. * h + 7.)).abs() < 1e-3);
        // Into the third heading (row 5): told from the entry under it.
        let anchor = rows.anchor(5. * h + 3., h).unwrap();
        assert_eq!(anchor.0, 3);
        assert!((anchor.1 - (3. - h)).abs() < 1e-3);
        assert!((rows.offset_of(anchor, h) - (5. * h + 3.)).abs() < 1e-3);
        // At the top.
        assert_eq!(rows.anchor(0., h), Some((0, -h)));
        assert!(rows.offset_of((0, -h), h).abs() < 1e-3);
        // The same entry in other groups: where it is now.
        let other = RowMap::new(Arc::from([span(0, 3), span(3, 3)]), 6);
        assert!((other.offset_of((2, 7.), h) - (3. * h + 7.)).abs() < 1e-3);
    }
}
