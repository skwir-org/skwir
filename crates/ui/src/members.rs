//! The rows of the container a member was opened from, kept beside the
//! member: while a file of an archive, or a message's attachment, is
//! previewed, the others stay one click or one arrow away instead of behind
//! Back. The rows are the container's inspection as it was shown (its folds,
//! its order), shared, never copied; the member shown is highlighted, and
//! another one is opened through `ui.inspect` (`sibling`, `sibling_step`,
//! `sibling_fold`) like any member (`preview::Nested`).
//!
//! The member is previewed at the right of the rows or under them, as
//! More's Layout says (`layout::Member`), and the edge between them is
//! dragged like a split's: it stores the rows' share of the room, part of
//! the window's shape (`session.rs`), and the drag reads the bounds GPUI
//! hands it, so nothing is measured.

use crate::{archive_preview, inspector::Display, keymap, shell::Shell, theme::Theme};
use bibli_core::inspection::Inspection;
use gpui::{
    AnyElement, Context, DragMoveEvent, FocusHandle, ScrollStrategy, UniformListScrollHandle,
    Window, div, prelude::*, px, relative, uniform_list,
};
use std::{collections::BTreeSet, path::Path, sync::Arc};

/// The rows' share of the room until their edge is dragged, and again after
/// a double click on it: the member is what is looked at.
pub const SHARE: f32 = 0.4;
/// What each side keeps, whatever the drag: a few rows, or a column wide
/// enough for a name, and as much of the member's preview.
const KEEP_HEIGHT: f32 = crate::views::list::ROW_HEIGHT * 3.;
const KEEP_WIDTH: f32 = 120.;

/// The rows' share of the room, one for each way the edge runs: dragged one
/// way, it says nothing of the other.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Share {
    /// Above the member.
    pub above: f32,
    /// At its left.
    pub beside: f32,
}

impl Default for Share {
    fn default() -> Self {
        Self {
            above: SHARE,
            beside: SHARE,
        }
    }
}

impl Share {
    fn get(self, beside: bool) -> f32 {
        if beside { self.beside } else { self.above }
    }

    fn set(&mut self, beside: bool, share: f32) {
        *if beside {
            &mut self.beside
        } else {
            &mut self.above
        } = share;
    }
}

/// Marker for dragging the edge between the rows and the member; it knows
/// which way the edge runs.
#[derive(Clone)]
pub struct MembersResize {
    beside: bool,
}

pub struct Members {
    data: Arc<Inspection>,
    icons: Arc<Vec<Option<Arc<Path>>>>,
    collapsed: Arc<BTreeSet<usize>>,
    visible: Arc<Vec<usize>>,
    /// How deep the member opened from these rows is: its index in
    /// `PreviewState::nested`.
    pub depth: usize,
    /// The target of the member shown, or asked for.
    current: String,
    pub focus: FocusHandle,
    scroll: UniformListScrollHandle,
}

impl Members {
    pub fn new(
        display: &Display,
        collapsed: Arc<BTreeSet<usize>>,
        visible: Arc<Vec<usize>>,
        depth: usize,
        current: &str,
        cx: &mut gpui::App,
    ) -> Self {
        let members = Self {
            data: display.data.clone(),
            icons: display.row_icons.clone(),
            collapsed,
            visible,
            depth,
            current: current.to_owned(),
            focus: cx.focus_handle(),
            scroll: UniformListScrollHandle::new(),
        };
        if let Some(position) = members.position() {
            members
                .scroll
                .scroll_to_item(position, ScrollStrategy::Center);
        }
        members
    }

    /// Where the member shown is among the rows shown.
    fn position(&self) -> Option<usize> {
        position(&self.data, &self.visible, &self.current)
    }

    /// The member `delta` rows after the one shown, among the rows shown
    /// that are members: folders and what cannot be opened are passed.
    pub fn step(&self, delta: isize) -> Option<String> {
        let row = step(&self.data, &self.visible, &self.current, delta)?;
        self.data.rows[row].target.clone()
    }

    /// Another member is shown: highlight it, and keep it in view.
    pub fn show(&mut self, target: &str) {
        let before = self.position();
        target.clone_into(&mut self.current);
        if let Some(position) = self.position() {
            // Only when it is out of view, and by as little as it takes: a
            // step down brings it to the bottom edge, a step up to the top.
            let strategy = if before.is_some_and(|before| position > before) {
                ScrollStrategy::Bottom
            } else {
                ScrollStrategy::Top
            };
            self.scroll.scroll_to_item(position, strategy);
        }
    }

    /// Open or close a folder of the rows, as the container's view does.
    pub fn fold(&mut self, row: usize) {
        if !self.data.rows.get(row).is_some_and(|r| r.branch) {
            return;
        }
        let collapsed = Arc::make_mut(&mut self.collapsed);
        if !collapsed.remove(&row) {
            collapsed.insert(row);
        }
        self.visible = Arc::new(crate::inspector::visible_rows(
            &self.data,
            &self.collapsed,
            None,
        ));
    }
}

