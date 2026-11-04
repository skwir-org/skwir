//! The History (ADR-079, 13): every action Undo can take back, newest
//! first, each with its own Undo, an undone one's Redo, opened above the
//! footer's Undo, split in two: its left part takes back the latest at
//! once, as Ctrl+Z does; its chevron (and a right click on Undo, and
//! `ui.undo_history`, Ctrl+Alt+Z) opens the History. Like the rest of the
//! footer, the two are no stops of the Tab order: their keys are the
//! commands', and a click on them leaves the keyboard where it was.
//!
//! An action whose Undo cannot take it back alone says which action after
//! it touched what it did, by a name that leads to that one's row, and
//! offers to take them back together, newest first. What each row can do
//! is the core's answer (`Executor::history`), read again only when its
//! version moves (`Read`), with each row by the runs that made it: what a
//! toast's and `Earlier`'s Undo ask (`undo_held`). Only the rows in view
//! are built; the keys walk the rows read, so one out of view is reached
//! too, and comes into view.

use crate::sizes::TextSizes;
use crate::sizes::sized;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use gpui::{
    AnyElement, Context, FocusHandle, ListOffset, SharedString, Window, div, prelude::*, px,
};
use skwir_core::op::{Can, Done, History, HistoryRow, PlanId, Since};

use crate::activity::{Press, stop};
use crate::controls::{self, ButtonStyle};
use crate::elements::popover::PopoverAnchor;
use crate::format;
use crate::keymap::RunCommand;
use crate::shell::Shell;
use crate::theme::Theme;
use crate::toast::{Action, capitalized};

/// The History's popup, as `Shell::toggle_popup` and `toolbar_menu` know it.
pub const POPUP: &str = "undo-history";

/// A row's height until it is measured: one line, as most are.
const ROW_HINT: f32 = 28.;

/// What is measured beyond the rows in view.
const OVERDRAW: f32 = 60.;

/// What a focus of a row is: the row's own (no stop of the Tab order: what
/// holds its buttons, which the list keeps drawn while one of them has the
/// keyboard), its Undo or Redo, its Undo with what came after, and the name
/// of the action that holds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Part {
    Row,
    Main,
    WithAfter,
    Blocker,
}

/// The history as the core said it at one version, read once for it: each
/// row by the runs that made it, and each row's place by its entry.
struct Read {
    history: History,
    by_plan: Rc<HashMap<PlanId, HistoryRow>>,
    at: HashMap<u64, usize>,
}

impl Read {
    fn of(history: History) -> Read {
        let by_plan = history
            .rows
            .iter()
            .flat_map(|row| row.plans.iter().map(move |plan| (*plan, row.clone())))
            .collect();
        let at = history
            .rows
            .iter()
            .enumerate()
            .map(|(at, row)| (row.entry, at))
            .collect();
        Read {
            history,
            by_plan: Rc::new(by_plan),
            at,
        }
    }
}

/// The History as the window shows it.
pub struct HistoryState {
    anchor: PopoverAnchor,
    /// What the core said, kept while its version stays.
    read: Option<Rc<Read>>,
    /// The read the rows are listed from (`list`): the one drawn.
    listed: Option<Rc<Read>>,
    /// The footer's Undo and its chevron.
    undo: Option<FocusHandle>,
    more: Option<FocusHandle>,
    /// Each row's focus and its buttons', by entry and part, kept for the
    /// rows read: the keys find them where they were as rows come and go.
    buttons: HashMap<(u64, Part), FocusHandle>,
    /// The rows: only those in view are built.
    list: gpui::ListState,
    /// The row a blocker's name led to: lit until the popup closes.
    lit: Option<u64>,
}

impl Default for HistoryState {
    fn default() -> Self {
        HistoryState {
            anchor: PopoverAnchor::default(),
            read: None,
            listed: None,
            undo: None,
            more: None,
            buttons: HashMap::new(),
            list: gpui::ListState::new(0, gpui::ListAlignment::Top, px(OVERDRAW)),
            lit: None,
        }
    }
}

/// A key that moves the keyboard among the History's buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Walk {
    Next,
    Prev,
    First,
    Last,
}

impl Walk {
    fn of(command: &str) -> Option<Walk> {
        Some(match command {
            "ui.toolbar_next" => Walk::Next,
            "ui.toolbar_prev" => Walk::Prev,
            "ui.toolbar_first" => Walk::First,
            "ui.toolbar_last" => Walk::Last,
            _ => return None,
        })
    }
}

