//! A form a plugin asks the user to fill (`App::ask_form`, ADR-086): a
//! server's address and name, a cloud's keys, what kind of storage to add.
//! The plugin describes its fields (`skwir_core::form`); this draws them
//! and answers what they hold to the plugin alone, never through a
//! command's arguments. One at a time, the oldest first, once no other
//! dialog holds the keyboard (`Shell::ask_forms`).
//!
//! Enter answers the form from its fields (or picks the choice a list
//! highlights), Escape and Cancel give no answer, Tab walks the fields and
//! buttons. A field folded under More Options is shown once unfolded (a
//! plugin that wants a field in sight leaves it unfolded: core's `Field`).
//! A list (a long choice, a pick) moves with the arrows, filtered by what
//! is typed: a choice unfolded, else the form's pick. A pick keeps its own
//! highlight, its value, whatever a choice's list does meanwhile. A file
//! to write is one line, folder and name, completed as the address field
//! completes a path (`save`).

mod save;

use crate::sizes::TextSizes;
use crate::theme::Chosen;
use gpui::{
    AnyElement, Context, Entity, FocusHandle, SharedString, Subscription, Window, div, prelude::*,
    relative,
};
use skwir_core::form::{Answer, Asked, Choice, Field, Form, Kind, Mark, Picture, Values};

use crate::controls::{self, ButtonStyle};
use crate::input::{InputEvent, TextInput};
use crate::keymap;
use crate::modal::{self, FocusReturn, ListMove, Picker};
use crate::shell::Shell;
use crate::theme::Theme;

/// What a field is drawn with.
enum Control {
    /// Text, a number, a secret.
    Input(Entity<TextInput>),
    Toggle {
        on: bool,
        focus: FocusHandle,
    },
    /// A choice: the value chosen, and the field typed in where any text
    /// may be chosen (`open`), else the dropdown's focus.
    Choice {
        value: String,
        focus: FocusHandle,
        input: Option<Entity<TextInput>>,
    },
    /// A pick from a long list: the filter, the choices it leaves, and
    /// the one highlighted, which is its value.
    Pick {
        input: Entity<TextInput>,
        shown: Vec<usize>,
        picker: Picker,
    },
    Link {
        focus: FocusHandle,
    },
    /// A few choices side by side: the value chosen, and the row's focus
    /// (Up and Down, Space and Enter change it).
    Row {
        value: String,
        focus: FocusHandle,
    },
    /// A file to write (`save`): its text, its list's button, the name
    /// put there (the one its `names` give, or one kept as a folder was
    /// chosen), which a folder chosen keeps and the form's fields change
    /// while it is untouched, and the text put there with it: edited
    /// since, its list completes what was typed.
    Save {
        input: Entity<TextInput>,
        focus: FocusHandle,
        settled: String,
        put: String,
    },
}

/// A row of a list as drawn: a choice's, a folder's; `id` known by its
/// element, whichever row it stands at.
pub(crate) struct Item {
    id: usize,
    title: String,
    detail: String,
    section: String,
}

/// The form shown.
pub struct FormState {
    pub id: u64,
    form: Form,
    controls: Vec<Control>,
    /// More Options unfolded.
    more: bool,
    more_focus: FocusHandle,
    /// The choice whose list is unfolded under it, by field.
    open: Option<usize>,
    /// The highlight in the unfolded choice's list (a pick keeps its own).
    picker: Picker,
    /// The unfolded list is of what was typed in a choice that takes any
    /// text: no row is highlighted, Enter takes the text as typed, and the
    /// arrows go into the list. Typing `~/` in a field of folders saves
    /// there, never in the first folder offered that matches it.
    typed: bool,
    /// Why the answer was refused (a field needed is empty).
    refusal: Option<String>,
    /// The folders a Save field read, and the read under way.
    read: save::Read,
    reading: Option<skwir_core::CancelToken>,
    /// Where a Save field's popover is scrolled (`save::popover`).
    list_scroll: gpui::ScrollHandle,
    /// A Save field's popover lists the asker's folders, asked for (its
    /// button, Down), rather than what completes its text.
    offered: bool,
    back_focus: FocusHandle,
    cancel_focus: FocusHandle,
    accept_focus: FocusHandle,
    scroll: gpui::ScrollHandle,
    give_back: FocusReturn,
    _subs: Vec<Subscription>,
}

/// Rows of an unfolded choice shown at once.
const CHOICE_ROWS: usize = 8;

/// Rows of a pick shown at once: two lines each, the form's buttons in
/// sight below them.
const PICK_ROWS: usize = 8;

impl FormState {
    /// What Tab walks: the fields shown, More Options, Back, Cancel, the
    /// answer (`Shell::walk_dialog`).
    pub(crate) fn walk(&self, cx: &gpui::App) -> Vec<FocusHandle> {
        use gpui::Focusable as _;
        let mut walk = Vec::new();
        if self.form.waiting.is_none() {
            let values = self.values(cx);
            for (field, control) in self.form.fields.iter().zip(&self.controls) {
                if !self.shows(field, &values) {
                    continue;
                }
                walk.push(match control {
                    Control::Input(input)
                    | Control::Pick { input, .. }
                    | Control::Save { input, .. }
                    | Control::Choice {
                        input: Some(input), ..
                    } => input.read(cx).focus_handle(cx),
                    Control::Toggle { focus, .. }
                    | Control::Choice { focus, .. }
                    | Control::Row { focus, .. }
                    | Control::Link { focus } => focus.clone(),
                });
            }
            if self.folded() > 0 {
                walk.push(self.more_focus.clone());
            }
        } else {
            for (field, control) in self.form.fields.iter().zip(&self.controls) {
                if let (Kind::Link, Control::Link { focus }) = (&field.kind, control) {
                    walk.push(focus.clone());
                }
            }
        }
        if self.form.back && self.form.waiting.is_none() {
            walk.push(self.back_focus.clone());
        }
        walk.push(self.cancel_focus.clone());
        if self.form.waiting.is_none() {
            walk.push(self.accept_focus.clone());
        }
        walk
    }

