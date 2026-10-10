//! The contents drawer every preview with headings shares: a document's
//! outline, a Markdown file's headings, a book's chapters.
use gpui::{AnyElement, Context, SharedString, div, prelude::*, px};

use crate::shell::Shell;
use crate::theme::Theme;
use crate::toolbar::Button;

pub struct Entry {
    pub depth: usize,
    pub title: SharedString,
    /// Where the entry leads; one without a target is only a label.
    pub command: Option<(&'static str, serde_json::Value)>,
}

/// The drawer over the left of the content. Choosing an entry goes there and
/// closes the drawer with `close`, the command that toggles it. Only the
/// rows in view are built (`uniform_list`): a document's outline of
/// thousands of entries built every row, its tooltip and its listeners,
/// at every frame the drawer was open.
pub fn drawer(
    entries: Vec<Entry>,
    close: (&'static str, serde_json::Value),
    shell: &Shell,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let list: AnyElement = if entries.is_empty() {
        div()
            .p_2()
            .text_color(theme.text_muted)
            .child("No contents")
            .into_any_element()
    } else {
        let count = entries.len();
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
                    cx.processor(
                        move |shell: &mut Shell, range: std::ops::Range<usize>, _, cx| {
                            let theme = cx.global::<Theme>().clone();
                            range
                                .map(|index| {
                                    row(&entries[index], index, &closing, shell, &theme, cx)
                                })
                                .collect::<Vec<_>>()
                        },
                    ),
                )
                .flex_1()
                .p_2(),
            )
            .into_any_element()
    };
    gpui::deferred(
        div()
            .absolute()
            .top_0()
            .bottom_0()
            .left_0()
            .w(px(220.))
            .max_w_full()
            .flex()
            .flex_col()
            .bg(theme.bg)
            .border_r_1()
            .border_color(theme.border)
            .shadow_md()
            .occlude()
            .text_xs()
            .child(
                div()
                    .flex()
                    .items_center()
                    .px_2()
                    .py_1()
                    .child(div().flex_1().child(crate::preview_bar::label::CONTENTS))
                    .child(
                        Button::new("outline-close", "close", "Close contents", close.0)
                            .args(close.1)
                            .render(shell, cx),
                    ),
            )
            .child(list),
    )
    .with_priority(1)
    .into_any_element()
}

/// One entry of the drawer.
fn row(
    entry: &Entry,
    index: usize,
    close: &(&'static str, serde_json::Value),
    shell: &Shell,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let (command, closing) = (entry.command.clone(), close.clone());
    let (title, clip) = crate::elements::clip::text(entry.title.clone());
    div()
        .id(("outline-entry", index))
        .optional_tooltip(clip.tooltip(None, shell.font.clone()))
        .pl(px(entry.depth.min(12) as f32 * 10. + 8.))
        .py_1()
        .rounded_md()
        .truncate()
        .when(command.is_some(), |d| {
            d.cursor_pointer()
                .tab_index(0)
                .key_context("ToolbarButton")
                .hover(|s| s.bg(theme.hover))
                .focus(|s| s.bg(theme.selection_bg))
        })
        .child(title)
        .on_click(cx.listener(move |shell, _, window, cx| {
            if let Some((name, args)) = &command {
                shell.run(name, args.clone(), window, cx);
                shell.run(closing.0, closing.1.clone(), window, cx);
            }
        }))
        .on_action(cx.listener(|_, action: &crate::keymap::RunCommand, _, cx| {
            // Activation comes from GPUI's key-up click, as in toolbar::Button.
            if action.name != "ui.toolbar_activate" {
                cx.propagate();
            }
        }))
        .into_any_element()
}
