//! What a dialog's field takes, said where it is typed (ADR-137): its
//! reference, opened from the `?` inside the field at its end or F1 in it,
//! hanging over the dialog from the field; and, as it is typed, what goes
//! on with a part begun there, listed under the field.
//!
//! - The owner of the field's language says it as data
//!   (`skwir_core::reference::Reference`): the bulk rename's plugin by
//!   its command (`rename_bulk.reference`, asked with each preview, so
//!   what each placeholder gives is the first item's as the dialog
//!   stands), a question by `ui.ask`'s `reference`. The window draws it
//!   as a menu (`toolbar::MenuPart`): each group under its title, each
//!   item a row (`Button::said`): as written, what it means, what it
//!   gives here; the group's notes under it.
//! - The `?` is one element for every field (`inner`), the address's
//!   too: inside the field's frame at its end, as a form's list button is
//!   (`inner_chevron`), lit while what it opens is open. It acts on the
//!   press and leaves the keyboard in the field (the caret where a row
//!   will write, the address's typing going on); no Tab stop, F1 its key.
//! - A row chosen (a click, Enter, Space) writes its part where the
//!   field's caret is, over what is selected there (`TextInput::put`),
//!   or fills the fields a recipe names (`replace_all`), each an edit
//!   Ctrl+Z takes back; the reference closes and the field has the
//!   keyboard again.
//! - It is a popup as any (`Shell::toggle_popup`): one at a time, its own
//!   keys (the arrows, Home, End, Escape, which closes it alone, not the
//!   dialog), closed by a press outside. Drawn among the window's
//!   overlays, never inside its dialog, so the dialog's keys (its Tab)
//!   stay the dialog's; F1 there shows the keys of the place, as
//!   anywhere.
//! - **What goes on with what is typed** (`Reference::offered`): a part
//!   begun by one of the reference's `starts` (`{` in a template) lists,
//!   under the field, the items going on with it. The keyboard stays in
//!   the field (`offers` in its key context): Up and Down move the
//!   highlight, Tab takes it (the first, unmoved), Enter takes it only
//!   once the arrows chose it, else answers the dialog; Escape puts the
//!   list away until the text or the caret moves. A press on a row takes
//!   it. The item takes the place of the part typed, as an edit.
//! - The address field says its own in its card (`Typed::Help`, F1 or
//!   its `?`): taking the keyboard from the field would end what is typed.

use std::rc::Rc;

use gpui::{
    AnyElement, Context, Entity, EntityId, Focusable as _, SharedString, Window, div, prelude::*,
    px,
};
use skwir_core::reference::{Offered, Reference};

use crate::elements::popover::PopoverAnchor;
use crate::input::TextInput;
use crate::shell::Shell;
use crate::sizes::{TextSizes, sized};
use crate::theme::{Chosen, Theme};
use crate::toolbar::{self, Button, MenuPart};

/// The popup's kind, as `Shell::toggle_popup` names it.
pub(crate) const POPUP: &str = "reference";

/// Drawn over the dialogs (theirs are 3 and 4).
const PRIORITY: usize = 6;

/// A field's reference, open or on offer: what it writes in, and what it
/// says.
pub(crate) struct Open {
    /// The fields it may write in, by the key a recipe names them by; the
    /// first is the one it is for.
    pub fields: Vec<(SharedString, Entity<TextInput>)>,
    pub reference: Rc<Reference>,
    /// The field's frame: the reference hangs from its end, what goes on
    /// with the text under it.
    pub anchor: PopoverAnchor,
}

/// The highlight among what a field offers, and what Escape put away:
/// each for a field, its text and its caret as they were, and so gone as
/// either moves.
#[derive(Default)]
pub(crate) struct Offers {
    moved: Option<(Typed, usize)>,
    away: Option<Typed>,
}

/// A field, its text and its caret.
type Typed = (EntityId, String, usize);

/// What the field offers now, and the row lit: moved there by the arrows
/// (`chosen`), else the first.
struct Shown {
    open: Open,
    offered: Offered,
    lit: usize,
    chosen: bool,
}

