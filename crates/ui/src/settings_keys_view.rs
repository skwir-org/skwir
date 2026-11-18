//! The Keyboard page (ADR-024), drawn: the keymap as `settings_keys` reads
//! it, in one list that draws the rows in view, with its scrollbar and the
//! caption of the place at its top kept there. A key's chip, or a row's +,
//! opens the bar under the list, which takes the next keys pressed before
//! any binding sees them, and says what they would do before they are
//! saved. The list walks with the keyboard (`ui.settings_key`).

use crate::sides::Sides;
use crate::sizes::TextSizes;
use crate::sizes::sized;
use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    AnyElement, App, Bounds, Context, Entity, FocusHandle, Focusable, Keystroke, Pixels, Point,
    ScrollStrategy, SharedString, Subscription, UniformListDecoration, UniformListScrollHandle,
    Window, div, prelude::*, px,
};
use skwir_core::keymap::BINDINGS_KEY;
use skwir_core::words;
use skwir_core::words::Words;

use crate::controls::{self, ButtonStyle};
use crate::input::{InputEvent, TextInput};
use crate::keymap::{self, Bound};
use crate::settings::SettingsView;
use crate::settings_keys::{
    Action, ActionId, Chip, Does, Edit, Effects, Keyboard, Note, Row, Show, Stay, Verdict, apply,
    effects, removal_refused, rows, verdict,
};
use crate::settings_records::{Record, records};
use crate::settings_view::{Edge, card_row, revert_slot};
use crate::theme::Chosen;
use crate::theme::Theme;

/// The height of every row of the list, a caption's too: the same for
/// all, so only the rows in view are drawn.
const ROW: f32 = 40.;
/// Strokes a sequence may have: `g g`, `ctrl-k ctrl-s`, three at most.
const STROKES: usize = 3;

/// What the Keyboard page shows, and what it is editing.
pub(crate) struct KeysPage {
    /// The keymap's bindings, as the shell installed them last.
    bound: Arc<Vec<Bound>>,
    keyboard: Rc<Keyboard>,
    /// The rows for what was asked last, worked out again when that or the
    /// keymap changes, not at each frame.
    rows: RefCell<Option<(Asked, Rc<Vec<Row>>)>>,
    generation: u64,
    /// The words' revision the keymap was read in (`words::revision`): its
    /// titles and places are said, read again when the language moves.
    said_in: u64,
    pub list: UniformListScrollHandle,
    /// The list's: it walks the actions (`ui.settings_key`).
    pub focus: FocusHandle,
    pub show: Show,
    /// The keys inside dialogs and menus are unfolded.
    pub inside: bool,
    /// Keys whose actions are listed, as `keymap::normal_keys` writes them.
    pub find: Option<String>,
    /// What narrows the list: words of the actions' or keys
    /// (`keymap::finds`).
    pub filter: Entity<TextInput>,
    /// The action the keyboard is on, and which of its keys: its chips,
    /// then its +.
    pub cursor: Option<(ActionId, usize)>,
    pub editor: Option<KeyEditor>,
    pub new: Option<crate::settings_keys_new::NewShortcut>,
    /// The records shown as they are stored.
    pub as_records: bool,
}

#[derive(Clone, PartialEq)]
struct Asked {
    show: Show,
    query: String,
    filter: String,
    find: Option<String>,
    inside: bool,
    generation: u64,
}

impl KeysPage {
    /// The keys bound to `command` without arguments, as people read
    /// them: the binding of the highest layer that no later one hides.
    /// What a tooltip names beside a control (a field's `?`, ADR-137).
    pub(crate) fn keys_for(&self, command: &str) -> Option<String> {
        self.bound
            .iter()
            .filter(|bound| {
                bound.hidden_by.is_none()
                    && bound.command.as_ref().is_some_and(|(name, args)| {
                        name == command && args.as_object().is_none_or(serde_json::Map::is_empty)
                    })
            })
            .max_by_key(|bound| bound.layer)
            .map(|bound| crate::keymap::display_keys(&bound.keys))
    }

    pub fn new(bound: Arc<Vec<Bound>>, cx: &mut App) -> Self {
        KeysPage {
            bound,
            keyboard: Rc::default(),
            rows: RefCell::default(),
            generation: 0,
            said_in: words::revision(),
            list: UniformListScrollHandle::new(),
            focus: cx.focus_handle(),
            show: Show::default(),
            inside: false,
            find: None,
            filter: cx.new(|cx| {
                TextInput::new(
                    words!(
                        keys_filter = "Filter by action or keys",
                        note = "The placeholder of the Keyboard page's filter: what it finds an action by."
                    ),
                    cx,
                )
            }),
            cursor: None,
            editor: None,
            new: None,
            as_records: false,
        }
    }
}

/// What the bar under the list edits.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Target {
    /// New keys for an action.
    Add(ActionId),
    /// Other keys in place of one of an action's.
    Change(ActionId, Chip),
    /// A default the user's record hides.
    Restore(ActionId, Chip),
    /// Back to Skwir's keys, when that takes keys from other actions.
    Reset(ActionId),
    /// Keys whose actions to list.
    Find,
    /// The keys of a new shortcut (`settings_keys_new`).
    New,
}

impl Target {
    fn action(&self) -> Option<&ActionId> {
        match self {
            Target::Add(id)
            | Target::Change(id, _)
            | Target::Restore(id, _)
            | Target::Reset(id) => Some(id),
            Target::Find | Target::New => None,
        }
    }

    /// Whether keys are taken for it.
    fn takes_keys(&self) -> bool {
        !matches!(self, Target::Restore(..) | Target::Reset(_))
    }
}

/// What the bar says of its change before it is saved: what the keys
/// pressed would do there (`verdict`), and what the change leaves of every
/// action's keys (`effects`). Worked out when the keys or the change
/// change, not at each frame.
pub(crate) struct Judged {
    pub verdict: Verdict,
    pub effects: Effects,
    /// The keys pressed, empty for none.
    pub keys: String,
}

impl Judged {
    /// Why the change cannot be made, if it cannot.
    fn refused(&self) -> Option<&Words> {
        self.verdict
            .refused
            .as_ref()
            .or(self.effects.refused.as_ref())
    }
}

/// The keys being taken, for an action, a search or a new shortcut.
pub(crate) struct KeyEditor {
    pub target: Target,
    /// The strokes taken, as GPUI writes them (`ctrl-k`).
    pub strokes: Vec<String>,
    /// Whether the next stroke is taken.
    pub recording: bool,
    /// Keys typed instead, for those the desktop keeps.
    pub typed: Option<Entity<TextInput>>,
    pub error: Option<String>,
    /// Why the last change asked was refused, where what the bar judges
    /// now does not say it (`Judged::refused` comes first): said once.
    pub refused: Option<Words>,
    /// Where the keyboard is while strokes are taken.
    pub focus: FocusHandle,
    intercept: Option<Subscription>,
    /// What its typed keys' field says, while it is open.
    subscriptions: Vec<Subscription>,
    /// What the bar says of the change, for what it was worked out for.
    judged: RefCell<Option<(String, Rc<Judged>)>>,
}

impl KeyEditor {
    /// The keys taken or typed, one space between strokes.
    pub fn keys(&self, cx: &App) -> String {
        match &self.typed {
            Some(input) => input
                .read(cx)
                .text()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" "),
            None => self.strokes.join(" "),
        }
    }
}

/// A key held alone, which GPUI may report: no stroke of its own.
fn modifier_alone(key: &str) -> bool {
    matches!(
        key,
        "shift" | "control" | "alt" | "platform" | "function" | "capslock"
    )
}

/// Keys as key caps, a gap between strokes.
pub(crate) fn caps(keys: &str, struck: bool, theme: &Theme) -> gpui::Div {
    // Left to right in every language, as shortcuts are written
    // (`sides::shortcut`).
    let mut all = div()
        .flex()
        .flex_none()
        .items_center()
        .gap(sized(8.))
        .left_to_right();
    for stroke in keymap::key_caps(keys) {
        let mut one = div().flex().items_center().gap(sized(3.));
        for cap in stroke {
            one = one.child(
                controls::keycap(cap, theme)
                    .when(struck, |d| d.line_through().text_color(theme.text_muted)),
            );
        }
        all = all.child(one);
    }
    all
}

