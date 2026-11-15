//! Single-line prompts (`ui.ask`: rename, new folder, a plugin's
//! question; finding a name; what a plan's step or a plugin asks that
//! only the user knows, a password or a volume's passphrase) and the
//! confirmation of what Undo cannot take back, or of quitting while a plan
//! runs.

use std::rc::Rc;
use std::sync::Arc;

use crate::sizes::TextSizes;
use crate::sizes::sized;
use gpui::{AnyElement, Context, Entity, FocusHandle, Subscription, Window, div, prelude::*};
use skwir_core::Plan;
use skwir_core::command::{CommandSpec, Invocation, Outcome};
use skwir_core::op::{EntryArg, EntryRef};
use skwir_core::os_path::PathArg;
use skwir_core::reference::Reference;
use skwir_core::scheduler::{Lane, Priority};
use skwir_core::workspace::PaneId;

use crate::controls::{self, ButtonStyle};
use crate::input::{InputEvent, TextInput};
use crate::keymap;
use crate::latest::Latest;
use crate::modal::{self, FocusReturn};
use crate::reference::FieldHelp;
use crate::shell::Shell;
use crate::theme::Theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptKind {
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
    /// What a plan's step or a plugin asks that only the user knows (an
    /// archive's password, a server's, a volume's passphrase:
    /// `App::ask_secret`): masked, never trimmed, answered to what waits
    /// for it, `None` however the prompt closes else.
    Secret { id: u64 },
}

impl PromptKind {
    /// Whether what is typed is hidden and kept as typed, spaces and all.
    fn secret(self) -> bool {
        matches!(self, PromptKind::Secret { .. })
    }

    /// What its answer does, on the button Enter presses (`ui.ask` names
    /// its own).
    fn accept(self) -> &'static str {
        match self {
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
    /// A server's password, a volume's passphrase: offer to keep it in the
    /// keyring (ADR-082, ADR-088).
    remember: bool,
    /// What keeping it means, said under Remember while it is on.
    keeping: Option<String>,
    /// What the field takes (ADR-137).
    reference: Option<Reference>,
    /// What says what the text comes to as it is typed.
    describe: Option<Describe>,
}

/// What a text was said to come to: the text, and what it comes to or why
/// it is no answer.
type Said = (String, Result<String, String>);

/// What answering does with `text` beside what was last said (`said`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Answering {
    /// It was said to be an answer: it goes.
    Goes,
    /// It has yet to be said: it waits for it.
    Waits,
    /// It was said to be no answer: held back.
    Refused,
}

fn answering(said: Option<&Said>, text: &str) -> Answering {
    match said {
        Some((said, Ok(_))) if said == text => Answering::Goes,
        Some((said, Err(_))) if said == text => Answering::Refused,
        _ => Answering::Waits,
    }
}

/// What says what a question's text comes to as it is typed (`ui.ask`'s
/// `describe`, ADR-137): a command that only answers, asked at each change
/// on a worker of the shared lane (it reads nothing a remote holds), the
/// latest ask alone answered, cancelled by the next and with the question.
struct Describe {
    name: Arc<str>,
    /// Its arguments but the text, made once as the question opens: what
    /// it is about may be many entries, never copied on the window's
    /// thread as the text is typed.
    args: Arc<serde_json::Value>,
    /// The argument the text is.
    field: String,
    /// It runs as an agent's call: an agent asked the question.
    agent: bool,
    asked: Latest<Said>,
    /// The latest answer: drawn until a newer one comes.
    said: Option<Said>,
    /// The question was answered (Enter, its button) before its text was
    /// said: answered once it is, unless it is no answer. A key since
    /// forgets it.
    answering: bool,
    /// That answer has waited `work::SHOWN_AFTER`: the line under the
    /// field says it is being worked out (`WORKING_OUT`).
    waited: bool,
    /// What makes `waited` true once that time is up; dropped, nothing.
    wait: Option<gpui::Task<()>>,
}

/// What the line under a question's field says while its answer waits for
/// what the text comes to (`Describe::waited`).
const WORKING_OUT: &str = "Working out what this does…";

/// How many lines what a question's text comes to takes under the field
/// at most: the rest is said when pointed at.
const SAID_LINES: usize = 3;