/// A field's `?`, inside its frame at its end: what it takes, opened by a
/// press as F1 opens it; `field` is the key its dialog knows it by. The
/// press leaves the keyboard in the field; lit while `open`.
pub(crate) fn inner(
    id: impl Into<SharedString>,
    field: &str,
    open: bool,
    shell: &Shell,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let theme = cx.global::<Theme>().clone();
    let title: SharedString = match shell.keys_for("ui.field_reference", &serde_json::Value::Null) {
        Some(keys) => format!("What it takes · {keys}").into(),
        None => "What it takes".into(),
    };
    let tip = title.clone();
    let field = field.to_owned();
    let hover = theme.hover;
    div()
        .id(id.into())
        .flex_none()
        // Within the field's frame, as tall as its text allows, as a form's
        // list button (`inner_chevron`).
        .size(sized(crate::controls::HEIGHT - 8.))
        .mr(px(-4.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .cursor_pointer()
        .when(open, |d| d.bg(theme.term))
        .when(!open, move |d| d.hover(move |s| s.bg(hover)))
        .when(shell.a11y, |d| {
            d.role(gpui::Role::Button)
                .aria_label(title)
                .aria_expanded(open)
        })
        .tooltip(move |_, cx| toolbar::tooltip(tip.clone(), cx))
        // The field keeps the keyboard: it acts on the press.
        .on_mouse_down(
            gpui::MouseButton::Left,
            cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                window.prevent_default();
                this.run(
                    "ui.field_reference",
                    serde_json::json!({ "field": field }),
                    window,
                    cx,
                );
            }),
        )
        .child(crate::controls::glyph(
            "help",
            if open {
                theme.accent_text
            } else {
                theme.text_muted
            },
            15.,
        ))
        .into_any_element()
}

impl Shell {
    /// Whether the reference open is the one of the field framed by
    /// `anchor`.
    pub(crate) fn reference_open_from(&self, anchor: &PopoverAnchor) -> bool {
        self.toolbar_menu
            .as_ref()
            .is_some_and(|popup| popup.kind == POPUP)
            && self
                .field_reference
                .as_ref()
                .is_some_and(|open| open.anchor.same(anchor))
    }

    /// The reference of the field being typed in, a question's or a
    /// dialog's.
    fn focused_reference(&self, window: &Window, cx: &gpui::App) -> Option<Open> {
        self.prompt_reference(None, window, cx)
            .or_else(|| self.bulk_reference(None, window, cx))
    }

    /// What the field being typed in offers now: nothing while a popup is
    /// open, or once Escape put it away for this text and caret.
    fn offers_now(&self, window: &Window, cx: &gpui::App) -> Option<Shown> {
        if self.toolbar_menu.is_some() {
            return None;
        }
        let open = self.focused_reference(window, cx)?;
        let typed = typed(&open, cx)?;
        if self.offers.away.as_ref() == Some(&typed) {
            return None;
        }
        let offered = open.reference.offered(&typed.1, typed.2)?;
        let (lit, chosen) = match &self.offers.moved {
            Some((at, lit)) if *at == typed => (*lit % offered.items.len(), true),
            _ => (0, false),
        };
        Some(Shown {
            open,
            offered,
            lit,
            chosen,
        })
    }

    /// Whether `input` offers what goes on with its text now: its key
    /// context then holds `offers`.
    pub(crate) fn offers_for(
        &self,
        input: &Entity<TextInput>,
        window: &Window,
        cx: &gpui::App,
    ) -> bool {
        self.offers_now(window, cx)
            .and_then(|shown| shown.open.fields.first().map(|(_, f)| f.entity_id()))
            == Some(input.entity_id())
    }

    /// `ui.field_complete { step?, group?, item? }`: among what the field
    /// offers, move the highlight by `step`, or take the item named, or
    /// the one lit.
    pub(crate) fn field_complete(
        &mut self,
        args: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(shown) = self.offers_now(window, cx) else {
            return;
        };
        let Some(typed) = typed(&shown.open, cx) else {
            return;
        };
        if let Some(step) = args.get("step").and_then(serde_json::Value::as_i64) {
            // From the row lit, the first until the arrows moved it: it is
            // drawn so.
            let count = i64::try_from(shown.offered.items.len()).unwrap_or(1);
            let from = i64::try_from(shown.lit).unwrap_or(0);
            let lit = usize::try_from((from + step).rem_euclid(count)).unwrap_or(0);
            self.offers.moved = Some((typed, lit));
            cx.notify();
            return;
        }
        let place = |key: &str| {
            args.get(key)
                .and_then(serde_json::Value::as_u64)
                .and_then(|n| usize::try_from(n).ok())
        };
        let item = match (place("group"), place("item")) {
            (Some(group), Some(item)) => (group, item),
            _ => shown.offered.items[shown.lit],
        };
        self.take_offer(&shown, item, window, cx);
    }

