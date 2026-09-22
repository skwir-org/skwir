//! Single-line prompts (rename, new folder, search, tags) and the delete
//! confirmation modal.

use bibli_core::Plan;
use bibli_core::command::Invocation;
use gpui::{AnyElement, Context, Entity, FocusHandle, Subscription, Window, div, prelude::*, px};

use crate::controls::{self, ButtonStyle};
use crate::input::{InputEvent, TextInput};
use crate::keymap;
use crate::modal::{self, FocusReturn};
use crate::shell::Shell;
use crate::theme::Theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptKind {
    /// The passphrase of `Shell::unlock_device`. Masked, never trimmed.
    Passphrase,
    Rename,
    Mkdir,
    Search,
    Tags,
    InspectJump {
        pane: bibli_core::workspace::PaneId,
        generation: u64,
    },
}

pub struct PromptState {
    pub title: String,
    pub kind: PromptKind,
    pub input: Entity<TextInput>,
    back: FocusReturn,
    _sub: Subscription,
}

/// A question before something that cannot be undone. It holds the plan
/// itself, built when the question was asked: what the user read about is
/// what happens, whatever the selection or the active pane is by then.
pub struct ConfirmState {
    pub message: String,
    pub plan: Plan,
    pub focus: FocusHandle,
    cancel: FocusHandle,
    accept: FocusHandle,
    back: FocusReturn,
}

impl Shell {
    pub fn open_prompt(
        &mut self,
        kind: PromptKind,
        title: &str,
        initial: &str,
        select: Option<std::ops::Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let placeholder = match kind {
            PromptKind::InspectJump { .. } => "Offset (decimal or 0x hexadecimal)",
            PromptKind::Passphrase => "Passphrase",
            PromptKind::Rename => "New name",
            PromptKind::Mkdir => "Folder name",
            PromptKind::Search => "*.pdf modified<7d size>10M  (in: picks the root)",
            PromptKind::Tags => "tag1, tag2 (empty clears)",
        };
        let back = FocusReturn::capture(window, cx);
        let input = cx.new(|cx| TextInput::new(placeholder, cx));
        input.update(cx, |i, cx| {
            i.set_masked(kind == PromptKind::Passphrase);
            i.set_text(initial.to_owned(), cx);
            match select {
                Some(r) => i.select_range(r, cx),
                None => i.select_all_now(cx),
            }
        });
        let sub = cx.subscribe_in(
            &input,
            window,
            |this, input, ev: &InputEvent, window, cx| match ev {
                InputEvent::Submit => {
                    let kind = this.prompt.as_ref().map(|p| p.kind);
                    // Spaces can be part of a passphrase.
                    let text = if kind == Some(PromptKind::Passphrase) {
                        input.read(cx).text().to_owned()
                    } else {
                        input.read(cx).text().trim().to_owned()
                    };
                    this.close_prompt(window, cx);
                    if let Some(kind) = kind {
                        this.prompt_submitted(kind, text, window, cx);
                    }
                    cx.notify();
                }
                InputEvent::Cancel => {
                    this.unlock_device = None;
                    this.close_prompt(window, cx);
                }
                InputEvent::Changed => {}
            },
        );
        input.update(cx, |i, _| i.focus(window));
        self.prompt = Some(PromptState {
            title: title.to_owned(),
            kind,
            input,
            back,
            _sub: sub,
        });
        cx.notify();
    }

