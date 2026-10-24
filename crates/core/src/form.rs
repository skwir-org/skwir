//! A form the window asks the user to fill (ADR-086): fields a plugin
//! describes (a server's address, a cloud's keys, a choice among kinds),
//! drawn by the window, which knows no plugin's. What it holds goes back
//! to the asker alone, never through a command's arguments: a form is how
//! a secret reaches a plugin from the user, as a step's password does
//! (`App::ask_secret`), and no agent or script can answer one.
//!
//! The asker waits on a worker (`App::fill`, or `App::ask_form` and its
//! receiver); the window takes the forms (`App::take_forms`), shows one at
//! a time, and answers (`App::answer_form`): `None` for Cancel, else what
//! the fields hold and whether Back or the answer's button was pressed.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::time::Duration;

use crate::app::App;
use crate::cancel::CancelToken;

/// What the window shows the user to fill.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Form {
    /// What it asks, in a line: "Add Server", "Edit “photos”".
    pub title: String,
    /// What it is for, under the title; empty for nothing.
    pub intro: String,
    pub fields: Vec<Field>,
    /// The verb on its answer's button: "Add", "Save", "Connect".
    pub accept: String,
    /// It offers Back, to the asker's step before.
    pub back: bool,
    /// Why the last answer was refused, shown above its buttons.
    pub error: Option<String>,
    /// What it waits for, while the asker works (signing in in the
    /// browser): its fields and answer are set aside, Cancel alone
    /// answers, and the asker withdraws it once done.
    pub waiting: Option<String>,
}

/// One field of a [`Form`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Field {
    /// What the answer names it by.
    pub key: String,
    /// Its label.
    pub title: String,
    /// What it is, under it; empty for nothing.
    pub help: String,
    pub kind: Kind,
    /// What it holds as the form opens: its text, `true` or `false` for a
    /// switch, a choice's value. Never a secret's: a secret kept says so
    /// (`Kind::Secret { kept }`).
    pub value: String,
    /// The answer's button waits until it holds something.
    pub required: bool,
    /// Folded under the form's More Options until the user unfolds them
    /// (what seldom needs changing): the asker leaves one it holds a value
    /// of the user's in unfolded.
    pub advanced: bool,
}

/// What a field holds, which picks its control.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    /// A line of text.
    #[default]
    Text,
    /// A whole number between `min` and `max`.
    Integer { min: i64, max: i64 },
    /// A secret, masked and kept as typed. `kept`: one is stored already,
    /// and an empty field keeps it.
    Secret { kept: bool },
    /// On or off.
    Toggle,
    /// One of `choices`; with `open`, any text too, the choices being
    /// suggestions (a region's names).
    Choice { choices: Vec<Choice>, open: bool },
    /// One of many, chosen from a list the form shows filtered by what is
    /// typed (what kind of server): picking one answers the form.
    Pick { choices: Vec<Choice> },
    /// Where the user can go to do what the form waits for (a sign-in's
    /// page): `value` is the address, opened by the window's link.
    Link,
}

/// One choice of a [`Kind::Choice`] or [`Kind::Pick`] field.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Choice {
    /// What the answer holds.
    pub value: String,
    /// What the user reads; the value where empty.
    pub title: String,
    /// A line under it, muted; empty for none.
    pub detail: String,
    /// The heading it is listed under, consecutive choices of one sharing
    /// it; empty for none.
    pub section: String,
}

impl Choice {
    pub fn new(value: impl Into<String>, title: impl Into<String>) -> Self {
        Choice {
            value: value.into(),
            title: title.into(),
            ..Choice::default()
        }
    }

    #[must_use]
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = detail.into();
        self
    }

    #[must_use]
    pub fn section(mut self, section: impl Into<String>) -> Self {
        self.section = section.into();
        self
    }

    /// What the user reads of it.
    pub fn shown(&self) -> &str {
        if self.title.is_empty() {
            &self.value
        } else {
            &self.title
        }
    }
}

impl Field {
    pub fn new(key: impl Into<String>, title: impl Into<String>, kind: Kind) -> Self {
        Field {
            key: key.into(),
            title: title.into(),
            kind,
            ..Field::default()
        }
    }

    pub fn text(key: impl Into<String>, title: impl Into<String>) -> Self {
        Field::new(key, title, Kind::Text)
    }

    #[must_use]
    pub fn help(mut self, help: impl Into<String>) -> Self {
        self.help = help.into();
        self
    }

