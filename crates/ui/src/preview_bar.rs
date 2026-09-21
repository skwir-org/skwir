//! The one toolbar every preview fills.
//!
//! A preview never builds a row: it lists its tools, each in a slot, and this
//! module lays them out, so the same action is in the same place whatever is
//! shown. Slots, left to right: navigate (parent, previous and next, page),
//! view (zoom or text size, fit, the ways to show the same content), a spacer,
//! then find, copy and contents, then More.
//!
//! The menu rule is the application toolbar's: More holds what has no button
//! in the variant the row chose this frame (`PopoverAnchor::popup_for`), plus
//! what never has one (open externally, copy the path, a kind's own settings).
//! Nothing is both a button and a menu row, and More is always there, so it
//! never moves.
use std::rc::Rc;

use gpui::{AnyElement, Context, Div, div, prelude::*, px};

use crate::elements::{
    adaptive::{AdaptiveRow, Chosen},
    popover::PopoverAnchor,
};
use crate::shell::Shell;
use crate::theme::Theme;
use crate::toolbar::{self, Button};

/// The labels every preview uses for the actions they share.
pub mod label {
    pub const FIT: &str = "Fit";
    pub const FIT_WIDTH: &str = "Fit width";
    pub const CONTENTS: &str = "Contents";
    pub const FIND: &str = "Find · Ctrl+F";
    pub const COPY: &str = "Copy · Ctrl+C";
    pub const HEX: &str = "View as hex";
    pub const ZOOM: &str = "Zoom and fit";
    pub const MORE: &str = "More";
}

/// The popup More opens; a tool names another with [`Item::opens`].
pub const MORE: &str = "preview";

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Slot {
    Navigate,
    View,
    /// Find, copy, contents: after the spacer, in the order given.
    Tools,
}

/// A piece of a row that is not a command button: a page field, a time, a
/// seek bar. It is built once per variant and never appears in the menu.
pub type Widget = Rc<dyn Fn(&Shell, &mut Context<Shell>) -> AnyElement>;

pub enum Tool {
    Button(Button),
    Widget(Widget),
}

pub struct Item {
    slot: Slot,
    /// The narrowest variant (0 wide, 1 medium, 2 narrow) that still shows it.
    keep: usize,
    tool: Tool,
    /// The popup this button opens, which is then anchored to it.
    opens: Option<&'static str>,
    /// Takes the row's spare width itself, so the bar adds no spacer.
    fills: bool,
}

impl Item {
    pub fn button(slot: Slot, keep: usize, button: Button) -> Self {
        Item {
            slot,
            keep,
            tool: Tool::Button(button),
            opens: None,
            fills: false,
        }
    }

    pub fn widget(
        slot: Slot,
        keep: usize,
        widget: impl Fn(&Shell, &mut Context<Shell>) -> AnyElement + 'static,
    ) -> Self {
        Item {
            slot,
            keep,
            tool: Tool::Widget(Rc::new(widget)),
            opens: None,
            fills: false,
        }
    }

    pub fn opens(mut self, popup: &'static str) -> Self {
        self.opens = Some(popup);
        self
    }

    pub fn fills(mut self) -> Self {
        self.fills = true;
        self
    }
}

/// A kind's own part of More.
pub type Panel = Rc<dyn Fn(&Shell, &Theme, &mut Context<Shell>) -> Vec<AnyElement>>;
/// A popup other than More, whole: it owns its frame, focus and keys.
pub type Popup = Rc<dyn Fn(&Shell, &Theme, &mut Context<Shell>) -> Option<AnyElement>>;

pub struct Bar {
    /// One id for every variant, so a focused button keeps its focus handle
    /// (element state keyed by the id path) across a responsive reflow.
    pub id: &'static str,
    pub items: Vec<Item>,
    /// A kind's settings that never have a button (volume, speed, tracks).
    pub settings: Option<Panel>,
    /// Popups a tool of this bar opens, by name.
    pub popups: Vec<(&'static str, Popup)>,
}

impl Bar {
    pub fn new(id: &'static str) -> Self {
        Bar {
            id,
            items: Vec::new(),
            settings: None,
            popups: Vec::new(),
        }
    }

