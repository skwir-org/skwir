//! What a field takes, said where it is typed (ADR-137): one component,
//! `FieldHelp`, for every field that says it, whichever view holds the
//! field (the shell's dialogs, Settings).
//!
//! - The owner of the field's language says it as data
//!   (`skwir_core::reference::Reference`): the bulk rename's plugin by
//!   its command (`rename_bulk.reference`, asked with each preview, so
//!   what each placeholder gives is the first item's as the dialog
//!   stands), a question by `ui.ask`'s `reference` (Access, Run Command),
//!   core for an exclusion's fields.
//! - **A `?` inside the field's frame at its end** (`question`), as a
//!   form's list button (`inner_chevron`) and the address's own buttons
//!   are; the address's too. A plain `?`, as `?` alone asks the address
//!   what it takes; on the term's ground while the field's whole list
//!   shows. It acts on the press and gives the keyboard to its field, so
//!   the caret stays where a row will write; no Tab stop.
//! - **One list under the field, as the address's card**: everything the
//!   field takes, under its groups' titles with their notes, once the `?`
//!   is pressed or F1 (`ui.field_reference`); else, as it is typed, the
//!   items going on with a part begun by one of the reference's `starts`
//!   (`Reference::offered`). The keyboard stays in the field, `lists` in
//!   its key context while the list shows: Up and Down move the
//!   highlight (`ui.field_complete`); none is lit in everything shown
//!   before they move, the first of what goes on with a part typed. While
//!   one is lit (`offers`) Tab takes it, else Tab goes on to the next
//!   control; Enter takes only one the arrows chose (the field's Enter
//!   else answers its dialog), Escape puts the list away for that text
//!   and caret before it closes anything. A press on a row takes it. A
//!   row taken writes its part where the caret is (`TextInput::put`), in
//!   place of the part typed, or fills the fields its recipe names
//!   (`replace_all_together`): an edit Ctrl+Z takes back, in every field
//!   it filled from any of them. F1 again shows the keys
//!   of the place. A screen reader hears a combobox (`modal::options`).
//! - The field's frame answers these commands itself (`with_help`): they
//!   come from the field, so the view holding it (`Helps`) is the one
//!   asked, the shell's or another's, which finds the help by the field's
//!   input, never by a name another dialog's field may share.
//! - The address field says its own in its card (`Typed::Help`, F1 or
//!   its `?`): taking the keyboard from the field would end what is typed.

use crate::sides::Sides;
use std::ops::Range;
use std::rc::Rc;

use gpui::{
    AnyElement, App, Context, Div, Entity, EntityId, FocusHandle, Focusable as _, KeyContext,
    ScrollHandle, SharedString, Window, div, prelude::*, px,
};
use skwir_core::reference::Reference;
use skwir_core::words;

use crate::elements::popover::PopoverAnchor;
use crate::input::TextInput;
use crate::keymap::RunCommand;
use crate::sizes::{TextSizes, sized};
use crate::theme::{Chosen, Theme};
use crate::toolbar;

/// The list is drawn over the dialogs (theirs are 3 and 4).
const PRIORITY: usize = 6;

/// How tall the list is at most, at 100 %: it scrolls beyond.
const LIST_MOST: f32 = 340.;

/// How wide the list is, at 100 %, where the window leaves room.
const LIST_WIDTH: f32 = 560.;

/// What a field says it takes, and its list's state: each for the field's
/// text and caret as they were, and so gone as either moves. A clone is
/// the same help (what a view draws from while it holds the original).
#[derive(Clone)]
pub(crate) struct FieldHelp {
    /// The name the view holding the field knows it by among its own, a
    /// recipe too (`fills`). Never what finds the help: two dialogs open
    /// at once may name a field alike (`Helps` finds it by its field).
    pub key: SharedString,
    /// What it takes, once its owner said it.
    pub reference: Option<Rc<Reference>>,
    /// The field's frame, which the list hangs under.
    anchor: PopoverAnchor,
    scroll: ScrollHandle,
    /// Everything it takes shown (F1, its `?`).
    asked: Option<Typed>,
    /// The list put away (Escape, its `?` again).
    away: Option<Typed>,
    /// The row the arrows lit.
    moved: Option<(Typed, usize)>,
}

/// A field's text and caret.
type Typed = (String, usize);

