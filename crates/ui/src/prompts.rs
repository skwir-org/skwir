//! Single-line prompts (`ui.ask`: rename, new folder, a plugin's
//! question; finding a name; a passphrase) and the confirmation of what
//! Undo cannot take back, or of quitting while a plan runs.

use gpui::{AnyElement, Context, Entity, FocusHandle, Subscription, Window, div, prelude::*, px};
use skwir_core::Plan;
use skwir_core::command::Invocation;

use crate::controls::{self, ButtonStyle};
use crate::input::{InputEvent, TextInput};
use crate::keymap;
use crate::modal::{self, FocusReturn};
use crate::shell::Shell;
use crate::theme::Theme;

/// The encrypted volume the passphrase prompt is for, and whether it then
/// opens in a tab behind the one shown (a middle click on its place).
pub struct Unlock {
    pub device: String,
    pub tab: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptKind {
    /// The passphrase of `Shell::unlock`. Masked, never trimmed.
    Passphrase,
    /// `/`: a name in the pane, found as it is typed from where the cursor
    /// was (`origin`), after it or, `backward`, before it.
    Find {
        origin: Option<skwir_core::EntryId>,
        backward: bool,
    },
    InspectJump {
        pane: skwir_core::workspace::PaneId,
        generation: u64,
    },
    /// `ui.ask`: the text completes a command (`PromptState::ask`).
    Ask,
}

impl PromptKind {
    /// What its answer does, on the button Enter presses (`ui.ask` names
    /// its own).
    fn accept(self) -> &'static str {
        match self {
            PromptKind::Passphrase => "Unlock",
            PromptKind::Find { .. } => "Find",
            PromptKind::InspectJump { .. } => "Go",
            PromptKind::Ask => "OK",
        }
    }
}

/// What `ui.ask` runs with the answer: `command`, its `field` set to the
/// text. A plugin asks for a name without the shell knowing its command.
pub struct Ask {
    pub command: Invocation,
    pub field: String,
    /// The button's verb (Rename, Create), OK when the asker named none.
    pub accept: Option<String>,
}

pub struct PromptState {
    pub title: String,
    pub kind: PromptKind,
    pub input: Entity<TextInput>,
    ask: Option<Ask>,
    /// The answer's button: its verb and its focus, beside Cancel's.
    accept: String,
    accept_focus: FocusHandle,
    cancel_focus: FocusHandle,
    back: FocusReturn,
    _sub: Subscription,
}

/// A question before something that cannot be undone.
pub struct ConfirmState {
    pub message: String,
    pub then: Confirmed,
    pub focus: FocusHandle,
    cancel: FocusHandle,
    accept: FocusHandle,
    back: FocusReturn,
}

