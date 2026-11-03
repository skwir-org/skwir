//! The History (ADR-079, 13): every action Undo can take back, newest
//! first, each with its own Undo, an undone one's Redo, opened above the
//! footer's Undo, split in two: its left part takes back the latest at
//! once, as Ctrl+Z does; its chevron (and a right click on Undo, and
//! `ui.undo_history`, Ctrl+Alt+Z) opens the History. Like the rest of the
//! footer, the two are no stops of the Tab order: their keys are the
//! commands'.
//!
//! An action whose Undo cannot take it back alone says which action after
//! it touched what it did, by a name that leads to that one's row, and
//! offers to take them back together, newest first. What each row can do
//! is the core's answer (`Executor::history`), read again only when its
//! version moves: nothing is compared as the History is drawn.

use std::collections::HashMap;

use gpui::{AnyElement, Context, FocusHandle, SharedString, Window, div, prelude::*, px};
use skwir_core::op::{Can, Done, History, HistoryRow, PlanId, Since};

use crate::activity::{Press, Reveal, stop};
use crate::controls::{self, ButtonStyle};
use crate::elements::popover::PopoverAnchor;
use crate::format;
use crate::shell::Shell;
use crate::theme::Theme;
use crate::toast::capitalized;

/// The History's popup, as `Shell::toggle_popup` and `toolbar_menu` know it.
pub const POPUP: &str = "undo-history";

/// What a row's button is: its Undo or Redo, its Undo with what came
/// after, and the name of the action that holds it.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Part {
    Main,
    WithAfter,
    Blocker,
}

/// The History as the window shows it.
#[derive(Default)]
pub struct HistoryState {
    anchor: PopoverAnchor,
    /// What the core said, kept while its version stays.
    read: Option<History>,
    /// The footer's Undo and its chevron.
    undo: Option<FocusHandle>,
    more: Option<FocusHandle>,
    /// Each row's buttons, by entry and part, kept for the rows read.
    buttons: HashMap<(u64, Part), FocusHandle>,
    /// What brings the button the keyboard reaches into view, made again
    /// as the popup opens.
    reveal: Reveal,
    /// The row a blocker's name led to: lit until the popup closes.
    lit: Option<u64>,
}

impl Shell {
    /// Whether the History is open.
    pub(crate) fn undo_history_open(&self) -> bool {
        self.toolbar_menu
            .as_ref()
            .is_some_and(|popup| popup.kind == POPUP)
    }

    /// `ui.undo_history`: open the History above the footer's Undo, or
    /// close it (`args`: `close`, `pointer`, as every popup's toggle).
    pub(crate) fn toggle_undo_history(
        &mut self,
        args: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.toggle_popup(POPUP, args, window, cx) {
            self.toolbar_menu = Some(crate::toolbar::Popup::new(POPUP, window, cx));
            self.undo_history.reveal = Reveal::default();
            self.undo_history.lit = None;
        }
        cx.notify();
    }

    /// The history as the core says it, read again only when it may say
    /// otherwise (`Executor::history_version`).
    pub(crate) fn history(&mut self) -> &History {
        let version = self.app.executor.history_version();
        let stale = self
            .undo_history
            .read
            .as_ref()
            .is_none_or(|read| read.version != version);
        if stale {
            self.undo_history.read = Some(self.app.executor.history());
        }
        self.undo_history.read.get_or_insert_with(History::default)
    }

    /// Each row of the history by the runs that made it: what a toast's
    /// or `Earlier`'s Undo names.
    pub(crate) fn history_by_plan(&mut self) -> HashMap<PlanId, HistoryRow> {
        self.history()
            .rows
            .iter()
            .flat_map(|row| row.plans.iter().map(move |plan| (*plan, row.clone())))
            .collect()
    }
}

/// Whether `row`'s Undo (an undone one's Redo) does something pressed now:
/// at once, or once it ends.
pub(crate) fn can_now(row: &HistoryRow) -> bool {
    matches!(row.can, Can::Now | Can::OnceEnded)
}

/// Why `row`'s Undo (Redo) waits or cannot, in words; `None` when it
/// takes it back at once.
pub(crate) fn why(row: &HistoryRow) -> Option<String> {
    let undone = row.state == Done::Undone;
    Some(match &row.can {
        Can::Now => return None,
        Can::OnceEnded => "Taken back once it ends".to_owned(),
        Can::Going if undone => "Being put back…".to_owned(),
        Can::Going => "Being undone…".to_owned(),
        Can::Never => "Cannot be undone".to_owned(),
        Can::After { title, since, .. } => {
            let (before, after) = held(*since);
            format!("{before}“{}”{after}", capitalized(title))
        }
    })
}