/// What the list shows now.
struct Listed {
    /// Its rows: items by their group's place and their own.
    rows: Vec<(usize, usize)>,
    /// The part typed a row takes the place of; `None`: everything shown,
    /// a row written at the caret.
    part: Option<Range<usize>>,
    /// The row lit: moved there by the arrows (`chosen`), else the first
    /// of what goes on with the part typed; none in everything shown
    /// before the arrows move, where Tab goes on to the next control.
    lit: Option<usize>,
    chosen: bool,
}

impl Listed {
    fn whole(&self) -> bool {
        self.part.is_none()
    }
}

/// A child of the list: a group's title, a row, a note.
enum Child {
    Title(usize),
    Row(usize),
    Note(usize, usize),
}

/// The view that holds fields that say what they take: what their frames'
/// commands and keys reach. A field is known by its input (`EntityId`),
/// which no other dialog's field shares, whatever its name.
pub(crate) trait Helps: Sized + 'static {
    /// The help of the field `field`, and the field.
    fn help_of(&mut self, field: EntityId) -> Option<(&mut FieldHelp, Entity<TextInput>)>;
    /// The field a recipe of `field`'s names by `key`: one of its own
    /// dialog's.
    fn field_of(&self, field: EntityId, key: &str) -> Option<Entity<TextInput>>;
}

fn typed(input: &Entity<TextInput>, cx: &App) -> Typed {
    let input = input.read(cx);
    (input.text().to_owned(), input.caret())
}

impl FieldHelp {
    pub fn new(key: impl Into<SharedString>, reference: Option<Rc<Reference>>) -> Self {
        FieldHelp {
            key: key.into(),
            reference,
            anchor: PopoverAnchor::default(),
            scroll: ScrollHandle::new(),
            asked: None,
            away: None,
            moved: None,
        }
    }

    /// What the list shows for `input`, while it has the keyboard.
    fn listed(&self, input: &Entity<TextInput>, focused: bool, cx: &App) -> Option<Listed> {
        if !focused {
            return None;
        }
        self.listed_at(&typed(input, cx))
    }

    /// What the list shows for the field's text and caret, `now`.
    fn listed_at(&self, now: &Typed) -> Option<Listed> {
        let reference = self.reference.as_ref()?;
        if self.away.as_ref() == Some(now) {
            return None;
        }
        let (rows, part) = if self.asked.as_ref() == Some(now) {
            let rows: Vec<(usize, usize)> = reference.items().map(|(g, i, _)| (g, i)).collect();
            (rows, None)
        } else {
            let offered = reference.offered(&now.0, now.1)?;
            (offered.items, Some(offered.range))
        };
        if rows.is_empty() {
            return None;
        }
        let (lit, chosen) = match &self.moved {
            Some((at, lit)) if at == now => (Some(*lit % rows.len()), true),
            _ => (part.is_some().then_some(0), false),
        };
        Some(Listed {
            rows,
            part,
            lit,
            chosen,
        })
    }

    /// Whether everything it takes shows for `input`.
    fn whole_shown(&self, input: &Entity<TextInput>, focused: bool, cx: &App) -> bool {
        self.listed(input, focused, cx).is_some_and(|l| l.whole())
    }

    /// The list's children, in the order drawn.
    fn children(&self, listed: &Listed) -> Vec<Child> {
        let Some(reference) = self.reference.as_ref() else {
            return Vec::new();
        };
        if !listed.whole() {
            return (0..listed.rows.len()).map(Child::Row).collect();
        }
        let mut children = Vec::new();
        let mut row = 0;
        for (g, group) in reference.groups.iter().enumerate() {
            children.push(Child::Title(g));
            for _ in &group.items {
                children.push(Child::Row(row));
                row += 1;
            }
            children.extend((0..group.notes.len()).map(|n| Child::Note(g, n)));
        }
        children
    }

    /// Bring the row lit into view.
    fn reveal(&self, listed: &Listed) {
        let Some(lit) = listed.lit else {
            return;
        };
        if let Some(at) = self
            .children(listed)
            .iter()
            .position(|child| matches!(child, Child::Row(row) if *row == lit))
        {
            self.scroll.scroll_to_item(at);
        }
    }
}

