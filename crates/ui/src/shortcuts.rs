//! Learning the keys without reading a manual.
//!
//! - F1 lists the shortcuts that work where the focus is right now, read
//!   from the focused element's key contexts, with the rest one click away:
//!   one row per action, its keys side by side, a family of actions
//!   (`Form::family`) on one row, under what they are about
//!   (`CommandSpec::topic`), the topics of the place nearest the focus
//!   first, flowing down as many columns as fit (`elements::columns`).
//!   What is typed there narrows them and finds the other places' too, by
//!   what they do or by their keys (`keymap::finds`).
//! - A command used three times with the pointer, when it has a shortcut,
//!   earns one toast naming it. Once per command, ever: the commands already
//!   hinted are kept with the settings.

use std::collections::HashMap;
use std::rc::Rc;

use gpui::{
    AnyElement, Context, Entity, FocusHandle, Focusable, MouseButton, ScrollHandle, Subscription,
    Window, div, prelude::*, px,
};
use skwir_core::command::Topic;

use crate::controls::{self, ButtonStyle};
use crate::elements::columns::{self, Block, Columns, Metrics};
use crate::elements::scrollbar::{self, Scrolls};
use crate::input::{InputEvent, TextInput};
use crate::keymap::{self, KeyHint, RunCommand};
use crate::modal::{self, FocusReturn};
use crate::shell::Shell;
use crate::theme::Theme;
use crate::toast::{Toast, Tone};

/// Application data: commands whose shortcut hint was shown.
pub const HINTS_KEY: &str = "ui.hints_shown";
const USES_BEFORE_HINT: u32 = 3;

/// The columns' sizes: a row is a line of keycaps.
const METRICS: Metrics = Metrics {
    column: 290.,
    gutter: 28.,
    heading: 20.,
    line: 22.,
    row_gap: 2.,
    block_gap: 10.,
};

/// A binding as the overview takes it in.
#[derive(Clone, Debug)]
struct Item {
    keys: String,
    command: String,
    args: serde_json::Value,
    title: String,
    topic: Topic,
    /// Its family, and where it stands in it.
    family: Option<(String, usize)>,
    /// Its place alone, which a filter's words find: not its conditions
    /// ("not in the trash" is no answer to "trash").
    place: String,
    /// How near the focus its place is, here; `None` elsewhere.
    depth: Option<usize>,
}

/// A row: an action and its keys, or the actions of one family and the
/// first key of each.
#[derive(Clone, Debug, PartialEq)]
struct Row {
    title: String,
    topic: Topic,
    /// Each action's title and keys, in the keymap's order; a family's in
    /// its own.
    actions: Vec<(String, Vec<String>)>,
    /// Where each action stands in its family.
    ranks: Vec<usize>,
    family: bool,
    /// What else a filter finds it by: its places and commands.
    words: Vec<String>,
    depth: Option<usize>,
}

impl Row {
    /// Whether what was typed finds it (`keymap::finds`).
    fn found(&self, query: &str) -> bool {
        let mut words: Vec<&str> = vec![&self.title, self.topic.title()];
        words.extend(self.words.iter().map(String::as_str));
        words.extend(self.actions.iter().map(|(title, _)| title.as_str()));
        let keys: Vec<&str> = self
            .actions
            .iter()
            .flat_map(|(_, keys)| keys.iter().map(String::as_str))
            .collect();
        keymap::finds(query, &words, &keys)
    }

    /// Its keys as shown: every binding of an action; a family's first of
    /// each action, with the modifiers they all hold said once.
    fn shown(&self) -> Shown {
        if !self.family {
            return Shown::Chords(
                self.actions
                    .iter()
                    .flat_map(|(_, keys)| keys.iter().cloned())
                    .collect(),
            );
        }
        let firsts: Vec<String> = self
            .actions
            .iter()
            .filter_map(|(_, keys)| keys.first().cloned())
            .collect();
        let caps: Vec<Vec<Vec<String>>> =
            firsts.iter().map(|keys| keymap::key_caps(keys)).collect();
        let held = |strokes: &Vec<Vec<String>>| match strokes.as_slice() {
            [stroke] if !stroke.is_empty() => Some(stroke[..stroke.len() - 1].to_vec()),
            _ => None,
        };
        match held(&caps[0]) {
            Some(first)
                if caps
                    .iter()
                    .all(|strokes| held(strokes).as_ref() == Some(&first)) =>
            {
                Shown::Held {
                    held: first,
                    keys: caps
                        .iter()
                        .filter_map(|strokes| strokes[0].last().cloned())
                        .collect(),
                }
            }
            _ => Shown::Chords(firsts),
        }
    }
}