    /// Close the prompt and give the keyboard back to what had it.
    pub fn close_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(prompt) = self.prompt.take() {
            prompt.back.restore(window, &self.ui().focus);
        }
        cx.notify();
    }

    fn prompt_submitted(
        &mut self,
        kind: PromptKind,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match kind {
            PromptKind::InspectJump { pane, generation } => {
                self.with_pane(pane, |this| {
                    if this.preview.generation == generation {
                        this.run(
                            "ui.inspect",
                            serde_json::json!({"action":"jump","value":text}),
                            window,
                            cx,
                        );
                    }
                });
            }
            PromptKind::Passphrase => {
                let Some(device) = self.unlock_device.take() else {
                    return;
                };
                if text.is_empty() {
                    return;
                }
                // Dispatched directly, not through `run`: `run` traces the
                // arguments of what it runs, and this one is a secret.
                let pane = self.pane;
                let unlock = Invocation::with_args(
                    "places.unlock",
                    &serde_json::json!({ "device": device, "passphrase": text, "pane": pane }),
                );
                match self.app.dispatch(&unlock) {
                    Ok(_) => self.set_status("Unlocking…", false, cx),
                    Err(error) => self.set_status(format!("{error:#}"), true, cx),
                }
            }
            PromptKind::Rename if !text.is_empty() => {
                self.run(
                    "rename",
                    serde_json::json!({ "new_name": text }),
                    window,
                    cx,
                );
            }
            PromptKind::Mkdir if !text.is_empty() => {
                self.run("mkdir", serde_json::json!({ "name": text }), window, cx);
            }
            PromptKind::Search if !text.is_empty() => {
                self.last_search.clone_from(&text);
                self.ui_mut().last_search.clone_from(&text);
                let pane = self.pane;
                self.run(
                    "query.search",
                    serde_json::json!({ "expr": text, "pane": pane }),
                    window,
                    cx,
                );
            }
            PromptKind::Tags => {
                let tags: Vec<String> = text
                    .split(',')
                    .map(|t| t.trim().to_owned())
                    .filter(|t| !t.is_empty())
                    .collect();
                let pane = self.pane;
                self.run(
                    "tags.set",
                    serde_json::json!({ "tags": tags, "pane": pane }),
                    window,
                    cx,
                );
            }
            _ => {}
        }
    }

    /// Ask before applying `plan`, which cannot be undone.
    pub fn open_confirm(
        &mut self,
        message: String,
        plan: Plan,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let back = FocusReturn::capture(window, cx);
        let focus = cx.focus_handle();
        window.focus(&focus);
        self.confirm = Some(ConfirmState {
            message,
            plan,
            focus,
            cancel: cx.focus_handle(),
            accept: cx.focus_handle(),
            back,
        });
        cx.notify();
    }

    pub fn confirm_accept(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(c) = self.confirm.take() {
            c.back.restore(window, &self.ui().focus);
            self.apply_user_plan(c.plan, cx);
            cx.notify();
        }
    }

    pub fn confirm_dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(c) = self.confirm.take() {
            c.back.restore(window, &self.ui().focus);
            cx.notify();
        }
    }
}

pub fn render_prompt(shell: &Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let p = shell.prompt.as_ref()?;
    let theme = cx.global::<Theme>().clone();
    let panel = modal::panel(560., &theme)
        .p_3()
        .gap_2()
        .child(div().text_color(theme.text_muted).child(p.title.clone()))
        .child(
            div()
                .h(px(30.))
                .px_2()
                .flex()
                .items_center()
                .rounded_md()
                .border_1()
                .border_color(theme.accent)
                .bg(theme.bg)
                .child(p.input.clone()),
        )
        .child(
            div()
                .text_xs()
                .text_color(theme.text_muted)
                .child("Enter to confirm, Escape to cancel"),
        );
    Some(modal::modal(
        panel,
        120.,
        3,
        |this, window, cx| {
            this.unlock_device = None;
            this.close_prompt(window, cx);
        },
        &theme,
        cx,
    ))
}

pub fn render_confirm(shell: &Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let c = shell.confirm.as_ref()?;
    let theme = cx.global::<Theme>().clone();
    let panel = modal::panel(560., &theme)
        .p_3()
        .gap_2()
        .track_focus(&c.focus)
        .key_context(keymap::CONFIRM)
        .child(div().child(c.message.clone()))
        .child(
            div()
                .flex()
                .gap_2()
                .justify_end()
                .child(
                    controls::button(
                        "confirm-no",
                        "Cancel (Esc)",
                        None,
                        ButtonStyle::Plain,
                        &c.cancel,
                        &theme,
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.confirm_dismiss(window, cx);
                    })),
                )
                .child(
                    controls::button(
                        "confirm-yes",
                        "Delete (Enter)",
                        Some("trash"),
                        ButtonStyle::Danger,
                        &c.accept,
                        &theme,
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.confirm_accept(window, cx);
                    })),
                ),
        );
    Some(modal::modal(
        panel,
        120.,
        3,
        Shell::confirm_dismiss,
        &theme,
        cx,
    ))
}