    #[must_use]
    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.value = value.into();
        self
    }

    #[must_use]
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    #[must_use]
    pub fn advanced(mut self) -> Self {
        self.advanced = true;
        self
    }

    /// Whether what it holds is a secret.
    pub fn secret(&self) -> bool {
        matches!(self.kind, Kind::Secret { .. })
    }

    /// Why `text`, typed in it, is no answer; `None` when it is one.
    pub fn refusal(&self, text: &str) -> Option<String> {
        if text.is_empty() {
            let kept = matches!(self.kind, Kind::Secret { kept: true });
            return (self.required && !kept).then(|| format!("{} is needed", self.title));
        }
        match &self.kind {
            Kind::Integer { min, max } => match text.trim().parse::<i64>() {
                Ok(n) if (*min..=*max).contains(&n) => None,
                _ => Some(format!(
                    "{} is a whole number from {min} to {max}",
                    self.title
                )),
            },
            Kind::Choice {
                choices,
                open: false,
            }
            | Kind::Pick { choices } => (!choices.iter().any(|c| c.value == text))
                .then(|| format!("{} is one of its choices", self.title)),
            _ => None,
        }
    }
}

impl Form {
    pub fn new(title: impl Into<String>, accept: impl Into<String>) -> Self {
        Form {
            title: title.into(),
            accept: accept.into(),
            ..Form::default()
        }
    }

    #[must_use]
    pub fn intro(mut self, intro: impl Into<String>) -> Self {
        self.intro = intro.into();
        self
    }

    #[must_use]
    pub fn field(mut self, field: Field) -> Self {
        self.fields.push(field);
        self
    }

    #[must_use]
    pub fn with_back(mut self) -> Self {
        self.back = true;
        self
    }

    #[must_use]
    pub fn error(mut self, error: Option<String>) -> Self {
        self.error = error;
        self
    }

    /// The form as it waits for `what` (see [`Form::waiting`]).
    #[must_use]
    pub fn waiting(mut self, what: impl Into<String>) -> Self {
        self.waiting = Some(what.into());
        self
    }

    /// Why `values` are no answer to it, the first field's that is not;
    /// `None` when they are one.
    pub fn refusal(&self, values: &Values) -> Option<String> {
        self.fields
            .iter()
            .find_map(|field| field.refusal(values.get(&field.key)))
    }

    /// The form again with what `values` hold in its fields, but its
    /// secrets (never written back where the user can read them), and
    /// `error` said: asked again after an answer was refused.
    #[must_use]
    pub fn again(mut self, values: &Values, error: impl Into<String>) -> Self {
        for field in &mut self.fields {
            if !field.secret()
                && let Some(value) = values.0.get(&field.key)
            {
                field.value.clone_from(value);
            }
        }
        self.error = Some(error.into());
        self
    }
}

/// What a form's fields hold as answered, by their keys: a switch's
/// `true` or `false`, a secret as typed (empty: none typed). No `Debug`:
/// it may hold a secret.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Values(pub BTreeMap<String, String>);

impl Values {
    /// What the field `key` holds, empty where it holds nothing.
    pub fn get(&self, key: &str) -> &str {
        self.0.get(key).map_or("", String::as_str)
    }

    /// Whether the switch `key` is on.
    pub fn on(&self, key: &str) -> bool {
        self.get(key) == "true"
    }

    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.0.insert(key.into(), value.into());
    }
}

/// How the user answered a form: its answer's button, or Back; Cancel
/// is no answer (`None`).
#[derive(Clone, PartialEq, Eq)]
pub enum Answer {
    Accepted(Values),
    Back(Values),
}

/// A form asked, until the window takes it (`App::take_forms`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asked {
    pub id: u64,
    pub form: Form,
}

/// The forms waiting for the window, and who waits for each answer.
#[derive(Default)]
pub(crate) struct Forms {
    asked: parking_lot::Mutex<Vec<Asked>>,
    replies: parking_lot::Mutex<HashMap<u64, crossbeam_channel::Sender<Option<Answer>>>>,
}

/// How often a filler looks whether its wait was cancelled.
const LOOK: Duration = Duration::from_millis(100);

impl App {
    /// Ask the window to have `form` filled: the receiver hears the
    /// answer, `None` for Cancel; it ends unanswered if the window
    /// closes, and whoever stops waiting `withdraw_form`s it.
    pub fn ask_form(&self, form: Form) -> (u64, crossbeam_channel::Receiver<Option<Answer>>) {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let (reply, answer) = crossbeam_channel::bounded(1);
        self.forms.replies.lock().insert(id, reply);
        self.forms.asked.lock().push(Asked { id, form });
        self.wake();
        (id, answer)
    }

