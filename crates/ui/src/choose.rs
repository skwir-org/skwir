//! `ui.choose`: pick one of a list and run what it says. A plugin that
//! needs the user to choose (an application to open a file with) dispatches
//! this with its choices or with the command that lists them, as `ui.ask`
//! asks for a line of text: it needs no picker of its own.
//!
//! `ui.choose { title?, placeholder?, option?, choices? | source? }`.
//! `source` is an invocation run on a worker (listing choices can read the
//! disk); it answers `{ title?, empty?, option?, choices }`, its title
//! replacing the one given. `option` is `{ title, field }`: a switch under
//! the list (Alt+A) that, when on, runs the chosen command with `field` set
//! to `true` ("Always use for PNG image files").
//! A choice is `{ title, detail?, icon?, section?, command }`: `icon` is a
//! theme icon name or an absolute path, and consecutive choices of one
//! `section` are shown under its name. Until the source answers the list is
//! blank, never "nothing found", so a quick answer does not flash.

use std::sync::atomic::{AtomicU64, Ordering};

use gpui::{AnyElement, Context, Entity, Subscription, Window, div, prelude::*, px};
use serde::Deserialize;
use skwir_core::command::{Invocation, Outcome};

use crate::input::{InputEvent, TextInput};
use crate::keymap;
use crate::modal::{self, FocusReturn, Picker};
use crate::shell::Shell;
use crate::theme::Theme;

#[derive(Clone, Debug, Deserialize)]
pub struct Choice {
    pub title: String,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub section: Option<String>,
    pub command: Invocation,
}

/// A switch that adds `field: true` to the chosen command.
#[derive(Clone, Debug, Deserialize)]
pub struct ChoiceOption {
    pub title: String,
    pub field: String,
}

/// What a source answers.
#[derive(Debug, Default, Deserialize)]
struct Answer {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    empty: Option<String>,
    #[serde(default)]
    option: Option<ChoiceOption>,
    choices: Vec<Choice>,
}

pub struct ChooseState {
    pub input: Entity<TextInput>,
    pub title: String,
    /// Said when no choice matches.
    pub empty: String,
    pub choices: Vec<Choice>,
    /// Indices into `choices`, in the order shown.
    pub filtered: Vec<usize>,
    pub picker: Picker,
    pub option: Option<ChoiceOption>,
    /// The option's switch is on.
    pub option_on: bool,
    option_focus: gpui::FocusHandle,
    /// The choices arrived.
    loaded: bool,
    /// The query `filtered` was computed for.
    query: Option<String>,
    /// Which source request this picker waits for: an answer to an earlier
    /// picker, closed since, is dropped.
    request: u64,
    back: FocusReturn,
    _sub: Subscription,
}

/// Size of a choice's icon, in px.
const ICON_PX: f32 = 20.;

impl Shell {
    pub fn open_choose(
        &mut self,
        args: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        static REQUESTS: AtomicU64 = AtomicU64::new(0);
        let text = |key: &str| args.get(key).and_then(serde_json::Value::as_str);
        let choices: Option<Vec<Choice>> = args
            .get("choices")
            .map(|c| serde_json::from_value(c.clone()))
            .transpose()
            .unwrap_or_else(|e| {
                tracing::debug!(%e, "ui.choose: unreadable choices");
                None
            });
        let source: Option<Invocation> = args
            .get("source")
            .and_then(|s| serde_json::from_value(s.clone()).ok());
        if choices.is_none() && source.is_none() {
            self.set_status("ui.choose needs choices or a source", true, cx);
            return;
        }
        let back = FocusReturn::capture(window, cx);
        let placeholder = text("placeholder").unwrap_or("Type to filter").to_owned();
        let input = cx.new(|cx| TextInput::new(placeholder, cx));
        let sub = cx.subscribe_in(
            &input,
            window,
            |this, _, ev: &InputEvent, window, cx| match ev {
                InputEvent::Submit => this.choose_submit(window, cx),
                InputEvent::Cancel => this.close_choose(window, cx),
                InputEvent::Changed => cx.notify(),
            },
        );
        input.update(cx, |i, cx| i.focus(window, cx));
        let request = REQUESTS.fetch_add(1, Ordering::Relaxed);
        let option = args
            .get("option")
            .and_then(|o| serde_json::from_value(o.clone()).ok());
        let loaded = choices.is_some();
        self.choose = Some(ChooseState {
            input,
            title: text("title").unwrap_or_default().to_owned(),
            empty: "Nothing to choose from".to_owned(),
            choices: choices.unwrap_or_default(),
            filtered: Vec::new(),
            picker: Picker::default(),
            option,
            option_on: false,
            option_focus: cx.focus_handle(),
            loaded,
            query: None,
            request,
            back,
            _sub: sub,
        });
        if let Some(source) = source.filter(|_| !loaded) {
            let app = std::sync::Arc::clone(&self.app);
            self.off_thread(
                "choose-source",
                move || app.dispatch(&source),
                move |this, result, window, cx| {
                    if this.choose.as_ref().is_none_or(|c| c.request != request) {
                        return;
                    }
                    let answer = match result {
                        Ok(Outcome::Value(value)) => serde_json::from_value::<Answer>(value)
                            .map_err(|e| anyhow::anyhow!("unreadable choices: {e}")),
                        Ok(_) => Err(anyhow::anyhow!("nothing to choose from")),
                        Err(e) => Err(e),
                    };
                    match answer {
                        Ok(answer) => {
                            if let Some(c) = &mut this.choose {
                                if let Some(title) = answer.title {
                                    c.title = title;
                                }
                                if let Some(empty) = answer.empty {
                                    c.empty = empty;
                                }
                                if answer.option.is_some() {
                                    c.option = answer.option;
                                }
                                c.choices = answer.choices;
                                c.loaded = true;
                                c.query = None;
                            }
                        }
                        // Nothing to pick from: the picker goes and says why.
                        Err(e) => {
                            this.close_choose(window, cx);
                            this.report_error(&e, cx);
                        }
                    }
                    cx.notify();
                },
                window,
                cx,
            );
        }
        cx.notify();
    }

