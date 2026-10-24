//! The forms that add, edit, sign in again and remove rclone's remotes
//! (ADR-086), made from what rclone describes of its kinds of storage
//! (`rclone_config::Backend`): the window draws them (`App::fill`), this
//! turns their answers into rclone's configuration, through its remote
//! control (`rclone_config::Session`). Each runs on a thread of its own,
//! waiting on the user; none keeps anything of Skwir's.
//!
//! Adding: what kind of storage (a long list, filtered as typed), its
//! provider where it has several (S3's), then its name and options, the
//! advanced folded. rclone's own questions follow, one form each (a
//! browser's sign-in waits in its own, with the page to open again). The
//! remote added is listed once: where rclone cannot, the user keeps it,
//! changes it, or drops it.
//!
//! A secret is typed in the window, never given by an agent or a script
//! (`Prefill` refuses one), and never shown again: its field says that
//! one is kept, and holds a new one only if typed.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Map, Value};
use skwir_core::app::App;
use skwir_core::cancel::CancelToken;
use skwir_core::form::{Answer, Choice, Field, Form, Kind, Values};

use crate::rclone_config::{self, Backend, Opt, Session, Step};

/// The field naming the remote.
const NAME: &str = "remote.name";
/// The field of a pick (a kind, a provider).
const PICKED: &str = "picked";
/// The field answering one of rclone's questions.
const ANSWER: &str = "answer";
/// The prefix of a union's or a combine's switch for each remote.
const UPSTREAM: &str = "upstream:";

/// The kinds of storage made of other remotes, which pick among them.
const ON_REMOTES: &[&str] = &[
    "alias", "crypt", "union", "combine", "chunker", "compress", "hasher",
];

/// The kinds never offered: a test's storage in memory, rclone's own
/// cache (deprecated by rclone), the Storj alias.
const NOT_OFFERED: &[&str] = &["memory", "cache", "tardigrade"];

/// What a caller (an agent, a script, the palette) gives a new remote's
/// form to start from: never a secret, which the user types.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Prefill {
    pub kind: Option<String>,
    pub provider: Option<String>,
    pub name: Option<String>,
    pub options: BTreeMap<String, String>,
}

impl Prefill {
    /// Why it cannot fill `backend`'s form: an option it does not have,
    /// or a secret, which only the user types in the window.
    pub fn refusal(&self, backend: &Backend) -> Option<String> {
        for key in self.options.keys() {
            match backend.options.iter().find(|o| o.name == *key) {
                None => return Some(format!("{} has no option “{key}”", backend.title)),
                Some(opt) if opt.secret() => {
                    return Some(format!(
                        "“{key}” is a secret: the user types it in the window"
                    ));
                }
                Some(_) => {}
            }
        }
        None
    }
}

/// What a form flow works with.
pub struct Flow<'a> {
    pub app: &'a App,
    pub session: &'a Arc<Session>,
    pub cancel: &'a CancelToken,
}

/// The first line of an option's help as its title where it is short,
/// else its name in words; the rest of its first paragraphs as its help.
fn titled(opt: &Opt) -> (String, String) {
    let help = opt.help.trim();
    let (first, rest) = help.split_once('\n').unwrap_or((help, ""));
    let first = first.trim().trim_end_matches('.');
    let rest: Vec<&str> = rest
        .split("\n\n")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .take(2)
        .collect();
    let rest = rest.join(" ").replace('\n', " ");
    if !first.is_empty() && first.chars().count() <= 60 && !first.ends_with('?') {
        (first.to_owned(), rest)
    } else {
        let mut name = opt.name.replace('_', " ");
        if let Some(c) = name.get(..1) {
            name = c.to_uppercase() + &name[1..];
        }
        let help = if rest.is_empty() {
            first.to_owned()
        } else {
            format!("{first}. {rest}")
        };
        (name, help)
    }
}

/// The choices an option suggests for `provider`.
fn examples(opt: &Opt, provider: &str) -> Vec<Choice> {
    opt.examples
        .iter()
        .filter(|e| rclone_config::applies(&e.provider, provider))
        .map(|e| {
            let help = e.help.lines().next().unwrap_or_default().to_owned();
            if help.is_empty() || help == e.value {
                Choice::new(e.value.clone(), e.value.clone())
            } else {
                Choice::new(e.value.clone(), e.value.clone()).detail(help)
            }
        })
        .collect()
}

