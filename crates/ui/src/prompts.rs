//! Single-line prompts (`ui.ask`: rename, new folder, a plugin's
//! question; finding a name; a passphrase; what a plan's step asks that
//! only the user knows) and the confirmation of what Undo cannot take
//! back, or of quitting while a plan runs.

use gpui::{AnyElement, Context, Entity, FocusHandle, Subscription, Window, div, prelude::*, px};
use skwir_core::Plan;
use skwir_core::command::{CommandSpec, Invocation};
use skwir_core::op::{EntryArg, EntryRef};
use skwir_core::os_path::PathArg;
use skwir_core::workspace::PaneId;

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
    /// What a plan's step asks that only the user knows (an archive's
    /// password, `App::ask_secret`): masked, never trimmed, answered to
    /// the step waiting for it, `None` however the prompt closes else.
    Secret { id: u64 },
}

impl PromptKind {
    /// Whether what is typed is hidden and kept as typed, spaces and all.
    fn secret(self) -> bool {
        matches!(self, PromptKind::Passphrase | PromptKind::Secret { .. })
    }

    /// What its answer does, on the button Enter presses (`ui.ask` names
    /// its own).
    fn accept(self) -> &'static str {
        match self {
            PromptKind::Passphrase => "Unlock",
            PromptKind::Find { .. } => "Find",
            PromptKind::InspectJump { .. } => "Go",
            PromptKind::Ask | PromptKind::Secret { .. } => "OK",
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

/// What a prompt is opened with beside its kind and text: `ui.ask`'s
/// command, the verb on its button (its kind's when none), a line under
/// its field.
#[derive(Default)]
struct Prompted {
    ask: Option<Ask>,
    accept: Option<String>,
    note: Option<String>,
}

pub struct PromptState {
    pub title: String,
    pub kind: PromptKind,
    pub input: Entity<TextInput>,
    ask: Option<Ask>,
    /// A line under the field, muted: what the answer may be.
    note: Option<String>,
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

/// Name in `args` what a question is about as it is asked (AGENTS.md,
/// "Pane ownership"): the pane, what is selected there and the folder it
/// shows, each that the asked command takes (`pane`, `entries`, `in`,
/// commands.md) and the asker left out. The answer acts on what the
/// question was put about (the file Rename was opened for), never on what
/// is selected or shown by the time it comes.
fn asked_about(
    spec: &CommandSpec,
    args: &mut serde_json::Value,
    pane: PaneId,
    selection: impl FnOnce() -> Vec<EntryRef>,
    folder: impl FnOnce() -> Option<std::path::PathBuf>,
) {
    let Some(named) = args.as_object_mut() else {
        return;
    };
    let wants = |named: &serde_json::Map<String, serde_json::Value>, arg: &str| {
        spec.takes(arg) && named.get(arg).is_none_or(serde_json::Value::is_null)
    };
    if wants(named, "pane") {
        named.insert("pane".into(), serde_json::json!(pane));
    }
    if wants(named, "entries") {
        let entries: Vec<EntryArg> = selection().into_iter().map(EntryArg::from).collect();
        if !entries.is_empty() {
            named.insert("entries".into(), serde_json::json!(entries));
        }
    }
    if wants(named, "in")
        && let Some(folder) = folder()
    {
        named.insert("in".into(), serde_json::json!(PathArg(folder)));
    }
}

impl PromptState {
    /// What Tab walks: the field, Cancel, the answer (`Shell::walk_dialog`).
    pub(crate) fn walk(&self, cx: &gpui::App) -> Vec<FocusHandle> {
        use gpui::Focusable as _;
        vec![
            self.input.read(cx).focus_handle(cx),
            self.cancel_focus.clone(),
            self.accept_focus.clone(),
        ]
    }
}

impl ConfirmState {
    /// The panel's own focus, which Enter and Escape answer from, and what
    /// Tab walks from there: Cancel, then the answer.
    pub(crate) fn walk(&self) -> (FocusHandle, Vec<FocusHandle>) {
        (
            self.focus.clone(),
            vec![self.cancel.clone(), self.accept.clone()],
        )
    }
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
            PromptKind::Secret { .. } => "Password",
        };
        self.open_prompt_with(
            kind,
            title,
            placeholder,
            initial,
            select,
            Prompted::default(),
            window,
            cx,
        );
    }

    /// Ask what plans' steps wait for (`secrets`) once the prompt is free,
    /// and no dialog or review holds the keyboard (a review is drawn
    /// above a prompt): one at a time, the first first. A question its
    /// step stopped waiting for (its plan cancelled) is not asked, or no
    /// longer.
    pub fn ask_secrets(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let app = std::sync::Arc::clone(&self.app);
        self.secrets.retain(|q| app.secret_waits(q.id));
        if let Some(PromptKind::Secret { id }) = self.prompt.as_ref().map(|p| p.kind)
            && !app.secret_waits(id)
        {
            self.close_prompt(window, cx);
        }
        if self.modal_open() || self.review_focus.is_some() {
            return;
        }
        let Some(question) = self.secrets.pop_front() else {
            return;
        };
        self.open_prompt_with(
            PromptKind::Secret { id: question.id },
            &question.title,
            "Password",
            "",
            None,
            Prompted {
                ask: None,
                accept: Some(question.accept),
                note: None,
            },
            window,
            cx,
        );
    }

    /// `ui.ask { title, text?, placeholder?, select?, accept?, note?,
    /// command, args?, field }`: ask for a line of text and run `command`
    /// with it as `args[field]`. `select` is the part of `text` selected at
    /// first (`[start, end]` in bytes: a name's stem), all of it when
    /// absent; `accept` is the verb on the button that answers (Rename), OK
    /// when absent; `note` a line under the field (what the answer may be).
    /// The one question of a line of text, the shell's own (Rename, New
    /// folder) as a plugin's.
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
        let mut command = Invocation {
            name: command.into(),
            args: args
                .get("args")
                .cloned()
                .filter(serde_json::Value::is_object)
                .unwrap_or_else(|| serde_json::json!({})),
        };
        if let Some(spec) = self.app.commands.get(&command.name) {
            let pane = command
                .args
                .get("pane")
                .and_then(|pane| serde_json::from_value::<PaneId>(pane.clone()).ok())
                .unwrap_or(self.pane);
            let app = &self.app;
            asked_about(
                &spec,
                &mut command.args,
                pane,
                || app.selected_refs(pane),
                || app.pane_dir(pane).ok(),
            );
        }
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
            Prompted {
                accept: ask.accept.clone(),
                ask: Some(ask),
                note: text("note").map(str::to_owned),
            },
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
        prompted: Prompted,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // One replaced still waited for its answer: none.
        if let Some(old) = &self.prompt {
            self.give_up_secret(old.kind);
        }
        let placeholder = placeholder.to_owned();
        let Prompted { ask, accept, note } = prompted;
        let accept = accept.unwrap_or_else(|| kind.accept().to_owned());
        let back = FocusReturn::capture(window, cx);
        let input = cx.new(|cx| TextInput::new(placeholder, cx));
        input.update(cx, |i, cx| {
            i.set_masked(kind.secret());
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
        input.update(cx, |i, cx| i.focus(window, cx));
        self.prompt = Some(PromptState {
            title: title.to_owned(),
            kind,
            input,
            ask,
            note,
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
        let text = if kind.secret() {
            text.to_owned()
        } else {
            text.trim().to_owned()
        };
        // Told before the prompt closes, which would tell it none.
        if let PromptKind::Secret { id } = kind {
            self.app.answer_secret(id, Some(text.clone()));
        }
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

    /// Close the prompt and give the keyboard back to what had it. What a
    /// step asked and was not answered is answered none.
    pub fn close_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(prompt) = self.prompt.take() {
            self.give_up_secret(prompt.kind);
            prompt.back.restore(window, &self.ui().focus, cx);
        }
        cx.notify();
    }

    /// A step's question closed unanswered: it hears none (nothing, when
    /// it was answered already).
    fn give_up_secret(&self, kind: PromptKind) {
        if let PromptKind::Secret { id } = kind {
            self.app.answer_secret(id, None);
        }
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
            PromptKind::Ask | PromptKind::Secret { .. } => {}
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
        window.focus(&focus, cx);
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
            c.back.restore(window, &self.ui().focus, cx);
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
            c.back.restore(window, &self.ui().focus, cx);
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
        .children(
            p.note
                .clone()
                .map(|note| div().text_xs().text_color(theme.text_muted).child(note)),
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use skwir_core::FileId;
    use skwir_core::app::{App, AppConfig};
    use skwir_core::op::EntryRef;
    use skwir_core::workspace::PaneId;

    use super::asked_about;

    fn selected() -> Vec<EntryRef> {
        vec![EntryRef::new(
            FileId { dev: 1, ino: 7 },
            "/tmp/example/a file's name.txt",
        )]
    }

    /// Rename asks about the file selected when it opens: the answer names
    /// it, whatever is selected by then.
    #[test]
    fn a_question_names_what_it_is_about_when_asked() {
        let tmp = tempfile::tempdir().unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        let rename = app.commands.get("rename").unwrap();
        let mut args = serde_json::json!({ "pane": 3 });
        asked_about(&rename, &mut args, PaneId(9), selected, || None);
        assert_eq!(args["pane"], 3, "the pane the asker named");
        assert_eq!(
            args["entries"],
            serde_json::json!([{ "id": { "dev": 1, "ino": 7 }, "path": "/tmp/example/a file's name.txt" }])
        );
        assert!(app.commands.check(&skwir_core::command::Invocation {
            name: "rename".into(),
            args: serde_json::json!({ "new_name": "b", "entries": args["entries"].clone() }),
        })
        .is_ok());

        // A new folder is made in the folder shown when it was asked.
        let mkdir = app.commands.get("mkdir").unwrap();
        let mut args = serde_json::json!({});
        let folder = || Some(PathBuf::from("/tmp/example/shown"));
        asked_about(&mkdir, &mut args, PaneId(2), Vec::new, folder);
        assert_eq!(
            args,
            serde_json::json!({ "pane": 2, "in": "/tmp/example/shown" })
        );

        // What the asker named stays; what the command does not take is
        // never added.
        let mut args = serde_json::json!({ "entries": ["/tmp/example/b"] });
        asked_about(&rename, &mut args, PaneId(2), selected, || None);
        assert_eq!(args["entries"], serde_json::json!(["/tmp/example/b"]));
        let hidden = app.commands.get("toggle_hidden").unwrap();
        let mut args = serde_json::json!({});
        asked_about(&hidden, &mut args, PaneId(2), selected, folder);
        assert_eq!(args, serde_json::json!({ "pane": 2 }));
    }
}