/// The `?` inside a field's frame at its end: what it takes, as F1 says
/// it. `open` while that shows; `tip` says it, with its keys, to a screen
/// reader too while one listens (`a11y`). The press is `pressed`'s, the
/// keyboard never going to the `?` itself.
pub(crate) fn question<V: 'static>(
    id: impl Into<SharedString>,
    open: bool,
    tip: SharedString,
    pressed: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static,
    a11y: bool,
    cx: &Context<V>,
) -> AnyElement {
    let theme = cx.global::<Theme>().clone();
    let hover = theme.hover;
    let (muted, text) = (theme.text_muted, theme.text);
    let shown = tip.clone();
    div()
        .id(id.into())
        .flex_none()
        // Within the field's frame, as tall as its text allows, as a form's
        // list button (`inner_chevron`).
        .size(sized(crate::controls::HEIGHT - 8.))
        .margin_end(px(-4.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .cursor_pointer()
        .font_family(theme.font.clone())
        .text_body()
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .when(open, |d| d.bg(theme.term).text_color(theme.accent_text))
        .when(!open, move |d| {
            d.text_color(muted)
                .hover(move |s| s.bg(hover).text_color(text))
        })
        .when(a11y, |d| {
            d.role(gpui::Role::Button)
                .aria_label(tip)
                .aria_expanded(open)
        })
        .tooltip(move |_, cx| toolbar::tooltip(shown.clone(), cx))
        .on_mouse_down(
            gpui::MouseButton::Left,
            cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                window.prevent_default();
                pressed(this, window, cx);
            }),
        )
        .child("?")
        .into_any_element()
}

/// What a field takes, as its `?` and F1 say it: with its keys.
pub(crate) fn tip(keys: Option<String>) -> SharedString {
    match keys {
        Some(keys) => words!(
            reference_tip_keys = "What it takes · { $keys }",
            keys = skwir_core::words::Keys(keys),
            note = "The tooltip of a field's ? button, which lists what the field takes; keys: its shortcut."
        )
        .say()
        .into(),
        None => words!(
            reference_tip = "What it takes",
            note = "The tooltip of a field's ? button, which lists what the field takes."
        )
        .say()
        .into(),
    }
}

/// `frame`, the frame of the field `input` that `help` says, with what
/// makes it say it: `reference` in its key context (`lists` while the list
/// shows, where the arrows walk it, and `offers` while a row is lit, which
/// Tab takes), its `?` after what it holds, the list under it, and the
/// answers to the commands its keys run. `keys` are F1's, for the `?`'s
/// tooltip.
pub(crate) fn with_help<V: Helps>(
    frame: Div,
    help: &FieldHelp,
    input: &Entity<TextInput>,
    keys: Option<String>,
    window: &Window,
    cx: &Context<V>,
) -> Div {
    let focused = input.read(cx).is_focused(window);
    let listed = help.listed(input, focused, cx);
    let mut context = KeyContext::default();
    context.add("reference");
    if let Some(listed) = &listed {
        context.add("lists");
        if listed.lit.is_some() {
            context.add("offers");
        }
    }
    let whole = listed.as_ref().is_some_and(Listed::whole);
    let id = input.entity_id();
    let a11y = window.is_a11y_active();
    let field = input.read(cx).focus_handle(cx);
    let list = listed.map(|listed| list(help, &listed, id, &field, a11y, cx));
    frame
        .key_context(context)
        .relative()
        .on_action(
            cx.listener(move |this: &mut V, action: &RunCommand, window, cx| {
                match action.name.as_str() {
                    "ui.field_reference" => ask(this, id, true, window, cx),
                    "ui.field_complete" => complete(this, id, &action.args, window, cx),
                    _ => cx.propagate(),
                }
            }),
        )
        .child(question(
            format!("help-{}-{}", help.key, id.as_u64()),
            whole,
            tip(keys),
            move |this: &mut V, window, cx| ask(this, id, false, window, cx),
            a11y,
            cx,
        ))
        .child(help.anchor.marker())
        .children(list)
}

