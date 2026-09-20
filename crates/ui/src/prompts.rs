//! Single-line prompts (rename, new folder, search, tags) and the delete
//! confirmation modal.

use bibli_core::command::Invocation;
use gpui::{
    AnyElement, Context, Entity, FocusHandle, MouseButton, Subscription, Window, deferred, div,
    prelude::*, px,
};

use crate::input::{InputEvent, TextInput};
use crate::keymap;
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
    InspectSearch {
        pane: bibli_core::workspace::PaneId,
        generation: u64,
    },
    InspectJump {
        pane: bibli_core::workspace::PaneId,
        generation: u64,
    },
}

pub struct PromptState {
    pub title: String,
    pub kind: PromptKind,
    pub input: Entity<TextInput>,
    _sub: Subscription,
}

pub struct ConfirmState {
    pub message: String,
    pub command: Invocation,
    pub focus: FocusHandle,
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
            PromptKind::InspectSearch { .. } => {
                "Text, or hex: followed by byte pairs in the hex viewer"
            }
            PromptKind::InspectJump { .. } => "Offset (decimal or 0x hexadecimal)",
            PromptKind::Passphrase => "Passphrase",
            PromptKind::Rename => "New name",
            PromptKind::Mkdir => "Folder name",
            PromptKind::Search => "*.pdf modified<7d size>10M  (in: picks the root)",
            PromptKind::Tags => "tag1, tag2 (empty clears)",
        };
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
                    this.prompt = None;
                    window.focus(&this.ui().focus);
                    if let Some(kind) = kind {
                        this.prompt_submitted(kind, text, window, cx);
                    }
                    cx.notify();
                }
                InputEvent::Cancel => {
                    this.unlock_device = None;
                    this.prompt = None;
                    window.focus(&this.ui().focus);
                    cx.notify();
                }
                InputEvent::Changed => {}
            },
        );
        input.update(cx, |i, _| i.focus(window));
        self.prompt = Some(PromptState {
            title: title.to_owned(),
            kind,
            input,
            _sub: sub,
        });
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
            PromptKind::InspectSearch { pane, generation }
            | PromptKind::InspectJump { pane, generation } => {
                self.with_pane(pane, |this| {
                    if this.preview.generation == generation {
                        let action = if matches!(kind, PromptKind::InspectSearch { .. }) {
                            "find"
                        } else {
                            "jump"
                        };
                        this.run(
                            "ui.inspect",
                            serde_json::json!({"action":action,"value":text}),
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

    pub fn open_confirm(
        &mut self,
        message: String,
        command: Invocation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focus = cx.focus_handle();
        window.focus(&focus);
        self.confirm = Some(ConfirmState {
            message,
            command,
            focus,
        });
        cx.notify();
    }

    pub fn confirm_accept(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(c) = self.confirm.take() {
            window.focus(&self.ui().focus);
            self.run(&c.command.name, c.command.args.clone(), window, cx);
            cx.notify();
        }
    }

    pub fn confirm_dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.confirm.take().is_some() {
            window.focus(&self.ui().focus);
            cx.notify();
        }
    }
}

fn backdrop(cx: &mut Context<Shell>) -> gpui::Div {
    div()
        .absolute()
        .inset_0()
        .flex()
        .justify_center()
        .items_start()
        .pt(px(120.))
        .occlude()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, window, cx| {
                this.prompt = None;
                this.confirm_dismiss(window, cx);
                window.focus(&this.ui().focus);
                cx.notify();
            }),
        )
}

fn panel(theme: &Theme) -> gpui::Div {
    div()
        .w(px(560.))
        .p_3()
        .flex()
        .flex_col()
        .gap_2()
        .rounded_lg()
        .border_1()
        .border_color(theme.border)
        .bg(theme.surface)
        .shadow_lg()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

pub fn render_prompt(shell: &Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let p = shell.prompt.as_ref()?;
    let theme = cx.global::<Theme>().clone();
    Some(
        deferred(
            backdrop(cx).child(
                panel(&theme)
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
                    ),
            ),
        )
        .with_priority(3)
        .into_any_element(),
    )
}

pub fn render_confirm(shell: &Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let c = shell.confirm.as_ref()?;
    let theme = cx.global::<Theme>().clone();
    Some(
        deferred(
            backdrop(cx).child(
                panel(&theme)
                    .track_focus(&c.focus)
                    .key_context(keymap::CONFIRM)
                    .child(div().child(c.message.clone()))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .justify_end()
                            .child(
                                div()
                                    .id("confirm-no")
                                    .px_3()
                                    .py_1()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(theme.border)
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.confirm_dismiss(window, cx);
                                    }))
                                    .child("Cancel (Esc)"),
                            )
                            .child(
                                div()
                                    .id("confirm-yes")
                                    .px_3()
                                    .py_1()
                                    .rounded_md()
                                    .bg(theme.error)
                                    .text_color(gpui::white())
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.confirm_accept(window, cx);
                                    }))
                                    .child("Delete (Enter)"),
                            ),
                    ),
            ),
        )
        .with_priority(3)
        .into_any_element(),
    )
}
