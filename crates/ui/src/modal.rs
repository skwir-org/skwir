//! What every dialog and popup shares: where the keyboard goes back to when
//! it closes, the dimmed backdrop and panel of a modal dialog, and the
//! window of rows a long picker list shows.

use gpui::{AnyElement, App, Context, Div, FocusHandle, MouseButton, Window, deferred, div};
use gpui::{prelude::*, px};

use crate::shell::Shell;
use crate::theme::Theme;

/// The focus to give back when a popup or dialog closes: whatever had it
/// when it opened (a preview's reader, the address bar, a pane), not
/// always the pane. Taken before the popup focuses itself.
#[derive(Default)]
pub struct FocusReturn(Option<FocusHandle>);

impl FocusReturn {
    pub fn capture(window: &Window, cx: &App) -> Self {
        FocusReturn(window.focused(cx))
    }

    /// Focus what had it, if anything did.
    pub fn restore_if_any(self, window: &mut Window) {
        if let Some(previous) = self.0 {
            window.focus(&previous);
        }
    }

    /// Focus what had it, or `fallback` when nothing did.
    pub fn restore(self, window: &mut Window, fallback: &FocusHandle) {
        window.focus(self.0.as_ref().unwrap_or(fallback));
    }
}

impl Shell {
    /// Whether a dialog that owns the keyboard is open. A proposal waits
    /// for these to close before it takes the keyboard for its review.
    pub fn modal_open(&self) -> bool {
        self.palette.is_some()
            || self.jump.is_some()
            || self.choose.is_some()
            || self.tag_editor.is_some()
            || self.bulk.is_some()
            || self.prompt.is_some()
            || self.confirm.is_some()
            || self.menu.is_some()
            || self.settings.is_some()
            || self.shortcuts.is_some()
    }
}

/// A modal dialog: `panel` on a scrim, `top` px from the top of the window.
/// A press outside the panel runs `dismiss`; the panel keeps its presses.
/// `priority` orders deferred overlays (menus above dialogs).
pub fn modal(
    panel: Div,
    top: f32,
    priority: usize,
    dismiss: impl Fn(&mut Shell, &mut Window, &mut Context<Shell>) + 'static,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    deferred(
        div()
            .absolute()
            .inset_0()
            .flex()
            .justify_center()
            .items_start()
            .pt(px(top))
            .bg(theme.scrim)
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| dismiss(this, window, cx)),
            )
            .child(panel.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())),
    )
    .with_priority(priority)
    .into_any_element()
}

/// The frame of a modal panel, `width` px wide.
pub fn panel(width: f32, theme: &Theme) -> Div {
    div()
        .w(px(width))
        .flex()
        .flex_col()
        .rounded_lg()
        .border_1()
        .border_color(theme.border)
        .bg(theme.surface)
        .shadow_lg()
}

/// Rows a picker (the palette, Jump) shows at once.
pub const PICKER_ROWS: usize = 14;

/// A picker's list: which rows are on screen and which one Enter runs. The
/// highlight never leaves the rows shown: moving past an edge scrolls.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Picker {
    pub highlighted: usize,
    /// First row shown.
    pub top: usize,
}

impl Picker {
    /// Move by `delta`, wrapping around the `len` rows.
    pub fn step(&mut self, delta: isize, len: usize) {
        if len == 0 {
            return;
        }
        let n = len as isize;
        self.highlighted = (self.highlighted as isize + delta).rem_euclid(n) as usize;
        if self.highlighted < self.top {
            self.top = self.highlighted;
        } else if self.highlighted >= self.top + PICKER_ROWS {
            self.top = self.highlighted + 1 - PICKER_ROWS;
        }
    }

    /// The rows on screen.
    pub fn shown(&self, len: usize) -> std::ops::Range<usize> {
        self.top.min(len)..(self.top + PICKER_ROWS).min(len)
    }
}

#[cfg(test)]
mod tests {
    use super::{PICKER_ROWS, Picker};

    #[test]
    fn the_highlight_stays_on_a_row_that_is_shown() {
        let mut picker = Picker::default();
        picker.step(-1, 30);
        assert_eq!(picker.highlighted, 29, "up from the top wraps to the end");
        assert!(picker.shown(30).contains(&29));
        picker.step(1, 30);
        assert_eq!((picker.highlighted, picker.top), (0, 0));
        for _ in 0..PICKER_ROWS {
            picker.step(1, 30);
        }
        assert_eq!(picker.highlighted, PICKER_ROWS);
        assert_eq!(picker.shown(30), 1..PICKER_ROWS + 1);
        let mut short = Picker::default();
        short.step(-1, 3);
        assert_eq!(short.shown(3), 0..3);
        assert_eq!(Picker::default().shown(0), 0..0);
    }
}
