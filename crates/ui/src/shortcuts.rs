//! Learning the keys without reading a manual.
//!
//! - F1 lists the shortcuts that apply where the focus is right now, read
//!   from the focused element's key contexts, with the rest one click away.
//!   What is typed there narrows them and finds the other places' too, by
//!   what they do or by their keys (`keymap::finds`).
//! - A command used three times with the pointer, when it has a shortcut,
//!   earns one toast naming it. Once per command, ever: the commands already
//!   hinted are kept with the settings.

use std::collections::HashMap;

use gpui::{
    AnyElement, Context, Entity, FocusHandle, Focusable, MouseButton, ScrollHandle, Subscription,
    Window, div, prelude::*, px,
};

use crate::controls::{self, ButtonStyle};
use crate::input::{InputEvent, TextInput};
use crate::keymap::{self, KeyHint, RunCommand};
use crate::modal::{self, FocusReturn};
use crate::shell::Shell;
use crate::theme::Theme;
use crate::toast::{Toast, Tone};

/// Application data: commands whose shortcut hint was shown.
pub const HINTS_KEY: &str = "ui.hints_shown";
const USES_BEFORE_HINT: u32 = 3;

/// A place, or a place in a state, and its bindings with what each does.
struct Group {
    /// What its column is titled.
    label: String,
    /// The place alone, which a filter's words find: not its conditions
    /// ("not in the trash" is no answer to "trash").
    place: String,
    hints: Vec<(KeyHint, String)>,
}

pub struct Overview {
    focus: FocusHandle,
    /// What narrows the lists, by words or keys; it has the keyboard when
    /// the overview opens.
    filter: Entity<TextInput>,
    /// The "All shortcuts" and close buttons', kept so Tab can reach
    /// them.
    all: FocusHandle,
    close: FocusHandle,
    previous: FocusReturn,
    /// The places the focus is in, the innermost first. Titled once, when
    /// the overview opens.
    groups: Vec<Group>,
    /// The bindings that do not run here (another place, a condition that
    /// does not hold), by place and condition: what a filter finds too.
    elsewhere: Vec<Group>,
    scroll: ScrollHandle,
    _typed: Subscription,
}

#[derive(Default)]
pub struct Nudges {
    uses: HashMap<String, u32>,
}

/// Whether a binding's context predicate (`Pane && single`) holds for the
/// contexts on the focus path. Conditions such as `single` are keys of the
/// pane's own context, so one rule covers places and conditions
/// (`keymap::predicate_holds`).
pub fn applies(predicate: &str, holds: &dyn Fn(&str) -> bool) -> bool {
    crate::keymap::predicate_holds(predicate, &|token| Some(holds(token)))
}

