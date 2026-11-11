//! A pane's groups as a view of rows draws them (ADR-130, 9): a heading
//! is a row, never an entry. `RowMap` says where each row of the view is
//! in the pane's order, made once per order from the pane's groups
//! (`Pane::groups`). Drawing, scrolling and the pointer work in rows;
//! everything else (the cursor, ranges, Select All, the rubber band's
//! selection, a screen reader's position and count) in positions, as
//! when nothing is grouped.

use std::ops::Range;
use std::sync::Arc;

use gpui::{
    AnyElement, App, Bounds, Div, Pixels, Point, UniformListDecoration, Window, div, prelude::*, px,
};
use skwir_core::grouping::Span;

use crate::sizes::{Scale, TextSizes};
use crate::theme::Theme;

/// What a row of a view that groups is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    /// The heading of the `n`th group.
    Heading(usize),
    /// The entry at this position of the order.
    Entry(usize),
}

/// Where each row of a view is in the order it draws: the entries of each
/// group under its heading. Not grouped, row and position are one.
/// Every question is a search among the groups, never a walk of the rows.
#[derive(Clone, Debug, Default)]
pub struct RowMap {
    /// Empty while not grouped.
    spans: Arc<[Span]>,
    entries: usize,
}

impl RowMap {
    /// The rows of an order of `entries` grouped as `spans` say. Spans that
    /// do not cover the order from its start, one after the other (an
    /// order and groups read apart), group nothing. A folded group's span
    /// holds none of them (ADR-131, 2): its heading, with its count.
    pub fn new(spans: Arc<[Span]>, entries: usize) -> Self {
        let mut at = 0;
        let whole = spans.iter().all(|span| {
            let follows = span.start == at && (span.len > 0 || span.count > 0);
            at += span.len;
            follows
        }) && at == entries;
        RowMap {
            spans: if whole { spans } else { Arc::from([]) },
            entries,
        }
    }

    /// An order of `entries` not grouped.
    pub fn flat(entries: usize) -> Self {
        RowMap {
            spans: Arc::from([]),
            entries,
        }
    }

    pub fn grouped(&self) -> bool {
        !self.spans.is_empty()
    }

    /// Which groups these are, for what is laid out from them to be kept
    /// while they are the same: the groups of another order are another
    /// list of them.
    pub fn key(&self) -> (usize, usize) {
        (
            Arc::as_ptr(&self.spans).cast::<()>().addr(),
            self.spans.len(),
        )
    }