/// F1 in the field (`keys`), or its `?`: everything it takes, or, shown,
/// put away (the `?`) or the keys of the place (F1).
fn ask<V: Helps>(
    this: &mut V,
    field: EntityId,
    keys: bool,
    window: &mut Window,
    cx: &mut Context<V>,
) {
    let Some((help, input)) = this.help_of(field) else {
        return;
    };
    let focus = input.read(cx).focus_handle(cx);
    let focused = focus.is_focused(window);
    let now = typed(&input, cx);
    if help.whole_shown(&input, focused, cx) {
        if keys {
            window.dispatch_action(
                Box::new(RunCommand {
                    name: "ui.shortcuts".into(),
                    args: serde_json::json!({}),
                }),
                cx,
            );
        } else {
            help.asked = None;
            help.away = Some(now);
        }
    } else {
        help.asked = Some(now);
        help.away = None;
        help.moved = None;
        help.scroll.scroll_to_item(0);
        window.focus(&focus, cx);
    }
    cx.notify();
}

/// `ui.field_complete { step?, group?, item? }`: move the highlight by
/// `step`, or take the item named, or the row lit.
fn complete<V: Helps>(
    this: &mut V,
    field: EntityId,
    args: &serde_json::Value,
    window: &mut Window,
    cx: &mut Context<V>,
) {
    if let Some(step) = args.get("step").and_then(serde_json::Value::as_i64) {
        let Some((help, input)) = this.help_of(field) else {
            return;
        };
        let focused = input.read(cx).is_focused(window);
        let Some(listed) = help.listed(&input, focused, cx) else {
            return;
        };
        let lit = stepped(listed.lit, listed.rows.len(), step);
        help.moved = Some((typed(&input, cx), lit));
        help.reveal(&Listed {
            lit: Some(lit),
            chosen: true,
            ..listed
        });
        cx.notify();
        return;
    }
    let place = |name: &str| {
        args.get(name)
            .and_then(serde_json::Value::as_u64)
            .and_then(|n| usize::try_from(n).ok())
    };
    take(this, field, place("group").zip(place("item")), window, cx);
}

/// The row lit once the arrows moved `step` from `lit` among `count`
/// rows, round the ends; none lit yet, Down lights the first, Up the last.
fn stepped(lit: Option<usize>, count: usize, step: i64) -> usize {
    let count = i64::try_from(count.max(1)).unwrap_or(1);
    let from = lit.map_or(if step > 0 { -1 } else { count }, |lit| {
        i64::try_from(lit).unwrap_or(0)
    });
    usize::try_from((from + step).rem_euclid(count)).unwrap_or(0)
}

/// The field's Enter: the row the arrows chose is taken, true; else it
/// answers its dialog.
pub(crate) fn on_submit<V: Helps>(
    this: &mut V,
    field: EntityId,
    window: &mut Window,
    cx: &mut Context<V>,
) -> bool {
    let chosen = this.help_of(field).is_some_and(|(help, input)| {
        let focused = input.read(cx).is_focused(window);
        help.listed(&input, focused, cx).is_some_and(|l| l.chosen)
    });
    if chosen {
        take(this, field, None, window, cx);
    }
    chosen
}

/// The field's Escape: the list is put away, true; else it closes its
/// dialog.
pub(crate) fn on_cancel<V: Helps>(
    this: &mut V,
    field: EntityId,
    window: &mut Window,
    cx: &mut Context<V>,
) -> bool {
    let Some((help, input)) = this.help_of(field) else {
        return false;
    };
    let focused = input.read(cx).is_focused(window);
    if help.listed(&input, focused, cx).is_none() {
        return false;
    }
    help.asked = None;
    help.away = Some(typed(&input, cx));
    cx.notify();
    true
}

/// Take `item` (else the row lit): its part where the caret is, in place
/// of the part typed, or the fields its recipe names filled.
fn take<V: Helps>(
    this: &mut V,
    field: EntityId,
    item: Option<(usize, usize)>,
    window: &mut Window,
    cx: &mut Context<V>,
) {
    let Some((help, input)) = this.help_of(field) else {
        return;
    };
    let focused = input.read(cx).is_focused(window);
    let Some(listed) = help.listed(&input, focused, cx) else {
        return;
    };
    let Some(reference) = help.reference.clone() else {
        return;
    };
    let Some((group, at)) = item.or_else(|| listed.lit.map(|lit| listed.rows[lit])) else {
        return;
    };
    let Some(item) = reference.item(group, at).cloned() else {
        return;
    };
    help.asked = None;
    help.moved = None;
    let focus = input.read(cx).focus_handle(cx);
    window.focus(&focus, cx);
    if let Some(part) = &item.inserts {
        input.update(cx, |input, cx| {
            if let Some(range) = listed.part.clone() {
                input.select_range(range, cx);
            }
            input.put(part, cx);
        });
    }
    // The fields a recipe names, filled as one edit: Ctrl+Z in any takes
    // it back in all.
    let filled: Vec<(Entity<TextInput>, &str)> = item
        .fills
        .iter()
        .filter_map(|(named, text)| Some((this.field_of(field, named)?, text.as_str())))
        .collect();
    TextInput::replace_all_together(&filled, cx);
    cx.notify();
}