/// A row's keys as drawn.
#[derive(Debug, PartialEq)]
enum Shown {
    /// Bindings, each whole.
    Chords(Vec<String>),
    /// Modifiers held, then each key they go with: Shift + ↑ ↓ ← →.
    Held {
        held: Vec<String>,
        keys: Vec<String>,
    },
}

/// `items` as rows, in order: an action's keys on one row, a family's on
/// one; a family of one action is that action's row.
fn merge(items: Vec<Item>) -> Vec<Row> {
    let mut keyed: Vec<(String, Row)> = Vec::new();
    for item in items {
        let key = match &item.family {
            Some((family, _)) => format!("{} {family}", item.command),
            None => format!("{} {}", item.command, item.args),
        };
        let at = if let Some(at) = keyed.iter().position(|(known, _)| *known == key) {
            at
        } else {
            keyed.push((
                key,
                Row {
                    title: item
                        .family
                        .as_ref()
                        .map_or_else(|| item.title.clone(), |(family, _)| family.clone()),
                    topic: item.topic,
                    actions: Vec::new(),
                    ranks: Vec::new(),
                    family: item.family.is_some(),
                    words: vec![item.command.clone()],
                    depth: item.depth,
                },
            ));
            keyed.len() - 1
        };
        let row = &mut keyed[at].1;
        row.depth = row.depth.max(item.depth);
        if !row.words.contains(&item.place) {
            row.words.push(item.place);
        }
        match row
            .actions
            .iter_mut()
            .find(|(title, _)| *title == item.title)
        {
            Some((_, keys)) if !keys.contains(&item.keys) => keys.push(item.keys),
            Some(_) => {}
            None => {
                // In its family's order: the views 1 to 5, the left tab
                // before the right.
                let rank = item.family.as_ref().map_or(0, |(_, rank)| *rank);
                let at = row.ranks.iter().take_while(|known| **known <= rank).count();
                row.ranks.insert(at, rank);
                row.actions.insert(at, (item.title, vec![item.keys]));
            }
        }
    }
    keyed
        .into_iter()
        .map(|(_, mut row)| {
            if row.family && row.actions.len() == 1 {
                row.family = false;
                row.title.clone_from(&row.actions[0].0);
            }
            row
        })
        .collect()
}

/// A heading and its rows.
struct Group {
    heading: String,
    rows: Vec<Row>,
}

