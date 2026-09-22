//! Status bar: item and selection counts, executor progress, messages,
//! undo, the shelf chip, pending proposals.

use gpui::{AnyElement, Context, div, prelude::*, px};

use crate::format;
use crate::shell::Shell;
use crate::theme::Theme;
use crate::toast::{Action, Toast, Tone};

impl Shell {
    /// Tell the user something; errors stay longer and are logged.
    pub fn set_status(&mut self, text: impl Into<String>, error: bool, cx: &mut Context<Self>) {
        let text = text.into();
        if error {
            tracing::warn!(%text, "status");
        }
        self.toast(
            Toast {
                text,
                tone: if error { Tone::Error } else { Tone::Info },
                action: None,
            },
            cx,
        );
    }

    /// A command failed. Nothing selected, reached from the palette or a
    /// menu, is a hint rather than a failure.
    pub fn report_error(&mut self, error: &anyhow::Error, cx: &mut Context<Self>) {
        // Plugins that still bail with the bare text (not yet the typed
        // `NothingSelected`) are recognized by it; remove once they adopt it.
        if bibli_core::command::is_nothing_selected(error)
            || error.root_cause().to_string() == "nothing selected"
        {
            self.set_status(crate::shell::NOTHING_SELECTED, false, cx);
        } else {
            self.set_status(format!("{error:#}"), true, cx);
        }
    }

    /// An error whose remedy is a setting: the toast links to it.
    pub fn set_status_about(
        &mut self,
        text: impl Into<String>,
        setting: &str,
        cx: &mut Context<Self>,
    ) {
        let text = text.into();
        tracing::warn!(%text, "status");
        self.toast(
            Toast {
                text,
                tone: Tone::Error,
                action: Some(Action {
                    label: "Open setting".into(),
                    command: "ui.settings".into(),
                    args: serde_json::json!({ "key": setting }),
                }),
            },
            cx,
        );
    }
}

/// The bar's height: the dock above it measures its drag from the bottom.
pub const HEIGHT: f32 = 26.;

pub fn render(shell: &Shell, cx: &mut Context<Shell>) -> AnyElement {
    let theme = cx.global::<Theme>().clone();
    let selection = shell.selection_ctx();
    let (count, complete, error, is_query) = {
        let ws = shell.app.workspace.read();
        let pane = ws.pane(shell.pane);
        pane.map_or((0, true, None, false), |p| {
            (
                shell.ui().list.sorted.len(),
                p.listing.is_complete(),
                p.listing.error().map(str::to_owned),
                p.location().is_some_and(|l| &*l.scheme == "query"),
            )
        })
    };
    // "Loading…" follows the same delayed signal as the pane banner.
    let loading = crate::loading::pending(shell, shell.pane);
    let items = match (is_query, complete) {
        (true, false) => format!("Searching… {}", format::count(count, "result")),
        (true, true) => format::count(count, "result"),
        (false, false) if error.is_none() && loading => {
            format!("Loading… {}", format::count(count, "item"))
        }
        (false, _) => format::count(count, "item"),
    };
    let mut bar = div()
        .h(px(HEIGHT))
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .gap_4()
        .px_2()
        .text_xs()
        .border_t_1()
        .border_color(theme.border)
        .bg(theme.surface)
        .text_color(theme.text_muted)
        .child(items);
    if selection.count > 0 {
        let mut text = format!("{} selected", format::thousands(selection.count));
        if selection.bytes > 0 {
            text = format!("{text} · {}", format::size(selection.bytes));
        }
        bar = bar.child(div().text_color(theme.text).child(text));
    }
    // With nothing listed the pane itself explains the failure.
    if let Some(e) = error.filter(|_| count > 0) {
        bar = bar.child(div().text_color(theme.error).truncate().child(e));
    }
    bar = bar.child(div().flex_1());
    bar = bar.children(crate::player::render_status(shell, &theme, cx));
    // The clipboard is invisible once the pane shows another folder: say
    // what is waiting, and let a click (or Escape) forget it.
    let waiting = shell.pending.entries.len();
    if waiting > 0 {
        let font = shell.font.clone();
        let (verb, icon) = if shell.pending.cut {
            ("cut", "cut")
        } else {
            ("copied", "copy")
        };
        let keys = shell
            .keys_for("clipboard_paste", &serde_json::Value::Null)
            .map_or_else(String::new, |keys| format!(" Paste with {keys}."));
        let tip: gpui::SharedString = format!("{keys} Click to forget.").trim().to_owned().into();
        bar = bar.child(
            div()
                .id("status-pending")
                .flex()
                .flex_none()
                .items_center()
                .gap_1()
                .px_2()
                .rounded_md()
                .cursor_pointer()
                .text_color(theme.text)
                .hover(|s| s.bg(theme.hover))
                .tooltip(move |_, cx| crate::toolbar::tooltip(tip.clone(), font.clone(), cx))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.run("clipboard_clear", serde_json::json!({}), window, cx);
                }))
                .child(crate::controls::glyph(icon, theme.text_muted, 13.))
                .child(format!("{} {verb}", format::count(waiting, "item"))),
        );
    }
    if !shell.reviews.is_empty() {
        bar = bar.child(
            div()
                .text_color(theme.accent)
                .child(format!("{} proposal(s) to review", shell.reviews.len())),
        );
    }
    // The shelf announces itself only when it holds something.
    if shell.shelf_count > 0 {
        bar = bar.child(
            div()
                .id("shelf-chip")
                .px_2()
                .rounded_md()
                .cursor_pointer()
                .bg(theme.selection_bg)
                .text_color(theme.selection_text)
                .hover(|s| s.bg(theme.hover))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.run("ui.shelf_open", serde_json::json!({}), window, cx);
                }))
                .child(format!("Shelf: {}", shell.shelf_count)),
        );
    }
    if let Some(title) = shell.app.executor.last_undo_title() {
        let font = shell.font.clone();
        let keys = shell
            .keys_for("undo", &serde_json::Value::Null)
            .map_or_else(String::new, |keys| format!(" ({keys})"));
        let tip: gpui::SharedString = format!("Undo {title}{keys}").into();
        bar = bar.child(
            div()
                .id("status-undo")
                .flex()
                .flex_none()
                .items_center()
                .gap_1()
                .px_2()
                .rounded_md()
                .cursor_pointer()
                .hover(|s| s.bg(theme.hover).text_color(theme.text))
                .tooltip(move |_, cx| crate::toolbar::tooltip(tip.clone(), font.clone(), cx))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.run("undo", serde_json::json!({}), window, cx);
                }))
                .child(crate::controls::glyph("revert", theme.text_muted, 13.))
                .child("Undo"),
        );
    }
    bar.into_any_element()
}
