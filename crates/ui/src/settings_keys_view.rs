//! The Keyboard page (ADR-024), drawn: the keymap as `settings_keys` reads
//! it, in one list that draws the rows in view, with its scrollbar and the
//! caption of the place at its top kept there. A key's chip, or a row's +,
//! opens the bar under the list, which takes the next keys pressed before
//! any binding sees them, and says what they would do before they are
//! saved. The list walks with the keyboard (`ui.settings_key`).

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    AnyElement, App, Bounds, Context, Entity, FocusHandle, Keystroke, Pixels, Point,
    ScrollStrategy, SharedString, Subscription, UniformListDecoration, UniformListScrollHandle,
    Window, div, prelude::*, px,
};
use skwir_core::keymap::BINDINGS_KEY;

use crate::controls::{self, ButtonStyle};
use crate::input::{InputEvent, TextInput};
use crate::keymap::{self, Bound};
use crate::settings::SettingsView;
use crate::settings_keys::{
    Action, ActionId, Chip, Edit, Keyboard, Note, Row, Show, Stay, Verdict, apply, removal_refused,
    rows, verdict,
};
use crate::settings_records::{Record, records};
use crate::settings_view::{Edge, card_row};
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
    pub list: UniformListScrollHandle,
    /// The list's: it walks the actions (`ui.settings_key`).
    pub focus: FocusHandle,
    pub show: Show,
    /// The keys inside dialogs and menus are unfolded.
    pub inside: bool,
    /// Keys whose actions are listed, as `keymap::normal_keys` writes them.
    pub find: Option<String>,
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
    find: Option<String>,
    inside: bool,
    generation: u64,
}