/// `rows` under their topics: the topics of the place nearest the focus
/// first, in `Topic::ALL`'s order among them; in a topic, that place's
/// rows first.
fn by_topic(mut rows: Vec<Row>) -> Vec<Group> {
    rows.sort_by_key(|row| std::cmp::Reverse(row.depth));
    let mut topics: Vec<(Topic, Option<usize>)> = Vec::new();
    for row in &rows {
        match topics.iter_mut().find(|(topic, _)| *topic == row.topic) {
            Some((_, depth)) => *depth = (*depth).max(row.depth),
            None => topics.push((row.topic, row.depth)),
        }
    }
    let rank = |topic: Topic| Topic::ALL.iter().position(|t| *t == topic);
    topics.sort_by_key(|(topic, depth)| (std::cmp::Reverse(*depth), rank(*topic)));
    topics
        .into_iter()
        .map(|(topic, _)| Group {
            heading: topic.title().to_owned(),
            rows: rows
                .iter()
                .filter(|row| row.topic == topic)
                .cloned()
                .collect(),
        })
        .collect()
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
    /// What works here, by topic. Titled once, when the overview opens.
    here: Vec<Group>,
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
        // How near the focus a binding's place is: the nearest of the
        // places it names (`Pane || Terminal`) on the focus path.
        let depth = |hint: &KeyHint| {
            let place = hint.context.split("&&").next().unwrap_or_default().trim();
            let place = place
                .strip_prefix('(')
                .and_then(|place| place.strip_suffix(')'))
                .unwrap_or(place);
            place
                .split("||")
                .filter_map(|place| {
                    let place = place.trim();
                    stack.iter().rposition(|context| context.contains(place))
                })
                .max()
        };
        let applying: Vec<(usize, &KeyHint)> = self
            .key_hints
            .iter()
            .enumerate()
            .filter(|(_, hint)| applies(&hint.context, &holds))
            .collect();
        // Of the bindings of one key that apply here, the one GPUI runs:
        // the deepest place's, then the latest (a later layer).
        let wins = |i: usize, hint: &KeyHint| {
            !applying.iter().any(|(j, other)| {
                *j != i && other.keys == hint.keys && (depth(other), *j) > (depth(hint), i)
            })
        };
        let item = |hint: &KeyHint, depth: Option<usize>| {
            let (topic, family) = keymap::topic_and_family(&self.app, &hint.command, &hint.args);
            Item {
                keys: hint.keys.clone(),
                command: hint.command.clone(),
                args: hint.args.clone(),
                title: keymap::title_of(&self.app, hint),
                topic,
                family,
                place: keymap::place_of(&hint.context).place,
                depth,
            }
        };
        let mut here: Vec<Item> = Vec::new();
        // Elsewhere, by place and the condition that says why it is not here.
        let mut elsewhere: Vec<(String, Vec<Item>)> = Vec::new();
        for (i, hint) in self.key_hints.iter().enumerate() {
            if applying.iter().any(|(j, _)| *j == i) && wins(i, hint) {
                here.push(item(hint, depth(hint)));
            } else if !self.key_hints[i + 1..]
                .iter()
                .any(|later| later.keys == hint.keys && later.context == hint.context)
            {
                let label = keymap::context_label(&hint.context);
                let found = item(hint, None);
                match elsewhere.iter_mut().find(|(known, _)| *known == label) {
                    Some((_, items)) => items.push(found),
                    None => elsewhere.push((label, vec![found])),
                }
            }
        }
        let here = by_topic(merge(here));
        let elsewhere = elsewhere
            .into_iter()
            .map(|(heading, items)| Group {
                heading,
                rows: merge(items),
            })
            .collect();
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
            here,
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
fn found<'a>(groups: &'a [Group], query: &str) -> Vec<(&'a str, Vec<&'a Row>)> {
    groups
        .iter()
        .filter_map(|group| {
            let rows: Vec<&Row> = group.rows.iter().filter(|row| row.found(query)).collect();
            (!rows.is_empty()).then_some((group.heading.as_str(), rows))
        })
        .collect()
}

pub fn render(shell: &Shell, window: &Window, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let overview = shell.shortcuts.as_ref()?;
    let theme = cx.global::<Theme>().clone();
    let query = overview.filter.read(cx).text().trim().to_owned();
    let here = found(&overview.here, &query);
    // The other places only for a filter: unasked, they would bury these.
    let elsewhere = if query.is_empty() {
        Vec::new()
    } else {
        found(&overview.elsewhere, &query)
    };
    let mut lists = div().flex().flex_col().gap_6().pr_3();
    if here.is_empty() && elsewhere.is_empty() {
        lists = lists.child(
            div()
                .text_color(theme.text_muted)
                .child(format!("No shortcut matches “{query}”.")),
        );
    }
    for (part, groups) in [("here", here), ("elsewhere", elsewhere)] {
        if groups.is_empty() {
            continue;
        }
        let columns = columns(&groups, part, &theme).into_any_element();
        lists = lists.child(if part == "elsewhere" {
            div()
                .flex()
                .flex_col()
                .gap_3()
                .child(div().text_color(theme.text_muted).child("Elsewhere"))
                .child(columns)
                .into_any_element()
        } else {
            columns
        });
    }
    let all = overview.all.clone();
    let filtering = overview.filter.read(cx).is_focused(window);
    // The lists scroll under the title, which keeps its buttons in sight.
    let panel = div()
        .w(px(1320.))
        .max_w(gpui::relative(0.94))
        .max_h(gpui::relative(0.9))
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
        // Its bar shows there is more below, wherever the list is cut.
        .child(
            div()
                .relative()
                .min_h_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .id("shortcuts")
                        .min_h_0()
                        .overflow_y_scroll()
                        .track_scroll(&overview.scroll)
                        .child(lists),
                )
                .child(scrollbar::vertical(
                    "shortcuts-scrollbar",
                    Scrolls::Offset(overview.scroll.clone()),
                )),
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
            .child(div().flex_none().h(gpui::relative(0.05)))
            .child(panel)
            .into_any_element(),
    )
}

