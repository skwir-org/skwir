//! Schema-driven preferences dialog. Drafts stay local until settings.apply;
//! disk writes and provider refreshes run on the core scheduler.

use std::collections::BTreeMap;

use bibli_core::command::{Invocation, Outcome};
use bibli_core::settings::{SaveState, SettingSpec};
use gpui::{
    AnyElement, Context, Entity, FocusHandle, MouseButton, Subscription, Window, deferred, div,
    prelude::*, px, relative,
};

use crate::input::{InputEvent, TextInput};
use crate::shell::Shell;
use crate::theme::Theme;

struct Field {
    spec: SettingSpec,
    initial: String,
    input: Entity<TextInput>,
    focus: FocusHandle,
}

pub struct SettingsState {
    fields: Vec<Field>,
    titles: BTreeMap<&'static str, String>,
    focus: FocusHandle,
    pending: Option<u64>,
    scroll: gpui::ScrollHandle,
    message: String,
    error: bool,
    _subscriptions: Vec<Subscription>,
}

impl Shell {
    pub fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings.is_some() {
            return;
        }
        self.palette = None;
        self.menu = None;
        let mut subscriptions = Vec::new();
        let fields = self
            .app
            .settings
            .schema()
            .into_iter()
            .filter(SettingSpec::editable)
            .map(|spec| {
                let value = self
                    .app
                    .settings
                    .get(&spec.key)
                    .unwrap_or_else(|| spec.default.clone());
                let initial = SettingSpec::format_value(&value);
                let input = cx.new(|cx| TextInput::new("", cx));
                input.update(cx, |input, cx| input.set_text(initial.clone(), cx));
                subscriptions.push(cx.subscribe_in(
                    &input,
                    window,
                    |this, _, event, window, cx| {
                        let command = match event {
                            InputEvent::Submit => "ui.settings_save",
                            InputEvent::Cancel => "ui.settings_close",
                            InputEvent::Changed => {
                                if let Some(state) = this.settings.as_mut() {
                                    state.message.clear();
                                    state.error = false;
                                }
                                cx.notify();
                                return;
                            }
                        };
                        this.run(command, serde_json::json!({}), window, cx);
                    },
                ));
                Field {
                    spec,
                    initial,
                    input,
                    focus: cx.focus_handle(),
                }
            })
            .collect();
        let focus = cx.focus_handle();
        window.focus(&focus);
        self.settings = Some(SettingsState {
            fields,
            titles: self
                .app
                .plugins
                .list()
                .into_iter()
                .map(|plugin| (plugin.id, plugin.title))
                .collect(),
            focus,
            pending: None,
            scroll: gpui::ScrollHandle::new(),
            message: String::new(),
            error: false,
            _subscriptions: subscriptions,
        });
    }

    pub fn settings_command(
        &mut self,
        name: &str,
        args: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.settings.as_mut() else {
            return;
        };
        if state.pending.is_some() {
            return;
        }
        match name {
            "settings_close" => {
                self.settings = None;
                window.focus(&self.ui().focus);
            }
            "settings_save" => {
                let changes: BTreeMap<_, _> = state
                    .fields
                    .iter()
                    .filter_map(|field| {
                        let text = field.input.read(cx).text();
                        (text != field.initial).then(|| (field.spec.key.clone(), text.to_owned()))
                    })
                    .collect();
                if changes.is_empty() {
                    state.message = "No changes to save".into();
                    state.error = false;
                    return;
                }
                match self.app.commands.dispatch(
                    &Invocation::with_args(
                        "settings.apply",
                        &serde_json::json!({ "changes": changes }),
                    ),
                    &self.app,
                ) {
                    Ok(Outcome::Value(id)) => {
                        state.pending = id.as_u64();
                        state.message = "Saving…".into();
                        state.error = false;
                        window.focus(&state.focus);
                    }
                    Err(error) => {
                        state.message = format!("{error:#}");
                        state.error = true;
                    }
                    _ => {}
                }
            }
            "settings_next" | "settings_prev" => {
                let len = state.fields.len();
                if len == 0 {
                    return;
                }
                let current = state.fields.iter().position(|field| {
                    field.focus.is_focused(window) || field.input.read(cx).is_focused(window)
                });
                let index = match (current, name == "settings_prev") {
                    (Some(index), false) => (index + 1) % len,
                    (Some(index), true) => (index + len - 1) % len,
                    (None, false) => 0,
                    (None, true) => len - 1,
                };
                state.scroll.scroll_to_item(index);
                let field = &state.fields[index];
                if options(&field.spec).is_empty() {
                    field.input.read(cx).focus(window);
                } else {
                    window.focus(&field.focus);
                }
            }
            "settings_cycle" | "settings_set" | "settings_reset" => {
                let key = args.get("key").and_then(serde_json::Value::as_str);
                let Some(field) = state.fields.iter().find(|field| {
                    key.map_or_else(
                        || field.focus.is_focused(window),
                        |key| field.spec.key == key,
                    )
                }) else {
                    return;
                };
                let value = if name == "settings_reset" {
                    SettingSpec::format_value(&field.spec.default)
                } else if name == "settings_cycle" {
                    let choices = options(&field.spec);
                    if choices.is_empty() {
                        return;
                    }
                    let index = choices
                        .iter()
                        .position(|s| s == field.input.read(cx).text())
                        .map_or(0, |i| (i + 1) % choices.len());
                    choices[index].clone()
                } else {
                    args.get("value")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default()
                        .into()
                };
                field
                    .input
                    .update(cx, |input, cx| input.set_text(value, cx));
                window.focus(&field.focus);
                state.message.clear();
            }
            _ => {}
        }
    }

    pub fn sync_settings(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.settings.as_mut() else {
            return;
        };
        let Some(pending) = state.pending else { return };
        let (id, status) = self.app.settings.save_status();
        if id != pending {
            return;
        }
        match status {
            SaveState::Saved => {
                state.pending = None;
                state.message = "Saved — changes are active".into();
                state.error = false;
                for field in &mut state.fields {
                    field.initial = field.input.read(cx).text().into();
                }
            }
            SaveState::Failed(error) => {
                state.pending = None;
                state.message = error;
                state.error = true;
            }
            _ => {}
        }
    }
}

