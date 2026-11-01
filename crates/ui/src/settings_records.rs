//! A list of records in the Settings dialog (`SettingKind::Records`): tile
//! servers, remotes, the languages and their grammars. The list is drawn
//! inset in its setting's card, a list under the setting rather than more
//! settings, saying so when it is empty; a long one (`FOLD`) has a filter
//! and shows its first rows until unfolded. Each record is a row with its
//! actions (the owner's own, such as Test; Edit; Remove; a choice of its
//! own); one at a time is edited in place, a draft kept apart from the
//! rows, which are rebuilt whenever a setting changes.
//!
//! A secret field's value goes to the desktop's keyring and never to the
//! settings (`skwir_platform::secrets`): saving a draft that holds a new
//! one stores it on a worker first, and only then saves the list, which
//! says the keyring has it. Removing a record forgets its secrets. The
//! whole list is validated before anything is kept, so a record the owner
//! refuses leaves nothing in the keyring.
use std::rc::Rc;
use std::sync::Arc;

use gpui::{AnyElement, Context, Entity, Focusable, Subscription, Window, div, prelude::*, px};
use skwir_core::settings::{FieldKind, RecordField, SECRET_STORED, SettingKind};

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
/// the one held.
#[derive(Clone)]
pub(crate) struct Pick {
    pub(crate) focus: gpui::FocusHandle,
    run: Run,
    values: Vec<String>,
    held: Option<usize>,
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

    /// Run it with `value`; what refuses it is said.
    fn with(&self, app: &skwir_core::App, value: &str) {
        let mut args = self.args.clone();
        if !args.is_object() {
            args = serde_json::json!({});
        }
        args[self.field.as_str()] = serde_json::json!(value);
        let invocation = skwir_core::command::Invocation::with_args(self.command.as_str(), &args);
        if let Err(error) = app.dispatch(&invocation) {
            app.notify(skwir_core::app::NotifyLevel::Error, format!("{error:#}"));
        }
    }
}

impl Pick {
    /// Choose the value `delta` from the one held, staying at the ends.
    pub(crate) fn step(&self, delta: i64, view: &SettingsView) {
        let Some(last) = self.values.len().checked_sub(1) else {
            return;
        };
        let held = i64::try_from(self.held.unwrap_or(0)).unwrap_or(0);
        let at = usize::try_from((held + delta).max(0))
            .unwrap_or(0)
            .min(last);
        if Some(at) != self.held {
            self.run.with(&view.app, &self.values[at]);
        }
    }
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
    error: Option<String>,
    /// Waiting for the keyring.
    busy: bool,
    subscriptions: Vec<Subscription>,
}