/// The buttons of `rows` in the order the keys walk them, each with its
/// row's place: a row's Undo with what came after and its Undo (or Redo),
/// then the name of the action that holds it, on the line under them.
fn stops(rows: &[HistoryRow]) -> Vec<(usize, u64, Part)> {
    rows.iter()
        .enumerate()
        .flat_map(|(at, row)| {
            let with = (!row.with.is_empty()).then_some((at, row.entry, Part::WithAfter));
            let blocker =
                matches!(row.can, Can::After { .. }).then_some((at, row.entry, Part::Blocker));
            with.into_iter()
                .chain([(at, row.entry, Part::Main)])
                .chain(blocker)
        })
        .collect()
}

/// The stop `walk` goes to among `count`, from the one at `at` (none has
/// the keyboard: the popup's own focus does): round the ends, as Tab in a
/// dialog goes.
fn walked(walk: Walk, at: Option<usize>, count: usize) -> Option<usize> {
    let last = count.checked_sub(1)?;
    Some(match (walk, at) {
        (Walk::First, _) | (Walk::Next, None) => 0,
        (Walk::Last, _) | (Walk::Prev, None | Some(0)) => last,
        (Walk::Next, Some(at)) => {
            if at >= last {
                0
            } else {
                at + 1
            }
        }
        (Walk::Prev, Some(at)) => at - 1,
    })
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
            self.undo_history.lit = None;
            // From the newest, the rows listed again.
            self.undo_history.listed = None;
            self.undo_history.list.scroll_to(ListOffset::default());
        }
        cx.notify();
    }

    /// The history as the core says it, read again only when it may say
    /// otherwise (`Executor::history_version`).
    fn history_read(&mut self) -> Rc<Read> {
        let version = self.app.executor.history_version();
        if let Some(read) = &self.undo_history.read
            && read.history.version == version
        {
            return Rc::clone(read);
        }
        let read = Rc::new(Read::of(self.app.executor.history()));
        self.undo_history.read = Some(Rc::clone(&read));
        read
    }

    /// Each row of the history by the runs that made it: what a toast's
    /// or `Earlier`'s Undo names. Made once a version of the history.
    pub(crate) fn history_by_plan(&mut self) -> Rc<HashMap<PlanId, HistoryRow>> {
        Rc::clone(&self.history_read().by_plan)
    }

    /// List the rows of `read`, when they are not listed already: each
    /// row's focus and its buttons' kept for the rows read, and the row at
    /// the top kept there as rows come before it, unless the newest was
    /// shown: then the newest still is.
    fn list_history(&mut self, read: &Rc<Read>, cx: &mut Context<Self>) {
        let state = &mut self.undo_history;
        if state
            .listed
            .as_ref()
            .is_some_and(|listed| Rc::ptr_eq(listed, read))
        {
            return;
        }
        let top = state.list.logical_scroll_top();
        let kept = state
            .listed
            .as_ref()
            .filter(|_| top.item_ix > 0 || top.offset_in_item > px(0.))
            .and_then(|listed| listed.history.rows.get(top.item_ix))
            .map(|row| row.entry);
        let rows = &read.history.rows;
        let parts: HashSet<(u64, Part)> = stops(rows)
            .into_iter()
            .map(|(_, entry, part)| (entry, part))
            .chain(rows.iter().map(|row| (row.entry, Part::Row)))
            .collect();
        state.buttons.retain(|key, _| parts.contains(key));
        for key in parts {
            state.buttons.entry(key).or_insert_with(|| match key.1 {
                Part::Row => cx.focus_handle(),
                _ => stop(cx),
            });
        }
        let focus: Vec<Option<FocusHandle>> = rows
            .iter()
            .map(|row| state.buttons.get(&(row.entry, Part::Row)).cloned())
            .collect();
        let at = kept
            .and_then(|entry| read.at.get(&entry))
            .map(|at| ListOffset {
                item_ix: *at,
                offset_in_item: top.offset_in_item,
            });
        relist(&state.list, focus, at);
        state.listed = Some(Rc::clone(read));
    }

    /// The keys among the History's buttons (`Walk`), by the rows read:
    /// one out of view is reached too, and comes into view.
    fn walk_history(&mut self, walk: Walk, window: &mut Window, cx: &mut Context<Self>) {
        let Some(read) = self.undo_history.listed.clone() else {
            return;
        };
        let stops = stops(&read.history.rows);
        let at = stops.iter().position(|(_, entry, part)| {
            self.undo_history
                .buttons
                .get(&(*entry, *part))
                .is_some_and(|focus| focus.is_focused(window))
        });
        if let Some((row, entry, part)) = walked(walk, at, stops.len()).map(|next| stops[next]) {
            self.focus_history(row, entry, part, window, cx);
        }
    }

    /// Give the keyboard to the button `part` of the row `entry`, at `row`
    /// in the rows listed, bringing the row into view: the least that
    /// shows it once drawn, at the top before.
    fn focus_history(
        &mut self,
        row: usize,
        entry: u64,
        part: Part,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(focus) = self.undo_history.buttons.get(&(entry, part)).cloned() else {
            return;
        };
        let list = &self.undo_history.list;
        if list.bounds_for_item(row).is_some() {
            list.scroll_to_reveal_item(row);
        } else {
            list.scroll_to(ListOffset {
                item_ix: row,
                offset_in_item: px(0.),
            });
        }
        window.focus(&focus, cx);
        cx.notify();
    }
}