    /// Whether `field` is drawn while the fields hold `values`: any not
    /// folded under More Options, and those once unfolded, while what it
    /// is shown with holds (`Field::when`).
    fn shows(&self, field: &Field, values: &Values) -> bool {
        (!field.advanced || self.more) && field.shown_with(values)
    }

    /// How many fields More Options folds.
    fn folded(&self) -> usize {
        self.form.fields.iter().filter(|f| f.advanced).count()
    }

    /// What the fields hold now.
    fn values(&self, cx: &gpui::App) -> Values {
        let mut values = Values::default();
        for (field, control) in self.form.fields.iter().zip(&self.controls) {
            let value = match control {
                Control::Input(input) => {
                    let text = input.read(cx).text();
                    // Spaces can be part of a secret.
                    if field.secret() {
                        text.to_owned()
                    } else {
                        text.trim().to_owned()
                    }
                }
                Control::Toggle { on, .. } => on.to_string(),
                Control::Choice {
                    input: Some(input), ..
                } => input.read(cx).text().trim().to_owned(),
                Control::Choice { value, .. } | Control::Row { value, .. } => value.clone(),
                Control::Save { input, settled, .. } => {
                    FormState::save_value(input.read(cx).text(), settled)
                }
                Control::Pick { shown, picker, .. } => shown
                    .get(picker.highlighted)
                    .and_then(|&at| choices(field).get(at))
                    .map(|choice| choice.value.clone())
                    .unwrap_or_default(),
                Control::Link { .. } => field.value.clone(),
            };
            values.set(field.key.clone(), value);
        }
        values
    }

    /// The field whose list the arrows and Enter act on: a choice
    /// unfolded, else the form's pick.
    fn listed(&self) -> Option<usize> {
        self.open.or_else(|| {
            self.controls
                .iter()
                .position(|c| matches!(c, Control::Pick { .. }))
        })
    }

    /// The choices field `at` lists, as indices of its choices: a pick's
    /// those its filter leaves, a choice's those that match what is typed
    /// in it. A choice that may take any text holds its value as text: as
    /// given or chosen, nothing typed since, it lists every choice, so
    /// that unfolding it offers the others.
    fn shown_of(&self, at: usize, cx: &gpui::App) -> Option<Vec<usize>> {
        let field = self.form.fields.get(at)?;
        match self.controls.get(at)? {
            Control::Pick { shown, .. } => Some(shown.clone()),
            Control::Choice { input, value, .. } => {
                let typed = input
                    .as_ref()
                    .map(|input| input.read(cx).text().to_owned())
                    .filter(|typed| typed != value)
                    .unwrap_or_default();
                Some(matching(choices(field), &typed))
            }
            Control::Save { .. } => Some((0..self.save_rows(at, cx).len()).collect()),
            _ => None,
        }
    }

    /// The highlight in field `at`'s list: a pick's own, else the
    /// unfolded choice's.
    fn picker_of(&mut self, at: usize) -> &mut Picker {
        match self.controls.get_mut(at) {
            Some(Control::Pick { picker, .. }) => picker,
            _ => &mut self.picker,
        }
    }

    fn highlight_of(&self, at: usize) -> Picker {
        match self.controls.get(at) {
            Some(Control::Pick { picker, .. }) => *picker,
            _ => self.picker,
        }
    }

    /// The row of field `at`'s list Enter chooses: none while it lists
    /// what was typed in a choice that takes any text (`typed`).
    fn highlighted(&self, at: usize) -> Option<usize> {
        match self.controls.get(at) {
            Some(Control::Pick { picker, .. }) => Some(picker.highlighted),
            _ => (!self.typed).then_some(self.picker.highlighted),
        }
    }
}

/// The choices of a choice, a pick or a row.
fn choices(field: &Field) -> &[Choice] {
    field.choices()
}

/// Which of `choices` match `typed`, best first: all, in their order, for
/// nothing typed.
fn matching(choices: &[Choice], typed: &str) -> Vec<usize> {
    modal::fuzzy(choices, typed, |c| {
        format!("{} {} {}", c.shown(), c.value, c.detail)
    })
}

impl Shell {
    /// Show the forms plugins asked (`forms`) once no other dialog holds
    /// the keyboard, one at a time, the first first; let go of one whose
    /// asker stopped waiting (a sign-in done, a connection cancelled).
    pub fn ask_forms(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let app = std::sync::Arc::clone(&self.app);
        self.forms.retain(|asked| app.form_waits(asked.id));
        if let Some(id) = self.form.as_ref().map(|f| f.id)
            && !app.form_waits(id)
        {
            self.close_form(window, cx);
        }
        if self.form.is_some() || self.modal_open_beside_settings() || self.review_focus.is_some() {
            return;
        }
        if let Some(asked) = self.forms.pop_front() {
            self.open_form(asked, window, cx);
        }
    }