impl KeysPage {
    pub fn new(bound: Arc<Vec<Bound>>, cx: &mut App) -> Self {
        KeysPage {
            bound,
            keyboard: Rc::default(),
            rows: RefCell::default(),
            generation: 0,
            list: UniformListScrollHandle::new(),
            focus: cx.focus_handle(),
            show: Show::default(),
            inside: false,
            find: None,
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
    /// Keys whose actions to list.
    Find,
    /// The keys of a new shortcut (`settings_keys_new`).
    New,
}

impl Target {
    fn action(&self) -> Option<&ActionId> {
        match self {
            Target::Add(id) | Target::Change(id, _) | Target::Restore(id, _) => Some(id),
            Target::Find | Target::New => None,
        }
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
    /// Where the keyboard is while strokes are taken.
    pub focus: FocusHandle,
    intercept: Option<Subscription>,
    /// What its typed keys' field says, while it is open.
    subscriptions: Vec<Subscription>,
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
    let mut all = div().flex().flex_none().items_center().gap(px(8.));
    for stroke in keymap::key_caps(keys) {
        let mut one = div().flex().items_center().gap(px(3.));
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
        self.keys.generation += 1;
        self.keys.rows.take();
    }

    /// The rows for `query`, kept until what is asked or the keymap
    /// changes.
    fn key_rows(&self, query: &str) -> Rc<Vec<Row>> {
        let asked = Asked {
            show: self.keys.show,
            query: query.to_owned(),
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
    pub(crate) fn keys_toolbar(&mut self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
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
            (Show::Keyed, "With keys".to_owned()),
            (Show::All, "All actions".to_owned()),
            (
                Show::Changed,
                if changed == 0 {
                    "Changed by you".to_owned()
                } else {
                    format!("Changed by you ({changed})")
                },
            ),
        ] {
            let focus = show.clone();
            segmented = segmented.child(
                controls::segment(
                    SharedString::from(format!("keys-show-{label}")),
                    label,
                    current == choice,
                    theme,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    window.focus(&focus);
                    this.keys.show = choice;
                    this.keys.as_records = false;
                    this.keys.new = None;
                    cx.notify();
                })),
            );
        }
        let find = self.handle("keys-find", cx);
        self.stop(&find, None, None);
        let finding = match &self.keys.find {
            Some(keys) => div()
                .flex()
                .items_center()
                .gap_1()
                .child(caps(keys, false, theme))
                .child(
                    controls::icon_button("keys-find-clear", "close", &find, theme).on_click(
                        cx.listener(|this, _, _, cx| {
                            this.keys.find = None;
                            cx.notify();
                        }),
                    ),
                )
                .into_any_element(),
            None => controls::button(
                "keys-find",
                "Find by keys",
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
        let new = self.handle("keys-new", cx);
        self.stop(&new, None, None);
        let records = self.handle("keys-records", cx);
        self.stop(&records, None, None);
        let as_records = self.keys.as_records;
        let waiting = self.keys.keyboard.waiting.len();
        let note = (waiting > 0).then(|| {
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(controls::glyph("plugin", theme.text_muted, 13.))
                .child(if waiting == 1 {
                    "One of your shortcuts waits for a plugin that is off: it comes back with it."
                        .to_owned()
                } else {
                    format!(
                        "{waiting} of your shortcuts wait for plugins that are off: they come back with them."
                    )
                })
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
                    .child(finding)
                    .child(
                        controls::button(
                            "keys-new",
                            "New shortcut…",
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
            // What waits, and the records as they are stored: small, as
            // what most never need.
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_xs()
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
                            .child(if as_records {
                                "Back to the list"
                            } else {
                                "Edit as records"
                            })
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
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_xs()
                    .text_color(theme.error)
                    .child(controls::glyph("warning", theme.error, 13.))
                    .child(div().flex_1().min_w_0().child(error.clone())),
            );
        }
        bar.into_any_element()
    }

    /// Every row asked for, in a list that draws those in view: hundreds
    /// of bindings were built at every frame the dialog drew.
    pub(crate) fn keys_list(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let rows = self.key_rows("");
        let focus = self.keys.focus.clone();
        self.stop(&focus, None, None);
        if rows.is_empty() {
            return div()
                .flex_1()
                .px_6()
                .py_6()
                .track_focus(&focus)
                .text_color(cx.global::<Theme>().text_muted)
                .child(match self.keys.show {
                    Show::Changed => "You changed no keys yet.",
                    _ => "No action has these keys.",
                })
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
                                .h(px(ROW))
                                .flex()
                                .items_end()
                                .pb_2()
                                .text_xs()
                                .text_color(theme.text_muted)
                                .child(place.clone())
                                .into_any_element(),
                            Row::Inside { open, count } => this
                                .inside_row(*open, *count, &theme, cx)
                                .into_any_element(),
                            Row::Action(index) => this
                                .action_row(*index, edge(&rows, n), focused, &theme, cx)
                                .h(px(ROW))
                                .into_any_element(),
                        };
                        Some(div().w_full().px_6().child(element).into_any_element())
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(self.keys.list.clone())
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
                window.focus(&this.keys.focus);
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
        div()
            .id("keys-inside")
            .h(px(ROW))
            .mt_2()
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .rounded_md()
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .text_xs()
            .text_color(theme.text_muted)
            .child(controls::glyph(
                if open { "chevron" } else { "chevron-right" },
                theme.text_muted,
                14.,
            ))
            .child(format!("Keys inside dialogs and menus ({count})"))
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
            .flex_shrink()
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
            .when(adding, |d| d.bg(theme.selection_bg))
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .child(controls::glyph("plus", theme.text_muted, 14.))
            .tooltip({
                let font = self.font.clone();
                move |_, cx| crate::toolbar::tooltip("Add keys".into(), font.clone(), cx)
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
                .tooltip({
                    let font = self.font.clone();
                    move |_, cx| {
                        crate::toolbar::tooltip("Back to skwir's keys".into(), font.clone(), cx)
                    }
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    if let Some(action) = this.keys.keyboard.find(&id).cloned() {
                        this.save_keys(&Edit::Reset(&action), cx);
                    }
                }))
        });
        let mut texts = div()
            .flex_1()
            .min_w(px(160.))
            .flex()
            .items_baseline()
            .gap_2()
            .child(
                div()
                    .flex_shrink()
                    .min_w_0()
                    .truncate()
                    .child(action.title.clone()),
            );
        if let Some(shared) = shared {
            texts = texts.child(
                div()
                    .flex_shrink()
                    .min_w_0()
                    .truncate()
                    .text_xs()
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
                window.focus(&this.keys.focus);
                cx.notify();
            }))
            .child(texts)
            .children(reset)
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
            (Some(_), _) => "No longer runs it: your shortcut took these keys. Click to restore.",
            (None, Some(_)) => "Yours. Click to change or remove.",
            (None, None) => "Click to change or remove.",
        };
        // Its conditions whole, which its row may cut short.
        let tip: SharedString = if chip.conditions.is_empty() {
            said.into()
        } else {
            let mut chars = chip.conditions.chars();
            let first: String = chars
                .next()
                .into_iter()
                .flat_map(char::to_uppercase)
                .collect();
            format!("{first}{}. {said}", chars.as_str()).into()
        };
        let font = self.font.clone();
        let id = action.id();
        let target = chip.clone();
        div()
            .id(SharedString::from(format!("key-{index}-{n}")))
            .flex()
            .flex_shrink()
            .min_w_0()
            .items_center()
            .gap_1()
            .px_1()
            .py(px(2.))
            .rounded_md()
            .border_1()
            .border_color(if pointed {
                accent
            } else {
                gpui::transparent_black()
            })
            .when(edited, |d| d.bg(theme.selection_bg))
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .child(caps(&chip.keys, !chip.runs(), theme))
            .when(chip.record.is_some(), |d| {
                d.child(div().size(px(6.)).rounded_full().bg(accent))
            })
            .when(conditions && !chip.conditions.is_empty(), |d| {
                d.child(
                    div()
                        .flex_shrink()
                        .min_w(px(24.))
                        .max_w(px(150.))
                        .truncate()
                        .text_xs()
                        .text_color(theme.text_muted)
                        .child(chip.conditions.clone()),
                )
            })
            .tooltip(move |_, cx| crate::toolbar::tooltip(tip.clone(), font.clone(), cx))
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
    /// in a list: a search's results are few); how many.
    pub(crate) fn keys_found(
        &mut self,
        query: &str,
        items: &mut Vec<AnyElement>,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> usize {
        let rows = self.key_rows(query);
        let focused = self.keys.focus.contains_focused(window, cx);
        let mut found = 0;
        for (n, row) in rows.iter().enumerate() {
            match row {
                Row::Place(place) => items.push(
                    div()
                        .flex_none()
                        .pt_5()
                        .pb_2()
                        .text_xs()
                        .text_color(theme.text_muted)
                        .child(format!("Keyboard: {place}"))
                        .into_any_element(),
                ),
                Row::Inside { .. } => {}
                Row::Action(index) => {
                    found += 1;
                    items.push(
                        self.action_row(*index, edge(&rows, n), focused, theme, cx)
                            .flex_none()
                            .into_any_element(),
                    );
                }
            }
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
        self.keys.editor = Some(KeyEditor {
            recording: !matches!(target, Target::Restore(..)),
            target,
            strokes: Vec::new(),
            typed: None,
            error: None,
            focus: focus.clone(),
            intercept: None,
            subscriptions: Vec::new(),
        });
        window.focus(&focus);
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
        editor.strokes.push(keystroke.unparse());
        editor.recording = false;
        editor.intercept = None;
        editor.error = None;
        if editor.target == Target::Find {
            let keys = keymap::normal_keys(&editor.strokes.join(" "));
            self.keys.find = Some(keys);
            self.keys.editor = None;
            self.keys.list.scroll_to_item(0, ScrollStrategy::Top);
            window.focus(&self.keys.focus);
        } else {
            // Enter then saves: the form's button for a new shortcut, the
            // bar's for the rest.
            let save = if editor.target == Target::New {
                "new-save"
            } else {
                "keys-save"
            };
            let save = self.handle(save, cx);
            window.focus(&save);
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
        window.focus(&editor.focus.clone());
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
            let mut input = TextInput::new("ctrl-shift-n, or g h for one key after another", cx);
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
        input.update(cx, |input, _| input.focus(window));
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
            window.focus(&self.keys.focus);
        }
        cx.notify();
    }

    /// What the bar's keys would do, for its action.
    fn editor_verdict(&self, cx: &App) -> Option<(Verdict, String)> {
        let editor = self.keys.editor.as_ref()?;
        let keys = editor.keys(cx);
        if keys.is_empty() || skwir_core::keymap::valid_keys(&keys).is_err() {
            return None;
        }
        let (action, context, replacing) = match &editor.target {
            Target::Add(id) => {
                let action = self.keys.keyboard.find(id)?;
                (action.clone(), action.context(), None)
            }
            Target::Change(id, chip) => (
                self.keys.keyboard.find(id)?.clone(),
                chip.context.clone(),
                Some(chip.clone()),
            ),
            Target::New => {
                let (action, context) = self.new_shortcut_action(cx)?;
                (action, context, None)
            }
            Target::Restore(..) | Target::Find => return None,
        };
        let verdict = verdict(
            &self.app,
            &self.keys.keyboard,
            &self.keys.bound,
            &keys,
            &context,
            &action,
            replacing.as_ref(),
        );
        Some((verdict, keys))
    }

    /// Save what the bar holds, unless the keys cannot be: the reason
    /// stays in the bar.
    pub(crate) fn save_key_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.keys.editor.as_ref() else {
            return;
        };
        let target = editor.target.clone();
        if let Target::Restore(_, chip) = &target {
            if self.save_keys(&Edit::Restore(chip), cx) {
                self.close_key_editor(window, cx);
            }
            return;
        }
        let keys = editor.keys(cx);
        let verdict = self.editor_verdict(cx).map(|(verdict, _)| verdict);
        if verdict
            .as_ref()
            .is_some_and(|verdict| verdict.notes.contains(&Note::Has))
        {
            // Keys it has there already: nothing to keep.
            self.close_key_editor(window, cx);
            return;
        }
        let refused = if keys.is_empty() {
            Some("Press the keys first".to_owned())
        } else if let Err(error) = skwir_core::keymap::valid_keys(&keys) {
            Some(format!("{error:#}"))
        } else {
            verdict.and_then(|verdict| verdict.refused)
        };
        if let Some(error) = refused {
            if let Some(editor) = self.keys.editor.as_mut() {
                editor.error = Some(error);
            }
            cx.notify();
            return;
        }
        let saved = match &target {
            Target::Add(id) => match self.keys.keyboard.find(id).cloned() {
                Some(action) => self.save_keys(
                    &Edit::Add {
                        context: action.context(),
                        action: &action,
                        keys,
                    },
                    cx,
                ),
                None => false,
            },
            Target::Change(id, chip) => match self.keys.keyboard.find(id).cloned() {
                Some(action) => self.save_keys(
                    &Edit::Change {
                        action: &action,
                        chip,
                        keys,
                    },
                    cx,
                ),
                None => false,
            },
            Target::New => self.save_new_shortcut(&keys, cx),
            Target::Restore(..) | Target::Find => false,
        };
        if saved {
            if target == Target::New {
                self.keys.new = None;
            }
            self.close_key_editor(window, cx);
        } else if let Some(editor) = self.keys.editor.as_mut() {
            editor.error = self
                .errors
                .get(BINDINGS_KEY)
                .cloned()
                .or_else(|| Some("These keys cannot be kept".to_owned()));
            cx.notify();
        }
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
        if let Some(why) = removal_refused(&action, &chip) {
            if let Some(editor) = self.keys.editor.as_mut() {
                editor.error = Some(why);
            }
            cx.notify();
            return;
        }
        if self.save_keys(&Edit::Remove(&chip), cx) {
            self.close_key_editor(window, cx);
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
        let invalid = editor.error.is_some();
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
                .h(px(36.))
                .min_w(px(220.))
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
                        .child(if strokes.is_empty() {
                            if finding {
                                "Press the keys to find…"
                            } else {
                                "Press the keys…"
                            }
                        } else {
                            "then the next key…"
                        }),
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
                    "Press again",
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
                        "Then another key",
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
                    "Type them",
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
                    .text_xs()
                    .text_color(theme.text_muted)
                    .child("Esc is a key here too: Cancel is a click away."),
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
            .max_h(px(112.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_1();
        let line = |icon: &str, color: gpui::Hsla, text: String| {
            div()
                .flex()
                .items_start()
                .gap(px(6.))
                .text_xs()
                .text_color(color)
                .child(controls::glyph(icon, color, 13.))
                .child(div().flex_1().min_w_0().child(text))
        };
        if let Some((verdict, keys)) = self.editor_verdict(cx) {
            let keys = keymap::display_keys(&keys);
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
                let some = deeper[..3].join(", ");
                notes = notes.child(line(
                    "info",
                    theme.text_muted,
                    format!(
                        "{keys} still runs what it runs in {} places that bind it themselves ({some}…): a place's own keys come first.",
                        deeper.len()
                    ),
                ));
            }
            for note in &verdict.notes {
                let (icon, color, text) = match note {
                    Note::Replaces { title, place } => (
                        "warning",
                        theme.warning,
                        format!("{keys} runs “{title}” now ({place}): yours replaces it there."),
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
                        format!(
                            "In {place}, {keys} still runs “{title}”: a place's own keys come first."
                        ),
                    ),
                    Note::Stays {
                        title,
                        why: Stay::Elsewhere,
                        ..
                    } => (
                        "info",
                        theme.text_muted,
                        format!("Elsewhere, {keys} still runs “{title}”."),
                    ),
                    Note::Stays {
                        title,
                        place,
                        why: Stay::Besides,
                    } => (
                        "info",
                        theme.text_muted,
                        format!("{keys} still runs “{title}” in {place}."),
                    ),
                    Note::Waits { title } => (
                        "plugin",
                        theme.text_muted,
                        format!(
                            "Your “{title}” waits on {keys} for its plugin: once it is back, the later of the two runs."
                        ),
                    ),
                    Note::Typing => (
                        "info",
                        theme.text_muted,
                        "A letter bound in the file list stops typing a name there to jump to it; / then finds a name.".to_owned(),
                    ),
                    Note::Has => (
                        "check",
                        theme.text_muted,
                        format!("It has {keys} here already."),
                    ),
                };
                notes = notes.child(line(icon, color, text));
            }
        }
        if let Some(error) = self.keys.editor.as_ref().and_then(|e| e.error.clone()) {
            notes = notes.child(line("warning", theme.error, error));
        }
        notes
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
            (Target::Find, _) => "Find the actions of a key".to_owned(),
            (Target::Add(_), Some(action)) => {
                format!("New keys for “{}” · {}", action.title, action.place)
            }
            (Target::Change(_, chip), Some(action)) => format!(
                "{} for “{}” · {}",
                keymap::display_keys(&chip.keys),
                action.title,
                action.place
            ),
            (Target::Restore(_, chip), Some(action)) => format!(
                "{} no longer runs “{}” · {}",
                keymap::display_keys(&chip.keys),
                action.title,
                action.place
            ),
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
        if let Target::Restore(_, chip) = &target {
            let by = chip
                .hidden_by
                .as_ref()
                .and_then(|id| {
                    self.keys
                        .keyboard
                        .actions
                        .iter()
                        .find(|a| a.chips.iter().any(|c| c.record.as_ref() == Some(id)))
                })
                .map(|a| a.title.clone());
            bar = bar.child(
                div()
                    .text_xs()
                    .text_color(theme.text_muted)
                    .child(match by {
                        Some(title) => format!(
                            "Your shortcut for “{title}” took these keys. Restoring removes it."
                        ),
                        None => "You freed these keys. Restoring gives them back.".to_owned(),
                    }),
            );
        } else {
            bar = bar.child(self.key_recorder(theme, window, cx));
            bar = bar.child(self.key_notes(theme, cx));
        }
        let cancel = self.handle("keys-cancel", cx);
        self.stop(&cancel, None, None);
        let mut buttons = div().flex().items_center().gap_2();
        if let Target::Change(_, chip) = &target {
            let remove = self.handle("keys-remove", cx);
            self.stop(&remove, None, None);
            let what = if chip.record.is_some() {
                "Remove these keys"
            } else {
                "Free these keys"
            };
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
                "Cancel",
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
                    if matches!(target, Target::Restore(..)) {
                        "Restore"
                    } else {
                        "Save"
                    },
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
    /// the key the cursor is on or add one, remove it.
    pub(crate) fn keys_key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let rows = self.key_rows("");
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
            self.keys.list.scroll_to_item(n, ScrollStrategy::Center);
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
                    if let Some(why) = removal_refused(action, chip) {
                        self.errors.insert(BINDINGS_KEY.to_owned(), why);
                    } else {
                        self.save_keys(&Edit::Remove(chip), cx);
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
                    .text_xs()
                    .text_color(self.theme.text_muted)
                    .occlude()
                    .child(place),
            )
            .into_any_element()
    }
}