    /// Have `form` filled and wait for the answer, on a worker: `None` for
    /// Cancel, with no window to ask, or once `cancel` fires (the form is
    /// then withdrawn).
    pub fn fill(&self, form: Form, cancel: &CancelToken) -> Option<Answer> {
        if !self.window_shown() {
            return None;
        }
        let (id, answer) = self.ask_form(form);
        loop {
            match answer.recv_timeout(LOOK) {
                Ok(answer) => return answer,
                Err(crossbeam_channel::RecvTimeoutError::Timeout) if cancel.is_cancelled() => {
                    self.withdraw_form(id);
                    return None;
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return None,
            }
        }
    }

    /// The forms asked since the last call, oldest first.
    pub fn take_forms(&self) -> Vec<Asked> {
        std::mem::take(&mut *self.forms.asked.lock())
    }

    /// The user answered the form `id` (`None`: Cancel): whether its
    /// asker still waited.
    pub fn answer_form(&self, id: u64, answer: Option<Answer>) -> bool {
        let reply = self.forms.replies.lock().remove(&id);
        reply.is_some_and(|reply| reply.send(answer).is_ok())
    }

    /// Whether the asker of `id` still waits for its answer.
    pub fn form_waits(&self, id: u64) -> bool {
        self.forms.replies.lock().contains_key(&id)
    }

    /// The asker of `id` stopped waiting, or is done with a form that
    /// waited on it: an answer goes nowhere, and one the window has not
    /// taken yet is not shown.
    pub fn withdraw_form(&self, id: u64) {
        self.forms.replies.lock().remove(&id);
        self.forms.asked.lock().retain(|asked| asked.id != id);
        // The window closes one it shows already.
        self.wake();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::AppConfig;

    fn values(pairs: &[(&str, &str)]) -> Values {
        Values(
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
        )
    }

    #[test]
    fn a_form_refuses_what_its_fields_cannot_hold() {
        let form = Form::new("Add Server", "Add")
            .field(Field::text("name", "Name").required())
            .field(Field::new("port", "Port", Kind::Integer { min: 1, max: 65535 }))
            .field(Field::new("key", "Key", Kind::Secret { kept: true }).required())
            .field(Field::new(
                "kind",
                "Kind",
                Kind::Choice {
                    choices: vec![Choice::new("s3", "Amazon S3")],
                    open: false,
                },
            ));
        assert_eq!(
            form.refusal(&values(&[])).as_deref(),
            Some("Name is needed")
        );
        assert_eq!(
            form.refusal(&values(&[("name", "it's mine"), ("port", "70000")]))
                .as_deref(),
            Some("Port is a whole number from 1 to 65535")
        );
        // A secret kept stays when nothing is typed.
        assert_eq!(form.refusal(&values(&[("name", "n"), ("port", "22")])), None);
        assert_eq!(
            form.refusal(&values(&[("name", "n"), ("kind", "b2")]))
                .as_deref(),
            Some("Kind is one of its choices")
        );
    }

    /// Asked again after a refusal, a form keeps what was typed but its
    /// secrets, and says why.
    #[test]
    fn a_form_asked_again_keeps_what_was_typed_but_secrets() {
        let form = Form::new("Add", "Add")
            .field(Field::text("name", "Name"))
            .field(Field::new("pass", "Password", Kind::Secret { kept: false }));
        let again = form.again(
            &values(&[("name", "nas"), ("pass", "hunter2")]),
            "A remote is named so",
        );
        assert_eq!(again.fields[0].value, "nas");
        assert_eq!(again.fields[1].value, "");
        assert_eq!(again.error.as_deref(), Some("A remote is named so"));
    }

    #[test]
    fn the_window_answers_a_form_to_its_asker_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        // No window: nobody can fill it.
        assert!(
            app.fill(Form::new("t", "OK"), &CancelToken::new())
                .is_none()
        );
        app.set_window_shown();
        let (id, answer) = app.ask_form(Form::new("Add", "Add"));
        let asked = app.take_forms();
        assert_eq!(asked.len(), 1);
        assert_eq!(asked[0].id, id);
        assert!(app.form_waits(id));
        assert!(app.answer_form(id, Some(Answer::Accepted(values(&[("a", "b")])))));
        assert!(!app.form_waits(id));
        match answer.recv().unwrap() {
            Some(Answer::Accepted(values)) => assert_eq!(values.get("a"), "b"),
            _ => panic!("accepted"),
        }
        // Answered once: a second answer goes nowhere.
        assert!(!app.answer_form(id, None));

        // Withdrawn before the window took it: never shown.
        let (id, _answer) = app.ask_form(Form::new("Gone", "OK"));
        app.withdraw_form(id);
        assert!(app.take_forms().is_empty());
        assert!(!app.form_waits(id));
    }

    /// A filler cancelled stops waiting and withdraws its form.
    #[test]
    fn a_cancelled_filler_withdraws_its_form() {
        let tmp = tempfile::tempdir().unwrap();
        let app = std::sync::Arc::new(App::new(AppConfig::ephemeral(tmp.path())).unwrap());
        app.set_window_shown();
        let cancel = CancelToken::new();
        let filler = {
            let (app, cancel) = (std::sync::Arc::clone(&app), cancel.clone());
            std::thread::spawn(move || app.fill(Form::new("Wait", "OK"), &cancel).is_none())
        };
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let id = loop {
            if let Some(asked) = app.take_forms().pop() {
                break asked.id;
            }
            assert!(std::time::Instant::now() < deadline, "never asked");
            std::thread::sleep(Duration::from_millis(10));
        };
        cancel.cancel();
        assert!(filler.join().unwrap());
        assert!(!app.form_waits(id));
    }
}