/// The field of `opt`, holding `value` (its default where none), a secret
/// `kept` where the configuration holds one; `remotes` the others, for a
/// remote built on one.
fn field_of(
    opt: &Opt,
    provider: &str,
    value: Option<&str>,
    kept: bool,
    remotes: &[String],
) -> Field {
    let (title, help) = titled(opt);
    let suggested = examples(opt, provider);
    let kind = if opt.secret() {
        Kind::Secret { kept }
    } else if opt.kind == "bool" {
        Kind::Toggle
    } else if opt.name == "remote" && !remotes.is_empty() {
        // What a remote built on another stands on: one of them, and a
        // folder of it.
        Kind::Choice {
            choices: remotes
                .iter()
                .map(|r| Choice::new(format!("{r}:"), format!("{r}:")))
                .collect(),
            open: true,
        }
    } else if !suggested.is_empty() {
        Kind::Choice {
            choices: suggested,
            open: !opt.exclusive,
        }
    } else {
        Kind::Text
    };
    let value = if opt.secret() {
        String::new()
    } else {
        value.unwrap_or(&opt.default).to_owned()
    };
    // What an option that is advanced holds unchanged stays folded; one
    // set otherwise (by the user, by a caller) is shown.
    let advanced = opt.advanced && value == opt.default;
    let mut field = Field::new(opt.name.clone(), title, kind)
        .help(help)
        .value(value);
    field.required = opt.required;
    field.advanced = advanced;
    field
}

/// The options of `backend` a form shows for `provider`: those not hidden,
/// that apply to it, the provider itself chosen before.
fn shown_options<'b>(backend: &'b Backend, provider: &str) -> Vec<&'b Opt> {
    backend
        .options
        .iter()
        .filter(|o| !o.hidden && o.name != "provider" && o.applies_to(provider))
        // A browser's sign-in fills these.
        .filter(|o| o.name != "token")
        .collect()
}

/// Whether `backend` signs in through a browser (it keeps a token).
pub fn signs_in(backend: &Backend) -> bool {
    backend.options.iter().any(|o| o.name == "token")
}

/// The pick of what kind of storage to add.
fn kind_form(backends: &[Backend], chosen: Option<&str>) -> Form {
    let mut choices: Vec<Choice> = backends
        .iter()
        .filter(|b| !NOT_OFFERED.contains(&b.name.as_str()))
        .map(|b| {
            let section = if ON_REMOTES.contains(&b.name.as_str()) {
                "On other remotes"
            } else {
                "Storage"
            };
            let detail = if b.description == b.title {
                b.name.clone()
            } else {
                format!("{} ({})", b.description, b.name)
            };
            Choice::new(b.name.clone(), b.title.clone())
                .detail(detail)
                .section(section)
        })
        .collect();
    choices.sort_by(|a, b| {
        (a.section != "Storage")
            .cmp(&(b.section != "Storage"))
            .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
    });
    Form::new("Add Server", "Next")
        .intro("A remote of rclone's: kept in rclone's own configuration, where rclone in a terminal finds it too.")
        .field(
            Field::new(PICKED, "Kind of storage", Kind::Pick { choices })
                .value(chosen.unwrap_or_default())
                .required(),
        )
}

/// The pick of `backend`'s provider (S3's: AWS, `MinIO`, Cloudflare…).
fn provider_form(backend: &Backend, chosen: Option<&str>) -> Option<Form> {
    let opt = backend
        .options
        .iter()
        .find(|o| o.name == "provider" && !o.examples.is_empty())?;
    let choices = opt
        .examples
        .iter()
        .map(|e| {
            let title = e.help.lines().next().unwrap_or(&e.value).to_owned();
            Choice::new(e.value.clone(), title).detail(e.value.clone())
        })
        .collect();
    Some(
        Form::new(format!("Add {}", backend.title), "Next")
            .with_back()
            .field(
                Field::new(PICKED, "Provider", Kind::Pick { choices })
                    .value(chosen.unwrap_or_default())
                    .required(),
            ),
    )
}

/// What a remote's form starts from: its name, and its options' values.
struct Start<'s> {
    name: Option<&'s str>,
    values: &'s BTreeMap<String, String>,
    /// The secrets the configuration holds already (`Kind::Secret`'s
    /// `kept`).
    kept: &'s [String],
}

