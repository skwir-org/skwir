//! The contents every preview with headings shares: a document's outline,
//! a Markdown file's headings, a book's chapters. A drawer over the left of
//! the content, which choosing an entry closes; pinned
//! (`reading.contents_pinned`), a column beside the content, which narrows
//! to make room, open until closed. The part shown is highlighted either
//! way, and kept in view as the reader moves.
use std::cell::Cell;

use gpui::{
    AnyElement, Context, ScrollStrategy, SharedString, UniformListScrollHandle, div, prelude::*,
    px, relative,
};

use crate::shell::Shell;
use crate::theme::Theme;
use crate::toolbar::Button;

/// The contents' width, at most a share of the preview's when pinned beside
/// it.
const WIDTH: f32 = 220.;
const PINNED_SHARE: f32 = 0.45;

pub struct Entry {
    pub depth: usize,
    pub title: SharedString,
    /// Where the entry leads; one without a target is only a label.
    pub command: Option<(&'static str, serde_json::Value)>,
}

/// What the contents list keeps between frames, held by the view that
/// draws it: where it is scrolled, and the entry it last brought into view,
/// so it follows the reader without taking back the user's own scrolling.
#[derive(Default)]
pub struct Follow {
    scroll: UniformListScrollHandle,
    revealed: Cell<Option<usize>>,
}

/// The contents of what is shown: `entries`, `current` the part shown.
/// Choosing an entry goes there and, unpinned, closes the contents with
/// `close`, the command that toggles them. Only the rows in view are built
/// (`uniform_list`): a document's outline of thousands of entries built
/// every row, its tooltip and its listeners, at every frame it was open.
pub fn contents(
    entries: Vec<Entry>,
    current: Option<usize>,
    follow: &Follow,
    close: (&'static str, serde_json::Value),
    shell: &Shell,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let pinned = shell.prefs.contents_pinned;
    let list: AnyElement = if entries.is_empty() {
        div()
            .p_2()
            .text_color(theme.text_muted)
            .child("No contents")
            .into_any_element()
    } else {
        let count = entries.len();
        let current = current.filter(|&index| index < count);
        // The part shown came into view once as it changed: in the middle
        // when the contents open, then as little as it takes.
        if current != follow.revealed.get() {
            if let Some(index) = current {
                let strategy = if follow.revealed.get().is_none() {
                    ScrollStrategy::Center
                } else {
                    ScrollStrategy::Nearest
                };
                follow.scroll.scroll_to_item(index, strategy);
            }
            follow.revealed.set(current);
        }
        let (entries, closing) = (std::rc::Rc::new(entries), close.clone());
        div()
            .id("outline-list")
            .tab_group()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                gpui::uniform_list(
                    "outline-rows",
                    count,
                    cx.processor(move |_: &mut Shell, range: std::ops::Range<usize>, _, cx| {
                        let theme = cx.global::<Theme>().clone();
                        range
                            .map(|index| {
                                let at = Row {
                                    index,
                                    current: current == Some(index),
                                    pinned,
                                };
                                row(&entries[index], at, &closing, &theme, cx)
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(&follow.scroll)
                .flex_1()
                .p_2(),
            )
            .into_any_element()
    };
    div()
        .flex()
        .flex_col()
        .size_full()
        .text_xs()
        .child(
            div()
                .flex()
                .items_center()
                .px_2()
                .py_1()
                .child(div().flex_1().child(crate::preview_bar::label::CONTENTS))
                .child(
                    Button::new(
                        "outline-pin",
                        "pin",
                        "Keep contents open",
                        "ui.contents_pin",
                    )
                    .active(pinned)
                    .render(shell, cx),
                )
                .child(
                    Button::new("outline-close", "close", "Close contents", close.0)
                        .args(close.1)
                        .render(shell, cx),
                ),
        )
        .child(list)
        .into_any_element()
}

/// `content` with its `contents`, when they are open: pinned, a column
/// beside it, the content narrowed to make room; else a drawer over its
/// left.
pub fn around(
    content: AnyElement,
    contents: Option<AnyElement>,
    shell: &Shell,
    theme: &Theme,
) -> AnyElement {
    let Some(contents) = contents else {
        return content;
    };
    if shell.prefs.contents_pinned {
        return div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_row()
            .child(
                div()
                    .flex_none()
                    .w(px(WIDTH))
                    .max_w(relative(PINNED_SHARE))
                    .h_full()
                    .bg(theme.bg)
                    .border_r_1()
                    .border_color(theme.border)
                    .child(contents),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(content),
            )
            .into_any_element();
    }
    div()
        .relative()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .child(content)
        .child(
            gpui::deferred(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left_0()
                    .w(px(WIDTH))
                    .max_w_full()
                    .bg(theme.bg)
                    .border_r_1()
                    .border_color(theme.border)
                    .shadow_md()
                    .occlude()
                    .child(contents),
            )
            .with_priority(1),
        )
        .into_any_element()
}

/// Which row is drawn, and how.
#[derive(Clone, Copy)]
struct Row {
    index: usize,
    /// The part shown.
    current: bool,
    pinned: bool,
}

/// One entry of the contents.
fn row(
    entry: &Entry,
    at: Row,
    close: &(&'static str, serde_json::Value),
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let (command, closing) = (entry.command.clone(), close.clone());
    let (title, clip) = crate::elements::clip::text(entry.title.clone());
    let hover = theme.hover;
    let row = div()
        .id(crate::elements::known_by(
            "outline-entry",
            (at.index, &entry.title),
        ))
        .optional_tooltip(clip.tooltip(None))
        .pl(px(entry.depth.min(12) as f32 * 10. + 8.))
        .py_1()
        .rounded_md()
        .truncate()
        // The keys' place is a ring, as in the sidebar, apart from the part
        // shown, which is the selection's colors.
        .border_1()
        .border_color(gpui::transparent_black())
        .when(at.current, |d| {
            d.bg(theme.selection_bg).text_color(theme.selection_text)
        })
        .when(command.is_some(), |d| {
            d.cursor_pointer()
                .tab_index(0)
                .when(!at.current, |d| d.hover(move |s| s.bg(hover)))
                .focus(|s| s.border_color(theme.accent))
        })
        .child(title);
    crate::modal::pressed(
        row,
        move |shell, window, cx| {
            if let Some((name, args)) = &command {
                shell.run(name, args.clone(), window, cx);
                if !at.pinned {
                    shell.run(closing.0, closing.1.clone(), window, cx);
                }
            }
        },
        cx,
    )
    .into_any_element()
}

/// The entry of the part shown in a document read by pages: the last of
/// those leading to `page` or before it, the deepest where several lead to
/// the same page (`pages`, each entry's in order).
pub fn at_page(pages: impl IntoIterator<Item = Option<u32>>, page: u32) -> Option<usize> {
    let mut best: Option<(usize, u32)> = None;
    for (index, at) in pages.into_iter().enumerate() {
        if let Some(at) = at.filter(|&at| at <= page)
            && best.is_none_or(|(_, shown)| at >= shown)
        {
            best = Some((index, at));
        }
    }
    best.map(|(index, _)| index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_part_shown_is_the_last_entry_at_or_before_the_page() {
        let pages = [Some(1), Some(3), Some(3), None, Some(7), Some(5)];
        assert_eq!(at_page(pages, 1), Some(0));
        assert_eq!(at_page(pages, 2), Some(0));
        assert_eq!(at_page(pages, 3), Some(2), "the deepest of a page's");
        assert_eq!(at_page(pages, 6), Some(5), "by page, not by order");
        assert_eq!(at_page(pages, 9), Some(4));
        assert_eq!(at_page([Some(4)], 2), None, "before the first");
        assert_eq!(at_page([None, None], 2), None);
    }
}