/// `groups` flowing down columns, each row an action or a family with
/// its keys. `part` tells the ids of here's rows from elsewhere's.
fn columns(groups: &[(&str, Vec<&Row>)], part: &'static str, theme: &Theme) -> Columns {
    let blocks = groups
        .iter()
        .map(|(heading, rows)| Block {
            heading: (*heading).to_owned(),
            rows: rows
                .iter()
                .map(|row| {
                    let id = crate::elements::known_by("shortcut", (part, heading, &row.title));
                    row_element(row, &id, theme)
                })
                .collect(),
        })
        .collect();
    let muted = theme.text_muted;
    Columns {
        blocks,
        metrics: METRICS,
        heading: Rc::new(move |heading, continued| {
            div()
                .pt(px(2.))
                .text_xs()
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(muted)
                .truncate()
                .child(if continued {
                    format!("{heading}, continued")
                } else {
                    heading.to_owned()
                })
                .into_any_element()
        }),
    }
}

/// A row: its title, then its keys on the same line where they fit the
/// column, else under it.
fn row_element(row: &Row, id: &gpui::ElementId, theme: &Theme) -> columns::Row {
    let shown = row.shown();
    let caps = |stroke: &[String]| {
        let mut caps = div().flex().flex_none().items_center().gap(px(3.));
        for cap in stroke {
            caps = caps.child(controls::keycap(cap.clone(), theme));
        }
        caps
    };
    let keys = || {
        let mut keys = div().flex().flex_none().items_center().gap(px(8.));
        match &shown {
            Shown::Chords(chords) => {
                for (n, chord) in chords.iter().enumerate() {
                    if n > 0 {
                        keys = keys.child(
                            div()
                                .flex_none()
                                .text_xs()
                                .text_color(theme.text_muted)
                                .child("/"),
                        );
                    }
                    // A sequence's strokes, apart.
                    let mut strokes = div().flex().flex_none().items_center().gap(px(8.));
                    for stroke in keymap::key_caps(chord) {
                        strokes = strokes.child(caps(&stroke));
                    }
                    keys = keys.child(strokes);
                }
            }
            Shown::Held { held, keys: each } => {
                let mut line = div().flex().flex_none().items_center().gap(px(3.));
                if !held.is_empty() {
                    line = line.child(caps(held)).child(
                        div()
                            .px(px(2.))
                            .text_xs()
                            .text_color(theme.text_muted)
                            .child("+"),
                    );
                }
                for key in each {
                    line = line.child(controls::keycap(key.clone(), theme));
                }
                keys = keys.child(line);
            }
        }
        keys
    };
    let title = || {
        crate::elements::clip::clipped(
            div().id(id.clone()).flex_1().min_w_0().truncate(),
            row.title.clone(),
        )
    };
    let line = || div().h(px(METRICS.line)).flex().items_center();
    columns::Row {
        inline: line()
            .w_full()
            .justify_between()
            .gap_3()
            .child(title())
            .child(keys())
            .into_any_element(),
        stacked: div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(METRICS.row_gap))
            .child(line().child(title()))
            .child(line().min_w_0().overflow_hidden().child(keys()))
            .into_any_element(),
    }
}

#[cfg(test)]
mod tests {
    use skwir_core::command::Topic;

