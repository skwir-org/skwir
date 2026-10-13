//! What every dialog and popup shares: where the keyboard goes back to when
//! it closes, the dimmed backdrop, panel and buttons of a modal dialog, and
//! the window of rows a long picker list shows.

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
            || self.about.is_some()
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

/// The end of a dialog that waits for an answer: what its keys do, muted,
/// then its buttons, Cancel first and the answer last, named by its verb
/// (Rename, Unlock, Delete). Every answer the keys give, a click gives.
pub fn footer(
    hint: impl Into<gpui::SharedString>,
    buttons: impl IntoIterator<Item = gpui::Stateful<Div>>,
    theme: &Theme,
) -> Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_xs()
                .text_color(theme.text_muted)
                .child(hint.into()),
        )
        .children(buttons)
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

/// A move along a list (`ui.list_move`): rows at a time, round its ends;
/// a page at a time, or to an end, stopping there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListMove {
    By(isize),
    Page(isize),
    First,
    Last,
}

impl ListMove {
    /// From `ui.list_move`'s arguments: `delta` rows, or `to` one of
    /// `page_up`, `page_down`, `first`, `last`.
    pub fn of(args: &serde_json::Value) -> ListMove {
        match args.get("to").and_then(serde_json::Value::as_str) {
            Some("page_up") => ListMove::Page(-1),
            Some("page_down") => ListMove::Page(1),
            Some("first") => ListMove::First,
            Some("last") => ListMove::Last,
            _ => ListMove::By(
                args.get("delta")
                    .and_then(serde_json::Value::as_i64)
                    .and_then(|d| isize::try_from(d).ok())
                    .unwrap_or(1),
            ),
        }
    }

    /// Where it leads from row `at` of `len` (more than none).
    pub fn from(self, at: usize, len: usize) -> usize {
        let (at, last) = (at.cast_signed(), len.cast_signed() - 1);
        match self {
            ListMove::By(delta) => (at + delta).rem_euclid(last + 1),
            ListMove::Page(pages) => (at + pages * PICKER_ROWS.cast_signed()).clamp(0, last),
            ListMove::First => 0,
            ListMove::Last => last,
        }
        .cast_unsigned()
    }
}

/// Indices of `items` matching `query` (fuzzy, case ignored), best first;
/// all of them in their order for an empty query. The palette's and
/// `ui.choose`'s filter.
pub fn fuzzy<T>(items: &[T], query: &str, hay: impl Fn(&T) -> String) -> Vec<usize> {
    use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
    use nucleo_matcher::{Config, Matcher, Utf32Str};
    if query.trim().is_empty() {
        return (0..items.len()).collect();
    }
    let mut matcher = Matcher::new(Config::DEFAULT);
    let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
    let mut buf = Vec::new();
    let mut scored: Vec<(u32, usize)> = items
        .iter()
        .enumerate()
        .filter_map(|(i, item)| {
            pattern
                .score(Utf32Str::new(&hay(item), &mut buf), &mut matcher)
                .map(|s| (s, i))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, i)| i).collect()
}

impl Picker {
    /// Move by `delta`, wrapping around the `len` rows.
    pub fn step(&mut self, delta: isize, len: usize) {
        self.go(ListMove::By(delta), len);
    }

    /// Move as `mv` says among `len` rows, the highlighted one kept in
    /// sight.
    pub fn go(&mut self, mv: ListMove, len: usize) {
        if len == 0 {
            return;
        }
        self.highlighted = mv.from(self.highlighted, len);
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