/// The list under the field: each row as written, what it means, what it
/// gives; everything under its groups' titles with their notes, or what
/// goes on with the part typed. The field (`field`) keeps the keyboard; a
/// press on a row takes it. A screen reader hears the web's combobox: a
/// list box beside the field, the row lit its active descendant
/// (`modal::options`, `modal::option`), each row named by what it shows.
fn list<V: Helps>(
    help: &FieldHelp,
    listed: &Listed,
    id: EntityId,
    field: &FocusHandle,
    a11y: bool,
    cx: &Context<V>,
) -> AnyElement {
    let theme = cx.global::<Theme>().clone();
    let reference = help.reference.clone().unwrap_or_default();
    let mut rows = crate::modal::options(
        crate::elements::known_by("field-help-rows", id),
        reference.title.say(),
        listed.rows.len(),
        a11y,
    )
    .flex()
    .flex_col()
    .min_h_0()
    .max_h(sized(LIST_MOST))
    .overflow_y_scroll()
    .track_scroll(&help.scroll)
    .p_1();
    for child in help.children(listed) {
        let element = match child {
            Child::Title(g) => div()
                .flex_none()
                .px_2()
                .pt_2()
                .pb_1()
                .text_small()
                .text_color(theme.text_muted)
                .child(reference.groups[g].title.say())
                .into_any_element(),
            Child::Note(g, n) => div()
                .flex_none()
                .px_2()
                .pb_1()
                .text_small()
                .text_color(theme.text_muted)
                .whitespace_normal()
                .child(reference.groups[g].notes[n].say())
                .into_any_element(),
            Child::Row(row) => {
                let (group, at) = listed.rows[row];
                let Some(item) = reference.item(group, at) else {
                    continue;
                };
                let lit = listed.lit == Some(row);
                let hover = theme.hover;
                let named = a11y.then(|| {
                    let named = match &item.gives {
                        Some(gives) => words!(
                            reference_item_gives = "{ $written }, { $says }, { $gives }",
                            written = item.written.as_str(),
                            says = item.says.clone(),
                            gives = gives.as_str(),
                            note = "What a screen reader calls an item of a field's list of what it takes; written: the item as it is written (a pattern); says: what it means; gives: what it gives for the first file."
                        ),
                        None => words!(
                            reference_item = "{ $written }, { $says }",
                            written = item.written.as_str(),
                            says = item.says.clone(),
                            note = "What a screen reader calls an item of a field's list of what it takes; written: the item as it is written (a pattern); says: what it means."
                        ),
                    };
                    SharedString::from(named.say())
                });
                crate::modal::option(
                    div().id(crate::elements::known_by(
                        "field-help-row",
                        (id, group, &item.written),
                    )),
                    named,
                    row,
                    lit,
                    field,
                )
                .flex_none()
                .h(sized(toolbar::MENU_ROW))
                .px_2()
                .flex()
                .items_center()
                .gap_2()
                .rounded_md()
                .cursor_pointer()
                .when(lit, |d| d.chosen(&theme))
                .when(!lit, move |d| d.hover(move |s| s.bg(hover)))
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        window.prevent_default();
                        take(this, id, Some((group, at)), window, cx);
                    }),
                )
                .child(
                    // One column for what is written; cut in its
                    // middle, as an end cut takes the signs before it
                    // away (gpui trims punctuation there).
                    div()
                        .flex_none()
                        .w(gpui::relative(0.3))
                        .overflow_hidden()
                        .text_ellipsis_middle()
                        .font_family(theme.code_font.clone())
                        .child(item.written.clone()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(theme.text_muted)
                        .child(item.says.say()),
                )
                .children(item.gives.clone().map(|gives| {
                    div()
                        .flex_none()
                        .max_w(gpui::relative(0.3))
                        .pad_start(crate::sides::step(2.))
                        .truncate()
                        .text_small()
                        .font_family(theme.code_font.clone())
                        .text_color(theme.text_muted)
                        .child(gives)
                }))
                .into_any_element()
            }
        };
        rows = rows.child(element);
    }
    // As the address's card: on the window's ground, framed, over what is
    // under it; a press here is the list's, never the dialog's behind it.
    let panel = div()
        .w_full()
        .flex()
        .flex_col()
        .rounded_lg()
        .border_1()
        .border_color(theme.border)
        .bg(theme.bg)
        .text_color(theme.text)
        .shadow_md()
        .occlude()
        .text_body()
        .whitespace_nowrap()
        .on_mouse_down(gpui::MouseButton::Left, |_, window, cx| {
            cx.stop_propagation();
            window.prevent_default();
        })
        .child(
            div()
                .relative()
                .flex()
                .flex_col()
                .min_h_0()
                .child(rows)
                .child(crate::elements::scrollbar::vertical(
                    crate::elements::known_by("field-help-scrollbar", id),
                    crate::elements::scrollbar::Scrolls::Offset(help.scroll.clone()),
                )),
        );
    // Under the field, never wider than the window (`popup_under`).
    let width = theme
        .sizes
        .scale(crate::sizes::Part::Interface)
        .px(LIST_WIDTH);
    help.anchor
        .popup_under(PRIORITY, width, panel.into_any_element())
}