/// The form of a remote of `backend` for `provider`.
fn options_form(
    backend: &Backend,
    provider: &str,
    start: &Start<'_>,
    remotes: &[String],
    title: String,
    accept: &str,
    back: bool,
) -> Form {
    let mut form = Form::new(title, accept);
    if back {
        form = form.with_back();
    }
    if start.name.is_none() {
        form = form.field(
            Field::text(NAME, "Name")
                .help("What the sidebar and rclone call it: letters a–z, digits and _ - . +")
                .required(),
        );
    }
    for opt in shown_options(backend, provider) {
        if matches!(backend.name.as_str(), "union" | "combine") && opt.name == "upstreams" {
            // A switch for each remote, and the list itself, folded, for
            // folders of them.
            let listed = start.values.get("upstreams").map(String::as_str);
            let chosen = upstream_names(listed.unwrap_or_default(), &backend.name);
            for remote in remotes {
                let on = chosen.as_ref().is_some_and(|c| c.contains(remote));
                form = form.field(
                    Field::new(
                        format!("{UPSTREAM}{remote}"),
                        format!("Include “{remote}”"),
                        Kind::Toggle,
                    )
                    .value(on.to_string()),
                );
            }
            let mut field = field_of(
                opt,
                provider,
                listed.filter(|_| chosen.is_none()),
                false,
                remotes,
            );
            field.required = remotes.is_empty();
            field.advanced = !remotes.is_empty();
            form = form.field(field);
            continue;
        }
        let kept = start.kept.contains(&opt.name);
        let value = start.values.get(&opt.name).map(String::as_str);
        form = form.field(field_of(opt, provider, value, kept, remotes));
    }
    form
}

/// The remotes a union's or a combine's `upstreams` names, where it names
/// whole remotes alone (`a: b:`, `a=a: b=b:`); `None` where it names more
/// (a folder of one), kept as written.
fn upstream_names(upstreams: &str, kind: &str) -> Option<Vec<String>> {
    let mut names = Vec::new();
    for part in split_upstreams(upstreams) {
        let remote = if kind == "combine" {
            let (dir, remote) = part.split_once('=')?;
            let name = remote.strip_suffix(':')?;
            (dir == name).then_some(name)?
        } else {
            part.strip_suffix(':')?
        };
        if remote.contains(':') || remote.is_empty() {
            return None;
        }
        names.push(remote.to_owned());
    }
    Some(names)
}

/// rclone's list of words, a quoted word holding spaces.
fn split_upstreams(upstreams: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut part = String::new();
    let mut quoted = false;
    for c in upstreams.chars() {
        match c {
            '"' => quoted = !quoted,
            ' ' if !quoted => {
                if !part.is_empty() {
                    parts.push(std::mem::take(&mut part));
                }
            }
            c => part.push(c),
        }
    }
    if !part.is_empty() {
        parts.push(part);
    }
    parts
}

/// The `upstreams` a union or a combine stands on, from the switches
/// turned on, a name holding a space quoted.
fn upstreams_of(values: &Values, kind: &str) -> Option<String> {
    let chosen: Vec<String> = values
        .0
        .iter()
        .filter(|(k, v)| k.starts_with(UPSTREAM) && *v == "true")
        .map(|(k, _)| {
            let name = &k[UPSTREAM.len()..];
            let word = if kind == "combine" {
                format!("{name}={name}:")
            } else {
                format!("{name}:")
            };
            if word.contains(' ') {
                format!("\"{word}\"")
            } else {
                word
            }
        })
        .collect();
    (!chosen.is_empty()).then(|| chosen.join(" "))
}

/// The parameters to give rclone of what `values` hold: each option shown
/// whose value is not what rclone has already (`before`: the remote's, or
/// nothing for a new one, whose defaults it then takes), a secret only if
/// one was typed.
fn parameters(
    backend: &Backend,
    provider: &str,
    values: &Values,
    before: &BTreeMap<String, String>,
) -> Map<String, Value> {
    let mut parameters = Map::new();
    if backend.options.iter().any(|o| o.name == "provider")
        && !provider.is_empty()
        && before.get("provider").map(String::as_str) != Some(provider)
    {
        parameters.insert("provider".into(), Value::String(provider.to_owned()));
    }
    for opt in shown_options(backend, provider) {
        let mut value = values.get(&opt.name).to_owned();
        if opt.name == "upstreams"
            && value.is_empty()
            && let Some(chosen) = upstreams_of(values, &backend.name)
        {
            value = chosen;
        }
        if opt.secret() {
            if !value.is_empty() {
                parameters.insert(opt.name.clone(), Value::String(value));
            }
            continue;
        }
        let was = before
            .get(&opt.name)
            .map_or(opt.default.as_str(), String::as_str);
        if value != was {
            // Emptied, it goes back to rclone's default.
            let value = if value.is_empty() {
                opt.default.clone()
            } else {
                value
            };
            parameters.insert(opt.name.clone(), Value::String(value));
        }
    }
    parameters
}

