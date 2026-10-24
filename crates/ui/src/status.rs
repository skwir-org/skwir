//! Each pane's footer: what it lists and what is selected there. The
//! window's own state (what plays while the preview is closed, the files
//! waiting for a paste, proposals to review, the shelf, Undo) ends the
//! footer of the last pane, at the window's bottom right whichever way a
//! split runs. The side panels have none: the sidebar's facts are on its
//! rows and the preview's in Properties, so they run to the window's foot.
//! Messages are toasts (`set_status`), never a footer's.

use gpui::{AnyElement, Context, div, prelude::*, px};
use skwir_core::location::AsOf;

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
                actions: Vec::new(),
            },
            cx,
        );
    }

    /// A command failed. Nothing selected, reached from the palette or a
    /// menu, is a hint rather than a failure.
    pub fn report_error(&mut self, error: &anyhow::Error, cx: &mut Context<Self>) {
        if skwir_core::command::is_nothing_selected(error) {
            self.set_status(crate::shell::NOTHING_SELECTED, false, cx);
        } else if skwir_core::command::wants_clipboard(error) {
            self.set_status("Nothing to paste", false, cx);
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
        self.toast_about(text, Tone::Error, setting, cx);
    }

    /// Something worth knowing, not a failure, that a setting changes:
    /// the toast links to it.
    pub fn set_hint_about(
        &mut self,
        text: impl Into<String>,
        setting: &str,
        cx: &mut Context<Self>,
    ) {
        self.toast_about(text.into(), Tone::Info, setting, cx);
    }

    fn toast_about(&mut self, text: String, tone: Tone, setting: &str, cx: &mut Context<Self>) {
        self.toast(
            Toast {
                text,
                tone,
                actions: vec![Action {
                    label: "Open setting".into(),
                    command: "ui.settings".into(),
                    args: serde_json::json!({ "key": setting }),
                }],
            },
            cx,
        );
    }
}

/// A pane footer's height.
pub const HEIGHT: f32 = 26.;

/// What the footer says of an answer that is an index's (`Listing::as_of`),
/// and its tooltip: `as of Today 03:00`, then what answered and what that
/// leaves out.
fn as_of_said(as_of: &AsOf, now: &jiff::Zoned) -> (String, String) {
    match as_of.updated {
        Some(updated) => (
            format!("as of {}", format::friendly_time(updated, now)),
            format!(
                "Found in {}, last brought up to date {}: what was made, renamed or removed since is not in this answer.",
                as_of.by,
                format::exact_time(updated)
            ),
        ),
        None => (
            format!("as of {}", as_of.by),
            format!(
                "Found in {}, whose last update cannot be told: what was made, renamed or removed since is not in this answer.",
                as_of.by
            ),
        ),
    }
}