/// Where a row stands in its card, from the rows around it.
fn edge(rows: &[Row], n: usize) -> Edge {
    let action = |i: Option<usize>| {
        i.and_then(|i| rows.get(i))
            .is_some_and(|row| matches!(row, Row::Action(_)))
    };
    Edge {
        first: !action(n.checked_sub(1)),
        last: !action(Some(n + 1)),
    }
}

impl SettingsView {
    /// The keymap changed while the dialog is open: a shortcut saved here,
    /// a plugin turned on or off.
    pub fn set_bound(&mut self, bound: Arc<Vec<Bound>>, cx: &mut Context<Self>) {
        self.keys.bound = bound;
        self.read_keys();
        cx.notify();
    }

    /// Read the keymap again, as the page lists it.
    pub(crate) fn read_keys(&mut self) {
        let user = skwir_core::keymap::user_bindings(&self.app.settings);
        self.keys.keyboard = Rc::new(Keyboard::read(&self.app, &self.keys.bound, &user));
        self.keys.said_in = words::revision();
        self.keys.generation += 1;
        self.keys.rows.take();
    }

    /// The keymap read again where what it says was said in another
    /// language than the one said now.
    fn keys_said_now(&mut self) {
        if self.keys.said_in != words::revision() {
            self.read_keys();
        }
    }

    /// The rows for `query`, the dialog's search, or else what the page
    /// asks for: kept until that or the keymap changes.
    fn key_rows(&self, query: &str, cx: &App) -> Rc<Vec<Row>> {
        let filter = if query.is_empty() {
            self.keys.filter.read(cx).text().trim().to_owned()
        } else {
            String::new()
        };
        let asked = Asked {
            show: self.keys.show,
            query: query.to_owned(),
            filter,
            find: self.keys.find.clone(),
            inside: self.keys.inside,
            generation: self.keys.generation,
        };
        if let Some((kept, rows)) = self.keys.rows.borrow().as_ref()
            && *kept == asked
        {
            return Rc::clone(rows);
        }
        let rows = Rc::new(rows(
            &self.keys.keyboard,
            asked.show,
            query,
            &asked.filter,
            asked.find.as_deref(),
            asked.inside,
        ));
        *self.keys.rows.borrow_mut() = Some((asked, Rc::clone(&rows)));
        rows
    }

    /// The records as the dialog shows them now: its own value, saved or
    /// on its way.
    fn key_records(&self) -> Vec<Record> {
        self.row(BINDINGS_KEY)
            .map(|row| records(&row.value))
            .unwrap_or_default()
    }

    /// Make `edit` and save the records it leaves; `false`, with the
    /// reason kept, when the setting refuses them.
    pub(crate) fn save_keys(&mut self, edit: &Edit, cx: &mut Context<Self>) -> bool {
        let mut list = self.key_records();
        apply(&mut list, edit);
        let text = serde_json::to_string(&list).unwrap_or_default();
        self.change(BINDINGS_KEY, text, cx);
        !self.errors.contains_key(BINDINGS_KEY)
    }

    // ------------------------------------------------------------ the page