/// How much of what a describe says is kept, in bytes: a command's error
/// may name every file it is about, which no line under a field shows,
/// nor a tooltip, and laying it out at each frame would cost.
const SAID_MOST: usize = 2048;

/// `text`, at most `SAID_MOST` bytes of it, cut at a character and said
/// to be cut.
fn said_bounded(mut text: String) -> String {
    if text.len() > SAID_MOST {
        let mut end = SAID_MOST;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push('…');
    }
    text
}

impl Describe {
    /// What was typed moved on, or was said: an answer that waited for it
    /// is forgotten, or goes on, and the line no longer says it waits.
    fn stop_waiting(&mut self) -> bool {
        self.waited = false;
        self.wait = None;
        std::mem::take(&mut self.answering)
    }
}

/// The prompt's Remember switch (a server's password, ADR-082; a volume's
/// passphrase, ADR-088): whether it is on, its focus, and what keeping
/// the answer means, said under it while it is on (and to a screen reader
/// as its description, before it is).
pub struct Remember {
    pub on: bool,
    focus: FocusHandle,
    keeping: Option<String>,
}

pub struct PromptState {
    pub title: String,
    pub kind: PromptKind,
    pub input: Entity<TextInput>,
    ask: Option<Ask>,
    /// A line under the field, muted: what the answer may be.
    note: Option<String>,
    /// What the field takes (ADR-137): `ui.ask`'s `reference`, its `?`,
    /// F1 and the list under it (`reference.rs`).
    help: Option<FieldHelp>,
    /// What says what the text comes to as it is typed: `ui.ask`'s
    /// `describe`.
    describe: Option<Describe>,
    /// Remember in the keyring, where offered.
    pub remember: Option<Remember>,
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
    /// What a plan deletes, listed under the question (`deleting.rs`).
    pub(crate) listed: Option<crate::deleting::Deleting>,
    pub focus: FocusHandle,
    cancel: FocusHandle,
    /// The second answer, between Cancel and the first: Quit's "Stop and
    /// quit".
    other: FocusHandle,
    accept: FocusHandle,
    back: FocusReturn,
}

/// Why `ui.ask`'s `describe` in `args` is refused, if it is: it runs as
/// the text is typed, before anyone answers, so it only answers (a data
/// command, `CommandSpec::data`), never one that acts (ADR-137). Asked
/// where `ui.ask` is dispatched (an agent's, a script's) and where the
/// question opens (a menu's).
pub(crate) fn describe_refused(app: &skwir_core::App, args: &serde_json::Value) -> Option<String> {
    let describe = args.get("describe").filter(|d| !d.is_null())?;
    let Some(name) = describe.get("command").and_then(serde_json::Value::as_str) else {
        return Some("ui.ask's describe names no command".to_owned());
    };
    match app.commands.get(name) {
        Some(spec) if spec.data => None,
        Some(_) => Some(format!(
            "ui.ask's describe runs as the text is typed, before it is answered: \
             {name} does more than answer"
        )),
        None => Some(format!("ui.ask's describe names no command known: {name}")),
    }
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
    /// The help of its field when it is `field`, and the field.
    pub(crate) fn help_of(
        &mut self,
        field: gpui::EntityId,
    ) -> Option<(&mut FieldHelp, Entity<TextInput>)> {
        let input = self.input.clone();
        self.help
            .as_mut()
            .filter(|_| input.entity_id() == field)
            .map(|help| (help, input))
    }

    /// Whether `field` is its field, one that says what it takes.
    pub(crate) fn helps(&self, field: gpui::EntityId) -> bool {
        self.help.is_some() && self.input.entity_id() == field
    }

    /// Whether its field, one that says what it takes, is named `key`:
    /// what a recipe of its own fills.
    pub(crate) fn names(&self, key: &str) -> bool {
        self.help.as_ref().is_some_and(|help| help.key == key)
    }

    /// What Tab walks: the field, Remember where offered, Cancel, the
    /// answer (`Shell::walk_dialog`).
    pub(crate) fn walk(&self, cx: &gpui::App) -> Vec<FocusHandle> {
        use gpui::Focusable as _;
        let mut walk = vec![self.input.read(cx).focus_handle(cx)];
        walk.extend(self.remember.as_ref().map(|r| r.focus.clone()));
        walk.extend([self.cancel_focus.clone(), self.accept_focus.clone()]);
        walk
    }
}