    fn open_form(&mut self, asked: Asked, window: &mut Window, cx: &mut Context<Self>) {
        let give_back = FocusReturn::capture(window, cx);
        let mut subs = Vec::new();
        let mut controls = Vec::new();
        let mut first: Option<FocusHandle> = None;
        let waiting = asked.form.waiting.is_some();
        for (at, field) in asked.form.fields.iter().enumerate() {
            let control = match &field.kind {
                Kind::Text | Kind::Integer { .. } | Kind::Secret { .. } => {
                    let placeholder = match field.kind {
                        Kind::Secret { kept: true } => "Kept: type another to replace it",
                        _ => field.placeholder.as_str(),
                    };
                    let input = self.form_input(field, placeholder, at, &mut subs, window, cx);
                    Control::Input(input)
                }
                Kind::Toggle => Control::Toggle {
                    on: field.value == "true",
                    focus: cx.focus_handle(),
                },
                Kind::Choice { open, .. } => Control::Choice {
                    value: field.value.clone(),
                    focus: cx.focus_handle(),
                    input: open.then(|| self.form_input(field, "", at, &mut subs, window, cx)),
                },
                Kind::Pick { choices } => {
                    let input = self.form_input(field, "Type to filter", at, &mut subs, window, cx);
                    input.update(cx, |i, cx| i.set_text(String::new(), cx));
                    // A pick starts on the choice the asker gave.
                    let mut picker = Picker::default();
                    if let Some(at) = choices.iter().position(|c| c.value == field.value) {
                        picker.go(ListMove::First, choices.len());
                        for _ in 0..at {
                            picker.step(1, choices.len());
                        }
                    }
                    Control::Pick {
                        input,
                        shown: (0..choices.len()).collect(),
                        picker,
                    }
                }
                Kind::Link => Control::Link {
                    focus: cx.focus_handle(),
                },
                Kind::Row { choices } => Control::Row {
                    // One is always chosen: the asker's, else the first.
                    value: if choices.iter().any(|c| c.value == field.value) {
                        field.value.clone()
                    } else {
                        choices.first().map(|c| c.value.clone()).unwrap_or_default()
                    },
                    focus: cx.focus_handle(),
                },
                // Its name put there once every field is (`save_open`).
                Kind::Save { .. } => {
                    use gpui::Focusable as _;
                    let input = self.form_input(
                        field,
                        field.placeholder.as_str(),
                        at,
                        &mut subs,
                        window,
                        cx,
                    );
                    // Its popover folded, its name given back, once it is
                    // left.
                    let text = input.read(cx).focus_handle(cx);
                    subs.push(cx.on_blur(&text, window, move |this, window, cx| {
                        this.save_left(at, window, cx);
                    }));
                    Control::Save {
                        input,
                        focus: cx.focus_handle(),
                        settled: String::new(),
                        put: String::new(),
                    }
                }
            };
            if first.is_none() && !field.advanced && !waiting {
                use gpui::Focusable as _;
                first = Some(match &control {
                    Control::Input(input)
                    | Control::Pick { input, .. }
                    | Control::Save { input, .. }
                    | Control::Choice {
                        input: Some(input), ..
                    } => input.read(cx).focus_handle(cx),
                    Control::Toggle { focus, .. }
                    | Control::Choice { focus, .. }
                    | Control::Row { focus, .. }
                    | Control::Link { focus } => focus.clone(),
                });
            }
            controls.push(control);
        }
        let state = FormState {
            id: asked.id,
            refusal: asked.form.error.clone(),
            form: asked.form,
            controls,
            more: false,
            more_focus: cx.focus_handle(),
            open: None,
            picker: Picker::default(),
            typed: false,
            read: save::Read::new(),
            reading: None,
            list_scroll: gpui::ScrollHandle::new(),
            offered: false,
            back_focus: cx.focus_handle(),
            cancel_focus: cx.focus_handle(),
            accept_focus: cx.focus_handle(),
            scroll: gpui::ScrollHandle::new(),
            give_back,
            _subs: subs,
        };
        // A form with no field to fill (a question, a wait): its answer has
        // the keyboard, or its Cancel while it waits.
        let focus = first.unwrap_or_else(|| {
            if state.form.waiting.is_some() {
                state.cancel_focus.clone()
            } else {
                state.accept_focus.clone()
            }
        });
        self.form = Some(state);
        self.save_open(window, cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    /// The text field of `field`, the `at`th, holding its value: Enter
    /// answers the form (or picks from its list), Escape cancels it.
    fn form_input(
        &self,
        field: &Field,
        placeholder: &str,
        at: usize,
        subs: &mut Vec<Subscription>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<TextInput> {
        let input = cx.new(|cx| {
            let mut input = TextInput::new(placeholder.to_owned(), cx);
            // Named by its label, as assistive technology reads it.
            input.set_label(field.title.clone());
            input.set_masked(field.secret());
            if !field.secret() {
                input.set_text(field.value.clone(), cx);
            }
            input
        });
        subs.push(cx.subscribe_in(
            &input,
            window,
            move |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::Submit => this.form_enter(Some(at), window, cx),
                InputEvent::Cancel => this.form_cancel(window, cx),
                InputEvent::Changed => this.form_typed(at, window, cx),
            },
        ));
        input
    }

    /// What was typed in field `at` changed: its list is filtered again
    /// (a file to write's folder read, to complete).
    fn form_typed(&mut self, at: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.form.as_mut() else {
            return;
        };
        form.refusal = None;
        let Some(field) = form.form.fields.get(at) else {
            return;
        };
        match form.controls.get_mut(at) {
            Some(Control::Pick {
                input,
                shown,
                picker,
            }) => {
                *shown = matching(choices(field), input.read(cx).text());
                *picker = Picker::default();
            }
            Some(Control::Choice { input: Some(_), .. }) => {
                form.open = Some(at);
                form.picker = Picker::default();
                form.typed = true;
            }
            // What the window put there itself (a folder chosen, the
            // folder above) is no typing.
            Some(Control::Save { input, put, .. }) if input.read(cx).text() == put.as_str() => {}
            Some(Control::Save { .. }) => {
                form.open = Some(at);
                form.picker = Picker::default();
                form.typed = true;
                form.offered = false;
                form.list_scroll.scroll_to_item(0);
                self.save_read(at, window, cx);
            }
            _ => {}
        }
        cx.notify();
    }

    /// Enter: in a field whose list is shown, its highlighted choice;
    /// elsewhere, or with none highlighted (what was typed in a choice
    /// that takes any text), the form's answer.
    fn form_enter(&mut self, at: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.form.as_mut() else {
            return;
        };
        if form.form.waiting.is_some() {
            return;
        }
        if let (Some(at), Some(open)) = (at, form.open)
            && at == open
        {
            if form.highlighted(at).is_some() {
                self.form_choose(None, window, cx);
                return;
            }
            form.open = None;
            form.typed = false;
        }
        self.form_answer(false, window, cx);
    }

    /// Choose from a list: `clicked`, a row of what field's list shows,
    /// else the highlighted choice of the list the keys act on
    /// (`FormState::listed`). A pick answers the form with it.
    fn form_choose(
        &mut self,
        clicked: Option<(usize, usize)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(form) = self.form.as_mut() else {
            return;
        };
        let Some(at) = clicked.map(|(at, _)| at).or_else(|| form.listed()) else {
            return;
        };
        if let Some(Control::Save { .. }) = form.controls.get(at) {
            if let Some((_, row)) = clicked {
                form.picker.highlighted = row;
                form.typed = false;
            }
            let rows = form.save_rows(at, cx);
            if let Some(row) = form.highlighted(at).and_then(|row| rows.get(row)) {
                self.save_choose(at, row, window, cx);
            }
            return;
        }
        let Some(shown) = form.shown_of(at, cx) else {
            return;
        };
        if let Some((_, row)) = clicked {
            form.picker_of(at).highlighted = row;
            form.typed = false;
        }
        let Some(&chosen) = form.highlighted(at).and_then(|row| shown.get(row)) else {
            return;
        };
        let Some(field) = form.form.fields.get(at) else {
            return;
        };
        let Some(choice) = choices(field).get(chosen).cloned() else {
            return;
        };
        match form.controls.get_mut(at) {
            Some(Control::Pick { .. }) => {
                self.form_answer(false, window, cx);
                return;
            }
            Some(Control::Choice {
                value,
                input,
                focus,
            }) => {
                value.clone_from(&choice.value);
                if let Some(input) = input {
                    input.update(cx, |i, cx| i.set_text(choice.value, cx));
                } else {
                    window.focus(focus, cx);
                }
            }
            _ => {}
        }
        form.open = None;
        form.typed = false;
        form.refusal = None;
        self.save_follow(cx);
        cx.notify();
    }

    /// Unfold or fold the list of choice `at`.
    fn form_unfold(&mut self, at: usize, cx: &mut Context<Self>) {
        let Some(form) = self.form.as_mut() else {
            return;
        };
        form.typed = false;
        if form.open == Some(at) {
            form.open = None;
        } else {
            form.open = Some(at);
            // Highlighted on the value chosen, where the list shows it.
            let value = match form.controls.get(at) {
                Some(Control::Choice { value, .. }) => value.clone(),
                _ => String::new(),
            };
            form.picker = Picker::default();
            if let Some(field) = form.form.fields.get(at) {
                let all = choices(field);
                if let Some(row) = all.iter().position(|c| c.value == value) {
                    for _ in 0..row {
                        form.picker.step(1, all.len());
                    }
                }
            }
        }
        cx.notify();
    }

    /// `ui.list_move` while a form shows a list, or a row of choices has
    /// the keyboard (Up the one before, Down the one after): whether it
    /// took the move.
    pub(crate) fn form_move(
        &mut self,
        mv: ListMove,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(form) = self.form.as_mut() else {
            return false;
        };
        let row = form.open.is_none().then(|| {
            form.controls
                .iter()
                .position(|c| matches!(c, Control::Row { focus, .. } if focus.is_focused(window)))
        });
        if let Some(Some(at)) = row {
            let step = match mv {
                ListMove::By(delta) | ListMove::Page(delta) => delta,
                ListMove::First => isize::MIN,
                ListMove::Last => isize::MAX,
            };
            self.form_row_move(at, step, false, cx);
            return true;
        }
        // Down in a Save field whose popover is folded, or shows nothing
        // (nothing completes what was typed), unfolds its folders, as its
        // button does.
        if let Some(at) = form.save_focused(window, cx)
            && (form.listed().is_none() || form.save_rows(at, cx).is_empty())
        {
            form.open = Some(at);
            form.typed = false;
            form.offered = true;
            form.picker = Picker::default();
            form.list_scroll.scroll_to_item(0);
            cx.notify();
            return true;
        }
        let Some(at) = form.listed() else {
            return false;
        };
        let Some(shown) = form.shown_of(at, cx) else {
            return false;
        };
        if form.highlighted(at).is_none() {
            // From what was typed into the list: Down to its first row,
            // Up to its last; nothing to go to when nothing matches.
            if shown.is_empty() {
                return true;
            }
            form.typed = false;
            let into = match mv {
                ListMove::By(delta) | ListMove::Page(delta) if delta < 0 => ListMove::Last,
                _ => ListMove::First,
            };
            form.picker_of(at).go(into, shown.len());
        } else {
            form.picker_of(at).go(mv, shown.len());
        }
        // A Save field's popover scrolls: the row highlighted in view.
        if let (Some(Control::Save { .. }), Some(row)) =
            (form.controls.get(at), form.highlighted(at))
        {
            form.list_scroll.scroll_to_item(row);
        }
        cx.notify();
        true
    }

    /// Answer the form with what its fields hold, or Back: refused (a
    /// field needed is empty), it stays, saying why.
    fn form_answer(&mut self, back: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.form.as_mut() else {
            return;
        };
        let values = form.values(cx);
        // A path no file can be saved at is said under its field, and
        // holds the answer back.
        if !back && form.blocked(cx).is_some() {
            return;
        }
        if !back && let Some(refusal) = form.form.refusal(&values) {
            // What it names may be folded: shown.
            form.more = true;
            form.refusal = Some(refusal);
            cx.notify();
            return;
        }
        let id = form.id;
        let answer = if back {
            Answer::Back(values)
        } else {
            Answer::Accepted(values)
        };
        // Told before the form closes, which would tell it none.
        self.app.answer_form(id, Some(answer));
        self.close_form(window, cx);
    }

    /// Close the form unanswered: Escape, Cancel, a press outside.
    pub(crate) fn form_cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let open = self.form.as_ref().and_then(|f| f.open);
        if let (Some(form), Some(_)) = (self.form.as_mut(), open) {
            // Escape folds a list first.
            form.open = None;
            form.typed = false;
            cx.notify();
            return;
        }
        self.close_form(window, cx);
    }

    /// A press in the form outside the list unfolded: the list folded.
    fn form_fold(&mut self, cx: &mut Context<Self>) {
        if let Some(form) = self.form.as_mut()
            && form.open.is_some()
        {
            form.open = None;
            form.typed = false;
            form.offered = false;
            cx.notify();
        }
    }

    /// A choice of row `at` clicked: chosen, and a field to type in that it
    /// shows (Pages…'s pages, a PDF's Save as) takes the keyboard, as what
    /// the click asks for next. The arrows on the row move no keyboard.
    fn form_row_click(
        &mut self,
        at: usize,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let shown = |form: &FormState, cx: &gpui::App| -> Vec<bool> {
            let values = form.values(cx);
            form.form
                .fields
                .iter()
                .map(|field| form.shows(field, &values))
                .collect()
        };
        let Some(before) = self.form.as_ref().map(|form| shown(form, cx)) else {
            return;
        };
        self.form_row_choose(at, value, cx);
        let Some(form) = self.form.as_ref() else {
            return;
        };
        let after = shown(form, cx);
        let revealed = (0..after.len()).find_map(|field| {
            if after[field] && !before.get(field).copied().unwrap_or(true) {
                match form.controls.get(field)? {
                    Control::Input(input) | Control::Save { input, .. } => Some(input.clone()),
                    _ => None,
                }
            } else {
                None
            }
        });
        if let Some(input) = revealed {
            use gpui::Focusable as _;
            let focus = input.read(cx).focus_handle(cx);
            window.focus(&focus, cx);
        }
    }

    /// Close the form and give the keyboard back to what had it. Its asker,
    /// if it still waits, hears no answer.
    pub fn close_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(form) = self.form.take() {
            if let Some(reading) = &form.reading {
                reading.cancel();
            }
            self.app.answer_form(form.id, None);
            form.give_back.restore(window, &self.ui().focus, cx);
        }
        cx.notify();
        // The next one waiting, if any.
        self.ask_forms(window, cx);
    }