/// The form of one of rclone's questions about remote `name`: what it
/// says, then the question itself (its last line, where it asks one) as
/// the field's label.
fn question_form(name: &str, option: &Opt, error: &str) -> Form {
    let help = option.help.trim();
    let (said, asked) = match help.rsplit_once('\n') {
        Some((said, last)) if last.trim().ends_with('?') => (said.trim(), last.trim()),
        _ if help.ends_with('?') && !help.contains('\n') => ("", help),
        _ => (help, "Answer"),
    };
    let mut field = field_of(option, "", None, false, &[]);
    field.key = ANSWER.into();
    asked.clone_into(&mut field.title);
    field.help = String::new();
    field.advanced = false;
    Form::new(format!("Set Up “{name}”"), "Continue")
        .intro(said.to_owned())
        .field(field)
        .error((!error.is_empty()).then(|| error.to_owned()))
}

/// What a flow does with rclone's questions about a remote's sign-in.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Token {
    /// Keep the one it has (an edit).
    Keep,
    /// Sign in again (Reconnect).
    Renew,
}

impl Flow<'_> {
    fn fill(&self, form: Form) -> Option<Answer> {
        self.app.fill(form, self.cancel)
    }

    /// The other remotes of the configuration, but `name`.
    fn others(&self, name: Option<&str>) -> anyhow::Result<Vec<String>> {
        let mut remotes = self.session.remotes(self.cancel)?;
        remotes.retain(|r| Some(r.as_str()) != name);
        remotes.sort_by_key(|r| r.to_lowercase());
        Ok(remotes)
    }

    /// Add a remote, from `prefill`: its name once added, `None` when the
    /// user gave up.
    ///
    /// # Errors
    /// When rclone did not answer.
    pub fn add(&self, prefill: &Prefill) -> anyhow::Result<Option<String>> {
        let backends = self.session.backends(self.cancel)?;
        let mut kind = prefill.kind.clone();
        let mut provider = prefill.provider.clone();
        let mut values = prefill.options.clone();
        let mut name = prefill.name.clone();
        // What the caller gave is checked once, against the kind it is for.
        let mut given = !prefill.options.is_empty();
        loop {
            let Some(chosen) = &kind else {
                match self.fill(kind_form(&backends, None)) {
                    Some(Answer::Accepted(answer)) => {
                        kind = Some(answer.get(PICKED).to_owned());
                        provider = None;
                        continue;
                    }
                    _ => return Ok(None),
                }
            };
            let backend = backends
                .iter()
                .find(|b| b.name == *chosen)
                .ok_or_else(|| anyhow::anyhow!("rclone has no kind of storage “{chosen}”"))?;
            if given {
                given = false;
                if let Some(refusal) = prefill.refusal(backend) {
                    anyhow::bail!("{refusal}");
                }
            }
            let mut asked_provider = false;
            if provider.is_none()
                && let Some(form) = provider_form(backend, None)
            {
                asked_provider = true;
                match self.fill(form) {
                    Some(Answer::Accepted(answer)) => {
                        provider = Some(answer.get(PICKED).to_owned());
                    }
                    Some(Answer::Back(_)) => {
                        kind = None;
                        continue;
                    }
                    None => return Ok(None),
                }
            }
            let shown = provider.clone().unwrap_or_default();
            match self.add_options(backend, &shown, &mut name, &mut values)? {
                Added::Done(name) => return Ok(Some(name)),
                Added::Cancelled => return Ok(None),
                Added::Back if asked_provider => provider = None,
                Added::Back => {
                    kind = None;
                    provider = None;
                }
            }
        }
    }

    /// The options form of a new remote of `backend`, until rclone made
    /// it, the user went back, or gave up.
    fn add_options(
        &self,
        backend: &Backend,
        provider: &str,
        name: &mut Option<String>,
        values: &mut BTreeMap<String, String>,
    ) -> anyhow::Result<Added> {
        let remotes = self.others(None)?;
        let mut error: Option<String> = None;
        loop {
            let mut start_values = values.clone();
            if let Some(name) = name.as_ref() {
                start_values.insert(NAME.into(), name.clone());
            }
            let mut form = options_form(
                backend,
                provider,
                &Start {
                    name: None,
                    values: &start_values,
                    kept: &[],
                },
                &remotes,
                format!("Add {}", backend.title),
                "Add",
                true,
            );
            if let Some(field) = form.fields.iter_mut().find(|f| f.key == NAME) {
                field.value = name.clone().unwrap_or_default();
            }
            let answer = match self.fill(form.error(error.take())) {
                Some(Answer::Accepted(answer)) => answer,
                Some(Answer::Back(answer)) => {
                    keep_typed(values, name, &answer);
                    return Ok(Added::Back);
                }
                None => return Ok(Added::Cancelled),
            };
            keep_typed(values, name, &answer);
            let chosen = answer.get(NAME).trim().to_owned();
            if let Some(refusal) = rclone_config::name_refusal(&chosen) {
                error = Some(refusal.to_owned());
                continue;
            }
            if remotes.contains(&chosen) {
                error = Some(format!("rclone has a remote named “{chosen}” already"));
                continue;
            }
            let parameters = parameters(backend, provider, &answer, &BTreeMap::new());
            let step = match self
                .session
                .create(&chosen, &backend.name, &parameters, self.cancel)
            {
                Ok(step) => step,
                Err(refused) => {
                    error = Some(said(&refused));
                    continue;
                }
            };
            match self.questions(&chosen, step, Token::Renew)? {
                Asked::Done => {}
                Asked::Cancelled => {
                    // Half made: not kept.
                    let _ = self.session.delete(&chosen, self.cancel);
                    return Ok(Added::Cancelled);
                }
            }
            match self.checked(&chosen, backend, provider, true)? {
                Checked::Kept => return Ok(Added::Done(chosen)),
                Checked::Dropped => return Ok(Added::Cancelled),
            }
        }
    }

    /// Answer rclone's questions about remote `name` from `step` on, until
    /// it has none: each a form, a browser's sign-in waiting in its own.
    fn questions(&self, name: &str, mut step: Step, token: Token) -> anyhow::Result<Asked> {
        loop {
            let Step::Question {
                state,
                option,
                error,
            } = step
            else {
                return Ok(Asked::Done);
            };
            // Asked where the window is: its browser is here.
            let given = match option.name.as_str() {
                "config_is_local" => Some("true".to_owned()),
                "config_refresh_token" => Some((token == Token::Renew).to_string()),
                _ => None,
            };
            let result = match given {
                Some(result) if error.is_empty() => result,
                _ => match self.fill(question_form(name, &option, &error)) {
                    Some(Answer::Accepted(answer)) => answer.get(ANSWER).to_owned(),
                    _ => return Ok(Asked::Cancelled),
                },
            };
            step = match self.answer_waiting(name, &state, &result)? {
                Some(next) => next,
                None => return Ok(Asked::Cancelled),
            };
        }
    }

    /// Answer `result` at `state`: an answer rclone takes long over (it
    /// waits for the browser's sign-in) shows a form that waits with it,
    /// the sign-in's page to open again; its Cancel stops the sign-in.
    /// `None` when the user gave up.
    fn answer_waiting(
        &self,
        name: &str,
        state: &str,
        result: &str,
    ) -> anyhow::Result<Option<Step>> {
        let (done, answered) = crossbeam_channel::bounded(1);
        {
            let (session, cancel) = (Arc::clone(self.session), self.cancel.clone());
            let (name, state, result) = (name.to_owned(), state.to_owned(), result.to_owned());
            std::thread::Builder::new()
                .name("skwir-rclone-answer".into())
                .spawn(move || {
                    let _ = done.send(session.answer(&name, &state, &result, &cancel));
                })?;
        }
        // Most answers come at once: no form flashes.
        if let Ok(step) = answered.recv_timeout(Duration::from_millis(800)) {
            return step.map(Some).map_err(|e| anyhow::anyhow!("{}", said(&e)));
        }
        let mut shown: Option<(u64, crossbeam_channel::Receiver<Option<Answer>>)> = None;
        let mut page: Option<String> = None;
        loop {
            // The page, once rclone serves it.
            if page.is_none()
                && let Some(found) = self.session.sign_in_page(self.cancel)
            {
                page = Some(found);
                if let Some((id, _)) = shown.take() {
                    self.app.withdraw_form(id);
                }
            }
            if shown.is_none() {
                shown = Some(self.app.ask_form(waiting_form(name, page.as_deref())));
            }
            let Some((id, gave_up)) = &shown else {
                continue;
            };
            crossbeam_channel::select! {
                recv(answered) -> step => {
                    self.app.withdraw_form(*id);
                    let step = step.map_err(|_| anyhow::anyhow!("rclone stopped"))?;
                    return step.map(Some).map_err(|e| anyhow::anyhow!("{}", said(&e)));
                }
                recv(gave_up) -> _ => {
                    self.session.stop_sign_in(self.cancel);
                    let _ = answered.recv_timeout(Duration::from_secs(5));
                    return Ok(None);
                }
                default(Duration::from_millis(300)) => {
                    if self.cancel.is_cancelled() {
                        self.app.withdraw_form(*id);
                        self.session.stop_sign_in(self.cancel);
                        return Ok(None);
                    }
                }
            }
        }
    }

    /// List the top of remote `name` once it is made or changed: where
    /// rclone cannot, the user keeps it, changes it (`backend`'s form
    /// again), or drops it (`new`: removed; else left as changed).
    fn checked(
        &self,
        name: &str,
        backend: &Backend,
        provider: &str,
        new: bool,
    ) -> anyhow::Result<Checked> {
        loop {
            let fs = format!("{name}:");
            let listed = self.waited(
                &format!("Trying “{name}”"),
                "rclone lists its top, as connecting would.",
                move |session, cancel| session.list_top(&fs, cancel),
            );
            let failed = match listed {
                Some(Err(failed)) if !self.cancel.is_cancelled() => failed,
                // Answered, or not waited for.
                _ => return Ok(Checked::Kept),
            };
            let drop = if new { " Cancel removes it." } else { "" };
            let form = Form::new(format!("“{name}” Does Not Answer"), "Keep It")
                .intro(format!(
                    "rclone could not list it: {}. Keep it as it is, or go back and change it.{drop}",
                    said(&failed)
                ))
                .with_back();
            match self.fill(form) {
                Some(Answer::Back(_)) => {
                    if !self.change(name, backend, provider, Token::Keep)? {
                        return Ok(Checked::Kept);
                    }
                }
                None if new => {
                    self.session.delete(name, self.cancel)?;
                    return Ok(Checked::Dropped);
                }
                // Kept, or (an edit) left as changed.
                Some(Answer::Accepted(_)) | None => return Ok(Checked::Kept),
            }
        }
    }

    /// `work` on a thread of its own, a form saying `title` and `what`
    /// once it takes long: `None` when the user stopped waiting (its
    /// Cancel), `work` then cancelled.
    fn waited<T: Send + 'static>(
        &self,
        title: &str,
        what: &str,
        work: impl FnOnce(&Session, &CancelToken) -> T + Send + 'static,
    ) -> Option<T> {
        let (done, finished) = crossbeam_channel::bounded(1);
        let cancel = self.cancel.child();
        {
            let (session, cancel) = (Arc::clone(self.session), cancel.clone());
            std::thread::Builder::new()
                .name("skwir-rclone-wait".into())
                .spawn(move || {
                    let _ = done.send(work(&session, &cancel));
                })
                .ok()?;
        }
        // Most answers come at once: no form flashes.
        if let Ok(result) = finished.recv_timeout(Duration::from_millis(800)) {
            return Some(result);
        }
        let (id, gave_up) = self.app.ask_form(Form::new(title, "OK").waiting(what));
        crossbeam_channel::select! {
            recv(finished) -> result => {
                self.app.withdraw_form(id);
                result.ok()
            }
            recv(gave_up) -> _ => {
                cancel.cancel();
                None
            }
        }
    }

    /// What the configuration holds of remote `name`: its options' values,
    /// and which secrets it keeps.
    fn held(&self, name: &str) -> anyhow::Result<(String, BTreeMap<String, String>)> {
        let config = self.session.get(name, self.cancel)?;
        let kind = config
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("rclone's configuration names no remote “{name}”"))?
            .to_owned();
        let mut values = BTreeMap::new();
        for (key, value) in &config {
            if key == "type" {
                continue;
            }
            if let Some(text) = value.as_str() {
                values.insert(key.clone(), text.to_owned());
            }
        }
        Ok((kind, values))
    }

    /// Change remote `name`'s options in its form, until saved (true) or
    /// given up (false).
    fn change(
        &self,
        name: &str,
        backend: &Backend,
        provider: &str,
        token: Token,
    ) -> anyhow::Result<bool> {
        let (_, mut before) = self.held(name)?;
        let kept: Vec<String> = backend
            .options
            .iter()
            .filter(|o| o.secret() && before.get(&o.name).is_some_and(|v| !v.is_empty()))
            .map(|o| o.name.clone())
            .collect();
        // Never shown, even obscured.
        before.retain(|k, _| !kept.contains(k));
        let remotes = self.others(Some(name))?;
        let mut error = None;
        loop {
            let form = options_form(
                backend,
                provider,
                &Start {
                    name: Some(name),
                    values: &before,
                    kept: &kept,
                },
                &remotes,
                format!("Edit “{name}”"),
                "Save",
                false,
            )
            .error(error.take());
            let Some(Answer::Accepted(answer)) = self.fill(form) else {
                return Ok(false);
            };
            let parameters = parameters(backend, provider, &answer, &before);
            let step = match self.session.update(name, &parameters, false, self.cancel) {
                Ok(step) => step,
                Err(refused) => {
                    error = Some(said(&refused));
                    continue;
                }
            };
            return Ok(self.questions(name, step, token)? == Asked::Done);
        }
    }

    /// Edit remote `name` in its form: whether it was saved.
    ///
    /// # Errors
    /// When rclone did not answer, or has no such remote.
    pub fn edit(&self, name: &str) -> anyhow::Result<bool> {
        let (kind, values) = self.held(name)?;
        let backends = self.session.backends(self.cancel)?;
        let backend = backends
            .iter()
            .find(|b| b.name == kind)
            .ok_or_else(|| anyhow::anyhow!("rclone has no kind of storage “{kind}”"))?;
        let provider = values.get("provider").cloned().unwrap_or_default();
        if !self.change(name, backend, &provider, Token::Keep)? {
            return Ok(false);
        }
        self.checked(name, backend, &provider, false)?;
        Ok(true)
    }

    /// Sign remote `name` in again through the browser (its token expired
    /// or was revoked): whether it was.
    ///
    /// # Errors
    /// When rclone did not answer, or refused.
    pub fn reconnect(&self, name: &str) -> anyhow::Result<bool> {
        let step = self.session.update(name, &Map::new(), false, self.cancel)?;
        Ok(self.questions(name, step, Token::Renew)? == Asked::Done)
    }

    /// Ask whether to remove remote `name`: whether the user said so.
    pub fn confirm_remove(&self, name: &str, mounted: bool) -> bool {
        let mut intro = format!(
            "rclone's configuration forgets “{name}”: what it holds stays where it is, and adding it again brings it back."
        );
        if mounted {
            intro.push_str(" Skwir disconnects it first.");
        }
        let form = Form::new(format!("Remove “{name}”?"), "Remove").intro(intro);
        matches!(self.fill(form), Some(Answer::Accepted(_)))
    }
}

