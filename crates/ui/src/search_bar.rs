//! The search designer: a pane that shows a search draws it in its address
//! field as pills, one per facet the query plugin describes as set
//! (`query.describe`): where it looks first, then the words, the kinds of
//! file, when, size, tag, contents, and any part of the search no facet
//! holds. "+ Filter" lists the facets not set. A pill opens its choices in
//! a popover anchored to it; a choice runs the invocation the plugin gave
//! (`query.refine`, or `ui.ask` for a value to type), which moves the pane
//! to the refined search, so Back undoes a choice. A set pill has a button
//! that takes it out.
//!
//! The chips are read once per location a pane shows, never per frame, and
//! the UI knows nothing of the query language: the text stays the one
//! truth, which the address field edits as it is (Ctrl+L, or a click
//! beside the pills).

use bibli_core::command::{Invocation, Outcome};
use bibli_core::workspace::PaneId;
use gpui::{AnyElement, Context, SharedString, Window, div, prelude::*, px};
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

    /// Where the search looks: always drawn, first.
    fn is_scope(&self) -> bool {
        self.id == "where"
    }

    /// Drawn as a pill: where it looks, and every facet that is set.
    fn is_pill(&self) -> bool {
        self.is_scope() || self.value.is_some()
    }
}

/// What the query plugin says of the search a pane shows.
#[derive(Clone, Debug, Default, Deserialize)]
struct Description {
    chips: Vec<Chip>,
    #[serde(default, with = "bibli_core::os_path")]
    root: std::path::PathBuf,
    #[serde(default)]
    text: String,
}

/// The chips of the location a pane shows, read when it changed, and the
/// search cut in two for the field that edits it: where it looks, and the
/// rest.
#[derive(Default)]
pub struct Chips {
    location: String,
    pub chips: Vec<Chip>,
    pub root: std::path::PathBuf,
    pub text: String,
}

/// What a pane's search popover shows: one chip's choices, or the facets
/// not set ("+ Filter").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Chip(usize),
    Facets,
}

/// The chip whose choices are open.
pub struct ChipMenu {
    pane: PaneId,
    target: Target,
    popup: Popup,
}

