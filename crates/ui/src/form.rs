//! A form a plugin asks the user to fill (`App::ask_form`, ADR-086): a
//! server's address and name, a cloud's keys, what kind of storage to add.
//! The plugin describes its fields (`skwir_core::form`); this draws them
//! and answers what they hold to the plugin alone, never through a
//! command's arguments. One at a time, the oldest first, once no other
//! dialog holds the keyboard (`Shell::ask_forms`).
//!
//! Enter answers the form from its fields (or picks the choice a list
//! highlights), Escape and Cancel give no answer, Tab walks the fields and
//! buttons. A field folded under More Options is shown once unfolded, or
//! when the plugin gave it a value. A list (a long choice, a pick) moves
//! with the arrows, filtered by what is typed.

use gpui::{
    AnyElement, Context, Entity, FocusHandle, SharedString, Subscription, Window, div, prelude::*,
    relative,
};
use skwir_core::form::{Answer, Asked, Choice, Field, Form, Kind, Values};

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
    /// A pick from a long list: the filter, the choices it leaves.
    Pick {
        input: Entity<TextInput>,
        shown: Vec<usize>,
    },
    Link {
        focus: FocusHandle,
    },
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
    /// The highlight in the list shown (an unfolded choice's, a pick's).
    picker: Picker,
    /// Why the answer was refused (a field needed is empty).
    refusal: Option<String>,
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
            for (field, control) in self.form.fields.iter().zip(&self.controls) {
                if !self.shows(field) {
                    continue;
                }
                walk.push(match control {
                    Control::Input(input) | Control::Pick { input, .. } => {
                        input.read(cx).focus_handle(cx)
                    }
                    Control::Choice {
                        input: Some(input), ..
                    } => input.read(cx).focus_handle(cx),
                    Control::Toggle { focus, .. }
                    | Control::Choice { focus, .. }
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

    /// Whether `field` is drawn: any not folded under More Options, and
    /// those once unfolded.
    fn shows(&self, field: &Field) -> bool {
        !field.advanced || self.more
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
                Control::Choice { value, .. } => value.clone(),
                Control::Pick { shown, .. } => shown
                    .get(self.picker.highlighted)
                    .and_then(|&at| choices(field).get(at))
                    .map(|choice| choice.value.clone())
                    .unwrap_or_default(),
                Control::Link { .. } => field.value.clone(),
            };
            values.set(field.key.clone(), value);
        }
        values
    }

    /// The choices of the list shown, as `(field, its choices' indices)`.
    fn listed(&self, cx: &gpui::App) -> Option<(usize, Vec<usize>)> {
        if let Some((at, Control::Pick { shown, .. })) = self
            .controls
            .iter()
            .enumerate()
            .find(|(_, c)| matches!(c, Control::Pick { .. }))
        {
            return Some((at, shown.clone()));
        }
        let at = self.open?;
        let field = self.form.fields.get(at)?;
        let typed = match self.controls.get(at)? {
            Control::Choice {
                input: Some(input), ..
            } => input.read(cx).text().to_owned(),
            _ => String::new(),
        };
        Some((at, matching(choices(field), &typed)))
    }
}