#[cfg(test)]
mod tests {
    use skwir_core::words::Words;
    use std::rc::Rc;

    use skwir_core::reference::{Group, Item, Reference};

    use super::{FieldHelp, stepped};

    fn help() -> FieldHelp {
        let mut said = Reference::new(Words::as_written("Template")).group(
            Group::new(Words::as_written("Parts of the name"))
                .item(Item::new("{name}", Words::as_written("The name")))
                .item(Item::new("{n}", Words::as_written("A number")))
                .item(Item::new(
                    "{n:03}",
                    Words::as_written("A number, three digits"),
                )),
        );
        said.starts = vec!["{".to_owned()];
        FieldHelp::new("template", Some(Rc::new(said)))
    }

    /// In everything a field takes no row is lit before the arrows move,
    /// so Tab goes on to the next control and Enter answers the dialog;
    /// what goes on with a part typed lights its first row, which Tab
    /// takes (ADR-137).
    #[test]
    fn a_row_is_lit_only_where_it_is_offered_or_chosen() {
        let mut help = help();
        let now = ("{full}".to_owned(), 6);
        help.asked = Some(now.clone());
        let whole = help.listed_at(&now).unwrap();
        assert!(whole.whole());
        assert_eq!((whole.lit, whole.chosen), (None, false));
        assert_eq!(whole.rows.len(), 3);
        // The arrows light a row: Tab and Enter take it.
        help.moved = Some((now.clone(), stepped(whole.lit, whole.rows.len(), 1)));
        let moved = help.listed_at(&now).unwrap();
        assert_eq!((moved.lit, moved.chosen), (Some(0), true));
        // A part begun: what goes on with it, the first lit for Tab, not
        // chosen for Enter.
        let typed = ("a_{n".to_owned(), 4);
        let offered = help.listed_at(&typed).unwrap();
        assert!(!offered.whole());
        assert_eq!(offered.part, Some(2..4));
        assert_eq!((offered.lit, offered.chosen), (Some(0), false));
        // Put away: nothing for that text and caret.
        help.away = Some(typed.clone());
        assert!(help.listed_at(&typed).is_none());
    }

    /// Down from nothing lit lights the first row, Up the last, and both
    /// go round the ends.
    #[test]
    fn the_arrows_go_round_the_rows() {
        assert_eq!(stepped(None, 3, 1), 0);
        assert_eq!(stepped(None, 3, -1), 2);
        assert_eq!(stepped(Some(2), 3, 1), 0);
        assert_eq!(stepped(Some(0), 3, -1), 2);
        assert_eq!(stepped(Some(1), 3, 1), 2);
    }
}