impl Shell {
    /// `ui.search_chip { pane, chip }` opens a chip's choices, or closes
    /// them when they are open; `{ pane, facets: true }` lists the facets
    /// not set; `{ close: true }` closes whichever is open.
    pub fn toggle_search_chip(
        &mut self,
        args: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let close = args.get("close").and_then(serde_json::Value::as_bool) == Some(true);
        let wanted = (!close)
            .then(|| {
                let pane = serde_json::from_value::<PaneId>(args.get("pane")?.clone()).ok()?;
                let target =
                    if args.get("facets").and_then(serde_json::Value::as_bool) == Some(true) {
                        Target::Facets
                    } else {
                        Target::Chip(usize::try_from(args.get("chip")?.as_u64()?).ok()?)
                    };
                Some((pane, target))
            })
            .flatten();
        let open = self.search_menu.take().map(|menu| {
            let at = (menu.pane, menu.target);
            menu.popup.restore(window);
            at
        });
        if close {
            if let Some(open) = open {
                self.note_dismissed(args, format!("chip {open:?}"));
            }
        } else if let Some((pane, target)) = wanted
            && open != Some((pane, target))
            && !self.just_dismissed(&format!("chip {:?}", (pane, target)))
        {
            self.search_menu = Some(ChipMenu {
                pane,
                target,
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

/// Read the chips of the search the pane being rendered shows, when it
/// shows another one than they were read for; none for a folder.
pub fn sync(shell: &mut Shell, window: &mut Window) {
    let pane = shell.pane;
    let location = shell
        .app
        .pane_location(pane)
        .filter(|l| &*l.scheme == "query");
    let shown = location
        .as_ref()
        .map(ToString::to_string)
        .unwrap_or_default();
    if shell.ui().chips.location == shown {
        return;
    }
    let described = match location {
        Some(_) if shell.app.commands.get("query.describe").is_some() => shell
            .app
            .dispatch(&Invocation::with_args(
                "query.describe",
                &serde_json::json!({ "pane": pane }),
            ))
            .ok()
            .and_then(|outcome| match outcome {
                Outcome::Value(value) => serde_json::from_value::<Description>(value).ok(),
                _ => None,
            })
            .unwrap_or_default(),
        _ => Description::default(),
    };
    shell.ui_mut().chips = Chips {
        location: shown,
        chips: described.chips,
        root: described.root,
        text: described.text,
    };
    // A menu belongs to the search it was opened on.
    if shell.search_menu.as_ref().is_some_and(|m| m.pane == pane) {
        shell.close_search_chip(window);
    }
}

/// The search the pane being rendered shows, as pills for its address
/// field: where it looks, each facet set, "+ Filter", then room that edits
/// the text when clicked. `None` when there are no chips (a folder, or the
/// query plugin away): the field shows text. `anchor` goes to what the
/// open popover hangs from. No width of its own, like the trail.
pub fn pills(shell: &Shell, anchor: &PopoverAnchor, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let chips = &shell.ui().chips.chips;
    if chips.is_empty() {
        return None;
    }
    let theme = cx.global::<Theme>().clone();
    let pane = shell.pane;
    let open = shell
        .search_menu
        .as_ref()
        .filter(|m| m.pane == pane)
        .map(|m| m.target);
    // A chip without a pill opens from "+ Filter", like the list of them.
    let from_filter = match open {
        Some(Target::Facets) => true,
        Some(Target::Chip(i)) => chips.get(i).is_some_and(|c| !c.is_pill()),
        None => false,
    };
    let order = chips
        .iter()
        .enumerate()
        .filter(|(_, c)| c.is_scope())
        .chain(
            chips
                .iter()
                .enumerate()
                .filter(|(_, c)| !c.is_scope() && c.is_pill()),
        );
    let mut line = div()
        .flex_1()
        .w(px(0.))
        .min_w_0()
        .h(px(24.))
        .overflow_hidden()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(4.))
        .px(px(3.));
    for (i, chip) in order {
        let tip = if chip.title.is_empty() {
            "Part of the search no chip holds".to_owned()
        } else {
            chip.title.clone()
        };
        let pill = Button::new(
            SharedString::from(format!("chip-{i}")),
            if chip.is_scope() {
                "folder".to_owned()
            } else {
                chip.icon.clone()
            },
            tip,
            "ui.search_chip",
        )
        .args(serde_json::json!({ "pane": pane, "chip": i }))
        .label(chip.label())
        .active(!chip.is_scope())
        .anchor_if(anchor, open == Some(Target::Chip(i)))
        .render(shell, cx)
        .h(px(22.))
        .rounded_full()
        .when(chip.is_scope(), |d| d.border_color(theme.border))
        .when(chip.clear.is_some(), |d| d.pr(px(2.)));
        // A pointer's shortcut, not a focusable button: it goes with what
        // it removes, and a focused element that goes takes the keyboard
        // with it. The chip's own menu ends with Remove for the keyboard.
        let pill = match chip.clear.clone() {
            Some(clear) => {
                let hover = theme.hover;
                let font = shell.font.clone();
                let tip: SharedString = format!("Remove {}", chip.label()).into();
                pill.child(
                    div()
                        .id(SharedString::from(format!("chip-clear-{i}")))
                        .size(px(18.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover))
                        .tooltip(move |_, cx| toolbar::tooltip(tip.clone(), font.clone(), cx))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.activate_pane(pane, cx);
                            let mut args = clear.args.clone();
                            args["pane"] = serde_json::json!(pane);
                            this.run(&clear.name, args, window, cx);
                        }))
                        .child(crate::controls::glyph("close", theme.selection_text, 10.)),
                )
            }
            None => pill,
        };
        line = line.child(pill);
    }
    if chips.iter().any(|c| !c.is_pill()) {
        line = line.child(
            Button::new(
                "chip-facets",
                "plus",
                "Narrow the search by name, kind, date, size, tag or contents",
                "ui.search_chip",
            )
            .args(serde_json::json!({ "pane": pane, "facets": true }))
            .label("Filter")
            .anchor_if(anchor, from_filter)
            .active(open == Some(Target::Facets))
            .render(shell, cx)
            .h(px(22.))
            .rounded_full()
            .border_color(theme.border),
        );
    }
    // What is left edits the search as text, as a click beside the trail
    // edits a path.
    let rest = div()
        .id(("chip-rest", pane.0 as usize))
        .flex_1()
        .min_w(px(24.))
        .h_full()
        .cursor_text()
        .on_click(cx.listener(move |this, _, window, cx| {
            this.activate_pane(pane, cx);
            this.run("ui.focus_address", serde_json::json!({}), window, cx);
        }));
    Some(line.child(rest).into_any_element())
}

/// The open popover of the pane being rendered's search: a chip's choices,
/// or the facets not set, under what `pills` gave `anchor`.
pub fn popup(shell: &Shell, anchor: &PopoverAnchor, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let pane = shell.pane;
    let menu = shell.search_menu.as_ref().filter(|m| m.pane == pane)?;
    let chips = &shell.ui().chips.chips;
    let theme = cx.global::<Theme>().clone();
    let mut panel = toolbar::popup_panel(&menu.popup, "ui.search_chip", &theme, cx);
    match menu.target {
        Target::Facets => {
            for (i, chip) in chips.iter().enumerate().filter(|(_, c)| !c.is_pill()) {
                panel = panel.child(
                    Button::new(
                        SharedString::from(format!("chip-facet-{i}")),
                        chip.icon.clone(),
                        chip.title.clone(),
                        "ui.search_chip",
                    )
                    .args(serde_json::json!({ "pane": pane, "chip": i }))
                    .label(chip.title.clone())
                    .menu()
                    .render(shell, cx),
                );
            }
        }
        Target::Chip(i) => {
            let chip = chips.get(i)?;
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
        }
    }
    Some(anchor.popup(panel))
}