/// What a keyring job came back with: the list to save, or why not.
pub(crate) struct KeyringDone {
    key: String,
    result: Result<String, String>,
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
        placeholder: String,
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
                    String::new(),
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
                                "Kept in the keyring: type to replace".into()
                            } else {
                                String::new()
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
        Pair {
            name: self.draft_input(key, name, "Name".into(), false, subscriptions, window, cx),
            value: self.draft_input(key, value, "Value".into(), false, subscriptions, window, cx),
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
            input.update(cx, |input, cx| input.set_placeholder("Not set", cx));
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
                draft.error = Some(format!("{error:#}"));
            }
            cx.notify();
            return;
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
        let label = format!("skwir: {name} ({})", spec.title);
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
                .map_err(|error| format!("The keyring did not keep it: {error:#}"));
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
                            format!("The keyring kept a secret of the record removed: {error:#}"),
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
        let editing = self.drafts.get(key).map(|d| (d.id.clone(), d.new));
        let all = self.parsed_records(key, value);
        // A long list: its filter, and its first rows until unfolded.
        let long = all.len() > FOLD;
        let filter = long.then(|| self.record_filter(key, cx));
        let query = filter
            .as_ref()
            .map(|input| input.read(cx).text().trim().to_lowercase())
            .unwrap_or_default();
        let text_fields: Vec<String> = spec
            .constraints
            .fields
            .iter()
            .filter(|f| matches!(f.kind, FieldKind::Text { .. }))
            .map(|f| f.key.clone())
            .collect();
        let matches = |record: &Record| {
            query.is_empty()
                || text_fields
                    .iter()
                    .any(|field| text_of(record, field).to_lowercase().contains(&query))
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
        for (n, (index, record)) in shown.iter().enumerate() {
            let edge = crate::settings_view::Edge {
                first: n == 0,
                last: n + 1 == shown.len(),
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
            rows = rows.child(element);
        }
        if shown.is_empty() {
            let said = if !query.is_empty() {
                format!("Nothing matches “{query}”")
            } else if spec
                .constraints
                .record_source
                .as_ref()
                .is_some_and(|source| source.add.is_empty())
            {
                "None found".to_owned()
            } else {
                "None yet".to_owned()
            };
            rows = rows.child(
                inset_row(theme, true)
                    .text_sm()
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
                            .text_xs()
                            .text_color(theme.text_muted)
                            .child(if query.is_empty() {
                                format!("{found}")
                            } else {
                                format!("{found} of {}", all.len())
                            }),
                    ),
            );
        }
        block = block.child(div().w_full().child(rows));
        if long && query.is_empty() {
            let focus = self.handle(&format!("fold:{key}"), cx);
            self.stop(&focus, Some(item), None);
            let toggled = key.to_owned();
            let label = if unfolded {
                "Show fewer".to_owned()
            } else {
                format!("Show all {found}")
            };
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
            let mut input = TextInput::new("Filter".to_owned(), cx);
            input.set_label("Filter the list".to_owned());
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
        let mut actions = div().flex().flex_none().items_center().gap(px(6.));
        if let Some(action) = spec.constraints.record_action.clone() {
            let focus = self.handle(&format!("action:{key}:{id}"), cx);
            self.stop(&focus, Some(item), None);
            self.record_stops
                .push((focus.clone(), key.to_owned(), id.to_owned()));
            let args = serde_json::json!({ "id": id });
            actions = actions.child(
                controls::button(
                    format!("action-{key}-{id}"),
                    action.label.clone(),
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
                            .notify(skwir_core::app::NotifyLevel::Error, format!("{error:#}"));
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
                    "Edit",
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
                    "Remove",
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
                    .gap(px(2.))
                    .child(crate::elements::clip::clipped(
                        div()
                            .id(gpui::SharedString::from(format!("record-title-{key}-{id}")))
                            .text_sm()
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
                                .text_xs()
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
        let verbs = [
            ("Edit", source.edit.clone(), ButtonStyle::Plain),
            ("Remove", source.remove.clone(), ButtonStyle::Danger),
        ];
        for (verb, command, style) in verbs.into_iter().filter(|_| !fixed) {
            let focus = self.handle(&format!("{verb}:{key}:{id}"), cx);
            self.stop(&focus, Some(item), None);
            self.record_stops
                .push((focus.clone(), key.to_owned(), id.to_owned()));
            let args = serde_json::json!({ "id": id });
            actions = actions.child(
                controls::button(
                    format!("{verb}-{key}-{id}"),
                    verb,
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
                            .notify(skwir_core::app::NotifyLevel::Error, format!("{error:#}"));
                    }
                })),
            );
        }
        // A choice of its own (a language's grammar), last of its actions.
        if let Some(choice) = record.get("choice").and_then(serde_json::Value::as_object) {
            actions = actions.child(self.record_choice(key, id, choice, item, theme, cx));
        }
        // What else the line does, as the owner says, after Edit and
        // Remove for the keys: they stand above.
        let mut own = div().flex().flex_wrap().gap(px(6.)).pt(px(4.));
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
                            .notify(skwir_core::app::NotifyLevel::Error, format!("{error:#}"));
                    }
                })),
            );
            owns = true;
        }
        inset_row(theme, edge.first)
            .key_context("SettingRecord")
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(crate::elements::clip::clipped(
                        div()
                            .id(gpui::SharedString::from(format!("record-title-{key}-{id}")))
                            .text_sm()
                            .truncate(),
                        title,
                    ))
                    .when(!detail.is_empty(), |d| {
                        d.child(crate::elements::clip::clipped(
                            div()
                                .id(gpui::SharedString::from(format!(
                                    "record-detail-{key}-{id}"
                                )))
                                .text_xs()
                                .text_color(theme.text_muted)
                                .truncate(),
                            detail,
                        ))
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
    fn record_choice(
        &mut self,
        key: &str,
        id: &str,
        choice: &serde_json::Map<String, serde_json::Value>,
        item: usize,
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
        self.record_picks.push(Pick {
            focus: focus.clone(),
            run: run.clone(),
            held: values.iter().position(|value| *value == held),
            values,
        });
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
                .min_w(px(180.))
                .aria_expanded(open)
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
                .min_w(px(220.))
                .max_h(px(360.))
                .overflow_y_scroll()
                .p_1()
                .flex()
                .flex_col()
                .rounded_md()
                .border_1()
                .border_color(theme.border)
                .bg(theme.bg)
                .text_color(theme.text)
                .text_xs()
                .font_family(theme.font.clone())
                .shadow_lg()
                .occlude()
                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    this.record_open = None;
                    cx.notify();
                }));
            for (n, (value, label, detail, disabled)) in offered.into_iter().enumerate() {
                let selected = value == held;
                let run = run.clone();
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
                                .child(label)
                                .when(!detail.is_empty(), |d| {
                                    d.child(div().text_color(theme.text_muted).child(detail))
                                }),
                        )
                        .when(!disabled, |d| {
                            d.on_click(cx.listener(move |this, _, _, cx| {
                                this.record_open = None;
                                if !selected {
                                    run.with(&this.app, &value);
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
        let mut form = div().flex_1().min_w_0().flex().flex_col().gap(px(12.));
        for (index, (field, inputs, secret)) in fields.into_iter().enumerate() {
            let mut body = div().flex().flex_col().gap(px(6.));
            let pairs = field.kind == FieldKind::Pairs;
            for (pair, (first, second)) in inputs.into_iter().enumerate() {
                let focused = first.focus_handle(cx).is_focused(window);
                self.stop(&first.focus_handle(cx), Some(item), None);
                let mut line = div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
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
                                "Remove",
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
                            "Forget",
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
                            "Add",
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
                    .gap(px(4.))
                    .child(div().text_sm().child(format!(
                        "{}{}",
                        field.title,
                        if field.required { "" } else { " (optional)" }
                    )))
                    .child(body)
                    .when(!field.description.is_empty(), |d| {
                        d.child(
                            div()
                                .text_xs()
                                .text_color(theme.text_muted)
                                .child(field.description.clone()),
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
            .gap(px(8.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_xs()
                    .text_color(if error.is_some() {
                        theme.error
                    } else {
                        theme.text_muted
                    })
                    .child(error.unwrap_or_else(|| {
                        if busy {
                            "Asking the keyring…".into()
                        } else {
                            String::new()
                        }
                    })),
            )
            .child(
                controls::button(
                    format!("draft-cancel-{key}"),
                    "Cancel",
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
                    "Save",
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
    use super::moved;

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
