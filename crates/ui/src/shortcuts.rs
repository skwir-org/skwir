//! Learning the keys without reading a manual.
//!
//! - F1 lists the shortcuts that apply where the focus is right now, read
//!   from the focused element's key contexts, with the rest one click away.
//! - A command used three times with the pointer, when it has a shortcut,
//!   earns one toast naming it. Once per command, ever: the commands already
//!   hinted are kept with the settings.

use std::collections::HashMap;

use gpui::{AnyElement, Context, FocusHandle, MouseButton, Window, div, prelude::*, px};

use crate::controls::{self, ButtonStyle};
use crate::keymap::{self, KeyHint, RunCommand};
use crate::modal::{self, FocusReturn};
use crate::shell::Shell;
use crate::theme::Theme;
use crate::toast::{Toast, Tone};

/// Application data: commands whose shortcut hint was shown.
pub const HINTS_KEY: &str = "ui.hints_shown";
const USES_BEFORE_HINT: u32 = 3;

pub struct Overview {
    focus: FocusHandle,
    /// The "All shortcuts" and close buttons', kept so Tab can reach
    /// them.
    all: FocusHandle,
    close: FocusHandle,
    previous: FocusReturn,
    /// Place name and its bindings with what each does, the focused place
    /// first. Titled once, when the overview opens.
    groups: Vec<(String, Vec<(KeyHint, String)>)>,
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
        let mut groups: Vec<(String, Vec<(KeyHint, String)>)> = Vec::new();
        for &(i, hint) in &applying {
            if !wins(i, hint) {
                continue;
            }
            // Conditions narrow a place; the overview groups by the place.
            let place = keymap::context_label(hint.context.split("&&").next().unwrap_or_default());
            let titled = (hint.clone(), keymap::title_of(&self.app, hint));
            match groups.iter_mut().find(|(name, _)| *name == place) {
                Some((_, hints)) => hints.push(titled),
                None => groups.push((place, vec![titled])),
            }
        }
        // The innermost place is what the user is looking at.
        groups.sort_by_key(|(name, _)| name == "Anywhere");
        let previous = FocusReturn::capture(window, cx);
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        self.shortcuts = Some(Overview {
            focus,
            all: cx.focus_handle(),
            close: cx.focus_handle(),
            previous,
            groups,
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
                action: None,
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

pub fn render(shell: &Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let overview = shell.shortcuts.as_ref()?;
    let theme = cx.global::<Theme>().clone();
    let mut columns = div().flex().flex_wrap().gap_x_8().gap_y_4();
    let mut row = 0_usize;
    for (place, hints) in &overview.groups {
        let mut column = div().w(px(300.)).flex().flex_col().gap(px(3.)).child(
            div()
                .pb_1()
                .text_xs()
                .text_color(theme.text_muted)
                .child(place.clone()),
        );
        for (hint, title) in hints {
            let mut keys = div().flex().flex_none().items_center().gap(px(8.));
            for stroke in keymap::key_caps(&hint.keys) {
                let mut caps = div().flex().items_center().gap(px(3.));
                for cap in stroke {
                    caps = caps.child(controls::keycap(cap, &theme));
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
                        div().id(("shortcut", row)).flex_1().min_w_0().truncate(),
                        title.clone(),
                        shell.font.clone(),
                    ))
                    .child(keys),
            );
            row += 1;
        }
        columns = columns.child(column);
    }
    let all = overview.all.clone();
    // The lists scroll under the title, which keeps its buttons in sight.
    let panel = div()
        .max_w(px(1000.))
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
                modal::tab_among(&[&overview.all, &overview.close], delta, window, cx);
            }
        }))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_4()
                .child(div().text_lg().child("Shortcuts here"))
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
                                    this.shortcuts = None;
                                    this.run(
                                        "ui.settings",
                                        serde_json::json!({ "page": "keyboard" }),
                                        window,
                                        cx,
                                    );
                                },
                            )),
                        )
                        .child(
                            controls::icon_button(
                                "shortcuts-close",
                                "close",
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
                .child(columns),
        );
    Some(
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .occlude()
            .bg(theme.scrim)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.toggle_shortcuts(window, cx)),
            )
            .child(panel)
            .into_any_element(),
    )
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