/// The waiting form of remote `name`'s sign-in, its `page` once known.
fn waiting_form(name: &str, page: Option<&str>) -> Form {
    let mut form = Form::new(format!("Sign In to “{name}”"), "OK").waiting(
        "Finish signing in in your browser: rclone waits for the answer. If no page opened, open it here.",
    );
    if let Some(page) = page {
        form = form.field(Field::new("page", "Open the Sign-In Page", Kind::Link).value(page));
    }
    form
}

/// What the user typed, kept for the form asked again (Back, a refusal):
/// never a secret.
fn keep_typed(values: &mut BTreeMap<String, String>, name: &mut Option<String>, answer: &Values) {
    for (key, value) in &answer.0 {
        if key == NAME {
            *name = Some(value.clone());
        } else if !key.starts_with(UPSTREAM) {
            values.insert(key.clone(), value.clone());
        }
    }
}

/// What rclone refused, in its words.
fn said(error: &anyhow::Error) -> String {
    let text = format!("{error:#}");
    let text = text
        .strip_prefix("rclone refused ")
        .map_or(text.as_str(), |rest| {
            rest.split_once(": ").map_or(rest, |(_, why)| why)
        });
    crate::rclone::words(text)
}

enum Added {
    Done(String),
    Back,
    Cancelled,
}

