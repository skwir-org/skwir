//! Single-line prompts (rename, new folder) and the delete confirmation modal.

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
    Rename,
    Mkdir,
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
            PromptKind::Rename => "New name",
            PromptKind::Mkdir => "Folder name",
        };
        let input = cx.new(|cx| TextInput::new(placeholder, cx));
        input.update(cx, |i, cx| {
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
                    let text = input.read(cx).text().trim().to_owned();
                    let kind = this.prompt.as_ref().map(|p| p.kind);
                    this.prompt = None;
                    window.focus(&this.pane_focus);
                    if let Some(kind) = kind
                        && !text.is_empty()
                    {
                        match kind {
                            PromptKind::Rename => this.run(
                                "rename",
                                serde_json::json!({ "new_name": text }),
                                window,
                                cx,
                            ),
                            PromptKind::Mkdir => {
                                this.run("mkdir", serde_json::json!({ "name": text }), window, cx);
                            }
                        }
                    }
                    cx.notify();
                }
                InputEvent::Cancel => {
                    this.prompt = None;
                    window.focus(&this.pane_focus);
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
            window.focus(&self.pane_focus);
            self.run(&c.command.name, c.command.args.clone(), window, cx);
            cx.notify();
        }
    }

    pub fn confirm_dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.confirm.take().is_some() {
            window.focus(&self.pane_focus);
            cx.notify();
        }
    }
}

fn backdrop(theme: &Theme, cx: &mut Context<Shell>) -> gpui::Div {
    let _ = theme;
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
                window.focus(&this.pane_focus);
                cx.notify();
            }),
        )
}

fn panel(theme: &Theme) -> gpui::Div {
    div()
        .w(px(480.))
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
            backdrop(&theme, cx).child(
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
            backdrop(&theme, cx).child(
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