    fn form_toggle(&mut self, at: usize, cx: &mut Context<Self>) {
        if let Some(Control::Toggle { on, .. }) =
            self.form.as_mut().and_then(|f| f.controls.get_mut(at))
        {
            *on = !*on;
            self.save_follow(cx);
            cx.notify();
        }
    }

    /// Choose `value` in row `at`.
    fn form_row_choose(&mut self, at: usize, value: &str, cx: &mut Context<Self>) {
        if let Some(form) = self.form.as_mut()
            && let Some(Control::Row { value: held, .. }) = form.controls.get_mut(at)
        {
            value.clone_into(held);
            form.refusal = None;
            self.save_follow(cx);
            cx.notify();
        }
    }

    /// Choose the choice `step` after the one row `at` holds (before it
    /// when negative), stopping at its ends; with `cycle`, the next one,
    /// past the last to the first (Space and Enter, as a segmented setting
    /// does).
    fn form_row_move(&mut self, at: usize, step: isize, cycle: bool, cx: &mut Context<Self>) {
        let Some(form) = self.form.as_ref() else {
            return;
        };
        let (Some(field), Some(Control::Row { value, .. })) =
            (form.form.fields.get(at), form.controls.get(at))
        else {
            return;
        };
        let all = choices(field);
        if all.is_empty() {
            return;
        }
        let last = all.len() - 1;
        let now = all.iter().position(|c| c.value == *value).unwrap_or(0);
        let next = if cycle {
            (now + 1) % all.len()
        } else if step < 0 {
            now.saturating_sub(step.unsigned_abs())
        } else {
            now.saturating_add(step.unsigned_abs()).min(last)
        };
        let chosen = all[next].value.clone();
        self.form_row_choose(at, &chosen, cx);
    }