#[derive(PartialEq, Eq)]
enum Asked {
    Done,
    Cancelled,
}

enum Checked {
    Kept,
    Dropped,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opt(name: &str) -> Opt {
        Opt {
            name: name.into(),
            kind: "string".into(),
            ..Opt::default()
        }
    }

    #[test]
    fn an_option_s_help_titles_its_field() {
        let mut key = opt("access_key_id");
        key.help =
            "AWS Access Key ID.\n\nLeave blank for anonymous access or runtime credentials.".into();
        assert_eq!(
            titled(&key),
            (
                "AWS Access Key ID".to_owned(),
                "Leave blank for anonymous access or runtime credentials.".to_owned()
            )
        );
        let mut long = opt("env_auth");
        long.help = "Get AWS credentials from runtime (environment variables or EC2/ECS meta data if no env vars).\n\nOnly applies if access_key_id and secret_access_key is blank.".into();
        let (title, help) = titled(&long);
        assert_eq!(title, "Env auth");
        assert!(help.starts_with("Get AWS credentials"), "{help}");
    }

    /// A field of each kind rclone has: a secret masked and kept, a switch,
    /// a choice among a provider's examples, a remote to stand on.
    #[test]
    fn rclone_s_options_are_fields_of_their_kind() {
        let mut secret = opt("secret_access_key");
        secret.sensitive = true;
        secret.default = "never shown".into();
        let field = field_of(&secret, "AWS", Some("obscured"), true, &[]);
        assert_eq!(field.kind, Kind::Secret { kept: true });
        assert_eq!(field.value, "", "a secret is never written back");

        let mut switch = opt("env_auth");
        switch.kind = "bool".into();
        switch.default = "false".into();
        assert_eq!(field_of(&switch, "", None, false, &[]).kind, Kind::Toggle);
        assert_eq!(field_of(&switch, "", None, false, &[]).value, "false");

        let mut region = opt("region");
        region.examples = vec![
            crate::rclone_config::Example {
                value: "us-east-1".into(),
                help: "The default endpoint".into(),
                provider: "AWS".into(),
            },
            crate::rclone_config::Example {
                value: "fr-par".into(),
                help: "Paris".into(),
                provider: "Scaleway".into(),
            },
        ];
        let Kind::Choice { choices, open } = field_of(&region, "AWS", None, false, &[]).kind else {
            panic!("a choice");
        };
        assert!(open, "not exclusive: any region");
        assert_eq!(choices.len(), 1, "AWS's alone");
        assert_eq!(choices[0].value, "us-east-1");

        let remote = opt("remote");
        let Kind::Choice { choices, open } = field_of(
            &remote,
            "",
            None,
            false,
            &["photos".into(), "it's mine".into()],
        )
        .kind
        else {
            panic!("a choice of remotes");
        };
        assert!(open, "a folder of one may follow");
        assert_eq!(choices[1].value, "it's mine:");
    }