    /// The field's Enter: the item the arrows chose is taken, true; else
    /// it answers its dialog.
    pub(crate) fn offer_on_submit(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        match self.offers_now(window, cx) {
            Some(shown) if shown.chosen => {
                let item = shown.offered.items[shown.lit];
                self.take_offer(&shown, item, window, cx);
                true
            }
            _ => false,
        }
    }

    /// The field's Escape: what it offers is put away, true; else it
    /// closes its dialog.
    pub(crate) fn offers_away(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(shown) = self.offers_now(window, cx) else {
            return false;
        };
        self.offers.away = typed(&shown.open, cx);
        cx.notify();
        true
    }

    /// Put `item` in the place of the part typed, as an edit.
    fn take_offer(
        &mut self,
        shown: &Shown,
        (group, at): (usize, usize),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(part) = shown
            .open
            .reference
            .item(group, at)
            .and_then(|item| item.inserts.clone())
        else {
            return;
        };
        let Some((_, field)) = shown.open.fields.first() else {
            return;
        };
        let focus = field.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        let range = shown.offered.range.clone();
        field.update(cx, |input, cx| {
            input.select_range(range, cx);
            input.put(&part, cx);
        });
        self.offers.moved = None;
        cx.notify();
    }

    /// `ui.field_reference { field?, close?, pointer? }`: open the
    /// reference of the field being typed in (or of `field`, its `?`
    /// pressed), or close the one open. The address field says its own in
    /// its card: F1 there, then its keys; its `?` turns it on and off.
    /// Nothing opens where no field with one has the keyboard.
    pub(crate) fn toggle_field_reference(
        &mut self,
        args: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let close = args.get("close").and_then(serde_json::Value::as_bool) == Some(true);
        let field = args.get("field").and_then(serde_json::Value::as_str);
        if !close && self.ui().address.read(cx).is_focused(window) {
            if field.is_some() {
                self.omnibox_reference_toggle(window, cx);
            } else {
                self.omnibox_reference(window, cx);
            }
            return;
        }
        let offer = if close {
            None
        } else {
            self.prompt_reference(field, window, cx)
                .or_else(|| self.bulk_reference(field, window, cx))
        };
        if !close && offer.is_none() {
            return;
        }
        if self.toggle_popup(POPUP, args, window, cx)
            && let Some(offer) = offer
        {
            self.toolbar_menu = Some(toolbar::Popup::new(POPUP, window, cx));
            self.field_reference = Some(offer);
        }
        cx.notify();
    }

    /// `ui.field_reference_put { group, item }`: write that item of the
    /// reference open in its field, where the caret is, or fill the
    /// fields its recipe names; the field has the keyboard again.
    pub(crate) fn field_reference_put(
        &mut self,
        args: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let place = |key: &str| {
            args.get(key)
                .and_then(serde_json::Value::as_u64)
                .and_then(|n| usize::try_from(n).ok())
        };
        let (Some(group), Some(item)) = (place("group"), place("item")) else {
            return;
        };
        if self
            .toolbar_menu
            .as_ref()
            .is_none_or(|popup| popup.kind != POPUP)
        {
            return;
        }
        let Some(open) = self.field_reference.take() else {
            return;
        };
        self.close_popup(window, cx);
        let Some(item) = open.reference.item(group, item).cloned() else {
            return;
        };
        let Some((_, field)) = open.fields.first() else {
            return;
        };
        let focus = field.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        if let Some(text) = &item.inserts {
            field.update(cx, |input, cx| input.put(text, cx));
        }
        for (key, text) in &item.fills {
            if let Some((_, input)) = open.fields.iter().find(|(named, _)| named == key) {
                input.update(cx, |input, cx| input.replace_all(text, cx));
            }
        }
        cx.notify();
    }
}