/// The choices of a choice or a pick.
fn choices(field: &Field) -> &[Choice] {
    match &field.kind {
        Kind::Choice { choices, .. } | Kind::Pick { choices } => choices,
        _ => &[],
    }
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
                        _ => "",
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
                    Control::Pick {
                        input,
                        shown: (0..choices.len()).collect(),
                    }
                }
                Kind::Link => Control::Link {
                    focus: cx.focus_handle(),
                },
            };
            if first.is_none() && !field.advanced && !waiting {
                use gpui::Focusable as _;
                first = Some(match &control {
                    Control::Input(input)
                    | Control::Pick { input, .. }
                    | Control::Choice {
                        input: Some(input), ..
                    } => input.read(cx).focus_handle(cx),
                    Control::Toggle { focus, .. }
                    | Control::Choice { focus, .. }
                    | Control::Link { focus } => focus.clone(),
                });
            }
            controls.push(control);
        }
        // A pick starts on the choice the asker gave.
        let mut picker = Picker::default();
        for (field, control) in asked.form.fields.iter().zip(&controls) {
            if let (Kind::Pick { choices }, Control::Pick { .. }) = (&field.kind, control)
                && let Some(at) = choices.iter().position(|c| c.value == field.value)
            {
                picker.go(ListMove::First, choices.len());
                for _ in 0..at {
                    picker.step(1, choices.len());
                }
            }
        }
        let state = FormState {
            id: asked.id,
            refusal: asked.form.error.clone(),
            form: asked.form,
            controls,
            more: false,
            more_focus: cx.focus_handle(),
            open: None,
            picker,
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
                InputEvent::Changed => this.form_typed(at, cx),
            },
        ));
        input
    }

    /// What was typed in field `at` changed: its list is filtered again.
    fn form_typed(&mut self, at: usize, cx: &mut Context<Self>) {
        let Some(form) = self.form.as_mut() else {
            return;
        };
        form.refusal = None;
        let Some(field) = form.form.fields.get(at) else {
            return;
        };
        match form.controls.get_mut(at) {
            Some(Control::Pick { input, shown }) => {
                *shown = matching(choices(field), input.read(cx).text());
                form.picker = Picker::default();
            }
            Some(Control::Choice { input: Some(_), .. }) => {
                form.open = Some(at);
                form.picker = Picker::default();
            }
            _ => {}
        }
        cx.notify();
    }

    /// Enter: in a field whose list is shown, its highlighted choice;
    /// elsewhere, the form's answer.
    fn form_enter(&mut self, at: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.form.as_ref() else {
            return;
        };
        if form.form.waiting.is_some() {
            return;
        }
        if let (Some(at), Some(open)) = (at, form.open)
            && at == open
        {
            self.form_choose(None, window, cx);
            return;
        }
        self.form_answer(false, window, cx);
    }

    /// Choose a choice of the unfolded list: `row` of what it shows, else
    /// the highlighted one. A pick answers the form with it.
    fn form_choose(&mut self, row: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.form.as_mut() else {
            return;
        };
        let Some((at, shown)) = form.listed(cx) else {
            return;
        };
        if let Some(row) = row {
            form.picker.highlighted = row;
        }
        let Some(&chosen) = shown.get(form.picker.highlighted) else {
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
        form.refusal = None;
        cx.notify();
    }

    /// Unfold or fold the list of choice `at`.
    fn form_unfold(&mut self, at: usize, cx: &mut Context<Self>) {
        let Some(form) = self.form.as_mut() else {
            return;
        };
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

    /// `ui.list_move` while a form shows a list: whether it took the move.
    pub(crate) fn form_move(&mut self, mv: ListMove, cx: &mut Context<Self>) -> bool {
        let Some(form) = self.form.as_mut() else {
            return false;
        };
        let Some((_, shown)) = form.listed(cx) else {
            return false;
        };
        form.picker.go(mv, shown.len());
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
            cx.notify();
            return;
        }
        self.close_form(window, cx);
    }

    /// Close the form and give the keyboard back to what had it. Its asker,
    /// if it still waits, hears no answer.
    pub fn close_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(form) = self.form.take() {
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
            cx.notify();
        }
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
                .text_xs()
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
        if state.shows(field) {
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
        buttons.push(modal::pressed(
            controls::button(
                "form-accept",
                form.accept.clone(),
                Some("check"),
                ButtonStyle::Plain,
                &state.accept_focus,
                &theme,
            ),
            |this, window, cx| this.form_answer(false, window, cx),
            cx,
        ));
    }
    let hint = if form.waiting.is_some() {
        "Escape to cancel"
    } else {
        "Enter to confirm, Escape to cancel"
    };
    let panel = modal::panel("form", 600., &theme)
        .when(shell.a11y, |d| d.aria_label(form.title.clone()))
        .key_context(keymap::FORM)
        .max_h(relative(0.8))
        .p_3()
        .gap_3()
        .child(
            div()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(form.title.clone()),
        )
        .child(body)
        .children(state.refusal.clone().map(|refusal| {
            div()
                .id("form-refusal")
                .role(gpui::Role::Alert)
                .text_xs()
                .text_color(theme.error)
                .child(refusal)
        }))
        .child(modal::footer(hint, buttons, &theme));
    Some(modal::modal(panel, 80., 3, Shell::form_cancel, &theme, cx))
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
        .text_xs()
        .font_weight(gpui::FontWeight::MEDIUM)
        .child(if field.required {
            format!("{} *", field.title)
        } else {
            field.title.clone()
        });
    let help = (!field.help.is_empty()).then(|| {
        div()
            .text_xs()
            .text_color(theme.text_muted)
            .child(field.help.clone())
    });
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
                            .text_xs()
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
                    div()
                        .flex()
                        .gap_1()
                        .child(controls::field(input, None, focused, false, None, theme))
                        .child(modal::pressed(
                            controls::icon_button(
                                SharedString::from(format!("form-suggest-{at}")),
                                "chevron",
                                "Suggestions",
                                focus,
                                theme,
                            )
                            .aria_expanded(open),
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
    row.children(help).into_any_element()
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
    let shown = state
        .listed(cx)
        .filter(|(listed, _)| *listed == at)
        .map(|(_, shown)| shown)
        .unwrap_or_default();
    let field_focus = match state.controls.get(at) {
        Some(
            Control::Pick { input, .. }
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
        let picker = state.picker;
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
                .text_xs()
                .text_color(theme.text_muted)
                .child("Nothing matches"),
        );
    }
    let mut section = None;
    for (row, &index) in shown.iter().enumerate().take(end).skip(start) {
        let Some(choice) = all.get(index) else {
            continue;
        };
        if !choice.section.is_empty() && section != Some(&choice.section) {
            section = Some(&choice.section);
            list = list.child(
                div()
                    .px_2()
                    .pt_1()
                    .text_xs()
                    .text_color(theme.text_muted)
                    .child(choice.section.clone()),
            );
        }
        let highlighted = row == state.picker.highlighted;
        let named = shell
            .a11y
            .then(|| SharedString::from(choice.shown().to_owned()));
        let item = div()
            .id(SharedString::from(format!("form-option-{at}-{row}")))
            .px_2()
            .py_1()
            .mx_1()
            .rounded_sm()
            .flex()
            .flex_col()
            .cursor_pointer()
            .when(highlighted, |d| {
                d.bg(theme.selection_bg).text_color(theme.selection_text)
            })
            .when(!highlighted, |d| d.hover(|s| s.bg(theme.hover)))
            .child(div().text_sm().child(choice.shown().to_owned()))
            .when(!choice.detail.is_empty(), |d| {
                d.child(
                    div()
                        .text_xs()
                        .when(!highlighted, |d| d.text_color(theme.text_muted))
                        .truncate()
                        .child(choice.detail.clone()),
                )
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                this.form_choose(Some(row), window, cx);
            }));
        list = list.child(modal::option(item, named, row, highlighted, &field_focus));
    }
    if end < shown.len() || start > 0 {
        list = list.child(
            div()
                .px_2()
                .text_xs()
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