    /// Only what changed goes to rclone: a default left as it is stays
    /// rclone's, a secret only when typed.
    #[test]
    fn rclone_is_given_what_changed() {
        let mut chunk = opt("chunk_size");
        chunk.default = "5Mi".into();
        let mut secret = opt("secret_access_key");
        secret.sensitive = true;
        let mut provider = opt("provider");
        provider.examples = vec![crate::rclone_config::Example {
            value: "Minio".into(),
            ..Default::default()
        }];
        let backend = Backend {
            name: "s3".into(),
            title: "Amazon S3".into(),
            description: String::new(),
            options: vec![provider, opt("endpoint"), chunk, secret],
        };
        let mut values = Values::default();
        values.set("endpoint", "http://127.0.0.1:9000");
        values.set("chunk_size", "5Mi");
        values.set("secret_access_key", "");
        let new = parameters(&backend, "Minio", &values, &BTreeMap::new());
        assert_eq!(
            Value::Object(new),
            serde_json::json!({ "provider": "Minio", "endpoint": "http://127.0.0.1:9000" })
        );
        values.set("secret_access_key", "typed s3cret");
        let before: BTreeMap<String, String> = [
            ("provider".to_owned(), "Minio".to_owned()),
            ("endpoint".to_owned(), "http://127.0.0.1:9000".to_owned()),
        ]
        .into();
        let changed = parameters(&backend, "Minio", &values, &before);
        assert_eq!(
            Value::Object(changed),
            serde_json::json!({ "secret_access_key": "typed s3cret" })
        );
    }