    /// The page under its title: what to show and the ways in, then the
    /// list, or the new shortcut's form, or the records as stored.
    pub(crate) fn keys_toolbar(
        &mut self,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.keys_said_now();
        let changed = self
            .keys
            .keyboard
            .actions
            .iter()
            .filter(|action| action.changed())
            .count();
        let show = self.handle("keys-show", cx);
        self.stop(&show, None, None);
        let current = self.keys.show;
        let mut segmented = controls::segmented("keys-show", &show, theme).on_click(cx.listener(
            move |this, event: &gpui::ClickEvent, _, cx| {
                if matches!(event, gpui::ClickEvent::Keyboard(_)) {
                    this.keys.show = match current {
                        Show::Keyed => Show::All,
                        Show::All => Show::Changed,
                        Show::Changed => Show::Keyed,
                    };
                    cx.notify();
                }
            },
        ));
        for (choice, label) in [
            (
                Show::Keyed,
                words!(
                    keys_show_keyed = "With keys",
                    note = "A choice of what the Keyboard page lists: the actions that have keys."
                ),
            ),
            (
                Show::All,
                words!(
                    keys_show_all = "All actions",
                    note = "A choice of what the Keyboard page lists: every action, those without keys too."
                ),
            ),
            (
                Show::Changed,
                if changed == 0 {
                    words!(
                        keys_show_changed = "Changed by you",
                        note = "A choice of what the Keyboard page lists: the actions whose keys the user changed."
                    )
                } else {
                    words!(
                        keys_show_changed_count = "Changed by you ({ $count })",
                        count = changed,
                        note = "A choice of what the Keyboard page lists: the actions whose keys the user changed, and how many."
                    )
                },
            ),
        ] {
            let focus = show.clone();
            segmented = segmented.child(
                controls::segment(
                    SharedString::from(format!("keys-show-{choice:?}")),
                    label.say(),
                    current == choice,
                    theme,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    window.focus(&focus, cx);
                    this.keys.show = choice;
                    this.keys.as_records = false;
                    this.keys.new = None;
                    cx.notify();
                })),
            );
        }
        let new = self.handle("keys-new", cx);
        self.stop(&new, None, None);
        // The list's own filter, where the list is shown.
        let listed = self.keys.new.is_none() && !self.keys.as_records;
        let filter = listed.then(|| {
            let focus = self.keys.filter.focus_handle(cx);
            self.stop(&focus, None, None);
            let input = &self.keys.filter;
            let focused = input.read(cx).is_focused(window);
            controls::field(input, None, focused, false, Some("search"), theme)
        });
        let find = self.handle("keys-find", cx);
        self.stop(&find, None, None);
        let finding = match &self.keys.find {
            Some(keys) => div()
                .flex()
                .items_center()
                .gap_1()
                .child(caps(keys, false, theme))
                .child(
                    controls::icon_button(
                        "keys-find-clear",
                        "close",
                        words!(
                            keys_find_clear = "Clear",
                            note = "Clears the keys the Keyboard page's list was narrowed to."
                        )
                        .say(),
                        &find,
                        theme,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.keys.find = None;
                        cx.notify();
                    })),
                )
                .into_any_element(),
            None => controls::button(
                "keys-find",
                words!(
                    keys_find = "Find by keys",
                    note =
                        "A button of the Keyboard page: press keys to list the actions they run."
                )
                .say(),
                Some("keyboard"),
                ButtonStyle::Plain,
                &find,
                theme,
            )
            .on_click(cx.listener(|this, _, window, cx| {
                this.open_key_editor(Target::Find, window, cx);
            }))
            .into_any_element(),
        };
        let records = self.handle("keys-records", cx);
        self.stop(&records, None, None);
        let as_records = self.keys.as_records;
        let waiting = self.keys.keyboard.waiting.len();
        let note = (waiting > 0).then(|| {
            controls::noted(
                "plugin",
                theme.text_muted,
                13.,
                words!(
                    keys_waiting = "{ $count ->
        [1] One of your shortcuts waits for a plugin that is off: it comes back with it.
       *[other] { $count } of your shortcuts wait for plugins that are off: they come back with them.
    }",
                    count = waiting
                )
                .say(),
            )
            .flex_1()
            .min_w_0()
        });
        let (accent, text) = (theme.accent, theme.text);
        let mut bar = div()
            .flex_none()
            .px_6()
            .pb_3()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(segmented)
                    .child(div().flex_1())
                    .child(
                        controls::button(
                            "keys-new",
                            words!(
                                keys_new_shortcut = "New shortcut…",
                                note = "A button of the Keyboard page: keys for an action no row lists (a folder to go to)."
                            )
                            .say(),
                            Some("plus"),
                            ButtonStyle::Plain,
                            &new,
                            theme,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.open_new_shortcut(window, cx);
                        })),
                    ),
            )
            // The ways to find an action: what it does or its keys, typed,
            // or its keys pressed.
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(filter.unwrap_or_else(|| div().flex_1()))
                    .child(finding),
            )
            // What waits, and the records as they are stored: small, as
            // what most never need.
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_small()
                    .text_color(theme.text_muted)
                    .children(note)
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("keys-records")
                            .track_focus(&records)
                            .px_1()
                            .rounded_sm()
                            .border_1()
                            .border_color(gpui::transparent_black())
                            .focus(move |s| s.border_color(accent))
                            .cursor_pointer()
                            .hover(move |s| s.text_color(text))
                            .child(
                                if as_records {
                                    words!(
                                        keys_back_to_list = "Back to the list",
                                        note = "Leaves the shortcuts as stored for the Keyboard page's list."
                                    )
                                } else {
                                    words!(
                                        keys_edit_as_records = "Edit as records",
                                        note = "Shows the user's shortcuts as they are stored, each a record."
                                    )
                                }
                                .say(),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.keys.as_records = !this.keys.as_records;
                                this.keys.new = None;
                                cx.notify();
                            })),
                    ),
            );
        if self.keys.editor.is_none()
            && let Some(error) = self.errors.get(BINDINGS_KEY)
        {
            bar = bar.child(
                controls::noted("warning", theme.error, 13., error.say())
                    .text_body()
                    .text_color(theme.error),
            );
        }
        bar.into_any_element()
    }

    /// Every row asked for, in a list that draws those in view: hundreds
    /// of bindings were built at every frame the dialog drew.
    pub(crate) fn keys_list(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let rows = self.key_rows("", cx);
        let focus = self.keys.focus.clone();
        self.stop(&focus, None, None);
        if rows.is_empty() {
            let filter = self.keys.filter.read(cx).text().trim();
            return div()
                .flex_1()
                .px_6()
                .py_6()
                .track_focus(&focus)
                .text_color(cx.global::<Theme>().text_muted)
                .child(
                    match (self.keys.show, filter.is_empty()) {
                        (Show::Keyed, false) => words!(
                            keys_no_shortcut_matches = "No shortcut matches “{ $filter }”.",
                            filter = filter.to_owned(),
                            note = "filter: what the user typed in the Keyboard page's filter."
                        ),
                        (Show::All, false) => words!(
                            keys_no_action_matches = "No action matches “{ $filter }”.",
                            filter = filter.to_owned(),
                            note = "filter: what the user typed in the Keyboard page's filter."
                        ),
                        (Show::Changed, false) => words!(
                            keys_no_changed_matches = "No key you changed matches “{ $filter }”.",
                            filter = filter.to_owned(),
                            note = "filter: what the user typed in the Keyboard page's filter."
                        ),
                        (Show::Changed, true) => {
                            words!(keys_none_changed = "You changed no keys yet.")
                        }
                        _ => words!(
                            keys_none_has_keys = "No action has these keys.",
                            note =
                                "The Keyboard page's list, narrowed to the keys pressed, is empty."
                        ),
                    }
                    .say(),
                )
                .into_any_element();
        }
        let focused = focus.contains_focused(window, cx);
        let count = rows.len();
        let sticky = Sticky {
            rows: Rc::clone(&rows),
            theme: cx.global::<Theme>().clone(),
        };
        let list = gpui::uniform_list(
            "keyboard-actions",
            count,
            cx.processor(move |this: &mut Self, range: Range<usize>, _, cx| {
                let theme = cx.global::<Theme>().clone();
                range
                    .filter_map(|n| {
                        let element = match rows.get(n)? {
                            Row::Place(place) => div()
                                .h(sized(ROW))
                                .flex()
                                .items_end()
                                .pb_2()
                                .text_small()
                                .text_color(theme.text_muted)
                                .child(place.clone())
                                .into_any_element(),
                            Row::Inside { open, count } => this
                                .inside_row(*open, *count, &theme, cx)
                                .into_any_element(),
                            Row::Action(index) => this
                                .action_row(*index, edge(&rows, n), focused, &theme, cx)
                                .h(sized(ROW))
                                .into_any_element(),
                        };
                        Some(div().w_full().px_6().child(element).into_any_element())
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.keys.list)
        .with_decoration(sticky)
        .size_full();
        div()
            .id("keyboard-list")
            .track_focus(&focus)
            .key_context("SettingKeys")
            .relative()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .on_click(cx.listener(|this, _, window, cx| {
                window.focus(&this.keys.focus, cx);
                cx.notify();
            }))
            .child(list)
            .child(crate::elements::scrollbar::vertical(
                "keyboard-scrollbar",
                crate::elements::scrollbar::Scrolls::Offset(
                    self.keys.list.0.borrow().base_handle.clone(),
                ),
            ))
            .into_any_element()
    }

    /// The fold of the keys inside dialogs and menus.
    fn inside_row(
        &self,
        open: bool,
        count: usize,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let hover = theme.hover;
        // As high as every row: the list draws them all in one height.
        div()
            .id("keys-inside")
            .h(sized(ROW))
            // One line, all it holds (a row of fixed height).
            .whitespace_nowrap()
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .rounded_md()
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .text_body()
            .text_color(theme.text_muted)
            .child(controls::glyph(
                if open { "chevron" } else { "chevron-right" },
                theme.text_muted,
                14.,
            ))
            .child(crate::elements::clip::clipped(
                div().id("keys-inside-title").min_w_0().truncate(),
                words!(
                    keys_inside = "Keys inside dialogs and menus ({ $count })",
                    count = count,
                    note = "A fold of the Keyboard page's list: the actions whose keys work inside dialogs and menus, and how many."
                )
                .say(),
            ))
            .on_click(cx.listener(|this, _, _, cx| {
                this.keys.inside = !this.keys.inside;
                cx.notify();
            }))
    }

    /// An action: its title, the conditions its keys share, a way back to
    /// skwir's keys when changed, its keys, and a + for another.
    pub(crate) fn action_row(
        &self,
        index: usize,
        edge: Edge,
        focused: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let Some(action) = self.keys.keyboard.actions.get(index) else {
            return div().id(SharedString::from(format!("key-action-{index}")));
        };
        let id = action.id();
        let cursor = self
            .keys
            .cursor
            .as_ref()
            .filter(|(at, _)| *at == id)
            .map(|(_, chip)| *chip);
        let editing = self
            .keys
            .editor
            .as_ref()
            .filter(|editor| editor.target.action() == Some(&id))
            .map(|editor| editor.target.clone());
        let shared = action.shared_conditions().map(str::to_owned);
        // The keys give way before the title: a key's conditions are cut
        // short first (whole in its tooltip).
        let mut keys = div()
            .flex()
            .flex_shrink_1()
            .min_w_0()
            .justify_end()
            .items_center()
            .gap_1();
        for (n, chip) in action.chips.iter().enumerate() {
            let pointed = focused && cursor == Some(n);
            let edited = matches!(
                &editing,
                Some(Target::Change(_, c) | Target::Restore(_, c)) if c == chip
            );
            keys = keys.child(self.chip(
                index,
                n,
                action,
                chip,
                shared.is_none(),
                pointed,
                edited,
                theme,
                cx,
            ));
        }
        let plus_pointed = focused && cursor == Some(action.chips.len());
        let adding = matches!(editing, Some(Target::Add(_)));
        let (accent, hover) = (theme.accent, theme.hover);
        let add = div()
            .id(SharedString::from(format!("key-add-{index}")))
            .flex_none()
            .size(px(24.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_md()
            .border_1()
            .border_color(if plus_pointed {
                accent
            } else {
                gpui::transparent_black()
            })
            .when(adding, |d| d.chosen(theme))
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .child(controls::glyph("plus", theme.text_muted, 14.))
            .tooltip(move |_, cx| {
                crate::toolbar::tooltip(
                    words!(
                        keys_add_tip = "Add keys",
                        note = "The tooltip of a row's + on the Keyboard page: more keys for its action."
                    )
                    .say()
                    .into(),
                    cx,
                )
            })
            .on_click(cx.listener({
                let id = id.clone();
                move |this, _, window, cx| {
                    // Its row would take the cursor and the keyboard back.
                    cx.stop_propagation();
                    let at = this.keys.keyboard.find(&id).map_or(0, |a| a.chips.len());
                    this.keys.cursor = Some((id.clone(), at));
                    this.open_key_editor(Target::Add(id.clone()), window, cx);
                }
            }));
        let reset = action.changed().then(|| {
            let id = id.clone();
            div()
                .id(SharedString::from(format!("key-reset-{index}")))
                .flex_none()
                .size(px(24.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_md()
                .cursor_pointer()
                .hover(move |s| s.bg(hover))
                .child(controls::glyph("revert", theme.text_muted, 14.))
                .tooltip(move |_, cx| {
                    crate::toolbar::tooltip(
                        words!(
                            keys_reset_tip = "Back to Skwir's keys",
                            note = "The tooltip of a row's revert button on the Keyboard page: its action's keys as skwir ships them."
                        )
                        .say()
                        .into(),
                        cx,
                    )
                })
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.reset_keys(&id, window, cx);
                }))
                .into_any_element()
        });
        let mut texts = div()
            .flex_1()
            .min_w(sized(160.))
            .flex()
            .items_baseline()
            .gap_2()
            .child(
                div()
                    .flex_shrink_1()
                    .min_w_0()
                    .truncate()
                    .child(action.title.clone()),
            );
        if let Some(shared) = shared {
            texts = texts.child(
                div()
                    .flex_shrink_1()
                    .min_w_0()
                    .truncate()
                    .text_small()
                    .text_color(theme.text_muted)
                    .child(shared),
            );
        }
        let id_click = id.clone();
        card_row(theme, edge)
            .id(SharedString::from(format!("key-action-{index}")))
            .py_1()
            .gap_2()
            .when(editing.is_some(), |d| d.bg(theme.highlight))
            .when(focused && cursor.is_some(), |d| {
                d.border_color(theme.accent)
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                let at = this
                    .keys
                    .cursor
                    .as_ref()
                    .filter(|(at, _)| *at == id_click)
                    .map_or(0, |(_, at)| *at);
                this.keys.cursor = Some((id_click.clone(), at));
                window.focus(&this.keys.focus, cx);
                cx.notify();
            }))
            .child(texts)
            .child(revert_slot(24., reset))
            .child(keys)
            .child(add)
    }

    /// One key of an action: its caps, the conditions it adds, the user's
    /// in the accent, a default the user's hides struck through.
    #[allow(clippy::too_many_arguments)] // A chip, where it is, and how it is drawn.
    fn chip(
        &self,
        index: usize,
        n: usize,
        action: &Action,
        chip: &Chip,
        conditions: bool,
        pointed: bool,
        edited: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let (accent, hover) = (theme.accent, theme.hover);
        let said = match (&chip.hidden_by, &chip.record) {
            (Some(_), _) => words!(
                keys_chip_hidden =
                    "No longer runs it: your shortcut took these keys. Click to restore.",
                note = "The tooltip of a key of skwir's on the Keyboard page that a shortcut of the user's took."
            ),
            (None, Some(_)) => words!(
                keys_chip_yours = "Yours. Click to change or remove.",
                note = "The tooltip of a key the user gave an action on the Keyboard page."
            ),
            (None, None) => words!(
                keys_chip_default = "Click to change or remove.",
                note = "The tooltip of a key of skwir's on the Keyboard page."
            ),
        };
        // Its conditions whole, which its row may cut short.
        let tip: SharedString = if chip.conditions.is_empty() {
            said.say().into()
        } else {
            // Transitional: the keymap's conditions capitalised here,
            // until the keymap says them as a sentence's start (ADR-139).
            let mut chars = chip.conditions.chars();
            let first: String = chars
                .next()
                .into_iter()
                .flat_map(char::to_uppercase)
                .collect();
            words!(
                keys_chip_conditions = "{ $conditions }. { $tip }",
                conditions = format!("{first}{}", chars.as_str()),
                tip = said,
                note = "A key's tooltip on the Keyboard page: when its keys work (In a folder, with a selection), then what a click does."
            )
            .say()
            .into()
        };
        let id = action.id();
        let target = chip.clone();
        div()
            .id(SharedString::from(format!("key-{index}-{n}")))
            .flex()
            .flex_shrink_1()
            .min_w_0()
            .items_center()
            .gap_1()
            .px_1()
            .py(sized(2.))
            .rounded_md()
            .border_1()
            .border_color(if pointed {
                accent
            } else {
                gpui::transparent_black()
            })
            .when(edited, |d| d.chosen(theme))
            .cursor_pointer()
            .when(!edited, |d| d.hover(move |s| s.bg(hover)))
            .child(caps(&chip.keys, !chip.runs(), theme))
            .when(chip.record.is_some(), |d| {
                d.child(controls::mark(accent).flex_none().size(px(6.)))
            })
            .when(conditions && !chip.conditions.is_empty(), |d| {
                d.child(
                    div()
                        .flex_shrink_1()
                        .min_w(px(24.))
                        .max_w(sized(150.))
                        .truncate()
                        .text_small()
                        .text_color(theme.text_muted)
                        .child(chip.conditions.clone()),
                )
            })
            .tooltip(move |_, cx| crate::toolbar::tooltip(tip.clone(), cx))
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.keys.cursor = Some((id.clone(), n));
                let edit = if target.runs() {
                    Target::Change(id.clone(), target.clone())
                } else {
                    Target::Restore(id.clone(), target.clone())
                };
                this.open_key_editor(edit, window, cx);
            }))
    }

    /// The actions a search finds, as rows of the page's own (not drawn
    /// in a list: a search's results are few), in one block that has the
    /// list's keyboard, walked as the list is; how many.
    pub(crate) fn keys_found(
        &mut self,
        query: &str,
        items: &mut Vec<AnyElement>,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> usize {
        self.keys_said_now();
        let rows = self.key_rows(query, cx);
        let focus = self.keys.focus.clone();
        let focused = focus.contains_focused(window, cx);
        let mut found = 0;
        let mut block: Vec<AnyElement> = Vec::new();
        for (n, row) in rows.iter().enumerate() {
            match row {
                Row::Place(place) => block.push(
                    div()
                        .flex_none()
                        .pt_5()
                        .pb_2()
                        .text_small()
                        .text_color(theme.text_muted)
                        .child(
                            words!(
                                keys_found_place = "Keyboard: { $place }",
                                place = place.clone(),
                                note = "A caption over the actions the settings' search found on the Keyboard page: place is where their keys work (File list)."
                            )
                            .say(),
                        )
                        .into_any_element(),
                ),
                Row::Inside { .. } => {}
                Row::Action(index) => {
                    found += 1;
                    block.push(
                        self.action_row(*index, edge(&rows, n), focused, theme, cx)
                            .flex_none()
                            .into_any_element(),
                    );
                }
            }
        }
        if found > 0 {
            self.stop(&focus, Some(items.len()), None);
            items.push(
                div()
                    .id("keyboard-found")
                    .track_focus(&focus)
                    .key_context("SettingKeys")
                    .flex_none()
                    .flex()
                    .flex_col()
                    .children(block)
                    .into_any_element(),
            );
        }
        found
    }

    // ------------------------------------------------------------ the bar

    /// Open the bar for `target` and take the next keys pressed.
    pub(crate) fn open_key_editor(
        &mut self,
        target: Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focus = cx.focus_handle();
        // With no keys to take, the bar's answer has the keyboard: the
        // recorder is not drawn, and focus there would leave the dialog.
        let first = if target.takes_keys() {
            focus.clone()
        } else {
            self.handle("keys-save", cx)
        };
        self.keys.editor = Some(KeyEditor {
            recording: target.takes_keys(),
            target,
            strokes: Vec::new(),
            typed: None,
            error: None,
            refused: None,
            focus,
            intercept: None,
            subscriptions: Vec::new(),
            judged: RefCell::default(),
        });
        window.focus(&first, cx);
        self.listen_for_keys(cx);
        cx.notify();
    }

    /// Take the next stroke before any binding sees it: Ctrl+Q is recorded,
    /// not run. Keys the desktop keeps never arrive; they can be typed.
    fn listen_for_keys(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.keys.editor.as_mut().filter(|e| e.recording) else {
            return;
        };
        let view = cx.entity().downgrade();
        editor.intercept = Some(cx.intercept_keystrokes(move |event, window, cx| {
            let keystroke = event.keystroke.clone();
            if modifier_alone(&keystroke.key) || keystroke.is_ime_in_progress() {
                return;
            }
            cx.stop_propagation();
            view.update(cx, |this, cx| this.take_stroke(&keystroke, window, cx))
                .ok();
        }));
    }

    fn take_stroke(&mut self, keystroke: &Keystroke, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.keys.editor.as_mut() else {
            return;
        };
        editor.strokes.push(crate::keymap::unparse(keystroke));
        editor.recording = false;
        editor.intercept = None;
        editor.error = None;
        editor.refused = None;
        if editor.target == Target::Find {
            let keys = keymap::normal_keys(&editor.strokes.join(" "));
            self.keys.find = Some(keys);
            self.keys.editor = None;
            self.keys.list.scroll_to_item(0, ScrollStrategy::Top);
            window.focus(&self.keys.focus, cx);
        } else {
            // Enter then saves: the form's button for a new shortcut, the
            // bar's for the rest.
            let save = if editor.target == Target::New {
                "new-save"
            } else {
                "keys-save"
            };
            let save = self.handle(save, cx);
            window.focus(&save, cx);
        }
        cx.notify();
    }

    /// Take the strokes again: from none, or `more` after those taken.
    fn record_again(&mut self, more: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.keys.editor.as_mut() else {
            return;
        };
        if !more {
            editor.strokes.clear();
        }
        editor.typed = None;
        editor.recording = true;
        editor.error = None;
        editor.refused = None;
        window.focus(&editor.focus.clone(), cx);
        self.listen_for_keys(cx);
        cx.notify();
    }

    /// Type the keys rather than press them.
    fn type_keys(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let typed = self
            .keys
            .editor
            .as_ref()
            .map(|e| e.strokes.join(" "))
            .unwrap_or_default();
        let input = cx.new(|cx| {
            let mut input = TextInput::new(
                words!(
                    keys_type_placeholder = "ctrl-shift-n, or g h for one key after another",
                    note = "The placeholder of the field keys are typed in: ctrl-shift-n and g h are keys as the keymap writes them, kept as they are."
                ),
                cx,
            );
            input.set_text(typed, cx);
            input
        });
        let subscription = cx.subscribe_in(
            &input,
            window,
            |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::Submit => this.save_key_editor(window, cx),
                InputEvent::Cancel => this.close_key_editor(window, cx),
                InputEvent::Changed => {
                    if let Some(editor) = this.keys.editor.as_mut() {
                        editor.error = None;
                        editor.refused = None;
                    }
                    cx.notify();
                }
            },
        );
        let Some(editor) = self.keys.editor.as_mut() else {
            return;
        };
        editor.recording = false;
        editor.intercept = None;
        editor.subscriptions.push(subscription);
        input.update(cx, |input, cx| input.focus(window, cx));
        editor.typed = Some(input);
        cx.notify();
    }

    /// Close the bar; the list has the keyboard again.
    pub(crate) fn close_key_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let new = self
            .keys
            .editor
            .take()
            .is_some_and(|editor| editor.target == Target::New);
        if new {
            self.focus_new_shortcut(window, cx);
        } else {
            window.focus(&self.keys.focus, cx);
        }
        cx.notify();
    }

    /// What the bar says of its change: what the keys pressed would do
    /// where they work, and what the change leaves of every action's keys.
    /// Worked out again only when the keys or the change itself change
    /// (a keymap built twice is too much for every frame).
    pub(crate) fn judged(&self, cx: &App) -> Option<Rc<Judged>> {
        let editor = self.keys.editor.as_ref()?;
        let typed = editor.keys(cx);
        let keys = if typed.is_empty() || skwir_core::keymap::valid_keys(&typed).is_err() {
            String::new()
        } else {
            typed
        };
        let (action, context, chip) = match &editor.target {
            Target::Add(id) => {
                let action = self.keys.keyboard.find(id)?.clone();
                let context = action.context();
                (action, context, None)
            }
            Target::Change(id, chip) | Target::Restore(id, chip) => (
                self.keys.keyboard.find(id)?.clone(),
                chip.context.clone(),
                Some(chip.clone()),
            ),
            Target::Reset(id) => (self.keys.keyboard.find(id)?.clone(), String::new(), None),
            Target::New => {
                let (action, context) = self.new_shortcut_action(cx)?;
                (action, context, None)
            }
            Target::Find => return None,
        };
        // What it says is kept while its language is the one said.
        let asked = format!(
            "{:?}\u{1f}{keys}\u{1f}{context}\u{1f}{}\u{1f}{}\u{1f}{}",
            editor.target,
            action.command,
            action.args,
            words::revision()
        );
        if let Some((kept, judged)) = editor.judged.borrow().as_ref()
            && *kept == asked
        {
            return Some(Rc::clone(judged));
        }
        let edit = match (&editor.target, chip.as_ref()) {
            (Target::Restore(..), Some(chip)) => Some(Edit::Restore(chip)),
            (Target::Reset(_), _) => Some(Edit::Reset(&action)),
            // No keys yet: what freeing these would leave, which changing
            // them leaves too.
            (Target::Change(..), Some(chip)) if keys.is_empty() => Some(Edit::Remove(chip)),
            (Target::Change(..), Some(chip)) => Some(Edit::Change {
                action: &action,
                chip,
                keys: keys.clone(),
            }),
            _ if keys.is_empty() => None,
            _ => Some(Edit::Add {
                action: &action,
                keys: keys.clone(),
                context: context.clone(),
            }),
        };
        let verdict = if keys.is_empty() {
            Verdict::default()
        } else {
            let replacing = chip
                .as_ref()
                .filter(|_| matches!(editor.target, Target::Change(..)));
            verdict(
                &self.app,
                &self.keys.keyboard,
                &self.keys.bound,
                &keys,
                &context,
                &action,
                replacing,
            )
        };
        let effects = match &edit {
            Some(edit) => effects(&self.app, &self.key_records(), edit, Some(&action.id())),
            None => Effects::default(),
        };
        let judged = Rc::new(Judged {
            verdict,
            effects,
            keys,
        });
        *editor.judged.borrow_mut() = Some((asked, Rc::clone(&judged)));
        Some(judged)
    }

    /// Save what the bar holds, unless it cannot be: the reason stays in
    /// the bar.
    pub(crate) fn save_key_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.keys.editor.as_ref() else {
            return;
        };
        let target = editor.target.clone();
        let keys = editor.keys(cx);
        let judged = self.judged(cx);
        if target.takes_keys()
            && judged
                .as_ref()
                .is_some_and(|judged| judged.verdict.notes.contains(&Note::Has))
        {
            // Keys it has there already: nothing to keep.
            self.close_key_editor(window, cx);
            return;
        }
        let error = if !target.takes_keys() {
            None
        } else if keys.is_empty() {
            Some(words!(
                keys_press_first = "Press the keys first",
                note = "Why the bar of the Keyboard page saves nothing: no keys were pressed or typed."
            ))
        } else if let Err(error) = skwir_core::keymap::valid_keys(&keys) {
            Some(Words::of_error(error))
        } else {
            None
        };
        if let Some(error) = error {
            if let Some(editor) = self.keys.editor.as_mut() {
                editor.error = Some(error.say());
            }
            cx.notify();
            return;
        }
        // Refused: the bar says why (`key_notes`), once.
        if let Some(why) = judged.as_ref().and_then(|judged| judged.refused()) {
            let why = why.clone();
            if let Some(editor) = self.keys.editor.as_mut() {
                editor.refused = Some(why);
            }
            cx.notify();
            return;
        }
        let action = target
            .action()
            .and_then(|id| self.keys.keyboard.find(id))
            .cloned();
        let saved = match (&target, action) {
            (Target::Add(_), Some(action)) => self.save_keys(
                &Edit::Add {
                    context: action.context(),
                    action: &action,
                    keys,
                },
                cx,
            ),
            (Target::Change(_, chip), Some(action)) => self.save_keys(
                &Edit::Change {
                    action: &action,
                    chip,
                    keys,
                },
                cx,
            ),
            (Target::Restore(_, chip), Some(_)) => self.save_keys(&Edit::Restore(chip), cx),
            (Target::Reset(_), Some(action)) => self.save_keys(&Edit::Reset(&action), cx),
            (Target::New, _) => self.save_new_shortcut(&keys, cx),
            _ => false,
        };
        if saved {
            if target == Target::New {
                self.keys.new = None;
            }
            self.close_key_editor(window, cx);
        } else if let Some(editor) = self.keys.editor.as_mut() {
            editor.error = self.errors.get(BINDINGS_KEY).map(Words::say).or_else(|| {
                Some(
                    words!(
                        keys_not_kept = "These keys cannot be kept",
                        note = "Why the user's new keys were not saved, where the setting gave no reason."
                    )
                    .say(),
                )
            });
            cx.notify();
        }
    }

    /// Why freeing or removing `chip` of `action` cannot be done, if it
    /// cannot: an action that keeps a key keeps its last, its own or
    /// another's (`effects`).
    fn removal_refusal(&self, action: &Action, chip: &Chip) -> Option<Words> {
        removal_refused(action, chip).or_else(|| {
            effects(
                &self.app,
                &self.key_records(),
                &Edit::Remove(chip),
                Some(&action.id()),
            )
            .refused
        })
    }

    /// Remove the key the bar edits, unless it is the last of an action
    /// that keeps one.
    fn remove_edited_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Target::Change(id, chip)) = self.keys.editor.as_ref().map(|e| e.target.clone())
        else {
            return;
        };
        let Some(action) = self.keys.keyboard.find(&id).cloned() else {
            return;
        };
        if let Some(why) = self.removal_refusal(&action, &chip) {
            if let Some(editor) = self.keys.editor.as_mut() {
                editor.refused = Some(why);
            }
            cx.notify();
            return;
        }
        if self.save_keys(&Edit::Remove(&chip), cx) {
            self.close_key_editor(window, cx);
        }
    }

    /// Back to Skwir's keys for `id`: at once, unless that takes keys from
    /// other actions (a shortcut of the user's over one of its defaults),
    /// which the bar says first; refused when an action that keeps a key
    /// would have none.
    fn reset_keys(&mut self, id: &ActionId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(action) = self.keys.keyboard.find(id).cloned() else {
            return;
        };
        let edit = Edit::Reset(&action);
        let judged = effects(&self.app, &self.key_records(), &edit, Some(id));
        if let Some(why) = judged.refused {
            self.errors.insert(BINDINGS_KEY.to_owned(), why);
            cx.notify();
        } else if judged.lost.is_empty() {
            self.save_keys(&edit, cx);
        } else {
            self.open_key_editor(Target::Reset(id.clone()), window, cx);
        }
    }

    /// The keys being taken: caps, or an invitation to press them, or the
    /// field they are typed in.
    pub(crate) fn key_recorder(
        &mut self,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let Some(editor) = self.keys.editor.as_ref() else {
            return div();
        };
        let (recording, strokes, typed, focus) = (
            editor.recording,
            editor.strokes.clone(),
            editor.typed.clone(),
            editor.focus.clone(),
        );
        let invalid = editor.error.is_some() || editor.refused.is_some();
        let finding = editor.target == Target::Find;
        let shown: AnyElement = if let Some(input) = &typed {
            controls::field(
                input,
                Some(260.),
                input.read(cx).is_focused(window),
                invalid,
                Some("keyboard"),
                theme,
            )
            .into_any_element()
        } else {
            let mut pad = div()
                .id("keys-pad")
                .track_focus(&focus)
                .h(sized(36.))
                .min_w(sized(220.))
                .px_3()
                .flex()
                .items_center()
                .gap_2()
                .rounded_md()
                .border_1()
                .border_color(if recording {
                    theme.accent
                } else {
                    theme.border
                })
                .bg(theme.bg)
                .cursor_pointer()
                .on_click(cx.listener(|this, _, window, cx| this.record_again(false, window, cx)));
            if !strokes.is_empty() {
                pad = pad.child(caps(&strokes.join(" "), false, theme));
            }
            if recording {
                pad = pad.child(
                    div()
                        .text_color(theme.text_muted)
                        .child(
                            if strokes.is_empty() {
                                if finding {
                                    words!(
                                        keys_press_to_find = "Press the keys to find…",
                                        note = "What the Keyboard page's recorder waits for: the keys whose actions to list."
                                    )
                                } else {
                                    words!(
                                        keys_press_prompt = "Press the keys…",
                                        note = "What the Keyboard page's recorder waits for: the keys to give an action."
                                    )
                                }
                            } else {
                                words!(
                                    keys_press_next = "then the next key…",
                                    note = "What the Keyboard page's recorder waits for after a first key: the next of a sequence (g then h)."
                                )
                            }
                            .say(),
                        ),
                );
            }
            pad.into_any_element()
        };
        let mut row = div().flex().flex_wrap().items_center().gap_2().child(shown);
        if !recording && typed.is_none() && !strokes.is_empty() && !finding {
            let again = self.handle("keys-again", cx);
            self.stop(&again, None, None);
            row = row.child(
                controls::button(
                    "keys-again",
                    words!(
                        keys_press_again = "Press again",
                        note = "A button of the Keyboard page's recorder: take the keys again from the first."
                    )
                    .say(),
                    None,
                    ButtonStyle::Plain,
                    &again,
                    theme,
                )
                .on_click(cx.listener(|this, _, window, cx| this.record_again(false, window, cx))),
            );
            if strokes.len() < STROKES {
                let more = self.handle("keys-more", cx);
                self.stop(&more, None, None);
                row = row.child(
                    controls::button(
                        "keys-more",
                        words!(
                            keys_then_another = "Then another key",
                            note = "A button of the Keyboard page's recorder: one key more after those taken, a sequence (g then h)."
                        )
                        .say(),
                        None,
                        ButtonStyle::Plain,
                        &more,
                        theme,
                    )
                    .on_click(
                        cx.listener(|this, _, window, cx| this.record_again(true, window, cx)),
                    ),
                );
            }
        }
        if typed.is_none() && !finding {
            let typing = self.handle("keys-type", cx);
            self.stop(&typing, None, None);
            row = row.child(
                controls::button(
                    "keys-type",
                    words!(
                        keys_type_them = "Type them",
                        note = "A button of the Keyboard page's recorder: write the keys rather than press them (those the desktop keeps)."
                    )
                    .say(),
                    None,
                    ButtonStyle::Plain,
                    &typing,
                    theme,
                )
                .on_click(cx.listener(|this, _, window, cx| this.type_keys(window, cx))),
            );
        }
        if recording && !strokes.is_empty() {
            row = row.child(
                div()
                    .text_body()
                    .text_color(theme.text_muted)
                    .child(
                        words!(
                            keys_esc_is_a_key = "Esc is a key here too: Cancel is a click away.",
                            note = "Said while the Keyboard page's recorder takes keys: Escape is taken as a key, Cancel is the bar's button."
                        )
                        .say(),
                    ),
            );
        }
        row
    }

    /// What the keys would do, said in the bar: what they replace, what
    /// they leave running near them, and only how many places keep their
    /// own when there are many (Esc means something in most).
    pub(crate) fn key_notes(&self, theme: &Theme, cx: &App) -> gpui::Stateful<gpui::Div> {
        let mut notes = div()
            .id("keys-notes")
            .max_h(sized(112.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_1();
        let line = |icon: &str, color: gpui::Hsla, text: String| {
            controls::noted(icon, color, 13., text)
                .text_body()
                .text_color(color)
        };
        let judged = self.judged(cx);
        if let Some(judged) = &judged {
            let verdict = &judged.verdict;
            let keys = keymap::display_keys(&judged.keys);
            let deeper: Vec<&str> = verdict
                .notes
                .iter()
                .filter_map(|note| match note {
                    Note::Stays {
                        place,
                        why: Stay::Deeper,
                        ..
                    } => Some(place.as_str()),
                    _ => None,
                })
                .collect();
            if deeper.len() > 2 {
                // Transitional: a list argument (ADR-139).
                let some = deeper[..3].join(", ");
                notes = notes.child(line(
                    "info",
                    theme.text_muted,
                    words!(
                        keys_note_deeper_many = "{ $count ->
       *[other] { $keys } still runs what it runs in { $count } places that bind it themselves ({ $some }…): a place's own keys come first.
    }",
                        keys = skwir_core::words::Keys(keys.clone()),
                        count = deeper.len(),
                        some = some,
                        note = "Said before new keys are saved, anywhere: keys are the keys (Ctrl+Q), count the places with keys of their own, some the first three of them (File list, Address field, Sidebar)."
                    )
                    .say(),
                ));
            }
            for note in &verdict.notes {
                let (icon, color, text) = match note {
                    Note::Replaces { title, place, .. } => (
                        "warning",
                        theme.warning,
                        words!(
                            keys_note_replaces = "{ $keys } runs “{ $action }” now ({ $place }): yours replaces it there.",
                            keys = skwir_core::words::Keys(keys.clone()),
                            action = title.clone(),
                            place = place.clone(),
                            note = "Said before new keys are saved: keys are the keys (Ctrl+Q), action what they run now (Quit), place where (Anywhere, File list)."
                        ),
                    ),
                    Note::Stays {
                        why: Stay::Deeper, ..
                    } if deeper.len() > 2 => continue,
                    Note::Stays {
                        title,
                        place,
                        why: Stay::Deeper,
                    } => (
                        "info",
                        theme.text_muted,
                        words!(
                            keys_note_deeper = "In { $place }, { $keys } still runs “{ $action }”: a place's own keys come first.",
                            keys = skwir_core::words::Keys(keys.clone()),
                            action = title.clone(),
                            place = place.clone(),
                            note = "Said before new keys are saved, anywhere: keys are the keys (Ctrl+Q), action what they keep running in place (File list)."
                        ),
                    ),
                    Note::Stays {
                        title,
                        why: Stay::Elsewhere,
                        ..
                    } => (
                        "info",
                        theme.text_muted,
                        words!(
                            keys_note_elsewhere = "Elsewhere, { $keys } still runs “{ $action }”.",
                            keys = skwir_core::words::Keys(keys.clone()),
                            action = title.clone(),
                            note = "Said before new keys for one place are saved: keys are the keys (Ctrl+Q), action what they keep running in the rest of the window."
                        ),
                    ),
                    Note::Stays {
                        title,
                        place,
                        why: Stay::Besides,
                    } => (
                        "info",
                        theme.text_muted,
                        words!(
                            keys_note_besides = "{ $keys } still runs “{ $action }” in { $place }.",
                            keys = skwir_core::words::Keys(keys.clone()),
                            action = title.clone(),
                            place = place.clone(),
                            note = "Said before new keys are saved: keys are the keys (Ctrl+Q), action what they keep running in place, under conditions the new keys do not ask for (File list (with a selection))."
                        ),
                    ),
                    Note::Waits { title, goes: true } => (
                        "plugin",
                        theme.warning,
                        words!(
                            keys_note_waiting_goes = "Your “{ $action }”, waiting on { $keys } for its plugin, goes: yours takes these keys there.",
                            keys = skwir_core::words::Keys(keys.clone()),
                            action = title.clone(),
                            note = "Said before new keys are saved: a shortcut of the user's on the same keys (keys, Ctrl+Q) runs action, whose plugin is off; it is replaced."
                        ),
                    ),
                    Note::Waits { title, goes: false } => (
                        "plugin",
                        theme.text_muted,
                        words!(
                            keys_note_waiting = "Your “{ $action }” waits on { $keys } for its plugin: once it is back, the later of the two runs.",
                            keys = skwir_core::words::Keys(keys.clone()),
                            action = title.clone(),
                            note = "Said before new keys are saved: a shortcut of the user's on the same keys (keys, Ctrl+Q) runs action, whose plugin is off; it is kept."
                        ),
                    ),
                    Note::Typing => (
                        "info",
                        theme.text_muted,
                        words!(
                            keys_note_typing = "A letter bound in the file list stops typing a name there to jump to it; / then finds a name.",
                            note = "Said before a letter is bound in the file list: typing there no longer jumps to a name; the / key still finds one."
                        ),
                    ),
                    Note::Fields => (
                        "info",
                        theme.text_muted,
                        words!(
                            keys_note_fields = "In a text field, { $keys } still types: a field keeps its characters.",
                            keys = skwir_core::words::Keys(keys.clone()),
                            note = "Said before a key that types a character is bound: keys are the keys (Shift+A)."
                        ),
                    ),
                    Note::Has => (
                        "check",
                        theme.text_muted,
                        words!(
                            keys_note_has = "It has { $keys } here already.",
                            keys = skwir_core::words::Keys(keys.clone()),
                            note = "Said when the keys pressed for an action are already its keys there: keys are the keys (Ctrl+Q)."
                        ),
                    ),
                };
                notes = notes.child(line(icon, color, text.say()));
            }
            // What the change leaves of other actions' keys, beside what
            // was said: what freeing keys in a place frees with them.
            // Told by what they run, never by their titles.
            let replaced: std::collections::HashSet<&Does> = verdict
                .notes
                .iter()
                .filter_map(|note| match note {
                    Note::Replaces { does, .. } => Some(does),
                    _ => None,
                })
                .collect();
            let pressed = keymap::normal_keys(&judged.keys);
            let hider = self.hider().map(Action::does);
            for lost in &judged.effects.lost {
                let said =
                    keymap::normal_keys(&lost.keys) == pressed && replaced.contains(&lost.does);
                if said || hider.as_ref() == Some(&lost.does) {
                    continue;
                }
                notes = notes.child(line(
                    "warning",
                    theme.warning,
                    words!(
                        keys_note_lost = "{ $keys } also stops running “{ $action }” ({ $place }).",
                        keys = skwir_core::words::Keys(keymap::display_keys(&lost.keys)),
                        action = lost.title.clone(),
                        place = lost.place.clone(),
                        note = "Said before a change of keys is saved: keys (Escape) no longer run action (Clear the filter) in place (File list)."
                    )
                    .say(),
                ));
            }
        }
        // Why the change is refused, as judged now, else as the last change
        // asked met it: said once.
        let editor = self.keys.editor.as_ref();
        let refused = judged
            .as_ref()
            .and_then(|judged| judged.refused().cloned())
            .or_else(|| editor.and_then(|e| e.refused.clone()))
            .map(|why| why.say());
        let error = editor.and_then(|e| e.error.clone());
        for why in refused.into_iter().chain(error) {
            notes = notes.child(line("warning", theme.error, why));
        }
        notes
    }

    /// The action whose shortcut took the keys the Restore bar gives
    /// back, if one did (none: the user freed them).
    fn hider(&self) -> Option<&Action> {
        let Some(Target::Restore(_, chip)) = self.keys.editor.as_ref().map(|e| &e.target) else {
            return None;
        };
        let id = chip.hidden_by.as_ref()?;
        self.keys
            .keyboard
            .actions
            .iter()
            .find(|a| a.chips.iter().any(|c| c.record.as_ref() == Some(id)))
    }

    /// The bar under the list, while keys are edited.
    pub(crate) fn key_editor_bar(
        &mut self,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let editor = self.keys.editor.as_ref()?;
        if editor.target == Target::New {
            return None;
        }
        let target = editor.target.clone();
        let action = target
            .action()
            .and_then(|id| self.keys.keyboard.find(id))
            .cloned();
        let heading: String = match (&target, &action) {
            (Target::Find, _) => words!(
                keys_heading_find = "Find the actions of a key",
                note = "The heading of the Keyboard page's bar while it takes the keys whose actions to list."
            )
            .say(),
            (Target::Add(_), Some(action)) => words!(
                keys_heading_add = "New keys for “{ $action }” · { $place }",
                action = action.title.clone(),
                place = action.place.clone(),
                note = "The heading of the Keyboard page's bar: new keys for action (Quit), working in place (Anywhere, File list)."
            )
            .say(),
            (Target::Change(_, chip), Some(action)) => words!(
                keys_heading_change = "{ $keys } for “{ $action }” · { $place }",
                keys = skwir_core::words::Keys(keymap::display_keys(&chip.keys)),
                action = action.title.clone(),
                place = action.place.clone(),
                note = "The heading of the Keyboard page's bar: keys (Ctrl+Q) of action (Quit) changed, in place (Anywhere)."
            )
            .say(),
            (Target::Restore(_, chip), Some(action)) => words!(
                keys_heading_restore = "{ $keys } no longer runs “{ $action }” · { $place }",
                keys = skwir_core::words::Keys(keymap::display_keys(&chip.keys)),
                action = action.title.clone(),
                place = action.place.clone(),
                note = "The heading of the Keyboard page's bar: keys of skwir's (Ctrl+Q) a shortcut of the user's took from action (Quit), in place."
            )
            .say(),
            (Target::Reset(_), Some(action)) => words!(
                keys_heading_reset = "Back to Skwir's keys for “{ $action }” · { $place }",
                action = action.title.clone(),
                place = action.place.clone(),
                note = "The heading of the Keyboard page's bar: action's keys as skwir ships them, in place."
            )
            .say(),
            _ => String::new(),
        };
        let mut bar = div()
            .flex_none()
            .mx_6()
            .mb_4()
            .p_3()
            .flex()
            .flex_col()
            .gap_3()
            .rounded_lg()
            .border_1()
            .border_color(theme.border)
            .bg(theme.surface)
            .shadow_md()
            .child(div().child(heading));
        match &target {
            Target::Restore(..) => {
                bar = bar
                    .child(div().text_body().text_color(theme.text_muted).child(
                        match self.hider() {
                            Some(hider) => words!(
                                keys_restore_hider = "Your shortcut for “{ $action }” took these keys. Restoring removes it.",
                                action = hider.title.clone(),
                                note = "Said in the Keyboard page's bar before keys of skwir's are restored: a shortcut of the user's for action (Quit) has them."
                            ),
                            None => words!(
                                keys_restore_freed = "You freed these keys. Restoring gives them back.",
                                note = "Said in the Keyboard page's bar before keys of skwir's the user freed are restored."
                            ),
                        }
                        .say(),
                    ))
                    .child(self.key_notes(theme, cx));
            }
            Target::Reset(_) => {
                bar = bar
                    .child(
                        div().text_body().text_color(theme.text_muted).child(
                            words!(
                                keys_reset_detail = "Its keys go back to Skwir's, and your shortcuts that took them go.",
                                note = "Said in the Keyboard page's bar before an action's keys go back to those skwir ships."
                            )
                            .say(),
                        ),
                    )
                    .child(self.key_notes(theme, cx));
            }
            _ => {
                bar = bar.child(self.key_recorder(theme, window, cx));
                bar = bar.child(self.key_notes(theme, cx));
            }
        }
        let cancel = self.handle("keys-cancel", cx);
        self.stop(&cancel, None, None);
        let mut buttons = div().flex().items_center().gap_2();
        if let Target::Change(_, chip) = &target {
            let remove = self.handle("keys-remove", cx);
            self.stop(&remove, None, None);
            let what = if chip.record.is_some() {
                words!(
                    keys_remove = "Remove these keys",
                    note = "A button of the Keyboard page's bar: the user's keys for an action go."
                )
            } else {
                words!(
                    keys_free = "Free these keys",
                    note = "A button of the Keyboard page's bar: keys of skwir's no longer run their action."
                )
            }
            .say();
            buttons = buttons.child(
                controls::button(
                    "keys-remove",
                    what,
                    Some("minus"),
                    ButtonStyle::Danger,
                    &remove,
                    theme,
                )
                .on_click(cx.listener(|this, _, window, cx| this.remove_edited_key(window, cx))),
            );
        }
        buttons = buttons.child(div().flex_1()).child(
            controls::button(
                "keys-cancel",
                words!(
                    keys_cancel = "Cancel",
                    note = "A button of the Keyboard page's bar: close it, nothing changed."
                )
                .say(),
                None,
                ButtonStyle::Plain,
                &cancel,
                theme,
            )
            .on_click(cx.listener(|this, _, window, cx| this.close_key_editor(window, cx))),
        );
        if target != Target::Find {
            let save = self.handle("keys-save", cx);
            self.stop(&save, None, None);
            buttons = buttons.child(
                controls::button(
                    "keys-save",
                    match target {
                        Target::Restore(..) => words!(
                            keys_restore = "Restore",
                            note = "A button of the Keyboard page's bar: keys of skwir's run their action again."
                        ),
                        Target::Reset(_) => words!(
                            keys_reset_save = "Back to Skwir's keys",
                            note = "A button of the Keyboard page's bar: an action's keys as skwir ships them."
                        ),
                        _ => words!(
                            keys_save = "Save",
                            note = "A button of the Keyboard page's bar: keep the keys pressed."
                        ),
                    }
                    .say(),
                    None,
                    ButtonStyle::Plain,
                    &save,
                    theme,
                )
                .on_click(cx.listener(|this, _, window, cx| this.save_key_editor(window, cx))),
            );
        }
        Some(bar.child(buttons).into_any_element())
    }

    // ------------------------------------------------------------ keys

    /// `ui.settings_key { key }`: walk the actions and their keys, change
    /// the key the cursor is on or add one, remove it. In a search, the
    /// actions it found.
    pub(crate) fn keys_key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.query(cx);
        let rows = self.key_rows(&query, cx);
        let actions: Vec<(usize, usize)> = rows
            .iter()
            .enumerate()
            .filter_map(|(n, row)| match row {
                Row::Action(index) => Some((n, *index)),
                _ => None,
            })
            .collect();
        if actions.is_empty() {
            return;
        }
        let keyboard = Rc::clone(&self.keys.keyboard);
        let at = self.keys.cursor.as_ref().and_then(|(id, _)| {
            actions
                .iter()
                .position(|(_, index)| keyboard.actions[*index].id() == *id)
        });
        let chip = self.keys.cursor.as_ref().map_or(0, |(_, chip)| *chip);
        let page = 8;
        let moved = match (key, at) {
            ("first", _) | (_, None) => Some(0),
            ("last", _) => Some(actions.len() - 1),
            ("up", Some(at)) => Some(at.saturating_sub(1)),
            ("down", Some(at)) => Some((at + 1).min(actions.len() - 1)),
            ("page_up", Some(at)) => Some(at.saturating_sub(page)),
            ("page_down", Some(at)) => Some((at + page).min(actions.len() - 1)),
            _ => None,
        };
        if let Some(to) = moved {
            let (n, index) = actions[to];
            let chips = keyboard.actions[index].chips.len();
            self.keys.cursor = Some((keyboard.actions[index].id(), chip.min(chips)));
            // A search's few rows are the page's own, not the list's.
            if query.is_empty() {
                self.keys.list.scroll_to_item(n, ScrollStrategy::Center);
            }
            cx.notify();
            return;
        }
        let Some(at) = at else { return };
        let action = &keyboard.actions[actions[at].1];
        let id = action.id();
        match key {
            "left" => self.keys.cursor = Some((id, chip.saturating_sub(1))),
            "right" => self.keys.cursor = Some((id, (chip + 1).min(action.chips.len()))),
            "open" => {
                let target = match action.chips.get(chip) {
                    None => Target::Add(id),
                    Some(chip) if chip.runs() => Target::Change(id, chip.clone()),
                    Some(chip) => Target::Restore(id, chip.clone()),
                };
                self.open_key_editor(target, window, cx);
            }
            "remove" => {
                if let Some(chip) = action.chips.get(chip).filter(|c| c.runs()) {
                    let freed = effects(
                        &self.app,
                        &self.key_records(),
                        &Edit::Remove(chip),
                        Some(&id),
                    );
                    if let Some(why) = removal_refused(action, chip).or(freed.refused) {
                        self.errors.insert(BINDINGS_KEY.to_owned(), why);
                    } else if freed.lost.is_empty() {
                        self.save_keys(&Edit::Remove(chip), cx);
                    } else {
                        // Freeing it frees other actions' keys beside it:
                        // the bar says which before anything is saved.
                        self.open_key_editor(Target::Change(id, chip.clone()), window, cx);
                    }
                }
            }
            _ => {}
        }
        cx.notify();
    }
}