impl Shell {
    pub fn toggle_shortcuts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(open) = self.shortcuts.take() {
            open.previous.restore(window, &self.ui().focus, cx);
            cx.notify();
            return;
        }
        let stack = window.context_stack();
        let holds = |token: &str| stack.iter().any(|context| context.contains(token));
        // Of the bindings of one key that apply here, the one GPUI runs:
        // the deepest place's, then the latest (a later layer).
        let depth = |hint: &KeyHint| {
            let place = hint.context.split("&&").next().unwrap_or_default().trim();
            stack.iter().rposition(|context| context.contains(place))
        };
        let applying: Vec<(usize, &KeyHint)> = self
            .key_hints
            .iter()
            .enumerate()
            .filter(|(_, hint)| applies(&hint.context, &holds))
            .collect();
        let wins = |i: usize, hint: &KeyHint| {
            !applying.iter().any(|(j, other)| {
                *j != i && other.keys == hint.keys && (depth(other), *j) > (depth(hint), i)
            })
        };
        let mut groups: Vec<Group> = Vec::new();
        let mut elsewhere: Vec<Group> = Vec::new();
        let add = |groups: &mut Vec<Group>, label: String, hint: &KeyHint| {
            let titled = (hint.clone(), keymap::title_of(&self.app, hint));
            match groups.iter_mut().find(|group| group.label == label) {
                Some(group) => group.hints.push(titled),
                None => groups.push(Group {
                    label,
                    place: keymap::place_of(&hint.context).place,
                    hints: vec![titled],
                }),
            }
        };
        for (i, hint) in self.key_hints.iter().enumerate() {
            if applying.iter().any(|(j, _)| *j == i) && wins(i, hint) {
                // Conditions narrow a place; here, it groups by the place.
                let place = hint.context.split("&&").next().unwrap_or_default();
                add(&mut groups, keymap::context_label(place), hint);
            } else if !self.key_hints[i + 1..]
                .iter()
                .any(|later| later.keys == hint.keys && later.context == hint.context)
            {
                // Elsewhere, the condition says why it is not here.
                add(&mut elsewhere, keymap::context_label(&hint.context), hint);
            }
        }
        // The innermost place is what the user is looking at.
        groups.sort_by_key(|group| group.label == "Anywhere");
        let previous = FocusReturn::capture(window, cx);
        let filter = cx.new(|cx| TextInput::new("Filter by action or keys", cx));
        let typed = cx.subscribe_in(
            &filter,
            window,
            |this, input, event: &InputEvent, window, cx| match event {
                InputEvent::Changed => {
                    if let Some(overview) = &this.shortcuts {
                        overview.scroll.set_offset(gpui::Point::default());
                    }
                    cx.notify();
                }
                // Escape empties the filter, then closes.
                InputEvent::Cancel if !input.read(cx).text().is_empty() => {
                    input.update(cx, |input, cx| input.set_text("", cx));
                    cx.notify();
                }
                InputEvent::Cancel => this.toggle_shortcuts(window, cx),
                InputEvent::Submit => {}
            },
        );
        window.focus(&filter.focus_handle(cx), cx);
        self.shortcuts = Some(Overview {
            focus: cx.focus_handle(),
            filter,
            all: cx.focus_handle(),
            close: cx.focus_handle(),
            previous,
            groups,
            elsewhere,
            scroll: ScrollHandle::new(),
            _typed: typed,
        });
        cx.notify();
    }

    /// A command was run with the pointer (toolbar, menu).
    pub fn note_pointer_use(
        &mut self,
        command: &str,
        args: &serde_json::Value,
        cx: &mut Context<Self>,
    ) {
        let empty = serde_json::json!({});
        let args = if args.is_null() { &empty } else { args };
        let Some(hint) = self
            .key_hints
            .iter()
            .find(|hint| hint.command == command && hint.args == *args)
            .cloned()
        else {
            return;
        };
        let id = format!("{command} {args}");
        let uses = self.nudges.uses.entry(id.clone()).or_insert(0);
        *uses += 1;
        if *uses != USES_BEFORE_HINT {
            return;
        }
        let mut shown: Vec<String> = self.app.settings.get(HINTS_KEY).unwrap_or_default();
        if shown.contains(&id) {
            return;
        }
        shown.push(id);
        self.app.settings.set(
            HINTS_KEY,
            toml::Value::Array(shown.into_iter().map(toml::Value::String).collect()),
        );
        self.save_settings_later();
        self.toast(
            Toast {
                text: format!(
                    "Tip: press {} for {}. F1 lists the keys for where you are.",
                    keymap::display_keys(&hint.keys),
                    keymap::title_of(&self.app, &hint).trim_end_matches('…')
                ),
                tone: Tone::Info,
                actions: Vec::new(),
            },
            cx,
        );
    }

    /// Persist application data kept with the settings, on a worker.
    pub fn save_settings_later(&self) {
        let weak = self.app.weak();
        self.app.scheduler.spawn(
            skwir_core::scheduler::Priority::Background,
            "ui-data-save",
            self.app.lifetime().child(),
            move || {
                if let Some(app) = weak.upgrade()
                    && let Err(error) = app.settings.save()
                {
                    tracing::warn!("cannot save interface data: {error:#}");
                }
            },
        );
    }
}

/// What of `groups` the overview's filter finds: all of them while it is
/// empty.
fn found<'a>(groups: &'a [Group], query: &str) -> Vec<(&'a str, Vec<&'a (KeyHint, String)>)> {
    groups
        .iter()
        .filter_map(|group| {
            let hints: Vec<_> = group
                .hints
                .iter()
                .filter(|(hint, title)| {
                    keymap::finds(query, &[title, &group.place, &hint.command], &[&hint.keys])
                })
                .collect();
            (!hints.is_empty()).then_some((group.label.as_str(), hints))
        })
        .collect()
}