    #[test]
    fn a_union_stands_on_the_remotes_switched_on() {
        let mut values = Values::default();
        values.set(format!("{UPSTREAM}photos"), "true");
        values.set(format!("{UPSTREAM}old disk"), "true");
        values.set(format!("{UPSTREAM}other"), "false");
        assert_eq!(
            upstreams_of(&values, "union").as_deref(),
            Some("\"old disk:\" photos:")
        );
        assert_eq!(
            upstreams_of(&values, "combine").as_deref(),
            Some("\"old disk=old disk:\" photos=photos:")
        );
        assert_eq!(
            upstream_names("\"old disk:\" photos:", "union"),
            Some(vec!["old disk".to_owned(), "photos".to_owned()])
        );
        assert_eq!(upstream_names("photos:2024 other:", "union"), None);
        assert_eq!(
            upstream_names("photos=photos:", "combine"),
            Some(vec!["photos".to_owned()])
        );
    }

    #[test]
    fn an_agent_never_gives_a_secret() {
        let mut secret = opt("secret_access_key");
        secret.sensitive = true;
        let backend = Backend {
            name: "s3".into(),
            title: "Amazon S3".into(),
            description: String::new(),
            options: vec![opt("endpoint"), secret],
        };
        let mut prefill = Prefill::default();
        prefill
            .options
            .insert("endpoint".into(), "https://example.com".into());
        assert_eq!(prefill.refusal(&backend), None);
        prefill
            .options
            .insert("secret_access_key".into(), "x".into());
        assert!(
            prefill
                .refusal(&backend)
                .unwrap()
                .contains("the user types it")
        );
        let mut odd = Prefill::default();
        odd.options.insert("nothing".into(), "x".into());
        assert!(odd.refusal(&backend).unwrap().contains("no option"));
    }
}