/// What answering yes does.
pub enum Confirmed {
    /// Apply a plan Undo cannot take back. The plan itself, built when
    /// the question was asked: what the user read about is what happens,
    /// whatever the selection or the active pane is by then.
    Apply(Plan),
    /// Quit, stopping the plans that run (`Shell::quit`).
    Quit,
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
            PromptKind::Find { .. } => "Part of a name (capitals: exact case)",
            PromptKind::Ask => "",
        };
        self.open_prompt_with(kind, title, placeholder, initial, select, None, window, cx);
    }

    /// `ui.ask { title, text?, placeholder?, select?, accept?, command,
    /// args?, field }`: ask for a line of text and run `command` with it as
    /// `args[field]`. `select` is the part of `text` selected at first
    /// (`[start, end]` in bytes: a name's stem), all of it when absent;
    /// `accept` is the verb on the button that answers (Rename), OK when
    /// absent. The one question of a line of text, the shell's own (Rename,
    /// New folder) as a plugin's.
    pub fn open_ask(
        &mut self,
        args: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = |key: &str| args.get(key).and_then(serde_json::Value::as_str);
        let (Some(command), Some(field)) = (text("command"), text("field")) else {
            self.set_status("ui.ask needs a command and a field", true, cx);
            return;
        };
        let select = args
            .get("select")
            .and_then(|s| serde_json::from_value::<[usize; 2]>(s.clone()).ok())
            .map(|[start, end]| start..end);
        let command = Invocation {
            name: command.into(),
            args: args
                .get("args")
                .cloned()
                .filter(serde_json::Value::is_object)
                .unwrap_or_else(|| serde_json::json!({})),
        };
        let ask = Ask {
            command,
            field: field.to_owned(),
            accept: text("accept").map(str::to_owned),
        };
        self.open_prompt_with(
            PromptKind::Ask,
            text("title").unwrap_or_default(),
            text("placeholder").unwrap_or_default(),
            text("text").unwrap_or_default(),
            select,
            Some(ask),
            window,
            cx,
        );
    }

    #[allow(clippy::too_many_arguments)] // Each is one part of the question.
    fn open_prompt_with(
        &mut self,
        kind: PromptKind,
        title: &str,
        placeholder: &str,
        initial: &str,
        select: Option<std::ops::Range<usize>>,
        ask: Option<Ask>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let placeholder = placeholder.to_owned();
        let accept = ask
            .as_ref()
            .and_then(|a| a.accept.clone())
            .unwrap_or_else(|| kind.accept().to_owned());
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
                InputEvent::Submit => this.prompt_answer(window, cx),
                InputEvent::Cancel => this.prompt_cancel(window, cx),
                InputEvent::Changed => {
                    if let Some(PromptKind::Find { origin, backward }) =
                        this.prompt.as_ref().map(|p| p.kind)
                    {
                        let pattern = input.read(cx).text().to_owned();
                        if let Some(ix) = this.find_from(origin, &pattern, backward) {
                            this.cursor_to(ix as isize, crate::list::Step::Follow, window, cx);
                        }
                    }
                }
            },
        );
        input.update(cx, |i, _| i.focus(window));
        self.prompt = Some(PromptState {
            title: title.to_owned(),
            kind,
            input,
            ask,
            accept,
            accept_focus: cx.focus_handle(),
            cancel_focus: cx.focus_handle(),
            back,
            _sub: sub,
        });
        cx.notify();
    }

    /// Answer the prompt with its text: Enter, or its button.
    fn prompt_answer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(prompt) = self.prompt.as_mut() else {
            return;
        };
        let (kind, ask) = (prompt.kind, prompt.ask.take());
        let text = prompt.input.read(cx).text();
        // Spaces can be part of a passphrase.
        let text = if kind == PromptKind::Passphrase {
            text.to_owned()
        } else {
            text.trim().to_owned()
        };
        self.close_prompt(window, cx);
        if let Some(Ask { command, field, .. }) = ask {
            let mut args = command.args;
            args[field.as_str()] = serde_json::json!(text);
            self.run(&command.name, args, window, cx);
        } else {
            self.prompt_submitted(kind, text, window, cx);
        }
        cx.notify();
    }

    /// Close the prompt unanswered: Escape, Cancel, or a press outside.
    fn prompt_cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.unlock = None;
        self.close_prompt(window, cx);
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
                let Some(Unlock { device, tab }) = self.unlock.take() else {
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
                    &serde_json::json!({
                        "device": device,
                        "passphrase": text,
                        "pane": pane,
                        "tab": tab,
                    }),
                );
                match self.app.dispatch(&unlock) {
                    Ok(_) => self.set_status("Unlocking…", false, cx),
                    Err(error) => self.set_status(format!("{error:#}"), true, cx),
                }
            }
            PromptKind::Find { backward, .. } => {
                self.last_find = (!text.is_empty()).then_some((text, backward));
            }
            PromptKind::Ask => {}
        }
    }

    /// Ask before applying `plan`, which cannot be undone.
    pub fn open_confirm(
        &mut self,
        message: String,
        then: Confirmed,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let back = FocusReturn::capture(window, cx);
        let focus = cx.focus_handle();
        window.focus(&focus);
        self.confirm = Some(ConfirmState {
            message,
            then,
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
            match c.then {
                // Asked and answered: applied as it is.
                Confirmed::Apply(plan) => self.app.apply_whole(plan),
                Confirmed::Quit => window.remove_window(),
            }
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
        .child(modal::footer(
            "Enter to confirm, Escape to cancel",
            [
                controls::button(
                    "prompt-cancel",
                    "Cancel",
                    None,
                    ButtonStyle::Plain,
                    &p.cancel_focus,
                    &theme,
                )
                .on_click(cx.listener(|this, _, window, cx| this.prompt_cancel(window, cx))),
                controls::button(
                    "prompt-accept",
                    p.accept.clone(),
                    Some("check"),
                    ButtonStyle::Plain,
                    &p.accept_focus,
                    &theme,
                )
                .on_click(cx.listener(|this, _, window, cx| this.prompt_answer(window, cx))),
            ],
            &theme,
        ));
    Some(modal::modal(
        panel,
        120.,
        3,
        Shell::prompt_cancel,
        &theme,
        cx,
    ))
}

pub fn render_confirm(shell: &Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let c = shell.confirm.as_ref()?;
    let theme = cx.global::<Theme>().clone();
    let (yes, icon) = match c.then {
        Confirmed::Apply(_) => ("Delete", Some("trash")),
        Confirmed::Quit => ("Quit", None),
    };
    let panel = modal::panel(560., &theme)
        .p_3()
        .gap_2()
        .track_focus(&c.focus)
        .key_context(keymap::CONFIRM)
        .child(div().child(c.message.clone()))
        .child(modal::footer(
            "Enter to confirm, Escape to cancel",
            [
                controls::button(
                    "confirm-no",
                    "Cancel",
                    None,
                    ButtonStyle::Plain,
                    &c.cancel,
                    &theme,
                )
                .on_click(cx.listener(|this, _, window, cx| this.confirm_dismiss(window, cx))),
                controls::button(
                    "confirm-yes",
                    yes,
                    icon,
                    ButtonStyle::Danger,
                    &c.accept,
                    &theme,
                )
                .on_click(cx.listener(|this, _, window, cx| this.confirm_accept(window, cx))),
            ],
            &theme,
        ));
    Some(modal::modal(
        panel,
        120.,
        3,
        Shell::confirm_dismiss,
        &theme,
        cx,
    ))
}