    pub fn close_choose(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(choose) = self.choose.take() {
            choose.back.restore(window, &self.ui().focus, cx);
        }
        cx.notify();
    }

    pub fn choose_move(&mut self, mv: crate::modal::ListMove, cx: &mut Context<Self>) {
        if let Some(c) = &mut self.choose {
            c.picker.go(mv, c.filtered.len());
            cx.notify();
        }
    }

    /// Turn the option's switch on or off; the keyboard stays in the
    /// filter.
    pub fn choose_toggle_option(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(c) = &mut self.choose
            && c.option.is_some()
        {
            c.option_on = !c.option_on;
            c.input.update(cx, |i, cx| i.focus(window, cx));
            cx.notify();
        }
    }

    fn choose_submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.choose_refilter(cx);
        let command = self.choose.as_ref().and_then(|c| {
            let ix = *c.filtered.get(c.picker.highlighted)?;
            let mut command = c.choices[ix].command.clone();
            if let Some(option) = c.option.as_ref().filter(|_| c.option_on)
                && let Some(args) = command.args.as_object_mut()
            {
                args.insert(option.field.clone(), true.into());
            }
            Some(command)
        });
        let Some(command) = command else { return };
        self.close_choose(window, cx);
        self.run(&command.name, command.args, window, cx);
    }

    /// Match the query against each choice's title and detail; the order
    /// given stands while nothing is typed.
    fn choose_refilter(&mut self, cx: &mut Context<Self>) {
        let Some(c) = &mut self.choose else { return };
        let q = c.input.read(cx).text().to_owned();
        if c.query.as_deref() == Some(q.as_str()) {
            return;
        }
        c.filtered = filter(&c.choices, &q);
        c.picker = Picker::default();
        c.query = Some(q);
    }
}

/// Indices of the choices matching `query`, best first; all of them in
/// their order for an empty query.
fn filter(choices: &[Choice], query: &str) -> Vec<usize> {
    crate::modal::fuzzy(choices, query, |c| match &c.detail {
        Some(detail) => format!("{} {detail}", c.title),
        None => c.title.clone(),
    })
}

