//! A list of records in the Settings dialog (`SettingKind::Records`): tile
//! servers, remotes, the languages and their grammars. The list is drawn
//! inset in its setting's card, a list under the setting rather than more
//! settings, saying so when it is empty; a long one (`FOLD`) has a filter
//! and shows its first rows until unfolded, or until a record is added,
//! which shows where it went. Each record is a row with its
//! actions (the owner's own, such as Test; Edit; Remove; a choice of its
//! own); one at a time is edited in place, a draft kept apart from the
//! rows, which are rebuilt whenever a setting changes. A record kept
//! elsewhere whose detail says what to do (`wraps`) shows it whole.
//!
//! A secret field's value goes to the desktop's keyring and never to the
//! settings (`skwir_platform::secrets`): saving a draft that holds a new
//! one stores it on a worker first, and only then saves the list, which
//! says the keyring has it. Removing a record forgets its secrets. The
//! whole list is validated before anything is kept, so a record the owner
//! refuses leaves nothing in the keyring.
use crate::sizes::TextSizes;
use crate::sizes::sized;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{AnyElement, Context, Entity, Focusable, Subscription, Window, div, prelude::*, px};
use skwir_core::settings::{FieldKind, RecordField, SECRET_STORED, SettingKind};
use skwir_core::words;
use skwir_core::words::Words;

use crate::controls::{self, ButtonStyle};
use crate::input::{InputEvent, TextInput};
use crate::settings::SettingsView;
use crate::theme::Theme;

pub(crate) type Record = serde_json::Map<String, serde_json::Value>;

/// Rows a list shows until unfolded; a longer one has a filter.
pub(crate) const FOLD: usize = 8;

/// A record's own choice (`choice`: its `value`, `label`, `choices`, each
/// a `value`, `label`, `detail` and whether `disabled`, and the `command`
/// choosing runs, with `args` and the value in its `field`), as Left and
/// Right step through it: the values that may be chosen, in order, and
/// the one held, as its owner's answer listed at `listed` (its revision)
/// says.
#[derive(Clone)]
pub(crate) struct Pick {
    pub(crate) focus: gpui::FocusHandle,
    run: Run,
    values: Vec<String>,
    held: Option<usize>,
    /// Its setting and its record, what `SettingsView::picks_asked` knows
    /// it by.
    asked_as: (String, String),
    listed: u64,
}

/// The value `delta` from `held` among values up to `last`, staying at
/// the ends: from none held, the first going right, the last going left.
fn stepped(held: Option<usize>, delta: i64, last: usize) -> usize {
    match held {
        Some(held) => {
            let held = i64::try_from(held).unwrap_or(0);
            usize::try_from((held + delta).max(0))
                .unwrap_or(0)
                .min(last)
        }
        None if delta < 0 => last,
        None => 0,
    }
}

/// What choosing a record's value runs.
#[derive(Clone)]
struct Run {
    command: String,
    args: serde_json::Value,
    field: String,
}

impl Run {
    fn of(choice: &serde_json::Map<String, serde_json::Value>) -> Option<Run> {
        Some(Run {
            command: choice.get("command")?.as_str()?.to_owned(),
            args: choice
                .get("args")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({})),
            field: choice
                .get("field")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("value")
                .to_owned(),
        })
    }

    /// Run it with `value`; what refuses it is said. Whether it was
    /// asked: the owner answers then (`ChoiceSource`).
    fn with(&self, app: &skwir_core::App, value: &str) -> bool {
        let mut args = self.args.clone();
        if !args.is_object() {
            args = serde_json::json!({});
        }
        args[self.field.as_str()] = serde_json::json!(value);
        let invocation = skwir_core::command::Invocation::with_args(self.command.as_str(), &args);
        match app.dispatch(&invocation) {
            Ok(_) => true,
            Err(error) => {
                app.notify(skwir_core::app::NotifyLevel::Error, Words::of_error(error));
                false
            }
        }
    }
}

impl Pick {
    /// Choose the value `delta` from the one held, staying at the ends;
    /// with none of the values held (not chosen), Right the first, Left
    /// the last. Asked of its owner and not answered yet (its owner's
    /// revision as when asked), it asks nothing more: a key held down
    /// would ask again for what its answer, not listed yet, holds already.
    pub(crate) fn step(&self, delta: i64, view: &mut SettingsView) {
        let Some(last) = self.values.len().checked_sub(1) else {
            return;
        };
        if view.picks_asked.get(&self.asked_as) == Some(&self.listed) {
            return;
        }
        let at = stepped(self.held, delta, last);
        if Some(at) != self.held {
            self.ask(&self.values[at], view);
        }
    }

    /// Run the choice of `value`, and wait for its owner's answer.
    fn ask(&self, value: &str, view: &mut SettingsView) {
        if self.run.with(&view.app, value) {
            view.picks_asked.insert(self.asked_as.clone(), self.listed);
        }
    }
}

/// The label of a record's Edit.
fn edit_words() -> Words {
    words!(
        records_edit = "Edit",
        note = "A button of a record in a list of the settings (a tile server): change it."
    )
}

/// The label of a record's Remove.
fn remove_words() -> Words {
    words!(
        records_remove = "Remove",
        note = "A button of a record in a list of the settings (a tile server): take it out."
    )
}

/// A row of a list inset in its setting's card: a line between rows.
fn inset_row(theme: &Theme, first: bool) -> gpui::Div {
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap_3()
        .px_3()
        .py_2()
        .when(!first, |d| d.border_t_1().border_color(theme.border))
}

/// The records a setting's editor text holds.
pub(crate) fn records(text: &str) -> Vec<Record> {
    serde_json::from_str(text).unwrap_or_default()
}

/// What an owner keeping a list elsewhere says of it beside its records
/// (`RecordSource`, ADR-150, 6): its groups, in order, and a note under
/// it, said for the window.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Shape {
    pub groups: Vec<Group>,
    pub note: Option<String>,
}

/// A group of a list: the records whose `group` is its `id`, headed by its
/// title and its switch.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Group {
    pub id: String,
    pub title: String,
    pub switch: Option<Toggle>,
}

/// A group's switch or a record's tick: whether it is on, and what turns
/// it the other way (its command, run with its `args` and `on`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Toggle {
    pub on: bool,
    pub command: String,
    pub args: serde_json::Value,
}

impl Toggle {
    pub(crate) fn of(value: Option<&serde_json::Value>) -> Option<Toggle> {
        let toggle = value?.as_object()?;
        Some(Toggle {
            on: toggle.get("on")?.as_bool()?,
            command: toggle.get("command")?.as_str()?.to_owned(),
            args: toggle
                .get("args")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({})),
        })
    }

    /// What turns it the other way.
    pub(crate) fn turned(&self) -> skwir_core::command::Invocation {
        let mut args = self.args.clone();
        match args.as_object_mut() {
            Some(args) => {
                args.insert("on".into(), serde_json::json!(!self.on));
            }
            None => args = serde_json::json!({ "on": !self.on }),
        }
        skwir_core::command::Invocation::with_args(self.command.as_str(), &args)
    }
}

impl Shape {
    /// What an owner's answer, said, says beside its records.
    pub(crate) fn of(answer: &serde_json::Value) -> Shape {
        let text = |value: Option<&serde_json::Value>| value?.as_str().map(str::to_owned);
        Shape {
            groups: answer
                .get("groups")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|group| {
                    Some(Group {
                        id: text(group.get("id"))?,
                        title: text(group.get("title")).unwrap_or_default(),
                        switch: Toggle::of(group.get("switch")),
                    })
                })
                .collect(),
            note: text(answer.get("note")).filter(|note| !note.is_empty()),
        }
    }
}

