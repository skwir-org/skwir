//! The view of the container a member was opened from, kept above the
//! member (or at its left): while a file of an archive, or a message's
//! attachment, is previewed, the archive or the message stays as it was
//! shown, its bar (Find, More), its folds, where it was scrolled, with the
//! member's row highlighted, and the member is previewed under it as a
//! supplement. The view is parked whole (`Parked`): its inspection and how
//! it was looked at, drawn by the inspector as before (`inspector::View`),
//! never read again; another member is opened from it like any member
//! (`ui.inspect` `target`, `preview::Nested`), and the member's × (or
//! Escape) gives the panel back to it as it was.
//!
//! Each side holds a focus handle of its own, so a press in one gives it
//! the keyboard, and a command acts on the side that has the keyboard
//! (`Shell::container_has_keyboard`).
//!
//! The member is previewed at the right of the container or under it, as
//! More's Layout says (`layout::Member`), and the edge between them is
//! dragged like a split's: it stores the container's share of the room,
//! part of the window's shape (`session.rs`), and the drag reads the bounds
//! GPUI hands it, so nothing is measured.
use crate::{inspector, keymap, shell::Shell, theme::Theme};
use gpui::{
    AnyElement, Context, DragMoveEvent, FocusHandle, Window, div, prelude::*, px, relative,
};

/// The container's share of the room until their edge is dragged, and again
/// after a double click on it: the member is what is looked at.
pub const SHARE: f32 = 0.4;
/// What each side keeps, whatever the drag: the container's bar, its strip,
/// its header and a few rows, or a column wide enough for a name, and as
/// much of the member's preview.
const KEEP_HEIGHT: f32 = crate::preview_bar::HEIGHT + crate::views::list::ROW_HEIGHT * 5.;
const KEEP_WIDTH: f32 = 160.;

/// The container's share of the room, one for each way the edge runs:
/// dragged one way, it says nothing of the other.
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

/// Marker for dragging the edge between the container and the member; it
/// knows which way the edge runs.
#[derive(Clone)]
pub struct MembersResize {
    beside: bool,
}

/// A container's view set aside while a member of it is shown.
pub struct Parked {
    pub(crate) display: inspector::Display,
    pub(crate) state: inspector::State,
    /// How deep the member opened from it is: its index in
    /// `PreviewState::nested`.
    pub depth: usize,
    /// Held by the container's side, whatever inside it has the keyboard.
    pub focus: FocusHandle,
}

/// The container's view drawn over the member shown, if any.
pub fn shown(shell: &Shell) -> Option<&Parked> {
    shell.preview.parked()
}

/// What the member's bar names it by: the name and size its container's
/// row gives the member shown (or on its way).
pub fn title(shell: &Shell) -> Option<(String, Option<String>)> {
    let parked = shown(shell)?;
    let target = shell.preview.member_target(parked.depth)?;
    let row = inspector::listed(&parked.display.data)
        .iter()
        .find(|row| row.target.as_deref() == Some(target))?;
    Some((
        row.cells.first()?.clone(),
        row.bytes.map(crate::format::size),
    ))
}

/// The panel's content under its header: the member's preview, with the
/// view of the container it was opened from above it or at its left. `wide`
/// is whether the room is wider than tall (a panel at the bottom, peek),
/// which Automatic follows.
pub fn split(body: AnyElement, wide: bool, shell: &Shell, cx: &mut Context<Shell>) -> AnyElement {
    let Some(parked) = shown(shell) else {
        return body;
    };
    let beside = shell.layout.member.beside(wide);
    let share = shell.preview.members_share.get(beside);
    let theme = cx.global::<Theme>().clone();
    let container = div()
        .id("preview-container")
        .track_focus(&parked.focus)
        .flex_none()
        .flex()
        .flex_col()
        .overflow_hidden()
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
        .child(inspector::parked(shell, &theme, cx));
    // The body grew in the panel's column; it keeps a column of its own,
    // and room to be looked at however the panel narrows. Its focus handle
    // is what a press in it gives the keyboard to, when nothing inside
    // takes it.
    let member = div()
        .id("preview-member")
        .when_some(shell.preview.member_focus.as_ref(), |d, focus| {
            d.track_focus(focus)
        })
        .relative()
        .flex_1()
        .when(beside, |d| d.min_w(px(KEEP_WIDTH)).min_h_0())
        .when(!beside, |d| d.min_h(px(KEEP_HEIGHT)).min_w_0())
        .flex()
        .flex_col()
        .child(body)
        .child(edge(beside, cx));
    div()
        .key_context(keymap::MEMBER)
        .flex_1()
        .min_h_0()
        .min_w_0()
        .flex()
        .when(!beside, gpui::Styled::flex_col)
        .on_drag_move(cx.listener(Shell::on_members_drag))
        .child(container)
        .child(member)
        .into_any_element()
}

/// The draggable edge at the start of the member's preview, over the
/// border that sets it apart from the container.
fn edge(beside: bool, cx: &mut Context<Shell>) -> AnyElement {
    use crate::elements::edge::{Side, edge};
    let side = if beside { Side::Left } else { Side::Top };
    edge(
        "preview-members-resize",
        side,
        MembersResize { beside },
        move |this| this.preview.members_share.set(beside, SHARE),
        cx,
    )
    .into_any_element()
}

impl Shell {
    /// The edge between the container and the member follows the pointer across
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
        if let Some(share) = crate::elements::edge::share(f32::from(along), f32::from(size), keep) {
            self.preview.members_share.set(beside, share);
            cx.notify();
        }
    }
}