pub fn render(shell: &mut Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    shell.choose_refilter(cx);
    let theme = cx.global::<Theme>().clone();
    // Icons first: the cache needs the shell mutably, the rows only read.
    let icons: Vec<Option<crate::icons::Pic>> = {
        let c = shell.choose.as_ref()?;
        let wanted: Vec<Option<String>> = c
            .picker
            .shown(c.filtered.len())
            .map(|row| c.choices[c.filtered[row]].icon.clone())
            .collect();
        wanted
            .into_iter()
            .map(|icon| {
                let icon = icon?;
                if std::path::Path::new(&icon).is_absolute() {
                    Some(crate::icons::Pic::Theme(std::path::Path::new(&icon).into()))
                } else {
                    shell.icons.named(&icon, ICON_PX as u16)
                }
            })
            .collect()
    };
    let option_keys = shell.keys_for("ui.choose_option", &serde_json::json!({}));
    let c = shell.choose.as_ref()?;
    let a11y = shell.a11y;
    let typing = gpui::Focusable::focus_handle(c.input.read(cx), cx);
    let mut list = modal::options("choose-results", "Choices", c.filtered.len(), a11y)
        .flex()
        .flex_col()
        .py_1()
        .overflow_hidden();
    let mut previous: Option<&str> = None;
    for (row, icon) in c.picker.shown(c.filtered.len()).zip(icons) {
        let choice = &c.choices[c.filtered[row]];
        let hl = row == c.picker.highlighted;
        let section = choice.section.as_deref();
        if section.is_some() && section != previous {
            list = list.child(
                div()
                    .px_3()
                    .pt_2()
                    .pb_1()
                    .text_xs()
                    .text_color(theme.text_muted)
                    .children(section.map(str::to_owned)),
            );
        }
        previous = section;
        let muted = if hl {
            theme.selection_text
        } else {
            theme.text_muted
        };
        let named = a11y.then(|| choice.title.clone().into());
        list = list.child(
            modal::option(div().id(("choice", row)), named, row, hl, &typing)
                .h(px(28.))
                .px_3()
                .flex()
                .items_center()
                .gap_3()
                .cursor_pointer()
                .when(hl, |d| {
                    d.bg(theme.selection_bg).text_color(theme.selection_text)
                })
                .when(!hl, |d| d.hover(|s| s.bg(theme.hover)))
                .on_click(cx.listener(move |this, _, window, cx| {
                    if let Some(c) = &mut this.choose {
                        c.picker.highlighted = row;
                    }
                    this.choose_submit(window, cx);
                }))
                .child(match icon {
                    Some(pic) => crate::icons::themed(&pic, ICON_PX, theme.text_muted),
                    None => div().size(px(ICON_PX)).flex_none().into_any_element(),
                })
                .child(crate::elements::clip::clipped(
                    div()
                        .id("choice-title")
                        .flex_none()
                        .max_w(px(260.))
                        .truncate(),
                    choice.title.clone(),
                    shell.font.clone(),
                ))
                .child(crate::elements::clip::clipped(
                    div()
                        .id("choice-detail")
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_xs()
                        .text_color(muted),
                    choice.detail.clone().unwrap_or_default(),
                    shell.font.clone(),
                )),
        );
    }
    if c.loaded && c.filtered.is_empty() {
        list = list.child(
            div()
                .px_3()
                .py_2()
                .text_color(theme.text_muted)
                .child(c.empty.clone()),
        );
    }
    let option = c.option.as_ref().map(|option| {
        div()
            .id("choose-option")
            .px_3()
            .py_2()
            .flex()
            .items_center()
            .gap_2()
            .border_t_1()
            .border_color(theme.border)
            .cursor_pointer()
            .on_click(cx.listener(|this, _, window, cx| this.choose_toggle_option(window, cx)))
            .child(crate::controls::switch(
                "choose-option-switch",
                c.option_on,
                &c.option_focus,
                &theme,
            ))
            .child(crate::elements::clip::clipped(
                div()
                    .id("choose-option-title")
                    .flex_1()
                    .min_w_0()
                    .truncate(),
                option.title.clone(),
                shell.font.clone(),
            ))
            .children(option_keys.map(|keys| crate::controls::keycap(keys, &theme)))
    });
    let panel = modal::panel("choose", 520., &theme)
        .when(shell.a11y, |d| d.aria_label(c.title.clone()))
        .key_context(keymap::CHOOSE)
        .child(
            div()
                .p_2()
                .flex()
                .flex_col()
                .gap_1()
                .border_b_1()
                .border_color(theme.border)
                // A file's name keeps both ends: the extension says what
                // the applications below are for.
                .child(crate::elements::clip::clipped(
                    div()
                        .id("choose-title")
                        .px_1()
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .text_ellipsis_middle()
                        .text_xs()
                        .text_color(theme.text_muted),
                    c.title.clone(),
                    shell.font.clone(),
                ))
                .child(c.input.clone()),
        )
        .child(list)
        .children(option);
    Some(modal::modal(panel, 80., 4, Shell::close_choose, &theme, cx))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choice(title: &str, detail: Option<&str>) -> Choice {
        Choice {
            title: title.into(),
            detail: detail.map(str::to_owned),
            icon: None,
            section: None,
            command: Invocation::new("x"),
        }
    }

    #[test]
    fn an_empty_query_keeps_the_order_given_and_a_query_matches_details() {
        let choices = [
            choice("Loupe", Some("Default · Image Viewer")),
            choice("GIMP", Some("Image Editor")),
            choice("Text Editor", None),
        ];
        assert_eq!(filter(&choices, " "), [0, 1, 2]);
        let editors = filter(&choices, "editor");
        assert_eq!(editors.len(), 2);
        assert!(editors.contains(&1) && editors.contains(&2));
        assert!(filter(&choices, "zzz").is_empty());
    }

    #[test]
    fn a_source_answer_reads_with_or_without_its_optional_parts() {
        let answer: Answer = serde_json::from_value(serde_json::json!({
            "choices": [{ "title": "A", "command": { "name": "open_with", "args": { "app": "a.desktop" } } }]
        }))
        .unwrap();
        assert!(answer.title.is_none());
        assert_eq!(&*answer.choices[0].command.name, "open_with");
    }
}