/// The footer of the pane being rendered; `window` adds the window's own
/// state at its end (the last pane's).
pub fn render(shell: &Shell, window: bool, cx: &mut Context<Shell>) -> AnyElement {
    let theme = cx.global::<Theme>().clone();
    let selection = shell.selection_ctx();
    let (count, complete, error, is_query, sweep, total, as_of) = {
        let ws = shell.app.workspace.read();
        let pane = ws.pane(shell.pane);
        pane.map_or((0, true, None, false, None, None, None), |p| {
            (
                shell.ui().list.sorted.len(),
                p.listing.is_complete(),
                p.listing.error().map(str::to_owned),
                shell.ui().place.search,
                p.selection.sweeping(),
                // Filtered by name or narrowed: how many there are in all.
                p.is_filtered().then(|| p.listing.sorted(p.sort).len()),
                p.listing.as_of().cloned(),
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
        (false, _) => match total {
            Some(total) => format!(
                "{} of {}",
                format::thousands(count),
                format::count(total, "item")
            ),
            None => format::count(count, "item"),
        },
    };
    // Nothing selected: what the files shown weigh, as a selection's size
    // is counted (folders not measured), once a worker has summed them
    // (`weight.rs`); a view that measures folders says what they weigh.
    let weighs = shell.view_weighs();
    let listed = (selection.count == 0)
        .then(|| weighs.map_or_else(|| shell.listed_bytes(), |w| w.shown))
        .flatten();
    let items = match listed {
        Some(bytes) => format!("{items} · {}", format::size(bytes)),
        None => items,
    };
    // An index's answer says as of when (ADR-050): what was made, renamed
    // or removed since is not in it.
    let as_of = as_of.map(|as_of| as_of_said(&as_of, &jiff::Zoned::now()));
    // What it says is the status line a screen reader reads when asked,
    // never announced: it changes with every key.
    let mut said = shell.a11y.then(|| match &as_of {
        Some((text, _)) => format!("{items}, {text}"),
        None => items.clone(),
    });
    let mut bar = div()
        .id(("pane-footer", shell.pane.0 as usize))
        .role(gpui::Role::Status)
        .flex_none()
        .h(px(HEIGHT))
        .w_full()
        .min_w_0()
        .overflow_hidden()
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
    if let Some((text, tip)) = as_of {
        let tip: gpui::SharedString = tip.into();
        bar = bar.child(
            div()
                .id("listing-as-of")
                .flex_none()
                .tooltip(move |_, cx| crate::toolbar::tooltip(tip.clone(), cx))
                .child(text),
        );
    }
    if selection.count > 0 {
        let mut text = format!("{} selected", format::thousands(selection.count));
        // A part of the size is never said as the whole: until every
        // selected file is stat-ed (`sync_selection_hydration`), none.
        let bytes = weighs.map_or(selection.bytes.unwrap_or(0), |w| w.selected.unwrap_or(0));
        if bytes > 0 {
            text = format!("{text} · {}", format::size(bytes));
        }
        if let Some(said) = &mut said {
            said.push_str(", ");
            said.push_str(&text);
        }
        bar = bar.child(div().text_color(theme.text).child(text));
    }
    // A sweep is a mode: the keys act differently while it lasts.
    if let Some(unset) = sweep {
        let (mode, keys) = (
            if unset { "Deselecting" } else { "Selecting" },
            shell.keys_for("select_sweep_end", &serde_json::json!({})),
        );
        let text = match keys {
            Some(keys) => format!("{mode} as the cursor moves · {keys} to stop"),
            None => format!("{mode} as the cursor moves"),
        };
        if let Some(said) = &mut said {
            said.push_str(", ");
            said.push_str(&text);
        }
        bar = bar.child(
            div()
                .flex_none()
                .px_1()
                .rounded_sm()
                .bg(theme.selection_bg)
                .text_color(theme.selection_text)
                .child(text),
        );
    }
    // A listing's error is a message, not state: the banner says it beside
    // the entries (`loading::message`), the pane itself with none.
    bar = bar
        .when_some(said, StatefulInteractiveElement::aria_label)
        .child(div().flex_1());
    if !window {
        return bar.into_any_element();
    }
    bar = bar.children(crate::player::render_status(shell, &theme, cx));
    // The clipboard is invisible once the pane shows another folder: say
    // what is waiting, and let a click forget it.
    let waiting = shell.pending.len();
    if waiting > 0 {
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
                .tooltip(move |_, cx| crate::toolbar::tooltip(tip.clone(), cx))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.run("clipboard_clear", serde_json::json!({}), window, cx);
                }))
                .child(crate::controls::glyph(icon, theme.text_muted, 13.))
                .child(format!("{} {verb}", format::count(waiting, "item"))),
        );
    }
    if !shell.reviews.is_empty() {
        let questions = shell.reviews.iter().filter(|r| r.is_question()).count();
        let plans = shell.reviews.len() - questions;
        let words: Vec<String> = [
            (plans > 0).then(|| format!("{} to review", format::count(plans, "proposal"))),
            (questions > 0).then(|| format::count(questions, "question")),
        ]
        .into_iter()
        .flatten()
        .collect();
        bar = bar.child(div().text_color(theme.accent).child(words.join(" · ")));
    }
    // What a plugin counts (the shelf's items) announces itself only
    // when it counts something, in its words; a click opens what it counts.
    for tally in &shell.tallies {
        let open = tally.open.clone();
        bar = bar.child(
            div()
                // Known by what it counts: one that comes or goes moves
                // the others.
                .id(crate::elements::known_by("tally", &tally.label))
                .role(gpui::Role::Button)
                .when(shell.a11y, |d| {
                    d.aria_label(format!("{}: {}", tally.label, tally.count))
                })
                .px_2()
                .rounded_md()
                .when(open.is_some(), gpui::Styled::cursor_pointer)
                .bg(theme.selection_bg)
                .text_color(theme.selection_text)
                .hover(|s| s.bg(theme.hover))
                .on_click(cx.listener(move |this, _, window, cx| {
                    if let Some(open) = &open {
                        this.run(&open.name, open.args.clone(), window, cx);
                    }
                }))
                .child(format!("{}: {}", tally.label, tally.count)),
        );
    }
    // What Undo takes: the last thing asked, once it ends (ADR-079).
    if let Some((title, running)) = shell.app.executor.undo_target() {
        let keys = shell
            .keys_for("undo", &serde_json::Value::Null)
            .map_or_else(String::new, |keys| format!(" ({keys})"));
        let once = if running { " (once it ends)" } else { "" };
        let tip: gpui::SharedString = format!("Undo {title}{once}{keys}").into();
        bar = bar.child(
            div()
                .id("status-undo")
                .role(gpui::Role::Button)
                .aria_label(tip.clone())
                .flex()
                .flex_none()
                .items_center()
                .gap_1()
                .px_2()
                .rounded_md()
                .cursor_pointer()
                .hover(|s| s.bg(theme.hover).text_color(theme.text))
                .tooltip(move |_, cx| crate::toolbar::tooltip(tip.clone(), cx))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.run("undo", serde_json::json!({}), window, cx);
                }))
                .child(crate::controls::glyph("revert", theme.text_muted, 13.))
                .child("Undo"),
        );
    }
    bar.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_index_s_answer_says_as_of_when_and_what_it_leaves_out() {
        let now = jiff::Timestamp::from_second(1_800_000_007)
            .unwrap()
            .to_zoned(jiff::tz::TimeZone::UTC);
        let an_hour_before = std::time::SystemTime::UNIX_EPOCH
            + std::time::Duration::from_secs(1_800_000_007 - 3600);
        let as_of = AsOf {
            by: "plocate's database".into(),
            updated: Some(an_hour_before),
        };
        let (text, tip) = as_of_said(&as_of, &now);
        assert_eq!(text, "as of Today 07:00");
        assert!(tip.starts_with("Found in plocate's database, last brought up to date "));
        assert!(tip.ends_with("is not in this answer."), "{tip}");

        let (text, tip) = as_of_said(
            &AsOf {
                updated: None,
                ..as_of
            },
            &now,
        );
        assert_eq!(text, "as of plocate's database");
        assert!(tip.contains("whose last update cannot be told"), "{tip}");
    }
}
