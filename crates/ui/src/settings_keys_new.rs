//! A new shortcut, for an action no row lists (ADR-024): a command whose
//! arguments no form covers (a folder to go to, a tag to add). The form
//! asks for the command, its arguments as its schema says them, where the
//! keys work and the keys; what it saves is a record of the user's, as a
//! row's + saves one.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    AnyElement, App, Context, Entity, Focusable, SharedString, Subscription, Window, div,
    prelude::*, px,
};
use serde_json::Value;
use skwir_core::command::{CommandSpec, Invocation};

use crate::controls::{self, ButtonStyle};
use crate::input::{InputEvent, TextInput};
use crate::keymap;
use crate::settings::SettingsView;
use crate::settings_keys::{Action, Edit};
use crate::settings_keys_view::Target;
use crate::theme::Theme;

/// Commands listed at once while one is looked for.
const SHOWN: usize = 8;

/// The file list's conditions a shortcut may ask for, as `when` says them.
const FLAGS: [&str; 8] = [
    "selection",
    "single",
    "empty",
    "dir",
    "file",
    "folder",
    "split",
    "!restores",
];

/// What a command's argument is, as its schema says it.
enum Kind {
    Text,
    Number,
    Switch,
    Choice(Vec<String>),
    /// Anything else, as JSON.
    Json,
}

/// One argument of the command picked.
struct Field {
    name: String,
    description: String,
    required: bool,
    kind: Kind,
    input: Option<Entity<TextInput>>,
    on: bool,
    chosen: Option<usize>,
}

/// A command picked, with the arguments a form fixed or the fields that
/// ask for them.
struct Picked {
    command: String,
    title: String,
    /// The arguments of a form, fixed; `None` when the fields say them.
    args: Option<Value>,
    fields: Vec<Field>,
    /// What its fields say, as they change.
    _subscriptions: Vec<Subscription>,
}

/// A command a shortcut may run, as the form lists it: with a form's
/// arguments, or `None` for the fields to ask them, and its title.
type Candidate = (CommandSpec, Option<Value>, String);

/// The form of a new shortcut.
pub(crate) struct NewShortcut {
    search: Entity<TextInput>,
    /// Every command a shortcut may run, read once when the form opens.
    candidates: Rc<Vec<Candidate>>,
    /// Those the search finds, for the search they were found for.
    found: RefCell<Option<(String, Rc<Vec<usize>>)>>,
    picked: Option<Picked>,
    /// The arguments the fields say, or why they cannot be run: worked out
    /// again when a field changes, not at each frame.
    args: RefCell<Option<Result<(String, Value), String>>>,
    /// Where the keys work: an index in `places()`.
    place: usize,
    /// The file list's conditions asked for.
    flags: Vec<&'static str>,
    places_open: bool,
    _subscriptions: Vec<Subscription>,
}

impl NewShortcut {
    /// The arguments are to be read again from the fields.
    fn changed(&self) {
        self.args.borrow_mut().take();
    }
}

/// The `i`th field of the command picked.
fn field_at(view: &mut SettingsView, i: usize) -> Option<&mut Field> {
    view.keys
        .new
        .as_mut()
        .and_then(|new| new.picked.as_mut())
        .and_then(|picked| picked.fields.get_mut(i))
}

/// The arguments `fields` say, or why they cannot be run.
fn args_of(fields: &[Field], cx: &App) -> Result<Value, String> {
    let mut args = serde_json::Map::new();
    for field in fields {
        let text = field
            .input
            .as_ref()
            .map(|input| input.read(cx).text().trim().to_owned())
            .unwrap_or_default();
        let value = match &field.kind {
            Kind::Text if !text.is_empty() => Some(Value::String(text)),
            Kind::Number if !text.is_empty() => Some(
                serde_json::from_str::<serde_json::Number>(&text)
                    .map(Value::Number)
                    .map_err(|_| format!("{} is a number", field.name))?,
            ),
            Kind::Json if !text.is_empty() => Some(
                serde_json::from_str(&text).map_err(|error| format!("{}: {error}", field.name))?,
            ),
            Kind::Switch if field.on => Some(Value::Bool(true)),
            Kind::Choice(values) => field
                .chosen
                .and_then(|i| values.get(i))
                .map(|v| Value::String(v.clone())),
            _ => None,
        };
        match value {
            Some(value) => {
                args.insert(field.name.clone(), value);
            }
            None if field.required => {
                return Err(format!("{} is needed", field.name));
            }
            None => {}
        }
    }
    Ok(Value::Object(args))
}