fn options(spec: &SettingSpec) -> Vec<String> {
    if spec.default.is_bool() {
        vec!["false".into(), "true".into()]
    } else {
        spec.constraints.choices.clone()
    }
}

fn button(
    id: String,
    label: String,
    command: &'static str,
    args: serde_json::Value,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(gpui::SharedString::from(id))
        .px_3()
        .py_1()
        .rounded_md()
        .border_1()
        .border_color(theme.border)
        .cursor_pointer()
        .hover(|s| s.bg(theme.border))
        .on_click(
            cx.listener(move |this, _, window, cx| this.run(command, args.clone(), window, cx)),
        )
        .child(label)
}

pub fn render(shell: &Shell, window: &Window, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let state = shell.settings.as_ref()?;
    let theme = cx.global::<Theme>().clone();
    let saving = state.pending.is_some();
    let dirty = state
        .fields
        .iter()
        .any(|field| field.input.read(cx).text() != field.initial);
    let mut rows = div()
        .id("settings-rows")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .track_scroll(&state.scroll)
        .flex()
        .flex_col()
        .gap_4()
        .p_4();
    let mut owner = "";
    for field in &state.fields {
        let mut row = div().flex().flex_col().gap_4().flex_shrink_0();
        let namespace = field.spec.key.split('.').next().unwrap_or_default();
        if namespace != owner {
            owner = namespace;
            let title = state
                .titles
                .get(namespace)
                .cloned()
                .unwrap_or_else(|| namespace.to_owned());
            row = row.child(div().text_color(theme.accent).child(title));
        }
        let value = field.input.read(cx).text().to_owned();
        let choices = options(&field.spec);
        let label = field
            .spec
            .key
            .split_once('.')
            .map_or(field.spec.key.as_str(), |(_, label)| label)
            .replace('_', " ");
        let mut controls = div()
            .flex()
            .gap_2()
            .items_center()
            .track_focus(&field.focus)
            .when(!choices.is_empty(), |d| d.key_context("SettingChoice"))
            .when(
                field.focus.is_focused(window) || field.input.read(cx).is_focused(window),
                |d| d.border_b_1().border_color(theme.accent),
            );
        if saving {
            controls = controls.child(value.clone());
        } else if choices.is_empty() {
            controls = controls.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h(px(30.))
                    .px_2()
                    .flex()
                    .items_center()
                    .bg(theme.bg)
                    .border_1()
                    .border_color(theme.border)
                    .rounded_md()
                    .child(field.input.clone()),
            );
        } else {
            for choice in choices {
                let text = match choice.as_str() {
                    "true" => "On".into(),
                    "false" => "Off".into(),
                    _ => choice.clone(),
                };
                controls = controls.child(
                    button(
                        format!("{}-{choice}", field.spec.key),
                        text,
                        "ui.settings_set",
                        serde_json::json!({ "key": field.spec.key, "value": choice }),
                        &theme,
                        cx,
                    )
                    .when(value == choice, |d| {
                        d.bg(theme.selection_bg).text_color(theme.selection_text)
                    }),
                );
            }
        }
        if !saving {
            controls = controls.child(button(
                format!("{}-reset", field.spec.key),
                "Default".into(),
                "ui.settings_reset",
                serde_json::json!({ "key": field.spec.key }),
                &theme,
                cx,
            ));
        }
        row = row.child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .flex_shrink_0()
                .child(div().child(label))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.text_muted)
                        .child(field.spec.description.clone()),
                )
                .child(controls),
        );
        rows = rows.child(row);
    }
    let footer = div()
        .p_4()
        .border_t_1()
        .border_color(theme.border)
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .text_xs()
                .text_color(if state.error {
                    theme.error
                } else {
                    theme.text_muted
                })
                .child(state.message.clone()),
        )
        .child(
            div()
                .flex()
                .gap_2()
                .justify_end()
                .child(
                    button(
                        "settings-close".into(),
                        if dirty { "Cancel (Esc)" } else { "Close (Esc)" }.into(),
                        "ui.settings_close",
                        serde_json::json!({}),
                        &theme,
                        cx,
                    )
                    .when(saving, |d| d.opacity(0.5)),
                )
                .child(
                    button(
                        "settings-save".into(),
                        "Save (Ctrl+S)".into(),
                        "ui.settings_save",
                        serde_json::json!({}),
                        &theme,
                        cx,
                    )
                    .bg(theme.selection_bg)
                    .when(saving, |d| d.opacity(0.5)),
                ),
        );
    let panel = div()
        .w(relative(0.9))
        .max_w(px(720.))
        .h(relative(0.85))
        .max_h(px(720.))
        .flex()
        .flex_col()
        .rounded_lg()
        .border_1()
        .border_color(theme.border)
        .bg(theme.surface)
        .shadow_lg()
        .track_focus(&state.focus)
        .key_context("Settings")
        .child(
            div()
                .p_4()
                .border_b_1()
                .border_color(theme.border)
                .flex()
                .flex_col()
                .gap_1()
                .child(div().text_lg().child("Settings"))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.text_muted)
                        .child("Preferences for enabled plugins. Save to apply changes."),
                ),
        )
        .child(rows)
        .child(footer);
    Some(
        deferred(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .occlude()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(panel),
        )
        .with_priority(5)
        .into_any_element(),
    )
}