fn position(data: &Inspection, visible: &[usize], current: &str) -> Option<usize> {
    visible
        .iter()
        .position(|&row| data.rows[row].target.as_deref() == Some(current))
}

/// The row `delta` members after `current` among `visible`, or `None` at
/// either end. From a member that is no longer shown (its folder was
/// closed), the first step lands on the nearest member in that direction.
fn step(data: &Inspection, visible: &[usize], current: &str, delta: isize) -> Option<usize> {
    let members: Vec<(usize, usize)> = visible
        .iter()
        .enumerate()
        .filter(|&(_, &row)| !data.rows[row].branch && data.rows[row].target.is_some())
        .map(|(position, &row)| (position, row))
        .collect();
    let at = members
        .iter()
        .position(|&(_, row)| data.rows[row].target.as_deref() == Some(current));
    let index = if let Some(at) = at {
        at.checked_add_signed(delta)?
    } else {
        // Where the member would be: the members before it in the file's
        // order are those before it among the rows shown.
        let row = data
            .rows
            .iter()
            .position(|r| r.target.as_deref() == Some(current))?;
        let before = members.iter().filter(|&&(_, shown)| shown < row).count();
        if delta > 0 {
            before + delta.unsigned_abs() - 1
        } else {
            before.checked_sub(delta.unsigned_abs())?
        }
    };
    members.get(index).map(|&(_, row)| row)
}

/// The panel's content under its header: the member's preview, with the
/// rows it was opened from above it or at its left, when it is a member of
/// a container whose rows are kept. `wide` is whether the room is wider
/// than tall (a panel at the bottom, peek), which Automatic follows.
pub fn split(body: AnyElement, wide: bool, shell: &Shell, cx: &mut Context<Shell>) -> AnyElement {
    let Some(members) = shell.preview.members() else {
        return body;
    };
    let beside = shell.layout.member.beside(wide);
    let share = shell.preview.members_share.get(beside);
    let theme = cx.global::<Theme>().clone();
    let rows = list(members, shell.pane, &theme, cx);
    let rows = div()
        .id("preview-members")
        .track_focus(&members.focus)
        .key_context(keymap::MEMBERS)
        .relative()
        .flex_none()
        .border_color(theme.border)
        .when(beside, |d| {
            d.w(relative(share))
                .h_full()
                .min_w(px(KEEP_WIDTH))
                .border_r_1()
        })
        .when(!beside, |d| {
            d.h(relative(share))
                .w_full()
                .min_h(px(KEEP_HEIGHT))
                .border_b_1()
        })
        .child(rows)
        .child(crate::elements::scrollbar::vertical(
            "preview-members-scrollbar",
            crate::elements::scrollbar::Scrolls::Offset(
                members.scroll.0.borrow().base_handle.clone(),
            ),
        ));
    // The body grew in the panel's column; it keeps a column of its own,
    // and room to be looked at however the panel narrows.
    let body = div()
        .relative()
        .flex_1()
        .when(beside, |d| d.min_w(px(KEEP_WIDTH)).min_h_0())
        .when(!beside, |d| d.min_h(px(KEEP_HEIGHT)).min_w_0())
        .flex()
        .flex_col()
        .child(body)
        .child(edge(beside, &theme, cx));
    div()
        .flex_1()
        .min_h_0()
        .min_w_0()
        .flex()
        .when(!beside, gpui::Styled::flex_col)
        .on_drag_move(cx.listener(Shell::on_members_drag))
        .child(rows)
        .child(body)
        .into_any_element()
}

/// The draggable edge at the start of the member's preview, over the
/// border that sets it apart from the rows.
fn edge(beside: bool, theme: &Theme, cx: &mut Context<Shell>) -> AnyElement {
    let edge = div()
        .id("preview-members-resize")
        .absolute()
        .top_0()
        .left_0()
        .hover({
            let hover = theme.handle;
            move |style| style.bg(hover)
        })
        .on_click(cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
            if event.click_count() == 2 {
                this.preview.members_share.set(beside, SHARE);
                this.remember_session();
                cx.notify();
            }
        }))
        .on_drag(MembersResize { beside }, |_, _, _, cx| {
            cx.new(|_| crate::preview::NoDragView)
        });
    if beside {
        edge.h_full()
            .w(px(6.))
            .cursor(gpui::CursorStyle::ResizeLeftRight)
    } else {
        edge.w_full()
            .h(px(6.))
            .cursor(gpui::CursorStyle::ResizeUpDown)
    }
    .into_any_element()
}

impl Shell {
    /// The edge between the rows and the member follows the pointer across
    /// the room they share: the bounds are those of their container. The
    /// root's mouse-up remembers where it was left (`resize_hook`).
    pub(crate) fn on_members_drag(
        &mut self,
        ev: &DragMoveEvent<MembersResize>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let beside = ev.drag(cx).beside;
        let (along, size, keep) = if beside {
            (
                ev.event.position.x - ev.bounds.left(),
                ev.bounds.size.width,
                KEEP_WIDTH,
            )
        } else {
            (
                ev.event.position.y - ev.bounds.top(),
                ev.bounds.size.height,
                KEEP_HEIGHT,
            )
        };
        if let Some(share) = share(f32::from(along), f32::from(size), keep) {
            self.preview.members_share.set(beside, share);
            cx.notify();
        }
    }
}