/// The reference open, hanging from its field's [?] over the dialog.
pub fn render(shell: &Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let popup = shell
        .toolbar_menu
        .as_ref()
        .filter(|popup| popup.kind == POPUP)?;
    let open = shell.field_reference.as_ref()?;
    let theme = cx.global::<Theme>().clone();
    let reference = &open.reference;
    let mut parts = Vec::new();
    for (g, group) in reference.groups.iter().enumerate() {
        parts.push(MenuPart::Title(group.title.clone().into(), None));
        for (i, item) in group.items.iter().enumerate() {
            let writes = item.inserts.is_some() || !item.fills.is_empty();
            parts.push(MenuPart::Row(
                Button::new(
                    format!("reference {g} {}", item.written),
                    "",
                    item.says.clone(),
                    "ui.field_reference_put",
                )
                .args(serde_json::json!({ "group": g, "item": i }))
                .label(item.says.clone())
                .said(item.written.clone(), item.gives.clone().map(Into::into))
                .enabled(writes),
            ));
        }
        for note in &group.notes {
            parts.push(MenuPart::Note(note.clone().into()));
        }
    }
    parts.push(MenuPart::Line);
    let keys = shell
        .keys_for("ui.shortcuts", &serde_json::Value::Null)
        .map_or_else(String::new, |keys| format!(" · {keys}: the keys here"));
    parts.push(MenuPart::Note(
        format!("Chosen, it is written where the caret is{keys}").into(),
    ));
    // As wide as it says, never as wide as its longest row: its columns
    // line up.
    let panel = toolbar::menu_panel(popup, "ui.field_reference", &theme, cx)
        .w(sized(640.))
        .min_w(gpui::px(0.))
        .max_w(sized(640.))
        .when(shell.a11y, |d| d.aria_label(reference.title.clone()));
    let panel = toolbar::menu_parts(panel, parts, shell, &theme, cx);
    Some(open.anchor.popup_right_over(PRIORITY, panel))
}

/// The field's text and caret, as what it offers was worked out for.
fn typed(open: &Open, cx: &gpui::App) -> Option<Typed> {
    let (_, field) = open.fields.first()?;
    let input = field.read(cx);
    Some((field.entity_id(), input.text().to_owned(), input.caret()))
}

/// What the field being typed in offers, under it: each item as written,
/// what it means and what it gives, the one lit drawn chosen. The field
/// keeps the keyboard; a press on a row takes it.
pub fn render_offers(
    shell: &Shell,
    window: &Window,
    cx: &mut Context<Shell>,
) -> Option<AnyElement> {
    let shown = shell.offers_now(window, cx)?;
    let theme = cx.global::<Theme>().clone();
    let reference = &shown.open.reference;
    let mut list = div()
        .id("field-offers")
        .w(sized(520.))
        .p_1()
        .flex()
        .flex_col()
        .rounded_lg()
        .border_1()
        .border_color(theme.border)
        .bg(theme.surface)
        .shadow_md()
        .occlude()
        .text_body()
        .whitespace_nowrap()
        // A press here is the list's, never the dialog's behind it.
        .on_mouse_down(gpui::MouseButton::Left, |_, window, cx| {
            cx.stop_propagation();
            window.prevent_default();
        });
    for (row, &(group, at)) in shown.offered.items.iter().enumerate() {
        let Some(item) = reference.item(group, at) else {
            continue;
        };
        let highlighted = row == shown.lit;
        let hover = theme.hover;
        list = list.child(
            div()
                .id(crate::elements::known_by("field-offer", &item.written))
                .h(sized(toolbar::MENU_ROW))
                .px_2()
                .flex()
                .items_center()
                .gap_2()
                .rounded_md()
                .cursor_pointer()
                .when(highlighted, |d| d.chosen(&theme))
                .when(!highlighted, move |d| d.hover(move |s| s.bg(hover)))
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        window.prevent_default();
                        this.run(
                            "ui.field_complete",
                            serde_json::json!({ "group": group, "item": at }),
                            window,
                            cx,
                        );
                    }),
                )
                .child(
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
                        .child(item.says.clone()),
                )
                .children(item.gives.clone().map(|gives| {
                    div()
                        .flex_none()
                        .max_w(gpui::relative(0.3))
                        .pl_2()
                        .truncate()
                        .text_small()
                        .font_family(theme.code_font.clone())
                        .text_color(theme.text_muted)
                        .child(gives)
                })),
        );
    }
    Some(PopoverAnchor::popup_first(
        std::slice::from_ref(&shown.open.anchor),
        PRIORITY,
        list.into_any_element(),
    ))
}