/// Where a new shortcut's keys may work: anywhere, then each place once,
/// by its `when` token and what it is called.
fn places() -> Vec<(Option<&'static str>, &'static str)> {
    let mut places = vec![(None, "Anywhere")];
    for place in keymap::PLACES {
        if !places.iter().any(|(_, label)| *label == place.label) {
            places.push((Some(place.token), place.label));
        }
    }
    places
}

/// Every command a shortcut may run: each form, and each command as it
/// is, asking for its arguments when it takes some; the shell's own as the
/// palette offers them, their other forms being keys of a place.
fn candidates(app: &skwir_core::App) -> Vec<Candidate> {
    let mut all = Vec::new();
    for spec in app.commands.list() {
        if matches!(&*spec.name, "settings.apply" | "plugins.set_enabled") || spec.data {
            continue;
        }
        let shell = &*spec.owner == crate::commands::OWNER;
        for form in &spec.forms {
            if !shell || crate::settings_keys::offered(&spec.name, &form.args) {
                all.push((spec.clone(), Some(form.args.clone()), form.title.clone()));
            }
        }
        if shell {
            continue;
        }
        let takes = properties(&spec).is_some_and(|p| !p.is_empty());
        if takes || spec.forms.is_empty() {
            let title = if takes {
                format!("{}…", spec.title.trim_end_matches('…'))
            } else {
                spec.title.clone()
            };
            let fixed = (!takes).then(|| serde_json::json!({}));
            all.push((spec, fixed, title));
        }
    }
    all.sort_by_cached_key(|(_, _, title)| title.to_lowercase());
    all
}

/// The properties a command's schema names, with the schema they are in.
fn properties(spec: &CommandSpec) -> Option<serde_json::Map<String, Value>> {
    let schema = serde_json::to_value(spec.args_schema.get()).ok()?;
    schema.get("properties")?.as_object().cloned()
}

/// A property's schema, its `$ref` and `Option`'s `null` looked through.
fn resolved<'a>(root: &'a Value, schema: &'a Value) -> &'a Value {
    if let Some(name) = schema
        .get("$ref")
        .and_then(Value::as_str)
        .and_then(|r| r.rsplit('/').next())
    {
        for defs in ["$defs", "definitions"] {
            if let Some(def) = root.get(defs).and_then(|d| d.get(name)) {
                return resolved(root, def);
            }
        }
    }
    for any in ["anyOf", "oneOf"] {
        if let Some(options) = schema.get(any).and_then(Value::as_array) {
            let real: Vec<&Value> = options
                .iter()
                .filter(|o| o.get("type").and_then(Value::as_str) != Some("null"))
                .collect();
            if let [one] = real.as_slice()
                && one.get("const").is_none()
            {
                return resolved(root, one);
            }
        }
    }
    schema
}

fn kind_of(root: &Value, schema: &Value) -> Kind {
    let schema = resolved(root, schema);
    let strings = |values: &Vec<Value>| -> Vec<String> {
        values
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect()
    };
    if let Some(values) = schema.get("enum").and_then(Value::as_array) {
        return Kind::Choice(strings(values));
    }
    if let Some(options) = schema.get("oneOf").and_then(Value::as_array) {
        let consts: Vec<Value> = options
            .iter()
            .filter_map(|o| {
                o.get("const")
                    .cloned()
                    .or_else(|| o.get("enum")?.as_array()?.first().cloned())
            })
            .collect();
        if consts.len() == options.len() {
            return Kind::Choice(strings(&consts));
        }
    }
    // Its types, and those of what it may be instead (a path is text, or
    // bytes where it is not UTF-8): text is the one a person types.
    let named = |schema: &Value| -> Vec<String> {
        match schema.get("type") {
            Some(Value::String(t)) => vec![t.clone()],
            Some(Value::Array(ts)) => ts
                .iter()
                .filter_map(|t| t.as_str().map(str::to_owned))
                .collect(),
            _ => Vec::new(),
        }
    };
    let mut types = named(schema);
    for any in ["anyOf", "oneOf"] {
        for option in schema
            .get(any)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            types.extend(named(resolved(root, option)));
        }
    }
    let types: Vec<&str> = types.iter().map(String::as_str).collect();
    if types.contains(&"boolean") {
        Kind::Switch
    } else if types.contains(&"integer") || types.contains(&"number") {
        Kind::Number
    } else if types.contains(&"string") {
        Kind::Text
    } else {
        Kind::Json
    }
}