    pub fn push(&mut self, item: Item) {
        self.items.push(item);
    }
}

/// What never has a button, for every kind.
fn always(shell: &Shell, cx: &mut Context<Shell>) -> Vec<AnyElement> {
    [
        Button::new(
            "preview-external",
            "open",
            "Open externally",
            "ui.preview_open_external",
        ),
        Button::new(
            "preview-copy-path",
            "copy",
            "Copy path",
            "ui.preview_copy_path",
        ),
    ]
    .into_iter()
    .map(|button| menu_row(button, shell, cx))
    .collect()
}

/// A button as a row of the menu: its title without the shortcut hint.
fn menu_row(button: Button, shell: &Shell, cx: &mut Context<Shell>) -> AnyElement {
    let title = button.title();
    let label = title.split(" · ").next().unwrap_or(&title).to_owned();
    button
        .label(label)
        .menu()
        .render(shell, cx)
        .into_any_element()
}

fn separator(theme: &Theme) -> Div {
    div().h(px(1.)).my_1().bg(theme.border)
}

fn row(
    bar: &Bar,
    mode: usize,
    active: Option<&str>,
    anchor: &PopoverAnchor,
    shell: &Shell,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let mut row = toolbar::row(theme).id(bar.id);
    let mut anchored = false;
    let mut last = None;
    let filled = bar.items.iter().any(|item| item.fills && item.keep >= mode);
    let mut items: Vec<&Item> = bar.items.iter().filter(|item| item.keep >= mode).collect();
    items.sort_by_key(|item| item.slot);
    for item in items {
        if last.is_some_and(|slot| slot != item.slot) {
            row = if item.slot == Slot::Tools && !filled {
                row.child(div().flex_1().min_w_0())
            } else if item.slot == Slot::View {
                row.child(toolbar::separator(theme))
            } else {
                row
            };
        } else if last.is_none() && item.slot == Slot::Tools && !filled {
            row = row.child(div().flex_1().min_w_0());
        }
        last = Some(item.slot);
        row = row.child(match &item.tool {
            Tool::Button(button) => {
                let here = item.opens.is_some() && item.opens == active;
                anchored |= here;
                button
                    .clone()
                    .anchor_if(anchor, here)
                    .render(shell, cx)
                    .into_any_element()
            }
            Tool::Widget(widget) => widget(shell, cx),
        });
    }
    if last != Some(Slot::Tools) && !filled {
        row = row.child(div().flex_1().min_w_0());
    }
    row.child(
        Button::new("preview-more", "more", label::MORE, "ui.preview_menu")
            .active(active == Some(MORE))
            // A popup whose own button this variant dropped opens here.
            .anchor_if(anchor, active.is_some() && !anchored)
            .render(shell, cx),
    )
    .into_any_element()
}

/// The bar, and whichever of its popups is open.
pub fn render(
    bar: &Bar,
    active: Option<&'static str>,
    shell: &Shell,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let anchor = PopoverAnchor::default();
    let chosen = Chosen::default();
    let mut root = div().relative().flex_none().child(AdaptiveRow {
        variants: (0..3)
            .map(|mode| row(bar, mode, active, &anchor, shell, theme, cx))
            .collect(),
        height: 42.,
        chosen: Some(chosen.clone()),
    });
    let Some(active) = active else {
        return root.into_any_element();
    };
    if let Some(popup) = shell.preview.menu.as_ref().filter(|_| active == MORE) {
        // One panel per variant; the row's choice picks it, this frame.
        let panels = (0..3)
            .map(|mode| {
                let mut panel = toolbar::popup_panel(popup, "ui.preview_menu", theme, cx)
                    .max_h(px(480.))
                    .overflow_y_scroll();
                let mut dropped = false;
                for item in bar.items.iter().filter(|item| item.keep < mode) {
                    if let Tool::Button(button) = &item.tool {
                        panel = panel.child(menu_row(button.clone(), shell, cx));
                        dropped = true;
                    }
                }
                if let Some(settings) = &bar.settings {
                    if dropped {
                        panel = panel.child(separator(theme));
                    }
                    panel = panel.children(settings(shell, theme, cx));
                    dropped = true;
                }
                if dropped {
                    panel = panel.child(separator(theme));
                }
                panel.children(always(shell, cx)).into_any_element()
            })
            .collect();
        root = root.child(anchor.popup_for(&chosen, panels));
    } else if let Some(panel) = bar
        .popups
        .iter()
        .find(|(name, _)| *name == active)
        .and_then(|(_, popup)| popup(shell, theme, cx))
    {
        root = root.child(anchor.popup(panel));
    }
    root.into_any_element()
}