/// The caption of the place at the top of the list, kept there while its
/// actions scroll under it, and pushed up by the next place's.
struct Sticky {
    rows: Rc<Vec<Row>>,
    theme: Theme,
}

impl UniformListDecoration for Sticky {
    fn compute(
        &self,
        visible_range: Range<usize>,
        _bounds: Bounds<Pixels>,
        scroll_offset: Point<Pixels>,
        item_height: Pixels,
        _item_count: usize,
        _window: &mut Window,
        _cx: &mut App,
    ) -> AnyElement {
        let top = visible_range.start;
        let place = self.rows[..=top.min(self.rows.len().saturating_sub(1))]
            .iter()
            .rev()
            .find_map(|row| match row {
                Row::Place(place) => Some(place.clone()),
                _ => None,
            });
        // The first row is a caption already: nothing to keep.
        let Some(place) = place.filter(|_| !matches!(self.rows.get(top), Some(Row::Place(_))))
        else {
            return div().into_any_element();
        };
        // The next caption, arriving, pushes this one up.
        let into = -scroll_offset.y - item_height * top as f32;
        let push = match self.rows.get(top + 1) {
            Some(Row::Place(_) | Row::Inside { .. }) => into.min(item_height),
            _ => Pixels::ZERO,
        };
        // Laid out as a root, an element's own offset is not kept: the
        // caption hangs in a frame the size of the list.
        div()
            .size_full()
            .child(
                div()
                    .absolute()
                    .top(-scroll_offset.y - push)
                    .left_0()
                    .w_full()
                    .h(item_height)
                    .px_6()
                    .flex()
                    .items_end()
                    .pb_2()
                    .bg(self.theme.bg)
                    .text_small()
                    .text_color(self.theme.text_muted)
                    .occlude()
                    .child(place),
            )
            .into_any_element()
    }
}