    fn form_more(&mut self, cx: &mut Context<Self>) {
        if let Some(form) = self.form.as_mut() {
            form.more = !form.more;
            cx.notify();
        }
    }
}

pub fn render(shell: &Shell, window: &Window, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let state = shell.form.as_ref()?;
    let theme = cx.global::<Theme>().clone();
    let form = &state.form;
    let values = state.values(cx);
    let mut body = div()
        .id("form-fields")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .track_scroll(&state.scroll)
        .flex()
        .flex_col()
        .gap_3()
        .pr_1();
    if !form.intro.is_empty() {
        body = body.child(
            div()
                .text_body()
                .text_color(theme.text_muted)
                .child(form.intro.clone()),
        );
    }
    if let Some(waiting) = &form.waiting {
        body = body.child(
            div()
                .flex()
                .items_start()
                .gap_2()
                .child(controls::glyph("busy", theme.text_muted, 16.))
                // Wraps, beside its glyph.
                .child(div().flex_1().min_w_0().child(waiting.clone())),
        );
    }
    for (at, (field, control)) in form.fields.iter().zip(&state.controls).enumerate() {
        if form.waiting.is_some() && !matches!(field.kind, Kind::Link) {
            continue;
        }
        if state.shows(field, &values) {
            body = body.child(field_row(
                shell, state, at, field, control, &theme, window, cx,
            ));
        }
    }
    let folded = state.folded();
    if folded > 0 && form.waiting.is_none() {
        let label: SharedString = if state.more {
            "Fewer options".into()
        } else {
            format!("More options ({folded})").into()
        };
        body = body.child(
            div().flex().child(modal::pressed(
                controls::button(
                    "form-more",
                    label,
                    Some(if state.more { "fold" } else { "unfold" }),
                    ButtonStyle::Plain,
                    &state.more_focus,
                    &theme,
                )
                .aria_expanded(state.more),
                |this, _, cx| this.form_more(cx),
                cx,
            )),
        );
    }
    let mut buttons = Vec::new();
    if form.back && form.waiting.is_none() {
        buttons.push(modal::pressed(
            controls::button(
                "form-back",
                "Back",
                None,
                ButtonStyle::Plain,
                &state.back_focus,
                &theme,
            ),
            |this, window, cx| this.form_answer(true, window, cx),
            cx,
        ));
    }
    buttons.push(modal::pressed(
        controls::button(
            "form-cancel",
            "Cancel",
            None,
            ButtonStyle::Plain,
            &state.cancel_focus,
            &theme,
        ),
        Shell::close_form,
        cx,
    ));
    if form.waiting.is_none() {
        let accept = form.accept_with(&values).to_owned();
        // Held back while a path no file can be saved at is typed: said,
        // not offered, as the reason under its field says.
        if let Some(why) = state.blocked(cx) {
            buttons.push(modal::pressed(
                controls::inert_button("form-accept", accept, &state.accept_focus, &theme)
                    .when(shell.a11y, |d| d.aria_description(why)),
                |_, _, _| {},
                cx,
            ));
        } else {
            buttons.push(modal::pressed(
                controls::button(
                    "form-accept",
                    accept,
                    Some("check"),
                    ButtonStyle::Plain,
                    &state.accept_focus,
                    &theme,
                ),
                |this, window, cx| this.form_answer(false, window, cx),
                cx,
            ));
        }
    }
    let hint = if form.waiting.is_some() {
        "Escape to cancel"
    } else {
        "Enter to confirm, Escape to cancel"
    };
    // As tall as the room below its top allows but a tenth: a dialog of
    // pictures (Print…'s) fits a window 800 pixels high whole.
    // Tab completes a folder typed while there is one to complete.
    let mut context = gpui::KeyContext::default();
    context.add(keymap::FORM);
    if state.completes(window, cx) {
        context.add("completes");
    }
    // A press anywhere in it but in the field whose list is unfolded (and
    // that list) folds the list, as a press outside a menu closes it.
    let inside = div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .p_3()
        .gap_3()
        .on_mouse_down(
            gpui::MouseButton::Left,
            cx.listener(|this, _, _, cx| this.form_fold(cx)),
        )
        .child(div().text_title().child(form.title.clone()))
        .child(body)
        .children(state.refusal.clone().map(|refusal| {
            div()
                .id("form-refusal")
                .role(gpui::Role::Alert)
                .text_body()
                .text_color(theme.error)
                .child(refusal)
        }))
        .child(modal::footer(hint, buttons, &theme));
    let panel = modal::panel("form", 600., &theme)
        .when(shell.a11y, |d| d.aria_label(form.title.clone()))
        .key_context(context)
        .max_h(relative(0.9))
        .child(inside);
    Some(modal::modal(panel, 80., 3, Shell::form_cancel, &theme, cx))
}

