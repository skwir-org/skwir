//! Jump overlay: frecency candidates from the jump plugin as you type.

use gpui::{
    AnyElement, Context, Entity, MouseButton, Subscription, Window, deferred, div, prelude::*, px,
};
use serde::Deserialize;

use crate::input::{InputEvent, TextInput};
use crate::keymap;
use crate::shell::Shell;
use crate::theme::Theme;

#[derive(Clone, Debug, Deserialize)]
pub struct Candidate {
    pub path: std::path::PathBuf,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub source: String,
}

pub struct JumpState {
    pub input: Entity<TextInput>,
    pub candidates: Vec<Candidate>,
    pub highlighted: usize,
    query: Option<String>,
    _sub: Subscription,
}

impl Shell {
    pub fn open_jump(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| TextInput::new("Jump to a folder or recent file", cx));
        let sub = cx.subscribe_in(
            &input,
            window,
            |this, _, ev: &InputEvent, window, cx| match ev {
                InputEvent::Submit => this.jump_submit(window, cx),
                InputEvent::Cancel => this.close_jump(window, cx),
                InputEvent::Changed => cx.notify(),
            },
        );
        input.update(cx, |i, _| i.focus(window));
        self.jump = Some(JumpState {
            input,
            candidates: Vec::new(),
            highlighted: 0,
            query: None,
            _sub: sub,
        });
        self.jump_refresh(cx);
        cx.notify();
    }

    pub fn close_jump(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.jump = None;
        window.focus(&self.ui().focus);
        cx.notify();
    }

    pub fn jump_move(&mut self, delta: isize, cx: &mut Context<Self>) {
        if let Some(j) = &mut self.jump
            && !j.candidates.is_empty()
        {
            let n = j.candidates.len() as isize;
            j.highlighted = ((j.highlighted as isize + delta).rem_euclid(n)) as usize;
            cx.notify();
        }
    }

    fn jump_submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let target = self
            .jump
            .as_ref()
            .and_then(|j| j.candidates.get(j.highlighted).map(|c| c.path.clone()));
        self.close_jump(window, cx);
        if let Some(p) = target {
            self.run(
                "jump.to",
                serde_json::json!({ "path": p.to_string_lossy() }),
                window,
                cx,
            );
        }
    }

    /// Ask the plugin for candidates when the query changed.
    fn jump_refresh(&mut self, cx: &mut Context<Self>) {
        let Some(j) = &self.jump else { return };
        let q = j.input.read(cx).text().to_owned();
        if j.query.as_deref() == Some(q.as_str()) {
            return;
        }
        let value = self.query_value(
            "jump.candidates",
            serde_json::json!({ "query": q, "limit": 30 }),
            cx,
        );
        let candidates: Vec<Candidate> = value
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        if let Some(j) = &mut self.jump {
            j.candidates = candidates;
            j.highlighted = 0;
            j.query = Some(q);
        }
    }
}

pub fn render(shell: &mut Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    shell.jump_refresh(cx);
    let j = shell.jump.as_ref()?;
    let theme = cx.global::<Theme>().clone();
    let mut list = div().flex().flex_col().py_1().overflow_hidden();
    for (ix, c) in j.candidates.iter().enumerate().take(14) {
        let hl = ix == j.highlighted;
        list = list.child(
            div()
                .id(("jump", ix))
                .h(px(26.))
                .px_3()
                .flex()
                .items_center()
                .gap_3()
                .cursor_pointer()
                .when(hl, |d| {
                    d.bg(theme.selection_bg).text_color(theme.selection_text)
                })
                .on_click(cx.listener(move |this, _, window, cx| {
                    if let Some(j) = &mut this.jump {
                        j.highlighted = ix;
                    }
                    this.jump_submit(window, cx);
                }))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .child(c.path.to_string_lossy().into_owned()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(if hl {
                            theme.selection_text
                        } else {
                            theme.text_muted
                        })
                        .child(format!("{} · {}", c.kind, c.source)),
                ),
        );
    }
    if j.candidates.is_empty() {
        list = list.child(
            div()
                .px_3()
                .py_2()
                .text_color(theme.text_muted)
                .child("No candidates"),
        );
    }
    Some(
        deferred(
            div()
                .absolute()
                .inset_0()
                .flex()
                .justify_center()
                .items_start()
                .pt(px(80.))
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| this.close_jump(window, cx)),
                )
                .child(
                    div()
                        .w(px(640.))
                        .flex()
                        .flex_col()
                        .rounded_lg()
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.surface)
                        .shadow_lg()
                        .key_context(keymap::JUMP)
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(
                            div()
                                .p_2()
                                .flex()
                                .items_center()
                                .gap_2()
                                .border_b_1()
                                .border_color(theme.border)
                                .child(div().text_xs().text_color(theme.text_muted).child("Jump"))
                                .child(div().flex_1().child(j.input.clone())),
                        )
                        .child(list),
                ),
        )
        .with_priority(4)
        .into_any_element(),
    )
}