/// A line of a list as it is drawn: a group's heading, or a record (none
/// for the one being added), dimmed while its group's switch is off.
enum Line<'a> {
    Heading(&'a Group, usize),
    Record(usize, Option<&'a Record>, bool),
}

/// The lines of `shown`, under their groups' headings in `shape`'s order
/// (a group with nothing shown has none), what no group holds last; the
/// records alone where the owner groups none.
fn lines<'a>(shape: &'a Shape, shown: &[(usize, Option<&'a Record>)]) -> Vec<Line<'a>> {
    if shape.groups.is_empty() {
        return shown
            .iter()
            .map(|(index, record)| Line::Record(*index, *record, false))
            .collect();
    }
    let group_of = |record: &Record| {
        record
            .get("group")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    };
    let mut held: std::collections::HashMap<String, Vec<(usize, Option<&'a Record>)>> =
        std::collections::HashMap::new();
    let mut loose = Vec::new();
    let known: std::collections::HashSet<&str> =
        shape.groups.iter().map(|g| g.id.as_str()).collect();
    for (index, record) in shown {
        match record.and_then(group_of) {
            Some(group) if known.contains(group.as_str()) => {
                held.entry(group).or_default().push((*index, *record));
            }
            _ => loose.push((*index, *record)),
        }
    }
    let mut lines = Vec::new();
    for group in &shape.groups {
        let Some(records) = held.remove(&group.id) else {
            continue;
        };
        let off = group.switch.as_ref().is_some_and(|s| !s.on);
        lines.push(Line::Heading(group, records.len()));
        lines.extend(
            records
                .into_iter()
                .map(|(index, record)| Line::Record(index, record, off)),
        );
    }
    lines.extend(
        loose
            .into_iter()
            .map(|(index, record)| Line::Record(index, record, false)),
    );
    lines
}

/// What an owner keeping a setting elsewhere answers (`RecordSource`,
/// `ChoiceSource`), its words said for the window: each words' data in it
/// (`Words::to_data`, which an agent reads in English) said in the
/// window's language, text as written left as it is. What a choice or an
/// action runs with (`args`) is the owner's, sent back as it came.
pub(crate) fn said_answer(mut answer: serde_json::Value) -> serde_json::Value {
    fn say(value: &mut serde_json::Value) {
        // A record or a choice is no words' data, and reads as none.
        if value.is_object()
            && let Ok(words) = <Words as serde::Deserialize>::deserialize(&*value)
        {
            *value = serde_json::Value::String(words.say());
            return;
        }
        match value {
            serde_json::Value::Object(fields) => fields
                .iter_mut()
                .filter(|(name, _)| *name != "args")
                .for_each(|(_, field)| say(field)),
            serde_json::Value::Array(items) => items.iter_mut().for_each(say),
            _ => {}
        }
    }
    say(&mut answer);
    answer
}

impl SettingsView {
    /// The records of setting `key` whose editor text is `value`, parsed
    /// once for each text: a list is drawn every frame, and may hold
    /// hundreds (the Grammars' languages).
    pub(crate) fn parsed_records(&self, key: &str, value: &str) -> Rc<Vec<Record>> {
        let mut parsed = self.parsed_records.borrow_mut();
        if let Some((text, records)) = parsed.get(key)
            && text == value
        {
            return Rc::clone(records);
        }
        let list = Rc::new(records(value));
        parsed.insert(key.to_owned(), (value.to_owned(), Rc::clone(&list)));
        list
    }
}

fn text_of(record: &Record, key: &str) -> String {
    match record.get(key) {
        Some(serde_json::Value::String(text)) => text.clone(),
        Some(serde_json::Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

pub(crate) struct Pair {
    name: Entity<TextInput>,
    value: Entity<TextInput>,
}

pub(crate) enum FieldInput {
    Text(Entity<TextInput>),
    Pairs(Vec<Pair>),
    Secret {
        input: Entity<TextInput>,
        /// The keyring has a value, and it is to be kept unless replaced.
        stored: bool,
    },
}

/// The record being edited.
pub(crate) struct Draft {
    id: String,
    /// Not in the list yet: Cancel drops it.
    new: bool,
    fields: Vec<(RecordField, FieldInput)>,
    /// Why it was not kept, said where it is drawn.
    error: Option<Words>,
    /// Waiting for the keyring.
    busy: bool,
    subscriptions: Vec<Subscription>,
}

/// What a keyring job came back with: the list to save, or why not.
pub(crate) struct KeyringDone {
    key: String,
    result: Result<String, Words>,
}

/// A record being dragged to another place in its list.
#[derive(Clone)]
pub(crate) struct RecordDrag {
    key: String,
    /// Its place in the list when the drag started.
    index: usize,
}

/// `list` with the item at `from` moved into the gap before the item at
/// `gap` (`list.len()` for the end); `None` when that is where it is.
pub(crate) fn moved<T: Clone>(list: &[T], from: usize, gap: usize) -> Option<Vec<T>> {
    if from >= list.len() || gap > list.len() || gap == from || gap == from + 1 {
        return None;
    }
    let mut out = list.to_vec();
    let item = out.remove(from);
    out.insert(if gap > from { gap - 1 } else { gap }, item);
    Some(out)
}

/// A fresh record id: unique among a list's, and saying nothing.
pub(crate) fn new_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos()),
    );
    format!("{:012x}", hasher.finish() & 0xffff_ffff_ffff)
}

impl SettingsView {
    fn draft_input(
        &mut self,
        key: &str,
        text: String,
        placeholder: Words,
        masked: bool,
        subscriptions: &mut Vec<Subscription>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<TextInput> {
        let input = cx.new(|cx| {
            let mut input = TextInput::new(placeholder, cx);
            input.set_masked(masked);
            input.set_text(text, cx);
            input
        });
        let key = key.to_owned();
        subscriptions.push(cx.subscribe_in(
            &input,
            window,
            move |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::Changed => {
                    if let Some(draft) = this.drafts.get_mut(&key) {
                        draft.error = None;
                    }
                    cx.notify();
                }
                InputEvent::Submit => this.save_record(&key, cx),
                InputEvent::Cancel => this.cancel_record(&key, window, cx),
            },
        ));
        input
    }

    /// Edit the record `id` of `key`, or a new one.
    pub(crate) fn edit_record(
        &mut self,
        key: &str,
        id: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(row) = self.row(key) else {
            return;
        };
        let fields = row.spec.constraints.fields.clone();
        let record = id
            .and_then(|id| {
                records(&row.value)
                    .into_iter()
                    .find(|r| r.get("id").and_then(|v| v.as_str()) == Some(id))
            })
            .unwrap_or_default();
        let mut subscriptions = Vec::new();
        let mut inputs = Vec::with_capacity(fields.len());
        for field in fields {
            let input = match &field.kind {
                FieldKind::Text { placeholder } => FieldInput::Text(self.draft_input(
                    key,
                    text_of(&record, &field.key),
                    placeholder.clone(),
                    false,
                    &mut subscriptions,
                    window,
                    cx,
                )),
                FieldKind::Integer { .. } => FieldInput::Text(self.draft_input(
                    key,
                    text_of(&record, &field.key),
                    Words::default(),
                    false,
                    &mut subscriptions,
                    window,
                    cx,
                )),
                FieldKind::Pairs => {
                    let pairs: Vec<(String, String)> = record
                        .get(&field.key)
                        .and_then(|v| v.as_array())
                        .into_iter()
                        .flatten()
                        .filter_map(|pair| {
                            let pair = pair.as_array()?;
                            Some((
                                pair.first()?.as_str()?.to_owned(),
                                pair.get(1)?.as_str()?.to_owned(),
                            ))
                        })
                        .collect();
                    let mut list = Vec::with_capacity(pairs.len());
                    for (name, value) in pairs {
                        list.push(self.pair(key, name, value, &mut subscriptions, window, cx));
                    }
                    FieldInput::Pairs(list)
                }
                FieldKind::Secret => {
                    let stored = text_of(&record, &field.key) == SECRET_STORED;
                    FieldInput::Secret {
                        input: self.draft_input(
                            key,
                            String::new(),
                            if stored {
                                words!(
                                    records_secret_kept = "Kept in the keyring: type to replace",
                                    note = "The placeholder of a secret's field (a password) whose value the desktop's keyring keeps."
                                )
                            } else {
                                Words::default()
                            },
                            true,
                            &mut subscriptions,
                            window,
                            cx,
                        ),
                        stored,
                    }
                }
            };
            inputs.push((field, input));
        }
        // The first field takes the keyboard.
        if let Some((_, FieldInput::Text(input))) = inputs.first() {
            window.focus(&input.focus_handle(cx), cx);
        }
        self.drafts.insert(
            key.to_owned(),
            Draft {
                id: id.map_or_else(new_id, str::to_owned),
                new: id.is_none(),
                fields: inputs,
                error: None,
                busy: false,
                subscriptions,
            },
        );
        cx.notify();
    }

    fn pair(
        &mut self,
        key: &str,
        name: String,
        value: String,
        subscriptions: &mut Vec<Subscription>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Pair {
        let (name_said, value_said) = (
            words!(
                records_pair_name = "Name",
                note = "The placeholder of the first field of a name and value pair (an HTTP header's name)."
            ),
            words!(
                records_pair_value = "Value",
                note = "The placeholder of the second field of a name and value pair (an HTTP header's value)."
            ),
        );
        Pair {
            name: self.draft_input(key, name, name_said, false, subscriptions, window, cx),
            value: self.draft_input(key, value, value_said, false, subscriptions, window, cx),
        }
    }

    /// Another empty pair at the end of field `index`.
    pub(crate) fn add_pair(
        &mut self,
        key: &str,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut subscriptions = Vec::new();
        let pair = self.pair(
            key,
            String::new(),
            String::new(),
            &mut subscriptions,
            window,
            cx,
        );
        let focus = pair.name.focus_handle(cx);
        let Some(draft) = self.drafts.get_mut(key) else {
            return;
        };
        if let Some((_, FieldInput::Pairs(pairs))) = draft.fields.get_mut(index) {
            pairs.push(pair);
            draft.subscriptions.extend(subscriptions);
            window.focus(&focus, cx);
        }
        cx.notify();
    }

    pub(crate) fn remove_pair(
        &mut self,
        key: &str,
        index: usize,
        pair: usize,
        cx: &mut Context<Self>,
    ) {
        if let Some(Draft { fields, .. }) = self.drafts.get_mut(key)
            && let Some((_, FieldInput::Pairs(pairs))) = fields.get_mut(index)
            && pair < pairs.len()
        {
            pairs.remove(pair);
        }
        cx.notify();
    }

    /// Forget the secret of field `index`: saving the draft deletes it.
    pub(crate) fn forget_secret(&mut self, key: &str, index: usize, cx: &mut Context<Self>) {
        if let Some(Draft { fields, .. }) = self.drafts.get_mut(key)
            && let Some((_, FieldInput::Secret { stored, input })) = fields.get_mut(index)
        {
            *stored = false;
            let unset = words!(
                records_secret_not_set = "Not set",
                note =
                    "The placeholder of a secret's field (a password) the keyring no longer keeps."
            );
            input.update(cx, |input, cx| input.set_placeholder(unset, cx));
        }
        cx.notify();
    }

    pub(crate) fn cancel_record(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.drafts.get(key).is_some_and(|d| !d.busy) {
            self.drafts.remove(key);
            window.focus(&self.root, cx);
            cx.notify();
        }
    }

    /// Save the draft of `key`: its secret to the keyring first, when it
    /// has a new one, then the list.
    pub(crate) fn save_record(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(row) = self.row(key) else {
            return;
        };
        let (spec, mut list) = (row.spec.clone(), records(&row.value));
        let Some(draft) = self.drafts.get(key).filter(|d| !d.busy) else {
            return;
        };
        let mut record = Record::new();
        record.insert("id".into(), draft.id.clone().into());
        // (field, new secret) for the keyring; a stored one forgotten.
        let mut secrets: Vec<(String, String)> = Vec::new();
        let mut forgotten: Vec<String> = Vec::new();
        for (field, input) in &draft.fields {
            let value = match input {
                FieldInput::Text(input) => {
                    serde_json::Value::String(input.read(cx).text().trim().to_owned())
                }
                FieldInput::Pairs(pairs) => serde_json::Value::Array(
                    pairs
                        .iter()
                        .map(|pair| {
                            serde_json::json!([
                                pair.name.read(cx).text().trim(),
                                pair.value.read(cx).text().trim()
                            ])
                        })
                        .collect(),
                ),
                FieldInput::Secret { input, stored } => {
                    let typed = input.read(cx).text().to_owned();
                    if !typed.is_empty() {
                        secrets.push((field.key.clone(), typed));
                        SECRET_STORED.into()
                    } else if *stored {
                        SECRET_STORED.into()
                    } else {
                        forgotten.push(field.key.clone());
                        serde_json::Value::String(String::new())
                    }
                }
            };
            record.insert(field.key.clone(), value);
        }
        let name = record
            .get("name")
            .and_then(|v| v.as_str())
            .filter(|n| !n.is_empty())
            .unwrap_or(&draft.id)
            .to_owned();
        match list
            .iter_mut()
            .find(|r| r.get("id").and_then(|v| v.as_str()) == Some(draft.id.as_str()))
        {
            Some(existing) => *existing = record,
            None => list.push(record),
        }
        let text = serde_json::to_string(&list).unwrap_or_default();
        // Refused as a whole before anything reaches the keyring.
        let stored = self.app.settings.get::<toml::Value>(key);
        if let Err(error) = spec.parse_over(&text, stored.as_ref()) {
            if let Some(draft) = self.drafts.get_mut(key) {
                draft.error = Some(Words::of_error(error));
            }
            cx.notify();
            return;
        }
        // A record just added is shown where it went, at the end of a list
        // it may have made long enough to fold.
        if self.drafts.get(key).is_some_and(|draft| draft.new) {
            self.records_unfolded.insert(key.to_owned());
        }
        if secrets.is_empty() && forgotten.is_empty() {
            self.drafts.remove(key);
            self.change(key, text, cx);
            return;
        }
        let id = self
            .drafts
            .get(key)
            .map(|d| d.id.clone())
            .unwrap_or_default();
        if let Some(draft) = self.drafts.get_mut(key) {
            draft.busy = true;
        }
        let (slot, weak, owner) = (Arc::clone(&self.keyring), self.app.weak(), key.to_owned());
        // What the keyring's own window shows the user: said as Skwir's.
        let label = words!(
            records_keyring_label = "skwir: { $record } ({ $setting })",
            record = name,
            setting = spec.title.clone(),
            note = "What the desktop's keyring calls a secret skwir keeps there: record is the record's name or id, as the user wrote it; setting is the setting it belongs to (Tile servers)."
        )
        .say();
        self.app.scheduler.spawn(
            skwir_core::scheduler::Priority::Visible,
            "settings-keyring",
            self.app.lifetime().child(),
            move || {
                let result = (|| {
                    use skwir_platform::secrets::{SecretRef, delete, store};
                    for (field, secret) in &secrets {
                        let at = SecretRef {
                            setting: &owner,
                            record: &id,
                            field,
                        };
                        store(at, &label, secret)?;
                    }
                    for field in &forgotten {
                        delete(SecretRef {
                            setting: &owner,
                            record: &id,
                            field,
                        })?;
                    }
                    anyhow::Ok(())
                })()
                .map(|()| text)
                .map_err(|error| {
                    words!(
                        records_keyring_failed = "The keyring did not keep it: { $error }",
                        error = error,
                        note = "Why a record holding a secret (a password) was not saved: the desktop's keyring refused it."
                    )
                });
                if let Ok(mut done) = slot.lock() {
                    done.push(KeyringDone { key: owner, result });
                }
                if let Some(app) = weak.upgrade() {
                    app.wake();
                }
            },
        );
        cx.notify();
    }

    /// Remove record `id` of `key`, and its secrets from the keyring.
    pub(crate) fn remove_record(&mut self, key: &str, id: &str, cx: &mut Context<Self>) {
        let Some(row) = self.row(key) else {
            return;
        };
        let mut list = records(&row.value);
        let secret_fields: Vec<String> = row
            .spec
            .constraints
            .fields
            .iter()
            .filter(|f| f.kind == FieldKind::Secret)
            .map(|f| f.key.clone())
            .collect();
        let had_secret = list
            .iter()
            .filter(|r| r.get("id").and_then(|v| v.as_str()) == Some(id))
            .any(|r| secret_fields.iter().any(|f| text_of(r, f) == SECRET_STORED));
        list.retain(|r| r.get("id").and_then(|v| v.as_str()) != Some(id));
        if self.drafts.get(key).is_some_and(|d| d.id == id) {
            self.drafts.remove(key);
        }
        self.change(key, serde_json::to_string(&list).unwrap_or_default(), cx);
        if had_secret {
            let (owner, id, weak) = (key.to_owned(), id.to_owned(), self.app.weak());
            self.app.scheduler.spawn(
                skwir_core::scheduler::Priority::Normal,
                "settings-keyring",
                self.app.lifetime().child(),
                move || {
                    if let Err(error) = skwir_platform::secrets::delete_record(&owner, &id)
                        && let Some(app) = weak.upgrade()
                    {
                        app.notify(
                            skwir_core::app::NotifyLevel::Error,
                            words!(
                                records_keyring_kept = "The keyring kept a secret of the record removed: { $error }",
                                error = error,
                                note = "A record holding a secret (a password) was removed, and the desktop's keyring did not forget the secret."
                            ),
                        );
                    }
                },
            );
        }
    }

    /// Move record `from` of `key` into the gap before record `gap`.
    pub(crate) fn move_record(
        &mut self,
        key: &str,
        from: usize,
        gap: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(row) = self.row(key) else {
            return;
        };
        // What its owner keeps elsewhere keeps the owner's order.
        if row.spec.constraints.record_source.is_some() {
            return;
        }
        if let Some(list) = moved(&records(&row.value), from, gap) {
            self.change(key, serde_json::to_string(&list).unwrap_or_default(), cx);
        }
    }

    /// `ui.settings_move { delta }`: the record whose button has the
    /// keyboard, one place up (-1) or down (1).
    pub(crate) fn move_focused_record(
        &mut self,
        delta: i64,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let Some((key, id)) = self
            .record_stops
            .iter()
            .find(|(focus, _, _)| focus.contains_focused(window, cx))
            .map(|(_, key, id)| (key.clone(), id.clone()))
        else {
            return;
        };
        let Some(index) = self.row(&key).and_then(|row| {
            records(&row.value)
                .iter()
                .position(|r| r.get("id").and_then(|v| v.as_str()) == Some(id.as_str()))
        }) else {
            return;
        };
        let gap = if delta < 0 {
            index.checked_sub(1)
        } else {
            Some(index + 2)
        };
        if let Some(gap) = gap {
            self.move_record(&key, index, gap, cx);
        }
    }

    /// Take what keyring jobs came back with: save the list, or say why not.
    pub(crate) fn drain_keyring(&mut self, cx: &mut Context<Self>) -> bool {
        let done: Vec<KeyringDone> = self
            .keyring
            .lock()
            .map(|mut done| std::mem::take(&mut *done))
            .unwrap_or_default();
        let any = !done.is_empty();
        for KeyringDone { key, result } in done {
            match result {
                Ok(text) => {
                    self.drafts.remove(&key);
                    self.change(&key, text, cx);
                }
                Err(error) => {
                    if let Some(draft) = self.drafts.get_mut(&key) {
                        draft.busy = false;
                        draft.error = Some(error);
                    }
                }
            }
        }
        any
    }

    /// The list under a records setting's own row, inset in its card:
    /// each record, or the form of the one edited; what an empty list or
    /// a filter finds nothing says; a long one's filter and fold. `last`
    /// rounds the card's bottom.
    pub(crate) fn record_rows(
        &mut self,
        key: &str,
        value: &str,
        last: bool,
        item: usize,
        items: &mut Vec<AnyElement>,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let Some(spec) = self.row(key).map(|row| row.spec.clone()) else {
            return;
        };
        debug_assert_eq!(spec.kind, SettingKind::Records);
        // The languages shown: their combined grammars are checked.
        if key == crate::settings::LANGUAGES_KEY {
            crate::settings::check_grammars(&self.app);
        }
        let editing = self.drafts.get(key).map(|d| (d.id.clone(), d.new));
        let all = self.parsed_records(key, value);
        // A long list: its filter, and its first rows until unfolded.
        let long = all.len() > FOLD;
        let filter = long.then(|| self.record_filter(key, cx));
        let query = filter
            .as_ref()
            .map(|input| input.read(cx).text().trim().to_owned())
            .unwrap_or_default();
        // Found as the dialog finds everything (`collate::Words`).
        let words = skwir_core::collate::Words::new(&query);
        let text_fields: Vec<String> = spec
            .constraints
            .fields
            .iter()
            .filter(|f| matches!(f.kind, FieldKind::Text { .. }))
            .map(|f| f.key.clone())
            .collect();
        let matches = |record: &Record| {
            words.is_empty() || {
                let texts: Vec<String> = text_fields
                    .iter()
                    .map(|field| text_of(record, field))
                    .collect();
                words.found_in(&texts.iter().map(String::as_str).collect::<Vec<_>>())
            }
        };
        let mut shown: Vec<(usize, Option<&Record>)> = all
            .iter()
            .enumerate()
            .filter(|(_, record)| matches(record))
            .map(|(index, record)| (index, Some(record)))
            .collect();
        let found = shown.len();
        let unfolded = self.records_unfolded.contains(key);
        let folded = long && query.is_empty() && !unfolded;
        if folded {
            shown.truncate(FOLD);
        }
        if editing.as_ref().is_some_and(|(_, new)| *new) {
            shown.push((found, None));
        }
        let mut rows = div()
            .flex()
            .flex_col()
            .rounded_md()
            .border_1()
            .border_color(theme.border)
            .bg(theme.bg);
        // Under their owner's groups, where it says some.
        let shape = self.sourced_shapes.get(key).cloned().unwrap_or_default();
        let lines = lines(&shape, &shown);
        for (n, line) in lines.iter().enumerate() {
            let edge = crate::settings_view::Edge {
                first: n == 0,
                last: n + 1 == lines.len(),
            };
            let (index, record, dimmed) = match line {
                Line::Heading(group, count) => {
                    rows =
                        rows.child(self.group_heading(key, group, *count, edge, item, theme, cx));
                    continue;
                }
                Line::Record(index, record, dimmed) => (index, record, *dimmed),
            };
            let id = record
                .as_ref()
                .and_then(|r| r.get("id").and_then(|v| v.as_str()).map(str::to_owned));
            let edited = match (&editing, &id) {
                (Some((edited, false)), Some(id)) => edited == id,
                (Some((_, true)), None) => true,
                _ => false,
            };
            let element = if edited {
                self.record_form(key, edge, item, theme, window, cx)
            } else if let (Some(record), Some(id)) = (record, id) {
                let place = (&id[..], *index, all.len());
                self.record_row(key, &spec, record, place, edge, item, theme, cx)
            } else {
                continue;
            };
            // A group switched off: its records said, dimmed.
            rows = rows.child(if dimmed {
                div().opacity(0.5).child(element).into_any_element()
            } else {
                element
            });
        }
        if shown.is_empty() {
            let said = if !query.is_empty() {
                words!(
                    records_none_match = "Nothing matches { $filter }",
                    filter = skwir_core::words::Name(query.clone()),
                    note = "A list of records in the settings, filtered: filter is what the user typed."
                )
            } else if spec
                .constraints
                .record_source
                .as_ref()
                .is_some_and(|source| source.add.is_empty())
            {
                words!(
                    records_none_found = "None found",
                    note = "A list of records in the settings that skwir fills (the languages found) is empty."
                )
            } else {
                words!(
                    records_none_yet = "None yet",
                    note = "A list of records in the settings that the user fills (tile servers) is empty."
                )
            }
            .say();
            rows = rows.child(
                inset_row(theme, true)
                    .text_body()
                    .text_color(theme.text_muted)
                    .child(said),
            );
        }
        let mut block = crate::settings_view::card_row(
            theme,
            crate::settings_view::Edge { first: false, last },
        )
        .flex_col()
        .items_start()
        .gap_2()
        .pt_2();
        if let Some(input) = &filter {
            let focused = input.read(cx).is_focused(window);
            self.stop(&input.focus_handle(cx), Some(item), None);
            block = block.child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(div().flex_1().child(controls::field(
                        input,
                        None,
                        focused,
                        false,
                        Some("search"),
                        theme,
                    )))
                    .child(
                        div()
                            .flex_none()
                            .text_small()
                            .text_color(theme.text_muted)
                            .child(
                                if query.is_empty() {
                                    words!(
                                        records_count = "{ $count }",
                                        count = found,
                                        note = "How many records a long list in the settings holds, beside its filter."
                                    )
                                } else {
                                    words!(
                                        records_found_of = "{ $found } of { $count }",
                                        found = found,
                                        count = all.len(),
                                        note = "How many records a long list's filter in the settings kept (found), of all it holds (count)."
                                    )
                                }
                                .say(),
                            ),
                    ),
            );
        }
        block = block.child(div().w_full().child(rows));
        // What the owner says under its list (what it costs).
        if let Some(note) = shape.note {
            block = block.child(
                div()
                    .id(gpui::SharedString::from(format!("records-note-{key}")))
                    // A label a reader reaches, as it reads the screen.
                    .role(gpui::Role::Label)
                    .aria_label(note.clone())
                    .w_full()
                    .text_small()
                    .text_color(theme.text_muted)
                    .child(note),
            );
        }
        if long && query.is_empty() {
            let focus = self.handle(&format!("fold:{key}"), cx);
            self.stop(&focus, Some(item), None);
            let toggled = key.to_owned();
            let label = if unfolded {
                words!(
                    records_show_fewer = "Show fewer",
                    note = "Folds a long list of records in the settings to its first rows."
                )
            } else {
                words!(
                    records_show_all = "Show all { $count }",
                    count = found,
                    note = "Unfolds a long list of records in the settings: count is how many it holds."
                )
            }
            .say();
            block = block.child(
                controls::button(
                    format!("records-fold-{key}"),
                    label,
                    Some(if unfolded { "fold" } else { "unfold" }),
                    ButtonStyle::Plain,
                    &focus,
                    theme,
                )
                .aria_expanded(unfolded)
                .on_click(cx.listener(move |this, _, _, cx| {
                    if !this.records_unfolded.remove(&toggled) {
                        this.records_unfolded.insert(toggled.clone());
                    }
                    cx.notify();
                })),
            );
        }
        items.push(block.into_any_element());
    }

    /// The filter of the list `key`, made the first time it is long.
    fn record_filter(&mut self, key: &str, cx: &mut Context<Self>) -> Entity<TextInput> {
        if let Some(input) = self.record_filters.get(key) {
            return input.clone();
        }
        let input = cx.new(|cx| {
            let mut input = TextInput::new(
                words!(
                    records_filter = "Filter",
                    note = "The placeholder of a long list's filter in the settings."
                ),
                cx,
            );
            input.set_label(words!(
                records_filter_label = "Filter the list",
                note = "What a screen reader calls a long list's filter in the settings."
            ));
            input
        });
        let sub = cx.subscribe(&input, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Changed) {
                cx.notify();
            }
        });
        self.keep_subscription(sub);
        self.record_filters.insert(key.to_owned(), input.clone());
        input
    }

    #[allow(clippy::too_many_arguments)] // a row of the list and its place in it
    fn record_row(
        &mut self,
        key: &str,
        spec: &skwir_core::settings::SettingSpec,
        record: &Record,
        (id, index, count): (&str, usize, usize),
        edge: crate::settings_view::Edge,
        item: usize,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let text_fields: Vec<&RecordField> = spec
            .constraints
            .fields
            .iter()
            .filter(|f| matches!(f.kind, FieldKind::Text { .. }))
            .collect();
        let title = text_fields
            .first()
            .map(|f| text_of(record, &f.key))
            .unwrap_or_default();
        let detail = text_fields
            .get(1)
            .map(|f| text_of(record, &f.key))
            .unwrap_or_default();
        let mut actions = div().flex().flex_none().items_center().gap(sized(6.));
        if let Some(action) = spec.constraints.record_action.clone() {
            let focus = self.handle(&format!("action:{key}:{id}"), cx);
            self.stop(&focus, Some(item), None);
            self.record_stops
                .push((focus.clone(), key.to_owned(), id.to_owned()));
            let args = serde_json::json!({ "id": id });
            actions = actions.child(
                controls::button(
                    format!("action-{key}-{id}"),
                    action.label.say(),
                    None,
                    ButtonStyle::Plain,
                    &focus,
                    theme,
                )
                .on_click(cx.listener(move |this, _, _, _| {
                    let invocation =
                        skwir_core::command::Invocation::with_args(action.command.as_str(), &args);
                    if let Err(error) = this.app.dispatch(&invocation) {
                        this.app
                            .notify(skwir_core::app::NotifyLevel::Error, Words::of_error(error));
                    }
                })),
            );
        }
        // Kept by their owner elsewhere: its commands change them.
        if let Some(source) = spec.constraints.record_source.clone() {
            return self.sourced_row(
                key,
                &source,
                record,
                (title, detail),
                (id, edge, item),
                actions,
                theme,
                cx,
            );
        }
        let busy = self.drafts.contains_key(key);
        let (edit_focus, remove_focus) = (
            self.handle(&format!("edit:{key}:{id}"), cx),
            self.handle(&format!("remove:{key}:{id}"), cx),
        );
        self.stop(&edit_focus, Some(item), None);
        self.stop(&remove_focus, Some(item), None);
        for focus in [&edit_focus, &remove_focus] {
            self.record_stops
                .push((focus.clone(), key.to_owned(), id.to_owned()));
        }
        let (edit_key, edit_id, remove_key, remove_id) =
            (key.to_owned(), id.to_owned(), key.to_owned(), id.to_owned());
        actions = actions
            .child(
                controls::button(
                    format!("edit-{key}-{id}"),
                    edit_words().say(),
                    None,
                    ButtonStyle::Plain,
                    &edit_focus,
                    theme,
                )
                .when(busy, |b| b.opacity(0.5))
                .on_click(cx.listener(move |this, _, window, cx| {
                    if !this.drafts.contains_key(&edit_key) {
                        this.edit_record(&edit_key, Some(&edit_id), window, cx);
                    }
                })),
            )
            .child(
                controls::button(
                    format!("remove-{key}-{id}"),
                    remove_words().say(),
                    None,
                    ButtonStyle::Danger,
                    &remove_focus,
                    theme,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.remove_record(&remove_key, &remove_id, cx);
                })),
            );
        let drag = RecordDrag {
            key: key.to_owned(),
            index,
        };
        let label = title.clone();
        let handle = div()
            .id(gpui::SharedString::from(format!("record-grip-{key}-{id}")))
            .flex_none()
            .cursor_grab()
            .child(controls::glyph("grip", theme.text_muted, 16.))
            .on_drag(drag, move |_, position, _, cx| {
                cx.new(|_| crate::dnd::DragPreview::named(label.clone(), position))
            });
        inset_row(theme, edge.first)
            .relative()
            .key_context("SettingRecord")
            .child(handle)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(sized(2.))
                    .child(crate::elements::clip::clipped(
                        div()
                            .id(gpui::SharedString::from(format!("record-title-{key}-{id}")))
                            .text_body()
                            .truncate(),
                        title,
                    ))
                    // A line of its own only when it says something.
                    .when(!detail.is_empty(), |d| {
                        d.child(crate::elements::clip::clipped(
                            div()
                                .id(gpui::SharedString::from(format!(
                                    "record-detail-{key}-{id}"
                                )))
                                .text_small()
                                .text_color(theme.text_muted)
                                .truncate(),
                            detail,
                        ))
                    }),
            )
            .child(actions)
            // The gaps above and below, where a dragged record lands.
            .child(self.record_gap(key, index, count, true, theme, cx))
            .child(self.record_gap(key, index + 1, count, false, theme, cx))
            .into_any_element()
    }

    /// A group's heading in a list its owner keeps elsewhere (ADR-150, 6),
    /// as the Exclusions page heads its categories: its title, how many it
    /// holds, and its switch, which runs the owner's command.
    #[allow(clippy::too_many_arguments)] // a heading of the list and its place in it
    fn group_heading(
        &mut self,
        key: &str,
        group: &Group,
        count: usize,
        edge: crate::settings_view::Edge,
        item: usize,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut heading = inset_row(theme, edge.first)
            .id(gpui::SharedString::from(format!("records-group-heading-{key}-{}", group.id)))
            .role(gpui::Role::Heading)
            // Named by its title, as a reader moves from heading to heading.
            .aria_label(group.title.clone())
            .child(
                div()
                    .flex_none()
                    .max_w(gpui::relative(0.6))
                    .truncate()
                    .text_body()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(group.title.clone()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_small()
                    .text_color(theme.text_muted)
                    .child(
                        words!(
                            records_group_count = "{ $count }",
                            count = count,
                            note = "How many records a group of a list in the settings holds, beside its title."
                        )
                        .say(),
                    ),
            );
        if let Some(switch) = &group.switch {
            let focus = self.handle(&format!("group:{key}:{}", group.id), cx);
            self.stop(&focus, Some(item), None);
            let turned = switch.turned();
            heading = heading.child(
                controls::switch(
                    format!("records-group-{key}-{}", group.id),
                    switch.on,
                    &focus,
                    theme,
                )
                .aria_label(group.title.clone())
                .on_click(cx.listener(move |this, _, _, _| {
                    if let Err(error) = this.app.dispatch(&turned) {
                        this.app
                            .notify(skwir_core::app::NotifyLevel::Error, Words::of_error(error));
                    }
                })),
            );
        }
        heading.into_any_element()
    }

    /// A row of records their owner keeps elsewhere (`RecordSource`): its
    /// Edit and Remove run the owner's commands, which ask what they need
    /// themselves; it is not moved. The record may say what else its line
    /// does (`actions`: each a `label`, a `command` and its `args`), drawn
    /// under its words, and that another program changes it (`fixed`: no
    /// Edit, no Remove).
    #[allow(clippy::too_many_arguments)] // a row of the list and its place in it
    fn sourced_row(
        &mut self,
        key: &str,
        source: &skwir_core::settings::RecordSource,
        record: &Record,
        (title, detail): (String, String),
        (id, edge, item): (&str, crate::settings_view::Edge, usize),
        mut actions: gpui::Div,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let fixed = record.get("fixed").and_then(serde_json::Value::as_bool) == Some(true);
        let wraps = record.get("wraps").and_then(serde_json::Value::as_bool) == Some(true);
        // Each by a name of its own, for its focus and its element, and its
        // words.
        let verbs = [
            (
                "Edit",
                edit_words(),
                source.edit.clone(),
                ButtonStyle::Plain,
            ),
            (
                "Remove",
                remove_words(),
                source.remove.clone(),
                ButtonStyle::Danger,
            ),
        ];
        // A command left empty is no button (Kept live's own folders
        // are removed, never edited).
        for (verb, label, command, style) in verbs
            .into_iter()
            .filter(|(_, _, command, _)| !fixed && !command.is_empty())
        {
            let focus = self.handle(&format!("{verb}:{key}:{id}"), cx);
            self.stop(&focus, Some(item), None);
            self.record_stops
                .push((focus.clone(), key.to_owned(), id.to_owned()));
            let args = serde_json::json!({ "id": id });
            actions = actions.child(
                controls::button(
                    format!("{verb}-{key}-{id}"),
                    label.say(),
                    None,
                    style,
                    &focus,
                    theme,
                )
                .on_click(cx.listener(move |this, _, _, _| {
                    let invocation =
                        skwir_core::command::Invocation::with_args(command.as_str(), &args);
                    if let Err(error) = this.app.dispatch(&invocation) {
                        this.app
                            .notify(skwir_core::app::NotifyLevel::Error, Words::of_error(error));
                    }
                })),
            );
        }
        // A choice of its own (a language's grammar), last of its actions.
        if let Some(choice) = record.get("choice").and_then(serde_json::Value::as_object) {
            actions = actions.child(self.record_choice(key, id, choice, item, None, theme, cx));
        }
        // What else the line does, as the owner says, after Edit and
        // Remove for the keys: they stand above.
        let mut own = div().flex().flex_wrap().gap(sized(6.)).pt(sized(4.));
        let listed = record
            .get("actions")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten();
        let mut owns = false;
        for (n, action) in listed.enumerate() {
            let text = |field: &str| action.get(field).and_then(serde_json::Value::as_str);
            let (Some(label), Some(command)) = (text("label"), text("command")) else {
                continue;
            };
            let invocation = skwir_core::command::Invocation::with_args(
                command,
                &action
                    .get("args")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({})),
            );
            let focus = self.handle(&format!("own:{n}:{key}:{id}"), cx);
            self.stop(&focus, Some(item), None);
            self.record_stops
                .push((focus.clone(), key.to_owned(), id.to_owned()));
            own = own.child(
                controls::button(
                    format!("own-{n}-{key}-{id}"),
                    label.to_owned(),
                    None,
                    ButtonStyle::Plain,
                    &focus,
                    theme,
                )
                .on_click(cx.listener(move |this, _, _, _| {
                    if let Err(error) = this.app.dispatch(&invocation) {
                        this.app
                            .notify(skwir_core::app::NotifyLevel::Error, Words::of_error(error));
                    }
                })),
            );
            owns = true;
        }
        // A box before its words, as the owner says it (Kept live's
        // folders): pressing it runs the owner's command.
        let tick = Toggle::of(record.get("tick")).map(|tick| {
            let focus = self.handle(&format!("tick:{key}:{id}"), cx);
            self.stop(&focus, Some(item), None);
            self.record_stops
                .push((focus.clone(), key.to_owned(), id.to_owned()));
            let turned = tick.turned();
            let accent = theme.accent;
            div()
                .id(gpui::SharedString::from(format!("record-tick-{key}-{id}")))
                .role(gpui::Role::CheckBox)
                .aria_label(title.clone())
                // What its line says under its name, heard with it.
                .when(!detail.is_empty(), |d| d.aria_description(detail.clone()))
                .aria_toggled(if tick.on {
                    gpui::Toggled::True
                } else {
                    gpui::Toggled::False
                })
                .track_focus(&focus)
                .flex_none()
                .p(px(2.))
                .rounded_sm()
                .border_1()
                .border_color(gpui::transparent_black())
                .focus(move |s| s.border_color(accent))
                .cursor_pointer()
                .child(controls::check_box(tick.on, theme))
                .on_click(cx.listener(move |this, _, _, _| {
                    if let Err(error) = this.app.dispatch(&turned) {
                        this.app
                            .notify(skwir_core::app::NotifyLevel::Error, Words::of_error(error));
                    }
                }))
        });
        inset_row(theme, edge.first)
            .key_context("SettingRecord")
            .children(tick)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(sized(2.))
                    .child(crate::elements::clip::clipped(
                        div()
                            .id(gpui::SharedString::from(format!("record-title-{key}-{id}")))
                            .text_body()
                            .truncate(),
                        title,
                    ))
                    .when(!detail.is_empty() && !wraps, |d| {
                        d.child(crate::elements::clip::clipped(
                            div()
                                .id(gpui::SharedString::from(format!(
                                    "record-detail-{key}-{id}"
                                )))
                                .text_small()
                                .text_color(theme.text_muted)
                                .truncate(),
                            detail.clone(),
                        ))
                    })
                    // What to do (a folder too deep, a library with
                    // nothing to color with) is read whole.
                    .when(!detail.is_empty() && wraps, |d| {
                        d.child(
                            div()
                                .text_body()
                                .text_color(theme.text_muted)
                                .child(detail.clone()),
                        )
                    })
                    .when(owns, |d| d.child(own)),
            )
            .child(actions)
            .into_any_element()
    }

    /// A record's own choice: a dropdown saying the value held, its
    /// choices in a popup under it, one that cannot be chosen greyed with
    /// why; choosing runs the owner's command. Left and Right step through
    /// them while it has the keyboard (`Pick`).
    /// `named`: what a screen reader hears it called and described as,
    /// for a choice that is a setting's own (`SettingKind::KeptChoice`);
    /// a record's is the record line's.
    #[allow(clippy::too_many_arguments)] // a choice, its place and its name
    pub(crate) fn record_choice(
        &mut self,
        key: &str,
        id: &str,
        choice: &serde_json::Map<String, serde_json::Value>,
        item: usize,
        named: Option<(gpui::SharedString, String)>,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(run) = Run::of(choice) else {
            return div().into_any_element();
        };
        let text = |value: &serde_json::Value, field: &str| {
            value
                .get(field)
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let held = choice
            .get("value")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let offered: Vec<(String, String, String, bool)> = choice
            .get("choices")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .map(|one| {
                (
                    text(one, "value"),
                    text(one, "label"),
                    text(one, "detail"),
                    one.get("disabled").and_then(serde_json::Value::as_bool) == Some(true),
                )
            })
            .collect();
        let focus = self.handle(&format!("pick:{key}:{id}"), cx);
        self.stop(&focus, Some(item), None);
        self.record_stops
            .push((focus.clone(), key.to_owned(), id.to_owned()));
        let values: Vec<String> = offered
            .iter()
            .filter(|(_, _, _, disabled)| !disabled)
            .map(|(value, ..)| value.clone())
            .collect();
        let pick = Pick {
            focus: focus.clone(),
            run,
            held: values.iter().position(|value| *value == held),
            values,
            asked_as: (key.to_owned(), id.to_owned()),
            listed: self.sourced.get(key).map_or(0, |((owner, _), _)| *owner),
        };
        self.record_picks.push(pick.clone());
        let open = self
            .record_open
            .as_ref()
            .is_some_and(|(k, i)| k == key && i == id);
        let label = choice
            .get("label")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let anchor = crate::elements::popover::PopoverAnchor::default();
        let (opened_key, opened_id, focused) = (key.to_owned(), id.to_owned(), focus.clone());
        let mut control = div().child(
            controls::dropdown(format!("pick-{key}-{id}"), label, &focus, theme)
                .min_w(sized(180.))
                .aria_expanded(open)
                .when_some(named, |d, (label, description)| {
                    d.aria_label(label).aria_description(description)
                })
                // Left and Right step through it, as through a choice's.
                .key_context("SettingControl")
                .child(anchor.marker())
                .on_click(cx.listener(move |this, _, window, cx| {
                    window.focus(&focused, cx);
                    let this_one = Some((opened_key.clone(), opened_id.clone()));
                    this.record_open = if this.record_open == this_one {
                        None
                    } else {
                        this_one
                    };
                    cx.notify();
                })),
        );
        if open {
            let hover = theme.hover;
            let mut panel = div()
                .id(gpui::SharedString::from(format!("picks-{key}-{id}")))
                .min_w(sized(220.))
                // A choice's detail (why a grammar cannot be used, a path)
                // wraps rather than widening the popup past the dialog.
                .max_w(sized(440.))
                .max_h(sized(360.))
                .overflow_y_scroll()
                .p_1()
                .flex()
                .flex_col()
                .rounded_md()
                .border_1()
                .border_color(theme.border)
                .bg(theme.bg)
                .text_color(theme.text)
                .text_body()
                .font_family(theme.font.clone())
                .shadow_lg()
                .occlude()
                .on_mouse_down_out(anchor.outside(cx.listener(|this, _, _, cx| {
                    this.record_open = None;
                    cx.notify();
                })));
            for (n, (value, label, detail, disabled)) in offered.into_iter().enumerate() {
                let selected = value == held;
                let pick = pick.clone();
                panel = panel.child(
                    div()
                        .id(crate::elements::known_by("pick-choice", (key, id, n)))
                        .px_2()
                        .py_1()
                        .flex()
                        .items_start()
                        .gap_2()
                        .rounded_sm()
                        .when(!disabled, |d| {
                            d.cursor_pointer().hover(move |s| s.bg(hover))
                        })
                        .when(disabled, |d| d.text_color(theme.text_muted))
                        .child(div().w(px(14.)).flex_none().when(selected, |d| {
                            d.child(controls::glyph("check", theme.accent, 14.))
                        }))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .min_w_0()
                                .child(label)
                                .when(!detail.is_empty(), |d| {
                                    d.child(div().text_color(theme.text_muted).child(detail))
                                }),
                        )
                        .when(!disabled, |d| {
                            d.on_click(cx.listener(move |this, _, _, cx| {
                                this.record_open = None;
                                if !selected {
                                    pick.ask(&value, this);
                                }
                                cx.notify();
                            }))
                        }),
                );
            }
            // Under the dropdown's right edge, reaching left: inside the
            // dialog, as the dropdown stands at the row's right.
            control = control.child(anchor.popup_right(panel));
        }
        control.into_any_element()
    }

    /// Half of a record's row: the drop zone of the gap before record
    /// `gap`, drawn as a line on the row's top or bottom edge while a record
    /// of the same list would move there. It does not block the row's
    /// clicks.
    fn record_gap(
        &self,
        key: &str,
        gap: usize,
        count: usize,
        top: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let edge = theme.accent;
        let (shown, dropped) = (key.to_owned(), key.to_owned());
        let zone = div().absolute().left_0().right_0().h(gpui::relative(0.5));
        let zone = if top { zone.top_0() } else { zone.bottom_0() };
        zone.drag_over::<RecordDrag>(move |s, drag, _, _| {
            let moves = drag.key == shown && moved(&vec![(); count], drag.index, gap).is_some();
            match (moves, top) {
                (false, _) => s,
                (true, true) => s.border_t_2().border_color(edge),
                (true, false) => s.border_b_2().border_color(edge),
            }
        })
        .on_drop(cx.listener(move |this, drag: &RecordDrag, _, cx| {
            if drag.key == dropped {
                this.move_record(&dropped, drag.index, gap, cx);
            }
        }))
    }

    fn record_form(
        &mut self,
        key: &str,
        edge: crate::settings_view::Edge,
        item: usize,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(draft) = self.drafts.get(key) else {
            return div().into_any_element();
        };
        let (error, busy) = (draft.error.clone(), draft.busy);
        // Detached from the draft: building the controls borrows `self`.
        let fields: Vec<(
            RecordField,
            Vec<(Entity<TextInput>, Option<Entity<TextInput>>)>,
            Option<bool>,
        )> = draft
            .fields
            .iter()
            .map(|(field, input)| match input {
                FieldInput::Text(input) => (field.clone(), vec![(input.clone(), None)], None),
                FieldInput::Pairs(pairs) => (
                    field.clone(),
                    pairs
                        .iter()
                        .map(|p| (p.name.clone(), Some(p.value.clone())))
                        .collect(),
                    None,
                ),
                FieldInput::Secret { input, stored } => {
                    (field.clone(), vec![(input.clone(), None)], Some(*stored))
                }
            })
            .collect();
        let mut form = div().flex_1().min_w_0().flex().flex_col().gap(sized(12.));
        for (index, (field, inputs, secret)) in fields.into_iter().enumerate() {
            let mut body = div().flex().flex_col().gap(sized(6.));
            let pairs = field.kind == FieldKind::Pairs;
            for (pair, (first, second)) in inputs.into_iter().enumerate() {
                let focused = first.focus_handle(cx).is_focused(window);
                self.stop(&first.focus_handle(cx), Some(item), None);
                let mut line = div()
                    .flex()
                    .items_center()
                    .gap(sized(6.))
                    .child(controls::field(
                        &first,
                        pairs.then_some(180.),
                        focused,
                        false,
                        None,
                        theme,
                    ));
                if let Some(second) = second {
                    let focused = second.focus_handle(cx).is_focused(window);
                    self.stop(&second.focus_handle(cx), Some(item), None);
                    let focus = self.handle(&format!("pair-remove:{key}:{index}:{pair}"), cx);
                    let remove_key = key.to_owned();
                    line = line
                        .child(controls::field(&second, None, focused, false, None, theme))
                        .child(
                            controls::icon_button(
                                format!("pair-remove-{key}-{index}-{pair}"),
                                "close",
                                words!(
                                    records_pair_remove = "Remove",
                                    note = "The label of a button that takes a name and value pair (an HTTP header) out of a record."
                                )
                                .say(),
                                &focus,
                                theme,
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.remove_pair(&remove_key, index, pair, cx);
                                },
                            )),
                        );
                }
                if secret == Some(true) {
                    let focus = self.handle(&format!("forget:{key}:{index}"), cx);
                    self.stop(&focus, Some(item), None);
                    let forget_key = key.to_owned();
                    line = line.child(
                        controls::button(
                            format!("forget-{key}-{index}"),
                            words!(
                                records_secret_forget = "Forget",
                                note = "A button beside a secret's field (a password): the keyring forgets it once the record is saved."
                            )
                            .say(),
                            None,
                            ButtonStyle::Danger,
                            &focus,
                            theme,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.forget_secret(&forget_key, index, cx);
                        })),
                    );
                }
                body = body.child(line);
            }
            if pairs {
                let focus = self.handle(&format!("pair-add:{key}:{index}"), cx);
                self.stop(&focus, Some(item), None);
                let add_key = key.to_owned();
                body = body.child(
                    div().flex().child(
                        controls::button(
                            format!("pair-add-{key}-{index}"),
                            words!(
                                records_pair_add = "Add",
                                note = "A button that adds a name and value pair (an HTTP header) to a record."
                            )
                            .say(),
                            Some("plus"),
                            ButtonStyle::Plain,
                            &focus,
                            theme,
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| this.add_pair(&add_key, index, window, cx),
                        )),
                    ),
                );
            }
            form = form.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(sized(4.))
                    .child(
                        div().text_body().child(
                            if field.required {
                                field.title.clone()
                            } else {
                                words!(
                                    records_field_optional = "{ $field } (optional)",
                                    field = field.title.clone(),
                                    note = "A record's field the user may leave empty: field is its name (Headers)."
                                )
                            }
                            .say(),
                        ),
                    )
                    .child(body)
                    .when(!field.description.is_empty(), |d| {
                        d.child(
                            div()
                                .text_body()
                                .text_color(theme.text_muted)
                                .child(field.description.say()),
                        )
                    }),
            );
        }
        let (cancel_focus, save_focus) = (
            self.handle(&format!("draft-cancel:{key}"), cx),
            self.handle(&format!("draft-save:{key}"), cx),
        );
        self.stop(&cancel_focus, Some(item), None);
        self.stop(&save_focus, Some(item), None);
        let (cancel_key, save_key) = (key.to_owned(), key.to_owned());
        let footer = div()
            .flex()
            .items_center()
            .gap(sized(8.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_body()
                    .text_color(if error.is_some() {
                        theme.error
                    } else {
                        theme.text_muted
                    })
                    .child(match error {
                        Some(error) => error.say(),
                        None if busy => words!(
                            records_keyring_asking = "Asking the keyring…",
                            note = "Said while a record holding a secret (a password) waits for the desktop's keyring."
                        )
                        .say(),
                        None => String::new(),
                    }),
            )
            .child(
                controls::button(
                    format!("draft-cancel-{key}"),
                    words!(
                        records_cancel = "Cancel",
                        note = "A button of a record's form in the settings: close it, nothing kept."
                    )
                    .say(),
                    None,
                    ButtonStyle::Plain,
                    &cancel_focus,
                    theme,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.cancel_record(&cancel_key, window, cx);
                })),
            )
            .child(
                controls::button(
                    format!("draft-save-{key}"),
                    words!(
                        records_save = "Save",
                        note = "A button of a record's form in the settings: keep it."
                    )
                    .say(),
                    None,
                    ButtonStyle::Plain,
                    &save_focus,
                    theme,
                )
                .when(busy, |b| b.opacity(0.5))
                .on_click(cx.listener(move |this, _, _, cx| this.save_record(&save_key, cx))),
            );
        inset_row(theme, edge.first)
            .items_start()
            .py_3()
            .child(form.child(footer))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{Line, Record, Shape, lines, moved, said_answer, stepped};

    /// What an owner says beside its records (ADR-150, 6): its groups in
    /// order, a group's switch and a record's tick, the note under it.
    #[test]
    fn an_owner_s_groups_ticks_and_note_are_read_and_drawn_in_order() {
        let answer = serde_json::json!({
            "records": [],
            "groups": [
                { "id": "own", "title": "Your folders",
                  "switch": { "on": false, "command": "x.follow", "args": { "source": "own" } } },
                { "id": "added", "title": "Added by you" },
                { "id": "empty", "title": "Nothing here" },
                { "title": "no id: none" },
            ],
            "note": "Watches used: 12 of 100",
        });
        let shape = Shape::of(&answer);
        assert_eq!(
            shape
                .groups
                .iter()
                .map(|g| g.id.as_str())
                .collect::<Vec<_>>(),
            ["own", "added", "empty"]
        );
        assert_eq!(shape.note.as_deref(), Some("Watches used: 12 of 100"));
        let switch = shape.groups[0].switch.clone().unwrap();
        assert!(!switch.on);
        let turned = switch.turned();
        assert_eq!(&*turned.name, "x.follow");
        assert_eq!(
            turned.args,
            serde_json::json!({ "source": "own", "on": true })
        );
        assert_eq!(
            Shape::of(&serde_json::json!({ "records": [] })),
            Shape::default()
        );

        let record = |id: &str, group: Option<&str>| -> Record {
            let mut r = Record::new();
            r.insert("id".into(), id.into());
            if let Some(group) = group {
                r.insert("group".into(), group.into());
            }
            r
        };
        let (a, b, c, d) = (
            record("a", Some("added")),
            record("b", Some("own")),
            record("c", None),
            record("d", Some("own")),
        );
        let shown = [(0, Some(&a)), (1, Some(&b)), (2, Some(&c)), (3, Some(&d))];
        let drawn: Vec<String> = lines(&shape, &shown)
            .iter()
            .map(|line| match line {
                Line::Heading(group, count) => format!("{} ({count})", group.id),
                Line::Record(index, _, dimmed) => {
                    format!("{index}{}", if *dimmed { " dim" } else { "" })
                }
            })
            .collect();
        assert_eq!(
            drawn,
            ["own (2)", "1 dim", "3 dim", "added (1)", "0", "2"],
            "in the groups' order, a group switched off dimmed, the empty one unheaded, the loose last"
        );
        let plain: Vec<usize> = lines(&Shape::default(), &shown)
            .iter()
            .filter_map(|line| match line {
                Line::Record(index, _, _) => Some(*index),
                Line::Heading(..) => None,
            })
            .collect();
        assert_eq!(plain, [0, 1, 2, 3], "no groups: the records alone");
    }

    /// Left and Right through a choice: from the one held, staying at the
    /// ends; from none held (Not chosen), Right the first and Left the
    /// last, never skipping the first (UA-6).
    #[test]
    fn a_choice_steps_from_what_it_holds() {
        assert_eq!(stepped(Some(1), 1, 3), 2);
        assert_eq!(stepped(Some(3), 1, 3), 3);
        assert_eq!(stepped(Some(0), -1, 3), 0);
        assert_eq!(stepped(None, 1, 3), 0);
        assert_eq!(stepped(None, -1, 3), 3);
    }

    /// An owner's answer says its words' data in the window's language
    /// (here the pseudo-language), as the dialog lists it, where the
    /// command's JSON holds their English for agents; text as written,
    /// values and what a choice runs with are left as they came (UA-1).
    #[test]
    fn an_owners_words_are_said_where_the_dialog_lists_them() {
        use skwir_core::words::{PSEUDO, Words};
        let label = skwir_core::words!(test_records_label = "Automatic ({ $now })", now = "bat");
        let answer = serde_json::json!({
            "value": "auto",
            "label": label.to_data(),
            "choices": [
                { "value": "auto", "label": label.to_data(), "detail": "" },
                { "value": "plain", "label": "Plain text as written" },
            ],
            "command": "ui.grammar_choose",
            "args": { "language": "rust", "said": label.to_data() },
            "field": "grammar",
        });
        assert_eq!(answer["label"]["english"], "Automatic (bat)", "for agents");
        skwir_core::locale::set_chosen(Some(PSEUDO.into()));
        let said = said_answer(answer.clone());
        let pseudo = label.say();
        assert_ne!(pseudo, "Automatic (bat)");
        assert_eq!(said["label"], pseudo.as_str());
        assert_eq!(said["choices"][0]["label"], pseudo.as_str());
        assert_eq!(said["choices"][0]["detail"], "");
        assert_eq!(said["choices"][1]["label"], "Plain text as written");
        assert_eq!(said["value"], "auto");
        assert_eq!(said["args"], answer["args"], "sent back as it came");
        let _: Words = serde_json::from_value(said["args"]["said"].clone()).unwrap();
    }

    #[test]
    fn a_record_moves_into_the_gap_it_is_dropped_in() {
        let list = ["a", "b", "c", "d"];
        // Down: into the gap before "d".
        assert_eq!(moved(&list, 0, 3).unwrap(), ["b", "c", "a", "d"]);
        // To the end.
        assert_eq!(moved(&list, 1, 4).unwrap(), ["a", "c", "d", "b"]);
        // Up: into the gap before "a".
        assert_eq!(moved(&list, 2, 0).unwrap(), ["c", "a", "b", "d"]);
        // The gaps on either side of it leave it where it is.
        assert!(moved(&list, 2, 2).is_none());
        assert!(moved(&list, 2, 3).is_none());
        // Nothing beyond the list.
        assert!(moved(&list, 4, 0).is_none());
        assert!(moved(&list, 0, 5).is_none());
    }
}