/// The button that unfolds a text field's list, inside the field at its
/// end, as a combobox draws it: down while the list is folded, up while
/// it is shown.
fn inner_chevron(
    id: SharedString,
    label: &str,
    open: bool,
    focus: &FocusHandle,
    theme: &Theme,
) -> gpui::Stateful<gpui::Div> {
    controls::icon_button(
        id,
        if open { "chevron-up" } else { "chevron" },
        label.to_owned(),
        focus,
        theme,
    )
    // Within the field's frame, as tall as its text allows.
    .size(crate::sizes::sized(controls::HEIGHT - 8.))
    .mr(gpui::px(-4.))
    .aria_expanded(open)
}

/// Field `at`: its label, its control, what it is.
#[allow(clippy::too_many_arguments)] // one field of one form, where it is drawn
fn field_row(
    shell: &Shell,
    state: &FormState,
    at: usize,
    field: &Field,
    control: &Control,
    theme: &Theme,
    window: &Window,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let label = div()
        .text_body()
        .font_weight(gpui::FontWeight::MEDIUM)
        .child(if field.required {
            format!("{} *", field.title)
        } else {
            field.title.clone()
        });
    // A file to write says what its folder read tells, else its help.
    let note = matches!(control, Control::Save { .. })
        .then(|| state.save_note(at, field, cx))
        .flatten();
    // A path no file can be saved at marks its field, as its note says.
    let blocked = note.as_ref().is_some_and(|note| note.blocks);
    let help = match note {
        Some(note) => Some(
            div()
                .text_small()
                // Red as the field's frame where it holds the answer back,
                // a warning's color where it only says something.
                .text_color(if note.blocks {
                    theme.error
                } else if note.warns {
                    theme.warning
                } else {
                    theme.text_muted
                })
                .child(note.text),
        ),
        None => (!field.help.is_empty()).then(|| {
            div()
                .text_small()
                .text_color(theme.text_muted)
                .child(field.help.clone())
        }),
    };
    let mut row = div().flex().flex_col().gap_1();
    match control {
        Control::Input(input) => {
            let focused = input.read(cx).is_focused(window);
            row = row
                .child(label)
                .child(controls::field(input, None, focused, false, None, theme));
        }
        Control::Toggle { on, focus } => {
            let title = field.title.clone();
            row = row.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(modal::pressed(
                        controls::switch(
                            SharedString::from(format!("form-switch-{at}")),
                            *on,
                            focus,
                            theme,
                        )
                        .aria_label(title.clone()),
                        move |this, _, cx| this.form_toggle(at, cx),
                        cx,
                    ))
                    .child(
                        div()
                            .id(SharedString::from(format!("form-switch-label-{at}")))
                            .cursor_pointer()
                            .text_body()
                            .on_click(cx.listener(move |this, _, _, cx| this.form_toggle(at, cx)))
                            .child(title),
                    ),
            );
        }
        Control::Choice {
            value,
            focus,
            input,
        } => {
            let open = state.open == Some(at);
            let shown = choices(field)
                .iter()
                .find(|c| c.value == *value)
                .map_or_else(|| value.clone(), |c| c.shown().to_owned());
            let picker = match input {
                Some(input) => {
                    let focused = input.read(cx).is_focused(window);
                    controls::field(input, None, focused, false, None, theme)
                        .child(modal::pressed(
                            inner_chevron(
                                SharedString::from(format!("form-suggest-{at}")),
                                "Suggestions",
                                open,
                                focus,
                                theme,
                            ),
                            move |this, _, cx| this.form_unfold(at, cx),
                            cx,
                        ))
                        .into_any_element()
                }
                None => modal::pressed(
                    controls::dropdown(
                        SharedString::from(format!("form-choice-{at}")),
                        if shown.is_empty() {
                            "Choose…".to_owned()
                        } else {
                            shown
                        },
                        focus,
                        theme,
                    )
                    .aria_label(field.title.clone())
                    .aria_expanded(open),
                    move |this, window, cx| {
                        if this.form.as_ref().is_some_and(|f| f.open == Some(at)) {
                            this.form_choose(None, window, cx);
                        } else {
                            this.form_unfold(at, cx);
                        }
                    },
                    cx,
                )
                .into_any_element(),
            };
            row = row.child(label).child(picker);
            if open {
                row = row.child(choice_list(shell, state, at, field, CHOICE_ROWS, theme, cx));
            }
        }
        Control::Save { input, focus, .. } => {
            let open = state.open == Some(at);
            let focused = input.read(cx).is_focused(window);
            let rows = if open {
                state.save_rows(at, cx)
            } else {
                Vec::new()
            };
            // The popover, under the line and over what follows; nothing
            // to offer, none, rather than one saying so.
            let popover =
                (!rows.is_empty()).then(|| save::popover(shell, state, at, &rows, theme, cx));
            row = row.child(label).child(
                div()
                    .relative()
                    .flex()
                    .child(
                        controls::field(input, None, focused, blocked, None, theme).child(
                            modal::pressed(
                                inner_chevron(
                                    SharedString::from(format!("form-folders-{at}")),
                                    "Folders",
                                    open,
                                    focus,
                                    theme,
                                ),
                                move |this, window, cx| this.save_unfold(at, window, cx),
                                cx,
                            ),
                        ),
                    )
                    .children(popover),
            );
        }
        Control::Pick { input, .. } => {
            let focused = input.read(cx).is_focused(window);
            row = row
                .child(label)
                .child(controls::field(
                    input,
                    None,
                    focused,
                    false,
                    Some("search"),
                    theme,
                ))
                .child(choice_list(shell, state, at, field, PICK_ROWS, theme, cx));
        }
        Control::Row { value, focus } => {
            row = row
                .child(label)
                .child(choice_row(at, field, value, focus, theme, cx));
        }
        Control::Link { focus } => {
            let url = field.value.clone();
            row = row.child(div().flex().child(modal::pressed(
                controls::button(
                    SharedString::from(format!("form-link-{at}")),
                    field.title.clone(),
                    Some("link"),
                    ButtonStyle::Plain,
                    focus,
                    theme,
                ),
                move |this, _, cx| this.open_address(&url, cx),
                cx,
            )));
        }
    }
    // A press in the field whose list is unfolded, or in its list, leaves
    // the list as it is (`form_fold`).
    row.when(state.open == Some(at), |row| {
        row.on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
    })
    .children(help)
    .into_any_element()
}