impl ConfirmState {
    /// The panel's own focus, which Enter and Escape answer from, and what
    /// Tab walks from there: Cancel, the second answer where there is
    /// one, then the first.
    pub(crate) fn walk(&self) -> (FocusHandle, Vec<FocusHandle>) {
        let mut stops = vec![self.cancel.clone()];
        if matches!(self.then, Confirmed::Quit) {
            stops.push(self.other.clone());
        }
        stops.push(self.accept.clone());
        (self.focus.clone(), stops)
    }
}

/// What answering yes does.
pub enum Confirmed {
    /// Apply a plan Undo cannot take back. The plan itself, built when
    /// the question was asked: what the user read about is what happens,
    /// whatever the selection or the active pane is by then.
    Apply(Plan),
    /// Quit while plans run (`Shell::quit`, ADR-115): yes finishes them
    /// without the window, then quits; the second answer stops them and
    /// quits now.
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
        if self.modal_open_beside_settings() || self.review_focus.is_some() {
            return;
        }
        let Some(question) = self.secrets.pop_front() else {
            return;
        };
        self.secret_shown = Some(question.clone());
        self.open_prompt_with(
            PromptKind::Secret { id: question.id },
            &question.title,
            &question.placeholder,
            "",
            None,
            Prompted {
                ask: None,
                accept: Some(question.accept),
                note: None,
                remember: question.remember,
                keeping: question.keeping,
                ..Prompted::default()
            },
            window,
            cx,
        );
    }

    /// `ui.ask { title, text?, placeholder?, select?, accept?, note?,
    /// reference?, describe?, command, args?, field }`: ask for a line of
    /// text and run `command` with it as `args[field]`. `select` is the
    /// part of `text` selected at first (`[start, end]` in bytes: a name's
    /// stem), all of it when absent; `accept` is the verb on the button
    /// that answers (Rename), OK when absent; `note` a line under the field
    /// (what the answer may be); `reference` what the field takes
    /// (`skwir_core::reference::Reference`, ADR-137), from a `?` inside it
    /// or F1; `describe` (`{ command, args? }`) what says, as it is
    /// typed, what the text comes to: run on a worker with the text as its
    /// `field`, about what the question is about (the `pane`, `entries` and
    /// `in` its command was given), its `says` shown under the field, its
    /// error shown there and holding the answer back.
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
        if let Some(refused) = describe_refused(&self.app, args) {
            self.set_status(refused, true, cx);
            return;
        }
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
        // What says what is typed comes to asks about what the question is
        // about too: the selection's paths for Run Command's line.
        let describe = args.get("describe").and_then(|describe| {
            let mut said = Invocation {
                name: describe.get("command")?.as_str()?.to_owned().into(),
                args: describe
                    .get("args")
                    .cloned()
                    .filter(serde_json::Value::is_object)
                    .unwrap_or_else(|| serde_json::json!({})),
            };
            if let Some(spec) = self.app.commands.get(&said.name) {
                let pane = command
                    .args
                    .get("pane")
                    .and_then(|pane| serde_json::from_value::<PaneId>(pane.clone()).ok())
                    .unwrap_or(self.pane);
                // The same entries and folder as the question's command.
                for named in ["pane", "entries", "in"] {
                    if let Some(value) = command.args.get(named)
                        && spec.takes(named)
                        && said.args.get(named).is_none()
                    {
                        said.args[named] = value.clone();
                    }
                }
                let app = &self.app;
                asked_about(
                    &spec,
                    &mut said.args,
                    pane,
                    || app.selected_refs(pane),
                    || app.pane_dir(pane).ok(),
                );
            }
            Some(Describe {
                name: said.name,
                args: Arc::new(said.args),
                field: field.to_owned(),
                agent: describe
                    .get("agent")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
                asked: Latest::default(),
                said: None,
                answering: false,
                waited: false,
                wait: None,
            })
        });
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
                remember: false,
                keeping: None,
                reference: args
                    .get("reference")
                    .and_then(|said| serde_json::from_value(said.clone()).ok()),
                describe,
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
        let Prompted {
            ask,
            accept,
            note,
            remember,
            keeping,
            reference,
            describe,
        } = prompted;
        let accept = accept.unwrap_or_else(|| kind.accept().to_owned());
        let back = FocusReturn::capture(window, cx);
        let input = cx.new(|cx| TextInput::new(placeholder, cx));
        input.update(cx, |i, cx| {
            // The field is named by what is asked.
            i.set_label(title.to_owned());
            i.set_masked(kind.secret());
            i.set_text(initial.to_owned(), cx);
            match select {
                Some(r) => i.select_range(r, cx),
                None => i.select_all_now(cx),
            }
        });
        // What the field lists takes Enter the arrows chose a row for, and
        // Escape first (ADR-137).
        let help = reference
            .filter(|said| !said.is_empty())
            .zip(ask.as_ref())
            .map(|(said, ask)| FieldHelp::new(ask.field.clone(), Some(Rc::new(said))));
        let helped = help.is_some();
        let sub = cx.subscribe_in(
            &input,
            window,
            move |this, input, ev: &InputEvent, window, cx| match ev {
                InputEvent::Submit => {
                    if !(helped && crate::reference::on_submit(this, input.entity_id(), window, cx))
                    {
                        this.prompt_answer(window, cx);
                    }
                }
                InputEvent::Cancel => {
                    if !(helped && crate::reference::on_cancel(this, input.entity_id(), window, cx))
                    {
                        this.prompt_cancel(window, cx);
                    }
                }
                InputEvent::Changed => {
                    this.prompt_describe(cx);
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
            help,
            describe,
            remember: remember.then(|| Remember {
                on: false,
                focus: cx.focus_handle(),
                keeping,
            }),
            accept,
            accept_focus: cx.focus_handle(),
            cancel_focus: cx.focus_handle(),
            back,
            _sub: sub,
        });
        self.prompt_describe(cx);
        cx.notify();
    }

    /// Ask what the text comes to (`ui.ask`'s `describe`), on a worker, in
    /// place of what was asked before: the answer is said under the field
    /// once it comes (`sync_prompt_describe`).
    fn prompt_describe(&mut self, cx: &mut Context<Self>) {
        let app = Arc::clone(&self.app);
        // Said in this window alone.
        let wake = self.waker();
        let Some(prompt) = self.prompt.as_mut() else {
            return;
        };
        let Some(describe) = prompt.describe.as_mut() else {
            return;
        };
        let text = prompt.input.read(cx).text().trim().to_owned();
        describe.stop_waiting();
        let (name, args, field, agent) = (
            Arc::clone(&describe.name),
            Arc::clone(&describe.args),
            describe.field.clone(),
            describe.agent,
        );
        let worker = Arc::clone(&app);
        describe.asked.ask_waking(
            &app,
            Lane::SHARED,
            app.lifetime(),
            Priority::Visible,
            "prompt-describe",
            wake,
            move |_| {
                let mut args = Arc::unwrap_or_clone(args);
                args[field.as_str()] = serde_json::json!(text);
                let invocation = Invocation { name, args };
                // An agent's question says it as the agent's call would.
                let said = if agent {
                    skwir_core::permissions::as_agent(worker.privacy(), || {
                        worker.dispatch(&invocation)
                    })
                } else {
                    worker.dispatch(&invocation)
                };
                let said = match said {
                    Ok(Outcome::Value(v)) => Ok(said_bounded(
                        v.get("says")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                    )),
                    Ok(_) => Ok(String::new()),
                    Err(e) => Err(said_bounded(format!("{e:#}"))),
                };
                (text, said)
            },
        );
    }

    /// Take what the question's text was said to come to, as the core
    /// wakes the window; an answer that waited for it goes on.
    pub(crate) fn sync_prompt_describe(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(describe) = self.prompt.as_mut().and_then(|p| p.describe.as_mut()) else {
            return;
        };
        let Some(said) = describe.asked.take() else {
            return;
        };
        describe.said = Some(said);
        let answering = describe.stop_waiting();
        cx.notify();
        if answering {
            self.prompt_answer(window, cx);
        }
    }

    /// Whether the answer waits: what is typed was said to be no answer,
    /// or has yet to be said, and is answered once it is.
    fn prompt_waits(&mut self, cx: &gpui::App) -> bool {
        let Some(prompt) = self.prompt.as_mut() else {
            return false;
        };
        let text = prompt.input.read(cx).text().trim();
        let Some(describe) = prompt.describe.as_mut() else {
            return false;
        };
        match answering(describe.said.as_ref(), text) {
            Answering::Goes => false,
            Answering::Refused => true,
            Answering::Waits => {
                describe.answering = true;
                true
            }
        }
    }

    /// An answer waits for what its text comes to: once it has waited
    /// `work::SHOWN_AFTER`, the line under the field says so (and a screen
    /// reader hears it once), Cancel and Escape answering still; it waits
    /// for as long as it takes.
    fn prompt_wait_said(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(describe) = self.prompt.as_mut().and_then(|p| p.describe.as_mut()) else {
            return;
        };
        if !describe.answering || describe.wait.is_some() {
            return;
        }
        describe.wait = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor()
                .timer(crate::work::SHOWN_AFTER)
                .await;
            let _ = this.update(cx, |shell, cx| {
                if let Some(describe) = shell
                    .prompt
                    .as_mut()
                    .and_then(|p| p.describe.as_mut())
                    .filter(|d| d.answering)
                {
                    describe.waited = true;
                    cx.notify();
                }
            });
        }));
    }

    /// Answer the prompt with its text: Enter, or its button.
    fn prompt_answer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.prompt_waits(cx) {
            self.prompt_wait_said(window, cx);
            return;
        }
        let Some(prompt) = self.prompt.as_mut() else {
            return;
        };
        let (kind, ask) = (prompt.kind, prompt.ask.take());
        let remember = prompt.remember.as_ref().is_some_and(|r| r.on);
        let text = prompt.input.read(cx).text();
        // Spaces can be part of a passphrase.
        let text = if kind.secret() {
            text.to_owned()
        } else {
            text.trim().to_owned()
        };
        // Told before the prompt closes, which would tell it none.
        if let PromptKind::Secret { id } = kind {
            let answer = skwir_core::app::SecretAnswer {
                text: text.clone(),
                remember,
            };
            self.app.answer_secret(id, Some(answer));
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

    /// Turn the prompt's Remember on or off: a click, or Space on it.
    fn prompt_toggle_remember(&mut self, cx: &mut Context<Self>) {
        if let Some(remember) = self.prompt.as_mut().and_then(|p| p.remember.as_mut()) {
            remember.on = !remember.on;
            cx.notify();
        }
    }

    /// Close the prompt unanswered: Escape, Cancel, or a press outside.
    fn prompt_cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
    pub(crate) fn give_up_secret(&self, kind: PromptKind) {
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
            listed: None,
            focus,
            cancel: cx.focus_handle(),
            other: cx.focus_handle(),
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
                // Finished without the window (`finishing.rs`).
                Confirmed::Quit => self.close_every_window(window, cx),
            }
            cx.notify();
        }
    }

    /// The second answer: Quit's "Stop and quit".
    pub fn confirm_other(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(c) = self.confirm.take()
            && matches!(c.then, Confirmed::Quit)
        {
            self.stop_and_quit(window, cx);
        }
    }

    pub fn confirm_dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(c) = self.confirm.take() {
            c.back.restore(window, &self.ui().focus, cx);
            cx.notify();
        }
    }
}