/// The rows' share when their edge is `along` px into `size` px of room,
/// each side keeping `keep` px; `None` in a room too small to share.
fn share(along: f32, size: f32, keep: f32) -> Option<f32> {
    if !size.is_finite() || size <= 2. * keep || !along.is_finite() {
        return None;
    }
    Some(along.clamp(keep, size - keep) / size)
}

fn list(
    members: &Members,
    pane: bibli_core::workspace::PaneId,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let data = members.data.clone();
    let icons = members.icons.clone();
    let collapsed = members.collapsed.clone();
    let visible = members.visible.clone();
    let current = members.current.clone();
    let focus = members.focus.clone();
    let theme = theme.clone();
    let entity = cx.entity().downgrade();
    uniform_list("preview-members-list", visible.len(), move |range, _, _| {
        range
            .map(|position| {
                let index = visible[position];
                let row = &data.rows[index];
                let shown = row.target.as_deref() == Some(current.as_str());
                let args = if row.branch {
                    serde_json::json!({ "action": "sibling_fold", "row": index })
                } else {
                    serde_json::json!({ "action": "sibling", "target": row.target })
                };
                let opens = row.branch || (row.target.is_some() && !shown);
                let (entity, focus) = (entity.clone(), focus.clone());
                archive_preview::row(
                    row,
                    icons.get(index).and_then(|icon| icon.as_deref()),
                    collapsed.contains(&index),
                    shown,
                    &theme,
                )
                .id(("preview-member", index))
                .when(shown, |d| d.bg(theme.selection_bg))
                .when(opens, gpui::Styled::cursor_pointer)
                .on_click(move |_, window, cx| {
                    // The rows take the keyboard, so the arrows go on from here.
                    window.focus(&focus);
                    if !opens {
                        return;
                    }
                    let _ = entity.update(cx, |shell, cx| {
                        shell.with_pane(pane, |shell| {
                            shell.run("ui.inspect", args.clone(), window, cx);
                        });
                    });
                })
                .into_any_element()
            })
            .collect()
    })
    .track_scroll(members.scroll.clone())
    .size_full()
    .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{position, share, step};

    #[test]
    fn the_edge_follows_the_pointer_and_leaves_each_side_its_room() {
        assert_eq!(share(300., 1000., 160.), Some(0.3));
        assert_eq!(share(50., 1000., 160.), Some(0.16));
        assert_eq!(share(990., 1000., 160.), Some(0.84));
        // Too small to share: the edge stays where it was.
        assert_eq!(share(100., 300., 160.), None);
        assert_eq!(share(f32::NAN, 1000., 160.), None);
    }
    use bibli_core::inspection::{Inspection, InspectionKind, Row};

    /// `docs/`, `docs/a.txt`, `docs/b.txt`, `locked.bin` (no target),
    /// `z.txt`: an archive's rows, folders first.
    fn archive() -> Inspection {
        let row = |name: &str, depth: usize, branch: bool, target: Option<&str>| Row {
            cells: vec![name.into()],
            depth,
            branch,
            target: target.map(str::to_owned),
            ..Row::default()
        };
        Inspection {
            kind: InspectionKind::Archive,
            rows: vec![
                row("docs", 0, true, None),
                row("a.txt", 1, false, Some("1")),
                row("b.txt", 1, false, Some("2")),
                row("locked.bin", 0, false, None),
                row("z.txt", 0, false, Some("4")),
            ],
            ..Inspection::default()
        }
    }

    #[test]
    fn a_step_goes_to_the_next_member_shown_passing_what_cannot_open() {
        let data = archive();
        let all = [0, 1, 2, 3, 4];
        assert_eq!(step(&data, &all, "1", 1), Some(2));
        // A file that cannot be opened is passed, and so is a folder.
        assert_eq!(step(&data, &all, "2", 1), Some(4));
        assert_eq!(step(&data, &all, "4", -1), Some(2));
        // Nothing past either end.
        assert_eq!(step(&data, &all, "4", 1), None);
        assert_eq!(step(&data, &all, "1", -1), None);
        assert_eq!(position(&data, &all, "2"), Some(2));
    }

    #[test]
    fn from_a_member_in_a_closed_folder_a_step_lands_beside_the_folder() {
        let data = archive();
        // `docs` closed: its files are not shown.
        let folded = [0, 3, 4];
        assert_eq!(position(&data, &folded, "2"), None);
        assert_eq!(step(&data, &folded, "2", 1), Some(4));
        assert_eq!(step(&data, &folded, "2", -1), None);
        assert_eq!(step(&data, &folded, "missing", 1), None);
    }
}
