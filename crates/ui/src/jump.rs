//! Jump overlay: frecency candidates from the jump plugin as you type.
//!
//! Ranking reads the recently-used list and checks that each file is still
//! there, so candidates are asked for on a worker, and only the answer to
//! the latest query is shown.

use bibli_core::command::{Invocation, Outcome};
use gpui::{AnyElement, Context, Entity, Subscription, Window, div, prelude::*, px};
use serde::Deserialize;

use crate::input::{InputEvent, TextInput};
use crate::keymap;
use crate::modal::{self, FocusReturn, Picker};
use crate::shell::Shell;
use crate::theme::Theme;

#[derive(Clone, Debug, Deserialize)]
pub struct Candidate {
    #[serde(with = "bibli_core::os_path")]
    pub path: std::path::PathBuf,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub source: String,
}

pub struct JumpState {
    pub input: Entity<TextInput>,
    pub candidates: Vec<Candidate>,
    pub picker: Picker,
    /// The query asked of the plugin last; its answer may still be coming.
    asked: Option<String>,
    /// The query its candidates answer: until the one asked is answered,
    /// an empty list says nothing, rather than "No candidates".
    answered: Option<String>,
    /// Enter was pressed before the query typed was answered: it goes
    /// where the answer's first candidate leads once it comes, never where
    /// an older query's did.
    submitted: bool,
    back: FocusReturn,
    _sub: Subscription,
}

impl Shell {
    pub fn open_jump(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let back = FocusReturn::capture(window, cx);
        let input = cx.new(|cx| TextInput::new("Jump to a folder or recent file", cx));
        let sub = cx.subscribe_in(
            &input,
            window,
            |this, _, ev: &InputEvent, window, cx| match ev {
                InputEvent::Submit => this.jump_submit(window, cx),
                InputEvent::Cancel => this.close_jump(window, cx),
                InputEvent::Changed => this.jump_refresh(window, cx),
            },
        );
        input.update(cx, |i, _| i.focus(window));
        self.jump = Some(JumpState {
            input,
            candidates: Vec::new(),
            picker: Picker::default(),
            asked: None,
            answered: None,
            submitted: false,
            back,
            _sub: sub,
        });
        self.jump_refresh(window, cx);
        cx.notify();
    }

    pub fn close_jump(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(jump) = self.jump.take() {
            jump.back.restore(window, &self.ui().focus);
        }
        cx.notify();
    }

    pub fn jump_move(&mut self, mv: crate::modal::ListMove, cx: &mut Context<Self>) {
        if let Some(j) = &mut self.jump {
            j.picker.go(mv, j.candidates.len());
            cx.notify();
        }
    }

    fn jump_submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(j) = &mut self.jump
            && j.answered != j.asked
        {
            j.submitted = true;
            return;
        }
        let target = self.jump.as_ref().and_then(|j| {
            j.candidates
                .get(j.picker.highlighted)
                .map(|c| c.path.clone())
        });
        self.close_jump(window, cx);
        if let Some(path) = target {
            // A folder opens; a file is selected in its folder. Which one it
            // is, and whether it is still there, is asked on a worker.
            let pane = self.pane;
            self.reveal_path(pane, &path, window, cx);
        }
    }

    /// Ask the plugin for candidates on a worker when the query changed.
    fn jump_refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(j) = &mut self.jump else { return };
        let query = j.input.read(cx).text().to_owned();
        if j.asked.as_deref() == Some(query.as_str()) {
            return;
        }
        j.asked = Some(query.clone());
        let app = std::sync::Arc::clone(&self.app);
        let args = serde_json::json!({ "query": query, "limit": 30 });
        self.off_thread(
            "jump-candidates",
            move || app.dispatch(&Invocation::with_args("jump.candidates", &args)),
            move |this, result, window, cx| {
                let Some(j) = this
                    .jump
                    .as_mut()
                    .filter(|j| j.asked.as_ref() == Some(&query))
                else {
                    return;
                };
                j.answered = Some(query);
                match result {
                    Ok(Outcome::Value(value)) => {
                        j.candidates = serde_json::from_value(value).unwrap_or_default();
                    }
                    Ok(_) => j.candidates.clear(),
                    Err(error) => {
                        j.candidates.clear();
                        this.set_status(format!("{error:#}"), true, cx);
                    }
                }
                if let Some(j) = &mut this.jump {
                    j.picker = Picker::default();
                }
                if this.jump.as_ref().is_some_and(|j| j.submitted) {
                    return this.jump_submit(window, cx);
                }
                cx.notify();
            },
            window,
            cx,
        );
    }
}

pub fn render(shell: &Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let j = shell.jump.as_ref()?;
    let theme = cx.global::<Theme>().clone();
    let mut list = div().flex().flex_col().py_1().overflow_hidden();
    for ix in j.picker.shown(j.candidates.len()) {
        let c = &j.candidates[ix];
        let hl = ix == j.picker.highlighted;
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
                .when(!hl, |d| d.hover(|s| s.bg(theme.hover)))
                .on_click(cx.listener(move |this, _, window, cx| {
                    if let Some(j) = &mut this.jump {
                        j.picker.highlighted = ix;
                    }
                    this.jump_submit(window, cx);
                }))
                .child(crate::elements::clip::clipped(
                    div()
                        .id("jump-path")
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis_middle(),
                    c.path.to_string_lossy().into_owned(),
                    shell.font.clone(),
                ))
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
    if j.candidates.is_empty() && j.answered == j.asked {
        list = list.child(
            div()
                .px_3()
                .py_2()
                .text_color(theme.text_muted)
                .child("No candidates"),
        );
    }
    let panel = modal::panel(640., &theme)
        .key_context(keymap::JUMP)
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
        .child(list);
    Some(modal::modal(panel, 80., 4, Shell::close_jump, &theme, cx))
}