pub fn render_prompt(
    shell: &Shell,
    window: &Window,
    cx: &mut Context<Shell>,
) -> Option<AnyElement> {
    let p = shell.prompt.as_ref()?;
    let theme = cx.global::<Theme>().clone();
    // What the text typed comes to: the last said, until a newer one
    // comes; what is no answer is said in the error's color, its frame
    // too, while it is the text's.
    let text = p.input.read(cx).text().trim().to_owned();
    let said = p.describe.as_ref().and_then(|d| d.said.as_ref());
    let refused = answering(said, &text) == Answering::Refused;
    // An answer that waits for its text to be said says so, in place of
    // what was said of the text before.
    let waited = p.describe.as_ref().is_some_and(|d| d.answering && d.waited);
    let frame = div()
        .h(sized(30.))
        .px_2()
        .flex()
        .items_center()
        .gap_1()
        .rounded_md()
        .border_1()
        .border_color(if refused { theme.error } else { theme.accent })
        .bg(theme.bg)
        .child(div().flex_1().min_w_0().child(p.input.clone()));
    // A field that says what it takes (ADR-137): its `?`, F1, and the list
    // under it.
    let frame = match &p.help {
        Some(help) => crate::reference::with_help(
            frame,
            help,
            &p.input,
            shell.keys_for("ui.field_reference", &serde_json::Value::Null),
            window,
            cx,
        ),
        None => frame,
    };
    let panel = modal::panel("prompt", 560., &theme)
        .when(shell.a11y, |d| d.aria_label(p.title.clone()))
        .p_3()
        .gap_2()
        .child(div().text_title().child(p.title.clone()))
        .child(frame)
        .children(waited.then(|| {
            div()
                .id("prompt-said")
                .role(gpui::Role::Status)
                .aria_live(gpui::Live::Polite)
                .when(shell.a11y, |d| d.aria_label(WORKING_OUT))
                .text_body()
                .text_color(theme.text_muted)
                .child(WORKING_OUT)
        }))
        .children(said.filter(|_| !waited).and_then(|(of, said)| {
            let (says, color) = match said {
                Ok(says) => (says, theme.text_muted),
                Err(why) if refused => (why, theme.error),
                Err(why) => (why, theme.text_muted),
            };
            // Heard as it is said of the text typed (a polite status).
            let heard = shell.a11y && *of == text;
            // A few lines at most, the rest said when pointed at.
            (!says.is_empty()).then(|| {
                crate::elements::clip::clipped(
                    div()
                        .id("prompt-said")
                        .role(gpui::Role::Status)
                        .aria_live(gpui::Live::Polite)
                        .when(heard, |d| d.aria_label(says.clone()))
                        .text_body()
                        .text_color(color)
                        .overflow_hidden()
                        .text_ellipsis()
                        .line_clamp(SAID_LINES),
                    says.clone(),
                )
            })
        }))
        .children(
            p.note
                .clone()
                .map(|note| div().text_body().text_color(theme.text_muted).child(note)),
        )
        .children(p.remember.as_ref().map(|remember| {
            let label = "Remember in the keyring";
            // Each its own click, the switch's answering Space and Enter
            // where it has the focus.
            let switch = div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    controls::switch(
                        "prompt-remember-switch",
                        remember.on,
                        &remember.focus,
                        &theme,
                    )
                    .aria_label(label)
                    .when_some(remember.keeping.clone(), |d, keeping| {
                        d.aria_description(keeping)
                    })
                    .on_click(cx.listener(|this, _, _, cx| this.prompt_toggle_remember(cx))),
                )
                .child(
                    div()
                        .id("prompt-remember-label")
                        .cursor_pointer()
                        .text_color(theme.text_muted)
                        .on_click(cx.listener(|this, _, _, cx| this.prompt_toggle_remember(cx)))
                        .child(label),
                );
            // What keeping it means, once the user means to.
            let keeping = remember
                .keeping
                .clone()
                .filter(|_| remember.on)
                .map(|keeping| {
                    div()
                        .text_body()
                        .text_color(theme.text_muted)
                        .child(keeping)
                });
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(switch)
                .children(keeping)
        }))
        .child(modal::footer(
            "Enter to confirm, Escape to cancel",
            [
                modal::pressed(
                    controls::button(
                        "prompt-cancel",
                        "Cancel",
                        None,
                        ButtonStyle::Plain,
                        &p.cancel_focus,
                        &theme,
                    ),
                    Shell::prompt_cancel,
                    cx,
                ),
                modal::pressed(
                    // What is typed is no answer: said, not offered, its
                    // icon kept so Cancel stays where it is.
                    if refused {
                        controls::inert_button_with(
                            "prompt-accept",
                            p.accept.clone(),
                            Some("check"),
                            &p.accept_focus,
                            &theme,
                        )
                    } else {
                        controls::button(
                            "prompt-accept",
                            p.accept.clone(),
                            Some("check"),
                            ButtonStyle::Plain,
                            &p.accept_focus,
                            &theme,
                        )
                    },
                    Shell::prompt_answer,
                    cx,
                ),
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
    let (yes, icon, style) = match c.then {
        Confirmed::Apply(_) => ("Delete", Some("trash"), ButtonStyle::Danger),
        Confirmed::Quit => ("Finish in the background", None, ButtonStyle::Plain),
    };
    let (no, other) = match c.then {
        Confirmed::Apply(_) => ("Cancel", None),
        Confirmed::Quit => ("Keep the window", Some("Stop and quit")),
    };
    // A question that asks before something is done says itself at once.
    let width = if c.listed.is_some() { 640. } else { 560. };
    let panel = modal::panel("confirm", width, &theme)
        .role(gpui::Role::AlertDialog)
        .when(shell.a11y, |d| d.aria_label(c.message.clone()))
        .p_3()
        .gap_2()
        .track_focus(&c.focus)
        .key_context(keymap::CONFIRM)
        .child(div().text_title().child(c.message.clone()))
        .children(
            c.listed
                .as_ref()
                .map(|listed| crate::deleting::render(listed, shell, &theme, cx)),
        )
        .child(modal::footer(
            match c.then {
                Confirmed::Apply(_) => "Enter to confirm, Escape to cancel",
                Confirmed::Quit => "Enter to finish in the background",
            },
            [
                Some(modal::pressed(
                    controls::button(
                        "confirm-no",
                        no,
                        None,
                        ButtonStyle::Plain,
                        &c.cancel,
                        &theme,
                    ),
                    Shell::confirm_dismiss,
                    cx,
                )),
                other.map(|other| {
                    modal::pressed(
                        controls::button(
                            "confirm-other",
                            other,
                            None,
                            ButtonStyle::Danger,
                            &c.other,
                            &theme,
                        ),
                        Shell::confirm_other,
                        cx,
                    )
                }),
                Some(modal::pressed(
                    controls::button("confirm-yes", yes, icon, style, &c.accept, &theme),
                    Shell::confirm_accept,
                    cx,
                )),
            ]
            .into_iter()
            .flatten(),
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

    use super::{Answering, SAID_MOST, answering, asked_about, said_bounded};

    fn selected() -> Vec<EntryRef> {
        vec![EntryRef::new(
            FileId::new(1, 7),
            "/tmp/example/a file's name.txt",
        )]
    }

    /// A question's answer waits for what its text comes to, goes once it
    /// is an answer, and is held back while it is no answer: an answer
    /// said of another text (the one before a key) decides nothing.
    #[test]
    fn an_answer_waits_for_its_text_to_be_said() {
        let said = |text: &str, ok: bool| {
            (
                text.to_owned(),
                if ok {
                    Ok("rw-r--r--".to_owned())
                } else {
                    Err("no mode".to_owned())
                },
            )
        };
        assert_eq!(answering(None, "644"), Answering::Waits);
        assert_eq!(answering(Some(&said("64", true)), "644"), Answering::Waits);
        assert_eq!(
            answering(Some(&said("64x", false)), "644"),
            Answering::Waits
        );
        assert_eq!(answering(Some(&said("644", true)), "644"), Answering::Goes);
        assert_eq!(
            answering(Some(&said("u+", false)), "u+"),
            Answering::Refused
        );
    }

    /// What a describe says is kept to a few lines' worth, cut at a
    /// character and said to be cut: an error naming thousands of files
    /// is not laid out whole under the field.
    #[test]
    fn what_is_said_of_a_text_is_bounded() {
        assert_eq!(said_bounded("rw-r--r--".to_owned()), "rw-r--r--");
        let long = "é".repeat(SAID_MOST);
        let cut = said_bounded(long);
        assert!(cut.len() <= SAID_MOST + '…'.len_utf8());
        assert!(cut.ends_with('…'));
        assert!(cut.trim_end_matches('…').chars().all(|c| c == 'é'));
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