/// Put a row in `list` for each of `focus` in place of those it had, at
/// `top` (the newest when `None`). Where it stands is said after the
/// splice, never left to it: rows put in before the top push the top down
/// with them, so the first rows put in an empty list (the History opened
/// the first time) would leave it at its oldest.
fn relist(list: &gpui::ListState, focus: Vec<Option<FocusHandle>>, top: Option<ListOffset>) {
    let count = list.item_count();
    list.splice_focusable(0..count, focus);
    // A height until each is measured: the list's height from the first
    // frame, and its scrollbar.
    let _ = list.clone().with_uniform_item_height(px(ROW_HINT));
    list.scroll_to(top.unwrap_or_default());
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

/// Why `action`, an answer's Undo (a toast's, `Earlier`'s), cannot take
/// its plan back now, as the History says it (`by_plan`): said, not
/// offered, rather than refused once pressed. `None` for another action,
/// or one it takes back.
pub(crate) fn undo_held(action: &Action, by_plan: &HashMap<PlanId, HistoryRow>) -> Option<String> {
    if action.command != "undo" {
        return None;
    }
    let plan: PlanId = serde_json::from_value(action.args.get("plan")?.clone()).ok()?;
    match by_plan.get(&plan) {
        Some(row) if row.state == Done::Undone => Some("Undone already".to_owned()),
        Some(row) if can_now(row) => None,
        Some(row) => why(row),
        None => Some("No longer in the history".to_owned()),
    }
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
        // A click leaves the keyboard where it was, as on the strip: the
        // keys go on acting on the files (Enter would press Undo again).
        .capture_any_mouse_down(|_, window, _| window.prevent_default())
        .flex()
        .flex_none()
        .items_center()
        .gap_1()
        .h(sized(20.))
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
        // The keyboard stays where it was: the History gives it back there.
        .capture_any_mouse_down(|_, window, _| window.prevent_default())
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .h(sized(20.))
        .w(px(18.))
        .rounded_r_md()
        .border_1()
        .border_color(gpui::transparent_black())
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .focus(move |s| s.border_color(accent))
        .when(open, |d| d.bg(theme.selection_bg))
        .tooltip(move |_, cx| crate::toolbar::tooltip(history_tip.clone(), cx))
        // A chevron points where its menu opens: up to the History,
        // down to fold it back.
        .child(controls::glyph(
            if open { "chevron" } else { "chevron-up" },
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
            .child(div().w(px(1.)).h(sized(12.)).bg(theme.border))
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
    let read = shell.history_read();
    shell.list_history(&read, cx);
    let count = read.history.rows.len();
    let a11y = shell.a11y;
    let title = div()
        .flex_none()
        .px_3()
        .pt_2()
        .pb_1()
        .text_small()
        .text_color(theme.text_muted)
        .child("Undo history");
    let rows = if count == 0 {
        div()
            .flex_none()
            .px_3()
            .py_2()
            .text_body()
            .text_color(theme.text_muted)
            .child("Nothing to undo yet")
            .into_any_element()
    } else {
        // Only the rows in view are built (`history_row`), the rows read
        // kept as they were drawn (`HistoryState::listed`).
        let list = gpui::list(
            shell.undo_history.list.clone(),
            cx.processor(|shell: &mut Shell, at: usize, _, cx| {
                let theme = cx.global::<Theme>().clone();
                match shell.undo_history.listed.clone() {
                    Some(read) => history_row(shell, &read, at, &theme, cx),
                    None => div().into_any_element(),
                }
            }),
        )
        .with_sizing_behavior(gpui::ListSizingBehavior::Infer)
        .min_h_0();
        div()
            .id("undo-history-rows")
            .role(gpui::Role::List)
            .when(a11y, |d| {
                d.aria_label("Undo history").aria_size_of_set(count)
            })
            .relative()
            .flex()
            .flex_col()
            .min_h_0()
            .child(list)
            .child(crate::elements::scrollbar::vertical(
                "undo-history-scrollbar",
                crate::elements::scrollbar::Scrolls::Items(shell.undo_history.list.clone()),
            ))
            .into_any_element()
    };
    let popup = shell.toolbar_menu.as_ref()?;
    let panel = crate::toolbar::popup_panel(popup, "ui.undo_history", theme, cx)
        .role(gpui::Role::Group)
        .aria_label("Undo history")
        // Its rows scroll, in their list.
        .overflow_hidden()
        .w(sized(420.))
        .max_h(sized(520.))
        .p_0()
        .gap_0()
        // The keys walk the rows read, not only those drawn.
        .on_action(cx.listener(|this, action: &RunCommand, window, cx| {
            match Walk::of(&action.name) {
                Some(walk) => this.walk_history(walk, window, cx),
                None => cx.propagate(),
            }
        }))
        .child(title)
        .child(rows)
        .child(div().flex_none().h(px(6.)));
    Some(shell.undo_history.anchor.popup_above_for(
        &crate::elements::adaptive::Chosen::default(),
        vec![panel.into_any_element()],
    ))
}

/// The action at `at` of `read`: its state, title and time, why its Undo
/// waits or cannot (the action that holds it named, leading to its row),
/// and its buttons.
fn history_row(
    shell: &Shell,
    read: &Read,
    at: usize,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let Some(row) = read.history.rows.get(at) else {
        return div().into_any_element();
    };
    let entry = row.entry;
    let focus = |part: Part| shell.undo_history.buttons.get(&(entry, part)).cloned();
    let lit = shell.undo_history.lit == Some(entry);
    let a11y = shell.a11y;
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
    // What a screen reader hears of the row, built only while one listens.
    let label: Option<SharedString> = a11y.then(|| {
        [
            Some(title.clone()),
            pill.map(str::to_owned),
            why_said.clone(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(", ")
        .into()
    });
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
                    .text_tiny()
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
                    .text_body()
                    .when(undone, |d| d.text_color(theme.text_muted)),
                title,
            )),
    );
    // Why it waits or cannot, on a line of its own under the title: the
    // action that holds it named, a button that leads to its row.
    let reason = match &row.can {
        Can::After {
            entry: holder,
            title: holder_title,
            since,
        } => focus(Part::Blocker).map(|link| {
            let (before, after) = held(*since);
            let holder = *holder;
            let named = capitalized(holder_title);
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .text_small()
                .text_color(theme.text_muted)
                .child(before)
                .child(
                    div()
                        .id(("undo-history-holder", entry))
                        .role(gpui::Role::Link)
                        .when(a11y, |d| d.aria_label(format!("Go to {named}")))
                        .track_focus(&link)
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
                        .child(format!("“{named}”"))
                        .press(
                            move |this, window, cx| {
                                // Its row lit, the keyboard on its Undo,
                                // which comes into view.
                                this.undo_history.lit = Some(holder);
                                let at = this
                                    .undo_history
                                    .listed
                                    .as_ref()
                                    .and_then(|read| read.at.get(&holder).copied());
                                if let Some(at) = at {
                                    this.focus_history(at, holder, Part::Main, window, cx);
                                }
                                cx.notify();
                            },
                            cx,
                        ),
                )
                .child(after)
        }),
        _ => why_said.map(|why| div().text_body().text_color(theme.text_muted).child(why)),
    };
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
            .h(sized(22.))
            .px_2()
            .text_color(theme.accent_text)
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
                .h(sized(22.))
                .px_2()
                .text_color(theme.accent_text)
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
                .h(sized(22.))
                .px_2()
                .border_color(gpui::transparent_black())
                .focus(move |s| s.border_color(accent))
                .into_any_element()
        };
        buttons = buttons.child(button);
    }
    let line = div()
        .flex()
        .items_center()
        .gap_2()
        .min_h(sized(26.))
        .child(glyph)
        .child(what)
        .child(
            div()
                .flex_none()
                .text_small()
                .text_color(theme.text_muted)
                .when(!matches!(row.state, Done::Running | Done::Waiting), |d| {
                    d.child(format::clock(row.at))
                }),
        )
        .child(buttons);
    let mut item = div()
        .id(("undo-history-row", entry))
        .role(gpui::Role::ListItem)
        .when_some(label, |d, label| {
            d.aria_label(label).aria_position_in_set(at)
        });
    if let Some(row_focus) = focus(Part::Row) {
        item = item.track_focus(&row_focus);
    }
    item
        // A click beside its buttons gives the row no keyboard: they take
        // it, as they are pressed.
        .on_mouse_down(gpui::MouseButton::Left, |_, window, _| {
            window.prevent_default();
        })
        .w_full()
        .flex()
        .flex_col()
        .py_0p5()
        .pl_3()
        .pr_1()
        .when(lit, |d| d.bg(theme.hover))
        .child(line)
        // Under the title, past the glyph.
        .children(reason.map(|reason| reason.pl(sized(21.)).pb_1()))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{OVERDRAW, Part, Walk, held, relist, stops, undo_held, walked, why};
    use crate::toast::Action;
    use skwir_core::op::{Can, Done, HistoryRow, Plan, PlanId, Since};
    use std::collections::HashMap;
    use std::time::SystemTime;

    fn row(entry: u64, state: Done, can: Can, with: Vec<u64>) -> HistoryRow {
        HistoryRow {
            entry,
            title: format!("rename to it's {entry}.txt"),
            plans: Vec::new(),
            at: SystemTime::UNIX_EPOCH,
            state,
            can,
            with,
        }
    }

    fn undo_of(plan: PlanId) -> Action {
        Action {
            label: "Undo".into(),
            command: "undo".into(),
            args: serde_json::json!({ "plan": plan }),
        }
    }

    /// A row's Undo that cannot act says why, naming the action that holds
    /// it as it stands; one it takes back says nothing.
    #[test]
    fn a_held_undo_says_why_by_how_the_one_holding_it_stands() {
        let after = |since| Can::After {
            entry: 9,
            title: "move “a.txt” to /example/b".into(),
            since,
        };
        let said = |state, can| why(&row(1, state, can, Vec::new()));
        assert_eq!(said(Done::Done, Can::Now), None);
        assert_eq!(
            said(Done::Done, after(Since::Done)).as_deref(),
            Some("Undo “Move “a.txt” to /example/b” first: it touched what this did")
        );
        assert_eq!(
            said(Done::Done, after(Since::Running)).as_deref(),
            Some("“Move “a.txt” to /example/b” is running, and touches what this did")
        );
        assert_eq!(
            said(Done::Undone, after(Since::Undone)).as_deref(),
            Some("Redo “Move “a.txt” to /example/b” first: it touched what this did")
        );
        assert_eq!(
            said(Done::Done, after(Since::ForGood)).as_deref(),
            Some("“Move “a.txt” to /example/b” took what this did for good")
        );
        assert_eq!(
            said(Done::Running, Can::OnceEnded).as_deref(),
            Some("Taken back once it ends")
        );
        assert_eq!(
            said(Done::Undone, Can::Going).as_deref(),
            Some("Being put back…")
        );
        assert_eq!(
            said(Done::Done, Can::Going).as_deref(),
            Some("Being undone…")
        );
        assert_eq!(
            said(Done::Done, Can::Never).as_deref(),
            Some("Cannot be undone")
        );
        // Every way an Undo is held reads as a sentence round the name.
        for since in [
            Since::Done,
            Since::Running,
            Since::Waiting,
            Since::Undone,
            Since::ForGood,
        ] {
            let (before, after) = held(since);
            assert!(after.starts_with(' '), "{since:?}");
            assert!(before.is_empty() || before.ends_with(' '), "{since:?}");
        }
    }

    /// An answer's Undo asks the History for the plan it reports: taken
    /// back now or once it ends, it is offered; undone already, held, or
    /// gone from the history, it says so; another button is not its.
    #[test]
    fn an_answers_undo_is_held_as_the_history_says() {
        let ids: Vec<PlanId> = (0..5).map(|n| Plan::new(format!("plan {n}")).id).collect();
        let mut by_plan: HashMap<PlanId, HistoryRow> = HashMap::new();
        by_plan.insert(ids[0], row(1, Done::Done, Can::Now, Vec::new()));
        by_plan.insert(ids[1], row(2, Done::Running, Can::OnceEnded, Vec::new()));
        by_plan.insert(ids[2], row(3, Done::Undone, Can::Now, Vec::new()));
        by_plan.insert(
            ids[3],
            row(
                4,
                Done::Done,
                Can::After {
                    entry: 7,
                    title: "copy to /example".into(),
                    since: Since::Waiting,
                },
                Vec::new(),
            ),
        );
        assert_eq!(undo_held(&undo_of(ids[0]), &by_plan), None);
        assert_eq!(undo_held(&undo_of(ids[1]), &by_plan), None);
        assert_eq!(
            undo_held(&undo_of(ids[2]), &by_plan).as_deref(),
            Some("Undone already")
        );
        assert_eq!(
            undo_held(&undo_of(ids[3]), &by_plan).as_deref(),
            Some("“Copy to /example” waits its turn, and touches what this did")
        );
        assert_eq!(
            undo_held(&undo_of(ids[4]), &by_plan).as_deref(),
            Some("No longer in the history")
        );
        let retry = Action {
            label: "Retry".into(),
            command: "retry".into(),
            args: serde_json::json!({ "plan": ids[3] }),
        };
        assert_eq!(undo_held(&retry, &by_plan), None);
        // An Undo naming no plan (the last asked) is the core's to answer.
        let last = Action {
            label: "Undo".into(),
            command: "undo".into(),
            args: serde_json::json!({}),
        };
        assert_eq!(undo_held(&last, &by_plan), None);
    }

    /// The keys walk every row's buttons in reading order, rows out of
    /// view included, round the ends; from the popup's own focus, Down
    /// takes the first and Up the last.
    #[test]
    fn the_keys_walk_the_rows_read_round_their_ends() {
        let rows = [
            row(
                5,
                Done::Done,
                Can::After {
                    entry: 6,
                    title: "rename".into(),
                    since: Since::Done,
                },
                vec![6],
            ),
            row(4, Done::Undone, Can::Now, Vec::new()),
        ];
        assert_eq!(
            stops(&rows),
            [
                (0, 5, Part::WithAfter),
                (0, 5, Part::Main),
                (0, 5, Part::Blocker),
                (1, 4, Part::Main),
            ]
        );
        assert_eq!(walked(Walk::Next, None, 4), Some(0));
        assert_eq!(walked(Walk::Prev, None, 4), Some(3));
        assert_eq!(walked(Walk::Next, Some(1), 4), Some(2));
        assert_eq!(walked(Walk::Next, Some(3), 4), Some(0));
        assert_eq!(walked(Walk::Prev, Some(0), 4), Some(3));
        assert_eq!(walked(Walk::First, Some(2), 4), Some(0));
        assert_eq!(walked(Walk::Last, Some(0), 4), Some(3));
        assert_eq!(walked(Walk::Next, None, 0), None);
        assert_eq!(Walk::of("ui.toolbar_close"), None);
    }

    /// The History opens at its newest, its first time too (its list was
    /// empty), and a row kept at the top stays there as rows come.
    #[test]
    fn the_history_opens_at_its_newest_and_keeps_its_top_row() {
        use gpui::{ListOffset, px};
        let list = gpui::ListState::new(0, gpui::ListAlignment::Top, px(OVERDRAW));
        list.scroll_to(ListOffset::default());
        relist(&list, vec![None; 30], None);
        assert_eq!(list.logical_scroll_top(), ListOffset::default());
        let kept = ListOffset {
            item_ix: 6,
            offset_in_item: px(4.),
        };
        relist(&list, vec![None; 31], Some(kept));
        assert_eq!(list.logical_scroll_top(), kept);
        relist(&list, vec![None; 32], None);
        assert_eq!(list.logical_scroll_top(), ListOffset::default());
    }
}