/// A row of choices, `value` chosen: cards when one has a picture (the
/// picture over its title and detail), else segments. The row has the
/// keyboard as one: Up and Down choose the one before or after, Space the
/// next; Enter answers the form, as it does from a field; a click
/// chooses one.
fn choice_row(
    at: usize,
    field: &Field,
    value: &str,
    focus: &FocusHandle,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let all = choices(field);
    let id = SharedString::from(format!("form-row-{at}"));
    // Space and Enter reach the focused row as a keyboard click; the
    // pointer's clicks belong to its choices.
    let keyboard = cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
        if let gpui::ClickEvent::Keyboard(key) = event {
            match key.button {
                gpui::KeyboardButton::Space => this.form_row_move(at, 1, true, cx),
                gpui::KeyboardButton::Enter => this.form_enter(None, window, cx),
            }
        }
    });
    // Segments hold a title alone: a choice that says more, or shows it,
    // is a card.
    if all
        .iter()
        .all(|c| c.picture.is_none() && c.detail.is_empty())
    {
        let mut frame = controls::segmented(id, focus, theme)
            .aria_label(field.title.clone())
            .on_click(keyboard);
        for (index, choice) in all.iter().enumerate() {
            let chosen = choice.value.clone();
            let focus = focus.clone();
            frame = frame.child(
                controls::segment(
                    format!("form-segment-{at}-{index}"),
                    choice.shown().to_owned(),
                    choice.value == value,
                    theme,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    window.focus(&focus, cx);
                    this.form_row_click(at, &chosen, window, cx);
                })),
            );
        }
        return div().flex().child(frame).into_any_element();
    }
    let accent = theme.accent;
    let mut frame = div()
        .id(id)
        .role(gpui::Role::RadioGroup)
        .aria_label(field.title.clone())
        .track_focus(focus)
        .flex()
        .gap_2()
        .p(gpui::px(2.))
        .rounded_lg()
        .border_1()
        .border_color(gpui::transparent_black())
        .focus(move |s| s.border_color(accent))
        .on_click(keyboard);
    for (index, choice) in all.iter().enumerate() {
        let selected = choice.value == value;
        let chosen = choice.value.clone();
        let focus = focus.clone();
        let hover = theme.hover;
        let card = div()
            .id(crate::elements::known_by("form-card", (at, index)))
            .role(gpui::Role::RadioButton)
            .aria_label(SharedString::from(choice.shown().to_owned()))
            .aria_toggled(if selected {
                gpui::Toggled::True
            } else {
                gpui::Toggled::False
            })
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .items_center()
            .gap_1()
            .px_2()
            .py_1p5()
            .rounded_md()
            .border_1()
            .cursor_pointer()
            .when(selected, |d| {
                d.border_color(theme.accent)
                    .bg(theme.selection_bg)
                    .text_color(theme.selection_text)
            })
            .when(!selected, |d| {
                d.border_color(theme.border).hover(move |s| s.bg(hover))
            })
            .children(choice.picture.as_ref().map(|picture| drawn(picture, theme)))
            .child(
                div()
                    .text_body()
                    .text_center()
                    .child(choice.shown().to_owned()),
            )
            .when(!choice.detail.is_empty(), |d| {
                d.child(
                    div()
                        .text_small()
                        .text_center()
                        .when(!selected, |d| d.text_color(theme.text_muted))
                        .child(choice.detail.clone()),
                )
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                window.focus(&focus, cx);
                this.form_row_click(at, &chosen, window, cx);
            }));
        frame = frame.child(card);
    }
    frame.into_any_element()
}