/// What is said before and after the name of the action that holds an
/// Undo, by how that one stands.
fn held(since: Since) -> (&'static str, &'static str) {
    match since {
        Since::Done => ("Undo ", " first: it touched what this did"),
        Since::Running => ("", " is running, and touches what this did"),
        Since::Waiting => ("", " waits its turn, and touches what this did"),
        Since::Undone => ("Redo ", " first: it touched what this did"),
        Since::ForGood => ("", " took what this did for good"),
    }
}

/// The footer's Undo, split in two (the left part takes back the latest,
/// the chevron opens the History), with the History above it when open;
/// `None` while there is nothing to undo or redo.
pub fn undo_control(
    shell: &mut Shell,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> Option<AnyElement> {
    let target = shell.app.executor.undo_target();
    let open = shell.undo_history_open();
    if target.is_none() && !shell.app.executor.can_redo() && !open {
        return None;
    }
    // As the strip's: no stops of the Tab order (module documentation).
    let undo = shell
        .undo_history
        .undo
        .get_or_insert_with(|| cx.focus_handle())
        .clone();
    let more = shell
        .undo_history
        .more
        .get_or_insert_with(|| cx.focus_handle())
        .clone();
    let keys = |shell: &Shell, command: &str| {
        shell
            .keys_for(command, &serde_json::Value::Null)
            .map_or_else(String::new, |keys| format!(" ({keys})"))
    };
    let enabled = target.is_some();
    let tip: SharedString = match &target {
        Some((title, running)) => {
            let once = if *running { " (once it ends)" } else { "" };
            format!("Undo {title}{once}{}", keys(shell, "undo")).into()
        }
        None => "Nothing to undo".into(),
    };
    let history_tip: SharedString =
        format!("Undo history{}", keys(shell, "ui.undo_history")).into();
    let (accent, hover) = (theme.accent, theme.hover);
    let color = if enabled {
        theme.text
    } else {
        theme.text_muted
    };
    let left = div()
        .id("status-undo")
        .role(gpui::Role::Button)
        .aria_label(tip.clone())
        .when(!enabled, |d| d.aria_disabled(true))
        .track_focus(&undo)
        .flex()
        .flex_none()
        .items_center()
        .gap_1()
        .h(px(20.))
        .px_2()
        .rounded_l_md()
        .border_1()
        .border_color(gpui::transparent_black())
        .text_color(color)
        .when(enabled, |d| d.cursor_pointer().hover(move |s| s.bg(hover)))
        .focus(move |s| s.border_color(accent))
        .tooltip(move |_, cx| crate::toolbar::tooltip(tip.clone(), cx))
        // A right click opens the History.
        .on_mouse_down(
            gpui::MouseButton::Right,
            cx.listener(|this, _, window, cx| {
                window.prevent_default();
                this.run("ui.undo_history", serde_json::json!({}), window, cx);
            }),
        )
        .child(controls::glyph("revert", theme.text_muted, 13.))
        .child("Undo")
        .press(
            move |this, window, cx| {
                if enabled {
                    this.run("undo", serde_json::json!({}), window, cx);
                }
            },
            cx,
        );
    let chevron = div()
        .id("status-undo-history")
        .role(gpui::Role::Button)
        .aria_label(history_tip.clone())
        .aria_expanded(open)
        .track_focus(&more)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .h(px(20.))
        .w(px(18.))
        .rounded_r_md()
        .border_1()
        .border_color(gpui::transparent_black())
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .focus(move |s| s.border_color(accent))
        .when(open, |d| d.bg(theme.selection_bg))
        .tooltip(move |_, cx| crate::toolbar::tooltip(history_tip.clone(), cx))
        .child(controls::glyph(
            if open { "up" } else { "chevron" },
            theme.text_muted,
            11.,
        ))
        .press(
            |this, window, cx| {
                this.run("ui.undo_history", serde_json::json!({}), window, cx);
            },
            cx,
        );
    let popup = popover(shell, theme, cx);
    Some(
        div()
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .child(shell.undo_history.anchor.marker())
            .child(left)
            .child(div().w(px(1.)).h(px(12.)).bg(theme.border))
            .child(chevron)
            .children(popup)
            .into_any_element(),
    )
}

/// The History, opened above the footer's Undo.
fn popover(shell: &mut Shell, theme: &Theme, cx: &mut Context<Shell>) -> Option<AnyElement> {
    if !shell.undo_history_open() {
        return None;
    }
    let rows = shell.history().rows.clone();
    // Each row's buttons, kept for the rows read: Tab finds them where
    // they were as rows come and go.
    let parts: std::collections::HashSet<(u64, Part)> = rows
        .iter()
        .flat_map(|row| {
            let mut parts = vec![(row.entry, Part::Main)];
            if !row.with.is_empty() {
                parts.push((row.entry, Part::WithAfter));
            }
            if matches!(row.can, Can::After { .. }) {
                parts.push((row.entry, Part::Blocker));
            }
            parts
        })
        .collect();
    shell
        .undo_history
        .buttons
        .retain(|key, _| parts.contains(key));
    for key in parts {
        shell
            .undo_history
            .buttons
            .entry(key)
            .or_insert_with(|| stop(cx));
    }
    let reveal = shell.undo_history.reveal.clone();
    let a11y = shell.a11y;
    let lit = shell.undo_history.lit;
    let mut body: Vec<AnyElement> = vec![
        div()
            .flex_none()
            .px_3()
            .pt_2()
            .pb_1()
            .text_xs()
            .text_color(theme.text_muted)
            .child("Undo history")
            .into_any_element(),
    ];
    if rows.is_empty() {
        body.push(
            div()
                .px_3()
                .py_2()
                .text_xs()
                .text_color(theme.text_muted)
                .child("Nothing to undo yet")
                .into_any_element(),
        );
    } else {
        let built: Vec<AnyElement> = rows
            .iter()
            .map(|row| history_row(shell, row, lit == Some(row.entry), &reveal, theme, cx))
            .collect();
        body.push(
            div()
                .id("undo-history-rows")
                .role(gpui::Role::List)
                .when(a11y, |d| d.aria_label("Undo history"))
                .flex()
                .flex_col()
                .children(built)
                .into_any_element(),
        );
    }
    body.push(div().flex_none().h(px(6.)).into_any_element());
    let popup = shell.toolbar_menu.as_ref()?;
    let panel = crate::toolbar::popup_panel(popup, "ui.undo_history", theme, cx)
        .role(gpui::Role::Group)
        .aria_label("Undo history")
        .track_scroll(&reveal.scroll)
        .w(px(420.))
        .max_h(px(520.))
        .p_0()
        .gap_0()
        .children(body);
    Some(shell.undo_history.anchor.popup_above_for(
        &crate::elements::adaptive::Chosen::default(),
        vec![panel.into_any_element()],
    ))
}

/// One action of the History: its state, title and time, why its Undo
/// waits or cannot (the action that holds it named, leading to its row),
/// and its buttons.
fn history_row(
    shell: &Shell,
    row: &HistoryRow,
    lit: bool,
    reveal: &Reveal,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let entry = row.entry;
    let focus = |part: Part| shell.undo_history.buttons.get(&(entry, part)).cloned();
    let undone = row.state == Done::Undone;
    let glyph = match row.state {
        Done::Done => controls::glyph("check", theme.success, 13.),
        Done::Undone => controls::glyph("revert", theme.text_muted, 13.),
        Done::Running | Done::Waiting => controls::glyph("busy", theme.text_muted, 13.),
    };
    let pill = match row.state {
        Done::Running => Some("running"),
        Done::Waiting => Some("waiting"),
        Done::Undone => Some("undone"),
        Done::Done => None,
    };
    let title = capitalized(&row.title);
    let why_said = why(row);
    let label: SharedString = [
        Some(title.clone()),
        pill.map(str::to_owned),
        why_said.clone(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(", ")
    .into();
    let what = div().flex().flex_1().min_w_0().child(
        div()
            .flex()
            .flex_1()
            .items_center()
            .gap_1()
            .min_w_0()
            .children(pill.map(|pill| {
                div()
                    .flex_none()
                    .px_1()
                    .rounded_sm()
                    .bg(theme.hover)
                    .text_color(theme.text_muted)
                    .text_size(px(10.))
                    .child(pill)
            }))
            .child(crate::elements::clip::clipped(
                div()
                    .id(("undo-history-title", entry))
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_xs()
                    .when(undone, |d| d.text_color(theme.text_muted)),
                title,
            )),
    );
    // Why it waits or cannot, on a line of its own under the title: the
    // action that holds it named, a button that leads to its row.
    let mut reason = None;
    match &row.can {
        Can::After {
            entry: holder,
            title: holder_title,
            since,
        } => {
            let (before, after) = held(*since);
            let holder = *holder;
            if let Some(link) = focus(Part::Blocker) {
                reason = Some(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .text_xs()
                        .text_color(theme.text_muted)
                        .child(before)
                        .child(
                            div()
                                .id(("undo-history-holder", entry))
                                .role(gpui::Role::Link)
                                .aria_label(format!("Go to {}", capitalized(holder_title)))
                                .track_focus(&link)
                                .relative()
                                .px_0p5()
                                .rounded_sm()
                                .border_1()
                                .border_color(gpui::transparent_black())
                                .text_color(theme.accent_text)
                                .underline()
                                .cursor_pointer()
                                .focus({
                                    let accent = theme.accent;
                                    move |s| s.border_color(accent)
                                })
                                .child(reveal.marker(&link))
                                .child(format!("“{}”", capitalized(holder_title)))
                                .press(
                                    move |this, window, cx| {
                                        // Its row lit, the keyboard on its
                                        // Undo, which comes into view.
                                        this.undo_history.lit = Some(holder);
                                        if let Some(target) =
                                            this.undo_history.buttons.get(&(holder, Part::Main))
                                        {
                                            window.focus(target, cx);
                                        }
                                        cx.notify();
                                    },
                                    cx,
                                ),
                        )
                        .child(after),
                );
            }
        }
        _ => {
            reason = why_said.map(|why| div().text_xs().text_color(theme.text_muted).child(why));
        }
    }
    let mut buttons = div().flex().flex_none().items_center().gap_1();
    if !row.with.is_empty()
        && let Some(with) = focus(Part::WithAfter)
    {
        let words = match row.with.len() {
            1 => "Undo both".to_owned(),
            n => format!("Undo with {n} after it"),
        };
        buttons = buttons.child(
            controls::button(
                SharedString::from(format!("undo-history-with-{entry}")),
                words,
                None,
                ButtonStyle::Plain,
                &with,
                theme,
            )
            .relative()
            .h(px(22.))
            .px_2()
            .text_color(theme.accent_text)
            .child(reveal.marker(&with))
            .press(
                move |this, window, cx| {
                    this.run(
                        "undo",
                        serde_json::json!({ "entry": entry, "with_after": true }),
                        window,
                        cx,
                    );
                },
                cx,
            ),
        );
    }
    if let Some(main) = focus(Part::Main) {
        let (word, command) = if undone {
            ("Redo", "redo")
        } else {
            ("Undo", "undo")
        };
        let id = SharedString::from(format!("undo-history-main-{entry}"));
        let button = if can_now(row) {
            controls::button(id, word, None, ButtonStyle::Plain, &main, theme)
                .relative()
                .h(px(22.))
                .px_2()
                .text_color(theme.accent_text)
                .child(reveal.marker(&main))
                .press(
                    move |this, window, cx| {
                        this.run(command, serde_json::json!({ "entry": entry }), window, cx);
                    },
                    cx,
                )
                .into_any_element()
        } else {
            // Said, not offered: its row says why. It keeps its place in
            // the Tab order, where a blocker's name leads; no frame but
            // the keyboard's, unlike what can be pressed.
            let accent = theme.accent;
            controls::inert_button(id, word, &main, theme)
                .relative()
                .h(px(22.))
                .px_2()
                .border_color(gpui::transparent_black())
                .focus(move |s| s.border_color(accent))
                .child(reveal.marker(&main))
                .into_any_element()
        };
        buttons = buttons.child(button);
    }
    let line = div()
        .flex()
        .items_center()
        .gap_2()
        .min_h(px(26.))
        .child(glyph)
        .child(what)
        .child(
            div()
                .flex_none()
                .text_xs()
                .text_color(theme.text_muted)
                .when(!matches!(row.state, Done::Running | Done::Waiting), |d| {
                    d.child(format::clock(row.at))
                }),
        )
        .child(buttons);
    div()
        .id(("undo-history-row", entry))
        .role(gpui::Role::ListItem)
        .aria_label(label)
        .flex()
        .flex_col()
        .flex_none()
        .py_0p5()
        .pl_3()
        .pr_1()
        .when(lit, |d| d.bg(theme.hover))
        .child(line)
        // Under the title, past the glyph.
        .children(reason.map(|reason| reason.pl(px(21.)).pb_1()))
        .into_any_element()
}
