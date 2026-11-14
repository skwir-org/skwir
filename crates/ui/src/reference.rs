//! What a dialog's field takes, said beside it (ADR-137): its reference,
//! opened from the [?] at the field's end or F1 in it, hanging over the
//! dialog from the [?].
//!
//! - The owner of the field's language says it as data
//!   (`skwir_core::reference::Reference`): the bulk rename's plugin by
//!   its command (`rename_bulk.reference`, asked with each preview, so
//!   what each placeholder gives is the first item's as the dialog
//!   stands), a question by `ui.ask`'s `reference`. The window draws it
//!   as a menu (`toolbar::MenuPart`): each group under its title, each
//!   item a row (`Button::said`): as written, what it means, what it
//!   gives here; the group's notes under it.
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
//! - The address field says its own in its card (`Typed::Help`, F1 or
//!   `?`): taking the keyboard from the field would end what is typed.

use std::rc::Rc;

use gpui::{AnyElement, Context, Entity, Focusable as _, SharedString, Window, prelude::*};
use skwir_core::reference::Reference;

use crate::elements::popover::PopoverAnchor;
use crate::input::TextInput;
use crate::shell::Shell;
use crate::sizes::sized;
use crate::theme::Theme;
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
    /// The [?] it hangs from.
    pub anchor: PopoverAnchor,
}

/// A field's [?]: its reference, opened by a press, as F1 in the field
/// opens it; `field` is the key the dialog knows the field by.
pub(crate) fn button(
    id: impl Into<SharedString>,
    field: &str,
    anchor: &PopoverAnchor,
    shell: &Shell,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let open = shell.reference_open_from(anchor);
    Button::new(id, "help", "What it takes", "ui.field_reference")
        .args(serde_json::json!({ "field": field }))
        .active(open)
        .anchor_if(anchor, true)
        .render(shell, cx)
        .into_any_element()
}

/// As much room as a [?] takes, for a field that has none beside fields
/// that do: they end together.
pub(crate) fn room() -> AnyElement {
    gpui::div()
        .flex_none()
        .min_w(gpui::px(30.))
        .px(sized(6.))
        .child(gpui::div().size(gpui::px(16.)))
        .into_any_element()
}

impl Shell {
    /// Whether the reference open hangs from `anchor`.
    pub(crate) fn reference_open_from(&self, anchor: &PopoverAnchor) -> bool {
        self.toolbar_menu
            .as_ref()
            .is_some_and(|popup| popup.kind == POPUP)
            && self
                .field_reference
                .as_ref()
                .is_some_and(|open| open.anchor.same(anchor))
    }

    /// `ui.field_reference { field?, close?, pointer? }`: open the
    /// reference of the field being typed in (or of `field`, its [?]
    /// pressed), or close the one open. The address field says its own in
    /// its card. Nothing opens where no field with one has the keyboard.
    pub(crate) fn toggle_field_reference(
        &mut self,
        args: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let close = args.get("close").and_then(serde_json::Value::as_bool) == Some(true);
        if !close && self.ui().address.read(cx).is_focused(window) {
            return self.omnibox_reference(window, cx);
        }
        let field = args.get("field").and_then(serde_json::Value::as_str);
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