impl SettingsView {
    /// Open the form, the command looked for first.
    pub(crate) fn open_new_shortcut(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let search = cx.new(|cx| TextInput::new("Search commands", cx));
        let subscription = cx.subscribe_in(
            &search,
            window,
            |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::Cancel => {
                    this.keys.new = None;
                    window.focus(&this.keys.focus);
                    cx.notify();
                }
                InputEvent::Submit => {
                    if let Some((spec, args, title)) = this.matching_commands(cx).into_iter().next()
                    {
                        this.pick_command(&spec, args, title, window, cx);
                    }
                }
                InputEvent::Changed => cx.notify(),
            },
        );
        self.keys.editor = None;
        self.keys.as_records = false;
        search.update(cx, |input, _| input.focus(window));
        self.keys.new = Some(NewShortcut {
            search,
            candidates: Rc::new(candidates(&self.app)),
            found: RefCell::default(),
            picked: None,
            args: RefCell::default(),
            place: 0,
            flags: Vec::new(),
            places_open: false,
            _subscriptions: vec![subscription],
        });
        cx.notify();
    }

    /// The keyboard back in the form, from its keys' recorder.
    pub(crate) fn focus_new_shortcut(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match &self.keys.new {
            Some(new) if new.picked.is_none() => {
                new.search.update(cx, |input, _| input.focus(window));
            }
            Some(_) => {
                let save = self.handle("new-save", cx);
                window.focus(&save);
            }
            None => window.focus(&self.keys.focus),
        }
    }

    /// The commands the search finds, the first `SHOWN`: looked for again
    /// only when the search changes.
    fn matching_commands(&self, cx: &App) -> Vec<Candidate> {
        let Some(new) = &self.keys.new else {
            return Vec::new();
        };
        let query = new.search.read(cx).text().trim().to_lowercase();
        let kept = new
            .found
            .borrow()
            .as_ref()
            .filter(|(asked, _)| *asked == query)
            .map(|(_, found)| Rc::clone(found));
        let found = kept.unwrap_or_else(|| {
            let found: Rc<Vec<usize>> = Rc::new(
                new.candidates
                    .iter()
                    .enumerate()
                    .filter(|(_, (spec, _, title))| {
                        query.is_empty()
                            || title.to_lowercase().contains(&query)
                            || spec.name.to_lowercase().contains(&query)
                    })
                    .map(|(n, _)| n)
                    .take(SHOWN)
                    .collect(),
            );
            *new.found.borrow_mut() = Some((query, Rc::clone(&found)));
            found
        });
        found
            .iter()
            .filter_map(|&n| new.candidates.get(n).cloned())
            .collect()
    }

    fn pick_command(
        &mut self,
        spec: &CommandSpec,
        args: Option<Value>,
        title: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let root = serde_json::to_value(spec.args_schema.get()).unwrap_or_default();
        let required: Vec<String> = spec.required_args();
        let mut fields = Vec::new();
        let mut subscriptions = Vec::new();
        if args.is_none() {
            for (name, schema) in properties(spec).unwrap_or_default() {
                // A key acts where the keyboard is: the pane an agent may
                // name is the one with the keyboard.
                if name == "pane" && !required.contains(&name) {
                    continue;
                }
                let kind = kind_of(&root, &schema);
                let input = matches!(kind, Kind::Text | Kind::Number | Kind::Json)
                    .then(|| cx.new(|cx| TextInput::new("", cx)));
                if let Some(input) = &input {
                    subscriptions.push(cx.subscribe(input, |this, _, event: &InputEvent, cx| {
                        if matches!(event, InputEvent::Changed)
                            && let Some(new) = &this.keys.new
                        {
                            new.changed();
                            cx.notify();
                        }
                    }));
                }
                fields.push(Field {
                    required: required.contains(&name),
                    description: schema
                        .get("description")
                        .or_else(|| resolved(&root, &schema).get("description"))
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    name,
                    kind,
                    input,
                    on: false,
                    chosen: None,
                });
            }
        }
        let first = fields.iter().find_map(|field| field.input.clone());
        if let Some(new) = self.keys.new.as_mut() {
            new.picked = Some(Picked {
                command: spec.name.to_string(),
                title,
                args,
                fields,
                _subscriptions: subscriptions,
            });
            new.changed();
        }
        match first {
            Some(input) => input.update(cx, |input, _| input.focus(window)),
            None => self.open_key_editor(Target::New, window, cx),
        }
        cx.notify();
    }

    /// The arguments the form says, or why they cannot be run: read from
    /// the fields and checked again only once one of them changed.
    fn new_shortcut_args(&self, cx: &App) -> Result<(String, Value), String> {
        let new = self
            .keys
            .new
            .as_ref()
            .ok_or("Choose what the keys run first")?;
        if let Some(said) = new.args.borrow().as_ref() {
            return said.clone();
        }
        let said = (|| {
            let picked = new
                .picked
                .as_ref()
                .ok_or("Choose what the keys run first")?;
            let args = if let Some(args) = &picked.args {
                args.clone()
            } else {
                args_of(&picked.fields, cx)?
            };
            self.app
                .commands
                .check(&Invocation {
                    name: picked.command.as_str().into(),
                    args: args.clone(),
                })
                .map_err(|error| format!("{error}"))?;
            Ok((picked.command.clone(), args))
        })();
        *new.args.borrow_mut() = Some(said.clone());
        said
    }

    /// Where the form's keys would work, as a context.
    fn new_shortcut_context(&self) -> String {
        let Some(new) = &self.keys.new else {
            return keymap::ROOT.to_owned();
        };
        let place = places().get(new.place).and_then(|(token, _)| *token);
        let mut when: Vec<&str> = place.into_iter().collect();
        if place == Some("pane") {
            when.extend(new.flags.iter().copied());
        }
        keymap::context_of((!when.is_empty()).then(|| when.join(" ")).as_deref())
    }

    /// The action the form would bind, and where.
    pub(crate) fn new_shortcut_action(&self, cx: &App) -> Option<(Action, String)> {
        let (command, args) = self.new_shortcut_args(cx).ok()?;
        let context = self.new_shortcut_context();
        let at = keymap::place_of(&context);
        Some((
            Action {
                place: at.place,
                rank: at.rank,
                inside: at.inside,
                title: keymap::title(&self.app, &command, &args),
                command,
                args,
                chips: Vec::new(),
                keeps_a_key: false,
            },
            context,
        ))
    }

    /// Save the form's shortcut on `keys`; `false` with the reason kept in
    /// the recorder.
    pub(crate) fn save_new_shortcut(&mut self, keys: &str, cx: &mut Context<Self>) -> bool {
        if let Err(why) = self.new_shortcut_args(cx) {
            if let Some(editor) = self.keys.editor.as_mut() {
                editor.error = Some(why);
            }
            return false;
        }
        let Some((action, context)) = self.new_shortcut_action(cx) else {
            return false;
        };
        self.save_keys(
            &Edit::Add {
                action: &action,
                keys: keys.to_owned(),
                context,
            },
            cx,
        )
    }

    /// The form, in place of the list.
    pub(crate) fn new_shortcut_page(
        &mut self,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(new) = &self.keys.new else {
            return div().into_any_element();
        };
        let (search, picked_title, place, flags, places_open) = (
            new.search.clone(),
            new.picked.as_ref().map(|p| p.title.clone()),
            new.place,
            new.flags.clone(),
            new.places_open,
        );
        let section = |title: &str| {
            div()
                .pt_4()
                .pb_2()
                .text_xs()
                .text_color(theme.text_muted)
                .child(title.to_owned())
        };
        let mut page = div()
            .id("new-shortcut")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px_6()
            .pb_6()
            .flex()
            .flex_col()
            .child(section("What the keys run"));
        match picked_title {
            None => {
                page = page.child(div().flex().child(controls::field(
                    &search,
                    None,
                    search.read(cx).is_focused(window),
                    false,
                    Some("search"),
                    theme,
                )));
                let mut list = div().pt_2().flex().flex_col().gap(px(2.));
                let hover = theme.hover;
                for (n, (spec, args, title)) in self.matching_commands(cx).into_iter().enumerate() {
                    let name = spec.name.to_string();
                    list = list.child(
                        div()
                            .id(SharedString::from(format!("new-command-{n}")))
                            .h(px(30.))
                            .px_2()
                            .flex()
                            .items_center()
                            .gap_2()
                            .rounded_md()
                            .cursor_pointer()
                            .hover(move |s| s.bg(hover))
                            .child(div().flex_1().min_w_0().truncate().child(title.clone()))
                            .child(div().text_xs().text_color(theme.text_muted).child(name))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.pick_command(&spec, args.clone(), title.clone(), window, cx);
                            })),
                    );
                }
                page = page.child(list);
            }
            Some(title) => {
                let change = self.handle("new-change", cx);
                self.stop(&change, None, None);
                page = page.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().flex_1().min_w_0().truncate().child(title))
                        .child(
                            controls::button(
                                "new-change",
                                "Another command",
                                None,
                                ButtonStyle::Plain,
                                &change,
                                theme,
                            )
                            .on_click(cx.listener(
                                |this, _, window, cx| {
                                    this.keys.editor = None;
                                    if let Some(new) = this.keys.new.as_mut() {
                                        new.picked = None;
                                        new.changed();
                                    }
                                    this.focus_new_shortcut(window, cx);
                                    cx.notify();
                                },
                            )),
                        ),
                );
                page = self.new_shortcut_fields(page, theme, window, cx);
                page = page.child(section("Where they work"));
                let all = places();
                let open = self.handle("new-place", cx);
                self.stop(&open, None, None);
                let mut chooser = div().flex().flex_wrap().gap_1();
                if places_open {
                    for (i, (_, label)) in all.iter().enumerate() {
                        chooser = chooser.child(
                            controls::segment(
                                SharedString::from(format!("new-place-{i}")),
                                *label,
                                i == place,
                                theme,
                            )
                            .border_1()
                            .border_color(theme.border)
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    if let Some(new) = this.keys.new.as_mut() {
                                        new.place = i;
                                        new.places_open = false;
                                    }
                                    cx.notify();
                                },
                            )),
                        );
                    }
                } else {
                    chooser = chooser.child(
                        controls::dropdown(
                            "new-place",
                            all.get(place).map_or("Anywhere", |p| p.1),
                            &open,
                            theme,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(new) = this.keys.new.as_mut() {
                                new.places_open = true;
                            }
                            cx.notify();
                        })),
                    );
                }
                page = page.child(chooser);
                if all.get(place).and_then(|p| p.0) == Some("pane") {
                    let mut conditions = div().pt_2().flex().flex_wrap().gap_1();
                    for flag in FLAGS {
                        let on = flags.contains(&flag);
                        conditions = conditions.child(
                            controls::segment(
                                SharedString::from(format!("new-flag-{flag}")),
                                keymap::condition(flag),
                                on,
                                theme,
                            )
                            .border_1()
                            .border_color(theme.border)
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    if let Some(new) = this.keys.new.as_mut() {
                                        if let Some(at) = new.flags.iter().position(|f| *f == flag)
                                        {
                                            new.flags.remove(at);
                                        } else {
                                            new.flags.push(flag);
                                        }
                                    }
                                    cx.notify();
                                },
                            )),
                        );
                    }
                    page = page.child(conditions);
                }
                page = page.child(section("The keys"));
                if self
                    .keys
                    .editor
                    .as_ref()
                    .is_some_and(|editor| editor.target == Target::New)
                {
                    page = page
                        .child(self.key_recorder(theme, window, cx))
                        .child(div().pt_2().child(self.key_notes(theme, cx)));
                } else {
                    let press = self.handle("new-press", cx);
                    self.stop(&press, None, None);
                    page = page.child(
                        div().flex().child(
                            controls::button(
                                "new-press",
                                "Press the keys…",
                                Some("keyboard"),
                                ButtonStyle::Plain,
                                &press,
                                theme,
                            )
                            .on_click(cx.listener(
                                |this, _, window, cx| {
                                    this.open_key_editor(Target::New, window, cx);
                                },
                            )),
                        ),
                    );
                    if let Err(why) = self.new_shortcut_args(cx) {
                        page = page.child(
                            div()
                                .pt_2()
                                .text_xs()
                                .text_color(theme.text_muted)
                                .child(why),
                        );
                    }
                }
            }
        }
        let cancel = self.handle("new-cancel", cx);
        self.stop(&cancel, None, None);
        let save = self.handle("new-save", cx);
        self.stop(&save, None, None);
        page = page.child(
            div()
                .pt_5()
                .flex()
                .items_center()
                .gap_2()
                .child(div().flex_1())
                .child(
                    controls::button(
                        "new-cancel",
                        "Cancel",
                        None,
                        ButtonStyle::Plain,
                        &cancel,
                        theme,
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.keys.new = None;
                        this.keys.editor = None;
                        window.focus(&this.keys.focus);
                        cx.notify();
                    })),
                )
                .child(
                    controls::button("new-save", "Save", None, ButtonStyle::Plain, &save, theme)
                        .on_click(cx.listener(|this, _, window, cx| {
                            if this.keys.editor.is_none() {
                                this.open_key_editor(Target::New, window, cx);
                            } else {
                                this.save_key_editor(window, cx);
                            }
                        })),
                ),
        );
        page.into_any_element()
    }

    /// The fields of the command's arguments, under its title.
    fn new_shortcut_fields(
        &mut self,
        mut page: gpui::Stateful<gpui::Div>,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        /// What a field shows, apart from the form it is in.
        struct Shown {
            name: String,
            description: String,
            required: bool,
            input: Option<Entity<TextInput>>,
            choices: Option<Vec<String>>,
            switch: Option<bool>,
            chosen: Option<usize>,
        }
        let shown: Vec<Shown> = self
            .keys
            .new
            .as_ref()
            .and_then(|new| new.picked.as_ref())
            .map(|picked| {
                picked
                    .fields
                    .iter()
                    .map(|field| Shown {
                        name: field.name.clone(),
                        description: field.description.clone(),
                        required: field.required,
                        input: field.input.clone(),
                        choices: match &field.kind {
                            Kind::Choice(values) => Some(values.clone()),
                            _ => None,
                        },
                        switch: matches!(field.kind, Kind::Switch).then_some(field.on),
                        chosen: field.chosen,
                    })
                    .collect()
            })
            .unwrap_or_default();
        if shown.is_empty() {
            return page;
        }
        page = page.child(
            div()
                .pt_4()
                .pb_2()
                .text_xs()
                .text_color(theme.text_muted)
                .child("Its arguments"),
        );
        for (i, one) in shown.into_iter().enumerate() {
            let mut row = div().py_1().flex().flex_col().gap_1().child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().text_sm().child(one.name))
                    .when(!one.required, |d| {
                        d.child(
                            div()
                                .text_xs()
                                .text_color(theme.text_muted)
                                .child("optional"),
                        )
                    }),
            );
            if let Some(input) = one.input {
                let focused = input.read(cx).is_focused(window);
                self.stop(&input.focus_handle(cx), None, None);
                row = row.child(
                    div()
                        .flex()
                        .child(controls::field(&input, None, focused, false, None, theme)),
                );
            } else if let Some(choices) = one.choices {
                let mut chooser = div().flex().flex_wrap().gap_1();
                for (n, value) in choices.into_iter().enumerate() {
                    chooser = chooser.child(
                        controls::segment(
                            SharedString::from(format!("new-arg-{i}-{n}")),
                            value,
                            one.chosen == Some(n),
                            theme,
                        )
                        .border_1()
                        .border_color(theme.border)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(field) = field_at(this, i) {
                                field.chosen = (field.chosen != Some(n)).then_some(n);
                            }
                            if let Some(new) = &this.keys.new {
                                new.changed();
                            }
                            cx.notify();
                        })),
                    );
                }
                row = row.child(chooser);
            } else if let Some(on) = one.switch {
                let focus = self.handle(&format!("new-arg-{i}"), cx);
                self.stop(&focus, None, None);
                row = row.child(div().flex().child(
                    controls::switch(format!("new-arg-{i}"), on, &focus, theme).on_click(
                        cx.listener(move |this, _, _, cx| {
                            if let Some(field) = field_at(this, i) {
                                field.on = !field.on;
                            }
                            if let Some(new) = &this.keys.new {
                                new.changed();
                            }
                            cx.notify();
                        }),
                    ),
                ));
            }
            if !one.description.is_empty() {
                row = row.child(
                    div()
                        .text_xs()
                        .text_color(theme.text_muted)
                        .child(one.description),
                );
            }
            page = page.child(row);
        }
        page
    }
}