pub fn render(shell: &Shell, window: &Window, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let overview = shell.shortcuts.as_ref()?;
    let theme = cx.global::<Theme>().clone();
    let query = overview.filter.read(cx).text().trim().to_owned();
    let here = found(&overview.groups, &query);
    // The other places only for a filter: unasked, they would bury these.
    let elsewhere = if query.is_empty() {
        Vec::new()
    } else {
        found(&overview.elsewhere, &query)
    };
    let mut row = 0_usize;
    let mut lists = div().flex().flex_col().gap_6();
    if here.is_empty() && elsewhere.is_empty() {
        lists = lists.child(
            div()
                .text_color(theme.text_muted)
                .child(format!("No shortcut matches “{query}”.")),
        );
    }
    for (part, groups) in [(None, here), (Some("Elsewhere"), elsewhere)] {
        if groups.is_empty() {
            continue;
        }
        let columns = columns(&groups, &mut row, &theme);
        lists = lists.child(match part {
            Some(title) => div()
                .flex()
                .flex_col()
                .gap_3()
                .child(div().text_color(theme.text_muted).child(title))
                .child(columns),
            None => columns,
        });
    }
    let all = overview.all.clone();
    let filtering = overview.filter.read(cx).is_focused(window);
    // The lists scroll under the title, which keeps its buttons in sight.
    let panel = div()
        .w(px(1000.))
        .max_w(gpui::relative(0.94))
        .max_h(gpui::relative(0.86))
        .p_6()
        .flex()
        .flex_col()
        .gap_4()
        .rounded_lg()
        .border_1()
        .border_color(theme.border)
        .bg(theme.bg)
        .shadow_lg()
        .line_height(px(20.))
        .track_focus(&overview.focus)
        .key_context("Shortcuts")
        .on_action(cx.listener(|this, action: &RunCommand, window, cx| {
            let delta = match action.name.as_str() {
                "ui.toolbar_next" => 1,
                "ui.toolbar_prev" => -1,
                _ => return cx.propagate(),
            };
            if let Some(overview) = &this.shortcuts {
                let filter = overview.filter.focus_handle(cx);
                modal::tab_among(
                    &[&filter, &overview.all, &overview.close],
                    delta,
                    window,
                    cx,
                );
            }
        }))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(
            div()
                .flex()
                .items_center()
                .gap_4()
                .child(div().flex_none().text_lg().child("Shortcuts here"))
                .child(controls::field(
                    &overview.filter,
                    Some(300.),
                    filtering,
                    false,
                    Some("search"),
                    &theme,
                ))
                .child(div().flex_1())
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            controls::button(
                                "shortcuts-all",
                                "All shortcuts…",
                                None,
                                ButtonStyle::Plain,
                                &all,
                                &theme,
                            )
                            .on_click(cx.listener(
                                |this, _, window, cx| {
                                    // The Keyboard page, narrowed as this was.
                                    let filter =
                                        this.shortcuts.take().map_or_else(String::new, |open| {
                                            open.filter.read(cx).text().trim().to_owned()
                                        });
                                    let mut args = serde_json::json!({ "page": "keyboard" });
                                    if !filter.is_empty() {
                                        args["filter"] = filter.into();
                                    }
                                    this.run("ui.settings", args, window, cx);
                                },
                            )),
                        )
                        .child(
                            controls::icon_button(
                                "shortcuts-close",
                                "close",
                                "Close",
                                &overview.close,
                                &theme,
                            )
                            .on_click(cx.listener(
                                |this, _, window, cx| {
                                    this.toggle_shortcuts(window, cx);
                                },
                            )),
                        ),
                ),
        )
        .child(
            div()
                .id("shortcuts")
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&overview.scroll)
                .child(lists),
        );
    // Its top stays where it is while a filter narrows it.
    Some(
        div()
            .absolute()
            .inset_0()
            .flex()
            .flex_col()
            .items_center()
            .occlude()
            .bg(theme.scrim)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.toggle_shortcuts(window, cx)),
            )
            .child(div().flex_none().h(gpui::relative(0.07)))
            .child(panel)
            .into_any_element(),
    )
}

/// `groups` in columns of a place each, its bindings with their keys;
/// `row` counts the rows drawn, for their ids.
fn columns(
    groups: &[(&str, Vec<&(KeyHint, String)>)],
    row: &mut usize,
    theme: &Theme,
) -> gpui::Div {
    let mut columns = div().flex().flex_wrap().gap_x_8().gap_y_4();
    for (place, hints) in groups {
        let mut column = div().w(px(300.)).flex().flex_col().gap(px(3.)).child(
            div()
                .pb_1()
                .text_xs()
                .text_color(theme.text_muted)
                .child((*place).to_owned()),
        );
        for (hint, title) in hints {
            let mut keys = div().flex().flex_none().items_center().gap(px(8.));
            for stroke in keymap::key_caps(&hint.keys) {
                let mut caps = div().flex().items_center().gap(px(3.));
                for cap in stroke {
                    caps = caps.child(controls::keycap(cap, theme));
                }
                keys = keys.child(caps);
            }
            column = column.child(
                div()
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(crate::elements::clip::clipped(
                        div()
                            .id(crate::elements::known_by("shortcut", (*row, title)))
                            .flex_1()
                            .min_w_0()
                            .truncate(),
                        title.clone(),
                    ))
                    .child(keys),
            );
            *row += 1;
        }
        columns = columns.child(column);
    }
    columns
}

#[cfg(test)]
mod tests {
    use super::applies;

    #[test]
    fn a_binding_applies_when_every_token_is_on_the_focus_path() {
        let pane_with_one_file =
            |token: &str| ["Shell", "Pane", "selection", "single"].contains(&token);
        assert!(applies("Shell", &pane_with_one_file));
        assert!(applies("Pane && single", &pane_with_one_file));
        assert!(!applies("Pane && split", &pane_with_one_file));
        assert!(!applies("DocumentReader", &pane_with_one_file));
        assert!(applies("Pane && (split || single)", &pane_with_one_file));
        assert!(!applies("Pane && (split || empty)", &pane_with_one_file));
    }

    /// Rename and Delete say what must not hold (`!refs !restores`):
    /// F1 lists them where it does not.
    #[test]
    fn a_negated_token_holds_where_its_token_does_not() {
        let pane_with_one_file =
            |token: &str| ["Shell", "Pane", "selection", "single"].contains(&token);
        assert!(applies(
            "Pane && selection && !restores",
            &pane_with_one_file
        ));
        assert!(!applies("Pane && !single", &pane_with_one_file));
        assert!(applies("Pane && (!single || split)", &|t| t == "Pane"
            || t == "split"));
    }
}