/// A choice's picture, drawn as on paper whatever the card's color: its
/// boxes on `bg` outlined in `text_muted`, those shaded (a sheet behind
/// another) on `surface`, what is written in `text`.
fn drawn(picture: &Picture, theme: &Theme) -> AnyElement {
    let px = |n: u16| gpui::px(f32::from(n));
    let mut drawing = div()
        .relative()
        .flex_none()
        .w(px(picture.width))
        .h(px(picture.height));
    for mark in &picture.marks {
        drawing = drawing.child(match mark {
            Mark::Box {
                x,
                y,
                width,
                height,
                shaded,
            } => div()
                .absolute()
                .left(px(*x))
                .top(px(*y))
                .w(px(*width))
                .h(px(*height))
                .border_1()
                .border_color(theme.text_muted)
                .bg(if *shaded { theme.surface } else { theme.bg }),
            Mark::Text {
                x,
                y,
                width,
                height,
                text,
            } => div()
                .absolute()
                .left(px(*x))
                .top(px(*y))
                .w(px(*width))
                .h(px(*height))
                .flex()
                .items_center()
                .justify_center()
                .text_size(gpui::px(10.))
                .line_height(gpui::px(12.))
                .text_color(theme.text)
                .child(text.clone()),
        });
    }
    drawing.into_any_element()
}

/// The list of a choice unfolded, or of a pick: what matches what was
/// typed, `rows` at most in sight, the highlighted one marked; a click
/// chooses.
#[allow(clippy::too_many_arguments)] // a list of one field of one form
fn choice_list(
    shell: &Shell,
    state: &FormState,
    at: usize,
    field: &Field,
    rows: usize,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let all = choices(field);
    let items: Vec<Item> = state
        .shown_of(at, cx)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|index| Some((index, all.get(index)?)))
        .map(|(id, choice)| Item {
            id,
            title: choice.shown().to_owned(),
            detail: choice.detail.clone(),
            section: choice.section.clone(),
        })
        .collect();
    list_of(shell, state, at, &items, rows, theme, cx)
}

/// The rows `items` of field `at`'s list, `rows` at most in sight, the
/// highlighted one marked; a click chooses.
fn list_of(
    shell: &Shell,
    state: &FormState,
    at: usize,
    items: &[Item],
    rows: usize,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let shown = items;
    let picker = state.highlight_of(at);
    let chosen_row = state.highlighted(at);
    let field_focus = match state.controls.get(at) {
        Some(
            Control::Pick { input, .. }
            | Control::Save { input, .. }
            | Control::Choice {
                input: Some(input), ..
            },
        ) => {
            use gpui::Focusable as _;
            input.read(cx).focus_handle(cx)
        }
        Some(Control::Choice { focus, .. }) => focus.clone(),
        _ => state.cancel_focus.clone(),
    };
    let (start, end) = {
        let top = picker.top.min(shown.len());
        // The highlight kept in sight within these rows.
        let top = if picker.highlighted >= top + rows {
            picker.highlighted + 1 - rows
        } else {
            top.min(picker.highlighted)
        };
        (top, (top + rows).min(shown.len()))
    };
    let mut list = modal::options("form-options", "Choices", shown.len(), shell.a11y)
        .flex()
        .flex_col()
        .py_1()
        .rounded_md()
        .border_1()
        .border_color(theme.border)
        .bg(theme.bg);
    if shown.is_empty() {
        list = list.child(
            div()
                .px_2()
                .py_1()
                .text_body()
                .text_color(theme.text_muted)
                .child("Nothing matches"),
        );
    }
    let mut section = None;
    for (row, choice) in shown.iter().enumerate().take(end).skip(start) {
        if !choice.section.is_empty() && section != Some(&choice.section) {
            section = Some(&choice.section);
            list = list.child(
                div()
                    .px_2()
                    .pt_1()
                    .text_small()
                    .text_color(theme.text_muted)
                    .child(choice.section.clone()),
            );
        }
        let highlighted = chosen_row == Some(row);
        let named = shell.a11y.then(|| SharedString::from(choice.title.clone()));
        let item = div()
            // Known by its choice: filtered, another stands at its row.
            .id(crate::elements::known_by("form-option", (at, choice.id)))
            .px_2()
            .py_1()
            .mx_1()
            .rounded_sm()
            .flex()
            .flex_col()
            .cursor_pointer()
            .when(highlighted, |d| d.chosen(theme))
            .when(!highlighted, |d| d.hover(|s| s.bg(theme.hover)))
            .child(div().text_body().child(choice.title.clone()))
            .when(!choice.detail.is_empty(), |d| {
                d.child(
                    div()
                        .text_small()
                        .when(!highlighted, |d| d.text_color(theme.text_muted))
                        .truncate()
                        .child(choice.detail.clone()),
                )
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                this.form_choose(Some((at, row)), window, cx);
            }));
        list = list.child(modal::option(item, named, row, highlighted, &field_focus));
    }
    if end < shown.len() || start > 0 {
        list = list.child(
            div()
                .px_2()
                .text_body()
                .text_color(theme.text_muted)
                .child(format!(
                    "{}–{} of {}: the arrows move further",
                    start + 1,
                    end,
                    shown.len()
                )),
        );
    }
    list.into_any_element()
}