    use super::{Item, Shown, applies, by_topic, merge};

    fn item(keys: &str, command: &str, title: &str, topic: Topic, depth: usize) -> Item {
        Item {
            keys: keys.into(),
            command: command.into(),
            args: serde_json::json!({}),
            title: title.into(),
            topic,
            family: None,
            place: if depth > 0 { "File list" } else { "Anywhere" }.into(),
            depth: Some(depth),
        }
    }

    fn member(keys: &str, to: &str, title: &str, family: &str) -> Item {
        let rank = ["up", "down", "left", "right", "home", "end"]
            .iter()
            .position(|t| *t == to)
            .unwrap();
        Item {
            args: serde_json::json!({ "to": to, "step": family }),
            family: Some((family.into(), rank)),
            ..item(keys, "ui.cursor", title, Topic::Select, 2)
        }
    }

    /// One row per action, its keys side by side; a family's on one row,
    /// the modifiers they all hold said once.
    #[test]
    fn an_action_or_a_family_is_one_row() {
        let rows = merge(vec![
            item("ctrl-shift-z", "redo", "Redo", Topic::Files, 0),
            member(
                "shift-up",
                "up",
                "Extend the selection up",
                "Extend the selection",
            ),
            item("ctrl-y", "redo", "Redo", Topic::Files, 0),
            member(
                "shift-down",
                "down",
                "Extend the selection down",
                "Extend the selection",
            ),
            member(
                "shift-home",
                "home",
                "Extend the selection to the first item",
                "Extend the selection",
            ),
            Item {
                family: Some(("Zoom".into(), 0)),
                ..item("ctrl-=", "ui.zoom", "Zoom in", Topic::View, 0)
            },
        ]);
        let titles: Vec<&str> = rows.iter().map(|row| row.title.as_str()).collect();
        assert_eq!(
            titles,
            ["Redo", "Extend the selection", "Zoom in"],
            "a family of one is its action"
        );
        assert_eq!(
            rows[0].shown(),
            Shown::Chords(vec!["ctrl-shift-z".into(), "ctrl-y".into()])
        );
        assert_eq!(
            rows[1].shown(),
            Shown::Held {
                held: vec!["Shift".into()],
                keys: vec!["↑".into(), "↓".into(), "Home".into()],
            }
        );
        assert!(!rows[2].family);
        // Keys that hold different modifiers are said whole.
        let tabs = merge(vec![
            Item {
                family: Some(("Go".into(), 1)),
                ..item("ctrl-tab", "tab.select", "Next tab", Topic::Tabs, 0)
            },
            Item {
                args: serde_json::json!({ "delta": -1 }),
                family: Some(("Go".into(), 0)),
                ..item(
                    "ctrl-shift-tab",
                    "tab.select",
                    "Previous tab",
                    Topic::Tabs,
                    0,
                )
            },
        ]);
        assert!(matches!(tabs[0].shown(), Shown::Chords(chords) if chords.len() == 2));
    }

    /// The topics of the place nearest the focus come first, its rows
    /// first in each; a filter finds a row by its topic, words or keys.
    #[test]
    fn rows_are_under_their_topics_nearest_first() {
        let groups = by_topic(merge(vec![
            item("ctrl-t", "tab.new", "New tab", Topic::Tabs, 0),
            item("alt-left", "back", "Back", Topic::Go, 0),
            item("ctrl-a", "select_all", "Select all", Topic::Select, 2),
            item("enter", "open", "Open", Topic::Go, 2),
            item("ctrl-q", "ui.quit", "Quit", Topic::Skwir, 0),
        ]));
        let headings: Vec<&str> = groups.iter().map(|g| g.heading.as_str()).collect();
        assert_eq!(
            headings,
            ["Going places", "Selecting", "Tabs and panes", "Skwir"]
        );
        let going: Vec<&str> = groups[0].rows.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(going, ["Open", "Back"]);
        let row = &groups[2].rows[0];
        assert!(row.found("tabs"));
        assert!(row.found("ctrl t"));
        assert!(row.found("new"));
        assert!(!row.found("quit"));
    }

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