    /// How many rows there are: the entries and the headings.
    pub fn len(&self) -> usize {
        self.entries + self.spans.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn spans(&self) -> &[Span] {
        &self.spans
    }

    /// The row of group `n`'s heading.
    pub fn heading_row(&self, n: usize) -> usize {
        self.spans[n].start + n
    }

    /// The group whose heading or entries `row` is, the last whose
    /// heading is not below it.
    pub fn group_at(&self, row: usize) -> Option<usize> {
        if self.spans.is_empty() || row >= self.len() {
            return None;
        }
        // Headings' rows grow with their group: the last at or above.
        let (mut lo, mut hi) = (0, self.spans.len());
        while hi - lo > 1 {
            let mid = lo + (hi - lo) / 2;
            if self.heading_row(mid) <= row {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        Some(lo)
    }

    /// What `row` is.
    pub fn at(&self, row: usize) -> Option<Row> {
        if row >= self.len() {
            return None;
        }
        let Some(n) = self.group_at(row) else {
            return Some(Row::Entry(row));
        };
        if row == self.heading_row(n) {
            Some(Row::Heading(n))
        } else {
            // Above it, the headings of the groups up to its own.
            Some(Row::Entry(row - n - 1))
        }
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

    /// The row of the entry at `position`.
    pub fn row_of(&self, position: usize) -> usize {
        match self.group_of(position) {
            Some(n) => position + n + 1,
            None => position,
        }
    }

    /// The first entry at `row` or below it (a heading's, its group's
    /// first, or a folded group's the next shown), none past the last.
    pub fn entry_from(&self, row: usize) -> Option<usize> {
        match self.at(row)? {
            Row::Entry(position) => Some(position),
            Row::Heading(n) => Some(self.spans[n].start).filter(|at| *at < self.entries),
        }
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
        let end = match self.at(rows.end - 1) {
            Some(Row::Entry(position)) => position + 1,
            Some(Row::Heading(n)) => self.spans[n].start,
            None => self.entries,
        };
        start..end.max(start)
    }

    /// The entry at the top of a view scrolled to `offset` (pixels down)
    /// over rows `height` tall, and how far above the view begins from
    /// that entry's row: a heading at the top is told by a negative
    /// distance, so that the view comes back to it.
    pub fn anchor(&self, offset: f32, height: f32) -> Option<(usize, f32)> {
        let row = (offset.max(0.) / height).floor() as usize;
        let within = offset.max(0.) - row as f32 * height;
        match self.at(row)? {
            Row::Entry(position) => Some((position, within)),
            Row::Heading(n) => Some((
                self.spans[n].start.min(self.entries.saturating_sub(1)),
                within - height,
            )),
        }
    }

    /// The offset (pixels down) that puts `anchor` back at the top.
    pub fn offset_of(&self, (position, within): (usize, f32), height: f32) -> f32 {
        let position = position.min(self.entries.saturating_sub(1));
        (self.row_of(position) as f32 * height + within.clamp(-height, height - 1.)).max(0.)
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

/// The heading of a group as a row `height` tall: its label in Small text,
/// the count beside it in the muted ink (ADR-117), a line above it unless
/// it is the first, set on the files' ground as rows are, after its
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
    let words = div()
        .min_w_0()
        .flex()
        .items_baseline()
        .gap(scale.px(8.))
        .children(chevron)
        .child(
            div()
                .min_w_0()
                .truncate()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(theme.text)
                .child(label(span).to_string()),
        )
        .child(
            div()
                .flex_none()
                .text_color(theme.text_muted)
                .child(span.count.to_string()),
        );
    div()
        .h(px(height))
        .w_full()
        .flex_none()
        .flex()
        .items_end()
        .pb(scale.px(5.))
        .when(!first, |d| d.border_t_1().border_color(theme.border))
        .text_small()
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
    div()
        .relative()
        .w_full()
        .h(px(height))
        .bg(ground)
        .occlude()
        .child(heading(span, true, height, left, scale, theme, chevron))
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
        let Some(n) = self.rows.group_at(top) else {
            return div().into_any_element();
        };
        let into = -scroll_offset.y - item_height * top as f32;
        // A heading wholly in its place is drawn there already.
        if top == self.rows.heading_row(n) && into < px(0.5) {
            return div().into_any_element();
        }
        // The next heading, arriving, pushes this one up.
        let push = if self.rows.spans().len() > n + 1 && self.rows.heading_row(n + 1) == top + 1 {
            into.min(item_height)
        } else {
            Pixels::ZERO
        };
        let height = f32::from(item_height);
        // Laid out as a root, an element's own offset is not kept: the
        // heading hangs in a frame the size of the list, where the view's
        // top is.
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
        .id(("pinned-heading", n));
        let heading = ring(heading, self.stopped == Some(n), &self.theme);
        div()
            .size_full()
            .child(
                acting(heading, n, self.pane, self.shell.clone(), &self.theme)
                    .absolute()
                    .top(-scroll_offset.y - push)
                    .left_0(),
            )
            .into_any_element()
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
        assert_eq!(rows.group_at(4), Some(1));
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
        assert_eq!(rows.group_at(1), None);
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
        // Folded last: nothing below it.
        let last = RowMap::new(Arc::from([span(0, 2), folded]), 2);
        assert_eq!(last.entry_from(3), None);
        assert_eq!(last.anchor(3. * 20., 20.).map(|a| a.0), Some(1));
        // A span of nothing that counts nothing is no group.
        assert!(!RowMap::new(Arc::from([span(0, 2), span(2, 0)]), 2).grouped());
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
