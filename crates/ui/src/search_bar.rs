//! The search designer: a pane that shows a search gets a row of chips
//! under its address row, one per facet the query plugin describes
//! (`query.describe`): the words, the kinds of file, when, size, tag,
//! contents and where, then any part of the search no facet holds. A chip
//! opens its choices in a popover anchored to it; a choice runs the
//! invocation the plugin gave (`query.refine`, or `ui.ask` for a value to
//! type), which moves the pane to the refined search, so Back undoes a
//! choice. A set chip has a button that takes it out.
//!
//! The chips are read once per location a pane shows, never per frame, and
//! the UI knows nothing of the query language: the text stays the one
//! truth, which the address bar edits as it is.

use bibli_core::command::{Invocation, Outcome};
use bibli_core::workspace::PaneId;
use gpui::{AnyElement, Context, SharedString, Window, div, prelude::*};
use serde::Deserialize;

use crate::elements::popover::PopoverAnchor;
use crate::shell::Shell;
use crate::theme::Theme;
use crate::toolbar::{self, Button, Popup};

/// A facet of the search as the plugin describes it.
#[derive(Clone, Debug, Deserialize)]
pub struct Chip {
    pub id: String,
    pub title: String,
    pub value: Option<String>,
    pub icon: String,
    pub choices: Vec<Choice>,
    pub clear: Option<Invocation>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Choice {
    pub label: String,
    pub command: Invocation,
    pub active: bool,
}

impl Chip {
    /// What the chip reads: its facet, and what it is set to.
    pub fn label(&self) -> String {
        match (&self.value, self.title.is_empty()) {
            (Some(value), true) => value.clone(),
            (Some(value), false) => format!("{}: {value}", self.title),
            (None, _) => self.title.clone(),
        }
    }
}

/// The chips of the location a pane shows, read when it changed.
#[derive(Default)]
pub struct Chips {
    location: String,
    chips: Vec<Chip>,
}

/// The chip whose choices are open.
pub struct ChipMenu {
    pane: PaneId,
    chip: usize,
    popup: Popup,
}

impl Shell {
    /// `ui.search_chip { pane, chip }` opens a chip's choices, or closes
    /// them when they are open; `{ close: true }` closes them.
    pub fn toggle_search_chip(
        &mut self,
        args: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let wanted = (args.get("close").and_then(serde_json::Value::as_bool) != Some(true))
            .then(|| {
                let pane = serde_json::from_value::<PaneId>(args.get("pane")?.clone()).ok()?;
                let chip = usize::try_from(args.get("chip")?.as_u64()?).ok()?;
                Some((pane, chip))
            })
            .flatten();
        let open = self.search_menu.take().map(|menu| {
            let at = (menu.pane, menu.chip);
            menu.popup.restore(window);
            at
        });
        if let Some((pane, chip)) = wanted
            && open != Some((pane, chip))
        {
            self.search_menu = Some(ChipMenu {
                pane,
                chip,
                popup: Popup::new("search-chip", window, cx),
            });
        }
        cx.notify();
    }

    /// Close the open chip's choices, giving the focus back.
    pub fn close_search_chip(&mut self, window: &mut Window) {
        if let Some(menu) = self.search_menu.take() {
            menu.popup.restore(window);
        }
    }
}

/// The chips row of the pane being rendered, when it shows a search and
/// the query plugin describes it.
pub fn render(
    shell: &mut Shell,
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> Option<AnyElement> {
    let pane = shell.pane;
    let location = shell
        .app
        .pane_location(pane)
        .filter(|l| &*l.scheme == "query")?;
    shell.app.commands.get("query.describe")?;
    let shown = location.to_string();
    if shell.ui().chips.location != shown {
        let described = shell.app.dispatch(&Invocation::with_args(
            "query.describe",
            &serde_json::json!({ "pane": pane }),
        ));
        let chips = match described {
            Ok(Outcome::Value(value)) => {
                serde_json::from_value::<Vec<Chip>>(value["chips"].clone()).unwrap_or_default()
            }
            _ => Vec::new(),
        };
        shell.ui_mut().chips = Chips {
            location: shown,
            chips,
        };
        // A menu belongs to the search it was opened on.
        if shell.search_menu.as_ref().is_some_and(|m| m.pane == pane) {
            shell.close_search_chip(window);
        }
    }
    let chips = shell.ui().chips.chips.clone();
    if chips.is_empty() {
        return None;
    }
    let theme = cx.global::<Theme>().clone();
    let open = shell
        .search_menu
        .as_ref()
        .filter(|m| m.pane == pane)
        .map(|m| m.chip);
    let mut row = div()
        .id(("pane-search", pane.0 as usize))
        .flex_none()
        .w_full()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_1()
        .px_2()
        .py_1()
        .border_b_1()
        .border_color(theme.border)
        .bg(theme.surface);
    let mut popup = None;
    for (i, chip) in chips.iter().enumerate() {
        let anchor = PopoverAnchor::default();
        let label = chip.label();
        let tip = if chip.title.is_empty() {
            "Part of the search no chip holds".to_owned()
        } else {
            chip.title.clone()
        };
        let mut pill = div().flex().items_center().child(
            Button::new(
                SharedString::from(format!("chip-{i}")),
                chip.icon.clone(),
                tip,
                "ui.search_chip",
            )
            .args(serde_json::json!({ "pane": pane, "chip": i }))
            .label(label)
            .active(chip.value.is_some())
            .anchor_if(&anchor, open == Some(i))
            .render(shell, cx),
        );
        // A pointer's shortcut, not a focusable button: it goes with what
        // it removes, and a focused element that goes takes the keyboard
        // with it. The chip's own menu ends with Remove for the keyboard.
        if let Some(clear) = chip.clear.clone() {
            let hover = theme.hover;
            let font = shell.font.clone();
            let tip: SharedString = format!("Remove {}", chip.label()).into();
            pill = pill.child(
                div()
                    .id(SharedString::from(format!("chip-clear-{i}")))
                    .size(gpui::px(22.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .tooltip(move |_, cx| toolbar::tooltip(tip.clone(), font.clone(), cx))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.activate_pane(pane, cx);
                        let mut args = clear.args.clone();
                        args["pane"] = serde_json::json!(pane);
                        this.run(&clear.name, args, window, cx);
                    }))
                    .child(crate::controls::glyph("close", theme.text_muted, 12.)),
            );
        }
        row = row.child(pill);
        if open == Some(i)
            && let Some(menu) = &shell.search_menu
        {
            let mut panel = toolbar::popup_panel(&menu.popup, "ui.search_chip", &theme, cx);
            let remove = chip.clear.as_ref().map(|clear| Choice {
                label: "Remove".to_owned(),
                command: clear.clone(),
                active: false,
            });
            // An advanced part's only choice is already its removal.
            let remove = remove.filter(|_| !chip.title.is_empty());
            for (j, choice) in chip.choices.iter().chain(remove.iter()).enumerate() {
                panel = panel.child(
                    Button::new(
                        SharedString::from(format!("chip-choice-{j}")),
                        if choice.active { "check" } else { "" },
                        choice.label.clone(),
                        choice.command.name.to_string(),
                    )
                    .args(choice.command.args.clone())
                    .label(choice.label.clone())
                    .menu()
                    .render(shell, cx),
                );
            }
            popup = Some(anchor.popup(panel));
        }
    }
    Some(row.children(popup).into_any_element())
}
