//! Status bar: item and selection counts, executor progress, messages,
//! undo, the shelf chip, pending proposals.

use std::time::Duration;

use bibli_core::op::ExecEvent;
use gpui::{AnyElement, Context, div, prelude::*, px};

use crate::format;
use crate::shell::Shell;
use crate::theme::Theme;

#[derive(Clone, Debug)]
pub struct Message {
    pub text: String,
    pub error: bool,
    /// A setting that resolves the message; the bar offers to open it.
    pub setting: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ProgressState {
    pub title: String,
    pub step: usize,
    pub total: usize,
    /// `Some(None)` finished fine, `Some(Some(err))` failed.
    pub done: Option<Option<String>>,
}

#[derive(Default)]
pub struct StatusState {
    pub message: Option<Message>,
    pub progress: Option<ProgressState>,
    generation: u64,
}

impl Shell {
    /// Show a message for five seconds.
    pub fn set_status(&mut self, text: impl Into<String>, error: bool, cx: &mut Context<Self>) {
        self.set_status_for(text, error, 5, cx);
    }

    /// Show a message for `secs` seconds.
    pub fn set_status_for(
        &mut self,
        text: impl Into<String>,
        error: bool,
        secs: u64,
        cx: &mut Context<Self>,
    ) {
        let text = text.into();
        if error {
            tracing::warn!(%text, "status");
        }
        self.status.message = Some(Message {
            text,
            error,
            setting: None,
        });
        self.status.generation += 1;
        let generation = self.status.generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_secs(secs))
                .await;
            this.update(cx, |shell, cx| {
                if shell.status.generation == generation {
                    shell.status.message = None;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// An error whose remedy is a setting: the bar links to it.
    pub fn set_status_about(
        &mut self,
        text: impl Into<String>,
        setting: &str,
        cx: &mut Context<Self>,
    ) {
        self.set_status_for(text, true, 12, cx);
        if let Some(message) = &mut self.status.message {
            message.setting = Some(setting.to_owned());
        }
    }

    /// Apply pending executor events to the progress state.
    pub fn drain_executor(&mut self, cx: &mut Context<Self>) {
        let rx = self.app.executor.events();
        let mut any = false;
        while let Ok(ev) = rx.try_recv() {
            any = true;
            match ev {
                ExecEvent::Started { title, total, .. } => {
                    self.status.progress = Some(ProgressState {
                        title,
                        step: 0,
                        total,
                        done: None,
                    });
                }
                ExecEvent::Step(p) => {
                    if let Some(s) = &mut self.status.progress {
                        s.step = p.step + 1;
                        s.total = p.total;
                    }
                }
                ExecEvent::Finished { error, .. } => {
                    if let Some(s) = &mut self.status.progress {
                        s.done = Some(error);
                    }
                    self.status.generation += 1;
                    let generation = self.status.generation;
                    cx.spawn(async move |this, cx| {
                        cx.background_executor().timer(Duration::from_secs(5)).await;
                        this.update(cx, |shell, cx| {
                            if shell.status.generation == generation {
                                shell.status.progress = None;
                                cx.notify();
                            }
                        })
                        .ok();
                    })
                    .detach();
                }
            }
        }
        if any {
            cx.notify();
        }
    }
}

pub fn render(shell: &Shell, cx: &mut Context<Shell>) -> AnyElement {
    let theme = cx.global::<Theme>().clone();
    let (count, complete, selected, error, is_query) = {
        let ws = shell.app.workspace.read();
        let pane = ws.pane(shell.pane);
        pane.map_or((0, true, 0, None, false), |p| {
            (
                shell.ui().list.sorted.len(),
                p.listing.is_complete(),
                p.selection.len(),
                p.listing.error().map(str::to_owned),
                p.location().is_some_and(|l| &*l.scheme == "query"),
            )
        })
    };
    // "Loading…" follows the same delayed signal as the pane banner.
    let loading = crate::loading::pending(shell, shell.pane);
    let items = match (is_query, complete) {
        (true, false) => format!("searching… {} results", format::thousands(count)),
        (true, true) => format!("{} results", format::thousands(count)),
        (false, false) if error.is_none() && loading => {
            format!("Loading… {} items", format::thousands(count))
        }
        (false, _) => format!("{} items", format::thousands(count)),
    };
    let mut bar = div()
        .h(px(26.))
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
    if selected > 0 {
        bar = bar.child(format!("{selected} selected"));
    }
    if let Some(p) = &shell.status.progress {
        let text = match &p.done {
            None => format!("{}: step {}/{}", p.title, p.step, p.total),
            Some(None) => format!("{}: done", p.title),
            Some(Some(e)) => format!("{}: {e}", p.title),
        };
        let color = if matches!(p.done, Some(Some(_))) {
            theme.error
        } else {
            theme.text
        };
        bar = bar.child(div().text_color(color).child(text));
    }
    if let Some(e) = error {
        bar = bar.child(div().text_color(theme.error).child(e));
    }
    if let Some(m) = &shell.status.message {
        let color = if m.error { theme.error } else { theme.text };
        bar = bar.child(div().text_color(color).truncate().child(m.text.clone()));
        if let Some(key) = m.setting.clone() {
            bar = bar.child(
                div()
                    .id("status-setting")
                    .flex_none()
                    .px_2()
                    .rounded_md()
                    .cursor_pointer()
                    .text_color(theme.accent)
                    .hover(|s| s.bg(theme.border))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.run("ui.settings", serde_json::json!({ "key": key }), window, cx);
                    }))
                    .child("Open setting"),
            );
        }
    }
    bar = bar.child(div().flex_1());
    if !shell.reviews.is_empty() {
        bar = bar.child(
            div()
                .text_color(theme.accent)
                .child(format!("{} proposal(s) to review", shell.reviews.len())),
        );
    }
    if shell.shelf_count > 0 || shell.app.commands.get("shelf.list").is_some() {
        bar = bar.child(
            div()
                .id("shelf-chip")
                .px_2()
                .rounded_md()
                .cursor_pointer()
                .when(shell.shelf_count > 0, |d| {
                    d.bg(theme.selection_bg).text_color(theme.selection_text)
                })
                .hover(|s| s.bg(theme.border))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.run("ui.shelf_open", serde_json::json!({}), window, cx);
                }))
                .child(format!("Shelf: {}", shell.shelf_count)),
        );
    }
    let undo = if shell.app.executor.can_undo() {
        "undo available"
    } else {
        ""
    };
    bar.child(undo).into_any_element()
}
